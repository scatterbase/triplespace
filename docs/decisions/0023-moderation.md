# 0023. Protection, deletion, hiding and patrolling

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-01 (A9)
- **Author:** James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0006](0006-log-integrity-and-erasure.md), [0009](0009-keyed-entity-types-and-domain.md), [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0021](0021-notifications.md), [0022](0022-federation.md), [0028](0028-tenancy-policy.md), [0030](0030-edit-filters.md), [MediaWiki API contract](../api/mediawiki-compat.md)

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

*Changed by A5, A9.*

Every administrative action in this ADR except patrolling is an **ACL record** ([0016](0016-permissions-and-access-control.md) §4): a restriction of named permissions on one target to one group, until an expiry. Undoing the action retires the record.

| Action | Target | Permission restricted | Restricted to | MediaWiki equivalent |
|---|---|---|---|---|
| **Protect** | A page, thread, talk page, board, entity, namespace or set | `edit`, `move`, or both; `createpage` for a title that does not exist yet (§2); `read`, as a **confidential** restriction that keeps the target live for its group and absent for everyone else ([0056](0056-security-model.md) §2, §4) | `autoconfirmed` (semi-protection) or `sysop` (full), or any group; for `read`, a group the actor belongs to | Page protection; `protected_titles`; no MediaWiki equivalent for `read` |
| **Delete** | A page, thread or local entity | `read` | The deletion group, `sysop` by default | Page deletion; the `archive` table |
| **Hide** (revision deletion) | One record, any subset of its parts | `read`, on the named parts | `sysop` | RevisionDelete bits 1, 2 and 4 |
| **Suppress** | As Hide, or a whole page or entity | `read` | `suppress` | RevisionDelete bit 8; suppression |
| **Hide a username** | An actor | `read` | `suppress` | `hideuser` on a block |

- **Restricting `read` to a group hides the target from everyone outside it.** This ADR's restrictions are the **moderation** kind: the target was removed, and outsiders see the notices below. The **confidential** kind, set with `protect`, hides even that the target exists ([0056](0056-security-model.md) §2, §5). Evaluation is [0016](0016-permissions-and-access-control.md) §4's, unchanged: to read *T*, an actor must hold `read`, which `universe` does, **and** satisfy every ACL restricting `read` on *T* or on a target enclosing *T*. A deletion ACL naming `sysop` is satisfied only by members of `sysop`. Nothing new is evaluated; the write-only assumption is dropped.
- **Suppression is a narrower group, not a different mechanism.** The same record with `suppress` in place of `sysop` hides the target from administrators too. MediaWiki's fourth bit is this and nothing more.
- **Groups, not rights, name who may still see.** MediaWiki gates deleted content on the `deletedhistory` and `deletedtext` rights and suppressed content on `viewsuppressed`. Here the ACL names a group, and those rights are what membership in that group reports to `meta=userinfo` ([0016](0016-permissions-and-access-control.md) §2). The default deletion group is `site` configuration (`moderation.delete_group`, default `sysop`; `moderation.suppress_group`, default `suppress`), so an instance that wants a separate reviewers group can have one without a new mechanism.
- **Nothing here touches the log.** Hidden and deleted content stays in its records, keeps its IDs, its place in the Merkle tree and its inclusion proofs ([0006](0006-log-integrity-and-erasure.md)). Removing content from the log is `erase`, and an `erase` may follow a hide when hiding is not enough.
- **Foreign entities are not deleted here.** What upstream holds is upstream's to delete ([0002](0002-source-graphs-and-mass-ingest.md) §5). A local assertion *about* a foreign entity is a local record and can be hidden like any other; the entity `WDQ42` itself cannot be. Keyed entities ([0009](0009-keyed-entity-types-and-domain.md) §4) likewise: a Domain exists because its key is valid, so it has no deletion; its local records can be hidden.

**Files.** For files, removal has two axes: who acts (a tenant, or the instance operator across every tenant) and how final it is. Tenants **delete** (this section), **hide** (§5) and **erase**; the instance **reclaims** versions that have stayed deleted for a retention period; operators **take down** and **reinstate**, and **expunge**, which erases every reference to a hash and destroys its bytes.

### 2. Targets (extends 0016 §4)

*Changed by A2, A5, A7, A9.*

Four target kinds are added to the table of [0016](0016-permissions-and-access-control.md) §4, and two existing kinds gain a use:

| Target kind | Key | Restricts | Added for |
|---|---|---|---|
| `record` | `acl:record:{partition}:{offset}` | `read` of the named **parts** of one record. The ACL's content carries `parts`, any subset of the payload type's parts (`content`, `comment`, `attestation`, and `text` for threads, [0019](0019-discussions.md) §4); an empty list means the whole record | Hiding and suppressing revisions and log events |
| `actor` | `acl:actor:{key}` | `read` of the actor's name and other attributes, everywhere they appear; the actor key and IRI, which never contain a name ([0007](0007-actor-identity.md) §2), stay visible | Hiding a username |
| `page` (existing) | `acl:page:{page id}` | Now also `read` (deletion) and, for a **reserved** page ID, `createpage` (below) | Deletion; create-protection |
| `entity` (existing) | `acl:entity:{id}` | Now also `read` (deletion of a local entity) | Entity deletion |
| `statement` | `acl:statement:{guid}`, the GUID as stored in the log; the UUID part identifies the statement across the canonical-ID rewrite of [0018](0018-tenants.md) §7 | `edit`: changing, overriding or removing that one statement, its qualifiers and references. `read`: the statement is left out of the resolved view, RDF, search and diffs for viewers outside the group | Statement-level protection and hiding (A2) |
| `property` | `acl:property:{id}` | `edit`: writing any snak whose property is *P*, as main snak, qualifier or reference, on any entity. `read`: every snak with predicate *P* is left out for viewers outside the group | Property-level protection and hiding; Scatterbase's property-specific rule (A2) |
| `blob` | `acl:blob:{takedown id}`, at instance scope | `read` and `upload`, to no group, on every file version in every tenant whose SHA-256 or SHA-1 equals the hash in the record | Takedowns ([0039](0039-files-and-media.md) §10) |
| `tenant` | `acl:tenant:{slug}`, in the tenant `config` | `read`: the tenant's visibility; a tenant with a `read` restriction is private. Encloses everything in the tenant | Private tenants ([0056](0056-security-model.md) §3) |
| `set` | `acl:set:{id}`, the ID from the page-ID sequence as a reserved title takes one; in the tenant `log` | `read`, `edit`, `move` on its members: pages with their subpages, entities and threads, listed by ID in the record with the set's name; membership changes need `protect` | Groupings of pages ([0056](0056-security-model.md) §3) |

**Create-protection reserves a page ID.** A title that does not exist has no page ID, and a key must never be content ([0006](0006-log-integrity-and-erasure.md) §3), so the protecting record takes a page ID from the sequence ([0015](0015-record-format-and-partition-registry.md) §2) and carries the namespace and title in its content part. The title index maps the title to the reserved ID; a `create` by an actor the ACL admits takes that ID, and the page then exists. This is the pattern [0019](0019-discussions.md) §2 uses for talk-page IDs.

**Containment** is [0016](0016-permissions-and-access-control.md) §4's and [0019](0019-discussions.md) §12's, with the tenant at the root and sets beside namespaces ([0056](0056-security-model.md) §3): the tenant encloses everything in it, a namespace encloses its pages and entities, a set its members, a page its subpages, a talk page or board the threads whose home it is, not those listed on it ([0049](0049-boards.md) §7). So a `read` ACL on a namespace makes it private, and deleting a talk page deletes the threads homed there. **A subject page does not enclose its talk page**, in either direction: protecting an item leaves its talk page open, which is what protection is for, and deleting an item leaves its talk page unless the administrator deletes that too. The delete form offers to, as MediaWiki's does, and writes two records.

**Enclosure gains a second axis.** Containment (graph ⊃ namespace ⊃ entity ⊃ statement) is joined by **predicate**: a property ACL applies to every snak using that property, wherever it sits. Evaluation stays conjunctive: to edit statement *S* on entity *E*, an actor satisfies the ACLs on *S*, on *E*, on *E*'s namespace, on the graph, and on *S*'s property. The cost is one indexed lookup per property a write touches, cached in L0 ([0014](0014-caches-and-search.md) §2); on the read side the resolution projection asks `view.acl` for the entity's GUIDs and properties in one query, and the public form of the resolved view ([0014](0014-caches-and-search.md) §1) simply lacks what a read ACL hides. A `blob` target adds **content** as a third axis of enclosure: it encloses every file version whose bytes hash to the record's value, in every tenant ([0039](0039-files-and-media.md) §10).

**A read ACL on a mirrored statement is not an `override`.** [0002](0002-source-graphs-and-mass-ingest.md) §3's suppression is an editorial act, a local record public in history, and the resolved view says the statement was suppressed. A `read` ACL is moderation: the statement is hidden for privacy or legal reasons, only the group sees that it exists, and the source graph is untouched. Both are available; they answer different questions. Setting either target kind needs `protect` for `edit` and `deleterevision` for `read`, as for a page; they project as `protect/*` and as a new `delete/statement` event. This settles [0016](0016-permissions-and-access-control.md) Q1 and answers Scatterbase's property-specific rule.

### 3. Records (amends 0015 §3 and 0016 §4)

*Changed by A3.*

**`scatter:v0/acl` is a payload type, not a `config` kind.** [0016](0016-permissions-and-access-control.md) §4 made ACLs `config` records of kind `acl`. Graph ACLs are configuration and stay in the tenant `config` partition. Every other target is **moderation**, and its records go to the tenant `log` partition ([0011](0011-logs.md) §2), which is where 0011 §6.1 already expected protection to be written: it is "an action with no other record". The same payload type is used in both partitions; `docs/registry/graphs.toml` lists it for both. Edit filter and hit records share the `log` partition for the same reason ([0030](0030-edit-filters.md) §5).

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

*Changed by A7.*

**A deleted page or thread** ([0008](0008-namespaces-and-document-pages.md) §4, [0019](0019-discussions.md) §1) is a page with a `read` ACL naming the deletion group. The `delete` and `undelete` operations those ADRs gave page and thread records are **retired**; their log events (`delete/delete`, `delete/restore`) now come from the ACL, and everything they said about the effect stands:

- the page leaves the title index, so its title can be taken by a new page (`view.page.deleted` is derived from the ACL; the partial unique index of [0013](0013-postgres-storage.md) §5.4 already excludes deleted rows);
- it leaves search, backlinks, `list=allpages`, feeds and the resolved metadata graph for everyone outside the group;
- `action=parse`, `prop=revisions` and the page itself answer as MediaWiki does: the page is reported missing, with the deletion log entry shown in its place, and members of the group see the content behind a banner;
- a deleted thread leaves its talk page's listing and history; a deleted talk page or board takes with it the threads whose home it is, by enclosure. A thread only listed there is untouched: the listing goes dormant, hidden outside the deletion group, and returns on undelete ([0049](0049-boards.md) §7).

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

*Changed by A3, A4.*

**A patrol mark is a log event**, `patrol/patrol`, in the tenant `log` partition, keyed by the patrolled record's coordinates and carrying its revision ID, as MediaWiki's patrol log does. The projection sets `patrolled` on the activity row. **Unpatrol** is `patrol/unpatrol`, same key, same right; the projection sets `patrolled` false, and the newest of the two events for a record wins.

**What is patrollable:** every activity row of kind `edit` in the tenant's `local` and `pages` partitions, which includes posts and thread operations ([0019](0019-discussions.md) §7), and page creations. Mirror syncs are not: upstream's edits are upstream's to review. A **job** is patrolled as one row; marking the job marks every change it made, which is the grouping [0010](0010-site-ui.md) §7 already gives jobs.

**Autopatrol is a projection rule, not a record.** A change made by an actor who held `autopatrol` when the record was appended is patrolled from the start. The projection decides this from the membership records in `actors` as of the record's `appended_at`, so a rebuild reaches the same answer, and no record is written per bot edit. MediaWiki writes a `patrol/autopatrol` entry for each; Triplespace does not, and `list=logevents&letype=patrol` returns only manual marks. An edit filter's `unpatrol` action withholds autopatrol as a write lands ([0030](0030-edit-filters.md) §4). Whether a viewer sees the unpatrolled mark at all depends on the `patrol` right, as in MediaWiki.

**Where the mark shows:** the `!` marker on rows in recent changes, related changes, the watchlist and histories ([0020](0020-change-feeds.md) §1) for viewers with `patrol`; a **Mark as patrolled** action on the diff and on the row, and **Mark as unpatrolled** on a patrolled one; a `patrolled` filter on every feed, which is the `rcshow=patrolled|!patrolled` of `list=recentchanges` and `list=watchlist`. Marks are shown only within the recent-changes window (`site` setting `rc.max_age`, [0010](0010-site-ui.md) §7), as MediaWiki's are.

**Rights.** `patrol` marks; `autopatrol` exempts. Defaults are in §11.

### 7. Visibility of the moderation records themselves

Moderation is public in the way MediaWiki's logs are, with one exception:

- **Protection, deletion, revision-hiding and patrol events are visible to everyone.** That a page was deleted, and by whom and why, is public; the content is not. This is [0016](0016-permissions-and-access-control.md) §6's rule for erasures, applied uniformly.
- **A record whose group holds `viewsuppressed` is visible only to that group.** Its log event is `suppress/*`, and to everyone else the affected fields are simply hidden, with no event saying so. This is MediaWiki's private suppression log.
- **The `log` partition's export policy is internal** ([0011](0011-logs.md) §2), so none of these records enters a public dump; a full dump ([0013](0013-postgres-storage.md) §8) made by the instance for itself carries them, and it is the instance's to protect.
- **The downstream stream** ([0020](0020-change-feeds.md) §4) carries the public events, so a consumer learns that a revision's comment was hidden and can redact its copy, as [0011](0011-logs.md) §5 has this instance follow upstream. It never carries a `suppress/*` event or the content behind one.

### 8. API (extends 0012 §4 and §5; extends 0016 §7)

*Changed by A4, A9.*

**Action API**, additively under [0012](0012-api-requirements.md) §1, with MediaWiki's meaning:

| Module | Behaviour |
|---|---|
| `action=protect` | `protections=edit=autoconfirmed\|move=sysop`, `expiry`, `reason`; writes or retires the ACL. `read={group}` writes a confidential restriction ([0056](0056-security-model.md) §13); `list=protectedpages` takes `prtype=read` and `list=protectedsets` lists sets. `cascade=1` is refused with `ts-no-cascade`, since there is no cascading protection ([0016](0016-permissions-and-access-control.md) §4). On a title that does not exist, reserves a page ID (§2). On an entity page, restricts `edit` on the entity target |
| `action=delete`, `action=undelete` | Pages, threads and entity pages. `deletetalk=1` writes the second record of §2. `undelete` with `timestamps` or `revisions` performs the partial form of §3 |
| `action=revisiondelete` | `type=revision\|logging`, `ids`, `hide=content\|comment\|user`, `show=…`, `suppress=yes\|no\|nochange`, `reason`; writes or amends `record` ACLs. `type=archive` is accepted and treated as `revision`, since deleted revisions are not moved |
| `action=block` | Gains `hidename`, which writes the actor ACL beside the block |
| `action=patrol` | `revid` or `rcid`; writes the patrol event, or the unpatrol event with `unpatrol=1`. `list=recentchanges` and `list=watchlist` gain `rcshow=patrolled\|!patrolled\|autopatrolled` and `rcprop=patrolled` |
| `prop=info` | `inprop=protection` reports the page's or entity's edit and move ACLs as MediaWiki `protection` entries; `list=protectedtitles` lists reserved titles; `list=protectedpages` the rest |
| `prop=deletedrevisions`, `list=alldeletedrevisions`, `list=deletedrevs` | The hidden records, for members of the group only |
| `wbgetentities` | `missing` for a deleted local entity; `prop=revisions` on its page as for a deleted page |

**REST**, under `rest.php/triplespace/v0`: `GET`, `PUT` and `DELETE /acl/{kind}/{id}` ([0016](0016-permissions-and-access-control.md) §7) accept `record` and `actor` kinds and a `parts` list; `POST /activity/{partition}/{offset}/patrol` marks a row. The activity row ([0012](0012-api-requirements.md) §3) gains `patrolled`, and its `visibility` field already reports hidden parts.

Every response is redacted for its viewer ([0012](0012-api-requirements.md) §8); a `read` ACL is one more input to that redaction, and the privacy test of 0012 §8 gains a case per target kind: nothing behind a `read` ACL is returned to a viewer outside its group, from any route, cache or index.

### 9. Site UI (extends 0010 §2, §5 and §7)

*Changed by A6, A9.*

- **Overflow menu.** Pages and entities gain **Protect…** ([0016](0016-permissions-and-access-control.md) §7), whose dialog has a **Read** row with a group picker ([0056](0056-security-model.md) §13), and **Delete…**, the latter with the "also delete the talk page" option. Deleted pages show **Undelete…** to the group.
- **A deleted page or entity** shows, to everyone outside the group, the frame of [0010](0010-site-ui.md) §2 with the deletion log entry in place of the content, and a link to the log. To the group it shows the content under a banner naming who deleted it and why, with Undelete.
- **History rows** gain RevisionDelete checkboxes for holders of `deleterevision`, and a suppress option for holders of `suppressrevision`, opening a dialog with the three parts and a reason. A hidden part is drawn as [0010](0010-site-ui.md) §5.2 already draws hidden actors: in place, labelled, never silently absent.
- **Patrol.** The `!` marker, the Mark as patrolled action, and the `patrolled` filter, for holders of `patrol`.
- **Special pages.** `Special:Log/protect`, `/delete`, `/patrol` and, for the group, `/suppress`; `Special:ProtectedPages`, which gains a **Read** column and a **Sets** tab and lists a confidential restriction only to viewers who may read its target ([0056](0056-security-model.md) §5, §13), `Special:ProtectedTitles`, `Special:Undelete`, `Special:DeletedContributions`, and `Special:BlockList` gains the hidden-name column, all where MediaWiki users expect them.
- **Nuke.** `Special:Nuke` applies these actions in bulk to one account's, or one tag's, contributions: one job that deletes what it created, reverts or rolls back what it changed and hides its posts, patrolled as one row and undone by **Revert this job…**.

### 10. Storage (amends 0013 §5)

- **`view.acl`** ([0016](0016-permissions-and-access-control.md) §9) holds moderation ACLs beside graph ACLs: `(target_kind, target_id, permission, parts smallint, "group", expires, partition, "offset")`, indexed by target. The evaluation path reads it for the target and its enclosures, cached in L0 ([0014](0014-caches-and-search.md) §2); the common case, no row, is one indexed miss.
- **Denormalized flags** keep lists from joining: `view.page.deleted` (already present) and a new `view.entity.deleted boolean NOT NULL DEFAULT false` are derived from `read` ACLs on the target; `view.activity.visibility` and `view.post.hidden` are derived from `record` ACLs together with the erased-parts bitmask of [0013](0013-postgres-storage.md) §2; `view.actor.status = 'hidden'` from an `actor` ACL.
- **`view.activity.patrolled boolean`** with a partial index `WHERE NOT patrolled` for the recent-changes filter, set by the patrol projection and the autopatrol rule of §6.
- **Projection order** ([0013](0013-postgres-storage.md) §7): the ACL projection runs in step 1 with the permission tables, since resolution, terms, search and activity all read it; it is synchronous for interactive writes, so a deletion is invisible the moment it commits.
- **Reserved page IDs** are rows in `view.page` with `latest_offset` null and a `reserved boolean`; the title index includes them so that `create` finds the ID.

### 11. Permissions (extends 0016 §2)

*Changed by A6.*

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
| `nuke` | `Special:Nuke` (§9); each write it makes also needs its own right from this table ([0047](0047-special-pages.md) §12) | `sysop` |

`autopatrolled` is a group administrators assign with `userrights`, as on Wikimedia projects. `docs/registry/groups.toml` is updated with the two permissions and the group.

**Ownership rules** ([0016](0016-permissions-and-access-control.md) §2) are unchanged; none applies here. A user may not delete or hide their own contributions without the right.

### 12. Scatterbase

Scatterbase inherits §1–3 and §5 whole. Read ACLs suit it better than deletion would: a signed claim's content stays in the log and verifiable, and only who may read it changes, which is the answer its decision record wanted for `view_deleted` and for restricting "individual claims". The `record` target with `parts` is the claim-level restriction it reserved. Patrolling is Triplespace's and is not shared.

### 13. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed. The table this section first gave is in A1.

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

- **Q1.** ~~**`statement` and `property` ACL targets** ([0016](0016-permissions-and-access-control.md) Q1) are unchanged: hiding one statement of a large local entity is not possible without them, only the whole record that added it.~~ *Settled by A2: both targets exist, with predicate as a second axis of enclosure.*
- **Q2. Deleting a heavily referenced local entity.** Dangling values are Wikidata's behaviour, but an instance may want a warning, or a redirect, when an entity with many referrers is deleted.
- **Q3.** ~~**Abuse filters and pre-save checks**, still neither permissions nor ACLs ([0016](0016-permissions-and-access-control.md) Q5).~~ *Settled by [0030](0030-edit-filters.md): edit filters, a third mechanism beside permissions and ACLs, evaluated in the appending transaction.*
- **Q4. Reviewing threads.** Whether a post should be patrollable separately from the thread operation that carried it, and whether `thread-status` marks ([0019](0019-discussions.md) §6) should count as review.
- **Q5. `autopatrolled` thresholds**, if an instance wants the group granted automatically as `autoconfirmed` is.
- **Q6.** ~~**Unpatrol**, which MediaWiki lacks and some wikis want.~~ *Settled by A5: a `patrol/unpatrol` log event in the tenant `log`, keyed like `patrol/patrol` (§6) and written by holders of `patrol`; the projection sets `patrolled` false, and the newest of the two events for a record wins. `action=patrol` gains `unpatrol=1`; the row's action is "Mark as unpatrolled". [0030](0030-edit-filters.md) §4's `unpatrol` filter action, which withholds autopatrol as a write lands, is unchanged.*
- **Q7. Whether deletion should expire.** The record allows it; MediaWiki has no temporary deletion, and none is offered here.
- **Q8.** ~~**Blocking and hiding across tenants** ([0018](0018-tenants.md) Q3): whether an `actor` ACL written by the primary tenant should apply to every tenant.~~ *Settled by [0028](0028-tenancy-policy.md) §4: an actor ACL on a farm account applies everywhere; a tenant's applies to its own account.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0001](0001-revision-metadata-rdf.md) §4 | §5 | extends | 0001 A9 |
| [0004](0004-identity-clusters-and-equivalence.md) §4 | §4 | extends | 0004 A5 |
| [0005](0005-crate-organization.md) §2 | §13 | amends | 0005 A19 |
| [0005](0005-crate-organization.md) §2 | §2 | extends | 0005 A25 |
| [0007](0007-actor-identity.md) §4 | §5 | extends | 0007 A7 |
| [0008](0008-namespaces-and-document-pages.md) §4 | §4 | amends | 0008 A4 |
| [0008](0008-namespaces-and-document-pages.md) Q6 | §1–4 | settles | 0008 Q6 |
| [0010](0010-site-ui.md) §2, §5.2, §7 | §9 | extends | 0010 A13 |
| [0010](0010-site-ui.md) Q2 | §6 | settles | 0010 Q2 |
| [0011](0011-logs.md) §3, §6.1 | §3, §5–7 | extends | 0011 A7 |
| [0011](0011-logs.md) Q4 | §3 | settles | 0011 Q4 |
| [0011](0011-logs.md) Q7 | §6 | settles | 0011 Q7 |
| [0012](0012-api-requirements.md) §3, §4, §5, §8 | §8 | extends | 0012 A12 |
| [0013](0013-postgres-storage.md) §5, §5.2, §5.6 | §10 | amends | 0013 A9 |
| [0013](0013-postgres-storage.md) §4, §5.4, §5.5, §7, §8 | §10 | extends | 0013 A9 |
| [0014](0014-caches-and-search.md) §1, §5 | §2, §5, §10 | amends | 0014 A3 |
| [0014](0014-caches-and-search.md) §10 | §2, §5, §10 | extends | 0014 A3 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §3 | amends | 0015 A6 |
| [0016](0016-permissions-and-access-control.md) §4, §8 | §1–3, §11 | amends | 0016 A6 |
| [0016](0016-permissions-and-access-control.md) §2 | §1–3, §11 | extends | 0016 A6 |
| [0016](0016-permissions-and-access-control.md) Q1 | §2 | settles | 0016 Q1 |
| [0016](0016-permissions-and-access-control.md) Q4 | §6, §11 | settles | 0016 Q4 |
| [0019](0019-discussions.md) §1, §7, §12 | §4, §6 | amends | 0019 A4 |
| [0019](0019-discussions.md) §9 | §4, §6 | extends | 0019 A4 |
| [0020](0020-change-feeds.md) §1 | §6 | extends | 0020 A3 |
| [0020](0020-change-feeds.md) Q2 | §6 | settles | 0020 Q2 |

## References

- [Manual:Protection](https://www.mediawiki.org/wiki/Manual:Protection), [Manual:Page_restrictions table](https://www.mediawiki.org/wiki/Manual:Page_restrictions_table), [Manual:Protected_titles table](https://www.mediawiki.org/wiki/Manual:Protected_titles_table)
- [Manual:Archive table](https://www.mediawiki.org/wiki/Manual:Archive_table), [Help:Deletion and undeletion](https://www.mediawiki.org/wiki/Help:Deletion_and_undeletion)
- [Manual:RevisionDelete](https://www.mediawiki.org/wiki/Manual:RevisionDelete) (the `rev_deleted` bits) and [Help:RevisionDelete](https://www.mediawiki.org/wiki/Help:RevisionDelete)
- [Manual:Patrolled edits](https://www.mediawiki.org/wiki/Manual:Patrolled_edits), [Help:Patrolled edits](https://www.mediawiki.org/wiki/Help:Patrolled_edits)
- [API:Protect](https://www.mediawiki.org/wiki/API:Protect), [API:Delete](https://www.mediawiki.org/wiki/API:Delete), [API:Undelete](https://www.mediawiki.org/wiki/API:Undelete), [API:Revisiondelete](https://www.mediawiki.org/wiki/API:Revisiondelete), [API:Patrol](https://www.mediawiki.org/wiki/API:Patrol)
- Scatterbase decision record (`view_deleted`; URI ACLs over "individual claims")

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §13
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A19).

Replaced text (§13):

> | Crate | Change |
> |---|---|
> | `scatter-actors` | ACL evaluation for `read`, with `parts` on `record` targets and the `actor` target; the autopatrol rule as a pure function over memberships as of a time |
> | `scatter-log` | The `scatter:v0/acl` payload type, replacing the `acl` config kind; registered for the `config` and `log` partitions |
> | `scatter-pages`, `scatter-threads` | The `delete` and `undelete` operations are removed from `scatter:v0/page` and `scatter:v0/thread` |
> | `scatter-mwlog` | `patrol` and `suppress` log types; `protect/*` and `delete/*` parameters as typed in §3 |
> | `triplespace-projections` | The ACL projection; derived `deleted`, `visibility`, `hidden` and `status` flags; the patrol projection; cluster re-evaluation on entity deletion and undeletion |
> | `triplespace-titles` | Reserved page IDs in the title index |
> | `triplespace-api-action`, `triplespace-api-rest` | The modules and routes of §8 |
> | `triplespace-cache`, `triplespace-search` | Hiding and deletion on the purge path of 0014 §5, as erasure already is |
>
> No crate is added.

### A2. Statement and property targets

- **Date:** 2026-09-27
- **Source:** Direct: James, design discussion of 2026-09-27 (resolvers and statement ACLs)
- **Change:** extends §2
- **Summary:** The two kinds [0016](0016-permissions-and-access-control.md) §4 reserved are added, and inherit everything above.
  
  | Target kind | Key | Restricts |
  |---|---|---|
  | `statement` | `acl:statement:{guid}`, the GUID as stored in the log; the UUID part identifies the statement across the canonical-ID rewrite of [0018](0018-tenants.md) §7 | `edit`: changing, overriding or removing that one statement, its qualifiers and references. `read`: the statement is left out of the resolved view, RDF, search and diffs for viewers outside the group |
  | `property` | `acl:property:{id}` | `edit`: writing any snak whose property is *P*, as main snak, qualifier or reference, on any entity. `read`: every snak with predicate *P* is left out for viewers outside the group |
  
  **Enclosure gains a second axis.** Containment (graph ⊃ namespace ⊃ entity ⊃ statement) is joined by **predicate**: a property ACL applies to every snak using that property, wherever it sits. Evaluation stays conjunctive: to edit statement *S* on entity *E*, an actor satisfies the ACLs on *S*, on *E*, on *E*'s namespace, on the graph, and on *S*'s property. The cost is one indexed lookup per property a write touches, cached in L0 ([0014](0014-caches-and-search.md) §2); on the read side the resolution projection asks `view.acl` for the entity's GUIDs and properties in one query, and the public form of the resolved view ([0014](0014-caches-and-search.md) §1) simply lacks what a read ACL hides.
  
  **A read ACL on a mirrored statement is not an `override`.** [0002](0002-source-graphs-and-mass-ingest.md) §3's suppression is an editorial act, a local record public in history, and the resolved view says the statement was suppressed. A `read` ACL is moderation: the statement is hidden for privacy or legal reasons, only the group sees that it exists, and the source graph is untouched. Both are available; they answer different questions. Setting either target kind needs `protect` for `edit` and `deleterevision` for `read`, as for a page; they project as `protect/*` and as a new `delete/statement` event. This settles 0016's open question and answers Scatterbase's property-specific rule. This settled Q1.

### A3. Edit filters

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §4–5
- **Change:** extends §3, §6
- **Summary:** Filter and hit records live in the `log` partition beside moderation ACLs, for the same reason; a filter's `unpatrol` action withholds autopatrol as a write lands.

### A4. Decisions of 2026-09-27 (evening)

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** amends §6, §8
- **Summary:** Unpatrol exists (decision 15): a `patrol/unpatrol` log event in the tenant `log`, keyed like `patrol/patrol` and written by holders of `patrol`; the projection sets `patrolled` false, and the newest of the two events for a record wins. `action=patrol` gains `unpatrol=1`; the row's action is "Mark as unpatrolled". §6 had been given a struck sentence with a note. This settled Q5.

Replaced text (§6):

> There is no unpatrol, as there is none in MediaWiki core.

### A5. Files

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §8–10
- **Change:** extends §1, §2
- **Summary:** By section:
  - §1: For files, removal has two axes: who acts (a tenant, or the instance operator across every tenant) and how final it is. Tenants **delete** (this section), **hide** (§5) and **erase**; the instance **reclaims** versions that have stayed deleted for a retention period; operators **take down** and **reinstate**, and **expunge**, which erases every reference to a hash and destroys its bytes.
  - §2: A `blob` target at instance scope, keyed `acl:blob:{takedown id}`, encloses every file version, in every tenant, whose SHA-256 or SHA-1 equals the hash in the record. Content becomes an axis of enclosure beside containment and predicate. A takedown restricts `read` and `upload` on it to no group.

### A6. Nuke

- **Date:** 2026-10-01
- **Source:** [0047](0047-special-pages.md) §7, §12
- **Change:** extends §9, §11
- **Summary:** By section:
  - §9: `Special:Nuke` applies these actions in bulk to one account's, or one tag's, contributions: one job that deletes what it created, reverts or rolls back what it changed and hides its posts, patrolled as one row and undone by **Revert this job…**.
  - §11: `nuke` (default `sysop`) runs `Special:Nuke`; each write it makes also needs its own right from this table.

### A7. Boards

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §7
- **Change:** amends §2, §4
- **Summary:** By section:
  - §2: A talk page or board encloses the threads whose home it is, not the threads listed on it.
  - §4: A deleted talk page or board takes with it the threads whose home it is. A thread only listed there is untouched; the listing goes dormant, hidden outside the deletion group, and returns on undelete.

Replaced text (§2):

> **Enclosure is unchanged** from [0016](0016-permissions-and-access-control.md) §4 and [0019](0019-discussions.md) §12: a namespace encloses its pages and entities, a page its subpages, a talk page its threads. So a `read` ACL on a namespace makes it private, and deleting a talk page deletes its threads.

Replaced text (§4):

> - a deleted thread leaves its talk page's listing and history; a deleted talk page takes its threads with it by enclosure.

### A8. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–13
- **Summary:** A1–A7 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A2 and A5–A7 were blockquotes, A4 a struck sentence with a note, and A3 was recorded only in 0030. The file before conversion is commit `0b26a3a`.

### A9. Confidential restrictions, the tenant and set targets

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §2–5, §13
- **Change:** amends §1, §2; extends §2, §8, §9
- **Summary:** Protection may restrict `read`. Such a restriction is of the confidential kind: the target stays live for its group and is absent, not marked deleted, for everyone else, and it is set with `protect` by a member of the group. The `read` ACLs this ADR defines are the moderation kind and are unchanged. Two targets join the table: `tenant`, the root of enclosure, and `set`, a grouping by ID that only `protect` changes. `action=protect` takes `read=`; `Special:ProtectedPages` gains a Read column and a Sets tab.

Replaced text (§1):

> | **Protect** | A page, thread, talk page, entity or namespace | `edit`, `move`, or both; `createpage` for a title that does not exist yet (§2) | `autoconfirmed` (semi-protection) or `sysop` (full), or any group | Page protection; `protected_titles` |

Replaced text (§2):

> **Containment is unchanged** from [0016](0016-permissions-and-access-control.md) §4 and [0019](0019-discussions.md) §12: a namespace encloses its pages and entities, a page its subpages,
