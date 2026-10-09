#!/usr/bin/env python3
"""Relocate an ADR's Decision text to the architecture chapters (0050 §14).

For each numbered Decision section that MAP.md assigns to a chapter, the body becomes the pointer
line `*Current text: [NN](../architecture/NN-slug.md) §a, §b.*`, naming every chapter section whose
provenance line cites it (or its parent section). The heading and a `*Changed by …*` provenance line
are kept; a `*Superseded by …*` stub is left alone. The header gains `Chapters`, `Updated` is set,
a `relocates` entry is appended to the Amendment log, and 0050's `Changes` header and "Changes to
other ADRs" table gain the row for this ADR.

Usage: pointers.py --commit <sha> [DOCS_DIR] NNNN [NNNN …]
       pointers.py --commit <sha> [DOCS_DIR] --all
"""
import glob
import os
import re
import sys

args = [a for a in sys.argv[1:] if not a.startswith("--")]
ALL = "--all" in sys.argv
commit = sys.argv[sys.argv.index("--commit") + 1] if "--commit" in sys.argv else None
if not commit:
    sys.exit("pointers.py: --commit <sha> is required (the last commit in which the ADR carried its text)")
args = [a for a in args if a != commit]
ROOT = next((a for a in args if os.path.isdir(a)), None)
nums = [a for a in args if re.fullmatch(r"\d{4}", a)]
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = ROOT or os.path.dirname(HERE)
DEC = os.path.join(ROOT, "decisions")
ARCH = os.path.join(ROOT, "architecture")
TODAY = "2026-10-09"

sys.argv = [sys.argv[0], ROOT]
import importlib.util
spec = importlib.util.spec_from_file_location("adrmap", os.path.join(ARCH, "map.py"))
adrmap = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adrmap)
CHAPTERS = adrmap.CHAPTERS
mapped = {}
for adr, name, title, num, stitle, words, ch in adrmap.rows():
    mapped[(adr, num)] = ch

# chapter provenance index: (adr, sec) -> [(chapter, section)]
index = {}
for c, slug in CHAPTERS.items():
    cpath = os.path.join(ARCH, f"{c}-{slug}.md")
    if not os.path.exists(cpath):
        continue
    lines = adrmap.strip_fences(open(cpath, encoding="utf-8").read()).split("\n")
    cur = None
    for line in lines:
        m = re.match(r"^(#{2,3}) (\d+(?:\.\d+)?)\.?\s", line)
        if m:
            cur = m.group(2)
            continue
        if cur and line.startswith("*Sources:"):
            for adr, specs in re.findall(r"\[(\d{4})\]\([^)]*\)((?:\s*§\d+(?:\.\d+)?,?)+)", line):
                for s in re.findall(r"§(\d+(?:\.\d+)?)", specs):
                    index.setdefault((adr, s), []).append((c, cur))


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


def relocate(adr):
    path = glob.glob(os.path.join(DEC, f"{adr}-*.md"))[0]
    text = open(path, encoding="utf-8").read()
    lines = text.split("\n")
    # locate the Decision region: from "## Decision" to the next "## " that is not a Part
    start = next(i for i, l in enumerate(lines) if l.startswith("## Decision") or l.startswith("## Part"))
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("## ") and not lines[i].startswith("## Part")), len(lines))
    # headings in the region (### and ####), tracking fences
    heads = []
    fenced = False
    for i in range(start + 1, end):
        l = lines[i]
        if l.lstrip().startswith("```"):
            fenced = not fenced
            continue
        if fenced:
            continue
        m = re.match(r"^(#{3,4}) (\d+(?:\.\d+)?)\.?\s", l)
        if m:
            heads.append((i, m.group(2)))
    out = lines[: start + 1]
    i = start + 1
    chapters = set()
    relocated = []
    for k, (hi, sec) in enumerate(heads):
        out.extend(lines[i:hi + 1])          # text before this heading (part markers etc.), and the heading
        body_end = heads[k + 1][0] if k + 1 < len(heads) else end
        body = lines[hi + 1: body_end]
        i = body_end
        first = next((l for l in body if l.strip()), "")
        ch = mapped.get((adr, sec), "--")
        if ch == "--" or first.strip().startswith("*Superseded by") or any(l.startswith("*Current text:") for l in body):
            out.extend(body)
            continue
        ptr = pointer_line(adr, sec)
        if ptr is None:
            sys.exit(f"{adr} §{sec}: no chapter section cites it")
        prov = first.strip() if first.strip().startswith("*Changed by") else None
        out.append("")
        if prov:
            out.append(prov)
            out.append("")
        out.append(ptr)
        out.append("")
        chapters |= set(re.findall(r"\[(\d{2})\]\(\.\./architecture/", ptr))
        if "." not in sec:
            relocated.append(sec)
    out.extend(lines[i:])
    text = "\n".join(out)
    if not relocated:
        print(f"{adr}: nothing to relocate")
        return
    # header
    chap_line = "- **Chapters:** " + ", ".join(f"[{c}](../architecture/{c}-{CHAPTERS[c]}.md)" for c in sorted(chapters))
    head, rest = text.split("\n## ", 1)
    hl = head.split("\n")
    hl = [l for l in hl if not l.startswith("- **Chapters:**")]
    # Amendment log id
    ids = [int(x) for x in re.findall(r"^### A(\d+)\. ", text, re.M)]
    n = max(ids) + 1 if ids else 1
    upd = f"- **Updated:** {TODAY} (A{n})"
    if any(l.startswith("- **Updated:**") for l in hl):
        hl = [upd if l.startswith("- **Updated:**") else l for l in hl]
    else:
        di = next(i for i, l in enumerate(hl) if l.startswith("- **Date:**"))
        hl.insert(di + 1, upd)
    # Chapters after Uses, else after Changes, else after Author
    for key in ("- **Uses:**", "- **Changes:**", "- **Author:**"):
        idx = next((i for i, l in enumerate(hl) if l.startswith(key)), None)
        if idx is not None:
            hl.insert(idx + 1, chap_line)
            break
    text = "\n".join(hl) + "\n## " + rest
    # log entry
    secs = sorted(relocated, key=seckey)
    change = f"relocates §{secs[0]}–§{secs[-1]}" if len(secs) > 1 else f"relocates §{secs[0]}"
    if len(secs) > 1 and [int(s) for s in secs] != list(range(int(secs[0]), int(secs[-1]) + 1)):
        change = "relocates " + ", ".join("§" + s for s in secs)
    entry = f"""### A{n}. Current text relocated to the architecture chapters

- **Date:** {TODAY}
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** {change}
- **Summary:** The Decision's current text now lives in the architecture chapters {", ".join(f"[{c}](../architecture/{c}-{CHAPTERS[c]}.md)" for c in sorted(chapters))}, in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `{commit}`. No decision changed.
"""
    if "## Amendment log" in text:
        text = text.rstrip("\n") + "\n\n" + entry
    else:
        text = text.rstrip("\n") + "\n\n## Amendment log\n\n" + entry
    open(path, "w", encoding="utf-8").write(text)
    # 0050: Changes header and the table
    p50 = os.path.join(DEC, "0050-adr-format.md")
    t50 = open(p50, encoding="utf-8").read()
    if adr != "0050":
        base = os.path.basename(path)
        row = f"| [{adr}]({base}) §{secs[0]}–{secs[-1]} | §14 | relocates | {adr} A{n} |" if len(secs) > 1 else f"| [{adr}]({base}) §{secs[0]} | §14 | relocates | {adr} A{n} |"
        if f"| relocates | {adr} A" not in t50:
            tbl_end = t50.rindex("\n## References")
            t50 = t50[:tbl_end].rstrip("\n") + "\n" + row + t50[tbl_end:]
        m = re.search(r"^- \*\*Changes:\*\* (.*)$", t50, re.M)
        links = dict(re.findall(r"\[(\d{4})\]\(([^)]*)\)", m.group(1)))
        links[adr] = base
        newline = "- **Changes:** " + ", ".join(f"[{k}]({links[k]})" for k in sorted(links))
        t50 = t50[: m.start()] + newline + t50[m.end():]
        open(p50, "w", encoding="utf-8").write(t50)
    print(f"{adr}: relocated §{', §'.join(secs)} -> chapters {', '.join(sorted(chapters))} (A{n})")


if ALL:
    nums = sorted(os.path.basename(f)[:4] for f in glob.glob(os.path.join(DEC, "0*.md")))
for adr in nums:
    if adr == "0050":
        continue
    relocate(adr)
