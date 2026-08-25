// Vorbis decoder written in Rust
//
// Copyright (c) 2019 est31 <MTest31@outlook.com>
// and contributors. All rights reserved.
// Licensed under MIT license, or Apache 2 license,
// at your option. Please see the LICENSE file
// attached to this source distribution for details.

/*!
Traits for sample formats
*/

use alloc::vec::Vec;
/// Trait for a packet of multiple samples
pub trait Samples {
	fn num_samples(&self) -> usize;
	fn truncate(&mut self, limit: usize);
	fn from_floats(floats: Vec<Vec<f32>>) -> Self;
}

impl<S: Sample> Samples for Vec<Vec<S>> {
	fn num_samples(&self) -> usize {
		self[0].len()
	}
	fn truncate(&mut self, limit: usize) {
		for ch in self.iter_mut() {
			if limit < ch.len() {
				ch.truncate(limit);
			}
		}
	}

	fn from_floats(floats: Vec<Vec<f32>>) -> Self {
		floats
			.into_iter()
			.map(|samples| samples.into_iter().map(S::from_float).collect())
			.collect()
	}
}

/// A packet of multi-channel interleaved samples
pub struct InterleavedSamples<S: Sample> {
	pub samples: Vec<S>,
	pub channel_count: usize,
}

impl<S: Sample> Samples for InterleavedSamples<S> {
	fn num_samples(&self) -> usize {
		self.samples.len() / self.channel_count
	}
	fn truncate(&mut self, limit: usize) {
		self.samples.truncate(limit * self.channel_count);
	}
	fn from_floats(floats: Vec<Vec<f32>>) -> Self {
		let channel_count = floats.len();
		// Note that a channel count of 0 is forbidden
		// by the spec and the header decoding code already
		// checks for that.
		assert!(floats.len() > 0);
		let samples_interleaved = if channel_count == 1 {
			// Because decoded_pck[0] doesn't work...
			<Vec<Vec<S>> as Samples>::from_floats(floats)
				.into_iter()
				.next()
				.unwrap()
		} else {
			let len = floats[0].len();
			let mut samples = Vec::with_capacity(len * channel_count);
			for i in 0..len {
				for ref chan in floats.iter() {
					samples.push(S::from_float(chan[i]));
				}
			}
			samples
		};
		Self {
			samples: samples_interleaved,
			channel_count,
		}
	}
}

/// Trait representing a single sample
pub trait Sample {
	fn from_float(fl: f32) -> Self;
}

impl Sample for f32 {
	fn from_float(fl: f32) -> Self {
		fl
	}
}

impl Sample for i16 {
	// libvorbis' `ov_read` is what pxtone converts an OGGV voice with, and it
	// scales by 32768, rounds halves toward +inf and then clamps:
	//
	//   val = vorbis_ftoi( pcm[i][j] * 32768.f );
	//   if( val >  32767 ) val =  32767;
	//   else if( val < -32768 ) val = -32768;
	//
	// Truncating instead is a sample out for every negative value, and rounding
	// halves to even is out on the exact halves, so this port spells the
	// rounding out as `floor(x + 0.5)` rather than leaving it to a cast.
	fn from_float(fl: f32) -> Self {
		let fl = lite_math::floorf(fl * 32768.0 + 0.5);
		if fl > 32767. {
			32767
		} else if fl < -32768. {
			-32768
		} else {
			fl as i16
		}
	}
}
