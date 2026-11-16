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
  adr.py log 0083 A6 --diff  what an entry changed, from git; --before for the text it replaced
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


def log_entry(lines, entry):
    """The lines of one Amendment log entry ('A6'), or None."""
    log = h2(lines, "Amendment log")
    heads = headings(log)
    for i, lv, text in heads:
        if lv == 3 and re.match(rf"^{re.escape(entry)}\.", text):
            return block(log, heads, i)
    return None


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
        if a.diff or a.before:
            die("--diff and --before need an entry: `log 0083 A6 --diff`")
        print(f"[{os.path.basename(path)}]")
        print("\n".join(log).rstrip())
        return
    entry = a.entry.upper()
    body = log_entry(lines, entry)
    if body is None:
        die(f"no {entry} in {os.path.basename(path)}'s log")
    if a.diff or a.before:
        emit_history(path, entry, body, before_only=a.before)
        return
    print(f"[{os.path.basename(path)}]")
    print("\n".join(body).rstrip())


# --- What an entry replaced, read from git (24 §6) ---------------------------------------
#
# An entry is appended in the same change as the text it describes, so the commit that added
# its `### An.` heading is the change, and that commit's first parent holds the text as it
# stood. Nothing is stored: the commit is found each time, so a rebase or a later commit of
# uncommitted work changes nothing.


def git(*args, check=True):
    import subprocess
    r = subprocess.run(["git", "-C", ROOT, *args], capture_output=True, text=True)
    if check and r.returncode:
        die(f"git {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout if r.returncode == 0 else None


def entry_commit(path, entry):
    """The commit that added the entry's heading, or None (not committed yet). The commit on the
    branch it was written on is preferred to the merge that brought it in, because one pull
    request often appends several entries to the same section, and the merge's parent holds the
    text as it stood before all of them."""
    rel = os.path.relpath(path, ROOT)
    for scope in ([], ["--first-parent"]):
        out = git("log", *scope, "--reverse", "--format=%H", "-G", rf"^### {entry}\. ", "--", rel)
        if out.split():
            return out.split()[0]
    return None


def file_at(rev, kind, num):
    """(path, text) of ADR `num` or chapter `num` as it stood at `rev`, or (None, None)."""
    folder = "docs/decisions" if kind == "adr" else "docs/architecture"
    stem = num if kind == "adr" else f"{int(num):02d}"
    names = git("ls-tree", "--name-only", f"{rev}:{folder}", check=False) or ""
    hit = next((n for n in names.split("\n") if n.startswith(stem + "-") and n.endswith(".md")), None)
    if not hit:
        return None, None
    return f"{folder}/{hit}", git("show", f"{rev}:{folder}/{hit}")


def section_at(rev, kind, num, sec):
    """The lines of `sec` in a document as it stood at `rev`, or None."""
    if rev is None:
        return None
    _, text = file_at(rev, kind, num)
    if text is None:
        return None
    lines = text.splitlines()
    if not re.match(r"\d", sec):  # a named section: Context, Consequences, Open questions…
        return h2(lines, sec) or None
    start, heads = find_section(kind, lines, sec)
    return block(lines, heads, start) if start is not None else None


REPLACED = re.compile(r"^Replaced text ?(?:\((.*)\)|:\s*(.*?))\s*:?\s*$")


def replaced_cites(adr, body):
    """[(kind, num, section), …] named by the entry's 'Replaced text' lines, in order. A bare
    '§4' names this ADR's §4 as it stood (entries older than the chapters)."""
    out = []
    for line in body:
        m = REPLACED.match(line.strip())
        if not m:
            continue
        cite = m.group(1) or m.group(2) or ""
        for seg in cite.split(";"):
            doc = ("adr", adr)
            # A section named rather than numbered (Consequences; the old "Changes to other ADRs").
            named = re.match(r"\s*([A-Z][A-Za-z ]+?)(?: of this ADR)?\s*(?:,|$)", seg)
            if named and "§" not in seg and not re.search(r"\[\d", seg):
                out.append(("adr", adr, named.group(1)))
                continue
            # A document is a link ([03](…)) or a bare number before a section sign (22 §2.1).
            for t in re.finditer(r"\[(\d{2}|\d{4})\]\([^)]*\)|\b(\d{2}|\d{4})(?= §)|§(\d+(?:\.\d+)*)", seg):
                d = t.group(1) or t.group(2)
                if d:
                    doc = ("chapter", d) if len(d) == 2 else ("adr", d)
                else:
                    out.append((*doc, t.group(3)))
    seen = set()
    return [c for c in out if not (c in seen or seen.add(c))]


def change_sections(body):
    """[(verb, section), …] of this ADR from the entry's Change line (`§1–§11` is a range)."""
    line = next((l for l in body if l.startswith("- **Change:**")), "")
    out = []
    for clause in line.split(";"):
        verb = re.search(r"\b(amends|extends|supersedes|corrects|relocates|consolidates)\b", clause)
        if not verb:
            continue
        for lo, hi in re.findall(r"§(\d+(?:\.\d+)*)(?:\s*[–-]\s*§(\d+))?", clause):
            for sec in ([str(n) for n in range(int(lo), int(hi) + 1)] if hi else [lo]):
                out.append((verb.group(1), sec))
    return out


def entry_history(path, entry, body):
    """{'commit', 'parent', 'cites', 'sections': [(label, before|None, after|None)]} for an entry,
    or None if the entry is not in a commit yet. The sections are those its Replaced text names;
    failing that, those its Change line names, followed through their pointers to the chapter
    sections that held the text at the time (except for a move, where the ADR's own text is
    what moved)."""
    adr = os.path.basename(path)[:4]
    commit = entry_commit(path, entry)
    if commit is None:
        return None
    parent = (git("rev-parse", "--verify", "--quiet", f"{commit}^1", check=False) or "").strip() or None
    cites = replaced_cites(adr, body)
    found = []

    def add(kind, num, sec, note=""):
        before = section_at(parent, kind, num, sec)
        after = section_at(commit, kind, num, sec)
        if before is None and after is None:
            return
        if any(f[0].startswith(f"{num} §{sec}") for f in found):
            return
        label = f"{num} §{sec}" if re.match(r"\d", sec) else f"{num} {sec}"
        found.append((label + note, before, after))

    for kind, num, sec in cites:
        if section_at(parent, kind, num, sec) is not None:
            add(kind, num, sec)
    if not found:
        # Entries written before the chapters: the cited chapter section did not exist yet.
        note = " (the cited text stood here then)" if cites else ""
        for verb, sec in change_sections(body):
            if verb in ("relocates", "consolidates"):
                add("adr", adr, sec, note)
                continue
            targets = pointer_targets(section_at(commit, "adr", adr, sec) or [])
            if targets:
                for ch, csec in targets:
                    before, after = section_at(parent, "chapter", ch, csec), section_at(commit, "chapter", ch, csec)
                    if before != after:
                        add("chapter", ch, csec)
            else:
                add("adr", adr, sec, note)
    # An entry that names its chapter text ("Chapter text: [16](…) §2.2") points there itself.
    summary = next((l for l in body if l.startswith("- **Summary:**")), "")
    named = summary.split("Chapter text:", 1)[1] if "Chapter text:" in summary else ""
    doc = None
    for t in re.finditer(r"\[(\d{2})\]\([^)]*\)|\b(\d{2})(?= §)|§(\d+(?:\.\d+)*)", named):
        if t.group(1) or t.group(2):
            doc = t.group(1) or t.group(2)
        elif doc and section_at(parent, "chapter", doc, t.group(3)) != section_at(commit, "chapter", doc, t.group(3)):
            add("chapter", doc, t.group(3))
    return {"commit": commit, "parent": parent, "cites": cites, "sections": found}


def emit_history(path, entry, body, before_only=False):
    import difflib
    adr = os.path.basename(path)[:4]
    h = entry_history(path, entry, body)
    if h is None:
        die(f"{adr} {entry} is not in a commit on this branch yet; `git diff` shows its change")
    subject = git("log", "-1", "--format=%h %ad %s", "--date=short", h["commit"]).strip()
    print(f"[{adr} {entry}] {subject}")
    # The commit may carry more than this entry: several entries written together, or entries
    # numbered later when the log took its present form (0050). Say so rather than imply otherwise.
    date = next((l.split(":**", 1)[1].strip() for l in body if l.startswith("- **Date:**")), "")
    rel = os.path.relpath(path, ROOT)
    added = re.findall(r"(?m)^\+### (A\d+)\. ", git("show", "--format=", "-U0", h["commit"], "--", rel, check=False) or "")
    others = [x for x in added if x != entry]
    if date and date != subject.split()[1]:
        print(f"(the entry is dated {date}; this commit gave it its present heading, so the diff shows the text as it stood then)")
    if others:
        shown = ", ".join(others[:8]) + (f" and {len(others) - 8} more" if len(others) > 8 else "")
        print(f"(this commit also added {shown} to {adr}; their changes may show here too)")
    if h["parent"] is None:
        print("(the first commit: the entry predates the repository, so its quoted text is the record)")
        return
    if not h["sections"]:
        print("(no cited section found at the parent; `git show --stat " + h["commit"][:9] + " -- docs` lists what changed)")
        return
    for label, before, after in h["sections"]:
        if before_only:
            print(f"\n--- {label}, as it stood ---")
            print("\n".join(before).rstrip() if before is not None else "(the section did not exist yet)")
            continue
        print(f"\n--- {label} ---")
        before = before or []
        if after is None:
            print("(the section is gone after this commit; --before shows it)")
            continue
        changed = False
        sm = difflib.SequenceMatcher(None, before, after, autojunk=False)
        for op, i1, i2, j1, j2 in sm.get_opcodes():
            if op == "equal":
                continue
            changed = True
            # A paragraph is one line in these docs, so a changed paragraph is shown word by word.
            if op == "replace" and i2 - i1 == j2 - j1:
                for old, new in zip(before[i1:i2], after[j1:j2]):
                    print(word_diff(old, new))
            else:
                for line in before[i1:i2]:
                    if line.strip():
                        print(f"[-{line}-]")
                for line in after[j1:j2]:
                    if line.strip():
                        print(f"{{+{line}+}}")
        if not changed:
            print(f"(no change to {label} in this commit)")


def word_diff(old, new, context=12):
    """One changed line as `[-removed-]{+added+}`, git --word-diff style, with long unchanged
    runs elided to `context` words either side."""
    import difflib
    a, b = re.findall(r"\s*\S+", old), re.findall(r"\s*\S+", new)
    out = []
    ops = difflib.SequenceMatcher(None, a, b, autojunk=False).get_opcodes()
    for k, (op, i1, i2, j1, j2) in enumerate(ops):
        if op == "equal":
            run = a[i1:i2]
            if len(run) > 2 * context:
                head = "".join(run[:context]) if k > 0 else ""
                tail = "".join(run[-context:]) if k < len(ops) - 1 else ""
                run = [head, " …", tail]
            out.append("".join(run))
            continue
        lead = " " if (a[i1:i2] or b[j1:j2])[0][:1].isspace() else ""
        if i2 > i1:
            out.append(lead + "[-" + "".join(a[i1:i2]).strip() + "-]")
        if j2 > j1:
            out.append(("" if i2 > i1 else lead) + "{+" + "".join(b[j1:j2]).strip() + "+}")
    return "".join(out).strip()


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
    lg.add_argument("--diff", action="store_true", help="what the entry changed, section by section, from git")
    lg.add_argument("--before", action="store_true", help="the sections the entry replaced, as they stood, from git")
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
