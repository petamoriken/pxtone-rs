use std::f64;
use std::io::*;
use std::vec::Vec;

use num_traits::FromPrimitive;
use byteorder::{LittleEndian, ReadBytesExt};

use helper::{Point, Descriptor};


pub struct Oscillator {
    points: Vec<Point>,

    point_reso: i32,

    volume: i32,
    sample_num: i32,
}

impl Oscillator {
    pub fn get_overtone(&self, index: i32) -> f64 {
        let work = self.points.iter().fold(0.0, |acc, point| {
            let sss = 2.0 * f64::consts::PI * (point.x as f64) * (index as f64) / (self.sample_num as f64);
            acc + sss.sin() * (point.y as f64) / (point.x as f64) / 128.0
        });
        work * (self.volume as f64) / 128.0
    }

    pub fn get_coodinate(&self, index: i32) -> f64 {
        let i = self.point_reso * index / self.sample_num;
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
                (y1 as f64) / 128.0
            },
            n => {
                let w = x2 - x1;
                let h = y2 - y1;
                (y1 as f64) + (h as f64) * (n as f64) / (w as f64) / 128.0
            },
        };
        work * (self.volume as f64) / 128.0
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

struct NoiseOscillator {
    wave_type: NoiseWaveType,
    rev: bool,
    freq: f32,
    volume: f32,
    offset: f32,
}

impl NoiseOscillator {
    pub fn new<T: AsRef<[u8]>>(bytes: &mut Cursor<T>) -> Result<Self> {
        let wave_type = NoiseWaveType::from_i32(bytes.read_i32_flex()?).unwrap();
        let rev = bytes.read_u32_flex()? != 0;
        let freq = bytes.read_f32_flex()? / 10.0;
        let volume = bytes.read_f32_flex()? / 10.0;
        let offset = bytes.read_f32_flex()? / 10.0;
        Ok(Self { wave_type, rev, freq, volume, offset })
    }
}


// const UNIT_FLAG_XX1: u32 = 0x0001;
// const UNIT_FLAG_XX2: u32 = 0x0002;
const UNIT_FLAG_ENVELOPE: u32 = 0x0004;
const UNIT_FLAG_PAN: u32 = 0x0008;
const UNIT_FLAG_OSC_MAIN: u32 = 0x0010;
const UNIT_FLAG_OSC_FREQ: u32 = 0x0020;
const UNIT_FLAG_OSC_VOLU: u32 = 0x0040;
// const UNIT_FLAG_OSC_PAN: u32 = 0x0080;
const UNIT_FLAG_UNCOVERED: u32 = 0xffffff83;

const UNIT_MAX_ENVELOPE_NUM: u32 = 3;

struct NoiseUnit {
    enable: bool,
    enves: Vec<Point>,
    pan: i32,
    main: Option<NoiseOscillator>,
    freq: Option<NoiseOscillator>,
    volu: Option<NoiseOscillator>,
}

impl NoiseUnit {
    pub fn new<T: AsRef<[u8]>>(bytes: &mut Cursor<T>) -> Result<Self> {
        let enable = true;

        let flags = bytes.read_u32_flex()?;
        assert_eq!(flags & UNIT_FLAG_UNCOVERED, 0);

        // envelope
        let enves = if flags & UNIT_FLAG_ENVELOPE != 0 {
            let enve_num = bytes.read_u32_flex()?;
            assert!(enve_num <= UNIT_MAX_ENVELOPE_NUM);

            let mut enves = Vec::with_capacity(enve_num as usize);
            for _ in 0..enve_num {
                enves.push(Point { x: bytes.read_i32_flex()?, y: bytes.read_i32_flex()? });
            }
            enves
        } else {
            Vec::with_capacity(0)
        };

        // pan
        let pan = if flags & UNIT_FLAG_PAN != 0 {
            bytes.read_i8()?
        } else {
            0
        } as i32;

        // oscillator
        let main = if flags & UNIT_FLAG_OSC_MAIN != 0 {
            Some(NoiseOscillator::new(bytes)?)
        } else {
            None
        };
        let freq = if flags & UNIT_FLAG_OSC_FREQ != 0 {
            Some(NoiseOscillator::new(bytes)?)
        } else {
            None
        };
        let volu = if flags & UNIT_FLAG_OSC_VOLU != 0 {
            Some(NoiseOscillator::new(bytes)?)
        } else {
            None
        };

        Ok(Self { enable, enves, pan, main, freq, volu })
    }
}


const NOISE_CODE: &'static[u8] = b"PTNOISE-";
const NOISE_VERSION: u32 = 20120418;

const NOISE_MAX_UNIT_NUM: u8 = 4;

pub struct Noise {
    smp_num: u32,
    units: Vec<NoiseUnit>,
}

impl Noise {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        let mut bytes = Cursor::new(bytes);

        // signature
        let mut code = [0; 8];
        bytes.read(&mut code)?;
        assert_eq!(code, NOISE_CODE);

        let version = bytes.read_u32::<LittleEndian>()?;
        assert!(version <= NOISE_VERSION);

        let smp_num = bytes.read_u32_flex()?;

        let unit_num = bytes.read_u8()?;
        assert!(unit_num <= NOISE_MAX_UNIT_NUM);

        let mut units = Vec::with_capacity(unit_num as usize);
        for _ in 0..unit_num {
            units.push(NoiseUnit::new(&mut bytes)?);
        }

        Ok(Self { smp_num, units })
    }
}
