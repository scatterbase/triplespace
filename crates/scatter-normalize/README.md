# scatter-normalize

Normalizers and comparison keys for Wikibase values, in pure Rust.

Two values that mean the same thing are often written differently: `HTTP://Example.com:80/a%7e`
and `http://example.com/a~` are one URL, `doi:10.1000/ABC` and `https://doi.org/10.1000/abc` one DOI.
This crate turns a value into its canonical key, so values can be compared, deduplicated and looked
up by what they mean.

- **URLs** (`url`): WHATWG parsing, an allowed set of schemes, the host normalized, a default
  port dropped, percent-encoding normalized (RFC 3986 §6.2.2). Nothing else is folded: `/Foo` and
  `/Foo/` stay different.
- **Hosts** (`domain`): UTS #46 processing, non-transitional, to a lowercase A-label.
- **Value keys** (`ValueKey`): the key of a Wikibase value by its data type: `string` (NFC),
  `monolingualtext`, `time`, `quantity`, `url`, `external-id`.
- **Resolvers** (`resolver`): an identifier scheme bound to a property, such as DOI, whose
  normalizer and case rule apply to every value of that property. The defaults are embedded from
  `registry/resolvers.toml`; `ResolverRegistry::parse` reads your own.

Every normalizer is idempotent, and a test holds each to it. The crate does no I/O and builds for
`wasm32-unknown-unknown`, so a browser can normalize with the same code as a server.

It is the base crate of a pair. Keyed entity types, an extension to Wikibase that identifies an
entity by its normalized natural key, are in
[`scatter-keyed`](https://crates.io/crates/scatter-keyed), which builds on this one. Both are part
of [Triplespace](https://triplespace.readthedocs.io/).

## Example

```rust
use scatter_normalize::{normalize_url, ResolverRegistry, ValueKey};
use scatter_normalize::url::DEFAULT_SCHEMES;

assert_eq!(
    normalize_url("HTTP://Example.COM:80/a%7e", DEFAULT_SCHEMES).unwrap(),
    "http://example.com/a~"
);

let doi = ResolverRegistry::default_registry().by_name("doi").unwrap();
assert_eq!(doi.normalize("https://doi.org/10.1000/ABC").unwrap(), "10.1000/abc");

// Values of a property bound to the DOI resolver compare by their DOI.
let a = ValueKey::external_id("doi:10.1000/ABC", Some(doi)).unwrap();
let b = ValueKey::external_id("https://doi.org/10.1000/abc", Some(doi)).unwrap();
assert_eq!(a, b);
```

## Licence

GPL-3.0-or-later. See `LICENSE`. The registry file is CC0-1.0, like everything in Triplespace's
`docs/`.

The code comments cite Triplespace's design documents, for example `0026 §1` (an architecture
decision record). They are published at <https://triplespace.readthedocs.io/>.
