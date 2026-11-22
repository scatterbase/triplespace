#!/bin/sh
# Runs one fuzz target for a bounded time, seeded from the repository's fixtures (22 §4.9).
#
#   fuzz/run.sh <wikitext|wikitext_parse|wikitext_expand|markdown|cbor_record> [seconds]
#                                                              default 60 seconds
#
# Each target's limits live here, so a local run and the nightly workflow agree:
#   -max_len        the longest input; the parsers take whole pages, the decoder short records
#   -timeout        well above what the wikitext parser's step budget allows (0033 §9.1, A36),
#                   so a slow input is reported as a timeout rather than hidden by libFuzzer's
#                   default
#   -rss_limit_mb   libFuzzer's default, made explicit
# Needs a nightly toolchain and cargo-fuzz (`cargo +nightly install cargo-fuzz`).
set -eu
target=${1:?target: wikitext, wikitext_parse, wikitext_expand, markdown or cbor_record}
seconds=${2:-60}
root=$(cd "$(dirname "$0")/.." && pwd)

case "$target" in
    wikitext)    opts="-max_len=32768 -timeout=20" ;;
    wikitext_parse) opts="-max_len=32768 -timeout=20" ;;
    wikitext_expand) opts="-max_len=32768 -timeout=20" ;;
    markdown)    opts="-max_len=32768 -timeout=20" ;;
    cbor_record) opts="-max_len=4096 -timeout=10" ;;
    *) echo "unknown target: $target" >&2; exit 2 ;;
esac

python3 "$root/fuzz/seed.py" "$target"
cd "$root"
# cargo-fuzz builds for the triple it was itself built for unless told otherwise, and a
# prebuilt cargo-fuzz (CI's, through cargo-binstall) is a static musl binary: the sanitizer
# then refuses the static libc and the build fails. Build for the toolchain's own host.
host=$(rustc +nightly -vV | sed -n 's/^host: //p')
# shellcheck disable=SC2086
exec cargo +nightly fuzz run --target "$host" "$target" -- -max_total_time="$seconds" -rss_limit_mb=2560 $opts
