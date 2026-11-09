#!/usr/bin/env python3
"""The permissions the running server checked, against the catalogue (0105 §1–2; 09 §2.2).

  python3 tools/permission_report.py PROBE_FILE...            report; exit 0
  python3 tools/permission_report.py --strict PROBE_FILE...   exit 1 on an unlisted permission

A probe file is what `triplespace-server --probe-permissions FILE` writes: every permission name
it checked, one per line (several servers may append to one file). The report compares that set
with the two lists of permissions the repository keeps:

  catalogue   docs/registry/permissions.toml, what each permission allows (09 §2.2)
  registry    the [permissions] lists of docs/registry/groups.toml, the names the code embeds

and names three things:

  unlisted       checked, but missing from the catalogue or the registry: the code asks about a
                 permission the documentation does not describe, or a typo that no group grants
  not observed   catalogued, but not checked in these runs: not built, or not exercised by them
  checked        catalogued and checked

Without --strict it only reports (0105 §1: a new check starts as a report); with it, an unlisted
permission or an empty probe fails the run. A run observes only the paths its tests exercise, so
"not observed" is not "not built". When GITHUB_STEP_SUMMARY is set the report is also appended
there, so a CI run shows it on its summary page.
"""
import os
import sys

try:
    import tomllib
except ModuleNotFoundError:   # Python < 3.11: put docs/tomli-*.whl on PYTHONPATH, as docs/check.sh does
    import tomli as tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REG = os.path.join(ROOT, "docs", "registry")


def catalogue():
    with open(os.path.join(REG, "permissions.toml"), "rb") as fh:
        rows = tomllib.load(fh).get("permission", [])
    return {n for r in rows for n in r.get("names", [])}


def registry():
    with open(os.path.join(REG, "groups.toml"), "rb") as fh:
        lists = tomllib.load(fh).get("permissions", {})
    return {n for v in lists.values() if isinstance(v, list) for n in v}


def observed(paths):
    out = set()
    for p in paths:
        try:
            with open(p, encoding="utf-8") as fh:
                out |= {line.strip() for line in fh if line.strip()}
        except FileNotFoundError:
            print(f"permission_report: no probe file {p}", file=sys.stderr)
    return out


def report(seen, cat, reg):
    unlisted = sorted(seen - (cat & reg))
    lines = ["## Permissions the server checked (0105 §2)", ""]
    if not seen:
        lines += ["**No permission checks were observed.** The probe was not on, or the runs checked nothing.", ""]
    lines.append(f"{len(seen & cat & reg)} of {len(cat)} catalogued permissions were checked; "
                 f"{len(unlisted)} checked permission(s) are unlisted.")
    lines.append("")
    if unlisted:
        lines += ["### Unlisted", "", "| Permission | Missing from |", "|---|---|"]
        for p in unlisted:
            missing = [f for f, s in (("`permissions.toml`", cat), ("`groups.toml` [permissions]", reg)) if p not in s]
            lines.append(f"| `{p}` | {', '.join(missing)} |")
        lines.append("")
    quiet = sorted(cat - seen)
    if quiet:
        lines += ["### Not observed", "",
                  "Catalogued but not checked in these runs: not built, or not exercised by them.", "",
                  ", ".join(f"`{p}`" for p in quiet), ""]
    return "\n".join(lines), unlisted


def main(argv):
    strict = "--strict" in argv
    paths = [a for a in argv if not a.startswith("--")]
    if not paths:
        sys.exit(__doc__.strip().split("\n\n")[1])
    seen = observed(paths)
    text, unlisted = report(seen, catalogue(), registry())
    print(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as fh:
            fh.write(text + "\n")
    if strict and (unlisted or not seen):
        sys.exit(1)


if __name__ == "__main__":
    main(sys.argv[1:])
