#!/usr/bin/env python3
"""Check that the architecture chapters cover every ADR decision (24 §10.2, §12).

The chapters' provenance lines (`*Sources: [0013](../decisions/0013-….md) §7; …*` under every
`##` and `###` heading) are the only map between ADR sections and chapter text. This checks:
 1. every chapter file `NN-slug.md` is in README.md's chapter table, and vice versa;
 2. every `##` and `###` heading is followed by a provenance line, and every citation in one
    names an ADR section that exists;
 3. every numbered Decision section of every ADR, other than a stub, is cited by at least one
    provenance line (a parent citation covers its subsections);
 4. links to ADRs use `../decisions/NNNN-slug.md`;
 5. every `**Pending X.**` marker in a chapter has a row in PENDING.md, and every row a marker.

Usage: map.py [DOCS_DIR]      DOCS_DIR defaults to the parent of this file's directory (docs/).
Exit 1 on any failure. `--sections` prints every ADR section with the chapter sections that cite it.
"""
import glob
import os
import re
import sys

ARGS = [a for a in sys.argv[1:] if not a.startswith("--")]
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = ARGS[0] if ARGS else os.path.dirname(HERE)
DEC = os.path.join(ROOT, "decisions")
ARCH = os.path.join(ROOT, "architecture")
VERBS = "amends|extends|supersedes|corrects|settles|uses"


def strip_fences(text):
    return re.sub(r"^```.*?^```", "", text, flags=re.M | re.S)


def chapters():
    """{NN: slug} for every NN-slug.md in the architecture directory."""
    out = {}
    for path in sorted(glob.glob(os.path.join(ARCH, "[0-9][0-9]-*.md"))):
        name = os.path.basename(path)[:-3]
        out[name[:2]] = name[3:]
    return out


def sections_of(path):
    """Yield (number, title, stub) for every numbered section under ## Decision (or ## Part …)."""
    lines = strip_fences(open(path, encoding="utf-8").read()).split("\n")
    heads = [(i, len(m.group(1)), m.group(2)) for i, l in enumerate(lines)
             if (m := re.match(r"^(#{2,4}) (.*)", l))]
    bounds = [i for i, _, _ in heads] + [len(lines)]
    part = None
    for k, (i, lvl, name) in enumerate(heads):
        if lvl == 2:
            part = name
            continue
        if part != "Decision" and not (part or "").startswith("Part"):
            continue
        m = re.match(r"^(\d+(?:\.\d+)?)\.?\s+(.*)", name)
        if not m:
            continue
        num, title = m.groups()
        title = re.sub(r"\s*\((" + VERBS + r")\b.*\)\s*$", "", title).strip()
        first = next((l.strip() for l in lines[i + 1:bounds[k + 1]] if l.strip()), "")
        yield num, title, first.startswith("*Superseded by")


def adr_sections():
    """{(adr, num): (file, title, stub)} over every ADR."""
    out = {}
    for path in sorted(glob.glob(os.path.join(DEC, "0*.md"))):
        name = os.path.basename(path)
        for num, title, stub in sections_of(path):
            out[(name[:4], num)] = (name, title, stub)
    return out


def provenance(chs):
    """{(adr, sec): [(chapter, chapter section)]} from every provenance line, plus the problems found."""
    cited = {}
    problems = []
    for c, slug in chs.items():
        cpath = os.path.join(ARCH, f"{c}-{slug}.md")
        raw = open(cpath, encoding="utf-8").read()
        lines = strip_fences(raw).split("\n")
        cur = None
        for i, line in enumerate(lines):
            m = re.match(r"^(#{2,3}) (\d+(?:\.\d+)?)\.?\s", line)
            if m:
                cur = m.group(2)
            if re.match(r"^#{2,3} ", line):
                nxt = next((l for l in lines[i + 1:i + 3] if l.strip()), "")
                if not nxt.startswith("*Sources:"):
                    problems.append(f"{c}: heading without a provenance line: {line.strip()}")
            if line.startswith("*Sources:") and cur:
                for adr, specs in re.findall(r"\[(\d{4})\]\([^)]*\)((?:\s*§\d+(?:\.\d+)?,?)+)", line):
                    for s in re.findall(r"§(\d+(?:\.\d+)?)", specs):
                        cited.setdefault((adr, s), []).append((c, cur))
        prose = re.sub(r"`[^`\n]*`", "", strip_fences(raw))
        for adr, link in set(re.findall(r"\[(\d{4})\]\(([^)]*)\)", prose)):
            if not re.fullmatch(rf"\.\./decisions/{adr}-[^/]+\.md", link):
                problems.append(f"{c}: link to {adr} does not use ../decisions/{adr}-slug.md ({link})")
    return cited, problems


def main():
    problems = []
    chs = chapters()

    # 1. README's table and the files agree
    readme = open(os.path.join(ARCH, "README.md"), encoding="utf-8").read()
    for c, slug in chs.items():
        if f"{c}-{slug}.md" not in readme:
            problems.append(f"chapter {c}-{slug}.md is not in README.md")
    for c, slug in re.findall(r"\((\d{2})-([a-z0-9-]+)\.md\)", readme):
        if chs.get(c) != slug:
            problems.append(f"README.md names {c}-{slug}.md, which does not exist")

    # 2. provenance lines
    secs = adr_sections()
    cited, prov_problems = provenance(chs)
    problems += prov_problems
    for (adr, s), where in cited.items():
        if (adr, s) not in secs:
            problems.append(f"{', '.join(f'{c} §{cs}' for c, cs in where)}: provenance cites {adr} §{s}, which does not exist")

    # 3. every ADR section is cited
    for (adr, num), (name, title, stub) in secs.items():
        if stub:
            continue
        parent = num.split(".")[0]
        if (adr, num) not in cited and (adr, parent) not in cited:
            problems.append(f"{adr} §{num} ({title}) is cited by no chapter: its text has no home")

    # 5. PENDING.md
    pending_path = os.path.join(ARCH, "PENDING.md")
    if os.path.exists(pending_path):
        ptext = open(pending_path, encoding="utf-8").read()
        rows_ids = set(re.findall(r"^\| ([A-F]\d+) \|", ptext, re.M))
        marked = set()
        for c, slug in chs.items():
            for m in re.findall(r"\*\*Pending ([A-F0-9, ]+)\.\*\*", open(os.path.join(ARCH, f"{c}-{slug}.md"), encoding="utf-8").read()):
                for pid in re.findall(r"[A-F]\d+", m):
                    marked.add(pid)
                    if pid not in rows_ids:
                        problems.append(f"{c}: Pending {pid} has no row in PENDING.md")
        for pid in sorted(rows_ids - marked):
            problems.append(f"PENDING.md row {pid} is marked in no chapter")

    if "--sections" in sys.argv:
        for (adr, num), (name, title, stub) in secs.items():
            where = cited.get((adr, num)) or cited.get((adr, num.split(".")[0]), [])
            print(f"{adr} §{num:6} {title[:50]:50} {'stub' if stub else ', '.join(f'{c} §{cs}' for c, cs in where)}")

    for p in problems:
        print("PROBLEM:", p)
    if problems:
        sys.exit(1)
    print(f"ok: {len(secs)} ADR sections, {len(chs)} chapters, every section cited")


if __name__ == "__main__":
    main()
