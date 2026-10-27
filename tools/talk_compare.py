#!/usr/bin/env python3
"""Compare the talk-page splitter with DiscussionTools on the samples `tools/talk_dumps.py`
writes, and measure history-based attribution (ADR 0090 §13).

    tools/talk_compare.py data/talk-samples                  # comments, parents, names, IDs
    tools/talk_compare.py data/talk-samples --show 'Talk:Jesus/Archive 139'
    tools/talk_compare.py data/talk-samples --attribution    # blame over history.xml.gz

For every page in `manifest.json` that has a DiscussionTools file, the page's dumped revision is
split by `tools/talk_split.py` and compared with DiscussionTools' parse of the same revision:

  comments   matched by (author, timestamp); found by both, only by one or the other
  parents    for comments both found once, whether the parent is the same comment
  names      whether the derived DiscussionTools name is the prefix of DiscussionTools' ID
  IDs        whether the derived ID equals DiscussionTools' ID

`--attribution` replays every revision in `history.xml.gz` with a line-level blame (a moved or
restored line keeps its origin) and counts, for each signature the first time it appears,
whether the revision that added it was the signer's, a bot's, SineBot's or someone else's;
how close signature timestamps are to their revisions; and how many comments their signer
changed later. The research of 2026-10-20 (User talk:Harej, 4,677 revisions) is the baseline:
3,132 signatures: 85.8% signer, 8.9% another name, 4.8% bot, 13 SineBot; 98.8% in the same
minute; 5.0% changed later by their signer. On the nine current pages: 2,176 of
DiscussionTools' 2,177 comments, 97.5% of parents and 97.1% of IDs agree.

Standard library only; imports `talk_split.py` from beside it.
"""

import argparse
import collections
import difflib
import gzip
import json
import os
import re
import sys
import xml.etree.ElementTree as ET
from datetime import datetime

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import talk_split as ts  # noqa: E402

NS = "{http://www.mediawiki.org/xml/export-0.11/}"
BOTISH = re.compile(r"bot\b|Bot\b|bot\d*$|MediaWiki message delivery", re.I)


def revisions(path):
    """(page title, revid, user, timestamp, text) for every revision of a dump, in order."""
    opener = gzip.open if path.endswith(".gz") else open
    title = None
    with opener(path, "rb") as f:
        for _, el in ET.iterparse(f):
            if el.tag == NS + "title":
                title = el.text
            elif el.tag == NS + "revision":
                c = el.find(NS + "contributor")
                user = (c.findtext(NS + "username") or c.findtext(NS + "ip") or "(hidden)") if c is not None else "(hidden)"
                yield title, int(el.findtext(NS + "id")), user, el.findtext(NS + "timestamp"), el.findtext(NS + "text") or ""
                el.clear()


def dt_items(path):
    """DiscussionTools' comments: key (author, time), parent key, ID; each once."""
    items = json.load(open(path, encoding="utf-8"))["discussiontoolspageinfo"]["threaditemshtml"]
    out, seen = [], set()

    def walk(it, parent):
        if it["id"] in seen:          # subheadings and their replies appear twice
            return
        seen.add(it["id"])
        key = None
        if it["type"] == "comment":
            key = (ts.norm_user(it["author"]), datetime.strptime(it["timestamp"], "%Y-%m-%dT%H:%M:%SZ"))
            out.append({"key": key, "parent": parent, "id": it["id"]})
        for r in it.get("replies", []):
            walk(r, key)
    for it in items:
        walk(it, None)
    return out


def compare(samples, show):
    manifest = json.load(open(os.path.join(samples, "manifest.json"), encoding="utf-8"))
    texts = {}
    for name in ("current.xml", "history.xml.gz"):
        path = os.path.join(samples, name)
        if os.path.exists(path):
            for title, revid, _user, _ts, text in revisions(path):
                texts[(title, revid)] = text
    tot = collections.Counter()
    for p in manifest["pages"]:
        dt = p.get("discussiontools", {})
        if "file" not in dt or (p["title"], p["revid"]) not in texts:
            continue
        lines, sections, comments = ts.split(texts[(p["title"], p["revid"])])
        derived = ts.dt_names_and_ids(sections, comments)
        ours = collections.defaultdict(list)
        for n, c in enumerate(comments):
            pk = (comments[c.parent].author, comments[c.parent].when) if c.parent is not None else None
            ours[(c.author, c.when)].append((pk, derived.get(n)))
        theirs = dt_items(os.path.join(samples, dt["file"]))
        dk = collections.Counter(d["key"] for d in theirs)
        ok = collections.Counter((c.author, c.when) for c in comments)
        r = collections.Counter(ours=len(comments), dt=len(theirs), both=sum((dk & ok).values()))
        bad = []
        for d in theirs:
            mine = ours.get(d["key"], [])
            if len(mine) != 1:
                continue
            pk, nid = mine[0]
            r["parent_ok" if pk == d["parent"] else "parent_bad"] += 1
            if pk != d["parent"]:
                bad.append(("parent", d["key"], d["parent"], pk))
            if nid:
                r["name_ok"] += d["id"].startswith(nid[0])
                r["id_ok" if nid[1] == d["id"] else "id_bad"] += 1
        n = r["parent_ok"] + r["parent_bad"]
        print(f"{p['title']}: ours {r['ours']}, DiscussionTools {r['dt']}, both {r['both']}; "
              f"parents {r['parent_ok']}/{n}; names {r['name_ok']}; IDs {r['id_ok']}/{r['id_ok'] + r['id_bad']}")
        if show and p["title"] == show:
            for k in list(dk - ok)[:20]:
                print("   only DiscussionTools:", k[0], k[1])
            for k in list(ok - dk)[:20]:
                print("   only ours:", k[0], k[1])
            for _, k, theirp, ourp in bad[:20]:
                print(f"   parent of {k[0]} {k[1]}: DiscussionTools {theirp and theirp[0]}, ours {ourp and ourp[0]}")
        tot.update(r)
    n = tot["parent_ok"] + tot["parent_bad"]
    print(f"\ntotal: DiscussionTools {tot['dt']}, found by both {tot['both']} ({tot['both'] / max(tot['dt'], 1):.1%}); "
          f"parents {tot['parent_ok']}/{n} ({tot['parent_ok'] / max(n, 1):.1%}); "
          f"IDs {tot['id_ok']}/{tot['id_ok'] + tot['id_bad']}")


def sigs(text):
    out = {}
    for line in text.split("\n"):
        for author, when, _ in ts.signatures(line):
            if when:
                out.setdefault((author, when), line)
    return out


def attribution(samples):
    path = os.path.join(samples, "history.xml.gz")
    cats, delta, edits = collections.Counter(), collections.Counter(), collections.Counter()
    first_seen, prev_lines, prev_sigs, seen = {}, [], {}, set()
    pages = 0
    for title, revid, user, stamp, text in revisions(path):
        lines = text.split("\n")
        added = []
        sm = difflib.SequenceMatcher(None, prev_lines, lines, autojunk=False)
        for op, _a1, _a2, b1, b2 in sm.get_opcodes():
            if op != "equal":
                added += [line for line in lines[b1:b2] if not (len(line.strip()) > 20 and line in first_seen)]
        for line in lines:
            if len(line.strip()) > 20:
                first_seen.setdefault(line, revid)
        cur = sigs(text)
        for key in dict.fromkeys(k for k in sigs("\n".join(added)) if k not in prev_sigs):
            if key in seen:
                continue
            seen.add(key)
            author, when = key
            if author == user:
                cats["added by the signer's own account"] += 1
                d = abs((datetime.strptime(stamp, "%Y-%m-%dT%H:%M:%SZ") - when).total_seconds()) / 60
                delta["same minute" if d < 1.5 else "within 10 minutes" if d <= 10 else
                      "within a day" if d <= 1440 else "more than a day"] += 1
            elif user == "SineBot":
                cats["added by SineBot for the signer"] += 1
            elif BOTISH.search(user):
                cats["added by a bot (delivered message)"] += 1
            else:
                cats["added by an account with another name (rename, alternate, move)"] += 1
        for key, line in cur.items():
            if key in prev_sigs and prev_sigs[key] != line and key[0] == user:
                edits[key] += 1
        prev_lines, prev_sigs = lines, cur
        pages += 1
    total = sum(cats.values())
    print(f"{pages} revisions; {total} signatures, each counted where it first appears")
    for c, n in cats.most_common():
        print(f"  {n:6} {n / max(total, 1):6.1%}  {c}")
    own = sum(delta.values())
    print("signature time against revision time, signer's own:",
          ", ".join(f"{k} {v} ({v / max(own, 1):.1%})" for k, v in delta.most_common()))
    print(f"comments whose signature line the signer changed later: {len(edits)} ({len(edits) / max(total, 1):.1%})")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("samples", help="the directory tools/talk_dumps.py wrote")
    ap.add_argument("--show", metavar="TITLE", help="list one page's disagreements")
    ap.add_argument("--attribution", action="store_true", help="measure attribution over history.xml.gz")
    a = ap.parse_args()
    if a.attribution:
        attribution(a.samples)
    else:
        compare(a.samples, a.show)


if __name__ == "__main__":
    main()
