//! Helpers shared by the snapshot based integration tests.

pub const WAV_HEADER_LEN: usize = 44;

/// Whether two WAV files hold the same samples.
///
/// Exactly the same: there is nothing platform dependent left in the decode.
/// `lite-math` computes its own trigonometry rather than calling out to a libm
/// that varies, and everything else on the path is integer work or IEEE `f32`
/// and `f64` arithmetic, which is specified to the bit.
pub fn wav_matches(actual: &[u8], expected: &[u8]) -> bool {
  if actual.len() != expected.len() {
    return false;
  }
  if actual[..WAV_HEADER_LEN] != expected[..WAV_HEADER_LEN] {
    return false;
  }
  let (actual_samples, _) = actual[WAV_HEADER_LEN..].as_chunks::<2>();
  let (expected_samples, _) = expected[WAV_HEADER_LEN..].as_chunks::<2>();
  actual_samples
    .iter()
    .zip(expected_samples)
    .all(|(a, e)| a == e)
}

pub fn pcm_to_wav(samples: &[u8], channels: u8, sample_rate: u32) -> Vec<u8> {
  let data_len = samples.len() as u32;
  let byte_rate = sample_rate * channels as u32 * 2;
  let mut wav = Vec::with_capacity(WAV_HEADER_LEN + samples.len());
  wav.extend_from_slice(b"RIFF");
  wav.extend_from_slice(&(36u32 + data_len).to_le_bytes());
  wav.extend_from_slice(b"WAVE");
  wav.extend_from_slice(b"fmt ");
  wav.extend_from_slice(&16u32.to_le_bytes());
  wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
  wav.extend_from_slice(&(channels as u16).to_le_bytes());
  wav.extend_from_slice(&sample_rate.to_le_bytes());
  wav.extend_from_slice(&byte_rate.to_le_bytes());
  wav.extend_from_slice(&(channels as u16 * 2).to_le_bytes());
  wav.extend_from_slice(&16u16.to_le_bytes());
  wav.extend_from_slice(b"data");
  wav.extend_from_slice(&data_len.to_le_bytes());
  wav.extend_from_slice(samples);
  wav
}
