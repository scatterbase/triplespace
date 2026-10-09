# 0002. Source graphs and mass ingest

- **Status:** Proposed
- **Date:** 2026-09-24
- **Updated:** 2026-10-09 (A30)
- **Author:** James Hare / Claude Opus
- **Changes:** [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md)
- **Uses:** [Wikibase data model and ontology contract](../api/wikibase-compat.md)
- **Chapters:** [05](../architecture/05-providers-and-ingest.md)

## Context

[0000](0000-init.md) makes bulk ingest a primary workload and lets foreign entities sit alongside local ones. It leaves open what a foreign entity holds, how removal works, and how granular log records are.

There is no existing bulk API to stay compatible with. The ingest API is designed for two workloads:

1. **Mass import of external graphs.** Examples are Wikidata (over 100 million entities, published as JSON dumps) and OpenAlex (hundreds of millions of works, published as a JSONL snapshot partitioned by update date). Upstream owns both the identity and the content of these entities. It publishes periodic snapshots, and it merges, redirects and deletes entities on its own schedule.
2. **Bulk creation of entities local to the instance,** such as citation records. Local entities need IDs allocated, references between new entities within a batch, re-runs that do not create duplicates, and coexistence with human editors.

Four scenarios were used to test the design:

| | Scenario |
|---|---|
| **A** | Wikidata is mirrored faithfully. An entity is deleted on Wikidata, and the local copy is erased as well. |
| **B** | Wikidata plans to delete a large set of items. An instance that mirrors Wikidata wants to become their new home. Millions of mirrored items are declared local in one event. Their data does not change, but later deletions on Wikidata no longer affect them. |
| **C** | An editor corrects an error in a mirrored OpenAlex entity. |
| **D** | An editor adds instance-specific properties, which do not exist on Wikidata, to mirrored Wikidata items. |

## Decision

### 1. Where an ID was minted is separate from who asserts a triple

*Current text: [05](../architecture/05-providers-and-ingest.md) §1.1.*

### 2. Source graphs

*Changed by A22.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §1.2, §1.3, §8.2, §8.4.*

### 3. The main graph becomes a resolved view (amends 0001 §2)

*Changed by A13, A22.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §1.4.*

### 4. IRIs for foreign entities

*Changed by A2, A7, A8, A18, A28.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §1.5, §8.1, §8.2, §8.3, §8.4, §8.5.*

### 5. Upstream deletion and retention (scenarios A and B)

*Changed by A2, A3, A6, A23, A29.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §2.1, §2.2.*

### 6. Converting a foreign entity into a local one

*Changed by A1, A10.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §2.3, §2.4.*

### 7. Corrections and extensions (scenarios C and D)

*Changed by A16, A20.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §2.5.*

### 8. Ingest API

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.1.*

#### 8.1 One change-set format, three entry points

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.1.*

#### 8.2 Operations

*Changed by A1, A4, A15, A19, A24, A25.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.2.*

#### 8.3 Import jobs

*Changed by A3, A6, A9, A27.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.3.*

#### 8.4 Foreign imports

*Changed by A11, A14, A21.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.4, §8.4.*

#### 8.5 Local bulk creation

*Changed by A12, A15, A19.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.5.*

#### 8.6 Throughput

*Changed by A5.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.7.*

#### 8.7 Wire format sketch

*Changed by A26.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.9.*

### 9. How the scenarios resolve

*Current text: [05](../architecture/05-providers-and-ingest.md) §1.6.*

## Consequences

- ~~**The log must be partitioned by graph, and mirror partitions must support compaction.** This constrains the choice of storage engine, which is still open in 0000.~~ *The constraint holds, and the engine is chosen: Postgres, with the log list-partitioned by log partition and compaction a batched delete ([0013](0013-postgres-storage.md) §2–3; 0000 Q6).*
- **Full history of mirrored data costs storage.** Keeping every revision of a full Wikidata mirror means billions of revisions, so `latest` is the practical default there. Full history is kept for retained entities, and elsewhere only where an instance opts in. ~~How affordable full history is depends on log granularity, which is still open in 0000: statement-level change sets are much smaller than whole-entity revisions.~~ *Granularity is settled (0000 Q2, §8 here): a mirror receives whole-entity `put`s.*
- **Rescue is time-sensitive.** Upstream history can be backfilled only while upstream still exposes it. Backfilling millions of entities through Wikidata's API is slow and rate-limited, so mass rescues should draw on the history dumps where possible.
- **The main graph is computed.** Any change in any source graph means re-resolving the affected entity, so reconciliation cost becomes part of projection throughput.
- **Retention can preserve data upstream removed deliberately.** If upstream deleted an entity as a legal takedown, a retained copy survives on the instance. Retained entities opt out of the faithful-mirror guarantee. Editors must be able to switch a retained entity to `cascade` after the fact.
- **The meaning of a three-letter prefix changes.** It now means "minted by the provider". An instance may be the only remaining holder of such an entity, and it will be asserting things about IRIs in a namespace it does not control and that no longer resolve upstream. The alias IRI is the one the instance can serve.
- **Local assertions about foreign entities build up and need maintenance.** Stale corrections, dangling overrides and orphaned annotations need tooling.
- **`snapshot` mode is destructive by design,** and its safeguards in §8.4 are required, not optional.

## Open questions

- **Q1.** ~~**Graph IRIs.** Their exact form, and whether they are fixed or per instance. Carried over from 0001.~~ *Settled by [0015](0015-record-format-and-partition-registry.md) §5: `{base}/graph/{name}`, per instance, names fixed in the registry.*
- **Q2.** ~~**Configurable reconciliation.** Whether the default rules can be set per instance or per provider. Also how to reconcile two mirror graphs whose entities are linked by same-as, such as a Wikidata item and an OpenAlex work.~~ *Settled by [0004](0004-identity-clusters-and-equivalence.md) §4–§9 (clusters, canonical ID by provider order, statement fusion) and [0015](0015-record-format-and-partition-registry.md) §3 (the `reconcile` config kind, keyed `default` or a provider code, recorded in the log).*
- **Q3.** ~~**A local alias that conflicts with an upstream redirect.** For example, `WDQ123` has been converted to `Q456`, and Wikidata then merges `Q123` into `Q789`.~~ *Settled by [0004](0004-identity-clusters-and-equivalence.md) §5: the merge moves the cluster's Wikidata member; if that joins two clusters, the links are held as a conflict for an editor to resolve.*
- **Q4.** ~~**Permissions.** Who may run a bulk `retain` or `convert`, and who may set the instance's default retention policy.~~ *Settled by [0016](0016-permissions-and-access-control.md) §5: `ts-retain` or `ts-convert` with `ts-runjob`; the default policy is a `config` record, so `ts-config`.*
- **Q5.** ~~**Legal erasure from the local graph.** This remains open from 0000. Only mirrored data is resolved here.~~ *Settled by [0006](0006-log-integrity-and-erasure.md) §7 (the `erase` record, headers kept, bodies destroyed) and [0015](0015-record-format-and-partition-registry.md) §1 (per part); who may erase is `ts-erase` ([0016](0016-permissions-and-access-control.md) §2).*
- **Q6.** ~~**Mirrored properties in local assertions.** Whether local assertions may use mirrored properties such as `WDP31`, on foreign or local subjects. Also what happens if a mirrored property is deleted or changes data type.~~ *Settled by [0004](0004-identity-clusters-and-equivalence.md) §6 and A10: any graph may use any property; and a mirrored property is set `retain` automatically the first time a local statement uses it, so an upstream deletion materializes it into the local graph and local statements stand; an upstream data-type change conflicts with the retained type, the retained type wins for existing statements, and the conflict is listed for maintenance.*
- **Q7.** ~~**Statement IDs after conversion.** Whether statements keep `WDQ123$<uuid>` or are re-minted under the local ID.~~ *Settled by [0018](0018-tenants.md) §7: the log keeps the GUID as written; the resolved view rewrites the entity-ID prefix to the canonical ID and keeps the UUID.*
- **Q8.** ~~**Match-key properties.** Which identifier properties get uniqueness indexes, and how existing violations are handled.~~ *Settled by [0013](0013-postgres-storage.md) §5.2: `view.match_key` is populated for properties named in registry configuration, and an existing violation is reported when the constraint is turned on, not dropped; [0029](0029-resolver-namespaces.md) §3 keeps resolvers and match keys apart.*
- **Q9. Wire formats beyond NDJSON.** For example, a binary or columnar format for maximum throughput.
- **Q10.** ~~**IRIs for upstream revisions and actors.** Candidates are provider-derived IRIs, such as `https://www.wikidata.org/w/index.php?oldid=N` for a Wikidata revision. This makes 0001 Q3 about actors from foreign sources concrete.~~ *Settled by [0007](0007-actor-identity.md) §2 (`Special:Redirect/user/{id}`) and [0015](0015-record-format-and-partition-registry.md) §4 (`Special:Redirect/revision/{revid}`), both on the provider's wiki.*
- **Q11. Eager forks.** Whether to support freezing a copy of an entity now, as an operation distinct from `retain`.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0000](0000-init.md) §3 | §1 | extends | 0000 A1 |
| [0000](0000-init.md) Q2 | §8 | settles | 0000 Q2 |
| [0000](0000-init.md) Q3 | §1–2, §7 | settles | 0000 Q3 |
| [0001](0001-revision-metadata-rdf.md) §2 | §3 | amends | 0001 A1 |
| [0001](0001-revision-metadata-rdf.md) Q5 | §8.3 | settles | 0001 Q5 |

## Amendment log

### A1. Conversion joins a cluster

- **Date:** 2026-09-25
- **Source:** [0004](0004-identity-clusters-and-equivalence.md) §5, §9
- **Change:** extends §6; amends §8.2
- **Summary:** `convert` is a `same-as` link in which the local member is canonical, so §6's behaviour follows from the cluster rules. A local `redirect` now records only a merge within one namespace; its same-as meaning moves to the new `same-as` operation, beside `different-from` and `equivalent-property`. 0004's amendment of 2026-09-27 lets a local `redirect` of properties cross namespaces. 0004 §4–§9 also settled Q2 and §5 settled Q3.

Replaced text (§8.2):

> | `redirect` | Mirror or local | In a mirror graph, records an upstream merge. In the local graph, records a local merge or same-as. |

### A2. Key-mapped providers

- **Date:** 2026-09-26
- **Source:** [0009](0009-keyed-entity-types-and-domain.md) §9
- **Change:** amends §4, §5
- **Summary:** For an item a key-mapped provider maps onto a keyed entity, the canonical IRI is the key's, with `owl:sameAs` from the upstream item IRI. Retention works differently for keyed entities: an upstream deletion clears the mirror's contribution with an empty `put`, `cascade` and `orphan` do not apply, and `retain` still applies.

Replaced text (§4):

> - **The canonical IRI is the provider's own.** (for every foreign entity)

Replaced text: the `tombstone` and the `cascade` and `orphan` policies, which this entry stops applying to keyed entities.

### A3. Upstream hiding, and jobs in the local log

- **Date:** 2026-09-26
- **Source:** [0011](0011-logs.md) §5, §6.3
- **Change:** amends §5, §8.3
- **Summary:** Retention does not keep fields upstream hides for privacy or legal reasons; once a retained entity is local data, hiding within it is a local decision. A job record is appended to the local log with an instance-minted job ID, and further records under that key mark its finish, failure or revert.

Replaced text (§8.3):

> Every ingest run is recorded as a **job** in the log. The job record holds:

### A4. Mirror metadata that survives compaction

- **Date:** 2026-09-26
- **Source:** [0012](0012-api-requirements.md) §2.2
- **Change:** extends §8.2
- **Summary:** A `put` carries `prev_upstream`, `first_seen`, `size`, `prev_size`, a `changes` summary and, where the provider sets `sync_deltas: full`, a statement-level `delta`.

### A5. Bootstrap mode in Postgres

- **Date:** 2026-09-26
- **Source:** [0013](0013-postgres-storage.md) §9
- **Change:** extends §8.6
- **Summary:** Writers `COPY` records in parallel from offset blocks into tables with primary keys only; leaf hashes, segment subtrees, indexes and projections follow in order, and checkpoints are signed last. 0013 §5.2 also settled Q8.

### A6. Upstream revision records

- **Date:** 2026-09-26
- **Source:** [0015](0015-record-format-and-partition-registry.md) §4
- **Change:** extends §5, §8.3
- **Summary:** Backfilled, dumped or streamed upstream revisions are upstream revision records, identified by upstream revision ID; content is a `put` carrying it. 0015's amendment of 2026-09-27 makes a `retain` backfill fetch every revision's content by default, from the API or an operator-supplied history dump; §5 recorded this in place, without a note. 0015 §4 and §5 also settled Q1 and Q10.

### A7. ID rewriting by grammar

- **Date:** 2026-09-27
- **Source:** [0017](0017-entity-id-grammar.md) §2
- **Change:** amends §4
- **Summary:** ID rewriting strips or adds the prefix and canonicalizes the rest by the type's ID grammar.

Replaced text (§4):

>   - the ID rewriting between upstream and prefixed forms, such as `Q123` ↔ `WDQ123`;

### A8. Tenants as providers

- **Date:** 2026-09-27
- **Source:** [0018](0018-tenants.md) §5
- **Change:** extends §4
- **Summary:** A tenant with a code is a provider whose partition other tenants read directly; IDs are rewritten on read, and the reading tenant's IRI is an alias of the provider tenant's. 0018 §7 also settled Q7.

### A9. Jobs run as subsidiaries

- **Date:** 2026-09-27
- **Source:** [0024](0024-subsidiary-accounts.md) §6
- **Change:** amends §8.3
- **Summary:** A job's actor is the subsidiary that submitted it; its operator is read from the actor record.

Replaced text (§8.3):

> - the actor;

### A10. Mirrored properties a tenant uses are retained

- **Date:** 2026-09-27
- **Source:** Direct: James, review of 2026-09-27
- **Change:** extends §6
- **Summary:** The first local statement, qualifier or reference that uses a mirrored property (`WDP31`, say) sets that property's retention policy to `retain`, with a `retention/set` log event ([0011](0011-logs.md) §6.1) whose performer is the editor and whose comment says why. An upstream deletion then materializes the property's last state into the local graph, as §5 already provides, so its label, description and data type survive and every local statement using it keeps rendering, validating and exporting unchanged; the property page says "Deleted on Wikidata on {date}" from the provider log (0011 §9). If upstream later **changes the data type** of a retained property, the new mirrored state conflicts with the retained one: the retained type wins for the tenant's existing statements, new statements are validated against it, and the conflict is listed for maintenance beside the corrections of §7 so an editor can convert or retire the property with `equivalent-property` ([0004](0004-identity-clusters-and-equivalence.md) §6). Adapter mappings onto a retyped property stop and are reported, as 0004 §6 says. A tenant may turn the automatic `retain` off in `site` configuration (`retention.auto_properties`) and accept dangling statements, which is Wikidata's own behaviour after a property deletion. This settled the second half of Q6.

### A11. Triplespace providers

- **Date:** 2026-09-27
- **Source:** [0022](0022-federation.md) §2
- **Change:** extends §8.4
- **Summary:** For a provider that is itself a Triplespace instance, the adapter reads the provider's `local` graph only, rewrites references to the reader's own entities back to bare local IDs, and verifies each batch against the provider's checkpoint and key chain by default; a provider registered `trust = stream` is read like any other source.

### A12. Edit filters in bulk jobs

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §7
- **Change:** extends §8.5
- **Summary:** Filters apply to each operation of a bulk job in the `local` partition; `disallow` sends the operation to the rejects file with the filter named, and the job continues.

### A13. Sitelinks are URLs

- **Date:** 2026-09-27
- **Source:** [0026](0026-sitelinks.md) §4
- **Change:** amends §3
- **Summary:** Sitelinks are URLs and their site is their host. The rule becomes: union by normalized URL; where two graphs give one host different URLs for an entity, the local graph wins, then the provider order; badges are the union; a denied host ([0026](0026-sitelinks.md) §3) is left out of the resolved view.

Replaced text (§3):

> | Labels and descriptions (one per language), sitelinks (one per site), statement rank | The local graph wins wherever it says anything. Otherwise the mirror graph's value is used. |

### A14. The hash guard

- **Date:** 2026-09-28
- **Source:** Direct: James, decision of 2026-09-28 (the hash guard, [0006](0006-log-integrity-and-erasure.md) §2)
- **Change:** extends §8.4
- **Summary:** A `put` carries upstream's snak and reference hashes only where they differ from the instance's own recomputation; the ingester compares every one, keeps a differing hash in place and counts it on the job ([0006](0006-log-integrity-and-erasure.md) §2, as amended 2026-09-28).

### A15. Adoption

- **Date:** 2026-09-28
- **Source:** [0035](0035-adopting-a-wikibase.md) §3
- **Change:** extends §8.2, §8.5
- **Summary:** One further local operation, `adopt`, writes an entity's whole state as its first local record under the ID the source wiki minted. It is accepted only from an adoption job on a tenant that is that wiki continuing on Triplespace, on a `local` partition that holds no other entity records; it never updates, and re-running it skips what is present. It is the one whole-entity write to the local graph.

### A16. Special:Corrections

- **Date:** 2026-09-30
- **Source:** [0047](0047-special-pages.md) §6
- **Change:** extends §7
- **Summary:** The set is served as `Special:Corrections`, filterable by state, kind and upstream graph, with **Retire** for redundant corrections and an export per upstream graph.

### A17. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §3–9
- **Summary:** A1–A16 were folded into the Decision, the unnumbered scenario section became §9, the open questions were numbered, and two consequences that called settled questions open were struck. No decision changed. Before this, A10, A11, A13, A14, A15 and A16 were blockquotes; the content-by-default sentence of A6 had been written into §5 without a note; and the other entries were recorded only in this ADR's header or in other ADRs. The file before conversion is commit `0b26a3a`.

### A18. IRIs minted here for a provider that publishes none

- **Date:** 2026-10-01
- **Source:** [0037](0037-gdelt-provider.md) §2
- **Change:** amends §4
- **Summary:** GDELT publishes no IRIs, so its entities' canonical IRIs are minted under `https://scatter.red/gdelt/…`. 0037 §2 (2026-09-28) said it amended §4; nothing was written here until 0037's conversion to the 0050 format.

Replaced text (§4):

> - **The canonical IRI is the provider's own.** `WDQ123` projects to `http://www.wikidata.org/entity/Q123`, and `OAW123` projects to `https://openalex.org/W123`.

### A19. `create` is strictly creation; `create-or-add` takes the match key

- **Date:** 2026-10-02
- **Source:** Direct: James, decisions of 2026-10-02
- **Change:** amends §8.2, §8.5
- **Summary:** `create` always mints a new local entity and never writes to one that exists; the match-key behaviour moves to a new operation, `create-or-add`, which becomes an `add` when its match key finds an entity and may replace the entity's state with `overwrite` and an explicit base revision. The log records the resolved operation with `match` and `via: "create-or-add"` ([payloads.md](../api/payloads.md) §3.2).

Replaced text (§8.2):

> | `create` | Local | Creates a new local entity. It takes an optional temporary `ref` and an optional `match` key (§8.5). |

Replaced text (§8.5):

> - **Match keys.** A `create` can carry a match key, for example `"match": {"P356": "10.1234/x"}`. If an entity with that identifier exists, the operation updates it; otherwise it creates one. This makes re-runs idempotent. It requires a uniqueness index on designated identifier properties. A match key may also be a foreign entity ID.

### A20. Proposals retire redundant corrections

- **Date:** 2026-10-05
- **Source:** [0067](0067-proposals.md) §5
- **Change:** amends §7
- **Summary:** A redundant correction that followed a proposal is retired automatically under `upstream.retire_adopted`; the list of fixes to report upstream is now the proposals feature.

Replaced text (§7):

> - If upstream comes to agree with a correction, the application detects that the correction is redundant and can retire it.

### A21. Shallow mirroring

- **Date:** 2026-10-07
- **Source:** [0070](0070-shallow-entity-mirroring.md) §1
- **Change:** extends §8.4
- **Summary:** A referenced subset of a provider is kept current without a dump, entity by entity, as whole-state `put`s; a later dump sync continues from it.

### A22. Derived graphs

- **Date:** 2026-10-07
- **Source:** [0071](0071-derived-statements-from-mirrored-pages.md) §1, §7
- **Change:** extends §2; amends §3
- **Summary:** A tenant has one derived graph per extraction source, written only by that source's extraction job, holding statements derived from pages. Derived graphs are sources; for labels, descriptions and rank they rank after the local graph and before the mirrors.

Replaced text (§2):

> The local and mirror graphs are *sources*. The metadata graph and the resolved graph (§3) are *projections*.

Replaced text (§3, table):

> | Labels and descriptions (one per language), statement rank | The local graph wins wherever it says anything. Otherwise the mirror graph's value is used. |

### A23. The retention default is a tenant setting

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §5
- **Summary:** The default retention policy is a tenant `site` setting, `retention.default`, so it is each tenant, not each instance, that sets the default; editors still override it per entity. This follows the rule that a dotted setting key an ADR names without a scope is a tenant `site` setting (PENDING D3). (PENDING E1)

Replaced text (§5):

> What happens next depends on the entity's **retention policy**. Each instance sets a default, and editors can override it for individual entities.

### A24. Re-running `adopt` checks the content hash

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §8.2
- **Summary:** Re-running `adopt` skips an entity whose content hash matches what is already present and rejects one whose content differs, as [0035](0035-adopting-a-wikibase.md) §3 says; it does not silently skip every entity that is present. (PENDING E3)

Replaced text (§8.2):

> It is accepted only from an adoption job on a tenant that is that wiki continuing on Triplespace, on a `local` partition that holds no other entity records; it never updates, and re-running it skips what is present.

### A25. `adopt` is the one whole-entity write that needs no base revision

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §8.2
- **Summary:** `adopt` is "the one whole-entity write that needs no base revision", not the one whole-entity write to the local graph: `create-or-add` with `overwrite` and an explicit base revision still replaces a matched entity's state, as §8.2's table says. [0035](0035-adopting-a-wikibase.md) §3 is corrected the same way. (PENDING E4)

Replaced text (§8.2):

> `adopt` is the one other whole-entity write to the local graph: one further local operation, `adopt`, writes an entity's whole state as its first local record under the ID the source wiki minted.

> It is the one whole-entity write to the local graph.

### A26. The bulk sketch uses `create-or-add`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §8.7
- **Summary:** In the local bulk job sketch, the `create` that carries a `match` key becomes `create-or-add`, since A19 made `create` take no match key. (PENDING E5)

Replaced text (§8.7):

> {"op":"create","ref":"$w1","match":{"P356":"10.1234/x"},"entity":{…}}

### A27. Instance jobs run as the instance

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §8.3
- **Summary:** Instance jobs, such as the shallow-mirror fetches of [0040](0040-instance-prerogatives.md) §6, are excepted from "every job's actor is a subsidiary": their actor is the instance (0040 §2). Every other job's actor is still a subsidiary account, so its operator is known. [0070](0070-shallow-entity-mirroring.md) §3 is amended the same way. (PENDING E6)

Replaced text (§8.3):

> - the actor, which is always a subsidiary account, so the job's operator is known ([0024](0024-subsidiary-accounts.md) §6);

### A28. `adapter` and `actor_model` are provider fields

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §4
- **Summary:** `adapter` is one field per provider entry in `providers.toml`, not a record per type code; and `actor_model` (`individual` or `provider-only`) is an explicit provider field, since `issuer == slug` is not the test (OSM has authors). [0037](0037-gdelt-provider.md) §1 and the TOML schema are amended the same way. (PENDING E8)

Replaced text (§4):

>   - the adapter that imports the type (§8.4).

### A29. A provider may clear instead of tombstoning

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §5
- **Summary:** [0036](0036-openstreetmap-providers.md) stands: a provider may opt into the keyed-style clear, a `put` of the empty state, in place of a tombstone for an upstream deletion, by the registry flag `deletion = clear`; OSM sets it. §5 gains the option, and 0036 §2 cites 0002 §5 for it rather than [0009](0009-keyed-entity-types-and-domain.md) §9. (PENDING E9)

Replaced text (§5):

> When upstream deletes an entity, its sync job appends a tombstone to the mirror graph.

### A30. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [05](../architecture/05-providers-and-ingest.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
