use crate::error::PxtoneError;
use crate::event::EVENTDEFAULT_BASICKEY;
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

pub(crate) const DATA_FLAG_WAVE: u32 = 0x00000001;
pub(crate) const DATA_FLAG_ENVELOPE: u32 = 0x00000002;
pub(crate) const DATA_FLAG_UNCOVERED: u32 = 0xfffffffc;

// ---- Types ----
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum WoiceType {
  #[default]
  None,
  Pcm,
  Ptv,
  Ptn,
  OggVorbis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum VoiceType {
  #[default]
  Coodinate,
  Overtone,
  Noise,
  Sampling,
  OggVorbis,
}

// ---- Waveform / Envelope ----

/// Oscillator waveform shape, used by `Coodinate` and `Overtone` voice types.
#[derive(Clone, Debug, Default)]
pub(crate) struct VoiceWave {
  /// Number of sample frames in one oscillation cycle (wave resolution).
  pub(crate) reso: u32,
  /// Waveform points. In `Coodinate` mode each entry is `(position 0..reso, amplitude -128..127)`;
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

// ---- Unit (design data) ----

/// Design parameters for one voice layer within an instrument.
pub(crate) struct VoiceUnit {
  /// Base pitch in raw pxtone key units. Event KEY values are relative to this.
  /// The default `EVENTDEFAULT_BASICKEY` (`0x4500`) is the reference concert pitch.
  pub(crate) basic_key: u32,
  /// Base volume (0–128).
  pub(crate) volume: u32,
  /// Stereo pan position: `0` = full left, `64` = center, `128` = full right.
  pub(crate) pan: u32,
  /// Pitch fine-tuning multiplier applied on top of `basic_key`.
  pub(crate) tuning: f32,
  /// Combination of `VOICE_FLAG_*` constants (loop, smooth, beat-fit, etc.).
  pub(crate) voice_flags: u32,
  /// Combination of `DATA_FLAG_*` constants indicating which embedded data fields are present.
  pub(crate) data_flags: u32,
  /// Synthesis algorithm used for this voice layer.
  pub(crate) voice_type: VoiceType,
  /// Raw PCM material; present for `Sampling` and `Pcm` voice types.
  pub(crate) pcm: Option<Pcm>,
  /// Noise generator design; present for the `Noise` (PTN) voice type.
  pub(crate) noise: Option<Noise>,
  /// OGG Vorbis bitstream with its decoded metadata; present for the `OggVorbis` voice type.
  pub(crate) ogg_data: Option<OggData>,
  /// Waveform shape; used by `Coodinate` and `Overtone` voice types.
  pub(crate) wave: VoiceWave,
  /// Amplitude envelope applied during playback.
  pub(crate) envelope: VoiceEnvelope,
}

impl Default for VoiceUnit {
  fn default() -> Self {
    Self {
      basic_key: EVENTDEFAULT_BASICKEY,
      volume: 128,
      pan: 64,
      tuning: 1.0,
      voice_flags: VOICE_FLAG_SMOOTH,
      data_flags: DATA_FLAG_WAVE,
      voice_type: VoiceType::Coodinate,
      pcm: None,
      noise: None,
      ogg_data: None,
      wave: VoiceWave::default(),
      envelope: VoiceEnvelope::default(),
    }
  }
}

// ---- OGG data ----

/// Raw OGG Vorbis data bundled with its decoded stream metadata.
pub(crate) struct OggData {
  /// Number of audio channels (`1` = mono, `2` = stereo).
  pub(crate) ch_num: u8,
  /// Sample rate in Hz.
  pub(crate) sps: u32,
  /// Total number of sample frames in the decoded stream.
  pub(crate) smp_num: u32,
  /// Raw OGG Vorbis bitstream bytes.
  pub(crate) data: Vec<u8>,
}

// ---- Instance (synthesis buffer) ----

/// Pre-rendered synthesis buffer for one voice layer, rebuilt at playback start.
/// All sample counts and the `samples_w` buffer are normalized to 44100 Hz stereo 16-bit PCM.
#[derive(Default)]
pub(crate) struct VoiceInstance {
  /// Sample frames in the attack (pre-loop) section.
  pub(crate) smp_head_w: u32,
  /// Sample frames in the loop body.
  pub(crate) smp_body_w: u32,
  /// Sample frames in the release (post-loop) section.
  pub(crate) smp_tail_w: u32,
  /// Rendered waveform as stereo 16-bit little-endian interleaved PCM at 44100 Hz.
  pub(crate) samples_w: Vec<u8>,
  /// Amplitude envelope table; one byte per frame, range 0–128.
  pub(crate) env: Vec<u8>,
  /// Number of valid frames in `env`.
  pub(crate) env_size: u32,
  /// Release duration in sample frames.
  pub(crate) env_release: u32,
  /// Set to `true` if the waveform clipped during synthesis.
  pub(crate) b_sine_over: bool,
}

impl VoiceInstance {
  /// Gets one sample from an interleaved stereo 16-bit buffer
  #[inline]
  pub(crate) fn get_sample_i16(&self, frame: usize, ch: usize) -> i16 {
    let offset = frame * 4 + ch * 2;
    self
      .samples_w
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
  /// Source material type (PCM, PTV, PTN, OGG Vorbis, etc.).
  pub(crate) woice_type: WoiceType,
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
      woice_type: WoiceType::None,
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
    let ch_num = r.read_u16::<LE>()? as u8;
    let bps = r.read_u16::<LE>()? as u8;
    let sps = r.read_u32::<LE>()?;
    let tuning = r.read_f32::<LE>()?;
    let data_size = r.read_u32::<LE>()?;

    if voice_flags & VOICE_FLAG_UNCOVERED != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    let sample_num = data_size * 8 / bps as u32 / ch_num as u32;
    let mut pcm = Pcm::create(ch_num, sps, bps, sample_num)?;
    r.read_exact(&mut pcm.samples_mut()[..data_size as usize])?;

    let unit = VoiceUnit {
      voice_type: VoiceType::Sampling,
      basic_key,
      tuning,
      voice_flags,
      pcm: Some(pcm),
      ..Default::default()
    };
    self.x3x_basic_key = basic_key;
    self.x3x_tuning = 0.0;
    self.voices = vec![unit];
    self.woice_type = WoiceType::Pcm;
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
      voice_type: VoiceType::Noise,
      voice_flags,
      basic_key,
      tuning,
      noise: Some(noise),
      ..Default::default()
    };
    self.x3x_basic_key = basic_key;
    self.x3x_tuning = 0.0;
    self.voices = vec![unit];
    self.woice_type = WoiceType::Ptn;
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

    // pxtn_read: ch, sps, smp_num, size, data
    let ch_num = r.read_u32::<LE>()? as u8;
    let sps = r.read_u32::<LE>()?;
    let smp_num = r.read_u32::<LE>()?;
    let size = r.read_i32::<LE>()?;
    let mut data = vec![0u8; size as usize];
    r.read_exact(&mut data)?;

    let unit = VoiceUnit {
      voice_type: VoiceType::OggVorbis,
      voice_flags,
      basic_key,
      tuning,
      ogg_data: Some(OggData {
        ch_num,
        sps,
        smp_num,
        data,
      }),
      ..Default::default()
    };
    self.x3x_basic_key = basic_key;
    self.x3x_tuning = 0.0;
    self.voices = vec![unit];
    self.woice_type = WoiceType::OggVorbis;
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
      let mut unit = VoiceUnit {
        basic_key: r.read_var_u32()?,
        volume: r.read_var_u32()?,
        pan: r.read_var_u32()?,
        tuning: f32::from_bits(r.read_var_u32()?),
        voice_flags: r.read_var_u32()?,
        data_flags: r.read_var_u32()?,
        ..Default::default()
      };

      if unit.voice_flags & VOICE_FLAG_UNCOVERED != 0 {
        return Err(PxtoneError::UnknownFormat);
      }
      if unit.data_flags & DATA_FLAG_UNCOVERED != 0 {
        return Err(PxtoneError::UnknownFormat);
      }

      if unit.data_flags & DATA_FLAG_WAVE != 0 {
        ptv_read_wave(r, &mut unit)?;
      }
      if unit.data_flags & DATA_FLAG_ENVELOPE != 0 {
        ptv_read_envelope(r, &mut unit)?;
      }

      self.voices.push(unit);
    }
    self.woice_type = WoiceType::Ptv;
    Ok(())
  }

  // ---- Sample buffer preparation ----
  pub(crate) fn tone_ready_sample(
    &mut self,
    noise_builder: &mut NoiseBuilder,
    freq: &FrequencyTable,
  ) -> Result<(), PxtoneError> {
    let ch_num = 2u8;
    let sps = 44100u32;
    let bps = 16u8;

    self.instances.clear();
    for unit in &mut self.voices {
      let mut inst = VoiceInstance::default();
      match unit.voice_type {
        VoiceType::Sampling => {
          if let Some(pcm) = &unit.pcm {
            let mut work = Pcm::create(pcm.ch_num, pcm.sps, pcm.bps, pcm.smp_body)?;
            let src = pcm.samples();
            let copy_len = src.len().min(work.samples().len());
            work.samples_mut()[..copy_len].copy_from_slice(&src[..copy_len]);
            work.convert(ch_num, sps, bps)?;
            inst.smp_head_w = work.smp_head;
            inst.smp_body_w = work.smp_body;
            inst.smp_tail_w = work.smp_tail;
            inst.samples_w = work.samples().to_vec();
          }
        }
        VoiceType::Overtone | VoiceType::Coodinate => {
          let smp_body = 400u32;
          let size = (smp_body * ch_num as u32 * bps as u32 / 8) as usize;
          inst.smp_body_w = smp_body;
          inst.samples_w = vec![0u8; size];
          update_wave_ptv(unit, &mut inst, ch_num, sps, bps);
        }
        VoiceType::Noise => {
          if let Some(noise) = &mut unit.noise {
            let pcm = noise_builder.build_noise(noise, ch_num, sps, bps, freq)?;
            inst.smp_body_w = noise.smp_num_44k;
            inst.samples_w = pcm.samples().to_vec();
          }
        }
        VoiceType::OggVorbis => {
          if let Some(ogg) = &unit.ogg_data {
            let decoded = decode_ogg(&ogg.data).map_err(PxtoneError::OggVorbis)?;
            let ogg_ch_num = ogg.ch_num;
            let ogg_sps = ogg.sps;
            let ogg_smp = ogg.smp_num;
            let mut work = Pcm::create(ogg_ch_num, ogg_sps, 16, ogg_smp)?;
            let src = &decoded[..decoded.len().min(work.samples().len())];
            work.samples_mut()[..src.len()].copy_from_slice(src);
            let _ = work.convert(ch_num, sps, bps);
            inst.smp_head_w = work.smp_head;
            inst.smp_body_w = work.smp_body;
            inst.smp_tail_w = work.smp_tail;
            inst.samples_w = work.samples().to_vec();
          }
        }
      }
      self.instances.push(inst);
    }
    Ok(())
  }

  // ---- Envelope buffer preparation ----
  pub(crate) fn tone_ready_envelope(&mut self, sps: u32) -> Result<(), PxtoneError> {
    for (unit, inst) in self.voices.iter().zip(self.instances.iter_mut()) {
      let env = &unit.envelope;
      inst.env.clear();
      inst.env_size = 0;
      inst.env_release = 0;

      if env.head_num > 0 {
        let size: i32 = env.points[..env.head_num as usize]
          .iter()
          .map(|p| p.0)
          .sum();
        inst.env_size = (size as f64 * sps as f64 / env.fps as f64) as u32;
        if inst.env_size == 0 {
          inst.env_size = 1;
        }

        inst.env = vec![0u8; inst.env_size as usize];

        // Convert points to sps scale
        let pts: Vec<(i32, i32)> = env.points[..env.head_num as usize]
          .iter()
          .enumerate()
          .filter(|&(e, p)| e == 0 || p.0 != 0 || p.1 != 0)
          .scan(0i32, |offset, (_, p)| {
            *offset += (p.0 as f64 * sps as f64 / env.fps as f64) as i32;
            Some((*offset, p.1))
          })
          .collect();

        // Fill the envelope table with linear interpolation
        let mut e = 0usize;
        let mut start = (0i32, 0i32);
        for s in 0..inst.env_size as usize {
          while e < pts.len() && s as i32 >= pts[e].0 {
            start = pts[e];
            e += 1;
          }
          inst.env[s] = if e < pts.len() {
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
        inst.env_release = (env.points[tail_idx].0 as f64 * sps as f64 / env.fps as f64) as u32;
      }
    }
    Ok(())
  }

  pub(crate) fn tone_ready(
    &mut self,
    noise_builder: &mut NoiseBuilder,
    freq: &FrequencyTable,
    sps: u32,
  ) -> Result<(), PxtoneError> {
    self.tone_ready_sample(noise_builder, freq)?;
    self.tone_ready_envelope(sps)?;
    Ok(())
  }
}

// ---- PTV wave-reading helpers ----
fn ptv_read_wave<R: Read>(r: &mut R, unit: &mut VoiceUnit) -> Result<(), PxtoneError> {
  let vtype = r.read_var_u32()?;
  unit.voice_type = match vtype {
    0 => VoiceType::Coodinate,
    1 => VoiceType::Overtone,
    _ => return Err(PxtoneError::Unsupported("voice type")),
  };
  match unit.voice_type {
    VoiceType::Coodinate => {
      let num = r.read_var_u32()?;
      let reso = r.read_var_u32()?;
      unit.wave.reso = reso;
      for _ in 0..num {
        let x = r.read_u8()? as i32;
        let y = r.read_i8()? as i32;
        unit.wave.points.push((x, y));
      }
    }
    VoiceType::Overtone => {
      let num = r.read_var_u32()?;
      for _ in 0..num {
        let x = r.read_var_i32()?;
        let y = r.read_var_i32()?;
        unit.wave.points.push((x, y));
      }
    }
    _ => {}
  }
  Ok(())
}

fn ptv_read_envelope<R: Read>(r: &mut R, unit: &mut VoiceUnit) -> Result<(), PxtoneError> {
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

  unit.envelope.fps = fps;
  unit.envelope.head_num = head_num;
  unit.envelope.body_num = body_num;
  unit.envelope.tail_num = tail_num;

  let total = head_num + body_num + tail_num;
  for _ in 0..total {
    let x = r.read_var_i32()?;
    let y = r.read_var_i32()?;
    unit.envelope.points.push((x, y));
  }
  Ok(())
}

// ---- PTV wave buffer update ----
fn update_wave_ptv(unit: &VoiceUnit, inst: &mut VoiceInstance, ch_num: u8, _sps: u32, bps: u8) {
  let pan_vol: [i32; 2] = match (ch_num, unit.pan) {
    (2, p) if p > 64 => [128 - p as i32, 64],
    (2, p) if p < 64 => [64, p as i32],
    _ => [64, 64],
  };

  let pts: Vec<Point> = unit
    .wave
    .points
    .iter()
    .map(|&(x, y)| Point { x, y })
    .collect();
  let mut osci = Oscillator::new();
  osci.ready_get_sample(pts, unit.volume, inst.smp_body_w, unit.wave.reso);

  let b_ovt = unit.voice_type == VoiceType::Overtone;

  inst.b_sine_over = false;
  for s in 0..inst.smp_body_w {
    let osc = if b_ovt {
      osci.get_one_sample_overtone(s)
    } else {
      osci.get_one_sample_coodinate(s)
    };
    for (c, &pv) in pan_vol.iter().enumerate().take(ch_num as usize) {
      let raw = osc * pv as f64 / 64.0;
      if raw.abs() > 1.0 {
        inst.b_sine_over = true;
      }
      let work = raw.clamp(-1.0, 1.0);
      if bps == 8 {
        inst.samples_w[s as usize * ch_num as usize + c] = ((work * 127.0) as i32 + 128) as u8;
      } else {
        let bytes = ((work * 32767.0) as i16).to_le_bytes();
        inst.samples_w[(s as usize * ch_num as usize + c) * 2..][..2].copy_from_slice(&bytes);
      }
    }
  }
}

// ---- OGG Vorbis decode (lewton) ----
fn decode_ogg(data: &[u8]) -> Result<Vec<u8>, String> {
  use lewton::inside_ogg::OggStreamReader;
  use std::io::Cursor;

  let cursor = Cursor::new(data);
  let mut reader = OggStreamReader::new(cursor).map_err(|e| format!("{e}"))?;

  let _ch = reader.ident_hdr.audio_channels as usize;
  let _sps = reader.ident_hdr.audio_sample_rate;

  let mut pcm_i16: Vec<i16> = Vec::new();
  while let Some(pck) = reader.read_dec_packet_itl().map_err(|e| format!("{e}"))? {
    pcm_i16.extend_from_slice(&pck);
  }

  // i16 → u8 LE byte sequence
  let out: Vec<u8> = pcm_i16.iter().flat_map(|&s| s.to_le_bytes()).collect();
  Ok(out)
}
