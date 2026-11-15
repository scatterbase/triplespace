#!/usr/bin/env python3
"""Moves a skin's own messages into the skin's bundle (0097 §13; 19 §5.6).

A skin's messages are the ones its own templates name with `msg("…")` and nothing else
reads: not the site's Rust, which builds the view models every skin draws, not a script,
not the API. Each moves from the binary's bundle, `i18n/`, to `skins/{skin}/i18n/`, its
key renamed from `ts-…` to `{skin}-…`, and the templates are rewritten to name it so.

    tools/skin_messages.py            move what qualifies for Plain, and say what moved
    tools/skin_messages.py --check    change nothing; exit 1 if something would move

It is safe to run again: a key already moved is left alone. Run it after a change that
adds a `msg("ts-…")` to a template, so that the new message joins the skin's bundle.

A key stays in the binary's bundle when any of these holds:
- it appears, whole, anywhere outside the skin's templates: in a crate, in `ui/`, in
  `docs/registry/`, in `tools/`;
- its prefix is one the site hands to a script whole (`with_prefix("ts-bell")`);
- its prefix is one the site builds keys from (`format!("ts-diff-{…}")`).
"""
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SKIN = "plain"
BINARY = ROOT / "i18n"
BUNDLE = ROOT / "skins" / SKIN / "i18n"
TEMPLATES = ROOT / "skins" / SKIN / "templates"

MSG = re.compile(r'msg\("(ts-[a-z0-9-]+)"')
TOKEN = re.compile(r"ts-[a-z0-9]+(?:-[a-z0-9]+)*")
PREFIX = re.compile(r'with_prefix\("(ts-[a-z0-9-]*)"\)')
BUILT = re.compile(r'format!\("(ts-[a-z0-9-]*)\{')

SKIPPED_DIRS = {"node_modules", "target", "dist", "corpus", "snapshots", ".git", ".venv"}
SOURCES = [
    ("crates", (".rs",)),
    ("xtask", (".rs",)),
    ("ui/src", (".js", ".ts", ".vue")),
    ("ui/e2e", (".js", ".mjs")),
    ("docs/registry", (".toml",)),
    ("tools", (".py", ".sh")),
]


def sources():
    for base, suffixes in SOURCES:
        for path in sorted((ROOT / base).rglob("*")):
            if path.suffix in suffixes and not SKIPPED_DIRS & set(path.parts):
                if path.resolve() != pathlib.Path(__file__).resolve():
                    yield path


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def entry_line(key):
    """The pattern of a message's line in a banana file: one message a line."""
    return re.compile(r'^\t"%s": (.*?),?$' % re.escape(key))


def take(path, keys):
    """Removes the lines of `keys` from a banana file, as text, so that the rest of the
    file keeps its spelling; returns each key's value as the file wrote it."""
    lines = path.read_text(encoding="utf-8").split("\n")
    taken, kept = {}, []
    for line in lines:
        for key in keys:
            m = entry_line(key).match(line)
            if m:
                taken[key] = m.group(1)
                break
        else:
            kept.append(line)
    if taken:
        # The last message of the object carries no comma.
        close = max(i for i, line in enumerate(kept) if line == "}")
        last = close - 1
        if kept[last].endswith(","):
            kept[last] = kept[last][:-1]
        path.write_text("\n".join(kept), encoding="utf-8")
    return taken


def put(path, values, metadata):
    """Adds messages to a skin's banana file, creating it with the binary's metadata."""
    if path.exists():
        lines = path.read_text(encoding="utf-8").split("\n")
        close = max(i for i, line in enumerate(lines) if line == "}")
        head, tail = lines[:close], lines[close:]
        if head and not head[-1].endswith((",", "{")):
            head[-1] += ","
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        meta = json.dumps(metadata, ensure_ascii=False, indent="\t").replace("\n", "\n\t")
        head, tail = ["{", f'\t"@metadata": {meta},'], ["}", ""]
    entries = [f'\t"{key}": {value},' for key, value in values.items()]
    entries[-1] = entries[-1][:-1]
    path.write_text("\n".join(head + entries + tail), encoding="utf-8")


def qualifying():
    named = set()
    for template in sorted(TEMPLATES.rglob("*.html")):
        named.update(MSG.findall(template.read_text(encoding="utf-8")))
    used, prefixes = set(), set()
    for path in sources():
        text = path.read_text(encoding="utf-8", errors="replace")
        used.update(TOKEN.findall(text))
        prefixes.update(PREFIX.findall(text))
        prefixes.update(BUILT.findall(text))
    # A key built from nothing but `ts-` and a variable names no family.
    prefixes.discard("ts-")
    english = load(BINARY / "en.json")
    return sorted(
        key
        for key in named
        if key in english
        and key not in used
        and not any(key.startswith(p) for p in prefixes)
    )


def main():
    check = "--check" in sys.argv[1:]
    keys = qualifying()
    if not keys:
        print(f"skin_messages: nothing to move; {SKIN}'s bundle holds its templates' own messages")
        return 0
    renamed = {key: f"{SKIN}-{key[len('ts-'):]}" for key in keys}
    if check:
        print(f"skin_messages: {len(keys)} messages only {SKIN}'s templates name are still in i18n/:")
        for key in keys:
            print(f"  {key}")
        print("run tools/skin_messages.py to move them")
        return 1
    for source in sorted(BINARY.glob("*.json")):
        taken = take(source, keys)
        if taken:
            values = {renamed[key]: taken[key] for key in keys if key in taken}
            put(BUNDLE / source.name, values, load(source).get("@metadata", {}))
    for template in sorted(TEMPLATES.rglob("*.html")):
        text = template.read_text(encoding="utf-8")
        new = MSG.sub(
            lambda m: f'msg("{renamed.get(m.group(1), m.group(1))}"', text
        )
        if new != text:
            template.write_text(new, encoding="utf-8")
    print(f"skin_messages: moved {len(keys)} messages to skins/{SKIN}/i18n/:")
    for key in keys:
        print(f"  {key} -> {renamed[key]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
