#!/usr/bin/env python3
"""What the code says it has built, section by section (24 §15).

  coverage.py                 one line per ADR: sections done, partial, untested, and its status
  coverage.py 0013            per section: its status and where the code cites it
  coverage.py 0013 §7         every citing line (file:line and the comment)
  coverage.py 03 §6.1         the same for a chapter section: cited directly, or through its Sources
  coverage.py --todo          the work list: not-built comments, then every section not done and what it needs
  coverage.py --status        the computed status of every ADR, as JSON (check_adrs.py reads it)
  coverage.py --check         exit 1 on a malformed claim (see below); check.sh runs this
  coverage.py --stale         citations of sections that do not exist (renumbered, typos, stubs)

Two kinds of citation, both in comments, read from crates/, xtask/, ui/src, ui/e2e and tools/:

  about    `0013 §7`, `0090 §5–6`, `03 §4.5`: the code is about the section. Evidence of work,
           not of completion.
  claim    `implements 0013 §7 (A41)`: the section is built in full, as it stood at log entry
           A41. Written once, at the code that completes it. `implements 0013 §7.3 (A41)` or
           `implements 0013 §7 rule 2` claims a part. A section that has never been changed
           (no provenance line) is claimed without an entry; one that has needs the entry, so
           that a later change (A44) shows the claim as stale rather than silently wrong. A claim
           on a chapter section (`implements 03 §6.1`) claims every ADR section its Sources name.

Each citation is classed by where it is: src (library and binary code, migrations), test
(tests/, `#[cfg(test)]`, ui/e2e, tools/), or not (the sentence says the thing is not built yet:
"not yet built", "deferred", "TODO", …; never counted as evidence).

A section's status, from its own citations and its subsections':
  done       a src claim covers the whole section (itself, or all its subsections), a test cites
             it, and no log entry changed it after the claim
  amended    done, except that entries changed it after the claim; names them
  untested   claimed in src, no test cites it
  partial    claimed in part, or only cited as about
  not        cited only as not built yet
  none       not cited
An ADR's status follows (24 §2.2): Accepted when every top-level section is done; Accepted with
proposed amendment (A44, …) when every one is done or amended; Proposed otherwise. Trailer
sections (Crates, API, UI, Storage, …, 24 §10.3) and superseded stubs are left out: a trailer's
subject is claimed under its home section, and the catalogues' own Built columns show the rest.

`adr.py code NNNN §N` prints the same citing lines.
"""
import json
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
CLAIM = re.compile(r"\bimplements\s*$")                       # the word before the anchor
RULE = re.compile(r"\s+rule\s+(\d+)")
ENTRY = re.compile(r"\s*\(A(\d+)\)")
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


# The cross-cutting trailer sections most ADRs end with (24 §10.3): shown but left out of the counts.
TRAILERS = [re.compile(rx) for rx in (
    r"^Crates\b", r"^Scatterbase\b", r"^Conformance\b", r"^Testing\b",
    r"^Storage, API and RDF", r"^Storage, operations and API", r"^API, operations and storage",
    r"^Pages, UI and API", r"^UI and API", r"^API and (UI|operations)", r"^(Action API|REST|API)\b",
    r"^Special pages, API and search", r"^Display, logs and RDF",
    r"^(UI|Site UI|Rendering and the UI|Editing and preview)\b",
    r"^Search and filters", r"^Storage\b", r"^Search\b", r"^Terms, search and storage",
    r"^Filters and feeds", r"^Permissions\b", r"^Logs and RDF", r"^RDF\b", r"^IRIs and RDF",
    r"^Log events", r"^Feeds, watch", r"^Rate limits", r"^Limits, rate limits", r"^Tenants\b",
    r"^Federation\b", r"^Registry", r"^Configuration and registry", r"^Settings\b", r"^Reports\b",
    r"^Deployment profiles", r"^Changelog",
)]


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
        # the entries that changed each section, from its provenance line (24 §3.1)
        for s in secs.values():
            m = re.search(r"^\*Changed by ((?:A\d+(?:, )?)+)\.\*", s.body, re.M)
            s.changed = sorted(int(x) for x in re.findall(r"A(\d+)", m.group(1))) if m else []
        docs[num] = {"title": lines[0].lstrip("# ").split(". ", 1)[-1], "status": status.group(1) if status else "?",
                     "sections": secs, "path": path}
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


Cite = collections.namedtuple("Cite", "doc sec rel line kind crate text via claim entry rule")


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
                    claim = bool(CLAIM.search(line[:m.start()]))
                    rule = entry = None
                    if claim:
                        # `implements 0013 §7 rule 2 (A41)`: the rule and the entry follow the sections
                        end = SEC_RUN.match(line, m.end()).end()
                        while (mm := SEC_MORE.match(line, end)):
                            end = mm.end()
                        if (mr := RULE.match(line, end)):
                            rule = int(mr.group(1)); end = mr.end()
                        if (me := ENTRY.match(line, end)):
                            entry = int(me.group(1))
                    if para is None:
                        para, off = paragraph(lines, i)
                    stripped = COMMENT.sub("", line).strip() if COMMENT.match(line) else line
                    pos = off + max(0, m.start() - (len(line) - len(stripped)))
                    kind = "test" if (base == "test" or in_test) else "src"
                    if NOT_BUILT.search(sentence_at(para, min(pos, len(para)))):
                        kind = "not"
                    for s in secs:
                        out.append(Cite(m.group(1), s, rel, i + 1, kind, crate_of(rel), line.strip(), None, claim, entry, rule))
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


def section_status(docs, rolled, adr, num):
    """(status, detail) for one section (24 §15), from its own and its subsections' citations."""
    d = docs[adr]
    sec = d["sections"][num]
    cs = rolled.get((adr, num), [])
    direct_claims = [c for c in cs if c.claim and c.kind == "src" and c.via is None and c.doc == adr]
    via_claims = [c for c in cs if c.claim and c.kind == "src" and c.via is not None]
    whole = [c for c in direct_claims if c.sec == num and c.rule is None] + via_claims
    subs = [n for n in d["sections"] if n.startswith(num + ".") and n.count(".") == num.count(".") + 1
            and not d["sections"][n].stub]
    if not whole and subs:
        covered = all(any(c.sec == n and c.rule is None for c in direct_claims) for n in subs)
        if covered:
            whole = direct_claims
    tested = any(c.kind == "test" for c in cs)
    if whole:
        claimed_entry = max((c.entry or 0) for c in whole)
        later = [a for a in sec.changed if a > claimed_entry]
        if not tested:
            return "untested", ""
        if later:
            return "amended", ", ".join(f"A{a}" for a in later)
        return "done", ""
    if direct_claims:
        return "partial", "claimed in part"
    g = grade(cs)
    if g in ("built", "src", "tests", "via"):
        return "partial", "cited as about only"
    if g == "not":
        return "not", ""
    return "none", ""


def claim_problems(docs, cites):
    """Malformed claims (--check): on a changed section without the entry, or with an entry the
    section's provenance does not name, or on a section that does not exist."""
    out = []
    for c in cites:
        if not c.claim or len(c.doc) != 4:
            continue
        d = docs.get(c.doc)
        s = d["sections"].get(c.sec) if d else None
        if s is None:
            out.append(f"{c.rel}:{c.line}: implements {c.doc} §{c.sec}, which does not exist")
        elif s.changed and c.entry is None:
            out.append(f"{c.rel}:{c.line}: implements {c.doc} §{c.sec} without the entry it is current to; the section was changed by A{s.changed[-1]}")
        elif c.entry is not None and c.entry not in s.changed:
            out.append(f"{c.rel}:{c.line}: implements {c.doc} §{c.sec} (A{c.entry}), but no entry A{c.entry} changed §{c.sec} (provenance: {', '.join(f'A{a}' for a in s.changed) or 'never changed'})")
    return out


MANUAL_STATUS = re.compile(r"^(Superseded by|Withdrawn)")


def adr_status(docs, rolled, num):
    """The computed header status of an ADR, and its section counts."""
    d = docs[num]
    if MANUAL_STATUS.match(d["status"]):
        return d["status"], {}
    tops = top_sections(d)
    st = {s.num: section_status(docs, rolled, num, s.num) for s in tops}
    counts = collections.Counter(v[0] for v in st.values())
    counts["total"] = len(tops)
    if tops and all(v[0] in ("done", "amended") for v in st.values()):
        entries = sorted({int(a[1:]) for v in st.values() if v[0] == "amended" for a in v[1].split(", ")})
        if entries:
            return "Accepted with proposed amendment (" + ", ".join(f"A{a}" for a in entries) + ")", counts
        return "Accepted", counts
    return "Proposed", counts


def status_json(docs, rolled):
    out = {}
    for num in docs:
        status, counts = adr_status(docs, rolled, num)
        out[num] = {"status": status, "done": counts.get("done", 0) + counts.get("amended", 0),
                    "partial": counts.get("partial", 0) + counts.get("untested", 0), "total": counts.get("total", 0)}
    return out


def todo(docs, rolled, cites):
    print("What waits on work (24 §13, §15). What waits on a decision is docs/decisions/OPEN.md.\n")
    nots = sorted({(c.rel, c.line, c.text) for c in cites if c.kind == "not"})
    if nots:
        print(f"Not built yet, as the code says ({len(nots)} comments):")
        for rel, line, text in nots:
            print(f"  {rel}:{line}  {text[:110]}")
        print()
    print("Sections not done, by ADR. What each still needs:\n")
    for num, d in docs.items():
        rows = []
        for s in top_sections(d):
            st, detail = section_status(docs, rolled, num, s.num)
            if st == "done":
                continue
            need = {"amended": f"re-check against {detail} and move the claim's entry",
                    "untested": "a test that cites it", "partial": f"an `implements` claim ({detail})",
                    "not": "building it (cited as not built yet)", "none": "building it (nothing cites it)"}[st]
            rows.append(f"  §{s.num:<5} {st:8} {s.title[:52]:52} {need}")
        if rows:
            print(f"{num}  {d['title']}")
            print("\n".join(rows))


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
    print(f"{'ADR':4}  {'secs':>4} {'done':>4} {'amend':>5} {'untst':>5} {'part':>4} {'not':>4} {'none':>4}  status / title")
    totals = collections.Counter()
    for num, d in docs.items():
        tops = top_sections(d)
        g = collections.Counter(section_status(docs, rolled, num, s.num)[0] for s in tops)
        totals.update(g)
        totals["secs"] += len(tops)
        status, _ = adr_status(docs, rolled, num)
        print(f"{num}  {len(tops):4} {g['done']:4} {g['amended']:5} {g['untested']:5} {g['partial']:4} {g['not']:4} {g['none']:4}  "
              f"[{status[:9]}] {d['title'][:52]}")
    print(f"\n{totals['secs']} top-level Decision sections (trailers and stubs left out): "
          f"{totals['done']} done, {totals['amended']} amended since their claim, {totals['untested']} claimed but untested, "
          f"{totals['partial']} partial (claimed in part or cited as about), {totals['not']} not built, {totals['none']} not cited.")
    print("done = an `implements` claim in src covering the section, a test citing it, no later log entry (24 §15).")


def per_adr(docs, rolled, num):
    d = docs[num]
    print(f"{num}. {d['title']}  [{d['status']}]")
    for n, s in d["sections"].items():
        cs = rolled.get((num, n), [])
        tag = "stub" if s.stub else ("trailer" if s.trailer else section_status(docs, rolled, num, n)[0])
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
        st = section_status(docs, rolled, doc, sec) if sec in docs[doc]["sections"] else ("?", "")
        print(f"{doc} §{sec} {title}: {st[0]}{' (' + st[1] + ')' if st[1] else ''}")
    else:
        direct = [c for c in cites if c.doc == doc and (c.sec == sec or c.sec.startswith(sec + "."))]
        srcs = sorted(ch_sources.get((doc, sec), set()))
        cs = direct + [c for a, s in srcs for c in rolled.get((a, s), []) if c not in direct]
        print(f"{doc} §{sec}: cited directly {len(direct)}×; Sources: {', '.join(f'{a} §{s}' for a, s in srcs) or '—'}")
    for c in sorted(set(cs), key=lambda c: (c.via is not None, c.kind != "src", c.kind != "test", c.rel, c.line)):
        via = f" (via {c.via})" if c.via else ""
        tag = "claim" if c.claim else c.kind
        print(f"  {tag:5} {c.rel}:{c.line}{via}  {c.text[:120]}")


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
    if "--check" in argv:
        probs = claim_problems(docs, cites)
        for p in probs:
            print("   -", p)
        if probs:
            sys.exit(1)
        print(f"ok: {sum(1 for c in cites if c.claim)} implements claims, all well formed")
        return
    if "--status" in argv:
        print(json.dumps(status_json(docs, rolled), indent=1))
        return
    if "--todo" in argv:
        todo(docs, rolled, cites)
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
