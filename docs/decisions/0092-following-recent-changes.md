# 0092. Following a wiki by its recent changes; loading a source

- **Status:** Proposed
- **Date:** 2026-10-26
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

**A provider or entity source with an `api` and no `events` is followed by polling its `list=recentchanges`.** This is one more way the fetch job of [0070](0070-shallow-entity-mirroring.md) §3 learns that something changed: the stream, the poll and the sweep all enqueue fetches into the same queue. It applies alike to:
- a registry Wikibase provider other than Wikidata, such as Librarybase or the OpenStreetMap wiki;
- a tenant's entity source.

The daily sweep stays behind it. Wikidata keeps its event stream; the stream's gap-closing is this poll.

**The query.** One request per poll, every `follow.interval` (§6, default one minute):
- `action=query&list=recentchanges&rcdir=newer`;
- `rcprop=title|ids|timestamp`, `rctype=edit|new|log`, `rclimit=max`;
- `rcnamespace` = the wiki's entity namespaces;
- `rcstart` = the position (§3) while it has no continuation, `rccontinue` while it does.

The entity namespaces are read from the wiki's `siteinfo` by their default content model (`wikibase-item`, `wikibase-property`, …). So a Miraheze wiki's 860 and 862 need no configuration, and neither do the standard 120 and 122. Requests go through the upstream client: the `upstream` rate class ([0024](0024-subsidiary-accounts.md) §5), `maxlag`, a User-Agent naming the instance, one request in flight per follower, and `Retry-After` honoured.

**Reading a batch:**
- Every row moves the position forward, whether or not it is used.
- A row whose `rcid` the follower has seen in the last `follow.tail` (§6) is skipped.
- The rest are coalesced to one per title: the highest revision wins, and a log row (revision 0: a deletion, a restoration, a merge's or a suppression's log entry) always stays.

**Which rows count.** Under `entities.mirror = all`, every row in the entity namespaces. Under `linked` and `on-demand`, only rows for entities already in the set, as for an event (0070 §5).

**Settling.** Recent rows can still arrive with earlier timestamps. While the position is within `follow.settle` of now (§6, default two minutes), each poll reads by time from ten seconds before the position rather than by continuation. If a full batch held nothing new, the position moves on one second.

**A tail pass.** After catching up, each poll also re-reads the window `follow.tail` behind the head (default 15 minutes; `PT0S` turns it off), deduped by `rcid`. A row the tail pass finds that the head missed is fetched like any other and counted on the job as `late`. The count measures how far a wiki's replication can be trusted. The pass is one more request per poll, not a second thread.

**Other wikis.** Wikidata keeps its event stream, but its gap-closing is this poll. A registry Wikibase provider other than Wikidata uses the same job with its own `api`, followed this way when it has no `events`. Another Triplespace instance is [0022](0022-federation.md)'s (Q2).

### 2. A row says only that a title changed (extends 0070 §3)

**The follower never decides from the row's type.** For each title a poll keeps, it reads the entity's current state, `wbgetentities` with 50 IDs per request, and writes what the state says. The rules are those of a fetch ([0070](0070-shallow-entity-mirroring.md) §3):

| The state | Record |
|---|---|
| An entity whose `lastrevid` is newer than its version cursor | a `put` |
| An entity whose `lastrevid` is not newer | nothing |
| An upstream redirect | a `redirect` |
| Missing, for an entity in the set | a `tombstone` |
| Missing, for an entity never mirrored | nothing ([0070](0070-shallow-entity-mirroring.md) A5) |

An edit, a creation, a deletion, a restoration and a merge therefore take one path, and order across rows does not matter. A state whose `lastrevid` is older than the row's revision comes from a lagging replica: the title is fetched again on the next poll, and the job counts it as `stale`.

Upstream hiding is not decided here. A mirror under `latest` holds the current state only, and whatever upstream shows now is what it holds. Following upstream when it hides an older revision is [0011](0011-logs.md) §5's, for mirrors that keep history.

### 3. The follower's position (amends 0053 §8; amends 0070 §10)

**`ops.repo_cursor` holds every follower's position**: a page repository's event stream, a provider's stream or poll, and a tenant's source's stream or poll. It is keyed by `(tenant, kind, name)`:
- `tenant` is `''` for the instance's followers;
- `kind` is `repo`, `provider` or `source`;
- `name` is the repository, the provider slug or the source name.

A stream keeps `last_event_id` and `last_event_time`. A poll keeps:
- `rc_time`, the newest timestamp it has seen;
- `rc_continue`, while it has one;
- `rc_seen`, the `rcid`s within `follow.tail` of `rc_time`.

**The position is written in the transaction that writes the records it covers.**

**The position is a hint; the version cursor is the guarantee.** A replayed row finds the entity current and writes nothing (§2). If a row is lost, as `ops` rows are not projections of the log, the follower resumes from the newest upstream time in `view.entity_source` for the provider or source, less `follow.tail`.

**A gap the wiki cannot close.** A wiki keeps a limited window of recent changes: MediaWiki's `$wgRCMaxAge`, 90 days by default. Before resuming, the follower reads the oldest row the wiki still has. A position older than that is a gap:
- **Under `all`**, the job stops with `follow-gap`, and the operator loads again (§4), after which the follower resumes from the new load.
- **Under `linked` and `on-demand`**, the follower resumes from the oldest row and runs the sweep at once; the sweep covers the set.

### 4. Loading a source: a dump or a crawl (amends 0078 §4; extends 0070 §7)

**A source's partition is written by the source's own jobs: its fetch job and its loads.** A load is how a type at `all` ([0070](0070-shallow-entity-mirroring.md) §7) is filled. It takes one of two forms:
- **A dump sync** from a dump the operator supplies, JSON or XML. It is the ordinary sync job ([0091](0091-one-wikibase-adapter.md) §4), with its upsert and snapshot modes, version cursor, rejects and threshold, writing to `source/{name}`. Its actor is the tenant's sync subsidiary, as for any job.
- **A crawl**, where no dump is available: `list=allpages` over the entity namespaces, fetched through the fetch job in batches of 50 with the reason `crawl`. A crawl is a snapshot: when it finishes, entities in the partition that it did not see are swept, as for a snapshot sync, under the same threshold.

A registry provider may be loaded the same ways. The only difference is that its jobs are the instance's ([0040](0040-instance-prerogatives.md) §2).

A source still records no upstream actors and keeps no upstream history ([0078](0078-entity-sources.md) Q1 stays open). The source is a passive relay; offering local changes back upstream is proposals' work ([0067](0067-proposals.md)).

### 5. From the load to the follower (extends 0070 §7)

**A load records where it ends:**
- for an XML dump, the newest revision timestamp in the dump;
- for a JSON dump, the newest `modified`;
- for a crawl, the time the crawl began.

**The follower starts there, less `follow.tail`.** The version cursor skips whatever the load already wrote.

A load whose position is older than the wiki's oldest recent change cannot hand over. `sync` warns at the end of the load, and the follower then reports a gap (§3). A newer dump or a crawl closes it.

### 6. Settings

Three `site` settings, per follower: the instance's record for a registry provider or a page repository (§9), the tenant's for its entity sources.

| Setting | Type | Default | Meaning |
|---|---|---|---|
| `follow.interval` | duration | `PT1M` | How often a follower without an event stream polls recent changes; for a page repository, at least `repo.debounce` |
| `follow.settle` | duration | `PT2M` | Within this of now, a poll reads by time from ten seconds before its position rather than by continuation |
| `follow.tail` | duration | `PT15M` | How far behind the head the tail pass re-reads, and how long `rcid`s are remembered; `PT0S` turns the tail pass off |

### 7. API and operations (extends 0070 §9)

- **`GET /provider/{code}/mirror`** gains:
  - how the provider or source is followed, `events` or `recentchanges`;
  - the follower's position and lag;
  - the `late` and `stale` counts;
  - the last load's position and whether it handed over.
- **`triplespace sync --tenant {slug} --source {name} --dump {file}`** loads a source (§4). `triplespace sync --provider {slug}` loads a registry provider, as now.
- **`triplespace follow --provider {slug}`** and **`triplespace follow --tenant {slug} --source {name}`** run the follower in the foreground. With `--once`, it catches up to the present and exits, which is what a test or a cron job needs. A server runs its followers in its background loop.

### 8. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-adapter-mediawiki` | The recent-changes query and reading a batch (dedupe by `rcid`, coalescing by title, the settle and tail windows) over namespaces the caller names, as functions over responses with no I/O of their own; a MediaWiki concern, shared by entity and page followers (§1, §9) |
| `scatter-adapter-wikibase` | The entity namespaces from `siteinfo` by content model, and a row's title as an entity ID (§1) |
| `scatter-ingest` | The follower loop in the fetch job, over a `ChangeSource` trait that yields rows and states, so that the ingester names no API, as its `FilterHook` names no filter language; the position and its handover from a load, the gap check, loads into a source partition and the crawl (§1–§5) |
| `triplespace-api-ingest` | `ChangeSource` over `scatter-adapter-wikibase` and `triplespace-upstream` (§1–§2) |
| `triplespace-upstream` | The requests, under the `upstream` rate class (§1) |
| `triplespace-db` | `ops.repo_cursor` keyed by `(tenant, kind, name)` with the poll's columns (§3) |
| `triplespace-cli` | `sync --source`, `follow` (§7); `upstream-login set` and `unset` (§10) |
| `triplespace-accounts` | `private.upstream_login`, sealed as the other secrets it keeps (§10) |
| `triplespace-repos` | The page repository's follower, over `scatter-adapter-mediawiki`'s poll and `triplespace-upstream` (§9) |

### 9. Page repositories without events (amends 0053 §6)

**A page repository without `events` is followed by the same poll** (James, 2026-10-26), not left to its TTL. The query is §1's, with `rcnamespace` set to the repository's served namespaces and `rcprop=title|ids|timestamp|loginfo`, since a page's move names its target in the log row. After deduplication and coalescing, each row is handled as an event is (0053 §6, in 13 §2.7):
1. it updates the title index;
2. it purges the page in `proxy` mode, or queues a `put` for a page in the set in `mirror` mode;
3. for a template or module, it bumps the render epoch of the local pages that use it;
4. for a move or a deletion, it refreshes the local pages that link to the title.

Coalescing per poll replaces `repo.debounce`, which bounds `follow.interval` from below for a repository. The position is the repository's row in `ops.repo_cursor` (§3), of kind `repo`. A repository with `events` keeps its stream, and the stream's gap-closing is this poll.

### 10. Logging in upstream, optionally (extends 0070 §3; extends 0053 §6)

**A follower, a fetch job and a crawl may log in to the wiki they read** (James, 2026-10-26: "allowed but optional"), so that the API grants them `apihighlimits`:
- `rclimit` rises from 500 to 5,000;
- `wbgetentities` rises from 50 IDs per request to 500;
- `list=allpages` rises from 500 to 5,000.

This applies to a registry provider, an entity source and a page repository alike. Without a login, everything works at the anonymous limits.

**The login is a MediaWiki bot password** (`Special:BotPasswords`) for an account the operator holds on the upstream wiki. It needs only the `basic` and `highvolume` grants, and should be given no others: the login is never used to write. A follower logs in with `action=login` at start, keeps the session for its requests, and logs in again when the session lapses. A login that fails is reported on the job and in `GET /provider/{code}/mirror`, and the follower carries on anonymously, at the lower limits.

**The secret lives in `private.upstream_login`**, one row per follower: `(tenant, kind, name)`, as `ops.repo_cursor` is keyed (§3), with `username`, `secret` and `created`/`updated`. The secret is sealed under the instance sealing key, as `private.oauth_client`'s is ([0088](0088-external-identity-providers.md) §3). It never appears in a registry file or a config record, both of which are public, and it "never leaves" under [0027](0027-preferences-and-portability.md) §2, so a tenant move asks for it again. `triplespace upstream-login set --provider {slug} | --tenant {slug} --source {name} | --repo {name} --username {name@label} --password-file {file}` writes it, and `upstream-login unset` removes it.

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

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §8 | amends | 0005 A106 |
| [0053](0053-mirrored-pages.md) §6 | §9, §10 | amends | 0053 A8 |
| [0053](0053-mirrored-pages.md) §8 | §3 | amends | 0053 A8 |
| [0070](0070-shallow-entity-mirroring.md) §3 | §2, §10 | extends | 0070 A8 |
| [0070](0070-shallow-entity-mirroring.md) §5 | §1 | extends | 0070 A8 |
| [0070](0070-shallow-entity-mirroring.md) §7 | §4, §5 | extends | 0070 A8 |
| [0070](0070-shallow-entity-mirroring.md) §9 | §7 | extends | 0070 A8 |
| [0070](0070-shallow-entity-mirroring.md) §10 | §3 | amends | 0070 A8 |
| [0078](0078-entity-sources.md) §4 | §1, §4 | amends | 0078 A5 |

## References

- `wikidata-query-rdf`, `tools/src/main/java/org/wikidata/query/rdf/tool/change/RecentChangesPoller.java` and `TailingChangesPoller.java`; `tool/wikibase/WikibaseRepository.java` (`recentChanges`); `tool/Updater.java` (`handleChange`); `tool/change/ChangeSourceContext.java` (`getStartTime`), as of the repository's master on 2026-10-26
- [T161731](https://phabricator.wikimedia.org/T161731), [T95194](https://phabricator.wikimedia.org/T95194)
- `docs/testing/demofarm.md`
