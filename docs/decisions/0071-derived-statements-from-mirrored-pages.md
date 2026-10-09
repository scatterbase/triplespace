# 0071. Derived statements from mirrored pages

- **Status:** Proposed
- **Date:** 2026-10-07
- **Updated:** 2026-10-09 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0011](0011-logs.md), [0018](0018-tenants.md), [0038](0038-page-metadata-and-categories.md), [0047](0047-special-pages.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0060](0060-scopes.md), [0070](0070-shallow-entity-mirroring.md), [0072](0072-template-mappings.md), [0073](0073-lines-links-and-url-patterns.md), [Record and payload shapes](../api/payloads.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [13](../architecture/13-mirrored-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

## Context

A wiki whose pages describe things in a regular way, with templates and with lists of links, holds structured data that no one has written as data. The FamilySearch Research Wiki is the first case: its guide pages carry a `{{CountrySidebar}}` call naming the country and topic, and list research resources one per bullet line or table row, each line a link to a collection at FamilySearch, Ancestry, Findmypast or elsewhere. Its owners want that structure as data, to answer questions by place, period, record type and provider, for people and for AI agents.

Triplespace can already mirror such a wiki's pages ([0053](0053-mirrored-pages.md) §5) and keep them current (0053 §6). What is missing is a place for statements derived from those pages, and the rules for writing them:

- **Not the `local` graph.** Derived statements are a function of a page revision. When the page changes they must be replaced, and editors' own statements must not be disturbed by that. Mixing the two in `local` would need every re-extraction to find and retract its own earlier statements among people's.
- **Not page statements.** [0038](0038-page-metadata-and-categories.md) §2 gives a page statements about itself, and [0052](0052-page-repositories-and-title-inheritance.md) Q1 keeps local statements off foreign pages. The statements here are about the things a page describes: a microfilm collection, an online index, the guide page as a resource. They belong on items.
- **Not a mirror.** A mirror holds what a provider asserts about its own entities. A guide page asserts nothing in Wikibase form; the instance derives the statements, about local items it creates or finds.

The resources become ordinary items of the tenant, matched by their identifiers, as `create-or-add` already provides ([0002](0002-source-graphs-and-mass-ingest.md) §8.5). A keyed entity type per identifier scheme was considered and rejected by James: a namespace for every scheme does not scale.

### Direction

James's direction, from the design discussion of 2026-10-07:

- **Simplify the scope to parsing external links from bullet lines and mapping template names, parameters and values**, which is more generalizable. Analysis by LLMs is a separate workflow, pending an LLM analysis module.
- **Resources are Librarybase items, not keyed entities.** "I am not sure we can create a keyed namespace every time we want to link to the same resource repeatedly."
- **FamilySearch does not run Triplespace.** The genealogy data is a scope, and the wiki receives data derived from it.

## Decision

### 1. Derived graphs (extends 0002 §2)

*Current text: [13](../architecture/13-mirrored-pages.md) §6.1.*

### 2. Extraction sources (extends 0015 §3)

*Current text: [13](../architecture/13-mirrored-pages.md) §6.2.*

### 3. The derivation record (extends 0015 §1)

*Current text: [13](../architecture/13-mirrored-pages.md) §6.3.*

### 4. Subjects: found by identifier, or created (uses 0002 §8.5)

*Changed by A1.*

*Current text: [13](../architecture/13-mirrored-pages.md) §6.4.*

### 5. Statement IDs

*Current text: [13](../architecture/13-mirrored-pages.md) §6.5.*

### 6. References

*Current text: [13](../architecture/13-mirrored-pages.md) §6.5.*

### 7. Resolution (amends 0002 §3)

*Current text: [13](../architecture/13-mirrored-pages.md) §6.6.*

### 8. Corrections (uses 0002 §7)

*Current text: [13](../architecture/13-mirrored-pages.md) §6.6.*

### 9. Following pages

*Current text: [13](../architecture/13-mirrored-pages.md) §6.7.*

### 10. Evidence, and analysis later

*Current text: [13](../architecture/13-mirrored-pages.md) §6.8.*

### 11. Orphans

*Current text: [13](../architecture/13-mirrored-pages.md) §6.8.*

### 12. Storage (extends 0013 §5.6)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.12, §5, §6.1.*

### 13. API (extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §3.2.*

### 14. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **Keyed entity types per identifier scheme** (`fs-collection:1804886`). Deterministic identity with no lookup, but a namespace, registry entry and normalizer for every scheme a page links to. Rejected by direction.
- **Derived statements in the `local` graph**, with a reference naming the page, and re-extraction retracting the previous ones by statement ID. One graph fewer, but machine statements and people's then share a history, a re-extraction rewrites the local log, and a person's edit to a derived statement is overwritten by the next re-extraction.
- **One record per subject**, keyed by entity, holding every page's contribution. Resolution reads one record, but every page edit must read, merge and rewrite the record of every subject it mentions, and a busy subject's record grows with every page that cites it.
- **Page statements on the mirrored page** ([0038](0038-page-metadata-and-categories.md) §1). The statements are about the resources, not the page, and [0052](0052-page-repositories-and-title-inheritance.md) Q1 keeps local statements off foreign pages.

## Consequences

- **A mirrored wiki becomes a source of data** about the things it describes, on items the tenant can query, scope ([0060](0060-scopes.md)), correct and export.
- **Re-extraction is safe.** A page edit replaces only that page's derivations; people's statements and corrections are untouched.
- **Item creation is idempotent per identifier,** but two identifiers for one thing make two items until someone merges them. Merging needs `wbmergeitems`, which the server does not implement yet.
- **`match_key` designation moves onto the critical path.** Every identifier property an extraction source keys on must be designated.
- **The evidence part copies page text into the tenant's log,** under the repository's licence ([0053](0053-mirrored-pages.md) §9) and erasable on its own.
- **Test plan.** A page with two lines naming one collection makes one item with two references; a second page in the same job naming it adds a third without creating a duplicate; a page edit that drops a line removes that line's statements and nothing else; a local suppression of a derived statement survives re-extraction; a page deletion empties its derivations and leaves an orphan listed; a rebuild from the log reproduces every derived statement and created item.

## Open questions

- **Q1. Merging items.** Two items created for one resource under different identifiers need `wbmergeitems` and a redirect; whether extraction should then learn the merge, so that the other identifier matches the survivor.
- **Q2. Statements about the source wiki's own pages**, such as which guides link to which, beyond `page_subject`: whether a `links` extractor belongs here.
- **Q3. A source over several repositories**, so that two wikis' guides contribute to one graph.
- **Q4. Confidence.** Whether derived statements carry an extractor confidence (a qualifier, or a field in the derivation), which an LLM analyzer would want and the deterministic extractors would not.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §2 | §1 | extends | 0002 A22 |
| [0002](0002-source-graphs-and-mass-ingest.md) §3 | §7 | amends | 0002 A22 |
| [0005](0005-crate-organization.md) §2 | §14 | amends | 0005 A74 |
| [0012](0012-api-requirements.md) §5 | §13 | extends | 0012 A51 |
| [0013](0013-postgres-storage.md) §5.6 | §12 | extends | 0013 A37 |
| [0015](0015-record-format-and-partition-registry.md) §1, §3, §5 | §1, §2, §3 | extends | 0015 A31 |

## References

- `claude/familysearch-semantic-layer-plan.md` (Triplespace project notes, 2026-10-07): the first use, and the decisions behind this ADR
- FamilySearch Research Wiki pages "Mexico Colonial Records" and "Ireland Land and Property" (wikitext supplied by James, 2026-10-07): the two layouts, table rows and bullet lines, that the extractors are designed against

## Amendment log

### A1. Undesignated identifier properties find but never create

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §4
- **Summary:** An undesignated `match_key` property may find a subject (step 1, via `view.identifier`) but never create one: the refusal in step 3 stands, so a `create-or-add` never carries an undesignated property as its `match`, and a source is not refused at configuration merely for naming one; 0073 §3's `lines.identifiers` follows the same rule. (PENDING F4)

Replaced text (§4):

> The match key's property must be designated for `match_key` for step 3 to be safe; a source whose mappings name an undesignated identifier property is refused at configuration with `ts-extraction-match-key`.

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§14
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [13](../architecture/13-mirrored-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
