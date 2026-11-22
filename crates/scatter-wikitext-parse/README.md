# scatter-wikitext-parse

Parse MediaWiki wikitext into a tree of elements, in pure Rust, with the byte position of every
element in the source.

This is a fork of [`parse-wiki-text-2`](https://github.com/soerenmeier/parse-wiki-text-2) 0.2.0,
itself a maintained fork of Fredrik Portström's
[`parse_wiki_text`](https://github.com/portstrom/parse_wiki_text). It is published under its own
name because it carries fixes upstream has not released; `CHANGELOG.md` lists each one. It is part
of [Triplespace](https://triplespace.readthedocs.io/), which uses it to render wiki pages, and it
knows nothing about Triplespace.

## What it does

The parser takes the text of a page and returns its nodes: headings, paragraphs, lists, tables,
links, external links, images and categories, templates and their parameters, HTML and extension
tags, comments, magic words, redirects, bold and italic. Every node records where it starts and
ends, so a caller can map anything it finds back to the source; a link also says where its trail
starts, and an extension tag where its content starts and ends. Malformed markup never fails a
parse; it comes back as warnings beside the nodes.

Parsing depends on the wiki's configuration: its namespaces and their aliases, its extension tags,
its URL protocols and its magic words. `Configuration::new` takes these from a
`ConfigurationSource`; `Configuration::default()` holds a configuration for testing.

It does not expand templates: a template call is a node, as written. Expand first, with a
preprocessor such as [`scatter-wikitext-expand`](https://crates.io/crates/scatter-wikitext-expand),
then parse the result. Braces are matched as MediaWiki's preprocessor matches them, so a call the
preprocessor left as written is the template node here, and unmatched braces are text.

Bold and italic come back as toggles, where the parser meets each run of apostrophes; the
`quotes` module turns a line's runs into the elements they open and close, by MediaWiki's rules.
HTML tags come back as separate start and end nodes; balancing them into a tree, and making
paragraphs, is HTML's business and the caller's.

**Bounding the work.** `parse_with_budget` stops after a given number of steps, one step being a
turn of the parser's main loop, and returns the output so far. Ordinary pages take one or two
steps a byte (`Output::steps` says how many), so a budget proportional to the text's length stops a
pathological input in the same place on every machine. `parse` and `parse_with_timeout` bound it
by wall-clock time instead, as upstream does.

It builds for `wasm32-unknown-unknown`. There, use `parse_with_budget` or
`parse_without_timeout`, since `parse` and `parse_with_timeout` read the clock.

## Example

```rust
use scatter_wikitext_parse::{Configuration, Node};

let text = "See [[Douglas Adams|the author]] and {{Cite book}}.";
let output = Configuration::default().parse(text).expect("parsing timed out");
assert!(output.warnings.is_empty());

let mut found = Vec::new();
for node in &output.nodes {
    match node {
        Node::Link { target, start, end, .. } => found.push(format!("link to {target} at {start}..{end}")),
        Node::Template { start, end, .. } => found.push(format!("template at {start}..{end}")),
        _ => {}
    }
}
assert_eq!(found, ["link to Douglas Adams at 4..32", "template at 37..50"]);
```

## Licence

MIT-0, as upstream: see `LICENSE`. It covers Scatter's changes too.

Code comments may cite Triplespace's design documents, for example `0115 §5` (an architecture
decision record) or `22 §4.6` (a chapter). They are published at
<https://triplespace.readthedocs.io/>.
