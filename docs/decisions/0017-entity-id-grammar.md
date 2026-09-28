# 0017. Entity ID grammar

- **Status:** Proposed
- **Date:** 2026-09-27
- **Author:** James Hare / Claude Fable
- **Amended by:** [0029 — Resolver namespaces](0029-resolver-namespaces.md) (§1 settles the DOI open question; §2 makes resolver names and keyed-type names one namespace)
- **Related:** [0000 — Initial proposition](0000-init.md) (§3), [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (§2 amends the ID rewriting of §4), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md) (§5 amends §1: keywords never cluster), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (amends §2), [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md) (§4 amends the `entity-id` normalizer of §3), [0009 — Keyed entity types and Domain](0009-keyed-entity-types-and-domain.md) (amends §3, §4, §5, §8, §9 and §10; §5 extends §1 with a second keyed type), [0010 — Site UI](0010-site-ui.md) (§4 amends §3), [0012 — API requirements for the site UI](0012-api-requirements.md) (§4 refines `/resolve` in §5), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§4 amends `view.entity.id` in §5.1), [0014 — Cache layers and search](0014-caches-and-search.md) (§4 amends the `id` and `key` fields of §7), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (§6 extends the registry files of §5), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§4)

## Context

[0000](0000-init.md) §3 gives foreign entities a two-letter provider code and a one-letter type code, and [0009](0009-keyed-entity-types-and-domain.md) §3 relies on the rest of every minted ID being digits: "a string that matches the entity ID grammar is an entity ID, and anything else is tried as a keyed type." Two providers in `docs/registry/providers.toml` ([0015](0015-record-format-and-partition-registry.md) §5) break that assumption from opposite sides.

- **MusicBrainz** identifies every entity by an MBID, which is a UUID. `MBA` followed by digits does not exist.
- **OpenAlex keywords** are identified by slugs, `keywords/machine-learning`, and OpenAlex defines a keyword as "a short phrase describing what a work is about". That is a natural key, not a minted identifier: a keyword is the phrase itself, so it belongs with Domain among the keyed types of 0009 §1, not among minted `OA` entities.

Keywords then expose a second problem. 0009 §3 lets a Domain's ID be its bare key because a valid domain name can never look like an entity ID. Keyword keys can: `p53`, `q10`, `cd4` and `h2o` are all ordinary keywords, and `p53` is also the local property P53. Disjointness cannot be proved for every keyed type, and a bare key in a URL or an API parameter does not say what kind of thing it names. A domain name with nothing marking it as a domain name is unclear in the same way, even where it is unambiguous.

This ADR replaces the one grammar of 0009 §3 with three, each declared in the registry, and registers Keyword and MusicBrainz under them.

## Decision

### 1. Three ID forms

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

### 2. ID grammars for minted types

Each foreign entity type declares an **ID grammar** in `providers.toml`. It says what may follow the three-letter prefix and what the canonical spelling is.

| Grammar | Rest of the ID | Canonical form | Used by |
|---|---|---|---|
| `digits` | One or more decimal digits, no leading zero | As given | Wikidata, Librarybase, OpenAlex (the default) |
| `uuid` | An RFC 4122 UUID in its 36-character hyphenated text form | Lowercase | MusicBrainz |

Input is case-insensitive in every position. **Canonicalizing an ID uppercases its prefix and applies the grammar's canonical form to the rest**, so `mbab10BBBFC-…` becomes `MBAb10bbbfc-…`. This amends the `entity-id` normalizer of [0008](0008-namespaces-and-document-pages.md) §3, which uppercased the whole ID, and the ID rewriting of [0002](0002-source-graphs-and-mass-ingest.md) §4, which becomes: strip or add the prefix, and canonicalize the rest by the type's grammar. The rewriting of upstream IDs also honours `upstream_prefix`, so `keywords/…` and MusicBrainz's bare MBIDs both round-trip.

A new grammar is added by ADR, as a value the registry accepts. Nothing else in the system needs to know a grammar's rules: the ID parser in `scatter-wikibase-model` ([0005](0005-crate-organization.md) §2) reads them from the registry passed in as data (0005 §3, rule 3).

### 3. Keyed IDs

The keyed-type registry ([0009](0009-keyed-entity-types-and-domain.md) §1, `keyed-types.toml`) gains one field, the **ID prefix**, which is the type's name followed by a colon. The canonical ID is the prefix, in lowercase, and the key in the form the type's normalizer produces. Everything 0009 §7 says about log header keys stands: the log never sees the key, only the surrogate.

Where an earlier ADR shows a bare Domain key as an ID, it now reads with the prefix. That is [0009](0009-keyed-entity-types-and-domain.md) §3 (the JSON `id` and the value form), §4 (`wbgetentities`), §9 (mapped upstream items are written under `domain:{key}` as their subject) and the wire format of §10, which becomes:

```
{"op":"add","id":"domain:en.wikipedia.org","claims":{"P12":[…]}}
{"op":"same-as","ids":["Q77","domain:en.wikipedia.org"]}
```

The concept IRI is unchanged, `https://scatter.red/domain/{key}`: the prefix is part of the ID, not of the key.

### 4. Where IDs are stored and shown

The ID form of §1 is the one that appears everywhere an entity ID does: `view.entity.id` and every column that references it ([0013](0013-postgres-storage.md) §5), the `id` field of a search document ([0014](0014-caches-and-search.md) §7, whose `key` field keeps the bare key), the `target_id` of an ACL ([0016](0016-permissions-and-access-control.md) §4), the ID chip in the UI ([0010](0010-site-ui.md) §2), and canonical JSON. The UI's identity line names the kind of thing in words, "Domain" or "Keyword", and the chip shows the ID as it is.

### 5. Keyword: a second keyed type (extends 0009 §1)

A **keyword** is a short phrase describing what a work is about. It is the phrase, not the thing the phrase denotes.

| | |
|---|---|
| Type name, ID prefix | `keyword`, `keyword:` |
| Namespace | `Keyword`, 212, with `Keyword talk` at 213 ([0008](0008-namespaces-and-document-pages.md) §2, as amended 2026-09-27) |
| Data type | `wikibase-keyword`, with value type `wikibase-entityid` as `wikibase-domain` has |
| IRI | `https://scatter.red/keyword/{key}`, content-derived and so shared by every instance ([0005](0005-crate-organization.md) §5, [0009](0009-keyed-entity-types-and-domain.md) §6) |
| Grammar | 1 to 200 characters of letters, digits and single hyphens; at least one letter; no leading or trailing hyphen |
| Normalizer | Unicode NFKC; casefold; every run of whitespace or punctuation becomes one hyphen; leading and trailing hyphens are removed |

**Labels are editable, not derived.** A Domain's label is derived from its key ([0009](0009-keyed-entity-types-and-domain.md) §5) because the key is the name. A keyword's key loses case and punctuation (`dna`, `type-1-diabetes`), so its labels and descriptions are stored and reconciled like an item's ([0002](0002-source-graphs-and-mass-ingest.md) §3). When no label exists in the requested language, the UI shows the key with hyphens as spaces. The keyed-type registry therefore distinguishes a **derived** display rule from an **editable** one.

**Keywords never join identity clusters (amends 0004 §1 and 0009 §8).** A Wikidata item about machine learning is the concept; `keyword:machine-learning` is a phrase. Treating them as one thing would make the phrase canonical over the concept, since keyed members rank first. So `same-as` with a keyword as either side is rejected at write time, as a data-type mismatch between properties is (0004 §6), and no tier-2 or tier-3 link may produce one. A keyword may carry an ordinary statement pointing at the concept it usually denotes, through a role in the role map ([0003](0003-statement-ui.md) §7); that is data, not identity.

**OpenAlex keywords are mapped by key** ([0009](0009-keyed-entity-types-and-domain.md) §9), as internetdomains.wiki's items are mapped to Domains. The adapter turns `https://openalex.org/keywords/{slug}` into the key by running the slug through the normalizer, writes the entity's `display_name` as the mirror graph's English label, emits `<https://openalex.org/keywords/{slug}> owl:sameAs <https://scatter.red/keyword/{key}>`, and turns a work's `keywords` list into `wikibase-keyword` values with the score as a qualifier by role. The `OAK` type code stays reserved in the registry and no `OAK` entity is ever minted.

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

| Crate | Change |
|---|---|
| `scatter-wikibase-model` | The ID parser reads the three forms of §1, the grammars of §2 and the prefixes of §3 from the registries; the `keyword` entity type and the `wikibase-keyword` data type (§5) |
| `scatter-providers` | The `id_grammar` field and its canonical forms; `upstream_prefix` in ID rewriting (§2) |
| `scatter-normalize` | The keyword normalizer and grammar; the `id_prefix` and display-rule fields of the keyed-type registry (§3, §5) |
| `scatter-wikibase-resolve` | Rejecting `same-as` involving a keyword (§5) |
| `scatter-adapter-openalex` | Key-mapping keywords (§5) |
| `scatter-adapter-musicbrainz` *(new)* | The MusicBrainz adapter (§6). Layer 3; depends on `scatter-wikibase-changeset`, `scatter-wikibase-model` and `scatter-providers` |
| `triplespace-titles` | Bare-key recognition for suggestions (§1) |

The workspace goes from thirty-five crates to thirty-six.

## Consequences

- **One rule for keyed IDs.** Every keyed ID says what kind of key it is. Domain IDs grow by seven characters, and every example in 0009 reads with the prefix.
- **The ID parser is table-driven.** A provider with an unusual ID scheme is a registry entry and, at most, a new grammar value, not a change to the parser.
- **MusicBrainz IDs are long.** Thirty-nine characters in URLs, chips and statement IDs. That is the cost of keeping the MBID legible.
- **Keywords are phrases.** They carry no identity claim about the world, so they stay out of clusters, and a keyword page can say which concept it usually denotes without being that concept.
- **DOI is now easier.** The obstacle 0009 listed for a DOI keyed type, that its keys could not be shown disjoint from entity IDs, is gone; `doi:` needs only its grammar, normalizer and IRI settled.
- **Stock Wikibase clients still cannot handle keyed entities**, as 0009 already noted; the colon form at least fails in a way federation-aware clients recognise.
- **One more crate.** `scatter-adapter-musicbrainz` joins the adapters in the build order's last step ([0005](0005-crate-organization.md) §7).

## Open questions

- **Keyword keys in scripts without word boundaries.** The normalizer produces one hyphen per run of whitespace or punctuation; a CJK phrase becomes its own key unchanged. Whether that is the right key, or whether keys should be transliterated or segmented, needs a look at OpenAlex's non-English keywords.
- **Keyword score.** Which role carries OpenAlex's per-work keyword score, and whether it should be a qualifier or dropped.
- ~~**Namespace numbers** for `Keyword` and `Keyword talk`, to be registered with Domain's.~~ *Settled: 212/213 ([0008](0008-namespaces-and-document-pages.md) §2, as amended).*
- **MusicBrainz edit history.** Whether to mirror MusicBrainz edits as log events ([0011](0011-logs.md)), which would give its issuer numeric editors.
- **A `slug` grammar.** No registered minted type needs one now that keywords are keyed; whether to define it in advance for a future provider.
- ~~**DOI.** Its grammar, normalizer and whether its IRI is `https://doi.org/{key}` or `https://scatter.red/doi/{key}` ([0009](0009-keyed-entity-types-and-domain.md), open questions).~~ *Settled by [0029](0029-resolver-namespaces.md) §1: a resolver, not a keyed type; the `doi:` prefix is withdrawn, and resolver names share the keyed-type namespace.*

## References

- [Wikibase entity IDs and federation](https://doc.wikimedia.org/Wikibase/master/php/docs_topics_entitysources.html) (prefixed IDs for entities from another repository)
- [OpenAlex keywords](https://help.openalex.org/hc/en-us/articles/24736201130391-Keywords) ("a short phrase describing what a work is about")
- [MusicBrainz identifiers](https://musicbrainz.org/doc/MusicBrainz_Identifier) and [entities](https://musicbrainz.org/doc/MusicBrainz_Entity)
- [RFC 4122](https://www.rfc-editor.org/rfc/rfc4122) (UUID text form)
- `docs/registry/providers.toml`, `keyed-types.toml`, `namespaces.toml`
