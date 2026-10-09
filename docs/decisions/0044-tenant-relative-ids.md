# 0044. Tenant-relative IDs

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-09 (A4)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0017](0017-entity-id-grammar.md)
- **Uses:** [0012](0012-api-requirements.md), [0018](0018-tenants.md), [0022](0022-federation.md), [0043](0043-lua-modules.md)
- **Chapters:** [04](../architecture/04-entities-and-identifiers.md), [10](../architecture/10-pages-and-content-models.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

## Context

[0017](0017-entity-id-grammar.md) §1 gives every minted entity ID one of two shapes: a **local** ID, one letter and the rest (`Q5`), and a **foreign** ID, a two-letter provider code, a type letter and the rest (`WDQ42`). A local ID is local to the tenant that reads it ([0018](0018-tenants.md) §5), and a tenant becomes addressable from elsewhere only when it takes a provider code.

[0043](0043-lua-modules.md) §8 lets a tenant make a bare `Q5` in Lua mean another provider's `Q5`, so that modules written for Wikipedia read Wikidata. That leaves the tenant's own Q5 needing a name that cannot be remapped.

James's direction, from the design discussion of 2026-09-30: **`QQQ`, `PPP` and so on always mean the tenant's own data.** This is also useful wherever someone needs three-letter prefixes across the board: a tool that stores IDs from several providers in one column, or parses every ID as a provider code and a type letter, can write the tenant's own entities in the same shape.

## Decision

### 1. The tenant-relative form (extends 0017 §1)

*Changed by A4.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §2.1, §2.3, §5.2, §6.*

### 2. The doubled codes are reserved (extends 0017 §2)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §2.2, §2.3.*

### 3. Where it is accepted (uses 0012 §4 and §5)

*Current text: [18](../architecture/18-api.md) §1.4, §3.4, §4.*

### 4. Federation and other tenants (uses 0022 §2)

*Current text: [17](../architecture/17-federation-and-publication.md) §1.6.*

### 5. Titles (extends 0008 §3)

*Current text: [10](../architecture/10-pages-and-content-models.md) §2.1.*

### 6. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **Every minted ID can be written with a three-letter prefix.** A tool that wants one shape writes `QQQ5` for the tenant's own and `WDQ42` for Wikidata's.
- **Remapping bare IDs in Lua is safe,** because the tenant's own entities always keep a name.
- **Nothing stored changes.** The form is parsed away at the edge, so logs, hashes, dumps and federation are untouched.
- **Twenty-six provider codes are gone for good.** There are 676 two-letter codes; the registry uses eight.

## Open questions

- **Q1. Output in the tenant-relative form on request.** Whether a client may ask the API to write every local ID as `QQQ…`, so that output has three-letter prefixes across the board as input already can. Doing so would need an additive parameter under [0012](0012-api-requirements.md) §1's rules.
- **Q2. Keyed IDs** (`domain:…`) have no prefix of this shape, and are left as they are.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §6 | extends | 0005 A44 |
| [0008](0008-namespaces-and-document-pages.md) §3 | §5 | extends | 0008 A14 |
| [0017](0017-entity-id-grammar.md) §1, §2 | §1–2 | extends | 0017 A6 |

## References

- `docs/registry/providers.toml` (allocated codes)
- [Wikibase entity IDs and federation](https://doc.wikimedia.org/Wikibase/master/php/docs_topics_entitysources.html)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-30
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §6
- **Summary:** 0005 §2 is the one crate table CI checks, and carries both changes this section listed (0005 A44).

Replaced text (§6):

> | Crate | Change |
> |---|---|
> | `scatter-wikibase-model` | The ID parser accepts the tenant-relative form and returns the local ID (§1) |
> | `scatter-providers` | The reserved doubled codes, rejected when a provider is registered (§2) |
>
> No crate is added.

### A2. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §6
- **Summary:** A1 was folded into the Decision. The open questions were numbered. No decision changed. Before this, A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A3. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§6
- **Summary:** The Decision's current text now lives in the architecture chapters [04](../architecture/04-entities-and-identifiers.md), [10](../architecture/10-pages-and-content-models.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A4. Naming a tenant's entity from elsewhere

- **Date:** 2026-10-09
- **Source:** [0080](0080-tenants-as-entity-sources.md) §7
- **Change:** amends §1
- **Summary:** A tenant's entity is named from elsewhere by its IRI, and on a tenant that reads it by that tenant's name for it: the registry code where there is one (`LBQ6`), otherwise the reader's source name (`lb:Q6`).

Replaced text (§1, in [04](../architecture/04-entities-and-identifiers.md) §2.3):

> To name a tenant's entity from elsewhere, use the tenant's provider code ([0018](0018-tenants.md) §5, in [08](../architecture/08-tenants-and-instances.md)).
