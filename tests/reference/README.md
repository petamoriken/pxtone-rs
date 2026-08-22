# Reference renders

What the original C++ implementation produces for the files in `tests/sample`,
so that `tests/compare_reference.ts` can check this port against ground truth
instead of only against its own previous output.

- `ptcop/` — the first five seconds of each song. Every difference found so far
  starts inside the first two, and the whole set would be 54 MiB instead of 5.
- `ptnoise/` — each instrument in full; they are short.

Both are 16-bit stereo at 44100 Hz, matching what `tests/decode_test.rs` asks of
the Rust decoder.

## Where this port deliberately differs

`pxtnService_moo.cpp` computes the samples-per-tick rate in `double` and then
keeps it in a `float`:

```c
float    _moo_clock_rate  ; // as the sample
...
_moo_clock_rate = (float)( 60.0f * (double)_dst_sps / ( (double)_moo_bt_tempo * (double)_moo_bt_clock ) );
```

Every use promotes it back, so the narrowing buys nothing and only costs
precision -- for a tempo of 145 the rate lands on 38.017242431640625 instead of
38.017241379310342. This port holds the `f64`, on purpose.

That is not what keeps the five songs off zero, though. Rendering the reference
with the field widened to `double` moves them no closer: the worst difference on
`Aisatsu[Rusk]` goes 243 to 245 and on `overworld2_nes[Se-ko]` 2074 to 2351. So
whatever is left in those songs is something else, and this choice costs nothing
measurable.

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
