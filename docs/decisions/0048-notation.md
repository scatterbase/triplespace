# 0048. Notation: one keyed type for strings from outside vocabularies

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-01 (A2)
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

A **notation** is a string that names something in an outside vocabulary, taken exactly as that vocabulary's own rule says. Its **scheme** says which vocabulary. The name follows SKOS, where `skos:notation` is "a string of characters … used to uniquely identify a concept within the scope of a given concept scheme".

| | |
|---|---|
| Type name, ID prefix | `notation`, `notation:` |
| Key | The scheme name, a colon, and the string in the scheme's canonical form: `osm:amenity=cafe` |
| ID | `notation:osm:amenity=cafe` ([0017](0017-entity-id-grammar.md) §3: the prefix and the key) |
| Namespace | `Notation`, 216, with `Notation talk` at 217 (§7). Titles read `Notation:osm:amenity=cafe` |
| Data type | `wikibase-notation`, value type `wikibase-entityid` |
| IRI | `https://scatter.red/notation/{key}`, content-derived and shared by every instance ([0009](0009-keyed-entity-types-and-domain.md) §6): `https://scatter.red/notation/osm:amenity=cafe` |
| Grammar and normalizer | The scheme's (§2) |
| Display | Derived: the string, without the scheme, as the `mul` label; `wbsetlabel` fails with `derived-label` as on a Domain ([0009](0009-keyed-entity-types-and-domain.md) §5). Descriptions and aliases are stored and reconciled like an item's |
| Clusters | Never (§3) |

**Parsing a key.** A scheme name never contains a colon (§2), so the first colon of the key ends the scheme name, and everything after it is the string. A string may contain colons of its own, as `osm:addr:street=Main Street` does. The scheme name is matched case-insensitively and written in lowercase. The string is then normalized by the scheme's normalizer and checked against the scheme's grammar. A key whose scheme is not registered is not a valid key.

The rest of 0009 applies unchanged. A notation exists because its key is valid, with no create, redirect or delete (§4 of 0009). Log headers carry a surrogate, one sequence for the type, not the string (§7 of 0009). Upstream items can be key-mapped onto notations (§9 of 0009).

### 2. Schemes

Schemes are registry data, in a new file, `docs/registry/notation-schemes.toml`, embedded by `scatter-normalize`. Each entry has:

| Field | Meaning |
|---|---|
| `name` | Lowercase ASCII letters, digits and single hyphens, starting with a letter, at most 32 characters. No colon |
| `label` | The vocabulary's name, shown in the UI's identity line ("OSM tag or key") |
| `normalizer` | One of a fixed set: `exact` (the input must already be NFC and is not changed), `nfc`, `casefold` (NFC, then full Unicode case folding, as [0017](0017-entity-id-grammar.md) §5 and [0029](0029-resolver-namespaces.md) §1 use it) and `ascii-lowercase` (NFC, then ASCII letters lowercased) |
| `grammar` | The name of a grammar implemented in `scatter-normalize`, checked after normalizing: `osm-tag` (0036 §3), or `text`, which is 1 to 255 characters with no control characters and no leading or trailing whitespace |
| `documentation` | Optional. A URL template for the vocabulary's own page about a string, `{string}` being the string |
| `defined_in` | The ADR that registered the scheme |
| `retired` | Optional. A retired scheme stays in the file so that its name is never given to another vocabulary |

**A new scheme is a registry change, and a new grammar is code.** A scheme that names a grammar `scatter-normalize` does not implement is rejected when the file is parsed, as an unimplemented keyed type is. The normalizers are a closed set so that the rule that decides identity is never free text.

**Schemes are allocated in the registry only, never by an instance or a tenant.** A notation's ID and IRI are shared by every instance (§1), so two instances that gave the name `foo` to different vocabularies would describe different things with one IRI. This differs from resolvers, which a tenant may add ([0029](0029-resolver-namespaces.md) §8), because a resolver mints no shared identifier. A vocabulary a tenant wants for itself is modelled with ordinary items.

**Scheme names join the one namespace of keyed-type and resolver names** (extends [0029](0029-resolver-namespaces.md) §2). `osm:amenity=cafe` typed into `/resolve` or the search box means the notation, as `doi:10.1000/x` means the DOI resolver. `/resolve` tries such input as a notation key after keyed IDs and before resolver strings. No scheme may share a name with a keyed type or a resolver, and the registry tests check it.

**What ships.** One scheme, `osm` (§6). The file drafts two candidates, commented out as `resolvers.toml` drafts its candidates ([0029](0029-resolver-namespaces.md) §8):

- `hashtag`, `casefold`, for ActivityPub hashtags on federated posts ([0022](0022-federation.md) §8). It needs a `hashtag` grammar.
- `media-type`, `ascii-lowercase`, for IANA media types such as `text/html`, the `mime` field of an upload ([0039](0039-files-and-media.md) §2). It needs a `media-type` grammar.

### 3. What a notation is, and what it is not

Four mechanisms now give strings an identity. The question that tells them apart is who decides that two strings are the same thing.

| Mechanism | Use it when | Example |
|---|---|---|
| **Notation** | An outside system defines the string, and its own rule decides sameness. The set is open: anyone may coin a new string. No authority mints an ID for it. | `notation:osm:amenity=cafe` |
| **Keyword** ([0017](0017-entity-id-grammar.md) §5) | The string is a phrase about a work, and variants should converge whoever writes them. Labels are editable because folding loses case | `keyword:machine-learning` |
| **Provider item** ([0037](0037-gdelt-provider.md) §2) | One provider publishes and identifies a finite list | `GDTTAX_FNCACT_MAYOR`, a GDELT theme |
| **Resolver** ([0029](0029-resolver-namespaces.md)) | The string identifies a thing that already has an item | A DOI |

**Domain stays its own type.** In principle, a domain name could be a scheme. It stays a type of its own for three reasons. Domains join identity clusters, and notations never do. Its derived label is a conversion (the U-label), not the string itself. Sitelink hosts ([0026](0026-sitelinks.md) §5) and source domains ([0037](0037-gdelt-provider.md) §3) depend on it.

**Keyword stays its own type,** for the reasons in the Context: convergence is the point of a keyword, and a notation preserves the distinctions a keyword erases.

**Notations never join identity clusters** (uses [0004](0004-identity-clusters-and-equivalence.md) §3), as 0036 §6 decided for `osm-tag` and [0017](0017-entity-id-grammar.md) §5 for keywords. `notation:osm:amenity=cafe` is a tag, and a Wikidata item about cafés is the concept. `same-as` with a notation on either side is rejected at write time, and no tier-2 or tier-3 link may produce one. A notation may carry an ordinary statement pointing at the concept it usually denotes.

### 4. A property's scheme (extends 0003 §7)

With one data type for every scheme, the data type no longer limits which strings a property takes. A property whose values should be OSM tags must not accept hashtags.

**A new role, `notation-scheme`, names the property that restricts the scheme.** It is a `string` property, and a `wikibase-notation` property may carry one or more statements with it whose values are scheme names. A value whose scheme is not among them is refused on the write path with `ts-notation-scheme`, in the same check that refuses a value of the wrong data type. A `wikibase-notation` property with no such statement takes any registered scheme, which is what a general "has tag" property needs.

**This is a type check, not a constraint** ([0031](0031-property-constraints.md)). Constraints report and never refuse (0031 §2), and they read the graph after the fact. The scheme belongs with the data type: it is part of what kind of value the property takes, read from the property itself when its data type is read. A change to the scheme statement applies to later writes, and values already stored are left as they are.

The UI's value editor ([0003](0003-statement-ui.md) §8) offers the property's schemes. When it has exactly one, the editor hides the scheme, so an editor types `amenity=cafe` and the editor stores `notation:osm:amenity=cafe`.

### 5. RDF

A notation's concept IRI is its subject (§1). Beside the triples every keyed entity gets, it carries:

```turtle
<https://scatter.red/notation/osm:amenity=cafe>
    skos:notation "amenity=cafe"^^<https://scatter.red/notation-scheme/osm> ;
    skos:inScheme <https://scatter.red/notation-scheme/osm> .
```

SKOS types a notation's literal with a datatype named for its scheme, and this follows it. `https://scatter.red/notation-scheme/{name}` is the scheme's IRI, which is content-derived like the notation's own. A notation is **not** typed `skos:Concept`: it is the string, not the concept (§3), and SKOS gives `skos:notation` and `skos:inScheme` no domain that would imply otherwise.

### 6. OpenStreetMap tags are the `osm` scheme (amends 0036 §3, §4, §5, §6 and §7)

```toml
[[scheme]]
name = "osm"
label = "OSM tag or key"
normalizer = "exact"
grammar = "osm-tag"
defined_in = "0036 §3, 0048 §6"
```

What 0036 decided about tags stands. Only the type it decided them into changes:

| 0036 | Now |
|---|---|
| §3: keyed type `osm-tag`, ID `osmtag:amenity=cafe` | Scheme `osm`, ID `notation:osm:amenity=cafe` |
| §3: namespace `OSM` 216/217, title `OSM:amenity=cafe` | `Notation` 216/217, title `Notation:osm:amenity=cafe` |
| §3: data type `wikibase-osm-tag` | `wikibase-notation`, on properties whose `notation-scheme` is `osm` |
| §3: grammar; no normalizer | Unchanged: the `osm-tag` grammar and the `exact` normalizer |
| §4: roles `osm-tag`, `osm-tag-text`, `osm-key` | Unchanged. Their values are `notation:osm:k=v` and `notation:osm:k`, and the properties bound to `osm-tag` and `osm-key` carry `notation-scheme` `osm` |
| §5: `OW` items of `Q7` and `Q2` key-mapped to `osm-tag` | Key-mapped to `notation`, with the key prefixed `osm:`; the key map gains `key_scheme = "osm"` |
| §6: `clusters = false` | Unchanged, as a property of `notation` (§3) |
| §7: `keyed-types.toml` gains `osm-tag`; `OSM` namespace | §7 below |

The `osm` scheme has no `documentation` template. The wiki's pages are `Key:amenity` and `Tag:amenity=cafe`, two forms one template cannot produce, and a mapped notation already carries the wiki's documentation-page statements (0036 §5).

[0036](0036-openstreetmap-providers.md) Q6, on whether a key and its tags should be two types, now reads as whether they should be two schemes. That is a registry change.

### 7. Registry, namespaces and content models

- **`keyed-types.toml`:** `osm-tag` is replaced by `notation`. The registry had allocated `osm-tag` but no instance holds data under it, so it is replaced rather than retired.
- **`notation-schemes.toml`:** new (§2), with `osm` and the two candidates.
- **`namespaces.toml`:** 216 and 217 are renamed from `OSM` and `OSM talk` to `Notation` and `Notation talk` (amends [0008](0008-namespaces-and-document-pages.md) §2), with normalizer `notation`. The numbers serve the same pages they were allocated for, now with the scheme in the title. They have not yet been filed on mediawiki.org, so the registration is filed under the new name.
- **`content-models.toml`:** `triplespace-osm-tag` becomes `triplespace-notation`, entity type `notation` (amends [0041](0041-content-models.md) §3).
- **`providers.toml`:** the `OW` key map names `key_type = "notation"` and `key_scheme = "osm"`.

### 8. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed. The table this section first gave is in A1.

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
