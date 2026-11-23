# scatter-wikibase-fusion

How [Triplespace](https://triplespace.readthedocs.io/) combines and indexes Wikibase statements. This
is an extension of [`scatter-wikibase-model`](https://crates.io/crates/scatter-wikibase-model).

- **Statement keys.** A statement's key is a hash of its main snak and its qualifiers, in any
  order. Each is in canonical form:
  - entity IDs are rewritten to their cluster's canonical ID;
  - literal values are reduced to their normalized key (from
    [`scatter-normalize`](https://crates.io/crates/scatter-normalize)).

  Statements with the same key are one statement seen from several sources. What "canonical"
  means is passed in through the `Canon` trait; `Identity` is the trivial one.
- **The search document.** `entity_document` gives the one document a search index holds for an
  entity: its terms, its statements' values and who may read it.
- **Badges.** These are marks drawn beside an entity, a sitelink or a statement. The registry is
  embedded from `registry/badges.toml`.

`Bound::for_model` binds resolvers to Wikidata's properties under the code a provider model
(`scatter-wikibase-ids`' `ProviderModel`) gives Wikidata. The default `providers` feature adds
`Bound::embedded`, the embedded resolvers bound under
[`scatter-providers`](https://crates.io/crates/scatter-providers)' code.

It does no I/O and builds for `wasm32-unknown-unknown`.

## Example

```rust
use scatter_wikibase_fusion::{Identity, statement_key};
use scatter_wikibase_model::Entity;

let item = |value: &str| {
    let json = format!(r#"{{"type": "item", "id": "Q1", "claims": {{"P1": [{{
        "type": "statement", "rank": "normal",
        "mainsnak": {{"snaktype": "value", "property": "P1", "datatype": "string",
            "datavalue": {{"type": "string", "value": "{value}"}}}}
    }}]}}}}"#);
    Entity::from_json(&json).unwrap().entity
};
let key = |e: &Entity| statement_key(&e.statements.values().next().unwrap()[0], &Identity);

// The same text, composed (`\u{e9}`) or decomposed (`e\u{301}`), is the same statement.
assert_eq!(key(&item("Andr\u{e9}")), key(&item("Andre\u{301}")));
assert_ne!(key(&item("Andr\u{e9}")), key(&item("Andre")));
```

## Licence

GPL-3.0-or-later. See `LICENSE`. The registry file is CC0-1.0, like everything in Triplespace's
`docs/`.

The code comments cite Triplespace's design documents, for example `0004 §8` (an architecture
decision record). They are published at <https://triplespace.readthedocs.io/>.
