# 0083. The write path in three tiers: recording, composition, consumers

- **Status:** Proposed
- **Date:** 2026-10-09
- **Updated:** 2026-10-30 (A5)
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

*Current text: [00](../architecture/00-overview.md) §2.1, §5; [01](../architecture/01-log-and-records.md) §1.3, §7; [03](../architecture/03-storage-caches-and-search.md) §2.1, §4.6, §4.13, §6.2, §12.3; [05](../architecture/05-providers-and-ingest.md) §3.6; [07](../architecture/07-actors-and-accounts.md) §6.2, §6.3; [09](../architecture/09-security-and-moderation.md) §7.1, §7.2, §7.5; [11](../architecture/11-rendering-templates-and-modules.md) §4.1; [22](../architecture/22-crates-and-stack.md) §4.3.*

### 2. Tier 2: composition is a pure function (amends 0013 §5 and §7, 0014 §3 and 0071 §7 and §12; extends 0004 §4 and 0014 §4)

*Current text: [00](../architecture/00-overview.md) §2.1, §5; [03](../architecture/03-storage-caches-and-search.md) §4.2, §5, §6.1, §6.2, §9.3, §12.1; [13](../architecture/13-mirrored-pages.md) §6.6; [18](../architecture/18-api.md) §2.5.*

### 3. Tier 3: consumers of composition (amends 0013 §7, 0014 §7, 0031 §2, 0032 §2, 0042 §10, 0043 §10, 0047 §4.2 and §4.3, 0056 §14, 0060 §5 and §10, 0061 §6 and 0064 §5; extends 0014 §5, 0060 §4 and 0061 §11)

*Changed by A3, A4.*

*Current text: [00](../architecture/00-overview.md) §2.1, §2.2, §5; [02](../architecture/02-graphs-rdf-and-query.md) §6.2; [03](../architecture/03-storage-caches-and-search.md) §4.4, §4.6, §4.10, §4.11, §4.14, §4.15, §5, §6.1, §6.2, §11.3; [06](../architecture/06-statements-and-properties.md) §2.2; [08](../architecture/08-tenants-and-instances.md) §2.5; [09](../architecture/09-security-and-moderation.md) §4.2, §4.4; [11](../architecture/11-rendering-templates-and-modules.md) §4.4, §7.9; [15](../architecture/15-structured-pages.md) §1.3, §1.4, §3.5, §6.4; [21](../architecture/21-special-pages.md) §2.2, §2.3; [23](../architecture/23-configuration-and-registry.md) §3.2.*

### 4. Graph state and revision state (extends 0002 §7, 0008 §4, 0012 §2.1 and §4, 0013 §5 and 0015 §2; amends 0012 §7)

*Changed by A1, A2.*

*Current text: [00](../architecture/00-overview.md) §5; [01](../architecture/01-log-and-records.md) §2.5; [03](../architecture/03-storage-caches-and-search.md) §4.2, §5; [05](../architecture/05-providers-and-ingest.md) §2.5; [10](../architecture/10-pages-and-content-models.md) §3.2; [18](../architecture/18-api.md) §1.3, §2.3, §3.2, §5.1, §5.2.*

### 5. Bulk append into a live partition, and catch-up (amends 0013 §2 and §9; extends 0002 §8 and 0070 §7)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §2.1, §6.2; [05](../architecture/05-providers-and-ingest.md) §3.10.*

### 6. Projections: classes, replay and rebuild (amends 0008 §10, 0013 §7, 0023 §6, 0038 §3, 0039 §9, 0042 §9 and §10, 0047 §4.3, 0055 §6, 0060 §5, 0061 §6, 0064 §5 and 0067 §5; extends 0006 §3, 0014 §4 and §10 and 0039 §20)

*Changed by A3, A4.*

*Current text: [01](../architecture/01-log-and-records.md) §2.4; [03](../architecture/03-storage-caches-and-search.md) §4.5, §4.6, §4.7, §4.10, §5, §6.1, §9.4, §12.1; [09](../architecture/09-security-and-moderation.md) §6.4; [10](../architecture/10-pages-and-content-models.md) §5.2, §5.3, §5.5; [11](../architecture/11-rendering-templates-and-modules.md) §4.1, §4.3; [12](../architecture/12-files-and-media.md) §5.3; [14](../architecture/14-discussions.md) §5.4; [15](../architecture/15-structured-pages.md) §1.4, §3.5, §3.6, §6.4; [21](../architecture/21-special-pages.md) §2.3.*

### 7. Stream mechanics (amends 0020 §4 and 0032 §2, §3 and §5; extends 0012 §2.3, 0020 §1, 0022 §2, 0032 §6 and 0059 §3)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §6.2, §6.3, §6.4, §7.3; [03](../architecture/03-storage-caches-and-search.md) §4.6, §4.11, §5; [16](../architecture/16-logs-feeds-and-notifications.md) §4.4; [17](../architecture/17-federation-and-publication.md) §1.2.*

### 8. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2, §3.1.*

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

## References

- [docs/audits/architecture-review-2026-10-09.md](../audits/architecture-review-2026-10-09.md): findings F-01-2, F-01-5 to F-01-9, F-02-2, F-02-3, F-02-5, F-02-8, F-02-9, F-04-2, F-06-1, F-07-3, F-07-7 to F-07-10, F-10-2 to F-10-5, F-13-4, F-13-5, F-13-7, F-13-8, F-16-3, F-16-4, F-19-2; decisions G8–G15
- `adoption-results-and-wikidata-projection.md` (project notes, 2026-10-06): the measured per-entity path and the four things a Wikidata-in-a-night load needs
- MediaWiki's `text` table and parser cache: the precedents for `view.entity_revision` and compose-on-miss

## Amendment log

### A1. The fold as the stepping stone to `view.entity_revision`

- **Date:** 2026-10-16
- **Source:** Direct: James, review of the activity-feeds pull requests, 2026-10-15
- **Change:** extends §4
- **Summary:** `oldid=`, `prop=revisions` with content, `GET /entity/{id}/changes/{partition}/{offset}` and `GET /entity/{id}/compare` are served before the tier-1 state of §4 is written. Until `view.entity_revision` exists, they read a past local state by folding the subject's local change-set records up to the revision (`triplespace_projections::revisions`): the fold `read::current` already performs for the latest state, bounded by the subject's own local record count, which on a mirror-dominated instance is small. The readers hold the signatures §4 gives them — a revision in, a `LocalState` out — so that the table, when it lands with the rest of the tier-1 state, replaces their backing and changes nothing they serve. §4's rule stands as the design: a past state is one row, never a replay; this entry records the stepping stone and when it ends.

### A2. `view.graph_state` and `view.entity_revision` land; the fold becomes the fallback

- **Date:** 2026-10-16
- **Source:** Direct: James, review of the activity-feeds pull requests, 2026-10-15
- **Change:** amends §4
- **Summary:** Migration `0010_revision_state` creates both tables and the `revision_state` projection (step 1, `Step::Registry`) writes them in the appending transaction for the `local` partition: the key's `graph_state` row is read, the record's operation applied, the row written back with `version` bumped and the record's offset as the graph's latest, and the state after the revision inserted into `entity_revision` under the record's revision ID. `entity_revision` carries `entity_id`, `partition` and `"offset"` beyond §4's `(tenant, revid, state)`: the record is the row's natural second key (a diff page resolves a revision through its `{partition, offset}`, [0012](0012-api-requirements.md) A74), and the state before a revision is the row at the previous offset the stored state's `offsets` list names, so a partially filled table never pairs a revision with a stale "before". The stored form is `LocalState`'s storage JSON — the local assertions, corrections, retention, redirect, conversion and the offsets folded — as bytes, uncompressed for now; it is a serving table's cell, re-derived by a rebuild, and nothing hashes it. `read::current` reads the `graph_state` row and the one record at its offset where the state is one whole record, else folds. A1's fold ends as the primary path and stays as the fallback: a database migrated to the tables keeps answering for revisions appended before the migration, and a key with no `graph_state` row is seeded from its earlier records on its next write rather than started at that write, so a rebuild is an optimization, not a prerequisite. The `pages` partition joins when the pages stack writes to it; `derived/*` and `source/*` join with their writers; compression of `state` is open until the rows are measured.

Replaced text (§4, as it stood):

> **`view.entity_revision (tenant, revid, state bytea)`** holds the local-graph state after every revision of the `local` and `pages` partitions, written in tier 1 beside `graph_state`, so that `oldid=`, `action=compare`, `GET /entity/{id}/compare`, `?as_of=`, `Special:PermanentLink` and export read one row ([0012](0012-api-requirements.md) §4, §7). It is MediaWiki's `text` table at local scale: local edits are the small part of a mirror-dominated instance, and the rows pack well. Mirror states are not reconstructable under `latest`, as [0012](0012-api-requirements.md) §7 already said for the resolved view. **Until this table exists, the past-state readers fold the subject's local records** (`triplespace_projections::revisions`, A1): the same fold the latest state already uses, bounded by the subject's own local record count, behind the signatures this section gives them — a revision in, a state out — so that landing the table changes their backing and nothing they serve.

### A3. Expansion has no setting

- **Date:** 2026-10-20
- **Source:** [0042](0042-template-expansion-and-parsoid.md) A19
- **Change:** amends §3, §6
- **Summary:** `wikitext.expansion` is withdrawn, so entity usage is idle while no page uses an entity rather than while no tenant has expansion on, and the one-writer-per-row-class rule loses its clause about turning expansion off. The row classes and their writers are unchanged.

Replaced text (§3, as it stood):

> - **Entity usage** ([0043](0043-lua-modules.md) §10) checks `view.entity_usage` for the canonical ID and every member of its cluster by index **before** computing an aspect diff, and does nothing while no tenant on the instance has `wikitext.expansion` on.

Replaced text (§6, as it stood):

> **One writer per row class.** `view.page_category`, `view.page_link` and `view.page_prop` ([0038](0038-page-metadata-and-categories.md) §2, [0055](0055-templatestyles-templatedata-and-page-properties.md) §6, [0042](0042-template-expansion-and-parsoid.md) §10) carry `from_render boolean`: the page projection writes source-derived rows (what the stored text says) and the refresh job writes expansion-derived rows, each owning its class exclusively; readers union them; turning `wikitext.expansion` off truncates the render-owned rows and nothing else; the refresh job never deletes a row it did not write.

### A4. One generic worker over the work queue

- **Date:** 2026-10-29
- **Source:** Direct: James, design discussion of 2026-10-29
- **Change:** extends §3, §6
- **Summary:** The review of what runs synchronously found that no projection emitted fan-out, that nothing in a running instance consumed `ops.projection_work`, and that the first tier-3 consumer built (reports, [0047](0047-special-pages.md) §4.3) had a worker of its own that removed items before applying them and ran rebuilds inside a reader's request. James asked for a reusable worker as the first step of taking inessential work off the write path. §6 now states it: one queue and one generic worker for every projection's fan-out and every tier-3 consumer; a tenant's work for a consumer leased and applied by one worker at a time, in order; items removed in the unit of work that applied them; failed attempts recorded and bounded, the failed items reported rather than retried forever; several workers as tasks of the serving process, or a process of their own; reads that show lag and never do the work; lag reported per consumer and tenant in `GET /version`. The report consumer is the first on it; the projections' fan-out is on it the day one of them emits any. Open: a bulk mode that turns consumers off (§5) and the division of the synchronous steps themselves, which this worker is the prerequisite for.

### A5. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [00](../architecture/00-overview.md), [01](../architecture/01-log-and-records.md), [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [05](../architecture/05-providers-and-ingest.md), [06](../architecture/06-statements-and-properties.md), [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [10](../architecture/10-pages-and-content-models.md), [11](../architecture/11-rendering-templates-and-modules.md), [12](../architecture/12-files-and-media.md), [13](../architecture/13-mirrored-pages.md), [14](../architecture/14-discussions.md), [15](../architecture/15-structured-pages.md), [16](../architecture/16-logs-feeds-and-notifications.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
