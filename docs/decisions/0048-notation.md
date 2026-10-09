# 0048. Notation: one keyed type for strings from outside vocabularies

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-09 (A3)
- **Author:** James Hare / Claude Opus
- **Changes:** [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0017](0017-entity-id-grammar.md), [0029](0029-resolver-namespaces.md), [0036](0036-openstreetmap-providers.md), [0037](0037-gdelt-provider.md), [0041](0041-content-models.md)
- **Uses:** [0014](0014-caches-and-search.md), [0031](0031-property-constraints.md)
- **Chapters:** [02](../architecture/02-graphs-rdf-and-query.md), [04](../architecture/04-entities-and-identifiers.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md)

## Context

Triplespace has three keyed entity types ([0009](0009-keyed-entity-types-and-domain.md) §1): Domain, Keyword ([0017](0017-entity-id-grammar.md) §5) and `osm-tag` ([0036](0036-openstreetmap-providers.md) §3). Keyword and `osm-tag` look alike. Both are strings that are their own identity, both have `clusters = false` because a string is not the concept it denotes, and both store descriptions and aliases. Two ways to tidy them up were considered: merging them into one type, or turning `osm-tag` into a general type for strings from any outside vocabulary.

**They cannot be merged, because their identity rules are opposite.** Keyword's normalizer casefolds and turns punctuation into hyphens, so `Fast Food`, `fast_food` and `fast:food` are one keyword. The point of that is convergence: an OpenAlex slug and a phrase an editor types land on the same entity. `osm-tag` has no normalizer, so `name` and `Name` are different tags, and so are `fast_food` and `fast-food`. 0036 §3 rejected the keyword normalizer for that reason. A merged type would need a normalizer chosen per vocabulary, which is the second option with Keyword as one vocabulary. The alternative is to make the exact string the identity and demote folding to a match key, as [0029](0029-resolver-namespaces.md) separates normalizers from match keys. That breaks Keyword: "Machine learning" typed by an editor and `machine-learning` from OpenAlex become two entities.

**One keyed type per vocabulary does not scale.** Every keyed type so far has cost:

- a namespace pair;
- a `wikibase-*` data type, which no Wikibase client knows ([0041](0041-content-models.md) asks whether these should become `triplespace-*`);
- an `EntityType` variant;
- a normalizer written in code.

210–219 is full: Table took 218/219, the last pair ([0045](0045-table-content-model.md) §2). The next vocabularies are already in view: ActivityPub hashtags on federated posts ([0022](0022-federation.md)) and IANA media types on files ([0039](0039-files-and-media.md) §2). Each would repeat all four costs.

James's direction, from the design discussion of 2026-10-01:

- `osm-tag` becomes a general type, named **Notation**, for strings from outside vocabularies.
- Keyword stays its own type.
- 310–319 and 320–329 are reserved as second ranges ([0008](0008-namespaces-and-document-pages.md) §2, by 0008 A17). That reservation is recorded in 0008 itself, not here.

## Decision

### 1. The `notation` keyed type (extends 0017 §3 and §5; amends 0036 §3)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.3, §3.4, §3.8.*

### 2. Schemes

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.8.*

### 3. What a notation is, and what it is not

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.6, §3.7, §3.9, §4.1.*

### 4. A property's scheme (extends 0003 §7)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.9.*

### 5. RDF

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §5.7.*

### 6. OpenStreetMap tags are the `osm` scheme (amends 0036 §3, §4, §5, §6 and §7)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.1, §3.8.*

### 7. Registry, namespaces and content models

*Current text: [23](../architecture/23-configuration-and-registry.md) §4.2, §4.3.*

### 8. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **A new outside vocabulary costs one registry entry.** It needs no namespace, no data type and no `EntityType` variant. It needs a grammar only when the existing ones do not fit.
- **One data type, so fewer unknown data types for Wikibase clients.** `wikibase-notation` replaces `wikibase-osm-tag`, and later vocabularies add nothing.
- **Properties restrict schemes themselves (§4).** A `wikibase-notation` property without a `notation-scheme` statement takes anything. That suits a general "has tag" property and is a mistake for an "OSM tag" property, so the property page should say when no scheme is set.
- **IDs and titles get longer.** `notation:osm:amenity=cafe` replaces `osmtag:amenity=cafe`, and `Notation:osm:amenity=cafe` replaces `OSM:amenity=cafe`. The value editor hides the scheme where a property allows one.
- **Keyword's convergence and a notation's exactness both survive,** and §3 draws the line between them.
- **Scheme names are a shared, permanent allocation,** like provider codes. They are never reused for another vocabulary.

## Open questions

- **Q1. Scheme names and interwiki prefixes.** MediaWiki refuses a talk-page title whose text begins with an interwiki prefix. A client that applies that rule would reject `Notation talk:osm:amenity=cafe` wherever `osm` is an interwiki prefix. The question is whether to refuse scheme names that collide with the Wikimedia interwiki map.
- **Q2. The IRI's form.** `https://scatter.red/notation/osm:amenity=cafe` keeps the key whole. `…/notation/osm/amenity=cafe` would nest notations under the scheme IRI, but needs a template with two parts. This must be settled before the first IRI is published.
- **Q3. Which candidate ships next,** `hashtag` with federation's inbound posts or `media-type` with files, and their grammars.
- **Q4. Data type IDs.** [0041](0041-content-models.md) Q2, on renaming `wikibase-domain`, `wikibase-keyword` and now `wikibase-notation` to `triplespace-*` before any data exists.
- **Q5. Search.** Whether the search document of a notation ([0014](0014-caches-and-search.md) §7) carries the scheme as a field for filtering, and how notations rank against keywords for the same input.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0003](0003-statement-ui.md) §7 | §4 | extends | 0003 A8 |
| [0004](0004-identity-clusters-and-equivalence.md) §1 | §3 | extends | 0004 A9 |
| [0005](0005-crate-organization.md) §2 | §8 | amends | 0005 A48 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §7 | amends | 0008 A18 |
| [0009](0009-keyed-entity-types-and-domain.md) §8, §9 | §3, §6 | extends | 0009 A6 |
| [0017](0017-entity-id-grammar.md) §3, §5 | §1 | extends | 0017 A7 |
| [0029](0029-resolver-namespaces.md) §2, §6 | §2 | extends | 0029 A5 |
| [0036](0036-openstreetmap-providers.md) §3, §4, §5, §6, §7 | §1, §3–4, §6–8 | amends | 0036 A2 |
| [0036](0036-openstreetmap-providers.md) | §1, §3–4, §6–8 | retitles | 0036 A2 |
| [0037](0037-gdelt-provider.md) §2 | §1, §6 | amends | 0037 A3 |
| [0041](0041-content-models.md) §3 | §1, §7 | amends | 0041 A4 |

## References

- [SKOS Reference §6, Notations](https://www.w3.org/TR/skos-reference/#notations) (`skos:notation`, typed literals per scheme)
- [Wikidata: P1282, OpenStreetMap tag or key](https://www.wikidata.org/wiki/Property:P1282)
- [OpenStreetMap wiki: Data items](https://wiki.openstreetmap.org/wiki/Data_items)
- [Mastodon: ActivityPub `Hashtag`](https://docs.joinmastodon.org/spec/activitypub/#Hashtag)
- [IANA Media Types](https://www.iana.org/assignments/media-types/media-types.xhtml)

## Amendment log

### A1. Crate table

- **Date:** 2026-10-01
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §8
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A48).

Replaced text (§8):

> | Crate | Change |
> |---|---|
> | `scatter-normalize` | The `notation` keyed type; the scheme registry, embedding `docs/registry/notation-schemes.toml`; the four normalizers; the `osm-tag` and `text` grammars. The `osm-tag` grammar is the former `osm-tag` normalizer, unchanged |
> | `scatter-wikibase-model` | The `notation` entity type and the `wikibase-notation` data type replace `osm-tag` and `wikibase-osm-tag` |
> | `scatter-wikibase-rdf` | The `skos:notation` and `skos:inScheme` triples (§5) |
> | `scatter-adapter-wikidata` | `key_scheme` in the key map (§6) |
> | `triplespace-titles` | The `Notation` namespace; scheme names in the `/resolve` order (§2) |
> | API crates | The `notation-scheme` check and `ts-notation-scheme` on the write path (§4) |
>
> No crate is added.

### A2. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §8
- **Summary:** A1 was folded into the Decision. The open questions were numbered, and the list this ADR kept under "Amendments to earlier ADRs" (quoted below) was replaced by the generated table. No decision changed. Before this, A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

Replaced text (Amendments to earlier ADRs):

> - **[0003](0003-statement-ui.md) §7:** the `notation-scheme` role (§4).
> - **[0005](0005-crate-organization.md) §2:** the rows of §8.
> - **[0008](0008-namespaces-and-document-pages.md) §2:** 216/217 are Notation and Notation talk (§7).
> - **[0017](0017-entity-id-grammar.md) §3 and §5:** a third keyed type whose key carries a scheme (§1).
> - **[0029](0029-resolver-namespaces.md) §2:** scheme names share the namespace of keyed-type and resolver names (§2).
> - **[0036](0036-openstreetmap-providers.md) §3–§7:** OSM keys and tags are notations in the `osm` scheme (§6).
> - **[0041](0041-content-models.md) §3:** `triplespace-notation` replaces `triplespace-osm-tag` (§7).

### A3. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [04](../architecture/04-entities-and-identifiers.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
