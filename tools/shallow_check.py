#!/usr/bin/env python3
"""Shallow mirroring against live Wikidata (0070; SCA-174), against a server
`shallow_check.sh` sets up: a Librarybase tenant links ten Wikidata items, sees their
labels at once from the label cache, has them fetched by the Wikidata follower with their
statements and their closure, and a second pass fetches nothing outside the set. The
follower is the server's own, the `follow` job `instance provider add` queued; each step
waits for a pass of it to end.

Each step is a check; any failure exits 1. Editing an item upstream and seeing the follower
bring the edit in is not automated, since it needs an account on Wikidata: the script says
so rather than pass it.
"""

import argparse
import json
import os
import subprocess
import sys
import time

import requests

DEFAULT_ITEMS = "Q42,Q1339,Q7186,Q937,Q5879,Q8023,Q1035,Q307,Q762,Q868"

failures = []


def check(name, ok, detail=""):
    print(f"{'ok  ' if ok else 'FAIL'} {name}{': ' + detail if detail and not ok else ''}")
    if not ok:
        failures.append(name)


def sql(db, query):
    out = subprocess.run(["psql", db, "-Atc", query], check=True, capture_output=True, text=True)
    return [line for line in out.stdout.splitlines() if line]


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--api", required=True, help="the server's base URL")
    ap.add_argument("--db", required=True, help="the instance's Postgres URL")
    ap.add_argument("--bin", required=True, help="where the `triplespace` binary is")
    ap.add_argument("--key-file", required=True)
    ap.add_argument("--login", required=True, help="a bot's login (`Name@label`)")
    ap.add_argument("--secret", default=os.environ.get("TRIPLESPACE_WBI_SECRET"))
    ap.add_argument("--item", required=True, help="the local item to link from")
    ap.add_argument("--items", default=DEFAULT_ITEMS, help="the Wikidata items to link")
    args = ap.parse_args()
    wd = args.items.split(",")
    stored = ["WD" + q for q in wd]
    api = f"{args.api}/w/api.php"
    rest = f"{args.api}/w/rest.php/triplespace/v0"
    s = requests.Session()

    def action(method="GET", **params):
        params.update(format="json", formatversion="2")
        r = s.post(api, data=params) if method == "POST" else s.get(api, params=params)
        r.raise_for_status()
        return r.json()

    token = action(action="query", meta="tokens", type="login")["query"]["tokens"]["logintoken"]
    login = action("POST", action="login", lgname=args.login, lgpassword=args.secret, lgtoken=token)
    check("the bot logs in", login.get("login", {}).get("result") == "Success", json.dumps(login))
    csrf = action(action="query", meta="tokens")["query"]["tokens"]["csrftoken"]

    # A property whose values are items, to link with.
    made = action(
        "POST", action="wbeditentity", new="property", token=csrf, bot="1",
        data=json.dumps({"datatype": "wikibase-item",
                         "labels": {"en": {"language": "en", "value": "on Wikidata"}}}),
    )
    prop = made.get("entity", {}).get("id")
    check("a property of type item is created", bool(prop), json.dumps(made))
    if not prop:
        return

    # 1. Labels at once, from the label cache: nothing is mirrored yet.
    # `mul` as well as `en`: Wikidata keeps many names under `mul` alone (Q42's among them).
    labels = s.get(f"{rest}/labels", params={"ids": "|".join(stored), "language": "en|mul"}).json()
    named = [i for i in stored if i in labels.get("labels", {})]
    check("labels of unmirrored items come from the label cache", len(named) == len(stored),
          f"{len(named)} of {len(stored)}: {json.dumps(labels)[:400]}")

    # 2. Linking queues each item for the follower (0070 A10).
    for wdid in stored:
        r = action(
            "POST", action="wbcreateclaim", entity=args.item, property=prop, snaktype="value",
            value=json.dumps({"entity-type": "item", "id": wdid}), token=csrf, bot="1",
        )
        check(f"{args.item} links {wdid}", "claim" in r, json.dumps(r)[:300])
    queued = sql(args.db, "SELECT entity_id FROM ops.entity_fetch WHERE tenant = '' AND provider = 'wikidata'")
    check("the linked items are queued", set(wd) <= set(queued), f"queued: {queued}")

    # 3. A pass of the server's follower fetches them, with their statements, and their
    # closure. The pass ends when the follow row is put back: unclaimed, with a new
    # `not_before`.
    def follower():
        rows = sql(args.db, "SELECT coalesce(claimed_by, '') || '|' || coalesce(not_before::text, '') FROM ops.job_queue"
                            " WHERE tenant = '' AND kind = 'follow' AND params->>'provider' = 'wikidata'")
        return rows[0] if rows else None

    def follow_once(timeout=600):
        """Waits for the next pass of the server's follower to end; whether one did."""
        before = follower()
        if before is None:
            return False
        deadline = time.time() + timeout
        while time.time() < deadline:
            time.sleep(2)
            now = follower()
            if now is None:
                return False
            if now != before and now.startswith("|"):
                return True
        return False

    # A pass may be under way as the links are made; the first whole pass after them is
    # the one that fetches.
    follow_once()
    ran = follow_once()
    check("the server's follower runs", ran, "no pass ended; see the server log")
    got = action(action="wbgetentities", ids="|".join(stored))["entities"]
    held = [i for i in stored if "missing" not in got.get(i, {"missing": ""})]
    check("the linked items are mirrored", len(held) == len(stored), f"{held}")
    q42 = got.get(stored[0], {})
    check(f"{stored[0]} has its statements", bool(q42.get("claims")), json.dumps(q42)[:300])
    mirror = s.get(f"{rest}/provider/WD/mirror").json()
    check("the queue is empty", mirror.get("queued") == 0, json.dumps(mirror)[:400])
    check("the mirror route counts the items",
          mirror.get("types", {}).get("item", {}).get("size", 0) >= len(stored), json.dumps(mirror.get("types")))
    instance_of = [
        st["mainsnak"]["datavalue"]["value"]["id"]
        for st in q42.get("claims", {}).get("WDP31", [])
        if st["mainsnak"].get("datavalue")
    ]
    if instance_of:
        closure = action(action="wbgetentities", ids="|".join(instance_of))["entities"]
        check(f"{stored[0]}'s instance-of values are mirrored (closure)",
              all("missing" not in closure.get(i, {"missing": ""}) for i in instance_of), f"{instance_of}")
    else:
        check(f"{stored[0]} has an instance-of statement to walk", False)

    # 4. A second pass fetches nothing outside the set: the set does not grow by itself.
    before = sql(args.db, "SELECT count(*) FROM view.entity_source WHERE graph = 'mirror/wikidata'")
    ran = follow_once()
    after = sql(args.db, "SELECT count(*) FROM view.entity_source WHERE graph = 'mirror/wikidata'")
    check("a second pass keeps to the set", ran and before == after, f"{before} → {after}")

    print("skip an upstream edit brought in by the follower: editing Wikidata needs an account,"
          " so it is not automated")


if __name__ == "__main__":
    main()
    if failures:
        print(f"\n{len(failures)} check(s) failed: {', '.join(failures)}")
        sys.exit(1)
    print("\nall shallow-mirroring checks passed")
