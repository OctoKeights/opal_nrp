#!/bin/sh
# Compares opal_nrp with opal (https://github.com/RealBitdancer/opal) 3.0.0.
#
#   compare/compare.sh <checkout of opal> [count] [seeds] [dumps]
#
# compare/dump_opal.c is built against opal's core and its example player's format readers,
# and examples/dump.rs against opal_nrp. Both run the same synthetic data for each seed, and
# their output is compared byte for byte. The dumps are chip (count register programs, 4,000
# by default), music (a tenth of count made-up songs) and tracks (the HSC tracks in the
# checkout's music folder, each played for a minute).
set -u
if [ $# -lt 1 ]; then
    echo "usage: $0 <checkout of opal> [count] [seeds] [dumps]" >&2
    exit 2
fi
here=$(cd "$(dirname "$0")/.." && pwd)
source=$1
programs=${2:-4000}
seeds=${3:-3}
dumps=${4:-chip music tracks}
tmp=${TMPDIR:-/tmp}/opal_nrp-compare
mkdir -p "$tmp" || exit 2
if ! [ -f "$source/src/opal.c" ] || ! [ -f "$source/examples/player/src/format_hsc.c" ]; then
    echo "$source does not look like a checkout of opal" >&2
    exit 2
fi

(cd "$here" && cargo build --release -q --ignore-rust-version --example dump) || exit 2
rust=$(cd "$here" && cargo metadata --format-version 1 --no-deps 2>/dev/null | sed 's/.*"target_directory":"\([^"]*\)".*/\1/')/release/examples/dump

player=$source/examples/player/src
${CC:-cc} -std=c11 -O2 -I "$source/include" -I "$player" "$here/compare/dump_opal.c" \
    "$source/src/opal.c" "$player/format_dro.c" "$player/format_hsc.c" "$player/format_imf.c" \
    -o "$tmp/dump-opal" || exit 2

bad=0
for what in $dumps; do
    case $what in
    chip) count=$programs ;;
    music) count=$((programs / 10)) ;;
    tracks) count=2646000 ;; # a minute at 44,100 Hz
    *)
        echo "unknown dump $what" >&2
        exit 2
        ;;
    esac
    seed=1
    while [ "$seed" -le "$seeds" ]; do
        if [ "$what" = tracks ]; then
            "$tmp/dump-opal" tracks "$seed" "$count" "$source"/music/*.hsc > "$tmp/reference.txt" 2> /dev/null
            "$rust" tracks "$seed" "$count" "$source"/music/*.hsc > "$tmp/rust.txt"
        else
            "$tmp/dump-opal" "$what" "$seed" "$count" > "$tmp/reference.txt" 2> /dev/null
            "$rust" "$what" "$seed" "$count" > "$tmp/rust.txt"
        fi
        if [ ! -s "$tmp/reference.txt" ]; then
            echo "$what, seed $seed: NOT COMPARED, the original's side printed nothing"
            bad=$((bad + 1))
        elif cmp -s "$tmp/reference.txt" "$tmp/rust.txt"; then
            echo "$what, seed $seed: the same ($(tail -1 "$tmp/rust.txt"))"
        else
            echo "$what, seed $seed: DIFFERENT"
            bad=$((bad + 1))
        fi
        seed=$((seed + 1))
    done
done
[ "$bad" -eq 0 ]
