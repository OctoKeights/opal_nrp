//! DOSBox's raw OPL captures, version 1 (opal's `format_dro.c`).
//!
//! DOSBox writes registers 00h-04h and 104h without the format's escape, and opens each
//! capture with a dump of the registers; both are detected, as opal detects them.

use super::{Step, read_u16_le, read_u32_le};
use crate::Opal;

const HEADER_LEN: usize = 24;
/// The early header, with a one-byte hardware type and no padding.
const HEADER_LEN_EARLY: usize = 21;
const HARDWARE_OFFSET: usize = 20;
const HARDWARE_OPL3: u8 = 1;
const HARDWARE_DUAL_OPL2: u8 = 2;

/// What the next command of a capture is.
#[derive(Clone, Copy)]
enum Event {
    End,
    None,
    /// A delay, in milliseconds.
    Delay(u32),
    Write {
        bank: u16,
        register: u8,
        value: u8,
    },
}

/// Where a reading of a capture is.
#[derive(Clone, Copy)]
struct Cursor {
    at: usize,
    bank: u16,
    /// The last register of the opening dump, once there is one.
    last_dump_register: Option<u8>,
    in_dump: bool,
    escapes: bool,
    two_arrays: bool,
}

impl Cursor {
    const fn start(two_arrays: bool) -> Cursor {
        Cursor {
            at: 0,
            bank: 0,
            last_dump_register: None,
            in_dump: true,
            escapes: false,
            two_arrays,
        }
    }

    /// Whether a register is past the last of the dump so far.
    fn beyond_dump(&self, register: u8) -> bool {
        self.last_dump_register.is_none_or(|last| register > last)
    }

    const fn is_escape(&self, next: u8) -> bool {
        self.escapes
            || if self.bank == 0 {
                next <= 0x04
            } else {
                next == 0x04
            }
    }

    fn next(&mut self, data: &[u8]) -> Event {
        let size = data.len();
        if self.at >= size {
            return Event::End;
        }

        let command = data[self.at];
        if self.in_dump
            && self.bank == 0
            && self.last_dump_register.is_none()
            && (command == 0x00 || (command == 0x03 && self.two_arrays))
        {
            self.in_dump = false;
            self.escapes = true;
        }
        if self.in_dump && command != 0x04 {
            if (command == 0x02 || command == 0x03) && !self.beyond_dump(command) {
                self.at += 1;
                self.bank = if command == 0x03 { 0x100 } else { 0 };
                self.last_dump_register = None;
                self.in_dump = command == 0x03;
                return Event::None;
            }
            if self.beyond_dump(command) {
                if self.at + 1 >= size {
                    self.at = size;
                    return Event::End;
                }
                self.last_dump_register = Some(command);
                self.at += 2;
                return Event::Write {
                    bank: self.bank,
                    register: command,
                    value: data[self.at - 1],
                };
            }
            self.in_dump = false;
        }

        self.at += 1;
        match command {
            0x00 => {
                if self.at >= size {
                    return Event::End;
                }
                self.at += 1;
                Event::Delay(u32::from(data[self.at - 1]) + 1)
            }
            0x01 => {
                if self.at + 1 >= size {
                    self.at = size;
                    return Event::End;
                }
                let ms = u32::from(read_u16_le(data, self.at)) + 1;
                self.at += 2;
                Event::Delay(ms)
            }
            0x02 | 0x03 => {
                self.bank = if command == 0x03 { 0x100 } else { 0 };
                Event::None
            }
            _ => {
                if self.at >= size {
                    return Event::End;
                }
                let mut register = command;
                if command == 0x04 && self.is_escape(data[self.at]) {
                    if self.at + 1 >= size {
                        self.at = size;
                        return Event::End;
                    }
                    self.escapes = self.escapes || self.bank == 0;
                    self.in_dump = false;
                    register = data[self.at];
                    self.at += 1;
                }
                if self.in_dump && self.beyond_dump(register) {
                    self.last_dump_register = Some(register);
                }
                self.at += 1;
                Event::Write {
                    bank: self.bank,
                    register,
                    value: data[self.at - 1],
                }
            }
        }
    }
}

/// A DRO capture.
pub(super) struct Dro {
    data: Vec<u8>,
    cursor: Cursor,
    sample_rate: u32,
    /// The remainder of the frames counted so far, in thousandths.
    frac: u32,
    /// Whether C0h writes need both speakers turned on (a capture of an OPL2).
    stereo_c0: bool,
    dual_opl2: bool,
}

impl Dro {
    /// Whether `data` is a version 1 capture.
    pub(super) fn matches(data: &[u8]) -> bool {
        data.len() >= HEADER_LEN_EARLY && data.starts_with(b"DBRAWOPL") && read_u16_le(data, 8) == 0
    }

    /// A capture of `data`, which [`Dro::matches`]; turns on the chip's OPL3 mode where the
    /// capture needs it.
    pub(super) fn new(mut data: Vec<u8>, sample_rate: u32, chip: &mut Opal) -> Dro {
        let size = data.len();
        let length = read_u32_le(data.as_slice(), 16) as usize;
        let header_len = if length > 0 && length == size - HEADER_LEN_EARLY {
            HEADER_LEN_EARLY
        } else if length > 0 && size >= HEADER_LEN && length == size - HEADER_LEN {
            HEADER_LEN
        } else if size < HEADER_LEN || read_u32_le(data.as_slice(), 20) & 0xFFFF_FF00 != 0 {
            HEADER_LEN_EARLY
        } else {
            HEADER_LEN
        };
        let mut data_len = size - header_len;
        if length > 0 && length <= data_len {
            data_len = length;
        }

        let hardware = data[HARDWARE_OFFSET];
        let cursor = Cursor::start(hardware == HARDWARE_OPL3 || hardware == HARDWARE_DUAL_OPL2);
        data.truncate(header_len + data_len);
        data.drain(..header_len);
        let dro = Dro {
            data,
            cursor,
            sample_rate,
            frac: 0,
            stereo_c0: hardware != HARDWARE_OPL3,
            dual_opl2: hardware == HARDWARE_DUAL_OPL2,
        };
        if dro.dual_opl2 || dro.snapshot_enables_opl3() {
            chip.write(0x105, 0x01);
        }
        dro
    }

    /// Whether the capture's opening state turns on OPL3 mode (bit 0 of register 105h).
    fn snapshot_enables_opl3(&self) -> bool {
        let mut cursor = self.cursor;
        loop {
            match cursor.next(&self.data) {
                Event::End | Event::Delay(_) => return false,
                Event::Write {
                    bank,
                    register: 0x05,
                    value,
                } if bank != 0 && value & 1 != 0 => return true,
                _ => {}
            }
        }
    }

    pub(super) fn step(&mut self, chip: &mut Opal) -> Step {
        match self.cursor.next(&self.data) {
            Event::End => Step::END,
            Event::Delay(ms) => {
                let frames = u64::from(ms) * u64::from(self.sample_rate) + u64::from(self.frac);
                self.frac = (frames % 1000) as u32;
                Step::after(frames / 1000)
            }
            Event::Write {
                bank,
                register,
                mut value,
            } => {
                if !(self.dual_opl2 && bank != 0 && register < 0x20) {
                    if self.stereo_c0 && (0xC0..=0xC8).contains(&register) {
                        value |= 0x30;
                    }
                    chip.write_buffered(bank + u16::from(register), value);
                }
                Step::after(0)
            }
            Event::None => Step::after(0),
        }
    }
}
