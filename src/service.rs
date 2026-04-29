use crate::effect::delay::Delay;
use crate::effect::overdrive::OverDrive;
use crate::error::PxtoneError;
use crate::event::{
  EVENT_DEFAULT_BASIC_KEY, EVENT_DEFAULT_VOICE_NO, EVENT_KIND_GROUP_NO, EVENT_KIND_KEY, EVENT_KIND_ON,
  EVENT_KIND_PAN_TIME, EVENT_KIND_PAN_VOLUME, EVENT_KIND_PORTAMENT, EVENT_KIND_TUNING,
  EVENT_KIND_VELOCITY, EVENT_KIND_VOICE_NO, EVENT_KIND_VOLUME, EventList, EventRecord,
};
use crate::master::Master;
use crate::pulse::frequency::FrequencyTable;
use crate::pulse::noise::Noise;
use crate::pulse::noise_builder::NoiseBuilder;
use crate::text::Text;
use crate::unit::Unit;
use crate::woice::{BUFSIZE_TIMEPAN, VOICE_FLAG_BEATFIT, VoiceInstance, Woice};
use byteorder::{LE, ReadBytesExt};
use std::io::{Read, Seek};

// ---- Constants ----
const MAX_UNIT_NUM: usize = 50;
const MAX_WOICE_NUM: usize = 100;
const MAX_GROUP_NUM: usize = 7;
const MAX_DELAY_NUM: usize = 4;
const MAX_OVERDRIVE_NUM: usize = 2;
const MAX_WOICE_NAME: usize = 16;
const MAX_UNIT_NAME: usize = 16;

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

// ---- PxtoneService ----

/// Decoder and playback engine for pxtone music files (`.ptcop`).
///
/// # Typical usage
///
/// ```no_run
/// use pxtone::{PxtoneService, VomitPreparation};
/// use std::fs::File;
/// use std::io::BufReader;
///
/// let mut service = PxtoneService::new();
/// let mut reader = BufReader::new(File::open("song.ptcop").unwrap());
/// service.read(&mut reader).unwrap();
/// service.tones_ready().unwrap();
/// service.moo_preparation(VomitPreparation::default()).unwrap();
///
/// let q = service.get_destination_quality();
/// let mut buf = vec![0u8; q.channels as usize * 2 * 4096];
/// while !service.is_end_vomit() {
///     service.moo(&mut buf);
///     // process buf as 16-bit LE PCM...
/// }
/// ```
pub struct PxtoneService {
  pub text: Text,
  pub master: Master,
  pub events: EventList,
  pub units: Vec<Unit>,

  pub(crate) delays: Vec<Delay>,
  pub(crate) overdrives: Vec<OverDrive>,
  pub(crate) woices: Vec<Woice>,

  noise_builder: NoiseBuilder,
  frequency: FrequencyTable,

  // Output quality
  dst_channels: u8,
  dst_sample_rate: u32,

  // moo runtime
  group_num: usize,
  unit_woice_idxs: Vec<usize>, // current voice index per unit

  moo_clock_rate: f64,
  moo_sample_stride: f32,
  moo_sample_count: u32,
  moo_sample_end: u32,
  moo_sample_repeat: u32,
  moo_sample_start: u32,
  moo_sample_smooth: u32,
  moo_output_clip: i32,
  moo_beat_clock: u16,
  moo_beat_num: u8,
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
}

impl PxtoneService {
  pub fn new() -> Self {
    Self {
      text: Text::new(),
      master: Master::new(),
      events: EventList::new(),
      woices: Vec::new(),
      units: Vec::new(),
      delays: Vec::new(),
      overdrives: Vec::new(),
      noise_builder: NoiseBuilder::new(),
      frequency: FrequencyTable::new(),

      dst_channels: 2,
      dst_sample_rate: 44100,

      group_num: MAX_GROUP_NUM,
      unit_woice_idxs: Vec::new(),

      moo_clock_rate: 0.0,
      moo_sample_stride: 1.0,
      moo_sample_count: 0,
      moo_sample_end: 0,
      moo_sample_repeat: 0,
      moo_sample_start: 0,
      moo_sample_smooth: 0,
      moo_output_clip: 0x7fff,
      moo_beat_clock: 0,
      moo_beat_num: 0,
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
    }
  }

  /// Sets the output audio quality. The default is stereo (2 ch) at 44100 Hz.
  ///
  /// Call this before [`tones_ready`](Self::tones_ready).
  #[inline]
  pub fn set_destination_quality(&mut self, quality: DestinationQuality) {
    self.dst_channels = quality.channels;
    self.dst_sample_rate = quality.sample_rate;
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
  pub fn render_noise<R: Read>(&mut self, r: &mut R) -> Result<NoiseWave, PxtoneError> {
    let mut noise = Noise::new();
    noise.read(r)?;
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

  /// Loads a `.ptcop` or `.pttune` file. Clears any previously loaded data.
  ///
  /// Call [`tones_ready`](Self::tones_ready) after loading.
  pub fn read<R: Read + Seek>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    self.clear();

    let fmt_ver = self.read_version(r)?;
    self.read_tune_items(r, fmt_ver)?;

    if matches!(fmt_ver, FmtVer::X3x | FmtVer::X2x | FmtVer::X1x) {
      self.x3x_tuning_key_event()?;
      self.x3x_add_tuning_event();
      self.x3x_set_voice_names();
    }

    let clock1 = self.events.get_max_clock() as u32;
    let clock2 = self.master.get_last_clock();
    self.master.adjust_measure_num(clock1.max(clock2));

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
  }

  /// Reads the version string and returns a FmtVer
  fn read_version<R: Read>(&self, r: &mut R) -> Result<FmtVer, PxtoneError> {
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
    let _exe_ver = r.read_u16::<LE>()?;
    let _rrr = r.read_u16::<LE>()?;

    Ok(fmt_ver)
  }

  fn read_tune_items<R: Read + Seek>(
    &mut self,
    r: &mut R,
    _fmt_ver: FmtVer,
  ) -> Result<(), PxtoneError> {
    loop {
      let mut code = [0u8; CODE_SIZE];
      r.read_exact(&mut code)?;

      match &code {
        // v5 tags
        b"num UNIT" => {
          let size = r.read_i32::<LE>()?;
          if size != 4 {
            return Err(PxtoneError::UnknownFormat);
          }
          let num = r.read_i16::<LE>()? as usize;
          let rrr = r.read_i16::<LE>()?;
          if rrr != 0 {
            return Err(PxtoneError::UnknownFormat);
          }
          if num > MAX_UNIT_NUM {
            return Err(PxtoneError::UnknownFormat);
          }
          self.units = (0..num).map(|_| Unit::new()).collect();
          self.unit_woice_idxs = vec![0usize; num];
        }
        b"MasterV5" => self.master.read_v5(r)?,
        b"Event V5" => self.events.read_v5(r)?,

        b"matePCM " | b"matePCM=" => {
          if self.woices.len() >= MAX_WOICE_NUM {
            return Err(PxtoneError::WoiceFull);
          }
          let mut w = Woice::new();
          w.read_mate_pcm(r)?;
          self.woices.push(w);
        }
        b"matePTV " => {
          if self.woices.len() >= MAX_WOICE_NUM {
            return Err(PxtoneError::WoiceFull);
          }
          let mut w = Woice::new();
          w.read_mate_ptv(r)?;
          self.woices.push(w);
        }
        b"matePTN " => {
          if self.woices.len() >= MAX_WOICE_NUM {
            return Err(PxtoneError::WoiceFull);
          }
          let mut w = Woice::new();
          w.read_mate_ptn(r)?;
          self.woices.push(w);
        }
        b"mateOGGV" => {
          if self.woices.len() >= MAX_WOICE_NUM {
            return Err(PxtoneError::WoiceFull);
          }
          let mut w = Woice::new();
          w.read_mate_oggv(r)?;
          self.woices.push(w);
        }
        b"effeDELA" => {
          if self.delays.len() >= MAX_DELAY_NUM {
            return Err(PxtoneError::UnknownFormat);
          }
          let mut d = Delay::new();
          d.read(r)?;
          self.delays.push(d);
        }
        b"effeOVER" => {
          if self.overdrives.len() >= MAX_OVERDRIVE_NUM {
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
          let _end = r.read_i32::<LE>()?; // 0
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

  fn read_assi_woic<R: Read>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let size = r.read_i32::<LE>()?;
    if size != (2 + 2 + MAX_WOICE_NAME) as i32 {
      return Err(PxtoneError::UnknownFormat);
    }
    let woice_index = r.read_u16::<LE>()? as usize;
    let rrr = r.read_u16::<LE>()?;
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

  fn read_assi_unit<R: Read>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let size = r.read_i32::<LE>()?;
    if size != (2 + 2 + MAX_UNIT_NAME) as i32 {
      return Err(PxtoneError::UnknownFormat);
    }
    let unit_index = r.read_u16::<LE>()? as usize;
    let rrr = r.read_u16::<LE>()?;
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
  fn read_old_unit_v1<R: Read>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    if self.units.len() >= MAX_UNIT_NUM {
      return Err(PxtoneError::UnknownFormat);
    }

    let _size = r.read_i32::<LE>()?;
    let mut name = [0u8; MAX_UNIT_NAME];
    r.read_exact(&mut name)?;
    let _utype = r.read_u16::<LE>()?;
    let group = r.read_u16::<LE>()? as i32;

    let u_idx = self.units.len();
    let end = name.iter().position(|&b| b == 0).unwrap_or(MAX_UNIT_NAME);
    let mut unit = Unit::new();
    unit.name = name[..end].to_vec();
    self.units.push(unit);
    self.unit_woice_idxs.push(0);

    let g = group.min(self.group_num as i32 - 1);
    self.events.add_i(0, u_idx as u8, EVENT_KIND_GROUP_NO, g);
    self
      .events
      .add_i(0, u_idx as u8, EVENT_KIND_VOICE_NO, u_idx as i32);
    Ok(())
  }

  /// Reads a v3x unit struct (size:i32 + type:u16 + group:u16)
  fn read_old_unit_v3<R: Read>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    if self.units.len() >= MAX_UNIT_NUM {
      return Err(PxtoneError::UnknownFormat);
    }

    let _size = r.read_i32::<LE>()?;
    let _utype = r.read_u16::<LE>()?;
    let group = r.read_u16::<LE>()? as i32;

    let u_idx = self.units.len();
    self.units.push(Unit::new());
    self.unit_woice_idxs.push(0);

    let g = group.min(self.group_num as i32 - 1);
    self.events.add_i(0, u_idx as u8, EVENT_KIND_GROUP_NO, g);
    self
      .events
      .add_i(0, u_idx as u8, EVENT_KIND_VOICE_NO, u_idx as i32);
    Ok(())
  }

  /// Reads x1x project info (size:i32 + name[16] + ...)
  fn read_x1x_project<R: Read>(&mut self, r: &mut R) -> Result<(), PxtoneError> {
    let _size = r.read_i32::<LE>()?;
    let mut name = [0u8; 16];
    r.read_exact(&mut name)?;
    let beat_tempo = r.read_f32::<LE>()?;
    let beat_clock = r.read_u16::<LE>()?;
    let beat_num = r.read_u16::<LE>()? as u8;
    let _beat_note = r.read_u16::<LE>()?;
    let _measure_num = r.read_u16::<LE>()?;
    let _channels = r.read_u16::<LE>()?;
    let _bits_per_sample = r.read_u16::<LE>()?;
    let _sample_rate = r.read_u32::<LE>()?;

    self.text.set_name_raw(&name);
    self.master.beat_num = beat_num;
    self.master.beat_tempo = beat_tempo;
    self.master.beat_clock = beat_clock;
    Ok(())
  }

  // ---- x3x/x2x/x1x post-processing ----

  fn x3x_tuning_key_event(&mut self) -> Result<(), PxtoneError> {
    use crate::event::EVENT_KIND_KEY;
    let unit_num = self.units.len().min(self.woices.len());
    for u in 0..unit_num {
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
    let unit_num = self.units.len().min(self.woices.len());
    for u in 0..unit_num {
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

    // noise_builder, freq, and woices are independent fields, so simultaneous borrows are OK
    let noise_builder = &mut self.noise_builder;
    let freq = &self.frequency;
    for w in &mut self.woices {
      w.tone_ready(noise_builder, freq, sample_rate)?;
    }
    for d in &mut self.delays {
      d.tone_ready(self.master.beat_num, self.master.beat_tempo, sample_rate);
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

    self.moo_beat_clock = self.master.beat_clock;
    self.moo_beat_num = self.master.beat_num;
    self.moo_beat_tempo = self.master.beat_tempo;
    self.moo_clock_rate = 60.0 * self.dst_sample_rate as f64
      / (self.moo_beat_tempo as f64 * self.moo_beat_clock as f64);
    self.moo_sample_stride = 44100.0 / self.dst_sample_rate as f32;
    self.moo_output_clip = 0x7fff;
    self.moo_time_pan_index = 0;

    let samples_per_measure =
      self.moo_beat_num as f64 * self.moo_beat_clock as f64 * self.moo_clock_rate;
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

    self.tones_clear();
    self.moo_event_index = 0;
    self.moo_init_unit_tone();
    self.playback_ended = false;
    Ok(())
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
    let total_beats = self.master.measure_num * self.master.beat_num as u32;
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
    let voice_num = self.woices[woice_idx].voices.len();
    let voice_flags: Vec<u32> = self.woices[woice_idx]
      .voices
      .iter()
      .map(|v| v.voice_flags)
      .collect();

    self.units[unit_idx].set_woice(voice_num, voice_flags);

    if unit_idx < self.unit_woice_idxs.len() {
      self.unit_woice_idxs[unit_idx] = woice_idx;
    }

    // Compute ofs_freq and env_release_clock for each voice, then reset
    let clock_rate = self.moo_clock_rate;
    let bt_tempo = self.moo_beat_tempo;
    let inst_len = self.woices[woice_idx].instances.len();

    for v in 0..voice_num.min(inst_len) {
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

      let env_rls_clock = if clock_rate > 0.0 {
        (envelope_release as f64 / clock_rate) as u32
      } else {
        0
      };

      self.units[unit_idx].tone_reset_and_2prm(v, env_rls_clock, ofs_freq);
    }
  }

  fn moo_init_unit_tone(&mut self) {
    for u in 0..self.units.len() {
      self.units[u].tone_init();
      self.moo_reset_voice_on(u, EVENT_DEFAULT_VOICE_NO);
    }
  }

  /// Synthesizes one sample and writes it into `out[0..channels]`.
  /// Returns `true` while playing, `false` when the end is reached.
  fn moo_pxtone_sample(&mut self, out: &mut [i16; 2]) -> bool {
    let unit_num = self.units.len();
    let group_num = self.group_num;
    let channel_count = self.dst_channels as usize;
    let clock_rate = self.moo_clock_rate;
    let sample_count = self.moo_sample_count;
    let mute_by_unit = self.moo_mute_by_unit;
    let smooth_samples = self.moo_sample_smooth;
    let time_pan_idx = self.moo_time_pan_index;
    let sample_end = self.moo_sample_end;
    let sample_stride = self.moo_sample_stride;

    // ---- 1. Envelope processing ----
    for u in 0..unit_num {
      let wi = self.unit_woice_idxs.get(u).copied().unwrap_or(0);
      if let Some(woice) = self.woices.get(wi) {
        // SAFETY: woices[wi] and units[u] are independent elements
        let instances = woice.instances.as_slice() as *const [VoiceInstance];
        let instances = unsafe { &*instances };
        self.units[u].tone_envelope(instances);
      }
    }

    // ---- 2. Event processing ----
    let clock = (sample_count as f64 / clock_rate) as i32;
    let event_count = self.events.records().len();

    while self.moo_event_index < event_count {
      let ev_clock = self.events.records()[self.moo_event_index].clock;
      if ev_clock > clock {
        break;
      }

      // Clone the event before advancing the index
      let ev: EventRecord = self.events.records()[self.moo_event_index].clone();
      self.moo_event_index += 1;

      let u = ev.unit_index as usize;
      if u >= self.units.len() {
        continue;
      }

      self.process_event(&ev, u, clock, sample_end, clock_rate);
    }

    // ---- 3. Tone_Sample ----
    for u in 0..unit_num {
      let wi = self.unit_woice_idxs.get(u).copied().unwrap_or(0);
      if let Some(woice) = self.woices.get(wi) {
        let instances = woice.instances.as_slice() as *const [VoiceInstance];
        let instances = unsafe { &*instances };
        self.units[u].tone_sample(
          mute_by_unit,
          self.dst_channels,
          time_pan_idx,
          smooth_samples,
          instances,
        );
      }
    }

    // ---- 4. Per-channel group sum → effects → output ----
    let mut group_smps = vec![0i32; group_num];

    for (ch, out_sample) in out.iter_mut().enumerate().take(channel_count) {
      group_smps.fill(0);

      for u in 0..unit_num {
        self.units[u].tone_supple(&mut group_smps, ch, time_pan_idx);
      }
      for od in &self.overdrives {
        od.tone_supple(&mut group_smps);
      }
      for d in &mut self.delays {
        d.tone_supple(ch, &mut group_smps);
      }

      let mut work: i32 = group_smps.iter().sum();

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

    // ---- 5. Increment ----
    self.moo_sample_count += 1;
    self.moo_time_pan_index = (self.moo_time_pan_index + 1) & (BUFSIZE_TIMEPAN - 1);

    for u in 0..unit_num {
      let key = self.units[u].tone_increment_key();
      let freq = self.frequency.get2(key) * sample_stride;
      let wi = self.unit_woice_idxs.get(u).copied().unwrap_or(0);
      if let Some(woice) = self.woices.get(wi) {
        let instances = woice.instances.as_slice() as *const [VoiceInstance];
        let instances = unsafe { &*instances };
        self.units[u].tone_increment_sample(freq, instances);
      }
    }

    for d in &mut self.delays {
      d.tone_increment();
    }

    // ---- 6. Fade processing ----
    if self.moo_fade_direction < 0 {
      if self.moo_fade_count > 0 {
        self.moo_fade_count -= 1;
      } else {
        return false;
      }
    } else if self.moo_fade_direction > 0 {
      if self.moo_fade_count < (self.moo_fade_max << 8) {
        self.moo_fade_count += 1;
      } else {
        self.moo_fade_direction = 0;
      }
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

  /// Processes one event
  fn process_event(
    &mut self,
    ev: &EventRecord,
    u: usize,
    clock: i32,
    sample_end: u32,
    clock_rate: f64,
  ) {
    match ev.kind {
      EVENT_KIND_ON => {
        let on_count = ((ev.clock + ev.value - clock) as f64 * clock_rate) as i32;
        if on_count <= 0 {
          self.units[u].tone_zero_lives();
          return;
        }
        self.units[u].tone_key_on();

        let wi = self.unit_woice_idxs.get(u).copied().unwrap_or(0);
        let voice_num = self.woices.get(wi).map(|w| w.voices.len()).unwrap_or(0);

        for v in 0..voice_num {
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

          // Read env_release_clock from tones (immutable borrow of self.units)
          let tone_rls_clock = self.units[u]
            .tones
            .get(v)
            .map(|t| t.envelope_release)
            .unwrap_or(0) as i32;

          let life_count = if envelope_release > 0 {
            let max_life1 = ((ev.value - (clock - ev.clock)) as f64 * clock_rate) as i32
              + envelope_release as i32;
            let c_limit = ev.clock + ev.value + tone_rls_clock;
            let mut max_life2 = sample_end as i32 - (clock as f64 * clock_rate) as i32;

            if let Some(ne) = self.events.records()[self.moo_event_index..]
              .iter()
              .take_while(|e| e.clock <= c_limit)
              .find(|e| e.unit_index == ev.unit_index && e.kind == EVENT_KIND_ON)
            {
              max_life2 = ((ne.clock - clock) as f64 * clock_rate) as i32;
            }
            max_life1.min(max_life2)
          } else {
            ((ev.value - (clock - ev.clock)) as f64 * clock_rate) as i32
          };

          if life_count > 0
            && let Some(tone) = self.units[u].tones.get_mut(v)
          {
            tone.on_count = on_count as u32;
            tone.sample_pos = 0.0;
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
        let v = (ev.value as f64 * clock_rate) as u32;
        self.units[u].tone_portament(v);
      }
      EVENT_KIND_VOICE_NO => self.moo_reset_voice_on(u, ev.value as usize),
      EVENT_KIND_GROUP_NO => self.units[u].tone_groupno(ev.value as usize),
      EVENT_KIND_TUNING => self.units[u].tone_tuning(f32::from_bits(ev.value as u32)),
      _ => {} // BEATCLOCK, BEATTEMPO, BEATNUM, REPEAT, LAST are ignored
    }
  }

  /// Fills `buf` with the next chunk of 16-bit interleaved PCM audio.
  ///
  /// `buf` must be a multiple of `channels * 2` bytes.
  /// Returns `true` while audio is available, `false` after playback ends.
  pub fn moo(&mut self, buf: &mut [u8]) -> bool {
    if !self.data_loaded {
      return false;
    }
    if self.playback_ended {
      return false;
    }

    let byte_per_smp = self.dst_channels as usize * 2;
    if !buf.len().is_multiple_of(byte_per_smp) {
      return false;
    }
    let _smp_num = buf.len() / byte_per_smp;

    let mut smp_written = 0usize;
    for chunk in buf.chunks_exact_mut(byte_per_smp) {
      let mut sample = [0i16; 2];
      if !self.moo_pxtone_sample(&mut sample) {
        self.playback_ended = true;
        break;
      }
      for (ch_bytes, &s) in chunk.chunks_exact_mut(2).zip(sample.iter()) {
        ch_bytes.copy_from_slice(&s.to_le_bytes());
      }
      smp_written += 1;
    }

    // Zero-fill the remainder
    let start = smp_written * byte_per_smp;
    if start < buf.len() {
      buf[start..].fill(0);
    }

    true
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
  pub fn moo_get_now_clock(&self) -> u32 {
    if self.moo_clock_rate > 0.0 {
      (self.moo_sample_count as f64 / self.moo_clock_rate) as u32
    } else {
      0
    }
  }

  /// Returns the tick position at which playback will end.
  #[inline]
  pub fn moo_get_end_clock(&self) -> u32 {
    if self.moo_clock_rate > 0.0 {
      (self.moo_sample_end as f64 / self.moo_clock_rate) as u32
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

impl Default for PxtoneService {
  fn default() -> Self {
    Self::new()
  }
}
