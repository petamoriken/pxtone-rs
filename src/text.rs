use std::io::{Read, Seek};
use byteorder::{LE, ReadBytesExt};
use crate::error::PxtoneError;

#[derive(Debug, Default)]
pub struct Text {
    pub name   : Option<String>,
    pub comment: Option<String>,
}

impl Text {
    pub fn new() -> Self {
        Self::default()
    }

    /// i32 サイズ + バイト列 の形式でテキストを読み込む
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
    if size < 0 { return Err(PxtoneError::BrokenFile); }
    let size = size as usize;
    let mut buf = vec![0u8; size];
    r.read_exact(&mut buf)?;
    // ヌル終端を除去してそのままバイト列を文字列化（元の C++ は生バッファ）
    let end = buf.iter().position(|&b| b == 0).unwrap_or(size);
    Ok(String::from_utf8_lossy(&buf[..end]).into_owned())
}
