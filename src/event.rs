use crate::error::PxtoneError;
use crate::read_ext::ReadExt;
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek};

// Event kind constants
pub const EVENTKIND_NULL: u8 = 0;
pub const EVENTKIND_ON: u8 = 1;
pub const EVENTKIND_KEY: u8 = 2;
pub const EVENTKIND_PAN_VOLUME: u8 = 3;
pub const EVENTKIND_VELOCITY: u8 = 4;
pub const EVENTKIND_VOLUME: u8 = 5;
pub const EVENTKIND_PORTAMENT: u8 = 6;
pub const EVENTKIND_BEATCLOCK: u8 = 7;
pub const EVENTKIND_BEATTEMPO: u8 = 8;
pub const EVENTKIND_BEATNUM: u8 = 9;
pub const EVENTKIND_REPEAT: u8 = 10;
pub const EVENTKIND_LAST: u8 = 11;
pub const EVENTKIND_VOICENO: u8 = 12;
pub const EVENTKIND_GROUPNO: u8 = 13;
pub const EVENTKIND_TUNING: u8 = 14;
pub const EVENTKIND_PAN_TIME: u8 = 15;
pub const EVENTKIND_NUM: usize = 16;

// Default values
pub const EVENTDEFAULT_VOLUME: u32 = 104;
pub const EVENTDEFAULT_VELOCITY: u32 = 104;
pub const EVENTDEFAULT_PAN_VOLUME: u32 = 64;
pub const EVENTDEFAULT_PAN_TIME: u32 = 64;
pub const EVENTDEFAULT_PORTAMENT: u32 = 0;
pub const EVENTDEFAULT_VOICENO: usize = 0;
pub const EVENTDEFAULT_GROUPNO: usize = 0;
pub const EVENTDEFAULT_KEY: i32 = 0x6000;
pub const EVENTDEFAULT_BASICKEY: i32 = 0x4500;
pub const EVENTDEFAULT_TUNING: f32 = 1.0;

pub const EVENTDEFAULT_BEATNUM: u8 = 4;
pub const EVENTDEFAULT_BEATTEMPO: f32 = 120.0;
pub const EVENTDEFAULT_BEATCLOCK: u16 = 480;

// Returns whether an event is a "tail" event (ON and PORTAMENT)
#[inline]
pub(crate) fn event_kind_is_tail(kind: u8) -> bool {
  kind == EVENTKIND_ON || kind == EVENTKIND_PORTAMENT
}

// Event priority table
const PRIORITY_TABLE: [u8; EVENTKIND_NUM] = [
  0,   // NULL
  50,  // ON
  40,  // KEY
  60,  // PAN_VOLUME
  70,  // VELOCITY
  80,  // VOLUME
  30,  // PORTAMENT
  0,   // BEATCLOCK
  0,   // BEATTEMPO
  0,   // BEATNUM
  0,   // REPEAT
  255, // LAST
  10,  // VOICENO
  20,  // GROUPNO
  90,  // TUNING
  100, // PAN_TIME
];

#[inline]
fn compare_priority(kind1: u8, kind2: u8) -> i16 {
  let p1 = PRIORITY_TABLE.get(kind1 as usize).copied().unwrap_or(0) as i16;
  let p2 = PRIORITY_TABLE.get(kind2 as usize).copied().unwrap_or(0) as i16;
  p1 - p2
}

/// A single automation event in a pxtone song.
#[derive(Clone, Debug, Default)]
pub struct EventRecord {
  pub(crate) kind: u8,
  pub(crate) unit_no: u8,
  pub(crate) value: i32,
  pub(crate) clock: i32,
}

/// The chronologically ordered list of automation events for a song.
#[derive(Debug, Default)]
pub struct EventList {
  events: Vec<EventRecord>,
}

impl EventList {
  pub fn new() -> Self {
    Self { events: Vec::new() }
  }

  pub fn clear(&mut self) {
    self.events.clear();
  }

  /// Returns all events in chronological order.
  #[inline]
  pub fn records(&self) -> &[EventRecord] {
    &self.events
  }

  /// Returns the tick position of the last event, including note durations.
  pub fn get_max_clock(&self) -> i32 {
    self
      .events
      .iter()
      .map(|e| {
        if event_kind_is_tail(e.kind) {
          e.clock + e.value
        } else {
          e.clock
        }
      })
      .max()
      .unwrap_or(0)
  }

  // Reads a v5-format event list (equivalent to Linear_Start / Linear_Add / Linear_End)
  pub(crate) fn read_v5<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    let eve_num = r.read_u32::<LE>()?;

    let mut absolute = 0i32;

    for _ in 0..eve_num {
      let clock_delta = r.read_var_int()?;
      let unit_no = r.read_u8()?;
      let kind = r.read_u8()?;
      let value = r.read_var_int()?;
      absolute += clock_delta;
      self.events.push(EventRecord {
        kind,
        unit_no,
        value,
        clock: absolute,
      });
    }

    // Sort in chronological order (priority is used as a tiebreaker)
    self.events.sort_by(|a, b| {
      a.clock
        .cmp(&b.clock)
        .then_with(|| compare_priority(b.kind, a.kind).cmp(&0))
    });

    Ok(())
  }

  // Reads an x4x-format event block
  pub(crate) fn read_x4x_block<R: Read + Seek>(
    &mut self,
    r: &mut R,
    tail_absolute: bool,
    check_rrr: bool,
  ) -> Result<(), PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    let unit_index = r.read_u16::<LE>()?;
    let event_kind = r.read_u16::<LE>()? as u8;
    let data_num = r.read_u16::<LE>()?;
    let rrr = r.read_u16::<LE>()?;
    let event_num = r.read_u32::<LE>()?;

    if data_num != 2 {
      return Err(PxtoneError::UnknownFormat);
    }
    if (event_kind as usize) >= EVENTKIND_NUM {
      return Err(PxtoneError::UnknownFormat);
    }
    if check_rrr && rrr != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    let mut absolute = 0i32;

    for _ in 0..event_num {
      let clock_delta = r.read_var_int()?;
      let value = r.read_var_int()?;
      absolute += clock_delta;
      let clock = absolute;

      self.insert_x4x(clock, unit_index as u8, event_kind, value);

      if tail_absolute && event_kind_is_tail(event_kind) {
        absolute += value;
      }
    }

    Ok(())
  }

  // Inserts an event in x4x format in priority order
  fn insert_x4x(&mut self, clock: i32, unit_no: u8, kind: u8, value: i32) {
    let rec = EventRecord {
      kind,
      unit_no,
      value,
      clock,
    };

    // Replace an existing record with the same clock/unit/kind, or insert at the appropriate position
    let pos = self.events.partition_point(|e| {
      e.clock < clock || (e.clock == clock && compare_priority(kind, e.kind) >= 0)
    });

    // Replace if a record with the same clock/unit/kind already exists
    if let Some(existing) = self.events[..pos]
      .iter()
      .rposition(|e| e.clock == clock && e.unit_no == unit_no && e.kind == kind)
    {
      self.events[existing] = rec;
    } else {
      self.events.insert(pos, rec);
    }
  }

  /// Removes events belonging to the given unit number and decrements subsequent unit numbers
  pub fn remove_unit(&mut self, unit_no: u8) {
    self.events.retain_mut(|e| {
      if e.unit_no == unit_no {
        return false;
      }
      if e.unit_no > unit_no {
        e.unit_no -= 1;
      }
      true
    });
  }

  /// Adds an event (inserts into the sorted list)
  pub fn add_i(&mut self, clock: i32, unit_no: u8, kind: u8, value: i32) {
    self.insert_x4x(clock, unit_no, kind, value);
  }

  /// Adds a floating-point event at the given tick clock.
  pub fn add_f(&mut self, clock: i32, unit_no: u8, kind: u8, value_f: f32) {
    self.add_i(clock, unit_no, kind, value_f.to_bits() as i32);
  }

  /// Shifts the value of all matching events in `[clock1, clock2)` by `delta`.
  /// Pass `clock2 = -1` to apply through the end of the song.
  pub fn value_change(&mut self, clock1: i32, clock2: i32, unit_no: u8, kind: u8, delta: i32) {
    let (max, min) = match kind {
      EVENTKIND_NULL => (0, 0),
      EVENTKIND_ON => (120, 120),
      EVENTKIND_KEY => (0xbfff, 0),
      EVENTKIND_PAN_VOLUME => (0x80, 0),
      EVENTKIND_PAN_TIME => (0x80, 0),
      EVENTKIND_VELOCITY => (0x80, 0),
      EVENTKIND_VOLUME => (0x80, 0),
      _ => (0, 0),
    };
    for e in &mut self.events {
      if e.unit_no == unit_no
        && e.kind == kind
        && e.clock >= clock1
        && (clock2 == -1 || e.clock < clock2)
      {
        e.value = (e.value + delta).clamp(min, max);
      }
    }
  }
}
