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

## Checking against the original

The snapshots under `tests/snapshots` are this port's own output, so on their
own they only catch regressions -- never a decode that was wrong from the start.
`tests/reference` holds what the original C++ implementation renders for the
same inputs, and diffing the two is what turns the corpus into a correctness
check:

```sh
deno task test:refs           # both suites
deno task test:refs ptnoise   # one of them
```

Both sides are committed WAV files, so this needs nothing but Deno and runs in
CI. Every snapshot matches its reference render sample for sample, so the check
is exact: any difference at all fails, because it means the decode has drifted
from the original. The `ogg` suite is the same idea with libvorbis standing in
for the C++, since that is what the C++ decodes an OGGV voice with.

Songs are stored as their first five seconds, which is where every difference
found so far begins; the instruments are short enough to keep whole. See
[`tests/reference/README.md`](tests/reference/README.md) for how that side is
produced -- the C++ is not vendored, so regenerating it is a manual step.

### The stepped noise waves are filled by walking the boundaries

`Saw6` and `Saw8` are staircases, and the C++ builds them by walking the step
boundaries and filling up to each -- `a = _smp_num * k / n`, truncated. Picking
the step per entry with `s * n / _smp_num` rounds the other way, so the entry
sitting exactly on a boundary lands in the previous step: 7 of `Saw8`'s 8
boundaries on a 441 entry table, each 9362 out. None of the committed noise
instruments uses either wave, which is how that survived to be found in a song
instead.

### Floor 0 follows libvorbis' factorization, not the spec's

No encoder emits Vorbis floor 0 -- vorbisenc has only ever written floor 1, and
all 23 streams to hand use it -- so nothing in the corpus reaches that code. It
still has to agree with libvorbis, and it did not: the spec factors the curve as
`(1-cos w)/2` times a product of `4(cos c - cos w)^2` terms while
`vorbis_lsp_to_curve` factors it around `w = 2 cos w` and squares the products,
and the two agree only to about 1e-6. `libs/lewton` now carries libvorbis'
arrangement, down to where the precision changes, and its bark map is libvorbis'
integer bins rather than a cosine per spectral line -- the map is floored to an
integer, so an f32 arctangent behind it moves a bin boundary rather than a last
bit. Bit exact across 160 configurations, with one of each filter parity pinned
in `libs/lewton/src/audio_floor0_test.rs`.

What is still unmeasured there is the bitstream side: reading the amplitude, the
book number and the coefficients. Nothing produces a stream to read.

### OGGV voices are held against libvorbis

pxtone decodes an OGGV voice with libvorbis'
`ov_read( &vf, pcmout, 4096, 0, 2,
1, &sec )`, and `libs/lewton` is a
reimplementation of Vorbis rather than a port of libvorbis, so agreement there
has to be built rather than inherited.

libvorbis' dylib exports the symbols its own translation units share, which is
what makes the comparison possible without the source:

```sh
nm -gU /opt/homebrew/opt/libvorbis/lib/libvorbis.dylib | grep -iE "window|mdct"
```

Declaring `_vorbis_window_get` and `mdct_init` reads back the exact window and
twiddle tables even without the source, and `ov_read_float` alongside `ov_read`
pins the float-to-i16 conversion down to its tie-breaking. What that turned up:
the conversion is `floor(f * 32768 + 0.5)` clamped to `[-32768, 32767]`, halves
going toward positive infinity; a stream's last packet is trimmed against the
samples handed out rather than against the last page's granule position; and
both the window and the MDCT twiddle factors are computed in double and stored
as floats, down to how the products are grouped. The twiddle factors now match
at every blocksize, and `libs/lewton/src/header_cached_test.rs` holds them there
alongside the transform's own output.

The MDCT is a port of `lib/mdct.c`'s backward transform rather than the
stb_vorbis one lewton carried, because only the same operation order gives the
same floats; it is bit exact against that file at every blocksize. And where
libvorbis' window tables are literals in `window.c` that no computation
reproduces, the 151 entries of 8160 that differ are carried as they are -- worth
only 1e-10 each, but a table is meant to be the same table. Decoded voices are
now bit identical to libvorbis as floats, which also says the floor, residue,
coupling, windowing and overlap-add around the MDCT were already right.

Matching it means compiling the C with `-ffp-contract=off`, which is not a
detail. Clang fuses `a*b + c*d` by default on a target with a fused
multiply-add, so a libvorbis built that way sits about 2e-6 from the C it was
built from. This port follows the C, which is what a build without one gives --
every x86-64 SSE2 build, and wasm, where there is no scalar fused multiply-add
to fuse into. `tests/reference/ogg` is generated accordingly.

`deno task test:rust` runs `cargo test`, which covers the root `pxtone` crate
only; the vendored crates need naming explicitly (`cargo test -p lite-math`,
`-p lewton`, `-p ogg`). Unit tests live next to the code they cover, in
`src/reader.rs`, `src/service.rs` and `src/pulse/frequency.rs`. Most of them
exist to hold the port bit for bit against the C++, so an optimization that
reorders arithmetic belongs there with a comparison against the previous
implementation.

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

Ablation says the remainder is spread thin. Disabling the whole unit loop leaves
4.5 of 35 ns a frame, so 87 to 95% of the pass is in there and 58 to 78% is the
voice loop inside it, at 2.4 to 3.0 ns per live voice. Within that, no piece is
worth much on its own: the velocity/volume/pan chain can be **run twice over for
free**, so there is no shortage of ALU slots, while breaking the sequential walk
through the wave costs 28%. What still buys anything is not touching `self` from
inside the sample loop.

### What the sample loop must not read

The wins left in the mixing pass all have the same shape: a value that holds for
a whole block, loaded back through `&mut self` on every sample. wasm pays
several times what a native build does for one — LLVM keeps the field in a
register there and the engine's allocator often does not — so measure both.

- The **effects run effect by effect** rather than sample by sample. An
  overdrive is stateless and each delay still walks its ring in sample order, so
  every sample sees the same sequence, all the overdrives then the delays in
  order. What changes is that the group index, the rate, the ring offset and the
  buffer bound are read once for the block. Each effect owns the sample loop
  itself and is `#[inline(never)]`, which is both smaller and faster than
  inlining it into the two `GROUPS` instantiations of the caller.
- **`is_flushed` is taken from what the frame did** instead of read back.
  Rendering a frame clears the quiet run, so the unit is not flushed.

Together: 2,658 bytes smaller and `moo` 8 to 13% faster on wasm, 3 to 11% on
native.

### Optimizations considered and rejected

| Option                                           | Result                                         |
| ------------------------------------------------ | ---------------------------------------------- |
| `f32`/`f64` `algebraic_*` (Rust 1.98)            | 37 bytes smaller, time within noise            |
| `-C target-feature=+simd128`                     | 3,799 bytes larger, `moo` within noise         |
| Fixed width SIMD in `libs/lite-math`             | Nothing to speed up: `moo` never calls it      |
| `wasm-opt --low-memory-unused`                   | 1,925 bytes smaller, but unsound here          |
| `wasm-opt -O4`                                   | Larger than `-O3`                              |
| `opt-level = "s"` / `"z"` for the `pxtone` crate | 5.6KB / 10.4KB smaller, `moo` 22% / 78% slower |

wasm has no scalar FMA, so the algebraic operators have no contraction to
perform, and enabling simd128 does not help either: of the 763 v128 instructions
LLVM then emits, 550 are `v128.load`, `v128.store` and `v128.const` — widened
memory moves that bulk memory already covers — and the float arithmetic amounts
to eight `f32x4.div` and eight lane conversions. The mixing loop is integer work
in which each sample depends on the state the previous one left behind.

Vectorizing `libs/lite-math` by hand does not pay either, and not for a reason
of precision: lane wise IEEE multiplies and adds are the same operations in the
same order, so a two lane version of these series is bit identical to the scalar
one, which is what separates this from the algebraic operators above. It is that
there is no time there to win. Counting the calls shows **`moo` reaching
`lite-math` zero times** on every sample song — the frequency table is built
from literal octave bases and the mixing pass holds no transcendental at all.
Every call happens while loading: 1,600 to 90,000 of them per song, which is 0.2
to 1.0 ms of `tones_ready` against 30 to 112 ms of `moo`. `sin` costs 4.79 ns a
call, 2.98 ns once its three `#[inline(never)]` hops collapse into one, and 1.96
ns as a branch free body that the caller's loop can keep two arguments in flight
through — so the entire headroom is about one nanosecond times at most 90,000
calls, under 0.1 ms per file loaded. The `#[inline]` step alone costs 366 bytes
of wasm, and the `wide` crate gates its wasm backend on
`cfg(target_feature = "simd128")`, so reaching it at all means the whole-build
flag in the row above.

The overtone oscillator, which makes most of those calls, cannot use wider lanes
in place anyway: it sums the harmonics into one accumulator, and that order is
what the C++ fixes. Lanes would have to run across output samples instead, one
accumulator each.

`--low-memory-unused` is out because rustc links the shadow stack first:
`__stack_pointer` starts at 1 MiB with the data segment above it, so the low
page is the bottom of the stack rather than unused, and the flag would quietly
compromise stack overflow detection.

## License

[MIT](LICENSE.md)

The vendored decoders keep their own licenses:
[`libs/lewton`](libs/lewton/LICENSE) is MIT or Apache-2.0, and
[`libs/ogg`](libs/ogg/LICENSE) is BSD-3-Clause.
