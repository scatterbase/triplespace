# 0040. Instance prerogatives

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-09 (A5)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0028](0028-tenancy-policy.md), [0030](0030-edit-filters.md), [0039](0039-files-and-media.md)
- **Uses:** [0023](0023-moderation.md), [0025](0025-oauth-server.md), [0026](0026-sitelinks.md), [0046](0046-primary-tenant.md)
- **Chapters:** [02](../architecture/02-graphs-rdf-and-query.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

[0018](0018-tenants.md) §4 made attribution per tenant: "every record is still attested by a tenant actor, and instance-level records by the primary tenant's". It assumed that the instance never writes into a tenant's partitions. It does, and more often each time a farm feature is added:

| Where | What the instance writes into a tenant | Attested by, as written |
|---|---|---|
| [0018](0018-tenants.md) §2 | Every registration and rotation of the instance key, appended to each tenant's key chain | Signed by the previous key; no actor named |
| [0028](0028-tenancy-policy.md) §2 | Renaming or vanishing a farm account writes actor records into every linked tenant | Unsaid |
| [0028](0028-tenancy-policy.md) §5 | A deleted tenant's partitions are erased after a grace period | Unsaid |
| [0028](0028-tenancy-policy.md) §8 | Changing a locked template re-writes it into every tenant's `config` | Unsaid |
| [0030](0030-edit-filters.md) §4, §8 | A global filter with a `block` or `degroup` action appends a record to the tenant's `actors` | Unsaid |
| [0039](0039-files-and-media.md) §9–10 | Reclamation and expunge erase upload records in the tenant's `pages` | An exception to 0018 §4, written for files alone |

0039 handled its case with a one-off exception, and James's observation is that this should be a principle: sometimes the instance has to make an overriding judgment on a guest tenant, and the system should say so the same way every time.

Two facts shape the answer. First, **the instance already has this authority**: its key is the first entry of every tenant's key chain and signs every tenant's checkpoints ([0006](0006-log-integrity-and-erasure.md) §6, [0018](0018-tenants.md) §2), so it can already put anything into a tenant's log. What is missing is that it does so visibly, per record, and on a stated authority. Second, **the key that proves it is the instance key**, not an operator's: an actor's signing key is a `scatter:v0/key` record in its own tenant's `actors` ([0015](0015-record-format-and-partition-registry.md) §1), which a guest's export bundle does not carry, whereas the instance key is already in the guest's own key chain.

James's direction, from the design discussion of 2026-09-30: an instance prerogative is an exceptional action outside the usual editorial scope, and **it is attributed to the instance as operator**, not to the person who carried it out.

## Decision

### 1. Instance acts, prerogatives and provisions (amends 0018 §4)

*Current text: [08](../architecture/08-tenants-and-instances.md) §8.1.*

### 2. The instance as an actor (extends 0007 §1 and §2)

*Changed by A2, A5.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §1.2, §8.2.*

### 3. The instance attestation (extends 0015 §1; extends 0006 §2)

*Changed by A5.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §8.3.*

### 4. Authority records

*Changed by A2.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §8.4.*

### 5. Binding

*Current text: [08](../architecture/08-tenants-and-instances.md) §8.5.*

### 6. The written acts (amends 0028 §2, §5 and §8; amends 0030 §8; amends 0039 §9 and §10)

*Current text: [08](../architecture/08-tenants-and-instances.md) §8.6.*

### 7. Display, logs and RDF (extends 0010 §5 and 0011 §6.1)

*Changed by A5.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §5.1; [16](../architecture/16-logs-feeds-and-notifications.md) §2.1, §2.3, §5.2; [19](../architecture/19-site-ui.md) §2.5, §3.4, §6.11.*

### 8. Verification and moving a tenant (extends 0006 §9 and 0018 §10)

*Current text: [08](../architecture/08-tenants-and-instances.md) §6.2, §6.5, §8.7.*

### 9. Instance rights (extends 0016 §2)

*Changed by A2.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §2.3.*

### 10. Scatterbase

*Current text: [22](../architecture/22-crates-and-stack.md) §1.3.*

### 11. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **One rule replaces several silences and one exception.** Every record in a tenant's log is the tenant's or the instance's, and the instance's say so, are signed, and cite an authority. 0039's exception is gone; the attestation gaps in 0028 and 0030 are closed.
- **Trust is made explicit, not extended.** The instance key could always write into a guest's log. Now each such write is labelled, bound to a stated authority, and checkable from the guest's bundle alone.
- **Operators are shielded.** Guests see the instance operator, never the person, and the person is recorded where the instance's own operators can hold each other to account.
- **Evaluated acts are preferred,** so most instance authority leaves no trace in guests' logs at all; it is applied at read and write time and disappears when the instance rule does.
- **Binding is uniform.** `ts-locked`, unliftable global blocks and irreversible erasures are three cases of one rule, with one refusal code.
- **Global filters move partitions** before any code exists for them: from the primary tenant's `log` to the instance `log`.
- **Exports carry a little more:** the authority extract, with operators' identities withheld.

## Open questions

- **Q1. Multiple operators' consent.** Whether some written prerogatives, such as expunge or tenant deletion, should need two operators' authority records before the instance acts.
- **Q2. Guest notice.** Whether a guest's administrators should be notified of every instance act on their tenant, or only of binding ones, and through which channel.
- **Q3. Appeals.** Whether an authority record should carry a structured appeal route (a contact, a deadline) that the guest's UI shows beside the act.
- **Q4. Scatterbase's server acts**, if Scatterbase wants the same distinction between a server's own claims and its users'.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §11 | extends | 0005 A40 |
| [0006](0006-log-integrity-and-erasure.md) §2, §9 | §3, §8 | extends | 0006 A11 |
| [0007](0007-actor-identity.md) §1 | §2 | extends | 0007 A13 |
| [0010](0010-site-ui.md) §5.2 | §7 | extends | 0010 A24 |
| [0011](0011-logs.md) §6.1 | §4, §7 | extends | 0011 A14 |
| [0011](0011-logs.md) §6.3 | §4, §7 | amends | 0011 A14 |
| [0015](0015-record-format-and-partition-registry.md) §1 | §3 | extends | 0015 A18 |
| [0016](0016-permissions-and-access-control.md) §2, §6 | §7, §9 | extends | 0016 A14 |
| [0018](0018-tenants.md) §4 | §1–3, §8 | amends | 0018 A7 |
| [0018](0018-tenants.md) §10 | §1–3, §8 | extends | 0018 A7 |
| [0028](0028-tenancy-policy.md) §2, §5, §8 | §5–6 | amends | 0028 A5 |
| [0030](0030-edit-filters.md) §8 | §6 | amends | 0030 A5 |
| [0039](0039-files-and-media.md) §9, §10 | §6 | amends | 0039 A2 |

## References

- [Wikimedia Foundation Office actions](https://foundation.wikimedia.org/wiki/Policy:Office_actions) (actions taken under a role account)
- [C2SP signed note](https://c2sp.org/signed-note) (the instance key's signature format)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-30
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §11
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A40).

Replaced text (§11):

> | Crate | Change |
> |---|---|
> | `scatter-log` | The instance attestation in the attestation part, domain tag `0x06`, and the write-path refusal of submitted instance attestations (§3) |
> | `scatter-integrity` | Verifying instance attestations against the key chain; the authority extract in bundles (§8) |
> | `scatter-actors` | The `instance` issuer and operator actor (§2); instance rights; binding as a floor in effective-permission evaluation (§5, §9). Embeds the new `issuers.toml` entry |
> | `scatter-mwlog` | `instance: true` and the authority link in log parameters (§7) |
> | `triplespace-projections` | The operator as performer, the Instance action label and `scatter:authority` (§7) |
> | `triplespace-api-action`, `triplespace-api-rest` | `ts-prerogative`; `Special:InstanceAction`; the farm-base instance-action list (§7) |
>
> No crate is added.

### A2. The primary tenant

- **Date:** 2026-09-30
- **Source:** [0046](0046-primary-tenant.md) §1, §6–8
- **Change:** amends §2, §4, §9
- **Summary:** By section:
  - §2: The operator's IRI is `{farm base}/instance/operator`. The farm slug in its actor key is defined in 0046 §6: the instance's own slug, distinct from every tenant's, and never changed.
  - §4: "An actor of the primary tenant" means an actor of the tenant that held the role when the authority record was appended, as the history of the `primary` record shows, and `verify` checks it so. An authority record attested by an operator of a former primary tenant stays valid after a transfer.
  - §9: Three more instance rights: `ts-primary` (offering the primary role; `owner` only), `mwoauthmanageconsumer` (approving OAuth consumers, [0025](0025-oauth-server.md) §2) and `ts-runjob` for instance jobs. An instance right is evaluated on the tenant that is primary at the time of the act.

Replaced text (§2):

> Its actor key is `instance:{farm slug}`, its IRI is `{farm base}/operator`, and its display name is the message "{instance name} operator" (by default the site name of the primary tenant).

Replaced text (§4):

> **Operators act from the primary tenant.** An authority record is attested by an actor of the primary tenant, or by a farm account acting through a global group at the farm base ([0028](0028-tenancy-policy.md) §3), and only with an instance right (§9).

Replaced text (§9):

> An **instance right** is a permission that authorizes an instance act. It is evaluated on the primary tenant, or through a global group at the farm base ([0028](0028-tenancy-policy.md) §3); held on any other tenant it grants nothing at instance scope. The instance rights so far are `ts-keys`, `ts-config` at the farm base, `abusefilter-modify` for `scope = global` filters ([0030](0030-edit-filters.md) §8), `userrights`, `block` and `renameuser` on farm accounts ([0028](0028-tenancy-policy.md) §3–4), and `ts-takedown` and `ts-expunge` ([0039](0039-files-and-media.md) §21).

### A3. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §2, §4, §9, §11
- **Summary:** A1–A2 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A2 was three blockquotes, and A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A4. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§11
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A5. The operator's key carries the farm code

- **Date:** 2026-10-09
- **Source:** [0079](0079-derived-issuer-codes.md) §4
- **Change:** amends §2, §3, §7
- **Summary:** The operator actor's key is `instance:{farm code}`, where the farm code is derived from the instance's founding record ([0079](0079-derived-issuer-codes.md) §4), not the farm slug, which no longer appears in any key. The attestation's `actor` field, log projections and contributions use that key.

Replaced text (§2, in [08](../architecture/08-tenants-and-instances.md) §8.2):

> Its actor key is `instance:{farm slug}` (§1.2);

Replaced text (§3, in [08](../architecture/08-tenants-and-instances.md) §8.3):

> | `actor` | `instance:{farm slug}` |

Replaced text (§7, in [16](../architecture/16-logs-feeds-and-notifications.md) §2.1 and [19](../architecture/19-site-ui.md) §2.5, §6.11):

> names the operator actor `instance:{farm slug}` as performer … Contributions of `instance:{farm slug}` at a tenant …
>
> - **Contributions of `instance:{farm slug}`** at a tenant …
>
> … and the contributions of `instance:{farm slug}` (§2.5; …
