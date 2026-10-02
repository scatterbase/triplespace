# 0044. Tenant-relative IDs

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-01 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0017](0017-entity-id-grammar.md)
- **Uses:** [0012](0012-api-requirements.md), [0018](0018-tenants.md), [0022](0022-federation.md), [0043](0043-lua-modules.md)

## Context

[0017](0017-entity-id-grammar.md) §1 gives every minted entity ID one of two shapes: a **local** ID, one letter and the rest (`Q5`), and a **foreign** ID, a two-letter provider code, a type letter and the rest (`WDQ42`). A local ID is local to the tenant that reads it ([0018](0018-tenants.md) §5), and a tenant becomes addressable from elsewhere only when it takes a provider code.

[0043](0043-lua-modules.md) §8 lets a tenant make a bare `Q5` in Lua mean another provider's `Q5`, so that modules written for Wikipedia read Wikidata. That leaves the tenant's own Q5 needing a name that cannot be remapped.

James's direction, from the design discussion of 2026-09-30: **`QQQ`, `PPP` and so on always mean the tenant's own data.** This is also useful wherever someone needs three-letter prefixes across the board: a tool that stores IDs from several providers in one column, or parses every ID as a provider code and a type letter, can write the tenant's own entities in the same shape.

## Decision

### 1. The tenant-relative form (extends 0017 §1)

**A type letter written three times, then the rest of a local ID, names that local entity of the tenant where it is read:**

| Written | Means, on the tenant reading it |
|---|---|
| `QQQ5` | `Q5` |
| `PPP31` | `P31` |
| `LLL3` | `L3` |
| `MMM1234` | `M1234`, the MediaInfo ID of File page 1234 ([0041](0041-content-models.md) §7) |

It has the shape of a foreign ID whose provider code is the type letter doubled. **It is an input form of the local ID, not a fourth form:**

- **It is never stored.** It is canonicalized to the local form wherever an ID is accepted, before anything is validated, hashed, appended or cached. No record, `view` row, cache key, search document or dump contains it.
- **It is never the canonical output.** Responses use the local form, `Q5`. The one place the tenant-relative form is written out is Lua under a mapped letter ([0043](0043-lua-modules.md) §8), where the bare form already means something else.
- **It is relative.** `QQQ5` means a different entity on each tenant, as `Q5` does. To name a tenant's entity from elsewhere, use the tenant's provider code ([0018](0018-tenants.md) §5).

Input is case-insensitive, as every ID is ([0017](0017-entity-id-grammar.md) §2): `qqq5` is `Q5`.

### 2. The doubled codes are reserved (extends 0017 §2)

**The twenty-six provider codes `AA` through `ZZ` are reserved** in `providers.toml` and are never allocated. A provider with code `QQ` would make `QQQ5` mean two things. No allocated code is a doubled letter today (`WD`, `LB`, `OA`, `MB`, `XD`, `OS`, `OW`, `GD`), so nothing moves.

Reserving all twenty-six, not only the letters local types use today, means a type letter added later already has its tenant-relative form.

**The rest of the ID follows the local type's grammar**, which for every local type is `digits`. `QQQb10bbbfc-…` is not an ID.

### 3. Where it is accepted (uses 0012 §4 and §5)

**Everywhere an entity ID is accepted on input:**

- the Action API: `wbgetentities&ids=`, entity values and IDs in `wbeditentity` and the statement modules, `Special:EntityPage/` and `Special:EntityData/`;
- REST under `triplespace/v0` and the Wikibase REST API's path segments;
- the change-set wire format of bulk jobs ([0002](0002-source-graphs-and-mass-ingest.md) §8), canonicalized during validation;
- `GET /resolve` and the search box's "Go to" ([0012](0012-api-requirements.md) §5);
- Lua and `#property`/`#statements` ([0043](0043-lua-modules.md) §8).

### 4. Federation and other tenants (uses 0022 §2)

**A tenant-relative ID never leaves the tenant,** because it never reaches the log. Rewriting between tenants and instances ([0018](0018-tenants.md) §5, [0022](0022-federation.md) §2) sees only local and foreign IDs, as before. A bulk job submitted to tenant A with `QQQ5` writes A's `Q5`.

### 5. Titles (extends 0008 §3)

**The `entity-id` normalizer accepts the form** ([0008](0008-namespaces-and-document-pages.md) §3): `Item:QQQ5` normalizes to `Item:Q5`, as `Item:q5` already does, and the title resolver serves the canonical title. This is normalization, not a redirect record.

### 6. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with both changes this section listed. The table this section first gave is in A1.

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
