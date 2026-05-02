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

To build the `.wasm` binary yourself:

```sh
deno task build:wasm:raw
```

The compiled module exports a C FFI interface. Memory management uses explicit
`alloc`/`dealloc` exports. See [`src/wasm.rs`](src/wasm.rs) for the full API and
[`tests/wasm_test.ts`](tests/wasm_test.ts) for usage examples.

To further optimize the binary with
[Binaryen](https://github.com/WebAssembly/binaryen)'s `wasm-opt`
(`brew install binaryen`):

```sh
deno task build:wasm:opt
```

## Running Tests

```sh
# Rust tests
deno task test:rust

# WebAssembly tests
deno task test:wasm
```

## License

[MIT](LICENSE.md)
