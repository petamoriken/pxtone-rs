use crate::error::PxtoneError;
use byteorder::{LE, ReadBytesExt};
use encoding_rs::SHIFT_JIS;
use std::io::Read;

#[derive(Debug, Default)]
pub struct Text {
  name_raw: Option<Vec<u8>>,    // raw Shift-JIS bytes (no null terminator)
  comment_raw: Option<Vec<u8>>, // raw Shift-JIS bytes (no null terminator)
}

impl Text {
  pub fn new() -> Self {
    Self::default()
  }

  /// Reads text in i32 size + byte sequence format and stores raw Shift-JIS bytes.
  /// The Shift-JIS → UTF-8 conversion is deferred until name() is called.
  pub fn read_name<R: Read>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    self.name_raw = Some(read_raw(r)?);
    Ok(())
  }

  /// Reads text in i32 size + byte sequence format and stores raw Shift-JIS bytes.
  /// The Shift-JIS → UTF-8 conversion is deferred until comment() is called.
  pub fn read_comment<R: Read>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    self.comment_raw = Some(read_raw(r)?);
    Ok(())
  }

  /// Sets the name from a raw Shift-JIS byte slice (null terminator is trimmed).
  pub(crate) fn set_name_raw(&mut self, raw: &[u8]) {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    self.name_raw = Some(raw[..end].to_vec());
  }

  /// Decodes the name from Shift-JIS to UTF-8 on demand.
  pub fn name(&self) -> Option<String> {
    self.name_raw.as_ref().map(|b| decode_sjis(b))
  }

  /// Decodes the comment from Shift-JIS to UTF-8 on demand.
  pub fn comment(&self) -> Option<String> {
    self.comment_raw.as_ref().map(|b| decode_sjis(b))
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
  // Strip null terminator (original C++ uses raw buffer)
  let end = buf.iter().position(|&b| b == 0).unwrap_or(size);
  Ok(buf[..end].to_vec())
}

fn decode_sjis(raw: &[u8]) -> String {
  let (decoded, _, _) = SHIFT_JIS.decode(raw);
  decoded.into_owned()
}
