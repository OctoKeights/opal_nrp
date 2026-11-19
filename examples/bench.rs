//! How fast `opal_nrp` plays: a minute of all 18 channels at 48 kHz, in OPL3 mode with two
//! four-operator pairs and the rhythm instruments, their keys changing every tenth of a second.
//! `compare/bench_opal.c` plays the same with opal, for comparison.
//!
//!   bench [runs]   (5 by default; prints each run's time, then the best)

use std::time::Instant;

use opal_nrp::Opal;

#[path = "../tests/made_up/mod.rs"]
mod made_up;

const RATE: u32 = 48_000;
const SECONDS: u32 = 60;

/// Sets every operator and channel up, from the generator's draws.
fn set_up(chip: &mut Opal, random: &mut made_up::Random) {
    chip.write(0x105, 0x01);
    chip.write(0x104, 0x09);
    for bank in [0u16, 0x100] {
        for offset in (0..0x16).filter(|offset| offset & 7 < 6) {
            chip.write(bank + 0x20 + offset, random.byte());
            chip.write(bank + 0x40 + offset, random.byte() & 0x9F);
            chip.write(bank + 0x60 + offset, random.byte() | 0x80);
            chip.write(bank + 0x80 + offset, random.byte());
            chip.write(bank + 0xE0 + offset, random.byte());
        }
        for ch in 0..9 {
            chip.write(bank + 0xC0 + ch, random.byte() | 0x30);
            chip.write(bank + 0xA0 + ch, random.byte());
        }
    }
    chip.write(0xBD, 0xE0);
}

/// A minute of sound; the sum of its samples, so that none goes unused.
fn play() -> i64 {
    let mut random = made_up::Random::new(1);
    let mut chip = Opal::new(RATE);
    set_up(&mut chip, &mut random);
    let mut sum = 0i64;
    let mut buffer = vec![0i16; 2 * 4800];
    for _ in 0..SECONDS * 10 {
        for ch in 0..18u16 {
            let register = if ch < 9 { 0xB0 + ch } else { 0x1B0 + ch - 9 };
            chip.write(register, random.byte());
        }
        let rhythm = 0xE0 | (random.byte() & 0x1F);
        chip.write(0xBD, rhythm);
        chip.fill(&mut buffer);
        sum += buffer.iter().map(|&sample| i64::from(sample)).sum::<i64>();
    }
    sum
}

fn main() {
    let runs = std::env::args()
        .nth(1)
        .and_then(|runs| runs.parse().ok())
        .unwrap_or(5);
    let mut best = f64::INFINITY;
    let mut sum = 0;
    for _ in 0..runs {
        let start = Instant::now();
        sum = play();
        let seconds = start.elapsed().as_secs_f64();
        best = best.min(seconds);
        println!("{:.1} ms", seconds * 1000.0);
    }
    println!(
        "best {:.1} ms for {SECONDS} s of sound (sum {sum})",
        best * 1000.0
    );
}
