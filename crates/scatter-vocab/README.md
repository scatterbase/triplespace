# scatter-vocab

IRI constants for RDF vocabularies, as `oxrdf::NamedNodeRef`s, so that code can use them in RDF
directly. It covers:
- the `scatter:` vocabulary of [Triplespace](https://triplespace.readthedocs.io/) and
  Scatterbase, minted under `https://scatter.red/terms/v0/`, with no stability promise yet;
- the vocabularies they reuse: PROV-O, PAV, SIOC, FOAF, Activity Streams, Dublin Core terms,
  schema.org, the Wikibase ontology, RDF, RDFS, OWL, SKOS, Creative Commons, GeoSPARQL and XSD.

Each vocabulary is a module with its namespace (`NS`), its conventional prefix (`PREFIX`), a
constant per term, and `TERMS`, every term it lists. `term` builds an IRI for a term a module
doesn't list.

Instance data (entity, revision, user and graph IRIs) is not here: those belong to each
instance's base.

## Example

```rust
use scatter_vocab::{rdfs, wikibase};

assert_eq!(rdfs::LABEL.as_str(), "http://www.w3.org/2000/01/rdf-schema#label");
assert_eq!(wikibase::ITEM.as_str(), "http://wikiba.se/ontology#Item");
assert_eq!(wikibase::PREFIX, "wikibase");

let custom = scatter_vocab::term(wikibase::NS, "Lexeme").unwrap();
assert_eq!(custom.as_str(), "http://wikiba.se/ontology#Lexeme");
```

## Licence

GPL-3.0-or-later. See `LICENSE`.

The code comments cite Triplespace's design documents, for example `0005 §5` (an architecture
decision record). They are published at <https://triplespace.readthedocs.io/>.
