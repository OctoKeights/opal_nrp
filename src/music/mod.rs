//! Playing the music files of the time: DOSBox's DRO captures (version 1), HSC-Tracker
//! modules, and id Software's IMF and WLF files.
//!
//! A [`Player`] owns a chip and a song, and fills a game's audio buffers as opal's example
//! player fills its device's.
//!
//! ```
//! use opal_nrp::music::{Format, Player};
//!
//! // An IMF file: a key on, then a fifth of a second (112 ticks at 560 Hz).
//! let song = vec![0x20, 0x01, 0, 0, 0x60, 0xF0, 0, 0, 0xB0, 0x31, 112, 0];
//! let mut player = Player::new("tune.imf", song, 44_100).unwrap();
//! assert_eq!(player.format(), Format::Imf);
//!
//! let mut buffer = [0i16; 2 * 512];
//! while !player.is_finished() {
//!     player.fill(&mut buffer);
//! }
//! ```

mod dro;
mod hsc;
mod imf;

use crate::{OPL3_SAMPLE_RATE, Opal};

/// How long the chip plays on after a song that does not loop has ended, in milliseconds.
const TAIL_MS: u32 = 250;

/// The most steps that move no time a [`Player::fill`] takes before it lets time pass.
const STEP_STALL_LIMIT: u32 = 1024;

/// The kinds of file a [`Player`] reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    /// DOSBox's raw OPL capture, version 1 (a `.dro` file), told by its signature.
    Dro,
    /// An HSC-Tracker module (a `.hsc` file), told by its name, since it has no signature. It
    /// loops.
    Hsc,
    /// id Software's music, as in Wolfenstein 3-D and Commander Keen (a `.imf` or `.wlf`
    /// file), told by its name.
    Imf,
}

impl Format {
    /// The format's name, as opal's player prints it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Format::Dro => "DRO",
            Format::Hsc => "HSC",
            Format::Imf => "IMF",
        }
    }
}

/// Why a [`Player`] could not read a file: it is none of the [`Format`]s.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnknownFormat;

impl std::fmt::Display for UnknownFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not a recognized format (DRO, HSC, IMF)")
    }
}

impl std::error::Error for UnknownFormat {}

/// What a song does next.
struct Step {
    /// Frames until the next event; 0 means step again at once.
    frames: u64,
    /// Whether the song has come to its end (for a song that loops, its wrap).
    done: bool,
}

impl Step {
    const fn after(frames: u64) -> Step {
        Step {
            frames,
            done: false,
        }
    }

    const END: Step = Step {
        frames: 0,
        done: true,
    };
}

/// A song being played.
enum Song {
    Dro(dro::Dro),
    Hsc(Box<hsc::Hsc>),
    Imf(imf::Imf),
}

impl Song {
    fn step(&mut self, chip: &mut Opal) -> Step {
        match self {
            Song::Dro(song) => song.step(chip),
            Song::Hsc(song) => song.step(chip),
            Song::Imf(song) => song.step(chip),
        }
    }

    const fn loops(&self) -> bool {
        matches!(self, Song::Hsc(_))
    }
}

/// A chip playing a song.
pub struct Player {
    chip: Opal,
    song: Song,
    format: Format,
    imf_rate: u32,
    /// Frames until the song's next event.
    wait: u64,
    /// Frames the song is behind, played while its steps moved no time.
    lag: u64,
    /// Frames left to play after the end.
    tail: u64,
    ended: bool,
    finished: bool,
}

impl Player {
    /// A player of the file `data`, whose name (or path) is `name`, making samples at
    /// `sample_rate` Hz (0 for the chip's own rate). An IMF file is replayed at 560 Hz, and a
    /// WLF file at 700 Hz.
    ///
    /// # Errors
    ///
    /// [`UnknownFormat`] for a file that is none of the formats.
    pub fn new(name: &str, data: Vec<u8>, sample_rate: u32) -> Result<Player, UnknownFormat> {
        Player::with_imf_rate(name, data, sample_rate, None)
    }

    /// As [`Player::new`], with an IMF or WLF file replayed at `imf_rate` Hz where it is
    /// given: the rate belongs to the game rather than to the file, and a few games use
    /// another (280 Hz, say). Other formats ignore it.
    ///
    /// # Errors
    ///
    /// [`UnknownFormat`] for a file that is none of the formats.
    pub fn with_imf_rate(
        name: &str,
        data: Vec<u8>,
        sample_rate: u32,
        imf_rate: Option<u32>,
    ) -> Result<Player, UnknownFormat> {
        let sample_rate = if sample_rate == 0 {
            OPL3_SAMPLE_RATE
        } else {
            sample_rate
        };
        let mut chip = Opal::new(sample_rate);
        let (song, format, imf_rate) = if dro::Dro::matches(&data) {
            let song = dro::Dro::new(data, sample_rate, &mut chip);
            (Song::Dro(song), Format::Dro, 0)
        } else if hsc::Hsc::matches(name, &data) {
            let song = hsc::Hsc::new(data, sample_rate, &mut chip);
            (Song::Hsc(Box::new(song)), Format::Hsc, 0)
        } else if let Some(rate) = imf::Imf::rate_for(name, &data) {
            let rate = imf_rate.filter(|&rate| rate != 0).unwrap_or(rate);
            (
                Song::Imf(imf::Imf::new(&data, sample_rate, rate)),
                Format::Imf,
                rate,
            )
        } else {
            return Err(UnknownFormat);
        };

        let mut player = Player {
            chip,
            song,
            format,
            imf_rate,
            wait: 0,
            lag: 0,
            tail: (u64::from(TAIL_MS) * u64::from(sample_rate) + 500) / 1000,
            ended: false,
            finished: false,
        };
        // (The song's opening writes land before the first sample.)
        let step = player.song.step(&mut player.chip);
        player.wait = step.frames;
        player.ended = step.done && !player.song.loops();
        if player.ended {
            player.chip.flush();
        }
        Ok(player)
    }

    /// The file's format.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// The rate an IMF or WLF file is replayed at, in Hz (0 for the other formats).
    #[must_use]
    pub const fn imf_rate(&self) -> u32 {
        self.imf_rate
    }

    /// Whether the song plays on forever: an HSC module loops, the other formats end.
    #[must_use]
    pub const fn loops(&self) -> bool {
        self.song.loops()
    }

    /// Whether the song has ended and its last notes have died away (a quarter of a second
    /// after the end); [`Player::fill`] gives silence from then on.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.finished
    }

    /// The chip the song plays on, as it is now.
    #[must_use]
    pub const fn chip(&self) -> &Opal {
        &self.chip
    }

    /// The chip the song plays on, to pan its channels, say.
    pub const fn chip_mut(&mut self) -> &mut Opal {
        &mut self.chip
    }

    /// Fills `out` with the song's next stereo samples, left then right in turn (a trailing
    /// odd sample is left as it is).
    pub fn fill(&mut self, out: &mut [i16]) {
        self.fill_frames(out.len() / 2, |k, (left, right)| {
            (out[2 * k], out[2 * k + 1]) = (left, right);
        });
    }

    /// Fills `out` with the song's next stereo samples as floats from -1 to 1, left then right
    /// in turn (a trailing odd sample is left as it is).
    pub fn fill_f32(&mut self, out: &mut [f32]) {
        self.fill_frames(out.len() / 2, |k, (left, right)| {
            out[2 * k] = f32::from(left) / 32768.0;
            out[2 * k + 1] = f32::from(right) / 32768.0;
        });
    }

    /// Plays `frames` frames, handing each to `put` with its number: what opal's player does
    /// for each buffer its device asks for.
    fn fill_frames(&mut self, frames: usize, mut put: impl FnMut(usize, (i16, i16))) {
        let mut stalled_steps = 0;
        for k in 0..frames {
            while self.wait == 0 && !self.ended && stalled_steps < STEP_STALL_LIMIT {
                let step = self.song.step(&mut self.chip);
                if step.done && !self.song.loops() {
                    self.ended = true;
                    self.chip.flush();
                }
                let absorbed = step.frames.min(self.lag);
                self.lag -= absorbed;
                self.wait += step.frames - absorbed;
                if self.wait == 0 {
                    stalled_steps += 1;
                }
            }

            let (left, right) = if self.wait > 0 {
                self.wait -= 1;
                self.chip.sample()
            } else if !self.ended {
                self.lag += 1;
                self.chip.sample()
            } else if self.tail > 0 {
                self.tail -= 1;
                self.chip.sample()
            } else {
                self.finished = true;
                (0, 0)
            };
            put(k, (left, right));
        }
    }
}

/// Whether `name` ends in `extension` (with its dot), in either case.
fn has_extension(name: &str, extension: &str) -> bool {
    name.len() >= extension.len()
        && name.as_bytes()[name.len() - extension.len()..]
            .eq_ignore_ascii_case(extension.as_bytes())
}

fn read_u16_le(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

fn read_u32_le(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An IMF file: a voice keyed on, held, keyed off.
    fn tune() -> Vec<u8> {
        let mut song = vec![0, 0, 0, 0];
        for (register, value, delay) in [
            (0x20u8, 0x01u8, 0u16),
            (0x60, 0xF0, 0),
            (0x23, 0x01, 0),
            (0x63, 0xF0, 0),
            (0xA0, 0x44, 0),
            (0xB0, 0x32, 280),
            (0xB0, 0x12, 56),
        ] {
            song.extend([register, value]);
            song.extend(delay.to_le_bytes());
        }
        song
    }

    #[test]
    fn floats_follow_the_samples() {
        let mut ints = Player::new("tune.imf", tune(), 22_050).unwrap();
        let mut floats = Player::new("tune.imf", tune(), 22_050).unwrap();
        let (mut a, mut b) = ([0i16; 2 * 700], [0f32; 2 * 700]);
        while !ints.is_finished() {
            ints.fill(&mut a);
            floats.fill_f32(&mut b);
            for (&int, &float) in a.iter().zip(&b) {
                assert_eq!((f32::from(int) / 32768.0).to_bits(), float.to_bits());
            }
        }
        assert!(floats.is_finished());
    }

    #[test]
    fn a_song_ends_a_quarter_second_after_its_last_event() {
        let mut player = Player::new("TUNE.IMF", tune(), 10_000).unwrap();
        assert_eq!(
            (player.format(), player.imf_rate(), player.loops()),
            (Format::Imf, 560, false)
        );
        let mut frames = 0;
        let mut frame = [0i16; 2];
        while !player.is_finished() {
            player.fill(&mut frame);
            frames += 1;
        }
        // Half a second and a tenth (336 ticks at 560 Hz), the quarter second after, and the
        // frame that finds it finished.
        assert_eq!(frames, 6000 + 2500 + 1);
    }

    #[test]
    fn other_files_are_refused() {
        assert_eq!(
            Player::new("tune.mid", tune(), 44_100).err(),
            Some(UnknownFormat)
        );
        assert!(Player::new("tune.hsc", tune(), 44_100).is_err());
        assert_eq!(
            Player::with_imf_rate("tune.wlf", tune(), 44_100, Some(280)).map(|p| p.imf_rate()),
            Ok(280)
        );
    }
}
