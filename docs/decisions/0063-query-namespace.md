# 0063. The Query namespace

- **Status:** Proposed
- **Date:** 2026-10-05
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0059](0059-query-service.md), [0060](0060-scopes.md), [0061](0061-sprints-and-tasks.md), [0062](0062-workspaces.md)
- **Uses:** [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0038](0038-page-metadata-and-categories.md), [0043](0043-lua-modules.md), [0044](0044-tenant-relative-ids.md), [0045](0045-table-content-model.md), [0047](0047-special-pages.md)

## Context

Wikibase's original roadmap had a third phase after interwiki links and statements: **queries**, stored on the wiki as first-class objects and used to generate lists in Wikipedia articles. The namespace was allocated — 124, `Query`, still defined in Wikibase's constants — and never used. The engine that could have answered the queries arrived separately in 2015 as the Wikidata Query Service, outside the wiki; lists in articles were left to Listeria, a bot writing wikitext from WDQS results into pages that nobody's watchlist understands.

Triplespace reserved 124 under [0008](0008-namespaces-and-document-pages.md) §2 rule 1: a number Wikibase uses is never given *another* meaning. That rule allows the number to be given *its own* meaning by an ADR. The pieces the original plan lacked now exist: a query engine inside the instance ([0059](0059-query-service.md)), bounded lists inside pages with refresh ([0062](0062-workspaces.md) §3), and three places that already accept inline SPARQL — a scope's `query` kind ([0060](0060-scopes.md) §4), a sprint's rules ([0061](0061-sprints-and-tasks.md) Q2) and `Special:Query`'s shareable URL. Each of those is a query with no name, no documentation, no history and no parameters, pasted where it is used.

### Direction

James's direction, from the design discussion of 2026-10-05:

- **Use namespace 124.**
- **The definition is a JSON wrapper** around the SPARQL, not bare SPARQL with annotated comments.
- **The version-1 renderer is a result table.** A list is a table with one column; a count is a table with one cell.

## Decision

### 1. A query is a page

**A `Query:` page is a saved, documented, parameterized SPARQL query.** It has a title, a description, a history, a talk page, watchers and page statements like any page, and it can be run, with parameters, by anyone who can read it. Everything in 0059–0062 that takes inline SPARQL also takes a `Query:` page by title, so a query is written once and used as a scope, a rule, a block and a link.

What it is not: an entity. Wikibase's plan made queries entities with IDs; here a query is a document with a name, because its content is text people edit and its identity is its title. Its topic, when it has one, is a page statement ([0038](0038-page-metadata-and-categories.md) §1).

### 2. The `Query` namespace (amends 0008 §2)

| Number | Canonical name | Kind | Allowed models | Default model |
|---|---|---|---|---|
| 124 | `Query` | `pages` *(was `reserved`)* | `triplespace-sparql`, `wikitext` | `triplespace-sparql`; `wikitext` for titles ending `/doc` |
| 125 | `Query talk` | `pages` | `triplespace-talk` | `triplespace-talk` |

124 keeps the meaning Wikibase gave it. Titles normalize first-letter; subpages are on; the `/doc` rule is Table's ([0045](0045-table-content-model.md) §2). `Query talk` is new, at the next odd number, as [0008](0008-namespaces-and-document-pages.md) §2 rule 2 requires for an implemented subject namespace.

### 3. The `triplespace-sparql` content model (extends 0041 §3)

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `triplespace-sparql` | Triplespace | text | main | `application/json` | Yes | 124 |

The model ID is not `wikibase-query`, which stays reserved for Wikibase's never-shipped model ([0041](0041-content-models.md) §3 rule). A **text** model as the table, scope and sprint models are: validation checks the schema and the SPARQL (§4), the pre-save transform canonicalizes IDs ([0044](0044-tenant-relative-ids.md) §1) in parameter defaults and key order, plain text for search is the title, description and parameter descriptions, render is the query page (§5).

### 4. The definition

```json
{
  "version": 1,
  "description": "People with a given occupation and their birthplaces",
  "sparql": "SELECT ?item ?itemLabel ?place WHERE { ?item wdt:P106 ?occupation ; wdt:P19 ?place . }",
  "params": [
    { "name": "occupation", "type": "entity", "description": "An occupation item", "default": "WDQ36180", "required": true }
  ],
  "limit": 1000
}
```

| Key | Required | Meaning |
|---|---|---|
| `version` | Yes | `1` |
| `description` | No | Plain text, shown under the title and indexed |
| `sparql` | Yes | A `SELECT` query, checked as [0059](0059-query-service.md) §5 checks user SPARQL: no `FROM`, no `SERVICE`, no update operation. The prefixes of 0059 §6 are implied. Any variable may be projected; `?item` is what scopes and rules read (§6) |
| `params` | No | Up to `queries.max_params` (default 10) parameters. Each has a `name` (a SPARQL variable name without `?`, which must occur in `sparql`), a `type`, an optional `description`, `default` and `required` |
| `limit` | No | The query's own bound on solutions, capped by `query.max_results` |

**Parameter types and how they bind.** A parameter is **never substituted into the query text**. The compiler ([0059](0059-query-service.md) §5) prepends a `VALUES` clause binding the variable to the supplied value as a SPARQL term, so a value can only ever be a term and the query's structure is fixed at save:

| Type | Value accepted | Bound as |
|---|---|---|
| `entity`, `property` | An entity ID in any form the tenant accepts; canonicalized | The entity's IRI |
| `string` | Text, optionally `@lang` | A literal, with language tag when given |
| `number`, `date` | A number; an ISO 8601 date or `xsd:dateTime` | A typed literal |
| `scope` | A scope title ([0060](0060-scopes.md)) | `VALUES ?x { … }` over the scope's materialized members, up to `queries.max_scope_values` (default 5,000); a larger scope is refused with the scope's notice and a pointer to a `saved` scope kind, which composes the other way round (§6) |

A missing parameter takes its `default`; a `required` parameter without a default or a value refuses the run (`ts-query-param-required`). An unknown parameter is refused.

### 5. Running a query, and the page

**Running** compiles the definition with its bound parameters through the envelope of [0059](0059-query-service.md) §5 (the tenant's dataset, `LIMIT` of `limit` + 1, the timeout) and returns solutions with the cursor they were computed at. Results are cached at L2 per `(definition revid, canonical parameters, cursor epoch)` for `query.cache_ttl`, as `/sparql` responses are ([0059](0059-query-service.md) §6); the page shows the age.

**The page** renders the description, a **parameter form** built from `params`, and the **result table**: one column per projected variable, entity values shown as labels in the viewer's language with links, literals as text, and a footer with the solution count, the truncation notice when the limit was reached, the cursor's age and the run time. There is one renderer in version 1 (direction): a one-column projection is a list and a `COUNT` is a single cell, and both are the table. Tabs: **Result**, **Definition** (the SPARQL editor of `Special:Query` with the parameter editor beside it), Related changes, talk, history. Actions: **Run**, **Save as scope** (a `saved` kind, §6), **Save as table** (a table whose `rows` is that scope), **Export** (CSV, TSV, JSON, as `/sparql` offers). The URL carries the parameters: `Query:People by occupation?occupation=Q36180` runs the query, so a query is a link, as it is on Wikidata, and the link has a name.

**`Special:Query` gains "Save as Query page"** (extends [0059](0059-query-service.md) §6): the current text becomes a new page's `sparql`, with variables the editor marks turned into parameters.

### 6. Where a query page is used (extends 0060 §4; extends 0061 §4; extends 0062 §3)

| Place | Form | Meaning |
|---|---|---|
| Scope kind **`saved`** ([0060](0060-scopes.md) §4) | `{ "saved": { "query": "Query:X", "params": { "occupation": "WDQ36180" } } }` | The subjects bound to `?item` in the result. Computed on the scope's refresh as a `query` kind is; a definition revision of the query triggers the refresh of scopes that use it |
| Sprint rule **`query`** ([0061](0061-sprints-and-tasks.md) §4) | `{ "kind": "query", "query": "Query:X", "params": {…}, "title": "…" }` | A task is open for a member of the sprint's scope while the member is bound to `?item` in the result, and resolved when it is not. The rule's open set is evaluated on the same refresh as a `saved` scope; this is the general rule 0061 Q2 asked for, and it is as fresh as the refresh, which the task says |
| Block **`#query`** ([0062](0062-workspaces.md) §3) | `{{#query:Query:X|occupation=Q36180|rows=50}}` | The result table in a page, bounded by `blocks.max_rows`, a `scoped` block whose manifest entry is the query's page and definition revision; `mw.ext.triplespace.query(title, params)` is the Lua form |
| A link | `[[Query:X?occupation=Q36180]]` | Runs it |

A `Query:` page therefore stands wherever 0060–0062 accepted inline SPARQL, and the inline forms stay for the one-off. `page_link` rows from scopes, sprints and pages to the query pages they use make "What links here" on a query list its users (0060 §8's rule).

### 7. API (extends 0012 §5)

| Route | Does |
|---|---|
| `GET /query/{pageid}/run?{param}=…&format=` | Runs with parameters; `Query-Cursor` and `Query-Truncated` headers as `/sparql`; formats as `/sparql` plus the labelled table as JSON |
| `GET /query/{pageid}/params` | The parameter schema, for forms and for the block editor |
| `prop=revisions`, `action=edit` | The definition, as any text model |

Rate class `query` ([0024](0024-subsidiary-accounts.md) §5, as amended by 0059); concurrency under `query.max_concurrent`. **Permission** `read` to run, `edit` to change; creating a query needs `createpage` in 124. **Protection** of a query page protects its definition, and so every scope, rule and block that trusts it, which is the point of protecting it.

### 8. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-scope` | The `triplespace-sparql` definition: schema, validation, canonicalization; parameter typing and `VALUES` binding; the `saved` scope kind's compilation |
| `scatter-tasks` | The `query` rule, evaluated over a `saved`-style result set |
| `scatter-wikitext-expand` | `#query` as a `scoped` block |
| `triplespace-render` | `#query` data through `ExpandHost`; `mw.ext.triplespace.query` |
| `triplespace-query` | Running a definition with parameters; the per-definition result cache |
| `triplespace-api-rest` | `/query/{pageid}/run`, `/query/{pageid}/params` |
| `triplespace-ui` | The query page, parameter form and result table; "Save as Query page" on `Special:Query` |

## Alternatives considered

- **Queries as entities**, Wikibase's original plan. An ID and labels for a thing whose content is text people edit and diff; the page model gives history, talk and protection for free, and page statements give it a topic.
- **Bare SPARQL with `# @param` comments**, as some tools annotate queries. Rejected by direction: a wrapper validates and types parameters, and binding through `VALUES` needs the types.
- **Text substitution of parameters.** The obvious implementation and an injection vector; `VALUES` binding fixes the query's structure at save time.
- **Several renderers in version 1.** Deferred by direction; a table covers lists and counts, and map, timeline and chart renderings are Q1.

## Consequences

- **Queries have names, owners and histories**, and the three places that took pasted SPARQL now take a title. A protected query is a protected scope, rule and block.
- **124 means what Wikibase meant by it.** Rule 1 is kept: the number has one meaning.
- **Parameterization makes one query serve many workspaces**: `Query:People by occupation` is every occupation project's wanted list.
- **Test plan.** Definitions: each parameter type binds as a term; a value that is not a term is refused; a parameter not in the query is refused. Running: the cache key includes the cursor epoch; the URL form equals the API form. Uses: a `saved` scope equals the query's `?item` set; a `query` rule opens and resolves with the refresh; `#query` is bounded and refreshes on a definition revision.

## Open questions

- **Q1. Renderers.** `map` when a projected variable is a coordinate, `timeline` for dates, `chart` for counts: a `render` key on the definition, after the table has been used for a while.
- **Q2. Parameterized `saved` scopes in the builder** ([0062](0062-workspaces.md) §5): a query page with parameters as a scope "kind" with its own form.
- **Q3. Queries across the `full` form.** A `graphs` key letting a query page read source graphs by name (what Wikidata asserts versus what we assert), which the dataset of 0059 §4 already contains.
- **Q4. Federated queries** follow 0059 Q2.
- **Q5. Query results as a provider.** Whether a query page's result could be an ingest source (0002 §8), the way Listeria results became wikitext; rejected for now as circular, kept as a question.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §8 | amends | 0005 A66 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §2 | amends | 0008 A27 |
| [0012](0012-api-requirements.md) §5 | §7 | extends | 0012 A43 |
| [0041](0041-content-models.md) §3 | §3 | extends | 0041 A10 |
| [0042](0042-template-expansion-and-parsoid.md) §5 | §6 | extends | 0042 A10 |
| [0059](0059-query-service.md) §6 | §5 | extends | 0059 A1 |
| [0060](0060-scopes.md) §4 | §6 | extends | 0060 A2 |
| [0061](0061-sprints-and-tasks.md) §4 | §6 | extends | 0061 A1 |
| [0062](0062-workspaces.md) §3 | §6 | extends | 0062 A1 |

## References

- Wikibase's namespace constants, `WB_NS_QUERY` = 124, in `lib/includes/WikibaseSettings.php` and the Wikidata namespace table; [Wikidata:Development plan](https://www.wikidata.org/wiki/Wikidata:Development_plan) (2012–2013), phase 3 "lists"
- [SPARQL 1.1 Query Language](https://www.w3.org/TR/sparql11-query/) §10.2 `VALUES`
- [Listeria](https://www.wikidata.org/wiki/Wikidata:Listeria) and [Wikidata:SPARQL query service/queries/examples](https://www.wikidata.org/wiki/Wikidata:SPARQL_query_service/queries/examples), where saved queries live today
