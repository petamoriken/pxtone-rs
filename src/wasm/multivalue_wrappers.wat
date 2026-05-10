(module
  (import "pxtone" "memory" (memory 0))
  (import "pxtone" "alloc" (func $alloc (param i32) (result i32)))
  (import "pxtone" "dealloc" (func $dealloc (param i32 i32)))

  ;; --- service_render_noise ---
  (import "pxtone" "_service_render_noise_impl"
    (func $render_noise_impl (param i32 i32 i32 i32) (result i32)))

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
    (func $get_text_name_impl (param i32 i32) (result i32)))
  (import "pxtone" "_service_get_text_comment_impl"
    (func $get_text_comment_impl (param i32 i32) (result i32)))

  ;; --- service_get_unit_name ---
  (import "pxtone" "_service_get_unit_name_impl"
    (func $get_unit_name_impl (param i32 i32 i32) (result i32)))

  ;; --- service_get_event ---
  (import "pxtone" "_service_get_event_tick_impl"
    (func $event_tick (param i32 i32) (result i32)))
  (import "pxtone" "_service_get_event_unit_index_impl"
    (func $event_unit_index (param i32 i32) (result i32)))
  (import "pxtone" "_service_get_event_kind_impl"
    (func $event_kind (param i32 i32) (result i32)))
  (import "pxtone" "_service_get_event_value_impl"
    (func $event_value (param i32 i32) (result i32)))

  ;; Returns (ptr: i32, samples_len: i32).
  (func (export "service_render_noise")
    (param $svc i32) (param $data i32) (param $data_len i32)
    (result i32 i32)
    (local $buf i32)
    (local $ptr i32)
    (local $len i32)
    (local.set $buf (call $alloc (i32.const 4)))
    (local.set $ptr
      (call $render_noise_impl
        (local.get $svc) (local.get $data) (local.get $data_len) (local.get $buf)))
    (local.set $len (i32.load (local.get $buf)))
    (call $dealloc (local.get $buf) (i32.const 4))
    (local.get $ptr)
    (local.get $len)
  )

  ;; Returns (ticks_per_beat, beats_per_measure, beat_tempo, measure_count,
  ;;          repeat_measure, last_measure).
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
    (local $buf i32)
    (local $ptr i32)
    (local $len i32)
    (local.set $buf (call $alloc (i32.const 4)))
    (local.set $ptr (call $get_text_name_impl (local.get $svc) (local.get $buf)))
    (local.set $len (i32.load (local.get $buf)))
    (call $dealloc (local.get $buf) (i32.const 4))
    (local.get $ptr)
    (local.get $len)
  )

  ;; Returns (ptr: i32, len: i32) — raw Shift-JIS bytes of the song comment.
  (func (export "service_get_text_comment")
    (param $svc i32)
    (result i32 i32)
    (local $buf i32)
    (local $ptr i32)
    (local $len i32)
    (local.set $buf (call $alloc (i32.const 4)))
    (local.set $ptr (call $get_text_comment_impl (local.get $svc) (local.get $buf)))
    (local.set $len (i32.load (local.get $buf)))
    (call $dealloc (local.get $buf) (i32.const 4))
    (local.get $ptr)
    (local.get $len)
  )

  ;; Returns (ptr: i32, len: i32) — raw name bytes of the unit at `idx`.
  (func (export "service_get_unit_name")
    (param $svc i32) (param $idx i32)
    (result i32 i32)
    (local $buf i32)
    (local $ptr i32)
    (local $len i32)
    (local.set $buf (call $alloc (i32.const 4)))
    (local.set $ptr
      (call $get_unit_name_impl (local.get $svc) (local.get $idx) (local.get $buf)))
    (local.set $len (i32.load (local.get $buf)))
    (call $dealloc (local.get $buf) (i32.const 4))
    (local.get $ptr)
    (local.get $len)
  )

  ;; Returns (tick, unit_index, kind, value) for the event at `idx`.
  (func (export "service_get_event")
    (param $svc i32) (param $idx i32)
    (result i32 i32 i32 i32)
    (call $event_tick (local.get $svc) (local.get $idx))
    (call $event_unit_index (local.get $svc) (local.get $idx))
    (call $event_kind (local.get $svc) (local.get $idx))
    (call $event_value (local.get $svc) (local.get $idx))
  )
)
