# 0020. Change feeds: recent changes, related changes and watchlists

- **Status:** Proposed
- **Date:** 2026-09-27
- **Author:** James Hare / Claude Fable
- **Amended by:** [0021 — Notifications](0021-notifications.md) (§2 adds `notify` to `private.watch`; settles the notifications open question), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§6 extends §1 with the `patrolled` filter; settles the patrolling open question), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§5 settles the rate-limit open question with the `stream` and `atom` classes), [0027 — Preferences, private state and portability](0027-preferences-and-portability.md) (§1 moves the auto-watch flags of §3 into the preference store; §3–4 make the watch set portable; settles the preferences open question), [0028 — Tenancy policy](0028-tenancy-policy.md) (§9 extends §2 with the all-tenants target set; §5 settles the provider-to-tenant sync question), [0030 — Edit filters](0030-edit-filters.md) (§6 extends §2 with the filter-hits set), [0032 — The SPARQL Update stream](0032-sparql-update-stream.md) (§6 amends §4: a second stream, of triples for triplestores, on the same transport; the activity stream is unchanged)
- **Related:** [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (§2), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md) (§2), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§8 amends §2), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (§4), [0007 — Actor identity](0007-actor-identity.md) (§3 follows §8), [0010 — Site UI](0010-site-ui.md) (§2 uses §5 and §7; §4 extends §11; settles the watchlists open question), [0011 — Upstream and local logs](0011-logs.md) (§4), [0012 — API requirements for the site UI](0012-api-requirements.md) (§1 uses §2.3 and §3; §5 extends §4 and §5; §4 settles the change-feed open question), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§3 and §6 extend §4 and §5), [0014 — Cache layers and search](0014-caches-and-search.md) (§6), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§7 extends §2), [0018 — Tenants](0018-tenants.md) (§3 follows §10), [0019 — Discussions](0019-discussions.md) (§3 settles §8's watching), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§1 publishes the stream of §4 for verified sync)

## Context

MediaWiki has four features that list changes: page history, recent changes, related changes (`Special:RecentChangesLinked`) and the watchlist. It implements them with three tables and two joins: history reads `revision`, recent changes reads `recentchanges`, related changes joins `recentchanges` to `pagelinks`, and the watchlist joins it to `watchlist`. They are one feature with four target sets.

Triplespace has already folded the tables. [0012](0012-api-requirements.md) §3 gave history, recent changes, contributions and the log one row shape; [0013](0013-postgres-storage.md) §5 made `view.activity` one table, indexed by time, actor and target; [0010](0010-site-ui.md) §5 defined a history as every record keyed to a target or a member of its cluster. What is left is to name the abstraction, add the two target sets that do not exist yet, and answer the two questions the earlier ADRs left open: how watchlists work, and whether a watch on a mirrored entity covers its syncs ([0010](0010-site-ui.md)); and whether there is a change feed for downstream consumers ([0012](0012-api-requirements.md)). [0019](0019-discussions.md) §8 also promised that watching a talk page covers its threads.

One constraint is fixed by James: **whether a user is watching something is private and is not part of the graph.** It is not a record in any partition, logged or otherwise.

## Decision

### 1. A feed is activity rows filtered by a target set

A **feed** is defined by three things:

- a **target set** (§2): which entities, pages, threads and talk pages it covers;
- **filters**: the row kinds of [0010](0010-site-ui.md) §7 (edits, page edits, jobs, log actions, mirror syncs), namespace, actor, the bot, minor and new flags, a period, and for target-scoped feeds a **syncs** switch (§2);
- a **delivery** (§4): a page, an Atom document or a live stream.

Its rows are the activity rows of [0012](0012-api-requirements.md) §3, redacted for the viewer as 0012 §8 requires, ordered by (time, partition, offset) with the continuation tokens of 0012 §2.3. Contributions are the actor filter over the everything set, so they are a feed too.

**Every list of changes the UI or the API shows is one of these.** No feature builds its own list. That is the rule the rest of this ADR follows, and the rule that keeps a history, a watchlist and the downstream stream from drifting apart.

### 2. Target sets

| Set | Members | Source |
|---|---|---|
| **Everything** | every local record | — |
| **One target** | an entity and the members of its cluster; a document page; a thread; a talk page and the threads attached to it; an actor's user page and user talk page | `cluster_member` ([0013](0013-postgres-storage.md) §5), `view.thread` ([0019](0019-discussions.md) §7) |
| **Related, outward** | for a document page or thread, what it links to; for an entity, the entities its statements, qualifiers and references point at, and the properties they use | `page_link`; the resolved JSON |
| **Related, inward** | for a page or entity, what links to or references it | `page_link_target`, `entity_ref` |
| **Watched** | the account's watch set (§3), each member expanded as a *one target* set | `private.watch` |

**A subject and its talk page are one target.** The one-target set for `Item:Q42` includes `Item talk:Q42` and its threads; for `User:Example`, the user page and `User talk:Example`. A thread is its own target, and a thread that moves to another talk page leaves the first target's set and joins the second's at query time, so history and watches follow the talk page, not the thread ([0019](0019-discussions.md) §2).

**Related changes are a join, not a list.** The relation is evaluated inside the query against the period window, as MediaWiki joins `pagelinks`, and is never materialized. A relation with more than `feeds.related_limit` members (site configuration, default 5,000) is truncated to that many, and the feed says so. This is the one place the model has a scale edge, and it is MediaWiki's edge too: inward relations of `Q5` are unbounded, and nobody's related-changes page can be.

**Mirror syncs in target-scoped feeds.** Recent changes has no per-entity sync rows, because a sync of a million entities is one job row ([0010](0010-site-ui.md) §7). A history does show them, because it is scoped to one target and draws its sync rows from that target's mirror records ([0010](0010-site-ui.md) §5). A watchlist is scoped to the watch set, so its cost is bounded the same way, and it shows them too. The `syncs` filter therefore defaults **on** for a watchlist and a history, **off** for recent changes and related changes. That settles [0010](0010-site-ui.md)'s question: a watch on a mirrored entity covers its syncs, because the person chose that entity, and "Wikidata changed it" is the thing they most want to know. They can turn it off.

### 3. The watch set is private state, not records (extends 0007 §8, 0013 §4)

**A watch is a row in the `private` schema**, not a record in any partition. It is the same kind of thing as a session or an OAuth token: state that belongs to an account, that nobody else may see, and that has no public history. It follows every rule [0007](0007-actor-identity.md) §8 and [0013](0013-postgres-storage.md) §4 set for that schema: never projected, never exported, never placed on a feed, readable only by the accounts role, excluded from every bundle.

```sql
CREATE TABLE private.watch (
  actor_key text NOT NULL, target_kind smallint NOT NULL, target_id text NOT NULL,
  added timestamptz NOT NULL, expires timestamptz,
  seen timestamptz,                -- MediaWiki's wl_notificationtimestamp: newest change the holder has seen
  notify boolean NOT NULL DEFAULT false,   -- deliver changes as notifications (0021 §2)
  PRIMARY KEY (actor_key, target_kind, target_id)
);
CREATE INDEX watch_expiry ON private.watch (expires) WHERE expires IS NOT NULL;
CREATE TABLE private.watch_token (actor_key text PRIMARY KEY, token text NOT NULL, issued timestamptz NOT NULL);
```

- **Target kinds** are `entity`, `page`, `thread` and `actor`. An entity watch stores the entity ID and is expanded through its cluster when the feed is read, so a merge or a redirect never loses it. A `page` watch is a document page; a `thread` watch is a thread; an `actor` watch is the actor's user page and user talk page. Watching a talk page is watching its subject, and watching a subject watches its talk page (§2): the UI's watch star on either page toggles the same row.
- **Expiry** is optional, with MediaWiki's choices (a week, a month, three months, six months, permanent). A daily sweep deletes expired rows.
- **`seen`** is updated when the holder reads the target or the watchlist, and drives the "changed since you last looked" mark. It is written often and matters little, which is why it lives here and not in a record.
- **Auto-watch** is a preference: pages and entities the account edits, pages it creates, threads it posts in, and threads it starts, each on or off. Preferences are private state too and belong beside the watch set; only these four are decided here (open questions).
- **Watcher counts are never shown**, to anyone, at any threshold. MediaWiki shows a count once it passes thirty; the accounts graph's promise is stricter and this follows it. `prop=info` returns `watched` and `notificationtimestamp` for the viewer's own watch and no `watchers` field.

**What this costs.** The watch set is not in the log, so it is not rebuilt from it: it survives on the strength of `private`'s backups, like sessions and bindings. It does not travel with a tenant ([0018](0018-tenants.md) §10), because nothing private does; a user who moves takes their list with `list=watchlistraw` and re-imports it. Rehosting an instance whole carries it, as 0018 §10 says of the `private` schema.

**Serving respects the schema boundary.** `triplespace-accounts` is the only code that reads `private.watch`. It resolves the holder's watch set to a list of targets and hands that list to the activity query, which runs under the public role against `view` alone. The join happens in the query as a target-set parameter, never as a cross-schema join.

### 4. Delivery: page, Atom, stream (settles 0012's open question)

Every feed can be delivered three ways.

| Delivery | Everything | One target | Related | Watched |
|---|---|---|---|---|
| **Page** | `Special:RecentChanges` | the History tab | `Special:RecentChangesLinked/{title}`, with a *changes to pages linking here* switch | `Special:Watchlist`, labelled **Private** ([0010](0010-site-ui.md) §1, principle 6) |
| **Atom** | `?feed=atom` on each, as MediaWiki offers | | | with a per-account token (`private.watch_token`), shown and resettable in `Special:Account` |
| **Stream** | `GET /activity/stream` for any feed specification, as server-sent events, with `Last-Event-ID` carrying the continuation token | | | authenticated, never by token |

**The stream is the downstream change feed.** [0012](0012-api-requirements.md) asked whether Triplespace offers an EventStreams-like stream and whether it follows the same redaction and log model. It does: the stream is the everything feed delivered live, its events are activity rows redacted exactly as any response is, and it carries `erased` rows and visibility changes so that a consumer can follow an erasure as [0011](0011-logs.md) §5 has this instance follow upstream. It is the same mechanism as `GET /jobs/{id}/events` (0012 §5), and a consumer that missed events resumes from its last token. Whether a consumer tenant should sync from a provider tenant this way instead of by job ([0018](0018-tenants.md) §5) is an open question.

> **Amended by [0032](0032-sparql-update-stream.md).** The activity stream is for consumers of change sets and events. A triplestore that wants to follow the resolved view as triples uses the SPARQL Update stream, `GET /updates/stream`, which shares this route's transport and `Last-Event-ID` discipline and carries `DELETE DATA`/`INSERT DATA` bodies instead of activity rows.

**Pages may go live.** A feed page has a *live updates* switch that subscribes to its own stream, as MediaWiki's recent changes does.

### 5. API (extends 0012 §4 and §5)

**Action API**, additively under [0012](0012-api-requirements.md) §1, with MediaWiki's meaning: `list=watchlist` (`wlprop`, `wlshow`, `wltype`, `wlexcludeuser`, and `wlowner` with `wltoken` for the Atom token), `list=watchlistraw`, `action=watch` and `unwatch` with `expiry`, `action=setnotificationtimestamp`, `prop=info&inprop=watched|notificationtimestamp`, and `meta=userinfo&uiprop=hasmsg` deferred to the notifications ADR. `list=recentchanges` and `list=usercontribs` are unchanged. MediaWiki has no module for related changes, so the REST route is the only one.

**REST**, under `rest.php/triplespace/v0`:

| Route | Meaning |
|---|---|
| `GET /activity` | Gains `target={kind}:{id}`, `related={kind}:{id}&direction=out\|in`, `set=watchlist`, and `syncs=on\|off`; the existing `kind`, namespace and period filters apply to every set |
| `GET /activity/stream` | The same parameters, as server-sent events |
| `GET /watchlist` | The holder's watch set with expiries |
| `PUT /watchlist/{kind}/{id}`, `DELETE …` | Watch, with an optional `expires`; unwatch |
| `POST /watchlist/seen` | Update `seen` for one target or all |
| `GET /watchlist/token`, `POST /watchlist/token` | Read and reset the Atom token |

Watchlist responses carry `Cache-Control: private` and never enter the shared caches ([0014](0014-caches-and-search.md) §1). The everything feed's page and Atom forms are public and cache briefly at L2.

### 6. Storage and cost (extends 0013 §5)

The everything, one-target and watched sets are served by `view.activity`'s `activity_time` and `activity_target` indexes, with cluster and thread expansion done before the query. A watch set of tens of thousands of targets, which experienced MediaWiki editors have, is passed as an array parameter and bounded by the period window; this is the join MediaWiki runs on every watchlist load. Sync rows for a watched mirrored entity are read per target from its mirror records, as a history reads them. Related sets are joins against `page_link_target` or `entity_ref` under the same window and the limit of §2. `private.watch` is small per account and never scanned across accounts.

### 7. Permissions (extends 0016 §2)

| Permission | Governs | Default groups |
|---|---|---|
| `viewmywatchlist`, `editmywatchlist` | Reading and changing one's own watch set, token and `seen` marks | `user` |
| `read` | Every public feed, including the stream | `universe` |

No permission reads another account's watch set; there is no `viewwatchlist`-of-others right and no route for one. Rate limits on the stream are open.

### 8. Crates

| Crate | Change |
|---|---|
| `triplespace-projections` ([0013](0013-postgres-storage.md) §10) | Feed specifications, target-set expansion, the activity query for every set, and related-changes joins |
| `triplespace-accounts` | `private.watch` and `private.watch_token`, the expiry sweep, auto-watch, and resolving a holder's watch set to targets |
| `triplespace-api-rest`, `triplespace-api-action` | The routes and modules of §5; Atom and server-sent events |

## Consequences

- **One query path.** History, recent changes, related changes, the watchlist, contributions and the downstream stream are one code path over one table with one row shape, and they cannot disagree about what a change is or who may see it.
- **A watch is a private filter, never data.** Nobody, including the instance's own graph, records who watches what. The price is that the watch set is not rebuildable from the log and does not follow a tenant when it moves.
- **Watching a mirrored entity includes its upstream changes,** by default, which is the answer 0010 was waiting for.
- **No watcher counts,** which MediaWiki tools occasionally use to gauge attention. Here attention is private.
- **The downstream feed exists without a new model**, and it carries erasures, so a consumer can be as faithful to this instance as this instance is to Wikidata.
- **Related changes for entities are new territory.** MediaWiki's are link-based; here the outward relation is the statement graph, which is what a Wikibase editor actually means by "related".

## Open questions

- ~~**Notifications.** Mentions, replies, new messages on one's talk page, email and digests: a feed with push delivery and per-user read state, deserving its own ADR now that the pull side exists.~~ *Settled by [0021](0021-notifications.md), which adds a `notify` flag to `private.watch`.*
- ~~**Patrolling.**~~ *Settled by [0023](0023-moderation.md) §6: a `patrolled` filter on every feed.*
- ~~**Preferences.** Where the rest of an account's preferences live; this ADR puts four in `private` beside the watch set.~~ *Settled by [0027](0027-preferences-and-portability.md) §1: one `private.preference` store with a registered key set; the four auto-watch flags move there.*
- ~~**Provider-to-tenant sync over the stream** instead of by job ([0018](0018-tenants.md) §5).~~ *Settled by [0028](0028-tenancy-policy.md) §5 and [0022](0022-federation.md) §2: same-instance reading is direct; cross-instance reading is a sync job that follows the stream filtered to `source = local`, verified against the provider's checkpoints by default.*
- ~~**Rate limits** on the stream and on Atom, and the default period window per feed.~~ *Rate limits: settled by [0024](0024-subsidiary-accounts.md) §5 (`stream`, `atom` classes). The period window is the `feeds.period` preference of [0027](0027-preferences-and-portability.md) §1, with its default in `preferences.toml`; the default's value is tuning.*
- **Watching a namespace or a query.** Dynamic watch sets defined by a SPARQL query or a search, as some wikis have wanted.
- **Related changes for keyed types.** Whether a domain's related set is its DNS parents and children ([0009](0009-keyed-entity-types-and-domain.md)).

## References

- [Help:Watchlist](https://www.mediawiki.org/wiki/Help:Watchlist), [Help:Watchlist expiry](https://www.mediawiki.org/wiki/Help:Watchlist_expiry) and [Help:Related changes](https://www.mediawiki.org/wiki/Help:Related_changes)
- [API:Watchlist](https://www.mediawiki.org/wiki/API:Watchlist), [API:Watch](https://www.mediawiki.org/wiki/API:Watch) and [API:Watchlistraw](https://www.mediawiki.org/wiki/API:Watchlistraw)
- [Manual:Watchlist table](https://www.mediawiki.org/wiki/Manual:Watchlist_table)
- [Wikimedia EventStreams](https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams)
- [Server-sent events](https://html.spec.whatwg.org/multipage/server-sent-events.html) and [Atom (RFC 4287)](https://www.rfc-editor.org/rfc/rfc4287)
