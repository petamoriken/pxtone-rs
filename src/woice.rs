use crate::error::PxtoneError;
use crate::event::EVENT_DEFAULT_BASIC_KEY;
use crate::pulse::frequency::FrequencyTable;
use crate::pulse::noise::Noise;
use crate::pulse::noise_builder::NoiseBuilder;
use crate::pulse::oscillator::{Oscillator, Point};
use crate::pulse::pcm::Pcm;
use crate::read_ext::ReadExt;
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek};

// ---- Constants ----
pub(crate) const BUFSIZE_TIMEPAN: usize = 0x40;

pub(crate) const VOICE_FLAG_WAVELOOP: u32 = 0x00000001;
pub(crate) const VOICE_FLAG_SMOOTH: u32 = 0x00000002;
pub(crate) const VOICE_FLAG_BEATFIT: u32 = 0x00000004;
pub(crate) const VOICE_FLAG_UNCOVERED: u32 = 0xfffffff8;

const DATA_FLAG_WAVE: u32 = 0x00000001;
const DATA_FLAG_ENVELOPE: u32 = 0x00000002;
const DATA_FLAG_UNCOVERED: u32 = 0xfffffffc;

// ---- Waveform / Envelope ----

/// Oscillator waveform shape, used by `Coordinate` and `Overtone` voice types.
#[derive(Clone, Debug, Default)]
pub(crate) struct VoiceWave {
  /// Number of sample frames in one oscillation cycle (wave resolution).
  pub(crate) resolution: u32,
  /// Waveform points. In `Coordinate` mode each entry is `(position 0..reso, amplitude -128..127)`;
  /// in `Overtone` mode each entry is `(harmonic number, amplitude)`.
  pub(crate) points: Vec<(i32, i32)>,
}

/// Amplitude envelope definition attached to a voice layer.
#[derive(Clone, Debug, Default)]
pub(crate) struct VoiceEnvelope {
  /// Frames per second of the envelope timeline.
  pub(crate) fps: u32,
  /// Number of points in the attack (head) phase.
  pub(crate) head_num: u32,
  /// Number of points in the sustain (body) phase. Always `0` in the current format.
  pub(crate) body_num: u32,
  /// Number of points in the release (tail) phase. Always `1` in the current format.
  pub(crate) tail_num: u32,
  /// Envelope points ordered head → body → tail.
  /// Each entry is `(duration in frames, volume 0–100)`.
  pub(crate) points: Vec<(i32, i32)>,
}

// ---- Voice data ----

/// Type-specific synthesis data for one voice layer.
pub(crate) enum VoiceData {
  /// Coordinate-based oscillator waveform synthesis.
  Coordinate {
    wave: VoiceWave,
    envelope: VoiceEnvelope,
  },
  /// Overtone-based oscillator waveform synthesis.
  Overtone {
    wave: VoiceWave,
    envelope: VoiceEnvelope,
  },
  /// Raw PCM sample playback.
  Sampling(Pcm),
  /// PTN noise generator.
  Noise(Noise),
  /// OGG Vorbis stream.
  OggVorbis(OggData),
}

impl Default for VoiceData {
  fn default() -> Self {
    VoiceData::Coordinate {
      wave: VoiceWave::default(),
      envelope: VoiceEnvelope::default(),
    }
  }
}

// ---- Unit (design data) ----

/// Design parameters for one voice layer within an instrument.
pub(crate) struct VoiceUnit {
  /// Base pitch in raw pxtone key units. Event KEY values are relative to this.
  /// The default `EVENT_DEFAULT_BASIC_KEY` (`0x4500`) is the reference concert pitch.
  pub(crate) basic_key: u32,
  /// Base volume (0–128).
  pub(crate) volume: u32,
  /// Stereo pan position: `0` = full left, `64` = center, `128` = full right.
  pub(crate) pan: u32,
  /// Pitch fine-tuning multiplier applied on top of `basic_key`.
  pub(crate) tuning: f32,
  /// Combination of `VOICE_FLAG_*` constants (loop, smooth, beat-fit, etc.).
  pub(crate) voice_flags: u32,
  /// Synthesis algorithm and its associated data.
  pub(crate) data: VoiceData,
}

impl Default for VoiceUnit {
  fn default() -> Self {
    Self {
      basic_key: EVENT_DEFAULT_BASIC_KEY,
      volume: 128,
      pan: 64,
      tuning: 1.0,
      voice_flags: VOICE_FLAG_SMOOTH,
      data: VoiceData::default(),
    }
  }
}

// ---- OGG data ----

/// Raw OGG Vorbis data bundled with its decoded stream metadata.
pub(crate) struct OggData {
  /// Number of audio channels (`1` = mono, `2` = stereo).
  pub(crate) channels: u8,
  /// Sample rate in Hz.
  pub(crate) sample_rate: u32,
  /// Total number of sample frames in the decoded stream.
  pub(crate) frame_count: u32,
  /// Raw OGG Vorbis bitstream bytes.
  pub(crate) data: Vec<u8>,
}

// ---- Instance (synthesis buffer) ----

/// Pre-rendered synthesis buffer for one voice layer, rebuilt at playback start.
/// All sample counts and the `samples` buffer are normalized to 44100 Hz stereo 16-bit PCM.
#[derive(Default)]
pub(crate) struct VoiceInstance {
  /// Sample frames in the attack (pre-loop) section.
  pub(crate) head_frames: u32,
  /// Sample frames in the loop body.
  pub(crate) body_frames: u32,
  /// Sample frames in the release (post-loop) section.
  pub(crate) tail_frames: u32,
  /// Rendered waveform as stereo 16-bit little-endian interleaved PCM at 44100 Hz.
  pub(crate) samples: Vec<u8>,
  /// Amplitude envelope table; one byte per frame, range 0–128.
  pub(crate) envelope: Vec<u8>,
  /// Number of valid frames in `envelope`.
  pub(crate) envelope_size: u32,
  /// Release duration in sample frames.
  pub(crate) envelope_release: u32,
  /// Set to `true` if the waveform clipped during synthesis.
  pub(crate) clipped: bool,
}

impl VoiceInstance {
  /// Gets one sample from an interleaved stereo 16-bit buffer
  #[inline]
  pub(crate) fn get_sample_i16(&self, frame: usize, ch: usize) -> i16 {
    let offset = frame * 4 + ch * 2;
    self
      .samples
      .get(offset..offset + 2)
      .map(|b| i16::from_le_bytes([b[0], b[1]]))
      .unwrap_or(0)
  }
}

// ---- Woice ----

/// An instrument in a pxtone song, comprising one or more voice layers.
pub(crate) struct Woice {
  /// Instrument name.
  pub(crate) name: String,
  /// Extra pitch tuning from legacy x3x-format data. `0.0` means no additional tuning.
  pub(crate) x3x_tuning: f32,
  /// Base pitch key from legacy x3x-format data.
  pub(crate) x3x_basic_key: u32,
  /// Voice layer design data (one entry per layer).
  pub(crate) voices: Vec<VoiceUnit>,
  /// Pre-rendered synthesis buffers, one per voice layer. Rebuilt by `tones_ready`.
  pub(crate) instances: Vec<VoiceInstance>,
}

impl Default for Woice {
  fn default() -> Self {
    Self {
      name: String::new(),
      x3x_tuning: 0.0,
      x3x_basic_key: 0,
      voices: Vec::new(),
      instances: Vec::new(),
    }
  }
}

impl Woice {
  pub(crate) fn new() -> Self {
    Self::default()
  }

  // ---- PCM material loading ----
  pub(crate) fn read_mate_pcm<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    // _MATERIALSTRUCT_PCM (24 bytes)
    let _x3x_unit_no = r.read_u16::<LE>()?;
    let basic_key = r.read_u16::<LE>()? as u32;
    let voice_flags = r.read_u32::<LE>()?;
    let channels = r.read_u16::<LE>()? as u8;
    let bits_per_sample = r.read_u16::<LE>()? as u8;
    let sample_rate = r.read_u32::<LE>()?;
    let tuning = r.read_f32::<LE>()?;
    let data_size = r.read_u32::<LE>()?;

    if voice_flags & VOICE_FLAG_UNCOVERED != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    let sample_num = data_size * 8 / bits_per_sample as u32 / channels as u32;
    let mut pcm = Pcm::create(channels, sample_rate, bits_per_sample, sample_num)?;
    r.read_exact(&mut pcm.samples_mut()[..data_size as usize])?;

    let unit = VoiceUnit {
      basic_key,
      tuning,
      voice_flags,
      data: VoiceData::Sampling(pcm),
      ..Default::default()
    };
    self.x3x_basic_key = basic_key;
    self.x3x_tuning = 0.0;
    self.voices = vec![unit];
    Ok(())
  }

  // ---- PTN material loading ----
  pub(crate) fn read_mate_ptn<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    // _MATERIALSTRUCT_PTN (16 bytes)
    let _x3x_unit_no = r.read_u16::<LE>()?;
    let basic_key = r.read_u16::<LE>()? as u32;
    let voice_flags = r.read_u32::<LE>()?;
    let tuning = r.read_f32::<LE>()?;
    let rrr = r.read_i32::<LE>()?;

    if !(0..=1).contains(&rrr) {
      return Err(PxtoneError::UnknownFormat);
    }

    let mut noise = Noise::new();
    noise.read(r)?;

    let unit = VoiceUnit {
      basic_key,
      tuning,
      voice_flags,
      data: VoiceData::Noise(noise),
      ..Default::default()
    };
    self.x3x_basic_key = basic_key;
    self.x3x_tuning = 0.0;
    self.voices = vec![unit];
    Ok(())
  }

  // ---- PTV material loading ----
  pub(crate) fn read_mate_ptv<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    // _MATERIALSTRUCT_PTV (12 bytes)
    let _x3x_unit_no = r.read_u16::<LE>()?;
    let rrr = r.read_u16::<LE>()?;
    let x3x_tuning = r.read_f32::<LE>()?;
    let _ptv_size = r.read_i32::<LE>()?;

    if rrr != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    self.x3x_tuning = if x3x_tuning != 1.0 { x3x_tuning } else { 0.0 };
    self.ptv_read(r)?;
    Ok(())
  }

  // ---- OGGV material loading ----
  pub(crate) fn read_mate_oggv<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    // _MATERIALSTRUCT_OGGV (12 bytes)
    let _xxx = r.read_u16::<LE>()?;
    let basic_key = r.read_u16::<LE>()? as u32;
    let voice_flags = r.read_u32::<LE>()?;
    let tuning = r.read_f32::<LE>()?;

    if voice_flags & VOICE_FLAG_UNCOVERED != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    let channels = r.read_u32::<LE>()? as u8;
    let sample_rate = r.read_u32::<LE>()?;
    let frame_count = r.read_u32::<LE>()?;
    let size = r.read_i32::<LE>()?;
    let mut data = vec![0u8; size as usize];
    r.read_exact(&mut data)?;

    let unit = VoiceUnit {
      basic_key,
      tuning,
      voice_flags,
      data: VoiceData::OggVorbis(OggData {
        channels,
        sample_rate,
        frame_count,
        data,
      }),
      ..Default::default()
    };
    self.x3x_basic_key = basic_key;
    self.x3x_tuning = 0.0;
    self.voices = vec![unit];
    Ok(())
  }

  // ---- PTV loading (PTVOICE- format) ----
  fn ptv_read<R: Read>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let mut code = [0u8; 8];
    r.read_exact(&mut code)?;
    if &code != b"PTVOICE-" {
      return Err(PxtoneError::InvalidCode);
    }

    let version = r.read_i32::<LE>()?;
    let _total = r.read_i32::<LE>()?;
    if version > 20060111 {
      return Err(PxtoneError::NewFormat);
    }

    let x3x_basic_key = r.read_var_u32()?;
    let work1 = r.read_var_u32()?;
    let work2 = r.read_var_u32()?;
    if work1 != 0 || work2 != 0 {
      return Err(PxtoneError::UnknownFormat);
    }
    let voice_num = r.read_var_u32()?;

    self.x3x_basic_key = x3x_basic_key;
    self.voices.clear();

    for _ in 0..voice_num {
      let basic_key = r.read_var_u32()?;
      let volume = r.read_var_u32()?;
      let pan = r.read_var_u32()?;
      let tuning = f32::from_bits(r.read_var_u32()?);
      let voice_flags = r.read_var_u32()?;
      let data_flags = r.read_var_u32()?;

      if voice_flags & VOICE_FLAG_UNCOVERED != 0 {
        return Err(PxtoneError::UnknownFormat);
      }
      if data_flags & DATA_FLAG_UNCOVERED != 0 {
        return Err(PxtoneError::UnknownFormat);
      }

      let mut data = if data_flags & DATA_FLAG_WAVE != 0 {
        ptv_read_wave(r)?
      } else {
        VoiceData::default()
      };

      if data_flags & DATA_FLAG_ENVELOPE != 0 {
        match &mut data {
          VoiceData::Coordinate { envelope, .. } | VoiceData::Overtone { envelope, .. } => {
            ptv_read_envelope(r, envelope)?;
          }
          _ => {}
        }
      }

      self.voices.push(VoiceUnit {
        basic_key,
        volume,
        pan,
        tuning,
        voice_flags,
        data,
      });
    }
    Ok(())
  }

  // ---- Sample buffer preparation ----
  pub(crate) fn tone_ready_sample(
    &mut self,
    noise_builder: &mut NoiseBuilder,
    frequency: &FrequencyTable,
  ) -> Result<(), PxtoneError> {
    let channels = 2u8;
    let sample_rate = 44100u32;
    let bits_per_sample = 16u8;

    self.instances.clear();
    for unit in &mut self.voices {
      let mut instance = VoiceInstance::default();
      let pan = unit.pan;
      let volume = unit.volume;
      match &mut unit.data {
        VoiceData::Sampling(pcm) => {
          let mut work = Pcm::create(
            pcm.channels,
            pcm.sample_rate,
            pcm.bits_per_sample,
            pcm.body_frames,
          )?;
          let src = pcm.samples();
          let copy_len = src.len().min(work.samples().len());
          work.samples_mut()[..copy_len].copy_from_slice(&src[..copy_len]);
          work.convert(channels, sample_rate, bits_per_sample)?;
          instance.head_frames = work.head_frames;
          instance.body_frames = work.body_frames;
          instance.tail_frames = work.tail_frames;
          instance.samples = work.samples().to_vec();
        }
        VoiceData::Coordinate { wave, .. } => {
          let body_frames = 400u32;
          let size = (body_frames * channels as u32 * bits_per_sample as u32 / 8) as usize;
          instance.body_frames = body_frames;
          instance.samples = vec![0u8; size];
          update_wave_ptv(
            pan,
            volume,
            false,
            wave,
            &mut instance,
            channels,
            bits_per_sample,
          );
        }
        VoiceData::Overtone { wave, .. } => {
          let body_frames = 400u32;
          let size = (body_frames * channels as u32 * bits_per_sample as u32 / 8) as usize;
          instance.body_frames = body_frames;
          instance.samples = vec![0u8; size];
          update_wave_ptv(
            pan,
            volume,
            true,
            wave,
            &mut instance,
            channels,
            bits_per_sample,
          );
        }
        VoiceData::Noise(noise) => {
          let body_frames = noise.frame_count_44k;
          let pcm =
            noise_builder.build_noise(noise, channels, sample_rate, bits_per_sample, frequency)?;
          instance.body_frames = body_frames;
          instance.samples = pcm.samples().to_vec();
        }
        VoiceData::OggVorbis(ogg) => {
          let decoded = decode_ogg(&ogg.data)?;
          let mut work = Pcm::create(ogg.channels, ogg.sample_rate, 16, ogg.frame_count)?;
          let src = &decoded[..decoded.len().min(work.samples().len())];
          work.samples_mut()[..src.len()].copy_from_slice(src);
          let _ = work.convert(channels, sample_rate, bits_per_sample);
          instance.head_frames = work.head_frames;
          instance.body_frames = work.body_frames;
          instance.tail_frames = work.tail_frames;
          instance.samples = work.samples().to_vec();
        }
      }
      self.instances.push(instance);
    }
    Ok(())
  }

  // ---- Envelope buffer preparation ----
  pub(crate) fn tone_ready_envelope(&mut self, sample_rate: u32) -> Result<(), PxtoneError> {
    for (unit, inst) in self.voices.iter().zip(self.instances.iter_mut()) {
      inst.envelope.clear();
      inst.envelope_size = 0;
      inst.envelope_release = 0;

      let env = match &unit.data {
        VoiceData::Coordinate { envelope, .. } | VoiceData::Overtone { envelope, .. } => envelope,
        _ => continue,
      };

      if env.head_num > 0 {
        let size: i32 = env.points[..env.head_num as usize]
          .iter()
          .map(|p| p.0)
          .sum();
        inst.envelope_size = (size as f64 * sample_rate as f64 / env.fps as f64) as u32;
        if inst.envelope_size == 0 {
          inst.envelope_size = 1;
        }

        inst.envelope = vec![0u8; inst.envelope_size as usize];

        // Convert points to sample_rate scale
        let pts: Vec<(i32, i32)> = env.points[..env.head_num as usize]
          .iter()
          .enumerate()
          .filter(|&(e, p)| e == 0 || p.0 != 0 || p.1 != 0)
          .scan(0i32, |offset, (_, p)| {
            *offset += (p.0 as f64 * sample_rate as f64 / env.fps as f64) as i32;
            Some((*offset, p.1))
          })
          .collect();

        // Fill the envelope table with linear interpolation
        let mut e = 0usize;
        let mut start = (0i32, 0i32);
        for s in 0..inst.envelope_size as usize {
          while e < pts.len() && s as i32 >= pts[e].0 {
            start = pts[e];
            e += 1;
          }
          inst.envelope[s] = if e < pts.len() {
            let dx = pts[e].0 - start.0;
            let dy = pts[e].1 - start.1;
            let x = s as i32 - start.0;
            (start.1 + if dx > 0 { dy * x / dx } else { 0 }) as u8
          } else {
            start.1 as u8
          };
        }
      }

      if env.tail_num > 0 {
        let tail_idx = env.head_num as usize;
        inst.envelope_release =
          (env.points[tail_idx].0 as f64 * sample_rate as f64 / env.fps as f64) as u32;
      }
    }
    Ok(())
  }

  pub(crate) fn tone_ready(
    &mut self,
    noise_builder: &mut NoiseBuilder,
    frequency: &FrequencyTable,
    sample_rate: u32,
  ) -> Result<(), PxtoneError> {
    self.tone_ready_sample(noise_builder, frequency)?;
    self.tone_ready_envelope(sample_rate)?;
    Ok(())
  }
}

// ---- PTV wave-reading helpers ----
fn ptv_read_wave<R: Read>(r: &mut R) -> Result<VoiceData, PxtoneError> {
  let vtype = r.read_var_u32()?;
  match vtype {
    0 => {
      let num = r.read_var_u32()?;
      let reso = r.read_var_u32()?;
      let mut wave = VoiceWave {
        resolution: reso,
        points: Vec::new(),
      };
      for _ in 0..num {
        let x = r.read_u8()? as i32;
        let y = r.read_i8()? as i32;
        wave.points.push((x, y));
      }
      Ok(VoiceData::Coordinate {
        wave,
        envelope: VoiceEnvelope::default(),
      })
    }
    1 => {
      let num = r.read_var_u32()?;
      let mut wave = VoiceWave::default();
      for _ in 0..num {
        let x = r.read_var_i32()?;
        let y = r.read_var_i32()?;
        wave.points.push((x, y));
      }
      Ok(VoiceData::Overtone {
        wave,
        envelope: VoiceEnvelope::default(),
      })
    }
    _ => Err(PxtoneError::Unsupported("voice type")),
  }
}

fn ptv_read_envelope<R: Read>(r: &mut R, envelope: &mut VoiceEnvelope) -> Result<(), PxtoneError> {
  let fps = r.read_var_u32()?;
  let head_num = r.read_var_u32()?;
  let body_num = r.read_var_u32()?;
  let tail_num = r.read_var_u32()?;
  if body_num != 0 {
    return Err(PxtoneError::UnknownFormat);
  }
  if tail_num != 1 {
    return Err(PxtoneError::UnknownFormat);
  }

  envelope.fps = fps;
  envelope.head_num = head_num;
  envelope.body_num = body_num;
  envelope.tail_num = tail_num;

  let total = head_num + body_num + tail_num;
  for _ in 0..total {
    let x = r.read_var_i32()?;
    let y = r.read_var_i32()?;
    envelope.points.push((x, y));
  }
  Ok(())
}

// ---- PTV wave buffer update ----
fn update_wave_ptv(
  pan: u32,
  volume: u32,
  is_overtone: bool,
  wave: &VoiceWave,
  instance: &mut VoiceInstance,
  channels: u8,
  bits_per_sample: u8,
) {
  let pan_vol: [i32; 2] = match (channels, pan) {
    (2, p) if p > 64 => [128 - p as i32, 64],
    (2, p) if p < 64 => [64, p as i32],
    _ => [64, 64],
  };

  let pts: Vec<Point> = wave.points.iter().map(|&(x, y)| Point { x, y }).collect();
  let mut osci = Oscillator::new();
  osci.ready_get_sample(pts, volume, instance.body_frames, wave.resolution);

  instance.clipped = false;
  for s in 0..instance.body_frames {
    let osc = if is_overtone {
      osci.get_one_sample_overtone(s)
    } else {
      osci.get_one_sample_coordinate(s)
    };
    for (c, &pv) in pan_vol.iter().enumerate().take(channels as usize) {
      let raw = osc * pv as f64 / 64.0;
      if raw.abs() > 1.0 {
        instance.clipped = true;
      }
      let work = raw.clamp(-1.0, 1.0);
      if bits_per_sample == 8 {
        instance.samples[s as usize * channels as usize + c] = ((work * 127.0) as i32 + 128) as u8;
      } else {
        let bytes = ((work * 32767.0) as i16).to_le_bytes();
        instance.samples[(s as usize * channels as usize + c) * 2..][..2].copy_from_slice(&bytes);
      }
    }
  }
}

// ---- OGG Vorbis decode (lewton) ----
fn decode_ogg(data: &[u8]) -> Result<Vec<u8>, PxtoneError> {
  use lewton::inside_ogg::OggStreamReader;
  use std::io::Cursor;

  let cursor = Cursor::new(data);
  let mut reader = OggStreamReader::new(cursor).map_err(|e| PxtoneError::OggVorbis(e))?;

  let _ch = reader.ident_hdr.audio_channels as usize;
  let _sps = reader.ident_hdr.audio_sample_rate;

  let mut pcm_i16: Vec<i16> = Vec::new();
  while let Some(pck) = reader
    .read_dec_packet_itl()
    .map_err(|e| PxtoneError::OggVorbis(e))?
  {
    pcm_i16.extend_from_slice(&pck);
  }

  // i16 → u8 LE byte sequence
  let out: Vec<u8> = pcm_i16.iter().flat_map(|&s| s.to_le_bytes()).collect();
  Ok(out)
}
