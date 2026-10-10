# 0046. The primary tenant

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-09 (A7)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0011](0011-logs.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0024](0024-subsidiary-accounts.md), [0025](0025-oauth-server.md), [0028](0028-tenancy-policy.md), [0040](0040-instance-prerogatives.md)
- **Uses:** [0010](0010-site-ui.md), [0039](0039-files-and-media.md), [0047](0047-special-pages.md)
- **Chapters:** [01](../architecture/01-log-and-records.md), [02](../architecture/02-graphs-rdf-and-query.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

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

*Current text: [08](../architecture/08-tenants-and-instances.md) §1, §7.1.*

### 2. The `primary` record (extends 0015 §3; amends 0018 §3)

*Current text: [08](../architecture/08-tenants-and-instances.md) §2.5, §7.2.*

### 3. Any tenant may be primary

*Current text: [08](../architecture/08-tenants-and-instances.md) §7.3.*

### 4. Instance records live in instance partitions (amends 0011 §6.3 and 0025 §5)

*Current text: [01](../architecture/01-log-and-records.md) §1.4; [08](../architecture/08-tenants-and-instances.md) §7.4.*

### 5. Transferring the role

*Changed by A7.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §7.5, §7.6.*

### 6. The farm slug (settles 0028 §2's reference)

*Changed by A7.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §1.2.*

### 7. Instance IRIs have their own path (amends 0006 §6, 0018 §2, 0028 §2 and §10, 0040 §2 and §7)

*Changed by A2.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.3, §2.2.*

### 8. Instance rights (amends 0040 §9; extends 0016 §2; amends 0025 §10)

*Changed by A4.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §2.3.*

### 9. API and UI

*Changed by A5, A7.*

*Current text: [18](../architecture/18-api.md) §1.2, §2.2; [19](../architecture/19-site-ui.md) §6.11.*

### 10. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **One sentence answers "what is the primary tenant":** the tenant whose accounts operate the instance. Everything else it was doing either follows from that or has moved to the instance's own partitions.
- **The role can move without moving data.** Because no instance record lives in the primary tenant, a transfer is one record, and the former primary is an ordinary tenant that can then move away or be deleted.
- **Operators can keep their accounts off customers' wikis.** A hosting farm runs from an empty, private operations tenant; Librarybase on its own instance runs from Librarybase.
- **A community wiki that is primary makes its groups farm-wide in instance rights.** That is deliberate and matches Meta, and it is why §3 exists.
- **The identifiers are unambiguous** whatever the farm base is, at the cost of one path segment that tenants may never use.
- **Milestone 1 needs a farm slug** for Librarybase's single-tenant instance, distinct from `librarybase`, before `instance create` runs.

## Open questions

- **Q1. Instance groups separate from the primary tenant's groups.** Whether instance rights should be granted by instance-scope group records rather than by the primary tenant's own groups, so that a community wiki can be primary without its bureaucrats approving consumers. Global groups already do this on farms with farm identity; the question is for farms without it.
- **Q2. Public dumps of the instance `log`.** Its job records and consumer events are public; its takedown and expunge records are not. Whether to give the instance `log` an export policy per payload type, or keep the whole partition `internal` and serve the public records only through the API.
- **Q3. Two owners for a transfer.** Whether offering the role should, like expunge ([0040](0040-instance-prerogatives.md) Q1), need two members of `owner`.
- **Q4. Offer expiry default.** Seven days is a placeholder.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §10 | extends | 0005 A46 |
| [0006](0006-log-integrity-and-erasure.md) §6 | §7 | amends | 0006 A12 |
| [0007](0007-actor-identity.md) §1 | §7 | amends | 0007 A14 |
| [0007](0007-actor-identity.md) §2 | §7 | extends | 0007 A14 |
| [0011](0011-logs.md) §6.3 | §4 | amends | 0011 A15 |
| [0013](0013-postgres-storage.md) §5.5 | §2 | extends | 0013 A18 |
| [0015](0015-record-format-and-partition-registry.md) §5 | §2, §7 | amends | 0015 A20 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §2, §7 | extends | 0015 A20 |
| [0016](0016-permissions-and-access-control.md) §3 | §1, §8 | amends | 0016 A15 |
| [0016](0016-permissions-and-access-control.md) §2 | §1, §8 | extends | 0016 A15 |
| [0018](0018-tenants.md) §1, §2, §3, §10 | §1–3, §5, §7 | amends | 0018 A8 |
| [0024](0024-subsidiary-accounts.md) §1 | §5, §8 | amends | 0024 A6 |
| [0025](0025-oauth-server.md) §5, §10 | §4, §8 | amends | 0025 A2 |
| [0028](0028-tenancy-policy.md) §2, §5, §10 | §5–7 | amends | 0028 A7 |
| [0040](0040-instance-prerogatives.md) §2, §4, §9 | §1, §6–8 | amends | 0040 A2 |

## References

- [0018 — Tenants](0018-tenants.md), [0028 — Tenancy policy](0028-tenancy-policy.md), [0040 — Instance prerogatives](0040-instance-prerogatives.md)
- [Meta-Wiki: OAuth administrators](https://meta.wikimedia.org/wiki/OAuth_administrators) (consumer approval on Wikimedia's primary-like wiki)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-30
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §10
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A46).

Replaced text (§10):

> | Crate | Change |
> |---|---|
> | `scatter-log` | The `primary` config kind and its single-record rule; the instance path in graph IRIs and origin lines (§2, §7) |
> | `scatter-actors` | Instance-right evaluation against the primary tenant at the record's offset; `ts-primary`; acceptance by the offered tenant's `owner` (§1, §5, §8) |
> | `scatter-integrity` | Verifying that an authority record's attester belonged to the primary tenant at its offset (§1) |
> | `scatter-mwlog` | `primary/offer`, `primary/withdraw`, `primary/transfer` (§5) |
> | `triplespace-projections` | Mirror sync job records and consumer events from the instance `log` (§4) |
> | `triplespace-api-action`, `triplespace-api-rest` | `siprop=triplespace` and `/tenancy` fields; `Special:Tenants` offer and accept; the two refusals (§9) |
> | `triplespace-cli` | `--farm-slug`; `primary offer`, `withdraw` and `accept` (§9) |
>
> No crate is added.

### A2. Special pages at a shared farm base

- **Date:** 2026-09-30
- **Source:** [0047](0047-special-pages.md) §3
- **Change:** extends §7
- **Summary:** Where the farm base is also a tenant's base, a special page with a tenant and an instance form, such as `Special:Log` or `Special:Jobs`, shows both, with a `scope` filter; the registry's names are unique, so nothing else collides.

### A3. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §7, §10
- **Summary:** A1–A2 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A2 was a blockquote, and A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A4. Jobs run under subsidiaries everywhere

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §8
- **Summary:** Jobs run under subsidiaries: `ts-runjob` defaults to `bot` everywhere, not only for instance jobs; [0016](0016-permissions-and-access-control.md) §2 drops `sysop` from it, and an administrator runs a job by creating or approving a subsidiary they operate ([0047](0047-special-pages.md) §12's `sysop` job rate row goes). James: "administrators should be able to run jobs under subsidiaries", read as this. The row's verb is `amends`, but §8 already gives `ts-runjob` for instance jobs the `bot` default and nothing in it is contradicted, so for this ADR it is logged as `extends`: §8's row is now the general rule, stated once for instance jobs. (PENDING C14)

### A5. The tenant list

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §9
- **Summary:** The tenant list is the `tenants` array of `GET /tenancy` at the farm base, one entry per tenant with `slug`, `host`, `name` and `status`, and `Special:Tenants` is its page ([0018](0018-tenants.md) §11). §9's `/tenancy` and `Special:Tenants` entries carry the farm slug, the primary tenant and the offers on top of that list. (PENDING E33)

### A6. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [01](../architecture/01-log-and-records.md), [02](../architecture/02-graphs-rdf-and-query.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A7. The farm code

- **Date:** 2026-10-09
- **Source:** [0079](0079-derived-issuer-codes.md) §4, §7
- **Change:** amends §5, §6; extends §9
- **Summary:** The farm slug stays the instance's readable name, chosen at `instance create --farm-slug` and never changed, but it is in no key. The **farm code**, derived from the instance's founding record (its first `key:` record, at offset 0 of the instance `config` partition) as a tenant's issuer code is, is what the operator's actor key `instance:{farm code}`, the farm issuer's code and the farm partition names carry, so that an instance act stays attributed to the instance that made it whatever other instance shares its slug (§6). A primary transfer leaves the operator's key unchanged (§5). `siprop=triplespace` and `GET /tenancy` at the farm base report the farm code beside the farm slug (§9).

Replaced text (§6, in [08](../architecture/08-tenants-and-instances.md) §1.2):

> **The farm slug is the instance's own slug.** It is chosen at `instance create` (`--farm-slug`, required, with no default derived from a tenant), registered in the same shared namespace of slugs, provider slugs and issuer codes, and **never changes**, since it is part of the operator's actor key `instance:{farm slug}` (§8.2), the farm issuer's code (§3.5) and the farm partition names `actors/{farm}`, `accounts/{farm}` and `log/{farm}` (§2.1). It differs from every tenant slug, the primary tenant's included. A single-tenant instance has one too, because the operator actor needs it whether or not a farm issuer exists. `instance` itself is reserved in the shared namespace, so no tenant or provider can take it.

Replaced text (§5, in [08](../architecture/08-tenants-and-instances.md) §7.5):

> - The operator actor keeps its key `instance:{farm slug}` and its IRI.
