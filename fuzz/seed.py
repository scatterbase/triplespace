#!/usr/bin/env python3
"""Seeds each fuzz target's corpus from the fixtures already in the repository (22 §4.9).

  fuzz/seed.py            fill fuzz/corpus/<target>/ for every target
  fuzz/seed.py wikitext   one target

The corpus directories are gitignored: libFuzzer writes what it finds there. Seeds are
copied, never moved, so a run can be repeated from the same start.

  wikitext     every revision of Librarybase's wikitext pages (docs/samples/librarybase/parse)
               and the head of each talk page of docs/samples/enwiki-talk/current.xml.bz2
  markdown     the repository's own Markdown (README, docs/) and fuzz/seeds/markdown/
  cbor_record  every CBOR byte string of the log test vectors (docs/api/vectors/log-v1.json)

Files over MAX_SEED bytes are skipped: libFuzzer caps inputs at `-max_len` (run.sh), and a
seed it would truncate is better left out.
"""

import bz2
import json
import shutil
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "fuzz" / "corpus"
MAX_SEED = 32 * 1024


def put(target: str, name: str, data: bytes) -> bool:
    if not data or len(data) > MAX_SEED:
        return False
    (CORPUS / target).mkdir(parents=True, exist_ok=True)
    (CORPUS / target / name).write_bytes(data)
    return True


def wikitext() -> int:
    n = 0
    pages = ROOT / "docs/samples/librarybase/parse/pages"
    for wiki in sorted(pages.glob("*/*.wiki")):
        n += put("wikitext", f"librarybase-{wiki.parent.name}-{wiki.stem}.wiki", wiki.read_bytes())
    dump = ROOT / "docs/samples/enwiki-talk/current.xml.bz2"
    if dump.exists():
        ns = {"mw": "http://www.mediawiki.org/xml/export-0.11/"}
        root = ET.fromstring(bz2.decompress(dump.read_bytes()))
        tag = root.tag.split("}")[0] + "}" if root.tag.startswith("{") else ""
        for i, page in enumerate(root.iter(f"{tag}page")):
            for text in page.iter(f"{tag}text"):
                # Talk pages run to hundreds of kilobytes: the head of each, cut at a
                # character boundary, still carries signatures, indentation and templates.
                head = (text.text or "")[:MAX_SEED].encode()[:MAX_SEED]
                n += put("wikitext", f"enwiki-talk-{i}-head.wiki", head.decode("utf-8", "ignore").encode())
    return n


def markdown() -> int:
    n = 0
    files = [ROOT / "README.md", ROOT / "SECURITY.md", *sorted((ROOT / "docs").rglob("*.md"))]
    files += sorted((ROOT / "fuzz/seeds/markdown").glob("*.md"))
    for f in files:
        if f.exists() and "node_modules" not in f.parts:
            rel = f.relative_to(ROOT)
            n += put("markdown", "-".join(rel.parts), f.read_bytes())
    return n


def cbor_record() -> int:
    n = 0
    doc = json.loads((ROOT / "docs/api/vectors/log-v1.json").read_text())

    def walk(value, name):
        nonlocal n
        if isinstance(value, dict):
            for k, v in value.items():
                walk(v, f"{name}-{k}")
        elif isinstance(value, list):
            for i, v in enumerate(value):
                walk(v, f"{name}-{i}")
        elif isinstance(value, str) and len(value) % 2 == 0 and len(value) >= 2:
            try:
                raw = bytes.fromhex(value)
            except ValueError:
                return
            n += put("cbor_record", f"{name}.cbor", raw)

    for case in doc.get("cbor", []):
        walk(case["expect"], f"cbor-{case['name'].replace(' ', '_')}")
    for case in doc.get("records", []):
        walk(case["expect"], f"record-{case['name'].replace(' ', '_')}")
    walk(doc.get("tree", {}).get("expect", {}), "tree")
    return n


TARGETS = {"wikitext": wikitext, "markdown": markdown, "cbor_record": cbor_record}


def main(argv):
    wanted = argv[1:] or list(TARGETS)
    for t in wanted:
        if t not in TARGETS:
            sys.exit(f"unknown target {t!r}; targets: {', '.join(TARGETS)}")
        n = TARGETS[t]()
        print(f"{t}: {n} seeds in {CORPUS / t}")


if __name__ == "__main__":
    main(sys.argv)
