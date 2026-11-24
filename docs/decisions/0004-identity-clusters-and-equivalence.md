# 0004. Identity clusters and equivalence

- **Status:** Proposed
- **Date:** 2026-09-25
- **Updated:** 2026-11-23 (A29)
- **Author:** James Hare / Claude Opus
- **Uses:** [0000](0000-init.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

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

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.1.*

### 2. Clusters are strictly one-to-one

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.2.*

### 3. Where links come from

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.3.*

### 4. The canonical ID

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.4, §4.12.*

### 5. Conversion joins a cluster (amends 0002 §6)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.5.*

### 6. Properties

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.6.*

### 7. Values

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.7.*

### 8. Fusing equivalent statements

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.8.*

### 9. Operations (amends 0002 §8.2)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.10.*

### 10. Conflicts

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

- **Q1. Provider order per entity type.** *Settled by A4: yes. The `reconcile` record ([0015](0015-record-format-and-partition-registry.md) §3) gains an optional `order_by_type` map (`work: [openalex, wikidata]`) that overrides the default order for clusters whose members are of that type; a cluster's members share a type by construction, so the order is well defined. Keyed members still rank first ([0009](0009-keyed-entity-types-and-domain.md) §8) and local members next.* *Extended by A16.*
- **Q2. Inferred links.** *Settled by A4.* *Changed by A14.*
- **Q3. Time precision.** *Settled by A4.*
- **Q4. Retiring a local property.** *Settled by A4: for properties only, a local `redirect` may cross namespaces: `redirect P12 → WDP585` retires `P12`, makes `WDP585` the canonical predicate, rewrites the resolved view as §6 rewrites a linked pair, and emits `owl:equivalentProperty` for each predicate family with the redirect-form `owl:sameAs`; existing local statements keep `P12` in the log and read as `WDP585`. It needs `property-create` ([0016](0016-permissions-and-access-control.md) §2) and a matching data type. Items keep §2's rule: no redirect across namespaces, and a local item stays canonical.* *Extended by A16.*
- **Q5. Sub-properties.** Whether `rdfs:subPropertyOf` should be recorded, and whether it should affect resolution.
- **Q6. Permissions.** *Settled by [0016](0016-permissions-and-access-control.md) §5.*
- **Q7. A stable external IRI.** *Settled by A4: the recommended stable handle is the **document node** of any member, `{base}/wiki/Special:EntityData/{id}` ([0015](0015-record-format-and-partition-registry.md) §6), which exists for every member whatever minted its ID and never moves. The resolved view already carries `schema:about` from each document node to the current canonical concept IRI ([wikibase-compat.md](../api/wikibase-compat.md) §5.2), so a consumer that cites the document node reaches the concept through one triple that the update stream ([0032](0032-sparql-update-stream.md)) keeps current; concept IRIs stay canonical-and-changing as §4 says, and no new IRI kind enters the main graph. The dump and stream documentation says so; [0029](0029-resolver-namespaces.md) §4's `DOI:{key}` is the same recommendation for a thing known by an identifier.* *Extended by A16.*
- **Q8. Statement IDs after conversion.** *Settled by [0018](0018-tenants.md) §7.* *Extended by A16.*

## Amendment log

### A1. Keyed types rank first

- **Date:** 2026-09-26
- **Source:** 0009 §8
- **Change:** extends §1; amends §4
- **Summary:** A keyed type is a namespace of its own. Keyed members rank first in a cluster, ahead of local members.

Replaced text: §4 of this ADR.

### A2. Keywords never cluster

- **Date:** 2026-09-27
- **Source:** 0017 §5
- **Change:** extends §1
- **Summary:** `same-as` with a keyword on either side is rejected at write time, and no tier-2 or tier-3 link may produce one. A keyword may carry an ordinary statement pointing at the concept it usually denotes.

### A3. Tenants: links and overlays

- **Date:** 2026-09-27
- **Source:** 0018 §5–6
- **Change:** extends §3
- **Summary:** Tier-2 and tier-3 links come from shared graphs and are instance-wide; tier-1 links and `different-from` blocks are each tenant's own. A tenant's clusters are the shared clusters with its overlay applied. A provider tenant's `same-as` and `convert` records are tier-1 links for it and tier-2 links for everyone else. 0018 §7 also settled Q8.

### A4. Decisions in review

- **Date:** 2026-09-27
- **Source:** Direct: James, review of 2026-09-27
- **Change:** extends §3, §4, §7; amends §9
- **Summary:** For **properties only**, a local `redirect` may cross namespaces (`{"op":"redirect","from":"P12","to":"WDP585"}`), retiring the local property in favour of the mirrored one; see the open-questions note on retiring a local property. The `reconcile` record may carry `order_by_type` to rank providers differently per entity type. The same review settled Q1–Q4 and Q7: providers may be ordered per entity type; inferred links apply at once for the properties the `reconcile` record names; a coarser time does not fuse with a finer one; properties, not items, may redirect across namespaces; and the stable external handle is a member's document node.

Replaced text: §9 of this ADR.

### A5. Deleted members leave the cluster

- **Date:** 2026-09-27
- **Source:** 0023 §4
- **Change:** extends §4
- **Summary:** A deleted local entity leaves its identity cluster, as a member tombstoned under `orphan` does; the next member becomes canonical. Its `same-as` and `different-from` records are kept but not applied, and apply again on undelete.

### A6. The url normalizer

- **Date:** 2026-09-27
- **Source:** 0026 §1
- **Change:** extends §7
- **Summary:** The sitelink URL normalizer is the `url` data type's normalized key.

### A7. OpenStreetMap tags are not concepts

- **Date:** 2026-09-28
- **Source:** 0036 §6
- **Change:** extends §1
- **Summary:** `osm-tag` entities never join an identity cluster. 0048 later moved the rule to notations of every scheme (A9).

### A8. Conflict and linking forms

- **Date:** 2026-09-30
- **Source:** 0047 §5–6
- **Change:** extends §10
- **Summary:** The review list is `Special:IdentityConflicts`, with these resolutions as row actions. `Special:LinkEntities` is the form for `same-as`, `different-from` and `equivalent-property`; `Special:MergeItems` writes a merging `redirect` within one namespace and refuses a merge across namespaces.

### A9. Notations never cluster

- **Date:** 2026-10-01
- **Source:** 0048 §3
- **Change:** extends §1
- **Summary:** No notation of any scheme joins an identity cluster, as A7 decided for OpenStreetMap tags and A2 for keywords.

### A10. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** 0050 §13
- **Change:** consolidates §1–10
- **Summary:** A1–A9 were folded into the Decision and the open questions were numbered; the review decisions that settled Q1–Q4 and Q7 are A4. No decision changed. Before this, A4's §9 part and A8 were blockquotes, and the other entries were recorded only in this ADR's header, its open questions or other ADRs. The file before conversion is commit `0b26a3a`.

### A11. Lexeme parts

- **Date:** 2026-10-05
- **Source:** 0066 §4
- **Change:** extends §1
- **Summary:** Clusters hold lexemes; forms and senses never join one and keep their IDs across a cluster.

### A12. One member per namespace the tenant reads; provider-number order

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §1, §2, §4
- **Summary:** A cluster has at most one member per namespace the tenant reads: the registry's providers, the keyed types and the entity sources, not "the local instance, Wikidata or OpenAlex" and not "three today". The default provider order, after keyed and local members, is provider-number order in `providers.toml`, overridable by `reconcile`; "Wikidata, then OpenAlex" is no longer stated as the default. (PENDING A16)

Replaced text: §1 of this ADR; §2 of this ADR; §4 of this ADR.

### A13. `reconcile` is tenant configuration

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §4, §9
- **Summary:** The `reconcile` record is tenant configuration (0018 §3), not "an instance's configuration": the provider order, link properties and normalizer overrides of §9 are set per tenant. The inference properties are the exception, which A14 states. The §4 sentence this corrects is the one A12 also replaced ("Each instance sets the order of the providers after them."); it is quoted under A12. (PENDING A17)

Replaced text: §9 of this ADR.

### A14. Inference properties are instance configuration

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §3
- **Summary:** The identifier properties that drive tier-3 inference are **instance** configuration, named in the instance `reconcile` record, so tier-3 links stay instance-wide (§3's farm rule). A tenant's `reconcile` may switch inference off for itself and set its provider order and link properties, but may not add inference properties. 0015 §3 (`reconcile`) and 0018 §3 are amended the same way. Q2's settlement is extended accordingly. (PENDING B6)

Replaced text: §3 of this ADR.

### A15. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** 0050 §14
- **Change:** relocates §1–§10

### A16. Source form, rewriting in responses and the instance policy record

- **Date:** 2026-10-09
- **Source:** 0082 §1, §3, §4, §5, §5.1
- **Change:** amends §3, §4, §5, §6, §9
- **Summary:** The internal graph is in source form: the canonical ID is an attribute of the cluster, named by `view.cluster`, and never applied to data; statement subjects, entity values, `entity_ref`, `identifier`, the search documents, the dump and the stream keep the IDs their sources wrote, and nothing rewrites an ID in storage because a cluster formed, changed or dissolved. The form a consumer sees is a property of the response: a layer on the way out rewrites entity IDs, the entity part of statement GUIDs and `unit`, `globe` and `calendarmodel` to the consumer's preferred namespace order (`prefer`, defaulting to the form of the request; local for the site UI), using exact matches only (tier 1, tier 2 and `equivalent-property`, never tier-3 inference or anything a `different-from` split), after the cache, never inside literals; `wbgetentities` answers under the requested ID with a `canonical` field, and the Wikibase redirect shape is reserved for true redirects. So a converted `WDQ123` no longer "resolves to" `Q456` (§5); the two are one cluster, and a property `redirect` across namespaces makes the old ID an alias served under the new one (§9). The fused body treats an `equivalent-property` pair as one property, with stored predicates keeping their IDs (§6). 0082's table listed §3, §4 and §9; the chapter also folds §5 (04 §4.5) and §6 (04 §4.6), and the table was extended. Chapter text: 04 §4.3, §4.4, §4.5, §4.6, §4.10. (REVIEW G1, G2, G3, G5, G6)

Replaced text: 04 §4.3, §4.4.

Replaced text: 04 §4.5, §4.6, §4.10.

### A17. Composition is a pure function run after commit

- **Date:** 2026-10-09
- **Source:** 0083 §2
- **Change:** extends §4
- **Summary:** The fused body of a cluster is composed by the resolver as a deterministic function of the contributing graphs' states, the policy record's offset and the cluster map version, recorded on the composed row as `composed_from`; it runs after the write that changed one of them, never inside its transaction: inline for the written entity, in the composition worker for the other members of a changed cluster and the overlays of tenants that hold one, on miss on the read path, and set-based in bootstrap and bulk modes. 0083's table said `amends`; the chapter text under §4 (04 §4.4) said nothing about when or where composition ran, so nothing is contradicted and this entry extends. (REVIEW G9)

### A18. `wbmergeitems` between a local and a mirrored item is a `same-as`

- **Date:** 2026-10-09
- **Source:** 0084 §2
- **Change:** extends §9
- **Summary:** A Wikibase write is reduced to the local-graph operations by ownership of what it touches, and `wbmergeitems` between a local and a mirrored item is a `same-as`, never a copy; between two foreign entities it is a `redirect`. The chapter carries the module table at 18 §3. (REVIEW G16)

### A19. Adapters never map onto a tenant's properties

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §6
- **Summary:** A mirror partition is an instance partition read by every tenant, a local `P12` means a different entity on each tenant, and an instance job cannot mint from a tenant's sequence; so no adapter writes a local property or item ID into a mirror graph, and none creates a local entity on import. A provider that needs vocabulary of its own has a provider **property type** in the registry, a `[[provider.type]]` row with `entity_type = "property"` under its code, its letter allocated when the adapter is built (OpenAlex already uses `P` for publishers; OpenStreetMap and GDELT likewise), with its properties minted by the adapter under deterministic IDs and the mapping versioned with the adapter. Enumerated values map onto Wikidata items where one fits, else onto a provider type of the provider's own. Where an adapter names a property by role, the binding it reads is the instance-scope `role` record's, never a tenant's (0003 A14). "Otherwise onto a local property", "the adapter can create them on first import" and "local properties that an adapter maps onto are protected from deletion" are struck. (REVIEW G26)

Replaced text: 04 §4.6.

### A20. `unit`, `globe` and `calendarmodel` are prefixed entity IDs

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §7
- **Summary:** The `unit` of a quantity, the `globe` of a coordinate and the `calendarmodel` of a time are stored as prefixed entity IDs (`"unit": "WDQ11573"`, `"unit": "Q7"`, `"unit": "1"` unchanged for a dimensionless quantity), never as a concept IRI under some base; they are expanded through the current IRI template on output, the adapter rewrites them on ingest as it rewrites every other ID, the content hash is over the stored form, and they participate in `entity_ref` and in the response rewrite as entity values do. `wikibase-compat.md` §4 describes the serialization. The ledger also named 0003 §7, which holds none of this text; the fold is at 04 §4.7, and the ledger row was corrected. (REVIEW G21)

### A21. A normalizer change is a rebuild, not a rewrite

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §7
- **Summary:** The normalized keys are stored beside the values in `view.value_key`, and each row carries the version of the normalizer that produced it. Changing a normalizer, whether a data type's, a property's override or a resolver's, enqueues a per-property rebuild job that recomputes the keys of every value of the affected properties and bumps the rendering version of the update stream, since the normalized (`wdtn:`) triples change lexical form; until the job completes, rows of the old version are compared by the old key. The sitelink half of the ledger row is under 0026 §5. (REVIEW G33)

### A22. Linking properties needs `ts-linkproperty`

- **Date:** 2026-10-09
- **Source:** 0016 A32
- **Change:** amends §9
- **Summary:** A local property's `redirect` across namespaces (A4) needs `ts-linkproperty`, not `property-create`, which every registered account now holds by default; `equivalent-property` needs it too (09 §2.4). Both change predicates for every consumer, so the right stays with `propertycreator` and `sysop`.

Replaced text: 04 §4.10.

### A23. How value IRIs are stored, served and hashed

- **Date:** 2026-11-21
- **Source:** Direct: Claude, while building A20 for SCA-282 on James's go-ahead (2026-11-21, "You may proceed to the next thing"); confirmed by James, 2026-11-21
- **Change:** extends §7
- **Summary:** A20 stores a value's `unit`, `globe` and `calendarmodel` as the entity ID it names and expands it on output, but does not say how an IRI is read, what happens to one no template matches, or which hashes see the expanded form. The calls made: (1) The map is the provider registry's concept IRI templates, a type's `{iri before {upstream_id}}{upstream prefix}` read as `{code}{type letter}` (a source's as `{name}:{upstream prefix}`), with the upstream ID's digits after either, and the tenant's `{base}/entity/` for its own IDs, checked first: a tenant whose base is a registered provider's template is that provider's own wiki, and its IRIs are its local IDs. Key-mapped types are not mapped. (2) An IRI no template matches is kept as written and served as it is, and so is a value stored before A20 was built; existing records are not converted, and a mirror written before is loaded again, as 0015 A64 has it. (3) The adapter stores them on ingest, and in adoption mode the source's own IRIs become local IDs. A local edit through the Action API stores them before it is compared with the stored entity, since the client writes IRIs; the bulk batch job of 0002 §8.5, which no surface runs yet, must do the same when one does. (4) Wherever a hash reaches a client or is matched against one a client sends, it is taken over the expanded form, as Wikibase takes it: the served JSON, the RDF value nodes, the hashes a claim or reference edit names, and the ingest guard against upstream's hashes. Hashes no client sees (a deterministic statement ID, the fusing of statements, a diff) are taken over the stored form. (5) A value's unit, globe or calendar model names its entity for the `fetch_queue` projection as an entity value does (0070 A10), so a linked unit is mirrored and labelled. Chapter text: 04 §4.7.

Replaced text: none; 04 §4.7 gains a paragraph.

### A24. Tier-1 clusters, and which links a conflict holds

- **Date:** 2026-11-23
- **Source:** Direct: James, 2026-11-23, asking for SCA-244 to be built after SCA-460. How it is built, Claude, while building SCA-244's first part, 2026-11-23; call (1) confirmed by James, 2026-11-23, the rest unconfirmed
- **Change:** extends §2, §3, §10
- **Summary:** The first clusters are a tenant's own: tier-1 links (`same-as`, and `convert` as the link it makes) and `different-from` from its graphs that are not mirrors, projected into `view.link`, `view.different_from`, `view.cluster` and `view.cluster_member` by the pure crate `scatter-identity`. The calls made: (1) **Which links a conflict holds.** The links of one tier are grouped by the would-be cluster they join, on top of what the higher tiers formed; a group that would hold two members of one namespace, or two IDs a `different-from` keeps apart, is held whole, every link of the group, not only those on the path between the two members. So `Q6 = WDQ1`, `WDQ1 = OAW1` and `OAW1 = WDQ2` are all held, and `Q6` and `WDQ1` stay apart until the conflict is resolved. This is the widest reading of "the links involved" (§2); it keeps the result independent of the order links arrive in, and never applies a link the conflict might be about. (2) A cluster's `cluster_id` is a stable 63-bit hash of its canonical ID, and a conflict's of the smallest ID its links name, so that a rebuild gives the same numbers. (3) `view.link` and `view.different_from` carry the `partition` and `offset` of the record that wrote a local link, so that a reset or an erasure removes it and the clusters around it are recomputed. (4) The interim `view.same_as` of 0078 A10 is dropped; the resolution of an entity source's ID finds the local item minted for it as a member of its cluster. Not built yet: tiers 2 and 3 and the shared rows, `equivalent-property`, aliases, members that are deleted or tombstoned leaving a cluster, `view.entity`'s `cluster_id` and `canonical_id`, and the fused body and rewriting in responses (§4, §8). Chapter text: 04 §4.2; 03 §4.4; 05 §6.5.

Replaced text: in 04 §4.2, "The links involved are held, not applied."; in 03 §4.4, the paragraph and DDL of `view.same_as`; in 05 §6.5, "Until clusters are projected, a local item the tenant's own `same-as` links to `mhc:Q1` is found through `view.same_as` instead (03 §4.4; 0078 A10)."

### A25. A cluster's members read as its fused body

- **Date:** 2026-11-23
- **Source:** Direct: James, 2026-11-23, asking for SCA-244's next part on the same branch. How it is built, Claude, while building it, 2026-11-23; unconfirmed
- **Change:** extends §4, §8
- **Summary:** A member of a tenant's cluster is resolved from every member's graphs, and so is read through the API and the site as the cluster's fused body. The calls made: (1) **Every member's row holds the fused body**, under the member's own ID, with the cluster's `cluster_id` and `canonical_id`; it is composed once for each member rather than once for the cluster, since each member's row keeps its own page ID, revision and local offset. A foreign member gets a tenant row of its own, as an entity the tenant asserts something about does. (2) **Graph order across members:** the local graphs first, the canonical member's before the others', then the mirrors and entity sources in provider order, members of equal rank in member order. Every member's local corrections apply. (3) A member's page ID, `lastrevid` and base revision stay its own, and a member no graph holds reads as nothing; its own local state is what a write takes as local. (4) Resolving one member resolves the others on the tenant, and a link, or its removal, resolves every entity it reaches; nothing else is fanned out. (5) Statement GUIDs stay in source form (`XDQ10$…` under `Q6`) until responses are rewritten (§4.9, §4.12); terms and identifiers are written for each member, so a resolver finds the canonical member, and search may list several members until it collapses clusters. Not built yet: rewriting in responses and the `canonical` field, RDF's `owl:sameAs`, search's collapsing, and resolving again what an erased link reached. Chapter text: 04 §4.4.

Replaced text: none; 04 §4.4 gains a paragraph.

### A26. The first rewriting of responses

- **Date:** 2026-11-23
- **Source:** Direct: James, 2026-11-23 ("Commit, push, move on to the next part"), for SCA-244. How it is built, Claude, while building it, 2026-11-23; unconfirmed
- **Change:** extends §4
- **Summary:** A member of a cluster is served as §4.9 and §4.12 say, in part. The calls made: (1) **Statement GUIDs.** A fused statement whose GUID's subject is another member of the cluster is served under the member asked for, its UUID kept; any other GUID is served as stored. The provenance keeps the GUIDs as stored, and RDF names a mirrored statement after the member that holds it, as its upstream does. (2) **`canonical`** is written beside the entity's JSON wherever an entity is served as Wikibase JSON (`wbgetentities`, `Special:EntityData`'s JSON) for a member that is not its cluster's canonical member. (3) **`prefer`** is read by those two, a list of namespace names (`local`, a provider code, a keyed type's or an entity source's name) compared without case; without it, the namespace of the ID asked for. It rewrites entity values in main snaks, qualifiers and references; a rewritten value's snak hash is recomputed over the value served, since upstream's was of the stored value. Not built yet: `unit`, `globe` and `calendarmodel`, property IDs (which wait for `equivalent-property`), the REST routes, the site's page bundle, search and SPARQL bindings, and a write that names another member's statement by the GUID it was served under. Chapter text: 04 §4.12.

Replaced text: none; 04 §4.12 gains a paragraph.

### A27. Clusters in search, on the site, across properties, and through deletion

- **Date:** 2026-11-23
- **Source:** Direct: James, 2026-11-23 ("You may work on the remaining tasks for SCA 244"). How it is built, Claude, while building it, 2026-11-23; unconfirmed
- **Change:** extends §4, §6, §7, §8
- **Summary:** The calls made: (1) **A member is indexed by its own graphs.** Its terms, identifiers and references are written from what its own graphs say, not from the fused body, so a search finds either member by its own label, and a reference is counted once, by the member that makes it; this replaces A25 (5)'s writing of the fused body's. (2) **Search** reads a shared row of `view.entity` only where the tenant has no row of its own, as the resolver lookup does, so that a cluster collapses to one hit, its canonical member. (3) **The page bundle** takes `prefer` as `wbgetentities` does, `|` or `,` between names, and the site asks for `local`. A response is tagged with the entities its values and properties name, at most 200, so that a cluster formed or dissolved around one purges it. (4) **Property IDs** are rewritten with values: a statement, qualifier or reference snak whose property is rewritten moves to that property's group. In the fused body itself a statement stays grouped under the property it was stored with. (5) **Resolution compares by cluster:** a property or entity value that is a non-canonical member of one of the tenant's clusters is compared as its canonical member, so statements that say the same thing through two members fuse (`P4 = "x"` and `XDP4 = "x"` once `P4` and `XDP4` are equivalent properties). `read::current()` resolves live, so reads see the clusters as they stand; a stored row of an entity that merely refers to a member is not resolved again when the cluster changes (§4). The bulk path for local-only entities compares by ID alone. (6) **`equivalent-property`** is a tier-1 link of the identity projection, as `same-as` is. (7) **A deleted member leaves its cluster:** the identity projection reads entity ACLs, a link touching a deleted entity is kept with `status = 'inactive'` and left out of the clusters, and undeleting it applies the link again; the entities the change reaches are resolved again. Not built yet: tiers 2 and 3 and the shared rows, which wait on the `reconcile` record's form; aliases, since no projection holds an entity redirect's target yet; a tombstoned member leaving its cluster; `unit`, `globe` and `calendarmodel`, search and SPARQL in the rewrite. Chapter text: 04 §4.4, §4.6, §4.7; 03 §4.4; 18 §1.4.

Replaced text: in 03 §4.4, "status text NOT NULL,                         -- applied | held"; A25 (5), as above.

### A28. Mirror-derived links and the shared clusters

- **Date:** 2026-11-24
- **Source:** Direct: James, 2026-11-24, choosing the form of the link properties: `links` in the instance `reconcile:default` record, a map from a provider's code to a list of `{property, target}`, with no links on by default. How it is built, Claude, while building it, 2026-11-24; unconfirmed
- **Change:** extends §3, §9
- **Summary:** Tier 2 is built. **The record:** `"links": {"WD": [{"property": "P10283", "target": "OA"}]}` in `reconcile:default`; the property is the provider's own ID (or its stored form, `WDP10283`), and a value of it names the target provider's entity as the target's code followed by the value (`W123` as `OAW123`). The registry ships no record, so no links are on. The calls made: (1) A statement of deprecated rank makes no link, and neither does a value that is not an ID of the target provider. (2) **The links are shared rows** of `view.link`, tier 2, naming the statement in `source` and its entity in a new `subject` column; a mirror's new state of an entity replaces the links its statements make, and its tombstone or upstream redirect removes them. (3) **The shared clusters** (`tenant = ''`) are made of the shared links alone; **a tenant's** of the shared links and its own, so a tenant's `same-as` builds on a shared cluster in its own view without changing the shared one. A tenant's own cluster rows decide an ID that one of its links, blocks or clusters touches; the shared rows decide the rest. A shared link's status is the shared view's; whether a tenant's view holds it shows only in that tenant's clusters. (4) A change to the shared links recomputes the shared clusters around it and those of every tenant that reaches them, and resolves again every entity it reaches, shared and in each tenant that holds a row of it; the bulk path for a mirror's entities leaves out any entity in a cluster. (5) The record is read once for each block of a mirror's records, and nothing more is read for a provider it names no properties for; a change to the record applies to each entity's next state, or to all at a rebuild. Not built yet: inferred links (tier 3), a tenant's `reconcile` record (its own sources' link properties, declining inference), and the record's provider order and `order_by_type`, which still default. Chapter text: 04 §4.3; 23 §2.3; 03 §4.4.

Replaced text: none; the three sections gain the paragraphs above.

### A29. Inferred links, the two records, retired IDs and what is left of the rewrite

- **Date:** 2026-11-23
- **Source:** Direct: James, 2026-11-23 ("Let's bring SCA-244 to completion"). How it is built, Claude, while building the rest of SCA-244, 2026-11-23; confirmed by James, 2026-11-23: the field names of both records and a link's targets, that tier 3 links only shared mirrors' entities, that a statement GUID is served under the member asked for in a cluster an inferred link joins, and that a change to the instance's record reaches existing rows at their next state or a rebuild; the rest unconfirmed
- **Change:** extends §1, §3, §4, §7, §9
- **Summary:** Tier 3, the rest of the instance's record, a tenant's record, aliases, upstream deletions and the last parts of the rewrite are built. The calls made: (1) **The instance record's other fields** are `order` (a list of provider codes, highest first), `order_by_type` (an entity type's name to such a list) and `infer` (a list of identifier properties as stored, `WDP356`); the order decides a cluster's canonical member and the order of an entity's mirror graphs in resolution, and is read when a cluster is recomputed or an entity resolved, so a change to it reaches existing rows as they next change, or all at a rebuild. (2) **A tenant's record** is `reconcile:default` in its own `config`: `links` keyed by the name of one of its entity sources, each entry's `property` the source's own and its `target` a provider code, `local` or another source's name, the links being the tenant's rows of `view.link`; `order`, a list of its source names; and `infer`, which only `false` changes. Anything else (a provider in `order` or `links`, `order_by_type`, a list in `infer`) is refused with `ts-instance-policy` and nothing is written. (3) **The record is written by `triplespace-cli reconcile set|unset|show`**, an instance act needing `ts-config`, for the instance or with `--tenant`; no API route writes it yet. (4) **Tier 3** reads the normalized keys of external-id values, as `view.identifier` holds them for shared rows, for the properties in `infer`. The holders of a value are grouped by namespace; each namespace with exactly one holder is eligible, and every eligible holder is linked to the least of them, as shared rows of tier 3 whose `source` is `{property}={value key}`. It is worked out in the identity step from the record's state, with the block's entities' values kept in memory, since the entity projection writes `view.identifier` afterwards. Only shared mirrors' entities are linked this way, never a tenant's local ones. (5) **A cluster an inferred link joins is marked `inferred`.** Its members read as one fused body, and a statement GUID is served under the member asked for as in any cluster, but a value, property or unit is rewritten only to a member the links of tiers 1 and 2 connect it to. (6) **A tenant that declines inferred links** has clusters of its own, without them, for every ID in a shared cluster an inferred link joins, and a row of its own in `view.entity` for each such member; writing or changing its record recomputes these at once. (7) **Retired IDs** are a new table, `view.retired`: an ID a redirect retired (upstream's in a mirror, a local `redirect`, an adopted one) with its target, and an entity upstream tombstoned with none. An alias is resolved to its target, through a chain, when clusters are computed, so a link naming it is a link to the target and the alias is no member; a later state of the ID takes it out of the table. Serving a request for an alias as a redirect is not part of this. (8) **A tombstoned member leaves** as a deleted one does, its links `inactive`, unless the tenant's local graph sets `retain` on it or sets nothing and the tenant's `retention.default` is `retain`; the shared view keeps none. Applying the retention policy itself (0002 §5) is not built. (9) **A tenant that deprecates or suppresses the mirrored statement a shared tier-2 link comes from** declines that link: a row of its own in `view.link` with the link's key and `status = 'declined'`, removed when the correction no longer holds. (10) A link value must be an ID in the target's own grammar. (11) **`unit`, `globe` and `calendarmodel`** are rewritten with entity values, and `wbsearchentities` takes `prefer` and names each hit by the exact match it selects. (12) **A tenant has cluster rows of its own only where something of its own touches a member**: a link or declined link, a block, a retired ID, an entity it retains, or its declining of inferred links. A cluster made of shared links alone is read from the shared rows, so a rebuild writes the rows the records wrote one at a time. Not built: the rewrite of SPARQL result bindings, since no `/sparql` route is served yet; the REST entity routes other than the page bundle; a write that names another member's statement by the GUID it was served under; `owl:sameAs` in RDF; an API route for the record. Chapter text: 04 §4.1, §4.3, §4.4, §4.10, §4.12; 03 §4.4; 23 §2.3.

Replaced text: none; the sections gain the paragraphs above.
