#!/usr/bin/env python3
"""The catalogues that live in registry files and show in a chapter table (24 §11):

  registry/routes.toml       -> 18 §3.2  (Built column derived from api/triplespace-v0.openapi.json)
  registry/permissions.toml  -> 09 §2.2  ('Granted by default to' derived from registry/groups.toml)
  registry/log-events.toml   -> 16 §2.3

  python3 architecture/catalogues.py [DOCS_DIR]          check: files well formed, cross-checks hold, tables current
  python3 architecture/catalogues.py --write [DOCS_DIR]  rewrite the tables, then check

The tables sit between `<!-- catalogues.py: NAME -->` and `<!-- /catalogues.py -->` in the chapter
and are never edited by hand. DOCS_DIR defaults to the parent of this file's directory (docs/).
Exit 1 on any problem.
"""
import json
import os
import re
import sys

try:
    import tomllib
except ModuleNotFoundError:   # Python < 3.11: docs/check.sh puts the vendored tomli wheel on the path
    import tomli as tomllib

ARGS = [a for a in sys.argv[1:] if not a.startswith("--")]
WRITE = "--write" in sys.argv[1:]
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = ARGS[0] if ARGS else os.path.dirname(HERE)
REG = os.path.join(ROOT, "registry")
ARCH = os.path.join(ROOT, "architecture")


def load_toml(name):
    with open(os.path.join(REG, name), "rb") as fh:
        return tomllib.load(fh)


def cell(s):
    return (s or "").replace("|", "\\|").strip() or "—"


def codes(names):
    return ", ".join(f"`{n}`" for n in names)


# ---------------------------------------------------------------- routes

def openapi_ops():
    path = os.path.join(ROOT, "api", "triplespace-v0.openapi.json")
    if not os.path.exists(path):
        return set()
    doc = json.load(open(path, encoding="utf-8"))
    ops = set()
    for p, item in doc.get("paths", {}).items():
        for m in ("get", "post", "put", "patch", "delete", "head", "options"):
            if m in item:
                ops.add((m.upper(), norm_path(p)))
    return ops


def norm_path(p):
    p = p.split("?")[0].rstrip("/") or "/"
    return re.sub(r"\{[^}]*\}", "{}", p)


def parse_route(r):
    m = re.match(r"^(?:(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s+)?(.+)$", r.strip())
    if not m:
        return None, None
    return m.group(1), norm_path(m.group(2))


def routes(write_errs):
    data = load_toml("routes.toml")
    rows = data.get("route", [])
    ops = openapi_ops()
    covered = set()
    out = []
    for r in rows:
        names = r.get("routes") or []
        if not names:
            write_errs.append("routes.toml: an entry has no routes")
            continue
        built = False
        for n in names:
            method, path = parse_route(n)
            if path is None:
                write_errs.append(f"routes.toml: cannot read route {n!r}")
                continue
            for m, p in ops:
                if p == path and (method is None or method == m):
                    built = True
                    covered.add((m, p))
        if not r.get("sources"):
            write_errs.append(f"routes.toml: {names[0]} has no sources")
        out.append(f"| {cell(r.get('display') or codes(names))} | {cell(r.get('purpose'))} | {'✓' if built else ''} | {cell(r.get('sources'))} |")
    for m, p in sorted(ops - covered):
        write_errs.append(f"routes.toml: the OpenAPI contract serves {m} {p}, which no route entry names")
    return ["| Route | Purpose | Built | ADR |", "|---|---|---|---|"] + out, len(rows)


# ---------------------------------------------------------------- permissions

def permissions(write_errs):
    data = load_toml("permissions.toml")
    rows = data.get("permission", [])
    groups = load_toml("groups.toml").get("group", [])
    holders = {}
    for g in groups:
        perms = g.get("permissions", [])
        if perms == ["*"]:
            continue   # owner: every permission by definition (09 §3.1)
        for p in perms:
            holders.setdefault(p, []).append(g["name"])
    listed = set()
    out = []
    for r in rows:
        names = r.get("names") or []
        if not names:
            write_errs.append("permissions.toml: an entry has no names")
            continue
        listed |= set(names)
        gs = []
        for n in names:
            for g in holders.get(n, []):
                if g not in gs:
                    gs.append(g)
        if not gs and not r.get("ungranted"):
            write_errs.append(f"permissions.toml: no group in groups.toml grants {names[0]} (mark it `ungranted = true` if that is by design)")
        if not r.get("sources"):
            write_errs.append(f"permissions.toml: {names[0]} has no sources")
        first = r.get("display") or codes(names)
        if r.get("new"):
            first += " *(new)*"
        granted = codes(gs) + (r.get("granted_note") or "")
        out.append(f"| {cell(first)} | {cell(granted)} | {cell(r.get('allows'))} | {cell(r.get('sources'))} |")
    for p in sorted(set(holders) - listed):
        write_errs.append(f"permissions.toml: groups.toml grants `{p}`, which is not catalogued")
    return ["| Permission | Granted by default to | What it allows | Source |", "|---|---|---|---|"] + out, len(rows)


# ---------------------------------------------------------------- log events

def log_events(write_errs):
    data = load_toml("log-events.toml")
    rows = data.get("event", [])
    out = []
    seen = set()
    for r in rows:
        names = r.get("events") or []
        if not names:
            write_errs.append("log-events.toml: an entry has no events")
            continue
        for n in names:
            if n in seen:
                write_errs.append(f"log-events.toml: `{n}` is listed twice")
            seen.add(n)
        if not r.get("sources"):
            write_errs.append(f"log-events.toml: {names[0]} has no sources")
        out.append(f"| {cell(r.get('display') or codes(names))} | {cell(r.get('from'))} | {cell(r.get('parameters'))} | {cell(r.get('visible_to'))} | {cell(r.get('sources'))} |")
    return ["| Log type/action | Comes from | Typed parameters | Visible to | Source |", "|---|---|---|---|---|"] + out, len(rows)


CATALOGUES = [
    ("routes", "18-api.md", routes),
    ("permissions", "09-security-and-moderation.md", permissions),
    ("log-events", "16-logs-feeds-and-notifications.md", log_events),
]


def main():
    errs = []
    stale = []
    counts = []
    for name, chapter, render in CATALOGUES:
        lines, n = render(errs)
        counts.append(f"{name} {n}")
        begin, end = f"<!-- catalogues.py: {name} -->", "<!-- /catalogues.py -->"
        cpath = os.path.join(ARCH, chapter)
        text = open(cpath, encoding="utf-8").read()
        a = text.find(begin)
        b = text.find(end, a) if a >= 0 else -1
        if a < 0 or b < 0:
            errs.append(f"{chapter}: no `catalogues.py: {name}` markers")
            continue
        new = text[:a] + begin + "\n" + "\n".join(lines) + "\n" + end + text[b + len(end):]
        if new != text:
            if WRITE:
                open(cpath, "w", encoding="utf-8").write(new)
                print(f"wrote the {name} table in {chapter}")
            else:
                stale.append(f"{chapter}: the {name} table is stale: run `python3 architecture/catalogues.py --write` from docs/")
    for e in errs + stale:
        print("   -", e)
    if errs or stale:
        sys.exit(1)
    print("ok: " + ", ".join(counts) + " entries; tables current")


if __name__ == "__main__":
    main()
