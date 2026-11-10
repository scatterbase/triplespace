# 0092. Following a wiki by its recent changes; loading a source

- **Status:** Proposed
- **Date:** 2026-10-26
- **Updated:** 2026-11-09 (A5)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0053](0053-mirrored-pages.md), [0070](0070-shallow-entity-mirroring.md), [0078](0078-entity-sources.md)
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

*Changed by A3, A5.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.7.*

### 3. The follower's position (amends 0053 §8; amends 0070 §10)

*Changed by A1.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.12; [05](../architecture/05-providers-and-ingest.md) §5.7.*

### 4. Loading a source: a dump or a crawl (amends 0078 §4; extends 0070 §7)

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.9, §6.4.*

### 5. From the load to the follower (extends 0070 §7)

*Changed by A4.*

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

- **Q1.** ~~**Page repositories without events.** [0053](0053-mirrored-pages.md) §6 says such a repository "is polled or waits for its TTL", without saying how. Whether it uses this follower, over its served namespaces.~~ *Settled by §9: it does.*
- **Q2.** ~~**Another Triplespace instance.** A provider that is a Triplespace instance publishes its log and checkpoints ([0022](0022-federation.md) §2), which serve better than recent changes. Whether its follower reads those instead.~~ *Withdrawn: [0022](0022-federation.md) §2 already answers it (17 §1.2; 16 §4.4). It follows the provider's activity stream filtered to `source = local`, verified against its checkpoints, never recent changes. Its position is the stream's per-partition resume token, kept in `ops.repo_cursor`'s `last_event_id` (§3).*
- **Q3.** ~~**Higher limits.** `rclimit` and `wbgetentities` take 500 and 50 for anonymous clients. Whether the instance may log in to a provider with a bot account for `apihighlimits`, and where that credential lives.~~ *Settled by §10: it may, optionally, with a bot password sealed in `private`.*

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

Replaced text (§3):

> A stream keeps `last_event_id` and `last_event_time`. A poll keeps:
> - `rc_time`, the newest timestamp it has seen;
> - `rc_continue`, while it has one;
> - `rc_seen`, the `rcid`s within `follow.tail` of `rc_time`.

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [05](../architecture/05-providers-and-ingest.md), [13](../architecture/13-mirrored-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.

### A3. A state that will not parse is rejected, and the follower goes on

- **Date:** 2026-11-09
- **Source:** Direct: Claude, while running the follower on the whole-dump mirror, 2026-11-09; unconfirmed
- **Change:** extends §2
- **Summary:** The follower reads every entity namespace (§1), and Wikidata's include `Lexeme`, whose states the entity model does not read yet ([0066](0066-lexemes.md)). One such state stopped the whole run, so a state that will not parse is now written as a rejection on the follow job, its reason the parse error, and the batch goes on, as a dump sync counts a line it cannot read. Until lexemes are read, each lexeme edit costs a fetch and a rejection. Chapter text: [05](../architecture/05-providers-and-ingest.md) §5.7.

### A4. A load moves the follower's start only forward

- **Date:** 2026-11-09
- **Source:** Direct: James, review of the follower's run on the whole-dump mirror, 2026-11-09
- **Change:** amends §5
- **Summary:** A sync of two entities into the whole-dump mirror recorded their newest `modified`, three months before the dump's, as where the load ended, and so would have started the follower behind the wiki's recent changes. A load now records its end only when it is later than the end already recorded, and only then resets the follower's position; otherwise `sync` says the position is kept. Chapter text: [05](../architecture/05-providers-and-ingest.md) §5.9.

Replaced text (§5, in 05 §5.9):

> It records where it ends: an XML dump's newest revision timestamp, a JSON dump's newest `modified`, or the time a crawl began. The follower starts there, less `follow.tail`, and the version cursor skips what the load already wrote.

### A5. A redirect or deletion at a revision already claimed waits for its own row

- **Date:** 2026-11-09
- **Source:** Direct: James, review of the follower's run on the whole-dump mirror, 2026-11-09
- **Change:** amends §2
- **Summary:** The follower reads an entity's current state, which can be newer than the row that sent it there. On the whole-dump mirror it wrote `Q141934632`'s state at revision 2565763758 from the item's creation row; the item was redirected afterwards, and when the follower reached the row for 2565763758 it read the redirect and wrote it at that revision, which the `put` already claimed, and `view.activity`'s unique revision stopped the run. A redirect or deletion whose revision the subject's records already claim is now written as nothing, counted unchanged: its own row (the redirecting edit, the deletion's log row) is still ahead of the follower and writes it, and the sweep catches what a lost row would miss. No record claims a revision twice, as for a `put` already. Chapter text: [05](../architecture/05-providers-and-ingest.md) §5.7.

Replaced text (§2, in 05 §5.7):

> The job reads the entity's current state and writes what the state says, as a fetch does (§5.5): a `put` when its `lastrevid` is newer than the cursor, a `redirect` for an upstream redirect, a `tombstone` when an entity in the set is missing, nothing otherwise ([0092](../decisions/0092-following-recent-changes.md) §2).
