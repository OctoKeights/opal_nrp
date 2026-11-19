//! Synthetic register programs and music files, generated from a seed exactly as the C side of
//! the comparison (`compare/dump_opal.c`) generates them, and the dumps that both the golden
//! tests and `examples/dump.rs` run.

// (The golden tests and the dump each use only part of this module.)
#![allow(dead_code)]

use std::fmt::Write;

use opal_nrp::Opal;
use opal_nrp::music::Player;

/// The two programs make the same data from the same seed: a 64-bit linear congruential
/// generator, the high half of its state.
pub struct Random {
    pub state: u64,
}

impl Random {
    pub fn new(seed: u64) -> Random {
        Random {
            state: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x0123_4567_89AB_CDEF,
        }
    }

    pub fn next(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 32) as u32
    }

    /// A number below `n`.
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }

    pub fn byte(&mut self) -> u8 {
        self.next() as u8
    }
}

/// FNV-1a, 64 bits.
pub struct Hash {
    pub value: u64,
}

impl Hash {
    pub fn new() -> Hash {
        Hash {
            value: 14_695_981_039_346_656_037,
        }
    }

    pub fn add(&mut self, data: &[u8]) {
        for &byte in data {
            self.value ^= u64::from(byte);
            self.value = self.value.wrapping_mul(1_099_511_628_211);
        }
    }

    pub fn add_u32(&mut self, value: u32) {
        self.add(&value.to_le_bytes());
    }

    pub fn add_sample(&mut self, (left, right): (i16, i16)) {
        self.add(&left.to_le_bytes());
        self.add(&right.to_le_bytes());
    }
}

/// Output sample rates, in Hz, with the edges: low ones, and one the chip takes as its
/// largest.
const RATES: [u32; 12] = [
    49_716,
    44_100,
    48_000,
    22_050,
    11_025,
    8_000,
    96_000,
    192_000,
    2_000,
    4_001,
    1_000_000,
    2_147_483_647,
];

/// A rate for [`Opal::new`]: one of [`RATES`], 0 (the chip's own) or any from 2,000 to
/// 200,000.
fn chip_rate(random: &mut Random) -> u32 {
    match random.below(16) {
        0 => 0,
        k @ 1..=12 => RATES[k as usize - 1],
        _ => 2_000 + random.below(198_001),
    }
}

/// A register: an operator's, a channel's, a control register, or anything at all.
fn register(random: &mut Random) -> u16 {
    let bank = if random.below(2) == 0 { 0 } else { 0x100 };
    match random.below(10) {
        0..=3 => {
            const GROUPS: [u16; 5] = [0x20, 0x40, 0x60, 0x80, 0xE0];
            bank + GROUPS[random.below(5) as usize] + random.below(0x16) as u16
        }
        4..=6 => {
            const GROUPS: [u16; 3] = [0xA0, 0xB0, 0xC0];
            bank + GROUPS[random.below(3) as usize] + random.below(0x10) as u16
        }
        7 | 8 => {
            const CONTROL: [u16; 9] = [0x01, 0x02, 0x03, 0x04, 0x08, 0xBD, 0x104, 0x105, 0x105];
            CONTROL[random.below(9) as usize] + if random.below(8) == 0 { bank } else { 0 }
        }
        _ => {
            if random.below(4) == 0 {
                random.next() as u16
            } else {
                random.below(0x200) as u16
            }
        }
    }
}

/// A value for `register`, biased towards sound: fast attacks, little attenuation, keys on.
fn value(random: &mut Random, register: u16) -> u8 {
    let value = random.byte();
    if random.below(2) == 0 {
        return value;
    }
    match register & 0xF0 {
        0x40 | 0x50 => value & 0xDF,
        0x60 | 0x70 => value | 0x80,
        0xB0 => value | 0x20,
        _ => value,
    }
}

/// A voice set up on a channel and keyed on: both its operators, its connection and its
/// pitch (in either bank).
fn voice(random: &mut Random, chip: &mut Opal, buffered: bool) {
    let ch = random.below(9) as u16;
    let bank = if random.below(3) == 0 { 0x100 } else { 0 };
    let first = (ch / 3) * 8 + ch % 3;
    let mut write = |register: u16, value: u8| {
        if buffered {
            chip.write_buffered(register, value);
        } else {
            chip.write(register, value);
        }
    };
    let mut writes = Vec::new();
    for offset in [first, first + 3] {
        writes.push((bank + 0x20 + offset, random.byte()));
        writes.push((bank + 0x40 + offset, random.byte() & 0x9F));
        writes.push((bank + 0x60 + offset, random.byte() | 0x80));
        writes.push((bank + 0x80 + offset, random.byte()));
        writes.push((bank + 0xE0 + offset, random.byte()));
    }
    writes.push((bank + 0xC0 + ch, random.byte()));
    writes.push((bank + 0xA0 + ch, random.byte()));
    writes.push((bank + 0xB0 + ch, random.byte() | 0x20));
    for (register, value) in writes {
        write(register, value);
    }
}

/// Plays `count` chip samples into `hash`, with the status after.
fn render(chip: &mut Opal, count: u32, hash: &mut Hash) -> u32 {
    let mut audible = 0;
    for _ in 0..count {
        let sample = chip.sample();
        if sample != (0, 0) {
            audible += 1;
        }
        hash.add_sample(sample);
    }
    hash.add(&[chip.status()]);
    audible
}

/// `count` register programs, each on a chip of its own: writes at once and buffered, voices,
/// flushes, pans, rate changes, clones and samples, every one of which is hashed.
pub fn chip(seed: u64, count: u32) -> String {
    let mut random = Random::new(seed);
    let mut out = String::new();
    let mut samples = 0u64;
    let mut audible = 0u64;
    for program in 0..count {
        let mut hash = Hash::new();
        let mut chip = Opal::new(chip_rate(&mut random));
        let actions = 20 + random.below(200);
        for _ in 0..actions {
            match random.below(100) {
                0..=29 => {
                    let register = register(&mut random);
                    let value = value(&mut random, register);
                    chip.write(register, value);
                }
                30..=44 => {
                    let register = register(&mut random);
                    let value = value(&mut random, register);
                    chip.write_buffered(register, value);
                }
                45..=52 => {
                    let buffered = random.below(2) == 0;
                    voice(&mut random, &mut chip, buffered);
                }
                53 | 54 => chip.flush(),
                55..=58 => {
                    let channel = match random.below(4) {
                        0 => random.below(20) as u16,
                        1 => 0x100 + random.below(11) as u16,
                        _ => random.next() as u16,
                    };
                    let pan = random.byte();
                    chip.pan(channel, pan);
                }
                59 => {
                    let rate = chip_rate(&mut random);
                    chip.set_sample_rate(rate);
                }
                60 | 61 => {
                    let mut copy = chip.clone();
                    let count = 1 + random.below(200);
                    audible += u64::from(render(&mut copy, count, &mut hash));
                    samples += u64::from(count);
                }
                _ => {
                    let count = 1 + random.below(600);
                    audible += u64::from(render(&mut chip, count, &mut hash));
                    samples += u64::from(count);
                }
            }
        }
        writeln!(out, "program {program}: {:016x}", hash.value).unwrap();
    }
    writeln!(
        out,
        "chip programs: {count}, samples: {samples}, audible: {audible}"
    )
    .unwrap();
    out
}

/// A song made up for the player: its name and its bytes.
pub struct Song {
    pub name: String,
    pub data: Vec<u8>,
}

/// A register a song might write, with a value biased towards sound.
fn song_write(random: &mut Random) -> (u8, u8) {
    let register = register(random) as u8;
    let value = value(random, u16::from(register));
    (register, value)
}

fn dro(random: &mut Random) -> Vec<u8> {
    let mut body = Vec::new();
    // An opening dump of registers in ascending order, as DOSBox writes, most of the time.
    if random.below(3) != 0 {
        let mut register = random.below(8) as u8;
        while register < 0xF6 {
            body.push(register);
            body.push(random.byte());
            register = register.saturating_add(1 + random.below(24) as u8);
        }
        if random.below(2) == 0 {
            body.push(0x03);
            let mut register = random.below(8) as u8;
            while register < 0xF6 {
                body.push(register);
                body.push(random.byte());
                register = register.saturating_add(1 + random.below(40) as u8);
            }
        }
    }
    let commands = random.below(600);
    for _ in 0..commands {
        match random.below(20) {
            0..=2 => body.extend([0x00, random.byte()]),
            3 => body.extend([0x01, random.byte(), random.below(4) as u8]),
            4 => body.push(0x02),
            5 => body.push(0x03),
            6 | 7 => {
                let (register, value) = song_write(random);
                body.extend([0x04, register, value]);
            }
            _ => {
                let (register, value) = song_write(random);
                body.extend([register, value]);
            }
        }
    }
    if random.below(10) == 0 {
        body.truncate(random.below(body.len() as u32 + 1) as usize);
    }

    let early = random.below(4) == 0;
    let mut data = b"DBRAWOPL".to_vec();
    let version: u32 = if random.below(12) == 0 {
        0x0002_0000 | random.below(3)
    } else {
        0x0001_0000
    };
    data.extend(version.to_le_bytes());
    data.extend(random.next().to_le_bytes()); // the length in milliseconds
    let length = match random.below(5) {
        0 => 0,
        1 => random.below(body.len() as u32 + 8),
        _ => body.len() as u32,
    };
    data.extend(length.to_le_bytes());
    let hardware = match random.below(5) {
        0 => 0,
        1 | 2 => 1,
        3 => 2,
        _ => random.byte(),
    };
    data.push(hardware);
    if !early {
        let padding = if random.below(6) == 0 {
            random.byte()
        } else {
            0
        };
        data.extend([padding, 0, 0]);
    }
    data.extend(body);
    data
}

fn imf(random: &mut Random) -> Vec<u8> {
    let mut stream = Vec::new();
    let commands = random.below(500);
    for _ in 0..commands {
        let (register, value) = song_write(random);
        let delay: u16 = match random.below(4) {
            0 | 1 => 0,
            2 => random.below(40) as u16,
            _ => random.next() as u16 % 400,
        };
        stream.extend([register, value]);
        stream.extend(delay.to_le_bytes());
    }
    if random.below(8) == 0 {
        stream.truncate(random.below(stream.len() as u32 + 1) as usize);
    }
    let mut data = if random.below(3) == 0 {
        // Type 0, which starts with a command to register 0.
        vec![0, 0, 0, 0]
    } else {
        let length = if random.below(4) == 0 {
            random.next() as u16
        } else {
            stream.len() as u16
        };
        length.to_le_bytes().to_vec()
    };
    data.extend(stream);
    data
}

fn hsc(random: &mut Random) -> Vec<u8> {
    let mut data = Vec::new();
    for _ in 0..128 {
        let mut record = [0u8; 12];
        for byte in &mut record {
            *byte = random.byte();
        }
        record[2] &= 0xDF;
        record[4] |= 0x80;
        record[5] |= 0x80;
        data.extend(record);
    }
    let patterns = 1 + random.below(12);
    let orders = 1 + random.below(51);
    for k in 0..51 {
        let order = if k < orders {
            match random.below(12) {
                0 => 0x80 + random.below(0x40) as u8,
                1 => 0xFF,
                _ => random.below(patterns) as u8,
            }
        } else {
            0xFF
        };
        data.push(order);
    }
    for _ in 0..patterns * 64 * 9 {
        let note = match random.below(10) {
            0..=3 => 0,
            4 => 0x7F,
            5 => 0x80,
            _ => 1 + random.below(96) as u8,
        };
        let effect = match random.below(6) {
            0..=2 => 0,
            3 => 0x01,
            _ => random.byte(),
        };
        data.extend([note, effect]);
    }
    match random.below(10) {
        0 => data.push(random.byte()),
        1 => data.truncate(random.below(data.len() as u32) as usize),
        _ => {}
    }
    data
}

/// A song for the player: a DRO, IMF or HSC file, sometimes damaged, or bytes that are none of
/// them, with a name to match.
pub fn song(random: &mut Random) -> Song {
    const NAMES: [&str; 8] = [
        "song.dro", "song.imf", "SONG.IMF", "song.wlf", "song.hsc", "SONG.HSC", "song", ".imf",
    ];
    let (name, mut data) = match random.below(10) {
        0..=2 => ("capture.dro", dro(random)),
        3..=5 => (
            if random.below(3) == 0 {
                "music.wlf"
            } else {
                "music.imf"
            },
            imf(random),
        ),
        6..=8 => ("module.hsc", hsc(random)),
        _ => {
            let len = random.below(2000);
            let data = (0..len).map(|_| random.byte()).collect();
            (NAMES[random.below(8) as usize], data)
        }
    };
    // Some files are damaged: bytes changed at random.
    if !data.is_empty() && random.below(6) == 0 {
        let changes = 1 + random.below(8);
        for _ in 0..changes {
            let at = random.below(data.len() as u32) as usize;
            data[at] = random.byte();
        }
    }
    Song {
        name: name.to_string(),
        data,
    }
}

/// Plays a song through a [`Player`] in buffers of random sizes, up to `frames` frames or
/// its end, hashing everything it gives.
fn play(
    random: &mut Random,
    song: Song,
    sample_rate: u32,
    imf_rate: Option<u32>,
    frames: u32,
) -> (u64, u64, usize) {
    let mut hash = Hash::new();
    let Ok(mut player) = Player::with_imf_rate(&song.name, song.data, sample_rate, imf_rate) else {
        hash.add(b"unknown");
        return (hash.value, 0, 3);
    };
    let kind = player.format() as usize;
    hash.add(player.format().name().as_bytes());
    hash.add_u32(player.imf_rate());
    hash.add(&[u8::from(player.loops())]);
    let mut played = 0;
    let mut buffer = vec![0i16; 2 * 4096];
    while played < frames && !player.is_finished() {
        let chunk = (1 + random.below(4096)).min(frames - played);
        let samples = &mut buffer[..2 * chunk as usize];
        player.fill(samples);
        for &[left, right] in samples.as_chunks().0 {
            hash.add_sample((left, right));
        }
        played += chunk;
    }
    hash.add(&[u8::from(player.is_finished())]);
    (hash.value, u64::from(played), kind)
}

/// `count` songs made up for the player, each played for up to a few seconds.
pub fn music(seed: u64, count: u32) -> String {
    const PLAYER_RATES: [u32; 6] = [44_100, 48_000, 22_050, 11_025, 49_716, 96_000];
    let mut random = Random::new(seed);
    let mut out = String::new();
    let mut total = 0;
    let mut kinds = [0; 4];
    for k in 0..count {
        let song = song(&mut random);
        let sample_rate = PLAYER_RATES[random.below(6) as usize];
        let imf_rate = (random.below(4) == 0).then(|| random.below(1500));
        let frames = random.below(4 * sample_rate);
        let (hash, played, kind) = play(&mut random, song, sample_rate, imf_rate, frames);
        total += played;
        kinds[kind] += 1;
        writeln!(out, "song {k}: {hash:016x}").unwrap();
    }
    let [dro, hsc, imf, unknown] = kinds;
    writeln!(
        out,
        "music songs: {count} (DRO {dro}, HSC {hsc}, IMF {imf}, none {unknown}), frames: {total}"
    )
    .unwrap();
    out
}

/// Each of `tracks` (name and bytes) played for `frames` frames.
pub fn tracks(seed: u64, frames: u32, tracks: Vec<Song>) -> String {
    let mut random = Random::new(seed);
    let mut out = String::new();
    let count = tracks.len();
    for song in tracks {
        let name = song.name.clone();
        let sample_rate = [44_100, 48_000, 22_050][random.below(3) as usize];
        let (hash, ..) = play(&mut random, song, sample_rate, None, frames);
        writeln!(out, "{name}: {hash:016x}").unwrap();
    }
    writeln!(out, "tracks: {count}, frames each: {frames}").unwrap();
    out
}
