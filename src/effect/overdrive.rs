use crate::error::PxtoneError;
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek};

const CUT_MIN: f32 = 0.0;
const CUT_MAX: f32 = 100.0;
const AMP_MIN: f32 = 0.0;
const AMP_MAX: f32 = 800.0;

pub struct OverDrive {
  pub played: bool,
  pub cut: f32,
  pub amp: f32,
  pub group: usize,
  // runtime
  cut_16bit_top: i32,
}

impl Default for OverDrive {
  fn default() -> Self {
    Self {
      played: true,
      cut: 0.0,
      amp: 0.0,
      group: 0,
      cut_16bit_top: 0,
    }
  }
}

impl OverDrive {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn tone_ready(&mut self) {
    self.cut_16bit_top = (32767.0 * (100.0 - self.cut) / 100.0) as i32;
  }

  pub fn tone_supple(&self, group_smps: &mut [i32]) {
    if !self.played {
      return;
    }
    let work = group_smps[self.group].clamp(-self.cut_16bit_top, self.cut_16bit_top);
    group_smps[self.group] = (work as f32 * self.amp) as i32;
  }

  /// Reads a (20-byte) overdrive structure
  pub fn read<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_u32::<LE>()?;
    let xxx = r.read_u16::<LE>()?;
    let group = r.read_u16::<LE>()? as usize;
    let cut = r.read_f32::<LE>()?;
    let amp = r.read_f32::<LE>()?;
    let yyy = r.read_f32::<LE>()?;

    if xxx != 0 {
      return Err(PxtoneError::UnknownFormat);
    }
    if yyy != 0.0 {
      return Err(PxtoneError::UnknownFormat);
    }
    if cut < CUT_MIN || cut > CUT_MAX {
      return Err(PxtoneError::UnknownFormat);
    }
    if amp < AMP_MIN || amp > AMP_MAX {
      return Err(PxtoneError::UnknownFormat);
    }

    self.cut = cut;
    self.amp = amp;
    self.group = group;
    Ok(())
  }
}
