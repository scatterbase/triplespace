#!/usr/bin/env bash
# MediaWiki import check (21 §4.2; 0047 §8.1): an export written by Triplespace's
# `POST /export` (MediaWiki XML, schema 0.11) imported into a stock MediaWiki 1.43 with
# Wikibase REL1_43 running in containers, then read back through that wiki's API. The
# Librarybase sample fixture is adopted into a fresh Triplespace instance, its wikitext
# pages exported with history and its items without, `importDump.php` loads both, and the
# check compares what MediaWiki holds (page count, revision count, the latest text of a
# page, an item's label) with what was exported.
#
#   TRIPLESPACE_TEST_DATABASE_URL=postgres://… tools/mw_import_check.sh
#
# Needs `cargo build -p triplespace-cli -p triplespace-server` (BIN overrides
# target/debug), psql, curl, python3, and a container runtime: docker (or podman, with
# RUNTIME=podman) with a daemon running and the images
# `wikibase/wikibase:3` (MediaWiki 1.43 + Wikibase REL1_43) and `mariadb:10.11` pullable.
# Without a runtime the script says so and exits 3, so that CI may skip it.
#
# Everything the run creates lives in a temporary directory removed on exit, and the two
# containers are removed with it; KEEP=1 keeps the containers and the database for a look.
# PAGES names the titles exported (default: every Project page and the first ten items).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
bin="${BIN:-$root/target/debug}"
runtime="${RUNTIME:-docker}"
admin="${TRIPLESPACE_TEST_DATABASE_URL:?set TRIPLESPACE_TEST_DATABASE_URL}"
db="${DB:-tsmwimport}"
url="${admin%/*}/$db"
port="${PORT:-18380}"
mwport="${MW_PORT:-18480}"
api="http://127.0.0.1:$port"
mw="http://127.0.0.1:$mwport"
tenant=librarybase
fixture="${FIXTURE:-$root/docs/samples/librarybase/dump/librarybase-sample.xml.gz}"
pages_fixture="$root/docs/samples/librarybase/dump/librarybase-wikitext.xml.gz"
work="$(mktemp -d)"
pids=()
containers=()
cleanup() {
	status=$?
	for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
	if [ "$status" -ne 0 ] && [ -f "$work/api.log" ]; then
		echo "--- API log"
		tail -n 30 "$work/api.log"
	fi
	if [ -z "${KEEP:-}" ]; then
		for c in "${containers[@]}"; do "$runtime" rm -f "$c" >/dev/null 2>&1 || true; done
		"$runtime" network rm tsmwimport >/dev/null 2>&1 || true
		psql "$admin" -q -c "DROP DATABASE IF EXISTS $db WITH (FORCE)" >/dev/null 2>&1 || true
	fi
	rm -rf "$work"
	exit "$status"
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$*"; }

if ! command -v "$runtime" >/dev/null 2>&1 || ! "$runtime" info >/dev/null 2>&1; then
	echo "no container runtime ($runtime) is running; see tools/README.md for how to run this check"
	exit 3
fi

step "database $db"
psql "$admin" -q -c "DROP DATABASE IF EXISTS $db WITH (FORCE)" -c "CREATE DATABASE $db" >/dev/null
export TRIPLESPACE_DATABASE_URL="$url"
echo 'import-owner-secret-1234' >"$work/owner.pw"
"$bin/triplespace" instance create --farm-slug scatter --tenant $tenant --base https://librarybase.org \
	--key-file "$work/k.key" --adopt https://librarybase.org/ \
	--owner 3 --owner-name Harej --owner-password-file "$work/owner.pw" >/dev/null

step "adopt $(basename "$pages_fixture") and $(basename "$fixture")"
"$bin/triplespace" adopt --tenant $tenant --dump "$pages_fixture" \
	--source https://librarybase.org/ --version wikitext --frozen --counters item=0,property=0 2>&1 | tail -n 3
"$bin/triplespace" adopt --tenant $tenant --dump "$fixture" \
	--source https://librarybase.org/ --version sample --frozen --no-accounts --skip-talk 2>&1 | tail -n 3

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

step "export"
# Every Project page with history, and the first ten items current-only.
if [ -n "${PAGES:-}" ]; then
	titles="$PAGES"
else
	project="$(curl -sf "$api/w/api.php?action=query&list=allpages&apnamespace=4&aplimit=max&format=json&formatversion=2" \
		| python3 -c 'import json,sys; print("\n".join(p["title"] for p in json.load(sys.stdin)["query"]["allpages"]))')"
	items="$(psql "$url" -Atc "SELECT 'Item:' || id FROM view.entity WHERE tenant = '$tenant' AND type = 'item' ORDER BY length(id), id LIMIT 10")"
	props="$(psql "$url" -Atc "SELECT 'Property:' || id FROM view.entity WHERE tenant = '$tenant' AND type = 'property' ORDER BY length(id), id")"
	titles="$project"$'\n'"$props"$'\n'"$items"
fi
python3 - "$titles" >"$work/pages.json" <<'PY'
import json, sys
titles = [t for t in sys.argv[1].splitlines() if t.strip()]
print(json.dumps({"format": "xml", "pages": titles, "history": True}))
PY
curl -sf -X POST -H 'Content-Type: application/json' --data-binary @"$work/pages.json" \
	"$api/w/rest.php/triplespace/v0/export" -D "$work/export.headers" -o "$work/export.xml"
echo "exported $(grep -c '<page>' "$work/export.xml") pages, $(grep -c '<revision>' "$work/export.xml") revisions; $(grep -i x-export-missing "$work/export.headers" | tr -d '\r')"

step "MediaWiki 1.43 + Wikibase in containers"
"$runtime" network create tsmwimport >/dev/null
"$runtime" run -d --name tsmwimport-db --network tsmwimport \
	-e MARIADB_DATABASE=my_wiki -e MARIADB_USER=wikiuser -e MARIADB_PASSWORD=sqlpass -e MARIADB_RANDOM_ROOT_PASSWORD=yes \
	mariadb:10.11 >/dev/null
containers+=(tsmwimport-db)
"$runtime" run -d --name tsmwimport-wb --network tsmwimport -p "127.0.0.1:$mwport:80" \
	-e DB_SERVER=tsmwimport-db:3306 -e DB_NAME=my_wiki -e DB_USER=wikiuser -e DB_PASS=sqlpass \
	-e MW_ADMIN_NAME=Admin -e MW_ADMIN_PASS=adminpass1234 -e MW_ADMIN_EMAIL=admin@example.invalid \
	-e MW_WG_SERVER="$mw" -e MW_WG_SITENAME=ImportCheck -e MW_WG_LANGUAGE_CODE=en \
	-e WIKIBASE_PINGBACK=no -e MW_WG_ENABLE_UPLOADS=false \
	wikibase/wikibase:3 >/dev/null
containers+=(tsmwimport-wb)
for _ in $(seq 300); do
	if curl -sf -o /dev/null "$mw/w/api.php?action=query&meta=siteinfo&format=json"; then break; fi
	sleep 1
done
curl -sf "$mw/w/api.php?action=query&meta=siteinfo&format=json" | python3 -c 'import json,sys; g=json.load(sys.stdin)["query"]["general"]; print(g["generator"], g["sitename"])' || {
	echo "MediaWiki did not come up"
	exit 1
}

step "importDump.php"
"$runtime" cp "$work/export.xml" tsmwimport-wb:/tmp/export.xml
# Entity pages need Wikibase's import hook (`$wgWBRepoSettings['allowEntityImport']`),
# which the suite's image leaves off: turn it on for this run.
"$runtime" exec tsmwimport-wb sh -c 'echo "\$wgWBRepoSettings[\"allowEntityImport\"] = true;" >> /var/www/html/LocalSettings.php'
"$runtime" exec tsmwimport-wb php /var/www/html/maintenance/run.php importDump --no-updates /tmp/export.xml 2>&1 | tail -n 5
"$runtime" exec tsmwimport-wb php /var/www/html/maintenance/run.php rebuildrecentchanges >/dev/null 2>&1 || true

step "compare"
python3 - "$work/export.xml" "$mw" <<'PY'
import json, re, sys, urllib.parse, urllib.request
import xml.etree.ElementTree as ET

xml_path, mw = sys.argv[1], sys.argv[2]
ns = {"mw": "http://www.mediawiki.org/xml/export-0.11/"}
tree = ET.parse(xml_path)
pages = tree.getroot().findall("mw:page", ns)
failures = []

def api(**params):
    params.update(format="json", formatversion="2")
    with urllib.request.urlopen(f"{mw}/w/api.php?" + urllib.parse.urlencode(params)) as r:
        return json.load(r)

for page in pages:
    title = page.find("mw:title", ns).text
    revisions = page.findall("mw:revision", ns)
    latest = max(revisions, key=lambda r: int(r.find("mw:id", ns).text))
    text = latest.find("mw:text", ns).text or ""
    model = latest.find("mw:model", ns).text
    got = api(action="query", prop="revisions", titles=title, rvprop="content|ids", rvslots="main", rvlimit="max")
    p = got["query"]["pages"][0]
    if p.get("missing"):
        failures.append(f"{title}: not imported")
        continue
    revs = p.get("revisions", [])
    if len(revs) != len(revisions):
        failures.append(f"{title}: {len(revs)} revisions in MediaWiki, {len(revisions)} exported")
    content = revs[0]["slots"]["main"]["content"]
    if model.startswith("wikibase-"):
        if json.loads(content).get("id") != json.loads(text).get("id"):
            failures.append(f"{title}: entity ID differs")
    elif content != text:
        failures.append(f"{title}: latest text differs")
print(f"{len(pages)} pages checked")
if failures:
    print("\n".join(failures))
    sys.exit(1)
print("MediaWiki holds every exported page with its revisions and latest text")
PY

step "summary"
echo "import check passed"
