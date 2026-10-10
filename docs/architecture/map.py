#!/usr/bin/env python3
"""Map every Decision section of every ADR to an architecture chapter.

The chapters are in `README.md` (the CHAPTERS table below must match it). A section's chapter
is decided in this order:

 1. an explicit entry in OVERRIDES for (ADR, section), or for its parent section;
 2. the first TRAILER rule whose pattern matches the section title (the cross-cutting trailer
    sections most ADRs end with: Crates, API, UI, Storage, Permissions, ...);
 3. the ADR's HOME chapter.

A section may map to several chapters ("03+18"): each named chapter absorbs the part of the
section that is about its subject. "--" means the section is process or changelog, not
architecture, and no chapter absorbs it.

Usage:
  map.py [DOCS_DIR]           check: every section of every ADR is mapped, every chapter named
                              exists in README.md, MAP.md is current. Exit 1 on any failure.
  map.py --write [DOCS_DIR]   rewrite MAP.md (and run the checks).
  map.py --totals [DOCS_DIR]  print the words of Decision text each chapter absorbs.

DOCS_DIR defaults to the parent of this file's directory (docs/).
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

CHAPTERS = {
    "00": "overview",
    "01": "log-and-records",
    "02": "graphs-rdf-and-query",
    "03": "storage-caches-and-search",
    "04": "entities-and-identifiers",
    "05": "providers-and-ingest",
    "06": "statements-and-properties",
    "07": "actors-and-accounts",
    "08": "tenants-and-instances",
    "09": "security-and-moderation",
    "10": "pages-and-content-models",
    "11": "rendering-templates-and-modules",
    "12": "files-and-media",
    "13": "mirrored-pages",
    "14": "discussions",
    "15": "structured-pages",
    "16": "logs-feeds-and-notifications",
    "17": "federation-and-publication",
    "18": "api",
    "19": "site-ui",
    "20": "web-tier",
    "21": "special-pages",
    "22": "crates-and-stack",
    "23": "configuration-and-registry",
}

# The chapter an ADR's sections go to unless a rule or override says otherwise.
HOME = {
    "0000": "00", "0001": "02", "0002": "05", "0003": "19", "0004": "04", "0005": "22",
    "0006": "01", "0007": "07", "0008": "10", "0009": "04", "0010": "19", "0011": "16",
    "0012": "18", "0013": "03", "0014": "03", "0015": "01", "0016": "09", "0017": "04",
    "0018": "08", "0019": "14", "0020": "16", "0021": "16", "0022": "17", "0023": "09",
    "0024": "07", "0025": "07", "0026": "06", "0027": "07", "0028": "08", "0029": "06",
    "0030": "09", "0031": "06", "0032": "02", "0033": "22", "0034": "19", "0035": "05",
    "0036": "05", "0037": "05", "0038": "10", "0039": "12", "0040": "08", "0041": "10",
    "0042": "11", "0043": "11", "0044": "04", "0045": "15", "0046": "08", "0047": "21",
    "0048": "04", "0049": "14", "0050": "--", "0051": "10", "0052": "13", "0053": "13",
    "0054": "13", "0055": "11", "0056": "09", "0057": "20", "0058": "03", "0059": "02",
    "0060": "15", "0061": "15", "0062": "15", "0063": "15", "0064": "15", "0065": "12",
    "0066": "04", "0067": "14", "0068": "13", "0069": "13", "0070": "05", "0071": "13",
    "0072": "13", "0073": "13", "0074": "17", "0075": "18", "0076": "17", "0077": "21",
    "0078": "05", "0079": "08", "0080": "08", "0081": "08",
    "0082": "04", "0083": "03", "0084": "18",
}

# Trailer sections, by title. First match wins. Multi-subject titles split across chapters.
TRAILER = [
    (r"^Crates\b", "22"), (r"^Scatterbase\b", "22"), (r"^Conformance\b", "22"), (r"^Testing\b", "22"),
    (r"^Storage, API and RDF", "03+18+02"), (r"^Storage, operations and API", "03+18"),
    (r"^API, operations and storage", "18+03"), (r"^Pages, UI and API", "18+19"), (r"^UI and API", "19+18"),
    (r"^API and (UI|operations)", "18"), (r"^(Action API|REST|API)\b", "18"),
    (r"^Special pages, API and search", "21+18+03"), (r"^Display, logs and RDF", "19+16+02"),
    (r"^(UI|Site UI|Rendering and the UI|Editing and preview)\b", "19"),
    (r"^Search and filters", "03+09"), (r"^Storage\b", "03"), (r"^Search\b", "03"), (r"^Terms, search and storage", "03"),
    (r"^Filters and feeds", "09+16"), (r"^Permissions\b", "09"),
    (r"^Logs and RDF", "16+02"), (r"^RDF\b", "02"), (r"^IRIs and RDF", "02"), (r"^Log events", "16"), (r"^Feeds, watch", "16"),
    (r"^Rate limits", "07"), (r"^Limits, rate limits", "07"), (r"^Tenants\b", "08"), (r"^Federation\b", "17"),
    (r"^Registry", "23"), (r"^Configuration and registry", "23"), (r"^Settings\b", "23"),
    (r"^Reports\b", "21"), (r"^Deployment profiles", "22"), (r"^Changelog", "--"),
]

# Sections whose subject is not the ADR's home and not a trailer. A parent entry covers its
# subsections unless they have their own.
OVERRIDES = {
    ("0003", "7"): "06", ("0003", "9"): "19",
    ("0005", "4.1"): "02", ("0005", "4.2"): "01", ("0005", "4.3"): "01", ("0005", "4.4"): "03",
    ("0005", "5"): "02", ("0005", "6"): "22", ("0005", "8"): "--",
    ("0007", "8"): "07", ("0007", "9"): "02",
    ("0008", "5"): "10", ("0008", "9"): "10", ("0008", "10"): "10",
    ("0009", "9"): "05", ("0009", "11"): "10",
    ("0010", "3"): "19",
    ("0011", "8"): "02", ("0011", "9"): "16",
    ("0012", "2"): "18", ("0012", "2.1"): "01", ("0012", "2.2"): "05", ("0012", "3"): "16",
    ("0012", "6"): "05", ("0012", "7"): "18", ("0012", "8"): "09",
    ("0013", "3"): "01", ("0013", "8"): "02", ("0013", "9"): "05", ("0013", "11"): "22", ("0013", "12"): "--",
    ("0015", "3"): "23", ("0015", "5"): "02", ("0015", "6"): "02",
    ("0016", "8"): "22",
    ("0018", "6"): "03", ("0018", "7"): "04", ("0018", "8"): "19", ("0018", "11"): "18+19",
    ("0019", "7"): "16", ("0019", "10"): "16", ("0019", "11"): "03",
    ("0020", "4"): "16", ("0020", "6"): "03",
    ("0021", "6"): "19", ("0021", "8"): "03",
    ("0022", "10"): "03", ("0022", "11"): "18+19",
    ("0023", "10"): "03",
    ("0024", "4"): "07", ("0024", "7"): "08", ("0024", "10"): "03",
    ("0025", "7"): "08", ("0025", "8"): "03",
    ("0026", "5"): "04", ("0026", "6"): "03",
    ("0027", "8"): "03",
    ("0028", "3"): "09", ("0028", "4"): "09", ("0028", "9"): "16", ("0028", "11"): "18+19", ("0028", "12"): "03",
    ("0029", "8"): "23",
    ("0030", "6"): "16", ("0030", "7"): "05", ("0030", "8"): "09", ("0030", "11"): "03",
    ("0031", "4"): "06",
    ("0033", "4"): "03", ("0033", "5"): "03", ("0033", "9"): "22", ("0033", "9.1"): "11", ("0033", "9.2"): "14",
    ("0033", "9.3"): "04", ("0033", "9.4"): "09", ("0033", "10"): "13", ("0033", "11"): "02", ("0033", "12"): "23",
    ("0034", "5"): "20", ("0034", "8"): "22", ("0034", "11"): "09",
    ("0035", "4"): "05", ("0035", "5"): "07", ("0035", "6"): "16",
    ("0036", "3"): "04", ("0036", "6"): "04", ("0036", "7"): "23+22",
    ("0037", "8"): "23+22",
    ("0038", "1"): "06", ("0038", "2"): "06", ("0038", "6"): "06", ("0038", "7"): "19", ("0038", "9"): "14",
    ("0038", "10"): "03", ("0038", "11"): "02", ("0038", "14"): "09",
    ("0039", "3"): "03", ("0039", "4"): "12", ("0039", "15"): "07", ("0039", "16"): "16", ("0039", "20"): "03",
    ("0040", "9"): "09",
    ("0041", "6"): "12", ("0041", "7"): "12", ("0041", "10"): "10",
    ("0042", "2"): "23", ("0042", "12"): "18", ("0042", "13"): "10", ("0042", "16"): "07", ("0042", "17"): "03+09",
    ("0043", "8"): "04", ("0043", "9"): "11", ("0043", "14"): "22",
    ("0044", "3"): "18", ("0044", "4"): "17", ("0044", "5"): "10",
    ("0045", "8"): "16", ("0045", "12"): "03",
    ("0046", "4"): "08+01", ("0046", "7"): "02", ("0046", "8"): "09", ("0046", "9"): "18+19",
    ("0047", "13"): "03",
    ("0048", "5"): "02", ("0048", "6"): "04", ("0048", "7"): "23",
    ("0049", "9"): "16+02", ("0049", "12"): "03", ("0049", "14"): "15",
    ("0051", "5"): "03", ("0051", "6"): "21",
    ("0052", "6"): "04+18", ("0052", "8"): "03", ("0052", "9"): "09+08",
    ("0053", "7"): "03", ("0053", "8"): "03", ("0053", "11"): "09+07",
    ("0054", "8"): "09+07", ("0054", "10"): "03",
    ("0055", "6"): "10", ("0055", "7"): "23",
    ("0056", "7"): "03", ("0056", "8"): "03", ("0056", "10"): "20", ("0056", "11"): "08", ("0056", "13"): "18+19",
    ("0056", "14"): "03", ("0056", "15"): "09",
    ("0057", "5"): "03", ("0057", "6"): "03", ("0057", "11"): "22", ("0057", "13"): "07", ("0057", "14"): "11",
    ("0058", "1"): "01+03", ("0058", "6"): "01+03", ("0058", "7"): "05", ("0058", "8"): "08",
    ("0059", "4"): "08", ("0059", "6"): "18+19", ("0059", "7"): "22",
    ("0060", "7"): "16", ("0060", "8"): "18+19", ("0060", "10"): "03",
    ("0061", "8"): "16", ("0061", "9"): "21", ("0061", "11"): "03",
    ("0062", "7"): "21",
    ("0064", "8"): "02",
    ("0065", "3"): "15",
    ("0066", "5"): "03", ("0066", "6"): "02", ("0066", "7"): "19+21", ("0066", "9"): "04",
    ("0067", "4"): "17", ("0067", "6"): "17", ("0067", "8"): "09+07",
    ("0068", "6"): "19+18",
    ("0069", "4"): "14", ("0069", "8"): "16", ("0069", "10"): "03", ("0069", "11"): "23+09",
    ("0070", "9"): "18", ("0070", "10"): "03",
    ("0071", "12"): "03", ("0071", "13"): "18",
    ("0072", "5"): "21", ("0072", "7"): "22",
    ("0075", "5"): "09+07", ("0075", "6"): "23+18",
    ("0077", "12"): "22", ("0077", "13"): "23", ("0077", "14"): "23", ("0077", "15"): "22",
    ("0078", "10"): "18", ("0078", "11"): "05",
    ("0079", "1"): "08+07", ("0079", "2"): "08+07", ("0079", "4"): "08+07", ("0079", "6"): "07", ("0079", "9"): "07",
    ("0079", "7"): "18", ("0079", "8"): "22",
    ("0080", "1"): "05", ("0080", "2"): "08+05", ("0080", "3"): "08", ("0080", "4"): "05", ("0080", "5"): "08", ("0080", "6"): "13",
    ("0080", "7"): "04", ("0080", "8"): "18", ("0080", "9"): "22",
    ("0081", "1"): "08", ("0081", "2"): "01", ("0081", "3"): "08", ("0081", "4"): "08", ("0081", "5"): "01+08",
    ("0081", "6"): "01", ("0081", "7"): "17", ("0081", "8"): "01", ("0081", "9"): "23", ("0081", "10"): "22",
    ("0082", "4"): "02", ("0082", "5"): "03", ("0082", "5.1"): "03", ("0082", "5.2"): "06", ("0082", "6"): "03",
}

VERBS = r"amends|extends|supersedes|corrects|settles|uses"


def strip_fences(text):
    out, fenced = [], False
    for line in text.split("\n"):
        if line.lstrip().startswith("```"):
            fenced = not fenced
            out.append("")
        else:
            out.append("" if fenced else line)
    return "\n".join(out)


def sections_of(path):
    """Yield (number, title, words) for every numbered section under ## Decision (or ## Part …)."""
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
        words = len(" ".join(lines[i + 1:bounds[k + 1]]).split())
        yield num, title, words


def chapter_for(adr, num, title):
    ch = OVERRIDES.get((adr, num))
    if ch is None and "." in num:
        ch = OVERRIDES.get((adr, num.split(".")[0]))
    if ch is None:
        for rx, c in TRAILER:
            if re.search(rx, title):
                ch = c
                break
    return ch if ch is not None else HOME[adr]


def rows():
    for path in sorted(glob.glob(os.path.join(DEC, "0*.md"))):
        name = os.path.basename(path)
        adr = name[:4]
        first = open(path, encoding="utf-8").readline().strip()
        title = first.lstrip("# ").split(". ", 1)[1] if ". " in first else first
        for num, stitle, words in sections_of(path):
            yield adr, name, title, num, stitle, words, chapter_for(adr, num, stitle)


def chapter_link(c):
    return f"[{c}]({c}-{CHAPTERS[c]}.md)" if c in CHAPTERS else "—"


def render(all_rows):
    out = [
        "# ADR section map",
        "",
        "Generated by `map.py --write` from the ADRs and the rules in `map.py`. Do not edit it by hand: "
        "change the rules and regenerate. Every numbered Decision section of every ADR is listed with the "
        "chapter (or chapters) that absorb its current text; `—` means the section is process or "
        "changelog and no chapter absorbs it. Since 0050 §14 the ADR section is a pointer to these chapter sections.",
        "",
        "| ADR | § | Section | Chapter |",
        "|---|---|---|---|",
    ]
    cur = None
    for adr, name, title, num, stitle, words, ch in all_rows:
        if adr != cur:
            out.append(f"| **[{adr}](../decisions/{name}) {title}** | | | |")
            cur = adr
        chs = " + ".join(chapter_link(c) for c in ch.split("+"))
        out.append(f"| | §{num} | {stitle} | {chs} |")
    return "\n".join(out) + "\n"


def main():
    all_rows = list(rows())
    problems = []

    # Every chapter named exists in README.md's chapter table.
    readme = open(os.path.join(ARCH, "README.md"), encoding="utf-8").read()
    for c, slug in CHAPTERS.items():
        if f"{c}-{slug}.md" not in readme:
            problems.append(f"chapter {c}-{slug}.md is not in README.md")
    for adr, name, title, num, stitle, words, ch in all_rows:
        for c in ch.split("+"):
            if c != "--" and c not in CHAPTERS:
                problems.append(f"{adr} §{num}: unknown chapter {c}")
    # Every ADR has a home and every override names a real section.
    seen = {(r[0], r[3]) for r in all_rows}
    for adr in {r[0] for r in all_rows}:
        if adr not in HOME:
            problems.append(f"{adr}: no HOME chapter")
    for key in OVERRIDES:
        if key not in seen:
            problems.append(f"override {key[0]} §{key[1]} names a section that does not exist")

    # Written chapters: every mapped section is cited in a provenance line of its chapter, every
    # provenance citation names a real ADR section, and every heading has a provenance line.
    all_sections = {(r[0], r[3]) for r in all_rows}
    adr_files = {r[0]: r[1] for r in all_rows}
    for c, slug in CHAPTERS.items():
        cpath = os.path.join(ARCH, f"{c}-{slug}.md")
        if not os.path.exists(cpath):
            continue
        text = strip_fences(open(cpath, encoding="utf-8").read())
        lines = text.split("\n")
        cited = set()
        for i, line in enumerate(lines):
            if re.match(r"^#{2,3} ", line):
                nxt = next((l for l in lines[i + 1:i + 3] if l.strip()), "")
                if not nxt.startswith("*Sources:"):
                    problems.append(f"{c}: heading without a provenance line: {line.strip()}")
            if line.startswith("*Sources:"):
                for adr, secs in re.findall(r"\[(\d{4})\]\([^)]*\)((?:\s*§\d+(?:\.\d+)?,?)+)", line):
                    for s in re.findall(r"§(\d+(?:\.\d+)?)", secs):
                        cited.add((adr, s))
                        if (adr, s) not in all_sections:
                            problems.append(f"{c}: provenance cites {adr} §{s}, which does not exist")
        for adr, name, title, num, stitle, words, ch in all_rows:
            if c not in ch.split("+"):
                continue
            parent = num.split(".")[0]
            if (adr, num) not in cited and (adr, parent) not in cited:
                problems.append(f"{c}: mapped section {adr} §{num} ({stitle}) is not cited in any provenance line")
        for adr, s in cited:
            if adr in adr_files and f"../decisions/{adr_files[adr]}" not in open(cpath, encoding="utf-8").read():
                problems.append(f"{c}: link to {adr} does not use ../decisions/{adr_files[adr]}")

    # PENDING.md: every chapter's "Pending X" marker names a row, and every A/B row has a marker.
    pending_path = os.path.join(ARCH, "PENDING.md")
    if os.path.exists(pending_path):
        ptext = open(pending_path, encoding="utf-8").read()
        rows_ids = set(re.findall(r"^\| ([A-F]\d+) \|", ptext, re.M))
        marked = set()
        for c, slug in CHAPTERS.items():
            cpath = os.path.join(ARCH, f"{c}-{slug}.md")
            if not os.path.exists(cpath):
                continue
            for m in re.findall(r"\*\*Pending ([A-F0-9, ]+)\.\*\*", open(cpath, encoding="utf-8").read()):
                for pid in re.findall(r"[A-F]\d+", m):
                    marked.add(pid)
                    if pid not in rows_ids:
                        problems.append(f"{c}: Pending {pid} has no row in PENDING.md")
        for pid in sorted(rows_ids - marked):
            problems.append(f"PENDING.md row {pid} is marked in no chapter")

    if "--totals" in sys.argv:
        totals = {}
        for r in all_rows:
            cs = r[6].split("+")
            for c in cs:
                totals[c] = totals.get(c, 0) + r[5] / len(cs)
        for c in sorted(totals):
            print(f"{c}  {CHAPTERS.get(c, '(none)'):36} {int(totals[c]):6} words")
        print(f"{len(all_rows)} sections")

    text = render(all_rows)
    map_path = os.path.join(ARCH, "MAP.md")
    if "--write" in sys.argv:
        open(map_path, "w", encoding="utf-8").write(text)
        print(f"wrote {map_path} ({len(all_rows)} sections)")
    elif not os.path.exists(map_path) or open(map_path, encoding="utf-8").read() != text:
        problems.append("MAP.md is out of date: run map.py --write")

    for p in problems:
        print("PROBLEM:", p)
    if problems:
        sys.exit(1)
    if "--write" not in sys.argv and "--totals" not in sys.argv:
        print(f"ok: {len(all_rows)} sections mapped")


if __name__ == "__main__":
    main()
