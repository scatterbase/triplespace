# 0078. Entity sources: foreign graphs a tenant declares

- **Status:** Proposed
- **Date:** 2026-10-08
- **Updated:** 2026-11-23 (A10)
- **Author:** James Hare / Claude Opus
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0007](0007-actor-identity.md), [0009](0009-keyed-entity-types-and-domain.md), [0022](0022-federation.md), [0024](0024-subsidiary-accounts.md), [0026](0026-sitelinks.md), [0043](0043-lua-modules.md), [0044](0044-tenant-relative-ids.md), [0048](0048-notation.md), [0052](0052-page-repositories-and-title-inheritance.md), [0056](0056-security-model.md), [0065](0065-mediainfo-captions-and-commons.md), [0071](0071-derived-statements-from-mirrored-pages.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

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

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.1.*

### 2. Source IDs (amends 0017 §1)

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.2.*

### 3. One table of names per tenant (extends 0029 §2 and §6)

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.3.*

### 4. Numbers, the mirror partition and fetching (extends 0015 §2 and §5, 0018 §10, 0070 §2.1, §3 and §10)

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.5, §6.4.*

### 5. The source's namespace (extends 0008 §2 and §3)

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.5; [10](../architecture/10-pages-and-content-models.md) §1.4.*

### 6. Minting a local item (uses 0004 §3 and §4)

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.6.*

### 7. IRIs and RDF (uses 0015 §6)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §2.3, §5.6.*

### 8. Across tenants and instances (extends 0018 §5)

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.7.*

### 9. Binding is permanent; promotion

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.8.*

### 10. API and operations (extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §2.2, §2.3, §3.2, §3.3.*

### 11. Adopted wikis' unregistered sources (extends 0035 §3)

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.3.*

### 12. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

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
- ~~**The registry's provider numbers halve**, to 4,194,303, which is far beyond the registry's use.~~ *Registry numbers are below 2^12 (4,095 of them) and entity-source numbers 2^12 to 2^13 − 1, so that every client-visible ID stays below 2^53 (A4).*
- **0008 gains a tenant range**, which the namespaces tenants already create needed and did not have.
- **Test plan.** Declaring `mhc` with a Wikibase API; a local statement with value `mhc:Q1` enqueues `Q1`, whose `put` lands in `source/mhc` with a ranged revision ID; `MHC:Q1` redirects to `Item:mhc:Q1`, and after Create an item for this to `Item:Q500`; with mirror `off`, `MHC:Q1` shows the external link and fetches nothing; a declaration named `doi`, or with Wikidata's IRI template, is refused; a namespace numbered 120 or 2000 is refused; `wikidata:Q42` in `wbgetentities` returns `WDQ42`; another tenant reading Librarybase without the source withholds the statement and counts it, and with a source `mhcom` for the same IRIs reads `mhcom:Q1`; promotion to a test provider rewrites on read and leaves the log unchanged.

## Open questions

- **Q1. Upstream history.** Whether a source may mirror its edit history, which needs an issuer per source ([0007](0007-actor-identity.md) §1 already says another Wikibase or Miraheze wiki gets one, keyed by its wiki ID) and `log/{name}` and `actors/{name}` partitions of tenant scope.
- **Q2. Verified sources.** A Triplespace instance as a source with `trust = verified` ([0022](0022-federation.md) §2) needs its key chain in the tenant's configuration.
- **Q3. Collisions by construction.** Whether to reserve a syntactic space for tenant names, so that rule 2 of §3 can never apply.
- **Q4. Page repositories.** *Settled by [0080](0080-tenants-as-entity-sources.md) §6, for tenants.*
- **Q5. Lua.** Whether [0043](0043-lua-modules.md) §8's letter remapping may map a letter to a source.
- **Q6. Withholding granularity.** §8 withholds a whole statement when one qualifier or reference value is unbound. Whether to drop only that qualifier or reference instead.
- **Q7. Sources that are not Wikibases.** Adapters for a source with a SPARQL endpoint or a dump but no Wikibase API.
- **Q8.** (Rest of Q4.) Whether a page repository may name an entity source that is not a Triplespace tenant.

## References

- [Wikibase: Entity sources](https://doc.wikimedia.org/Wikibase/master/php/docs_topics_entitysources.html): concept base URIs, interwiki prefixes and federated IDs such as `wikidata:Q42`
- [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces): "The namespaces in 100-199 are reserved for site-specific namespaces"; "Namespaces using numbers from 3000 and higher are meant to be used by system administrators" (read 2026-10-08)
- [Manual:Using custom namespaces](https://www.mediawiki.org/wiki/Manual:Using_custom_namespaces): numbering conventions for wiki-local namespaces
- [Miraheze Communities](https://communities.miraheze.org/): the example source
- `claude/familysearch-semantic-layer-plan.md` (Triplespace project notes, 2026-10-07): the shallow mirroring this ADR reuses

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** 0050 §14
- **Change:** relocates §1–§12

### A2. Tenant sources

- **Date:** 2026-10-09
- **Source:** 0080 §1, §2, §3, §4, §8
- **Change:** extends §1, §4, §10; amends §8, §9
- **Summary:** An `entity-source` record may name a Triplespace tenant by its issuer code (`tenant`) with its current `base`; its types, licence and endpoints are read from the provider (§1). On the same instance a tenant source has no partition and no fetch job: the provider's `local` partition is read directly (§4). Across tenants, a reader's source matches by `tenant` code before IRI template, and a source naming a promoted tenant becomes the provider's ID (§8). A tenant source is bound by its code rather than its templates, may not name a tenant that the registry has, and is promoted when a registry entry's `issuer` is its code (§9). `GET /entity-sources` gives `tenant` and `base` (§10).

Replaced text: §8, in 05 §6.7; §9, in 05 §6.8.

### A3. The cross-tenant rewrite is the response rewriting layer

- **Date:** 2026-10-09
- **Source:** 0082 §3
- **Change:** extends §8
- **Summary:** The rewrite that carries a provider's reference to a source entity across a tenant boundary is the read-side rewrite of IDs between tenants, the same layer that rewrites a response to the consumer's preferred form with exact matches only (04 §4.12); the provider's records are never changed, and the four rules of §8 are applied by that layer. 0082's table and heading say `amends`, but §8's text contradicts nothing in this: the rules stand, and only their home is named, so it is logged as `extends`. (REVIEW G5)

### A4. Entity-source numbers are 2^12 to 2^13 − 1

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §4
- **Summary:** Every ID a client sees is below 2^53, so that it survives a JavaScript number and every JSON parser. Registry provider numbers are below 2^12, and entity-source numbers are assigned per tenant from 2^12 to 2^13 − 1 (4096–8191), not from 2^22 to 2^23 − 1, so `number << 40 | n` stays under 2^53 for `n` below 2^40; the registry keeps the 4,095 numbers below the range. (REVIEW G20)

Replaced text: 05 §6.4.

### A5. A source without events follows its recent changes, and may be loaded

- **Date:** 2026-10-26
- **Source:** 0092 §1, §4
- **Change:** amends §4
- **Summary:** A source with an `api` and no `events` is followed by its recent changes, with the daily sweep behind, instead of by the sweep alone. Its partition is written by its own jobs: the fetch job, and loads, which are a dump sync from a dump the operator supplies (`sync --tenant {slug} --source {name}`) or a crawl of its entity namespaces; the actor is the tenant's sync subsidiary. A source still records no upstream actors (Q1 stays open), as befits a passive relay. Chapter text: 05 §5.5, §6.4.

Replaced text: 05 §5.5 of this ADR; 05 §6.4 of this ADR.

### A6. What separates a keyed ID from a source ID

- **Date:** 2026-11-15
- **Source:** 0110 §3
- **Change:** amends §2
- **Summary:** The line between keyed IDs and source IDs is drawn by what makes the ID valid, not by who minted it: a remote key's number was minted by a host, yet the key is valid by its form alone on every instance, while a source ID means something only where the source is declared. Chapter text: 05 §6.2.

Replaced text: 05 §6.2.

### A7. Tenant remote hosts join the table of names

- **Date:** 2026-11-16
- **Source:** Direct: James, design discussion of 2026-11-16
- **Change:** extends §3
- **Summary:** A host a tenant declares for its remote keys (0110 A3) joins the tenant's table of names under the rules a source's name follows. Chapter text: 05 §6.3.

### A8. Retiring keeps the record; a later record is checked against it

- **Date:** 2026-11-20
- **Source:** Direct: James, 2026-11-20, choosing to keep a retired source's record flagged `retired` rather than read the log's history; the other calls below unconfirmed
- **Change:** amends §9
- **Summary:** §9 retires a source by writing its record null, and `view.registry` then drops the row, so the writer no longer saw the source: the next source declared on the tenant took the retired number (4096 went to a new source while `source/lb`'s records still used it), a retired source declared again got a new number, and its IDs stopped resolving. Nor was a later record checked against the first, so a changed IRI template was accepted. Redeclaring a current source already updated it. The calls made: (1) retiring writes the record again with `retired: true`; the row stays in the table of names, so the name, the number and the templates stay reserved and the IDs keep resolving, and a retired source is a retired provider, which `sync` and `follow` refuse. (2) Declaring a retired source again makes it current under its own number; retiring one already retired is refused. (3) A later record is checked against the one it replaces and refused with `ts-source-binding` when it changes the number, or for a type the earlier record declares, its entity type, ID grammar or IRI template, or drops the type; a new type may be added. The prefix's case is not a change. (4) A record written null by an earlier version is still read as no record; none exists outside tests. Chapter text: [05](../architecture/05-providers-and-ingest.md) §6.8.

Replaced text: 05 §6.8 (the binding paragraph).

### A9. `GET /entity-sources` lists every field of a source record

- **Date:** 2026-11-21
- **Source:** Direct: Claude, while building `GET /entity-sources` and siteinfo `entity_sources` (SCA-285), 2026-11-21; confirmed by James, 2026-11-21
- **Change:** extends §10
- **Summary:** `GET /entity-sources` answers `{"sources": [...]}`, and `siprop=triplespace` carries the same list as `entity_sources`. Each source has its name, label, number, types (upstream prefix, entity type, ID grammar and IRI template), `api`, `entity_data`, `revision_ids`, licence, `promoted_to` (null when unpromoted) and `retired`. That is every field of the `entity-source` record; §10 named fewer, and a reader on another instance needs the endpoints and the number as much as the templates. `namespace` is absent until the source's namespace is built (05 §6.5), and `tenant` and `base` until tenant sources are (0080). Sources cannot yet be unpublished, so every source's templates are shown. Retired sources are listed, flagged, since their IDs still resolve (A8). Chapter text: 18 §3.2.

### A10. The source's namespace, its resolution and minting, as built

- **Date:** 2026-11-23
- **Source:** Direct: James, 2026-11-22, deciding that a source's namespace takes the lowest free even number of the tenant's own range (05 §6.5) and its talk namespace the next; and 2026-11-23, choosing to build *Create an item for this* with the `same-as` it writes, and to find the minted item through the tenant's `same-as` links until identity clusters are projected. The rest (the default name, the alias rule, when a namespace is assigned, the `/resolve` and page shapes, `ts-sameas`, `view.same_as`, the rank of source IDs, what is not built), Claude, while building the source's namespace (SCA-460), 2026-11-23; unconfirmed
- **Change:** extends §1, §5, §6, §10
- **Summary:** The `entity-source` record's `namespace` is `{id, canonical, aliases}`. The server gives `id` on a write that has none, the lowest even number from 3000 that neither it nor the next number is used on the tenant, and keeps it after (`ts-source-binding`); the names may change. A declaration naming no namespace gets the name in capitals (`MHC`), or keeps the names it has; the source's name is added as an alias unless a name already matches it in any case. A name another namespace has on the tenant is refused with `ts-namespace-name-taken`; interwiki prefixes are not checked yet, since a tenant has none. A source declared before this has no namespace until it is written again. The tenant's namespace table includes every source's namespace, retired ones too. `GET /resolve` tries a source's ID first, by the source's name or any name of its namespace in any case, and answers `kind: entity-source` with `source`, `label`, `namespace`, the canonical `id`, `external` (the source's entity page under `entity_data`, else the concept IRI), and `target` where the tenant holds the entity or a local item a `same-as` links to it; otherwise it queues the fetch as a read does and reports `fetching`; a string the source's grammar refuses has `invalid`. The site redirects (`303`) to the target, and otherwise shows a `404` page with the ID, the link out, the fetch, and for a signed-in reader *Create an item for this*, which opens `Special:NewItem` with `ts-sameas` and the label-cache label. `wbeditentity` takes `ts-sameas` with `new=item` and writes the `same-as` in the change set that creates the item; any provider's or source's item may be named. `view.same_as` indexes the tenant's own `same-as` links in both directions until clusters are projected; only this resolution reads it. In the provider order a source's ID ranks after every registry provider's, and sources tie, ordered by ID text, since the tenant's `reconcile` order is not read yet. Not built: the lookup by the source's `role` property (step 2 of 05 §6.5; the record has no `role` yet), the redirect of a promoted source's namespace to the provider's ID, and a wiki link's label from the label cache. Chapter text: 05 §6.1, §6.5, §6.6; 03 §4.4; 18 §3.3.
