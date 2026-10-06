#!/usr/bin/env python3
"""Cut the first N entities out of a MediaWiki XML dump, as a valid dump.

    dump_slice.py librarybase-20261005.xml.gz slice-10k.xml.gz 10000

Streams the input, so it never holds more than one <page> in memory. An entity is a
page whose <model> is wikibase-* (item, property, lexeme); other pages before the Nth
entity are kept too, so the slice is a prefix of the dump — the same pages
`triplespace adopt` would see first. Input and output may be .gz or plain .xml.
"""

import gzip
import re
import sys

MODEL = re.compile(r"<model>(wikibase-[a-z]+)</model>")


def open_in(path):
    return gzip.open(path, "rt", encoding="utf-8") if path.endswith(".gz") else open(path, encoding="utf-8")


def open_out(path):
    return gzip.open(path, "wt", encoding="utf-8", compresslevel=6) if path.endswith(".gz") else open(path, "w", encoding="utf-8")


def main(src, dst, limit):
    limit = int(limit)
    entities = pages = 0
    page = None  # lines of the page being read, or None between pages
    with open_in(src) as inp, open_out(dst) as out:
        for line in inp:
            s = line.strip()
            if page is None:
                if s == "<page>":
                    page = [line]
                elif s == "</mediawiki>":
                    break
                else:
                    out.write(line)  # the <siteinfo> header, whitespace
                continue
            page.append(line)
            if s == "</page>":
                text = "".join(page)
                page = None
                out.writelines(text)
                pages += 1
                if MODEL.search(text):
                    entities += 1
                    if entities >= limit:
                        break
        out.write("</mediawiki>\n")
    print(f"{pages} pages, {entities} entities -> {dst}", file=sys.stderr)


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(*sys.argv[1:])
