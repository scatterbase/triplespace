# 0017. Entity ID grammar

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A12)
- **Author:** James Hare / Claude Fable
- **Changes:** [0000](0000-init.md), [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0003](0003-statement-ui.md), [0007](0007-actor-identity.md), [0011](0011-logs.md), [0016](0016-permissions-and-access-control.md)
- **Chapters:** [04](../architecture/04-entities-and-identifiers.md), [22](../architecture/22-crates-and-stack.md)

## Context

[0000](0000-init.md) §3 gives foreign entities a two-letter provider code and a one-letter type code, and [0009](0009-keyed-entity-types-and-domain.md) §3 relies on the rest of every minted ID being digits: "a string that matches the entity ID grammar is an entity ID, and anything else is tried as a keyed type." Two providers in `docs/registry/providers.toml` ([0015](0015-record-format-and-partition-registry.md) §5) break that assumption from opposite sides.

- **MusicBrainz** identifies every entity by an MBID, which is a UUID. `MBA` followed by digits does not exist.
- **OpenAlex keywords** are identified by slugs, `keywords/machine-learning`, and OpenAlex defines a keyword as "a short phrase describing what a work is about". That is a natural key, not a minted identifier: a keyword is the phrase itself, so it belongs with Domain among the keyed types of 0009 §1, not among minted `OA` entities.

Keywords then expose a second problem. 0009 §3 lets a Domain's ID be its bare key because a valid domain name can never look like an entity ID. Keyword keys can: `p53`, `q10`, `cd4` and `h2o` are all ordinary keywords, and `p53` is also the local property P53. Disjointness cannot be proved for every keyed type, and a bare key in a URL or an API parameter does not say what kind of thing it names. A domain name with nothing marking it as a domain name is unclear in the same way, even where it is unambiguous.

This ADR replaces the one grammar of 0009 §3 with three, each declared in the registry, and registers Keyword and MusicBrainz under them.

## Decision

### 1. Three ID forms

*Changed by A5, A6, A9, A10, A12.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §1, §2.1, §2.3, §5.2.*

### 2. ID grammars for minted types

*Changed by A4, A6.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §2.2.*

### 3. Keyed IDs

*Changed by A7.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.1, §3.2, §3.4, §3.5.*

### 4. Where IDs are stored and shown

*Current text: [04](../architecture/04-entities-and-identifiers.md) §6.*

### 5. Keyword: a second keyed type (extends 0009 §1)

*Changed by A2, A3, A7.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.7, §4.1.*

### 6. MusicBrainz

*Current text: [04](../architecture/04-entities-and-identifiers.md) §2.2.*

### 7. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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

### A9. Lexeme part IDs

- **Date:** 2026-10-05
- **Source:** [0066](0066-lexemes.md) §4
- **Change:** extends §1
- **Summary:** A sub-entity suffix on `L` only: `-F` or `-S` and digits, in the local, foreign and input forms. No other letter takes a suffix.

### A10. Entity sources

- **Date:** 2026-10-08
- **Source:** [0078](0078-entity-sources.md) §2–3
- **Change:** amends §1
- **Summary:** A fifth form, the **source** ID, `{name}:{upstream ID}` (`mhc:Q1`): a foreign ID in the colon shape, minted by an entity source a tenant declares, and meaning that source only on that tenant. Colon forms are told apart by the kind of their prefix in the tenant's table of names. Registry provider slugs join that table, and `wikidata:Q42` is an input form of `WDQ42`.

Replaced text (§1):

> Every entity ID has one of three forms, told apart by its first character and by the presence of a colon:

> No minted ID contains a colon, and every keyed ID does, so the two never collide, whatever a key looks like.

### A11. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§7
- **Summary:** The Decision's current text now lives in the architecture chapters [04](../architecture/04-entities-and-identifiers.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A12. Any member of a cluster is accepted wherever an entity ID is accepted

- **Date:** 2026-10-09
- **Source:** [0082](0082-source-form-and-the-shared-view.md) §1, §2
- **Change:** extends §1
- **Summary:** The three ID forms of §1 are what the store holds, in source form: the stored form of an entity keeps the IDs its source wrote, and the UUID of a statement GUID stays its identity while the entity part is a property of the response. At every entry point, API parameters, change sets, titles, the search box, `wbgetentities`, `GET /resolve` and SPARQL constants, any member of a cluster is accepted for the entity and normalized to the internal form before anything executes, as the tenant-relative form and a provider slug already are; a lookup that names a cluster is expanded to the member set, one indexed probe of `view.cluster_member` per constant with L0 in front; property paths are not expanded; search results collapse by `cluster_id`. (REVIEW G3, G4)
