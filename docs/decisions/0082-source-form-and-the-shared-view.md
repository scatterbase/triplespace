# 0082. Source form, identifier normalization and the shared view

- **Status:** Proposed
- **Date:** 2026-10-09
- **Author:** James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0031](0031-property-constraints.md), [0032](0032-sparql-update-stream.md), [0044](0044-tenant-relative-ids.md), [0059](0059-query-service.md), [0060](0060-scopes.md), [0070](0070-shallow-entity-mirroring.md), [0078](0078-entity-sources.md)
- **Uses:** [0029](0029-resolver-namespaces.md), [0080](0080-tenants-as-entity-sources.md), [0083](0083-write-path-in-three-tiers.md)

## Context

The architecture chapters ([docs/architecture/](../architecture/README.md)) put the resolved view, the identity clusters and the tenant overlay side by side for the first time, and the review of 2026-10-09 ([docs/audits/architecture-review-2026-10-09.md](../audits/architecture-review-2026-10-09.md)) found that three decisions made in different ADRs could not hold together at the scale the core use case names: a Wikidata `full` mirror, local data and OpenAlex on one instance, read by more than one tenant.

[0004](0004-identity-clusters-and-equivalence.md) §4 had the resolved view *rewrite* subjects and entity values to a cluster's canonical ID, and [0013](0013-postgres-storage.md) §5 materialized the rewritten JSON wherever it differed from the source. Together these made the set of materialized entities not "entities in a cluster" but "entities in a cluster plus every entity that references a non-canonical member". With OpenAlex as a second provider most works reference authors, institutions and sources clustered with Wikidata items, so most of the largest table would be materialized, which is the doubling the `source` kind exists to avoid. Tier-1 links are per tenant, so one `same-as` from a local item to `WDQ5` (human) would make the local item canonical for that tenant and require an overlay row, a rewritten body and an RDF delta for every one of the ten million items with `P31 = WDQ5`: one edit, ten million rows per tenant, with the synchronous budget of [0013](0013-postgres-storage.md) §7 bounding the editor's latency but not the work (F-01-1, F-04-1, F-02-7).

[0013](0013-postgres-storage.md) §5 keyed tenant overlays on records in the tenant's own partitions. But provider order, link properties and normalizer overrides ([0004](0004-identity-clusters-and-equivalence.md) §9), the provider list ([0018](0018-tenants.md) §5), the constraint roles ([0031](0031-property-constraints.md) §1) and the term languages ([0070](0070-shallow-entity-mirroring.md) §6) were tenant configuration, and each changes the resolved result of every multi-source entity without a record in any tenant partition. Two tenants with different policy could not share a row, and the only implementations were a full copy of the view per tenant or re-resolution on every read (F-19-14, F-06-3). The cluster tables and `view.entity_source` had no tenant column where the overlay rule needed one (F-01-4, F-04-4), and the shared quad store held one resolved graph per tenant, so forty tenants mirroring Wikidata would hold forty copies (F-02-1). `view.term` under a full mirror was an open question ([0070](0070-shallow-entity-mirroring.md) §6) bounded by a tenant setting that shared rows cannot read (F-04-6), and `entity_ref` at 10^9 rows was keyed on text IDs and statement GUIDs (F-01-10).

The identifier decision here follows the rule [0029](0029-resolver-namespaces.md) and [0004](0004-identity-clusters-and-equivalence.md) §7 already apply to identifier values: normalize at the input, store the canonical form, never rewrite data because a mapping changed. Applied to entity IDs it is three things that this ADR decides together: the stored graph is in source form; every entry point normalizes what it is given; and a layer on the way out rewrites identifiers to the form the consumer asked for. The read-side rewrite is not new either: [0018](0018-tenants.md) §5 and [0078](0078-entity-sources.md) §8 already rewrite IDs on read between tenants, and this ADR generalizes that rewrite from "provider form per tenant" to "the form this consumer prefers", with the cluster map as one more rewrite table.

### Direction

James's direction, from the design review of 2026-10-09:

- On shared rows under one policy: **"I would go with Option A. It does reduce customization per-tenant, but I think performance of the graph as a whole is more important here. I think we could incorporate Option B a little by allowing the addition of configuration profiles later on, but we only implement the one for the whole instance."**
- On the stored form: **"I wonder if we want consistent internal form for the internal graph for lookup purposes (basically the original IRI), and then aliases and alternate forms are rewritten as part of query planning. But the idea is that user inputs are normalized according to the internal representation before the query executes."**
- On responses: **"What if, once the query results were returned, a middle layer rewrote the results to provide the entity identifiers expected by the consumer? This would be done with known exact matches only."**

## Decision

### 1. The internal graph is in source form (amends 0002 §3 and §6, 0004 §4, 0013 §5, 0014 §7, 0018 §7 and 0032 §2; extends 0017 §1 and 0032 §3)

**Every stored form of an entity keeps the IDs its source wrote.** Statement subjects, entity values, qualifier and reference values, `view.entity_ref`, `view.identifier`, the search documents, the RDF dump and the update stream carry `WDQ5` where Wikidata wrote `Q5` and `Q9` where a local editor wrote `Q9`. Nothing rewrites an ID in storage because a cluster formed, changed or dissolved.

**The canonical ID is an attribute of the cluster**, not something applied to data. `view.cluster` names it; the fused body of a cluster is composed once per cluster ([0083](0083-write-path-in-three-tiers.md) §2) from the members' graph states; the per-graph views ([0003](0003-statement-ui.md) §6) are unchanged.

**`materialized` means two or more contributing graphs, or local deltas.** A mirrored entity that merely references a clustered entity is `source`, served from its record. [0013](0013-postgres-storage.md) §5's "materialize only where it differs" is therefore bounded by what tenants have asserted about an entity, never by what references it.

**A cluster change recomposes the cluster's members and nothing else.** There is no delta per referrer ([0032](0032-sparql-update-stream.md) §2), no re-resolution of referrers ([0004](0004-identity-clusters-and-equivalence.md), Consequences), and `entity_ref` is no longer required for correctness (§6).

Statement GUIDs keep [0017](0017-entity-id-grammar.md) §1's rule that the UUID is the identity; the entity part is handled by §3.

### 2. Normalization at every entry point (extends 0012 §5, 0014 §8, 0017 §1, 0044 §1 and §3 and 0059 §2; amends 0010 §3, 0012 §4 and 0014 §7)

**Any member of a cluster is accepted wherever an entity ID is accepted**: API parameters, change sets, titles, the search box, `wbgetentities` and `/resolve`, and SPARQL constants. The entry point normalizes the input to the internal form before anything executes, as it already canonicalizes the tenant-relative form ([0044](0044-tenant-relative-ids.md) §1) and a registered issuer's actor keys ([0079](0079-derived-issuer-codes.md) §9).

**Where an input names a cluster, the planner expands it to the member set.** A lookup by `Q9` over `entity_ref`, `identifier`, `sitelink` or `term` is a lookup over `{Q9, WDQ5, …}`; a SPARQL constant `lb:Q9` becomes `VALUES ?v { lb:Q9 wd:Q5 … }` through the service's rewriting layer ([0059](0059-query-service.md) §2). The expansion is one indexed lookup against `view.cluster_member` per constant, with L0 in front of it.

**Property paths are not expanded.** `wdt:P279*` follows stored triples; a query that needs `owl:sameAs` closure writes it, and the chapters say so. Neither backend does `owl:sameAs` reasoning, and §4 makes the common cases work without it.

**Search results collapse by cluster.** Every search document carries `cluster_id`, maintained by the cluster projection (bounded by members, never by referrers), and `wbsearchentities` and the search page return one result per cluster, the member the consumer's preference (§3) selects.

**The map is a table, not a structure in memory.** An OpenAlex-authors-to-Wikidata map is millions of links; `view.cluster_member` is the map, L0 caches hot entries, and nothing assumes the whole map fits in a process.

### 3. Rewriting in responses, with exact matches only (extends 0003 §2, 0012 §4, 0014 §3 and §4, 0032 §1, 0044 §1 and 0078 §8; amends 0004 §4, §5 and §6 and 0018 §5 and §7)

**A layer on the way out rewrites entity identifiers to the form the consumer prefers.** It is the read-side rewrite of [0018](0018-tenants.md) §5 and [0078](0078-entity-sources.md) §8 — which already turns Librarybase's `Q6` into `LBQ6` or `lb:Q6` for a reader — with the cluster map as one more table: where the consumer prefers the local namespace and `WDQ5` has an exact match `Q9`, the response says `Q9`.

**Exact matches only.** The rewrite uses tier-1 links (`same-as`, `convert`), tier-2 links (an identifier statement of a configured link property, [0004](0004-identity-clusters-and-equivalence.md) §3) and `equivalent-property` ([0004](0004-identity-clusters-and-equivalence.md) §6). Tier-3 inference from shared identifier values is never used, nor is anything a `different-from` has split. The rewrite is therefore a bijection within a cluster and a pure function of the map, so two surfaces never disagree and a consumer can round-trip what it was shown through §2 without the server remembering what form it gave out.

**The preference is per request.** `prefer` is a namespace order (`local`, a provider code, a source name); the default is the form of the IDs in the request itself — ask for `WDQ42` and values come back in Wikidata form, ask for `Q9` and they come back local — and the site UI asks for local. **An ID with no exact match in the preferred namespace is left as stored.**

**Where it applies:** Action API and REST JSON; SPARQL result bindings, through the concept-IRI template, since local IRIs dereference; search results; and HTML. It rewrites the entity part of a statement GUID, consistently with [0017](0017-entity-id-grammar.md) §1 and [0004](0004-identity-clusters-and-equivalence.md) §9, where the UUID is the identity. It never rewrites inside string literals or URLs.

**It runs after the cache.** L1 holds the internal form once per entity version ([0014](0014-caches-and-search.md) §4); the rewrite is applied to a hit on the way out, so there is one cache entry, not one per preference.

**Dumps and the update stream are source form** (§1): the contract of [0032](0032-sparql-update-stream.md) §1 is the dump kept current, and the dump is the internal graph. A rewritten dump profile, if ever wanted, is a serialization-time pass over a dump and not a stored form.

**`wbgetentities` answers under the requested ID.** A request for a non-canonical member returns the fused body under that ID, with `canonical` naming the cluster's canonical member, and GUIDs prefixed with the requested ID. The Wikibase redirect shape (`redirects: {from, to}`) is reserved for true redirects: a `redirect` operation or an upstream merge. This replaces [0004](0004-identity-clusters-and-equivalence.md) §4's "the API resolves a non-canonical member to the canonical entity, as Wikibase resolves a redirect", which made Pywikibot raise `IsRedirectPageError` on every clustered Wikidata item (F-16-7).

**Two consumers of one tenant may see different IDs for one value**, and a URL copied from one view resolves in the other only because §2 accepts any member. §2 is therefore not optional for any entry point.

### 4. RDF: the fused body under every member (amends 0004 §4; extends 0001 §1 and 0032 §2)

The resolved graph emits the cluster's fused statements under **every member's** concept IRI, with `owl:sameAs` between the members and the redirect-form `owl:sameAs` from each non-canonical member to the canonical one as before. Duplication is bounded by cluster size, never by referrers. A query that reaches `wd:Q5` through a value and asks for its label or its statements then succeeds without rewriting, which is why §2 can leave paths alone. The `schema:about` triple from each document node to the canonical concept IRI ([0004](0004-identity-clusters-and-equivalence.md) §4) is kept.

### 5. Shared rows are computed under a named instance policy (amends 0004 §9, 0013 §5, 0018 §3, §5 and §6, 0031 §4 and 0070 §6; extends 0031 §1, 0032 §4 and 0059 §3 and §4)

**The shared row (`tenant = ''`) of every `view` table is computed under one policy record, the instance's `reconcile` record** (`reconcile:default` in the instance `config`, the record that [0004](0004-identity-clusters-and-equivalence.md) A17/B6 already gave the inference properties), which now holds for shared rows: the provider order among registry providers; the tier-2 link properties; the normalizer overrides; the role bindings the constraint checker and the adapters read (§5.2); and the languages that populate shared `view.term` (§6).

**A tenant narrows; it does not reorder.** A tenant's `reconcile` record may hide a provider in its own view and API (a filter on read), restrict its own display languages, and add link properties and ordering for tenant sources ([0080](0080-tenants-as-entity-sources.md) §2), whose graphs are read per tenant anyway. It may not reorder shared providers, change a shared normalizer or rebind a shared role; a tenant-scope write that tries is refused with **`ts-instance-policy`**, and the tenancy presets ([0028](0028-tenancy-policy.md)) say so.

**The shared row names the policy it was computed under.** `view.entity` and the shared rows of its dependents carry `policy`, the policy record's code. Only `default` exists; *resolution profiles* — a shared view computed once per distinct policy record, with tenants bound to a profile and the operator deciding how many the instance affords — can be added later by adding records and a projection rebuild, with no change to the log. That is the extension James named and this ADR leaves the door to.

**The quad store holds one shared resolved graph** ([0059](0059-query-service.md) §4; [0032](0032-sparql-update-stream.md) §4): the instance's, computed under `default`, plus small per-tenant overlay graphs holding the entities each tenant has overlaid. A tenant's dataset is the shared graph with its overlay shadowing, and [0018](0018-tenants.md) §6's promise that a query written for Wikidata "runs unchanged" holds for a tenant with no overlay and for the shared graph. Forty tenants mirroring Wikidata share one copy of Wikidata's resolved triples, which was the claim and is now true.

#### 5.1 Tenant columns and the overlay-exclusion rule (amends 0004 §3, 0013 §5, 0014 §7 and §10 and 0018 §6)

`view.cluster`, `view.cluster_member`, `view.link`, `view.different_from` and `view.entity_source` gain the `tenant` column with the `''`-then-tenant lookup of `view.entity`; `cluster_member`'s unique key is `(tenant, entity_id)`, so a tenant's tier-1 link on `WDQ42` sits beside the shared cluster, and two tenants' `local` rows on one foreign entity no longer collide in `entity_source`.

**A read over shared rows by any key other than the entity excludes shared rows whose entity the viewer's tenant has overlaid**: `identifier` by `(property, value_key)`, `sitelink` by `url_key`, `entity_ref` by target, `term` by prefix. Each of those tables carries an index that makes the anti-join one probe. For search, the overlay projection sets a `tenant_overlaid` term on the shared document naming the tenants that have overlaid the entity, and a tenant's query excludes shared documents that name it, so a tenant document replaces the shared one without a post-query dedup.

#### 5.2 Roles at instance scope (extends 0031 §1; amends 0003 §7 and 0031 §4)

The `role` config kind has an **instance-scope record** beside the tenant records. The constraint checker computes shared-row violations under the instance bindings ([0031](0031-property-constraints.md) §4's "once for the instance" now has a vocabulary to compute with), adapters read the instance bindings and never a tenant's, and a tenant whose bindings differ gets overlay rows for what the difference touches, which on a Wikidata mirror is everything, so in practice a tenant on a farm does not rebind the constraint roles.

### 6. `view.term` and `view.entity_ref` at scale (amends 0013 §5 and §11, 0014 §8, 0018 §6 and 0070 §4 and §6)

**`view.term` holds** rows for local entities, for entities in any tenant overlay, and for mirrored entities in the **instance's** term languages (`entities.term_languages` is instance configuration for shared rows; a tenant's setting of the same name only narrows its own display). Every other label lookup reads the record body at `entity_source.offset` through the L1 label cache that [0070](0070-shallow-entity-mirroring.md) §4 defines for unmirrored entities; a value shown under one member may take its label from another member's rows, with precedence from the policy order. `wbsearchentities` at Wikidata scale is the search index ([0014](0014-caches-and-search.md) §8), and **`term_prefix` is never created on an instance holding a full mirror**; the small profile's index is partial over local entities. The budget is rows ≈ mirrored entities × |instance languages| × term kinds, and the Wikidata-scale profile lists it beside `entity_ref`. This settles [0070](0070-shallow-entity-mirroring.md) Q2.

**`view.entity_ref` is keyed on integers:** `(target_page_id, source_page_id, roles)` with `roles` a bitmask of main snak, qualifier and reference, and no `statement_id` (the resolved JSON answers "which statement" when a panel asks). Its one job is "Links here" and the backlink count; the Wikidata-scale profile may serve both from the query service and omit the table.

### 7. Crates (extends 0005 §2 and 0060 §11)

| Crate | Change |
|---|---|
| `scatter-wikibase-model` | Source-form invariant: no rewriting of entity IDs in stored or hashed JSON; the fused body composed per cluster |
| `scatter-identity` | The exact-match map (tier 1, tier 2, `equivalent-property`), `cluster_id` for search documents, the member-set expansion |
| `scatter-wikibase-rdf` | Emission under every member with `owl:sameAs` (§4) |
| `scatter-scope` | Constant expansion in SPARQL through the rewriting layer; paths untouched |
| `triplespace-api-action`, `triplespace-api-rest` | Normalization at every parameter; the `prefer` parameter and the response rewrite after the cache; `wbgetentities` under the requested ID with `canonical` |
| `triplespace-ui` | Local preference; labels by cluster |
| `triplespace-projections` | `policy` on shared rows; the tenant columns and `tenant_overlaid` of §5.1; instance-language terms; integer-keyed `entity_ref`; `ts-instance-policy` on tenant-scope writes |
| `triplespace-db` | The migrations for §5.1 and §6 |

## Alternatives considered

- **Rewrite on write, as [0004](0004-identity-clusters-and-equivalence.md) §4 had it.** Correct for a handful of local items linked to Wikidata; O(referrers) per tenant per link at the scale the core use case names, and it is what forced `entity_ref` to exist at 10^9 rows.
- **Rewrite on write, with tier-1 links as instance acts.** Halves the problem (one canonical per cluster for everyone) and does nothing about OpenAlex works becoming materialized by referencing clustered institutions.
- **Store the rewritten form as a diff over the source record.** Keeps `source` kind for most entities, but the fan-out is still O(referrers) rows and it is a new storage shape.
- **Read-time rewrite on output only, without input normalization.** Works for display; a consumer could not round-trip what it saw, and a Wikidata-keyed bot would see mixed IDs.
- **Resolution profiles now.** Honest and general, and the shape this ADR leaves room for; it multiplies every shared table by the profile count for a second tenant that does not yet exist.
- **Re-resolve on read under the tenant's policy.** No storage cost; O(contributing graphs) per read, uncached across tenants, and API latency depending on how many providers an entity has.

## Consequences

- **A cluster change is O(members).** Linking a local item to `WDQ5` touches the cluster's rows and no referrer; the overlay holds only what a tenant asserted something about.
- **The internal graph is stable.** Nothing in storage depends on cluster membership, so a cluster can be split or joined without rewriting a row, and the dump and stream never emit a referrer's triples for an identity change.
- **Consumers see their own IDs.** A Wikidata-keyed bot sees `WDQ…` everywhere, a local user sees `Q…` everywhere, both looking at one stored form; the price is that two views of one tenant can differ, and every entry point must normalize.
- **Tenants on a shared instance lose a freedom [0004](0004-identity-clusters-and-equivalence.md) §9 promised**: provider order among shared providers and shared normalizers are the instance's. The tenancy presets must say so, and profiles are the escape hatch.
- **Milestone 1 already behaves this way** for a single tenant with no clusters; the changes land in stage 3 (the full mirror) and with the first cluster work. `scatter-wikibase-resolve` as built fuses one local-plus-mirror case and rewrites nothing, which is the invariant of §1.

## Open questions

- **Q1. The exact-match map for provider-tenant links.** [0080](0080-tenants-as-entity-sources.md) §2 makes a provider tenant's `same-as` records tier-2 links for readers; whether those count as exact matches for rewriting on a reader that has not itself asserted anything about the entities is open. The conservative reading, taken for now, is yes, since they are explicit assertions of identity by a source.
- **Q2. A short form of `prefer` in URLs.** Whether the site UI's local preference should be expressible in a sharable URL, so that a link copied from a Wikidata-form view opens in the same form.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0001](0001-revision-metadata-rdf.md) §1 | §4 | extends | 0001 A18 |
| [0002](0002-source-graphs-and-mass-ingest.md) §3 | §1, §5 | amends | 0002 A31 |
| [0002](0002-source-graphs-and-mass-ingest.md) §6 | §1, §3 | amends | 0002 A31 |
| [0003](0003-statement-ui.md) §2 | §3, §6 | extends | 0003 A15 |
| [0003](0003-statement-ui.md) §7 | §5.2 | amends | 0003 A14 |
| [0004](0004-identity-clusters-and-equivalence.md) §3 | §5.1 | amends | 0004 A16 |
| [0004](0004-identity-clusters-and-equivalence.md) §4 | §1, §3, §4, §5 | amends | 0004 A16 |
| [0004](0004-identity-clusters-and-equivalence.md) §5 | §1, §3 | amends | 0004 A16 |
| [0004](0004-identity-clusters-and-equivalence.md) §6 | §3 | amends | 0004 A16 |
| [0004](0004-identity-clusters-and-equivalence.md) §9 | §1, §5 | amends | 0004 A16 |
| [0005](0005-crate-organization.md) §2 | §7 | extends | 0005 A89 |
| [0010](0010-site-ui.md) §3 | §2, §3 | amends | 0010 A37 |
| [0012](0012-api-requirements.md) §4 | §2, §3 | amends | 0012 A64 |
| [0012](0012-api-requirements.md) §5 | §2, §3 | extends | 0012 A64 |
| [0013](0013-postgres-storage.md) §5 | §1, §5, §5.1, §6 | amends | 0013 A52 |
| [0013](0013-postgres-storage.md) §11 | §6 | amends | 0013 A52 |
| [0014](0014-caches-and-search.md) §3 | §3 | extends | 0014 A24 |
| [0014](0014-caches-and-search.md) §4 | §3 | extends | 0014 A24 |
| [0014](0014-caches-and-search.md) §7 | §1, §2, §5.1 | amends | 0014 A24 |
| [0014](0014-caches-and-search.md) §8 | §2, §6 | amends | 0014 A24 |
| [0014](0014-caches-and-search.md) §10 | §5.1 | amends | 0014 A24 |
| [0017](0017-entity-id-grammar.md) §1 | §1, §2 | extends | 0017 A12 |
| [0018](0018-tenants.md) §3 | §5 | amends | 0018 A23 |
| [0018](0018-tenants.md) §5 | §3, §5 | amends | 0018 A23 |
| [0018](0018-tenants.md) §6 | §5, §5.1, §6 | amends | 0018 A23 |
| [0018](0018-tenants.md) §7 | §1, §3 | amends | 0018 A23 |
| [0031](0031-property-constraints.md) §1 | §5, §5.2 | extends | 0031 A6 |
| [0031](0031-property-constraints.md) §4 | §5.2 | amends | 0031 A6 |
| [0032](0032-sparql-update-stream.md) §1 | §3 | extends | 0032 A8 |
| [0032](0032-sparql-update-stream.md) §2 | §1, §4 | amends | 0032 A8 |
| [0032](0032-sparql-update-stream.md) §3 | §1 | extends | 0032 A8 |
| [0032](0032-sparql-update-stream.md) §4 | §5 | extends | 0032 A8 |
| [0044](0044-tenant-relative-ids.md) §1 | §2, §3 | extends | 0044 A5 |
| [0044](0044-tenant-relative-ids.md) §3 | §2 | extends | 0044 A5 |
| [0059](0059-query-service.md) §2 | §2 | extends | 0059 A7 |
| [0059](0059-query-service.md) §3 | §5 | extends | 0059 A7 |
| [0059](0059-query-service.md) §4 | §5 | extends | 0059 A7 |
| [0060](0060-scopes.md) §11 | §7 | extends | 0060 A10 |
| [0070](0070-shallow-entity-mirroring.md) §4 | §6 | amends | 0070 A4 |
| [0070](0070-shallow-entity-mirroring.md) §6 | §6 | amends | 0070 A4 |
| [0078](0078-entity-sources.md) §8 | §3 | extends | 0078 A3 |

## References

- Wikibase redirects: a redirect target is substituted on read and `owl:sameAs` is emitted in RDF; the precedent for leaving stored values alone
- [docs/audits/architecture-review-2026-10-09.md](../audits/architecture-review-2026-10-09.md): findings F-01-1, F-01-4, F-01-10, F-02-1, F-02-7, F-04-1, F-04-4, F-04-6, F-06-2, F-06-3, F-16-7, F-19-14; decisions G1–G7
