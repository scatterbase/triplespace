# 0063. The Query namespace

- **Status:** Proposed
- **Date:** 2026-10-05
- **Updated:** 2026-10-09 (A1)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0059](0059-query-service.md), [0060](0060-scopes.md), [0061](0061-sprints-and-tasks.md), [0062](0062-workspaces.md)
- **Uses:** [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0038](0038-page-metadata-and-categories.md), [0043](0043-lua-modules.md), [0044](0044-tenant-relative-ids.md), [0045](0045-table-content-model.md), [0047](0047-special-pages.md)
- **Chapters:** [15](../architecture/15-structured-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

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

*Current text: [15](../architecture/15-structured-pages.md) §5.1.*

### 2. The `Query` namespace (amends 0008 §2)

*Current text: [15](../architecture/15-structured-pages.md) §5.2.*

### 3. The `triplespace-sparql` content model (extends 0041 §3)

*Current text: [15](../architecture/15-structured-pages.md) §5.2.*

### 4. The definition

*Current text: [15](../architecture/15-structured-pages.md) §5.3.*

### 5. Running a query, and the page

*Current text: [15](../architecture/15-structured-pages.md) §5.4.*

### 6. Where a query page is used (extends 0060 §4; extends 0061 §4; extends 0062 §3)

*Current text: [15](../architecture/15-structured-pages.md) §3.3, §4.3, §5.5.*

### 7. API (extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §2.3, §3.2, §6.*

### 8. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [15](../architecture/15-structured-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
