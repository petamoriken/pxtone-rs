use std::io::*;

use byteorder::{LittleEndian, ReadBytesExt};


pub struct Point {
    pub x: i32,
    pub y: i32
}


pub trait Descriptor: BufRead + Seek {
    fn read_i32_flex(&mut self) -> Result<i32>;
    fn read_u32_flex(&mut self) -> Result<u32>;
    fn read_f32_flex(&mut self) -> Result<f32>;
}

#[inline]
fn read_32<T: AsRef<[u8]>>(bytes: &mut Cursor<T>) -> Result<[u8; 5]> {
    let mut r = Vec::new();

    for _ in 0 .. 5 {
        let buf = bytes.read_u8()?;
        r.push(buf);
        if buf & 0x80 == 0 { break; }
    }

    let mut buf = [0; 5];

    let length = r.len();
    match length {
        1 => {
            buf[0] = (r[0] & 0x7F) >> 0;
        }
        2 => {
            buf[0] = ((r[0] & 0x7F) >> 0) | (r[1] << 7);
            buf[1] =  (r[1] & 0x7F) >> 1;
        }
        3 => {
            buf[0] = ((r[0] & 0x7F) >> 0) | (r[1] << 7);
            buf[1] = ((r[1] & 0x7F) >> 1) | (r[2] << 6);
            buf[2] =  (r[2] & 0x7F) >> 2;
        }
        4 => {
            buf[0] = ((r[0] & 0x7F) >> 0) | (r[1] << 7);
            buf[1] = ((r[1] & 0x7F) >> 1) | (r[2] << 6);
            buf[2] = ((r[2] & 0x7F) >> 2) | (r[3] << 5);
            buf[3] =  (r[3] & 0x7F) >> 3;
        }
        5 => {
            buf[0] = ((r[0] & 0x7F) >> 0) | (r[1] << 7);
            buf[1] = ((r[1] & 0x7F) >> 1) | (r[2] << 6);
            buf[2] = ((r[2] & 0x7F) >> 2) | (r[3] << 5);
            buf[3] = ((r[3] & 0x7F) >> 3) | (r[4] << 4);
            buf[4] =  (r[4] & 0x7F) >> 4;
        }
        _ => panic!("Unreachable")
    }

    Ok(buf)
}

impl<T> Descriptor for Cursor<T> where T: AsRef<[u8]> {
    fn read_i32_flex(&mut self) -> Result<i32> {
        Ok(Cursor::new(read_32(self)?).read_i32::<LittleEndian>()?)
    }

    fn read_u32_flex(&mut self) -> Result<u32> {
        Ok(Cursor::new(read_32(self)?).read_u32::<LittleEndian>()?)
    }

    fn read_f32_flex(&mut self) -> Result<f32> {
        Ok(Cursor::new(read_32(self)?).read_f32::<LittleEndian>()?)
    }
}
