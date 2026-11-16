# 0106. Special:CreateEntities: a grid over the table, and drafts

- **Status:** Proposed
- **Date:** 2026-11-08
- **Updated:** 2026-11-16 (A11)
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

*Changed by A2, A6, A9, A11.*

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

*Changed by A1, A6, A7, A9, A10, A11.*

*Current text: [21](../architecture/21-special-pages.md) §6.7.*

### 7. Upload and column mapping (extends 0104 §11)

*Changed by A6, A9, A10.*

*Current text: [21](../architecture/21-special-pages.md) §6.8.*

### 8. One batch; a statement's own reference replaces the default (amends 0104 §11)

*Changed by A4.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.5; [21](../architecture/21-special-pages.md) §6.6.*

### 9. Drafts are tables with staged rows (amends 0045 §1; extends 0045 §4 and §7)

*Changed by A1, A2, A3, A5, A9, A10, A11.*

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

- **Q1. Conflicting terms on a matched row.** *Settled by A1.*
- **Q2. Drafts' place in the namespace.** *Settled by A1.*
- **Q3. Datatype proposals.** *Settled by A1.*
- **Q4. Transient drafts after import.** *Settled by A3.*

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

### A9. One button, no text switch, every run a Table page, and files and QuickStatements on pages of their own

- **Date:** 2026-11-15
- **Source:** Direct: James, 2026-11-15, after using the page: "When I use Special:CreateManyEntities I don't end up with a saved Table namespace page, not even a draft one." "You don't need a button to switch between table mode and text mode in Special:CreateManyEntities. I would also move 'Add row' to the bottom and 'Add column' as a table-height button with a plus icon to the right of the rightmost column." "Get rid of the check button at the top of the form; it makes the page look like it's two separate forms and it doesn't queue the table up for edits anyway. You can move its behavior to the bottom button, where clicking it once validates the rows and clicking it a second time submits. The current interstitial page can be kept as a no-JS fallback." "Creating entities by file should be its own special page, which directs people to either CreateManyEntities or QuickStatements depending on format. Or QuickStatements is just a shim on top of CreateManyEntities. But having file upload on Special:CreateManyEntities right now clutters the interface."
- **Change:** amends §1, §6 and §7; extends §9
- **Summary:** (1) Every import saves the table as a Table page before anything is written: the draft it is kept in, or a new one where it is not. Until now only a table already saved as a draft was recorded. (2) The grid stands in for the text box, and nothing switches between the two; the text stays the canonical form, and is what the page without JavaScript shows. The button that adds a row stands beneath the last row, and the one that adds a column to the right of the last column, as tall as the table, under a plus sign. (3) The grid has no button of its own that checks the rows. The form's button checks them on its first press and writes them on its second; the preview page with its confirmation stays as what the page shows without JavaScript. This amends A6 (1), under which the grid posted nothing itself. (4) A file is read on a page of its own, which passes a table on to `Special:CreateManyEntities` and QuickStatements commands on to `Special:QuickStatements`; `Special:CreateManyEntities` has no file field. `Special:QuickStatements` is a reader in front of `Special:CreateManyEntities`, of the two forms James offered.

Replaced text: [21](../architecture/21-special-pages.md) §6.6.

Replaced text: [21](../architecture/21-special-pages.md) §6.7, §6.8.

### A10. What A9 needed, found while building

- **Date:** 2026-11-15
- **Source:** Direct: Claude, while building A9, 2026-11-15; confirmed by James, 2026-11-15
- **Change:** extends §6, §7 and §9
- **Summary:** (1) The file's page is `Special:CreateEntitiesFromFile`; with a table's title after it, it fills that draft, and a saved mapping (A6 (5)) applies there. It passes a table on by posting its text to `Special:CreateManyEntities` with `wpStep=open`, which shows the form holding it and checks and writes nothing. Without JavaScript the file is uploaded to the page, which tells commands from a table by the first line that says anything. (2) `Special:QuickStatements` reads `CREATE` and `LAST`, and also a command on an item's ID, as a row that adds to that item; on such a row a label or description is written with `!`, since the command sets the term whatever it was. Sources become the statement's own reference. A removal, a sitelink, `MERGE` and a target that is not an item are refused by line, and nothing opens until every line reads; qualifiers are left out and counted, as A6 (4) left them out of a file. A quantity's unit is read (`12U11573`). (3) A table saved by an import with no name given is named from the time, and takes a number after the name when a page already has it. A name the editor gave that a page already has is refused before anything is written. (4) The form's button writes only while nothing has changed since the check, anything typed beside the grid included; otherwise it checks again. (5) The statements on every item and the reference on every statement are lists beside the grid over their text boxes, a property and its value on each line, the reference's value given or read from a plain column: a step toward the locked columns and the reference block of §6, which are not built.

### A11. Keeping a table, runs past 500 rows, locked columns and reference chips, found while building

- **Date:** 2026-11-16
- **Source:** Direct: Claude, while building the kept table, the runs of a larger table, the locked columns and the reference chips (SCA-333, SCA-334), 2026-11-16; unconfirmed
- **Change:** extends §1 and §9; amends §6
- **Summary:** What §6, §9 and A2 decided and the building had to settle. (1) **Keeping a table** is a tick beside the table's name. A table not yet saved is made as `Table:{title}`; a draft already saved is moved there before anything else, and the move leaves a redirect, since leaving none needs a right most editors lack. A name is required, and a table of its own is not asked again. (2) **Runs.** A table is checked whole, up to 5000 rows, the default of `tables.max_rows`. One run writes 500 rows. The first run writes what the confirmation carries; each later run reads the saved table, checks again the staged rows that no run has written or refused, and writes the next 500. The page sends the next run itself with JavaScript; without it the report carries a button. The batch is closed by the last run. (3) A row the check finds a problem in is recorded in the table as refused, with what was found, where it used to stay staged without a reason; such a row waits for no run. A property a run created becomes a column of that property in the table; before, importing the rest of a draft would have tried to create it again. (4) An import ends when a run writes and refuses nothing, so that a table whose rest cannot be written does not run for ever. (5) **Locked columns.** A statement on every item stands as a locked column at the grid's right edge. Unlocking a row's cell writes it into the table with the shared value; from then on the table has a column of the property, whose empty cells show the shared statement, locked, and emptying a cell locks it again. (6) **Reference chips.** A statement's own reference is opened from a chip on its cell and edited as a list; its `P…/ref:P…` columns are not columns of the grid, and **+ Add column** no longer offers one. (7) The statements on every item and the reference on every statement are still edited as the lists of A10 (5), not in the statement UI's group shape or with a shared-reference component: neither exists as a component the grid could use. A draft larger than `tables.max_rows` still waits on the queued batch.

Replaced text: [21](../architecture/21-special-pages.md) §6.7.

