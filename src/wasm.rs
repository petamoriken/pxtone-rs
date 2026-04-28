use crate::service::{PxtoneService, VomitPreparation};
use std::alloc::{Layout, alloc as sys_alloc, dealloc as sys_dealloc};
use std::io::Cursor;

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
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_moo_preparation(svc: *mut PxtoneService) -> i32 {
  if svc.is_null() {
    return -1;
  }
  let svc = unsafe { &mut *svc };
  match svc.moo_preparation(VomitPreparation::default()) {
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
/// Writes channel count, sample rate, and byte length to the respective `out_*` pointers.
/// Returns null on failure. All `out_*` pointers must be non-null.
///
/// # Safety
/// `svc` must be a valid pointer from [`service_new`].
/// `data` must be valid for `data_len` bytes.
/// `out_channels`, `out_sample_rate`, and `out_samples_len` must be valid writable pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn service_render_noise(
  svc: *mut PxtoneService,
  data: *const u8,
  data_len: usize,
  out_channels: *mut u32,
  out_sample_rate: *mut u32,
  out_samples_len: *mut u32,
) -> *mut u8 {
  if svc.is_null()
    || data.is_null()
    || out_channels.is_null()
    || out_sample_rate.is_null()
    || out_samples_len.is_null()
  {
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
      *out_channels = wave.channels as u32;
      *out_sample_rate = wave.sample_rate;
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
    *out_channels = wave.channels as u32;
    *out_sample_rate = wave.sample_rate;
    *out_samples_len = len as u32;
  }
  ptr
}
