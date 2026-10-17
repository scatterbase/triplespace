#!/usr/bin/env python3
"""Generate the settings catalogue of chapter 23 §3.2–3.3 from docs/registry/settings.toml (0085 §1).

The registry is the catalogue of record; the two tables between the `settings.py` markers in
23-configuration-and-registry.md are rendered from it, so the chapter and the file cannot drift.

Usage, from docs/:
  python3 architecture/settings.py          check: fail if a table is stale or an entry is malformed
  python3 architecture/settings.py --write  rewrite the tables
"""
import os, re, sys

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib

ARGS = [a for a in sys.argv[1:] if not a.startswith("--")]
ROOT = ARGS[0] if ARGS else "."
WRITE = "--write" in sys.argv[1:]
REG = os.path.join(ROOT, "registry", "settings.toml")
CHAPTER = os.path.join(ROOT, "architecture", "23-configuration-and-registry.md")

TYPES = {"boolean", "integer", "duration", "size", "text", "string", "url", "language", "language-list", "choice",
         "string-list", "integer-list", "map", "json", "page", "page-list", "file", "group", "actor", "repository",
         "repository-list", "property-list", "namespace-list", "site", "licence", "collation", "pattern", "schedule",
         "site-asset"}
FIELDS = {"key", "scope", "bound", "ceiling", "tenant_may_narrow", "group", "type", "choices", "values", "default",
          "default_text", "no_default", "media", "by_rule", "scope_note", "right", "reported", "meaning", "source", "retired"}
CANON_VALUES = {"boolean": "off, on", "integer": "integer", "duration": "duration", "size": "size", "text": "text",
                "url": "URL", "language": "a language", "language-list": "language list", "file": "a `File:` title"}

LISTS = {"language-list", "string-list", "integer-list", "page-list", "repository-list", "property-list", "namespace-list"}

def default_fits(e):
    d, ty = e["default"], e.get("type")
    if ty == "boolean": return isinstance(d, bool)
    if ty in ("integer", "size"): return isinstance(d, int) and not isinstance(d, bool)
    if ty == "duration": return isinstance(d, str) and d.startswith("P")
    if ty in LISTS: return isinstance(d, list)
    if ty == "map": return isinstance(d, dict)
    if ty == "json": return True
    return isinstance(d, str)

def problems(reg):
    out, seen = [], set()
    groups = {g["id"] for g in reg.get("group", [])}
    for e in reg.get("setting", []):
        k = e.get("key", "?")
        if k in seen: out.append(f"{k}: duplicate key")
        seen.add(k)
        if set(e) - FIELDS: out.append(f"{k}: unknown fields {sorted(set(e) - FIELDS)}")
        for f in ("scope", "bound", "group", "type", "meaning", "source"):
            if f not in e: out.append(f"{k}: missing `{f}`")
        if e.get("scope") not in ("tenant", "instance"): out.append(f"{k}: scope must be tenant or instance")
        if e.get("bound") not in ("tenant", "ceiling", "instance"): out.append(f"{k}: bad bound")
        if e.get("scope") == "instance" and e.get("bound") != "instance": out.append(f"{k}: an instance setting is instance-bound")
        if e.get("group") not in groups: out.append(f"{k}: unknown group {e.get('group')!r}")
        if e.get("type") not in TYPES: out.append(f"{k}: unknown type {e.get('type')!r}")
        if e.get("type") == "choice" and not e.get("choices"): out.append(f"{k}: a choice needs `choices`")
        if (e.get("type") == "site-asset") != bool(e.get("media")): out.append(f"{k}: `media` goes with type site-asset, and only there")
        if "default" in e and e.get("type") == "choice" and e["default"] not in e.get("choices", []):
            out.append(f"{k}: default is not one of the choices")
        if ("default" in e) == bool(e.get("no_default")):
            out.append(f"{k}: needs exactly one of `default` and `no_default = true`")
        if "default" in e and not default_fits(e):
            out.append(f"{k}: default {e['default']!r} does not fit type {e.get('type')}")
        if "ceiling" in e and e.get("bound") != "ceiling": out.append(f"{k}: `ceiling` on a setting not bound by one")
        if "right" in e and not isinstance(e["right"], str): out.append(f"{k}: `right` must be a string")
        if "values" not in e and e.get("type") not in CANON_VALUES and e.get("type") != "choice":
            out.append(f"{k}: type {e.get('type')} needs a `values` text")
    return out

def values(e):
    if "values" in e: return e["values"]
    if e["type"] == "choice": return ", ".join(f"`{c}`" for c in e["choices"])
    return CANON_VALUES[e["type"]]

def default(e):
    if "default_text" in e: return e["default_text"]
    if "default" not in e: return "—"
    d = e["default"]
    if isinstance(d, bool): return "on" if d else "off"
    if isinstance(d, int): return f"{d:,}"
    return f"`{d}`"

def bound(e):
    if e["bound"] == "ceiling":
        return "tenant ≤ instance ceiling" + (f" (`{e['ceiling']}`)" if "ceiling" in e else "")
    return e["bound"]

def scope(e):
    if "scope_note" in e: return e["scope_note"]
    return "`site` (by rule, §3.1)" if e.get("by_rule") else "`site`"

def row(cells):
    return "| " + " | ".join(cells) + " |"

def render(reg, which):
    rows = sorted((e for e in reg["setting"] if e["scope"] == which and not e.get("retired")), key=lambda e: e["key"])
    if which == "tenant":
        head = ["| Setting | Scope | Bound | Values | Default | Meaning | Source |", "|---|---|---|---|---|---|---|"]
        body = [row([f"`{e['key']}`", scope(e), bound(e), values(e), default(e), e["meaning"], e["source"]]) for e in rows]
    else:
        head = ["| Setting | Values | Default | Meaning | Source |", "|---|---|---|---|---|"]
        body = [row([f"`{e['key']}`", values(e), default(e), e["meaning"], e["source"]]) for e in rows]
    return "\n".join(head + body)

def main():
    reg = tomllib.load(open(REG, "rb"))
    errs = problems(reg)
    if errs:
        print("\n".join("   - " + e for e in errs))
        sys.exit(1)
    text = open(CHAPTER, encoding="utf-8").read()
    new = text
    for which in ("tenant", "instance"):
        pat = re.compile(r"(<!-- settings\.py: %s -->\n)(.*?)(\n<!-- /settings\.py -->)" % which, re.S)
        if not pat.search(new):
            errs.append(f"chapter 23: no `settings.py: {which}` markers")
            continue
        new = pat.sub(lambda m: m.group(1) + render(reg, which) + m.group(3), new)
    if errs:
        print("\n".join("   - " + e for e in errs))
        sys.exit(1)
    if new != text:
        if WRITE:
            open(CHAPTER, "w", encoding="utf-8").write(new)
            print("rewrote the settings tables in chapter 23")
        else:
            print("   - chapter 23 settings tables are stale: run `python3 architecture/settings.py --write` from docs/")
            sys.exit(1)
    else:
        print(f"ok: {len(reg['setting'])} settings")

main()
