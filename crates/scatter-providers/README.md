# scatter-providers

Triplespace's provider model: which sources' entity IDs can be written directly, under which
prefixes, with which grammars and concept IRIs. It is opinionated. It encodes the scheme
[Triplespace](https://triplespace.readthedocs.io/) uses, not a general standard.

A **provider** is a source whose entity IDs can be referred to directly: Wikidata, OpenAlex, another
Triplespace instance. Each has:

- a two-letter **code** that prefixes the IDs it minted, so Wikidata's `Q42` is written `WDQ42`;
- a **slug** used in graph names (`mirror/wikidata`);
- a **number** that partitions the revision-ID space (`revid = number << 40 | n`).

Each provider mints one or more **entity types**. A type has:

- a one-letter type code;
- an upstream prefix for rewriting (`Q42` ↔ `WDQ42`);
- an ID grammar;
- a concept-IRI template.

A **source** is a provider named by a slug instead of a code (`lb:Q7`).

The default registry is embedded from `registry/providers.toml`. An instance can start from it and
diverge, so nothing here is a constant: a `Registry` is a value the caller parses and passes
around.

If you want the ID forms of
[`scatter-wikibase-ids`](https://crates.io/crates/scatter-wikibase-ids) without this model,
implement its `ProviderModel` trait for your own model instead. Then build that crate, and
`scatter-wikibase-fusion`, without their default `providers` feature.

It does no I/O and builds for `wasm32-unknown-unknown`.

## Example

```rust
use scatter_providers::Registry;

let registry = Registry::default_registry();
let id = registry.parse_foreign_id("wdq42").unwrap();
assert_eq!(id.to_string(), "WDQ42");
assert_eq!(id.upstream_id(), "Q42");
assert_eq!(id.concept_iri().as_str(), "http://www.wikidata.org/entity/Q42");

// A bare Wikibase ID is local, not foreign.
assert!(registry.parse_foreign_id("Q5").is_err());
```

## Licence

GPL-3.0-or-later. See `LICENSE`. The registry file is CC0-1.0, like everything in Triplespace's
`docs/`.

The code comments cite Triplespace's design documents, for example `0015 §2` (an architecture
decision record). They are published at <https://triplespace.readthedocs.io/>.
