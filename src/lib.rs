#![warn(clippy::all)]

mod descriptor;
mod error;
mod pulse;

#[macro_use]
extern crate num_derive;

use error::Result;

use pulse::Noise;
use std::fs::File;

pub fn decode_noise() -> Result<()> {
    let noise = Noise::new(File::open("resources/drum_bass1.ptnoise")?)?;
    noise.build(2, 44100, 16)?;
    Ok(())
}
