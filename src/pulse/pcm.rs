// PCM buffer (pxtnPulse_PCM)
// RIFF WAV loading and channel/bit-depth/sample-rate conversion

use crate::error::PxtoneError;

#[derive(Debug)]
pub struct Pcm {
  pub(crate) ch: u8,
  pub(crate) sps: u32,
  pub(crate) bps: u8,
  pub(crate) smp_head: u32,
  pub(crate) smp_body: u32,
  pub(crate) smp_tail: u32,
  samples: Vec<u8>,
}

impl Pcm {
  pub(crate) fn create(ch: u8, sps: u32, bps: u8, sample_num: u32) -> Result<Self, PxtoneError> {
    if bps != 8 && bps != 16 {
      return Err(PxtoneError::UnknownFormat);
    }
    let size = (sample_num * bps as u32 * ch as u32 / 8) as usize;
    let fill = if bps == 8 { 128u8 } else { 0u8 };
    Ok(Self {
      ch,
      sps,
      bps,
      smp_head: 0,
      smp_body: sample_num,
      smp_tail: 0,
      samples: vec![fill; size],
    })
  }

  pub(crate) fn samples(&self) -> &[u8] {
    &self.samples
  }
  pub(crate) fn samples_mut(&mut self) -> &mut [u8] {
    &mut self.samples
  }

  // ---- Conversion ----

  pub(crate) fn convert(
    &mut self,
    new_ch: u8,
    new_sps: u32,
    new_bps: u8,
  ) -> Result<(), PxtoneError> {
    self.convert_channel(new_ch)?;
    self.convert_bps(new_bps)?;
    self.convert_sps(new_sps)?;
    Ok(())
  }

  fn total_samples(&self) -> u32 {
    self.smp_head + self.smp_body + self.smp_tail
  }

  fn convert_channel(&mut self, new_ch: u8) -> Result<(), PxtoneError> {
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

  fn convert_bps(&mut self, new_bps: u8) -> Result<(), PxtoneError> {
    if self.bps == new_bps {
      return Ok(());
    }
    let total = (self.total_samples() * self.ch as u32) as usize;
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

  fn convert_sps(&mut self, new_sps: u32) -> Result<(), PxtoneError> {
    if self.sps == new_sps {
      return Ok(());
    }
    let bytes_per_sample = (self.ch as usize * self.bps as usize / 8) as usize;
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
    self.smp_head = new_head as u32;
    self.smp_body = new_body as u32;
    self.smp_tail = new_tail as u32;
    self.sps = new_sps;
    Ok(())
  }
}
