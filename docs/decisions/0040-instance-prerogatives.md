# 0040. Instance prerogatives

- **Status:** Proposed
- **Date:** 2026-09-30
- **Author:** James Hare / Claude Opus
- **Related:** [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§11 amends §2), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (§3 extends §2 with domain tag `0x06`; §8 extends §9: instance attestations are verified, and bundles carry an authority extract), [0007 — Actor identity](0007-actor-identity.md) (§2 extends §1 and §2: the `instance` issuer and the operator actor), [0010 — Site UI](0010-site-ui.md) (§7 extends §5: instance acts in history), [0011 — Upstream and local logs](0011-logs.md) (§7 extends §6.1: the operator as performer; §4 amends §6.3: jobs that act on the instance's authority record in the instance `log`), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (§3 extends §1: the instance attestation), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§9 extends §2: instance rights and `ts-viewoperator`), [0018 — Tenants](0018-tenants.md) (§1 amends §4: a guest's records are attested by its own actors or by the instance; §8 extends §10: the authority extract), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§6 uses §1: a takedown is an evaluated prerogative), [0026 — Sitelinks are URLs](0026-sitelinks.md) (§6 uses §3: the instance deny list is an evaluated prerogative), [0028 — Tenancy policy](0028-tenancy-policy.md) (§6 amends §2, §5 and §8: cascades, tenant deletion and locked templates are written by the instance; settles the attestation of each), [0030 — Edit filters](0030-edit-filters.md) (§6 amends §8: global filters move to the instance `log`, and their blocks are prerogatives), [0039 — Files, blob storage and foreign file repositories](0039-files-and-media.md) (§6 amends §9 and §10: reclamation and expunge are prerogatives; replaces 0039's attestation exception)

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

Two facts shape the answer. First, **the instance already has this authority**: its key is the first entry of every tenant's key chain and signs every tenant's checkpoints ([0006](0006-log-integrity-and-erasure.md) §6, [0018](0018-tenants.md) §2), so it can already put anything into a tenant's log. What is missing is that it does so visibly, per record, and on a stated authority. Second, **the key that proves it is the instance key**, not an operator's: an actor's signing key is a `scatter:v0/key` record in its own tenant's `actors` ([0015](0015-record-format-and-partition-registry.md) §1, as amended), which a guest's export bundle does not carry, whereas the instance key is already in the guest's own key chain.

James's direction, from the design discussion of 2026-09-30: an instance prerogative is an exceptional action outside the usual editorial scope, and **it is attributed to the instance as operator**, not to the person who carried it out.

## Decision

### 1. Instance acts, prerogatives and provisions (amends 0018 §4)

An **instance act** is anything the instance does to a tenant's data on its own authority rather than the tenant's. It takes one of two forms, and the first is preferred:

- **Evaluated.** An instance rule is applied when a tenant's data is read or written, and nothing is written to the tenant. Global groups and blocks ([0028](0028-tenancy-policy.md) §3–4), IP blocks, the instance sitelink deny list ([0026](0026-sitelinks.md) §3), global filters' refusals ([0030](0030-edit-filters.md) §8) and file takedowns ([0039](0039-files-and-media.md) §10) are evaluated.
- **Written.** A record is appended to the tenant's own partition, because the tenant's log itself has to change: something must be erased from it, or a record the tenant's projections read must exist in it. A written act is used only when an evaluated one cannot do the job.

An instance act is a **prerogative** when it is **binding**: the tenant cannot refuse, undo or lower it (§5). It is a **provision** when it is not: the instance writes a starting value, and the tenant may change it like any of its own records. Templates written into a new tenant under `config.template = defaults` ([0028](0028-tenancy-policy.md) §8) are provisions; locked templates are prerogatives.

**The rule replacing 0018 §4's.** Every record in a tenant's partitions is attested either by one of that tenant's actors, or by the instance under §3. Nothing else. The tenant whose partition it is, and every tenant on the instance including the primary one, is a **guest** of the instance for this purpose: a prerogative on the primary tenant is attested by the instance like any other.

### 2. The instance as an actor (extends 0007 §1 and §2)

The instance is an actor of a reserved issuer, **`instance`**, with actor model `provider-only` ([0007](0007-actor-identity.md) §1, §6): changes are attributed to the instance as a whole, as OpenAlex's are to OpenAlex. Its actor key is `instance:{farm slug}`, its IRI is `{farm base}/operator`, and its display name is the message "{instance name} operator" (by default the site name of the primary tenant). `instance` is reserved in the shared namespace of slugs and issuer codes ([0018](0018-tenants.md) §1), so no tenant or provider can take it.

**The person who acted is not the actor.** Every instance act has an **authority record** (§4) attested by the operator who carried it out, an actor of the primary tenant. That attestation is visible only to `ts-viewoperator` (§9). A guest's history, logs, RDF, feeds and notifications show the instance operator, as Wikimedia shows office actions under a role account. The reason is the one James gave: these are exceptional acts outside editorial scope, and the people who carry them out, often in response to legal notices, should not become the target of the guest community's disagreement with them.

**The operator actor edits nothing editorial.** It cannot be logged into, holds no group memberships on any tenant, and appears only on records written under §3.

### 3. The instance attestation (extends 0015 §1; extends 0006 §2)

A written instance act carries, in the attestation part of the record appended to the guest, an **instance attestation**:

| Field | Holds |
|---|---|
| `actor` | `instance:{farm slug}` |
| `authority` | The authority record's partition, offset and leaf hash ([0006](0006-log-integrity-and-erasure.md) §5) |
| `job` | The job that wrote it, where one did (§4) |
| `binding` | `true` for a prerogative, `false` for a provision |
| `signature` | `{key: <instance key ID>, alg: "ed25519", sig: <64 bytes>}`, by the instance key current when the record is appended |

**The signature** is over `H(0x06 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment) ‖ authority)`, where `authority` is the canonical CBOR of the `authority` field. Tag `0x06` is new and joins the table of [0006](0006-log-integrity-and-erasure.md) §2; like `0x04` and `0x05` ([0015](0015-record-format-and-partition-registry.md) §1) it never appears in the header tree. It has the shape of a client signature (0015 §1, as amended), with the instance key in place of an actor's and the authority bound into the preimage, so the same verification code checks both.

**Why the instance key.** It is the one key a guest's bundle already trusts: the first `key:` record of the guest's `config` and the key on every one of its checkpoints. Signing instance acts with it adds no trust the guest does not already extend. It makes that trust auditable record by record, and it lets a third party holding only the guest's bundle tell an instance act from a tenant edit and check it, without the primary tenant's partitions.

**Only the instance writes them.** The write path refuses any submitted record whose attestation names the `instance` issuer, and any attestation with an `authority` field, with `ts-prerogative`. The server writes instance attestations itself, after checking the authority record and the operator's rights.

### 4. Authority records

**Every written act has an authority record**, a record in an **instance partition** ([0018](0018-tenants.md) §2) that says what the instance decided and why. It is attested by the operator who acted, and its comment part holds the public reason. The authority is whatever record already expresses the decision:

| Act | Authority record |
|---|---|
| Instance key registration and rotation | The `key:` record in the instance `config` |
| Tenant creation, template provisions and locked templates | The `tenant:`, `template:` and `tenancy:locked` records in the instance `config` |
| Farm account rename and vanish cascades | The actor record in `actors/{farm}` |
| Global filter `block` and `degroup` | The global filter record, in the instance `log` (§6), with the hit's coordinates in the guest record's content |
| Expunge | The `expunge` record in the instance `log` ([0039](0039-files-and-media.md) §10) |
| Reclamation, tenant deletion, and any other scheduled or bulk act | The **job record** in the instance `log` |

**Jobs that act on the instance's authority record in the instance `log` (amends 0011 §6.3).** [0011](0011-logs.md) §6.3 puts every job record in the local log. A job that writes instance acts into guests (a reclamation run, the propagation of an expunge, the erasure of a deleted tenant, a cascade of renames) now records its start, finish and counts in the instance `log`, keyed by job ID, and that job record is the authority its writes cite. Mirror syncs are unchanged.

**Operators act from the primary tenant.** An authority record is attested by an actor of the primary tenant, or by a farm account acting through a global group at the farm base ([0028](0028-tenancy-policy.md) §3), and only with an instance right (§9).

### 5. Binding

A prerogative binds the guest in three ways:

1. **It cannot be superseded by the guest.** A guest write that would replace or retire a binding record is refused with `ts-prerogative`, naming the authority's public reason. Only another instance act, on the same or a later authority, can change it.
2. **It cannot be erased or hidden by the guest.** No part of a binding record can be the target of a guest's `erase` or `record` ACL ([0023](0023-moderation.md) §2). The instance can erase it, as an instance act.
3. **Where the guest's own records share a key with it, the prerogative is a floor.** A block, a group membership or a setting can come from both the guest and the instance. They are evaluated as two layers, and the guest's layer cannot lower the instance's: a guest unblock lifts only the guest's own block, and a global filter's block holds until it expires or the instance lifts it. A guest may still add to it, by blocking longer or more widely.

A provision binds nothing: the guest's next record on the same key replaces it in the ordinary way, and the provision stays in history as the instance's.

### 6. The written acts (amends 0028 §2, §5 and §8; amends 0030 §8; amends 0039 §9 and §10)

Each case of the Context, settled:

| Act | Written into the guest | Binding | Changes to the earlier ADR |
|---|---|---|---|
| **Instance key** registration and rotation ([0018](0018-tenants.md) §2) | `key:` records in `config` | Yes | None in substance; the copies now carry the instance attestation with the instance `config` record as authority |
| **Tenant creation** and template **provisions** ([0028](0028-tenancy-policy.md) §8, `defaults`) | `config` records | No | Settles who attests them |
| **Locked templates** (0028 §8, `locks`) | `config` records, re-written on every change | Yes | `ts-locked` becomes a case of `ts-prerogative`; the code `ts-locked` is kept for compatibility |
| **Farm account rename and vanish** ([0028](0028-tenancy-policy.md) §2) | Actor records in `actors` | Yes | Settles who attests them; a guest bureaucrat cannot rename a linked account away from its farm name while shared names are on |
| **Tenant deletion** ([0028](0028-tenancy-policy.md) §5) | `erase` records over every partition, after the grace period | Yes | Settles who attests them; the job record in the instance `log` is the authority |
| **Global filter `block` and `degroup`** ([0030](0030-edit-filters.md) §4, §8) | `block` and `membership` records in `actors` | Yes, for the block's duration | Global filters move from the primary tenant's `log` to the **instance `log`**, which did not exist when 0030 §8 was written; they remain `scope = global` filters with the same evaluation order. Hits stay in the guest's `log`, attested by the actor whose write was hit, since the hit records that actor's act |
| **Expunge** ([0039](0039-files-and-media.md) §10) | `erase` records over upload content parts | Yes | Replaces 0039's exception: the erasures carry the instance attestation, with the `expunge` record as authority |
| **Reclamation** ([0039](0039-files-and-media.md) §9) | `erase` records over upload content parts | Yes | Replaces 0039's exception: the reclamation job record in the instance `log` is the authority |

The evaluated acts of §1 write nothing into guests and need no attestation. Each has its authority record in an instance partition already (a global group, a block in `actors/{farm}`, the `sitelink-policy:deny` record, a `blob` ACL), and that record is attested by the operator who wrote it, visible under §9's rule.

**A new instance act** needs only to say which form it takes, what its authority record is, and whether it binds. If it is written, it uses §3.

### 7. Display, logs and RDF (extends 0010 §5 and 0011 §6.1)

- **History and feeds** ([0010](0010-site-ui.md) §5) show an instance act with the operator actor, an **Instance action** label, the public reason, and a link to the authority's public page at the farm base, `Special:InstanceAction/{partition}/{offset}`. The page shows the authority record's kind, time, public reason and, where the kind allows, its content; the operator's own name only to `ts-viewoperator`.
- **Log events** projected from instance acts ([0011](0011-logs.md) §6.1) keep their own types (`erase/erase`, `block/block`, `rights/rights`, `renameuser/renameuser`) with the operator actor as performer and `instance: true` in their parameters. `list=logevents` reports the performer as the operator actor's display name, and `leprop=details` includes the authority link.
- **Contributions** of `instance:{farm slug}` at a tenant list the instance acts on that tenant; at the farm base, for `ts-viewoperator`, every instance act on every tenant with the operator who carried it out.
- **RDF** ([0011](0011-logs.md) §8): the record's revision node has `prov:wasAttributedTo` the operator IRI, and `scatter:authority` the authority record's IRI `{farm base}/record/{partition}/{offset}`. The person never appears.
- **Notifications** that an act triggers on a guest (a rename, a block) name the instance operator.

### 8. Verification and moving a tenant (extends 0006 §9 and 0018 §10)

**`verify`** ([0006](0006-log-integrity-and-erasure.md) §9) level 2 checks every instance attestation: the signature against the guest's key chain, using the key that was current when the record was appended; that the record's `actor` and `authority` are well formed; and, where the authority extract (below) is present, that the authority's leaf hash matches. An instance attestation with no extract entry is reported as **authority not included**, which is a warning, since the instance `log` is internal; a bad signature is a failure.

**The authority extract.** An export bundle of a tenant, and the bundle that moves it ([0018](0018-tenants.md) §10), carries every authority record its instance attestations cite, with an inclusion proof against a signed checkpoint of the authority's partition. Each is carried with its attestation part withheld (`[null, leaf]`, as an erased part is stored under [0015](0015-record-format-and-partition-registry.md) §1), so that the operator's identity does not leave the instance while the record still verifies; content and comment are withheld too where the act's kind makes them confidential (a `confidential` takedown, an expunge's hash, [0039](0039-files-and-media.md) §10). This is the same arrangement as the retention extract: what the tenant's history depends on travels with it.

**After a move,** the old instance's acts stay verifiable through the old key, which stays in the guest's key chain. The new instance's acts are signed by the new key. A guest that leaves takes no prerogative with it as a rule: bindings from the old instance stop binding once the tenant is registered elsewhere, as global groups and blocks stop applying ([0028](0028-tenancy-policy.md) §10). The records remain in history, attested by the old instance's operator.

### 9. Instance rights (extends 0016 §2)

An **instance right** is a permission that authorizes an instance act. It is evaluated on the primary tenant, or through a global group at the farm base ([0028](0028-tenancy-policy.md) §3); held on any other tenant it grants nothing at instance scope. The instance rights so far are `ts-keys`, `ts-config` at the farm base, `abusefilter-modify` for `scope = global` filters ([0030](0030-edit-filters.md) §8), `userrights`, `block` and `renameuser` on farm accounts ([0028](0028-tenancy-policy.md) §3–4), and `ts-takedown` and `ts-expunge` ([0039](0039-files-and-media.md) §21).

One new right:

| Permission | Governs | Default groups |
|---|---|---|
| `ts-viewoperator` *(new)* | Seeing which operator carried out an instance act: the attestation of authority records, and the operator column of the farm-base instance-action list | `owner` on the primary tenant; the farm's `steward` and `platform-admin` |

### 10. Scatterbase

Scatterbase has no tenants, so it has no guests. It inherits the instance attestation as a shape: a Scatterbase server that writes a claim on its own authority (a server-side correction, a takedown of a claim) can attest it the same way, with its server key in place of the instance key. Whether it does is Scatterbase policy.

### 11. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-log` | The instance attestation in the attestation part, domain tag `0x06`, and the write-path refusal of submitted instance attestations (§3) |
| `scatter-integrity` | Verifying instance attestations against the key chain; the authority extract in bundles (§8) |
| `scatter-actors` | The `instance` issuer and operator actor (§2); instance rights; binding as a floor in effective-permission evaluation (§5, §9). Embeds the new `issuers.toml` entry |
| `scatter-mwlog` | `instance: true` and the authority link in log parameters (§7) |
| `triplespace-projections` | The operator as performer, the Instance action label and `scatter:authority` (§7) |
| `triplespace-api-action`, `triplespace-api-rest` | `ts-prerogative`; `Special:InstanceAction`; the farm-base instance-action list (§7) |

No crate is added.

## Consequences

- **One rule replaces several silences and one exception.** Every record in a tenant's log is the tenant's or the instance's, and the instance's say so, are signed, and cite an authority. 0039's exception is gone; the attestation gaps in 0028 and 0030 are closed.
- **Trust is made explicit, not extended.** The instance key could always write into a guest's log. Now each such write is labelled, bound to a stated authority, and checkable from the guest's bundle alone.
- **Operators are shielded.** Guests see the instance operator, never the person, and the person is recorded where the instance's own operators can hold each other to account.
- **Evaluated acts are preferred,** so most instance authority leaves no trace in guests' logs at all; it is applied at read and write time and disappears when the instance rule does.
- **Binding is uniform.** `ts-locked`, unliftable global blocks and irreversible erasures are three cases of one rule, with one refusal code.
- **Global filters move partitions** before any code exists for them: from the primary tenant's `log` to the instance `log`.
- **Exports carry a little more:** the authority extract, with operators' identities withheld.

## Open questions

- **Multiple operators' consent.** Whether some written prerogatives, such as expunge or tenant deletion, should need two operators' authority records before the instance acts.
- **Guest notice.** Whether a guest's administrators should be notified of every instance act on their tenant, or only of binding ones, and through which channel.
- **Appeals.** Whether an authority record should carry a structured appeal route (a contact, a deadline) that the guest's UI shows beside the act.
- **Scatterbase's server acts**, if Scatterbase wants the same distinction between a server's own claims and its users'.

## References

- [Wikimedia Foundation Office actions](https://foundation.wikimedia.org/wiki/Policy:Office_actions) (actions taken under a role account)
- [C2SP signed note](https://c2sp.org/signed-note) (the instance key's signature format)
