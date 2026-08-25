//! The maths the decoder needs, under libm's names, for `no_std` builds.
//!
//! `core` has no transcendentals and the ones in `compiler_builtins` cost 5.6KB
//! of generic argument reduction, so `portable` below carries its own for wasm:
//! widen to `f64`, reduce once, evaluate a series. No FMA (wasm has no scalar
//! one) and no SIMD (the callers are cold table-building loops). Every other
//! target calls its libm, which is what the C++ and libvorbis call; `portable`
//! stays compiled under `cfg(test)` there, where the tests hold the libm to it.
//!
//! Only what has a caller is here, so most of it is `f64`: those callers mirror
//! `double` arithmetic in the C. The `f32` three go straight to libm where
//! there is one, which they can afford -- [`cosf`] only feeds a slow reference
//! DCT the tests compare fuzzily, and [`exp2f`] and [`floorf`] are exact for
//! the arguments the decoder passes.

#![no_std]
#![deny(unsafe_code)]

/// The maths the target has. Only wasm lacks a libm.
#[cfg(target_family = "wasm")]
use portable as math;
#[cfg(not(target_family = "wasm"))]
use sys as math;

/// Largest argument [`sin`] and [`cos`] accept: past it the rounding of
/// `n * PI_2_HI` alone outgrows what the low half corrects. The libm builds
/// turn the same arguments away, so that every target answers alike.
const MAX_ARGUMENT: f64 = 1.0e6;

fn in_domain(x: f64) -> bool {
  !x.is_nan() && x.abs() <= MAX_ARGUMENT
}

/// Returns the sine of `x` radians.
///
/// Arguments that are not finite, or whose magnitude exceeds `1e6`, return
/// `NaN`; the decoder never produces those.
#[inline]
pub fn sin(x: f64) -> f64 {
  math::sin(x)
}

/// Returns the cosine of `x` radians. See [`sin`] for the domain.
#[inline]
pub fn cos(x: f64) -> f64 {
  math::cos(x)
}

/// Returns the cosine of `x` radians in `f32`. See [`sin`] for the domain.
#[inline]
pub fn cosf(x: f32) -> f32 {
  math::cosf(x)
}

/// Returns the square root of `x`, correctly rounded.
///
/// The callers mirror a `double` square root in the C, so it has to be:
/// Newton-Raphson in `f64` sits an ulp out for arguments such as 0.125, and
/// closing that costs more than the instruction or the call does.
#[inline]
pub fn sqrt(x: f64) -> f64 {
  math::sqrt(x)
}

/// Returns the largest integer less than or equal to `x`.
#[inline]
pub fn floor(x: f64) -> f64 {
  math::floor(x)
}

/// Returns the largest integer less than or equal to `x`, in `f32`.
#[inline]
pub fn floorf(x: f32) -> f32 {
  math::floorf(x)
}

/// Returns `2` raised to the power of `x`.
#[inline]
pub fn exp2f(x: f32) -> f32 {
  math::exp2f(x)
}

/// Returns `e` raised to the power of `x`.
///
/// Its one caller narrows afterwards, which an `f32` result cannot feed: an
/// error of 1e-10 lands on the wrong side of an `f32` rounding boundary about
/// once in six hundred values.
#[inline]
pub fn exp(x: f64) -> f64 {
  math::exp(x)
}

/// Returns the arctangent of `x`, in radians.
#[inline]
pub fn atan(x: f64) -> f64 {
  math::atan(x)
}

/// The platform's libm, declared by hand because this crate is `no_std`.
#[cfg(not(target_family = "wasm"))]
mod sys {
  #[allow(unsafe_code, reason = "calls the platform's libm")]
  mod libm {
    unsafe extern "C" {
      pub(super) safe fn sin(x: f64) -> f64;
      pub(super) safe fn cos(x: f64) -> f64;
      pub(super) safe fn cosf(x: f32) -> f32;
      pub(super) safe fn sqrt(x: f64) -> f64;
      pub(super) safe fn floor(x: f64) -> f64;
      pub(super) safe fn floorf(x: f32) -> f32;
      pub(super) safe fn exp(x: f64) -> f64;
      pub(super) safe fn exp2f(x: f32) -> f32;
      pub(super) safe fn atan(x: f64) -> f64;
    }
  }

  /// libm has no domain limit; this keeps the portable one, so that a caller
  /// cannot pass wasm an argument only the native build answers.
  #[inline]
  pub(super) fn sin(x: f64) -> f64 {
    if super::in_domain(x) {
      libm::sin(x)
    } else {
      f64::NAN
    }
  }

  #[inline]
  pub(super) fn cos(x: f64) -> f64 {
    if super::in_domain(x) {
      libm::cos(x)
    } else {
      f64::NAN
    }
  }

  #[inline]
  pub(super) fn cosf(x: f32) -> f32 {
    if super::in_domain(x as f64) {
      libm::cosf(x)
    } else {
      f32::NAN
    }
  }

  #[inline]
  pub(super) fn sqrt(x: f64) -> f64 {
    libm::sqrt(x)
  }

  /// LLVM turns these calls back into the rounding instruction where the target
  /// has one.
  #[inline]
  pub(super) fn floor(x: f64) -> f64 {
    libm::floor(x)
  }

  #[inline]
  pub(super) fn floorf(x: f32) -> f32 {
    libm::floorf(x)
  }

  #[inline]
  pub(super) fn exp2f(x: f32) -> f32 {
    libm::exp2f(x)
  }

  #[inline]
  pub(super) fn exp(x: f64) -> f64 {
    libm::exp(x)
  }

  #[inline]
  pub(super) fn atan(x: f64) -> f64 {
    libm::atan(x)
  }
}

/// The maths for wasm, which has no libm worth calling.
///
/// Compiled under `cfg(test)` on the other targets too, where the tests hold
/// their libm to it: the `f64` entry points are worth about 3e-9 relative
/// before narrowing, a good deal finer than the `f32` the callers keep.
#[cfg(any(target_family = "wasm", test))]
mod portable {
  /// 2/pi, used to find the quadrant of the argument.
  const FRAC_2_PI: f64 = 0.636_619_772_367_581_3;
  /// pi/2, split so that `n * PI_2_HI` is exact: the high half carries 33
  /// significant bits, enough for any quadrant index the domain can produce.
  /// Splitting at the full f64 instead leaves the product's own rounding in the
  /// reduced argument, 4e-14 by an argument of 625.
  const PI_2_HI: f64 = 1.570_796_326_734_125_6;
  const PI_2_LO: f64 = 6.077_100_506_506_192e-11;

  #[inline(never)]
  pub(super) fn sin(x: f64) -> f64 {
    match reduce(x) {
      Some((quadrant, r)) => quadrant_sin(quadrant, r),
      None => f64::NAN,
    }
  }

  #[inline(never)]
  pub(super) fn cos(x: f64) -> f64 {
    match reduce(x) {
      // cos(x) == sin(x + pi/2), one quadrant along.
      Some((quadrant, r)) => quadrant_sin(quadrant + 1, r),
      None => f64::NAN,
    }
  }

  /// No `f32` grade fit to go with it: the one caller is cold, and a second
  /// series would be a second thing to keep in step with libm.
  #[inline]
  pub(super) fn cosf(x: f32) -> f32 {
    cos(x as f64) as f32
  }

  /// Splits `x` into a quadrant index and a remainder in `[-pi/4, pi/4]`, so
  /// that `x == quadrant * pi/2 + remainder`. `None` outside the domain.
  fn reduce(x: f64) -> Option<(i64, f64)> {
    if !super::in_domain(x) {
      return None;
    }
    // Round x / (pi/2) to the nearest integer, ties away from zero.
    let scaled = x * FRAC_2_PI;
    let quadrant = (scaled + if scaled < 0.0 { -0.5 } else { 0.5 }) as i64;
    let n = quadrant as f64;
    // Cody-Waite: subtracting the two halves separately keeps the cancellation
    // error out of the reduced argument.
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

  /// Maclaurin series for `sin(r)/r`, truncated where the next term falls under
  /// an `f64` ulp over `[-pi/4, pi/4]`: `r^16/17!` is 6.5e-17 of the result.
  ///
  /// An `f32` grade fit is not enough: [`sin`] hands the `f64` back for the
  /// caller to narrow, and 0.03 `f32` ulp of error lands on the wrong side of
  /// the rounding boundary for one argument in fifty -- which is what kept the
  /// window and MDCT tables off libvorbis'.
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

  /// Maclaurin series for `cos(r)`, truncated the same way: `r^18/18!` is
  /// 2.2e-18 of the result over `[-pi/4, pi/4]`. See [`sin_poly`].
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

  /// `f64.sqrt`, from the `src/wasm.s` that `build.rs` assembles: stable Rust
  /// cannot emit the instruction.
  #[cfg(wasm_instructions)]
  #[allow(unsafe_code, reason = "calls into the hand written wasm assembly")]
  #[inline]
  pub(super) fn sqrt(x: f64) -> f64 {
    unsafe extern "C" {
      safe fn lite_math_sqrt(x: f64) -> f64;
    }
    lite_math_sqrt(x)
  }

  /// Whatever libm the target has, for the builds with no `src/wasm.s`: the
  /// wasm ones `build.rs` found no clang for, and the tests. There is no
  /// portable stand-in -- see [`super::sqrt`] for why.
  #[cfg(not(wasm_instructions))]
  #[allow(unsafe_code, reason = "calls the platform's libm")]
  #[inline]
  pub(super) fn sqrt(x: f64) -> f64 {
    unsafe extern "C" {
      safe fn sqrt(x: f64) -> f64;
    }
    sqrt(x)
  }

  /// `f64.floor`. See [`sqrt`] above for where it comes from.
  #[cfg(wasm_instructions)]
  #[allow(unsafe_code, reason = "calls into the hand written wasm assembly")]
  #[inline]
  pub(super) fn floor(x: f64) -> f64 {
    unsafe extern "C" {
      safe fn lite_math_floor(x: f64) -> f64;
    }
    lite_math_floor(x)
  }

  /// See [`super::floor`]. Reached by the wasm builds with no `src/wasm.s`,
  /// and by the tests.
  #[cfg(not(wasm_instructions))]
  pub(super) fn floor(x: f64) -> f64 {
    // Every f64 of magnitude 2^52 or above is already an integer, which also
    // covers the infinities and NaN.
    if x.is_nan() || x.abs() >= 4_503_599_627_370_496.0 {
      return x;
    }
    let truncated = (x as i64) as f64;
    let floored = if truncated > x {
      truncated - 1.0
    } else {
      truncated
    };
    // `0 as f64` is positive zero, so restore the sign for -0.0 and -0.5..0.
    if floored == 0.0 {
      floored.copysign(x)
    } else {
      floored
    }
  }

  /// `f32.floor`. See [`sqrt`] above for where it comes from.
  #[cfg(wasm_instructions)]
  #[allow(unsafe_code, reason = "calls into the hand written wasm assembly")]
  #[inline]
  pub(super) fn floorf(x: f32) -> f32 {
    unsafe extern "C" {
      safe fn lite_math_floorf(x: f32) -> f32;
    }
    lite_math_floorf(x)
  }

  /// See [`super::floorf`]. Reached by the wasm builds with no `src/wasm.s`,
  /// and by the tests.
  #[cfg(not(wasm_instructions))]
  pub(super) fn floorf(x: f32) -> f32 {
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

  pub(super) fn exp2f(x: f32) -> f32 {
    let x = x as f64;
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

    // 2^x = 2^k * 2^r, with k integral and |r| <= 1/2. 2^k is built straight
    // from the exponent field; the checks above keep k well inside its range.
    let k = (x + if x < 0.0 { -0.5 } else { 0.5 }) as i64;
    let r = x - k as f64;
    let scale = f64::from_bits(((k + 1023) as u64) << 52);
    (exp2_poly(r) * scale) as f32
  }

  #[inline(never)]
  pub(super) fn exp(x: f64) -> f64 {
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

    // e^x = 2^k * e^r with k integral and |r| <= ln(2)/2, and 2^k straight from
    // the exponent field. Splitting ln 2 so that `n * LN_2_HI` is exact -- the
    // high half carries 32 significant bits -- keeps the cancellation error out
    // of `r`; the full f64 would leave 1.8e-15 of it, the whole budget.
    const LN_2_HI: f64 = 0.693_147_180_369_123_8;
    const LN_2_LO: f64 = 1.908_214_929_270_587_7e-10;
    let k = (x * core::f64::consts::LOG2_E + if x < 0.0 { -0.5 } else { 0.5 }) as i64;
    let n = k as f64;
    let r = (x - n * LN_2_HI) - n * LN_2_LO;
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

  #[inline(never)]
  pub(super) fn atan(x: f64) -> f64 {
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

  /// Maclaurin series for `atan(t)/t`, truncated where the next term falls
  /// under an `f64` ulp over `[-tan(pi/8), tan(pi/8)]`: `t^44/45` is 3.2e-19.
  /// It converges slowly enough to want 22 terms where an `f32` result needs
  /// six; the coefficients are exact reciprocals of the odd integers.
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

  /// `portable` hands back what the reduction and the series produce, which is
  /// what the callers mirroring `double` arithmetic in the C want. Both series
  /// run to under an `f64` ulp over the reduced range, so what is left to
  /// measure is the reduction and the Horner sum -- hence the sweep out to the
  /// edge of the domain, where the high half of pi/2 has the least room left.
  #[test]
  fn matches_the_reference_in_f64() {
    const TOLERANCE_F64: f64 = 1.0e-15;

    for x in samples() {
      let x = x as f64;
      let (sine, cosine) = (super::portable::sin(x), super::portable::cos(x));
      assert!((sine - x.sin()).abs() <= TOLERANCE_F64, "sin({x})");
      assert!((cosine - x.cos()).abs() <= TOLERANCE_F64, "cos({x})");
    }
  }

  /// The `f32` entry points are the one place the two backends do their own
  /// thing -- libm's `cosf` against a `f64` cosine narrowed -- so this pins how
  /// far apart they are allowed to drift.
  #[test]
  fn the_backends_agree_on_the_f32_entry_points() {
    let mut worst = 0i32;
    for x in samples() {
      // Exact: both are a power of two for the integral arguments the decoder
      // passes, and floor is exact for everything.
      assert_eq!(
        super::exp2f(x).to_bits(),
        super::portable::exp2f(x).to_bits()
      );
      assert_eq!(
        super::floorf(x).to_bits(),
        super::portable::floorf(x).to_bits()
      );
      let wide = x as f64;
      assert_eq!(
        super::floor(wide).to_bits(),
        super::portable::floor(wide).to_bits()
      );
      let (ours, theirs) = (super::cosf(x), super::portable::cosf(x));
      let apart = (ours.to_bits() as i32 - theirs.to_bits() as i32).abs();
      if ours.is_finite() && apart > worst {
        worst = apart;
      }
    }
    // One ulp, which only the slow reference DCT in lewton would ever see.
    assert!(worst <= 1, "cosf is {worst} ulp from the narrowed cosine");
  }

  #[test]
  fn keeps_the_sign_of_zero() {
    for sin in [super::sin as fn(f64) -> f64, super::portable::sin] {
      assert_eq!(sin(0.0).to_bits(), 0.0f64.to_bits());
      assert_eq!(sin(-0.0).to_bits(), (-0.0f64).to_bits());
    }
    assert_eq!(super::cosf(0.0), 1.0);
    assert_eq!(super::portable::cosf(0.0), 1.0);
  }

  /// `f64.sqrt` is correctly rounded, so this has to be too: the caller mirrors
  /// a `double` square root in the C.
  #[test]
  fn matches_libm_sqrt() {
    let sqrt = super::sqrt;
    for i in 0..200_000u64 {
      let x = (i as f64) * 0.0625;
      assert_eq!(sqrt(x).to_bits(), x.sqrt().to_bits(), "sqrt({x})");
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
      assert_eq!(sqrt(x).to_bits(), x.sqrt().to_bits(), "sqrt({x})");
    }
    for x in [f64::MIN_POSITIVE, 1e-300, 1e300, f64::MAX] {
      assert_eq!(sqrt(x).to_bits(), x.sqrt().to_bits(), "sqrt({x})");
    }
    assert_eq!(sqrt(0.0), 0.0);
    assert!(sqrt(-1.0).is_nan());
    assert_eq!(sqrt(f64::INFINITY), f64::INFINITY);
    // The same call on the targets these run on; only the wasm build with
    // `src/wasm.s` takes the other branch, and that one is an instruction the
    // spec requires to be correctly rounded.
    assert_eq!(
      super::portable::sqrt(2.0).to_bits(),
      2.0f64.sqrt().to_bits()
    );
  }

  #[test]
  fn matches_the_reference_floor() {
    for floor in [super::floorf, super::portable::floorf] {
      for i in -100_000..100_000i32 {
        let x = (i as f32) * 0.125;
        assert_eq!(floor(x), x.floor(), "floor({x})");
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
        assert_eq!(floor(x).to_bits(), x.floor().to_bits(), "floor({x})");
      }
      assert!(floor(f32::NAN).is_nan());
    }

    for floor in [super::floor as fn(f64) -> f64, super::portable::floor] {
      for i in -100_000..100_000i64 {
        let x = (i as f64) * 0.125;
        assert_eq!(floor(x), x.floor(), "floor({x})");
      }
      for x in [
        0.0,
        -0.0,
        1e300,
        -1e300,
        f64::MAX,
        f64::INFINITY,
        f64::NEG_INFINITY,
      ] {
        assert_eq!(floor(x).to_bits(), x.floor().to_bits(), "floor({x})");
      }
      assert!(floor(f64::NAN).is_nan());
    }
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
      let ours = super::portable::exp2f(x);
      let reference = (x as f64).exp2() as f32;
      assert!(
        close_enough(ours, reference),
        "exp2({x}): {ours} vs {reference}"
      );
    }
    for exp2 in [super::exp2f, super::portable::exp2f] {
      assert_eq!(exp2(0.0), 1.0);
      assert_eq!(exp2(10.0), 1024.0);
      assert_eq!(exp2(-160.0), 0.0);
      assert_eq!(exp2(200.0), f32::INFINITY);
      assert!(exp2(f32::NAN).is_nan());
    }
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
      assert!(close(super::portable::exp(x), x.exp()), "exp({x})");
    }
    for i in -400_000..400_000i32 {
      let x = (i as f64) * 0.001;
      assert!(close(super::portable::atan(x), x.atan()), "atan({x})");
    }
    for x in [0.0, -0.0, 1e20, -1e20, f64::INFINITY, f64::NEG_INFINITY] {
      assert!(close(super::portable::atan(x), x.atan()), "atan({x})");
    }
    for (exp, atan) in [
      (super::exp as fn(f64) -> f64, super::atan as fn(f64) -> f64),
      (super::portable::exp, super::portable::atan),
    ] {
      assert_eq!(exp(0.0), 1.0);
      assert_eq!(exp(1000.0), f64::INFINITY);
      assert_eq!(exp(-1000.0), 0.0);
      assert!(exp(f64::NAN).is_nan());
      assert!(atan(f64::NAN).is_nan());
    }
  }

  #[test]
  fn rejects_arguments_outside_the_domain() {
    for (sin, cos) in [
      (super::sin as fn(f64) -> f64, super::cos as fn(f64) -> f64),
      (super::portable::sin, super::portable::cos),
    ] {
      assert!(sin(f64::NAN).is_nan());
      assert!(cos(f64::NAN).is_nan());
      assert!(sin(1.0e7).is_nan());
      assert!(cos(1.0e7).is_nan());
    }
    assert!(super::cosf(1.0e7).is_nan());
    assert!(super::portable::cosf(1.0e7).is_nan());
  }
}
