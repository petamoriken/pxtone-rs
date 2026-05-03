use crate::service::{PxtoneService, StartPos, VomitPrepFlags, VomitPreparation};
use std::alloc::{Layout, alloc as sys_alloc, dealloc as sys_dealloc};
use std::io::Cursor;

#[cfg(not(target_feature = "atomics"))]
use talc::wasm::{WasmDynamicTalc, new_wasm_dynamic_allocator};

#[cfg(not(target_feature = "atomics"))]
#[global_allocator]
static TALC: WasmDynamicTalc = new_wasm_dynamic_allocator();

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
/// Free with [`service_free`].
#[unsafe(no_mangle)]
pub extern "C" fn service_new() -> *mut PxtoneService {
  Box::into_raw(Box::new(PxtoneService::new()))
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
  let mut cursor = Cursor::new(slice);
  match svc.read(&mut cursor) {
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

/// Renders the next chunk of PCM samples into `buf[..len]` (signed 16-bit interleaved).
/// Returns 1 if samples were written, 0 if playback ended or any pointer is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
/// `buf` must be valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_moo(svc: *mut PxtoneService, buf: *mut u8, len: usize) -> i32 {
  if svc.is_null() || buf.is_null() {
    return 0;
  }
  let svc = unsafe { &mut *svc };
  let slice = unsafe { std::slice::from_raw_parts_mut(buf, len) };
  if svc.moo(slice) { 1 } else { 0 }
}

/// Returns 1 if playback has reached the end, 0 otherwise.
/// Returns 1 if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_is_end_vomit(svc: *const PxtoneService) -> i32 {
  if svc.is_null() {
    return 1;
  }
  let svc = unsafe { &*svc };
  if svc.is_end_vomit() { 1 } else { 0 }
}

/// Returns the number of output channels (e.g. 2 for stereo).
/// Returns 0 if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_channels(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  let svc = unsafe { &*svc };
  svc.get_destination_quality().channels as u32
}

/// Returns the sample rate in Hz.
/// Returns 0 if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_sample_rate(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  let svc = unsafe { &*svc };
  svc.get_destination_quality().sample_rate
}

/// Renders a `.ptnoise` file and returns a pointer to the allocated PCM samples buffer
/// (signed 16-bit interleaved). The caller must free it with `dealloc(ptr, *out_samples_len)`.
/// Writes byte length to `out_samples_len`.
/// Returns null on failure. `out_samples_len` must be non-null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
/// `data` must be valid for `data_len` bytes.
/// `out_samples_len` must be a valid writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_render_noise(
  svc: *mut PxtoneService,
  data: *const u8,
  data_len: usize,
  out_samples_len: *mut u32,
) -> *mut u8 {
  if svc.is_null() || data.is_null() || out_samples_len.is_null() {
    return std::ptr::null_mut();
  }
  let svc = unsafe { &mut *svc };
  let slice = unsafe { std::slice::from_raw_parts(data, data_len) };
  let mut cursor = Cursor::new(slice);
  let wave = match svc.render_noise(&mut cursor) {
    Ok(w) => w,
    Err(_) => return std::ptr::null_mut(),
  };
  let len = wave.samples.len();
  if len == 0 {
    unsafe {
      *out_samples_len = 0;
    }
    return std::ptr::null_mut();
  }
  let layout = match Layout::array::<u8>(len) {
    Ok(l) => l,
    Err(_) => return std::ptr::null_mut(),
  };
  let ptr = unsafe { sys_alloc(layout) };
  if ptr.is_null() {
    return std::ptr::null_mut();
  }
  unsafe {
    std::ptr::copy_nonoverlapping(wave.samples.as_ptr(), ptr, len);
    *out_samples_len = len as u32;
  }
  ptr
}

/// Returns the number of ticks per beat.
/// Returns 0 if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_beat_clock(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.master.beat_clock() as u32
}

/// Returns the number of beats per measure.
/// Returns 0 if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_beat_num(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.master.beat_num() as u32
}

/// Returns the tempo in beats per minute.
/// Returns 0.0 if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_beat_tempo(svc: *const PxtoneService) -> f32 {
  if svc.is_null() {
    return 0.0;
  }
  unsafe { &*svc }.master.beat_tempo()
}

/// Returns the number of measures in the loaded song.
/// Returns 0 if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_measure_num(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.master.measure_num()
}

/// Returns the repeat position in measures.
/// Returns 0 if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_repeat_measure(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.master.repeat_measure()
}

/// Returns the last measure position.
/// Returns 0 if `svc` is null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_last_measure(svc: *const PxtoneService) -> u32 {
  if svc.is_null() {
    return 0;
  }
  unsafe { &*svc }.master.last_measure()
}

/// Returns a pointer to the song title as raw Shift-JIS bytes and writes the byte length to
/// `*out_len`. The pointer is valid as long as `svc` is alive and unmodified.
/// Returns null if `svc` is null, `out_len` is null, or no title is set.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
/// `out_len` must be a valid writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_text_name(
  svc: *const PxtoneService,
  out_len: *mut u32,
) -> *const u8 {
  if svc.is_null() || out_len.is_null() {
    return std::ptr::null();
  }
  let svc = unsafe { &*svc };
  match svc.text.name() {
    Some(bytes) => {
      unsafe { *out_len = bytes.len() as u32 };
      bytes.as_ptr()
    }
    None => {
      unsafe { *out_len = 0 };
      std::ptr::null()
    }
  }
}

/// Returns a pointer to the song comment as raw Shift-JIS bytes and writes the byte length to
/// `*out_len`. The pointer is valid as long as `svc` is alive and unmodified.
/// Returns null if `svc` is null, `out_len` is null, or no comment is set.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
/// `out_len` must be a valid writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_text_comment(
  svc: *const PxtoneService,
  out_len: *mut u32,
) -> *const u8 {
  if svc.is_null() || out_len.is_null() {
    return std::ptr::null();
  }
  let svc = unsafe { &*svc };
  match svc.text.comment() {
    Some(bytes) => {
      unsafe { *out_len = bytes.len() as u32 };
      bytes.as_ptr()
    }
    None => {
      unsafe { *out_len = 0 };
      std::ptr::null()
    }
  }
}

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

/// Returns a pointer to the unit's raw name bytes and writes their byte length to `*out_len`.
/// The pointer is valid as long as `svc` is alive and unmodified.
/// Returns null if `svc` is null, `out_len` is null, or `idx` is out of range.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
/// `out_len` must be a valid writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_unit_name(
  svc: *const PxtoneService,
  idx: u32,
  out_len: *mut u32,
) -> *const u8 {
  if svc.is_null() || out_len.is_null() {
    return std::ptr::null();
  }
  let svc = unsafe { &*svc };
  match svc.units.get(idx as usize) {
    Some(unit) => {
      let name = unit.name();
      unsafe { *out_len = name.len() as u32 };
      name.as_ptr()
    }
    None => {
      unsafe { *out_len = 0 };
      std::ptr::null()
    }
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

/// Returns the tick clock of the event at `idx`, or 0 if `svc` is null or `idx` is out of range.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_get_event_clock(svc: *const PxtoneService, idx: u32) -> i32 {
  if svc.is_null() {
    return 0;
  }
  let svc = unsafe { &*svc };
  svc
    .events
    .records()
    .get(idx as usize)
    .map_or(0, |e| e.clock())
}

/// Returns the unit index of the event at `idx`, or 0 if `svc` is null or `idx` is out of range.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
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

/// Returns the kind of the event at `idx`, or 0 if `svc` is null or `idx` is out of range.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
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

/// Returns the value of the event at `idx`, or 0 if `svc` is null or `idx` is out of range.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`] or null.
#[unsafe(no_mangle)]
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
