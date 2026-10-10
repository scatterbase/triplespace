# 0004. Identity clusters and equivalence

- **Status:** Proposed
- **Date:** 2026-09-25
- **Updated:** 2026-10-09 (A22)
- **Author:** James Hare / Claude Opus
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md)
- **Uses:** [0000](0000-init.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)
- **Chapters:** [04](../architecture/04-entities-and-identifiers.md)

## Context

The same thing often exists in several sources. A journal article can have a Wikidata item, an OpenAlex work and a local entity at once. [0002](0002-source-graphs-and-mass-ingest.md) lets entities from different sources sit side by side, but it does not say how they come to be known as one thing. Two of its open questions depend on this:

- how to reconcile two mirror graphs whose entities are linked by same-as;
- what happens when a local alias conflicts with an upstream redirect.

There are three needs:

1. **A canonical identifier** for a thing that has several.
2. **Equivalent properties** across sources.
3. **Equivalent values** for a property across sources.

## Decision

### 1. Identity clusters

*Changed by A1, A2, A7, A9, A11, A12.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.1.*

### 2. Clusters are strictly one-to-one

*Changed by A12.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.2.*

### 3. Where links come from

*Changed by A3, A4, A14, A16.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.3.*

### 4. The canonical ID

*Changed by A1, A4, A5, A12, A13, A16, A17.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.4.*

### 5. Conversion joins a cluster (amends 0002 §6)

*Changed by A16.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.5.*

### 6. Properties

*Changed by A16, A19.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.6.*

### 7. Values

*Changed by A4, A6, A20, A21.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.7.*

### 8. Fusing equivalent statements

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.8.*

### 9. Operations (amends 0002 §8.2)

*Changed by A4, A13, A16, A18, A22.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.10.*

### 10. Conflicts

*Changed by A8.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.11.*

## Consequences

- ~~**Canonical IRIs in the resolved view can change.** External consumers must follow `owl:sameAs`. Any change in membership re-resolves every entity that refers to any member. This needs a reverse index from each member to the statements that refer to it. Linking a heavily cited author can fan out to many works.~~ *The stored graph is in source form and the fused body is emitted under every member, so a cluster change recomposes the cluster's members and no referrer; `entity_ref` serves "Links here", not correctness (A16).*
- **Clusters are cheap to maintain.** Their size is bounded by the number of namespaces, so a cluster is recomputed from its links whenever one changes. Retracting a link does not require an incremental union-find that supports deletion.
- **Conflicts are handled conservatively, and that can take data away.** A held conflict can detach statements an entity used to have. An example is the upstream-merge case in §5. The conflict is visible, but the entity's resolved view has lost those statements until it is resolved.
- ~~**An OpenAlex mirror depends on Wikidata properties.** An instance that mirrors OpenAlex must import at least the mapped `WDP` properties, for example as a terms-only subset ([0002](0002-source-graphs-and-mass-ingest.md) §8.4). It must also bind the adapter's local roles.~~ *The import of the mapped `WDP` properties still holds; the roles an adapter reads are the instance-scope `role` record's, bound to registry-provider properties or to the provider's own property type, never a tenant's local ones (A19).*
- **Link coverage depends on upstream.** An OpenAlex work that no Wikidata item links to, and that no local assertion links, stays canonical under its `OAW` ID.
- **The two property forms in [0003](0003-statement-ui.md) §7 can be linked.** 0003 lets one role name both a local and a mirrored property. If the two are linked by `equivalent-property`, a role map can name either one.
- **Resolution costs more.** Every statement key requires normalizing values and looking up clusters, and this adds to the reconciliation cost noted in 0002.

## Open questions

- **Q1.** ~~**Provider order per entity type.** For example, whether a citation-focused instance could rank OpenAlex above Wikidata for works.~~ *Settled by A4: yes. The `reconcile` record ([0015](0015-record-format-and-partition-registry.md) §3) gains an optional `order_by_type` map (`work: [openalex, wikidata]`) that overrides the default order for clusters whose members are of that type; a cluster's members share a type by construction, so the order is well defined. Keyed members still rank first ([0009](0009-keyed-entity-types-and-domain.md) §8) and local members next.* *Extended by A16: the record that sets the order of the registry providers, `order_by_type` included, is the instance's `reconcile` record, the policy record of every shared row; a tenant's record orders only its own entity sources ([0082](0082-source-form-and-the-shared-view.md) §5).*
- **Q2.** ~~**Inferred links.** Which identifier properties should be eligible, and whether inferred links need review before they are applied.~~ *Settled by A4: a tier-3 link is applied at once, as §3 implies, for the identifier properties the `reconcile` record names; nothing is queued. The safeguards are the ones §3 already has, a value must be unique within each namespace and `different-from` beats every tier, plus the conflict list of §10 and the maintenance listing of [0002](0002-source-graphs-and-mass-ingest.md) §7, where an editor undoes a bad inference with `different-from`. Which properties are eligible is the tenant's `reconcile` configuration; the registry ships none enabled.* *Changed by A14: the identifier properties that drive inference are instance configuration, in the instance `reconcile` record; a tenant's `reconcile` may switch inference off for itself but not add inference properties.*
- **Q3.** ~~**Time precision.** Whether a less precise value that a more precise one contains, such as "2019" and "2019-03-04", should fuse or only be shown as corroborating it.~~ *Settled by A4: they do not fuse; §7 already makes precision part of the key. The statement UI ([0003](0003-statement-ui.md)) marks the coarser value as consistent with the finer one in the same group, and the resolved view keeps both, so the view never claims a precision no source asserted.*
- **Q4.** ~~**Retiring a local property.** When Wikidata later creates a property that a local property duplicates, `equivalent-property` keeps the local one canonical. It is open whether a redirect across namespaces should exist so that the local property can be retired in favor of Wikidata's.~~ *Settled by A4: for properties only, a local `redirect` may cross namespaces: `redirect P12 → WDP585` retires `P12`, makes `WDP585` the canonical predicate, rewrites the resolved view as §6 rewrites a linked pair, and emits `owl:equivalentProperty` for each predicate family with the redirect-form `owl:sameAs`; existing local statements keep `P12` in the log and read as `WDP585`. It needs `property-create` ([0016](0016-permissions-and-access-control.md) §2) and a matching data type. Items keep §2's rule: no redirect across namespaces, and a local item stays canonical.* *Extended by A16: stored predicates keep their IDs; `P12` is served under `WDP585` by the response rewrite, and the fused body treats the pair as one property ([0082](0082-source-form-and-the-shared-view.md) §1, §3).*
- **Q5. Sub-properties.** Whether `rdfs:subPropertyOf` should be recorded, and whether it should affect resolution.
- **Q6.** ~~**Permissions.** Who may assert `same-as` and `different-from`, run them in bulk, or resolve conflicts.~~ *Settled by [0016](0016-permissions-and-access-control.md) §5: `ts-link`, in bulk with `ts-runjob`; `equivalent-property` needs `property-create`.*
- **Q7.** ~~**A stable external IRI.** Whether to recommend the alias form `{base}/entity/<any member>` to external consumers as a reference that survives changes of canonical ID.~~ *Settled by A4: the recommended stable handle is the **document node** of any member, `{base}/wiki/Special:EntityData/{id}` ([0015](0015-record-format-and-partition-registry.md) §6), which exists for every member whatever minted its ID and never moves. The resolved view already carries `schema:about` from each document node to the current canonical concept IRI ([wikibase-compat.md](../api/wikibase-compat.md) §5.2), so a consumer that cites the document node reaches the concept through one triple that the update stream ([0032](0032-sparql-update-stream.md)) keeps current; concept IRIs stay canonical-and-changing as §4 says, and no new IRI kind enters the main graph. The dump and stream documentation says so; [0029](0029-resolver-namespaces.md) §4's `DOI:{key}` is the same recommendation for a thing known by an identifier.* *Extended by A16: the document node stays the recommended handle, and the fused body is now also emitted under every member's concept IRI, so any member's concept IRI reaches the statements directly ([0082](0082-source-form-and-the-shared-view.md) §4).*
- **Q8.** ~~**Statement IDs after conversion.** Carried over from 0002.~~ *Settled by [0018](0018-tenants.md) §7: the resolved view rewrites the GUID's entity prefix to the canonical ID and keeps the UUID.* *Extended by A16: the resolved view keeps the prefix as written; the entity part is rewritten only in responses, to the form the consumer prefers, and the UUID is the identity ([0082](0082-source-form-and-the-shared-view.md) §1, §3).*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §6 | §5, §9 | extends | 0002 A1 |
| [0002](0002-source-graphs-and-mass-ingest.md) §8.2 | §5, §9 | amends | 0002 A1 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q2 | §4–§9 | settles | 0002 Q2 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q3 | §5 | settles | 0002 Q3 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q6 | §6 | settles | 0002 Q6 |
| [0003](0003-statement-ui.md) §6, §7 | §6, §8 | extends | 0003 A1 |

## Amendment log

### A1. Keyed types rank first

- **Date:** 2026-09-26
- **Source:** [0009](0009-keyed-entity-types-and-domain.md) §8
- **Change:** extends §1; amends §4
- **Summary:** A keyed type is a namespace of its own. Keyed members rank first in a cluster, ahead of local members.

Replaced text (§4):

> - A local member always ranks first.
> - Each instance sets the order of the providers after it. The default is Wikidata, then OpenAlex.

### A2. Keywords never cluster

- **Date:** 2026-09-27
- **Source:** [0017](0017-entity-id-grammar.md) §5
- **Change:** extends §1
- **Summary:** `same-as` with a keyword on either side is rejected at write time, and no tier-2 or tier-3 link may produce one. A keyword may carry an ordinary statement pointing at the concept it usually denotes.

### A3. Tenants: links and overlays

- **Date:** 2026-09-27
- **Source:** [0018](0018-tenants.md) §5–6
- **Change:** extends §3
- **Summary:** Tier-2 and tier-3 links come from shared graphs and are instance-wide; tier-1 links and `different-from` blocks are each tenant's own. A tenant's clusters are the shared clusters with its overlay applied. A provider tenant's `same-as` and `convert` records are tier-1 links for it and tier-2 links for everyone else. 0018 §7 also settled Q8.

### A4. Decisions in review

- **Date:** 2026-09-27
- **Source:** Direct: James, review of 2026-09-27
- **Change:** extends §3, §4, §7; amends §9
- **Summary:** For **properties only**, a local `redirect` may cross namespaces (`{"op":"redirect","from":"P12","to":"WDP585"}`), retiring the local property in favour of the mirrored one; see the open-questions note on retiring a local property. The `reconcile` record may carry `order_by_type` to rank providers differently per entity type. The same review settled Q1–Q4 and Q7: providers may be ordered per entity type; inferred links apply at once for the properties the `reconcile` record names; a coarser time does not fuse with a finer one; properties, not items, may redirect across namespaces; and the stable external handle is a member's document node.

Replaced text (§9):

> `redirect` in the local graph now records only a merge within one namespace. The same-as meaning that 0002 §8.2 gave it moves to `same-as`.

### A5. Deleted members leave the cluster

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §4
- **Change:** extends §4
- **Summary:** A deleted local entity leaves its identity cluster, as a member tombstoned under `orphan` does; the next member becomes canonical. Its `same-as` and `different-from` records are kept but not applied, and apply again on undelete.

### A6. The url normalizer

- **Date:** 2026-09-27
- **Source:** [0026](0026-sitelinks.md) §1
- **Change:** extends §7
- **Summary:** The sitelink URL normalizer is the `url` data type's normalized key.

### A7. OpenStreetMap tags are not concepts

- **Date:** 2026-09-28
- **Source:** [0036](0036-openstreetmap-providers.md) §6
- **Change:** extends §1
- **Summary:** `osm-tag` entities never join an identity cluster. 0048 later moved the rule to notations of every scheme (A9).

### A8. Conflict and linking forms

- **Date:** 2026-09-30
- **Source:** [0047](0047-special-pages.md) §5–6
- **Change:** extends §10
- **Summary:** The review list is `Special:IdentityConflicts`, with these resolutions as row actions. `Special:LinkEntities` is the form for `same-as`, `different-from` and `equivalent-property`; `Special:MergeItems` writes a merging `redirect` within one namespace and refuses a merge across namespaces.

### A9. Notations never cluster

- **Date:** 2026-10-01
- **Source:** [0048](0048-notation.md) §3
- **Change:** extends §1
- **Summary:** No notation of any scheme joins an identity cluster, as A7 decided for OpenStreetMap tags and A2 for keywords.

### A10. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–10
- **Summary:** A1–A9 were folded into the Decision and the open questions were numbered; the review decisions that settled Q1–Q4 and Q7 are A4. No decision changed. Before this, A4's §9 part and A8 were blockquotes, and the other entries were recorded only in this ADR's header, its open questions or other ADRs. The file before conversion is commit `0b26a3a`.

### A11. Lexeme parts

- **Date:** 2026-10-05
- **Source:** [0066](0066-lexemes.md) §4
- **Change:** extends §1
- **Summary:** Clusters hold lexemes; forms and senses never join one and keep their IDs across a cluster.

### A12. One member per namespace the tenant reads; provider-number order

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §1, §2, §4
- **Summary:** A cluster has at most one member per namespace the tenant reads: the registry's providers, the keyed types and the entity sources, not "the local instance, Wikidata or OpenAlex" and not "three today". The default provider order, after keyed and local members, is provider-number order in `providers.toml`, overridable by `reconcile`; "Wikidata, then OpenAlex" is no longer stated as the default. (PENDING A16)

Replaced text (§1):

> Each member of a cluster belongs to a **namespace**, meaning whoever minted the ID: the local instance, Wikidata or OpenAlex.

Replaced text (§2):

> **Clusters stay small.** A cluster can hold no more members than there are namespaces, which is three today.

Replaced text (§4):

> - Each instance sets the order of the providers after them. The default is Wikidata, then OpenAlex.

### A13. `reconcile` is tenant configuration

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §4, §9
- **Summary:** The `reconcile` record is tenant configuration ([0018](0018-tenants.md) §3), not "an instance's configuration": the provider order, link properties and normalizer overrides of §9 are set per tenant. The inference properties are the exception, which A14 states. The §4 sentence this corrects is the one A12 also replaced ("Each instance sets the order of the providers after them."); it is quoted under A12. (PENDING A17)

Replaced text (§9):

> **Configuration is recorded in the log.** The following settings are part of an instance's configuration:

### A14. Inference properties are instance configuration

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §3
- **Summary:** The identifier properties that drive tier-3 inference are **instance** configuration, named in the instance `reconcile` record, so tier-3 links stay instance-wide (§3's farm rule). A tenant's `reconcile` may switch inference off for itself and set its provider order and link properties, but may not add inference properties. [0015](0015-record-format-and-partition-registry.md) §3 (`reconcile`) and 0018 §3 are amended the same way. Q2's settlement is extended accordingly. (PENDING B6)

Replaced text (§3):

> An inferred link is applied at once, for the identifier properties the tenant's `reconcile` record names; nothing is queued for review, and the registry ships none enabled.

### A15. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [04](../architecture/04-entities-and-identifiers.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A16. Source form, rewriting in responses and the instance policy record

- **Date:** 2026-10-09
- **Source:** [0082](0082-source-form-and-the-shared-view.md) §1, §3, §4, §5, §5.1
- **Change:** amends §3, §4, §5, §6, §9
- **Summary:** The internal graph is in source form: the canonical ID is an attribute of the cluster, named by `view.cluster`, and never applied to data; statement subjects, entity values, `entity_ref`, `identifier`, the search documents, the dump and the stream keep the IDs their sources wrote, and nothing rewrites an ID in storage because a cluster formed, changed or dissolved. The resolved view is the cluster's fused body, composed once per cluster; `materialized` means two or more contributing graphs or local deltas, nothing else; a cluster change recomposes the cluster's members and nothing else, with no delta per referrer and no re-resolution of referrers. In RDF the fused body is emitted under every member's concept IRI with `owl:sameAs` between the members, beside the redirect-form `owl:sameAs` to the canonical one. The form a consumer sees is a property of the response: a layer on the way out rewrites entity IDs, the entity part of statement GUIDs and `unit`, `globe` and `calendarmodel` to the consumer's preferred namespace order (`prefer`, defaulting to the form of the request; local for the site UI), using exact matches only (tier 1, tier 2 and `equivalent-property`, never tier-3 inference or anything a `different-from` split), after the cache, never inside literals; `wbgetentities` answers under the requested ID with a `canonical` field, and the Wikibase redirect shape is reserved for true redirects. So a converted `WDQ123` no longer "resolves to" `Q456` (§5); the two are one cluster, and a property `redirect` across namespaces makes the old ID an alias served under the new one (§9). Shared rows are computed under the instance's `reconcile` record, the named policy record, which holds the provider order among registry providers, the tier-2 link properties, the inference properties, the normalizer overrides, the role bindings for shared rows and the shared term languages; a tenant narrows (hides a provider, restricts its display languages, declines inferred links, orders its own entity sources) but may not reorder shared providers, change a shared normalizer or rebind a shared role, and a tenant-scope write that tries is refused with `ts-instance-policy`; the shared row carries `policy`, and only `default` exists. `view.cluster`, `view.cluster_member`, `view.link`, `view.different_from` and `view.entity_source` gain the `tenant` column with the `''`-then-tenant lookup, and `cluster_member`'s unique key is `(tenant, entity_id)` (§3). The fused body treats an `equivalent-property` pair as one property, with stored predicates keeping their IDs (§6). 0082's table listed §3, §4 and §9; the chapter also folds §5 ([04](../architecture/04-entities-and-identifiers.md) §4.5) and §6 ([04](../architecture/04-entities-and-identifiers.md) §4.6), and the table was extended. (REVIEW G1, G2, G3, G5, G6)

Replaced text ([04](../architecture/04-entities-and-identifiers.md) §4.3, as it stood):

> **Inferred links need a unique value.** A value that appears on more than one entity in a namespace produces no link. Values are compared by their normalized keys (§4.7). An inferred link is applied at once, for the identifier properties the **instance's** `reconcile` record names; nothing is queued for review, and the registry ships none enabled. A tenant's `reconcile` record may switch inference off for that tenant, and sets its provider order and link properties (§4.10), but cannot add inference properties, so every tenant of a farm infers the same links from a shared graph.
>
> **On a farm,** tier-2 and tier-3 links come from shared graphs and are instance-wide. Tier-1 links and `different-from` blocks are each tenant's own. A provider tenant's `same-as` and `convert` records are tier-2 links for every other tenant ([0018](../decisions/0018-tenants.md) §5–6, in [08](../architecture/08-tenants-and-instances.md)).

Replaced text ([04](../architecture/04-entities-and-identifiers.md) §4.4, as it stood):

> - The tenant's `reconcile` record sets the order of the providers after them; the default is provider-number order in `providers.toml` (Wikidata first). The `reconcile` record ([0015](../decisions/0015-record-format-and-partition-registry.md) §3, in [23](../architecture/23-configuration-and-registry.md)) may set a different order per entity type, `order_by_type`; a cluster's members share a type, so the order is always defined.

> So if a local item `Q77` about en.wikipedia.org existed before Domains did, `same-as` between `Q77` and `domain:en.wikipedia.org` makes the Domain canonical. `Q77` resolves to it and gets the redirect-form `owl:sameAs` in RDF. A cluster whose links would join two Domains is a conflict, and the links are held (§4.11).

> **The canonical ID is chosen when the resolved view is projected.**
>
> - Source graphs keep IDs exactly as they were asserted.
> - The resolved view ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3, in [02](../architecture/02-graphs-rdf-and-query.md)) rewrites subjects and entity values to the canonical ID.
> - Each non-canonical member is emitted in Wikibase's redirect form, `<member IRI> owl:sameAs <canonical IRI>`.
> - The API resolves a non-canonical member to the canonical entity, as Wikibase resolves a redirect.
> - The per-graph views ([0003](../decisions/0003-statement-ui.md) §6) still show each member's own data.

Replaced text ([04](../architecture/04-entities-and-identifiers.md) §4.5, as it stood):

> - `WDQ123` resolves to `Q456`;
> - mirrored statements under `WDQ123` attach to `Q456`;
> - the redirect-form `owl:sameAs` is emitted.

Replaced text ([04](../architecture/04-entities-and-identifiers.md) §4.6, as it stood):

> - **The resolved view uses the canonical property's predicates** for every statement in the cluster.
> - **RDF output:** property entity IRIs get the redirect-form `owl:sameAs`, as items do. OWL equivalence holds between predicates, not between entity IRIs. So `owl:equivalentProperty` is emitted for each pair of predicates in the same family: `wdt:`, `p:`, `ps:`, `psv:`, `pq:`, `pqv:`, `pr:`, `prv:`, and the normalized forms.

Replaced text ([04](../architecture/04-entities-and-identifiers.md) §4.10, as it stood):

> `redirect` in the local graph records a merge within one namespace. **For properties only, a local `redirect` may cross namespaces:** `{"op":"redirect","from":"P12","to":"WDP585"}` retires `P12` and makes `WDP585` the canonical predicate. The resolved view is rewritten as §4.6 rewrites a linked pair, and existing local statements keep `P12` in the log and read as `WDP585`. It needs `property-create` and a matching data type. An item never redirects across namespaces, and a local item stays canonical.

> **Configuration is recorded in the log.** The following settings are the `reconcile` record's; the first, second and fourth are the tenant's, the third the instance's (§4.3):
>
> - the provider order;
> - the link properties for each provider;
> - the identifier properties used for inference;
> - normalizer overrides.

### A17. Composition is a pure function run after commit

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §2
- **Change:** extends §4
- **Summary:** The fused body of a cluster is composed by the resolver as a deterministic function of the contributing graphs' states, the policy record's offset and the cluster map version, recorded on the composed row as `composed_from`; it runs after the write that changed one of them, never inside its transaction: inline for the written entity, in the composition worker for the other members of a changed cluster and the overlays of tenants that hold one, on miss on the read path, and set-based in bootstrap and bulk modes. 0083's table said `amends`; the chapter text under §4 ([04](../architecture/04-entities-and-identifiers.md) §4.4) said nothing about when or where composition ran, so nothing is contradicted and this entry extends. (REVIEW G9)

### A18. `wbmergeitems` between a local and a mirrored item is a `same-as`

- **Date:** 2026-10-09
- **Source:** [0084](0084-wikibase-writes-against-the-resolved-view.md) §2
- **Change:** extends §9
- **Summary:** A Wikibase write is reduced to the local-graph operations by ownership of what it touches, and `wbmergeitems` between a local and a mirrored item is a `same-as`, never a copy; between two foreign entities it is a `redirect`. The chapter carries the module table at [18](../architecture/18-api.md) §3. (REVIEW G16)

### A19. Adapters never map onto a tenant's properties

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §6
- **Summary:** A mirror partition is an instance partition read by every tenant, a local `P12` means a different entity on each tenant, and an instance job cannot mint from a tenant's sequence; so no adapter writes a local property or item ID into a mirror graph, and none creates a local entity on import. A provider that needs vocabulary of its own has a provider **property type** in the registry, a `[[provider.type]]` row with `entity_type = "property"` under its code, its letter allocated when the adapter is built (OpenAlex already uses `P` for publishers; OpenStreetMap and GDELT likewise), with its properties minted by the adapter under deterministic IDs and the mapping versioned with the adapter. Enumerated values map onto Wikidata items where one fits, else onto a provider type of the provider's own. Where an adapter names a property by role, the binding it reads is the instance-scope `role` record's, never a tenant's ([0003](0003-statement-ui.md) A14). "Otherwise onto a local property", "the adapter can create them on first import" and "local properties that an adapter maps onto are protected from deletion" are struck. (REVIEW G26)

Replaced text ([04](../architecture/04-entities-and-identifiers.md) §4.6, as it stood):

> **OpenAlex properties are mapped by the adapter.** OpenAlex has no property entities. Its fields have names, not identifiers, and the set of fields is small and changes little. The OpenAlex adapter therefore maps each field:

> - otherwise onto a local property.

> There is no OpenAlex property namespace or type code.

> - **Enumerated values** such as work type are mapped in the same way, onto Wikidata items or local items.
> - **Local targets are named by role.** Local property and item IDs differ between instances. The adapter therefore refers to its local targets by role, as [0003](../decisions/0003-statement-ui.md) §7 does. Each instance binds the roles to its own entities, and the adapter can create them on first import.
> - **The mapping is versioned with the adapter** ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3).

> **Any graph may use any property.** The OpenAlex mirror graph holds statements whose predicates are `WDP` or local properties. A graph records who asserts a triple, not whose vocabulary the triple uses. Mapped properties are affected by changes as follows:

> - **Local properties** that an adapter maps onto are protected from deletion.

### A20. `unit`, `globe` and `calendarmodel` are prefixed entity IDs

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §7
- **Summary:** The `unit` of a quantity, the `globe` of a coordinate and the `calendarmodel` of a time are stored as prefixed entity IDs (`"unit": "WDQ11573"`, `"unit": "Q7"`, `"unit": "1"` unchanged for a dimensionless quantity), never as a concept IRI under some base; they are expanded through the current IRI template on output, the adapter rewrites them on ingest as it rewrites every other ID, the content hash is over the stored form, and they participate in `entity_ref` and in the response rewrite as entity values do. `wikibase-compat.md` §4 describes the serialization. The ledger also named [0003](0003-statement-ui.md) §7, which holds none of this text; the fold is at [04](../architecture/04-entities-and-identifiers.md) §4.7, and the ledger row was corrected. (REVIEW G21)

### A21. A normalizer change is a rebuild, not a rewrite

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §7
- **Summary:** The normalized keys are stored beside the values in `view.value_key`, and each row carries the version of the normalizer that produced it. Changing a normalizer, whether a data type's, a property's override or a resolver's, enqueues a per-property rebuild job that recomputes the keys of every value of the affected properties and bumps the rendering version of the update stream, since the normalized (`wdtn:`) triples change lexical form; until the job completes, rows of the old version are compared by the old key. The sitelink half of the ledger row is under [0026](0026-sitelinks.md) §5. (REVIEW G33)

### A22. Linking properties needs `ts-linkproperty`

- **Date:** 2026-10-09
- **Source:** [0016](0016-permissions-and-access-control.md) A32
- **Change:** amends §9
- **Summary:** A local property's `redirect` across namespaces (A4) needs `ts-linkproperty`, not `property-create`, which every registered account now holds by default; `equivalent-property` needs it too ([09](../architecture/09-security-and-moderation.md) §2.4). Both change predicates for every consumer, so the right stays with `propertycreator` and `sysop`.

Replaced text ([04](../architecture/04-entities-and-identifiers.md) §4.10, as it stood):

> It needs `property-create` and a matching data type.
