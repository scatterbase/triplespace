#!/usr/bin/env python3
"""The crate map's generated views (24 §11): registry/crates.toml is the catalogue of record;
the table between the `crates.py` markers in chapter 22 §2.1 and architecture/CRATES.md are
generated from it and never edited by hand. Beside the design, both show what the code says
(24 §15): the table's Built column is whether each crate is in the workspace, and each entry of
CRATES.md ends with an As built paragraph, the first paragraph of the crate's own `//!` docs and
the sections its src claims to implement (`tools/coverage.py --crates`).

  python3 architecture/crates.py [DOCS_DIR]          check: the file is well formed and both views are current
  python3 architecture/crates.py --write [DOCS_DIR]  rewrite both views, then check

DOCS_DIR defaults to the parent of this file's directory (docs/). Exit 1 on any problem.
"""
import json
import os
import re
import subprocess
import sys

try:
    import tomllib
except ModuleNotFoundError:   # Python < 3.11: docs/check.sh puts the vendored tomli wheel on the path
    import tomli as tomllib

ARGS = [a for a in sys.argv[1:] if not a.startswith("--")]
WRITE = "--write" in sys.argv[1:]
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = ARGS[0] if ARGS else os.path.dirname(HERE)
TOML = os.path.join(ROOT, "registry", "crates.toml")
CHAPTER = os.path.join(ROOT, "architecture", "22-crates-and-stack.md")
FULL = os.path.join(ROOT, "architecture", "CRATES.md")
LAYERS = ["substrate", "wikibase", "ingest", "triplespace"]
LAYER_TITLE = {"substrate": "Substrate", "wikibase": "Wikibase", "ingest": "Ingest", "triplespace": "Triplespace"}
BEGIN, END = "<!-- crates.py: table -->", "<!-- /crates.py -->"
WORKSPACE = os.path.join(os.path.dirname(os.path.abspath(ROOT)), "crates")
COVERAGE = os.path.join(os.path.dirname(os.path.abspath(ROOT)), "tools", "coverage.py")


def built(name):
    return os.path.exists(os.path.join(WORKSPACE, name, "Cargo.toml"))


def built_md(r):
    have = sum(1 for n in r["names"] if built(n))
    if have == len(r["names"]):
        return "yes"
    return f"{have} of {len(r['names'])}" if have else ""


def crate_doc(name):
    """The first paragraph of the crate's `//!` docs, as Markdown that reads outside rustdoc."""
    for f in ("lib.rs", "main.rs"):
        path = os.path.join(WORKSPACE, name, "src", f)
        if os.path.exists(path):
            break
    else:
        return ""
    para = []
    for line in open(path, encoding="utf-8"):
        if not line.startswith("//!"):
            if para or not line.strip() or line.startswith(("#!", "//")):
                if para:
                    break
                continue
            break
        body = line[3:].strip()
        if not body:
            if para:
                break
            continue
        para.append(body)
    text = " ".join(para)
    text = re.sub(r"\[`((?:mod|fn|struct|enum|trait|type|macro|const|static)@)?([^`\]]+)`\](?!\()", r"`\2`", text)
    text = re.sub(r"\[([^\]]+)\]\((?!https?:)[^)]*\)", r"\1", text)
    text = re.sub(r"\[([^\]]+)\]\[[^\]]*\]", r"\1", text)
    return text


_claims = {}
def claims():
    if "c" not in _claims:
        out = subprocess.run([sys.executable, COVERAGE, "--crates"], capture_output=True, text=True)
        if out.returncode != 0:
            print("   - tools/coverage.py --crates failed:", out.stderr.strip())
            sys.exit(1)
        _claims["c"] = json.loads(out.stdout)
    return _claims["c"]


def as_built(name):
    if not built(name):
        return f"**As built** (`{name}`): not a crate in the workspace yet."
    doc = crate_doc(name) or "no `//!` docs."
    cl = claims().get(name, [])
    said = "Claims " + ", ".join(cl) + "." if cl else "Claims no section in full yet."
    return f"**As built** (`{name}`, from its `//!` docs and `implements` claims): {doc} {said}"


def load():
    with open(TOML, "rb") as fh:
        data = tomllib.load(fh)
    rows = data.get("crate", [])
    errs = []
    seen = set()
    known = {n for r in rows for n in r.get("names", [])}
    for r in rows:
        names = r.get("names") or []
        if not names:
            errs.append("a [[crate]] entry has no names")
            continue
        for n in names:
            if n in seen:
                errs.append(f"`{n}` is listed twice")
            seen.add(n)
        for key in ("layer", "summary", "contents"):
            if not r.get(key):
                errs.append(f"`{names[0]}` lacks {key}")
        if r.get("layer") not in LAYERS:
            errs.append(f"`{names[0]}`: layer {r.get('layer')!r} is not one of {LAYERS}")
        if r.get("depends_any") not in (None, "lower", "core"):
            errs.append(f"`{names[0]}`: depends_any {r.get('depends_any')!r} is not lower or core")
        for d in r.get("depends", []):
            if re.match(r"(scatter|triplespace)-", d) and d not in known:
                errs.append(f"`{names[0]}` depends on `{d}`, which is not in the map")
            if d in known and LAYERS.index(layer_of(rows, d)) > LAYERS.index(r.get("layer", "triplespace")):
                errs.append(f"`{names[0]}` ({r['layer']}) depends on `{d}` ({layer_of(rows, d)}): layers point only downward")
    for r in data.get("retired", []):
        if not r.get("name") or not r.get("into"):
            errs.append("a [[retired]] entry lacks name or into")
        elif r["into"] not in known:
            errs.append(f"retired `{r['name']}`: into `{r['into']}` is not in the map")
        elif r["name"] in known:
            errs.append(f"retired `{r['name']}` is still in the map")
    order = [r.get("layer") for r in rows]
    if order != sorted(order, key=lambda l: LAYERS.index(l) if l in LAYERS else 9):
        errs.append("entries are not in layer order (substrate, wikibase, ingest, triplespace)")
    return rows, errs


def layer_of(rows, name):
    for r in rows:
        if name in r.get("names", []):
            return r.get("layer", "triplespace")
    return "triplespace"


def names_md(r):
    return ", ".join(f"`{n}`" for n in r["names"])


def depends_md(r):
    parts = [f"`{d}`" for d in r.get("depends", [])]
    if r.get("depends_any") == "lower":
        parts.append("any crate in the same or a lower layer")
    elif r.get("depends_any") == "core":
        parts.append("any substrate or Wikibase crate")
    if r.get("depends_note"):
        parts.append(r["depends_note"])
    return ", ".join(parts) if parts else "—"


def rules_md(r):
    return ", ".join(x for x, on in (("pure", r.get("pure")), ("wasm", r.get("wasm"))) if on) or ""


def table(rows):
    out = [BEGIN,
           "| Layer | Crate | What it holds | Depends on | Rules | Built |",
           "|---|---|---|---|---|---|"]
    last = None
    for r in rows:
        layer = LAYER_TITLE[r["layer"]] if r["layer"] != last else ""
        last = r["layer"]
        anchor = r["names"][0]
        out.append(f"| {layer} | [{names_md(r)}](CRATES.md#{anchor}) | {r['summary']} | {depends_md(r)} | {rules_md(r)} | {built_md(r)} |")
    retired = retired_entries()
    if retired:
        parts = []
        for r in retired:
            how = f"renamed `{r['into']}`" if r.get("renamed") else f"in `{r['into']}`"
            if r.get("sources"):
                how += f", {r['sources']}"
            parts.append(f"`{r['name']}` ({how})")
        out.append("")
        out.append("Crates named by earlier ADRs that no longer exist as separate crates: " + ", ".join(parts)
                   + ". A reference to one of them in an ADR reads as a reference to its new home.")
    out.append(END)
    return "\n".join(out)


_data = {}
def retired_entries():
    if "retired" not in _data:
        with open(TOML, "rb") as fh:
            _data["retired"] = tomllib.load(fh).get("retired", [])
    return _data["retired"]


def full(rows):
    out = ["# The crates in full",
           "",
           "Generated by `architecture/crates.py --write` from [`../registry/crates.toml`](../registry/crates.toml): "
           "every crate's row of the crate map ([22](22-crates-and-stack.md) §2.1) with what it holds in full. "
           "Do not edit it by hand: change the registry file and regenerate. "
           "Citations are to the ADR sections whose Crates trailers the entry assembles. "
           "Each entry ends with what the code says (24 §15): the first paragraph of the crate's own `//!` docs "
           "and the sections it claims to implement, or that it is not a crate yet. A change to either makes this "
           "file stale: run `docs/check.sh --write`.",
           ""]
    last = None
    for r in rows:
        if r["layer"] != last:
            out.append(f"## {LAYER_TITLE[r['layer']]}")
            out.append("")
            last = r["layer"]
        out.append(f"### {' '.join(r['names'])}")
        out.append("")
        meta = [f"**Depends on:** {depends_md(r)}"]
        rules = rules_md(r)
        if rules:
            meta.append(f"**Rules:** {rules}")
        out.append(" · ".join(meta))
        out.append("")
        out.append(r["contents"].strip())
        out.append("")
        for n in r["names"]:
            out.append(as_built(n))
            out.append("")
    return "\n".join(out).rstrip("\n") + "\n"


def splice(text, block):
    a, b = text.find(BEGIN), text.find(END)
    if a < 0 or b < 0 or b < a:
        return None
    return text[:a] + block + text[b + len(END):]


def main():
    rows, errs = load()
    if errs:
        for e in errs:
            print("   - registry/crates.toml:", e)
        sys.exit(1)
    tbl = table(rows)
    ful = full(rows)
    chapter = open(CHAPTER, encoding="utf-8").read()
    new_chapter = splice(chapter, tbl)
    problems = []
    if new_chapter is None:
        problems.append("chapter 22: no `crates.py: table` markers")
    elif WRITE:
        if new_chapter != chapter:
            open(CHAPTER, "w", encoding="utf-8").write(new_chapter)
            print("wrote the crate table in chapter 22 §2.1")
    elif new_chapter != chapter:
        problems.append("chapter 22 §2.1's crate table is stale: run `python3 architecture/crates.py --write` from docs/")
    old_full = open(FULL, encoding="utf-8").read() if os.path.exists(FULL) else None
    if WRITE:
        if old_full != ful:
            open(FULL, "w", encoding="utf-8").write(ful)
            print("wrote architecture/CRATES.md")
    elif old_full != ful:
        problems.append("architecture/CRATES.md is stale: run `python3 architecture/crates.py --write` from docs/")
    for p in problems:
        print("   -", p)
    if problems:
        sys.exit(1)
    print(f"ok: {sum(len(r['names']) for r in rows)} crates in {len(rows)} rows, both views current")


if __name__ == "__main__":
    main()
