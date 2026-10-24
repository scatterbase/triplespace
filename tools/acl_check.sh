#!/usr/bin/env bash
# The permission model's attack battery (09 §4–§5; 0056) against a server it sets up itself:
# a database loaded from the Librarybase sample fixture, an instance whose owner is Alice,
# the API, and then tools/acl_check.py — every leak, escalation or false denial it finds
# fails the run.
#
#   TRIPLESPACE_TEST_DATABASE_URL=postgres://… tools/acl_check.sh
#
# Needs `cargo build -p triplespace-cli -p triplespace-server` (BIN overrides target/debug),
# psql, and a Python with `requests` (PYTHON overrides python3; requirements-compat.txt has
# it). Everything the run creates lives in a temporary directory removed on exit.
# SECTIONS limits the run (`SECTIONS=leaks,positive`; `acl_check.py --list` names them);
# KEEP_DB=1 leaves the database.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
bin="${BIN:-$root/target/debug}"
python="${PYTHON:-python3}"
admin="${TRIPLESPACE_TEST_DATABASE_URL:?set TRIPLESPACE_TEST_DATABASE_URL}"
db="${DB:-tsacl}"
url="${admin%/*}/$db"
port="${PORT:-18290}"
api="http://127.0.0.1:$port"
tenant=librarybase
fixture="${FIXTURE:-$root/crates/triplespace-server/tests/fixtures/librarybase-sample.xml.gz}"
work="$(mktemp -d)"
pids=()
cleanup() {
	status=$?
	for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
	if [ "$status" -ne 0 ] && [ -f "$work/api.log" ]; then
		echo "--- API log"
		tail -n 50 "$work/api.log"
	fi
	if [ -z "${KEEP_DB:-}" ]; then
		psql "$admin" -q -c "DROP DATABASE IF EXISTS $db WITH (FORCE)" >/dev/null 2>&1 || true
	fi
	rm -rf "$work"
	exit "$status"
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$*"; }

step "database $db"
psql "$admin" -q -c "DROP DATABASE IF EXISTS $db WITH (FORCE)" -c "CREATE DATABASE $db" >/dev/null
export TRIPLESPACE_DATABASE_URL="$url"
echo 'acl-owner-secret-1234' >"$work/owner.pw"
"$bin/triplespace" instance create --farm-slug scatter --tenant $tenant --base https://librarybase.org \
	--key-file "$work/k.key" --provider internetdomains --adopt https://librarybase.org/ \
	--owner 7 --owner-name Alice --owner-password-file "$work/owner.pw" >/dev/null

# The battery registers six accounts from one address; the registration challenge rises a
# bit for each (0087 §4), so at the default 20 bits the last one takes minutes. Permissions
# are under test, not proof-of-work: set the challenge to its floor.
"$bin/triplespace" settings set --tenant $tenant challenge.bits 8 >/dev/null
"$bin/triplespace" settings set --tenant $tenant challenge.max_bits 8 >/dev/null

step "adopt $(basename "$fixture")"
"$bin/triplespace" adopt --tenant $tenant --dump "$fixture" \
	--source https://librarybase.org/ --version sample --frozen 2>&1 | tail -n 3

step "server on $api"
"$bin/triplespace-server" --listen "127.0.0.1:$port" --key-file "$work/k.key" --mode development \
	--dev-tenant $tenant --trusted-proxy 127.0.0.1 --ui off >"$work/api.log" 2>&1 &
pids+=($!)
for _ in $(seq 100); do
	if curl -sf -o /dev/null "$api/w/api.php?action=query&meta=siteinfo&format=json"; then break; fi
	sleep 0.2
done
curl -sf -o /dev/null "$api/w/api.php?action=query&meta=siteinfo&format=json" || {
	echo "server did not come up"
	exit 1
}

step "attack battery"
export HOME="$work/home"
mkdir -p "$HOME"
"$python" "$here/acl_check.py" --api "$api" --tenant $tenant --owner-password-file "$work/owner.pw" \
	--cli "$bin/triplespace" --db "$url" ${SECTIONS:+--sections "$SECTIONS"}
