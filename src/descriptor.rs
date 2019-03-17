use std::{io::{Read, Seek, Result}, mem, unreachable};
use byteorder::{ReadBytesExt as _};

#[inline]
fn read_32_flex<T: Read + Seek + ?Sized>(bytes: &mut T) -> Result<[u8; 4]> {
    let mut r = [0; 5];
    let mut len = 0;
    for val in r.iter_mut() {
        let byte = bytes.read_u8()?;
        *val = byte;
        len += 1;
        if byte & 0x80 == 0 { break; }
    }
    let r = r;
    let len = len;

    let mut buf = [0; 4];
    if cfg!(target_endian = "little") {
        match len {
            1 => {
                buf[0] =   r[0] & 0x7F;
            }
            2 => {
                buf[0] =  (r[0] & 0x7F)       | (r[1] << 7);
                buf[1] =  (r[1] & 0x7F) >> 1;
            }
            3 => {
                buf[0] =  (r[0] & 0x7F)       | (r[1] << 7);
                buf[1] = ((r[1] & 0x7F) >> 1) | (r[2] << 6);
                buf[2] =  (r[2] & 0x7F) >> 2;
            }
            4 => {
                buf[0] =  (r[0] & 0x7F)       | (r[1] << 7);
                buf[1] = ((r[1] & 0x7F) >> 1) | (r[2] << 6);
                buf[2] = ((r[2] & 0x7F) >> 2) | (r[3] << 5);
                buf[3] =  (r[3] & 0x7F) >> 3;
            }
            5 => {
                buf[0] =  (r[0] & 0x7F)       | (r[1] << 7);
                buf[1] = ((r[1] & 0x7F) >> 1) | (r[2] << 6);
                buf[2] = ((r[2] & 0x7F) >> 2) | (r[3] << 5);
                buf[3] = ((r[3] & 0x7F) >> 3) | (r[4] << 4);
            }
            _ => unreachable!()
        }
    } else {
        match len {
            1 => {
                buf[3] =   r[0] & 0x7F;
            }
            2 => {
                buf[3] =  (r[0] & 0x7F)       | (r[1] << 7);
                buf[2] =  (r[1] & 0x7F) >> 1;
            }
            3 => {
                buf[3] =  (r[0] & 0x7F)       | (r[1] << 7);
                buf[2] = ((r[1] & 0x7F) >> 1) | (r[2] << 6);
                buf[1] =  (r[2] & 0x7F) >> 2;
            }
            4 => {
                buf[3] =  (r[0] & 0x7F)       | (r[1] << 7);
                buf[2] = ((r[1] & 0x7F) >> 1) | (r[2] << 6);
                buf[1] = ((r[2] & 0x7F) >> 2) | (r[3] << 5);
                buf[0] =  (r[3] & 0x7F) >> 3;
            }
            5 => {
                buf[3] =  (r[0] & 0x7F)       | (r[1] << 7);
                buf[2] = ((r[1] & 0x7F) >> 1) | (r[2] << 6);
                buf[1] = ((r[2] & 0x7F) >> 2) | (r[3] << 5);
                buf[0] = ((r[3] & 0x7F) >> 3) | (r[4] << 4);
            }
            _ => unreachable!()
        }
    }

    Ok(buf)
}

pub(crate) trait ReadBytesExt: Read + Seek {
    fn read_i32_flex(&mut self) -> Result<i32> {
        Ok(unsafe { mem::transmute::<[u8; 4], i32>(read_32_flex(self)?) })
    }

    fn read_u32_flex(&mut self) -> Result<u32> {
        Ok(unsafe { mem::transmute::<[u8; 4], u32>(read_32_flex(self)?) })
    }

    fn read_f32_flex(&mut self) -> Result<f32> {
        Ok(unsafe { mem::transmute::<[u8; 4], f32>(read_32_flex(self)?) })
    }
}

impl<R: Read + Seek + ?Sized> ReadBytesExt for R {}
