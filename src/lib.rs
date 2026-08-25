//! A decoder for the pxtone music formats.
//!
//! `no_std` on wasm, where the panic machinery and the formatting it drags in
//! are worth more than the convenience; the other targets keep `std` so that
//! the `cdylib` still links (a `no_std` one would need its own panic handler
//! and allocator). Everything below imports from `alloc` either way, so the
//! wasm build is the one that decides what the crate may use.
#![cfg_attr(target_family = "wasm", no_std)]

extern crate alloc;

pub mod error;
pub mod event;
pub mod master;
pub mod service;
pub mod text;
pub mod unit;

pub(crate) mod effect;
pub(crate) mod pulse;
pub(crate) mod reader;
pub(crate) mod woice;

#[cfg(target_family = "wasm")]
pub mod wasm;

pub use error::PxtoneError;
pub use service::{
  DestinationQuality, NoiseWave, PxtoneService, StartPos, VomitPrepFlags, VomitPreparation,
};
