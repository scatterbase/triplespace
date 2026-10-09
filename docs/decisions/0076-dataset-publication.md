# 0076. Dataset publication

- **Status:** Proposed
- **Date:** 2026-10-07
- **Updated:** 2026-10-09 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0001](0001-revision-metadata-rdf.md), [0006](0006-log-integrity-and-erasure.md), [0039](0039-files-and-media.md), [0053](0053-mirrored-pages.md), [0056](0056-security-model.md), [0060](0060-scopes.md), [0070](0070-shallow-entity-mirroring.md), [0071](0071-derived-statements-from-mirrored-pages.md), [0073](0073-lines-links-and-url-patterns.md), [0074](0074-publishing-a-scope-to-an-external-wiki.md), [0075](0075-mcp-server.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

## Context

A scope ([0060](0060-scopes.md)) is the natural unit of a dataset: the FamilySearch genealogy scope is the resources, guides and facet values a customer wants published. Three kinds of reader want it whole, or want to walk it:

- **Bulk consumers**, such as an AI developer building an index, want a file.
- **Crawlers** that build a graph from embedded JSON-LD want each entity's description to say what it is in a vocabulary they know (schema.org), and to lead them to related entities. A crawler that enters at a facet (a record type, a period, a cultural group) must be able to reach the resources about it without going through the place hierarchy the wiki is organized by.
- **A later MediaWiki extension** on the source wiki, if its owners install one, would embed per-page JSON-LD and needs a way to ask "what does Triplespace know about the resources on this page".

Triplespace already emits Wikibase RDF ([0001](0001-revision-metadata-rdf.md) §2), which no general-purpose crawler understands, and keeps that output identical in shape to Wikibase's.

### Direction

James's direction, from the design discussion of 2026-10-07:

- **One AI access pattern will be to crawl the whole wiki and construct the graph from embedded JSON-LD**; access whose pattern is not locality, such as time period or ethnicity, must also be possible, and an agent will need to traverse more of the graph to get it.
- **If FamilySearch later adopts Triplespace, the relevant dataset is exportable.**

## Decision

### 1. Scope dumps

*Current text: [17](../architecture/17-federation-and-publication.md) §5.1.*

### 2. The schema.org profile (extends 0015 §3)

*Current text: [17](../architecture/17-federation-and-publication.md) §5.2.*

### 3. Walking from any entity (extends 0012 §5)

*Current text: [17](../architecture/17-federation-and-publication.md) §5.3.*

### 4. Discovery

*Changed by A1.*

*Current text: [17](../architecture/17-federation-and-publication.md) §5.4.*

### 5. Looking up by URL

*Current text: [17](../architecture/17-federation-and-publication.md) §5.5.*

### 6. Storage, operations and API (extends 0013 §5.6)

*Changed by A1.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.15, §5; [18](../architecture/18-api.md) §3.2.*

### 7. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **Adding schema.org triples to the main graph.** It would change Wikibase-compatible output, which 0001 §2 forbids, and mix two vocabularies in one graph.
- **A full RDF dump of the tenant** instead of per-scope dumps. Shared provider graphs make a tenant's resolved view as large as its mirrors; a scope is the part someone means to publish.
- **Inverse properties materialized as statements** ("has resource" on every place). They would double the data, and every new resource would edit every place above it; a referrers endpoint answers the same question on demand.

## Consequences

- **A scope is a downloadable, verifiable dataset**, which is also what a customer that adopts Triplespace later takes with it.
- **Crawlers that know schema.org can read a tenant's entities** and walk from any facet value to what it describes.
- **The source wiki can embed the data later** with one lookup per page, without Triplespace writing to it.
- **Test plan.** A scope of three resources dumps in three formats whose manifests verify against the log's checkpoints; the profiled JSON-LD of a resource carries `spatialCoverage`, `temporalCoverage` as an interval, `inLanguage` and `subjectOf` the guide; the referrers of a record-type item list the resources of that type; `by-url` on a guide's URL returns the guide and its resources; a restricted statement is in no anonymous dump.

## Open questions

- **Q1. Dumps of restricted tenants per principal**, or only per group.
- **Q2. The MediaWiki extension** that embeds `by-url` JSON-LD: Triplespace's to ship, or the destination's to write.
- **Q3. Incremental dumps** from `view.rdf_delta`, for consumers that keep a copy current without downloading the whole scope.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §7 | amends | 0005 A79 |
| [0012](0012-api-requirements.md) §5 | §3, §6 | extends | 0012 A54 |
| [0013](0013-postgres-storage.md) §5.6 | §6 | extends | 0013 A39 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §2 | extends | 0015 A35 |

## References

- [schema.org Dataset](https://schema.org/Dataset), [DataDownload](https://schema.org/DataDownload), [temporalCoverage](https://schema.org/temporalCoverage), [spatialCoverage](https://schema.org/spatialCoverage)
- [Google: Dataset structured data](https://developers.google.com/search/docs/appearance/structured-data/dataset): how crawlers read `Dataset`
- `claude/familysearch-semantic-layer-plan.md` (Triplespace project notes, 2026-10-07)

## Amendment log

### A1. Scopes are addressed by page ID

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §4, §6, §7
- **Summary:** Scopes are addressed by page ID, as every scope route is: `/scope/{pageid}/dumps`, `/scope/{pageid}/dumps/{file}`, `/scope/{pageid}/dataset.jsonld`, not `{title}`. The ledger row names §2 and §6; the `{title}` routes are in §4 and §6 (§2 has none), and the §7 crates row names them too, so this entry corrects §4, §6 and §7. (PENDING E35)

Replaced text (§4):

> - `GET /scope/{title}/dataset.jsonld` describes the scope as a schema.org `Dataset`: name and description from the scope page, `license` from the tenant's `content.licence` ([0053](0053-mirrored-pages.md) §9), `dateModified`, and one `DataDownload` per dump with `contentUrl`, `encodingFormat` and `contentSize`.

Replaced text (§6):

> - `GET /scope/{title}/dumps` lists the current dumps and their manifests; `GET /scope/{title}/dumps/{file}` serves one.

Replaced text (§7):

> | `triplespace-api-rest` | `/entity/{id}/referrers`, `/scope/{title}/dataset.jsonld`, `/scope/{title}/dumps`, `/jsonld/by-url` |

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§7
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
