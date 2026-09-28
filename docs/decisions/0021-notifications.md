# 0021. Notifications

- **Status:** Proposed
- **Date:** 2026-09-27
- **Author:** James Hare / Claude Fable
- **Amended by:** [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§6 confirms §2's routing of a bot's notifications to its operator and adds `rights` and `talk` for subsidiaries; §5 settles the rate-limit open question with the `notify` class), [0027 — Preferences, private state and portability](0027-preferences-and-portability.md) (§1 amends §3: the preference matrix is `notifications.{reason}.{channel}` preferences and `notification_pref` is dropped; §2–4 make contacts and the inbox portable), [0028 — Tenancy policy](0028-tenancy-policy.md) (§7 settles the cross-tenant notification open question; §11 gives `notwikis` its meaning), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§6–8 extend §5: per-actor keys, Person and Group actors, inbound replies; §13 moves ActivityPub delivery to `triplespace-federation`; settles the 0022 open question)
- **Related:** [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§10 amends §2), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (§5 uses the instance key model of §6), [0007 — Actor identity](0007-actor-identity.md) (§2 uses §2; §5 follows §8), [0010 — Site UI](0010-site-ui.md) (§6 extends §2 and §11; settles the notifications half of the watchlists open question), [0011 — Upstream and local logs](0011-logs.md) (§2 uses §6), [0012 — API requirements for the site UI](0012-api-requirements.md) (§7 extends §4 and §5), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§3 and §8 extend §4), [0014 — Cache layers and search](0014-caches-and-search.md) (§2 extends §5: erasure purges inboxes), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§9), [0018 — Tenants](0018-tenants.md) (§5 follows §10), [0019 — Discussions](0019-discussions.md) (§2 settles the notifications open question; §4 uses §10), [0020 — Change feeds](0020-change-feeds.md) (§1 builds on §1; §2 amends §3; settles the notifications open question)

## Context

[0020](0020-change-feeds.md) built the pull side: a feed is activity rows filtered by a target set, and a watchlist is a private filter over them. What remains is push: telling a person that something happened that concerns them, without their asking. [0019](0019-discussions.md) needs it for mentions and replies, and for messages on a user's talk page, which MediaWiki has announced with an orange bar since 2005 and with Echo since 2013.

Echo is the precedent. It has notification *types* (mention, edit-user-talk, reverted, thanks, page-linked, and so on), *channels* (web and email), a per-type per-channel preference matrix, bundling (five replies in one thread are one notification), a distinction between *seen* (the bell was opened) and *read* (the item was), and an API that the Wikipedia apps and user scripts depend on. DiscussionTools adds topic subscription: a notification when a thread you subscribed to gets a post.

[0019](0019-discussions.md) §10 adopted the Activity Streams 2.0 vocabulary. ActivityPub, the protocol built on it, has exactly one mechanism: an actor has an inbox, and an activity addressed to the actor is delivered there. That is what a notification is. This ADR uses the inbox as the model, and takes the smallest step across the network that the model makes available: delivering a person's notifications to their own fediverse account, as direct messages, so that Mastodon can be their notification client. Making users, talk pages and threads followable from the fediverse, and accepting replies from it, are a separate decision ([0022](0022-federation.md), to be written).

## Decision

### 1. A notification is an activity delivered to an inbox

Every local account has an **inbox**. An activity row ([0012](0012-api-requirements.md) §3, [0020](0020-change-feeds.md) §1) is delivered to an inbox when an **addressing rule** (§2) says the account is a recipient. A notification is one inbox row: the activity, the recipient, the reason it was addressed, and its seen and read state. In AS2 terms, the reasons of §2 compute the activity's `to` and `cc`; the inbox is the account's `as:inbox`.

**Addressing is a projection.** It runs over activity rows after they are written, like every other projection ([0013](0013-postgres-storage.md) §7), and is idempotent on (activity, recipient, reason). Nothing about who was notified is a record: an inbox is private state (§3), for the reason the watch set is.

**The inbox is a feed** in 0020's sense: rows ordered by (time, partition, offset), with a per-row read state and a per-account seen time, delivered as a page, a bell, an email, or a fediverse message (§4).

### 2. Addressing rules (amends 0020 §3)

| Reason | An account is addressed when | Echo's name |
|---|---|---|
| `mention` | A post ([0019](0019-discussions.md)) or a document page revision links to its user page with `[[User:Name]]`, resolved by the title resolver; in AS2 terms, an `as:Mention` tag whose `href` is the actor IRI ([0007](0007-actor-identity.md) §2) | mention |
| `reply` | A post's `inReplyTo` names a post attributed to it | (DiscussionTools) |
| `talk` | A thread is created on, or a post is added to a thread attached to, its user talk page: the thread's target is `actor:{its key}` ([0019](0019-discussions.md) §2) | edit-user-talk |
| `watch` | A change reaches a target it watches with **`notify`** set. `private.watch` ([0020](0020-change-feeds.md) §3) gains a `notify boolean NOT NULL DEFAULT false`. This is DiscussionTools' topic subscription, generalized to every watchable target, and it inherits the subject-and-talk pairing and the sync default of 0020 §2 | (DiscussionTools subscription) |
| `job` | A job it ran, or that ran on its behalf as operator ([0007](0007-actor-identity.md) §6), finished, failed or was reverted ([0011](0011-logs.md) §6.3) | — |
| `rights` | Its group memberships changed, or it was blocked or unblocked ([0016](0016-permissions-and-access-control.md) §3) | user-rights |
| `thread-status` | A thread it started was marked resolved, rejected or otherwise closed ([0019](0019-discussions.md) §6) | — |

- **Nobody is notified of their own action.** A bot's operator is notified of the bot's jobs and of mentions of the bot.
- **Only local accounts have inboxes.** A mention of a foreign actor, such as a Wikidata editor's IRI, addresses nobody here.
- **Auto-subscription** follows 0020 §3's auto-watch preferences: an account may choose that threads it starts and threads it posts in are watched with `notify`.
- **Hidden and erased content is not delivered**, and an already-delivered notification whose activity is later hidden or erased is removed from every inbox by the same purge that clears caches ([0014](0014-caches-and-search.md) §5).
- **Bundling** is presentational: rows with the same (reason, target) within a window are shown as one item with a count, as Echo bundles. The rows stay separate.

### 3. The inbox is private state (extends 0007 §8, 0013 §4)

```sql
CREATE TABLE private.inbox (
  actor_key text NOT NULL, id bigint GENERATED ALWAYS AS IDENTITY,
  partition bigint NOT NULL, "offset" bigint NOT NULL,      -- the activity row
  reason text NOT NULL, target_kind smallint, target_id text,
  published timestamptz NOT NULL, read_at timestamptz,
  PRIMARY KEY (actor_key, id), UNIQUE (actor_key, partition, "offset", reason)
);
CREATE TABLE private.inbox_state (actor_key text PRIMARY KEY, seen_at timestamptz);
CREATE TABLE private.notification_pref (
  actor_key text NOT NULL, reason text NOT NULL, channel text NOT NULL, enabled boolean NOT NULL,
  PRIMARY KEY (actor_key, reason, channel)
);
```

> **Amended by [0027](0027-preferences-and-portability.md) §1 and §8.** `private.notification_pref` is dropped; the reason-by-channel matrix is the set of `notifications.{reason}.{channel}` keys in `private.preference`, and the addressing projection reads those. `private.inbox` and `private.inbox_state` stand. The inbox is portable state (0027 §2): it travels in the user data bundle and the private extract of a cooperative tenant move.

The rules of [0013](0013-postgres-storage.md) §4 apply: readable by the accounts role only, never exported, never in a tenant bundle ([0018](0018-tenants.md) §10) except as [0027](0027-preferences-and-portability.md) §4 carries it. Rows are deleted after `notifications.retention` (site configuration, default 90 days; read rows sooner). An inbox is rebuildable from the activity rows and the addressing rules for the retention window, except for read state, which is lost on a rebuild and costs only bold text, as `seen` does in 0020 §3.

### 4. Channels

Each reason can be delivered on each channel, subject to the account's preference matrix (§3) and the instance's defaults.

| Channel | Delivery |
|---|---|
| **Web** | The bell in the global header ([0010](0010-site-ui.md) §2), with the unseen count; `Special:Notifications`, which is the inbox as a page with mark-read and mark-all-read |
| **Email** | Immediate, or a daily or weekly digest, to an address held in `private` and verified by a token link. No address is ever shown or exported |
| **Fediverse** | A direct message to the account's registered fediverse account (§5) |

**Web Push** is not in this ADR (open questions). Every channel renders the same item: who, did what, where, when, and a permalink (`Special:PermanentLink/{revid}` for a post, the job or log page otherwise), in the recipient's interface language.

### 5. The fediverse channel

This is the smallest step ActivityPub allows: the instance **sends** activities and accepts only what it needs to be allowed to send. Nothing here makes any local user, page or thread visible to the fediverse.

**The notifier actor.** Each tenant has one ActivityPub actor, of type `as:Service`, at `{base}/notifier`, discoverable by WebFinger as `acct:notifier@{host}`. Its document carries `inbox`, `outbox` (empty), `preferredUsername`, `name`, `summary` explaining what it is, and `publicKey`. Its keypair is generated at tenant creation and held in `private.ap_key`; it is unrelated to the instance key of [0006](0006-log-integrity-and-erasure.md) §6, whose job is checkpoints, and it does not travel when a tenant moves, since nothing private does ([0018](0018-tenants.md) §10): a moved tenant's notifier is a new actor, and its users re-register.

**What the notifier accepts.** Its inbox handles `Follow` (answered with `Accept`, and the follower recorded in `private.ap_follower`), `Undo` of a `Follow`, and nothing else: every other activity is acknowledged with 202 and dropped. Following the notifier is what lets a Mastodon account receive its messages when the account filters messages from strangers, and the registration flow says so.

**Registering an account.** In `Special:Account`, under a new **Notifications** section labelled Private ([0010](0010-site-ui.md) §11), a user enters a fediverse handle. The instance resolves it by WebFinger, fetches the actor document (with a signed request, since many servers require one), stores the actor and inbox IRIs in `private.fediverse_handle`, and sends one direct message containing a confirmation link with a token. Clicking it marks the handle verified. Until then nothing else is sent.

**What is sent.** One `Create` of an `as:Note` per notification or bundle, addressed only to the recipient: `to` is their actor IRI, there is no `as:Public` and no followers collection, and the Note carries an `as:Mention` tag for them, which is what Mastodon requires to show a message as a direct message. `content` is the rendered item of §4 as HTML with a permalink; `attributedTo` is the notifier; `published` is the notification's time. There is no `inReplyTo`, and the Note is not a post on this wiki: it is the notifier speaking. Delivery is an HTTP-signed `POST` to the recipient's inbox with a `Digest` header, using the HTTP Signatures profile Mastodon accepts (draft-cavage; RFC 9421 when Mastodon does), through a queue in `ops` with exponential backoff, giving up after seven days; a handle that fails repeatedly is marked broken and the account page says so. Bundled notifications are sent as one message, and no account is sent more than one message per `notifications.fediverse_interval` (default five minutes).

**Privacy.** The handle is private in this instance's sense: never shown, projected or exported, and unrelated to the public account links of [0007](0007-actor-identity.md) §7. But a message that says "Example replied to you on Librarybase" is delivered to another server, which thereby learns that this fediverse account belongs to a Librarybase user, and stores the message under its own rules. The registration form says this in plain words. The channel is off until a user registers a handle, and an instance may disable it, or restrict it to a domain allow-list, in site configuration.

**Discovery endpoints**, all public and read-only: `/.well-known/webfinger` (answering only for the notifier), `/.well-known/nodeinfo` with a minimal NodeInfo 2.1 document naming the software, and the notifier's actor document, served as `application/activity+json`. WebFinger answers for local users, and actor documents for them, are [0022](0022-federation.md)'s to decide; until then a lookup of `acct:example@{host}` returns 404.

### 6. UI (extends 0010 §2 and §11)

- **The bell** in the global header shows the unseen count and opens the inbox as a panel; opening it sets `seen_at`. Items link to their permalink, and each has mark-read.
- **`Special:Notifications`** is the inbox as a page, filterable by reason and read state, with mark-all-read.
- **`Special:Account`** gains a **Notifications** section (Private): the preference matrix of reasons by channel; the email address with its verification state; the fediverse handle with its verification state, a *follow the notifier* hint with the notifier's handle, and *Remove*.
- **Watch controls** gain a *notify me* toggle beside the star, which sets `notify` on the watch row; subscribing to a thread is watching it with `notify`.
- **The orange bar** is kept in spirit: an unread `talk` notification shows a persistent banner until read, as MediaWiki's "You have new messages" did, because a message on one's talk page is the one notification a wiki must not let pass.

### 7. API (extends 0012 §4 and §5)

**Action API**, additively under [0012](0012-api-requirements.md) §1, in **Echo's shape**, because the Wikipedia apps, Convenient Discussions and other scripts speak it: `meta=notifications` with `notprop=list|count|seenTime`, `notfilter=read|!read`, `notsections`, `notlimit` and continuation, returning Echo's model format; `action=echomarkread` with `list` or `all`; `action=echomarkseen`. `meta=userinfo&uiprop=hasmsg` reports an unread `talk` notification. Echo's `notwikis` cross-wiki parameter is accepted and ignored.

**REST**, under `rest.php/triplespace/v0`:

| Route | Meaning |
|---|---|
| `GET /inbox` | The inbox as an `as:OrderedCollection` of activities, each with `scatter:reason` and read state, paginated with 0020's continuation |
| `POST /inbox/read`, `POST /inbox/seen` | Mark items read; set the seen time |
| `GET /notifications/preferences`, `PUT …` | The matrix of §3 |
| `PUT /notifications/email`, `DELETE …` | Register (sends the verification link) and remove |
| `PUT /notifications/fediverse`, `DELETE …` | Register (resolves the handle, sends the confirmation message) and remove |
| `GET /notifications/verify/{token}` | The link target for both verifications |

The ActivityPub endpoints of §5 are outside `triplespace/v0`, at the paths ActivityPub and WebFinger fix.

### 8. Storage and operations (extends 0013 §4)

Beside §3: `private.fediverse_handle` (actor key, handle, actor IRI, inbox IRI, verified, broken, added), `private.email` (actor key, address, verified, added), `private.ap_key` (tenant, key ID, private key, public key, created) and `private.ap_follower` (the notifier's followers: actor IRI, accepted). Since [0022](0022-federation.md) §10 the last two are keyed by actor, the notifier being one actor among the opted-in ones, and are read by `triplespace-federation`. The delivery queue for email and fediverse messages lives in `ops`, with attempts, next attempt and last error per item. Outbound requests go through the same egress path and rate limits as live upstream fetches ([0012](0012-api-requirements.md) §6), and the `notifications.fediverse_domains` allow and deny lists are site configuration.

### 9. Permissions (extends 0016 §2)

MediaWiki's `editmyoptions` governs the preference matrix, addresses and handles; `viewmyprivateinfo` governs reading them. Both default to `user`. The inbox needs no permission beyond being its holder. The discovery endpoints of §5 need `read`.

### 10. Crates

| Layer | Crate | Contents |
|---|---|---|
| Substrate | `scatter-activitypub` (new) | AS2 object building for the profile of [0019](0019-discussions.md) §10 and this ADR, actor documents, WebFinger and NodeInfo documents, HTTP Signatures (signing and verification), and inbox activity parsing. Pure; no I/O. Written so that [0022](0022-federation.md) adds actors, not a new stack |
| Triplespace | `triplespace-notify` (new) | The addressing projection of §2, inbox storage, bundling, the channels of §4 and the delivery queue |
| | `triplespace-accounts` | `private.email`, `private.fediverse_handle`, `private.ap_key`, verification flows |
| | `triplespace-api-rest`, `triplespace-api-action` | §7, including the Echo-shaped modules and the ActivityPub and well-known endpoints |

## Consequences

- **Notifications are the inbox half of ActivityPub**, so [0022](0022-federation.md) adds actors and inbound handling to an existing mechanism rather than a second notification system beside this one.
- **Mastodon works as a notification client today**, with the instance speaking only as much of the protocol as sending requires: WebFinger, signed fetch and delivery, and `Follow`/`Accept` on one actor.
- **A private handle is still a disclosure to another server.** The instance keeps its promise not to show the association; it cannot promise what the recipient's server does with a message. The form says so.
- **Echo's API shape is a compatibility surface** like the Action API's, with the same additive rule and the same obligation to keep it in step.
- **Read state is the second thing that is not rebuildable**, after `seen` on watches, and for the same reason.
- **Two private tables hold contact details** (email, fediverse handle) that did not exist before; the privacy test of [0012](0012-api-requirements.md) §8 now covers them.

## Open questions

- **Web Push**, and native app push.
- **Bundling rules and windows**, and whether a digest email should summarize watch notifications the way the watchlist page does.
- **Temporary accounts**: MediaWiki delivers talk-page messages to temporary accounts by session; whether to do the same.
- ~~**Cross-tenant notifications**: whether a mention of `librarybase:42` from another tenant on the same instance reaches that user's inbox, which would be the first thing to cross a tenant boundary.~~ *Settled by [0028](0028-tenancy-policy.md) §7: by policy, through farm identity; the bell aggregates linked accounts' inboxes and a farm-name mention goes to the home tenant.*
- ~~**RFC 9421 timing**, and FEP-8b32 object integrity proofs, as Mastodon adopts them.~~ *Settled 2026-09-27 with [0022](0022-federation.md) §8: both implemented behind `site` switches, off until Mastodon accepts them; outbound stays cavage until then.*
- ~~**[0022](0022-federation.md)**: WebFinger and actor documents for users, talk pages as `Group` actors that `Announce` their threads, `Update` and `Delete` delivery, opt-in per account with the vanish page amended, `rel="me"` public links, and, separately, inbound replies as posts attributed to fediverse actors with a `federated` group, domain lists and moderation.~~ *Settled by [0022](0022-federation.md) Part B, which does all of this and fixes the inbound rules while leaving the inbound protocol details open.*

## References

- [Extension:Echo](https://www.mediawiki.org/wiki/Extension:Echo), [Echo API](https://www.mediawiki.org/wiki/Extension:Echo/API) and [Notifications/Types](https://www.mediawiki.org/wiki/Notifications/Types)
- [Extension:DiscussionTools — Topic subscriptions](https://www.mediawiki.org/wiki/Help:DiscussionTools#Topic_subscriptions)
- [ActivityPub](https://www.w3.org/TR/activitypub/) and [Activity Streams 2.0](https://www.w3.org/TR/activitystreams-core/)
- [WebFinger (RFC 7033)](https://www.rfc-editor.org/rfc/rfc7033) and [NodeInfo](https://nodeinfo.diaspora.software/)
- [Mastodon: ActivityPub in Mastodon](https://docs.joinmastodon.org/spec/activitypub/) and [Security (HTTP Signatures)](https://docs.joinmastodon.org/spec/security/)
- [HTTP Message Signatures (RFC 9421)](https://www.rfc-editor.org/rfc/rfc9421)
