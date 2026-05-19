use crate::error::PxtoneError;
use crate::event::{
  EVENT_DEFAULT_BEAT_TEMPO, EVENT_DEFAULT_BEATS_PER_MEASURE, EVENT_DEFAULT_TICKS_PER_BEAT,
  EVENT_KIND_BEAT_TEMPO, EVENT_KIND_BEATS_PER_MEASURE, EVENT_KIND_LAST, EVENT_KIND_REPEAT,
  EVENT_KIND_TICKS_PER_BEAT,
};
use crate::read_ext::ReadExt;
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek};

/// Song-level timing parameters loaded from the file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Master {
  pub(crate) beats_per_measure: u8,
  pub(crate) beat_tempo: f32,
  pub(crate) ticks_per_beat: u16,
  pub(crate) measure_count: u32,
  pub(crate) repeat_measure: u32,
  pub(crate) last_measure: u32,
}

impl Default for Master {
  fn default() -> Self {
    Self {
      beats_per_measure: EVENT_DEFAULT_BEATS_PER_MEASURE,
      beat_tempo: EVENT_DEFAULT_BEAT_TEMPO,
      ticks_per_beat: EVENT_DEFAULT_TICKS_PER_BEAT,
      measure_count: 1,
      repeat_measure: 0,
      last_measure: 0,
    }
  }
}

impl Master {
  pub fn new() -> Self {
    Self::default()
  }

  /// Returns the number of ticks per beat.
  pub fn ticks_per_beat(&self) -> u16 {
    self.ticks_per_beat
  }

  /// Returns the number of beats per measure.
  pub fn beats_per_measure(&self) -> u8 {
    self.beats_per_measure
  }

  /// Returns the tempo in beats per minute.
  pub fn beat_tempo(&self) -> f32 {
    self.beat_tempo
  }

  /// Returns the total length of the song in measures.
  pub fn measure_count(&self) -> u32 {
    self.measure_count
  }

  /// Returns the loop start position in measures. `0` means no loop point is set.
  pub fn repeat_measure(&self) -> u32 {
    self.repeat_measure
  }

  /// Returns the loop end position in measures. `0` means use the full song length.
  pub fn last_measure(&self) -> u32 {
    self.last_measure
  }

  pub(crate) fn get_last_tick(&self) -> u32 {
    self.last_measure * self.ticks_per_beat as u32 * self.beats_per_measure as u32
  }

  pub(crate) fn get_play_meas(&self) -> u32 {
    if self.last_measure != 0 {
      self.last_measure
    } else {
      self.measure_count
    }
  }

  pub(crate) fn adjust_measure_count(&mut self, tick: u32) {
    let b_count = tick.div_ceil(self.ticks_per_beat as u32);
    let m_count = b_count.div_ceil(self.beats_per_measure as u32);
    if self.measure_count <= m_count {
      self.measure_count = m_count;
    }
    if self.repeat_measure >= self.measure_count {
      self.repeat_measure = 0;
    }
    if self.last_measure > self.measure_count {
      self.last_measure = self.measure_count;
    }
  }

  pub(crate) fn set_repeat_measure(&mut self, meas: u32) {
    self.repeat_measure = meas;
  }

  pub(crate) fn set_last_measure(&mut self, meas: u32) {
    self.last_measure = meas;
  }

  // Reads a v5-format Master block.
  // Block: u32 size(=15), i16 ticks_per_beat, u8 beats_per_measure, f32 beat_tempo,
  //        i32 tick_repeat, i32 tick_last
  pub(crate) fn read_v5<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let size = r.read_u32::<LE>()?;
    if size != 15 {
      return Err(PxtoneError::UnknownFormat);
    }

    let ticks_per_beat = r.read_i16::<LE>()? as i32;
    let beats_per_measure = r.read_u8()?;
    let beat_tempo = r.read_f32::<LE>()?;
    let tick_repeat = r.read_i32::<LE>()?;
    let tick_last = r.read_i32::<LE>()?;

    self.ticks_per_beat = ticks_per_beat as u16;
    self.beats_per_measure = beats_per_measure;
    self.beat_tempo = beat_tempo;

    let denom = beats_per_measure as i32 * ticks_per_beat;
    if denom > 0 {
      self.set_repeat_measure((tick_repeat / denom) as u32);
      self.set_last_measure((tick_last / denom) as u32);
    }

    Ok(())
  }

  // Reads an x4x-format Master block.
  pub(crate) fn read_x4x<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    let data_count = r.read_u16::<LE>()?;
    let rrr = r.read_u16::<LE>()?;
    let event_count = r.read_u32::<LE>()?;

    if data_count != 3 {
      return Err(PxtoneError::UnknownFormat);
    }
    if rrr != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    let mut ticks_per_beat: i32 = EVENT_DEFAULT_TICKS_PER_BEAT.into();
    let mut beats_per_measure: u8 = EVENT_DEFAULT_BEATS_PER_MEASURE;
    let mut beat_tempo = EVENT_DEFAULT_BEAT_TEMPO;
    let mut repeat_tick = 0i32;
    let mut last_tick = 0i32;
    let mut absolute = 0i32;

    for _ in 0..event_count {
      let status = r.read_var_u32()?;
      let tick_delta = r.read_var_i32()?;
      let volume = r.read_var_i32()?;
      absolute += tick_delta;
      let tick = absolute;

      match status as u8 {
        EVENT_KIND_TICKS_PER_BEAT | EVENT_KIND_BEAT_TEMPO | EVENT_KIND_BEATS_PER_MEASURE
          if tick != 0 =>
        {
          return Err(PxtoneError::BrokenFile);
        }
        EVENT_KIND_REPEAT | EVENT_KIND_LAST if volume != 0 => return Err(PxtoneError::BrokenFile),
        EVENT_KIND_TICKS_PER_BEAT => ticks_per_beat = volume,
        EVENT_KIND_BEAT_TEMPO => beat_tempo = f32::from_bits(volume as u32),
        EVENT_KIND_BEATS_PER_MEASURE => beats_per_measure = volume as u8,
        EVENT_KIND_REPEAT => repeat_tick = tick,
        EVENT_KIND_LAST => last_tick = tick,
        _ => return Err(PxtoneError::UnknownFormat),
      }
    }

    self.beats_per_measure = beats_per_measure;
    self.beat_tempo = beat_tempo;
    self.ticks_per_beat = ticks_per_beat as u16;

    let denom = beats_per_measure as i32 * ticks_per_beat;
    if denom > 0 {
      self.set_repeat_measure((repeat_tick / denom) as u32);
      self.set_last_measure((last_tick / denom) as u32);
    }

    Ok(())
  }
}
