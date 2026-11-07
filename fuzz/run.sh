#!/bin/sh
# Runs one fuzz target for a bounded time, seeded from the repository's fixtures (22 §4.9).
#
#   fuzz/run.sh <wikitext|markdown|cbor_record> [seconds]     default 60
#
# Each target's limits live here, so a local run and the nightly workflow agree:
#   -max_len        the longest input; the parsers take whole pages, the decoder short records
#   -timeout        above the wikitext parser's own five-second bound (0033 §9.1), so a slow
#                   input is reported as a timeout rather than hidden by libFuzzer's default
#   -rss_limit_mb   libFuzzer's default, made explicit
# Needs a nightly toolchain and cargo-fuzz (`cargo +nightly install cargo-fuzz`).
set -eu
target=${1:?target: wikitext, markdown or cbor_record}
seconds=${2:-60}
root=$(cd "$(dirname "$0")/.." && pwd)

case "$target" in
    wikitext)    opts="-max_len=32768 -timeout=20" ;;
    markdown)    opts="-max_len=32768 -timeout=20" ;;
    cbor_record) opts="-max_len=4096 -timeout=10" ;;
    *) echo "unknown target: $target" >&2; exit 2 ;;
esac

python3 "$root/fuzz/seed.py" "$target"
cd "$root"
# shellcheck disable=SC2086
exec cargo +nightly fuzz run "$target" -- -max_total_time="$seconds" -rss_limit_mb=2560 $opts
