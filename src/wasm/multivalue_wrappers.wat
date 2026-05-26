(module
  (import "pxtone" "memory" (memory 0))

  ;; --- service_moo ---
  (import "pxtone" "_service_moo_impl"
    (func $moo_impl (param i32 i32 i32) (result i64)))

  ;; --- service_render_noise ---
  (import "pxtone" "_service_render_noise_impl"
    (func $render_noise_impl (param i32 i32 i32) (result i64)))

  ;; --- service_get_master ---
  (import "pxtone" "_service_get_ticks_per_beat_impl"
    (func $ticks_per_beat (param i32) (result i32)))
  (import "pxtone" "_service_get_beats_per_measure_impl"
    (func $beats_per_measure (param i32) (result i32)))
  (import "pxtone" "_service_get_beat_tempo_impl"
    (func $beat_tempo (param i32) (result f32)))
  (import "pxtone" "_service_get_measure_count_impl"
    (func $measure_count (param i32) (result i32)))
  (import "pxtone" "_service_get_repeat_measure_impl"
    (func $repeat_measure (param i32) (result i32)))
  (import "pxtone" "_service_get_last_measure_impl"
    (func $last_measure (param i32) (result i32)))

  ;; --- service_get_text_name / service_get_text_comment ---
  (import "pxtone" "_service_get_text_name_impl"
    (func $get_text_name_impl (param i32) (result i64)))
  (import "pxtone" "_service_get_text_comment_impl"
    (func $get_text_comment_impl (param i32) (result i64)))

  ;; --- service_get_unit_name ---
  (import "pxtone" "_service_get_unit_name_impl"
    (func $get_unit_name_impl (param i32 i32) (result i64)))

  ;; --- service_get_event ---
  (import "pxtone" "_service_get_event_tick_impl"
    (func $event_tick (param i32 i32) (result i32)))
  (import "pxtone" "_service_get_event_unit_index_impl"
    (func $event_unit_index (param i32 i32) (result i32)))
  (import "pxtone" "_service_get_event_kind_impl"
    (func $event_kind (param i32 i32) (result i32)))
  (import "pxtone" "_service_get_event_value_impl"
    (func $event_value (param i32 i32) (result i32)))

  ;; Returns (ptr: i32, written_len: i32). ptr is null on error; written_len may be
  ;; less than the requested len at the end of the song, and 0 when playback has ended.
  (func (export "service_moo")
    (param $svc i32) (param $buf i32) (param $len i32)
    (result i32 i32)
    (local $r i64)
    (local.set $r (call $moo_impl (local.get $svc) (local.get $buf) (local.get $len)))
    (i32.wrap_i64 (i64.shr_u (local.get $r) (i64.const 32)))
    (i32.wrap_i64 (local.get $r))
  )

  ;; Returns (ptr: i32, samples_len: i32).
  (func (export "service_render_noise")
    (param $svc i32) (param $data i32) (param $data_len i32)
    (result i32 i32)
    (local $r i64)
    (local.set $r
      (call $render_noise_impl
        (local.get $svc) (local.get $data) (local.get $data_len)))
    (i32.wrap_i64 (i64.shr_u (local.get $r) (i64.const 32)))
    (i32.wrap_i64 (local.get $r))
  )

  ;; Returns (ticks_per_beat: i32, beats_per_measure: i32, beat_tempo: f32,
  ;;          measure_count: i32, repeat_measure: i32, last_measure: i32).
  (func (export "service_get_master")
    (param $svc i32)
    (result i32 i32 f32 i32 i32 i32)
    (call $ticks_per_beat (local.get $svc))
    (call $beats_per_measure (local.get $svc))
    (call $beat_tempo (local.get $svc))
    (call $measure_count (local.get $svc))
    (call $repeat_measure (local.get $svc))
    (call $last_measure (local.get $svc))
  )

  ;; Returns (ptr: i32, len: i32) — raw Shift-JIS bytes of the song title.
  (func (export "service_get_text_name")
    (param $svc i32)
    (result i32 i32)
    (local $r i64)
    (local.set $r (call $get_text_name_impl (local.get $svc)))
    (i32.wrap_i64 (i64.shr_u (local.get $r) (i64.const 32)))
    (i32.wrap_i64 (local.get $r))
  )

  ;; Returns (ptr: i32, len: i32) — raw Shift-JIS bytes of the song comment.
  (func (export "service_get_text_comment")
    (param $svc i32)
    (result i32 i32)
    (local $r i64)
    (local.set $r (call $get_text_comment_impl (local.get $svc)))
    (i32.wrap_i64 (i64.shr_u (local.get $r) (i64.const 32)))
    (i32.wrap_i64 (local.get $r))
  )

  ;; Returns (ptr: i32, len: i32) — raw name bytes of the unit at `idx`.
  (func (export "service_get_unit_name")
    (param $svc i32) (param $idx i32)
    (result i32 i32)
    (local $r i64)
    (local.set $r (call $get_unit_name_impl (local.get $svc) (local.get $idx)))
    (i32.wrap_i64 (i64.shr_u (local.get $r) (i64.const 32)))
    (i32.wrap_i64 (local.get $r))
  )

  ;; Returns (tick: i32, unit_index: i32, kind: i32, value: i32) for the event at `idx`.
  (func (export "service_get_event")
    (param $svc i32) (param $idx i32)
    (result i32 i32 i32 i32)
    (call $event_tick (local.get $svc) (local.get $idx))
    (call $event_unit_index (local.get $svc) (local.get $idx))
    (call $event_kind (local.get $svc) (local.get $idx))
    (call $event_value (local.get $svc) (local.get $idx))
  )
)
