use crate::event::{
  EVENT_DEFAULT_GROUP_NO, EVENT_DEFAULT_KEY, EVENT_DEFAULT_TUNING, EVENT_DEFAULT_VELOCITY,
  EVENT_DEFAULT_VOLUME,
};
use crate::woice::{BUFSIZE_TIMEPAN, VOICE_FLAG_SMOOTH, VOICE_FLAG_WAVELOOP, VoiceInstance};

pub const MAX_CHANNEL: usize = 2;
pub const MAX_UNIT_CONTROL_VOICE: usize = 2;

/// Runtime playback state for a single voice layer within a unit.
#[derive(Clone, Default)]
pub struct VoiceTone {
  pub(crate) sample_pos: f64,
  pub(crate) offset_frequency: f32,
  pub(crate) envelope_volume: i32,
  pub(crate) life_count: u32,
  pub(crate) on_count: u32,
  pub(crate) envelope_start: i32,
  pub(crate) envelope_pos: u32,
  pub(crate) envelope_release: u32,
  pub(crate) smooth_volume: u32,
}

/// A single track (channel) in the song, with its current playback state.
pub struct Unit {
  pub(crate) played: bool,
  pub(crate) name: Vec<u8>,

  // Key state
  pub(crate) key: i32,
  pub(crate) key_start: i32,
  pub(crate) key_delta: i32,
  pub(crate) portamento_pos: u32,
  pub(crate) portamento_duration: u32,

  // Pan
  pub(crate) pan_volumes: [u32; MAX_CHANNEL],
  pub(crate) pan_delays: [u32; MAX_CHANNEL],
  pub(crate) pan_delay_buffers: [[i32; BUFSIZE_TIMEPAN]; MAX_CHANNEL],

  // Velocity, volume, etc.
  pub(crate) volume: u32,
  pub(crate) velocity: u32,
  pub(crate) group_index: usize,
  pub(crate) tuning: f32,

  // Voice references (one per instance)
  pub(crate) voice_num: usize,
  pub(crate) voice_flags: Vec<u32>,
  pub(crate) tones: [VoiceTone; MAX_UNIT_CONTROL_VOICE],
}

impl Default for Unit {
  fn default() -> Self {
    Self {
      played: true,
      name: b"no name".to_vec(),
      key: EVENT_DEFAULT_KEY,
      key_start: EVENT_DEFAULT_KEY,
      key_delta: 0,
      portamento_pos: 0,
      portamento_duration: 0,
      pan_volumes: [64; MAX_CHANNEL],
      pan_delays: [0; MAX_CHANNEL],
      pan_delay_buffers: [[0; BUFSIZE_TIMEPAN]; MAX_CHANNEL],
      volume: EVENT_DEFAULT_VOLUME,
      velocity: EVENT_DEFAULT_VELOCITY,
      group_index: EVENT_DEFAULT_GROUP_NO,
      tuning: EVENT_DEFAULT_TUNING,
      voice_num: 0,
      voice_flags: Vec::new(),
      tones: Default::default(),
    }
  }
}

impl Unit {
  /// Unit name as raw bytes (may be Shift-JIS encoded for Japanese names).
  #[inline]
  pub fn name(&self) -> &[u8] {
    &self.name
  }

  /// Whether this unit is not muted.
  #[inline]
  pub fn played(&self) -> bool {
    self.played
  }

  /// Sets whether this unit is active. Pass `false` to mute, `true` to enable.
  #[inline]
  pub fn set_played(&mut self, played: bool) {
    self.played = played;
  }

  pub(crate) fn new() -> Self {
    Self::default()
  }

  pub(crate) fn tone_init(&mut self) {
    self.group_index = EVENT_DEFAULT_GROUP_NO;
    self.velocity = EVENT_DEFAULT_VELOCITY;
    self.volume = EVENT_DEFAULT_VOLUME;
    self.tuning = EVENT_DEFAULT_TUNING;
    self.portamento_duration = 0;
    self.portamento_pos = 0;
    self.pan_volumes.fill(64);
    self.pan_delays.fill(0);
  }

  pub(crate) fn tone_clear(&mut self) {
    for buf in &mut self.pan_delay_buffers {
      buf.fill(0);
    }
  }

  pub(crate) fn tone_reset_and_2prm(
    &mut self,
    voice_idx: usize,
    env_rls_clock: u32,
    offset_frequency: f32,
  ) {
    let t = &mut self.tones[voice_idx];
    t.life_count = 0;
    t.on_count = 0;
    t.sample_pos = 0.0;
    t.smooth_volume = 0;
    t.envelope_release = env_rls_clock;
    t.offset_frequency = offset_frequency;
  }

  pub(crate) fn set_woice(&mut self, voice_num: usize, voice_flags: Vec<u32>) {
    self.voice_num = voice_num;
    self.voice_flags = voice_flags;
    self.key = EVENT_DEFAULT_KEY;
    self.key_delta = 0;
    self.key_start = EVENT_DEFAULT_KEY;
  }

  #[inline]
  pub(crate) fn tone_zero_lives(&mut self) {
    for t in &mut self.tones {
      t.life_count = 0;
    }
  }

  #[inline]
  pub(crate) fn tone_key_on(&mut self) {
    self.key = self.key_start + self.key_delta;
    self.key_start = self.key;
    self.key_delta = 0;
  }

  #[inline]
  pub(crate) fn tone_key(&mut self, key: i32) {
    self.key_start = self.key;
    self.key_delta = key - self.key_start;
    self.portamento_pos = 0;
  }

  pub(crate) fn tone_pan_volume(&mut self, channels: u32, pan: u32) {
    self.pan_volumes[0] = 64;
    self.pan_volumes[1] = 64;
    if channels == 2 {
      if pan >= 64 {
        self.pan_volumes[0] = 128 - pan;
      } else {
        self.pan_volumes[1] = pan;
      }
    }
  }

  pub(crate) fn tone_pan_time(&mut self, channels: u32, pan: u32, sample_rate: u32) {
    self.pan_delays[0] = 0;
    self.pan_delays[1] = 0;
    if channels == 2 {
      if pan >= 64 {
        let v = (pan - 64).min(63);
        self.pan_delays[0] = v * 44100 / sample_rate;
      } else {
        let v = (64 - pan).min(63);
        self.pan_delays[1] = v * 44100 / sample_rate;
      }
    }
  }

  #[inline]
  pub(crate) fn tone_velocity(&mut self, val: u32) {
    self.velocity = val;
  }
  #[inline]
  pub(crate) fn tone_volume(&mut self, val: u32) {
    self.volume = val;
  }
  #[inline]
  pub(crate) fn tone_portament(&mut self, val: u32) {
    self.portamento_duration = val;
  }
  #[inline]
  pub(crate) fn tone_groupno(&mut self, val: usize) {
    self.group_index = val;
  }
  #[inline]
  pub(crate) fn tone_tuning(&mut self, val: f32) {
    self.tuning = val;
  }

  pub(crate) fn tone_envelope(&mut self, instances: &[VoiceInstance]) {
    for (v, vi) in instances.iter().enumerate().take(self.voice_num) {
      let vt = &mut self.tones[v];
      if vt.life_count > 0 && vi.envelope_size > 0 {
        if vt.on_count > 0 {
          if vt.envelope_pos < vi.envelope_size {
            vt.envelope_volume = vi.envelope[vt.envelope_pos as usize] as i32;
            vt.envelope_pos += 1;
          }
        } else {
          // release
          vt.envelope_volume = vt.envelope_start
            + (0 - vt.envelope_start) * vt.envelope_pos as i32 / vi.envelope_release.max(1) as i32;
          vt.envelope_pos += 1;
        }
      }
    }
  }

  // Generates samples and writes them into pan_time_bufs
  pub(crate) fn tone_sample(
    &mut self,
    mute_by_unit: bool,
    channels: u8,
    time_pan_index: usize,
    smooth_smp: u32,
    instances: &[VoiceInstance],
  ) {
    if mute_by_unit && !self.played {
      for ch in 0..channels as usize {
        self.pan_delay_buffers[ch][time_pan_index] = 0;
      }
      return;
    }

    for ch in 0..MAX_CHANNEL {
      let mut buf = 0i32;
      for (v, vi) in instances.iter().enumerate().take(self.voice_num) {
        let vt = &self.tones[v];
        if vt.life_count > 0 {
          let pos = vt.sample_pos as usize;
          let mut work = vi.get_sample_i16(pos, ch) as i32;

          if channels == 1 {
            work += vi.get_sample_i16(pos, 1) as i32;
            work /= 2;
          }

          work = work * self.velocity as i32 / 128;
          work = work * self.volume as i32 / 128;
          work = work * self.pan_volumes[ch] as i32 / 64;

          if vi.envelope_size > 0 {
            work = work * vt.envelope_volume / 128;
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
      self.pan_delay_buffers[ch][time_pan_index] = buf;
    }
  }

  // Adds pan_delay_buffers values to group samples
  #[inline]
  pub(crate) fn tone_supple(&self, group_smps: &mut [i32], ch: usize, time_pan_index: usize) {
    let idx =
      (time_pan_index + BUFSIZE_TIMEPAN - self.pan_delays[ch] as usize) & (BUFSIZE_TIMEPAN - 1);
    if self.group_index < group_smps.len() {
      group_smps[self.group_index] += self.pan_delay_buffers[ch][idx];
    }
  }

  // Applies portamento processing and returns the current key
  #[inline]
  pub(crate) fn tone_increment_key(&mut self) -> i32 {
    if self.portamento_duration != 0 && self.key_delta != 0 {
      if self.portamento_pos < self.portamento_duration {
        self.portamento_pos += 1;
        self.key = self.key_start
          + (self.key_delta as f64 * self.portamento_pos as f64 / self.portamento_duration as f64)
            as i32;
      } else {
        self.key = self.key_start + self.key_delta;
        self.key_start = self.key;
        self.key_delta = 0;
      }
    } else {
      self.key = self.key_start + self.key_delta;
    }
    self.key
  }

  // Advances the sample position
  pub(crate) fn tone_increment_sample(&mut self, frequency: f32, instances: &[VoiceInstance]) {
    for (v, vi) in instances.iter().enumerate().take(self.voice_num) {
      let vt = &mut self.tones[v];
      if vt.life_count > 0 {
        vt.life_count -= 1;
      }
      if vt.life_count > 0 {
        if vt.on_count > 0 {
          vt.on_count -= 1;
        }
        vt.sample_pos += vt.offset_frequency as f64 * self.tuning as f64 * frequency as f64;

        let body = vi.body_frames as f64;
        if vt.sample_pos >= body {
          if self.voice_flags.get(v).copied().unwrap_or(0) & VOICE_FLAG_WAVELOOP != 0 {
            vt.sample_pos -= body;
            if vt.sample_pos >= body {
              vt.sample_pos = 0.0;
            }
          } else {
            vt.life_count = 0;
          }
        }

        if vt.on_count == 0 && vi.envelope_size > 0 {
          vt.envelope_start = vt.envelope_volume;
          vt.envelope_pos = 0;
        }
      }
    }
  }
}
