# 0041. Content models

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-01 (A6)
- **Author:** James Hare / Claude Opus
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0017](0017-entity-id-grammar.md), [0019](0019-discussions.md), [0029](0029-resolver-namespaces.md), [0038](0038-page-metadata-and-categories.md), [0039](0039-files-and-media.md)
- **Uses:** [0035](0035-adopting-a-wikibase.md), [0043](0043-lua-modules.md), [0045](0045-table-content-model.md), [0048](0048-notation.md), [0049](0049-boards.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

MediaWiki decides what a page is with its **content model**. A content handler for each model says how the page's content is stored, validated, serialized, diffed and rendered, and whether `action=edit` may change it. The namespace supplies a default model. Wikibase uses the same mechanism to make `Item:Q42` a page whose content is an entity (`wikibase-item`), and WikibaseMediaInfo uses its multi-content extension to put an entity in a second **slot** of a File page (`wikibase-mediainfo`).

Triplespace has built the same thing piece by piece without naming it:

| Where | What it is in practice |
|---|---|
| [0008](0008-namespaces-and-document-pages.md) §1 | Namespace kinds `entity view` and `document`: what a page is made of |
| [0008](0008-namespaces-and-document-pages.md) §5 | Content models, by that name, for document pages only: `wikitext`, `markdown`, `json`, `yaml`, `text` |
| [0019](0019-discussions.md) §2–3, §11 | Kinds `composite` and `thread`, and the values `thread` and `talk` in `view.page.content_model` |
| [0019](0019-discussions.md) §9 | A new error code, `ts-structured`, to tell bots that a page is not text |
| [0029](0029-resolver-namespaces.md) §2 | Kind `resolver`, for namespaces that hold no pages |
| [0039](0039-files-and-media.md) §1 | Kind `file`: "`document` plus uploads" |
| [0038](0038-page-metadata-and-categories.md) §1 | Page statements in one history with page text, shown "as MediaWiki shows a revision that changed only a secondary slot" |

So the namespace `kind` answers three questions at once: whether a namespace holds pages at all, what its pages are, and what extra things they can do. Only the second is a content model. The others are properties of the namespace.

MediaWiki clients already ask the content-model question. `meta=siteinfo` reports `defaultcontentmodel` per namespace (the 1.43 snapshot reports it for `Item` and `Property` only, because only Wikibase sets one), `prop=info` and `prop=revisions` report each page's model, and the core write modules refuse edits by model with error codes clients already handle.

James's direction, from the design discussion of 2026-09-30:

- The concept exists in all but name: entity views and other projections, wikitext pages, markdown and files. Making it canonical, with each namespace naming its models, helps compatibility with the MediaWiki API.
- **A generic content model needs no prefix. A model unique to Triplespace takes the prefix `triplespace-`.**
- **Page statements need not be REST-only**, if an existing MediaWiki API contract can carry them. What Triplespace must not do is invent a convention to force them into the MediaWiki API. Wikibase-style statements are expected on files, because of Structured Data on Commons, and not on other kinds of page.

## Decision

### 1. Every page has a content model (amends 0008 §1; extends 0008 §5)

**Every page, whatever its namespace, has exactly one content model in its main slot.** That includes entity pages, threads, talk pages and file pages, not only the document pages of [0008](0008-namespaces-and-document-pages.md) §5. The model decides where the page's content comes from (§5), how it is serialized and validated, and whether `action=edit` can change it.

**Where a page's model is recorded depends on its source** (§5):

- For a **text** model, the model is stored in the page's `page` records, as [0008](0008-namespaces-and-document-pages.md) §4–5 already does, and can change with `action=changecontentmodel`.
- For every other source, the model is **fixed by the registry**: an entity page's model follows from its entity type, a thread's and a talk page's from their namespace. It is never stored in a record and never changes.

### 2. Model IDs: generic, Triplespace, reserved

| Kind of model | ID | Examples |
|---|---|---|
| **MediaWiki's or a Wikibase extension's** | Their ID, unchanged and with their meaning | `wikitext`, `json`, `text`, `wikibase-item`, `wikibase-mediainfo` |
| **Generic**: a format the wider world already names, with no Triplespace-specific meaning | The plain name, no prefix | `markdown`, `yaml` |
| **Unique to Triplespace** | `triplespace-` and a name | `triplespace-thread`, `triplespace-domain` |

**Every ID that MediaWiki core or any Wikibase extension uses is reserved**, as their namespace numbers are ([0008](0008-namespaces-and-document-pages.md) §2, rule 1): it is listed in the registry, never given another meaning, and not implemented unless an ADR says so. Reserved now and not implemented: `css`, `javascript` and `unknown` (core); `wikibase-lexeme` (WikibaseLexeme); `EntitySchema` (EntitySchema). `css` and `javascript` stay unimplemented for the reason [0008](0008-namespaces-and-document-pages.md) §5 gives.

IDs that other MediaWiki extensions use are not reserved. A generic ID is used only with the meaning the wider ecosystem gives it, so it cannot clash with another extension's use of it.

### 3. The content model registry (extends 0008 §5)

*Changed by A2, A3, A4, A5.*

The models are registry data, in a new file, `docs/registry/content-models.toml`, embedded by `scatter-pages`. Each entry records the ID, its origin, its **source** (§5), its **slot role**, the entity type where there is one, the serialization format reported as MediaWiki's `contentformat`, and whether it supports direct editing. The defaults:

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `wikitext` | MediaWiki | text | main | `text/x-wiki` | Yes | 0, 2, 4, 6, 14 |
| `markdown` | Generic | text | main | `text/markdown` | Yes | — (allowed in 0, 2, 4) |
| `json` | MediaWiki | text | main | `application/json` | Yes | — (allowed in 0, 2, 4) |
| `yaml` | Generic | text | main | `application/yaml` | Yes | — (allowed in 0, 2, 4) |
| `text` | MediaWiki | text | main | `text/plain` | Yes | — (allowed in 0, 2, 4) |
| `wikibase-item` | Wikibase | entity (`item`) | main | `application/json` | No | 120 |
| `wikibase-property` | Wikibase | entity (`property`) | main | `application/json` | No | 122 |
| `triplespace-domain` | Triplespace | entity (`domain`) | main | `application/json` | No | 210 |
| `triplespace-keyword` | Triplespace | entity (`keyword`) | main | `application/json` | No | 212 |
| `triplespace-notation` | Triplespace | entity (`notation`) | main | `application/json` | No | 216 ([0048](0048-notation.md) §7) |
| `triplespace-thread` | Triplespace | thread | main | `application/json` | No | 214 |
| `triplespace-talk` | Triplespace | composite | main | — | No | Every enabled talk namespace |
| `wikibase-mediainfo` | WikibaseMediaInfo | statements (`mediainfo`) | `mediainfo` | `application/json` | No | The second slot of every File page (§6) |
| `Scribunto` | Scribunto | text | main | `text/plain` | Yes | 828, by Scribunto's title rules ([0043](0043-lua-modules.md) §3) |
| `triplespace-table` | Triplespace | text | main | `application/json` | Yes | 218 ([0045](0045-table-content-model.md) §3) |
| `triplespace-board` | Triplespace | text | main | `application/json` | Yes | 310 ([0049](0049-boards.md) §3) |

**Two models hold a definition, not the page's content.** A `triplespace-table` page is a definition naming entities and properties, from which the grid is generated ([0045](0045-table-content-model.md) §3); a `triplespace-board` page is a definition, from which the thread listing is generated ([0049](0049-boards.md) §3).

**One model per entity type.** Every entity namespace hosts one entity type ([0008](0008-namespaces-and-document-pages.md) §1), so it has one model, whichever provider the entity comes from: `Item:Q5`, `Item:WDQ42` and `Item:OAW123` are all `wikibase-item`. An entity type added by a later ADR takes a model in the same change: `wikibase-` and the type if a Wikibase extension already defines it, `triplespace-` and the type otherwise. When Lexeme (146) is implemented, it takes `wikibase-lexeme`.

**An instance may add models**, as 0008 §5 already allows, as `config` records. It may not reuse a reserved ID, or take the `triplespace-` prefix for a model the repository does not define.

### 4. Namespaces name models, not kinds (amends 0008 §1, 0019 §2–3, 0029 §2 and 0039 §1)

**A namespace's `kind` now says only whether it holds pages:**

| Kind | Meaning |
|---|---|
| `pages` | It holds pages. The entry names the **allowed models** and the **default model** |
| `reserved` | Listed in `meta=siteinfo`; holds no pages ([0008](0008-namespaces-and-document-pages.md) §2, rule 1) |
| `virtual` | Its pages are generated by the server: `Special`, `Media` |
| `resolver` | Every title is a key that performs a lookup ([0029](0029-resolver-namespaces.md) §2). It holds no pages, so it has no model |

The name is `pages`, not `content`, because MediaWiki's siteinfo already uses `content` for a different flag: whether a namespace counts as article space.

**The five old kinds that held pages become models and one flag:**

| Old kind | Now |
|---|---|
| `entity` (entity view) | `pages`, with the entity type's model (§3) |
| `document` | `pages`, with text models |
| `file` | `pages`, with `wikitext` and **`uploads = true`**: a page in the namespace may carry upload records ([0039](0039-files-and-media.md) §2). This is what 0039 §1 already said the kind meant: "`document` plus uploads" |
| `composite` (talk) | `pages`, with `triplespace-talk` |
| `thread` | `pages`, with `triplespace-thread` |

`entity_types` leaves the namespace entry: the model names the entity type. Everything else in an entry stays (normalizer, `case`, subpages, paired talk, creation rule). The registry rejects a `pages` namespace whose allowed models mix sources, since a page can only change between models of the same source (§5).

### 5. Sources, and what a model provides (extends 0008 §5)

A model's **source** says where its content comes from and how it is written:

| Source | Content comes from | Written by | Revisions |
|---|---|---|---|
| **text** | The full text stored in each `page` record ([0008](0008-namespaces-and-document-pages.md) §4) | `action=edit` and the page routes | Its own |
| **entity** | The fold of the entity's change sets, resolved across source graphs ([0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md)) | Change sets: the Wikibase modules, ingest | Its own (mirrored entities' are provider-ranged, [0013](0013-postgres-storage.md) §6) |
| **thread** | The fold of the thread's records ([0019](0019-discussions.md) §1) | The thread routes | Its own: one per record |
| **composite** | Other pages: a talk page is the threads attached to it ([0019](0019-discussions.md) §2) | Nothing; it changes when its members do | None of its own: its history is its members' ([0019](0019-discussions.md) §7) |
| **statements** | A page's statement change sets ([0038](0038-page-metadata-and-categories.md) §1) | The Wikibase modules on an `M` ID (§7), and the page statement routes | The page's own; a statement revision is a page revision |

**Direct editing follows the source.** Only text models support it. This is MediaWiki's `ContentHandler::supportsDirectEditing()` as one registry field, and it replaces the per-kind refusals ADRs have been writing one at a time (§8).

**The content-model trait in `scatter-pages`** (0008 §5) grows from "validate and render text" to what every source needs:

| Operation | text | entity | thread | composite | statements |
|---|---|---|---|---|---|
| Validate submitted content | 0008 §5 | — | — | — | — |
| Serialize as of a revision (`rvprop=content`, export) | The text | Wikibase JSON ([wikibase-compat.md](../api/wikibase-compat.md)) | The `GET /thread/{page ID}` body ([0019](0019-discussions.md) §9) as of that revision | — | MediaInfo JSON (§7) |
| Plain text for search | 0014 §7 `text` | The `entities` index fields | Subject and posts ([0019](0019-discussions.md) §11) | — | `statement_keywords` ([0038](0038-page-metadata-and-categories.md) §12) |
| Diff | Line diff | Wikibase's entity diff ([0012](0012-api-requirements.md) §7) | Per post | — | Statement diff |

Rendering stays where earlier ADRs put it: text models in `scatter-pages` and `scatter-wikitext` (0008 §8, [0033](0033-backend-stack.md)), entity pages in the frame of [0010](0010-site-ui.md) §2, threads in [0019](0019-discussions.md) §8. The trait is implemented by the crate that owns each source (§12); `scatter-pages` holds the trait and the registry, and depends on none of them.

### 6. Slots: `main` everywhere, `mediainfo` on File pages

**Every page has one slot, `main`, holding its model.** That is all MediaWiki clients see of most pages, as on a MediaWiki without multi-content revisions.

**File pages have a second slot, `mediainfo`,** with the model `wikibase-mediainfo`. It holds the page's statements ([0038](0038-page-metadata-and-categories.md) §1) presented as a MediaInfo entity (§7). This is WikibaseMediaInfo's arrangement on Commons: the entity namespace for `mediainfo` is the File namespace, slot `mediainfo`.

**Nothing in storage changes.** A slot is how the Action API presents a page's records. The statement change sets stay where 0038 §1 put them, in the page's one history. A revision that changed only statements already appears "as MediaWiki shows a revision that changed only a secondary slot" (0038 §1). On a File page that secondary slot now has a name.

**A slot is added only to adopt an existing MediaWiki contract.** Statements on other pages (document pages, threads) get no slot, because no MediaWiki or Wikibase client expects statements there and Triplespace does not invent a convention to put them in the MediaWiki API. They stay on the REST routes of [0038](0038-page-metadata-and-categories.md) §13.

### 7. MediaInfo: a File page's statements over the Wikibase API (amends 0038 §1 and §13; extends 0017 §1)

On File pages, 0038 §1's "no entity ID" is replaced by WikibaseMediaInfo's contract, because tools written for Structured Data on Commons expect it.

**The ID is `M` followed by the page ID**: the File page with page ID 1234 has the MediaInfo ID `M1234`, as on Commons. It has the local form of [0017](0017-entity-id-grammar.md) §1, but it is **derived, not minted**: it is a name for one File page's statements. It is not a row in `view.entity`, never joins an identity cluster, cannot be the value of an entity data type, and goes when the page goes. `M` followed by the ID of a page that is not a File page, or does not exist, is a missing entity. Because adoption keeps source page IDs ([0035](0035-adopting-a-wikibase.md) §4, [0039](0039-files-and-media.md) §14), a file's `M` ID survives adoption from a wiki that numbered it.

**Reading:**

- `wbgetentities&ids=M1234` returns a `mediainfo` entity in WikibaseMediaInfo's serialization: `type`, `id`, `title` (the File title), `ns` 6, `pageid`, `lastrevid`, `modified`, `labels`, `descriptions`, and the page's resolved statements under `statements`. Projected statements ([0038](0038-page-metadata-and-categories.md) §2) are included, since they are part of what the page asserts.
- A File page with no statements is what WikibaseMediaInfo calls a *virtual* entity, and is returned as Commons returns one. The exact shape is pinned by a contract test against Commons before implementation.
- `wbgetentities` by `sites` and `titles` with a File title on the tenant's own site returns the MediaInfo entity, as it does on Commons. For other titles it finds the paired item, as 0038 §13 says.
- `prop=revisions&rvslots=mediainfo` (or `*`) returns the same JSON as of each revision, with `contentmodel` `wikibase-mediainfo` and `contentformat` `application/json`.
- `Special:EntityPage/M1234` and `Special:EntityData/M1234` resolve to the File page and its MediaInfo JSON.

**Writing.** The statement modules accept an `M` ID: `wbeditentity` (statements only), `wbcreateclaim`, `wbsetclaim`, `wbremoveclaims`, `wbsetclaimvalue`, `wbsetqualifier`, `wbremovequalifiers`, `wbsetreference` and `wbremovereferences`. Each writes a page change set exactly as the REST routes do (0038 §1): the same records, permissions, protection, filters and constraints. `baserevid` is the page's latest revision ID of any kind (text, statements or upload), which gives the base offset. A write to a projected statement is refused with `ts-derived-statement` (0038 §13).

**Terms and sitelinks are not supported yet.** MediaInfo's labels are Commons' *captions*. They are page terms, which [0038](0038-page-metadata-and-categories.md) Q4 leaves open, and 0038 §1 keeps terms out of page change sets. Until that is settled, `labels` and `descriptions` are always empty, and `wbsetlabel`, `wbsetdescription`, `wbsetaliases`, `wbsetsitelink` and a `wbeditentity` carrying terms or sitelinks on an `M` ID are refused with Wikibase's `not-supported` ("The requested feature is not supported by the given entity").

**Unchanged:** REST stays `/page/{pageid}/statements` for every page, File pages included; the Wikibase REST API has no MediaInfo routes to follow. RDF stays as [0038](0038-page-metadata-and-categories.md) §11 has it, with the page node as subject. Statements on pages other than File pages stay out of the Action API.

### 8. Action API (extends 0012 §4; amends 0019 §9)

| Module | Behaviour |
|---|---|
| `meta=siteinfo&siprop=namespaces` | `defaultcontentmodel` on every `pages` namespace whose default is not `wikitext`, as MediaWiki reports it: `wikibase-item` on 120, `triplespace-domain` on 210, `triplespace-thread` on 214, `triplespace-talk` on each enabled talk namespace |
| `meta=siteinfo&siprop=triplespace` | Gains `contentmodels`: each implemented model with its source, slot, format and direct-editing flag, so the UI finds out from the API which pages can change format ([0012](0012-api-requirements.md) §1) |
| `action=paraminfo` | Every `contentmodel` enumeration lists the implemented models, as MediaWiki lists its registered handlers ([mediawiki-compat.md](../api/mediawiki-compat.md) treats these values as site-specific) |
| `prop=info` | `contentmodel` for every page ID: document, entity, thread, talk and file pages ([0013](0013-postgres-storage.md) §6 already gives them one `pageid` space) |
| `prop=revisions`, `prop=deletedrevisions` | `rvslots` (and `drvslots`) takes `main` and, on File pages, `mediainfo`. `rvprop=contentmodel` is per slot. `rvprop=content` returns the serialization of §5. A talk page's rows are its threads' revisions (0019 §7) and carry their content |
| `action=edit` | Text models only. Any other model is refused with MediaWiki's `no-direct-editing` (`apierror-no-direct-editing`), which **replaces 0019 §9's `ts-structured`**. A `contentmodel` the namespace does not allow is refused as MediaWiki refuses one its handler cannot use on the title |
| `action=changecontentmodel` | Between text models the namespace allows. A page whose current model is not text: `changecontentmodel-nodirectediting`. A new model the namespace does not allow, including every non-text model: `changecontentmodel-cannotbeused`. The same model: `nochanges`. These are the codes MediaWiki core gives |
| `action=parse`, `action=compare` | `contentmodel` accepts text models only. `compare` takes `fromslots` and `toslots` of `main` and `mediainfo` |
| Wikibase modules | Accept `M` IDs as §7 describes |

### 9. REST (extends 0012 §5)

Page objects in MediaWiki REST's shape (`GET /v1/page/{title}/bare` and its relatives) carry `content_model` for every page, as MediaWiki's do. In `triplespace/v0`, page summaries carry `content_model` and, for File pages, `mediainfo_id`. No new route is added.

### 10. Storage, search, filters, RDF and the UI

- **`view.page`** ([0013](0013-postgres-storage.md) §5): `content_model` holds registry IDs. Threads and talk pages are written as `triplespace-thread` and `triplespace-talk`, which **replaces 0019 §11's `thread` and `talk`**. Entity pages keep no row in `view.page`, and their model is read from the registry by entity type. The `registry` projection gains the kind `content_model`.
- **Search** ([0014](0014-caches-and-search.md) §7): the `content_model` field of the `pages` index takes the same IDs.
- **Filters** ([0030](0030-edit-filters.md) §2): `content_model` already has MediaWiki's meaning, so it takes the registry IDs: a filter on posts tests `content_model == "triplespace-thread"`.
- **RDF** ([0001](0001-revision-metadata-rdf.md) §6): every revision node in the metadata graph carries `sw:contentModel`, not only document revisions; a statement revision of a File page carries `wikibase-mediainfo`.
- **UI** ([0010](0010-site-ui.md) §4): the Format selector and the model shown beside the title appear only for text models, and the selector only where the namespace allows more than one.

### 11. Scatterbase

In Scatterbase's terms, the stored bytes are the blob and the content model is the view (0008 §5). Scatterbase has no namespaces, but the registry and the trait are in a substrate crate, and the source, serialization and diff parts apply to any claim store that serves pages. Whether Scatterbase uses them is its own decision.

### 12. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed. The table this section first gave is in A1.

## Consequences

- **One concept replaces five page kinds and two ad hoc values.** What a page is made of is its model; whether a namespace has pages is its kind; uploads are a flag.
- **MediaWiki clients see true models everywhere.** Siteinfo, `prop=info` and `prop=revisions` agree with what a page is, entity pages and threads included.
- **Refusals use MediaWiki's own codes.** `ts-structured` is retired before any code used it. A bot that handles `no-direct-editing` on Wikidata handles it here.
- **Structured Data on Commons tools can work on File pages.** Reading and editing statements on `M` IDs uses the contract they were written for. Captions do not work yet (§7).
- **No convention was invented.** Page statements outside File pages remain REST-only, because no MediaWiki contract for them exists.
- **The namespace registry is rewritten** before any code reads it: kinds collapse to four, and `entity_types` becomes models.
- **Two Triplespace model IDs change** before any data uses them: `thread` and `talk` become `triplespace-thread` and `triplespace-talk`.

## Open questions

- **Q1. Captions.** MediaInfo labels are page terms, which [0038](0038-page-metadata-and-categories.md) Q4 leaves open. If File pages get captions, they are the first page terms, and the term modules on `M` IDs start working.
- **Q2. Data type IDs.** `wikibase-domain`, `wikibase-keyword` and `wikibase-notation` ([0009](0009-keyed-entity-types-and-domain.md), [0017](0017-entity-id-grammar.md) §5, [0048](0048-notation.md) §1) are unique to Triplespace but carry Wikibase's prefix. Whether §2's rule applies to data types too, so they become `triplespace-domain` and so on before any data exists, is for a later decision.
- **Q3. A MediaInfo IRI.** Whether the RDF should also give each File page's statements the entity IRI `{base}/entity/M{pageid}`, so that queries written for the Commons Query Service (`sdc:M…`) work unchanged.
- **Q4. Adopting a wiki that ran WikibaseMediaInfo.** Importing its `mediainfo` slots as page statements on the adopted File pages ([0035](0035-adopting-a-wikibase.md), [0039](0039-files-and-media.md) §14).
- **Q5. Mirrored Commons MediaInfo** (`WDM`) shown on foreign files, which [0039](0039-files-and-media.md) Q7 leaves open. With this ADR, a mirrored `WDM` entity would be the `mediainfo` slot of a foreign File page, not the page of a MediaInfo namespace.
- **Q6. EntitySchema's namespace.** This ADR reserves the `EntitySchema` model; by [0008](0008-namespaces-and-document-pages.md) §2 rule 1, its namespace numbers should be reserved in `namespaces.toml` too.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0001](0001-revision-metadata-rdf.md) §6 | §10 | extends | 0001 A13 |
| [0005](0005-crate-organization.md) §2 | §12 | extends | 0005 A41 |
| [0008](0008-namespaces-and-document-pages.md) §1, §3, §5 | §1–5 | amends | 0008 A11 |
| [0009](0009-keyed-entity-types-and-domain.md) §11 | §3–4 | amends | 0009 A5 |
| [0010](0010-site-ui.md) §4 | §10 | amends | 0010 A25 |
| [0012](0012-api-requirements.md) §4, §5 | §8–9 | extends | 0012 A25 |
| [0013](0013-postgres-storage.md) §5.4 | §10 | amends | 0013 A15 |
| [0014](0014-caches-and-search.md) §7 | §10 | amends | 0014 A6 |
| [0017](0017-entity-id-grammar.md) §1 | §7 | extends | 0017 A5 |
| [0019](0019-discussions.md) §2, §3, §9, §11 | §4, §8, §10 | amends | 0019 A9 |
| [0029](0029-resolver-namespaces.md) §2 | §4 | amends | 0029 A3 |
| [0038](0038-page-metadata-and-categories.md) §1, §13 | §6–7 | amends | 0038 A3 |
| [0039](0039-files-and-media.md) §1 | §4, §6–7 | amends | 0039 A3 |

## References

- [Manual:ContentHandler](https://www.mediawiki.org/wiki/Manual:ContentHandler) and [Multi-Content Revisions](https://www.mediawiki.org/wiki/Multi-Content_Revisions)
- MediaWiki core 1.43: `ApiEditPage` (`apierror-no-direct-editing`) and `ContentModelChange` (`apierror-changecontentmodel-nodirectediting`, `apierror-changecontentmodel-cannotbeused`, `apierror-nochanges`)
- [Extension:WikibaseMediaInfo](https://www.mediawiki.org/wiki/Extension:WikibaseMediaInfo): `M` IDs from page IDs, virtual entities; `MediaInfoContent::CONTENT_MODEL_ID` is `wikibase-mediainfo`; its entity namespace is the File namespace, slot `mediainfo`
- [T223792](https://phabricator.wikimedia.org/T223792): `wbgetentities` by `sites` and `titles` on Commons File pages
- Wikibase API message `wikibase-api-not-supported`
- [RFC 7763](https://www.rfc-editor.org/rfc/rfc7763) (`text/markdown`) and [RFC 9512](https://www.rfc-editor.org/rfc/rfc9512) (`application/yaml`)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-30
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §12
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A41).

Replaced text (§12):

> | Crate | Change |
> |---|---|
> | `scatter-pages` | The content model registry, embedding `docs/registry/content-models.toml`; the trait of §5 with source, slot, format and direct-editing; text models as before |
> | `scatter-threads` | Implements `triplespace-thread`: serialization as of a revision, and per-post diff |
> | `scatter-wikibase-model` | Implements the entity models and `wikibase-mediainfo`: the `mediainfo` entity type and its serialization, and derived `M` IDs in the ID parser ([0017](0017-entity-id-grammar.md) §1). Gains a dependency on `scatter-pages` for the trait; `scatter-pages` depends on nothing, so no cycle results |
> | `triplespace-titles` | The `pages` kind, allowed and default models and the `uploads` flag in the namespace registry; the registry check that a namespace's models share a source |
> | `triplespace-projections` | `view.page.content_model` from the registry; the `content_model` registry kind |
> | `triplespace-api-action`, `triplespace-api-rest` | `defaultcontentmodel`, `siprop=triplespace` `contentmodels`, the `contentmodel` enumerations, `no-direct-editing` and the `changecontentmodel` codes, the `mediainfo` slot, the Wikibase modules on `M` IDs, `content_model` on page objects |
>
> No crate is added.

### A2. The `Scribunto` model

- **Date:** 2026-09-30
- **Source:** [0043](0043-lua-modules.md) §3
- **Change:** extends §3
- **Summary:** `Scribunto` (Scribunto's ID, origin Scribunto): source text, slot `main`, format `text/plain`, direct editing yes, the default in Module (828) by Scribunto's title rules.

### A3. The `triplespace-table` model

- **Date:** 2026-09-30
- **Source:** [0045](0045-table-content-model.md) §3
- **Change:** extends §3
- **Summary:** `triplespace-table` (origin Triplespace): source text, slot `main`, format `application/json`, direct editing yes, the default in Table (218). Its content is a definition naming entities and properties; the grid is generated from the entities.

### A4. `triplespace-osm-tag` becomes `triplespace-notation`

- **Date:** 2026-10-01
- **Source:** [0048](0048-notation.md) §1, §7
- **Change:** amends §3
- **Summary:** `triplespace-osm-tag` is replaced by `triplespace-notation`, entity type `notation`, the default in Notation (216). The rest of its row is unchanged. Q2 names `wikibase-notation` in place of `wikibase-osm-tag`.

Replaced text (§3):

> | `triplespace-osm-tag` | Triplespace | entity (`osm-tag`) | main | `application/json` | No | 216 |

Replaced text (Q2):

> - **Data type IDs.** `wikibase-domain`, `wikibase-keyword` and `wikibase-osm-tag` (now `wikibase-notation`, [0048](0048-notation.md)) ([0009](0009-keyed-entity-types-and-domain.md), [0017](0017-entity-id-grammar.md) §5, [0036](0036-openstreetmap-providers.md)) are unique to Triplespace

### A5. The `triplespace-board` model

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §3
- **Change:** extends §3
- **Summary:** `triplespace-board` (origin Triplespace): source text, slot `main`, format `application/json`, direct editing yes, the default in Board (310). Its content is a definition; the thread listing is generated.

### A6. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §3, §12
- **Summary:** A1–A5 were folded into the Decision: the three added models are rows of §3's table, with a paragraph on the two definition models. The open questions were numbered. No decision changed. Before this, A2–A5 were blockquotes, and A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.
