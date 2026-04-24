use crate::error::PxtoneError;
use crate::event::{
  EVENTDEFAULT_BEATCLOCK, EVENTDEFAULT_BEATNUM, EVENTDEFAULT_BEATTEMPO, EVENTKIND_BEATCLOCK,
  EVENTKIND_BEATNUM, EVENTKIND_BEATTEMPO, EVENTKIND_LAST, EVENTKIND_REPEAT, read_var_int,
};
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek};

#[derive(Debug)]
pub struct Master {
  pub(crate) beat_num: i32,
  pub(crate) beat_tempo: f32,
  pub(crate) beat_clock: i32,
  pub(crate) meas_num: i32,
  pub(crate) repeat_meas: i32,
  pub(crate) last_meas: i32,
}

impl Default for Master {
  fn default() -> Self {
    Self {
      beat_num: EVENTDEFAULT_BEATNUM,
      beat_tempo: EVENTDEFAULT_BEATTEMPO,
      beat_clock: EVENTDEFAULT_BEATCLOCK,
      meas_num: 1,
      repeat_meas: 0,
      last_meas: 0,
    }
  }
}

impl Master {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn beat_clock(&self) -> i32 {
    self.beat_clock
  }
  pub fn beat_num(&self) -> i32 {
    self.beat_num
  }
  pub fn beat_tempo(&self) -> f32 {
    self.beat_tempo
  }
  pub fn meas_num(&self) -> i32 {
    self.meas_num
  }
  pub fn repeat_meas(&self) -> i32 {
    self.repeat_meas
  }
  pub fn last_meas(&self) -> i32 {
    self.last_meas
  }

  pub fn get_last_clock(&self) -> i32 {
    self.last_meas * self.beat_clock * self.beat_num
  }

  pub fn get_play_meas(&self) -> i32 {
    if self.last_meas != 0 {
      self.last_meas
    } else {
      self.meas_num
    }
  }

  pub fn get_this_clock(&self, meas: i32, beat: i32, clock: i32) -> i32 {
    self.beat_num * self.beat_clock * meas + self.beat_clock * beat + clock
  }

  pub fn adjust_meas_num(&mut self, clock: i32) {
    let b_num = (clock + self.beat_clock - 1) / self.beat_clock;
    let m_num = (b_num + self.beat_num - 1) / self.beat_num;
    if self.meas_num <= m_num {
      self.meas_num = m_num;
    }
    if self.repeat_meas >= self.meas_num {
      self.repeat_meas = 0;
    }
    if self.last_meas > self.meas_num {
      self.last_meas = self.meas_num;
    }
  }

  pub fn set_meas_num(&mut self, mut meas_num: i32) {
    if meas_num < 1 {
      meas_num = 1;
    }
    if meas_num <= self.repeat_meas {
      meas_num = self.repeat_meas + 1;
    }
    if meas_num < self.last_meas {
      meas_num = self.last_meas;
    }
    self.meas_num = meas_num;
  }

  pub fn set_repeat_meas(&mut self, mut meas: i32) {
    if meas < 0 {
      meas = 0;
    }
    self.repeat_meas = meas;
  }

  pub fn set_last_meas(&mut self, mut meas: i32) {
    if meas < 0 {
      meas = 0;
    }
    self.last_meas = meas;
  }

  pub fn set_beat_clock(&mut self, mut beat_clock: i32) {
    if beat_clock < 0 {
      beat_clock = 0;
    }
    self.beat_clock = beat_clock;
  }

  /// Reads a v5-format Master block
  /// Block: u32 size(=15), i16 beat_clock, i8 beat_num, f32 beat_tempo,
  ///        i32 clock_repeat, i32 clock_last
  pub fn read_v5<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let size = r.read_u32::<LE>()?;
    if size != 15 {
      return Err(PxtoneError::UnknownFormat);
    }

    let beat_clock = r.read_i16::<LE>()? as i32;
    let beat_num = r.read_i8()? as i32;
    let beat_tempo = r.read_f32::<LE>()?;
    let clock_repeat = r.read_i32::<LE>()?;
    let clock_last = r.read_i32::<LE>()?;

    self.beat_clock = beat_clock;
    self.beat_num = beat_num;
    self.beat_tempo = beat_tempo;

    let denom = beat_num * beat_clock;
    if denom > 0 {
      self.set_repeat_meas(clock_repeat / denom);
      self.set_last_meas(clock_last / denom);
    }

    Ok(())
  }

  /// Skips a v5-format Master block and returns the event count (constant 5)
  pub fn count_v5<R: Read + Seek>(r: &mut R) -> Result<i32, PxtoneError> {
    let size = r.read_u32::<LE>()?;
    if size != 15 {
      return Err(PxtoneError::UnknownFormat);
    }
    let mut buf = [0u8; 15];
    r.read_exact(&mut buf)?;
    Ok(5)
  }

  /// Reads an x4x-format Master block
  pub fn read_x4x<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
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

    let mut beat_clock = EVENTDEFAULT_BEATCLOCK;
    let mut beat_num = EVENTDEFAULT_BEATNUM;
    let mut beat_tempo = EVENTDEFAULT_BEATTEMPO;
    let mut repeat_clock = 0i32;
    let mut last_clock = 0i32;
    let mut absolute = 0i32;

    for _ in 0..event_num {
      let status = read_var_int(r)?;
      let clock_delta = read_var_int(r)?;
      let volume = read_var_int(r)?;
      absolute += clock_delta;
      let clock = absolute;

      match status as u8 {
        EVENTKIND_BEATCLOCK => {
          if clock != 0 {
            return Err(PxtoneError::BrokenFile);
          }
          beat_clock = volume;
        }
        EVENTKIND_BEATTEMPO => {
          if clock != 0 {
            return Err(PxtoneError::BrokenFile);
          }
          beat_tempo = f32::from_bits(volume as u32);
        }
        EVENTKIND_BEATNUM => {
          if clock != 0 {
            return Err(PxtoneError::BrokenFile);
          }
          beat_num = volume;
        }
        EVENTKIND_REPEAT => {
          if volume != 0 {
            return Err(PxtoneError::BrokenFile);
          }
          repeat_clock = clock;
        }
        EVENTKIND_LAST => {
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
    self.beat_clock = beat_clock;

    let denom = beat_num * beat_clock;
    if denom > 0 {
      self.set_repeat_meas(repeat_clock / denom);
      self.set_last_meas(last_clock / denom);
    }

    Ok(())
  }

  /// Skips an x4x-format Master block and returns the event count
  pub fn count_x4x<R: Read + Seek>(r: &mut R) -> Result<i32, PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    let data_num = r.read_u16::<LE>()?;
    let _rrr = r.read_u16::<LE>()?;
    let event_num = r.read_u32::<LE>()?;

    if data_num != 3 {
      return Err(PxtoneError::UnknownFormat);
    }

    for _ in 0..event_num {
      read_var_int(r)?;
      read_var_int(r)?;
      read_var_int(r)?;
    }

    Ok(event_num as i32)
  }
}
