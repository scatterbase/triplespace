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
"""
import os, re, sys, glob, collections

ROOT = sys.argv[1] if len(sys.argv) > 1 else "."
DEC = os.path.join(ROOT, "decisions")
files = sorted(glob.glob(os.path.join(DEC, "0*.md")))
texts = {os.path.basename(f): open(f, encoding="utf-8").read() for f in files}
compat = {
    "wikibase-compat.md": open(os.path.join(ROOT, "api", "wikibase-compat.md"), encoding="utf-8").read(),
    "mediawiki-compat.md": open(os.path.join(ROOT, "api", "mediawiki-compat.md"), encoding="utf-8").read(),
}

def sections(text):
    s = set()
    for m in re.finditer(r"^#{2,4}\s+(\d+(?:\.\d+)?)[.\s]", text, re.M):
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
        tail = re.split(r"\[|\n|;", tail)[0]
        # inside a parenthetical "(§5 amends §4)" the first § is the *citing* ADR's section
        # when the verb is amends/settles/extends/refines/uses/follows; skip those
        paren = re.match(r"\s*\(([^)]*)\)", tail)
        cands = []
        if paren:
            inner = paren.group(1)
            # split on verbs: "§5 amends §4 and §8.2; §7 settles ..." -> after the verb are target §s
            for clause in re.split(r",|;| and (?=§)", inner):
                mm = re.search(r"(amends|settles|extends|refines|uses|follows|adds|builds on|specifies|answers|makes|registers|declares)\s+(.*)", clause)
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

# 3: amended-by / related symmetry
def header_field(text, field):
    m = re.search(rf"^- \*\*{field}:\*\*\s*(.*)$", text, re.M)
    return m.group(1) if m else ""

amended_by = {}
related = {}
for name, text in texts.items():
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
    rel = header_field(text, "Related")
    for m in re.finditer(r"\[(\d{4})[^\]]*\]\([^)]+\)\s*(\([^)]*\))?", rel):
        a = m.group(1)
        clause = m.group(2) or ""
        if re.search(r"amends|settles|extends|refines|adds|answers|fills|specifies", clause):
            af = num2file.get(a)
            if af and b[:4] not in amended_by[af]:
                problems[af].append(f"{b[:4]}'s Related says it {clause} {a}, but {a} does not list {b[:4]} under Amended-by")

# 4: crate names
table = texts[num2file["0005"]]
tbl_crates = set(re.findall(r"`((?:scatter|triplespace)-[a-z0-9-]+)`", table.split("### 2. Crate map")[1].split("### 3.")[0]))
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
    sp_known = {m.group(2).lower() for m in re.finditer(r'^(name|mediawiki_name) = "([^"]+)"', _t, re.M)}
    for m in re.finditer(r'^aliases = \[([^\]]*)\]', _t, re.M):
        sp_known |= {x.lower() for x in re.findall(r'"([^"]+)"', m.group(1))}
    for m in re.finditer(r'^section_aliases = \{(.*)\}$', _t, re.M):
        sp_known |= {x.lower() for x in re.findall(r'([A-Za-z]+) = ', m.group(1))}
    sources = dict(texts)
    sources.update(compat)
    for name, text in sources.items():
        for u in sorted(set(re.findall(r"Special:([A-Za-z]+)", text))):
            if u.lower() not in sp_known:
                problems[name].append(f"Special:{u} is not in registry/special-pages.toml")
else:
    print("== registry/special-pages.toml not found; check 5 skipped ==")

print("== per-ADR problems ==")
for name in sorted(texts):
    if problems[name]:
        print(name)
        for p in problems[name]:
            print("   -", p)
print("\ndone:", len(texts), "ADRs")
