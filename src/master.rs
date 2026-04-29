use crate::error::PxtoneError;
use crate::event::{
  EVENT_DEFAULT_BEAT_CLOCK, EVENT_DEFAULT_BEAT_NUM, EVENT_DEFAULT_BEAT_TEMPO,
  EVENT_KIND_BEAT_CLOCK, EVENT_KIND_BEAT_NUM, EVENT_KIND_BEAT_TEMPO, EVENT_KIND_LAST,
  EVENT_KIND_REPEAT,
};
use crate::read_ext::ReadExt;
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek};

/// Song-level timing parameters loaded from the file.
#[derive(Debug)]
pub struct Master {
  pub(crate) beat_num: u8,
  pub(crate) beat_tempo: f32,
  pub(crate) beat_clock: u16,
  pub(crate) measure_num: u32,
  pub(crate) repeat_measure: u32,
  pub(crate) last_measure: u32,
}

impl Default for Master {
  fn default() -> Self {
    Self {
      beat_num: EVENT_DEFAULT_BEAT_NUM,
      beat_tempo: EVENT_DEFAULT_BEAT_TEMPO,
      beat_clock: EVENT_DEFAULT_BEAT_CLOCK,
      measure_num: 1,
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
  pub fn beat_clock(&self) -> u16 {
    self.beat_clock
  }

  /// Returns the number of beats per measure.
  pub fn beat_num(&self) -> u8 {
    self.beat_num
  }

  /// Returns the tempo in beats per minute.
  pub fn beat_tempo(&self) -> f32 {
    self.beat_tempo
  }

  /// Returns the total length of the song in measures.
  pub fn measure_num(&self) -> u32 {
    self.measure_num
  }

  /// Returns the loop start position in measures. `0` means no loop point is set.
  pub fn repeat_measure(&self) -> u32 {
    self.repeat_measure
  }

  /// Returns the loop end position in measures. `0` means use the full song length.
  pub fn last_measure(&self) -> u32 {
    self.last_measure
  }

  pub(crate) fn get_last_clock(&self) -> u32 {
    self.last_measure * self.beat_clock as u32 * self.beat_num as u32
  }

  pub(crate) fn get_play_meas(&self) -> u32 {
    if self.last_measure != 0 {
      self.last_measure
    } else {
      self.measure_num
    }
  }

  pub(crate) fn adjust_measure_num(&mut self, clock: u32) {
    let b_num = clock.div_ceil(self.beat_clock as u32);
    let m_num = b_num.div_ceil(self.beat_num as u32);
    if self.measure_num <= m_num {
      self.measure_num = m_num;
    }
    if self.repeat_measure >= self.measure_num {
      self.repeat_measure = 0;
    }
    if self.last_measure > self.measure_num {
      self.last_measure = self.measure_num;
    }
  }

  pub(crate) fn set_repeat_measure(&mut self, meas: u32) {
    self.repeat_measure = meas;
  }

  pub(crate) fn set_last_measure(&mut self, meas: u32) {
    self.last_measure = meas;
  }

  // Reads a v5-format Master block.
  // Block: u32 size(=15), i16 beat_clock, u8 beat_num, f32 beat_tempo,
  //        i32 clock_repeat, i32 clock_last
  pub(crate) fn read_v5<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let size = r.read_u32::<LE>()?;
    if size != 15 {
      return Err(PxtoneError::UnknownFormat);
    }

    let beat_clock = r.read_i16::<LE>()? as i32;
    let beat_num = r.read_u8()?;
    let beat_tempo = r.read_f32::<LE>()?;
    let clock_repeat = r.read_i32::<LE>()?;
    let clock_last = r.read_i32::<LE>()?;

    self.beat_clock = beat_clock as u16;
    self.beat_num = beat_num;
    self.beat_tempo = beat_tempo;

    let denom = beat_num as i32 * beat_clock;
    if denom > 0 {
      self.set_repeat_measure((clock_repeat / denom) as u32);
      self.set_last_measure((clock_last / denom) as u32);
    }

    Ok(())
  }

  // Reads an x4x-format Master block.
  pub(crate) fn read_x4x<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    let data_num = r.read_u16::<LE>()?;
    let rrr = r.read_u16::<LE>()?;
    let event_num = r.read_u32::<LE>()?;

    if data_num != 3 {
      return Err(PxtoneError::UnknownFormat);
    }
    if rrr != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    let mut beat_clock: i32 = EVENT_DEFAULT_BEAT_CLOCK.into();
    let mut beat_num: u8 = EVENT_DEFAULT_BEAT_NUM;
    let mut beat_tempo = EVENT_DEFAULT_BEAT_TEMPO;
    let mut repeat_clock = 0i32;
    let mut last_clock = 0i32;
    let mut absolute = 0i32;

    for _ in 0..event_num {
      let status = r.read_var_u32()?;
      let clock_delta = r.read_var_i32()?;
      let volume = r.read_var_i32()?;
      absolute += clock_delta;
      let clock = absolute;

      match status as u8 {
        EVENT_KIND_BEAT_CLOCK => {
          if clock != 0 {
            return Err(PxtoneError::BrokenFile);
          }
          beat_clock = volume;
        }
        EVENT_KIND_BEAT_TEMPO => {
          if clock != 0 {
            return Err(PxtoneError::BrokenFile);
          }
          beat_tempo = f32::from_bits(volume as u32);
        }
        EVENT_KIND_BEAT_NUM => {
          if clock != 0 {
            return Err(PxtoneError::BrokenFile);
          }
          beat_num = volume as u8;
        }
        EVENT_KIND_REPEAT => {
          if volume != 0 {
            return Err(PxtoneError::BrokenFile);
          }
          repeat_clock = clock;
        }
        EVENT_KIND_LAST => {
          if volume != 0 {
            return Err(PxtoneError::BrokenFile);
          }
          last_clock = clock;
        }
        _ => return Err(PxtoneError::UnknownFormat),
      }
    }

    self.beat_num = beat_num;
    self.beat_tempo = beat_tempo;
    self.beat_clock = beat_clock as u16;

    let denom = beat_num as i32 * beat_clock;
    if denom > 0 {
      self.set_repeat_measure((repeat_clock / denom) as u32);
      self.set_last_measure((last_clock / denom) as u32);
    }

    Ok(())
  }
}
