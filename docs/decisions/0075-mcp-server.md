# 0075. MCP server

- **Status:** Proposed
- **Date:** 2026-10-07
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0014](0014-caches-and-search.md), [0024](0024-subsidiary-accounts.md), [0025](0025-oauth-server.md), [0033](0033-backend-stack.md), [0056](0056-security-model.md), [0059](0059-query-service.md), [0060](0060-scopes.md), [0062](0062-workspaces.md), [0063](0063-query-namespace.md), [0067](0067-proposals.md), [0071](0071-derived-statements-from-mirrored-pages.md), [0074](0074-publishing-a-scope-to-an-external-wiki.md), [0076](0076-dataset-publication.md)

## Context

The first customer for derived data ([0071](0071-derived-statements-from-mirrored-pages.md)) asked for two things: the data back in its wiki, and an MCP server, so that AI agents can find research resources by asking. An agent can already reach Triplespace's data three ways, and none suits it:

- **The Action API and REST** answer one entity at a time, so a question by facet becomes many calls.
- **SPARQL** ([0059](0059-query-service.md)) answers anything, but an agent must know the tenant's properties and write correct queries for them.
- **Crawling** pages or JSON-LD ([0076](0076-dataset-publication.md)) works for bulk, but is slow for one question.

The Model Context Protocol gives an agent typed tools with descriptions and schemas, over HTTP. Scopes ([0060](0060-scopes.md)) and their facets ([0062](0062-workspaces.md) §8) are already the right shape for "which resources match these filters". This ADR puts an MCP server in front of them.

### Direction

James's direction, from the design discussion of 2026-10-07:

- **An MCP server is another thing FamilySearch asked for**, beside the semantic layer.
- **It is a general component**, configured per tenant, not written for one customer.

## Decision

### 1. One endpoint per tenant

**Each tenant with `mcp.enabled` serves MCP at `{base}/mcp`**, over the protocol's Streamable HTTP transport, from `triplespace-server` like every other API surface ([0033](0033-backend-stack.md) §3). `mcp.enabled` is a `site` setting, default off.

**A default scope narrows every tool.** `mcp.scope` names the scope the tools search within by default; `mcp.scopes` lists other scopes a client may select per session with `?scope={title}` on the endpoint URL. With no scope, the tools range over the tenant's resolved view.

### 2. Tools, read-only

| Tool | Answers | Backed by |
|---|---|---|
| `search` | Entities by name in a language, with labels, descriptions and match type | The search index ([0014](0014-caches-and-search.md) §7), as `wbsearchentities` |
| `get_entity` | One entity: labels, descriptions, statements with property and value labels, identifiers with their URLs, and the canonical IRI | The resolved view |
| `find` | Entities in the scope matching filters: `{property, value}`, `{property, values}` (any of), `{property, from, to}` for times and quantities, and `{path}` for transitive paths as [0074](0074-publishing-a-scope-to-an-external-wiki.md) §2 writes them; paged by cursor | A compiled query ([0059](0059-query-service.md) §5) over the scope |
| `facets` | For a filter, the values of the configured facet properties among the matches, with counts | `/scope/{id}/facets` ([0062](0062-workspaces.md) §8) |
| `mentions` | The pages an entity was derived from, with the lines that mention it | `GET /entity/{id}/derivations` ([0071](0071-derived-statements-from-mirrored-pages.md) §13) |
| `run_query` | A saved `Query:` page by title, with its parameters | [0063](0063-query-namespace.md) |
| `sparql` | A SELECT query, under the query service's limits | `/sparql` ([0059](0059-query-service.md) §6) |

Every result carries labels in the language the client asks for (`lang`, default the tenant's content language), the canonical IRI of every entity, and the URL of every page. `find`, `facets`, `run_query` and `sparql` need the query service; on a tenant without one, the server lists only `search`, `get_entity` and `mentions`.

No tool writes in this version (Q1).

### 3. Teaching the agent the tenant's vocabulary

**An agent learns which properties matter from the server, not from the property list.**

- **`mcp.facets`** lists the properties offered as filters and facets, each with a short description written for the agent ("record type: the kind of records a resource holds, such as probate or census"). The descriptions of `find` and `facets` are generated from it, with the properties' labels and IDs.
- **`mcp.instructions`** is free text returned as the server's instructions at initialization: what the tenant holds, what a typical question looks like, what the facets mean.

### 4. Resources

The server exposes, as MCP resources:

- each scope it serves: its description, size, facet properties, and the URLs of its dumps ([0076](0076-dataset-publication.md) §1), so that an agent that wants everything downloads a dump instead of paging `find`;
- the facet property list of §3, with each property's data type and, for items, its most common values.

### 5. Access and limits

- **Public tenants** serve the endpoint anonymously, with the visibility of an anonymous reader ([0056](0056-security-model.md) §2). **Other tenants** require a bearer credential: a subsidiary's API key ([0024](0024-subsidiary-accounts.md) §4) or an OAuth token ([0025](0025-oauth-server.md)), with the `read` grant. Every tool reads as that principal, so restricted data never reaches an agent that could not read it through the API.
- Every tool call counts against the `read` rate class; `find`, `facets`, `run_query` and `sparql` also against `query` ([0024](0024-subsidiary-accounts.md) §5).
- Results are capped at `mcp.max_results` (default 200) per call, with a cursor; queries run under `query.timeout` (0059 §6).

### 6. Settings and discovery (extends 0015 §3; extends 0012 §5)

- `mcp.enabled`, `mcp.scope`, `mcp.scopes`, `mcp.facets`, `mcp.instructions` and `mcp.max_results` join the `site` settings.
- `meta=siteinfo&siprop=triplespace` reports the endpoint when enabled, and the REST root lists it, so a client that knows the wiki can find the server.

### 7. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `triplespace-mcp` | **New.** The Streamable HTTP transport, initialization with instructions, the tools of §2 with their JSON schemas and generated descriptions, resources, cursors, and the mapping of tool calls to the query service, scopes, search and the resolved view |
| `triplespace-server` | Mounting `/mcp` per tenant, with authentication and rate limits as for the REST API |

## Alternatives considered

- **SPARQL alone, with a prompt.** It works for capable agents and fails quietly for the rest, which write plausible queries against the wrong properties. `sparql` is kept as one tool among several.
- **One server per customer**, written for its vocabulary. Facet descriptions in configuration give the same help to the agent without code.
- **Tools that return wikitext or HTML.** Agents read structured results more reliably, and the page URLs let them open a page when they need its prose.

## Consequences

- **An agent can ask "probate indexes for Ohio counties before 1900" in one `find` call** with filters it learned from the server.
- **The same server serves every tenant**, and the tenant's configuration says what its data means.
- **Agents see what anonymous readers see**, or what their credential's principal sees, and nothing more.
- **Test plan.** Initialization returns the instructions and, without a query service, three tools; `find` with two filters and a time range returns the matching members with labels in the requested language; `facets` counts agree with `find`; a private tenant refuses a call without a credential; `max_results` and the cursor page a large result; a restricted statement never appears in `get_entity` for an anonymous client.

## Open questions

- **Q1. Write tools**: corrections ([0071](0071-derived-statements-from-mirrored-pages.md) §8) and proposals ([0067](0067-proposals.md)) from an agent, under the agent's own subsidiary and an edit filter.
- **Q2. The protocol implementation**: an MCP SDK crate (the official Rust SDK, `rmcp`) against the dependency policy of [0033](0033-backend-stack.md) §16, or JSON-RPC over the existing HTTP stack, which the protocol's size allows.
- **Q3. Natural-language search over the evidence of derivations**, which would let an agent find a resource by the words of the line that mentions it.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §7 | amends | 0005 A78 |
| [0012](0012-api-requirements.md) §5 | §6 | extends | 0012 A53 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §6 | extends | 0015 A34 |

## References

- [Model Context Protocol specification](https://modelcontextprotocol.io/specification): tools, resources, server instructions, the Streamable HTTP transport
- `claude/familysearch-semantic-layer-plan.md` (Triplespace project notes, 2026-10-07)
