use std::{i16, f64};
use std::io::*;
use std::vec::Vec;

use num_traits::FromPrimitive;
use byteorder::{LittleEndian, ReadBytesExt};

use helper::{Point, Descriptor};


struct Oscillator {
    points: Vec<Point>,

    volu: i32,
    smp_num: usize,

    point_reso: i32,
}

impl Oscillator {
    fn get_overtone(&self, index: usize) -> f64 {
        let work = self.points.iter().fold(0.0, |acc, point| {
            let sss = 2.0 * f64::consts::PI * (point.x as f64) * (index as f64) / (self.smp_num as f64);
            acc + sss.sin() * (point.y as f64) / (point.x as f64) / 128.0
        });
        work * (self.volu as f64) / 128.0
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
                (y1 as f64) / 128.0
            },
            n => {
                let w = x2 - x1;
                let h = y2 - y1;
                (y1 as f64) + (h as f64) * (n as f64) / (w as f64) / 128.0
            },
        };
        work * (self.volu as f64) / 128.0
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


struct NoiseUnit {
    enable: bool,
    enves: Vec<Point>,
    pan: i32,
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
const NOISE_UNIT_FLAG_UNCOVERED: u32 = 0xffffff83;

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
        } as i32;

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


pub struct Noise {
    smp_num: u32,
    units: Vec<NoiseUnit>,
}

const NOISE_CODE: &'static[u8] = b"PTNOISE-";
const NOISE_VERSION: u32 = 20120418;

const NOISE_MAX_UNIT_NUM: u8 = 4;

const NOISE_LIMIT_SMP_NUM: u32 = 48000 * 10;

// NoiseBuilder
const BASIC_SPS: u16 = 44100;
const BASIC_FREQ: u16 = 100;

const SMP_NUM_RAND: usize = 44100;
const SMP_NUM: usize = (BASIC_SPS / BASIC_FREQ) as usize;

lazy_static! {
    static ref NOISE_TABLE_SINE: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        let points = vec![Point { x: 1, y: 128 }];
        let osci = Oscillator { points, volu: 128, smp_num: SMP_NUM, point_reso: 0 };
        for i in 0..SMP_NUM {
            arr[i] = (osci.get_overtone(i).max(0.0).min(1.0) * (i16::MAX as f64)) as i16;
        }
        arr
    };
    static ref NOISE_TABLE_SAW: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        let work = (i16::MAX as f64) * 2.0;
        for i in 0..SMP_NUM {
            arr[i] = ((i16::MAX as f64) - work * (i as f64) / (SMP_NUM as f64)) as i16;
        }
        arr
    };
    static ref NOISE_TABLE_RECT: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        for i in 0..SMP_NUM/2 {
            arr[i] = i16::MAX;
        }
        for i in SMP_NUM/2..SMP_NUM {
            arr[i] = -i16::MAX;
        }
        arr
    };
    static ref MOISE_TABLE_RANDOM: [i16; SMP_NUM_RAND] = {
        let mut arr = [0; SMP_NUM_RAND];
        let mut buf: [i32; 2] = [0x4444, 0x8888];
        for i in 0..SMP_NUM_RAND {
            let w1 = ((buf[0] as i16) as i32) + buf[1];
            let w2 = ((w1 >> 8) + (w1 << 8)) as i16;
            buf[1] = (buf[0] as i16) as i32;
            buf[0] = w2 as i32;
            arr[i] = w2;
        }
        arr
    };
    static ref NOISE_TABLE_SAW2: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        let mut points: Vec<Point> = Vec::new();
        for i in 0..16 {
            points.push(Point { x: i + 1, y: 128 });
        }
        let osci = Oscillator { points, volu: 128, smp_num: SMP_NUM, point_reso: 0 };
        for i in 0..SMP_NUM {
            arr[i] = (osci.get_overtone(i).max(0.0).min(1.0) * (i16::MAX as f64)) as i16;
        }
        arr
    };
    static ref NOISE_TABLE_RECT2: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        let mut points: Vec<Point> = Vec::new();
        for i in 0..8 {
            points.push(Point { x: i*2 + 1, y: 128 });
        }
        let osci = Oscillator { points, volu: 128, smp_num: SMP_NUM, point_reso: 0 };
        for i in 0..SMP_NUM {
            arr[i] = (osci.get_overtone(i).max(0.0).min(1.0) * (i16::MAX as f64)) as i16;
        }
        arr
    };
    static ref NOISE_TABLE_TRI: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        let points = vec![Point { x: 0, y: 0 }, Point { x: (SMP_NUM/4) as i32, y: 128 }, Point { x: (SMP_NUM*3/4) as i32, y: -128 }, Point { x: SMP_NUM as i32, y: 0 }];
        let osci = Oscillator { points, volu: 128, smp_num: SMP_NUM, point_reso: SMP_NUM as i32 };
        for i in 0..SMP_NUM {
            arr[i] = (osci.get_coodinate(i).max(0.0).min(1.0) * (i16::MAX as f64)) as i16;
        }
        arr
    };
    static ref NOISE_TABLE_RECT3: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        for i in 0..SMP_NUM/3 {
            arr[i] = i16::MAX;
        }
        for i in SMP_NUM/3..SMP_NUM {
            arr[i] = -i16::MAX;
        }
        arr
    };
    static ref NOISE_TABLE_RECT4: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        for i in 0..SMP_NUM/4 {
            arr[i] = i16::MAX;
        }
        for i in SMP_NUM/4..SMP_NUM {
            arr[i] = -i16::MAX;
        }
        arr
    };
    static ref NOISE_TABLE_RECT8: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        for i in 0..SMP_NUM/8 {
            arr[i] = i16::MAX;
        }
        for i in SMP_NUM/8..SMP_NUM {
            arr[i] = -i16::MAX;
        }
        arr
    };
    static ref NOISE_TABLE_RECT16: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        for i in 0..SMP_NUM/16 {
            arr[i] = i16::MAX;
        }
        for i in SMP_NUM/16..SMP_NUM {
            arr[i] = -i16::MAX;
        }
        arr
    };
    static ref NOISE_TABLE_SAW3: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        for i in 0..SMP_NUM/3 {
            arr[i] = i16::MAX;
        }
        for i in SMP_NUM/3..SMP_NUM*2/3 {
            arr[i] = 0;
        }
        for i in SMP_NUM*2/3..SMP_NUM {
            arr[i] = -i16::MAX;
        }
        arr
    };
    static ref NOISE_TABLE_SAW4: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        for i in 0..SMP_NUM/4 {
            arr[i] = i16::MAX;
        }
        for i in SMP_NUM/4..SMP_NUM/2 {
            arr[i] = i16::MAX/3;
        }
        for i in SMP_NUM/2..SMP_NUM*3/4 {
            arr[i] = -i16::MAX/3;
        }
        for i in SMP_NUM*3/4..SMP_NUM {
            arr[i] = -i16::MAX;
        }
        arr
    };
    static ref NOISE_TABLE_SAW6: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        for i in 0..SMP_NUM/6 {
            arr[i] = i16::MAX;
        }
        for i in SMP_NUM/6..SMP_NUM/3 {
            arr[i] = i16::MAX/5*3;
        }
        for i in SMP_NUM/3..SMP_NUM/2 {
            arr[i] = i16::MAX/5;
        }
        for i in SMP_NUM/2..SMP_NUM*2/3 {
            arr[i] = -i16::MAX/5;
        }
        for i in SMP_NUM*2/3..SMP_NUM*5/6 {
            arr[i] = -i16::MAX/5*3;
        }
        for i in SMP_NUM*5/6..SMP_NUM {
            arr[i] = -i16::MAX;
        }
        arr
    };
    static ref NOISE_TABLE_SAW8: [i16; SMP_NUM] = {
        let mut arr = [0; SMP_NUM];
        for i in 0..SMP_NUM/8 {
            arr[i] = i16::MAX;
        }
        for i in SMP_NUM/8..SMP_NUM/4 {
            arr[i] = i16::MAX/7*5;
        }
        for i in SMP_NUM/4..SMP_NUM*3/8 {
            arr[i] = i16::MAX/7*3;
        }
        for i in SMP_NUM*3/8..SMP_NUM/2 {
            arr[i] = i16::MAX/7;
        }
        for i in SMP_NUM/2..SMP_NUM*5/8 {
            arr[i] = -i16::MAX/7;
        }
        for i in SMP_NUM*5/8..SMP_NUM*3/4 {
            arr[i] = -i16::MAX/7*3;
        }
        for i in SMP_NUM*3/4..SMP_NUM*7/8 {
            arr[i] = -i16::MAX/7*5;
        }
        for i in SMP_NUM*7/8..SMP_NUM {
            arr[i] = -i16::MAX;
        }
        arr
    };
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

        let smp_num = bytes.read_u32_flex()?.min(NOISE_LIMIT_SMP_NUM);

        let unit_num = bytes.read_u8()?;
        assert!(unit_num <= NOISE_MAX_UNIT_NUM);

        let mut units = Vec::with_capacity(unit_num as usize);
        for _ in 0..unit_num {
            units.push(NoiseUnit::new(&mut bytes)?);
        }

        Ok(Self { smp_num, units })
    }
}
