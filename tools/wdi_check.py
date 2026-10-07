#!/usr/bin/env python3
"""Exercise a Triplespace tenant with WikidataIntegrator (WDI), WBI's older sibling.

    pip install wikidataintegrator          # pulls pandas, pyshex, shexer; a venv is kind
    tools/wdi_check.py --login "James bot@test" --secret "$SECRET" [--api http://127.0.0.1:8080] \\
        [--item Q1] [--property P3] [--item-floor 250000] [--no-write]

WDI (SuLab, 0.9.x) is still in legacy bot workflows. It differs from WBI and Pywikibot
in ways this script records:
  - `WDItemEngine.write()` sends no baserevid at all: WDI relies on maxlag, not on edit
    conflicts, so a WDI bot can overwrite a concurrent edit. Only `delete_statement()`
    passes a revision, so the stale-baserevid check (0006 section 8) runs through it.
  - The constructor always queries a SPARQL endpoint (MappingRelationHelper, and unless
    `core_props` is given, distinct-value constraints), defaulting to query.wikidata.org.
    Triplespace has no SPARQL endpoint yet (ADR 0059 is proposed), and WDI retries a
    failed SPARQL request without limit, so against a bare Triplespace a WDItemEngine
    never finishes constructing unless `BACKOFF_MAX_TRIES` is set. This script sets it
    to 2; a WDI user must do the same (or point `sparql_endpoint_url` somewhere that
    answers) until 0059 lands. The clean steps also pass `core_props=set()`; one step
    uses WDI's default discovery against `<api>/sparql` and records what happens.
Every step runs on its own and reports PASS, FAIL (with the exception), INFO or SKIP;
the exit status is the number of failures. Writes are permanent; --no-write skips them.
"""

import argparse
import json
import os
import sys
import time
import traceback
import urllib.parse
import urllib.request


def api_get(base, **params):
    params.setdefault("format", "json")
    url = f"{base.rstrip('/')}/w/api.php?" + urllib.parse.urlencode(params)
    with urllib.request.urlopen(url, timeout=60) as r:
        return json.load(r)


def entity(base, eid):
    return api_get(base, action="wbgetentities", ids=eid)["entities"][eid]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--api", default="http://127.0.0.1:8080")
    ap.add_argument("--login", required=True, metavar="NAME@LABEL")
    ap.add_argument("--secret", help="or TRIPLESPACE_WBI_SECRET")
    ap.add_argument("--item", default="Q1")
    ap.add_argument("--property", help="a string-typed property (default: first found)")
    ap.add_argument("--item-floor", type=int, default=0)
    ap.add_argument("--no-write", action="store_true")
    a = ap.parse_args()

    secret = a.secret or os.environ.get("TRIPLESPACE_WBI_SECRET")
    if not secret:
        sys.exit("no secret: --secret or TRIPLESPACE_WBI_SECRET")
    api_url = f"{a.api.rstrip('/')}/w/api.php"
    sparql_url = f"{a.api.rstrip('/')}/sparql"

    try:
        from wikidataintegrator import wdi_core, wdi_login
        from wikidataintegrator.wdi_config import config as wdi_config
    except ImportError as e:
        # WDI imports pandas, pyshex, shexer and the antlr runtime at import time; any one
        # of them failing surfaces here as an ImportError, so show which.
        import importlib.util
        if importlib.util.find_spec("wikidataintegrator") is None:
            sys.exit(f"wikidataintegrator is not installed in {sys.executable} (pip install wikidataintegrator)")
        sys.exit(f"wikidataintegrator is installed in {sys.executable} but failed to import: "
                 f"{type(e).__name__}: {e}\n"
                 "(a dependency of WDI, not WDI itself; `pip install -U pyshex shexer antlr4-python3-runtime` "
                 "or check the pandas/numpy pair)")

    # WDI retries failed SPARQL requests without limit (BACKOFF_MAX_TRIES = None, waits up
    # to an hour). With no SPARQL endpoint behind <api>/sparql that is a hang, not a
    # failure: the constructor's own try/except around MappingRelationHelper never gets
    # to run. Bounded, the HTTPError propagates and WDI continues without the helper.
    wdi_config["BACKOFF_MAX_TRIES"] = 2
    wdi_config["BACKOFF_MAX_VALUE"] = 1

    results = []

    def step(name, fn, skip=False, info=False):
        if skip:
            print(f"SKIP {name}")
            results.append((name, None))
            return None
        try:
            out = fn()
            tag = "INFO" if info else "PASS"
            print(f"{tag} {name}" + (f": {out}" if out not in (None, True) else ""))
            results.append((name, True))
            return out if out is not True else None
        except Exception as e:  # noqa: BLE001
            if info:
                print(f"INFO {name}: {type(e).__name__}: {str(e).strip().splitlines()[0][:200]}")
                results.append((name, True))
                return None
            tb = traceback.extract_tb(e.__traceback__)[-1]
            print(f"FAIL {name}: {type(e).__name__}: {str(e).strip().splitlines()[0][:200]}"
                  f"  [{os.path.basename(tb.filename)}:{tb.lineno}]")
            results.append((name, False))
            return None

    state = {}
    stamp = time.strftime("%Y%m%d-%H%M%S")

    def engine(**kw):
        kw.setdefault("mediawiki_api_url", api_url)
        kw.setdefault("sparql_endpoint_url", sparql_url)
        kw.setdefault("core_props", set())
        return wdi_core.WDItemEngine(**kw)

    # --- login ------------------------------------------------------------------------
    def login():
        lg = wdi_login.WDLogin(user=a.login, pwd=secret, mediawiki_api_url=api_url)
        state["login"] = lg
        ui = api_get(a.api, action="query", meta="userinfo")["query"]["userinfo"]
        return f"edit token obtained; plain GET userinfo is {ui.get('name')!r} (anonymous, as expected)"
    step("login: WDLogin with a bot password", login)
    logged_in = "login" in state

    # --- read -------------------------------------------------------------------------
    def read():
        item = engine(wd_item_id=a.item)
        state["item"] = item
        claims = item.get_wd_json_representation().get("claims", {})
        n = sum(len(v) for v in claims.values())
        untyped = [c for cs in claims.values() for c in cs if "datatype" not in c.get("mainsnak", {})]
        if untyped:
            raise RuntimeError(f"{len(untyped)} mainsnaks without datatype")
        return f"label {item.get_label('en')!r}, {n} claims"
    step(f"read: WDItemEngine({a.item})", read)

    def sparql_default():
        # What a user who sets only mediawiki_api_url (and points SPARQL here) gets.
        wdi_core.WDItemEngine.DISTINCT_VALUE_PROPS.pop(sparql_url, None)
        engine(wd_item_id=a.item, core_props=None)
        return ("constructed; WDI asked <api>/sparql for distinct-value constraints and the mapping "
                "relation helper, got no endpoint, and continued (see its warnings above)")
    step("sparql: WDI's default discovery against <api>/sparql (no endpoint yet, 0059; bounded retries)",
         sparql_default, info=True)

    def search():
        # The item's own English label (or any label it has): the hit must include it.
        labels = state["item"].get_wd_json_representation().get("labels", {})
        lang = "en" if "en" in labels else next(iter(labels))
        text = labels[lang]["value"]
        ids = wdi_core.WDItemEngine.get_wd_search_results(text, mediawiki_api_url=api_url,
                                                         language=lang, max_results=10)
        if a.item not in ids:
            raise RuntimeError(f"{a.item} not in {ids} for {text!r} ({lang})")
        return f"{text!r} ({lang}) -> {ids}"
    step("search: get_wd_search_results by the item's label", search, skip="item" not in state)

    def find_property():
        if a.property:
            return a.property
        claims = state["item"].get_wd_json_representation().get("claims", {}) if "item" in state else {}
        for pid in list(claims) + [f"P{n}" for n in range(1, 60)]:
            try:
                p = entity(a.api, pid)
            except Exception:  # noqa: BLE001
                continue
            if p.get("datatype") == "string":
                return pid
        raise RuntimeError("no string-typed property found; pass --property")
    prop = step("read: a string-typed property", find_property)

    # --- write ------------------------------------------------------------------------
    skip_w = a.no_write or not logged_in or "item" not in state
    lg = state.get("login")

    def write_label():
        item = engine(wd_item_id=a.item)
        before = entity(a.api, a.item)["lastrevid"]
        item.set_label(f"wdi {stamp}", lang="nl")
        item.write(lg, edit_summary="wdi_check: label")
        got = entity(a.api, a.item)
        if got["labels"].get("nl", {}).get("value") != f"wdi {stamp}":
            raise RuntimeError("nl label not read back")
        if not got["lastrevid"] > before:
            raise RuntimeError(f"lastrevid did not advance ({before} -> {got['lastrevid']})")
        state["rev_after_label"] = got["lastrevid"]
        return f"revision {before} -> {got['lastrevid']} (WDI reports lastrevid {item.lastrevid})"
    step("write: set_label + write()", write_label, skip=skip_w)

    def add_statement():
        item = engine(wd_item_id=a.item, data=[wdi_core.WDString(value=f"wdi {stamp}", prop_nr=prop)],
                      append_value=[prop])
        item.write(lg, edit_summary="wdi_check: add statement")
        got = entity(a.api, a.item)
        mine = [c for c in got["claims"].get(prop, [])
                if c["mainsnak"].get("datavalue", {}).get("value") == f"wdi {stamp}"]
        if not mine:
            raise RuntimeError("statement not read back")
        state["claim"] = mine[0]["id"]
        state["rev_after_claim"] = got["lastrevid"]
        return mine[0]["id"]
    step(f"write: WDString on {prop} + write() (append_value)", add_statement, skip=skip_w or not prop)

    step("info: write() carries no baserevid — WDI has no edit-conflict protection on wbeditentity",
         lambda: "by design in WDI; a concurrent edit between read and write would be overwritten",
         skip=skip_w, info=True)

    def stale_delete():
        stale = state["rev_after_label"]  # older than the revision the claim was added in
        # WDI prints an API error body and returns None rather than raising, so the
        # verdict is read from the entity: the statement must still be there.
        try:
            wdi_core.WDItemEngine.delete_statement(state["claim"], stale, lg, mediawiki_api_url=api_url)
        except Exception as e:  # noqa: BLE001
            if "conflict" not in str(e).lower():
                raise
        got = entity(a.api, a.item)
        if any(c["id"] == state["claim"] for c in got["claims"].get(prop, [])):
            return f"refused: statement still present (WDI printed the editconflict body above)"
        raise RuntimeError(f"stale baserevid {stale} was accepted: statement removed")
    step("write: delete_statement with a stale baserevid is refused", stale_delete,
         skip=skip_w or "claim" not in state)

    def delete_current():
        rev = entity(a.api, a.item)["lastrevid"]
        wdi_core.WDItemEngine.delete_statement(state["claim"], rev, lg, mediawiki_api_url=api_url)
        got = entity(a.api, a.item)
        if any(c["id"] == state["claim"] for c in got["claims"].get(prop, [])):
            raise RuntimeError("statement still present")
        return True
    step("write: delete_statement with the current revision", delete_current,
         skip=skip_w or "claim" not in state)

    def new_item():
        item = engine(new_item=True, data=[wdi_core.WDString(value=f"wdi {stamp}", prop_nr=prop)] if prop else [])
        item.set_label(f"wdi item {stamp}", lang="en")
        item.set_description("created by tools/wdi_check.py; safe to delete", lang="en")
        item.write(lg, edit_summary="wdi_check: new item")
        n = int(item.wd_item_id[1:])
        if n <= a.item_floor:
            raise RuntimeError(f"{item.wd_item_id} is not above the floor {a.item_floor}")
        return f"{item.wd_item_id}" + (f" (floor {a.item_floor})" if a.item_floor else "")
    step("write: new item above the counter floor", new_item, skip=skip_w)

    failures = sum(1 for _, ok in results if ok is False)
    passed = sum(1 for _, ok in results if ok is True)
    skipped = sum(1 for _, ok in results if ok is None)
    try:
        from importlib.metadata import version
        v = version("wikidataintegrator")
    except Exception:  # noqa: BLE001
        v = "?"
    print(f"\n{passed} passed/info, {failures} failed, {skipped} skipped  (wikidataintegrator {v})")
    sys.exit(failures)


if __name__ == "__main__":
    main()
