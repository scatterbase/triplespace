# 0029. Resolver namespaces

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A10)
- **Author:** James Hare / Claude Fable
- **Changes:** [0003](0003-statement-ui.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0017](0017-entity-id-grammar.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0023](0023-moderation.md), [0028](0028-tenancy-policy.md)
- **Chapters:** [06](../architecture/06-statements-and-properties.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md)

## Context

Many things carry an identifier that is unique by convention but is not the thing itself: a DOI names a publication, an ORCID a researcher, an ISBN an edition, a Handle whatever its registrant says. [0009](0009-keyed-entity-types-and-domain.md) and [0017](0017-entity-id-grammar.md) considered making DOI a **keyed entity type**, so that `doi:10.1000/xyz` would be an entity in its own right, and left it open: its grammar and normalizer were unsettled, and it was not clear that a DOI should be a subject rather than a value.

James's proposal is different in kind. A **resolver prefix** is a namespace that *looks up* an entity by a value: `DOI:10.1000/xyz` is not a page about the DOI, it is a way of reaching the item that carries that DOI. The namespace is tied to a property. If exactly one entity has that property with that value, and that entity has no other value for the property, the reader is sent to it. Some resolvers fold case (DOIs are case-insensitive by definition), some do not.

The instance already has every part of this. [0013](0013-postgres-storage.md) §5.2's `view.identifier` indexes every normalized external-identifier value in the resolved view by property, so that "which entity has DOI *x*" is one lookup; [0004](0004-identity-clusters-and-equivalence.md) §7 defines per-property normalizers for `external-id` values, DOI lowercasing among them; [0009](0009-keyed-entity-types-and-domain.md) §1–2 defines what a key grammar and normalizer are; [0008](0008-namespaces-and-document-pages.md) §1 makes a namespace a registry entry with a kind and a normalizer; and [0003](0003-statement-ui.md) §7 binds properties from any provider to roles. What is missing is the namespace kind that ties them together.

## Decision

### 1. A resolver is a namespace bound to a property (extends 0003 §7; settles 0009 Q6 and 0017 Q6)

*Current text: [06](../architecture/06-statements-and-properties.md) §3.1.*

### 2. The `resolver` namespace kind (extends 0008 §1 and §3)

*Changed by A3, A5, A7, A8.*

*Current text: [06](../architecture/06-statements-and-properties.md) §3.2.*

### 3. Resolution

*Changed by A4.*

*Current text: [06](../architecture/06-statements-and-properties.md) §3.3.*

### 4. Where resolvers appear

*Current text: [06](../architecture/06-statements-and-properties.md) §3.4.*

### 5. A resolver bound to sitelinks (extends 0026 §2)

*Current text: [06](../architecture/06-statements-and-properties.md) §3.5.*

### 6. API (extends 0012 §5)

*Changed by A2, A5, A7, A9.*

*Current text: [18](../architecture/18-api.md) §2.3, §3.2, §3.3.*

### 7. UI (extends 0010 §3)

*Current text: [19](../architecture/19-site-ui.md) §2.1, §6.4.*

### 8. Configuration and registry (extends 0015 §3 and §5)

*Current text: [23](../architecture/23-configuration-and-registry.md) §2.2, §4.4, §5.2.*

### 9. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **A DOI is a way to find a thing, not a thing.** Citation data keeps its items, and `DOI:10.1000/xyz` is a stable, human-readable address for "the item with this DOI" that survives merges, redirects and changes of canonical ID.
- **Resolvers and identifier normalization are one mechanism.** Declaring a resolver fixes how its property's values compare everywhere, which removes a class of near-duplicates (case variants of one DOI) from fusion and lookup at once.
- **Nothing is guessed.** Every non-unique case is a page a person can act on, which is also where duplicates surface.
- **Properties from any provider serve**, through roles, so a tenant that mirrors Wikidata gets a working DOI resolver from `WDP356` without a local property.
- **The keyed-type question for DOI closes**, and the pattern for the next identifier is clear: if it names a thing the tenant wants to describe directly, it is a keyed type; if it names a thing that already has an item, it is a resolver.
- **One more namespace kind, no new table, no RDF.**

## Open questions

- **Q1.** ~~**Namespace numbers.** `DOI` and `URL` are allocated; the 220–229 range holds three more pairs, so a sixth resolver needs a second range.~~ *Settled by the numbering policy of 0008 A17: the second resolver range is 320–329.*
- **Q2. Resolvers across tenants.** Whether a resolver at the farm base should search every tenant's identifiers under a `search.farm_wide` policy ([0028](0028-tenancy-policy.md) §9).
- **Q3. Resolving to non-canonical members.** When the entity found is a non-canonical cluster member, the redirect goes to the canonical entity ([0004](0004-identity-clusters-and-equivalence.md) §4); whether the page should say so.
- **Q4. Grammar strictness for DOI.** The suffix may contain almost any character; whether to accept everything after `10.xxxx/` or apply Crossref's practical limits.
- **Q5. Which candidates ship.** `orcid`, `isbn` and `handle` are drafted; `arxiv`, `pmid`, `viaf` and `isni` are obvious next ones.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0003](0003-statement-ui.md) §7 | §1 | extends | 0003 A3 |
| [0005](0005-crate-organization.md) §2 | §9 | extends | 0005 A25 |
| [0008](0008-namespaces-and-document-pages.md) §1, §3 | §2 | extends | 0008 A5 |
| [0009](0009-keyed-entity-types-and-domain.md) Q6 | §1 | settles | 0009 Q6 |
| [0010](0010-site-ui.md) §3 | §7 | extends | 0010 A18 |
| [0012](0012-api-requirements.md) §4, §5 | §6 | extends | 0012 A17 |
| [0013](0013-postgres-storage.md) §5, §5.2, §5.6 | §3 | amends | 0013 A9 |
| [0013](0013-postgres-storage.md) §4, §5.4, §5.5, §7, §8 | §3 | extends | 0013 A9 |
| [0015](0015-record-format-and-partition-registry.md) §3, §5 | §1 | extends | 0015 A12 |
| [0017](0017-entity-id-grammar.md) §5 | §1–2 | extends | 0017 A2 |
| [0017](0017-entity-id-grammar.md) Q6 | §1 | settles | 0017 Q6 |

## References

- [DOI Handbook: DOI names are case-insensitive](https://www.doi.org/doi-handbook/HTML/doi-name-syntax.html)
- [Wikibase: `Special:ItemByTitle`](https://www.wikidata.org/wiki/Special:ItemByTitle), [`Special:GoToLinkedPage`](https://www.wikidata.org/wiki/Special:GoToLinkedPage) and [`Special:ItemDisambiguation`](https://www.wikidata.org/wiki/Special:ItemDisambiguation)
- [Wikidata: formatter URL (P1630)](https://www.wikidata.org/wiki/Property:P1630) and the `wdtn:` normalized-value predicates
- `docs/registry/resolvers.toml`

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §9
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A25).

Replaced text (§9):

> | Crate | Change |
> |---|---|
> | `scatter-normalize` | Resolver grammars and normalizers, with the case rule; the rule that a resolver's normalizer is its bound properties' normalizer (§1). Embeds `docs/registry/resolvers.toml`. Builds for `wasm32`, so the search box can recognise a key |
> | `triplespace-titles` | The `resolver` namespace kind and its normalizer; the lookup of §3 and its decision; `/resolve` ordering (§2, §6) |
> | `triplespace-projections` | Nothing new: `view.identifier` and `view.sitelink` already exist |
> | `triplespace-api-action`, `triplespace-api-rest` | §6 |
>
> No crate is added.

### A2. Main-namespace titles resolve last

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §8
- **Change:** extends §6
- **Summary:** With the main namespace implemented, `/resolve` tries main-namespace titles last, after every form above. A bare ID still reaches its entity first; a main-namespace page with the same title is returned in an `also` field.

### A3. Namespace kinds name models

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §4
- **Change:** amends §2
- **Summary:** The kinds beside `resolver` are now `pages`, `reserved` and `virtual`: entity view, document, composite and thread namespaces are `pages` namespaces told apart by content model. `resolver` stays a kind, because a resolver namespace holds no pages and so has no model.

Replaced text (§2):

> `resolver` joins the namespace kinds of [0008](0008-namespaces-and-document-pages.md) §1 (entity view, document, reserved, virtual, composite, thread). A resolver namespace holds no pages and no records:

### A4. Special pages for the item form and disambiguation

- **Date:** 2026-10-01
- **Source:** [0047](0047-special-pages.md) §5, §9
- **Change:** extends §3
- **Summary:** The item form is `Special:NewItem` with `statement={property}:{key}` filled in, and the disambiguation page uses `Special:ItemDisambiguation`'s layout.

### A5. Notation scheme names

- **Date:** 2026-10-01
- **Source:** [0048](0048-notation.md) §2
- **Change:** extends §2, §6
- **Summary:** Scheme names join the one namespace of keyed-type and resolver names, and `/resolve` tries a notation key after keyed IDs and before resolver strings.

### A6. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–9
- **Summary:** A1–A5 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A2–A4 were blockquotes, and A5 was recorded only in 0048. The file before conversion is commit `0b26a3a`.

### A7. Entity sources

- **Date:** 2026-10-08
- **Source:** [0078](0078-entity-sources.md) §3, §5
- **Change:** extends §2, §6
- **Summary:** Registry provider slugs and, per tenant, the tenant's entity-source names join the one namespace of keyed-type, resolver and notation scheme names; `wikidata:Q42` is an input form of `WDQ42`. A source may not take a registry name, and a registry name added later is shadowed on a tenant whose source has it. An entity source's namespace is a `resolver` namespace with its own lookup. `/resolve` tries source IDs with the other entity IDs.

### A8. Thread talk is `virtual`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §2
- **Summary:** Drop "as Thread talk is": `Thread talk` has been `virtual` since [0049](0049-boards.md) §2, so it is no longer the example of a `reserved` talk namespace. A resolver's paired talk namespace stays `reserved` and empty. (PENDING E14)

Replaced text (§2):

> Its paired talk namespace is `reserved` and empty, as `Thread talk` is ([0019](0019-discussions.md) §3).

### A9. Notation keys resolve; other bare keys are suggestions

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §6
- **Summary:** The `/resolve` order of [0012](0012-api-requirements.md) §5 folds §6's step: a string that normalizes to a notation key ([0048](0048-notation.md) §2) resolves, as the one exception to the bare-key rule; any other bare string that normalizes to a valid key of a resolver is returned as a suggestion, not resolved. The order stays entity IDs (source IDs among them), keyed-type IDs, notation keys, resolver-prefixed strings, main-namespace titles. (PENDING E40)

Replaced text (§6):

> | `GET /resolve?q=` | Now tries, in order: entity IDs, source IDs among them ([0078](0078-entity-sources.md) §3), keyed-type IDs, notation keys ([0048](0048-notation.md) §2), then resolver-prefixed strings (`doi:10.1000/xyz`) and bare strings that normalize to a valid key of exactly one resolver, and last main-namespace titles ([0038](0038-page-metadata-and-categories.md) §8): a bare ID still reaches its entity first, and a main-namespace page with the same title is returned in an `also` field. The response carries `kind: resolver`, the resolver, the normalized key and either the target entity or the candidate list |

### A10. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [06](../architecture/06-statements-and-properties.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
