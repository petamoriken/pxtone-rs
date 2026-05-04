use crate::error::PxtoneError;
use crate::pulse::frequency::FrequencyTable;
use crate::pulse::noise::{Noise, NoiseOscillator, WAVETYPE_COUNT, WaveType};
use crate::pulse::oscillator::{Oscillator, Point};
use crate::pulse::pcm::Pcm;

const BASIC_SAMPLE_RATE: f64 = 44100.0;
const BASIC_FREQUENCY: f64 = 100.0;
const SAMPLING_TOP: i16 = 32767;
const SMP_COUNT_RAND: usize = 44100;
const SMP_COUNT: usize = (BASIC_SAMPLE_RATE / BASIC_FREQUENCY) as usize; // 441

// ---- PRNG ----
struct Rand {
  buf: [u16; 2],
}
impl Rand {
  fn new() -> Self {
    Self {
      buf: [0x4444, 0x8888],
    }
  }
  fn get(&mut self) -> i16 {
    let w1 = (self.buf[0] as i32).wrapping_add(self.buf[1] as i32);
    let b = w1.to_le_bytes();
    let w2 = i32::from_le_bytes([b[1], b[0], 0, 0]);
    self.buf[1] = self.buf[0];
    self.buf[0] = w2 as u16;
    w2 as i16
  }
}

// ---- Internal oscillator state ----
#[derive(Clone)]
struct OscState {
  increment: f64,
  offset: f64,
  volume: f64,
  wave_type: WaveType,
  reversed: bool,
  rdm_start: i16,
  rdm_margin: i32,
  rdm_index: usize,
}

impl OscState {
  fn from_design(osc: &NoiseOscillator, sample_rate: u32, rand_tbl: &[i16]) -> Self {
    let ran = matches!(osc.wave_type, WaveType::Random | WaveType::Random2);
    let increment =
      (BASIC_SAMPLE_RATE / sample_rate as f64) * (osc.frequency as f64 / BASIC_FREQUENCY);
    let offset = if ran {
      0.0
    } else {
      SMP_COUNT as f64 * (osc.offset as f64 / 100.0)
    };
    let volume = osc.volume as f64 / 100.0;
    let (rdm_start, rdm_margin, rdm_index) = if ran && !rand_tbl.is_empty() {
      let idx =
        ((SMP_COUNT_RAND as f64 * osc.offset as f64 / 100.0) as usize).min(SMP_COUNT_RAND - 1);
      let start = rand_tbl[idx];
      let margin = rand_tbl[(idx + 1).min(SMP_COUNT_RAND - 1)] as i32 - start as i32;
      (start, margin, idx)
    } else {
      (0, 0, 0)
    };
    OscState {
      increment,
      offset,
      volume,
      wave_type: osc.wave_type,
      reversed: osc.reversed,
      rdm_start,
      rdm_margin,
      rdm_index,
    }
  }

  fn get_sample(&self, tables: &[Option<Vec<i16>>; WAVETYPE_COUNT]) -> f64 {
    let tbl = tables[self.wave_type as usize].as_deref().unwrap_or(&[]);
    if tbl.is_empty() {
      return 0.0;
    }
    let offset = self.offset as usize;
    let work = match self.wave_type {
      WaveType::Random => {
        self.rdm_start as f64 + self.rdm_margin as f64 * offset as f64 / SMP_COUNT as f64
      }
      WaveType::Random2 => self.rdm_start as f64,
      _ => {
        let idx = offset.min(tbl.len() - 1);
        tbl[idx] as f64
      }
    };
    let work = if self.reversed { -work } else { work };
    work * self.volume
  }

  fn increment(&mut self, inc: f64, rand_tbl: &[i16]) {
    self.offset += inc;
    if self.offset > SMP_COUNT as f64 {
      self.offset -= SMP_COUNT as f64;
      if self.offset >= SMP_COUNT as f64 {
        self.offset = 0.0;
      }
      if matches!(self.wave_type, WaveType::Random | WaveType::Random2) {
        self.rdm_start = rand_tbl[self.rdm_index];
        self.rdm_index = (self.rdm_index + 1) % SMP_COUNT_RAND;
        self.rdm_margin = rand_tbl[self.rdm_index] as i32 - self.rdm_start as i32;
      }
    }
  }
}

// ---- Unit state ----
struct UnitState {
  enabled: bool,
  pan: [f64; 2],
  enves: Vec<(u32, f64)>, // (smp, mag)
  enve_index: usize,
  enve_mag_start: f64,
  enve_mag_margin: f64,
  enve_count: u32,
  main: OscState,
  frequency: OscState,
  volume: OscState,
}

// ---- NoiseBuilder ----

pub(crate) struct NoiseBuilder {
  tables: [Option<Vec<i16>>; WAVETYPE_COUNT],
}

impl NoiseBuilder {
  pub(crate) fn new() -> Self {
    Self {
      tables: std::array::from_fn(|_| None),
    }
  }

  /// Builds the wave table for `wave_type` if not already built.
  /// `WaveType::Random2` is left empty intentionally (returns 0 in get_sample,
  /// matching the original C++ behaviour where it shares the Random pointer
  /// but is used differently).
  fn build_table(&mut self, wave_type: WaveType) {
    if matches!(wave_type, WaveType::Random2) {
      return;
    }
    let idx = wave_type as usize;
    if self.tables[idx].is_some() {
      return;
    }
    let mut osci = Oscillator::new();
    let table: Vec<i16> = match wave_type {
      WaveType::None => vec![0i16; SMP_COUNT],
      WaveType::Sine => {
        osci.ready_get_sample(vec![Point { x: 1, y: 128 }], 128, SMP_COUNT as u32, 0);
        (0..SMP_COUNT as u32)
          .map(|s| (osci.get_one_sample_overtone(s).clamp(-1.0, 1.0) * SAMPLING_TOP as f64) as i16)
          .collect()
      }
      WaveType::Saw => {
        let top2 = (SAMPLING_TOP as i32 * 2) as f64;
        (0..SMP_COUNT)
          .map(|s| (SAMPLING_TOP as f64 - top2 * s as f64 / SMP_COUNT as f64) as i16)
          .collect()
      }
      WaveType::Rect => {
        let half = SMP_COUNT / 2;
        (0..SMP_COUNT)
          .map(|s| {
            if s < half {
              SAMPLING_TOP
            } else {
              -SAMPLING_TOP
            }
          })
          .collect()
      }
      WaveType::Random => {
        let mut rng = Rand::new();
        (0..SMP_COUNT_RAND).map(|_| rng.get()).collect()
      }
      WaveType::Saw2 => {
        osci.ready_get_sample(
          (1..=16).map(|i| Point { x: i, y: 128 }).collect(),
          128,
          SMP_COUNT as u32,
          0,
        );
        (0..SMP_COUNT as u32)
          .map(|s| (osci.get_one_sample_overtone(s).clamp(-1.0, 1.0) * SAMPLING_TOP as f64) as i16)
          .collect()
      }
      WaveType::Rect2 => {
        osci.ready_get_sample(
          (0..8)
            .map(|i| Point {
              x: 1 + i * 2,
              y: 128,
            })
            .collect(),
          128,
          SMP_COUNT as u32,
          0,
        );
        (0..SMP_COUNT as u32)
          .map(|s| (osci.get_one_sample_overtone(s).clamp(-1.0, 1.0) * SAMPLING_TOP as f64) as i16)
          .collect()
      }
      WaveType::Tri => {
        let n = SMP_COUNT as i32;
        osci.ready_get_sample(
          vec![
            Point { x: 0, y: 0 },
            Point { x: n / 4, y: 128 },
            Point {
              x: n * 3 / 4,
              y: -128,
            },
            Point { x: n, y: 0 },
          ],
          128,
          SMP_COUNT as u32,
          SMP_COUNT as u32,
        );
        (0..SMP_COUNT as u32)
          .map(|s| {
            (osci.get_one_sample_coordinate(s).clamp(-1.0, 1.0) * SAMPLING_TOP as f64) as i16
          })
          .collect()
      }
      WaveType::Rect3 => {
        let t3 = SMP_COUNT / 3;
        (0..SMP_COUNT)
          .map(|s| if s < t3 { SAMPLING_TOP } else { -SAMPLING_TOP })
          .collect()
      }
      WaveType::Rect4 => {
        let t4 = SMP_COUNT / 4;
        (0..SMP_COUNT)
          .map(|s| if s < t4 { SAMPLING_TOP } else { -SAMPLING_TOP })
          .collect()
      }
      WaveType::Rect8 => {
        let t8 = SMP_COUNT / 8;
        (0..SMP_COUNT)
          .map(|s| if s < t8 { SAMPLING_TOP } else { -SAMPLING_TOP })
          .collect()
      }
      WaveType::Rect16 => {
        let t16 = SMP_COUNT / 16;
        (0..SMP_COUNT)
          .map(|s| if s < t16 { SAMPLING_TOP } else { -SAMPLING_TOP })
          .collect()
      }
      WaveType::Saw3 => {
        let t1 = SMP_COUNT / 3;
        let t2 = SMP_COUNT * 2 / 3;
        (0..SMP_COUNT)
          .map(|s| {
            if s < t1 {
              SAMPLING_TOP
            } else if s < t2 {
              0
            } else {
              -SAMPLING_TOP
            }
          })
          .collect()
      }
      WaveType::Saw4 => {
        let a1 = SMP_COUNT / 4;
        let a2 = SMP_COUNT * 2 / 4;
        let a3 = SMP_COUNT * 3 / 4;
        (0..SMP_COUNT)
          .map(|s| {
            if s < a1 {
              SAMPLING_TOP
            } else if s < a2 {
              SAMPLING_TOP / 3
            } else if s < a3 {
              -(SAMPLING_TOP / 3)
            } else {
              -SAMPLING_TOP
            }
          })
          .collect()
      }
      WaveType::Saw6 => {
        let seg6 = [
          SAMPLING_TOP,
          (SAMPLING_TOP as i32 - SAMPLING_TOP as i32 * 2 / 5) as i16,
          (SAMPLING_TOP / 5),
          -(SAMPLING_TOP / 5),
          (-(SAMPLING_TOP as i32) + SAMPLING_TOP as i32 * 2 / 5) as i16,
          -SAMPLING_TOP,
        ];
        (0..SMP_COUNT)
          .map(|s| seg6[(s * 6 / SMP_COUNT).min(5)])
          .collect()
      }
      WaveType::Saw8 => {
        let seg8 = [
          SAMPLING_TOP,
          (SAMPLING_TOP as i32 - SAMPLING_TOP as i32 * 2 / 7) as i16,
          (SAMPLING_TOP as i32 - SAMPLING_TOP as i32 * 4 / 7) as i16,
          SAMPLING_TOP / 7,
          -SAMPLING_TOP / 7,
          (-SAMPLING_TOP as i32 + SAMPLING_TOP as i32 * 4 / 7) as i16,
          (-SAMPLING_TOP as i32 + SAMPLING_TOP as i32 * 2 / 7) as i16,
          -SAMPLING_TOP,
        ];
        (0..SMP_COUNT)
          .map(|s| seg8[(s * 8 / SMP_COUNT).min(7)])
          .collect()
      }
      WaveType::Random2 => unreachable!(),
    };
    self.tables[idx] = Some(table);
  }

  /// Generates PCM from a Noise design
  pub(crate) fn build_noise(
    &mut self,
    noise: &mut Noise,
    channels: u8,
    sample_rate: u32,
    bits_per_sample: u8,
    frequency: &FrequencyTable,
  ) -> Result<Pcm, PxtoneError> {
    noise.fix();

    // Build only the tables required by this Noise design
    for unit in &noise.units {
      if unit.enabled {
        self.build_table(unit.main.wave_type);
        self.build_table(unit.frequency.wave_type);
        self.build_table(unit.volume.wave_type);
      }
    }

    let rand_tbl = self.tables[WaveType::Random as usize]
      .as_deref()
      .unwrap_or(&[]);
    let frame_count = (noise.frame_count_44k as f64 / (44100.0 / sample_rate as f64)) as u32;

    // Build unit states
    let mut units: Vec<UnitState> = noise
      .units
      .iter()
      .map(|du| {
        let pan = if du.pan == 0 {
          [1.0, 1.0]
        } else if du.pan < 0 {
          [1.0, (100.0 + du.pan as f64) / 100.0]
        } else {
          [(100.0 - du.pan as f64) / 100.0, 1.0]
        };

        let enves: Vec<(u32, f64)> = du
          .envelopes
          .iter()
          .map(|e| (sample_rate * e.x / 1000, e.y as f64 / 100.0))
          .collect();

        let mut enve_index = 0usize;
        let mut enve_mag_start = 0.0f64;
        let mut enve_mag_margin = 0.0f64;
        while enve_index < enves.len() {
          enve_mag_margin = enves[enve_index].1 - enve_mag_start;
          if enves[enve_index].0 != 0 {
            break;
          }
          enve_mag_start = enves[enve_index].1;
          enve_index += 1;
        }

        UnitState {
          enabled: du.enabled,
          pan,
          enves,
          enve_index,
          enve_mag_start,
          enve_mag_margin,
          enve_count: 0,
          main: OscState::from_design(&du.main, sample_rate, rand_tbl),
          frequency: OscState::from_design(&du.frequency, sample_rate, rand_tbl),
          volume: OscState::from_design(&du.volume, sample_rate, rand_tbl),
        }
      })
      .collect();

    let mut pcm = Pcm::create(channels, sample_rate, bits_per_sample, frame_count)?;
    let buf = pcm.samples_mut();
    let mut buf_pos = 0usize;

    for _ in 0..frame_count as usize {
      for c in 0..channels as usize {
        let store: f64 = units
          .iter()
          .filter(|u| u.enabled)
          .map(|u| {
            let main = u.main.get_sample(&self.tables);
            let vol = u.volume.get_sample(&self.tables);
            let work = main * (vol + SAMPLING_TOP as f64) / (SAMPLING_TOP as f64 * 2.0) * u.pan[c];
            let envelope = if u.enve_index < u.enves.len() {
              let smp = u.enves[u.enve_index].0;
              if smp > 0 {
                u.enve_mag_start + u.enve_mag_margin * u.enve_count as f64 / smp as f64
              } else {
                u.enve_mag_start
              }
            } else {
              u.enve_mag_start
            };
            work * envelope
          })
          .sum();
        let byte4 = (store as i32).clamp(-SAMPLING_TOP as i32, SAMPLING_TOP as i32);
        if bits_per_sample == 8 {
          buf[buf_pos] = ((byte4 >> 8) + 128) as u8;
          buf_pos += 1;
        } else {
          let bytes = (byte4 as i16).to_le_bytes();
          buf[buf_pos] = bytes[0];
          buf[buf_pos + 1] = bytes[1];
          buf_pos += 2;
        }
      }

      // increment all oscillators
      for u in units.iter_mut() {
        if !u.enabled {
          continue;
        }
        // freq → fre
        let fre = {
          let po = &u.frequency;

          po.get_sample(&self.tables) // already scaled by volume in get_sample
        };
        let main_inc = u.main.increment * frequency.get(fre as i32) as f64;
        u.main.increment(main_inc, rand_tbl);
        let freq_inc = u.frequency.increment;
        u.frequency.increment(freq_inc, rand_tbl);
        let volu_inc = u.volume.increment;
        u.volume.increment(volu_inc, rand_tbl);

        // envelope
        if u.enve_index < u.enves.len() {
          u.enve_count += 1;
          let smp = u.enves[u.enve_index].0;
          if u.enve_count >= smp {
            u.enve_count = 0;
            u.enve_mag_start = u.enves[u.enve_index].1;
            u.enve_mag_margin = 0.0;
            u.enve_index += 1;
            while u.enve_index < u.enves.len() {
              u.enve_mag_margin = u.enves[u.enve_index].1 - u.enve_mag_start;
              if u.enves[u.enve_index].0 != 0 {
                break;
              }
              u.enve_mag_start = u.enves[u.enve_index].1;
              u.enve_index += 1;
            }
          }
        }
      }
    }

    Ok(pcm)
  }
}
