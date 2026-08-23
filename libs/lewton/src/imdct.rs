// Vorbis decoder written in Rust
//
// Copyright (c) 2016 est31 <MTest31@outlook.com>
// and contributors. All rights reserved.
// Licensed under MIT license, or Apache 2 license,
// at your option. Please see the LICENSE file
// attached to this source distribution for details.

/*!
Inverse modified discrete cosine transform

A port of the backward transform in libvorbis' `lib/mdct.c`, operation for
operation. Any correct IMDCT gives the same transform, but not the same floats:
the order the products are summed in decides the last bit, and pxtone decodes an
OGGV voice through libvorbis, so that is the order to keep. The implementation
this replaced came from stb_vorbis and disagreed by around 2e-6 -- a couple of
`f32` ulps, and the largest term in the decoder's disagreement with `ov_read`.

libvorbis builds with `MULT_NORM` and `FLOAT_CONV` as the identity and
`HALVE(x)` as `x * 0.5f` in its float configuration, and every intermediate is a
`float`, so each multiply and add here rounds to `f32` on its own. Compiling the
C with contraction enabled -- clang's default on a target with a fused
multiply-add -- gives something else again, and this follows the C rather than
any one build of it.
*/

use crate::header_cached::CachedBlocksizeDerived;

const CPI3_8: f32 = 0.382_683_432_365_089_78;
const CPI2_8: f32 = 0.707_106_781_186_547_5;
const CPI1_8: f32 = 0.923_879_532_511_286_8;

/// 8 point butterfly, in place.
fn butterfly_8(x: &mut [f32], o: usize) {
	let r0 = x[o + 6] + x[o + 2];
	let r1 = x[o + 6] - x[o + 2];
	let r2 = x[o + 4] + x[o];
	let r3 = x[o + 4] - x[o];

	x[o + 6] = r0 + r2;
	x[o + 4] = r0 - r2;

	let r0 = x[o + 5] - x[o + 1];
	let r2 = x[o + 7] - x[o + 3];
	x[o] = r1 + r0;
	x[o + 2] = r1 - r0;

	let r0 = x[o + 5] + x[o + 1];
	let r1 = x[o + 7] + x[o + 3];
	x[o + 3] = r2 + r3;
	x[o + 1] = r2 - r3;
	x[o + 7] = r1 + r0;
	x[o + 5] = r1 - r0;
}

/// 16 point butterfly, in place.
fn butterfly_16(x: &mut [f32], o: usize) {
	let r0 = x[o + 1] - x[o + 9];
	let r1 = x[o] - x[o + 8];
	x[o + 8] += x[o];
	x[o + 9] += x[o + 1];
	x[o] = (r0 + r1) * CPI2_8;
	x[o + 1] = (r0 - r1) * CPI2_8;

	let r0 = x[o + 3] - x[o + 11];
	let r1 = x[o + 10] - x[o + 2];
	x[o + 10] += x[o + 2];
	x[o + 11] += x[o + 3];
	x[o + 2] = r0;
	x[o + 3] = r1;

	let r0 = x[o + 12] - x[o + 4];
	let r1 = x[o + 13] - x[o + 5];
	x[o + 12] += x[o + 4];
	x[o + 13] += x[o + 5];
	x[o + 4] = (r0 - r1) * CPI2_8;
	x[o + 5] = (r0 + r1) * CPI2_8;

	let r0 = x[o + 14] - x[o + 6];
	let r1 = x[o + 15] - x[o + 7];
	x[o + 14] += x[o + 6];
	x[o + 15] += x[o + 7];
	x[o + 6] = r0;
	x[o + 7] = r1;

	butterfly_8(x, o);
	butterfly_8(x, o + 8);
}

/// 32 point butterfly, in place.
fn butterfly_32(x: &mut [f32], o: usize) {
	let r0 = x[o + 30] - x[o + 14];
	let r1 = x[o + 31] - x[o + 15];
	x[o + 30] += x[o + 14];
	x[o + 31] += x[o + 15];
	x[o + 14] = r0;
	x[o + 15] = r1;

	let r0 = x[o + 28] - x[o + 12];
	let r1 = x[o + 29] - x[o + 13];
	x[o + 28] += x[o + 12];
	x[o + 29] += x[o + 13];
	x[o + 12] = r0 * CPI1_8 - r1 * CPI3_8;
	x[o + 13] = r0 * CPI3_8 + r1 * CPI1_8;

	let r0 = x[o + 26] - x[o + 10];
	let r1 = x[o + 27] - x[o + 11];
	x[o + 26] += x[o + 10];
	x[o + 27] += x[o + 11];
	x[o + 10] = (r0 - r1) * CPI2_8;
	x[o + 11] = (r0 + r1) * CPI2_8;

	let r0 = x[o + 24] - x[o + 8];
	let r1 = x[o + 25] - x[o + 9];
	x[o + 24] += x[o + 8];
	x[o + 25] += x[o + 9];
	x[o + 8] = r0 * CPI3_8 - r1 * CPI1_8;
	x[o + 9] = r1 * CPI3_8 + r0 * CPI1_8;

	let r0 = x[o + 22] - x[o + 6];
	let r1 = x[o + 7] - x[o + 23];
	x[o + 22] += x[o + 6];
	x[o + 23] += x[o + 7];
	x[o + 6] = r1;
	x[o + 7] = r0;

	let r0 = x[o + 4] - x[o + 20];
	let r1 = x[o + 5] - x[o + 21];
	x[o + 20] += x[o + 4];
	x[o + 21] += x[o + 5];
	x[o + 4] = r1 * CPI1_8 + r0 * CPI3_8;
	x[o + 5] = r1 * CPI3_8 - r0 * CPI1_8;

	let r0 = x[o + 2] - x[o + 18];
	let r1 = x[o + 3] - x[o + 19];
	x[o + 18] += x[o + 2];
	x[o + 19] += x[o + 3];
	x[o + 2] = (r1 + r0) * CPI2_8;
	x[o + 3] = (r1 - r0) * CPI2_8;

	let r0 = x[o] - x[o + 16];
	let r1 = x[o + 1] - x[o + 17];
	x[o + 16] += x[o];
	x[o + 17] += x[o + 1];
	x[o] = r1 * CPI3_8 + r0 * CPI1_8;
	x[o + 1] = r1 * CPI1_8 - r0 * CPI3_8;

	butterfly_16(x, o);
	butterfly_16(x, o + 16);
}

/// First stage butterfly over `points` samples at `o`, reading the trig table
/// straight through.
fn butterfly_first(trig: &[f32], x: &mut [f32], o: usize, points: usize) {
	let mut x1 = (o + points - 8) as isize;
	let mut x2 = (o + (points >> 1) - 8) as isize;
	let mut t = 0;

	loop {
		let (a, b) = (x1 as usize, x2 as usize);
		for (k, ts) in [(6, 0), (4, 4), (2, 8), (0, 12)] {
			let r0 = x[a + k] - x[b + k];
			let r1 = x[a + k + 1] - x[b + k + 1];
			x[a + k] += x[b + k];
			x[a + k + 1] += x[b + k + 1];
			x[b + k] = r1 * trig[t + ts + 1] + r0 * trig[t + ts];
			x[b + k + 1] = r1 * trig[t + ts] - r0 * trig[t + ts + 1];
		}
		x1 -= 8;
		x2 -= 8;
		t += 16;
		if x2 < o as isize {
			break;
		}
	}
}

/// Generic stage butterfly, stepping the trig table by `trigint` between pairs.
fn butterfly_generic(trig: &[f32], x: &mut [f32], o: usize, points: usize, trigint: usize) {
	let mut x1 = (o + points - 8) as isize;
	let mut x2 = (o + (points >> 1) - 8) as isize;
	let mut t = 0;

	loop {
		let (a, b) = (x1 as usize, x2 as usize);
		for k in [6, 4, 2, 0] {
			let r0 = x[a + k] - x[b + k];
			let r1 = x[a + k + 1] - x[b + k + 1];
			x[a + k] += x[b + k];
			x[a + k + 1] += x[b + k + 1];
			x[b + k] = r1 * trig[t + 1] + r0 * trig[t];
			x[b + k + 1] = r1 * trig[t] - r0 * trig[t + 1];
			t += trigint;
		}
		x1 -= 8;
		x2 -= 8;
		if x2 < o as isize {
			break;
		}
	}
}

fn butterflies(trig: &[f32], log2n: usize, x: &mut [f32], o: usize, points: usize) {
	// `stages` is decremented before each test, as the C's `--stages` does.
	let mut stages = log2n as isize - 5;

	stages -= 1;
	if stages > 0 {
		butterfly_first(trig, x, o, points);
	}

	let mut i = 1;
	loop {
		stages -= 1;
		if stages <= 0 {
			break;
		}
		for j in 0..(1usize << i) {
			butterfly_generic(trig, x, o + (points >> i) * j, points >> i, 4 << i);
		}
		i += 1;
	}

	for j in (0..points).step_by(32) {
		butterfly_32(x, o + j);
	}
}

/// Untangles the butterflies' output, reading the upper half of `x` through
/// `bitrev` and writing the lower half.
fn bitreverse(trig: &[f32], bitrev: &[usize], n: usize, x: &mut [f32]) {
	let n2 = n >> 1;
	let mut w0 = 0;
	let mut w1 = n2;
	let mut t = n;
	let mut bit = 0;

	loop {
		for (half, ts) in [(0, 0), (1, 2)] {
			let x0 = n2 + bitrev[bit + half * 2];
			let x1 = n2 + bitrev[bit + half * 2 + 1];

			let r0 = x[x0 + 1] - x[x1 + 1];
			let r1 = x[x0] + x[x1];
			let r2 = r1 * trig[t + ts] + r0 * trig[t + ts + 1];
			let r3 = r1 * trig[t + ts + 1] - r0 * trig[t + ts];

			if half == 0 {
				w1 -= 4;
			}

			let r0 = (x[x0 + 1] + x[x1 + 1]) * 0.5;
			let r1 = (x[x0] - x[x1]) * 0.5;

			// The C writes `w0[0], w1[2], w0[1], w1[3]` for the first half and
			// `w0[2], w1[0], w0[3], w1[1]` for the second.
			let (o0, o1) = if half == 0 { (0, 2) } else { (2, 0) };
			x[w0 + o0] = r0 + r2;
			x[w1 + o1] = r0 - r2;
			x[w0 + o0 + 1] = r1 + r3;
			x[w1 + o1 + 1] = r3 - r1;
		}

		t += 4;
		bit += 4;
		w0 += 4;
		if w0 >= w1 {
			break;
		}
	}
}

/// Transforms `buffer` in place: `n/2` spectral values in, `n` samples out.
pub fn inverse_mdct(cached_bd: &CachedBlocksizeDerived, buffer: &mut [f32], bs: u8) {
	let n = 1usize << bs;
	let n2 = n >> 1;
	let n4 = n >> 2;
	let trig = &cached_bd.trig;
	let x = buffer;

	// rotate
	let mut i_x = n2 as isize - 7;
	let mut o_x = n2 + n4;
	let mut t = n4;
	loop {
		let i = i_x as usize;
		o_x -= 4;
		x[o_x] = -x[i + 2] * trig[t + 3] - x[i] * trig[t + 2];
		x[o_x + 1] = x[i] * trig[t + 3] - x[i + 2] * trig[t + 2];
		x[o_x + 2] = -x[i + 6] * trig[t + 1] - x[i + 4] * trig[t];
		x[o_x + 3] = x[i + 4] * trig[t + 1] - x[i + 6] * trig[t];
		i_x -= 8;
		t += 4;
		if i_x < 0 {
			break;
		}
	}

	let mut i_x = n2 as isize - 8;
	let mut o_x = n2 + n4;
	let mut t = n4;
	loop {
		let i = i_x as usize;
		t -= 4;
		x[o_x] = x[i + 4] * trig[t + 3] + x[i + 6] * trig[t + 2];
		x[o_x + 1] = x[i + 4] * trig[t + 2] - x[i + 6] * trig[t + 3];
		x[o_x + 2] = x[i] * trig[t + 1] + x[i + 2] * trig[t];
		x[o_x + 3] = x[i] * trig[t] - x[i + 2] * trig[t + 1];
		i_x -= 8;
		o_x += 4;
		if i_x < 0 {
			break;
		}
	}

	butterflies(trig, bs as usize, x, n2, n2);
	bitreverse(trig, &cached_bd.bitrev, n, x);

	// rotate
	let mut o_x1 = n2 + n4;
	let mut o_x2 = n2 + n4;
	let mut i_x = 0;
	let mut t = n2;
	loop {
		o_x1 -= 4;

		x[o_x1 + 3] = x[i_x] * trig[t + 1] - x[i_x + 1] * trig[t];
		x[o_x2] = -(x[i_x] * trig[t] + x[i_x + 1] * trig[t + 1]);

		x[o_x1 + 2] = x[i_x + 2] * trig[t + 3] - x[i_x + 3] * trig[t + 2];
		x[o_x2 + 1] = -(x[i_x + 2] * trig[t + 2] + x[i_x + 3] * trig[t + 3]);

		x[o_x1 + 1] = x[i_x + 4] * trig[t + 5] - x[i_x + 5] * trig[t + 4];
		x[o_x2 + 2] = -(x[i_x + 4] * trig[t + 4] + x[i_x + 5] * trig[t + 5]);

		x[o_x1] = x[i_x + 6] * trig[t + 7] - x[i_x + 7] * trig[t + 6];
		x[o_x2 + 3] = -(x[i_x + 6] * trig[t + 6] + x[i_x + 7] * trig[t + 7]);

		o_x2 += 4;
		i_x += 8;
		t += 8;
		if i_x >= o_x1 {
			break;
		}
	}

	let mut i_x = n2 + n4;
	let mut o_x1 = n4;
	let mut o_x2 = n4;
	loop {
		o_x1 -= 4;
		i_x -= 4;

		x[o_x1 + 3] = x[i_x + 3];
		x[o_x2] = -x[o_x1 + 3];
		x[o_x1 + 2] = x[i_x + 2];
		x[o_x2 + 1] = -x[o_x1 + 2];
		x[o_x1 + 1] = x[i_x + 1];
		x[o_x2 + 2] = -x[o_x1 + 1];
		x[o_x1] = x[i_x];
		x[o_x2 + 3] = -x[o_x1];

		o_x2 += 4;
		if o_x2 >= i_x {
			break;
		}
	}

	let mut i_x = n2 + n4;
	let mut o_x1 = n2 + n4;
	let o_x2 = n2;
	loop {
		o_x1 -= 4;
		x[o_x1] = x[i_x + 3];
		x[o_x1 + 1] = x[i_x + 2];
		x[o_x1 + 2] = x[i_x + 1];
		x[o_x1 + 3] = x[i_x];
		i_x += 4;
		if o_x1 <= o_x2 {
			break;
		}
	}
}
