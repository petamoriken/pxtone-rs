use crate::error::PxtoneError;
use crate::reader::Reader;
use alloc::vec::Vec;

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
  pub(crate) fn read_v5(&mut self, r: &mut Reader<'_>) -> Result<(), PxtoneError> {
    let _size = r.read_i32()?;
    let eve_count = r.read_u32()?;

    let mut absolute = 0i32;

    // Four bytes a record at the very least, two varints and two plain bytes,
    // so the stated count is worth reserving for only as far as the bytes
    // behind it go.
    self
      .events
      .reserve((eve_count as usize).min(r.remaining() / 4));
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

    // No sorting: `Linear_Add_i` appends and `Linear_End` links the records in
    // the order the file stores them, so that is the order the C++ plays them
    // in. The clocks are stored as non-negative deltas, so file order is
    // already chronological; what a sort would change is the order of events
    // that share a tick, and that order is audible. A note-on followed by a key
    // starts the note at the old key and slides to the new one over the
    // portamento, where a key followed by the note-on jumps straight to it.
    Ok(())
  }

  // Reads an x4x-format event block
  pub(crate) fn read_x4x_block(
    &mut self,
    r: &mut Reader<'_>,
    tail_absolute: bool,
    check_rrr: bool,
  ) -> Result<(), PxtoneError> {
    let _size = r.read_i32()?;
    let unit_index = r.read_u16()?;
    let event_kind = r.read_u16()? as u8;
    let data_count = r.read_u16()?;
    let rrr = r.read_u16()?;
    let event_count = r.read_u32()?;

    if data_count != 2 {
      return Err(PxtoneError::UnknownFormat);
    }
    if (event_kind as usize) >= EVENT_KIND_COUNT {
      return Err(PxtoneError::UnknownFormat);
    }
    if check_rrr && rrr != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    // Two varints a record, so a byte each at the very least: a count the file
    // states can be taken at face value only as far as the bytes behind it.
    let capacity = (event_count as usize).min(r.remaining() / 2);
    let mut block: Vec<EventRecord> = Vec::with_capacity(capacity);
    let mut absolute = 0i32;
    let mut ascending = true;

    for _ in 0..event_count {
      let tick_delta = r.read_var_i32()?;
      let value = r.read_var_i32()?;
      absolute += tick_delta;
      let tick = absolute;

      if let Some(prev) = block.last() {
        ascending &= prev.tick <= tick;
      }
      block.push(EventRecord {
        kind: event_kind,
        unit_index: unit_index as u8,
        value,
        tick,
      });

      if tail_absolute && event_kind_is_tail(event_kind) {
        absolute += value;
      }
    }

    if ascending {
      self.merge_x4x(&block);
    } else {
      // The deltas a file stores are non-negative, so this is unreachable for
      // anything the editor wrote; a hand-made one still gets the list it
      // would have got from inserting its records one at a time.
      for rec in &block {
        self.insert_x4x(rec.tick, rec.unit_index, rec.kind, rec.value);
      }
    }

    Ok(())
  }

  // Merges a block of records, all of one unit and kind and in ascending tick
  // order, into the list in a single pass.
  //
  // `insert_x4x` moves the tail of the list for every record it places, which
  // is quadratic over a block: reading `overworld2_orche` moves 17.2M records
  // that way against 0.17M copied here. The result is the same list. A record
  // still lands after every record at its tick that its kind does not outrank,
  // and still replaces the last record at that tick carrying its own unit and
  // kind -- including one this same block just placed, which is how a repeated
  // tick within a block keeps behaving like a replacement.
  fn merge_x4x(&mut self, block: &[EventRecord]) {
    let old = core::mem::take(&mut self.events);
    let mut out = Vec::with_capacity(old.len() + block.len());
    let mut rest = old.as_slice();

    for rec in block {
      // Everything the record sorts after. The list is ordered by tick and
      // then by priority, so this is the same split point `insert_x4x` finds.
      let taken = rest
        .iter()
        .position(|e| {
          !(e.tick < rec.tick || (e.tick == rec.tick && compare_priority(rec.kind, e.kind) >= 0))
        })
        .unwrap_or(rest.len());
      out.extend_from_slice(&rest[..taken]);
      rest = &rest[taken..];

      // Walking back over the run at this tick covers exactly the records
      // `insert_x4x` searches, last one first.
      let mut replaced = None;
      for (i, e) in out.iter().enumerate().rev() {
        if e.tick != rec.tick {
          break;
        }
        if e.unit_index == rec.unit_index && e.kind == rec.kind {
          replaced = Some(i);
          break;
        }
      }
      match replaced {
        Some(i) => out[i] = *rec,
        None => out.push(*rec),
      }
    }

    out.extend_from_slice(rest);
    self.events = out;
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

    // Only the run of records already at `tick` can hold the one to replace, and
    // the list is ordered by tick, so the search starts where that run does
    // rather than at the front of the list.
    let run = self.events[..pos].partition_point(|e| e.tick < tick);
    if let Some(existing) = self.events[run..pos]
      .iter()
      .rposition(|e| e.unit_index == unit_index && e.kind == kind)
    {
      self.events[run + existing] = rec;
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

#[cfg(test)]
mod tests {
  use super::*;

  // Enough of a generator to build blocks with repeated ticks, several units
  // and kinds, and runs that overlap what is already in the list.
  struct Lcg(u32);

  impl Lcg {
    fn next(&mut self, max: u32) -> u32 {
      self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
      (self.0 >> 16) % max
    }
  }

  /// The merge has to place a block exactly where inserting its records one at
  /// a time would have, replacements and all, because two events sharing a tick
  /// are played in list order and that order is audible.
  #[test]
  fn merging_a_block_matches_inserting_its_records() {
    const KINDS: [u8; 5] = [
      EVENT_KIND_ON,
      EVENT_KIND_KEY,
      EVENT_KIND_VOLUME,
      EVENT_KIND_PAN_VOLUME,
      EVENT_KIND_PORTAMENT,
    ];

    let mut rng = Lcg(1);
    for _ in 0..200 {
      let mut merged = EventList::new();
      let mut inserted = EventList::new();

      for _ in 0..rng.next(6) + 1 {
        let unit_index = rng.next(3) as u8;
        let kind = KINDS[rng.next(KINDS.len() as u32) as usize];

        // Ascending ticks, with the odd repeat: that is what a block holds.
        let mut tick = 0i32;
        let mut block = Vec::new();
        for _ in 0..rng.next(8) + 1 {
          tick += rng.next(3) as i32;
          block.push(EventRecord {
            kind,
            unit_index,
            value: rng.next(100) as i32,
            tick,
          });
        }

        merged.merge_x4x(&block);
        for rec in &block {
          inserted.insert_x4x(rec.tick, rec.unit_index, rec.kind, rec.value);
        }
      }

      assert_eq!(merged.records(), inserted.records());
    }
  }
}
