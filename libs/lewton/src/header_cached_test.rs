// Vorbis decoder written in Rust
//
// Copyright (c) 2016 est31 <MTest31@outlook.com>
// and contributors. All rights reserved.
// Licensed under MIT license, or Apache 2 license,
// at your option. Please see the LICENSE file
// attached to this source distribution for details.

/*!
The IMDCT and its tables, held against libvorbis

Any correct IMDCT gives the same transform; only the same operation order gives
the same floats. The values here are what libvorbis 1.3.7 produces for the same
input, which is what makes a decoded OGGV voice agree with `ov_read` bit for bit.
They were taken two ways: the tables by reading `mdct_init` back through the
symbols the installed dylib exports, and the transform by compiling
`lib/mdct.c` with `-ffp-contract=off`, so that -- as in the C, and as here --
every multiply and add rounds to `f32` on its own. A build that fuses them
answers differently, which is why the reference is the source and not a binary.
*/

use super::CachedBlocksizeDerived;
use crate::imdct::inverse_mdct;
use alloc::vec;

/// libvorbis' `trig` for blocksize 64, written out so a change to the table
/// arithmetic shows up as a value rather than as a hash.
const TRIG_64: &[f32] = &[
	1.000000000e+00,
	-0.000000000e+00,
	9.807852507e-01,
	-1.950903237e-01,
	9.238795042e-01,
	-3.826834261e-01,
	8.314695954e-01,
	-5.555702448e-01,
	7.071067691e-01,
	-7.071067691e-01,
	5.555702448e-01,
	-8.314695954e-01,
	3.826834261e-01,
	-9.238795042e-01,
	1.950903237e-01,
	-9.807852507e-01,
	6.123234263e-17,
	-1.000000000e+00,
	-1.950903237e-01,
	-9.807852507e-01,
	-3.826834261e-01,
	-9.238795042e-01,
	-5.555702448e-01,
	-8.314695954e-01,
	-7.071067691e-01,
	-7.071067691e-01,
	-8.314695954e-01,
	-5.555702448e-01,
	-9.238795042e-01,
	-3.826834261e-01,
	-9.807852507e-01,
	-1.950903237e-01,
	9.996988177e-01,
	2.454122901e-02,
	9.972904325e-01,
	7.356456667e-02,
	9.924795628e-01,
	1.224106774e-01,
	9.852776527e-01,
	1.709618866e-01,
	9.757021070e-01,
	2.191012353e-01,
	9.637760520e-01,
	2.667127550e-01,
	9.495281577e-01,
	3.136817515e-01,
	9.329928160e-01,
	3.598950505e-01,
	9.142097831e-01,
	4.052413106e-01,
	8.932242990e-01,
	4.496113360e-01,
	8.700869679e-01,
	4.928981960e-01,
	8.448535800e-01,
	5.349976420e-01,
	8.175848126e-01,
	5.758081675e-01,
	7.883464098e-01,
	6.152315736e-01,
	7.572088242e-01,
	6.531728506e-01,
	7.242470980e-01,
	6.895405650e-01,
	4.975923598e-01,
	-4.900857061e-02,
	4.784701765e-01,
	-1.451423317e-01,
	4.409606457e-01,
	-2.356983721e-01,
	3.865052164e-01,
	-3.171966374e-01,
	3.171966374e-01,
	-3.865052164e-01,
	2.356983721e-01,
	-4.409606457e-01,
	1.451423317e-01,
	-4.784701765e-01,
	4.900857061e-02,
	-4.975923598e-01,
];

/// FNV-1a over the bits of `trig`, for blocksizes 64 through 8192.
const TRIG_DIGESTS: &[(u8, u64)] = &[
	(6, 0xe24d_5a54_9c58_e1dd),
	(7, 0x0c9a_8805_a469_66fb),
	(8, 0xd156_937f_10f0_99be),
	(9, 0x6f82_9038_0a97_e706),
	(10, 0x3c90_d5ba_dc12_cbb5),
	(11, 0x73c7_add8_8b3d_a4b9),
	(12, 0x6e3f_72cc_b08f_57e6),
	(13, 0xdcdc_28c1_0fb8_7660),
];

/// libvorbis' window for blocksize 64, literals from `window.c` where the
/// formula and those literals disagree.
const WINDOW_64: &[f32] = &[
	9.460463189e-04,
	8.500646800e-03,
	2.353522554e-02,
	4.589505494e-02,
	7.533518970e-02,
	1.115073040e-01,
	1.539458036e-01,
	2.020557523e-01,
	2.551056743e-01,
	3.122276664e-01,
	3.724270165e-01,
	4.346027672e-01,
	4.975790083e-01,
	5.601459742e-01,
	6.211085320e-01,
	6.793382764e-01,
	7.338252664e-01,
	7.837246060e-01,
	8.283939362e-01,
	8.674186468e-01,
	9.006222486e-01,
	9.280614853e-01,
	9.500073195e-01,
	9.669131637e-01,
	9.793740511e-01,
	9.880793095e-01,
	9.937636256e-01,
	9.971582890e-01,
	9.989462495e-01,
	9.997230172e-01,
	9.999638796e-01,
	9.999995232e-01,
];

/// FNV-1a over the bits of the window, for blocksizes 64 through 8192.
const WINDOW_DIGESTS: &[(u8, u64)] = &[
	(6, 0x35d5_a5cb_7c58_7a2d),
	(7, 0x3c02_ec80_89ab_eba0),
	(8, 0x7968_4aca_9145_8e3a),
	(9, 0x1e4a_5a5b_8078_d8c8),
	(10, 0x19a6_64a8_2a10_784f),
	(11, 0x0679_30b4_9b80_49ef),
	(12, 0x2a9d_e996_b0e0_c2c4),
	(13, 0xe497_47a0_a27d_7148),
];

/// A fixed spectrum for blocksize 64 and what libvorbis transforms it into.
const IMDCT_IN_64: &[f32] = &[
	-6.255999804e-01,
	-4.972000122e-01,
	-1.140000019e-02,
	5.594000220e-01,
	-8.687999845e-01,
	-2.587000132e-01,
	-8.161000013e-01,
	1.439000070e-01,
	2.971999943e-01,
	-3.100999892e-01,
	-4.837999940e-01,
	6.995999813e-01,
	8.101999760e-01,
	8.057000041e-01,
	-6.226000190e-01,
	-4.052999914e-01,
	-6.471999884e-01,
	-7.536000013e-01,
	-8.777999878e-01,
	4.941999912e-01,
	-4.832000136e-01,
	-5.655000210e-01,
	-1.677999943e-01,
	6.377999783e-01,
	-1.914999932e-01,
	-5.230000019e-01,
	3.940000013e-02,
	-3.623999953e-01,
	7.484999895e-01,
	1.739999950e-01,
	-5.594000220e-01,
	-4.354000092e-01,
];
const IMDCT_OUT_64: &[f32] = &[
	2.210132122e+00,
	-1.095002651e+00,
	-1.916371465e+00,
	2.996572018e+00,
	-6.765830517e-01,
	5.305084586e-01,
	2.282737732e+00,
	9.448525906e-01,
	1.287496805e+00,
	-1.744777679e+00,
	2.056591511e-01,
	-1.859711885e+00,
	-1.297453642e+00,
	4.560343325e-01,
	-3.381532907e+00,
	-1.505852699e+00,
	1.505852699e+00,
	3.381532907e+00,
	-4.560343325e-01,
	1.297453642e+00,
	1.859711885e+00,
	-2.056591511e-01,
	1.744777679e+00,
	-1.287496805e+00,
	-9.448525906e-01,
	-2.282737732e+00,
	-5.305084586e-01,
	6.765830517e-01,
	-2.996572018e+00,
	1.916371465e+00,
	1.095002651e+00,
	-2.210132122e+00,
	-3.272244930e+00,
	1.991963983e+00,
	-1.245208621e+00,
	5.709547043e+00,
	2.001688957e+00,
	2.712355852e-01,
	-8.924369812e-01,
	1.406548500e+00,
	3.440619946e+00,
	-2.971848845e-02,
	-3.427128077e+00,
	-1.129106760e+00,
	3.318128347e+00,
	1.806756616e+00,
	-6.701472998e-01,
	3.374313354e+00,
	3.374313354e+00,
	-6.701472998e-01,
	1.806756616e+00,
	3.318128347e+00,
	-1.129106760e+00,
	-3.427128077e+00,
	-2.971848845e-02,
	3.440619946e+00,
	1.406548500e+00,
	-8.924369812e-01,
	2.712355852e-01,
	2.001688957e+00,
	5.709547043e+00,
	-1.245208621e+00,
	1.991963983e+00,
	-3.272244930e+00,
];

/// FNV-1a over the transform of [`lcg_spectrum`], for blocksizes 64 to 8192.
const IMDCT_DIGESTS: &[(u8, u64)] = &[
	(6, 0x556f_802a_f299_e5b9),
	(7, 0xc3db_0a18_b0ea_5ba5),
	(8, 0xcb86_4d51_4304_3b31),
	(9, 0x0955_2f88_276a_b09d),
	(10, 0x6139_590a_2bbe_9e6d),
	(11, 0x742e_41be_ca44_2789),
	(12, 0x7e13_803c_238e_4d85),
	(13, 0x045f_3170_3656_e23d),
];

fn digest(values: &[f32]) -> u64 {
	let mut h: u64 = 0xcbf2_9ce4_8422_2325;
	for value in values {
		for byte in value.to_bits().to_le_bytes() {
			h ^= byte as u64;
			h = h.wrapping_mul(0x0000_0100_0000_01b3);
		}
	}
	h
}

/// The spectrum the reference harness feeds in: a plain LCG mapped onto
/// `[-1, 1]` in hundredths, filling the lower half of an `n` sample buffer.
fn lcg_spectrum(n: usize) -> vec::Vec<f32> {
	let mut state: u32 = 12345;
	let mut buffer = vec::from_elem(0f32, n);
	for slot in buffer.iter_mut().take(n / 2) {
		state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
		*slot = (((state >> 8) as i32 % 20001) - 10000) as f32 / 10000.0;
	}
	buffer
}

#[test]
fn trig_matches_libvorbis_at_blocksize_64() {
	assert_eq!(CachedBlocksizeDerived::from_blocksize(6).trig, TRIG_64);
}

#[test]
fn trig_matches_libvorbis_at_every_blocksize() {
	for &(bs, expected) in TRIG_DIGESTS {
		let cbd = CachedBlocksizeDerived::from_blocksize(bs);
		assert_eq!(digest(&cbd.trig), expected, "trig moved at blocksize {}", 1u32 << bs);
	}
}

#[test]
fn window_matches_libvorbis_at_blocksize_64() {
	assert_eq!(CachedBlocksizeDerived::from_blocksize(6).window_slope, WINDOW_64);
}

#[test]
fn window_matches_libvorbis_at_every_blocksize() {
	for &(bs, expected) in WINDOW_DIGESTS {
		let cbd = CachedBlocksizeDerived::from_blocksize(bs);
		assert_eq!(
			digest(&cbd.window_slope),
			expected,
			"the window moved at blocksize {}",
			1u32 << bs
		);
	}
}

#[test]
fn imdct_matches_libvorbis_at_blocksize_64() {
	let cbd = CachedBlocksizeDerived::from_blocksize(6);
	let mut buffer = vec::from_elem(0f32, 64);
	buffer[..32].copy_from_slice(IMDCT_IN_64);
	inverse_mdct(&cbd, &mut buffer, 6);
	assert_eq!(buffer, IMDCT_OUT_64);
}

#[test]
fn imdct_matches_libvorbis_at_every_blocksize() {
	for &(bs, expected) in IMDCT_DIGESTS {
		let n = 1usize << bs;
		let cbd = CachedBlocksizeDerived::from_blocksize(bs);
		let mut buffer = lcg_spectrum(n);
		inverse_mdct(&cbd, &mut buffer, bs);
		assert_eq!(digest(&buffer), expected, "the transform moved at blocksize {}", n);
	}
}
