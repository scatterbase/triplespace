#!/usr/bin/env python3
"""Download wiki talk pages as MediaWiki XML dumps, with DiscussionTools' reading of each, as
samples for converting wikitext talk pages into threads (0019, 0054 §5, 0069).

    tools/talk_dumps.py OUT                   # the default sample set, from English Wikipedia
    tools/talk_dumps.py OUT --current 'Talk:Foo' 'Talk:Foo/Archive 1' --history 'User talk:Bar'
    tools/talk_dumps.py OUT --api https://de.wikipedia.org/w/api.php --current 'Diskussion:Berlin' --history

Two kinds of page:

  --current   the latest revision only, from the Action API's `export` module; all of them
              in one dump, OUT/current.xml
  --history   every revision, oldest first, read with `prop=revisions` and written in the
              same export-0.11 shape to OUT/history.xml.gz (`export` gives only the latest).
              The siteinfo is the wiki's own; each <origin> is the revision's own ID, since
              the API does not report it

Then, for the dumped revision of every page (the newest one of a history page),
DiscussionTools' parse of it from `action=discussiontoolspageinfo`, pinned by `oldid` so it
reads exactly the text in the dump:

    discussiontools/<pageid>-<revid>.json   `threaditemshtml`: the heading and comment tree,
                                            with each item's ID, name, author, timestamp,
                                            level and HTML; and `transcludedfrom`

`manifest.json` records the API, the wiki's generator, when each dump was fetched, and per
page its ID, namespace, title, dumped revision, revision count and how many headings and
comments DiscussionTools found (or the error it gave).

Live pages change, but a dump is pinned once written: files already in OUT are kept, and a
rerun fetches only what is missing (DiscussionTools for the revisions in the existing
dumps, and any lookup that failed). --force fetches everything again. Titles that do not exist are reported and skipped.

Standard library only. Requests are sequential, --delay seconds apart (default 0.2), with
maxlag=5 and a few retries on network errors and lag.
"""

import argparse
import gzip
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_API = "https://en.wikipedia.org/w/api.php"
USER_AGENT = "triplespace-tools/talk_dumps.py (https://github.com/scatterbase/triplespace)"
EXPORT_NS = "{http://www.mediawiki.org/xml/export-0.11/}"

DEFAULT_CURRENT = [
    "Wikipedia:Reference desk/Science",
    "Wikipedia:Administrators' noticeboard/Incidents",
    "Talk:Barack Obama/Archive 1",
    "Talk:Barack Obama/Archive 84",
    "Wikipedia:Requests for adminship/Vermont",
    "Talk:Jesus/Archive 1",
    "Talk:Jesus/Archive 139",
    "Talk:Gdańsk/Vote",
    "User talk:Jimbo Wales/Archive 256",
]
DEFAULT_HISTORY = ["User talk:Harej"]

REV_PROPS = "ids|timestamp|flags|comment|user|userid|size|sha1|content|contentmodel"
DT_PROPS = "transcludedfrom|threaditemshtml"


def now():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


class Api:
    def __init__(self, url, delay, retries=5):
        self.url = url
        self.delay = delay
        self.retries = retries
        self.last = 0.0

    def raw(self, **params):
        """The response body as text, after waiting out lag and retrying network errors."""
        params = {"format": "json", "formatversion": "2", "maxlag": "5", **params}
        data = urllib.parse.urlencode(params).encode()
        for attempt in range(self.retries + 1):
            wait = self.last + self.delay - time.monotonic()
            if wait > 0:
                time.sleep(wait)
            self.last = time.monotonic()
            try:
                # POST, so a long title list never meets a URL length limit.
                req = urllib.request.Request(self.url, data=data, headers={"User-Agent": USER_AGENT})
                with urllib.request.urlopen(req, timeout=120) as r:
                    ctype = r.headers.get("Content-Type", "")
                    body = r.read().decode("utf-8")
            except (urllib.error.URLError, TimeoutError, ConnectionError) as e:
                if attempt == self.retries:
                    raise
                print(f"  retrying after {e}", file=sys.stderr)
                time.sleep(2 ** attempt)
                continue
            if "json" in ctype:
                try:
                    err = json.loads(body).get("error")
                except json.JSONDecodeError:
                    err = None
                if err and err.get("code") == "maxlag" and attempt < self.retries:
                    time.sleep(5)
                    continue
            return ctype, body
        raise RuntimeError("unreachable")

    def get(self, **params):
        _, body = self.raw(**params)
        return json.loads(body)

    def query_all(self, **params):
        """Every result of a `query`, following `continue`."""
        cont = {}
        while True:
            data = self.get(action="query", **params, **cont)
            if "error" in data:
                raise RuntimeError(f"{params.get('titles') or params.get('pageids')}: {data['error']}")
            yield data.get("query", {})
            if "continue" not in data:
                return
            cont = data["continue"]

    def export(self, titles):
        """A current-revision export-0.11 dump of `titles`."""
        ctype, body = self.raw(action="query", titles="|".join(titles), export="1",
                               exportnowrap="1", exportschema="0.11")
        if "xml" not in ctype:
            raise RuntimeError(f"export returned {ctype}: {body[:500]}")
        return body


def resolve(api, titles):
    """Each title's normalized form and page info; missing and invalid ones reported."""
    found, missing = [], []
    for i in range(0, len(titles), 50):
        data = api.get(action="query", titles="|".join(titles[i:i + 50]), prop="info")
        if "error" in data:
            raise RuntimeError(data["error"])
        q = data.get("query", {})
        for p in q.get("pages", []):
            if p.get("missing") or p.get("invalid"):
                missing.append(p.get("title") or p.get("invalidreason"))
            else:
                found.append({"pageid": p["pageid"], "ns": p["ns"], "title": p["title"]})
    return found, missing


def dumped_pages(path):
    """(pageid, ns, title, newest revid, newest timestamp, revision count) per page of a dump."""
    opener = gzip.open if path.suffix == ".gz" else open
    pages = []
    with opener(path, "rb") as f:
        page = None
        for event, el in ET.iterparse(f, events=("start", "end")):
            tag = el.tag.removeprefix(EXPORT_NS)
            if event == "start":
                if tag == "page":
                    page = {"revisions": 0}
                continue
            if page is None:
                continue
            if tag == "revision":
                page["revisions"] += 1
                page["revid"] = int(el.findtext(EXPORT_NS + "id"))
                page["timestamp"] = el.findtext(EXPORT_NS + "timestamp")
                el.clear()
            elif tag in ("title", "ns", "id") and "revid" not in page and tag not in page:
                page[tag] = el.text if tag == "title" else int(el.text)
            elif tag == "page":
                pages.append({"pageid": page["id"], "ns": page["ns"], "title": page["title"],
                              "revid": page["revid"], "timestamp": page["timestamp"],
                              "revisions": page["revisions"]})
                page = None
                el.clear()
    return pages


# --- writing a history dump -----------------------------------------------------------------

def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;")


def base36(hex_sha1):
    """MediaWiki's dump form of a SHA-1: base 36, zero-padded to 31 digits."""
    n, digits = int(hex_sha1, 16), ""
    while n:
        n, d = divmod(n, 36)
        digits = "0123456789abcdefghijklmnopqrstuvwxyz"[d] + digits
    return digits.rjust(31, "0")


def revision_xml(r):
    out = ["    <revision>", f"      <id>{r['revid']}</id>"]
    if r.get("parentid"):
        out.append(f"      <parentid>{r['parentid']}</parentid>")
    out.append(f"      <timestamp>{r['timestamp']}</timestamp>")
    if r.get("userhidden"):
        out.append('      <contributor deleted="deleted" />')
    elif r.get("anon"):
        out += ["      <contributor>", f"        <ip>{esc(r['user'])}</ip>", "      </contributor>"]
    else:
        out += ["      <contributor>", f"        <username>{esc(r['user'])}</username>",
                f"        <id>{r.get('userid', 0)}</id>", "      </contributor>"]
    if r.get("minor"):
        out.append("      <minor />")
    if r.get("commenthidden"):
        out.append('      <comment deleted="deleted" />')
    elif r.get("comment"):
        out.append(f"      <comment>{esc(r['comment'])}</comment>")
    main = r.get("slots", {}).get("main", {})
    out += [f"      <origin>{r['revid']}</origin>",
            f"      <model>{esc(main.get('contentmodel', 'wikitext'))}</model>",
            f"      <format>{esc(main.get('contentformat', 'text/x-wiki'))}</format>"]
    sha1 = "" if r.get("sha1hidden") or not r.get("sha1") else base36(r["sha1"])
    sha1_attr = f' sha1="{sha1}"' if sha1 else ""
    if main.get("texthidden") or "content" not in main:
        out.append(f'      <text bytes="{r.get("size", 0)}"{sha1_attr} deleted="deleted" />')
    else:
        out.append(f'      <text bytes="{r.get("size", 0)}"{sha1_attr} xml:space="preserve">'
                   f"{esc(main['content'])}</text>")
    out.append(f"      <sha1>{sha1}</sha1>" if sha1 else "      <sha1 />")
    out.append("    </revision>")
    return "\n".join(out) + "\n"


def write_history(api, pages, path):
    """Every revision of `pages`, oldest first, as one export-0.11 dump."""
    current = api.export([p["title"] for p in pages])
    header = current[:current.index("  <page>")]
    root = ET.fromstring(current)
    redirects = {}
    for el in root.iter(EXPORT_NS + "page"):
        r = el.find(EXPORT_NS + "redirect")
        if r is not None:
            redirects[int(el.findtext(EXPORT_NS + "id"))] = r.get("title")
    tmp = path.with_name(path.name + ".part")
    with gzip.open(tmp, "wt", encoding="utf-8", compresslevel=6) as f:
        f.write(header)
        for p in pages:
            f.write(f"  <page>\n    <title>{esc(p['title'])}</title>\n    <ns>{p['ns']}</ns>\n"
                    f"    <id>{p['pageid']}</id>\n")
            if p["pageid"] in redirects:
                f.write(f'    <redirect title="{esc(redirects[p["pageid"]])}" />\n')
            n = 0
            for q in api.query_all(prop="revisions", pageids=p["pageid"], rvprop=REV_PROPS,
                                   rvslots="main", rvlimit="max", rvdir="newer"):
                for qp in q.get("pages", []):
                    for r in qp.get("revisions", []):
                        f.write(revision_xml(r))
                        n += 1
                print(f"  {p['title']}: {n} revisions", file=sys.stderr, end="\r")
            print(f"  {p['title']}: {n} revisions", file=sys.stderr)
            f.write("  </page>\n")
        f.write("</mediawiki>\n")
    tmp.replace(path)


# --- DiscussionTools ------------------------------------------------------------------------

def count_items(items):
    headings = comments = 0
    stack = list(items)
    while stack:
        it = stack.pop()
        if it.get("type") == "heading":
            headings += 1
        elif it.get("type") == "comment":
            comments += 1
        stack.extend(it.get("replies", []))
    return headings, comments


def discussiontools(api, out, page, force):
    path = out / "discussiontools" / f"{page['pageid']}-{page['revid']}.json"
    data = None
    if not force and path.exists():
        data = json.loads(path.read_text(encoding="utf-8"))
        if "error" in data:
            data = None  # a failed lookup is tried again
    if data is None:
        data = api.get(action="discussiontoolspageinfo", page=page["title"], oldid=page["revid"],
                       prop=DT_PROPS)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(data, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    if "error" in data:
        return {"error": data["error"]}
    headings, comments = count_items(data["discussiontoolspageinfo"].get("threaditemshtml", []))
    return {"file": str(path.relative_to(out)), "headings": headings, "comments": comments}


# --------------------------------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out", type=Path, help="directory to write the dumps into")
    ap.add_argument("--api", default=DEFAULT_API, help=f"the wiki's api.php (default {DEFAULT_API})")
    ap.add_argument("--current", nargs="*", metavar="TITLE",
                    help="pages to dump at their latest revision (default: the built-in sample set; "
                         "give the flag alone for none)")
    ap.add_argument("--history", nargs="*", metavar="TITLE",
                    help="pages to dump with every revision (default: User talk:Harej; "
                         "give the flag alone for none)")
    ap.add_argument("--no-discussiontools", action="store_true",
                    help="skip the DiscussionTools lookups")
    ap.add_argument("--delay", type=float, default=0.2, help="seconds between requests (default 0.2)")
    ap.add_argument("--force", action="store_true", help="fetch again even where files exist")
    a = ap.parse_args()
    current_titles = DEFAULT_CURRENT if a.current is None else a.current
    history_titles = DEFAULT_HISTORY if a.history is None else a.history

    api = Api(a.api, a.delay)
    a.out.mkdir(parents=True, exist_ok=True)
    manifest_path = a.out / "manifest.json"
    old = json.loads(manifest_path.read_text(encoding="utf-8")) if manifest_path.exists() else {}

    general = api.get(action="query", meta="siteinfo", siprop="general")["query"]["general"]
    print(f"{general.get('sitename')} ({general.get('generator')})", file=sys.stderr)

    manifest = {"api": a.api, "sitename": general.get("sitename"),
                "generator": general.get("generator"), "dumps": {}, "missing": [], "pages": []}
    for name, titles, file in (("current", current_titles, "current.xml"),
                               ("history", history_titles, "history.xml.gz")):
        path = a.out / file
        if not titles:
            continue
        if a.force or not path.exists():
            found, missing = resolve(api, titles)
            manifest["missing"] += missing
            for t in missing:
                print(f"  missing: {t}", file=sys.stderr)
            if not found:
                continue
            print(f"{name}: {len(found)} page(s) -> {path}", file=sys.stderr)
            if name == "current":
                body = api.export([p["title"] for p in found])
                path.write_text(body if body.endswith("\n") else body + "\n", encoding="utf-8")
            else:
                write_history(api, found, path)
            fetched = now()
        else:
            print(f"{name}: keeping {path}", file=sys.stderr)
            fetched = old.get("dumps", {}).get(name, {}).get("fetched")
        manifest["dumps"][name] = {"file": file, "fetched": fetched}
        for p in dumped_pages(path):
            p["dump"] = name
            manifest["pages"].append(p)

    errors = 0
    for p in manifest["pages"]:
        if a.no_discussiontools:
            continue
        p["discussiontools"] = dt = discussiontools(api, a.out, p, a.force)
        if "error" in dt:
            errors += 1
            print(f"  {p['title']} ({p['revid']}): DiscussionTools: {dt['error'].get('code')}: "
                  f"{dt['error'].get('info')}", file=sys.stderr)
        else:
            print(f"  {p['title']} ({p['revid']}): {dt['headings']} headings, "
                  f"{dt['comments']} comments", file=sys.stderr)
    manifest["discussiontools_props"] = DT_PROPS

    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    revs = sum(p["revisions"] for p in manifest["pages"])
    print(f"{len(manifest['pages'])} pages, {revs} revisions, {len(manifest['missing'])} missing, "
          f"{errors} DiscussionTools errors -> {a.out}", file=sys.stderr)
    return 1 if errors or manifest["missing"] else 0


if __name__ == "__main__":
    sys.exit(main())
