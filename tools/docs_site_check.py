#!/usr/bin/env python3
"""Checks the documentation site's generated pages (0109 §1–4; 24 §16) without building it, so
`docs/check.sh` can run it where MkDocs is not installed; CI also builds the site strictly.

  python3 tools/docs_site_check.py      exit 1 on any problem
"""
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import docs_site as site  # noqa: E402

problems = []

# 0109 §1: the navigation names every chapter and every ADR, from the files that exist.
nav = site.on_config({})["nav"]
flat = json.dumps(nav)
for _, f, _ in site.chapters():
    if f"architecture/{f}" not in flat or f"built/{f}" not in flat:
        problems.append(f"the navigation lacks chapter {f} or its As built page")
for f in site.decisions():
    if f"decisions/{f}" not in flat:
        problems.append(f"the navigation lacks {f}")

# 0109 §2: what the site generates is never committed.
for uri in ["index.md", "reference/rest.md", "built/index.md"] + [f"built/{f}" for _, f, _ in site.chapters()]:
    if os.path.exists(os.path.join(site.DOCS, uri)):
        problems.append(f"docs/{uri} is committed, but the site generates it")

# 0109 §3: an As built page per chapter, with every numbered heading of the chapter.
for n, f, title in site.chapters():
    page = site.built_chapter(n, f, title)
    for num, level, heading in site.chapter_sections(f):
        if f"{'#' * level} {num}. {heading}" not in page:
            problems.append(f"built/{f} lacks §{num}")

# 0109 §4: the REST reference has an entry for every operation of the contract.
ref = site.rest()
with open(os.path.join(site.DOCS, "api", "triplespace-v0.openapi.json"), encoding="utf-8") as fh:
    contract = json.load(fh)
for path, item in contract["paths"].items():
    for method in ("get", "post", "put", "patch", "delete"):
        if method in item and f"## `{method.upper()} {path}`" not in ref:
            problems.append(f"the REST reference lacks {method.upper()} {path}")

for p in problems:
    print("   -", p)
if problems:
    sys.exit(1)
print(f"ok: the site's navigation, {len(site.chapters())} As built pages and the REST reference are generated, none committed")
