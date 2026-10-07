#!/usr/bin/env python3
"""Exercise a Triplespace tenant with Pywikibot, the client most Wikibase operators use.

    pip install pywikibot
    tools/pwb_check.py --login "James bot@test" --secret "$SECRET" [--api http://127.0.0.1:8080] \\
        [--item Q1] [--property P3] [--item-floor 250000] [--no-write]

Test plan 1.4 (optional item): "point Pywikibot at the server and record where it breaks".
Every step runs on its own and reports PASS, FAIL (with the exception) or SKIP, so one
failure does not hide the next; the exit status is the number of failures. Pywikibot is
configured in-process (no user-config.py, no pwb wrapper): a WikibaseFamily is written
to a temporary directory and registered, the bot password goes into a temporary
password file, throttling is off and retries are few so a wrong answer fails fast. The
"user-config.py cannot be loaded" lines Pywikibot prints at import are expected: the
same settings are made in code right after. Pywikibot's on-disk API cache is kept in the
temporary directory too, so each run sees the server as it is now, not as it was.

What is exercised, roughly in the order Pywikibot itself needs it:
  site      siteinfo, version parsing, paraminfo, the data repository, concept base URI
  login     bot-password login (lgname `name@label`), userinfo, rights
  read      ItemPage.get(), labels, claims (with datatype), latest_revision_id
  property  PropertyPage.get() and its datatype
  search    site.search_entities by label
  write     editLabels, addClaim, removeClaims, a new item above the counter floor,
            and an edit against a stale baserevid, which must be refused
Writes are permanent on the tenant; --no-write skips them.
"""

import argparse
import os
import sys
import tempfile
import time
import traceback

os.environ.setdefault("PYWIKIBOT_NO_USER_CONFIG", "1")

FAMILY = """\
from pywikibot import family

class Family(family.WikibaseFamily):
    name = 'triplespace'
    langs = {{'{code}': '{host}'}}

    def protocol(self, code):
        return '{protocol}'

    def scriptpath(self, code):
        return '/w'

    def interface(self, code):
        return 'DataSite'
"""


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--api", default="http://127.0.0.1:8080", help="server base URL")
    ap.add_argument("--tenant", default="librarybase", help="family code (any slug)")
    ap.add_argument("--login", required=True, metavar="NAME@LABEL", help="bot-password login name from `subsidiary key`")
    ap.add_argument("--secret", help="the key's secret (or TRIPLESPACE_WBI_SECRET)")
    ap.add_argument("--item", default="Q1")
    ap.add_argument("--property", help="a string-typed property for the claim test (default: first found)")
    ap.add_argument("--item-floor", type=int, default=0, help="a new item's number must exceed this")
    ap.add_argument("--no-write", action="store_true")
    a = ap.parse_args()

    secret = a.secret or os.environ.get("TRIPLESPACE_WBI_SECRET")
    if not secret:
        sys.exit("no secret: --secret or TRIPLESPACE_WBI_SECRET")
    if "@" not in a.login:
        sys.exit("--login must be name@label (a bot password)")
    user, label = a.login.split("@", 1)

    from urllib.parse import urlparse
    u = urlparse(a.api)
    host = u.netloc
    protocol = u.scheme or "http"

    # Pywikibot configuration, in-process.
    tmp = tempfile.mkdtemp(prefix="pwb_check_")
    family_path = os.path.join(tmp, "triplespace_family.py")
    with open(family_path, "w", encoding="utf-8") as f:
        f.write(FAMILY.format(code=a.tenant, host=host, protocol=protocol))
    pw_path = os.path.join(tmp, "passwords.py")
    with open(pw_path, "w", encoding="utf-8") as f:
        f.write(f"({user!r}, BotPassword({label!r}, {secret!r}))\n")
    os.chmod(pw_path, 0o600)

    import pywikibot
    from pywikibot import config

    # Pywikibot finds `<name>_family.py` in registered folders.
    config.register_families_folder(tmp)
    # Pywikibot caches API answers on disk (paraminfo for 30 days, `meta=wikibase` for
    # 7) under base_dir, which would replay a previous run's answers against a changed
    # server. A fresh directory per run keeps every answer live.
    config.base_dir = tmp
    config.family = "triplespace"
    config.mylang = a.tenant
    config.usernames["triplespace"][a.tenant] = user
    config.password_file = pw_path
    config.put_throttle = 0
    config.max_retries = 1
    config.retry_wait = 1
    config.socket_timeout = 30
    config.user_agent_description = "tools/pwb_check.py"

    results = []

    def step(name, fn, skip=False):
        if skip:
            print(f"SKIP {name}")
            results.append((name, None))
            return None
        try:
            out = fn()
            print(f"PASS {name}" + (f": {out}" if out not in (None, True) else ""))
            results.append((name, True))
            return out if out is not True else None
        except Exception as e:  # noqa: BLE001
            tb = traceback.extract_tb(e.__traceback__)[-1]
            print(f"FAIL {name}: {type(e).__name__}: {e}  [{os.path.basename(tb.filename)}:{tb.lineno}]")
            results.append((name, False))
            return None

    state = {}
    stamp = time.strftime("%Y%m%d-%H%M%S")

    # --- site -------------------------------------------------------------------------
    def site_init():
        site = pywikibot.Site(a.tenant, "triplespace")
        state["site"] = site
        gen = site.siteinfo["general"]
        return f"{gen.get('sitename')!r}, generator {gen.get('generator')!r}, version {site.version()}"
    step("site: siteinfo and version", site_init)
    if "site" not in state:
        print("\ncannot continue without a site")
        sys.exit(1)
    site = state["site"]

    step("site: paraminfo (modules Pywikibot asks about)",
         lambda: f"{len(site._paraminfo.query_modules)} query modules known")  # noqa: SLF001
    step("site: data repository", lambda: (state.__setitem__("repo", site.data_repository()),
                                           f"{state['repo']}, concept base {state['repo'].concept_base_uri}")[1])
    repo = state.get("repo", site)
    step("site: namespaces", lambda: f"{len(site.namespaces)} namespaces; item ns {repo.item_namespace.id}, "
                                     f"property ns {repo.property_namespace.id}")

    # --- login ------------------------------------------------------------------------
    def login():
        site.login()
        if not site.logged_in():
            raise RuntimeError("site.logged_in() is False after login()")
        ui = site.userinfo
        return f"as {ui.get('name')!r}, groups {ui.get('groups')}, {len(ui.get('rights', []))} rights"
    step("login: bot password", login)
    logged_in = results[-1][1] is True
    step("login: has 'edit' right", lambda: "edit" in site.userinfo.get("rights", []) or
         (_ for _ in ()).throw(RuntimeError(f"rights: {site.userinfo.get('rights')}")), skip=not logged_in)

    # --- read -------------------------------------------------------------------------
    def read_item():
        item = pywikibot.ItemPage(repo, a.item)
        item.get()
        state["item"] = item
        n_claims = sum(len(v) for v in item.claims.values())
        untyped = [c for cs in item.claims.values() for c in cs if not getattr(c, "type", None)]
        if untyped:
            raise RuntimeError(f"{len(untyped)} claims without a datatype")
        return (f"{len(item.labels)} labels, {len(item.descriptions)} descriptions, {n_claims} claims, "
                f"revision {item.latest_revision_id}, pageid {item.pageid}")
    step(f"read: ItemPage({a.item}).get()", read_item)

    def find_property():
        if a.property:
            p = pywikibot.PropertyPage(repo, a.property)
            p.get()
            if p.type != "string":
                raise RuntimeError(f"{a.property} is {p.type!r}, not string")
            return a.property
        item = state.get("item")
        candidates = list(item.claims) if item else []
        candidates += [f"P{n}" for n in range(1, 60)]
        for pid in dict.fromkeys(candidates):
            try:
                p = pywikibot.PropertyPage(repo, pid)
                p.get()
            except Exception:  # noqa: BLE001
                continue
            if p.type == "string":
                return pid
        raise RuntimeError("no string-typed property found; pass --property")
    prop = step("read: PropertyPage.get() and a string-typed property", find_property)

    def search():
        hits = list(site.search_entities("Librarybase", "en", total=10))
        ids = [h["id"] for h in hits]
        return f"{ids}"
    step("search: search_entities('Librarybase', 'en')", search)

    # --- write ------------------------------------------------------------------------
    skip_w = a.no_write or not logged_in or "item" not in state
    item = state.get("item")

    def edit_label():
        text = f"pwb {stamp}"
        item.editLabels({"de": text}, summary="pwb_check: label")
        fresh = pywikibot.ItemPage(repo, a.item)
        fresh.get()
        if fresh.labels.get("de") != text:
            raise RuntimeError(f"de label is {fresh.labels.get('de')!r}")
        state["rev_after_label"] = fresh.latest_revision_id
        return f"revision {item.latest_revision_id} -> {fresh.latest_revision_id}"
    step("write: editLabels (de)", edit_label, skip=skip_w)

    def add_claim():
        claim = pywikibot.Claim(repo, prop)
        claim.setTarget(f"pwb {stamp}")
        item.addClaim(claim, summary="pwb_check: add claim")
        if not claim.snak:
            raise RuntimeError("claim has no GUID after addClaim")
        state["claim"] = claim
        fresh = pywikibot.ItemPage(repo, a.item)
        fresh.get()
        if not any(c.snak == claim.snak for c in fresh.claims.get(prop, [])):
            raise RuntimeError("claim not read back")
        return claim.snak
    step(f"write: addClaim on {prop}", add_claim, skip=skip_w or not prop)

    def remove_claim():
        item.removeClaims([state["claim"]], summary="pwb_check: remove claim")
        fresh = pywikibot.ItemPage(repo, a.item)
        fresh.get()
        if any(c.snak == state["claim"].snak for c in fresh.claims.get(prop, [])):
            raise RuntimeError("claim still present")
        return True
    step("write: removeClaims", remove_claim, skip=skip_w or "claim" not in state)

    def stale_baserevid():
        # Pywikibot passes baserevid from the entity it holds; hold an old one on purpose.
        stale = pywikibot.ItemPage(repo, a.item)
        stale.get()
        stale.latest_revision_id = state.get("rev_after_label", stale.latest_revision_id) - 1
        try:
            stale.editLabels({"de": f"pwb stale {stamp}"}, summary="pwb_check: must fail", baserevid=stale.latest_revision_id)
        except pywikibot.exceptions.APIError as e:
            if "conflict" in e.code or "conflict" in str(e).lower():
                return f"refused with {e.code}"
            raise
        raise RuntimeError("edit with a stale baserevid was accepted")
    step("write: stale baserevid is refused", stale_baserevid, skip=skip_w)

    def new_item():
        new = pywikibot.ItemPage(repo)
        new.editEntity({"labels": {"en": f"pwb item {stamp}"},
                        "descriptions": {"en": "created by tools/pwb_check.py; safe to delete"}},
                       summary="pwb_check: new item")
        n = int(new.getID()[1:])
        if n <= a.item_floor:
            raise RuntimeError(f"{new.getID()} is not above the floor {a.item_floor}")
        return f"{new.getID()}" + (f" (floor {a.item_floor})" if a.item_floor else "")
    step("write: new item above the counter floor", new_item, skip=skip_w)

    failures = sum(1 for _, ok in results if ok is False)
    passed = sum(1 for _, ok in results if ok is True)
    skipped = sum(1 for _, ok in results if ok is None)
    print(f"\n{passed} passed, {failures} failed, {skipped} skipped  (pywikibot {pywikibot.__version__})")
    sys.exit(failures)


if __name__ == "__main__":
    main()
