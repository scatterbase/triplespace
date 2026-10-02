# 0032. The SPARQL Update stream

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-01 (A3)
- **Author:** James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0020](0020-change-feeds.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0018](0018-tenants.md), [0022](0022-federation.md), [0024](0024-subsidiary-accounts.md), [Wikibase contract](../api/wikibase-compat.md)

## Context

[0013](0013-postgres-storage.md) §8 made RDF an output: `triplespace-rdf` streams the resolved view, the source graphs and the metadata graph as N-Quads into two dump products, and an external triplestore loads them. Nothing keeps that triplestore current. The activity stream of [0020](0020-change-feeds.md) §4 carries change sets, page operations and log events, which is what another Triplespace instance or a bot wants and what a triplestore cannot use. A QLever instance loaded from last week's dump is a week behind until it reloads, which at Wikidata scale is hours of work.

Wikidata's answer is the WDQS updater: a stream of per-entity diffs, each one `DELETE`s and `INSERT`s, applied to Blazegraph in order. QLever now accepts SPARQL 1.1 Update, as do Oxigraph, Fuseki, Virtuoso and GraphDB. James's requirement is that Triplespace **emit a SPARQL Update stream** that any of them can apply.

Three things are already in place. The resolution projection re-resolves an entity whenever any contributing record changes and bumps `resolved_version` ([0013](0013-postgres-storage.md) §5.1); the diff of [0012](0012-api-requirements.md) §7 already computes the truthy triples added and removed by a change; and every dump is a projection of a known point in the log. What is missing is the **delta** between two resolved states as triples, kept long enough for a consumer to fetch, ordered so that consumers agree, and serialized as Update requests.

Three choices James made in review shape the rest: the stream is **selectable per subscription** (the resolved view alone, or with source and metadata graphs); delivery is **pull**, with a sync client the operator runs, so the instance never holds a consumer's credentials; and blank nodes are **skolemized** in the stream and in the dump it continues from, so every delete is `DELETE DATA`.

## Decision

### 1. The stream is the dump, kept current (extends 0001 §2 and 0013 §8)

*Changed by A2.*

For every graph an instance publishes in RDF, it also publishes the **sequence of changes** to that graph as SPARQL 1.1 Update requests. A consumer loads a dump, notes the **cursor** the dump was taken at, and applies every event after that cursor in order. The result is, at every cursor, the same set of triples the dump would contain if it were taken then. This is the contract, and the end-to-end test in the Consequences checks it.

The stream carries the **public form** only, exactly as a dump does: nothing behind a read ACL ([0023](0023-moderation.md)), nothing erased ([0006](0006-log-integrity-and-erasure.md) §7), nothing from an `internal` or `private` partition ([0005](0005-crate-organization.md) §4.1). The graphs a subscription may name are those of [0013](0013-postgres-storage.md) §8:

| `graphs=` | Carries | Continues from |
|---|---|---|
| `resolved` (default) | The resolved view of [0002](0002-source-graphs-and-mass-ingest.md) §3, including the truthy and normalized triples, as a single default graph | The Wikibase-compatible dump |
| `resolved,local`, `resolved,mirror/wikidata`, … | The resolved view and the named source graphs, each in a `GRAPH` clause | The Wikibase-compatible dump with those source graphs added |
| `full` | Every public source graph, the resolved view and the metadata graph, all in `GRAPH` clauses. A modifier `metadata=local` (the default) carries revision, actor and event nodes for the tenant's own records and the provenance triples of [0015](0015-record-format-and-partition-registry.md) §6 for mirrored entities, but not a mirror's per-revision nodes; `metadata=all` carries every node the full dump does | The full dump, which takes the same modifier |

When exactly one graph is subscribed, its triples are emitted without a `GRAPH` clause, as the Wikibase-compatible dump is a single-graph dump ([0001](0001-revision-metadata-rdf.md), Consequences); with more than one, every triple is inside `GRAPH <{base}/graph/{name}> { … }`, with the IRIs of [0015](0015-record-format-and-partition-registry.md) §5. A consumer that loads the single-graph dump into its default graph and subscribes to `resolved` never sees a graph name.

### 2. The delta is computed when the view changes (extends 0002 §2–3 and 0013 §7)

A **delta** is a pair of triple sets, deleted and inserted, for one graph, caused by one event. It is computed by the projections that already produce the new state, at the moment they have both the old state and the new:

| Graph | Event | Old state | New state |
|---|---|---|---|
| `resolved` | The resolution projection writes an entity ([0013](0013-postgres-storage.md) §5.1, step 4 of §7) | The entity's previous resolved JSON, read before it is overwritten | The new resolved JSON |
| `local`, `mirror/{provider}` | A change set is applied to the entity's state in that graph ([0002](0002-source-graphs-and-mass-ingest.md) §8.2) | The graph's previous state for the entity, which the ingester reads to compute `changes` ([0012](0012-api-requirements.md) §2.2) | The new state |
| `metadata` | A record is appended, hidden or erased | The revision, actor, event and provenance nodes the record projected before | The nodes it projects now |

Each side is rendered to triples by `scatter-wikibase-rdf` ([wikibase-compat.md](../api/wikibase-compat.md) §4–5) and the delta is the set difference, so a change that touches one statement of a large entity is a few triples, not the entity. A **cluster change** ([0004](0004-identity-clusters-and-equivalence.md) §4) re-resolves every referrer and produces one delta per referrer; that is the fan-out 0004's Consequences already accept, and here it is where it becomes visible. A **read ACL** or an **erasure** produces a delta whose deleted side is what left the public form and whose inserted side is empty, or the tombstone form where one exists.

**Why at projection time.** Under the `latest` history policy a mirror's earlier state is compacted away ([0002](0002-source-graphs-and-mass-ingest.md) §2), and [0012](0012-api-requirements.md) §7 already concedes that the resolved view at a past time cannot be rebuilt in general. The only moment both states exist is when the new one replaces the old. The delta is therefore stored (§3), not recomputed on request.

**Bootstrap mode** ([0002](0002-source-graphs-and-mass-ingest.md) §8.6, [0013](0013-postgres-storage.md) §9) produces no deltas: there is no previous state, and a consumer of a bootstrapped partition loads the dump taken at the end of the bootstrap. A `snapshot` job's tombstone sweep ([0002](0002-source-graphs-and-mass-ingest.md) §8.4) produces one delta per removed entity, as any tombstone does.

### 3. Storage: `view.rdf_delta` (extends 0013 §5.6)

*Changed by A2.*

```sql
CREATE TABLE view.rdf_delta (
  seq          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,   -- the cursor; one sequence per instance
  tenant       text NOT NULL DEFAULT '',  -- '': a shared-row delta, visible to every tenant that reads its provider (0018 §6, 0013 §5)
  graph        text NOT NULL,             -- resolved | local | mirror/{provider} | metadata
  provider     text,                      -- for a shared delta, the provider whose graph produced it (§4)
  entity_id    text,                      -- the subject entity; NULL for a metadata-only event
  partition    bigint, "offset" bigint,   -- the record that caused it, where one did
  resolved_version bigint,                -- 0013 §5.1, for resolved deltas
  deleted      bytea NOT NULL,            -- N-Triples (N-Quads under full), canonical order, lz4
  inserted     bytea NOT NULL,
  txn          bigint NOT NULL,            -- the appending transaction; one event per txn (§6)
  time         timestamptz NOT NULL
);
CREATE INDEX rdf_delta_txn ON view.rdf_delta (txn, seq);
CREATE INDEX rdf_delta_tenant ON view.rdf_delta (tenant, seq);
CREATE INDEX rdf_delta_entity ON view.rdf_delta (entity_id, seq);   -- the erasure purge (§5)
```

- **`seq` is the cursor.** It is one sequence for the instance, so every tenant's stream is a filter over one total order, and it is monotonic in the order deltas were produced, which is the order projections applied them. Consumers apply events in `seq` order and never see two events for one entity out of order.
- **A transaction is one event.** Every row carries the `txn` of the appending transaction that produced it (`txn bigint NOT NULL`, indexed with `seq`); the deltas of one transaction, an edit with its fan-out within the synchronous budget ([0013](0013-postgres-storage.md) §7) or one batch of a sync job, are delivered as one event whose cursor is the transaction's last `seq`, so a consumer applies an edit atomically and never sees half of it. The queued remainder of a large fan-out is its own transactions and its own events.
- **Retention is a window.** Rows older than `updates.retention` (`site` configuration, default 30 days) are deleted by a daily sweep. A consumer whose cursor has fallen out of the window is told to reload (§6). This is how a Kafka-fed updater behaves, with the window as the topic's retention.
- **The table is a projection**, rebuilt from the log with every other `view` table ([0013](0013-postgres-storage.md) §7). A rebuild reproduces the same *final* state, but not the same sequence of intermediate states under compaction, and its rows take new `seq` values. Every cursor therefore carries an **epoch**, a number in `ops` that a rebuild increments; a cursor from an earlier epoch is refused with the reload response, as an expired one is.
- **A delta is never a record.** Nothing here enters the log; the stream is derived, unverifiable and rebuildable, which is what distinguishes it from the verified sync of [0022](0022-federation.md) Part A (§8).

### 4. Tenants: shared deltas and overlay deltas (uses 0018 §6)

A tenant's stream is the sequence of changes to **what that tenant's resolved view shows**. Under [0018](0018-tenants.md) §6 most of a tenant's view is shared rows computed once for the instance; only entities the tenant has touched have overlay rows.

- **A shared-row delta is stored once**, with the instance's empty-string tenant and `provider` set. It appears in the stream of every tenant whose `providers` list ([0018](0018-tenants.md) §3) includes that provider and which has no overlay for the entity. A change to a Wikidata item is one row, read by a thousand tenants' streams.
- **An overlay delta is the tenant's own**, stored with its `tenant`. It is computed against the state *that tenant's stream last showed*: when a tenant first overlays an entity, the old state is the shared state, so the delta removes the shared triples that change and adds the overlay's; when an overlay goes away, the reverse. While an overlay exists, shared deltas for that entity are filtered out of the tenant's stream and the overlay is recomputed instead, which the overlay rule of [0013](0013-postgres-storage.md) §7 already does.
- **The farm base** serves the instance-level source graphs only, `graphs=mirror/{provider}`, for a consumer that wants a provider's mirror without any tenant's view; there is no resolved view at the farm base.
- **A provider tenant's entities** appear in a reading tenant's stream under the reading tenant's rewriting ([0018](0018-tenants.md) §5): `LBQ6` with Librarybase's canonical IRI, exactly as its dump shows them.

### 5. Erasure and hiding reach the consumer, and the store (extends 0006 §7 and 0014 §5)

**The consumer.** An `erase` record or a read ACL produces a delta whose deleted side names every triple that left the public form ([0015](0015-record-format-and-partition-registry.md) §1, [0023](0023-moderation.md) §5): the statement, its qualifiers, references and values, the truthy triple, the revision node's hidden fields in the metadata graph. A consumer that applies the stream is therefore as compliant as this instance is, which is the property [0020](0020-change-feeds.md) §4 gives the activity stream and [0006](0006-log-integrity-and-erasure.md) §7 asks of copies elsewhere. It cannot be enforced beyond that, as 0006 says.

**The store.** Delta rows hold triples, and an erased literal may sit in the `inserted` side of an earlier row within the retention window. The erasure path of [0014](0014-caches-and-search.md) §5 gains a **sixth step**: delete or rewrite every `rdf_delta` row for the entity that contains an erased triple, found through `rdf_delta_entity`. A consumer that fetches that range afterwards receives the rows as rewritten; one that fetched them before is in the position 0006 §7 describes, and the deleting delta reaches it next. The retention window bounds how long an erased triple can persist in the table, as the TTL ceilings of 0014 bound the caches.

**Hidden and suppressed content never enters a delta**, because the delta is computed from the public form.

### 6. Delivery: pull, with a sync client (extends 0012 §5)

*Changed by A2.*

The instance **serves**; it never pushes, and it holds no consumer's credentials.

| Route | Meaning |
|---|---|
| `GET /updates/stream?graphs=&metadata=&from=` | Server-sent events. Each event is one transaction (§3): its `id` is the cursor `{epoch}:{seq}` of the transaction's last delta, its `event` is `update`, its `data` is one SPARQL Update request, `DELETE DATA { … } ; INSERT DATA { … }`, over every delta in the transaction, and a leading `: ` comment line carries the event's provenance: the tenant, the record `partition` and `offset` that caused it and its `revid`, or the job for a sync batch, so a consumer can fetch `/record/{partition}/{offset}` and its proof on demand. Nothing of that enters the triples. `Last-Event-ID` resumes. It is the mechanism of `GET /activity/stream` ([0020](0020-change-feeds.md) §4) with a different body |
| `GET /updates?graphs=&metadata=&from=&limit=` | The same events as one `application/sparql-update` document, one request per event separated by `;`, each preceded by a `#` comment line with its cursor and provenance, with the next cursor in a `Link: rel="next"` header, for consumers that poll |
| `{base}/dumps/updates/{epoch}/{from}-{to}.ru` | Batch files, written every `updates.batch_interval` (default one hour), each holding the events of its range; `manifest.json` beside them lists the ranges and the cursor of every dump under `{base}/dumps/` |
| `GET /updates/cursor` | The current epoch and head `seq`, and the retention horizon |

**Every dump is stamped** with the cursor it was taken at, in its manifest and in a comment on its first line, so a consumer knows where to start. A `from` older than the horizon, or from another epoch, is answered with **`410 Gone`** and the URL of the newest dump that continues into the window; the client reloads.

**Idempotency.** `DELETE DATA` and `INSERT DATA` are set operations, so applying an event twice is harmless and a consumer may commit its cursor after each event or after a batch. Applying events out of order is not harmless; the stream is ordered and the client applies it in order.

**`triplespace-cli sparql-sync`** is the reference consumer, run by the operator of the triplestore:

```
triplespace-cli sparql-sync --source https://librarybase.org --graphs resolved \
    --endpoint https://qlever.example/api/update --token-file ./qlever.token \
    --state ./librarybase.cursor
```

It loads the dump on first run when asked, then follows the stream, POSTs each event (or a batch) to the endpoint as `application/sparql-update` with the endpoint's own authorization, and commits the cursor to its state file after the endpoint acknowledges. On `410` it stops and names the dump to load. It speaks only standard SPARQL 1.1 Update over HTTP, so it works against QLever, Oxigraph, Fuseki and any store that does; endpoint-specific options (QLever's access token, a graph-store URL) are flags, not code paths.

**The local quad store is a consumer** ([0013](0013-postgres-storage.md) §8, [0005](0005-crate-organization.md) §4.4). An instance that runs Oxigraph or QLever for its own SPARQL endpoint feeds it from `view.rdf_delta` in process, through `scatter-quadstore`'s `apply(delta)`, or from its own stream with `sparql-sync`; either way it is the same rows the stream serializes, and the separate quad projection of 0013 §8 is retired. `scatter-quadstore` gains `apply(deleted, inserted)` beside `lookup`, which Scatterbase's drivers may implement or ignore.

**Rate limits** are the `stream` class of [0024](0024-subsidiary-accounts.md) §5; a consumer that needs more opens a subsidiary and asks for it. **Permissions:** `read`. A tenant whose partitions are `private` has no stream, as it has no dump.

### 7. Blank nodes are skolemized (extends 0013 §8)

*Changed by A2.*

Wikibase's RDF gives an *unknown value* snak a blank node object. `DELETE DATA` cannot name a blank node, and a diff between two renderings of one entity cannot match blank nodes at all. In the stream, and in every dump the stream continues from, a blank node is replaced by a **skolem IRI**:

```
https://scatter.red/genid/{H(statement id ‖ snak role ‖ property ‖ index)}
```

The IRI is derived from what the blank node stands in for and from nothing about the instance, so it is the same on every re-resolution, every rebuild and **every instance**: two tenants' streams about one Wikidata item agree on the node, and a store fed by both holds one. It lives under `scatter.red` for the reason Domain IRIs do ([0005](0005-crate-organization.md) §5, [0009](0009-keyed-entity-types-and-domain.md) §6): an identifier derived only from content is not instance data. The statement ID in the hash is upstream's for a mirrored statement and the tenant's for a local one, so the inputs never collide. A store that does not care sees an IRI and treats it as opaque.

**The plain dump keeps its blank nodes.** The Wikibase-compatible dump of [0013](0013-postgres-storage.md) §8 is unchanged and identical in shape to Wikibase's ([0001](0001-revision-metadata-rdf.md) §3). Each dump is also offered with `?bnodes=skolem`, and it is that form the stream continues from; `manifest.json` says which form a file is. The batch files and every event use skolem IRIs always. A consumer that loaded the blank-node form and subscribes is told so at its first event (`412`), because its deletes would never match.

Nothing else in the resolved view's RDF changes. Statement, reference and value nodes are already IRIs ([0001](0001-revision-metadata-rdf.md) §2), which is what makes a triple-level diff possible at all.

### 8. What this stream is not

- **Not the verified sync of [0022](0022-federation.md).** Another Triplespace instance reads a provider's `local` graph through the activity stream and verifies every record against the provider's checkpoints. This stream is derived from the resolved view, includes mirrors, and carries no proofs. An instance that consumed it would hold what the provider *shows*, unverifiably; 0022 gives it what the provider *asserts*, provably. The two are kept apart.
- **Not the activity stream of [0020](0020-change-feeds.md) §4.** That stream carries activity rows redacted per viewer, for bots, tools and other instances; this one carries triples, for triplestores. They share the transport and the `Last-Event-ID` discipline and nothing else.
- **Not a change feed of the log.** A delta is a projection's output. Records, offsets and proofs are the log's; a consumer that wants them reads `/record/` and `/.well-known/tlog/` ([0022](0022-federation.md) §1).

### 9. Crates (amends 0005 §2 and §4.4)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed. The table this section first gave is in A1; the paragraph on the local quad store that followed it is now in §6.

## Consequences

- **A triplestore can stay current** with any Triplespace tenant by loading one dump and following one stream, using only standard SPARQL 1.1 Update. QLever is the first target; nothing is specific to it.
- **The dump and the stream are one contract.** At every cursor the stream reproduces the dump. A test loads a dump into Oxigraph, applies the stream to a later cursor, and compares the result with the dump taken at that cursor, for every `graphs=` form; erasure and hiding cases are in the fixture set.
- **Deltas are stored, not recomputed.** `view.rdf_delta` grows with the instance's rate of change times the retention window, not with the size of the mirrors: a Wikidata sync of a million changed entities is a million rows, for thirty days. At Wikidata's edit rate that is on the order of 10^8 rows and tens of gigabytes in the window; the bootstrap audit of 0013 should measure it beside `entity_ref`.
- **Cluster changes fan out twice.** Once into the resolved view, as 0004 accepts, and now into the stream, where a consumer receives one event per referrer. Linking a heavily cited author is a burst of events; the consumer applies them in order and is done.
- **Erasure has a sixth layer to walk** and a window that bounds it, as with every other derived copy.
- **Rebuilds invalidate cursors.** A `view` rebuild is a new epoch and every consumer reloads. Rebuilds are rare and announced; the epoch makes the failure loud rather than silent.
- **Skolem IRIs are a documented departure**, confined to somevalue snaks, offered as a second dump form and never in the plain one. A Wikidata tool that counts blank nodes sees the plain dump; a store that follows the stream sees IRIs.
- **The instance does no pushing.** Operators run `sparql-sync` where the triplestore is, with that store's credentials. The instance's cost is serving a filtered range read, which is the cost of any feed.
- **The local quad store gets simpler.** One delta table feeds the instance's own SPARQL endpoint and everyone else's.

## Open questions

- **Q1. Retention default and the epoch's lifetime.** Thirty days is a guess; the right value is the longest a consumer is expected to be down, measured against the table's size at Wikidata scale.
- **Q2.** ~~**Metadata-graph volume under `full`.** Thin revision nodes for a Wikidata mirror ([0002](0002-source-graphs-and-mass-ingest.md) §8.3) are on the order of a billion triples; whether a `full` subscription should be able to exclude the mirror's revision nodes and keep only local ones is a `graphs=` refinement to decide from a consumer's actual need.~~ *Settled by A2: `metadata=local` by default, `metadata=all` on request, on the stream and the full dump alike.*
- **Q3.** ~~**Batching within an event.** One event per delta row is simple and ordered; a consumer applying a million-entity sync one request at a time is slow.~~ *Settled by A2: one event per appending transaction.*
- **Q4. Other formats.** RDF Patch (Jena's delta format) and LDES both express the same delta; whether to offer either beside SPARQL Update, once a consumer asks.
- **Q5. A farm-wide stream.** Whether a consumer may subscribe to every tenant's resolved view at the farm base, each event carrying its tenant, under the `feeds.farm_wide` policy of [0028](0028-tenancy-policy.md) §9.
- **Q6.** ~~**The skolem hash input.** Statement ID, snak role, property and index are enough for uniqueness; whether the IRI should instead be content-derived, so that two instances skolemize a mirrored Wikidata blank node identically, as [0009](0009-keyed-entity-types-and-domain.md) §6 does for Domain IRIs.~~ *Settled by A2: content-derived, under `https://scatter.red/genid/`.*
- **Q7.** ~~**Consumers verifying what they applied.** A consumer holds triples and no proofs. Whether an event should carry the record coordinates and checkpoint of the change that caused it, so a consumer could check a delta against `/record/` on demand, without turning this into the verified sync of 0022.~~ *Settled by A2: as event metadata in a comment line, never as triples; spot-checkable per event, unverifiable as a whole.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0001](0001-revision-metadata-rdf.md) §2, §3 | §1, §7 | extends | 0001 A11 |
| [0005](0005-crate-organization.md) §2, §4.4 | §9 | extends | 0005 A28 |
| [0012](0012-api-requirements.md) §5 | §6 | extends | 0012 A20 |
| [0013](0013-postgres-storage.md) §8 | §1–3, §7, §9 | amends | 0013 A10 |
| [0013](0013-postgres-storage.md) §5.6, §7 | §1–3, §7, §9 | extends | 0013 A10 |
| [0014](0014-caches-and-search.md) §1, §5 | §5–6 | amends | 0014 A3 |
| [0014](0014-caches-and-search.md) §10 | §5–6 | extends | 0014 A3 |
| [0020](0020-change-feeds.md) §4 | §6 | extends | 0020 A8 |

## References

- [SPARQL 1.1 Update](https://www.w3.org/TR/sparql11-update/) and the [SPARQL 1.1 Protocol](https://www.w3.org/TR/sparql11-protocol/) (`application/sparql-update`)
- [QLever: SPARQL Update support](https://github.com/ad-freiburg/qlever) and [QLever access tokens](https://github.com/ad-freiburg/qlever/wiki)
- [Wikidata Query Service Streaming Updater](https://wikitech.wikimedia.org/wiki/Wikidata_Query_Service/Streaming_Updater) (per-entity diffs, Kafka offsets as cursors)
- [RDF 1.1 Concepts §3.5 — Replacing blank nodes with IRIs (skolemization)](https://www.w3.org/TR/rdf11-concepts/#section-skolemization)
- [Server-sent events](https://html.spec.whatwg.org/multipage/server-sent-events.html)
- [RDF Patch](https://afs.github.io/rdf-patch/), [LDES — Linked Data Event Streams](https://w3id.org/ldes/specification)
- [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) §8, [0020 — Change feeds](0020-change-feeds.md) §4, [0022 — Federation](0022-federation.md) Part A

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §9
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A28). The paragraph on the local quad store as a consumer, which followed the table, moved to §6 at conversion.

Replaced text (§9):

> | Layer | Crate | Change |
> |---|---|---|
> | Wikibase | `scatter-wikibase-rdf` | The triple-set diff of two entity renderings; blank-node skolemization by statement, role, property and index (§7); serializing a delta as SPARQL 1.1 Update, single-graph and `GRAPH` forms (§1, §6). Pure |
> | Triplespace | `triplespace-projections` | The delta projection: computes deltas in the resolution, source-graph and metadata projections and writes `view.rdf_delta` (§2–3); the tenant filter and overlay transitions (§4); the epoch; the retention sweep; the sixth erasure step (§5) |
> | | `triplespace-rdf` | Stamps every dump with its cursor; the `?bnodes=skolem` dump form; the batch files and `manifest.json` (§6) |
> | | `triplespace-api-rest` | The routes of §6 |
> | | `triplespace-cli` | `sparql-sync` (§6) |
>
> **The local quad store is a consumer** ([0013](0013-postgres-storage.md) §8, [0005](0005-crate-organization.md) §4.4). An instance that runs Oxigraph or QLever for its own SPARQL endpoint feeds it from `view.rdf_delta` in process, through `scatter-quadstore`'s `apply(delta)`, or from its own stream with `sparql-sync`; either way it is the same rows the stream serializes, and the separate quad projection of 0013 §8 is retired. `scatter-quadstore` gains `apply(deleted, inserted)` beside `lookup`, which Scatterbase's drivers may implement or ignore.
>
> No crate is added.

### A2. Decisions of 2026-09-27 (evening)

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** amends §1, §3, §6, §7
- **Summary:** Decision 16, 0032's calls: `metadata=local` by default and `metadata=all` on request, on the stream and the full dump alike (§1); one event per appending transaction, with a `txn` column (§3, §6); skolem IRIs content-derived under `https://scatter.red/genid/` (§7); provenance (tenant, partition, offset, revid) as a comment line per event, never as triples (§6). The sections were revised in place. This settled Q2, Q3, Q6 and Q7.

Replaced text: not recorded; the sections were revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A3. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–9
- **Summary:** A1–A2 were folded into the Decision. The open questions were numbered. §9's quad-store paragraph moved to §6. No decision changed. Before this, A2 was four struck questions with notes. The file before conversion is commit `0b26a3a`.
