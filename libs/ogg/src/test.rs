// Ogg decoder and encoder written in Rust
//
// Copyright (c) 2016-2017 est31 <MTest31@outlook.com>
// and contributors. All rights reserved.
// Redistribution or use only under the terms
// specified in the LICENSE file attached to this
// source distribution.

use std::boxed::Box;
use std::vec::Vec;

use super::*;
use crate::writing::{PacketWriteEndInfo, PacketWriter};

use std::io::{Cursor, Seek, SeekFrom};

macro_rules! test_arr_eq {
	($a_arr:expr_2021, $b_arr:expr_2021) => {
		let a_arr = &$a_arr;
		let b_arr = &$b_arr;
		for i in 0..b_arr.len() {
			if a_arr[i] != b_arr[i] {
				panic!("Mismatch of values at index {}: {} {}", i, a_arr[i], b_arr[i]);
			}
		}
	};
}

#[test]
fn test_packet_rw() {
	let mut c = Cursor::new(Vec::new());
	let test_arr = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
	let test_arr_2 = [2, 4, 8, 16, 32, 64, 128, 127, 126, 125, 124];
	let test_arr_3 = [3, 5, 9, 17, 33, 65, 129, 129, 127, 126, 125];
	{
		let mut w = PacketWriter::new(&mut c);
		let np = PacketWriteEndInfo::NormalPacket;
		w.write_packet(Box::new(test_arr), 0xdeadb33f, np, 0).unwrap();
		w.write_packet(Box::new(test_arr_2), 0xdeadb33f, np, 1).unwrap();
		w.write_packet(Box::new(test_arr_3), 0xdeadb33f, PacketWriteEndInfo::EndPage, 2)
			.unwrap();
	}
	//print_u8_slice(c.get_ref());
	assert_eq!(c.seek(SeekFrom::Start(0)).unwrap(), 0);
	{
		let mut r = PacketReader::new(c.get_ref());
		let p1 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr, *p1.data);
		let p2 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr_2, *p2.data);
		let p3 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr_3, *p3.data);
	}

	// Now test packets spanning multiple segments
	let mut c = Cursor::new(Vec::new());
	let test_arr = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
	let mut test_arr_2 = [0; 700];
	let test_arr_3 = [3, 5, 9, 17, 33, 65, 129, 129, 127, 126, 125];
	for (idx, a) in test_arr_2.iter_mut().enumerate() {
		*a = (idx as u8) / 4;
	}
	{
		let mut w = PacketWriter::new(&mut c);
		let np = PacketWriteEndInfo::NormalPacket;
		w.write_packet(Box::new(test_arr), 0xdeadb33f, np, 0).unwrap();
		w.write_packet(Box::new(test_arr_2), 0xdeadb33f, np, 1).unwrap();
		w.write_packet(Box::new(test_arr_3), 0xdeadb33f, PacketWriteEndInfo::EndPage, 2)
			.unwrap();
	}
	//print_u8_slice(c.get_ref());
	assert_eq!(c.seek(SeekFrom::Start(0)).unwrap(), 0);
	{
		let mut r = PacketReader::new(c.get_ref());
		let p1 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr, *p1.data);
		let p2 = r.read_packet().unwrap().unwrap();
		test_arr_eq!(test_arr_2, *p2.data);
		let p3 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr_3, *p3.data);
	}

	// Now test packets spanning multiple pages
	let mut c = Cursor::new(Vec::new());
	let mut test_arr_2 = [0; 14_000];
	let test_arr_3 = [3, 5, 9, 17, 33, 65, 129, 129, 127, 126, 125];
	for (idx, a) in test_arr_2.iter_mut().enumerate() {
		*a = (idx as u8) / 4;
	}
	{
		let mut w = PacketWriter::new(&mut c);
		let np = PacketWriteEndInfo::NormalPacket;
		w.write_packet(Box::new(test_arr_2), 0xdeadb33f, np, 1).unwrap();
		w.write_packet(Box::new(test_arr_3), 0xdeadb33f, PacketWriteEndInfo::EndPage, 2)
			.unwrap();
	}
	//print_u8_slice(c.get_ref());
	assert_eq!(c.seek(SeekFrom::Start(0)).unwrap(), 0);
	{
		let mut r = PacketReader::new(c.get_ref());
		let p2 = r.read_packet().unwrap().unwrap();
		test_arr_eq!(test_arr_2, *p2.data);
		let p3 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr_3, *p3.data);
	}
}

#[test]
fn test_page_end_after_first_packet() {
	// Test that everything works well if we force a page end
	// after the first packet
	let mut c = Cursor::new(Vec::new());
	let test_arr = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
	let test_arr_2 = [2, 4, 8, 16, 32, 64, 128, 127, 126, 125, 124];
	let test_arr_3 = [3, 5, 9, 17, 33, 65, 129, 129, 127, 126, 125];
	{
		let mut w = PacketWriter::new(&mut c);
		let np = PacketWriteEndInfo::NormalPacket;
		w.write_packet(Box::new(test_arr), 0xdeadb33f, PacketWriteEndInfo::EndPage, 0)
			.unwrap();
		w.write_packet(Box::new(test_arr_2), 0xdeadb33f, np, 1).unwrap();
		w.write_packet(Box::new(test_arr_3), 0xdeadb33f, PacketWriteEndInfo::EndPage, 2)
			.unwrap();
	}
	//print_u8_slice(c.get_ref());
	assert_eq!(c.seek(SeekFrom::Start(0)).unwrap(), 0);
	{
		let mut r = PacketReader::new(c.get_ref());
		let p1 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr, *p1.data);
		let p2 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr_2, *p2.data);
		let p3 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr_3, *p3.data);
	}
}

#[test]
fn test_packet_write() {
	let mut c = Cursor::new(Vec::new());

	// Test page taken from real Ogg file
	let test_arr_out = [
		0x4f, 0x67, 0x67, 0x53, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x74, 0xa3, 0x90, 0x5b,
		0x00, 0x00, 0x00, 0x00, 0x6d, 0x94, 0x4e, 0x3d, 0x01, 0x1e, 0x01, 0x76, 0x6f, 0x72, 0x62, 0x69, 0x73, 0x00,
		0x00, 0x00, 0x00, 0x02, 0x44, 0xac, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0xb5, 0x01, 0x00, 0x00, 0x00,
		0x00, 0x00, 0xb8, 0x01u8,
	];
	let test_arr_in = [
		0x01, 0x76, 0x6f, 0x72, 0x62, 0x69, 0x73, 0x00, 0x00, 0x00, 0x00, 0x02, 0x44, 0xac, 0x00, 0x00, 0x00, 0x00,
		0x00, 0x00, 0x80, 0xb5, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xb8, 0x01u8,
	];

	{
		let mut w = PacketWriter::new(&mut c);
		w.write_packet(Box::new(test_arr_in), 0x5b90a374, PacketWriteEndInfo::EndPage, 0)
			.unwrap();
	}
	//print_u8_slice(c.get_ref());
	assert_eq!(c.get_ref().len(), test_arr_out.len());

	let cr = c.get_ref();
	test_arr_eq!(cr, test_arr_out);
}

#[test]
fn test_write_large() {
	// Test that writing an overlarge packet works,
	// aka where a new page is forced by the
	// first packet in the page.

	let mut c = Cursor::new(Vec::new());

	// A page can contain at most 255 * 255 = 65025
	// bytes of payload packet data.
	// A length of 70_00 will guaranteed create a page break.
	let test_arr = gen_pck(1234, 70_000 / 4);
	{
		let mut w = PacketWriter::new(&mut c);
		w.write_packet(test_arr.clone(), 0x5b90a374, PacketWriteEndInfo::EndPage, 0)
			.unwrap();
	}
	//print_u8_slice(c.get_ref());

	assert_eq!(c.seek(SeekFrom::Start(0)).unwrap(), 0);
	{
		let mut r = PacketReader::new(c.get_ref());
		let p = r.read_packet().unwrap().unwrap();
		test_arr_eq!(test_arr, *p.data);
	}
}

struct XorShift {
	state: (u32, u32, u32, u32),
}
impl XorShift {
	fn from_two(seed: (u32, u32)) -> Self {
		let mut xs = XorShift {
			state: (
				seed.0 ^ 0x2a24a930,
				seed.1 ^ 0xa9f60227,
				!seed.0 ^ 0x68c44d2d,
				!seed.1 ^ 0xa1f9794a,
			),
		};
		xs.next();
		xs.next();
		xs.next();
		xs
	}

	fn next(&mut self) -> u32 {
		let mut r = self.state.3;
		r ^= r << 11;
		r ^= r >> 8;
		self.state.3 = self.state.2;
		self.state.2 = self.state.1;
		self.state.1 = self.state.0;
		r ^= self.state.0;
		r ^= self.state.0 >> 19;
		self.state.0 = r;
		r
	}
}

fn gen_pck(seed: u32, len_d_four: usize) -> Box<[u8]> {
	let mut ret = Vec::with_capacity(len_d_four * 4);
	let mut xs = XorShift::from_two((seed, len_d_four as u32));
	if len_d_four > 0 {
		ret.push(seed as u8);
		ret.push((seed >> 8) as u8);
		ret.push((seed >> 16) as u8);
		ret.push((seed >> 24) as u8);
	}
	for _ in 1..len_d_four {
		let v = xs.next();
		ret.push(v as u8);
		ret.push((v >> 8) as u8);
		ret.push((v >> 16) as u8);
		ret.push((v >> 24) as u8);
	}
	ret.into_boxed_slice()
}

// Upstream ogg can seek inside the physical stream; this fork always decodes
// from the start, so the seeking tests are gone along with the feature.

// Regression test for issue 14:
// Have "O" right before the OggS magic.
#[test]
fn test_issue_14() {
	let mut c = Cursor::new(Vec::new());
	let test_arr = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
	let test_arr_2 = [2, 4, 8, 16, 32, 64, 128, 127, 126, 125, 124];
	let test_arr_3 = [3, 5, 9, 17, 33, 65, 129, 129, 127, 126, 125];
	{
		use std::io::Write;
		c.write_all(&[b'O']).unwrap();
		let mut w = PacketWriter::new(&mut c);
		let np = PacketWriteEndInfo::NormalPacket;
		w.write_packet(Box::new(test_arr), 0xdeadb33f, np, 0).unwrap();
		w.write_packet(Box::new(test_arr_2), 0xdeadb33f, np, 1).unwrap();
		w.write_packet(Box::new(test_arr_3), 0xdeadb33f, PacketWriteEndInfo::EndPage, 2)
			.unwrap();
	}
	//print_u8_slice(c.get_ref());
	assert_eq!(c.seek(SeekFrom::Start(0)).unwrap(), 0);
	{
		let mut r = PacketReader::new(c.get_ref());
		let p1 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr, *p1.data);
		let p2 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr_2, *p2.data);
		let p3 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr_3, *p3.data);
	}

	// Now test packets spanning multiple segments
	let mut c = Cursor::new(Vec::new());
	let test_arr = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
	let mut test_arr_2 = [0; 700];
	let test_arr_3 = [3, 5, 9, 17, 33, 65, 129, 129, 127, 126, 125];
	for (idx, a) in test_arr_2.iter_mut().enumerate() {
		*a = (idx as u8) / 4;
	}
	{
		let mut w = PacketWriter::new(&mut c);
		let np = PacketWriteEndInfo::NormalPacket;
		w.write_packet(Box::new(test_arr), 0xdeadb33f, np, 0).unwrap();
		w.write_packet(Box::new(test_arr_2), 0xdeadb33f, np, 1).unwrap();
		w.write_packet(Box::new(test_arr_3), 0xdeadb33f, PacketWriteEndInfo::EndPage, 2)
			.unwrap();
	}
	//print_u8_slice(c.get_ref());
	assert_eq!(c.seek(SeekFrom::Start(0)).unwrap(), 0);
	{
		let mut r = PacketReader::new(c.get_ref());
		let p1 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr, *p1.data);
		let p2 = r.read_packet().unwrap().unwrap();
		test_arr_eq!(test_arr_2, *p2.data);
		let p3 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr_3, *p3.data);
	}

	// Now test packets spanning multiple pages
	let mut c = Cursor::new(Vec::new());
	let mut test_arr_2 = [0; 14_000];
	let test_arr_3 = [3, 5, 9, 17, 33, 65, 129, 129, 127, 126, 125];
	for (idx, a) in test_arr_2.iter_mut().enumerate() {
		*a = (idx as u8) / 4;
	}
	{
		let mut w = PacketWriter::new(&mut c);
		let np = PacketWriteEndInfo::NormalPacket;
		w.write_packet(Box::new(test_arr_2), 0xdeadb33f, np, 1).unwrap();
		w.write_packet(Box::new(test_arr_3), 0xdeadb33f, PacketWriteEndInfo::EndPage, 2)
			.unwrap();
	}
	//print_u8_slice(c.get_ref());
	assert_eq!(c.seek(SeekFrom::Start(0)).unwrap(), 0);
	{
		let mut r = PacketReader::new(c.get_ref());
		let p2 = r.read_packet().unwrap().unwrap();
		test_arr_eq!(test_arr_2, *p2.data);
		let p3 = r.read_packet().unwrap().unwrap();
		assert_eq!(test_arr_3, *p3.data);
	}
}
