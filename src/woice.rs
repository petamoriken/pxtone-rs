use crate::error::PxtoneError;
use crate::event::{EVENTDEFAULT_BASICKEY, read_var_int};
use crate::pulse::frequency::FrequencyTable;
use crate::pulse::noise::Noise;
use crate::pulse::noise_builder::NoiseBuilder;
use crate::pulse::oscillator::{Oscillator, Point};
use crate::pulse::pcm::Pcm;
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek};

// ---- Constants ----
pub const MAX_WOICE_NAME: usize = 16;
pub const MAX_VOICE_NUM: usize = 2; // pxtnMAX_UNITCONTROLVOICE
pub const BUFSIZE_TIMEPAN: usize = 0x40;
pub const BIT_PER_SAMPLE: i32 = 16;

pub const VOICE_FLAG_WAVELOOP: u32 = 0x00000001;
pub const VOICE_FLAG_SMOOTH: u32 = 0x00000002;
pub const VOICE_FLAG_BEATFIT: u32 = 0x00000004;
pub const VOICE_FLAG_UNCOVERED: u32 = 0xfffffff8;

pub const DATA_FLAG_WAVE: u32 = 0x00000001;
pub const DATA_FLAG_ENVELOPE: u32 = 0x00000002;
pub const DATA_FLAG_UNCOVERED: u32 = 0xfffffffc;

// ---- Types ----
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WoiceType {
  #[default]
  None,
  Pcm,
  Ptv,
  Ptn,
  OggVorbis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VoiceType {
  #[default]
  Coodinate,
  Overtone,
  Noise,
  Sampling,
  OggVorbis,
}

// ---- Waveform / Envelope ----
#[derive(Clone, Debug, Default)]
pub struct VoiceWave {
  pub reso: i32,
  pub points: Vec<(i32, i32)>, // (x, y)
}

#[derive(Clone, Debug, Default)]
pub struct VoiceEnvelope {
  pub fps: i32,
  pub head_num: i32,
  pub body_num: i32,
  pub tail_num: i32,
  pub points: Vec<(i32, i32)>,
}

// ---- Unit (design data) ----
pub struct VoiceUnit {
  pub basic_key: i32,
  pub volume: i32,
  pub pan: i32,
  pub tuning: f32,
  pub voice_flags: u32,
  pub data_flags: u32,
  pub voice_type: VoiceType,
  pub pcm: Option<Pcm>,
  pub noise: Option<Noise>,
  pub ogg_data: Option<OggData>,
  pub wave: VoiceWave,
  pub envelope: VoiceEnvelope,
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
pub struct OggData {
  pub ch: i32,
  pub sps: i32,
  pub smp_num: i32,
  pub data: Vec<u8>,
}

// ---- Instance (synthesis buffer) ----
#[derive(Default)]
pub struct VoiceInstance {
  pub smp_head_w: i32,
  pub smp_body_w: i32,
  pub smp_tail_w: i32,
  pub samples_w: Vec<u8>, // stereo 16-bit interleaved
  pub env: Vec<u8>,
  pub env_size: i32,
  pub env_release: i32,
  pub b_sine_over: bool,
}

impl VoiceInstance {
  /// Gets one sample from an interleaved stereo 16-bit buffer
  pub fn get_sample_i16(&self, frame: usize, ch: usize) -> i16 {
    let offset = frame * 4 + ch * 2;
    if offset + 1 >= self.samples_w.len() {
      return 0;
    }
    i16::from_le_bytes([self.samples_w[offset], self.samples_w[offset + 1]])
  }
}

// ---- Woice ----
pub struct Woice {
  pub name: String,
  pub woice_type: WoiceType,
  pub x3x_tuning: f32,
  pub x3x_basic_key: i32,
  pub voices: Vec<VoiceUnit>,
  pub instances: Vec<VoiceInstance>,
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
  pub fn new() -> Self {
    Self::default()
  }

  // ---- PCM material loading ----
  pub fn read_mate_pcm<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_u32::<LE>()?;
    // _MATERIALSTRUCT_PCM (24 bytes)
    let _x3x_unit_no = r.read_u16::<LE>()?;
    let basic_key = r.read_u16::<LE>()? as i32;
    let voice_flags = r.read_u32::<LE>()?;
    let ch = r.read_u16::<LE>()? as i32;
    let bps = r.read_u16::<LE>()? as i32;
    let sps = r.read_u32::<LE>()? as i32;
    let tuning = r.read_f32::<LE>()?;
    let data_size = r.read_u32::<LE>()?;

    if voice_flags & VOICE_FLAG_UNCOVERED != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    let sample_num = data_size as i32 * 8 / bps / ch;
    let mut pcm = Pcm::create(ch, sps, bps, sample_num).ok_or(PxtoneError::UnknownFormat)?;
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
  pub fn read_mate_ptn<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
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
  pub fn read_mate_ptv<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
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
  pub fn read_mate_oggv<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_u32::<LE>()?;
    // _MATERIALSTRUCT_OGGV (12 bytes)
    let _xxx = r.read_u16::<LE>()?;
    let basic_key = r.read_u16::<LE>()? as i32;
    let voice_flags = r.read_u32::<LE>()?;
    let tuning = r.read_f32::<LE>()?;

    if voice_flags & VOICE_FLAG_UNCOVERED != 0 {
      return Err(PxtoneError::UnknownFormat);
    }

    // pxtn_read: ch, sps, smp_num, size, data
    let ch = r.read_i32::<LE>()?;
    let sps = r.read_i32::<LE>()?;
    let smp_num = r.read_i32::<LE>()?;
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

    let x3x_basic_key = read_var_int(r)?;
    let work1 = read_var_int(r)?;
    let work2 = read_var_int(r)?;
    if work1 != 0 || work2 != 0 {
      return Err(PxtoneError::UnknownFormat);
    }
    let voice_num = read_var_int(r)?;

    self.x3x_basic_key = x3x_basic_key;
    self.voices.clear();

    for _ in 0..voice_num {
      let mut unit = VoiceUnit::default();
      unit.basic_key = read_var_int(r)?;
      unit.volume = read_var_int(r)?;
      unit.pan = read_var_int(r)?;
      let tuning_bits = read_var_int(r)? as u32;
      unit.tuning = f32::from_bits(tuning_bits);
      unit.voice_flags = read_var_int(r)? as u32;
      unit.data_flags = read_var_int(r)? as u32;

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
  pub fn tone_ready_sample(
    &mut self,
    noise_builder: &mut NoiseBuilder,
    freq: &FrequencyTable,
  ) -> Result<(), PxtoneError> {
    let ch = 2;
    let sps = 44100;
    let bps = 16;

    self.instances.clear();
    for unit in &mut self.voices {
      let mut inst = VoiceInstance::default();
      match unit.voice_type {
        VoiceType::Sampling => {
          if let Some(pcm) = &unit.pcm {
            let mut work =
              Pcm::create(pcm.ch, pcm.sps, pcm.bps, pcm.smp_body).ok_or(PxtoneError::PcmConvert)?;
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
          let smp_body = 400i32;
          let size = (smp_body * ch * bps / 8) as usize;
          inst.smp_body_w = smp_body;
          inst.samples_w = vec![0u8; size];
          update_wave_ptv(unit, &mut inst, ch, sps, bps);
        }
        VoiceType::Noise => {
          if let Some(noise) = &mut unit.noise {
            if let Some(pcm) = noise_builder.build_noise(noise, ch as usize, sps, bps, freq) {
              inst.smp_body_w = noise.smp_num_44k;
              inst.samples_w = pcm.samples().to_vec();
            }
          }
        }
        VoiceType::OggVorbis => {
          if let Some(ogg) = &unit.ogg_data {
            let decoded = decode_ogg(&ogg.data).map_err(|e| PxtoneError::OggVorbis(e))?;
            let ogg_ch = ogg.ch;
            let ogg_sps = ogg.sps;
            let ogg_smp = ogg.smp_num;
            if let Some(mut work) = Pcm::create(ogg_ch, ogg_sps, 16, ogg_smp) {
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
      }
      self.instances.push(inst);
    }
    Ok(())
  }

  // ---- Envelope buffer preparation ----
  pub fn tone_ready_envelope(&mut self, sps: i32) -> Result<(), PxtoneError> {
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
        inst.env_size = (size as f64 * sps as f64 / env.fps as f64) as i32;
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
        inst.env_release = (env.points[tail_idx].0 as f64 * sps as f64 / env.fps as f64) as i32;
      }
    }
    Ok(())
  }

  pub fn tone_ready(
    &mut self,
    noise_builder: &mut NoiseBuilder,
    freq: &FrequencyTable,
    sps: i32,
  ) -> Result<(), PxtoneError> {
    self.tone_ready_sample(noise_builder, freq)?;
    self.tone_ready_envelope(sps)?;
    Ok(())
  }
}

// ---- PTV wave-reading helpers ----
fn ptv_read_wave<R: Read>(r: &mut R, unit: &mut VoiceUnit) -> Result<(), PxtoneError> {
  let vtype = read_var_int(r)?;
  unit.voice_type = match vtype {
    0 => VoiceType::Coodinate,
    1 => VoiceType::Overtone,
    _ => return Err(PxtoneError::Unsupported("voice type")),
  };
  match unit.voice_type {
    VoiceType::Coodinate => {
      let num = read_var_int(r)?;
      let reso = read_var_int(r)?;
      unit.wave.reso = reso;
      for _ in 0..num {
        let x = r.read_u8()? as i32;
        let y = r.read_i8()? as i32;
        unit.wave.points.push((x, y));
      }
    }
    VoiceType::Overtone => {
      let num = read_var_int(r)?;
      for _ in 0..num {
        let x = read_var_int(r)?;
        let y = read_var_int(r)?;
        unit.wave.points.push((x, y));
      }
    }
    _ => {}
  }
  Ok(())
}

fn ptv_read_envelope<R: Read>(r: &mut R, unit: &mut VoiceUnit) -> Result<(), PxtoneError> {
  let fps = read_var_int(r)?;
  let head_num = read_var_int(r)?;
  let body_num = read_var_int(r)?;
  let tail_num = read_var_int(r)?;
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
    let x = read_var_int(r)?;
    let y = read_var_int(r)?;
    unit.envelope.points.push((x, y));
  }
  Ok(())
}

// ---- PTV wave buffer update ----
fn update_wave_ptv(unit: &VoiceUnit, inst: &mut VoiceInstance, ch: i32, _sps: i32, bps: i32) {
  let smp_body = inst.smp_body_w as usize;
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
  osci.ready_get_sample(pts, unit.volume, smp_body as i32, unit.wave.reso);

  let b_ovt = unit.voice_type == VoiceType::Overtone;

  inst.b_sine_over = false;
  if bps == 8 {
    for s in 0..smp_body {
      let osc = if b_ovt {
        osci.get_one_sample_overtone(s as i32)
      } else {
        osci.get_one_sample_coodinate(s as i32)
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
        inst.samples_w[s * ch as usize + c] = ((work * 127.0) as i32 + 128) as u8;
      }
    }
  } else {
    for s in 0..smp_body {
      let osc = if b_ovt {
        osci.get_one_sample_overtone(s as i32)
      } else {
        osci.get_one_sample_coodinate(s as i32)
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
        let idx = (s * ch as usize + c) * 2;
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
