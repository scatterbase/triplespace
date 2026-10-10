# 0022. Federation: verified data sync and ActivityPub

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A12)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md), [0028](0028-tenancy-policy.md)
- **Uses:** [0000](0000-init.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0014](0014-caches-and-search.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0026](0026-sitelinks.md), [0030](0030-edit-filters.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

"Federation" entered these ADRs through [0021](0021-notifications.md), which took the smallest step onto the fediverse, one notifier actor per tenant, and left the rest to this ADR: followable users and talk pages, and replies from elsewhere. James's observation is that the word covers a second thing the design already makes possible: **synchronizing data between instances.** [0000](0000-init.md) §3 lets foreign entities sit beside local ones; [0002](0002-source-graphs-and-mass-ingest.md) mirrors a provider's graph; [0018](0018-tenants.md) §5 and [0028](0028-tenancy-policy.md) §5 made one Triplespace instance a provider to another, first by direct partition reads and then by a sync over the activity stream. And [0006](0006-log-integrity-and-erasure.md) gives every record on every instance a signed, provable place in its log, which a mirror of Wikidata can never have. What none of them says is how one instance *verifies* another, what it reads, and what happens when two instances read each other.

The two halves share a stance. Triplespace federates **facts and speech, not identity**: an instance mirrors another's assertions and can prove they were made; a person on one instance can be followed and answered from elsewhere; but no account, no permission and no attribution is ever merged across instances, exactly as [0018](0018-tenants.md) §4 keeps them apart across tenants.

## Part A — Data federation

### 1. What a Triplespace instance publishes (settles 0006 Q3)

*Changed by A2, A10, A12.*

*Current text: [17](../architecture/17-federation-and-publication.md) §1.1.*

### 2. The Triplespace adapter reads the local graph and verifies it (extends 0002 §8.4; amends 0028 §5)

*Changed by A2, A10, A11, A12.*

*Current text: [17](../architecture/17-federation-and-publication.md) §1.2.*

### 3. Mutual sync: two instances annotate each other's things

*Current text: [17](../architecture/17-federation-and-publication.md) §1.3.*

### 4. Provenance chains and re-export

*Current text: [17](../architecture/17-federation-and-publication.md) §1.4.*

### 5. Beyond Triplespace

*Current text: [17](../architecture/17-federation-and-publication.md) §1.5.*

### 6. Actors (extends 0021 §5)

*Changed by A3.*

*Current text: [17](../architecture/17-federation-and-publication.md) §2.1.*

### 7. Outbound: what a follower receives

*Changed by A3, A6, A8.*

*Current text: [17](../architecture/17-federation-and-publication.md) §2.2.*

### 8. Inbound: the rules fixed now, the protocol left open

*Changed by A2.*

*Current text: [17](../architecture/17-federation-and-publication.md) §2.3.*

### 9. Tenancy (uses 0028 §8)

*Current text: [17](../architecture/17-federation-and-publication.md) §1.6, §2.4.*

### 10. Storage (extends 0013 §5.6)

*Changed by A7, A12.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.2, §4.5, §4.6, §4.16, §5.*

### 11. API and UI (extends 0012 §5, 0010 §2 and §11)

*Changed by A5.*

*Current text: [18](../architecture/18-api.md) §3.2; [19](../architecture/19-site-ui.md) §1.3, §1.5, §2.4, §6.11.*

### 12. Permissions (extends 0016 §2–3)

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §3.1, §8.3.*

### 13. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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
- **Q4.** ~~**Witnesses across federated instances** ([0006](0006-log-integrity-and-erasure.md) Q1): whether federation partners should cosign each other's checkpoints.~~ *Settled by [0081](0081-recovery-keys-and-continuations.md) §6: they may, since an instance can serve the tlog-witness API under `witness.enabled`.*
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

### A5. Page repositories on `Special:Providers`

- **Date:** 2026-10-01
- **Source:** [0053](0053-mirrored-pages.md) §3, §5
- **Change:** extends §11
- **Summary:** `Special:Providers` lists page repositories with their index date, event lag and mirror sync lag beside the data providers.

### A6. Pins and foreign threads

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §4, §8
- **Change:** extends §7
- **Summary:** A `Group`'s `featured` collection is its pinned threads; foreign threads are never announced.

### A7. The delivery queue is `ops.delivery`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §10
- **Summary:** The delivery queue is one `ops` table, named `ops.delivery` by [0021](0021-notifications.md) §8 (0021 A12), and §10 uses that name in place of `private.ap_outbox_queue`, which named a `private` table for what was already the `ops` queue. (PENDING A8)

Replaced text (§10):

> - `private.ap_key` and `private.ap_follower` become per-actor (`actor_key` beside the notifier's tenant row); `private.ap_outbox_queue` is the `ops` delivery queue of [0021](0021-notifications.md) §8, unchanged.

### A8. The `federation` rate class

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §7
- **Summary:** ActivityPub fan-out has its own rate class, `federation`, counted per instance against the delivery queue and never against the posting actor; the `notify` class of [0024](0024-subsidiary-accounts.md) §5 stays for requests an actor initiates. A post to a well-followed `Group` therefore cannot exhaust its author's limits. (PENDING F13)

Replaced text (§7):

> Delivery is the signed `POST` of [0021](0021-notifications.md) §5, through the same `ops` queue, rate-limited in the `notify` class ([0024](0024-subsidiary-accounts.md) §5), fanned out to followers' shared inboxes.

### A9. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§13
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A10. The chain of custody on the key chain endpoint and in the adapter

- **Date:** 2026-10-09
- **Source:** [0081](0081-recovery-keys-and-continuations.md) §7, §9
- **Change:** extends §1, §2
- **Summary:** `/.well-known/tlog/keys` serves a tenant's `recovery-key`, `continuation` and `continuation-cancel` records with its key records (§1). The Triplespace adapter follows a `recovered` continuation after its delay with no cancel, stops on an `unauthorized` one or a fork until an administrator accepts with a `provider/accept-continuation` event, and follows nothing silently (§2).

### A11. The verified sync resumes from the stream's per-partition token

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §7
- **Change:** extends §2
- **Summary:** The activity stream's resume token is a vector of per-partition high-water marks, since offsets are commit-ordered under the append lock, and the adapter of §2 resumes from that token when it follows the provider's stream filtered to `source = local`, so that a reader never misses a record that committed late; `(time, partition, offset)` stays the display order of paged lists. (REVIEW G14)

### A12. Verified federation bootstraps from the export bundle; `verified_at_size` is cursor state

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §2, §10; extends §1
- **Summary:** An instance publishes, beside the local-graph source dump, the export bundle of its `local` partition (segments with headers and bodies, checkpoints, manifests and the key records a verifier needs); a `verified` reader bootstraps from the bundle, which carries the canonical record bytes the leaf hashes commit to, and a `stream` reader from the source dump. The activity stream filtered to `source = local` may carry each row's record header and body (`bodies=1`), so the steady-state path fetches no records one by one and `GET /record/{partition}/{offset}` fills gaps only. The provider's header stays in the mirror record's content part, where it is stable; the checkpoint tree size a record was last verified against lives in `view.entity_source.verified_at_size` only, written by the sync job as cursor state, so a re-verification against a newer checkpoint updates the column and writes no `put`. A `delete/statement` event joins the log-event catalogue, telling a reading instance to drop a statement its provider hid. The skolem hash of blank nodes is over the statement UUID, never the entity part, with the snak role, property and index, reference snaks hashing the reference hash, fixed in `wikibase-compat.md` ([0032](0032-sparql-update-stream.md) §7). The ledger names §11; its chapter text ([18](../architecture/18-api.md) §3.2) is unchanged, and §1 gained the bundle and the `bodies` parameter. (REVIEW G48)

Replaced text ([17](../architecture/17-federation-and-publication.md) §1.2, as it stood):

> So the adapter bootstraps from the local-graph source dump (§1.1) and follows the stream filtered to `source = local`, and the mirror partition `mirror/{provider}` holds only what the provider itself asserts.
>
> **It verifies by default.** For each record it mirrors, the adapter fetches the record with its header, checks the header's leaf against the provider's latest checkpoint with an inclusion proof, checks the checkpoint's signature against the provider's key chain, and checks consistency between the checkpoint it last saw and the current one ([0006](../decisions/0006-log-integrity-and-erasure.md) §5–6, §9; [01](../architecture/01-log-and-records.md) §4).
>
> The **provider's header** (partition, offset, revision ID, commitment) and the **checkpoint tree size** it was verified against are stored in the mirror record's content part beside the entity state, so that a third party holding the reader's log can re-verify against the provider without trusting the reader; `view.entity_source` holds them in `provider_revid` and `verified_at_size` ([03](../architecture/03-storage-caches-and-search.md) §4.2).

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §4.2, as it stood):

> **Verified providers.** The mirror record content for a `verified` provider carries the provider header and checkpoint size ([0022](../decisions/0022-federation.md) §2); `view.entity_source` holds them in `provider_revid` and `verified_at_size`.
