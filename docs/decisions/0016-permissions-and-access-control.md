# 0016. Permissions and access control

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-01 (A18)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [MediaWiki API contract](../api/mediawiki-compat.md)

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

*Changed by A2, A3, A4, A5, A6, A7, A8, A10, A13, A14, A15, A16.*

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

**Ownership rules** are built in and are not records. The holder of an account may rename it ([0010](0010-site-ui.md) §11), vanish it, and add or remove its own bindings and links ([0007](0007-actor-identity.md) §3, §7). The owner of a user page may edit its `json` and `yaml` subpages ([0008](0008-namespaces-and-document-pages.md) §6). The actor who ran a job may revert it and read its rejects. The operator of a subsidiary may edit its user pages, issue and revoke its keys, retire, reactivate and rename it ([0024](0024-subsidiary-accounts.md) §3). No permission grants access to another user's bindings: the `accounts` graph is outside this model entirely ([0012](0012-api-requirements.md) §8).

**Permissions added by later ADRs.** Each ADR lists its permissions with their defaults; `docs/registry/groups.toml` is the registry of record.

| Area | Permissions | ADR |
|---|---|---|
| Threads | `ts-editpost`; `edit`, `move`, `delete`, `deleterevision` and `ts-erase` applied to threads and posts | [0019](0019-discussions.md) §12 |
| Watchlists | `viewmywatchlist`, `editmywatchlist` | [0020](0020-change-feeds.md) §7 |
| Preferences and contacts | `editmyoptions`, `viewmyprivateinfo` | [0021](0021-notifications.md) §9 |
| Federation | `ts-config` for trust modes and policy lists; `protect` to enable inbound replies on a talk namespace | [0022](0022-federation.md) §12 |
| Moderation | `protect`, `delete`, `deleterevision`, `suppressrevision`, `hideuser` and the rest, by ACL target; `patrol`, `autopatrol` and the `autopatrolled` group | [0023](0023-moderation.md) §11 |
| Subsidiaries | `createaccount`, `ts-transferaccount`, `noratelimit`; `bot` also selects the rate-limit row | [0024](0024-subsidiary-accounts.md) §11 |
| OAuth | `mwoauthproposeconsumer`, `mwoauthupdateownconsumer`, `mwoauthmanageconsumer`, `mwoauthmanagemygrants`, `mwoauthviewprivate` | [0025](0025-oauth-server.md) §10 |
| Edit filters | `abusefilter-view`, `abusefilter-log`, `abusefilter-view-private`, `abusefilter-log-private`, `abusefilter-log-detail`, `abusefilter-modify`, `abusefilter-modify-restricted`, `abusefilter-revert`, `abusefilter-bypass` | [0030](0030-edit-filters.md) §12 |
| Files | `upload`, `reupload`, `reupload-own`, `reupload-shared`, `upload_by_url`, `movefile`; the operator rights `ts-takedown`, `ts-expunge`, `ts-viewtakedown` | [0039](0039-files-and-media.md) §21 |
| Instance acts | `ts-viewoperator` | [0040](0040-instance-prerogatives.md) §9 |
| The primary tenant | `ts-primary` | [0046](0046-primary-tenant.md) §8 |
| Nuke | `nuke`, MediaWiki's right from the Nuke extension; default `sysop`, and part of the `delete` grant | [0047](0047-special-pages.md) §12 |

**Instance rights** authorize instance acts and are evaluated only on the primary tenant, the tenant that is primary at the time of the act, or through a global group at the farm base; held on any other tenant they grant nothing at instance scope ([0040](0040-instance-prerogatives.md) §9, [0046](0046-primary-tenant.md) §8). They are `ts-keys`, `ts-primary`, `ts-config` at the farm base, `abusefilter-modify` for global filters, `userrights`, `block` and `renameuser` on farm accounts, `ts-takedown`, `ts-expunge`, `mwoauthmanageconsumer`, and `ts-runjob` for instance jobs.

### 3. Groups, memberships and blocks

*Changed by A5, A7, A8, A9, A11, A12, A15.*

**Default groups.** The instance ships with `universe`, `temp`, `user`, `autoconfirmed`, `bot`, `propertycreator`, `sysop`, `bureaucrat`, `suppress`, `owner` and `federated`, the group of fediverse surrogates, which holds no permissions until a tenant's ACL names it ([0022](0022-federation.md) §8). MediaWiki's `interface-admin` is not created, because there are no user scripts or styles ([0008](0008-namespaces-and-document-pages.md) §5). `temp` is the group of temporary accounts ([0007](0007-actor-identity.md) §3); its default permissions are `read`, `edit` and `createpage`, and an instance may empty it to require login. `user` is every registered local account, except a subsidiary with status `pending` ([0025](0025-oauth-server.md) §3), which receives `user` only by an explicit membership. `autoconfirmed` is granted automatically once an account meets an age and edit-count threshold set in `site` configuration.

**Groups are configuration.** A group is a `config` record of kind `group` ([0015](0015-record-format-and-partition-registry.md) §3) listing its permissions. The defaults above ship in `docs/registry/groups.toml` (0015 §5) and are written to `config` when an instance is created. Changing a group's permissions needs `ts-config`.

**Memberships are actor records.** A membership change is a record of payload type `scatter:v0/membership` in the `actors` partition ([0007](0007-actor-identity.md) §8), keyed by the actor, whose content names the group, whether the actor is added or removed, and an expiry. Its comment part holds the reason. It projects as a `rights/rights` log event ([0011](0011-logs.md) §6.1). Vanishing an account erases its memberships with everything else keyed to it.

**Blocks are actor records too.** A block is a record of payload type `scatter:v0/block` in the `actors` partition, keyed by the blocked actor, naming the permissions removed (by default `edit` and `createpage`, optionally everything but `read`) and an expiry. It projects as `block/block`, `block/reblock` or `block/unblock`. A block is the only negative rule in the model: an actor's effective permissions are the union of their groups' permissions, minus what their active blocks remove. A subsidiary's permissions are the union over its own groups, minus its own blocks and its operator's, so blocking a person blocks their bots; a block on a subsidiary does not reach its operator ([0024](0024-subsidiary-accounts.md) §3). On a farm with global groups, a tenant account's permissions also include the global groups of the farm account it is publicly linked to, and subtract that farm account's global blocks ([0028](0028-tenancy-policy.md) §3–4). There is no revocation at the group level.

**Blocks on IP addresses** apply to anonymous requests before a temporary account exists. IP addresses are never written to the log ([0007](0007-actor-identity.md) §3), so IP blocks live in the `private` schema ([0013](0013-postgres-storage.md) §4) and appear in `list=blocks` without appearing in the log.

**`owner` is created with the instance,** on the primary tenant, and that group is the instance's `owner`: the one that holds `ts-keys` and `ts-primary`. A tenant created later has an `owner` of its own, which is that wiki's owner and exercises no instance right there ([0046](0046-primary-tenant.md) §1). `instance create` creates the first local user, `local:1`, as the sole member of `owner`, and gives it a binding to the built-in `password` issuer with a password the CLI sets ([0007](0007-actor-identity.md) A3); a primary account holds no token ([0024](0024-subsidiary-accounts.md) §1). On a tenant that adopts an existing Wikibase, `instance create --adopt {source} --owner {user_id}` creates the owner under the source's user ID instead, with the same password binding, and sets the user-ID floor ([0035](0035-adopting-a-wikibase.md) §5). That is the single-user fallback Scatterbase asks for: an instance with no external issuer configured has exactly this one user, who logs in with a password and registers issuers, groups and the sync subsidiaries from there. Membership in `owner` is changed only by a member of `owner`, and the last member cannot be removed.

### 4. ACLs (amends 0005 §4.1)

*Changed by A2, A6, A17.*

An ACL attaches a restriction to a **target**, named by identifier so that the record's key is never content ([0006](0006-log-integrity-and-erasure.md) §3):

| Target kind | Key | Restricts |
|---|---|---|
| `graph` | `acl:graph:{name}` | Writing records to the graph. This replaces the *writers* field of [0005](0005-crate-organization.md) §4.1, which was already an ACL under another name |
| `namespace` | `acl:namespace:{number}` | Actions on every page or entity in the namespace |
| `page` | `acl:page:{page id}` | Actions on one document page and, if the namespace allows subpages, its subpages |
| `entity` | `acl:entity:{id}` | Local assertions about one entity, and its `retain` and `convert` |
| `statement` | `acl:statement:{guid}` | One statement, its qualifiers and references; `read` hides it ([0023](0023-moderation.md) §2) |
| `property` | `acl:property:{id}` | Every snak using the property, as main snak, qualifier or reference, on any entity; enclosure by predicate ([0023](0023-moderation.md) §2) |
| `record` | `acl:record:{partition}:{offset}`, with a `parts` list | Reading the named parts of one record: revision-hiding and suppression ([0023](0023-moderation.md) §2) |
| `actor` | `acl:actor:{key}` | Reading an account's name: username-hiding ([0023](0023-moderation.md) §2) |

An ACL record has the payload type `scatter:v0/acl` ([0023](0023-moderation.md) §3): a graph ACL is appended to the `config` partition, and every other target's ACL is a moderation record in the tenant `log` partition, so that suppressions stay out of public dumps. Its content lists, for each permission it restricts, the group whose members may still perform it, and an expiry. ACLs restrict `read` as well as write permissions, which is what deletion, revision-hiding, suppression and username-hiding are ([0023](0023-moderation.md) §1). Setting an ACL on a page, entity or namespace needs `protect`; on a graph, `ts-config`; a `read` ACL needs `delete`, `deleterevision`, `suppressrevision` or `hideuser` by target ([0023](0023-moderation.md) §11). Page ACLs project as `protect/protect`, `protect/modify` and `protect/unprotect` log events with the parameters [0011](0011-logs.md) §7 already lists.

**Evaluation is conjunctive.** To perform action *A* on target *T*, an actor must hold the permission for *A* after blocks (§3), **and** satisfy every ACL that restricts *A* on *T* or on a target enclosing *T*. Enclosure is fixed: a graph encloses the records written to it; a namespace encloses its pages and entities; a page encloses its subpages; a talk page or board encloses the threads whose **home** it is, not those only listed on it, and the page's `edit` ACL governs what may be listed there ([0019](0019-discussions.md) §12, [0049](0049-boards.md) §7). There is no other inheritance, no cascading protection, and no rule under which a more specific ACL loosens a broader one. This is how MediaWiki protection composes with group rights, and it dissolves Scatterbase's questions about exact matching, inheritance and deny precedence: matching is by target and enclosures, inheritance is enclosure only, and nothing denies except a block.

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

*Changed by A13, A14.*

That a record was erased is public: the gap row of [0010](0010-site-ui.md) §5.2 and the `erased` flag of [0012](0012-api-requirements.md) §4 are shown to everyone, as MediaWiki shows that a revision was deleted. The **reason class** and the **authority reference** of an `erase` record ([0006](0006-log-integrity-and-erasure.md) §7) are shown only to holders of `ts-viewerasures`. The `erase/erase` log event ([0011](0011-logs.md) §6.1) follows the same rule: listed for everyone, with its parameters visible to `ts-viewerasures`. The reason-class vocabulary is `legal`, `privacy`, `upstream` and `operational`; an instance may add classes in `site` configuration. Erasures the instance makes in a tenant's partitions (an operator's expunge, scheduled reclamation of long-deleted file versions) follow the same rule. They are attributed to the instance operator, and their authority is a record in the instance `log`.

### 7. API and UI

**Action API.** Additive, under [0012](0012-api-requirements.md) §1: `meta=siteinfo&siprop=usergroups` lists every group with its permissions, reporting `universe` as `*`; `meta=userinfo&uiprop=rights|groups|blockinfo`; `list=users&usprop=groups|rights|blockinfo`; `action=userrights`, `action=protect`, `action=block` and `action=unblock` as in MediaWiki; `list=blocks` and `list=protectedtitles`; `prop=info&inprop=protection`. Every write module returns `permissiondenied` with the missing permission named. `action=protect` on an entity page sets an `entity` ACL.

**REST.** `GET /acl/{kind}/{id}` and `PUT /acl/{kind}/{id}` under `triplespace/v0` for graph and entity targets, which `action=protect` cannot express fully.

**Site UI** ([0010](0010-site-ui.md)). `Special:ListGroupRights`, `Special:UserRights`, `Special:Block`, `Special:BlockList` and `Special:ProtectedPages` appear where MediaWiki users expect them. The page overflow menu gains **Protect…** for pages and entities. The account settings page (0010 §11) is unchanged, since every action on it is an ownership rule.

### 8. Scatterbase

*Changed by A6.*

Scatterbase inherits §1, §3 and §4 whole: the vocabulary, groups and memberships as records, blocks as the only negative rule, ACLs by target with conjunctive evaluation over enclosures, and the single-user fallback. Its five open points are answered as follows. Matching is exact on the target plus its enclosures. Inheritance is enclosure only. Deny has no precedence because there is no deny; a block is the only subtraction and it applies before ACLs are consulted. A property-specific rule is the `property` target kind ([0023](0023-moderation.md) §2). ACL changes are records, in the `config` partition for graphs and in `log` for moderation ([0023](0023-moderation.md) §3), signed by the server like every record ([0006](0006-log-integrity-and-erasure.md) §1); Scatterbase may make them client-signed claims in its `server` graph without changing their shape.

### 9. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed; the tables are in [0013](0013-postgres-storage.md) §5.6. The table this section first gave is in A1.

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
