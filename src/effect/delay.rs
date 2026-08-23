use crate::error::PxtoneError;
use crate::reader::Reader;
use crate::unit::{MAX_CHANNEL, MAX_GROUP_COUNT};

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

    // The C++ works the sample count out as an integer product divided by two
    // floats, so both divisions run in `f32`:
    //   _smp_num = (int32_t)( sps * 60 / beat_tempo / _freq );
    // In `f64` the length comes out a sample longer for some tempos, which
    // shifts the whole delay line and feeds back.
    self.buffer_size = match self.unit {
      DelayUnit::Beat => ((sample_rate as i32 * 60) as f32 / beat_tempo / self.frequency) as usize,
      DelayUnit::Meas => {
        ((sample_rate as i32 * 60 * beats_per_measure as i32) as f32 / beat_tempo / self.frequency)
          as usize
      }
      DelayUnit::Second => (sample_rate as f32 / self.frequency) as usize,
    };

    if self.buffer_size > 0 {
      self.bufs[0] = vec![0i32; self.buffer_size];
      self.bufs[1] = vec![0i32; self.buffer_size];
    }
  }

  /// Applies the delay to a block of group samples, advancing the ring buffer by
  /// one slot per sample.
  ///
  /// The whole block runs inside the delay so that the rate, the group, the
  /// buffer bound and the ring offset are read once instead of once per sample.
  /// Both channels are handled together for the same reason. Samples are still
  /// visited in order, which is what the ring requires.
  #[inline(never)]
  pub(crate) fn tone_supple<const GROUPS: usize>(
    &mut self,
    mix: &mut [[[i32; GROUPS]; MAX_CHANNEL]],
    channels: usize,
  ) {
    let buffer_size = self.buffer_size;
    if buffer_size == 0 {
      return;
    }
    let rate = self.rate_s32;
    // `PxtoneService::calc_group_count` sizes GROUPS so that every effect's
    // group is in range; spelling that out lets GROUPS == 1 fold to index 0.
    debug_assert!(self.group < GROUPS);
    let group = if GROUPS == 1 { 0 } else { self.group };
    let played = self.played;
    let mut offset = self.offset;

    for group_smps in mix.iter_mut() {
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
      offset = if next < buffer_size { next } else { 0 };
    }

    self.offset = offset;
  }

  pub(crate) fn tone_clear(&mut self) {
    for buf in &mut self.bufs {
      buf.fill(0);
    }
  }

  /// Reads a (12-byte) delay structure
  pub(crate) fn read(&mut self, r: &mut Reader<'_>) -> Result<(), PxtoneError> {
    let _size = r.read_i32()?;
    let unit = r.read_u16()?;
    let group = r.read_u16()? as usize;
    let rate = r.read_f32()?;
    self.unit = DelayUnit::try_from(unit).map_err(|_| PxtoneError::UnknownFormat)?;
    self.frequency = r.read_f32()?;
    self.rate = rate;
    // pxtnDelay::Read falls back to group 0 when the stored group is out of range
    self.group = if group < MAX_GROUP_COUNT { group } else { 0 };
    Ok(())
  }
}
