#!/usr/bin/env python3
"""Cross-reference checker for docs/decisions/*.md.

Checks:
 1. every relative markdown link resolves to a file
 2. every `§N[.M]` that follows a link to an ADR (within the same clause) names a
    section that exists in the target (heuristic; the report is for eyeballing)
 3. Amended-by ↔ Related symmetry: if A lists B under Amended-by, B must list A
    under Related (or Amended-by); and if B's Related says it "amends"/"settles"/
    "extends" something in A, A should list B under Amended-by.
 4. crate names mentioned anywhere vs. the 0005 §2 table
 5. every `Special:` name used in an ADR, the API docs or the registry is a name, alias,
    MediaWiki name or section alias in registry/special-pages.toml (0047 §1)
 6. ADRs in the 0050 format (a header with neither `Related` nor `Amended by`):
    header fields, section numbering, heading verbs, the Amendment log, provenance
    lines, open-question IDs and "Changes to other ADRs" (0050 §12). In check 3 the
    log and the settled questions of such an ADR stand in for `Amended by`, and its
    `Changes` and `Uses` for `Related`.
 7. every `NNNN An` and `NNNN Qn` citation names a log entry or question that exists
 8. decisions/INDEX.md is current (0050 §12). Run with --index to rewrite it.
 9. every agent an ADR's Author line names is an [[agent]] in registry/version.toml, and every
    [[agent]] is named by an ADR; a bare "Claude" only for [agent_attribution].unattributed (0077 §12)
10. every `origin` in content-models.toml, special-pages.toml and wikitext-functions.toml is
    mediawiki, triplespace, generic or an [[extension]] in registry/version.toml; no two names differ
    only in case; every [[extension]] is named by an origin or has inspired_by (0077 §12)
11. a Decision section that is a pointer (0050 §14) names an existing section of an architecture
    chapter whose provenance line cites it back, and the Chapters header lists exactly the chapters
    the pointers name; check 4 reads the crate map from chapter 22 §2.1, and check 5 compares
    canonical special-page names case-sensitively (0050 §14, checks 12–15)

Usage: check_adrs.py [--index] [DOCS_DIR]
"""
import os, re, sys, glob, collections

ARGS = [a for a in sys.argv[1:] if not a.startswith("--")]
WRITE_INDEX = "--index" in sys.argv[1:]
ROOT = ARGS[0] if ARGS else "."
DEC = os.path.join(ROOT, "decisions")
files = sorted(glob.glob(os.path.join(DEC, "0*.md")))
texts = {os.path.basename(f): open(f, encoding="utf-8").read() for f in files}
compat = {
    "wikibase-compat.md": open(os.path.join(ROOT, "api", "wikibase-compat.md"), encoding="utf-8").read(),
    "mediawiki-compat.md": open(os.path.join(ROOT, "api", "mediawiki-compat.md"), encoding="utf-8").read(),
}

def strip_fences(text):
    """Blank out fenced code blocks, keeping line count, so examples are not parsed."""
    out, fenced = [], False
    for line in text.split("\n"):
        if line.lstrip().startswith("```"):
            fenced = not fenced
            out.append("")
        else:
            out.append("" if fenced else line)
    return "\n".join(out)

def sections(text):
    s = set()
    for m in re.finditer(r"^#{2,4}\s+(\d+(?:\.\d+)?)[.\s]", strip_fences(text), re.M):
        s.add(m.group(1))
    return s

secs = {k: sections(v) for k, v in texts.items()}
secs.update({k: sections(v) for k, v in compat.items()})

num2file = {}
for name in texts:
    num2file[name[:4]] = name

problems = collections.defaultdict(list)

# 1 + 2: links and section refs
link_re = re.compile(r"\[([^\]]+)\]\(([^)#\s]+)(?:#[^)]*)?\)")
for name, text in texts.items():
    for m in link_re.finditer(text):
        label, target = m.group(1), m.group(2)
        if target.startswith("http"):
            continue
        tpath = os.path.normpath(os.path.join(DEC, target))
        if not os.path.exists(tpath):
            problems[name].append(f"broken link -> {target}")
            continue
        tbase = os.path.basename(tpath)
        if tbase not in secs:
            continue
        # look at text following the link up to the next link, ')' at depth, or sentence end
        tail = text[m.end(): m.end() + 140]
        tail = re.split(r"\[|\n|;|\|", tail)[0]   # a table cell ends the clause
        # inside a parenthetical "(§5 amends §4)" the first § is the *citing* ADR's section
        # when the verb is amends/settles/extends/refines/uses/follows; skip those
        paren = re.match(r"\s*\(([^)]*)\)", tail)
        cands = []
        if paren:
            inner = paren.group(1)
            # split on verbs: "§5 amends §4 and §8.2; §7 settles ..." -> after the verb are target §s
            for clause in re.split(r",|;| and (?=§)", inner):
                mm = re.search(r"(amends|settles|extends|refines|uses|follows|adds|builds on|specifies|answers|makes|registers|declares|supersedes|corrects)\s+(.*)", clause)
                if mm:
                    cands += re.findall(r"§(\d+(?:\.\d+)?)", mm.group(2))
                elif "§" in clause and not re.search(r"(open question|Consequences|Context)", clause):
                    cands += re.findall(r"§(\d+(?:\.\d+)?)", clause)
        else:
            head = re.match(r"[^.(]*", tail).group(0)
            cands = re.findall(r"§(\d+(?:\.\d+)?)", head)
        for c in cands:
            if c not in secs[tbase]:
                problems[name].append(f"{tbase} §{c} does not exist  (context: …{text[max(0,m.start()-30):m.end()+60].replace(chr(10),' ')}…)")

# --- the 0050 format -------------------------------------------------------------

VERBS = {"amends", "extends", "supersedes", "corrects", "settles", "uses"}   # 0050 §7
LOG_VERBS = {"amends", "extends", "supersedes", "corrects", "retitles", "consolidates", "relocates"}  # 0050 §8, §14
NEEDS_REPLACED = {"amends", "supersedes", "corrects"}
HEADER_ORDER = ["Status", "Date", "Updated", "Author", "Changes", "Uses", "Chapters"]  # 0050 §3, §14
STATUS_RE = re.compile(r"^(Proposed|Accepted|Accepted with proposed amendment \((A\d+)(, A\d+)*\)|Withdrawn|Superseded by \[?\d{4}\]?(\([^)]*\))?)$")

def header_fields(text):
    """The `- **Field:** value` lines between the H1 and the first H2, in order."""
    head = text.split("\n## ", 1)[0]
    return re.findall(r"^- \*\*([A-Za-z ]+):\*\*\s*(.*)$", head, re.M)

def is_new_format(text):
    names = [f for f, _ in header_fields(text)]
    return "Status" in names and "Related" not in names and "Amended by" not in names

def h2_block(text, title):
    """Body of the `## title` section (fences blanked), or None."""
    t = strip_fences(text)
    m = re.search(rf"^## {re.escape(title)}\s*$", t, re.M)
    if not m:
        return None
    rest = t[m.end():]
    n = re.search(r"^## ", rest, re.M)
    return rest[: n.start()] if n else rest

def expand_secs(spec):
    """'§2, §8–10, §4.1' -> {'2','8','9','10','4.1'}"""
    out = set()
    for a, b in re.findall(r"§(\d+(?:\.\d+)?)(?:[–-]§?(\d+))?", spec):
        if b and "." not in a:
            out |= {str(i) for i in range(int(a), int(b) + 1)}
        else:
            out.add(a)
    return out

def parse_log(text):
    """{n: {'title', 'fields': {...}, 'body'}} from `## Amendment log`."""
    block = h2_block(text, "Amendment log")
    entries = {}
    if block is None:
        return entries
    parts = re.split(r"^### A(\d+)\. (.*)$", block, flags=re.M)
    for i in range(1, len(parts), 3):
        n, title, body = int(parts[i]), parts[i + 1], parts[i + 2]
        fields = dict(re.findall(r"^- \*\*([A-Za-z ]+):\*\*\s*(.*)$", body, re.M))
        entries[n] = {"title": title, "fields": fields, "body": body}
    return entries

def parse_change(spec):
    """'amends §2, §8; extends §4' -> [('amends', {'2','8'}), ('extends', {'4'})]"""
    out = []
    for clause in spec.split(";"):
        clause = clause.strip()
        m = re.match(r"([a-z]+)\s*(.*)", clause)
        if m:
            out.append((m.group(1), expand_secs(m.group(2))))
    return out

def parse_questions(text):
    block = h2_block(text, "Open questions") or ""
    qs = {}
    for m in re.finditer(r"^- \*\*Q(\d+)\.(.*)$", block, re.M):
        qs[int(m.group(1))] = m.group(2)
    return qs

def decision_sections(text):
    """Numbered ### and #### headings under ## Decision (or ## Part … blocks), with their bodies."""
    block = h2_block(text, "Decision")
    if block is None:   # an ADR in parts (0022) heads each part with its own H2
        t = strip_fences(text)
        block = "".join(m.group(0) for m in re.finditer(r"^## Part .*?(?=^## (?!Part)|\Z)", t, re.M | re.S))
    heads = list(re.finditer(r"^(#{3,4}) (.*)$", block, re.M))
    out = []
    for i, h in enumerate(heads):
        body = block[h.end(): heads[i + 1].start() if i + 1 < len(heads) else len(block)]
        out.append((len(h.group(1)), h.group(2), body))
    return out

_chapter_cache = {}
def chapter_text(path):
    if path not in _chapter_cache:
        _chapter_cache[path] = strip_fences(open(path, encoding="utf-8").read())
    return _chapter_cache[path]

def chapter_section_sources(ctext, num):
    """The *Sources:* line under chapter heading `## N.` or `### N.M`, or None if no such heading."""
    m = re.search(rf"^#{{2,3}} {re.escape(num)}\.?\s", ctext, re.M)
    if not m:
        return None
    rest = ctext[m.end():].split("\n")
    for line in rest[1:6]:
        if line.startswith("*Sources:"):
            return line
        if line.startswith("#"):
            break
    return ""

new_format = {name for name, text in texts.items() if is_new_format(text)}
logs = {name: parse_log(texts[name]) for name in new_format}
questions = {name: parse_questions(texts[name]) for name in new_format}

def adr_nums(s):
    return set(re.findall(r"\((\d{4})-[^)]+\.md\)", s))

def settlers(note):
    """The sources of '*Settled by X and Y: answer*' are what comes before the colon (0050 §10)."""
    return re.split(r": ", note, maxsplit=1)[0]

for name in sorted(new_format):
    text = texts[name]
    me = name[:4]
    P = problems[name]

    # header
    fields = header_fields(text)
    names = [f for f, _ in fields]
    for f in names:
        if f not in HEADER_ORDER:
            P.append(f"header field '{f}' is not one of {HEADER_ORDER}")
    known = [f for f in names if f in HEADER_ORDER]
    if known != sorted(known, key=HEADER_ORDER.index):
        P.append(f"header fields out of order: {known}")
    for req in ("Status", "Date", "Author"):
        if req not in names:
            P.append(f"header lacks {req}")
    fd = dict(fields)
    if "Status" in fd and not STATUS_RE.match(fd["Status"].strip()):
        P.append(f"Status '{fd['Status']}' is not a 0050 §3 value")
    if not re.match(r"^# \d{4}\. ", text):
        P.append("H1 is not '# NNNN. Title'")
    for f in ("Changes", "Uses"):
        nums = re.findall(r"\[(\d{4})\]", fd.get(f, ""))
        if nums != sorted(nums):
            P.append(f"{f} is not in ascending order")

    # sections: numbered, contiguous, verbs in headings
    top, subs = [], collections.defaultdict(list)
    for level, head, body in decision_sections(text):
        m = re.match(r"(\d+)(?:\.(\d+))?\.?\s", head)
        if not m:
            P.append(f"unnumbered Decision section: '{head}'")
            continue
        if level == 3:
            top.append(int(m.group(1)))
        elif m.group(2):
            subs[int(m.group(1))].append(int(m.group(2)))
        paren = re.search(r"\(([^()]*\d{4}[^()]*)\)\s*$", head)
        if paren:
            for clause in re.split(r";", paren.group(1)):
                clause = clause.strip()
                if re.search(r"\b\d{4}\b", clause) and clause.split(" ")[0] not in VERBS:
                    P.append(f"§{m.group(0).strip(' .')}: heading verb '{clause.split(' ')[0]}' is not one of {sorted(VERBS)}")
    if top != list(range(1, len(top) + 1)):
        P.append(f"Decision sections are not numbered 1..n: {top}")
    for n, ms in subs.items():
        if ms != list(range(1, len(ms) + 1)):
            P.append(f"§{n} subsections are not numbered {n}.1..{n}.{len(ms)}: {ms}")

    # the log
    log = logs[name]
    ids = sorted(log)
    for a in re.findall(r"A(\d+)", fd.get("Status", "")):
        if int(a) not in log:
            P.append(f"Status names A{a}, which is not in the log")
    if ids and ids != list(range(1, len(ids) + 1)):
        P.append(f"log IDs are not A1..An: {ids}")
    if log and "Updated" in fd:
        m = re.search(r"\(A(\d+)\)", fd["Updated"])
        if not m or int(m.group(1)) != max(ids):
            P.append(f"Updated names {fd['Updated']!r}, but the last entry is A{max(ids)}")
    if log and "Updated" not in fd:
        P.append("header lacks Updated, but there is an Amendment log")
    touched = collections.defaultdict(set)   # section -> {A ids} from the log
    for n, e in log.items():
        f = e["fields"]
        for req in ("Date", "Source", "Change", "Summary"):
            if req not in f:
                P.append(f"A{n} lacks {req}")
        verbs = parse_change(f.get("Change", ""))
        for v, ss in verbs:
            if v not in LOG_VERBS:
                P.append(f"A{n}: change verb '{v}' is not one of {sorted(LOG_VERBS)}")
            if v in NEEDS_REPLACED and "Replaced text" not in e["body"]:
                P.append(f"A{n} {v} but quotes no Replaced text")
            if v not in ("consolidates", "retitles", "relocates"):
                for s in ss:
                    touched[s].add(n)
        if not f.get("Source", "").startswith("Direct") and not adr_nums(f.get("Source", "")):
            P.append(f"A{n}: Source is neither an ADR link nor 'Direct: …'")

    # provenance lines ↔ log
    for level, head, body in decision_sections(text):
        m = re.match(r"(\d+(?:\.\d+)?)", head)
        if not m:
            continue
        sec = m.group(1)
        first = next((l for l in body.split("\n") if l.strip()), "")
        prov = re.match(r"^\*Changed by ((?:A\d+(?:, )?)+)\.\*$", first.strip())
        stub = re.match(r"^\*Superseded by .*\(A(\d+)\)\.\*$", first.strip())
        listed = {int(x) for x in re.findall(r"A(\d+)", prov.group(1))} if prov else set()
        if stub:
            listed.add(int(stub.group(1)))
        for a in listed - touched.get(sec, set()):
            P.append(f"§{sec}: provenance names A{a}, whose Change does not name §{sec}" if a in log else f"§{sec}: provenance names A{a}, which is not in the log")
        for a in touched.get(sec, set()) - listed:
            P.append(f"§{sec}: A{a} changes it, but its provenance line does not say so")
    for s in touched:
        if s not in secs[name]:
            P.append(f"the log changes §{s}, which does not exist")

    # pointers (0050 §14): each names a chapter section that cites this section back
    pointed_chapters = set()
    for level, head, body in decision_sections(text):
        m = re.match(r"(\d+(?:\.\d+)?)", head)
        if not m:
            continue
        sec = m.group(1)
        ptrs = re.findall(r"^\*Current text: (.*)\*$", body, re.M)
        if not ptrs:
            continue
        rest = re.sub(r"^\*(Changed by|Current text:)[^\n]*$", "", body, flags=re.M).strip()
        if rest:
            P.append(f"§{sec}: is a pointer but still carries text (0050 §14: text or pointer, never both)")
        for ptr in ptrs:
            for ch, path, specs in re.findall(r"\[(\d{2})\]\(([^)]*)\)((?:\s*§\d+(?:\.\d+)?[,;]?)+)", ptr):
                pointed_chapters.add(ch)
                cpath = os.path.normpath(os.path.join(DEC, path))
                if not os.path.exists(cpath):
                    P.append(f"§{sec}: pointer to missing chapter file {path}")
                    continue
                ctext = chapter_text(cpath)
                for cs in re.findall(r"§(\d+(?:\.\d+)?)", specs):
                    prov = chapter_section_sources(ctext, cs)
                    if prov is None:
                        P.append(f"§{sec}: pointer names {ch} §{cs}, which does not exist")
                    else:
                        cited = set()
                        for a_, specs_ in re.findall(r"\[(\d{4})\]\([^)]*\)((?:\s*§\d+(?:\.\d+)?,?)+)", prov):
                            if a_ == me:
                                cited |= set(re.findall(r"§(\d+(?:\.\d+)?)", specs_))
                        if sec not in cited and sec.split(".")[0] not in cited:
                            P.append(f"§{sec}: pointer names {ch} §{cs}, whose provenance does not cite {me} §{sec}")
    listed_chapters = set(re.findall(r"\[(\d{2})\]", fd.get("Chapters", "")))
    if pointed_chapters != listed_chapters:
        P.append(f"Chapters header {sorted(listed_chapters)} ≠ chapters the pointers name {sorted(pointed_chapters)}")

    # callouts: every change is folded (0050 §6)
    for m in re.finditer(r"^\s*> \*\*(A\d+)\.\*\*", strip_fences(text), re.M):
        P.append(f"{m.group(1)} stands as a callout: fold it (0050 §6)")
    for m in re.finditer(r"^\s*> \*\*(Amended|Extended|Refined|Settled)\b[^*]*\*\*", strip_fences(text), re.M):
        P.append(f"old-style callout '{m.group(0).strip()[:60]}': fold it and log it (0050 §6)")

    # open questions
    qs = questions[name]
    qids = sorted(qs)
    if qids != list(range(1, len(qids) + 1)):
        P.append(f"open questions are not Q1..Qn: {qids}")
    for n, rest in qs.items():
        if "~~" in rest and not re.search(r"\*(Settled by|Withdrawn)", rest):
            P.append(f"Q{n} is struck but neither Settled by nor Withdrawn")

    # Changes to other ADRs ↔ header Changes ↔ the targets' logs
    ch_block = h2_block(text, "Changes to other ADRs")
    header_changes = set(re.findall(r"\[(\d{4})\]", fd.get("Changes", "")))
    rows = []
    if ch_block:
        for line in ch_block.split("\n"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if len(cells) >= 4 and re.search(r"\[\d{4}\]", cells[0]):
                rows.append(cells)
    elif header_changes:
        P.append("header lists Changes, but there is no '## Changes to other ADRs'")
    row_targets = {re.search(r"\[(\d{4})\]", r[0]).group(1) for r in rows}
    if row_targets != header_changes:
        P.append(f"Changes header {sorted(header_changes)} ≠ Changes to other ADRs table {sorted(row_targets)}")
    for r in rows:
        tnum = re.search(r"\[(\d{4})\]", r[0]).group(1)
        verb = r[2].split()[0] if r[2] else ""
        if verb not in VERBS | {"consolidates", "retitles", "relocates"}:
            P.append(f"Changes to other ADRs: verb '{verb}' for {tnum}")
        tfile = num2file.get(tnum)
        if tfile in new_format and verb == "settles":
            m = re.search(rf"{tnum} Q(\d+)", r[3])
            q = questions[tfile].get(int(m.group(1))) if m else None
            if not m:
                P.append(f"Changes to other ADRs: a settles row for {tnum} names its question ({tnum} Qn)")
            elif q is None:
                P.append(f"Changes to other ADRs: {tnum} Q{m.group(1)} does not exist")
            elif not any(me in adr_nums(settlers(sm)) for sm in re.findall(r"\*Settled by ([^*]*)\*", q)):
                P.append(f"Changes to other ADRs: {tnum} Q{m.group(1)} is not settled by {me}")
        elif tfile in new_format:
            m = re.search(rf"{tnum} A(\d+)", r[3])
            if not m:
                P.append(f"Changes to other ADRs: {tnum} is in the 0050 format, so name its log entry ({tnum} An)")
            else:
                e = logs[tfile].get(int(m.group(1)))
                if not e:
                    P.append(f"Changes to other ADRs: {tnum} A{m.group(1)} does not exist")
                elif me not in adr_nums(e["fields"].get("Source", "")):
                    P.append(f"Changes to other ADRs: {tnum} A{m.group(1)} does not cite {me} as its Source")

# 3: amended-by / related symmetry
def header_field(text, field):
    m = re.search(rf"^- \*\*{field}:\*\*\s*(.*)$", text, re.M)
    return m.group(1) if m else ""

amended_by = {}
related = {}
for name, text in texts.items():
    if name in new_format:
        srcs = set()
        for e in logs[name].values():
            srcs |= adr_nums(e["fields"].get("Source", ""))
        for q in questions[name].values():
            for m in re.finditer(r"\*Settled by ([^*]*)\*", q):
                srcs |= adr_nums(settlers(m.group(1)))
        amended_by[name] = srcs
        related[name] = set(re.findall(r"\[(\d{4})\]", header_field(text, "Changes") + header_field(text, "Uses")))
        continue
    ab = header_field(text, "Amended by")
    rel = header_field(text, "Related")
    amended_by[name] = set(re.findall(r"\((\d{4})-[^)]+\.md\)", ab))
    related[name] = set(re.findall(r"\((\d{4})-[^)]+\.md\)", rel))

for a, bs in amended_by.items():
    for b in bs:
        bf = num2file.get(b)
        if not bf:
            problems[a].append(f"Amended-by names unknown ADR {b}")
            continue
        if a[:4] not in related[bf] and a[:4] not in amended_by[bf]:
            problems[bf].append(f"{a[:4]} lists {b} under Amended-by, but {b} does not list {a[:4]} under Related")

# reverse: B's Related clause says it amends/settles/extends A -> A should list B as amender
for b, text in texts.items():
    if b in new_format:
        for a in set(re.findall(r"\[(\d{4})\]", header_field(text, "Changes"))):
            af = num2file.get(a)
            if af and b[:4] not in amended_by[af]:
                problems[af].append(f"{b[:4]}'s Changes names {a}, but {a} does not list {b[:4]} under Amended-by or in its log")
        continue
    rel = header_field(text, "Related")
    for m in re.finditer(r"\[(\d{4})[^\]]*\]\([^)]+\)\s*(\([^)]*\))?", rel):
        a = m.group(1)
        clause = m.group(2) or ""
        if re.search(r"amends|settles|extends|refines|adds|answers|fills|specifies", clause):
            af = num2file.get(a)
            if af and b[:4] not in amended_by[af]:
                problems[af].append(f"{b[:4]}'s Related says it {clause} {a}, but {a} does not list {b[:4]} under Amended-by")

# 8: the cross-reference index (0050 §12)
def index_markdown():
    rows = []
    for name in sorted(texts):
        text = texts[name]
        me = name[:4]
        title = re.match(r"^# \d{4}(?:\.| —) (.*)$", text, re.M).group(1)
        fd = dict(header_fields(text))
        inbound = sorted(n for n in amended_by[name] if n in num2file and n != me)
        outbound = sorted(b[:4] for b in texts if b != name and me in amended_by[b])
        link = lambda n: f"[{n}]({num2file[n]})"
        upd = re.match(r"\S+", fd.get("Updated", "")) if name in new_format else None
        rows.append(f"| {link(me)} | {title} | {fd.get('Status', '?')} | "
                    f"{'0050' if name in new_format else 'old'} | {upd.group(0) if upd else '—'} | "
                    f"{', '.join(map(link, outbound)) or '—'} | {', '.join(map(link, inbound)) or '—'} |")
    return "\n".join([
        "# ADR index",
        "",
        "Generated by `check_adrs.py --index` from the ADRs themselves ([0050](0050-adr-format.md) §12). Do not edit it by hand: change the ADRs and regenerate.",
        "",
        "**Changes** lists the ADRs whose text this one changes. **Changed by** lists the ADRs that change this one: for an ADR in the 0050 format, the sources in its Amendment log and of its settled questions; for one in the old format, its `Amended by` line. Each column is the inverse of the other.",
        "",
        "| ADR | Title | Status | Format | Updated | Changes | Changed by |",
        "|---|---|---|---|---|---|---|",
    ] + rows) + "\n"

INDEX_PATH = os.path.join(DEC, "INDEX.md")
index_md = index_markdown()
index_problem = None
if WRITE_INDEX:
    with open(INDEX_PATH, "w", encoding="utf-8") as fh:
        fh.write(index_md)
elif not os.path.exists(INDEX_PATH):
    index_problem = "decisions/INDEX.md is missing: run check_adrs.py --index"
elif open(INDEX_PATH, encoding="utf-8").read() != index_md:
    index_problem = "decisions/INDEX.md is stale: run check_adrs.py --index"

# 7: An and Qn citations
for name, text in texts.items():
    for m in re.finditer(r"\b(\d{4})(?:\]\([^)]*\))? ([AQ])(\d+)\b", strip_fences(text)):
        tnum, kind, n = m.group(1), m.group(2), int(m.group(3))
        tf = num2file.get(tnum)
        if not tf:
            continue
        if tf not in new_format:
            problems[name].append(f"cites {tnum} {kind}{n}, but {tnum} is not in the 0050 format yet")
        elif kind == "A" and n not in logs[tf]:
            problems[name].append(f"cites {tnum} A{n}, which is not in its log")
        elif kind == "Q" and n not in questions[tf]:
            problems[name].append(f"cites {tnum} Q{n}, which is not among its open questions")

# 4: crate names
_ch22 = os.path.join(ROOT, "architecture", "22-crates-and-stack.md")
if os.path.exists(_ch22):   # 0050 §14 check 14: the crate map lives in chapter 22 §2.1
    table = open(_ch22, encoding="utf-8").read().split("### 2.1 The table")[1].split("### 2.2")[0]
else:
    table = texts[num2file["0005"]].split("### 2. Crate map")[1].split("### 3.")[0]
tbl_crates = set(re.findall(r"`((?:scatter|triplespace)-[a-z0-9-]+)`", table))
retired = {"scatter-graphs", "scatter-markdown", "scatter-keyed", "triplespace-activity", "triplespace-revmeta"}
# Content model IDs unique to Triplespace share the `triplespace-` prefix (0041 §2); they are not crates.
_cm = os.path.join(ROOT, "registry", "content-models.toml")
model_ids = set(re.findall(r'^id = "([^"]+)"', open(_cm, encoding="utf-8").read(), re.M)) if os.path.exists(_cm) else set()
# Model IDs renamed before any data existed, still named in the ADRs that record the rename.
model_ids |= {"triplespace-osm-tag"}  # 0048 §7: now triplespace-notation
tbl_crates -= model_ids
all_mentions = collections.defaultdict(set)
for name, text in texts.items():
    for c in set(re.findall(r"`((?:scatter|triplespace)-[a-z0-9-]+)`", text)) - model_ids:
        all_mentions[c].add(name[:4])
print("== crates named in ADRs but not in the 0005 §2 table ==")
for c, where in sorted(all_mentions.items()):
    if c not in tbl_crates:
        tag = " (retired name)" if c in retired else ""
        print(f"  {c}{tag}: {sorted(where)}")
print()

# 5: special page names (0047 §1). Parsed with regexes so the checker needs no TOML library.
_sp = os.path.join(ROOT, "registry", "special-pages.toml")
if os.path.exists(_sp):
    _t = open(_sp, encoding="utf-8").read()
    sp_canonical = {m.group(2) for m in re.finditer(r'^(name|mediawiki_name) = "([^"]+)"', _t, re.M)}
    sp_exact = set(sp_canonical)
    sp_known = {x.lower() for x in sp_canonical}
    for m in re.finditer(r'^aliases = \[([^\]]*)\]', _t, re.M):
        sp_exact |= set(re.findall(r'"([^"]+)"', m.group(1)))
        sp_known |= {x.lower() for x in re.findall(r'"([^"]+)"', m.group(1))}
    for m in re.finditer(r'^section_aliases = \{(.*)\}$', _t, re.M):
        sp_known |= {x.lower() for x in re.findall(r'([A-Za-z]+) = ', m.group(1))}
    sources = dict(texts)
    sources.update(compat)
    for name, text in sources.items():
        for u in sorted(set(re.findall(r"Special:([A-Za-z]+)", text))):
            if u.lower() not in sp_known:
                problems[name].append(f"Special:{u} is not in registry/special-pages.toml")
            elif u not in sp_exact and u.lower() in {c.lower() for c in sp_canonical}:
                problems[name].append(f"Special:{u} differs in case from the registry's canonical name (0050 §14 check 15)")
else:
    print("== registry/special-pages.toml not found; check 5 skipped ==")

# 9 and 10: registry/version.toml (0077 §12). Parsed with regexes, like check 5.
reg_problems = collections.defaultdict(list)
_vt = os.path.join(ROOT, "registry", "version.toml")
if os.path.exists(_vt):
    _v = open(_vt, encoding="utf-8").read()
    agents = set(re.findall(r'^\[\[agent\]\]\nname = "([^"]+)"', _v, re.M))
    _ua = re.search(r'^\[agent_attribution\]\nunattributed = \[([^\]]*)\]', _v, re.M)
    unattributed = set(re.findall(r'"(\d{4})"', _ua.group(1))) if _ua else set()
    named = set()
    for name, text in texts.items():
        line = header_field(text, "Author")
        for who in re.findall(r"/\s*([^;/]+?)\s*(?=;|$)", line):
            if who == "Claude":
                if name[:4] not in unattributed:
                    problems[name].append('Author names "Claude" without the model, and the ADR is not in version.toml\'s [agent_attribution].unattributed')
            elif who not in agents:
                problems[name].append(f"Author names {who}, which is not an [[agent]] in registry/version.toml")
            else:
                named.add(who)
    for a in sorted(agents - named):
        reg_problems["registry/version.toml"].append(f"[[agent]] {a} is named by no ADR's Author line")
    for n in sorted(unattributed - {x[:4] for x in texts}):
        reg_problems["registry/version.toml"].append(f"[agent_attribution] names {n}, which is not an ADR")

    ext = {}
    for block in re.split(r"^\[\[extension\]\]$", _v, flags=re.M)[1:]:
        m = re.search(r'^name = "([^"]+)"', block, re.M)
        if m:
            ext[m.group(1)] = "inspired_by" in block.split("\n[")[0]
    keywords = {"mediawiki", "triplespace", "generic"}
    origins = collections.defaultdict(set)
    for f in ("content-models.toml", "special-pages.toml", "wikitext-functions.toml"):
        fp = os.path.join(ROOT, "registry", f)
        if not os.path.exists(fp):
            continue
        for line in open(fp, encoding="utf-8"):
            if line.lstrip().startswith("#"):
                continue
            for o in re.findall(r'\borigin = "([^"]+)"', line):
                origins[o].add(f)
    for o, where in sorted(origins.items()):
        if o not in keywords and o not in ext:
            reg_problems["registry/version.toml"].append(f"origin {o} ({', '.join(sorted(where))}) is not an [[extension]]")
    seen = collections.defaultdict(set)
    for o in set(origins) | set(ext) | keywords:
        seen[o.lower()].add(o)
    for k, v in sorted(seen.items()):
        if len(v) > 1:
            reg_problems["registry/version.toml"].append(f"names differ only in case: {', '.join(sorted(v))}")
    for e, inspired in sorted(ext.items()):
        if e not in origins and not inspired:
            reg_problems["registry/version.toml"].append(f"[[extension]] {e} is named by no origin and has no inspired_by")
else:
    print("== registry/version.toml not found; checks 9 and 10 skipped ==")

print("== per-ADR problems ==")
for name in sorted(texts):
    if problems[name]:
        print(name)
        for p in problems[name]:
            print("   -", p)
for name in sorted(reg_problems):
    print(name)
    for p in reg_problems[name]:
        print("   -", p)
if index_problem:
    print("INDEX.md")
    print("   -", index_problem)
if WRITE_INDEX:
    print("\nwrote decisions/INDEX.md")
print(f"\ndone: {len(texts)} ADRs, {len(new_format)} in the 0050 format")
