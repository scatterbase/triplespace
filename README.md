# Triplespace

Wikibase reimagined as an append-only log, in Rust. Bulk ingest is a first-class path, and
foreign entities (Wikidata's `WDQ42`, OpenAlex's `OAW123`, a domain name as `domain:example.org`)
sit alongside local ones without being reified.

The design lives in [`docs/decisions/`](docs/decisions/) as architecture decision records; the
names and codes the code embeds live in [`docs/registry/`](docs/registry/); the compatibility
contracts with Wikibase and MediaWiki are in [`docs/api/`](docs/api/). Start with
[0000](docs/decisions/0000-init.md), then [0005](docs/decisions/0005-crate-organization.md) for
the crate map and build order.

## Layout

| Path | Holds |
|---|---|
| `crates/` | One Cargo workspace. `scatter-*` crates are shared with Scatterbase and know nothing about Triplespace; `triplespace-*` crates are the product (0005 §1). |
| `xtask/` | Repository tasks: `cargo xtask deps` checks the workspace dependency graph against the table in 0005 §2; `cargo xtask wasm` builds the crates 0005 rule 7 requires to build for `wasm32-unknown-unknown`. |
| `docs/` | ADRs, registry, API contracts and test vectors. CC0-1.0 (`docs/LICENSE`), so other implementations can embed them. |
| `ui/` | The frontend (0034). Not yet started. |

## Building

Stable Rust as pinned in `rust-toolchain.toml`, edition 2024. A small instance needs Postgres and
nothing else (0033 §1); Valkey, OpenSearch and QLever are optional services a larger one adds.

```
cargo build --workspace
cargo nextest run --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
cargo xtask deps
```

## Licence

Triplespace is GPL-3.0-or-later (`LICENSE`). The shared `scatter-*` crates are also available
under a commercial licence from Scatter LLC. `docs/` is CC0-1.0. See 0005 §6 and 0033 §16.
