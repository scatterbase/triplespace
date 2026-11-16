# 0048. Notation: one keyed type for strings from outside vocabularies

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-11-16 (A5)
- **Author:** James Hare / Claude Opus
- **Changes:** [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0017](0017-entity-id-grammar.md), [0029](0029-resolver-namespaces.md), [0036](0036-openstreetmap-providers.md), [0037](0037-gdelt-provider.md), [0041](0041-content-models.md)
- **Uses:** [0014](0014-caches-and-search.md), [0031](0031-property-constraints.md)

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

*Changed by A4, A5.*

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

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

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

Replaced text: §8 of this ADR, as it stood before this entry; in git (`tools/adr.py log 0048 A1 --before`).

### A2. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §8
- **Summary:** A1 was folded into the Decision. The open questions were numbered, and the list this ADR kept under "Amendments to earlier ADRs" (quoted below) was replaced by the generated table. No decision changed. Before this, A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

Replaced text: Amendments to earlier ADRs of this ADR, as it stood before this entry; in git (`tools/adr.py log 0048 A2 --before`).

### A3. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text moved to the chapter sections its pointers name; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and log. No decision changed. The text as it stood here is in git (`tools/adr.py log 0048 A3 --diff`).

### A4. A remote key is not a notation

- **Date:** 2026-11-15
- **Source:** [0110](0110-remote-pages-and-revisions.md) §3
- **Change:** extends §3
- **Summary:** The table of mechanisms that give strings an identity gains a fifth row, the remote key, which names a page or revision itself rather than a term for it, as a Domain names its domain. Host names, like scheme names, are allocated in the registry only. Chapter text: [04](../architecture/04-entities-and-identifiers.md) §3.9.

### A5. The test for a keyed type

- **Date:** 2026-11-16
- **Source:** Direct: James, design discussion of 2026-11-16
- **Change:** extends §3
- **Summary:** A new keyed type must name something self-validating in identity, so the key alone says what it names; a vocabulary of terms stays a notation scheme. Remote is one keyed type standing for what would be one per host ([0110](0110-remote-pages-and-revisions.md) A5). Chapter text: [04](../architecture/04-entities-and-identifiers.md) §3.9.
