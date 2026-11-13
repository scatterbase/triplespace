"""MkDocs hooks for the documentation site (0109; 24 §16): mkdocs.yml names this file.
Product code (24 §15): the site, with mkdocs.yml and .readthedocs.yaml (implements 0109 §1).

Every page this file adds is generated when the site is built, from the source it describes,
and is never written to the repository, so no copy of it can fall out of date (0109 §2):

  index.md           the home page: the opening of the repository's README.md, and the sections
  built/index.md     what the code says is built, chapter by chapter (0109 §3)
  built/NN-*.md      one per architecture chapter: each section's ADR sections, their status as
                     tools/coverage.py computes it, the code that claims each, and the code's own
                     "not yet built" comments, linked to the commit the site was built from
  reference/rest.md  the REST API, from docs/api/triplespace-v0.openapi.json (0109 §4)

and the navigation, built from the files themselves (on_config). Nothing here holds content of
its own beyond the frame around what it reads.

Run `mkdocs serve` or `mkdocs build` at the repository root (requirements: docs/requirements.txt).
"""
import collections
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOCS = os.path.join(ROOT, "docs")
REPO = "https://github.com/scatterbase/triplespace"

sys.path.insert(0, os.path.join(ROOT, "tools"))
import coverage  # noqa: E402  the same computation as `tools/coverage.py` (24 §15)

_cache = {}


def commit():
    """The commit the site is built from: Read the Docs says, else git does."""
    if "commit" not in _cache:
        c = os.environ.get("READTHEDOCS_GIT_COMMIT_HASH")
        if not c:
            try:
                c = subprocess.run(["git", "-C", ROOT, "rev-parse", "HEAD"], capture_output=True,
                                   text=True).stdout.strip()
            except OSError:
                c = ""
        _cache["commit"] = c or "main"
    return _cache["commit"]


def code_link(rel, line):
    return f"[`{rel}:{line}`]({REPO}/blob/{commit()}/{rel}#L{line})"


def analysis():
    if "analysis" not in _cache:
        docs = coverage.adr_docs()
        ch_sources = coverage.chapter_sources()
        cites = coverage.scan()
        rolled, _ = coverage.index(docs, cites, ch_sources)
        _cache["analysis"] = (docs, ch_sources, rolled)
    return _cache["analysis"]


def chapters():
    """[(number, file name, title)] of the architecture chapters, in order."""
    out = []
    for f in sorted(os.listdir(os.path.join(DOCS, "architecture"))):
        m = re.match(r"^(\d\d)-.*\.md$", f)
        if m:
            with open(os.path.join(DOCS, "architecture", f), encoding="utf-8") as fh:
                title = fh.readline().lstrip("# ").strip()
            out.append((m.group(1), f, title))
    return out


def decisions():
    out = []
    for f in sorted(os.listdir(os.path.join(DOCS, "decisions"))):
        if re.match(r"^\d{4}-.*\.md$", f):
            out.append(f)
    return out


def md_files(sub):
    d = os.path.join(DOCS, sub)
    return sorted(f for f in os.listdir(d) if f.endswith(".md")) if os.path.isdir(d) else []


# --- the navigation ------------------------------------------------------------------------------

def on_config(config):
    """The navigation, from the files that exist: nothing to keep in step by hand."""
    arch = [{"Overview": "architecture/README.md"}] if os.path.exists(os.path.join(DOCS, "architecture", "README.md")) else []
    arch += [{f"{n}. {t.split('. ', 1)[-1]}": f"architecture/{f}"} for n, f, t in chapters()]
    built = [{"Overview": "built/index.md"}] + [{f"{n}. {t.split('. ', 1)[-1]}": f"built/{f}"} for n, f, t in chapters()]
    adrs = [{"Index": "decisions/INDEX.md"}, {"Open": "decisions/OPEN.md"}] + [f"decisions/{f}" for f in decisions()]
    api = [f"api/{f}" for f in md_files("api")] + [{"REST reference": "reference/rest.md"}]
    testing = [f"testing/{f}" for f in md_files("testing")]
    # The guides' README is the section's own page; the quickstart comes first, the rest by name.
    guides = [{"Overview": "guides/README.md"}] if os.path.exists(os.path.join(DOCS, "guides", "README.md")) else []
    guides += [f"guides/{f}" for f in sorted(md_files("guides"), key=lambda f: (f != "quickstart.md", f))
               if f != "README.md"]
    nav = [{"Home": "index.md"}, {"Guides": guides}, {"Architecture": arch}, {"As built": built}, {"Decisions": adrs},
           {"API": api}, {"Registry": [f"registry/{f}" for f in md_files("registry")]},
           {"Testing": testing}]
    others = [f for f in md_files("") if f not in ("index.md",)]
    if others:
        nav.append({"More": others})
    config["nav"] = nav
    return config


# --- the generated pages -------------------------------------------------------------------------

def on_files(files, config):
    """The generated pages, added to the build and never written to the repository
    (implements 0109 §2)."""
    from mkdocs.structure.files import File
    pages = {"index.md": home(), "built/index.md": built_index(), "reference/rest.md": rest()}
    for n, f, title in chapters():
        pages[f"built/{f}"] = built_chapter(n, f, title)
    for uri, content in pages.items():
        files.append(File.generated(config, uri, content=content))
    return files


def home():
    with open(os.path.join(ROOT, "README.md"), encoding="utf-8") as fh:
        readme = fh.read()
    # The README's opening, up to its first section, with its links made to work from here.
    opening = readme.split("\n## ", 1)[0]

    def link(m):
        text, target = m.group(1), m.group(2)
        if re.match(r"^[a-z]+:", target) or target.startswith("#"):
            return m.group(0)
        if target.startswith("docs/"):
            return f"[{text}]({target[len('docs/'):]})"
        return f"[{text}]({REPO}/blob/main/{target})"

    opening = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", link, opening)
    return opening + f"""

## This site

Built from the repository's `docs/` at [`{commit()[:12]}`]({REPO}/commit/{commit()}); every page
is the file of the same name there, except the pages this build generates from their sources
(0109 §2), which are never committed:

- **[Guides](guides/README.md)**: running Triplespace, step by step, from the
  [Quickstart](guides/quickstart.md) to [moving an instance](guides/moving-an-instance.md).
- **[Architecture](architecture/README.md)**: what the design holds now, by subject.
- **[As built](built/index.md)**: what the code says is built of each chapter, section by
  section, with the code that claims it (generated, from the code's `implements` claims).
- **[Decisions](decisions/INDEX.md)**: why and when, the ADRs.
- **[REST reference](reference/rest.md)**: the REST API's operations (generated, from the
  OpenAPI contract).
"""


STATUS = {"done": "done", "amended": "amended since", "untested": "claimed, untested",
          "partial": "partial", "not": "not built", "none": "not cited", "trailer": "trailer",
          "stub": "superseded"}


def section_rows(adr, num):
    docs, _, rolled = analysis()
    d = docs[adr]
    s = d["sections"][num]
    if s.stub:
        return "stub", "", [], []
    if s.trailer:
        status, detail = "trailer", ""
    else:
        status, detail = coverage.section_status(docs, rolled, adr, num)
    cs = rolled.get((adr, num), [])
    claims = sorted({(c.rel, c.line) for c in cs if c.claim and c.kind == "src"})
    nots = sorted({(c.rel, c.line, c.text) for c in cs if c.kind == "not"})
    return status, detail, claims, nots


def adr_link(adr, num, prefix="../decisions/"):
    docs, _, _ = analysis()
    f = os.path.basename(docs[adr]["path"])
    return f"[{adr} §{num}]({prefix}{f})"


def chapter_sections(f):
    """[(number, level, title)] of a chapter's numbered headings."""
    out = []
    with open(os.path.join(DOCS, "architecture", f), encoding="utf-8") as fh:
        for line in fh:
            m = re.match(r"^(#{2,3}) (\d+(?:\.\d+)*)\.?\s+(.*)$", line.rstrip())
            if m:
                out.append((m.group(2), len(m.group(1)), m.group(3)))
    return out


def counts_for(n, f):
    _, ch_sources, _ = analysis()
    tally = collections.Counter()
    for num, _, _ in chapter_sections(f):
        for adr, s in sorted(ch_sources.get((n, num), ())):
            if adr in analysis()[0] and s in analysis()[0][adr]["sections"]:
                tally[section_rows(adr, s)[0]] += 1
    return tally


def built_index():
    lines = ["# As built", "",
             "What the code says is built of each architecture chapter, from the `implements` claims "
             "and citations in its comments ([24](../architecture/24-documentation.md) §15), as "
             "`tools/coverage.py` computes it. A section is **done** when the code claims it in "
             "full, a test cites it, and no amendment has changed it since. Generated when this "
             f"site was built, from [`{commit()[:12]}`]({REPO}/commit/{commit()}).", "",
             "| Chapter | Done | Amended since | Claimed, untested | Partial | Not built | Not cited |",
             "|---|---|---|---|---|---|---|"]
    for n, f, title in chapters():
        t = counts_for(n, f)
        lines.append(f"| [{title}]({f}) | {t['done']} | {t['amended']} | {t['untested']} | "
                     f"{t['partial']} | {t['not']} | {t['none']} |")
    lines += ["", "Counts are of ADR sections as each chapter section cites them, so a section a "
              "chapter cites twice counts twice; trailers and superseded sections are left out."]
    return "\n".join(lines) + "\n"


def built_chapter(n, f, title):
    """A chapter's As built page: each numbered heading's ADR sections, their status, the
    code that claims them and its "not yet built" notes, and what the code says at each
    (implements 0109 §3)."""
    _, ch_sources, _ = analysis()
    docs = analysis()[0]
    lines = [f"# As built: {title}", "",
             f"What the code says is built of [{title}](../architecture/{f}), section by section "
             "([24](../architecture/24-documentation.md) §15): for each ADR section a chapter "
             "section holds, its status, the code that claims it, and the code's own \"not yet "
             "built\" comments. Generated when this site was built, from "
             f"[`{commit()[:12]}`]({REPO}/commit/{commit()}); never committed.", ""]
    for num, level, heading in chapter_sections(f):
        srcs = sorted(p for p in ch_sources.get((n, num), ()) if p[0] in docs and p[1] in docs[p[0]]["sections"])
        lines.append(f"{'#' * level} {num}. {heading}")
        lines.append("")
        if not srcs:
            lines += ["Cites no ADR section.", ""]
            continue
        lines += ["| ADR section | Status | Claimed at | Not built yet |", "|---|---|---|---|"]
        said = []
        for adr, s in srcs:
            status, detail, claims, nots = section_rows(adr, s)
            label = STATUS.get(status, status) + (f" ({detail})" if detail else "")
            claimed = "<br>".join(code_link(r, l) for r, l in claims) or ""
            pending = "<br>".join(code_link(r, l) for r, l, _ in nots) or ""
            # A title's own links are relative to its ADR; here they would break.
            title = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", docs[adr]["sections"][s].title)
            lines.append(f"| {adr_link(adr, s)} {title} | {label} | {claimed} | {pending} |")
            said += [(r, l, "") for r, l in claims] + [(r, l, "not yet") for r, l, _ in nots]
        lines.append("")
        # What the code says, in its own words: the comment at each claim and at each
        # "not yet built" note, which changes with the code it sits beside.
        seen = set()
        for r, l, kind in said:
            if (r, l) in seen:
                continue
            seen.add((r, l))
            text = comment_at(r, l)
            if text:
                tag = " (not yet built)" if kind else ""
                lines += [f"- {code_link(r, l)}{tag}: {text}", ""]
    return "\n".join(lines) + "\n"


def comment_at(rel, line):
    """The comment paragraph at a line of code, as rustdoc's reader would see it."""
    try:
        with open(os.path.join(ROOT, rel), encoding="utf-8") as fh:
            lines = fh.read().splitlines()
    except OSError:
        return ""
    if not 0 < line <= len(lines):
        return ""
    text, _ = coverage.paragraph(lines, line - 1)
    if not coverage.COMMENT.match(lines[line - 1]):
        return ""
    # Intra-doc links read as code here: [`Thing`] and [`mod@thing`].
    text = re.sub(r"\[`(?:\w+@)?([^`\]]+)`\](?!\()", r"`\1`", text)
    return text.replace("|", "\\|")


def rest():
    """The REST reference, one entry per operation of the OpenAPI contract: this
    implements 0109 §4."""
    path = os.path.join(DOCS, "api", "triplespace-v0.openapi.json")
    with open(path, encoding="utf-8") as fh:
        doc = json.load(fh)
    info = doc.get("info", {})
    lines = ["# REST reference", "",
             f"The `triplespace/v0` REST API, version {info.get('version', '?')}, generated when this "
             "site was built from the OpenAPI contract, "
             f"[`docs/api/triplespace-v0.openapi.json`]({REPO}/blob/{commit()}/docs/api/triplespace-v0.openapi.json), "
             "which the server's tests hold to the router operation by operation. The bases are "
             "`/w/rest.php/triplespace/v0` and `/rest.php/triplespace/v0`. The routes the design "
             "names, built or not, are the route catalogue of "
             "[18](../architecture/18-api.md) §3.2.", ""]
    if info.get("description"):
        lines += [info["description"], ""]
    for p in sorted(doc.get("paths", {})):
        item = doc["paths"][p]
        for method in ("get", "post", "put", "patch", "delete"):
            op = item.get(method)
            if not op:
                continue
            lines.append(f"## `{method.upper()} {p}`")
            lines.append("")
            if op.get("summary"):
                lines += [op["summary"], ""]
            if op.get("description"):
                lines += [op["description"], ""]
            params = op.get("parameters", []) + item.get("parameters", [])
            if params:
                lines += ["| Parameter | In | Required | Description |", "|---|---|---|---|"]
                for prm in params:
                    if "$ref" in prm:
                        ref = prm["$ref"].split("/")[-1]
                        prm = doc.get("components", {}).get("parameters", {}).get(ref, {"name": ref})
                    desc = (prm.get("description") or "").replace("\n", " ").replace("|", "\\|")
                    lines.append(f"| `{prm.get('name', '?')}` | {prm.get('in', '')} | "
                                 f"{'yes' if prm.get('required') else ''} | {desc} |")
                lines.append("")
            responses = op.get("responses", {})
            if responses:
                lines.append("Responses: " + ", ".join(
                    f"`{code}`" + (f" {r.get('description')}" if isinstance(r, dict) and r.get("description") else "")
                    for code, r in responses.items()) + ".")
                lines.append("")
    return "\n".join(lines) + "\n"
