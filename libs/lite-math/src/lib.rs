//! Compact sine and cosine for `no_std` builds.
//!
//! On wasm the standard library's trigonometry comes from `compiler_builtins`,
//! whose generic argument reduction costs 5.6KB. Sine and cosine here widen to
//! `f64`, reduce once and evaluate a Chebyshev fit: more accurate than `f32`
//! needs, a few hundred bytes, and bit-identical on every platform.
//!
//! No FMA (wasm has no scalar one, so `mul_add` would call libm) and no SIMD
//! (the callers are cold table-building loops).

#![no_std]
#![forbid(unsafe_code)]

/// 2/pi, used to find the quadrant of the argument.
const FRAC_2_PI: f64 = 0.636_619_772_367_581_3;
/// pi/2, split so that the reduction keeps the bits `PI_2_HI` cannot hold.
const PI_2_HI: f64 = core::f64::consts::FRAC_PI_2;
const PI_2_LO: f64 = 6.123_233_995_736_766e-17;

/// Largest argument the single reduction step stays accurate for.
///
/// Beyond this the rounding error of `n * PI_2_HI` alone exceeds what the low
/// half can correct. The decoder's arguments stay below a few thousand radians.
const MAX_ARGUMENT: f64 = 1.0e6;

/// Returns the sine of `x` radians.
///
/// Arguments that are not finite, or whose magnitude exceeds `1e6`, return
/// `NaN`; the decoder never produces those.
#[inline(never)]
pub fn sin(x: f32) -> f32 {
  match reduce(x) {
    Some((quadrant, r)) => quadrant_sin(quadrant, r) as f32,
    None => f32::NAN,
  }
}

/// Returns the cosine of `x` radians. See [`sin`] for the accepted domain.
#[inline(never)]
pub fn cos(x: f32) -> f32 {
  match reduce(x) {
    // cos(x) == sin(x + pi/2), one quadrant along.
    Some((quadrant, r)) => quadrant_sin(quadrant + 1, r) as f32,
    None => f32::NAN,
  }
}

/// Returns `(sin(x), cos(x))`, sharing the argument reduction. See [`sin`] for
/// the accepted domain.
#[inline(never)]
pub fn sin_cos(x: f32) -> (f32, f32) {
  let Some((quadrant, r)) = reduce(x) else {
    return (f32::NAN, f32::NAN);
  };
  // Odd quadrants exchange the two polynomials, and each result is negative in
  // two of the four quadrants. Both selections compile to value selects.
  let (sine, cosine) = (sin_poly(r), cos_poly(r));
  let swapped = quadrant & 1 != 0;
  (
    negate_if(if swapped { cosine } else { sine }, quadrant & 2 != 0) as f32,
    negate_if(if swapped { sine } else { cosine }, (quadrant + 1) & 2 != 0) as f32,
  )
}

/// Splits `x` into a quadrant index and a remainder in `[-pi/4, pi/4]`, so that
/// `x == quadrant * pi/2 + remainder`. Returns `None` outside the domain.
fn reduce(x: f32) -> Option<(i64, f64)> {
  let x = x as f64;
  if x.is_nan() || x.abs() > MAX_ARGUMENT {
    return None;
  }
  // Round x / (pi/2) to the nearest integer, ties away from zero.
  let scaled = x * FRAC_2_PI;
  let quadrant = (scaled + if scaled < 0.0 { -0.5 } else { 0.5 }) as i64;
  let n = quadrant as f64;
  // Cody-Waite: subtracting the two halves of pi/2 separately keeps the
  // cancellation error out of the reduced argument.
  Some((quadrant, (x - n * PI_2_HI) - n * PI_2_LO))
}

/// Evaluates `sin(quadrant * pi/2 + r)`, using only the polynomial it needs.
#[inline(never)]
fn quadrant_sin(quadrant: i64, r: f64) -> f64 {
  match quadrant & 3 {
    0 => sin_poly(r),
    1 => cos_poly(r),
    2 => -sin_poly(r),
    _ => -cos_poly(r),
  }
}

/// Flips the sign bit of `value` when `negate` holds.
fn negate_if(value: f64, negate: bool) -> f64 {
  f64::from_bits(value.to_bits() ^ ((negate as u64) << 63))
}

/// Chebyshev fit of `sin(r)/r` over `[-pi/4, pi/4]`: 3.1e-9, or 0.03 f32 ulp.
#[inline(never)]
fn sin_poly(r: f64) -> f64 {
  const S0: f64 = 0.999_999_996_945_006;
  const S1: f64 = -0.166_666_507_065_048_93;
  const S2: f64 = 0.008_332_036_654_645_55;
  const S3: f64 = -0.000_195_039_634_174_997_2;

  let r2 = r * r;
  r * (S0 + r2 * (S1 + r2 * (S2 + r2 * S3)))
}

/// Chebyshev fit of `cos(r)` over `[-pi/4, pi/4]`: 4.8e-11.
#[inline(never)]
fn cos_poly(r: f64) -> f64 {
  const C0: f64 = 0.999_999_999_953_015_9;
  const C1: f64 = -0.499_999_996_159_102_57;
  const C2: f64 = 0.041_666_616_745_679_55;
  const C3: f64 = -0.001_388_661_892_894_610_3;
  const C4: f64 = 2.437_988_057_747_178e-5;

  let r2 = r * r;
  C0 + r2 * (C1 + r2 * (C2 + r2 * (C3 + r2 * C4)))
}

#[cfg(test)]
mod tests {
  extern crate std;

  use std::vec::Vec;

  /// One `f32` ulp near 1.0; the fits stay far inside this.
  const TOLERANCE: f32 = 1.2e-7;

  /// Arguments covering the ranges the decoder actually uses, plus extremes.
  fn samples() -> Vec<f32> {
    let mut values = Vec::new();
    for i in -20_000..20_000i32 {
      values.push(i as f32 * 0.031_25);
    }
    for i in 0..2000i32 {
      values.push(i as f32 * 499.5);
    }
    values.push(0.0);
    values.push(-0.0);
    values.push(1.0e6);
    values.push(-1.0e6);
    values
  }

  #[test]
  fn matches_the_reference_sine() {
    for x in samples() {
      let ours = super::sin(x);
      let reference = (x as f64).sin() as f32;
      assert!(
        (ours - reference).abs() <= TOLERANCE,
        "sin({x}): {ours} vs {reference}"
      );
    }
  }

  #[test]
  fn matches_the_reference_cosine() {
    for x in samples() {
      let ours = super::cos(x);
      let reference = (x as f64).cos() as f32;
      assert!(
        (ours - reference).abs() <= TOLERANCE,
        "cos({x}): {ours} vs {reference}"
      );
    }
  }

  #[test]
  fn sin_cos_matches_the_separate_functions() {
    for x in samples() {
      assert_eq!(super::sin_cos(x), (super::sin(x), super::cos(x)), "x = {x}");
    }
  }

  #[test]
  fn keeps_the_sign_of_zero() {
    assert_eq!(super::sin(0.0).to_bits(), 0.0f32.to_bits());
    assert_eq!(super::sin(-0.0).to_bits(), (-0.0f32).to_bits());
    assert_eq!(super::cos(0.0), 1.0);
  }

  #[test]
  fn rejects_arguments_outside_the_domain() {
    assert!(super::sin(f32::NAN).is_nan());
    assert!(super::cos(f32::INFINITY).is_nan());
    assert!(super::sin(1.0e7).is_nan());
  }
}
