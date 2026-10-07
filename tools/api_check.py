#!/usr/bin/env python3
"""Check adopted entities through the Action API (test plan 1.4), after `adopt`.

    # server up:  triplespace-server --database "$URL" --key-file triplespace.key \
    #                 --mode development --dev-tenant librarybase --listen 127.0.0.1:8080
    tools/api_check.py --ids Q1 Q100 P1
    tools/api_check.py --db "$URL" --sample 20                  # random adopted entities
    tools/api_check.py --db "$URL" --sample 20 --compare https://librarybase.org/

What is checked, per entity:
  1. Special:EntityData/{id}.json serves it, and EVERY snak (mainsnak, qualifiers,
     references) carries `datatype` -- the bug of adoption-throughput.md section 2b.
  2. wbgetentities returns the same entity, with pageid and lastrevid.
  3. wbsearchentities finds it by its English (or first) label.
  4. With --db: view.identifier holds a row for each external-id mainsnak with a value.
  5. With --compare: the entity equals the source's Special:EntityData output, ignoring
     lastrevid/modified/pageid/ns/title (the test plan's spot check); pageid equals the
     source's; lastrevid is a fresh local ID above the source's (0035 section 3-4).
Exit status is non-zero when any check fails. Standard library only.
"""

import argparse
import json
import random
import subprocess
import sys
import urllib.parse
import urllib.request

META = {"lastrevid", "modified", "pageid", "ns", "title"}


def get_json(url):
    with urllib.request.urlopen(url, timeout=60) as r:
        return json.load(r)


def api(base, **params):
    params.setdefault("format", "json")
    return get_json(f"{base.rstrip('/')}/w/api.php?" + urllib.parse.urlencode(params))


def entity_data(base, eid):
    doc = get_json(f"{base.rstrip('/')}/wiki/Special:EntityData/{eid}.json")
    return doc.get("entities", {}).get(eid)


def snaks(entity):
    """Every snak in the entity, with where it sits."""
    for prop, statements in entity.get("claims", {}).items():
        for s in statements:
            yield "mainsnak", s.get("mainsnak", {})
            for q in [x for group in s.get("qualifiers", {}).values() for x in group]:
                yield "qualifier", q
            for ref in s.get("references", []):
                for x in [x for group in ref.get("snaks", {}).values() for x in group]:
                    yield "reference", x


def psql(db, sql):
    out = subprocess.run(
        ["psql", db, "-At", "-c", sql], capture_output=True, text=True, check=True
    )
    return [line for line in out.stdout.splitlines() if line]


def sample_ids(db, tenant, n):
    rows = psql(
        db,
        f"SELECT id FROM view.entity WHERE tenant = '{tenant}' AND type IN ('item','property') "
        f"ORDER BY random() LIMIT {int(n)}",
    )
    return rows


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--api", default="http://127.0.0.1:8080", help="server base URL")
    ap.add_argument("--tenant", default="librarybase")
    ap.add_argument("--ids", nargs="*", default=[], help="entity IDs to check")
    ap.add_argument("--sample", type=int, default=0, help="also check N random adopted entities (needs --db)")
    ap.add_argument("--db", help="Postgres URL, for --sample and the view.identifier check (uses psql)")
    ap.add_argument("--compare", metavar="SOURCE_BASE", help="compare with the source wiki's Special:EntityData")
    a = ap.parse_args()

    ids = list(a.ids)
    if a.sample:
        if not a.db:
            sys.exit("--sample needs --db")
        ids += sample_ids(a.db, a.tenant, a.sample)
    if not ids:
        sys.exit("nothing to check: pass --ids or --db --sample N")

    failures = 0

    def fail(eid, what):
        nonlocal failures
        failures += 1
        print(f"FAIL {eid}: {what}")

    # 0. the server answers for the tenant
    try:
        info = api(a.api, action="query", meta="siteinfo", siprop="general")["query"]["general"]
        print(f"server: {info.get('sitename')} at {info.get('server', a.api)}")
    except Exception as e:  # noqa: BLE001
        sys.exit(f"siteinfo failed: {e} (is the server up, with --dev-tenant {a.tenant}?)")

    for eid in ids:
        # 1. EntityData and datatypes
        try:
            ent = entity_data(a.api, eid)
        except Exception as e:  # noqa: BLE001
            fail(eid, f"EntityData: {e}")
            continue
        if not ent or "missing" in ent:
            fail(eid, "EntityData: missing")
            continue
        untyped = {}
        total = 0
        for where, snak in snaks(ent):
            total += 1
            if "datatype" not in snak:
                untyped.setdefault(snak.get("property", "?"), []).append(where)
        if untyped:
            fail(eid, f"{sum(map(len, untyped.values()))}/{total} snaks lack datatype: "
                      + ", ".join(f"{p}({len(w)})" for p, w in sorted(untyped.items())))

        # 2. wbgetentities
        try:
            got = api(a.api, action="wbgetentities", ids=eid)["entities"].get(eid, {})
        except Exception as e:  # noqa: BLE001
            fail(eid, f"wbgetentities: {e}")
            got = {}
        if "missing" in got or not got:
            fail(eid, "wbgetentities: missing")
        else:
            for k in ("pageid", "lastrevid"):
                if not got.get(k):
                    fail(eid, f"wbgetentities: no {k}")
            if got.get("claims") != ent.get("claims"):
                fail(eid, "wbgetentities claims differ from EntityData")

        # 3. search by label
        labels = ent.get("labels", {})
        label = labels.get("en") or (next(iter(labels.values())) if labels else None)
        if label:
            lang = label["language"]
            try:
                hits = api(a.api, action="wbsearchentities", search=label["value"],
                           language=lang, type=ent.get("type", "item"), limit=50)
                found = {h["id"] for h in hits.get("search", [])}
                if eid not in found:
                    # A catalogue has many records sharing a title prefix; past the
                    # limit the miss is ambiguity, not a search fault.
                    sharing = None
                    if a.db:
                        q = label["value"].replace("'", "''")
                        sharing = int(psql(a.db, "SELECT count(*) FROM view.term WHERE tenant = "
                                           f"'{a.tenant}' AND kind = 1 AND lang = '{lang}' AND "
                                           f"lower(text) LIKE lower('{q}') || '%'")[0])
                    if sharing is not None and sharing > 50:
                        print(f"note {eid}: label shared as a prefix by {sharing} entities; "
                              "wbsearchentities(limit=50) cannot single it out")
                    else:
                        fail(eid, f"wbsearchentities({lang!r}, {label['value']!r}) did not return it"
                                  + (f" ({sharing} entities share the prefix)" if sharing is not None else ""))
            except Exception as e:  # noqa: BLE001
                fail(eid, f"wbsearchentities: {e}")

        # 4. identifiers indexed
        if a.db:
            ext = [s for where, s in snaks(ent) if where == "mainsnak"
                   and s.get("datatype") == "external-id" and s.get("snaktype") == "value"]
            if ext:
                rows = psql(a.db, "SELECT property, statement_id FROM view.identifier "
                                  f"WHERE tenant = '{a.tenant}' AND entity_id = '{eid}'")
                have = {r.split("|")[0] for r in rows}
                want = {s["property"] for s in ext}
                if not want <= have:
                    fail(eid, f"view.identifier lacks {sorted(want - have)} (has {sorted(have)})")

        # 5. compare with the source
        if a.compare:
            try:
                src = entity_data(a.compare, eid)
            except Exception as e:  # noqa: BLE001
                fail(eid, f"source EntityData: {e}")
                src = None
            if src:
                mine = {k: v for k, v in ent.items() if k not in META}
                theirs = {k: v for k, v in src.items() if k not in META}
                if mine != theirs:
                    diff = [k for k in sorted(set(mine) | set(theirs)) if mine.get(k) != theirs.get(k)]
                    fail(eid, f"differs from source in: {', '.join(diff)}")
                if ent.get("pageid") != src.get("pageid"):
                    fail(eid, f"pageid {ent.get('pageid')} != source {src.get('pageid')}")
                # 0035 section 3: an adopted record is a fresh local revision above the
                # floor (section 4), never the source's number.
                if not (ent.get("lastrevid") or 0) > (src.get("lastrevid") or 0):
                    fail(eid, f"lastrevid {ent.get('lastrevid')} is not above the source's {src.get('lastrevid')}")

        print(f"seen {eid}: {total} snaks" + (f", label {label['value']!r}" if label else ""))

    print(f"\n{len(ids)} entities, {failures} failures")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
