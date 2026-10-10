# 0023. Protection, deletion, hiding and patrolling

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A13)
- **Author:** James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0006](0006-log-integrity-and-erasure.md), [0009](0009-keyed-entity-types-and-domain.md), [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0021](0021-notifications.md), [0022](0022-federation.md), [0028](0028-tenancy-policy.md), [0030](0030-edit-filters.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

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

*Current text: [09](../architecture/09-security-and-moderation.md) §4.1, §4.4, §5.2, §6.1.*

### 2. Targets (extends 0016 §4)

*Changed by A2, A5, A7, A9.*

*Current text: [09](../architecture/09-security-and-moderation.md) §4.2, §4.4, §4.5, §4.7, §4.8.*

### 3. Records (amends 0015 §3 and 0016 §4)

*Changed by A3.*

*Current text: [09](../architecture/09-security-and-moderation.md) §4.1, §4.3; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 4. What deletion does (amends 0008 §4 and 0019 §1; amends 0004 §4)

*Changed by A7, A13.*

*Current text: [09](../architecture/09-security-and-moderation.md) §6.2.*

### 5. What hiding does (extends 0001 §4, 0006 Context, 0015 §1; amends 0007 §4)

*Current text: [09](../architecture/09-security-and-moderation.md) §6.3.*

### 6. Patrolling

*Changed by A3, A4, A12.*

*Current text: [09](../architecture/09-security-and-moderation.md) §6.4.*

### 7. Visibility of the moderation records themselves

*Current text: [09](../architecture/09-security-and-moderation.md) §6.5.*

### 8. API (extends 0012 §4 and §5; extends 0016 §7)

*Changed by A4, A9.*

*Current text: [18](../architecture/18-api.md) §2.3, §3.1, §3.2.*

### 9. Site UI (extends 0010 §2, §5 and §7)

*Changed by A6, A9.*

*Current text: [19](../architecture/19-site-ui.md) §3.4, §3.9, §6.3.*

### 10. Storage (amends 0013 §5)

*Changed by A10.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.5, §5, §6.1, §12.2.*

### 11. Permissions (extends 0016 §2)

*Changed by A6.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §3.1.*

### 12. Scatterbase

*Current text: [22](../architecture/22-crates-and-stack.md) §1.3.*

### 13. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **Six administrative actions are one record type**, evaluated by code that already exists. Protect, delete, hide, suppress, hide a name and every undo of them are ACL records; only patrol is a second shape, and it is a plain log event.
- **Nothing is moved or copied.** Deletion has no archive table, undeletion no merge, and hiding no redacted copy. Every deleted or hidden thing keeps its IDs, its proofs and its place in history, and the log stays append-only.
- **Two earlier ADRs lose an operation.** 0008 §4's and 0019 §1's `delete` and `undelete` are retired before any code exists for them. Their log events and effects are unchanged.
- **Local entities can be deleted**, which closes a gap Wikibase clients would have hit at once, and the cluster rule of 0004 gets one more membership change to handle.
- **Suppression stays out of public dumps** because moderation records live in the internal `log` partition, at the cost of splitting ACL records across two partitions by target kind.
- **Reads consult ACLs.** Every response path already redacts per viewer; it now also asks whether the target is behind a `read` ACL. The denormalized flags keep the cost out of list queries, and the common case is one cached miss.
- ~~**Patrolling is MediaWiki-shaped without MediaWiki's volume.** Autopatrol as a projection rule spares the log a record per bot edit, and `list=logevents&letype=patrol` differs from MediaWiki's only by omitting `autopatrol` entries.~~ *Autopatrol is decided by the write path and written into the record's attestation part, not derived by the projection; the log still gets no record per bot edit and the patrol log still omits `autopatrol` entries (A12).*
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

### A10. `view.acl` is as 0013 §5.4 defines it

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §10
- **Summary:** `view.acl` is the table [0013](0013-postgres-storage.md) §5.4 defines (0013 A27): `(target, restrictions jsonb, read_kind, extra jsonb, "offset")`, primary key `(target)`, one row per target holding every permission it restricts. §10's own column list, one row per (target, permission), is replaced by a reference to that definition; the evaluation path, caching and the common-case miss are unchanged. [0056](0056-security-model.md) §14's `kind` is that table's `read_kind` (0056's log). (PENDING A4)

Replaced text (§10):

> - **`view.acl`** ([0016](0016-permissions-and-access-control.md) §9) holds moderation ACLs beside graph ACLs: `(target_kind, target_id, permission, parts smallint, "group", expires, partition, "offset")`, indexed by target. The evaluation path reads it for the target and its enclosures, cached in L0 ([0014](0014-caches-and-search.md) §2); the common case, no row, is one indexed miss.

### A11. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§13
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A12. Autopatrol is decided by the write path and written into the attestation part

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §6
- **Change:** amends §6
- **Summary:** Autopatrol is a decision the write path makes from the state of another partition, so it is written down: the appending transaction evaluates the actor's effective permissions, which it has already resolved for the write itself, and writes the decision as an `autopatrol` field of the record's attestation map; the activity projection reads the field and sets `patrolled` from it, and never reconstructs the memberships in `actors` as of the record's `appended_at`, since a rebuild replays partitions in an order that need not interleave them as they were written and a vanish may have erased the memberships it would need. No record is written per bot edit, and an edit filter's `unpatrol` action withholds the field as a write lands. (REVIEW G13)

Replaced text ([09](../architecture/09-security-and-moderation.md) §6.4, as it stood):

> **Autopatrol is a projection rule, not a record.** A change made by an actor who held `autopatrol` when the record was appended is patrolled from the start. The projection decides this from the membership records in `actors` as of the record's `appended_at`, so a rebuild reaches the same answer, and no record is written per bot edit.

### A13. Visibility epochs, set generations and stored enclosures

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §4
- **Summary:** Public cache entries for tenant keys carry `-{epoch}` and shared keys carry no epoch; only `namespace` and `tenant` `read` ACLs bump the tenant's visibility epoch, and a `set` change bumps the generation of the members it touched; a stored `read_groups` becomes `read_enclosures`, a reference to the enclosures evaluated at serve time from L0, so a restriction written or retired after a row was stored takes effect without rewriting it; the privacy test "an entry cached under the empty set before a namespace restriction is not served after it" is required. The ledger's verb is amends; §4's chapter text ([09](../architecture/09-security-and-moderation.md) §6.2) is unchanged, since what deletion does is unchanged and the cache and index mechanics it relies on are [0014](0014-caches-and-search.md) §4, §5 and §7's and [0056](0056-security-model.md) §14's, logged there (0014 A26). (REVIEW G38)
