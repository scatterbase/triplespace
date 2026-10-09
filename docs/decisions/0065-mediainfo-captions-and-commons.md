# 0065. MediaInfo: captions, and Commons as a source

- **Status:** Proposed
- **Date:** 2026-10-05
- **Updated:** 2026-10-09 (A2)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0038](0038-page-metadata-and-categories.md), [0039](0039-files-and-media.md), [0041](0041-content-models.md), [0045](0045-table-content-model.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0011](0011-logs.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0017](0017-entity-id-grammar.md), [0023](0023-moderation.md), [0032](0032-sparql-update-stream.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)
- **Chapters:** [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [12](../architecture/12-files-and-media.md), [15](../architecture/15-structured-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

## Context

[0041](0041-content-models.md) §6–7 gave every File page a `mediainfo` slot and the MediaInfo ID `M{page ID}`, served through the Wikibase modules as Commons serves Structured Data. Two things were left: **captions** — MediaInfo's labels, which [0038](0038-page-metadata-and-categories.md) §1 kept out of page change sets and 0038 Q4 left open, so that `wbsetlabel` on an `M` ID is refused with `not-supported` — and **Commons' own MediaInfo** on files a tenant uses from Commons through a file repository ([0039](0039-files-and-media.md) §11, Q7). `providers.toml` already gives Wikidata's provider a type `M` with a Commons IRI, so a Commons MediaInfo entity is `WDM{pageid}`, and nothing yet reads it. [0045](0045-table-content-model.md) Q6 waits on captions before `M` IDs can be table rows.

### Direction

James's direction, from the design discussion of 2026-10-05:

- **Commons is a provider of MediaInfo under the foreign entity prefix `WDM`.** Not a provider of its own.
- Captions now.

## Decision

### 1. Captions are the File page's terms (amends 0038 §1; amends 0041 §7; settles 0038 Q4 for File pages)

*Changed by A1.*

*Current text: [12](../architecture/12-files-and-media.md) §8.4.*

### 2. Commons' MediaInfo on foreign files (extends 0039 §11; settles 0039 Q7)

*Current text: [12](../architecture/12-files-and-media.md) §6.6.*

### 3. `M` and `WDM` as table rows (settles 0045 Q6)

*Current text: [15](../architecture/15-structured-pages.md) §2.3, §2.4.*

### 4. Storage, API and RDF (extends 0013 §5.6; extends 0012 §5)

*Changed by A1.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §5.6; [03](../architecture/03-storage-caches-and-search.md) §4.2, §4.3, §4.9, §5, §11.1; [18](../architecture/18-api.md) §2.1, §2.3, §3.2.*

### 5. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **Commons as a provider of its own**, with a code and number. Rejected by direction: the IDs are already `WDM`, and a Commons MediaInfo entity's statements use Wikidata's properties and items, so it is Wikidata's provider family with a second host.
- **Captions as page properties**, like the short description. A caption is a term in many languages with Wikibase's API around it; `view.term` already models that and search already reads it.
- **Settling page terms for all pages here.** File pages have Commons' contract to follow; other pages have no such contract and the short description serves them; 0038 Q4 stays open for them.

## Consequences

- **Structured Data on Commons tools work against a tenant's files**, captions included, and a tenant that uses Commons files sees Commons' structured data on them, overlaid with its own.
- **`WDM` is one more thing the Wikidata adapter mirrors**, from a second host, under the same provider; the mirror graph and stream carry it unchanged.
- **Tables and scopes over files** work (0045 Q6 settled), which is what a Commons-style curation workspace needs.
- **Test plan.** A caption round-trips through `wbsetlabel`, `wbgetentities`, search and the dump; a Commons file used by a page gets its `WDM` fetched on demand and shown overlaid; a local caption override wins for its language; `same-as` on `M` is refused; a `statement` scope over `M` subjects feeds a table with thumbnails.

## Open questions

- **Q1. Captions on inherited files without a local page.** A foreign file shown only through the stack has no local page to carry an override; whether a caption override may be written against the ranged page ID, as 0052 Q1 asks for page statements.
- **Q2. Commons' MediaInfo dump at scale** for `mediainfo.mirror = all`: tens of millions of entities; whether the adapter should take a filtered dump (files referenced by the tenant's mirrored statements) instead.
- **Q3. Depicts as a scope with qualifiers.** `P180` with `P2677` (relative position) and `P1354` (shown with features) qualifiers; a `statement` kind with qualifier filters is 0060's question too.
- **Q4. Caption-only search profile** for an image-heavy tenant, ranking captions above statements.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §5 | amends | 0005 A68 |
| [0012](0012-api-requirements.md) §5 | §4 | extends | 0012 A45 |
| [0013](0013-postgres-storage.md) §5.6 | §4 | extends | 0013 A34 |
| [0038](0038-page-metadata-and-categories.md) §1 | §1 | amends | 0038 A9 |
| [0039](0039-files-and-media.md) §11 | §2 | extends | 0039 A8 |
| [0041](0041-content-models.md) §7 | §1 | amends | 0041 A12 |
| [0045](0045-table-content-model.md) §4 | §3 | extends | 0045 A5 |

## References

- [Commons:Structured data](https://commons.wikimedia.org/wiki/Commons:Structured_data) and [Extension:WikibaseMediaInfo](https://www.mediawiki.org/wiki/Extension:WikibaseMediaInfo): captions as labels, the `mediainfo` slot, `M` IDs
- [Commons SDC RDF export](https://commons.wikimedia.org/wiki/Commons:SPARQL_query_service): `rdfs:label` for captions on `sdc:M` nodes
- [0041](0041-content-models.md) §6–7, the contract this ADR completes; [0039](0039-files-and-media.md) §11, foreign file repositories

## Amendment log

### A1. Caption triples go on the page node

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §1, §4
- **Summary:** Caption triples (`rdfs:label`, `schema:description` per language) are emitted on the page node `{base}/page/{page ID}` ([0038](0038-page-metadata-and-categories.md) §11, [0041](0041-content-models.md) §7); there is no "`M` node". (PENDING E24)

Replaced text (§1):

> **RDF**: the `M` node ([0038](0038-page-metadata-and-categories.md) §11) gains `rdfs:label` and `schema:description` per language, as Commons' SDC export has them.

Replaced text (§4):

> - **RDF**: captions on the `M` node (§1); `WDM` entities in the Wikidata mirror graph and, through resolution, in the resolved view of the tenant's own File pages for foreign files, as any mirrored entity.

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§5
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [12](../architecture/12-files-and-media.md), [15](../architecture/15-structured-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
