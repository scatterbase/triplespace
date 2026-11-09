#!/usr/bin/env python3
"""What the running code used from the catalogues, against the catalogues (0107 §1–3; 0108 §1–2;
0105 §2; 24 §11).

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
  tenancy      the [switches] of docs/registry/tenancy.toml (08 §5.1), and the ceilings of the
               `bound = "ceiling"` settings of settings.toml (23 §3.1), 0108 §2
  module       docs/registry/action-api.toml (18 §2.3), `action=edit`, `list=blocks`; a bare
               name (`wbeditentity`) is an `action=` module, 0108 §1
  route        docs/registry/routes.toml (18 §3.2), `GET /account`; a bare path covers every
               method, and path parameters compare as `{}` whatever their names, 0108 §1

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
import re
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
    settings_rows = load("settings.toml").get("setting", [])
    # A switch, or the ceiling of a `bound = "ceiling"` setting, which is a tenancy record
    # keyed by the setting (or by its `ceiling` field) (23 §1.4, §3.1).
    switches = set(load("tenancy.toml").get("switches", {})) | {
        r.get("ceiling") or r["key"] for r in settings_rows if r.get("bound") == "ceiling" and "key" in r}
    modules = {m if "=" in m else f"action={m}"
               for r in load("action-api.toml").get("module", []) for m in r.get("modules", [])}
    routes = {norm_route(x) for r in load("routes.toml").get("route", []) for x in r.get("routes", [])}
    return {
        "permission": [("`permissions.toml`", perms), ("`groups.toml` [permissions]", groups)],
        "setting": [("`settings.toml`", settings)],
        "log-event": [("`log-events.toml`", events)],
        "tenancy": [("`tenancy.toml` [switches], or a ceiling in `settings.toml`", switches)],
        "module": [("`action-api.toml`", modules)],
        "route": [("`routes.toml`", routes)],
    }


def norm_route(r):
    """A route with every path parameter written `{}`, as catalogues.py compares them: the
    catalogue writes a parameter as its ADR does (`{page ID}`), the router as the code does."""
    m = re.match(r"^(?:(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s+)?(.+)$", r.strip())
    method, path = (m.group(1), m.group(2)) if m else (None, r.strip())
    path = re.sub(r"\{[^}]*\}", "{}", path.split("?")[0].rstrip("/") or "/")
    return f"{method} {path}" if method else path


def listed(kind, name, names):
    if name in names:
        return True
    if kind == "log-event" and "/" in name:
        return f"{name.split('/', 1)[0]}/*" in names
    if kind == "route" and " " in name:
        return name.split(" ", 1)[1] in names   # normalized: no space inside a path
    return False


def used(kind, entry, seen):
    """Whether a listed entry was used: a bare route path by any method."""
    if entry in seen:
        return True
    return kind == "route" and " " not in entry and any(s.split(" ", 1)[-1] == entry for s in seen)


def observed(paths):
    out = {}
    for p in paths:
        try:
            with open(p, encoding="utf-8") as fh:
                for line in fh:
                    kind, _, name = line.strip().partition(" ")
                    if kind == "route":
                        name = norm_route(name)
                    if name:
                        out.setdefault(kind, set()).add(name)
        except FileNotFoundError:
            print(f"probe_report: no probe file {p}", file=sys.stderr)
    return out


TITLES = {"permission": "Permissions checked", "setting": "Settings read", "log-event": "Log events written",
          "tenancy": "Tenancy switches read", "module": "Action API modules served", "route": "REST routes served"}


def report(seen, cats):
    lines = ["## What the code used from the catalogues (0107; 24 §11)", ""]
    problems = []
    if not seen:
        lines += ["**Nothing was observed.** The probe was not on, or the runs used nothing.", ""]
        problems.append("empty probe")
    for kind, files in cats.items():
        got = seen.get(kind, set())
        unlisted = sorted(n for n in got if not all(listed(kind, n, names) for _, names in files))
        union = set().union(*(names for _, names in files))
        quiet = sorted(n for n in union if not n.endswith("/*") and not used(kind, n, got))
        lines.append(f"### {TITLES[kind]}")
        lines.append("")
        lines.append(f"{len(got) - len(unlisted)} listed used, {len(unlisted)} unlisted, "
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
