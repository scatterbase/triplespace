# 0046. The primary tenant

- **Status:** Proposed
- **Date:** 2026-09-30
- **Author:** James Hare / Claude Opus
- **Related:** [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§10 amends §2), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (§7 amends §6: origin lines of instance partitions), [0007 — Actor identity](0007-actor-identity.md) (§7 amends the IRIs of the farm issuer and the operator actor), [0011 — Upstream and local logs](0011-logs.md) (§4 amends §6.3: mirror sync job records are in the instance `log`), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§2 extends §5: `primary` is an instance-level kind), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (§2 extends §3: the `primary` config kind; the `tenant` record loses its primary flag), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§1 refines §3: `owner` on the primary tenant is the instance's; §8 extends §2: `ts-primary`), [0018 — Tenants](0018-tenants.md) (§1 amends §1: what the primary tenant is; §2 amends §3; §5 amends §10: the primary tenant cannot move away; §7 amends §2: instance graph IRIs), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§5 refines §1: instance-job subsidiaries follow the primary role), [0025 — The instance as an OAuth server](0025-oauth-server.md) (§4 amends §5: consumer events are in the instance `log`; §8 amends §10: `mwoauthmanageconsumer` is an instance right), [0028 — Tenancy policy](0028-tenancy-policy.md) (§6 settles §2's undefined farm slug; §5 amends §5: the primary tenant cannot be deleted; §7 amends §2 and §10: farm account IRIs), [0040 — Instance prerogatives](0040-instance-prerogatives.md) (§1 refines §4: "an actor of the primary tenant" means primary when the record is appended; §7 amends §2: the operator IRI; §8 amends §9: the instance-rights list)

## Context

[0018](0018-tenants.md) §1 introduced the primary tenant in two sentences: "Its actors attest instance-level records (§3, §4). On a single-tenant instance that is the only tenant; on a farm it is whichever tenant the operators run the farm from." Since then nine ADRs have given it work to do, each in passing:

| Where | What the primary tenant does |
|---|---|
| [0016](0016-permissions-and-access-control.md) §3 | `instance create` makes `local:1` the sole member of `owner`; `ts-keys` is `owner`'s alone |
| [0018](0018-tenants.md) §3, §11 | Its actors attest instance-level records; mirror sync jobs are attested by them |
| [0024](0024-subsidiary-accounts.md) §1 | The instance's sync and ingest jobs run as subsidiaries of its operators |
| [0025](0025-oauth-server.md) §5 | OAuth consumer events are written to its `log` |
| [0030](0030-edit-filters.md) §8 | Global filters were kept in its `log` (moved by 0040 §6) |
| [0039](0039-files-and-media.md) §21 | `ts-takedown`, `ts-expunge` and `ts-viewtakedown` are evaluated on it |
| [0040](0040-instance-prerogatives.md) §2, §4, §9 | Operators act from it; authority records are attested by its actors; instance rights are evaluated on it; the operator's display name defaults to its site name |

And twice an ADR has said what it is *not*: [0028](0028-tenancy-policy.md) §4 (farm-wide actor ACLs are "a property of farm identity, not of the primary tenant") and [0040](0040-instance-prerogatives.md) §1 (the primary tenant is a guest of the instance like any other).

Reading these together turns up five problems:

1. **Instance records in a tenant's partitions.** Mirror sync job records are in "the local log" ([0011](0011-logs.md) §6.3) and OAuth consumer events in the primary tenant's `log` ([0025](0025-oauth-server.md) §5). The instance has had a `log` of its own since [0039](0039-files-and-media.md) §10, and global filters have already moved there ([0040](0040-instance-prerogatives.md) §6).
2. **An instance right left off the list.** Consumer approval is "instance-level" ([0025](0025-oauth-server.md) §2), but `mwoauthmanageconsumer` is not in [0040](0040-instance-prerogatives.md) §9's list of instance rights and is held by `bureaucrat`, so as written a bureaucrat on any tenant approves consumers for the whole farm.
3. **The farm slug is undefined.** [0028](0028-tenancy-policy.md) §2 cites [0018](0018-tenants.md) §1 for it, which does not define it. It cannot equal any tenant slug, the primary's included, because it is the farm issuer's code and tenant slugs are issuer codes ([0018](0018-tenants.md) §4).
4. **IRI collisions.** On a single-tenant instance the farm base is the tenant's base ([0018](0018-tenants.md) §1). `config` and `log` are graphs of both scopes (`graphs.toml`, `scope = "both"`), so `{base}/graph/config`, `{base}/graph/log` and the origin lines `{host}/log/config` and `{host}/log/log` each name two partitions, and a farm account's `{farm base}/user/{id}` collides with a tenant account's `{tenant base}/user/{id}`.
5. **No lifecycle.** Nothing says whether the role can move to another tenant, or what happens when the primary tenant is moved away ([0018](0018-tenants.md) §10) or deleted ([0028](0028-tenancy-policy.md) §5). The `tenant` record's "primary flag" ([0018](0018-tenants.md) §3) admits zero primaries or two.

James's direction, from the design discussion of 2026-09-30: the role is **transferable**; **any tenant may hold it**, including one that exists only for the operators; instance IRIs get **their own path**; and the concept gets an ADR of its own.

## Decision

### 1. The primary tenant is where the instance's operators have their accounts (amends 0018 §1)

**The primary tenant is the tenant whose accounts operate the instance.** An instance has exactly one at every point in its log (§2). The role is about identity: it says whose accounts may exercise instance rights, and whose actors may attest records the instance holds on its own behalf. It is **not a storage location** for instance records (§4), and it gives **no authority over other tenants' data** beyond what instance rights already give, exercised through instance acts ([0040](0040-instance-prerogatives.md)).

What the role carries:

| | |
|---|---|
| **Instance rights** | Evaluated on the primary tenant, or through a global group at the farm base ([0028](0028-tenancy-policy.md) §3); held on any other tenant they grant nothing at instance scope (§8, [0040](0040-instance-prerogatives.md) §9) |
| **Attestation of instance records** | Records in instance partitions that are not an upstream's or a farm account's are attested by actors of the primary tenant (§4), including the authority records of instance acts ([0040](0040-instance-prerogatives.md) §4) |
| **The instance's owners** | `owner` on the primary tenant is the instance's `owner`: the group that holds `ts-keys` and `ts-primary`. `owner` on any other tenant is that wiki's owner, whose `all_permissions` include instance rights that grant it nothing there |
| **The instance's bots** | Instance jobs (mirror syncs, provider backfills) run as subsidiaries of primary-tenant accounts ([0024](0024-subsidiary-accounts.md) §1; §5) |
| **Bootstrap** | `triplespace-cli instance create` creates the instance key, the primary tenant, its `owner` and its sync subsidiaries ([0016](0016-permissions-and-access-control.md) §3) |
| **Defaults** | The operator actor's display name defaults to the primary tenant's site name ([0040](0040-instance-prerogatives.md) §2); on a single-tenant instance the farm base is the primary tenant's base ([0018](0018-tenants.md) §1) |

What it does not carry: the primary tenant's records are not instance records; its blocks, ACLs and groups apply to its own accounts and content only; it is a guest under [0040](0040-instance-prerogatives.md) §1, so an instance act on it is attested by the instance like any other; farm accounts, global groups and global blocks live in the farm partitions, not in it ([0028](0028-tenancy-policy.md) §2–4); and it needs no provider code.

**"An actor of the primary tenant" means primary when the record was appended.** Evaluating an instance right, and verifying the attestation of an authority record ([0040](0040-instance-prerogatives.md) §4, §8), look up which tenant held the role at that record's offset in the instance `config` (§2). A record attested by an operator of a former primary tenant stays valid.

**On the primary tenant, its groups are the instance's groups for instance rights.** Whoever holds an instance right there exercises it across the farm: a primary-tenant `bureaucrat` approves OAuth consumers, as Meta's OAuth administrators do for Wikimedia. A farm whose primary tenant is a community wiki either confines instance rights to `owner` there, by its own group records, or uses an operations tenant (§3).

### 2. The `primary` record (extends 0015 §3; amends 0018 §3)

The role is held by one `config` record of a new instance-scope kind, **`primary`**, keyed `primary`, whose content is the tenant's slug and, for a transfer, the offer it accepts (§5). The `tenant` record of [0018](0018-tenants.md) §3 loses its primary flag and keeps slug, base URI and, for a provider tenant, its code and number.

`instance create` appends the first `primary` record after the instance `key:` and the primary tenant's `tenant:` record. The instance `config` partition is `logged` and `full`, so its history answers "which tenant was primary at offset *n*" without a separate table, which §1 relies on.

### 3. Any tenant may be primary

The primary tenant may be a wiki with content, as Librarybase is on its own instance, or an **operations tenant** created only to hold the operators' accounts and the instance's bots, with no pages or entities. An operations tenant may be private (its partitions with the `private` export policy, [0018](0018-tenants.md) §5), since nothing about the role needs to be public; the public face of instance acts is the operator actor and `Special:InstanceAction` at the farm base ([0040](0040-instance-prerogatives.md) §7).

This is a choice each instance makes, not a tenancy switch. By archetype ([0028](0028-tenancy-policy.md) §1): an `isolated` hosting farm will usually want an operations tenant, so that no customer's wiki carries its operators; a `community` farm will usually make its Meta-style wiki primary; an `enterprise` farm, its platform team's wiki.

### 4. Instance records live in instance partitions (amends 0011 §6.3 and 0025 §5)

**The rule.** Every record in an instance partition is attested by one of:

- an upstream actor, for the records a sync job writes into `mirror/*`, `actors/{provider}`, `log/{provider}` and `files/{repo}` (the job's ID in the attestation names the primary-tenant subsidiary that ran it);
- a farm account, for its own records in `actors/{farm}`, `accounts/{farm}` and `log/{farm}`, and for what members of global groups write ([0028](0028-tenancy-policy.md) §2–3, [0040](0040-instance-prerogatives.md) §4);
- an actor of the primary tenant, for everything else.

It is the instance counterpart of [0040](0040-instance-prerogatives.md) §1's rule for a tenant's partitions. Nothing the instance holds on its own behalf is written into the primary tenant's partitions, so a transfer (§5) moves no records and splits no history.

Two cases move accordingly:

- **Mirror sync job records** ([0011](0011-logs.md) §6.3) are appended to the **instance `log`**, keyed by job ID, as the records of jobs that write instance acts already are ([0040](0040-instance-prerogatives.md) §4). "A mirror sync is a local action" now reads "an instance action". Tenant bulk jobs stay in the tenant's `log`.
- **OAuth consumer events** (`oauth/propose`, `oauth/update`, `oauth/approve`, `oauth/reject`, `oauth/disable`; [0025](0025-oauth-server.md) §5) are appended to the **instance `log`**, not the primary tenant's `log` or `log/{farm}`.

The instance `log` stays `internal` ([0039](0039-files-and-media.md) §10). Its public records (job records, consumer events, the public reasons of authority records) are served at the farm base by the same projections as a tenant's log: `Special:Log`, `list=logevents` and the job pages ([0010](0010-site-ui.md) §9). Whether they should also go into a public dump is an open question below.

### 5. Transferring the role

**By offer and acceptance.** A member of `owner` on the primary tenant offers the role to another tenant on the instance with `ts-primary`: a `primary/offer` record in the instance `log` naming the tenant, with an expiry (default seven days). A member of `owner` **on the offered tenant** accepts by appending the new `primary` record, citing the offer. Acceptance is the receiving tenant's consent to its groups becoming the instance's (§1), and is the one write to an instance partition that an actor of a non-primary tenant may attest; it is refused without an open offer naming that tenant. The offerer may withdraw an open offer (`primary/withdraw`). The new `primary` record projects as `primary/transfer` in the instance `log`.

**Effects, from the offset of the new `primary` record:**

- Instance rights are evaluated on the new primary tenant. `owner` on the former primary is that wiki's owner and nothing more.
- Authority records and instance acts made before stay valid (§1); nothing is re-attested.
- Instance jobs already running finish under the subsidiaries that started them; new ones need subsidiaries of the new primary's accounts. `triplespace-cli primary accept` creates them beside the accepting account, as `instance create` does ([0024](0024-subsidiary-accounts.md) §1), and the former primary's sync subsidiaries are left for that tenant's bureaucrats to retire.
- The operator actor keeps its key `instance:{farm slug}` and its IRI. Its display name follows the new primary's site name unless the instance has set its own name.
- Farm accounts, global groups and global blocks are untouched; they never belonged to the primary tenant.

A transfer is not an instance act on either tenant: it writes nothing into either tenant's partitions.

**The primary tenant cannot leave (amends 0018 §10 and 0028 §5).** Moving it away, deleting it, or freezing it for a move is refused with `ts-primary-tenant` until the role has been transferred. On a single-tenant instance there is no tenant to transfer to: moving the only tenant is rehosting the instance ([0018](0018-tenants.md) §10, last paragraph), and deleting it is decommissioning the instance.

### 6. The farm slug (settles 0028 §2's reference)

**The farm slug is the instance's own slug.** It is chosen at `instance create` (`--farm-slug`, required, with no default derived from a tenant), registered in the shared namespace of slugs, provider slugs and issuer codes ([0018](0018-tenants.md) §1), and **never changes**, since it is part of the operator's actor key `instance:{farm slug}` ([0040](0040-instance-prerogatives.md) §2), the farm issuer's code and the farm partition names `actors/{farm}`, `accounts/{farm}` and `log/{farm}` ([0028](0028-tenancy-policy.md) §2). It differs from every tenant slug, the primary tenant's included. A single-tenant instance has one too, because the operator actor needs it whether or not a farm issuer exists. `instance` itself stays reserved ([0040](0040-instance-prerogatives.md) §2).

### 7. Instance IRIs have their own path (amends 0006 §6, 0018 §2, 0028 §2 and §10, 0040 §2 and §7)

Every IRI and origin line the instance mints for something of instance scope is under the reserved path segment **`instance`** of the farm base:

| Thing | Was | Is |
|---|---|---|
| Instance graph, every name in `graphs.toml` with `scope = "instance"` or `"both"` | `{farm base}/graph/{name}` | `{farm base}/instance/graph/{name}` |
| Checkpoint origin line of an instance partition ([0006](0006-log-integrity-and-erasure.md) §6) | `{instance host}/log/{name}` | `{farm host}/instance/log/{name}` |
| Farm account ([0028](0028-tenancy-policy.md) §2, §10) | `{farm base}/user/{id}` | `{farm base}/instance/user/{id}` |
| Operator actor ([0040](0040-instance-prerogatives.md) §2) | `{farm base}/operator` | `{farm base}/instance/operator` |

Tenant IRIs and origin lines are unchanged, and no tenant route, article path or IRI template may begin with `/instance/`. The farm base may then be any host, including the base of the primary tenant or of any other tenant. Record IRIs `{base}/record/{partition}/{offset}` and job IRIs `{base}/job/{id}` are unchanged, because partition IDs and job IDs are unique on the instance. Special pages and REST routes served at the farm base are addresses for people and clients, not identifiers, and keep their paths.

Nothing has been published under the old forms, so this is a change to the documents only.

### 8. Instance rights (amends 0040 §9; extends 0016 §2; amends 0025 §10)

[0040](0040-instance-prerogatives.md) §9's list gains three entries:

| Permission | As an instance right | Default groups |
|---|---|---|
| `ts-primary` *(new)* | Offering and withdrawing the primary role (§5) | `owner` only |
| `mwoauthmanageconsumer` | Approving, rejecting and disabling OAuth consumers, which are instance configuration ([0025](0025-oauth-server.md) §2) | `bureaucrat`, so the primary tenant's bureaucrats; the farm's `steward` and `platform-admin` |
| `ts-runjob` for instance jobs | Submitting a mirror sync or another job that writes only instance partitions | `bot`, so the primary tenant's approved subsidiaries |

Accepting an offer needs no permission beyond membership of `owner` on the offered tenant (§5).

### 9. API and UI

- `meta=siteinfo&siprop=triplespace` reports the farm slug and the primary tenant's slug; `GET /tenancy` at the farm base ([0028](0028-tenancy-policy.md) §11) includes both.
- `Special:Tenants` at the farm base ([0018](0018-tenants.md) §11) marks the primary tenant, lists open offers, and offers **Offer the primary role…** to holders of `ts-primary`. An `owner` of the offered tenant sees **Accept** there and on their own tenant's `Special:Tenancy`.
- `triplespace-cli`: `instance create --farm-slug {slug}`; `primary offer {tenant}`, `primary withdraw`, `primary accept`.
- Refusals: `ts-primary-tenant` (moving, deleting or freezing the primary tenant); `ts-no-offer` (accepting without an open offer).

### 10. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-log` | The `primary` config kind and its single-record rule; the instance path in graph IRIs and origin lines (§2, §7) |
| `scatter-actors` | Instance-right evaluation against the primary tenant at the record's offset; `ts-primary`; acceptance by the offered tenant's `owner` (§1, §5, §8) |
| `scatter-integrity` | Verifying that an authority record's attester belonged to the primary tenant at its offset (§1) |
| `scatter-mwlog` | `primary/offer`, `primary/withdraw`, `primary/transfer` (§5) |
| `triplespace-projections` | Mirror sync job records and consumer events from the instance `log` (§4) |
| `triplespace-api-action`, `triplespace-api-rest` | `siprop=triplespace` and `/tenancy` fields; `Special:Tenants` offer and accept; the two refusals (§9) |
| `triplespace-cli` | `--farm-slug`; `primary offer`, `withdraw` and `accept` (§9) |

No crate is added.

## Consequences

- **One sentence answers "what is the primary tenant":** the tenant whose accounts operate the instance. Everything else it was doing either follows from that or has moved to the instance's own partitions.
- **The role can move without moving data.** Because no instance record lives in the primary tenant, a transfer is one record, and the former primary is an ordinary tenant that can then move away or be deleted.
- **Operators can keep their accounts off customers' wikis.** A hosting farm runs from an empty, private operations tenant; Librarybase on its own instance runs from Librarybase.
- **A community wiki that is primary makes its groups farm-wide in instance rights.** That is deliberate and matches Meta, and it is why §3 exists.
- **The identifiers are unambiguous** whatever the farm base is, at the cost of one path segment that tenants may never use.
- **Milestone 1 needs a farm slug** for Librarybase's single-tenant instance, distinct from `librarybase`, before `instance create` runs.

## Open questions

- **Instance groups separate from the primary tenant's groups.** Whether instance rights should be granted by instance-scope group records rather than by the primary tenant's own groups, so that a community wiki can be primary without its bureaucrats approving consumers. Global groups already do this on farms with farm identity; the question is for farms without it.
- **Public dumps of the instance `log`.** Its job records and consumer events are public; its takedown and expunge records are not. Whether to give the instance `log` an export policy per payload type, or keep the whole partition `internal` and serve the public records only through the API.
- **Two owners for a transfer.** Whether offering the role should, like expunge ([0040](0040-instance-prerogatives.md), open), need two members of `owner`.
- **Offer expiry default.** Seven days is a placeholder.

## References

- [0018 — Tenants](0018-tenants.md), [0028 — Tenancy policy](0028-tenancy-policy.md), [0040 — Instance prerogatives](0040-instance-prerogatives.md)
- [Meta-Wiki: OAuth administrators](https://meta.wikimedia.org/wiki/OAuth_administrators) (consumer approval on Wikimedia's primary-like wiki)
