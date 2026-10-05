# 0058. Packed record storage: fragments and dictionaries

- **Status:** Proposed
- **Date:** 2026-10-04
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0015](0015-record-format-and-partition-registry.md), [0018](0018-tenants.md), [0033](0033-backend-stack.md), [0039](0039-files-and-media.md), [0056](0056-security-model.md), [Record and payload shapes](../api/payloads.md)

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

A record's **logical form** is what [0015](0015-record-format-and-partition-registry.md) §1 and payloads.md define: the header and the body of salted parts, in canonical CBOR. Everything that hashes, signs, proves, exports or crosses a wire uses the logical form: leaves, commitments, content hashes, checkpoints, inclusion proofs, bundles, the `segments` backend, `LogStore::read` and `scan`, and every API.

A record's **stored form** is how `scatter-log-postgres` keeps the body in `log.record`. It is physical: the body is rebuilt from it byte for byte, and nothing outside the store can tell which form a row uses. Changing the stored form is never a format version. A partition may hold rows in several stored forms at once, and `repack` (§10) converts rows between them.

**Every read checks the commitment.** When a stored body is rebuilt, the store recomputes each present part's leaf and the commitment, and compares the result with the row's `commitment` column (header field 6). This costs one SHA-256 pass over a few kilobytes, a few microseconds. A mismatch is a `Corrupt` error, which is logged and never served. A packing bug, a damaged dictionary or a wrongly deleted fragment therefore fails loudly on the first read, never silently. A `plain` row is checked the same way.

### 2. The `log` schema (amends 0013 §2)

```sql
ALTER TABLE log.partition
  ADD COLUMN storage text NOT NULL DEFAULT 'plain';   -- plain | packed (§9)

ALTER TABLE log.record
  ADD COLUMN codec smallint NOT NULL DEFAULT 0,       -- 0 plain: body is the logical body
                                                      -- 1 packed: body is zstd(packed body), §3–4
  ADD COLUMN dict  integer;                           -- the log.dict a packed body was compressed with, or NULL

CREATE TABLE log.fragment_domain (                    -- §8: one per dedup scope
  domain    integer PRIMARY KEY,
  scope     text    NOT NULL UNIQUE,                  -- '' for the instance's shared partitions, else a tenant slug
  key       bytea   NOT NULL,                         -- 32 random bytes that key fragment references (§3)
  body_dict integer,                                  -- current dictionary for bodies, or NULL
  frag_dict integer,                                  -- current dictionary for fragments, or NULL
  epoch     integer NOT NULL DEFAULT 0,               -- bumped when a sweep starts (§6)
  sweeping  boolean NOT NULL DEFAULT false
);

CREATE TABLE log.fragment (                           -- §3
  domain    integer NOT NULL,
  ref       bytea   NOT NULL,                         -- 16 bytes
  bytes     bytea   NOT NULL,                         -- zstd of the subtree's packed CBOR
  dict      integer,                                  -- the log.dict it was compressed with, or NULL
  shared    boolean NOT NULL DEFAULT false,           -- more than one append has used it (§6)
  epoch     integer NOT NULL,                         -- the domain epoch when it was last written or pinned
  PRIMARY KEY (domain, ref)
) PARTITION BY LIST (domain);

CREATE TABLE log.dict (                               -- §4; immutable
  dict       integer PRIMARY KEY,
  domain     integer NOT NULL,
  purpose    text    NOT NULL,                        -- body | fragment
  bytes      bytea   NOT NULL,
  sha256     bytea   NOT NULL,
  trained_at timestamptz NOT NULL,
  samples    integer NOT NULL
);

CREATE TABLE ops.fragment_candidate (                 -- §6: possibly unreferenced fragments
  domain integer NOT NULL,
  ref    bytea   NOT NULL,
  PRIMARY KEY (domain, ref)
);
```

- **The child table of a packed partition sets `body` to `STORAGE EXTERNAL`,** so TOAST never tries to recompress zstd output. `log.fragment`'s children do the same for `bytes`. A plain partition keeps `lz4` as before.
- **Fragments, dictionaries and domains belong in the `log` schema.** Bodies cannot be rebuilt without them, so they share the log's backup and recovery point and the read grants of `ts_server` and `ts_verify` ([0013](0013-postgres-storage.md) §4). They are never exported: bundles carry logical bytes.
- **Fragments have one child table per domain,** so a tenant's fragments are dropped, vacuumed and sized on their own.

### 3. Fragments

A **packed body** is the logical body with zero or more subtrees inside present parts replaced by **fragment references**. Erased parts (`[null, leaf]`) are left as they are.

- **A reference** is CBOR tag `0x54530001` wrapping a 16-byte string. Canonical CBOR has no tags (payloads.md §1), so a tag in stored form is unambiguous. The tag is never registered, and it never appears outside `log.record` and `log.fragment`.
- **`ref`** is the first 16 bytes of HMAC-SHA-256 under the domain's `key`, computed over the subtree's **logical** bytes (with nested references expanded). A ref therefore does not depend on how the subtree's own children were packed. Being keyed, refs cannot be computed or probed without the key, and colliding inputs cannot be crafted. At 128 bits, accidental collisions are negligible: about 10⁻¹⁵ expected at 10¹² distinct fragments. Any collision, accidental or not, is still caught by §1's commitment check.
- **A fragment's `bytes`** are the subtree's canonical CBOR, packed in turn: a statement fragment can refer to its reference fragments. Rebuilding splices bytes recursively. Because canonical CBOR is compositional (a map's encoding is its header followed by its encoded pairs), splicing reproduces the logical bytes exactly, with no re-encoding. The depth limit is 4.
- **The packer is generic.** `scatter-log` walks CBOR by path pattern and knows nothing about Wikibase. The Wikibase knowledge lives in the registry file, so that file can change at any time, and `repack` (§10) applies the change to old rows.

**Which subtrees become fragments** is registry data. `docs/registry/fragments.toml` is authoritative and gives path patterns per payload type, a `history` condition and `min_bytes`. As of this ADR it lists:

| Payload type | Path | History | What |
|---|---|---|---|
| `scatter:v0/changeset` | `/**/claims/*/*/references/*` | any | One reference: `snaks`, `snaks-order`, `hash` where kept |
| `scatter:v0/changeset` | `/references/*/*` | any | One reference added by an `add` op |
| `scatter:v0/changeset` | `/**/claims/*/*/qualifiers` | any | A statement's qualifier map |
| `scatter:v0/changeset` | `/**/claims/*/*` | `full` | A whole statement, GUID included, which repeats across one entity's revisions |

A subtree is packed only if it encodes to at least `min_bytes` (64 initially). A reference costs 22 bytes (a 5-byte tag head, a 1-byte string head and 16 bytes of ref), so anything smaller is cheaper inline. Repeats inside one record are packed once. Statement fragments are limited to `full` partitions: in a `latest` partition every statement GUID is unique to its entity and every edit replaces the statement, so a fragment for it would save nothing and become garbage on the next compaction.

#### 3.1 Writing a packed record

Inside the appending transaction ([0013](0013-postgres-storage.md) §7), after the logical record is sealed (header, leaf and commitment unchanged), a packed partition:

1. reads its domain's row (`epoch`, `sweeping`, `body_dict`, `frag_dict`), one cached heap page;
2. packs the body bottom-up, computing each ref locally;
3. writes the fragments:
   - **No sweep running:** a ref the process holds in its *known-shared* cache (§11) for the current epoch is skipped. Every other ref goes through `INSERT … ON CONFLICT (domain, ref) DO NOTHING RETURNING ref`. Refs that conflicted are marked with `UPDATE log.fragment SET shared = true WHERE domain = $d AND ref = ANY($conflicted) AND NOT shared`, which locks only rows that are not yet shared. That is at most once in each fragment's life, so a popular reference is never a hot row.
   - **A sweep running:** every ref, cached or not, goes through `INSERT … ON CONFLICT (domain, ref) DO UPDATE SET epoch = $epoch, shared = true`. This pins it against the sweep (§6). Pinning costs row locks for the sweep's duration, which is why bulk loads and sweeps exclude each other (§7).
4. compresses the packed body with the domain's body dictionary and inserts the row with `codec = 1`.

### 4. Dictionaries

Small bodies and fragments compress badly alone, and well against a dictionary trained on their siblings. The keys, data-type names, common property IDs and common item IDs that every Wikidata entity repeats are exactly what a dictionary captures.

- **Dictionaries are per domain and purpose** (`body`, `fragment`), trained with zstd's trainer (`ZDICT_trainFromBuffer`, through the `zstd` crate) on a sample of that domain's packed bodies or fragments, about 110 kB each.
- **A domain starts without a dictionary** and compresses with plain zstd. `triplespace-cli storage retrain` trains one once a domain holds enough samples (10,000 bodies by default). New writes use the new dictionary, and old rows keep the one they name.
- **Dictionaries are immutable and never deleted while a row names them,** checked by `retrain --prune`. They are small enough to keep forever, and every process holds the dictionaries of the domains it serves.
- **Level:** zstd 3 for interactive appends and 19 for `repack` and bulk loads. Decompression speed is about the same at either level.

### 5. Reading

`LogStore::read` and `scan` return logical records. For a packed row the store decompresses the body, collects its refs, fetches the missing ones with one `SELECT … WHERE domain = $d AND ref = ANY($refs)` per batch (a page of records in a `scan`), splices them, recurses for nested refs, and checks the commitment (§1). Fragments are immutable, so the fragment cache (§11) holds them without invalidation except on erasure.

The serving read of a `source` entity ([0013](0013-postgres-storage.md) §5.1) is therefore one row, at most one fragment query on a cold cache, and a decompression. Above the store, the L1 entity JSON cache ([0014](0014-caches-and-search.md) §4) hides even that for hot entities.

### 6. Garbage: candidates, sweeps and erasure (extends 0013 §3; extends 0006 §7)

There are no reference counts. A fragment becomes garbage only when the rows that used it are compacted away or erased, and two mechanisms reclaim it.

**Candidates.** Compaction and erasure write the refs of what they remove (the deleted rows' bodies, the erased parts) into `ops.fragment_candidate`, in their own transaction. A **candidate sweep** deletes every candidate that is **not `shared`**. `shared = false` means exactly one append ever used the fragment, and that append's row is the one just removed. Deleting a fragment makes its nested refs candidates in turn. A shared candidate is left for a full sweep, and the domain's count of such leftovers decides when one is worth running.

**Full sweeps.** A **full sweep** marks every ref reachable from the domain's live rows (bodies, then nested fragments) into a temporary table, then deletes every fragment that is not marked and whose `epoch` is older than the sweep's.

**The fence.** Both kinds of sweep start the same way:

1. set `sweeping = true` and increment `epoch` on the domain's row;
2. wait until every transaction that began before that commit has ended, as `CREATE INDEX CONCURRENTLY` does.

After the fence, every append into the domain sees `sweeping` and pins each ref it uses (§3.1 step 3). An append that began before the fence has committed by the time marking starts, so the mark sees its row. If a deletion and a pin race on the same fragment, Postgres's row lock decides. If the pin commits first, the deletion's recheck finds the new `epoch` and skips the row. If the deletion commits first, the pin's `INSERT` finds no row and writes the fragment again, from the bytes the writer is holding. When the sweep ends, `sweeping` is cleared and a `NOTIFY ts_fragment` names the domain and epoch.

**Erasure.** The erase transaction rewrites the target's body as [0013](0013-postgres-storage.md) §3 says. In a packed row the remaining parts stay packed, and the erased parts' refs become candidates. The erasure worker then runs a candidate sweep before the `VACUUM` it already runs, so bytes used only by the erased record are gone within the same job. A fragment the erased record **shared** with other rows is still held by them, legitimately, unless they too are gone. For reason class `legal`, the worker therefore runs a full sweep of the domain as well, and vacuums the domain's fragment table afterwards. [0013](0013-postgres-storage.md) Q5 (vacuum policy) covers the fragment tables as it covers `log.record`.

### 7. Bulk loading (extends 0013 §9)

Bootstrap writers pack in-process. Each writer holds the domain key and, if one exists yet, the dictionaries. A first block's sample trains the dictionaries before the load if none exist. Each writer computes refs locally, keeps the set of refs it has already written, and `COPY`s new fragments into an unlogged staging table beside its records. After the load, one statement merges the staging table into `log.fragment` with `ON CONFLICT DO NOTHING`, setting `shared` where a ref was written by more than one writer or already existed. Writers never wait on one another for a ref.

**A bulk load and a sweep of the same domain exclude each other** by an advisory lock on the domain, so the pinning cost of §3.1 never meets bootstrap volume. Incremental ingest jobs ([0002](0002-source-graphs-and-mass-ingest.md) §8) take the same lock in shared mode, and a sweep takes it exclusively.

### 8. Domains and tenants (uses 0018 §2, 0039 §4, 0056 §1)

A **domain** is the scope within which a fragment is stored once.

- **By default each tenant is its own domain,** covering all its partitions, and the instance's shared partitions (those with a NULL tenant: the provider mirrors) form domain `''`. A tenant's rows refer only to its own domain's fragments.
- **An instance may choose one domain for everything** with `storage.dedup_scope = "instance"`, as [0039](0039-files-and-media.md) §4 lets it share file storage. It is told what it gives up: a tenant's fragments can no longer be dropped with its partitions, and erasure sweeps cover every tenant's rows.
- **Keyed refs and separate domains keep storage inside the 0056 boundary.** No tenant's write can learn, even through timing, whether another tenant holds the same bytes. When a tenant's partitions are dropped (reclaim, [0039](0039-files-and-media.md) §9), its domain's child table goes with them. Moves ship logical bundles, which know nothing of fragments.
- **A `private` partition is never packed.** The accounts partition's child table is owned by the accounts role ([0013](0013-postgres-storage.md) §4), and its content must not land in a table that `ts_server` reads.

Rows of a tenant's local graph do not refer to fragments in the shared domain, even where a local statement cites the same reference as the mirror. That would couple the shared domain's sweeps and erasures to every tenant's rows. Whether the saving is worth that coupling is Q2.

### 9. Which partitions are packed

`log.partition.storage` is an operator setting, not a log record: `[storage] packed = ["mirror/*", "local"]` in the server's configuration, applied when a partition is created and changed later with `storage pack` and `repack` (§10). The default is `plain` for every partition until Q1's measurements are recorded here. Then `mirror/*` is expected to become `packed`, since mirror partitions hold nearly all of the volume.

Interactive appends to a packed `local` partition pay for one HMAC per fragment-sized subtree, one `INSERT` per new fragment and a zstd pass, inside a transaction that already runs projections 1–5. That is small next to the projections. Packing `local` pays mainly for full-history tenants that adopt or backfill ([0035](0035-adopting-a-wikibase.md) §7).

### 10. Measurement and repacking

`triplespace-cli storage measure --partition {name} [--sample N]` packs a sample of a partition's rows without writing anything, and reports bytes per record under each candidate: plain canonical CBOR, `lz4` alone (what TOAST would do), zstd, zstd with a dictionary, and packed with a dictionary. It also reports the number of distinct fragments, the distribution of how many times each is used, and fragment bytes per body byte. Run over a sample of the Wikidata JSON dump through `scatter-adapter-wikidata`, and over Librarybase's adoption, it answers Q1 and [0013](0013-postgres-storage.md) Q1.

`triplespace-cli storage repack --partition {name} [--dict new]` rewrites rows in batches to the partition's current storage setting, dictionary and `fragments.toml`. It checks every row's commitment before and after, and runs as a writer under §3.1, so it is safe beside sweeps. `storage sweep --domain {scope} [--full]` runs §6 on demand. `storage pack` changes `log.partition.storage`.

### 11. The fragment cache (extends 0014 §10)

Each process keeps an L0 LRU of fragments by `(domain, ref)`, holding their decompressed bytes, together with a set of refs known to be `shared` in the current epoch, which §3.1 uses to skip writes.

- **The known-shared set is dropped whenever the domain's `epoch` differs from the one it was built under.** Every append reads the epoch (§3.1 step 1), so a writer never relies on a ref that a sweep has since deleted.
- **The LRU drops a domain's entries on `NOTIFY ts_fragment`,** and on erasure that is the seventh erasure step of [0014](0014-caches-and-search.md) §10. Entries also expire after the L0 ceiling, which bounds a missed notification, as 0014 §5's TTL ceilings bound a missed purge.

### 12. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-log` | The stored-form codec: path patterns over CBOR, packing into fragment references, splicing back, and the commitment check on rebuild; embeds `docs/registry/fragments.toml`. Pure, and it does not compress |
| `scatter-log-postgres` | `codec`, `log.fragment_domain`, `log.fragment`, `log.dict`, `ops.fragment_candidate`; the write protocol of §3.1; zstd and dictionaries; candidate and full sweeps with the fence; the fragment cache and its `NOTIFY` listener; the bootstrap staging merge. Gains `zstd` and `hmac` |
| `triplespace-cli` | `storage measure`, `repack`, `retrain`, `sweep` and `pack` |

The `segments` backend is unchanged: a segment file holds logical slots, and any compression there is a matter for the file format of payloads.md §10.

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
