# 0059. The query service

- **Status:** Proposed
- **Date:** 2026-10-04
- **Updated:** 2026-10-09 (A6)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0024](0024-subsidiary-accounts.md), [0028](0028-tenancy-policy.md), [0032](0032-sparql-update-stream.md), [0033](0033-backend-stack.md), [0047](0047-special-pages.md), [0056](0056-security-model.md)
- **Uses:** [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0045](0045-table-content-model.md), [0049](0049-boards.md), [0060](0060-scopes.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)
- **Chapters:** [02](../architecture/02-graphs-rdf-and-query.md), [08](../architecture/08-tenants-and-instances.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

[0013](0013-postgres-storage.md) §8 made Postgres the serving model and demoted the quad store to a deployment option: an instance that wants its own SPARQL endpoint feeds Oxigraph or QLever from `view.rdf_delta`, the rows the SPARQL Update stream of [0032](0032-sparql-update-stream.md) serializes, and "nothing in the API reads from it". That rule kept the application off a store that lags and holds the public form only, and it has held through every ADR since.

Two ADRs have since asked for something only a graph store answers well. [0045](0045-table-content-model.md) Q1 wants a table's rows to come from a query: every entity with a given statement, the members of a category, the entities in another table's column, "a restricted query language". [0049](0049-boards.md) §14 wants a board to gather threads by the same kinds of selection, and fixes the rule that selection is a projection while attachment is a record. A single statement pattern is a lookup in the statement indexes. A join (`P31` = `Q5` and `P21` = `Q6581072` and a `P19` in a given country), a property path (`P31/P279*`), or an intersection of two such sets is a query plan, and `view` is not shaped to plan it. The shared storage contract of [0005](0005-crate-organization.md) §4.4 is `lookup(s, p, o, g)` for a reason: the thing that plans queries over triples is a triplestore, and the instance already knows how to keep one current.

QLever is also where the data is going. [0033](0033-backend-stack.md) §11 names it as the first export destination; `triplespace-cli sparql-sync` is tested against it. A store the instance is already feeding for others can answer questions for the instance itself.

### Direction

James's direction, from the design discussion of 2026-10-04:

- **The graph database should be available for use within Triplespace.** QLever or another store is a target destination for the data; it should also serve the instance. Scope-based tables were the first contemplated use.
- **One `full` store with graph injection**, not a store per tenant. Operating several QLever instances across tenants would not scale.
- **A store per tenant stays possible** where tenant separation is highly desired, so long as it never means several QLevers.
- **Raw SPARQL for people in version 1.**
- **Embedded Oxigraph is the default** store when the service is on.

## Decision

### 1. One reader of the quad store (amends 0013 §8; extends 0032 §6)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §7.1.*

### 2. Two backends (amends 0033 §1 and §11)

*Changed by A2, A3, A4.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §7.2.*

### 3. What the store holds, and how fresh it is

*Changed by A2.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §7.3.*

### 4. Tenant isolation: by dataset, or by store (extends 0028 §12; extends 0056 §3 and §10)

*Changed by A3.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §9.1.*

### 5. Compiled queries

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §7.4.*

### 6. Raw SPARQL for people (extends 0012 §5; extends 0047 §9)

*Changed by A1.*

*Current text: [18](../architecture/18-api.md) §3.2, §6; [19](../architecture/19-site-ui.md) §6.9.*

### 7. Operations

*Current text: [22](../architecture/22-crates-and-stack.md) §4.7.*

### 8. Crates (amends 0005 §2 and §4.4)

*Changed by A5.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **Compile scopes to SQL over `view.statement_assertion` and `page_category`.** Right for one pattern, which [0060](0060-scopes.md) keeps on the indexes. For joins and paths it is a query planner over tables shaped for serving pages, written and maintained here. The triplestore is that planner, already fed.
- **A store per tenant as the only model.** Forty tenants mirroring Wikidata would hold forty copies of Wikidata, and `remote` would mean forty QLevers. Rejected by direction; kept as `store` isolation for the tenant that wants it, embedded only.
- **Rewriting the query text to restrict graphs**, instead of the protocol's dataset. Rewriting has to be right for every SPARQL construct; the protocol's dataset is a one-line contract both stores honour. `FROM` is refused rather than rewritten for the same reason.
- **Serving the application from the store.** Rejected in [0013](0013-postgres-storage.md) and still: it lags, it holds public form only, and the application's views are rows.
- **No raw SPARQL, compiled queries only.** Safer to operate, but a wiki of structured data without a query page is not what anyone coming from Wikidata expects; the direction was to ship it in version 1, bounded.

## Consequences

- **The instance can answer graph questions about its own data,** at the cost of a second copy of the public graphs and the lag of a consumer. Tables and boards gain query scopes through [0060](0060-scopes.md); later ADRs can compile other things.
- **One store serves a farm.** Dataset isolation makes the shared store the common case; `store` isolation is there for the tenant that needs it, without a second QLever.
- **The public form is the only thing queryable.** Nothing restricted is ever in a store, so no query result needs filtering; the price is that privileged readers find less than they could read.
- **Lag is visible everywhere a query result is shown**, as a cursor and an age, and the service refuses to pretend to be current past `query.max_lag`.
- **Test plan.** The round-trip test of [0032](0032-sparql-update-stream.md) (dump, apply stream, compare with a later dump) gains a `full`-form run against both backends and a dataset-isolation test: two tenants, one query each, no solution from the other's graphs under any of `GRAPH ?g`, `FROM` (refused) or a bare pattern. `full` on QLever is benchmarked here, which 0033 §11 asked for.
- **`instance check` gains two lines** (0056 §10) and the embedded store's directory is inside the deployment boundary.

## Open questions

- **Q1. Private tenants.** Whether a tenant with a `read` restriction may run the query service under `store` isolation, with an embedded store fed from its deltas in the tenant-visible rather than the public form and read only through the server, which applies the tenant ACL before answering. This needs [0032](0032-sparql-update-stream.md) to compute deltas for a tenant it does not stream.
- **Q2. `SERVICE`.** Federated queries to Wikidata's endpoint and others are what makes many Wikidata queries useful. Refused in version 1 because the store makes the outbound request; an allow-list of endpoints (`query.services`) is the likely answer.
- **Q3. Query results as tables.** Whether `Special:Query` should save a result as a `triplespace-table` whose rows are the query, which [0060](0060-scopes.md) §6 nearly gives already.
- **Q4. Cursor-stamped reads.** Whether `/sparql` should accept `cursor=` and refuse to answer from a store behind it, so that a client that just wrote can know when its write is queryable.
- **Q5. Embedded store size.** Oxigraph's limits on a Wikidata-sized `full` form are not known from experience here; the bootstrap audit of [0013](0013-postgres-storage.md) should measure the embedded store beside `entity_ref` and `rdf_delta`.
- **Q6. Metadata graph queries.** Whether raw SPARQL over the metadata graph (revision nodes, actors) needs a bound of its own, since it is the one graph with a row per revision.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2, §4.4 | §8 | amends | 0005 A62 |
| [0012](0012-api-requirements.md) §5 | §6 | extends | 0012 A39 |
| [0013](0013-postgres-storage.md) §8 | §1 | amends | 0013 A32 |
| [0024](0024-subsidiary-accounts.md) §5 | §6 | extends | 0024 A11 |
| [0028](0028-tenancy-policy.md) §12 | §4 | extends | 0028 A10 |
| [0032](0032-sparql-update-stream.md) §6 | §1 | extends | 0032 A4 |
| [0033](0033-backend-stack.md) §1, §11 | §2 | amends | 0033 A11 |
| [0047](0047-special-pages.md) §9 | §6 | extends | 0047 A6 |
| [0056](0056-security-model.md) §3, §10 | §4 | extends | 0056 A3 |

## References

- [SPARQL 1.1 Protocol](https://www.w3.org/TR/sparql11-protocol/), §2.1.4 "Specifying an RDF Dataset"
- [SPARQL 1.1 Query Language](https://www.w3.org/TR/sparql11-query/), §13 "RDF Dataset"
- [Oxigraph](https://github.com/oxigraph/oxigraph): `Store::query_opt` with `QueryDataset`; the `spargebra` crate
- [QLever](https://github.com/ad-freiburg/qlever): SPARQL 1.1 Update and the Graph Store Protocol (June 2025); `qlever update-wikidata`
- [Wikidata Query Service: User Manual](https://www.mediawiki.org/wiki/Wikidata_Query_Service/User_Manual), the pre-declared prefixes and the 60-second timeout
- [0032](0032-sparql-update-stream.md), the stream this service consumes; [0045](0045-table-content-model.md) Q1 and [0049](0049-boards.md) §14, the uses that asked for it

## Amendment log

### A1. Save as Query page

- **Date:** 2026-10-05
- **Source:** [0063](0063-query-namespace.md) §5
- **Change:** extends §6
- **Summary:** `Special:Query` saves its text as a `Query:` page.

### A2. The store holds `full` with `metadata=local`; `query.metadata`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §2, §3
- **Summary:** The query service's store holds the `full` form with `metadata=local` by default, for both backends. `query.metadata = all` is a query-service setting that makes the embedded worker apply every delta and the QLever loader take the `all` dump. (PENDING B9)

### A3. `store` isolation with a remote backend on a single-tenant instance

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §2, §4
- **Summary:** `store` with `remote` is refused only on a farm; on a single-tenant instance the remote store holds one tenant's graphs and so satisfies `store`. `query.isolation` is a row of [0028](0028-tenancy-policy.md) §1's switch table: the `isolated` preset sets `store`, the shared presets `dataset`. (PENDING C9)

Replaced text (§2):

> | `remote` | Any SPARQL 1.1 Protocol endpoint; QLever first | `triplespace-cli sparql-sync` ([0032](0032-sparql-update-stream.md) §6) run by the operator against the instance's own `full` stream, or QLever's own `update` tooling ([0033](0033-backend-stack.md) §11) | `dataset` only |

Replaced text (§4):

> `store` is **only ever embedded**: the setting is refused with `remote`, because a store per tenant would mean a QLever per tenant, which this ADR exists to avoid.

### A4. Backend settings are deployment configuration

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §2
- **Summary:** `query.backend`, `query.path` and `query.endpoint` are deployment configuration ([0033](0033-backend-stack.md) §12), not `site` records. (PENDING D5)

### A5. `triplespace-query` is a surfaces-layer crate

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §8
- **Summary:** `triplespace-query` is in the surfaces layer (layer 4), not layer 3. (PENDING F25)

Replaced text (§8):

> | `triplespace-query` | **New, layer 3.** The `QueryService` trait; the embedded backend over `scatter-quadstore` and the remote backend over the SPARQL Protocol; dataset computation from a tenant's provider policy; the prefix set; the envelope of §5 (dataset, `LIMIT`, order) and the checks on user SPARQL; the cursor marker. Depends on `triplespace-db`, `scatter-quadstore` (feature `oxigraph`), `scatter-wikibase-rdf`, `scatter-vocab`, `spargebra` for parsing and the `FROM`/`SERVICE` checks, `reqwest` for the remote backend |

### A6. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [08](../architecture/08-tenants-and-instances.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
