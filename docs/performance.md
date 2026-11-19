# Performance

[Overview](../README.md) · [Guide](guide.md) · [Verification](verification.md) · **Performance**

---

`examples/bench.rs` plays a minute of sound at 48 kHz with all 18 channels sounding: OPL3
mode, two four-operator pairs and the rhythm instruments, their keys changing every tenth of
a second. `compare/bench_opal.c` plays the same minute with opal, from the same generator,
and both print the sum of the samples, which is the same. Each reports the best of five
runs:

```sh
cargo run --release --example bench
cc -std=c11 -O3 -I <opal>/include compare/bench_opal.c <opal>/src/opal.c -o bench-opal
./bench-opal
```

## Compared with opal

Measured on one core of an Intel Xeon at 2.1 GHz, the builds run alternately, three times
each:

| Build | Time for a minute of sound | opal_nrp's time | Instructions | opal_nrp's |
|---|---|---|---|---|
| opal, gcc 13 `-O2` | 2.01 s | 23% less | 31.9 billion | 15% fewer |
| opal, gcc 13 `-O3` | 2.12 s | 27% less | 32.7 billion | 17% fewer |
| opal, clang 18 `-O3` | 2.05 s | 25% less | 34.6 billion | 22% fewer |
| opal_nrp | 1.54 s | | 27.2 billion | |

That is 39 times real time on one core, all 18 channels sounding; a game's music on a few
channels takes less.

## Why

opal works out each operator's place in its voice, the channel whose pitch it plays at,
and which operators each channel hears from the registers at every sample, and each
operator's key scale level from its channel's pitch. These change only when a register is
written, so opal_nrp works them out then, and each sample reads them. The vibrato's step and
the tremolo's level, the same for every operator in a sample, are found once a sample. The
output is unchanged (see [Verification](verification.md)).
