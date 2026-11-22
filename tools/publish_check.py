#!/usr/bin/env python3
"""Checks every crate the workspace would publish, as CI's *Publishable crates* job (0115 §7).

A publishable crate is a workspace crate whose manifest does not say `publish = false`
(22 §3.6). For all of them together it runs `cargo publish --workspace --dry-run`, which packages
each one, in dependency order, and builds it from its package, so a path-only dependency, a file
outside the crate's directory (a registry file that is not a symlink, 0115 §2) or a package that
does not build fails here rather than at James's publication. Then, for each crate crates.io
already has, it runs `cargo semver-checks` against the latest published version, so a breaking
change needs the version raised as 22 §3.5 says; a crate with no published version is listed and
skipped, since there is nothing to break yet.

Needs `cargo` and, once any crate is published, `cargo-semver-checks`. Reads crates.io's sparse
index (https://index.crates.io/) to learn what is published. Standard library only.

    tools/publish_check.py                 # both checks
    tools/publish_check.py --list          # print the publishable crates and what crates.io has
    tools/publish_check.py --no-semver     # the dry run only
"""

import argparse
import json
import subprocess
import sys
import urllib.error
import urllib.request

INDEX = "https://index.crates.io"


def publishable():
    """The workspace crates whose manifests allow publishing, by name."""
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    meta = json.loads(out)
    members = set(meta["workspace_members"])
    # `publish = false` is `[]` in cargo's metadata; no `publish` key is `null`.
    return sorted(
        p["name"]
        for p in meta["packages"]
        if p["id"] in members and p.get("publish") != []
    )


def index_path(name):
    """The crate's file in the sparse index (the Cargo book's *Index format*)."""
    n = name.lower()
    if len(n) <= 2:
        return f"{len(n)}/{n}"
    if len(n) == 3:
        return f"3/{n[0]}/{n}"
    return f"{n[:2]}/{n[2:4]}/{n}"


def published_versions(name):
    """The versions crates.io has of a crate, not yanked; empty when it has none."""
    try:
        with urllib.request.urlopen(f"{INDEX}/{index_path(name)}", timeout=30) as r:
            lines = r.read().decode().splitlines()
    except urllib.error.HTTPError as e:
        if e.code in (403, 404):
            return []
        raise
    return [
        v["vers"]
        for v in (json.loads(line) for line in lines if line.strip())
        if not v.get("yanked")
    ]


def run(cmd):
    print("$", " ".join(cmd), flush=True)
    return subprocess.run(cmd).returncode


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--list", action="store_true", help="list the crates and stop")
    ap.add_argument("--no-semver", action="store_true", help="skip cargo semver-checks")
    args = ap.parse_args()

    crates = publishable()
    if not crates:
        print("no publishable crates")
        return 0
    versions = {c: published_versions(c) for c in crates}
    for c in crates:
        have = ", ".join(versions[c]) if versions[c] else "not published"
        print(f"{c}: {have}")
    if args.list:
        return 0

    failed = []
    if run(["cargo", "publish", "--workspace", "--dry-run", "--locked"]) != 0:
        failed.append("cargo publish --dry-run")

    if not args.no_semver:
        for c in crates:
            if not versions[c]:
                print(f"{c}: no published version, semver-checks skipped")
                continue
            if run(["cargo", "semver-checks", "--package", c]) != 0:
                failed.append(f"semver-checks {c}")

    if failed:
        print("FAILED: " + "; ".join(failed), file=sys.stderr)
        return 1
    print("ok: " + ", ".join(crates))
    return 0


if __name__ == "__main__":
    sys.exit(main())
