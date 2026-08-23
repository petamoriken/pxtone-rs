//! Compact `f32` maths for `no_std` builds.
//!
//! Stable `core` only has `abs`, `signum`, `copysign`, `min` and `max`, and on
//! wasm the standard library's trigonometry comes from `compiler_builtins`,
//! whose generic argument reduction costs 5.6KB. Everything here widens to
//! `f64`, reduces once and evaluates a Chebyshev fit: more accurate than `f32`
//! needs, a few hundred bytes, and bit-identical on every platform.
//!
//! No FMA (wasm has no scalar one, so `mul_add` would call libm) and no SIMD
//! (the callers are cold table-building loops). [`sqrt`] and [`floor_f32`] are
//! single wasm instructions that stable Rust cannot emit, so `src/wasm.s`
//! spells them out and `build.rs` links that in; other targets use the
//! portable implementations here.

#![no_std]
#![deny(unsafe_code)]

/// 2/pi, used to find the quadrant of the argument.
const FRAC_2_PI: f64 = 0.636_619_772_367_581_3;
/// pi/2, split so that `n * PI_2_HI` is exact: the high half carries 33
/// significant bits, which leaves room for any quadrant index the domain below
/// can produce. Splitting at the full f64 instead leaves the product's own
/// rounding in the reduced argument -- 4e-14 by an argument of 625, which is
/// far more than the series that follows is worth.
const PI_2_HI: f64 = 1.570_796_326_734_125_6;
const PI_2_LO: f64 = 6.077_100_506_506_192e-11;

/// Largest argument the single reduction step stays accurate for.
///
/// Beyond this the rounding error of `n * PI_2_HI` alone exceeds what the low
/// half can correct. The decoder's arguments stay below a few thousand radians.
const MAX_ARGUMENT: f64 = 1.0e6;

/// Returns the sine of `x` radians.
///
/// Arguments that are not finite, or whose magnitude exceeds `1e6`, return
/// `NaN`; the decoder never produces those.
#[inline]
pub fn sin_f32(x: f32) -> f32 {
  sin(x as f64) as f32
}

/// Returns the sine of `x` radians without narrowing the result.
///
/// The reduction and the polynomial already run in `f64`; this hands back what
/// they produced, accurate to about 3e-9 rather than to an `f64` ulp. Callers
/// that mirror `double` arithmetic in the C++ want this rather than [`sin_f32`],
/// whose `f32` result is only good to 1e-7.
#[inline(never)]
pub fn sin(x: f64) -> f64 {
  match reduce(x) {
    Some((quadrant, r)) => quadrant_sin(quadrant, r),
    None => f64::NAN,
  }
}

/// Returns the cosine of `x` radians. See [`sin_f32`] for the accepted domain.
#[inline]
pub fn cos_f32(x: f32) -> f32 {
  cos(x as f64) as f32
}

/// Returns the cosine of `x` radians without narrowing the result. See
/// [`sin`] for what that is worth.
#[inline(never)]
pub fn cos(x: f64) -> f64 {
  match reduce(x) {
    // cos(x) == sin(x + pi/2), one quadrant along.
    Some((quadrant, r)) => quadrant_sin(quadrant + 1, r),
    None => f64::NAN,
  }
}

/// Returns `(sin(x), cos(x))`, sharing the argument reduction. See [`sin_f32`] for
/// the accepted domain.
#[inline(never)]
pub fn sin_cos_f32(x: f32) -> (f32, f32) {
  let Some((quadrant, r)) = reduce(x as f64) else {
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
fn reduce(x: f64) -> Option<(i64, f64)> {
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

/// Maclaurin series for `sin(r)/r`, truncated where the next term falls under
/// an `f64` ulp over `[-pi/4, pi/4]`: `r^16/17!` is 6.5e-17 of the result.
///
/// A shorter fit is enough for an `f32` result, and this used to carry one, but
/// [`sin`] and [`cos`] hand the `f64` back and callers narrow it
/// themselves. An error of 0.03 `f32` ulp then lands on the wrong side of the
/// rounding boundary for about one argument in fifty, which is what kept the
/// window and MDCT tables from matching the ones libvorbis builds with the
/// platform libm. The exact rational coefficients need no fitting and cost a
/// handful of multiplies in what are all cold table-building loops.
#[inline(never)]
fn sin_poly(r: f64) -> f64 {
  const S0: f64 = 1.0;
  const S1: f64 = -0.166_666_666_666_666_66;
  const S2: f64 = 0.008_333_333_333_333_333;
  const S3: f64 = -0.000_198_412_698_412_698_4;
  const S4: f64 = 2.755_731_922_398_589_3e-6;
  const S5: f64 = -2.505_210_838_544_172e-8;
  const S6: f64 = 1.605_904_383_682_161_3e-10;
  const S7: f64 = -7.647_163_731_819_816e-13;

  let r2 = r * r;
  r * (S0 + r2 * (S1 + r2 * (S2 + r2 * (S3 + r2 * (S4 + r2 * (S5 + r2 * (S6 + r2 * S7)))))))
}

/// Maclaurin series for `cos(r)`, truncated the same way: `r^18/18!` is 2.2e-18
/// of the result over `[-pi/4, pi/4]`. See [`sin_poly`].
#[inline(never)]
fn cos_poly(r: f64) -> f64 {
  const C0: f64 = 1.0;
  const C1: f64 = -0.5;
  const C2: f64 = 0.041_666_666_666_666_664;
  const C3: f64 = -0.001_388_888_888_888_889;
  const C4: f64 = 2.480_158_730_158_73e-5;
  const C5: f64 = -2.755_731_922_398_589e-7;
  const C6: f64 = 2.087_675_698_786_81e-9;
  const C7: f64 = -1.147_074_559_772_972_5e-11;
  const C8: f64 = 4.779_477_332_387_385e-14;

  let r2 = r * r;
  C0 + r2 * (C1 + r2 * (C2 + r2 * (C3 + r2 * (C4 + r2 * (C5 + r2 * (C6 + r2 * (C7 + r2 * C8)))))))
}

/// The `f32.sqrt` and `f32.floor` instructions, assembled by `build.rs` from
/// `src/wasm.s` because stable Rust cannot emit them.
#[cfg(wasm_instructions)]
#[allow(unsafe_code, reason = "calls into the hand written wasm assembly")]
mod wasm {
  unsafe extern "C" {
    safe fn lite_math_sqrt_f64(x: f64) -> f64;
    safe fn lite_math_floor_f32(x: f32) -> f32;
  }

  pub(super) fn sqrt(x: f64) -> f64 {
    lite_math_sqrt_f64(x)
  }

  pub(super) fn floor(x: f32) -> f32 {
    lite_math_floor_f32(x)
  }
}

/// Returns the square root of `x`.
#[cfg(wasm_instructions)]
pub fn sqrt(x: f64) -> f64 {
  wasm::sqrt(x)
}

/// Returns the square root of `x`, from the platform's libm.
///
/// IEEE 754 requires a correctly rounded square root, so this and the
/// `f64.sqrt` above agree on every bit -- which the callers need, because they
/// mirror `double` arithmetic in the C. Newton-Raphson in `f64` does not: its
/// fixed point sits an ulp out for arguments such as 0.125, and closing that
/// costs more code than the one call does.
#[cfg(not(wasm_instructions))]
#[allow(unsafe_code, reason = "calls the platform's libm")]
pub fn sqrt(x: f64) -> f64 {
  unsafe extern "C" {
    safe fn sqrt(x: f64) -> f64;
  }
  sqrt(x)
}

/// Returns the largest integer less than or equal to `x`.
#[cfg(wasm_instructions)]
pub fn floor_f32(x: f32) -> f32 {
  wasm::floor(x)
}

/// Returns the largest integer less than or equal to `x`.
#[cfg(not(wasm_instructions))]
pub fn floor_f32(x: f32) -> f32 {
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
pub fn exp2_f32(x: f32) -> f32 {
  exp2_narrowing(x as f64)
}

/// Shared body of [`exp2_f32`] and [`exp`], taking the exponent in `f64`.
fn exp2_narrowing(x: f64) -> f32 {
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

/// Returns `e` raised to the power of `x`, without narrowing the result.
///
/// Its one caller mirrors a `double` `exp` in the C and narrows afterwards, so
/// the `f32` grade fit behind [`exp2_f32`] is not enough: an error of 1e-10 lands
/// on the wrong side of an `f32` rounding boundary about once in six hundred
/// values.
#[inline(never)]
pub fn exp(x: f64) -> f64 {
  if x.is_nan() {
    return x;
  }
  // f64 saturates outside of this range.
  if x >= 710.0 {
    return f64::INFINITY;
  }
  if x <= -746.0 {
    return 0.0;
  }

  // e^x = 2^k * e^r with k integral and |r| <= ln(2)/2. Subtracting the two
  // halves of ln 2 separately keeps the cancellation error out of `r`, which
  // the series then only has to be accurate about.
  // Split so that `n * LN_2_HI` is exact: the high half carries 32 significant
  // bits, leaving room for any `n` this can produce. Splitting at the full f64
  // instead leaves 1.8e-15 of the product's own rounding in `r`, which is the
  // whole error budget.
  const LN_2_HI: f64 = 0.693_147_180_369_123_8;
  const LN_2_LO: f64 = 1.908_214_929_270_587_7e-10;
  let k = (x * core::f64::consts::LOG2_E + if x < 0.0 { -0.5 } else { 0.5 }) as i64;
  let n = k as f64;
  let r = (x - n * LN_2_HI) - n * LN_2_LO;
  // 2^k straight from the exponent field; k stays inside the range by the
  // checks above.
  let scale = f64::from_bits(((k + 1023) as u64) << 52);
  exp_poly(r) * scale
}

/// Maclaurin series for `e^r`, truncated where the next term falls under an
/// `f64` ulp over `[-ln(2)/2, ln(2)/2]`: `r^14/14!` is 4.1e-18.
#[inline(never)]
fn exp_poly(r: f64) -> f64 {
  const C: [f64; 14] = [
    1.0,
    1.0,
    0.5,
    0.166_666_666_666_666_66,
    0.041_666_666_666_666_664,
    0.008_333_333_333_333_333,
    0.001_388_888_888_888_889,
    0.000_198_412_698_412_698_4,
    2.480_158_730_158_73e-5,
    2.755_731_922_398_589_3e-6,
    2.755_731_922_398_589e-7,
    2.505_210_838_544_172e-8,
    2.087_675_698_786_81e-9,
    1.605_904_383_682_161_3e-10,
  ];
  let mut acc = C[13];
  let mut i = 12;
  loop {
    acc = C[i] + r * acc;
    if i == 0 {
      return acc;
    }
    i -= 1;
  }
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

/// Returns the arctangent of `x`, in radians, without narrowing the result.
#[inline(never)]
pub fn atan(x: f64) -> f64 {
  /// tan(pi/8)
  const TAN_PI_8: f64 = 0.414_213_562_373_095_1;
  /// tan(3*pi/8)
  const TAN_3PI_8: f64 = 2.414_213_562_373_095;

  if x.is_nan() {
    return x;
  }
  let magnitude = x.abs();

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

  let value = offset + atan_poly(reduced);
  if x < 0.0 { -value } else { value }
}

/// Maclaurin series for `atan(t)/t`, truncated where the next term falls under
/// an `f64` ulp over `[-tan(pi/8), tan(pi/8)]`: `t^44/45` is 3.2e-19.
///
/// The series converges slowly enough at the end of that range to want 22
/// terms, where an `f32` result needs six. The coefficients are exact
/// reciprocals of the odd integers, so there is nothing to fit.
#[inline(never)]
fn atan_poly(t: f64) -> f64 {
  const C: [f64; 22] = [
    1.0,
    -0.333_333_333_333_333_3,
    0.2,
    -0.142_857_142_857_142_85,
    0.111_111_111_111_111_1,
    -0.090_909_090_909_090_91,
    0.076_923_076_923_076_93,
    -0.066_666_666_666_666_67,
    0.058_823_529_411_764_705,
    -0.052_631_578_947_368_42,
    0.047_619_047_619_047_616,
    -0.043_478_260_869_565_216,
    0.04,
    -0.037_037_037_037_037_035,
    0.034_482_758_620_689_655,
    -0.032_258_064_516_129_03,
    0.030_303_030_303_030_304,
    -0.028_571_428_571_428_57,
    0.027_027_027_027_027_03,
    -0.025_641_025_641_025_64,
    0.024_390_243_902_439_025,
    -0.023_255_813_953_488_372,
  ];
  let t2 = t * t;
  let mut acc = C[21];
  let mut i = 20;
  loop {
    acc = C[i] + t2 * acc;
    if i == 0 {
      return t * acc;
    }
    i -= 1;
  }
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
      let ours = super::sin_f32(x);
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
      let ours = super::cos_f32(x);
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
      assert_eq!(
        super::sin_cos_f32(x),
        (super::sin_f32(x), super::cos_f32(x)),
        "x = {x}"
      );
    }
  }

  /// The `f64` entry points hand back what the reduction and the series
  /// produce, which is what the callers mirroring `double` arithmetic in the
  /// C want. Both series run to under an `f64` ulp over the reduced range, so
  /// what is left is the reduction and the Horner sum.
  #[test]
  fn matches_the_reference_in_f64() {
    const TOLERANCE_F64: f64 = 1.0e-15;

    for i in -20_000..20_000i32 {
      let x = i as f64 * 0.031_25;
      assert!((super::sin(x) - x.sin()).abs() <= TOLERANCE_F64, "sin({x})");
      assert!((super::cos(x) - x.cos()).abs() <= TOLERANCE_F64, "cos({x})");
    }
    assert!(super::sin(f64::NAN).is_nan());
    assert!(super::cos(1.0e7).is_nan());
  }

  #[test]
  fn keeps_the_sign_of_zero() {
    assert_eq!(super::sin_f32(0.0).to_bits(), 0.0f32.to_bits());
    assert_eq!(super::sin_f32(-0.0).to_bits(), (-0.0f32).to_bits());
    assert_eq!(super::cos_f32(0.0), 1.0);
  }

  /// `f64.sqrt` is correctly rounded, so this has to be too: the caller mirrors
  /// a `double` square root in the C.
  #[test]
  fn matches_libm_sqrt() {
    for i in 0..200_000u64 {
      let x = (i as f64) * 0.0625;
      assert_eq!(super::sqrt(x).to_bits(), x.sqrt().to_bits(), "sqrt({x})");
    }
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    for _ in 0..200_000 {
      state ^= state << 13;
      state ^= state >> 7;
      state ^= state << 17;
      // Mantissas across a wide span of exponents.
      let x = f64::from_bits((state >> 12) | (((state >> 52) & 0x3ff) + 512) << 52);
      if !x.is_finite() {
        continue;
      }
      assert_eq!(super::sqrt(x).to_bits(), x.sqrt().to_bits(), "sqrt({x})");
    }
    for x in [f64::MIN_POSITIVE, 1e-300, 1e300, f64::MAX] {
      assert_eq!(super::sqrt(x).to_bits(), x.sqrt().to_bits(), "sqrt({x})");
    }
    assert_eq!(super::sqrt(0.0), 0.0);
    assert!(super::sqrt(-1.0).is_nan());
    assert_eq!(super::sqrt(f64::INFINITY), f64::INFINITY);
  }

  #[test]
  fn matches_the_reference_floor() {
    for i in -100_000..100_000i32 {
      let x = (i as f32) * 0.125;
      assert_eq!(super::floor_f32(x), x.floor(), "floor({x})");
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
      assert_eq!(
        super::floor_f32(x).to_bits(),
        x.floor().to_bits(),
        "floor({x})"
      );
    }
    assert!(super::floor_f32(f32::NAN).is_nan());
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
      let ours = super::exp2_f32(x);
      let reference = (x as f64).exp2() as f32;
      assert!(
        close_enough(ours, reference),
        "exp2({x}): {ours} vs {reference}"
      );
    }
    assert_eq!(super::exp2_f32(0.0), 1.0);
    assert_eq!(super::exp2_f32(10.0), 1024.0);
    assert_eq!(super::exp2_f32(-160.0), 0.0);
    assert_eq!(super::exp2_f32(200.0), f32::INFINITY);
    assert!(super::exp2_f32(f32::NAN).is_nan());
  }

  /// `exp` and `atan` hand the `f64` back, and their callers mirror
  /// `double` arithmetic in the C, so they are held to a few `f64` ulps rather
  /// than to an `f32` one.
  #[test]
  fn matches_libm_in_f64() {
    /// Room for a handful of ulps of the reduction and the Horner sum.
    const TOLERANCE_F64: f64 = 1.0e-15;

    fn close(ours: f64, reference: f64) -> bool {
      (ours - reference).abs() <= reference.abs().max(1.0) * TOLERANCE_F64
    }

    for i in -70_000..70_000i32 {
      let x = (i as f64) * 0.01;
      assert!(close(super::exp(x), x.exp()), "exp({x})");
    }
    for i in -400_000..400_000i32 {
      let x = (i as f64) * 0.001;
      assert!(close(super::atan(x), x.atan()), "atan({x})");
    }
    for x in [0.0, -0.0, 1e20, -1e20, f64::INFINITY, f64::NEG_INFINITY] {
      assert!(close(super::atan(x), x.atan()), "atan({x})");
    }
    assert_eq!(super::exp(0.0), 1.0);
    assert_eq!(super::exp(1000.0), f64::INFINITY);
    assert_eq!(super::exp(-1000.0), 0.0);
    assert!(super::exp(f64::NAN).is_nan());
    assert!(super::atan(f64::NAN).is_nan());
  }

  #[test]
  fn rejects_arguments_outside_the_domain() {
    assert!(super::sin(f64::NAN).is_nan());
    assert!(super::cos(1.0e7).is_nan());
    assert!(super::sin(1.0e7).is_nan());
  }
}
