# 0065. MediaInfo: captions, and Commons as a source

- **Status:** Proposed
- **Date:** 2026-10-05
- **Updated:** 2026-10-09 (A3)
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

*Changed by A1, A3.*

*Current text: [12](../architecture/12-files-and-media.md) §8.4.*

### 2. Commons' MediaInfo on foreign files (extends 0039 §11; settles 0039 Q7)

*Changed by A3.*

*Current text: [12](../architecture/12-files-and-media.md) §6.6.*

### 3. `M` and `WDM` as table rows (settles 0045 Q6)

*Current text: [15](../architecture/15-structured-pages.md) §2.3, §2.4.*

### 4. Storage, API and RDF (extends 0013 §5.6; extends 0012 §5)

*Changed by A1, A3.*

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
- **A foreign file is annotated through a local page.** The `M` ID of that page keys the annotations, and `M`–`WDM` is a cluster whose canonical the lookup fixes, so nothing is keyed by a ranged page ID (A3).

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

### A3. Captions are term operations; `M` and `WDM` are an ordinary cluster; a local File page is required

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §1, §2, §4
- **Summary:** The `terms` operation is withdrawn: captions are written with the same `add`, `remove` and `override` operations a change set uses for any term, on the subject `M{page ID}`, in the page's change set in `pages`; a caption on a local file is an `add`, replacing or clearing a Commons caption on a foreign file is an `override` or `remove` of the mirrored term; `wbsetlabel` and `wbsetdescription` on an `M` ID become those operations by the module mapping of [0084](0084-wikibase-writes-against-the-resolved-view.md) §2, which the chapter cites for that step. Local annotation of a foreign file requires a local File page, a `create` with no upload, keyed by that page's `M` ID; the ranged-ID overlay is dropped, and a foreign file no local page describes has no local statements. A local `M` and the `WDM` it mirrors are an ordinary identity cluster with a fixed canonical, the `WDM` member, formed by the file lookup when the local page is created, never re-pointed by a `same-as` or `different-from` record, and dissolved when the page is deleted or the file stops being the repository's; the two resolve and rewrite in responses as any cluster members do, and `wbgetentities&ids=M{page ID}` says `canonical: "WDM123"` rather than `foreign`. `mediainfo.mirror = on-demand` enqueues a fetch through `ops.entity_fetch` with reason `read`, and the page or statement renders without the statements until the fetch lands. Page subjects use one key form across `view`: `M{page ID}` for a File page, the decimal page ID otherwise. (REVIEW G44)

Replaced text ([12](../architecture/12-files-and-media.md) §8.4, as it stood):

> **A File page's `mediainfo` slot carries labels and descriptions**, Commons' captions, in every language, as Wikibase terms. They are written by a **`terms`** operation on the page's change set in `pages` — the change-set payload already carries page statements ([0038](0038-page-metadata-and-categories.md) §1), and gains terms for File pages only — and projected into `view.term` with `entity_id = 'M{page ID}'` ([03](../architecture/03-storage-caches-and-search.md) §4.3), so every reader of terms (search, the label in the viewer's language, `wbgetentities`, the dump) sees them with no new table.

> `wbsetlabel`, `wbsetdescription` and a `wbeditentity` carrying `labels` or `descriptions` on an `M` ID are accepted and append the `terms` operation.

Replaced text ([12](../architecture/12-files-and-media.md) §6.6, as it stood):

> **Mirroring set**: `mediainfo.mirror` (site, default **`on-demand`**): a Commons file's MediaInfo is fetched through `Special:EntityData` ([0012](0012-api-requirements.md) §6's upstream fetch) the first time the file is used by a local page or shown, written as a `put`, and kept current by Commons' EventStreams `mediainfo` changes as [0053](0053-mirrored-pages.md) §6 keeps mirrored pages current;

> The local page's own `M{local page ID}` names the local overlay; `wbgetentities&ids=M{local}` returns the resolved view and says `foreign: "WDM123"` in `triplespace`, and `wbgetentities&ids=WDM123` returns the mirror alone.

> **Clusters**: a local `M` entity and the `WDM` it mirrors are **one subject by construction**, not a cluster: the local page *is* the foreign file's page here, with the ranged page ID of [0052](0052-page-repositories-and-title-inheritance.md) §6 ([04](../architecture/04-entities-and-identifiers.md) §5.3) where the file is only inherited. `same-as` between `M` IDs is refused, as §8.2's "derived, not minted" implies.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §4.3, as it stood):

> Captions are `term` rows with `entity_id` of the form `M{page ID}` and `WDM{page ID}`; the page projection writes the local ones from the `terms` operation, the mirror projection the foreign ones ([0065](0065-mediainfo-captions-and-commons.md) §4).
