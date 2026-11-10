# 0106. Special:CreateEntities: a grid over the table, and drafts

- **Status:** Proposed
- **Date:** 2026-11-08
- **Updated:** 2026-11-10 (A8)
- **Author:** James Hare / Claude Opus
- **Changes:** [0045](0045-table-content-model.md), [0104](0104-batches-and-the-job-runner.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0043](0043-lua-modules.md), [0064](0064-entityschema-and-validation.md), [0073](0073-lines-links-and-url-patterns.md)

## Context

By 2026-11-08 `Special:CreateEntities` ([0104](0104-batches-and-the-job-runner.md) §11, A3) created many items from one table: a pasted or uploaded CSV or TSV whose header row named terms and properties, statements every row shared given beside it, a preview, and one keyed batch with its revert. The table was a text box. That worked without JavaScript and passed well between people, but 400 rows could not be read or corrected there, nothing showed which rows would collide with existing items, and an uploaded file could not be seen before it was imported. 0104 A3 had left out match keys, references from one row to another, updating existing items and creating properties.

Tables ([0045](0045-table-content-model.md)) were pages whose rows were existing entities and which stored no values.

### Direction

James's direction, from the design discussion of 2026-11-08:

- "I want you to imagine a more interactive interface where each row represents an item. There are the standard columns of label, description, aliases. Those are in the user's language. The columns can be labels/descriptions/aliases in other languages, or other properties, including properties created through the interface. (These properties would just have a label.) You can define statements that apply to all the items, and you can define the default citation that applies to the statements. When you select a data file to upload, you use this table interface to visualize the data (and modify if you want) before importing."
- "Yes, the staging table is a draft. Agreed with the grid being an enhancement on top of the textarea format."
- "The ability to use Special:CreateEntities to create new properties is limited to those who can create properties, but the special page is not otherwise restricted to only them."
- "For this interface, reference override means replace. No qualifiers for now."
- "Drafts should be stored as Tables. Row that matches an existing item: add its statements to that item. Choosing a class should add columns from its EntitySchema. Lua column transforms should only be in this version if Lua support generally is; otherwise defer."

## Decision

### 1. The grid is an enhancement over the table's text (amends 0104 §11)

*Changed by A2, A6.*

*Current text: [21](../architecture/21-special-pages.md) §6.6, §6.7.*

### 2. The text names ids, new properties, references and other rows (amends 0104 §11)

*Changed by A1, A4, A7, A8.*

*Current text: [21](../architecture/21-special-pages.md) §6.6.*

### 3. Columns from a class's EntitySchemas (uses 0064 §5)

*Current text: [21](../architecture/21-special-pages.md) §6.7.*

### 4. New properties from a column, for holders of `property-create` (amends 0104 §11)

*Changed by A1, A4.*

*Current text: [21](../architecture/21-special-pages.md) §6.6.*

### 5. A row that matches an existing item adds to it (amends 0104 §11; uses 0002 §8.5)

*Changed by A1, A4.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.5; [21](../architecture/21-special-pages.md) §6.6.*

### 6. Row status and cells by data type (uses 0003 §9 and 0073)

*Changed by A1, A6, A7.*

*Current text: [21](../architecture/21-special-pages.md) §6.7.*

### 7. Upload and column mapping (extends 0104 §11)

*Changed by A6.*

*Current text: [21](../architecture/21-special-pages.md) §6.8.*

### 8. One batch; a statement's own reference replaces the default (amends 0104 §11)

*Changed by A4.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.5; [21](../architecture/21-special-pages.md) §6.6.*

### 9. Drafts are tables with staged rows (amends 0045 §1; extends 0045 §4 and §7)

*Changed by A1, A2, A3, A5.*

*Current text: [15](../architecture/15-structured-pages.md) §2.1, §2.3, §2.6, §2.7.*

### 10. Not taken yet: qualifiers, and Lua column transforms until Lua is built (uses 0043)

*Current text: [21](../architecture/21-special-pages.md) §6.6.*

## Alternatives considered

- **The grid as the primary form, with the text box as a lesser fallback.** Anything the grid could do and the text could not would make the no-JavaScript path second-class and stop a table being passed around as text. James chose the text as canonical.
- **Drafts in browser storage.** They would vanish with the browser, could not be shared or reviewed, and would force a large file to load whole. A dedicated draft object was the other server-side option; a Table page already has history, diffs, protection and a grid, and after import it is a useful table of the batch's items.
- **A staged reference added beside the default.** Two references on every overridden statement, one of them often wrong for it; James chose replace.
- **Matched rows created anyway, or skipped by default.** Creating would duplicate items, and Wikibase refuses a second item with the same label and description; skipping would make re-running an export useless. James chose to add the row's statements to the item.

## Consequences

- **The text format grows.** `id`, `new:`, `P…/ref:`, `row:n` and `|`-separated values must parse and serialize, and the grid's edits must round-trip through them; a test should hold every grid operation to that.
- **Re-runs are idempotent where a match key exists.** A monthly export run twice adds nothing the second time.
- **A table can now hold values.** Staged rows are the exception to 0045's rule; they are never shown as an entity's.
- **Import is not atomic across a draft.** As 0045 §7 says for tables, written entities and the definition edit that records them are two writes.
- **Schema-seeded columns depend on scopes.** A class gets schema columns only where a tenant has written the `statement` scope that binds them.

## Open questions

- **Q1.** ~~**Conflicting terms on a matched row.** §5 leaves a differing label or description unwritten and marks it. Whether it should instead become an alias, or offer to overwrite, once real imports show which is wanted.~~ *Settled by A1: the term is flagged and the row waits until the editor says whether to keep the item's term, add the row's as an alias, or replace it.*
- **Q2.** ~~**Drafts' place in the namespace.** Whether drafts should sit under a conventional prefix such as `Table:Drafts/{user}/…`, or anywhere in `Table`.~~ *Settled by A1: new drafts are created as `Table:Drafts/{user}/{name}`.*
- **Q3.** ~~**Datatype proposals.** Which data types the proposal of §4 can infer beyond times, item IDs and identifier patterns, such as quantities with units or URLs.~~ *Settled by A1: those three to start; any other data type is chosen by hand.*
- **Q4.** ~~**Transient drafts after import.** Whether a draft under `Table:Drafts/` that was never given a title should be deleted, or left for its creator, once all its rows are imported.~~ *Settled by A3: it stays, as the interface for further edits on the terms it set up; administrators may delete it, which destroys no data.*

## References

- SCA-277, SCA-281: the table and the CSV upload this builds on; SCA-307: this ADR's ticket.
- SCA-173, SCA-225: the batch path the import runs through. SCA-205 (transforms), SCA-206 (a form from a schema), SCA-210 (identifier URL stripping).
- [Wikidata:QuickStatements](https://www.wikidata.org/wiki/Help:QuickStatements); [OpenRefine reconciliation](https://openrefine.org/docs/manual/reconciling)

## Amendment log

### A1. Term conflicts, the drafts' prefix and datatype proposals

- **Date:** 2026-11-08
- **Source:** Direct: James, design discussion of 2026-11-08
- **Change:** extends §2, §4, §5, §6 and §9
- **Summary:** (1) On a matched row, a label or description that differs from the item's is flagged, and the row is not written until the editor clarifies the intent: keep the item's term (the cell is cleared), add the row's as an alias (it moves to that language's `aliases`), or replace the item's term (the cell's value is prefixed with `!`). Row status gains *term conflict (n)*. (2) `Special:CreateEntities` creates drafts as `Table:Drafts/{user}/{name}`; the prefix is a convention, not a requirement. (3) A new property's data type is proposed only when a column's values are all times, all item IDs, or all match one identifier property's pattern; otherwise it is chosen by hand. Q1, Q2 and Q3 are settled.

### A2. Kept tables and transient drafts

- **Date:** 2026-11-08
- **Source:** Direct: James, design discussion of 2026-11-08
- **Change:** extends §1 and §9
- **Summary:** If the editor means the table to persist as a table, they give it a title and it becomes a permanent page in the `Table` namespace, `Table:{title}`; the draft is moved there with its history, before or after import. If the table only stages edits and is transient, it stays under A1's convention, `Table:Drafts/{user}/{name}`.

### A3. A finished draft stays as an interface

- **Date:** 2026-11-08
- **Source:** Direct: James, design discussion of 2026-11-08
- **Change:** extends §9
- **Summary:** Once a draft is finalized it remains, as a view or interface for making further edits to its items according to the parameters set up on the page: its columns, its default reference, and its statements on every item, which a row added later as a new entity carries. This applies to transient drafts as well as kept tables. Wiki administrators may delete one if they want; that destroys none of the underlying data. Q4 is settled.

### A4. The text's forms for creating, skipping and replacing, found while building

- **Date:** 2026-11-10
- **Source:** Direct: Claude, while building the table's text for `Special:CreateEntities` (SCA-307), 2026-11-10; confirmed by James, 2026-11-10
- **Change:** extends §2, §4, §5 and §8
- **Summary:** Four forms the text needed and the decision did not give. (1) The `id` cell takes `new`, which creates an item whatever the row matches, and `skip`, which leaves the row out, beside an item ID or nothing: §5 let an ambiguous row be "told to create" and any matched row be skipped, and both choices must hold in the text. (2) `!` before a label or description means "replace the item's term" only on a row that adds to an item; on a row that creates one it is part of the term, so a label that begins with `!` can be written. (3) A `{property}/ref:{property}` cell on a row with no statement of the first property is a problem of the row, since the reference would otherwise be dropped without a word. (4) Without JavaScript, the preview writes a proposed data type into the `new:` header and starts nothing; checking the rows again with the header as written is the confirmation §4 requires.

### A5. What a draft needed to hold the table's text, found while building

- **Date:** 2026-11-10
- **Source:** Direct: Claude, while building drafts for `Special:CreateEntities` (SCA-307), 2026-11-10; unconfirmed
- **Change:** extends §9
- **Summary:** Four things the definition needed so that a draft holds everything the table's text says. (1) A `reference` column without `of` holds a value the default reference reads through a snak's `"column"`: §9 let a snak name a column but gave no column kind for a plain column such as `source url`, and extending `reference` avoids a new kind. Its `title` is the header the text gives it. (2) A staged row may carry `"create": true`, the definition's form of the `id` cell's `new` (A4). (3) A column of the text that nothing reads is not kept when a draft is saved, since the definition has no column for it and the import ignores it. (4) When a row is written, a staged cell that named it with `row:n` is rewritten to name its item, so that the rows left staged still resolve; and the text of a draft numbers its staged rows from 1 in order, whatever their `n`.

### A6. How the grid and the mapping step are built, found while building

- **Date:** 2026-11-10
- **Source:** Direct: Claude, while building the grid and the mapping step (SCA-329, SCA-330), 2026-11-10; unconfirmed
- **Change:** extends §1, §6 and §7
- **Summary:** (1) The grid writes every change into the form's text box and posts nothing itself: checking, saving a draft and importing stay the form's. To show each row's status it asks the page for its own check as JSON, `wpFormat=json` on the form's post, which answers the notices, each row's status and problems, each column's kind, label and data type, the labels of the items the cells name, and the text as the check wrote it back. Statuses are refreshed when the editor asks, not on every keystroke, because the check searches for matches row by row. (2) The statements on every item and the reference on every statement stay text boxes beside the grid; a column the reference reads is added from **Add column**, which also writes the reference's line. (3) An item cell is looked up from a search above the grid for the cell that has the focus, and shows its items' labels beneath it. (4) A chosen file is read in the browser and never uploaded: CSV, TSV, JSON Lines, QuickStatements `CREATE` and `LAST` lines, and the first sheet of an XLSX file, read without a library. (5) A saved mapping applies when a draft is open with columns and no rows and the file has as many columns as the draft, the `id` column apart.

### A7. A quantity's unit in the text, found while building

- **Date:** 2026-11-10
- **Source:** Direct: Claude, while building the unit picker James asked for (SCA-329), 2026-11-10; unconfirmed
- **Change:** extends §2 and §6
- **Summary:** §6 gave quantities a unit picker, and the text had no form for a unit. A quantity is written as its amount, a space and its unit's item ID, `12 Q11573`; without a unit it is the number alone, as before. The unit is stored under the site's concept base, as the statement editor stores it. The picker is a search beside the grid for the focused quantity cell, on `Special:CreateManyEntities` and in a Table page's editor, and writes the ID after the amount; the cell shows the unit's label beneath it, and a Table page shows `12 page`. A quantity with bounds still has no plain form and is edited on the entity's page.

### A8. A literal backslash in a cell of several values

- **Date:** 2026-11-10
- **Source:** Direct: Claude, while fixing a code-scanning finding on PR #198, 2026-11-10; unconfirmed
- **Change:** extends §2
- **Summary:** In a cell that may hold several values, `\\` is a literal backslash, beside `\|` for a literal bar; a backslash before any other character stands for itself, so `C:\dir` is read as written. Without it a value ending in a backslash could not be written before another value: its backslash and the separator read as an escaped bar, and the two values became one. What writes such cells (the preview, a draft, the grid, the file readers) escapes both characters. A value typed by hand with two backslashes in a row, such as a UNC path or a line break in LaTeX, now reads as one and must be written with four.
