# 0079. Issuer codes derived from the founding record

- **Status:** Proposed
- **Date:** 2026-10-09
- **Updated:** 2026-10-09 (A1)
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

A tenant's **founding record** is the record at offset 0 of its `config` partition: the `key:` record that opens its key chain, a copy of the instance key current when the tenant was created ([0018](0018-tenants.md) §2). The tenant's **issuer code** is derived from that record's Merkle leaf (§2), and is the code its actor keys carry: `{code}:{id}`. It is no longer the tenant's slug.

The code is fixed at creation and never changes:

- **A move doesn't touch it.** It appends a rotation to the key chain and never touches offset 0 ([0018](0018-tenants.md) §10).
- **Erasure doesn't either.** It removes parts but leaves the leaf ([0006](0006-log-integrity-and-erasure.md) §3).
- **Nothing allocates it.** No registry of instances is consulted, and no instance can take another's code except by a hash collision.

The code names the tenant. Control of it is the key chain. Anyone can copy a tenant's founding record from its public bundle and so present the same code, but the record names a key the copier does not hold. The copier can therefore sign no checkpoint, rotation or record that verifies against it. This is the model of `did:plc`, whose identifier is a truncated hash of a signed genesis operation.

**Examples write the slug.** In prose, ADRs and chapters write a tenant's actor keys with its slug: `librarybase:42` stands for `{Librarybase's issuer code}:42`. `local` still reads as the current tenant's issuer ([0018](0018-tenants.md) §4), and the `{tenant}` issuer template of `issuers.toml` is instantiated with the code.

### 2. The derivation (amends 0007 §1)

*Changed by A1.*

**The code is the first 26 characters of the lower-case RFC 4648 base32 encoding, without padding, of the founding record's Merkle leaf hash**, `H(0x00 ‖ header)` ([0006](0006-log-integrity-and-erasure.md) §2). That is 130 bits, written in `a`–`z` and `2`–`7`, for example `vk3q7zcx2m4hrt6wd5nbfy2lpa`.

- **Why the leaf.** It is already public, carried in every inclusion proof, and survives erasure. It covers the header: the partition's 64 random bits, the time and the body commitment, whose parts are each salted with 16 random bytes ([0015](0015-record-format-and-partition-registry.md) §1). No new domain tag and no new field are needed.
- **Why 130 bits.** An instance that wanted two tenants with one code, to show different histories under one name, would need about 2^65 attempts. A forger matching an existing code would need about 2^130. `did:plc` uses 120 bits. 26 characters is also a whole number of base32 characters.
- **Registered codes are shorter.** An issuer code in `issuers.toml` is at most 25 characters, so a registered code can never equal a derived one. `scatter-actors` refuses a longer registered code at build time.
- **The founding record carries the founding attestation**, the third attestation form ([0015](0015-record-format-and-partition-registry.md) §1): no actor, a signature by the key the record itself registers, so the record is self-certifying, and for a tenant the authority `(instance config, 0)`, the instance's own founding record. The code is derived from the leaf afterwards, so nothing that depends on the code, an actor key or an authority record of the tenant's own, has to exist before it.

### 3. The slug is a name (amends 0018 §1, §9 and §10)

A tenant's slug names it on its instance:

- in the instance-scope `tenant` and `graph` records that register it and its partitions ([0015](0015-record-format-and-partition-registry.md) §3);
- in hostnames by convention, in the CLI, and in the API's tenant list;
- in `providers.toml` if it becomes a provider ([0018](0018-tenants.md) §5).

It is chosen when the tenant is created, is unique on the instance, and may not take a name the registry has: tenant slugs, provider slugs and registered issuer codes remain one namespace on an instance. **It is in no actor key and in no record of the tenant's own partitions**, since the records that hold it are instance-scope and do not travel ([0018](0018-tenants.md) §10). So two instances may each have a `librarybase`. A tenant that arrives at an instance where its slug is taken is registered under another, and nothing in its bundle changes.

A change of base remains an `alias` record ([0018](0018-tenants.md) §9). It changes no actor key, since actor keys carry neither the host nor the slug.

### 4. The farm code (amends 0015 §1, 0028 §2, 0040 §2, §3 and §7, and 0046 §5 and §6)

*Changed by A1.*

**The farm code** is derived as in §2 from the instance's founding record: its first `key:` record, at offset 0 of the instance `config` partition ([0015](0015-record-format-and-partition-registry.md) §3). Every instance has one, whether or not a farm issuer exists. It is what the farm's keys carry:

- the operator's actor key, now `instance:{farm code}` ([0040](0040-instance-prerogatives.md) §2), in instance attestations ([0040](0040-instance-prerogatives.md) §3), log projections and contributions ([0040](0040-instance-prerogatives.md) §7);
- the farm issuer's code, so farm accounts are `{farm code}:{id}` ([0028](0028-tenancy-policy.md) §2);
- the per-issuer names of the farm partitions, `actors/{farm}`, `accounts/{farm}` and `log/{farm}`, where `{farm}` is the farm code.

**Offsets 1 and 2 of the instance `config`**, the primary tenant's `tenant:` record and the first `primary` record, are attested by `instance:{farm code}` with the founding record as authority, which exists by then. A single-tenant instance therefore needs no authority record of its own and no `ts-prerogative` path to exist before its first entity is written.

**The farm slug stays the instance's readable name** ([0046](0046-primary-tenant.md) §6). It is chosen at `instance create --farm-slug`, is unique in the instance's namespace, differs from every tenant slug and never changes, but it is in no key. A primary transfer leaves the operator's key unchanged, as before ([0046](0046-primary-tenant.md) §5).

### 5. Verifying and moving a tenant (extends 0018 §10)

`verify` recomputes the tenant's issuer code from the founding record in the bundle, and checks that the actor keys of the tenant's own actors use it. With the instance attestation, this is how a verifier recognises the records [0018](0018-tenants.md) §4 allows in a tenant's partitions. On arrival, the receiving instance registers the tenant under its existing code and the slug of its choice (§3). Account reclaiming is unchanged ([0018](0018-tenants.md) §10): a reclaimed account keeps its actor key because the code travelled.

### 6. Adopted accounts (amends 0007 §3 and 0035 §5)

An adopted wiki's accounts are written under the adopting tenant's issuer code and their own numbers. The source's user 42 is `{code}:42` ([0035](0035-adopting-a-wikibase.md) §5).

### 7. API (extends 0018 §11, 0028 §11 and 0046 §9)

- **The tenant's own siteinfo.** `meta=siteinfo&siprop=triplespace` reports the tenant's issuer code beside its slug.
- **At the farm base,** the same siteinfo reports the farm code beside the farm slug and the primary tenant's slug.
- **`GET /tenancy`** includes the farm code. Its `tenants` array gives each tenant's issuer code, as `issuer`, beside slug, host, name and status.

A client can thus turn any actor key it meets into a tenant on the instance.

### 8. Crates (extends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-actors` | The derivation of §2 from a leaf hash; `IssuerRegistry::for_tenant` and `for_farm` keyed by code; the 25-character limit on registered codes |
| `triplespace-cli` | `instance create` derives the farm code, and tenant creation each tenant's issuer code, from the founding record once it is appended |

### 9. A wiki adopted under a registered issuer (amends 0035 §5)

The registry may already know a wiki as an issuer before a tenant adopts it. Librarybase's MediaWiki is `librarybase` in `issuers.toml`, and other instances' mirrors attribute its revisions to `librarybase:42` ([0007](0007-actor-identity.md) §1). After adoption, that issuer's accounts and the tenant's are the same accounts, under two codes ([0035](0035-adopting-a-wikibase.md) §5).

- **The registry entry gains `tenant`**, the adopting tenant's issuer code, once the tenant exists.
- **`{registered code}:{n}` becomes an input form of `{tenant code}:{n}`** wherever an actor key is accepted: canonicalized, never stored, never output, as `wikidata:Q42` is for `WDQ42`.
- **Records written before the adoption keep their keys.** A reader that meets `librarybase:42` in a mirror and `{code}:42` in the tenant's own records reads one account, by the entry's `tenant` field.

On Librarybase, the example convention of §1 is therefore literal.

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

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §8 | extends | 0005 A86 |
| [0007](0007-actor-identity.md) §1 | §1, §2 | amends | 0007 A21 |
| [0007](0007-actor-identity.md) §3 | §6 | amends | 0007 A21 |
| [0015](0015-record-format-and-partition-registry.md) §1 | §4 | amends | 0015 A46 |
| [0018](0018-tenants.md) §1 | §3 | amends | 0018 A20 |
| [0018](0018-tenants.md) §2 | §1 | extends | 0018 A20 |
| [0018](0018-tenants.md) §4 | §1 | amends | 0018 A20 |
| [0018](0018-tenants.md) §9 | §3 | amends | 0018 A20 |
| [0018](0018-tenants.md) §10 | §3, §5 | amends | 0018 A20 |
| [0018](0018-tenants.md) §11 | §7 | extends | 0018 A20 |
| [0028](0028-tenancy-policy.md) §2 | §4 | amends | 0028 A16 |
| [0028](0028-tenancy-policy.md) §11 | §7 | extends | 0028 A16 |
| [0035](0035-adopting-a-wikibase.md) §5 | §6, §9 | amends | 0035 A10 |
| [0040](0040-instance-prerogatives.md) §2 | §4 | amends | 0040 A5 |
| [0040](0040-instance-prerogatives.md) §3 | §4 | amends | 0040 A5 |
| [0040](0040-instance-prerogatives.md) §7 | §4 | amends | 0040 A5 |
| [0046](0046-primary-tenant.md) §5 | §4 | amends | 0046 A7 |
| [0046](0046-primary-tenant.md) §6 | §4 | amends | 0046 A7 |
| [0046](0046-primary-tenant.md) §9 | §7 | extends | 0046 A7 |

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
