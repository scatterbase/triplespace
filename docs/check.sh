#!/bin/sh
# Runs every documentation checker and exits non-zero if any of them fails.
#
#   docs/check.sh            check (what CI runs)
#   docs/check.sh --write    regenerate decisions/INDEX.md, the settings tables of chapter 23 and
#                            the crate map's views (22 §2.1, CRATES.md), then check
#
# Works from any directory. PYTHON= names the interpreter (default python3). The log test
# vectors are never regenerated here: run `python3 docs/api/vectors/check.py --write` by hand.
set -u
DOCS=$(cd "$(dirname "$0")" && pwd)
cd "$DOCS" || exit 2
PY=${PYTHON:-python3}

# tomllib is in the standard library from Python 3.11; older interpreters use the vendored wheel.
if ! "$PY" -c 'import tomllib' 2>/dev/null; then
    for whl in "$DOCS"/tomli-*.whl; do
        [ -e "$whl" ] && PYTHONPATH="$whl${PYTHONPATH:+:$PYTHONPATH}" && export PYTHONPATH
        break
    done
fi

WRITE=
case "${1:-}" in
    --write) WRITE=1 ;;
    "") ;;
    *) echo "usage: docs/check.sh [--write]" >&2; exit 2 ;;
esac

failed=
run() {
    name=$1; shift
    printf '\n== %s ==\n' "$name"
    if ! "$@"; then
        failed="$failed
  - $name"
    fi
}

run "ADRs (24 §12)"               "$PY" decisions/check_adrs.py ${WRITE:+--index} .
run "Chapter provenance (24 §12)" "$PY" architecture/map.py .
run "Settings catalogue (0085 §1)" "$PY" architecture/settings.py ${WRITE:+--write} .
run "Crate map (24 §11)"           "$PY" architecture/crates.py ${WRITE:+--write} .
run "Log test vectors (payloads.md §1–2)" "$PY" api/vectors/check.py

if [ -n "$failed" ]; then
    printf '\nFAILED:%s\n' "$failed"
    exit 1
fi
printf '\nAll documentation checks passed.\n'
