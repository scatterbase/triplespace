# scatter-wikitext

Render MediaWiki wikitext to sanitized HTML in pure Rust, and read out what a page links to, with
the byte position of everything in the source.

The crate parses text with [`scatter-wikitext-parse`](https://crates.io/crates/scatter-wikitext-parse),
lowers its nodes to a render tree that keeps source spans, and writes the tree as HTML inside
`<div class="mw-parser-output">`, the way MediaWiki's own output is wrapped, sanitized with
[`ammonia`](https://crates.io/crates/ammonia). The links, categories, external links, template
calls, behaviour switches and sections come back from the same walk over the tree, so what is
rendered and what is extracted always agree. It does no I/O: whatever it needs to know about the
wiki (whether a page exists, its URL, its label) comes through the `Resolver` trait, which you
implement. It builds for `wasm32-unknown-unknown`, so a browser preview can run the same code as a
server.

It is part of [Triplespace](https://triplespace.readthedocs.io/), which renders its wiki pages with
it, and it knows nothing about Triplespace.

## What it renders

Version 0.1 renders a subset of wikitext, following MediaWiki's documented behaviour:

- headings, paragraphs, `----`, lists (`*`, `#`, `;`, `:`), tables and preformatted lines;
- bold and italic by MediaWiki's apostrophe rules;
- internal links, resolved through your `Resolver` (red links for missing pages, `mw-redirect`
  for redirects, a label in place of the title where the resolver gives one), interwiki links,
  external links and bare URLs;
- HTML tags and attributes on the allow-list MediaWiki's sanitizer uses, with inline CSS checked;
- `<nowiki>`, `<pre>`, `<syntaxhighlight>` (as plain `<pre>`), comments (dropped);
- the table of contents, with `__TOC__`, `__NOTOC__` and `__FORCETOC__`;
- redirects, and category links, which are extracted rather than shown.

It does not expand templates. Run a preprocessor first, such as
[`scatter-wikitext-expand`](https://crates.io/crates/scatter-wikitext-expand), and render its
output. Anything the subset does not cover renders as a visible *chip* showing its source: a
template call, a parser function, a file or image link, an extension tag such as `<ref>` or
`<gallery>`. A chip is a `<span>` with the classes `wt-chip`, `wt-chip--wikitext` and
`wt-chip--template` or `wt-chip--unsupported`; `ConfigSource::class_prefix` changes the `wt-`.

The parser's work is bounded by a step budget proportional to the text, so a pathological page
renders as its source rather than taking unbounded time.

## Example

```rust
use scatter_wikitext::{render, Config, ConfigSource, PageInfo, Resolver, Title};

struct Wiki;

impl Resolver for Wiki {
    fn info(&self, title: &Title) -> PageInfo {
        PageInfo {
            exists: title.text == "Douglas Adams",
            ..PageInfo::default()
        }
    }
    fn url(&self, title: &Title) -> String {
        format!("/wiki/{}", title.text.replace(' ', "_"))
    }
    fn missing_url(&self, title: &Title) -> String {
        format!("/w/index.php?title={}&action=edit", title.text.replace(' ', "_"))
    }
}

let config = Config::new(ConfigSource::default());
let page = Title::new(0, "Example");
let out = render(
    "'''Hello''' to [[Douglas Adams]] and [[Nowhere]]. {{Infobox}}",
    &config,
    &Wiki,
    &page,
);

assert!(out.html.contains("<b>Hello</b>"));
assert!(out.html.contains(r#"<a href="/wiki/Douglas_Adams" title="Douglas Adams">Douglas Adams</a>"#));
assert!(out.html.contains(r#"class="new""#)); // the red link
assert_eq!(out.extracted.links.len(), 2);
assert_eq!(out.extracted.templates[0].name, "Infobox"); // left as a chip
```

`ConfigSource` holds what parsing depends on: the wiki's namespaces and their aliases, interwiki
prefixes, URL protocols, extension tags, behaviour switches and interface messages.
`ConfigSource::default()` is MediaWiki's for an English wiki; build one from a wiki's siteinfo for
anything else. `extract` reads out the links and the rest without rendering, and `render_marked`
marks the blocks a template drew, from the call map `scatter-wikitext-expand` gives.

## Licence

GPL-3.0-or-later. See `LICENSE`.

The code comments cite Triplespace's design documents, for example `0008 §8` (an architecture
decision record) or `11 §1.2` (a chapter). They are published at
<https://triplespace.readthedocs.io/>.
