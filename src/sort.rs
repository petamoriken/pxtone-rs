//! Compact stable sort.
//!
//! `slice::sort_by` (driftsort) is fast, but its generic machinery costs several
//! kilobytes of code per instantiation, which is a lot for the wasm build.
//! Sorting only happens while loading a file, never while mixing, so a plain
//! run-insertion plus bottom-up merge sort is the better trade-off here.

/// Length of the initial runs built with insertion sort.
const RUN: usize = 16;

/// Sorts `values` stably, keeping the order of elements for which `le` reports
/// that the earlier element is less than or equal to the later one.
pub(crate) fn stable_sort_by<T: Copy>(values: &mut [T], mut le: impl FnMut(&T, &T) -> bool) {
  let len = values.len();
  if len < 2 {
    return;
  }

  let mut start = 0;
  while start < len {
    let end = (start + RUN).min(len);
    insertion_sort(&mut values[start..end], &mut le);
    start = end;
  }
  if len <= RUN {
    return;
  }

  let mut scratch = values.to_vec();
  let mut sorted_in_values = true;
  let mut width = RUN;
  while width < len {
    if sorted_in_values {
      merge_pass(values, &mut scratch, width, &mut le);
    } else {
      merge_pass(&scratch, values, width, &mut le);
    }
    sorted_in_values = !sorted_in_values;
    width *= 2;
  }
  if !sorted_in_values {
    values.copy_from_slice(&scratch);
  }
}

fn insertion_sort<T: Copy>(values: &mut [T], le: &mut impl FnMut(&T, &T) -> bool) {
  for i in 1..values.len() {
    let mut j = i;
    while j > 0 && !le(&values[j - 1], &values[j]) {
      values.swap(j - 1, j);
      j -= 1;
    }
  }
}

/// Merges every pair of adjacent `width` sized runs of `src` into `dst`.
fn merge_pass<T: Copy>(
  src: &[T],
  dst: &mut [T],
  width: usize,
  le: &mut impl FnMut(&T, &T) -> bool,
) {
  let len = src.len();
  let mut start = 0;
  while start < len {
    let mid = (start + width).min(len);
    let end = (start + 2 * width).min(len);

    let (mut left, mut right, mut out) = (start, mid, start);
    while left < mid && right < end {
      if le(&src[left], &src[right]) {
        dst[out] = src[left];
        left += 1;
      } else {
        dst[out] = src[right];
        right += 1;
      }
      out += 1;
    }
    dst[out..out + (mid - left)].copy_from_slice(&src[left..mid]);
    out += mid - left;
    dst[out..end].copy_from_slice(&src[right..end]);

    start = end;
  }
}

#[cfg(test)]
mod tests {
  use super::stable_sort_by;

  #[test]
  fn sorts_stably() {
    // Pairs of (key, original index): a stable sort must keep the indices
    // ascending within each key.
    let mut values: Vec<(u32, usize)> = (0..1000).map(|i| ((i * 7919 % 23) as u32, i)).collect();
    let mut expected = values.clone();
    expected.sort_by_key(|&(key, _)| key);

    stable_sort_by(&mut values, |a, b| a.0 <= b.0);
    assert_eq!(values, expected);
  }

  #[test]
  fn sorts_short_slices() {
    for len in 0..40usize {
      let mut values: Vec<i32> = (0..len as i32).map(|i| (i * 13 % 17) - 8).collect();
      let mut expected = values.clone();
      expected.sort();

      stable_sort_by(&mut values, |a, b| a <= b);
      assert_eq!(values, expected, "len = {len}");
    }
  }
}
