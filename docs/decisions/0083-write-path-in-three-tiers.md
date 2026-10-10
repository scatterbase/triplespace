# 0083. The write path in three tiers: recording, composition, consumers

- **Status:** Proposed
- **Date:** 2026-10-09
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0020](0020-change-feeds.md), [0022](0022-federation.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0030](0030-edit-filters.md), [0031](0031-property-constraints.md), [0032](0032-sparql-update-stream.md), [0038](0038-page-metadata-and-categories.md), [0039](0039-files-and-media.md), [0042](0042-template-expansion-and-parsoid.md), [0043](0043-lua-modules.md), [0047](0047-special-pages.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [0056](0056-security-model.md), [0059](0059-query-service.md), [0060](0060-scopes.md), [0061](0061-sprints-and-tasks.md), [0064](0064-entityschema-and-validation.md), [0067](0067-proposals.md), [0070](0070-shallow-entity-mirroring.md), [0071](0071-derived-statements-from-mirrored-pages.md)
- **Uses:** [0082](0082-source-form-and-the-shared-view.md), [0084](0084-wikibase-writes-against-the-resolved-view.md)

## Context

[0013](0013-postgres-storage.md) §7 made an interactive write one transaction: append, ID allocation, the base check and "projections 1–5 for the affected keys" commit together, under the partition's row lock, with a budget (1,000 rows or 250 ms) after which fan-out is queued. That was right for one tenant and a few projections. Every later ADR then attached its own work to the same point: constraint checks ([0031](0031-property-constraints.md) §2), the RDF delta ([0032](0032-sparql-update-stream.md) §2), scope membership ([0060](0060-scopes.md) §5), tasks ([0061](0061-sprints-and-tasks.md) §6), schema validation ([0064](0064-entityschema-and-validation.md) §5), entity-usage classification ([0043](0043-lua-modules.md) §10), report entries ([0047](0047-special-pages.md) §4.3), per-resolution ACL lookups ([0056](0056-security-model.md) §14), and the overlay recomputation of every tenant on every shared record ([0013](0013-postgres-storage.md) §5). The chapters ([03](../architecture/03-storage-caches-and-search.md) §6) put the whole chain in one place, and the review of 2026-10-09 ([docs/audits/architecture-review-2026-10-09.md](../audits/architecture-review-2026-10-09.md)) read it against the core use case, a Wikidata `full` mirror re-synced weekly beside local editing and OpenAlex.

What it found: a budget bounds one editor's latency, not the chain; the lock is held through every synchronous projection, so one tenant's interactive edits serialize behind each other's projections at single digits per second under load (F-01-8); the only bulk mode is bootstrap into an empty partition, so the paths the next phase will actually walk — `linked` → `all`, weekly re-syncs of millions of entities, adding a provider to a live instance — go through the interactive path one record at a time (F-04-2, F-01-6); the current state of a delta graph is stored nowhere, so re-resolution and the delta replay a key's history inside the transaction (F-01-2); concurrency between the inline path and the worker, and the L1 write before commit, are unspecified (F-01-9); and "a rebuild truncates and replays from offset 0" is unaffordable for a mirror, unsafe for the two projections that have side effects, and silent about replay order across partitions (F-19-2, F-10-4, F-13-4, F-10-5, F-07-7, F-13-7). The stream's cursor is not commit-ordered, the activity stream's cursor is ordered by source time, and the delta table is both "rebuilt from the log" and "stored, not recomputed" (F-02-2, F-02-3, F-16-4).

James named the genre: "writes where Wikidata items are nearby have big downstream consequences", and asked for ways to uncouple the write path from the graph composition path. This ADR is that uncoupling. [0082](0082-source-form-and-the-shared-view.md) removed the worst link of the chain (referrers never need recomposition for identity); this ADR removes composition from the transaction and makes it a pure function, then hangs everything else off composition instead of off writes.

### Direction

James's direction, from the design review of 2026-10-09:

- **"I think one genre of problem you're describing is 'writes where Wikidata items are nearby have big downstream consequences'. Let's talk about that issue more, including ways we can uncouple the write path from the graph composition path."**
- On the three tiers as proposed: **"I think your proposed solution is sound."**

## Decision

### 1. Tier 1: the appending transaction records (amends 0002 §8, 0005 §4.2, 0013 §2 and §7 and 0030 §7; extends 0006 §8, 0014 §4, 0015 §2, 0024 §5 and 0030 §1, §2 and §5)

**The appending transaction does only what is a function of the written record and the local graph.** In order: authenticate and resolve the actor's effective permissions; rate limit; ACLs on the target and its enclosures; edit filters; normalization ([0082](0082-source-form-and-the-shared-view.md) §2) and the diff of [0084](0084-wikibase-writes-against-the-resolved-view.md) §1; the base check against the local graph ([0084](0084-wikibase-writes-against-the-resolved-view.md) §5); ID allocation; the append; the written graph's new state in `view.graph_state` (§4); the activity row; commit. **The partition lock of [0013](0013-postgres-storage.md) §2 covers exactly this**, so gapless offsets are kept and the interactive ceiling is the append rate, not the projection rate.

**Nothing composes inside the transaction.** [0013](0013-postgres-storage.md) §7's "projections 1–5 for the affected keys before commit" is withdrawn; the projections of its step 1 (registry, surrogates, actors, groups, memberships, ACLs, blocks, filters) remain inline because the write path reads them, and they are functions of the record alone.

**Edit filters stay in tier 1, scoped.** A filter's scope excludes job writes by default (`jobs = false`); a filter that opts in sees them, and hits from job writes aggregate into one hit record per (filter, job) carrying a count and the first N record coordinates, as the job's rejects already do ([0030](0030-edit-filters.md) §7). The `rate` variable is computed lazily. The `read` rate class ([0024](0024-subsidiary-accounts.md) §5) is counted for authenticated principals once a per-process count passes a fraction of the limit, and never for a response served from L0 or L1.

### 2. Tier 2: composition is a pure function (amends 0013 §5 and §7, 0014 §3 and 0071 §7 and §12; extends 0004 §4 and 0014 §4)

**Composition** is the resolver producing an entity's fused body — the resolved view of [0002](0002-source-graphs-and-mass-ingest.md) §3 — from the contributing graphs' states under the instance policy and the cluster map. It is **a deterministic function of its inputs**: the offset of each contributing graph's state (`view.graph_state` for delta graphs, `view.entity_source` for mirror graphs), the offset of the policy record ([0082](0082-source-form-and-the-shared-view.md) §5), and the cluster map version. The composed row records those inputs as **`composed_from`**, so staleness is a comparison and the same function, run anywhere, produces the same bytes.

**Where it runs.**

- **After commit, for the written entity.** The write response composes that one entity inline — no fan-out, no lock held — and returns the body, so read-your-writes holds for the thing edited.
- **In the composition worker, for everything else:** the other members of a changed cluster, the overlays of tenants that hold one, the entities a mirror block touched. Work is keyed by entity and **coalesced**: a mirror `put` and a local edit on one key in the same second compose once.
- **On miss, on the read path.** If `view.entity`'s `composed_from` is behind the key's newest graph state, the read composes now and stores the result. `view.entity` is a cache of composition over the graph states, which are the truth.
- **Set-based in bootstrap and bulk modes** (§5), one pass over the loaded states rather than one call per entity.

**Replacement by inputs, not by lock.** A composed row is replaced only when the new inputs are newer than `composed_from`; two composers meeting on one key cannot overwrite a newer result with an older one, which is what [0013](0013-postgres-storage.md) §7's per-key race needed `SELECT … FOR UPDATE` for. **`resolved_version`** is drawn from one instance-wide sequence, so a rolled-back version is never reused. **L1 is written by the composer after commit** ([0014](0014-caches-and-search.md) §4), never by the write path before it.

### 3. Tier 3: consumers of composition (amends 0013 §7, 0014 §7, 0031 §2, 0032 §2, 0042 §10, 0043 §10, 0047 §4.2 and §4.3, 0056 §14, 0060 §5 and §10, 0061 §6 and 0064 §5; extends 0014 §5, 0060 §4 and 0061 §11)

**Everything that is a function of a composed entity consumes composition events.** The composer emits "entity X, tenant T, composed to version N" after each composed row commits; each consumer has its own queue, rate and lag, reports lag on its own pages, runs in no one's transaction, and is **off during bootstrap and bulk modes** until the operator turns it on. The consumers, each with the rule the review asked for:

- **Constraints** ([0031](0031-property-constraints.md) §2). `constraints.scope` is a `site` setting: `local` (the default: local-graph statements and entities in the tenant's overlay), `overlay`, or `all`, and `all` is a batch job with progress, never a step. A constraint-statement change on a mirrored property enqueues a batch rather than a scan. `type` and `value type` are checked with the L0 class cache, and the chapter states that a `P279` change does not invalidate stored results until the next composition of the subject.
- **Scopes** ([0060](0060-scopes.md) §5). A scope definition compiles at save to rows of **`view.scope_trigger (tenant, trigger_kind, key, scope_page_id)`** — `property:P31`, `category:{page}`, `table-row:{page}`, `sitelink` — held in L0, so a composition event that matches no trigger is one hash probe. A truncated scope is "the first N found, refilled by a job when it falls below N"; an `intersection` or `difference` whose non-probeable operand is truncated is refused at save; a scope's initial computation is always a job.
- **Sprints and tasks** ([0061](0061-sprints-and-tasks.md) §6) follow the scope projection as before, from events.
- **Schema validation** ([0064](0064-entityschema-and-validation.md) §5) never runs inline or in a fan-out budget; only the ad hoc check does. Binding a schema to a scope is refused at scope save when bound-schema × scope size exceeds a per-tenant cap, unless an operator raises it.
- **Entity usage** ([0043](0043-lua-modules.md) §10) checks `view.entity_usage` for the canonical ID and every member of its cluster by index **before** computing an aspect diff, and does nothing while no tenant on the instance has `wikitext.expansion` on.
- **Reports** ([0047](0047-special-pages.md) §4.3) consume events; an entity report triggers only when a canonical's row changes and the cluster has a local member, and its freshness is bounded by composition lag, shown beside "computed at".
- **The RDF delta** ([0032](0032-sparql-update-stream.md) §2) is a consumer behind **`updates.enabled`** (default off), reading old state from `view.graph_state` and the previous composed row; it is not computed in any write (§7).
- **Search documents** ([0014](0014-caches-and-search.md) §5) are built from composed rows by a consumer, with external versioning as before.
- **ACL and visibility re-projection** ([0056](0056-security-model.md) §14) consumes events; the statement and property `read`-ACL lookup during composition is skipped while a per-tenant counter of live such ACLs in `view.tenant` is zero, and shared-row composition never looks.

### 4. Graph state and revision state (extends 0002 §7, 0008 §4, 0012 §2.1 and §4, 0013 §5 and 0015 §2; amends 0012 §7)

**`view.graph_state (tenant, entity_id, graph, state bytea, "offset", version)`** holds the current state of every graph whose records are deltas — `local`, `derived/*`, `source/*` — written in tier 1 by the appending transaction (one row per written key), and is the "old state" that composition and the delta read. A mirror graph's state is its latest `put`, which `view.entity_source` already names.

**`view.entity_revision (tenant, revid, state bytea)`** holds the local-graph state after every revision of the `local` and `pages` partitions, written in tier 1 beside `graph_state`, so that `oldid=`, `action=compare`, `GET /entity/{id}/compare`, `?as_of=`, `Special:PermanentLink` and export read one row ([0012](0012-api-requirements.md) §4, §7). It is MediaWiki's `text` table at local scale: local edits are the small part of a mirror-dominated instance, and the rows pack well. Mirror states are not reconstructable under `latest`, as [0012](0012-api-requirements.md) §7 already said for the resolved view.

### 5. Bulk append into a live partition, and catch-up (amends 0013 §2 and §9; extends 0002 §8 and 0070 §7)

**A third mode between interactive and bootstrap.** A bulk job writing into a partition that already has its indexes and projections takes the partition lock **once per block** (`ops.bootstrap_block` has the shape), `COPY`s the block's records, folds the block's leaves into the tree from the in-memory frontier, writes the block's graph states, and commits; tier 2 then composes the block set-based, and `ops.projection_state` advances per block. **The `put` skip** (the version cursor of [0002](0002-source-graphs-and-mass-ingest.md) §8.4) is evaluated in bulk against `view.entity_source` before a block is written, so a re-sync's unchanged entities never reach the lock. `stream` jobs are one transaction per block, not per entity.

**Per-partition catch-up.** A provider added to a live instance is loaded into its own partition in bootstrap mode ([0013](0013-postgres-storage.md) §9), then composed against the existing `view`: its tier-2 links into other providers' entities and the fusion of clusters it joins are tier-2 work queued for the composer. Bootstrap mode is for an empty partition; catch-up is for a new partition on a live instance; bulk append is for a live partition.

### 6. Projections: classes, replay and rebuild (amends 0008 §10, 0013 §7, 0023 §6, 0038 §3, 0039 §9, 0042 §9 and §10, 0047 §4.3, 0055 §6, 0060 §5, 0061 §6, 0064 §5 and 0067 §5; extends 0006 §3, 0014 §4 and §10 and 0039 §20)

**Every projection declares its class.** *Log-replayed* projections replay from offset 0 as [0013](0013-postgres-storage.md) §7 described. *View-derived* projections (reports, constraints, `site_stats`, scopes, tasks, schema reports, entity usage) are populated by a scan of the named `view` tables they read and never by a log replay; "rebuild projection P for tenant T from `view`" is a named operation. A projection added after a mirror exists is view-derived or it does not exist.

**No projection has side effects during replay.** `view.blob` ([0039](0039-files-and-media.md) §20) is a pure count; byte destruction is an `ops` job that takes a hash whose count reached zero, re-checks the count at the live head under a lock, and deletes. Proposal adoption ([0067](0067-proposals.md) §5) writes `view.proposal.state` and enqueues the retirement; a job appends the `remove` with the proposal thread and upstream revision as an idempotency key in its content part after checking the current composed state. A rebuild re-derives the tables and re-runs no job; the `ops` queues are not projection targets and are not truncated.

**Rebuild by swap.** A projection whose tables are large rebuilds into `view_next` and swaps; every rebuild bumps an instance-wide **cache epoch** carried in every L1 key ([0014](0014-caches-and-search.md) §4), so a key from before the rebuild never matches a body after it.

**Replay order within a tenant.** A tenant's partitions replay in `(appended_at, partition, offset)` order, with `appended_at` set once per appending transaction so records one transaction wrote to two partitions ([0051](0051-page-redirects.md) §2's move over a redirect) replay together. The title index is rebuilt with `deleted` applied in a pre-pass over the ACL partition, so no replay order can hit the partial unique index.

**Decisions from other partitions' state are written down.** Where the write path decides something from the state of another partition at the time — autopatrol from memberships ([0023](0023-moderation.md) §6), the operator of a transfer — the decision goes into the attestation part of the record, and the projection reads it rather than reconstructing "as of" by timestamp across partitions, which a vanish would make unreconstructible.

**Lag is `ops.projection_work`.** `ops.projection_work` is the unit of truth for lag; `ops.projection_state.applied_offset` records the contiguous prefix, with inline-recorded records entered as done.

**One writer per row class.** `view.page_category`, `view.page_link` and `view.page_prop` ([0038](0038-page-metadata-and-categories.md) §2, [0055](0055-templatestyles-templatedata-and-page-properties.md) §6, [0042](0042-template-expansion-and-parsoid.md) §10) carry `from_render boolean`: the page projection writes source-derived rows (what the stored text says) and the refresh job writes expansion-derived rows, each owning its class exclusively; readers union them; turning `wikitext.expansion` off truncates the render-owned rows and nothing else; the refresh job never deletes a row it did not write.

### 7. Stream mechanics (amends 0020 §4 and 0032 §2, §3 and §5; extends 0012 §2.3, 0020 §1, 0022 §2, 0032 §6 and 0059 §3)

**`rdf_delta.seq` is assigned by a single sequencer**, the delta consumer of §3, so insert order is commit order and a consumer reading `seq > cursor` never skips a late-committing row; "the order projections applied them" is withdrawn. **A rebuild truncates `rdf_delta`** and bumps the epoch; the delta is stored, not recomputed, and consumers reload from the post-rebuild dump as [0059](0059-query-service.md) §7 already has them do. **The epoch carries a rendering version** that a change to `scatter-wikibase-rdf`, `triplespace-rdf` or a normalizer increments, because `DELETE DATA` matches only byte-identical triples; the lexical forms are pinned as a fixture in `wikibase-compat.md`. **Each tenant has its own epoch** (`ops.tenant_epoch`), bumped by a provider-list change, an isolation change and a base alias, so a tenant that opts in to a provider tells only its own consumers to reload. **`rdf_delta` is indexed by `(partition, "offset")`** so erasure purge reaches metadata-only rows.

**The activity stream's resume token** ([0020](0020-change-feeds.md) §4) is a vector of per-partition high-water marks, which offsets already are under the append lock; `(time, partition, offset)` stays the display order of paged lists, where MediaWiki's `rccontinue` has the same edge. The verified sync of [0022](0022-federation.md) §2 resumes from that token.

### 8. Crates (extends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-ingest` | Tier 1 as §1; the bulk-append mode and block coordinator of §5; `graph_state` and `entity_revision` writes |
| `scatter-wikibase-resolve` | Composition as a pure function with `composed_from`; coalescing; set-based composition |
| `scatter-projection` | Projection classes (log-replayed, view-derived), the side-effect-free rule, rebuild by swap, the cache epoch, replay order |
| `triplespace-projections` | The composition worker; compose-on-miss; the tier-3 consumers and their queues; `view.scope_trigger`; `from_render` on the page tables; the single-sequencer delta consumer; per-tenant epochs |
| `triplespace-api-action`, `triplespace-api-rest` | Inline composition of the written entity for the write response; the per-partition activity resume token |
| `triplespace-db` | `view.graph_state`, `view.entity_revision`, `view.scope_trigger`, `ops.tenant_epoch`, the `rdf_delta` coordinate index, `composed_from` and `policy` columns |

## Alternatives considered

- **Keep composition in the transaction and raise the budget.** The budget bounds latency, not throughput; the lock would still be held through composition; and every later consumer would attach to the same point again.
- **Compose inline for the written entity only, before commit.** Read-your-writes without the fan-out, but the lock is still held through one composition, and the L1 write before commit stays a correctness bug on rollback.
- **A per-tenant commit sequence for replay order.** Exact; it needs a sequence taken at commit, which Postgres does not give, or a sequencer in front of every append. `appended_at` per transaction with partition and offset as tie-breaks is deterministic and causal for the cases that matter.
- **Serve rows below the oldest in-flight transaction as the stream cursor.** The outbox pattern; works with concurrent writers and needs a `txid` column, but with the delta already a single consumer, a single sequencer is simpler and also lets the stream be served from a replica.

## Consequences

- **An interactive write holds the partition lock for the append alone.** The ceiling becomes the append rate; MediaWiki-shaped bot batches no longer wait behind projections.
- **A cold read may compose.** Immediately after a sync, or after a write to another member of the same cluster, a read can pay one entity's composition rather than finding a row; the worker racing ahead makes this rare.
- **Markers appear within tier-3 lag, not on reload.** Constraint markers, scope membership and report entries follow composition by milliseconds for one entity and honestly for a sync; [0031](0031-property-constraints.md) §3's "on reload" is withdrawn.
- **Bootstrap and bulk modes are simply "tier 1 in blocks, tier 2 set-based, tier 3 off"**, which is what `adoption-results-and-wikidata-projection.md` asked for in other words.
- **Every projection must be classified** when it is written, and a view-derived one must name the tables it reads.
- **Milestone 1's code moves composition out of the transaction** at stage 3; nothing in the log changes, since `graph_state` and `entity_revision` are projections.

## Open questions

- **Q1. Compose-on-miss under load.** Whether a read that finds a stale row should compose synchronously, serve the stale row with a header and enqueue, or do either by a threshold on how far behind the row is.
- **Q2. `entity_revision` retention.** Whether revision states of erased or very old revisions are compacted into periodic snapshots once packed storage ([0058](0058-packed-record-storage.md)) exists.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §7 | §4 | extends | 0002 A32 |
| [0002](0002-source-graphs-and-mass-ingest.md) §8 | §1, §5 | amends | 0002 A32 |
| [0004](0004-identity-clusters-and-equivalence.md) §4 | §2 | extends | 0004 A17 |
| [0005](0005-crate-organization.md) §2 | §8 | extends | 0005 A90 |
| [0005](0005-crate-organization.md) §4.2 | §1 | amends | 0005 A90 |
| [0006](0006-log-integrity-and-erasure.md) §3 | §6 | extends | 0006 A20 |
| [0006](0006-log-integrity-and-erasure.md) §8 | §1 | extends | 0006 A20 |
| [0008](0008-namespaces-and-document-pages.md) §4 | §4 | extends | 0008 A34 |
| [0008](0008-namespaces-and-document-pages.md) §10 | §6 | amends | 0008 A34 |
| [0012](0012-api-requirements.md) §2.1 | §4 | extends | 0012 A65 |
| [0012](0012-api-requirements.md) §2.3 | §7 | extends | 0012 A65 |
| [0012](0012-api-requirements.md) §4 | §4 | extends | 0012 A65 |
| [0012](0012-api-requirements.md) §7 | §4 | amends | 0012 A65 |
| [0013](0013-postgres-storage.md) §2 | §1, §5 | amends | 0013 A53 |
| [0013](0013-postgres-storage.md) §5 | §2, §4 | amends | 0013 A53 |
| [0013](0013-postgres-storage.md) §7 | §1, §2, §3, §6 | amends | 0013 A53 |
| [0013](0013-postgres-storage.md) §9 | §5 | amends | 0013 A53 |
| [0014](0014-caches-and-search.md) §3 | §2 | amends | 0014 A25 |
| [0014](0014-caches-and-search.md) §4 | §1, §2, §6 | extends | 0014 A25 |
| [0014](0014-caches-and-search.md) §5 | §3 | extends | 0014 A25 |
| [0014](0014-caches-and-search.md) §7 | §3 | amends | 0014 A25 |
| [0014](0014-caches-and-search.md) §10 | §6 | extends | 0014 A25 |
| [0015](0015-record-format-and-partition-registry.md) §2 | §1, §4 | extends | 0015 A49 |
| [0020](0020-change-feeds.md) §1 | §7 | extends | 0020 A15 |
| [0020](0020-change-feeds.md) §4 | §7 | amends | 0020 A15 |
| [0022](0022-federation.md) §2 | §7 | extends | 0022 A11 |
| [0023](0023-moderation.md) §6 | §6 | amends | 0023 A12 |
| [0024](0024-subsidiary-accounts.md) §5 | §1 | extends | 0024 A16 |
| [0030](0030-edit-filters.md) §1 | §1 | extends | 0030 A13 |
| [0030](0030-edit-filters.md) §2 | §1 | extends | 0030 A13 |
| [0030](0030-edit-filters.md) §5 | §1 | extends | 0030 A13 |
| [0030](0030-edit-filters.md) §7 | §1 | amends | 0030 A13 |
| [0031](0031-property-constraints.md) §2 | §3 | amends | 0031 A7 |
| [0032](0032-sparql-update-stream.md) §2 | §3, §7 | amends | 0032 A9 |
| [0032](0032-sparql-update-stream.md) §3 | §7 | amends | 0032 A9 |
| [0032](0032-sparql-update-stream.md) §5 | §7 | amends | 0032 A9 |
| [0032](0032-sparql-update-stream.md) §6 | §7 | extends | 0032 A9 |
| [0038](0038-page-metadata-and-categories.md) §3 | §6 | amends | 0038 A15 |
| [0039](0039-files-and-media.md) §9 | §6 | amends | 0039 A11 |
| [0039](0039-files-and-media.md) §20 | §6 | extends | 0039 A11 |
| [0042](0042-template-expansion-and-parsoid.md) §9 | §6 | amends | 0042 A17 |
| [0042](0042-template-expansion-and-parsoid.md) §10 | §3, §6 | amends | 0042 A17 |
| [0043](0043-lua-modules.md) §10 | §3 | amends | 0043 A8 |
| [0047](0047-special-pages.md) §4.2 | §3 | amends | 0047 A18 |
| [0047](0047-special-pages.md) §4.3 | §3, §6 | amends | 0047 A18 |
| [0055](0055-templatestyles-templatedata-and-page-properties.md) §6 | §6 | amends | 0055 A3 |
| [0056](0056-security-model.md) §14 | §3 | amends | 0056 A13 |
| [0059](0059-query-service.md) §3 | §7 | extends | 0059 A10 |
| [0060](0060-scopes.md) §4 | §3 | extends | 0060 A9 |
| [0060](0060-scopes.md) §5 | §3, §6 | amends | 0060 A9 |
| [0060](0060-scopes.md) §10 | §3 | amends | 0060 A9 |
| [0061](0061-sprints-and-tasks.md) §6 | §3, §6 | amends | 0061 A6 |
| [0061](0061-sprints-and-tasks.md) §11 | §3 | extends | 0061 A6 |
| [0064](0064-entityschema-and-validation.md) §5 | §3, §6 | amends | 0064 A4 |
| [0067](0067-proposals.md) §5 | §6 | amends | 0067 A7 |
| [0070](0070-shallow-entity-mirroring.md) §7 | §5 | extends | 0070 A7 |
| [0071](0071-derived-statements-from-mirrored-pages.md) §7 | §2 | amends | 0071 A4 |
| [0071](0071-derived-statements-from-mirrored-pages.md) §12 | §2 | amends | 0071 A4 |

## References

- [docs/audits/architecture-review-2026-10-09.md](../audits/architecture-review-2026-10-09.md): findings F-01-2, F-01-5 to F-01-9, F-02-2, F-02-3, F-02-5, F-02-8, F-02-9, F-04-2, F-06-1, F-07-3, F-07-7 to F-07-10, F-10-2 to F-10-5, F-13-4, F-13-5, F-13-7, F-13-8, F-16-3, F-16-4, F-19-2; decisions G8–G15
- `adoption-results-and-wikidata-projection.md` (project notes, 2026-10-06): the measured per-entity path and the four things a Wikidata-in-a-night load needs
- MediaWiki's `text` table and parser cache: the precedents for `view.entity_revision` and compose-on-miss
