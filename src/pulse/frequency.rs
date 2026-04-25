// Frequency table (pxtnPulse_Frequency)
//   16 octaves × 12 keys × 16 samples/key = 3072 entries

const OCTAVE_NUM: usize = 16;
const KEY_PER_OCTAVE: usize = 12;
const FREQUENCY_PER_KEY: usize = 0x10; // 16
const TABLE_SIZE: usize = OCTAVE_NUM * KEY_PER_OCTAVE * FREQUENCY_PER_KEY; // 3072

/// Computes oct^(1/divi) with high precision (ported from C++)
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
  pub(crate) fn new() -> Self {
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

  /// Returns the frequency for a key value (event key)
  #[inline]
  pub(crate) fn get(&self, key: i32) -> f32 {
    let i = key.saturating_add(0x6000) / 0x10;
    let i = i.clamp(0, (TABLE_SIZE - 1) as i32) as usize;
    self.table[i]
  }

  /// Returns the frequency for a raw index (key >> 4)
  #[inline]
  pub(crate) fn get2(&self, key: i32) -> f32 {
    let i = (key >> 4).clamp(0, (TABLE_SIZE - 1) as i32) as usize;
    self.table[i]
  }
}
