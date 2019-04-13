use byteorder::ReadBytesExt as _;
use std::{io::Read, mem};

use crate::error::{Error, ErrorKind};

/// LEB128 limited to 32 bits
#[inline]
fn read_var_32<T: Read + ?Sized>(bytes: &mut T) -> Result<u32, Error> {
    let mut result = 0;

    for i in 0..5 {
        let byte = bytes.read_u8()? as u32;
        result |= (byte & 0x7F) << (i * 7);
        if byte & 0x80 == 0 { break; }
        else if i == 4 { return Err(Error::from(ErrorKind::InvalidVar32)) }
    }

    Ok(result)
}

pub(crate) trait ReadBytesExt: Read {
    fn read_var_u32(&mut self) -> Result<u32, Error> {
        read_var_32(self)
    }

    fn read_var_i32(&mut self) -> Result<i32, Error> {
        Ok(read_var_32(self)? as i32)
    }

    fn read_var_f32(&mut self) -> Result<f32, Error> {
        Ok(unsafe { mem::transmute::<u32, f32>(read_var_32(self)?) })
    }
}

impl<R: Read + ?Sized> ReadBytesExt for R {}
