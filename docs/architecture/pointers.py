#!/usr/bin/env python3
"""Write an ADR's pointer lines from the chapters' provenance lines (24 §3.2).

For each numbered Decision section of the named ADRs that is empty or already a pointer, the body
becomes the line `*Current text: [NN](../architecture/NN-slug.md) §a, §b.*`, naming every chapter
section whose provenance line cites it (or its parent section). The heading, a `*Changed by …*`
provenance line and a `*Superseded by …*` stub are kept. A section that carries text is left alone
and reported, unless --replace is given: then its text is replaced too (the one-off relocation of
24 §14; the caller logs it).

Write the chapter text first, with provenance lines citing the ADR sections; then run this to
fill the ADR. Nothing is logged: a new ADR is born as pointers, and refreshing a pointer after a
chapter section is split or renamed changes no decision.

Usage: pointers.py [--replace] [DOCS_DIR] NNNN [NNNN …]
"""
import glob
import os
import re
import sys

args = [a for a in sys.argv[1:] if not a.startswith("--")]
REPLACE = "--replace" in sys.argv
nums = [a for a in args if re.fullmatch(r"\d{4}", a)]
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = next((a for a in args if os.path.isdir(a)), None) or os.path.dirname(HERE)
DEC = os.path.join(ROOT, "decisions")
ARCH = os.path.join(ROOT, "architecture")

import importlib.util
spec = importlib.util.spec_from_file_location("adrmap", os.path.join(ARCH, "map.py"))
adrmap = importlib.util.module_from_spec(spec)
sys.argv = [sys.argv[0], ROOT]
spec.loader.exec_module(adrmap)
CHAPTERS = adrmap.chapters()
index, _ = adrmap.provenance(CHAPTERS)


def seckey(s):
    return [int(x) for x in s.split(".")]


def pointer_line(adr, sec):
    targets = index.get((adr, sec)) or ([] if "." not in sec else index.get((adr, sec.split(".")[0]), []))
    if not targets:
        return None
    by = {}
    for c, cs in targets:
        by.setdefault(c, set()).add(cs)
    parts = []
    for c in sorted(by):
        secs = sorted(by[c], key=seckey)
        # a chapter's ## section is the union of its ### sections: name only the most specific
        secs = [s for s in secs if "." in s or not any(t.startswith(s + ".") for t in secs)]
        parts.append(f"[{c}](../architecture/{c}-{CHAPTERS[c]}.md) " + ", ".join("§" + s for s in secs))
    return "*Current text: " + "; ".join(parts) + ".*"


def fill(adr):
    path = glob.glob(os.path.join(DEC, f"{adr}-*.md"))[0]
    lines = open(path, encoding="utf-8").read().split("\n")
    start = next(i for i, l in enumerate(lines) if l.startswith("## Decision") or l.startswith("## Part"))
    end = len(lines)
    heads = []
    fenced = False
    for i in range(start + 1, len(lines)):
        l = lines[i]
        if l.lstrip().startswith("```"):
            fenced = not fenced
            continue
        if fenced:
            continue
        if l.startswith("## ") and not l.startswith("## Part"):
            end = i
            break
        m = re.match(r"^(#{3,4}) (\d+(?:\.\d+)?)\.?\s", l)
        if m:
            heads.append((i, m.group(2), len(m.group(1))))
    out = lines[: start + 1]
    i = start + 1
    written, kept, missing = [], [], []
    for k, (hi, sec, lvl) in enumerate(heads):
        out.extend(lines[i:hi + 1])
        body_end = heads[k + 1][0] if k + 1 < len(heads) else end
        body = lines[hi + 1: body_end]
        i = body_end
        meaningful = [l for l in body if l.strip() and not l.startswith("*Changed by") and not l.startswith("*Current text:")]
        first = next((l.strip() for l in body if l.strip()), "")
        has_subs = k + 1 < len(heads) and heads[k + 1][2] > lvl
        if first.startswith("*Superseded by") or (not body or not any(l.strip() for l in body)) and has_subs:
            out.extend(body)
            continue
        if meaningful and not REPLACE:
            kept.append(sec)
            out.extend(body)
            continue
        ptr = pointer_line(adr, sec)
        if ptr is None:
            missing.append(sec)
            out.extend(body)
            continue
        prov = first if first.startswith("*Changed by") else None
        out.append("")
        if prov:
            out.append(prov)
            out.append("")
        out.append(ptr)
        out.append("")
        written.append(sec)
    out.extend(lines[i:])
    open(path, "w", encoding="utf-8").write("\n".join(out))
    print(f"{adr}: pointers written for §{', §'.join(written) or '—'}")
    if kept:
        print(f"{adr}: left as text (use --replace to overwrite): §{', §'.join(kept)}")
    if missing:
        print(f"{adr}: NO CHAPTER CITES §{', §'.join(missing)}: write the text into a chapter first")


for adr in nums:
    fill(adr)
