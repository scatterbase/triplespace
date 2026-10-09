# 0020. Change feeds: recent changes, related changes and watchlists

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A14)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0011](0011-logs.md), [0018](0018-tenants.md), [0022](0022-federation.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

## Context

MediaWiki has four features that list changes: page history, recent changes, related changes (`Special:RecentChangesLinked`) and the watchlist. It implements them with three tables and two joins: history reads `revision`, recent changes reads `recentchanges`, related changes joins `recentchanges` to `pagelinks`, and the watchlist joins it to `watchlist`. They are one feature with four target sets.

Triplespace has already folded the tables. [0012](0012-api-requirements.md) §3 gave history, recent changes, contributions and the log one row shape; [0013](0013-postgres-storage.md) §5 made `view.activity` one table, indexed by time, actor and target; [0010](0010-site-ui.md) §5 defined a history as every record keyed to a target or a member of its cluster. What is left is to name the abstraction, add the two target sets that do not exist yet, and answer the two questions the earlier ADRs left open: how watchlists work, and whether a watch on a mirrored entity covers its syncs ([0010](0010-site-ui.md)); and whether there is a change feed for downstream consumers ([0012](0012-api-requirements.md)). [0019](0019-discussions.md) §8 also promised that watching a talk page covers its threads.

One constraint is fixed by James: **whether a user is watching something is private and is not part of the graph.** It is not a record in any partition, logged or otherwise.

## Decision

### 1. A feed is activity rows filtered by a target set

*Changed by A3.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.1.*

### 2. Target sets

*Changed by A6, A7, A9, A11, A12.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.2.*

### 3. The watch set is private state, not records (extends 0007 §8, 0013 §4)

*Changed by A2, A5, A11, A13.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.3.*

### 4. Delivery: page, Atom, stream (settles 0012 Q6)

*Changed by A6, A8.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.4.*

### 5. API (extends 0012 §4 and §5)

*Changed by A2, A6.*

*Current text: [18](../architecture/18-api.md) §2.3, §3.1, §3.2.*

### 6. Storage and cost (extends 0013 §5)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.6.*

### 7. Permissions (extends 0016 §2)

*Changed by A4.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §8.2.*

### 8. Crates

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **One query path.** History, recent changes, related changes, the watchlist, contributions and the downstream stream are one code path over one table with one row shape, and they cannot disagree about what a change is or who may see it.
- **A watch is a private filter, never data.** Nobody, including the instance's own graph, records who watches what. The price is that the watch set is not rebuildable from the log and does not follow a tenant when it moves.
- **Watching a mirrored entity includes its upstream changes,** by default, which is the answer 0010 was waiting for.
- **No watcher counts,** which MediaWiki tools occasionally use to gauge attention. Here attention is private.
- **The downstream feed exists without a new model**, and it carries erasures, so a consumer can be as faithful to this instance as this instance is to Wikidata.
- **Related changes for entities are new territory.** MediaWiki's are link-based; here the outward relation is the statement graph, which is what a Wikibase editor actually means by "related".

## Open questions

- **Q1.** ~~**Notifications.** Mentions, replies, new messages on one's talk page, email and digests: a feed with push delivery and per-user read state, deserving its own ADR now that the pull side exists.~~ *Settled by [0021](0021-notifications.md), which adds a `notify` flag to `private.watch`.*
- **Q2.** ~~**Patrolling.**~~ *Settled by [0023](0023-moderation.md) §6: a `patrolled` filter on every feed.*
- **Q3.** ~~**Preferences.** Where the rest of an account's preferences live; this ADR puts four in `private` beside the watch set.~~ *Settled by [0027](0027-preferences-and-portability.md) §1: one `private.preference` store with a registered key set; the four auto-watch flags move there.*
- **Q4.** ~~**Provider-to-tenant sync over the stream** instead of by job ([0018](0018-tenants.md) §5).~~ *Settled by [0028](0028-tenancy-policy.md) §5 and [0022](0022-federation.md) §2: same-instance reading is direct; cross-instance reading is a sync job that follows the stream filtered to `source = local`, verified against the provider's checkpoints by default.*
- **Q5.** ~~**Rate limits** on the stream and on Atom, and the default period window per feed.~~ *Settled by [0024](0024-subsidiary-accounts.md) §5 and [0027](0027-preferences-and-portability.md) §1: rate limits are the `stream` and `atom` classes; the period window is the `feeds.period` preference, with its default in `preferences.toml`; the default's value is tuning.*
- **Q6. Watching a namespace or a query.** Dynamic watch sets defined by a SPARQL query or a search, as some wikis have wanted.
- **Q7. Related changes for keyed types.** Whether a domain's related set is its DNS parents and children ([0009](0009-keyed-entity-types-and-domain.md)).

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §8 | extends | 0005 A16 |
| [0007](0007-actor-identity.md) §8 | §3 | extends | 0007 A4 |
| [0010](0010-site-ui.md) §7, §11, §12 | §4 | extends | 0010 A10 |
| [0010](0010-site-ui.md) Q1 | §2–3 | settles | 0010 Q1 |
| [0012](0012-api-requirements.md) §4, §5 | §4–5 | extends | 0012 A9 |
| [0012](0012-api-requirements.md) Q5 | §5 | settles | 0012 Q5 |
| [0012](0012-api-requirements.md) Q6 | §4 | settles | 0012 Q6 |
| [0013](0013-postgres-storage.md) §4, §5.6 | §3 | extends | 0013 A7 |
| [0014](0014-caches-and-search.md) §1, §5 | §5 | amends | 0014 A3 |
| [0014](0014-caches-and-search.md) §10 | §5 | extends | 0014 A3 |
| [0016](0016-permissions-and-access-control.md) §2 | §7 | extends | 0016 A3 |
| [0019](0019-discussions.md) §8 | §3 | extends | 0019 A2 |

## References

- [Help:Watchlist](https://www.mediawiki.org/wiki/Help:Watchlist), [Help:Watchlist expiry](https://www.mediawiki.org/wiki/Help:Watchlist_expiry) and [Help:Related changes](https://www.mediawiki.org/wiki/Help:Related_changes)
- [API:Watchlist](https://www.mediawiki.org/wiki/API:Watchlist), [API:Watch](https://www.mediawiki.org/wiki/API:Watch) and [API:Watchlistraw](https://www.mediawiki.org/wiki/API:Watchlistraw)
- [Manual:Watchlist table](https://www.mediawiki.org/wiki/Manual:Watchlist_table)
- [Wikimedia EventStreams](https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams)
- [Server-sent events](https://html.spec.whatwg.org/multipage/server-sent-events.html) and [Atom (RFC 4287)](https://www.rfc-editor.org/rfc/rfc4287)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §8
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A16).

Replaced text (§8):

> | Crate | Change |
> |---|---|
> | `triplespace-projections` ([0013](0013-postgres-storage.md) §10) | Feed specifications, target-set expansion, the activity query for every set, and related-changes joins |
> | `triplespace-accounts` | `private.watch` and `private.watch_token`, the expiry sweep, auto-watch, and resolving a holder's watch set to targets |
> | `triplespace-api-rest`, `triplespace-api-action` | The routes and modules of §5; Atom and server-sent events |

### A2. Notifications

- **Date:** 2026-09-27
- **Source:** [0021](0021-notifications.md) §2, §7
- **Change:** extends §3, §5
- **Summary:** `private.watch` gains `notify`, which delivers a watched target's changes as notifications; `uiprop=hasmsg` is defined there. The SQL had been given the column in place. This settled Q1.

### A3. Patrolling

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §6
- **Change:** extends §1
- **Summary:** A `patrolled` filter on every feed. This settled Q2.

### A4. Rate limits

- **Date:** 2026-09-27
- **Source:** [0024](0024-subsidiary-accounts.md) §5
- **Change:** amends §7
- **Summary:** Streams and Atom fetches are rate-limited in the `stream` and `atom` classes. This settled the rate-limit half of Q5.

Replaced text (§7):

> No permission reads another account's watch set; there is no `viewwatchlist`-of-others right and no route for one. Rate limits on the stream are open.

### A5. Preferences and portability

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §1, §3–4
- **Change:** amends §3
- **Summary:** The four auto-watch flags move into the preference store, with `autowatch.notify`; the watch set is portable in the user data bundle and the private extract of a move; the period window is the `feeds.period` preference. This settled Q3 and the period half of Q5.

Replaced text (§3):

> - **Auto-watch** is a preference: pages and entities the account edits, pages it creates, threads it posts in, and threads it starts, each on or off. Preferences are private state too and belong beside the watch set; only these four are decided here (open questions).
>
> It does not travel with a tenant ([0018](0018-tenants.md) §10), because nothing private does; a user who moves takes their list with `list=watchlistraw` and re-imports it.

### A6. Farm-wide feeds and the stream across instances

- **Date:** 2026-09-27
- **Source:** [0028](0028-tenancy-policy.md) §5, §9
- **Change:** extends §2, §5; amends §4
- **Summary:** An all-tenants target set, served at the farm base under `feeds.farm_wide`. Cross-instance provider-to-tenant sync follows the stream, filtered to `source = local` and verified ([0022](0022-federation.md) §2); same-instance reading is direct. This settled Q4.

Replaced text (§4):

> It is the same mechanism as `GET /jobs/{id}/events` (0012 §5), and a consumer that missed events resumes from its last token. Whether a consumer tenant should sync from a provider tenant this way instead of by job ([0018](0018-tenants.md) §5) is an open question.

### A7. The filter log is a feed

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §6
- **Change:** extends §2
- **Summary:** Filter hits are a target set, delivered as a page, Atom or stream like any feed.

### A8. The SPARQL Update stream

- **Date:** 2026-09-27
- **Source:** [0032](0032-sparql-update-stream.md) §6
- **Change:** extends §4
- **Summary:** The activity stream is for consumers of change sets and events. A triplestore that wants to follow the resolved view as triples uses the SPARQL Update stream, `GET /updates/stream`, which shares this route's transport and `Last-Event-ID` discipline and carries `DELETE DATA`/`INSERT DATA` bodies instead of activity rows.

### A9. Boards

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §7
- **Change:** amends §2
- **Summary:** A talk page's one-target set includes every thread attached to it, home or listing, and a board's is the board and its threads. A thread that is listed or detached joins or leaves the set at query time, as a moved one does.

Replaced text (§2):

> **A subject and its talk page are one target.** The one-target set for `Item:Q42` includes `Item talk:Q42` and its threads; for `User:Example`, the user page and `User talk:Example`. A thread is its own target, and a thread that moves to another talk page leaves the first target's set and joins the second's at query time, so history and watches follow the talk page, not the thread ([0019](0019-discussions.md) §2).

### A10. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–8
- **Summary:** A1–A9 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A8 and A9 were blockquotes, A2's column had been added in place, and the other entries were recorded in the header, the open questions or other ADRs. The file before conversion is commit `0b26a3a`.

### A11. Scopes as target sets and watches

- **Date:** 2026-10-04
- **Source:** [0060](0060-scopes.md) §7
- **Change:** extends §2, §3
- **Summary:** A Scope target set, joined against `view.scope_member` and bounded by `scopes.max_members`; `scope` and `rows` watch kinds, expanded at query time.

Replaced text (§3, in part):

> - **Target kinds** are `entity`, `page`, `thread` and `actor`.

### A12. Followed talk pages

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §8
- **Change:** extends §2
- **Summary:** A talk page that follows upstream includes the followed page's `put` rows in its one-target set; a foreign thread is a target of its own.

### A13. `notify` is refused on `scope` and `rows` watches

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §3
- **Summary:** `notify` is refused on a `scope` or `rows` watch, with the error `ts-watch-notify-unsupported`: those watches are feed filters that expand at query time, not subscriptions to a target, so the generalization of DiscussionTools' topic subscription stops at the four target kinds that name one thing (`entity`, `page`, `thread`, `actor`). [0060](0060-scopes.md) §7 says the same of its side. (PENDING E31)

Replaced text (§3):

> - **`notify`** marks a watch whose changes are also delivered as notifications ([0021](0021-notifications.md) §2): DiscussionTools' topic subscription, generalized to every watchable target.

### A14. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
