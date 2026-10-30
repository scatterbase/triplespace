# 0021. Notifications

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-27 (A22)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0011](0011-logs.md), [0018](0018-tenants.md), [0022](0022-federation.md), [0028](0028-tenancy-policy.md)

## Context

[0020](0020-change-feeds.md) built the pull side: a feed is activity rows filtered by a target set, and a watchlist is a private filter over them. What remains is push: telling a person that something happened that concerns them, without their asking. [0019](0019-discussions.md) needs it for mentions and replies, and for messages on a user's talk page, which MediaWiki has announced with an orange bar since 2005 and with Echo since 2013.

Echo is the precedent. It has notification *types* (mention, edit-user-talk, reverted, thanks, page-linked, and so on), *channels* (web and email), a per-type per-channel preference matrix, bundling (five replies in one thread are one notification), a distinction between *seen* (the bell was opened) and *read* (the item was), and an API that the Wikipedia apps and user scripts depend on. DiscussionTools adds topic subscription: a notification when a thread you subscribed to gets a post.

[0019](0019-discussions.md) §10 adopted the Activity Streams 2.0 vocabulary. ActivityPub, the protocol built on it, has exactly one mechanism: an actor has an inbox, and an activity addressed to the actor is delivered there. That is what a notification is. This ADR uses the inbox as the model, and takes the smallest step across the network that the model makes available: delivering a person's notifications to their own fediverse account, as direct messages, so that Mastodon can be their notification client. Making users, talk pages and threads followable from the fediverse, and accepting replies from it, are a separate decision ([0022](0022-federation.md), to be written).

## Decision

### 1. A notification is an activity delivered to an inbox

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §5.1.*

### 2. Addressing rules (amends 0020 §3)

*Changed by A3, A5, A6, A8, A9, A10, A11, A13, A14, A17, A18, A19, A20, A21.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §5.2.*

### 3. The inbox is private state (extends 0007 §8, 0013 §4)

*Changed by A4.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §5.3.*

### 4. Channels

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §5.4.*

### 5. The fediverse channel

*Changed by A2.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §5.5.*

### 6. UI (extends 0010 §2 and §11)

*Changed by A16, A20, A21, A22.*

*Current text: [19](../architecture/19-site-ui.md) §1.2, §2.4, §2.6, §6.2.*

### 7. API (extends 0012 §4 and §5)

*Changed by A5, A20.*

*Current text: [18](../architecture/18-api.md) §2.3, §3.2.*

### 8. Storage and operations (extends 0013 §4)

*Changed by A2, A3, A12.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.16, §5.*

### 9. Permissions (extends 0016 §2)

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §8.2.*

### 10. Crates

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **Notifications are the inbox half of ActivityPub**, so [0022](0022-federation.md) adds actors and inbound handling to an existing mechanism rather than a second notification system beside this one.
- **Mastodon works as a notification client today**, with the instance speaking only as much of the protocol as sending requires: WebFinger, signed fetch and delivery, and `Follow`/`Accept` on one actor.
- **A private handle is still a disclosure to another server.** The instance keeps its promise not to show the association; it cannot promise what the recipient's server does with a message. The form says so.
- **Echo's API shape is a compatibility surface** like the Action API's, with the same additive rule and the same obligation to keep it in step.
- **Read state is the second thing that is not rebuildable**, after `seen` on watches, and for the same reason.
- **Two private tables hold contact details** (email, fediverse handle) that did not exist before; the privacy test of [0012](0012-api-requirements.md) §8 now covers them.

## Open questions

- **Q1. Web Push**, and native app push.
- **Q2. Bundling rules and windows**, and whether a digest email should summarize watch notifications the way the watchlist page does.
- **Q3. Temporary accounts**: MediaWiki delivers talk-page messages to temporary accounts by session; whether to do the same.
- **Q4.** ~~**Cross-tenant notifications**: whether a mention of `librarybase:42` from another tenant on the same instance reaches that user's inbox, which would be the first thing to cross a tenant boundary.~~ *Settled by [0028](0028-tenancy-policy.md) §7: by policy, through farm identity; the bell aggregates linked accounts' inboxes and a farm-name mention goes to the home tenant.*
- **Q5.** ~~**RFC 9421 timing**, and FEP-8b32 object integrity proofs, as Mastodon adopts them.~~ *Settled by [0022](0022-federation.md) §8: both implemented behind `site` switches, off until Mastodon accepts them; outbound stays cavage until then.*
- **Q6.** ~~**[0022](0022-federation.md)**: WebFinger and actor documents for users, talk pages as `Group` actors that `Announce` their threads, `Update` and `Delete` delivery, opt-in per account with the vanish page amended, `rel="me"` public links, and, separately, inbound replies as posts attributed to fediverse actors with a `federated` group, domain lists and moderation.~~ *Settled by [0022](0022-federation.md) Part B, which does all of this and fixes the inbound rules while leaving the inbound protocol details open.*

## References

- [Extension:Echo](https://www.mediawiki.org/wiki/Extension:Echo), [Echo API](https://www.mediawiki.org/wiki/Extension:Echo/API) and [Notifications/Types](https://www.mediawiki.org/wiki/Notifications/Types)
- [Extension:DiscussionTools — Topic subscriptions](https://www.mediawiki.org/wiki/Help:DiscussionTools#Topic_subscriptions)
- [ActivityPub](https://www.w3.org/TR/activitypub/) and [Activity Streams 2.0](https://www.w3.org/TR/activitystreams-core/)
- [WebFinger (RFC 7033)](https://www.rfc-editor.org/rfc/rfc7033) and [NodeInfo](https://nodeinfo.diaspora.software/)
- [Mastodon: ActivityPub in Mastodon](https://docs.joinmastodon.org/spec/activitypub/) and [Security (HTTP Signatures)](https://docs.joinmastodon.org/spec/security/)
- [HTTP Message Signatures (RFC 9421)](https://www.rfc-editor.org/rfc/rfc9421)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §10
- **Summary:** 0005 §2 is the one crate table CI checks, and carries the two crates this section added (0005 A17).

Replaced text (§10):

> | Layer | Crate | Contents |
> |---|---|---|
> | Substrate | `scatter-activitypub` (new) | AS2 object building for the profile of [0019](0019-discussions.md) §10 and this ADR, actor documents, WebFinger and NodeInfo documents, HTTP Signatures (signing and verification), and inbox activity parsing. Pure; no I/O. Written so that [0022](0022-federation.md) adds actors, not a new stack |
> | Triplespace | `triplespace-notify` (new) | The addressing projection of §2, inbox storage, bundling, the channels of §4 and the delivery queue |
> | | `triplespace-accounts` | `private.email`, `private.fediverse_handle`, `private.ap_key`, verification flows |
> | | `triplespace-api-rest`, `triplespace-api-action` | §7, including the Echo-shaped modules and the ActivityPub and well-known endpoints |

### A2. Federation

- **Date:** 2026-09-27
- **Source:** [0022](0022-federation.md) §6–8, §10, §13
- **Change:** extends §5, §8
- **Summary:** Per-actor keys, opt-in Person and Group actors beside the notifier, WebFinger for opted-in users, RFC 9421 and FEP-8b32 behind switches, and ActivityPub delivery moved to `triplespace-federation`. §8 had been given the per-actor keys in place. This settled Q5 and Q6.

Replaced text (§5):

> using the HTTP Signatures profile Mastodon accepts (draft-cavage; RFC 9421 when Mastodon does)
>
> WebFinger answers for local users, and actor documents for them, are [0022](0022-federation.md)'s to decide; until then a lookup of `acct:example@{host}` returns 404.

Replaced text (§8):

> Since [0022](0022-federation.md) §10 the last two are keyed by actor, the notifier being one actor among the opted-in ones, and are read by `triplespace-federation`. The delivery queue for email and fediverse messages lives in `ops`, with attempts, next attempt and last error per item. Outbound requests go through the same egress path and rate limits as live upstream fetches ([0012](0012-api-requirements.md) §6), and the `notifications.fediverse_domains` allow and deny lists are site configuration.

### A3. Subsidiaries

- **Date:** 2026-09-27
- **Source:** [0024](0024-subsidiary-accounts.md) §5–6
- **Change:** extends §2, §8
- **Summary:** A subsidiary's `rights` and `talk` notifications reach its operator, as its `job` and `mention` ones already did; outbound notification requests an actor initiates are rate-limited in the `notify` class.

Replaced text (§2):

> - **Nobody is notified of their own action.** A bot's operator is notified of the bot's jobs and of mentions of the bot.

### A4. The preference matrix is preferences

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §1–4, §8
- **Change:** amends §3
- **Summary:** `private.notification_pref` is dropped; the reason-by-channel matrix is the set of `notifications.{reason}.{channel}` keys in `private.preference`, and the addressing projection reads those. `private.inbox` and `private.inbox_state` stand. The inbox is portable state (0027 §2): it travels in the user data bundle and the private extract of a cooperative tenant move.

Replaced text (§3):

> CREATE TABLE private.inbox_state (actor_key text PRIMARY KEY, seen_at timestamptz);
> CREATE TABLE private.notification_pref (
>   actor_key text NOT NULL, reason text NOT NULL, channel text NOT NULL, enabled boolean NOT NULL,
>   PRIMARY KEY (actor_key, reason, channel)
> );
>
> The rules of [0013](0013-postgres-storage.md) §4 apply: readable by the accounts role only, never exported, never in a tenant bundle ([0018](0018-tenants.md) §10) except as [0027](0027-preferences-and-portability.md) §4 carries it.

### A5. Cross-tenant notifications

- **Date:** 2026-09-27
- **Source:** [0028](0028-tenancy-policy.md) §7, §11
- **Change:** extends §2; amends §7
- **Summary:** Under `notifications.cross_tenant = home`, the bell aggregates linked accounts' inboxes and a farm-name mention goes to the home tenant; `notwikis` gains its real meaning. This settled Q4.

Replaced text (§7):

> Echo's `notwikis` cross-wiki parameter is accepted and ignored.

### A6. The filter reason

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §4
- **Change:** extends §2
- **Summary:** An edit filter's `notify` action delivers the hit to the inbox of every member of a named group, as a reason `filter`.

### A7. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §2–10
- **Summary:** A1–A6 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A4 was a blockquote, part of A2 had been written into §8 in place, and the other entries were recorded in the header, the open questions or other ADRs. The file before conversion is commit `0b26a3a`.

### A8. Restricted targets address nobody

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §6
- **Change:** extends §2
- **Summary:** No notification is delivered for a target the addressed account may not read under a confidential restriction, and one already delivered is purged when the target becomes restricted; a watch on it stays and goes quiet.

### A9. `task-resolved`

- **Date:** 2026-10-04
- **Source:** [0061](0061-sprints-and-tasks.md) §8
- **Change:** extends §2
- **Summary:** A `task-resolved` reason for the claimant of a task resolved by someone else, under the `*` matrix defaults.

### A10. `proposal-state`

- **Date:** 2026-10-05
- **Source:** [0067](0067-proposals.md) §5
- **Change:** extends §2
- **Summary:** A reason for the proposer when a proposal's state changes.

### A11. Replies upstream

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §8
- **Change:** extends §2
- **Summary:** `reply` also addresses a person when a synced upstream comment answers one by the upstream account their grant acts as.

### A12. The delivery queue is `ops.delivery`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §8
- **Summary:** The delivery queue for email and fediverse messages is one `ops` table, and §8 gives it its name: `ops.delivery` (proposed, since this ADR never named it), with attempts, next attempt and last error per item. [0022](0022-federation.md) §10 uses that name in place of its `private.ap_outbox_queue`, so ActivityPub fan-out and the notifier's deliveries share one queue. The ledger verb is corrects, but §8 lacked a name rather than stating a wrong one, so this entry extends §8 and contradicts no text of it. (PENDING A8)

### A13. The `rename` reason

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §2
- **Summary:** A `rename` reason joins the table of §2: the renamed account is addressed when its actor record changes its name ([0007](0007-actor-identity.md) §4); the performer is the renamer or, for a rename the instance carries out, the instance operator ([0040](0040-instance-prerogatives.md) §7). Echo has no name for it. (PENDING E32)

### A14. The `watch` reason stops at single targets

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §2
- **Summary:** The `watch` reason is not "generalized to every watchable target": `notify` is refused on a `scope` or `rows` watch with `ts-watch-notify-unsupported`, because those watches are feed filters that expand at query time, not subscriptions ([0020](0020-change-feeds.md) §3, 0020 A13; [0060](0060-scopes.md) §7). The reason addresses an account only for `entity`, `page`, `thread` and `actor` watches with `notify` set. (PENDING E31)

Replaced text (§2):

> | `watch` | A change reaches a target it watches with **`notify`** set. `private.watch` ([0020](0020-change-feeds.md) §3) gains a `notify boolean NOT NULL DEFAULT false`. This is DiscussionTools' topic subscription, generalized to every watchable target, and it inherits the subject-and-talk pairing and the sync default of 0020 §2 | (DiscussionTools subscription) |

### A15. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A16. The notification matrix moves to Special:Preferences

- **Date:** 2026-10-17
- **Source:** [0085](0085-settings-and-preferences.md) §5
- **Change:** amends §6
- **Summary:** The account page's Notifications section is split. The matrix of reasons by channel and the digest are the Notifications tab of `Special:Preferences`; the email address and fediverse handle with their verification state, the *follow the notifier* hint and *Remove* are the **Contacts** section of `Special:Account`, where `ChangeEmail`, `Confirmemail` and `Invalidateemail` lead. The chapter text is [19](../architecture/19-site-ui.md) §2.4 and §2.6.

Replaced text ([19](../architecture/19-site-ui.md) §2.4, as it stood):

> | Notifications | Private | The preference matrix of reasons by channel, read and written in place as the `notifications.*` keys; the email address with its verification state; the fediverse handle with its verification state, a *follow the notifier* hint with the notifier's handle, and *Remove* ([0021](0021-notifications.md) §6) |

### A17. `group-join`

- **Date:** 2026-10-22
- **Source:** Direct: James, design discussion of 2026-10-22
- **Change:** extends §2
- **Summary:** A new reason, `group-join`: a member of a group that a confidential `read` restriction in force names is notified when another account joins the group, by a membership record or because a group came to include it, a self-grant under `userrights` included. It is the notice that makes `userrights` the accountable way into a confidential group ([0056](0056-security-model.md) A18). The reason × channel preferences cover it through their `notifications.*` patterns.

### A18. `group-join` covers joining an including group

- **Date:** 2026-10-23
- **Source:** [0016](0016-permissions-and-access-control.md) A37
- **Change:** amends §2
- **Summary:** Joining `auditor`, or `owner` with its default `includes`, gives the reading of every confidential group without a membership in any of them, so `group-join` also fires for the members of each group a confidential restriction names when an account joins a group that includes it.

Replaced text ([16](../architecture/16-logs-feeds-and-notifications.md) §5.2, as it stood):

> and another account joined the group, by a membership record or because a group came to include it; a self-grant under `userrights` included

### A19. `group-join` is the group's choice

- **Date:** 2026-10-23
- **Source:** [0016](0016-permissions-and-access-control.md) A38
- **Change:** amends §2
- **Summary:** `group-join` no longer depends on a confidential restriction naming the group, nor reaches through `includes` (A18): it fires for the members of a group whose record sets `notify_joins = true` when anyone joins that group, by any route. It is off unless set, and every shipped group, `read-all` among them, leaves it off.

Replaced text ([16](../architecture/16-logs-feeds-and-notifications.md) §5.2, as it stood):

> | `group-join` | It is a member of a group that a confidential `read` restriction in force names, and another account joined the group, by a membership record, by joining a group that includes it (`auditor`, or `owner` with its default `includes`), or because a group came to include it; a self-grant under `userrights` included ([0021](0021-notifications.md) A18) ([09](../architecture/09-security-and-moderation.md) §4.5) | — |

### A20. Thread notifications, the read check, and the bell

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-25
- **Change:** amends §2, §6; extends §7
- **Summary:** The reasons `talk`, `reply`, `mention` and `thread-status` are built. James chose four things: `watch` waits for the watch set and its controls ("Defer with the watchlist"); what a reader may no longer see leaves the inbox both ways ("Read-time check + purge"): the addressing projection deletes the rows of a record a `record` ACL hides in any part and, on a `read` ACL on a thread, the rows of those who may no longer read it, and every inbox read leaves out an item about a thread the reader may not read now, whose talk page is hidden from them (a confidential page's or a hidden user's), or whose record is hidden or erased, its counts included; the site gets the bell, `Special:Notifications`, the orange bar and new topics on user talk pages; and an edit addresses only the mentions it adds while imported threads and posts address nobody. Settled in the building, for James to confirm: an account is addressed once per record, for the first of `talk`, `thread-status`, `reply`, `mention`; a mention is a link to a user page itself, not a subpage, a markdown post's `[[wiki links]]` and a wikitext post's links as written with templates unexpanded, at most 50 per post (Echo's `$wgEchoMaxMentionsCount`); `talk` is a new thread or post, not an edit; `thread-status` addresses the first post's author, on a move from an open category to a closed one; a reply to an imported post addresses nobody; a thread notification reaches a registered account or a bot's operator and no temporary account while Q3 is open; a subsidiary's action is its operator's own. `talk` is Echo's `message` section and every other reason `alert`; `meta=notifications` takes `notgroupbysection` and counts to 100 after the read check; `echomarkread` takes `sections`. The bell is a link to `Special:Notifications` with the unread count, marked while something unread is newer than `seen_at`; the panel waits for JavaScript; opening the inbox sets `seen_at`; reading one's own talk page marks its `talk` notifications read; the orange bar is not drawn on that page. Chapter text: [16](../architecture/16-logs-feeds-and-notifications.md) §5.2; [19](../architecture/19-site-ui.md) §6.2; [18](../architecture/18-api.md) §2.3, §2.4.

Replaced text ([16](../architecture/16-logs-feeds-and-notifications.md) §5.2, as it stood):

> **Nobody is notified of their own action.** A subsidiary has no inbox of its own: its operator is notified of its jobs, of mentions of it, of its `rights` changes, and of `talk` messages on its user talk page, since that is who can answer them ([07](../architecture/07-actors-and-accounts.md) §4.6).

> **Only local accounts have inboxes.** A mention of a foreign actor, such as a Wikidata editor's IRI, addresses nobody here.

> **Hidden and erased content is not delivered**, and an already-delivered notification whose activity is later hidden or erased is removed from every inbox by the same purge that clears caches ([03](../architecture/03-storage-caches-and-search.md) §9.6).

> and a delivered notification whose target becomes restricted is removed by the same purge ([09](../architecture/09-security-and-moderation.md) §5.3).

Replaced text ([19](../architecture/19-site-ui.md) §6.2, as it stood):

> **The bell** in the global header shows the unseen count and opens the inbox as a panel; opening it sets `seen_at`. Items link to their permalink, and each has mark-read.

### A21. `watch` notifications, the star and the watchlist pages

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-25
- **Change:** extends §2; amends §6
- **Summary:** The `watch` reason is built ([0020](0020-change-feeds.md) A18). James chose threads, talk pages and document pages for this round ("Threads and pages"), entities waiting for a decision on mirror syncs. A new thread or post reaches the watchers with `notify` of the thread and of the subject of every page it is attached to; any record of a document page reaches the watchers of the page and of its title. `watch` comes last in A20's order, so an account a thread record already addresses is not addressed again; the recipient's reading is checked as for every thread notification. Its Echo type is `ts-watch`, an alert. The site's watch controls are the star and a **Watch for…** menu with the spans and **Notify me of changes**, and `Special:Watchlist` and `Special:EditWatchlist` are built and linked from the account menu. Chapter text: [16](../architecture/16-logs-feeds-and-notifications.md) §5.2; [19](../architecture/19-site-ui.md) §6.2.

Replaced text ([19](../architecture/19-site-ui.md) §6.2, as it stood):

> **Watch controls** gain a *notify me* toggle beside the star, which sets `notify` on the watch row; subscribing to a thread is watching it with `notify` ([16](../architecture/16-logs-feeds-and-notifications.md)).

### A22. One watch menu

- **Date:** 2026-10-27
- **Source:** Direct: James, design discussion of 2026-10-27
- **Change:** amends §6
- **Summary:** James asked that the **Watch for…** menu and the watch button be merged into one button, sitting toward the right with **More**. The watch control is one menu at the end of the tabs, before **More**: its button is the star with **Watch**, or a filled star with **Watching**; it opens the spans and **Notify me of changes**, submitted with **Watch** (or **Save** for a watched title), and a watched title's menu also has **Unwatch**. Watching permanently takes two clicks where the star took one. A lapsing watch says until when in the menu and on the button's hover. **More** is the size of the watch button, as the other tab-bar controls are. Chapter text: [19](../architecture/19-site-ui.md) §6.2.

Replaced text ([19](../architecture/19-site-ui.md) §6.2, as it stood):

> - **Watch controls** are the star at the end of the tabs of a page, an entity, a talk page or a thread, and a **Watch for…** menu beside it, both forms that work without JavaScript: the star watches permanently or stops watching; the menu watches for a week, a month, three months, six months or permanently, with **Notify me of changes**, which sets `notify` on the watch row. A watch that lapses says until when. Subscribing to a thread is watching it with `notify` ([16](../architecture/16-logs-feeds-and-notifications.md) §4.3; [0021](../decisions/0021-notifications.md) A21).
