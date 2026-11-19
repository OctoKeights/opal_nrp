//! `opal_nrp` against hashes recorded from opal, so that any change in output shows without a
//! C compiler.
//!
//! `tests/data/chip_expected.txt` is what `compare/dump_opal.c`, built against opal 3.0.0,
//! prints for `chip 3 120` and `chip 8 120`: hashes of the samples and status of synthetic
//! register programs. `tests/data/music_expected.txt` is what it prints for `music 3 30` and
//! `music 8 30`: hashes of made-up DRO, IMF and HSC files played by opal's example player.
//! `compare/compare.sh` runs the full comparison.

mod made_up;

fn assert_prints(printed: &str, expected: &str) {
    for (line, (ours, theirs)) in printed.lines().zip(expected.lines()).enumerate() {
        assert_eq!(ours, theirs, "line {}", line + 1);
    }
    assert_eq!(printed.lines().count(), expected.lines().count());
}

#[test]
fn plays_as_opal_does() {
    let printed = [made_up::chip(3, 120), made_up::chip(8, 120)].concat();
    assert_prints(&printed, include_str!("data/chip_expected.txt"));
}

#[test]
fn plays_music_as_opals_player_does() {
    let printed = [made_up::music(3, 30), made_up::music(8, 30)].concat();
    assert_prints(&printed, include_str!("data/music_expected.txt"));
}
