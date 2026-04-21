// PCM buffer (pxtnPulse_PCM)
// RIFF WAV loading and channel/bit-depth/sample-rate conversion

use crate::error::PxtoneError;
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};

#[derive(Debug)]
pub struct Pcm {
  pub ch: i32,
  pub sps: i32,
  pub bps: i32,
  pub smp_head: i32,
  pub smp_body: i32,
  pub smp_tail: i32,
  samples: Vec<u8>,
}

impl Pcm {
  pub fn create(ch: i32, sps: i32, bps: i32, sample_num: i32) -> Option<Self> {
    if bps != 8 && bps != 16 {
      return None;
    }
    let size = (sample_num * bps * ch / 8) as usize;
    let fill = if bps == 8 { 128u8 } else { 0u8 };
    Some(Self {
      ch,
      sps,
      bps,
      smp_head: 0,
      smp_body: sample_num,
      smp_tail: 0,
      samples: vec![fill; size],
    })
  }

  pub fn samples(&self) -> &[u8] {
    &self.samples
  }
  pub fn samples_mut(&mut self) -> &mut [u8] {
    &mut self.samples
  }

  pub fn buf_size(&self) -> usize {
    ((self.smp_head + self.smp_body + self.smp_tail) * self.ch * self.bps / 8) as usize
  }

  pub fn get_sec(&self) -> f32 {
    (self.smp_head + self.smp_body + self.smp_tail) as f32 / self.sps as f32
  }

  /// Reads a RIFF WAV file
  pub fn read_wav<R: Read + Seek>(r: &mut R) -> Result<Self, PxtoneError> {
    // 16-byte "RIFFxxxxWAVEfmt " header
    let mut header = [0u8; 16];
    r.read_exact(&mut header)?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" || &header[12..16] != b"fmt " {
      return Err(PxtoneError::UnknownFormat);
    }

    // fmt chunk size
    let _fmt_size = r.read_u32::<LE>()?;

    // WAVEFORMATEX (read 18 bytes)
    let format_id = r.read_u16::<LE>()?;
    let ch = r.read_u16::<LE>()? as i32;
    let sps = r.read_u32::<LE>()? as i32;
    let _byte_per_s = r.read_u32::<LE>()?;
    let _block_size = r.read_u16::<LE>()?;
    let bps = r.read_u16::<LE>()? as i32;
    let _ext = r.read_u16::<LE>()?;

    if format_id != 0x0001 {
      return Err(PxtoneError::UnknownFormat);
    }
    if ch != 1 && ch != 2 {
      return Err(PxtoneError::UnknownFormat);
    }
    if bps != 8 && bps != 16 {
      return Err(PxtoneError::UnknownFormat);
    }

    // Search for the "data" chunk (seek to start of "RIFFxxxxWAVE" = 12 bytes)
    r.seek(SeekFrom::Start(12))?;
    let data_size = loop {
      let mut tag = [0u8; 4];
      r.read_exact(&mut tag)?;
      let chunk_size = r.read_u32::<LE>()?;
      if &tag == b"data" {
        break chunk_size;
      }
      r.seek(SeekFrom::Current(chunk_size as i64))?;
    };

    let sample_num = data_size as i32 * 8 / bps / ch;
    let mut pcm = Self::create(ch, sps, bps, sample_num).ok_or(PxtoneError::UnknownFormat)?;
    r.read_exact(&mut pcm.samples[..data_size as usize])?;

    Ok(pcm)
  }

  // ---- Conversion ----

  pub fn convert(&mut self, new_ch: i32, new_sps: i32, new_bps: i32) -> Result<(), PxtoneError> {
    self.convert_channel(new_ch)?;
    self.convert_bps(new_bps)?;
    self.convert_sps(new_sps)?;
    Ok(())
  }

  fn total_samples(&self) -> i32 {
    self.smp_head + self.smp_body + self.smp_tail
  }

  fn convert_channel(&mut self, new_ch: i32) -> Result<(), PxtoneError> {
    if self.ch == new_ch {
      return Ok(());
    }
    let total = self.total_samples() as usize;
    let work = match (self.ch, new_ch) {
      // mono → stereo
      (1, 2) => {
        let mut w = vec![0u8; total * self.bps as usize / 8 * 2];
        match self.bps {
          8 => {
            for (i, &b) in self.samples.iter().enumerate() {
              w[i * 2] = b;
              w[i * 2 + 1] = b;
            }
          }
          16 => {
            for i in 0..total {
              let s = &self.samples[i * 2..i * 2 + 2];
              w[i * 4] = s[0];
              w[i * 4 + 1] = s[1];
              w[i * 4 + 2] = s[0];
              w[i * 4 + 3] = s[1];
            }
          }
          _ => return Err(PxtoneError::PcmConvert),
        }
        w
      }
      // stereo → mono
      (2, 1) => {
        let mut w = vec![0u8; total * self.bps as usize / 8 / 2];
        match self.bps {
          8 => {
            for i in 0..total / 2 {
              let a = self.samples[i * 2] as i32;
              let b = self.samples[i * 2 + 1] as i32;
              w[i] = ((a + b) / 2) as u8;
            }
          }
          16 => {
            for i in 0..total / 2 {
              let a = i16::from_le_bytes([self.samples[i * 4], self.samples[i * 4 + 1]]) as i32;
              let b = i16::from_le_bytes([self.samples[i * 4 + 2], self.samples[i * 4 + 3]]) as i32;
              let out = ((a + b) / 2) as i16;
              let bytes = out.to_le_bytes();
              w[i * 2] = bytes[0];
              w[i * 2 + 1] = bytes[1];
            }
          }
          _ => return Err(PxtoneError::PcmConvert),
        }
        w
      }
      _ => return Err(PxtoneError::PcmConvert),
    };
    self.samples = work;
    self.ch = new_ch;
    Ok(())
  }

  fn convert_bps(&mut self, new_bps: i32) -> Result<(), PxtoneError> {
    if self.bps == new_bps {
      return Ok(());
    }
    let total = (self.total_samples() * self.ch) as usize;
    let work = match (self.bps, new_bps) {
      // 16 → 8
      (16, 8) => {
        let mut w = vec![0u8; total / 2];
        for i in 0..w.len() {
          let v = i16::from_le_bytes([self.samples[i * 2], self.samples[i * 2 + 1]]);
          w[i] = ((v as i32 / 0x100) + 128) as u8;
        }
        w
      }
      // 8 → 16
      (8, 16) => {
        let mut w = vec![0u8; total * 2];
        for i in 0..total {
          let v = (self.samples[i] as i32 - 128) * 0x100;
          let bytes = (v as i16).to_le_bytes();
          w[i * 2] = bytes[0];
          w[i * 2 + 1] = bytes[1];
        }
        w
      }
      _ => return Err(PxtoneError::PcmConvert),
    };
    self.samples = work;
    self.bps = new_bps;
    Ok(())
  }

  fn convert_sps(&mut self, new_sps: i32) -> Result<(), PxtoneError> {
    if self.sps == new_sps {
      return Ok(());
    }
    let bytes_per_sample = (self.ch * self.bps / 8) as usize;
    let old_total = self.total_samples() as usize;
    let new_head =
      ((self.smp_head as f64 * new_sps as f64 + self.sps as f64 - 1.0) / self.sps as f64) as usize;
    let new_body =
      ((self.smp_body as f64 * new_sps as f64 + self.sps as f64 - 1.0) / self.sps as f64) as usize;
    let new_tail =
      ((self.smp_tail as f64 * new_sps as f64 + self.sps as f64 - 1.0) / self.sps as f64) as usize;
    let new_total = new_head + new_body + new_tail;
    let mut work = vec![0u8; new_total * bytes_per_sample];
    for a in 0..new_total {
      let b = (a as f64 * self.sps as f64 / new_sps as f64) as usize;
      let b = b.min(old_total - 1);
      let src = &self.samples[b * bytes_per_sample..(b + 1) * bytes_per_sample];
      let dst = &mut work[a * bytes_per_sample..(a + 1) * bytes_per_sample];
      dst.copy_from_slice(src);
    }
    self.samples = work;
    self.smp_head = new_head as i32;
    self.smp_body = new_body as i32;
    self.smp_tail = new_tail as i32;
    self.sps = new_sps;
    Ok(())
  }

  pub fn convert_volume(&mut self, v: f32) {
    let total = (self.total_samples() * self.ch) as usize;
    match self.bps {
      8 => {
        for b in &mut self.samples[..total] {
          *b = ((*b as f32 - 128.0) * v + 128.0) as u8;
        }
      }
      16 => {
        for i in 0..total {
          let s = i16::from_le_bytes([self.samples[i * 2], self.samples[i * 2 + 1]]);
          let v = (s as f32 * v) as i16;
          let bytes = v.to_le_bytes();
          self.samples[i * 2] = bytes[0];
          self.samples[i * 2 + 1] = bytes[1];
        }
      }
      _ => {}
    }
  }

  /// For moo synthesis: reads as an interleaved stereo 16-bit sample.
  /// pos is in sample units (not frame units). For ch=2, interleaved as (L, R).
  pub fn get_sample_i16_at(&self, frame: usize, ch: usize) -> i16 {
    let bytes_per_frame = (self.ch * self.bps / 8) as usize;
    let total_frames = (self.smp_head + self.smp_body + self.smp_tail) as usize;
    if frame >= total_frames {
      return 0;
    }
    let offset = frame * bytes_per_frame + ch * (self.bps / 8) as usize;
    match self.bps {
      8 => (self.samples[offset] as i32 * 256 - 32768) as i16,
      16 => i16::from_le_bytes([self.samples[offset], self.samples[offset + 1]]),
      _ => 0,
    }
  }
}
