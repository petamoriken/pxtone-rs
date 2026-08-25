# pxtone-rs

A Rust decoder for [pxtone](https://pxtone.org/) music files (`.ptcop`,
`.pttune`, `.ptnoise`), ported from the original C++ implementation. Supports
both native Rust usage and WebAssembly via a C FFI interface.

## Features

- Decode and render `.ptcop` / `.pttune` song files to 16-bit PCM audio
- Decode and render `.ptnoise` instrument files to 16-bit PCM audio
- Access song metadata: title, comment, tempo, time signature, units, and events
- WebAssembly build support (no JavaScript glue code; pure C FFI exports)
- Bit exact against the original C++ on every sample in the corpus, and against
  libvorbis for OGGV voices

## Project layout

The `pxtone` crate sits at the root; `libs/` holds the crates it decodes with,
all wired up as path dependencies of the workspace. All of them are `no_std`, as
is `pxtone` itself when built for wasm.

| Crate            | Contents                                                                                         |
| ---------------- | ------------------------------------------------------------------------------------------------ |
| `libs/lewton`    | Vorbis decoder. Fork of [lewton](https://github.com/RustAudio/lewton) 0.10.2, decode only        |
| `libs/ogg`       | Ogg container. Fork of [ogg](https://github.com/RustAudio/ogg) 0.8.0, reads from a byte slice    |
| `libs/lite-math` | Sine, cosine, square root, floor, exponentials and arctangent under libm's names, sized for wasm |

## Usage

### Rust

Add to your `Cargo.toml`:

```toml
[dependencies]
pxtone = { git = "https://github.com/petamoriken/pxtone" }
```

Decode a `.ptcop` or `.pttune` file and render it to raw PCM:

```rust
use pxtone::{DestinationQuality, PxtoneService, VomitPreparation};

let mut service = PxtoneService::new(DestinationQuality::default()).unwrap();
let data = std::fs::read("song.ptcop").unwrap();
service.read(data).unwrap();
service.tones_ready().unwrap();
service.moo_preparation(VomitPreparation::default()).unwrap();

let q = service.get_destination_quality();
let mut buf = vec![0u8; q.channels as usize * 2 * 4096];
loop {
    let written = service.moo(&mut buf);
    if written == 0 { break; }
    // buf[..written] contains 16-bit little-endian interleaved PCM samples
}
```

Decode a `.ptnoise` file:

```rust
use pxtone::{DestinationQuality, PxtoneService};

let mut service = PxtoneService::new(DestinationQuality::default()).unwrap();
let data = std::fs::read("instrument.ptnoise").unwrap();
let wave = service.render_noise(&data).unwrap();
// wave.samples: Vec<u8> of 16-bit LE PCM
// wave.channels: u8
// wave.sample_rate: u32
```

### WebAssembly

Pre-built `pxtone.wasm` binaries are available on the
[Releases page](https://github.com/petamoriken/pxtone-rs/releases).

The module uses fixed-width SIMD, so it needs an engine with the WebAssembly
SIMD proposal: Chrome 91, Firefox 89, Safari 16.4 and Node 16.4 onwards.

To build it yourself, install
[Binaryen](https://github.com/WebAssembly/binaryen) (`brew install binaryen`)
and [Deno](https://deno.com/), then run:

```sh
deno task build:wasm
```

A clang that can target wasm (`brew install llvm`) is optional: `libs/lite-math`
uses it to assemble the `f64.sqrt`, `f64.floor` and `f32.floor` instructions,
which stable Rust cannot emit. Without it the build falls back to portable
implementations of those three functions.

This runs the following pipeline:

| Step | Command                 | Description                                                      |
| ---- | ----------------------- | ---------------------------------------------------------------- |
| 1    | `build:wasm:raw`        | Compiles Rust → `pxtone_raw.wasm`                                |
| 2    | `build:wasm:merge`      | Compiles WAT wrappers and merges them into `pxtone.wasm`         |
| 3    | `build:wasm:strip-impl` | Strips internal `_`-prefixed exports from the binary             |
| 4    | `build:wasm:stub-panic` | Traps in the panic paths and clears the messages they pointed at |
| 5    | `build:wasm:opt`        | Optimizes with `wasm-opt -Oz --converge` for size                |

Panics therefore trap in the wasm build rather than aborting with a message,
which nothing could observe anyway: the module imports nothing to write to.

Target features are set in `.cargo/config.toml`, which a `RUSTFLAGS` in the
environment would replace rather than add to, so leave that variable unset. See
[`DESIGN.md`](DESIGN.md) for the reasoning behind these steps.

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

| Export                    | Signature                                                        | Description                                                    |
| ------------------------- | ---------------------------------------------------------------- | -------------------------------------------------------------- |
| `service_new`             | `(channels: i32, sample_rate: i32) → i32`                        | Create service; returns pointer (null on error)                |
| `service_free`            | `(svc: i32) → void`                                              | Free the service                                               |
| `service_read`            | `(svc: i32, data: i32, len: i32) → i32`                          | Load `.ptcop`/`.pttune` data; 0=OK, -1=error                   |
| `service_tones_ready`     | `(svc: i32) → i32`                                               | Prepare synthesizer tones; 0=OK, -1=error                      |
| `service_moo_preparation` | `(svc: i32, start_sample: i32, unit_mute: i32, loop: i32) → i32` | Prepare playback; 0=OK, -1=error                               |
| `service_moo`             | `(svc: i32, buf: i32, len: i32) → (ptr: i32, written_len: i32)`  | Render next PCM chunk; ptr=0 on error, written_len=0 when done |

**Metadata**

| Export                     | Signature                                                                                                                                 | Description                           |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| `service_get_text_name`    | `(svc: i32) → (ptr: i32, len: i32)`                                                                                                       | Song title as raw Shift-JIS bytes     |
| `service_get_text_comment` | `(svc: i32) → (ptr: i32, len: i32)`                                                                                                       | Song comment as raw Shift-JIS bytes   |
| `service_get_master`       | `(svc: i32) → (ticks_per_beat: i32, beats_per_measure: i32, beat_tempo: f32, measure_count: i32, repeat_measure: i32, last_measure: i32)` | Master settings                       |
| `service_get_unit_count`   | `(svc: i32) → i32`                                                                                                                        | Number of units                       |
| `service_get_unit_name`    | `(svc: i32, idx: i32) → (ptr: i32, len: i32)`                                                                                             | Unit name bytes                       |
| `service_get_unit_played`  | `(svc: i32, idx: i32) → i32`                                                                                                              | 1=active, 0=muted, -1=error           |
| `service_set_unit_played`  | `(svc: i32, idx: i32, played: i32) → i32`                                                                                                 | Set unit active state; 0=OK, -1=error |
| `service_get_event_count`  | `(svc: i32) → i32`                                                                                                                        | Number of events                      |
| `service_get_event`        | `(svc: i32, idx: i32) → (tick: i32, unit_index: i32, kind: i32, value: i32)`                                                              | Event fields                          |

**Stateless**

| Export                 | Signature                                                        | Description                                   |
| ---------------------- | ---------------------------------------------------------------- | --------------------------------------------- |
| `validate`             | `(data: i32, len: i32) → i32`                                    | Validate `.ptcop`/`.pttune`; 0=OK, -1=invalid |
| `validate_noise`       | `(data: i32, len: i32) → i32`                                    | Validate `.ptnoise`; 0=OK, -1=invalid         |
| `service_render_noise` | `(svc: i32, data: i32, len: i32) → (ptr: i32, samples_len: i32)` | Render `.ptnoise` to PCM; ptr=0 on error      |

## Running tests

```sh
# Everything: Rust, WebAssembly, and the comparison against the original
deno task test

# Rust tests. This is `cargo test`, which covers the root crate only; the
# vendored ones need naming (`cargo test -p lite-math`, `-p lewton`, `-p ogg`)
deno task test:rust

# WebAssembly tests. Always go through this task: invoking
# `cargo build --target wasm32-unknown-unknown` directly overwrites
# `target/.../pxtone.wasm` with a module the WAT wrappers were never merged
# into, and the tests then fail on missing exports
deno task test:wasm

# Compare the snapshots against what the original C++ renders
deno task test:refs

# Regenerate the reference WAV and TOML snapshots
UPDATE_SNAPSHOTS=1 cargo test
```

Unit tests live next to the code they cover, in `src/reader.rs`,
`src/service.rs` and `src/pulse/frequency.rs`. Most of them exist to hold the
port bit for bit against the C++, so an optimization that reorders arithmetic
belongs there with a comparison against the previous implementation.

## Design notes

[`DESIGN.md`](DESIGN.md) covers how the port is held to the original C++ and to
libvorbis, what the wasm build does and why, and where the mixing pass spends
its time.

## License

[MIT](LICENSE.md)

The vendored decoders keep their own licenses:
[`libs/lewton`](libs/lewton/LICENSE) is MIT or Apache-2.0, and
[`libs/ogg`](libs/ogg/LICENSE) is BSD-3-Clause.
