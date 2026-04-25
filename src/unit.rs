use crate::event::{
  EVENTDEFAULT_GROUPNO, EVENTDEFAULT_KEY, EVENTDEFAULT_TUNING, EVENTDEFAULT_VELOCITY,
  EVENTDEFAULT_VOLUME,
};
use crate::woice::{BUFSIZE_TIMEPAN, VOICE_FLAG_SMOOTH, VOICE_FLAG_WAVELOOP, VoiceInstance};

pub const MAX_CHANNEL: usize = 2;
pub const MAX_UNIT_CONTROL_VOICE: usize = 2;

/// Per-unit voice tone state
#[derive(Clone, Default)]
pub struct VoiceTone {
  pub(crate) smp_pos: f64,
  pub(crate) offset_freq: f32,
  pub(crate) env_volume: i32,
  pub(crate) life_count: u32,
  pub(crate) on_count: u32,
  pub(crate) env_start: i32,
  pub(crate) env_pos: u32,
  pub(crate) env_release_clock: u32,
  pub(crate) smooth_volume: u32,
}

/// Unit (playback state)
pub struct Unit {
  pub(crate) played: bool,
  pub(crate) name: String,

  // Key state
  pub(crate) key_now: i32,
  pub(crate) key_start: i32,
  pub(crate) key_margin: i32,
  pub(crate) portament_sample_pos: u32,
  pub(crate) portament_sample_num: u32,

  // Pan
  pub(crate) pan_vols: [u32; MAX_CHANNEL],
  pub(crate) pan_times: [u32; MAX_CHANNEL],
  pub(crate) pan_time_bufs: [[i32; BUFSIZE_TIMEPAN]; MAX_CHANNEL],

  // Velocity, volume, etc.
  pub(crate) v_volume: u32,
  pub(crate) v_velocity: u32,
  pub(crate) v_groupno: usize,
  pub(crate) v_tuning: f32,

  // Voice references (one per instance)
  pub(crate) voice_num: usize,
  pub(crate) voice_flags: Vec<u32>,
  pub(crate) tones: [VoiceTone; MAX_UNIT_CONTROL_VOICE],
}

impl Default for Unit {
  fn default() -> Self {
    Self {
      played: true,
      name: "no name".to_string(),
      key_now: EVENTDEFAULT_KEY,
      key_start: EVENTDEFAULT_KEY,
      key_margin: 0,
      portament_sample_pos: 0,
      portament_sample_num: 0,
      pan_vols: [64; MAX_CHANNEL],
      pan_times: [0; MAX_CHANNEL],
      pan_time_bufs: [[0; BUFSIZE_TIMEPAN]; MAX_CHANNEL],
      v_volume: EVENTDEFAULT_VOLUME,
      v_velocity: EVENTDEFAULT_VELOCITY,
      v_groupno: EVENTDEFAULT_GROUPNO,
      v_tuning: EVENTDEFAULT_TUNING,
      voice_num: 0,
      voice_flags: Vec::new(),
      tones: Default::default(),
    }
  }
}

impl Unit {
  pub(crate) fn new() -> Self {
    Self::default()
  }

  pub(crate) fn tone_init(&mut self) {
    self.v_groupno = EVENTDEFAULT_GROUPNO;
    self.v_velocity = EVENTDEFAULT_VELOCITY;
    self.v_volume = EVENTDEFAULT_VOLUME;
    self.v_tuning = EVENTDEFAULT_TUNING;
    self.portament_sample_num = 0;
    self.portament_sample_pos = 0;
    for i in 0..MAX_CHANNEL {
      self.pan_vols[i] = 64;
      self.pan_times[i] = 0;
    }
  }

  pub(crate) fn tone_clear(&mut self) {
    for ch in 0..MAX_CHANNEL {
      for v in &mut self.pan_time_bufs[ch] {
        *v = 0;
      }
    }
  }

  pub(crate) fn tone_reset_and_2prm(
    &mut self,
    voice_idx: usize,
    env_rls_clock: u32,
    offset_freq: f32,
  ) {
    let t = &mut self.tones[voice_idx];
    t.life_count = 0;
    t.on_count = 0;
    t.smp_pos = 0.0;
    t.smooth_volume = 0;
    t.env_release_clock = env_rls_clock;
    t.offset_freq = offset_freq;
  }

  pub(crate) fn set_woice(&mut self, voice_num: usize, voice_flags: Vec<u32>) {
    self.voice_num = voice_num;
    self.voice_flags = voice_flags;
    self.key_now = EVENTDEFAULT_KEY;
    self.key_margin = 0;
    self.key_start = EVENTDEFAULT_KEY;
  }

  pub(crate) fn tone_zero_lives(&mut self) {
    for i in 0..MAX_UNIT_CONTROL_VOICE {
      self.tones[i].life_count = 0;
    }
  }

  pub(crate) fn tone_key_on(&mut self) {
    self.key_now = self.key_start + self.key_margin;
    self.key_start = self.key_now;
    self.key_margin = 0;
  }

  pub(crate) fn tone_key(&mut self, key: i32) {
    self.key_start = self.key_now;
    self.key_margin = key - self.key_start;
    self.portament_sample_pos = 0;
  }

  pub(crate) fn tone_pan_volume(&mut self, ch: u32, pan: u32) {
    self.pan_vols[0] = 64;
    self.pan_vols[1] = 64;
    if ch == 2 {
      if pan >= 64 {
        self.pan_vols[0] = 128 - pan;
      } else {
        self.pan_vols[1] = pan;
      }
    }
  }

  pub(crate) fn tone_pan_time(&mut self, ch: u32, pan: u32, sps: u32) {
    self.pan_times[0] = 0;
    self.pan_times[1] = 0;
    if ch == 2 {
      if pan >= 64 {
        let v = (pan - 64).min(63);
        self.pan_times[0] = v * 44100 / sps;
      } else {
        let v = (64 - pan).min(63);
        self.pan_times[1] = v * 44100 / sps;
      }
    }
  }

  pub(crate) fn tone_velocity(&mut self, val: u32) {
    self.v_velocity = val;
  }
  pub(crate) fn tone_volume(&mut self, val: u32) {
    self.v_volume = val;
  }
  pub(crate) fn tone_portament(&mut self, val: u32) {
    self.portament_sample_num = val;
  }
  pub(crate) fn tone_groupno(&mut self, val: usize) {
    self.v_groupno = val;
  }
  pub(crate) fn tone_tuning(&mut self, val: f32) {
    self.v_tuning = val;
  }

  pub(crate) fn tone_envelope(&mut self, instances: &[VoiceInstance]) {
    for v in 0..self.voice_num {
      let vi = &instances[v];
      let vt = &mut self.tones[v];
      if vt.life_count > 0 && vi.env_size > 0 {
        if vt.on_count > 0 {
          if vt.env_pos < vi.env_size {
            vt.env_volume = vi.env[vt.env_pos as usize] as i32;
            vt.env_pos += 1;
          }
        } else {
          // release
          vt.env_volume =
            vt.env_start + (0 - vt.env_start) * vt.env_pos as i32 / vi.env_release.max(1) as i32;
          vt.env_pos += 1;
        }
      }
    }
  }

  /// Generates samples and writes them into pan_time_bufs
  pub(crate) fn tone_sample(
    &mut self,
    b_mute_by_unit: bool,
    ch_num: u8,
    time_pan_index: usize,
    smooth_smp: u32,
    instances: &[VoiceInstance],
  ) {
    if b_mute_by_unit && !self.played {
      for ch in 0..ch_num as usize {
        self.pan_time_bufs[ch][time_pan_index] = 0;
      }
      return;
    }

    for ch in 0..MAX_CHANNEL {
      let mut buf = 0i32;
      for v in 0..self.voice_num {
        let vt = &self.tones[v];
        let vi = &instances[v];
        if vt.life_count > 0 {
          let pos = vt.smp_pos as usize;
          let mut work = vi.get_sample_i16(pos, ch) as i32;

          if ch_num == 1 {
            work += vi.get_sample_i16(pos, 1) as i32;
            work /= 2;
          }

          work = work * self.v_velocity as i32 / 128;
          work = work * self.v_volume as i32 / 128;
          work = work * self.pan_vols[ch] as i32 / 64;

          if vi.env_size > 0 {
            work = work * vt.env_volume / 128;
          }

          // smooth tail
          if self.voice_flags.get(v).copied().unwrap_or(0) & VOICE_FLAG_SMOOTH != 0
            && vt.life_count < smooth_smp
          {
            work = work * vt.life_count as i32 / smooth_smp as i32;
          }
          buf += work;
        }
      }
      self.pan_time_bufs[ch][time_pan_index] = buf;
    }
  }

  /// Adds pan_time_bufs values to group samples
  pub(crate) fn tone_supple(&self, group_smps: &mut [i32], ch: usize, time_pan_index: usize) {
    let idx =
      (time_pan_index + BUFSIZE_TIMEPAN - self.pan_times[ch] as usize) & (BUFSIZE_TIMEPAN - 1);
    if self.v_groupno < group_smps.len() {
      group_smps[self.v_groupno as usize] += self.pan_time_bufs[ch][idx];
    }
  }

  /// Applies portamento processing and returns the current key
  pub(crate) fn tone_increment_key(&mut self) -> i32 {
    if self.portament_sample_num != 0 && self.key_margin != 0 {
      if self.portament_sample_pos < self.portament_sample_num {
        self.portament_sample_pos += 1;
        self.key_now = self.key_start
          + (self.key_margin as f64 * self.portament_sample_pos as f64
            / self.portament_sample_num as f64) as i32;
      } else {
        self.key_now = self.key_start + self.key_margin;
        self.key_start = self.key_now;
        self.key_margin = 0;
      }
    } else {
      self.key_now = self.key_start + self.key_margin;
    }
    self.key_now
  }

  /// Advances the sample position
  pub(crate) fn tone_increment_sample(&mut self, freq: f32, instances: &[VoiceInstance]) {
    for v in 0..self.voice_num {
      let vi = &instances[v];
      let vt = &mut self.tones[v];
      if vt.life_count > 0 {
        vt.life_count -= 1;
      }
      if vt.life_count > 0 {
        if vt.on_count > 0 {
          vt.on_count -= 1;
        }
        vt.smp_pos += vt.offset_freq as f64 * self.v_tuning as f64 * freq as f64;

        let body = vi.smp_body_w as f64;
        if vt.smp_pos >= body {
          if self.voice_flags.get(v).copied().unwrap_or(0) & VOICE_FLAG_WAVELOOP != 0 {
            vt.smp_pos -= body;
            if vt.smp_pos >= body {
              vt.smp_pos = 0.0;
            }
          } else {
            vt.life_count = 0;
          }
        }

        if vt.on_count == 0 && vi.env_size > 0 {
          vt.env_start = vt.env_volume;
          vt.env_pos = 0;
        }
      }
    }
  }
}
