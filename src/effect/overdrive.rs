use crate::error::PxtoneError;
use crate::reader::Reader;
use crate::unit::{MAX_CHANNEL, MAX_GROUP_COUNT};

const CUT_MIN: f32 = 0.0;
const CUT_MAX: f32 = 100.0;
const AMP_MIN: f32 = 0.0;
const AMP_MAX: f32 = 800.0;

pub(crate) struct OverDrive {
  pub(crate) played: bool,
  pub(crate) cut: f32,
  pub(crate) amp: f32,
  pub(crate) group: usize,
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
  pub(crate) fn new() -> Self {
    Self::default()
  }

  pub(crate) fn tone_ready(&mut self) {
    self.cut_16bit_top = (32767.0 * (100.0 - self.cut) / 100.0) as i32;
  }

  /// Clips and amplifies one group across a block of samples.
  ///
  /// Stateless, so the block only has to be walked in some order, not in sample
  /// order; running it here rather than at the call site keeps the group index,
  /// the clip bound and the gain out of the per-sample path.
  #[inline(never)]
  pub(crate) fn tone_supple<const GROUPS: usize>(
    &self,
    mix: &mut [[[i32; GROUPS]; MAX_CHANNEL]],
    channels: usize,
  ) {
    if !self.played {
      return;
    }
    // `PxtoneService::calc_group_count` sizes GROUPS so that every effect's
    // group is in range; spelling that out lets GROUPS == 1 fold to index 0.
    debug_assert!(self.group < GROUPS);
    let group = if GROUPS == 1 { 0 } else { self.group };
    let cut = self.cut_16bit_top;
    let amp = self.amp;

    for group_smps in mix.iter_mut() {
      for groups in group_smps.iter_mut().take(channels) {
        let work = groups[group].clamp(-cut, cut);
        groups[group] = (work as f32 * amp) as i32;
      }
    }
  }

  /// Reads a (20-byte) overdrive structure
  pub(crate) fn read(&mut self, r: &mut Reader<'_>) -> Result<(), PxtoneError> {
    let _size = r.read_u32()?;
    let xxx = r.read_u16()?;
    let group = r.read_u16()? as usize;
    let cut = r.read_f32()?;
    let amp = r.read_f32()?;
    let yyy = r.read_f32()?;

    if xxx != 0 {
      return Err(PxtoneError::UnknownFormat);
    }
    if yyy != 0.0 {
      return Err(PxtoneError::UnknownFormat);
    }
    if !(CUT_MIN..=CUT_MAX).contains(&cut) {
      return Err(PxtoneError::UnknownFormat);
    }
    if !(AMP_MIN..=AMP_MAX).contains(&amp) {
      return Err(PxtoneError::UnknownFormat);
    }
    if group >= MAX_GROUP_COUNT {
      return Err(PxtoneError::UnknownFormat);
    }

    self.cut = cut;
    self.amp = amp;
    self.group = group;
    Ok(())
  }
}
