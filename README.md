# pxtone-rs

A Rust decoder for [pxtone](https://pxtone.org/) music files (`.ptcop`,
`.pttune`, `.ptnoise`), ported from the original C++ implementation. Supports
both native Rust usage and WebAssembly via a C FFI interface.

## Features

- Decode and render `.ptcop` / `.pttune` song files to 16-bit PCM audio
- Decode and render `.ptnoise` instrument files to 16-bit PCM audio
- Access song metadata: title, comment, tempo, time signature, units, and events
- WebAssembly build support (no JavaScript glue code; pure C FFI exports)

## Usage

### Rust

Add to your `Cargo.toml`:

```toml
[dependencies]
pxtone = { git = "https://github.com/petamoriken/pxtone" }
```

Decode a `.ptcop` or `.pttune` file and render it to raw PCM:

```rust
use pxtone::{PxtoneService, VomitPreparation};
use std::fs::File;
use std::io::BufReader;

let mut service = PxtoneService::new();
let mut reader = BufReader::new(File::open("song.ptcop").unwrap());
service.read(&mut reader).unwrap();
service.tones_ready().unwrap();
service.moo_preparation(VomitPreparation::default()).unwrap();

let q = service.get_destination_quality();
let mut buf = vec![0u8; q.channels as usize * 2 * 4096];
while !service.is_end_vomit() {
    service.moo(&mut buf);
    // buf contains 16-bit little-endian interleaved PCM samples
}
```

Decode a `.ptnoise` file:

```rust
use pxtone::PxtoneService;
use std::fs::File;
use std::io::BufReader;

let mut service = PxtoneService::new();
let mut reader = BufReader::new(File::open("instrument.ptnoise").unwrap());
let wave = service.render_noise(&mut reader).unwrap();
// wave.samples: Vec<u8> of 16-bit LE PCM
// wave.channels: u8
// wave.sample_rate: u32
```

### WebAssembly

Pre-built `pxtone.wasm` binaries are available on the
[Releases page](https://github.com/petamoriken/pxtone-rs/releases).

To build the `.wasm` binary yourself, install
[Binaryen](https://github.com/WebAssembly/binaryen) (`brew install binaryen`)
and [Deno](https://deno.com/), then run:

```sh
deno task build:wasm
```

This runs the following pipeline:

| Step | Command                 | Description                                              |
| ---- | ----------------------- | -------------------------------------------------------- |
| 1    | `build:wasm:raw`        | Compiles Rust → `pxtone_raw.wasm`                        |
| 2    | `build:wasm:merge`      | Compiles WAT wrappers and merges them into `pxtone.wasm` |
| 3    | `build:wasm:strip-impl` | Strips internal `_`-prefixed exports from the binary     |
| 4    | `build:wasm:opt`        | Optimizes with `wasm-opt -O3`                            |

The compiled module exports a C FFI interface with WebAssembly multi-value
returns. Memory management uses explicit `alloc`/`dealloc` exports. See
[`src/wasm/mod.rs`](src/wasm/mod.rs) for the Rust source and
[`tests/wasm_test.ts`](tests/wasm_test.ts) for usage examples.

#### WASM API overview

**Memory**

| Export    | Signature                      | Description                            |
| --------- | ------------------------------ | -------------------------------------- |
| `alloc`   | `(size: i32) → i32`            | Allocate `size` bytes; returns pointer |
| `dealloc` | `(ptr: i32, size: i32) → void` | Free a buffer allocated by `alloc`     |

**Service lifecycle**

| Export                    | Signature                                    | Description                                     |
| ------------------------- | -------------------------------------------- | ----------------------------------------------- |
| `service_new`             | `(channels: i32, sample_rate: i32) → i32`    | Create service; returns pointer (null on error) |
| `service_free`            | `(svc: i32) → void`                          | Free the service                                |
| `service_read`            | `(svc, data, len) → i32`                     | Load `.ptcop`/`.pttune` data; 0=OK, -1=error    |
| `service_tones_ready`     | `(svc) → i32`                                | Prepare synthesizer tones; 0=OK, -1=error       |
| `service_moo_preparation` | `(svc, start_sample, unit_mute, loop) → i32` | Prepare playback; 0=OK, -1=error                |
| `service_moo`             | `(svc, buf, len) → i32`                      | Render next PCM chunk; 1=wrote samples, 0=done  |

**Metadata**

| Export                     | Signature                                                                                                   | Description                           |
| -------------------------- | ----------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| `service_get_text_name`    | `(svc) → (ptr: i32, len: i32)`                                                                              | Song title as raw Shift-JIS bytes     |
| `service_get_text_comment` | `(svc) → (ptr: i32, len: i32)`                                                                              | Song comment as raw Shift-JIS bytes   |
| `service_get_master`       | `(svc) → (ticks_per_beat, beats_per_measure, beat_tempo: f32, measure_count, repeat_measure, last_measure)` | Master settings                       |
| `service_get_unit_count`   | `(svc) → i32`                                                                                               | Number of units                       |
| `service_get_unit_name`    | `(svc, idx) → (ptr: i32, len: i32)`                                                                         | Unit name bytes                       |
| `service_get_unit_played`  | `(svc, idx) → i32`                                                                                          | 1=active, 0=muted, -1=error           |
| `service_set_unit_played`  | `(svc, idx, played) → i32`                                                                                  | Set unit active state; 0=OK, -1=error |
| `service_get_event_count`  | `(svc) → i32`                                                                                               | Number of events                      |
| `service_get_event`        | `(svc, idx) → (tick, unit_index, kind, value: i32)`                                                         | Event fields                          |

**Stateless**

| Export                 | Signature                                         | Description                                   |
| ---------------------- | ------------------------------------------------- | --------------------------------------------- |
| `validate`             | `(data, len) → i32`                               | Validate `.ptcop`/`.pttune`; 0=OK, -1=invalid |
| `validate_noise`       | `(data, len) → i32`                               | Validate `.ptnoise`; 0=OK, -1=invalid         |
| `service_render_noise` | `(svc, data, len) → (ptr: i32, samples_len: i32)` | Render `.ptnoise` to PCM; ptr=0 on error      |

## Running Tests

```sh
# Rust tests
deno task test:rust

# WebAssembly tests
deno task test:wasm
```

## License

[MIT](LICENSE.md)
