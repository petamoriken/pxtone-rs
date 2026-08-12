// Vorbis decoder written in Rust
//
// Copyright (c) 2016 est31 <MTest31@outlook.com>
// and contributors. All rights reserved.
// Licensed under MIT license, or Apache 2 license,
// at your option. Please see the LICENSE file
// attached to this source distribution for details.

/*!
Higher-level utilities for Ogg streams and files

This module provides higher level access to the library functionality,
and useful helper methods for the Ogg `PacketReader` struct.
*/

use crate::VorbisError;
use crate::audio::{PreviousWindowRight, get_decoded_sample_count, read_audio_packet, read_audio_packet_generic};
use crate::header::HeaderSet;
use crate::header::*;
use crate::samples::{InterleavedSamples, Samples};
use ogg::{Packet, PacketReader};

/// Reads the vorbis headers from an ogg stream as well as stream serial information
pub fn read_headers(rdr: &mut PacketReader<'_>) -> Result<(HeaderSet, u32), VorbisError> {
	let pck: Packet<'_> = try_from!(rdr.read_packet_expected());
	let ident_hdr = try_from!(read_header_ident(&pck.data));
	let stream_serial = pck.stream_serial();

	let mut pck: Packet<'_> = try_from!(rdr.read_packet_expected());
	while pck.stream_serial() != stream_serial {
		pck = try_from!(rdr.read_packet_expected());
	}
	try_from!(read_header_comment(&pck.data));

	let mut pck: Packet<'_> = try_from!(rdr.read_packet_expected());
	while pck.stream_serial() != stream_serial {
		pck = try_from!(rdr.read_packet_expected());
	}
	let setup_hdr = try_from!(read_header_setup(
		&pck.data,
		ident_hdr.audio_channels,
		(ident_hdr.blocksize_0, ident_hdr.blocksize_1)
	));

	rdr.delete_unread_packets();
	return Ok(((ident_hdr, setup_hdr), pck.stream_serial()));
}

/**
Reading ogg/vorbis files or streams

This is a small helper struct to help reading ogg/vorbis files
or streams in that format.

It only supports the main use case of pure audio ogg files streams.
Reading a file where vorbis is only one of multiple streams, like
in the case of ogv, is not supported.

If you need support for this, you need to use the lower level methods
instead.
*/
pub struct OggStreamReader<'a> {
	rdr: PacketReader<'a>,
	pwr: PreviousWindowRight,

	stream_serial: u32,

	pub ident_hdr: IdentHeader,
	pub setup_hdr: SetupHeader,

	cur_absgp: Option<u64>,
}

impl<'a> OggStreamReader<'a> {
	/// Constructs a new OggStreamReader for a physical ogg stream in memory.
	pub fn new(data: &'a [u8]) -> Result<Self, VorbisError> {
		OggStreamReader::from_ogg_reader(PacketReader::new(data))
	}
	/// Constructs a new OggStreamReader from a given Ogg PacketReader.
	///
	/// The `new` function is a nice wrapper around this function that
	/// also creates the ogg reader.
	pub fn from_ogg_reader(mut rdr: PacketReader<'a>) -> Result<Self, VorbisError> {
		let ((ident_hdr, setup_hdr), stream_serial) = try_from!(read_headers(&mut rdr));
		return Ok(OggStreamReader {
			rdr,
			pwr: PreviousWindowRight::new(),
			ident_hdr,
			setup_hdr,
			stream_serial,
			cur_absgp: None,
		});
	}
	pub fn into_inner(self) -> PacketReader<'a> {
		self.rdr
	}
	fn read_next_audio_packet(&mut self) -> Result<Option<Packet<'a>>, VorbisError> {
		loop {
			let pck = match try_from!(self.rdr.read_packet()) {
				Some(p) => p,
				None => return Ok(None),
			};
			if pck.stream_serial() != self.stream_serial {
				if pck.first_in_stream() {
					// We have a chained ogg file. This means we need to
					// re-initialize the internal context.
					let ident_hdr = try_from!(read_header_ident(&pck.data));

					let pck: Packet<'_> = try_from!(self.rdr.read_packet_expected());
					try_from!(read_header_comment(&pck.data));

					let pck: Packet<'_> = try_from!(self.rdr.read_packet_expected());
					let setup_hdr = try_from!(read_header_setup(
						&pck.data,
						ident_hdr.audio_channels,
						(ident_hdr.blocksize_0, ident_hdr.blocksize_1)
					));

					// Update the context
					self.pwr = PreviousWindowRight::new();
					self.ident_hdr = ident_hdr;
					self.setup_hdr = setup_hdr;
					self.stream_serial = pck.stream_serial();
					self.cur_absgp = None;

					// Now, read the first audio packet to prime the pwr
					// and discard the packet.
					let pck = match try_from!(self.rdr.read_packet()) {
						Some(p) => p,
						None => return Ok(None),
					};
					let _decoded_pck = try_from!(read_audio_packet(
						&self.ident_hdr,
						&self.setup_hdr,
						&pck.data,
						&mut self.pwr
					));
					self.cur_absgp = Some(pck.absgp_page());

					return Ok(try_from!(self.rdr.read_packet()));
				} else {
					// Ignore every packet that has a mismatching stream serial
				}
			} else {
				return Ok(Some(pck));
			}
		}
	}
	/// Reads and decompresses an audio packet from the stream.
	///
	/// On read errors, it returns Err(e) with the error.
	///
	/// On success, it either returns None, when the end of the
	/// stream has been reached, or Some(packet_data),
	/// with the data of the decompressed packet.
	pub fn read_dec_packet(&mut self) -> Result<Option<Vec<Vec<i16>>>, VorbisError> {
		let pck = try_from!(self.read_dec_packet_generic());
		Ok(pck)
	}

	/// Reads and decompresses an audio packet from the stream (interleaved).
	///
	/// On read errors, it returns Err(e) with the error.
	///
	/// On success, it either returns None, when the end of the
	/// stream has been reached, or Some(packet_data),
	/// with the data of the decompressed packet.
	///
	/// Unlike `read_dec_packet`, this function returns the
	/// interleaved samples.
	pub fn read_dec_packet_itl(&mut self) -> Result<Option<Vec<i16>>, VorbisError> {
		let decoded_pck: InterleavedSamples<_> = match try_from!(self.read_dec_packet_generic()) {
			Some(p) => p,
			None => return Ok(None),
		};
		return Ok(Some(decoded_pck.samples));
	}

	/// Reads and decompresses an audio packet from the stream (generic).
	///
	/// On read errors, it returns Err(e) with the error.
	///
	/// On success, it either returns None, when the end of the
	/// stream has been reached, or Some(packet_data),
	/// with the data of the decompressed packet.
	pub fn read_dec_packet_generic<S: Samples>(&mut self) -> Result<Option<S>, VorbisError> {
		let pck = match try_from!(self.read_next_audio_packet()) {
			Some(p) => p,
			None => return Ok(None),
		};
		self.dec_packet_generic(pck).map(Option::Some)
	}

	#[inline]
	pub fn dec_packet_generic<S: Samples>(&mut self, pck: Packet<'_>) -> Result<S, VorbisError> {
		let mut decoded_pck: S = try_from!(read_audio_packet_generic(
			&self.ident_hdr,
			&self.setup_hdr,
			&pck.data,
			&mut self.pwr
		));

		// If this is the last packet in the logical bitstream,
		// we need to truncate it so that its ending matches
		// the absgp of the current page.
		// This is what the spec mandates and also the behaviour
		// of libvorbis.
		if let (Some(absgp), true) = (self.cur_absgp, pck.last_in_stream()) {
			let target_length = pck.absgp_page().saturating_sub(absgp) as usize;
			decoded_pck.truncate(target_length);
		}
		if pck.last_in_page() {
			self.cur_absgp = Some(pck.absgp_page());
		} else if let &mut Some(ref mut absgp) = &mut self.cur_absgp {
			*absgp += decoded_pck.num_samples() as u64;
		}
		return Ok(decoded_pck);
	}
	/// Skips the given number of samples
	///
	/// Skips multiple packets without decoding any but the last two, so that
	/// the leftover number of samples to skip is lower than the length of the
	/// returned packet.
	///
	/// The function runs a linear skip algorithm which means that instead of
	/// logarithmic *seeking*, it inspects each packet for its length, and
	/// subtracts the length from the length to skip. This function does no
	/// packet decoding until it arrives at the destination, which makes it
	/// way cheaper than just decoding all packets.
	///
	/// The absolute granule position is always increased in whole-package
	/// increments.
	pub fn skip_samples_linear<S: Samples>(&mut self, to_skip: usize) -> Result<(Option<S>, usize), VorbisError> {
		let mut to_skip = to_skip;
		let mut last_pck: Option<Packet<'_>> = None;
		let mut next_pck;

		loop {
			if let Some(p) = try_from!(self.read_next_audio_packet()) {
				next_pck = p;
			} else {
				return Ok((None, to_skip));
			}
			let mut sample_cnt = try_from!(get_decoded_sample_count(
				&self.ident_hdr,
				&self.setup_hdr,
				&next_pck.data
			));
			// If this is the last packet in the logical bitstream,
			// we need to truncate it so that its ending matches
			// the absgp of the current page.
			// This is what the spec mandates and also the behaviour
			// of libvorbis.
			if let (Some(absgp), true) = (self.cur_absgp, next_pck.last_in_stream()) {
				last_pck = None;
				let target_length = next_pck.absgp_page().saturating_sub(absgp) as usize;
				sample_cnt = sample_cnt.min(target_length);
			}
			if to_skip < sample_cnt {
				// We reached the end of our search.
				if let Some(last_pck) = last_pck {
					self.pwr = PreviousWindowRight::new();
					let _decoded_pck: S = try_from!(read_audio_packet_generic(
						&self.ident_hdr,
						&self.setup_hdr,
						&last_pck.data,
						&mut self.pwr
					));
				}
				let decoded_pck = try_from!(self.dec_packet_generic(next_pck));
				return Ok((Some(decoded_pck), to_skip));
			} else {
				to_skip -= sample_cnt;
			}
			if let &mut Some(ref mut absgp) = &mut self.cur_absgp {
				*absgp += sample_cnt as u64;
			}
			last_pck = Some(next_pck);
		}
	}

	/// Returns the stream serial of the current stream
	///
	/// The stream serial can change in chained ogg files.
	pub fn stream_serial(&self) -> u32 {
		self.stream_serial
	}

	/// Returns the absolute granule position of the last read page.
	///
	/// In the case of ogg/vorbis, the absolute granule position is given
	/// as number of PCM samples, on a per channel basis.
	pub fn get_last_absgp(&self) -> Option<u64> {
		self.cur_absgp
	}
}
