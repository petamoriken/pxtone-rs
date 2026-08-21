# pxtone-rs

A Rust decoder for [pxtone](https://pxtone.org/) music files (`.ptcop`,
`.pttune`, `.ptnoise`), ported from the original C++ implementation. Supports
both native Rust usage and WebAssembly via a C FFI interface.

## Features

- Decode and render `.ptcop` / `.pttune` song files to 16-bit PCM audio
- Decode and render `.ptnoise` instrument files to 16-bit PCM audio
- Access song metadata: title, comment, tempo, time signature, units, and events
- WebAssembly build support (no JavaScript glue code; pure C FFI exports)

## Project layout

The `pxtone` crate sits at the root; `libs/` holds the crates it decodes with,
all wired up as path dependencies of the workspace.

| Crate            | Contents                                                                                                |
| ---------------- | ------------------------------------------------------------------------------------------------------- |
| `libs/lewton`    | Vorbis decoder. Fork of [lewton](https://github.com/RustAudio/lewton) 0.10.2, `no_std`, decode only     |
| `libs/ogg`       | Ogg container. Fork of [ogg](https://github.com/RustAudio/ogg) 0.8.0, `no_std`, reads from a byte slice |
| `libs/lite-math` | `f32` sine, cosine, square root, floor, exponentials and arctangent, sized for the wasm build           |

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

To build the `.wasm` binary yourself, install
[Binaryen](https://github.com/WebAssembly/binaryen) (`brew install binaryen`)
and [Deno](https://deno.com/), then run:

```sh
deno task build:wasm
```

A clang that can target wasm (`brew install llvm`) is optional: `libs/lite-math`
uses it to assemble the `f32.sqrt` and `f32.floor` instructions, which stable
Rust cannot emit. Without it the build falls back to portable implementations of
those two functions.

This runs the following pipeline:

| Step | Command                 | Description                                                      |
| ---- | ----------------------- | ---------------------------------------------------------------- |
| 1    | `build:wasm:raw`        | Compiles Rust → `pxtone_raw.wasm`                                |
| 2    | `build:wasm:merge`      | Compiles WAT wrappers and merges them into `pxtone.wasm`         |
| 3    | `build:wasm:strip-impl` | Strips internal `_`-prefixed exports from the binary             |
| 4    | `build:wasm:stub-panic` | Traps in the panic paths and clears the messages they pointed at |
| 5    | `build:wasm:opt`        | Optimizes with `wasm-opt -Oz --converge`                         |

Panics therefore trap in the wasm build rather than aborting with a message,
which nothing could observe anyway: the module imports nothing to write to.

The last step optimizes for size rather than speed, because the module is meant
to be base64'd into a JavaScript bundle. `-Oz --converge` is 3,319 bytes smaller
than `-O3` and costs at most 1.2% of `moo` time on the sample songs — running
Binaryen's size passes over output LLVM already compiled at `-O3` is nothing
like lowering the Rust `opt-level`. Note also that the `release-wasm` profile
sets `strip = "debuginfo"` rather than stripping everything, since step 4 needs
the name section to find the panic entry points.

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

## Running Tests

```sh
# Rust tests
deno task test:rust

# WebAssembly tests
deno task test:wasm

# Regenerate the reference WAV and TOML snapshots
UPDATE_SNAPSHOTS=1 cargo test
```

`deno task test:rust` runs `cargo test`, which covers the root `pxtone` crate
only; the vendored crates need naming explicitly (`cargo test -p lite-math`,
`-p lewton`, `-p ogg`). Unit tests live next to the code they cover, in
`src/reader.rs`, `src/sort.rs`, `src/service.rs` and `src/pulse/frequency.rs`.
Most of them exist to hold the port bit for bit against the C++, so an
optimization that reorders arithmetic belongs there with a comparison against
the previous implementation.

Always go through `deno task test:wasm`. Invoking
`cargo build --target wasm32-unknown-unknown` directly overwrites
`target/.../pxtone.wasm` with a module the WAT wrappers were never merged into,
and the tests then fail on missing exports.

## Performance

```sh
# One module for absolute timings, two to compare them
deno run --allow-read tools/bench_wasm.ts <wasm> [baseline_wasm]
```

The benchmark renders three of the sample songs to completion and reports the
median of ten runs. For load time or a per-function breakdown, profile a native
release build instead: put a harness under `examples/` and run it under macOS
`sample`. Everything inlines into `main` there, so getting attribution means
replacing `#[inline(always)]` and `#[inline]` with `#[inline(never)]` across
`src/`, which makes `moo` 2.1x slower but keeps the proportions readable. Trust
only the functions with substantial bodies: a helper of a few instructions looks
expensive once every call to it is real.

`moo` has no single hot spot left — `tone_sample` 22%, `step_advance` 15%,
`tone_supple` 8%, `step_envelope` 8%, `get_frame` 6%, the delay effect 5%, the
rest below 5% each. What used to sit alongside them, the frequency table lookup
and the portamento step, is gone: everything the mixing pass reads off a unit
holds for a whole block, so `ToneParams` reads it once instead of once per
sample.

### Optimizations considered and rejected

| Option                                           | Result                                         |
| ------------------------------------------------ | ---------------------------------------------- |
| `f32`/`f64` `algebraic_*` (Rust 1.98)            | 37 bytes smaller, time within noise            |
| `-C target-feature=+simd128`                     | 1,385 bytes larger, up to 3.7% slower          |
| `wasm-opt --low-memory-unused`                   | 1,925 bytes smaller, but unsound here          |
| `wasm-opt -O4`                                   | Larger than `-O3`                              |
| `opt-level = "s"` / `"z"` for the `pxtone` crate | 5.6KB / 10.4KB smaller, `moo` 22% / 78% slower |

wasm has no scalar FMA, so the algebraic operators have no contraction to
perform, and enabling simd128 does not help either: of the 560 v128 instructions
LLVM then emits, 479 are `v128.load`, `v128.store` and `v128.const` — widened
memory moves that bulk memory already covers — and the arithmetic amounts to
eight `i32x4.add` with no float lanes at all. The mixing loop is integer work in
which each sample depends on the state the previous one left behind.
`libs/lite-math` additionally documents bit identical results on every platform,
which a deliberately non-deterministic optimization cannot promise.

`--low-memory-unused` is out because rustc links the shadow stack first:
`__stack_pointer` starts at 1 MiB with the data segment above it, so the low
page is the bottom of the stack rather than unused, and the flag would quietly
compromise stack overflow detection.

## License

[MIT](LICENSE.md)

The vendored decoders keep their own licenses:
[`libs/lewton`](libs/lewton/LICENSE) is MIT or Apache-2.0, and
[`libs/ogg`](libs/ogg/LICENSE) is BSD-3-Clause.
