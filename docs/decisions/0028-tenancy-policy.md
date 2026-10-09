# 0028. Tenancy policy: what crosses a tenant boundary

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A17)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md), [0023](0023-moderation.md)
- **Uses:** [0024](0024-subsidiary-accounts.md), [0026](0026-sitelinks.md), [0027](0027-preferences-and-portability.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

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

*Changed by A3, A6, A9, A13.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §5.1.*

### 2. Farm identity (extends 0007 §1, §3 and §7; amends 0018 §4)

*Changed by A5, A7, A12, A16.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §2.1, §3.4, §3.5, §3.6, §3.7, §8.6.*

### 3. Global groups (amends 0016 §3)

*Current text: [09](../architecture/09-security-and-moderation.md) §3.4, §3.7.*

### 4. Global blocks and farm-wide actor ACLs (settles 0018 Q3 and 0023 Q8)

*Current text: [09](../architecture/09-security-and-moderation.md) §3.5, §3.7.*

### 5. Reading across tenants (extends 0018 §5; settles 0018 Q2, Q4 and 0020 Q4)

*Changed by A2, A5, A7, A9, A11, A17.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §4.2, §4.4, §6.6.*

### 6. Discussion (settles 0019 Q6)

*Current text: [08](../architecture/08-tenants-and-instances.md) §10.1.*

### 7. Notifications (settles 0021 Q4)

*Current text: [08](../architecture/08-tenants-and-instances.md) §10.2.*

### 8. Shared machinery: templates and locks

*Changed by A4, A5.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §5.3, §5.4, §8.6.*

### 9. Farm-wide feeds and search (extends 0020 §2 and 0014 §7)

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.2, §4.5.*

### 10. Leaving and joining a farm (extends 0018 §10)

*Changed by A7.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §6.5.*

### 11. API and UI (extends 0012 and 0010)

*Changed by A14, A16.*

*Current text: [18](../architecture/18-api.md) §1.2, §2.2, §2.3, §3.2; [19](../architecture/19-site-ui.md) §1.3, §2.4, §6.11.*

### 12. Storage (extends 0013)

*Changed by A10.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.5, §4.6, §5, §12.2, §13.*

### 13. Permissions (extends 0016 §2)

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §2.3, §3.7.*

### 14. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **One instance of Triplespace serves three kinds of farm** by policy, without three codebases and without a bolt-on identity system. The `isolated` preset is the earlier ADRs unchanged.
- **Provenance still travels.** Farm identity adds a public link *to* a person's tenant accounts; it never replaces the tenant account as the actor of an edit, so a tenant that leaves keeps every attribution it ever made.
- **Five open questions close**, and the pattern behind them is now stated: a crossing is a switch, a mechanism behind it is a record, and evaluation reads public links, never private bindings.
- **Global groups cost one join** in permission evaluation, cached per session, and only where a farm has them.
- **The community preset is CentralAuth by another route**: shared names, global groups, global blocks and cross-wiki notifications, with the local account still the actor, which is also what CentralAuth does underneath.
- **A tenant on a farm gives up some autonomy by policy, and knows it**: which switches are on is public in `siprop=triplespace`, and a tenant's operators can read the policy before they join.
- **One more crate and three more instance partitions**, both only where a farm turns them on.

## Open questions

- **Q1.** ~~**Farm-level abuse filters** and other pre-save checks shared across tenants ([0016](0016-permissions-and-access-control.md) Q5), which the `enterprise` and `community` presets would both want.~~ *Settled by [0030](0030-edit-filters.md) §8: the `filters.global` switch; global filters are `scope = global` filters in the instance `log` ([0040](0040-instance-prerogatives.md) §6).*
- **Q2.** ~~**Global groups and ACLs.** Whether a tenant ACL should be able to name a global group, so that a wiki can protect a page to "stewards only".~~ *Settled by [0056](0056-security-model.md) §11: yes, where `groups.global = inherited`; group names are one namespace on a farm, which `tenancy check` verifies, and no global group reads a private tenant unless that tenant's ACL names it.*
- **Q3. Home tenant selection** and whether the aggregated bell should be the default view or a switch.
- **Q4. Name registry at scale**: a unique index over every account name on a farm of thousands of tenants is one large index; whether it should be per farm issuer only, with tenant names checked against it on creation.
- **Q5. Templates for non-config state**, such as a default set of project pages for a new tenant, which are page records rather than configuration.
- **Q6. Reader lists on discussion**, if a provider tenant wants to share data but not threads (today `discussion.share` is all or nothing).
- **Q7. The grace period** for a deleted tenant (§5), and whether its former referrers should be told before erasure.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §14 | extends | 0005 A24 |
| [0007](0007-actor-identity.md) §1, §3, §4, §7, §8 | §2 | extends | 0007 A11 |
| [0013](0013-postgres-storage.md) §5, §5.2, §5.6 | §12 | amends | 0013 A9 |
| [0013](0013-postgres-storage.md) §4, §5.4, §5.5, §7, §8 | §12 | extends | 0013 A9 |
| [0014](0014-caches-and-search.md) §1, §5 | §9, §12 | amends | 0014 A3 |
| [0014](0014-caches-and-search.md) §10 | §9, §12 | extends | 0014 A3 |
| [0015](0015-record-format-and-partition-registry.md) §3, §5 | §1–2, §5, §8 | extends | 0015 A11 |
| [0016](0016-permissions-and-access-control.md) §3 | §3–4 | amends | 0016 A9 |
| [0018](0018-tenants.md) §4 | §2–5 | amends | 0018 A4 |
| [0018](0018-tenants.md) §5 | §2–5 | extends | 0018 A4 |
| [0018](0018-tenants.md) Q2 | §5 | settles | 0018 Q2 |
| [0018](0018-tenants.md) Q3 | §4 | settles | 0018 Q3 |
| [0018](0018-tenants.md) Q4 | §5 | settles | 0018 Q4 |
| [0019](0019-discussions.md) §2 | §6 | extends | 0019 A6 |
| [0019](0019-discussions.md) Q6 | §6 | settles | 0019 Q6 |
| [0020](0020-change-feeds.md) §2, §5 | §5, §9 | extends | 0020 A6 |
| [0020](0020-change-feeds.md) §4 | §5, §9 | amends | 0020 A6 |
| [0020](0020-change-feeds.md) Q4 | §5 | settles | 0020 Q4 |
| [0021](0021-notifications.md) §2 | §7, §11 | extends | 0021 A5 |
| [0021](0021-notifications.md) §7 | §7, §11 | amends | 0021 A5 |
| [0021](0021-notifications.md) Q4 | §7 | settles | 0021 Q4 |
| [0023](0023-moderation.md) Q8 | §4 | settles | 0023 Q8 |

## References

- [Extension:CentralAuth](https://www.mediawiki.org/wiki/Extension:CentralAuth), [Help:Unified login](https://meta.wikimedia.org/wiki/Help:Unified_login) and [Global groups](https://meta.wikimedia.org/wiki/Global_groups)
- [Extension:GlobalBlocking](https://www.mediawiki.org/wiki/Extension:GlobalBlocking)
- [Extension:Echo — cross-wiki notifications](https://www.mediawiki.org/wiki/Help:Notifications/Cross-wiki)
- [Miraheze: Stewards](https://meta.miraheze.org/wiki/Stewards) and [Global Sysops](https://meta.miraheze.org/wiki/Global_Sysops)
- [0018 — Tenants](0018-tenants.md), `docs/registry/tenancy.toml`

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §14
- **Summary:** 0005 §2 is the one crate table CI checks, and carries `scatter-adapter-triplespace` and every change this section listed (0005 A24).

Replaced text (§14):

> | Layer | Crate | Change |
> |---|---|---|
> | Substrate | `scatter-actors` | Evaluation over a linked farm account (global groups, global blocks); template and lock checks; the tenancy policy type and preset validation. Embeds `docs/registry/tenancy.toml` |
> | | `scatter-log` | The `tenancy`, `template` and `provider-readers` config kinds; `scope` on `group` |
> | Ingest | `scatter-adapter-triplespace` *(new)* | Reading a Triplespace provider on another instance: dump bootstrap, then the activity stream as the version cursor (§5) |
> | Triplespace | `triplespace-accounts` | Farm accounts, the name registry, auto-creation and auto-linking, cascading rename and vanish (§2); the aggregated inbox join for `triplespace-notify` (§7) |
> | | `triplespace-projections` | The all-tenants target set (§9); reader lists at partition open (§5); cross-tenant thread reads (§6) |
> | | `triplespace-search` | Farm-wide `msearch` (§9) |
> | | `triplespace-api-action`, `triplespace-api-rest` | §11 |
> | | `triplespace-cli` | `instance create --tenancy`, `tenancy check`, `tenancy set` |
>
> The workspace goes from thirty-nine crates to forty.

### A2. The adapter verifies the local graph

- **Date:** 2026-09-27
- **Source:** [0022](0022-federation.md) §2
- **Change:** amends §5
- **Summary:** The adapter bootstraps from the provider's local-graph source dump rather than its Wikibase-compatible dump, follows the stream filtered to `source = local`, rewrites references to the reader's own entities back to bare IDs, and verifies each batch against the provider's checkpoint and key chain unless the provider is registered `trust = stream`.

Replaced text (§5):

> It bootstraps from the provider's Wikibase-compatible dump ([0013](0013-postgres-storage.md) §8) and then follows the provider's activity stream ([0020](0020-change-feeds.md) §4) with `Last-Event-ID` as its version cursor,

### A3. Farm-wide filters

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §8
- **Change:** extends §1
- **Summary:** The `filters.global` switch; global filters run on every tenant before its own. §1 had been given the row in place. This settled Q1.

### A4. Takedowns are an operator rule

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §10
- **Change:** extends §8
- **Summary:** File takedowns and expunges are a second operator rule every preset has: they apply to every tenant, no tenant can override them, and they are made through the primary tenant or a global group.

### A5. Instance acts

- **Date:** 2026-09-30
- **Source:** [0040](0040-instance-prerogatives.md) §5–6
- **Change:** amends §2, §5, §8
- **Summary:** By section:
  - §2: The actor records a farm-account rename or vanish writes into each linked tenant are instance prerogatives: attributed to the instance operator, signed by the instance key, citing the farm account's record in `actors/{farm}` as authority. A tenant bureaucrat cannot rename a linked account away from its farm name while shared names are on.
  - §5: Erasing a deleted tenant's partitions is an instance prerogative, carried out by a job whose record in the instance `log` is its authority.
  - §8: Records written into a tenant from a template are instance acts: under `defaults` they are **provisions**, which the tenant may replace; under `locks` they are **prerogatives**, and `ts-locked` is a case of `ts-prerogative`.

Replaced text (§5):

> After the grace period the partitions are erased.

### A6. The wikitext ceiling

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §2
- **Change:** extends §1
- **Summary:** A switch `wikitext.ceiling` (`subset`, `expansion`, `lua`; `lua` in every preset) caps how much of template expansion and Lua a tenant may turn on.

### A7. The primary tenant

- **Date:** 2026-09-30
- **Source:** [0046](0046-primary-tenant.md) §5–7
- **Change:** amends §2, §5, §10
- **Summary:** By section:
  - §2: The **farm slug** is the instance's own slug: chosen at `instance create`, distinct from every tenant slug (the primary tenant's included) and never changed. Farm account IRIs are `{farm base}/instance/user/{id}`, and the farm partitions' graph IRIs are under `{farm base}/instance/graph/`.
  - §5: The primary tenant cannot be deleted until the primary role has been transferred (`ts-primary-tenant`).
  - §10: The farm account IRI is `{farm base}/instance/user/{id}`.

Replaced text (§2):

> the instance registers an issuer whose code is the **farm slug** ([0018](0018-tenants.md) §1), actor model `numeric`, `login = true`.

Replaced text (§10):

> the farm account IRI `{farm base}/user/{id}` still resolves,

### A8. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–14
- **Summary:** A1–A7 were folded into the Decision. The open questions were numbered. §12's "`NULL` tenant" reads as the empty-string tenant, which 0013 A11 decided for every earlier ADR. No decision changed. Before this, A2 and A4–A7 were blockquotes, and A3 had been written in place. The file before conversion is commit `0b26a3a`.

### A9. `security.restrictions`, and cross-tenant reads are anonymous

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §6, §11
- **Change:** extends §1; amends §5
- **Summary:** The policy gains `security.restrictions` (`none`, `tenant`, `any`; `any` in `isolated` and `enterprise`, `none` in `community`), which decides whether tenants may restrict `read` beyond moderation. A tenant reading a provider is the provider's anonymous principal and sees its public form, so a provider may hold confidential entities and a private tenant provides nothing. Q2 is settled: an ACL may name a global group where the policy has them.

Replaced text (§5):

> Reading is all or nothing; a provider does not restrict individual entities, since its `local` graph is public data by 0018 §5's definition.

### A10. The query store

- **Date:** 2026-10-04
- **Source:** [0059](0059-query-service.md) §4
- **Change:** extends §12
- **Summary:** One query store per instance, every public tenant's graphs under their names, tenant isolation by dataset at query time; `store` isolation per tenant as an embedded store, selected by the `isolated` preset. Private tenants are in no store.

### A11. `provider-readers` is tenant scope

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5
- **Summary:** `provider-readers` is **tenant** scope, a record in the provider tenant's own `config` ([0015](0015-record-format-and-partition-registry.md) §3, §5); the 0028 row of [0013](0013-postgres-storage.md) §5.6 says so instead of listing it beside the instance-level `tenancy` and `template` kinds. The row's verb is `corrects`, but §5 already places the record in the provider tenant's own `config` and nothing in this ADR says otherwise, so here it is an extension that states the scope outright; the correction lands in 0013. (PENDING A5)

### A12. The farm partitions are the instance's

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §2
- **Summary:** A tenant has one `source/{name}` partition per entity source ([0078](0078-entity-sources.md) §4) beside the partitions [0018](0018-tenants.md) §2 lists, and the instance has the farm partitions of §2, `actors/{farm}`, `accounts/{farm}` and `log/{farm}`; 0018 §2's fixed count of six partitions goes, and §2's three instance partitions are part of the count 0018 §2 now gives. (PENDING C6)

### A13. `query.isolation` is a tenancy switch

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §1
- **Summary:** `query.isolation` is a row of §1's switch table: `isolated = store`, the shared presets (`community`, `enterprise`) `dataset`. `store` with a `remote` query backend is refused only on a farm; on a single-tenant instance the remote store holds one tenant's graphs and satisfies `store` ([0059](0059-query-service.md) §4). The row's verb is `amends`, but §1's table merely lacks the row that §12 already refers to, and no sentence of this ADR contradicts it, so here it is an extension. (PENDING C9)

### A14. The farm-base global interface is the Action API

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §11
- **Summary:** The farm-base global group and block interface is the Action API: `action=globalblock`, `list=globalblocks` and the global-group modules. No REST routes are claimed from [0016](0016-permissions-and-access-control.md) §7; the farm-base REST routes are `GET /tenancy`, `GET /farm/users/{id}` and `GET /activity?set=farm` only. (PENDING E37)

Replaced text (§11):

> - **REST**, at the farm base: `GET /tenancy` (the policy), `GET /farm/users/{id}` (a farm account and its linked tenant accounts, public), `GET /activity?set=farm`, and the global group and block routes mirroring 0016 §7's.

### A15. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§14
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A16. The farm issuer's code is the farm code

- **Date:** 2026-10-09
- **Source:** [0079](0079-derived-issuer-codes.md) §4, §7
- **Change:** amends §2; extends §11
- **Summary:** The farm issuer's code is the farm code derived from the instance's founding record ([0079](0079-derived-issuer-codes.md) §4), so farm accounts are `{farm code}:{id}` (§2). `GET /tenancy` includes the farm code, and its `tenants` array gives each tenant's issuer code as `issuer` (§11).

Replaced text (§2, in [08](../architecture/08-tenants-and-instances.md) §3.5):

> With `identity.farm_issuer` other than `none` (§5.1), the instance registers an issuer whose code is the **farm slug** (§1.2);

### A17. Who may read whom, by issuer code

- **Date:** 2026-10-09
- **Source:** [0080](0080-tenants-as-entity-sources.md) §2, §5
- **Change:** amends §5
- **Summary:** `providers.between_tenants` governs entity sources that name a tenant of the same instance: under `operator` only the operator may write one (or set `files.share` and `pages.share`); under `opt-in` any tenant may name any public tenant of the instance. A source naming a tenant elsewhere is an ordinary entity source. `provider-readers` lists name tenants by issuer code, since the record travels with the provider. The Triplespace adapter writes a tenant source into the reader's `source/{name}` partition, after checking the provider's founding record against the source's code.

Replaced text (§5, in [08](../architecture/08-tenants-and-instances.md) §4.2):

> **Who may read whom.** With `providers.between_tenants = operator`, only the farm operator can make a tenant a provider, by allocating it a code ([0015](0015-record-format-and-partition-registry.md) §5) and writing its `tenant` record; the hosting case, where tenants have no business reading each other unless the operator says so. With `opt-in`, a tenant that has a code is readable by any tenant that lists it in `providers`, which is what §4.1 describes.
>
> … `mode` (`allow` or `deny`) and a list of tenant slugs, …

Replaced text (§5, in [08](../architecture/08-tenants-and-instances.md) §4.4):

> …writing `put`, `redirect` and `tombstone` records into a `mirror/{provider}` partition as any adapter does.
