# Fuzz targets

The `cargo-fuzz` targets chapter 22 §4.9 lists (0033 §15), over libFuzzer. This directory
is its own Cargo workspace: it is not a crate of the product and not in the crate map
(`docs/registry/crates.toml`), so `cargo xtask deps`, the ordinary workspace build and
cargo-deny never see it. It reaches the crates under test by path.

| Target | Entry points | Asserts |
|---|---|---|
| `wikitext` | `scatter_wikitext::render` and `extract` (0008 §8; 0033 §9.1) | no panic; returns within the parser's five-second bound |
| `markdown` | `scatter_pages::markdown::render` with the `scatter-wikitext` sanitizer (0008 §5; 0033 §9.2) | no panic |
| `cbor_record` | `scatter_log::cbor::decode`, `decode_prefix`, `Header`, `Body` and `Record::decode` (0006 §2–3, §5) | no panic |

Nothing about the output is asserted here: the golden tests, the test vectors and the
property tests (`crates/*/tests/properties.rs`) do that.

## Running

Needs a nightly toolchain and cargo-fuzz:

```sh
rustup toolchain install nightly
cargo +nightly install cargo-fuzz
fuzz/run.sh wikitext 60        # seconds; seeds the corpus first
```

`run.sh` holds each target's `-max_len` and `-timeout`, and `seed.py` fills
`fuzz/corpus/<target>/` from the repository's fixtures: Librarybase's wikitext revisions
and the English Wikipedia talk pages of `docs/samples/`, the repository's own Markdown
plus `seeds/markdown/`, and the CBOR byte strings of the log test vectors. The corpus,
`artifacts/` and `target/` are gitignored.

A crash lands in `fuzz/artifacts/<target>/`; reproduce it with
`cargo +nightly fuzz run <target> fuzz/artifacts/<target>/<file>`.

`.github/workflows/fuzz.yml` runs every target nightly and on manual dispatch, never on a
pull request, and uploads the artifacts of a failed run.
