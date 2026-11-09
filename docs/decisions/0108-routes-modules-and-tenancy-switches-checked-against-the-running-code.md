# 0108. Routes, modules and tenancy switches checked against the running code

- **Status:** Accepted
- **Date:** 2026-11-09
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0028](0028-tenancy-policy.md), [0107](0107-settings-and-log-events-checked-against-the-running-code.md)
- **Uses:** [0105](0105-catalogues-checked-against-the-running-code.md)

## Context

[0107](0107-settings-and-log-events-checked-against-the-running-code.md) made one probe for every catalogue the code acts on, covering permissions, settings and log events. Its first run found a tenancy switch, `search.farm_wide`, read as a setting, so that Special:Version always reported it as its default. Its open questions asked whether the probe should also report the tenancy switches a run reads (Q2) and the routes and Action API modules a run serves (Q1). The route and module catalogues already had a *Built* column, derived from the OpenAPI contract and the paraminfo lists, but those say what is declared, not what a test run answered.

The module catalogue listed only the modules Triplespace adds or changes. Five modules the API serves unchanged (`action=help`, `action=query`, `meta=siteinfo`, `meta=tokens`, `meta=wikibase`) had no row, so a strict check of the modules served would have failed on them.

### Direction

James's direction, from the design discussion of 2026-11-09: **yes** to both of 0107's open questions, the routes and modules served (Q1) and the tenancy switches read (Q2).

## Decision

### 1. The routes and modules served are observed (amends 0012 §4; extends 0012 §5)

#### 1.1 Modules

*Current text: [18](../architecture/18-api.md) §2.3.*

#### 1.2 Routes

*Current text: [18](../architecture/18-api.md) §3.2.*

### 2. The tenancy switches read are observed (extends 0028 §1)

*Current text: [08](../architecture/08-tenants-and-instances.md) §5.1.*

### 3. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **The module catalogue lists every module served**, unchanged ones included, so that a module the API serves without a row fails CI. Five rows were added.
- **"Not observed" for routes and modules means "not exercised by a test run"**, beside the *Built* column's "declared": a route the contract serves that no test calls shows as not observed.
- **A tenancy switch read as a setting, or a misspelt switch, fails CI.** The ceilings of `bound = "ceiling"` settings are tenancy records too, catalogued in `settings.toml`, and count as listed; the `tenancy:locked` list is not a switch and is not reported.
- **The first runs found nothing unlisted** once the report compared route parameters regardless of their names, as the route catalogue's *Built* column does, and counted the ceilings: the eight routes and ten switches it first flagged were the check's mistakes, not the code's.
- **Routes outside `rest.php/triplespace/v0`** (`api.php`, the site, the auth base, `/.well-known`) are not reported: the route catalogue covers only the REST API's.

## Open questions

- **Q1. Routes outside the REST API.** The site's routes (`triplespace-web routes`), `/.well-known` and the auth base have no catalogue checked against the running code. Whether to give them one.

## References

- [0107](0107-settings-and-log-events-checked-against-the-running-code.md) Q1, Q2; [0105](0105-catalogues-checked-against-the-running-code.md) §1
