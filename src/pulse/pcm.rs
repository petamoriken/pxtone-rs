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
    sample_count: u32,
  ) -> Result<Self, PxtoneError> {
    if bits_per_sample != 8 && bits_per_sample != 16 {
      return Err(PxtoneError::UnknownFormat);
    }
    let size = (sample_count * bits_per_sample as u32 * channels as u32 / 8) as usize;
    let fill = if bits_per_sample == 8 { 128u8 } else { 0u8 };
    Ok(Self {
      channels,
      sample_rate,
      bits_per_sample,
      head_frames: 0,
      body_frames: sample_count,
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
            #[cfg(target_feature = "simd128")]
            // SAFETY: wasm32 linear memory allows unaligned SIMD access
            unsafe {
              simd::mono16_to_stereo(&self.samples, &mut w, total);
            }
            #[cfg(not(target_feature = "simd128"))]
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
            #[cfg(target_feature = "simd128")]
            // SAFETY: wasm32 linear memory allows unaligned SIMD access
            unsafe {
              simd::stereo16_to_mono(&self.samples, &mut w, total / 2);
            }
            #[cfg(not(target_feature = "simd128"))]
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
        #[cfg(target_feature = "simd128")]
        // SAFETY: wasm32 linear memory allows unaligned SIMD access
        unsafe {
          simd::bits8_to_16(&self.samples, &mut w, total);
        }
        #[cfg(not(target_feature = "simd128"))]
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

// ---- SIMD-accelerated conversions (wasm32 simd128) ----

#[cfg(target_feature = "simd128")]
mod simd {
  use std::arch::wasm32::*;

  // mono 16bit → stereo 16bit: duplicate each i16 sample into both channels.
  // Processes 4 samples per iteration using i16x8_shuffle.
  pub(super) unsafe fn mono16_to_stereo(src: &[u8], dst: &mut [u8], n: usize) {
    let mut i = 0;
    while i + 4 <= n {
      // Load 8 bytes (4 i16 mono samples), zero-extend upper 8 bytes
      let input = v128_load64_zero(src.as_ptr().add(i * 2) as *const u64);
      // Duplicate each lane: [s0,s1,s2,s3,_,_,_,_] → [s0,s0,s1,s1,s2,s2,s3,s3]
      let result = i16x8_shuffle::<0, 0, 1, 1, 2, 2, 3, 3>(input, input);
      v128_store(dst.as_mut_ptr().add(i * 4) as *mut v128, result);
      i += 4;
    }
    // Scalar tail
    while i < n {
      let s = &src[i * 2..i * 2 + 2];
      dst[i * 4] = s[0];
      dst[i * 4 + 1] = s[1];
      dst[i * 4 + 2] = s[0];
      dst[i * 4 + 3] = s[1];
      i += 1;
    }
  }

  // stereo 16bit → mono 16bit: average each L/R pair.
  // Matches scalar `(a + b) / 2` (truncation toward zero) for all values.
  // Processes 4 stereo pairs per iteration.
  pub(super) unsafe fn stereo16_to_mono(src: &[u8], dst: &mut [u8], n: usize) {
    let mut i = 0;
    while i + 4 <= n {
      // Load 16 bytes = 4 stereo pairs as [L0,R0,L1,R1,L2,R2,L3,R3] in i16x8
      let input = v128_load(src.as_ptr().add(i * 4) as *const v128);
      // Pairwise signed add → i32x4: [(L0+R0),(L1+R1),(L2+R2),(L3+R3)]
      let sums = i32x4_extadd_pairwise_i16x8(input);
      // Truncation-toward-zero division by 2:
      //   For negative odd sums, arithmetic right shift rounds toward -∞
      //   but integer division rounds toward 0.  Correction: add 1 to negative sums.
      let sign = i32x4_shr(sums, 31); // 0xFFFFFFFF for negative, 0 for non-negative
      let correction = v128_and(sign, i32x4_splat(1));
      let avgs = i32x4_shr(i32x4_add(sums, correction), 1);
      // Narrow i32x4 → i16: lower 4 lanes from avgs, upper 4 discarded
      let result = i16x8_narrow_i32x4(avgs, avgs);
      // Write lower 8 bytes (4 i16 mono samples)
      (dst.as_mut_ptr().add(i * 2) as *mut i64).write_unaligned(i64x2_extract_lane::<0>(result));
      i += 4;
    }
    // Scalar tail
    while i < n {
      let a = i16::from_le_bytes([src[i * 4], src[i * 4 + 1]]) as i32;
      let b = i16::from_le_bytes([src[i * 4 + 2], src[i * 4 + 3]]) as i32;
      let out = ((a + b) / 2) as i16;
      let bytes = out.to_le_bytes();
      dst[i * 2] = bytes[0];
      dst[i * 2 + 1] = bytes[1];
      i += 1;
    }
  }

  // 8bit → 16bit: `(v - 128) * 256`.  Exact: multiplication is lossless.
  // Processes 8 samples per iteration.
  pub(super) unsafe fn bits8_to_16(src: &[u8], dst: &mut [u8], n: usize) {
    let mut i = 0;
    while i + 8 <= n {
      // Load 8 u8s into lower 8 lanes, upper 8 lanes = 0
      let input = v128_load64_zero(src.as_ptr().add(i) as *const u64);
      // Zero-extend u8×8 → i16×8: [u0..u7]
      let extended = i16x8_extend_low_u8x16(input);
      // Subtract 128: [-128..127]
      let centered = i16x8_sub(extended, i16x8_splat(128));
      // Shift left 8 (× 256): [-32768..32512]
      let result = i16x8_shl(centered, 8);
      v128_store(dst.as_mut_ptr().add(i * 2) as *mut v128, result);
      i += 8;
    }
    // Scalar tail
    while i < n {
      let v = (src[i] as i32 - 128) * 0x100;
      let bytes = (v as i16).to_le_bytes();
      dst[i * 2] = bytes[0];
      dst[i * 2 + 1] = bytes[1];
      i += 1;
    }
  }
}
