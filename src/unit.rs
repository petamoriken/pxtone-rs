use crate::event::{
  EVENT_DEFAULT_GROUP_NO, EVENT_DEFAULT_KEY, EVENT_DEFAULT_TUNING, EVENT_DEFAULT_VELOCITY,
  EVENT_DEFAULT_VOLUME,
};
use crate::pulse::frequency::FrequencyTable;
use crate::woice::{BUFSIZE_TIMEPAN, VOICE_FLAG_SMOOTH, VOICE_FLAG_WAVELOOP, VoiceInstance};
use alloc::vec::Vec;

pub const MAX_CHANNEL: usize = 2;
pub const MAX_UNIT_CONTROL_VOICE: usize = 2;
pub(crate) const MAX_GROUP_COUNT: usize = 7;

/// One group's mixer accumulator over the samples of a block: `[channel][sample]`.
///
/// The samples of a plane are contiguous, so a unit, an effect and the output
/// sum each walk one straight through instead of striding over the groups they
/// do not touch.
pub(crate) type MixPlanes<'a> = [&'a mut [i32]; MAX_CHANNEL];

/// Adds `ring`, read from slot `from` onwards and wrapping once, into `dst`.
///
/// `dst` must be no longer than the ring. Split at the wrap the two halves are
/// plain contiguous adds.
#[inline]
fn add_ring(dst: &mut [i32], ring: &[i32; BUFSIZE_TIMEPAN], from: usize) {
  let (head, tail) = ring.split_at(from & (BUFSIZE_TIMEPAN - 1));
  let split = tail.len().min(dst.len());
  let (first, second) = dst.split_at_mut(split);
  for (w, &v) in first.iter_mut().zip(tail) {
    *w += v;
  }
  for (w, &v) in second.iter_mut().zip(head) {
    *w += v;
  }
}

/// `x / 2^SHIFT`, truncating toward zero, written out as shifts.
///
/// Native backends strength-reduce `/` to exactly this, but the wasm backend
/// emits `i32.div_s` and leaves the reduction to the engine. Spelling it out
/// keeps the hot path free of a division instruction on every target.
///
/// `sign` is all ones when `x` is negative and zero otherwise. The mixing chain
/// scales a sample by velocity, volume, pan and envelope, none of which is
/// negative, so the sign holds for the whole chain and the caller works it out
/// once. A step that reaches zero is unaffected: the correction the truncation
/// needs is smaller than the divisor.
#[inline(always)]
fn div_pow2_i32<const SHIFT: u32>(x: i32, sign: i32) -> i32 {
  (x + (sign & ((1i32 << SHIFT) - 1))) >> SHIFT
}

/// The per-unit constants a block of samples shares, hoisted out of the sample
/// loop by [`Unit::tone_params`]. Only an event can change any of them and no
/// event fires inside a block; left in `self` every one would be reloaded each
/// sample, because the `&mut self` that renders the sample could have written
/// them.
#[derive(Clone, Copy)]
pub(crate) struct ToneParams {
  /// Velocity, volume and pan, kept apart because the C++ divides by each in
  /// turn and every one of those divisions truncates. Folding them into a single
  /// factor and shifting once rounds differently.
  velocity: i32,
  volume: i32,
  pan_volumes: [i32; MAX_CHANNEL],
  /// `offset_frequency × tuning` per voice, the block-invariant half of the
  /// step [`Unit::step_advance`] adds to the sample position.
  ///
  /// In `f32`: the C++ multiplies the three floats together and only the sum
  /// widens, so computing the step in `f64` drifts the position and eventually
  /// reads a frame either side of the one it should.
  steps: [f32; MAX_UNIT_CONTROL_VOICE],
  voice_count: usize,
  voice_flags: [u32; MAX_UNIT_CONTROL_VOICE],
  /// Whether this unit renders silence because it is muted.
  muted: bool,
  pub(crate) group_index: usize,
  /// Pan delay per channel, already reduced modulo the ring. The C++ lets the
  /// delay run past the ring for sample rates below 44100, where reading it
  /// back lands on the current turn; taking the remainder up front says so.
  pan_delays: [usize; MAX_CHANNEL],
  /// Whether each voice's instrument carries an envelope, and how long its wave
  /// body is. Both live on the instrument rather than the unit, so the sample
  /// loop was reading them through the instance slice every time round.
  has_envelope: [bool; MAX_UNIT_CONTROL_VOICE],
  bodies: [f64; MAX_UNIT_CONTROL_VOICE],
}

/// Runtime playback state for a single voice layer within a unit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
#[derive(Clone)]
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
  pub(crate) voice_count: usize,
  pub(crate) voice_flags: [u32; MAX_UNIT_CONTROL_VOICE],
  pub(crate) tones: [VoiceTone; MAX_UNIT_CONTROL_VOICE],

  /// Consecutive silent samples already flushed into `pan_delay_buffers`.
  /// Once it reaches `BUFSIZE_TIMEPAN` the buffers hold nothing but zeros, so
  /// the unit contributes nothing and can be skipped entirely.
  quiet_run: u32,
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
      voice_count: 0,
      voice_flags: [0; MAX_UNIT_CONTROL_VOICE],
      tones: Default::default(),
      quiet_run: BUFSIZE_TIMEPAN as u32,
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
    self.quiet_run = BUFSIZE_TIMEPAN as u32;
  }

  /// `true` while at least one voice layer is still alive.
  #[inline]
  pub(crate) fn is_sounding(&self) -> bool {
    let n = self.voice_count.min(MAX_UNIT_CONTROL_VOICE);
    self.tones[..n].iter().any(|t| t.life_count > 0)
  }

  /// `true` once the pan-delay buffers contain nothing but zeros, i.e. the unit
  /// can no longer contribute to the mix.
  #[inline]
  pub(crate) fn is_flushed(&self) -> bool {
    self.quiet_run >= BUFSIZE_TIMEPAN as u32
  }

  /// Writes one silent frame into the pan-delay buffers, stopping once they are
  /// fully drained.
  #[inline]
  pub(crate) fn tone_silence(&mut self, time_pan_index: usize) {
    if self.quiet_run < BUFSIZE_TIMEPAN as u32 {
      self.pan_delay_buffers[0][time_pan_index] = 0;
      self.pan_delay_buffers[1][time_pan_index] = 0;
      self.quiet_run += 1;
    }
  }

  pub(crate) fn tone_reset_and_2prm(
    &mut self,
    voice_idx: usize,
    env_rls_ticks: u32,
    offset_frequency: f32,
  ) {
    let t = &mut self.tones[voice_idx];
    t.life_count = 0;
    t.on_count = 0;
    t.sample_pos = 0.0;
    t.smooth_volume = 0;
    t.envelope_release = env_rls_ticks;
    t.offset_frequency = offset_frequency;
  }

  pub(crate) fn set_woice(
    &mut self,
    voice_count: usize,
    voice_flags: [u32; MAX_UNIT_CONTROL_VOICE],
  ) {
    self.voice_count = voice_count.min(MAX_UNIT_CONTROL_VOICE);
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

  /// Advances one voice layer's envelope by a sample. `vi.envelope_size > 0`
  /// and `vt.life_count > 0` must already hold.
  #[inline(always)]
  fn step_envelope(vt: &mut VoiceTone, vi: &VoiceInstance) {
    if vt.on_count > 0 {
      if let Some(&e) = vi.envelope.get(vt.envelope_pos as usize) {
        vt.envelope_volume = e as i32;
        vt.envelope_pos += 1;
      }
    } else {
      // release
      vt.envelope_volume = vt.envelope_start
        + (0 - vt.envelope_start) * vt.envelope_pos as i32 / vi.envelope_release.max(1) as i32;
      vt.envelope_pos += 1;
    }
  }

  #[inline(always)]
  pub(crate) fn tone_envelope(&mut self, instances: &[VoiceInstance]) {
    let voice_count = self.voice_count.min(MAX_UNIT_CONTROL_VOICE);
    for (v, vi) in instances.iter().enumerate().take(voice_count) {
      let vt = &mut self.tones[v];
      if vt.life_count > 0 && vi.envelope_size > 0 {
        Self::step_envelope(vt, vi);
      }
    }
  }

  /// One live voice's contribution to one sample, on both channels, advancing
  /// its envelope and playback position by the sample.
  ///
  /// `vt.life_count > 0` must already hold. Shared by the sample-major and the
  /// voice-major block paths so the arithmetic the C++ pins down lives in one
  /// place.
  #[inline(always)]
  #[allow(clippy::too_many_arguments)]
  fn voice_sample<const ENVELOPE: bool>(
    vt: &mut VoiceTone,
    vi: &VoiceInstance,
    params: &ToneParams,
    v: usize,
    channels: u8,
    smooth_smp: u32,
    frequency: f32,
  ) -> [i32; MAX_CHANNEL] {
    let voice_flags = params.voice_flags[v];
    if ENVELOPE && params.has_envelope[v] {
      Self::step_envelope(vt, vi);
    }
    let [s0, s1] = vi.get_frame(vt.sample_pos as usize);
    let (s0, s1) = (s0 as i32, s1 as i32);

    // For mono output, ch=0 gets the average of both wave channels.
    // ch=1 keeps the raw ch=1 sample (unused when channel_count=1).
    let (w0, w1) = if channels == 1 {
      ((s0 + s1) / 2, s1)
    } else {
      (s0, s1)
    };

    // Velocity, volume and pan in turn, each truncating, as the C++ does.
    // Largest intermediate is 32767 × 128, well inside i32. The sign holds
    // for the whole chain, so it is taken once.
    let (sign0, sign1) = (w0 >> 31, w1 >> 31);
    let mut w0 = div_pow2_i32::<7>(w0 * params.velocity, sign0);
    let mut w1 = div_pow2_i32::<7>(w1 * params.velocity, sign1);
    w0 = div_pow2_i32::<7>(w0 * params.volume, sign0);
    w1 = div_pow2_i32::<7>(w1 * params.volume, sign1);
    w0 = div_pow2_i32::<6>(w0 * params.pan_volumes[0], sign0);
    w1 = div_pow2_i32::<6>(w1 * params.pan_volumes[1], sign1);

    if params.has_envelope[v] {
      w0 = div_pow2_i32::<7>(w0 * vt.envelope_volume, sign0);
      w1 = div_pow2_i32::<7>(w1 * vt.envelope_volume, sign1);
    }

    if voice_flags & VOICE_FLAG_SMOOTH != 0 && vt.life_count < smooth_smp {
      let lc = vt.life_count as i32;
      let sm = smooth_smp as i32;
      w0 = w0 * lc / sm;
      w1 = w1 * lc / sm;
    }

    Self::step_advance(
      vt,
      params.has_envelope[v],
      params.bodies[v],
      voice_flags,
      params.steps[v],
      frequency,
    );
    [w0, w1]
  }

  /// Runs one sample for this unit: envelope, mixing into `pan_delay_buffers`,
  /// and lifetime/position advance. All three stages share a single pass over
  /// the voice layers, since the loop scaffolding costs more than the arithmetic.
  ///
  /// Both channels are handled in one voice iteration to expose data parallelism
  /// to the auto-vectorizer (LLVM can SIMD-pack w0/w1 when simd128 is enabled).
  ///
  /// `ENVELOPE` selects whether the envelope stage runs here. The event path has
  /// to update every unit's envelope before dispatching events, so it passes
  /// `false` and calls [`Unit::tone_envelope`] up front instead.
  ///
  /// `frequency` is the unit's current playback rate, i.e. the result of
  /// [`Unit::tone_increment_key`] fed through the frequency table.
  #[inline(always)]
  pub(crate) fn tone_sample<const ENVELOPE: bool>(
    &mut self,
    params: ToneParams,
    channels: u8,
    time_pan_index: usize,
    smooth_smp: u32,
    frequency: f32,
    instances: &[VoiceInstance],
  ) {
    if params.muted {
      if ENVELOPE {
        self.tone_envelope(instances);
      }
      self.tone_increment_sample(params, frequency, instances);
      self.tone_silence(time_pan_index);
      return;
    }
    self.quiet_run = 0;

    let mut buf0 = 0i32;
    let mut buf1 = 0i32;

    for (v, vi) in instances.iter().enumerate().take(params.voice_count) {
      let vt = &mut self.tones[v];
      if vt.life_count > 0 {
        let [w0, w1] =
          Self::voice_sample::<ENVELOPE>(vt, vi, &params, v, channels, smooth_smp, frequency);
        buf0 += w0;
        buf1 += w1;
      }
    }

    self.pan_delay_buffers[0][time_pan_index] = buf0;
    self.pan_delay_buffers[1][time_pan_index] = buf1;
  }

  /// `voice_count` lives are still running. Matches [`Unit::is_sounding`] for a
  /// block that holds its voice state in locals.
  #[inline(always)]
  fn tones_live(voice_count: usize, tone0: &VoiceTone, tone1: &VoiceTone) -> bool {
    (voice_count > 0 && tone0.life_count > 0) || (voice_count > 1 && tone1.life_count > 0)
  }

  /// Both pan-delay slots. `index` is masked to the ring length by the caller.
  #[inline(always)]
  fn write_ring(ring0: *mut i32, ring1: *mut i32, index: usize, s0: i32, s1: i32) {
    unsafe {
      ring0.add(index).write(s0);
      ring1.add(index).write(s1);
    }
  }

  /// [`Unit::tone_silence`] against locals and a ring pointer.
  #[inline(always)]
  fn silence_at(quiet_run: &mut u32, ring0: *mut i32, ring1: *mut i32, index: usize) {
    if *quiet_run < BUFSIZE_TIMEPAN as u32 {
      Self::write_ring(ring0, ring1, index, 0, 0);
      *quiet_run += 1;
    }
  }

  /// Muted sample: envelope, then playback advance, no wave. Voice order matches
  /// [`Unit::tone_envelope`] followed by [`Unit::tone_increment_sample`].
  #[inline(always)]
  fn advance_muted(
    tone0: &mut VoiceTone,
    tone1: &mut VoiceTone,
    params: &ToneParams,
    instances: &[VoiceInstance],
    frequency: f32,
  ) {
    let n = params.voice_count.min(instances.len());
    if n > 0 {
      Self::advance_muted_voice(tone0, &instances[0], params, 0, frequency);
    }
    if n > 1 {
      Self::advance_muted_voice(tone1, &instances[1], params, 1, frequency);
    }
  }

  #[inline(always)]
  fn advance_muted_voice(
    vt: &mut VoiceTone,
    vi: &VoiceInstance,
    params: &ToneParams,
    v: usize,
    frequency: f32,
  ) {
    if vt.life_count > 0 && params.has_envelope[v] {
      Self::step_envelope(vt, vi);
    }
    if vt.life_count > 0 {
      Self::step_advance(
        vt,
        params.has_envelope[v],
        params.bodies[v],
        params.voice_flags[v],
        params.steps[v],
        frequency,
      );
    }
  }

  /// One sounding sample, voice 0 then voice 1, summed the way [`Unit::tone_sample`]
  /// sums them.
  #[inline(always)]
  fn mix_voices(
    tone0: &mut VoiceTone,
    tone1: &mut VoiceTone,
    params: &ToneParams,
    instances: &[VoiceInstance],
    channels: u8,
    smooth_smp: u32,
    frequency: f32,
  ) -> [i32; 2] {
    let mut buf0 = 0i32;
    let mut buf1 = 0i32;
    let n = params.voice_count.min(instances.len());
    if n > 0 && tone0.life_count > 0 {
      let [w0, w1] = Self::voice_sample::<true>(
        tone0,
        &instances[0],
        params,
        0,
        channels,
        smooth_smp,
        frequency,
      );
      buf0 += w0;
      buf1 += w1;
    }
    if n > 1 && tone1.life_count > 0 {
      let [w0, w1] = Self::voice_sample::<true>(
        tone1,
        &instances[1],
        params,
        1,
        channels,
        smooth_smp,
        frequency,
      );
      buf0 += w0;
      buf1 += w1;
    }
    [buf0, buf1]
  }

  /// Renders one block of samples for this unit, accumulating into `mix`.
  ///
  /// `mix[i]` is the group accumulator for the `i`-th sample of the block and
  /// `time_pan_index` is the pan-delay ring slot of `mix[0]`. Keeping the sample
  /// loop inside the unit lets the mixing parameters and the voice-layer state
  /// stay in registers across the whole block.
  ///
  /// Only valid when no event fires during the block: a unit that is idle at the
  /// start then stays idle, so the whole block can be skipped for it.
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn tone_block(
    &mut self,
    planes: &mut MixPlanes<'_>,
    mute_by_unit: bool,
    channels: u8,
    channel_count: usize,
    time_pan_index: usize,
    smooth_smp: u32,
    frequency: &FrequencyTable,
    sample_stride: f32,
    instances: &[VoiceInstance],
  ) {
    let count = planes[0].len();
    if !self.is_sounding() && self.is_flushed() {
      // The C++ increments every unit's key on every sample, sounding or not,
      // so a portamento walks on through a rest and the key the next note
      // starts from has moved. With no portamento in flight the increment is
      // idempotent, so once stands in for the whole block.
      if self.portamento_duration != 0 && self.key_delta != 0 {
        for _ in 0..count {
          self.tone_increment_key();
        }
      } else {
        self.tone_increment_key();
      }
      return;
    }

    let params = self.tone_params(mute_by_unit, instances);
    // The key only moves while a portamento is in flight; otherwise it, and the
    // playback rate that comes out of the frequency table, are the same for
    // every sample in the block. `advance_key` is idempotent in that case, so
    // calling it once leaves the same state behind.
    let steady = self.portamento_duration == 0 || self.key_delta == 0;

    // Voice fields and the pan-delay ring share `&mut self`. A store into the
    // ring forces every voice field out to memory, and the next sample loads it
    // back; on wasm that round trip is what the sample loop spends. Hold the
    // fields in locals for the block, write the ring through a pointer that
    // does not alias them, and copy the fields back once.
    let mut tone0 = self.tones[0];
    let mut tone1 = self.tones[1];
    let mut quiet_run = self.quiet_run;
    let mut key = self.key;
    let mut key_start = self.key_start;
    let mut key_delta = self.key_delta;
    let mut portamento_pos = self.portamento_pos;
    let portamento_duration = self.portamento_duration;

    // Rendering fills the pan-delay ring and reading it back trails behind by
    // the pan delay, so a run longer than the slots left before the ring turns
    // over would overwrite history the later reads still need. Rendering a run
    // of that length and then draining it into the planes keeps every read
    // standing while leaving the drain a contiguous walk of both.
    let chunk = BUFSIZE_TIMEPAN - params.pan_delays[0].max(params.pan_delays[1]);

    let mut freq = 0.0f32;
    let mut have_freq = false;

    let mut start = 0;
    while start < count {
      let end = (start + chunk).min(count);
      // `addr_of_mut` does not borrow, so the pointer stays valid for the writes
      // below and the shared borrow in `add_ring` can follow them.
      let ring0 = core::ptr::addr_of_mut!(self.pan_delay_buffers[0]).cast::<i32>();
      let ring1 = core::ptr::addr_of_mut!(self.pan_delay_buffers[1]).cast::<i32>();
      for i in start..end {
        let time_pan_index = (time_pan_index + i) & (BUFSIZE_TIMEPAN - 1);
        if !have_freq || !steady {
          key = Self::advance_key(
            &mut key_start,
            &mut key_delta,
            &mut portamento_pos,
            portamento_duration,
          );
          freq = frequency.get2(key) * sample_stride;
          have_freq = true;
        }
        if Self::tones_live(params.voice_count, &tone0, &tone1) {
          if params.muted {
            Self::advance_muted(&mut tone0, &mut tone1, &params, instances, freq);
            Self::silence_at(&mut quiet_run, ring0, ring1, time_pan_index);
          } else {
            quiet_run = 0;
            let [s0, s1] = Self::mix_voices(
              &mut tone0, &mut tone1, &params, instances, channels, smooth_smp, freq,
            );
            Self::write_ring(ring0, ring1, time_pan_index, s0, s1);
          }
        } else {
          Self::silence_at(&mut quiet_run, ring0, ring1, time_pan_index);
        }
      }

      // Taken from what the run did rather than read back out of `self`.
      // Rendering a frame clears the quiet run, so a unit that reports itself
      // flushed here wrote nothing but silence for the last 64 slots, which is
      // every slot the run reads back. A muted unit writes silence too and can
      // report the same, and it costs nothing: those slots are all zero.
      if quiet_run < BUFSIZE_TIMEPAN as u32 {
        for (ch, plane) in planes.iter_mut().enumerate().take(channel_count) {
          let from = time_pan_index + start + BUFSIZE_TIMEPAN - params.pan_delays[ch];
          add_ring(&mut plane[start..end], &self.pan_delay_buffers[ch], from);
        }
      }
      start = end;
    }

    self.tones[0] = tone0;
    self.tones[1] = tone1;
    self.quiet_run = quiet_run;
    self.key = key;
    self.key_start = key_start;
    self.key_delta = key_delta;
    self.portamento_pos = portamento_pos;
  }

  /// Reads the constants a block of samples shares. See [`ToneParams`].
  #[inline]
  pub(crate) fn tone_params(&self, mute_by_unit: bool, instances: &[VoiceInstance]) -> ToneParams {
    let envelope_of = |v: usize| instances.get(v).is_some_and(|vi| vi.envelope_size > 0);
    let body_of = |v: usize| instances.get(v).map_or(0.0, |vi| vi.body_frames as f64);
    ToneParams {
      velocity: self.velocity as i32,
      volume: self.volume as i32,
      pan_volumes: [self.pan_volumes[0] as i32, self.pan_volumes[1] as i32],
      steps: [
        self.tones[0].offset_frequency * self.tuning,
        self.tones[1].offset_frequency * self.tuning,
      ],
      voice_count: self.voice_count.min(MAX_UNIT_CONTROL_VOICE),
      voice_flags: self.voice_flags,
      muted: mute_by_unit && !self.played,
      group_index: self.group_index,
      pan_delays: [
        self.pan_delays[0] as usize & (BUFSIZE_TIMEPAN - 1),
        self.pan_delays[1] as usize & (BUFSIZE_TIMEPAN - 1),
      ],
      has_envelope: [envelope_of(0), envelope_of(1)],
      bodies: [body_of(0), body_of(1)],
    }
  }

  // Adds one sample of this unit's pan_delay_buffers into its group planes.
  // Both channels are handled in one call so the caller only walks the unit list
  // once. The block path drains a whole run at a time instead; see `tone_block`.
  #[inline]
  pub(crate) fn tone_supple(
    &self,
    params: ToneParams,
    planes: &mut MixPlanes<'_>,
    channels: usize,
    time_pan_index: usize,
  ) {
    for (ch, plane) in planes.iter_mut().enumerate().take(channels) {
      let idx = (time_pan_index + BUFSIZE_TIMEPAN - params.pan_delays[ch]) & (BUFSIZE_TIMEPAN - 1);
      plane[0] += self.pan_delay_buffers[ch][idx];
    }
  }

  /// One step of [`Unit::tone_increment_key`], on the key fields alone.
  ///
  /// Split out so a block can keep the fields in locals: the sample loop must
  /// not store through `&mut self`, or the voice state is written back with
  /// them.
  #[inline(always)]
  fn advance_key(
    key_start: &mut i32,
    key_delta: &mut i32,
    portamento_pos: &mut u32,
    portamento_duration: u32,
  ) -> i32 {
    if portamento_duration != 0 && *key_delta != 0 {
      if *portamento_pos < portamento_duration {
        *portamento_pos += 1;
        // The C++ truncates the whole sum, not just the fraction:
        //   _key_now = (int32_t)( _key_start + (double)_key_margin * pos / num );
        // For a downward portamento the margin is negative, and truncating
        // toward zero after the addition lands a key lower than truncating
        // before it.
        (*key_start as f64
          + *key_delta as f64 * *portamento_pos as f64 / portamento_duration as f64) as i32
      } else {
        let key = *key_start + *key_delta;
        *key_start = key;
        *key_delta = 0;
        key
      }
    } else {
      *key_start + *key_delta
    }
  }

  // Applies portamento processing and returns the current key
  #[inline]
  pub(crate) fn tone_increment_key(&mut self) -> i32 {
    let key = Self::advance_key(
      &mut self.key_start,
      &mut self.key_delta,
      &mut self.portamento_pos,
      self.portamento_duration,
    );
    self.key = key;
    key
  }

  /// Advances one voice layer's lifetime and sample position by a sample.
  /// `vt.life_count > 0` must already hold.
  #[inline(always)]
  fn step_advance(
    vt: &mut VoiceTone,
    has_envelope: bool,
    body: f64,
    voice_flags: u32,
    step: f32,
    frequency: f32,
  ) {
    vt.life_count -= 1;
    if vt.life_count == 0 {
      return;
    }
    if vt.on_count > 0 {
      vt.on_count -= 1;
      // Trigger release phase exactly once, when on_count first reaches 0.
      // (C++ uses int32_t which goes negative, so this condition fires only once.)
      if vt.on_count == 0 && has_envelope {
        vt.envelope_start = vt.envelope_volume;
        vt.envelope_pos = 0;
      }
    }
    vt.sample_pos += (step * frequency) as f64;

    if vt.sample_pos >= body {
      if voice_flags & VOICE_FLAG_WAVELOOP != 0 {
        vt.sample_pos -= body;
        if vt.sample_pos >= body {
          vt.sample_pos = 0.0;
        }
      } else {
        vt.life_count = 0;
      }
    }
  }

  // Advances the sample position of every live voice layer.
  #[inline(always)]
  pub(crate) fn tone_increment_sample(
    &mut self,
    params: ToneParams,
    frequency: f32,
    instances: &[VoiceInstance],
  ) {
    // Bounded by the instrument as well as the unit, as the mixing pass is.
    for v in 0..params.voice_count.min(instances.len()) {
      let voice_flags = params.voice_flags[v];
      let vt = &mut self.tones[v];
      if vt.life_count > 0 {
        Self::step_advance(
          vt,
          params.has_envelope[v],
          params.bodies[v],
          voice_flags,
          params.steps[v],
          frequency,
        );
      }
    }
  }
}
