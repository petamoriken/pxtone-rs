use crate::error::PxtoneError;
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek};

#[derive(Debug, Default)]
pub struct Text {
  pub name: Option<String>,
  pub comment: Option<String>,
}

impl Text {
  pub fn new() -> Self {
    Self::default()
  }

  /// Reads text in i32 size + byte sequence format
  pub fn read_name<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    self.name = Some(read_text(r)?);
    Ok(())
  }

  pub fn read_comment<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    self.comment = Some(read_text(r)?);
    Ok(())
  }
}

fn read_text<R: Read>(r: &mut R) -> Result<String, PxtoneError> {
  let size = r.read_i32::<LE>()?;
  if size < 0 {
    return Err(PxtoneError::BrokenFile);
  }
  let size = size as usize;
  let mut buf = vec![0u8; size];
  r.read_exact(&mut buf)?;
  // Strip null terminator and convert raw bytes to string (original C++ uses raw buffer)
  let end = buf.iter().position(|&b| b == 0).unwrap_or(size);
  Ok(String::from_utf8_lossy(&buf[..end]).into_owned())
}
