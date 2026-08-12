// Ogg decoder and encoder written in Rust
//
// Copyright (c) 2016 est31 <MTest31@outlook.com>
// and contributors. All rights reserved.
// Redistribution or use only under the terms
// specified in the LICENSE file attached to this
// source distribution.

#![no_std]
#![forbid(unsafe_code)]

/*!
Ogg container decoder

The most interesting structure in this mod is `PacketReader`.

Unlike upstream ogg this fork decodes from a byte slice rather than from an
`io::Read`, it is `no_std` (`alloc` is required), and the encoder is only built
for the tests.
*/

extern crate alloc;

#[cfg(test)]
extern crate std;

/// Used by the test only encoder.
#[cfg(test)]
macro_rules! tri {
	($e:expr) => {
		match $e {
			Ok(val) => val,
			Err(err) => return Err(err.into()),
		}
	};
}

#[cfg(test)]
mod test;

mod crc;
pub mod reading;
/// The encoder is only used to build streams for the reader's tests, so parts of
/// it are unused.
#[cfg(test)]
#[allow(dead_code)]
mod writing;

pub use crate::reading::{OggReadError, PacketReader};

use alloc::borrow::Cow;

/**
Ogg packet representation.

For the Ogg format, packets are the logically smallest subdivision it handles.

Every packet belongs to a *logical* bitstream. The *logical* bitstreams then form a *physical* bitstream, with the data combined in multiple different ways.

Every logical bitstream is identified by the serial number its pages have stored. The Packet struct contains a field for that number as well, so that one can find out which logical bitstream the Packet belongs to.
*/
pub struct Packet<'a> {
	/// The data the `Packet` contains
	///
	/// Borrowed from the stream, unless the packet spans multiple pages and had
	/// to be glued together.
	pub data: Cow<'a, [u8]>,
	/// `true` iff this packet is the first one in the page.
	first_packet_pg: bool,
	/// `true` iff this packet is the first one in the logical bitstream.
	first_packet_stream: bool,
	/// `true` iff this packet is the last one in the page.
	last_packet_pg: bool,
	/// `true` iff this packet is the last one in the logical bitstream
	last_packet_stream: bool,
	/// Absolute granule position of the last page the packet was in.
	/// The meaning of the absolute granule position is defined by the codec.
	absgp_page: u64,
	/// Serial number. Uniquely identifying the logical bitstream.
	stream_serial: u32,
}

impl Packet<'_> {
	/// Returns whether the packet is the first one starting in the page
	pub fn first_in_page(&self) -> bool {
		self.first_packet_pg
	}
	/// Returns whether the packet is the first one of the entire stream
	pub fn first_in_stream(&self) -> bool {
		self.first_packet_stream
	}
	/// Returns whether the packet is the last one starting in the page
	pub fn last_in_page(&self) -> bool {
		self.last_packet_pg
	}
	/// Returns whether the packet is the last one of the entire stream
	pub fn last_in_stream(&self) -> bool {
		self.last_packet_stream
	}
	/// Returns the absolute granule position of the page the packet ended in.
	///
	/// The meaning of the absolute granule position is defined by the codec.
	pub fn absgp_page(&self) -> u64 {
		self.absgp_page
	}
	/// Returns the serial number that uniquely identifies the logical bitstream.
	pub fn stream_serial(&self) -> u32 {
		self.stream_serial
	}
}
