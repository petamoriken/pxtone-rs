// PCM buffer (pxtnPulse_PCM)
// RIFF WAV loading and channel/bit-depth/sample-rate conversion

use crate::error::PxtoneError;

#[derive(Debug)]
pub(crate) struct Pcm {
  pub(crate) channels: u8,
  pub(crate) sample_rate: u32,
  pub(crate) bits_per_sample: u8,
  pub(crate) head_frames: u32,
  pub(crate) body_frames: u32,
  pub(crate) tail_frames: u32,
  samples: Vec<u8>,
}

impl Pcm {
  pub(crate) fn create(
    channels: u8,
    sample_rate: u32,
    bits_per_sample: u8,
    sample_num: u32,
  ) -> Result<Self, PxtoneError> {
    if bits_per_sample != 8 && bits_per_sample != 16 {
      return Err(PxtoneError::UnknownFormat);
    }
    let size = (sample_num * bits_per_sample as u32 * channels as u32 / 8) as usize;
    let fill = if bits_per_sample == 8 { 128u8 } else { 0u8 };
    Ok(Self {
      channels,
      sample_rate,
      bits_per_sample,
      head_frames: 0,
      body_frames: sample_num,
      tail_frames: 0,
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
    new_sample_rate: u32,
    new_bits_per_sample: u8,
  ) -> Result<(), PxtoneError> {
    self.convert_channel(new_ch)?;
    self.convert_bps(new_bits_per_sample)?;
    self.convert_sps(new_sample_rate)?;
    Ok(())
  }

  fn total_samples(&self) -> u32 {
    self.head_frames + self.body_frames + self.tail_frames
  }

  fn convert_channel(&mut self, new_ch: u8) -> Result<(), PxtoneError> {
    if self.channels == new_ch {
      return Ok(());
    }
    let total = self.total_samples() as usize;
    let work = match (self.channels, new_ch) {
      // mono → stereo
      (1, 2) => {
        let mut w = vec![0u8; total * self.bits_per_sample as usize / 8 * 2];
        match self.bits_per_sample {
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
        let mut w = vec![0u8; total * self.bits_per_sample as usize / 8 / 2];
        match self.bits_per_sample {
          8 => {
            for (i, item) in w.iter_mut().enumerate().take(total / 2) {
              let a = self.samples[i * 2] as i32;
              let b = self.samples[i * 2 + 1] as i32;
              *item = ((a + b) / 2) as u8;
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
    self.channels = new_ch;
    Ok(())
  }

  fn convert_bps(&mut self, new_bits_per_sample: u8) -> Result<(), PxtoneError> {
    if self.bits_per_sample == new_bits_per_sample {
      return Ok(());
    }
    let total = (self.total_samples() * self.channels as u32) as usize;
    let work = match (self.bits_per_sample, new_bits_per_sample) {
      // 16 → 8
      (16, 8) => {
        let mut w = vec![0u8; total / 2];
        for (i, item) in w.iter_mut().enumerate() {
          let v = i16::from_le_bytes([self.samples[i * 2], self.samples[i * 2 + 1]]);
          *item = ((v as i32 / 0x100) + 128) as u8;
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
    self.bits_per_sample = new_bits_per_sample;
    Ok(())
  }

  fn convert_sps(&mut self, new_sample_rate: u32) -> Result<(), PxtoneError> {
    if self.sample_rate == new_sample_rate {
      return Ok(());
    }
    let bytes_per_sample = self.channels as usize * self.bits_per_sample as usize / 8;
    let old_total = self.total_samples() as usize;
    let new_head = ((self.head_frames as f64 * new_sample_rate as f64 + self.sample_rate as f64
      - 1.0)
      / self.sample_rate as f64) as usize;
    let new_body = ((self.body_frames as f64 * new_sample_rate as f64 + self.sample_rate as f64
      - 1.0)
      / self.sample_rate as f64) as usize;
    let new_tail = ((self.tail_frames as f64 * new_sample_rate as f64 + self.sample_rate as f64
      - 1.0)
      / self.sample_rate as f64) as usize;
    let new_total = new_head + new_body + new_tail;
    let mut work = vec![0u8; new_total * bytes_per_sample];
    for a in 0..new_total {
      let b = (a as f64 * self.sample_rate as f64 / new_sample_rate as f64) as usize;
      let b = b.min(old_total - 1);
      let src = &self.samples[b * bytes_per_sample..(b + 1) * bytes_per_sample];
      let dst = &mut work[a * bytes_per_sample..(a + 1) * bytes_per_sample];
      dst.copy_from_slice(src);
    }
    self.samples = work;
    self.head_frames = new_head as u32;
    self.body_frames = new_body as u32;
    self.tail_frames = new_tail as u32;
    self.sample_rate = new_sample_rate;
    Ok(())
  }
}
