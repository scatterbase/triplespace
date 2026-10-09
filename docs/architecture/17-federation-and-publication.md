# 17. Federation and publication

This chapter covers what leaves an instance for another party and what comes back: verified data sync between Triplespace instances; ActivityPub, by which accounts, talk pages and boards are followed from the fediverse and replies arrive; exporting and pushing proposals to the wiki an entity came from; publications, which write a scope's data to an external wiki as template calls; and dataset publication, the scope dumps and the schema.org profile. It assumes the log, its checkpoints, proofs and key chain from [01](01-log-and-records.md); source graphs, mirror partitions, adapters and the provider registry from [05](05-providers-and-ingest.md); tenants as providers and the Triplespace adapter's place in the ingest layer from [08](08-tenants-and-instances.md); actors, subsidiaries, the OAuth client role and private state from [07](07-actors-and-accounts.md); the notifier actor and its fediverse channel from [16](16-logs-feeds-and-notifications.md); proposals as threads, and scopes and kits, from [14](14-discussions.md) and [15](15-structured-pages.md). The tables these features add are in [03](03-storage-caches-and-search.md), their routes in [18](18-api.md), their settings and config kinds in [23](23-configuration-and-registry.md), and their permissions in [09](09-security-and-moderation.md).

## 1. Verified data sync between instances

*Sources: [0022](../decisions/0022-federation.md) §1, §2, §3, §4, §5, §9; [0044](../decisions/0044-tenant-relative-ids.md) §4.*

### 1.1 What an instance publishes

*Sources: [0022](../decisions/0022-federation.md) §1; [0081](../decisions/0081-recovery-keys-and-continuations.md) §9.*

Every instance, for each tenant that has a provider code ([0015](../decisions/0015-record-format-and-partition-registry.md) §5), publishes at the tenant's base:

| What | Where | From |
|---|---|---|
| The **local-graph source dump**: the `local` partition's current state as N-Quads and as canonical JSON, one file per snapshot, with the record coordinates of each entity's newest record | `{base}/dumps/local/` | [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3 (source graphs may be exported), [0013](../decisions/0013-postgres-storage.md) §8 |
| The **Wikibase-compatible** and **full** dumps ([02](02-graphs-rdf-and-query.md)) | `{base}/dumps/` | [0013](../decisions/0013-postgres-storage.md) §8 |
| The **activity stream**, with `Last-Event-ID` ([16](16-logs-feeds-and-notifications.md)) | `GET /activity/stream` | [0020](../decisions/0020-change-feeds.md) §4 |
| **Records**: header and body of any record in a public partition, redacted as any response is | `GET /record/{partition}/{offset}` | [0012](../decisions/0012-api-requirements.md) §5 |
| **Checkpoints and segment manifests**, current and historical, as C2SP signed notes ([01](01-log-and-records.md) §4.2) | `{base}/.well-known/tlog/{partition name}/checkpoint` and `/checkpoint/{tree size}`; manifests at `/segment/{n}` | [0006](../decisions/0006-log-integrity-and-erasure.md) §6 |
| **Inclusion and consistency proofs** | `GET /record/{partition}/{offset}/proof?checkpoint=`; `GET /.well-known/tlog/{name}/consistency?from=&to=`; and the multi-proof `GET /record/proofs?partition=&checkpoint=&from=&to=` for a range of records against one checkpoint, with their shared upper path sent once | [0006](../decisions/0006-log-integrity-and-erasure.md) §9, [0012](../decisions/0012-api-requirements.md) §5 |
| The **key chain**: every `key:` record of the tenant's `config` partition ([08](08-tenants-and-instances.md) §2.4), with its `recovery-key`, `continuation` and `continuation-cancel` records, so a reader has the chain of custody from one URL ([0081](../decisions/0081-recovery-keys-and-continuations.md) §9) | `GET /.well-known/tlog/keys` | [0015](../decisions/0015-record-format-and-partition-registry.md) §3, [0018](../decisions/0018-tenants.md) §2 |

Only partitions whose export policy is `public` are published ([0005](../decisions/0005-crate-organization.md) §4.1); `internal` and `private` partitions have no records, checkpoints or proofs here. The local-graph source dump exists so that a reader can take a tenant's own assertions without the tenant's mirrors, for the reason §1.2 gives.

### 1.2 The Triplespace adapter reads the local graph and verifies it

*Sources: [0022](../decisions/0022-federation.md) §2; [0081](../decisions/0081-recovery-keys-and-continuations.md) §7.*

`scatter-adapter-triplespace` ([0028](../decisions/0028-tenancy-policy.md) §5; [08](08-tenants-and-instances.md) §4.4) is the adapter for a provider that is itself a Triplespace instance. Three rules refine what 0028 said of it.

**It reads the provider's `local` graph, and nothing else.** A provider's Wikibase-compatible dump is its *resolved view*, which includes its Wikidata mirror, its OpenAlex mirror and every other provider it reads. A reader has those from their sources already, under their own codes. So the adapter bootstraps from the local-graph source dump (§1.1) and follows the stream filtered to `source = local`, and the mirror partition `mirror/{provider}` holds only what the provider itself asserts. References inside those assertions are rewritten as [0018](../decisions/0018-tenants.md) §5 rewrites them: the provider's own `Q6` becomes `LBQ6`; a reference to a global entity, `WDQ42` or `domain:x`, passes through; and a reference to **the reader's own entities**, which the provider holds under the reader's code (`EXQ9`), is rewritten **back to the bare local ID** `Q9`. That last rule is what makes §1.3 work.

**It verifies by default.** For each record it mirrors, the adapter fetches the record with its header, checks the header's leaf against the provider's latest checkpoint with an inclusion proof, checks the checkpoint's signature against the provider's key chain, and checks consistency between the checkpoint it last saw and the current one ([0006](../decisions/0006-log-integrity-and-erasure.md) §5–6, §9; [01](01-log-and-records.md) §4). A batch of N records is verified with one checkpoint, one consistency proof and one multi-proof (§1.1), at O(N log N) hashes; every record is verified, always, and "verified" never means "probably". The **provider's header** (partition, offset, revision ID, commitment) and the **checkpoint tree size** it was verified against are stored in the mirror record's content part beside the entity state, so that a third party holding the reader's log can re-verify against the provider without trusting the reader; `view.entity_source` holds them in `provider_revid` and `verified_at_size` ([03](03-storage-caches-and-search.md) §4.2). A verification failure fails the sync job ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3), notifies the job's operator ([0021](../decisions/0021-notifications.md) §2, reason `job`), and leaves the mirror where it was; nothing unverified is appended.

**It follows a key chain only with authority** ([0081](../decisions/0081-recovery-keys-and-continuations.md) §7). Meeting a continuation in the provider's key chain ([08](08-tenants-and-instances.md) §6.8), it follows a `recovered` one once its delay has passed with no cancel, reporting it as pending on `Special:Providers` until then. It stops on an `unauthorized` one and notifies the job's operator; an administrator of the reading tenant may accept it with `ts-config`, which writes a `provider/accept-continuation` event to the reader's `log` and resumes the sync. On a fork, where the old host has published a checkpoint the continuation does not include, it stops and reports both, and an administrator chooses by the same event. Nothing is followed silently.

**Trusted is the fallback, not the default.** A provider registered with `trust = stream` ([0015](../decisions/0015-record-format-and-partition-registry.md) §5, `providers.toml`) is mirrored from its stream and dumps without proofs, as any non-Triplespace source is. The provider registry entry records which; the identity line and the source chip ([0010](../decisions/0010-site-ui.md) §2) show a verification mark for a `verified` provider and none for a `stream` one.

**Erasure and hiding follow.** A provider's `erase` record travels on its stream ([0006](../decisions/0006-log-integrity-and-erasure.md) §7); the adapter erases the mirrored parts with reason class `upstream`, as [0011](../decisions/0011-logs.md) §5 and [0015](../decisions/0015-record-format-and-partition-registry.md) §4 do for Wikidata's hiding ([01](01-log-and-records.md) §5.4). A provider's `read` ACL ([0023](../decisions/0023-moderation.md)) simply keeps the hidden statement out of its public stream and dump, so the reader never sees it; if the reader already held it, the `delete/statement` event on the stream tells the reader to drop it, which it does as an `upstream` erasure. A reader is therefore as faithful to a Triplespace provider as this instance is to Wikidata, by the same mechanism.

### 1.3 Mutual sync: two instances annotate each other's things

*Sources: [0022](../decisions/0022-federation.md) §3.*

Nothing stops two instances reading each other, and the result is the property [0000](../decisions/0000-init.md) §3 promised, between wikis. Librarybase (`LB`) reads example.wiki (`EX`) and example.wiki reads Librarybase:

- example.wiki asserts, in its `local` graph, a statement about `LBQ6`. Librarybase's adapter mirrors it into `mirror/example`, rewriting `LBQ6` back to `Q6` (§1.2). On Librarybase, `Item:Q6` now shows that statement with an `EX` source chip ([0003](../decisions/0003-statement-ui.md) §6), reconciled under [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3: Librarybase's local graph wins on terms, sitelinks and rank; statements union.
- Librarybase can override or suppress the foreign assertion locally ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7; [05](05-providers-and-ingest.md) §2.5); the correction is tracked against upstream as any correction is, and "Report upstream" ([0003](../decisions/0003-statement-ui.md) §4) links to example.wiki's own page for `LBQ6`.
- A `same-as` asserted on either instance ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §9) is a tier-1 link at home and a tier-2 link for the other, ranked by the reader's provider order, with conflicts held as 0004 §10 has them ([04](04-entities-and-identifiers.md) §4). Identity clusters therefore span instances without any instance controlling another's.
- Each instance's `local` graph stays the only thing it publishes as its own (§1.4). A third instance reads both directly; it never learns what Librarybase thinks example.wiki said, only what each said.

This is not replication and not consensus. Each instance keeps its own log, its own canonical IDs and its own view; what it gains is verifiable knowledge of what its partners assert, and a place on its pages to show it.

### 1.4 Provenance chains and re-export

*Sources: [0022](../decisions/0022-federation.md) §4.*

A reader **never re-exports what it mirrored as its own**. Its local-graph source dump (§1.1) is its `local` partition alone; its full dump ([0013](../decisions/0013-postgres-storage.md) §8) carries mirrored graphs marked by source for comparison, and a downstream instance that wants Librarybase's data reads Librarybase, whose code is the same everywhere because codes are allocated once in the registry ([0015](../decisions/0015-record-format-and-partition-registry.md) §5). The metadata graph carries, for every mirrored statement, `pav:importedFrom` the provider's document node and `pav:retrievedFrom` the job ([0015](../decisions/0015-record-format-and-partition-registry.md) §6; [02](02-graphs-rdf-and-query.md) §1.5), and for a verified provider the checkpoint it was verified against as `scatter:verifiedAgainst`. A statement's provenance is therefore one hop long by construction.

### 1.5 Beyond Triplespace

*Sources: [0022](../decisions/0022-federation.md) §5.*

Any provider that publishes a C2SP tlog checkpoint, RFC 6962 proofs and records in the canonical encoding of [0006](../decisions/0006-log-integrity-and-erasure.md) §2 ([01](01-log-and-records.md) §3) can be mirrored as `verified`. Scatterbase is the intended second case: its claim partitions are `logged` ([0006](../decisions/0006-log-integrity-and-erasure.md) §4), its checkpoints are the same format, and its claim payload declares its own parts ([0015](../decisions/0015-record-format-and-partition-registry.md) §1), so a Scatterbase adapter is the Triplespace adapter with a payload mapping. A provider that publishes only a stream or dumps is `stream`.

### 1.6 Tenants, instances and tenant-relative IDs

*Sources: [0022](../decisions/0022-federation.md) §9; [0044](../decisions/0044-tenant-relative-ids.md) §4.*

Data federation is between *tenants*, wherever they are hosted: a tenant on another instance is a provider like any other, and the same instance's tenants read each other directly ([0018](../decisions/0018-tenants.md) §5; [08](08-tenants-and-instances.md) §4). The rest of [0022](../decisions/0022-federation.md) §9, ActivityPub per tenant, is §2.4.

**A tenant-relative ID never leaves the tenant,** because it never reaches the log ([04](04-entities-and-identifiers.md) §2.3). Rewriting between tenants and instances ([0018](../decisions/0018-tenants.md) §5, §1.2 above) sees only local and foreign IDs. A bulk job submitted to tenant A with `QQQ5` writes A's `Q5`.

## 2. ActivityPub

*Sources: [0022](../decisions/0022-federation.md) §6, §7, §8, §9; [0049](../decisions/0049-boards.md) §10.*

### 2.1 Actors

*Sources: [0022](../decisions/0022-federation.md) §6; [0049](../decisions/0049-boards.md) §10.*

[0021](../decisions/0021-notifications.md) §5 gave each tenant one `as:Service` notifier ([16](16-logs-feeds-and-notifications.md) §5.5). Beside it are three actor kinds, each **opt-in** and each with its own keypair in `private.ap_key`, keyed by actor:

| Actor | Type | Who turns it on | Address |
|---|---|---|---|
| A local account | `as:Person` | The holder, in `Special:Account` | `acct:{name}@{host}`; document at `{base}/user/{id}` with `application/activity+json` |
| A talk page | `as:Group` | Its tenant, per namespace, in `site` configuration (`federation.talk_pages`) | `acct:{title}@{host}` for the subject's title, document at `{base}/page/{talk page id}` |
| A thread | `as:Collection`-bearing `as:Note` chain | Follows its talk page | Posts at `{base}/record/{partition}/{offset}`, as [0019](../decisions/0019-discussions.md) §7 already names them |

A person who opts in makes their **user page and posts** followable; nothing else about them changes, and WebFinger answers for their name only once they have. `rel="me"` links ([0007](../decisions/0007-actor-identity.md) §7; [07](07-actors-and-accounts.md) §3.1) are emitted from the account's public account links in both directions, so a Mastodon profile can verify a wiki account and the reverse. Subsidiaries ([0024](../decisions/0024-subsidiary-accounts.md); [07](07-actors-and-accounts.md) §4) may be actors too, which is how a bot's edits can be followed.

**A board can be a `Group` actor.** `Board` (310) is a namespace like any other in `federation.talk_pages`. The handle of a board's `Group` is not yet fixed.

The **vanish page** ([0010](../decisions/0010-site-ui.md) §11; [07](07-actors-and-accounts.md) §3.2) gains a sentence: copies of posts delivered to other servers may persist after vanishing; the instance sends `Delete` for each, and cannot enforce it.

### 2.2 Outbound: what a follower receives

*Sources: [0022](../decisions/0022-federation.md) §7; [0049](../decisions/0049-boards.md) §10.*

A talk-page `Group` **`Announce`s** every thread created on it and every post in those threads, which is the Lemmy pattern Mastodon and its kin understand. A person actor's outbox carries `Create` for their posts. Edits are `Update` with the new content; hiding or erasing a post ([0023](../decisions/0023-moderation.md) §5, [0006](../decisions/0006-log-integrity-and-erasure.md) §7) is `Delete` with an `as:Tombstone`, and a deleted thread `Delete`s its collection. Every object uses the fixed profile of [0019](../decisions/0019-discussions.md) §10 ([16](16-logs-feeds-and-notifications.md) §6.2) with `source` carrying the markdown. Delivery is the signed `POST` of [0021](../decisions/0021-notifications.md) §5, through the same delivery queue, `ops.delivery` ([03](03-storage-caches-and-search.md) §4.16), fanned out to followers' shared inboxes. The fan-out has its own rate class, `federation` ([07](07-actors-and-accounts.md) §6.2), counted per instance against the delivery queue, never against the posting actor. Followers are `private.ap_follower` rows per actor; a `Follow` is accepted automatically for a `Group` and for a person who opted in, and `Undo` removes it.

**Boards and listings.** Boards (310) may be federated as talk pages are. When a thread is listed on a page whose `Group` is federated, that `Group` `Announce`s the thread, and then every post in it, as the home's `Group` does; removing the listing (a `detach`) sends `Undo` of that `Announce`. A follower of both `Group`s receives the same objects twice, and fediverse software collapses them by `id`. This is how Lemmy cross-posts reach their communities. Inbound replies are governed by the thread's home (§2.3).

**Pins and foreign threads.** A talk page's or board's `Group` publishes its pinned threads as its `featured` collection, as Mastodon and Lemmy expose pinned posts. Foreign threads on a followed talk page ([0069](../decisions/0069-synchronized-talk-pages.md) §3; [13](13-mirrored-pages.md)) are never announced: they are the repository's speech, not the tenant's ([0069](../decisions/0069-synchronized-talk-pages.md) §4, §8).

Nothing about entity data is federated this way. Statements move between instances by §1; ActivityPub carries speech.

### 2.3 Inbound: the rules fixed, the protocol as implemented

*Sources: [0022](../decisions/0022-federation.md) §8; [0049](../decisions/0049-boards.md) §10.*

Replies from fediverse actors may become posts on this instance, under these rules:

- **A fediverse actor is a `federated` actor** ([0007](../decisions/0007-actor-identity.md) §5; [07](07-actors-and-accounts.md) §1.4): a surrogate `{base}/actor/{n}` of a new kind, whose actor record holds the remote actor IRI and its last-seen name. The IRI is content and is never a key; the surrogate is. Renames upstream append a new actor record, as for any foreign actor.
- **The `federated` group holds no permissions by default** ([0016](../decisions/0016-permissions-and-access-control.md) §3). A tenant enables inbound replies per talk namespace with an ACL on that namespace ([0016](../decisions/0016-permissions-and-access-control.md) §4, [0023](../decisions/0023-moderation.md) §1; [09](09-security-and-moderation.md) §8.3) restricting `edit` to a group that includes `federated`, and nowhere else. A federated actor can never create a thread, edit a page or touch an entity. A remote reply to a post is a post in the thread, whichever `Group` it reached the follower through, so the per-namespace ACL is evaluated on the thread's home, by enclosure ([0049](../decisions/0049-boards.md) §7; [14](14-discussions.md)).
- **Domain allow and deny lists**, in the shape of [0026](../decisions/0026-sitelinks.md) §3 ([06](06-statements-and-properties.md) §4.4), as `config` records of kind `federation-policy` ([23](23-configuration-and-registry.md) §2.2): an instance deny list that wins, and a tenant mode with a list. The instance list is the spam list a farm operator keeps ([0028](../decisions/0028-tenancy-policy.md) §8).
- **The signed activity is evidence.** An inbound post is a `post` record ([0019](../decisions/0019-discussions.md) §1) attested by the surrogate, and its attestation part ([0015](../decisions/0015-record-format-and-partition-registry.md) §1; [01](01-log-and-records.md) §2.4) gains an `evidence` field holding the remote activity and its HTTP signature, so the instance can show that it did not make the words up. This is the attestation slot [0006](../decisions/0006-log-integrity-and-erasure.md) §3 reserved for client signatures, used for exactly that.
- **A remote `Delete` is followed** as [0011](../decisions/0011-logs.md) §5 follows upstream hiding ([16](16-logs-feeds-and-notifications.md) §1.5): the post's text part is erased with reason class `upstream`, and the tree keeps its shape ([0019](../decisions/0019-discussions.md) §4).
- **Moderation is [0023](../decisions/0023-moderation.md) unchanged**: hide, suppress, block the surrogate, protect the talk page; edit filters ([0030](../decisions/0030-edit-filters.md); [09](09-security-and-moderation.md) §7) see an inbound post in the `text` context with `user_kind = federated`.
- **Signatures.** The inbox accepts draft-cavage HTTP Signatures with a mandatory `Digest`, which is what every Mastodon-compatible server sends; RFC 9421 signatures and FEP-8b32 object-integrity proofs are implemented in `scatter-activitypub` behind `site` switches (`federation.accept_rfc9421`, `federation.accept_fep8b32`, both off) and turned on when the fediverse turns, with no change to the inbox path; outbound stays cavage until Mastodon accepts 9421 ([0021](../decisions/0021-notifications.md) §5).
- **Replay.** Every accepted activity's `id` is recorded in `ops.ap_inbox_seen` for `federation.replay_window` (default seven days) and a repeat is acknowledged with 202 and dropped; a request whose `Date` lies outside ±`federation.clock_skew` (default twelve hours) is rejected; inbound activities are verified on receipt and then handled through the `ops` queue, so a burst never blocks the request path.
- **Promotion.** A `federated` surrogate is never merged into a local account. A local account that proves control of the remote actor, by the instance finding a `rel="me"` link from the remote profile to its user page, may add it as a public **account link** under [0007](../decisions/0007-actor-identity.md) §7, the same act as linking a Wikidata account; the surrogate stays the actor of every post it made, and contributions and history show "also {remote} here" as for any linked account. Attribution never moves, which is the invariant every inclusion proof relies on.

### 2.4 Tenancy

*Sources: [0022](../decisions/0022-federation.md) §9.*

ActivityPub is per tenant, as its notifier is: each tenant has its own host, actors and keys. A farm sets defaults and locks for `federation.*` settings through templates ([0028](../decisions/0028-tenancy-policy.md) §8; [08](08-tenants-and-instances.md) §5.3) and keeps the instance deny list. Data federation (§1) is between tenants, as §1.6 says.

## 3. Exporting and pushing proposals

*Sources: [0067](../decisions/0067-proposals.md) §4, §6.*

A proposal is a thread with a payload, compiled and closed as [14](14-discussions.md) describes; this section is how its payload reaches the destination wiki.

### 3.1 Export: version 1

*Sources: [0067](../decisions/0067-proposals.md) §4.*

**Export is the first phase and always available.** From the proposal thread: **QuickStatements** (copy or download, with a link that opens QuickStatements with the text pre-filled where the tool supports it), **Wikibase JSON** (download), and for a page the **wikitext** and a **diff** ([0068](../decisions/0068-merging-with-upstream.md) §4; [13](13-mirrored-pages.md)). Exporting appends a `submit` with `method: export` and no `upstream`, so that the proposal is marked offered and the loop of [0067](../decisions/0067-proposals.md) §5 starts watching upstream; the person is told to come back and paste the revision ID, or to let the detector find it. `Special:Corrections`' "export per upstream graph" ([0047](../decisions/0047-special-pages.md) §6; [21](21-special-pages.md)) becomes **Propose**: it opens a proposal thread for the selection, one per subject.

Nothing here needs a credential, a consumer registration or a policy conversation with the destination wiki. A tenant that never enables §3.2 has a working proposals feature.

### 3.2 Push as the person: the later phase

*Sources: [0067](../decisions/0067-proposals.md) §6.*

**Specified now, built later.** `proposals.push` (site, default `off`) is refused until the instance implements this section.

**The instance becomes an OAuth client of the destination wiki with edit scopes** ([07](07-actors-and-accounts.md) §7.7). Wikimedia's OAuth 2 (`editpage`, `createeditmovepage`, `highvolume` where granted) through the identity issuer the tenant already uses for login ([0007](../decisions/0007-actor-identity.md) §3), as a **second authorization** the person grants from the proposal thread ("Allow this wiki to edit Wikidata as you"), separate from login so that logging in never grants editing. The grant is stored in **`private.upstream_grant (actor_key, wiki, scopes text[], access_hash, refresh_hash, issued, expires)`**, portability class **re-established, not carried** ([0027](../decisions/0027-preferences-and-portability.md) §2; [07](07-actors-and-accounts.md) §8.3): it is reissued on a new instance and revocable from `Special:Preferences` and from the destination wiki's own settings. **One consumer per instance**, registered with the destination wiki by the operator with the instance's callback; on a farm the callback is the farm base and the tenant is carried in `state`, so a forty-tenant farm registers once.

**Pushing** appends the compiled payload as the person: `wbeditentity` with the JSON for an entity, or an edit to the destination page for a page ([0068](../decisions/0068-merging-with-upstream.md) §4), with the summary "Proposed at {proposal URL} via {instance}" and the destination's `maxlag` honoured; the resulting revision IDs go into the `submit` record. Rate class `upstream` ([0024](../decisions/0024-subsidiary-accounts.md) §5), counted per person. **The instance never holds a shared upstream account** and never pushes a proposal under its own name: attribution belongs to the person, the destination's bot policy applies to them, and a tenant's reputation upstream is its editors'. A subsidiary ([0024](../decisions/0024-subsidiary-accounts.md)) may hold a grant of its own when its operator authorizes it, which is how a tenant's reconciliation bot pushes under a flagged upstream bot account that is also the operator's responsibility. The rule is scoped to proposals; a publication's credential is a subsidiary's (§4.4).

**Talk pages use the grant first.** Replies and new sections sent to a followed talk page ([0069](../decisions/0069-synchronized-talk-pages.md) §5–6; [13](13-mirrored-pages.md)) are the first use of this client, which is therefore built with 0069; pushing a proposal still waits on `proposals.push`, except that a following fork sends a `talk`-destination proposal through [0069](../decisions/0069-synchronized-talk-pages.md) §6 now, gated by that section's switch; the later phase applies to page proposals.

The instance is thus an OAuth server for its own API ([0025](../decisions/0025-oauth-server.md); [07](07-actors-and-accounts.md) §7) *and* a client of other wikis' APIs for one purpose; the grant table sits beside 0025 §8's consumer and token tables, and the two never share a credential.

## 4. Publishing a scope to an external wiki

*Sources: [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §1, §2, §3, §4, §5, §6, §7.*

### 4.1 Publications

*Sources: [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §1.*

**A publication writes a scope's data to another wiki as pages of template calls.** It is tenant configuration, a `config` record of kind `publication` ([23](23-configuration-and-registry.md) §2.2) whose code is the publication's name:

| Field | Meaning |
|---|---|
| `destination` | The destination wiki's API endpoint and a display name |
| `account` | The credential the publication writes with (§4.4) |
| `scope` | The scope whose members are published ([0060](../decisions/0060-scopes.md) §1; [15](15-structured-pages.md)) |
| `rows` | The row template and its fields (§4.2) |
| `pages` | How rows are grouped into data pages (§4.3) |
| `kits` | Generated pages (§4.5) |
| `debounce`, `edits_per_minute`, `overwrite` | §4.3–4.4 |

### 4.2 Rows

*Sources: [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §2.*

**Each scope member is one call of the destination's data template.** `rows` names the template and its fields; each field is a name on the destination and a path on the member:

| Path | Yields |
|---|---|
| `id`, `label`, `description` | The member's ID, and its label and description in the field's `lang` (default the publication's language) |
| A property (`P9010`) | The values of the member's best-rank statements, as labels for items and as text for the rest |
| A property with `as = "id"` or `as = "url"` | Item values as IDs, or as their canonical IRIs |
| A path, `P9011/WDP131*` | Values reached through a chain of properties; `*` follows a property transitively, over mirrored and local data, to `entities.closure_depth` ([0070](../decisions/0070-shallow-entity-mirroring.md) §2.3; [05](05-providers-and-ingest.md) §5.3) |
| `mentioned_in.url` | The URL of the page subject the member is mentioned in ([0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §4; [13](13-mirrored-pages.md)), so the row links back to its guide |

A field with several values is joined by its `join` (default `;`), which matches a Cargo `List (;) of …` field. **Transitive paths are how hierarchies reach Cargo**, which cannot follow them: a resource about Cuyahoga County gets `Cuyahoga County;Ohio;United States` in its jurisdiction field, and a query for Ohio finds it with `HOLDS`.

Values are written as plain text, escaped for a template parameter: `|` as `{{!}}`, `=` in a value unchanged (fields are always named), `{{` and `}}` and `[[` as HTML entities, newlines as spaces. A row's field order is the configuration's, and rows are sorted by member ID, so the same data always makes the same text.

### 4.3 Data pages

*Sources: [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §3.*

**Rows are written to machine-owned data pages, grouped by `pages.group_by`**: a path whose first value names the group (for FamilySearch, the guide page each resource is mentioned in, or its top jurisdiction), and `pages.title`, a pattern such as `"{group}/Semantic data"`. A member with no value for the path goes to `pages.ungrouped`. A group larger than `pages.max_rows` (default 500) is split into numbered pages.

Each data page holds a comment saying it is generated and from where, then its rows. **A data page is rewritten only when its text would change.** A change to a member's resolved state, or to the scope's membership, marks the member's group dirty; a dirty group is regenerated after `debounce` (default 1 hour), so a re-extraction of many pages produces one edit per data page. A group that becomes empty gets a page holding only the comment, not a deletion.

Not yet: deleting data pages whose group is gone is open; blanking leaves stubs, and deletion needs a right the destination may not grant.

### 4.4 Writing to the destination

*Sources: [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §4.*

- **The account** is a bot account on the destination, with a bot password, or an OAuth grant (§3.2). The publication runs under a subsidiary account ([07](07-actors-and-accounts.md) §4) that holds the credential: a bot password is kept in `private.publication_credential` ([03](03-storage-caches-and-search.md) §4.15), encrypted at rest and never returned by any API, in the "re-established" portability class ([0027](../decisions/0027-preferences-and-portability.md) §2), and a grant is that subsidiary's row of `private.upstream_grant`.
- **Edits** go through the upstream client with `bot`, `maxlag=5`, the last known `basetimestamp`, and the summary "Updated from {scope} by {instance} (job {id})". They count against the publication's `edits_per_minute` (default 10) as well as the `upstream` rate class ([0024](../decisions/0024-subsidiary-accounts.md) §5).
- **A data page someone else has edited is not overwritten.** Before writing, the job compares the page's latest revision with the last it wrote. With `overwrite = never` (default), a page edited by anyone other than the account is skipped and reported as a conflict until someone resolves it; `overwrite = always` writes anyway, for a destination that agrees the pages are machine-owned.
- **The job is one job per publication**, run under that subsidiary ([0040](../decisions/0040-instance-prerogatives.md) §6; [07](07-actors-and-accounts.md) §4.6), with `job/start` and checkpoints, so its runs, edit counts and conflicts are on its job page. Its state table is in [03](03-storage-caches-and-search.md) §4.15 and its routes in [18](18-api.md) §3.2.

### 4.5 Generated pages from kits

*Sources: [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §5.*

**A publication can create pages from a kit, once each.** A kit here is a [0062](../decisions/0062-workspaces.md) §6 kit ([15](15-structured-pages.md)) whose pages are written to the destination, not the tenant, with one more placeholder kind: `for_each`, a path over the scope's members whose distinct values each make one page.

```json
{ "for_each": "P9010",
  "title": "{{label}} records, all localities",
  "content": "{{Resource list|record_type={{label}}}}" }
```

A page is **created when missing** and never edited afterwards, unless `kits.update = true` and every revision of the page is the account's. A value that appears later makes its page then. The content is meant to be static wikitext whose list is a `{{#cargo_query:…}}`, usually through a template on the destination, so the page stays current as data pages change and needs no further edits.

### 4.6 The destination's template and table

*Sources: [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §6.*

**The publication can write the data template for an administrator to install; it never installs it.** `triplespace-cli publication template {name}` prints a template with a `{{#cargo_declare:…}}` built from the row fields, with types inferred from their paths (`String`, `List (;) of String`, `URL`, `Date`, `Page` for `mentioned_in.url`'s title), and the matching `{{#cargo_store:…}}`. Creating the table on Cargo's table administration page and protecting the data pages are the destination's acts.

### 4.7 Publications are not proposals

*Sources: [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §7.*

A proposal ([0067](../decisions/0067-proposals.md) §1; [14](14-discussions.md)) offers a change to the wiki an entity came from, for its community to adopt or decline, and closes the loop by watching for adoption. A publication writes pages its destination has agreed to receive, under an account the destination granted, and owns them. The two share the upstream client and nothing else: a publication has no thread, no state machine and no adoption test.

## 5. Dataset publication

*Sources: [0076](../decisions/0076-dataset-publication.md) §1, §2, §3, §4, §5.*

### 5.1 Scope dumps

*Sources: [0076](../decisions/0076-dataset-publication.md) §1.*

**A scope can be dumped on a schedule, in several formats, as files anyone permitted can download.** `dumps` on a scope's definition ([15](15-structured-pages.md)) lists formats and a schedule (default `daily`):

| Format | Holds |
|---|---|
| `wikibase.json` | One line per member, in Wikibase JSON as `Special:EntityData` serves it, like Wikidata's JSON dump |
| `wikibase.nt` | The members' Wikibase RDF in the resolved view, as N-Triples ([0001](../decisions/0001-revision-metadata-rdf.md) §2; [02](02-graphs-rdf-and-query.md) §4.2) |
| `schema.jsonld` | One JSON-LD document per line, in the schema.org profile of §5.2 |

Each dump is gzip-compressed, stored as a blob ([0039](../decisions/0039-files-and-media.md) §3; [03](03-storage-caches-and-search.md) §8), and described by a manifest: the scope's revision, the member count, the generation time, the SHA-256, and the checkpoint of each partition it reflects ([0006](../decisions/0006-log-integrity-and-erasure.md) §6; [01](01-log-and-records.md) §4.2), so that a dump can be checked against the log. The previous `dumps.keep` (default 7) dumps of each format are kept. Dumps run as jobs ([03](03-storage-caches-and-search.md) §4.15) and are listed and served at `GET /scope/{pageid}/dumps` and `GET /scope/{pageid}/dumps/{file}` ([18](18-api.md) §3.2).

**A dump holds what an anonymous reader can read** ([0056](../decisions/0056-security-model.md) §2; [09](09-security-and-moderation.md) §5). A tenant that restricts reading serves its dumps only to principals with `read`, and dumps hold that principal's view only if generated for it.

Not yet: whether dumps of restricted tenants are generated per principal, or only per group, is open.

### 5.2 The schema.org profile

*Sources: [0076](../decisions/0076-dataset-publication.md) §2.*

**A tenant can describe its entities in schema.org, beside Wikibase RDF and without changing it.** A `config` record of kind `jsonld-context` ([23](23-configuration-and-registry.md) §2.2), whose code is the profile's name, maps the tenant's properties to schema.org terms:

| Field | Meaning |
|---|---|
| `types` | Item values of `P31` (or another property) to `@type`s: `research guide` to `WebPage`, `record collection` to `Collection` and `Dataset` |
| `properties` | Property to term, with an optional form: `P9011` to `spatialCoverage`; `WDP407` to `inLanguage`; `WDP123` to `publisher`; `P9030` to `url`; `WDP1343` to `subjectOf` |
| `intervals` | Pairs of properties that make one ISO 8601 interval: `WDP580`/`WDP582` to `temporalCoverage` as `1850/1885` |
| `external` | Identifier properties written as `identifier` with a `PropertyValue` naming the scheme |

Item values are written as `{"@id": canonical IRI, "name": label}`, with the label in the request's language, so a crawler can follow each one. The profile is served at `Special:EntityData/{id}.jsonld?profile={name}` and in `schema.jsonld` dumps. `Special:EntityData/{id}.jsonld` without a profile stays as Wikibase emits it, as [0001](../decisions/0001-revision-metadata-rdf.md) §2 requires ([02](02-graphs-rdf-and-query.md) §4.2).

### 5.3 Walking from any entity

*Sources: [0076](../decisions/0076-dataset-publication.md) §3.*

**An entity's description says how to find what points at it.** A crawler that reaches a period, a record type or a cultural group must be able to list the resources about it:

- `GET /entity/{id}/referrers?property={P}&cursor=` returns, paged, the entities whose best-rank statements under `P` have `id` as their value, from `view.entity_ref` ([03](03-storage-caches-and-search.md) §4.3). Without `property`, it returns the counts per property.
- In the schema.org profile, every entity carries `"subjectOf"`-style links only where the data says so; in addition it carries, for each property with referrers, a `potentialAction` of type `SearchAction` whose `target` is the referrers URL, with the property's label and the count. A crawler that knows schema.org follows it; one that does not still sees the URL.

Facet values thereby become entry points: a crawler entering at "Swedish Americans" lists the resources whose cultural-group statement names it, without ever visiting a place.

### 5.4 Discovery

*Sources: [0076](../decisions/0076-dataset-publication.md) §4.*

- `GET /scope/{pageid}/dataset.jsonld` describes the scope as a schema.org `Dataset`: name and description from the scope page, `license` from the tenant's `content.licence` ([0053](../decisions/0053-mirrored-pages.md) §9; [23](23-configuration-and-registry.md) §3.2), `dateModified`, and one `DataDownload` per dump with `contentUrl`, `encodingFormat` and `contentSize`.
- A sitemap at `/sitemap-datasets.xml` lists every dumped scope's dataset description and its members' profiled JSON-LD URLs, and `robots.txt` names it.
- The MCP server ([0075](../decisions/0075-mcp-server.md) §4; [18](18-api.md) §7.3) exposes the same description as a resource.

### 5.5 Looking up by URL

*Sources: [0076](../decisions/0076-dataset-publication.md) §5.*

**`GET /jsonld/by-url?url={u}&profile={name}` returns the profiled JSON-LD of the entities a URL identifies**: the subject whose `page_subject` match key is that URL ([0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §2; [13](13-mirrored-pages.md)), and the resources mentioned on that page with their facets. This is the one call a MediaWiki extension on the source wiki would need to embed JSON-LD in each page's head.

Not yet: no such extension is specified; whether it is Triplespace's to ship or the destination's to write is open.
