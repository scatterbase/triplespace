# 0028. Tenancy policy: what crosses a tenant boundary

- **Status:** Proposed
- **Date:** 2026-09-27
- **Author:** James Hare / Claude Fable
- **Amended by:** [0030 — Edit filters](0030-edit-filters.md) (§8 extends §1 with `filters.global`; settles the farm-filter open question), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§2 refines §5: the cross-instance adapter reads the provider's local graph only and verifies by default; §9 follows §8)
- **Related:** [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§14 amends §2: adds `scatter-adapter-triplespace`), [0007 — Actor identity](0007-actor-identity.md) (§2 extends §1, §3 and §7: the farm as an issuer; accounts created and linked from it), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§12), [0014 — Cache layers and search](0014-caches-and-search.md) (§9 extends §7: farm-wide search), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (§1 and §8 extend §3 with the `tenancy`, `template` and global `group` kinds; §2 extends §5 with `tenancy.toml` and three instance partitions), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§3–4 amend §3: global groups and global blocks enter effective-permission evaluation), [0018 — Tenants](0018-tenants.md) (§2 amends §4; §5 extends §5; settles the cross-tenant ACL, global block and deletion-referrer open questions), [0019 — Discussions](0019-discussions.md) (§6 settles the cross-tenant discussion open question), [0020 — Change feeds](0020-change-feeds.md) (§5 settles provider-to-tenant sync over the stream; §9 extends §2 with the all-tenants target set), [0021 — Notifications](0021-notifications.md) (§7 settles the cross-tenant notification open question), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§4 settles the cross-tenant actor ACL open question), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§2), [0026 — Sitelinks are URLs](0026-sitelinks.md) (§3: the instance deny list as one instance of §8), [0027 — Preferences, private state and portability](0027-preferences-and-portability.md) (§10 follows §4: private state on leaving a farm)

## Context

[0018](0018-tenants.md) made a tenant a wiki with its own partitions, identity, configuration and users, and made a farm an instance that hosts several. It fixed what is shared (mirrors, the instance configuration, sync jobs) and what is not (everything else), and it left five questions about crossing the boundary: cross-tenant ACLs and global blocks (0018), cross-tenant discussion ([0019](0019-discussions.md)), cross-tenant notifications ([0021](0021-notifications.md)), provider-to-tenant sync over the stream ([0020](0020-change-feeds.md)), and whether an actor ACL by the primary tenant should apply farm-wide ([0023](0023-moderation.md)).

James's observation is that these are not five questions but one, and that its answer is **policy for the instance**, because shared tenancy happens for different reasons:

| Deployment | Who the tenants are to each other | What should cross |
|---|---|---|
| **Hosting** | Unrelated customers of one operator | As little as possible. Sharing creates a moderation burden the operator does not want; a spam blocklist is about the limit |
| **Community farm** (Miraheze) | Local communities under a farm-wide community of volunteers, with one username database | Identity, global groups such as stewards, global blocks, cross-wiki notifications and recent changes; tenants otherwise autonomous |
| **Enterprise** | Departments or subsidiaries of one organisation | Identity from the corporate directory, platform administrators, shared configuration that some tenants may not change, reading and searching across tenants |

MediaWiki's answer to the second is CentralAuth: one global account, local accounts attached to it on every wiki, global groups, global blocks, cross-wiki notifications. Its answer to the first and third is configuration. Triplespace can do better than a bolt-on, because [0018](0018-tenants.md) §4 already made identity per tenant with an opt-in link between accounts ([0007](0007-actor-identity.md) §7), and [0016](0016-permissions-and-access-control.md) made every permission decision a function of records. What is missing is a place to say which crossings an instance allows, and a small number of mechanisms behind the switches.

## Decision

### 1. The tenancy policy is instance configuration, with three presets

The **tenancy policy** is a set of switches, each a `config` record of kind `tenancy` in the instance `config` partition ([0015](0015-record-format-and-partition-registry.md) §3), keyed `tenancy:{switch}`. Three **presets** ship in `docs/registry/tenancy.toml` and are what `triplespace-cli instance create --tenancy {preset}` writes; every switch can then be changed on its own with `ts-config` at the farm base, except the identity switches, which need `owner` because they are hard to reverse (§2). A single-tenant instance is the `isolated` preset and never notices any of this.

| Switch | Values | `isolated` | `community` | `enterprise` |
|---|---|---|---|---|
| `identity.farm_issuer` | `none`, `optional`, `required` | `none` | `required` | `optional` |
| `identity.required_issuer` | an issuer code, or none: every tenant account must bind to it | none | none | the corporate identity provider |
| `identity.shared_names` | `off`, `on` | `off` | `on` | `on` |
| `identity.auto_link` | `off`, `on` | `off` | `on` | `on` |
| `groups.global` | `none`, `inherited` | `none` | `inherited` | `inherited` |
| `blocks.global` | `ip`, `farm-actor` | `ip` | `farm-actor` | `farm-actor` |
| `providers.between_tenants` | `operator`, `opt-in` | `operator` | `opt-in` | `opt-in` |
| `providers.reader_lists` | `off`, `on` | `off` | `on` | `on` |
| `discussion.cross_tenant` | `off`, `read-only` | `off` | `read-only` | `read-only` |
| `notifications.cross_tenant` | `off`, `home` | `off` | `home` | `home` |
| `config.template` | `none`, `defaults`, `locks` | `none` | `defaults` | `locks` |
| `feeds.farm_wide` | `off`, `global-groups`, `everyone` | `off` | `global-groups` | `everyone` |
| `search.farm_wide` | `off`, `global-groups`, `everyone` | `off` | `off` | `everyone` |
| `filters.global` | `none`, `inherited` ([0030](0030-edit-filters.md) §8) | `none` | `inherited` | `inherited` |

Every switch that says "farm actor" or "global" requires `identity.farm_issuer` to be at least `optional`; the registry rejects a preset or a record that does not satisfy that. `meta=siteinfo&siprop=triplespace` reports the preset and the switches, so a client and the UI know what exists.

### 2. Farm identity (extends 0007 §1, §3 and §7; amends 0018 §4)

**The farm is an issuer.** With `identity.farm_issuer` other than `none`, the instance registers an issuer whose code is the **farm slug** ([0018](0018-tenants.md) §1), actor model `numeric`, `login = true`. Its accounts are **farm accounts**: actors `{farm}:{id}` with names, kinds and statuses like any account ([0007](0007-actor-identity.md) §4), held in three **instance** partitions added to [0018](0018-tenants.md) §2 under the per-issuer names 0015 §5 already reserves: `actors/{farm}` (`logged`, `full`, internal), `accounts/{farm}` (`logged`, `full`, private) and `log/{farm}` (`logged`, `full`, internal). A farm account authenticates through bindings to identity providers exactly as a tenant account does ([0007](0007-actor-identity.md) §3); the farm has no passwords of its own. **A farm account edits nothing.** It is a person's identity across the farm, not an actor on any tenant's data, and [0018](0018-tenants.md) §4's rule stands: every record on a tenant is attested by that tenant's actor, and instance-level records by the primary tenant's.

**Shared names.** With `identity.shared_names = on`, the farm keeps one **name registry**: a name held by any farm account or any tenant account is reserved farm-wide, and a tenant account can be created only under the name of the farm account creating it. A person who signs up to the farm chooses a name once. On first visit to a tenant that admits them, the tenant creates `{tenant}:{n}` under that name, binds it to the farm subject in the tenant's `accounts` partition, and, with `identity.auto_link = on`, appends the public `link-account` record of [0007](0007-actor-identity.md) §7 between the tenant account and the farm account. The consent 0007 §7 requires is given once, at farm signup, where the form says in plain words that accounts created on the farm's wikis will be publicly linked to the farm account. A person who declines has no farm account and, under the `community` preset, no account at all.

What this preserves: `librarybase:42` is still the actor of every edit made on Librarybase, on every instance that ever holds it ([0018](0018-tenants.md) §4). What it adds: `librarybase:42` and `example:17` are publicly the accounts of `{farm}:123`, so a farm can act on the person rather than on each account.

**Renames and vanishing cascade.** Renaming a farm account renames every linked tenant account in one batch of actor records, one per tenant, as [0008](0008-namespaces-and-document-pages.md) §6 already batches page moves. Vanishing a farm account vanishes each linked account under [0007](0007-actor-identity.md) §4's procedure and then the farm account. Subsidiaries ([0024](0024-subsidiary-accounts.md)) belong to tenant accounts and are unaffected: the operator relation is per tenant, and there is still no cross-tenant bot.

**Turning shared names on later** is refused while any two tenants hold the same name for accounts that are not linked to one farm account, or any tenant name collides with a farm name. `triplespace-cli tenancy check` lists the collisions; each is resolved by renaming or linking. This is why the identity switches need `owner`: the check is the cost of a farm that grew before it decided to be one.

### 3. Global groups (amends 0016 §3)

With `groups.global = inherited`, the instance `config` may hold `group` records with `scope = global`. Their memberships are `membership` records in `actors/{farm}` keyed by the **farm account**, written with `userrights` by members of a global group that holds it, and projected as `rights/rights` in `log/{farm}`.

**Evaluation** ([0016](0016-permissions-and-access-control.md) §3) on tenant *T* for a session gains one term: the effective permissions of a tenant account are the union over its own groups **and over the global groups of the farm account it is publicly linked to**, minus its own blocks, its operator's ([0024](0024-subsidiary-accounts.md) §3) and the farm account's (§4). The join runs over `view.account_link` ([0013](0013-postgres-storage.md) §5.4), which is public, so no private data enters evaluation. **A tenant cannot exclude a global group**, as a Wikimedia wiki cannot exclude stewards; the presets that enable them are the ones where that is wanted, and an instance that wants otherwise sets `none`. Global groups are subject to ACLs like anyone: an ACL restricting `edit` to a tenant's `sysop` is not satisfied by a global group unless the ACL names it.

The `community` preset ships two: `steward` (`userrights`, `block`, `hideuser`, `ts-config`, `renameuser`, `deletedhistory`, `viewsuppressed`) and `global-sysop` (the tenant `sysop` set). The `enterprise` preset ships `platform-admin` with `steward`'s set. All are in `tenancy.toml`.

### 4. Global blocks and farm-wide actor ACLs (settles 0018 and 0023)

With `blocks.global = farm-actor`, a `block` record ([0016](0016-permissions-and-access-control.md) §3) in `actors/{farm}` keyed by a farm account removes permissions from **every tenant account linked to it**, through the same join as §3, and prevents the farm account from creating accounts on further tenants. It projects as `block/block` in `log/{farm}` and is listed on `Special:GlobalBlockList` at the farm base. A tenant's own blocks are unaffected and still apply to its accounts alone.

Under `blocks.global = ip`, farm-wide blocking is by IP address only, in `private` as [0016](0016-permissions-and-access-control.md) §3 has it, applied to every tenant's anonymous requests and account creations. Every preset has that much, because a hosting operator needs it too.

**Actor ACLs** ([0023](0023-moderation.md) §5) on a farm account hide its name on every tenant, since every linked account carries the same name by construction (§2). An actor ACL written by a tenant on its own account hides that account only. This settles 0023's open question: farm-wide hiding is a property of farm identity, not of the primary tenant.

### 5. Reading across tenants (extends 0018 §5; settles 0020's stream question)

**Who may read whom.** With `providers.between_tenants = operator`, only the farm operator can make a tenant a provider, by allocating it a code ([0015](0015-record-format-and-partition-registry.md) §5) and writing its `tenant` record; the hosting case, where tenants have no business reading each other unless the operator says so. With `opt-in`, a tenant that has a code is readable by any tenant that lists it in `providers` ([0018](0018-tenants.md) §3), which is what 0018 §5 already describes.

**Reader lists.** With `providers.reader_lists = on`, a provider tenant may restrict who reads it with a `config` record of kind `provider-readers` in its own `config`: `mode` (`allow` or `deny`) and a list of tenant slugs, in the shape of [0026](0026-sitelinks.md) §3's sitelink lists. A tenant not admitted sees the provider as if it had no code. This settles 0018's cross-tenant ACL question without a new ACL kind: it is a list, evaluated where the reading tenant's projections open the provider's partition. Reading is all or nothing; a provider does not restrict individual entities, since its `local` graph is public data by 0018 §5's definition.

**Across instances.** A provider tenant on another instance, or one that has moved away ([0018](0018-tenants.md) §10), is read by a **Triplespace adapter**, `scatter-adapter-triplespace`, added to the ingest layer ([0005](0005-crate-organization.md) §2). It bootstraps from the provider's Wikibase-compatible dump ([0013](0013-postgres-storage.md) §8) and then follows the provider's activity stream ([0020](0020-change-feeds.md) §4) with `Last-Event-ID` as its version cursor, writing `put`, `redirect` and `tombstone` records into a `mirror/{provider}` partition as any adapter does. This settles 0020's question: same-instance reading is direct ([0018](0018-tenants.md) §5), cross-instance reading is a sync over the stream, and a tenant that leaves the instance is switched from the first to the second with no change in what its readers see.

> **Amended by [0022](0022-federation.md) §2.** The adapter bootstraps from the provider's local-graph source dump rather than its Wikibase-compatible dump, follows the stream filtered to `source = local`, rewrites references to the reader's own entities back to bare IDs, and verifies each batch against the provider's checkpoint and key chain unless the provider is registered `trust = stream`.

**Deleting a tenant** (0018, open) is now the same as moving one away without a destination: its partitions are retained read-only for a grace period set in `site` configuration, its providers switch nothing (there is no new home), and its former referrers see its entities as they would see an upstream that has gone silent: the last mirrored state, with the identity line saying the provider is gone. After the grace period the partitions are erased.

### 6. Discussion (settles 0019)

With `discussion.cross_tenant = read-only`, the talk page of an entity mirrored from a provider tenant shows, beneath the local threads, the **provider tenant's threads about that entity**, read-only, with a link to reply there, provided the provider tenant's `discussion.share` setting (its `site` configuration, default on under this switch) allows it. The threads are read through the same partition access as the entity data ([0018](0018-tenants.md) §5), never copied, and never fed into the reading tenant's feeds, search or notifications. Threads about entities the reading tenant does not mirror are not shown. Under `off`, [0019](0019-discussions.md) §2's rule stands unchanged: a mirrored entity's talk page holds local threads only.

### 7. Notifications (settles 0021)

With `notifications.cross_tenant = home`, a person's **bell aggregates the inboxes of every tenant account linked to their farm account**. The join is made by `triplespace-notify` under the accounts role, over the public link table and the private inboxes the person owns, and is what Echo's cross-wiki notifications are. A mention that resolves to a **farm account**, because the person has no account on the mentioning tenant or because the mentioner wrote the farm name, is delivered to the inbox of the person's **home tenant**: a farm preference ([0027](0027-preferences-and-portability.md) §1, key `farm.home_tenant`, default the tenant where the farm account was created). Cross-tenant delivery for the `talk`, `reply`, `watch`, `rights` and `job` reasons needs nothing new, since each fires on the tenant that holds the account. Under `off`, [0021](0021-notifications.md) §2's rule stands: only a tenant's own accounts have inboxes there, and a mention of `librarybase:42` on another tenant addresses nobody.

### 8. Shared machinery: templates and locks

With `config.template = defaults`, the instance `config` may hold records of kind `template`, keyed `template:{kind}:{code}`, each holding a tenant-config record (namespaces, groups, `sitelink-policy`, `thread-status`, `site` settings, `view-pin`, and any other tenant kind of [0015](0015-record-format-and-partition-registry.md) §3) that `tenant create` writes into a new tenant's `config`. A tenant may then change them. With `locks`, the instance also holds `tenancy:locked`, a list of `{kind}:{code}` keys; a tenant `config` record for a locked key is refused with `ts-locked`, and changing a locked template re-writes it into every tenant. The instance sitelink deny list ([0026](0026-sitelinks.md) §3) is the one such rule every preset has; templates and locks generalise it to any setting an enterprise wants uniform.

### 9. Farm-wide feeds and search (extends 0020 §2 and 0014 §7)

With `feeds.farm_wide` other than `off`, the feed model of [0020](0020-change-feeds.md) gains a target set, **all tenants**: the union of every tenant's everything set, each row carrying its tenant, served at the farm base as `Special:GlobalRecentChanges`, `?feed=atom` and `/activity/stream?set=farm`, with a tenant filter. `global-groups` limits it to members of a global group, which is what support volunteers use; `everyone` opens it. With `search.farm_wide`, `/suggest` and `list=search` at the farm base run one `msearch` across every tenant's index and every provider index ([0014](0014-caches-and-search.md) §7), with each hit carrying its tenant; the same two gates apply. Redaction is per viewer on the tenant each row belongs to, so a deletion on one tenant hides the row for everyone outside that tenant's deletion group, whatever farm-wide view it appears in.

### 10. Leaving and joining a farm (extends 0018 §10)

When a tenant **leaves** a farm with farm identity, its accounts keep their names and their public links, which now point at farm accounts on another instance: the farm account IRI `{farm base}/user/{id}` still resolves, and the link is what [0007](0007-actor-identity.md) §7 calls a foreign account link. Global groups and global blocks stop applying the moment the tenant is registered elsewhere. Bindings do not travel ([0018](0018-tenants.md) §10) and reclaiming proceeds as there; the farm may act as the vouching identity provider, which is the natural case.

When a tenant **joins** a farm with shared names, its accounts are checked against the farm's name registry (§2). Colliding names are renamed by the joining tenant's bureaucrats, or the person proves both are theirs and they are linked. Only then is the tenant's identity switched on.

### 11. API and UI (extends 0012 and 0010)

- **Action API**: `meta=siteinfo&siprop=triplespace` reports the tenancy preset and switches; CentralAuth's `meta=globaluserinfo` and `list=globalallusers` are offered at the farm base with their meaning, because tools and the Wikipedia apps use them; `action=globalblock` and `list=globalblocks`; `meta=notifications` gains `notwikis` with its real meaning under `notifications.cross_tenant = home` ([0021](0021-notifications.md) §7 accepted and ignored it).
- **REST**, at the farm base: `GET /tenancy` (the policy), `GET /farm/users/{id}` (a farm account and its linked tenant accounts, public), `GET /activity?set=farm`, and the global group and block routes mirroring 0016 §7's.
- **UI**: `Special:Tenancy` for the policy (owner and `ts-config`); `Special:GlobalUsers`, `Special:GlobalGroupMembership`, `Special:GlobalBlock`, `Special:GlobalBlockList`, `Special:GlobalRecentChanges` at the farm base; the account page shows the farm account and every linked tenant account under **Linked accounts** ([0010](0010-site-ui.md) §11), labelled Public; a farm signup page states the linking consent of §2; the identity line of a mirrored provider-tenant entity is unchanged from [0018](0018-tenants.md) §11.

### 12. Storage (extends 0013)

- Three instance partitions, `actors/{farm}`, `accounts/{farm}`, `log/{farm}`, as child tables like any partition; farm accounts are rows in `view.actor` with the farm issuer, and their memberships and blocks in the tables [0016](0016-permissions-and-access-control.md) §9 already has, with a `NULL` tenant.
- The **name registry** is a unique index over the names of farm accounts and of every tenant's accounts, maintained by the actor projection when `identity.shared_names` is on; it is the farm-wide form of `actor_local_name` ([0013](0013-postgres-storage.md) §5.4).
- Evaluation joins `view.account_link` to reach a tenant account's farm account (§3–4); L0 caches the result per session ([0014](0014-caches-and-search.md) §2).
- `view.registry` holds the `tenancy`, `template`, `provider-readers` and global `group` kinds.
- Nothing new in `private`: farm bindings are bindings; the aggregated bell reads existing inboxes.

### 13. Permissions (extends 0016 §2)

| Permission | Governs | Default groups |
|---|---|---|
| `ts-config` | Every `tenancy` switch except identity; `template`, locks, `provider-readers` | `bureaucrat` (tenant), the farm's `steward` or `platform-admin` (instance) |
| `ts-keys` | Also the identity switches (§2), since they are as consequential as the key | `owner` |
| `userrights`, `block`, `hideuser` | Global memberships, blocks and actor ACLs when held through a global group | as [0016](0016-permissions-and-access-control.md) §2, in the global groups of §3 |

### 14. Crates (amends 0005 §2)

| Layer | Crate | Change |
|---|---|---|
| Substrate | `scatter-actors` | Evaluation over a linked farm account (global groups, global blocks); template and lock checks; the tenancy policy type and preset validation. Embeds `docs/registry/tenancy.toml` |
| | `scatter-log` | The `tenancy`, `template` and `provider-readers` config kinds; `scope` on `group` |
| Ingest | `scatter-adapter-triplespace` *(new)* | Reading a Triplespace provider on another instance: dump bootstrap, then the activity stream as the version cursor (§5) |
| Triplespace | `triplespace-accounts` | Farm accounts, the name registry, auto-creation and auto-linking, cascading rename and vanish (§2); the aggregated inbox join for `triplespace-notify` (§7) |
| | `triplespace-projections` | The all-tenants target set (§9); reader lists at partition open (§5); cross-tenant thread reads (§6) |
| | `triplespace-search` | Farm-wide `msearch` (§9) |
| | `triplespace-api-action`, `triplespace-api-rest` | §11 |
| | `triplespace-cli` | `instance create --tenancy`, `tenancy check`, `tenancy set` |

The workspace goes from thirty-nine crates to forty.

## Consequences

- **One instance of Triplespace serves three kinds of farm** by policy, without three codebases and without a bolt-on identity system. The `isolated` preset is the earlier ADRs unchanged.
- **Provenance still travels.** Farm identity adds a public link *to* a person's tenant accounts; it never replaces the tenant account as the actor of an edit, so a tenant that leaves keeps every attribution it ever made.
- **Five open questions close**, and the pattern behind them is now stated: a crossing is a switch, a mechanism behind it is a record, and evaluation reads public links, never private bindings.
- **Global groups cost one join** in permission evaluation, cached per session, and only where a farm has them.
- **The community preset is CentralAuth by another route**: shared names, global groups, global blocks and cross-wiki notifications, with the local account still the actor, which is also what CentralAuth does underneath.
- **A tenant on a farm gives up some autonomy by policy, and knows it**: which switches are on is public in `siprop=triplespace`, and a tenant's operators can read the policy before they join.
- **One more crate and three more instance partitions**, both only where a farm turns them on.

## Open questions

- ~~**Farm-level abuse filters** and other pre-save checks shared across tenants ([0016](0016-permissions-and-access-control.md), open), which the `enterprise` and `community` presets would both want.~~ *Settled by [0030](0030-edit-filters.md) §8: the `filters.global` switch; global filters live in the primary tenant's `log` with `scope = global`.*
- **Global groups and ACLs.** Whether a tenant ACL should be able to name a global group, so that a wiki can protect a page to "stewards only".
- **Home tenant selection** and whether the aggregated bell should be the default view or a switch.
- **Name registry at scale**: a unique index over every account name on a farm of thousands of tenants is one large index; whether it should be per farm issuer only, with tenant names checked against it on creation.
- **Templates for non-config state**, such as a default set of project pages for a new tenant, which are page records rather than configuration.
- **Reader lists on discussion**, if a provider tenant wants to share data but not threads (today `discussion.share` is all or nothing).
- **The grace period** for a deleted tenant (§5), and whether its former referrers should be told before erasure.

## References

- [Extension:CentralAuth](https://www.mediawiki.org/wiki/Extension:CentralAuth), [Help:Unified login](https://meta.wikimedia.org/wiki/Help:Unified_login) and [Global groups](https://meta.wikimedia.org/wiki/Global_groups)
- [Extension:GlobalBlocking](https://www.mediawiki.org/wiki/Extension:GlobalBlocking)
- [Extension:Echo — cross-wiki notifications](https://www.mediawiki.org/wiki/Help:Notifications/Cross-wiki)
- [Miraheze: Stewards](https://meta.miraheze.org/wiki/Stewards) and [Global Sysops](https://meta.miraheze.org/wiki/Global_Sysops)
- [0018 — Tenants](0018-tenants.md), `docs/registry/tenancy.toml`
