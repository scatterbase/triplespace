# scatter-wikitext-expand

MediaWiki's wikitext preprocessor and template expansion, in Rust, with no I/O.

This crate takes the text of a wiki page and expands its template calls in the way MediaWiki's
preprocessor does. It never reads pages or a database itself: whatever it needs to know about the
wiki (the text of a template, an interface message) comes through the `ExpandHost` trait, which you
implement. That keeps it pure. It builds for `wasm32-unknown-unknown`, so a browser preview can
run the same code as a server.

It is written from MediaWiki's documented behaviour, not translated from its PHP. It is part of
[Triplespace](https://triplespace.readthedocs.io/), which uses it to render pages and to check
imported wikis, and it knows nothing about Triplespace.

## What it does today

Version 0.1 covers the first slice of the preprocessor:

- brace matching as MediaWiki does it, keeping track of `[[…]]` so that a `|` inside a link does
  not split a call;
- `<noinclude>`, `<includeonly>` and `<onlyinclude>`, resolved for viewing a page or for
  transcluding it;
- extension tags and comments passed through as written, their contents never expanded;
- `{{Name}}` and `{{:Name}}` transcluded through the host, recursively, with loop detection and
  MediaWiki's depth limit (`$wgMaxTemplateDepth`, 100); a missing page becomes a red link;
- `{{{name|default}}}` in a frame with no arguments;
- the newline MediaWiki inserts before an expansion that starts a list, a table or a definition;
- `{{int:…}}` through the host;
- the **pre-save transform**: `~~~`, `~~~~` and `~~~~~` signatures, line ends and trailing
  whitespace;
- a **census** of what a text calls (templates, modules, variables, parser functions, extension
  tags and behaviour switches), each with its status in the registry.

Not yet: templates called with arguments, the core variables and parser functions,
ParserFunctions (`#if`, `#expr`, `#time` and the rest), strip markers and the limit report. These
calls are left in the output exactly as written, for a renderer to show as they are. They are
coming in later releases. Under Cargo's rules for 0.x versions, each of those releases may change
the API and will raise the minor version.

## Example

```rust
use scatter_wikitext_expand::{expand_for_view, ExpandHost, Transcluded};

struct Wiki;

impl ExpandHost for Wiki {
    fn transclude(&self, name: &str) -> Option<Transcluded> {
        Some(match name.trim() {
            "Greeting" => Transcluded::Found(
                "Hello<noinclude>[[Category:Templates]]</noinclude>".into(),
            ),
            other => Transcluded::Missing(format!("Template:{other}")),
        })
    }
}

assert_eq!(expand_for_view("{{Greeting}}, world", &Wiki), "Hello, world");
```

`expand_for_view_mapped` returns the same text, plus a list showing which part of the output each
of the page's own calls produced. `call_spans` gives where each of a text's outermost calls starts
and ends, as the preprocessor matches its braces, for a renderer or a parser to check its own
reading against.

## The registry

The crate needs to know which names are variables, parser functions and extension tags, so that
it leaves them alone rather than taking them for templates. It reads them from
`registry/wikitext-functions.toml`, embedded at build time (`Registry::embedded`). The file lists
MediaWiki's core words, those of common extensions (ParserFunctions, Scribunto, Cite,
WikibaseClient…) and Triplespace's own, each with a status: `implemented`, `chip` (recognised, but
not expanded) or `ignored`. To read a different list, use `Registry::parse` and the `census::scan_with`
function.

The file is CC0-1.0, like everything in Triplespace's `docs/`.

## Changes in 0.1.1

- **0.1.0 could loop for ever** on a lone `}` inside an open call, as in `{{}`. Upgrade.
- An extension tag with no end tag is text, and expansion goes on after it, as MediaWiki's
  preprocessor reads it; 0.1.0 treated the rest of the text as the tag's content. Only the
  inclusion tags run to the end.
- An end tag is `</name>` with nothing but whitespace before the `>`: `</ref name=x>` is not one.
- `call_spans`, above.

## Licence

GPL-3.0-or-later. See `LICENSE`.

The code comments cite Triplespace's design documents, for example `0042 §4` (an architecture
decision record) or `11 §1.4` (a chapter). They are published at
<https://triplespace.readthedocs.io/>.
