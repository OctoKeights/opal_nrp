# Verification

[Overview](../README.md) · [Guide](guide.md) · **Verification** · [Performance](performance.md)

---

opal_nrp is checked against opal itself: both are run over the same synthetic data and their
output compared sample for sample.

## The comparison

`compare/compare.sh` builds opal's core and its example player's format readers from a
checkout, builds `compare/dump_opal.c` against them and `examples/dump.rs` against opal_nrp,
runs both, and compares what they print:

```sh
compare/compare.sh <checkout of opal 3.0.0> [count] [seeds] [dumps]
```

Both programs generate the same data from each seed and print a running hash of everything
they produce:

- **chip:** register programs, each on a chip of its own at a rate from 2 kHz to the largest
  the chip takes. Each writes registers at once and buffered (control registers, operators
  and channels in both banks, anything at all), sets voices up and keys them on, flushes,
  pans, changes the rate and clones the chip, and every sample and status read is hashed.

- **music:** made-up DRO captures (with and without the opening register dump, of OPL2,
  OPL3 and dual OPL2, with the early header), IMF files of both types, and HSC modules,
  some cut short or with bytes changed, and files that are none of them, each played by the
  player in buffers of random sizes for up to four seconds.

- **tracks:** the seven HSC tracks in opal's `music` folder, each played for a minute.

## Results

| opal built by | Register programs | Samples | Songs | Frames | Tracks |
|---|---|---|---|---|---|
| gcc 13 | 12,000 | 165 million | 1,200 | 97 million | 7, at three rates |
| clang 18 | 2,000 | 27 million | 200 | 16 million | 7 |

Every hash is identical. The songs were 429 DRO captures, 354 HSC modules, 511 IMF files
and 106 files of none of them.

## Tests

`cargo test` needs no C compiler. `tests/golden.rs` checks opal_nrp against hashes recorded
from opal (`tests/data`, 240 register programs and 60 songs), and the examples in the
README and the Guide are run as tests. `tests/data/README.md` explains how to record the
hashes again.
