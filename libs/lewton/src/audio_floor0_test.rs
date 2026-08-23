// Vorbis decoder written in Rust
//
// Copyright (c) 2016 est31 <MTest31@outlook.com>
// and contributors. All rights reserved.
// Licensed under MIT license, or Apache 2 license,
// at your option. Please see the LICENSE file
// attached to this source distribution for details.

/*!
Floor type 0, held against libvorbis

No encoder emits floor 0 -- vorbisenc has only ever written floor 1 -- so nothing
in the sample corpus reaches this code, and the values below are the only thing
holding it to libvorbis. They come from `lib/floor0.c`'s bark map and
`lib/lsp.c`'s `vorbis_lsp_to_curve`, compiled with `-ffp-contract=off` so that
every multiply and add rounds on its own, and driven with the parameters named
in each case.

The spec factors the curve differently from libvorbis and the two agree only to
about 1e-6, so this is a port of libvorbis' arrangement rather than of the spec's.
*/

use super::floor_zero_compute_curve;
use crate::header::FloorTypeZero;
use crate::header_cached::compute_bark_map;
use alloc::vec::Vec;

struct Case {
	order: u8,
	rate: u16,
	bark_map_size: u16,
	n: u16,
	amplitude_bits: u8,
	amplitude_offset: u8,
	amplitude: u64,
	coefficients: &'static [f32],
	bark_map: &'static [i32],
	curve: &'static [f32],
}

/// Order 8, rate 44100, bark map 128, 64 spectral lines.
const EVEN_ORDER: Case = Case {
	order: 8,
	rate: 44100,
	bark_map_size: 128,
	n: 64,
	amplitude_bits: 6,
	amplitude_offset: 60,
	amplitude: 40,
	coefficients: &[0.3, 0.6, 0.9, 1.2, 1.5, 1.8, 2.1, 2.4],
	bark_map: &[
		0, 16, 31, 44, 53, 61, 67, 72, 77, 80, 83, 86, 88, 91, 93, 95, 96, 98, 100, 101, 103, 104, 105, 106, 107, 109,
		110, 110, 111, 112, 113, 114, 114, 115, 116, 116, 117, 117, 118, 118, 119, 119, 120, 120, 121, 121, 122, 122,
		122, 123, 123, 123, 124, 124, 125, 125, 125, 125, 126, 126, 126, 127, 127, 127,
	],
	curve: &[
		1.089679260e+02,
		1.126285019e+02,
		3.981229782e+01,
		1.055257702e+01,
		3.577214956e+00,
		2.007785320e+00,
		6.691259742e-01,
		2.492395490e-01,
		1.338674873e-01,
		1.090542823e-01,
		9.076365083e-02,
		6.532570720e-02,
		4.535174742e-02,
		2.245387621e-02,
		1.397270802e-02,
		9.180615656e-03,
		7.640880533e-03,
		5.580276717e-03,
		4.341110587e-03,
		3.906854894e-03,
		3.271589754e-03,
		3.036589129e-03,
		2.841146430e-03,
		2.677188721e-03,
		2.538566478e-03,
		2.319466323e-03,
		2.232423285e-03,
		2.232423285e-03,
		2.157134702e-03,
		2.091772156e-03,
		2.034864854e-03,
		1.985217445e-03,
		1.985217445e-03,
		1.941856346e-03,
		1.903982600e-03,
		1.903982600e-03,
		1.870937296e-03,
		1.870937296e-03,
		1.842176192e-03,
		1.842176192e-03,
		1.817248645e-03,
		1.817248645e-03,
		1.795781893e-03,
		1.795781893e-03,
		1.777466852e-03,
		1.777466852e-03,
		1.762050088e-03,
		1.762050088e-03,
		1.762050088e-03,
		1.749324962e-03,
		1.749324962e-03,
		1.749324962e-03,
		1.739124651e-03,
		1.739124651e-03,
		1.731319702e-03,
		1.731319702e-03,
		1.731319702e-03,
		1.731319702e-03,
		1.725812675e-03,
		1.725812675e-03,
		1.725812675e-03,
		1.722535002e-03,
		1.722535002e-03,
		1.722535002e-03,
	],
};

/// Order 5, rate 22050, bark map 64, 32 spectral lines.
const ODD_ORDER: Case = Case {
	order: 5,
	rate: 22050,
	bark_map_size: 64,
	n: 32,
	amplitude_bits: 8,
	amplitude_offset: 64,
	amplitude: 200,
	coefficients: &[0.11, 0.47, 1.03, 1.9, 2.77],
	bark_map: &[
		0, 9, 17, 24, 30, 34, 37, 40, 42, 44, 46, 48, 49, 50, 51, 53, 54, 54, 55, 56, 57, 58, 58, 59, 60, 60, 61, 61,
		62, 62, 63, 63,
	],
	curve: &[
		f32::INFINITY,
		3.253636000e+06,
		1.824205518e+00,
		3.986069560e-02,
		1.081223693e-02,
		6.998617202e-03,
		5.792689510e-03,
		5.248494446e-03,
		5.141372792e-03,
		5.221175030e-03,
		5.512185860e-03,
		6.086767185e-03,
		6.524981000e-03,
		7.105413359e-03,
		7.879099809e-03,
		1.036730595e-02,
		1.241358556e-02,
		1.241358556e-02,
		1.541991811e-02,
		2.003212832e-02,
		2.748475783e-02,
		4.027062654e-02,
		4.027062654e-02,
		6.366132200e-02,
		1.089609787e-01,
		1.089609787e-01,
		1.984935999e-01,
		1.984935999e-01,
		3.617421389e-01,
		3.617421389e-01,
		5.804001689e-01,
		5.804001689e-01,
	],
};

fn check(case: &Case) {
	let bark_map = compute_bark_map(case.n, case.rate, case.bark_map_size);
	assert_eq!(bark_map, case.bark_map, "the bark map moved");

	// What the decoder hands the curve: `2 * cos(coefficient)`, narrowed.
	let lsp: Vec<f32> = case
		.coefficients
		.iter()
		.map(|&c| (2.0 * lite_math::cos(c as f64)) as f32)
		.collect();
	let fl = FloorTypeZero {
		floor0_order: case.order,
		floor0_amplitude_bits: case.amplitude_bits,
		floor0_amplitude_offset: case.amplitude_offset,
		floor0_number_of_books: 1,
		floor0_book_list: alloc::vec![0],
		floor0_bark_map_size: case.bark_map_size,
		bark_map: [bark_map.clone(), bark_map],
	};
	let curve = floor_zero_compute_curve(&lsp, case.amplitude, &fl, false, case.n);
	assert_eq!(curve, case.curve, "the curve moved");
}

#[test]
fn even_order_matches_libvorbis() {
	check(&EVEN_ORDER);
}

#[test]
fn odd_order_matches_libvorbis() {
	check(&ODD_ORDER);
}
