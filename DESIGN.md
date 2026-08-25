# Design notes

How this port is held to the original C++, and what the mixing pass costs. For
installing and using the library, see [`README.md`](README.md).

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
`ov_read( &vf, pcmout, 4096, 0, 2, 1, &sec )`, and `libs/lewton` is a
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

## The wasm build

### Target features live in `.cargo/config.toml`

`+multivalue,+bulk-memory,+nontrapping-fptoint,+sign-ext,+mutable-globals,+simd128`
is passed through `[target.wasm32-unknown-unknown] rustflags`. **A `RUSTFLAGS`
in the environment replaces that list rather than adding to it**, so no build
task may set one: `build:wasm:raw` used to pass a `--remap-path-prefix`, which
silently disabled every feature above. The host paths that flag was there for
are removed by `strip = "debuginfo"` and the panic stubbing step instead.

### `no_std` on wasm

`src/lib.rs` is `#![cfg_attr(target_family = "wasm", no_std)]`. Only wasm,
because `crate-type` cannot vary per target and a `no_std` `cdylib` on a native
target would need its own panic handler and allocator. Imports come from `alloc`
either way, so the wasm build is what decides what the crate may use. The crates
under `libs/` were already `no_std`.

The panic handler traps, and `tools/wasm_stub_panic.ts` still runs: a message is
built even though the handler ignores it. Dropping `std` is worth 268 bytes,
which is small because that step was already removing most of it -- the value is
in the guarantee, not the size.

### Size over speed, except where it is not

The module is meant to be base64'd into a JavaScript bundle, so the last step
optimizes for size: `-Oz --converge` is 3,319 bytes smaller than `-O3` and costs
at most 1.2% of `moo` time -- running Binaryen's size passes over output LLVM
already compiled at `-O3` is nothing like lowering the Rust `opt-level`, which
costs 22% to 78%. simd128 is the one trade the other way: 4,575 bytes for 12 to
15% of `moo`.

## Performance

```sh
# One module for absolute timings, two to compare them
deno run --allow-read tools/bench_wasm.ts <wasm> [baseline_wasm]

# The phases before playback, over the whole sample corpus
deno run --allow-read tools/bench_load.ts <wasm> [baseline_wasm]
```

The benchmark renders three of the sample songs to completion and reports the
median of ten runs. `bench_load.ts` takes the same arguments and times
`service_read`, `service_tones_ready` and `service_render_noise` over the 6
songs and 47 noise designs under `tests/sample`, the median of 21 runs a file
summed over the corpus. For a per-function breakdown, profile a native release
build instead: put a harness under `examples/` and run it under macOS `sample`.
Everything inlines into `main` there, so getting attribution means replacing
`#[inline(always)]` and `#[inline]` with `#[inline(never)]` across `src/`, which
makes `moo` 2.1x slower but keeps the proportions readable. Trust only the
functions with substantial bodies: a helper of a few instructions looks
expensive once every call to it is real.

### The sample loop is not short of arithmetic

Ablation says the cost is spread thin and none of it is ALU. Disabling the whole
unit loop leaves 4.5 of 35 ns a frame, so 87 to 95% of the pass is in there and
58 to 78% is the voice loop inside it, at 2.4 to 3.0 ns per live voice. Within
that, no piece is worth much on its own: the velocity/volume/pan chain can be
**run twice over for free**, while breaking the sequential walk through the wave
costs 28%.

So a voice's mixing chain cannot be vectorized, and not for want of trying to
fit it: there is no arithmetic to remove, its sample position is a sequential
`f64` recurrence, and reading the wave is a gather. Lanes cannot run across
samples at all.

What buys anything is not touching `self` from inside the sample loop. wasm pays
several times what a native build does for one such load -- LLVM keeps the field
in a register there and the engine's allocator often does not -- so measure
both.

- **Block invariants are read once** into `ToneParams`: everything the mixing
  pass reads off a unit holds for a whole block, including the instrument's
  envelope flag and body length.
- **The effects run effect by effect** rather than sample by sample. An
  overdrive is stateless and each delay still walks its ring in sample order, so
  every sample sees the same sequence, all the overdrives then the delays in
  order. What changes is that the group index, the rate, the ring offset and the
  buffer bound are read once for the block. Each effect owns the sample loop
  itself and is `#[inline(never)]`.
- **`is_flushed` is taken from what the frame did** instead of read back.
  Rendering a frame clears the quiet run, so the unit is not flushed.

### The mixer is a set of planes

Everything downstream of the units is a per-sample independent pipeline, and
that part does vectorize -- once it is laid out for it. Disabling each stage in
turn puts about 20% of `moo` there:

| Stage disabled                       | orche | nes   | 5LOVE |
| ------------------------------------ | ----- | ----- | ----- |
| `tone_supple`                        | -6.5% | -7.7% | -5.3% |
| The effects (delays)                 | -7.5% | -6.1% | -6.8% |
| `moo_output` and the frame writes    | -4.2% | -7.3% | -8.7% |
| Narrowing the accumulator to 1 group | -3.3% | -3.8% | --    |

The accumulator used to be `mix[sample][channel][group]`, which every one of
those walked with a 56 byte stride. It is now one contiguous plane of samples
per (channel, group):

- **A unit drains a run at a time.** The pan-delay ring holds 64 samples, so a
  run longer than the slots left before it turns over would overwrite history
  the later reads still need; rendering `64 - pan_delay` samples and then
  draining them keeps every read standing and leaves the drain a contiguous walk
  of both sides. Split at the ring's wrap it is two plain adds.
- **The effects and the output sum walk a plane straight through.**
- **The group count is a runtime value again.** Resolving a group is one slice
  per block rather than an index per sample, so the two `const GROUPS`
  instantiations are gone and only the planes a song uses are cleared, mixed and
  summed. A bin plane for group numbers past the last one keeps that test out of
  the sample loop as well.

Native gets 13 to 17% out of that on its own (SSE2 vectorizes the plane loops
with no flag), wasm 3.5 to 6% and 1,838 bytes. With simd128 on, wasm gets 14.9,
21.7 and 15.9% on the three sample songs.

### Loading is the noise render and the event list

`moo` is where a player spends its time, but nothing sounds until a file has
been read, its voices readied and its noise designs rendered. Over the corpus:

| Phase                  | Before  | Now     |
| ---------------------- | ------- | ------- |
| `service_read`         | 3.14ms  | 0.72ms  |
| `service_tones_ready`  | 7.80ms  | 6.31ms  |
| `service_render_noise` | 38.49ms | 16.07ms |

Native `sample` puts 92% of a noise render inside `build_noise` and 40% of
reading a song inside `read_x4x_block`, 28 of those points in the `memmove`
behind `Vec::insert`. Both turned out to be shape rather than arithmetic.

- **A noise design renders a unit at a time.** The loops were frames outside and
  units inside, so every frame read three oscillators and an envelope back out
  of the unit list. A unit now runs 1,024 frames at a stretch with its state in
  locals, accumulating into an `f64` block. The sum is a left fold from zero
  either way, so the units are added in the same order they were. Worth 40.7% of
  the phase.
- **A frame is computed once for both channels.** The oscillators do not advance
  between the channels, and the pan enters last, so only the pan was ever
  different. Worth 22.2%.
- **An oscillator resolves its wave table once**, rather than unwrapping
  `tables[wave_type]` per sample, and the units a design disables are dropped
  when the states are built. Worth 9.3%.
- **An x4x block is merged, not inserted.** Each block holds one (unit, kind)
  pair with non-negative tick deltas, which makes it a sorted run going into a
  sorted list; inserting it a record at a time moves 17.2M records for
  `overworld2_orche` against 0.17M copied by a merge. The order of events
  sharing a tick is audible, so `merging_a_block_matches_inserting_its_records`
  holds the merge to what the insertions produced, replacements and all. Worth
  76.1% of the phase and 1,732 bytes.

Together they take the corpus from 49.5ms to 23.3ms for 2,510 bytes, most of it
the merge; the rest is 534 bytes for the noise loops and 295 back from folding
`lite-math`'s sine into one function.

### Optimizations considered and rejected

| Option                                           | Result                                         |
| ------------------------------------------------ | ---------------------------------------------- |
| `f32`/`f64` `algebraic_*` (Rust 1.98)            | 37 bytes smaller, time within noise            |
| Fixed width SIMD in `libs/lite-math`             | 0.1ms a song at best, and 4.8% down to start   |
| `wasm-opt --low-memory-unused`                   | 1,925 bytes smaller, but unsound here          |
| `wasm-opt -O4`                                   | Larger than `-O3`                              |
| `opt-level = "s"` / `"z"` for the `pxtone` crate | 5.6KB / 10.4KB smaller, `moo` 22% / 78% slower |

wasm has no scalar FMA, so the algebraic operators have no contraction to
perform. simd128 was rejected on the same measurement and then taken back: with
the old accumulator layout there was nothing for it to vectorize, and of the 763
v128 instructions LLVM emitted, 550 were `v128.load`, `v128.store` and
`v128.const`. Measured again against the planes above it earns its 4,575 bytes.

Vectorizing `libs/lite-math` by hand does not pay either, and not for a reason
of precision: lane wise IEEE multiplies and adds are the same operations in the
same order, so a two lane version of these series is bit identical to the scalar
one, which is what separates this from the algebraic operators above. It is that
there is not enough time there to win. **`moo` reaches `lite-math` zero times**
on every sample song -- the frequency table is built from literal octave bases
and the mixing pass holds no transcendental at all -- so all of it is load time,
and a build with `sin` and `cos` stubbed out, which is the whole of what
vectorizing them could ever return, moves `tones_ready` 20.6% and a noise render
0.8%.

Two lanes cannot have that. They can land in different quadrants, so the body
has to lose its branch and evaluate both series to blend them, and doing that
costs 4.8% of the phase before a second lane has bought anything back: halving
what is left of a 25% share is about a tenth of a millisecond on a song. What
the same measurement did turn up is that the three `#[inline(never)]` hops a
call went through were worth 4.1% and 295 bytes to fold into one, which is what
`portable` does now. The `wide` crate would work now that simd128 is on; there
is still nothing for it to do.

The overtone oscillator, which makes most of those calls, cannot use wider lanes
in place anyway: it sums the harmonics into one accumulator, and that order is
what the C++ fixes. Lanes would have to run across output samples instead, one
accumulator each.

`--low-memory-unused` is out because rustc links the shadow stack first:
`__stack_pointer` starts at 1 MiB with the data segment above it, so the low
page is the bottom of the stack rather than unused, and the flag would quietly
compromise stack overflow detection.
