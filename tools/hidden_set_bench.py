#!/usr/bin/env python3
"""The hidden set's cost (09 §5.3; 0056 A19): request latency as confidential pages
accumulate, for a reader outside their group (a non-empty hidden set, recomputed every
request) and for a member (an empty one, remembered by `pages::NothingHidden` until the
ACL frontier moves).

    tools/hidden_set_bench.py --api http://127.0.0.1:18290 --owner-password-file owner.pw

`BENCH=1 BIN=target/release tools/acl_check.sh` sets the server up and runs this against it
(BENCH_ARGS passes options). The numbers go in docs/testing/performance.md §7.
"""

import argparse
import statistics
import subprocess
import sys
import time

import requests


class C:
    def __init__(self, api):
        self.api = api
        self.s = requests.Session()

    def get(self, **params):
        params.setdefault("format", "json")
        params.setdefault("formatversion", "2")
        return self.s.get(f"{self.api}/w/api.php", params=params).json()

    def post(self, **params):
        params.setdefault("format", "json")
        params.setdefault("formatversion", "2")
        return self.s.post(f"{self.api}/w/api.php", data=params).json()

    def token(self, kind="csrf"):
        return self.get(action="query", meta="tokens", type=kind)["query"]["tokens"][f"{kind}token"]

    def login(self, name, pw):
        t = self.token("login")
        r = self.post(action="clientlogin", username=name, password=pw, logintoken=t,
                      loginreturnurl=f"{self.api}/")
        assert r["clientlogin"]["status"] == "PASS", r
        return self

    def timed(self, n, **params):
        """Median and p90 of `n` requests, in milliseconds, after one warm-up."""
        self.get(**params)
        samples = []
        for _ in range(n):
            t0 = time.perf_counter()
            self.get(**params)
            samples.append((time.perf_counter() - t0) * 1000)
        samples.sort()
        return statistics.median(samples), samples[int(len(samples) * 0.9) - 1]


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--api", default="http://127.0.0.1:18290")
    p.add_argument("--owner-password-file", required=True)
    p.add_argument("--pages", type=int, default=200, help="confidential pages to accumulate")
    p.add_argument("--steps", default="0,50,200", help="page counts at which to measure")
    p.add_argument("--requests", type=int, default=40, help="timed requests per probe")
    p.add_argument("--db", help="the database URL, to ANALYZE before each measurement: the pages "
                   "are created moments before, and a wiki that grew slowly has current statistics")
    args = p.parse_args()
    with open(args.owner_password_file, encoding="utf-8") as f:
        pw = f.read().strip()
    owner = C(args.api).login("Alice", pw)
    anon = C(args.api)
    steps = sorted({int(s) for s in args.steps.split(",")} | {args.pages})
    t = owner.token()
    r = owner.s.request("PUT", f"{args.api}/w/rest.php/triplespace/v0/group/board",
                        json={"permissions": [], "token": t}).json()
    assert "error" not in r, r
    r = owner.post(action="userrights", user="Alice", add="board", token=owner.token("userrights"))
    assert "error" not in r, r
    owner.post(action="edit", title="Project:Open", text="Public page.", token=t)
    probes = [
        ("prop=info, one public page", {"action": "query", "prop": "info", "titles": "Project:Open"}),
        ("list=recentchanges, 50", {"action": "query", "list": "recentchanges", "rclimit": "50"}),
        ("list=allpages, 50", {"action": "query", "list": "allpages", "apnamespace": "4", "aplimit": "50"}),
    ]
    print(f"{'confidential pages':>20} {'probe':<30} {'outsider ms (p50/p90)':>24} {'member ms (p50/p90)':>22}")
    made = 0
    for step in steps:
        while made < step:
            made += 1
            title = f"Project:Secret {made}"
            owner.post(action="edit", title=title, text=f"Hush {made}.", token=t)
            r = owner.post(action="protect", title=title, protections="read=board", token=t)
            assert "error" not in r, r
        if args.db:
            subprocess.run(["psql", args.db, "-q", "-c", "ANALYZE"], check=True)
        for name, params in probes:
            o50, o90 = anon.timed(args.requests, **params)
            m50, m90 = owner.timed(args.requests, **params)
            print(f"{step:>20} {name:<30} {o50:>11.1f} / {o90:<10.1f} {m50:>9.1f} / {m90:<8.1f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
