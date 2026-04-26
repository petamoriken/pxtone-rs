use crate::error::PxtoneError;
use byteorder::{LE, ReadBytesExt};
use std::io::Read;

/// Song title and comment text loaded from the file.
///
/// Both fields are stored as raw Shift-JIS bytes as written in the file.
/// Use an encoding library such as `encoding_rs` to convert them to UTF-8.
#[derive(Debug, Default)]
pub struct Text {
  name: Option<Vec<u8>>,
  comment: Option<Vec<u8>>,
}

impl Text {
  pub fn new() -> Self {
    Self::default()
  }

  pub(crate) fn read_name<R: Read>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    self.name = Some(read_raw(r)?);
    Ok(())
  }

  pub(crate) fn read_comment<R: Read>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    self.comment = Some(read_raw(r)?);
    Ok(())
  }

  pub(crate) fn set_name_raw(&mut self, raw: &[u8]) {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    self.name = Some(raw[..end].to_vec());
  }

  /// Returns the song title as raw Shift-JIS bytes, or `None` if not set.
  pub fn name(&self) -> Option<&[u8]> {
    self.name.as_deref()
  }

  /// Returns the song comment as raw Shift-JIS bytes, or `None` if not set.
  pub fn comment(&self) -> Option<&[u8]> {
    self.comment.as_deref()
  }
}

fn read_raw<R: Read>(r: &mut R) -> Result<Vec<u8>, PxtoneError> {
  let size = r.read_i32::<LE>()?;
  if size < 0 {
    return Err(PxtoneError::BrokenFile);
  }
  let size = size as usize;
  let mut buf = vec![0u8; size];
  r.read_exact(&mut buf)?;
  let end = buf.iter().position(|&b| b == 0).unwrap_or(size);
  Ok(buf[..end].to_vec())
}
