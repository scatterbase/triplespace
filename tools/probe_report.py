#!/usr/bin/env python3
"""What the running code used from the catalogues, against the catalogues (0107 §1–3; 0105 §2;
24 §11).

  python3 tools/probe_report.py PROBE_FILE...            report; exit 0
  python3 tools/probe_report.py --strict PROBE_FILE...   exit 1 on anything unlisted, or an empty probe

A probe file is what `triplespace --probe FILE` and `triplespace-server --probe FILE` write
(`TRIPLESPACE_PROBE`): one `{kind} {name}` line per name a process used, several processes
appending to one file. Each kind is compared with the registry files that list it:

  permission   docs/registry/permissions.toml (the catalogue, 09 §2.2) and the [permissions]
               lists of docs/registry/groups.toml (the names the code embeds)
  setting      docs/registry/settings.toml (23 §1), keyed by `key`
  log-event    docs/registry/log-events.toml (16 §2.3), `type/action`; a `type/*` entry covers
               every action of the type

and for each the report names:

  unlisted       used, but missing from a file that lists the kind: the code does something the
                 documentation does not say, or misspells it
  not observed   listed, but not used in these runs: not built, or not exercised by them
  used           listed and used

A run observes only the paths its tests take, so "not observed" is not "not built". With
--strict, anything unlisted, or a probe with no lines at all, fails the run. When
GITHUB_STEP_SUMMARY is set the report is also appended there, for the CI run's summary page.
"""
import os
import sys

try:
    import tomllib
except ModuleNotFoundError:   # Python < 3.11: put docs/tomli-*.whl on PYTHONPATH, as docs/check.sh does
    import tomli as tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REG = os.path.join(ROOT, "docs", "registry")


def load(name):
    with open(os.path.join(REG, name), "rb") as fh:
        return tomllib.load(fh)


def catalogues():
    """{kind: [(file label, set of names)]}: every file that lists each kind."""
    perms = {n for r in load("permissions.toml").get("permission", []) for n in r.get("names", [])}
    groups = {n for v in load("groups.toml").get("permissions", {}).values() if isinstance(v, list) for n in v}
    settings = {r["key"] for r in load("settings.toml").get("setting", []) if "key" in r}
    events = {n for r in load("log-events.toml").get("event", []) for n in r.get("events", [])}
    return {
        "permission": [("`permissions.toml`", perms), ("`groups.toml` [permissions]", groups)],
        "setting": [("`settings.toml`", settings)],
        "log-event": [("`log-events.toml`", events)],
    }


def listed(kind, name, names):
    if name in names:
        return True
    if kind == "log-event" and "/" in name:
        return f"{name.split('/', 1)[0]}/*" in names
    return False


def observed(paths):
    out = {}
    for p in paths:
        try:
            with open(p, encoding="utf-8") as fh:
                for line in fh:
                    kind, _, name = line.strip().partition(" ")
                    if name:
                        out.setdefault(kind, set()).add(name)
        except FileNotFoundError:
            print(f"probe_report: no probe file {p}", file=sys.stderr)
    return out


TITLES = {"permission": "Permissions checked", "setting": "Settings read", "log-event": "Log events written"}


def report(seen, cats):
    lines = ["## What the code used from the catalogues (0107; 24 §11)", ""]
    problems = []
    if not seen:
        lines += ["**Nothing was observed.** The probe was not on, or the runs used nothing.", ""]
        problems.append("empty probe")
    for kind, files in cats.items():
        used = seen.get(kind, set())
        unlisted = sorted(n for n in used if not all(listed(kind, n, names) for _, names in files))
        union = set().union(*(names for _, names in files))
        quiet = sorted(n for n in union if not n.endswith("/*") and n not in used)
        lines.append(f"### {TITLES[kind]}")
        lines.append("")
        lines.append(f"{len(used) - len(unlisted)} listed used, {len(unlisted)} unlisted, "
                     f"{len(quiet)} listed but not observed.")
        lines.append("")
        if unlisted:
            lines += ["| Unlisted | Missing from |", "|---|---|"]
            for n in unlisted:
                missing = [label for label, names in files if not listed(kind, n, names)]
                lines.append(f"| `{n}` | {', '.join(missing)} |")
            lines.append("")
            problems += [f"{kind} {n}" for n in unlisted]
        if quiet:
            lines += ["<details><summary>Not observed: not built, or not exercised by these runs</summary>", "",
                      ", ".join(f"`{n}`" for n in quiet), "", "</details>", ""]
    others = sorted(set(seen) - set(cats))
    if others:
        lines += [f"Lines of an unknown kind were ignored: {', '.join(others)}.", ""]
    return "\n".join(lines), problems


def main(argv):
    strict = "--strict" in argv
    paths = [a for a in argv if not a.startswith("--")]
    if not paths:
        sys.exit("usage: probe_report.py [--strict] PROBE_FILE...")
    text, problems = report(observed(paths), catalogues())
    print(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as fh:
            fh.write(text + "\n")
    if strict and problems:
        sys.exit(1)


if __name__ == "__main__":
    main(sys.argv[1:])
