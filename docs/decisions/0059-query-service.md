# 0059. The query service

- **Status:** Proposed
- **Date:** 2026-10-04
- **Updated:** 2026-10-05 (A1)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0024](0024-subsidiary-accounts.md), [0028](0028-tenancy-policy.md), [0032](0032-sparql-update-stream.md), [0033](0033-backend-stack.md), [0047](0047-special-pages.md), [0056](0056-security-model.md)
- **Uses:** [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0045](0045-table-content-model.md), [0049](0049-boards.md), [0060](0060-scopes.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

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

**The quad store gains exactly one reader: the query service.** The service is a trait in a new crate, `triplespace-query` (§8):

```rust
trait QueryService {
    /// Evaluate a read-only SPARQL query against the dataset of one tenant.
    fn query(&self, tenant: TenantId, q: &CompiledQuery, limits: &Limits) -> Result<Solutions, QueryError>;
    /// The last `view.rdf_delta` row the store has applied, as the stream's cursor.
    fn cursor(&self) -> Cursor; // {epoch, seq}
}
```

Everything else [0013](0013-postgres-storage.md) §8 says stands: `view` is the serving model; the store is a consumer of `view.rdf_delta` and never written to by anything else; it holds the public form only ([0032](0032-sparql-update-stream.md) §1); it is never the authority for anything, and a rebuild of `view` rebuilds it (§3). The sentence "nothing in the API reads from it" becomes **"nothing reads from it but the query service (0059)"**. What the service answers, and for whom, is the rest of this ADR.

The service is **optional**: `query.enabled` (`site` configuration, default `false`) turns it on. Every feature that uses it says so, and either degrades to its last result or is absent while the service is off (§3). The one-binary, one-required-service rule of [0033](0033-backend-stack.md) §1 is kept by the embedded backend (§2).

### 2. Two backends (amends 0033 §1 and §11)

| `query.backend` | Store | Fed by | Isolation (§4) |
|---|---|---|---|
| `embedded` (default) | Oxigraph, in process, behind `scatter-quadstore`'s existing feature ([0005](0005-crate-organization.md) §4.4), on a RocksDB directory under `query.path` | `scatter-quadstore::apply(deleted, inserted)` from `view.rdf_delta`, in the projection worker, after each delta batch commits | `dataset` or `store` |
| `remote` | Any SPARQL 1.1 Protocol endpoint; QLever first | `triplespace-cli sparql-sync` ([0032](0032-sparql-update-stream.md) §6) run by the operator against the instance's own `full` stream, or QLever's own `update` tooling ([0033](0033-backend-stack.md) §11) | `dataset` only |

**Embedded is the default** so that turning the service on changes nothing about the deployment: the `triplespace` binary and Postgres. The embedded store is a second copy of the public graphs on the server's disk, grown by `rdf_delta`, and is sized like the `full` dump. A larger instance moves to `remote` when the embedded store's query times or disk no longer suit it; nothing above the trait notices.

**The remote backend is one store.** It is loaded once from the `full` dump and followed from the `full` stream, so that it holds every tenant's graphs under their names (§4). It is never loaded from `view` or `log` directly, which [0056](0056-security-model.md) §10 line 7 already forbids. The endpoint is `query.endpoint`; its credential, if any, is read as every secret is ([0033](0033-backend-stack.md) §12). The service checks the endpoint's cursor by reading a marker triple the stream's consumer writes, `<{base}/.well-known/query> scatter:cursor "{epoch}:{seq}"`, in the store's default graph; `sparql-sync` writes it after each batch.

0033 §11's "QLever is an export destination, not a runtime dependency" becomes **"QLever is an export destination, and optionally the remote backend of the query service"**. The `full` subscription, which 0033 §11 said would be benchmarked before being recommended, is the only form a shared store loads, so that benchmark is now part of this ADR's test plan (Consequences).

### 3. What the store holds, and how fresh it is

**The `full` form** ([0032](0032-sparql-update-stream.md) §1): every public source graph, every tenant's resolved graph and metadata graph, each in its own named graph. A shared provider's source graph (Wikidata's, OpenAlex's) is stored once, since its deltas are stored once (0032 §4), and is part of the dataset of every tenant that opted in to the provider ([0018](0018-tenants.md) §5). This is why one store scales where one per tenant would not: forty tenants mirroring Wikidata share one copy of Wikidata.

**Public form only.** The store holds what the stream carries: read ACLs applied, erased content purged ([0014](0014-caches-and-search.md) §5's sixth step), export policy respected. A query therefore never leaks, whoever asks, and a viewer who may read more than the public sees fewer results than `view` could show them. Accepted: the store is for finding things, and what is found is then read through `view` with the viewer's rights.

**Freshness is a cursor.** `cursor()` is the last `rdf_delta` row applied, in the stream's `{epoch}:{seq}` form. `siprop=triplespace` reports `query: {available, backend, cursor, lag_seconds}`, where lag is the age of the newest delta row not yet applied. Every result the service returns carries the cursor it was computed at, and every stored result (a scope's members, [0060](0060-scopes.md) §5) records it, so that a page can say "as of {time}; {n} changes since". A `view` rebuild increments the epoch ([0032](0032-sparql-update-stream.md) §3) and empties the embedded store, which is then reloaded from a fresh `full` dump by the worker before it resumes applying deltas; a remote store is reloaded by the operator, as any stream consumer is, and the service reports `available: false` while its epoch is behind.

**When the service is unavailable** (off, reloading, the endpoint down, or lag above `query.max_lag`, default 1 h), a compiled query (§5) returns `QueryError::Unavailable` and its caller keeps the last materialized result with its cursor; raw SPARQL (§6) answers `503` with `Retry-After`.

### 4. Tenant isolation: by dataset, or by store (extends 0028 §12; extends 0056 §3 and §10)

**`query.isolation = dataset`** (the default). One store holds every public tenant's graphs. A tenant's **dataset** is the set of graphs its own `full` stream would carry: its resolved graph, its metadata graph, and the source graphs of the providers it has opted in to. The service fixes the dataset through the SPARQL Protocol, `default-graph-uri` and `named-graph-uri` ([SPARQL 1.1 Protocol](https://www.w3.org/TR/sparql11-protocol/) §2.1.4): the resolved graph is the default graph, so a query written for the Wikibase-compatible dump runs unchanged, and the rest are the named graphs, so `GRAPH ?g` ranges over them and nothing else. Oxigraph and QLever both honour the protocol's dataset. A query whose text carries `FROM` or `FROM NAMED` is refused (`ts-query-dataset`), because the protocol says the protocol's dataset overrides the query's and the service does not rely on a store getting that right. The dataset is computed from the tenant's provider policy at each request, so a tenant that drops a provider stops seeing it at once, before the store has forgotten anything.

**`query.isolation = store`.** A tenant may instead have its own embedded store, under `query.path/{tenant}`, fed from its own filtered deltas (the rows its `full` stream would carry), and queried with no other tenant's graphs present. This is for the tenant whose separation requirement is that its queries never touch a store holding anyone else's data, or that another tenant's fault never slows its queries. It costs that tenant its own copy of every provider graph it uses. `store` is **only ever embedded**: the setting is refused with `remote`, because a store per tenant would mean a QLever per tenant, which this ADR exists to avoid. On a farm the two may mix: a tenant `config` setting `query.isolation` overrides the instance default, and a `store` tenant's graphs are excluded from the shared store.

**A private tenant** ([0056](0056-security-model.md) §3) has no graphs in any store, as it has no stream and no public form. It has no query service in version 1; Q1 asks whether a `store` tenant behind the evaluator could have one.

**For 0056.** This adds two lines to the deployment boundary of 0056 §10: the shared store is reachable only from the servers and requires credentials, like OpenSearch (line 2), and every query against it passes through the service, which fixes the dataset; and 0056 §10 line 7 reads "A SPARQL endpoint, QLever or any other external index, **and the embedded query store,** is loaded from the public dump or the update stream, **or from `view.rdf_delta`, which carries the same rows,** never from `view` or `log` otherwise". The embedded store's directory holds public form only and is not a store in the sense of 0056 §10's first paragraph, but it is listed so that `instance check` knows about it.

**For 0028.** The store is instance infrastructure, like OpenSearch ([0028](0028-tenancy-policy.md) §9, §12): one service, every tenant's public data, isolation at query time. The tenancy policy's `isolated` preset ([0028](0028-tenancy-policy.md) §1) sets `query.isolation = store`.

### 5. Compiled queries

**A compiled query is SPARQL the instance wrote.** Its source is a structured definition: a scope's `statement`, `category`, `column`, `query` or set-algebra kinds ([0060](0060-scopes.md) §4), and whatever later ADRs add. The pure crate that owns the definition generates the query body (for scopes, `scatter-scope`, [0060](0060-scopes.md) §11); `triplespace-query::compile` wraps it into a `CompiledQuery`: a `SELECT DISTINCT ?item` with the dataset of §4, a `LIMIT` of the caller's bound plus one (so that truncation is detected), and `ORDER BY ?item` where the caller needs a stable prefix. The result is a set of subject IRIs and nothing else; the service maps each back to an entity or page ID through the IRI rules of [0015](0015-record-format-and-partition-registry.md) §4–6 and drops any it cannot.

A `query`-kind definition carries user-written SPARQL. The compiler **wraps** it rather than trusting it: `SELECT DISTINCT ?item WHERE { { <user query> } } LIMIT n`, after checking that the user query is a `SELECT` that projects `?item`, has no `FROM`, no `SERVICE` (Q2) and no update operation, and after the same prefix handling as §6. A definition that fails these checks is refused at save ([0060](0060-scopes.md) §4).

Compiled queries run in the projection worker ([0013](0013-postgres-storage.md) §7), never in a request, under `query.timeout` (default 30 s) like any query. They are what the scope projection of [0060](0060-scopes.md) §5 calls.

### 6. Raw SPARQL for people (extends 0012 §5; extends 0047 §9)

*Changed by A1.*

**`GET` and `POST /sparql`**, at the tenant base, implement the SPARQL 1.1 Protocol for queries: `query=`, result formats `application/sparql-results+json` (default), `+xml`, `text/csv`, `text/tab-separated-values`, and for `CONSTRUCT`/`DESCRIBE` the RDF formats `triplespace-rdf` already writes. The dataset is the tenant's (§4); `FROM` is refused; `SERVICE` is refused in version 1 (Q2); Update is not served, as the protocol's update operation is simply absent. `query.timeout` (default 30 s) and `query.max_results` (default 100,000 solutions) bound every query, and a result cut at either says so in a `Query-Truncated: timeout | results` header, since the result formats have no place for a warning. The response carries `Query-Cursor: {epoch}:{seq}`.

**Prefixes.** The prefixes of the Wikibase-compatible dump ([wikibase-compat.md](../api/wikibase-compat.md) §4) are pre-declared, with the tenant's own `wd:`, `wdt:`, `p:`, `ps:`, `pq:` and the rest bound to the tenant's IRIs, as the Wikidata Query Service pre-declares Wikidata's; a query may redeclare any of them. The prefix set is served at `GET /sparql/prefixes` as Turtle, so that a client can show it.

**`Special:Query`** is the page: a SPARQL editor with the prefix set, a result table with entity labels resolved and linked, a "Try with scope" button that turns the query into a scope definition ([0060](0060-scopes.md) §4), and a shareable URL, `Special:Query?query=…`, so that a query is a link as it is on Wikidata; **Save as Query page** turns the text into a `Query:` page with parameters ([0063](0063-query-namespace.md) §5). It is served when `query.enabled` is on (`requires = "query.enabled"`), in the `wikibaserepo` group, unrestricted; it reads nothing but `/sparql`.

**Rate limits**: a new class, `query`, 30 / 300 per minute (`user` / `bot`), counting queries started; a subsidiary that needs more asks for it as any does ([0024](0024-subsidiary-accounts.md) §5). Concurrency is bounded instance-wide by `query.max_concurrent` (default 8), beyond which a query waits up to its own timeout. **Permission**: `read`. **Caching**: a `GET /sparql` response is cacheable at L2 for `query.cache_ttl` (default 60 s) by its full URL, since the store holds public form only and the dataset is a function of the tenant ([0014](0014-caches-and-search.md) §4); `POST` is not cached.

### 7. Operations

- **`triplespace-cli query status`**: backend, cursor, lag, store size, and for `remote` the endpoint's marker cursor.
- **`triplespace-cli query rebuild [--tenant]`**: drops the embedded store (or one tenant's) and reloads it from a fresh `full` dump taken at the current cursor, then resumes deltas. Run by the `view` rebuild automatically (§3).
- **`triplespace-cli query run --tenant {slug} {file}`**: runs a query as the service would, for operators and tests.
- **Metrics** on `server.admin_listen` ([0056](0056-security-model.md) §10 line 9): queries started, finished, timed out, truncated; lag; store size.

### 8. Crates (amends 0005 §2 and §4.4)

| Crate | Change |
|---|---|
| `triplespace-query` | **New, layer 3.** The `QueryService` trait; the embedded backend over `scatter-quadstore` and the remote backend over the SPARQL Protocol; dataset computation from a tenant's provider policy; the prefix set; the envelope of §5 (dataset, `LIMIT`, order) and the checks on user SPARQL; the cursor marker. Depends on `triplespace-db`, `scatter-quadstore` (feature `oxigraph`), `scatter-wikibase-rdf`, `scatter-vocab`, `spargebra` for parsing and the `FROM`/`SERVICE` checks, `reqwest` for the remote backend |
| `scatter-quadstore` | Gains a `query(sparql, dataset) -> Solutions` method behind the `oxigraph` feature, so that the embedded backend needs no second handle to the store. Scatterbase's drivers may implement or ignore it, as with `apply` |
| `triplespace-projections` | The embedded store's feeder: applies each committed `rdf_delta` batch through `scatter-quadstore::apply`, per tenant under `store` isolation; the epoch reload |
| `triplespace-api-rest` | `/sparql`, `/sparql/prefixes` |
| `triplespace-ui` | `Special:Query` |
| `triplespace-cli` | `query status`, `query rebuild`, `query run` |
| `triplespace-server` | `query.*` settings; the `query` rate class; `siprop=triplespace` `query` block |

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
