# 0016. Permissions and access control

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-09 (A32)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

## Context

[0012](0012-api-requirements.md) §8 names the rights Triplespace needs and reuses MediaWiki's where they exist, but leaves open which groups hold them. Seven ADRs carry a permissions question that depends on the answer: who may run `retain` and `convert` or set the default retention policy ([0002](0002-source-graphs-and-mass-ingest.md)); who may assert `same-as` and `different-from` or resolve conflicts ([0004](0004-identity-clusters-and-equivalence.md)); who may rename, hide or vanish accounts and remove links ([0007](0007-actor-identity.md)); who may create, move, delete and protect pages ([0008](0008-namespaces-and-document-pages.md)); who may revert jobs, see hidden usernames, erase, or retain from the history page ([0010](0010-site-ui.md)); who may protect, block and change rights ([0011](0011-logs.md)); and how erasure reasons are shown ([0006](0006-log-integrity-and-erasure.md), 0010, 0011).

Two models are already in play.

**MediaWiki's.** Users belong to groups; groups hold named rights; a user's rights are the union over their groups. `*` is everyone, `user` every registered account, and `sysop`, `bureaucrat` and `suppress` hold the administrative rights. Page protection adds a restriction on top: an action on a protected page also requires membership in the protecting group, until an expiry. Blocks take rights away from one account. `$wgRevokePermissions` can subtract rights from a group, but Wikimedia does not use it. Clients read all of this from `meta=siteinfo&siprop=usergroups`, `meta=userinfo&uiprop=rights|groups`, `list=users&usprop=groups|rights`, and change it with `action=userrights`, `action=protect` and `action=block` ([mediawiki-compat.md](../api/mediawiki-compat.md) §5). Wikibase adds `item-term`, `property-term`, `item-merge`, `item-redirect` and `property-create`.

**Scatterbase's.** Its decision record says: "The early permission set was view, edit, delete, view_deleted, and block_user. owner has all permissions and universe covers everyone. The API should create users, groups, memberships, and permissions, and set URI ACLs that can govern blobs and views and potentially individual claims. Authentication is pluggable, with a simple single user fallback. Exact matching, inheritance, deny precedence, a property specific rule, and whether ACL changes are themselves signed claims remain open."

The two agree on the shape: users, groups, memberships, named permissions, and restrictions attached to things by URI. This ADR adopts Scatterbase's vocabulary for the parts MediaWiki has no name for, and MediaWiki's names for everything MediaWiki clients will see, so that one model serves both products and Scatterbase can inherit it.

## Decision

### 1. Vocabulary

*Changed by A23.*

*Current text: [09](../architecture/09-security-and-moderation.md) §1.1.*

### 2. Permissions

*Changed by A2, A3, A4, A5, A6, A7, A8, A10, A13, A14, A15, A16, A19, A20, A21, A24, A25, A27, A32.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.1, §2.2, §2.3.*

### 3. Groups, memberships and blocks

*Changed by A5, A7, A8, A9, A11, A12, A15, A22, A30, A31.*

*Current text: [09](../architecture/09-security-and-moderation.md) §3.1, §3.2, §3.3, §3.4, §3.5, §3.6.*

### 4. ACLs (amends 0005 §4.1)

*Changed by A2, A6, A17, A21, A26.*

*Current text: [09](../architecture/09-security-and-moderation.md) §4.1, §4.2, §4.3, §4.4, §4.5, §4.6.*

### 5. What each earlier ADR's question resolves to

*Changed by A27, A32.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.4, §6.*

### 6. Erasure visibility (settles 0006, 0010 and 0011)

*Changed by A13, A14.*

*Current text: [09](../architecture/09-security-and-moderation.md) §6.6.*

### 7. API and UI

*Changed by A21, A28.*

*Current text: [18](../architecture/18-api.md) §1.5, §2.2, §2.3, §3.2.*

### 8. Scatterbase

*Changed by A6.*

*Current text: [22](../architecture/22-crates-and-stack.md) §1.3.*

### 9. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **Every earlier permissions question has an answer**, and each is one row in §2 or §5 rather than a new mechanism.
- **MediaWiki clients and users see MediaWiki.** The same groups, rights, modules and special pages, with `universe` and `owner` as the only names they have not seen before.
- **One negative rule.** Blocks are the only way to take a permission away from someone who holds it through a group. This is simpler than MediaWiki, which also has `$wgRevokePermissions`, and it is the answer to Scatterbase's deny question.
- **Permissions have history.** Groups and ACLs are `config` records, memberships and blocks are `actors` records; all are signed, ordered and rebuildable, and each projects as the log event MediaWiki would write.
- **A vanished account loses its memberships**, because they are keyed to it and erased with it. An `owner` cannot vanish while sole member.
- **Graph ACLs replace the *writers* field**, so [0005](0005-crate-organization.md) §4.1's registry entry has one field fewer and one more record kind beside it.
- **Scatterbase gets a permission system it did not have to design**, at the cost of adopting MediaWiki's right names for the five it had.

## Open questions

- **Q1.** ~~**`statement` and `property` ACL targets.** Whether a restriction on one statement, or on every statement of one property (Scatterbase's property-specific rule), is worth the evaluation cost on every local write.~~ *Settled by [0023](0023-moderation.md) §2: both exist; the cost is one indexed lookup per property touched.*
- **Q2. `autoconfirmed` thresholds** and whether they should follow Wikimedia's defaults. [0030](0030-edit-filters.md) asks the same of autopromotion.
- **Q3.** ~~**Rate limits and throttles** (`$wgRateLimits`), which are operational state rather than permissions, and where they are configured.~~ *Settled by [0024](0024-subsidiary-accounts.md) §5: `site` configuration by action class and group, counted in Valkey.*
- **Q4.** ~~**Patrolling and `autopatrol`**, which wait on [0010](0010-site-ui.md)'s patrolling decision.~~ *Settled by [0023](0023-moderation.md) §6 and §11.*
- **Q5.** ~~**Abuse filters** and other pre-save checks, which are neither permissions nor ACLs.~~ *Settled by [0030](0030-edit-filters.md): edit filters, evaluated in the appending transaction after permissions and ACLs.*
- **Q6.** ~~**Whether `owner` should be a single account rather than a group** on instances that want a legal owner of record.~~ *Settled by A11: `owner` stays a group with at least one member, which key rotation and continuity need. An instance that wants a named legal owner sets `instance.owner_of_record` in `site` configuration to one of its members; it is shown on `Special:Version` and in `siprop=triplespace` and grants nothing.*
- **Q7.** ~~**Group membership privacy.** MediaWiki exposes group membership publicly; whether an instance may hide `suppress` membership, as Wikimedia hides some, would need a private group flag.~~ *Settled by A11: no. Every membership is public, as `list=users&usprop=groups` and the `rights/rights` event ([0011](0011-logs.md) §6.1) promise; an instance that wants a discreet suppression team keeps it small.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) Q4 | §5 | settles | 0002 Q4 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q5 | §2 | settles | 0002 Q5 |
| [0004](0004-identity-clusters-and-equivalence.md) Q6 | §5 | settles | 0004 Q6 |
| [0005](0005-crate-organization.md) §2, §4.1 | §9 | amends | 0005 A11 |
| [0006](0006-log-integrity-and-erasure.md) §7 | §2, §6 | amends | 0006 A4 |
| [0006](0006-log-integrity-and-erasure.md) Q5 | §6 | settles | 0006 Q5 |
| [0007](0007-actor-identity.md) Q6 | §5 | settles | 0007 Q6 |
| [0008](0008-namespaces-and-document-pages.md) Q6 | §5 | settles | 0008 Q6 |
| [0010](0010-site-ui.md) §5.2 | §6–7 | amends | 0010 A5 |
| [0010](0010-site-ui.md) §2, §12 | §6–7 | extends | 0010 A5 |
| [0010](0010-site-ui.md) Q5 | §6 | settles | 0010 Q5 |
| [0010](0010-site-ui.md) Q8 | §5 | settles | 0010 Q8 |
| [0011](0011-logs.md) §6.1 | §3–4, §6 | amends | 0011 A4 |
| [0011](0011-logs.md) Q4 | §3–4 | settles | 0011 Q4 |
| [0011](0011-logs.md) Q5 | §6 | settles | 0011 Q5 |
| [0012](0012-api-requirements.md) §8 | §2, §6–7 | amends | 0012 A5 |
| [0012](0012-api-requirements.md) §3, §4, §5 | §2, §6–7 | extends | 0012 A5 |
| [0012](0012-api-requirements.md) Q8 | §2 | settles | 0012 Q8 |
| [0013](0013-postgres-storage.md) §4, §5.6, §7 | §3, §9 | extends | 0013 A3 |
| [0015](0015-record-format-and-partition-registry.md) Q4 | §6 | settles | 0015 Q4 |

## References

- [Manual:User rights](https://www.mediawiki.org/wiki/Manual:User_rights), [Manual:$wgGroupPermissions](https://www.mediawiki.org/wiki/Manual:$wgGroupPermissions), [Manual:$wgRevokePermissions](https://www.mediawiki.org/wiki/Manual:$wgRevokePermissions)
- [Manual:Protection](https://www.mediawiki.org/wiki/Manual:Protection), [Manual:Block and unblock](https://www.mediawiki.org/wiki/Manual:Block_and_unblock)
- [Wikibase user rights](https://www.mediawiki.org/wiki/Wikibase/Installation/Advanced_configuration#User_rights)
- [API:Userrights](https://www.mediawiki.org/wiki/API:Userrights), [API:Protect](https://www.mediawiki.org/wiki/API:Protect), [API:Block](https://www.mediawiki.org/wiki/API:Block)
- Scatterbase decision record (permissions, `owner`, `universe`, URI ACLs)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-26
- **Source:** [0005](0005-crate-organization.md) §2, revision of 2026-09-26
- **Change:** supersedes §9
- **Summary:** 0005 §2 became the one crate table CI checks, and carries every change this section listed (0005 A11).

Replaced text (§9):

> | Crate | Change |
> |---|---|
> | `scatter-actors` | The permission model: groups, memberships, blocks, ACL targets and enclosure, and the effective-permission and ACL evaluation of §3–4. Pure; shared with Scatterbase |
> | `scatter-log` | The `membership` and `block` payload types; the `group` and `acl` config kinds |
> | `triplespace-projections` | `view.group`, `view.membership`, `view.acl` and `view.block`, with `groups text[]` on `view.actor` |
> | `triplespace-accounts` | Resolving a session to an actor and its effective permissions; IP blocks in `private` |
> | `triplespace-api-action`, `triplespace-api-rest` | The modules and routes of §7 |
> | `triplespace-cli` | Creating `owner` (§3) |
>
> No crate is added.

### A2. Threads

- **Date:** 2026-09-27
- **Source:** [0019](0019-discussions.md) §12
- **Change:** extends §2; amends §4
- **Summary:** `ts-editpost`, and the thread operations under the existing rights. A talk page encloses the threads attached to it. The enclosure clause had been written into §4 in place.

Replaced text: not recorded. 0016 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A3. Watchlists

- **Date:** 2026-09-27
- **Source:** [0020](0020-change-feeds.md) §7
- **Change:** extends §2
- **Summary:** `viewmywatchlist` and `editmywatchlist`.

### A4. Preferences and contacts

- **Date:** 2026-09-27
- **Source:** [0021](0021-notifications.md) §9
- **Change:** extends §2
- **Summary:** `editmyoptions` and `viewmyprivateinfo`.

### A5. Federation

- **Date:** 2026-09-27
- **Source:** [0022](0022-federation.md) §8, §12
- **Change:** extends §2, §3
- **Summary:** The `federated` group, holding no permissions; `ts-config` and `protect` for federation settings and inbound replies. §3 had been given the group in place.

### A6. Moderation

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §1–3, §11
- **Change:** amends §4, §8; extends §2
- **Summary:** ACLs restrict `read` as well as write permissions, which is what deletion, revision-hiding, suppression and username-hiding are. Two target kinds are added, `record` (with a `parts` list) and `actor`. The record is the payload type `scatter:v0/acl`, not a config kind: graph ACLs are appended to `config`; every other target's ACL is a moderation record in the tenant `log` partition, so that suppressions stay out of public dumps. Writing a `read` ACL needs `delete`, `deleterevision`, `suppressrevision` or `hideuser` by target, as 0023 §11 lists. §11 adds `patrol`, `autopatrol` and the `autopatrolled` group. 0023's amendment of 2026-09-27 added the `statement` and `property` targets, whose rows §4 had been given in place. This settled Q1 and Q4.

Replaced text (§4):

> An ACL record is a `config` record of kind `acl` ([0015](0015-record-format-and-partition-registry.md) §3). Its content lists, for each permission it restricts, the group whose members may still perform it, and an expiry. Setting an ACL on a page, entity or namespace needs `protect`; on a graph, `ts-config`.

### A7. Subsidiaries

- **Date:** 2026-09-27
- **Source:** [0024](0024-subsidiary-accounts.md) §3, §5, §11
- **Change:** amends §2, §3; extends §2
- **Summary:** An operator's blocks reach their subsidiaries, and the ownership rules gain the operator's; a key's grants intersect the permissions; `createaccount`, `ts-transferaccount` and `noratelimit`. Rate limits are site configuration by action class, which settled Q3.

Replaced text (§3):

> A block is the only negative rule in the model: an actor's effective permissions are the union of their groups' permissions, minus what their active blocks remove. There is no revocation at the group level.

### A8. OAuth

- **Date:** 2026-09-27
- **Source:** [0025](0025-oauth-server.md) §3, §10
- **Change:** extends §2; amends §3
- **Summary:** The `mwoauth*` permissions; a `pending` subsidiary does not receive implicit `user`. §3 had been given the `pending` clause in place.

Replaced text: not recorded. 0016 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A9. Global groups and blocks

- **Date:** 2026-09-27
- **Source:** [0028](0028-tenancy-policy.md) §3–4
- **Change:** amends §3
- **Summary:** A tenant account's effective permissions include the global groups, and subtract the global blocks, of the farm account it is publicly linked to.

Replaced text: the block rule as A7 quotes it.

### A10. Edit filters

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §12
- **Change:** extends §2
- **Summary:** The `abusefilter-*` rights. This settled Q5.

### A11. Decisions of 2026-09-27 (evening)

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** amends §3
- **Summary:** `local:1` is created with a binding to the built-in `password` issuer and a password the CLI sets, in place of "a login token", since a primary account holds no token (decision 1, recorded as 0007 A3). `owner` stays a group with at least one member; `instance.owner_of_record` names a legal owner and grants nothing (decision 8). Every membership is public (decision 8). The password binding had been written into §3 in place. This settled Q6 and Q7.

Replaced text: not recorded. 0016 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A12. Adopted owners

- **Date:** 2026-09-28
- **Source:** [0035](0035-adopting-a-wikibase.md) §5
- **Change:** amends §3
- **Summary:** On a tenant that adopts an existing Wikibase, `instance create --adopt {source} --owner {user_id}` creates the owner under the source's user ID instead of `local:1`, with the same password binding, and sets the user-ID floor; "`local:1`" here reads as "the owner account".

Replaced text (§3):

> **`owner` is created with the instance.** `triplespace-cli` creates the first local user, `local:1`, as the sole member of `owner`, and gives it a binding to the built-in `password` issuer with a password the CLI sets ([0007](0007-actor-identity.md) §3, as amended 2026-09-27); the earlier wording, "a login token", is withdrawn, since a primary account holds no token ([0024](0024-subsidiary-accounts.md) §1). That is the single-user fallback Scatterbase asks for: an instance with no external issuer configured has exactly this one user, who logs in with a password and registers issuers, groups and the sync subsidiaries from there. Membership in `owner` is changed only by a member of `owner`, and the last member cannot be removed.

### A13. Files

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §10, §21
- **Change:** extends §2, §6
- **Summary:** The file rights and the operator rights `ts-takedown`, `ts-expunge` and `ts-viewtakedown`, evaluated on the primary tenant; erasures the instance makes follow §6's rule. §2 had been given three rows in place.

### A14. Instance rights

- **Date:** 2026-09-30
- **Source:** [0040](0040-instance-prerogatives.md) §7, §9
- **Change:** extends §2, §6
- **Summary:** By section:
  - §2: **Instance rights** authorize instance acts and are evaluated only on the primary tenant or through a global group at the farm base: `ts-keys`, `ts-config` at the farm base, `abusefilter-modify` for global filters, `userrights`, `block` and `renameuser` on farm accounts, `ts-takedown` and `ts-expunge`. A new right, `ts-viewoperator` (`owner`), reveals which operator carried out an instance act.
  - §6: Erasures the instance makes in a tenant's partitions (an operator's expunge, scheduled reclamation of long-deleted file versions) follow the same rule. They are attributed to the instance operator, and their authority is a record in the instance `log`.

### A15. The primary tenant

- **Date:** 2026-09-30
- **Source:** [0046](0046-primary-tenant.md) §1, §8
- **Change:** amends §3; extends §2
- **Summary:** `instance create` creates `owner` on the **primary tenant**, and that group is the instance's `owner`: the one that holds `ts-keys` and `ts-primary`. A tenant created later has an `owner` of its own, which is that wiki's owner and exercises no instance right there. `ts-primary`, `mwoauthmanageconsumer` and `ts-runjob` for instance jobs join the instance rights. §2 had been given the `ts-primary` row in place.

Replaced text: the `owner` paragraph as A12 quotes it.

### A16. Nuke

- **Date:** 2026-10-01
- **Source:** [0047](0047-special-pages.md) §12
- **Change:** extends §2
- **Summary:** `nuke`, MediaWiki's right from the Nuke extension, runs `Special:Nuke`; default `sysop`, and part of the `delete` grant.

### A17. Boards

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §7
- **Change:** amends §4
- **Summary:** A talk page or board encloses the threads whose **home** it is. A thread only listed on it is not enclosed by it; the page's `edit` ACL governs what may be listed there.

Replaced text (§4):

> a page encloses its subpages; a talk page encloses the threads attached to it ([0019](0019-discussions.md) §12).

### A18. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §2–9
- **Summary:** A1–A17 were folded into the Decision; the permissions later ADRs add are indexed in one table in §2, which points to the ADR that specifies each. The open questions were numbered. No decision changed. Before this, A6, A12, A13 (in part), A14, A15 (in part), A16 and A17 were blockquotes; A2, A5, A8, A11, A13 and A15 had also been written in place; the other entries were recorded only in other ADRs. The file before conversion is commit `0b26a3a`.

### A19. Redirect rights

- **Date:** 2026-10-01
- **Source:** [0051](0051-page-redirects.md) §8
- **Change:** extends §2
- **Summary:** `suppressredirect`, `delete-redirect`, `move-subpages`, `move-rootuserpages` and `move-categorypages`, with MediaWiki's default groups.

### A20. Forking needs no new right

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §8
- **Change:** extends §2
- **Summary:** A fork is an edit: `createpage` and `edit` on the title, with the history, dependency and talk-page seeding part of what saving means, and `import` not required. Copying a fork's files needs the upload rights.

### A21. Confidential `read` restrictions, the tenant and set targets

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §3, §4, §13
- **Change:** amends §2, §4; extends §4, §7
- **Summary:** `protect` may restrict `read`: a confidential restriction, distinct from the moderation kind that deletion and suppression set, which keeps the target live for its group and absent for everyone else; the setting actor must be a member of the group named. Two ACL targets are added: `tenant`, the root of enclosure and a tenant's visibility, and `set`, a grouping of pages, entities and threads by ID that only `protect` can change. The REST routes for both are in 0056 §13.

Replaced text (§2):

> | `protect` | Setting ACLs on pages, entities and namespaces (§4) | `sysop` |

Replaced text (§4):

> Setting an ACL on a page, entity or namespace needs `protect`; on a graph, `ts-config`; a `read` ACL needs `delete`, `deleterevision`, `suppressrevision` or `hideuser` by target ([0023](0023-moderation.md) §11).

> Enclosure is fixed: a graph encloses the records written to it; a namespace encloses its pages and entities; a page encloses its subpages;

### A22. How `instance create` sets the owner's password

- **Date:** 2026-10-03
- **Source:** Direct: James, decision of 2026-10-03 (`triplespace-accounts`, `triplespace-api-action`, `triplespace-server`, `scatter-adapter-internetdomains`)
- **Change:** extends §3
- **Summary:** The password `instance create` gives the first administrator comes from `--owner-password-file` or `TRIPLESPACE_OWNER_PASSWORD`; without either the owner has no binding until `triplespace password set`. The owner group is written to the tenant's `config` as `group:owner`; the other default groups come from the embedded registry until an instance customizes them.

### A23. MediaWiki names are the permission names

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §1
- **Summary:** The MediaWiki names are the permission names. The Scatterbase column of §1's mapping table (`view`, `view_deleted`, `block_user`) is historical: it records where the five early permissions came from and names nothing a client or a record uses. (PENDING C10)

Replaced text (§1):

> The Scatterbase permissions map onto MediaWiki's rights and are known to MediaWiki clients by MediaWiki's names:

### A24. Rights the index of later ADRs lacked

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §2
- **Summary:** The table of permissions added by later ADRs gains `browsearchive` in the Moderation row ([0023](0023-moderation.md) §11) and `managechangetags` and `changetags` in the Edit filters row ([0030](0030-edit-filters.md) §5), and the instance-rights list gains `ts-viewoperator` ([0040](0040-instance-prerogatives.md) §9). `docs/registry/groups.toml` already carries all four. (PENDING C11)

### A25. Jobs run under subsidiaries

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §2
- **Summary:** Jobs run under subsidiaries: `ts-runjob` defaults to `bot` everywhere, and §2 drops `sysop` from its default groups. An administrator runs a job by creating or approving a subsidiary they operate ([0024](0024-subsidiary-accounts.md) §3); 0047 §12's `sysop` job rate row goes with it (0047's log). James: "administrators should be able to run jobs under subsidiaries", read as this. (PENDING C14)

Replaced text (§2):

> | `ts-runjob` | Submitting bulk jobs (0002 §8.1). A bulk `retain` or `convert` also needs the right above | `bot`, `sysop` |

### A26. The `actors` graph's record kinds

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §4
- **Summary:** The default graph ACL table's `actors` row names the records by their payload types: "`membership` and `block` records" (payload type `scatter:v0/membership`, as §3 says), not "`rights` and `block` records"; `rights/rights` is the log event a membership projects as, not the record. (PENDING C15)

Replaced text (§4):

> | `actors` | Written only by account management; `rights` and `block` records need `userrights` and `block` respectively |

### A27. `deletedhistory` and `deletedtext` gate nothing

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §2, §5
- **Summary:** `deletedhistory` and `deletedtext` are reported to MediaWiki clients by members of the deletion group and gate no evaluation of their own: what may still be seen is decided by the `read` ACL's group ([0023](0023-moderation.md) §11). §5's answer to 0010's "see hidden usernames" is therefore membership in the deletion group, not `deletedhistory`. (PENDING C16)

Replaced text (§2):

> | `deletedhistory`, `deletedtext` | Seeing hidden revisions, hidden usernames and deleted pages ([0001](0001-revision-metadata-rdf.md) §4) | `sysop` |

Replaced text (§5):

> | [0010](0010-site-ui.md) | Revert a job, see hidden usernames, erase, retain from the history page | `ts-revertjob`, `deletedhistory`, `ts-erase`, `ts-retain` |

### A28. Retiring an ACL over REST

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §7
- **Summary:** `DELETE /acl/{kind}/{id}` under `triplespace/v0` retires an ACL, beside the `GET` and `PUT` of §7; [0023](0023-moderation.md) §8 already assumes it. (PENDING E38)

### A29. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A30. `autoconfirmed` is an implicit group computed from `view.actor`; `pending` is OAuth-only

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** corrects §3
- **Summary:** `autoconfirmed` is an implicit group, like `user`: no membership record is written for it, and an account is a member once `view.actor.created_at` and `view.actor.editcount` meet the age and edit-count thresholds set in `site` configuration; `editcount` is maintained by the activity projection from the account's local `edit` rows, erased ones included and job rows excluded, so a bot's job writes do not confirm its operator. Hand-made subsidiaries are never pending: `pending` is a status only an OAuth authorization sets, the MCP requirement is the `basic` grant, and `instance create` puts the sync subsidiaries in `bot`. (REVIEW G40)

Replaced text ([09](../architecture/09-security-and-moderation.md) §3.1, as it stood):

> `user` is every registered local account, except a subsidiary with status `pending`, which receives `user` only by an explicit membership. `autoconfirmed` is granted automatically once an account meets an age and edit-count threshold set in `site` configuration.

### A31. Global groups are copied from the registry at `instance create` and kept by `registry sync`

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §3
- **Summary:** An instance's `config` starts as a copy of the registry, the default groups of §3 included: `instance create` appends a config record for every registry entry, `triplespace-cli registry sync` appends records for registry entries newer than the instance's under the instance attestation, and `instance check` warns when the instance is behind; every config record carries a per-kind `schema` integer and `view.registry` projects an unknown kind or schema opaquely. The ledger's verb is amends; §3's chapter text ([09](../architecture/09-security-and-moderation.md) §3.1–§3.6) says nothing about how the shipped groups reach an instance's log, so this entry extends; the contradicted text is [0015](0015-record-format-and-partition-registry.md) §3's and §5's, logged there as 0015 A58. (REVIEW G36)

### A32. `property-create` is held by `user` by default; `ts-linkproperty` links properties

- **Date:** 2026-10-09
- **Source:** Direct: James, review of `Special:NewProperty` on 2026-10-09
- **Change:** amends §2, §5
- **Summary:** `property-create` is granted to `user` by default, as a stock Wikibase grants it, so a registered account can create properties through `wbeditentity` and `Special:NewProperty`, and Wikibase clients (WikibaseIntegrator, WikidataIntegrator, Pywikibot) that create properties work unchanged. `propertycreator` and `sysop` keep it, so a tenant that wants Wikidata's arrangement removes it from `user` in its configuration. What changes predicates for every consumer stays restricted: `equivalent-property` links and the cross-namespace property redirect of [0004](0004-identity-clusters-and-equivalence.md) A4 move from `property-create` to a new right, `ts-linkproperty`, held by `propertycreator` and `sysop` (0004 A22). `wbeditentity` with `new=property` enforces it, which it had not before. A key exercises `property-create` through the `editentity` or `createeditmovepage` grant, and `ts-linkproperty` through `editentity` ([0024](0024-subsidiary-accounts.md) A18).

Replaced text ([09](../architecture/09-security-and-moderation.md) §2.2, as it stood):

> | `property-create` | `propertycreator`, `sysop` | Creating a local property; `equivalent-property` links, because they change predicates for every consumer | [0016](../decisions/0016-permissions-and-access-control.md) §2 |

Replaced text ([09](../architecture/09-security-and-moderation.md) §2.4, as it stood):

> | `same-as`, `different-from`, bulk links, conflict resolution | `ts-link`; in bulk, with `ts-runjob`; `equivalent-property` needs `property-create` |
