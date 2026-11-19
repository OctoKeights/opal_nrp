//! id Software's IMF and WLF music: a stream of register writes, each with a delay in ticks of
//! the game's own rate (opal's `format_imf.c`).

use super::{Step, has_extension, read_u16_le};
use crate::Opal;

/// An IMF song.
pub(super) struct Imf {
    stream: Vec<u8>,
    at: usize,
    sample_rate: u32,
    rate: u32,
    /// The remainder of the frames counted so far, in ticks of `rate`.
    frac: u32,
}

impl Imf {
    /// The rate, in Hz, a file named `name` is replayed at, where it is an IMF one: 560 for
    /// `.imf` and 700 for `.wlf`.
    pub(super) fn rate_for(name: &str, data: &[u8]) -> Option<u32> {
        if data.len() < 4 {
            return None;
        }
        if has_extension(name, ".wlf") {
            Some(700)
        } else if has_extension(name, ".imf") {
            Some(560)
        } else {
            None
        }
    }

    pub(super) fn new(data: &[u8], sample_rate: u32, rate: u32) -> Imf {
        // A nonzero first word is a type-1 file's length.
        let length = usize::from(read_u16_le(data, 0));
        let stream = if length == 0 {
            data
        } else {
            &data[2..(2 + length).min(data.len())]
        };
        Imf {
            stream: stream.to_vec(),
            at: 0,
            sample_rate,
            rate,
            frac: 0,
        }
    }

    pub(super) fn step(&mut self, chip: &mut Opal) -> Step {
        let Some(&[register, value, delay_low, delay_high]) = self.stream.get(self.at..self.at + 4)
        else {
            return Step::END;
        };
        self.at += 4;
        chip.write_buffered(u16::from(register), value);
        let delay = u16::from_le_bytes([delay_low, delay_high]);
        if delay == 0 {
            return Step::after(0);
        }
        let frames = u64::from(delay) * u64::from(self.sample_rate) + u64::from(self.frac);
        self.frac = (frames % u64::from(self.rate)) as u32;
        Step::after(frames / u64::from(self.rate))
    }
}
