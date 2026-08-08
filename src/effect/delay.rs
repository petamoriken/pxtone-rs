use crate::error::PxtoneError;
use crate::unit::{MAX_CHANNEL, MAX_GROUP_COUNT};
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u16)]
pub(crate) enum DelayUnit {
  #[default]
  Beat = 0,
  Meas = 1,
  Second = 2,
}

impl TryFrom<u16> for DelayUnit {
  type Error = ();
  fn try_from(v: u16) -> Result<Self, ()> {
    match v {
      0 => Ok(DelayUnit::Beat),
      1 => Ok(DelayUnit::Meas),
      2 => Ok(DelayUnit::Second),
      _ => Err(()),
    }
  }
}

pub(crate) struct Delay {
  pub(crate) played: bool,
  pub(crate) unit: DelayUnit,
  pub(crate) group: usize,
  pub(crate) rate: f32,
  pub(crate) frequency: f32,
  // runtime
  buffer_size: usize,
  offset: usize,
  rate_s32: i32,
  bufs: [Vec<i32>; 2],
}

impl Default for Delay {
  fn default() -> Self {
    Self {
      played: true,
      unit: DelayUnit::Beat,
      group: 0,
      rate: 33.0,
      frequency: 3.0,
      buffer_size: 0,
      offset: 0,
      rate_s32: 100,
      bufs: Default::default(),
    }
  }
}

impl Delay {
  pub(crate) fn new() -> Self {
    Self::default()
  }

  /// Prepares before playback: allocates the delay buffer
  pub(crate) fn tone_ready(&mut self, beats_per_measure: u8, beat_tempo: f32, sample_rate: u32) {
    self.buffer_size = 0;
    self.bufs[0].clear();
    self.bufs[1].clear();

    if self.frequency == 0.0 || self.rate == 0.0 {
      return;
    }

    self.offset = 0;
    self.rate_s32 = self.rate as i32;

    self.buffer_size = match self.unit {
      DelayUnit::Beat => {
        (sample_rate as f64 * 60.0 / beat_tempo as f64 / self.frequency as f64) as usize
      }
      DelayUnit::Meas => {
        (sample_rate as f64 * 60.0 * beats_per_measure as f64
          / beat_tempo as f64
          / self.frequency as f64) as usize
      }
      DelayUnit::Second => (sample_rate as f64 / self.frequency as f64) as usize,
    };

    if self.buffer_size > 0 {
      self.bufs[0] = vec![0i32; self.buffer_size];
      self.bufs[1] = vec![0i32; self.buffer_size];
    }
  }

  /// Applies the delay to every channel's group samples and advances the ring
  /// buffer. Both channels share the same `offset`, `rate` and `group`, so
  /// handling them together loads that state once per sample.
  #[inline(always)]
  pub(crate) fn tone_supple<const GROUPS: usize>(
    &mut self,
    group_smps: &mut [[i32; GROUPS]; MAX_CHANNEL],
    channels: usize,
  ) {
    if self.buffer_size == 0 {
      return;
    }
    let offset = self.offset;
    let rate = self.rate_s32;
    // `PxtoneService::calc_group_count` sizes GROUPS so that every effect's
    // group is in range; spelling that out lets GROUPS == 1 fold to index 0.
    debug_assert!(self.group < GROUPS);
    let group = if GROUPS == 1 { 0 } else { self.group };
    let played = self.played;

    for (buf, groups) in self
      .bufs
      .iter_mut()
      .zip(group_smps.iter_mut())
      .take(channels)
    {
      let slot = &mut buf[offset];
      let a = *slot * rate / 100;
      if played {
        groups[group] += a;
      }
      *slot = groups[group];
    }

    let next = offset + 1;
    self.offset = if next < self.buffer_size { next } else { 0 };
  }

  pub(crate) fn tone_clear(&mut self) {
    for buf in &mut self.bufs {
      buf.fill(0);
    }
  }

  /// Reads a (12-byte) delay structure
  pub(crate) fn read<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    let unit = r.read_u16::<LE>()?;
    let group = r.read_u16::<LE>()? as usize;
    let rate = r.read_f32::<LE>()?;
    self.unit = DelayUnit::try_from(unit).map_err(|_| PxtoneError::UnknownFormat)?;
    self.frequency = r.read_f32::<LE>()?;
    self.rate = rate;
    // pxtnDelay::Read falls back to group 0 when the stored group is out of range
    self.group = if group < MAX_GROUP_COUNT { group } else { 0 };
    Ok(())
  }
}
