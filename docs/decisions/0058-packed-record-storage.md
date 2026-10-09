# 0058. Packed record storage: fragments and dictionaries

- **Status:** Proposed
- **Date:** 2026-10-04
- **Updated:** 2026-10-09 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0015](0015-record-format-and-partition-registry.md), [0018](0018-tenants.md), [0033](0033-backend-stack.md), [0039](0039-files-and-media.md), [0056](0056-security-model.md), [Record and payload shapes](../api/payloads.md)
- **Chapters:** [01](../architecture/01-log-and-records.md), [03](../architecture/03-storage-caches-and-search.md), [05](../architecture/05-providers-and-ingest.md), [08](../architecture/08-tenants-and-instances.md), [22](../architecture/22-crates-and-stack.md)

## Context

Wikidata repeats itself. The same reference block ("imported from Wikimedia project: English Wikipedia"; "stated in" a database with a "retrieved" date) sits under millions of statements. Qualifier sets repeat. Every statement spells out the same keys: `mainsnak`, `snaktype`, `property`, `datavalue`, `datatype`, `type`, `rank`, `references`, `snaks-order`. Where history is kept, consecutive whole-entity `put`s of one entity share nearly every statement. A full-history partition holds retained entities and backfills ([0002](0002-source-graphs-and-mass-ingest.md) §2, §5), and later any provider an instance mirrors with `full` history.

[0013](0013-postgres-storage.md) §2 stores each record's body as one `bytea` of canonical CBOR, compressed by TOAST with `lz4`. That captures none of the repetition above:

- **TOAST compresses only large rows.** Postgres tries compression only when a row exceeds about 2 kB (`TOAST_TUPLE_THRESHOLD`). Many entity bodies are smaller and are stored as they are.
- **TOAST compresses each value alone.** `lz4` finds repetition inside one body, never across bodies, so the ten-millionth copy of a reference costs the same as the first.
- **Postgres 18, the target ([0033](0033-backend-stack.md) §4), offers `pglz` and `lz4` only.** It has no `zstd` for TOAST and no shared dictionaries. 0013 Q1 already asked for `lz4` and `zstd` to be measured against each other.

0013's Consequences estimate a `latest` mirror of Wikidata at "on the order of 1 TB of record bodies before compression". The serving path reads those bodies directly for every entity whose resolved view is its single source ([0013](0013-postgres-storage.md) §5.1). So the bodies are both the largest thing in the database and the hottest.

On 2026-10-03 the design discussion compared Entitybase (`Entitybasedev/entitybase-monorepo`), a clean-room Wikibase backend that deduplicates statements, references, qualifiers, snaks and terms into content-addressed MySQL tables with reference counts. It showed both the size of the prize and the traps. Its reference counts are a write hotspot: a common reference's row is updated on every edit that uses it. Its keys are 64-bit hashes, with no check that stored content matches. Its revisions are immutable only at the API, while the rows underneath are counted and garbage-collected.

Triplespace's constraints are different:

1. **The record format is fixed.** Bodies are hashed per part, with a 16-byte salt per part ([0015](0015-record-format-and-partition-registry.md) §1), and changing anything in [payloads.md](../api/payloads.md) after the first instance is a format version. Deduplication must not change a single logical byte.
2. **Erasure must really remove bytes** ([0006](0006-log-integrity-and-erasure.md) §7, [0013](0013-postgres-storage.md) §3). Shared storage must not keep an erased part alive.
3. **Tenants do not bleed into one another** ([0056](0056-security-model.md) §1). [0039](0039-files-and-media.md) §4 already made the same trade for files: separate storage scopes by default, deduplication across tenants as an instance's explicit choice.
4. **Bulk loading uses parallel writers without coordination** ([0013](0013-postgres-storage.md) §9).

### Direction

James's direction, from the design discussion of 2026-10-04:

- **"There's indeed a lot of repetition across Wikidata. What changes to the database schema would you recommend to implement this optimization? This seems like an ADR topic."**

## Decision

### 1. Stored form and logical form (uses 0015 §1; amends 0005 §3; amends 0013 Consequences)

*Current text: [01](../architecture/01-log-and-records.md) §2.7, §8; [03](../architecture/03-storage-caches-and-search.md) §3.1.*

### 2. The `log` schema (amends 0013 §2)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §2.1, §3.2, §5.*

### 3. Fragments

*Current text: [03](../architecture/03-storage-caches-and-search.md) §3.3.*

#### 3.1 Writing a packed record

*Current text: [03](../architecture/03-storage-caches-and-search.md) §3.4.*

### 4. Dictionaries

*Current text: [03](../architecture/03-storage-caches-and-search.md) §3.5.*

### 5. Reading

*Current text: [03](../architecture/03-storage-caches-and-search.md) §3.6.*

### 6. Garbage: candidates, sweeps and erasure (extends 0013 §3; extends 0006 §7)

*Current text: [01](../architecture/01-log-and-records.md) §5.2, §6; [03](../architecture/03-storage-caches-and-search.md) §3.7.*

### 7. Bulk loading (extends 0013 §9)

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.8.*

### 8. Domains and tenants (uses 0018 §2, 0039 §4, 0056 §1)

*Changed by A1.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §9.2.*

### 9. Which partitions are packed

*Current text: [03](../architecture/03-storage-caches-and-search.md) §3.8.*

### 10. Measurement and repacking

*Current text: [03](../architecture/03-storage-caches-and-search.md) §3.8.*

### 11. The fragment cache (extends 0014 §10)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.5, §9.6, §12.2.*

### 12. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **Normalized component tables in `view`, Entitybase's design.** These would add statement, reference and snak tables keyed by hash, with each entity's revision listing hashes. The log would still hold full bodies, so this adds storage rather than removing it, and the serving path reads bodies from the log anyway. Reference counts make every popular reference a row updated on every edit that uses it.
- **Content-addressed records: statements as their own log entries, entities as lists of hashes.** That is a new format version, and it undoes 0015 §1. A shared, unsalted statement hash is publicly computable, so an erased short statement could be confirmed by guessing, which is what per-part salts prevent. Erasing one entity's statement would also have to reckon with every other entity that shares it. Granularity was settled by 0000 Q2 and 0002 §8.
- **Status quo with a newer Postgres.** TOAST still compresses large values one at a time. A future zstd TOAST method would help with small rows only if it supported dictionaries, and it would not see repetition across rows.
- **Filesystem compression** (ZFS or btrfs with zstd under the data directory). This captures some repetition across rows within a record-sized block, costs no code, and stays available to operators who run their own disks. It is not available on managed Postgres, does not deduplicate at a distance, and is not something the schema can count on. It complements this decision rather than replacing it.
- **Sequence-allocated fragment IDs (`bigint`) instead of keyed hashes.** References would be 8 bytes instead of 16, but every writer would need a database round trip to learn an ID. That defeats the uncoordinated bulk writers of §7, and the IDs would differ between instances for identical content.
- **Full 32-byte refs.** These would cost 16 more bytes per reference for no gain. Keying makes collisions impossible to craft, and §1's check catches any collision that occurs.
- **Reference counts with batched updates.** Batching cuts the hotspot but keeps a counter that every compaction and erasure must update exactly, and a wrong count either leaks or deletes live data. Sweeps can only leak, and only until the next sweep.

## Consequences

- **The record format is untouched.** Proofs, bundles, the `segments` backend, verification and every API see logical bytes. A packed instance and a plain one export identical bundles.
- **Every stored body is verified on read.** Before this ADR, a damaged body in Postgres surfaced only when `verify` ran. Now any read of a plain or packed row checks it.
- **Reads cost a decompression and, on a cold cache, one fragment query.** Rebuilding projections over a packed partition reads fragments in batches per page of records. Q1's measurement includes rebuild throughput.
- **Erasure gains a step.** A candidate sweep runs in every erasure job, and a `legal` erasure in a packed domain also runs a full sweep. For the shared Wikidata domain, that sweep is a scan of the domain's bodies, measured in hours, not seconds.
- **Garbage is bounded by sweep cadence.** Compaction garbage that was shared waits for a full sweep. The count of shared candidates tells the operator when one is worth running.
- **Appends pin fragments while a sweep runs,** which costs row locks on popular references for the sweep's duration. Bulk loads wait for sweeps to finish.
- **This is a Postgres optimization.** The trait and the `segments` backend are unchanged, so Scatterbase gains it without asking for it ([0013](0013-postgres-storage.md) Consequences).
- **The savings are unmeasured.** The default stays `plain` until Q1 is answered.

## Open questions

- **Q1. Measured savings.** `storage measure` over a Wikidata JSON dump sample and Librarybase: bytes per entity under each codec, fragment reuse, rebuild throughput, and the right `min_bytes`. Together with [0013](0013-postgres-storage.md) Q1, this decides whether `mirror/*` defaults to `packed`.
- **Q2. Tenant rows referring to the shared domain.** Whether a tenant's local rows may refer to fragments in domain `''` (mirror data, public to every tenant), at the price of sweeps and erasures in the shared domain having to scan tenant rows.
- **Q3. Page text.** Wikitext revisions from history backfills ([0015](0015-record-format-and-partition-registry.md) A25), forked articles ([0054](0054-forking-a-mirrored-page.md)) and mirrored HTML ([0053](0053-mirrored-pages.md)) repeat across revisions far more than entities do. This calls for delta or content-defined-chunk deduplication, as MediaWiki's external storage does, which is a different mechanism with its own ADR.
- **Q4. Term text.** Whether `view.term` should move its text into a normalized table, as Wikibase's `wbt_text` and `wbt_text_in_lang` do, once its size is measured. The `term_prefix` index would then need the join.
- **Q5. Materialized resolved JSON.** Whether `view.entity.resolved` should use the same codec. It holds only entities whose resolved view differs from its source, so it is a minority of rows by design ([0013](0013-postgres-storage.md) §5.1).
- **Q6. Erasing shared fragments without a full sweep.** If legal deadlines in a large shared domain need it, add an inverted index from fragment to rows, kept only for fragments whose content an `erase` names, rather than for all of them.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §12 | extends | 0005 A61 |
| [0005](0005-crate-organization.md) §3 | §1 | amends | 0005 A61 |
| [0006](0006-log-integrity-and-erasure.md) §7 | §6 | extends | 0006 A15 |
| [0013](0013-postgres-storage.md) §2 | §2 | amends | 0013 A31 |
| [0013](0013-postgres-storage.md) §3 | §6 | extends | 0013 A31 |
| [0013](0013-postgres-storage.md) §9 | §7 | extends | 0013 A31 |
| [0013](0013-postgres-storage.md) Consequences | §1 | amends | 0013 A31 |
| [0014](0014-caches-and-search.md) §10 | §11 | extends | 0014 A16 |

## References

- Entitybase, `Entitybasedev/entitybase-monorepo`, reviewed 2026-10-03: content-addressed statement, reference, qualifier, snak and term tables with reference counts.
- PostgreSQL 18 documentation, "TOAST" (§65.2): the compression threshold and the `pglz` and `lz4` methods.
- PostgreSQL 18 documentation, `CREATE INDEX CONCURRENTLY`: waiting out older transactions.
- Zstandard, "Dictionary compression": `ZDICT_trainFromBuffer` and why small inputs need a dictionary.
- RFC 8949 §4.2.1 (core deterministic encoding) and §3.4 (tags); RFC 2104 (HMAC).
- Wikibase term store (`wbt_text`, `wbt_text_in_lang`, `wbt_term_in_lang`, `wbt_item_terms`): normalization of repeated term text.
- MediaWiki external storage and history blobs: deduplication of revision text across revisions.

## Amendment log

### A1. Tenant deletion drops the domain's child table after the `erase` records

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §8
- **Summary:** Tenant deletion is [0028](0028-tenancy-policy.md) §5 and [0040](0040-instance-prerogatives.md) §6, not 0039 §9's reclaim: a tenant's partitions are dropped after its `erase` records are written, and only then can the domain's child table go with them. §8 cited the wrong ADR and did not say when the drop happens. (PENDING C8)

Replaced text (§8):

> When a tenant's partitions are dropped (reclaim, [0039](0039-files-and-media.md) §9), its domain's child table goes with them.

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§12
- **Summary:** The Decision's current text now lives in the architecture chapters [01](../architecture/01-log-and-records.md), [03](../architecture/03-storage-caches-and-search.md), [05](../architecture/05-providers-and-ingest.md), [08](../architecture/08-tenants-and-instances.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
