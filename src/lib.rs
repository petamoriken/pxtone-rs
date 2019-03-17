#![warn(clippy::all)]

#[macro_use]
extern crate num_derive;

mod descriptor;
mod pulse;

use std::{fs::File, io::Error};
use pulse::Noise;

pub fn decode_noise() -> Result<(), Error> {
    let noise = Noise::new(File::open("resources/drum_bass1.ptnoise")?);

    Ok(())
}
