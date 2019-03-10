#![warn(clippy::all)]

#[macro_use]
extern crate num_derive;

mod descriptor;
mod pulse;

use std::{fs::File, io::{Read as _, Error}};
use pulse::Noise;

pub fn decode_noise() -> Result<(), Error> {
    let mut file = File::open("resources/drum_bass1.ptnoise")?;
    let mut bytes = Vec::new();

    file.read_to_end(&mut bytes)?;

    let noise = Noise::new(bytes);

    Ok(())
}