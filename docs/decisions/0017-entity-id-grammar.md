# 0017. Entity ID grammar

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-01 (A8)
- **Author:** James Hare / Claude Fable
- **Changes:** [0000](0000-init.md), [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0003](0003-statement-ui.md), [0007](0007-actor-identity.md), [0011](0011-logs.md), [0016](0016-permissions-and-access-control.md)

## Context

[0000](0000-init.md) §3 gives foreign entities a two-letter provider code and a one-letter type code, and [0009](0009-keyed-entity-types-and-domain.md) §3 relies on the rest of every minted ID being digits: "a string that matches the entity ID grammar is an entity ID, and anything else is tried as a keyed type." Two providers in `docs/registry/providers.toml` ([0015](0015-record-format-and-partition-registry.md) §5) break that assumption from opposite sides.

- **MusicBrainz** identifies every entity by an MBID, which is a UUID. `MBA` followed by digits does not exist.
- **OpenAlex keywords** are identified by slugs, `keywords/machine-learning`, and OpenAlex defines a keyword as "a short phrase describing what a work is about". That is a natural key, not a minted identifier: a keyword is the phrase itself, so it belongs with Domain among the keyed types of 0009 §1, not among minted `OA` entities.

Keywords then expose a second problem. 0009 §3 lets a Domain's ID be its bare key because a valid domain name can never look like an entity ID. Keyword keys can: `p53`, `q10`, `cd4` and `h2o` are all ordinary keywords, and `p53` is also the local property P53. Disjointness cannot be proved for every keyed type, and a bare key in a URL or an API parameter does not say what kind of thing it names. A domain name with nothing marking it as a domain name is unclear in the same way, even where it is unambiguous.

This ADR replaces the one grammar of 0009 §3 with three, each declared in the registry, and registers Keyword and MusicBrainz under them.

## Decision

### 1. Three ID forms

*Changed by A5, A6.*

Every entity ID has one of three forms, told apart by its first character and by the presence of a colon:

| Form | Shape | Examples | Minted by |
|---|---|---|---|
| **Local** | One uppercase letter and digits | `Q5`, `P12`, `L3` | This instance ([0000](0000-init.md) §3) |
| **Foreign** | Two-letter provider code, one-letter type code, and the rest in the type's **ID grammar** (§2) | `WDQ42`, `OAW123`, `MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d` | A provider (0000 §3, [0002](0002-source-graphs-and-mass-ingest.md) §1) |
| **Keyed** | The keyed type's name, a colon, and the normalized key | `domain:en.wikipedia.org`, `keyword:p53` | The thing itself ([0009](0009-keyed-entity-types-and-domain.md) §1) |

No minted ID contains a colon, and every keyed ID does, so the two never collide, whatever a key looks like. This replaces the disjointness argument of 0009 §3, and it applies uniformly: **a Domain's ID is now `domain:en.wikipedia.org`**, not the bare key. The colon form is the one Wikibase already uses for entities from another repository (`wikidata:Q42` in a federated install), so clients that parse federated IDs parse these.

**Statement IDs** keep Wikibase's form, `<EntityId>$<UUID>`: `domain:en.wikipedia.org$C7C2A847-…`. **Entity values** carry the same ID: `{"entity-type": "domain", "id": "domain:en.wikipedia.org"}`.

**Titles are unchanged.** In an entity namespace the title is the namespace and the bare key or ID, as [0008](0008-namespaces-and-document-pages.md) §1 and [0009](0009-keyed-entity-types-and-domain.md) §11 already say: `Domain:en.wikipedia.org`, `Keyword:p53`, `Item:MBAb10bbbfc-…`. The namespace supplies what the colon supplies in an ID. `Special:EntityPage/{id}` takes the ID form.

**Bare keys are not IDs.** `wbgetentities&ids=en.wikipedia.org` is an `invalid-entity-id`. The title resolver and `GET /resolve` ([0012](0012-api-requirements.md) §5) still recognise a bare string that normalizes to a valid key of some keyed type and offer it as a suggestion, so the search box's "Go to" ([0010](0010-site-ui.md) §3) keeps working for a pasted domain name.

**Derived IDs.** `M` and digits, a local-form ID, names the statements of the File page with that page ID (WikibaseMediaInfo's contract). It is derived, not minted: never in `view.entity`, never in a cluster, never the value of an entity data type.

**An input form.** A type letter written three times and the rest of a local ID, `QQQ5`, is an **input form** of the local ID on the tenant that reads it. It is canonicalized to `Q5` wherever IDs are accepted and never stored or returned, except as Lua output under a remapped letter ([0043](0043-lua-modules.md) §8).

### 2. ID grammars for minted types

*Changed by A4, A6.*

Each foreign entity type declares an **ID grammar** in `providers.toml`. It says what may follow the three-letter prefix and what the canonical spelling is.

| Grammar | Rest of the ID | Canonical form | Used by |
|---|---|---|---|
| `digits` | One or more decimal digits | As given; leading zeros are significant where upstream has them (a CAMEO code is `0311`, [0037](0037-gdelt-provider.md) §2). Providers that never mint them, such as Wikidata, never produce them | Wikidata, Librarybase, OpenAlex (the default) |
| `uuid` | An RFC 4122 UUID in its 36-character hyphenated text form | Lowercase | MusicBrainz |
| `gdelt-record` | 14 digits, `-`, an optional uppercase `T`, digits | As given | GDELT documents ([0037](0037-gdelt-provider.md) §2) |
| `token` | Uppercase letters, digits and underscores, starting with a letter, at most 128 characters | As given | GDELT themes ([0037](0037-gdelt-provider.md) §2) |

Input is case-insensitive in every position. **Canonicalizing an ID uppercases its prefix and applies the grammar's canonical form to the rest**, so `mbab10BBBFC-…` becomes `MBAb10bbbfc-…`. This amends the `entity-id` normalizer of [0008](0008-namespaces-and-document-pages.md) §3, which uppercased the whole ID, and the ID rewriting of [0002](0002-source-graphs-and-mass-ingest.md) §4, which becomes: strip or add the prefix, and canonicalize the rest by the type's grammar. The rewriting of upstream IDs also honours `upstream_prefix`, so `keywords/…` and MusicBrainz's bare MBIDs both round-trip.

A grammar is declared per provider and may be set on a single type, overriding the provider's ([0037](0037-gdelt-provider.md) §2). A new grammar is added by ADR, as a value the registry accepts. Nothing else in the system needs to know a grammar's rules: the ID parser in `scatter-wikibase-model` ([0005](0005-crate-organization.md) §2) reads them from the registry passed in as data (0005 §3, rule 3).

**Reserved codes.** The provider codes `AA` through `ZZ` are reserved and never allocated, so that the tenant-relative input form (§1) is unambiguous.

### 3. Keyed IDs

*Changed by A7.*

The keyed-type registry ([0009](0009-keyed-entity-types-and-domain.md) §1, `keyed-types.toml`) gains one field, the **ID prefix**, which is the type's name followed by a colon. The canonical ID is the prefix, in lowercase, and the key in the form the type's normalizer produces. A key may itself carry a scheme: the `notation` type's key is a scheme name, a colon and the string in the scheme's canonical form, so its ID is `notation:osm:amenity=cafe` ([0048](0048-notation.md) §1). Everything 0009 §7 says about log header keys stands: the log never sees the key, only the surrogate.

Where an earlier ADR shows a bare Domain key as an ID, it now reads with the prefix. That is [0009](0009-keyed-entity-types-and-domain.md) §3 (the JSON `id` and the value form), §4 (`wbgetentities`), §9 (mapped upstream items are written under `domain:{key}` as their subject) and the wire format of §10, which becomes:

```
{"op":"add","id":"domain:en.wikipedia.org","claims":{"P12":[…]}}
{"op":"same-as","ids":["Q77","domain:en.wikipedia.org"]}
```

The concept IRI is unchanged, `https://scatter.red/domain/{key}`: the prefix is part of the ID, not of the key.

### 4. Where IDs are stored and shown

The ID form of §1 is the one that appears everywhere an entity ID does: `view.entity.id` and every column that references it ([0013](0013-postgres-storage.md) §5), the `id` field of a search document ([0014](0014-caches-and-search.md) §7, whose `key` field keeps the bare key), the `target_id` of an ACL ([0016](0016-permissions-and-access-control.md) §4), the ID chip in the UI ([0010](0010-site-ui.md) §2), and canonical JSON. The UI's identity line names the kind of thing in words, "Domain" or "Keyword", and the chip shows the ID as it is.

### 5. Keyword: a second keyed type (extends 0009 §1)

*Changed by A2, A3, A7.*

A **keyword** is a short phrase describing what a work is about. It is the phrase, not the thing the phrase denotes.

| | |
|---|---|
| Type name, ID prefix | `keyword`, `keyword:` |
| Namespace | `Keyword`, 212, with `Keyword talk` at 213 ([0008](0008-namespaces-and-document-pages.md) §2) |
| Data type | `wikibase-keyword`, with value type `wikibase-entityid` as `wikibase-domain` has |
| IRI | `https://scatter.red/keyword/{key}`, content-derived and so shared by every instance ([0005](0005-crate-organization.md) §5, [0009](0009-keyed-entity-types-and-domain.md) §6) |
| Grammar | 1 to 200 characters of letters, digits and single hyphens; at least one letter; no leading or trailing hyphen |
| Normalizer | Unicode NFKC; casefold; every run of whitespace or punctuation becomes one hyphen; leading and trailing hyphens are removed |

**Labels are editable, not derived.** A Domain's label is derived from its key ([0009](0009-keyed-entity-types-and-domain.md) §5) because the key is the name. A keyword's key loses case and punctuation (`dna`, `type-1-diabetes`), so its labels and descriptions are stored and reconciled like an item's ([0002](0002-source-graphs-and-mass-ingest.md) §3). When no label exists in the requested language, the UI shows the key with hyphens as spaces. The keyed-type registry therefore distinguishes a **derived** display rule from an **editable** one.

**Keywords never join identity clusters (amends 0004 §1 and 0009 §8).** A Wikidata item about machine learning is the concept; `keyword:machine-learning` is a phrase. Treating them as one thing would make the phrase canonical over the concept, since keyed members rank first. So `same-as` with a keyword as either side is rejected at write time, as a data-type mismatch between properties is (0004 §6), and no tier-2 or tier-3 link may produce one. A keyword may carry an ordinary statement pointing at the concept it usually denotes, through a role in the role map ([0003](0003-statement-ui.md) §7); that is data, not identity.

**OpenAlex keywords are mapped by key** ([0009](0009-keyed-entity-types-and-domain.md) §9), as internetdomains.wiki's items are mapped to Domains. The adapter turns `https://openalex.org/keywords/{slug}` into the key by running the slug through the normalizer, writes the entity's `display_name` as the mirror graph's English label, emits `<https://openalex.org/keywords/{slug}> owl:sameAs <https://scatter.red/keyword/{key}>`, and turns a work's `keywords` list into `wikibase-keyword` values with the score as a qualifier by role. The `OAK` type code stays reserved in the registry and no `OAK` entity is ever minted.

**Later keyed types.** The third keyed type is `notation` ([0048](0048-notation.md) §1), a string in an outside vocabulary with the scheme in its key; it replaced `osm-tag` ([0036](0036-openstreetmap-providers.md) §3), whose keys and tags are now the `osm` scheme. DOI is not a keyed type but a resolver ([0029](0029-resolver-namespaces.md) §1), and resolver names and keyed-type names share one namespace, so `doi:10.1000/xyz` can mean only one thing ([0029](0029-resolver-namespaces.md) §2).

### 6. MusicBrainz

MusicBrainz is registered in `providers.toml` with code `MB`, slug `musicbrainz` and provider number 4 ([0015](0015-record-format-and-partition-registry.md) §2, §5). Every type has ID grammar `uuid`, so an ID is the prefix and the MBID verbatim: `MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d`. Keeping the MBID intact was preferred to a shorter encoding because the ID stays recognisable and pastes into musicbrainz.org unchanged.

| Code | Type | Code | Type |
|---|---|---|---|
| `A` | Artist | `P` | Place |
| `G` | Release group | `E` | Event |
| `R` | Release | `S` | Series |
| `C` | Recording | `I` | Instrument |
| `W` | Work | `N` | Genre |
| `L` | Label | `D` | Area |

MusicBrainz `url` entities are not mirrored as items; the adapter turns them into URL-valued statements on the entity that links them. Concept IRIs are `https://musicbrainz.org/{type}/{mbid}`. MusicBrainz has global edit numbers but no per-entity revision IDs, so `revision_ids` is false and a mirror record's revision ID uses its offset (0015 §2). Its issuer, `musicbrainz`, is provider-only ([0007](0007-actor-identity.md) §6) until edit history is mirrored, since the data the adapter reads carries no editors. The adapter is a new crate, `scatter-adapter-musicbrainz` (§7).

### 7. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed and `scatter-adapter-musicbrainz`. The table this section first gave is in A1.

## Consequences

- **One rule for keyed IDs.** Every keyed ID says what kind of key it is. Domain IDs grow by seven characters, and every example in 0009 reads with the prefix.
- **The ID parser is table-driven.** A provider with an unusual ID scheme is a registry entry and, at most, a new grammar value, not a change to the parser.
- **MusicBrainz IDs are long.** Thirty-nine characters in URLs, chips and statement IDs. That is the cost of keeping the MBID legible.
- **Keywords are phrases.** They carry no identity claim about the world, so they stay out of clusters, and a keyword page can say which concept it usually denotes without being that concept.
- ~~**DOI is now easier.** The obstacle 0009 listed for a DOI keyed type, that its keys could not be shown disjoint from entity IDs, is gone; `doi:` needs only its grammar, normalizer and IRI settled.~~ *DOI became a resolver, not a keyed type (A2).*
- **Stock Wikibase clients still cannot handle keyed entities**, as 0009 already noted; the colon form at least fails in a way federation-aware clients recognise.
- **One more crate.** `scatter-adapter-musicbrainz` joins the adapters in the build order's last step ([0005](0005-crate-organization.md) §7).

## Open questions

- **Q1. Keyword keys in scripts without word boundaries.** The normalizer produces one hyphen per run of whitespace or punctuation; a CJK phrase becomes its own key unchanged. Whether that is the right key, or whether keys should be transliterated or segmented, needs a look at OpenAlex's non-English keywords.
- **Q2. Keyword score.** Which role carries OpenAlex's per-work keyword score, and whether it should be a qualifier or dropped.
- **Q3.** ~~**Namespace numbers** for `Keyword` and `Keyword talk`, to be registered with Domain's.~~ *Settled by `docs/registry/namespaces.toml`, under the numbering policy of 0008 A6: 212/213.*
- **Q4. MusicBrainz edit history.** Whether to mirror MusicBrainz edits as log events ([0011](0011-logs.md)), which would give its issuer numeric editors.
- **Q5. A `slug` grammar.** No registered minted type needs one now that keywords are keyed; whether to define it in advance for a future provider.
- **Q6.** ~~**DOI.** Its grammar, normalizer and whether its IRI is `https://doi.org/{key}` or `https://scatter.red/doi/{key}` ([0009](0009-keyed-entity-types-and-domain.md) Q6).~~ *Settled by [0029](0029-resolver-namespaces.md) §1: a resolver, not a keyed type; the `doi:` prefix is withdrawn, and resolver names share the keyed-type namespace.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0000](0000-init.md) §3 | §1–2, §5 | amends | 0000 A3 |
| [0002](0002-source-graphs-and-mass-ingest.md) §4 | §2 | amends | 0002 A7 |
| [0004](0004-identity-clusters-and-equivalence.md) §1 | §5 | extends | 0004 A2 |
| [0005](0005-crate-organization.md) §2 | §7 | extends | 0005 A13 |
| [0008](0008-namespaces-and-document-pages.md) §3 | §2 | amends | 0008 A2 |
| [0009](0009-keyed-entity-types-and-domain.md) §3, §4, §9, §10 | §1, §3, §5 | amends | 0009 A2 |
| [0009](0009-keyed-entity-types-and-domain.md) §1, §5, §8 | §1, §3, §5 | extends | 0009 A2 |
| [0009](0009-keyed-entity-types-and-domain.md) Q6 | §5 | settles | 0009 Q6 |
| [0010](0010-site-ui.md) §3 | §1, §4 | amends | 0010 A7 |
| [0012](0012-api-requirements.md) §5 | §1 | amends | 0012 A6 |
| [0013](0013-postgres-storage.md) §5.1 | §4 | amends | 0013 A4 |
| [0014](0014-caches-and-search.md) §7 | §4 | amends | 0014 A2 |
| [0015](0015-record-format-and-partition-registry.md) §5 | §6 | extends | 0015 A2 |

## References

- [Wikibase entity IDs and federation](https://doc.wikimedia.org/Wikibase/master/php/docs_topics_entitysources.html) (prefixed IDs for entities from another repository)
- [OpenAlex keywords](https://help.openalex.org/hc/en-us/articles/24736201130391-Keywords) ("a short phrase describing what a work is about")
- [MusicBrainz identifiers](https://musicbrainz.org/doc/MusicBrainz_Identifier) and [entities](https://musicbrainz.org/doc/MusicBrainz_Entity)
- [RFC 4122](https://www.rfc-editor.org/rfc/rfc4122) (UUID text form)
- `docs/registry/providers.toml`, `keyed-types.toml`, `namespaces.toml`

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §7
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A13).

Replaced text (§7):

> | Crate | Change |
> |---|---|
> | `scatter-wikibase-model` | The ID parser reads the three forms of §1, the grammars of §2 and the prefixes of §3 from the registries; the `keyword` entity type and the `wikibase-keyword` data type (§5) |
> | `scatter-providers` | The `id_grammar` field and its canonical forms; `upstream_prefix` in ID rewriting (§2) |
> | `scatter-normalize` | The keyword normalizer and grammar; the `id_prefix` and display-rule fields of the keyed-type registry (§3, §5) |
> | `scatter-wikibase-resolve` | Rejecting `same-as` involving a keyword (§5) |
> | `scatter-adapter-openalex` | Key-mapping keywords (§5) |
> | `scatter-adapter-musicbrainz` *(new)* | The MusicBrainz adapter (§6). Layer 3; depends on `scatter-wikibase-changeset`, `scatter-wikibase-model` and `scatter-providers` |
> | `triplespace-titles` | Bare-key recognition for suggestions (§1) |
>
> The workspace goes from thirty-five crates to thirty-six.

### A2. DOI is a resolver

- **Date:** 2026-09-27
- **Source:** [0029](0029-resolver-namespaces.md) §1–2
- **Change:** extends §5
- **Summary:** A DOI is a value an entity carries, not a subject: `doi` is a resolver namespace, not a keyed type, and resolver names share the keyed-type namespace. This settled Q6 and falsified the consequence that `doi:` needed only its grammar, normalizer and IRI.

### A3. Keyed types can be case-sensitive

- **Date:** 2026-09-28
- **Source:** [0036](0036-openstreetmap-providers.md) §3
- **Change:** extends §5
- **Summary:** A third keyed type, `osm-tag`, with no normalizer: OSM keys and tags are exact strings. Superseded by A8, which made them notations in the `osm` scheme.

### A4. Two grammars, set per type

- **Date:** 2026-09-28
- **Source:** [0037](0037-gdelt-provider.md) §2
- **Change:** amends §2
- **Summary:** `gdelt-record` and `token` join `digits` and `uuid`; `id_grammar` may be set on a single type, overriding the provider's; the `digits` grammar keeps leading zeros where upstream has them, since a CAMEO code is `0311`. The `digits` row had been edited in place (commit `7a038f5`).

Replaced text (§2):

> | `digits` | One or more decimal digits, no leading zero | As given | Wikidata, Librarybase, OpenAlex (the default) |
>
> A new grammar is added by ADR, as a value the registry accepts.

### A5. MediaInfo IDs

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §7
- **Change:** extends §1
- **Summary:** `M` and digits, a local-form ID, names the statements of the File page with that page ID (WikibaseMediaInfo's contract). It is derived, not minted: never in `view.entity`, never in a cluster, never the value of an entity data type.

### A6. Tenant-relative IDs

- **Date:** 2026-09-30
- **Source:** [0044](0044-tenant-relative-ids.md) §1–2
- **Change:** extends §1, §2
- **Summary:** By section:
  - §1: A type letter written three times and the rest of a local ID, `QQQ5`, is an **input form** of the local ID on the tenant that reads it. It is canonicalized to `Q5` wherever IDs are accepted and never stored or returned, except as Lua output under a remapped letter ([0043](0043-lua-modules.md) §8).
  - §2: The provider codes `AA` through `ZZ` are reserved and never allocated, so that the tenant-relative form is unambiguous.

### A7. Notation

- **Date:** 2026-10-01
- **Source:** [0048](0048-notation.md) §1
- **Change:** extends §3, §5
- **Summary:** The `notation` keyed type, whose key carries a scheme, replaces 0036's `osm-tag`; a keyed type's key may therefore contain a scheme name and a colon before the string.

### A8. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–7
- **Summary:** A1–A7 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A5 and A6 were blockquotes, A4 had been edited in place, and the other entries were recorded only in other ADRs. The file before conversion is commit `0b26a3a`.
