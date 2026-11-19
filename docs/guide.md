# Guide

[Overview](../README.md) · **Guide** · [Verification](verification.md) · [Performance](performance.md)

---

- [How the chip makes sound](#how-the-chip-makes-sound)
- [Instruments and notes](#instruments-and-notes)
- [In a game's audio](#in-a-games-audio)
- [Playing music files](#playing-music-files)
- [Correspondence with opal](#correspondence-with-opal)
- [Differences from opal](#differences-from-opal)

## How the chip makes sound

The YMF262 has 36 **operators**, each a sine-like oscillator (eight waveforms) with its own
envelope, and 18 **channels** of two operators each. In a channel the first operator, the
modulator, bends the phase of the second, the carrier, which is heard: that is FM
synthesis. A channel can instead add its two operators (the connection bit), and pairs of
channels can join into four-operator voices. Channels 6 to 8 can also be the five rhythm
instruments: bass drum, snare, tom-tom, cymbal and hi-hat.

Everything is set by writing registers, as games did through the sound card's ports:

| Registers | For | What |
|---|---|---|
| 20h-35h | operator | tremolo, vibrato, sustain, key scale rate, frequency multiple |
| 40h-55h | operator | key scale level, attenuation (0 loudest, 63 silent) |
| 60h-75h | operator | attack and decay rates (15 fastest) |
| 80h-95h | operator | sustain level and release rate |
| E0h-F5h | operator | waveform |
| A0h-A8h | channel | frequency number, low eight bits |
| B0h-B8h | channel | key on, block (octave), frequency number's top two bits |
| C0h-C8h | channel | left and right outputs, feedback, connection |
| BDh | chip | tremolo and vibrato depth, rhythm mode and the rhythm keys |
| 105h | chip | OPL3 mode (all 18 channels, eight waveforms, stereo) |
| 104h | chip | which channel pairs are four-operator voices |

Registers 100h-1F5h are the same for the second bank (channels 9 to 17). The operators of
channel `c` (0 to 8) are at offsets `[0, 1, 2, 8, 9, 10, 16, 17, 18][c]` (the modulator) and
three more (the carrier).

## Instruments and notes

A game kept its instruments as the registers' values and wrote them to a channel before
playing on it. A note's pitch is its frequency number and block: the frequency is
`number × 49716 / 2^(20 - block)` Hz.

```rust
use opal_nrp::Opal;

/// An instrument: the 20h, 40h, 60h, 80h and E0h registers of its two operators, and its
/// feedback and connection (C0h).
struct Instrument {
    modulator: [u8; 5],
    carrier: [u8; 5],
    feedback_connection: u8,
}

/// An electric piano.
const PIANO: Instrument = Instrument {
    modulator: [0x01, 0x10, 0xF2, 0x74, 0x00],
    carrier: [0x01, 0x00, 0xF2, 0x75, 0x00],
    feedback_connection: 0x0A,
};

const OFFSETS: [u16; 9] = [0x00, 0x01, 0x02, 0x08, 0x09, 0x0A, 0x10, 0x11, 0x12];
const OPERATOR_REGISTERS: [u16; 5] = [0x20, 0x40, 0x60, 0x80, 0xE0];

fn set_instrument(chip: &mut Opal, channel: u16, instrument: &Instrument) {
    let offset = OFFSETS[usize::from(channel)];
    for (k, register) in OPERATOR_REGISTERS.into_iter().enumerate() {
        chip.write(register + offset, instrument.modulator[k]);
        chip.write(register + offset + 3, instrument.carrier[k]);
    }
    // (With OPL3 mode on, bits 4 and 5 turn the left and right outputs on.)
    chip.write(0xC0 + channel, instrument.feedback_connection | 0x30);
}

/// Keys a note of `hz` on; the value of register B0h to key it off with.
fn note_on(chip: &mut Opal, channel: u16, hz: f64) -> u8 {
    let (block, number) = (0..8)
        .map(|block| (block, (hz * f64::from(1 << (20 - block)) / 49716.0).round() as u16))
        .find(|&(_, number)| number < 1024)
        .unwrap_or((7, 1023));
    let high = (block << 2) as u8 | (number >> 8) as u8;
    chip.write(0xA0 + channel, number as u8);
    chip.write(0xB0 + channel, high | 0x20);
    high
}

let mut chip = Opal::new(44_100);
chip.write(0x105, 0x01); // OPL3 mode
set_instrument(&mut chip, 0, &PIANO);
let key_off = note_on(&mut chip, 0, 440.0); // A4

let mut held = vec![0i16; 2 * 22_050]; // half a second
chip.fill(&mut held);
chip.write(0xB0, key_off);
let mut released = vec![0i16; 2 * 22_050];
chip.fill(&mut released);

assert!(held.iter().any(|&sample| sample.abs() > 1000));
```

A sound effect is the same, with registers changed as it plays: rewrite the frequency
number every few milliseconds for a sweep, or the attenuation for a fade. Instruments from
the time are easy to find as `.ins`, `.sbi` or `.bnk` files, whose bytes are these registers.

## In a game's audio

Audio libraries ask for samples from their own thread. Keep the chip where both that
thread and the game can reach it, and fill the buffer the library gives:

```rust
use std::sync::{Arc, Mutex};

use opal_nrp::Opal;

let chip = Arc::new(Mutex::new(Opal::new(48_000)));

// The audio thread's callback, as cpal or another library calls it.
let for_audio = Arc::clone(&chip);
let callback = move |out: &mut [f32]| for_audio.lock().unwrap().fill_f32(out);

// The game writes registers as things happen.
chip.lock().unwrap().write(0xB0, 0x31);

let mut buffer = [0f32; 2 * 480];
callback(&mut buffer);
```

- **Rates.** Give `Opal::new` the rate the device runs at: the chip runs at its own
  49,716 Hz and is resampled to it. `set_sample_rate` changes it without resetting.

- **Timing.** `write` takes effect on the next sample. `write_buffered` spaces writes two
  chip samples apart, as emulators do, so that a key off and a key on written together still
  restart the note; `flush` applies them at once.

- **Panning.** `pan` places a channel between the speakers on top of its left and right
  outputs, for sound effects that come from one side.

- **Save states.** `Opal` is a plain value: a clone plays on exactly as the original.

## Playing music files

`music::Player` plays the files that DOS games and trackers left behind, on a chip of its
own:

| Format | Files | Told by | Ends |
|---|---|---|---|
| DRO | DOSBox's captures of a game's music (version 1) | its signature | at its end |
| IMF | id Software's music (Wolfenstein 3-D, Commander Keen; `.imf`, `.wlf`) | its name | at its end |
| HSC | HSC-Tracker modules | its name | never: it loops |

```rust
use opal_nrp::music::Player;

let module: Vec<u8> = std::fs::read("song.hsc").unwrap_or_default();
if let Ok(mut player) = Player::new("song.hsc", module, 44_100) {
    let mut buffer = [0i16; 2 * 1024];
    player.fill(&mut buffer); // in the audio callback
}
```

IMF files hold no replay rate: `.imf` plays at 560 Hz and `.wlf` at 700 Hz, and
`Player::with_imf_rate` takes another (some games use 280 Hz). `Player::chip_mut` reaches
the chip, to pan its channels. `examples/player.rs` plays a file into a `.wav` file:

```sh
cargo run --release --example player -- song.dro
```

## Correspondence with opal

| opal | opal_nrp |
|---|---|
| `opalInit(&chip, rate)` | `Opal::new(rate)` |
| `opalSetSampleRate` | `Opal::set_sample_rate` |
| `opalWriteReg` | `Opal::write` |
| `opalWriteRegBuffered` | `Opal::write_buffered` |
| `opalFlushWriteBuf` | `Opal::flush` |
| `opalPan` | `Opal::pan` |
| `opalSample(&chip, &left, &right)` | `Opal::sample() -> (left, right)` |
| `opalReadStatus` | `Opal::status` |
| `OPAL_OPL3_SAMPLE_RATE`, `OPAL_WRITEBUF_DELAY` | `OPL3_SAMPLE_RATE`, `WRITE_DELAY` |
| a copy of the struct | `Opal::clone` |
| the player's `MusicFormat` loaders and callback | `music::Player` |

## Differences from opal

- **No visualiser state.** opal's `Opal` struct exposes its channels and operators (their
  envelope stages, attenuation and routing) for visualisers to draw. opal_nrp keeps its
  state private and works out the routing when registers are written rather than at every
  sample, so it has none of those fields; the sound is the same.

- **Sample rates are `u32`.** 0 still selects the chip's own rate; opal's negative rates,
  which also select it, cannot be given.

- **Additions.** `Opal::fill` and `Opal::fill_f32` fill a buffer of stereo samples, and
  `Default` makes a chip at its own rate.

- **The players' readers are a module.** opal keeps them in its example player, which plays
  through miniaudio; here they are `music::Player`, and the example writes a `.wav` file.
  A `Player` given a rate of 0 plays at the chip's own rate in every format, where opal's DRO
  and IMF readers would count no time.
