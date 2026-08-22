// Frequency table (pxtnPulse_Frequency)
//   16 octaves × 12 keys × 16 samples/key = 3072 entries

const OCTAVE_COUNT: usize = 16;
const KEY_PER_OCTAVE: usize = 12;
const FREQUENCY_PER_KEY: usize = 0x10; // 16
const FREQUENCY_PER_OCTAVE: usize = KEY_PER_OCTAVE * FREQUENCY_PER_KEY; // 192
const TABLE_SIZE: usize = OCTAVE_COUNT * FREQUENCY_PER_OCTAVE; // 3072

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

/// Playback rate of the first frequency of each octave, oct 0 being eight
/// octaves below the reference.
const OCTAVE_BASES: [f64; OCTAVE_COUNT] = [
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

/// Fills one octave, each entry being the previous one times `step`.
///
/// This is the same chain of multiplications the C++ walks from the octave base
/// for every entry, only carried across the octave instead of restarted, so the
/// values stay bit identical while the count drops from 294k to 3072 (see
/// `carries_the_octave_bit_identically`). Out of line on purpose: with the
/// length visible LLVM unrolls the loop 64 times, for 1.2KB of wasm.
#[inline(never)]
fn fill_octave(octave: &mut [f32], base: f64, step: f64) {
  let mut work = base;
  for entry in octave {
    *entry = work as f32;
    work *= step;
  }
}

pub(crate) struct FrequencyTable {
  table: Box<[f32; TABLE_SIZE]>,
}

impl FrequencyTable {
  pub(crate) fn new() -> Self {
    let step = get_divide_octave_rate(FREQUENCY_PER_OCTAVE);

    let mut table = Box::new([0.0f32; TABLE_SIZE]);
    let (octaves, _) = table.as_chunks_mut::<FREQUENCY_PER_OCTAVE>();
    for (octave, &base) in octaves.iter_mut().zip(&OCTAVE_BASES) {
      fill_octave(octave, base, step);
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

#[cfg(test)]
mod tests {
  use super::*;

  /// Builds the table the way the C++ does, recomputing every entry from its
  /// octave base, to pin the carried version to the same values.
  fn reference_table(step: f64) -> Vec<f32> {
    (0..TABLE_SIZE)
      .map(|f| {
        let mut work = OCTAVE_BASES[f / FREQUENCY_PER_OCTAVE];
        for _ in 0..f % FREQUENCY_PER_OCTAVE {
          work *= step;
        }
        work as f32
      })
      .collect()
  }

  #[test]
  fn carries_the_octave_bit_identically() {
    let step = get_divide_octave_rate(FREQUENCY_PER_OCTAVE);
    let reference = reference_table(step);

    let table = FrequencyTable::new();
    for (i, expected) in reference.iter().enumerate() {
      assert_eq!(
        table.table[i].to_bits(),
        expected.to_bits(),
        "entry {i}: {} vs {expected}",
        table.table[i]
      );
    }
  }
}
