# Changes to parse-wiki-text-2

Vendored from crates.io `parse-wiki-text-2` 0.2.0 (upstream
<https://github.com/soerenmeier/parse-wiki-text-2>, revision in `vendor.toml`), with the changes
below. Each is marked in the source with a `Triplespace:` comment, and `triplespace.patch` is
the whole diff against the release, for offering upstream (chapter 22 §4.6: changes are
offered upstream where they are general). The upstream tests in `tests/` still pass.

| File | Change | Why |
|---|---|---|
| `src/parse.rs` | The clock is read only when a timeout is asked for | `Instant::now()` panics on `wasm32-unknown-unknown`, so `parse_without_timeout` could not run in the browser preview |
| `src/configuration.rs` | Extension tags are entered after the HTML tags | A name in both lists (`pre`) was an HTML tag whatever the configuration said, so `<pre>` content was parsed as wiki text |
| `src/link.rs`, `tests/cases/link.test` | A piped link's trail is consumed | `[[a\|b]]cd` put `cd` both in the link and in the text after it; upstream's snapshot had recorded the duplicate |
| `src/tag.rs` | Text before `<nowiki>` and `<math>` is flushed | It was dropped: `x <nowiki>y</nowiki>` lost `x ` |
| `src/template.rs` | A parameter's end is after its closing `}}}` | Its span ended three bytes early |
| `src/html_entities.rs` | `&Agrave;` is spelled correctly | It was entered as `Agrav` |
| `src/lib.rs`, `tests/*.rs` | Clippy's lints are allowed | Upstream's style is kept; CI's `cargo clippy --workspace -- -D warnings` reaches the crate as a path dependency |
| `src/lib.rs` | `HTML_ENTITIES` is public | So callers decode attribute values with the same table |
| `Cargo.toml` | `publish = false`, `rust-version` 1.95, and a comment | It is built from the tree, not crates.io; as a workspace member its `rust-version` of 1.65 made Cargo resolve the whole workspace for 1.65, which held `ammonia` at a release with advisories |
| `rustfmt.toml`, `src/line.rs` | Upstream's `rustfmt.toml` is kept, and `src/line.rs` is formatted by it | `cargo fmt --all --check` reaches the crate as a path dependency; `line.rs` had drifted from current rustfmt |

Known and not changed, since rendering does not depend on them: a table cell after the first
on a line has a start position past its content.
