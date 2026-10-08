# 0029. Resolver namespaces

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-08 (A7)
- **Author:** James Hare / Claude Fable
- **Changes:** [0003](0003-statement-ui.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0017](0017-entity-id-grammar.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0023](0023-moderation.md), [0028](0028-tenancy-policy.md)

## Context

Many things carry an identifier that is unique by convention but is not the thing itself: a DOI names a publication, an ORCID a researcher, an ISBN an edition, a Handle whatever its registrant says. [0009](0009-keyed-entity-types-and-domain.md) and [0017](0017-entity-id-grammar.md) considered making DOI a **keyed entity type**, so that `doi:10.1000/xyz` would be an entity in its own right, and left it open: its grammar and normalizer were unsettled, and it was not clear that a DOI should be a subject rather than a value.

James's proposal is different in kind. A **resolver prefix** is a namespace that *looks up* an entity by a value: `DOI:10.1000/xyz` is not a page about the DOI, it is a way of reaching the item that carries that DOI. The namespace is tied to a property. If exactly one entity has that property with that value, and that entity has no other value for the property, the reader is sent to it. Some resolvers fold case (DOIs are case-insensitive by definition), some do not.

The instance already has every part of this. [0013](0013-postgres-storage.md) §5.2's `view.identifier` indexes every normalized external-identifier value in the resolved view by property, so that "which entity has DOI *x*" is one lookup; [0004](0004-identity-clusters-and-equivalence.md) §7 defines per-property normalizers for `external-id` values, DOI lowercasing among them; [0009](0009-keyed-entity-types-and-domain.md) §1–2 defines what a key grammar and normalizer are; [0008](0008-namespaces-and-document-pages.md) §1 makes a namespace a registry entry with a kind and a normalizer; and [0003](0003-statement-ui.md) §7 binds properties from any provider to roles. What is missing is the namespace kind that ties them together.

## Decision

### 1. A resolver is a namespace bound to a property (extends 0003 §7; settles 0009 Q6 and 0017 Q6)

A **resolver** is a registry entry, a `config` record of kind `resolver` in the tenant `config` ([0015](0015-record-format-and-partition-registry.md) §3), with defaults in `docs/registry/resolvers.toml`. It records:

| Field | Meaning | DOI |
|---|---|---|
| **Namespace** | The MediaWiki namespace whose titles are keys, of kind `resolver` (§2) | `DOI` |
| **Binding** | A **role** ([0003](0003-statement-ui.md) §7) naming the property, or `sitelink` (§5) | `doi`, bound on a Wikidata-mirroring tenant to `WDP356` and to any local DOI property |
| **Grammar** | Which strings are valid keys ([0009](0009-keyed-entity-types-and-domain.md) §1) | `10\.\d{4,9}/.+`, after the normalizer |
| **Normalizer** | How input becomes the canonical key, including the **case rule** | Strip a leading `https://doi.org/`, `http://dx.doi.org/` or `doi:`; percent-decode; **casefold** |
| **External IRI** | Where the key resolves outside the instance | `https://doi.org/{key}` |
| **Label** | For the disambiguation page and the identity line | "DOI" |

**The binding is a role, so the property may come from any provider.** A tenant binds the role to the properties it holds, exactly as it binds `time-point` for the statement UI. Where two bound properties are joined by `equivalent-property` ([0004](0004-identity-clusters-and-equivalence.md) §6) they are already one predicate in the resolved view; where they are not, the resolver reads both. The normalizer of a resolver **is** the normalizer of its property's values ([0004](0004-identity-clusters-and-equivalence.md) §7): registering a resolver for a role sets the normalizer for every property bound to it, so that the index and the lookup agree by construction. A property whose values a resolver normalizes with casefolding therefore compares its values case-insensitively everywhere, in statement fusion as in lookup, which is the right answer for a DOI and is why a resolver declares its case rule rather than leaving it to each property.

**DOI is the first resolver, and is not a keyed type.** This settles [0009](0009-keyed-entity-types-and-domain.md) Q6 and [0017](0017-entity-id-grammar.md) Q6: a DOI is a value an entity carries, not a subject to say things about, and a citation record stays an item with a DOI statement. The `doi:` prefix reserved for a keyed type is withdrawn.

### 2. The `resolver` namespace kind (extends 0008 §1 and §3)

*Changed by A3, A5, A7.*

`resolver` joins the namespace kinds of [0008](0008-namespaces-and-document-pages.md) §1, beside `pages`, `reserved` and `virtual` ([0041](0041-content-models.md) §4): entity view, document, composite and thread namespaces are `pages` namespaces told apart by content model, and `resolver` stays a kind of its own because a resolver namespace holds no pages and so has no model. It holds no records either: every title in it is a key, and viewing it performs the lookup of §3. Its paired talk namespace is `reserved` and empty, as `Thread talk` is ([0019](0019-discussions.md) §3). Its **title normalizer** is the resolver's normalizer, and `meta=siteinfo` reports its `case` as `case-sensitive` whatever the case rule, because the server normalizes ([0008](0008-namespaces-and-document-pages.md) §3). Subpages are not allowed; a `/` inside a DOI is part of the key, and the resolver's grammar, not the namespace, decides what a key may contain.

**Resolver names and keyed-type names share one namespace.** `doi` cannot be both a resolver and a keyed type, so `doi:10.1000/xyz` in an API parameter can mean only one thing ([0017](0017-entity-id-grammar.md) §1). The registry refuses a resolver whose name is a keyed type's, and the reverse. Notation scheme names join the same namespace ([0048](0048-notation.md) §2): `osm:amenity=cafe` typed into `/resolve` means the notation, as `doi:10.1000/x` means the DOI resolver. So do registry provider slugs, `wikidata:Q42` being an input form of `WDQ42`, and, on one tenant only, the names of that tenant's entity sources ([0078](0078-entity-sources.md) §3). A source may not take a name the registry has; a registry name added later that a tenant's source already uses is shadowed on that tenant. An entity source's namespace is a `resolver` namespace whose lookup is [0078](0078-entity-sources.md) §5's.

### 3. Resolution

*Changed by A4.*

Viewing `DOI:10.1000/xyz`, or calling `GET /resolve?q=doi:10.1000/xyz` ([0012](0012-api-requirements.md) §5), does the following:

1. **Normalize** the key with the resolver's normalizer and check it against the grammar. An invalid key is a page saying so.
2. **Look up** `(property, key)` in `view.identifier` ([0013](0013-postgres-storage.md) §5.2) for every property bound to the role, in the viewer's tenant's resolved view, so that mirrored values count and deleted or hidden entities ([0023](0023-moderation.md)) do not.
3. **Decide:**

| Result | Response |
|---|---|
| Exactly one entity, holding exactly one value for the property | A **redirect** (HTTP 303) to the entity's page, `Item:Q5`, with the resolver named in the identity line ("Reached via DOI 10.1000/xyz") |
| No entity | A **disambiguation page** saying no entity carries the key, with the external link (`https://doi.org/…`) and, for editors, **Create an item with this DOI**, which opens `Special:NewItem` with `statement={property}:{key}` filled in ([0047](0047-special-pages.md) §5) |
| Several entities | A disambiguation page listing them with their labels and descriptions, in `Special:ItemDisambiguation`'s layout ([0047](0047-special-pages.md) §9), and a note that this may be a duplicate to merge ([0004](0004-identity-clusters-and-equivalence.md) §2) |
| One entity with several values for the property | A disambiguation page showing the entity and each of its values, since the key does not name the entity unambiguously by James's rule |

The lookup never guesses: any case but the first shows a page. It is also cheap: one primary-key range read per bound property.

**Match keys.** A resolver's property is a natural candidate for a uniqueness constraint ([0002](0002-source-graphs-and-mass-ingest.md) §8.5, [0013](0013-postgres-storage.md) §5.2 `match_key`), and a tenant that adds one will see the "several entities" page only for values that predate the constraint. The two are kept separate: a match key refuses a duplicate at write; a resolver tolerates it at read and shows it.

### 4. Where resolvers appear

- **Titles and links.** `[[DOI:10.1000/xyz]]` in a document page or a post ([0008](0008-namespaces-and-document-pages.md) §8, [0019](0019-discussions.md) §5) links to the resolver page and renders, where the lookup is unique, with the target entity's label. A `page_link` row records the link with the resolver title as its target, since the target entity may change as data changes.
- **Search** ([0010](0010-site-ui.md) §3). Input that normalizes to a valid key of some resolver gets a "Go to" suggestion beside the keyed-type and ID suggestions the title resolver already offers ([0017](0017-entity-id-grammar.md) §1), labelled with the resolver: "Go to the item with DOI 10.1000/xyz".
- **The entity page.** A statement whose property is bound to a resolver shows the external link ([0003](0003-statement-ui.md) §9's value cell already does this for `external-id` formatter URLs) and a **permalink** in the resolver form, `DOI:10.1000/xyz`, which is the citable address for "the item with this DOI, whatever its ID".
- **RDF.** Nothing. A resolver page has no document node and emits no triples; the DOI is already in the main graph as the value of its property, with Wikibase's normalized form (`wdtn:`) carrying the external IRI ([wikibase-compat.md §5.5](../api/wikibase-compat.md)).

### 5. A resolver bound to sitelinks (extends 0026 §2)

A resolver's binding may be `sitelink` instead of a role. Its key is then a URL, normalized as [0026](0026-sitelinks.md) §1 normalizes sitelinks, and the lookup reads `view.sitelink` by `url_key`, which is unique by construction (0026 §2), so the "several entities" case cannot arise. `URL:https://en.wikipedia.org/wiki/Douglas_Adams` reaches the item linked to that page. This is what `Special:ItemByTitle` and `Special:GoToLinkedPage` do on Wikibase, and both are served as aliases of it. It ships as a default resolver in `resolvers.toml`.

### 6. API (extends 0012 §5)

*Changed by A2, A5, A7.*

| Route or module | Behaviour |
|---|---|
| `GET /resolve?q=` | Now tries, in order: entity IDs, source IDs among them ([0078](0078-entity-sources.md) §3), keyed-type IDs, notation keys ([0048](0048-notation.md) §2), then resolver-prefixed strings (`doi:10.1000/xyz`) and bare strings that normalize to a valid key of exactly one resolver, and last main-namespace titles ([0038](0038-page-metadata-and-categories.md) §8): a bare ID still reaches its entity first, and a main-namespace page with the same title is returned in an `also` field. The response carries `kind: resolver`, the resolver, the normalized key and either the target entity or the candidate list |
| `GET /resolvers` | The tenant's resolvers with their grammars and bindings (public) |
| `GET /resolvers/{name}/{key}` | The lookup of §3 as JSON, with `303` semantics available by `Accept` |
| `wbgetentities` | A new `resolver` and `key` pair of parameters, mirroring `sites`+`titles`, returning the entity when the lookup is unique and `missing` with a `candidates` list otherwise |
| `Special:ItemByTitle`, `Special:GoToLinkedPage` | Served by the sitelink resolver (§5) |

### 7. UI (extends 0010 §3)

The disambiguation page of §3 uses the site frame ([0010](0010-site-ui.md) §2) with the resolver's label and the key in the identity line, the external link, the candidate list, and the create action. A resolver namespace appears in the namespace selector wherever namespaces are listed, and its pages are excluded from `list=allpages`, since it has none.

### 8. Configuration and registry (extends 0015 §3 and §5)

`docs/registry/resolvers.toml` ships `doi` and `url` as defaults, with `orcid`, `isbn` and `handle` as commented candidates whose grammars are written but whose roles a tenant binds when it wants them. Adding a resolver is a `config` record with `ts-config`; binding its role is the ordinary role record ([0015](0015-record-format-and-partition-registry.md) §3, kind `role`). `DOI` is namespace 220 (talk 221) and `URL` 222 (talk 223), in the resolver range 220–229 of [0008](0008-namespaces-and-document-pages.md) §2; three pairs remain in it, and the range continues at 320–329 (0008 A17).

### 9. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed. The table this section first gave is in A1.

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
