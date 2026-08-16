//! Helpers shared by the snapshot based integration tests.

pub const WAV_HEADER_LEN: usize = 44;
pub const WAV_PCM_TOLERANCE: i32 = 2;

pub fn wav_matches(actual: &[u8], expected: &[u8]) -> bool {
  if actual.len() != expected.len() {
    return false;
  }
  if actual[..WAV_HEADER_LEN] != expected[..WAV_HEADER_LEN] {
    return false;
  }
  actual[WAV_HEADER_LEN..]
    .chunks_exact(2)
    .zip(expected[WAV_HEADER_LEN..].chunks_exact(2))
    .all(|(a, e)| {
      let av = i16::from_le_bytes([a[0], a[1]]) as i32;
      let ev = i16::from_le_bytes([e[0], e[1]]) as i32;
      (av - ev).abs() <= WAV_PCM_TOLERANCE
    })
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
