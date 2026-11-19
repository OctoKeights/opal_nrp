//! The Rust side of the comparison with opal: runs `opal_nrp` over synthetic data and prints a
//! running hash of its output, as `compare/dump_opal.c` does with opal. `compare/compare.sh`
//! compares the two.
//!
//!   dump chip <seed> <count>               register programs, each on a chip of its own:
//!                                          writes at once and buffered, voices, flushes, pans,
//!                                          rate changes, clones and samples
//!   dump music <seed> <count>              made-up DRO, IMF and HSC files, some damaged, and
//!                                          files of none of them, played in buffers of random
//!                                          sizes
//!   dump tracks <seed> <frames> <file>...  the files given, played

#[path = "../tests/made_up/mod.rs"]
mod made_up;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(what), Some(seed), Some(count)) = (
        args.get(1),
        args.get(2).and_then(|s| s.parse::<u64>().ok()),
        args.get(3).and_then(|s| s.parse::<u32>().ok()),
    ) else {
        eprintln!("usage: dump <what> <seed> <count> [files...]   (see the top of dump.rs)");
        std::process::exit(2);
    };
    match what.as_str() {
        "chip" => print!("{}", made_up::chip(seed, count)),
        "music" => print!("{}", made_up::music(seed, count)),
        "tracks" => {
            let tracks = args[4..]
                .iter()
                .map(|path| {
                    let path = std::path::Path::new(path);
                    let data = std::fs::read(path).unwrap_or_else(|error| {
                        eprintln!("cannot read {}: {error}", path.display());
                        std::process::exit(2);
                    });
                    let name = path
                        .file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                    made_up::Song { name, data }
                })
                .collect();
            print!("{}", made_up::tracks(seed, count, tracks));
        }
        _ => {
            eprintln!("unknown dump {what}");
            std::process::exit(2);
        }
    }
}
