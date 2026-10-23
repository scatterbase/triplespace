#!/usr/bin/env python3
"""Which ADR sections the code cites, as evidence of what is built.

  coverage.py                 one line per ADR: its Decision sections, and how many the code cites
  coverage.py 0013            per section: where the code cites it
  coverage.py 0013 §7         every citing line (file:line and the comment)
  coverage.py 03 §6.1         the same for a chapter section: cited directly, or through its Sources
  coverage.py --stale         citations of sections that do not exist (renumbered, typos, stubs)
  coverage.py --candidates    ADRs whose every Decision section is cited by code and by a test

The evidence is the citations already in comments (`0013 §7`, `0090 §5–6`, `03 §4.5`), read from
crates/, xtask/, ui/src, ui/e2e and tools/. A citation says the code is *about* a section, not that
the section is built in full; read the lines (`coverage.py NNNN §N`) before relying on it. Each is
classed by where it is:

  src    library and binary code, migrations
  test   tests/ directories, `#[cfg(test)]` modules, ui/e2e, tools/ (the client checks)
  not    the sentence citing it says the thing is not built yet ("not yet built", "deferred",
         "TODO", "not implemented", …); listed apart and never counted as evidence

A section's own citations include its subsections' (§7 counts §7.1). Trailer sections (Crates,
API, UI, Storage, …, as map.py classes them) are shown but left out of the counts, since the
code cites a trailer's subject under the home section instead. Superseded stubs are left out.
A chapter citation (`03 §6.1`) counts for every ADR section that chapter section's *Sources* line
names: weaker evidence, marked "via chapter".

`adr.py code NNNN §N` prints the same citing lines.
"""
import ast
import collections
import glob
import os
import re
import signal
import sys

if hasattr(signal, "SIGPIPE"):
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEC = os.path.join(ROOT, "docs", "decisions")
ARCH = os.path.join(ROOT, "docs", "architecture")
SCAN = ["crates", "xtask", "ui/src", "ui/e2e", "tools"]
SKIP_DIRS = {"node_modules", "fixtures", "target", "dist", "__pycache__", "snapshots", "vectors"}
EXTS = (".rs", ".sql", ".ts", ".js", ".mjs", ".vue", ".py", ".sh", ".toml", ".css", ".less")
SELF = {os.path.join("tools", "coverage.py"), os.path.join("tools", "adr.py")}

NOT_BUILT = re.compile(
    r"\b(?:not (?:yet )?(?:built|implemented|supported|served|written|wired)|"
    r"(?:is|are)n't (?:yet )?(?:built|implemented|supported|served|written)|"
    r"unimplemented|deferred|TODO|FIXME|XXX|left for later|not done yet)\b", re.I)
SEC = r"\d+(?:\.\d+)*"
ADR_ANCHOR = re.compile(r"(?<![\w./-])(0\d{3})(?= §)")
CH_ANCHOR = re.compile(r"(?<![\w./§-])([0-2]\d)(?= §)")
# After an anchor: "§2", "§2.1", "§2–4", then more joined by ", " / " and " / ", and " / "–".
SEC_RUN = re.compile(rf" §({SEC})(?:\s*[–-]\s*§?({SEC}))?")
SEC_MORE = re.compile(rf"(?:,\s*|\s+and\s+|,\s+and\s+)§({SEC})(?:\s*[–-]\s*§?({SEC}))?")


# --- documents --------------------------------------------------------------------------------

def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def headings(lines):
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


def trailer_rules():
    """map.py's TRAILER list, read without running map.py."""
    tree = ast.parse(read(os.path.join(ARCH, "map.py")))
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(getattr(t, "id", "") == "TRAILER" for t in node.targets):
            return [re.compile(rx) for rx, _ in ast.literal_eval(node.value)]
    return []


TRAILERS = trailer_rules()


class Section:
    def __init__(self, doc, num, title, body):
        self.doc, self.num, self.title, self.body = doc, num, title, body
        self.stub = bool(re.search(r"^\*Superseded by ", body, re.M)) and len(body.strip().splitlines()) <= 3
        bare = re.sub(r"\s*\((?:amends|extends|supersedes|corrects|settles|uses)\b.*\)$", "", title)
        self.trailer = any(rx.search(bare) for rx in TRAILERS)


def adr_docs():
    docs = {}
    for path in sorted(glob.glob(os.path.join(DEC, "[0-9][0-9][0-9][0-9]-*.md"))):
        num = os.path.basename(path)[:4]
        text = read(path)
        lines = text.splitlines()
        heads = headings(lines)
        lo = next((i for i, lv, t in heads if lv == 2 and (t == "Decision" or t.startswith("Part "))), None)
        if lo is None:
            continue
        # Some Decisions are split into "## Part A — …" headings, so end only at the parts that follow.
        hi = next((i for i, lv, t in heads if lv == 2 and i > lo and t in ("Alternatives considered", "Consequences", "Open questions", "Changes to other ADRs", "References", "Amendment log")), len(lines))
        secs = collections.OrderedDict()
        dh = [(i, lv, t) for i, lv, t in heads if lo < i < hi and lv >= 3]
        # Within a Part, a section's end is the next heading at its level or above, Parts included.
        parts = [i for i, lv, _ in heads if lo < i < hi and lv == 2]
        for k, (i, lv, t) in enumerate(dh):
            m = re.match(rf"^({SEC})\.?\s+(.*)$", t)
            if not m:
                continue
            end = min([j for j, lv2, _ in dh[k + 1:] if lv2 <= lv] + [j for j in parts if j > i] + [hi])
            secs[m.group(1)] = Section(num, m.group(1), m.group(2), "\n".join(lines[i + 1:end]))
        status = re.search(r"^- \*\*Status:\*\* (.*)$", text, re.M)
        docs[num] = {"title": lines[0].lstrip("# ").split(". ", 1)[-1], "status": status.group(1) if status else "?",
                     "sections": secs}
    return docs


def chapter_sources():
    """{('03', '6.1'): {('0013', '7'), …}} from each chapter section's *Sources:* line."""
    out = {}
    for path in sorted(glob.glob(os.path.join(ARCH, "[0-9][0-9]-*.md"))):
        ch = os.path.basename(path)[:2]
        lines = read(path).splitlines()
        for i, lv, t in headings(lines):
            m = re.match(rf"^({SEC})\.?\s", t)
            if not m:
                continue
            src = next((l for l in lines[i + 1:i + 4] if l.startswith("*Sources:")), "")
            cites = set()
            for adr, run in re.findall(r"\[(0\d{3})\]\([^)]*\)((?: §[\d.]+(?:–[\d.]+)?,?)+)", src):
                for s in re.findall(rf"§({SEC})", run):
                    cites.add((adr, s))
            out[(ch, m.group(1))] = cites
    return out


# --- code -------------------------------------------------------------------------------------

def code_files():
    for root in SCAN:
        for dp, dn, fn in os.walk(os.path.join(ROOT, root)):
            dn[:] = sorted(d for d in dn if d not in SKIP_DIRS)
            for f in sorted(fn):
                if f.endswith(EXTS):
                    p = os.path.join(dp, f)
                    rel = os.path.relpath(p, ROOT)
                    if rel not in SELF:
                        yield rel, p


def file_kind(rel):
    parts = rel.split(os.sep)
    if "tests" in parts or "e2e" in parts or parts[0] == "tools" or rel.endswith(("_test.rs", ".test.ts", ".test.js", ".spec.ts")):
        return "test"
    return "src"


def crate_of(rel):
    parts = rel.split(os.sep)
    if parts[0] == "crates" and len(parts) > 1:
        return parts[1]
    return parts[0] if parts[0] != "ui" else "ui/" + parts[1]


def expand(a, b):
    """'2', '4' -> ['2', '3', '4']; dotted ranges keep their ends only."""
    if not b:
        return [a]
    if re.fullmatch(r"\d+", a) and re.fullmatch(r"\d+", b) and int(b) >= int(a) and int(b) - int(a) < 40:
        return [str(n) for n in range(int(a), int(b) + 1)]
    return [a, b]


def sections_after(text, pos):
    """Section numbers that follow an anchor ending at `pos`."""
    m = SEC_RUN.match(text, pos)
    if not m:
        return []
    out = expand(m.group(1), m.group(2))
    pos = m.end()
    while True:
        m = SEC_MORE.match(text, pos)
        if not m:
            return out
        out += expand(m.group(1), m.group(2))
        pos = m.end()


COMMENT = re.compile(r"^\s*(?://[/!]?|#|--|\*|/\*\*?)\s?")


def paragraph(lines, i):
    """The comment paragraph around line i, as one string, and the line's offset in it."""
    if not COMMENT.match(lines[i]):
        return lines[i], 0
    lo = i
    while lo > 0 and COMMENT.match(lines[lo - 1]) and COMMENT.sub("", lines[lo - 1]).strip():
        lo -= 1
    hi = i
    while hi + 1 < len(lines) and COMMENT.match(lines[hi + 1]) and COMMENT.sub("", lines[hi + 1]).strip():
        hi += 1
    parts = [COMMENT.sub("", l).strip() for l in lines[lo:hi + 1]]
    offset = sum(len(p) + 1 for p in parts[: i - lo])
    return " ".join(parts), offset


def sentence_at(text, pos):
    starts = [m.end() for m in re.finditer(r"[.;:!?](?:\s|$)", text[:pos])]
    start = starts[-1] if starts else 0
    m = re.search(r"[.!?](?:\s|$)", text[pos:])
    end = pos + m.end() if m else len(text)
    return text[start:end]


Cite = collections.namedtuple("Cite", "doc sec rel line kind crate text via")


def scan():
    """Every citation in the code, as Cite records (doc is an ADR number or a chapter number)."""
    out = []
    for rel, path in code_files():
        try:
            lines = read(path).splitlines()
        except (UnicodeDecodeError, OSError):
            continue
        base = file_kind(rel)
        in_test = False
        for i, line in enumerate(lines):
            if rel.endswith(".rs") and line.strip().startswith("#[cfg(test)]"):
                in_test = True
            if "§" not in line:
                continue
            para = None
            for anchor, is_adr in ((ADR_ANCHOR, True), (CH_ANCHOR, False)):
                for m in anchor.finditer(line):
                    secs = sections_after(line, m.end())
                    if not secs:
                        continue
                    if para is None:
                        para, off = paragraph(lines, i)
                    stripped = COMMENT.sub("", line).strip() if COMMENT.match(line) else line
                    pos = off + max(0, m.start() - (len(line) - len(stripped)))
                    kind = "test" if (base == "test" or in_test) else "src"
                    if NOT_BUILT.search(sentence_at(para, min(pos, len(para)))):
                        kind = "not"
                    for s in secs:
                        out.append(Cite(m.group(1), s, rel, i + 1, kind, crate_of(rel), line.strip(), None))
    return out


# --- analysis ---------------------------------------------------------------------------------

def index(docs, cites, ch_sources):
    """{(adr, sec): [Cite]} with chapter citations attributed through Sources lines, and
    subsection citations rolled up to their parents. Also the stale citations."""
    by = collections.defaultdict(list)
    stale = []
    for c in cites:
        if len(c.doc) == 4:
            d = docs.get(c.doc)
            if not d:
                stale.append(c)
                continue
            if c.sec not in d["sections"]:
                # `0057 §1.4` for the fourth numbered item of §1, which 0050 §11 writes `§1 rule 4`:
                # count it for the nearest existing parent, and report it.
                parts = c.sec.split(".")
                parent = next((".".join(parts[:k]) for k in range(len(parts) - 1, 0, -1)
                               if ".".join(parts[:k]) in d["sections"]), None)
                stale.append(c._replace(via=f"§{parent}" if parent else None))
                if parent is None:
                    continue
                c = c._replace(sec=parent)
            by[(c.doc, c.sec)].append(c)
        else:
            srcs = ch_sources.get((c.doc, c.sec))
            if srcs is None:
                stale.append(c)
                continue
            for adr, s in srcs:
                if adr in docs and s in docs[adr]["sections"]:
                    by[(adr, s)].append(c._replace(via=f"{c.doc} §{c.sec}"))
    rolled = collections.defaultdict(list)
    for (adr, s), cs in by.items():
        parts = s.split(".")
        for k in range(1, len(parts) + 1):
            p = ".".join(parts[:k])
            if p in docs[adr]["sections"]:
                rolled[(adr, p)].extend(cs)
    return rolled, stale


def grade(cs):
    direct = [c for c in cs if c.via is None]
    src = any(c.kind == "src" for c in direct)
    test = any(c.kind == "test" for c in direct)
    via = any(c.via and c.kind in ("src", "test") for c in cs)
    if src and test:
        return "built"
    if src:
        return "src"
    if test:
        return "tests"
    if via:
        return "via"
    if any(c.kind == "not" for c in cs):
        return "not"
    return "—"


def top_sections(d):
    return [s for n, s in d["sections"].items() if "." not in n and not s.stub and not s.trailer]


def summary(docs, rolled):
    print(f"{'ADR':4}  {'secs':>4} {'built':>5} {'src':>4} {'tests':>5} {'via':>4} {'not':>4} {'none':>4}  title")
    totals = collections.Counter()
    for num, d in docs.items():
        tops = top_sections(d)
        g = collections.Counter(grade(rolled.get((num, s.num), [])) for s in tops)
        totals.update(g)
        totals["secs"] += len(tops)
        print(f"{num}  {len(tops):4} {g['built']:5} {g['src']:4} {g['tests']:5} {g['via']:4} {g['not']:4} {g['—']:4}  "
              f"{d['title'][:60]}")
    print(f"\n{totals['secs']} top-level Decision sections (trailers and stubs left out): "
          f"{totals['built']} cited by code and tests, {totals['src']} by code only, {totals['tests']} by tests only, "
          f"{totals['via']} only through a chapter citation, {totals['not']} only as not built, {totals['—']} not cited.")
    print("built = cited in src and tests; src/tests = in one only; via = through a chapter section's Sources; "
          "not = cited only as not built yet.")


def per_adr(docs, rolled, num):
    d = docs[num]
    print(f"{num}. {d['title']}  [{d['status']}]")
    for n, s in d["sections"].items():
        cs = rolled.get((num, n), [])
        tag = "stub" if s.stub else ("trailer" if s.trailer else grade(cs))
        crates = collections.Counter(c.crate for c in cs if c.kind != "not")
        nots = sum(1 for c in cs if c.kind == "not")
        where = ", ".join(f"{k}×{v}" if v > 1 else k for k, v in crates.most_common(5))
        more = f" +{len(crates) - 5}" if len(crates) > 5 else ""
        nb = f"  (not built: {nots})" if nots else ""
        print(f"{'  ' * n.count('.')}§{n:<6} {tag:7} {s.title[:58]:58} {where}{more}{nb}")


def lines_for(docs, rolled, cites, ch_sources, doc, sec):
    if len(doc) == 4:
        cs = rolled.get((doc, sec), [])
        title = docs[doc]["sections"][sec].title if doc in docs and sec in docs[doc]["sections"] else "?"
        print(f"{doc} §{sec} {title}: {grade(cs)}")
    else:
        direct = [c for c in cites if c.doc == doc and (c.sec == sec or c.sec.startswith(sec + "."))]
        srcs = sorted(ch_sources.get((doc, sec), set()))
        cs = direct + [c for a, s in srcs for c in rolled.get((a, s), []) if c not in direct]
        print(f"{doc} §{sec}: cited directly {len(direct)}×; Sources: {', '.join(f'{a} §{s}' for a, s in srcs) or '—'}")
    for c in sorted(set(cs), key=lambda c: (c.via is not None, c.kind != "src", c.kind != "test", c.rel, c.line)):
        via = f" (via {c.via})" if c.via else ""
        print(f"  {c.kind:4} {c.rel}:{c.line}{via}  {c.text[:120]}")


def candidates(docs, rolled):
    print("ADRs whose every top-level Decision section is cited by code and by a test, with no")
    print("not-built mention. Evidence for Accepted, not proof: read `coverage.py NNNN` first.\n")
    for num, d in docs.items():
        tops = top_sections(d)
        if tops and all(grade(rolled.get((num, s.num), [])) == "built" for s in tops) \
                and not any(c.kind == "not" for s in tops for c in rolled.get((num, s.num), [])):
            print(f"  {num}  [{d['status']}]  {d['title']}")


def main(argv):
    docs = adr_docs()
    ch_sources = chapter_sources()
    cites = scan()
    rolled, stale = index(docs, cites, ch_sources)
    args = [a for a in argv if not a.startswith("--")]
    joined = []
    for a in args:
        m = re.fullmatch(rf"(\d{{1,4}})§({SEC})", a)
        joined += [m.group(1), m.group(2)] if m else [a.replace("§", "")]
    if "--stale" in argv:
        print(f"{len(stale)} citations name a section that does not exist. Those marked → were counted for")
        print("the parent section: usually a numbered item of it, which 0050 §11 writes `§N rule K`.")
        for c in sorted(stale, key=lambda c: (c.doc, c.sec, c.rel)):
            to = f" → {c.via}" if c.via else ""
            print(f"  {c.doc} §{c.sec:6}{to:8} {c.rel}:{c.line}")
        return
    if "--candidates" in argv:
        candidates(docs, rolled)
        return
    if not joined:
        summary(docs, rolled)
        return
    doc = joined[0]
    if re.fullmatch(r"\d{1,2}", doc):
        doc = f"{int(doc):02d}"
    elif not (re.fullmatch(r"\d{4}", doc) and doc in docs):
        sys.exit(f"coverage.py: no ADR {doc}")
    if len(joined) == 1:
        if len(doc) != 4:
            sys.exit("coverage.py: name a chapter section, e.g. `03 §6.1`")
        per_adr(docs, rolled, doc)
    else:
        lines_for(docs, rolled, cites, ch_sources, doc, joined[1])


if __name__ == "__main__":
    main(sys.argv[1:])
