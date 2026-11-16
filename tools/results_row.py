#!/usr/bin/env python3
"""Runs a real-dump command and writes its row into the results log of
docs/testing/real-dumps.md, so that no run goes unrecorded and no row is typed by hand.

  python3 tools/results_row.py --run "3.2 WD 1M slice" --input wd-1m.json.gz \\
      [--note "…"] [--rebuild yes|no|"…"] [--log FILE] [--dry] -- triplespace sync …

What it measures, for the columns of the log:

  Input              the --input file's name, the first 8 hex digits of its sha256, and the
                     entity count the command reports (`of N entities`), when it reports one
  Wall               the command's wall time
  Peak RSS           the peak resident set of the command and its children (getrusage)
  DB size after      pg_database_size of TRIPLESPACE_DATABASE_URL (or --database), via psql
  Created / adopted  `created N` (sync) or `adopted N revisions of M entities` (adopt)
  Rejected           `rejected N`
  Unreadable         the sum of the `unreadable N` lines
  Rebuild identical? --rebuild, which only a rebuild and a diff can tell
  Notes              the date (UTC), the commit (with `+` when the tree has changes), the
                     command's exit status when it is not 0, and --note

The command's output passes through unchanged. A failed run is recorded too, with its exit
status in the notes; the script then exits with the command's status. --dry prints the row
instead of writing it. The row goes at the end of the log's table.
"""
import argparse
import datetime
import hashlib
import os
import re
import resource
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOG = os.path.join(ROOT, "docs", "testing", "real-dumps.md")


def sha256_prefix(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for block in iter(lambda: fh.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()[:8]


def wall(seconds):
    s = int(round(seconds))
    h, rest = divmod(s, 3600)
    m, s = divmod(rest, 60)
    return f"{h}h{m:02d}m{s:02d}s" if h else f"{m}m{s:02d}s" if m else f"{s}s"


def size(n_bytes):
    if n_bytes is None:
        return ""
    for unit, scale in (("GB", 1 << 30), ("MB", 1 << 20), ("KB", 1 << 10)):
        if n_bytes >= scale:
            v = n_bytes / scale
            return f"{v:.2f} {unit}" if v < 10 else f"{v:.0f} {unit}"
    return f"{n_bytes} B"


def counts(output):
    """What the command's summary reports: created/adopted, entities, rejected, unreadable."""
    out = {}
    if (m := re.search(r"^\s*adopted\s+(\d+) revisions of (\d+) entities", output, re.M)):
        out["created"] = f"{int(m.group(1)):,} revisions"
        out["entities"] = int(m.group(2))
    elif (m := re.search(r"^\s*created\s+(\d+)", output, re.M)):
        out["created"] = f"{int(m.group(1)):,}"
    if (m := re.search(r"^\s*rejected\s+(\d+)", output, re.M)):
        out["rejected"] = f"{int(m.group(1)):,}"
    unreadable = [int(n) for n in re.findall(r"^\s*unreadable\s+(\d+)", output, re.M)]
    out["unreadable"] = f"{sum(unreadable):,}" if unreadable else ("0" if out else "")
    return out


def db_size(url):
    if not url:
        return None
    try:
        r = subprocess.run(["psql", url, "-Atc", "SELECT pg_database_size(current_database())"],
                           capture_output=True, text=True, timeout=60)
        return int(r.stdout.strip()) if r.returncode == 0 and r.stdout.strip() else None
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return None


def commit():
    try:
        rev = subprocess.run(["git", "-C", ROOT, "rev-parse", "--short", "HEAD"],
                             capture_output=True, text=True).stdout.strip()
        dirty = subprocess.run(["git", "-C", ROOT, "status", "--porcelain", "--untracked-files=no"],
                               capture_output=True, text=True).stdout.strip()
        return f"`{rev}{'+' if dirty else ''}`" if rev else ""
    except OSError:
        return ""


def cell(s):
    return str(s).replace("|", "\\|").replace("\n", " ")


def row(args, seconds, rss, db, out, status):
    c = counts(out)
    inp = ""
    if args.input:
        inp = f"`{os.path.basename(args.input)}` (sha256 `{sha256_prefix(args.input)}…`)"
        if "entities" in c:
            inp += f", {c['entities']:,} entities"
    notes = [datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d"), commit()]
    if status:
        notes.append(f"**exit {status}**")
    if args.note:
        notes.append(args.note)
    cells = [args.run, inp, wall(seconds), size(rss), size(db), c.get("created", ""),
             c.get("rejected", ""), c.get("unreadable", ""), args.rebuild or "",
             ", ".join(n for n in notes if n)]
    return "| " + " | ".join(cell(x) for x in cells) + " |"


def append(log, line):
    text = open(log, encoding="utf-8").read()
    head = text.index("## Results log")
    lines = text[head:].split("\n")
    # The table: the first `| Run |` header, its rule, then rows until the first non-row.
    start = next(i for i, l in enumerate(lines) if l.startswith("| Run |"))
    end = start + 2
    while end < len(lines) and lines[end].startswith("|"):
        end += 1
    lines.insert(end, line)
    with open(log, "w", encoding="utf-8") as fh:
        fh.write(text[:head] + "\n".join(lines))


def main(argv):
    if "--" not in argv:
        sys.exit("results_row.py: give the command after `--`")
    i = argv.index("--")
    p = argparse.ArgumentParser(prog="results_row.py")
    p.add_argument("--run", required=True, help="the run's name, as the stages number it")
    p.add_argument("--input", help="the dump the run reads")
    p.add_argument("--note", default="")
    p.add_argument("--rebuild", default="", help="the Rebuild identical? cell")
    p.add_argument("--database", default=os.environ.get("TRIPLESPACE_DATABASE_URL", ""))
    p.add_argument("--log", default=LOG)
    p.add_argument("--dry", action="store_true")
    args = p.parse_args(argv[:i])
    command = argv[i + 1:]
    if not command:
        sys.exit("results_row.py: no command after `--`")
    began = time.monotonic()
    proc = subprocess.Popen(command, stdout=subprocess.PIPE, text=True, bufsize=1)
    captured = []
    for line in proc.stdout:
        sys.stdout.write(line)
        captured.append(line)
    status = proc.wait()
    seconds = time.monotonic() - began
    peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    rss = peak if sys.platform == "darwin" else peak * 1024   # bytes on macOS, KB on Linux
    line = row(args, seconds, rss, db_size(args.database), "".join(captured), status)
    if args.dry:
        print(line)
    else:
        append(args.log, line)
        print(f"results_row.py: recorded in {os.path.relpath(args.log, ROOT)}", file=sys.stderr)
    sys.exit(status)


if __name__ == "__main__":
    main(sys.argv[1:])
