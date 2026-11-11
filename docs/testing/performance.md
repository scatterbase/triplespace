# Write-path performance

How fast the write path is, how it was measured, what was fixed, and what a Wikidata-sized load
still needs. The procedure for the real-dump runs themselves is [`real-dumps.md`](real-dumps.md).

Moved into the repo on 2026-10-21 from working notes of 2026-10-05/06. Figures are from those
runs; re-measure before relying on them.

## 1. Librarybase adoption, before and after

Full Librarybase dump (`librarybase-20261005.xml.gz`, 247,011 entities), `adopt --batch 200`,
one connection. "Before" is `main` before #56; "after" is `3d84c87` + #56 + migration 0008.

| | before | after |
|---|---|---|
| wall | 51m44s | **11m56s** (4.3×) |
| per entity | 12.6 ms | **2.9 ms** |
| entities/s | 80 | **345** |
| user CPU | 10m50s (2.6 ms/entity) | 4m58s (1.2 ms/entity) |
| sys CPU | 11m31s (2.8 ms/entity) | 1m58s (0.5 ms/entity) |
| SQL statements | 44.5 per entity | **17.05 per entity** (4,212,753) |
| server exec / plan | 9.1 s / 13.6 s per 10k entities | 102.8 s / 16.8 s for the full run |
| `view.identifier` rows | 0 (bug, fixed in `3d84c87`) | 246,453 |

`rebuild` of the same log took **10m01s** (3m45s user, 1m28s sys) and produced an identical `view`
schema (1,481,456 rows). The catch-up path costs about what inline adoption does.

**Where the 716 s went after the fixes:** client CPU 416 s (58%), Postgres 120 s (17%), the wire
about 180 s (25%). The database is no longer the bottleneck; the client's per-entity CPU is, and it
has not been profiled.

The per-entity statement mix on the full run matched the 10k slice, so nothing grew with table
size. The remaining planning time is the `read_by_key` select on the partitioned `log.record`,
which Postgres re-plans per call despite the prepared statement (it prefers a custom plan for
plan-time pruning). That is 2% of the run; `plan_cache_mode = force_generic_plan` on the writer's
connection would remove it.

## 2. What the cost was, and the fixes

Before #56, every entity cost about 45 SQL statements, each sent unprepared, so each was two round
trips (Parse+Describe, then Bind+Execute): about 90 round trips per entity at roughly 140 µs each
over loopback TCP. Postgres spent more time *planning* these point lookups than running them (60% of
server time), and `page_id_of` alone was 15% of server time in planning.

| Fix | Statements removed per entity | What it did |
|---|---|---|
| 3.1 Projection positions in memory | 12 | `set_applied` writes to a map on `PgCx`; `commit` flushes all positions in one `unnest` upsert (`projection_state::set_applied_many`). |
| 3.2 Merkle frontier in memory | 8–10 | `Frontier::push_leaf_completing` returns the nodes a leaf completes; `merkle::put_nodes` inserts them in one statement; `Frontiers` carries one frontier per partition per unit of work, checked against `next_offset` under the row lock. |
| 3.3 Partition metadata cached | 6 | The `Partitions` trait: `PgCx` answers `partition_info`/`partition_of` from a per-transaction cache (positive results only). |
| 3.4 Prepared statements | (planning) | `scatter_log_postgres::PgClient`; deadpool's `prepare_cached`. `PgCx` no longer derefs to the raw client, so a forgotten trait import fails to compile. |
| 3.5 Multi-row inserts, pipelining | T + I − 4 | Terms and identifiers are one `DELETE` + one `unnest` `INSERT` each; the entity upsert and both writes are pipelined with `tokio::try_join!`. |
| 3.6 Dead work | 1 | `read_by_key` no longer re-reads the partition row. |
| Migration 0008 | — | `entity_by_id`, `entity_source_by_entity`, `identifier_entity`. `DELETE FROM view.identifier` went from a sequential scan (about 300 µs and growing) to 6 µs. |

**Partial dumps need every property page.** XML dumps store entity JSON without `datatype` on snaks;
`adopt` types snaks from the dump's own property pages (`3d84c87`). Pywikibot and
WikidataIntegrator also fetch every property an item uses, so a slice must carry all property pages
(`tools/dump_slice.py --all-properties`, with `--defined-only` to drop items a prefix slice cannot
type).

## 3. Measuring

1. In `postgresql.conf`: `shared_preload_libraries = 'pg_stat_statements'` and
   `pg_stat_statements.track_planning = on`. Restart.
2. `CREATE DATABASE ts_lb_10k;`, run `instance create` into it, then `CREATE EXTENSION
   pg_stat_statements;` there.
3. `python3 tools/dump_slice.py data/librarybase-20261005.xml data/librarybase-slice-10k.xml.gz --limit 10000`
4. `SELECT pg_stat_statements_reset();`, then `adopt` the slice with the same flags as the full run.
5. `pg_stat_statements` is cluster-wide, so filter by database:

```sql
SELECT sum(calls) AS statements, round(sum(calls)/10000.0, 1) AS per_entity,
       round(sum(total_exec_time)) AS exec_ms, round(sum(total_plan_time)) AS plan_ms
FROM pg_stat_statements WHERE dbid = (SELECT oid FROM pg_database WHERE datname = 'ts_lb_10k');

SELECT calls, round(calls/10000.0, 1) AS per_entity,
       round(total_exec_time) AS exec_ms, round(total_plan_time) AS plan_ms,
       left(regexp_replace(query, '\s+', ' ', 'g'), 90) AS query
FROM pg_stat_statements
WHERE dbid = (SELECT oid FROM pg_database WHERE datname = 'ts_lb_10k')
ORDER BY calls DESC LIMIT 40;
```

Re-run into a fresh database after each change; the per-entity column is the scoreboard. With
prepared statements, `plan_ms` should be near zero. A point lookup whose `exec_ms` is out of
proportion to its `calls` is the signature of a missing index.

Also worth trying: connect over the Unix socket instead of loopback TCP (a percent-encoded socket
directory in the URL, as in [`real-dumps.md`](real-dumps.md) §0.1). While round trips dominate, the
lower per-message latency shows up directly. `top` on the client alongside the run shows whether the
client or Postgres is busy.

## 4. Postgres settings

Measured effect on the load: about 3% (the load is round-trip bound); it helps the running site
more. For a machine with `R` of RAM:

| Setting | Default | Suggest | Why |
|---|---|---|---|
| `shared_buffers` | 128MB | R/4 | 128MB cannot hold the hot indexes (`merkle_node` PK, `term` PK and `term_prefix`, `identifier` PK, `entity_source` PK, the record partitions' key indexes) |
| `effective_cache_size` | 4GB | R/2 to 3R/4 | planner hint only |
| `max_wal_size` | 1GB | 8GB or more during loads | with 1GB a load checkpoints every minute or two, and each checkpoint brings full-page images for randomly touched pages across about eight indexes |
| `checkpoint_timeout` | 5min | 30min | same |
| `wal_compression` | off | `zstd` | compresses those full-page images |
| `work_mem` / `maintenance_work_mem` | 4MB / 64MB | 32MB / 1GB | the latter for index builds and vacuum after a load |
| `synchronous_commit` | on | leave on | about 1,235 commits per full adoption; fsyncs are not the problem |
| `random_page_cost` | 4.0 | 1.1 on SSD | planner |

## 5. Open problems on the write path

1. **`page_id_of` probes every record partition** (`triplespace-api-ingest/src/store.rs`): the join
   on `log.partition` gives the planner nothing to prune by. Resolve the tenant's partitions first
   (free, from the 3.3 cache) and query `partition = ANY($2)`.
2. ~~**`sync` materializes the whole dump in a `Vec<SyncItem>`** before `run_sync`.~~ Since
   2026-11-04 it streams (`SyncItems` in `triplespace-cli/src/sync.rs`): the dump is read one
   entity at a time as the job takes them. The job still keeps the set of IDs seen (in every mode,
   for the snapshot sweep), so a full Wikidata sync holds every ID, not every entity, in memory.
3. ~~**`sync` has only the internetdomains adapter.**~~ Since 0091, `sync` dispatches on the
   registry's `adapter` field and reads every Wikibase provider, Wikidata included, through
   `scatter-adapter-wikibase`; a JSON dump without key maps is read in one pass.
4. **The NDJSON job endpoint and the interactive API share `write_to`**, so a bulk import over the
   API runs at the per-entity rate (about 345 entities/s per job on Librarybase-shaped entities).
5. **Client CPU (1.2 ms/entity) is unprofiled.** Per entity the op is parsed from XML, validated,
   cloned, converted to JSON then to the CBOR value, encoded and hashed; the projection decodes it
   again and builds canonical JSON even when only its length is kept. The survey pass also reads and
   gunzips the whole dump before the job pass does it again. Profile with `perf record -g` or
   `samply` on the 10k slice before changing anything.

## 6. A Wikidata-sized load

**Why the Librarybase rate does not carry over.** Wikidata has about 117M items, 1.3M lexemes and
the properties. At 345/s that is about 4 days, if entities were Librarybase-sized. They are not: a
Librarybase citation item is 1–2 KB, and the mean Wikidata item is around ten times that, and every
per-entity cost scales with bytes or rows. On the present per-entity path, expect 10–20 ms per
Wikidata entity: **2–4 weeks**.

**The term table.** Wikidata has 1.5–2 billion labels, descriptions and aliases. One `view.term` row
per term, with its PK and `term_prefix` on `lower(text)`, is hundreds of GB of index: structurally
the `wb_terms` table Wikidata itself abandoned. 0014 §8 names `term_prefix` the small profile's
fallback. Before any full mirror, decide whether mirror entities populate `view.term` at all (all
languages, a configured subset such as `entities.term_languages`, or none, with search delegated).
[0070](../decisions/0070-shallow-entity-mirroring.md) keeps shallow mirrors to the configured term
languages; a full sync needs the same decision. Related: `view.activity` gets a row per mirrored
record, which a bulk sync probably should not produce.

**What gets to roughly 3,000 entities/s** (each independent; items 1–2 alone are about a week,
all five plus not materializing mirror terms is about overnight):

1. **Overlap parsing and writing.** Parsing, validating and encoding (1.2 ms) currently alternate
   with waiting on the database (1.7 ms). Worker threads feeding a bounded channel to the database
   task approach the maximum of the two instead of the sum: 1.5–2×, and it helps `adopt` and `sync`
   too. This is the only parallelism within one partition, since `FOR UPDATE` on the partition row
   serializes appends by design (gapless offsets, 0006 §3).
2. **Batch-shaped writes (`write_many`).** Per block of 200: lock the partition once, allocate
   offsets and revision IDs in one statement (`SELECT nextval(…) FROM generate_series(1, $1)`), one
   multi-row `INSERT` or `COPY … FROM STDIN BINARY` per table, and the block's Merkle nodes from the
   in-memory `Frontier` (the groundwork is in 3.2). About 10 statements per 200 entities. Used by
   `run_adoption`, `run_sync` and the NDJSON job; interactive edits keep `write_to`.
3. **Deferred indexes** (0013 §9 bootstrap mode): load `log.record` and the view tables without
   secondary indexes, build them after. Building `term_prefix` over a billion rows is itself hours.
4. **Set-based projections for bootstrap.** Resolution as one pass over the loaded log instead of
   117M round trips; `entity_source`, `activity` and `keyed_map` as one `INSERT … SELECT` each.
5. **`sync` plumbing:** stream instead of collecting a `Vec`, and skip `overlay_tenants` in bootstrap.

**Next, in order:** profile the client on the 10k slice; overlap parsing and writing; decide the
mirror-term question as an amendment to 0013/0014; then `write_many`, deferred indexes and set-based
projections as the stage 3 work, measured against the 1M slice of [`real-dumps.md`](real-dumps.md) §3.2.

## 7. The hidden set (read side)

Every read on a tenant with confidential targets starts by computing what the caller may not see
(09 §5.3; 0056 A19): one query over the tenant's confidential ACLs, then the subpages and threads
they enclose. `tools/hidden_set_bench.py` measures it: an outsider (a non-empty set, recomputed
every request) and a member of the group (an empty set, remembered by `pages::NothingHidden`
until the ACL frontier moves), at 0, 200 and 1,000 confidential pages, p50/p90 of 20 requests
after a warm-up, on a release build in the cloud sandbox (2026-10-24, `BENCH=1 BIN=target/release
tools/acl_check.sh`).

| Confidential pages | Probe | Outsider ms | Member ms |
|---|---|---|---|
| 0 | `prop=info`, one public page | 5.9 / 7.9 | 9.4 / 11.1 |
| 0 | `list=recentchanges`, 50 | 6.9 / 9.7 | 13.8 / 18.2 |
| 0 | `list=allpages`, 50 | 5.9 / 6.7 | 10.7 / 12.1 |
| 200 | `prop=info` | 10.1 / 13.1 | 12.3 / 16.1 |
| 200 | `list=recentchanges` | 11.8 / 13.7 | 15.3 / 17.2 |
| 200 | `list=allpages` | 10.7 / 12.3 | 11.4 / 14.8 |
| 1,000 | `prop=info` | 13.8 / 17.1 | 13.2 / 17.4 |
| 1,000 | `list=recentchanges` | 19.6 / 21.4 | 22.6 / 28.5 |
| 1,000 | `list=allpages` | 15.7 / 21.3 | 12.6 / 18.4 |

The member's baseline is higher because a logged-in request evaluates the tenant's groups; the
outsider here is anonymous. The set itself costs about 7 ms at 1,000 pages (1,000 ACL rows
decoded, then two indexed queries), linear in the number of confidential targets.

**Two fixes the measurement found**, both in the first run (1,000 pages: 160–220 ms per
outsider request, 90–170 ms per member `recentchanges`):

1. **The subpage query scanned every page per hidden root.** `left(p.title, length(r.title) + 1)
   = r.title || '/'` cannot use an index, so 1,000 roots × 1,000 pages was 150 ms. It is now a
   range of the title index, `Root/` up to the character after `/`, which is a prefix only in
   byte order: migration `0020_page_title_bytes` adds `view.page (tenant, ns, title COLLATE "C")`
   and the query compares under `COLLATE "C"`. 3 ms at 1,000 roots.
2. **The feed's page lateral cast the key** (`x.page_id::text = a.target_id`), which scanned
   `view.page` for every activity row the feed considered; the comparison now casts the row's
   target instead, behind a digits guard, and uses the primary key. This was not an ACL cost at
   all: every `recentchanges` paid it in proportion to the page count.

The debug-build figures in the implementation backlog (12→34 ms, 20→58 ms at 200 pages) predate
both fixes. **Still open:** a non-empty set is recomputed on every request; caching it needs a
key that also moves with page and thread writes under hidden targets (the pages frontier beside
the ACL frontier), which is the next step if a tenant with thousands of confidential pages
appears. The feed also computes its lateral joins before sorting and limiting when the planner
scans `view.activity` sequentially (a small table); on a large one it walks the time index.

## 8. The 2M Wikidata baseline (2026-11-04)

**What ran.** `sync --provider wikidata` of `wd-bench-2m.json.gz` (sha256 `4ceed74c…`), at `main`
`26f7754`, into a fresh database on a 16-core, 62 GB Debian 13 VM with Postgres 17 tuned per §4
(`shared_buffers` 16 GB, `max_wal_size` 16 GB, `checkpoint_timeout` 30 min, `wal_compression`
zstd). The set is every property (14,017), the first million items that exist (Q1–Q1084808) and a
seeded sample of a million scholarly articles (P31 = Q13442814, seed 1, from 46,010,359), all from
the 2026-10-26 dump, cut with `tools/wikidata_zindex.py` (real-dumps.md §3.2): 54.8 GB of JSON,
27 KB per entity.

**Result.** 2,014,017 created, 0 rejected, 0 unreadable, 0 hash mismatches, in 9 h 28 min
(59 entities/s: 54/s over the properties and dense early items, 63/s over the articles). Peak
RSS 406 MB, flat in the dump's size since sync streams. Database 39 GB: `view` 26 GB, of which
`view.term` 18 GB and `view.identifier` 4.3 GB; `log` 13 GB.

**Where the time goes, per entity:**

| | ms | share of wall |
|---|---|---|
| Wall clock | 16.9 | 100% |
| Client CPU (user + sys) | 11.8 | **70%** |
| Postgres execution | 2.2 | 13% |
| … `INSERT INTO view.term` alone | 1.3 | 8% |
| Postgres planning | 0.3 | 2% |
| Round trips and the rest | ~2.6 | 15% |

20.0 statements per entity. Sampled over 30 s mid-run, the client used 78% of a core and its
backend 18%, active in 9 of 30 one-second samples. **On Wikidata-sized entities the load is bound
by client CPU, not by round trips**: the reverse of Librarybase (§2), whose entities are a tenth
the size.

**Where the client's CPU goes** (callgrind over 1,000 dense items and 1,000 articles, 54M
instructions per entity; shares overlap where functions nest):

| | share |
|---|---|
| Decoding the record just appended, once in each of five projections (`common::operation` from `activity`, `entities` ×2, `sources` ×2) | **44%** |
| … of which re-encoding the body to check it is canonical CBOR (`cbor::decode_prefix` → `cbor::write`, 10 per entity) | 19% |
| … of which deserializing it into an `Operation` | 13% |
| Parsing the dump line (`ParsedEntity::deserialize`) | 12% |
| The entity-source and activity projections' own work | ~10% each |
| Canonical JSON (`Resolved::equals`, sizes) | 7% |
| Encoding the record (`Value::from_json`, body parts) | 8% |

`malloc` and `free` are about a third of all instructions, spread over the rows. `perf` is not
usable on Debian without lowering `kernel.perf_event_paranoid` (3 by default); callgrind on a
binary built with `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only` needs no privileges.

**What this changes in §6.** Overlapping parsing and writing (§6 item 1) is worth at most the 12%
parse share here. Decoding each record once per write and handing the `Operation` to every
projection removes four of the five decodes, about a third of client CPU, with no change to what
is verified (one canonical check still runs). `view.term` is 46% of the database and half of
Postgres's execution time: the mirror-term question of §6 is now measured, not projected. At
59/s the full dump (122M entities) is about 24 days and `view.term` about 1.1 TB.

**Decoding once (2026-11-04).** `common::operation` now keeps the last record it decoded per
thread, with a copy of its content bytes, and returns it again (cloned) for equal bytes: the five
projections and the entity projection's read-back of the record share one decode and one canonical
check. Against `26f7754` on the same slices: instructions per 2k-entity profile 108.7 G → 71.8 G
(−34%); on 10,000 dense items and 10,000 articles, client CPU 187.6 s → 128.3 s (−32%) and wall
273 s → 217 s (73 → 92 entities/s, +26%). The next client costs, in the new profile: entity
resolution (`resolve_subject`) 31%, the entity-source projection 18%, SHA-256 for commitments 13%,
and `Resolved::equals` by canonical JSON 12%.

**The term-language rule (2026-11-04).** 05 §5.8 (0070 §6, settled for full mirrors by 0082 §6)
gives a mirrored entity `view.term` rows in the instance's term languages only; the baseline wrote
every language (110.8M rows in 629 languages for 2M entities). The entity projection now filters
shared rows (`tenant = ''`) inside the term insert, to the `tenancy:entities.term_languages`
ceiling or by default `mul`, `en` and every tenant's `site.language` (0082 A2). On the same 20k
slice, two runs each, alternating, against decode-once alone: term rows 811k → 50k (−94%),
`view.term` 127 MB → 9.7 MB, database 318 MB → 200 MB (−37%); Postgres execution 23 s → 15.5 s,
term inserts 9.7 s → 2.2 s; wall 225 s → 191 s (89 → 105 entities/s, +18%); client CPU unchanged.
With decoding once, 73 → 105 entities/s (+44%) on this slice against `26f7754`. A mirrored
entity's labels in other languages are in its record (`wbgetentities`) but no longer in
`view.term`, so a page shows its ID in those languages until the label cache (0070 §4, 05 §5.6)
is built.

**The state comparison and loads' activity rows (2026-11-04).** `Resolved::equals` turned
both states into canonical JSON to compare them, and the projection then built the resolved
state's a third time; it now builds it once and compares the contribution against it
(`equals_given`), which stays exact (a structural comparison would not be: `StatementId`
compares without case and `-0.0 == 0.0`). Instructions on the 2k profile 71.8 G → 67.4 G (−6.2%).
A load's mirror records no longer write `view.activity` rows (03 §4.6; 0013 §5.5): a mirror
change-set record whose job is not marked `follow` is skipped by the activity projection, so a
load is its job's rows alone, while the follower's records keep theirs. On the 20k slice the two
together, against the term rule alone: wall 191 s → 181 s (105 → 110 entities/s), client CPU
about 142 s → 132 s, activity rows 20,003 → 3, database 200 MB → 175 MB. **Against `26f7754` on
this slice, the four changes of this section take 273 s to 181 s (73 → 110 entities/s, +51%).**

**The allocator, measured (2026-11-04).** `malloc` and `free` were about a third of the client's
instructions. The same binary (all the changes above) under glibc, jemalloc 5.3.0 and mimalloc
3.0.1 by `LD_PRELOAD` (Debian's `libjemalloc2`, `libmimalloc3`), on the 20k slice, alternating,
two runs each: CPU 130 s / 104 s (−20%) / 96 s (−26%); wall 185 s / 169 s / 156 s (−9% / −16%;
runs vary by ±6%); peak RSS 49 MB / 128 MB / 85 MB. mimalloc is now the binaries' allocator
(0033 A24; 22 §4.2).

**The 2M set again, at `867c673` (2026-11-05).** Same file, settings and machine as the
baseline: 2,014,017 created, 0 rejected, 0 unreadable, 0 hash mismatches, in **5 h 18 min
(105 entities/s), against 9 h 28 min (59/s): −44% wall, +78% throughput**. Client CPU 12,653 s
against 23,818 (−47%); Postgres execution 1,862 s against 4,463 (−58%), still 20.0 statements
per entity (the job lookup replaced the activity insert); database 19 GB against 39 GB, with
`view.term` 5.1M rows and 1.1 GB against 110.8M and 18 GB, and `view.activity` 3 rows. Unlike
the baseline, the articles went slower than the dense items (99/s against 110/s). Per entity:
9.5 ms wall, of which client CPU 6.3 ms (66%), Postgres execution 0.9 ms (10%), round trips
and the rest about 2.3 ms (24%). The client is still the larger share, so preparing entities
on worker threads (§6 item 1, for parsing, ID rewriting, hash checks and CBOR encoding) and
then batch-shaped writes (§6 items 2–4) are next.

**Bootstrap mode, stage A (2026-11-05).** 0013 §9 for an empty instance mirror without a key
map and a JSON dump (A70; 05 §3.8): each record is a new subject's `put`, so it is prepared
from the dump alone on worker threads (parsing, the hash check, the operation, its encoding,
the part hashes), and one coordinator gives offsets and page IDs in dump order and appends a
block at a time by binary `COPY` of the records and the tree nodes (`append_many`), with one
partition lock, one page-ID reservation and one commit per block. The projections still run
per record, inline, so the key index stays; set-based projections and the deferred index build
are stage B. On the 20k slice: 178 s → 89 s (112 → 225 entities/s), with the same records,
tree and view rows as the ordinary sync and a rebuild identical to the load. **On the 2M set:
2 h 56 min (190 entities/s), against 5 h 18 min at `867c673` and 9 h 28 min at `26f7754`
(3.2×)**: 159/s over the dense items and 244/s over the articles; 11.0 statements per entity
against 20.0, Postgres execution 1,456 s against 1,862, the database the same 19 GB. Peak RSS
rose to 1.44 GB (386 MB in the ordinary sync; 658 MB on the 20k slice), not yet explained: the
queues between the threads and the coordinator are bounded, so the growth with the load's size
is worth a look before a 122M-entity load. What is left is the per-record projection work on the coordinator's thread
(the client was at about 110% of one core at the end): stage B's set-based projections.

**Bootstrap mode, set-based projections (2026-11-05).** The projections run over each block at
once (`Projection::apply_many`, `Pipeline::apply_inline_many`; 0013 A71): the entity source is
one multi-row upsert per block; a mirror's `put` of a subject no other graph or overlay holds is
resolved from its own state on worker threads, and its entity rows, terms (in the instance's term
languages) and identifiers are written with one statement per table; activity looks a load's job
up once per block. Any other record goes record by record. On the 20k slice: 89 s → 62 s
(225 → 321 entities/s), the same rows as the ordinary sync, a rebuild identical. **On the 2M set:
1 h 44 min (321 entities/s), against 2 h 56 min with projections record by record, and 9 h 28 min
at `26f7754` (5.4×)**: 278/s over the dense items, 358/s over the articles; 0.13 statements per
entity, Postgres execution 719 s; every view row the same as the record-by-record load's. The
client was at about 180% of one core and Postgres at about 12%: the coordinator's thread (the
append, the block's encoding for `COPY`, the rows' assembly) is the limit now. Peak RSS was
1.75 GB; the next paragraph explains it.

**A bootstrap load's memory (2026-11-05).** Peak RSS does not grow with the length of a load: it
is reached in the first minute and follows the size of the entities then in flight. RSS sampled
every 5 s over 300k entities: the dense start of the 2M file (38 KB a line) peaked at 1,744 MB
within a minute, then held 600–1,000 MB and ended near 300 MB; the articles at its end (16 KB)
peaked at 853 MB, then fell to 140–340 MB. The 2M run's 1.75 GB is the first peak: the dump
opens with Wikidata's largest items (Q1 onwards average 160 KB of JSON; record bodies there
average 76 KB, the largest 1.9 MB). On the first 60k lines: mimalloc as shipped 1,641 MB;
`MIMALLOC_PURGE_DELAY=0` 1,430 MB; the system allocator 1,580 MB, flat at about 1.5 GB to the
end and 38% slower. So the peak is live data, which mimalloc's delayed purge adds about an
eighth to and returns as the load goes on. massif on the first 2,000 items put 69% of the peak
in prepared record bodies waiting between the workers and the coordinator, 8% in copies of the
block's records made per projection, 7% in entities decoded for the block's projections. The
bodies piled up because the collector's reorder buffer was unbounded: while the next unit in
order was still being prepared (a unit holding megabyte-sized items), the collector kept taking
later units into it, freeing the channels for the other workers; halving the channels lowered
the steady state by a quarter but the peak by 7%. Three changes: the units in flight anywhere
between the reader and the coordinator are bounded (`UNITS_PER_WORKER`, 3 per worker: the
reader takes a permit per unit, the collector returns it when it takes the unit up); an encoded
part is trimmed to its length (`Part::of`: the encoder's buffer grows by doubling); a projection
that accepts every record of a block takes the block without a copy (`apply_inline_many`). On
the same 60k lines: peak 1,641 → 1,286 and 1,231 MB (two runs), steady state 900–1,300 → 600–800
MB, time 472 s → 515 s and 474 s (runs vary by about 6%). What remains of the peak is a window
of 42 units of the largest items, the projections' working set for a block of them, and
mimalloc's retention; bounding the window by bytes rather than units would take it lower still.

**Each block decoded once (2026-11-05).** callgrind per thread over a 2,000-item bootstrap load:
88% of the coordinator's instructions were `common::operation`, decoding each record's content
again for every projection. The memo kept only the last record decoded, and a block is applied a
projection at a time, so every projection after the first missed it. The memo is now a cache of
decoded operations per thread, keyed by the content bytes (4,096 operations or 64 MiB, cleared
when full), and the pipeline runs a prelude over each block before its first projection
(`Pipeline::before_block`), which the projections' pipeline sets to `common::decode_block`: the
block's operations decoded on every core. The workers' own cost (JSON to CBOR, canonical JSON)
is spread over 14 threads and is not the limit. On the 20k slice: 62 s → 38 s (321 → 532
entities/s); every compared row the same as the ordinary sync, a rebuild identical; the ordinary
sync on the same run took 149 s (178 s before, not rerun to separate the cause). **On the 2M set:
49 min 15 s (681 entities/s), against 1 h 44 min (2.1×) and the 9 h 28 min baseline (11.5×)**;
the same row counts in `view.entity`, `view.term`, `view.identifier` and `view.entity_source`
as the previous load; peak RSS 1.68 GB; client CPU 9,525 s, about 310% of one core, with
Postgres at 83–94% while the dense items loaded and about 30% after. From the 5-minute samples,
roughly 570 entities/s over the first half of the file and 850/s over the rest.

**The second profile: hashes, statistics, commits (2026-11-05).** callgrind again on the 2,000
items: the coordinator now 9.5% of the client's instructions (it was the larger part), and of
those, 54% SHA-256 of each record's content for `view.entity_source.content_hash`, on the
coordinator and in software (this CPU has no SHA extensions). The hash is now computed with the
block's decoding on every core and kept beside the decoded operation (`common::content_hash`).
Per-thread CPU over a real load of the 20k slice then showed no thread near one core (the
coordinator 19% of wall, each worker 8%) and Postgres's backend at 61%: the load was waiting on
Postgres. `pg_stat_statements`: three-quarters of its time in the block's `DELETE FROM view.term`
and `view.identifier` `… entity_id = ANY(…)`, which delete nothing in a load. `auto_explain`
showed why: with no statistics on a table growing from empty, the planner served the term delete
through `term_prefix`, an index that narrows by tenant only, and rechecked every term row
written so far, about 180 ms a block by the end of 20k entities and growing with the table. A
bootstrap load now refreshes the planner's statistics on the `log` and `view` tables each time
its block count doubles (0013 A73; about 4 s each at 2M entities). On the 20k slice Postgres
execution fell from 19.4 s to 6.1 s, and the load from 38 s to about 20–25 s; the parallel work
(`common::par_map`) now hands records out one at a time rather than in fixed shares, which
made little difference (the decoding prelude 3.6 s → 3.1 s). Timing the coordinator's phases on
the slice: projections 13–14 s (entity rows 7.0, the decoding prelude 3.1, entity source 1.8,
keyed map 1.2), append 2.2 s, waiting for prepared records 0.2 s, ANALYZE 1.1 s, and commits
1.2 to 9.9 s from run to run: each block's commit waited for its write-ahead log on disk. **On
the 2M set: 40 min 56 s (820 entities/s)**, the same row counts; peak RSS 1.81 GB (1.68 GB
before; the decoding cache's copies of content bytes, up to 64 MiB, are the likeliest part);
client CPU 9,954 s, about 400% of one core; Postgres about 35% after the first minutes, against
83–94% early in the previous run. A block's commit now does not wait for the disk (0013 A72,
James's decision): `synchronous_commit` off for each block's transaction, on for the last, which
finishes the job and so makes the load durable once it returns. On the 20k slice: 16, 25 and
23 s in three runs, the rows the same as the ordinary sync's and a rebuild identical; the disk's
variance makes the slice too short to measure it. **On the 2M set: 39 min 14 s (856 entities/s)**
against 40 min 56 s (4%), the same row counts, peak RSS 1.77 GB; 14.5× the 9 h 28 min baseline.

**The coordinator's own work (2026-11-06).** callgrind on the 2,000 items, the coordinator's
thread alone: 39% SipHash, hashing every byte of a record's content on every lookup in the
decoding cache, which was keyed by the bytes; 24% cloning a decoded operation out of the cache
for each projection that read it; 14% `memcpy`. The cache now files content by a fingerprint
(its length and three 32-byte windows) and confirms a hit by comparing the bytes whole, so it
still never answers wrongly, and keeps each operation behind an `Arc`, which projections share
(`common::operation` returns `Arc<Operation>`); the block path clones an entity only on the
threads that build its rows. The coordinator then used 2.7 s of CPU over a 20k load. On the 20k
slice: 14 s against 16 s, the rows the same as the ordinary sync's and a rebuild identical. **On
the 2M set: 31 min 51 s (1,054 entities/s)** against 39 min 14 s, the same row counts, peak RSS
1.81 GB, client CPU 9,039 s; 17.8× the baseline. The client stayed under five of its cores and
Postgres under half of one: each block still waits on its writes before the next block's
projections start.

**The projections' work moved to the preparing threads (2026-11-06).** Timing the coordinator's
phases over a 20k load (14 s): the block's decoding 3.0 s and the entity rows' computation
2.0 s, both spread over every core while the coordinator waited, against about 6.5 s of
statements (the entity rows 3.5, the append 2.3, the entity source 0.45) and 0.2 s waiting for
prepared records; the preparing threads were mostly idle. Those threads now hand each record's
operation, with the body it was encoded into, to the projections (`scatter_ingest::Warm`,
`common::warm`; 0013 A74), which keep it, its content hash and a mirror entity's rows but for
its page ID, in a cache shared by the process and forgetting its oldest entries first; the
coordinator then decodes and resolves nothing. On the 20k slice: 8 s against 14 s, the records,
Merkle nodes and views the same as the ordinary sync's and a rebuild identical; peak RSS
1.33 GB against 0.78 GB, the work done ahead being held until its block is applied. **On the
2M set: 24 min 11 s (1,389 entities/s)** against 31 min 51 s, the same row counts, peak RSS
2.35 GB against 1.81 GB, client CPU 8,546 s, the client at 510–630% and Postgres about 50%;
23.5× the 9 h 28 min baseline.

**The ordinary sync, profiled (2026-11-06).** The 20k slice through `sync --no-bootstrap`: 148 s
(135 entities/s). The client's main thread 85 s of CPU, its runtime threads about 10 s each,
Postgres's backends 41 s; 403,336 statements, about 20 an entity, one round trip at a time, of
which execution was 13 s and planning about 5 s (`pg_stat_statements`); 1,396 commits. No
planner trouble: the entity's term and identifier deletes are point lookups, 0.14 s in all.
callgrind of the client on the 2,000 items: everything on one thread, and about 40% of it
repeated work: each record's part hashes computed three times (sealed in the store's append,
sealed again by the write path, and checked again when the entity projection read the record
back by offset), the projections decoding the operation the write path had just encoded, and
each entity's canonical JSON built three times (its size, the resolved state, the comparison
with it). SHA-256 alone was 20% of the client. The store now returns the record it sealed
(`IngestStore::append_sealed`), the write path hands the projections the operation it encoded
(`IngestStore::encoded`), and the entity projection takes the record being applied as it is
(0013 A75). The canonical JSON is still built three times: removing the third means carrying
it from the sync through the write path, for about 5% of the client. The client's main
thread then used 65 s of CPU and the load took 137–140 s; the ordinary sync's views the same
as a bootstrap load's, a rebuild identical. What is left is mostly the round trips.

**The ordinary sync in blocks (2026-11-06).** A sync now reads, decides and writes each unit
of `--batch` items (200) as a block when the unit is of the common kind (0013 A76): its
cursors, the records at them, its keys' claimed revisions and page IDs in one statement each,
the items decided in memory by the item-by-item path's own functions, the records appended by
binary `COPY` and projected by the set-based block path. Checked against the item-by-item path
(`--batch 1`) on the 20k slice and on an update pass over it (`ab-20k-v2`: every tenth
entity's English label changed at a higher revision, 2,000 entities of the 2M set added, by
a local script, as is the comparison): the same records, offsets,
page and revision IDs, Merkle nodes, views and job counts, and a rebuild identical.

| 20k slice, ordinary sync | item by item, `--batch 1` | item by item, 200 (step 1) | blocks of 200 |
|---|---|---|---|
| first load | 241 s | 137–140 s | **67 s** |
| update pass: 2,000 changed, 2,000 new, 18,000 unchanged | 108 s | | **61 s** |

`--batch 1` also commits after every item, so the fair comparison for the first load is the
step-1 figure: about twice as fast. The update pass is mostly the 18,000 unchanged states,
each decoded from its previous record and compared by canonical JSON on the client's one
thread (52 s of CPU): that, and the parsing, is the parallel preparation of step 3.

**The ordinary sync's per-item work on every core (2026-11-06).** callgrind of the client on
the update pass of a 2,000-item slice (`prof-2k-v2`): 29 G instructions on the main thread, of
which deciding 54% (decoding previous records 23%, the unchanged test's canonical JSON 19%,
checking the records read 8%), encoding the new records 18%, and parsing and the adapter about
28%; all of it per item. A JSON dump of a provider with no key map is now parsed and adapted on
worker threads (the reader and workers of bootstrap mode, made generic), and within a block
the states are reconciled, decoded, decided and encoded on every core (`par_map`, now in
`scatter-projection`; 0013 A77).

| 20k slice, ordinary sync | step 1 | step 2 (blocks) | step 3 (every core) |
|---|---|---|---|
| first load | 137–140 s | 67 s | **39 s** |
| update pass | | 61 s | **13 s** |

The first load: client CPU 86 s over 39 s, so about two cores busy on average; the update
pass 62 s of CPU in 13 s. Records, Merkle nodes, views and job counts the same as writing item
by item (`--batch 1`), a rebuild identical. From 148 s at the start of this work, the first
load is 3.8× faster.

**`term_prefix` is not yet as 03 §4.3 has it.** The chapter's index is partial over tenant rows
(`WHERE tenant <> ''`); the migration's (`0002_view.sql`) covers every row, shared ones included,
and `wbsearchentities` reads shared rows through it (`t.tenant IN ($1, '')`). Making it partial
before the search index (0014) serves mirror search would leave that half of the query to a
sequential scan, so it stays as built; with the term-language rule the shared part is small.

**A killed load resumes (2026-11-06).** A 2M bootstrap load (`b4ab619`) killed with `kill -9`
at 1,013,400 entities left a partition of exactly that size, with no holes. The same
`triplespace sync` command, rerun, went through the ordinary sync: 1,013,400 unchanged and
1,000,617 created in 24m08s. Record keys and revids, `view.entity` (but page IDs), `view.term`,
`view.identifier` and `view.entity_source` (but offsets and times) equal the uninterrupted load's;
every entity has its own page ID. The killed load's job stayed `running`, which is what 0011 A36
and A37 fix.

**A uniform 10M sample (2026-11-06).** Every property and 10,000,000 other entities drawn
uniformly from the 2026-10-26 dump (`wikidata_zindex.py ids --sample 10000000 --seed 1`),
10,014,017 entities, 17.6 GB gzip (`wd-sample-10m.json.gz`, sha256 `dfc78356…`), loaded in
bootstrap mode by `b4ab619`:

| | |
|---|---|
| wall | 1h21m02s (2,060 entities/s); test builds and runs shared the machine from about 07:26 to 08:31 |
| rate per 5 minutes | 1,300–3,700/s, with no downward trend as the database grew from 5 to 56 GB |
| client | 23,442 s user CPU, 4.8 cores on average; peak RSS 1.61 GB |
| database | 56 GB, 5,963 bytes per entity |
| largest relations | the log partition 37 GB (4.0 KB per entity), `view.identifier` 6.3 GB, `view.term` 4.1 GB, Merkle nodes 3.2 GB, `view.entity_source` 2.7 GB, `view.entity` 2.5 GB |
| rejected | 0 |

The sample's entities are half the 2M set's size (about 15 KB of JSON against 27 KB), and its
rate per 5 minutes varies with what is in each stretch, so the rate is not one number.

**What this predicts for the whole dump** (122,056,985 entities), scaling linearly: about
**730 GB** of database and about **16 hours** at the sample's rate. Neither is certain at twelve
times the size: the indexes outgrow the 62 GB of memory, which the sample, at 56 GB in all,
did not test, and the Merkle nodes grow a little faster than linearly. The disk is the
constraint: 551 GB is free with the benchmark databases in place, about 660 GB without them,
and the dump itself (the `.bz2`, 104 GB) has to stay while it loads.

**The whole dump (2026-11-06/07).** The 2026-10-26 dump, every one of its 122,056,985
entities in the dump's own order (properties scattered through it), recompressed to gzip from
the zindex archive (`wd-full.json.gz`, 211 GB, sha256 `d277f32a…`; gotchas.md on why not the
`.bz2`), loaded in bootstrap mode by `a726543` on the same VM grown to 125 GB of RAM and
2.5 TB of disk, with `shared_buffers` 32 GB and `effective_cache_size` 96 GB (§4's R/4 and 3R/4):

| | |
|---|---|
| wall | **25h25m15s** (1,334 entities/s) |
| loaded | 122,056,983 created, 0 rejected, 2 unreadable (statement IDs whose UUID part is not hexadecimal, which Wikibase allows; loaded since, 0017 A13) |
| rate per hour | 1,970–2,800/s for the first six hours (to about 290 GB); 670–1,480/s after, ending at 745/s |
| client | 326,015 s user CPU, 3.6 cores on average (up to 8 in dense stretches); peak RSS 1.77 GB |
| Postgres | execution **78,424 s, 86% of the wall time**; planning 31 s; 18.1M statements, 0.15 an entity |
| database | 712 GB (663 GiB), 5,832 bytes per entity; indexes 112 GB |
| largest relations | the log partition 451 GB (3.7 KB per entity), `view.identifier` 72 GB, `view.term` 46 GB, Merkle nodes 38 GB, `view.entity_source` 28 GB, `view.entity` 28 GB |

| statement (per block of 200) | calls | execution | per call |
|---|---|---|---|
| terms (`WITH term_ceiling … INSERT INTO view.term`) | 606,637 | 24,864 s | 41 ms |
| `INSERT INTO view.identifier` | 603,316 | 21,907 s | 36 ms |
| `COPY log.record` | 606,636 | 16,317 s | 27 ms |
| `INSERT INTO view.entity_source` | 606,636 | 5,461 s | 9.0 ms |
| `INSERT INTO view.entity` | 606,637 | 4,750 s | 7.8 ms |
| `COPY log.merkle_node` | 606,636 | 1,903 s | 3.1 ms |

The 10M sample's projection held for size (730 GB predicted, 712 GB) but not for time (16 h
predicted, 25.4 h). Two things changed past the sample's scale, and the run cannot separate
them cleanly:

- **The dump is not uniform.** Long stretches hold larger entities than the sample's average
  (at 65M entities, 3.1 KB of gzip each against 1.77 KB over the run so far), and the rate
  follows them: the dip to 466/s at 22:14 recovered to 2,500/s within the hour.
- **The writes became the critical path.** On the 2M set Postgres was under half a core and the
  client the bound; here Postgres execution is 86% of the wall time, and since the coordinator
  applies one block at a time on one connection, almost all of the run waited on it. Postgres
  used 60–75% of one core throughout, so most of each statement's time was reading index pages:
  once the database passed RAM (about 20:00, three hours in), it read 14–35 MB/s from disk
  steadily, with 4–7% iowait. The index-heavy inserts, terms and identifiers, are 60% of it.

What would most likely shorten the next run, in order (each unmeasured): applying a block's
writes on a second connection while the next block's projections run (the coordinator is idle
while it waits); building `view.term`'s and `view.identifier`'s secondary indexes after the
load, as 0013 §9's deferred indexes intend; and sorting each block's term and identifier rows
by their index keys before inserting, to make the index writes less random.

Keep `ts_full` (712 GB) until the follower has been run against it from the dump's date
(`triplespace follow --provider wikidata --from 2026-10-27T11:54:55Z`, where the load ended):
that is the next measurement this database makes possible.

**Several blocks at once (2026-11-07/08).** The first of the steps above, measured. Applying a
block's writes on a second connection gains nothing by itself: the append locks the
partition's row, and the next block waits on it. So the load writes several blocks at once,
each in a transaction of its own, and commits them in dump order (0013 A82, A83): the
coordinator gives each block its page IDs, offsets and tree nodes, the frontier kept in
memory, and a block moves the partition's next offset, and commits, only after the block
before it has. Prototyped first and measured on the same VM and dump as the whole-dump load:

- **To 40M entities with 4 writers: 4 h 00 min, against 5 h 07 min** for the whole-dump load's
  first 40M (1.28×). By stretch, 2,897/s against 2,517/s up to 10M, 2,974 against 2,541 to
  20M, 3,086 against 2,271 to 30M and 2,304 against 1,645 to 40M: the gain grows once the
  database outgrows memory. Killed at 40,051,000 entities, the database held exactly that many
  records, at offsets 0 to 40,050,999, as many leaves, and every projection at the same
  position. The whole-dump load was an older build (`a726543`), so this compares builds as well
  as writers; the sweep below does not.
- **A sweep, 40 minutes each from empty, one build:** 4.81M entities with 1 writer, 5.96M with
  2 (1.24×), **7.15M with 4 (1.49×)** and 6.57M with 8 (1.37×). With 2 writers and more, the 14
  threads preparing records ran at 93–98% each through dense stretches, the coordinator's own
  thread at 20–39% of a core, and I/O wait at 0–2%; Postgres's connections, sampled every 2 s,
  were waiting for the client's next statement (`Client:ClientRead`) in about 5% of samples
  with 1 writer, 25% with 2, 50% with 4 and 75% with 8. Preparing the records is now the limit,
  so more writers only add connections: 4 is the default.

`pg_stat_statements` over several connections at once adds up to more execution time than the
wall time, so its totals no longer say what share of a load waits on Postgres; per-thread CPU
and wait-event samples do. What would shorten the next run is the preparation's CPU per
record, not more writers.

**Preparing a record with less CPU (2026-11-08/09).** callgrind per thread over the first
2,000 items of a dense stretch, a bootstrap load: the preparing threads spent 28.4G
instructions in `Prepare::item`. Of that, `draft` was 23.9G and, within it, the projections'
warm-up 11.5G; the entity's canonical JSON 8.2G, built three times per record (once for the
size, once for the projections' check against the given JSON, once for their rows); SHA-256
5.8G (this VM's CPU has no SHA extensions, so `sha2` runs in software); converting the body to
CBOR values 3.5G; parsing the dump's line 2.3G; the Wikidata adapter 1.8G. Changed, each with
the same bytes out:

- the canonical JSON is built once and handed to the warm-up, which compares against it instead
  of building it again (a debug assertion checks they agree);
- canonical JSON writes into one buffer, escaping each string straight into it rather than
  through a `String` per key and value;
- CBOR maps encode their keys into one shared buffer and their values straight into the output,
  not each pair into its own; `Value::from_json_owned` moves strings out of the JSON value
  instead of cloning them;
- the shared cache of decoded records drops what it forgets after releasing its lock. Stack
  samples past 7M entities had 8 of 14 preparing threads and the coordinator waiting on that
  lock while one thread freed an evicted entity under it.

`Prepare::item` fell to 22.7G on the dense 2,000 (−20%) and from 29.2G to 23.6G on the mixed
2,000. On `mem-dense-60k`, three runs of each, alternating, 4 writers, fresh databases: **user
CPU 722 s → 594 s (−18%), wall 70.6 s → 61.0 s (−14%)**. The 20k slice loaded by both builds
gives the same records, Merkle nodes, body lengths, projection positions and view rows, and a
rebuild of the new load's views is identical.

40-minute sweeps over the whole dump, 4 writers, do not resolve a gain of this size: `main`
reached 10.24M one evening and 7.32M at 01:00 the next night, on the same build and machine,
and the new build 10.95M, 10.77M (with stack samples and a build running alongside it) and
9.99M. The same-evening pair is 10.24M against 10.95M (+7%). Use alternating short runs, or
CPU counts, to compare builds here; a sweep needs repeats on both sides.

Still in the preparation, measured but not changed: SHA-256 in software (the `sha2` crate's
`asm` feature would add a dependency, rule 7; production CPUs with SHA extensions make it moot),
building CBOR values from JSON rather than serializing straight to CBOR (one encoding per
hashed type, rule 4, would need a careful argument), `Changes::between` and resolving the
entity, parsing (about 7%) and the adapter (about 6%). In the sweeps' samples, threads named
`tokio-rt-worker` running at 60–90% while the runtime's own workers were parked: on Linux a
thread left unnamed keeps its creator's name, so those were unnamed threads started from a
runtime worker. Name them before reading per-thread figures from a load.

**Following from the whole-dump mirror (2026-11-09/10).** `follow --provider wikidata --once
--anonymous` on the whole-dump database (§6 row in `real-dumps.md`), from the dump's end at
2026-10-27 11:54:55: anonymous, so 50 entities per `wbgetentities` request, one request at a
time. Two runs, at `bd52604` plus 0092 A3–A5:

- 18h38m, 257 h of Wikidata's edits (10-27 11:42 to 11-07 05:05), +2,307,745 records, about
  14× Wikidata's edit rate. It stopped on a redirect that borrowed a revision a put already held
  (0092 A5). Client CPU 17,785 s (27% of a core), peak RSS 1.74 GB.
- Resumed with the fix: 8h58m, the remaining 74 h to the present (lag 0.0 h at the end), 1,680,972
  change rows in 3,361 batches: 24,556 created, 869,708 updated, 266,213 unchanged, 26,886
  rejected, 256 tombstoned, 1,422 redirected; +895,945 records, about 8× the edit rate. Client
  CPU 7,115 s (22% of a core), peak RSS 1.21 GB.

The database grew 663 → 686 GB (about 7 KB per record). The limit is the serial fetch, not the
client or Postgres: the client stays near a quarter of a core, and the waits are reading pages
and, later, WAL writes (`LWLock:WALWrite`, `IO:WalSync`, iowait 8–13%). A bot login's
`apihighlimits` (500 per request) or parallel requests within Wikimedia's limits would raise it.
The first run's rejections were all lexemes, whose forms and senses the adapter does not read yet (0092 A3); each
batch is its own job, about 8,000 over the two runs. Before running a newer build against an
older benchmark database, run `triplespace migrate`.

**The follower reads ahead (2026-11-11).** `run_follow` reads the next batch while it writes the
current one: still one request upstream at a time, batches written in order, each position saved
after its own batch's job. On the whole-dump mirror, from 16 h behind to caught up, anonymous,
alternating 12-minute segments of the two builds (`main` with 0092 A3–A5, with and without the
read-ahead):

| Build | Segments | Batches per minute | Entities per minute | Wikidata time per wall time |
|---|---|---|---|---|
| serial | 4 | 5.0, 5.9, 6.0, 7.0 (mean 6.0) | 2,471 | 7.8–10.7× |
| read-ahead | 3 (and a last 2 minutes) | 8.8, 9.3, 9.4 (mean 9.2) | 3,718 | 14.4–17.4× |

**+53% batches per minute**; each read-ahead segment beat both serial segments beside it, by
1.34× to 1.76×. More than the write's share of a batch (about 30%) suggested: the read's parsing
and the write's waits on Postgres overlap too.
