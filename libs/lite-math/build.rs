//! Assembles `src/wasm.s` (the `f32.sqrt` and `f32.floor` instructions) with
//! clang and links it in.
//!
//! clang is optional -- Apple's cannot target wasm -- and without it the crate
//! falls back to its portable implementations. The archive around the object is
//! written here rather than with llvm-ar, which is missing or version suffixed
//! on many systems, the CI runners included.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where to look for a clang that can target wasm, after `$CLANG` and `$PATH`.
const LLVM_DIRS: [&str; 2] = ["/opt/homebrew/opt/llvm/bin", "/usr/local/opt/llvm/bin"];

/// The symbols `src/wasm.s` defines, for the archive's index.
const SYMBOLS: [&str; 2] = ["lite_math_sqrt_f32", "lite_math_floor_f32"];

/// Name of the object inside the archive, `/` terminated as GNU ar writes it.
const MEMBER_NAME: &str = "lite_math_wasm.o/";

fn main() {
  println!("cargo::rerun-if-changed=src/wasm.s");
  println!("cargo::rerun-if-env-changed=CLANG");
  println!("cargo::rustc-check-cfg=cfg(wasm_instructions)");

  if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("wasm32") {
    return;
  }
  let Ok(out_dir) = env::var("OUT_DIR").map(PathBuf::from) else {
    return;
  };

  let object = out_dir.join("lite_math_wasm.o");
  if !assemble(&object) {
    warn_fallback("no clang that can target wasm32 was found");
    return;
  }

  let archive = out_dir.join("liblite_math_wasm.a");
  if let Err(error) = write_archive(&archive, &object) {
    warn_fallback(&format!("the archive could not be written ({error})"));
    return;
  }

  println!("cargo::rustc-link-search=native={}", out_dir.display());
  println!("cargo::rustc-link-lib=static=lite_math_wasm");
  println!("cargo::rustc-cfg=wasm_instructions");
}

/// Assembles `src/wasm.s` into `object` with the first clang that manages it.
fn assemble(object: &Path) -> bool {
  let clangs = env::var_os("CLANG")
    .map(PathBuf::from)
    .into_iter()
    .chain(std::iter::once(PathBuf::from("clang")))
    .chain(LLVM_DIRS.iter().map(|dir| PathBuf::from(dir).join("clang")));

  clangs.into_iter().any(|clang| {
    run(
      &clang,
      &[
        "--target=wasm32-unknown-unknown".as_ref(),
        "-c".as_ref(),
        "src/wasm.s".as_ref(),
        "-o".as_ref(),
        object.as_ref(),
      ],
    )
  })
}

/// Writes `object` into a GNU style archive, index and all, so that rustc can
/// hand it to the linker.
fn write_archive(archive: &Path, object: &Path) -> io::Result<()> {
  let contents = fs::read(object)?;

  let mut names = Vec::new();
  for symbol in SYMBOLS {
    names.extend_from_slice(symbol.as_bytes());
    names.push(0);
  }
  let index_size = 4 + 4 * SYMBOLS.len() + names.len();
  // Members start on an even offset, so an odd sized one is padded.
  let member_offset = 8 + 60 + index_size + index_size % 2;

  let mut out = Vec::with_capacity(member_offset + contents.len());
  out.extend_from_slice(b"!<arch>\n");

  // The index: the number of symbols, the offset of the member defining each,
  // then their names.
  out.extend_from_slice(&member_header(b"/", index_size));
  out.extend_from_slice(&(SYMBOLS.len() as u32).to_be_bytes());
  for _ in SYMBOLS {
    out.extend_from_slice(&(member_offset as u32).to_be_bytes());
  }
  out.extend_from_slice(&names);
  if index_size % 2 == 1 {
    out.push(b'\n');
  }

  out.extend_from_slice(&member_header(MEMBER_NAME.as_bytes(), contents.len()));
  out.extend_from_slice(&contents);
  if contents.len() % 2 == 1 {
    out.push(b'\n');
  }

  fs::write(archive, out)
}

/// The 60 byte header in front of every archive member.
fn member_header(name: &[u8], size: usize) -> [u8; 60] {
  let mut header = [b' '; 60];
  let mut write = |at: usize, value: &[u8]| header[at..at + value.len()].copy_from_slice(value);

  write(0, name);
  write(16, b"0"); // modification time
  write(28, b"0"); // owner
  write(34, b"0"); // group
  write(40, b"644"); // mode
  write(48, size.to_string().as_bytes());
  write(58, b"`\n");
  header
}

fn run(program: &Path, args: &[&OsStr]) -> bool {
  Command::new(program)
    .args(args)
    .status()
    .is_ok_and(|status| status.success())
}

fn warn_fallback(reason: &str) {
  println!("cargo::warning=lite-math: {reason}, using the portable maths instead");
}
