# 0078. Entity sources: foreign graphs a tenant declares

- **Status:** Proposed
- **Date:** 2026-10-08
- **Updated:** 2026-10-09 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0015](0015-record-format-and-partition-registry.md), [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0029](0029-resolver-namespaces.md), [0035](0035-adopting-a-wikibase.md), [0070](0070-shallow-entity-mirroring.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0007](0007-actor-identity.md), [0009](0009-keyed-entity-types-and-domain.md), [0022](0022-federation.md), [0024](0024-subsidiary-accounts.md), [0026](0026-sitelinks.md), [0043](0043-lua-modules.md), [0044](0044-tenant-relative-ids.md), [0048](0048-notation.md), [0052](0052-page-repositories-and-title-inheritance.md), [0056](0056-security-model.md), [0065](0065-mediainfo-captions-and-commons.md), [0071](0071-derived-statements-from-mirrored-pages.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)
- **Chapters:** [02](../architecture/02-graphs-rdf-and-query.md), [05](../architecture/05-providers-and-ingest.md), [10](../architecture/10-pages-and-content-models.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

## Context

A foreign graph reaches a tenant today only through the provider registry ([0015](0015-record-format-and-partition-registry.md) §5): a two-letter code, a slug and a provider number in `providers.toml`, allocated by a commit to the Triplespace repository and carried by every instance. That is right for Wikidata, OpenAlex or MusicBrainz, which many tenants read. It is wrong for a graph that one tenant needs. Librarybase including Miraheze Communities' Wikibase (`https://communities.miraheze.org/entity/Q1`) would need a registry commit, a code from a global space, and a public record that Librarybase uses it, before the first value could be written.

Wikibase already has the concept. A federated install declares **entity sources**, each with a concept base URI and an interwiki prefix, and writes their IDs as `wikidata:Q42`. [0017](0017-entity-id-grammar.md) §1 chose the colon for keyed IDs partly because it is this form. [0035](0035-adopting-a-wikibase.md) §3 rewrites an adopted wiki's entity-source prefixes to provider form, `WDQ42`, but only where the source is a registry provider; a prefix naming any other source has no rule.

The first design discussed, on 2026-10-07, reserved a block of provider codes, `YA` through `YZ`, whose meaning each tenant would set. It is under Alternatives. James then proposed the design this ADR records, and asked for a check against the ADRs. The check found that 0008 defines no range for a tenant's own namespaces, which James had assumed; §5 defines one.

### Direction

James's direction, from the design discussion of 2026-10-07 and 2026-10-08:

- **"There will be times when a tenant wants to incorporate data from a foreign graph, but the foreign graph in question isn't of broad enough usefulness to become part of the global graph namespace."**
- **"Tenants can have a very large number of custom wiki namespaces. Say, for example, Librarybase wanted to include Miraheze Communities as part of its graph, and I did not want to add Miraheze Communities to the registry. I can allocate a custom namespace "MHC" that resolves to the entity, for example MHC:Q1 for <https://communities.miraheze.org/entity/Q1>. If at some point an identifier needs to be assigned to it so that it can be fully incorporated into the local graph, a local entity ID can be minted, and the "keyed" identifier becomes more like a resolver."**
- **"The authority of what [the prefix] means comes from that tenant, but outside of the immediate context of that tenant, it should be converted into the real meaning of the triple anyway."** The mechanism is the one that turns `WDQ42` into `http://www.wikidata.org/entity/Q42`.
- **"It's good to have deterministic behavior around this"**, of possible clashes between `MHC:Q1`, `Item:MHQ1` and `Item:MHC1`.
- **The namespace comes from the tenant's own range, not the Triplespace blocks.**
- Agreed in the same discussion: one name moves through three states (a pointer, a mirror, then a member of a local item's cluster) without a third mechanism; a tenant's name wins inside the tenant; identity across tenants is the IRI; a name, once it has data, is never pointed at another graph.

## Decision

### 1. An entity source is a provider a tenant declares (extends 0015 §3)

*Changed by A2.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.1.*

### 2. Source IDs (amends 0017 §1)

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.2.*

### 3. One table of names per tenant (extends 0029 §2 and §6)

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.3.*

### 4. Numbers, the mirror partition and fetching (extends 0015 §2 and §5, 0018 §10, 0070 §2.1, §3 and §10)

*Changed by A2.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.4.*

### 5. The source's namespace (extends 0008 §2 and §3)

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.5; [10](../architecture/10-pages-and-content-models.md) §1.4.*

### 6. Minting a local item (uses 0004 §3 and §4)

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.6.*

### 7. IRIs and RDF (uses 0015 §6)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §2.3, §5.6.*

### 8. Across tenants and instances (extends 0018 §5)

*Changed by A2.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.7.*

### 9. Binding is permanent; promotion

*Changed by A2.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.8.*

### 10. API and operations (extends 0012 §5)

*Changed by A2.*

*Current text: [18](../architecture/18-api.md) §2.2, §2.3, §3.2, §3.3.*

### 11. Adopted wikis' unregistered sources (extends 0035 §3)

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.3.*

### 12. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **A reserved block of provider codes whose meaning each tenant sets** (the first design, 2026-10-07). It gives 25 codes, not 26, since `YY` is reserved for the tenant-relative form ([0044](0044-tenant-relative-ids.md) §2). It also gives `YAQ5`, a string in the provider form whose meaning is otherwise global, a meaning per tenant that nothing in the string shows. The colon form is Wikibase's own form for this, has no cap, and keeps the source's ID verbatim.
- **Registering every graph.** A commit and a global code per graph, a public record of what each tenant reads, and every instance carrying the entry. Promotion (§9) is still the path for a graph that turns out to be widely used.
- **A keyed type per source.** Wrong by [0009](0009-keyed-entity-types-and-domain.md) §1's test: the source, not the thing, minted the ID. Surrogates and content-derived IRIs would solve problems a source does not have.
- **The source's namespace hosting its entities as pages**, `MHC:Q1` holding an item and `MHC:P31` a property. That would break [0008](0008-namespaces-and-document-pages.md) §1, where the model names the entity type and entity namespaces are keyed by type.
- **A tenant-qualified form across tenants**, `librarybase/mhc:Q1`. The IRI already carries the meaning, and a form that names a tenant would tie the data to where it was first written.

## Consequences

- **A tenant can read any Wikibase without a registry change,** naming its source and its namespace itself.
- **Two tenants' `mhc:Q1` may be different things.** Meaning crosses a tenant only as an IRI, and a reader that has not declared the same source does not see statements that use it. The withheld count shows what declaring one would add.
- **Clashes with the registry are possible only for names the registry adds later,** and then only shadow on the tenant that has the source.
- **Promotion keeps every record.** It costs a rewrite on read for that tenant's old records, as a provider tenant's IDs already cost every reader.
- **The registry's provider numbers halve**, to 4,194,303, which is far beyond the registry's use.
- **0008 gains a tenant range**, which the namespaces tenants already create needed and did not have.
- **Test plan.** Declaring `mhc` with a Wikibase API; a local statement with value `mhc:Q1` enqueues `Q1`, whose `put` lands in `source/mhc` with a ranged revision ID; `MHC:Q1` redirects to `Item:mhc:Q1`, and after Create an item for this to `Item:Q500`; with mirror `off`, `MHC:Q1` shows the external link and fetches nothing; a declaration named `doi`, or with Wikidata's IRI template, is refused; a namespace numbered 120 or 2000 is refused; `wikidata:Q42` in `wbgetentities` returns `WDQ42`; another tenant reading Librarybase without the source withholds the statement and counts it, and with a source `mhcom` for the same IRIs reads `mhcom:Q1`; promotion to a test provider rewrites on read and leaves the log unchanged.

## Open questions

- **Q1. Upstream history.** Whether a source may mirror its edit history, which needs an issuer per source ([0007](0007-actor-identity.md) §1 already says another Wikibase or Miraheze wiki gets one, keyed by its wiki ID) and `log/{name}` and `actors/{name}` partitions of tenant scope.
- **Q2. Verified sources.** A Triplespace instance as a source with `trust = verified` ([0022](0022-federation.md) §2) needs its key chain in the tenant's configuration.
- **Q3. Collisions by construction.** Whether to reserve a syntactic space for tenant names, so that rule 2 of §3 can never apply.
- **Q4.** ~~**Page repositories.** Whether a page repository's `provider` ([0052](0052-page-repositories-and-title-inheritance.md) §1) may name an entity source, so a tenant can serve an unregistered wiki's pages without a registry entry.~~ *Settled by [0080](0080-tenants-as-entity-sources.md) §6, for tenants: a `tenant` repository names the reader's tenant source; the rest is Q8.*
- **Q5. Lua.** Whether [0043](0043-lua-modules.md) §8's letter remapping may map a letter to a source.
- **Q6. Withholding granularity.** §8 withholds a whole statement when one qualifier or reference value is unbound. Whether to drop only that qualifier or reference instead.
- **Q7. Sources that are not Wikibases.** Adapters for a source with a SPARQL endpoint or a dump but no Wikibase API.
- **Q8.** (Rest of Q4.) Whether a page repository may name an entity source that is not a Triplespace tenant.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §12 | extends | 0005 A80 |
| [0008](0008-namespaces-and-document-pages.md) §2, §3 | §5 | extends | 0008 A31 |
| [0012](0012-api-requirements.md) §5 | §10 | extends | 0012 A56 |
| [0015](0015-record-format-and-partition-registry.md) §2, §3, §5 | §1, §4 | extends | 0015 A37 |
| [0017](0017-entity-id-grammar.md) §1 | §2, §3 | amends | 0017 A10 |
| [0018](0018-tenants.md) §5, §10 | §4, §8 | extends | 0018 A12 |
| [0029](0029-resolver-namespaces.md) §2, §6 | §3 | extends | 0029 A7 |
| [0035](0035-adopting-a-wikibase.md) §3 | §11 | extends | 0035 A4 |
| [0070](0070-shallow-entity-mirroring.md) §2.1, §3, §10 | §4 | extends | 0070 A1 |
| [0070](0070-shallow-entity-mirroring.md) Q3 | §4 | settles | 0070 Q3 |

## References

- [Wikibase: Entity sources](https://doc.wikimedia.org/Wikibase/master/php/docs_topics_entitysources.html): concept base URIs, interwiki prefixes and federated IDs such as `wikidata:Q42`
- [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces): "The namespaces in 100-199 are reserved for site-specific namespaces"; "Namespaces using numbers from 3000 and higher are meant to be used by system administrators" (read 2026-10-08)
- [Manual:Using custom namespaces](https://www.mediawiki.org/wiki/Manual:Using_custom_namespaces): numbering conventions for wiki-local namespaces
- [Miraheze Communities](https://communities.miraheze.org/): the example source
- `claude/familysearch-semantic-layer-plan.md` (Triplespace project notes, 2026-10-07): the shallow mirroring this ADR reuses

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§12
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [05](../architecture/05-providers-and-ingest.md), [10](../architecture/10-pages-and-content-models.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A2. Tenant sources

- **Date:** 2026-10-09
- **Source:** [0080](0080-tenants-as-entity-sources.md) §1, §2, §3, §4, §8
- **Change:** extends §1, §4, §10; amends §8, §9
- **Summary:** An `entity-source` record may name a Triplespace tenant by its issuer code (`tenant`) with its current `base`; its types, licence and endpoints are read from the provider (§1). On the same instance a tenant source has no partition and no fetch job: the provider's `local` partition is read directly (§4). Across tenants, a reader's source matches by `tenant` code before IRI template, and a source naming a promoted tenant becomes the provider's ID (§8). A tenant source is bound by its code rather than its templates, may not name a tenant that the registry has, and is promoted when a registry entry's `issuer` is its code (§9). `GET /entity-sources` gives `tenant` and `base` (§10).

Replaced text (§8, in [05](../architecture/05-providers-and-ingest.md) §6.7):

> 1. **A's source is unpublished.** The reference is not readable, as a tenant without a code cannot be referenced ([0018](0018-tenants.md) §5).
> 2. **The IRI is a registry provider's.** It becomes that provider's ID, `MHQ1`, once the registry has Miraheze Communities as `MH`.
> 3. **B has a source with the same IRI template.** It becomes B's ID for the entity, `mhcom:Q1`, whatever B named its source.

Replaced text (§9, in [05](../architecture/05-providers-and-ingest.md) §6.8):

> **A source may not duplicate a registry provider.** A declaration with a type IRI template equal to a registry provider's is refused with `ts-source-is-provider`, …
>
> **Promotion.** When the registry later allocates a provider whose IRI templates equal a source's, …
