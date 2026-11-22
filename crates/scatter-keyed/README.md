# scatter-keyed

Keyed entity types for Wikibase, in pure Rust: entities whose ID is a normalized natural key rather
than a minted number.

Wikibase gives every item a number (`Q42`). Some things already have a name that identifies them
exactly: a domain name, a keyword, an OpenStreetMap tag. A keyed entity type makes that name the
entity's ID, `domain:en.wikipedia.org`, `keyword:climate-change`, `notation:osm:amenity=cafe`, so
two wikis that describe the same domain arrive at the same entity without coordinating.

- **The registry** (`keyed`): each type's namespace, Wikibase data type, concept IRI template, ID
  prefix and display rule, embedded from `registry/keyed-types.toml`. `KeyedRegistry::parse` reads
  your own.
- **Grammars and normalizers**: a Domain key is a host normalized by UTS #46 (from
  [`scatter-normalize`](https://crates.io/crates/scatter-normalize)); a Keyword key is NFKC,
  casefolded, with runs of punctuation and space made one hyphen (`keyword`); a Notation key is a
  scheme and a string normalized by that scheme's rules (`notation`), with the schemes embedded from
  `registry/notation-schemes.toml` and the `osm` grammar built in. `KeyedRegistry::parse_with_schemes`
  takes your own schemes.
- **Surrogates** (`surrogate`): the allocator trait that keeps keys, which can be personal data, out
  of an append-only log's headers.

It is the extension crate of a pair: [`scatter-normalize`](https://crates.io/crates/scatter-normalize)
holds what any Wikibase can use, and this crate holds Triplespace's extension. Both are part of
[Triplespace](https://triplespace.readthedocs.io/). The crate does no I/O and builds for
`wasm32-unknown-unknown`.

## Example

```rust
use scatter_keyed::KeyedRegistry;

let registry = KeyedRegistry::default_registry();

let id = registry.parse_id("domain:EN.Wikipedia.org.").unwrap();
assert_eq!(id.to_string(), "domain:en.wikipedia.org");

let keyword = registry.by_name("keyword").unwrap();
assert_eq!(keyword.normalize("Climate Change").unwrap(), "climate-change");

// A notation is normalized by its scheme.
let notation = registry.by_name("notation").unwrap();
assert_eq!(notation.normalize("OSM:amenity=cafe").unwrap(), "osm:amenity=cafe");
```

## Licence

GPL-3.0-or-later. See `LICENSE`. The registry files are CC0-1.0, like everything in Triplespace's
`docs/`.

The code comments cite Triplespace's design documents, for example `0009 §2` (an architecture
decision record). They are published at <https://triplespace.readthedocs.io/>.
