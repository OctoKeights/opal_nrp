# Changelog

## 0.1.0

- First release: a native port of [opal](https://github.com/RealBitdancer/opal) 3.0.0, the
  YMF262 (OPL3) emulator by Bitdancer, with `Opal::new`, `write`, `write_buffered`, `flush`,
  `pan`, `sample` and `status` for opal's eight functions, and `fill` and `fill_f32` to fill
  an audio buffer.

- **Music files.** `music::Player` plays DOSBox's DRO captures, id Software's IMF and WLF
  music, and HSC-Tracker modules, ported from opal's example player;
  `examples/player.rs` plays one into a `.wav` file.

- **Checked against opal.** Every sample and status read matches opal's on synthetic
  register programs, and every sample of the player's on made-up and damaged music files and
  opal's bundled HSC tracks (see [Verification](docs/verification.md)).

- **Faster than opal.** The routing is worked out when registers are written rather than at
  every sample (see [Performance](docs/performance.md)).

- **No visualiser state.** opal's public channel and operator fields are not ported (see the
  [Guide](docs/guide.md#differences-from-opal)).
