# 0080. Provider tenants are entity sources

- **Status:** Proposed
- **Date:** 2026-10-09
- **Updated:** 2026-10-30 (A3)
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

*Changed by A1.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.1, §6.8.*

### 2. Reading a tenant source (amends 0018 §5; extends 0078 §4)

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.4; [08](../architecture/08-tenants-and-instances.md) §4.1, §4.4, §6.3.*

### 3. Registry codes for tenants are promotion (amends 0018 §5; extends 0078 §9)

*Changed by A2.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.8; [08](../architecture/08-tenants-and-instances.md) §4.1.*

### 4. Across tenants, by issuer code first (amends 0078 §8)

*Current text: [05](../architecture/05-providers-and-ingest.md) §6.7.*

### 5. Who may read whom, by issuer code (amends 0028 §5)

*Current text: [08](../architecture/08-tenants-and-instances.md) §4.2, §4.3.*

### 6. Tenant repositories (extends 0052 §1; settles 0078 Q4 in part)

*Current text: [13](../architecture/13-mirrored-pages.md) §1.1.*

### 7. Naming a tenant's entity from elsewhere (amends 0044 §1)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §2.3.*

### 8. API (extends 0078 §10)

*Current text: [18](../architecture/18-api.md) §3.2.*

### 9. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

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

## References

- `claude/derived-issuer-codes.md` (Triplespace project notes, 2026-10-09): the discussion that led here

## Amendment log

### A1. A tenant source's number is in the entity-source range

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §1
- **Summary:** Every ID a client sees is below 2^53: registry provider numbers are below 2^12 and a tenant assigns its entity-source numbers, a tenant source's included, from 2^12 to 2^13 − 1, so `number << 40 | n` stays under 2^53 for `n` below 2^40. The range itself is [0078](0078-entity-sources.md) A4's; §1 says only that a tenant source's `number` comes from it. (REVIEW G20)

### A2. The registry binding is enforced by the hosting instance

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §3
- **Summary:** An instance refuses a sync job for a provider whose registry `issuer` is a tenant of this instance, since the tenant's `local` partition is the source and a mirror of it would be a stale copy, and refuses to adopt or import a tenant under a code for which it holds a live mirror partition until that partition has been frozen: its `entity_source` rows dropped and composition switched to reading the tenant's `local`. The sync refusal is also logged on [0070](0070-shallow-entity-mirroring.md) §7 (0070 A6); the chapters hold it in [08](../architecture/08-tenants-and-instances.md) §4.1 and [05](../architecture/05-providers-and-ingest.md) §5.9 and §8.3. (REVIEW G31)

### A3. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [04](../architecture/04-entities-and-identifiers.md), [05](../architecture/05-providers-and-ingest.md), [08](../architecture/08-tenants-and-instances.md), [13](../architecture/13-mirrored-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
