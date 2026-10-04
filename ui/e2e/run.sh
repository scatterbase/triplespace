#!/usr/bin/env bash
# The end-to-end suite (web-frontend plan §7, Phase 1): a database loaded from the server's
# test fixtures, the API with its site off, two web replicas, and an edge that splits the
# paths and alternates the replicas; then Playwright, with JavaScript off, and axe.
#
#   TRIPLESPACE_TEST_DATABASE_URL=postgres://… ui/e2e/run.sh [playwright args]
#
# Needs the binaries built (`cargo build -p triplespace-cli -p triplespace-server
# -p triplespace-web`; BIN overrides target/debug) and `ui/dist` built before them.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
bin="${BIN:-$root/target/debug}"
admin="${TRIPLESPACE_TEST_DATABASE_URL:?set TRIPLESPACE_TEST_DATABASE_URL}"
db=tse2e
url="${admin%/*}/$db"
work="$(mktemp -d)"
pids=()
cleanup() {
	for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
	rm -rf "$work"
}
trap cleanup EXIT

psql "$admin" -q -c "DROP DATABASE IF EXISTS $db WITH (FORCE)" -c "CREATE DATABASE $db" >/dev/null
fixtures="$root/crates/triplespace-server/tests/fixtures"
echo 'e2e-owner-secret-1234' > "$work/owner.pw"
export TRIPLESPACE_DATABASE_URL="$url"
"$bin/triplespace" instance create --tenant librarybase --base https://librarybase.org \
	--key-file "$work/k.key" --provider internetdomains --adopt https://librarybase.org/ \
	--owner 7 --owner-name Alice --owner-password-file "$work/owner.pw" >/dev/null
"$bin/triplespace" adopt --tenant librarybase --dump "$fixtures/site-entities.xml" \
	--source https://librarybase.org/ --version test --frozen --counters item=6,property=12 >/dev/null
"$bin/triplespace" sync --provider internetdomains --dump "$fixtures/internetdomains.json" \
	--version 20261001 >/dev/null

"$bin/triplespace-server" --listen 127.0.0.1:18180 --key-file "$work/k.key" --mode development \
	--dev-tenant librarybase --trusted-proxy 127.0.0.1 --ui off >"$work/api.log" 2>&1 &
pids+=($!)
for port in 18181 18182; do
	"$bin/triplespace-web" --api http://127.0.0.1:18180 --listen "127.0.0.1:$port" >"$work/web-$port.log" 2>&1 &
	pids+=($!)
done
"$bin/triplespace-web" routes --format json >"$work/routes.json"
node "$here/edge.mjs" 18100 "$work/routes.json" http://127.0.0.1:18180 \
	http://127.0.0.1:18181 http://127.0.0.1:18182 &
pids+=($!)

for _ in $(seq 50); do
	if curl -sf -o /dev/null http://127.0.0.1:18100/wiki/Main_Page; then break; fi
	sleep 0.2
done

cd "$here/.."
E2E_BASE=http://127.0.0.1:18100 E2E_OWNER_PASSWORD="$(cat "$work/owner.pw")" npx playwright test "$@" || {
	status=$?
	echo "--- API log"; tail -n 50 "$work/api.log"
	exit $status
}
