#![warn(clippy::all)]

mod descriptor;
mod pulse;
mod error;

#[macro_use]
extern crate num_derive;

use pulse::Noise;
use std::{fs::File, io::Error};

pub fn decode_noise() -> Result<(), Error> {
    let noise = Noise::new(File::open("resources/drum_bass1.ptnoise")?);

    Ok(())
}
