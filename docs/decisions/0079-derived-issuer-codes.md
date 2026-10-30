# 0079. Issuer codes derived from the founding record

- **Status:** Proposed
- **Date:** 2026-10-09
- **Updated:** 2026-10-30 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0015](0015-record-format-and-partition-registry.md), [0018](0018-tenants.md), [0028](0028-tenancy-policy.md), [0035](0035-adopting-a-wikibase.md), [0040](0040-instance-prerogatives.md), [0046](0046-primary-tenant.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0022](0022-federation.md), [0027](0027-preferences-and-portability.md)

## Context

An actor key is `{issuer}:{id}`, and a tenant's issuer code was its slug ([0018](0018-tenants.md) §4): an edit on Librarybase was attested by `librarybase:42`, on every instance that ever held it. Actor keys are written into the attestation of every record and are inside the hash, so the slug was permanent identity. [0018](0018-tenants.md) §1 said slugs were "allocated in the registry", and §10 said a moved tenant's slugs map trivially "because slugs come from the same registry".

Neither holds for tenants. The registry is committed TOML in which nothing is allocated at runtime ([0015](0015-record-format-and-partition-registry.md) §5), while tenants are created at runtime by instances that do not know of each other. Two independent instances can each create a `librarybase`. When one of them later moves to an instance where the other lives, nothing resolves the collision, and it cannot be fixed without rewriting the log. The farm slug of [0046](0046-primary-tenant.md) §6 has the same problem: it is in the operator's actor key `instance:{farm slug}`, which appears in the history of every tenant the instance ever acted on.

The question came up in a discussion of identity on 2026-10-09. It began with letting one tenant admit another tenant's actors (a popular bot, a secondary wiki, a small network sharing one user pool, and instance staff), went on to whether user identity should be uncoupled from tenants altogether, and turned on portability: a wiki can leave its host, and its users must not stay the host's users when it does. Reading the ADRs found that most of what portability needs was already decided. IRIs are not in the log, a change of base is an `alias` record ([0018](0018-tenants.md) §9), and leaving a farm keeps per-tenant attribution while the farm only vouches, for a time, as an identity provider ([0028](0028-tenancy-policy.md) §10). The gap was the slug. A key that is permanent and travels with the tenant cannot come from a namespace that neither travels nor is shared.

Three fixes were considered (see Alternatives). James chose to derive the code from the tenant's own log. A first sketch hashed the genesis *key*. That fails on a farm, where every tenant's key chain opens with a copy of the same instance key ([0018](0018-tenants.md) §2). It also proposed adding a random nonce to the founding record. That turned out to be unnecessary, because the record's header already carries the partition's 64 random bits and a commitment over parts salted with 16 random bytes each. So this ADR hashes the founding *record* and adds no field.

The admission of other tenants' actors, and identity pools, are not decided here (Q5).

### Direction

James's direction, from the design discussion of 2026-10-09:

- **"Miraheze (to use a hypothetical example) can vouch for who that person is at the time they were hosting, but they won't continue being a Miraheze user if that wiki becomes independent."**
- **"I agree with slugs derived from the tenant's genesis key. Having a central registry of every possible multitenant instance is a losing battle."**
- **"I don't think the actor key becomes that much less readable because the user component is already an opaque number."**

## Decision

### 1. A tenant's issuer code is derived from its founding record (amends 0007 §1 and 0018 §4; extends 0018 §2)

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.1; [08](../architecture/08-tenants-and-instances.md) §1.2, §2.4, §3.1.*

### 2. The derivation (amends 0007 §1)

*Changed by A1.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.1; [08](../architecture/08-tenants-and-instances.md) §1.2.*

### 3. The slug is a name (amends 0018 §1, §9 and §10)

*Current text: [08](../architecture/08-tenants-and-instances.md) §1.2, §6.1, §6.3.*

### 4. The farm code (amends 0015 §1, 0028 §2, 0040 §2, §3 and §7, and 0046 §5 and §6)

*Changed by A1.*

*Current text: [01](../architecture/01-log-and-records.md) §2.4; [07](../architecture/07-actors-and-accounts.md) §1.1; [08](../architecture/08-tenants-and-instances.md) §1.2, §3.5, §7.5, §8.2, §8.3; [16](../architecture/16-logs-feeds-and-notifications.md) §2.1; [19](../architecture/19-site-ui.md) §2.5, §6.11.*

### 5. Verifying and moving a tenant (extends 0018 §10)

*Current text: [08](../architecture/08-tenants-and-instances.md) §6.3.*

### 6. Adopted accounts (amends 0007 §3 and 0035 §5)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.1, §2.5.*

### 7. API (extends 0018 §11, 0028 §11 and 0046 §9)

*Current text: [18](../architecture/18-api.md) §1.2, §2.2, §3.2.*

### 8. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 9. A wiki adopted under a registered issuer (amends 0035 §5)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.5.*

## Alternatives considered

- **A central registry of tenant slugs**, allocated by scatter.red as provider codes are. It would make every multitenant instance depend on one authority to create a wiki. James: "a losing battle".
- **Refuse a colliding slug on arrival and map it locally.** Keeps the log intact, but an actor key would mean different tenants on different instances, which defeats the point of keys that travel.
- **Rewrite actor keys on a move.** Impossible: keys are inside the hash of every record.
- **Hash the genesis key.** On a farm every tenant's key chain opens with the same instance key ([0018](0018-tenants.md) §2), so every tenant would get the same code.
- **An explicit random nonce in the founding record.** Redundant with the header's random partition ID and the salted body commitment, and a field that exists only at offset 0.
- **24 characters, as `did:plc`.** 120 bits leaves 2^60 for an instance creating two tenants with one code. 26 characters costs two more characters per key.
- **IRIs or DIDs as actor keys.** A key must be an identifier, never content ([0006](0006-log-integrity-and-erasure.md) §3), and a host-based IRI moves when a wiki does. The derived code gives what a DID would give without a resolver.

## Consequences

- **A wiki's identity travels without anyone's permission.** No registry, no host and no other instance is consulted to create, move or receive a tenant. A tenant's slug can collide; its code cannot.
- **The host vouches, the wiki names.** The instance key attests records, and the founding record names the tenant. A wiki that leaves keeps every actor key, and the old host's operator stays attributed under the old farm code, which is right.
- **Actor keys are longer.** About 14 more bytes per key than a short slug. Packed storage ([0058](0058-packed-record-storage.md)) should be measured with it.
- **Examples change meaning, not spelling.** `librarybase:42` in the ADRs is a convention for the derived key (§1).
- **A registered issuer and a tenant can name one account** (§9). Librarybase's `issuers.toml` entry is given its `tenant` code once `instance create --adopt` has run.
- **Milestone 1 is unaffected in order.** Librarybase's tenant and farm codes are derived when `instance create` runs. The farm slug [0046](0046-primary-tenant.md) asked for is still needed, as a name. Test fixtures that build tenant issuers from slugs move to codes.

## Open questions

- **Q1. Renaming slugs.** With no key carrying a slug, whether a tenant's slug or the farm slug may be changed by `owner` beyond reassignment on arrival.
- **Q2.** ~~**Provider codes for tenants.** A tenant that becomes a provider takes a two-letter code and a number from `providers.toml`, and those go into entity IDs (`EXQ9`) that cross instances. This is the same collision problem on the entity side.~~ *Settled by [0080](0080-tenants-as-entity-sources.md) §3: a tenant is read as an entity source named by issuer code, and a registry code is promotion.*
- **Q3. A short display form.** Whether the UI shows an abbreviated code where an actor key must be shown, as git abbreviates commit hashes.
- **Q4.** ~~**Witnessing the founding record.** A code is only as discoverable as the founding record. Whether a checkpoint witness ([0006](0006-log-integrity-and-erasure.md)) should countersign it.~~ *Settled by [0081](0081-recovery-keys-and-continuations.md) §6: the founding record is witnessed with the first checkpoint of a witnessed `config` partition.*
- **Q5. Admitting another tenant's actors and identity pools.** Letting a tenant admit actors of another tenant on the same instance (a bot, a secondary wiki, a network sharing one user pool), and attributing instance staff under the instance. Discussed on 2026-10-09 as a pool being a tenant, possibly contentless, that others admit. Not decided.

## References

- [did:plc method specification](https://web.plc.directory/spec/v0.1/did-plc): identifier as a truncated base32 SHA-256 of the signed genesis operation
- [RFC 4648](https://www.rfc-editor.org/rfc/rfc4648) §6: base32 alphabet
- [RFC 9162](https://www.rfc-editor.org/rfc/rfc9162) §2.1: Merkle leaf hashes

## Amendment log

### A1. The founding attestation

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §2, §4
- **Summary:** The record at offset 0 of any `config` partition carries a third attestation form, the founding attestation: no actor, a signature by the key the record registers (self-certifying), and for a tenant the authority `(instance config, 0)`; the issuer or farm code is derived from its leaf afterwards. Offsets 1 and 2 of the instance `config` are attested by `instance:{farm code}` with the founding record as authority. A single-tenant instance therefore needs no authority record and no `ts-prerogative` path to exist before its first entity. The form itself is [0015](0015-record-format-and-partition-registry.md) §1's; the chapters hold it in [01](../architecture/01-log-and-records.md) §2.4 and [08](../architecture/08-tenants-and-instances.md) §2.4, §7.2 and §8.3. (REVIEW G22)

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [01](../architecture/01-log-and-records.md), [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
