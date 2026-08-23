# Reference renders

What the original C++ implementation produces for the files in `tests/sample`,
so that `tests/compare_reference.ts` can check this port against ground truth
instead of only against its own previous output.

- `ptcop/` — the first five seconds of each song. Every difference found so far
  starts inside the first two, and the whole set would be 54 MiB instead of 5.
- `ptnoise/` — each instrument in full; they are short.

Both are 16-bit stereo at 44100 Hz, matching what `tests/decode_test.rs` asks of
the Rust decoder.

## The OGG Vorbis material

`ogg/` is not a C++ render: it is what libvorbis 1.3.7 gives for the fixtures in
`tests/sample/ogg`, which is what the C++ decodes an OGGV voice with
(`ov_read( &vf, pcmout, 4096, 0, 2, 1, &sec )`). `libs/lewton` is a
reimplementation of Vorbis rather than a port of libvorbis, so this is the side
of the port that had to be built rather than inherited, and it is now exact.

Regenerate it by compiling libvorbis from source **with `-ffp-contract=off`**.
That flag is the whole story: clang fuses `a*b + c*d` by default on a target
that has a fused multiply-add, and a libvorbis built that way lands about 2e-6
from the C it was built from -- the installed dylib on an arm64 Mac does. The
port follows the C, which is also what a build without a fused multiply-add
gives, including every x86-64 SSE2 build and wasm.

```sh
clang -O2 -ffp-contract=off -I include -I lib \
  dump.c $(ls lib/*.c | grep -vE "barkmel|psytune|tone\.c") -logg -o dump
```

where `dump.c` opens the file with `ov_fopen` and writes what `ov_read` returns
after a 44 byte WAV header.

## The samples-per-tick rate

`pxtnService_moo.cpp` computes it in `double` and keeps it in a `float`:

```c
float    _moo_clock_rate  ; // as the sample
...
_moo_clock_rate = (float)( 60.0f * (double)_dst_sps / ( (double)_moo_bt_tempo * (double)_moo_bt_clock ) );
```

Narrowing looks pointless -- every use promotes it back -- but the uses are
`int * float` and `int / float`, so they run in `f32` as well. Only the song
length, loop point and start are worked out in `double`, and those cast to it
explicitly.

That matters because `clock = smp_count / rate` is compared against event ticks:
where the quotient lands near a tick boundary, an `f64` division floors to a
different tick and a note starts a sample early or late. This port held the
`f64` for a while on the grounds that the narrowing was a mistake in the
original. It is not one to reproduce selectively: narrowing the stored value
while dividing in `f64` is worse than either, which is how the wrong conclusion
was reached the first time.

The same `f32` division is what decides when an event fires, and this port
splits the sample loop into event-free blocks, so it also has to work out
_which_ sample that is ahead of time. Multiplying the tick back by the rate does
not give it: the product and the quotient round in different directions, and at
large sample counts they disagree by a sample or two. A tempo of 250 puts the
rate at 22.05, and tick 1048800 then has an `f32` product of 23126040 while the
quotient already reads 1048800 at sample 23126038.
`PxtoneService::moo_safe_count` therefore walks its estimate onto the sample the
division fires on rather than trusting the product; anything that recomputes
that bound has to keep doing so.

## Regenerating

The C++ sources are not vendored, so this is a manual step: put them in
`pxtone-source-code/`, write a harness against the two entry points below,
compile it together with `pxtone-source-code/pxtone/*.cpp` (`-std=c++17`, and
`-w` because the upstream warnings are not ours), and wrap the raw output in a
WAV header.

For a song, mirroring `VomitPreparation::default()`:

```c
pxtnService *pxtn = new pxtnService(io_read, io_write, io_seek, io_pos);
pxtn->init();
pxtn->set_destination_quality(2, 44100);
pxtn->read(file);
pxtn->tones_ready();

pxtnVOMITPREPARATION prep = {0};
prep.master_volume = 1.0f;   // {0} would leave it silent
pxtn->moo_preparation(&prep);

while (!pxtn->moo_is_end_vomit()) pxtn->Moo(buffer, bytes);
```

`Moo` always fills the buffer and reports the end separately, so the last chunk
runs past the end of the song. The comparison stops at the shorter of the two
streams, so trailing samples do not matter.

For a `.ptnoise` design:

```c
pxtnPulse_Noise noise(io_read, io_write, io_seek, io_pos);
noise.read(file);
pxtnPulse_NoiseBuilder builder(io_read, io_write, io_seek, io_pos);
builder.Init();
pxtnPulse_PCM *pcm = builder.BuildNoise(&noise, 2, 44100, 16);
// pcm->get_p_buf(), pcm->get_smp_body() * 2 * 2 bytes
```
