//! Plays a DRO, IMF, WLF or HSC file into a `.wav` file, as opal's example player plays one
//! to the speakers.
//!
//!   player [--rate <hz>] [--seconds <n>] <song> [<out.wav>]
//!
//!   --rate <hz>     the IMF replay rate (280, 560 or 700, say); by default 700 for .wlf and
//!                   560 for .imf, and ignored by the other formats
//!   --seconds <n>   how long to play a song that loops (an HSC module), 120 by default
//!
//! The sound is written at 44,100 Hz, to `<song>.wav` if no name is given.

use std::process::ExitCode;

use opal_nrp::music::Player;

const SAMPLE_RATE: u32 = 44_100;

fn usage() {
    eprintln!("usage: player [--rate <hz>] [--seconds <n>] <song> [<out.wav>]");
}

/// A `.wav` file of interleaved 16-bit stereo `samples`.
fn wav(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend(b"RIFF");
    out.extend((36 + data_len).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(2u16.to_le_bytes()); // stereo
    out.extend(SAMPLE_RATE.to_le_bytes());
    out.extend((SAMPLE_RATE * 4).to_le_bytes());
    out.extend(4u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data_len.to_le_bytes());
    for sample in samples {
        out.extend(sample.to_le_bytes());
    }
    out
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut imf_rate = None;
    let mut seconds = 120;
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--rate" | "--seconds" => {
                let Some(value) = args.next().and_then(|value| value.parse::<u32>().ok()) else {
                    eprintln!("player: {arg} needs a number");
                    return ExitCode::FAILURE;
                };
                if arg == "--rate" {
                    if !(1..=0xFFFF).contains(&value) {
                        eprintln!("player: invalid --rate value {value} (expected 1..65535 Hz)");
                        return ExitCode::FAILURE;
                    }
                    imf_rate = Some(value);
                } else {
                    seconds = value;
                }
            }
            "-h" | "--help" => {
                usage();
                return ExitCode::SUCCESS;
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                eprintln!("player: unknown option '{arg}'");
                usage();
                return ExitCode::FAILURE;
            }
            _ => paths.push(arg),
        }
    }
    let (song, out) = match paths.as_slice() {
        [song] => (song.clone(), format!("{song}.wav")),
        [song, out] => (song.clone(), out.clone()),
        _ => {
            usage();
            return ExitCode::FAILURE;
        }
    };

    let data = match std::fs::read(&song) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("player: cannot read '{song}': {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut player = match Player::with_imf_rate(&song, data, SAMPLE_RATE, imf_rate) {
        Ok(player) => player,
        Err(error) => {
            eprintln!("player: '{song}' is {error}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "player: {song} | {} | {SAMPLE_RATE} Hz",
        player.format().name()
    );
    if player.imf_rate() != 0 {
        let how = if imf_rate.is_some() {
            "forced"
        } else {
            "by extension"
        };
        println!("player: IMF replay rate {} Hz ({how})", player.imf_rate());
    }

    // (A song that does not loop plays to its end; any song stops at the limit.)
    let limit = u64::from(seconds) * u64::from(SAMPLE_RATE);
    let mut samples = Vec::new();
    let mut buffer = [0i16; 2 * 1024];
    while !player.is_finished() && (samples.len() as u64) < 2 * limit {
        player.fill(&mut buffer);
        samples.extend_from_slice(&buffer);
    }
    if let Err(error) = std::fs::write(&out, wav(&samples)) {
        eprintln!("player: cannot write '{out}': {error}");
        return ExitCode::FAILURE;
    }
    println!(
        "player: wrote {:.1} s to {out}",
        samples.len() as f64 / 2.0 / f64::from(SAMPLE_RATE)
    );
    ExitCode::SUCCESS
}
