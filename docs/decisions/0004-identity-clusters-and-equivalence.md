# 0004. Identity clusters and equivalence

- **Status:** Proposed
- **Date:** 2026-09-25
- **Updated:** 2026-10-09 (A15)
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

*Changed by A3, A4, A14.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.3.*

### 4. The canonical ID

*Changed by A1, A4, A5, A12, A13.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.4.*

### 5. Conversion joins a cluster (amends 0002 §6)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.5.*

### 6. Properties

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.6.*

### 7. Values

*Changed by A4, A6.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.7.*

### 8. Fusing equivalent statements

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.8.*

### 9. Operations (amends 0002 §8.2)

*Changed by A4, A13.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.10.*

### 10. Conflicts

*Changed by A8.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.11.*

## Consequences

- **Canonical IRIs in the resolved view can change.** External consumers must follow `owl:sameAs`. Any change in membership re-resolves every entity that refers to any member. This needs a reverse index from each member to the statements that refer to it. Linking a heavily cited author can fan out to many works.
- **Clusters are cheap to maintain.** Their size is bounded by the number of namespaces, so a cluster is recomputed from its links whenever one changes. Retracting a link does not require an incremental union-find that supports deletion.
- **Conflicts are handled conservatively, and that can take data away.** A held conflict can detach statements an entity used to have. An example is the upstream-merge case in §5. The conflict is visible, but the entity's resolved view has lost those statements until it is resolved.
- **An OpenAlex mirror depends on Wikidata properties.** An instance that mirrors OpenAlex must import at least the mapped `WDP` properties, for example as a terms-only subset ([0002](0002-source-graphs-and-mass-ingest.md) §8.4). It must also bind the adapter's local roles.
- **Link coverage depends on upstream.** An OpenAlex work that no Wikidata item links to, and that no local assertion links, stays canonical under its `OAW` ID.
- **The two property forms in [0003](0003-statement-ui.md) §7 can be linked.** 0003 lets one role name both a local and a mirrored property. If the two are linked by `equivalent-property`, a role map can name either one.
- **Resolution costs more.** Every statement key requires normalizing values and looking up clusters, and this adds to the reconciliation cost noted in 0002.

## Open questions

- **Q1.** ~~**Provider order per entity type.** For example, whether a citation-focused instance could rank OpenAlex above Wikidata for works.~~ *Settled by A4: yes. The `reconcile` record ([0015](0015-record-format-and-partition-registry.md) §3) gains an optional `order_by_type` map (`work: [openalex, wikidata]`) that overrides the default order for clusters whose members are of that type; a cluster's members share a type by construction, so the order is well defined. Keyed members still rank first ([0009](0009-keyed-entity-types-and-domain.md) §8) and local members next.*
- **Q2.** ~~**Inferred links.** Which identifier properties should be eligible, and whether inferred links need review before they are applied.~~ *Settled by A4: a tier-3 link is applied at once, as §3 implies, for the identifier properties the `reconcile` record names; nothing is queued. The safeguards are the ones §3 already has, a value must be unique within each namespace and `different-from` beats every tier, plus the conflict list of §10 and the maintenance listing of [0002](0002-source-graphs-and-mass-ingest.md) §7, where an editor undoes a bad inference with `different-from`. Which properties are eligible is the tenant's `reconcile` configuration; the registry ships none enabled.* *Changed by A14: the identifier properties that drive inference are instance configuration, in the instance `reconcile` record; a tenant's `reconcile` may switch inference off for itself but not add inference properties.*
- **Q3.** ~~**Time precision.** Whether a less precise value that a more precise one contains, such as "2019" and "2019-03-04", should fuse or only be shown as corroborating it.~~ *Settled by A4: they do not fuse; §7 already makes precision part of the key. The statement UI ([0003](0003-statement-ui.md)) marks the coarser value as consistent with the finer one in the same group, and the resolved view keeps both, so the view never claims a precision no source asserted.*
- **Q4.** ~~**Retiring a local property.** When Wikidata later creates a property that a local property duplicates, `equivalent-property` keeps the local one canonical. It is open whether a redirect across namespaces should exist so that the local property can be retired in favor of Wikidata's.~~ *Settled by A4: for properties only, a local `redirect` may cross namespaces: `redirect P12 → WDP585` retires `P12`, makes `WDP585` the canonical predicate, rewrites the resolved view as §6 rewrites a linked pair, and emits `owl:equivalentProperty` for each predicate family with the redirect-form `owl:sameAs`; existing local statements keep `P12` in the log and read as `WDP585`. It needs `property-create` ([0016](0016-permissions-and-access-control.md) §2) and a matching data type. Items keep §2's rule: no redirect across namespaces, and a local item stays canonical.*
- **Q5. Sub-properties.** Whether `rdfs:subPropertyOf` should be recorded, and whether it should affect resolution.
- **Q6.** ~~**Permissions.** Who may assert `same-as` and `different-from`, run them in bulk, or resolve conflicts.~~ *Settled by [0016](0016-permissions-and-access-control.md) §5: `ts-link`, in bulk with `ts-runjob`; `equivalent-property` needs `property-create`.*
- **Q7.** ~~**A stable external IRI.** Whether to recommend the alias form `{base}/entity/<any member>` to external consumers as a reference that survives changes of canonical ID.~~ *Settled by A4: the recommended stable handle is the **document node** of any member, `{base}/wiki/Special:EntityData/{id}` ([0015](0015-record-format-and-partition-registry.md) §6), which exists for every member whatever minted its ID and never moves. The resolved view already carries `schema:about` from each document node to the current canonical concept IRI ([wikibase-compat.md](../api/wikibase-compat.md) §5.2), so a consumer that cites the document node reaches the concept through one triple that the update stream ([0032](0032-sparql-update-stream.md)) keeps current; concept IRIs stay canonical-and-changing as §4 says, and no new IRI kind enters the main graph. The dump and stream documentation says so; [0029](0029-resolver-namespaces.md) §4's `DOI:{key}` is the same recommendation for a thing known by an identifier.*
- **Q8.** ~~**Statement IDs after conversion.** Carried over from 0002.~~ *Settled by [0018](0018-tenants.md) §7: the resolved view rewrites the GUID's entity prefix to the canonical ID and keeps the UUID.*

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
