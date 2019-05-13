mod frequency_table;
mod noise_builder;

use std::{
    f64,
    io::{Read, Write, Seek, SeekFrom},
    vec::Vec,
};

use crate::error::Result;

use num_traits::FromPrimitive;

use crate::descriptor::ReadBytesExt as _;
use byteorder::{LittleEndian, ReadBytesExt as _, WriteBytesExt as _};

use frequency_table::*;
use noise_builder::NoiseBuilder;

pub(crate) struct Noise {
    units: Vec<NoiseUnit>,
    smp_num_44k: u32,
}

impl Noise {
    const CODE: &'static [u8] = b"PTNOISE-";
    const VERSION: u32 = 2012_0418;
    const MAX_UNIT_NUM: u8 = 4;
    const LIMIT_SMP_NUM: u32 = 48000 * 10;

    pub fn new<T: Read + Seek>(mut bytes: T) -> Result<Self> {
        // signature
        let mut code = [0; 8];
        bytes.read_exact(&mut code)?;
        assert_eq!(code, Self::CODE);

        let version = bytes.read_u32::<LittleEndian>()?;
        assert!(version <= Self::VERSION);

        let smp_num_44k = bytes.read_var_u32()?.min(Self::LIMIT_SMP_NUM);

        let unit_num = bytes.read_u8()?;
        assert!(unit_num <= Self::MAX_UNIT_NUM);

        let mut units = Vec::with_capacity(unit_num as usize);
        for _ in 0..unit_num {
            units.push(NoiseUnit::new(&mut bytes)?);
        }

        Ok(Self { units, smp_num_44k })
    }

    pub fn build(&self, ch: u16, sps: u32, bps: u16) -> Result<Pcm> {
        NoiseBuilder::build(self, ch, sps, bps)
    }
}

struct NoiseUnit {
    enable: bool,
    enves: Vec<Point>,
    pan: i8,
    main: Option<NoiseOscillator>,
    freq: Option<NoiseOscillator>,
    volu: Option<NoiseOscillator>,
}

impl NoiseUnit {
    // const FLAG_XX1: u32 = 0x0001;
    // const FLAG_XX2: u32 = 0x0002;
    const FLAG_ENVELOPE: u32 = 0x0004;
    const FLAG_PAN: u32 = 0x0008;
    const FLAG_OSC_MAIN: u32 = 0x0010;
    const FLAG_OSC_FREQ: u32 = 0x0020;
    const FLAG_OSC_VOLU: u32 = 0x0040;
    // const FLAG_OSC_PAN: u32 = 0x0080;
    const FLAG_UNCOVERED: u32 = 0xffff_ff83;

    const MAX_ENVELOPE_NUM: u32 = 3;
    const LIMIT_ENVE_X: i32 = 1000 * 10;
    const LIMIT_ENVE_Y: i32 = 100;

    fn new<T: Read + Seek>(bytes: &mut T) -> Result<Self> {
        let enable = true;

        let flags = bytes.read_var_u32()?;
        assert_eq!(flags & Self::FLAG_UNCOVERED, 0);

        // envelope
        let enves = if flags & Self::FLAG_ENVELOPE != 0 {
            let enve_num = bytes.read_var_u32()?;
            assert!(enve_num <= Self::MAX_ENVELOPE_NUM);

            let mut enves = Vec::with_capacity(enve_num as usize);
            for _ in 0..enve_num {
                enves.push(Point {
                    x: bytes.read_var_i32()?.max(0).min(Self::LIMIT_ENVE_X),
                    y: bytes.read_var_i32()?.max(0).min(Self::LIMIT_ENVE_Y),
                });
            }
            enves
        } else {
            Vec::with_capacity(0)
        };

        // pan
        let pan = if flags & Self::FLAG_PAN != 0 {
            bytes.read_i8()?
        } else {
            0
        };

        // oscillator
        let main = if flags & Self::FLAG_OSC_MAIN != 0 {
            Some(NoiseOscillator::new(bytes)?)
        } else {
            None
        };
        let freq = if flags & Self::FLAG_OSC_FREQ != 0 {
            Some(NoiseOscillator::new(bytes)?)
        } else {
            None
        };
        let volu = if flags & Self::FLAG_OSC_VOLU != 0 {
            Some(NoiseOscillator::new(bytes)?)
        } else {
            None
        };

        Ok(Self {
            enable,
            enves,
            pan,
            main,
            freq,
            volu,
        })
    }
}

struct NoiseOscillator {
    wave: NoiseWave,
    rev: bool,
    freq: f32,
    volu: f32,
    offset: f32,
}

impl NoiseOscillator {
    const LIMIT_FREQ: f32 = 44100.0;
    const LIMIT_VOLU: f32 = 200.0;
    const LIMIT_OFFSET: f32 = 100.0;

    fn new<T: Read + Seek>(bytes: &mut T) -> Result<Self> {
        let wave = NoiseWave::from_i32(bytes.read_var_i32()?).unwrap();
        let rev = bytes.read_var_u32()? != 0;
        let freq = (bytes.read_var_f32()? / 10.0)
            .max(0.0)
            .min(Self::LIMIT_FREQ);
        let volu = (bytes.read_var_f32()? / 10.0)
            .max(0.0)
            .min(Self::LIMIT_VOLU);
        let offset = (bytes.read_var_f32()? / 10.0)
            .max(0.0)
            .min(Self::LIMIT_OFFSET);
        Ok(Self {
            wave,
            rev,
            freq,
            volu,
            offset,
        })
    }
}

#[derive(FromPrimitive)]
enum NoiseWave {
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

struct Voice {
    units: Vec<VoiceUnit>,
    x3x_basic_key: i32,
}

impl Voice {
    const CODE: &'static [u8] = b"PTVOICE-";
    const VERSION: u32 = 2006_0111;

    pub fn new<T: Read + Seek>(mut bytes: T) -> Result<Self> {
        // signature
        let mut code = [0; 8];
        bytes.read_exact(&mut code)?;
        assert_eq!(code, Self::CODE);

        let version = bytes.read_u32::<LittleEndian>()?;
        assert!(version <= Self::VERSION);
        bytes.seek(SeekFrom::Current(4))?;

        let x3x_basic_key = bytes.read_var_i32()?;
        let work1 = bytes.read_var_u32()?;
        let work2 = bytes.read_var_u32()?;
        assert!(work1 == 0 && work2 == 0);

        let unit_num = bytes.read_var_u32()?;
        let mut units = Vec::with_capacity(unit_num as usize);
        for _ in 0..unit_num {
            units.push(VoiceUnit::new(&mut bytes)?);
        }

        Ok(Self {
            units,
            x3x_basic_key,
        })
    }
}

struct VoiceUnit {
    basic_key: i32,
    volu: i32,
    pan: i32,
    tuning: f32,
    flags: u32,
    wave: Option<VoiceWave>,
    enve: Option<VoiceEnvelope>,
}

impl VoiceUnit {
    const FLAG_WAVELOOP: u32 = 0x0001;
    const FLAG_SMOOTH: u32 = 0x0002;
    const FLAG_BEATFIT: u32 = 0x0004;
    const FLAG_UNCOVERED: u32 = 0xffff_fff8;

    const DATA_FLAG_WAVE: u32 = 0x0001;
    const DATA_FLAG_ENVELOPE: u32 = 0x0002;
    const DATA_FLAG_UNCOVERED: u32 = 0xffff_fffc;

    fn new<T: Read + Seek>(bytes: &mut T) -> Result<Self> {
        let basic_key = bytes.read_var_i32()?;
        let volu = bytes.read_var_i32()?;
        let pan = bytes.read_var_i32()?;
        let tuning = bytes.read_var_f32()?;

        let flags = bytes.read_var_u32()?;
        assert_eq!(flags & Self::FLAG_UNCOVERED, 0);

        let data_flags = bytes.read_var_u32()?;
        assert_eq!(data_flags & Self::DATA_FLAG_UNCOVERED, 0);

        // wave
        let wave = if data_flags & Self::DATA_FLAG_WAVE != 0 {
            let wave_type = VoiceWaveType::from_i32(bytes.read_var_i32()?).unwrap();
            match wave_type {
                VoiceWaveType::Coodinate => {
                    let num = bytes.read_var_u32()?;
                    let reso = bytes.read_var_i32()?;
                    let mut points = Vec::with_capacity(num as usize);
                    for _ in 0..num {
                        points.push(Point {
                            x: i32::from(bytes.read_i8()?),
                            y: i32::from(bytes.read_i8()?),
                        });
                    }
                    Some(VoiceWave::Coodinate { points, reso })
                }
                VoiceWaveType::Overtone => {
                    let num = bytes.read_var_u32()?;
                    let mut points = Vec::with_capacity(num as usize);
                    for _ in 0..num {
                        points.push(Point {
                            x: bytes.read_var_i32()?,
                            y: bytes.read_var_i32()?,
                        });
                    }
                    Some(VoiceWave::Overtone { points })
                }
                _ => unreachable!(),
            }
        } else {
            None
        };

        // envelope
        let enve = if data_flags & Self::DATA_FLAG_ENVELOPE != 0 {
            Some(VoiceEnvelope::new(bytes)?)
        } else {
            None
        };

        Ok(Self {
            basic_key,
            volu,
            pan,
            tuning,
            flags,
            wave,
            enve,
        })
    }
}

#[derive(FromPrimitive)]
enum VoiceWaveType {
    Coodinate,
    Overtone,
    Noise,
    Sampling,
    OggVorbis,
}

enum VoiceWave {
    Coodinate { points: Vec<Point>, reso: i32 },
    Overtone { points: Vec<Point> },
}

struct VoiceEnvelope {
    points: Vec<Point>,
    fps: i32,
}

impl VoiceEnvelope {
    fn new<T: Read + Seek>(bytes: &mut T) -> Result<Self> {
        let fps = bytes.read_var_i32()?;
        let head_num = bytes.read_var_u32()?;
        let body_num = bytes.read_var_u32()?; // 0
        let tail_num = bytes.read_var_u32()?; // 1
        assert_eq!(body_num, 0);
        assert_eq!(tail_num, 1);

        let num = head_num + body_num + tail_num;
        let mut points = Vec::with_capacity(num as usize);
        for _ in 0..num {
            points.push(Point {
                x: bytes.read_var_i32()?,
                y: bytes.read_var_i32()?,
            });
        }
        Ok(Self { points, fps })
    }
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
            let sss = 2.0 * f64::consts::PI * f64::from(point.x) * f64::from(index)
                / f64::from(self.smp_num);
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
                let first = &self.points[c - 1];
                let second = &self.points[c];
                x1 = first.x;
                y1 = first.y;
                x2 = second.x;
                y2 = second.y;
            }
            None => {
                let first = self.points.first().unwrap();
                let last = self.points.last().unwrap();
                x1 = last.x;
                y1 = last.y;
                x2 = self.point_reso;
                y2 = first.y;
            }
        }

        let work = match i - x1 {
            0 => f64::from(y1) / 128.0,
            n => {
                let w = x2 - x1;
                let h = y2 - y1;
                f64::from(y1) + f64::from(h) * f64::from(n) / f64::from(w) / 128.0
            }
        };
        work * f64::from(self.volu) / 128.0
    }
}

struct Point {
    x: i32,
    y: i32,
}

struct Frequency {}

impl Frequency {
    fn get(key: i32) -> f32 {
        let i = ((key + 0x6000) * (FREQUENCY_PER_KEY as i32) / 0x100)
            .max(0)
            .min((FREQUENCY_TABLE_SIZE as i32) - 1) as usize;
        FREQUENCY_TABLE[i]
    }

    fn get2(key: i32) -> f32 {
        let i = (key >> 4).max(0).min((FREQUENCY_TABLE_SIZE as i32) - 1) as usize;
        FREQUENCY_TABLE[i]
    }
}

pub(crate) struct Pcm {
    fmt: PcmWaveFormat,
    smp: Vec<u8>,
}

pub trait Sample {
    fn from_u8(bits: u8) -> Self;
    fn from_i16(bits: i16) -> Self;
}

impl Sample for u8 {
    #[inline]
    fn from_u8(bits: u8) -> Self {
        bits
    }

    #[inline]
    fn from_i16(bits: i16) -> Self {
        ((bits >> 8) as i8) as u8 ^ 0x80
    }
}

impl Sample for i8 {
    #[inline]
    fn from_u8(bits: u8) -> Self {
        (bits ^ 0x80) as i8
    }

    #[inline]
    fn from_i16(bits: i16) -> Self {
        (bits >> 8) as i8
    }
}

impl Sample for u16 {
    #[inline]
    fn from_u8(bits: u8) -> Self {
        u16::from(bits) << 8
    }

    #[inline]
    fn from_i16(bits: i16) -> Self {
        (bits as u16) ^ 0x8000
    }
}

impl Sample for i16 {
    #[inline]
    fn from_u8(bits: u8) -> Self {
        i16::from((bits ^ 0x80) as i8) << 8
    }

    #[inline]
    fn from_i16(bits: i16) -> Self {
        bits
    }
}

impl Sample for f32 {
    #[inline]
    #[allow(non_upper_case_globals)]
    fn from_u8(bits: u8) -> Self {
        const i8_min_abs: f32 = -(i8::min_value() as f32);
        const i8_max_abs: f32 = i8::max_value() as f32;
        let float_i8 = f32::from((bits ^ 0x80) as i8);
        if float_i8 < 0.0 { float_i8 / i8_min_abs } else { float_i8 / i8_max_abs }
    }

    #[inline]
    #[allow(non_upper_case_globals)]
    fn from_i16(bits: i16) -> Self {
        const i16_min_abs: f32 = -(i16::min_value() as f32);
        const i16_max_abs: f32 = i16::max_value() as f32;
        let float_i16 = f32::from(bits);
        if float_i16 < 0.0 { float_i16 / i16_min_abs } else { float_i16 / i16_max_abs }
    }
}

impl Pcm {
    const RIFF_CODE: &'static [u8] = b"RIFF";
    const WAVE_FMT_CODE: &'static [u8] = b"WAVEfmt ";
    const DATA_CODE: &'static [u8] = b"data";

    fn new<T: Read + Seek>(mut bytes: T) -> Result<Self> {
        // riff
        {
            let mut riff = [0; 4];
            bytes.read_exact(&mut riff)?;
            assert_eq!(riff, Self::RIFF_CODE);
        }
        bytes.seek(SeekFrom::Current(4))?;

        // fmt chunk
        {
            let mut wavefmt = [0; 8];
            bytes.read_exact(&mut wavefmt)?;
            assert_eq!(wavefmt, Self::WAVE_FMT_CODE);
        }
        let size = bytes.read_u32::<LittleEndian>()?;
        let fmt = PcmWaveFormat::read_chunk(&mut bytes, i64::from(size))?;

        // data chunk (skip unnecessary chunks)
        loop {
            let mut data = [0; 4];
            bytes.read_exact(&mut data)?;
            if data == Self::DATA_CODE {
                break;
            }
            let size = bytes.read_u32::<LittleEndian>()?;
            bytes.seek(SeekFrom::Current(i64::from(size)))?;
        }
        let size = bytes.read_u32::<LittleEndian>()?;
        let mut smp = Vec::with_capacity(size as usize);
        bytes.take(u64::from(size)).read_to_end(&mut smp)?;

        Ok(Self { fmt, smp })
    }

    pub fn into_bytes(mut self) -> Vec<u8> {
        let size = 44 + self.smp.len();
        let mut bytes = Vec::with_capacity(size);

        // riff
        bytes.write_all(Self::RIFF_CODE).unwrap();
        bytes.write_u32::<LittleEndian>((size - 8) as u32).unwrap();

        // fmt
        bytes.write_all(Self::WAVE_FMT_CODE).unwrap();
        bytes.write_u32::<LittleEndian>(16).unwrap();
        self.fmt.write_chunk(&mut bytes).unwrap();

        // data
        bytes.write_all(Self::DATA_CODE).unwrap();
        bytes.write_u32::<LittleEndian>(self.smp.len() as u32).unwrap();
        bytes.append(&mut self.smp);

        bytes
    }

    pub fn to_channels<T: Sample>(&self) -> Vec<Vec<T>> {
        let PcmWaveFormat { ch, bps, .. } = self.fmt;
        let mut channels = Vec::with_capacity(ch as usize);
        let size = self.smp.len() / (ch as usize) / ((bps / 8) as usize);
        for _ in 0..ch {
            channels.push(Vec::with_capacity(size));
        }

        let mut bytes = &self.smp[..];
        while !bytes.is_empty() {
            for c in channels.iter_mut() {
                if bps == 8 {
                    c.push(T::from_u8(bytes.read_u8().unwrap()));
                } else {
                    c.push(T::from_i16(bytes.read_i16::<LittleEndian>().unwrap()));
                }
            }
        }

        channels
    }
}

struct PcmWaveFormat {
    ch: u16,  // 1 or 2
    sps: u32,
    bps: u16, // 8 or 16
}

impl PcmWaveFormat {
    fn read_chunk<T: Read + Seek>(bytes: &mut T, size: i64) -> Result<Self> {
        assert!(size >= 16);
        let id = bytes.read_u16::<LittleEndian>()?;
        let ch = bytes.read_u16::<LittleEndian>()?;
        let sps = bytes.read_u32::<LittleEndian>()?;
        let byte_per_sec = bytes.read_u32::<LittleEndian>()?;
        let block_size = bytes.read_u16::<LittleEndian>()?;
        let bps = bytes.read_u16::<LittleEndian>()?;
        bytes.seek(SeekFrom::Current(size - 16))?;
        assert_eq!(id, 1); // Linear PCM
        assert!(ch == 1 || ch == 2);
        assert!(bps == 8 || bps == 16);
        assert_eq!(byte_per_sec, sps * u32::from(ch) * u32::from(bps) / 8);
        assert_eq!(block_size, ch * bps / 8);
        Ok(Self { ch, sps, bps })
    }

    fn write_chunk<T: Write>(&self, writer: &mut T) -> Result<()> {
        writer.write_u16::<LittleEndian>(1)?;
        writer.write_u16::<LittleEndian>(self.ch)?;
        writer.write_u32::<LittleEndian>(self.sps)?;
        writer.write_u32::<LittleEndian>(self.sps * u32::from(self.ch) * u32::from(self.bps) / 8)?;
        writer.write_u16::<LittleEndian>(self.ch * self.bps / 8)?;
        writer.write_u16::<LittleEndian>(self.bps)?;
        Ok(())
    }
}
