#[macro_use]
extern crate num_derive;
extern crate num_traits;

extern crate byteorder;

use std::fs::File;

use std::io::prelude::*;
use std::io::Error;

mod helper;

mod noise;
mod pulse;

fn main() {
	match run() {
		Ok(_) =>println!("Ok"),
		Err(err) => println!("Error: {}", err),
	}
}

fn run() -> Result<(), Error> {
    let mut file = File::open("resources/drum_bass1.ptnoise")?;
    let mut bytes = Vec::new();

    file.read_to_end(&mut bytes)?;
    let noise = noise::PxtoneNoise { channel: 2, sample_rate: 44100, bits_per_sample: 16 };
    noise.generate(bytes)?;

    Ok(())
}