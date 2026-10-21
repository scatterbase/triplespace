#!/usr/bin/env bash
# Prepares a Linux workspace (Debian or Ubuntu, e.g. a cloud agent's container) to run the whole
# test suite: a running Postgres with a role that may create databases and roles, the test
# database URL, and the pinned Wikibase client libraries for tools/client_compat.sh.
#
#   eval "$(tools/dev_env.sh)"                 set up, then export the variables into this shell
#   eval "$(tools/dev_env.sh --ui --nextest)"  also `npm ci` in ui/ and install cargo-nextest
#   tools/dev_env.sh --no-compat               skip the Python client libraries
#
# Idempotent: a second run finds everything in place and only prints the exports. Progress goes
# to stderr; stdout carries only `export` lines. Without TRIPLESPACE_TEST_DATABASE_URL the
# database tests print "skipping" and pass without testing anything, so check that it is set
# before reporting a test run. On macOS, use Postgres.app instead (docs/testing/real-dumps.md §0.1).
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
COMPAT=1 UI= NEXTEST=
for arg in "$@"; do
    case "$arg" in
        --no-compat) COMPAT= ;;
        --ui) UI=1 ;;
        --nextest) NEXTEST=1 ;;
        -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 0 ;;
        *) echo "dev_env.sh: unknown option $arg" >&2; exit 2 ;;
    esac
done

log() { printf 'dev_env: %s\n' "$*" >&2; }
die() { printf 'dev_env: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = Linux ] || die "Linux only; on macOS use Postgres.app (docs/testing/real-dumps.md §0.1)"
ME=$(id -un)
if [ "$(id -u)" = 0 ]; then
    SUDO=
    as_postgres() { runuser -u postgres -- "$@"; }
else
    command -v sudo >/dev/null || die "not root and no sudo: cannot install or start Postgres"
    SUDO=sudo
    as_postgres() { sudo -u postgres "$@"; }
fi
apt_install() {
    log "installing $*"
    $SUDO apt-get update -qq >&2
    DEBIAN_FRONTEND=noninteractive $SUDO apt-get install -y -qq "$@" >&2
}

# --- Postgres --------------------------------------------------------------------------------
if ! ls -d /usr/lib/postgresql/*/bin >/dev/null 2>&1; then
    apt_install postgresql postgresql-contrib
fi
VER=$(ls /usr/lib/postgresql | sort -n | tail -1)
command -v pg_lsclusters >/dev/null || apt_install postgresql-common
if ! pg_lsclusters -h | awk -v v="$VER" '$1 == v && $2 == "main"' | grep -q .; then
    log "creating cluster $VER/main"
    $SUDO pg_createcluster "$VER" main >&2
fi
if ! pg_lsclusters -h | awk -v v="$VER" '$1 == v && $2 == "main" && $4 == "online"' | grep -q .; then
    log "starting Postgres $VER"
    $SUDO pg_ctlcluster "$VER" main start >&2
fi
PORT=$(pg_lsclusters -h | awk -v v="$VER" '$1 == v && $2 == "main" {print $3}')
# The first of the server's socket directories (pg_lsclusters does not report it).
SOCKET=$(as_postgres psql -p "$PORT" -tAc "SHOW unix_socket_directories" | cut -d, -f1 | tr -d ' ')
SOCKET=${SOCKET:-/var/run/postgresql}

# The tests CREATE DATABASE one scratch database per test, and triplespace-db's tests create
# roles, so the role needs CREATEDB and CREATEROLE. Peer authentication over the socket needs
# no password; tokio-postgres reads neither PGPASSWORD nor ~/.pgpass.
if [ "$(as_postgres psql -p "$PORT" -tAc "SELECT 1 FROM pg_roles WHERE rolname = '$ME'")" != 1 ]; then
    log "creating role $ME"
    as_postgres psql -p "$PORT" -qc "CREATE ROLE \"$ME\" LOGIN CREATEDB CREATEROLE" >&2
else
    as_postgres psql -p "$PORT" -qc "ALTER ROLE \"$ME\" LOGIN CREATEDB CREATEROLE" >&2
fi

# URL-shaped, with the socket directory percent-encoded: the test suites and ui/e2e/run.sh swap
# the database by replacing the text after the last "/", which a key=value string would break.
ENC_SOCKET=$(printf '%s' "$SOCKET" | sed 's|/|%2F|g')
PORT_PART=
[ "${PORT:-5432}" = 5432 ] || PORT_PART=":$PORT"
URL="postgresql://$ME@$ENC_SOCKET$PORT_PART/postgres"

# Prove the role can do what the tests do, rather than discovering it as a vacuous pass.
probe="dev_env_probe_$$"
psql "$URL" -qc "CREATE DATABASE $probe" >&2 && psql "$URL" -qc "DROP DATABASE $probe" >&2 \
    || die "role $ME cannot create databases through $URL"
log "Postgres $VER ready at $SOCKET (port ${PORT:-5432}); role $ME may create databases and roles"

EXPORTS="export TRIPLESPACE_TEST_DATABASE_URL='$URL'"

# --- Rust toolchain --------------------------------------------------------------------------
# rust-toolchain.toml pins the toolchain. A sandbox that cannot reach static.rust-lang.org cannot
# install it; then use the newest installed stable (it satisfies rust-version) and say so, since
# formatting, lints and the wasm32 target can differ from what CI runs.
PIN=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$ROOT/rust-toolchain.toml")
if command -v rustup >/dev/null && [ -n "$PIN" ]; then
    if rustup toolchain list 2>/dev/null | grep -q "^$PIN"; then
        log "Rust $PIN (pinned) installed"
    elif timeout 300 rustup toolchain install "$PIN" --profile minimal -c rustfmt -c clippy \
            -t wasm32-unknown-unknown >/dev/null 2>&1; then
        log "installed Rust $PIN (pinned)"
    elif rustup toolchain list 2>/dev/null | grep -q '^stable'; then
        EXPORTS="$EXPORTS
export RUSTUP_TOOLCHAIN=stable"
        log "WARNING: cannot install the pinned Rust $PIN; using $(rustc +stable --version | cut -d' ' -f1-2) via RUSTUP_TOOLCHAIN=stable."
        log "WARNING: say so when reporting results: rustfmt and clippy may differ from CI's, and wasm32 may be missing."
    else
        log "WARNING: cannot install the pinned Rust $PIN and no stable toolchain is installed"
    fi
fi

# --- Wikibase client libraries (tools/client_compat.sh) -------------------------------------
if [ -n "$COMPAT" ]; then
    VENV="$ROOT/.venv"
    if [ ! -x "$VENV/bin/python" ]; then
        python3 -m venv "$VENV" >&2 2>/dev/null || {
            apt_install "python3-venv"
            rm -rf "$VENV"
            python3 -m venv "$VENV" >&2
        }
    fi
    if ! cmp -s "$ROOT/tools/requirements-compat.txt" "$VENV/.requirements-compat.txt"; then
        log "installing the pinned client libraries into .venv"
        "$VENV/bin/pip" install -q --disable-pip-version-check -r "$ROOT/tools/requirements-compat.txt" >&2
        cp "$ROOT/tools/requirements-compat.txt" "$VENV/.requirements-compat.txt"
    fi
    EXPORTS="$EXPORTS
export PYTHON='$VENV/bin/python'"
    log "client libraries ready in .venv (PYTHON for tools/client_compat.sh)"
fi

# --- Optional: front end and nextest ---------------------------------------------------------
if [ -n "$UI" ]; then
    command -v npm >/dev/null || die "--ui needs Node 22 and npm"
    log "npm ci in ui/"
    (cd "$ROOT/ui" && npm ci --no-audit --no-fund >&2)
fi
if [ -n "$NEXTEST" ] && ! command -v cargo-nextest >/dev/null; then
    log "installing cargo-nextest (a few minutes; cargo test --workspace works without it)"
    cargo install cargo-nextest --locked -q >&2
fi

printf '%s\n' "$EXPORTS"
