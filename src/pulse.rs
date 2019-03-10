use std::{f64, io::{Cursor, Read, Result}, vec::Vec};

use num_traits::FromPrimitive;

use byteorder::{LittleEndian, ReadBytesExt as _};
use crate::descriptor::ReadBytesExt as _;

mod noise_table;
use self::noise_table::*;

pub(crate) struct Noise {
    smp_num_44k: u32,
    units: Vec<NoiseUnit>,
}

static NOISE_CODE: &[u8] = b"PTNOISE-";
const NOISE_VERSION: u32 = 2012_0418;
const NOISE_MAX_UNIT_NUM: u8 = 4;
const NOISE_LIMIT_SMP_NUM: u32 = 48000 * 10;

impl Noise {
    pub fn new<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        let mut bytes = Cursor::new(bytes);

        // signature
        let mut code = [0; 8];
        bytes.read_exact(&mut code)?;
        assert_eq!(code, NOISE_CODE);

        let version = bytes.read_u32::<LittleEndian>()?;
        assert!(version <= NOISE_VERSION);

        let smp_num_44k = bytes.read_u32_flex()?.min(NOISE_LIMIT_SMP_NUM);

        let unit_num = bytes.read_u8()?;
        assert!(unit_num <= NOISE_MAX_UNIT_NUM);

        let mut units = Vec::with_capacity(unit_num as usize);
        for _ in 0..unit_num {
            units.push(NoiseUnit::new(&mut bytes)?);
        }

        Ok(Self { smp_num_44k, units })
    }

    // pub fn build(&self, ch: u32, sps: u32, bps: u32) -> Vec<u8> {
    //     let smp_num = self.smp_num_44k / 44100 / sps;

    //     let mut work = 0.0;

    //     Vec::new()
    // }
}

struct NoiseUnit {
    enable: bool,
    enves: Vec<Point>,
    pan: i8,
    main: Option<NoiseOscillator>,
    freq: Option<NoiseOscillator>,
    volu: Option<NoiseOscillator>,
}

// const NOISE_UNIT_FLAG_XX1: u32 = 0x0001;
// const NOISE_UNIT_FLAG_XX2: u32 = 0x0002;
const NOISE_UNIT_FLAG_ENVELOPE: u32 = 0x0004;
const NOISE_UNIT_FLAG_PAN: u32 = 0x0008;
const NOISE_UNIT_FLAG_OSC_MAIN: u32 = 0x0010;
const NOISE_UNIT_FLAG_OSC_FREQ: u32 = 0x0020;
const NOISE_UNIT_FLAG_OSC_VOLU: u32 = 0x0040;
// const NOISE_UNIT_FLAG_OSC_PAN: u32 = 0x0080;
const NOISE_UNIT_FLAG_UNCOVERED: u32 = 0xffff_ff83;
const NOISE_UNIT_MAX_ENVELOPE_NUM: u32 = 3;
const NOISE_UNIT_LIMIT_ENVE_X: i32 = 1000 * 10;
const NOISE_UNIT_LIMIT_ENVE_Y: i32 = 100;

impl NoiseUnit {
    fn new<T: AsRef<[u8]>>(bytes: &mut Cursor<T>) -> Result<Self> {
        let enable = true;

        let flags = bytes.read_u32_flex()?;
        assert_eq!(flags & NOISE_UNIT_FLAG_UNCOVERED, 0);

        // envelope
        let enves = if flags & NOISE_UNIT_FLAG_ENVELOPE != 0 {
            let enve_num = bytes.read_u32_flex()?;
            assert!(enve_num <= NOISE_UNIT_MAX_ENVELOPE_NUM);

            let mut enves = Vec::with_capacity(enve_num as usize);
            for _ in 0..enve_num {
                enves.push(Point { x: bytes.read_i32_flex()?.max(0).min(NOISE_UNIT_LIMIT_ENVE_X), y: bytes.read_i32_flex()?.max(0).min(NOISE_UNIT_LIMIT_ENVE_Y) });
            }
            enves
        } else {
            Vec::with_capacity(0)
        };

        // pan
        let pan = if flags & NOISE_UNIT_FLAG_PAN != 0 {
            bytes.read_i8()?
        } else {
            0
        };

        // oscillator
        let main = if flags & NOISE_UNIT_FLAG_OSC_MAIN != 0 {
            Some(NoiseOscillator::new(bytes)?)
        } else {
            None
        };
        let freq = if flags & NOISE_UNIT_FLAG_OSC_FREQ != 0 {
            Some(NoiseOscillator::new(bytes)?)
        } else {
            None
        };
        let volu = if flags & NOISE_UNIT_FLAG_OSC_VOLU != 0 {
            Some(NoiseOscillator::new(bytes)?)
        } else {
            None
        };

        Ok(Self { enable, enves, pan, main, freq, volu })
    }
}

struct NoiseOscillator {
    wave_type: NoiseWaveType,
    rev: bool,
    freq: f32,
    volume: f32,
    offset: f32,
}

const NOISE_OSC_LIMIT_FREQ: f32 = 44100.0;
const NOISE_OSC_LIMIT_VOLU: f32 = 200.0;
const NOISE_OSC_LIMIT_OFFSET: f32 = 100.0;

impl NoiseOscillator {
    fn new<T: AsRef<[u8]>>(bytes: &mut Cursor<T>) -> Result<Self> {
        let wave_type = NoiseWaveType::from_i32(bytes.read_i32_flex()?).unwrap();
        let rev = bytes.read_u32_flex()? != 0;
        let freq = (bytes.read_f32_flex()? / 10.0).max(0.0).min(NOISE_OSC_LIMIT_FREQ);
        let volume = (bytes.read_f32_flex()? / 10.0).max(0.0).min(NOISE_OSC_LIMIT_VOLU);
        let offset = (bytes.read_f32_flex()? / 10.0).max(0.0).min(NOISE_OSC_LIMIT_OFFSET);
        Ok(Self { wave_type, rev, freq, volume, offset })
    }
}

#[derive(FromPrimitive)]
enum NoiseWaveType {
    None,
    Sine,
    Saw,
    Rect,
    Random,
    Saw2,
    Rect2,

    Tri,
    Random2,
    Rect3,
    Rect4,
    Rect8,
    Rect16,
    Saw3,
    Saw4,
    Saw6,
    Saw8,
}

struct Point {
    x: i32,
    y: i32,
}




pub(crate) struct Oscillator {
    points: Vec<Point>,

    volu: i32,
    smp_num: usize,

    point_reso: i32,
}

impl Oscillator {
    fn get_overtone(&self, index: usize) -> f64 {
        let work = self.points.iter().fold(0.0, |acc, point| {
            let sss = 2.0 * f64::consts::PI * f64::from(point.x) * f64::from(index as u32) / f64::from(self.smp_num as u32);
            acc + sss.sin() * f64::from(point.y) / f64::from(point.x) / 128.0
        });
        work * f64::from(self.volu) / 128.0
    }

    fn get_coodinate(&self, index: usize) -> f64 {
        let i = self.point_reso * (index as i32) / (self.smp_num as i32);
        let current = self.points.iter().position(|point| point.x <= i);

        let x1;
        let y1;
        let x2;
        let y2;

        match current {
            Some(0) => {
                let first = &self.points.first().unwrap();
                x1 = first.x;
                y1 = first.y;
                x2 = first.x;
                y2 = first.y;
            }
            Some(c) => {
                let first = &self.points[c-1];
                let second = &self.points[c];
                x1 = first.x;
                y1 = first.y;
                x2 = second.x;
                y2 = second.y;
            },
            None => {
                let first = self.points.first().unwrap();
                let last = self.points.last().unwrap();
                x1 = last.x;
                y1 = last.y;
                x2 = self.point_reso;
                y2 = first.y;
            },
        }

        let work = match i - x1 {
            0 => {
                f64::from(y1) / 128.0
            },
            n => {
                let w = x2 - x1;
                let h = y2 - y1;
                f64::from(y1) + f64::from(h) * f64::from(n) / f64::from(w) / 128.0
            },
        };
        work * f64::from(self.volu) / 128.0
    }
}



struct WaveFormat {
    format_id: u16,
    ch: u16,
    sps: u16,
    byte_per_sec: u16,
    bps: u16,
    ext: u16,
}

pub(crate) struct PCM {
    ch: i32,
    sps: i32,
    bps: i32,
    smp: [u8],
}

// impl PCM {
//     fn new<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {

//     }
// }
