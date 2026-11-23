# scatter-wikibase-ids

How [Triplespace](https://triplespace.readthedocs.io/) names Wikibase entities. This is an extension
of [`scatter-wikibase-model`](https://crates.io/crates/scatter-wikibase-model): the model carries any
entity ID as data, and this crate says what the IDs mean.

- **Three ID forms.** An ID is one of:
  - *local*: `Q42`, the instance's own;
  - *foreign*: `WDQ42`, Wikidata's `Q42` under a provider's prefix, or `lb:Q7`, a source's;
  - *keyed*: `domain:example.org`, an entity named by a normalized natural key.

  The `TriplespaceId` trait tells them apart on the model's `EntityId`, and validates an ID
  against a provider model and a keyed-type registry.
- **Providers as a trait.** `ProviderModel` is what the ID forms need to know about providers:
  how a foreign ID is canonicalized, and which IRI prefixes its values use. Triplespace's own
  model, [`scatter-providers`](https://crates.io/crates/scatter-providers), implements it behind the
  default `providers` feature. That feature also brings `hasher_for`, which gives the model's
  hasher for a provider's IDs. If you have another model, implement the trait and turn the
  default feature off.
- **Value IRIs stored as entity IDs.** `ValueIris` stores the `unit`, `globe` and `calendarmodel`
  of a value as the entity ID they name (`WDQ11573`). It expands them back to the IRIs Wikibase
  writes, both for output and before hashes are taken.

- **Entity IRIs in RDF**, with the `rdf` feature. `TriplespaceIris` is an implementation of
  [`scatter-wikibase-rdf`](https://crates.io/crates/scatter-wikibase-rdf)'s `EntityIris`. It
  names a foreign ID by its provider's IRI template, and a keyed ID by its type's template. With
  `providers` as well, `rdf::upstream_nodes` names mirrored statements as their upstream names
  them.

It does no I/O and builds for `wasm32-unknown-unknown`.

## Example

```rust
use scatter_keyed::KeyedRegistry;
use scatter_wikibase_ids::{IdForm, ProviderModel, TriplespaceId, ValueIris};
use scatter_wikibase_model::EntityId;

/// A provider model of one provider, `XX`, whose items are `XXQ…`.
struct Mine;

impl ProviderModel for Mine {
    fn canonical_foreign_id(&self, id: &str) -> Result<String, String> {
        let id = id.to_ascii_uppercase();
        match id.strip_prefix("XXQ") {
            Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => Ok(id),
            _ => Err(format!("{id} is not an XX item")),
        }
    }
    fn value_iri_prefixes(&self) -> Vec<(String, String)> {
        vec![("XXQ".into(), "https://xx.example/entity/Q".into())]
    }
}

let keyed = KeyedRegistry::default_registry();
let id = EntityId::parse("xxq7").unwrap();
assert_eq!(id.form(), IdForm::Foreign);
assert_eq!(id.validate(&Mine, keyed).unwrap().as_str(), "XXQ7");
assert_eq!(EntityId::parse("Q42").unwrap().form(), IdForm::Local);

let iris = ValueIris::new(&Mine, Some("https://my.example/entity/"));
assert_eq!(iris.store("https://xx.example/entity/Q7"), "XXQ7");
assert_eq!(iris.expand("Q42"), "https://my.example/entity/Q42");
```

## Licence

GPL-3.0-or-later. See `LICENSE`.

The code comments cite Triplespace's design documents, for example `0017 §1` (an architecture
decision record). They are published at <https://triplespace.readthedocs.io/>.
