# 0022. Federation: verified data sync and ActivityPub

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-01 (A4)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md), [0028](0028-tenancy-policy.md)
- **Uses:** [0000](0000-init.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0014](0014-caches-and-search.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0026](0026-sitelinks.md), [0030](0030-edit-filters.md)

## Context

"Federation" entered these ADRs through [0021](0021-notifications.md), which took the smallest step onto the fediverse, one notifier actor per tenant, and left the rest to this ADR: followable users and talk pages, and replies from elsewhere. James's observation is that the word covers a second thing the design already makes possible: **synchronizing data between instances.** [0000](0000-init.md) §3 lets foreign entities sit beside local ones; [0002](0002-source-graphs-and-mass-ingest.md) mirrors a provider's graph; [0018](0018-tenants.md) §5 and [0028](0028-tenancy-policy.md) §5 made one Triplespace instance a provider to another, first by direct partition reads and then by a sync over the activity stream. And [0006](0006-log-integrity-and-erasure.md) gives every record on every instance a signed, provable place in its log, which a mirror of Wikidata can never have. What none of them says is how one instance *verifies* another, what it reads, and what happens when two instances read each other.

The two halves share a stance. Triplespace federates **facts and speech, not identity**: an instance mirrors another's assertions and can prove they were made; a person on one instance can be followed and answered from elsewhere; but no account, no permission and no attribution is ever merged across instances, exactly as [0018](0018-tenants.md) §4 keeps them apart across tenants.

## Part A — Data federation

### 1. What a Triplespace instance publishes (settles 0006 Q3)

*Changed by A2.*

Every instance, for each tenant that has a provider code ([0015](0015-record-format-and-partition-registry.md) §5), publishes at the tenant's base:

| What | Where | From |
|---|---|---|
| The **local-graph source dump**: the `local` partition's current state as N-Quads and as canonical JSON, one file per snapshot, with the record coordinates of each entity's newest record | `{base}/dumps/local/` | [0002](0002-source-graphs-and-mass-ingest.md) §3 (source graphs may be exported), [0013](0013-postgres-storage.md) §8 |
| The **Wikibase-compatible** and **full** dumps, as before | `{base}/dumps/` | [0013](0013-postgres-storage.md) §8 |
| The **activity stream**, with `Last-Event-ID` | `GET /activity/stream` | [0020](0020-change-feeds.md) §4 |
| **Records**: header and body of any record in a public partition, redacted as any response is | `GET /record/{partition}/{offset}` | [0012](0012-api-requirements.md) §5 |
| **Checkpoints and segment manifests**, current and historical, as C2SP signed notes | `{base}/.well-known/tlog/{partition name}/checkpoint` and `/checkpoint/{tree size}`; manifests at `/segment/{n}` | [0006](0006-log-integrity-and-erasure.md) §6; this settles 0006 Q3 |
| **Inclusion and consistency proofs** | `GET /record/{partition}/{offset}/proof?checkpoint=`; `GET /.well-known/tlog/{name}/consistency?from=&to=`; and the multi-proof `GET /record/proofs?partition=&checkpoint=&from=&to=` for a range of records against one checkpoint, with their shared upper path sent once | [0006](0006-log-integrity-and-erasure.md) §9, [0012](0012-api-requirements.md) §5 |
| The **key chain**: every `key:` record of the tenant's `config` partition | `GET /.well-known/tlog/keys` | [0015](0015-record-format-and-partition-registry.md) §3, [0018](0018-tenants.md) §2 |

Only partitions whose export policy is `public` are published ([0005](0005-crate-organization.md) §4.1); `internal` and `private` partitions have no records, checkpoints or proofs here. The local-graph source dump is new; it exists so that a reader can take a tenant's own assertions without the tenant's mirrors, for the reason §2 gives.

### 2. The Triplespace adapter reads the local graph and verifies it (extends 0002 §8.4; amends 0028 §5)

*Changed by A2.*

`scatter-adapter-triplespace` ([0028](0028-tenancy-policy.md) §5) is the adapter for a provider that is itself a Triplespace instance. Three rules refine what 0028 said of it.

**It reads the provider's `local` graph, and nothing else.** A provider's Wikibase-compatible dump is its *resolved view*, which includes its Wikidata mirror, its OpenAlex mirror and every other provider it reads. A reader has those from their sources already, under their own codes. So the adapter bootstraps from the local-graph source dump (§1) and follows the stream filtered to `source = local`, and the mirror partition `mirror/{provider}` holds only what the provider itself asserts. References inside those assertions are rewritten as [0018](0018-tenants.md) §5 rewrites them: the provider's own `Q6` becomes `LBQ6`; a reference to a global entity, `WDQ42` or `domain:x`, passes through; and a reference to **the reader's own entities**, which the provider holds under the reader's code (`EXQ9`), is rewritten **back to the bare local ID** `Q9`. That last rule is what makes §3 work.

**It verifies by default.** For each record it mirrors, the adapter fetches the record with its header, checks the header's leaf against the provider's latest checkpoint with an inclusion proof, checks the checkpoint's signature against the provider's key chain, and checks consistency between the checkpoint it last saw and the current one ([0006](0006-log-integrity-and-erasure.md) §5–6, §9). A batch of N records is verified with one checkpoint, one consistency proof and one multi-proof (§1), at O(N log N) hashes; every record is verified, always, and "verified" never means "probably". The **provider's header** (partition, offset, revision ID, commitment) and the **checkpoint tree size** it was verified against are stored in the mirror record's content part beside the entity state, so that a third party holding the reader's log can re-verify against the provider without trusting the reader. A verification failure fails the sync job ([0002](0002-source-graphs-and-mass-ingest.md) §8.3), notifies the job's operator ([0021](0021-notifications.md) §2, reason `job`), and leaves the mirror where it was; nothing unverified is appended.

**Trusted is the fallback, not the default.** A provider registered with `trust = stream` ([0015](0015-record-format-and-partition-registry.md) §5, `providers.toml`) is mirrored from its stream and dumps without proofs, as any non-Triplespace source is. The provider registry entry records which; the identity line and the source chip ([0010](0010-site-ui.md) §2) show a verification mark for a `verified` provider and none for a `stream` one.

**Erasure and hiding follow.** A provider's `erase` record travels on its stream ([0006](0006-log-integrity-and-erasure.md) §7); the adapter erases the mirrored parts with reason class `upstream`, as [0011](0011-logs.md) §5 and [0015](0015-record-format-and-partition-registry.md) §4 do for Wikidata's hiding. A provider's `read` ACL ([0023](0023-moderation.md)) simply keeps the hidden statement out of its public stream and dump, so the reader never sees it; if the reader already held it, the `delete/statement` event on the stream tells the reader to drop it, which it does as an `upstream` erasure. A reader is therefore as faithful to a Triplespace provider as this instance is to Wikidata, by the same mechanism.

### 3. Mutual sync: two instances annotate each other's things

Nothing stops two instances reading each other, and the result is the property [0000](0000-init.md) §3 promised, between wikis. Librarybase (`LB`) reads example.wiki (`EX`) and example.wiki reads Librarybase:

- example.wiki asserts, in its `local` graph, a statement about `LBQ6`. Librarybase's adapter mirrors it into `mirror/example`, rewriting `LBQ6` back to `Q6` (§2). On Librarybase, `Item:Q6` now shows that statement with an `EX` source chip ([0003](0003-statement-ui.md) §6), reconciled under [0002](0002-source-graphs-and-mass-ingest.md) §3: Librarybase's local graph wins on terms, sitelinks and rank; statements union.
- Librarybase can override or suppress the foreign assertion locally ([0002](0002-source-graphs-and-mass-ingest.md) §7); the correction is tracked against upstream as any correction is, and "Report upstream" ([0003](0003-statement-ui.md) §4) links to example.wiki's own page for `LBQ6`.
- A `same-as` asserted on either instance ([0004](0004-identity-clusters-and-equivalence.md) §9) is a tier-1 link at home and a tier-2 link for the other, ranked by the reader's provider order, with conflicts held as 0004 §10 has them. Identity clusters therefore span instances without any instance controlling another's.
- Each instance's `local` graph stays the only thing it publishes as its own (§4). A third instance reads both directly; it never learns what Librarybase thinks example.wiki said, only what each said.

This is not replication and not consensus. Each instance keeps its own log, its own canonical IDs and its own view; what it gains is verifiable knowledge of what its partners assert, and a place on its pages to show it.

### 4. Provenance chains and re-export

A reader **never re-exports what it mirrored as its own**. Its local-graph source dump (§1) is its `local` partition alone; its full dump ([0013](0013-postgres-storage.md) §8) carries mirrored graphs marked by source for comparison, and a downstream instance that wants Librarybase's data reads Librarybase, whose code is the same everywhere because codes are allocated once in the registry ([0015](0015-record-format-and-partition-registry.md) §5). The metadata graph carries, for every mirrored statement, `pav:importedFrom` the provider's document node and `pav:retrievedFrom` the job ([0015](0015-record-format-and-partition-registry.md) §6), and for a verified provider the checkpoint it was verified against as `scatter:verifiedAgainst`. A statement's provenance is therefore one hop long by construction.

### 5. Beyond Triplespace

Any provider that publishes a C2SP tlog checkpoint, RFC 6962 proofs and records in the canonical encoding of [0006](0006-log-integrity-and-erasure.md) §2 can be mirrored as `verified`. Scatterbase is the intended second case: its claim partitions are `logged` ([0006](0006-log-integrity-and-erasure.md) §4), its checkpoints are the same format, and its claim payload declares its own parts ([0015](0015-record-format-and-partition-registry.md) §1), so a Scatterbase adapter is the Triplespace adapter with a payload mapping. A provider that publishes only a stream or dumps is `stream`.

## Part B — ActivityPub

### 6. Actors (extends 0021 §5)

*Changed by A3.*

[0021](0021-notifications.md) §5 gave each tenant one `as:Service` notifier. This ADR adds three actor kinds, each **opt-in** and each with its own keypair in `private.ap_key`, keyed by actor:

| Actor | Type | Who turns it on | Address |
|---|---|---|---|
| A local account | `as:Person` | The holder, in `Special:Account` | `acct:{name}@{host}`; document at `{base}/user/{id}` with `application/activity+json` |
| A talk page | `as:Group` | Its tenant, per namespace, in `site` configuration (`federation.talk_pages`) | `acct:{title}@{host}` for the subject's title, document at `{base}/page/{talk page id}` |
| A thread | `as:Collection`-bearing `as:Note` chain | Follows its talk page | Posts at `{base}/record/{partition}/{offset}`, as [0019](0019-discussions.md) §7 already names them |

A person who opts in makes their **user page and posts** followable; nothing else about them changes, and WebFinger answers for their name only once they have. `rel="me"` links ([0007](0007-actor-identity.md) §7) are emitted from the account's public account links in both directions, so a Mastodon profile can verify a wiki account and the reverse. Subsidiaries ([0024](0024-subsidiary-accounts.md)) may be actors too, which is how a bot's edits can be followed.

The **vanish page** ([0010](0010-site-ui.md) §11) gains a sentence: copies of posts delivered to other servers may persist after vanishing; the instance sends `Delete` for each, and cannot enforce it.

### 7. Outbound: what a follower receives

*Changed by A3.*

A talk-page `Group` **`Announce`s** every thread created on it and every post in those threads, which is the Lemmy pattern Mastodon and its kin understand. A person actor's outbox carries `Create` for their posts. Edits are `Update` with the new content; hiding or erasing a post ([0023](0023-moderation.md) §5, [0006](0006-log-integrity-and-erasure.md) §7) is `Delete` with an `as:Tombstone`, and a deleted thread `Delete`s its collection. Every object uses the fixed profile of [0019](0019-discussions.md) §10 with `source` carrying the markdown. Delivery is the signed `POST` of [0021](0021-notifications.md) §5, through the same `ops` queue, rate-limited in the `notify` class ([0024](0024-subsidiary-accounts.md) §5), fanned out to followers' shared inboxes. Followers are `private.ap_follower` rows per actor; a `Follow` is accepted automatically for a `Group` and for a person who opted in, and `Undo` removes it.

**Boards.** Boards (310) may be federated as talk pages are. A thread listed on a page whose `Group` is federated is announced by that `Group` too, with every later post; removing the listing sends `Undo` of the `Announce`. Inbound replies are governed by the thread's home.

Nothing about entity data is federated this way. Statements move between instances by Part A; ActivityPub carries speech.

### 8. Inbound: the rules fixed now, the protocol left open

*Changed by A2.*

Replies from fediverse actors may become posts on this instance, under rules fixed here and mechanics left for implementation:

- **A fediverse actor is a `federated` actor** ([0007](0007-actor-identity.md) §5): a surrogate `{base}/actor/{n}` of a new kind, whose actor record holds the remote actor IRI and its last-seen name. The IRI is content and is never a key; the surrogate is. Renames upstream append a new actor record, as for any foreign actor.
- **The `federated` group holds no permissions by default** ([0016](0016-permissions-and-access-control.md) §3). A tenant enables inbound replies per talk namespace with an ACL on that namespace ([0016](0016-permissions-and-access-control.md) §4, [0023](0023-moderation.md) §1) restricting `edit` to a group that includes `federated`, and nowhere else. A federated actor can never create a thread, edit a page or touch an entity.
- **Domain allow and deny lists**, in the shape of [0026](0026-sitelinks.md) §3, as `config` records of kind `federation-policy`: an instance deny list that wins, and a tenant mode with a list. The instance list is the spam list a farm operator keeps ([0028](0028-tenancy-policy.md) §8).
- **The signed activity is evidence.** An inbound post is a `post` record ([0019](0019-discussions.md) §1) attested by the surrogate, and its attestation part ([0015](0015-record-format-and-partition-registry.md) §1) gains an `evidence` field holding the remote activity and its HTTP signature, so the instance can show that it did not make the words up. This is the attestation slot [0006](0006-log-integrity-and-erasure.md) §3 reserved for client signatures, used for exactly that.
- **A remote `Delete` is followed** as [0011](0011-logs.md) §5 follows upstream hiding: the post's text part is erased with reason class `upstream`, and the tree keeps its shape ([0019](0019-discussions.md) §4).
- **Moderation is [0023](0023-moderation.md) unchanged**: hide, suppress, block the surrogate, protect the talk page; edit filters ([0030](0030-edit-filters.md)) see an inbound post in the `text` context with `user_kind = federated`.

- **Signatures.** the inbox accepts draft-cavage HTTP Signatures with a mandatory `Digest`, which is what every Mastodon-compatible server sends; RFC 9421 signatures and FEP-8b32 object-integrity proofs are implemented in `scatter-activitypub` behind `site` switches (`federation.accept_rfc9421`, `federation.accept_fep8b32`, both off) and turned on when the fediverse turns, with no change to the inbox path; outbound stays cavage until Mastodon accepts 9421 ([0021](0021-notifications.md) §5).
- **Replay.** every accepted activity's `id` is recorded in `ops.ap_inbox_seen` for `federation.replay_window` (default seven days) and a repeat is acknowledged with 202 and dropped; a request whose `Date` lies outside ±`federation.clock_skew` (default twelve hours) is rejected; inbound activities are verified on receipt and then handled through the `ops` queue, so a burst never blocks the request path.
- **Promotion.** a `federated` surrogate is never merged into a local account. A local account that proves control of the remote actor, by the instance finding a `rel="me"` link from the remote profile to its user page, may add it as a public **account link** under [0007](0007-actor-identity.md) §7, the same act as linking a Wikidata account; the surrogate stays the actor of every post it made, and contributions and history show "also {remote} here" as for any linked account. Attribution never moves, which is the invariant every inclusion proof relies on.

### 9. Tenancy (uses 0028 §8)

ActivityPub is per tenant, as its notifier is: each tenant has its own host, actors and keys. A farm sets defaults and locks for `federation.*` settings through templates ([0028](0028-tenancy-policy.md) §8) and keeps the instance deny list. Data federation (Part A) is between *tenants*, wherever they are hosted: a tenant on another instance is a provider like any other, and the same instance's tenants read each other directly ([0018](0018-tenants.md) §5).

### 10. Storage (extends 0013 §5.6)

- The mirror record content for a `verified` provider carries the provider header and checkpoint size (§2); `view.entity_source` gains `provider_revid` and `verified_at_size`.
- `private.ap_key` and `private.ap_follower` become per-actor (`actor_key` beside the notifier's tenant row); `private.ap_outbox_queue` is the `ops` delivery queue of [0021](0021-notifications.md) §8, unchanged.
- `view.actor` rows of kind `federated` with `raw` holding the remote IRI ([0013](0013-postgres-storage.md) §5.4).
- `view.registry` holds `federation-policy` records; `providers.toml` gains `trust` (`verified` or `stream`) and the provider's key-chain URL.

### 11. API and UI (extends 0012 §5, 0010 §2 and §11)

- **Publishing** (§1): `/dumps/local/`, `/.well-known/tlog/…`, and the existing record and proof routes.
- **Provider pages**: `Special:Providers` lists each provider with its trust mode, last verified checkpoint and lag; a `verified` chip on the identity line of mirrored entities.
- **Account page**: a **Fediverse** section (Public where it concerns actors, Private where it concerns keys): make this account followable, the follower count, `rel="me"` links.
- **Talk page header**: the `Group`'s handle when the namespace is federated.
- **REST**: `GET /providers/{slug}/verification` (last checkpoint, key chain, failures); the ActivityPub endpoints at their fixed paths for each actor kind.

### 12. Permissions (extends 0016 §2–3)

| Permission or group | Governs | Default |
|---|---|---|
| `ts-config` | `trust` mode per provider; `federation.*` settings; `federation-policy` lists | `bureaucrat` |
| `editmyoptions` | Making one's own account an actor | `user` |
| group `federated` | Remote actors' surrogates; no permissions | — |
| `protect` | Enabling inbound replies on a talk namespace by ACL | `sysop` |

### 13. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with `triplespace-federation` and every change this section listed. The table this section first gave is in A1.

## Consequences

- **An instance can prove what another instance said.** A mirror of a Triplespace provider carries the provider's headers and checkpoint, so its contents are checkable by anyone against the provider's key, which is more than any Wikibase can offer and is the purpose 0006 was written for.
- **Two wikis can annotate each other's things**, each keeping its own log, IDs and view, with every foreign assertion marked, overridable and reportable upstream. This is 0000 §3 between institutions.
- **Nothing is merged.** No account, permission, attribution or canonical ID crosses an instance boundary except as a mirrored fact or a held link.
- **Speech federates on ActivityPub; facts on the log.** The two never mix: a post is never an entity, and an entity change is never an activity.
- **Inbound is contained by construction**: a `federated` actor holds nothing until a tenant opens one talk namespace to it, and every word it contributes arrives with the remote server's signature attached.
- **Erasure propagates across instances** by the same mechanism it propagates from Wikidata, and 0006 §7's caveat that copies elsewhere cannot be forced holds here too.
- **One more crate**, and per-actor keys where there was one per tenant.

## Open questions

- **Q1.** ~~**Inbound protocol details** (§8): signature versions, integrity proofs, replay protection, and promotion of a `federated` actor.~~ *Settled by A2: cavage now, with 9421 and FEP-8b32 behind switches; an activity-ID seen table with a `Date` window; link, never merge.*
- **Q2.** ~~**Verification depth for very large providers**: whether to verify every record or sample under a `sample` trust mode with a stated rate.~~ *Settled by A2: every record, always; "verified" never means "probably". The cost is kept low by a **multi-proof**: `GET /record/proofs?checkpoint=&from=&to=` returns the inclusion proofs of a range of records against one checkpoint with their shared upper path sent once, so a batch of N costs one checkpoint, one consistency proof and O(N log N) hashes; a bootstrap from the source dump is verified the same way against the checkpoint the dump is stamped with ([0032](0032-sparql-update-stream.md) §6). The route joins §1's table and [0012](0012-api-requirements.md) §5; `scatter-integrity` produces and checks multi-proofs.*
- **Q3. A Scatterbase adapter** as the payload mapping of §5.
- **Q4. Witnesses across federated instances** ([0006](0006-log-integrity-and-erasure.md) Q1): whether federation partners should cosign each other's checkpoints.
- **Q5. Entity data on ActivityPub**: whether an entity's document node should be an `as:Article` that announces its revisions, for readers who want to follow an item. Deliberately not done here.
- **Q6. Followers as private state**: whether the follower list of a `Group` is public, as Mastodon shows it, or private as the notifier's is.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §8.4 | §2 | extends | 0002 A11 |
| [0005](0005-crate-organization.md) §2, §7 | §13 | amends | 0005 A18 |
| [0006](0006-log-integrity-and-erasure.md) §6 | §1 | extends | 0006 A8 |
| [0006](0006-log-integrity-and-erasure.md) Q3 | §1 | settles | 0006 Q3 |
| [0007](0007-actor-identity.md) §4, §5, §7 | §6, §8 | extends | 0007 A6 |
| [0010](0010-site-ui.md) §2, §11 | §6, §11 | extends | 0010 A12 |
| [0011](0011-logs.md) §5 | §8 | extends | 0011 A6 |
| [0012](0012-api-requirements.md) §5 | §1, §11 | extends | 0012 A11 |
| [0013](0013-postgres-storage.md) §5, §5.2, §5.6 | §1, §8, §10, §13 | amends | 0013 A9 |
| [0013](0013-postgres-storage.md) §4, §5.4, §5.5, §7, §8 | §1, §8, §10, §13 | extends | 0013 A9 |
| [0015](0015-record-format-and-partition-registry.md) §1 | §2, §8, §10 | amends | 0015 A5 |
| [0015](0015-record-format-and-partition-registry.md) §3, §5 | §2, §8, §10 | extends | 0015 A5 |
| [0016](0016-permissions-and-access-control.md) §2, §3 | §8, §12 | extends | 0016 A5 |
| [0019](0019-discussions.md) Q7 | — | settles | 0019 Q7 |
| [0020](0020-change-feeds.md) Q4 | §2 | settles | 0020 Q4 |
| [0021](0021-notifications.md) §5, §8 | §6–8, §10, §13 | extends | 0021 A2 |
| [0021](0021-notifications.md) Q5 | §8 | settles | 0021 Q5 |
| [0021](0021-notifications.md) Q6 | — | settles | 0021 Q6 |
| [0028](0028-tenancy-policy.md) §5 | §2 | amends | 0028 A2 |

## References

- [C2SP tlog-checkpoint](https://c2sp.org/tlog-checkpoint), [tlog-witness](https://c2sp.org/tlog-witness), [RFC 6962](https://www.rfc-editor.org/rfc/rfc6962)
- [ActivityPub](https://www.w3.org/TR/activitypub/), [Mastodon: ActivityPub](https://docs.joinmastodon.org/spec/activitypub/), [Lemmy federation (Group actors and Announce)](https://join-lemmy.org/docs/contributors/05-federation.html)
- [FEP-8b32: Object Integrity Proofs](https://codeberg.org/fediverse/fep/src/branch/main/fep/8b32/fep-8b32.md), [RFC 9421 — HTTP Message Signatures](https://www.rfc-editor.org/rfc/rfc9421)
- [PAV ontology](https://pav-ontology.github.io/pav/)
- [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md), [0021 — Notifications](0021-notifications.md), [0028 — Tenancy policy](0028-tenancy-policy.md)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §13
- **Summary:** 0005 §2 is the one crate table CI checks, and carries `triplespace-federation` and every change this section listed (0005 A18).

Replaced text (§13):

> | Layer | Crate | Change |
> |---|---|---|
> | Substrate | `scatter-integrity` | Verifying a remote checkpoint, key chain, inclusion and consistency proof from fetched bytes (§2); pure, as before |
> | | `scatter-activitypub` | Person, Group and Collection actor documents, `Announce`, `Update`, `Delete` and `Tombstone`, inbound activity parsing and signature verification (§6–8) |
> | Ingest | `scatter-adapter-triplespace` | Local-graph-only reads, the back-rewrite of the reader's own IDs, verification per batch, provider header storage, following `erase` and `delete/statement` (§2) |
> | Triplespace | `triplespace-federation` *(new)* | Per-actor keys and followers, outbox fan-out, inbox handling and the `federated` surrogate flow, the federation lists (§6–9). Takes ActivityPub delivery from `triplespace-notify`, which keeps the notifier and its channels, and becomes the third crate that reads `private` ([0013](0013-postgres-storage.md) §4), for `ap_key` and `ap_follower` only |
> | | `triplespace-rdf`, `triplespace-api-rest` | The local-graph source dump and `/.well-known/tlog/` (§1) |
>
> The workspace goes from forty-two crates to forty-three.

### A2. Decisions of 2026-09-27 (evening)

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** amends §8; extends §1, §2
- **Summary:** By section:
  - §8: Inbound protocol details (decision 13). *Signatures:* the inbox accepts draft-cavage HTTP Signatures with a mandatory `Digest`, which is what every Mastodon-compatible server sends; RFC 9421 signatures and FEP-8b32 object-integrity proofs are implemented in `scatter-activitypub` behind `site` switches (`federation.accept_rfc9421`, `federation.accept_fep8b32`, both off) and turned on when the fediverse turns, with no change to the inbox path; outbound stays cavage until Mastodon accepts 9421 ([0021](0021-notifications.md) §5). *Replay:* every accepted activity's `id` is recorded in `ops.ap_inbox_seen` for `federation.replay_window` (default seven days) and a repeat is acknowledged with 202 and dropped; a request whose `Date` lies outside ±`federation.clock_skew` (default twelve hours) is rejected; inbound activities are verified on receipt and then handled through the `ops` queue, so a burst never blocks the request path. *Promotion:* a `federated` surrogate is never merged into a local account. A local account that proves control of the remote actor, by the instance finding a `rel="me"` link from the remote profile to its user page, may add it as a public **account link** under [0007](0007-actor-identity.md) §7, the same act as linking a Wikidata account; the surrogate stays the actor of every post it made, and contributions and history show "also {remote} here" as for any linked account. Attribution never moves, which is the invariant every inclusion proof relies on.
  - §1, §2: Verification depth (decision 14): every record, always; no `sample` trust mode. The cost is kept low by a multi-proof, `GET /record/proofs?checkpoint=&from=&to=`, which returns the inclusion proofs of a range of records against one checkpoint with their shared upper path sent once, so a batch of N costs one checkpoint, one consistency proof and O(N log N) hashes. §1 had been given the route in place.

Replaced text (§8):

> Left open: which HTTP Signatures versions and object-integrity proofs to accept, inbox queueing and replay protection, and whether a `federated` actor may be promoted to a fuller kind by a local account that proves control of it.

Replaced text (§2):

> A batch of records is verified with one checkpoint and one consistency proof; the cost is a handful of hashes per record.

### A3. Boards

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §10
- **Change:** extends §6, §7
- **Summary:** Boards (310) may be federated as talk pages are. A thread listed on a page whose `Group` is federated is announced by that `Group` too, with every later post; removing the listing sends `Undo` of the `Announce`. Inbound replies are governed by the thread's home.

### A4. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–13
- **Summary:** A1–A3 were folded into the Decision. The open questions were numbered. The header's note that a placeholder of the same date was replaced is dropped. No decision changed. Before this, A2's §8 part and A3 were blockquotes. The file before conversion is commit `0b26a3a`.
