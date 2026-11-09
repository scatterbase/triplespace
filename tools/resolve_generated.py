#!/usr/bin/env python3
"""Resolve a merge or rebase conflict in generated files by regenerating them (AGENTS.md,
"Generated files"; 24 §11, §13, §15).

  python3 tools/resolve_generated.py          resolve, regenerate, check, and stage the result
  python3 tools/resolve_generated.py --dry    say what it would do, change nothing

Run it while a `git merge` or `git rebase` has stopped on conflicts (in a rebase, at each stop,
then `git rebase --continue`). It resolves two kinds of conflicted file and refuses any other:

  wholly generated   a file `.gitattributes` marks `linguist-generated=true` (INDEX.md, OPEN.md,
                     CRATES.md): either side is taken.
  generated blocks   a Markdown file whose every conflict lies inside a generated block, between
                     `<!-- NAME.py: … -->` and `<!-- /NAME.py -->` (the chapter tables, and the
                     status tables of 24 §15): each conflict takes either side.

Then `docs/check.sh --write` rebuilds every generated file from its sources, `docs/check.sh`
must pass, and the resolved and regenerated files are staged. Which side was taken does not
matter, since every generated line is rewritten from the sources; that is also why a conflict in
a source (a registry file, an ADR, a chapter's hand-written text, code) is never resolved here.

Exit 0 when every conflict was resolved and the checks pass; 1 otherwise, with each unresolved
file named and nothing staged. The workflow `.github/workflows/generated-files.yml` runs this
on pull requests when `main` moves.
"""
import os
import re
import subprocess
import sys

# The repository in the current directory, not this file's: update_generated.sh runs a copy.
ROOT = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True).stdout.strip() or os.getcwd()
DRY = "--dry" in sys.argv[1:]
BEGIN = re.compile(r"^<!-- [\w.-]+\.py: .* -->$")
END = re.compile(r"^<!-- /[\w.-]+\.py -->$")


def git(*args, check=True):
    out = subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True)
    if check and out.returncode != 0:
        sys.exit(f"resolve_generated: git {' '.join(args)} failed: {out.stderr.strip()}")
    return out.stdout


def wholly_generated():
    path = os.path.join(ROOT, ".gitattributes")
    out = set()
    if os.path.exists(path):
        for line in open(path, encoding="utf-8"):
            parts = line.split()
            if len(parts) >= 2 and not line.startswith("#") and "linguist-generated=true" in parts[1:]:
                out.add(parts[0])
    return out


def resolve_blocks(text):
    """The text with every conflict taken from the incoming side, or None if a conflict lies
    outside a generated block (or crosses a block's edge)."""
    out, inside, lines, i = [], False, text.split("\n"), 0
    while i < len(lines):
        line = lines[i]
        if line.startswith("<<<<<<< "):
            if not inside:
                return None
            ours, base, theirs, part = [], [], [], "ours"
            i += 1
            while i < len(lines) and not lines[i].startswith(">>>>>>> "):
                l = lines[i]
                if l.startswith("||||||| "):
                    part = "base"
                elif l == "=======":
                    part = "theirs"
                else:
                    {"ours": ours, "base": base, "theirs": theirs}[part].append(l)
                i += 1
            if i == len(lines):
                return None
            if any(BEGIN.match(l.strip()) or END.match(l.strip()) for l in ours + base + theirs):
                return None
            out.extend(theirs)
            i += 1
            continue
        if BEGIN.match(line.strip()):
            inside = True
        elif END.match(line.strip()):
            inside = False
        out.append(line)
        i += 1
    return "\n".join(out)


def changed():
    return set(git("diff", "--name-only").split())


def main():
    conflicted = git("diff", "--name-only", "--diff-filter=U").split()
    if not conflicted:
        print("resolve_generated: no conflicted files")
    generated = wholly_generated()
    plan, refused = [], []
    for f in conflicted:
        if f in generated:
            plan.append((f, "whole"))
        elif f.endswith(".md"):
            text = open(os.path.join(ROOT, f), encoding="utf-8").read()
            new = resolve_blocks(text)
            if new is None:
                refused.append(f)
            else:
                plan.append((f, new))
        else:
            refused.append(f)
    if refused:
        print("resolve_generated: conflicts outside generated files; resolve these by hand, then rerun:")
        for f in refused:
            print("   -", f)
        sys.exit(1)
    for f, how in plan:
        print(f"resolve_generated: {f}: {'either side, regenerated' if how == 'whole' else 'conflicts inside generated blocks, regenerated'}")
    if DRY:
        return
    for f, how in plan:
        if how == "whole":
            # A side that deleted the file has no version to take: take the other.
            r = subprocess.run(["git", "checkout", "--theirs", "--", f], cwd=ROOT, capture_output=True)
            if r.returncode != 0:
                git("checkout", "--ours", "--", f)
        else:
            with open(os.path.join(ROOT, f), "w", encoding="utf-8") as fh:
                fh.write(how)
    before = changed()
    check = os.path.join(ROOT, "docs", "check.sh")
    for args in ([check, "--write"], [check]):
        r = subprocess.run(args, cwd=ROOT, capture_output=True, text=True)
        if r.returncode != 0:
            print(r.stdout[-4000:], r.stderr[-2000:], sep="\n")
            sys.exit(f"resolve_generated: `{' '.join(os.path.relpath(a, ROOT) for a in args)}` failed; nothing staged")
    regenerated = changed() - before
    stage = sorted({f for f, _ in plan} | regenerated)
    if stage:
        git("add", "--", *stage)
    print(f"resolve_generated: staged {len(stage)} file(s); the documentation checks pass")


if __name__ == "__main__":
    main()
