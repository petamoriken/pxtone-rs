//! Cursor over an in-memory pxtone file.
//!
//! Replaces `std::io::Read + Seek` in the parsers: every input is a byte buffer
//! already, and `io` is expensive in the wasm build (the boxed `io::Error` of
//! `Cursor::seek` alone pulls in `core::fmt`'s string formatting).

use crate::error::PxtoneError;

/// Mask 7 data bits variable-length integer byte.
const VAR_INT_MASK: u8 = 0x7F;
/// High bit: set means more bytes follow.
const VAR_INT_CONTINUATION: u8 = 0x80;

/// Reads little endian values out of a pxtone file held in memory.
pub(crate) struct Reader<'a> {
  data: &'a [u8],
  pos: usize,
}

impl<'a> Reader<'a> {
  pub(crate) fn new(data: &'a [u8]) -> Self {
    Self { data, pos: 0 }
  }

  /// Current offset from the start of the file.
  pub(crate) fn position(&self) -> u64 {
    self.pos as u64
  }

  /// Moves to an absolute offset. Seeking to the end of the file is allowed.
  pub(crate) fn set_position(&mut self, pos: u64) -> Result<(), PxtoneError> {
    let pos = usize::try_from(pos).map_err(|_| PxtoneError::BrokenFile)?;
    if pos > self.data.len() {
      return Err(PxtoneError::BrokenFile);
    }
    self.pos = pos;
    Ok(())
  }

  /// Skips `len` bytes.
  pub(crate) fn skip(&mut self, len: u64) -> Result<(), PxtoneError> {
    let len = usize::try_from(len).map_err(|_| PxtoneError::BrokenFile)?;
    let pos = self.pos.checked_add(len).ok_or(PxtoneError::BrokenFile)?;
    self.set_position(pos as u64)
  }

  /// Returns the next `len` bytes, borrowed from the file.
  pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8], PxtoneError> {
    let end = self.pos.checked_add(len).ok_or(PxtoneError::BrokenFile)?;
    let bytes = self
      .data
      .get(self.pos..end)
      .ok_or(PxtoneError::BrokenFile)?;
    self.pos = end;
    Ok(bytes)
  }

  /// Fills `buf` with the next bytes.
  pub(crate) fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), PxtoneError> {
    buf.copy_from_slice(self.take(buf.len())?);
    Ok(())
  }

  /// Returns the next `N` bytes as an array.
  fn take_array<const N: usize>(&mut self) -> Result<[u8; N], PxtoneError> {
    let mut bytes = [0u8; N];
    bytes.copy_from_slice(self.take(N)?);
    Ok(bytes)
  }

  pub(crate) fn read_u8(&mut self) -> Result<u8, PxtoneError> {
    Ok(self.take_array::<1>()?[0])
  }

  pub(crate) fn read_i8(&mut self) -> Result<i8, PxtoneError> {
    Ok(self.read_u8()? as i8)
  }

  pub(crate) fn read_u16(&mut self) -> Result<u16, PxtoneError> {
    Ok(u16::from_le_bytes(self.take_array()?))
  }

  pub(crate) fn read_i16(&mut self) -> Result<i16, PxtoneError> {
    Ok(i16::from_le_bytes(self.take_array()?))
  }

  pub(crate) fn read_u32(&mut self) -> Result<u32, PxtoneError> {
    Ok(u32::from_le_bytes(self.take_array()?))
  }

  pub(crate) fn read_i32(&mut self) -> Result<i32, PxtoneError> {
    Ok(i32::from_le_bytes(self.take_array()?))
  }

  pub(crate) fn read_f32(&mut self) -> Result<f32, PxtoneError> {
    Ok(f32::from_le_bytes(self.take_array()?))
  }

  /// Reads a pxtone variable-length integer (up to 5 bytes) as `u32`.
  pub(crate) fn read_var_u32(&mut self) -> Result<u32, PxtoneError> {
    let mut bytes = [0u8; 5];
    let mut count = 0usize;
    for (i, byte) in bytes.iter_mut().enumerate() {
      *byte = self.read_u8()?;
      count = i + 1;
      if *byte & VAR_INT_CONTINUATION == 0 {
        break;
      }
    }
    Ok(v_to_int(&bytes[..count]))
  }

  /// Reads a pxtone variable-length integer (up to 5 bytes) as `i32`.
  pub(crate) fn read_var_i32(&mut self) -> Result<i32, PxtoneError> {
    self.read_var_u32().map(|v| v as i32)
  }
}

/// Assembles the 7-bit groups of a pxtone variable-length integer.
fn v_to_int(bytes: &[u8]) -> u32 {
  let mut value: u32 = 0;
  for (i, byte) in bytes.iter().enumerate() {
    value |= ((byte & VAR_INT_MASK) as u32) << (7 * i);
  }
  value
}

#[cfg(test)]
mod tests {
  use super::{Reader, v_to_int};

  #[test]
  fn reads_little_endian_values() {
    let data = [0x01u8, 0x02, 0x03, 0x00, 0x00, 0x80, 0x3f];
    let mut r = Reader::new(&data);
    assert_eq!(r.read_u8().unwrap(), 1);
    assert_eq!(r.read_u16().unwrap(), 0x0302);
    assert_eq!(r.read_f32().unwrap(), 1.0);
    assert_eq!(r.position(), 7);
    assert!(r.read_u8().is_err());
  }

  #[test]
  fn seeks_within_the_file() {
    let data = [0u8; 4];
    let mut r = Reader::new(&data);
    r.skip(4).unwrap();
    assert_eq!(r.position(), 4);
    assert!(r.skip(1).is_err());
    r.set_position(2).unwrap();
    assert_eq!(r.position(), 2);
    assert!(r.set_position(5).is_err());
  }

  #[test]
  fn decodes_variable_length_integers() {
    // The 7 bit groups are little endian, the high bit marks continuation.
    assert_eq!(v_to_int(&[0x00]), 0);
    assert_eq!(v_to_int(&[0x7f]), 127);
    assert_eq!(v_to_int(&[0x80, 0x01]), 128);
    assert_eq!(v_to_int(&[0xff, 0x7f]), 16383);
    assert_eq!(v_to_int(&[0x80, 0x80, 0x80, 0x80, 0x0f]), 0xf000_0000);

    let data = [0x80u8, 0x01, 0x7f];
    let mut r = Reader::new(&data);
    assert_eq!(r.read_var_u32().unwrap(), 128);
    assert_eq!(r.read_var_i32().unwrap(), 127);
  }
}
