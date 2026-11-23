# scatter-wikibase-rdf

Wikibase's RDF, format 1.0.0, from the Wikibase data model
([`scatter-wikibase-model`](https://crates.io/crates/scatter-wikibase-model)). It writes what
Wikibase's `Special:EntityData` and `dumpRdf.php` write, triple for triple, including where
they depart from `ontology.owl`.

- **The renderer.** `Renderer` writes an entity's triples under Wikibase's `RdfProducer`
  flags: the four `Special:EntityData` flavours (`simple`, `long`, `dump`, `full`) and the two
  dump-file flavours. That covers the document node, terms, sitelinks, the truthy triples of
  best-rank statements, and statement, reference and value nodes. In the `full` flavour it lists
  the properties and entities to stub, and writes each stub.
- **IRIs.** `IriScheme` names every node and predicate from the instance's bases (`Namespaces`).
  Entities named elsewhere come through the `EntityIris` trait. A stock Wikibase names
  everything under its own base, which is `OwnBase`; an instance that mirrors other Wikibases
  implements the trait. [Triplespace](https://triplespace.readthedocs.io/)'s implementation is
  in [`scatter-wikibase-ids`](https://crates.io/crates/scatter-wikibase-ids), under its `rdf`
  feature.
- **Writers.** `dump_header`, `to_ntriples` and `to_turtle`, with Wikibase's prefix names.
- **Diffs.** The triple-set difference between two renderings of an entity, and a SPARQL 1.1
  Update request that applies it.

It does no I/O; the renderer returns triples and the writers return strings.

## Example

```rust
use scatter_wikibase_model::{Entity, Hasher};
use scatter_wikibase_rdf::{Flavor, IriScheme, Namespaces, Renderer, to_ntriples};

let entity = Entity::from_json(r#"{
    "type": "item", "id": "Q42",
    "labels": {"en": {"language": "en", "value": "Douglas Adams"}}
}"#).unwrap().entity;

let iris = IriScheme::own(Namespaces::with_wiki_path("https://example.org/"));
let hasher = Hasher::local();
let triples = Renderer::new(&iris, &hasher, Flavor::Simple.produce()).entity(&entity, None);

let nt = to_ntriples(&triples);
assert!(nt.contains(
    "<https://example.org/entity/Q42> <http://www.w3.org/2000/01/rdf-schema#label> \"Douglas Adams\"@en ."
));
```

## Licence

GPL-3.0-or-later. See `LICENSE`.

The code comments cite Triplespace's design documents, for example `wikibase-compat.md §5`
(a compatibility contract) or `0002 A43` (an architecture decision record). They are published at
<https://triplespace.readthedocs.io/>.
