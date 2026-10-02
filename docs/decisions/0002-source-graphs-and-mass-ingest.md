# 0002. Source graphs and mass ingest

- **Status:** Proposed
- **Date:** 2026-09-24
- **Updated:** 2026-10-01 (A17)
- **Author:** James Hare / Claude Opus
- **Changes:** [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md)
- **Uses:** [Wikibase data model and ontology contract](../api/wikibase-compat.md)

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

0000 §3 treats the three-letter prefix as marking a foreign entity. This ADR splits that into two independent facts:

- **Identity origin.** The prefix records who *minted* the ID. `WDQ123` was minted by Wikidata. `Q5` was minted by this instance.
- **Assertion source.** The named graph a triple lives in records who *asserts* it (§2).

An entity with ID `WDQ123` can have triples in the Wikidata mirror graph, in the local graph, or in both. A three-letter prefix means "minted by provider X". It does not guarantee that provider X still holds the entity (§5).

### 2. Source graphs

Every log record names the graph it writes to.

| Graph | Illustrative IRI | Written by | History in the log |
|---|---|---|---|
| **Local** | `{base}/graph/local` | Editors, local bulk jobs, and retention when it materializes data (§5) | Full |
| **Mirror**, one per provider | `{base}/graph/mirror/wikidata`, `{base}/graph/mirror/openalex` | Only that provider's sync jobs | Set per provider: `latest` or `full`. Retained entities always keep full history (§5). |
| **Metadata** ([0001](0001-revision-metadata-rdf.md)) | `{base}/graph/metadata` | Projection | Derived |

The local and mirror graphs are *sources*. The metadata graph and the resolved graph (§3) are *projections*.

**How much mirror history the log keeps is a storage policy.** It does not limit what the model can represent. Each mirror graph has a history policy:

- **`latest`** compacts the log to the latest state of each entity, as Kafka's log compaction does. This is the practical default for full mirrors of very large graphs.
- **`full`** keeps every state the instance has received.

Individual entities can be exempted from compaction, and retained entities always are (§5). Under either policy, a tombstone for an entity that is not retained erases its mirrored history once it is compacted.

**The metadata graph describes mirrored data just as it describes local data.** There are two kinds of history for mirrored data:

- **Observed history** is the sequence of states the instance received from sync jobs. How complete it is depends on the sync. A weekly dump sync skips upstream revisions made between dumps, while a sync from an edit stream sees each one. For Wikidata, every observed state carries its upstream revision ID. Observed history is therefore a sparse subset of upstream history.
- **Upstream history** is the provider's own revision history. For Wikidata, that is each revision's ID, timestamp, user, edit summary and tags. It can be backfilled for an entity from the provider's API or its history dumps, which fills in the gaps in observed history. OpenAlex publishes no revision history, so only observed history exists for it.

No editor or local bulk job can write to a mirror graph.

### 3. The main graph becomes a resolved view (amends 0001 §2)

*Changed by A13.*

[0001](0001-revision-metadata-rdf.md) §2 describes the main graph as the Wikibase-compatible RDF. That main graph is now **computed**. A reconciliation policy is applied over the source graphs to produce it. The result is still Wikibase-shaped, it is still what gets exported to QLever, and it is still rebuildable from the log. The source graphs may also be exported, for consumers who want to compare what the upstream source asserts with what the instance asserts.

Reconciliation happens at the level of Wikibase statements, not individual triples. Statements, references and values are already IRI nodes, so one graph can annotate a node that another graph asserts.

Default reconciliation rules:

| What | Rule |
|---|---|
| Statements, references, aliases | Union across graphs. A statement IRI that appears in two graphs is one statement. |
| Labels and descriptions (one per language), statement rank | The local graph wins wherever it says anything. Otherwise the mirror graph's value is used. |
| Sitelinks | Union by normalized URL: a sitelink is a URL, and its site is its host. Where two graphs give one host different URLs for an entity, the local graph wins, then the provider order. Badges are the union. A denied host is left out of the resolved view ([0026](0026-sitelinks.md) §3–4). |
| Suppressions | An explicit local-graph assertion removes a mirrored statement or term from the resolved view. |
| Truthy (`wdt:`) and normalized (`wdtn:`) triples | Computed only in the resolved view, from the resolved ranks. Never stored in a source graph. |

### 4. IRIs for foreign entities

*Changed by A2, A7, A8.*

- **The canonical IRI is the provider's own.** `WDQ123` projects to `http://www.wikidata.org/entity/Q123`, and `OAW123` projects to `https://openalex.org/W123`.
  - Nodes that come from upstream keep upstream's IRIs. Examples are Wikidata's statement nodes and mirrored property predicates such as `http://www.wikidata.org/prop/direct/P31`.
  - Statements that the local graph makes about a foreign entity get statement IRIs under the instance's base.
  - An item that a key-mapped provider maps onto a keyed entity takes the key's IRI instead, such as `https://scatter.red/domain/{key}`, and the resolved view emits `<upstream item IRI> owl:sameAs` it ([0009](0009-keyed-entity-types-and-domain.md) §9).
- **`{base}/entity/WDQ123` is an alias.** The instance can dereference it, but it is not used as a subject in RDF.
- **The prefixed ID is the working name** in the API and UI, as in a page called `Item:WDQ123`. It exists because an ID is easier to work with than a full IRI.
- **The provider registry** of 0000 Q4 records the following for each type code:
  - the canonical IRI template;
  - the ID rewriting between upstream and prefixed forms, such as `Q123` ↔ `WDQ123`: strip or add the prefix, and canonicalize the rest by the type's ID grammar ([0017](0017-entity-id-grammar.md) §2);
  - the adapter that imports the type (§8.4).

  This parallels Wikibase's own entity sources (see [wikibase-compat.md §5.1](../api/wikibase-compat.md)).
- **A tenant can be a provider.** Its IDs are rewritten when read, not when written: another tenant's projections read its partition and rewrite `Q6` to `LBQ6`, and `{base}/entity/LBQ6` is an alias of the provider tenant's canonical IRI ([0018](0018-tenants.md) §5).

### 5. Upstream deletion and retention (scenarios A and B)

*Changed by A2, A3, A6.*

When upstream deletes an entity, its sync job appends a tombstone to the mirror graph. Compaction later removes the mirrored data from the log. A faithful mirror therefore really forgets deleted data, including data upstream suppressed or took down. This resolves 0000's deletion question **for mirrored data only**.

What happens next depends on the entity's **retention policy**. Each instance sets a default, and editors can override it for individual entities.

| Policy | Mirrored data | Local-graph assertions about the entity |
|---|---|---|
| `cascade` | Erased | Retracted |
| `orphan` | Erased | Kept, but hidden from the resolved view |
| `retain` | Its history is kept, and its last state is **materialized into the local graph** in the same log record as the tombstone | Kept. The entity survives. |

**Scenario A** is `cascade`, or `orphan` if the instance wants to keep local annotations recoverable.

**Scenario B** is a bulk operation that sets `retain` on a set of entities. Nothing about their data changes when the operation runs. The mirror stays live and keeps syncing for as long as upstream holds each entity, and data moves into the local graph only when a tombstone arrives. Copying at rescue time was considered and rejected for two reasons:

- The copy would go stale while upstream keeps editing the entity. Because local values win in reconciliation, stale labels would override current ones.
- A lazy reference to the mirror's state at some log offset would break once that state is compacted away.

**Retention preserves history.** Setting `retain` on an entity does two more things:

1. **It exempts the entity from compaction,** so every state observed from then on is kept.
2. **It starts a job that backfills the entity's upstream history** (§2). This is time-sensitive. Once Wikidata deletes an item, its revisions are visible only to Wikidata administrators, so the backfill has to finish before the deletion. Since 2026-09-27 the backfill brings every revision's content by default, from the provider's API or, for a large set, from a history dump the operator supplies ([0015](0015-record-format-and-partition-registry.md) §4, as amended).

When the tombstone arrives, the local graph continues the entity's history: the first local revision's `prov:wasRevisionOf` points at the last upstream revision. This gives the same continuity as MediaWiki's transwiki import, where imported revisions keep their original authors.

**A retained entity keeps its ID and its canonical IRI.** `WDQ123` stays `WDQ123` and keeps projecting to `http://www.wikidata.org/entity/Q123`. This preserves continuity between the two projects. If upstream later restores the entity, the mirror graph fills in again next to the local copy. Identical statement IRIs collapse into one statement under §3.

**Retention does not override upstream hiding** ([0011](0011-logs.md) §5). `retain` keeps an entity that upstream deletes, not fields that upstream hides for privacy or legal reasons. Once a retained entity has been deleted upstream and materialized into the local graph, it is local data, and hiding within it is a local decision.

**Keyed entities do not stop existing** when upstream deletes the item mapped onto them ([0009](0009-keyed-entity-types-and-domain.md) §9). The mirror's contribution is cleared with a `put` of an empty state, not a `tombstone`, and nothing else happens: `cascade` and `orphan` do not apply. `retain` still keeps the mirrored history and materializes the last mirrored state.

### 6. Converting a foreign entity into a local one

*Changed by A1, A10.*

Editors may convert a foreign entity into a local entity.

- **A local ID is minted.** For example, `WDQ123` becomes `Q456`. `convert` links the two with `same-as`, and the local member is the cluster's canonical member, so the rest of this list follows from the cluster rules ([0004](0004-identity-clusters-and-equivalence.md) §5).
- **The foreign ID becomes an alias of the local entity.** `WDQ123` resolves to `Q456` in the API and UI. In RDF, the resolved view emits Wikibase's redirect form, `wd:Q123 owl:sameAs <{base}/entity/Q456>`, where `wd:` is Wikidata's namespace.
- **Foreign statements stay attached.** The sync job keeps writing to the mirror graph under the foreign ID, and the resolved view attaches those statements to the local entity through the alias. The same applies to any local-graph assertions previously made about `WDQ123`.
- **The local entity survives upstream deletion.** Its mirrored statements follow its retention policy, and converted entities default to `retain`.

**Properties a tenant depends on are retained automatically.** The first local statement, qualifier or reference that uses a mirrored property (`WDP31`, say) sets that property's retention policy to `retain`, with a `retention/set` log event ([0011](0011-logs.md) §6.1) whose performer is the editor and whose comment says why. An upstream deletion then materializes the property's last state into the local graph, as §5 already provides, so its label, description and data type survive and every local statement using it keeps rendering, validating and exporting unchanged; the property page says "Deleted on Wikidata on {date}" from the provider log (0011 §9). If upstream later **changes the data type** of a retained property, the new mirrored state conflicts with the retained one: the retained type wins for the tenant's existing statements, new statements are validated against it, and the conflict is listed for maintenance beside the corrections of §7 so an editor can convert or retire the property with `equivalent-property` ([0004](0004-identity-clusters-and-equivalence.md) §6). Adapter mappings onto a retyped property stop and are reported, as 0004 §6 says. A tenant may turn the automatic `retain` off in `site` configuration (`retention.auto_properties`) and accept dangling statements, which is Wikidata's own behaviour after a property deletion.

Retention keeps the foreign identity. Conversion gives the entity a local identity, for things that are now curated primarily on the instance.

### 7. Corrections and extensions (scenarios C and D)

*Changed by A16.*

Every local edit to a foreign entity is a **local-graph assertion whose subject is the foreign entity**. No local entity has to be created.

**Correcting a value (scenario C).** A correction is a rank override, an added statement, a suppression, or any combination of the three:

```trig
GRAPH <{base}/graph/mirror/openalex> {
  s:OAW123-a1 ps:P577 "2019-01-01T00:00:00Z"^^xsd:dateTime ; wikibase:rank wikibase:NormalRank .
}
GRAPH <{base}/graph/local> {
  s:OAW123-a1 wikibase:rank wikibase:DeprecatedRank .      # overrides the upstream rank
  s:OAW123-b2 ps:P577 "2018-01-01T00:00:00Z"^^xsd:dateTime ; wikibase:rank wikibase:PreferredRank .
}
```

(`s:` and `ps:` are used loosely here. Mirrored nodes carry their provider IRIs, per §4.)

Local corrections are tracked against upstream:

- If upstream comes to agree with a correction, the application detects that the correction is redundant and can retire it.
- If upstream removes the statement a local override points at, the override is flagged as dangling.
- The set of local corrections doubles as a list of fixes to report upstream.
- The set is served as `Special:Corrections`, filterable by state, kind and upstream graph, with **Retire** for redundant corrections and an export per upstream graph ([0047](0047-special-pages.md) §6).

**Correcting identity.** If upstream has conflated two things under one ID, the split requires new local entities, with the split recorded in the local graph. If upstream has two IDs for one thing, a local redirect or same-as assertion is enough.

**Extending (scenario D).** Local properties are asserted on foreign entities in the local graph. Their statement IDs take the form `WDQ42$<uuid>` and their IRIs sit under the instance's base. The UUID keeps them from colliding with upstream statement IDs.

### 8. Ingest API

#### 8.1 One change-set format, three entry points

All writes use a single internal change-set type. There are three ways to submit one:

1. **An in-process or CLI ingester that writes straight to the log.** This is the path for full loads of Wikidata or OpenAlex. Terabyte-scale dumps do not go over HTTP.
2. **An HTTP bulk-job endpoint.** It accepts streamed, compressed NDJSON, runs asynchronously, and reports status, rejected records and projection lag.
3. **The ordinary edit API,** which submits a batch of one.

#### 8.2 Operations

*Changed by A1, A4, A15.*

| Operation | Graph | Meaning |
|---|---|---|
| `put` | Mirror | Replaces the entity's mirrored state. It is skipped when the upstream version is not newer or the content hash is unchanged. It carries `prev_upstream`, `first_seen`, the old and new sizes, a summary of what changed and, optionally, the statement-level delta, so that history survives compaction ([0012](0012-api-requirements.md) §2.2). |
| `tombstone` | Mirror | Records an upstream deletion and applies the entity's retention policy (§5). |
| `redirect` | Mirror or local | In a mirror graph, records an upstream merge. In the local graph, records a merge within one namespace; for properties only, it may cross namespaces, retiring a local property in favour of a mirrored one ([0004](0004-identity-clusters-and-equivalence.md) §9). |
| `create` | Local | Creates a new local entity. It takes an optional temporary `ref` and an optional `match` key (§8.5). |
| `add` | Local | Merges statements, terms or references onto any subject, local or foreign. |
| `remove` | Local | Retracts local assertions. |
| `override` | Local | Overrides the rank or a term of an assertion from another graph, or suppresses it. |
| `retain` | Local (policy) | Sets the retention policy for a set of entities. |
| `convert` | Local | Mints a local entity and makes the foreign ID an alias of it (§6). |
| `same-as` | Local | Links two items from different namespaces ([0004](0004-identity-clusters-and-equivalence.md) §9). |
| `different-from` | Local | Blocks two IDs from ever sharing a cluster ([0004](0004-identity-clusters-and-equivalence.md) §9). |
| `equivalent-property` | Local | Links two properties from different namespaces ([0004](0004-identity-clusters-and-equivalence.md) §9). |
| `adopt` | Local | Writes an entity's whole state as its first local record, under the ID the source wiki minted, in an adoption job ([0035](0035-adopting-a-wikibase.md) §3). |

Local entities are merged into by default. Replacing a local entity wholesale is allowed only with an explicit base revision, so a bulk job cannot overwrite editors' work unseen. `adopt` is the one other whole-entity write to the local graph: one further local operation, `adopt`, writes an entity's whole state as its first local record under the ID the source wiki minted. It is accepted only from an adoption job on a tenant that is that wiki continuing on Triplespace, on a `local` partition that holds no other entity records; it never updates, and re-running it skips what is present. It is the one whole-entity write to the local graph.

#### 8.3 Import jobs

*Changed by A3, A6, A9.*

Every ingest run is recorded as a **job** in the log: a job record keyed by a job ID the instance mints, with further records under the same key for its finish, failure or revert ([0011](0011-logs.md) §6.3). The job record holds:

- the actor, which is always a subsidiary account, so the job's operator is known ([0024](0024-subsidiary-accounts.md) §6);
- the source, and the source version (a dump date or snapshot manifest);
- the adapter version;
- the job's parameters.

Every entity change in the run points to its job. This answers 0001 Q5, what a revision is for data that never passed through MediaWiki: it is the entity's change within a job.

- **Observed states get thin revisions.** When a state arrives without upstream revision metadata, its revision node carries little more than `prov:wasGeneratedBy` pointing at the job, which is a `prov:Activity`, plus the upstream revision ID where there is one. The details are stored once, on the job. Full revision nodes for every entity in a Wikidata import would add on the order of a billion metadata triples.
- **Upstream revisions get full revision nodes.** When upstream history is backfilled (§2), each upstream revision becomes a revision node carrying the provider's own metadata, in the vocabulary [0001](0001-revision-metadata-rdf.md) defines: timestamp, actor, parsed summary, tags and flags. The node describes the upstream revision itself. The job that brought it in is recorded separately, with `pav:retrievedFrom` and `pav:importedOn`. When a backfill covers a revision that was already observed, it enriches that revision's existing node instead of creating a second one. Each is recorded as an upstream revision record, identified by its upstream revision ID; content a backfill brings is a `put` carrying that ID ([0015](0015-record-format-and-partition-registry.md) §4).
- **A whole job can be reverted.**
  - For a local-graph job, reverting appends the inverse change sets.
  - For a mirror job, reverting means syncing again.

#### 8.4 Foreign imports

*Changed by A11, A14.*

- **Adapters** are Rust trait implementations, one per provider type. An adapter:
  - rewrites IDs (`Q`→`WDQ`, `P`→`WDP`);
  - maps non-Wikibase sources such as OpenAlex onto properties;
  - supplies canonical IRIs.

  **Statement IDs from sources that have none, such as OpenAlex, must be deterministic.** They are derived from the entity, property and value, as a name-based UUID for example, so that they stay stable across re-imports. Without that, a local override would detach on every sync.
- **Version cursor.** An index records each entity's upstream version (Wikidata `lastrevid`, OpenAlex `updated_date`) and content hash. A re-import appends only the entities that changed.
- **Import modes:**
  - **`upsert`** adds and updates entities. It never deletes anything.
  - **`snapshot`** declares that the input is complete for one provider and type. Entities the job did not see are tombstoned. This sweep runs only after the job completes successfully, and it aborts if the tombstone count exceeds a configured threshold, so a truncated dump cannot wipe out a mirror.
- **Upstream redirects and deletions** are explicit operations. They are never inferred.
- **Subset imports** are supported alongside full imports. A subset can be:
  - a list of IDs;
  - the closure of everything the local graph references;
  - terms only, meaning labels, descriptions and aliases without statements.
- **A provider that is itself a Triplespace instance** is read from its `local` graph only. The adapter rewrites references to the reader's own entities back to bare local IDs, and verifies each batch against the provider's checkpoint and key chain by default; a provider registered `trust = stream` is read like any other source. ([0022](0022-federation.md) §2)
- **Upstream hashes.** A `put` carries upstream's snak and reference hashes only where they differ from the instance's own recomputation; the ingester compares every one, keeps a differing hash in place and counts it on the job ([0006](0006-log-integrity-and-erasure.md) §2, as amended 2026-09-28).

#### 8.5 Local bulk creation

*Changed by A12, A15.*

- **Temporary refs.** New entities in a batch are named with handles such as `$w1`, and claims in the same batch can refer to those handles. The server allocates the real IDs and returns the mapping.
- **Match keys.** A `create` can carry a match key, for example `"match": {"P356": "10.1234/x"}`. If an entity with that identifier exists, the operation updates it; otherwise it creates one. This makes re-runs idempotent. It requires a uniqueness index on designated identifier properties. A match key may also be a foreign entity ID.
- **Merging statements by default.** A statement is added only if no identical statement already exists. Two statements are identical when their main snak and qualifiers hash the same. If an identical statement exists, the incoming references are merged onto it.
- **ID blocks.** An ingester can reserve a block of IDs up front instead of making a round trip for each entity. Gaps in the ID sequence are acceptable.
- **Adoption keeps the source's IDs.** An adoption job writes local entities under the IDs the source wiki minted, with `adopt`, not `create` ([0035](0035-adopting-a-wikibase.md) §3).
- **Atomicity:**
  - An `atomic` batch is all-or-nothing, because temporary refs point across the batch.
  - A `stream` job applies each entity independently and writes rejected records to a rejects file. An operation an edit filter disallows goes there too, with the filter named as the reason ([0030](0030-edit-filters.md) §7).

#### 8.6 Throughput

*Changed by A5.*

- **Bootstrap mode.** An initial load writes log segments and defers projections. The projections are then built from the log in a single pass, the way a database bulk load defers index builds. Incremental projection only needs to keep up with steady-state syncs. In Postgres, writers `COPY` records in parallel into tables that have only their primary keys, and indexes and projections are built afterwards ([0013](0013-postgres-storage.md) §9).
- **Validation in two tiers.** The shape of each change and property data types are checked eagerly; this needs only the property type map, which is small. References between entities are checked later, because dumps contain forward references.
- **Parsing is the expected bottleneck.** Decompression should be parallel (multistream bz2, for example) and JSON parsing should use SIMD.

#### 8.7 Wire format sketch

A mirror sync job:

```
{"job":{"source":"openalex","snapshot":"2026-09-01","mode":"upsert","graph":"mirror/openalex"}}
{"op":"put","id":"OAW123","upstream":"2026-08-30","entity":{…}}
{"op":"redirect","from":"OAW99","to":"OAW123"}
{"op":"tombstone","id":"OAW77"}
```

A local bulk job:

```
{"job":{"source":"citation-batch","graph":"local","atomic":true}}
{"op":"create","ref":"$w1","match":{"P356":"10.1234/x"},"entity":{…}}
{"op":"add","id":"Q5","claims":{"P50":[…"$w1"…]}}
{"op":"add","id":"WDQ42","claims":{"P7":[…]}}
{"op":"override","statement":"OAW123$…","rank":"deprecated"}
{"op":"retain","ids":["WDQ123","WDQ124"]}
```

Entity payloads use the Wikibase canonical JSON described in [wikibase-compat.md §3](../api/wikibase-compat.md).

### 9. How the scenarios resolve

| | Resolution |
|---|---|
| **A** | A `tombstone` in the mirror graph, followed by `cascade` (or `orphan`). Compaction erases the mirrored data. |
| **B** | A bulk `retain`. IDs and canonical IRIs are kept, and data is materialized into the local graph only when each tombstone arrives. |
| **C** | A local-graph `override` or `add` on the existing foreign entity. A local entity is needed only when the upstream identity itself is wrong. |
| **D** | A local-graph `add` of local properties onto the foreign entity. |

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
