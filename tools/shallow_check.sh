#!/usr/bin/env bash
# Shallow mirroring against live Wikidata (0070; SCA-174): a database loaded from the
# Librarybase sample fixture, a bot account, the API on its own, and then shallow_check.py,
# which links ten Wikidata items from a Librarybase item and checks their labels, their
# fetch, their statements, the closure and the set. It reads www.wikidata.org: a few dozen
# requests, anonymous, one at a time. The closure is walked one step
# (`entities.closure_depth` 1 at the instance) to keep it at that.
#
#   TRIPLESPACE_TEST_DATABASE_URL=postgres://… tools/shallow_check.sh
#
# Needs `cargo build -p triplespace-cli -p triplespace-server` (BIN overrides target/debug),
# psql, and a Python with `requests` (PYTHON overrides python3). Everything it writes lives
# in a temporary directory removed on exit. KEEP_DB=1 leaves the database; ITEMS=Q1,Q2
# names other Wikidata items.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
bin="${BIN:-$root/target/debug}"
python="${PYTHON:-python3}"
admin="${TRIPLESPACE_TEST_DATABASE_URL:?set TRIPLESPACE_TEST_DATABASE_URL}"
db="${DB:-tsshallowcheck}"
url="${admin%/*}/$db"
port="${PORT:-18281}"
api="http://127.0.0.1:$port"
tenant=librarybase
fixture="$root/docs/samples/librarybase/dump/librarybase-sample.xml.gz"
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
echo 'shallow-owner-secret-1234' >"$work/owner.pw"
"$bin/triplespace" instance create --farm-slug scatter --tenant $tenant --base https://librarybase.org \
	--key-file "$work/k.key" --provider wikidata --adopt https://librarybase.org/ \
	--owner 7 --owner-name Alice --owner-password-file "$work/owner.pw" >/dev/null

step "adopt $(basename "$fixture")"
"$bin/triplespace" adopt --tenant $tenant --dump "$fixture" \
	--source https://librarybase.org/ --version sample --frozen 2>&1 | tail -n 3
"$bin/triplespace" settings set --instance --tenancy config.template defaults >/dev/null
"$bin/triplespace" settings set --instance --default entities.closure_depth 1 >/dev/null

step "bot account"
"$bin/triplespace" subsidiary create --tenant $tenant --name ShallowBot --operator Alice --group bot >/dev/null
"$bin/triplespace" subsidiary key --tenant $tenant --name ShallowBot --label check \
	--grant editentity --grant createeditmovepage --grant highvolume --json >"$work/key.json"
login="$($python -c 'import json,sys; print(json.load(open(sys.argv[1]))["login"])' "$work/key.json")"
TRIPLESPACE_WBI_SECRET="$($python -c 'import json,sys; print(json.load(open(sys.argv[1]))["secret"])' "$work/key.json")"
export TRIPLESPACE_WBI_SECRET
item="${ITEM:-$(psql "$url" -Atc "SELECT id FROM view.entity WHERE tenant = '$tenant' AND type = 'item' ORDER BY length(id), id LIMIT 1")}"
echo "login $login; item $item"

step "server on $api"
"$bin/triplespace-server" --listen "127.0.0.1:$port" --key-file "$work/k.key" --mode development \
	--dev-tenant $tenant --trusted-proxy 127.0.0.1 --ui off >"$work/api.log" 2>&1 &
pids+=($!)
for _ in $(seq 100); do
	if curl -sf -o /dev/null "$api/w/api.php?action=query&meta=siteinfo&format=json"; then break; fi
	sleep 0.2
done

step "shallow mirroring against www.wikidata.org"
"$python" "$here/shallow_check.py" --api "$api" --db "$url" --bin "$bin" --key-file "$work/k.key" \
	--login "$login" --item "$item" ${ITEMS:+--items "$ITEMS"}
