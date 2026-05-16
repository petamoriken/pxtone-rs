use crate::pulse::noise::Noise;
use crate::service::{
  DestinationQuality, PxtoneService, StartPos, VomitPrepFlags, VomitPreparation,
};
use std::alloc::{Layout, alloc as sys_alloc, dealloc as sys_dealloc};
use std::io::Cursor;

#[cfg(not(target_feature = "atomics"))]
use talc::wasm::{WasmDynamicTalc, new_wasm_dynamic_allocator};

#[cfg(not(target_feature = "atomics"))]
#[global_allocator]
static TALC: WasmDynamicTalc = new_wasm_dynamic_allocator();

#[inline(always)]
fn pack_ptr_len(ptr: *const u8, len: usize) -> u64 {
  (ptr as u32 as u64) << 32 | len as u32 as u64
}

/// Allocates `size` bytes on the heap and returns a pointer to the buffer.
/// Returns null if `size` is 0 or allocation fails.
/// Free with [`dealloc`].
#[unsafe(no_mangle)]
pub extern "C" fn alloc(size: usize) -> *mut u8 {
  if size == 0 {
    return std::ptr::null_mut();
  }
  let layout = match Layout::array::<u8>(size) {
    Ok(l) => l,
    Err(_) => return std::ptr::null_mut(),
  };
  unsafe { sys_alloc(layout) }
}

/// Frees a buffer previously allocated by [`alloc`].
/// Does nothing if `ptr` is null or `size` is 0.
///
/// # Safety
/// `ptr` must have been returned by [`alloc`] with the same `size`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, size: usize) {
  if ptr.is_null() || size == 0 {
    return;
  }
  let layout = match Layout::array::<u8>(size) {
    Ok(l) => l,
    Err(_) => return,
  };
  unsafe { sys_dealloc(ptr, layout) };
}

/// Creates a new [`PxtoneService`] and returns an owning pointer.
/// `channels` must be `1` (mono) or `2` (stereo); returns null otherwise.
/// Free with [`service_free`].
#[unsafe(no_mangle)]
pub extern "C" fn service_new(channels: u32, sample_rate: u32) -> *mut PxtoneService {
  if channels != 1 && channels != 2 {
    return std::ptr::null_mut();
  }
  let quality = DestinationQuality {
    channels: channels as u8,
    sample_rate,
  };
  Box::into_raw(Box::new(PxtoneService::new(quality)))
}

/// Frees a [`PxtoneService`] previously created by [`service_new`].
///
/// # Safety
/// `ptr` must have been returned by [`service_new`] and must not be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_free(ptr: *mut PxtoneService) {
  if !ptr.is_null() {
    unsafe { drop(Box::from_raw(ptr)) };
  }
}

/// Reads a `.ptcop` file from `data[..len]` into the service.
/// Returns 0 on success, -1 on failure or if any pointer is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
/// `data` must be valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_read(svc: *mut PxtoneService, data: *const u8, len: usize) -> i32 {
  if svc.is_null() || data.is_null() {
    return -1;
  }
  let svc = unsafe { &mut *svc };
  let slice = unsafe { std::slice::from_raw_parts(data, len) };
  match svc.read(slice.to_vec()) {
    Ok(()) => 0,
    Err(_) => -1,
  }
}

/// Validates a `.ptcop`/`.pttune` file from `data[..len]` without creating a persistent service.
/// Returns 0 if valid, -1 otherwise.
///
/// # Safety
/// `data` must be valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn validate(data: *const u8, len: usize) -> i32 {
  if data.is_null() {
    return -1;
  }
  let slice = unsafe { std::slice::from_raw_parts(data, len) };
  let mut cursor = Cursor::new(slice);
  match PxtoneService::new(DestinationQuality::default()).read_metadata(&mut cursor) {
    Ok(()) => 0,
    Err(_) => -1,
  }
}

/// Prepares synthesizer tones. Must be called after [`service_read`].
/// Returns 0 on success, -1 on failure or if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_tones_ready(svc: *mut PxtoneService) -> i32 {
  if svc.is_null() {
    return -1;
  }
  let svc = unsafe { &mut *svc };
  match svc.tones_ready() {
    Ok(()) => 0,
    Err(_) => -1,
  }
}

/// Prepares playback. Must be called after [`service_tones_ready`].
/// Returns 0 on success, -1 on failure or if `svc` is null.
///
/// `start_sample` — sample offset to start from; `0` means the beginning of the song.
/// `unit_mute` — non-zero to mute units whose played flag is false.
/// `loop_` — non-zero to loop playback from the song's repeat point.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_moo_preparation(
  svc: *mut PxtoneService,
  start_sample: u32,
  unit_mute: i32,
  loop_: i32,
) -> i32 {
  if svc.is_null() {
    return -1;
  }
  let svc = unsafe { &mut *svc };
  let start_pos = if start_sample == 0 {
    StartPos::Beginning
  } else {
    StartPos::Sample(start_sample)
  };
  let mut flags = 0u8;
  if unit_mute != 0 {
    flags |= VomitPrepFlags::UNIT_MUTE;
  }
  if loop_ != 0 {
    flags |= VomitPrepFlags::LOOP;
  }
  let prep = VomitPreparation {
    flags,
    start_pos,
    ..VomitPreparation::default()
  };
  match svc.moo_preparation(prep) {
    Ok(()) => 0,
    Err(_) => -1,
  }
}

/// Internal: renders PCM samples into `buf[..len]`.
/// Returns `(buf, written_len)` packed as `u64` (`buf << 32 | written_len`).
/// `written_len` may be less than `len` at the end of the song, and 0 when playback
/// has already ended. Returns `(0, 0)` if any pointer is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
/// `buf` must be valid for `len` bytes.
#[unsafe(export_name = "_service_moo_impl")]
pub unsafe extern "C" fn service_moo(svc: *mut PxtoneService, buf: *mut u8, len: usize) -> u64 {
  if svc.is_null() || buf.is_null() {
    return 0;
  }
  let svc = unsafe { &mut *svc };
  let slice = unsafe { std::slice::from_raw_parts_mut(buf, len) };
  let written = svc.moo(slice);
  pack_ptr_len(buf, written)
}

/// Internal: renders a `.ptnoise` file.
/// Returns `(ptr, samples_len)` packed as `u64` (`ptr << 32 | samples_len`).
/// `ptr` points to a heap-allocated buffer of signed 16-bit interleaved PCM samples.
/// The caller must free it with `dealloc(ptr, samples_len)`.
/// Returns `(0, 0)` on failure.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
/// `data` must be valid for `data_len` bytes.
#[unsafe(export_name = "_service_render_noise_impl")]
pub unsafe extern "C" fn service_render_noise(
  svc: *mut PxtoneService,
  data: *const u8,
  data_len: usize,
) -> u64 {
  if svc.is_null() || data.is_null() {
    return 0;
  }
  let svc = unsafe { &mut *svc };
  let slice = unsafe { std::slice::from_raw_parts(data, data_len) };
  let mut cursor = Cursor::new(slice);
  let wave = match svc.render_noise(&mut cursor) {
    Ok(w) => w,
    Err(_) => return 0,
  };
  let len = wave.samples.len();
  if len == 0 {
    return 0;
  }
  let layout = match Layout::array::<u8>(len) {
    Ok(l) => l,
    Err(_) => return 0,
  };
  let ptr = unsafe { sys_alloc(layout) };
  if ptr.is_null() {
    return 0;
  }
  unsafe { std::ptr::copy_nonoverlapping(wave.samples.as_ptr(), ptr, len) };
  pack_ptr_len(ptr, len)
}

/// Validates a `.ptnoise` file from `data[..len]` without using a service.
/// Returns 0 if valid, -1 otherwise.
///
/// # Safety
/// `data` must be valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn validate_noise(data: *const u8, len: usize) -> i32 {
  if data.is_null() {
    return -1;
  }
  let slice = unsafe { std::slice::from_raw_parts(data, len) };
  let mut cursor = Cursor::new(slice);
  let mut noise = Noise::new();
  match noise.read(&mut cursor) {
    Ok(()) => 0,
    Err(_) => -1,
  }
}

// --- Master getters (internal; exported via WAT multi-value wrapper `service_get_master`) ---

#[unsafe(export_name = "_service_get_ticks_per_beat_impl")]
pub unsafe extern "C" fn service_get_ticks_per_beat(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.master.ticks_per_beat() as u32
}

#[unsafe(export_name = "_service_get_beats_per_measure_impl")]
pub unsafe extern "C" fn service_get_beats_per_measure(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.master.beats_per_measure() as u32
}

#[unsafe(export_name = "_service_get_beat_tempo_impl")]
pub unsafe extern "C" fn service_get_beat_tempo(svc: *const PxtoneService) -> f32 {
  if svc.is_null() {
    return 0.0;
  }
  unsafe { &*svc }.master.beat_tempo()
}

#[unsafe(export_name = "_service_get_measure_count_impl")]
pub unsafe extern "C" fn service_get_measure_count(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.master.measure_count()
}

#[unsafe(export_name = "_service_get_repeat_measure_impl")]
pub unsafe extern "C" fn service_get_repeat_measure(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.master.repeat_measure()
}

#[unsafe(export_name = "_service_get_last_measure_impl")]
pub unsafe extern "C" fn service_get_last_measure(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.master.last_measure()
}

// --- Text getters (internal; exported via WAT multi-value wrappers) ---

/// Internal: returns `(ptr, len)` for the song title as raw Shift-JIS bytes, packed as
/// `u64` (`ptr << 32 | len`). The pointer is valid as long as `svc` is alive and unmodified.
/// Returns `(0, 0)` if `svc` is null or no title is set.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(export_name = "_service_get_text_name_impl")]
pub unsafe extern "C" fn service_get_text_name(svc: *const PxtoneService) -> u64 {
  if svc.is_null() {
    return 0;
  }
  let svc = unsafe { &*svc };
  match svc.text.name() {
    Some(bytes) => pack_ptr_len(bytes.as_ptr(), bytes.len()),
    None => 0,
  }
}

/// Internal: returns `(ptr, len)` for the song comment as raw Shift-JIS bytes, packed as
/// `u64` (`ptr << 32 | len`). The pointer is valid as long as `svc` is alive and unmodified.
/// Returns `(0, 0)` if `svc` is null or no comment is set.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(export_name = "_service_get_text_comment_impl")]
pub unsafe extern "C" fn service_get_text_comment(svc: *const PxtoneService) -> u64 {
  if svc.is_null() {
    return 0;
  }
  let svc = unsafe { &*svc };
  match svc.text.comment() {
    Some(bytes) => pack_ptr_len(bytes.as_ptr(), bytes.len()),
    None => 0,
  }
}

// --- Unit getters ---

/// Returns the number of units in the loaded song.
/// Returns 0 if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_unit_count(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.units.len() as u32
}

/// Internal: returns `(ptr, len)` for the unit's raw name bytes, packed as
/// `u64` (`ptr << 32 | len`). The pointer is valid as long as `svc` is alive and unmodified.
/// Returns `(0, 0)` if `svc` is null or `idx` is out of range.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(export_name = "_service_get_unit_name_impl")]
pub unsafe extern "C" fn service_get_unit_name(svc: *const PxtoneService, idx: u32) -> u64 {
  if svc.is_null() {
    return 0;
  }
  let svc = unsafe { &*svc };
  match svc.units.get(idx as usize) {
    Some(unit) => {
      let name = unit.name();
      pack_ptr_len(name.as_ptr(), name.len())
    }
    None => 0,
  }
}

/// Returns 1 if the unit at `idx` is active (not muted), 0 if muted, -1 on error.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_unit_played(svc: *const PxtoneService, idx: u32) -> i32 {
  if svc.is_null() {
    return -1;
  }
  let svc = unsafe { &*svc };
  match svc.units.get(idx as usize) {
    Some(u) => {
      if u.played() {
        1
      } else {
        0
      }
    }
    None => -1,
  }
}

/// Sets whether the unit at `idx` is active (not muted).
/// Returns 0 on success, -1 on error.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_set_unit_played(
  svc: *mut PxtoneService,
  idx: u32,
  played: i32,
) -> i32 {
  if svc.is_null() {
    return -1;
  }
  let svc = unsafe { &mut *svc };
  match svc.units.get_mut(idx as usize) {
    Some(u) => {
      u.set_played(played != 0);
      0
    }
    None => -1,
  }
}

// --- Event getters ---

/// Returns the number of events in the loaded song.
/// Returns 0 if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_event_count(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.events.records().len() as u32
}

#[unsafe(export_name = "_service_get_event_tick_impl")]
pub unsafe extern "C" fn service_get_event_tick(svc: *const PxtoneService, idx: u32) -> i32 {
  if svc.is_null() {
    return 0;
  }
  let svc = unsafe { &*svc };
  svc
    .events
    .records()
    .get(idx as usize)
    .map_or(0, |e| e.tick())
}

#[unsafe(export_name = "_service_get_event_unit_index_impl")]
pub unsafe extern "C" fn service_get_event_unit_index(svc: *const PxtoneService, idx: u32) -> u32 {
  if svc.is_null() {
    return 0;
  }
  let svc = unsafe { &*svc };
  svc
    .events
    .records()
    .get(idx as usize)
    .map_or(0, |e| e.unit_index() as u32)
}

#[unsafe(export_name = "_service_get_event_kind_impl")]
pub unsafe extern "C" fn service_get_event_kind(svc: *const PxtoneService, idx: u32) -> u32 {
  if svc.is_null() {
    return 0;
  }
  let svc = unsafe { &*svc };
  svc
    .events
    .records()
    .get(idx as usize)
    .map_or(0, |e| e.kind() as u32)
}

#[unsafe(export_name = "_service_get_event_value_impl")]
pub unsafe extern "C" fn service_get_event_value(svc: *const PxtoneService, idx: u32) -> i32 {
  if svc.is_null() {
    return 0;
  }
  let svc = unsafe { &*svc };
  svc
    .events
    .records()
    .get(idx as usize)
    .map_or(0, |e| e.value())
}
