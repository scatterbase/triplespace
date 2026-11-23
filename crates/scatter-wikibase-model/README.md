# scatter-wikibase-model

The Wikibase data model in pure Rust: entities, terms, sitelinks, statements, snaks, data values and
data types, read from and written to Wikibase's canonical JSON, with Wikibase's own hashes
recomputed.

The model mirrors Wikibase's JSON faithfully. It holds what a source asserted, in the shape the source
asserted it: sitelinks keyed by site, values with the strings Wikibase gives them, and property order
inside qualifiers and references kept as data. Normalizing values is a function over the model
(in [`scatter-normalize`](https://crates.io/crates/scatter-normalize)), never a change made while
parsing.

- **JSON in two forms.** `Form::Storage` leaves out the fields Wikibase derives (snak and reference
  `hash`, `numeric-id`); `Form::Wikibase` puts them back, as the Wikibase API serves them.
  `json::canonical` writes compact JSON with keys in a deterministic order, so the same entity
  always produces the same bytes.
- **Wikibase's own hashes.** `Hasher` reproduces how Wikibase hashes snaks, references and value
  nodes, PHP `serialize()` emulation and all, so a recomputed hash equals the one Wikidata serves.
- **Entity types**: items, properties, lexemes with forms and senses, MediaInfo and EntitySchema.
- **Sitelinks** to URLs and back through a site-alias registry embedded from `registry/sites.toml`.

It does no I/O and builds for `wasm32-unknown-unknown`. It is part of
[Triplespace](https://triplespace.readthedocs.io/), which stores and serves Wikibase entities with it.

## What it accepts beyond plain Wikibase

The model is the base of a set: Triplespace's extensions are in `scatter-wikibase-ids` (how it names
entities) and `scatter-wikibase-fusion` (how it combines and indexes them). A few shape rules stay
in this crate because they are harmless to anyone else, and they never trigger on plain Wikibase
data:

- IDs are read case-insensitively and written canonically: `q42` is `Q42`.
- Besides Wikibase's own `Q42`, `P31`, `L3-F1` and `M123`, an ID may be a three-letter prefixed
  form (`WDQ42`), a colon form (`domain:example.org`), or the tenant-relative `QQQ5` (read as
  `Q5`). Such IDs are carried as data; this crate does not decide which prefixes exist.
- Three extra data types, `wikibase-domain`, `wikibase-keyword` and `wikibase-notation`, are known
  by name.

## Example

```rust
use scatter_wikibase_model::{Entity, Form, Hasher};

let json = r#"{
    "type": "item", "id": "Q42",
    "labels": {"en": {"language": "en", "value": "Douglas Adams"}},
    "claims": {"P31": [{
        "type": "statement", "rank": "normal",
        "mainsnak": {"snaktype": "value", "property": "P31", "datatype": "wikibase-item",
            "datavalue": {"type": "wikibase-entityid",
                "value": {"entity-type": "item", "id": "Q5"}}}
    }]}
}"#;

let parsed = Entity::from_json(json).unwrap();
let entity = parsed.entity;
assert_eq!(entity.id.as_str(), "Q42");
assert_eq!(entity.statement_count(), 1);

// The served form adds `numeric-id` and recomputes the snak hash Wikibase would give.
let served = entity.to_value(Form::Wikibase(&Hasher::local()));
let snak = &served["claims"]["P31"][0]["mainsnak"];
assert_eq!(snak["datavalue"]["value"]["numeric-id"], 5);
assert_eq!(snak["hash"].as_str().unwrap().len(), 40);
```

## Licence

GPL-3.0-or-later. See `LICENSE`. The registry file is CC0-1.0, like everything in Triplespace's
`docs/`.

The code comments cite Triplespace's design documents, for example `0006 §2` (an architecture
decision record). They are published at <https://triplespace.readthedocs.io/>.
