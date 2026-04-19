use std::io::{Read, Seek};
use byteorder::{LE, ReadBytesExt};
use crate::error::PxtoneError;

// イベント種別
pub const EVENTKIND_NULL       : u8 =  0;
pub const EVENTKIND_ON         : u8 =  1;
pub const EVENTKIND_KEY        : u8 =  2;
pub const EVENTKIND_PAN_VOLUME : u8 =  3;
pub const EVENTKIND_VELOCITY   : u8 =  4;
pub const EVENTKIND_VOLUME     : u8 =  5;
pub const EVENTKIND_PORTAMENT  : u8 =  6;
pub const EVENTKIND_BEATCLOCK  : u8 =  7;
pub const EVENTKIND_BEATTEMPO  : u8 =  8;
pub const EVENTKIND_BEATNUM    : u8 =  9;
pub const EVENTKIND_REPEAT     : u8 = 10;
pub const EVENTKIND_LAST       : u8 = 11;
pub const EVENTKIND_VOICENO    : u8 = 12;
pub const EVENTKIND_GROUPNO    : u8 = 13;
pub const EVENTKIND_TUNING     : u8 = 14;
pub const EVENTKIND_PAN_TIME   : u8 = 15;
pub const EVENTKIND_NUM        : usize = 16;

// デフォルト値
pub const EVENTDEFAULT_VOLUME      : i32 = 104;
pub const EVENTDEFAULT_VELOCITY    : i32 = 104;
pub const EVENTDEFAULT_PAN_VOLUME  : i32 = 64;
pub const EVENTDEFAULT_PAN_TIME    : i32 = 64;
pub const EVENTDEFAULT_PORTAMENT   : i32 = 0;
pub const EVENTDEFAULT_VOICENO     : i32 = 0;
pub const EVENTDEFAULT_GROUPNO     : i32 = 0;
pub const EVENTDEFAULT_KEY         : i32 = 0x6000;
pub const EVENTDEFAULT_BASICKEY    : i32 = 0x4500;
pub const EVENTDEFAULT_TUNING      : f32 = 1.0;

pub const EVENTDEFAULT_BEATNUM     : i32 = 4;
pub const EVENTDEFAULT_BEATTEMPO   : f32 = 120.0;
pub const EVENTDEFAULT_BEATCLOCK   : i32 = 480;

/// イベントが「テール」かどうか（ON と PORTAMENT）
pub fn event_kind_is_tail(kind: u8) -> bool {
    kind == EVENTKIND_ON || kind == EVENTKIND_PORTAMENT
}

/// イベント優先度テーブル
const PRIORITY_TABLE: [i32; EVENTKIND_NUM] = [
      0, // NULL
     50, // ON
     40, // KEY
     60, // PAN_VOLUME
     70, // VELOCITY
     80, // VOLUME
     30, // PORTAMENT
      0, // BEATCLOCK
      0, // BEATTEMPO
      0, // BEATNUM
      0, // REPEAT
    255, // LAST
     10, // VOICENO
     20, // GROUPNO
     90, // TUNING
    100, // PAN_TIME
];

fn compare_priority(kind1: u8, kind2: u8) -> i32 {
    let p1 = if (kind1 as usize) < EVENTKIND_NUM { PRIORITY_TABLE[kind1 as usize] } else { 0 };
    let p2 = if (kind2 as usize) < EVENTKIND_NUM { PRIORITY_TABLE[kind2 as usize] } else { 0 };
    p1 - p2
}

/// イベントレコード
#[derive(Clone, Debug, Default)]
pub struct EventRecord {
    pub kind    : u8,
    pub unit_no : u8,
    pub value   : i32,
    pub clock   : i32,
}

/// イベントリスト（ソート済み双方向リストの代替として Vec を使用）
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

    pub fn records(&self) -> &[EventRecord] {
        &self.events
    }

    pub fn get_max_clock(&self) -> i32 {
        self.events.iter().map(|e| {
            if event_kind_is_tail(e.kind) { e.clock + e.value } else { e.clock }
        }).max().unwrap_or(0)
    }

    /// v5 形式のイベントリストを読み込む（Linear_Start / Linear_Add / Linear_End の相当）
    pub fn read_v5<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
        let _size    = r.read_i32::<LE>()?;
        let eve_num  = r.read_i32::<LE>()?;

        let mut absolute = 0i32;

        for _ in 0..eve_num {
            let clock_delta = read_var_int(r)?;
            let unit_no = r.read_u8()?;
            let kind    = r.read_u8()?;
            let value   = read_var_int(r)?;
            absolute += clock_delta;
            self.events.push(EventRecord { kind, unit_no, value, clock: absolute });
        }

        // 時系列順にソート（priority でタイブレーク）
        self.events.sort_by(|a, b| {
            a.clock.cmp(&b.clock)
                .then_with(|| compare_priority(b.kind, a.kind).cmp(&0))
        });

        Ok(())
    }

    /// v5 形式のイベント数だけカウントしてシーク（プリカウント用）
    pub fn count_v5<R: Read + Seek>(r: &mut R) -> Result<i32, PxtoneError> {
        let _size   = r.read_i32::<LE>()?;
        let eve_num = r.read_i32::<LE>()?;

        for _ in 0..eve_num {
            read_var_int(r)?;
            r.read_u8()?;
            r.read_u8()?;
            read_var_int(r)?;
        }

        Ok(eve_num)
    }

    /// x4x 形式のイベントブロックを読み込む
    pub fn read_x4x_block<R: Read + Seek>(
        &mut self,
        r: &mut R,
        tail_absolute: bool,
        check_rrr: bool,
    ) -> Result<(), PxtoneError> {
        let _size      = r.read_i32::<LE>()?;
        let unit_index = r.read_u16::<LE>()?;
        let event_kind = r.read_u16::<LE>()? as u8;
        let data_num   = r.read_u16::<LE>()?;
        let rrr        = r.read_u16::<LE>()?;
        let event_num  = r.read_u32::<LE>()?;

        if data_num != 2 { return Err(PxtoneError::UnknownFormat); }
        if (event_kind as usize) >= EVENTKIND_NUM { return Err(PxtoneError::UnknownFormat); }
        if check_rrr && rrr != 0 { return Err(PxtoneError::UnknownFormat); }

        let mut absolute = 0i32;

        for _ in 0..event_num {
            let clock_delta = read_var_int(r)?;
            let value       = read_var_int(r)?;
            absolute += clock_delta;
            let clock = absolute;

            self.insert_x4x(clock, unit_index as u8, event_kind, value);

            if tail_absolute && event_kind_is_tail(event_kind) {
                absolute += value;
            }
        }

        Ok(())
    }

    /// x4x 形式のイベント数をカウント（プリカウント用）
    pub fn count_x4x_block<R: Read + Seek>(r: &mut R) -> Result<i32, PxtoneError> {
        let _size      = r.read_i32::<LE>()?;
        let _unit_idx  = r.read_u16::<LE>()?;
        let _kind      = r.read_u16::<LE>()?;
        let data_num   = r.read_u16::<LE>()?;
        let _rrr       = r.read_u16::<LE>()?;
        let event_num  = r.read_u32::<LE>()?;

        if data_num != 2 { return Err(PxtoneError::UnknownFormat); }

        for _ in 0..event_num {
            read_var_int(r)?;
            read_var_int(r)?;
        }

        Ok(event_num as i32)
    }

    /// x4x 形式でイベントを優先度順に挿入する
    fn insert_x4x(&mut self, clock: i32, unit_no: u8, kind: u8, value: i32) {
        let rec = EventRecord { kind, unit_no, value, clock };

        // 同一クロック・ユニット・種別の既存レコードを置換するか、適切な位置に挿入
        let pos = self.events.partition_point(|e| {
            e.clock < clock || (e.clock == clock && compare_priority(kind, e.kind) >= 0)
        });

        // 同一クロック・ユニット・種別があれば置換
        if let Some(existing) = self.events[..pos].iter().rposition(|e| {
            e.clock == clock && e.unit_no == unit_no && e.kind == kind
        }) {
            self.events[existing] = rec;
        } else {
            self.events.insert(pos, rec);
        }
    }

    /// ユニット番号を持つイベントを削除し、後続ユニット番号をデクリメントする
    pub fn remove_unit(&mut self, unit_no: u8) {
        self.events.retain_mut(|e| {
            if e.unit_no == unit_no { return false; }
            if e.unit_no > unit_no { e.unit_no -= 1; }
            true
        });
    }

    /// イベント追加（ソート済みリストに挿入）
    pub fn add_i(&mut self, clock: i32, unit_no: u8, kind: u8, value: i32) {
        self.insert_x4x(clock, unit_no, kind, value);
    }

    pub fn add_f(&mut self, clock: i32, unit_no: u8, kind: u8, value_f: f32) {
        self.add_i(clock, unit_no, kind, value_f.to_bits() as i32);
    }

    pub fn value_change(&mut self, clock1: i32, clock2: i32, unit_no: u8, kind: u8, delta: i32) {
        let (max, min) = match kind {
            EVENTKIND_NULL       => (0, 0),
            EVENTKIND_ON         => (120, 120),
            EVENTKIND_KEY        => (0xbfff, 0),
            EVENTKIND_PAN_VOLUME => (0x80, 0),
            EVENTKIND_PAN_TIME   => (0x80, 0),
            EVENTKIND_VELOCITY   => (0x80, 0),
            EVENTKIND_VOLUME     => (0x80, 0),
            _                    => (0, 0),
        };
        for e in &mut self.events {
            if e.unit_no == unit_no && e.kind == kind && e.clock >= clock1 {
                if clock2 == -1 || e.clock < clock2 {
                    e.value = (e.value + delta).clamp(min, max);
                }
            }
        }
    }
}

// ---- 可変長整数の読み込み ----

/// pxtone 可変長整数（最大 5 バイト）を読み込む
pub fn read_var_int<R: Read>(r: &mut R) -> Result<i32, PxtoneError> {
    let mut bytes = [0u8; 5];
    let mut count = 0usize;

    for i in 0..5 {
        let b = r.read_u8()?;
        bytes[i] = b;
        count = i + 1;
        if b & 0x80 == 0 { break; }
    }

    let value = v_to_int(&bytes[..count]);
    Ok(value as i32)
}

fn v_to_int(bytes: &[u8]) -> u32 {
    let mut b = [0u8; 5];
    match bytes.len() {
        0 => {},
        1 => { b[0] = (bytes[0] & 0x7F) >> 0; },
        2 => {
            b[0] = ((bytes[0] & 0x7F) >> 0) | (bytes[1] << 7);
            b[1] =  (bytes[1] & 0x7F) >> 1;
        },
        3 => {
            b[0] = ((bytes[0] & 0x7F) >> 0) | (bytes[1] << 7);
            b[1] = ((bytes[1] & 0x7F) >> 1) | (bytes[2] << 6);
            b[2] =  (bytes[2] & 0x7F) >> 2;
        },
        4 => {
            b[0] = ((bytes[0] & 0x7F) >> 0) | (bytes[1] << 7);
            b[1] = ((bytes[1] & 0x7F) >> 1) | (bytes[2] << 6);
            b[2] = ((bytes[2] & 0x7F) >> 2) | (bytes[3] << 5);
            b[3] =  (bytes[3] & 0x7F) >> 3;
        },
        _ => {
            b[0] = ((bytes[0] & 0x7F) >> 0) | (bytes[1] << 7);
            b[1] = ((bytes[1] & 0x7F) >> 1) | (bytes[2] << 6);
            b[2] = ((bytes[2] & 0x7F) >> 2) | (bytes[3] << 5);
            b[3] = ((bytes[3] & 0x7F) >> 3) | (bytes[4] << 4);
        },
    }
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
