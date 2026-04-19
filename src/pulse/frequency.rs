// 周波数テーブル (pxtnPulse_Frequency)
//   16 オクターブ × 12 鍵 × 16 サンプル/鍵 = 3072 エントリ

const OCTAVE_NUM: usize = 16;
const KEY_PER_OCTAVE: usize = 12;
const FREQUENCY_PER_KEY: usize = 0x10; // 16
const TABLE_SIZE: usize = OCTAVE_NUM * KEY_PER_OCTAVE * FREQUENCY_PER_KEY; // 3072

/// 基準インデックス（オクターブ 8 の先頭 = 中央 A あたり）
pub const BASIC_FREQUENCY_INDEX: usize = (OCTAVE_NUM / 2) * KEY_PER_OCTAVE * FREQUENCY_PER_KEY;

/// oct^(1/divi) を高精度で求める（C++ 移植）
fn get_divide_octave_rate(divi: usize) -> f64 {
  let mut parameter = 1.0f64;
  for i in 0..17usize {
    let mut add = 1.0f64;
    for _ in 0..i {
      add *= 0.1;
    }
    let mut j = 0usize;
    loop {
      let work = parameter + add * j as f64;
      let mut result = 1.0f64;
      let mut k = 0usize;
      while k < divi {
        result *= work;
        if result >= 2.0 {
          break;
        }
        k += 1;
      }
      if k != divi {
        break;
      }
      j += 1;
      if j >= 10 {
        break;
      }
    }
    parameter += add * (j as f64 - 1.0);
  }
  parameter
}

pub struct FrequencyTable {
  table: Box<[f32; TABLE_SIZE]>,
}

impl FrequencyTable {
  pub fn new() -> Self {
    let oct_table: [f64; OCTAVE_NUM] = [
      0.00390625, // oct 0  (-8)
      0.0078125,  // oct 1  (-7)
      0.015625,   // oct 2  (-6)
      0.03125,    // oct 3  (-5)
      0.0625,     // oct 4  (-4)
      0.125,      // oct 5  (-3)
      0.25,       // oct 6  (-2)
      0.5,        // oct 7  (-1)
      1.0,        // oct 8  ( 0)
      2.0,        // oct 9  (+1)
      4.0,        // oct 10 (+2)
      8.0,        // oct 11 (+3)
      16.0,       // oct 12 (+4)
      32.0,       // oct 13 (+5)
      64.0,       // oct 14 (+6)
      128.0,      // oct 15 (+7)
    ];

    let step = get_divide_octave_rate(KEY_PER_OCTAVE * FREQUENCY_PER_KEY);

    let mut table = Box::new([0.0f32; TABLE_SIZE]);
    for f in 0..TABLE_SIZE {
      let oct_idx = f / (KEY_PER_OCTAVE * FREQUENCY_PER_KEY);
      let sub_idx = f % (KEY_PER_OCTAVE * FREQUENCY_PER_KEY);
      let mut work = oct_table[oct_idx];
      for _ in 0..sub_idx {
        work *= step;
      }
      table[f] = work as f32;
    }

    Self { table }
  }

  /// key 値（イベントのキー値）から周波数を得る
  pub fn get(&self, key: i32) -> f32 {
    let i = ((key + 0x6000) as usize) * FREQUENCY_PER_KEY / 0x100;
    let i = i.clamp(0, TABLE_SIZE - 1);
    self.table[i]
  }

  /// 生インデックス（key >> 4）から周波数を得る
  pub fn get2(&self, key: i32) -> f32 {
    let i = (key >> 4) as usize;
    let i = i.clamp(0, TABLE_SIZE - 1);
    self.table[i]
  }

  pub fn table_size() -> usize {
    TABLE_SIZE
  }
}
