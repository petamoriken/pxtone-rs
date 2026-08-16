//! Compact `f32` maths for `no_std` builds.
//!
//! Stable `core` only has `abs`, `signum`, `copysign`, `min` and `max`, and on
//! wasm the standard library's trigonometry comes from `compiler_builtins`,
//! whose generic argument reduction costs 5.6KB. Everything here widens to
//! `f64`, reduces once and evaluates a Chebyshev fit: more accurate than `f32`
//! needs, a few hundred bytes, and bit-identical on every platform.
//!
//! No FMA (wasm has no scalar one, so `mul_add` would call libm) and no SIMD
//! (the callers are cold table-building loops). [`sqrt`] and [`floor`] are
//! single wasm instructions that stable Rust cannot emit, so `src/wasm.s`
//! spells them out and `build.rs` links that in; other targets use the
//! portable implementations here.

#![no_std]
#![deny(unsafe_code)]

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

/// The `f32.sqrt` and `f32.floor` instructions, assembled by `build.rs` from
/// `src/wasm.s` because stable Rust cannot emit them.
#[cfg(wasm_instructions)]
#[allow(unsafe_code, reason = "calls into the hand written wasm assembly")]
mod wasm {
  unsafe extern "C" {
    safe fn lite_math_sqrt_f32(x: f32) -> f32;
    safe fn lite_math_floor_f32(x: f32) -> f32;
  }

  pub(super) fn sqrt(x: f32) -> f32 {
    lite_math_sqrt_f32(x)
  }

  pub(super) fn floor(x: f32) -> f32 {
    lite_math_floor_f32(x)
  }
}

/// Returns the square root of `x`.
#[cfg(wasm_instructions)]
pub fn sqrt(x: f32) -> f32 {
  wasm::sqrt(x)
}

/// Returns the square root of `x`, by Newton-Raphson in `f64`.
#[cfg(not(wasm_instructions))]
pub fn sqrt(x: f32) -> f32 {
  if x.is_nan() || x < 0.0 {
    return f32::NAN;
  }
  if x == 0.0 || x == f32::INFINITY {
    return x;
  }
  let x = x as f64;
  // Halving the exponent gives a seed within a few percent, and every
  // iteration doubles the number of correct digits from there.
  let mut y = f64::from_bits((x.to_bits() >> 1) + (1023u64 << 51));
  for _ in 0..5 {
    y = 0.5 * (y + x / y);
  }
  y as f32
}

/// Returns the largest integer less than or equal to `x`.
#[cfg(wasm_instructions)]
pub fn floor(x: f32) -> f32 {
  wasm::floor(x)
}

/// Returns the largest integer less than or equal to `x`.
#[cfg(not(wasm_instructions))]
pub fn floor(x: f32) -> f32 {
  // Every f32 of magnitude 2^23 or above is already an integer, which also
  // covers the infinities and NaN.
  if x.is_nan() || x.abs() >= 8_388_608.0 {
    return x;
  }
  let truncated = (x as i32) as f32;
  let floored = if truncated > x {
    truncated - 1.0
  } else {
    truncated
  };
  // `0 as f32` is positive zero, so restore the sign for -0.0 and -0.5..0.
  if floored == 0.0 {
    floored.copysign(x)
  } else {
    floored
  }
}

/// Returns `2` raised to the power of `x`.
pub fn exp2(x: f32) -> f32 {
  exp2_f64(x as f64)
}

/// Shared body of [`exp2`] and [`exp`], taking the exponent in `f64`.
fn exp2_f64(x: f64) -> f32 {
  if x.is_nan() {
    return f32::NAN;
  }
  // f32 saturates outside of this range: 2^128 overflows, and 2^-150 is half
  // of the smallest subnormal.
  if x >= 128.0 {
    return f32::INFINITY;
  }
  if x <= -150.0 {
    return 0.0;
  }

  // 2^x = 2^k * 2^r, with k integral and |r| <= 1/2.
  let k = (x + if x < 0.0 { -0.5 } else { 0.5 }) as i64;
  let r = x - k as f64;
  // 2^k built straight from the exponent field. k stays well inside the f64
  // exponent range because of the checks above.
  let scale = f64::from_bits(((k + 1023) as u64) << 52);
  (exp2_poly(r) * scale) as f32
}

/// Returns `e` raised to the power of `x`.
pub fn exp(x: f32) -> f32 {
  // The conversion to the base two exponent happens in f64: doing it in f32
  // would lose about seven digits of the exponent before exp2 even starts.
  exp2_f64(x as f64 * core::f64::consts::LOG2_E)
}

/// Chebyshev fit of `2^r` over `[-1/2, 1/2]`, accurate to 1.4e-10 relative.
fn exp2_poly(r: f64) -> f64 {
  const E0: f64 = 0.999_999_999_971_765_7;
  const E1: f64 = 0.693_147_180_549_580_2;
  const E2: f64 = 0.240_226_511_018_980_6;
  const E3: f64 = 0.055_504_109_272_240_34;
  const E4: f64 = 0.009_618_039_860_015_862;
  const E5: f64 = 0.001_333_346_349_317_848_7;
  const E6: f64 = 0.000_154_653_420_744_679_9;
  const E7: f64 = 1.530_674_795_324_745e-5;

  E0 + r * (E1 + r * (E2 + r * (E3 + r * (E4 + r * (E5 + r * (E6 + r * E7))))))
}

/// Returns the arctangent of `x`, in radians.
pub fn atan(x: f32) -> f32 {
  /// tan(pi/8)
  const TAN_PI_8: f64 = 0.414_213_562_373_095_1;
  /// tan(3*pi/8)
  const TAN_3PI_8: f64 = 2.414_213_562_373_095;

  if x.is_nan() {
    return x;
  }
  let magnitude = (x as f64).abs();

  // Fold the argument into [0, tan(pi/8)] with the two standard identities.
  let (offset, reduced) = if magnitude > TAN_3PI_8 {
    (core::f64::consts::FRAC_PI_2, -1.0 / magnitude)
  } else if magnitude > TAN_PI_8 {
    (
      core::f64::consts::FRAC_PI_4,
      (magnitude - 1.0) / (magnitude + 1.0),
    )
  } else {
    (0.0, magnitude)
  };

  let value = (offset + atan_poly(reduced)) as f32;
  if x < 0.0 { -value } else { value }
}

/// Chebyshev fit of `atan(t)/t` over `[-tan(pi/8), tan(pi/8)]`, accurate to
/// 5.2e-10 absolute.
fn atan_poly(t: f64) -> f64 {
  const A0: f64 = 0.999_999_998_654_900_5;
  const A1: f64 = -0.333_332_998_986_727;
  const A2: f64 = 0.199_980_142_902_063_04;
  const A3: f64 = -0.142_381_227_922_909_25;
  const A4: f64 = 0.105_665_137_014_591_02;
  const A5: f64 = -0.060_295_272_537_596_964;

  let t2 = t * t;
  t * (A0 + t2 * (A1 + t2 * (A2 + t2 * (A3 + t2 * (A4 + t2 * A5)))))
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
  fn matches_the_reference_square_root() {
    for i in 0..200_000u32 {
      let x = (i as f32) * 0.0625;
      let ours = super::sqrt(x);
      let reference = (x as f64).sqrt() as f32;
      assert!(
        close_enough(ours, reference),
        "sqrt({x}): {ours} vs {reference}"
      );
    }
    for x in [f32::MIN_POSITIVE, 1e-30, 1e30, f32::MAX] {
      let reference = (x as f64).sqrt() as f32;
      assert!(close_enough(super::sqrt(x), reference), "sqrt({x})");
    }
    assert_eq!(super::sqrt(0.0), 0.0);
    assert!(super::sqrt(-1.0).is_nan());
    assert_eq!(super::sqrt(f32::INFINITY), f32::INFINITY);
  }

  #[test]
  fn matches_the_reference_floor() {
    for i in -100_000..100_000i32 {
      let x = (i as f32) * 0.125;
      assert_eq!(super::floor(x), x.floor(), "floor({x})");
    }
    for x in [
      0.0,
      -0.0,
      1e30,
      -1e30,
      f32::MAX,
      f32::INFINITY,
      f32::NEG_INFINITY,
    ] {
      assert_eq!(super::floor(x).to_bits(), x.floor().to_bits(), "floor({x})");
    }
    assert!(super::floor(f32::NAN).is_nan());
  }

  /// Compares against a reference value, allowing a relative error and
  /// requiring an exact match once the reference is not finite.
  fn close_enough(ours: f32, reference: f32) -> bool {
    if !reference.is_finite() {
      return ours.to_bits() == reference.to_bits();
    }
    (ours - reference).abs() <= reference.abs() * TOLERANCE
  }

  #[test]
  fn matches_the_reference_exponentials() {
    for i in -15_000..12_800i32 {
      let x = (i as f32) * 0.01;
      let ours = super::exp2(x);
      let reference = (x as f64).exp2() as f32;
      assert!(
        close_enough(ours, reference),
        "exp2({x}): {ours} vs {reference}"
      );

      let ours = super::exp(x);
      let reference = (x as f64).exp() as f32;
      assert!(
        close_enough(ours, reference),
        "exp({x}): {ours} vs {reference}"
      );
    }
    assert_eq!(super::exp2(0.0), 1.0);
    assert_eq!(super::exp2(10.0), 1024.0);
    assert_eq!(super::exp2(-160.0), 0.0);
    assert_eq!(super::exp2(200.0), f32::INFINITY);
    assert!(super::exp2(f32::NAN).is_nan());
  }

  #[test]
  fn matches_the_reference_arctangent() {
    for i in -200_000..200_000i32 {
      let x = (i as f32) * 0.001;
      let ours = super::atan(x);
      let reference = (x as f64).atan() as f32;
      assert!(
        (ours - reference).abs() <= TOLERANCE,
        "atan({x}): {ours} vs {reference}"
      );
    }
    for x in [0.0, -0.0, 1e20, -1e20, f32::INFINITY, f32::NEG_INFINITY] {
      let reference = (x as f64).atan() as f32;
      assert!((super::atan(x) - reference).abs() <= TOLERANCE, "atan({x})");
    }
    assert!(super::atan(f32::NAN).is_nan());
  }

  #[test]
  fn rejects_arguments_outside_the_domain() {
    assert!(super::sin(f32::NAN).is_nan());
    assert!(super::cos(f32::INFINITY).is_nan());
    assert!(super::sin(1.0e7).is_nan());
  }
}
