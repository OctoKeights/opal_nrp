//! HSC-Tracker modules, played as HSC-Tracker's HSCOBJ 1.4 and HSCPLAY play them, not as
//! `AdPlug` does (opal's `format_hsc.c`).
//!
//! A module is 128 instruments of 12 bytes, an order list of 51 bytes and up to 50 patterns
//! of 64 rows of 9 channels. It loops at an order jump or its end marker.

use super::Step;
use crate::{OPL3_SAMPLE_RATE, Opal};

const CHANNELS: usize = 9;
const PATCHES: usize = 128;
const RECORD_LEN: usize = 12;
const BANK_LEN: usize = PATCHES * RECORD_LEN;
const ORDER_LEN: u8 = 51;
const HEADER_LEN: usize = BANK_LEN + ORDER_LEN as usize;
const ROWS: u8 = 64;
const PATTERN_LEN: usize = ROWS as usize * CHANNELS * 2;
const MAX_PATTERNS: usize = 50;
const MAX_FILE_LEN: usize = HEADER_LEN + MAX_PATTERNS * PATTERN_LEN + 1;
const KEY_ON: u16 = 0x2000;
const NOTE_OFF: u8 = 0x7F;
const SET_PATCH: u8 = 0x80;
const BREAK: u8 = 0x01;
const END: u8 = 0xFF;
const JUMP: u8 = 0x80;
const JUMP_LIMIT: u8 = 0x31;
const LEVEL_REGISTER: u8 = 0x40;
const CARRIER_OFFSET: u8 = 3;
const NO_PATCH: u8 = 0xFF;
const START_TICKS: u32 = 2;

/// Each channel's first operator register.
const OPERATOR_BASE: [u8; CHANNELS] = [0x00, 0x01, 0x02, 0x08, 0x09, 0x0A, 0x10, 0x11, 0x12];
/// The frequency number of each note of an octave.
const NOTE_F_NUMBER: [u16; 12] = [363, 385, 408, 432, 458, 485, 514, 544, 577, 611, 647, 686];
/// The key scale levels as the file numbers them, by the chip's numbers.
const KSL_FROM_FILE: [u8; 4] = [0, 3, 2, 1];

/// A register's values for a voice's two operators.
#[derive(Clone, Copy, Default)]
struct OperatorPair {
    carrier: u8,
    modulator: u8,
}

/// One of a module's 128 instruments.
#[derive(Clone, Copy, Default)]
struct Patch {
    character: OperatorPair,
    level: OperatorPair,
    attack_decay: OperatorPair,
    sustain_release: OperatorPair,
    waveform: OperatorPair,
    connection: u8,
    fine_tune: u8,
}

impl Patch {
    fn decode(record: &[u8]) -> Patch {
        let connection = record[8];
        let pair = |carrier, modulator| OperatorPair { carrier, modulator };
        let mut patch = Patch {
            character: pair(record[0], record[1]),
            level: pair(normalize_ksl(record[2]), record[3]),
            attack_decay: pair(record[4], record[5]),
            sustain_release: pair(record[6], record[7]),
            waveform: pair(record[9], record[10]),
            connection,
            fine_tune: record[11] >> 4,
        };
        if patch.is_additive() {
            patch.level.modulator = normalize_ksl(record[3]);
        }
        patch
    }

    const fn is_additive(&self) -> bool {
        self.connection & 1 != 0
    }

    /// The patch's registers, in the order opal's player writes them.
    const fn rows(&self) -> [(u8, OperatorPair); 5] {
        [
            (0x20, self.character),
            (0x60, self.attack_decay),
            (0x80, self.sustain_release),
            (0xE0, self.waveform),
            (LEVEL_REGISTER, self.level),
        ]
    }
}

fn normalize_ksl(level: u8) -> u8 {
    (KSL_FROM_FILE[usize::from(level >> 6)] << 6) | (level & 0x3F)
}

/// Which operators a level command sets.
#[derive(Clone, Copy)]
enum LevelCommand {
    Carrier,
    Modulator,
    /// The carrier, and the modulator too in a voice whose operators are both heard.
    Voice,
}

/// What a channel is playing.
#[derive(Clone, Copy)]
struct Voice {
    patch: u8,
    freq_word: u16,
    freq_pending: bool,
}

/// An HSC module.
pub(super) struct Hsc {
    data: Vec<u8>,
    sample_rate: u32,
    /// The remainder of the frames counted so far, in 91sts.
    frac: u32,
    patches: [Patch; PATCHES],
    voices: [Voice; CHANNELS],
    row: u8,
    order_position: u8,
    break_pending: bool,
    wrapped: bool,
    ticks_per_row: u32,
    ticks_remaining: u32,
}

const fn register(base: u8, ch: usize, carrier: bool) -> u16 {
    (base + OPERATOR_BASE[ch] + if carrier { CARRIER_OFFSET } else { 0 }) as u16
}

impl Hsc {
    /// Whether a file named `name` of `data` is a module: by its extension, since the format
    /// has no signature, and its size.
    pub(super) fn matches(name: &str, data: &[u8]) -> bool {
        super::has_extension(name, ".hsc")
            && (HEADER_LEN + PATTERN_LEN..=MAX_FILE_LEN).contains(&data.len())
    }

    /// A module of `data`, which [`Hsc::matches`]; sets the chip up for it.
    pub(super) fn new(data: Vec<u8>, sample_rate: u32, chip: &mut Opal) -> Hsc {
        let mut patches = [Patch::default(); PATCHES];
        for (patch, record) in patches.iter_mut().zip(data.chunks_exact(RECORD_LEN)) {
            *patch = Patch::decode(record);
        }
        let mut hsc = Hsc {
            data,
            sample_rate: if sample_rate == 0 {
                OPL3_SAMPLE_RATE
            } else {
                sample_rate
            },
            frac: 0,
            patches,
            voices: [Voice {
                patch: NO_PATCH,
                freq_word: 0,
                freq_pending: false,
            }; CHANNELS],
            row: 0,
            order_position: 0,
            break_pending: false,
            wrapped: false,
            ticks_per_row: START_TICKS,
            ticks_remaining: 1,
        };
        chip.write_buffered(0x01, 0x20);
        chip.write_buffered(0x08, 0x40);
        chip.write_buffered(0xBD, 0x00);
        for ch in 0..CHANNELS {
            hsc.select_patch(chip, ch, ch as u8);
        }
        chip.flush();
        hsc
    }

    pub(super) fn step(&mut self, chip: &mut Opal) -> Step {
        if self.wrapped && self.ticks_remaining == 1 {
            self.wrapped = false;
            return Step::END;
        }
        self.update(chip);
        // A tick of the PC's timer, at 18.2 Hz: the sample rate * 5 / 91.
        let frames = u64::from(self.sample_rate) * 5 + u64::from(self.frac);
        self.frac = (frames % 91) as u32;
        Step::after(frames / 91)
    }

    fn order_byte(&self, slot: u8) -> u8 {
        self.data
            .get(BANK_LEN + usize::from(slot))
            .copied()
            .unwrap_or(END)
    }

    /// A channel's note and effect at a row of a pattern, where the file holds them.
    fn cell(&self, pattern: u8, row: u8, ch: usize) -> Option<[u8; 2]> {
        let at = HEADER_LEN
            + usize::from(pattern) * PATTERN_LEN
            + (usize::from(row) * CHANNELS + ch) * 2;
        if at + 1 >= self.data.len() {
            return None;
        }
        Some([self.data[at], self.data[at + 1]])
    }

    fn voice_patch(&self, ch: usize) -> &Patch {
        &self.patches[usize::from(self.voices[ch].patch) & (PATCHES - 1)]
    }

    fn select_patch(&mut self, chip: &mut Opal, ch: usize, index: u8) {
        let voice = &mut self.voices[ch];
        if usize::from(index) >= PATCHES || voice.patch == index {
            return;
        }
        voice.patch = index;
        let patch = &self.patches[usize::from(index)];
        chip.write_buffered(0xB0 + ch as u16, 0);
        chip.write_buffered(0xC0 + ch as u16, patch.connection);
        for (base, pair) in patch.rows() {
            chip.write_buffered(register(base, ch, true), pair.carrier);
            chip.write_buffered(register(base, ch, false), pair.modulator);
        }
    }

    fn write_frequency(&mut self, chip: &mut Opal, ch: usize) {
        let fine_tune = self.voice_patch(ch).fine_tune;
        let voice = &mut self.voices[ch];
        if fine_tune == 0 && !voice.freq_pending {
            return;
        }
        voice.freq_pending = false;
        let [low, high] = voice.freq_word.to_le_bytes();
        chip.write_buffered(0xA0 + ch as u16, low.wrapping_add(fine_tune));
        chip.write_buffered(0xB0 + ch as u16, high);
    }

    fn play_note(&mut self, chip: &mut Opal, ch: usize, note: u8) {
        let voice = &mut self.voices[ch];
        if note == NOTE_OFF {
            voice.freq_word &= !KEY_ON;
            voice.freq_pending = true;
            return;
        }
        let n = note.wrapping_sub(1);
        chip.write_buffered(0xB0 + ch as u16, 0);
        voice.freq_word = NOTE_F_NUMBER[usize::from(n % 12)] + (u16::from(n / 12) << 10) + KEY_ON;
        voice.freq_pending = true;
    }

    fn set_level(&self, chip: &mut Opal, ch: usize, command: LevelCommand, level: u8) {
        let (carrier, modulator) = match command {
            LevelCommand::Carrier => (true, false),
            LevelCommand::Modulator => (false, true),
            LevelCommand::Voice => (true, self.voice_patch(ch).is_additive()),
        };
        if carrier {
            chip.write_buffered(register(LEVEL_REGISTER, ch, true), level);
        }
        if modulator {
            chip.write_buffered(register(LEVEL_REGISTER, ch, false), level);
        }
    }

    fn apply_effect(&mut self, chip: &mut Opal, ch: usize, effect: u8) {
        let param = effect & 0x0F;
        let slide = |voice: &mut Voice, step: i32| {
            let low = (i32::from(voice.freq_word & 0xFF) + step * (i32::from(param) + 1)) as u8;
            voice.freq_word = (voice.freq_word & 0xFF00) | u16::from(low);
            voice.freq_pending = true;
        };
        match effect >> 4 {
            0x1 => slide(&mut self.voices[ch], 1),
            0x2 => slide(&mut self.voices[ch], -1),
            0xA => self.set_level(chip, ch, LevelCommand::Carrier, param << 2),
            0xB => self.set_level(chip, ch, LevelCommand::Modulator, param << 2),
            0xC => self.set_level(chip, ch, LevelCommand::Voice, param << 2),
            _ => self.ticks_per_row = u32::from(param) + 1,
        }
    }

    fn advance_order(&mut self) {
        let previous = self.order_position;
        self.order_position = self.order_position.wrapping_add(1);
        let value = self.order_byte(self.order_position);
        if value & JUMP != 0 {
            let destination = if value == END { 0 } else { value - JUMP };
            self.order_position = if destination < JUMP_LIMIT {
                destination
            } else {
                0
            };
            self.wrapped = true;
        }
        if previous < ORDER_LEN && self.order_position >= ORDER_LEN {
            self.wrapped = true;
        }
    }

    fn play_row(&mut self, chip: &mut Opal, pattern: u8) {
        for ch in 0..CHANNELS {
            let Some([note, effect]) = self.cell(pattern, self.row, ch) else {
                continue;
            };
            if note == SET_PATCH {
                self.select_patch(chip, ch, effect & (PATCHES as u8 - 1));
                continue;
            }
            if note != 0 {
                self.play_note(chip, ch, note);
            }
            match effect {
                0 => {}
                BREAK => self.break_pending = true,
                _ => self.apply_effect(chip, ch, effect),
            }
        }
        for ch in 0..CHANNELS {
            self.write_frequency(chip, ch);
        }
    }

    fn update(&mut self, chip: &mut Opal) {
        self.ticks_remaining = self.ticks_remaining.wrapping_sub(1);
        if self.ticks_remaining != 0 {
            return;
        }
        let pattern = self.order_byte(self.order_position);
        self.play_row(chip, pattern);
        self.ticks_remaining = self.ticks_per_row;
        self.row += 1;
        if self.break_pending || self.row >= ROWS {
            self.row = 0;
            self.break_pending = false;
            self.advance_order();
        }
    }
}
