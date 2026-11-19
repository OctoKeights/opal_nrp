# opal_nrp

**The OPL3 FM synthesis chip of the AdLib and Sound Blaster cards, in pure, safe Rust.**

**Overview** · [Guide](docs/guide.md) · [Verification](docs/verification.md) · [Performance](docs/performance.md)

---

opal_nrp is a native Rust port of [opal](https://github.com/RealBitdancer/opal) 3.0.0 by
Bitdancer, an emulation of the Yamaha YMF262 (OPL3), with its OPL2 compatibility mode.
These chips made the music and sound effects of most DOS games. Write the chip's registers
as a game of the time did, and take stereo samples at your game's own rate.

## Highlights

- **Sample-for-sample output.** Every sample matches opal's, and through opal that of
  [Nuked-OPL3](https://github.com/nukeykt/Nuked-OPL3), at any output rate.

- **The whole chip.** Two- and four-operator FM, all eight waveforms, the tremolo and vibrato,
  the rhythm instruments with their noise, the timers and the status register.

- **Music files.** `music::Player` plays DOSBox's DRO captures, id Software's IMF and WLF
  music, and HSC-Tracker modules, as opal's example player does.

- **Fast.** 39 times real time on one core with all 18 channels sounding, in 23% to 27%
  less time than opal built by gcc or clang (see [Performance](docs/performance.md)).

- **Zero dependencies.** Standard library only: no C compiler, no build script.

- **No `unsafe` code.** Enforced by `#![forbid(unsafe_code)]`.

## Installation

```toml
[dependencies]
opal_nrp = "0.1"
```

## Quick start

```rust
use opal_nrp::Opal;

let mut chip = Opal::new(48_000);
chip.write(0x20, 0x01); // channel 0's modulator: frequency multiple 1
chip.write(0x40, 0x10); //   a little attenuation
chip.write(0x60, 0xF0); //   the fastest attack
chip.write(0x23, 0x01); // channel 0's carrier: frequency multiple 1
chip.write(0x63, 0xF0); //   the fastest attack
chip.write(0xA0, 0x44); // channel 0: frequency number 580 (0x244), with block 4 440 Hz
chip.write(0xB0, 0x32); //   key on, block 4, the number's top bits

let mut buffer = [0f32; 2 * 480]; // 10 ms of stereo, left then right
chip.fill_f32(&mut buffer);
```

Hand `fill_f32` (or `fill`, for `i16` samples) the buffer your audio library asks you to
fill. The [Guide](docs/guide.md) shows instruments, notes, sharing the chip with an audio
thread, and playing music files:

```rust,no_run
use opal_nrp::music::Player;

let song = std::fs::read("theme.dro").unwrap();
let mut player = Player::new("theme.dro", song, 48_000).unwrap();
let mut buffer = [0i16; 2 * 1024];
while !player.is_finished() {
    player.fill(&mut buffer);
}
```

## Licence

MIT, as opal is; see [`LICENSE`](LICENSE), which keeps opal's copyright notice.

Bug reports and contributions are welcome, above all any register write or file that
opal_nrp plays differently from opal.
