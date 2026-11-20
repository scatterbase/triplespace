# 0092. Following a wiki by its recent changes; loading a source

- **Status:** Proposed
- **Date:** 2026-10-26
- **Updated:** 2026-11-20 (A11)
- **Author:** James Hare / Claude Opus
- **Uses:** [0011](0011-logs.md), [0022](0022-federation.md), [0024](0024-subsidiary-accounts.md), [0027](0027-preferences-and-portability.md), [0040](0040-instance-prerogatives.md), [0067](0067-proposals.md), [0085](0085-settings-and-preferences.md), [0088](0088-external-identity-providers.md), [0091](0091-one-wikibase-adapter.md)

## Context

[0070](0070-shallow-entity-mirroring.md) §5 keeps a mirrored set current in two ways:
- **An event stream.** For Wikidata, `mediawiki.recentchange` from Wikimedia's EventStreams, with gaps closed by paging `list=recentchanges`.
- **A daily sweep.** `wbgetentities&props=info` over every entity in the set.

[0078](0078-entity-sources.md) §4 gives a tenant's entity source the same job, following `events` "where the source declares them", and otherwise "the daily sweep alone". It also makes the fetch job the only writer of `source/{name}`, so a source can only be filled entity by entity.

Most Wikibases have no event stream: Wikibase Cloud, Miraheze, self-hosted wikis. For them the design has two gaps:
- **The sweep finds only what the set already holds.** Re-checking known entities never discovers a new item, and edits arrive up to a day late.
- **There is no bulk start.** A source cannot be loaded from a dump; it can only grow by fetches.

The case at hand is the Credit Unions Wiki on Miraheze. `docs/testing/demofarm.md` reads it on a tenant as the entity source `opencredit`: load its dump, then follow the live wiki to the present.

The Wikidata Query Service solved the same problem before its streaming updater. Its `RecentChangesPoller` (`wikidata-query-rdf`, `tools/.../change/`, last changed 2022-11) works as follows:
- **The query.** It polls `list=recentchanges` with `rcdir=newer`, `rcprop=title|ids|timestamp` and the entity namespaces. It starts from a timestamp, then follows `rccontinue`.
- **Dedupe and coalesce.** It dedupes by `rcid` against about an hour of seen IDs. It coalesces a batch to one change per title, keeping the highest revision; a deletion log row always wins.
- **Backing off near the present.** Within two minutes of now, it re-reads from ten seconds before its position instead of continuing. Rows can be committed late with earlier timestamps ([T161731](https://phabricator.wikimedia.org/T161731)), and a timestamp position is only safe if the last second is re-read and deduped ([T95194](https://phabricator.wikimedia.org/T95194)). An optional trailing poller re-reads a window behind the head for rows the head missed.
- **The entity decides.** For each title it fetches the entity: by revision when the change is recent, otherwise the latest. "Not found" means delete, so deletes, restores and edits share one path.
- **The starting point.** It starts from a stored position, and refuses one more than 30 days old ("reload from scratch").

### Direction

James's direction, from the design discussion of 2026-10-26:

- **"What if my goal was to test being able to sync against the live Credit Unions wiki on Miraheze? Does that require adoption on a local tenant? Adoption implies that Triplespace is taking over, rather than being a passive relay of an upstream source."**
- On a follower that polls `list=recentchanges` when a source declares no event stream: **"yes. You should look into how the Wikidata Query Service sync scripts do it, back when the query service used recent changes instead of the new thing they now have."**
- On attribution: **"I think this is consistent with the passive mirror function."** A source records no upstream actors.
- On write-back: **"No need to work on code pushing content upstream until after proposals is implemented."**
- **"Yes, page repositories without events should use this new follower. Using a bot login upstream for higher API limits should be allowed but optional."**
- **"What would we need to build before we can support importing via dump and then using recent changes to sync to the present?"**

## Decision

### 1. Following a wiki by its recent changes (extends 0070 §5; amends 0078 §4; settles 0070 Q5)

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.5, §5.7, §6.4.*

### 2. A row says only that a title changed (extends 0070 §3)

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.7.*

### 3. The follower's position (amends 0053 §8; amends 0070 §10)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.12; [05](../architecture/05-providers-and-ingest.md) §5.7.*

### 4. Loading a source: a dump or a crawl (amends 0078 §4; extends 0070 §7)

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.9, §6.4.*

### 5. From the load to the follower (extends 0070 §7)

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.9.*

### 6. Settings

*Current text: [23](../architecture/23-configuration-and-registry.md) §3.2.*

### 7. API and operations (extends 0070 §9)

*Current text: [18](../architecture/18-api.md) §3.2.*

### 8. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 9. Page repositories without events (amends 0053 §6)

*Current text: [13](../architecture/13-mirrored-pages.md) §2.7.*

### 10. Logging in upstream, optionally (extends 0070 §3; extends 0053 §6)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.16; [05](../architecture/05-providers-and-ingest.md) §5.7.*

## Alternatives considered

- **The daily sweep alone, as 0078 §4 had it.** It never finds a new item, and its latency is a day.
- **Polling `list=logevents` and `list=recentchanges` separately.** `recentchanges` already carries the log rows that matter (deletions, restorations, merges) in the entity namespaces, in one ordered list.
- **Deciding from the row's type.** A row says a delete or an edit happened, not what state the entity is in now; acting on it needs order and completeness that `recentchanges` does not promise ([T161731](https://phabricator.wikimedia.org/T161731)). The state is always right.
- **A position by `rcid` alone.** `rcid`s are allocated before commit, so a later commit can carry a smaller ID ([T161731](https://phabricator.wikimedia.org/T161731)). Time with a re-read window and an `rcid` set is what the query service settled on.
- **A trailing poller thread, as the query service ran.** The tail pass does the same work in the same loop.

## Consequences

- **Any Wikibase with an API can be followed to within a minute**, without an event stream and without a registry entry.
- **A source can be loaded in bulk from a dump**, and the dump's age is checked against the wiki's recent-changes window before the follower takes over.
- **The test plan's live phase** (`docs/testing/demofarm.md`) runs on these pieces and needs no fetch tool of its own.
- **Upstream editors are not recorded for a source.** That is intended for a passive relay; [0078](0078-entity-sources.md) Q1 remains the place to change it.

## Open questions

- **Q1. Page repositories without events.** *Settled by §9.*
- **Q2. Another Triplespace instance.** *Withdrawn: [0022](0022-federation.md) §2 already answers it (17 §1.2; 16 §4.4). It follows the provider's activity stream filtered to `source = local`, verified against its checkpoints, never recent changes. Its position is the stream's per-partition resume token, kept in `ops.repo_cursor`'s `last_event_id` (§3).*
- **Q3. Higher limits.** *Settled by §10.*

## References

- `wikidata-query-rdf`, `tools/src/main/java/org/wikidata/query/rdf/tool/change/RecentChangesPoller.java` and `TailingChangesPoller.java`; `tool/wikibase/WikibaseRepository.java` (`recentChanges`); `tool/Updater.java` (`handleChange`); `tool/change/ChangeSourceContext.java` (`getStartTime`), as of the repository's master on 2026-10-26
- [T161731](https://phabricator.wikimedia.org/T161731), [T95194](https://phabricator.wikimedia.org/T95194)
- `docs/testing/demofarm.md`

## Amendment log

### A1. What the position row holds

- **Date:** 2026-10-27
- **Source:** Direct: James, go-ahead to implement (talk conversion, then Stage A), 2026-10-26
- **Change:** amends §3
- **Summary:** The implementation keeps each seen `rcid` with its time, since the tail window expires them by time and `rc_time` alone cannot say when a row was seen; keeps the stale reads to retry, since §2 reads them again "on the next poll" and a `follow --once` run has no next poll of its own; and keeps the load's end, the handover flag and the counters in the same row rather than beside it. The chapter 03 table (§4.12) is folded to match. The decision is unchanged.

Replaced text: §3 of this ADR.

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** 0050 §14
- **Change:** relocates §1–§10

### A3. A state that will not parse is rejected, and the follower goes on

- **Date:** 2026-11-09
- **Source:** Direct: Claude, while running the follower on the whole-dump mirror, 2026-11-09; unconfirmed
- **Change:** extends §2
- **Summary:** The follower reads every entity namespace (§1), and Wikidata's include `Lexeme`, whose states the entity model does not read yet (0066). One such state stopped the whole run, so a state that will not parse is now written as a rejection on the follow job, its reason the parse error, and the batch goes on, as a dump sync counts a line it cannot read. Until lexemes are read, each lexeme edit costs a fetch and a rejection. Chapter text: 05 §5.7.

### A4. A load moves the follower's start only forward

- **Date:** 2026-11-09
- **Source:** Direct: James, review of the follower's run on the whole-dump mirror, 2026-11-09
- **Change:** amends §5
- **Summary:** A sync of two entities into the whole-dump mirror recorded their newest `modified`, three months before the dump's, as where the load ended, and so would have started the follower behind the wiki's recent changes. A load now records its end only when it is later than the end already recorded, and only then resets the follower's position; otherwise `sync` says the position is kept. Chapter text: 05 §5.9.

Replaced text: §5, in 05 §5.9 of this ADR.

### A5. A redirect or deletion at a revision already claimed waits for its own row

- **Date:** 2026-11-09
- **Source:** Direct: James, review of the follower's run on the whole-dump mirror, 2026-11-09
- **Change:** amends §2
- **Summary:** The follower reads an entity's current state, which can be newer than the row that sent it there. On the whole-dump mirror it wrote `Q141934632`'s state at revision 2565763758 from the item's creation row; the item was redirected afterwards, and when the follower reached the row for 2565763758 it read the redirect and wrote it at that revision, which the `put` already claimed, and `view.activity`'s unique revision stopped the run. A redirect or deletion whose revision the subject's records already claim is now written as nothing, counted unchanged: its own row (the redirecting edit, the deletion's log row) is still ahead of the follower and writes it, and the sweep catches what a lost row would miss. No record claims a revision twice, as for a `put` already. Chapter text: 05 §5.7.

Replaced text: §2, in 05 §5.7 of this ADR.

### A6. A follower writes under one job a day

- **Date:** 2026-11-11
- **Source:** Direct: James, review of the follower's runs on the whole-dump mirror, 2026-11-11
- **Change:** extends §2
- **Summary:** The follower wrote each batch as a job of its own: 11,955 jobs over two days on the whole-dump mirror, about 1,440 a day per followed source once caught up, each with its own start and finish records and its own rejects, while the module said "one run is one job". Neither fits: a job per batch buries the jobs list and splits a day's rejects across thousands of jobs, and a job per run never finishes in a server's loop and differs between a timer's `--once` runs and a server. A follower now writes under one job per UTC day, version `follow-YYYY-MM-DD`: the day's first batch starts it, each batch commits under it as its own unit of work under the load lock, as before, and the first batch of a later day, or the first run after it, finishes it with the day's counts, batches, rows, late rows and stale states. The open job is kept with the position in `ops.repo_cursor.job` and reopened by the next run. Because it stays `running` between batches, a load of the same source and graph no longer marks a follower's job interrupted (0011 A36, A37); only a follower's new job marks an earlier follower's job, one whose position row lost it. A crash between a batch's commit and the position's save replays that batch, as before, and its counts may then be counted twice. The bookkeeping a job per batch cost was small, about 16 ms of Postgres time a batch; this is about what an operator reads. Chapter text: 05 §5.7, 16 §2.2, 03 §4.12.

### A7. A follower compacts its mirror as the day's job closes

- **Date:** 2026-11-11
- **Source:** Direct: James, review of the whole-dump mirror's size, 2026-11-11 ("Start with compaction"), on Claude's proposal to compact the keys a day wrote as its job closes; the calls below unconfirmed
- **Change:** extends §2
- **Summary:** The whole-dump mirror is a `latest` partition, but nothing compacted it, so every followed edit kept the entity's previous state as well: about 3.28 million superseded records after two days of following, and growth of about 7.2 KB an edit, about 1.5 GB a Wikidata day. A registry provider's follower now compacts its mirror by the keys it wrote as each day's job closes (0013 A96). The calls made: (1) the day's job notes the mirror's size when it opens, `compact_from`, kept with the open job in `ops.repo_cursor.job`; a mirror not yet created starts at 0, and a job opened before this change has none and is not compacted. (2) When the first batch of a later day closes the job, the mirror is compacted from `compact_from` to its size then, before the finish record, which gains `compacted`, the records removed; a crash between the two compacts again, which removes nothing more, and then finishes. (3) A tenant's entity source is not compacted by its follower. (4) What runs before this change left is compacted by hand with `triplespace compact`: on the whole-dump mirror, the keys of the 3,456,078 records the follower had written (offsets 122,056,985 to 125,513,063) lost 3,259,844 older records in 58 minutes, about 940 a second, 10,000 offsets a unit of work, leaving 122,253,219 records for 122,231,562 entities; every key's `view.entity_source` record survived, and no key kept more than two records. Chapter text: 05 §5.7.

### A8. A day's compaction runs beside the next day's batches

- **Date:** 2026-11-15
- **Source:** Direct: James, 2026-11-15 ("Daily compaction beside the follower": "Yes"), on Claude's proposal after the follower's run on the whole-dump mirror; the calls below unconfirmed
- **Change:** amends §2 (A7 (2))
- **Summary:** A7 compacted the closed day inside the follower's write, so the next batch waited for all of it. On the whole-dump mirror, the close of 2026-11-13 compacted 748,739 records in about 12 minutes, with no appends for 12.5 minutes, and a caught-up Wikidata day is about that size. A manual `triplespace compact` beside the running follower delayed its appends by at most 5.5 s at a time, about one window. The day's compaction now runs beside the batches that follow. The calls made: (1) when a day closes, the job and its compaction's range and progress (`closing`: the partition, the mirror's size at the close, the first offset not yet compacted, the records removed so far) are kept with the open job and the position in `ops.repo_cursor.job`, which now holds `{open, closing}`; a position saved before this change, the open job alone, still reads. (2) With each batch the follower compacts at least one window of 10,000 offsets, and goes on window by window while the batch is written and the next one read. (3) The closed day's job is finished, with `compacted`, once its range is done, so its finish record follows the next day's start. A `--once` run completes the compaction before it returns, so a timer's runs leave no compaction behind, and a day that closes while an earlier day is still compacting completes the earlier one first, inside the write, as A7 did. (4) A crash loses only the progress not yet saved; the windows run again remove nothing more, so `compacted` can then fall short, as A7's crash between compaction and finish could. Chapter text: 05 §5.7.

Replaced text: A7 (2), in 05 §5.7 of this ADR.

### A9. A run that stops when caught up leaves the rest of the day's compaction to the next

- **Date:** 2026-11-18
- **Source:** Direct: James, 2026-11-18 ("Make the recommended fix"), on Claude's report of the whole-dump mirror's second day close; the calls below unconfirmed
- **Change:** amends §2 (A8 (3))
- **Summary:** A8 had a run with `once` complete the closed day's compaction before it returned. `triplespace follow` without `--once` was a loop of such runs with a wait between them, and the job queue's follower is a pass with `once` each time, so once the follower had caught up, the first run after midnight compacted the whole day with nothing written beside it. On the whole-dump mirror, the close of 2026-11-17 (2,290,119 offsets) stopped the follower from 00:00:11 to 00:47, and it fell 0.8 hours behind; the first close had not shown it, because the follower was still catching up and its run never ended. Now: (1) no run completes a compaction before it returns; a run that stops when caught up compacts beside its batches, at least a window, and leaves the rest, and the day's finish, to the next run, as it leaves the day's open job. (2) `triplespace follow` without `--once` is one run that never stops when caught up: its change source waits `follow.interval` before the poll that follows a caught-up one, saying so on stderr in place of the summary line each run printed, and the compaction runs beside that wait as well as beside the batches. (3) The job queue's follower still runs passes, so its compaction moves at least a window a pass; a day that closes while an earlier one is still compacting still completes the earlier one first. Chapter text: [05](../architecture/05-providers-and-ingest.md) §5.7.

Replaced text: A8 (3), in 05 §5.7 of this ADR.

### A10. The position and the jobs commit with what they describe

- **Date:** 2026-11-19
- **Source:** Direct: James, 2026-11-19 ("Yes"), to Claude's proposal to start SCA-285 by moving the follower's position into the batch's transaction; the calls below unconfirmed
- **Change:** amends §2 (A6)
- **Summary:** §3 has the position written in the transaction that writes the records it covers ([05](../architecture/05-providers-and-ingest.md) §5.7), but the follower saved it, with its jobs, in a statement of its own after the batch had committed. A crash between the two replayed the batch and counted it twice, as A6 said. Worse, a batch commits in several units of work, the first of which starts a new day's job, so a crash before the save left a job started and not saved: the next run started a second job for the day, and the first stayed `running` until that start marked it interrupted. A closed day's finish record and the jobs that no longer held it were committed apart in the same way. The calls made: (1) each unit of work of a batch saves the jobs, with the day's job as it stood before the batch, or as the batch started it; the batch's last unit saves the position the batch leaves and the day's job with the batch counted. A batch cut off part way is read again under the same job and counted once, the items its committed units wrote then counting as unchanged. (2) A day's finish record is committed with the jobs that no longer hold it. (3) A batch with nothing to write saves the position in a unit of work of its own, and a compaction that moved on without finishing saves its progress in one after the batch; a crash still loses only the progress not yet saved (A8 (4)). (4) The change source gives the follower a saver that writes in the store's units of work, so that the next batch is still read while one is written. Chapter text: [05](../architecture/05-providers-and-ingest.md) §5.7.

Replaced text: A6, in 05 §5.7 of this ADR.

### A11. A lost position is rebuilt from the newest revision the graph holds

- **Date:** 2026-11-20
- **Source:** Direct: James, 2026-11-20 ("Yes"), to Claude's proposal to continue SCA-285 with recovering a lost position from `view.entity_source`; the calls below unconfirmed
- **Change:** amends §3
- **Summary:** §3 rebuilds a lost position "from the newest upstream time in `view.entity_source`, less `follow.tail`", but the version cursor holds no time: for a Wikibase source its `upstream_version` is the revision ID. The follower refused to start without a position or a load's end, unless given `--from`. The calls made: (1) a follower with neither a position nor a load's end, whose graph holds entities, takes the newest revision ID in `view.entity_source` for its tenant and graph, asks the wiki when that revision was saved (`prop=revisions`), and starts from that time less `follow.tail`; the version cursor skips what is already written, and the gap check applies as to any position. (2) A load's end, when there is one, still comes first; `--from` is used only when the graph holds nothing. (3) The newest revision is read from every row of the graph, about 77 seconds on the whole-dump mirror; that is paid only when a position is lost. (4) A newest revision the wiki no longer has (deleted or suppressed) refuses to start and names `--from`. On the whole-dump mirror, the time rebuilt was 01:39:09 against a saved position of 01:40:10. Chapter text: [05](../architecture/05-providers-and-ingest.md) §5.7.

Replaced text: §3, in 05 §5.7 of this ADR.
