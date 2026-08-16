use crate::effect::delay::Delay;
use crate::effect::overdrive::OverDrive;
use crate::error::PxtoneError;
use crate::event::{
  EVENT_DEFAULT_BASIC_KEY, EVENT_DEFAULT_GROUP_NO, EVENT_DEFAULT_VOICE_NO, EVENT_KIND_GROUP_NO,
  EVENT_KIND_KEY, EVENT_KIND_ON, EVENT_KIND_PAN_TIME, EVENT_KIND_PAN_VOLUME, EVENT_KIND_PORTAMENT,
  EVENT_KIND_TUNING, EVENT_KIND_VELOCITY, EVENT_KIND_VOICE_NO, EVENT_KIND_VOLUME, EventList,
  EventRecord,
};
use crate::master::Master;
use crate::pulse::frequency::FrequencyTable;
use crate::pulse::noise::Noise;
use crate::pulse::noise_builder::NoiseBuilder;
use crate::reader::Reader;
use crate::text::Text;
use crate::unit::{MAX_CHANNEL, MAX_GROUP_COUNT, MAX_UNIT_CONTROL_VOICE, Unit};
use crate::woice::{BUFSIZE_TIMEPAN, VOICE_FLAG_BEATFIT, VOICE_FLAG_WAVELOOP, Woice};
use tinyvec::ArrayVec;

// ---- Constants ----
/// Samples rendered per pass of the block mixer. Bounded by the pan-delay ring
/// so the scratch accumulator stays small enough to sit in L1.
const MOO_BLOCK: usize = 128;

const MAX_UNIT_COUNT: usize = 50;
const MAX_WOICE_COUNT: usize = 100;
const MAX_DELAY_COUNT: usize = 4;
const MAX_OVERDRIVE_COUNT: usize = 2;
const MAX_WOICE_NAME: usize = 16;
const MAX_UNIT_NAME: usize = 16;
const MAX_OUTPUT_AMPLITUDE: i32 = 0x7fff;

const VERSION_SIZE: usize = 16;
const CODE_SIZE: usize = 8;

// Version strings
const CODE_TUNE_X2X: &[u8; 16] = b"PTTUNE--20050608";
const CODE_TUNE_X3X: &[u8; 16] = b"PTTUNE--20060115";
const CODE_TUNE_X4X: &[u8; 16] = b"PTTUNE--20060930";
const CODE_TUNE_V5: &[u8; 16] = b"PTTUNE--20071119";
const CODE_PROJ_X1X: &[u8; 16] = b"PTCOLLAGE-050227";
const CODE_PROJ_X2X: &[u8; 16] = b"PTCOLLAGE-050608";
const CODE_PROJ_X3X: &[u8; 16] = b"PTCOLLAGE-060115";
const CODE_PROJ_X4X: &[u8; 16] = b"PTCOLLAGE-060930";
const CODE_PROJ_V5: &[u8; 16] = b"PTCOLLAGE-071119";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FmtVer {
  X1x,
  X2x,
  X3x,
  X4x,
  V5,
}

// ---- Public API ----

/// Output audio quality (channel count and sample rate) used for playback and rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DestinationQuality {
  /// Number of output channels. `1` = mono, `2` = stereo.
  pub channels: u8,
  /// Sample rate in Hz (samples per second).
  pub sample_rate: u32,
}

impl Default for DestinationQuality {
  fn default() -> Self {
    Self {
      channels: 2,
      sample_rate: 44100,
    }
  }
}

/// Rendered audio returned by [`PxtoneService::render_noise`].
#[derive(Debug, Clone)]
pub struct NoiseWave {
  /// Raw PCM audio data in 16-bit little-endian signed format.
  pub samples: Vec<u8>,
  /// Number of channels. `1` = mono, `2` = stereo.
  pub channels: u8,
  /// Sample rate in Hz (samples per second).
  pub sample_rate: u32,
}

/// Start position for [`VomitPreparation`].
#[derive(Clone, Debug, Default)]
pub enum StartPos {
  /// Start from the beginning of the song.
  #[default]
  Beginning,
  /// Start at the given measure.
  Meas(u32),
  /// Start at the given sample offset.
  Sample(u32),
  /// Start at a fraction of the total song length (`0.0`–`1.0`).
  Float(f32),
}

/// Flag constants for [`VomitPreparation::flags`].
pub struct VomitPrepFlags;

impl VomitPrepFlags {
  /// Mute units whose [`Unit::played`](crate::unit::Unit::played) flag is `false`.
  pub const UNIT_MUTE: u8 = 0x1;
  /// Loop playback from [`VomitPreparation::measure_repeat`] when the end is reached.
  pub const LOOP: u8 = 0x2;
}

/// Playback settings passed to [`PxtoneService::moo_preparation`].
#[derive(Clone)]
pub struct VomitPreparation {
  /// Combination of [`VomitPrepFlags`] constants.
  pub flags: u8,
  /// Where in the song to begin playback.
  pub start_pos: StartPos,
  /// Measure at which playback ends. `None` uses the song's natural end.
  pub measure_end: Option<u32>,
  /// Measure to loop back to when the end is reached. `None` uses the song's repeat point.
  pub measure_repeat: Option<u32>,
  /// Fade-in duration in seconds. `0.0` means no fade-in.
  pub fade_in_secs: f32,
  /// Master volume scale factor. `1.0` is full volume.
  pub master_volume: f32,
}

impl Default for VomitPreparation {
  fn default() -> Self {
    Self {
      flags: 0,
      start_pos: StartPos::default(),
      measure_end: None,
      measure_repeat: None,
      fade_in_secs: 0.0,
      master_volume: 1.0,
    }
  }
}

/// Writes one 16-bit LE interleaved frame at sample index `pos`.
/// `byte_per_smp` is either 2 (mono) or 4 (stereo).
#[inline(always)]
fn write_frame(buf: &mut [u8], pos: usize, byte_per_smp: usize, sample: [i16; 2]) {
  let offset = pos * byte_per_smp;
  if byte_per_smp == 4 {
    let packed = sample[0] as u16 as u32 | ((sample[1] as u16 as u32) << 16);
    buf[offset..offset + 4].copy_from_slice(&packed.to_le_bytes());
  } else {
    buf[offset..offset + 2].copy_from_slice(&sample[0].to_le_bytes());
  }
}

// ---- PxtoneService ----

/// Decoder and playback engine for pxtone music files (`.ptcop`).
///
/// # Typical usage
///
/// ```no_run
/// use pxtone::{DestinationQuality, PxtoneService, VomitPreparation};
///
/// let mut service = PxtoneService::new(DestinationQuality::default()).unwrap();
/// let data = std::fs::read("song.ptcop").unwrap();
/// service.read(data).unwrap();
/// service.tones_ready().unwrap();
/// service.moo_preparation(VomitPreparation::default()).unwrap();
///
/// let q = service.get_destination_quality();
/// let mut buf = vec![0u8; q.channels as usize * 2 * 4096];
/// loop {
///     let written = service.moo(&mut buf);
///     if written == 0 { break; }
///     // process buf[..written] as 16-bit LE PCM...
/// }
/// ```
pub struct PxtoneService {
  text: Text,
  master: Master,
  events: EventList,
  units: Vec<Unit>,

  pub(crate) delays: ArrayVec<[Delay; MAX_DELAY_COUNT]>,
  pub(crate) overdrives: ArrayVec<[OverDrive; MAX_OVERDRIVE_COUNT]>,
  pub(crate) woices: Vec<Woice>,

  noise_builder: NoiseBuilder,
  frequency: FrequencyTable,

  // Output quality
  dst_channels: u8,
  dst_sample_rate: u32,

  // moo runtime
  group_count: usize,
  unit_woice_idxs: Vec<usize>, // current voice index per unit

  /// Number of leading groups that can actually receive samples. Groups beyond
  /// it always stay zero, so the per-sample mixing loop skips them.
  moo_group_count: usize,

  moo_samples_per_tick: f64,
  moo_sample_stride: f32,
  moo_sample_count: u32,
  moo_sample_end: u32,
  moo_sample_repeat: u32,
  moo_sample_start: u32,
  moo_sample_smooth: u32,
  moo_output_clip: i32,
  moo_ticks_per_beat: u16,
  moo_beats_per_measure: u8,
  moo_beat_tempo: f32,
  moo_time_pan_index: usize,
  moo_event_index: usize,
  moo_loop: bool,
  moo_mute_by_unit: bool,
  moo_master_volume: f32,
  moo_fade_direction: i32,
  moo_fade_count: u32,
  moo_fade_max: u32,

  data_loaded: bool,
  playback_ended: bool,

  // Retained raw file bytes set by read_with_data(); consumed and cleared by tones_ready().
  raw_data: Vec<u8>,
}

impl PxtoneService {
  pub fn new(quality: DestinationQuality) -> Result<Self, PxtoneError> {
    if quality.channels != 1 && quality.channels != 2 {
      return Err(PxtoneError::Init);
    }
    Ok(Self {
      text: Text::new(),
      master: Master::new(),
      events: EventList::new(),
      woices: Vec::new(),
      units: Vec::new(),
      delays: ArrayVec::new(),
      overdrives: ArrayVec::new(),
      noise_builder: NoiseBuilder::new(),
      frequency: FrequencyTable::new(),

      dst_channels: quality.channels,
      dst_sample_rate: quality.sample_rate,

      group_count: MAX_GROUP_COUNT,
      unit_woice_idxs: Vec::new(),
      moo_group_count: MAX_GROUP_COUNT,

      moo_samples_per_tick: 0.0,
      moo_sample_stride: 1.0,
      moo_sample_count: 0,
      moo_sample_end: 0,
      moo_sample_repeat: 0,
      moo_sample_start: 0,
      moo_sample_smooth: 0,
      moo_output_clip: MAX_OUTPUT_AMPLITUDE,
      moo_ticks_per_beat: 0,
      moo_beats_per_measure: 0,
      moo_beat_tempo: 0.0,
      moo_time_pan_index: 0,
      moo_event_index: 0,
      moo_loop: true,
      moo_mute_by_unit: false,
      moo_master_volume: 1.0,
      moo_fade_direction: 0,
      moo_fade_count: 0,
      moo_fade_max: 0,

      data_loaded: false,
      playback_ended: true,

      raw_data: Vec::new(),
    })
  }

  /// Sets the output audio quality. The default is stereo (2 ch) at 44100 Hz.
  ///
  /// Call this before [`tones_ready`](Self::tones_ready).
  pub fn set_destination_quality(
    &mut self,
    quality: DestinationQuality,
  ) -> Result<(), PxtoneError> {
    if quality.channels != 1 && quality.channels != 2 {
      return Err(PxtoneError::Init);
    }
    self.dst_channels = quality.channels;
    self.dst_sample_rate = quality.sample_rate;
    Ok(())
  }

  /// Returns a reference to the song text metadata.
  #[inline]
  pub fn text(&self) -> &Text {
    &self.text
  }

  /// Returns a reference to the song timing parameters.
  #[inline]
  pub fn master(&self) -> &Master {
    &self.master
  }

  /// Returns a reference to the event list.
  #[inline]
  pub fn events(&self) -> &EventList {
    &self.events
  }

  /// Returns a slice of the units (tracks).
  #[inline]
  pub fn units(&self) -> &[Unit] {
    &self.units
  }

  /// Returns a mutable slice of the units (tracks).
  #[inline]
  pub fn units_mut(&mut self) -> &mut [Unit] {
    &mut self.units
  }

  /// Returns the current output audio quality.
  #[inline]
  pub fn get_destination_quality(&self) -> DestinationQuality {
    DestinationQuality {
      channels: self.dst_channels,
      sample_rate: self.dst_sample_rate,
    }
  }

  /// Loads a `.ptnoise` file and returns the rendered audio.
  ///
  /// The output format matches the current destination quality.
  pub fn render_noise(&mut self, data: &[u8]) -> Result<NoiseWave, PxtoneError> {
    let mut noise = Noise::new();
    noise.read(&mut Reader::new(data))?;
    let pcm = self.noise_builder.build_noise(
      &mut noise,
      self.dst_channels,
      self.dst_sample_rate,
      16,
      &self.frequency,
    )?;
    Ok(NoiseWave {
      samples: pcm.samples().to_vec(),
      channels: self.dst_channels,
      sample_rate: self.dst_sample_rate,
    })
  }

  // ---- File loading ----

  /// Loads a `.ptcop` or `.pttune` file for playback. Clears any previously loaded data.
  ///
  /// The raw bytes are retained internally so that [`tones_ready`](Self::tones_ready) can load
  /// and decode PCM samples and OGG streams. They are freed automatically after `tones_ready()`
  /// completes.
  ///
  /// Call [`tones_ready`](Self::tones_ready) after loading.
  pub fn read(&mut self, data: Vec<u8>) -> Result<(), PxtoneError> {
    self.read_metadata(data.as_slice())?;
    self.raw_data = data;
    Ok(())
  }

  /// Parses the file structure without retaining the binary data (PCM samples, OGG streams).
  ///
  /// Use this for lightweight validation. For playback, use [`read`](Self::read) instead.
  pub fn read_metadata(&mut self, data: &[u8]) -> Result<(), PxtoneError> {
    let r = &mut Reader::new(data);
    self.clear();

    let fmt_ver = self.read_version(r)?;
    self.read_tune_items(r, fmt_ver)?;

    if matches!(fmt_ver, FmtVer::X3x | FmtVer::X2x | FmtVer::X1x) {
      self.x3x_tuning_key_event()?;
      self.x3x_add_tuning_event();
      self.x3x_set_voice_names();
    }

    let max_event_tick = self.events.get_max_tick() as u32;
    let last_master_tick = self.master.get_last_tick();
    self
      .master
      .adjust_measure_count(max_event_tick.max(last_master_tick));

    self.data_loaded = true;
    Ok(())
  }

  fn clear(&mut self) {
    self.text = Text::new();
    self.master = Master::new();
    self.events.clear();
    self.woices.clear();
    self.units.clear();
    self.delays.clear();
    self.overdrives.clear();
    self.unit_woice_idxs.clear();
    self.data_loaded = false;
    self.raw_data = Vec::new();
  }

  /// Reads the version string and returns a FmtVer
  fn read_version(&self, r: &mut Reader<'_>) -> Result<FmtVer, PxtoneError> {
    let mut ver = [0u8; VERSION_SIZE];
    r.read_exact(&mut ver)?;

    // x1x / x2x do not have exe_ver/rrr fields
    if &ver == CODE_PROJ_X1X {
      return Ok(FmtVer::X1x);
    }
    if &ver == CODE_PROJ_X2X {
      return Ok(FmtVer::X2x);
    }
    if &ver == CODE_TUNE_X2X {
      return Ok(FmtVer::X2x);
    }

    let fmt_ver = if &ver == CODE_PROJ_X3X || &ver == CODE_TUNE_X3X {
      FmtVer::X3x
    } else if &ver == CODE_PROJ_X4X || &ver == CODE_TUNE_X4X {
      FmtVer::X4x
    } else if &ver == CODE_PROJ_V5 || &ver == CODE_TUNE_V5 {
      FmtVer::V5
    } else {
      return Err(PxtoneError::UnknownFormat);
    };

    // Skip exe_ver + rrr (4 bytes)
    let _exe_ver = r.read_u16()?;
    let _rrr = r.read_u16()?;

    Ok(fmt_ver)
  }

  fn read_tune_items(&mut self, r: &mut Reader<'_>, _fmt_ver: FmtVer) -> Result<(), PxtoneError> {
    loop {
      let mut code = [0u8; CODE_SIZE];
      r.read_exact(&mut code)?;

      match &code {
        // v5 tags
        b"num UNIT" => {
          let size = r.read_i32()?;
          if size != 4 {
            return Err(PxtoneError::UnknownFormat);
          }
          let num = r.read_i16()? as usize;
          let rrr = r.read_i16()?;
          if rrr != 0 {
            return Err(PxtoneError::UnknownFormat);
          }
          if num > MAX_UNIT_COUNT {
            return Err(PxtoneError::UnknownFormat);
          }
          let mut units = Vec::with_capacity(num);
          units.resize_with(num, Unit::new);
          self.units = units;
          self.unit_woice_idxs = vec![0usize; num];
        }
        b"MasterV5" => self.master.read_v5(r)?,
        b"Event V5" => self.events.read_v5(r)?,

        b"matePCM " | b"matePCM=" => {
          if self.woices.len() >= MAX_WOICE_COUNT {
            return Err(PxtoneError::WoiceFull);
          }
          let mut w = Woice::new();
          w.read_mate_pcm(r)?;
          self.woices.push(w);
        }
        b"matePTV " => {
          if self.woices.len() >= MAX_WOICE_COUNT {
            return Err(PxtoneError::WoiceFull);
          }
          let mut w = Woice::new();
          w.read_mate_ptv(r)?;
          self.woices.push(w);
        }
        b"matePTN " => {
          if self.woices.len() >= MAX_WOICE_COUNT {
            return Err(PxtoneError::WoiceFull);
          }
          let mut w = Woice::new();
          w.read_mate_ptn(r)?;
          self.woices.push(w);
        }
        b"mateOGGV" => {
          if self.woices.len() >= MAX_WOICE_COUNT {
            return Err(PxtoneError::WoiceFull);
          }
          let mut w = Woice::new();
          w.read_mate_oggv(r)?;
          self.woices.push(w);
        }
        b"effeDELA" => {
          if self.delays.len() >= MAX_DELAY_COUNT {
            return Err(PxtoneError::UnknownFormat);
          }
          let mut d = Delay::new();
          d.read(r)?;
          self.delays.push(d);
        }
        b"effeOVER" => {
          if self.overdrives.len() >= MAX_OVERDRIVE_COUNT {
            return Err(PxtoneError::UnknownFormat);
          }
          let mut od = OverDrive::new();
          od.read(r)?;
          self.overdrives.push(od);
        }
        b"textNAME" => self.text.read_name(r)?,
        b"textCOMM" => self.text.read_comment(r)?,
        b"assiWOIC" => self.read_assi_woic(r)?,
        b"assiUNIT" => self.read_assi_unit(r)?,

        b"pxtoneND" | b"END=====" => {
          let _end = r.read_i32()?; // 0
          break;
        }

        // Legacy formats
        b"evenMAST" => self.master.read_x4x(r)?,
        b"evenUNIT" => self.events.read_x4x_block(r, false, true)?,
        b"pxtnUNIT" => self.read_old_unit_v3(r)?,
        b"PROJECT=" => self.read_x1x_project(r)?,
        b"UNIT====" => self.read_old_unit_v1(r)?,
        b"EVENT===" => self.events.read_x4x_block(r, true, false)?,

        b"antiOPER" => return Err(PxtoneError::AntiOperation),

        _ => return Err(PxtoneError::UnknownFormat),
      }
    }
    Ok(())
  }

  fn read_assi_woic(&mut self, r: &mut Reader<'_>) -> Result<(), PxtoneError> {
    let size = r.read_i32()?;
    if size != (2 + 2 + MAX_WOICE_NAME) as i32 {
      return Err(PxtoneError::UnknownFormat);
    }
    let woice_index = r.read_u16()? as usize;
    let rrr = r.read_u16()?;
    let mut name = [0u8; MAX_WOICE_NAME];
    r.read_exact(&mut name)?;

    if rrr != 0 {
      return Err(PxtoneError::UnknownFormat);
    }
    if woice_index >= self.woices.len() {
      return Err(PxtoneError::UnknownFormat);
    }

    let end = name.iter().position(|&b| b == 0).unwrap_or(MAX_WOICE_NAME);
    self.woices[woice_index].name = String::from_utf8_lossy(&name[..end]).into_owned();
    Ok(())
  }

  fn read_assi_unit(&mut self, r: &mut Reader<'_>) -> Result<(), PxtoneError> {
    let size = r.read_i32()?;
    if size != (2 + 2 + MAX_UNIT_NAME) as i32 {
      return Err(PxtoneError::UnknownFormat);
    }
    let unit_index = r.read_u16()? as usize;
    let rrr = r.read_u16()?;
    let mut name = [0u8; MAX_UNIT_NAME];
    r.read_exact(&mut name)?;

    if rrr != 0 {
      return Err(PxtoneError::UnknownFormat);
    }
    if unit_index >= self.units.len() {
      return Err(PxtoneError::UnknownFormat);
    }

    let end = name.iter().position(|&b| b == 0).unwrap_or(MAX_UNIT_NAME);
    self.units[unit_index].name = name[..end].to_vec();
    Ok(())
  }

  /// Reads a v1x unit struct (size:i32 + name[16] + type:u16 + group:u16)
  fn read_old_unit_v1(&mut self, r: &mut Reader<'_>) -> Result<(), PxtoneError> {
    if self.units.len() >= MAX_UNIT_COUNT {
      return Err(PxtoneError::UnknownFormat);
    }

    let _size = r.read_i32()?;
    let mut name = [0u8; MAX_UNIT_NAME];
    r.read_exact(&mut name)?;
    let _utype = r.read_u16()?;
    let group = r.read_u16()? as i32;

    let u_idx = self.units.len();
    let end = name.iter().position(|&b| b == 0).unwrap_or(MAX_UNIT_NAME);
    let mut unit = Unit::new();
    unit.name = name[..end].to_vec();
    self.units.push(unit);
    self.unit_woice_idxs.push(0);

    let g = group.min(self.group_count as i32 - 1);
    self.events.add_i(0, u_idx as u8, EVENT_KIND_GROUP_NO, g);
    self
      .events
      .add_i(0, u_idx as u8, EVENT_KIND_VOICE_NO, u_idx as i32);
    Ok(())
  }

  /// Reads a v3x unit struct (size:i32 + type:u16 + group:u16)
  fn read_old_unit_v3(&mut self, r: &mut Reader<'_>) -> Result<(), PxtoneError> {
    if self.units.len() >= MAX_UNIT_COUNT {
      return Err(PxtoneError::UnknownFormat);
    }

    let _size = r.read_i32()?;
    let _utype = r.read_u16()?;
    let group = r.read_u16()? as i32;

    let u_idx = self.units.len();
    self.units.push(Unit::new());
    self.unit_woice_idxs.push(0);

    let g = group.min(self.group_count as i32 - 1);
    self.events.add_i(0, u_idx as u8, EVENT_KIND_GROUP_NO, g);
    self
      .events
      .add_i(0, u_idx as u8, EVENT_KIND_VOICE_NO, u_idx as i32);
    Ok(())
  }

  /// Reads x1x project info (size:i32 + name[16] + ...)
  fn read_x1x_project(&mut self, r: &mut Reader<'_>) -> Result<(), PxtoneError> {
    let _size = r.read_i32()?;
    let mut name = [0u8; 16];
    r.read_exact(&mut name)?;
    let beat_tempo = r.read_f32()?;
    let ticks_per_beat = r.read_u16()?;
    let beats_per_measure = r.read_u16()? as u8;
    let _beat_note = r.read_u16()?;
    let _measure_count = r.read_u16()?;
    let _channels = r.read_u16()?;
    let _bits_per_sample = r.read_u16()?;
    let _sample_rate = r.read_u32()?;

    self.text.set_name_raw(&name);
    self.master.beats_per_measure = beats_per_measure;
    self.master.beat_tempo = beat_tempo;
    self.master.ticks_per_beat = ticks_per_beat;
    Ok(())
  }

  // ---- x3x/x2x/x1x post-processing ----

  fn x3x_tuning_key_event(&mut self) -> Result<(), PxtoneError> {
    use crate::event::EVENT_KIND_KEY;
    let unit_count = self.units.len().min(self.woices.len());
    for u in 0..unit_count {
      let change = self.woices[u].x3x_basic_key as i32 - EVENT_DEFAULT_BASIC_KEY as i32;
      let has_key = self
        .events
        .records()
        .iter()
        .any(|e| e.unit_index == u as u8 && e.kind == EVENT_KIND_KEY);
      if !has_key {
        self.events.add_i(0, u as u8, EVENT_KIND_KEY, 0x6000);
      }
      self
        .events
        .value_change(0, -1, u as u8, EVENT_KIND_KEY, change);
    }
    Ok(())
  }

  fn x3x_add_tuning_event(&mut self) {
    let unit_count = self.units.len().min(self.woices.len());
    for u in 0..unit_count {
      let tuning = self.woices[u].x3x_tuning;
      if tuning != 0.0 {
        self.events.add_f(0, u as u8, EVENT_KIND_TUNING, tuning);
      }
    }
  }

  fn x3x_set_voice_names(&mut self) {
    for (i, w) in self.woices.iter_mut().enumerate() {
      w.name = format!("voice_{:02}", i);
    }
  }

  // ---- tone_ready / tone_clear ----

  /// Prepares all instruments for playback.
  ///
  /// Must be called after [`read`](Self::read) and before [`moo_preparation`](Self::moo_preparation).
  pub fn tones_ready(&mut self) -> Result<(), PxtoneError> {
    let sample_rate = self.dst_sample_rate;

    // noise_builder, freq, woices, and raw_data are independent fields — simultaneous borrows are OK
    let noise_builder = &mut self.noise_builder;
    let freq = &self.frequency;
    let raw_data: &[u8] = &self.raw_data;
    for w in &mut self.woices {
      w.tone_ready(noise_builder, freq, sample_rate, raw_data)?;
    }
    self.raw_data = Vec::new();
    for d in &mut self.delays {
      d.tone_ready(
        self.master.beats_per_measure,
        self.master.beat_tempo,
        sample_rate,
      );
    }
    for od in &mut self.overdrives {
      od.tone_ready();
    }
    Ok(())
  }

  fn tones_clear(&mut self) {
    for d in &mut self.delays {
      d.tone_clear();
    }
    for u in &mut self.units {
      u.tone_clear();
    }
  }

  // ---- moo synthesis engine ----

  /// Configures a playback session. Must be called before the first [`moo`](Self::moo) call.
  pub fn moo_preparation(&mut self, prep: VomitPreparation) -> Result<(), PxtoneError> {
    if !self.data_loaded || self.dst_channels == 0 || self.dst_sample_rate == 0 {
      self.playback_ended = true;
      return Err(PxtoneError::Init);
    }

    let measure_end = prep
      .measure_end
      .unwrap_or_else(|| self.master.get_play_meas());
    let measure_repeat = prep.measure_repeat.unwrap_or(self.master.repeat_measure);
    let fade_in_secs = prep.fade_in_secs;
    self.moo_mute_by_unit = prep.flags & VomitPrepFlags::UNIT_MUTE != 0;
    self.moo_loop = prep.flags & VomitPrepFlags::LOOP != 0;
    self.moo_master_volume = prep.master_volume;

    self.moo_ticks_per_beat = self.master.ticks_per_beat;
    self.moo_beats_per_measure = self.master.beats_per_measure;
    self.moo_beat_tempo = self.master.beat_tempo;
    self.moo_samples_per_tick = 60.0 * self.dst_sample_rate as f64
      / (self.moo_beat_tempo as f64 * self.moo_ticks_per_beat as f64);
    self.moo_sample_stride = 44100.0 / self.dst_sample_rate as f32;
    self.moo_output_clip = 0x7fff;
    self.moo_time_pan_index = 0;

    let samples_per_measure = self.moo_beats_per_measure as f64
      * self.moo_ticks_per_beat as f64
      * self.moo_samples_per_tick;
    self.moo_sample_end = (measure_end as f64 * samples_per_measure) as u32;
    self.moo_sample_repeat = (measure_repeat as f64 * samples_per_measure) as u32;

    self.moo_sample_start = match prep.start_pos {
      StartPos::Float(f) => {
        let total = self.calc_total_sample();
        (total as f32 * f) as u32
      }
      StartPos::Sample(s) => s,
      StartPos::Meas(m) => (m as f64 * samples_per_measure) as u32,
      StartPos::Beginning => 0,
    };

    self.moo_sample_count = self.moo_sample_start;
    self.moo_sample_smooth = self.dst_sample_rate / 250;

    if fade_in_secs > 0.0 {
      self.moo_set_fade(1, fade_in_secs);
    } else {
      self.moo_set_fade(0, 0.0);
    }

    self.moo_group_count = self.calc_group_count();
    self.tones_clear();
    self.moo_event_index = 0;
    self.moo_init_unit_tone();
    self.playback_ended = false;
    Ok(())
  }

  /// Highest group index reachable by any unit or effect, plus one.
  /// Groups above it never receive a sample, so leaving them out of the
  /// per-sample mix is bit-identical and shortens the inner loop.
  fn calc_group_count(&self) -> usize {
    let mut count = EVENT_DEFAULT_GROUP_NO + 1;
    for ev in self.events.records() {
      if ev.kind == EVENT_KIND_GROUP_NO {
        let group = ev.value as usize;
        if group < MAX_GROUP_COUNT {
          count = count.max(group + 1);
        }
      }
    }
    for d in &self.delays {
      count = count.max(d.group + 1);
    }
    for od in &self.overdrives {
      count = count.max(od.group + 1);
    }
    count.min(MAX_GROUP_COUNT)
  }

  fn moo_set_fade(&mut self, fade: i32, sec: f32) {
    self.moo_fade_max = ((self.dst_sample_rate as f32 * sec) as u32) >> 8;
    if fade < 0 {
      self.moo_fade_direction = -1;
      self.moo_fade_count = self.moo_fade_max << 8;
    } else if fade > 0 {
      self.moo_fade_direction = 1;
      self.moo_fade_count = 0;
    } else {
      self.moo_fade_direction = 0;
      self.moo_fade_count = 0;
    }
  }

  fn calc_total_sample(&self) -> u32 {
    let tempo = self.master.beat_tempo;
    if tempo == 0.0 {
      return 0;
    }
    let total_beats = self.master.measure_count * self.master.beats_per_measure as u32;
    (self.dst_sample_rate as f64 * 60.0 * total_beats as f64 / tempo as f64) as u32
  }

  fn moo_reset_voice_on(&mut self, unit_idx: usize, woice_idx: usize) {
    if self.woices.is_empty() {
      return;
    }
    if unit_idx >= self.units.len() {
      return;
    }

    let woice_idx = woice_idx.min(self.woices.len() - 1);

    // Collect voice_flags from the woice
    let voice_count = self.woices[woice_idx].voices.len();
    let mut voice_flags = [0u32; MAX_UNIT_CONTROL_VOICE];
    for (dst, v) in voice_flags
      .iter_mut()
      .zip(self.woices[woice_idx].voices.iter())
    {
      *dst = v.voice_flags;
    }

    self.units[unit_idx].set_woice(voice_count, voice_flags);

    if unit_idx < self.unit_woice_idxs.len() {
      self.unit_woice_idxs[unit_idx] = woice_idx;
    }

    // Compute ofs_freq and env_rls_ticks for each voice, then reset
    let samples_per_tick = self.moo_samples_per_tick;
    let bt_tempo = self.moo_beat_tempo;
    let inst_len = self.woices[woice_idx].instances.len();

    for v in 0..voice_count.min(inst_len) {
      let vc = &self.woices[woice_idx].voices[v];
      let inst = &self.woices[woice_idx].instances[v];
      let body_frames = inst.body_frames;
      let envelope_release = inst.envelope_release;
      let basic_key = vc.basic_key;
      let tuning = vc.tuning;
      let beat_fit = vc.voice_flags & VOICE_FLAG_BEATFIT != 0;

      let ofs_freq = if beat_fit {
        if tuning != 0.0 {
          (body_frames as f32 * bt_tempo) / (44100.0 * 60.0 * tuning)
        } else {
          0.0
        }
      } else {
        self
          .frequency
          .get(EVENT_DEFAULT_BASIC_KEY as i32 - basic_key as i32)
          * tuning
      };

      let env_rls_ticks = if samples_per_tick > 0.0 {
        (envelope_release as f64 / samples_per_tick) as u32
      } else {
        0
      };

      self.units[unit_idx].tone_reset_and_2prm(v, env_rls_ticks, ofs_freq);
    }
  }

  fn moo_init_unit_tone(&mut self) {
    for u in 0..self.units.len() {
      self.units[u].tone_init();
      self.moo_reset_voice_on(u, EVENT_DEFAULT_VOICE_NO);
    }
  }

  /// Returns the number of samples that can be synthesized before the next
  /// event fires, end-of-stream is reached, or a fade-out completes.
  /// Processing this many samples is safe without event/boundary checks.
  fn moo_safe_count(&self) -> u32 {
    // Samples until the next event would fire.
    // Event fires when floor(sample_count / samples_per_tick) >= ev_tick,
    // i.e. sample_count >= ev_tick * samples_per_tick.
    let event_safe = if self.moo_event_index < self.events.records().len() {
      let next_ev_tick = self.events.records()[self.moo_event_index].tick as f64;
      let threshold = (next_ev_tick * self.moo_samples_per_tick) as u32;
      threshold.saturating_sub(self.moo_sample_count)
    } else {
      u32::MAX
    };

    // Samples until the loop/end boundary (the boundary sample itself must go
    // through the full path, so subtract 1).
    let end_safe = self
      .moo_sample_end
      .saturating_sub(self.moo_sample_count + 1);

    // Samples until a fade-out returns false (N decrements succeed before the
    // (N+1)th call returns false at fade_count == 0).
    let fade_safe = if self.moo_fade_direction < 0 {
      self.moo_fade_count
    } else {
      u32::MAX
    };

    event_safe.min(end_safe).min(fade_safe)
  }

  /// Synthesizes the single sample on which events fire, writing it into
  /// `out[0..channels]`. Returns `true` while playing, `false` when the end is
  /// reached. Event-free stretches go through [`PxtoneService::moo_block`].
  fn moo_pxtone_sample(&mut self, out: &mut [i16; 2]) -> bool {
    let channel_count = self.dst_channels as usize;
    let channels = self.dst_channels;
    let samples_per_tick = self.moo_samples_per_tick;
    let mute_by_unit = self.moo_mute_by_unit;
    let smooth_samples = self.moo_sample_smooth;
    let time_pan_idx = self.moo_time_pan_index;
    let sample_end = self.moo_sample_end;
    let sample_stride = self.moo_sample_stride;

    // Runs once per event boundary, so it always takes the full-width
    // accumulator rather than being monomorphized over the group count.
    let mut group_smps = [[0i32; MAX_GROUP_COUNT]; MAX_CHANNEL];

    // ---- 1. Envelope processing ----
    // Every unit's envelope has to be up to date before events are dispatched,
    // so this stage cannot be folded into the mixing pass below.
    {
      let woices = &self.woices;
      for (unit, &wi) in self.units.iter_mut().zip(self.unit_woice_idxs.iter()) {
        if !unit.is_sounding() {
          continue;
        }
        if let Some(woice) = woices.get(wi) {
          unit.tone_envelope(&woice.instances);
        }
      }
    }

    // ---- 2. Event processing ----
    let tick = (self.moo_sample_count as f64 / samples_per_tick) as i32;
    let event_count = self.events.records().len();

    while self.moo_event_index < event_count {
      let ev_tick = self.events.records()[self.moo_event_index].tick;
      if ev_tick > tick {
        break;
      }

      let ev: EventRecord = self.events.records()[self.moo_event_index];
      self.moo_event_index += 1;

      let u = ev.unit_index as usize;
      if u >= self.units.len() {
        continue;
      }

      self.process_event(&ev, u, tick, sample_end, samples_per_tick);
    }

    // ---- 3. Mix + advance → group accumulation ----
    {
      let woices = &self.woices;
      let frequency = &self.frequency;
      for (unit, &wi) in self.units.iter_mut().zip(self.unit_woice_idxs.iter()) {
        if let Some(woice) = woices.get(wi) {
          if unit.is_sounding() {
            let key = unit.tone_increment_key();
            let freq = frequency.get2(key) * sample_stride;
            unit.tone_sample::<false>(
              mute_by_unit,
              channels,
              time_pan_idx,
              smooth_samples,
              freq,
              &woice.instances,
            );
          } else {
            unit.tone_silence(time_pan_idx);
          }
        }
        if !unit.is_flushed() {
          unit.tone_supple(&mut group_smps, channel_count, time_pan_idx);
        }
      }
    }

    // ---- 4. Effects → output ----
    self.moo_effects(&mut group_smps, channel_count);
    self.moo_output(&group_smps, channel_count, out);

    // ---- 5. Increment ----
    self.moo_sample_count += 1;
    self.moo_time_pan_index = (time_pan_idx + 1) & (BUFSIZE_TIMEPAN - 1);

    // ---- 6. Fade processing ----
    if self.moo_fade_direction < 0 {
      if self.moo_fade_count > 0 {
        self.moo_fade_count -= 1;
      } else {
        return false;
      }
    } else if self.moo_fade_direction > 0 {
      self.moo_fade_step_in();
    }

    // ---- 7. Loop / end-of-stream check ----
    if self.moo_sample_count >= self.moo_sample_end {
      if !self.moo_loop {
        return false;
      }
      self.moo_sample_count = self.moo_sample_repeat;
      self.moo_event_index = 0;
      self.moo_init_unit_tone();
    }

    true
  }

  /// Runs the overdrives and delays over one sample's group accumulator.
  /// Overdrive is per-channel and stateless; each delay walks both channels in
  /// one call so its buffer/rate/group state is loaded once per sample instead
  /// of once per channel. Within a channel the overdrive -> delay order is
  /// unchanged, and neither effect reads the other channel's groups.
  #[inline(always)]
  fn moo_effects<const GROUPS: usize>(
    &mut self,
    group_smps: &mut [[i32; GROUPS]; MAX_CHANNEL],
    channel_count: usize,
  ) {
    for groups in group_smps.iter_mut().take(channel_count) {
      for od in &self.overdrives {
        od.tone_supple(groups);
      }
    }
    for d in &mut self.delays {
      d.tone_supple(group_smps, channel_count);
    }
  }

  /// Sums one sample's groups and applies fade, master volume and clipping.
  #[inline(always)]
  fn moo_output<const GROUPS: usize>(
    &self,
    group_smps: &[[i32; GROUPS]; MAX_CHANNEL],
    channel_count: usize,
    out: &mut [i16; 2],
  ) {
    for (groups, out_sample) in group_smps.iter().zip(out.iter_mut()).take(channel_count) {
      let mut work: i32 = groups.iter().sum();

      // Fade
      if self.moo_fade_direction != 0 && self.moo_fade_max != 0 {
        work = work * (self.moo_fade_count >> 8) as i32 / self.moo_fade_max as i32;
      }

      // Master volume
      work = (work as f32 * self.moo_master_volume) as i32;

      // Clip
      work = work.clamp(-self.moo_output_clip, self.moo_output_clip);
      *out_sample = work as i16;
    }
  }

  /// One step of a fade-in. Fade-outs are handled by the caller because they
  /// can end playback.
  #[inline(always)]
  fn moo_fade_step_in(&mut self) {
    if self.moo_fade_count < (self.moo_fade_max << 8) {
      self.moo_fade_count += 1;
    } else {
      self.moo_fade_direction = 0;
    }
  }

  /// Synthesizes `count` samples that need no event dispatch into `buf`.
  ///
  /// The whole block is rendered unit by unit rather than sample by sample:
  /// walking the samples inside the per-unit loop lets the backend hoist the
  /// unit's mixing parameters and its voice-layer state out of the loop, which
  /// measurements show costs far more than the arithmetic itself. Units are
  /// independent, and integer accumulation is order-insensitive, so the mix is
  /// identical to rendering sample by sample.
  ///
  /// The caller must guarantee (via `moo_safe_count`) that no event fires and
  /// that neither the fade-out nor the end of the song lands inside the block.
  fn moo_block<const GROUPS: usize>(&mut self, buf: &mut [u8], byte_per_smp: usize, count: usize) {
    let mut mix = [[[0i32; GROUPS]; MAX_CHANNEL]; MOO_BLOCK];
    let mix = &mut mix[..count];

    let channel_count = self.dst_channels as usize;
    let channels = self.dst_channels;
    let mute_by_unit = self.moo_mute_by_unit;
    let smooth_samples = self.moo_sample_smooth;
    let sample_stride = self.moo_sample_stride;
    let time_pan_idx = self.moo_time_pan_index;

    {
      let woices = &self.woices;
      let frequency = &self.frequency;
      for (unit, &wi) in self.units.iter_mut().zip(self.unit_woice_idxs.iter()) {
        if let Some(woice) = woices.get(wi) {
          unit.tone_block(
            mix,
            mute_by_unit,
            channels,
            channel_count,
            time_pan_idx,
            smooth_samples,
            frequency,
            sample_stride,
            &woice.instances,
          );
        }
      }
    }

    for (i, groups) in mix.iter_mut().enumerate() {
      self.moo_effects(groups, channel_count);

      let mut sample = [0i16; 2];
      self.moo_output(groups, channel_count, &mut sample);
      write_frame(buf, i, byte_per_smp, sample);

      if self.moo_fade_direction < 0 {
        self.moo_fade_count -= 1;
      } else if self.moo_fade_direction > 0 {
        self.moo_fade_step_in();
      }
    }

    self.moo_sample_count += count as u32;
    self.moo_time_pan_index = (time_pan_idx + count) & (BUFSIZE_TIMEPAN - 1);
  }

  /// Processes one event
  fn process_event(
    &mut self,
    ev: &EventRecord,
    u: usize,
    tick: i32,
    sample_end: u32,
    samples_per_tick: f64,
  ) {
    match ev.kind {
      EVENT_KIND_ON => {
        let on_count = ((ev.tick + ev.value - tick) as f64 * samples_per_tick) as i32;
        if on_count <= 0 {
          self.units[u].tone_zero_lives();
          return;
        }
        self.units[u].tone_key_on();

        // Pre-compute values needed for mid-note seek sample_pos correction.
        // elapsed > 0 means we started playback after this note's ON tick.
        let elapsed = tick - ev.tick;
        let unit_key = self.units[u].key;
        let unit_tuning = self.units[u].tuning;
        let unit_freq = self.frequency.get2(unit_key) * self.moo_sample_stride;

        let wi = self.unit_woice_idxs.get(u).copied().unwrap_or(0);
        let voice_count = self.woices.get(wi).map(|w| w.voices.len()).unwrap_or(0);

        for v in 0..voice_count {
          // Read instance data first (immutable borrow of self.woices)
          let envelope_release = self
            .woices
            .get(wi)
            .and_then(|w| w.instances.get(v))
            .map(|i| i.envelope_release)
            .unwrap_or(0);
          let envelope_size = self
            .woices
            .get(wi)
            .and_then(|w| w.instances.get(v))
            .map(|i| i.envelope_size)
            .unwrap_or(0);
          let body_frames = self
            .woices
            .get(wi)
            .and_then(|w| w.instances.get(v))
            .map(|i| i.body_frames)
            .unwrap_or(0);
          let wave_loop = self
            .woices
            .get(wi)
            .and_then(|w| w.voices.get(v))
            .map(|vc| vc.voice_flags & VOICE_FLAG_WAVELOOP != 0)
            .unwrap_or(false);

          // Read tone's envelope release (in ticks) for life calculation
          let tone_rls_ticks = self.units[u]
            .tones
            .get(v)
            .map(|t| t.envelope_release)
            .unwrap_or(0) as i32;

          let life_count = if envelope_release > 0 {
            let max_life1 = ((ev.value - (tick - ev.tick)) as f64 * samples_per_tick) as i32
              + envelope_release as i32;
            let c_limit = ev.tick + ev.value + tone_rls_ticks;
            let mut max_life2 = sample_end as i32 - (tick as f64 * samples_per_tick) as i32;

            if let Some(ne) = self.events.records()[self.moo_event_index..]
              .iter()
              .take_while(|e| e.tick <= c_limit)
              .find(|e| e.unit_index == ev.unit_index && e.kind == EVENT_KIND_ON)
            {
              max_life2 = ((ne.tick - tick) as f64 * samples_per_tick) as i32;
            }
            max_life1.min(max_life2)
          } else {
            ((ev.value - (tick - ev.tick)) as f64 * samples_per_tick) as i32
          };

          if life_count > 0
            && let Some(tone) = self.units[u].tones.get_mut(v)
          {
            tone.on_count = on_count as u32;

            // When seeking into the middle of a note, advance sample_pos by the
            // number of samples that would have elapsed since the note's ON tick.
            // This keeps PCM/OGG voices in sync with the song position.
            if elapsed > 0 {
              let step = tone.offset_frequency as f64 * unit_tuning as f64 * unit_freq as f64;
              let initial_pos = elapsed as f64 * samples_per_tick * step;
              let body = body_frames as f64;
              if body > 0.0 && !wave_loop && initial_pos >= body {
                // Non-looping voice whose sample data is already exhausted.
                tone.life_count = 0;
                continue;
              }
              tone.sample_pos = if wave_loop && body > 0.0 {
                initial_pos % body
              } else {
                initial_pos
              };
            } else {
              tone.sample_pos = 0.0;
            }

            tone.envelope_pos = 0;
            if envelope_size > 0 {
              tone.envelope_volume = 0;
              tone.envelope_start = 0;
            } else {
              tone.envelope_volume = 128;
              tone.envelope_start = 128;
            }
            tone.life_count = life_count as u32;
          }
        }
      }
      EVENT_KIND_KEY => self.units[u].tone_key(ev.value),
      EVENT_KIND_PAN_VOLUME => {
        self.units[u].tone_pan_volume(self.dst_channels as u32, ev.value as u32)
      }
      EVENT_KIND_PAN_TIME => self.units[u].tone_pan_time(
        self.dst_channels as u32,
        ev.value as u32,
        self.dst_sample_rate,
      ),
      EVENT_KIND_VELOCITY => self.units[u].tone_velocity(ev.value as u32),
      EVENT_KIND_VOLUME => self.units[u].tone_volume(ev.value as u32),
      EVENT_KIND_PORTAMENT => {
        let v = (ev.value as f64 * samples_per_tick) as u32;
        self.units[u].tone_portament(v);
      }
      EVENT_KIND_VOICE_NO => self.moo_reset_voice_on(u, ev.value as usize),
      EVENT_KIND_GROUP_NO => self.units[u].tone_groupno(ev.value as usize),
      EVENT_KIND_TUNING => self.units[u].tone_tuning(f32::from_bits(ev.value as u32)),
      _ => {} // TICKS_PER_BEAT, BEAT_TEMPO, BEATS_PER_MEASURE, REPEAT, LAST are ignored
    }
  }

  /// Fills `buf` with the next chunk of 16-bit interleaved PCM audio.
  ///
  /// `buf` must be a multiple of `channels * 2` bytes.
  /// Returns `true` while audio is available, `false` after playback ends.
  /// Renders samples into `buf`. Returns the number of bytes actually written,
  /// which may be less than `buf.len()` at the end of the song. Returns 0 when
  /// playback has already ended or the buffer length is invalid.
  pub fn moo(&mut self, buf: &mut [u8]) -> usize {
    if !self.data_loaded {
      return 0;
    }
    if self.playback_ended {
      return 0;
    }

    let byte_per_smp = self.dst_channels as usize * 2;
    if !buf.len().is_multiple_of(byte_per_smp) {
      return 0;
    }

    // Songs that route everything through group 0 get a one-element mixer
    // accumulator. That makes the group index a constant, so the backend can
    // keep the accumulator in registers instead of on the stack — worth a
    // separate instantiation because the accumulator is touched by every unit
    // and every effect on every sample.
    if self.moo_group_count == 1 {
      self.moo_run::<1>(buf, byte_per_smp)
    } else {
      self.moo_run::<MAX_GROUP_COUNT>(buf, byte_per_smp)
    }
  }

  /// Body of [`PxtoneService::moo`], specialised on the width of the mixer
  /// accumulator. `GROUPS` must be at least [`PxtoneService::calc_group_count`].
  fn moo_run<const GROUPS: usize>(&mut self, buf: &mut [u8], byte_per_smp: usize) -> usize {
    let total = buf.len() / byte_per_smp;
    let mut pos = 0usize;

    while pos < total {
      // Number of consecutive samples that need no event/boundary check.
      let mut safe = (self.moo_safe_count() as usize).min(total - pos);

      while safe > 0 {
        let count = safe.min(MOO_BLOCK);
        self.moo_block::<GROUPS>(&mut buf[pos * byte_per_smp..], byte_per_smp, count);
        pos += count;
        safe -= count;
      }

      // Boundary sample: run with full event dispatch.
      if pos < total {
        let mut sample = [0i16; 2];
        if !self.moo_pxtone_sample(&mut sample) {
          self.playback_ended = true;
          break;
        }
        write_frame(buf, pos, byte_per_smp, sample);
        pos += 1;
      }
    }

    pos * byte_per_smp
  }

  // ---- Getters ----

  /// Returns `true` when playback has reached the end.
  #[inline]
  pub fn is_end_vomit(&self) -> bool {
    self.playback_ended
  }

  /// Returns `true` if a file has been successfully loaded.
  #[inline]
  pub fn is_valid_data(&self) -> bool {
    self.data_loaded
  }

  /// Returns the current playback position in ticks.
  #[inline]
  pub fn moo_get_now_tick(&self) -> u32 {
    if self.moo_samples_per_tick > 0.0 {
      (self.moo_sample_count as f64 / self.moo_samples_per_tick) as u32
    } else {
      0
    }
  }

  /// Returns the tick position at which playback will end.
  #[inline]
  pub fn moo_get_end_tick(&self) -> u32 {
    if self.moo_samples_per_tick > 0.0 {
      (self.moo_sample_end as f64 / self.moo_samples_per_tick) as u32
    } else {
      0
    }
  }

  /// Returns the current playback position as a sample offset.
  #[inline]
  pub fn moo_get_sampling_offset(&self) -> u32 {
    if self.playback_ended {
      0
    } else {
      self.moo_sample_count
    }
  }

  /// Returns the sample position at which playback will end.
  #[inline]
  pub fn moo_get_sampling_end(&self) -> u32 {
    if self.playback_ended {
      0
    } else {
      self.moo_sample_end
    }
  }

  /// Returns the total number of samples in the current playback session.
  #[inline]
  pub fn moo_get_total_sample(&self) -> u32 {
    self.calc_total_sample()
  }
}
