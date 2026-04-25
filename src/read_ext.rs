use crate::error::PxtoneError;
use byteorder::ReadBytesExt;
use std::io::Read;

pub(crate) trait ReadExt: Read {
  /// Reads a pxtone variable-length integer (up to 5 bytes)
  fn read_var_int(&mut self) -> Result<i32, PxtoneError>;
}

impl<R: Read> ReadExt for R {
  fn read_var_int(&mut self) -> Result<i32, PxtoneError> {
    let mut bytes = [0u8; 5];
    let mut count = 0usize;

    for (i, byte) in bytes.iter_mut().enumerate() {
      *byte = self.read_u8()?;
      count = i + 1;
      if *byte & 0x80 == 0 {
        break;
      }
    }

    Ok(v_to_int(&bytes[..count]) as i32)
  }
}

fn v_to_int(bytes: &[u8]) -> u32 {
  let mut b = [0u8; 5];
  match bytes.len() {
    0 => {}
    1 => {
      b[0] = bytes[0] & 0x7F;
    }
    2 => {
      b[0] = (bytes[0] & 0x7F) | (bytes[1] << 7);
      b[1] = (bytes[1] & 0x7F) >> 1;
    }
    3 => {
      b[0] = (bytes[0] & 0x7F) | (bytes[1] << 7);
      b[1] = ((bytes[1] & 0x7F) >> 1) | (bytes[2] << 6);
      b[2] = (bytes[2] & 0x7F) >> 2;
    }
    4 => {
      b[0] = (bytes[0] & 0x7F) | (bytes[1] << 7);
      b[1] = ((bytes[1] & 0x7F) >> 1) | (bytes[2] << 6);
      b[2] = ((bytes[2] & 0x7F) >> 2) | (bytes[3] << 5);
      b[3] = (bytes[3] & 0x7F) >> 3;
    }
    _ => {
      b[0] = (bytes[0] & 0x7F) | (bytes[1] << 7);
      b[1] = ((bytes[1] & 0x7F) >> 1) | (bytes[2] << 6);
      b[2] = ((bytes[2] & 0x7F) >> 2) | (bytes[3] << 5);
      b[3] = ((bytes[3] & 0x7F) >> 3) | (bytes[4] << 4);
    }
  }
  u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
