# Changelog

## scatter-wikitext-parse 0.1.0

The first release of the fork, from `parse-wiki-text-2` 0.2.0 (upstream
<https://github.com/soerenmeier/parse-wiki-text-2>, revision
`b13cfc0fddfd7432f196dbd16e2cdc29b7e37072`). The crate is renamed, and its library is
`scatter_wikitext_parse`. Each change to the code is marked in the source with a `Fork:`
comment. Upstream's tests in `tests/` still pass, except `issue_1`, which recorded the
exponential case fixed below as a timeout and now asserts that it finishes.

Added:

- **A step budget.** `Configuration::parse_with_budget` stops after a number of steps, a step
  being one turn of the main loop, with `ParseError::BudgetExhausted` and the output so far; it
  reads no clock. `Output::steps` reports the steps a parse took. `ParseError` and `Output` are
  `#[non_exhaustive]`.
- **More positions.** `Node::Link` has `trail_start`, where the link trail starts after `]]`.
  `Node::Tag` has `content_start` and `content_end`, the extent of the content between the start
  and end tags. A caller no longer searches the source for them.
- `HTML_ENTITIES` is public, so callers decode attribute values with the same table as the
  parser.

Fixed:

- **Nested unclosed openers took exponential time and memory** (upstream's issue 1, `src/state.rs`,
  `src/parse.rs`). Each opener found unclosed at the end of the text was rewound and everything
  after it parsed again, re-opening and re-rewinding every opener inside it: 24 `{{` asked for
  3 GiB. An opener found unclosed is now rewound at once when it is met again, which leaves the
  output exactly as before (checked against the old parser on 2.5 million random inputs) and
  takes at most quadratic steps. A warning repeated by those re-parses is now given once.
- **Text before a last line of spaces was repeated** (`src/line.rs`). `" a\n "` gave a
  preformatted block holding `a` and then `a` again after it.
- **Positions** checked by the new `tests/positions.rs`: a table cell that goes on past its first
  line had its start moved to the next line, past its content (`src/table.rs`); a preformatted block started at its first non-blank character, after the start
  of its own text, and one at the end of the text ended past it (`src/line.rs`); a comment never
  closed ended after the trailing whitespace of the line or list item that held it
  (`src/comment.rs`).
- **A parse stopped at its limit could panic** (`src/parse.rs`): the output so far was cut at the
  scan position, which may lie inside a character. It is cut at the boundary before it.

Also changed, as Triplespace's patches to the vendored copy were:

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
- Clippy's lints are allowed in `src/lib.rs` and `tests/*.rs`, keeping upstream's style, and
  `src/line.rs` is formatted by upstream's `rustfmt.toml`.
- The minimum Rust version is 1.95, Triplespace's.

## parse-wiki-text-2 0.2.0

By default execution time is now limited to 5 seconds.
