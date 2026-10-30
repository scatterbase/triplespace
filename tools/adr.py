#!/usr/bin/env python3
"""Look things up in the ADRs and architecture chapters without grep chains.

  adr.py show 0013 §7        an ADR section; a pointer is followed to the chapter text it names
  adr.py show 0013 §7 --adr  the ADR section as written (heading, provenance, pointer), not followed
  adr.py show 0098 --full    the whole ADR with every pointer replaced by its chapter text
  adr.py show 03 §4.5        a chapter section
  adr.py show 0013           an ADR's header and outline
  adr.py outline 03          a chapter's (or ADR's) headings
  adr.py where entity source chapter sections, ADRs and registry files that mention a phrase
  adr.py log 0008            an ADR's Amendment log; `log 0008 A6` for one entry
  adr.py questions 0090      an ADR's open questions, settled ones marked
  adr.py code 0013 §7        the code that cites a section (file:line), from tools/coverage.py

Four digits name an ADR, one or two a chapter. "§" is optional: `show 0013 7.1` works.
`where` ranks by mentions (case-insensitive), sections and ADRs whose heading or title matches
first (marked *), and prints at most --limit rows per group.
Links between docs print as their text alone (`[0013](../decisions/0013-….md) §7` as `0013 §7`)
to keep output short; --raw keeps them. Standard library only; run from anywhere in the repository.
"""
import argparse
import glob
import os
import re
import signal
import sys

# Behave like other Unix tools when piped into head.
if hasattr(signal, "SIGPIPE"):
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOCS = os.path.join(ROOT, "docs")
DEC = os.path.join(DOCS, "decisions")
ARCH = os.path.join(DOCS, "architecture")
REG = os.path.join(DOCS, "registry")


def die(msg):
    print(f"adr.py: {msg}", file=sys.stderr)
    sys.exit(1)


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def adr_path(num):
    hits = glob.glob(os.path.join(DEC, f"{num}-*.md"))
    if not hits:
        die(f"no ADR {num}")
    return hits[0]


def chapter_path(num):
    hits = glob.glob(os.path.join(ARCH, f"{int(num):02d}-*.md"))
    if not hits:
        die(f"no chapter {num}")
    return hits[0]


def doc_for(ref):
    """'0013' -> ('adr', path); '3' or '03' -> ('chapter', path)."""
    ref = ref.strip()
    if re.fullmatch(r"\d{4}", ref):
        return "adr", adr_path(ref)
    if re.fullmatch(r"\d{1,2}", ref):
        return "chapter", chapter_path(ref)
    die(f"{ref!r} is neither an ADR number (four digits) nor a chapter number (one or two)")


def headings(lines):
    """(index, level, text) for every heading outside code fences."""
    out, fence = [], False
    for i, line in enumerate(lines):
        if line.lstrip().startswith("```"):
            fence = not fence
            continue
        if not fence:
            m = re.match(r"^(#{1,6}) (.*)$", line)
            if m:
                out.append((i, len(m.group(1)), m.group(2).rstrip()))
    return out


def block(lines, heads, start):
    """Lines of the section whose heading is at line index `start`, up to the next heading at
    the same or a higher level."""
    level = next(lv for i, lv, _ in heads if i == start)
    end = next((i for i, lv, _ in heads if i > start and lv <= level), len(lines))
    return lines[start:end]


def decision_range(heads, n):
    """Line range of '## Decision' in an ADR, so the template's examples and the log's
    '### A1.' headings never match a section number."""
    start = next((i for i, lv, t in heads if lv == 2 and t == "Decision"), 0)
    # Some Decisions are split into "## Part A — …" headings, so end only at the parts that follow.
    end = next((i for i, lv, t in heads if lv == 2 and i > start and t in AFTER_DECISION), n)
    return start, end


AFTER_DECISION = ("Alternatives considered", "Consequences", "Open questions", "Changes to other ADRs", "References", "Amendment log")


def find_section(kind, lines, num):
    heads = headings(lines)
    lo, hi = decision_range(heads, len(lines)) if kind == "adr" else (0, len(lines))
    pat = re.compile(rf"^{re.escape(num)}(\.|\s|$)")
    for i, lv, text in heads:
        if lo <= i < hi and pat.match(text):
            return i, heads
    return None, heads


POINTER = re.compile(r"^\*Current text: (.*)\*\s*$")


def pointer_targets(body):
    """[(chapter, section), …] from a '*Current text: [03](…) §4.5, §4.6; [18](…) §2.*' line."""
    for line in body:
        m = POINTER.match(line.strip())
        if m:
            out = []
            for part in m.group(1).split(";"):
                cm = re.search(r"\[(\d{2})\]", part)
                if cm:
                    out += [(cm.group(1), s) for s in re.findall(r"§(\d+(?:\.\d+)*)", part)]
            return out
    return []


def clean_secnum(s):
    s = s.replace("§", "").strip()
    if not re.fullmatch(r"\d+(\.\d+)*", s):
        die(f"{s!r} is not a section number")
    return s


def emit(body, limit):
    body = "\n".join(body).rstrip().splitlines()
    if limit and len(body) > limit:
        print("\n".join(body[:limit]))
        print(f"… {len(body) - limit} more lines (--lines 0 for all)")
    else:
        print("\n".join(body))


def cmd_show(a):
    kind, path = doc_for(a.doc)
    lines = read(path).splitlines()
    if not a.section and getattr(a, "full", False):
        emit_full(path, lines)
        return
    if not a.section:
        heads = headings(lines)
        stop = next((i for i, lv, _ in heads if lv == 2), len(lines))
        print("\n".join(lines[:stop]).rstrip())
        print()
        for i, lv, text in heads:
            if lv >= 2:
                print("  " * (lv - 2) + text)
        return
    num = clean_secnum(a.section)
    start, heads = find_section(kind, lines, num)
    if start is None:
        die(f"no §{num} in {os.path.basename(path)}")
    body = block(lines, heads, start)
    targets = pointer_targets(body) if kind == "adr" else []
    if not targets or a.adr:
        print(f"[{os.path.basename(path)}]")
        emit(body, a.lines)
        return
    print(f"[{os.path.basename(path)}] {body[0]}")
    for line in body[1:]:
        if line.strip() and not POINTER.match(line.strip()):
            print(line)
    for ch, sec in targets:
        cpath = chapter_path(ch)
        clines = read(cpath).splitlines()
        cstart, cheads = find_section("chapter", clines, sec)
        print(f"\n--- {os.path.basename(cpath)} §{sec} ---")
        if cstart is None:
            print(f"(no §{sec}: the pointer is stale; run docs/check.sh)")
            continue
        emit(block(clines, cheads, cstart), a.lines)


def emit_full(path, lines):
    """The whole ADR as one document, every pointer replaced by the chapter text it names
    (what a reviewer reads; CI posts it on a pull request that adds an ADR)."""
    heads = headings(lines)
    lo, hi = decision_range(heads, len(lines))
    for i, line in enumerate(lines):
        m = POINTER.match(line.strip()) if lo <= i < hi else None
        if not m:
            print(line)
            continue
        for ch, sec in pointer_targets([line]):
            cpath = chapter_path(ch)
            clines = read(cpath).splitlines()
            cstart, cheads = find_section("chapter", clines, sec)
            print(f"> *From {ch} §{sec} ({os.path.basename(cpath)}):*")
            if cstart is None:
                print("> (no such section: the pointer is stale; run docs/check.sh)\n")
                continue
            for cl in block(clines, cheads, cstart)[1:]:
                print(f"> {cl}" if cl.strip() else ">")
            print()


def cmd_outline(a):
    kind, path = doc_for(a.doc)
    print(f"[{os.path.basename(path)}]")
    for i, lv, text in headings(read(path).splitlines()):
        if lv >= 2:
            print("  " * (lv - 2) + text)


def count(text, phrase):
    return len(re.findall(re.escape(phrase), text, re.I))


def cmd_where(a):
    phrase = " ".join(a.phrase)
    rows = []
    for path in sorted(glob.glob(os.path.join(ARCH, "[0-9][0-9]-*.md"))):
        lines = read(path).splitlines()
        heads = headings(lines)
        ch = os.path.basename(path)[:2]
        # Count each line once, in the deepest section that holds it.
        for k, (i, lv, text) in enumerate(heads):
            if lv < 2:
                continue
            end = heads[k + 1][0] if k + 1 < len(heads) else len(lines)
            n = count("\n".join(lines[i:end]), phrase)
            if n:
                rows.append((count(text, phrase) > 0, n, f"{ch} {text}"))
    rows.sort(key=lambda r: (not r[0], -r[1]))
    print(f"Chapter sections mentioning {phrase!r} ({len(rows)}; heading matches first):")
    for in_head, n, label in rows[: a.limit]:
        print(f"  {n:4}  {label}{'  *' if in_head else ''}")
    adrs = []
    for path in sorted(glob.glob(os.path.join(DEC, "[0-9][0-9][0-9][0-9]-*.md"))):
        text = read(path)
        n = count(text, phrase)
        if n:
            title = text.splitlines()[0].lstrip("# ").strip()
            in_title = count(title, phrase) > 0
            adrs.append((in_title, n, title))
    adrs.sort(key=lambda r: (not r[0], -r[1]))
    print(f"\nADRs mentioning it ({len(adrs)}; title matches first):")
    for in_title, n, title in adrs[: a.limit]:
        print(f"  {n:4}  {title}{'  *' if in_title else ''}")
    regs = []
    for path in sorted(glob.glob(os.path.join(REG, "*"))):
        n = count(read(path), phrase)
        if n:
            regs.append((n, os.path.basename(path)))
    if regs:
        print("\nRegistry files:")
        for n, name in sorted(regs, key=lambda r: -r[0])[: a.limit]:
            print(f"  {n:4}  {name}")


def h2(lines, title):
    heads = headings(lines)
    for i, lv, text in heads:
        if lv == 2 and text == title:
            return block(lines, heads, i)
    return []


def cmd_log(a):
    kind, path = doc_for(a.doc)
    if kind != "adr":
        die("chapters have no Amendment log; their sections cite the ADRs")
    lines = read(path).splitlines()
    log = h2(lines, "Amendment log")
    if not log:
        print(f"{os.path.basename(path)} has no Amendment log")
        return
    if not a.entry:
        print(f"[{os.path.basename(path)}]")
        print("\n".join(log).rstrip())
        return
    entry = a.entry.upper()
    heads = headings(log)
    for i, lv, text in heads:
        if lv == 3 and re.match(rf"^{re.escape(entry)}\.", text):
            print(f"[{os.path.basename(path)}]")
            print("\n".join(block(log, heads, i)).rstrip())
            return
    die(f"no {entry} in {os.path.basename(path)}'s log")


def cmd_questions(a):
    kind, path = doc_for(a.doc)
    if kind != "adr":
        die("chapters carry no open questions; ask the ADRs they cite")
    q = h2(read(path).splitlines(), "Open questions")
    print(f"[{os.path.basename(path)}]")
    print("\n".join(q[1:]).strip() or "(none)")


DOC_LINK = re.compile(r"\[([^\]]+)\]\((?!https?:)[^)\s]*\.(?:md|toml|json)(?:#[^)\s]*)?\)")


class Compact:
    """stdout filter that prints links to other docs as their text alone."""

    def __init__(self, stream):
        self.stream = stream

    def write(self, s):
        return self.stream.write(DOC_LINK.sub(r"\1", s))

    def flush(self):
        self.stream.flush()


def cmd_code(a):
    import importlib.util
    spec = importlib.util.spec_from_file_location("coverage", os.path.join(ROOT, "tools", "coverage.py"))
    cov = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(cov)
    cov.main([a.doc] + ([a.section] if a.section else []))


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--raw", action="store_true", help="keep markdown links to other docs")
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("show", help="a section, following pointers; or a header and outline")
    s.add_argument("doc")
    s.add_argument("section", nargs="?")
    s.add_argument("--adr", action="store_true", help="don't follow a pointer")
    s.add_argument("--full", action="store_true", help="the whole ADR with every pointer followed (no section)")
    s.add_argument("--lines", type=int, default=0, help="print at most N lines of each section (0: all)")
    s.set_defaults(fn=cmd_show)
    o = sub.add_parser("outline", help="headings of an ADR or chapter")
    o.add_argument("doc")
    o.set_defaults(fn=cmd_outline)
    w = sub.add_parser("where", help="where a phrase is discussed")
    w.add_argument("phrase", nargs="+")
    w.add_argument("--limit", type=int, default=12)
    w.set_defaults(fn=cmd_where)
    lg = sub.add_parser("log", help="an ADR's Amendment log, or one entry")
    lg.add_argument("doc")
    lg.add_argument("entry", nargs="?")
    lg.set_defaults(fn=cmd_log)
    qu = sub.add_parser("questions", help="an ADR's open questions")
    qu.add_argument("doc")
    qu.set_defaults(fn=cmd_questions)
    co = sub.add_parser("code", help="code citing an ADR or chapter section (tools/coverage.py)")
    co.add_argument("doc")
    co.add_argument("section", nargs="?")
    co.set_defaults(fn=cmd_code)
    # Accept "0013§7" and "0013 §7" alike.
    argv = []
    for arg in sys.argv[1:]:
        m = re.fullmatch(r"(\d{1,4})§([\d.]+)", arg)
        argv += [m.group(1), m.group(2)] if m else [arg]
    a = p.parse_args(argv)
    if not a.raw:
        sys.stdout = Compact(sys.stdout)
    a.fn(a)


if __name__ == "__main__":
    main()
