# 0041. Content models

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-09 (A16)
- **Author:** James Hare / Claude Opus
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0017](0017-entity-id-grammar.md), [0019](0019-discussions.md), [0029](0029-resolver-namespaces.md), [0038](0038-page-metadata-and-categories.md), [0039](0039-files-and-media.md)
- **Uses:** [0035](0035-adopting-a-wikibase.md), [0043](0043-lua-modules.md), [0045](0045-table-content-model.md), [0048](0048-notation.md), [0049](0049-boards.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [10](../architecture/10-pages-and-content-models.md), [12](../architecture/12-files-and-media.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

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

*Current text: [10](../architecture/10-pages-and-content-models.md) §4.1, §4.5.*

### 2. Model IDs: generic, Triplespace, reserved

*Changed by A14.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §4.2.*

### 3. The content model registry (extends 0008 §5)

*Changed by A2, A3, A4, A5, A7, A8, A9, A10, A11, A13, A15.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §1.4, §4.4.*

### 4. Namespaces name models, not kinds (amends 0008 §1, 0019 §2–3, 0029 §2 and 0039 §1)

*Current text: [10](../architecture/10-pages-and-content-models.md) §1.1, §1.2.*

### 5. Sources, and what a model provides (extends 0008 §5)

*Current text: [10](../architecture/10-pages-and-content-models.md) §4.3.*

### 6. Slots: `main` everywhere, `mediainfo` on File pages

*Current text: [12](../architecture/12-files-and-media.md) §8.1.*

### 7. MediaInfo: a File page's statements over the Wikibase API (amends 0038 §1 and §13; extends 0017 §1)

*Changed by A12.*

*Current text: [12](../architecture/12-files-and-media.md) §8.2, §8.3, §8.4.*

### 8. Action API (extends 0012 §4; amends 0019 §9)

*Current text: [18](../architecture/18-api.md) §2.1, §2.2, §2.3, §8.*

### 9. REST (extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §3.1, §3.4.*

### 10. Storage, search, filters, RDF and the UI

*Current text: [10](../architecture/10-pages-and-content-models.md) §4.6.*

### 11. Scatterbase

*Current text: [22](../architecture/22-crates-and-stack.md) §1.3.*

### 12. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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
- **Q6.** ~~**EntitySchema's namespace.** This ADR reserves the `EntitySchema` model; by [0008](0008-namespaces-and-document-pages.md) §2 rule 1, its namespace numbers should be reserved in `namespaces.toml` too.~~ *Settled by [0064](0064-entityschema-and-validation.md) §2: 640 and 641 are implemented with the extension's meaning.*

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

### A7. `sanitized-css`

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §1
- **Change:** extends §3
- **Summary:** TemplateStyles' model joins the table as a text model, `text/css`, directly editable, validated by the `scatter-css` sanitizer at save; the default for `.css` titles in Template and allowed in Project for the site-styles page. 0008 §5's exclusion now covers `css` and `javascript` only.

### A8. The `triplespace-scope` model

- **Date:** 2026-10-04
- **Source:** [0060](0060-scopes.md) §3
- **Change:** extends §3
- **Summary:** `triplespace-scope` (origin Triplespace): source text, slot `main`, format `application/json`, direct editing yes, the default in Scope (312). Its content is a definition; the member list is projected.

Replaced text (§3, in part):

> **Two models hold a definition, not the page's content.**

### A9. The `triplespace-sprint` model

- **Date:** 2026-10-04
- **Source:** [0061](0061-sprints-and-tasks.md) §3
- **Change:** extends §3
- **Summary:** `triplespace-sprint` (origin Triplespace): source text, slot `main`, format `application/json`, direct editing yes; allowed on `Project` subpages, default nowhere. Its content is a definition; the task board is projected.

### A10. The `triplespace-sparql` model

- **Date:** 2026-10-05
- **Source:** [0063](0063-query-namespace.md) §3
- **Change:** extends §3
- **Summary:** `triplespace-sparql` (origin Triplespace): text, `application/json`, direct editing, the default in Query (124). A parameterized query; the result table is computed.

### A11. The `EntitySchema` model

- **Date:** 2026-10-05
- **Source:** [0064](0064-entityschema-and-validation.md) §3
- **Change:** extends §3
- **Summary:** `EntitySchema` moves from reserved to implemented as an entity model for the `entityschema` type, the default in 640. Q6 is settled.

### A12. Captions

- **Date:** 2026-10-05
- **Source:** [0065](0065-mediainfo-captions-and-commons.md) §1
- **Change:** amends §7
- **Summary:** Labels and descriptions on `M` IDs are captions, accepted and stored as the File page's terms; aliases and sitelinks stay refused.

Replaced text (§7):

> **Terms and sitelinks are not supported yet.** MediaInfo's labels are Commons' *captions*. They are page terms, which [0038](0038-page-metadata-and-categories.md) Q4 leaves open, and 0038 §1 keeps terms out of page change sets. Until that is settled, `labels` and `descriptions` are always empty, and `wbsetlabel`, `wbsetdescription`, `wbsetaliases`, `wbsetsitelink` and a `wbeditentity` carrying terms or sitelinks on an `M` ID are refused with Wikibase's `not-supported` ("The requested feature is not supported by the given entity").

### A13. The `wikibase-lexeme` model

- **Date:** 2026-10-05
- **Source:** [0066](0066-lexemes.md) §3
- **Change:** extends §3
- **Summary:** `wikibase-lexeme` moves from reserved to implemented as the entity model for `lexeme`, the default in 146.

### A14. The reserved-and-unimplemented models

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §2
- **Summary:** The models reserved and not implemented are `css`, `javascript`, `unknown` and `wikibase-query`; `wikibase-lexeme` and `EntitySchema` are implemented ([0066](0066-lexemes.md) §3, [0064](0064-entityschema-and-validation.md) §3; A11, A13), so §2's list no longer names them. (PENDING E16)

Replaced text (§2):

> Reserved now and not implemented: `css`, `javascript` and `unknown` (core); `wikibase-lexeme` (WikibaseLexeme); `EntitySchema` (EntitySchema).

### A15. The namespace catalogue says where a model is allowed

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §3
- **Summary:** The namespace catalogue ([0008](0008-namespaces-and-document-pages.md) §2 as amended; chapter 10 §1.4) is the one place that says where a model is allowed. §3's "Default in" column carries defaults only, and its "allowed in" notes go. (PENDING E17)

Replaced text (§3):

> | `markdown` | Generic | text | main | `text/markdown` | Yes | — (allowed in 0, 2, 4) |
> | `json` | MediaWiki | text | main | `application/json` | Yes | — (allowed in 0, 2, 4) |
> | `yaml` | Generic | text | main | `application/yaml` | Yes | — (allowed in 0, 2, 4) |
> | `text` | MediaWiki | text | main | `text/plain` | Yes | — (allowed in 0, 2, 4) |

> | `triplespace-sprint` | Triplespace | text | main | `application/json` | Yes | — ; allowed on Project (4) subpages ([0061](0061-sprints-and-tasks.md) §2–3) |

> | `sanitized-css` | TemplateStyles | text | main | `text/css` | Yes | Template (10) for titles ending in `.css`; allowed in Project (4) for the site-styles page ([0055](0055-templatestyles-templatedata-and-page-properties.md) §1) |

### A16. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§12
- **Summary:** The Decision's current text now lives in the architecture chapters [10](../architecture/10-pages-and-content-models.md), [12](../architecture/12-files-and-media.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
