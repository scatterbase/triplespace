#!/usr/bin/env python3
"""The reference prototype of the talk-page splitter (ADR 0090 §2, §3, §9): split wikitext talk
pages into sections and signed comments, and derive DiscussionTools names and IDs.

    tools/talk_split.py DUMP.xml                       # per page: sections, comments, undated, unsigned
    tools/talk_split.py DUMP.xml --page 'Talk:Gdańsk/Vote'      # that page's comment tree
    tools/talk_split.py PAGE.wiki                      # a single page's wikitext

It is the Python counterpart the Rust splitter in `scatter-talk` is measured against, through
`tools/talk_compare.py` (0090 §13). The rules, common case first:

  * a heading line starts a section; level 2 starts a thread, deeper ones are subsections
  * a comment ends at a signature: a user link followed, within 100 rendered characters, by a
    timestamp; character references are decoded first; the first signature on a line ends the
    comment and later ones are notes inside it
  * an undated signature (before ~2005) is a dash, then a user link, ending the line
  * {{unsigned|Name|date}} signs the text before it for Name
  * a comment's depth is its first line's indentation; {{outdent}} makes the next comment a
    reply to the previous one; its parent is the nearest earlier comment in the section with
    smaller depth

The formats and template names here are the English entries of docs/registry/talk-markup.toml,
written out by hand; the Rust splitter reads the registry. Standard library only.
"""

import argparse
import gzip
import html
import re
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from datetime import datetime

MONTHS = ["january", "february", "march", "april", "may", "june", "july", "august",
          "september", "october", "november", "december"]
MON = r"(?P<mon>[A-Z][a-z]{2,8})\.?"
TIME = r"(?P<h>\d{1,2}):(?P<mi>\d{2})"
TS_FORMATS = [
    TIME + r",? (?P<d>\d{1,2}) " + MON + r",? (?P<y>\d{4})",               # H:i, j F Y / j M Y
    TIME + r",? " + MON + r" (?P<d>\d{1,2}),? (?P<y>\d{4})",               # H:i, F j, Y / M j, Y
    TIME + r",? (?P<y>\d{4}) " + MON + r" (?P<d>\d{1,2})",                 # H:i, Y F j / Y M j
    r"(?P<y>\d{4})-(?P<mo>\d{2})-(?P<d>\d{2})[ T]" + TIME + r"(?::\d{2})?",  # Y-m-d\TH:i:s, Y-m-d H:i
    TIME + r",? (?P<y>\d{4})-(?P<mo>\d{2})-(?P<d>\d{2})",                  # H:i, Y-m-d
]
TS_RES = [re.compile(f + r"(?:\s*\((?P<tz>[A-Z]{2,5})\))?") for f in TS_FORMATS]

USER_LINK = re.compile(
    r"\[\[\s*:?\s*(?:User|User[ _]talk|U|UT|Special:Contributions|Special:Contribs)\s*[:/]\s*"
    r"(?P<name>[^\]|#/]+?)\s*(?:#[^\]|]*)?(?:\|[^\]]*)?\]\]", re.I)
UNSIGNED = re.compile(r"\{\{\s*(?:subst:)?\s*(?:unsigned(?:IP|2|3|IP2| IP| IP2|-ip|ip)?|unsig)\s*\|\s*"
                      r"(?P<name>[^|}]+?)\s*(?:\|\s*(?P<when>[^|}]*?)\s*)?\}\}", re.I)
HEADING = re.compile(r"^(?P<eq>={1,6})(?P<text>.+?)(?P=eq)\s*(?:<!--.*?-->\s*)*$")
OUTDENT = re.compile(r"\{\{\s*(?:od|od2|outdent|outdent2|out|unindent)\s*(?:\|[^}]*)?\}\}", re.I)
INDENT = re.compile(r"^[:*#]+")
LINK_TEXT = re.compile(r"\[\[(?:[^\]|]*\|)?([^\]]*)\]\]")
TAG = re.compile(r"<[^>]*>")
UNDATED_MARKS = ("--", "—", "–", "-", "~")
DT_SWITCH = datetime(2022, 7, 12)   # $wgDiscussionToolsTimestampFormatSwitchTime, read in local time


def month(s):
    s = s.lower().rstrip(".")
    if len(s) < 3:
        return None
    for i, name in enumerate(MONTHS, 1):
        if name.startswith(s):
            return i
    return None


def find_timestamps(line):
    """(start, end, datetime, zone) for every timestamp on the line, leftmost-longest."""
    out = []
    for rx in TS_RES:
        for m in rx.finditer(line):
            g = m.groupdict()
            mo = int(g["mo"]) if g.get("mo") else month(g.get("mon") or "")
            if not mo:
                continue
            try:
                when = datetime(int(g["y"]), mo, int(g["d"]), int(g["h"]), int(g["mi"]))
            except ValueError:
                continue
            if 2001 <= when.year <= 2100:
                out.append((m.start(), m.end(), when, g.get("tz")))
    out.sort(key=lambda t: (t[0], -t[1]))
    dedup, last_end = [], -1
    for t in out:
        if t[0] >= last_end:
            dedup.append(t)
            last_end = t[1]
    return dedup


def norm_user(name):
    name = html.unescape(name).replace("_", " ").strip()
    return name[:1].upper() + name[1:] if name else name


def visible_len(s):
    """Roughly how long wikitext reads once rendered: tags, link targets and quote marks gone."""
    s = TAG.sub("", LINK_TEXT.sub(r"\1", s)).replace("''", "")
    return len(html.unescape(s))


def signatures(line):
    """(author, timestamp or None, end offset) of each signature on the line."""
    line = html.unescape(line)
    out, floor = [], 0
    for start, end, when, _zone in find_timestamps(line):
        links = list(USER_LINK.finditer(line, floor, start))
        if links and visible_len(line[links[-1].end():start]) <= 100:
            out.append((norm_user(links[-1].group("name")), when, end))
            floor = end
    if out:
        return out
    links = list(USER_LINK.finditer(line))
    if links:
        last = links[-1]
        before = TAG.sub("", line[:last.start()]).rstrip()
        if visible_len(line[last.end():]) <= 30 and (before.endswith(UNDATED_MARKS) or not before.lstrip(":*# ")):
            return [(norm_user(last.group("name")), None, len(line))]
    m = None
    for m in UNSIGNED.finditer(line):
        pass
    if m:
        when = None
        if m.group("when"):
            ts = find_timestamps(m.group("when"))
            when = ts[0][2] if ts else None
        return [(norm_user(m.group("name")), when, m.end())]
    return []


@dataclass
class Comment:
    section: int
    depth: int
    first: int
    last: int
    author: str
    when: datetime | None
    parent: int | None = None
    level: int = 1
    outdented: bool = False


@dataclass
class Section:
    level: int
    title: str
    line: int
    parent: int | None = None
    comments: list = field(default_factory=list)
    unsigned_lines: int = 0


def depth_of(line):
    m = INDENT.match(line)
    return len(m.group(0)) if m else 0


def split(text):
    """(lines, sections, comments). Section 0 is the text above the first heading."""
    lines = text.split("\n")
    sections = [Section(0, "", 0)]
    comments = []
    pending = []
    outdent_next = False
    in_html_comment = False

    def add(c):
        same = sections[c.section].comments
        if c.outdented and same:
            c.parent = same[-1]
        else:
            for k in reversed(same):
                if comments[k].depth < c.depth:
                    c.parent = k
                    break
        c.level = comments[c.parent].level + 1 if c.parent is not None else 1
        comments.append(c)
        same.append(len(comments) - 1)

    for i, line in enumerate(lines):
        h = HEADING.match(line)
        if h:
            sections[-1].unsigned_lines += sum(1 for j in pending if lines[j].strip())
            pending = []
            outdent_next = False
            level = len(h.group("eq"))
            parent = next((k for k in range(len(sections) - 1, 0, -1) if sections[k].level < level), None)
            sections.append(Section(level, h.group("text").strip(), i, parent))
            continue
        if in_html_comment or ("<!--" in line and "-->" not in line):
            in_html_comment = "-->" not in line
            continue
        sigs = signatures(line)
        if OUTDENT.search(line) and not sigs:
            outdent_next = True
            continue
        pending.append(i)
        if sigs:
            author, when, _end = sigs[0]          # later signatures on the line are notes
            body = [j for j in pending if lines[j].strip()]
            first = body[0] if body else i
            add(Comment(len(sections) - 1, depth_of(lines[first]), first, i, author, when,
                        outdented=outdent_next))
            pending = []
            outdent_next = False
    sections[-1].unsigned_lines += sum(1 for j in pending if lines[j].strip())
    return lines, sections, comments


# --- DiscussionTools names and IDs (0090 §9) -----------------------------------------------

def dt_timestamp(when):
    return when.strftime("%Y-%m-%dT%H:%M:%S.000Z") if when < DT_SWITCH else when.strftime("%Y%m%d%H%M%S")


def dt_truncate(s, legacy=False):
    t = s.encode()[:80].decode(errors="ignore")
    return t if legacy else t.strip("_")


def heading_anchor(title):
    """An approximation of a heading's linkable ID; the Rust splitter uses the renderer's."""
    t = LINK_TEXT.sub(r"\1", title)
    t = re.sub(r"<[^>]*>|'''?|\{\{[^}]*\}\}", "", t)
    return re.sub(r"[ _]+", "_", html.unescape(t).strip())


def dt_names_and_ids(sections, comments):
    """{comment index: (name, id)} for the dated comments, as DiscussionTools computes them."""
    seen, anchors = {}, {}
    for k, s in enumerate(sections):
        if k:
            a = heading_anchor(s.title)
            seen[a] = seen.get(a, 0) + 1
            anchors[k] = a if seen[a] == 1 else f"{a}_{seen[a]}"
    ids, out = set(), {}
    for n, c in enumerate(comments):
        if c.when is None:
            continue
        name = f"c-{dt_truncate(c.author.replace(' ', '_'))}-{dt_timestamp(c.when)}"
        cid = name
        if c.parent is not None and comments[c.parent].when is not None:
            p = comments[c.parent]
            cid += f"-{dt_truncate(p.author.replace(' ', '_'))}-{dt_timestamp(p.when)}"
        elif c.parent is None and c.section:
            cid += "-" + dt_truncate(anchors[c.section])
        base, i = cid, 1
        while cid in ids:
            cid = f"{base}-{i}"
            i += 1
        ids.add(cid)
        out[n] = (name, cid)
    return out


# --- CLI -------------------------------------------------------------------------------------

def dump_pages(path):
    """(title, revid, text) of each page's last revision in an export-0.11 dump."""
    opener = gzip.open if path.endswith(".gz") else open
    ns = "{http://www.mediawiki.org/xml/export-0.11/}"
    with opener(path, "rb") as f:
        title = rev = text = None
        for _, el in ET.iterparse(f):
            if el.tag == ns + "title":
                title = el.text
            elif el.tag == ns + "revision":
                rev, text = el.findtext(ns + "id"), el.findtext(ns + "text") or ""
                el.clear()
            elif el.tag == ns + "page":
                yield title, rev, text
                el.clear()


def show_tree(lines, sections, comments):
    for k, s in enumerate(sections):
        if s.comments or k == 0:
            print(("=" * s.level + " " + s.title) if k else "(above the first heading)")
        for n in s.comments:
            c = comments[n]
            when = c.when.strftime("%Y-%m-%d %H:%M") if c.when else "undated"
            print(f"{'  ' * c.level}{c.author} {when}  [lines {c.first + 1}–{c.last + 1}]")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("input", help="an export-0.11 dump (.xml or .xml.gz) or a .wiki file")
    ap.add_argument("--page", help="print this page's comment tree")
    a = ap.parse_args()
    if a.input.endswith((".xml", ".xml.gz")):
        pages = dump_pages(a.input)
    else:
        pages = [(a.input, None, open(a.input, encoding="utf-8").read())]
    for title, rev, text in pages:
        if a.page and title != a.page:
            continue
        lines, sections, comments = split(text)
        if a.page or len(sys.argv) == 2 and not a.input.endswith((".xml", ".xml.gz")):
            show_tree(lines, sections, comments)
            continue
        print(f"{title} ({rev}): {len(sections) - 1} headings, {len(comments)} comments "
              f"({sum(c.when is None for c in comments)} undated), "
              f"{sum(s.unsigned_lines for s in sections)} unsigned lines")


if __name__ == "__main__":
    main()
