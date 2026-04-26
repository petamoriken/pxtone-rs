use crate::effect::delay::Delay;
use crate::effect::overdrive::OverDrive;
use crate::error::PxtoneError;
use crate::event::{
  EVENTDEFAULT_BASICKEY, EVENTDEFAULT_VOICENO, EVENTKIND_GROUPNO, EVENTKIND_KEY, EVENTKIND_ON,
  EVENTKIND_PAN_TIME, EVENTKIND_PAN_VOLUME, EVENTKIND_PORTAMENT, EVENTKIND_TUNING,
  EVENTKIND_VELOCITY, EVENTKIND_VOICENO, EVENTKIND_VOLUME, EventList, EventRecord,
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
  pub ch_num: u8,
  pub sps: u32,
}

impl Default for DestinationQuality {
  fn default() -> Self {
    Self {
      ch_num: 2,
      sps: 44100,
    }
  }
}

/// Rendered audio returned by [`PxtoneService::render_noise`].
pub struct NoiseWave {
  pub samples: Vec<u8>,
  pub ch_num: u8,
  pub sps: u32,
}

/// Flag constants for [`VomitPreparation::flags`].
pub struct VomitPrepFlags;

impl VomitPrepFlags {
  pub const UNIT_MUTE: u8 = 0x1;
  pub const LOOP: u8 = 0x2;
}

/// Playback settings passed to [`PxtoneService::moo_preparation`].
#[derive(Clone)]
pub struct VomitPreparation {
  pub flags: u8,
  pub start_pos_meas: i32,
  pub start_pos_sample: i32,
  pub start_pos_float: f32,
  pub meas_end: Option<i32>,
  pub meas_repeat: Option<i32>,
  pub fadein_sec: f32,
  pub master_volume: f32,
}

impl Default for VomitPreparation {
  fn default() -> Self {
    Self {
      flags: 0,
      start_pos_meas: 0,
      start_pos_sample: 0,
      start_pos_float: 0.0,
      meas_end: None,
      meas_repeat: None,
      fadein_sec: 0.0,
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
/// let mut buf = vec![0u8; q.ch_num as usize * 2 * 4096];
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
  freq: FrequencyTable,

  // Output quality
  dst_ch_num: u8,
  dst_sps: u32,

  // moo runtime
  group_num: usize,
  unit_woice_idxs: Vec<usize>, // current voice index per unit

  moo_clock_rate: f64,
  moo_smp_stride: f32,
  moo_smp_count: u32,
  moo_smp_end: u32,
  moo_smp_repeat: u32,
  moo_smp_start: u32,
  moo_smp_smooth: u32,
  moo_top: i32,
  moo_bt_clock: u16,
  moo_bt_num: u8,
  moo_bt_tempo: f32,
  moo_time_pan_index: usize,
  moo_eve_idx: usize,
  moo_b_loop: bool,
  moo_b_mute_by_unit: bool,
  moo_master_vol: f32,
  moo_fade_fade: i32,
  moo_fade_count: u32,
  moo_fade_max: u32,

  b_valid_data: bool,
  b_end_vomit: bool,
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
      freq: FrequencyTable::new(),

      dst_ch_num: 2,
      dst_sps: 44100,

      group_num: MAX_GROUP_NUM,
      unit_woice_idxs: Vec::new(),

      moo_clock_rate: 0.0,
      moo_smp_stride: 1.0,
      moo_smp_count: 0,
      moo_smp_end: 0,
      moo_smp_repeat: 0,
      moo_smp_start: 0,
      moo_smp_smooth: 0,
      moo_top: 0x7fff,
      moo_bt_clock: 0,
      moo_bt_num: 0,
      moo_bt_tempo: 0.0,
      moo_time_pan_index: 0,
      moo_eve_idx: 0,
      moo_b_loop: true,
      moo_b_mute_by_unit: false,
      moo_master_vol: 1.0,
      moo_fade_fade: 0,
      moo_fade_count: 0,
      moo_fade_max: 0,

      b_valid_data: false,
      b_end_vomit: true,
    }
  }

  /// Sets the output audio quality. The default is stereo (2 ch) at 44100 Hz.
  ///
  /// Call this before [`tones_ready`](Self::tones_ready).
  #[inline]
  pub fn set_destination_quality(&mut self, quality: DestinationQuality) {
    self.dst_ch_num = quality.ch_num;
    self.dst_sps = quality.sps;
  }

  /// Returns the current output audio quality.
  #[inline]
  pub fn get_destination_quality(&self) -> DestinationQuality {
    DestinationQuality {
      ch_num: self.dst_ch_num,
      sps: self.dst_sps,
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
      self.dst_ch_num as usize,
      self.dst_sps,
      16,
      &self.freq,
    )?;
    Ok(NoiseWave {
      samples: pcm.samples().to_vec(),
      ch_num: self.dst_ch_num,
      sps: self.dst_sps,
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
    self.master.adjust_meas_num(clock1.max(clock2));

    self.b_valid_data = true;
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
    self.b_valid_data = false;
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
    self.units[unit_index].name = String::from_utf8_lossy(&name[..end]).into_owned();
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
    unit.name = String::from_utf8_lossy(&name[..end]).into_owned();
    self.units.push(unit);
    self.unit_woice_idxs.push(0);

    let g = group.min(self.group_num as i32 - 1);
    self.events.add_i(0, u_idx as u8, EVENTKIND_GROUPNO, g);
    self
      .events
      .add_i(0, u_idx as u8, EVENTKIND_VOICENO, u_idx as i32);
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
    self.events.add_i(0, u_idx as u8, EVENTKIND_GROUPNO, g);
    self
      .events
      .add_i(0, u_idx as u8, EVENTKIND_VOICENO, u_idx as i32);
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
    let _meas_num = r.read_u16::<LE>()?;
    let _ch_num = r.read_u16::<LE>()?;
    let _bps = r.read_u16::<LE>()?;
    let _sps = r.read_u32::<LE>()?;

    self.text.set_name_raw(&name);
    self.master.beat_num = beat_num;
    self.master.beat_tempo = beat_tempo;
    self.master.beat_clock = beat_clock;
    Ok(())
  }

  // ---- x3x/x2x/x1x post-processing ----

  fn x3x_tuning_key_event(&mut self) -> Result<(), PxtoneError> {
    use crate::event::EVENTKIND_KEY;
    let unit_num = self.units.len().min(self.woices.len());
    for u in 0..unit_num {
      let change = self.woices[u].x3x_basic_key - EVENTDEFAULT_BASICKEY;
      let has_key = self
        .events
        .records()
        .iter()
        .any(|e| e.unit_no == u as u8 && e.kind == EVENTKIND_KEY);
      if !has_key {
        self.events.add_i(0, u as u8, EVENTKIND_KEY, 0x6000);
      }
      self
        .events
        .value_change(0, -1, u as u8, EVENTKIND_KEY, change);
    }
    Ok(())
  }

  fn x3x_add_tuning_event(&mut self) {
    let unit_num = self.units.len().min(self.woices.len());
    for u in 0..unit_num {
      let tuning = self.woices[u].x3x_tuning;
      if tuning != 0.0 {
        self.events.add_f(0, u as u8, EVENTKIND_TUNING, tuning);
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
    let sps = self.dst_sps;

    // noise_builder, freq, and woices are independent fields, so simultaneous borrows are OK
    let noise_builder = &mut self.noise_builder;
    let freq = &self.freq;
    for w in &mut self.woices {
      w.tone_ready(noise_builder, freq, sps)?;
    }
    for d in &mut self.delays {
      d.tone_ready(self.master.beat_num, self.master.beat_tempo, sps);
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
    if !self.b_valid_data || self.dst_ch_num == 0 || self.dst_sps == 0 {
      self.b_end_vomit = true;
      return Err(PxtoneError::Init);
    }

    let start_meas = prep.start_pos_meas;
    let start_sample = prep.start_pos_sample;
    let start_float = prep.start_pos_float;
    let meas_end = prep
      .meas_end
      .unwrap_or_else(|| self.master.get_play_meas() as i32);
    let meas_repeat = prep.meas_repeat.unwrap_or(self.master.repeat_meas as i32);
    let fadein_sec = prep.fadein_sec;
    self.moo_b_mute_by_unit = prep.flags & VomitPrepFlags::UNIT_MUTE != 0;
    self.moo_b_loop = prep.flags & VomitPrepFlags::LOOP != 0;
    self.moo_master_vol = prep.master_volume;

    self.moo_bt_clock = self.master.beat_clock;
    self.moo_bt_num = self.master.beat_num;
    self.moo_bt_tempo = self.master.beat_tempo;
    self.moo_clock_rate =
      60.0 * self.dst_sps as f64 / (self.moo_bt_tempo as f64 * self.moo_bt_clock as f64);
    self.moo_smp_stride = 44100.0 / self.dst_sps as f32;
    self.moo_top = 0x7fff;
    self.moo_time_pan_index = 0;

    let bt = self.moo_bt_num as f64 * self.moo_bt_clock as f64 * self.moo_clock_rate;
    self.moo_smp_end = (meas_end as f64 * bt) as u32;
    self.moo_smp_repeat = (meas_repeat as f64 * bt) as u32;

    self.moo_smp_start = if start_float != 0.0 {
      let total = self.calc_total_sample();
      (total as f32 * start_float) as u32
    } else if start_sample != 0 {
      start_sample.max(0) as u32
    } else {
      (start_meas as f64 * bt) as u32
    };

    self.moo_smp_count = self.moo_smp_start;
    self.moo_smp_smooth = self.dst_sps / 250;

    if fadein_sec > 0.0 {
      self.moo_set_fade(1, fadein_sec);
    } else {
      self.moo_set_fade(0, 0.0);
    }

    self.tones_clear();
    self.moo_eve_idx = 0;
    self.moo_init_unit_tone();
    self.b_end_vomit = false;
    Ok(())
  }

  fn moo_set_fade(&mut self, fade: i32, sec: f32) {
    self.moo_fade_max = ((self.dst_sps as f32 * sec) as u32) >> 8;
    if fade < 0 {
      self.moo_fade_fade = -1;
      self.moo_fade_count = self.moo_fade_max << 8;
    } else if fade > 0 {
      self.moo_fade_fade = 1;
      self.moo_fade_count = 0;
    } else {
      self.moo_fade_fade = 0;
      self.moo_fade_count = 0;
    }
  }

  fn calc_total_sample(&self) -> u32 {
    let tempo = self.master.beat_tempo;
    if tempo == 0.0 {
      return 0;
    }
    let total_beats = self.master.meas_num * self.master.beat_num as u32;
    (self.dst_sps as f64 * 60.0 * total_beats as f64 / tempo as f64) as u32
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
    let bt_tempo = self.moo_bt_tempo;
    let inst_len = self.woices[woice_idx].instances.len();

    for v in 0..voice_num.min(inst_len) {
      let vc = &self.woices[woice_idx].voices[v];
      let inst = &self.woices[woice_idx].instances[v];
      let smp_body_w = inst.smp_body_w;
      let env_release = inst.env_release;
      let basic_key = vc.basic_key;
      let tuning = vc.tuning;
      let beat_fit = vc.voice_flags & VOICE_FLAG_BEATFIT != 0;

      let ofs_freq = if beat_fit {
        if tuning != 0.0 {
          (smp_body_w as f32 * bt_tempo) / (44100.0 * 60.0 * tuning)
        } else {
          0.0
        }
      } else {
        self.freq.get(EVENTDEFAULT_BASICKEY - basic_key) * tuning
      };

      let env_rls_clock = if clock_rate > 0.0 {
        (env_release as f64 / clock_rate) as u32
      } else {
        0
      };

      self.units[unit_idx].tone_reset_and_2prm(v, env_rls_clock, ofs_freq);
    }
  }

  fn moo_init_unit_tone(&mut self) {
    let unit_num = self.units.len();
    for u in 0..unit_num {
      self.units[u].tone_init();
      self.moo_reset_voice_on(u, EVENTDEFAULT_VOICENO);
    }
  }

  /// Synthesizes one sample and writes it into `out[0..ch_num]`.
  /// Returns `true` while playing, `false` when the end is reached.
  fn moo_pxtone_sample(&mut self, out: &mut [i16; 2]) -> bool {
    let unit_num = self.units.len();
    let group_num = self.group_num;
    let ch_num = self.dst_ch_num as usize;
    let clock_rate = self.moo_clock_rate;
    let smp_count = self.moo_smp_count;
    let b_mute = self.moo_b_mute_by_unit;
    let smp_smooth = self.moo_smp_smooth;
    let time_pan_idx = self.moo_time_pan_index;
    let smp_end = self.moo_smp_end;
    let smp_stride = self.moo_smp_stride;

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
    let clock = (smp_count as f64 / clock_rate) as i32;
    let event_count = self.events.records().len();

    while self.moo_eve_idx < event_count {
      let ev_clock = self.events.records()[self.moo_eve_idx].clock;
      if ev_clock > clock {
        break;
      }

      // Clone the event before advancing the index
      let ev: EventRecord = self.events.records()[self.moo_eve_idx].clone();
      self.moo_eve_idx += 1;

      let u = ev.unit_no as usize;
      if u >= self.units.len() {
        continue;
      }

      self.process_event(&ev, u, clock, smp_end, clock_rate, event_count);
    }

    // ---- 3. Tone_Sample ----
    for u in 0..unit_num {
      let wi = self.unit_woice_idxs.get(u).copied().unwrap_or(0);
      if let Some(woice) = self.woices.get(wi) {
        let instances = woice.instances.as_slice() as *const [VoiceInstance];
        let instances = unsafe { &*instances };
        self.units[u].tone_sample(b_mute, self.dst_ch_num, time_pan_idx, smp_smooth, instances);
      }
    }

    // ---- 4. Per-channel group sum → effects → output ----
    let mut group_smps = vec![0i32; group_num];

    for (ch, out_sample) in out.iter_mut().enumerate().take(ch_num) {
      for smp in group_smps.iter_mut().take(group_num) {
        *smp = 0;
      }

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
      if self.moo_fade_fade != 0 && self.moo_fade_max != 0 {
        work = work * (self.moo_fade_count >> 8) as i32 / self.moo_fade_max as i32;
      }

      // Master volume
      work = (work as f32 * self.moo_master_vol) as i32;

      // Clip
      work = work.clamp(-self.moo_top, self.moo_top);
      *out_sample = work as i16;
    }

    // ---- 5. Increment ----
    self.moo_smp_count += 1;
    self.moo_time_pan_index = (self.moo_time_pan_index + 1) & (BUFSIZE_TIMEPAN - 1);

    for u in 0..unit_num {
      let key_now = self.units[u].tone_increment_key();
      let freq = self.freq.get2(key_now) * smp_stride;
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
    if self.moo_fade_fade < 0 {
      if self.moo_fade_count > 0 {
        self.moo_fade_count -= 1;
      } else {
        return false;
      }
    } else if self.moo_fade_fade > 0 {
      if self.moo_fade_count < (self.moo_fade_max << 8) {
        self.moo_fade_count += 1;
      } else {
        self.moo_fade_fade = 0;
      }
    }

    // ---- 7. Loop / end-of-stream check ----
    if self.moo_smp_count >= self.moo_smp_end {
      if !self.moo_b_loop {
        return false;
      }
      self.moo_smp_count = self.moo_smp_repeat;
      self.moo_eve_idx = 0;
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
    smp_end: u32,
    clock_rate: f64,
    event_count: usize,
  ) {
    match ev.kind {
      EVENTKIND_ON => {
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
          let env_release = self
            .woices
            .get(wi)
            .and_then(|w| w.instances.get(v))
            .map(|i| i.env_release)
            .unwrap_or(0);
          let env_size = self
            .woices
            .get(wi)
            .and_then(|w| w.instances.get(v))
            .map(|i| i.env_size)
            .unwrap_or(0);

          // Read env_release_clock from tones (immutable borrow of self.units)
          let tone_rls_clock = self.units[u]
            .tones
            .get(v)
            .map(|t| t.env_release_clock)
            .unwrap_or(0) as i32;

          let life_count = if env_release > 0 {
            let max_life1 =
              ((ev.value - (clock - ev.clock)) as f64 * clock_rate) as i32 + env_release as i32;
            let c_limit = ev.clock + ev.value + tone_rls_clock;
            let mut max_life2 = smp_end as i32 - (clock as f64 * clock_rate) as i32;

            for ne_idx in self.moo_eve_idx..event_count {
              let ne_clock = self.events.records()[ne_idx].clock;
              let ne_unit = self.events.records()[ne_idx].unit_no;
              let ne_kind = self.events.records()[ne_idx].kind;
              if ne_clock > c_limit {
                break;
              }
              if ne_unit == ev.unit_no && ne_kind == EVENTKIND_ON {
                max_life2 = ((ne_clock - clock) as f64 * clock_rate) as i32;
                break;
              }
            }
            max_life1.min(max_life2)
          } else {
            ((ev.value - (clock - ev.clock)) as f64 * clock_rate) as i32
          };

          if life_count > 0
            && let Some(tone) = self.units[u].tones.get_mut(v)
          {
            tone.on_count = on_count as u32;
            tone.smp_pos = 0.0;
            tone.env_pos = 0;
            if env_size > 0 {
              tone.env_volume = 0;
              tone.env_start = 0;
            } else {
              tone.env_volume = 128;
              tone.env_start = 128;
            }
            tone.life_count = life_count as u32;
          }
        }
      }
      EVENTKIND_KEY => self.units[u].tone_key(ev.value),
      EVENTKIND_PAN_VOLUME => {
        self.units[u].tone_pan_volume(self.dst_ch_num as u32, ev.value as u32)
      }
      EVENTKIND_PAN_TIME => {
        self.units[u].tone_pan_time(self.dst_ch_num as u32, ev.value as u32, self.dst_sps)
      }
      EVENTKIND_VELOCITY => self.units[u].tone_velocity(ev.value as u32),
      EVENTKIND_VOLUME => self.units[u].tone_volume(ev.value as u32),
      EVENTKIND_PORTAMENT => {
        let v = (ev.value as f64 * clock_rate) as u32;
        self.units[u].tone_portament(v);
      }
      EVENTKIND_VOICENO => self.moo_reset_voice_on(u, ev.value as usize),
      EVENTKIND_GROUPNO => self.units[u].tone_groupno(ev.value as usize),
      EVENTKIND_TUNING => self.units[u].tone_tuning(f32::from_bits(ev.value as u32)),
      _ => {} // BEATCLOCK, BEATTEMPO, BEATNUM, REPEAT, LAST are ignored
    }
  }

  /// Fills `buf` with the next chunk of 16-bit interleaved PCM audio.
  ///
  /// `buf` must be a multiple of `ch_num * 2` bytes.
  /// Returns `true` while audio is available, `false` after playback ends.
  pub fn moo(&mut self, buf: &mut [u8]) -> bool {
    if !self.b_valid_data {
      return false;
    }
    if self.b_end_vomit {
      return false;
    }

    let byte_per_smp = self.dst_ch_num as usize * 2;
    if !buf.len().is_multiple_of(byte_per_smp) {
      return false;
    }
    let _smp_num = buf.len() / byte_per_smp;
    let ch_num = self.dst_ch_num as usize;

    let mut smp_written = 0usize;
    for chunk in buf.chunks_exact_mut(byte_per_smp) {
      let mut sample = [0i16; 2];
      if !self.moo_pxtone_sample(&mut sample) {
        self.b_end_vomit = true;
        break;
      }
      for ch in 0..ch_num {
        let bytes = sample[ch].to_le_bytes();
        chunk[ch * 2] = bytes[0];
        chunk[ch * 2 + 1] = bytes[1];
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
    self.b_end_vomit
  }

  /// Returns `true` if a file has been successfully loaded.
  #[inline]
  pub fn is_valid_data(&self) -> bool {
    self.b_valid_data
  }

  /// Returns the current playback position in ticks.
  #[inline]
  pub fn moo_get_now_clock(&self) -> i32 {
    if self.moo_clock_rate > 0.0 {
      (self.moo_smp_count as f64 / self.moo_clock_rate) as i32
    } else {
      0
    }
  }

  /// Returns the tick position at which playback will end.
  #[inline]
  pub fn moo_get_end_clock(&self) -> i32 {
    if self.moo_clock_rate > 0.0 {
      (self.moo_smp_end as f64 / self.moo_clock_rate) as i32
    } else {
      0
    }
  }

  /// Returns the current playback position as a sample offset.
  #[inline]
  pub fn moo_get_sampling_offset(&self) -> u32 {
    if self.b_end_vomit {
      0
    } else {
      self.moo_smp_count
    }
  }

  /// Returns the sample position at which playback will end.
  #[inline]
  pub fn moo_get_sampling_end(&self) -> u32 {
    if self.b_end_vomit {
      0
    } else {
      self.moo_smp_end
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
