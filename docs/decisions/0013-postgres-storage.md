# 0013. Postgres as the log store and serving model

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-11-16 (A106)
- **Author:** James Hare / Claude Fable
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0007](0007-actor-identity.md), [0009](0009-keyed-entity-types-and-domain.md), [0014](0014-caches-and-search.md)

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

*Current text: [03](../architecture/03-storage-caches-and-search.md) §2.1, §2.2, §5.*

### 3. Erasure and compaction in Postgres (amends 0006 §7)

*Current text: [01](../architecture/01-log-and-records.md) §5.2, §6.*

### 4. Four schemas

*Current text: [03](../architecture/03-storage-caches-and-search.md) §1.2, §4.16; [08](../architecture/08-tenants-and-instances.md) §6.7.*

### 5. The `view` schema

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.1, §7.*

#### 5.1 Entities

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.2, §5.*

#### 5.2 Terms, sitelinks, identifiers and references

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.3, §5.*

#### 5.3 Statements, clusters and corrections

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.4, §5.*

#### 5.4 Keyed types, pages, actors

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.5, §5.*

#### 5.5 Activity, jobs and configuration

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.6, §5.*

#### 5.6 Tables added by later ADRs

*Current text: [03](../architecture/03-storage-caches-and-search.md) §5.*

### 6. Global revision, log and page IDs (amends 0012 §2.1)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §2.3.*

### 7. Projections, synchrony and read-your-writes

*Current text: [03](../architecture/03-storage-caches-and-search.md) §5, §6.1, §6.2, §6.3.*

### 8. RDF becomes an output, not the read path (amends 0001 §2 and 0005 §4.4)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.5, §4.1, §6.5; [17](../architecture/17-federation-and-publication.md) §1.1.*

### 9. Bootstrap mode (extends 0002 §8.6; uses 0006 §5)

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.8.*

### 10. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 11. Deployment profiles

*Current text: [22](../architecture/22-crates-and-stack.md) §6.*

### 12. Changelog of the 2026-09-27 revision

*Superseded by the Amendment log.* Each row of the changelog this section held is an entry there, among A2–A10.

## Consequences

- **One durable system.** The log, its proofs, every projection and the private store share one backup, one replication stream and one recovery point. The storage-engine question open since 0000 is closed.
- ~~**Interactive writes are one transaction.** Append, ID allocation, base-offset check and the synchronous projections commit together. There is no window in which an editor's own edit is invisible to them.~~ *The appending transaction records and nothing composes inside it; the written entity is composed after commit, before the response, so the editor's next read still hits (A53).*
- **Storage at Wikidata scale is estimated, not measured.** A `latest` mirror of Wikidata is on the order of 1 TB of record bodies before compression; `entity_ref` and `term` are each on the order of 10^9 rows. The materialize-only-when-different rule (§5.1) keeps the resolved view from doubling the first figure. The bootstrap audit must measure all three before a full load is attempted. `term` is bounded by the instance's term languages and `entity_ref` may be omitted on the Wikidata profile (A52).
- **Erasure has an operational tail.** An `UPDATE` erases the live tuple; `VACUUM` and WAL retention decide when the bytes are gone. The runbook, not the schema, guarantees the deadline.
- ~~**Postgres is on the append path.** Local appends are serialized per partition by a row lock, which caps single-partition write throughput at what one transaction stream can do. That is well above what human editing produces, and bulk ingest does not use that path.~~ *The lock covers the append alone, so the ceiling is the append rate, not the projection rate; bulk append into a live partition takes it once per block, and only bootstrap of an empty partition bypasses it (A53).*
- **Scatterbase gains a Postgres backend without asking for one.** `scatter-log-postgres` is a substrate crate; its claim partition can live in Postgres or in files with no change above the trait.
- **The quad store is demoted.** Anything that assumed the metadata graph would be queried by the application has to read `view` instead. Nothing in 0010–0012 did.
- **Privacy is enforceable in two places.** The route-level test of 0012 §8 and the grant-level separation of §4 have to agree; a table that moves between schemas changes what a role can read.
- ~~**Migrations become part of the compatibility surface.** A `view` change needs a rebuild or a migration; a `log` change needs a new payload or header version and a migration that keeps every existing header verifiable.~~ *Only a change to what `log` records are; a change to how they are stored needs a migration alone (A31).*
- **Migrations become part of the compatibility surface.** A `view` change needs a rebuild or a migration. A change to what `log` records are needs a new payload or header version and a migration that keeps every existing header verifiable; a change to how they are stored that keeps every record's logical bytes ([0058](0058-packed-record-storage.md) §1) needs only a migration.

## Open questions

- **Q1. Measured sizes.** Record, `entity_ref` and `term` sizes for a full Wikidata mirror, and the compression ratio of canonical CBOR under `lz4` versus `zstd` (which needs an extension or a newer Postgres).
- **Q2. Where large mirrors live.** Whether mirror child tables should go to a separate database or tablespace, or whether their bodies should move to object storage behind the same `LogStore` trait once measured sizes justify it. Sharding is out of scope until then.
- **Q3. Whether `revid` and `logid` enter the header.** *Settled by [0015](0015-record-format-and-partition-registry.md) §2.*
- **Q4. Statement rows for the local graph.** Whether `statement_assertion` should also index property and rank for every local statement, so that per-property views of large local entities can be paged in SQL instead of from the blob.
- **Q5. Vacuum policy.** How soon after an `erase` the affected child table is vacuumed, and whether `VACUUM FULL` is the default for reason class `legal`.
- **Q6. Which projections are synchronous.** *Settled by A11.* *Extended by A53.*
- **Q7. `page_id` for entities on rebuild.** *Settled by [0015](0015-record-format-and-partition-registry.md) §2.*
- **Q8. Partition count at farm scale** and whether small tenants share physical child tables ([0018](0018-tenants.md) Q1). [0028](0028-tenancy-policy.md) §2 adds three instance partitions per farm, which does not change the shape of the question.
- **Q9. The addressing projection's role.** §7 runs it under the notifier's role because it writes to `private`; whether `ops.projection_state` should record its position like the others, or `private.inbox` should carry its own high-water mark, is an implementation choice to settle with [0021](0021-notifications.md).
- **Q10. A `NULL` tenant in a key.** *Settled by A11.*
- **Q11. The accounts partition read through `log.record`.** *Settled by A98.*

## References

- [Manual:Database layout](https://www.mediawiki.org/wiki/Manual:Database_layout), [Manual:Content table](https://www.mediawiki.org/wiki/Manual:Content_table) and [Manual:External storage](https://www.mediawiki.org/wiki/Manual:External_storage)
- [Wikibase term store (`wbt_*` tables)](https://doc.wikimedia.org/Wikibase/master/php/docs_storage_terms.html)
- [Manual:ChronologyProtector](https://www.mediawiki.org/wiki/Manual:ChronologyProtector)
- [PostgreSQL: table partitioning](https://www.postgresql.org/docs/current/ddl-partitioning.html), [TOAST and compression](https://www.postgresql.org/docs/current/storage-toast.html), [`COPY`](https://www.postgresql.org/docs/current/sql-copy.html), [`pg_last_wal_replay_lsn()`](https://www.postgresql.org/docs/current/functions-admin.html)
- [Kafka log compaction](https://kafka.apache.org/documentation/#compaction)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-26
- **Source:** 0005 §2, revision of 2026-09-26
- **Change:** supersedes §10
- **Summary:** 0005 §2 became the one crate table CI checks, and carries every crate of this section (0005 A8). The section was kept in step with it until this conversion: A2 renamed `erase_bodies`. It still named `sqlx`, which 0033 §4 replaced with `tokio-postgres` in 0005 §2's dependency notes.

Replaced text: §10 of this ADR.

### A2. Header IDs, erasable parts and upstream revisions

- **Date:** 2026-09-26
- **Source:** 0015 §1–2, §4, §7
- **Change:** amends §2, §3, §6; extends §5.6
- **Summary:** `log.record`: `revid`, `logid` and `page_id` as header columns; `body` never `NULL`; `erased` bitmask beside `erased_by`; the erasure statement in §3 rewrites parts; `erase_bodies` → `erase_parts` in §10; §6 rewritten for header IDs, provider-ranged revision IDs and carried-forward page IDs; two open questions closed. §4 adds `view.upstream_revision`. This settled Q3 and Q7.

Replaced text: not recorded. 0013 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A3. Permission tables

- **Date:** 2026-09-26
- **Source:** 0016 §3, §9
- **Change:** extends §4, §5.6, §7
- **Summary:** Permission tables in §5.6 and step 1 of §7; IP blocks in `private`.

### A4. Entity IDs

- **Date:** 2026-09-27
- **Source:** 0017 §4
- **Change:** amends §5.1
- **Summary:** `view.entity.id` comment.

Replaced text: not recorded. 0013 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A5. Tenants

- **Date:** 2026-09-27
- **Source:** 0018 §2–3, §6
- **Change:** amends §2, §5, §6, §7
- **Summary:** `partition` as `bigint` everywhere; `tenant` on `log.partition` with the `(tenant, name)` key; the tenant column rule at the head of §5; `view.registry` keyed by tenant; per-tenant sequences in §6; overlay recomputation in §7; a farm-scale open question.

Replaced text: not recorded. 0013 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A6. Threads

- **Date:** 2026-09-27
- **Source:** 0019 §2, §11
- **Change:** extends §5.6, §6, §7
- **Summary:** Thread tables in §5.6 and step 5 of §7; talk-page IDs from the page sequence in §6.

### A7. The watch set

- **Date:** 2026-09-27
- **Source:** 0020 §3
- **Change:** extends §4, §5.6
- **Summary:** `private.watch` and `watch_token` in §4 and §5.6; the no-cross-schema-join rule in §4.

### A8. Notifications

- **Date:** 2026-09-27
- **Source:** 0021 §3, §5, §8, §10
- **Change:** extends §4, §5.6, §7
- **Summary:** Inbox and contact tables in §4 and §5.6; the delivery queue in `ops`; `triplespace-notify` as the second reader of `private`; the addressing projection as step 6 of §7.

### A9. Third pass: 0022 through 0031

- **Date:** 2026-09-27
- **Source:** 0022 §1, §8, §10, §13; 0023 §10; 0024 §10; 0025 §8; 0026 §6; 0027 §8; 0028 §12; 0029 §3; 0030 §11; 0031 §5
- **Change:** amends §5, §5.2, §5.6; extends §4, §5.4, §5.5, §7, §8
- **Summary:** `partition` made `bigint` in `view.record_statement` and `view.activity` (and in `view.upstream_revision`, 0015 §4), which the head of §5 already required; `view.entity.deleted`, `view.page.reserved`, `view.actor.operator`, `groups`, the `kind` and `status` vocabularies, and `view.activity.patrolled` with its partial index written into the SQL they were listed against in §5.6; the `private` and `ops` cells of §4 list keys, tokens, consumer secrets, preferences, the hit IP row, per-actor ActivityPub keys, and the export, filter-test and re-check jobs; `triplespace-federation` named as the third reader in the table; the write path of §7 stated in order, with `filter` in step 1, `keyed_map` in step 2, `value_key` and `constraint_count` in step 4 and `filter_hit` in step 5; §5.6 gains the `ops` job rows, the rewritten `sitelink`, `view-pin` and `resolver`; the `NULL`-tenant key question added below. The sitelink table was rewritten (0026), `notification_pref` was dropped for preferences (0027), and `ap_inbox_seen` joined `ops` (0022 §8). 0022 §1's local-graph source dump, which extends §8, was recorded only in 0022 until this conversion.

Replaced text: not recorded. 0013 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A10. The SPARQL Update stream

- **Date:** 2026-09-27
- **Source:** 0032 §1–3, §7, §9
- **Change:** amends §8; extends §5.6, §7
- **Summary:** By section:
  - §5.6, §7, §8: `view.rdf_delta` and the delta epoch in §5.6; the delta projection noted in §7; §8 amended: stamped and skolemized dumps, and the local quad store fed from the delta table.
  - §8: Every dump is stamped with the cursor of the SPARQL Update stream it continues into, and is offered in a second form with blank nodes skolemized (0032 §7); the plain form is unchanged. The local quad store is no longer a separate projection: it is fed from `view.rdf_delta`, the same rows the stream serializes, in process through `scatter-quadstore::apply` or externally through `triplespace-cli sparql-sync` (0032 §9).

Replaced text: §8 of this ADR.

### A11. Decisions of 2026-09-27 (evening)

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** amends §5; extends §4, §5.4, §5.6, §7
- **Summary:** By section:
  - §4, §5.6: Password hashes for the built-in `password` issuer live in `private.password` (decision 1, recorded as 0007 A3). Names a local account releases are kept as keyed hashes in `private.name_tombstone`, so that no name is ever reused (decision 7, recorded as 0010 A6).
  - §5: `tenant text NOT NULL DEFAULT ''` leads every primary key, the empty string being the instance; a read tries the viewer's tenant, then `''` (decision 3). This settled Q10.
  - §7: A write's own rows are always synchronous: the written entity's or page's resolution, terms, identifiers, sitelinks, its own `entity_ref` rows, its own constraint checks (0031 §2), its activity row and its delta (0032 §2). **Fan-out** to other entities, meaning referrers re-resolved by a cluster change (0004, Consequences), `type`, `inverse` and `symmetric` constraint re-checks of statements that point at the changed entity (0031 §2), and the deltas those produce, is applied inline until a budget is spent and then handed to the projection worker: `projections.sync_budget` (`site` configuration, default 1,000 rows) or `projections.sync_time` (default 250 ms), whichever comes first. The remainder is queued in `ops.projection_state` as work for the affected keys, applied in append order by the worker that serves bulk jobs, and reported as lag on the entity page's identity line ("N referrers updating") and in `siprop=triplespace`. Read-your-writes therefore holds for the thing edited and for small fan-outs; a link to a heavily cited author shows its effect on referrers within lag. The append lock of §2 is released at commit, before the queued remainder runs, so a large fan-out never blocks the next editor. (decision 6). This settled Q6.

Replaced text: not recorded. 0013 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A12. Entity-ID sequences

- **Date:** 2026-09-28
- **Source:** 0035 §4
- **Change:** extends §6
- **Summary:** Local entity IDs come from one more set of per-tenant sequences, one per minted entity type (`log.item_id`, `log.property_id`, …), taken in the appending transaction as Wikibase's `wb_id_counters` are; the ID blocks of 0002 §8.5 reserve ranges from them. An adoption sets every sequence here — entity, page, revision, log and user IDs — past what the source wiki consumed, records the floors in its job record, and supplies `page_id` for adopted records rather than taking one.

### A13. Page statements and categories

- **Date:** 2026-09-29
- **Source:** 0038 §10
- **Change:** extends §5.6, §7
- **Summary:** `page_statements`, `page_category`, `category`; page subjects in existing tables. §5.6 had been given its row in place; the projection steps it names were added to §7 at conversion.

### A14. Files

- **Date:** 2026-09-30
- **Source:** 0039 §20
- **Change:** extends §5.6, §7
- **Summary:** File tables, `ops.upload_stash` and `ops.blob_delete`. §5.6 had been given its row in place; the file projection's step was added to §7 at conversion.

### A15. Content model IDs

- **Date:** 2026-09-30
- **Source:** 0041 §10
- **Change:** amends §5.4
- **Summary:** `view.page.content_model` holds content model registry IDs. The SQL had been given the comment in place.

Replaced text: `content_model text NOT NULL, owner_actor text,` with no comment.

### A16. Tables written by rendering

- **Date:** 2026-09-30
- **Source:** 0042 §10
- **Change:** amends §5.6; extends §7
- **Summary:** On a tenant with expansion on, `transclusion`, `render_state`, `entity_usage`, and the links, categories and file usage they drive are written by the refresh job and rebuilt by re-rendering, not by replay. Where a render read a foreign template repository or the clock, a rebuild can differ, so the first rule below does not hold for them. §5.6 had been given the table row in place.

Replaced text: §5.6 of this ADR.

### A17. Entity usage

- **Date:** 2026-09-30
- **Source:** 0043 §10
- **Change:** extends §5.6
- **Summary:** `view.entity_usage`. §5.6 had been given its row in place.

### A18. The primary tenant

- **Date:** 2026-09-30
- **Source:** 0046 §2
- **Change:** extends §5.5
- **Summary:** `primary` is an instance-level kind, with the single code `primary`.

### A19. Reports and site statistics

- **Date:** 2026-10-01
- **Source:** 0047 §4.3, §13
- **Change:** amends §5.6; extends §7
- **Summary:** As noted under each section before the migration:
  - §5.6: A report configured as `batch` keeps a dated snapshot in `view.report_entry`. Like 0042 §10's tables, it is not a pure function of the log: a rebuild empties it, and the next scheduled run fills it. Projection-backed report rows are rebuilt with the tables they read. §5.6 had been given the table row in place.
  - §7: The `report` and `site_stats` projections run in step 5, after `activity` and `page_link`. Report entries are fan-out under the synchronous budget; the refresh job of 0042 §10 applies them for the tables it writes.

Replaced text: the rule as A16 states it, which this extended to batch reports.

### A20. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** 0050 §13
- **Change:** consolidates §4–12
- **Summary:** A1–A19 were folded into the Decision, and §12, the changelog of the 2026-09-27 revision, became the log: A2–A8 are its rows, A9 its third-pass row, which named ten ADRs, and A10 its 0032 row. The `Revised` header line is retired. The open questions were numbered. No decision changed. Before this, A10–A12, A16, A18 and A19 were blockquotes, A13–A17 and A19 had also been given rows or comments in place, and A9's local-graph dump was recorded only in 0022. The file before conversion is commit `0b26a3a`.

### A21. Redirects

- **Date:** 2026-10-01
- **Source:** 0051 §5
- **Change:** extends §5.4, §5.6
- **Summary:** `view.redirect` and `is_redirect` on `view.page`, written in step 2.

### A22. The title index and ranged page IDs

- **Date:** 2026-10-01
- **Source:** 0052 §6, §8
- **Change:** extends §5.6, §6
- **Summary:** `view.foreign_title` at instance scope; a repository page's `pageid` is provider-ranged, derived from its upstream page ID.

### A23. Bundles, cursors and the fetch queue

- **Date:** 2026-10-01
- **Source:** 0053 §8
- **Change:** extends §5.6
- **Summary:** `view.foreign_page`, `ops.repo_cursor` and `ops.page_fetch`; in `proxy` mode `foreign_page` indexes the L2 cache and is a projection of no log, which the two-rules paragraph now notes.

### A24. Forks

- **Date:** 2026-10-01
- **Source:** 0054 §3, §10
- **Change:** extends §5.6
- **Summary:** `view.fork`, page subjects in `view.upstream_revision`, and `ops.fork`.

### A25. Page properties

- **Date:** 2026-10-01
- **Source:** 0055 §6
- **Change:** extends §5.6
- **Summary:** `view.page_prop`, MediaWiki's `page_props`; the properties a render sets are rebuilt by re-rendering, like the render tables.

### A26. Visibility tables

- **Date:** 2026-10-01
- **Source:** 0056 §14
- **Change:** extends §5.6
- **Summary:** `view.set_member`, `read_groups` on `view.activity`, `visibility_epoch` on `view.tenant`, and the `tenant` and `set` target kinds with a confidential-or-moderation `kind` on `view.acl`. Nothing is added to `private`.

### A27. Clarifications from the first implementation

- **Date:** 2026-10-02
- **Source:** Direct: James, review of 2026-10-02 (`scatter-log-postgres`, `triplespace-db`, `triplespace-projections`)
- **Change:** amends §2, §4, §5.4 and §7
- **Summary:** Five things the ADR left to the implementation, written down as built: `log.checkpoint.key_id` is the signed-note key hash; the database roles are named (`ts_server`, `ts_accounts`, `ts_notify`, `ts_federation`, `ts_verify`); the `group`, `membership`, `block` and `acl` tables that 0016 §9 names get their SQL, with a `layer` column for the instance floor of 0040 §5; an erased actor record projects a vanished row, created on replay if missing; queued fan-out lives in `ops.projection_work`, not in `ops.projection_state`, which holds positions only; and the replica route checks `pg_is_in_recovery()` before trusting a replay position.

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
- **Source:** 0058 §1, §2, §6, §7
- **Change:** amends §2, Consequences; extends §3, §9
- **Summary:** A partition's `storage` is `plain` or `packed`. A packed body is stored as zstd, against a per-domain dictionary, over a packed form in which repeated subtrees named by `docs/registry/fragments.toml` are 16-byte keyed references into `log.fragment`, stored once per dedup domain (each tenant, and the instance's shared partitions). `log.record` gains `codec` and `dict`; `log.fragment_domain`, `log.fragment`, `log.dict` and `ops.fragment_candidate` are added. Erasure and compaction make the removed refs candidates for a candidate sweep, which deletes unshared fragments; shared ones wait for a full sweep, which a `legal` erasure runs at once. Bootstrap writers pack in-process and merge fragments after the load. A storage-only change to the `log` schema no longer needs a format version. The text is in git (`tools/adr.py log 0013 A31 --diff`).

Replaced text: §2 of this ADR; Consequences of this ADR.

### A32. The query service reads the quad store

- **Date:** 2026-10-04
- **Source:** 0059 §1
- **Change:** amends §8
- **Summary:** The local quad store gains exactly one reader, the query service: compiled queries that materialize scope membership and raw SPARQL for people, isolated per tenant by the protocol's dataset. "Nothing in the API reads from it" becomes "nothing reads from it but the query service". The store stays a consumer of `view.rdf_delta`, holds public form only, and is never the authority.

Replaced text (§8): none removed. The rule quoted in the Summary was stated in 0005 §4.4 (A10 here brought the store in as a consumer) and is amended there by 0005 A62.

### A33. Schema reports

- **Date:** 2026-10-05
- **Source:** 0064 §5
- **Change:** extends §5.6, §7
- **Summary:** `view.schema_report`; the scope, task and validation projections named at the end of step 7.

### A34. Caption terms

- **Date:** 2026-10-05
- **Source:** 0065 §4
- **Change:** extends §5.6
- **Summary:** `view.term` rows keyed by `M` and `WDM` IDs.

### A35. Lexeme term kinds

- **Date:** 2026-10-05
- **Source:** 0066 §5
- **Change:** extends §5.6
- **Summary:** Term kinds 4–6 for lemmas, representations and glosses.

### A36. Shallow mirroring

- **Date:** 2026-10-07
- **Source:** 0070 §6, §10
- **Change:** extends §5.6
- **Summary:** Mirrored entities write `term` rows only in configured languages; the fetch queue; cursors for entity streams.

### A37. Derived statements

- **Date:** 2026-10-07
- **Source:** 0071 §12
- **Change:** extends §5.6
- **Summary:** `derivation`, `derivation_subject` and the extraction queue.

### A38. Publications

- **Date:** 2026-10-07
- **Source:** 0074 §8
- **Change:** extends §5.6
- **Summary:** `ops.publication_page` and `private.publication_credential`.

### A39. Scope dumps

- **Date:** 2026-10-07
- **Source:** 0076 §6
- **Change:** extends §5.6
- **Summary:** `ops.dump`.

### A40. Sealed manifests never change

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §3
- **Summary:** The sentence saying that the compaction worker re-signs the manifest of each segment it touched is struck: a compacted offset keeps its leaf (0006 A14), so a sealed manifest never changes and nothing replaces it. (PENDING A1)

Replaced text: §3 of this ADR.

### A41. `provider-readers` is tenant scope

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §5.6
- **Summary:** `provider-readers` is **tenant** scope, a record in the provider tenant's own `config` (0015 §3, 0028 §5); the 0028 row of §5.6 says so instead of listing it beside the instance-level `tenancy` and `template` kinds. (Corrected 2026-10-08: first written as "add to §5.5's instance-level list", which 0015 §3 and 0028 §5 contradict.) (PENDING A5)

Replaced text: §5.6 of this ADR.

### A42. `foreign_page` in `proxy` mode indexes L1

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §5.6
- **Summary:** In `proxy` mode `view.foreign_page` indexes the **L1** (Valkey) cache, where 0014 §4 keys the bundle `fp:`, not L2; the 0053 row of §5.6 is corrected with 0053 §8. (PENDING A7)

Replaced text: §5.6 of this ADR.

### A43. Rows the schema map lacks

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5.4, §5.6
- **Summary:** §5.6 gains rows for `view.thread_attachment`, `view.foreign_thread`, `ops.upstream_post`, `view.scope`, `view.scope_member`, `view.sprint`, `view.task`, `file.mediainfo_id`, the 0058 `log` and `ops` tables and columns, `ops.migration` and `view.page_text`; the catalogue in chapter 03 §5 is the draft. The `view.page` SQL of §5.4 gains `len`, `latest_at`, `revisions` and `random`, and `view.actor` gains `last_active`, the columns 0047 §13 adds. Found while writing chapter 03. (PENDING A9)

### A44. Step 2 of the projection order

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §7
- **Summary:** Step 2 of §7's projection order also names `redirect` (0051 §5), `page_prop` (0055 §6) and `derivation` (0071 §12), which §5.6 already says run in step 2. (PENDING A10)

Replaced text: §7 of this ADR.

### A45. Session state

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §4
- **Summary:** Session state lives in Valkey when a shared cache is configured and in `private` otherwise (0025 §8, 0014 §2); "sessions' secrets" in §4's `private` row means that fallback, not an unconditional home. (PENDING A12)

Replaced text: §4 of this ADR.

### A46. Instance-scope `view` tables

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5
- **Summary:** A third class of `view` table, **instance scope**, with no `tenant` column: `foreign_title`, `foreign_page` and `foreign_thread`; and `rdf_delta`'s shared rows carry `tenant = ''`. The other two classes are the overlay tables of §5's tenant rule and the tables that are per tenant outright. (PENDING A13)

### A47. `view.tenant`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5.5
- **Summary:** `CREATE TABLE view.tenant (tenant text PRIMARY KEY, visibility_epoch integer NOT NULL DEFAULT 0)`, a projection of the tenant's `read` ACL records (0056 §7, §14); a tenant's configuration stays in `view.registry`. (PENDING B1)

### A48. Provider actor records

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5.4
- **Summary:** One actor record per provider, kind `provider`, in `actors/{provider}`, agent type `prov:Organization`, written when the provider is registered, so that every attestation, OpenAlex's mirror records included, names an actor key; it projects to a `view.actor` row of kind `provider`. The row's verb is amends, but §5.4 already lists `provider` among `actor.kind`'s values and contradicts nothing, so this entry extends; the text it replaces is 0007 §6's "no actor records are kept". (PENDING C1)

### A49. The instance kinds are listed once

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §5.5
- **Summary:** 0015 §3's scope paragraph is the one list of instance-level config kinds, gaining `reports`, `forwarder` and `page-repo` (tenant or instance); §5.5 refers to it instead of carrying its own list. (PENDING C18)

Replaced text: §5.5 of this ADR.

### A50. `view.proposal`

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §5.6
- **Summary:** `view.proposal` (0067 §5) is a row of the schema map of §5.6 and of chapter 03 §5. (PENDING F28)

### A51. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** 0050 §14
- **Change:** relocates §1–§11

### A52. Source form, the shared view under a policy record, and `term` and `entity_ref` at scale

- **Date:** 2026-10-09
- **Source:** 0082 §1, §5, §5.1, §6
- **Change:** amends §5, §5.1, §5.2, §5.3, §5.6, §11
- **Summary:** The storage side of 0082's source form: no stored ID is rewritten when a cluster forms, shared rows are computed under the instance's policy record and a tenant only narrows them, and `view.term` and `view.entity_ref` are cut down to what a Wikidata-sized instance can hold. The chapter text is in git (`tools/adr.py log 0013 A52 --diff`). (REVIEW G1, G2, G3, G7)

Replaced text: 03 §4.1, §4.2, §4.3, §4.4, §5, §7; 22 §6.

### A53. The write path in three tiers

- **Date:** 2026-10-09
- **Source:** 0083 §1, §2, §3, §4, §5, §6, §7
- **Change:** amends §2, §5, §5.1, §5.4, §5.5, §5.6, §7, §9
- **Summary:** The storage side of 0083's three tiers: only tier 1 runs in the appending transaction, composition runs after commit, and the consumers of composition run on queues of their own, never in a transaction. §7's "projections 1–5 for the affected keys before commit" and the synchronous budget's inline fan-out are withdrawn. The chapter text is in git (`tools/adr.py log 0013 A53 --diff`). (REVIEW G8–G14)

Replaced text: 03 §2.1, §4.2, §4.5, §4.6, §5, §6.1, §6.2; 05 §3.8.

### A54. `lastrevid` is the newest local revision

- **Date:** 2026-10-09
- **Source:** 0084 §5
- **Change:** amends §5.1
- **Summary:** `view.entity.local_revid`, the `lastrevid` of the Action API, is the revision ID of the key's newest local record when it has one, and otherwise the provider-ranged ID of its newest mirror record; the base check of a write is per source graph, against the local partition's newest record for the key, and a mirror advancing is never a conflict. The 0084 table named §6 for this row; the chapter text that holds the rule is 03 §4.2, which §5.1 points to, so the entry lands here. (REVIEW G17)

Replaced text: 03 §4.2.

### A55. A mirrored entity's page ID is provider-ranged

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §5.1, §5.4, §6
- **Summary:** A mirrored entity's page ID is provider-ranged, `provider_number << 40 | upstream page ID` where the provider publishes one and `provider_number << 40 | mirror offset` otherwise, computed by the writer with no allocation and no read of `view`; `log."instance.page_id"` is not used for entities; the sub-2^40 range of the page ID space is the tenant's own sequence, so one `pageid` space covers local pages, local entities and mirrored entities on a tenant without collision; and `view.entity`'s unique key on the page ID is `(tenant, page_id)`, since a shared row and a tenant's overlay row for one entity carry the same page ID. Chapter 10 §3.1 says "per tenant". (REVIEW G18)

Replaced text: 03 §2.3, §4.2, §4.5.

### A56. The actor key is in the attestation part, never the header

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** corrects §5.4
- **Summary:** The actor key lives in the attestation part of a record and never in its header, except as the key of an actor record itself; the actor tables and `view.activity.actor_key` are projected from the attestation, which is why an attestation cut leaves the row with no actor; `view.activity`'s issuer index, and the `actor_local_name` index, filter on the tenant's derived issuer code (0079 §1), which `local` names. Chapter 07 §1.1's "what log headers carry" is struck. (REVIEW G19)

Replaced text: 03 §4.5.

### A57. No genesis records, manifests keyed per segment, `logid` for every tenant record, and concurrent appends

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §2
- **Summary:** Non-`config` partitions have no genesis record: a partition's name, hash function, `k` and policies are its `graph:` record in `config`, which the bundle carries, and `log.partition.hash` is denormalized from it. `log.segment_manifest` is keyed `(partition, segment)` with no `replaces` column and no `sealed_at` in the key; a manifest is signed once and never re-signed. A `hashed` partition has segment manifests only: its job-end "checkpoint" is the manifest of the last sealed segment plus an unsigned head offset, records in the open segment are uncommitted until sealed, and `log.checkpoint` rows exist for `logged` partitions only. Several processes append: the tree's right edge is read under the append lock and only complete subtrees are persisted, and checkpoints are signed by the projection worker under an advisory lock per partition, the only process holding the instance key. Chapter text: 03 §2.1, §2.2, §2.3. (REVIEW G24, G25)

Replaced text: 03 §2.1, §2.2, §2.3.

### A58. Surrogates are allocated under an advisory lock

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §6
- **Summary:** Surrogates are allocated in the appending transaction under an advisory lock on `(keyed_type, key)`, with the mapping record that binds the surrogate to its key appended in the same transaction as the first record that uses it; bulk jobs reserve surrogate blocks as they reserve entity ID blocks; a mapping is erased only when no live record in any partition is keyed to the surrogate. The ledger's verb is amends; §6's chapter text (03 §2.3) named the `log."instance.{type}_surrogate"` sequences and said nothing the rule contradicts, so this entry extends. (REVIEW G29)

### A59. Derivations naming a foreign subject are an overlay trigger

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §5
- **Summary:** A derivation that names a foreign subject is an overlay trigger, found by composition through `view.derivation_subject`: a tenant whose derivation names `WDQ123` gets its own `(tenant, id)` row for it, beside local assertions, cluster links and corrections as the things that make an overlay. The derived statement ID hashes the match key, never the resolved subject, so a later `same-as` changes no statement ID and orphans no correction. (REVIEW G34)

### A60. `view.actor` gains `editcount` and `created_at`; `pending` is OAuth-only

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §5.4
- **Summary:** `view.actor` gains `editcount` and `created_at`, maintained by the activity projection from the account's local `edit` rows, erased rows included and job rows excluded, and the implicit `autoconfirmed` group is computed from them; the status value `pending` is one that an OAuth-created subsidiary alone takes. The ledger's verb is corrects; §5.4's chapter text (03 §4.5) listed `pending` among the status values without saying who sets it and had no `editcount`, so nothing is contradicted and this entry extends. (REVIEW G40)

### A61. `view.entity_history` backs a mirrored entity's history

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §5.1; amends §5.5
- **Summary:** `view.entity_history (tenant, canonical_id, time, partition, offset, kind, member)` is the one backing of a mirrored entity's history, written by composition with one row per mirror record and per relevant upstream log event of every member of the cluster, so a history page is one keyset query bounded by the entity's own record count; upstream log events are rows of it and never of `view.activity`, which grows with local activity and jobs only; a watched mirrored entity's sync rows are read from it. The default mirrored provider log is `delete/*` and `protect/*`, the backfill covers held entities only, and `create/create` is derived from the first upstream revision. (REVIEW G42)

Replaced text: 03 §4.6.

### A62. Dumps from a replica, and the embedded backend's ceiling

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §11
- **Summary:** Dumps are taken from a replica under `pg_export_snapshot()` with parallel workers sharing the snapshot; the embedded query backend is the small profile's and is refused above a configured triple count (`query.embedded_max_triples`); the Wikidata profile requires `query.backend = remote`; `term_prefix` is kept as schema and never created or read by the large profile. (REVIEW G49)

Replaced text: 22 §6.

### A63. The projection order is a partial order over registered projections

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §7
- **Summary:** The projection order of §7 is a partial order over the projections a build registers, with `ops.projection_state` rows for those only; chapter 22 §3.1 names the milestone's crate and projection set, and the build order puts `scatter-log-postgres` before the file backend and its conformance suite. (REVIEW G52)

### A64. `private.account_request` and `private.challenge_spent`

- **Date:** 2026-10-17
- **Source:** 0087 §3, §6
- **Change:** extends §4
- **Summary:** Two `private` tables: `account_request`, the account requests awaiting review, deleted 7 days after a rejection or on expiry and "never leaves" under 0027 §2; and `challenge_spent`, the spent challenges until their expiry, for an instance without a shared cache. The chapter rows are 03 §5.

### A65. `private.rate_event`

- **Date:** 2026-10-18
- **Source:** 0087 A1
- **Change:** extends §4
- **Summary:** A `private` table, `rate_event` `(tenant, class, key, at)`, holds one row per act counted by the `createaccount` rate-limit class and the two `badlogin` counters where there is no shared cache, read as the rows inside each window and swept once older than the longest. Its keys are address keys and case-folded account names, as the cache would hold them; it is in no export. The chapter row is 03 §5.

### A66. `private.oauth_client`, `private.oauth_flow` and `private.binding`

- **Date:** 2026-10-18
- **Source:** 0088 §3, §5, §7
- **Change:** extends §4
- **Summary:** Three `private` tables: client credentials per tenant and issuer, `tenant = ''` the instance default, the secret sealed; login flow handles where there is no shared cache; and the index of external bindings, pointing at each binding record, with `added` and `last_used`. The chapter text is 03 §4.16 and §5.

### A67. Instance logins in `private`

- **Date:** 2026-10-18
- **Source:** 0089 §2, §3, §5, §10
- **Change:** amends §4
- **Summary:** `private.login` indexes the instance's `logins` partition. `private.password` is keyed by login, not by account. The chapter text is 03 §4.16.

Replaced text: 03 §4.16.

Replaced text: 03 §5.

Replaced text: 03 §5.

### A68. Adopted revision IDs; the surrogate sequence

- **Date:** 2026-10-19
- **Source:** 0035 A12; 0007 A27
- **Change:** amends §6
- **Summary:** `log.revision_id` is not taken for an adopted revision, which carries the source wiki's own revision ID below the floor, as an adopted record carries its page ID (0035 A12). The chapter text is 03 §2.3.

Replaced text: 03 §2.3.

### A69. DiscussionTools names and original times on `view.post`

- **Date:** 2026-10-20
- **Source:** 0090 §9
- **Change:** extends §5
- **Summary:** `view.post` gains `dt_name`, `dt_id` and `original`, filled from an imported post's `imported_from`, with partial indexes on `dt_name` and `dt_id` for `Special:GoToComment`. Chapter text: 03 §4.8.

Replaced text: 03 §4.8.

### A70. How a load enters bootstrap mode

- **Date:** 2026-11-05
- **Source:** Direct: Claude, while building bootstrap mode's first stage, 2026-11-05; unconfirmed
- **Change:** extends §9
- **Summary:** `triplespace sync` enters bootstrap mode by itself for an instance mirror whose partition is empty, whose provider has no key map and whose dump is JSON, and `--no-bootstrap` opts out. A key-mapped provider is excluded because a keyed subject's surrogate is minted on the write path (0009 §7); an XML dump because its pages are not independent lines. The records are the ordinary sync's for an empty partition, prepared on worker threads from the dump alone and given offsets and page IDs in dump order by one coordinator; its job carries `bootstrap: true` among its arguments. A load commits block by block, so a failure leaves no holes, and a rerun, the partition no longer empty, continues through the ordinary sync. Measured on the 20k slice of the 2M Wikidata benchmark: 112 to 225 entities/s, with the same records, tree and view rows as the ordinary sync and a rebuild identical to the load (`docs/testing/performance.md` §8).

### A71. Bootstrap projections run per block, during the load

- **Date:** 2026-11-05
- **Source:** Direct: Claude, while building bootstrap mode's set-based projections, 2026-11-05; unconfirmed
- **Change:** amends §9
- **Summary:** A bootstrap load projects as it goes, a block at a time: each synchronous projection takes the block's records at once (`Projection::apply_many`, defaulting to one at a time), set-based where it has a form for it (for a mirror's `put` of a subject no other graph or overlay holds: the entity source, the entity row with its terms and identifiers, and activity), record by record otherwise. The views are current after every committed block, a load that stops part way needs no projection pass, and the partition keeps its key index for the record-by-record path. Building indexes after the load and projecting in one pass over it stay open, should per-block projection become the limit. Measured on the 2M Wikidata benchmark: 1 h 44 min (321 entities/s) against 2 h 56 min with projections record by record, 0.13 statements per entity against 11.0, and the same view rows (`docs/testing/performance.md` §8).

Replaced text: 05 §3.8.

### A72. A bootstrap block's commit does not wait for the disk

- **Date:** 2026-11-05
- **Source:** Direct: James, design discussion of 2026-11-05
- **Change:** extends §9
- **Summary:** Each block transaction of a bootstrap load runs with `synchronous_commit` off (`SET LOCAL`, so no pooled connection keeps it): its commit does not wait for the write-ahead log to be flushed. A crash can lose the last few committed blocks but never leaves the partition or the views inconsistent, and the rerun continues through the ordinary sync from what survived. The final transaction, which writes the last block and finishes the job, commits synchronously, which flushes everything before it, so a load that has returned is durable. The wait had been 1 to 10 s of a 20 s load of the 20k slice of the 2M Wikidata benchmark, varying with the disk (`docs/testing/performance.md` §8).

### A73. A bootstrap load refreshes the planner's statistics as its tables double

- **Date:** 2026-11-05
- **Source:** Direct: Claude, while building bootstrap mode's set-based projections, 2026-11-05; unconfirmed
- **Change:** extends §9
- **Summary:** A bootstrap load runs `ANALYZE` over the tables of the `log` and `view` schemas after its 1st, 2nd, 4th, 8th… committed block (`IngestStore::refresh_statistics`; a store without a planner does nothing). The tables grow from empty faster than autovacuum's statistics follow, and with none the planner served each block's `DELETE FROM view.term … entity_id = ANY(…)` through `term_prefix`, which narrows by tenant only, rechecking every term row written so far: three-quarters of Postgres's time on the 20k slice and a load quadratic in its length. About twenty refreshes for a full Wikidata load, each about 4 s at 2M entities. On the 20k slice Postgres execution fell from 19.4 s to 6.1 s (`docs/testing/performance.md` §8).

### A74. A bootstrap load's projections compute ahead on the preparing threads

- **Date:** 2026-11-06
- **Source:** Direct: Claude, while building bootstrap mode's pipelining, 2026-11-06; unconfirmed
- **Change:** extends §9
- **Summary:** The worker threads that prepare a bootstrap load's records also do the projections' work that depends on a record alone, through a hook the load is given (`scatter_ingest::Warm`, set by `triplespace sync` to `triplespace_projections::common::warm`): with the operation a record's body was just encoded from, they keep it, its content hash and, for a mirror's `put`, the entity's shared rows but for the header's page ID and time, in a cache keyed by the content bytes, so that the coordinator applying the block decodes and resolves none of it and issues only the block's statements. The operation kept is taken to be what decoding the record gives, an operation having one canonical encoding; a test checks it on the load's own records, and the equivalence of a bootstrap load with the ordinary sync checks the views. The cache is the process's, kept in the order records are prepared and forgetting the oldest first, so what is computed ahead outlasts the blocks applied; what it has forgotten is computed on the coordinator as before. The alternative, overlapping a block's statements with the next block's computation on the coordinator, would leave that work on one block's critical path and need the next block's records before the current one is written. On the 20k slice a load fell from 14 s to 8 s and on the 2M Wikidata set from 31 min 51 s to 24 min 11 s, its peak memory rising from 1.81 GB to 2.35 GB (`docs/testing/performance.md` §8).

### A75. The projections take the operation the write path encoded, and the record in hand

- **Date:** 2026-11-06
- **Source:** Direct: Claude, while building the ordinary sync's optimizations, 2026-11-06; unconfirmed
- **Change:** extends §7
- **Summary:** Every change-set write hands the projections the operation it has just encoded into the record's content (`IngestStore::encoded`, which the Postgres store passes to `triplespace_projections::common::remember_encoded`), and the projections take it from their cache instead of decoding the content, as a bootstrap load's do (A74); this applies to writes through the API as much as to a sync. The entity projection takes the record being applied as it is when a source cursor names it, instead of reading it back by offset and checking its part hashes again, and the store returns the record it sealed to append it (`IngestStore::append_sealed`) instead of the write path sealing it a second time. The assumption is A74's: an operation decodes back to itself, which a test checks on the records a sync appends. Records read from the store are still checked as before. On the 20k Wikidata slice the ordinary sync's client CPU fell from 85 s to 65 s and its wall time from 148 s to about 138 s (`docs/testing/performance.md` §8).

### A76. A sync reads, decides and writes each unit as a block

- **Date:** 2026-11-06
- **Source:** Direct: James, design discussion of 2026-11-06
- **Change:** extends §7
- **Summary:** A sync (`run_sync_in`, so a dump sync and the follower alike) takes its items a unit of `batch` at a time (`--batch`, 200 by default), each unit its own unit of work as before. A unit of the common kind is written as a block: every subject its own key and named once in the unit, no state mapped from another upstream ID, no deletion that clears, and a provider that owns no keys. Its reads are made once for the unit (the subjects' cursors, the records at them, the revisions their keys have claimed, the keys' page IDs: `IngestStore::cursors`, `read_many`, `highest_revids`, `page_ids_of`, one statement each in Postgres); each item is decided as the item-by-item path decides it, by the same functions; and the unit's records are appended together (`append_many`, binary `COPY`) and projected over the block (`apply_inline_many`, the set-based path of A71), which a bootstrap load writes its blocks with too (`append_projected_many`). Any other unit, and a unit of one item, is written item by item. A block writes the same records, at the same offsets, with the same page and revision IDs, as writing item by item: page IDs are taken in item order, and since no two items name the same subject, nothing one writes is anything another reads. Choices made: the common case only, the key-moving logic of 0009 and 05 §4.2 left to the item-by-item path (rather than batching every kind of item); the block-writing half shared with bootstrap, which keeps its own entry and parallel preparation (rather than bootstrap becoming the batched sync on an empty partition); the existing `--batch` as the block (no new setting); recorded here rather than in a new ADR, since a sync's records and job are unchanged. A redirect's check that it is not already recorded is still made one at a time. On the 20k Wikidata slice the ordinary sync's first load fell from about 138 s to 67 s, and an update pass over it (2,000 changed, 2,000 new, 18,000 unchanged) took 61 s, against 108 s item by item; records, Merkle nodes, views and job counts the same, a rebuild identical (`docs/testing/performance.md` §8).

### A77. A sync's per-item work runs on every core

- **Date:** 2026-11-06
- **Source:** Direct: Claude, while building the ordinary sync's parallel preparation, 2026-11-06; unconfirmed (James approved the direction: parsing and the per-item work on worker threads, as bootstrap mode does)
- **Change:** extends §7
- **Summary:** The ordinary sync's per-item work, which needs no database, runs on the machine's threads. The calls made: (1) a JSON dump of a provider with no key map is parsed and put through the adapter on worker threads and handed to the sync in dump order (`triplespace sync`, the reader and workers bootstrap mode uses, made generic: `bootstrap::items`); such a dump has no first pass and nothing carried from one entity to the next, the same condition bootstrap mode asks of its dumps; an XML dump and a key-mapped provider's are read on the caller's thread as before; the summary of snaks left untyped is not printed on this path, as on bootstrap's, since a single-pass JSON read has no dump-defined types to apply. (2) In a unit written as a block (A76), each state's reconciliation with the provider's hashes, the decoding of its previous record, the unchanged test and its `put` are computed on every core (`scatter_projection::par_map`, moved there from `triplespace-projections` so that `scatter-ingest` can use it; no new dependency), the hash mismatches handed back to the job; and the block's records are encoded and hashed on every core, page IDs having been settled in item order first. The unit's reads, appends and projections stay on its connection, in order. The records written are unchanged. On the 20k Wikidata slice the ordinary sync's first load fell from 67 s to 39 s and an update pass (2,000 changed, 2,000 new, 18,000 unchanged) from 61 s to 13 s; records, Merkle nodes, views and job counts the same as writing item by item, a rebuild identical (`docs/testing/performance.md` §8).

### A78. `triplespace dump` writes the Wikibase-compatible and local-graph dumps into `dumps.dir`

- **Date:** 2026-11-07
- **Source:** Direct: James, design discussion of 2026-11-07
- **Change:** extends §8
- **Summary:** Building the dumps before the job runner and the blob store exist, James chose that the CLI writes them, `triplespace dump --tenant {slug}`, run by hand or by cron, into a directory per tenant under the deployment setting `dumps.dir`, from which the server serves `{base}/dumps/`; the job runner and `scatter-blob` take over the writing later without changing the URLs. Of the three products, the Wikibase-compatible dump and the local-graph source dump are built now; the full dump waits for the metadata graph (`scatter-mwlog`), and the update-stream cursor for the stream, a manifest's `cursor` being `null` until then. Chapter text: 02 §4.1; 23 §3.

### A79. A dump's layout, files and manifest, and how it is served

- **Date:** 2026-11-07
- **Source:** Direct: Claude, while building the dumps (SCA-301), 2026-11-07; confirmed by James, 2026-11-08
- **Change:** extends §8
- **Summary:** Calls made building A78. (1) One directory per dump, `{dumps.dir}/{tenant}/{YYYYMMDDTHHMMSSZ}/`, named by the snapshot's time, written as `.{name}.partial` and renamed once complete, holding both products: `wikibase.nt.gz` and `wikibase-skolem.nt.gz` (N-Triples, the resolved view being one graph), `local.nq.gz` and `local-skolem.nq.gz` (N-Quads in `{base}/graph/local`), `local.json.gz` (one entity per line with its newest local record's partition, offset and revision ID), and `manifest.json` (tenant, base, time, whether from a replica, RDF format version, cursor, entity counts, and per file its records, size and SHA-256). (2) The resolved view is written as `dumpRdf.php --flavor full-dump` writes an entity, with the upstream naming of 0002 A43; the local graph without truthy triples (05 §1.4) but with `wikibase:BestRank` computed within the graph. (3) Each file opens with a comment naming the base, the time and the cursor, the stamp 02 §6.4 asks for. (4) A dump holds what an anonymous reader may read, entity by entity through the read check `.json` uses; a tenant an anonymous reader may not read gets no dump, and `{base}/dumps/` serves nothing for it, whatever was written while it was public. (5) The tenant setting `dumps.keep` (default 7), defined for scope dumps by 0076 §1, also sets how many of the tenant's own dumps are kept. (6) Reference and value nodes already written are remembered per 10,000 entities and then forgotten, bounding memory as Wikibase's dumper does; a node met again is written again, which a set of triples does not mind. (7) One worker reads the snapshot; the exported snapshot is there for the parallel workers 02 §4.1 asks for. (8) `GET /dumps/` answers JSON, the manifests newest first with each file's URL; files are served as `application/gzip` with the SHA-256 as the `ETag`, immutable but for `latest`. Chapter text: 02 §4.1.

### A80. The full dump is an export, never a backup

- **Date:** 2026-11-07
- **Source:** 0103 §5
- **Change:** amends §8
- **Summary:** The full dump carries the resolved view, the exportable source graphs and the metadata graph, each in its visible form; it is published at `{base}/dumps/` like the other dumps and is no longer described as being for backups, which are the operator's copies of the database (0103 §1). Chapter text: 02 §4.1.

Replaced text: 02 §4.1.

### A81. Each dump carries the public bundle of the `local` partition

- **Date:** 2026-11-07
- **Source:** Direct: Claude, while building the public bundle (SCA-302), 2026-11-07; confirmed by James, 2026-11-08
- **Change:** extends §8
- **Summary:** 17 §1.1 publishes an export bundle of the `local` partition "one bundle per snapshot" at `{base}/dumps/local/bundle/`. Calls made building it. (1) `triplespace dump` writes it into the dump's own directory, `bundle/`, in the dump's `REPEATABLE READ` snapshot, so the bundle and the RDF files describe one instant; it is the export scope of 0103 §1 for an anonymous reader, decided as `export --scope export` decides (0006 A31), and carries the checkpoints signed so far, so a dump taken after `triplespace checkpoint` verifies against them. (2) It is served as its directory tree, file by file, not as an archive: `GET /dumps/local/bundle/` lists the dumps that carry one with its origin, record, withheld and checkpoint counts (also in the dump's `manifest.json` as `bundle`), `GET /dumps/local/bundle/{name}/` lists its files and `GET /dumps/local/bundle/{name}/{path}` serves one, every path segment checked so nothing outside the bundle can be named; a reader fetching segments one at a time needs no archive tool, and a tree needs no new dependency. (3) The bundle is checked at level 1 as it is written. (4) The code `triplespace export` and the dump share (partitions, origins, `key:` records and the export scope's reader) moves into `triplespace-rdf`. Chapter text: 17 §1.1.

### A82. A bootstrap load writes several blocks at once and commits them in order

- **Date:** 2026-11-08
- **Source:** Direct: James, design discussion of 2026-11-08
- **Change:** amends §9
- **Summary:** A bootstrap load writes up to `--writers` blocks at once (4 by default), each in a transaction of its own, and commits them in dump order. One coordinator gives each block, in order, its page IDs, its offsets and the tree nodes its leaves complete, keeping the partition's Merkle frontier in memory; each block's transaction then `COPY`s its records and nodes and runs its projections, and its last statement, which moves the partition's next offset from the block's first offset to past its last, runs only once every earlier block has committed, followed by the commit (the projections' positions are written at commit). So what has committed, and what a crash leaves, is always a prefix of whole blocks, with the records, tree, views and positions agreeing, as with one block at a time; a statement that finds the next offset elsewhere fails rather than commit out of order. The text of §9 had a coordinator hand writers offset blocks from an `ops.bootstrap_block` table and hand out again the block of a writer that failed, with the tree folded in one pass after the load; neither was built, the load having so far written one block at a time with each block's tree nodes, and the commit order now keeps a failed block from leaving a hole instead. Why several blocks: one block at a time left Postgres waiting on the client and the client on Postgres, neither near its limit. Why 4: on the full Wikidata dump on 16 cores, 40 minutes from empty loaded 4.81M entities with 1 writer, 5.96M with 2, 7.15M with 4 and 6.57M with 8; from 2 writers up, the 14 threads preparing records ran at 93–98% in dense stretches, so preparation, not writing, became the limit, and 8 only added connections. No relationship with the number of cores was found to set it by. Loading the dump to 40M entities with 4 writers took 4 h 00 min, against 5 h 07 min for the whole-dump load's first 40M with one, the gain growing as the database outgrew memory (`docs/testing/performance.md` §8).

Replaced text: 05 §3.8.

### A83. How a bootstrap load keeps its blocks apart

- **Date:** 2026-11-08
- **Source:** Direct: Claude, while building bootstrap mode's concurrent blocks, 2026-11-08; unconfirmed
- **Change:** extends §9
- **Summary:** Calls made building A82. (1) A block waits for every earlier block to commit before its transaction starts when it holds a record other than a `put`, names a subject twice, or names a subject a block still in flight names. A block's projections read what earlier blocks wrote about its subjects, which they cannot see before those commit; and a later block that reached a row first would hold it while the earlier block, inside the database, waited on it, and it, in the client, on the earlier block, a deadlock Postgres cannot see. A JSON dump of new subjects meets none of these, so the rule costs nothing on the common case; the race it avoids was not reproduced, and a test checks the rule itself. (2) A block that fails stops the load: every block after it rolls back rather than commit, so no transaction is left open and nothing past the failure commits; the rerun continues through the ordinary sync as before (A70). (3) Page IDs are reserved for each block in a short transaction of the coordinator's, committed at once, so that the blocks need not wait on one another for the sequence; the IDs a rolled-back block reserved are not reused, a gap like any sequence's. (4) The job's start commits on its own before the first block, rather than with it. (5) A store says whether it can run several units of work at once (`IngestStore::concurrent_blocks`, with `write_sealed` and `advance_partition`); the in-memory store cannot, and writes one block at a time whatever `--writers` asks. (6) The command's connection pool is the writers and four more. The blocks in flight are driven on the coordinator's own task, polled between the items it takes, so no runtime or new dependency is named.
### A84. The job queue

- **Date:** 2026-11-08
- **Source:** 0104 §8
- **Change:** extends §5.6
- **Summary:** `ops.job_queue`, a submitted job waiting for the runner, keyed by the job ID minted at submission, claimed under `SELECT … FOR UPDATE SKIP LOCKED` by one runner task per partition, and deleted when the job's finish or fail record is written. Chapter text: 03 §4.12.

### A85. A bootstrap load leaves signing to the checkpoint command

- **Date:** 2026-11-09
- **Source:** Direct: James, design discussion of 2026-11-09
- **Change:** amends §9
- **Summary:** A bootstrap load does not sign its checkpoints: they are signed as every partition's are, by `triplespace checkpoint`, until 0006 Q4 settles the cadence a worker signs on. The step is deferred, not dropped. Chapter text: 05 §3.8.

Replaced text: 05 §3.8.

### A86. A snapshot sweep writes its operations in blocks

- **Date:** 2026-11-11
- **Source:** Direct: Claude, while building the snapshot sweep's block write, 2026-11-11; unconfirmed (James asked for the sweep to be moved to the block path)
- **Change:** extends §7
- **Summary:** The snapshot sweep's operations (the tombstones and clearing `put`s for what the job did not see) are written in blocks, by the block write of A76 (`write_block`: one lookup of page IDs, `append_many`, `apply_inline_many`), rather than one at a time. The calls made: (1) the operations are taken in the order the sweep lists them, and each run of up to `batch` consecutive operations a block can take (a subject that is its own key, named once) is one block; an operation no block takes (a keyed entity's clearing `put`, whose key is its surrogate) is written by itself in its place, as is a run of one, so the records, offsets, page IDs and revision IDs are those of writing one at a time (rather than writing the block-shaped operations first, which would reorder the records). (2) The sweep stays in the one unit of work that finishes the job, as before: it is all written or none of it, and `batch` sizes its blocks, not its commits. (3) Deciding the sweep is unchanged: a clearing `put` of a `deletion = clear` provider still reads its subject's cursor and previous record one subject at a time; only the writing is in blocks. Chapter text: 03 §6.

### A87. One block write for every graph

- **Date:** 2026-11-11
- **Source:** Direct: Claude, while building the general block write, 2026-11-11; unconfirmed (James asked for the block write's core to be generalized so that adoption, the NDJSON job and imports can use it, and accepted that a streamed batch then commits a block at a time)
- **Change:** extends §7
- **Summary:** The block write of A76 is one function for every graph (`scatter_ingest::write_many`), of which a sync's block (`write_block`) is the case with one request, a mirror or source graph and subjects named once. No job but the sync and its sweep uses it yet: adoption, the NDJSON job and the imports still write one record at a time. The calls made: (1) each record of a block has its own request (who is responsible, the comment, the revision ID it brings), since an adoption or an import attributes every revision to its author; the tenant is the block's, and the fan-out budget is given for the block. (2) A block is written to a tenant's `local` or `pages` as well as to a mirror or source graph: a tenant record takes the revision ID its request brings or the tenant's next, the sequence taken once for the block and handed out in record order. (3) A key with no page ID takes what its first record in the block brings (an adoption's source page ID, a caller-encoded record's) or the next of the sequence, in record order, and keeps it for the rest of the block; a caller-encoded record with no key is keyed by the page ID it mints. These are the IDs writing one record at a time gives. (4) A key may come more than once in a block. The records are appended together, but the synchronous projections run over each stretch of records in which no key repeats, one stretch after another, rather than over the whole block: applied over a block, a projection sees what the earlier projections wrote for all of it, which for two records of one key is not what it sees one record at a time. This is the cautious choice: on the test's block the views came out the same without the stretches, so it may be relaxed once the projections are audited for it; until then a run of revisions of one entity is appended together and projected one record at a time. (5) A keyed entity's surrogate is looked up, or minted, before the block is written, one entity at a time (`key_for`); the lookups are not yet batched. (6) Caller-encoded records (`RawRecord`, a page's) go in a block as operations do. (7) A block makes no base-offset check (0006 §8) and refuses a record that carries one: such a write is interactive and is written by itself. Checked against writing one record at a time, in memory and over Postgres with the real projections: the same records, offsets, page and revision IDs, and the same rows in every `view` table. Not done here: the projections' set-based forms for tenant records (the entity projection applies every record outside an instance mirror one at a time, in a block too), so a tenant block saves the append's and the lookups' round trips, not the projections'. Chapter text: 03 §6.

### A88. An adoption writes its revisions in blocks

- **Date:** 2026-11-11
- **Source:** Direct: Claude, while building the adoption's block write, 2026-11-11; unconfirmed (James asked for adoption to be wired onto `write_many`)
- **Change:** extends §7
- **Summary:** An adoption (`run_adoption`; 0035 §3) writes each unit's revisions with the block write of A87, entity revisions to `local` and page revisions to `pages`, rather than one at a time. The calls made: (1) each revision is decided as it arrives, as before (validation, the resume check, its author's surrogate), and queued; the queue is written when the unit of `batch` revisions ends, so a unit is still one unit of work and commits what it did before. (2) Entity and page revisions are not mixed in a block: the queue is written when the other kind begins, so that the revision IDs a `--current-only` adoption mints are taken in the order the revisions come. (3) The resume check reads the store for an entity or page, so the queue is written first when the job comes back to one whose records wait in it, and when a revision has to be compared with the content of one that waits (the same revision twice in a dump); otherwise the newest revision of the entity the job is on is known from the queue. (4) An author's surrogate actor record is written when its revision is decided, ahead of the block that holds the revision: it is in the `actors` partition, with its own sequences, so its place among the `local` and `pages` records does not change. (5) The resume check's reads are still one entity at a time. The records, counts and every `view` table are those of the adoption that wrote one revision at a time, checked over Postgres against that code on a history with repeated entities, surrogate and hidden authors, a redirect, pages between entities and a re-run. Since A87 projects a run of one key's revisions one at a time, the gain is the append's and the lookups' round trips: the Librarybase sample (295 revisions of 254 entities, release build, cloud sandbox) adopts in about 0.63 s against 0.89 s. Chapter text: 03 §6.

### A89. The projections say where a key may repeat in a block

- **Date:** 2026-11-11
- **Source:** Direct: James, design discussion of 2026-11-11 (the per-projection declaration, chosen over a rule in `write_many`, after the audit A87 left open); the rule each projection gives is Claude's, while building it, unconfirmed
- **Change:** amends §7
- **Summary:** A87 (4) ended a block's stretch at every repeated key until the projections were audited. They were: of the synchronous projections that accept a tenant's change sets and pages (revision state, entity source, keyed map, entity, activity, page), only the entity projection reads another projection's rows (the entity sources and the graph state), and it recomputes its subject's rows from them, so over a whole block it leaves the rows it leaves one record at a time but for two columns of `view.entity` that follow the key's history: `resolved_version`, which starts again when the entity goes and comes back (a state, a redirect, a state: 1 one at a time, 3 over the block), and `first_seen`, the earliest time the key's records carry (a block finds the last). Both were reproduced over Postgres with the stretches off, and nothing else differed. Decided: a projection says whether a key may repeat (`Projection::repeats_in_block(earlier, later)`, the earlier records of the key in the stretch and the one arriving; the default is no), and the pipeline ends a stretch before a record whose key it holds unless every synchronous projection that accepts the record says yes (`Pipeline::stretches`, `apply_inline_stretches`, which `write_many` calls), so that `scatter-ingest` knows nothing of what a projection reads. The answers given: revision state, entity source, keyed map and page say yes; activity says yes for change sets and pages and no for the other payloads, whose rows were not audited; the entity projection says yes for a local entity whose records in the stretch were appended at one time, the first a whole state (a `create`, or an `adopt` with a state and no redirect) and the rest whole states or `add`s, and no otherwise. A run of an adopted entity's revisions is then one stretch; a redirect, a withheld revision or a deletion ends it. Not audited, and so still kept apart: thread, ACL and actor records, and a mirror's records where tenants overlay them (a sync's block names each subject once anyway). On the Librarybase sample, which has few repeated entities (295 revisions of 254), adoption took about 0.55 s against 0.59 s. Chapter text: 03 §6.

Replaced text: 03 §6.

### A90. A tenant's NDJSON batch writes its operations in blocks

- **Date:** 2026-11-11
- **Source:** Direct: James, design discussion of 2026-11-11 (the batch job moved to the block write; a streamed batch commits a block at a time rather than an operation at a time); the rest is Claude's, while building it, unconfirmed
- **Change:** amends §7
- **Summary:** A tenant's bulk job over an NDJSON batch (`run_batch`; 0002 §8.5) writes its operations with the block write of A87, 200 at a time (`batch::BLOCK`, a sync's unit), rather than one at a time. The batch is prepared whole before anything is written, as before. The calls made: (1) consecutive operations for one graph are a block (`local`, or `pages` for a page's statements), so the page and revision IDs are taken in the order the operations come. (2) An atomic batch is still one unit of work: its blocks are written inside it, and the operations before the first line that has problems are written before that line fails the batch, so a failure among them is still the one reported. (3) A streamed batch commits a block at a time. A block in which a write fails is rolled back and its operations are written again each in a unit of work of its own, as every operation was before: the one that fails is rejected with its line and the rest land. A line with problems is rejected after the operations before it are written, so the rejects stay in batch order. (4) What differs from one operation at a time: a streamed batch that is interrupted has landed whole blocks rather than every operation up to the interruption; and since a sequence does not give back what a rolled-back block took, a block that fails leaves a gap in the tenant's revision and page IDs the size of the block rather than of one operation. With no failed write the records, their IDs, the counts, the rejects and every `view` table are those of the job that wrote one operation at a time, checked over Postgres against that code, streamed and atomic. A batch of 2,000 `create`s (release build, cloud sandbox, preparation included) takes about 2.6 s against 7.7 s streamed and 2.6 s against 5.4 s atomic. Chapter text: 03 §6.

Replaced text: 03 §6.

### A91. An import writes each page as a block, and a few records are written one at a time

- **Date:** 2026-11-11
- **Source:** Direct: Claude, while building the import's block write, 2026-11-11; unconfirmed (James asked for the XML import to be moved to the block write)
- **Change:** amends §7
- **Summary:** The general import job (`run_import`; 0008 §9) writes each page's revisions, and the restoring edit after them, as one block (`write_raw_many`, the block write of A87 for caller-encoded records, which needs no table of providers), rather than one at a time. A page is still its own unit of work, decided from what it held before the job, so what an interrupted import has committed, and the skip rule that makes a second run write nothing twice, are unchanged. The calls made: (1) a new page of several revisions takes its page ID from the sequence before its block, for all its records; a new page of one revision leaves that to the write, as a record with no key, as before. (2) Pages are not batched together: that would commit several pages at a time, which was not asked for. (3) No more than four records (`write::SMALL_BLOCK`) are written one at a time by every caller of the block write, the same records by the same functions as before A87: a block spends on itself (the lookups for the block, two `COPY`s) what it saves on each record, and on an import of 600 revisions in pages of so many each it took 2.1 s against 1.5 s at one revision a page, as long at five, and less above. With the rule: 1.6 s against 1.6 s at one revision a page, 0.9 s against 1.0 s at ten, 0.7 s against 1.0 s at a hundred (release build, cloud sandbox). The records, counts and every `view` table are those of the import that wrote one revision at a time, checked over Postgres against that code on a page of six revisions with registered, imported and hidden authors and a withheld text, a second run, and older revisions merged behind a restored newest. Chapter text: 03 §6.

Replaced text: 03 §6.

### A92. A sync's unit is taken in blocks and single items, where they come

- **Date:** 2026-11-11
- **Source:** Direct: Claude, while building the sync's mixed units, 2026-11-11; unconfirmed (James asked for the sync units that fall back to be moved to the block write)
- **Change:** amends §7
- **Summary:** A76 wrote a unit as a block only when every item in it was of the common kind and the provider owned no keys; one other item sent the whole unit, and a provider that maps keys its whole sync, to the item-by-item path. A unit is now taken in stretches, in item order (`sync::segments`): each run of consecutive items a block can take, no two on the same subject, is a block, read, decided and written as A76 has it; an item no block takes is taken by itself, by the item-by-item path, where it comes; and a run of one is taken by itself too. The unit is still one unit of work. The calls made: (1) what makes an item one a block can take is asked of the item, not of the unit or the provider (`sync::in_block`): its subject is its own key and deciding it reads and writes nothing but that subject's records. So a state mapped from another upstream ID, a keyed subject and a clearing deletion are taken by themselves, as before; and where A76 asked that the provider own no keys, a state is now taken by itself only when its own upstream item owned a key before the job, which is the one case in which a state not mapped onto a key reads another subject's records (the key's, for the revision it recorded). (2) A subject named again ends the run and begins the next, rather than sending the unit item by item. (3) A block is decided when the job comes to it, after everything before it in the unit is written, so an item taken by itself that retires a subject an earlier block wrote, or that a later block writes, finds and leaves what it did item by item. The records, counts, owners of keys and every `view` table are those of the sync that fell back, checked over Postgres against that code on three passes over a provider that maps keys: mapped states among foreign ones, a key that moves, a foreign item that becomes mapped and retires what a block wrote, an item that stops owning a key, a keyed deletion, a subject twice in a unit, and a snapshot. A sync of 4,000 items of which every fortieth is mapped onto a key (release build, cloud sandbox, units of 200) takes about 1.6 s against 5.7 s for a first load and 2.0 s against 9.8 s for a pass that changes every item. Chapter text: 03 §6.

Replaced text: 03 §6.

### A93. The projections apply a tenant's block a key at a time, the keys together

- **Date:** 2026-11-11
- **Source:** Direct: Claude, while building the projections' block forms for tenant records, 2026-11-11; unconfirmed (James asked for the projections to be done next, starting with a measurement)
- **Change:** amends §7
- **Summary:** Measured first (release build, cloud sandbox, `pg_stat_statements` and a timer on each projection, 2,000 records each): with the writes in blocks, the synchronous projections took 89% of a streamed batch of `create`s (3.0 s; the entity projection half of it, revision state a quarter), 95% of a history adoption of 200 entities of ten revisions (2.4 s; the entity projection 59%) and 94% of an import of 20 pages of 100 revisions (2.6 s; the page projection 79%), at 12 to 16 statements a record sent one after another, of which Postgres's own time was under a third. The calls made: (1) a projection whose rows for a record are its key's alone applies a block a key at a time (`common::by_key`): a key's records in order, the keys together, thirty-two at a time, so that the driver pipelines their statements on the one connection rather than each waiting for the answer before it. The statements are those of applying the records in order, and so are the rows; no SQL changed. Revision state, entity source (where the block is not all `put`s) and activity (for change sets and pages) do so for every block; the entity projection for a tenant's partition when no record is a `convert`. (2) Where a key repeats in a block and A89's rule holds for its records, the entity projection resolves the key once, after its last record, and moves `resolved_version` by the number of records, which is what resolving it for each record leaves. (3) Left as they were: a shared partition's records in the entity projection, since they reach every tenant's overlay; a `convert`, which names two entities; and the page projection, whose records reach other pages' rows (a redirect's target), so that pages cannot be applied together, and which was the whole of the import's time. After: the batch 1.6 s, the adoption 0.6 s, the import unchanged at 2.2 s. The records and every `view` table are those of one record at a time, by the tests of A87 to A92, with a block of more keys than are sent at once added. Not done: statements for a block rather than for a record (a multi-row form per table), which is what is left of the batch's time, and a block form for the page projection. Chapter text: 03 §6.

Replaced text: 03 §6.

### A94. The page projection applies a run of one page's revisions as one

- **Date:** 2026-11-11
- **Source:** Direct: Claude, while building the page projection's block form, 2026-11-11; unconfirmed (James asked for the page projection to be worked on next)
- **Change:** amends §7
- **Summary:** A93 left the page projection applying a block one record at a time, at about ten statements a revision, which was four fifths of an import's time. Of what a revision writes, only its text under its revision ID is the revision's own; the page's row, its redirect, its links and its categories are the page's as its latest text leaves them, written again by every revision. The calls made: (1) the projection applies a block's records in order and pages never together, as before, since a page's record reaches other pages' rows (a `create` resolves the redirects that name its title; a redirect looks its target up by title). (2) A run of consecutive records of one page that is a `create` and the `edit`s after it, or `edit`s alone, is applied as one (`pages::apply_run`): the `create` as it was; the `edit`s as one update of the page's row, with the last revision's offset, revision ID and time, the length and content model of the last revision that carries one, and `revisions` moved by their number; every revision's text in one statement; the redirects naming a created page's title resolved once; and the last text of the run indexed once (the redirect, links and categories). The `view` rows are those of one record at a time. (3) What differs is outside `view`: the report keys queued (`ops.projection_work`) are the page's title and the links and categories the run removes or leaves, queued once at the run's last offset, where one record at a time queued every revision's at its own offset. A link or category that came and went inside the run is not queued, and changes no report, since the page neither held it before the run nor holds it after. (4) A run with a `move`, an erased record or a `create` after its first record goes one record at a time; so does a run of `edit`s the page's row does not take from the first (its offset is not behind the row's, which is records applied again), since then some would apply and some not. An import of 20 pages of 100 revisions takes about 0.6 s against 2.4 s, and of 60 pages of ten 0.5 s against 0.9 s (release build, cloud sandbox). Checked over Postgres: an adoption of pages whose histories change links and categories, pass through a redirect, withhold a text and end as a redirect leaves the same rows in every `view` table as one revision to a unit, and an import the rows the projection left before. Chapter text: 03 §6.

Replaced text: 03 §6.

### A95. Three projections write a tenant's block a table at a time

- **Date:** 2026-11-11
- **Source:** Direct: Claude, while building the projections' multi-row statements, 2026-11-11; unconfirmed (James asked for multi-row statements next)
- **Change:** amends §7
- **Summary:** A93 left a statement for each record and table, pipelined; a batch of `create`s still sent about sixteen a record, and Postgres's own time on them was then half the job. Three projections now write a block with a statement a table where the block allows it, and fall back to A93's key-at-a-time path where it does not. The calls made: (1) **revision state** reads the states of the block's keys in one statement, folds each key's records onto its state in memory in record order, and writes `view.graph_state` (each key's state after its last record, its version moved by the number of its records) and `view.entity_revision` (the state after each record that has a revision ID) in one statement each. A key with no stored state is seeded from its earlier records as before (0083 A1); one statement finds which such keys have any record before the block, so a key the block is first to write costs no read of its own. A block that names a revision ID twice goes a key at a time, since one statement cannot write a row twice. (2) **Entity source** writes each entity the cursor its last record in the block leaves, in one statement, for any block in which no record removes a cursor (a `tombstone`, an upstream `redirect`); before, only a block of `put`s that named each subject once. (3) **The entity projection**, for a tenant's `local` block, takes the entities whose one source is that graph: a local ID with no other row in `view.entity_source`, whose stored state is at the block's last record of it, which resolves to something, and whose records in the block may be resolved once (A89, A93). It reads their sources and their states in two statements, resolves each in memory as `resolve_subject` does (the local state and its corrections; `source` or `materialized` by the same test; the page ID, offset and revision ID of the last record; `first_seen` from the source's row; `resolved_version` moved by the number of records), and writes `view.entity` in one upsert with the conflict rule of the single form, and the terms and identifiers in one delete and one insert each. Every other entity of the block (a foreign or keyed ID, one a mirror or an overlay also holds, one that resolves to nothing, one whose records may not be resolved once) goes a key at a time as before. (4) Activity still writes a row a statement, pipelined; its row has too many columns of too many kinds to be worth a block form for a twelfth of the time. A batch of 2,000 `create`s takes about 0.76 s against 1.5 s, and a history adoption of 200 entities of ten revisions 0.35 s against 0.6 s (release build, cloud sandbox); against 3.0 s and 2.4 s before A93. The rows of every `view` table are those of one record at a time, by the tests of A87 to A94 and one added for a block that seeds a state from the records before it. Chapter text: 03 §6.

Replaced text: 03 §6.

### A96. Compaction keeps the record that claims a key's highest revision, and runs by a range of offsets

- **Date:** 2026-11-11
- **Source:** Direct: Claude, while building the follower's compaction, 2026-11-11; unconfirmed (James asked for the whole-dump mirror to be compacted, which nothing did)
- **Change:** amends §3
- **Summary:** Nothing called compaction: the whole-dump Wikidata mirror, a `latest` partition, held 125,513,063 records for 122,231,562 entities, every followed edit adding a whole state and none removed, about 7.2 KB an edit with the record's body and index entries. Building it found that "every record for a key but the newest" would break the guard 0092 §2 relies on: a mirror's sync reads the highest revision ID any record under a key carries, so as never to claim a revision twice, and a deletion or redirect is written without one, so a key whose newest record is a tombstone or redirect would lose every revision it had claimed, and an entity undeleted at a revision already recorded would claim it again. On the mirror, 150 of 99,661 keys written in a run of 100,000 offsets were such keys. The calls made: (1) compaction keeps a key's newest record and, where it is an older record, the one carrying the key's highest revision ID (ties to the newer), and removes the rest (`scatter_log_postgres::log::compact_keys`). (2) It is asked of a range of offsets: the keys of the records at `from..to`, less any key a tenant retains (`view.entity.retention = 'retain'`, matched on the entity ID the key names), wherever their other records are (`IngestStore::compact_range`); a partition whose history policy is `full` is left alone. (3) It runs 10,000 offsets to a unit of work, each under the partition's append lock (`scatter_ingest::compact`), so that appends wait at most one window. (4) `triplespace compact --provider {slug} [--from] [--to] [--window]` runs it by hand over a registry provider's mirror. Left open: a tombstoned entity that is not retained still keeps the record that claims its highest revision, which the text's "loses every record for its key" does not allow for; until tombstones are removed, which nothing does yet, the two do not meet. The space removed rows held is reused once the table is vacuumed; the files do not shrink. Chapter text: 01 §6.

Replaced text: 01 §6.

### A97. The roles and grants converge on every migration run

- **Date:** 2026-11-12
- **Source:** Direct: Claude, while building the restore path for rehosting, 2026-11-12; confirmed by James, 2026-11-12
- **Change:** extends §4
- **Summary:** Migrating an instance by dumping and restoring its database required creating the five roles by hand: roles are the server's, `pg_dump` does not carry them, and `0004_roles` is recorded in the restored `ops.migration`, so `triplespace migrate` never ran it again. The calls made: (1) `triplespace_db::grants::ensure` creates any role the server lacks and gives every grant the migrations give, then each partition's, and the migration runner calls it at the end of every run, applied or not, in one transaction; `0004_roles` is left as it was, since a recorded migration is never edited. (2) The grants are a copy, `grants::GRANTS`, of every `GRANT` and `ALTER DEFAULT PRIVILEGES` in the migrations; a unit test fails when a migration adds one the copy lacks, and a database test revokes every grant, migrates again and compares. (3) A partition named `accounts` is the one hidden from every role but `ts_accounts`. (4) `grant_partition`, until now called only by tests, is called where the three production paths create a partition (a tenant's partitions, its `config` partition, an entity source's), so the accounts partition's exclusion now holds in production. (5) The documented restore is `pg_restore --no-owner --no-privileges` then `triplespace migrate`, so that the binary, not the dump, decides every grant (08 §6.7). (6) The runner creates the roles before it applies any migration, so that `0004_roles` always finds them made: two sessions creating one role at once fail with `unique_violation`, which `0004_roles` does not catch, as two databases on a new server migrating together, or the test suite's first run there, would; `ensure_roles` catches both.

### A98. Row-level security hides the accounts partition in `log.record`

- **Date:** 2026-11-12
- **Source:** Direct: Claude, while building the accounts partition's exclusion from the parent table, 2026-11-12; confirmed by James, 2026-11-12
- **Change:** amends §4
- **Summary:** Postgres checks privileges only on the table a query names, so revoking the accounts partition's child table left its rows readable, and writable, through the parent `log.record`, to which every role is granted (Q11). Migration `0034_record_policies` enables row-level security on `log.record` with two policies: `ts_accounts_reads_all` admits every row to `ts_accounts`, and `ts_public_skips_accounts` admits to `ts_server`, `ts_notify`, `ts_federation` and `ts_verify` every row whose partition is not in `SELECT partition FROM log.partition WHERE name = 'accounts'`, for reads and writes. The calls made: (1) **The form of the check.** Measured as `ts_server` counting a 200,000-row partition, against 12 ms with no policy: a function called per row took 812 ms; an array read once per query 27 ms with one accounts partition and 401 ms with 1,001, since `<> ALL` over a parameter is a linear scan; a correlated `NOT EXISTS` 129 ms; a literal array 13 ms at any size, but it would be rewritten, under an exclusive lock on `log.record`, whenever a tenant is created; an uncorrelated `NOT IN`, which Postgres hashes once a query, 18 ms with 1 or 1,001. The `NOT IN` was chosen: flat in the number of tenants, and no DDL after the migration. `log.partition.partition` is `NOT NULL`, so `NOT IN` has no null trap. (2) **Restores.** A policy names roles, so a restore onto a server without them leaves it out, and the table, its security enabled, then shows those roles nothing; `grants::POLICIES` re-creates each policy that is missing on every migration run (A97), and leaves one that exists alone, since creating a policy locks the table. (3) **The owner bypasses row-level security** (it is not `FORCE`d): every service connects as the owner today, so nothing changes in production until services connect as the roles. (4) Partitions are recognised by the name `accounts`, as `grants::ensure` does; packed record storage (0058), when built, will hold accounts bodies outside `log.record` and needs the same exclusion.

Replaced text: 03 §1.2.

### A99. Each part of the server connects as its own login

- **Date:** 2026-11-12
- **Source:** Direct: James, design discussion of 2026-11-12
- **Change:** extends §4
- **Summary:** §4's privacy rests on the role a connection holds, but every part of the server connected as the database owner, which holds every grant and bypasses row-level security (A98), so nothing kept the public API out of `private`. James chose, between one login that switches role per pool (`SET ROLE`, which a compromised process could undo) and a login per part: **each part connects with its own URL, naming a login the deployment has granted the part's role**. The server takes `TRIPLESPACE_ACCOUNTS_DATABASE_URL` and `TRIPLESPACE_NOTIFY_DATABASE_URL` beside `TRIPLESPACE_DATABASE_URL`, which the public API connects with.

### A100. Which part is which, and what the roles were missing

- **Date:** 2026-11-12
- **Source:** Direct: Claude, while building the logins of A99, 2026-11-12; confirmed by James, 2026-11-12, as A101 changes it
- **Change:** extends §4, §6, §9
- **Summary:** Building A99 by running every server test with each part connected under its role found what belongs where and what the owner had hidden. The calls made: (1) **The accounts service** is a request's caller (the session or API key, read on an accounts connection for every request), the Action API's sign-in, sign-up, account-link, preference, rename, watch and inbox actions (which run on its connection throughout, so their records and `private` rows are one transaction), the REST account, auth, email, account-request, settings and watchlist routes, external logins and their OAuth clients, challenges and rate limits, forwarder keys, and the job runners, since an adoption appends to accounts partitions; a public action that needs it for one step takes a second connection. **The notifier** is addressing and email delivery. (2) A part without a URL shares the public API's, so a single owner URL still works, and the server warns at start when the public API's login can read `private`, rather than refusing. (3) `0035_service_grants` gives `ts_accounts` the sequences in `private` (`private.login_id`) and `ts_notify` `SELECT` on `private.preference`, which addressing reads; `grants::GRANTS` repeats both. (4) **§9:** a bulk append copies into the partition's own table, not `log.record`: Postgres refuses `COPY FROM` into a table under row-level security for a role it applies to (A98), and every bulk append is one partition's. (5) **§6:** the `actor_surrogate` sequence made on first use for tenants older than 0007 A27 is looked up before it is created, since creating needs a right in `log` the roles lack even when `IF NOT EXISTS` would make the statement a no-op; such a tenant's first surrogate is minted by the owner (any `triplespace` command that writes) or fails. (6) The tests connect each part under its role by the URL's `options=-c role=…`, which sets it as the connection opens, standing in for separate logins.

### A101. Callers, settings, jobs and forwarder keys leave the accounts service

- **Date:** 2026-11-12
- **Source:** Direct: James, design discussion of 2026-11-12
- **Change:** amends §4
- **Summary:** Reviewing which parts A100 gave the accounts service, James confirmed that the server warns, rather than refusing to start, when the public API connects as a login that can read `private`, and decided four changes, after which A100 stands confirmed: (1) **a request's caller** is found through narrow functions that read `private` as their owner and return the account and nothing else, `ops.caller_session` and `ops.caller_key` (`0036_caller_lookup`), so that the public API resolves its callers on its own connection and still may not read `private.session` or `private.api_key`; (2) **a settings save** is the public API's again, its login checks no longer holding the whole save on the accounts login; (3) **the job runners** no longer give jobs that write only public partitions the accounts login; (4) **forwarder keys** move from `private` to `ops` (`0037_forwarder_key_ops`): they belong to no account, and the public API checks one on every forwarded request. A102 records how (2) and (3) were built.

Replaced text: 03 §1.2.

### A102. How settings checks and the job runners left the accounts service

- **Date:** 2026-11-12
- **Source:** Direct: Claude, while building A101, 2026-11-12; confirmed by James, 2026-11-12
- **Change:** extends §4
- **Summary:** Two of A101's changes were built otherwise than first proposed. (1) **Settings checks:** narrow yes-or-no functions, as for callers, do not fit two of the three: `ts-external-required-unmet` and `ts-strict-names-unmet` name the logins that block them, and the strict-names check derives names by the issuer registry's rules in Rust. Instead, a save that changes how people log in takes one connection of the accounts service, through which its checks read the logins and OAuth clients, while the settings they weigh are read in the save's own unit of work, which sees what the save wrote; the readiness reads take the two connections separately. (2) **Job runners:** rather than two kinds of runner, every runner connects as the public API, since no kind queued today writes an accounts partition (import writes actor records to `actors`, revert public records); adoption, which does, is run by the command line, and gets a runner of its own on the accounts login when it joins the queue (SCA-309).

### A103. The followers' runner is the one on the accounts login

- **Date:** 2026-11-13
- **Source:** Direct: Claude, while building the queue's adoption, sync and follower jobs (SCA-309), 2026-11-13; unconfirmed
- **Change:** corrects §4
- **Summary:** A102 (2) gave adoption a runner of its own on the accounts login when it joined the queue, "since an adoption appends to accounts partitions". It does not: an adoption writes the source's accounts as actor records in the tenant's public `actors` partition, and no binding (07 §2.5), so the public API's login runs it, as it runs imports. The job that needs the accounts login is the **follower**, which reads the bot password it logs in upstream with from `private.upstream_login` (0092 §10). So: every runner but one connects as the public API and runs imports, reverts, dumps, exports, adoptions and syncs; one runner connects as the accounts service and claims `follow` jobs alone (0104 A9). Where the server is given no accounts URL, it uses the one it has.

Replaced text: 03 §1.2.

### A104. The large tables vacuum after a hundredth of their rows change

- **Date:** 2026-11-15
- **Source:** Direct: James, 2026-11-15, on the vacuum settings proposed after the whole-Wikidata benchmark; the values are Claude's, while building it, unconfirmed
- **Change:** extends §2
- **Summary:** Postgres's autovacuum waits until a fifth of a table's rows are dead (a tenth for analyze), which on the Wikidata mirror is 24M rows of its record partition and 54M of `view.identifier`: compaction removed 710,000 superseded records in the day-close benchmark and the database still grew, because the space stays unusable until a vacuum. Every record partition, `log.merkle_node`, and the views that grow with a mirror (`view.identifier`, `view.term`, `view.entity`, `view.entity_source`, `view.activity`) are vacuumed after 1% of their rows are dead or newly inserted and analyzed after 0.5%, with a per-table cost limit of 2000, ten times the default, so that a vacuum of a table of hundreds of gigabytes finishes. `create_partition` gives each new partition these settings, and the migrations `scatter-log-postgres/0003_record_autovacuum` and `triplespace-db/0040_view_autovacuum` give them to existing tables. Storage parameters only: no table, index or projection changes. Chapter text: 03 §2.

### A105. The sprint and task tables

- **Date:** 2026-11-16
- **Source:** 0061 §6
- **Change:** extends §5.6, §7
- **Summary:** The `view` schema gains `sprint` and `task`, with their initial computation as an `ops` job, and sprints and tasks are among the consumers that run after composition. This records an extension made when 0061 was written, on 2026-10-04: its heading has said so since then and the chapter text has carried it, but this log had no entry. No text changes with this entry. Chapter text: 03 §5, §6.1.

### A106. The tables of synchronized talk pages

- **Date:** 2026-11-16
- **Source:** 0069 §10
- **Change:** extends §5.6
- **Summary:** The `view` schema gains `foreign_thread` at instance scope and `pinned` on `thread_attachment`, and `ops` gains `upstream_post`, the send queue. This records an extension made when 0069 was written, on 2026-10-06: its heading has said so since then and the chapter text has carried it, but this log had no entry. No text changes with this entry. Chapter text: 03 §5.
