// Vorbis decoder written in Rust
//
// Copyright (c) 2016 est31 <MTest31@outlook.com>
// and contributors. All rights reserved.
// Licensed under MIT license, or Apache 2 license,
// at your option. Please see the LICENSE file
// attached to this source distribution for details.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(unknown_lints))]
#![forbid(non_ascii_idents)]

/*!
A `vorbis` decoder, written in Rust.

If you "just" want to decode `ogg/vorbis` files, take a look into
the `inside_ogg` module (make sure you haven't disabled the `ogg` feature).

For lower level, per-packet usage, you can have a look at the `audio` and `header`
modules.
*/

#[macro_use]
extern crate alloc;

#[cfg(test)]
extern crate std;

macro_rules! try_from {
	($expr:expr) => {
		match $expr {
			::core::result::Result::Ok(val) => val,
			::core::result::Result::Err(err) => {
				return Err(::core::convert::From::from(err));
			},
		}
	};
}

/*
// This little thing is very useful.
macro_rules! try_from {
	($expr:expr) => (match $expr {
		::core::result::Result::Ok(val) => val,
		::core::result::Result::Err(err) => {
			panic!("Panic on Err turned on for debug reasons. Encountered Err: {:?}", err)
		}
	})
}
// */

// The following macros are super useful for debugging

macro_rules! record_residue_pre_inverse {
	($residue_vectors:expr) => {
		// 		for v in $residue_vectors.iter() {
		// 			for &re in v {
		// 				println!("{}", re);
		// 			}
		// 		}
	};
}

macro_rules! record_residue_post_inverse {
	($residue_vectors:expr) => {
		// 		for v in $residue_vectors.iter() {
		// 			for &re in v {
		// 				println!("{}", re);
		// 			}
		// 		}
	};
}

macro_rules! record_pre_mdct {
	($audio_spectri:expr) => {
		// 		for v in $audio_spectri.iter() {
		// 			for &s in v {
		// 				println!("{:.5}", s);
		// 			}
		// 		}
	};
}

macro_rules! record_post_mdct {
	($audio_spectri:expr) => {
		// 		for v in $audio_spectri.iter() {
		// 			for &s in v {
		// 				println!("{:.4}", s);
		// 			}
		// 		}
	};
}

pub mod audio;
mod bitpacking;
pub mod header;
mod header_cached;
mod huffman_tree;
mod imdct;
#[cfg(test)]
mod imdct_test;
#[cfg(feature = "ogg")]
pub mod inside_ogg;
pub mod samples;

#[cfg(feature = "ogg")]
#[doc(no_inline)]
pub use ogg::OggReadError;

/// Errors that can occur during decoding
#[derive(Debug)]
pub enum VorbisError {
	BadAudio(audio::AudioReadError),
	BadHeader(header::HeaderReadError),
	#[cfg(feature = "ogg")]
	OggError(OggReadError),
}

impl core::fmt::Display for VorbisError {
	fn fmt(&self, fmt: &mut core::fmt::Formatter) -> Result<(), core::fmt::Error> {
		write!(
			fmt,
			"{}",
			match self {
				VorbisError::BadAudio(_) => "Vorbis bitstream audio decode problem",
				VorbisError::BadHeader(_) => "Vorbis bitstream header decode problem",
				#[cfg(feature = "ogg")]
				VorbisError::OggError(_) => "Ogg decode problem",
			}
		)
	}
}

impl From<audio::AudioReadError> for VorbisError {
	fn from(err: audio::AudioReadError) -> VorbisError {
		VorbisError::BadAudio(err)
	}
}

impl From<header::HeaderReadError> for VorbisError {
	fn from(err: header::HeaderReadError) -> VorbisError {
		VorbisError::BadHeader(err)
	}
}

#[cfg(feature = "ogg")]
impl From<OggReadError> for VorbisError {
	fn from(err: OggReadError) -> VorbisError {
		VorbisError::OggError(err)
	}
}

fn ilog(val: u64) -> u8 {
	64 - val.leading_zeros() as u8
}

#[test]
fn test_ilog() {
	// Uses the test vectors from the Vorbis I spec
	assert_eq!(ilog(0), 0);
	assert_eq!(ilog(1), 1);
	assert_eq!(ilog(2), 2);
	assert_eq!(ilog(3), 2);
	assert_eq!(ilog(4), 3);
	assert_eq!(ilog(7), 3);
}

fn bit_reverse(n: u32) -> u32 {
	n.reverse_bits()
}
