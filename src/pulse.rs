use std::{f64, io::{Read, Seek, SeekFrom, Result}, vec::Vec};

use num_traits::FromPrimitive;

use byteorder::{LittleEndian, ReadBytesExt as _};
use crate::descriptor::ReadBytesExt as _;

mod noise_table;
use self::noise_table::*;

pub(crate) struct Noise {
    units: Vec<NoiseUnit>,
    smp_num_44k: u32,
}

static NOISE_CODE: &[u8] = b"PTNOISE-";
const NOISE_VERSION: u32 = 2012_0418;
const NOISE_MAX_UNIT_NUM: u8 = 4;
const NOISE_LIMIT_SMP_NUM: u32 = 48000 * 10;

impl Noise {
    pub fn new<T: Read + Seek>(mut bytes: T) -> Result<Self> {
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
    fn new<T: Read + Seek>(bytes: &mut T) -> Result<Self> {
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
    volu: f32,
    offset: f32,
}

const NOISE_OSC_LIMIT_FREQ: f32 = 44100.0;
const NOISE_OSC_LIMIT_VOLU: f32 = 200.0;
const NOISE_OSC_LIMIT_OFFSET: f32 = 100.0;

impl NoiseOscillator {
    fn new<T: Read + Seek>(bytes: &mut T) -> Result<Self> {
        let wave_type = NoiseWaveType::from_i32(bytes.read_i32_flex()?).unwrap();
        let rev = bytes.read_u32_flex()? != 0;
        let freq = (bytes.read_f32_flex()? / 10.0).max(0.0).min(NOISE_OSC_LIMIT_FREQ);
        let volu = (bytes.read_f32_flex()? / 10.0).max(0.0).min(NOISE_OSC_LIMIT_VOLU);
        let offset = (bytes.read_f32_flex()? / 10.0).max(0.0).min(NOISE_OSC_LIMIT_OFFSET);
        Ok(Self { wave_type, rev, freq, volu, offset })
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



struct Oscillator {
    points: Vec<Point>,
    point_reso: i32,
    volu: u32,
    smp_num: u32,
}

impl Oscillator {
    fn get_overtone(&self, index: i32) -> f64 {
        let work = self.points.iter().fold(0.0, |acc, point| {
            let sss = 2.0 * f64::consts::PI * f64::from(point.x) * f64::from(index) / f64::from(self.smp_num);
            acc + sss.sin() * f64::from(point.y) / f64::from(point.x) / 128.0
        });
        work * f64::from(self.volu) / 128.0
    }

    fn get_coodinate(&self, index: i32) -> f64 {
        let i = self.point_reso * index / self.smp_num as i32;
        let current = self.points.iter().position(|point| point.x <= i);

        let x1;
        let y1;
        let x2;
        let y2;

        match current {
            Some(0) => {
                let first = self.points.first().unwrap();
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

struct Point {
    x: i32,
    y: i32,
}



pub(crate) struct Pcm {
    ch: u16, // 1 or 2
    sps: u32, // 11025 or 22050 or 44100
    bps: u16, // 8 or 16
    smp: Vec<u8>,
}

static RIFF_CODE: &[u8] = b"RIFF";
static WAVE_FMT_CODE: &[u8] = b"WAVEfmt ";
static DATA_CODE: &[u8] = b"data";

impl Pcm {
    fn new<T: Read + Seek>(mut bytes: T) -> Result<Self> {
        // riff
        {
            let mut riff = [0; 4];
            bytes.read_exact(&mut riff)?;
            assert_eq!(riff, RIFF_CODE);
        }
        bytes.seek(SeekFrom::Current(4))?;

        // fmt chunk
        {
            let mut wavefmt = [0; 8];
            bytes.read_exact(&mut wavefmt)?;
            assert_eq!(wavefmt, WAVE_FMT_CODE);
        }
        let size = bytes.read_u32::<LittleEndian>()?;
        let WaveFormatTag { ch, sps, bps } = WaveFormatTag::read_tag(&mut bytes, i64::from(size))?;

        // data chunk (skip unnecessary chunks)
        loop {
            let mut data = [0; 4];
            bytes.read_exact(&mut data)?;
            if data == DATA_CODE { break; }
            let size = bytes.read_u32::<LittleEndian>()?;
            bytes.seek(SeekFrom::Current(i64::from(size)))?;
        }
        let size = bytes.read_u32::<LittleEndian>()?;
        let mut smp = Vec::with_capacity(size as usize);
        bytes.by_ref().take(u64::from(size)).read_to_end(&mut smp)?;

        Ok(Self { ch, sps, bps, smp })
    }
}

struct WaveFormatTag {
    ch: u16, // 1 or 2
    sps: u32, // 11025 or 22050 or 44100
    bps: u16, // 8 or 16
}

impl WaveFormatTag {
    fn read_tag<T: Read + Seek>(bytes: &mut T, size: i64) -> Result<Self> {
        let format_id = bytes.read_u16::<LittleEndian>()?;
        let ch = bytes.read_u16::<LittleEndian>()?;
        let sps = bytes.read_u32::<LittleEndian>()?;
        let byte_per_sec = bytes.read_u32::<LittleEndian>()?;
        let block_size = bytes.read_u16::<LittleEndian>()?;
        let bps = bytes.read_u16::<LittleEndian>()?;
        bytes.seek(SeekFrom::Current(size - 16))?;
        assert_eq!(format_id, 1);
        assert!(ch == 1 || ch == 2);
        assert!(bps == 8 || bps == 16);
        assert_eq!(byte_per_sec, sps * u32::from(ch) * u32::from(bps) / 8);
        assert_eq!(block_size, ch * bps / 8);
        Ok(Self { ch, sps, bps })
    }
}
