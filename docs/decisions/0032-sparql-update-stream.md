# 0032. The SPARQL Update stream

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A7)
- **Author:** James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0020](0020-change-feeds.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0018](0018-tenants.md), [0022](0022-federation.md), [0024](0024-subsidiary-accounts.md), [Wikibase contract](../api/wikibase-compat.md)
- **Chapters:** [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [08](../architecture/08-tenants-and-instances.md), [22](../architecture/22-crates-and-stack.md)

## Context

[0013](0013-postgres-storage.md) §8 made RDF an output: `triplespace-rdf` streams the resolved view, the source graphs and the metadata graph as N-Quads into two dump products, and an external triplestore loads them. Nothing keeps that triplestore current. The activity stream of [0020](0020-change-feeds.md) §4 carries change sets, page operations and log events, which is what another Triplespace instance or a bot wants and what a triplestore cannot use. A QLever instance loaded from last week's dump is a week behind until it reloads, which at Wikidata scale is hours of work.

Wikidata's answer is the WDQS updater: a stream of per-entity diffs, each one `DELETE`s and `INSERT`s, applied to Blazegraph in order. QLever now accepts SPARQL 1.1 Update, as do Oxigraph, Fuseki, Virtuoso and GraphDB. James's requirement is that Triplespace **emit a SPARQL Update stream** that any of them can apply.

Three things are already in place. The resolution projection re-resolves an entity whenever any contributing record changes and bumps `resolved_version` ([0013](0013-postgres-storage.md) §5.1); the diff of [0012](0012-api-requirements.md) §7 already computes the truthy triples added and removed by a change; and every dump is a projection of a known point in the log. What is missing is the **delta** between two resolved states as triples, kept long enough for a consumer to fetch, ordered so that consumers agree, and serialized as Update requests.

Three choices James made in review shape the rest: the stream is **selectable per subscription** (the resolved view alone, or with source and metadata graphs); delivery is **pull**, with a sync client the operator runs, so the instance never holds a consumer's credentials; and blank nodes are **skolemized** in the stream and in the dump it continues from, so every delete is `DELETE DATA`.

## Decision

### 1. The stream is the dump, kept current (extends 0001 §2 and 0013 §8)

*Changed by A2.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §6.1.*

### 2. The delta is computed when the view changes (extends 0002 §2–3 and 0013 §7)

*Changed by A5.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §6.2.*

### 3. Storage: `view.rdf_delta` (extends 0013 §5.6)

*Changed by A2.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.11, §5.*

### 4. Tenants: shared deltas and overlay deltas (uses 0018 §6)

*Current text: [08](../architecture/08-tenants-and-instances.md) §10.5.*

### 5. Erasure and hiding reach the consumer, and the store (extends 0006 §7 and 0014 §5)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §6.3.*

### 6. Delivery: pull, with a sync client (extends 0012 §5)

*Changed by A2, A4, A7.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §6.4, §6.5.*

### 7. Blank nodes are skolemized (extends 0013 §8)

*Changed by A2.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §4.3.*

### 8. What this stream is not

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §6.6.*

### 9. Crates (amends 0005 §2 and §4.4)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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

### A4. The query service

- **Date:** 2026-10-04
- **Source:** [0059](0059-query-service.md) §1
- **Change:** extends §6
- **Summary:** The local quad store, a consumer of `view.rdf_delta`, now has one reader: the query service of 0059, which answers compiled queries and raw SPARQL from it. Nothing about the stream or its deltas changes.

### A5. Who renders each side of a delta

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §2
- **Summary:** `scatter-wikibase-rdf` renders the resolved-view and source-graph (`resolved`, `local`, `mirror/{provider}`) sides of a delta; `triplespace-rdf` renders the metadata-graph sides, since the revision, actor, event and provenance nodes are its vocabulary ([0013](0013-postgres-storage.md) §8). §2 had named `scatter-wikibase-rdf` for every side. (PENDING A19)

Replaced text (§2):

> Each side is rendered to triples by `scatter-wikibase-rdf` ([wikibase-compat.md](../api/wikibase-compat.md) §4–5) and the delta is the set difference, so a change that touches one statement of a large entity is a few triples, not the entity.

### A6. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [08](../architecture/08-tenants-and-instances.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A7. The sync client's marker triple

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §6
- **Summary:** Beside committing its cursor to its state file, `sparql-sync` writes a marker triple `<{base}/.well-known/query> scatter:cursor "{epoch}:{seq}"` in the store's default graph after each batch, which the query service reads to check the endpoint's position ([0059](0059-query-service.md) §2). The two were compatible but §6 did not say so. (PENDING T1)
