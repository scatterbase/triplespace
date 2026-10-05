# 0065. MediaInfo: captions, and Commons as a source

- **Status:** Proposed
- **Date:** 2026-10-05
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0038](0038-page-metadata-and-categories.md), [0039](0039-files-and-media.md), [0041](0041-content-models.md), [0045](0045-table-content-model.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0011](0011-logs.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0017](0017-entity-id-grammar.md), [0023](0023-moderation.md), [0032](0032-sparql-update-stream.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

[0041](0041-content-models.md) §6–7 gave every File page a `mediainfo` slot and the MediaInfo ID `M{page ID}`, served through the Wikibase modules as Commons serves Structured Data. Two things were left: **captions** — MediaInfo's labels, which [0038](0038-page-metadata-and-categories.md) §1 kept out of page change sets and 0038 Q4 left open, so that `wbsetlabel` on an `M` ID is refused with `not-supported` — and **Commons' own MediaInfo** on files a tenant uses from Commons through a file repository ([0039](0039-files-and-media.md) §11, Q7). `providers.toml` already gives Wikidata's provider a type `M` with a Commons IRI, so a Commons MediaInfo entity is `WDM{pageid}`, and nothing yet reads it. [0045](0045-table-content-model.md) Q6 waits on captions before `M` IDs can be table rows.

### Direction

James's direction, from the design discussion of 2026-10-05:

- **Commons is a provider of MediaInfo under the foreign entity prefix `WDM`.** Not a provider of its own.
- Captions now.

## Decision

### 1. Captions are the File page's terms (amends 0038 §1; amends 0041 §7; settles 0038 Q4 for File pages)

**A File page's `mediainfo` slot carries labels and descriptions**, Commons' captions, in every language, as Wikibase terms. They are written by a **`terms`** operation on the page's change set in `pages` — the change-set payload already carries page statements ([0038](0038-page-metadata-and-categories.md) §1), and gains terms for File pages only — and projected into `view.term` with `entity_id = 'M{page ID}'`, so every reader of terms (search, the label in the viewer's language, `wbgetentities`, the dump) sees them with no new table. Aliases and sitelinks stay refused (`not-supported`), as Commons refuses them.

0041 §7's "Terms and sitelinks are not supported yet" becomes **"Labels and descriptions are captions; aliases and sitelinks are not supported"**: `wbsetlabel`, `wbsetdescription` and `wbeditentity` with `labels`/`descriptions` on an `M` ID are accepted and append the `terms` operation; `wbsetaliases` and `wbsetsitelink` stay refused. 0038 §1's "terms stay out of page change sets" is narrowed to pages other than File pages: **0038 Q4 is settled for File pages** and stays open for the rest, where the short description remains a page property ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6).

**Where captions show**: the File page's header, under the title, in the viewer's language with the fallback chain; the caption editor is the term editor of the entity page ([0003](0003-statement-ui.md) §8). **Search** indexes captions as the File page's terms ([0014](0014-caches-and-search.md) §7), so a search for "sunset over Lisbon" finds the file by caption. **RDF**: the `M` node ([0038](0038-page-metadata-and-categories.md) §11) gains `rdfs:label` and `schema:description` per language, as Commons' SDC export has them. **Diffs and history**: a caption change is a page revision whose diff shows the terms, as an item's does.

### 2. Commons' MediaInfo on foreign files (extends 0039 §11; settles 0039 Q7)

**A file served from a Commons file repository has its MediaInfo mirrored as the foreign entity `WDM{Commons page ID}`**, in the Wikidata provider's mirror graph as any `WD` entity ([0002](0002-source-graphs-and-mass-ingest.md) §2). The Wikidata provider's type `M` gains its own **source** fields in `providers.toml` — `api`, `entity_data` and `dumps` pointing at Commons — so that the adapter fetches `M` entities from `commons.wikimedia.org` while attributing them to the same provider; the IRI stays `https://commons.wikimedia.org/entity/M…` as the registry already has it.

**Mirroring set**: `mediainfo.mirror` (site, default **`on-demand`**): a Commons file's MediaInfo is fetched through `Special:EntityData` ([0012](0012-api-requirements.md) §6's upstream fetch) the first time the file is used by a local page or shown, written as a `put`, and kept current by Commons' EventStreams `mediainfo` changes as [0053](0053-mirrored-pages.md) §6 keeps mirrored pages current; **`linked`** mirrors every file any local page or statement references; **`all`** loads Commons' MediaInfo JSON dump through the adapter, which is a Wikidata-scale job and off by default (0039 Q6's scale concern). `off` disables it.

**The local page for a foreign file** ([0039](0039-files-and-media.md) §11) shows the mirrored statements and captions **as its page data**: the page's resolved view is the mirror's `WDM` state with the local graph's `add`/`override`/`remove` overlays ([0002](0002-source-graphs-and-mass-ingest.md) §7) applied, exactly as a foreign item's page shows Wikidata's statements with local corrections. The local page's own `M{local page ID}` names the local overlay; `wbgetentities&ids=M{local}` returns the resolved view and says `foreign: "WDM123"` in `triplespace`, and `wbgetentities&ids=WDM123` returns the mirror alone. **Captions on a foreign file** are overlaid the same way: a local caption `override`s the Commons one for that language on this tenant. Nothing is written to Commons; proposing a caption or statement back is the proposals ADR's job.

**Clusters**: a local `M` entity and the `WDM` it mirrors are **one subject by construction**, not a cluster: the local page *is* the foreign file's page here, with the ranged page ID of [0052](0052-page-repositories-and-title-inheritance.md) §6 where the file is only inherited. `same-as` between `M` IDs is refused, as 0041 §7's "derived, not minted" implies.

### 3. `M` and `WDM` as table rows (settles 0045 Q6)

A table's `ids`, and every scope kind, accept `M` and `WDM` IDs as entity-kind subjects. Term columns show captions; statement columns read the `mediainfo` slot; the ID cell links the File page and shows the thumbnail where the table's `render` asks for it (a `thumb` column kind, `field: "thumbnail"`, `width`). A scope `statement: P180 = Q146` over `M` subjects is "files depicting cats", which is what Commons-style curation needs.

### 4. Storage, API and RDF (extends 0013 §5.6; extends 0012 §5)

- `view.term` rows with `entity_id` of the form `M{page ID}` and `WDM{page ID}` (extends [0013](0013-postgres-storage.md) §5.6); the page projection writes the local ones from the `terms` operation, the mirror projection the foreign ones.
- `view.entity` gains rows for `WDM` as for any foreign entity; `view.file` ([0039](0039-files-and-media.md) §20) gains `mediainfo_id` for the foreign file's `WDM`.
- **REST**: `GET /file/{title}/mediainfo` returns the resolved view (local or overlaid foreign) with `foreign`; `/page/{pageid}/statements` is unchanged. **Action API**: the term modules on `M` (§1); `wbgetentities` on `WDM`.
- **RDF**: captions on the `M` node (§1); `WDM` entities in the Wikidata mirror graph and, through resolution, in the resolved view of the tenant's own File pages for foreign files, as any mirrored entity.
- **Logs**: a `terms` operation projects as a page edit; nothing new in [0011](0011-logs.md).

### 5. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-pages` | The `terms` operation on File page change sets |
| `scatter-wikibase-model`, `scatter-wikibase-rdf` | Terms on `mediainfo` entities; `rdfs:label`/`schema:description` on the `M` node |
| `scatter-providers` | Per-type source fields (`api`, `entity_data`, `dumps`) on a provider type |
| `scatter-adapter-wikidata` | `M` entities from Commons' endpoints and dump; the `mediainfo` EventStreams topic |
| `triplespace-projections` | `view.term` rows for `M`/`WDM`; the foreign-file resolved view; `mediainfo.mirror` on-demand fetch and `linked` tracking |
| `triplespace-api-action`, `triplespace-api-rest` | The term modules on `M`; `GET /file/{title}/mediainfo` |
| `triplespace-ui` | Captions in the File page header and editor; the overlaid view of a foreign file's data; the `thumb` table column |

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
