use crate::error::PxtoneError;
use crate::read_ext::ReadExt;
use std::io::Read;

// ---- Wave type ----

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub(crate) enum WaveType {
  #[default]
  None = 0,
  Sine = 1,
  Saw = 2,
  Rect = 3,
  Random = 4,
  Saw2 = 5,
  Rect2 = 6,
  Tri = 7,
  Random2 = 8,
  Rect3 = 9,
  Rect4 = 10,
  Rect8 = 11,
  Rect16 = 12,
  Saw3 = 13,
  Saw4 = 14,
  Saw6 = 15,
  Saw8 = 16,
}

pub(crate) const WAVETYPE_NUM: usize = 17;

impl TryFrom<i32> for WaveType {
  type Error = ();
  fn try_from(v: i32) -> Result<Self, ()> {
    match v {
      0 => Ok(WaveType::None),
      1 => Ok(WaveType::Sine),
      2 => Ok(WaveType::Saw),
      3 => Ok(WaveType::Rect),
      4 => Ok(WaveType::Random),
      5 => Ok(WaveType::Saw2),
      6 => Ok(WaveType::Rect2),
      7 => Ok(WaveType::Tri),
      8 => Ok(WaveType::Random2),
      9 => Ok(WaveType::Rect3),
      10 => Ok(WaveType::Rect4),
      11 => Ok(WaveType::Rect8),
      12 => Ok(WaveType::Rect16),
      13 => Ok(WaveType::Saw3),
      14 => Ok(WaveType::Saw4),
      15 => Ok(WaveType::Saw6),
      16 => Ok(WaveType::Saw8),
      _ => Err(()),
    }
  }
}

// ---- Oscillator design ----

#[derive(Clone, Debug, Default)]
pub(crate) struct NoiseOscillator {
  pub(crate) wave_type: WaveType,
  pub(crate) freq: f32,
  pub(crate) volume: f32,
  pub(crate) offset: f32,
  pub(crate) b_rev: bool,
}

// ---- Noise unit design ----

#[derive(Clone, Debug)]
pub(crate) struct NoisePoint {
  pub(crate) x: u32,
  pub(crate) y: u32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NoiseUnit {
  pub(crate) enabled: bool,
  pub(crate) envelopes: Vec<NoisePoint>,
  pub(crate) pan: i8,
  pub(crate) main: NoiseOscillator,
  pub(crate) freq: NoiseOscillator,
  pub(crate) volu: NoiseOscillator,
}

// ---- Flag constants ----

const FLAG_ENVELOPE: u32 = 0x0004;
const FLAG_PAN: u32 = 0x0008;
const FLAG_OSC_MAIN: u32 = 0x0010;
const FLAG_OSC_FREQ: u32 = 0x0020;
const FLAG_OSC_VOLU: u32 = 0x0040;
const FLAG_UNCOVERED: u32 = 0xffffff83;

const MAX_UNIT_NUM: usize = 4;
const MAX_ENVELOPE_NUM: usize = 3;

const CODE: &[u8; 8] = b"PTNOISE-";
const VER: u32 = 20120418;

const LIMIT_SMP_NUM: u32 = 48000 * 10;
const LIMIT_OSC_FREQUENCY: f32 = 44100.0;
const LIMIT_OSC_VOLUME: f32 = 200.0;
const LIMIT_OSC_OFFSET: f32 = 100.0;
const LIMIT_ENVE_X: u32 = 1000 * 10;
const LIMIT_ENVE_Y: u32 = 100;

// ---- Noise ----

#[derive(Debug, Default)]
pub(crate) struct Noise {
  pub(crate) smp_num_44k: u32,
  pub(crate) units: Vec<NoiseUnit>,
}

impl Noise {
  pub(crate) fn new() -> Self {
    Self::default()
  }

  /// Clamps all parameters to their valid ranges
  pub(crate) fn fix(&mut self) {
    if self.smp_num_44k > LIMIT_SMP_NUM {
      self.smp_num_44k = LIMIT_SMP_NUM;
    }
    for unit in &mut self.units {
      if unit.enabled {
        for env in &mut unit.envelopes {
          env.x = env.x.min(LIMIT_ENVE_X);
          env.y = env.y.min(LIMIT_ENVE_Y);
        }
        unit.pan = unit.pan.clamp(-100, 100);
        fix_osc(&mut unit.main);
        fix_osc(&mut unit.freq);
        fix_osc(&mut unit.volu);
      }
    }
  }

  /// Reads a "PTNOISE-" format noise block
  pub(crate) fn read<R: Read>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let mut code = [0u8; 8];
    r.read_exact(&mut code)?;
    if &code != CODE {
      return Err(PxtoneError::InvalidCode);
    }

    let mut ver_buf = [0u8; 4];
    r.read_exact(&mut ver_buf)?;
    let ver = u32::from_le_bytes(ver_buf);
    if ver > VER {
      return Err(PxtoneError::NewFormat);
    }

    self.smp_num_44k = r.read_var_u32()?;

    let mut unit_num_byte = [0u8; 1];
    r.read_exact(&mut unit_num_byte)?;
    let unit_num = unit_num_byte[0] as usize;
    if unit_num > MAX_UNIT_NUM {
      return Err(PxtoneError::UnknownFormat);
    }

    self.units.clear();
    for _ in 0..unit_num {
      let mut unit = NoiseUnit {
        enabled: true,
        ..Default::default()
      };

      let flags = r.read_var_u32()?;
      if flags & FLAG_UNCOVERED != 0 {
        return Err(PxtoneError::UnknownFormat);
      }

      if flags & FLAG_ENVELOPE != 0 {
        let enve_num = r.read_var_u32()?;
        if enve_num as usize > MAX_ENVELOPE_NUM {
          return Err(PxtoneError::UnknownFormat);
        }
        for _ in 0..enve_num {
          let x = r.read_var_u32()?;
          let y = r.read_var_u32()?;
          unit.envelopes.push(NoisePoint { x, y });
        }
      }
      if flags & FLAG_PAN != 0 {
        let mut b = [0u8; 1];
        r.read_exact(&mut b)?;
        unit.pan = b[0] as i8;
      }
      if flags & FLAG_OSC_MAIN != 0 {
        read_oscillator(r, &mut unit.main)?;
      }
      if flags & FLAG_OSC_FREQ != 0 {
        read_oscillator(r, &mut unit.freq)?;
      }
      if flags & FLAG_OSC_VOLU != 0 {
        read_oscillator(r, &mut unit.volu)?;
      }

      self.units.push(unit);
    }
    Ok(())
  }
}

fn fix_osc(osc: &mut NoiseOscillator) {
  osc.freq = osc.freq.clamp(0.0, LIMIT_OSC_FREQUENCY);
  osc.volume = osc.volume.clamp(0.0, LIMIT_OSC_VOLUME);
  osc.offset = osc.offset.clamp(0.0, LIMIT_OSC_OFFSET);
}

fn read_oscillator<R: Read>(r: &mut R, osc: &mut NoiseOscillator) -> Result<(), PxtoneError> {
  let type_val = r.read_var_i32()?;
  osc.wave_type = WaveType::try_from(type_val).map_err(|_| PxtoneError::UnknownFormat)?;
  let b_rev = r.read_var_u32()?;
  osc.b_rev = b_rev != 0;
  let freq = r.read_var_u32()?;
  osc.freq = freq as f32 / 10.0;
  let volume = r.read_var_u32()?;
  osc.volume = volume as f32 / 10.0;
  let offset = r.read_var_u32()?;
  osc.offset = offset as f32 / 10.0;
  Ok(())
}
