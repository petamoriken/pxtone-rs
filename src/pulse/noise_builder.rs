mod noise_table;

use super::{
    NoiseUnit,
    NoiseOscillator,
    NoiseWave,
};
use noise_table::*;

const BASIC_SPS: u32 = 44100;
const BASIC_FREQUENCY: u32 = 100;

struct NoiseBuilderUnit {
    enable: bool,
    pan: [f64; 2],
    enves: Vec<NoiseBuilderPoint>,
    enve_index: i32,
    enve_mag_start: f64,
    enve_mag_margin: f64,
    enve_count: i32,
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
            x => [(100.0 + f64::from(x)) / 100.0, 1.0]
        };
        let enves = unit.enves.iter().map( |enve| NoiseBuilderPoint {
            smp: (sps as i32) * enve.x / 1000,
            mag: f64::from(enve.y) / 100.0,
        }).collect::<Vec<NoiseBuilderPoint>>();
        let enve_index = 0;
        let enve_mag_start = 0.0;
        let enve_mag_margin = 0.0;
        let enve_count = 0;
        let main = if let Some(osc) = &unit.main {
            NoiseBuilderOscillator::new(&osc, sps)
        } else {
            NoiseBuilderOscillator::empty()
        };
        let freq = if let Some(osc) = &unit.freq {
            NoiseBuilderOscillator::new(&osc, sps)
        } else {
            NoiseBuilderOscillator::empty()
        };
        let volu = if let Some(osc) = &unit.volu {
            NoiseBuilderOscillator::new(&osc, sps)
        } else {
            NoiseBuilderOscillator::empty()
        };
        Self { enable, pan, enves, enve_index, enve_mag_start, enve_mag_margin, enve_count, main, freq, volu }
    }
}

struct NoiseBuilderPoint {
    smp: i32,
    mag: f64,
}

struct NoiseBuilderOscillator {
    wave: NoiseBuilderWave,
    rev: bool,
    increment: f64,
    volu: f64,
    offset: f64,
}

impl NoiseBuilderOscillator {
    fn new(osc: &NoiseOscillator, sps: u32) -> Self {
        let wave = match &osc.wave {
            NoiseWave::None => NoiseBuilderWave::None,
            NoiseWave::Sine => NoiseBuilderWave::Sine,
            NoiseWave::Saw => NoiseBuilderWave::Saw,
            NoiseWave::Rect => NoiseBuilderWave::Rect,
            NoiseWave::Saw2 => NoiseBuilderWave::Saw2,
            NoiseWave::Rect2 => NoiseBuilderWave::Rect2,
            NoiseWave::Tri => NoiseBuilderWave::Tri,
            NoiseWave::Rect3 => NoiseBuilderWave::Rect3,
            NoiseWave::Rect4 => NoiseBuilderWave::Rect4,
            NoiseWave::Rect8 => NoiseBuilderWave::Rect8,
            NoiseWave::Rect16 => NoiseBuilderWave::Rect16,
            NoiseWave::Saw3 => NoiseBuilderWave::Saw3,
            NoiseWave::Saw4 => NoiseBuilderWave::Saw4,
            NoiseWave::Saw6 => NoiseBuilderWave::Saw6,
            NoiseWave::Saw8 => NoiseBuilderWave::Saw8,
            random => {
                let kind = match random {
                    NoiseWave::Random => RandomKind::Rect,
                    NoiseWave::Random2 => RandomKind::Saw,
                    _ => unreachable!()
                };
                NoiseBuilderWave::Random { kind, start: 0, index: (f64::from(SMP_NUM_RAND as i32) * f64::from(osc.offset) / 100.0) as i32 }
            },
        };
        let rev = osc.rev;
        let increment = (f64::from(BASIC_SPS) / f64::from(sps)) * (f64::from(osc.freq) / f64::from(BASIC_FREQUENCY));
        let volu = f64::from(osc.volu) / 100.0;
        let offset = match osc.wave {
            NoiseWave::Random | NoiseWave::Random2 => f64::from(SMP_NUM as i32) * f64::from(osc.offset) / 100.0,
            _ => 0.0,
        };
        Self { wave, rev, increment, volu, offset }
    }

    fn empty() -> Self {
        Self { wave: NoiseBuilderWave::None, rev: false, increment: 0.0, volu: 0.0, offset: 0.0 }
    }
}

enum NoiseBuilderWave {
    None,
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
    Random { kind: RandomKind, start: i32, index: i32 },
}

enum RandomKind {
    Saw,  // Random
    Rect, // Random2
}