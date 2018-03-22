use std::io::*;
use std::vec::Vec;

use pulse;

pub struct PxtoneNoise {
    /// channel
    /// 2, 1
    pub channel: u16,

    /// samples per second
    /// 48000, 44100, 22050, 11025
    pub sample_rate: u32,

    /// bits per sample
    /// 16, 8
    pub bits_per_sample: u16,
}

impl PxtoneNoise {
    pub fn generate(&self, bytes: Vec<u8>) -> Result<()> {
        let noise = pulse::Noise::new(bytes)?;

        Ok(())
    }
}