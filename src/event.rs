use crate::error::PxtoneError;
use crate::read_ext::ReadExt;
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek};

// Event kind constants
pub const EVENT_KIND_NULL: u8 = 0;
pub const EVENT_KIND_ON: u8 = 1;
pub const EVENT_KIND_KEY: u8 = 2;
pub const EVENT_KIND_PAN_VOLUME: u8 = 3;
pub const EVENT_KIND_VELOCITY: u8 = 4;
pub const EVENT_KIND_VOLUME: u8 = 5;
pub const EVENT_KIND_PORTAMENT: u8 = 6;
pub const EVENT_KIND_TICKS_PER_BEAT: u8 = 7;
pub const EVENT_KIND_BEAT_TEMPO: u8 = 8;
pub const EVENT_KIND_BEATS_PER_MEASURE: u8 = 9;
pub const EVENT_KIND_REPEAT: u8 = 10;
pub const EVENT_KIND_LAST: u8 = 11;
pub const EVENT_KIND_VOICE_NO: u8 = 12;
pub const EVENT_KIND_GROUP_NO: u8 = 13;
pub const EVENT_KIND_TUNING: u8 = 14;
pub const EVENT_KIND_PAN_TIME: u8 = 15;
pub const EVENT_KIND_COUNT: usize = 16;

// Default values
pub const EVENT_DEFAULT_VOLUME: u32 = 104;
pub const EVENT_DEFAULT_VELOCITY: u32 = 104;
pub const EVENT_DEFAULT_PAN_VOLUME: u32 = 64;
pub const EVENT_DEFAULT_PAN_TIME: u32 = 64;
pub const EVENT_DEFAULT_PORTAMENT: u32 = 0;
pub const EVENT_DEFAULT_VOICE_NO: usize = 0;
pub const EVENT_DEFAULT_GROUP_NO: usize = 0;
pub const EVENT_DEFAULT_KEY: i32 = 0x6000;
pub const EVENT_DEFAULT_BASIC_KEY: u32 = 0x4500;
pub const EVENT_DEFAULT_TUNING: f32 = 1.0;

pub const EVENT_DEFAULT_BEATS_PER_MEASURE: u8 = 4;
pub const EVENT_DEFAULT_BEAT_TEMPO: f32 = 120.0;
pub const EVENT_DEFAULT_TICKS_PER_BEAT: u16 = 480;

// Returns whether an event is a "tail" event (ON and PORTAMENT)
#[inline]
pub(crate) fn event_kind_is_tail(kind: u8) -> bool {
  kind == EVENT_KIND_ON || kind == EVENT_KIND_PORTAMENT
}

// Event priority table
const PRIORITY_TABLE: [u8; EVENT_KIND_COUNT] = [
  0,   // NULL
  50,  // ON
  40,  // KEY
  60,  // PAN_VOLUME
  70,  // VELOCITY
  80,  // VOLUME
  30,  // PORTAMENT
  0,   // TICKS_PER_BEAT
  0,   // BEAT_TEMPO
  0,   // BEATS_PER_MEASURE
  0,   // REPEAT
  255, // LAST
  10,  // VOICE_NO
  20,  // GROUP_NO
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
  pub(crate) unit_index: u8,
  pub(crate) value: i32,
  pub(crate) tick: i32,
}

impl EventRecord {
  /// Event kind. See the `EVENTKIND_*` constants.
  #[inline]
  pub fn kind(&self) -> u8 {
    self.kind
  }

  /// Index of the unit (track) this event belongs to.
  #[inline]
  pub fn unit_index(&self) -> u8 {
    self.unit_index
  }

  /// Event value. Interpretation depends on [`kind`](Self::kind).
  #[inline]
  pub fn value(&self) -> i32 {
    self.value
  }

  /// Tick position at which the event occurs.
  #[inline]
  pub fn tick(&self) -> i32 {
    self.tick
  }
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
  pub fn get_max_tick(&self) -> i32 {
    self
      .events
      .iter()
      .map(|e| {
        if event_kind_is_tail(e.kind) {
          e.tick + e.value
        } else {
          e.tick
        }
      })
      .max()
      .unwrap_or(0)
  }

  // Reads a v5-format event list (equivalent to Linear_Start / Linear_Add / Linear_End)
  pub(crate) fn read_v5<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    let eve_count = r.read_u32::<LE>()?;

    let mut absolute = 0i32;

    for _ in 0..eve_count {
      let tick_delta = r.read_var_i32()?;
      let unit_index = r.read_u8()?;
      let kind = r.read_u8()?;
      let value = r.read_var_i32()?;
      absolute += tick_delta;
      self.events.push(EventRecord {
        kind,
        unit_index,
        value,
        tick: absolute,
      });
    }

    // Sort in chronological order (priority is used as a tiebreaker).
    // Lower numeric priority values should come first for same-tick events.
    self.events.sort_by(|a, b| {
      a.tick
        .cmp(&b.tick)
        .then_with(|| compare_priority(a.kind, b.kind).cmp(&0))
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
    let data_count = r.read_u16::<LE>()?;
    let rrr = r.read_u16::<LE>()?;
    let event_count = r.read_u32::<LE>()?;

    if data_count != 2 {
      return Err(PxtoneError::UnknownFormat);
    }
    if (event_kind as usize) >= EVENT_KIND_COUNT {
      return Err(PxtoneError::UnknownFormat);
    }
    if check_rrr && rrr != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    let mut absolute = 0i32;

    for _ in 0..event_count {
      let tick_delta = r.read_var_i32()?;
      let value = r.read_var_i32()?;
      absolute += tick_delta;
      let tick = absolute;

      self.insert_x4x(tick, unit_index as u8, event_kind, value);

      if tail_absolute && event_kind_is_tail(event_kind) {
        absolute += value;
      }
    }

    Ok(())
  }

  // Inserts an event in x4x format in priority order
  fn insert_x4x(&mut self, tick: i32, unit_index: u8, kind: u8, value: i32) {
    let rec = EventRecord {
      kind,
      unit_index,
      value,
      tick,
    };

    // Replace an existing record with the same tick/unit/kind, or insert at the appropriate position
    let pos = self.events.partition_point(|e| {
      e.tick < tick || (e.tick == tick && compare_priority(kind, e.kind) >= 0)
    });

    // Replace if a record with the same tick/unit/kind already exists
    if let Some(existing) = self.events[..pos]
      .iter()
      .rposition(|e| e.tick == tick && e.unit_index == unit_index && e.kind == kind)
    {
      self.events[existing] = rec;
    } else {
      self.events.insert(pos, rec);
    }
  }

  /// Removes events belonging to the given unit number and decrements subsequent unit numbers
  pub fn remove_unit(&mut self, unit_index: u8) {
    self.events.retain_mut(|e| {
      if e.unit_index == unit_index {
        return false;
      }
      if e.unit_index > unit_index {
        e.unit_index -= 1;
      }
      true
    });
  }

  /// Adds an event (inserts into the sorted list)
  pub fn add_i(&mut self, tick: i32, unit_index: u8, kind: u8, value: i32) {
    self.insert_x4x(tick, unit_index, kind, value);
  }

  /// Adds a floating-point event at the given tick position.
  pub fn add_f(&mut self, tick: i32, unit_index: u8, kind: u8, value_f: f32) {
    self.add_i(tick, unit_index, kind, value_f.to_bits() as i32);
  }

  /// Shifts the value of all matching events in `[tick1, tick2)` by `delta`.
  /// Pass `tick2 = -1` to apply through the end of the song.
  pub fn value_change(&mut self, tick1: i32, tick2: i32, unit_index: u8, kind: u8, delta: i32) {
    let (max, min) = match kind {
      EVENT_KIND_NULL => (0, 0),
      EVENT_KIND_ON => (120, 120),
      EVENT_KIND_KEY => (0xbfff, 0),
      EVENT_KIND_PAN_VOLUME => (0x80, 0),
      EVENT_KIND_PAN_TIME => (0x80, 0),
      EVENT_KIND_VELOCITY => (0x80, 0),
      EVENT_KIND_VOLUME => (0x80, 0),
      _ => (0, 0),
    };
    for e in &mut self.events {
      if e.unit_index == unit_index
        && e.kind == kind
        && e.tick >= tick1
        && (tick2 == -1 || e.tick < tick2)
      {
        e.value = (e.value + delta).clamp(min, max);
      }
    }
  }
}
