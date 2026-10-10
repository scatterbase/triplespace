# 0075. MCP server

- **Status:** Proposed
- **Date:** 2026-10-07
- **Updated:** 2026-10-09 (A3)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0014](0014-caches-and-search.md), [0024](0024-subsidiary-accounts.md), [0025](0025-oauth-server.md), [0033](0033-backend-stack.md), [0056](0056-security-model.md), [0059](0059-query-service.md), [0060](0060-scopes.md), [0062](0062-workspaces.md), [0063](0063-query-namespace.md), [0067](0067-proposals.md), [0071](0071-derived-statements-from-mirrored-pages.md), [0074](0074-publishing-a-scope-to-an-external-wiki.md), [0076](0076-dataset-publication.md)
- **Chapters:** [07](../architecture/07-actors-and-accounts.md), [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md)

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

*Current text: [18](../architecture/18-api.md) §7.1.*

### 2. Tools, read-only

*Current text: [18](../architecture/18-api.md) §7.2.*

### 3. Teaching the agent the tenant's vocabulary

*Current text: [18](../architecture/18-api.md) §7.3.*

### 4. Resources

*Current text: [18](../architecture/18-api.md) §7.3.*

### 5. Access and limits

*Changed by A1, A3.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §5.3, §6.2, §6.4; [09](../architecture/09-security-and-moderation.md) §8.11.*

### 6. Settings and discovery (extends 0015 §3; extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §2.2, §7.1; [23](../architecture/23-configuration-and-registry.md) §3.2, §6.*

### 7. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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

## Amendment log

### A1. The `basic` grant, not `read`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §5
- **Summary:** "the `read` grant" is `basic` (0024 §4), which every API key and OAuth token carries; there is no `read` grant. (PENDING C3)

Replaced text (§5):

> **Other tenants** require a bearer credential: a subsidiary's API key ([0024](0024-subsidiary-accounts.md) §4) or an OAuth token ([0025](0025-oauth-server.md)), with the `read` grant.

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§7
- **Summary:** The Decision's current text now lives in the architecture chapters [07](../architecture/07-actors-and-accounts.md), [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A3. The chapter says `basic`

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** corrects §5
- **Summary:** The MCP requirement on a non-public tenant is the `basic` grant, which every API key and OAuth token carries; A1 recorded this, and the chapter text still said "the `read` grant" until this review. The rest of the row (hand-made subsidiaries are never `pending`, `instance create` puts the sync subsidiaries in `bot`, `view.actor` gains `editcount` and `created_at`, `autoconfirmed` is an implicit group) is [0024](0024-subsidiary-accounts.md) §2's and [0016](0016-permissions-and-access-control.md) §3's. The ledger names §4, which is the resources section and unchanged. (REVIEW G40)

Replaced text ([09](../architecture/09-security-and-moderation.md) §8.11, as it stood):

> **Other tenants** require a bearer credential: a subsidiary's API key or an OAuth token, with the `read` grant.
