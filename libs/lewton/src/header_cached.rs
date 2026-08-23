// Vorbis decoder written in Rust
//
// Copyright (c) 2016 est31 <MTest31@outlook.com>
// and contributors. All rights reserved.
// Licensed under MIT license, or Apache 2 license,
// at your option. Please see the LICENSE file
// attached to this source distribution for details.

/*!
Cached header info

This mod contains logic to generate and deal with
data derived from header information
that's used later in the decode process.

The caching is done to speed up decoding.
*/

#[cfg(test)]
#[path = "header_cached_test.rs"]
mod tests;

use alloc::vec::Vec;
#[derive(Clone)]
pub struct CachedBlocksizeDerived {
	/// The three tables libvorbis' `mdct_init` concatenates into one `trig`
	/// array of `n + n/4` floats:
	///
	///   T[i*2]      =  cos((M_PI/n)*(4*i));       i < n/4
	///   T[i*2+1]    = -sin((M_PI/n)*(4*i));
	///   T[n2+i*2]   =  cos((M_PI/(2*n))*(2*i+1)); i < n/4
	///   T[n2+i*2+1] =  sin((M_PI/(2*n))*(2*i+1));
	///   T[n+i*2]    =  cos((M_PI/n)*(4*i+2))*.5;  i < n/8
	///   T[n+i*2+1]  = -sin((M_PI/n)*(4*i+2))*.5;
	///
	/// Computed in f64 and narrowed, as libvorbis does, down to how the
	/// products are grouped: in f32 every entry is up to 1.8e-7 out, which is
	/// enough to move a decoded sample.
	pub trig: Vec<f32>,
	/// libvorbis' `bitrev`, `n/4` entries, read four at a time.
	pub bitrev: Vec<usize>,
	pub window_slope: Vec<f32>,
}

impl CachedBlocksizeDerived {
	pub fn from_blocksize(bs: u8) -> Self {
		CachedBlocksizeDerived {
			window_slope: generate_window(bs),
			trig: compute_trig(bs),
			bitrev: compute_bitreverse(bs),
		}
	}
}

fn compute_trig(bs: u8) -> Vec<f32> {
	let n = 1usize << bs;
	let n2 = n >> 1;
	let pi_n = core::f64::consts::PI / n as f64;
	let pi_2n = core::f64::consts::PI / (2.0 * n as f64);
	let mut t = alloc::vec![0f32; n + n / 4];
	for i in 0..n / 4 {
		let arg = pi_n * (4 * i) as f64;
		t[i * 2] = lite_math::cos(arg) as f32;
		t[i * 2 + 1] = -lite_math::sin(arg) as f32;
		let arg = pi_2n * (2 * i + 1) as f64;
		t[n2 + i * 2] = lite_math::cos(arg) as f32;
		t[n2 + i * 2 + 1] = lite_math::sin(arg) as f32;
	}
	for i in 0..n / 8 {
		let arg = pi_n * (4 * i + 2) as f64;
		t[n + i * 2] = (lite_math::cos(arg) * 0.5) as f32;
		t[n + i * 2 + 1] = (-lite_math::sin(arg) * 0.5) as f32;
	}
	t
}

fn compute_bitreverse(bs: u8) -> Vec<usize> {
	let n = 1usize << bs;
	let log2n = bs as usize;
	let mask = (1usize << (log2n - 1)) - 1;
	let msb = 1usize << (log2n - 2);
	let mut rev = alloc::vec![0usize; n / 4];
	for i in 0..n / 8 {
		let mut acc = 0;
		let mut j = 0;
		while (msb >> j) != 0 {
			if (msb >> j) & i != 0 {
				acc |= 1 << j;
			}
			j += 1;
		}
		// `acc` never carries bit 0 -- `i < n/8` while the shifts run down to
		// bit 1 -- so `!acc & mask` is at least one and the subtraction holds.
		rev[i * 2] = (!acc & mask) - 1;
		rev[i * 2 + 1] = acc;
	}
	rev
}

fn win_slope(x: u16, n: u16) -> f32 {
	// please note that there might be a MISTAKE
	// in how the spec specifies the right window slope
	// function. See "4.3.1. packet type, mode and window decode"
	// step 7 where it adds an "extra" pi/2.
	// The left slope doesn't have it, only the right one.
	// as stb_vorbis shares the window slope generation function,
	// The *other* possible reason is that we don't need the right
	// window for anything. TODO investigate this more.
	// libvorbis computes its window tables in double and stores them as floats.
	// In f32 every window value carries an error of up to 1.8e-7, which is the
	// same order as the decoder's whole disagreement with libvorbis; in f64 it
	// falls below 1e-9.
	let v = lite_math::sin(0.5 * core::f64::consts::PI * (x as f64 + 0.5) / n as f64);
	return lite_math::sin(0.5 * core::f64::consts::PI * v * v) as f32;
}

/// Where libvorbis' window literals differ from the computed value.
///
/// `window.c` carries its tables as decimal literals of eight significant
/// digits -- `0.0753351908F` and the like -- and whatever produced those digits
/// is not this formula in double: 151 of the 8160 entries across the eight
/// blocksizes land on a different float. They are the small values at the head
/// of each table, where a decimal digit buys less than a float ulp, so the
/// absolute error is under 1e-10 and never reached a 16-bit sample. Carrying the
/// whole tables to close it would cost 32 KiB; carrying the differences costs
/// under one.
///
/// Indexed by `blocksize.ilog2() - 6`. `header_cached_test.rs` checks the result
/// against the tables the installed libvorbis returns.
const WINDOW_CORRECTIONS: [&[(u16, f32)]; 8] = [
	// 64
	&[(5, 0.111507304)],
	// 128
	&[(0, 0.000236547203), (1, 0.00212806859), (5, 0.0284463726)],
	// 256
	&[(0, 5.91390017e-05)],
	// 512
	&[
		(0, 1.47848996e-05),
		(1, 0.000133060705),
		(2, 0.000369594607),
		(3, 0.000724350917),
		(5, 0.00178829825),
	],
	// 1024
	&[
		(0, 3.69620011e-06),
		(1, 3.32659001e-05),
		(2, 9.24041015e-05),
		(3, 0.000181108597),
		(4, 0.000299376086),
		(5, 0.000447202096),
		(6, 0.000624581124),
		(16, 0.00402175309),
		(29, 0.0128311533),
		(45, 0.0304055475),
		(181, 0.424804568),
	],
	// 2048
	&[
		(0, 9.24099993e-07),
		(1, 8.31649959e-06),
		(2, 2.31014001e-05),
		(3, 4.52784989e-05),
		(4, 7.48476014e-05),
		(5, 0.000111808498),
		(6, 0.000156160793),
		(7, 0.000207904101),
		(8, 0.000267037889),
		(10, 0.000407474814),
		(11, 0.000488776481),
		(13, 0.000673542672),
		(14, 0.000777005393),
		(19, 0.00140507414),
		(21, 0.00170796504),
		(31, 0.00366472849),
		(38, 0.0054723518),
		(77, 0.0220943298),
		(95, 0.0334638841),
	],
	// 4096
	&[
		(0, 2.31000001e-07),
		(1, 2.07910011e-06),
		(2, 5.77540004e-06),
		(3, 1.13197002e-05),
		(4, 1.87121004e-05),
		(5, 2.79525993e-05),
		(6, 3.90411988e-05),
		(7, 5.19776986e-05),
		(8, 6.67623026e-05),
		(9, 8.33949016e-05),
		(10, 0.000101875303),
		(11, 0.000122203593),
		(12, 0.000144379796),
		(13, 0.000168403698),
		(15, 0.000221994705),
		(16, 0.000251561607),
		(17, 0.000282976107),
		(18, 0.000316238002),
		(19, 0.000351347204),
		(21, 0.000427107589),
		(22, 0.000467758393),
		(23, 0.000510256272),
		(24, 0.00055460108),
		(25, 0.000600792817),
		(28, 0.000750447391),
		(30, 0.000859448977),
		(38, 0.00136928796),
		(40, 0.00151519978),
		(44, 0.00182915654),
		(46, 0.00199719821),
		(97, 0.00876786094),
		(131, 0.0159242768),
		(204, 0.0383191258),
		(316, 0.0906364918),
		(1196, 0.836468458),
	],
	// 8192
	&[
		(0, 5.78000012e-08),
		(1, 5.19799983e-07),
		(2, 1.44379999e-06),
		(3, 2.82990004e-06),
		(4, 4.6780001e-06),
		(5, 6.98819986e-06),
		(6, 9.76039973e-06),
		(7, 1.29945001e-05),
		(8, 1.66907994e-05),
		(9, 2.08490001e-05),
		(10, 2.54692004e-05),
		(11, 3.05514986e-05),
		(12, 3.60958002e-05),
		(13, 4.21020995e-05),
		(14, 4.85704004e-05),
		(15, 5.55006009e-05),
		(16, 6.28928974e-05),
		(17, 7.07471991e-05),
		(18, 7.90635022e-05),
		(19, 8.78416977e-05),
		(20, 9.70819965e-05),
		(21, 0.000106784202),
		(22, 0.0001169483),
		(23, 0.0001275744),
		(24, 0.000138662494),
		(25, 0.000150212596),
		(26, 0.000162224504),
		(27, 0.000174698405),
		(28, 0.000187634301),
		(29, 0.000201032002),
		(30, 0.000214891697),
		(31, 0.000229213198),
		(33, 0.000259242108),
		(34, 0.0002749493),
		(35, 0.000291118398),
		(36, 0.000307749287),
		(38, 0.000342396699),
		(39, 0.000360413193),
		(40, 0.000378891506),
		(41, 0.000397831609),
		(42, 0.000417233503),
		(43, 0.0004370971),
		(44, 0.000457422604),
		(45, 0.000478209811),
		(54, 0.00068607158),
		(55, 0.000711475674),
		(64, 0.000960882986),
		(68, 0.00108372956),
		(72, 0.00121395825),
		(78, 0.00142314017),
		(80, 0.00149655726),
		(81, 0.00153395755),
		(82, 0.00157181895),
		(115, 0.00307977479),
		(116, 0.00313329929),
		(125, 0.00363572361),
		(126, 0.00369384862),
		(134, 0.00417539757),
		(151, 0.00529632019),
		(154, 0.00550790271),
		(162, 0.00609230343),
		(165, 0.00631901808),
		(233, 0.0125614842),
		(240, 0.0133237122),
		(252, 0.01468213),
		(267, 0.0164718982),
		(317, 0.0231708009),
		(323, 0.0240501184),
		(341, 0.0267845802),
		(425, 0.0414434969),
		(964, 0.203841701),
		(1225, 0.316602826),
		(1245, 0.325867385),
		(1499, 0.448106408),
		(1542, 0.469267011),
		(1979, 0.67734915),
	],
];

fn generate_window(bs: u8) -> Vec<f32> {
	let n = (1u16 << bs) >> 1;
	let mut window = Vec::with_capacity(n as usize);
	for i in 0..n {
		window.push(win_slope(i, n));
	}
	if let Some(corrections) = WINDOW_CORRECTIONS.get((bs as usize).wrapping_sub(6)) {
		for &(index, value) in *corrections {
			window[index as usize] = value;
		}
	}
	return window;
}

/// libvorbis' `toBARK`, in the precision the C gives it:
///
///   13.1f*atan(.00074f*(n)) + 2.24f*atan((n)*(n)*1.85e-8f) + 1e-4f*(n)
///
/// The argument and the three products with `f` suffixed constants are `f32`;
/// the arctangents, and so the sum, are `f64`. The `f` suffixes matter -- `13.1f`
/// promotes to 13.100000381469727, not to 13.1 -- and so does the precision:
/// the result is floored to an integer bin, so an `f32` arctangent moves a bin
/// boundary rather than the last bit of a sample.
fn to_bark(x: f32) -> f64 {
	13.1f32 as f64 * lite_math::atan((0.00074f32 * x) as f64)
		+ 2.24f32 as f64 * lite_math::atan((x * x * 1.85e-8f32) as f64)
		+ (1.0e-4f32 * x) as f64
}

/// The bark scale bin each spectral line falls in, as `floor0_map_lazy_init`
/// builds it:
///
///   float scale = look->ln / toBARK(info->rate/2.f);
///   int val = floor( toBARK((info->rate/2.f)/n*j) * scale );
///   if( val >= look->ln ) val = look->ln - 1;
///
/// `scale` is narrowed to `f32` on the way in, and the product it goes into is
/// `f64`. Runs of equal bins share one curve value, which is why the decoder
/// keeps the bins rather than a cosine per line.
pub fn compute_bark_map(n: u16, floor0_rate: u16, floor0_bark_map_size: u16) -> Vec<i32> {
	let ln = floor0_bark_map_size as i32;
	let half_rate = floor0_rate as f32 / 2.0;
	let step = half_rate / n as f32;
	let scale = (ln as f64 / to_bark(half_rate)) as f32;
	let mut map = Vec::with_capacity(n as usize);
	for j in 0..n {
		// Non-negative, so truncating is the floor the C takes.
		let val = (to_bark(step * j as f32) * scale as f64) as i32;
		map.push(if val >= ln { ln - 1 } else { val });
	}
	map
}
