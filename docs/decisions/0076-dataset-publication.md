# 0076. Dataset publication

- **Status:** Proposed
- **Date:** 2026-10-07
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0001](0001-revision-metadata-rdf.md), [0006](0006-log-integrity-and-erasure.md), [0039](0039-files-and-media.md), [0053](0053-mirrored-pages.md), [0056](0056-security-model.md), [0060](0060-scopes.md), [0070](0070-shallow-entity-mirroring.md), [0071](0071-derived-statements-from-mirrored-pages.md), [0073](0073-lines-links-and-url-patterns.md), [0074](0074-publishing-a-scope-to-an-external-wiki.md), [0075](0075-mcp-server.md)

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

**A scope can be dumped on a schedule, in several formats, as files anyone permitted can download.** `dumps` on a scope's definition lists formats and a schedule (default `daily`):

| Format | Holds |
|---|---|
| `wikibase.json` | One line per member, in Wikibase JSON as `Special:EntityData` serves it, like Wikidata's JSON dump |
| `wikibase.nt` | The members' Wikibase RDF in the resolved view, as N-Triples ([0001](0001-revision-metadata-rdf.md) §2) |
| `schema.jsonld` | One JSON-LD document per line, in the schema.org profile of §2 |

Each dump is gzip-compressed, stored as a blob ([0039](0039-files-and-media.md) §3), and described by a manifest: the scope's revision, the member count, the generation time, the SHA-256, and the checkpoint of each partition it reflects ([0006](0006-log-integrity-and-erasure.md) §6), so that a dump can be checked against the log. The previous `dumps.keep` (default 7) dumps of each format are kept.

**A dump holds what an anonymous reader can read** ([0056](0056-security-model.md) §2). A tenant that restricts reading serves its dumps only to principals with `read`, and dumps hold that principal's view only if generated for it (Q1).

### 2. The schema.org profile (extends 0015 §3)

**A tenant can describe its entities in schema.org, beside Wikibase RDF and without changing it.** A `config` record of kind `jsonld-context`, whose code is the profile's name, maps the tenant's properties to schema.org terms:

| Field | Meaning |
|---|---|
| `types` | Item values of `P31` (or another property) to `@type`s: `research guide` to `WebPage`, `record collection` to `Collection` and `Dataset` |
| `properties` | Property to term, with an optional form: `P9011` to `spatialCoverage`; `WDP407` to `inLanguage`; `WDP123` to `publisher`; `P9030` to `url`; `WDP1343` to `subjectOf` |
| `intervals` | Pairs of properties that make one ISO 8601 interval: `WDP580`/`WDP582` to `temporalCoverage` as `1850/1885` |
| `external` | Identifier properties written as `identifier` with a `PropertyValue` naming the scheme |

Item values are written as `{"@id": canonical IRI, "name": label}`, with the label in the request's language, so a crawler can follow each one. The profile is served at `Special:EntityData/{id}.jsonld?profile={name}` and in `schema.jsonld` dumps. `Special:EntityData/{id}.jsonld` without a profile stays as Wikibase emits it, as 0001 §2 requires.

### 3. Walking from any entity (extends 0012 §5)

**An entity's description says how to find what points at it.** A crawler that reaches a period, a record type or a cultural group must be able to list the resources about it:

- `GET /entity/{id}/referrers?property={P}&cursor=` returns, paged, the entities whose best-rank statements under `P` have `id` as their value, from `view.entity_ref`. Without `property`, it returns the counts per property.
- In the schema.org profile, every entity carries `"subjectOf"`-style links only where the data says so; in addition it carries, for each property with referrers, a `potentialAction` of type `SearchAction` whose `target` is the referrers URL, with the property's label and the count. A crawler that knows schema.org follows it; one that does not still sees the URL.

Facet values thereby become entry points: a crawler entering at "Swedish Americans" lists the resources whose cultural-group statement names it, without ever visiting a place.

### 4. Discovery

- `GET /scope/{title}/dataset.jsonld` describes the scope as a schema.org `Dataset`: name and description from the scope page, `license` from the tenant's `content.licence` ([0053](0053-mirrored-pages.md) §9), `dateModified`, and one `DataDownload` per dump with `contentUrl`, `encodingFormat` and `contentSize`.
- A sitemap at `/sitemap-datasets.xml` lists every dumped scope's dataset description and its members' profiled JSON-LD URLs, and `robots.txt` names it.
- The MCP server ([0075](0075-mcp-server.md) §4) exposes the same description as a resource.

### 5. Looking up by URL

**`GET /jsonld/by-url?url={u}&profile={name}` returns the profiled JSON-LD of the entities a URL identifies**: the subject whose `page_subject` match key is that URL ([0071](0071-derived-statements-from-mirrored-pages.md) §2), and the resources mentioned on that page with their facets. This is the one call a MediaWiki extension on the source wiki would need to embed JSON-LD in each page's head; no such extension is specified here (Q2).

### 6. Storage, operations and API (extends 0013 §5.6)

- `ops.dump (tenant, scope, format, generated_at, blob, sha256, members, manifest)`; dumps run as jobs and appear on `Special:Jobs`.
- `GET /scope/{title}/dumps` lists the current dumps and their manifests; `GET /scope/{title}/dumps/{file}` serves one.
- `triplespace-cli dump run {scope} [--format …]`.

### 7. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-wikibase-rdf` | The schema.org profile: types, property terms, intervals, identifiers, referrer actions. Pure |
| `triplespace-rdf` | Scope dumps in the three formats, manifests with checkpoints, the dump job and its schedule; the `jsonld-context` config kind; the dataset description and sitemap |
| `triplespace-api-rest` | `/entity/{id}/referrers`, `/scope/{title}/dataset.jsonld`, `/scope/{title}/dumps`, `/jsonld/by-url` |
| `triplespace-cli` | `dump run` |

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
