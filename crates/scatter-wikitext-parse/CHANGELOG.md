# Changelog

## scatter-wikitext-parse 0.1.0

The first release of the fork, from `parse-wiki-text-2` 0.2.0 (upstream
<https://github.com/soerenmeier/parse-wiki-text-2>, revision
`b13cfc0fddfd7432f196dbd16e2cdc29b7e37072`). The crate is renamed, and its library is
`scatter_wikitext_parse`; otherwise its API is upstream's, with one addition. Each change to
the code is marked in the source with a `Fork:` comment. Upstream's tests in `tests/` still pass.

- **The clock is read only when a timeout is asked for** (`src/parse.rs`). `Instant::now()`
  panics on `wasm32-unknown-unknown`, so `parse_without_timeout` could not run in a browser.
- **Extension tags are entered after the HTML tags** (`src/configuration.rs`). A name in both
  lists, such as `pre`, was an HTML tag whatever the configuration said, so `<pre>` content was
  parsed as wikitext.
- **A piped link's trail is consumed** (`src/link.rs`, `tests/cases/link.test`). `[[a|b]]cd`
  put `cd` both in the link and in the text after it; upstream's snapshot had recorded the
  duplicate.
- **Text before `<nowiki>` and `<math>` is kept** (`src/tag.rs`). It was dropped:
  `x <nowiki>y</nowiki>` lost `x `.
- **A parameter's end is after its closing `}}}`** (`src/template.rs`). Its span ended three
  bytes early.
- **`&Agrave;` is spelled correctly** (`src/html_entities.rs`). It was entered as `Agrav`.
- **`HTML_ENTITIES` is public** (`src/lib.rs`), so callers decode attribute values with the same
  table as the parser.
- Clippy's lints are allowed in `src/lib.rs` and `tests/*.rs`, keeping upstream's style, and
  `src/line.rs` is formatted by upstream's `rustfmt.toml`.
- The minimum Rust version is 1.95, Triplespace's.

Known and not changed: a table cell after the first on a line has a start position past its
content.

## parse-wiki-text-2 0.2.0

By default execution time is now limited to 5 seconds.
