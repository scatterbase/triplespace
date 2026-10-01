# 0023. Protection, deletion, hiding and patrolling

- **Status:** Proposed
- **Date:** 2026-09-27
- **Author:** James Hare / Claude Fable
- **Amended by:** [0028 — Tenancy policy](0028-tenancy-policy.md) (§4 settles the cross-tenant actor ACL open question), [0030 — Edit filters](0030-edit-filters.md) (§4 adds the `unpatrol` action to §6; §5 places filter records beside ACLs in `log`). §2 was amended on 2026-09-27 to add the `statement` and `property` target kinds and the predicate axis of enclosure, [0039 — Files, blob storage and foreign file repositories](0039-files-and-media.md) (§8 extends §1: delete, erase, reclaim, take down, reinstate and expunge; §10 extends §2: the instance `blob` target)
- **Related:** [0001 — Revision metadata in RDF](0001-revision-metadata-rdf.md) (§4: hiding as application policy; §5 gives it a record), [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (§5; §2 places upstream deletion and retention in the taxonomy), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md) (§6 amends §4: a deleted member leaves its cluster), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§13 amends §2), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (Context: hide, strike, erase; §5 gives *hide* its record), [0007 — Actor identity](0007-actor-identity.md) (§5 amends §4: the `hidden` status is an ACL), [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md) (§4 amends §4: page `delete` and `undelete` are ACLs; §4 settles the protection open question), [0010 — Site UI](0010-site-ui.md) (§9 extends §2, §5 and §7; §7 settles the patrolling open question), [0011 — Upstream and local logs](0011-logs.md) (§7 amends §6.1: protection, deletion, hiding and patrol events come from `acl` and patrol records in the local log; settles the patrol-log open question), [0012 — API requirements for the site UI](0012-api-requirements.md) (§8 extends §4 and §5), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§10 amends §5), [0014 — Cache layers and search](0014-caches-and-search.md) (§10 extends §5), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (§3 amends §3: `acl` is a payload type, not a config kind), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§2–3 amend §4: read ACLs, record and actor targets, moderation ACLs in `log`; §11 extends §2; settles the patrolling open question), [0019 — Discussions](0019-discussions.md) (§4 amends §1: thread `delete` and `undelete` are ACLs), [0020 — Change feeds](0020-change-feeds.md) (§8 extends §1: patrol filter; settles the patrolling open question), [MediaWiki API contract](../api/mediawiki-compat.md), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§8 moderates federated actors with §1–5 unchanged)

## Context

MediaWiki gives administrators four tools that the earlier ADRs name but do not specify: page protection, page deletion and undeletion, RevisionDelete (hiding the text, summary or username of one revision, with a suppression level that hides them from administrators too), and patrolling. [0016](0016-permissions-and-access-control.md) settled *who* may do each of them; this ADR settles *what each one is*.

"Deletion" in particular covers several things that Triplespace already tells apart, and the taxonomy is worth stating before adding to it:

| What happens | What it is | Settled in |
|---|---|---|
| Upstream deletes an entity and the mirror follows | A `tombstone` in the mirror graph; compaction erases the data | [0002](0002-source-graphs-and-mass-ingest.md) §5 |
| Upstream deletes an entity and this instance keeps it | The `retain` policy: the last state is materialized into the local graph | 0002 §5 |
| An editor removes a statement, or reverts an edit | A later record that undoes an earlier one; both stay in history | 0002 §8.2, [0010](0010-site-ui.md) §6 |
| Content must leave the log itself | `erase`, per part, leaving a verifiable gap | [0006](0006-log-integrity-and-erasure.md) §7, [0015](0015-record-format-and-partition-registry.md) §1 |
| Upstream hides part of a revision or log event | Erasure of that part with reason class `upstream` | [0011](0011-logs.md) §5, 0015 §4 |
| **A local page, thread or entity is deleted** | **This ADR** | |
| **Part of a local revision is hidden or suppressed** | **This ADR** | |
| **An account's name is hidden** | **This ADR** | |

The first five are settled and are not changed here. What the last three have in common is that nothing leaves the log: 0006 calls this **hiding**, distinguishes it from striking and erasing, and [0001](0001-revision-metadata-rdf.md) §4 leaves it to "application policy". [0015](0015-record-format-and-partition-registry.md) §1 already maps the three RevisionDelete bits onto the three body parts, content, comment and attestation, for *erasure*; it says hiding uses "the same three bits" as "a projection-level flag", without saying what record sets the flag. [0008](0008-namespaces-and-document-pages.md) §4 and [0019](0019-discussions.md) §1 gave pages and threads `delete` and `undelete` operations of their own, and [0011](0011-logs.md) §6.1 left `protect/*`, `delete/revision` and patrolling to "records in the local log, once these features exist". Local entities can not be deleted at all: no ADR defines it, though Wikibase allows it and `wbgetentities` reports deleted items as `missing`.

The observation this ADR builds on is James's: **protection and deletion are both access-control policy applied to one thing.** A protected page is one that only some group may edit. A deleted page is one that only some group may read. A revision with a hidden summary is one whose comment only some group may read. A suppressed one is the same with a smaller group. [0016](0016-permissions-and-access-control.md) §4 already has ACLs by target with conjunctive evaluation over enclosures; it used them only for writes. Extending them to reads gives every administrative action here one record shape, one evaluation path, one log, and one way to undo it.

Patrolling is the odd one out, since it restricts nothing: it is a mark that a trusted editor has looked at a change. It is included because MediaWiki's recent-changes tooling assumes it, and because four ADRs ([0010](0010-site-ui.md), [0011](0011-logs.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md)) deferred it to the same place.

## Decision

### 1. One mechanism: an ACL on a target

Every administrative action in this ADR except patrolling is an **ACL record** ([0016](0016-permissions-and-access-control.md) §4): a restriction of named permissions on one target to one group, until an expiry. Undoing the action retires the record.

| Action | Target | Permission restricted | Restricted to | MediaWiki equivalent |
|---|---|---|---|---|
| **Protect** | A page, thread, talk page, entity or namespace | `edit`, `move`, or both; `createpage` for a title that does not exist yet (§2) | `autoconfirmed` (semi-protection) or `sysop` (full), or any group | Page protection; `protected_titles` |
| **Delete** | A page, thread or local entity | `read` | The deletion group, `sysop` by default | Page deletion; the `archive` table |
| **Hide** (revision deletion) | One record, any subset of its parts | `read`, on the named parts | `sysop` | RevisionDelete bits 1, 2 and 4 |
| **Suppress** | As Hide, or a whole page or entity | `read` | `suppress` | RevisionDelete bit 8; suppression |
| **Hide a username** | An actor | `read` | `suppress` | `hideuser` on a block |

- **Restricting `read` to a group hides the target from everyone outside it.** Evaluation is [0016](0016-permissions-and-access-control.md) §4's, unchanged: to read *T*, an actor must hold `read`, which `universe` does, **and** satisfy every ACL restricting `read` on *T* or on a target enclosing *T*. A deletion ACL naming `sysop` is satisfied only by members of `sysop`. Nothing new is evaluated; the write-only assumption is dropped.
- **Suppression is a narrower group, not a different mechanism.** The same record with `suppress` in place of `sysop` hides the target from administrators too. MediaWiki's fourth bit is this and nothing more.
- **Groups, not rights, name who may still see.** MediaWiki gates deleted content on the `deletedhistory` and `deletedtext` rights and suppressed content on `viewsuppressed`. Here the ACL names a group, and those rights are what membership in that group reports to `meta=userinfo` ([0016](0016-permissions-and-access-control.md) §2). The default deletion group is `site` configuration (`moderation.delete_group`, default `sysop`; `moderation.suppress_group`, default `suppress`), so an instance that wants a separate reviewers group can have one without a new mechanism.
- **Nothing here touches the log.** Hidden and deleted content stays in its records, keeps its IDs, its place in the Merkle tree and its inclusion proofs ([0006](0006-log-integrity-and-erasure.md)). Removing content from the log is `erase`, and an `erase` may follow a hide when hiding is not enough.
- **Foreign entities are not deleted here.** What upstream holds is upstream's to delete ([0002](0002-source-graphs-and-mass-ingest.md) §5). A local assertion *about* a foreign entity is a local record and can be hidden like any other; the entity `WDQ42` itself cannot be. Keyed entities ([0009](0009-keyed-entity-types-and-domain.md) §4) likewise: a Domain exists because its key is valid, so it has no deletion; its local records can be hidden.

> **Extended by [0039](0039-files-and-media.md) §8–10.** For files, removal has two axes: who acts (a tenant, or the instance operator across every tenant) and how final it is. Tenants **delete** (this section), **hide** (§5) and **erase**; the instance **reclaims** versions that have stayed deleted for a retention period; operators **take down** and **reinstate**, and **expunge**, which erases every reference to a hash and destroys its bytes.

### 2. Targets (extends 0016 §4)

Two target kinds are added to the table of [0016](0016-permissions-and-access-control.md) §4, and one existing kind gains a use:

| Target kind | Key | Restricts | Added for |
|---|---|---|---|
| `record` | `acl:record:{partition}:{offset}` | `read` of the named **parts** of one record. The ACL's content carries `parts`, any subset of the payload type's parts (`content`, `comment`, `attestation`, and `text` for threads, [0019](0019-discussions.md) §4); an empty list means the whole record | Hiding and suppressing revisions and log events |
| `actor` | `acl:actor:{key}` | `read` of the actor's name and other attributes, everywhere they appear; the actor key and IRI, which never contain a name ([0007](0007-actor-identity.md) §2), stay visible | Hiding a username |
| `page` (existing) | `acl:page:{page id}` | Now also `read` (deletion) and, for a **reserved** page ID, `createpage` (below) | Deletion; create-protection |
| `entity` (existing) | `acl:entity:{id}` | Now also `read` (deletion of a local entity) | Entity deletion |

> **Amended 2026-09-27: `statement` and `property` targets.** The two kinds [0016](0016-permissions-and-access-control.md) §4 reserved are added, and inherit everything above.
>
> | Target kind | Key | Restricts |
> |---|---|---|
> | `statement` | `acl:statement:{guid}`, the GUID as stored in the log; the UUID part identifies the statement across the canonical-ID rewrite of [0018](0018-tenants.md) §7 | `edit`: changing, overriding or removing that one statement, its qualifiers and references. `read`: the statement is left out of the resolved view, RDF, search and diffs for viewers outside the group |
> | `property` | `acl:property:{id}` | `edit`: writing any snak whose property is *P*, as main snak, qualifier or reference, on any entity. `read`: every snak with predicate *P* is left out for viewers outside the group |
>
> **Enclosure gains a second axis.** Containment (graph ⊃ namespace ⊃ entity ⊃ statement) is joined by **predicate**: a property ACL applies to every snak using that property, wherever it sits. Evaluation stays conjunctive: to edit statement *S* on entity *E*, an actor satisfies the ACLs on *S*, on *E*, on *E*'s namespace, on the graph, and on *S*'s property. The cost is one indexed lookup per property a write touches, cached in L0 ([0014](0014-caches-and-search.md) §2); on the read side the resolution projection asks `view.acl` for the entity's GUIDs and properties in one query, and the public form of the resolved view ([0014](0014-caches-and-search.md) §1) simply lacks what a read ACL hides.
>
> **A read ACL on a mirrored statement is not an `override`.** [0002](0002-source-graphs-and-mass-ingest.md) §3's suppression is an editorial act, a local record public in history, and the resolved view says the statement was suppressed. A `read` ACL is moderation: the statement is hidden for privacy or legal reasons, only the group sees that it exists, and the source graph is untouched. Both are available; they answer different questions. Setting either target kind needs `protect` for `edit` and `deleterevision` for `read`, as for a page; they project as `protect/*` and as a new `delete/statement` event. This settles 0016's open question and answers Scatterbase's property-specific rule.

**Create-protection reserves a page ID.** A title that does not exist has no page ID, and a key must never be content ([0006](0006-log-integrity-and-erasure.md) §3), so the protecting record takes a page ID from the sequence ([0015](0015-record-format-and-partition-registry.md) §2) and carries the namespace and title in its content part. The title index maps the title to the reserved ID; a `create` by an actor the ACL admits takes that ID, and the page then exists. This is the pattern [0019](0019-discussions.md) §2 uses for talk-page IDs.

**Enclosure is unchanged** from [0016](0016-permissions-and-access-control.md) §4 and [0019](0019-discussions.md) §12: a namespace encloses its pages and entities, a page its subpages, a talk page its threads. So a `read` ACL on a namespace makes it private, and deleting a talk page deletes its threads. **A subject page does not enclose its talk page**, in either direction: protecting an item leaves its talk page open, which is what protection is for, and deleting an item leaves its talk page unless the administrator deletes that too. The delete form offers to, as MediaWiki's does, and writes two records.

> **Extended by [0039](0039-files-and-media.md) §10.** A `blob` target at instance scope, keyed `acl:blob:{takedown id}`, encloses every file version, in every tenant, whose SHA-256 or SHA-1 equals the hash in the record. Content becomes an axis of enclosure beside containment and predicate. A takedown restricts `read` and `upload` on it to no group.

### 3. Records (amends 0015 §3 and 0016 §4)

**`scatter:v0/acl` is a payload type, not a `config` kind.** [0016](0016-permissions-and-access-control.md) §4 made ACLs `config` records of kind `acl`. Graph ACLs are configuration and stay in the tenant `config` partition. Every other target is **moderation**, and its records go to the tenant `log` partition ([0011](0011-logs.md) §2), which is where 0011 §6.1 already expected protection to be written: it is "an action with no other record". The same payload type is used in both partitions; `docs/registry/graphs.toml` lists it for both.

Why `log` and not `config`: moderation records are frequent, they are not configuration, and the `config` partition is exported publicly. Suppression records must not be. The `log` partition is `logged`, `full` and **internal**: served by the API under §7's visibility rule, never in a public dump.

| | |
|---|---|
| Header key | The target key of §2, so the latest record per key is the current ACL, and erasing a target's history by key ([0006](0006-log-integrity-and-erasure.md) §7) takes its ACL history with it |
| Content part | The target; for each restricted permission, the group and an optional expiry; `parts` for a `record` target; the reserved title for create-protection; for a whole-page or whole-entity deletion, the flag `talk: true` when the paired talk page is deleted by the same action. A content of `null` retires the ACL |
| Comment part | The reason. This is MediaWiki's deletion, protection or suppression reason, and a `record` ACL on this record's `comment` part hides it if the reason is itself abusive |
| Attestation | Who acted, as for every record |

**Undelete, unhide and unprotect are retirements.** A record with null content, same key. **Partial undeletion**, MediaWiki's restoring of some revisions but not others, is a retirement of the page ACL followed by `record` ACLs on the revisions that stay hidden; both are written in one request. **Expiry** applies to protection as MediaWiki's does; a deletion is written without one.

**Log events**, projected from these records as [0011](0011-logs.md) §6.1 requires:

| ACL | Log event | Parameters ([0011](0011-logs.md) §7) |
|---|---|---|
| Protect: written, changed, retired | `protect/protect`, `protect/modify`, `protect/unprotect` | Each restriction with its group and expiry |
| Delete: written, retired | `delete/delete`, `delete/restore` | Whether the talk page went with it; on restore, the record ACLs that stay |
| Hide a record in `local`, `pages` or `actors` | `delete/revision` | The revision ID, the parts hidden and the parts revealed |
| Hide a record in `log` | `delete/event` | The log ID, likewise |
| Suppress (any of the above with the suppression group) | `suppress/delete`, `suppress/revision`, `suppress/event` | As above |
| Hide a username | `suppress/hide-user` (MediaWiki writes this as `suppress/block` with `hideuser`) | The actor key |

A record whose target is itself hidden appears with its target hidden, as MediaWiki shows `(username removed)`.

### 4. What deletion does (amends 0008 §4 and 0019 §1; amends 0004 §4)

**A deleted page or thread** ([0008](0008-namespaces-and-document-pages.md) §4, [0019](0019-discussions.md) §1) is a page with a `read` ACL naming the deletion group. The `delete` and `undelete` operations those ADRs gave page and thread records are **retired**; their log events (`delete/delete`, `delete/restore`) now come from the ACL, and everything they said about the effect stands:

- the page leaves the title index, so its title can be taken by a new page (`view.page.deleted` is derived from the ACL; the partial unique index of [0013](0013-postgres-storage.md) §5.4 already excludes deleted rows);
- it leaves search, backlinks, `list=allpages`, feeds and the resolved metadata graph for everyone outside the group;
- `action=parse`, `prop=revisions` and the page itself answer as MediaWiki does: the page is reported missing, with the deletion log entry shown in its place, and members of the group see the content behind a banner;
- a deleted thread leaves its talk page's listing and history; a deleted talk page takes its threads with it by enclosure.

**Undeleting into a taken title.** If a new page has taken the title meanwhile, the two have different page IDs and there is nothing to merge. The undelete is refused with a new error code, `ts-title-taken`, unless the request supplies a new title, in which case a `move` record follows the retirement in the same request.

**A deleted local entity** is a local entity with a `read` ACL naming the deletion group. While it holds:

- `wbgetentities` returns `missing`, `Special:EntityData/{id}` returns 404, and the entity leaves search, the term store, the resolved dump and every feed, for everyone outside the group. `oldid=N` permalinks to its revisions answer as MediaWiki does for a deleted revision.
- **It leaves its identity cluster** ([0004](0004-identity-clusters-and-equivalence.md) §4), exactly as a member tombstoned under `orphan` does ([0002](0002-source-graphs-and-mass-ingest.md) §5): the next member becomes canonical, and the mirrored data that member holds stays public under its own ID. `same-as` and `different-from` records naming the deleted entity are kept but not applied; on undelete they apply again and any conflict they now cause is held as [0004](0004-identity-clusters-and-equivalence.md) §10 describes.
- **Local assertions about it are kept but hidden**, with their `entity_ref` rows. Statements on other entities that point at it are untouched and dangle, as they do on Wikidata after an item is deleted; the UI shows the value with its ID and no label.
- Its `page_id` stays with it ([0015](0015-record-format-and-partition-registry.md) §2). Undeletion restores every row and rejoins the cluster.

Only local entities can be deleted: the ID must be in the local form of [0017](0017-entity-id-grammar.md) §1. `action=delete` on `Item:WDQ42` or `Domain:…` is refused with `ts-not-deletable`.

**Deletion is not a stale-copy problem.** Nothing is copied to an archive; the records stay where they are, and the projections that already exist read the ACL. MediaWiki's `archive` table, and its history-merge machinery, have no counterpart.

### 5. What hiding does (extends 0001 §4, 0006 Context, 0015 §1; amends 0007 §4)

**Hiding one revision's parts.** A `record` ACL with `parts` sets the visibility bits that [0012](0012-api-requirements.md) §3 and [0013](0013-postgres-storage.md) §5.5 already carry:

| Part hidden | Effect outside the group | Action API flag |
|---|---|---|
| `content` (or `text` for a thread record) | The diff, the change set and the page or post text are withheld; the row stays in history and feeds with its time, size and IDs | `texthidden` |
| `comment` | The summary disappears from history, recent changes, diffs, the metadata graph and notifications | `commenthidden` |
| `attestation` | The actor is shown as hidden; the row stays attributed to nobody, and the revision carries `scatter:attributionHidden` in RDF ([0007](0007-actor-identity.md) §9) | `userhidden` |

A suppressed part adds MediaWiki's `suppressed` flag for those who may see that much. This is the "projection-level flag" [0015](0015-record-format-and-partition-registry.md) §1 named; the erasure of a part is its irreversible form and uses the same bits.

**The same ACL on a `log` record** hides a log event's parts (`delete/event`): the comment part is the event's comment, the attestation its performer, the content its parameters and target.

**Hiding a username.** An `actor` ACL restricted to `suppress` is what sets the `hidden` status of [0007](0007-actor-identity.md) §4. While it holds, the name is omitted from every projection and response for everyone outside the group, `sioc:name` is not emitted, and every attribution to the actor reads as hidden. It is set by the `hideuser` right, and MediaWiki's practice of setting it on a block is kept as a checkbox on `Special:Block` that writes the ACL beside the `block` record ([0016](0016-permissions-and-access-control.md) §3). Hiding a name is reversible; removing a past name from the actor's history is `erase`, as 0007 §4 already describes for vanishing.

**Redaction is per viewer** ([0012](0012-api-requirements.md) §8), and hiding reaches every derived copy as erasure does ([0014](0014-caches-and-search.md) §5): the projection that applies an ACL bumps the target's generation, purges its keys and tags, deletes or re-sends its search document, and removes delivered notifications whose content is now hidden ([0021](0021-notifications.md) §2). Shared caches hold only the public form, so a hidden field never has to be purged from a cache that a member of the group filled.

### 6. Patrolling

**A patrol mark is a log event**, `patrol/patrol`, in the tenant `log` partition, keyed by the patrolled record's coordinates and carrying its revision ID, as MediaWiki's patrol log does. The projection sets `patrolled` on the activity row. ~~There is no unpatrol, as there is none in MediaWiki core.~~ *Since 2026-09-27 there is: `patrol/unpatrol`, same key, same right, and the newest event wins (open questions).*

**What is patrollable:** every activity row of kind `edit` in the tenant's `local` and `pages` partitions, which includes posts and thread operations ([0019](0019-discussions.md) §7), and page creations. Mirror syncs are not: upstream's edits are upstream's to review. A **job** is patrolled as one row; marking the job marks every change it made, which is the grouping [0010](0010-site-ui.md) §7 already gives jobs.

**Autopatrol is a projection rule, not a record.** A change made by an actor who held `autopatrol` when the record was appended is patrolled from the start. The projection decides this from the membership records in `actors` as of the record's `appended_at`, so a rebuild reaches the same answer, and no record is written per bot edit. MediaWiki writes a `patrol/autopatrol` entry for each; Triplespace does not, and `list=logevents&letype=patrol` returns only manual marks. Whether a viewer sees the unpatrolled mark at all depends on the `patrol` right, as in MediaWiki.

**Where the mark shows:** the `!` marker on rows in recent changes, related changes, the watchlist and histories ([0020](0020-change-feeds.md) §1) for viewers with `patrol`; a **Mark as patrolled** action on the diff and on the row; a `patrolled` filter on every feed, which is the `rcshow=patrolled|!patrolled` of `list=recentchanges` and `list=watchlist`. Marks are shown only within the recent-changes window (`site` setting `rc.max_age`, [0010](0010-site-ui.md) §7), as MediaWiki's are.

**Rights.** `patrol` marks; `autopatrol` exempts. Defaults are in §11.

### 7. Visibility of the moderation records themselves

Moderation is public in the way MediaWiki's logs are, with one exception:

- **Protection, deletion, revision-hiding and patrol events are visible to everyone.** That a page was deleted, and by whom and why, is public; the content is not. This is [0016](0016-permissions-and-access-control.md) §6's rule for erasures, applied uniformly.
- **A record whose group holds `viewsuppressed` is visible only to that group.** Its log event is `suppress/*`, and to everyone else the affected fields are simply hidden, with no event saying so. This is MediaWiki's private suppression log.
- **The `log` partition's export policy is internal** ([0011](0011-logs.md) §2), so none of these records enters a public dump; a full dump ([0013](0013-postgres-storage.md) §8) made by the instance for itself carries them, and it is the instance's to protect.
- **The downstream stream** ([0020](0020-change-feeds.md) §4) carries the public events, so a consumer learns that a revision's comment was hidden and can redact its copy, as [0011](0011-logs.md) §5 has this instance follow upstream. It never carries a `suppress/*` event or the content behind one.

### 8. API (extends 0012 §4 and §5; extends 0016 §7)

**Action API**, additively under [0012](0012-api-requirements.md) §1, with MediaWiki's meaning:

| Module | Behaviour |
|---|---|
| `action=protect` | `protections=edit=autoconfirmed\|move=sysop`, `expiry`, `reason`; writes or retires the ACL. `cascade=1` is refused with `ts-no-cascade`, since there is no cascading protection ([0016](0016-permissions-and-access-control.md) §4). On a title that does not exist, reserves a page ID (§2). On an entity page, restricts `edit` on the entity target |
| `action=delete`, `action=undelete` | Pages, threads and entity pages. `deletetalk=1` writes the second record of §2. `undelete` with `timestamps` or `revisions` performs the partial form of §3 |
| `action=revisiondelete` | `type=revision\|logging`, `ids`, `hide=content\|comment\|user`, `show=…`, `suppress=yes\|no\|nochange`, `reason`; writes or amends `record` ACLs. `type=archive` is accepted and treated as `revision`, since deleted revisions are not moved |
| `action=block` | Gains `hidename`, which writes the actor ACL beside the block |
| `action=patrol` | `revid` or `rcid`; writes the patrol event. `list=recentchanges` and `list=watchlist` gain `rcshow=patrolled\|!patrolled\|autopatrolled` and `rcprop=patrolled` |
| `prop=info` | `inprop=protection` reports the page's or entity's edit and move ACLs as MediaWiki `protection` entries; `list=protectedtitles` lists reserved titles; `list=protectedpages` the rest |
| `prop=deletedrevisions`, `list=alldeletedrevisions`, `list=deletedrevs` | The hidden records, for members of the group only |
| `wbgetentities` | `missing` for a deleted local entity; `prop=revisions` on its page as for a deleted page |

**REST**, under `rest.php/triplespace/v0`: `GET`, `PUT` and `DELETE /acl/{kind}/{id}` ([0016](0016-permissions-and-access-control.md) §7) accept `record` and `actor` kinds and a `parts` list; `POST /activity/{partition}/{offset}/patrol` marks a row. The activity row ([0012](0012-api-requirements.md) §3) gains `patrolled`, and its `visibility` field already reports hidden parts.

Every response is redacted for its viewer ([0012](0012-api-requirements.md) §8); a `read` ACL is one more input to that redaction, and the privacy test of 0012 §8 gains a case per target kind: nothing behind a `read` ACL is returned to a viewer outside its group, from any route, cache or index.

### 9. Site UI (extends 0010 §2, §5 and §7)

- **Overflow menu.** Pages and entities gain **Protect…** ([0016](0016-permissions-and-access-control.md) §7) and **Delete…**, the latter with the "also delete the talk page" option. Deleted pages show **Undelete…** to the group.
- **A deleted page or entity** shows, to everyone outside the group, the frame of [0010](0010-site-ui.md) §2 with the deletion log entry in place of the content, and a link to the log. To the group it shows the content under a banner naming who deleted it and why, with Undelete.
- **History rows** gain RevisionDelete checkboxes for holders of `deleterevision`, and a suppress option for holders of `suppressrevision`, opening a dialog with the three parts and a reason. A hidden part is drawn as [0010](0010-site-ui.md) §5.2 already draws hidden actors: in place, labelled, never silently absent.
- **Patrol.** The `!` marker, the Mark as patrolled action, and the `patrolled` filter, for holders of `patrol`.
- **Special pages.** `Special:Log/protect`, `/delete`, `/patrol` and, for the group, `/suppress`; `Special:ProtectedPages`, `Special:ProtectedTitles`, `Special:Undelete`, `Special:DeletedContributions`, and `Special:BlockList` gains the hidden-name column, all where MediaWiki users expect them.

### 10. Storage (amends 0013 §5)

- **`view.acl`** ([0016](0016-permissions-and-access-control.md) §9) holds moderation ACLs beside graph ACLs: `(target_kind, target_id, permission, parts smallint, "group", expires, partition, "offset")`, indexed by target. The evaluation path reads it for the target and its enclosures, cached in L0 ([0014](0014-caches-and-search.md) §2); the common case, no row, is one indexed miss.
- **Denormalized flags** keep lists from joining: `view.page.deleted` (already present) and a new `view.entity.deleted boolean NOT NULL DEFAULT false` are derived from `read` ACLs on the target; `view.activity.visibility` and `view.post.hidden` are derived from `record` ACLs together with the erased-parts bitmask of [0013](0013-postgres-storage.md) §2; `view.actor.status = 'hidden'` from an `actor` ACL.
- **`view.activity.patrolled boolean`** with a partial index `WHERE NOT patrolled` for the recent-changes filter, set by the patrol projection and the autopatrol rule of §6.
- **Projection order** ([0013](0013-postgres-storage.md) §7): the ACL projection runs in step 1 with the permission tables, since resolution, terms, search and activity all read it; it is synchronous for interactive writes, so a deletion is invisible the moment it commits.
- **Reserved page IDs** are rows in `view.page` with `latest_offset` null and a `reserved boolean`; the title index includes them so that `create` finds the ID.

### 11. Permissions (extends 0016 §2)

| Permission | Governs | Default groups |
|---|---|---|
| `protect` | Every protection ACL, including reserved titles; `edit` and `move` restrictions on pages, threads, talk pages, entities and namespaces | `sysop` |
| `delete`, `undelete` | `read` ACLs on pages, threads and local entities naming the deletion group; their retirement | `sysop` |
| `deleterevision`, `deletelogentry` | `record` ACLs on `local`, `pages` and `actors` records, and on `log` records, naming `sysop` | `sysop` |
| `suppressrevision` | Any `read` ACL naming the suppression group | `suppress` |
| `viewsuppressed` | Seeing what the suppression group may see, and the `suppress/*` events | `suppress` |
| `hideuser` | `actor` ACLs | `suppress` |
| `deletedhistory`, `deletedtext`, `browsearchive` | Reported to MediaWiki clients by members of the deletion group; they gate no evaluation of their own | `sysop` |
| `patrol` *(new default)* | Writing patrol events; seeing the unpatrolled mark | `autoconfirmed` |
| `autopatrol` *(new)* | Changes by this actor are patrolled from the start (§6) | `autopatrolled` *(new group)*, `bot`, `sysop` |

`autopatrolled` is a group administrators assign with `userrights`, as on Wikimedia projects. `docs/registry/groups.toml` is updated with the two permissions and the group.

**Ownership rules** ([0016](0016-permissions-and-access-control.md) §2) are unchanged; none applies here. A user may not delete or hide their own contributions without the right.

### 12. Scatterbase

Scatterbase inherits §1–3 and §5 whole. Read ACLs suit it better than deletion would: a signed claim's content stays in the log and verifiable, and only who may read it changes, which is the answer its decision record wanted for `view_deleted` and for restricting "individual claims". The `record` target with `parts` is the claim-level restriction it reserved. Patrolling is Triplespace's and is not shared.

### 13. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-actors` | ACL evaluation for `read`, with `parts` on `record` targets and the `actor` target; the autopatrol rule as a pure function over memberships as of a time |
| `scatter-log` | The `scatter:v0/acl` payload type, replacing the `acl` config kind; registered for the `config` and `log` partitions |
| `scatter-pages`, `scatter-threads` | The `delete` and `undelete` operations are removed from `scatter:v0/page` and `scatter:v0/thread` |
| `scatter-mwlog` | `patrol` and `suppress` log types; `protect/*` and `delete/*` parameters as typed in §3 |
| `triplespace-projections` | The ACL projection; derived `deleted`, `visibility`, `hidden` and `status` flags; the patrol projection; cluster re-evaluation on entity deletion and undeletion |
| `triplespace-titles` | Reserved page IDs in the title index |
| `triplespace-api-action`, `triplespace-api-rest` | The modules and routes of §8 |
| `triplespace-cache`, `triplespace-search` | Hiding and deletion on the purge path of 0014 §5, as erasure already is |

No crate is added.

## Consequences

- **Six administrative actions are one record type**, evaluated by code that already exists. Protect, delete, hide, suppress, hide a name and every undo of them are ACL records; only patrol is a second shape, and it is a plain log event.
- **Nothing is moved or copied.** Deletion has no archive table, undeletion no merge, and hiding no redacted copy. Every deleted or hidden thing keeps its IDs, its proofs and its place in history, and the log stays append-only.
- **Two earlier ADRs lose an operation.** 0008 §4's and 0019 §1's `delete` and `undelete` are retired before any code exists for them. Their log events and effects are unchanged.
- **Local entities can be deleted**, which closes a gap Wikibase clients would have hit at once, and the cluster rule of 0004 gets one more membership change to handle.
- **Suppression stays out of public dumps** because moderation records live in the internal `log` partition, at the cost of splitting ACL records across two partitions by target kind.
- **Reads consult ACLs.** Every response path already redacts per viewer; it now also asks whether the target is behind a `read` ACL. The denormalized flags keep the cost out of list queries, and the common case is one cached miss.
- **Patrolling is MediaWiki-shaped without MediaWiki's volume.** Autopatrol as a projection rule spares the log a record per bot edit, and `list=logevents&letype=patrol` differs from MediaWiki's only by omitting `autopatrol` entries.
- **Four open questions close:** patrolling in 0010, 0011, 0016 and 0020; page protection in 0008; the entity-deletion gap of the 2026-09-27 review.

## Open questions

- ~~**`statement` and `property` ACL targets** ([0016](0016-permissions-and-access-control.md), open questions) are unchanged: hiding one statement of a large local entity is not possible without them, only the whole record that added it.~~ *Settled by the 2026-09-27 amendment of §2: both targets exist, with predicate as a second axis of enclosure.*
- **Deleting a heavily referenced local entity.** Dangling values are Wikidata's behaviour, but an instance may want a warning, or a redirect, when an entity with many referrers is deleted.
- ~~**Abuse filters and pre-save checks**, still neither permissions nor ACLs ([0016](0016-permissions-and-access-control.md), open questions).~~ *Settled by [0030](0030-edit-filters.md): edit filters, a third mechanism beside permissions and ACLs, evaluated in the appending transaction.*
- **Reviewing threads.** Whether a post should be patrollable separately from the thread operation that carried it, and whether `thread-status` marks ([0019](0019-discussions.md) §6) should count as review.
- **`autopatrolled` thresholds**, if an instance wants the group granted automatically as `autoconfirmed` is.
- ~~**Unpatrol**, which MediaWiki lacks and some wikis want.~~ *Settled 2026-09-27: a `patrol/unpatrol` log event in the tenant `log`, keyed like `patrol/patrol` (§6) and written by holders of `patrol`; the projection sets `patrolled` false, and the newest of the two events for a record wins. `action=patrol` gains `unpatrol=1`; the row's action is "Mark as unpatrolled". [0030](0030-edit-filters.md) §4's `unpatrol` filter action, which withholds autopatrol as a write lands, is unchanged.*
- **Whether deletion should expire.** The record allows it; MediaWiki has no temporary deletion, and none is offered here.
- ~~**Blocking and hiding across tenants** ([0018](0018-tenants.md), open questions): whether an `actor` ACL written by the primary tenant should apply to every tenant.~~ *Settled by [0028](0028-tenancy-policy.md) §4: an actor ACL on a farm account applies everywhere; a tenant's applies to its own account.*

## References

- [Manual:Protection](https://www.mediawiki.org/wiki/Manual:Protection), [Manual:Page_restrictions table](https://www.mediawiki.org/wiki/Manual:Page_restrictions_table), [Manual:Protected_titles table](https://www.mediawiki.org/wiki/Manual:Protected_titles_table)
- [Manual:Archive table](https://www.mediawiki.org/wiki/Manual:Archive_table), [Help:Deletion and undeletion](https://www.mediawiki.org/wiki/Help:Deletion_and_undeletion)
- [Manual:RevisionDelete](https://www.mediawiki.org/wiki/Manual:RevisionDelete) (the `rev_deleted` bits) and [Help:RevisionDelete](https://www.mediawiki.org/wiki/Help:RevisionDelete)
- [Manual:Patrolled edits](https://www.mediawiki.org/wiki/Manual:Patrolled_edits), [Help:Patrolled edits](https://www.mediawiki.org/wiki/Help:Patrolled_edits)
- [API:Protect](https://www.mediawiki.org/wiki/API:Protect), [API:Delete](https://www.mediawiki.org/wiki/API:Delete), [API:Undelete](https://www.mediawiki.org/wiki/API:Undelete), [API:Revisiondelete](https://www.mediawiki.org/wiki/API:Revisiondelete), [API:Patrol](https://www.mediawiki.org/wiki/API:Patrol)
- Scatterbase decision record (`view_deleted`; URI ACLs over "individual claims")
