//! An emulation of the Yamaha YMF262 (OPL3), the FM synthesis chip of the AdLib and Sound
//! Blaster cards, in pure, safe Rust: a native port of
//! [opal](https://github.com/RealBitdancer/opal) 3.0.0 by Bitdancer.
//!
//! Drive the chip as games drove the real one: write its registers, then take stereo samples
//! at whatever rate the game's audio runs at.
//!
//! ```
//! use opal_nrp::Opal;
//!
//! let mut chip = Opal::new(48_000);
//! chip.write(0x20, 0x01); // operator 0: multiple 1
//! chip.write(0x40, 0x10); // operator 0: some attenuation
//! chip.write(0x60, 0xF0); // operator 0: fast attack
//! chip.write(0x23, 0x01); // operator 3, channel 0's carrier: multiple 1
//! chip.write(0x63, 0xF0); // operator 3: fast attack
//! chip.write(0xA0, 0x44); // channel 0: frequency number 580 (0x244), with block 4 440 Hz
//! chip.write(0xB0, 0x32); // channel 0: key on, block 4, the number's top bits
//!
//! let mut buffer = [0i16; 2 * 480]; // 10 ms of stereo
//! chip.fill(&mut buffer);
//! assert!(buffer.iter().any(|&sample| sample != 0));
//! ```
//!
//! The chip runs at its native 49,716 Hz and is resampled to the rate asked for. Its output
//! matches opal's sample for sample, and through opal that of
//! [Nuked-OPL3](https://github.com/nukeykt/Nuked-OPL3). See the [`music`] module to play
//! the DRO, IMF and HSC files that games and trackers of the time left behind.

use std::collections::VecDeque;

pub mod music;
mod tables;

use tables::{
    ADDITIVE, ALGORITHMS, Algorithm, CHAIN, EXPONENT_ROM, FM, KICK_DIRECT, KICK_FM, KSL_ROM,
    LOG_SIN, MUL_TIMES_2, OPERATOR_OWNER, PERCUSSION, RHYTHM_KEY_BIT, VIBRATO_STEPS,
};

/// The chip's own sample rate, in Hz.
pub const OPL3_SAMPLE_RATE: u32 = 49_716;

/// The version of opal this crate ports.
pub const OPAL_VERSION: &str = "3.0.0";

/// How many chip samples apart [`Opal::write_buffered`] spaces its writes.
pub const WRITE_DELAY: u64 = 2;

const CHANNELS: usize = 18;
const OPERATORS: usize = 36;
const CHANNELS_PER_BANK: usize = 9;
const OPERATORS_PER_BANK: usize = 18;
const PAIRS_PER_BANK: usize = 3;
/// The distance in slots between the operators of one voice.
const VOICE_STRIDE: usize = 3;
/// The operator after which the left side is mixed.
const LEFT_SUM_SLOT: usize = 14;
/// The operator after which the right side is mixed.
const RIGHT_SUM_SLOT: usize = 32;
const BASS_DRUM_CHANNEL: usize = 6;
const RHYTHM_FIRST_SLOT: usize = 12;
const HI_HAT_SLOT: usize = 13;
const SNARE_SLOT: usize = 16;
const CYMBAL_SLOT: usize = 17;
const HI_HAT_PHASE_NOISY: u32 = 0xD0;
const HI_HAT_PHASE_QUIET: u32 = 0x34;
const CYMBAL_PHASE: u32 = 0x80;
const NEW_REGISTER: usize = 0x105;
const FOUR_OP_REGISTER: usize = 0x104;
const RHYTHM_REGISTER: usize = 0xBD;
const PAIR_ENABLE_MASK: u8 = 0x3F;
const BOTH_SIDES: u8 = 3;
const ENVELOPE_SILENT: u32 = 511;
const ENVELOPE_OFF_ZONE: u32 = 504;
const ENVELOPE_PERIOD: u64 = (1 << 36) - 1;
const RATE_FAST: u32 = 48;
const RATE_INSTANT: u32 = 60;
const TREMOLO_STEPS: u8 = 210;
const TREMOLO_PEAK: u8 = 105;
const AUDIBLE_LEVEL_LIMIT: u32 = 3072;
const EXPONENT_IMPLICIT_ONE: u32 = 1024;
const FULL_GAIN: i32 = 256;
/// The most writes [`Opal::write_buffered`] holds before it applies them all at once.
const QUEUE_LIMIT: usize = 4095;

/// Where an operator's envelope is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
}

/// One of the chip's 36 FM operators (slots).
// (Its register bits, each independent of the others.)
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug)]
struct Operator {
    stage: Stage,
    /// Bit 0 while keyed on by its channel, bit 1 while keyed on as a rhythm instrument.
    key: u8,
    out: i16,
    prev_out: i16,
    phase: u32,
    envelope: u32,
    tremolo: bool,
    vibrato: bool,
    sustain_hold: bool,
    key_scale_rate: bool,
    multiple: u8,
    key_scale_level: u8,
    total_level: u8,
    attack_rate: u8,
    decay_rate: u8,
    sustain_level: u8,
    release_rate: u8,
    waveform: u8,
    /// The channel whose pitch it plays at (its own, or its partner's as the second of a
    /// four-operator pair), kept by [`Opal::refresh_routing`].
    pitch: u8,
    /// Where its phase modulation comes from, kept by [`Opal::refresh_routing`].
    modulation: Modulation,
}

/// Where an operator's phase modulation comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Modulation {
    None,
    /// Its own last two outputs, by its channel's feedback.
    Feedback,
    /// The output of the operator before it in its voice.
    Chain,
}

impl Operator {
    const IDLE: Operator = Operator {
        stage: Stage::Release,
        key: 0,
        out: 0,
        prev_out: 0,
        phase: 0,
        envelope: ENVELOPE_SILENT,
        tremolo: false,
        vibrato: false,
        sustain_hold: false,
        key_scale_rate: false,
        multiple: 0,
        key_scale_level: 0,
        total_level: 0,
        attack_rate: 0,
        decay_rate: 0,
        sustain_level: 0,
        release_rate: 0,
        waveform: 0,
        pitch: 0,
        modulation: Modulation::None,
    };
}

/// One of the chip's 18 channels.
#[derive(Clone, Copy, Debug)]
struct Channel {
    left_pan: i32,
    right_pan: i32,
    f_number: u16,
    block: u8,
    latched_key_scale: u8,
    feedback: u8,
    connection: bool,
    /// Bit 0 for the left side, bit 1 for the right.
    output_enable: u8,
    /// The routing, an index into [`ALGORITHMS`].
    algorithm: u8,
    /// The key scale level's full attenuation at the channel's pitch, kept by
    /// [`Opal::write_frequency`].
    key_scale_attenuation: u32,
    /// The operators heard in the voice the channel carries (the first `heard_count`), and
    /// what each counts for, kept by [`Opal::refresh_routing`].
    heard: [u8; 4],
    heard_count: u8,
    gain: i32,
}

impl Channel {
    const IDLE: Channel = Channel {
        left_pan: FULL_GAIN,
        right_pan: FULL_GAIN,
        f_number: 0,
        block: 0,
        latched_key_scale: 0,
        feedback: 0,
        connection: false,
        output_enable: BOTH_SIDES,
        algorithm: FM,
        key_scale_attenuation: 0,
        heard: [0; 4],
        heard_count: 0,
        gain: 1,
    };

    const fn algorithm(&self) -> Algorithm {
        ALGORITHMS[self.algorithm as usize]
    }

    /// The attenuation an operator's key scale level adds at the channel's pitch.
    const fn key_scale_level(&self, level: u8) -> u32 {
        let full = self.key_scale_attenuation;
        match level {
            1 => full / 2,
            2 => full / 4,
            3 => full,
            _ => 0,
        }
    }
}

/// A register write waiting in [`Opal::write_buffered`]'s queue.
#[derive(Clone, Copy, Debug)]
struct QueuedWrite {
    due: u64,
    register: u16,
    value: u8,
}

/// Where the envelope generator is in its cycle.
#[derive(Clone, Copy)]
struct EnvelopeTime {
    odd: bool,
    count: u64,
}

/// What every operator shares in a chip sample.
#[derive(Clone, Copy)]
struct Moment {
    envelope: EnvelopeTime,
    tremolo_level: u32,
    /// The vibrato's step now: halves of the frequency number's top bits, its sign, and the
    /// shift of its depth.
    vibrato: (i32, bool, u32),
}

/// The parts of a sample of a waveform, before the envelope.
#[derive(Clone, Copy)]
struct Wave {
    attenuation: u32,
    negative: bool,
    silent: bool,
}

/// An emulated OPL3.
///
/// A plain value: a clone is a complete save state, and plays on from the moment it was
/// taken.
// (The chip's register bits, each independent of the others.)
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug)]
pub struct Opal {
    sample_rate: i32,
    sample_accum: i32,
    last_output: [i16; 2],
    curr_output: [i16; 2],
    channels: [Channel; CHANNELS],
    operators: [Operator; OPERATORS],
    timer: [u8; 2],
    timer_control: u8,
    timer_count: [u16; 2],
    status: u8,
    new_mode: bool,
    pair_enable: u8,
    rhythm_mode: bool,
    note_select: bool,
    tremolo_depth: bool,
    vibrato_depth: bool,
    tremolo_position: u8,
    tremolo_level: u8,
    sample_counter: u16,
    hi_hat_phase: u16,
    cymbal_phase: u16,
    noise: u32,
    pending_right: i32,
    chip_samples: u64,
    last_due: u64,
    queue: VecDeque<QueuedWrite>,
}

impl Default for Opal {
    /// A chip at its own rate, [`OPL3_SAMPLE_RATE`].
    fn default() -> Self {
        Opal::new(OPL3_SAMPLE_RATE)
    }
}

/// Whether a channel is the second of a four-operator pair (channels 3-5 and 12-14).
const fn is_pair_tail(ch: usize) -> bool {
    (ch % CHANNELS_PER_BANK) / PAIRS_PER_BANK == 1
}

/// A pairable channel's bit in register 104h.
const fn pair_bit(ch: usize) -> usize {
    (ch / CHANNELS_PER_BANK) * PAIRS_PER_BANK + ch % PAIRS_PER_BANK
}

/// A pairable channel's partner: 0 with 3, 1 with 4, 2 with 5, and the same in bank 1.
const fn partner(ch: usize) -> usize {
    if ch % CHANNELS_PER_BANK < PAIRS_PER_BANK {
        ch + PAIRS_PER_BANK
    } else {
        ch - PAIRS_PER_BANK
    }
}

/// A channel's first operator (its modulator); the second is [`VOICE_STRIDE`] on.
const fn first_slot(ch: usize) -> usize {
    let local = ch % CHANNELS_PER_BANK;
    (ch / CHANNELS_PER_BANK) * OPERATORS_PER_BANK + (local / 3) * 6 + local % 3
}

impl Opal {
    /// A chip just reset, whose samples come at `sample_rate` Hz (0 for the chip's own rate,
    /// [`OPL3_SAMPLE_RATE`]).
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        let mut chip = Opal {
            sample_rate: 0,
            sample_accum: 0,
            last_output: [0; 2],
            curr_output: [0; 2],
            channels: [Channel::IDLE; CHANNELS],
            operators: [Operator::IDLE; OPERATORS],
            timer: [0; 2],
            timer_control: 0,
            timer_count: [0; 2],
            status: 0,
            new_mode: false,
            pair_enable: 0,
            rhythm_mode: false,
            note_select: false,
            tremolo_depth: false,
            vibrato_depth: false,
            tremolo_position: 0,
            tremolo_level: 0,
            sample_counter: 0,
            hi_hat_phase: 0,
            cymbal_phase: 0,
            noise: 1,
            pending_right: 0,
            chip_samples: 0,
            last_due: 0,
            queue: VecDeque::new(),
        };
        chip.refresh_routing();
        chip.set_sample_rate(sample_rate);
        chip
    }

    /// Changes the rate samples come at, in Hz (0 for the chip's own rate). The chip's state
    /// is kept. A rate above `i32::MAX - 49716` is taken as that.
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        const MAX: i32 = i32::MAX - OPL3_SAMPLE_RATE as i32;
        self.sample_rate = match sample_rate {
            0 => OPL3_SAMPLE_RATE as i32,
            rate => i32::try_from(rate).map_or(MAX, |rate| rate.min(MAX)),
        };
        self.sample_accum = 0;
    }

    /// Writes `value` to `register`, at once. Registers 000h-0F5h are the first bank and
    /// 100h-1F5h the second, as the chip numbers them.
    pub fn write(&mut self, register: u16, value: u8) {
        let mut reg = usize::from(register) & 0x1FF;
        if !self.new_mode && reg != NEW_REGISTER {
            reg &= 0xFF;
        }
        let bank = reg >> 8;
        let addr = reg & 0xFF;

        if addr < 0x20 {
            self.write_control(reg, value);
        } else if !(0xA0..0xE0).contains(&addr) {
            let row = (addr & 0x1F) >> 3;
            let column = addr & 7;
            if row < 3 && column < 6 {
                let slot = bank * OPERATORS_PER_BANK + row * 6 + column;
                self.write_operator(slot, addr & 0xE0, value);
            }
        } else if reg == RHYTHM_REGISTER {
            self.write_rhythm(value);
        } else if addr < 0xD0 && (addr & 0x0F) < CHANNELS_PER_BANK {
            let ch = bank * CHANNELS_PER_BANK + (addr & 0x0F);
            match addr & 0xF0 {
                0xC0 => self.write_connection(ch, value),
                group => self.write_frequency(ch, group == 0xB0, value),
            }
        }
    }

    /// Queues a write. Queued writes land at least [`WRITE_DELAY`] chip samples apart, so
    /// a key off followed by a key on still restarts the envelope. The spacing is an emulator's
    /// convention, far slower than the chip's bus, so long bursts fall behind.
    pub fn write_buffered(&mut self, register: u16, value: u8) {
        if self.queue.len() >= QUEUE_LIMIT {
            self.flush();
        }
        let due = (self.last_due + WRITE_DELAY).max(self.chip_samples);
        self.queue.push_back(QueuedWrite {
            due,
            register,
            value,
        });
        self.last_due = due;
    }

    /// Applies every queued write at once.
    pub fn flush(&mut self) {
        while let Some(entry) = self.queue.pop_front() {
            self.write(entry.register, entry.value);
        }
        self.last_due = self.chip_samples;
    }

    /// Pans a channel on top of the chip's own left and right switches: `pan` 0 is left, 64
    /// the middle and 127 right (higher values are taken as 127). `channel` is 0 to 17, or 0
    /// to 8 with bit 8 set for the second bank; others are ignored.
    pub fn pan(&mut self, channel: u16, pan: u8) {
        let ch = usize::from(channel & 0xFF) + if channel & 0x100 != 0 { 9 } else { 0 };
        let Some(chan) = self.channels.get_mut(ch) else {
            return;
        };
        let pan = i32::from(pan.min(127));
        chan.left_pan = if pan <= 64 {
            256
        } else {
            256 * (127 - pan) / 63
        };
        chan.right_pan = if pan >= 64 { 256 } else { 256 * pan / 64 };
    }

    /// The next stereo sample, as (left, right).
    pub fn sample(&mut self) -> (i16, i16) {
        while self.sample_accum >= self.sample_rate {
            self.last_output = self.curr_output;
            self.curr_output = self.output();
            self.sample_accum -= self.sample_rate;
        }
        let fract = ((i64::from(self.sample_accum) * 65536 + i64::from(self.sample_rate / 2))
            / i64::from(self.sample_rate)) as i32;
        let interpolate =
            |side: usize| interpolate(self.last_output[side], self.curr_output[side], fract);
        let sample = (interpolate(0), interpolate(1));
        self.sample_accum += OPL3_SAMPLE_RATE as i32;
        sample
    }

    /// Fills `out` with stereo samples, left then right in turn (a trailing odd sample is
    /// left as it is).
    pub fn fill(&mut self, out: &mut [i16]) {
        for [left, right] in out.as_chunks_mut().0 {
            (*left, *right) = self.sample();
        }
    }

    /// Fills `out` with stereo samples as floats from -1 to 1, left then right in turn (a
    /// trailing odd sample is left as it is).
    pub fn fill_f32(&mut self, out: &mut [f32]) {
        for [left, right] in out.as_chunks_mut().0 {
            let sample = self.sample();
            *left = f32::from(sample.0) / 32768.0;
            *right = f32::from(sample.1) / 32768.0;
        }
    }

    /// The status register, as a read of the chip's status port gives it: bit 7 for an
    /// interrupt, bit 6 for timer 1's overflow and bit 5 for timer 2's. A flag stays set until
    /// a reset write (bit 7 of register 4) or a write that sets its timer's mask bit; the
    /// interrupt stays set while any flag is. A masked timer sets neither.
    #[must_use]
    pub const fn status(&self) -> u8 {
        self.status
    }

    // Registers.

    fn write_control(&mut self, reg: usize, value: u8) {
        match reg {
            0x002 => self.timer[0] = value,
            0x003 => self.timer[1] = value,
            0x004 => self.write_timer_control(value),
            // (Bit 7, composite sine mode, does nothing on the YMF262.)
            0x008 => self.note_select = (value >> 6) & 1 != 0,
            FOUR_OP_REGISTER => self.write_pairing(value),
            NEW_REGISTER => self.new_mode = value & 1 != 0,
            // (Register 1's waveform select enable is ignored by the YMF262, as by opal.)
            _ => {}
        }
    }

    fn write_timer_control(&mut self, value: u8) {
        if value & 0x80 != 0 {
            self.status = 0;
            return;
        }
        let started = value & !self.timer_control;
        if started & 1 != 0 {
            self.timer_count[0] = u16::from(self.timer[0]);
        }
        if started & 2 != 0 {
            self.timer_count[1] = u16::from(self.timer[1]);
        }
        self.timer_control = value;
        self.status &= !(value & 0x60);
        if self.status & 0x60 == 0 {
            self.status = 0;
        }
    }

    fn write_pairing(&mut self, value: u8) {
        let enabled = value & PAIR_ENABLE_MASK;
        let released = self.pair_enable & !enabled;
        self.pair_enable = enabled;
        for ch in 0..CHANNELS {
            if is_pair_tail(ch) && (released >> pair_bit(ch)) & 1 != 0 {
                self.latch_key_scale(ch);
            }
        }
        self.refresh_routing();
    }

    fn write_operator(&mut self, slot: usize, group: usize, value: u8) {
        let waveform_mask = if self.new_mode { 7 } else { 3 };
        let op = &mut self.operators[slot];
        match group {
            0x20 => {
                op.tremolo = value >> 7 != 0;
                op.vibrato = (value >> 6) & 1 != 0;
                op.sustain_hold = (value >> 5) & 1 != 0;
                op.key_scale_rate = (value >> 4) & 1 != 0;
                op.multiple = value & 15;
            }
            0x40 => {
                op.key_scale_level = value >> 6;
                op.total_level = value & 63;
            }
            0x60 => {
                op.attack_rate = value >> 4;
                op.decay_rate = value & 15;
            }
            0x80 => {
                op.sustain_level = value >> 4;
                op.release_rate = value & 15;
            }
            _ => op.waveform = value & waveform_mask,
        }
    }

    fn write_rhythm(&mut self, value: u8) {
        self.tremolo_depth = value >> 7 != 0;
        self.vibrato_depth = (value >> 6) & 1 != 0;
        self.rhythm_mode = (value >> 5) & 1 != 0;
        for (op, bit) in self.operators[RHYTHM_FIRST_SLOT..]
            .iter_mut()
            .zip(RHYTHM_KEY_BIT)
        {
            let key = if self.rhythm_mode {
                (value >> bit) & 1
            } else {
                0
            };
            op.key = (op.key & 1) | (key << 1);
        }
        self.refresh_routing();
    }

    fn write_connection(&mut self, ch: usize, value: u8) {
        let sides = if self.new_mode {
            (value >> 4) & BOTH_SIDES
        } else {
            BOTH_SIDES
        };
        let chan = &mut self.channels[ch];
        chan.output_enable = sides;
        chan.feedback = (value >> 1) & 7;
        chan.connection = value & 1 != 0;
        self.refresh_routing();
    }

    fn write_frequency(&mut self, ch: usize, high: bool, value: u8) {
        let chan = &mut self.channels[ch];
        if high {
            chan.f_number = (chan.f_number & 0xFF) | (u16::from(value & 3) << 8);
            chan.block = (value >> 2) & 7;
        } else {
            chan.f_number = (chan.f_number & 0x300) | u16::from(value);
        }
        chan.key_scale_attenuation = (4 * i32::from(KSL_ROM[usize::from(chan.f_number >> 6)])
            - 32 * (8 - i32::from(chan.block)))
        .max(0) as u32;
        if self.borrows_pitch(ch) {
            return;
        }
        self.latch_key_scale(ch);
        if high {
            let slots = if self.is_paired(ch) { 4 } else { 2 };
            let key = (value >> 5) & 1;
            for k in 0..slots {
                let op = &mut self.operators[first_slot(ch) + VOICE_STRIDE * k];
                op.key = (op.key & 2) | key;
            }
        }
    }

    fn latch_key_scale(&mut self, ch: usize) {
        let shift = if self.note_select { 8 } else { 9 };
        let chan = &mut self.channels[ch];
        let note_bit = ((chan.f_number >> shift) & 1) as u8;
        chan.latched_key_scale = 2 * chan.block + note_bit;
    }

    // Routing.

    /// Whether a channel is one of a four-operator pair now.
    const fn is_paired(&self, ch: usize) -> bool {
        ch % CHANNELS_PER_BANK < 2 * PAIRS_PER_BANK && (self.pair_enable >> pair_bit(ch)) & 1 != 0
    }

    /// Whether a channel takes its pitch from its partner, as the second of a pair.
    const fn borrows_pitch(&self, ch: usize) -> bool {
        is_pair_tail(ch) && self.is_paired(ch)
    }

    const fn is_rhythm(&self, ch: usize) -> bool {
        self.rhythm_mode && ch >= BASS_DRUM_CHANNEL && ch < CHANNELS_PER_BANK
    }

    fn pick_algorithm(&self, ch: usize) -> u8 {
        let chan = &self.channels[ch];
        if self.is_rhythm(ch) {
            if ch != BASS_DRUM_CHANNEL {
                return PERCUSSION;
            }
            return if chan.connection {
                KICK_DIRECT
            } else {
                KICK_FM
            };
        }
        if self.is_paired(ch) {
            let other = &self.channels[partner(ch)];
            let (lead, tail) = if is_pair_tail(ch) {
                (other, chan)
            } else {
                (chan, other)
            };
            return CHAIN + 2 * u8::from(lead.connection) + u8::from(tail.connection);
        }
        if chan.connection { ADDITIVE } else { FM }
    }

    /// Picks every channel's routing, and works out what follows from it for each sample: each
    /// operator's pitch and modulation, and the operators each channel hears.
    fn refresh_routing(&mut self) {
        for ch in 0..CHANNELS {
            self.channels[ch].algorithm = self.pick_algorithm(ch);
        }
        for (slot, &owner) in OPERATOR_OWNER.iter().enumerate() {
            let algorithm = self.channels[usize::from(owner)].algorithm();
            let position = self.voice_position(slot);
            let modulation = if algorithm.feeds_back(position) {
                Modulation::Feedback
            } else if algorithm.chains(position) {
                Modulation::Chain
            } else {
                Modulation::None
            };
            let pitch = self.pitch_channel(slot) as u8;
            let op = &mut self.operators[slot];
            op.modulation = modulation;
            op.pitch = pitch;
        }
        for ch in 0..CHANNELS {
            let algorithm = self.channels[ch].algorithm();
            let base = self.voice_base(ch);
            let carries = self.carries_voice(ch);
            let chan = &mut self.channels[ch];
            chan.heard_count = 0;
            chan.gain = algorithm.gain;
            if !carries {
                continue;
            }
            for position in 0..usize::from(algorithm.operator_count) {
                if algorithm.hears(position) {
                    chan.heard[usize::from(chan.heard_count)] =
                        (base + VOICE_STRIDE * position) as u8;
                    chan.heard_count += 1;
                }
            }
        }
    }

    /// Whether a channel's output carries its voice: the second channel of a four-operator
    /// voice carries all four operators, and the first nothing.
    fn carries_voice(&self, ch: usize) -> bool {
        !self.channels[ch].algorithm().spans_pair() || is_pair_tail(ch)
    }

    /// The first operator of the voice a channel carries.
    fn voice_base(&self, ch: usize) -> usize {
        if self.channels[ch].algorithm().spans_pair() && is_pair_tail(ch) {
            first_slot(partner(ch))
        } else {
            first_slot(ch)
        }
    }

    /// An operator's place in its voice: 0 and 1 in a channel's own voice, 2 and 3 in the
    /// second channel of a four-operator one.
    fn voice_position(&self, slot: usize) -> usize {
        let ch = usize::from(OPERATOR_OWNER[slot]);
        let position = usize::from(slot != first_slot(ch));
        if self.channels[ch].algorithm().spans_pair() && is_pair_tail(ch) {
            position + 2
        } else {
            position
        }
    }

    /// The channel whose pitch an operator plays at.
    fn pitch_channel(&self, slot: usize) -> usize {
        let ch = usize::from(OPERATOR_OWNER[slot]);
        if self.borrows_pitch(ch) {
            partner(ch)
        } else {
            ch
        }
    }

    // Each chip sample.

    fn envelope_time(&self) -> EnvelopeTime {
        let pairs = self.chip_samples >> 1;
        EnvelopeTime {
            odd: self.chip_samples & 1 != 0,
            count: if pairs < 2 {
                0
            } else {
                (pairs - 2) % ENVELOPE_PERIOD + 1
            },
        }
    }

    fn cymbal_bit(&self) -> u32 {
        let hat = u32::from(self.hi_hat_phase);
        let cymbal = u32::from(self.cymbal_phase);
        (((hat >> 2) ^ (hat >> 7)) | ((hat >> 3) ^ (cymbal >> 5)) | ((cymbal >> 3) ^ (cymbal >> 5)))
            & 1
    }

    fn rhythm_phase(&mut self, slot: usize, phase: u32, noise_bit: u32) -> u32 {
        match slot {
            HI_HAT_SLOT => {
                let cymbal_bit = self.cymbal_bit();
                let low = if cymbal_bit ^ noise_bit != 0 {
                    HI_HAT_PHASE_NOISY
                } else {
                    HI_HAT_PHASE_QUIET
                };
                (cymbal_bit << 9) | low
            }
            SNARE_SLOT => {
                let hat_bit = (u32::from(self.hi_hat_phase) >> 8) & 1;
                (hat_bit << 9) | ((hat_bit ^ noise_bit) << 8)
            }
            CYMBAL_SLOT => {
                self.cymbal_phase = phase as u16;
                (self.cymbal_bit() << 9) | CYMBAL_PHASE
            }
            _ => phase,
        }
    }

    fn turn_operator(&mut self, slot: usize, moment: Moment) {
        let op = &mut self.operators[slot];
        let pitch = &self.channels[usize::from(op.pitch)];

        let mut attenuation =
            op.envelope + 4 * u32::from(op.total_level) + pitch.key_scale_level(op.key_scale_level);
        if op.tremolo {
            attenuation += moment.tremolo_level;
        }

        let restart = run_envelope(op, pitch, moment.envelope);

        let mut phase = (op.phase >> 9) & 0xFFFF;
        let increment = increment(op, pitch, moment.vibrato);
        op.phase = if restart {
            increment
        } else {
            op.phase.wrapping_add(increment)
        };
        let modulation = op.modulation;

        let noise_bit = self.noise & 1;
        self.noise = (self.noise >> 1) | (((self.noise ^ (self.noise >> 14)) & 1) << 22);

        if slot == HI_HAT_SLOT {
            self.hi_hat_phase = phase as u16;
        }
        if self.rhythm_mode {
            phase = self.rhythm_phase(slot, phase, noise_bit);
        }

        // (The modulation reads this operator's output as it was, before this turn.)
        let modulation = match modulation {
            Modulation::None => 0,
            Modulation::Feedback => {
                let feedback = self.channels[usize::from(OPERATOR_OWNER[slot])].feedback;
                let op = &self.operators[slot];
                if feedback == 0 {
                    0
                } else {
                    // (An arithmetic shift: the floor, as opal takes it.)
                    (i32::from(op.out) + i32::from(op.prev_out)) >> (9 - feedback)
                }
            }
            Modulation::Chain => i32::from(self.operators[slot - VOICE_STRIDE].out),
        };
        let final_phase = (phase as i32 + modulation) as u32 & 0x3FF;
        let op = &mut self.operators[slot];
        op.prev_out = op.out;
        op.out = output(wave(final_phase, op.waveform), attenuation);
    }

    fn mix(&self, side: usize) -> i32 {
        let mut sum = 0;
        for chan in &self.channels {
            if (chan.output_enable >> side) & 1 == 0 {
                continue;
            }
            let voice: i32 = chan.heard[..usize::from(chan.heard_count)]
                .iter()
                .map(|&slot| i32::from(self.operators[usize::from(slot)].out))
                .sum();
            let gain = if side == 0 {
                chan.left_pan
            } else {
                chan.right_pan
            };
            // (An arithmetic shift: the floor, as opal takes it.)
            sum += (voice * chan.gain * gain) >> 8;
        }
        sum
    }

    /// One sample at the chip's own rate. The right side comes a sample after the left, as
    /// the YMF262's pipeline delays it.
    fn output(&mut self) -> [i16; 2] {
        let step = VIBRATO_STEPS[usize::from(self.sample_counter >> 10) & 7];
        let moment = Moment {
            envelope: self.envelope_time(),
            tremolo_level: u32::from(self.tremolo_level),
            vibrato: (step.halves, step.negative, u32::from(!self.vibrato_depth)),
        };
        let mut left = 0;
        let mut right = 0;
        for slot in 0..OPERATORS {
            self.turn_operator(slot, moment);
            if slot == LEFT_SUM_SLOT {
                left = self.mix(0);
            } else if slot == RIGHT_SUM_SLOT {
                right = self.mix(1);
            }
        }
        let output = [saturate(left), saturate(self.pending_right)];
        self.pending_right = right;
        self.close_sample();
        output
    }

    fn close_sample(&mut self) {
        self.sample_counter = self.sample_counter.wrapping_add(1);
        if self.sample_counter.is_multiple_of(64) {
            self.tremolo_position = (self.tremolo_position + 1) % TREMOLO_STEPS;
        }
        let position = self.tremolo_position;
        let triangle = if position < TREMOLO_PEAK {
            position
        } else {
            TREMOLO_STEPS - position
        };
        self.tremolo_level = triangle >> if self.tremolo_depth { 2 } else { 4 };

        if self.sample_counter.is_multiple_of(4) {
            self.tick_timer(0);
        }
        if self.sample_counter.is_multiple_of(16) {
            self.tick_timer(1);
        }

        while let Some(entry) = self.queue.front().copied() {
            if entry.due > self.chip_samples {
                break;
            }
            self.queue.pop_front();
            self.write(entry.register, entry.value);
        }
        self.chip_samples += 1;
    }

    fn tick_timer(&mut self, t: usize) {
        if self.timer_control & (1 << t) == 0 {
            return;
        }
        self.timer_count[t] += 1;
        if self.timer_count[t] > 0xFF {
            let flag = 0x40 >> t;
            self.timer_count[t] = u16::from(self.timer[t]);
            if self.timer_control & flag == 0 {
                self.status |= flag | 0x80;
            }
        }
    }
}

fn saturate(sum: i32) -> i16 {
    sum.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

fn interpolate(last: i16, curr: i16, fract: i32) -> i16 {
    let step = i64::from(fract) * i64::from(i32::from(curr) - i32::from(last)) / 65536;
    (i32::from(last) + step as i32) as i16
}

/// How far an operator's phase moves in a sample, at its channel's pitch and the vibrato's
/// step.
fn increment(
    op: &Operator,
    pitch: &Channel,
    (halves, negative, depth_shift): (i32, bool, u32),
) -> u32 {
    let mut f_number = i32::from(pitch.f_number);
    if op.vibrato {
        let magnitude = (((f_number >> 7) * halves) >> 1) >> depth_shift;
        f_number += if negative { -magnitude } else { magnitude };
    }
    let base = ((f_number as u32) << pitch.block) >> 1;
    (base * MUL_TIMES_2[usize::from(op.multiple)]) >> 1
}

/// Whether a slow rate (below 48) steps its envelope at this time.
fn slow_step_due(count: u64, group: u32, fraction: u32) -> bool {
    let period = 1u64 << (RATE_FAST / 4 - group);
    count % period == period / 2
        || (fraction & 2 != 0 && count % (2 * period) == period)
        || (fraction & 1 != 0 && count % (4 * period) == 2 * period)
}

/// How far an envelope moves at this time, at its register's rate and its rate with the key
/// scaling.
fn envelope_speed(register_rate: u32, actual_rate: u32, time: EnvelopeTime) -> u32 {
    let fraction = actual_rate & 3;
    if register_rate == 0 {
        return 0;
    }
    if actual_rate >= RATE_INSTANT {
        return 4;
    }
    if actual_rate >= RATE_FAST {
        let pair_position = (time.count & 3) as u32;
        let spread = ((pair_position & 1) << 1) | (pair_position >> 1);
        let pair_units =
            (if spread < fraction { 2 } else { 1 }) << ((actual_rate - RATE_FAST) >> 2);
        return (pair_units >> 1) | if time.odd { pair_units & 1 } else { 0 };
    }
    u32::from(time.odd && slow_step_due(time.count, actual_rate >> 2, fraction))
}

/// Moves an operator's envelope on a step; true where it is keyed on afresh, which restarts
/// its phase.
fn run_envelope(op: &mut Operator, pitch: &Channel, time: EnvelopeTime) -> bool {
    let key_active = op.key != 0;
    let mut stage = op.stage;
    let key_on = key_active && stage == Stage::Release;
    let mut level = op.envelope;

    let rate = if key_on || stage == Stage::Attack {
        op.attack_rate
    } else if stage == Stage::Decay {
        op.decay_rate
    } else if stage == Stage::Sustain && op.sustain_hold {
        0
    } else {
        op.release_rate
    };
    let rate = u32::from(rate);

    let key_scale = u32::from(pitch.latched_key_scale);
    let rate_offset = if op.key_scale_rate {
        key_scale
    } else {
        key_scale >> 2
    };
    let actual_rate = 4 * rate + rate_offset;
    let instant = actual_rate >= RATE_INSTANT;
    let speed = envelope_speed(rate, actual_rate, time);
    let sustain_band = if op.sustain_level == 15 {
        31
    } else {
        u32::from(op.sustain_level)
    };
    let off_zone = level >= ENVELOPE_OFF_ZONE;

    if key_on {
        if instant {
            level = 0;
        }
        stage = Stage::Attack;
    } else if stage == Stage::Attack {
        if level == 0 {
            stage = Stage::Decay;
        } else if key_active && !instant {
            level -= (speed * (level + 1) + 7) >> 3;
        }
    } else if stage == Stage::Decay && level >> 4 == sustain_band {
        if off_zone {
            level = ENVELOPE_SILENT;
        }
        stage = Stage::Sustain;
    } else if off_zone {
        level = ENVELOPE_SILENT;
    } else {
        level += speed;
    }

    if !key_active {
        stage = Stage::Release;
    }
    op.envelope = level & 0xFFFF;
    op.stage = stage;
    key_on
}

/// A sample of `waveform` at `phase` (ten bits), as an attenuation and a sign.
fn wave(phase: u32, waveform: u8) -> Wave {
    let second_half = phase & 0x200 != 0;
    let second_quarter = phase & 0x100 != 0;
    let half_position = phase & 0x1FF;
    let quarter_position = (phase & 0xFF) as usize;
    let sine = u32::from(
        LOG_SIN[if second_quarter {
            0xFF - quarter_position
        } else {
            quarter_position
        }],
    );
    let eighth_position = 2 * (phase & 0x7F) as usize;
    let fast_sine = u32::from(
        LOG_SIN[if phase & 0x80 != 0 {
            254 - eighth_position
        } else {
            eighth_position
        }],
    );
    let wave = |attenuation, negative, silent| Wave {
        attenuation,
        negative,
        silent,
    };
    match waveform {
        0 => wave(sine, second_half, false),
        1 => wave(sine, false, second_half),
        2 => wave(sine, false, false),
        3 => wave(u32::from(LOG_SIN[quarter_position]), false, second_quarter),
        4 => wave(fast_sine, !second_half && second_quarter, second_half),
        5 => wave(fast_sine, false, second_half),
        6 => wave(0, second_half, false),
        _ => {
            let position = if second_half {
                0x1FF - half_position
            } else {
                half_position
            };
            wave(8 * position, second_half, false)
        }
    }
}

/// An operator's output: its wave's sample at `attenuation` (in steps of 0.1875 dB).
fn output(wave: Wave, attenuation: u32) -> i16 {
    if wave.silent {
        return 0;
    }
    let level = wave.attenuation + 8 * attenuation;
    let magnitude = if level < AUDIBLE_LEVEL_LIMIT {
        let mantissa =
            EXPONENT_IMPLICIT_ONE + u32::from(EXPONENT_ROM[0xFF - (level & 0xFF) as usize]);
        ((2 * mantissa) >> (level >> 8)) as i32
    } else {
        0
    };
    (if wave.negative {
        -magnitude - 1
    } else {
        magnitude
    }) as i16
}

/// The README's examples, compiled and run as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// The Guide's examples, compiled and run as doctests.
#[cfg(doctest)]
#[doc = include_str!("../docs/guide.md")]
struct GuideDoctests;

#[cfg(test)]
mod tests {
    use super::*;

    /// A chip playing a note on channel 0.
    fn playing() -> Opal {
        let mut chip = Opal::new(44_100);
        for (register, value) in [
            (0x20, 0x01),
            (0x40, 0x10),
            (0x60, 0xF0),
            (0x80, 0x77),
            (0x23, 0x01),
            (0x43, 0x00),
            (0x63, 0xF0),
            (0x83, 0x77),
            (0xA0, 0x98),
            (0xB0, 0x31),
        ] {
            chip.write(register, value);
        }
        chip
    }

    #[test]
    fn a_clone_plays_on_as_the_original() {
        let mut chip = playing();
        let mut buffer = [0; 2 * 300];
        chip.fill(&mut buffer);
        let mut copy = chip.clone();
        let (mut a, mut b) = ([0; 2 * 300], [0; 2 * 300]);
        chip.fill(&mut a);
        copy.fill(&mut b);
        assert_eq!(a, b);
        assert!(a.iter().any(|&sample| sample != 0));
    }

    #[test]
    fn a_silent_chip_is_silent() {
        let mut chip = Opal::default();
        let mut buffer = [1; 2 * 1000];
        chip.fill(&mut buffer);
        assert!(buffer.iter().all(|&sample| sample == 0));
    }

    #[test]
    fn timer_1_overflows_into_the_status() {
        let mut chip = Opal::new(0);
        chip.write(0x02, 0xFF); // one tick to overflow
        chip.write(0x04, 0x01); // start timer 1
        for _ in 0..8 {
            chip.sample();
        }
        assert_eq!(chip.status(), 0xC0);
        chip.write(0x04, 0x80); // reset the flags
        assert_eq!(chip.status(), 0);
        chip.write(0x04, 0x41); // timer 1 masked
        for _ in 0..8 {
            chip.sample();
        }
        assert_eq!(chip.status(), 0);
    }

    #[test]
    fn buffered_writes_land_two_chip_samples_apart() {
        let mut queued = playing();
        let mut direct = queued.clone();
        queued.write_buffered(0xB0, 0x11);
        queued.write_buffered(0xB0, 0x31);
        direct.write(0xB0, 0x11);
        direct.write(0xB0, 0x31);
        assert_eq!(queued.queue.len(), 2);
        queued.flush();
        assert!(queued.queue.is_empty());
        for _ in 0..100 {
            assert_eq!(queued.sample(), direct.sample());
        }
    }

    #[test]
    fn floats_follow_the_samples() {
        let mut chip = playing();
        let mut copy = chip.clone();
        let (mut ints, mut floats) = ([0i16; 2 * 200], [0f32; 2 * 200]);
        chip.fill(&mut ints);
        copy.fill_f32(&mut floats);
        for (&int, &float) in ints.iter().zip(&floats) {
            assert_eq!((f32::from(int) / 32768.0).to_bits(), float.to_bits());
        }
    }

    #[test]
    fn rates_out_of_range_are_taken_as_the_nearest() {
        let mut chip = Opal::new(u32::MAX);
        assert_eq!(chip.sample_rate, i32::MAX - 49_716);
        chip.set_sample_rate(0);
        assert_eq!(chip.sample_rate, 49_716);
    }

    #[test]
    fn the_right_side_comes_a_sample_after_the_left() {
        let mut chip = playing();
        chip.set_sample_rate(OPL3_SAMPLE_RATE);
        let samples: Vec<(i16, i16)> = (0..400).map(|_| chip.sample()).collect();
        let first_left = samples.iter().position(|&(left, _)| left != 0).unwrap();
        let first_right = samples.iter().position(|&(_, right)| right != 0).unwrap();
        assert_eq!(first_right, first_left + 1);
    }
}
