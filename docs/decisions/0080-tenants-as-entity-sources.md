# 0080. Provider tenants are entity sources

- **Status:** Proposed
- **Date:** 2026-10-09
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0018](0018-tenants.md), [0028](0028-tenancy-policy.md), [0044](0044-tenant-relative-ids.md), [0052](0052-page-repositories-and-title-inheritance.md), [0078](0078-entity-sources.md), [0079](0079-derived-issuer-codes.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0022](0022-federation.md), [0056](0056-security-model.md)

## Context

A tenant became a provider by taking a two-letter code, a slug and a number in `providers.toml` ([0018](0018-tenants.md) §5). Librarybase is `LB`/2, and example.wiki reads its `Q6` as `LBQ6`. A tenant without a code "cannot be referenced. Its IDs have no absolute form."

[0079](0079-derived-issuer-codes.md) took the tenant slug out of identity, because a central registry of every tenant on every instance is "a losing battle". It left the same problem on the entity side as its Q2. The provider registry is that central registry, and it has three limits for tenants:

- **It is small.** Two letters give 676 codes, less the 26 doubled ones reserved by [0044](0044-tenant-relative-ids.md) §2. That leaves 650 provider tenants across every Triplespace instance there will be, so a farm cannot make its wikis readable by each other.
- **It needs a commit.** An independent instance cannot make its own tenant a provider without the upstream repository.
- **Its premise no longer holds.** "No absolute form" stopped being true with [0078](0078-entity-sources.md), under which meaning crosses tenants as an IRI.

[0078](0078-entity-sources.md) already gives a tenant a way to read a graph the registry does not know. The tenant names it (`mhc`), its entities are `mhc:Q1`, their revision IDs are ranged by a number the tenant assigns itself at runtime, and a reference crosses to another tenant by IRI. A provider tenant is such a graph. It differs only in being a Triplespace tenant: it has an issuer code that survives moves, a key chain to verify against and, on the same instance, a partition that can be read directly.

In the discussion of 2026-10-09, James summed up the design: the `lb:` form is a tenant-specific prefix, which keeps the global registry (`LB`) as an exclusive system. The same discussion found that `provider-readers` lists named tenants by slug inside the provider's own `config`, which travels with it. After [0079](0079-derived-issuer-codes.md), such a slug may mean a different tenant after a move.

### Direction

James's direction, from the design discussion of 2026-10-09:

- **"You have the "lb:" style name as a tenant-specific prefix, preserving the global registry ("LB") as an exclusive system."**
- **"You can draft the ADR and/or update the previous one as needed."**

## Decision

### 1. A tenant source (extends 0078 §1; amends 0078 §9)

An `entity-source` record may name a Triplespace tenant instead of describing a graph. It then carries:

| Field | Meaning |
|---|---|
| `tenant` | The provider tenant's issuer code ([0079](0079-derived-issuer-codes.md) §1). Fixed by the first record, with `name` and `number` |
| `base` | Where the tenant is now: its base URI. Writable, and on the same instance followed from the provider's `alias` records ([0018](0018-tenants.md) §9) without a write |

Everything else is read from the provider, not declared:

- **`types`:** the entity types it mints, with their upstream prefixes and IRI templates over its current base.
- **`licence`:** its `content.licence`.
- **`api`, `entity_data` and `events`:** its Action API, `Special:EntityData` and activity stream, all at `base`.

`label` and `namespace` are the reader's, as for any source.

**Binding is to the code.** [0078](0078-entity-sources.md) §9 fixes a source's type IRI templates by its first record. A tenant source instead fixes its `tenant` code, and its templates follow the provider's base. A record that changes `tenant` is refused with `ts-source-binding`.

**A provider that is in the registry is read as a provider.** A tenant source naming a tenant whose code a registry entry names as its `issuer` (§3) is refused with `ts-source-is-provider`, as [0078](0078-entity-sources.md) §9 refuses a source that duplicates a registry provider. The tenant adds the provider to its `providers` list instead.

### 2. Reading a tenant source (amends 0018 §5; extends 0078 §4)

**On the same instance it is read directly,** as a provider tenant was ([0018](0018-tenants.md) §5):

- The provider's `local` partition is the source. There is no `source/{name}` partition, no fetch job and no mirror setting.
- IDs are rewritten on read: Librarybase's `Q6` is `lb:Q6` on a reader that named it `lb`. Revision and page IDs are ranged by the reader's number for the source ([0078](0078-entity-sources.md) §4).
- References to the reader's own entities come back as bare IDs.
- The provider's `same-as` and `convert` records are tier-2 links for the reader ([0004](0004-identity-clusters-and-equivalence.md) §3), ranked where the reader's `reconcile` order puts the source.

**On another instance it is read by `scatter-adapter-triplespace`** ([0022](0022-federation.md) §2), which writes into the reader's `source/{name}` partition as [0078](0078-entity-sources.md) §4 provides, with `trust = verified`. Before the first batch, the adapter checks that the founding record at offset 0 of the provider's key chain derives the source's `tenant` code. That binds the code to the chain it verifies against.

**A move switches between the two** ([0018](0018-tenants.md) §10). Readers on the instance the provider leaves start the adapter into `source/{name}`. Readers on the instance it joins freeze their `source/{name}` and read it directly. What a reader shows does not change.

Cross-tenant discussion, and `tenant` file and page repositories, apply to a provider read as a tenant source as they did to a provider tenant ([0028](0028-tenancy-policy.md) §6; §6).

### 3. Registry codes for tenants are promotion (amends 0018 §5; extends 0078 §9)

**Any public tenant can be read without a registry entry.** Its entities' IRIs are their global names, and each reader names it as a source. A registry code is not a condition of being referenced.

**A tenant gets a code only by promotion,** a commit to `providers.toml` for a tenant that many tenants read. That makes its IDs, `LBQ6`, mean one thing on every instance. **The entry's `issuer` is the tenant's issuer code**, which binds the code to the tenant wherever it is hosted. Readers' tenant sources for it are then promoted as [0078](0078-entity-sources.md) §9 provides, matched by `tenant` code rather than by IRI template.

Librarybase's `LB` stays. Its entry's `issuer` becomes Librarybase's code once Librarybase is a tenant ([0079](0079-derived-issuer-codes.md) §9).

### 4. Across tenants, by issuer code first (amends 0078 §8)

[0078](0078-entity-sources.md) §8 rewrites a provider's reference to a source entity, crossing to a reader, by IRI. For a tenant source, matching is by code, which survives a change of base. Its rules read:

1. **The provider's source is unpublished.** The reference is not readable.
2. **The IRI is a registry provider's, or the source names a tenant that is one.** It becomes the provider's ID.
3. **The reader has a source for the same graph:** one with the same `tenant` code, or, for a source that is not a tenant, the same IRI template. It becomes the reader's ID for the entity.
4. **Otherwise it is unbound,** and its statements are withheld and counted as before.

A reader that reads Librarybase as `lb` and meets Librarybase's reference to its own source `mhc:Q1` applies the same rules.

### 5. Who may read whom, by issuer code (amends 0028 §5)

**`providers.between_tenants`** governs tenant sources that name a tenant of the same instance:

- **`operator`:** only the operator may write such a source, or set `files.share` and `pages.share`. This is the hosting case.
- **`opt-in`:** any tenant may name any public tenant of the instance.

A source naming a tenant on another instance is an ordinary entity source, which no tenancy switch governs.

**Reader lists name tenants by issuer code.** A `provider-readers` record is in the provider's own `config` and travels with it, so it lists issuer codes, not slugs. A slug there would come to mean another tenant after a move ([0079](0079-derived-issuer-codes.md) §3). As before, a list governs reading on the instance, and visibility governs what is read ([0056](0056-security-model.md) §6).

**A private tenant cannot be read as a source,** as it could not take a code. A tenant source naming one is refused with `ts-source-private`.

### 6. Tenant repositories (extends 0052 §1; settles 0078 Q4 in part)

A `tenant` page repository's `provider` is the tenant's registry entry where it has one. Otherwise it is the reading tenant's tenant source for that tenant, which supplies the number for ranged page IDs, the issuer and the IRIs. The source is declared first. Whether a repository may name a source that is not a tenant remains [0078](0078-entity-sources.md) Q4.

### 7. Naming a tenant's entity from elsewhere (amends 0044 §1)

To name a tenant's entity from elsewhere, use its IRI. On a tenant that reads it, use that tenant's name for it: the registry code where there is one (`LBQ6`), otherwise the tenant's source name (`lb:Q6`). `QQQ5` stays relative.

### 8. API (extends 0078 §10)

`GET /entity-sources` gives a tenant source's `tenant` and `base` beside its other fields, and its types as read from the provider.

### 9. Crates (extends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-providers` | The `tenant` and `base` fields of an `entity-source`, their validation and refusals (`ts-source-is-provider` by issuer, `ts-source-private`), and promotion matched by issuer code |
| `triplespace-projections` | Direct reading of a tenant source on the same instance, rewritten to the source's name and number |
| `scatter-adapter-triplespace` | Writing a tenant source into `source/{name}`, and checking the provider's founding record against the source's `tenant` code |

## Alternatives considered

- **Keep codes for every provider tenant and widen the space**, for example to three letters. That gives 17,576 codes, still finite and still a commit each. It also makes `ABCQ5` ambiguous against the tenant-relative and source forms that [0044](0044-tenant-relative-ids.md) and [0078](0078-entity-sources.md) parse by length and case.
- **Codes allocated per instance.** `LBQ6` would mean different things on different instances, which defeats the registry's one purpose.
- **A universal compact form from the issuer code**, `vk3q7zcx2m4hrt6wd5nbfy2lpa:Q6`. It is unique without a registry, but unreadable in stored data, and the IRI already serves as the global form. Left open as Q1.

## Consequences

- **Any public tenant is referenceable,** by its IRIs and by any reader's source name, with no registry change and no operator act beyond the tenancy switch.
- **The registry stays small and exclusive.** A code is promotion for a widely read tenant, as for any other graph.
- **Readers name providers differently.** One tenant's `lb:Q6` may be another's `librarybase:Q6`. Data crosses as the IRI and comes back in each reader's own names.
- **Each reader resolves an unpromoted provider in its own rows** ([0078](0078-entity-sources.md) §4, Views). Many readers of one tenant multiply the cost, which is a reason to promote it. That needs measuring.
- **Reader lists move with their provider** without changing meaning.
- **[0079](0079-derived-issuer-codes.md) Q2 closes.**

## Open questions

- **Q1. A universal compact form.** Whether `{issuer code}:{ID}` should be accepted, or stored, as the instance-independent short form of a tenant's entity, with each reader's name for display.
- **Q2. When to promote.** Who asks for a registry code for a tenant, and on what evidence of use.
- **Q3. Shared rows.** Whether an instance should resolve a heavily read unpromoted tenant once for all its readers on the same instance, as it does for registry providers, without giving it a code.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §9 | extends | 0005 A87 |
| [0018](0018-tenants.md) §5 | §2, §3 | amends | 0018 A21 |
| [0018](0018-tenants.md) §10 | §2 | amends | 0018 A21 |
| [0028](0028-tenancy-policy.md) §5 | §2, §5 | amends | 0028 A17 |
| [0044](0044-tenant-relative-ids.md) §1 | §7 | amends | 0044 A4 |
| [0052](0052-page-repositories-and-title-inheritance.md) §1 | §6 | extends | 0052 A6 |
| [0078](0078-entity-sources.md) §1 | §1 | extends | 0078 A2 |
| [0078](0078-entity-sources.md) §4 | §2 | extends | 0078 A2 |
| [0078](0078-entity-sources.md) §8 | §4 | amends | 0078 A2 |
| [0078](0078-entity-sources.md) §9 | §1, §3 | amends | 0078 A2 |
| [0078](0078-entity-sources.md) §10 | §8 | extends | 0078 A2 |
| [0078](0078-entity-sources.md) Q4 | §6 | settles | 0078 Q4 |
| [0079](0079-derived-issuer-codes.md) Q2 | §3 | settles | 0079 Q2 |

## References

- `claude/derived-issuer-codes.md` (Triplespace project notes, 2026-10-09): the discussion that led here
