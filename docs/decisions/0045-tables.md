# 0045. Tables

- **Status:** Proposed
- **Date:** 2026-09-30
- **Author:** James Hare / Claude Opus
- **Related:** [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md), [0003 — Statement UI](0003-statement-ui.md), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§13 amends §2), [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md) (§2 extends §2: the `Table` namespaces), [0011 — Logs](0011-logs.md), [0012 — API requirements for the site UI](0012-api-requirements.md) (§10 extends §5: the table routes), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md), [0016 — Permissions and access control](0016-permissions-and-access-control.md), [0020 — Change feeds](0020-change-feeds.md), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md), [0026 — Sitelinks are URLs](0026-sitelinks.md), [0030 — Edit filters](0030-edit-filters.md), [0031 — Property constraints](0031-property-constraints.md), [0034 — Frontend stack](0034-frontend-stack.md) (§11 extends §4: the table grid editor), [0041 — Content models](0041-content-models.md) (§3 extends §3: the `triplespace-table` model), [0044 — Tenant-relative IDs](0044-tenant-relative-ids.md), [MediaWiki API contract](../api/mediawiki-compat.md)

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

**A table is a page in the `Table` namespace (§2) whose content is a definition** (§4): which entities are its rows and which fields of those entities are its columns. **It stores no values.** Every cell is read from the row entity's resolved view ([0002](0002-source-graphs-and-mass-ingest.md) §3, [0004](0004-identity-clusters-and-equivalence.md) §4) when the table is shown, and every cell edit is a change set on that entity (§6).

So a table has two kinds of change and two histories:

| Change | Written as | History it appears in |
|---|---|---|
| Rows or columns added, removed or reordered; the default sort or reference changed | A page revision of the table, in `pages` ([0008](0008-namespaces-and-document-pages.md) §4) | The table's |
| A value changed in a cell | A change set on the row's entity, in `local` | The entity's, tagged with the table (§6) |

Nothing is copied, so a table can never disagree with its entities, and an edit made on the entity page shows in every table that includes it.

### 2. The `Table` namespace (extends 0008 §2)

| Number | Canonical name | Kind | Allowed models | Default model |
|---|---|---|---|---|
| 218 | `Table` | `pages` | `triplespace-table`, `wikitext` | `triplespace-table`; `wikitext` for titles ending `/doc` |
| 219 | `Table talk` | `pages` | `triplespace-talk` | `triplespace-talk` |

- The `/doc` rule follows Module's ([0043](0043-lua-modules.md) §2): `Table:Journals/doc` is the table's documentation, and is shown above the grid. Both models are text models, so the namespace satisfies [0041](0041-content-models.md) §4's one-source rule, and `changecontentmodel` may move a page between them as it may in Module.
- Titles normalize first-letter, as in Project; subpages are on.
- **218–219 was the last free pair in 210–219.** The next Triplespace-specific namespace needs a new block (Open questions). The mediawiki.org registration of 210–229 is updated to include them.

### 3. The `triplespace-table` content model (extends 0041 §3)

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `triplespace-table` | Triplespace | text | main | `application/json` | Yes | 218 |

It is a **text** model ([0041](0041-content-models.md) §5): the definition is stored as text in the page's `page` records, revised and diffed like any text page, and `action=edit` can change it. Only the grid is generated. No new source kind is needed.

| Operation (0041 §5) | For `triplespace-table` |
|---|---|
| Validate | Against the definition schema (§4). Content that fails is refused, as 0008 §5 refuses invalid `json` |
| Pre-save transform | The definition is canonicalized: IDs to their stored form ([0044](0044-tenant-relative-ids.md) §1: tenant-relative input is never stored), keys in schema order, two-space indentation, **one row ID per line**. As MediaWiki's JSON content model pretty-prints on save, so that diffs and three-way merges work line by line |
| Serialize | The canonical JSON |
| Plain text for search | Title, `description`, and the column headers in the content language |
| Diff | Line diff of the canonical JSON: one line per added or removed row |
| Render | The grid (§5) |

### 4. The definition

```json
{
  "version": 1,
  "description": "Journals cited in the 2026 citation sample",
  "rows": {
    "ids": [
      "Q5",
      "WDQ42",
      "LBQ6"
    ]
  },
  "columns": [
    { "field": "label", "language": "en" },
    { "field": "statements", "property": "P50" },
    { "field": "statements", "property": "WDP577", "title": "Published" },
    { "field": "sitelink", "site": "en.wikipedia.org" }
  ],
  "default_reference": {
    "snaks": {
      "P248": [ { "snaktype": "value", "property": "P248",
                  "datavalue": { "type": "wikibase-entityid", "value": { "id": "Q1234" } } } ]
    }
  },
  "sort": [ { "column": 2, "order": "descending" } ]
}
```

| Key | Required | Meaning |
|---|---|---|
| `version` | Yes | `1`. A later scope or column kind that old readers cannot ignore raises it |
| `description` | No | Plain text, shown under the title and indexed for search |
| `rows` | Yes | The scope: an object holding exactly one scope kind. Version 1 has one kind, **`ids`**: a list of entity IDs in any form the tenant accepts (local, foreign, another tenant's, keyed). An object rather than a bare list so that query scopes (Open questions) can be added without changing the shape |
| `columns` | Yes | An ordered list of one or more columns (below) |
| `default_reference` | No | One reference in Wikibase's canonical JSON, without its hash. New statements written from the grid carry it unless the editor unchecks it (§6) |
| `sort` | No | The default order: a list of column positions (zero-based) and `ascending` or `descending`. Without it, rows appear in the order listed. A viewer can re-sort without saving |

**Columns.** Each column has a `field`, the keys that field needs, and an optional `title` that overrides the header:

| `field` | Keys | Shows |
|---|---|---|
| `label`, `description`, `aliases` | `language` | The term in that language |
| `statements` | `property` | The property's best values (§5) |
| `sitelink` | `site` | The page linked on that site. A host or a site alias ([0026](0026-sitelinks.md) §2); aliases are canonicalized to the host on save |

**What validation checks, and what it does not.** The schema, the syntax of every ID, no unknown keys, no duplicate row IDs (after canonicalization), and the limits: `tables.max_rows` (site setting, default 5,000, the same as [0020](0020-change-feeds.md) §2's `feeds.related_limit`, so a table's related changes are never truncated, §8) and `tables.max_columns` (default 50). **It does not check that entities or properties exist.** A row whose entity is missing is shown as missing, as a red link is: entities are deleted, merged and mirrored after the table is saved, so existence at save time would guarantee nothing.

**Rows keep the ID as written.** Two rows that later turn out to be members of one identity cluster stay two rows; the grid marks the second as a duplicate (§5). The definition is not rewritten when clusters change, as source graphs keep IDs as asserted ([0004](0004-identity-clusters-and-equivalence.md) §4).

### 5. What a cell shows

**A row is the resolved view of its entity.** A non-canonical cluster member is shown as its canonical entity, as the API resolves it ([0004](0004-identity-clusters-and-equivalence.md) §4), with a note in the ID cell ("shown as Q5"). A row whose canonical entity already appears higher in the table is marked as a duplicate. A missing or deleted entity, or one the viewer may not read ([0023](0023-moderation.md) §2), is a row with its ID and the state, and no cells.

**Statement cells follow 0003's rule: show the exception, not the rule.**

- The cell shows the property's **best values**: the preferred values if any, else the normal ones. Deprecated values are never shown inline.
- **One best value** is shown as 0003 §9's value cell, formatted by data type.
- **Several best values:** the first, then a fold, "+N", which opens the property's statement group (0003) in a popover, in whatever shape it takes there.
- **Only deprecated values:** an empty cell with a muted "N deprecated" fold.
- Qualifiers and references are not drawn in the cell. The popover shows them.
- A **constraint marker** ([0031](0031-property-constraints.md) §3) appears beside a violating value, and the **correction chip** ([0003](0003-statement-ui.md) §4) beside a mirrored value corrected here, as on the entity page.

**Term cells** show the term in the column's language. Where it is missing, the fallback is shown in italics with its language code, as Wikibase does, and editing that cell writes the column's language. **Sitelink cells** show the linked title.

**Read restrictions apply cell by cell.** A statement hidden by a `statement` ACL is absent for viewers outside the group, as it is from the resolved view. A column whose property is under a `property` read ACL the viewer does not satisfy shows its header as restricted and its cells empty ([0023](0023-moderation.md) §2).

### 6. Editing cells: one row, one change set

**Each row is saved on its own.** Edits accumulate in the grid by row; saving submits each changed row as **one change set on that row's entity**, an ordinary interactive edit with read-your-writes ([0013](0013-postgres-storage.md) §7) and the row's own base revision. Rows are submitted in order, and each succeeds or fails alone. There is no `atomic` batch and no job.

**What a cell edit becomes:**

| Cell edit | Change set operation |
|---|---|
| A value typed into an empty statement cell | `add` a statement with that main snak and, unless unchecked, the table's `default_reference` ([0002](0002-source-graphs-and-mass-ingest.md) §8.2, §8.5: an identical existing statement gets the reference merged onto it instead) |
| The one best value changed | The statement's main snak replaced, keeping its GUID, qualifiers and references, as Wikibase's `wbsetclaimvalue` does |
| The one best value cleared | `remove` of a local statement |
| A mirrored value changed or cleared | A local `override` or `add`, leaving the mirrored statement untouched: [0003](0003-statement-ui.md) §8's "Correct a mirrored value" |
| A term or sitelink changed or cleared | The term or sitelink operation, under the term rights ([0016](0016-permissions-and-access-control.md) §2) |

**A cell with several best values is not edited inline.** The editor opens the statement group in the popover and edits there, with 0003 §8's table editing. A grid cell cannot say which of several values a typed value replaces.

**Attribution.** Each change set carries the actor, an automatic summary naming the table ("Edited via [[Table:Journals]]") followed by the editor's own summary if given, and the change tag **`table:{page id}`** in its attestation part, a tag of the same kind as `job:{id}` ([0030](0030-edit-filters.md) §5). Edits made through a table can then be found by filter, feed or tag.

**Everything else is the ordinary write path**, applied per row:

- **Permissions** are the actor's on each entity: entity, namespace, statement and property ACLs and protection ([0023](0023-moderation.md) §2). Cells the viewer cannot edit are drawn read-only. **Protecting a table protects its definition only**; anyone who may edit an entity may edit its cells in any table.
- **Edit filters** ([0030](0030-edit-filters.md) §4) run on each row. `warn` shows the public message on that row, and re-saving it resubmits with the token; `disallow` marks the row refused with the message. The remaining rows carry on.
- **Rate limits** ([0024](0024-subsidiary-accounts.md) §5): each row is one `edit`, and a row that creates an entity one `create`. On a 429 the grid waits `Retry-After` and continues, showing how many rows are queued. There is no exemption: at `user`'s default of 90 a minute, a 500-row paste takes about six minutes. Volume beyond that is what subsidiaries and bulk jobs are for.
- **Conflicts.** If the entity changed since the grid loaded, the row's edits are applied to the current state and only the cells that clash are marked, as [0003](0003-statement-ui.md) §8 does for a group. Other rows are unaffected.
- **Patrolling** is as for any edit ([0023](0023-moderation.md) §6).

### 7. Adding and removing rows

**Rows are part of the definition.** Adding existing entities, by typing IDs, picking them with a lookup, or pasting a column of IDs, is a definition edit. Removing a row is a definition edit, and never touches the entity. The grid collects row additions and removals and saves them as **one page revision** per save, with the page's base revision; a conflicting definition edit is merged line by line, which the one-ID-per-line serialization (§3) makes reliable.

**A new row can create a new entity.** That is two writes in two partitions: the entity's `create`, carrying the row's cells as its first change set, in `local`; then the definition edit adding its ID, in `pages`. They are **not atomic**: [0011](0011-logs.md) §6.1 does not make writes atomic across partitions, and this ADR does not start. If the second write fails, the entity exists and is not in the table; the grid says so and offers to add it again, and the creation's summary and tag name the table, so the entity can be found.

### 8. Links, feeds and backlinks

**The links projection** ([0008](0008-namespaces-and-document-pages.md) §10, `view.page_link`) records a link from the table to each row entity and each column property, after resolution. That gives, with nothing new built:

- **"What links here"** on an entity or property lists the tables that show it, and `list=backlinks` returns them.
- **Related changes** from a table ([0020](0020-change-feeds.md) §2, *related, outward*) is the recent changes to its rows and its column properties: the table's data as a feed. `tables.max_rows` defaults to `feeds.related_limit`, so the feed covers every row.

**Watching a table watches its definition**, as watching any page does. Following its data is what related changes is for.

### 9. Action API (uses 0041 §8)

| Module | Behaviour |
|---|---|
| `meta=siteinfo` | Namespace 218 reports `defaultcontentmodel` `triplespace-table`; `siprop=triplespace` lists the model under `contentmodels`, and the limits `tables.max_rows` and `tables.max_columns` |
| `prop=revisions` | The definition as of each revision, `contentmodel` `triplespace-table`, `contentformat` `application/json` |
| `action=edit` | Accepts a definition. Invalid content is refused as MediaWiki refuses content its handler rejects, with the failing schema path in the message; the exact code is pinned by a contract test against MediaWiki 1.43's `json` model. The pre-save transform (§3) applies |
| `action=parse` | Renders the grid's first page as static HTML, without editing controls |
| `list=backlinks` | Includes tables, through the links of §8 |

Cell edits made with the Wikibase modules directly are ordinary entity edits: a bot can edit what a table shows without going through the table.

### 10. REST (extends 0012 §5)

| Route | Purpose |
|---|---|
| `GET /table/{pageid}/rows?offset=&limit=&sort=&lang=` | The rows, paged: for each, the ID as written and as shown, the row state (§5), and per column the best values in Wikibase JSON with the fold count, constraint flags and whether the viewer may edit the cell. `revid=` reads the definition as of that revision; the values are always current |
| `POST /table/{pageid}/rows/{id}` | One row's cell edits with `baserevid` and an optional summary. The server maps them to one change set (§6) and returns the new revision and the row as `GET` would. The grid's save path, and usable by bots |
| `GET /table/{pageid}/export?format=tsv\|csv` | The current values as one file: an ID column, then one column per table column, multiple best values joined by `; ` |

### 11. UI (extends 0034 §4)

- **Reading needs no JavaScript.** The server renders the grid with Codex's CSS-only table and 0003 §9's value cells ([0034](0034-frontend-stack.md) §3), a page of rows at a time, inside the frame of [0010](0010-site-ui.md) §2. Tabs: Table, Definition, Talk, History.
- **The grid editor** is a new interactive component (0034 §4): Vue with Codex's Table, Lookup and TextInput, reusing the statement value editor for cells. Keyboard as [0003](0003-statement-ui.md) §8, with arrow keys between cells. Paste: tab-separated text maps to the columns from the focused cell, each cell parsed with `wbparsevalue` for its column's data type; a pasted column of IDs into the ID column adds rows (§7). Pending rows are marked, and a save bar shows the rows to save, progress, and refused rows with their reasons (§6).
- **The Definition tab** is the source editor ([0034](0034-frontend-stack.md) §7) on the JSON, with schema validation in the browser.
- **Viewer state is not saved.** Sorting and filtering in the grid are the viewer's own; "Make this the default sort" is a definition edit.

### 12. Storage, caches and search

**No new tables.** The definition is a page record's text ([0008](0008-namespaces-and-document-pages.md) §4); the table has a `view.page` row with `content_model` `triplespace-table`; its links are `page_link` rows (§8). The grid is assembled from the entities' cached resolved views ([0014](0014-caches-and-search.md) §2) and is not cached as a whole page, since any row's edit would invalidate it. Search indexes the table as a page, with the text of §3.

### 13. Crates (amends 0005 §2)

No new crate.

| Crate | Change |
|---|---|
| `scatter-wikibase-shape` | The `triplespace-table` model: the definition types, schema validation, canonical serialization, the cell rule of §5 (best values, fold count). Builds for `wasm32`, so the grid editor validates definitions and predicts cells in the browser. Gains a direct dependency on `scatter-pages` for the content-model trait, which it already had through `scatter-wikibase-model` |
| `scatter-pages` | The model's registry entry |
| `triplespace-titles` | Namespaces 218 and 219, with the `/doc` rule |
| `triplespace-projections` | `page_link` rows from table definitions |
| `triplespace-api-action`, `triplespace-api-rest` | The behaviour of §9, the routes of §10, and the mapping from cell edits to change-set operations (§6) |

## Consequences

- **Many entities can be curated at once without a second copy of their data.** A table can't drift from its entities, and edits made elsewhere show in it.
- **Mixed sources in one grid.** Local, mirrored and other tenants' entities sit in the same table; a correction to a mirrored value is the same local `override` as on the entity page.
- **Two histories, by design.** Who changed the table's shape is in the table's history; who changed a value is in the entity's, tagged with the table.
- **A partial save is normal.** A row refused by a filter, a conflict or a permission does not hold up the others. The cost is that a table save is not atomic; a set of rows that must change together belongs in a bulk job ([0002](0002-source-graphs-and-mass-ingest.md) §8.5).
- **Creating entities from a table is two writes**, and can leave an entity outside its table.
- **The 210–219 block is full.**

## Open questions

- **Query scopes.** Further kinds of `rows`, in rough order of cost: every entity with a given statement (`P31` = `Q5`), served from the statement indexes; the members of a category ([0038](0038-page-metadata-and-categories.md) §3); the entities in a column of another table; a restricted query language. All under `tables.max_rows`. A query scope also removes §7's two-write problem: an entity created with the right statements appears in the table by itself.
- **More column kinds:** a qualifier of a property's statements; a rank filter; references; a computed column (a count, a constraint status).
- **Values as of a time.** `revid` gives the definition as of a revision but current values. Showing a table's values as they were needs entity states as of a time.
- **Watching a table's rows,** as a watch option that expands to them, if related changes proves not to be enough.
- **Tables in wikitext and Lua.** Transcluding `{{Table:Journals}}` into a page, or reading a table from a module, with usage tracking through the render manifest ([0042](0042-template-expansion-and-parsoid.md) §10).
- **`M` IDs as rows.** File pages' statements ([0041](0041-content-models.md) §7) would make tables useful for Commons-style curation; term columns would be read-only until captions are settled.
- **A faster lane for people.** Whether a large paste by a person should count against a class other than `edit`, or whether rate limits as they stand are the right brake.
- **The next namespace block** for Triplespace-specific namespaces, now that 210–219 is full.

## References

- [Extension:JsonConfig/Tabular](https://www.mediawiki.org/wiki/Extension:JsonConfig/Tabular) and [Help:Tabular data](https://www.mediawiki.org/wiki/Help:Tabular_data) (Commons `Data:*.tab`)
- Tabernacle and Listeria, Wikidata tools on Toolforge (prior art; not dependencies)
- [Codex Table](https://doc.wikimedia.org/codex/latest/components/demos/table.html), including its CSS-only form
