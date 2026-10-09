# 0045. Tables

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-09 (A7)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0034](0034-frontend-stack.md), [0041](0041-content-models.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0011](0011-logs.md), [0013](0013-postgres-storage.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0026](0026-sitelinks.md), [0030](0030-edit-filters.md), [0031](0031-property-constraints.md), [0044](0044-tenant-relative-ids.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [15](../architecture/15-structured-pages.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

The statement UI ([0003](0003-statement-ui.md)) edits one entity at a time. Its table shape lays out many values of one property on one entity, with qualifiers as columns. Curation work often needs the transposed view: many entities side by side, one property per column, so that a gap or an inconsistency shows up down a column and can be fixed in place. On Wikidata this is done outside the wiki, in tools such as Magnus Manske's Tabernacle (a grid over a set of items and properties, kept by nobody), Listeria (a bot that writes a read-only wikitext table from a SPARQL query) and OpenRefine.

Commons has a page type for tables, `Data:*.tab` (JsonConfig's `Tabular.JsonConfig` model in namespace 486), but it is the opposite arrangement: the values live in the page. A table there is a second copy of data, unconnected to any item.

James's direction, from the design discussion of 2026-09-30:

- **A table is a new namespace and content model.** It is a page that defines a set of entities, manually at first by an entity ID column, and the properties chosen as columns. The values shown are the entities' own, and edits made in the table are written back to the entities.
- **The definition is directly editable JSON.**
- **Saving is row by row**, which suits the browser better than an all-or-nothing save.
- **The namespace is called `Table`.**

## Decision

### 1. A table is a page that names entities and properties

*Current text: [15](../architecture/15-structured-pages.md) §2.1.*

### 2. The `Table` namespace (extends 0008 §2)

*Current text: [15](../architecture/15-structured-pages.md) §2.2.*

### 3. The `triplespace-table` content model (extends 0041 §3)

*Current text: [15](../architecture/15-structured-pages.md) §2.2.*

### 4. The definition

*Changed by A3, A5, A6.*

*Current text: [15](../architecture/15-structured-pages.md) §2.3.*

### 5. What a cell shows

*Current text: [15](../architecture/15-structured-pages.md) §2.4.*

### 6. Editing cells: one row, one change set

*Current text: [15](../architecture/15-structured-pages.md) §2.5.*

### 7. Adding and removing rows

*Current text: [15](../architecture/15-structured-pages.md) §2.6.*

### 8. Links, feeds and backlinks

*Changed by A4.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.2, §4.3.*

### 9. Action API (uses 0041 §8)

*Current text: [18](../architecture/18-api.md) §2.2, §2.3.*

### 10. REST (extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §3.2, §4.*

### 11. UI (extends 0034 §4)

*Current text: [19](../architecture/19-site-ui.md) §6.7.*

### 12. Storage, caches and search

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.7, §9.7, §11.1.*

### 13. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **Many entities can be curated at once without a second copy of their data.** A table can't drift from its entities, and edits made elsewhere show in it.
- **Mixed sources in one grid.** Local, mirrored and other tenants' entities sit in the same table; a correction to a mirrored value is the same local `override` as on the entity page.
- **Two histories, by design.** Who changed the table's shape is in the table's history; who changed a value is in the entity's, tagged with the table.
- **A partial save is normal.** A row refused by a filter, a conflict or a permission does not hold up the others. The cost is that a table save is not atomic; a set of rows that must change together belongs in a bulk job ([0002](0002-source-graphs-and-mass-ingest.md) §8.5).
- **Creating entities from a table is two writes**, and can leave an entity outside its table.
- **The 210–219 block is full.**

## Open questions

- **Q1.** ~~**Query scopes.** Further kinds of `rows`, in rough order of cost: every entity with a given statement (`P31` = `Q5`), served from the statement indexes; the members of a category ([0038](0038-page-metadata-and-categories.md) §3); the entities in a column of another table; a restricted query language. All under `tables.max_rows`. A query scope also removes §7's two-write problem: an entity created with the right statements appears in the table by itself.~~ *Settled by [0060](0060-scopes.md) §6: `rows` takes every scope kind and a named scope; the two-write problem is gone for any kind but `ids`.*
- **Q2. More column kinds:** a qualifier of a property's statements; a rank filter; references; a computed column (a count, a constraint status).
- **Q3. Values as of a time.** `revid` gives the definition as of a revision but current values. Showing a table's values as they were needs entity states as of a time.
- **Q4.** ~~**Watching a table's rows,** as a watch option that expands to them, if related changes proves not to be enough.~~ *Settled by [0060](0060-scopes.md) §7: a `rows` watch kind expands the table's rows as a scope.*
- **Q5.** ~~**Tables in wikitext and Lua.** Transcluding `{{Table:Journals}}` into a page, or reading a table from a module, with usage tracking through the render manifest ([0042](0042-template-expansion-and-parsoid.md) §10).~~ *Settled by [0062](0062-workspaces.md) §3: the `#table` block and `mw.ext.triplespace.table`, tracked by the scoped-block manifest entry.*
- **Q6.** ~~**`M` IDs as rows.** File pages' statements ([0041](0041-content-models.md) §7) would make tables useful for Commons-style curation; term columns would be read-only until captions are settled.~~ *Settled by [0065](0065-mediainfo-captions-and-commons.md) §3: `M` and `WDM` rows, with captions in term columns and a `thumb` column.*
- **Q7. A faster lane for people.** Whether a large paste by a person should count against a class other than `edit`, or whether rate limits as they stand are the right brake.
- **Q8.** ~~**The next namespace block** for Triplespace-specific namespaces, now that 210–219 is full.~~ *Settled by the numbering policy of 0008 A17: 310–319.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §13 | extends | 0005 A45 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §2 | extends | 0008 A15 |
| [0012](0012-api-requirements.md) §5 | §10 | extends | 0012 A28 |
| [0034](0034-frontend-stack.md) §4 | §11 | extends | 0034 A3 |
| [0041](0041-content-models.md) §3 | §3 | extends | 0041 A3 |

## References

- [Extension:JsonConfig/Tabular](https://www.mediawiki.org/wiki/Extension:JsonConfig/Tabular) and [Help:Tabular data](https://www.mediawiki.org/wiki/Help:Tabular_data) (Commons `Data:*.tab`)
- Tabernacle and Listeria, Wikidata tools on Toolforge (prior art; not dependencies)
- [Codex Table](https://doc.wikimedia.org/codex/latest/components/demos/table.html), including its CSS-only form

## Amendment log

### A1. Crate table

- **Date:** 2026-09-30
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §13
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A45).

Replaced text (§13):

> No new crate.
>
> | Crate | Change |
> |---|---|
> | `scatter-wikibase-shape` | The `triplespace-table` model: the definition types, schema validation, canonical serialization, the cell rule of §5 (best values, fold count). Builds for `wasm32`, so the grid editor validates definitions and predicts cells in the browser. Gains a direct dependency on `scatter-pages` for the content-model trait, which it already had through `scatter-wikibase-model` |
> | `scatter-pages` | The model's registry entry |
> | `triplespace-titles` | Namespaces 218 and 219, with the `/doc` rule |
> | `triplespace-projections` | `page_link` rows from table definitions |
> | `triplespace-api-action`, `triplespace-api-rest` | The behaviour of §9, the routes of §10, and the mapping from cell edits to change-set operations (§6) |

### A2. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §13
- **Summary:** A1 was folded into the Decision. The open questions were numbered. No decision changed. Before this, A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A3. Scopes

- **Date:** 2026-10-04
- **Source:** [0060](0060-scopes.md) §6
- **Change:** amends §4
- **Summary:** `rows` accepts every scope kind of 0060 §4 and a named scope; page-subject scopes give page rows. Q1 and Q4 are settled.

Replaced text (§4, the `rows` row):

> | `rows` | Yes | The scope: an object holding exactly one scope kind. Version 1 has one kind, **`ids`**: a list of entity IDs in any form the tenant accepts (local, foreign, another tenant's, keyed). An object rather than a bare list so that query scopes (Open questions) can be added without changing the shape |

### A4. Transclusion

- **Date:** 2026-10-05
- **Source:** [0062](0062-workspaces.md) §3
- **Change:** extends §8
- **Summary:** Tables transclude through the `#table` block and read from Lua; Q5 is settled.

### A5. MediaInfo and lexeme rows

- **Date:** 2026-10-05
- **Source:** [0065](0065-mediainfo-captions-and-commons.md) §3; [0066](0066-lexemes.md) §9
- **Change:** extends §4
- **Summary:** `M`, `WDM`, `L` and part IDs as rows; caption and lexeme term columns; Q6 settled.

### A6. The `thumbnail` column

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §4
- **Summary:** §4's columns table gains a `thumbnail` field, with a `width` key, showing the row's file at that width ([0065](0065-mediainfo-captions-and-commons.md) §3), which is the column A5 called `thumb`; `render` is struck from 0065 §3. (PENDING F10)

### A7. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§13
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [15](../architecture/15-structured-pages.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
