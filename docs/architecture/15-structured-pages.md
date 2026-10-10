# 15. Structured pages

This chapter describes the pages whose content is a definition the site evaluates rather than text it shows: scopes, whose members are a projection; tables, whose cells are read from entities; sprints, whose tasks are a projection and whose claims are records; workspaces, the project pages that declare scopes and transclude blocks, with the kit templates and kits that build them; query pages, saved parameterized SPARQL; and entity schemas, with the validation report that binds them to scopes. It assumes the namespace and content-model catalogues of [10](10-pages-and-content-models.md), the `view` schema and projection order of [03](03-storage-caches-and-search.md), the resolved view of [05](05-providers-and-ingest.md), statements and constraints from [06](06-statements-and-properties.md), the query service of [02](02-graphs-rdf-and-query.md), the expander and render manifest of [11](11-rendering-templates-and-modules.md), ACLs and protection from [09](09-security-and-moderation.md), and boards from [14](14-discussions.md). Feeds, watches and sprint notifications are [16](16-logs-feeds-and-notifications.md); the pages' API routes and Action API modules are [18](18-api.md); `Special:CreateSprint`, `Special:CreateWorkspace` and the EntitySchema special pages are [21](21-special-pages.md); the scope, table, sprint, query and schema pages as the site shows them are [19](19-site-ui.md).

## 1. Scopes

*Sources: [0060](../decisions/0060-scopes.md) §1, §2, §3, §4, §5, §6; [0049](../decisions/0049-boards.md) §14.*

### 1.1 A scope is a page that names a set of subjects

*Sources: [0060](../decisions/0060-scopes.md) §1.*

**A scope is a page in the `Scope` namespace (§1.2) whose content is a definition (§1.3) of a set of subjects**, entities or pages. The set itself is computed: **membership is a projection** (§1.4), `view.scope_member`, recomputed as the definition and the data change, and written nowhere else. Nothing is added to a member when it joins a scope, and nothing is removed when it leaves; a scope is what a WikiProject banner would be if the banner were never placed.

A scope is **content**, and is never a target of restriction. The sets of [0056](../decisions/0056-security-model.md) §3 ([09](09-security-and-moderation.md) §4.7) are the other thing: a grouping only `protect` may change, by ID, because it restricts reading. A scope is edited by whoever may edit the page, may name things by title and by query, and grants and withholds nothing. The two do not meet.

What references a scope: a table's rows (§2.3), a board's selection (§1.5), a feed and a watch ([16](16-logs-feeds-and-notifications.md) §4.2), and the sprints (§3) and workspaces (§4) built on it. Each names the scope by title and reads its members from the projection. Creating a scope needs `createpage` in namespace 312 and editing one `edit`; there is no right to compute ([09](09-security-and-moderation.md) §8.10).

### 1.2 Namespace and content model

*Sources: [0060](../decisions/0060-scopes.md) §2, §3.*

`Scope` is namespace 312, kind `pages`, with `Scope talk` at 313; the allowed models are `triplespace-scope` and `wikitext`, the default `triplespace-scope`, and `wikitext` for titles ending `/doc`. The rows are in the namespace catalogue, [10](10-pages-and-content-models.md) §1.4. It is the second pair of the 310–319 block, after Board. Titles normalize first-letter; subpages are on, and the `/doc` rule is Table's (§2.2). A scope has an ordinary talk page, unlike a board, because a scope is a subject to discuss.

`triplespace-scope` is a Triplespace **text** model ([0041](../decisions/0041-content-models.md) §5; [10](10-pages-and-content-models.md) §4.4): source text, main slot, format `application/json`, directly editable, default in 312. As `triplespace-table` is, the definition is the page's content, revised and diffed as text. The operations follow `triplespace-table`'s (§2.2) exactly, with these particulars: validation checks the schema, the syntax of every ID and title, the limits of §1.3, and the absence of cycles among referenced scopes (§1.3); the pre-save transform canonicalizes IDs to their stored form ([0044](../decisions/0044-tenant-relative-ids.md) §1), titles to their normalized form, keys to schema order, and one ID per line in `ids`, `include` and `exclude`; plain text for search is the title, the description and the titles and properties the definition names; render is the scope page ([19](19-site-ui.md)).

### 1.3 The definition

*Sources: [0060](../decisions/0060-scopes.md) §4; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

```json
{
  "version": 1,
  "description": "Women writers born in the 19th century",
  "members": {
    "intersection": [
      "Scope:Women",
      "Scope:Writers",
      { "statement": { "property": "WDP569", "range": { "from": "1801", "to": "1900" } } }
    ]
  },
  "include": [ "WDQ7259" ],
  "exclude": [ "WDQ1234" ]
}
```

| Key | Required | Meaning |
|---|---|---|
| `version` | Yes | `1`. A kind that old readers cannot ignore raises it |
| `description` | No | Plain text, shown under the title and indexed |
| `members` | Yes | An object holding exactly one **kind** (below), the same shape as a table's `rows` |
| `include` | No | Subject IDs that are members whatever `members` says |
| `exclude` | No | Subject IDs that are never members. Applied last |
| `schemas` | No | Entity schema IDs every member is validated against; the report is `view.schema_report` (§6.4) |

**Kinds.** A kind selects subjects. Each has a **subject type** — entity or page, and since [0066](../decisions/0066-lexemes.md) §5 also `form` and `sense` for lexeme parts, which `statement` selects when the property's statements sit on parts — which the scope takes from its kind; set algebra requires its operands to agree.

| Kind | Keys | Selects | Subject type | Computed by |
|---|---|---|---|---|
| `ids` | a list | The listed subjects. Entity IDs in any form the tenant accepts; pages by title | as listed | the definition |
| `statement` | `property`; one of `value`, `range {from, to}`, or none | Subjects with a best-rank statement of the property, with that value, in that range, or at all. Page subjects count ([0038](../decisions/0038-page-metadata-and-categories.md) §1) | entity, or page if `subjects: "pages"` | `view.statement_assertion`, incrementally |
| `category` | `title` | The pages in the category, direct members only | page | `view.page_category`, incrementally |
| `column` | `table`, `column` | The entities in a column of a table: its rows for the ID column, the best values for a statement column | entity | the table's resolved rows |
| `query` | `sparql` | The subjects a `SELECT` projecting `?item` returns | entity or page, by IRI | the query service ([0059](../decisions/0059-query-service.md) §5), on refresh |
| `pages_of` | `scope` or an inline kind | The articles paired with the entities of the operand ([0038](../decisions/0038-page-metadata-and-categories.md) §6) | page | the pairing index |
| `entities_of` | `scope` or an inline kind | The entities paired with the pages of the operand | entity | the pairing index |
| `union`, `intersection` | a list of operands | The union or intersection of the operands' members | the operands' | set algebra over the operands' member rows |
| `difference` | `of`, `minus` | The members of `of` that are not in `minus` | `of`'s | set algebra |
| `saved` | `query` (a `Query:` title), `params` | The subjects bound to `?item` in a saved query's result, with the parameters given (§5.5) | entity or page, by IRI | the query service, on refresh |
| `conforms` | `schema`, optional `not` | The subjects whose schema report says they conform, or with `not`, fail (§6.5) | the report's | `view.schema_report` |

An **operand** is a scope title (`"Scope:Women"`) or an inline kind object. Operands nest to `scopes.max_depth` (default 8, counting referenced scopes' own definitions). **Cycles are refused** at save: the validator walks every referenced scope's current definition, and a save that would make a scope a member of its own ancestry fails with `ts-scope-cycle`. A referenced scope that does not exist is allowed, as a missing row entity is in a table: the reference is shown red and contributes nothing until the scope exists. **An `intersection` or `difference` with a truncated operand that cannot be probed is refused** at save with `ts-scope-truncated-operand` ([0083](../decisions/0083-write-path-in-three-tiers.md) §3): set algebra over a truncated operand is correct only where the operand can answer "is this subject a member?" for any subject rather than list its members, which a `statement`, `category`, `ids` or `conforms` kind can and a `query`, `saved`, `column`, `pages_of` or `entities_of` kind cannot; a save whose non-probeable operand's current `view.scope` row is `truncated` is refused, and a scope that becomes truncated later makes the scopes that intersect it `stale` (§1.4) rather than silently wrong.

**Saving compiles the triggers.** A definition is compiled at save to the rows of `view.scope_trigger` (§1.4) that say which composition events can change its membership, so that a scope costs nothing on the write path beyond one hash probe per event.

`subjects: "pages"` on a `statement` kind selects pages by their page statements instead of entities by theirs; without it a `statement` kind selects entities. A `query` kind's `sparql` is checked as [0059](../decisions/0059-query-service.md) §5 says, at save, and refused with the compiler's reason; it is saved even while the query service is off, and the page says the scope is not computed (§1.4). A `query` kind can only ever select public subjects ([0059](../decisions/0059-query-service.md) §3).

**Limits.** `scopes.max_members` (site, default **100,000**): the bound on a scope's materialized members (§1.4). `scopes.max_depth` (default 8). `scopes.max_ids` (default 5,000, as `tables.max_rows`) on `ids`, `include` and `exclude` together. The keys are catalogued in [23](23-configuration-and-registry.md) §3.2.

### 1.4 Membership is a projection

*Sources: [0060](../decisions/0060-scopes.md) §5; [0083](../decisions/0083-write-path-in-three-tiers.md) §3, §6.*

```sql
CREATE TABLE view.scope (
  tenant        text    NOT NULL,
  page_id       bigint  NOT NULL,          -- the scope page
  count         integer NOT NULL,          -- members materialized
  truncated     boolean NOT NULL,          -- cut at scopes.max_members
  cursor_epoch  integer, cursor_seq bigint, -- the query service cursor, for query kinds
  computed_at   timestamptz,               -- NULL: never computed (service off)
  definition_revid bigint NOT NULL,
  PRIMARY KEY (tenant, page_id)
);
CREATE TABLE view.scope_member (
  tenant   text   NOT NULL,
  page_id  bigint NOT NULL,
  kind     text   NOT NULL,                -- 'entity' | 'page' | 'form' | 'sense'
  id       text   NOT NULL,                -- entity ID in stored form, a page subject as 03 §4.4 keys it, or a lexeme part's ID
  PRIMARY KEY (tenant, page_id, kind, id)
);
CREATE INDEX ON view.scope_member (tenant, kind, id);  -- "scopes this subject is in"

CREATE TABLE view.scope_trigger (                 -- compiled at save (0083 §3); held in L0
  tenant        text   NOT NULL,
  trigger_kind  text   NOT NULL,                  -- 'property' | 'category' | 'table-row' | 'sitelink' | 'schema'
  key           text   NOT NULL,                  -- 'P31', the category's page ID, the table's page ID, '' for sitelink, the schema ID
  scope_page_id bigint NOT NULL,
  PRIMARY KEY (tenant, trigger_kind, key, scope_page_id)
);
```

`kind` admits `form` and `sense` beside `entity` and `page`; a lexeme part is keyed by its part ID.

All three tables are in the `view` catalogue, [03](03-storage-caches-and-search.md) §4.15 and §5.

**Membership is a tier-3 consumer of composition** ([0083](../decisions/0083-write-path-in-three-tiers.md) §3). The scope projection consumes "subject X, tenant T, composed to version N" events in its own queue, with its own rate and lag, never inside the appending or composing transaction, and it is off during bootstrap and bulk modes until the operator turns it on. For each event it probes `view.scope_trigger`, held in L0, with the keys the event carries: `property:{P}` for every property whose statements the composed row gained, lost or changed, `category:{page}` for a page's category rows, `table-row:{page}` for a table whose rows changed, `sitelink` for a sitelink change and `schema:{id}` for a conformance change. **An event that matches no trigger is one hash probe and nothing more**, which is what the weekly re-sync of a Wikidata mirror mostly produces; an event that matches re-evaluates the matching scopes for that subject alone, through the inverted index above, and set algebra over them propagates one subject at a time. `ids`, `statement`, `category`, `column`, `pages_of`, `entities_of`, `conforms` and the set algebra over them are therefore **incremental**, current within the consumer's lag, which the scope page shows beside `computed_at`. `query` and `saved` kinds, and any set algebra with such an operand, are **refreshed**: a job runs the compiled query at `scopes.refresh` (default 15 min) and whenever the definition is saved or a person asks (`POST /scope/{pageid}/refresh`, [18](18-api.md) §3.2), and replaces the members in one transaction with the cursor the service reported. Which deltas affect an arbitrary query is not knowable, so these scopes are as fresh as the last refresh, and say so.

**The initial computation is always a job.** Saving a definition, or changing one, never computes membership in the save: the save compiles the triggers, writes `view.scope` with `computed_at` null, and enqueues the scope's computation as a job with progress, which materializes the members from the named `view` tables and sets `computed_at`; the page says "being computed" until it does. Only events after that point are applied incrementally. A rebuild of the scope projection is the same job run for every scope of a tenant from `view`, since the projection is view-derived ([0083](../decisions/0083-write-path-in-three-tiers.md) §6).

**The bound.** Members are materialized in a deterministic order, entity IDs then page IDs, each ascending, so that the prefix kept is stable between computations. **A truncated scope is "the first N found, refilled by a job when it falls below N."** When a computation yields more than `scopes.max_members`, the first `scopes.max_members` are kept and `truncated` is set; events then add nothing to a truncated scope and remove what they remove, and once removals have taken the count below `scopes.max_members`, a refill job, debounced with the refresh interval `scopes.refresh`, recomputes the scope from `view` and tops it up to the bound again. **The scope page, and every table, board, feed and watch that uses the scope, shows the notice**: "This scope has more than 100,000 members; the first 100,000 are shown." A truncated scope is a full operand only where it can be **probed**: `intersection` and `difference` test each candidate subject against a `statement`, `category`, `ids` or `conforms` operand directly rather than against its materialized prefix, so `Scope:Humans` is useless to list and fine to intersect with; where the truncated operand is a `query`, `saved`, `column`, `pages_of` or `entities_of` kind, which can only list, the save is refused (§1.3), and a scope that was saved before its operand became truncated is marked `stale` and recomputed when the operand is no longer truncated or the definition changes.

**What the projection does not do.** It writes no record, produces no activity row and sends no notification: joining or leaving a scope is not an event, as [0049](../decisions/0049-boards.md) §14 fixed for boards (§1.5). A scope's history is the history of its definition.

**Read ACLs** ([0023](../decisions/0023-moderation.md) §2) apply when members are shown, not when they are computed: a member the viewer may not read is omitted from the list and the count the viewer sees, as a search result is ([0056](../decisions/0056-security-model.md) §8).

### 1.5 Scopes in tables and boards

*Sources: [0060](../decisions/0060-scopes.md) §6; [0049](../decisions/0049-boards.md) §14.*

**Tables.** A table's `rows` takes every kind of §1.3, and a kind **`scope`** that names a scope page: `"rows": { "scope": "Scope:Women writers" }`. A table over a page-subject scope has pages as rows: its statement columns read page statements ([0038](../decisions/0038-page-metadata-and-categories.md) §1), its term columns are empty, and its sitelink column shows the paired item's. `tables.max_rows` still bounds the grid: a scope larger than it is shown to that many rows with the scope's own notice beneath. The two-write problem of §2.6 is gone for any scope that is not `ids`, since an entity created with the right statements appears by itself.

**Boards.** The rule fixed for boards before automatic scopes existed is that **attachments are records, and scope membership is a projection**, the same split [0038](../decisions/0038-page-metadata-and-categories.md) §2 makes between asserted and projected statements ([06](06-statements-and-properties.md) §5.3). A scoped board's threads are its attachments, plus the threads its scope selects, minus any its definition excludes. Selection by scope writes nothing to the thread: it does not enclose, it is not a `thread/attach` event, and it ends when the scope stops selecting the thread. The scope is the board definition's `scope` key, an object holding exactly one scope kind, as a table's `rows` is; version 1 of the board definition refuses it and `version: 2` accepts it ([14](14-discussions.md)). It selects **threads**: `{ "scope": "Scope:X" }` selects the threads attached to the talk pages of the scope's members, and the kinds of §1.3 may be written inline with the same meaning. Two kinds are the board's own:

| Kind | Selects |
|---|---|
| `thread_statement` | Threads carrying a given statement ([0038](../decisions/0038-page-metadata-and-categories.md) §9), such as a topic property bound to a role |
| `boards` | The threads of other boards, which is how boards nest |

`thread_statement` stays a board kind since a thread is not a subject in the scope's sense, and is where manual and automatic meet: a person states a thread's topic once, and every board scoped to that topic shows it. `mentions`, once a candidate here, is withdrawn in favour of a later `links` kind.

Not yet: whether scoped threads announce and notify is open ([0049](../decisions/0049-boards.md) Q2).

## 2. Tables

*Sources: [0045](../decisions/0045-table-content-model.md) §1, §2, §3, §4, §5, §6, §7; [0060](../decisions/0060-scopes.md) §6; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §3.*

### 2.1 A table is a page that names entities and properties

*Sources: [0045](../decisions/0045-table-content-model.md) §1.*

**A table is a page in the `Table` namespace (§2.2) whose content is a definition** (§2.3): which entities are its rows and which fields of those entities are its columns. **It stores no values.** Every cell is read from the row entity's resolved view ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3, [0004](../decisions/0004-identity-clusters-and-equivalence.md) §4) when the table is shown, and every cell edit is a change set on that entity (§2.5).

So a table has two kinds of change and two histories:

| Change | Written as | History it appears in |
|---|---|---|
| Rows or columns added, removed or reordered; the default sort or reference changed | A page revision of the table, in `pages` ([0008](../decisions/0008-namespaces-and-document-pages.md) §4) | The table's |
| A value changed in a cell | A change set on the row's entity, in `local` | The entity's, tagged with the table (§2.5) |

Nothing is copied, so a table can never disagree with its entities, and an edit made on the entity page shows in every table that includes it. A table is transcluded by the `#table` block (§4.3) and read from Lua by `mw.ext.triplespace.table`; a scope's `column` kind reads one of its columns (§1.3). Its links, feeds and backlinks are [16](16-logs-feeds-and-notifications.md), its API [18](18-api.md), its page [19](19-site-ui.md).

### 2.2 Namespace and content model

*Sources: [0045](../decisions/0045-table-content-model.md) §2, §3.*

`Table` is namespace 218, kind `pages`, with `Table talk` at 219; the allowed models are `triplespace-table` and `wikitext`, the default `triplespace-table`, and `wikitext` for titles ending `/doc`. The rows are in [10](10-pages-and-content-models.md) §1.4. The `/doc` rule follows Module's ([0043](../decisions/0043-lua-modules.md) §2): `Table:Journals/doc` is the table's documentation, and is shown above the grid. Both models are text models, so the namespace satisfies [0041](../decisions/0041-content-models.md) §4's one-source rule, and `changecontentmodel` may move a page between them as it may in Module. Titles normalize first-letter, as in Project; subpages are on. 218–219 was the last free pair in 210–219 ([10](10-pages-and-content-models.md) §1.3); the mediawiki.org registration of 210–229 is updated to include them.

`triplespace-table` is a Triplespace **text** model ([0041](../decisions/0041-content-models.md) §5; [10](10-pages-and-content-models.md) §4.4): source text, main slot, format `application/json`, directly editable, default in 218. The definition is stored as text in the page's `page` records, revised and diffed like any text page, and `action=edit` can change it. Only the grid is generated. No new source kind is needed.

| Operation (0041 §5) | For `triplespace-table` |
|---|---|
| Validate | Against the definition schema (§2.3). Content that fails is refused, as 0008 §5 refuses invalid `json` |
| Pre-save transform | The definition is canonicalized: IDs to their stored form ([0044](../decisions/0044-tenant-relative-ids.md) §1: tenant-relative input is never stored), keys in schema order, two-space indentation, **one row ID per line**. As MediaWiki's JSON content model pretty-prints on save, so that diffs and three-way merges work line by line |
| Serialize | The canonical JSON |
| Plain text for search | Title, `description`, and the column headers in the content language |
| Diff | Line diff of the canonical JSON: one line per added or removed row |
| Render | The grid (§2.4) |

### 2.3 The definition

*Sources: [0045](../decisions/0045-table-content-model.md) §4; [0060](../decisions/0060-scopes.md) §6; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §3.*

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
| `rows` | Yes | The scope: an object holding exactly one scope kind. **`ids`**: a list of entity IDs in any form the tenant accepts (local, foreign, another tenant's, keyed). Also every kind of §1.3 and **`scope`**, naming a scope page (§1.5); a page-subject scope gives rows that are pages, whose statement columns read page statements. A scope larger than `tables.max_rows` is shown to that many rows with the scope's notice. `M`, `WDM`, `L` and lexeme part IDs are accepted as rows; term columns show captions ([12](12-files-and-media.md)) and the lexeme term fields `lemma:{lang}`, `representation:{lang}`, `gloss:{lang}` ([0066](../decisions/0066-lexemes.md) §9; [04](04-entities-and-identifiers.md) §5.1) |
| `columns` | Yes | An ordered list of one or more columns (below) |
| `default_reference` | No | One reference in Wikibase's canonical JSON, without its hash. New statements written from the grid carry it unless the editor unchecks it (§2.5) |
| `sort` | No | The default order: a list of column positions (zero-based) and `ascending` or `descending`. Without it, rows appear in the order listed. A viewer can re-sort without saving |

**Columns.** Each column has a `field`, the keys that field needs, and an optional `title` that overrides the header:

| `field` | Keys | Shows |
|---|---|---|
| `label`, `description`, `aliases` | `language` | The term in that language |
| `statements` | `property` | The property's best values (§2.4) |
| `sitelink` | `site` | The page linked on that site. A host or a site alias ([0026](../decisions/0026-sitelinks.md) §2; [06](06-statements-and-properties.md) §4.3); aliases are canonicalized to the host on save |
| `thumbnail` | `width` | The thumbnail of an `M` or `WDM` row's file ([12](12-files-and-media.md)); the ID cell of such a row links the File page |

**What validation checks, and what it does not.** The schema, the syntax of every ID, no unknown keys, no duplicate row IDs (after canonicalization), and the limits: `tables.max_rows` (site setting, default 5,000, the same as [0020](../decisions/0020-change-feeds.md) §2's `feeds.related_limit`, so a table's related changes are never truncated, [16](16-logs-feeds-and-notifications.md)) and `tables.max_columns` (default 50). **It does not check that entities or properties exist.** A row whose entity is missing is shown as missing, as a red link is: entities are deleted, merged and mirrored after the table is saved, so existence at save time would guarantee nothing.

**Rows keep the ID as written.** Two rows that later turn out to be members of one identity cluster stay two rows; the grid marks the second as a duplicate (§2.4). The definition is not rewritten when clusters change, as source graphs keep IDs as asserted ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §4).

### 2.4 What a cell shows

*Sources: [0045](../decisions/0045-table-content-model.md) §5; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §3.*

**A row is the resolved view of its entity.** A non-canonical cluster member is shown as its canonical entity, as the API resolves it ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §4), with a note in the ID cell ("shown as Q5"). A row whose canonical entity already appears higher in the table is marked as a duplicate. A missing or deleted entity, or one the viewer may not read ([0023](../decisions/0023-moderation.md) §2), is a row with its ID and the state, and no cells.

**Statement cells follow 0003's rule: show the exception, not the rule.**

- The cell shows the property's **best values**: the preferred values if any, else the normal ones. Deprecated values are never shown inline.
- **One best value** is shown as 0003 §9's value cell, formatted by data type.
- **Several best values:** the first, then a fold, "+N", which opens the property's statement group ([0003](../decisions/0003-statement-ui.md)) in a popover, in whatever shape it takes there.
- **Only deprecated values:** an empty cell with a muted "N deprecated" fold.
- Qualifiers and references are not drawn in the cell. The popover shows them.
- A **constraint marker** ([0031](../decisions/0031-property-constraints.md) §3; [06](06-statements-and-properties.md) §2.3) appears beside a violating value, and the **correction chip** ([0003](../decisions/0003-statement-ui.md) §4) beside a mirrored value corrected here, as on the entity page.

**Term cells** show the term in the column's language. Where it is missing, the fallback is shown in italics with its language code, as Wikibase does, and editing that cell writes the column's language. For `M` and `WDM` rows term columns show captions and statement columns read the `mediainfo` slot ([12](12-files-and-media.md)). **Sitelink cells** show the linked title.

**Read restrictions apply cell by cell.** A statement hidden by a `statement` ACL is absent for viewers outside the group, as it is from the resolved view. A column whose property is under a `property` read ACL the viewer does not satisfy shows its header as restricted and its cells empty ([0023](../decisions/0023-moderation.md) §2; [09](09-security-and-moderation.md) §5.3).

### 2.5 Editing cells: one row, one change set

*Sources: [0045](../decisions/0045-table-content-model.md) §6.*

**Each row is saved on its own.** Edits accumulate in the grid by row; saving submits each changed row as **one change set on that row's entity**, an ordinary interactive edit with read-your-writes ([0013](../decisions/0013-postgres-storage.md) §7; [03](03-storage-caches-and-search.md) §6.2) and the row's own base revision. Rows are submitted in order, and each succeeds or fails alone. There is no `atomic` batch and no job.

**What a cell edit becomes:**

| Cell edit | Change set operation |
|---|---|
| A value typed into an empty statement cell | `add` a statement with that main snak and, unless unchecked, the table's `default_reference` ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.2, §8.5: an identical existing statement gets the reference merged onto it instead) |
| The one best value changed | The statement's main snak replaced, keeping its GUID, qualifiers and references, as Wikibase's `wbsetclaimvalue` does |
| The one best value cleared | `remove` of a local statement |
| A mirrored value changed or cleared | A local `override` or `add`, leaving the mirrored statement untouched: [0003](../decisions/0003-statement-ui.md) §8's "Correct a mirrored value" |
| A term or sitelink changed or cleared | The term or sitelink operation, under the term rights ([0016](../decisions/0016-permissions-and-access-control.md) §2) |

**A cell with several best values is not edited inline.** The editor opens the statement group in the popover and edits there, with 0003 §8's table editing. A grid cell cannot say which of several values a typed value replaces.

**Attribution.** Each change set carries the actor, an automatic summary naming the table ("Edited via [[Table:Journals]]") followed by the editor's own summary if given, and the change tag **`table:{page id}`** in its attestation part, a tag of the same kind as `job:{id}` ([0030](../decisions/0030-edit-filters.md) §5; [09](09-security-and-moderation.md) §7.6). Edits made through a table can then be found by filter, feed or tag.

**Everything else is the ordinary write path**, applied per row:

- **Permissions** are the actor's on each entity: entity, namespace, statement and property ACLs and protection ([0023](../decisions/0023-moderation.md) §2). Cells the viewer cannot edit are drawn read-only. **Protecting a table protects its definition only**; anyone who may edit an entity may edit its cells in any table.
- **Edit filters** ([0030](../decisions/0030-edit-filters.md) §4) run on each row. `warn` shows the public message on that row, and re-saving it resubmits with the token; `disallow` marks the row refused with the message. The remaining rows carry on.
- **Rate limits** ([0024](../decisions/0024-subsidiary-accounts.md) §5): each row is one `edit`, and a row that creates an entity one `create`. On a 429 the grid waits `Retry-After` and continues, showing how many rows are queued. There is no exemption: at `user`'s default of 90 a minute, a 500-row paste takes about six minutes. Volume beyond that is what subsidiaries and bulk jobs are for.
- **Conflicts.** If the entity changed since the grid loaded, the row's edits are applied to the current state and only the cells that clash are marked, as [0003](../decisions/0003-statement-ui.md) §8 does for a group. Other rows are unaffected.
- **Patrolling** is as for any edit ([0023](../decisions/0023-moderation.md) §6).

Cell edits made with the Wikibase modules directly are ordinary entity edits: a bot can edit what a table shows without going through the table ([18](18-api.md) §2.1).

### 2.6 Adding and removing rows

*Sources: [0045](../decisions/0045-table-content-model.md) §7; [0060](../decisions/0060-scopes.md) §6.*

**Rows are part of the definition.** Adding existing entities, by typing IDs, picking them with a lookup, or pasting a column of IDs, is a definition edit. Removing a row is a definition edit, and never touches the entity. The grid collects row additions and removals and saves them as **one page revision** per save, with the page's base revision; a conflicting definition edit is merged line by line, which the one-ID-per-line serialization (§2.2) makes reliable.

**A new row can create a new entity.** That is two writes in two partitions: the entity's `create`, carrying the row's cells as its first change set, in `local`; then the definition edit adding its ID, in `pages`. They are **not atomic**: [0011](../decisions/0011-logs.md) §6.1 does not make writes atomic across partitions, and tables do not start. If the second write fails, the entity exists and is not in the table; the grid says so and offers to add it again, and the creation's summary and tag name the table, so the entity can be found. For any `rows` that is not `ids` the second write does not arise: an entity created with the right statements appears by itself (§1.5).

## 3. Sprints, tasks and claims

*Sources: [0061](../decisions/0061-sprints-and-tasks.md) §1, §2, §3, §4, §5, §6, §7.*

### 3.1 A sprint is a page; a task is a projection; a claim is a record

*Sources: [0061](../decisions/0061-sprints-and-tasks.md) §1.*

**A sprint is a subpage in the `Project` namespace whose content model is `triplespace-sprint`** (§3.2). Its definition (§3.3) names a scope, a list of rules and a window. **A task** is one (sprint, rule, subject) triple that the rule currently finds open for a member of the scope. Tasks are **computed**: `view.task` (§3.5) is a projection over the scope's members and the indexes each rule reads, and a task is resolved by whatever write makes the rule's condition false, whoever made it and whether or not they knew the sprint existed. Credit goes to that write's actor (§3.6).

The one thing a person asserts is **a claim**: "I am working on this" (§3.4). It is a record, because it is a statement by a person that others act on, and it has to be visible, attributable and reversible. It is the only write a sprint adds to the log. A sprint page's history is its definition's revisions and its claims.

This is the asserted/projected split of [0038](../decisions/0038-page-metadata-and-categories.md) §2 and §1.5 once more: membership and task state are projected; the claim is asserted. The sprint page and the one notification a sprint sends are [16](16-logs-feeds-and-notifications.md) §5.6; the `#tasks` and `#sprint-progress` blocks transclude it (§4.3); its API is [18](18-api.md).

### 3.2 The model in the Project namespace

*Sources: [0061](../decisions/0061-sprints-and-tasks.md) §2, §3.*

`Project` (4) has `triplespace-sprint` among its allowed models, on subpages only; the default stays `wikitext` ([10](10-pages-and-content-models.md) §1.4, §3.5). **A `triplespace-sprint` page must be a subpage**: a title with no `/` in namespace 4, or any title in another namespace, is refused with `ts-sprint-title`. So `Project:WikiProject Women/Sprint March 2027` is a sprint; `Project:Sprint March 2027` is not; and a project's sprints sit under its page, where `Special:PrefixIndex` lists them. The restriction keeps the project page itself a document and keeps sprints findable, without a namespace.

A sprint is created by **`Special:CreateSprint`** ([21](21-special-pages.md)), which writes the page with the model set, or by creating a wikitext subpage and changing its model with `action=changecontentmodel` ([0041](../decisions/0041-content-models.md) §8; [10](10-pages-and-content-models.md) §4.5). Both are page `create` records in `pages`.

`triplespace-sprint` is a Triplespace **text** model, as the table, board and scope models are (§2.2, §1.2): source text, main slot, format `application/json`, directly editable, with no namespace it is the default in ([10](10-pages-and-content-models.md) §4.4). The definition is the page's content, revised and diffed as text. Validation checks the schema, the title rule above, the syntax of IDs and titles, the rule parameters (§3.3) and the limits; the pre-save transform canonicalizes IDs ([0044](../decisions/0044-tenant-relative-ids.md) §1), titles and key order; plain text for search is the title, description and rule titles; render is the sprint page ([16](16-logs-feeds-and-notifications.md) §5.6).

### 3.3 The definition and its rules

*Sources: [0061](../decisions/0061-sprints-and-tasks.md) §4; [0063](../decisions/0063-query-namespace.md) §6; [0064](../decisions/0064-entityschema-and-validation.md) §6.*

```json
{
  "version": 1,
  "description": "Women writers born in the 19th century: gender, birth date, article",
  "scope": "Scope:Women writers",
  "rules": [
    { "kind": "missing-statement", "property": "WDP21", "title": "Add gender" },
    { "kind": "missing-statement", "property": "WDP569" },
    { "kind": "constraint-violation", "severity": "mandatory" },
    { "kind": "missing-page", "title": "Write the article" }
  ],
  "from": "2027-03-01T00:00:00Z",
  "to": "2027-04-01T00:00:00Z",
  "board": "Board:Women writers sprint"
}
```

| Key | Required | Meaning |
|---|---|---|
| `version` | Yes | `1` |
| `description` | No | Plain text, shown under the title and indexed |
| `scope` | Yes | A scope title, or an inline kind object of §1.3. The subjects the rules are evaluated over |
| `rules` | Yes | One to `sprints.max_rules` (default 20) rules, each with a `kind`, its parameters and an optional `title` shown on its tasks |
| `from`, `to` | No | The window, RFC 3339. Without `from`, the window starts at the definition's first revision; without `to`, it never ends: an open-ended sprint is a project's standing backlog |
| `board` | No | A board ([14](14-discussions.md)) linked from the sprint page as its discussion. Without it, the sprint's own talk page serves |

**Rules.** A rule names a condition evaluated per member of the scope. Each has a **subject type** it applies to; members of the other type are skipped.

| Kind | Parameters | Open for a member when | Resolved when | Subject type | Reads |
|---|---|---|---|---|---|
| `missing-statement` | `property`; optional `value` | The member has no best-rank statement of the property, or none with that value | Such a statement exists | entity, or page | `view.statement_assertion` |
| `constraint-violation` | optional `property`, `constraint_type`, `severity` | The member has at least one `view.constraint_violation` row matching the parameters. One task per (member, property), however many rows | No matching row remains | entity, or page | `view.constraint_violation` ([0031](../decisions/0031-property-constraints.md) §5; [06](06-statements-and-properties.md) §2.2) |
| `missing-page` | optional `namespace` (default 0) | The member entity has no paired page in that namespace ([0038](../decisions/0038-page-metadata-and-categories.md) §6; [06](06-statements-and-properties.md) §4.6) | A paired page exists | entity | the sitelink projection |
| `query` | `query` (a `Query:` title), `params` | The member is bound to `?item` in the saved query's result (§5.5) | It is not, at the next refresh | the query's | the query service, on refresh |
| `schema-violation` | `schema` | The member's schema report says `conforms = false` (§6.5) | It says `true` | the report's | `view.schema_report` |

`subjects` is not a rule parameter: a rule's subject type follows the sprint's scope, so a `missing-statement` rule over a page-subject scope checks page statements.

The `query` and `schema-violation` rules are written as §5.5 and §6.5 show; the action each rule's task opens is on the sprint page, [16](16-logs-feeds-and-notifications.md) §5.6.

A rule's parameters are validated for syntax, not existence, as a table's columns are. **Limits:** `sprints.max_rules` (20); `sprints.max_tasks` (site, default 100,000, as `scopes.max_members`): tasks are materialized in the scope's member order, rule by rule, and cut at the limit with the scope notice component of §1.4 ("This sprint has more than 100,000 tasks; the first 100,000 are shown"); a truncated scope passes its notice through.

**What the rules do not cover,** on purpose: anything a person has to judge. "Needs copyediting" is a category or a page statement someone asserts, and a `category` scope with a `missing-statement` rule over a "reviewed" property expresses it; there is no free-text task.

### 3.4 Claims

*Sources: [0061](../decisions/0061-sprints-and-tasks.md) §5.*

**A claim is a record** of the payload type **`scatter:v0/task`**, in the tenant's `pages` partition, keyed to the sprint's page ID so that it appears in the sprint's history and is enclosed by the sprint page's ACLs ([0023](../decisions/0023-moderation.md) §2; [09](09-security-and-moderation.md) §4.4). Three standard parts ([01](01-log-and-records.md)). Operations:

| Operation | Content part | Meaning |
|---|---|---|
| `claim` | `rule` (index into the definition), `subject` `{kind, id}`, optional `note` | The actor is working on the task |
| `release` | `rule`, `subject` | The actor's claim, or any claim if the actor may `edit` the sprint page and passes `force: true`, is withdrawn |

**Validation**, against the sprint's projected state: a `claim` is refused if the task is not open (`ts-task-not-open`), if it is claimed by someone else and the claim has not expired (`ts-task-claimed`), or if the actor already holds `sprints.max_claims` (default 10) live claims on the sprint; a `release` is refused if there is no live claim to release. **A claim expires** after `sprints.claim_ttl` (default 7 days) without a `release`: the projection treats it as released, and nothing is written. A task that is resolved while claimed stays credited to its resolver, not its claimant (§3.6); the claimant is told by the `task-resolved` notification ([16](16-logs-feeds-and-notifications.md) §5.6).

**Permissions.** `claim` and `release` need `edit` on the sprint page, which is how a sprint restricts who may claim: protect the page. Anonymous and temporary accounts may not claim, since a claim is a promise with a name on it. Edit filters see a claim as a record with its content part ([0030](../decisions/0030-edit-filters.md) §5; [09](09-security-and-moderation.md) §7.2). Rate class `edit`.

**Logs.** `task/claim` and `task/release` are projected from the records, visible to everyone, as `as:Add` and `as:Remove` with `as:target` the sprint page and `as:object` the subject; the rows are in the log-event catalogue, [16](16-logs-feeds-and-notifications.md) §2.3.

### 3.5 The task projection

*Sources: [0061](../decisions/0061-sprints-and-tasks.md) §6; [0083](../decisions/0083-write-path-in-three-tiers.md) §3, §6.*

```sql
CREATE TABLE view.sprint (
  tenant text NOT NULL, page_id bigint NOT NULL,
  scope_page_id bigint,                 -- NULL for an inline scope, which is computed as a scope would be
  from_ts timestamptz, to_ts timestamptz,
  task_count integer NOT NULL, truncated boolean NOT NULL,
  definition_revid bigint NOT NULL,
  PRIMARY KEY (tenant, page_id)
);
CREATE TABLE view.task (
  tenant text NOT NULL, sprint_page_id bigint NOT NULL,
  rule smallint NOT NULL,
  subject_kind text NOT NULL, subject_id text NOT NULL,
  state text NOT NULL,                  -- open | claimed | resolved
  opened_at timestamptz NOT NULL,
  claimed_by text, claimed_at timestamptz, claim_record text,
  resolved_by text, resolved_at timestamptz, resolved_record text, resolved_revid bigint,
  reopened integer NOT NULL DEFAULT 0,
  PRIMARY KEY (tenant, sprint_page_id, rule, subject_kind, subject_id)
);
CREATE INDEX ON view.task (tenant, sprint_page_id, state);
CREATE INDEX ON view.task (tenant, subject_kind, subject_id);   -- "tasks about this subject"
CREATE INDEX ON view.task (tenant, sprint_page_id, resolved_by) WHERE state = 'resolved';
```

Both tables are in the `view` catalogue, [03](03-storage-caches-and-search.md) §4.15 and §5.

**When it runs.** The task projection is a tier-3 consumer that follows the scope projection (§1.4): it consumes the same composition events, after the scope consumer has applied them, in its own queue and outside every transaction, and is off during bootstrap and bulk modes ([0083](../decisions/0083-write-path-in-three-tiers.md) §3). It reads three kinds of change:

- **The scope changed.** Members added to the sprint's scope get a task per rule that finds them open; members removed lose their open and claimed tasks (resolved ones are kept, for credit). For a `query` scope this follows the refresh (§1.4).
- **A member changed.** A write to a subject that is a member of a sprint's scope (found through `view.scope_member`'s inverted index, then `view.sprint.scope_page_id`) re-evaluates that sprint's rules for that subject: a task whose condition is now false is **resolved**, with the write's actor, time, record and revision; a resolved task whose condition is true again is **reopened**, its resolution cleared and `reopened` incremented. Constraint re-checks by the property-wide job ([0031](../decisions/0031-property-constraints.md) §5) are writes by the job: a violation that disappears under a re-check resolves the task with the job as `resolved_by`, which the leaderboard (§3.6) does not count.
- **A claim record.** Sets or clears `claimed_by`; the state is `claimed` while a live claim exists on an open task.

A scope of a hundred thousand members with twenty rules is two million rows to compute when the sprint is saved, which is a job, as a scope's initial computation is (§1.4), and small per-event work after that. The sprint page shows "Tasks are being computed" until the job is done.

**The projection is view-derived** ([0083](../decisions/0083-write-path-in-three-tiers.md) §6): it is populated by a scan of the `view` tables it reads — `view.scope_member`, `view.statement_assertion`, `view.constraint_violation`, `view.schema_report`, `view.page_category` and the claim records' rows — and by **`view.activity`** for credit, and never by a log replay. "Rebuild tasks for tenant T from `view`" recomputes every sprint's tasks from the current members and rules and then re-derives each task's resolution history from the activity rows of its subject, so a rebuild reproduces the same `resolved_by`, `resolved_at`, `resolved_record` and `reopened` as the live projection wrote.

**What it does not do.** No activity row, no notification and no record for a task opening, resolving or reopening: these are projections of other people's writes, as §1.4 says of membership. The one row a sprint adds to feeds is a claim.

### 3.6 Credit, and the window

*Sources: [0061](../decisions/0061-sprints-and-tasks.md) §7; [0083](../decisions/0083-write-path-in-three-tiers.md) §6.*

**Whoever's write resolved the task is credited**, read from the `view.activity` row of the write whose composition event the projection was applying: `resolved_by` is that row's actor ([0007](../decisions/0007-actor-identity.md) §1; [07](07-actors-and-accounts.md)), `resolved_record` and `resolved_revid` point at its record. **Credit follows replay order.** Where two writes to one subject compose together, or a rebuild re-derives a task's history, the write that resolved the task is the first, in `(appended_at, partition, offset)` order ([0083](../decisions/0083-write-path-in-three-tiers.md) §6), after which the rule's condition is false, so the live projection and a rebuild from `view.activity` credit the same actor whatever order the events arrived in. There is no self-reporting and no sign-up: a person who never saw the sprint page and fixed a birth date on an item in scope counts. Subsidiary accounts ([0024](../decisions/0024-subsidiary-accounts.md)) are credited to the subsidiary, shown with its operator; jobs are credited to the job and not counted.

**The window frames the counting, not the computing.** Tasks are computed from the sprint's first revision, so that organizers see the backlog before the start. A resolution with `resolved_at` before `from` or after `to` is a resolved task (it is not open) but is **outside the window**, and the leaderboard and the progress figures count only resolutions inside it. The sprint page says "Starts in 3 days", "Day 12 of 31" or "Ended 2 April 2027".

**A revert takes the credit back.** When a task reopens, its resolution is cleared, and the actor's count falls. This is the correct answer for a reverted edit and the wrong one for a statement legitimately removed and re-added by someone else: the second resolver gets the credit ([0061](../decisions/0061-sprints-and-tasks.md) Q1).

## 4. Workspaces, blocks and kits

*Sources: [0062](../decisions/0062-workspaces.md) §1, §2, §3, §4, §5, §6, §9.*

### 4.1 A workspace is a project page that declares its scopes and transcludes blocks

*Sources: [0062](../decisions/0062-workspaces.md) §1.*

**A workspace is an ordinary `Project:` page** ([10](10-pages-and-content-models.md) §3.5) in wikitext, with two additions: a **declaration** of the scopes it is about (§4.2), and **blocks** (§4.3), parser functions that render a table, a count, a task board, a feed or a board listing over those scopes. Its sprints are its subpages (§3.2); its tables, boards and scopes are pages named under it by convention (`Table:WikiProject Women/Assessment`, `Board:WikiProject Women`, `Scope:WikiProject Women`). Nothing is a new namespace or content model: a workspace is what a project page becomes when it declares a scope.

So a WikiProject page keeps its prose, its membership list and its style guide, and gains the lists and dashboards it used to get from bots, each kept current by the projections behind it. A workspace is created, with its components, by `Special:CreateWorkspace` ([21](21-special-pages.md)) from a kit (§4.6).

### 4.2 Declaring scopes: roles

*Sources: [0062](../decisions/0062-workspaces.md) §2.*

**`{{#workspace: subjects=Scope:Women writers | articles=… | sources=… }}`**, once on the project page, declares the workspace's scopes by **role**. The expander records each as a page property, `workspace.{role}`, in `view.page_prop` ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6; [10](10-pages-and-content-models.md) §5.5), as `DEFAULTSORT` and `SHORTDESC` are recorded; the function itself renders nothing. A value is a scope title or an inline kind of §1.3 in the function's parameter syntax (`subjects=statement:WDP31=WDQ5`).

| Role | Subject type | Default when not declared |
|---|---|---|
| `subjects` | entities | — (required for a workspace) |
| `articles` | pages | `pages_of` the subjects (§1.3) |
| `sources` | entities | The entities cited in the subjects' references: a compiled query over the subjects' `prov:wasDerivedFrom` nodes, when the query service is on; absent otherwise |

A workspace usually has one scope that is really three; the editor declares the subjects and the rest is derived. A block names the role it reads (§4.3) and the derivation is invisible until overridden.

**Subpages inherit.** A block on `Project:WikiProject Women/Sprint March 2027` or `…/Participants` reads the nearest ancestor's `workspace.*` properties through the title's subpage chain; a subpage may declare its own `{{#workspace:}}` to narrow them (a task force is a subpage with `subjects=intersection:…`). A block that finds no declaration up the chain and names no scope renders the error chip "no workspace scope".

### 4.3 Blocks

*Sources: [0062](../decisions/0062-workspaces.md) §3; [0063](../decisions/0063-query-namespace.md) §6; [0064](../decisions/0064-entityschema-and-validation.md) §6.*

**A block is a parser function** registered in `wikitext-functions.toml` with origin `triplespace`, `expensive = true`, and the flag **`scoped = true`**: its output depends on a scope's membership or a table's, board's or sprint's state, and the render manifest records that dependency (below). The registry rows are [11](11-rendering-templates-and-modules.md) §2.2. Each takes an optional `scope=` (a role name, a scope title or an inline kind), defaulting to the role in its row, and the display parameters named.

| Function | Renders | Default role | Parameters |
|---|---|---|---|
| `#table` | A table (§2.4) | — | `1=Table:…` renders a saved table; or `columns=` (a comma list of `label:en`, `P31`, `sitelink:en.wikipedia.org`) with `scope=` renders an unsaved one; `sort=`, `rows=` up to `blocks.max_rows` |
| `#scope-count` | The member count, with the truncation notice when truncated | `subjects` | `scope=` |
| `#scope-members` | The first *n* members as a linked list, with "and 41,203 more" linking to the scope page | `subjects` | `scope=`, `rows=` |
| `#completeness` | For each property named, the share of members with a best-rank statement of it, as a bar per property | `subjects` | `properties=P21,P569,P19`, `scope=` |
| `#tasks` | A sprint's task board ([16](16-logs-feeds-and-notifications.md) §5.6), optionally one state or rule | — | `1=Project:…/Sprint …`, `state=open`, `rule=` |
| `#sprint-progress` | A sprint's progress bar, counts and leaderboard | — | `1=…`, `leaderboard=5` |
| `#board` | A board's thread listing ([14](14-discussions.md)), most recent first | — | `1=Board:…`, `rows=` |
| `#scope-changes` | The scope's related changes ([16](16-logs-feeds-and-notifications.md) §4.2), last *n* | `subjects` | `scope=`, `rows=`, `days=` |
| `#query` | A saved query's result table (§5.5) | — | `1=Query:…`, its parameters by name, `rows=` |
| `#conformance` | A schema's conformance counts and bar over a scope (§6.5) | `subjects` | `1=E…`, `scope=` |

**Bounds.** `blocks.max_rows` (site, default 200) bounds every list a block renders; a block never renders a whole 100,000-member scope into a page, and every truncated block links to the page that shows the rest. `blocks.max_per_page` (default 20) bounds blocks per render, since each is a query against `view`. Blocks count against the expensive-function limit ([0042](../decisions/0042-template-expansion-and-parsoid.md) §16; [11](11-rendering-templates-and-modules.md) §1.4).

**Dependencies.** The manifest has an entry kind, **scoped block**: the scope page ID and its `(definition_revid, computed_at)`, or the table's, sprint's or board's page ID and revision, plus for `#tasks` and `#sprint-progress` the sprint's last task state change and for `#scope-changes` the volatile "now" rule ([11](11-rendering-templates-and-modules.md) §4.2). The refresh job of [0042](../decisions/0042-template-expansion-and-parsoid.md) §10 re-renders a page whose block dependencies changed, as it re-renders for a changed template, with one addition: the scope projection and the task projection (§1.4, §3.5) **enqueue the refresh** of pages that depend on a scope or sprint they just changed, found through the manifest index `view.render_dep_scoped (page_id, kind, target_id)`. A busy sprint re-renders its project page often; `blocks.refresh_debounce` (default 60 s) coalesces that.

**Lua.** `mw.ext.triplespace.scope(title)` returns a scope's members (bounded as above) and count, and `mw.ext.triplespace.table(title)` a table's resolved rows, with the same manifest entries; this keeps modules able to build what blocks don't offer ([11](11-rendering-templates-and-modules.md) §7). Both are expensive.

**Rendering.** Blocks render server-side at expansion time through the `ExpandHost` ([0042](../decisions/0042-template-expansion-and-parsoid.md) §4; [11](11-rendering-templates-and-modules.md) §1.4), which has the scope, table, sprint and board reads; the output is the same HTML the pages themselves render ([19](19-site-ui.md); [16](16-logs-feeds-and-notifications.md) §5.6), so a table in a project page is editable in place where the viewer may edit, and a task's Claim button works where it is shown. Under the Parsoid renderer ([0042](../decisions/0042-template-expansion-and-parsoid.md) §8; [11](11-rendering-templates-and-modules.md) §3) blocks arrive as the expander's HTML, as `#invoke` output does.

`{{#table:Table:Journals}}` transcludes a table; `{{Table:Journals}}` (bare transclusion of a Table page) expands to the same, since the Table namespace's transclusion rule is defined as this function. Usage tracking is the manifest entry above.

### 4.4 Kit templates: blocks behind TemplateData

*Sources: [0062](../decisions/0062-workspaces.md) §4.*

Blocks are the primitive; **the editor's surface is a template that wraps a block**, with TemplateData ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §5; [11](11-rendering-templates-and-modules.md) §6.5), so that the template dialog of the visual editor ([0034](../decisions/0034-frontend-stack.md) §6) offers a form for "Assessment grid" with a scope picker and a properties list, and inserts `{{Workspace/Assessment grid|scope=articles}}` into the page. The templates live in the `Template` namespace under `Template:Workspace/…`, are ordinary templates a tenant may edit, and are what kits (§4.6) ship. A kit template's TemplateData parameter of type `triplespace-scope` (a TemplateData type registered beside `wiki-page-name`) makes the dialog show the scope picker of §4.5.

The instance ships a default set in the primary tenant's `Template:Workspace/` on `instance create` ([08](08-tenants-and-instances.md)): Assessment grid (quality and importance page statements over `articles`), Wanted articles (a `missing-page`-shaped table over `subjects`: label, sitelink count, "create"), Completeness, Open tasks, Recent changes, Discussion, Member sample, and Scope count. Each is a page a tenant may change; a farm shares them through page repositories ([0052](../decisions/0052-page-repositories-and-title-inheritance.md); [13](13-mirrored-pages.md)), as any template.

### 4.5 The scope builder

*Sources: [0062](../decisions/0062-workspaces.md) §5.*

The scope page's editor ([19](19-site-ui.md)) has a **Builder** tab beside the JSON source, and the same builder is the scope picker in kit templates and `Special:CreateWorkspace`. Every path edits the one `members` object; the JSON tab always shows what the builder built.

- **By example.** The editor pastes two or more entity IDs. A compiled query ([0059](../decisions/0059-query-service.md) §5; [02](02-graphs-rdf-and-query.md) §7.4) returns the best-rank statements they all share, each shown as a chip with the count of entities it would select alone and the count of the chips chosen together: "human 11.2M · female 2.1M · writer 190k · chosen together 41k". Choosing chips builds an `intersection` of `statement` kinds.
- **Facets.** For a draft with fewer than `scopes.facet_limit` members (default 1,000,000), the builder shows the distribution of the members' values for their most common properties, as clickable facets. A facet narrows (adds an `intersection`) or, in **split** mode, produces one child scope per value (`Scope:Women writers/France`, `…/Germany`), which is how task forces are made.
- **From a category, a scope, a table column or a query.** The remaining kinds of §1.3 as pickers; "Try with scope" on `Special:Query` ([0059](../decisions/0059-query-service.md) §6; [21](21-special-pages.md)) lands here with a `query` kind.
- **Algebra.** Operands as chips joined by AND, OR and NOT, nested by drag; `include` and `exclude` as lists.
- **Live count and sample.** Every change calls `POST /scope/preview` ([18](18-api.md) §3.2) and shows the count, twenty sample members with labels, and the truncation warning the moment a draft passes `scopes.max_members`.

**Without the query service**, by-example and facets fall back to single-pattern counts from the statement indexes (one chip at a time, no "chosen together"), and the builder says why. Everything else works.

### 4.6 Kits

*Sources: [0062](../decisions/0062-workspaces.md) §6.*

**A kit is a page of model `json`** ([0008](../decisions/0008-namespaces-and-document-pages.md) §5; [10](10-pages-and-content-models.md) §4.4) under `Project:Kits/`, validated by `Special:CreateWorkspace` against a schema when offered:

```json
{
  "version": 1,
  "name": "WikiProject",
  "description": "A project page with assessment, wanted articles, completeness, open tasks and a board",
  "roles": ["subjects", "articles"],
  "pages": [
    { "title": "Scope:{{name}}",                 "model": "triplespace-scope",  "content": "{{scope}}" },
    { "title": "Table:{{name}}/Assessment",       "model": "triplespace-table",  "content": { "...": "a definition with {{scope}} placeholders" } },
    { "title": "Board:{{name}}",                  "model": "triplespace-board",  "content": { "version": 1, "description": "{{name}} discussion" } },
    { "title": "Project:{{name}}/Backlog",        "model": "triplespace-sprint", "content": { "scope": "Scope:{{name}}", "rules": [ { "kind": "missing-page" } ] } },
    { "title": "Project:{{name}}",                "model": "wikitext",           "content": "{{#workspace:subjects=Scope:{{name}}}}\n{{Workspace/Assessment grid}}\n…" }
  ]
}
```

Placeholders: `{{name}}`, `{{scope}}` (the `members` object or the scope title the wizard produced), `{{description}}`. A kit may name pages in any namespace the creating user may create in. **Built-in kits** — WikiProject, Editathon (a scope, one sprint with a window, its leaderboard, a board), Source audit (a `sources` role, a reliability table, citing counts) — ship as `Project:Kits/…` pages in the primary tenant, like the kit templates of §4.4, and reach a farm's tenants through page repositories ([0052](../decisions/0052-page-repositories-and-title-inheritance.md); [13](13-mirrored-pages.md)), which is "export to other wikis" done as content. A tenant adds kits by adding pages. `GET /kits` lists the kits visible to the tenant ([18](18-api.md) §3.2).

### 4.7 Moving, deleting, and going quiet

*Sources: [0062](../decisions/0062-workspaces.md) §9.*

Components are named under the project by convention, not by containment, so **moving a project page** does not move them. The move form, when the page carries `workspace.*` properties, lists the component pages that `page_link` finds and offers to move them with it, as `move-subpages` offers subpages ([0051](../decisions/0051-page-redirects.md) §3; [10](10-pages-and-content-models.md) §2.5); the blocks' references are by title ([0060](../decisions/0060-scopes.md) Q3) and are rewritten in the same move. **Deleting** a project page deletes the page; its components stand until deleted. **A workspace nobody edits** costs a scope that keeps computing and some tables nobody opens: there is no banner to strip and no bot to retire, and the scope stays usable as an operand for the next group.

## 5. Query pages

*Sources: [0063](../decisions/0063-query-namespace.md) §1, §2, §3, §4, §5, §6.*

### 5.1 A query is a page

*Sources: [0063](../decisions/0063-query-namespace.md) §1.*

**A `Query:` page is a saved, documented, parameterized SPARQL query.** It has a title, a description, a history, a talk page, watchers and page statements like any page, and it can be run, with parameters, by anyone who can read it. Everything in scopes, sprints and blocks that takes inline SPARQL also takes a `Query:` page by title, so a query is written once and used as a scope, a rule, a block and a link (§5.5).

What it is not: an entity. Wikibase's plan made queries entities with IDs; here a query is a document with a name, because its content is text people edit and its identity is its title. Its topic, when it has one, is a page statement ([0038](../decisions/0038-page-metadata-and-categories.md) §1; [06](06-statements-and-properties.md) §5).

### 5.2 Namespace and content model

*Sources: [0063](../decisions/0063-query-namespace.md) §2, §3.*

`Query` is namespace 124, kind `pages` (it was `reserved`), with `Query talk` at 125; the allowed models are `triplespace-sparql` and `wikitext`, the default `triplespace-sparql`, and `wikitext` for titles ending `/doc`. The rows are in [10](10-pages-and-content-models.md) §1.4. 124 keeps the meaning Wikibase gave it. Titles normalize first-letter; subpages are on; the `/doc` rule is Table's (§2.2). `Query talk` is new, at the next odd number, as [0008](../decisions/0008-namespaces-and-document-pages.md) §2 rule 2 requires for an implemented subject namespace ([10](10-pages-and-content-models.md) §1.3).

`triplespace-sparql` is a Triplespace **text** model ([10](10-pages-and-content-models.md) §4.4): source text, main slot, format `application/json`, directly editable, default in 124. The model ID is not `wikibase-query`, which stays reserved for Wikibase's never-shipped model ([0041](../decisions/0041-content-models.md) §3 rule; [10](10-pages-and-content-models.md) §4.2). As the table, scope and sprint models are: validation checks the schema and the SPARQL (§5.3), the pre-save transform canonicalizes IDs ([0044](../decisions/0044-tenant-relative-ids.md) §1) in parameter defaults and key order, plain text for search is the title, description and parameter descriptions, render is the query page (§5.4).

### 5.3 The definition

*Sources: [0063](../decisions/0063-query-namespace.md) §4.*

```json
{
  "version": 1,
  "description": "People with a given occupation and their birthplaces",
  "sparql": "SELECT ?item ?itemLabel ?place WHERE { ?item wdt:P106 ?occupation ; wdt:P19 ?place . }",
  "params": [
    { "name": "occupation", "type": "entity", "description": "An occupation item", "default": "WDQ36180", "required": true }
  ],
  "limit": 1000
}
```

| Key | Required | Meaning |
|---|---|---|
| `version` | Yes | `1` |
| `description` | No | Plain text, shown under the title and indexed |
| `sparql` | Yes | A `SELECT` query, checked as [0059](../decisions/0059-query-service.md) §5 checks user SPARQL: no `FROM`, no `SERVICE`, no update operation. The prefixes of 0059 §6 are implied. Any variable may be projected; `?item` is what scopes and rules read (§5.5) |
| `params` | No | Up to `queries.max_params` (default 10) parameters. Each has a `name` (a SPARQL variable name without `?`, which must occur in `sparql`), a `type`, an optional `description`, `default` and `required` |
| `limit` | No | The query's own bound on solutions, capped by `query.max_results` |

**Parameter types and how they bind.** A parameter is **never substituted into the query text**. The compiler ([0059](../decisions/0059-query-service.md) §5; [02](02-graphs-rdf-and-query.md) §7.4) prepends a `VALUES` clause binding the variable to the supplied value as a SPARQL term, so a value can only ever be a term and the query's structure is fixed at save:

| Type | Value accepted | Bound as |
|---|---|---|
| `entity`, `property` | An entity ID in any form the tenant accepts; canonicalized | The entity's IRI |
| `string` | Text, optionally `@lang` | A literal, with language tag when given |
| `number`, `date` | A number; an ISO 8601 date or `xsd:dateTime` | A typed literal |
| `scope` | A scope title (§1) | `VALUES ?x { … }` over the scope's materialized members, up to `queries.max_scope_values` (default 5,000); a larger scope is refused with the scope's notice and a pointer to a `saved` scope kind, which composes the other way round (§5.5) |

A missing parameter takes its `default`; a `required` parameter without a default or a value refuses the run (`ts-query-param-required`). An unknown parameter is refused.

### 5.4 Running a query, and the page

*Sources: [0063](../decisions/0063-query-namespace.md) §5.*

**Running** compiles the definition with its bound parameters through the envelope of [0059](../decisions/0059-query-service.md) §5 (the tenant's dataset, `LIMIT` of `limit` + 1, the timeout) and returns solutions with the cursor they were computed at. Results are cached at L2 per `(definition revid, canonical parameters, cursor epoch)` for `query.cache_ttl`, as `/sparql` responses are ([0059](../decisions/0059-query-service.md) §6; [03](03-storage-caches-and-search.md) §9); the page shows the age. The route is `GET /query/{pageid}/run` ([18](18-api.md) §3.2, §6).

**The page** renders the description, a **parameter form** built from `params`, and the **result table**: one column per projected variable, entity values shown as labels in the viewer's language with links, literals as text, and a footer with the solution count, the truncation notice when the limit was reached, the cursor's age and the run time. There is one renderer in version 1: a one-column projection is a list and a `COUNT` is a single cell, and both are the table. Tabs: **Result**, **Definition** (the SPARQL editor of `Special:Query` with the parameter editor beside it), Related changes, talk, history. Actions: **Run**, **Save as scope** (a `saved` kind, §5.5), **Save as table** (a table whose `rows` is that scope), **Export** (CSV, TSV, JSON, as `/sparql` offers). The URL carries the parameters: `Query:People by occupation?occupation=Q36180` runs the query, so a query is a link, as it is on Wikidata, and the link has a name.

**`Special:Query` has "Save as Query page"** ([21](21-special-pages.md)): the current text becomes a new page's `sparql`, with variables the editor marks turned into parameters.

### 5.5 Where a query page is used

*Sources: [0063](../decisions/0063-query-namespace.md) §6; [0060](../decisions/0060-scopes.md) §4; [0061](../decisions/0061-sprints-and-tasks.md) §4; [0062](../decisions/0062-workspaces.md) §3.*

| Place | Form | Meaning |
|---|---|---|
| Scope kind **`saved`** (§1.3) | `{ "saved": { "query": "Query:X", "params": { "occupation": "WDQ36180" } } }` | The subjects bound to `?item` in the result. Computed on the scope's refresh as a `query` kind is; a definition revision of the query triggers the refresh of scopes that use it |
| Sprint rule **`query`** (§3.3) | `{ "kind": "query", "query": "Query:X", "params": {…}, "title": "…" }` | A task is open for a member of the sprint's scope while the member is bound to `?item` in the result, and resolved when it is not. The rule's open set is evaluated on the same refresh as a `saved` scope, and it is as fresh as the refresh, which the task says |
| Block **`#query`** (§4.3) | `{{#query:Query:X|occupation=Q36180|rows=50}}` | The result table in a page, bounded by `blocks.max_rows`, a `scoped` block whose manifest entry is the query's page and definition revision; `mw.ext.triplespace.query(title, params)` is the Lua form |
| A link | `[[Query:X?occupation=Q36180]]` | Runs it |

A `Query:` page therefore stands wherever scopes, sprints and blocks accepted inline SPARQL, and the inline forms stay for the one-off. `page_link` rows from scopes, sprints and pages to the query pages they use make "What links here" on a query list its users. The MCP server's `run_query` tool runs a saved query by title with its parameters ([18](18-api.md) §7.2).

## 6. EntitySchema pages and validation

*Sources: [0064](../decisions/0064-entityschema-and-validation.md) §1, §2, §3, §4, §5, §6.*

### 6.1 An entity type with a text body

*Sources: [0064](../decisions/0064-entityschema-and-validation.md) §1.*

**`entityschema` is an entity type**, local `E1`, foreign `WDE1` ([04](04-entities-and-identifiers.md) §2), hosted in namespace 640 (§6.2) under the `EntitySchema` model, as items are hosted in 120 under `wikibase-item`. It has **terms** — labels, descriptions, aliases — through the term machinery every entity has ([0013](../decisions/0013-postgres-storage.md) §5, `view.term`; [03](03-storage-caches-and-search.md) §4.3), so it is searched, shown in the viewer's language and resolved by label like any entity. It has **no statements and no sitelinks**, as on Wikidata. Its content is a **text body**, `schemaText`, in ShExC.

The JSON is EntitySchema's own page content, so that a Wikidata schema mirrors unchanged:

```json
{ "id": "E1", "serializationVersion": "3.0", "type": "ShExC",
  "labels": { "en": "human" }, "descriptions": { "en": "a person" }, "aliases": {},
  "schemaText": "PREFIX wd: <http://www.wikidata.org/entity/> …\nstart = @<human>\n<human> { wdt:P31 [ wd:Q5 ] ; … }" }
```

**Mirroring.** Wikidata's schemas are not in its entity dumps; the Wikidata adapter reads them as pages of namespace 640 from the XML dump or `prop=revisions`, and writes `put` records for `WDE` entities as for any entity ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4; [05](05-providers-and-ingest.md)). The Wikidata provider has the type `E` in `providers.toml` ([23](23-configuration-and-registry.md) §4.2). A mirrored schema is validated against local data like a local one (§6.4).

**Clusters.** Schemas may be `same-as` one another ([0004](../decisions/0004-identity-clusters-and-equivalence.md); [04](04-entities-and-identifiers.md) §4); the canonical schema's text is what is validated against. **Local overlays** ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7) on a foreign schema may change its terms; the text body is replaced whole, not overlaid, because a ShEx text has no statement-level parts.

Entity schemas have **no RDF**: no terms on an entity node, no type, nothing in any dump or in the update stream ([02](02-graphs-rdf-and-query.md) §5.9).

### 6.2 Namespace and content model

*Sources: [0064](../decisions/0064-entityschema-and-validation.md) §2, §3.*

`EntitySchema` is namespace 640, kind `pages`, with `EntitySchema talk` at 641; the only allowed model is `EntitySchema`, which is the default, and the talk namespace holds `triplespace-talk`. The rows are in [10](10-pages-and-content-models.md) §1.4. They are the EntitySchema extension's numbers, implemented with its meaning, as 124 is for `Query` (§5.2). Titles are entity IDs (`EntitySchema:E1`, `EntitySchema:WDE1`), normalized as entity namespaces are ([0008](../decisions/0008-namespaces-and-document-pages.md) §3); `Special:EntityPage/E1` resolves.

`EntitySchema` is an **entity** model of origin EntitySchema, source entity (`entityschema`), main slot, format `application/json`, not directly editable, default in 640 ([10](10-pages-and-content-models.md) §4.4): the page is the entity's view; edits go through the forms and modules of [21](21-special-pages.md) and [18](18-api.md) (`NewEntitySchema`, the schema page's **Edit schema** form, `action=entityschema&text=`), never through `action=edit`, as for items. The content is the JSON of §6.1. `scatter-wikibase-model` has the `entityschema` entity type: terms, and a `schema_text` body with a `schema_type` of `ShExC` ([22](22-crates-and-stack.md)).

### 6.3 Validation: `scatter-shex`

*Sources: [0064](../decisions/0064-entityschema-and-validation.md) §4.*

**A pure crate, `scatter-shex`** ([22](22-crates-and-stack.md)), implements ShExC parsing and validation:

- **The language**: ShEx 2.1 as Wikidata's schemas use it — shapes, shape references, triple constraints with cardinalities, value sets (IRIs, literals, stems, exclusions), node constraints (datatypes, facets), `AND`/`OR`/`NOT`, `EXTRA`, `CLOSED`, `start`, and the `wikibase:`, `wd:`, `wdt:`, `p:`, `ps:`, `pq:`, `pr:`, `prov:` prefixes bound as the dump binds them ([wikibase-compat.md](../api/wikibase-compat.md) §5.1). **Not validated**: `IMPORT`, `EXTERNAL`, semantic actions and annotations other than `rdfs:label`/`rdfs:comment`, and any prefix the tenant's dump does not define. A schema that uses them is saved and marked `unvalidatable`, with the reason on its page, since a mirrored Wikidata schema may use them and must still mirror.
- **The data**: validation runs over a `TripleSource` trait — `triples(subject) -> iterator` and `exists(iri)`. The default source renders the subject's triples through `scatter-wikibase-rdf` (format 1.0.0, full and truthy), exactly as the dump would, from the resolved JSON in `view`. A shape reference to another node (`wdt:P19 @<place>`) follows into that node's triples to **`schemas.max_depth`** (default 2); past the depth, a referenced shape is checked for its node constraints only, and the report says `depth_limited`. With the query service on ([0059](../decisions/0059-query-service.md); [02](02-graphs-rdf-and-query.md) §7), a tenant may set `schemas.source = query` to validate against the store instead, exact to any depth and lagging by the cursor.
- **The result**: `conforms`, and for a failure the **reasons**: which triple constraint of which shape failed on which node, in ShEx's result-shape-map form, which is what Wikidata's validators show and what the entity page can point at a statement.

Validation is pure and bounded: one subject, one schema, a depth, a triple budget (`schemas.max_triples`, default 10,000).

### 6.4 Binding a schema to a scope, and the report

*Sources: [0064](../decisions/0064-entityschema-and-validation.md) §5; [0083](../decisions/0083-write-path-in-three-tiers.md) §3, §6.*

**A schema applies where a scope says it does.** A scope definition (§1.3) has **`schemas`**: a list of schema IDs. Every member of the scope is validated against each. A schema has no statements, so the binding lives on the scope, and a schema's page lists the scopes bound to it through `page_link`. A tenant that wants "every human is checked against E10" writes a scope `statement: P31 = Q5` with `schemas: ["E10"]`.

```sql
CREATE TABLE view.schema_report (
  tenant text NOT NULL, schema_id text NOT NULL,
  subject_kind text NOT NULL, subject_id text NOT NULL,
  conforms boolean, reasons jsonb,          -- NULL conforms: not yet checked, or unvalidatable
  depth_limited boolean NOT NULL DEFAULT false,
  checked_at timestamptz, subject_revid bigint, schema_revid bigint,
  PRIMARY KEY (tenant, schema_id, subject_kind, subject_id)
);
CREATE INDEX ON view.schema_report (tenant, schema_id, conforms);
CREATE INDEX ON view.schema_report (tenant, subject_kind, subject_id);
```

The table is in the `view` catalogue, [03](03-storage-caches-and-search.md) §4.15 and §5.

**When it runs: in tier 3 only.** Validation against a bound schema is a tier-3 consumer that follows the scope projection (§1.4), consuming composition events in its own queue, outside every transaction, with its own lag shown on the schema page beside "checked at"; it **never runs inline in a write or in a fan-out budget**, and only the ad hoc check of §6.5 runs on request ([0083](../decisions/0083-write-path-in-three-tiers.md) §3). Its triggers are the `schema:{id}` rows of `view.scope_trigger` (§1.4): a **subject's composition** re-validates the subject against every schema bound to a scope it is in; **a scope's membership change** validates joining members and drops leaving ones; **a schema's revision** re-validates every member of every scope bound to it, as a job; and a periodic **`schemas.recheck`** job (default daily) re-validates subjects whose referenced nodes may have changed, which the subject's own composition does not see. **The cap.** Binding a schema to a scope is refused at scope save with `ts-schema-binding-cap` when the sum over the tenant of bound-schema × scope size — each binding's scope `count` from `view.scope`, or `scopes.max_members` for a scope not yet computed — would exceed **`schemas.max_bound`** (`site`, ceiling-bounded; default 1,000,000), unless an operator raises the ceiling; the refusal names the bindings that fill the cap. `schemas.max_subjects` (default `scopes.max_members`) bounds a schema's materialized report, truncated with the scope notice. The report writes no record and no activity row, as the constraint and scope projections do not, and the projection is view-derived ([0083](../decisions/0083-write-path-in-three-tiers.md) §6): a rebuild re-validates every bound subject from `view` as a job.

### 6.5 Where conformance shows

*Sources: [0064](../decisions/0064-entityschema-and-validation.md) §6.*

| Surface | Form |
|---|---|
| **The schema page** | Tabs: Schema (the ShExC, highlighted), **Conformance** (counts per bound scope: conforming, failing, unchecked; the failing subjects with their first reason), Scopes (bound through `page_link`), talk, history |
| **The entity page** | A marker beside the identity line, "Does not conform to *human* (E10): missing `P569`", in the style of the constraint markers of [0031](../decisions/0031-property-constraints.md) §3 ([06](06-statements-and-properties.md) §2.3), with the reason pointing at the statement or its absence; **Check against a schema** in the overflow menu runs an ad hoc check against any schema, not just the bound ones |
| **Scope kind `conforms`** (§1.3) | `{ "conforms": { "schema": "E10", "not": true } }`: the subjects of the schema's report with `conforms = false` (or true). A scope over a report, so "humans failing E10" is a scope and therefore a table, a feed and a watch |
| **Sprint rule `schema-violation`** (§3.3) | `{ "kind": "schema-violation", "schema": "E10" }`: a task per member whose report says `false`; resolved when it says `true`. The task's action opens the entity page at the first failing constraint |
| **Block `#conformance`** (§4.3) | `{{#conformance:E10|scope=subjects}}`: the counts and a bar; a `scoped` block on the schema and the scope |

The ad hoc check and the report are `POST /schema/{id}/check`, `GET /schema/{id}/report` and `GET /entity/{id}/schemas` ([18](18-api.md) §3.2); `Special:CheckEntitySchema` takes a schema and a subject and shows the result-shape map ([21](21-special-pages.md)).
