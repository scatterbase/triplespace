# 0004. Identity clusters and equivalence

- **Status:** Proposed
- **Date:** 2026-09-25
- **Updated:** 2026-10-01 (A10)
- **Author:** James Hare / Claude Opus
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md)
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

*Changed by A1, A2, A7, A9.*

IDs that denote the same thing form an **identity cluster**. Each member of a cluster belongs to a **namespace**, meaning whoever minted the ID: the local instance, Wikidata or OpenAlex. The namespace is the provider, not the type code. A Wikidata item can share a cluster with an OpenAlex work (`OAW`) or an OpenAlex author (`OAA`).

**Aliases are not members.** An ID retired by a redirect is an alias of its target. This covers an upstream merge recorded in a mirror graph and a local merge. Aliases are resolved to their targets before clusters are built.

**A keyed type is a namespace of its own** ([0009](0009-keyed-entity-types-and-domain.md) §8). Its keys are minted by the thing itself, and a cluster holds at most one member from it.

**Keyed entities that are names never join a cluster.** Keywords ([0017](0017-entity-id-grammar.md) §5) and notations ([0048](0048-notation.md) §3, and the OpenStreetMap tags they replaced, [0036](0036-openstreetmap-providers.md) §6) are phrases, not concepts. `same-as` with one on either side is rejected at write time, and no tier-2 or tier-3 link may produce one.

### 2. Clusters are strictly one-to-one

**A cluster holds at most one member from each namespace.** A set of links that would put two members from one namespace into a cluster is a conflict (§10). The links involved are held, not applied. For example:

- `WDQ1 = OAW1` and `OAW1 = WDQ2` would join two Wikidata items.
- Both links are held, so `WDQ1`, `OAW1` and `WDQ2` stay out of each other's clusters until the conflict is resolved.

Holding every conflicting link, not only the later one, makes the result independent of the order in which links arrive.

**Duplicates within one namespace are merges.** Two IDs from the same provider that denote one thing are a duplicate within that provider. That is expressed by a `redirect`, an explicit merge ([0002](0002-source-graphs-and-mass-ingest.md) §7). It is never inferred from links.

**Clusters stay small.** A cluster can hold no more members than there are namespaces, which is three today. Chains of links cannot grow, and a cluster can be recomputed from its links whenever one of them changes.

### 3. Where links come from

*Changed by A3, A4.*

Links come from three tiers of source. A higher tier beats a lower one when they conflict. Conflicting links within one tier are all held (§2).

| Tier | Source | Example | Default |
|---|---|---|---|
| 1. Asserted | A local-graph `same-as` (§9) | An editor links `Q456` and `OAW123` | Always on |
| 2. Mirror-derived | A mirrored statement of a configured link property | Wikidata's P10283 (OpenAlex ID) on a Wikidata item; OpenAlex's `ids.wikidata`, on the entity types that carry it | On for configured properties |
| 3. Inferred | Two entities from different namespaces share a value of a configured identifier property | Both carry the same DOI | Off |

**A mirror-derived link lasts only as long as its statement.** It disappears when:

- the mirror drops the statement;
- the entity is tombstoned;
- the local graph deprecates or suppresses the statement.

A link value that points at an alias follows the redirect. If Wikidata says an item's OpenAlex ID is `W1` and OpenAlex has merged `W1` into `W2`, the link goes to `OAW2`.

**Inferred links need a unique value.** A value that appears on more than one entity in a namespace produces no link. Values are compared by their normalized keys (§7). An inferred link is applied at once, for the identifier properties the tenant's `reconcile` record names; nothing is queued for review, and the registry ships none enabled.

**On a farm,** tier-2 and tier-3 links come from shared graphs and are instance-wide. Tier-1 links and `different-from` blocks are each tenant's own. A provider tenant's `same-as` and `convert` records are tier-2 links for every other tenant ([0018](0018-tenants.md) §5–6).

**A `different-from` block beats links from every tier.** Two IDs joined by a local `different-from` never share a cluster. Wikidata's P1889 ("different from") needs no special handling. It relates two Wikidata items, which can never share a cluster anyway.

### 4. The canonical ID

*Changed by A1, A4, A5.*

**The canonical ID of a cluster is its highest-ranked member.**

- A keyed member ranks first ([0009](0009-keyed-entity-types-and-domain.md) §8), and a local member next.
- Each instance sets the order of the providers after them. The default is Wikidata, then OpenAlex. The `reconcile` record ([0015](0015-record-format-and-partition-registry.md) §3) may set a different order per entity type, `order_by_type`; a cluster's members share a type, so the order is always defined.

**The canonical ID is chosen when the resolved view is projected.**

- Source graphs keep IDs exactly as they were asserted.
- The resolved view ([0002](0002-source-graphs-and-mass-ingest.md) §3) rewrites subjects and entity values to the canonical ID.
- Each non-canonical member is emitted in Wikibase's redirect form, `<member IRI> owl:sameAs <canonical IRI>`.
- The API resolves a non-canonical member to the canonical entity, as Wikibase resolves a redirect.
- The per-graph views ([0003](0003-statement-ui.md) §6) still show each member's own data.

**The canonical ID changes when membership changes.** Examples:

- A Wikidata item is created for an OpenAlex work, and the new item becomes canonical.
- A local member is minted (§5) and becomes canonical.
- The canonical member is tombstoned under `cascade` or `orphan`. It leaves the cluster, and the next member becomes canonical. A retained member stays in the cluster.
- A local member is deleted. It leaves the cluster as a member tombstoned under `orphan` does. `same-as` and `different-from` records naming it are kept but not applied, and apply again if it is undeleted ([0023](0023-moderation.md) §4).

**The stable handle is the document node.** External consumers are told to cite the document node of any member, `{base}/wiki/Special:EntityData/{id}` ([0015](0015-record-format-and-partition-registry.md) §6), which exists for every member and never moves. The resolved view carries `schema:about` from each document node to the current canonical concept IRI, and the update stream ([0032](0032-sparql-update-stream.md)) keeps that triple current.

### 5. Conversion joins a cluster (amends 0002 §6)

`convert` mints a local entity and links it to the foreign entity with `same-as`. The local member is canonical, so the behavior 0002 §6 specifies follows from the cluster rules:

- `WDQ123` resolves to `Q456`;
- mirrored statements under `WDQ123` attach to `Q456`;
- the redirect-form `owl:sameAs` is emitted.

**This settles 0002 Q3, a local alias that conflicts with an upstream redirect.** Suppose `WDQ123` has been converted to `Q456`, and Wikidata then merges `Q123` into `Q789`.

- `WDQ123` becomes an alias of `WDQ789`, so `WDQ789` takes its place as the cluster's Wikidata member.
- If `WDQ789` already belongs to another cluster, the merge would join the two clusters. One-to-one applies.
- For example, if `WDQ789` is already linked to `Q999`, the joined cluster would have two local members. The links are held as a conflict.
- The editor resolves it with a local `redirect` from `Q456` to `Q999`, or the reverse.

### 6. Properties

**Wikidata properties are used as they are,** as mirrored `WDP` entities.

**OpenAlex properties are mapped by the adapter.** OpenAlex has no property entities. Its fields have names, not identifiers, and the set of fields is small and changes little. The OpenAlex adapter therefore maps each field:

- onto a Wikidata property, where one fits;
- otherwise onto a local property.

There is no OpenAlex property namespace or type code.

- **Enumerated values** such as work type are mapped in the same way, onto Wikidata items or local items.
- **Local targets are named by role.** Local property and item IDs differ between instances. The adapter therefore refers to its local targets by role, as [0003](0003-statement-ui.md) §7 does. Each instance binds the roles to its own entities, and the adapter can create them on first import.
- **The mapping is versioned with the adapter** ([0002](0002-source-graphs-and-mass-ingest.md) §8.3).

**Any graph may use any property.** The OpenAlex mirror graph holds statements whose predicates are `WDP` or local properties. This settles part of 0002 Q6, mirrored properties in local assertions. A graph records who asserts a triple, not whose vocabulary the triple uses. Mapped properties are affected by changes as follows:

- **Local properties** that an adapter maps onto are protected from deletion.
- **Wikidata properties** can change upstream. If one that an adapter maps onto is deleted upstream, or its data type changes, that mapping stops and the change is reported. The mapping then has to be revised.

**Properties form clusters as items do.** A local property may duplicate a Wikidata property, for example because it was created before Wikidata had one. A local `equivalent-property` (§9) links the two. Property clusters follow the same rules as item clusters:

- They are one-to-one.
- The local property is canonical.
- `different-from` blocks a link.

Property links come only from local assertions. Wikidata's P1628 ("equivalent property") points at external vocabularies, not at entities this instance holds, so it is ordinary data.

- **Data types must match.** A link between properties with different data types is rejected when it is written.
- **The resolved view uses the canonical property's predicates** for every statement in the cluster.
- **RDF output:** property entity IRIs get the redirect-form `owl:sameAs`, as items do. OWL equivalence holds between predicates, not between entity IRIs. So `owl:equivalentProperty` is emitted for each pair of predicates in the same family: `wdt:`, `p:`, `ps:`, `psv:`, `pq:`, `pqv:`, `pr:`, `prv:`, and the normalized forms.

### 7. Values

*Changed by A4, A6.*

The resolved view compares values by a canonical form:

- **Entity values** compare by the canonical ID of their cluster.
- **Literal values** compare by a **normalized key**, derived per data type. A property can override its data type's normalizer.

| Data type | Normalized key |
|---|---|
| `external-id` | Per property. For example, a DOI is lowercased, and an ORCID loses its URL prefix. The default is the exact value. |
| `string` | Unicode NFC |
| `monolingualtext` | Unicode NFC, together with the language code |
| `time` | The value, precision and calendar model. Values with different precisions are different values. |
| `quantity` | The amount, unit and bounds |
| `url` | The normalized URL that sitelinks also use ([0026](0026-sitelinks.md) §1) |
| Everything else | The exact value |

Stored values are never changed. A fused statement (§8) shows the value asserted by the highest-ranked graph.

**A coarser time does not fuse with a finer one.** "2019" and "2019-03-04" stay two values. The statement UI marks the coarser one as consistent with the finer one in the same group, and the resolved view keeps both, so it never claims a precision no source asserted.

### 8. Fusing equivalent statements

This extends the statement hash in [0002](0002-source-graphs-and-mass-ingest.md) §8.5. A statement's **key** is a hash of two things, both in canonical form (§4, §6, §7):

- the main snak;
- the qualifiers, in any order.

In the resolved view, statements with the same key fuse into one statement. This applies both across graphs and within a graph. Source graphs are not changed.

**References** are combined, with duplicates removed by the reference hash. Reference snaks are put into canonical form before hashing.

**Rank:**

- The local graph wins, as 0002 §3 already says.
- Otherwise the rank comes from the highest-ranked mirror graph, in the same provider order as §4.

**Statement ID:**

- The fused statement takes its ID and IRI from the highest-ranked graph that asserts it.
- The provenance response ([0003](0003-statement-ui.md) §6) lists every member statement and its graph.

**Overrides:**

- A rank override on any member statement applies to the fused statement.
- A suppression removes only one member's contribution. To remove the fused statement, every member is suppressed, which is what the UI does.

**Non-equivalent values** are kept side by side, as 0002 §3 already says.

### 9. Operations (amends 0002 §8.2)

*Changed by A4.*

| Operation | Graph | Meaning |
|---|---|---|
| `same-as` | Local | Links two items from different namespaces (§3, tier 1). |
| `different-from` | Local | Blocks two IDs from ever sharing a cluster (§3). |
| `equivalent-property` | Local | Links two properties from different namespaces (§6). |

`remove` retracts any of these.

`redirect` in the local graph records a merge within one namespace. The same-as meaning that 0002 §8.2 gave it moves to `same-as`. **For properties only, a local `redirect` may cross namespaces:** `{"op":"redirect","from":"P12","to":"WDP585"}` retires `P12` and makes `WDP585` the canonical predicate. The resolved view is rewritten as §6 rewrites a linked pair, and existing local statements keep `P12` in the log and read as `WDP585`. It needs `property-create` and a matching data type. An item never redirects across namespaces, and a local item stays canonical.

```
{"op":"same-as","ids":["Q456","OAW123"]}
{"op":"different-from","ids":["WDQ1","OAW7"]}
{"op":"equivalent-property","ids":["P12","WDP585"]}
```

**Configuration is recorded in the log.** The following settings are part of an instance's configuration:

- the provider order;
- the link properties for each provider;
- the identifier properties used for inference;
- normalizer overrides.

A change to any of them is appended to the log as a record, so the resolved view stays rebuildable from the log ([0000](0000-init.md) §1).

### 10. Conflicts

*Changed by A8.*

A **conflict** is a set of links that is held instead of applied. Conflicts arise from:

- a one-to-one violation, whether from links or from an upstream redirect that joins two clusters;
- a link blocked by `different-from`.

Conflicts are listed for review, in the same way as the maintenance tooling for local corrections ([0002](0002-source-graphs-and-mass-ingest.md) §7). An editor resolves a conflict in one of these ways:

- with a local `same-as`, which outranks mirror-derived links;
- with `different-from`;
- with a `redirect` that merges a duplicate;
- by deprecating the mirrored statement a link came from.

**The forms.** The review list is `Special:IdentityConflicts`, with these resolutions as row actions. `Special:LinkEntities` is the form for `same-as`, `different-from` and `equivalent-property`; `Special:MergeItems` writes a merging `redirect` within one namespace and refuses a merge across namespaces. ([0047](0047-special-pages.md) §5–6)

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
- **Q2.** ~~**Inferred links.** Which identifier properties should be eligible, and whether inferred links need review before they are applied.~~ *Settled by A4: a tier-3 link is applied at once, as §3 implies, for the identifier properties the `reconcile` record names; nothing is queued. The safeguards are the ones §3 already has, a value must be unique within each namespace and `different-from` beats every tier, plus the conflict list of §10 and the maintenance listing of [0002](0002-source-graphs-and-mass-ingest.md) §7, where an editor undoes a bad inference with `different-from`. Which properties are eligible is the tenant's `reconcile` configuration; the registry ships none enabled.*
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
