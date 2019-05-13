mod noise_table;

use byteorder::{LittleEndian, WriteBytesExt as _};

use super::{Frequency, Noise, NoiseOscillator, NoiseUnit, NoiseWave, Pcm, PcmWaveFormat, Sample as _};
use noise_table::*;

use crate::error::Result;

const BASIC_SPS: u32 = 44100;
const BASIC_FREQUENCY: u32 = 100;
const KEY_TOP: u32 = 0x3200;

const SAMPLING_TOP: f64 = i16::max_value() as f64;

pub(super) struct NoiseBuilder {}

impl NoiseBuilder {
    pub(super) fn build(noise: &Noise, ch: u16, sps: u32, bps: u16) -> Result<Pcm> {
        assert!(ch == 1 || ch == 2);
        assert!(sps == 11025 || sps == 22050 || sps == 44100 || sps == 48000);
        assert!(bps == 8 || bps == 16);
        let smp_num = ((f64::from(noise.smp_num_44k) / (f64::from(BASIC_SPS) / f64::from(sps)))
            as u32
            * (u32::from(bps) / 8)
            * u32::from(ch)) as usize;
        let mut units: Vec<NoiseBuilderUnit> = noise
            .units
            .iter()
            .map(|unit| NoiseBuilderUnit::new(&unit, sps))
            .collect();
        let mut smp = Vec::with_capacity(smp_num);

        while smp.len() < smp_num {
            let sample_and_pans: Vec<(f64, [f64; 2])> = units
                .iter_mut()
                .map(|unit| (unit.get_sample(), unit.pan))
                .collect();
            for i in 0..ch {
                let sample = (sample_and_pans
                    .iter()
                    .fold(0.0, |acc, (sample, pan)| acc + sample * pan[i as usize]) as i32)
                    .max(i32::from(i16::min_value()))
                    .min(i32::from(i16::max_value())) as i16;
                if sps == 8 {
                    smp.write_u8(u8::from_i16(sample))?;
                } else {
                    smp.write_i16::<LittleEndian>(sample)?;
                }
            }
        }

        Ok(Pcm { fmt: PcmWaveFormat { ch, sps, bps }, smp })
    }
}

struct NoiseBuilderUnit {
    enable: bool,
    pan: [f64; 2],
    enves: Vec<NoiseBuilderPoint>,
    enve_index: usize,
    enve_mag_start: f64,
    enve_mag_margin: f64,
    enve_count: u32,
    main: NoiseBuilderOscillator,
    freq: NoiseBuilderOscillator,
    volu: NoiseBuilderOscillator,
}

impl NoiseBuilderUnit {
    fn new(unit: &NoiseUnit, sps: u32) -> Self {
        let enable = unit.enable;
        let pan = match unit.pan {
            0 => [1.0, 1.0],
            x if x < 0 => [1.0, (100.0 + f64::from(x)) / 100.0],
            x => [(100.0 + f64::from(x)) / 100.0, 1.0],
        };
        let enves = unit
            .enves
            .iter()
            .map(|enve| NoiseBuilderPoint {
                smp: (sps as i32) * enve.x / 1000,
                mag: f64::from(enve.y) / 100.0,
            })
            .collect::<Vec<NoiseBuilderPoint>>();
        let enve_index = 0;
        let enve_mag_start = 0.0;
        let enve_mag_margin = 0.0;
        let enve_count = 0;
        let main = if let Some(osc) = &unit.main {
            NoiseBuilderOscillator::new(&osc, OscillatorKind::Main, sps)
        } else {
            NoiseBuilderOscillator::empty(OscillatorKind::Main)
        };
        let freq = if let Some(osc) = &unit.freq {
            NoiseBuilderOscillator::new(&osc, OscillatorKind::Freq, sps)
        } else {
            NoiseBuilderOscillator::empty(OscillatorKind::Freq)
        };
        let volu = if let Some(osc) = &unit.volu {
            NoiseBuilderOscillator::new(&osc, OscillatorKind::Volu, sps)
        } else {
            NoiseBuilderOscillator::empty(OscillatorKind::Volu)
        };
        Self {
            enable,
            pan,
            enves,
            enve_index,
            enve_mag_start,
            enve_mag_margin,
            enve_count,
            main,
            freq,
            volu,
        }
    }

    fn get_sample(&mut self) -> f64 {
        if !self.enable {
            return 0.0;
        }

        // main
        let mut work = self.main.get_sample();

        // volume
        let vol = self.volu.get_sample();
        work *= (vol + SAMPLING_TOP) / (SAMPLING_TOP + SAMPLING_TOP);

        // envelope
        if self.enve_index < self.enves.len() {
            work *= self.enve_mag_start
                + (self.enve_mag_margin * f64::from(self.enve_count)
                    / f64::from(self.enves[self.enve_index].smp));
        } else {
            work *= self.enve_mag_start;
        }

        // increment
        let freq = self.freq.get_sample() as i32;
        self.main
            .increment(self.main.increment * f64::from(Frequency::get(freq)));
        self.freq.increment(self.freq.increment);
        self.volu.increment(self.volu.increment);

        if self.enve_index < self.enves.len() {
            self.enve_count += 1;
            let current = &self.enves[self.enve_index];
            if (self.enve_count as i32) >= current.smp {
                self.enve_count = 0;
                self.enve_mag_start = current.mag;
                self.enve_mag_margin = 0.0;
                self.enve_index += 1;
                while self.enve_index < self.enves.len() {
                    let enve = &self.enves[self.enve_index];
                    self.enve_mag_margin = enve.mag - self.enve_mag_start;
                    if enve.smp != 0 {
                        break;
                    }
                    self.enve_mag_start = enve.mag;
                    self.enve_index += 1;
                }
            }
        }

        work
    }
}

struct NoiseBuilderPoint {
    smp: i32,
    mag: f64,
}

struct NoiseBuilderOscillator {
    kind: OscillatorKind,
    wave: NoiseBuilderWave,
    rev: bool,
    increment: f64,
    volu: f64,
    offset: f64,
}

enum OscillatorKind {
    Main,
    Volu,
    Freq,
}

impl NoiseBuilderOscillator {
    fn empty(kind: OscillatorKind) -> Self {
        Self {
            kind,
            wave: NoiseBuilderWave::None,
            rev: false,
            increment: 0.0,
            volu: 0.0,
            offset: 0.0,
        }
    }

    fn new(osc: &NoiseOscillator, kind: OscillatorKind, sps: u32) -> Self {
        let wave = match &osc.wave {
            NoiseWave::None => NoiseBuilderWave::None,
            NoiseWave::Sine => NoiseBuilderWave::Raw {
                kind: RawKind::Sine,
            },
            NoiseWave::Saw => NoiseBuilderWave::Raw { kind: RawKind::Saw },
            NoiseWave::Rect => NoiseBuilderWave::Raw {
                kind: RawKind::Rect,
            },
            NoiseWave::Saw2 => NoiseBuilderWave::Raw {
                kind: RawKind::Saw2,
            },
            NoiseWave::Rect2 => NoiseBuilderWave::Raw {
                kind: RawKind::Rect2,
            },
            NoiseWave::Tri => NoiseBuilderWave::Raw { kind: RawKind::Tri },
            NoiseWave::Rect3 => NoiseBuilderWave::Raw {
                kind: RawKind::Rect3,
            },
            NoiseWave::Rect4 => NoiseBuilderWave::Raw {
                kind: RawKind::Rect4,
            },
            NoiseWave::Rect8 => NoiseBuilderWave::Raw {
                kind: RawKind::Rect8,
            },
            NoiseWave::Rect16 => NoiseBuilderWave::Raw {
                kind: RawKind::Rect16,
            },
            NoiseWave::Saw3 => NoiseBuilderWave::Raw {
                kind: RawKind::Saw3,
            },
            NoiseWave::Saw4 => NoiseBuilderWave::Raw {
                kind: RawKind::Saw4,
            },
            NoiseWave::Saw6 => NoiseBuilderWave::Raw {
                kind: RawKind::Saw6,
            },
            NoiseWave::Saw8 => NoiseBuilderWave::Raw {
                kind: RawKind::Saw8,
            },
            random => NoiseBuilderWave::init_random(random, osc.offset),
        };
        let rev = osc.rev;
        let increment = (f64::from(BASIC_SPS) / f64::from(sps))
            * (f64::from(osc.freq) / f64::from(BASIC_FREQUENCY));
        let volu = f64::from(osc.volu) / 100.0;
        let offset = match osc.wave {
            NoiseWave::Random | NoiseWave::Random2 => {
                f64::from(SMP_NUM as u32) * f64::from(osc.offset) / 100.0
            }
            _ => 0.0,
        };
        Self {
            kind,
            wave,
            rev,
            increment,
            volu,
            offset,
        }
    }

    fn get_sample(&self) -> f64 {
        let offset = self.offset as i32;
        let mut work = if let OscillatorKind::Main = self.kind {
            if offset >= 0 {
                f64::from(self.wave.get_sample(offset as u32))
            } else {
                0.0
            }
        } else {
            f64::from(self.wave.get_sample(offset as u32))
        };
        if let OscillatorKind::Freq = self.kind {
            if let NoiseBuilderWave::Raw { .. } = self.wave {
                work *= f64::from(KEY_TOP) / SAMPLING_TOP;
            }
        }

        if self.rev {
            work *= -1.0
        };
        work * self.volu
    }

    fn increment(&mut self, increment: f64) {
        let mut offset = self.offset + increment;
        if offset > f64::from(SMP_NUM as u32) {
            let temp = offset - f64::from(SMP_NUM as u32);
            offset = if temp > 0.0 { temp } else { 0.0 };
        }
        self.offset = offset;

        if let NoiseBuilderWave::Random {
            start,
            margin,
            index,
            ..
        } = &mut self.wave
        {
            let next_start = i32::from(NOISE_TABLE_RANDOM[*index]);
            let mut next_index = *index;
            if next_index >= SMP_NUM_RAND {
                next_index = 0
            }
            let next_margin = i32::from(NOISE_TABLE_RANDOM[next_index]) - next_start;

            *start = next_start;
            *margin = next_margin;
            *index = next_index;
        }
    }
}

enum NoiseBuilderWave {
    None,
    Raw {
        kind: RawKind,
    },
    Random {
        kind: RandomKind,
        start: i32,
        margin: i32,
        index: usize,
    },
}

enum RawKind {
    Sine,
    Saw,
    Rect,
    Saw2,
    Rect2,
    Tri,
    Rect3,
    Rect4,
    Rect8,
    Rect16,
    Saw3,
    Saw4,
    Saw6,
    Saw8,
}

enum RandomKind {
    Saw,  // Random
    Rect, // Random2
}

impl NoiseBuilderWave {
    fn init_random(kind: &NoiseWave, offset: f32) -> Self {
        let kind = match kind {
            NoiseWave::Random => RandomKind::Rect,
            NoiseWave::Random2 => RandomKind::Saw,
            _ => unreachable!(),
        };
        let index = (f64::from(SMP_NUM_RAND as u32) * f64::from(offset) / 100.0) as usize;
        NoiseBuilderWave::Random {
            kind,
            start: 0,
            margin: i32::from(NOISE_TABLE_RANDOM[index as usize]),
            index,
        }
    }

    fn get_sample(&self, offset: u32) -> i32 {
        match self {
            NoiseBuilderWave::None => 0,
            NoiseBuilderWave::Raw { kind } => match kind {
                RawKind::Sine => i32::from(NOISE_TABLE_SINE[offset as usize]),
                RawKind::Saw => i32::from(NOISE_TABLE_SAW[offset as usize]),
                RawKind::Rect => i32::from(NOISE_TABLE_RECT[offset as usize]),
                RawKind::Saw2 => i32::from(NOISE_TABLE_SAW2[offset as usize]),
                RawKind::Rect2 => i32::from(NOISE_TABLE_RECT2[offset as usize]),
                RawKind::Tri => i32::from(NOISE_TABLE_TRI[offset as usize]),
                RawKind::Rect3 => i32::from(NOISE_TABLE_RECT3[offset as usize]),
                RawKind::Rect4 => i32::from(NOISE_TABLE_RECT4[offset as usize]),
                RawKind::Rect8 => i32::from(NOISE_TABLE_RECT8[offset as usize]),
                RawKind::Rect16 => i32::from(NOISE_TABLE_RECT16[offset as usize]),
                RawKind::Saw3 => i32::from(NOISE_TABLE_SAW3[offset as usize]),
                RawKind::Saw4 => i32::from(NOISE_TABLE_SAW4[offset as usize]),
                RawKind::Saw6 => i32::from(NOISE_TABLE_SAW6[offset as usize]),
                RawKind::Saw8 => i32::from(NOISE_TABLE_SAW8[offset as usize]),
            },
            NoiseBuilderWave::Random {
                kind,
                start,
                margin,
                ..
            } => match kind {
                RandomKind::Rect => *start + *margin * (offset as i32) / (SMP_NUM as i32),
                RandomKind::Saw => *start,
            },
        }
    }
}
