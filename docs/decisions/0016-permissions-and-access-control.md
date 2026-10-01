# 0016. Permissions and access control

- **Status:** Proposed
- **Date:** 2026-09-26
- **Author:** James Hare / Claude Fable
- **Amended by:** [0019 — Discussions](0019-discussions.md) (§12 adds `ts-editpost` and a talk-page enclosure rule), [0020 — Change feeds](0020-change-feeds.md) (§7 adds `viewmywatchlist` and `editmywatchlist`), [0021 — Notifications](0021-notifications.md) (§9 adds `editmyoptions` and `viewmyprivateinfo`), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§1–3 amend §4: ACLs restrict `read` as well as writes, gain `record` and `actor` targets, and moderation ACLs live in the tenant `log`; §11 extends §2 with `patrol`, `autopatrol` and the `autopatrolled` group; settles the patrolling open question), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§3 amends §3: an operator's blocks reach their subsidiaries, and ownership rules gain the operator's; §4 intersects permissions with a key's grants; §11 extends §2 with `createaccount`, `ts-transferaccount` and `noratelimit`; settles the rate-limit open question), [0028 — Tenancy policy](0028-tenancy-policy.md) (§3–4 amend §3: a tenant account's effective permissions include the global groups, and subtract the global blocks, of the farm account it is publicly linked to), [0030 — Edit filters](0030-edit-filters.md) (§12 extends §2 with the `abusefilter-*` rights; settles the abuse-filter open question). The permissions these add are listed in `docs/registry/groups.toml`, the registry of record, [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§8 and §12 extend §2–3: the `federated` group, holding no permissions), [0025 — The instance as an OAuth server](0025-oauth-server.md) (§10 extends §2 with the `mwoauth*` permissions; §3 refines §3: a `pending` subsidiary does not receive implicit `user`), [0035 — Adopting an existing Wikibase as a tenant](0035-adopting-a-wikibase.md) (§5 amends §3: on an adopting tenant `instance create --adopt --owner` creates the owner under the source wiki's user ID), [0039 — Files, blob storage and foreign file repositories](0039-files-and-media.md) (§21 extends §2: file and operator rights; §10 extends §6: erasures made by the instance), [0040 — Instance prerogatives](0040-instance-prerogatives.md) (§9 extends §2: instance rights and `ts-viewoperator`)
- **Related:** [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (§5 settles the permissions open question), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§3 and §9 add tables to §5 and IP blocks to `private`), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md) (§5 settles the permissions open question), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (amends §2 and §4.1), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (§6 settles the reason-class visibility open question), [0007 — Actor identity](0007-actor-identity.md) (§5 settles the permissions open question; §3), [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md) (§5 settles the protection open question; §6), [0010 — Site UI](0010-site-ui.md) (§5 and §6 settle the permissions and erasure-visibility open questions; §7 extends §11–12), [0011 — Upstream and local logs](0011-logs.md) (amends §6.1; settles the permissions open question), [0012 — API requirements for the site UI](0012-api-requirements.md) (amends §4 and §8; settles the groups open question), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (§3), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

[0012](0012-api-requirements.md) §8 names the rights Triplespace needs and reuses MediaWiki's where they exist, but leaves open which groups hold them. Seven ADRs carry a permissions question that depends on the answer: who may run `retain` and `convert` or set the default retention policy ([0002](0002-source-graphs-and-mass-ingest.md)); who may assert `same-as` and `different-from` or resolve conflicts ([0004](0004-identity-clusters-and-equivalence.md)); who may rename, hide or vanish accounts and remove links ([0007](0007-actor-identity.md)); who may create, move, delete and protect pages ([0008](0008-namespaces-and-document-pages.md)); who may revert jobs, see hidden usernames, erase, or retain from the history page ([0010](0010-site-ui.md)); who may protect, block and change rights ([0011](0011-logs.md)); and how erasure reasons are shown ([0006](0006-log-integrity-and-erasure.md), 0010, 0011).

Two models are already in play.

**MediaWiki's.** Users belong to groups; groups hold named rights; a user's rights are the union over their groups. `*` is everyone, `user` every registered account, and `sysop`, `bureaucrat` and `suppress` hold the administrative rights. Page protection adds a restriction on top: an action on a protected page also requires membership in the protecting group, until an expiry. Blocks take rights away from one account. `$wgRevokePermissions` can subtract rights from a group, but Wikimedia does not use it. Clients read all of this from `meta=siteinfo&siprop=usergroups`, `meta=userinfo&uiprop=rights|groups`, `list=users&usprop=groups|rights`, and change it with `action=userrights`, `action=protect` and `action=block` ([mediawiki-compat.md](../api/mediawiki-compat.md) §5). Wikibase adds `item-term`, `property-term`, `item-merge`, `item-redirect` and `property-create`.

**Scatterbase's.** Its decision record says: "The early permission set was view, edit, delete, view_deleted, and block_user. owner has all permissions and universe covers everyone. The API should create users, groups, memberships, and permissions, and set URI ACLs that can govern blobs and views and potentially individual claims. Authentication is pluggable, with a simple single user fallback. Exact matching, inheritance, deny precedence, a property specific rule, and whether ACL changes are themselves signed claims remain open."

The two agree on the shape: users, groups, memberships, named permissions, and restrictions attached to things by URI. This ADR adopts Scatterbase's vocabulary for the parts MediaWiki has no name for, and MediaWiki's names for everything MediaWiki clients will see, so that one model serves both products and Scatterbase can inherit it.

## Decision

### 1. Vocabulary

| Term | Meaning | MediaWiki clients see |
|---|---|---|
| **Permission** | A named capability, such as `edit` or `ts-erase`. A string. | A right |
| **Group** | A named set of permissions | A user group |
| **Membership** | An actor's membership in a group, with an optional expiry | Group membership |
| **`universe`** | The group that contains everyone, including anonymous readers and temporary accounts. It cannot be joined or left. | `*` |
| **`owner`** | The group that holds every permission, including any added later. It always has at least one member. | A group named `owner` whose rights are the full list |
| **ACL** | An access-control entry attached to a target by identifier, restricting named permissions to a group until an expiry | Page protection, where the target is a page |
| **Block** | A record that removes named permissions from one actor until an expiry | A block |

The Scatterbase permissions map onto MediaWiki's rights and are known to MediaWiki clients by MediaWiki's names:

| Scatterbase | MediaWiki | Meaning |
|---|---|---|
| `view` | `read` | Read public data |
| `edit` | `edit` | Write to the local graph and to document pages |
| `delete` | `delete` | Delete document pages |
| `view_deleted` | `deletedhistory`, `deletedtext` | See hidden revisions and deleted pages |
| `block_user` | `block` | Block accounts |

### 2. Permissions

The permission set is the union of MediaWiki's rights that Triplespace implements, Wikibase's, and the `ts-*` rights of [0012](0012-api-requirements.md) §8, with three additions. Each is listed with the action it governs and the group that holds it by default.

| Permission | Governs | Default groups |
|---|---|---|
| `read` | Reading anything public | `universe` |
| `edit` | Local-graph `add`, `remove` and `override` on any subject ([0002](0002-source-graphs-and-mass-ingest.md) §8.2); page `edit` ([0008](0008-namespaces-and-document-pages.md) §4) | `user`, `temp` |
| `createpage` | Page `create` | `user`, `temp` |
| `item-term`, `property-term` | Labels, descriptions and aliases of items and properties | `user` |
| `item-redirect`, `item-merge` | Local `redirect` between local items ([0004](0004-identity-clusters-and-equivalence.md) §9) | `user` |
| `property-create` | Creating a local property; `equivalent-property` links, because they change predicates for every consumer (0004 §6) | `propertycreator`, `sysop` |
| `ts-link` *(new)* | `same-as` and `different-from`; resolving held conflicts (0004 §3, §10) | `autoconfirmed` |
| `move` | Page `move` | `autoconfirmed` |
| `editcontentmodel` | `action=changecontentmodel` (0008 §5) | `user` |
| `delete`, `undelete` | Page `delete` and `undelete` | `sysop` |
| `deletedhistory`, `deletedtext` | Seeing hidden revisions, hidden usernames and deleted pages ([0001](0001-revision-metadata-rdf.md) §4) | `sysop` |
| `deleterevision`, `deletelogentry` | Hiding parts of revisions and log events | `sysop` |
| `suppressrevision`, `viewsuppressed`, `hideuser` | Suppression, and seeing what is suppressed | `suppress` |
| `protect` | Setting ACLs on pages, entities and namespaces (§4) | `sysop` |
| `block` | Blocking accounts (§3) | `sysop` |
| `userrights` | Changing memberships in any group except `owner` | `bureaucrat` |
| `renameuser` | Renaming any account ([0007](0007-actor-identity.md) §4) | `bureaucrat` |
| `import`, `importupload` | Page imports (0008 §9) | `sysop` |
| `upload`, `reupload`, `reupload-own`, `movefile` *(added by [0039](0039-files-and-media.md) §21)* | Uploading and overwriting files; moving file pages | `autoconfirmed` |
| `reupload-shared`, `upload_by_url` *(0039 §21)* | Shadowing a foreign repository's file with a local one; uploading from a URL | `sysop`; none |
| `ts-takedown`, `ts-expunge`, `ts-viewtakedown` *(0039 §21)* | Operator removal of files across every tenant, evaluated on the primary tenant | `owner` |
| `bot` | The bot flag on edits ([0010](0010-site-ui.md) §7) | `bot` |
| `ts-retain`, `ts-convert` | `retain` and `convert` on entities ([0002](0002-source-graphs-and-mass-ingest.md) §5–6) | `sysop` |
| `ts-runjob` | Submitting bulk jobs (0002 §8.1). A bulk `retain` or `convert` also needs the right above | `bot`, `sysop` |
| `ts-revertjob` | Reverting a job ([0010](0010-site-ui.md) §9) | `sysop`, and the job's own actor |
| `ts-viewrejects` | Downloading a job's rejects file | `sysop`, and the job's own actor |
| `ts-erase` | Appending `erase` records ([0006](0006-log-integrity-and-erasure.md) §7, [0015](0015-record-format-and-partition-registry.md) §1) | `suppress` |
| `ts-viewerasures` | Seeing an erasure's reason class and authority reference (§6) | `sysop` |
| `ts-unlink` *(new)* | Removing another user's account link (0007 §7) | `sysop` |
| `ts-config` *(new)* | Writing `config` records ([0015](0015-record-format-and-partition-registry.md) §3): providers, issuers, namespaces, keyed types, roles, reconciliation, site settings, graph ACLs, the default retention policy | `bureaucrat` |
| `ts-keys` *(new)* | Registering and rotating the instance key (0006 §6) | `owner` only |

**Ownership rules** are built in and are not records. The holder of an account may rename it ([0010](0010-site-ui.md) §11), vanish it, and add or remove its own bindings and links ([0007](0007-actor-identity.md) §3, §7). The owner of a user page may edit its `json` and `yaml` subpages ([0008](0008-namespaces-and-document-pages.md) §6). The actor who ran a job may revert it and read its rejects. No permission grants access to another user's bindings: the `accounts` graph is outside this model entirely ([0012](0012-api-requirements.md) §8).

> **Extended by [0040](0040-instance-prerogatives.md) §9.** **Instance rights** authorize instance acts and are evaluated only on the primary tenant or through a global group at the farm base: `ts-keys`, `ts-config` at the farm base, `abusefilter-modify` for global filters, `userrights`, `block` and `renameuser` on farm accounts, `ts-takedown` and `ts-expunge`. A new right, `ts-viewoperator` (`owner`), reveals which operator carried out an instance act.

### 3. Groups, memberships and blocks

**Default groups.** The instance ships with `universe`, `temp`, `user`, `autoconfirmed`, `bot`, `propertycreator`, `sysop`, `bureaucrat`, `suppress`, `owner` and, since [0022](0022-federation.md) §8, `federated`, the group of fediverse surrogates, which holds no permissions until a tenant's ACL names it. MediaWiki's `interface-admin` is not created, because there are no user scripts or styles ([0008](0008-namespaces-and-document-pages.md) §5). `temp` is the group of temporary accounts ([0007](0007-actor-identity.md) §3); its default permissions are `read`, `edit` and `createpage`, and an instance may empty it to require login. `user` is every registered local account, except a subsidiary with status `pending` ([0025](0025-oauth-server.md) §3), which receives `user` only by an explicit membership. `autoconfirmed` is granted automatically once an account meets an age and edit-count threshold set in `site` configuration.

**Groups are configuration.** A group is a `config` record of kind `group` ([0015](0015-record-format-and-partition-registry.md) §3) listing its permissions. The defaults above ship in `docs/registry/groups.toml` (0015 §5) and are written to `config` when an instance is created. Changing a group's permissions needs `ts-config`.

**Memberships are actor records.** A membership change is a record of payload type `scatter:v0/membership` in the `actors` partition ([0007](0007-actor-identity.md) §8), keyed by the actor, whose content names the group, whether the actor is added or removed, and an expiry. Its comment part holds the reason. It projects as a `rights/rights` log event ([0011](0011-logs.md) §6.1). Vanishing an account erases its memberships with everything else keyed to it.

**Blocks are actor records too.** A block is a record of payload type `scatter:v0/block` in the `actors` partition, keyed by the blocked actor, naming the permissions removed (by default `edit` and `createpage`, optionally everything but `read`) and an expiry. It projects as `block/block`, `block/reblock` or `block/unblock`. A block is the only negative rule in the model: an actor's effective permissions are the union of their groups' permissions, minus what their active blocks remove. There is no revocation at the group level.

**Blocks on IP addresses** apply to anonymous requests before a temporary account exists. IP addresses are never written to the log ([0007](0007-actor-identity.md) §3), so IP blocks live in the `private` schema ([0013](0013-postgres-storage.md) §4) and appear in `list=blocks` without appearing in the log.

**`owner` is created with the instance.** `triplespace-cli` creates the first local user, `local:1`, as the sole member of `owner`, and gives it a binding to the built-in `password` issuer with a password the CLI sets ([0007](0007-actor-identity.md) §3, as amended 2026-09-27); the earlier wording, "a login token", is withdrawn, since a primary account holds no token ([0024](0024-subsidiary-accounts.md) §1). That is the single-user fallback Scatterbase asks for: an instance with no external issuer configured has exactly this one user, who logs in with a password and registers issuers, groups and the sync subsidiaries from there. Membership in `owner` is changed only by a member of `owner`, and the last member cannot be removed.

> **Amended by [0035](0035-adopting-a-wikibase.md) §5.** On a tenant that adopts an existing Wikibase, `instance create --adopt {source} --owner {user_id}` creates the owner under the source's user ID instead of `local:1`, with the same password binding, and sets the user-ID floor; "`local:1`" here reads as "the owner account".

### 4. ACLs (amends 0005 §4.1)

An ACL attaches a restriction to a **target**, named by identifier so that the record's key is never content ([0006](0006-log-integrity-and-erasure.md) §3):

| Target kind | Key | Restricts |
|---|---|---|
| `graph` | `acl:graph:{name}` | Writing records to the graph. This replaces the *writers* field of [0005](0005-crate-organization.md) §4.1, which was already an ACL under another name |
| `namespace` | `acl:namespace:{number}` | Actions on every page or entity in the namespace |
| `page` | `acl:page:{page id}` | Actions on one document page and, if the namespace allows subpages, its subpages |
| `entity` | `acl:entity:{id}` | Local assertions about one entity, and its `retain` and `convert` |
| `statement` | `acl:statement:{guid}` | One statement, its qualifiers and references; `read` hides it ([0023](0023-moderation.md) §2, as amended) |
| `property` | `acl:property:{id}` | Every snak using the property, as main snak, qualifier or reference, on any entity; enclosure by predicate ([0023](0023-moderation.md) §2, as amended) |

An ACL record is a `config` record of kind `acl` ([0015](0015-record-format-and-partition-registry.md) §3). Its content lists, for each permission it restricts, the group whose members may still perform it, and an expiry. Setting an ACL on a page, entity or namespace needs `protect`; on a graph, `ts-config`. Page ACLs project as `protect/protect`, `protect/modify` and `protect/unprotect` log events with the parameters [0011](0011-logs.md) §7 already lists.

> **Amended by [0023](0023-moderation.md) §1–3.** ACLs restrict `read` as well as write permissions, which is what deletion, revision-hiding, suppression and username-hiding are. Two target kinds are added, `record` (with a `parts` list) and `actor`. The record is the payload type `scatter:v0/acl`, not a config kind: graph ACLs are appended to `config`; every other target's ACL is a moderation record in the tenant `log` partition, so that suppressions stay out of public dumps. Writing a `read` ACL needs `delete`, `deleterevision`, `suppressrevision` or `hideuser` by target, as 0023 §11 lists.

**Evaluation is conjunctive.** To perform action *A* on target *T*, an actor must hold the permission for *A* after blocks (§3), **and** satisfy every ACL that restricts *A* on *T* or on a target enclosing *T*. Enclosure is fixed: a graph encloses the records written to it; a namespace encloses its pages and entities; a page encloses its subpages; a talk page encloses the threads attached to it ([0019](0019-discussions.md) §12). There is no other inheritance, no cascading protection, and no rule under which a more specific ACL loosens a broader one. This is how MediaWiki protection composes with group rights, and it dissolves Scatterbase's questions about exact matching, inheritance and deny precedence: matching is by target and enclosures, inheritance is enclosure only, and nothing denies except a block.

**Default graph ACLs**, written with the instance:

| Graph | `edit` restricted to |
|---|---|
| `local`, `pages` | `universe` (no restriction beyond the permission itself) |
| `mirror/{provider}`, `actors/{provider}`, `log/{provider}` | `bot`, so only sync jobs write there ([0002](0002-source-graphs-and-mass-ingest.md) §2) |
| `config` | `bureaucrat`; `key:` records additionally need `ts-keys` |
| `actors` | Written only by account management; `rights` and `block` records need `userrights` and `block` respectively |
| `accounts` | The login system only; no group |

### 5. What each earlier ADR's question resolves to

| ADR | Question | Answer |
|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) | Bulk `retain` or `convert`; the default retention policy | `ts-retain` or `ts-convert` with `ts-runjob`; the default is a `config` record, so `ts-config` |
| [0004](0004-identity-clusters-and-equivalence.md) | `same-as`, `different-from`, bulk links, conflict resolution | `ts-link`; in bulk, with `ts-runjob`; `equivalent-property` needs `property-create` |
| [0007](0007-actor-identity.md) | Rename, hide, vanish, remove a link | Own account: the holder. Others: `renameuser`, `hideuser`, `ts-unlink`. Nobody vanishes another's account |
| [0008](0008-namespaces-and-document-pages.md) | Create, move, delete, protect pages | `createpage`, `move`, `delete`, `protect`, plus the user-page ownership rule |
| [0010](0010-site-ui.md) | Revert a job, see hidden usernames, erase, retain from the history page | `ts-revertjob`, `deletedhistory`, `ts-erase`, `ts-retain` |
| [0011](0011-logs.md) | Protection, blocks, user rights | §3–4; each projects as the log events 0011 §6.1 reserved |
| [0012](0012-api-requirements.md) | Which groups hold which rights | §2 |

### 6. Erasure visibility (settles 0006, 0010 and 0011)

That a record was erased is public: the gap row of [0010](0010-site-ui.md) §5.2 and the `erased` flag of [0012](0012-api-requirements.md) §4 are shown to everyone, as MediaWiki shows that a revision was deleted. The **reason class** and the **authority reference** of an `erase` record ([0006](0006-log-integrity-and-erasure.md) §7) are shown only to holders of `ts-viewerasures`. The `erase/erase` log event ([0011](0011-logs.md) §6.1) follows the same rule: listed for everyone, with its parameters visible to `ts-viewerasures`. The reason-class vocabulary is `legal`, `privacy`, `upstream` and `operational`; an instance may add classes in `site` configuration.

> **Extended by [0039](0039-files-and-media.md) §10 and [0040](0040-instance-prerogatives.md) §7.** Erasures the instance makes in a tenant's partitions (an operator's expunge, scheduled reclamation of long-deleted file versions) follow the same rule. They are attributed to the instance operator, and their authority is a record in the instance `log`.

### 7. API and UI

**Action API.** Additive, under [0012](0012-api-requirements.md) §1: `meta=siteinfo&siprop=usergroups` lists every group with its permissions, reporting `universe` as `*`; `meta=userinfo&uiprop=rights|groups|blockinfo`; `list=users&usprop=groups|rights|blockinfo`; `action=userrights`, `action=protect`, `action=block` and `action=unblock` as in MediaWiki; `list=blocks` and `list=protectedtitles`; `prop=info&inprop=protection`. Every write module returns `permissiondenied` with the missing permission named. `action=protect` on an entity page sets an `entity` ACL.

**REST.** `GET /acl/{kind}/{id}` and `PUT /acl/{kind}/{id}` under `triplespace/v0` for graph and entity targets, which `action=protect` cannot express fully.

**Site UI** ([0010](0010-site-ui.md)). `Special:ListGroupRights`, `Special:UserRights`, `Special:Block`, `Special:BlockList` and `Special:ProtectedPages` appear where MediaWiki users expect them. The page overflow menu gains **Protect…** for pages and entities. The account settings page (0010 §11) is unchanged, since every action on it is an ownership rule.

### 8. Scatterbase

Scatterbase inherits §1, §3 and §4 whole: the vocabulary, groups and memberships as records, blocks as the only negative rule, ACLs by target with conjunctive evaluation over enclosures, and the single-user fallback. Its five open points are answered as follows. Matching is exact on the target plus its enclosures. Inheritance is enclosure only. Deny has no precedence because there is no deny; a block is the only subtraction and it applies before ACLs are consulted. A property-specific rule is the `property` target kind ([0023](0023-moderation.md) §2, as amended). ACL changes are records in the `config` partition, signed by the server like every record ([0006](0006-log-integrity-and-erasure.md) §1); Scatterbase may make them client-signed claims in its `server` graph without changing their shape.

### 9. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-actors` | The permission model: groups, memberships, blocks, ACL targets and enclosure, and the effective-permission and ACL evaluation of §3–4. Pure; shared with Scatterbase |
| `scatter-log` | The `membership` and `block` payload types; the `group` and `acl` config kinds |
| `triplespace-projections` | `view.group`, `view.membership`, `view.acl` and `view.block`, with `groups text[]` on `view.actor` |
| `triplespace-accounts` | Resolving a session to an actor and its effective permissions; IP blocks in `private` |
| `triplespace-api-action`, `triplespace-api-rest` | The modules and routes of §7 |
| `triplespace-cli` | Creating `owner` (§3) |

No crate is added.

## Consequences

- **Every earlier permissions question has an answer**, and each is one row in §2 or §5 rather than a new mechanism.
- **MediaWiki clients and users see MediaWiki.** The same groups, rights, modules and special pages, with `universe` and `owner` as the only names they have not seen before.
- **One negative rule.** Blocks are the only way to take a permission away from someone who holds it through a group. This is simpler than MediaWiki, which also has `$wgRevokePermissions`, and it is the answer to Scatterbase's deny question.
- **Permissions have history.** Groups and ACLs are `config` records, memberships and blocks are `actors` records; all are signed, ordered and rebuildable, and each projects as the log event MediaWiki would write.
- **A vanished account loses its memberships**, because they are keyed to it and erased with it. An `owner` cannot vanish while sole member.
- **Graph ACLs replace the *writers* field**, so [0005](0005-crate-organization.md) §4.1's registry entry has one field fewer and one more record kind beside it.
- **Scatterbase gets a permission system it did not have to design**, at the cost of adopting MediaWiki's right names for the five it had.

## Open questions

- ~~**`statement` and `property` ACL targets.** Whether a restriction on one statement, or on every statement of one property (Scatterbase's property-specific rule), is worth the evaluation cost on every local write.~~ *Settled by [0023](0023-moderation.md) §2 as amended on 2026-09-27: both exist; the cost is one indexed lookup per property touched.*
- **`autoconfirmed` thresholds** and whether they should follow Wikimedia's defaults. [0030](0030-edit-filters.md) asks the same of autopromotion.
- ~~**Rate limits and throttles** (`$wgRateLimits`), which are operational state rather than permissions, and where they are configured.~~ *Settled by [0024](0024-subsidiary-accounts.md) §5: `site` configuration by action class and group, counted in Valkey.*
- ~~**Patrolling and `autopatrol`**, which wait on [0010](0010-site-ui.md)'s patrolling decision.~~ *Settled by [0023](0023-moderation.md) §6 and §11.*
- ~~**Abuse filters** and other pre-save checks, which are neither permissions nor ACLs.~~ *Settled by [0030](0030-edit-filters.md): edit filters, evaluated in the appending transaction after permissions and ACLs.*
- ~~**Whether `owner` should be a single account rather than a group** on instances that want a legal owner of record.~~ *Settled 2026-09-27: `owner` stays a group with at least one member, which key rotation and continuity need. An instance that wants a named legal owner sets `instance.owner_of_record` in `site` configuration to one of its members; it is shown on `Special:Version` and in `siprop=triplespace` and grants nothing.*
- ~~**Group membership privacy.** MediaWiki exposes group membership publicly; whether an instance may hide `suppress` membership, as Wikimedia hides some, would need a private group flag.~~ *Settled 2026-09-27: no. Every membership is public, as `list=users&usprop=groups` and the `rights/rights` event ([0011](0011-logs.md) §6.1) promise; an instance that wants a discreet suppression team keeps it small.*

## References

- [Manual:User rights](https://www.mediawiki.org/wiki/Manual:User_rights), [Manual:$wgGroupPermissions](https://www.mediawiki.org/wiki/Manual:$wgGroupPermissions), [Manual:$wgRevokePermissions](https://www.mediawiki.org/wiki/Manual:$wgRevokePermissions)
- [Manual:Protection](https://www.mediawiki.org/wiki/Manual:Protection), [Manual:Block and unblock](https://www.mediawiki.org/wiki/Manual:Block_and_unblock)
- [Wikibase user rights](https://www.mediawiki.org/wiki/Wikibase/Installation/Advanced_configuration#User_rights)
- [API:Userrights](https://www.mediawiki.org/wiki/API:Userrights), [API:Protect](https://www.mediawiki.org/wiki/API:Protect), [API:Block](https://www.mediawiki.org/wiki/API:Block)
- Scatterbase decision record (permissions, `owner`, `universe`, URI ACLs)
