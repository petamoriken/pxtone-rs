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
#[derive(Clone, Debug, Default)]
pub(crate) struct VoiceWave {
  pub(crate) reso: u32,
  pub(crate) points: Vec<(i32, i32)>, // (x, y)
}

#[derive(Clone, Debug, Default)]
pub(crate) struct VoiceEnvelope {
  pub(crate) fps: u32,
  pub(crate) head_num: u32,
  pub(crate) body_num: u32,
  pub(crate) tail_num: u32,
  pub(crate) points: Vec<(i32, i32)>,
}

// ---- Unit (design data) ----
pub(crate) struct VoiceUnit {
  pub(crate) basic_key: i32,
  pub(crate) volume: u32,
  pub(crate) pan: i32,
  pub(crate) tuning: f32,
  pub(crate) voice_flags: u32,
  pub(crate) data_flags: u32,
  pub(crate) voice_type: VoiceType,
  pub(crate) pcm: Option<Pcm>,
  pub(crate) noise: Option<Noise>,
  pub(crate) ogg_data: Option<OggData>,
  pub(crate) wave: VoiceWave,
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
pub(crate) struct OggData {
  pub(crate) ch: u8,
  pub(crate) sps: u32,
  pub(crate) smp_num: u32,
  pub(crate) data: Vec<u8>,
}

// ---- Instance (synthesis buffer) ----
#[derive(Default)]
pub(crate) struct VoiceInstance {
  pub(crate) smp_head_w: u32,
  pub(crate) smp_body_w: u32,
  pub(crate) smp_tail_w: u32,
  pub(crate) samples_w: Vec<u8>, // stereo 16-bit interleaved
  pub(crate) env: Vec<u8>,
  pub(crate) env_size: u32,
  pub(crate) env_release: u32,
  pub(crate) b_sine_over: bool,
}

impl VoiceInstance {
  /// Gets one sample from an interleaved stereo 16-bit buffer
  pub(crate) fn get_sample_i16(&self, frame: usize, ch: usize) -> i16 {
    let offset = frame * 4 + ch * 2;
    if offset + 1 >= self.samples_w.len() {
      return 0;
    }
    i16::from_le_bytes([self.samples_w[offset], self.samples_w[offset + 1]])
  }
}

// ---- Woice ----
pub(crate) struct Woice {
  pub(crate) name: String,
  pub(crate) woice_type: WoiceType,
  pub(crate) x3x_tuning: f32,
  pub(crate) x3x_basic_key: i32,
  pub(crate) voices: Vec<VoiceUnit>,
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
    let basic_key = r.read_u16::<LE>()? as i32;
    let voice_flags = r.read_u32::<LE>()?;
    let ch = r.read_u16::<LE>()? as u8;
    let bps = r.read_u16::<LE>()? as u8;
    let sps = r.read_u32::<LE>()?;
    let tuning = r.read_f32::<LE>()?;
    let data_size = r.read_u32::<LE>()?;

    if voice_flags & VOICE_FLAG_UNCOVERED != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    let sample_num = data_size * 8 / bps as u32 / ch as u32;
    let mut pcm = Pcm::create(ch, sps, bps, sample_num)?;
    r.read_exact(&mut pcm.samples_mut()[..data_size as usize])?;

    let mut unit = VoiceUnit::default();
    unit.voice_type = VoiceType::Sampling;
    unit.basic_key = basic_key;
    unit.tuning = tuning;
    unit.voice_flags = voice_flags;
    unit.pcm = Some(pcm);
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
    let basic_key = r.read_u16::<LE>()? as i32;
    let voice_flags = r.read_u32::<LE>()?;
    let tuning = r.read_f32::<LE>()?;
    let rrr = r.read_i32::<LE>()?;

    if rrr > 1 || rrr < 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    let mut noise = Noise::new();
    noise.read(r)?;

    let mut unit = VoiceUnit::default();
    unit.voice_type = VoiceType::Noise;
    unit.voice_flags = voice_flags;
    unit.basic_key = basic_key;
    unit.tuning = tuning;
    unit.noise = Some(noise);
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
    let basic_key = r.read_u16::<LE>()? as i32;
    let voice_flags = r.read_u32::<LE>()?;
    let tuning = r.read_f32::<LE>()?;

    if voice_flags & VOICE_FLAG_UNCOVERED != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    // pxtn_read: ch, sps, smp_num, size, data
    let ch = r.read_u32::<LE>()? as u8;
    let sps = r.read_u32::<LE>()?;
    let smp_num = r.read_u32::<LE>()?;
    let size = r.read_i32::<LE>()?;
    let mut data = vec![0u8; size as usize];
    r.read_exact(&mut data)?;

    let mut unit = VoiceUnit::default();
    unit.voice_type = VoiceType::OggVorbis;
    unit.voice_flags = voice_flags;
    unit.basic_key = basic_key;
    unit.tuning = tuning;
    unit.ogg_data = Some(OggData {
      ch,
      sps,
      smp_num,
      data,
    });
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

    let x3x_basic_key = r.read_var_int()?;
    let work1 = r.read_var_int()?;
    let work2 = r.read_var_int()?;
    if work1 != 0 || work2 != 0 {
      return Err(PxtoneError::UnknownFormat);
    }
    let voice_num = r.read_var_int()?;

    self.x3x_basic_key = x3x_basic_key;
    self.voices.clear();

    for _ in 0..voice_num {
      let mut unit = VoiceUnit::default();
      unit.basic_key = r.read_var_int()?;
      unit.volume = r.read_var_int()? as u32;
      unit.pan = r.read_var_int()?;
      let tuning_bits = r.read_var_int()? as u32;
      unit.tuning = f32::from_bits(tuning_bits);
      unit.voice_flags = r.read_var_int()? as u32;
      unit.data_flags = r.read_var_int()? as u32;

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
    let ch = 2u8;
    let sps = 44100u32;
    let bps = 16u8;

    self.instances.clear();
    for unit in &mut self.voices {
      let mut inst = VoiceInstance::default();
      match unit.voice_type {
        VoiceType::Sampling => {
          if let Some(pcm) = &unit.pcm {
            let mut work = Pcm::create(pcm.ch, pcm.sps, pcm.bps, pcm.smp_body)?;
            let src = pcm.samples();
            let copy_len = src.len().min(work.samples().len());
            work.samples_mut()[..copy_len].copy_from_slice(&src[..copy_len]);
            work.convert(ch, sps, bps)?;
            inst.smp_head_w = work.smp_head;
            inst.smp_body_w = work.smp_body;
            inst.smp_tail_w = work.smp_tail;
            inst.samples_w = work.samples().to_vec();
          }
        }
        VoiceType::Overtone | VoiceType::Coodinate => {
          let smp_body = 400u32;
          let size = (smp_body * ch as u32 * bps as u32 / 8) as usize;
          inst.smp_body_w = smp_body;
          inst.samples_w = vec![0u8; size];
          update_wave_ptv(unit, &mut inst, ch, sps, bps);
        }
        VoiceType::Noise => {
          if let Some(noise) = &mut unit.noise {
            let pcm = noise_builder.build_noise(noise, ch as usize, sps, bps, freq)?;
            inst.smp_body_w = noise.smp_num_44k;
            inst.samples_w = pcm.samples().to_vec();
          }
        }
        VoiceType::OggVorbis => {
          if let Some(ogg) = &unit.ogg_data {
            let decoded = decode_ogg(&ogg.data).map_err(|e| PxtoneError::OggVorbis(e))?;
            let ogg_ch = ogg.ch;
            let ogg_sps = ogg.sps;
            let ogg_smp = ogg.smp_num;
            let mut work = Pcm::create(ogg_ch, ogg_sps, 16, ogg_smp)?;
            let src = &decoded[..decoded.len().min(work.samples().len())];
            work.samples_mut()[..src.len()].copy_from_slice(src);
            let _ = work.convert(ch, sps, bps);
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
        let mut size = 0i32;
        for e in 0..env.head_num as usize {
          size += env.points[e].0;
        }
        inst.env_size = (size as f64 * sps as f64 / env.fps as f64) as u32;
        if inst.env_size == 0 {
          inst.env_size = 1;
        }

        inst.env = vec![0u8; inst.env_size as usize];

        // Convert points to sps scale
        let mut pts: Vec<(i32, i32)> = Vec::new();
        let mut offset = 0i32;
        for e in 0..env.head_num as usize {
          if e == 0 || env.points[e].0 != 0 || env.points[e].1 != 0 {
            offset += (env.points[e].0 as f64 * sps as f64 / env.fps as f64) as i32;
            pts.push((offset, env.points[e].1));
          }
        }

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
  let vtype = r.read_var_int()?;
  unit.voice_type = match vtype {
    0 => VoiceType::Coodinate,
    1 => VoiceType::Overtone,
    _ => return Err(PxtoneError::Unsupported("voice type")),
  };
  match unit.voice_type {
    VoiceType::Coodinate => {
      let num = r.read_var_int()?;
      let reso = r.read_var_int()?;
      unit.wave.reso = reso as u32;
      for _ in 0..num {
        let x = r.read_u8()? as i32;
        let y = r.read_i8()? as i32;
        unit.wave.points.push((x, y));
      }
    }
    VoiceType::Overtone => {
      let num = r.read_var_int()?;
      for _ in 0..num {
        let x = r.read_var_int()?;
        let y = r.read_var_int()?;
        unit.wave.points.push((x, y));
      }
    }
    _ => {}
  }
  Ok(())
}

fn ptv_read_envelope<R: Read>(r: &mut R, unit: &mut VoiceUnit) -> Result<(), PxtoneError> {
  let fps = r.read_var_int()?;
  let head_num = r.read_var_int()?;
  let body_num = r.read_var_int()?;
  let tail_num = r.read_var_int()?;
  if body_num != 0 {
    return Err(PxtoneError::UnknownFormat);
  }
  if tail_num != 1 {
    return Err(PxtoneError::UnknownFormat);
  }

  unit.envelope.fps = fps as u32;
  unit.envelope.head_num = head_num as u32;
  unit.envelope.body_num = body_num as u32;
  unit.envelope.tail_num = tail_num as u32;

  let total = head_num + body_num + tail_num;
  for _ in 0..total {
    let x = r.read_var_int()?;
    let y = r.read_var_int()?;
    unit.envelope.points.push((x, y));
  }
  Ok(())
}

// ---- PTV wave buffer update ----
fn update_wave_ptv(unit: &VoiceUnit, inst: &mut VoiceInstance, ch: u8, _sps: u32, bps: u8) {
  let pan_vol: [i32; 2] = if ch == 2 {
    if unit.pan > 64 {
      [128 - unit.pan, 64]
    } else if unit.pan < 64 {
      [64, unit.pan]
    } else {
      [64, 64]
    }
  } else {
    [64, 64]
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
  if bps == 8 {
    for s in 0..inst.smp_body_w {
      let osc = if b_ovt {
        osci.get_one_sample_overtone(s)
      } else {
        osci.get_one_sample_coodinate(s)
      };
      for c in 0..ch as usize {
        let mut work = osc * pan_vol[c] as f64 / 64.0;
        if work > 1.0 {
          work = 1.0;
          inst.b_sine_over = true;
        }
        if work < -1.0 {
          work = -1.0;
          inst.b_sine_over = true;
        }
        inst.samples_w[s as usize * ch as usize + c] = ((work * 127.0) as i32 + 128) as u8;
      }
    }
  } else {
    for s in 0..inst.smp_body_w {
      let osc = if b_ovt {
        osci.get_one_sample_overtone(s)
      } else {
        osci.get_one_sample_coodinate(s)
      };
      for c in 0..ch as usize {
        let mut work = osc * pan_vol[c] as f64 / 64.0;
        if work > 1.0 {
          work = 1.0;
          inst.b_sine_over = true;
        }
        if work < -1.0 {
          work = -1.0;
          inst.b_sine_over = true;
        }
        let v = (work * 32767.0) as i16;
        let bytes = v.to_le_bytes();
        let idx = (s as usize * ch as usize + c) * 2;
        inst.samples_w[idx] = bytes[0];
        inst.samples_w[idx + 1] = bytes[1];
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
  let mut out = vec![0u8; pcm_i16.len() * 2];
  for (i, &s) in pcm_i16.iter().enumerate() {
    let bytes = s.to_le_bytes();
    out[i * 2] = bytes[0];
    out[i * 2 + 1] = bytes[1];
  }
  Ok(out)
}
