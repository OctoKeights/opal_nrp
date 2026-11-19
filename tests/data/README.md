# Recorded results

`chip_expected.txt` and `music_expected.txt` hold the hashes `compare/dump_opal.c` prints
when built against opal 3.0.0: for synthetic register programs, and for made-up music
files played by opal's example player. `tests/golden.rs` checks that opal_nrp prints the
same.

To record them again, run the comparison once, which builds opal's side, then run that
program directly:

    compare/compare.sh <checkout of opal> 100 1 chip
    ref="${TMPDIR:-/tmp}/opal_nrp-compare/dump-opal"
    { "$ref" chip 3 120; "$ref" chip 8 120; } > tests/data/chip_expected.txt
    { "$ref" music 3 30; "$ref" music 8 30; } > tests/data/music_expected.txt

All test data is generated from seeds; no external files are used.
