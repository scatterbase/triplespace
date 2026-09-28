# 0013. Postgres as the log store and serving model

- **Status:** Proposed
- **Date:** 2026-09-26
- **Author:** James Hare / Claude Fable
- **Revised:** 2026-09-27, to apply the schema changes of [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md) and [0018](0018-tenants.md) to the SQL in §2 and §5, and to gather the tables that [0015](0015-record-format-and-partition-registry.md) through [0021](0021-notifications.md) add in §5.6 and §4; and again later that day, to carry the columns, readers, queues and projection steps of [0022](0022-federation.md) through [0031](0031-property-constraints.md) into §4, §5 and §7, and to make every `partition` column `bigint`. §12 lists what changed. Like the crate table of [0005](0005-crate-organization.md) §2, the schema here is the one migrations are written from, so it is kept current in this ADR rather than spread across the amending ones.
- **Amended by:** [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (§2 and §7 amend §2, §3 and §6; §4 adds `view.upstream_revision`), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§3 and §9 add group, membership, ACL and block tables and IP blocks in `private`), [0017 — Entity ID grammar](0017-entity-id-grammar.md) (§4 amends `view.entity.id`), [0018 — Tenants](0018-tenants.md) (§2 and §6 amend §2 and §5), [0019 — Discussions](0019-discussions.md) (§11 adds tables), [0020 — Change feeds](0020-change-feeds.md) (§3 adds `private.watch`), [0021 — Notifications](0021-notifications.md) (§3 and §8 add inbox and contact tables to `private` and a delivery queue to `ops`; §10 makes `triplespace-notify` a second reader of `private`), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§10 amends §5: moderation ACLs in `view.acl`, `deleted` on `view.entity`, `patrolled` on `view.activity`, reserved page IDs), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§10 extends §4 and §5: `private.api_key`, `operator` on `view.actor`), [0026 — Sitelinks are URLs](0026-sitelinks.md) (§6 amends `view.sitelink` in §5.2), [0027 — Preferences, private state and portability](0027-preferences-and-portability.md) (§8 extends §4 and §5.6: `private.preference`, portability classes; `notification_pref` dropped), [0028 — Tenancy policy](0028-tenancy-policy.md) (§12: farm partitions, the name registry index, evaluation joins over `account_link`), [0029 — Resolver namespaces](0029-resolver-namespaces.md) (§3 reads `view.identifier` and `view.sitelink`; no table added), [0030 — Edit filters](0030-edit-filters.md) (§11 adds `view.filter` and `view.filter_hit`, and evaluation as step 0 of the write path), [0031 — Property constraints](0031-property-constraints.md) (§5 adds `view.constraint_violation`, `constraint_count`, `value_key`; the constraint projection in step 4), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§10 extends §5.6: `entity_source.provider_revid` and `verified_at_size`, per-actor `ap_key` and `ap_follower`, `federated` actors, `federation-policy`; §1 extends §8 with the local-graph source dump; §13 adds a third reader of `private` to §4), [0025 — The instance as an OAuth server](0025-oauth-server.md) (§8 extends §4 and §5.6: `private.oauth_token` and `private.oauth_consumer`; `actor.status` gains `pending`), [0032 — The SPARQL Update stream](0032-sparql-update-stream.md) (§3 extends §5.6 with `view.rdf_delta` and §7 with the delta projection; §1 and §9 amend §8: dumps are stamped with a cursor and offered in a skolemized form, and the local quad store is fed from the delta table rather than by a separate quad projection)
- **Related:** [0000 — Initial proposition](0000-init.md) (settles the storage-engine question), [0001 — Revision metadata in RDF](0001-revision-metadata-rdf.md) (amends §2; §8 says which dumps carry the metadata graph), [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (§9 refines §8.6; §5.2 answers the match-key open question), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (amends §2 and §4.4), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (amends §5 and §7), [0007 — Actor identity](0007-actor-identity.md), [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md) (§6 settles the global-revision-ID and entity page-ID open questions), [0009 — Keyed entity types and Domain](0009-keyed-entity-types-and-domain.md), [0010 — Site UI](0010-site-ui.md) (amends §13; §6 settles the global-revision-ID open question), [0011 — Upstream and local logs](0011-logs.md) (§6 settles the log-ID open question; §8 refines the export rule of §2), [0012 — API requirements for the site UI](0012-api-requirements.md) (amends §2.1), [0014 — Cache layers and search](0014-caches-and-search.md)

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

The log records of [0006](0006-log-integrity-and-erasure.md) are rows in Postgres. The alternative, segment files on disk with Postgres holding only `(partition, offset)` pointers, is MediaWiki's external-storage split. It was rejected because every interactive write would then span two systems:

- appending the record;
- allocating its global revision ID ([0012](0012-api-requirements.md) §2.1);
- checking the base offset ([0006](0006-log-integrity-and-erasure.md) §8);
- updating the projections a reader expects to see at once (§7).

In one database, these are one transaction. Erasure becomes an `UPDATE`, compaction a `DELETE`, and backup, replication and point-in-time recovery are the database's.

**The segment file format stays.** `scatter-log` keeps a file format for two purposes:

- **export bundles** ([0006](0006-log-integrity-and-erasure.md) §9), which `verify` reads without a database;
- **Scatterbase**, which may store its claim log in files.

`scatter-log` therefore defines a `LogStore` trait with two backends (§10). The record and header formats, the hash tags, the Merkle tree and the checkpoint format of 0006 are unchanged. What changes is where the bytes live.

**Segments become offset ranges (amends 0006 §5).** A segment is the range `[n·2^k, (n+1)·2^k)` of a partition. It is sealed when every offset in the range has been appended and its manifest written. Nothing physical corresponds to it in Postgres. The file backend writes one file per segment, as before.

### 2. The `log` schema

```sql
CREATE TABLE log.partition (
  partition     bigint PRIMARY KEY,            -- 64 random bits, fixed at creation (0018 §2); 0 is the instance config
  tenant        text,                          -- tenant slug, or NULL for an instance partition (0018 §2)
  name          text NOT NULL,                 -- "local", "mirror/wikidata", "pages", …
  graph_iri     text NOT NULL,                 -- {base}/graph/{name} (0015 §5)
  history       text NOT NULL,                 -- full | latest            (0002 §2)
  integrity     text NOT NULL,                 -- logged | hashed          (0006 §4)
  export        text NOT NULL,                 -- public | internal | private (0005 §4.1, 0007 §8)
  segment_k     smallint NOT NULL,             -- segments hold 2^k records (0006 §5)
  hash          text NOT NULL,                 -- named in the genesis record (0006 §2, 0015 §3)
  next_offset   bigint NOT NULL DEFAULT 0,
  UNIQUE (tenant, name)                        -- the registry key of 0018 §2
);

CREATE TABLE log.record (
  partition     bigint   NOT NULL REFERENCES log.partition,
  "offset"      bigint   NOT NULL,
  appended_at   bigint   NOT NULL,             -- µs since the epoch, UTC   (header 3)
  payload_type  text     NOT NULL,             --                          (header 4)
  key           text,                          -- identifier, never content (header 5)
  commitment    bytea    NOT NULL,             -- root over the part leaves, 32 bytes (header 6, 0015 §1)
  revid         bigint,                        -- global or provider-ranged revision ID (header 7, 0015 §2)
  logid         bigint,                        -- global log ID (header 8, 0015 §2)
  page_id       bigint,                        -- page ID of the key, carried forward (header 9, 0015 §2)
  header        bytea    NOT NULL,             -- the canonical CBOR header, the Merkle preimage
  leaf          bytea    NOT NULL,             -- H(0x00 ‖ header)
  body          bytea    NOT NULL,             -- canonical CBOR; an erased part is [null, leaf] (0015 §1)
  erased        smallint NOT NULL DEFAULT 0,   -- bitmask of erased parts (0015 §7)
  erased_by     bigint,                        -- offset of the latest erase record touching this row
  PRIMARY KEY (partition, "offset")
) PARTITION BY LIST (partition);
```

- **One child table per log partition**, which since [0018](0018-tenants.md) §2 means one per tenant partition as well as one per shared partition. Compaction, vacuum, bootstrap loading and index choice are then per partition. A mirror partition under `latest` carries an index on `(key, "offset" DESC)` for compaction and the version cursor. The local partition carries the same index for base-offset checks. Whether small tenants should share physical tables is open in 0018.
- **The header columns are denormalized from `header`.** The CBOR bytes are the preimage that `leaf` and every proof are computed over. The columns exist so that the header can be queried without decoding. A test checks that re-encoding the columns reproduces the bytes. `revid`, `logid` and `page_id` are header fields 7–9 ([0015](0015-record-format-and-partition-registry.md) §2), so they are inside the Merkle tree; the sidecar this ADR first gave them is gone.
- **Bodies use TOAST with `lz4` compression.** Canonical CBOR compresses well, and `lz4` decompresses fast enough for the read path. Bodies are never queried inside the database; they are decoded by the server.
- **A body is never `NULL`.** Under [0015](0015-record-format-and-partition-registry.md) §1 erasure is per part, and an erased part is rewritten as `[null, leaf]` so the commitment still verifies. `erased` records which parts are gone; a fully erased record has every bit set. Which `erase` record removed which part is recoverable from the `erase` records themselves, which name their targets and parts; `erased_by` is a convenience for the gap row of [0010](0010-site-ui.md) §5.2 and holds the latest one.

**Offsets are allocated in the appending transaction.** The transaction locks the partition's row in `log.partition`, takes `next_offset`, inserts, and advances it. A rollback releases the offsets, so the sequence stays gapless as [0006](0006-log-integrity-and-erasure.md) §3 requires. Appends to one partition are serialized by that lock. This is the correct behaviour for the local partition, where edits must be ordered, and irrelevant for bulk ingest, which uses §9.

**Merkle nodes are persisted.**

```sql
CREATE TABLE log.merkle_node (
  partition bigint   NOT NULL,
  level     smallint NOT NULL,                 -- 0 is the leaf level
  index     bigint   NOT NULL,
  hash      bytea    NOT NULL,
  PRIMARY KEY (partition, level, index)
);
```

An inclusion proof for a record in a partition of 10^8 records needs 27 sibling hashes. Recomputing them from leaves would read millions of rows; reading them from this table reads 27. In a `hashed` partition, each segment's tree is stored the same way and stands alone.

**Checkpoints and manifests** are rows, not files. [0006](0006-log-integrity-and-erasure.md) §6 says checkpoints are "stored beside their partition"; a table is beside the partition.

```sql
CREATE TABLE log.checkpoint (
  partition   bigint   NOT NULL,
  tree_size   bigint   NOT NULL,
  root        bytea    NOT NULL,
  key_id      text     NOT NULL,
  signed_note text     NOT NULL,               -- the C2SP note, verbatim
  written_at  timestamptz NOT NULL,
  PRIMARY KEY (partition, tree_size)
);

CREATE TABLE log.segment_manifest (
  partition   bigint   NOT NULL,
  segment     bigint   NOT NULL,
  root        bytea    NOT NULL,
  replaces    bigint[] NOT NULL DEFAULT '{}',  -- manifests this one supersedes after compaction
  signed_note text     NOT NULL,
  sealed_at   timestamptz NOT NULL,
  PRIMARY KEY (partition, segment, sealed_at)
);
```

`GET /record/{partition}/{offset}/checkpoint` ([0012](0012-api-requirements.md) §5) is `SELECT min(tree_size) WHERE tree_size > offset`.

### 3. Erasure and compaction in Postgres (amends 0006 §7)

**Erasure.** In the same transaction that appends the `erase` record, each target's body is rewritten with the named parts replaced by `[null, leaf]` ([0015](0015-record-format-and-partition-registry.md) §1), and the row is marked:

```sql
UPDATE log.record SET body = $rewritten_body, erased = erased | $parts_mask, erased_by = $erase_offset
 WHERE partition = $p AND "offset" = $target;
```

one row at a time, since each body is rewritten from its own bytes; erasing by key selects the targets with the `(key, "offset")` index first. The header, the commitment and the leaf survive. Projections then replay the `erase` record and drop what they derived from the erased parts, as 0006 §7 and 0015 §1 require.

**Erased bytes linger until vacuumed.** MVCC keeps the old tuple until `VACUUM` reclaims it, and the WAL archive keeps it for the archive's retention. This is the "backups must be handled operationally" caveat of 0006 §7, made concrete:

- the erasure worker runs `VACUUM` on the affected child table after an `erase`, and the instance's runbook sets a WAL retention window that bounds how long erased bytes can survive in archives;
- an instance under a legal deadline runs `VACUUM FULL` or `pg_repack` on that child table, which rewrites it.

**Compaction** of a `latest` partition is a batched delete of every record for a key except the newest:

```sql
DELETE FROM log.record_mirror_wikidata r
 USING (SELECT key, max("offset") AS keep FROM log.record_mirror_wikidata
        WHERE key = ANY($keys) GROUP BY key) k
 WHERE r.key = k.key AND r."offset" < k.keep;
```

Offsets are never reused, so a compacted partition has holes. That is the model 0002 §2 borrowed from Kafka. The compaction worker re-signs the manifest of each segment it touched, listing the manifest it replaces. A tombstoned entity that is not retained loses every record for its key, including the tombstone once its retention policy has been applied.

### 4. Four schemas

| Schema | Holds | Lifetime | Role that may read it |
|---|---|---|---|
| `log` | §2: partitions, records, Merkle nodes, checkpoints, manifests | Permanent. The source of truth. | The server, the ingester, `verify` |
| `view` | §5: every projection the API and UI read | Rebuildable from `log` at any time | The server |
| `private` | State that belongs to one account and has no public history: the accounts graph's binding index, password hashes for the built-in `password` issuer ([0007](0007-actor-identity.md) §3, as amended), sessions' secrets, IP addresses held for abuse handling and IP blocks, when each binding was last used ([0007](0007-actor-identity.md) §3, [0012](0012-api-requirements.md) §5, [0016](0016-permissions-and-access-control.md) §3); the watch set and its Atom token ([0020](0020-change-feeds.md) §3); inboxes and read state ([0021](0021-notifications.md) §3); email addresses, fediverse handles, and the ActivityPub keys and followers of the notifier and of every opted-in actor ([0021](0021-notifications.md) §5, §8, [0022](0022-federation.md) §6); subsidiaries' API keys, OAuth tokens and confidential consumers' secrets ([0024](0024-subsidiary-accounts.md) §4, [0025](0025-oauth-server.md) §8); every preference, including the notification matrix and the auto-watch flags ([0027](0027-preferences-and-portability.md) §1); the IP row behind each edit-filter hit ([0030](0030-edit-filters.md) §11). §5.6 lists the tables | Permanent; never exported. The portable part travels with an account as a user data bundle, and with a cooperative tenant move as a sealed private extract applied on reclaim ([0027](0027-preferences-and-portability.md) §2–4); nothing else leaves ([0018](0018-tenants.md) §10) | `triplespace-accounts`, `triplespace-notify` and, for `ap_key` and `ap_follower` only, `triplespace-federation` |
| `ops` | Projection positions (§7), job runtime state, the bootstrap coordinator's block table (§9), the email and fediverse delivery queue ([0021](0021-notifications.md) §8, [0022](0022-federation.md) §7), and the asynchronous jobs that are not log jobs: large user-data exports ([0027](0027-preferences-and-portability.md) §3), filter tests ([0030](0030-edit-filters.md) §6) and property-wide constraint re-checks ([0031](0031-property-constraints.md) §2) | Operational | The server, the ingester, the notifier |

**Privacy is enforced by grants.** The database role the public API connects with has no privilege on `private`. Only the accounts service ([0007](0007-actor-identity.md) §10), the notifier ([0021](0021-notifications.md) §10) and, since [0022](0022-federation.md) §13, the federation service, for actor keys and followers, connect with a role that does; all three are holder-only services, and none serves a route that returns another account's rows. The OAuth server ([0025](0025-oauth-server.md) §8) reaches `private` only through the accounts service. The privacy test of [0012](0012-api-requirements.md) §8, which checks that no route returns private data, is backed by a check that no query outside those three crates can. Where a private set has to meet public data, as the watch set meets `view.activity`, the private side resolves to a list of identifiers and hands it to a query under the public role ([0020](0020-change-feeds.md) §3); there is no cross-schema join. `pg_dump` for exports and verification bundles excludes `private` and `ops`.

**The accounts partition is still in `log`.** Bindings are log records ([0007](0007-actor-identity.md) §8), and they keep their `logged` integrity. Their child table is in the `log` schema but owned by the accounts role, and the public role's `SELECT` on `log.record` is granted per child table, excluding it.

### 5. The `view` schema

The rule for the serving model is: **the blob serves the page; tables serve queries.** A table exists only where a route needs a lookup or an order that the entity's own JSON cannot give. Nothing is normalized for its own sake.

**Tenants.** Under [0018](0018-tenants.md) §6, `view.entity` and the tables that hang off it (`term`, `sitelink`, `identifier`, `entity_ref`, `statement_assertion`, `correction`, `constraint_violation`, `value_key`) gain a `tenant text NOT NULL DEFAULT ''` column that leads every primary key: the **empty string is the instance**, and the row with `tenant = ''` is the shared row computed from the shared source graphs; a tenant that has local assertions, a cluster link or a correction touching the entity has its own `(tenant, id)` row; a tenant with none reads the shared one. A read is therefore one primary-key lookup with the viewer's tenant, falling back to one with `''`. Every other table in this section is per tenant outright and carries `tenant` in its key. The sentinel is chosen over a `NULL` tenant (decided 2026-09-27) because a primary key cannot hold `NULL` and a plain unique constraint would not reject a duplicate shared row; no slug can be empty ([0018](0018-tenants.md) §1), so nothing collides. Where an earlier ADR says "a `NULL` tenant", read `''`. The SQL below is written for one tenant; the column is implied throughout. `partition` columns are `bigint`, as in §2.

#### 5.1 Entities

```sql
CREATE TABLE view.entity (
  id                text PRIMARY KEY,          -- local, foreign or keyed ID in the form of 0017 §1
  type              text NOT NULL,             -- item, property, domain, …
  page_id           bigint NOT NULL UNIQUE,    -- §6
  surrogate         bigint,                    -- keyed types only (0009 §7)
  cluster_id        bigint,                    -- NULL when the entity is in no cluster
  canonical_id      text NOT NULL,             -- itself unless a non-canonical member (0004 §4)
  retention         text,                      -- cascade | orphan | retain (0002 §5)
  first_seen        timestamptz NOT NULL,
  local_offset      bigint,                    -- latest local-graph record for this key
  local_revid       bigint,                    -- lastrevid for the Action API
  resolved_version  bigint NOT NULL,           -- monotonic; bumped on every re-resolution (0014 §3)
  generation        integer NOT NULL DEFAULT 0,-- bumped on erasure and hiding (0014 §5)
  resolved_kind     text NOT NULL,             -- source | materialized
  resolved          bytea,                     -- canonical JSON, compressed; NULL when source
  resolved_size     integer NOT NULL,
  deleted           boolean NOT NULL DEFAULT false   -- derived from a read ACL on the entity (0023 §4, §10)
);

CREATE TABLE view.entity_source (               -- the version cursor of 0002 §8.4, per graph
  entity_id         text NOT NULL,
  graph             text NOT NULL,             -- local, mirror/wikidata, …
  "offset"          bigint NOT NULL,           -- the graph's current record for the entity
  upstream_version  text,                      -- lastrevid, updated_date, …
  prev_upstream     text,                      -- 0012 §2.2
  content_hash      bytea NOT NULL,            -- H(0x03 ‖ payload)
  size              integer NOT NULL,          -- 0012 §2.4
  synced_at         timestamptz NOT NULL,
  job_id            bigint,
  provider_revid    text,                      -- 0022 §2: the provider's own revision ID, for a verified Triplespace provider
  verified_at_size  bigint,                    -- 0022 §2: the provider checkpoint tree size this record was verified against
  PRIMARY KEY (entity_id, graph)
);
```

**The resolved view is materialized only where it differs from its source.** Reconciliation across graphs ([0002](0002-source-graphs-and-mass-ingest.md) §3), statement fusion ([0004](0004-identity-clusters-and-equivalence.md) §8) and canonical-ID rewriting ([0004](0004-identity-clusters-and-equivalence.md) §4) are too costly to do per request, so the projection computes the resolved canonical JSON whenever any contributing record changes. But for most entities in a full mirror, exactly one graph contributes and no value refers to a clustered entity. For those, the resolved JSON *is* the mirror record's payload, which the adapter already wrote in prefixed-ID form ([0002](0002-source-graphs-and-mass-ingest.md) §8.4). Storing it twice would double the largest table in the database. So:

- The projection computes the resolved JSON and compares its content hash with the single source's.
- If they match, `resolved_kind = 'source'` and `resolved` is `NULL`. A reader takes the body at `entity_source.offset`.
- Otherwise `resolved_kind = 'materialized'` and `resolved` holds the JSON.

Either way, a reader gets the entity with one or two primary-key lookups, and `resolved_version` is what the cache and `ETag` key on ([0014](0014-caches-and-search.md) §3).

**Only canonical members have a full row.** A non-canonical cluster member keeps its row for resolution (`canonical_id` points at the canonical entity), its `entity_source` rows and its terms in the per-graph views, but its resolved JSON is the canonical entity's.

#### 5.2 Terms, sitelinks, identifiers and references

```sql
CREATE TABLE view.term (                        -- the resolved fingerprint (wikibase-compat §2)
  entity_id text NOT NULL, kind smallint NOT NULL,   -- 1 label, 2 description, 3 alias
  lang text NOT NULL, ordinal smallint NOT NULL DEFAULT 0,
  text text NOT NULL,
  PRIMARY KEY (entity_id, kind, lang, ordinal)
);
-- Optional, small profile only (0014 §8): prefix search without OpenSearch.
CREATE INDEX term_prefix ON view.term (lang, kind, lower(text) text_pattern_ops);

CREATE TABLE view.sitelink (                    -- sitelinks are URLs; the host is the site (0026 §1–2, §6)
  url_key text PRIMARY KEY,                     -- the normalized URL; one item per URL
  entity_id text NOT NULL, host text NOT NULL,  -- host is a Domain key
  site_id text, title text,                     -- alias site ID (enwiki) where one applies; display title
  badges text[] NOT NULL DEFAULT '{}',
  UNIQUE (entity_id, host)                      -- one link per host per item
);
CREATE INDEX sitelink_host ON view.sitelink (host);
CREATE INDEX sitelink_entity ON view.sitelink (entity_id);

CREATE TABLE view.identifier (                  -- normalized external-id values (0004 §7)
  property text NOT NULL, value_key text NOT NULL,
  entity_id text NOT NULL, graph text NOT NULL, statement_id text NOT NULL,
  PRIMARY KEY (property, value_key, entity_id, graph)
);

CREATE TABLE view.match_key (                   -- uniqueness for designated properties (0002 §8.5)
  property text NOT NULL, value_key text NOT NULL, entity_id text NOT NULL,
  PRIMARY KEY (property, value_key)
);

CREATE TABLE view.entity_ref (                  -- the reverse index (0004 Consequences, 0008 §10)
  target_id text NOT NULL, source_id text NOT NULL,
  statement_id text NOT NULL, role smallint NOT NULL,     -- 1 main, 2 qualifier, 3 reference
  PRIMARY KEY (target_id, source_id, statement_id, role)
);
```

- **`term` serves label lookups**, which every entity page does in bulk for the entities it links to. It is the equivalent of Wikibase's term store. Prefix search on it is a fallback; the search index ([0014](0014-caches-and-search.md) §7) is the primary path.
- **`identifier` holds every normalized external-id value in the resolved view**, keyed so that "which entity has DOI *x*" is one lookup. It serves match keys, tier-3 inferred links and identifier resolution in the title resolver. `match_key` is the subset under a uniqueness constraint; which properties populate it is registry configuration, and an existing violation is reported when the constraint is turned on, not silently dropped.
- **`entity_ref` is the largest table after `record`** at Wikidata scale, on the order of 10^9 rows. It is required: a change of cluster membership must find every referrer, and "Links here" must list them. Its `target_id` is the ID as asserted; the projection resolves through `canonical_id` when it reads.

#### 5.3 Statements, clusters and corrections

There is no row per statement. A statement's provenance ([0003](0003-statement-ui.md) §6) is derivable for the common case: absent a row, the statement is asserted only by the mirror graph of the entity's provider. Rows exist for the exceptions:

```sql
CREATE TABLE view.statement_assertion (         -- local statements, and statements asserted by 2+ graphs
  statement_id text NOT NULL, graph text NOT NULL, entity_id text NOT NULL, property text NOT NULL,
  member_statement_id text NOT NULL,            -- the member's own ID when fused (0004 §8)
  rank smallint, "offset" bigint NOT NULL,
  PRIMARY KEY (statement_id, graph, member_statement_id)
);

CREATE TABLE view.correction (                  -- local overrides on mirrored assertions (0002 §7)
  statement_id text NOT NULL, entity_id text NOT NULL, kind text NOT NULL,   -- rank | suppress | term
  "offset" bigint NOT NULL, upstream_graph text NOT NULL,
  state text NOT NULL,                          -- active | redundant | dangling (0003 §4)
  PRIMARY KEY (statement_id, kind)
);

CREATE TABLE view.record_statement (            -- "History of this value" (0010 §5.7); local records only
  statement_id text NOT NULL, partition bigint NOT NULL, "offset" bigint NOT NULL,
  PRIMARY KEY (statement_id, partition, "offset")
);

CREATE TABLE view.cluster (cluster_id bigserial PRIMARY KEY, canonical_id text NOT NULL);
CREATE TABLE view.cluster_member (
  cluster_id bigint NOT NULL, namespace text NOT NULL, entity_id text NOT NULL UNIQUE,
  PRIMARY KEY (cluster_id, namespace)           -- one member per namespace (0004 §2)
);
CREATE TABLE view.link (                        -- every link, applied or held (0004 §3, §10)
  a text NOT NULL, b text NOT NULL, tier smallint NOT NULL,
  source text NOT NULL,                         -- statement ID, or the local record's offset
  status text NOT NULL,                         -- applied | held
  conflict_id bigint,
  PRIMARY KEY (a, b, tier, source)
);
CREATE TABLE view.different_from (a text NOT NULL, b text NOT NULL, "offset" bigint NOT NULL, PRIMARY KEY (a, b));
CREATE TABLE view.property_link (a text NOT NULL, b text NOT NULL, "offset" bigint NOT NULL, PRIMARY KEY (a, b));
```

`cluster_member`'s primary key is the one-to-one rule of [0004](0004-identity-clusters-and-equivalence.md) §2 as a constraint. A link that would violate it is held, with `status = 'held'`, and the conflict list ([0004](0004-identity-clusters-and-equivalence.md) §10) is a query on `link`.

#### 5.4 Keyed types, pages, actors

```sql
CREATE TABLE view.keyed_surrogate (             -- 0009 §7; key is NULLed on erasure
  keyed_type text NOT NULL, surrogate bigint NOT NULL, key text,
  PRIMARY KEY (keyed_type, surrogate), UNIQUE (keyed_type, key)
);
CREATE TABLE view.keyed_map (                   -- 0009 §9: upstream item ↔ key
  provider text NOT NULL, upstream_id text NOT NULL, keyed_type text NOT NULL, key text NOT NULL,
  PRIMARY KEY (provider, upstream_id), UNIQUE (keyed_type, key, provider)
);

CREATE TABLE view.page (                        -- 0008 §4; rows for threads and talk pages too (0019 §11)
  page_id bigint PRIMARY KEY, ns integer NOT NULL, title text NOT NULL,
  content_model text NOT NULL, owner_actor text,
  latest_offset bigint, latest_revid bigint,    -- NULL while the page ID is only reserved (0023 §2)
  reserved boolean NOT NULL DEFAULT false,      -- create-protection holds the ID (0023 §2, §10)
  deleted boolean NOT NULL DEFAULT false,       -- derived from a read ACL on the page (0023 §4)
  generation integer NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX page_title ON view.page (ns, title) WHERE NOT deleted;   -- the title index; reserved rows included
CREATE TABLE view.page_link (                   -- 0008 §10, both directions served by two indexes
  from_page bigint NOT NULL, target_kind smallint NOT NULL, target_id text NOT NULL,
  PRIMARY KEY (from_page, target_kind, target_id)
);
CREATE INDEX page_link_target ON view.page_link (target_kind, target_id);

CREATE TABLE view.actor (                       -- 0007 §4; name and raw are NULLed on erasure
  actor_key text PRIMARY KEY, issuer text NOT NULL, subject text NOT NULL,
  kind text NOT NULL,                           -- registered | temporary | bot | anonymous | imported | provider | federated (0022 §8)
  name text,
  status text NOT NULL,                         -- active | renamed | vanished | hidden | retired (0024 §2) | pending (0025 §3)
  raw text,                                     -- a surrogate's raw value: IP, imported name, remote actor IRI
  operator text,                                -- the operator's actor key for a subsidiary (0024 §1); NULL otherwise
  groups text[] NOT NULL DEFAULT '{}',          -- 0016 §9
  iri text NOT NULL
);
CREATE UNIQUE INDEX actor_local_name ON view.actor (lower(name)) WHERE issuer = 'local' AND name IS NOT NULL;
-- Name tombstones (0010 §10, as amended 2026-09-27) are not here: a vanished account's names are erased
-- from the log, so they live as keyed hashes in private.name_tombstone (§5.6), which survives a rebuild.
CREATE TABLE view.account_link (                -- public links only (0007 §7); a row is deleted on unlink
  local_actor text NOT NULL, foreign_actor text NOT NULL UNIQUE, "offset" bigint NOT NULL,
  PRIMARY KEY (local_actor, foreign_actor)
);
```

Entity pages get their `page_id` from the same sequence as document pages (§6), so `view.entity.page_id` and `view.page.page_id` never collide and one `pageid` space covers both kinds of page.

#### 5.5 Activity, jobs and configuration

```sql
CREATE TABLE view.activity (                    -- 0010 §13, 0012 §3: one row per local record, job or event
  time timestamptz NOT NULL, partition bigint NOT NULL, "offset" bigint NOT NULL,
  kind text NOT NULL,                           -- edit | sync | log | job | erased
  source text NOT NULL,
  revid bigint, logid bigint,
  target_kind smallint, target_id text, target_member text,
  actor_key text, operator text, requested_by text, job_id bigint,
  summary_op text, summary_args text, comment text,
  tags text[] NOT NULL DEFAULT '{}',            -- from the attestation part (0030 §5)
  minor boolean, bot boolean, new boolean,
  size integer, delta integer,
  visibility smallint NOT NULL DEFAULT 0,       -- record ACLs and the erased bitmask (0023 §5)
  patrolled boolean NOT NULL DEFAULT false,     -- the patrol projection and the autopatrol rule (0023 §6)
  log_type text, log_action text, params jsonb,
  changes jsonb,                                -- sync summaries (0012 §2.2)
  PRIMARY KEY (partition, "offset")
);
CREATE INDEX activity_time   ON view.activity (time DESC, partition, "offset");   -- recent changes, 0012 §2.3
CREATE INDEX activity_actor  ON view.activity (actor_key, time DESC);             -- contributions
CREATE INDEX activity_target ON view.activity (target_kind, target_id, time DESC);-- history
CREATE INDEX activity_job    ON view.activity (job_id) WHERE job_id IS NOT NULL;
CREATE INDEX activity_unpatrolled ON view.activity (time DESC) WHERE NOT patrolled;  -- the review queue (0023 §10)
CREATE UNIQUE INDEX activity_revid ON view.activity (revid) WHERE revid IS NOT NULL;
CREATE UNIQUE INDEX activity_logid ON view.activity (logid) WHERE logid IS NOT NULL;

CREATE TABLE view.job (                         -- projected from the local log (0011 §6.3)
  job_id bigint PRIMARY KEY, actor_key text NOT NULL, requested_by text,
  source text NOT NULL, source_version text, adapter_version text, mode text, graph text NOT NULL,
  params jsonb NOT NULL, status text NOT NULL, started timestamptz NOT NULL, finished timestamptz,
  counts jsonb, checkpoint_size bigint, sweep_count integer, sweep_threshold integer,
  start_offset bigint NOT NULL, finish_offset bigint
);
CREATE TABLE view.job_reject (job_id bigint NOT NULL, line bigint NOT NULL, match_key text, reason text NOT NULL, PRIMARY KEY (job_id, line));

CREATE TABLE view.registry (                    -- current configuration (0004 §9): providers, issuers,
  tenant text NOT NULL DEFAULT '',              -- '' for instance-level kinds (0018 §3); the sentinel rule above
  kind text NOT NULL, code text NOT NULL,       -- namespaces, keyed types, roles, graph registry
  config jsonb NOT NULL, "offset" bigint NOT NULL,
  PRIMARY KEY (tenant, kind, code)
);
```

`view.activity` is MediaWiki's `recentchanges`, `revision` and `logging` tables folded into one, because [0012](0012-api-requirements.md) §3 already folds their rows into one shape. It grows with local activity and the number of jobs, never with the mirrors: a sync of a million entities is one `job` row and one activity row, and per-entity mirror history is served from `entity_source` and the record itself.

`job_reject` holds the first N rejects for the job page ([0010](0010-site-ui.md) §9); the full rejects file lives on disk or in object storage, and the download route streams it.

`view.registry`'s key is `(tenant, kind, code)` since [0018](0018-tenants.md) §3 split the config kinds by scope; instance-level kinds (`key`, `graph`, `provider`, `issuer`, `keyed-type`, `tenant`, `alias`, and since then `tenancy`, `template`, `consumer`, the instance `sitelink-policy` and `federation-policy` lists and global `group`s) have the empty-string tenant.

The `view.filter`, `view.filter_hit`, `view.constraint_violation`, `view.constraint_count` and `view.value_key` tables of [0030](0030-edit-filters.md) §11 and [0031](0031-property-constraints.md) §5 sit beside these and are indexed in §5.6.

#### 5.6 Tables added by later ADRs

The ADRs after this one add tables in the same style. Each is specified where it is listed; this table is the index, so that the schema has one map.

| Schema | Table | Serves | Specified in |
|---|---|---|---|
| `view` | `upstream_revision` | Backfilled and observed upstream revisions; the stored half of the upstream-edits fold | [0015](0015-record-format-and-partition-registry.md) §4 |
| `view` | `group`, `membership`, `acl`, `block`; `groups text[]` on `actor` | Permissions, ACLs by target and enclosure, blocks | [0016](0016-permissions-and-access-control.md) §9 |
| `view` | `thread`, `post`, `talk_page`; rows in `page` for threads and talk pages | Threads, posts and composite talk pages | [0019](0019-discussions.md) §11 |
| `private` | `watch`, `watch_token` | The watch set, its expiry and `seen`, the Atom token; the four auto-watch preferences | [0020](0020-change-feeds.md) §3 |
| `private` | `inbox`, `inbox_state` | Notifications, seen and read state (the reason × channel matrix was `notification_pref`, now preferences) | [0021](0021-notifications.md) §3, [0027](0027-preferences-and-portability.md) §1 |
| `private` | `preference` | Every registered preference key, including the notification matrix, auto-watch flags and shape pins; each `private` table carries a portability class | [0027](0027-preferences-and-portability.md) §1–2, §8 |
| `log`, `view` | Instance partitions `actors/{farm}`, `accounts/{farm}`, `log/{farm}`; farm accounts in `actor` with a `NULL` tenant; the farm-wide name-registry index; `tenancy`, `template`, `provider-readers` in `registry` | Farm identity, global groups and blocks, templates | [0028](0028-tenancy-policy.md) §12 |
| `view`, `private` | `filter`, `filter_hit`; a per-hit IP row in the abuse-handling store | Edit filters and their log | [0030](0030-edit-filters.md) §11 |
| `view` | `constraint_violation`, `constraint_count`, `value_key` | Property-constraint violations, derived from the resolved view | [0031](0031-property-constraints.md) §5 |
| `private` | `email`, `fediverse_handle`, `ap_key`, `ap_follower` | Contact details and their verification; the notifier's key and followers and, since [0022](0022-federation.md) §6, each opted-in actor's, keyed by actor | [0021](0021-notifications.md) §5, §8, [0022](0022-federation.md) §10 |
| `private` | IP blocks | Blocks on addresses before a temporary account exists | [0016](0016-permissions-and-access-control.md) §3 |
| `private` | `password` | `(actor_key, hash, updated)` for the built-in `password` issuer; a portable-class-"re-established" table under 0027 §2 | [0007](0007-actor-identity.md) §3, as amended 2026-09-27 |
| `private` | `name_tombstone` | `(name_hmac PRIMARY KEY, released)`: a keyed hash of every name a local account has released by rename or vanish, so that no name is ever reused; holds no actor key and no plaintext, survives erasure by design, and is "never leaves" class under 0027 §2 | [0010](0010-site-ui.md) §10, as amended 2026-09-27 |
| `view` | Moderation ACLs in `acl`; `entity.deleted`, `activity.patrolled`, `page.reserved`; `visibility`, `hidden` and `actor.status` derived from `record` and `actor` ACLs | Protection, deletion, hiding, suppression and patrolling | [0023](0023-moderation.md) §10 |
| `private` | `api_key` | Subsidiary accounts' keys: label, hash, grants, IP ranges, expiry, last used | [0024](0024-subsidiary-accounts.md) §4, §10 |
| `view` | `actor.operator`; status `retired` | The operator of a subsidiary account | [0024](0024-subsidiary-accounts.md) §1, §10 |
| `view` | `entity_source.provider_revid`, `entity_source.verified_at_size`; `actor` rows of kind `federated`; `federation-policy` in `registry` | Verified sync from another instance; fediverse surrogates and domain lists | [0022](0022-federation.md) §10 |
| `private` | `oauth_token`, `oauth_consumer` | OAuth tokens as credentials of a subsidiary; confidential consumers' secrets. Authorization and device codes are Valkey keys, not rows | [0025](0025-oauth-server.md) §8 |
| `view` | `consumer` and `consumer-policy` in `registry`; status `pending` on `actor` | Registered OAuth consumers; a subsidiary awaiting approval | [0025](0025-oauth-server.md) §2–3, §8 |
| `ops` | The delivery queue | Email and fediverse deliveries with attempts and backoff; ActivityPub fan-out to followers | [0021](0021-notifications.md) §8, [0022](0022-federation.md) §7 |
| `ops` | Export jobs, filter-test jobs, constraint re-check jobs | Asynchronous work that is not a log job: a large user data bundle, a draft filter run over a period, a property-wide constraint re-check | [0027](0027-preferences-and-portability.md) §3, [0030](0030-edit-filters.md) §6, [0031](0031-property-constraints.md) §2 |
| `view` | `sitelink`, rewritten | Sitelinks as normalized URLs keyed by host, with site aliases; only permitted links are rows | [0026](0026-sitelinks.md) §6 (the SQL in §5.2 is the current form) |
| `view` | `view-pin` in `registry` | Tenant-wide shape pins | [0027](0027-preferences-and-portability.md) §5 |
| `view` | `resolver` in `registry`; no table | Resolver namespaces read `identifier` and `sitelink` | [0029](0029-resolver-namespaces.md) §3 |
| `view` | `rdf_delta` | The deleted and inserted triples of every change to a public graph, in one sequence, kept for a retention window; the SPARQL Update stream and the local quad store read it | [0032](0032-sparql-update-stream.md) §3 |
| `ops` | The delta epoch | Incremented by a `view` rebuild, so that cursors from before it are refused | [0032](0032-sparql-update-stream.md) §3 |
| `ops` | `ap_inbox_seen` | Accepted inbound activity IDs for the replay window | [0022](0022-federation.md) §8, as settled 2026-09-27 |

Two rules follow from the table. Every `view` table is a projection under §7 and is rebuilt from the log. Nothing in `private` is: the watch set, inboxes and contact details survive on the strength of that schema's backups, and two things in it are not recoverable at all, `seen` on a watch and `read_at` on a notification ([0020](0020-change-feeds.md) §3, [0021](0021-notifications.md) §3). Each `private` table also carries a portability class in `triplespace-db`'s schema definition, from which the user data bundle and the private extract are generated ([0027](0027-preferences-and-portability.md) §2).

### 6. Global revision, log and page IDs (amends 0012 §2.1; amended by 0015 §2 and 0018 §2)

[0012](0012-api-requirements.md) §2.1 asks for one sequence for revisions and one for log events, assigned at append and stored in the record. Postgres sequences do this, one set per tenant ([0018](0018-tenants.md) §2), since MediaWiki clients expect one sequence per wiki:

- `log.revision_id` is taken for every record appended to a partition whose records are this tenant's revisions: `local` and `pages`. It is written to header field 7 and to `log.record.revid` in the appending transaction.
- `log.log_id` is taken for every record that projects as a log event ([0011](0011-logs.md) §6.1), which includes the records of the tenant's `actors` partition, and for every record in the tenant's log partition, written to header field 8 and `log.record.logid`.
- `log.page_id` is taken the first time a key is written in any partition, and every later record for that key repeats it in header field 9 ([0015](0015-record-format-and-partition-registry.md) §2). It is never derived from replay order. Talk pages take theirs from the same sequence through the thread record that first attaches to them ([0019](0019-discussions.md) §2).
- Mirror records take a **provider-ranged** revision ID, `provider_number << 40 | n` ([0015](0015-record-format-and-partition-registry.md) §2), computed by the writer with no allocation.

The IDs are inside the hashed header, so an inclusion proof covers them, an export bundle needs no sidecar, and an erased record keeps them. This closes the global-ID question that 0012 opened; the first revision of this ADR kept them outside the header, and 0015 §2 moved them in.

### 7. Projections, synchrony and read-your-writes

Every table in `view` belongs to a named projection. `ops.projection_state (projection, partition, applied_offset)` records how far each has replayed; lag is the distance to the partition's head, and it is what job pages report ([0010](0010-site-ui.md) §9). A rebuild truncates the projection's tables and replays from offset 0.

Projections run in dependency order:

1. `registry`, `keyed_surrogate`, `actor`, and `group`, `membership`, `acl`, `block` ([0016](0016-permissions-and-access-control.md) §9, [0023](0023-moderation.md) §10), and `filter` ([0030](0030-edit-filters.md) §11), since the write path reads all of these;
2. `entity_source`, `keyed_map` ([0009](0009-keyed-entity-types-and-domain.md) §9), `page`, `job`, `upstream_revision` ([0015](0015-record-format-and-partition-registry.md) §4);
3. `cluster` and `link`;
4. `entity` (resolution), `term`, `sitelink` with the denied-host filter ([0026](0026-sitelinks.md) §6), `identifier`, `value_key`, `entity_ref`, `statement_assertion`, `correction`, then `constraint_violation` and `constraint_count` ([0031](0031-property-constraints.md) §2), which read them;
5. `activity` with `patrolled` ([0023](0023-moderation.md) §6), `filter_hit` ([0030](0030-edit-filters.md) §11), `page_link`, `record_statement`, and `thread`, `post`, `talk_page` ([0019](0019-discussions.md) §11);
6. the addressing projection that fills inboxes ([0021](0021-notifications.md) §1), which reads `activity` and writes to `private`; it is the one projection whose target is not `view`, and it runs asynchronously under the notifier's role;
7. the RDF and search projections (§8, [0014](0014-caches-and-search.md) §7). The **delta projection** of [0032](0032-sparql-update-stream.md) §2 is not a separate step: it runs inside steps 2, 4 and 7, wherever a projection has both the old and the new state of an entity in hand, and writes `view.rdf_delta` in the same transaction.

**Edit filters run before the append** ([0030](0030-edit-filters.md) §1, §11), in the same transaction, after the permission and ACL checks and the rate-limit check ([0024](0024-subsidiary-accounts.md) §5); a refusal appends only the hit record. The write path in full is therefore: authenticate the credential and resolve the actor and its effective permissions, including a key's or token's grants ([0024](0024-subsidiary-accounts.md) §4, [0025](0025-oauth-server.md) §3); rate limit; ACLs on the target and its enclosures; edit filters; the base-offset check ([0006](0006-log-integrity-and-erasure.md) §8); the append with ID allocation; projections 1–5 for the affected keys; commit.

**Interactive writes update the synchronous set in the same transaction as the append.** An editor who saves and reloads must see their edit, as they do on MediaWiki. So a write through the edit API appends the record and, before commit, applies projections 1–5 for the affected keys. Bulk jobs apply projections in batches behind the append, and report lag.

> **Amended 2026-09-27: the synchronous budget.** A write's own rows are always synchronous: the written entity's or page's resolution, terms, identifiers, sitelinks, its own `entity_ref` rows, its own constraint checks ([0031](0031-property-constraints.md) §2), its activity row and its delta ([0032](0032-sparql-update-stream.md) §2). **Fan-out** to other entities, meaning referrers re-resolved by a cluster change ([0004](0004-identity-clusters-and-equivalence.md), Consequences), `type`, `inverse` and `symmetric` constraint re-checks of statements that point at the changed entity (0031 §2), and the deltas those produce, is applied inline until a budget is spent and then handed to the projection worker: `projections.sync_budget` (`site` configuration, default 1,000 rows) or `projections.sync_time` (default 250 ms), whichever comes first. The remainder is queued in `ops.projection_state` as work for the affected keys, applied in append order by the worker that serves bulk jobs, and reported as lag on the entity page's identity line ("N referrers updating") and in `siprop=triplespace`. Read-your-writes therefore holds for the thing edited and for small fan-outs; a link to a heavily cited author shows its effect on referrers within lag. The append lock of §2 is released at commit, before the queued remainder runs, so a large fan-out never blocks the next editor.

**Tenant overlays.** Under [0018](0018-tenants.md) §6 the shared rows are computed once from the shared partitions, and a tenant's rows only where its own partitions change the result. A record in a tenant partition therefore re-runs steps 3–5 for that tenant's overlay of the affected keys; a record in a shared partition re-runs them for the shared row and for every tenant that holds an overlay row for the key.

**Replicas.** Reads may go to streaming replicas. After a write, the session records the commit's WAL position; a read is routed to a replica only when `pg_last_wal_replay_lsn()` on that replica has passed it, and to the primary otherwise. This is MediaWiki's ChronologyProtector, with the LSN in place of the binlog position.

### 8. RDF becomes an output, not the read path (amends 0001 §2 and 0005 §4.4)

- **The API and UI read only `view`.** No request path queries a triplestore.
- **The RDF projection streams.** The resolved view (the main graph of [0001](0001-revision-metadata-rdf.md) §2, computed as [0002](0002-source-graphs-and-mass-ingest.md) §3 describes), the source graphs and the metadata graph are produced as N-Quads from `view` and `log` by `triplespace-rdf` (§10). No intermediate quad store is needed to produce them. Two dump products are offered:
  - **A Wikibase-compatible dump** holds the resolved view only. It is what QLever and any Wikibase or Wikidata tool load, and it never contains the metadata graph ([0001](0001-revision-metadata-rdf.md) §2). Source graphs may be added to it for consumers who want to compare what a provider asserts with what the instance asserts ([0002](0002-source-graphs-and-mass-ingest.md) §3).
  - **A full dump** holds the resolved view, every source graph whose export policy allows it ([0005](0005-crate-organization.md) §4.1), and the metadata graph. It is the RDF form of the instance's own history: for the instance's own SPARQL endpoint, for backups, and for another instance that wants the whole record.
  Neither is the log export bundle of [0006](0006-log-integrity-and-erasure.md) §9, which carries records, not RDF.
- **A local quad store is a deployment option.** An instance that wants its own SPARQL endpoint runs the quad projection into Oxigraph or QLever as one more consumer of the log. `scatter-quadstore` keeps its contract for Scatterbase, whose drivers implement it; in Triplespace it is a projection target, not the store projections write into.

> **Amended by [0032](0032-sparql-update-stream.md).** Every dump is stamped with the cursor of the SPARQL Update stream it continues into, and is offered in a second form with blank nodes skolemized (0032 §7); the plain form is unchanged. The local quad store is no longer a separate projection: it is fed from `view.rdf_delta`, the same rows the stream serializes, in process through `scatter-quadstore::apply` or externally through `triplespace-cli sparql-sync` (0032 §9).

[0001](0001-revision-metadata-rdf.md) §2's consumers column therefore reads: resolved view (main graph) for external triplestores and Wikibase tools; metadata graph for the instance's own SPARQL endpoint and full dumps. "Not published to external RDF stores" in 0001 §2 and "never exported to external stores" in [0011](0011-logs.md) §2 mean the Wikibase-compatible dump; a full dump is the instance's own export and carries both. The application's history, attribution, diffs and moderation views come from `view.activity` and the records.

### 9. Bootstrap mode (refines 0002 §8.6 and 0006 §5)

An initial load writes records first and builds everything else afterwards, as 0002 §8.6 already says. In Postgres:

1. The target child tables are created with the primary key only. The mirror partition's `(key, offset)` index and every `view` table are absent.
2. A coordinator hands each writer a contiguous block of offsets from `ops.bootstrap_block`. Writers `COPY` records in parallel. A block whose writer fails is handed out again, so no segment is sealed with a hole.
3. Leaf hashes are computed by the writers. Segment subtrees and the partition root are folded in one pass afterwards, as 0006 §5 describes, and written to `log.merkle_node`.
4. Indexes are built, then projections run in the order of §7, each in one pass over the partition.
5. Checkpoints are signed and the partition goes live.

Nothing about this is specific to Postgres except the use of `COPY` and the deferred index build, which is how any bulk load into it is done.

### 10. Crates (amends 0005 §2)

| Layer | Crate | Change |
|---|---|---|
| Substrate | `scatter-log` | Gains the `LogStore` trait: `append`, `read`, `scan`, `erase_parts` (named `erase_bodies` until [0015](0015-record-format-and-partition-registry.md) §7), `compact`, `head`. Its file backend, `segments`, implements it and remains the export-bundle format. |
| | `scatter-log-postgres` (new) | `LogStore` over the `log` schema of §2, with `sqlx`. It also owns that schema's migrations. Shared with Scatterbase, which may use it in place of files. |
| | `scatter-projection` | The projection trait gains a transactional variant, so a projection can apply inside the appending transaction (§7). |
| Triplespace | `triplespace-db` (new) | The `view`, `private` and `ops` schemas, their migrations, roles and grants (§4), connection pools, and replica routing (§7) |
| | `triplespace-projections` (new) | Every `view` projection of §5, in the order of §7. Absorbs `triplespace-activity` from [0010](0010-site-ui.md) §13. Pure computation stays in `scatter-wikibase-resolve`, `scatter-identity` and the other core crates; this crate only reads records and writes rows. |
| | `triplespace-rdf` (new) | The streaming RDF projection of §8, using `scatter-wikibase-rdf`. Absorbs `triplespace-revmeta`, the metadata-graph projection of [0001](0001-revision-metadata-rdf.md) and [0011](0011-logs.md) §10. |

The rules of [0005](0005-crate-organization.md) §3 hold: no core crate depends on `sqlx` or Postgres, async and I/O appear only in `scatter-log-postgres`, `scatter-ingest` and the surfaces, and the `segments` backend keeps `scatter-log` usable without a database.

### 11. Deployment profiles

The schema is the same everywhere. Two profiles differ only in what is optional:

| | Small instance | Wikidata scale |
|---|---|---|
| Postgres | One server | Primary plus replicas; mirror child tables on their own tablespace |
| `term_prefix` index | Present; serves suggest and `wbsearchentities` | Absent; OpenSearch serves them ([0014](0014-caches-and-search.md) §7) |
| `entity_ref` | Present | Present; the largest `view` table |
| Shared cache | Optional | Required ([0014](0014-caches-and-search.md) §2) |
| Quad store | Optional | Optional; QLever from dumps |

This is [0000](0000-init.md) §4's rule that smaller deployments follow as corollaries: they drop services, not tables.

### 12. Changelog of the 2026-09-27 revision

| Source | Change to this ADR |
|---|---|
| [0015](0015-record-format-and-partition-registry.md) §1–2, §7 | `log.record`: `revid`, `logid` and `page_id` as header columns; `body` never `NULL`; `erased` bitmask beside `erased_by`; the erasure statement in §3 rewrites parts; `erase_bodies` → `erase_parts` in §10; §6 rewritten for header IDs, provider-ranged revision IDs and carried-forward page IDs; two open questions closed |
| [0016](0016-permissions-and-access-control.md) §3, §9 | Permission tables in §5.6 and step 1 of §7; IP blocks in `private` |
| [0017](0017-entity-id-grammar.md) §4 | `view.entity.id` comment |
| [0018](0018-tenants.md) §2–3, §6 | `partition` as `bigint` everywhere; `tenant` on `log.partition` with the `(tenant, name)` key; the tenant column rule at the head of §5; `view.registry` keyed by tenant; per-tenant sequences in §6; overlay recomputation in §7; a farm-scale open question |
| [0019](0019-discussions.md) §2, §11 | Thread tables in §5.6 and step 5 of §7; talk-page IDs from the page sequence in §6 |
| [0020](0020-change-feeds.md) §3 | `private.watch` and `watch_token` in §4 and §5.6; the no-cross-schema-join rule in §4 |
| [0021](0021-notifications.md) §3, §5, §8, §10 | Inbox and contact tables in §4 and §5.6; the delivery queue in `ops`; `triplespace-notify` as the second reader of `private`; the addressing projection as step 6 of §7 |
| Third pass, 2026-09-27: [0022](0022-federation.md) through [0031](0031-property-constraints.md) | `partition` made `bigint` in `view.record_statement` and `view.activity` (and in `view.upstream_revision`, 0015 §4), which the head of §5 already required; `view.entity.deleted`, `view.page.reserved`, `view.actor.operator`, `groups`, the `kind` and `status` vocabularies, and `view.activity.patrolled` with its partial index written into the SQL they were listed against in §5.6; the `private` and `ops` cells of §4 list keys, tokens, consumer secrets, preferences, the hit IP row, per-actor ActivityPub keys, and the export, filter-test and re-check jobs; `triplespace-federation` named as the third reader in the table; the write path of §7 stated in order, with `filter` in step 1, `keyed_map` in step 2, `value_key` and `constraint_count` in step 4 and `filter_hit` in step 5; §5.6 gains the `ops` job rows, the rewritten `sitelink`, `view-pin` and `resolver`; the `NULL`-tenant key question added below |
| [0032](0032-sparql-update-stream.md) §2–3, §9 | `view.rdf_delta` and the delta epoch in §5.6; the delta projection noted in §7; §8 amended: stamped and skolemized dumps, and the local quad store fed from the delta table |

## Consequences

- **One durable system.** The log, its proofs, every projection and the private store share one backup, one replication stream and one recovery point. The storage-engine question open since 0000 is closed.
- **Interactive writes are one transaction.** Append, ID allocation, base-offset check and the synchronous projections commit together. There is no window in which an editor's own edit is invisible to them.
- **Storage at Wikidata scale is estimated, not measured.** A `latest` mirror of Wikidata is on the order of 1 TB of record bodies before compression; `entity_ref` and `term` are each on the order of 10^9 rows. The materialize-only-when-different rule (§5.1) keeps the resolved view from doubling the first figure. The bootstrap audit must measure all three before a full load is attempted.
- **Erasure has an operational tail.** An `UPDATE` erases the live tuple; `VACUUM` and WAL retention decide when the bytes are gone. The runbook, not the schema, guarantees the deadline.
- **Postgres is on the append path.** Local appends are serialized per partition by a row lock, which caps single-partition write throughput at what one transaction stream can do. That is well above what human editing produces, and bulk ingest does not use that path.
- **Scatterbase gains a Postgres backend without asking for one.** `scatter-log-postgres` is a substrate crate; its claim partition can live in Postgres or in files with no change above the trait.
- **The quad store is demoted.** Anything that assumed the metadata graph would be queried by the application has to read `view` instead. Nothing in 0010–0012 did.
- **Privacy is enforceable in two places.** The route-level test of 0012 §8 and the grant-level separation of §4 have to agree; a table that moves between schemas changes what a role can read.
- **Migrations become part of the compatibility surface.** A `view` change needs a rebuild or a migration; a `log` change needs a new payload or header version and a migration that keeps every existing header verifiable.

## Open questions

- **Measured sizes.** Record, `entity_ref` and `term` sizes for a full Wikidata mirror, and the compression ratio of canonical CBOR under `lz4` versus `zstd` (which needs an extension or a newer Postgres).
- **Where large mirrors live.** Whether mirror child tables should go to a separate database or tablespace, or whether their bodies should move to object storage behind the same `LogStore` trait once measured sizes justify it. Sharding is out of scope until then.
- ~~**Whether `revid` and `logid` enter the header.** Left to the global-ID ADR, which this ADR otherwise answers.~~ *Settled by [0015](0015-record-format-and-partition-registry.md) §2: they do, with `page_id`, as header fields 7–9 inside the Merkle tree.*
- **Statement rows for the local graph.** Whether `statement_assertion` should also index property and rank for every local statement, so that per-property views of large local entities can be paged in SQL instead of from the blob.
- **Vacuum policy.** How soon after an `erase` the affected child table is vacuumed, and whether `VACUUM FULL` is the default for reason class `legal`.
- ~~**Which projections are synchronous.** §7 puts 1–5 in the write transaction. `entity_ref` for an entity with tens of thousands of referrers may be too slow; the threshold at which a projection goes asynchronous for one write is to be set from measurement.~~ *Settled 2026-09-27 (§7 amendment): a write's own rows are always synchronous; fan-out to other entities is inline up to `projections.sync_budget` / `projections.sync_time`, then queued with lag reported. The default values are tuning.*
- ~~**`page_id` for entities on rebuild.** §6 assigns it from the entity's first record in append order. If two partitions first mention an entity in records that interleave differently on rebuild, the ID could differ; the rule may need to be "first record in the local partition, else first mirror record by partition number".~~ *Settled by [0015](0015-record-format-and-partition-registry.md) §2: the ID is in the header and carried forward, never derived.*
- **Partition count at farm scale** and whether small tenants share physical child tables ([0018](0018-tenants.md), open questions). [0028](0028-tenancy-policy.md) §2 adds three instance partitions per farm, which does not change the shape of the question.
- **The addressing projection's role.** §7 runs it under the notifier's role because it writes to `private`; whether `ops.projection_state` should record its position like the others, or `private.inbox` should carry its own high-water mark, is an implementation choice to settle with [0021](0021-notifications.md).
- ~~**A `NULL` tenant in a key.** §5 and [0018](0018-tenants.md) §6 give shared rows and instance-level registry entries a `NULL` tenant and say the tenant is in the key, but a Postgres primary key cannot hold `NULL`.~~ *Settled 2026-09-27 (§5): `tenant text NOT NULL DEFAULT ''`, the empty string being the instance, leading every primary key; a read tries the viewer's tenant and falls back to `''`.*

## References

- [Manual:Database layout](https://www.mediawiki.org/wiki/Manual:Database_layout), [Manual:Content table](https://www.mediawiki.org/wiki/Manual:Content_table) and [Manual:External storage](https://www.mediawiki.org/wiki/Manual:External_storage)
- [Wikibase term store (`wbt_*` tables)](https://doc.wikimedia.org/Wikibase/master/php/docs_storage_terms.html)
- [Manual:ChronologyProtector](https://www.mediawiki.org/wiki/Manual:ChronologyProtector)
- [PostgreSQL: table partitioning](https://www.postgresql.org/docs/current/ddl-partitioning.html), [TOAST and compression](https://www.postgresql.org/docs/current/storage-toast.html), [`COPY`](https://www.postgresql.org/docs/current/sql-copy.html), [`pg_last_wal_replay_lsn()`](https://www.postgresql.org/docs/current/functions-admin.html)
- [Kafka log compaction](https://kafka.apache.org/documentation/#compaction)
