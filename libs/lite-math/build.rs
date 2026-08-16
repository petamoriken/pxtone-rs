//! Assembles `src/wasm.s` (the `f32.sqrt` and `f32.floor` instructions) with
//! clang and links the archive in.
//!
//! Both tools are optional -- Apple's clang cannot target wasm -- and without
//! them the crate falls back to its portable implementations.

use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where to look for an LLVM that can target wasm, after `$CLANG` and `$PATH`.
const LLVM_DIRS: [&str; 2] = ["/opt/homebrew/opt/llvm/bin", "/usr/local/opt/llvm/bin"];

fn main() {
  println!("cargo::rerun-if-changed=src/wasm.s");
  println!("cargo::rerun-if-env-changed=CLANG");
  println!("cargo::rerun-if-env-changed=LLVM_AR");
  println!("cargo::rustc-check-cfg=cfg(wasm_instructions)");

  if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("wasm32") {
    return;
  }
  let Ok(out_dir) = env::var("OUT_DIR").map(PathBuf::from) else {
    return;
  };

  let object = out_dir.join("lite_math_wasm.o");
  let Some(clang) = assemble(&object) else {
    warn_fallback("no clang that can target wasm32 was found");
    return;
  };

  let archive = out_dir.join("liblite_math_wasm.a");
  let _ = std::fs::remove_file(&archive);
  if !archive_object(&clang, &archive, &object) {
    warn_fallback("llvm-ar could not archive the assembled object");
    return;
  }

  println!("cargo::rustc-link-search=native={}", out_dir.display());
  println!("cargo::rustc-link-lib=static=lite_math_wasm");
  println!("cargo::rustc-cfg=wasm_instructions");
}

/// Assembles `src/wasm.s` into `object`, returning the clang that managed it.
fn assemble(object: &Path) -> Option<PathBuf> {
  candidates("CLANG", "clang").find(|clang| {
    run(
      clang,
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

/// Packs `object` into a static archive rustc can link.
fn archive_object(clang: &Path, archive: &Path, object: &Path) -> bool {
  // Prefer the llvm-ar sitting next to the clang that just worked.
  let sibling = clang.parent().map(|dir| dir.join("llvm-ar"));
  sibling
    .into_iter()
    .chain(candidates("LLVM_AR", "llvm-ar"))
    .any(|archiver| {
      run(
        &archiver,
        &["crs".as_ref(), archive.as_ref(), object.as_ref()],
      )
    })
}

/// The programs to try for a tool, in order of preference.
fn candidates(variable: &str, name: &str) -> impl Iterator<Item = PathBuf> {
  env::var_os(variable)
    .map(PathBuf::from)
    .into_iter()
    .chain(std::iter::once(PathBuf::from(name)))
    .chain(
      LLVM_DIRS
        .iter()
        .map(move |dir| PathBuf::from(dir).join(name)),
    )
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
