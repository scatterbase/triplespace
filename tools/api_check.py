#!/usr/bin/env python3
"""Check adopted entities through the Action API (test plan 1.4), after `adopt`.

    # server up:  triplespace-server --database "$URL" --key-file triplespace.key \
    #                 --mode development --dev-tenant librarybase --listen 127.0.0.1:8080
    tools/api_check.py --ids Q1 Q100 P1
    tools/api_check.py --db "$URL" --sample 20                  # random adopted entities
    tools/api_check.py --db "$URL" --sample 20 --compare https://librarybase.org/
    tools/api_check.py --db "$URL" --ids Q1 --wbi-login "James bot@test" --wbi-secret "$SECRET" \
        [--wbi-property P5] [--wbi-create-property]       # test plan 1.4, needs wikibaseintegrator

What is checked, per entity:
  1. Special:EntityData/{id}.json serves it, and EVERY snak (mainsnak, qualifiers,
     references) carries `datatype` -- the bug of adoption-throughput.md section 2b.
  2. wbgetentities returns the same entity, with pageid and lastrevid.
  3. wbsearchentities finds it by its English (or first) label.
  4. With --db: view.identifier holds a row for each external-id mainsnak with a value.
  5. With --compare: the entity equals the source's Special:EntityData output, ignoring
     lastrevid/modified/pageid/ns/title (the test plan's spot check); pageid equals the
     source's; lastrevid is a fresh local ID above the source's (0035 section 3-4).
With --wbi-login/--wbi-secret (a subsidiary key, `name@label` and its secret), the
WikibaseIntegrator section of test plan 1.4 runs on the first --ids entity: log in as a
bot; set a label; add then remove a string statement; a stale baserevid is refused; create
an item (and with --wbi-create-property a property) whose number is above the counter
floor; every write is read back through wbgetentities. These writes are permanent on the
tenant, so point them at a test database. Needs `pip install wikibaseintegrator`.
Exit status is non-zero when any check fails. Standard library only otherwise.
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
    ap.add_argument("--wbi-login", metavar="NAME@LABEL", help="bot-password login name from `subsidiary key`")
    ap.add_argument("--wbi-secret", help="the key's secret (or set TRIPLESPACE_WBI_SECRET)")
    ap.add_argument("--wbi-property", help="a string-typed property to add a statement with (found automatically if omitted)")
    ap.add_argument("--wbi-create-property", action="store_true", help="also create a property (consumes a P number)")
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

    if a.wbi_login:
        failures += wbi_section(a, ids[0])

    print(f"\n{len(ids)} entities, {failures} failures")
    sys.exit(1 if failures else 0)


def sequence_value(db, tenant, kind):
    """`last_value` of a tenant's ID sequence (0013 section 6), or None without --db."""
    if not db:
        return None
    rows = psql(db, f"""SELECT last_value FROM log."{tenant}.{kind}" """)
    return int(rows[0]) if rows else None


def numeric(eid):
    return int("".join(ch for ch in eid if ch.isdigit()))


def wbi_section(a, eid):
    """Test plan 1.4: edits through WikibaseIntegrator, as a bot. Returns the failure count."""
    import os
    import time

    try:
        from wikibaseintegrator import WikibaseIntegrator, wbi_login
        from wikibaseintegrator.datatypes import String
        from wikibaseintegrator.wbi_config import config as wbi_config
        from wikibaseintegrator.wbi_enums import ActionIfExists
    except ImportError:
        print("FAIL wbi: wikibaseintegrator is not installed (pip install wikibaseintegrator)")
        return 1

    secret = a.wbi_secret or os.environ.get("TRIPLESPACE_WBI_SECRET")
    if not secret:
        print("FAIL wbi: no secret (--wbi-secret or TRIPLESPACE_WBI_SECRET)")
        return 1
    failures = 0

    def fail(what):
        nonlocal failures
        failures += 1
        print(f"FAIL wbi: {what}")

    api_url = f"{a.api.rstrip('/')}/w/api.php"
    wbi_config["MEDIAWIKI_API_URL"] = api_url
    wbi_config["USER_AGENT"] = "triplespace api_check (tools/api_check.py)"
    stamp = time.strftime("%Y%m%d-%H%M%S")

    # 1. Bot-password login, as the acceptance test does by hand.
    try:
        login = wbi_login.Login(user=a.wbi_login, password=secret, mediawiki_api_url=api_url)
    except Exception as e:  # noqa: BLE001
        fail(f"login as {a.wbi_login!r}: {e}")
        return failures
    wbi = WikibaseIntegrator(login=login)
    ui = api(a.api, action="query", meta="userinfo", uiprop="groups|rights")
    print(f"wbi: logged in; userinfo via plain GET is {ui['query']['userinfo'].get('name')!r} (anonymous, as expected)")

    # 2. Read the entity, set a label, write with baserevid.
    try:
        item = wbi.item.get(entity_id=eid)
    except Exception as e:  # noqa: BLE001
        fail(f"item.get({eid}): {e}")
        return failures
    before = item.lastrevid
    label = f"api_check {stamp}"
    item.labels.set(language="fr", value=label)
    try:
        item.write(summary="api_check: label")
    except Exception as e:  # noqa: BLE001
        fail(f"write label on {eid}: {e}")
        return failures
    got = api(a.api, action="wbgetentities", ids=eid)["entities"][eid]
    if got.get("labels", {}).get("fr", {}).get("value") != label:
        fail(f"{eid}: fr label not {label!r} after write")
    if not got.get("lastrevid", 0) > before:
        fail(f"{eid}: lastrevid did not advance ({before} -> {got.get('lastrevid')})")
    else:
        print(f"wbi: {eid} label set, lastrevid {before} -> {got['lastrevid']}")

    # 3. A stale baserevid must be refused (0006 section 8).
    session = login.get_session()
    token = login.get_edit_token()
    r = session.post(api_url, data={
        "action": "wbsetlabel", "format": "json", "id": eid, "language": "fr",
        "value": label + " stale", "baserevid": before, "token": token,
    }, timeout=60).json()
    if "error" not in r:
        fail(f"{eid}: wbsetlabel with stale baserevid {before} was accepted: {r}")
    else:
        print(f"wbi: stale baserevid refused ({r['error'].get('code')})")

    # 4. Add a string statement, then remove it.
    prop = a.wbi_property
    if not prop:
        for n in range(1, 200):
            try:
                p = api(a.api, action="wbgetentities", ids=f"P{n}", props="datatype")["entities"][f"P{n}"]
            except Exception:  # noqa: BLE001
                continue
            if p.get("datatype") == "string":
                prop = f"P{n}"
                break
    if not prop:
        fail("no string-typed property found in P1..P199; pass --wbi-property")
    else:
        item = wbi.item.get(entity_id=eid)
        item.claims.add(String(prop_nr=prop, value=f"api_check {stamp}"),
                        action_if_exists=ActionIfExists.APPEND_OR_REPLACE)
        try:
            item.write(summary="api_check: add statement")
        except Exception as e:  # noqa: BLE001
            fail(f"add {prop} statement on {eid}: {e}")
        item = wbi.item.get(entity_id=eid)
        mine = [c for c in item.claims.get(prop) if c.mainsnak.datavalue
                and c.mainsnak.datavalue.get("value") == f"api_check {stamp}"]
        if not mine:
            fail(f"{eid}: added {prop} statement not read back")
        else:
            print(f"wbi: {eid} statement {mine[0].id} added on {prop}")
            r = session.post(api_url, data={
                "action": "wbremoveclaims", "format": "json", "claim": mine[0].id,
                "baserevid": item.lastrevid, "token": login.get_edit_token(),
            }, timeout=60).json()
            if "error" in r:
                fail(f"{eid}: wbremoveclaims: {r['error']}")
            else:
                item = wbi.item.get(entity_id=eid)
                if any(c.id == mine[0].id for c in item.claims.get(prop)):
                    fail(f"{eid}: statement {mine[0].id} still present after removal")
                else:
                    print(f"wbi: {eid} statement removed")

    # 5. A new item lands above the item counter floor (0035 section 4).
    floor = sequence_value(a.db, a.tenant, "item_id")
    new = wbi.item.new()
    new.labels.set(language="en", value=f"api_check item {stamp}")
    new.descriptions.set(language="en", value="created by tools/api_check.py; safe to delete")
    try:
        new.write(summary="api_check: new item")
        n = numeric(new.id)
        if floor is not None and n <= floor:
            fail(f"new item {new.id} is not above the item floor {floor}")
        else:
            print(f"wbi: created {new.id}" + (f" (floor was {floor})" if floor is not None else ""))
        if "missing" in api(a.api, action="wbgetentities", ids=new.id)["entities"][new.id]:
            fail(f"new item {new.id} not readable back")
    except Exception as e:  # noqa: BLE001
        fail(f"create item: {e}")

    # 6. Optionally a property, likewise above its floor.
    if a.wbi_create_property:
        floor = sequence_value(a.db, a.tenant, "property_id")
        newp = wbi.property.new(datatype="string")
        newp.labels.set(language="en", value=f"api_check property {stamp}")
        try:
            newp.write(summary="api_check: new property")
            n = numeric(newp.id)
            if floor is not None and n <= floor:
                fail(f"new property {newp.id} is not above the property floor {floor}")
            else:
                print(f"wbi: created {newp.id}" + (f" (floor was {floor})" if floor is not None else ""))
        except Exception as e:  # noqa: BLE001
            fail(f"create property: {e}")

    return failures


if __name__ == "__main__":
    main()
