# 0013. Postgres as the log store and serving model

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-09 (A51)
- **Author:** James Hare / Claude Fable
- **Changes:** [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0007](0007-actor-identity.md), [0009](0009-keyed-entity-types-and-domain.md), [0014](0014-caches-and-search.md)
- **Chapters:** [01](../architecture/01-log-and-records.md), [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [05](../architecture/05-providers-and-ingest.md), [22](../architecture/22-crates-and-stack.md)

## Context

The ADRs so far describe Triplespace's data as RDF. Source graphs hold quads ([0002](0002-source-graphs-and-mass-ingest.md) §2), the resolved view and the metadata graph are projections ([0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md) §3), and the shared storage contract is `lookup(s, p, o, g)` ([0005](0005-crate-organization.md) §4.4). RDF is the right model: it is what the Wikibase contract exports, what QLever loads, and what Scatterbase shares.

It is the wrong thing for the API and the site to read from. Everything the later ADRs ask the server to do is a keyed lookup or an ordered list, not a graph pattern:

- serve an entity's canonical JSON and the labels of the entities it links to ([0003](0003-statement-ui.md) §2);
- resolve a title, an ID, a domain key or a cluster member to a page ([0008](0008-namespaces-and-document-pages.md) §3, [0004](0004-identity-clusters-and-equivalence.md) §4);
- list history, recent changes, contributions and log events in one total order with continuation ([0010](0010-site-ui.md) §5–8, [0012](0012-api-requirements.md) §2.3);
- find an entity by an identifier value, for match keys and inferred links ([0002](0002-source-graphs-and-mass-ingest.md) §8.5, [0004](0004-identity-clusters-and-equivalence.md) §3);
- find everything that refers to an entity, when its cluster changes or for "Links here" ([0004](0004-identity-clusters-and-equivalence.md) Consequences, [0008](0008-namespaces-and-document-pages.md) §10);
- check a base offset on write ([0006](0006-log-integrity-and-erasure.md) §8);
- allocate global revision and log IDs ([0012](0012-api-requirements.md) §2.1).

[0001](0001-revision-metadata-rdf.md) §2 originally gave the metadata graph to "the Triplespace application only", for history views, attribution, diffs and moderation. [0010](0010-site-ui.md) §13 and [0012](0012-api-requirements.md) §3 then designed those views on an **activity projection**, a time-ordered index, without SPARQL. The ADRs have already moved to a position where RDF is the model and the export, and something else serves the application. This ADR names that something.

Every store the ADRs already require is a relational table in disguise:

| Store | Required by |
|---|---|
| Version cursor: upstream version and content hash per mirrored entity | [0002](0002-source-graphs-and-mass-ingest.md) §8.4, [0006](0006-log-integrity-and-erasure.md) §3, §8 |
| Uniqueness index on match-key properties | [0002](0002-source-graphs-and-mass-ingest.md) §8.5 |
| Reverse index from each cluster member to the statements that refer to it | [0004](0004-identity-clusters-and-equivalence.md) Consequences |
| Clusters, links by tier, held conflicts | [0004](0004-identity-clusters-and-equivalence.md) §2, §10 |
| Correction states: active, redundant, dangling | [0002](0002-source-graphs-and-mass-ingest.md) §7, [0003](0003-statement-ui.md) §4 |
| Actor records, surrogates, and the private binding lookup | [0007](0007-actor-identity.md) §3–5 |
| Title index, page table, links projection | [0008](0008-namespaces-and-document-pages.md) §4, §10 |
| Keyed-type surrogates and the upstream ID ↔ key map | [0009](0009-keyed-entity-types-and-domain.md) §7, §9 |
| Activity projection | [0010](0010-site-ui.md) §13 |
| Jobs, checkpoints and segment manifests | [0011](0011-logs.md) §6.3, [0006](0006-log-integrity-and-erasure.md) §6 |
| Global revision and log ID allocation | [0012](0012-api-requirements.md) §2.1 |

MediaWiki is the precedent. Its `page`, `revision`, `slots` and `content` tables hold metadata and a `content_address` that points at the text in external storage. Wikibase adds a normalized term store (`wbt_item_terms`, `wbt_term_in_lang`, `wbt_text_in_lang`, `wbt_text`), `wb_items_per_site` for sitelinks, `wb_id_counters`, `wb_property_info` and `wb_changes`. The entity JSON itself is opaque to the database; every query the wiki needs to answer has its own table.

PostgreSQL is already part of the deployment stack, and the only question the storage-engine decision in [0000](0000-init.md) has left is whether the log itself goes into it, or only the projections.

## Decision

### 1. Postgres holds the log

*Current text: [03](../architecture/03-storage-caches-and-search.md) §1.1.*

### 2. The `log` schema

*Changed by A2, A5, A27, A31.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §2.1, §2.2, §5.*

### 3. Erasure and compaction in Postgres (amends 0006 §7)

*Changed by A2, A31, A40.*

*Current text: [01](../architecture/01-log-and-records.md) §5.2, §6.*

### 4. Four schemas

*Changed by A3, A7, A8, A9, A11, A27, A45.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §1.2, §4.16.*

### 5. The `view` schema

*Changed by A5, A9, A11, A46.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.1, §7.*

#### 5.1 Entities

*Changed by A4, A28.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.2, §5.*

#### 5.2 Terms, sitelinks, identifiers and references

*Changed by A9.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.3, §5.*

#### 5.3 Statements, clusters and corrections

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.4, §5.*

#### 5.4 Keyed types, pages, actors

*Changed by A9, A11, A15, A21, A27, A43, A48.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.5, §5.*

#### 5.5 Activity, jobs and configuration

*Changed by A9, A18, A47, A49.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.6, §5.*

#### 5.6 Tables added by later ADRs

*Changed by A2, A3, A6, A7, A8, A9, A10, A11, A13, A14, A16, A17, A19, A21, A22, A23, A24, A25, A26, A30, A33, A34, A35, A36, A37, A38, A39, A41, A42, A43, A50.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §5.*

### 6. Global revision, log and page IDs (amends 0012 §2.1)

*Changed by A2, A5, A6, A12, A22, A29.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §2.3.*

### 7. Projections, synchrony and read-your-writes

*Changed by A3, A5, A6, A8, A9, A10, A11, A13, A14, A16, A19, A27, A28, A33, A44.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §5, §6.1, §6.2, §6.3.*

### 8. RDF becomes an output, not the read path (amends 0001 §2 and 0005 §4.4)

*Changed by A9, A10, A32.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.5, §4.1, §6.5.*

### 9. Bootstrap mode (extends 0002 §8.6; uses 0006 §5)

*Changed by A31.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.8.*

### 10. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

### 11. Deployment profiles

*Current text: [22](../architecture/22-crates-and-stack.md) §6.*

### 12. Changelog of the 2026-09-27 revision

*Superseded by the Amendment log.* Each row of the changelog this section held is an entry there, among A2–A10.

## Consequences

- **One durable system.** The log, its proofs, every projection and the private store share one backup, one replication stream and one recovery point. The storage-engine question open since 0000 is closed.
- **Interactive writes are one transaction.** Append, ID allocation, base-offset check and the synchronous projections commit together. There is no window in which an editor's own edit is invisible to them.
- **Storage at Wikidata scale is estimated, not measured.** A `latest` mirror of Wikidata is on the order of 1 TB of record bodies before compression; `entity_ref` and `term` are each on the order of 10^9 rows. The materialize-only-when-different rule (§5.1) keeps the resolved view from doubling the first figure. The bootstrap audit must measure all three before a full load is attempted.
- **Erasure has an operational tail.** An `UPDATE` erases the live tuple; `VACUUM` and WAL retention decide when the bytes are gone. The runbook, not the schema, guarantees the deadline.
- **Postgres is on the append path.** Local appends are serialized per partition by a row lock, which caps single-partition write throughput at what one transaction stream can do. That is well above what human editing produces, and bulk ingest does not use that path.
- **Scatterbase gains a Postgres backend without asking for one.** `scatter-log-postgres` is a substrate crate; its claim partition can live in Postgres or in files with no change above the trait.
- **The quad store is demoted.** Anything that assumed the metadata graph would be queried by the application has to read `view` instead. Nothing in 0010–0012 did.
- **Privacy is enforceable in two places.** The route-level test of 0012 §8 and the grant-level separation of §4 have to agree; a table that moves between schemas changes what a role can read.
- ~~**Migrations become part of the compatibility surface.** A `view` change needs a rebuild or a migration; a `log` change needs a new payload or header version and a migration that keeps every existing header verifiable.~~ *Only a change to what `log` records are; a change to how they are stored needs a migration alone (A31).*
- **Migrations become part of the compatibility surface.** A `view` change needs a rebuild or a migration. A change to what `log` records are needs a new payload or header version and a migration that keeps every existing header verifiable; a change to how they are stored that keeps every record's logical bytes ([0058](0058-packed-record-storage.md) §1) needs only a migration.

## Open questions

- **Q1. Measured sizes.** Record, `entity_ref` and `term` sizes for a full Wikidata mirror, and the compression ratio of canonical CBOR under `lz4` versus `zstd` (which needs an extension or a newer Postgres).
- **Q2. Where large mirrors live.** Whether mirror child tables should go to a separate database or tablespace, or whether their bodies should move to object storage behind the same `LogStore` trait once measured sizes justify it. Sharding is out of scope until then.
- **Q3.** ~~**Whether `revid` and `logid` enter the header.** Left to the global-ID ADR, which this ADR otherwise answers.~~ *Settled by [0015](0015-record-format-and-partition-registry.md) §2: they do, with `page_id`, as header fields 7–9 inside the Merkle tree.*
- **Q4. Statement rows for the local graph.** Whether `statement_assertion` should also index property and rank for every local statement, so that per-property views of large local entities can be paged in SQL instead of from the blob.
- **Q5. Vacuum policy.** How soon after an `erase` the affected child table is vacuumed, and whether `VACUUM FULL` is the default for reason class `legal`.
- **Q6.** ~~**Which projections are synchronous.** §7 puts 1–5 in the write transaction. `entity_ref` for an entity with tens of thousands of referrers may be too slow; the threshold at which a projection goes asynchronous for one write is to be set from measurement.~~ *Settled by A11: a write's own rows are always synchronous; fan-out to other entities is inline up to `projections.sync_budget` / `projections.sync_time`, then queued with lag reported. The default values are tuning.*
- **Q7.** ~~**`page_id` for entities on rebuild.** §6 assigns it from the entity's first record in append order. If two partitions first mention an entity in records that interleave differently on rebuild, the ID could differ; the rule may need to be "first record in the local partition, else first mirror record by partition number".~~ *Settled by [0015](0015-record-format-and-partition-registry.md) §2: the ID is in the header and carried forward, never derived.*
- **Q8. Partition count at farm scale** and whether small tenants share physical child tables ([0018](0018-tenants.md) Q1). [0028](0028-tenancy-policy.md) §2 adds three instance partitions per farm, which does not change the shape of the question.
- **Q9. The addressing projection's role.** §7 runs it under the notifier's role because it writes to `private`; whether `ops.projection_state` should record its position like the others, or `private.inbox` should carry its own high-water mark, is an implementation choice to settle with [0021](0021-notifications.md).
- **Q10.** ~~**A `NULL` tenant in a key.** §5 and [0018](0018-tenants.md) §6 give shared rows and instance-level registry entries a `NULL` tenant and say the tenant is in the key, but a Postgres primary key cannot hold `NULL`.~~ *Settled by A11: `tenant text NOT NULL DEFAULT ''`, the empty string being the instance, leading every primary key; a read tries the viewer's tenant and falls back to `''`.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0000](0000-init.md) Q6 | §1 | settles | 0000 Q6 |
| [0001](0001-revision-metadata-rdf.md) §2 | §8 | amends | 0001 A6 |
| [0002](0002-source-graphs-and-mass-ingest.md) §8.6 | §9 | extends | 0002 A5 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q8 | §5.2 | settles | 0002 Q8 |
| [0005](0005-crate-organization.md) §2, §3, §4.2, §4.4 | §1, §8, §10 | amends | 0005 A8 |
| [0006](0006-log-integrity-and-erasure.md) §5 | §1, §3 | extends | 0006 A2 |
| [0006](0006-log-integrity-and-erasure.md) §7 | §1, §3 | amends | 0006 A2 |
| [0008](0008-namespaces-and-document-pages.md) Q1 | §6 | settles | 0008 Q1 |
| [0008](0008-namespaces-and-document-pages.md) Q2 | §6 | settles | 0008 Q2 |
| [0010](0010-site-ui.md) §13 | §5.5, §10 | amends | 0010 A2 |
| [0010](0010-site-ui.md) §5.2, §12 | §6 | amends | 0010 A3 |
| [0010](0010-site-ui.md) Q3 | §6 | settles | 0010 Q3 |
| [0011](0011-logs.md) §2, §8 | §6, §8 | amends | 0011 A2 |
| [0011](0011-logs.md) Q1 | §6 | settles | 0011 Q1 |
| [0012](0012-api-requirements.md) §2.1 | §6 | amends | 0012 A2 |
| [0012](0012-api-requirements.md) Q1 | §6 | settles | 0012 Q1 |

## References

- [Manual:Database layout](https://www.mediawiki.org/wiki/Manual:Database_layout), [Manual:Content table](https://www.mediawiki.org/wiki/Manual:Content_table) and [Manual:External storage](https://www.mediawiki.org/wiki/Manual:External_storage)
- [Wikibase term store (`wbt_*` tables)](https://doc.wikimedia.org/Wikibase/master/php/docs_storage_terms.html)
- [Manual:ChronologyProtector](https://www.mediawiki.org/wiki/Manual:ChronologyProtector)
- [PostgreSQL: table partitioning](https://www.postgresql.org/docs/current/ddl-partitioning.html), [TOAST and compression](https://www.postgresql.org/docs/current/storage-toast.html), [`COPY`](https://www.postgresql.org/docs/current/sql-copy.html), [`pg_last_wal_replay_lsn()`](https://www.postgresql.org/docs/current/functions-admin.html)
- [Kafka log compaction](https://kafka.apache.org/documentation/#compaction)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-26
- **Source:** [0005](0005-crate-organization.md) §2, revision of 2026-09-26
- **Change:** supersedes §10
- **Summary:** 0005 §2 became the one crate table CI checks, and carries every crate of this section (0005 A8). The section was kept in step with it until this conversion: A2 renamed `erase_bodies`. It still named `sqlx`, which 0033 §4 replaced with `tokio-postgres` in 0005 §2's dependency notes.

Replaced text (§10):

> | Layer | Crate | Change |
> |---|---|---|
> | Substrate | `scatter-log` | Gains the `LogStore` trait: `append`, `read`, `scan`, `erase_parts` (named `erase_bodies` until [0015](0015-record-format-and-partition-registry.md) §7), `compact`, `head`. Its file backend, `segments`, implements it and remains the export-bundle format. |
> | | `scatter-log-postgres` (new) | `LogStore` over the `log` schema of §2, with `sqlx`. It also owns that schema's migrations. Shared with Scatterbase, which may use it in place of files. |
> | | `scatter-projection` | The projection trait gains a transactional variant, so a projection can apply inside the appending transaction (§7). |
> | Triplespace | `triplespace-db` (new) | The `view`, `private` and `ops` schemas, their migrations, roles and grants (§4), connection pools, and replica routing (§7) |
> | | `triplespace-projections` (new) | Every `view` projection of §5, in the order of §7. Absorbs `triplespace-activity` from [0010](0010-site-ui.md) §13. Pure computation stays in `scatter-wikibase-resolve`, `scatter-identity` and the other core crates; this crate only reads records and writes rows. |
> | | `triplespace-rdf` (new) | The streaming RDF projection of §8, using `scatter-wikibase-rdf`. Absorbs `triplespace-revmeta`, the metadata-graph projection of [0001](0001-revision-metadata-rdf.md) and [0011](0011-logs.md) §10. |
>
> The rules of [0005](0005-crate-organization.md) §3 hold: no core crate depends on `sqlx` or Postgres, async and I/O appear only in `scatter-log-postgres`, `scatter-ingest` and the surfaces, and the `segments` backend keeps `scatter-log` usable without a database.

### A2. Header IDs, erasable parts and upstream revisions

- **Date:** 2026-09-26
- **Source:** [0015](0015-record-format-and-partition-registry.md) §1–2, §4, §7
- **Change:** amends §2, §3, §6; extends §5.6
- **Summary:** `log.record`: `revid`, `logid` and `page_id` as header columns; `body` never `NULL`; `erased` bitmask beside `erased_by`; the erasure statement in §3 rewrites parts; `erase_bodies` → `erase_parts` in §10; §6 rewritten for header IDs, provider-ranged revision IDs and carried-forward page IDs; two open questions closed. §4 adds `view.upstream_revision`. This settled Q3 and Q7.

Replaced text: not recorded. 0013 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A3. Permission tables

- **Date:** 2026-09-26
- **Source:** [0016](0016-permissions-and-access-control.md) §3, §9
- **Change:** extends §4, §5.6, §7
- **Summary:** Permission tables in §5.6 and step 1 of §7; IP blocks in `private`.

### A4. Entity IDs

- **Date:** 2026-09-27
- **Source:** [0017](0017-entity-id-grammar.md) §4
- **Change:** amends §5.1
- **Summary:** `view.entity.id` comment.

Replaced text: not recorded. 0013 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A5. Tenants

- **Date:** 2026-09-27
- **Source:** [0018](0018-tenants.md) §2–3, §6
- **Change:** amends §2, §5, §6, §7
- **Summary:** `partition` as `bigint` everywhere; `tenant` on `log.partition` with the `(tenant, name)` key; the tenant column rule at the head of §5; `view.registry` keyed by tenant; per-tenant sequences in §6; overlay recomputation in §7; a farm-scale open question.

Replaced text: not recorded. 0013 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A6. Threads

- **Date:** 2026-09-27
- **Source:** [0019](0019-discussions.md) §2, §11
- **Change:** extends §5.6, §6, §7
- **Summary:** Thread tables in §5.6 and step 5 of §7; talk-page IDs from the page sequence in §6.

### A7. The watch set

- **Date:** 2026-09-27
- **Source:** [0020](0020-change-feeds.md) §3
- **Change:** extends §4, §5.6
- **Summary:** `private.watch` and `watch_token` in §4 and §5.6; the no-cross-schema-join rule in §4.

### A8. Notifications

- **Date:** 2026-09-27
- **Source:** [0021](0021-notifications.md) §3, §5, §8, §10
- **Change:** extends §4, §5.6, §7
- **Summary:** Inbox and contact tables in §4 and §5.6; the delivery queue in `ops`; `triplespace-notify` as the second reader of `private`; the addressing projection as step 6 of §7.

### A9. Third pass: 0022 through 0031

- **Date:** 2026-09-27
- **Source:** [0022](0022-federation.md) §1, §8, §10, §13; [0023](0023-moderation.md) §10; [0024](0024-subsidiary-accounts.md) §10; [0025](0025-oauth-server.md) §8; [0026](0026-sitelinks.md) §6; [0027](0027-preferences-and-portability.md) §8; [0028](0028-tenancy-policy.md) §12; [0029](0029-resolver-namespaces.md) §3; [0030](0030-edit-filters.md) §11; [0031](0031-property-constraints.md) §5
- **Change:** amends §5, §5.2, §5.6; extends §4, §5.4, §5.5, §7, §8
- **Summary:** `partition` made `bigint` in `view.record_statement` and `view.activity` (and in `view.upstream_revision`, 0015 §4), which the head of §5 already required; `view.entity.deleted`, `view.page.reserved`, `view.actor.operator`, `groups`, the `kind` and `status` vocabularies, and `view.activity.patrolled` with its partial index written into the SQL they were listed against in §5.6; the `private` and `ops` cells of §4 list keys, tokens, consumer secrets, preferences, the hit IP row, per-actor ActivityPub keys, and the export, filter-test and re-check jobs; `triplespace-federation` named as the third reader in the table; the write path of §7 stated in order, with `filter` in step 1, `keyed_map` in step 2, `value_key` and `constraint_count` in step 4 and `filter_hit` in step 5; §5.6 gains the `ops` job rows, the rewritten `sitelink`, `view-pin` and `resolver`; the `NULL`-tenant key question added below. The sitelink table was rewritten (0026), `notification_pref` was dropped for preferences (0027), and `ap_inbox_seen` joined `ops` (0022 §8). 0022 §1's local-graph source dump, which extends §8, was recorded only in 0022 until this conversion.

Replaced text: not recorded. 0013 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A10. The SPARQL Update stream

- **Date:** 2026-09-27
- **Source:** [0032](0032-sparql-update-stream.md) §1–3, §7, §9
- **Change:** amends §8; extends §5.6, §7
- **Summary:** By section:
  - §5.6, §7, §8: `view.rdf_delta` and the delta epoch in §5.6; the delta projection noted in §7; §8 amended: stamped and skolemized dumps, and the local quad store fed from the delta table.
  - §8: Every dump is stamped with the cursor of the SPARQL Update stream it continues into, and is offered in a second form with blank nodes skolemized (0032 §7); the plain form is unchanged. The local quad store is no longer a separate projection: it is fed from `view.rdf_delta`, the same rows the stream serializes, in process through `scatter-quadstore::apply` or externally through `triplespace-cli sparql-sync` (0032 §9).

Replaced text (§8):

> - **A local quad store is a deployment option.** An instance that wants its own SPARQL endpoint runs the quad projection into Oxigraph or QLever as one more consumer of the log.

### A11. Decisions of 2026-09-27 (evening)

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** amends §5; extends §4, §5.4, §5.6, §7
- **Summary:** By section:
  - §4, §5.6: Password hashes for the built-in `password` issuer live in `private.password` (decision 1, recorded as 0007 A3). Names a local account releases are kept as keyed hashes in `private.name_tombstone`, so that no name is ever reused (decision 7, recorded as 0010 A6).
  - §5: `tenant text NOT NULL DEFAULT ''` leads every primary key, the empty string being the instance; a read tries the viewer's tenant, then `''` (decision 3). This settled Q10.
  - §7: A write's own rows are always synchronous: the written entity's or page's resolution, terms, identifiers, sitelinks, its own `entity_ref` rows, its own constraint checks ([0031](0031-property-constraints.md) §2), its activity row and its delta ([0032](0032-sparql-update-stream.md) §2). **Fan-out** to other entities, meaning referrers re-resolved by a cluster change ([0004](0004-identity-clusters-and-equivalence.md), Consequences), `type`, `inverse` and `symmetric` constraint re-checks of statements that point at the changed entity (0031 §2), and the deltas those produce, is applied inline until a budget is spent and then handed to the projection worker: `projections.sync_budget` (`site` configuration, default 1,000 rows) or `projections.sync_time` (default 250 ms), whichever comes first. The remainder is queued in `ops.projection_state` as work for the affected keys, applied in append order by the worker that serves bulk jobs, and reported as lag on the entity page's identity line ("N referrers updating") and in `siprop=triplespace`. Read-your-writes therefore holds for the thing edited and for small fan-outs; a link to a heavily cited author shows its effect on referrers within lag. The append lock of §2 is released at commit, before the queued remainder runs, so a large fan-out never blocks the next editor. (decision 6). This settled Q6.

Replaced text: not recorded. 0013 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A12. Entity-ID sequences

- **Date:** 2026-09-28
- **Source:** [0035](0035-adopting-a-wikibase.md) §4
- **Change:** extends §6
- **Summary:** Local entity IDs come from one more set of per-tenant sequences, one per minted entity type (`log.item_id`, `log.property_id`, …), taken in the appending transaction as Wikibase's `wb_id_counters` are; the ID blocks of [0002](0002-source-graphs-and-mass-ingest.md) §8.5 reserve ranges from them. An adoption sets every sequence here — entity, page, revision, log and user IDs — past what the source wiki consumed, records the floors in its job record, and supplies `page_id` for adopted records rather than taking one.

### A13. Page statements and categories

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §10
- **Change:** extends §5.6, §7
- **Summary:** `page_statements`, `page_category`, `category`; page subjects in existing tables. §5.6 had been given its row in place; the projection steps it names were added to §7 at conversion.

### A14. Files

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §20
- **Change:** extends §5.6, §7
- **Summary:** File tables, `ops.upload_stash` and `ops.blob_delete`. §5.6 had been given its row in place; the file projection's step was added to §7 at conversion.

### A15. Content model IDs

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §10
- **Change:** amends §5.4
- **Summary:** `view.page.content_model` holds content model registry IDs. The SQL had been given the comment in place.

Replaced text: `content_model text NOT NULL, owner_actor text,` with no comment.

### A16. Tables written by rendering

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §10
- **Change:** amends §5.6; extends §7
- **Summary:** On a tenant with expansion on, `transclusion`, `render_state`, `entity_usage`, and the links, categories and file usage they drive are written by the refresh job and rebuilt by re-rendering, not by replay. Where a render read a foreign template repository or the clock, a rebuild can differ, so the first rule below does not hold for them. §5.6 had been given the table row in place.

Replaced text (§5.6):

> Every `view` table is a projection under §7 and is rebuilt from the log.

### A17. Entity usage

- **Date:** 2026-09-30
- **Source:** [0043](0043-lua-modules.md) §10
- **Change:** extends §5.6
- **Summary:** `view.entity_usage`. §5.6 had been given its row in place.

### A18. The primary tenant

- **Date:** 2026-09-30
- **Source:** [0046](0046-primary-tenant.md) §2
- **Change:** extends §5.5
- **Summary:** `primary` is an instance-level kind, with the single code `primary`.

### A19. Reports and site statistics

- **Date:** 2026-10-01
- **Source:** [0047](0047-special-pages.md) §4.3, §13
- **Change:** amends §5.6; extends §7
- **Summary:** As noted under each section before the migration:
  - §5.6: A report configured as `batch` keeps a dated snapshot in `view.report_entry`. Like 0042 §10's tables, it is not a pure function of the log: a rebuild empties it, and the next scheduled run fills it. Projection-backed report rows are rebuilt with the tables they read. §5.6 had been given the table row in place.
  - §7: The `report` and `site_stats` projections run in step 5, after `activity` and `page_link`. Report entries are fan-out under the synchronous budget; the refresh job of 0042 §10 applies them for the tables it writes.

Replaced text: the rule as A16 states it, which this extended to batch reports.

### A20. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §4–12
- **Summary:** A1–A19 were folded into the Decision, and §12, the changelog of the 2026-09-27 revision, became the log: A2–A8 are its rows, A9 its third-pass row, which named ten ADRs, and A10 its 0032 row. The `Revised` header line is retired. The open questions were numbered. No decision changed. Before this, A10–A12, A16, A18 and A19 were blockquotes, A13–A17 and A19 had also been given rows or comments in place, and A9's local-graph dump was recorded only in 0022. The file before conversion is commit `0b26a3a`.

### A21. Redirects

- **Date:** 2026-10-01
- **Source:** [0051](0051-page-redirects.md) §5
- **Change:** extends §5.4, §5.6
- **Summary:** `view.redirect` and `is_redirect` on `view.page`, written in step 2.

### A22. The title index and ranged page IDs

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §6, §8
- **Change:** extends §5.6, §6
- **Summary:** `view.foreign_title` at instance scope; a repository page's `pageid` is provider-ranged, derived from its upstream page ID.

### A23. Bundles, cursors and the fetch queue

- **Date:** 2026-10-01
- **Source:** [0053](0053-mirrored-pages.md) §8
- **Change:** extends §5.6
- **Summary:** `view.foreign_page`, `ops.repo_cursor` and `ops.page_fetch`; in `proxy` mode `foreign_page` indexes the L2 cache and is a projection of no log, which the two-rules paragraph now notes.

### A24. Forks

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §3, §10
- **Change:** extends §5.6
- **Summary:** `view.fork`, page subjects in `view.upstream_revision`, and `ops.fork`.

### A25. Page properties

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §6
- **Change:** extends §5.6
- **Summary:** `view.page_prop`, MediaWiki's `page_props`; the properties a render sets are rebuilt by re-rendering, like the render tables.

### A26. Visibility tables

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §14
- **Change:** extends §5.6
- **Summary:** `view.set_member`, `read_groups` on `view.activity`, `visibility_epoch` on `view.tenant`, and the `tenant` and `set` target kinds with a confidential-or-moderation `kind` on `view.acl`. Nothing is added to `private`.

### A27. Clarifications from the first implementation

- **Date:** 2026-10-02
- **Source:** Direct: James, review of 2026-10-02 (`scatter-log-postgres`, `triplespace-db`, `triplespace-projections`)
- **Change:** amends §2, §4, §5.4 and §7
- **Summary:** Five things the ADR left to the implementation, written down as built: `log.checkpoint.key_id` is the signed-note key hash; the database roles are named (`ts_server`, `ts_accounts`, `ts_notify`, `ts_federation`, `ts_verify`); the `group`, `membership`, `block` and `acl` tables that [0016](0016-permissions-and-access-control.md) §9 names get their SQL, with a `layer` column for the instance floor of [0040](0040-instance-prerogatives.md) §5; an erased actor record projects a vanished row, created on replay if missing; queued fan-out lives in `ops.projection_work`, not in `ops.projection_state`, which holds positions only; and the replica route checks `pg_is_in_recovery()` before trusting a replay position.

Replaced text (§7): "The remainder is queued in `ops.projection_state` as work for the affected keys," and "a read is routed to a replica only when `pg_last_wal_replay_lsn()` on that replica has passed it, and to the primary otherwise." §2, §4 and §5.4 gain text.

### A28. Resolution as built

- **Date:** 2026-10-02
- **Source:** Direct: James, implementation of 2026-10-02 (`triplespace-projections`, `scatter-wikibase-resolve`)
- **Change:** extends §5.1, §7
- **Summary:** The `entity` projection writes `term` and `identifier` from the same resolution pass, so the three tables share one position; a rebuild orders partitions by dependency and the resolution projection fails loudly on a keyed subject without a surrogate. The `source` kind of §5.1 requires a whole-state record (`put`, or a `create`/`adopt` that is the only local record); an edited local entity is materialized. `resolved` is lz4-compressed by TOAST, not by the application.

### A29. Instance sequences and mirrored page IDs

- **Date:** 2026-10-03
- **Source:** Direct: James, decision of 2026-10-03 (`scatter-ingest`, `scatter-log-postgres`)
- **Change:** extends §6
- **Summary:** The instance's sequences (`job_id`, `log_id`, `page_id`, one surrogate counter per keyed type) live under the reserved tenant name `instance`; a mirrored entity's first record takes its page ID from the instance's `page_id` sequence, provider-ranged page IDs being for page repositories only; the per-tenant set gains `user_id`.

### A30. `private.session`

- **Date:** 2026-10-03
- **Source:** Direct: James, decision of 2026-10-03 (`triplespace-accounts`, `triplespace-api-action`, `triplespace-server`, `scatter-adapter-internetdomains`)
- **Change:** extends §5.6
- **Summary:** `private.session (id text PRIMARY KEY, tenant, actor_key, key_id, created, expires, last_seen)`, `portability: never-leaves`: the session store of an instance without a shared cache (0014 A14). `id` is the cookie value, 32 random bytes; `actor_key` is NULL for an anonymous session that holds only tokens; `key_id` is the API key a bot-password login used, so revoking the key finds the sessions (0024 §4). Migration `0006_private_session`.

### A31. Packed record storage

- **Date:** 2026-10-04
- **Source:** [0058](0058-packed-record-storage.md) §1, §2, §6, §7
- **Change:** amends §2, Consequences; extends §3, §9
- **Summary:** A partition's `storage` is `plain` or `packed`. A packed body is stored as zstd, against a per-domain dictionary, over a packed form in which repeated subtrees named by `docs/registry/fragments.toml` are 16-byte keyed references into `log.fragment`, stored once per dedup domain (each tenant, and the instance's shared partitions). `log.record` gains `codec` and `dict`; `log.fragment_domain`, `log.fragment`, `log.dict` and `ops.fragment_candidate` are added. The logical bytes, header, leaf and commitment do not change, and every rebuilt body is checked against the commitment. Erasure and compaction make the removed refs candidates for a candidate sweep, which deletes unshared fragments; shared ones wait for a full sweep, which a `legal` erasure runs at once. Bootstrap writers pack in-process and merge fragments after the load. A storage-only change to the `log` schema no longer needs a format version.

Replaced text (§2):

> - **Bodies use TOAST with `lz4` compression.** Canonical CBOR compresses well, and `lz4` decompresses fast enough for the read path. Bodies are never queried inside the database; they are decoded by the server.

Replaced text (Consequences):

> - **Migrations become part of the compatibility surface.** A `view` change needs a rebuild or a migration; a `log` change needs a new payload or header version and a migration that keeps every existing header verifiable.

### A32. The query service reads the quad store

- **Date:** 2026-10-04
- **Source:** [0059](0059-query-service.md) §1
- **Change:** amends §8
- **Summary:** The local quad store gains exactly one reader, the query service: compiled queries that materialize scope membership and raw SPARQL for people, isolated per tenant by the protocol's dataset. "Nothing in the API reads from it" becomes "nothing reads from it but the query service". The store stays a consumer of `view.rdf_delta`, holds public form only, and is never the authority.

Replaced text (§8): none removed. The rule quoted in the Summary was stated in [0005](0005-crate-organization.md) §4.4 (A10 here brought the store in as a consumer) and is amended there by 0005 A62.

### A33. Schema reports

- **Date:** 2026-10-05
- **Source:** [0064](0064-entityschema-and-validation.md) §5
- **Change:** extends §5.6, §7
- **Summary:** `view.schema_report`; the scope, task and validation projections named at the end of step 7.

### A34. Caption terms

- **Date:** 2026-10-05
- **Source:** [0065](0065-mediainfo-captions-and-commons.md) §4
- **Change:** extends §5.6
- **Summary:** `view.term` rows keyed by `M` and `WDM` IDs.

### A35. Lexeme term kinds

- **Date:** 2026-10-05
- **Source:** [0066](0066-lexemes.md) §5
- **Change:** extends §5.6
- **Summary:** Term kinds 4–6 for lemmas, representations and glosses.

### A36. Shallow mirroring

- **Date:** 2026-10-07
- **Source:** [0070](0070-shallow-entity-mirroring.md) §6, §10
- **Change:** extends §5.6
- **Summary:** Mirrored entities write `term` rows only in configured languages; the fetch queue; cursors for entity streams.

### A37. Derived statements

- **Date:** 2026-10-07
- **Source:** [0071](0071-derived-statements-from-mirrored-pages.md) §12
- **Change:** extends §5.6
- **Summary:** `derivation`, `derivation_subject` and the extraction queue.

### A38. Publications

- **Date:** 2026-10-07
- **Source:** [0074](0074-publishing-a-scope-to-an-external-wiki.md) §8
- **Change:** extends §5.6
- **Summary:** `ops.publication_page` and `private.publication_credential`.

### A39. Scope dumps

- **Date:** 2026-10-07
- **Source:** [0076](0076-dataset-publication.md) §6
- **Change:** extends §5.6
- **Summary:** `ops.dump`.

### A40. Sealed manifests never change

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §3
- **Summary:** The sentence saying that the compaction worker re-signs the manifest of each segment it touched is struck: a compacted offset keeps its leaf ([0006](0006-log-integrity-and-erasure.md) A14), so a sealed manifest never changes and nothing replaces it. (PENDING A1)

Replaced text (§3):

> The compaction worker re-signs the manifest of each segment it touched, listing the manifest it replaces.

### A41. `provider-readers` is tenant scope

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §5.6
- **Summary:** `provider-readers` is **tenant** scope, a record in the provider tenant's own `config` ([0015](0015-record-format-and-partition-registry.md) §3, [0028](0028-tenancy-policy.md) §5); the 0028 row of §5.6 says so instead of listing it beside the instance-level `tenancy` and `template` kinds. (Corrected 2026-10-08: first written as "add to §5.5's instance-level list", which 0015 §3 and 0028 §5 contradict.) (PENDING A5)

Replaced text (§5.6):

> | `log`, `view` | Instance partitions `actors/{farm}`, `accounts/{farm}`, `log/{farm}`; farm accounts in `actor` with a `NULL` tenant; the farm-wide name-registry index; `tenancy`, `template`, `provider-readers` in `registry` | Farm identity, global groups and blocks, templates | [0028](0028-tenancy-policy.md) §12 |

### A42. `foreign_page` in `proxy` mode indexes L1

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §5.6
- **Summary:** In `proxy` mode `view.foreign_page` indexes the **L1** (Valkey) cache, where [0014](0014-caches-and-search.md) §4 keys the bundle `fp:`, not L2; the 0053 row of §5.6 is corrected with [0053](0053-mirrored-pages.md) §8. (PENDING A7)

Replaced text (§5.6):

> | `view`, `ops` | `foreign_page` (instance scope); `ops.repo_cursor`, `ops.page_fetch` | The bundles the instance holds for repository pages, in `proxy` mode the L2 cache's index and in `mirror` mode the projection of `pages/{repo}`; the event cursor and fetch queue | [0053](0053-mirrored-pages.md) §8 |

### A43. Rows the schema map lacks

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5.4, §5.6
- **Summary:** §5.6 gains rows for `view.thread_attachment`, `view.foreign_thread`, `ops.upstream_post`, `view.scope`, `view.scope_member`, `view.sprint`, `view.task`, `file.mediainfo_id`, the [0058](0058-packed-record-storage.md) `log` and `ops` tables and columns, `ops.migration` and `view.page_text`; the catalogue in chapter 03 §5 is the draft. The `view.page` SQL of §5.4 gains `len`, `latest_at`, `revisions` and `random`, and `view.actor` gains `last_active`, the columns [0047](0047-special-pages.md) §13 adds. Found while writing chapter 03. (PENDING A9)

### A44. Step 2 of the projection order

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §7
- **Summary:** Step 2 of §7's projection order also names `redirect` ([0051](0051-page-redirects.md) §5), `page_prop` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6) and `derivation` ([0071](0071-derived-statements-from-mirrored-pages.md) §12), which §5.6 already says run in step 2. (PENDING A10)

Replaced text (§7):

> 2. `entity_source`, `keyed_map` ([0009](0009-keyed-entity-types-and-domain.md) §9), `page`, `job`, `upstream_revision` ([0015](0015-record-format-and-partition-registry.md) §4), `page_category` and `category` ([0038](0038-page-metadata-and-categories.md) §10), and the file projection ([0039](0039-files-and-media.md) §20);

### A45. Session state

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §4
- **Summary:** Session state lives in Valkey when a shared cache is configured and in `private` otherwise ([0025](0025-oauth-server.md) §8, [0014](0014-caches-and-search.md) §2); "sessions' secrets" in §4's `private` row means that fallback, not an unconditional home. (PENDING A12)

Replaced text (§4):

> | `private` | State that belongs to one account and has no public history: the accounts graph's binding index, password hashes for the built-in `password` issuer ([0007](0007-actor-identity.md) A3), sessions' secrets, IP addresses held for abuse handling and IP blocks, when each binding was last used ([0007](0007-actor-identity.md) §3, [0012](0012-api-requirements.md) §5, [0016](0016-permissions-and-access-control.md) §3); the watch set and its Atom token ([0020](0020-change-feeds.md) §3); inboxes and read state ([0021](0021-notifications.md) §3); email addresses, fediverse handles, and the ActivityPub keys and followers of the notifier and of every opted-in actor ([0021](0021-notifications.md) §5, §8, [0022](0022-federation.md) §6); subsidiaries' API keys, OAuth tokens and confidential consumers' secrets ([0024](0024-subsidiary-accounts.md) §4, [0025](0025-oauth-server.md) §8); every preference, including the notification matrix and the auto-watch flags ([0027](0027-preferences-and-portability.md) §1); the IP row behind each edit-filter hit ([0030](0030-edit-filters.md) §11). §5.6 lists the tables | Permanent; never exported. The portable part travels with an account as a user data bundle, and with a cooperative tenant move as a sealed private extract applied on reclaim ([0027](0027-preferences-and-portability.md) §2–4); nothing else leaves ([0018](0018-tenants.md) §10) | `triplespace-accounts`, `triplespace-notify` and, for `ap_key` and `ap_follower` only, `triplespace-federation` |

### A46. Instance-scope `view` tables

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5
- **Summary:** A third class of `view` table, **instance scope**, with no `tenant` column: `foreign_title`, `foreign_page` and `foreign_thread`; and `rdf_delta`'s shared rows carry `tenant = ''`. The other two classes are the overlay tables of §5's tenant rule and the tables that are per tenant outright. (PENDING A13)

### A47. `view.tenant`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5.5
- **Summary:** `CREATE TABLE view.tenant (tenant text PRIMARY KEY, visibility_epoch integer NOT NULL DEFAULT 0)`, a projection of the tenant's `read` ACL records ([0056](0056-security-model.md) §7, §14); a tenant's configuration stays in `view.registry`. (PENDING B1)

### A48. Provider actor records

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5.4
- **Summary:** One actor record per provider, kind `provider`, in `actors/{provider}`, agent type `prov:Organization`, written when the provider is registered, so that every attestation, OpenAlex's mirror records included, names an actor key; it projects to a `view.actor` row of kind `provider`. The row's verb is amends, but §5.4 already lists `provider` among `actor.kind`'s values and contradicts nothing, so this entry extends; the text it replaces is [0007](0007-actor-identity.md) §6's "no actor records are kept". (PENDING C1)

### A49. The instance kinds are listed once

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §5.5
- **Summary:** [0015](0015-record-format-and-partition-registry.md) §3's scope paragraph is the one list of instance-level config kinds, gaining `reports`, `forwarder` and `page-repo` (tenant or instance); §5.5 refers to it instead of carrying its own list. (PENDING C18)

Replaced text (§5.5):

> `view.registry`'s key is `(tenant, kind, code)` since [0018](0018-tenants.md) §3 split the config kinds by scope; instance-level kinds (`key`, `graph`, `provider`, `issuer`, `keyed-type`, `tenant`, `alias`, and since then `tenancy`, `template`, `consumer`, the instance `sitelink-policy` and `federation-policy` lists, global `group`s, and `primary`, whose single code is `primary` ([0046](0046-primary-tenant.md) §2)) have the empty-string tenant.

### A50. `view.proposal`

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §5.6
- **Summary:** `view.proposal` ([0067](0067-proposals.md) §5) is a row of the schema map of §5.6 and of chapter 03 §5. (PENDING F28)

### A51. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§11
- **Summary:** The Decision's current text now lives in the architecture chapters [01](../architecture/01-log-and-records.md), [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [05](../architecture/05-providers-and-ingest.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
