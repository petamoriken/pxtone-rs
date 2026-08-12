// Ogg decoder and encoder written in Rust
//
// Copyright (c) 2016 est31 <MTest31@outlook.com>
// and contributors. All rights reserved.
// Redistribution or use only under the terms
// specified in the LICENSE file attached to this
// source distribution.

/*!
Reading logic

Unlike upstream ogg, this fork reads the whole physical stream from a byte
slice rather than from an `io::Read`. Pages are parsed in one pass, and packets
borrow their data from the input unless they span page boundaries.
*/

use crate::Packet;
use crate::crc::vorbis_crc32_update;
use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

/// Error that can be raised when decoding an Ogg transport.
#[derive(Debug, PartialEq, Eq)]
pub enum OggReadError {
	/// The capture pattern for a new page was not found
	/// where one was expected.
	NoCapturePatternFound,
	/// Invalid stream structure version, with the given one
	/// attached.
	InvalidStreamStructVer(u8),
	/// Mismatch of the hash value with (expected, calculated) value.
	HashMismatch(u32, u32),
	/// The stream ended inside a page, or before a packet that was expected.
	UnexpectedEndOfStream,
	/// Some constraint required by the spec was not met.
	InvalidData,
}

impl OggReadError {
	fn description_str(&self) -> &str {
		match *self {
			OggReadError::NoCapturePatternFound => "No Ogg capture pattern found",
			OggReadError::InvalidStreamStructVer(_) => "A non zero stream structure version was passed",
			OggReadError::HashMismatch(_, _) => "CRC32 hash mismatch",
			OggReadError::UnexpectedEndOfStream => "Unexpected end of the physical stream",
			OggReadError::InvalidData => "Constraint violated",
		}
	}
}

impl Display for OggReadError {
	fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
		fmt.write_str(self.description_str())
	}
}

/// Length of the fixed part of a page header.
const PAGE_HEADER_LEN: usize = 27;

/// Byte pattern every page starts with.
const CAPTURE_PATTERN: [u8; 4] = *b"OggS";

/// How far the reader searches for a capture pattern before giving up.
///
/// 150kb gives us a bit of safety: we can survive up to one page with a
/// corrupted capture pattern.
const MAX_SEARCH_LEN: usize = 150 * 1024;

/// Containing information about an OGG page that is shared between multiple places
struct PageBaseInfo {
	/// `true`: the first packet is continued from the page before. `false`: if it's a "fresh" one
	starts_with_continued: bool,
	/// `true` if this page is the first one in the logical bitstream
	first_page: bool,
	/// `true` if this page is the last one in the logical bitstream
	last_page: bool,
	/// Absolute granule position. The codec defines further meaning.
	absgp: u64,
	/// Packet information:
	/// index is number of packet,
	/// tuple is (offset, length) of packet
	/// if ends_with_continued is true, the last element will contain information
	/// about the continued packet
	packet_positions: Vec<(u16, u16)>,
	/// `true` if the packet is continued in subsequent page(s)
	/// `false` if the packet has a segment of length < 255 inside this page
	ends_with_continued: bool,
}

/// A parsed page, borrowing its body from the stream.
struct Page<'a> {
	bi: PageBaseInfo,
	stream_serial: u32,
	/// Number of packet ending segments
	packet_count: u16,
	body: &'a [u8],
}

/// State of one logical bitstream inside the physical stream.
struct PageInfo<'a> {
	/// Basic information about the last read page
	bi: PageBaseInfo,
	/// The index of the first "unread" packet
	packet_idx: u8,
	/// The last read page's body
	page_body: &'a [u8],
	/// Some(offs), if the last packet is continued in the next page,
	/// with offs being the offset of the continued packet's start
	last_overlap_pck: Vec<&'a [u8]>,
}

impl PageInfo<'_> {
	/// Returns whether the packet that is about to be read is the first one in the page
	fn is_first_pck_in_pg(&self) -> bool {
		self.packet_idx == 0
	}
	/// Returns whether the packet that is about to be read is the last one in the page
	fn is_last_pck_in_pg(&self) -> bool {
		(self.packet_idx as usize + 1 + self.bi.ends_with_continued as usize) == self.bi.packet_positions.len()
	}
}

/// Returns the offset of the capture pattern inside `data`, if there is one.
fn find_capture_pattern(data: &[u8]) -> Option<usize> {
	data.windows(CAPTURE_PATTERN.len())
		.position(|window| window == CAPTURE_PATTERN)
}

/// Parses the page that begins at the start of `data`.
///
/// Returns the page and the number of bytes it occupies.
fn parse_page(data: &[u8]) -> Result<(Page<'_>, usize), OggReadError> {
	let Some(header) = data.first_chunk::<PAGE_HEADER_LEN>() else {
		return Err(OggReadError::UnexpectedEndOfStream);
	};
	if header[..CAPTURE_PATTERN.len()] != CAPTURE_PATTERN {
		return Err(OggReadError::NoCapturePatternFound);
	}
	let stream_structure_version = header[4];
	if stream_structure_version != 0 {
		return Err(OggReadError::InvalidStreamStructVer(stream_structure_version));
	}
	let header_type_flag = header[5];
	let starts_with_continued = header_type_flag & 0x01 != 0;
	let segment_count = header[26] as usize;

	let after_header = &data[PAGE_HEADER_LEN..];
	if after_header.len() < segment_count {
		return Err(OggReadError::UnexpectedEndOfStream);
	}
	let (segments, after_segments) = after_header.split_at(segment_count);

	// First run over the segment table: the size of the page's body, the number
	// of packets, and whether the page ends with a continued packet.
	let mut body_len: usize = 0;
	let mut packet_count: u16 = 0;
	let mut ends_with_continued = starts_with_continued;
	for &segment in segments {
		body_len += segment as usize;
		// Increment by 1 if the segment is < 255, otherwise by 0
		packet_count += (segment < 255) as u16;
		ends_with_continued = segment == 255;
	}
	if after_segments.len() < body_len {
		return Err(OggReadError::UnexpectedEndOfStream);
	}
	let body = &after_segments[..body_len];

	// Second run: the offsets of the packets inside the body.
	let mut packet_positions = Vec::with_capacity(packet_count as usize + ends_with_continued as usize);
	let mut packet_offs: u16 = 0;
	let mut packet_siz: u16 = 0;
	for &segment in segments {
		packet_siz += segment as u16;
		if segment < 255 {
			packet_positions.push((packet_offs, packet_siz));
			packet_offs += packet_siz;
			packet_siz = 0;
		}
	}
	if ends_with_continued {
		packet_positions.push((packet_offs, packet_siz));
	}

	// The checksum is computed over the page with the checksum field zeroed.
	let checksum = u32::from_le_bytes([header[22], header[23], header[24], header[25]]);
	let mut header_for_hash = *header;
	header_for_hash[22..26].fill(0);
	let mut hash = vorbis_crc32_update(0, &header_for_hash);
	hash = vorbis_crc32_update(hash, segments);
	hash = vorbis_crc32_update(hash, body);
	if checksum != hash {
		// Do not verify checksum when the decoder is being fuzzed.
		// This allows random input from fuzzers reach decoding code that's actually interesting,
		// instead of being rejected early due to checksum mismatch.
		if !cfg!(fuzzing) {
			return Err(OggReadError::HashMismatch(checksum, hash));
		}
	}

	let page = Page {
		bi: PageBaseInfo {
			starts_with_continued,
			first_page: header_type_flag & 0x02 != 0,
			last_page: header_type_flag & 0x04 != 0,
			absgp: u64::from_le_bytes([
				header[6], header[7], header[8], header[9], header[10], header[11], header[12], header[13],
			]),
			packet_positions,
			ends_with_continued,
		},
		stream_serial: u32::from_le_bytes([header[14], header[15], header[16], header[17]]),
		packet_count,
		body,
	};
	Ok((page, PAGE_HEADER_LEN + segment_count + body_len))
}

/**
Reader for packets from an Ogg stream.

This reads codec packets belonging to several different logical streams from
one physical Ogg container stream, held entirely in memory.
*/
pub struct PacketReader<'a> {
	data: &'a [u8],
	/// Offset of the next page to parse
	pos: usize,

	/// State of every logical stream seen so far.
	///
	/// In practice a stream contains one or two logical streams whose setup
	/// changes very rarely, so a linear scan beats a hash map here, and it keeps
	/// the hashing code out of the binary.
	page_infos: Vec<(u32, PageInfo<'a>)>,

	/// Contains the stream_serial of the stream that contains some unprocessed packet data.
	/// There is always <= 1, bc if there is one, no new pages will be read, so there is no chance for a second to be added
	/// None if there is no such stream and one has to read a new page.
	stream_with_stuff: Option<u32>,

	/// Set to true once the reader dropped its unread packets, which makes the
	/// page checks tolerate the inconsistencies that follow from it.
	has_reset: bool,
}

impl<'a> PacketReader<'a> {
	/// Constructs a new `PacketReader` for a physical stream held in memory.
	pub fn new(data: &'a [u8]) -> PacketReader<'a> {
		PacketReader {
			data,
			pos: 0,
			page_infos: Vec::new(),
			stream_with_stuff: None,
			has_reset: false,
		}
	}

	/// Reads a packet, and returns it on success.
	///
	/// Ok(None) is returned if the physical stream has ended.
	pub fn read_packet(&mut self) -> Result<Option<Packet<'a>>, OggReadError> {
		// Read pages until we got a valid entire packet
		// (packets may span multiple pages, so reading one page
		// doesn't always suffice to give us a valid packet)
		loop {
			if let Some(pck) = self.take_packet() {
				return Ok(Some(pck));
			}
			match self.next_page()? {
				Some(page) => self.push_page(page)?,
				None => return Ok(None),
			}
		}
	}

	/// Reads a packet, and returns it on success.
	///
	/// The difference to the `read_packet` function is that this function
	/// returns an Err(_) if the physical stream has ended.
	/// This function is useful if you expect a new packet to come.
	pub fn read_packet_expected(&mut self) -> Result<Packet<'a>, OggReadError> {
		self.read_packet()?.ok_or(OggReadError::UnexpectedEndOfStream)
	}

	/// Resets the internal state by deleting all unread packets.
	pub fn delete_unread_packets(&mut self) {
		self.page_infos.clear();
		self.stream_with_stuff = None;
		self.has_reset = true;
	}

	/// Parses the next page, skipping over any data before its capture pattern.
	///
	/// Ok(None) is returned once the stream has ended.
	fn next_page(&mut self) -> Result<Option<Page<'a>>, OggReadError> {
		let rest = &self.data[self.pos..];
		if rest.is_empty() {
			return Ok(None);
		}
		// The ogg spec does not allow non page data after the last page, so
		// leftover bytes without a capture pattern are treated as corruption.
		let Some(offset) = find_capture_pattern(rest) else {
			return Err(OggReadError::NoCapturePatternFound);
		};
		if offset > MAX_SEARCH_LEN {
			return Err(OggReadError::NoCapturePatternFound);
		}
		let (page, page_len) = parse_page(&rest[offset..])?;
		self.pos += offset + page_len;
		Ok(Some(page))
	}

	/// Returns the state of the logical stream with the given serial number.
	fn page_info_mut(&mut self, stream_serial: u32) -> Option<&mut PageInfo<'a>> {
		self.page_infos
			.iter_mut()
			.find(|(serial, _)| *serial == stream_serial)
			.map(|(_, info)| info)
	}

	/// Extracts a packet from the cache, if the cache contains valid packet data,
	/// otherwise it returns `None`.
	fn take_packet(&mut self) -> Option<Packet<'a>> {
		let stream_serial = self.stream_with_stuff?;
		let pg_info = self.page_info_mut(stream_serial)?;
		let &(offs, len) = pg_info.bi.packet_positions.get(pg_info.packet_idx as usize)?;
		let (offs, len) = (offs as usize, len as usize);

		// If there is a continued packet, and we are at the start right now,
		// and we actually have its end in the current page, glue it together.
		let need_to_glue = pg_info.packet_idx == 0
			&& pg_info.bi.starts_with_continued
			&& !(pg_info.bi.ends_with_continued && pg_info.bi.packet_positions.len() == 1);
		let data: Cow<'a, [u8]> = if need_to_glue {
			let overlap_len: usize = pg_info.last_overlap_pck.iter().map(|part| part.len()).sum();
			let mut glued = Vec::with_capacity(overlap_len + len);
			for part in pg_info.last_overlap_pck.drain(..) {
				glued.extend_from_slice(part);
			}
			glued.extend_from_slice(&pg_info.page_body[offs..offs + len]);
			Cow::Owned(glued)
		} else {
			// The packet lies inside one page, so it can borrow the stream.
			Cow::Borrowed(&pg_info.page_body[offs..offs + len])
		};

		let first_pck_in_pg = pg_info.is_first_pck_in_pg();
		let first_pck_overall = pg_info.bi.first_page && first_pck_in_pg;

		let last_pck_in_pg = pg_info.is_last_pck_in_pg();
		let last_pck_overall = pg_info.bi.last_page && last_pck_in_pg;

		let absgp_page = pg_info.bi.absgp;

		// Update the last read index.
		pg_info.packet_idx += 1;
		// Set stream_with_stuff to None so that future packet reads
		// yield a page read first
		if last_pck_in_pg {
			self.stream_with_stuff = None;
		}

		Some(Packet {
			data,
			first_packet_pg: first_pck_in_pg,
			first_packet_stream: first_pck_overall,
			last_packet_pg: last_pck_in_pg,
			last_packet_stream: last_pck_overall,
			absgp_page,
			stream_serial,
		})
	}

	/// Adds a parsed page to the cache, updating the internal structures
	/// with its contents.
	fn push_page(&mut self, mut page: Page<'a>) -> Result<(), OggReadError> {
		let has_reset = self.has_reset;
		match self.page_info_mut(page.stream_serial) {
			Some(inf) => {
				if page.bi.first_page {
					return Err(OggReadError::InvalidData);
				}
				if page.bi.starts_with_continued != inf.bi.ends_with_continued {
					if !has_reset {
						return Err(OggReadError::InvalidData);
					} else {
						// If we have dropped unread packets, we are more tolerant
						// here, and just drop the continued packet's content.

						inf.last_overlap_pck.clear();
						if page.bi.starts_with_continued {
							page.bi.packet_positions.remove(0);
							if page.packet_count != 0 {
								// Decrease packet count by one. Normal case.
								page.packet_count -= 1;
							} else {
								// If the packet count is 0, this means
								// that we start and end with the same continued packet.
								// So now as we ignore that packet, we must clear the
								// ends_with_continued state as well.
								page.bi.ends_with_continued = false;
							}
						}
					}
				} else if page.bi.starts_with_continued {
					// Remember the packet at the end so that it can be glued together once
					// we encounter the next segment with length < 255 (doesnt have to be in this page)
					if let Some(&(offs, len)) = inf.bi.packet_positions.get(inf.packet_idx as usize) {
						let (offs, len) = (offs as usize, len as usize);
						inf.last_overlap_pck.push(&inf.page_body[offs..offs + len]);
					}
				}
				inf.bi = page.bi;
				inf.packet_idx = 0;
				inf.page_body = page.body;
			},
			None => {
				if !has_reset {
					if !page.bi.first_page || page.bi.starts_with_continued {
						// If we haven't dropped any packets, this is an error.
						return Err(OggReadError::InvalidData);
					}
				} else if page.bi.starts_with_continued {
					// Ignore the continued packet's content.
					// This is a normal occurence if we have just dropped packets.
					page.bi.packet_positions.remove(0);
					if page.packet_count != 0 {
						// Decrease packet count by one. Normal case.
						page.packet_count -= 1;
					} else {
						// If the packet count is 0, this means
						// that we start and end with the same continued packet.
						// So now as we ignore that packet, we must clear the
						// ends_with_continued state as well.
						page.bi.ends_with_continued = false;
					}
					// Not actually needed, but good for consistency
					page.bi.starts_with_continued = false;
				}
				self.page_infos.push((
					page.stream_serial,
					PageInfo {
						bi: page.bi,
						packet_idx: 0,
						page_body: page.body,
						last_overlap_pck: Vec::new(),
					},
				));
			},
		}

		self.stream_with_stuff = if page.packet_count > 0 {
			Some(page.stream_serial)
		} else {
			None
		};
		Ok(())
	}
}
