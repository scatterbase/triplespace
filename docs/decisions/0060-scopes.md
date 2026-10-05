# 0060. Scopes

- **Status:** Proposed
- **Date:** 2026-10-04
- **Updated:** 2026-10-05 (A4)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0020](0020-change-feeds.md), [0041](0041-content-models.md), [0045](0045-table-content-model.md), [0049](0049-boards.md), [0056](0056-security-model.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0023](0023-moderation.md), [0026](0026-sitelinks.md), [0038](0038-page-metadata-and-categories.md), [0042](0042-template-expansion-and-parsoid.md), [0044](0044-tenant-relative-ids.md), [0059](0059-query-service.md)

## Context

A WikiProject on Wikipedia operationalizes its subject by building a parallel categorization system: a banner on every talk page in scope, maintained by hand and by bots, left behind as clutter when the project goes quiet. James's 2023 essay on organizing work on Wikipedia proposed instead a **Pageset API**: define a set of articles once, by category membership or a Wikidata query, and let every tool that needs the project's scope read the same definition, so that "you could define a new project by mashing together existing lists of women, writers, and 19th century biographies, instead of having to define this complex scope yourself". The set updates itself as articles are created and deleted, and nothing is written to the articles.

Triplespace has already specified that object twice, inline. A table's `rows` is "an object holding exactly one scope kind" ([0045](0045-table-content-model.md) §4), with `ids` the only kind and query kinds left as Q1. A board's `scope` is reserved with the same shape ([0049](0049-boards.md) §4, §14), with four candidate kinds and the rule that selection by scope is a projection while attachment is a record. Each would carry its own copy of a definition that several things want to share, and neither can express the intersection of two others.

The query service of [0059](0059-query-service.md) now makes the expensive kinds answerable. What is missing is the named thing.

### Direction

James's direction, from the design discussion of 2026-10-04:

- **A scope is its own namespace and content model**, like Table and Board.
- **100,000 members is a good limit.** Beyond it a scope is cut off at the limit and a notice says so. The limit is instance-configurable.
- **Bots should not be necessary** for any of this.

## Decision

### 1. A scope is a page that names a set of subjects (extends 0056 §3)

**A scope is a page in the `Scope` namespace (§2) whose content is a definition (§4) of a set of subjects**, entities or pages. The set itself is computed: **membership is a projection** (§5), `view.scope_member`, recomputed as the definition and the data change, and written nowhere else. Nothing is added to a member when it joins a scope, and nothing is removed when it leaves; a scope is what a WikiProject banner would be if the banner were never placed.

A scope is **content**, and is never a target of restriction. The sets of [0056](0056-security-model.md) §3 are the other thing: a grouping only `protect` may change, by ID, because it restricts reading. A scope is edited by whoever may edit the page, may name things by title and by query, and grants and withholds nothing. The two do not meet.

What references a scope: a table's rows, a board's selection (§6), a feed and a watch (§7), and the sprints and workspaces that later ADRs build on it. Each names the scope by title and reads its members from the projection.

### 2. The `Scope` namespace (extends 0008 §2)

| Number | Canonical name | Kind | Allowed models | Default model |
|---|---|---|---|---|
| 312 | `Scope` | `pages` | `triplespace-scope`, `wikitext` | `triplespace-scope`; `wikitext` for titles ending `/doc` |
| 313 | `Scope talk` | `pages` | `triplespace-talk` | `triplespace-talk` |

The second pair of the 310–319 block ([0008](0008-namespaces-and-document-pages.md) §2, by 0008 A17), after Board. Titles normalize first-letter; subpages are on, and the `/doc` rule is Table's ([0045](0045-table-content-model.md) §2). A scope has an ordinary talk page, unlike a board, because a scope is a subject to discuss.

### 3. The `triplespace-scope` content model (extends 0041 §3)

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `triplespace-scope` | Triplespace | text | main | `application/json` | Yes | 312 |

A **text** model ([0041](0041-content-models.md) §5), as `triplespace-table` is: the definition is the page's content, revised and diffed as text. The operations follow 0045 §3 exactly, with these particulars: validation checks the schema, the syntax of every ID and title, the limits of §4, and the absence of cycles among referenced scopes (§4); the pre-save transform canonicalizes IDs to their stored form ([0044](0044-tenant-relative-ids.md) §1), titles to their normalized form, keys to schema order, and one ID per line in `ids`, `include` and `exclude`; plain text for search is the title, the description and the titles and properties the definition names; render is the scope page (§8).

### 4. The definition

*Changed by A2, A3, A4.*

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
| `schemas` | No | Entity schema IDs every member is validated against; the report is `view.schema_report` ([0064](0064-entityschema-and-validation.md) §5) |

**Kinds.** A kind selects subjects. Each has a **subject type** — entity or page, and since [0066](0066-lexemes.md) §5 also `form` and `sense` for lexeme parts, which `statement` selects when the property's statements sit on parts — which the scope takes from its kind; set algebra requires its operands to agree.

| Kind | Keys | Selects | Subject type | Computed by |
|---|---|---|---|---|
| `ids` | a list | The listed subjects. Entity IDs in any form the tenant accepts; pages by title | as listed | the definition |
| `statement` | `property`; one of `value`, `range {from, to}`, or none | Subjects with a best-rank statement of the property, with that value, in that range, or at all. Page subjects count ([0038](0038-page-metadata-and-categories.md) §1) | entity, or page if `subjects: "pages"` | `view.statement_assertion`, incrementally |
| `category` | `title` | The pages in the category, direct members only (`depth` is Q1) | page | `view.page_category`, incrementally |
| `column` | `table`, `column` | The entities in a column of a table: its rows for the ID column, the best values for a statement column | entity | the table's resolved rows |
| `query` | `sparql` | The subjects a `SELECT` projecting `?item` returns | entity or page, by IRI | the query service ([0059](0059-query-service.md) §5), on refresh |
| `pages_of` | `scope` or an inline kind | The articles paired with the entities of the operand ([0038](0038-page-metadata-and-categories.md) §6) | page | the pairing index |
| `entities_of` | `scope` or an inline kind | The entities paired with the pages of the operand | entity | the pairing index |
| `union`, `intersection` | a list of operands | The union or intersection of the operands' members | the operands' | set algebra over the operands' member rows |
| `difference` | `of`, `minus` | The members of `of` that are not in `minus` | `of`'s | set algebra |
| `saved` | `query` (a `Query:` title), `params` | The subjects bound to `?item` in a saved query's result, with the parameters given ([0063](0063-query-namespace.md) §6) | entity or page, by IRI | the query service, on refresh |
| `conforms` | `schema`, optional `not` | The subjects whose schema report says they conform, or with `not`, fail ([0064](0064-entityschema-and-validation.md) §6) | the report's | `view.schema_report` |

An **operand** is a scope title (`"Scope:Women"`) or an inline kind object. Operands nest to `scopes.max_depth` (default 8, counting referenced scopes' own definitions). **Cycles are refused** at save: the validator walks every referenced scope's current definition, and a save that would make a scope a member of its own ancestry fails with `ts-scope-cycle`. A referenced scope that does not exist is allowed, as a missing row entity is in a table: the reference is shown red and contributes nothing until the scope exists.

`subjects: "pages"` on a `statement` kind selects pages by their page statements instead of entities by theirs; without it a `statement` kind selects entities. A `query` kind's `sparql` is checked as [0059](0059-query-service.md) §5 says, at save, and refused with the compiler's reason; it is saved even while the query service is off, and the page says the scope is not computed (§5).

**Limits.** `scopes.max_members` (site, default **100,000**): the bound on a scope's materialized members (§5). `scopes.max_depth` (default 8). `scopes.max_ids` (default 5,000, as `tables.max_rows`) on `ids`, `include` and `exclude` together.

### 5. Membership is a projection

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
  kind     text   NOT NULL,                -- 'entity' | 'page'
  id       text   NOT NULL,                -- entity ID in stored form, or page ID in decimal
  PRIMARY KEY (tenant, page_id, kind, id)
);
CREATE INDEX ON view.scope_member (tenant, kind, id);  -- "scopes this subject is in"
```

**Two speeds.** `ids`, `statement`, `category`, `column`, `pages_of`, `entities_of` and the set algebra over them are **incremental**: the scope projection runs in step 7 of [0013](0013-postgres-storage.md) §7, after the statement, category and page projections, and applies the effect of each write to the scopes it touches, found through the inverted index above and through `page_link` (§8) for the kinds that name a property, category or table. These scopes are current within the fan-out budget of 0013 §7 (as amended), like referrers. `query` kinds, and any set algebra with a `query` operand, are **refreshed**: a job runs the compiled query at `scopes.refresh` (default 15 min) and whenever the definition is saved or a person asks (§8), and replaces the members in one transaction with the cursor the service reported. Which deltas affect an arbitrary query is not knowable, so these scopes are as fresh as the last refresh, and say so.

**The bound.** Members are materialized in a deterministic order, entity IDs then page IDs, each ascending, so that the prefix kept is stable between computations. When a computation yields more than `scopes.max_members`, the first `scopes.max_members` are kept and `truncated` is set. **The scope page, and every table, board, feed and watch that uses the scope, shows the notice**: "This scope has more than 100,000 members; the first 100,000 are shown." A truncated scope is still a full operand: `intersection` and `difference` are computed over the operands' complete sets where the operand is incremental, and by the query service where any operand is a query, and only the result is bounded. So `Scope:Humans` is useless to list and fine to intersect with.

**What the projection does not do.** It writes no record, produces no activity row and sends no notification: joining or leaving a scope is not an event, as [0049](0049-boards.md) §14 fixed for boards. A scope's history is the history of its definition.

**Read ACLs** ([0023](0023-moderation.md) §2) apply when members are shown, not when they are computed: a member the viewer may not read is omitted from the list and the count the viewer sees, as a search result is ([0056](0056-security-model.md) §8). A `query` kind can only ever select public subjects ([0059](0059-query-service.md) §3).

### 6. Scopes in tables and boards (amends 0045 §4; amends 0049 §4 and §14; settles 0045 Q1 and 0049 Q2)

**Tables.** `rows` gains every kind of §4, and a kind **`scope`** that names a scope page: `"rows": { "scope": "Scope:Women writers" }`. A table over a page-subject scope has pages as rows: its statement columns read page statements ([0038](0038-page-metadata-and-categories.md) §1), its term columns are empty, and its sitelink column shows the paired item's. `tables.max_rows` still bounds the grid: a scope larger than it is shown to that many rows with the scope's own notice beneath. 0045 Q1 is settled by this section; 0045 §7's two-write problem is gone for any scope that is not `ids`, since an entity created with the right statements appears by itself.

**Boards.** A board's `scope` key, refused in 0049 version 1, is accepted at `version: 2` of the board definition and selects **threads**: `{ "scope": "Scope:X" }` selects the threads attached to the talk pages of the scope's members; the kinds of §4 may be written inline with the same meaning. The thread-specific kinds of [0049](0049-boards.md) §14 are redrawn against this ADR: 0049's `subjects` is this ADR's `statement`; 0049's `statement` (threads carrying a statement themselves) stays a board kind, `thread_statement`, since a thread is not a subject in this ADR's sense; `boards` stays; `mentions` is withdrawn in favour of a later `links` kind (Q2). Selected threads are listed as 0049 §14 says: no attachment, no enclosure, no event, gone when the scope stops selecting them. 0049 Q2's "which kinds first" is this section; whether scoped threads announce and notify is still open there.

### 7. Feeds and watching (extends 0020 §2 and §3; settles 0045 Q4)

**A scope is a target set.** [0020](0020-change-feeds.md) §2 gains a row: **Scope** — the one-target sets of every member of a scope, from `view.scope_member`. Related changes of `Scope:Women writers` is the project's recent changes, and it is a join against materialized rows, not a relation evaluated in the query, so `feeds.related_limit` does not apply; `scopes.max_members` is its bound, and the feed carries the truncation notice when the scope is truncated. `Special:RecentChangesLinked/Scope:X` and the `rcscope` parameter of `list=recentchanges` serve it.

**Watching a scope** is a row in `private.watch` with kind `scope` ([0020](0020-change-feeds.md) §3), expanded at query time as the Scope set. It is one watch however many members the scope has, and it follows the members as they change. **A table's rows are watched the same way** (0045 Q4): a watch on a table with kind `rows` expands the table's `rows` as a scope would, whatever its kind. Notifications are not fanned out per member: a scope watch is a watchlist filter, not a subscription to each member ([0021](0021-notifications.md) is unchanged).

### 8. The scope page, links and the API (extends 0012 §5)

*Changed by A1.*

**The page** has a **Builder** tab beside the Definition tab, by example, by facet and by algebra, with a live count and sample through `POST /scope/preview` ([0062](0062-workspaces.md) §5). It shows the description, the count with its notice and its "as of" (the `computed_at`, and for query scopes the cursor's lag), the members in pages of 100 with their kind and label, a **Refresh** button for scopes with a `query` operand (needs `edit` on the page; rate class `query`), and tabs: Members, Definition (the JSON source editor, as a table's), Related changes, and the usual talk and history. A scope that has never been computed because the query service is off says so instead of a count.

**Links.** The page projection writes `page_link` rows ([0038](0038-page-metadata-and-categories.md) §10) from a scope to every scope, table, category and property its definition names, so that "What links here" on `Scope:Women` lists the scopes built on it, and moving or deleting a scope shows what it would break (Q3). Members get no link row: a hundred thousand backlinks would be the banner clutter this ADR removes.

**REST** (`triplespace/v0`): `GET /scope/{pageid}` (count, truncated, cursor, computed_at), `GET /scope/{pageid}/members?after=&limit=` (paged, kind and ID, labels on request), `GET /scope/{pageid}/export` (TSV of IDs), `POST /scope/{pageid}/refresh`. `GET /subject/{kind}/{id}/scopes` lists the scopes a subject is in, which the entity and page UIs show in the About panel ("In 3 scopes"). **Action API:** the definition through `prop=revisions` and `action=edit`, as a table's; `list=scopemembers&smscope=`.

### 9. Permissions and filters

Creating a scope needs `createpage` in namespace 312; editing one, `edit`. A tenant that wants fewer scopes restricts the namespace with an ACL ([0023](0023-moderation.md) §1). Edit filters ([0030](0030-edit-filters.md)) see a scope save as a page edit with the definition as text. There is no right to compute: a scope computes for everyone or no one, and a reader sees the members they may read.

### 10. Storage, caches and search

`view.scope` and `view.scope_member` as in §5; the scope projection in step 7 of [0013](0013-postgres-storage.md) §7 and the refresh job in `ops`. Cache keys: `sc:{page id}:{definition revid}:{computed_at}` for member pages, purged on recomputation; the scope page itself follows the page rules of [0014](0014-caches-and-search.md). Search indexes the plain text of §3; member lists are not indexed.

### 11. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-scope` | **New, layer 2, pure, wasm.** The `triplespace-scope` definition: schema, validation, canonicalization, cycle detection given a resolver of referenced definitions, the subject-type rules, and compilation of `query` and set-algebra kinds to SPARQL through the compiler contract of [0059](0059-query-service.md) §5. Depends on `scatter-pages` (the content model trait) and `scatter-wikibase-model` |
| `triplespace-projections` | `view.scope` and `view.scope_member`; the incremental scope projection and its inverted-index fan-out; the refresh job; the Scope target set and `rows` watch expansion; `page_link` rows from scope definitions |
| `triplespace-api-rest`, `triplespace-api-action` | The `/scope` routes and `/subject/…/scopes`; `list=scopemembers`; `rcscope` |
| `triplespace-ui` | The scope page and its editor; the scope notice component shared with tables, boards and feeds; "In n scopes" in About panels |
| `scatter-wikibase-shape` | `rows` kinds of §6 in the table model, by depending on `scatter-scope` |
| `scatter-threads` | Board definition `version: 2` with `scope` and `thread_statement` |

## Alternatives considered

- **Scopes as page statements on a Project page.** A WikiProject would carry its scope as statements. But a definition with nested operands and user SPARQL is a document, not a statement, and a statement is content that the resolver reads into every consumer; the page model keeps the definition editable as JSON and gives it a history.
- **Leaving kinds inline in tables and boards, no named scope.** Every table over a project's articles would repeat the project's definition, and no two would agree for long. The named scope is the essay's whole point.
- **Members as records**, so that joining a scope is an event. Rejected as [0049](0049-boards.md) §14 rejected it: a bot's worth of writes with no author, and clutter when the project goes quiet.
- **Refusing a scope over the bound.** The direction was to cut it off with a notice: a too-large scope is usually a mistake in a definition, and seeing the first hundred thousand members shows what the mistake was; and it stays usable as an operand.

## Consequences

- **A project's scope is defined once and read everywhere**: tables, boards, feeds, watches, and the sprints and workspaces to come. Nothing is written to the members.
- **Set algebra over scopes** gives the "mash together existing lists" of the essay; the query service does the work for any operand it has to.
- **Two speeds are visible.** Incremental scopes are current within fan-out; query scopes show their refresh time and lag. The notice component makes both, and truncation, read the same everywhere.
- **The 310 block has three free pairs left** (314–319). Sprints are a `Project:` subpage model by direction and take none.
- **Test plan.** Definitions: every kind, nesting to the depth limit, a cycle through three scopes, a missing referenced scope. Projection: an incremental scope follows a statement add and remove within one write; a `difference` with a truncated operand is exact; a page-subject `statement` scope follows page statements. Feeds: related changes of a scope equals the union of its members' one-target feeds; a `rows` watch on an `ids` table equals a watch on each row.

## Open questions

- **Q1. Category depth.** Whether `category` may take a `depth`, bounded, knowing that category trees loop and that the essay's complaint about them is right. A depth-limited walk over `view.page_category` is cheap to offer and easy to misuse.
- **Q2. A `links` kind.** Subjects that link to, or are linked from, the members of another scope, from `page_link` and `entity_ref`. 0049's `mentions` in general form.
- **Q3. References by title.** Definitions name scopes, tables and categories by title. A move leaves the references pointing at the old title, which `page_link` makes visible but does not fix. Whether the pre-save transform should store page IDs and the editor show titles, as it does for entity IDs, or a move should rewrite referring definitions.
- **Q4. Scopes as statement values.** Whether an entity or page may carry a statement whose value is a scope (a WikiProject item's "scope" property), which would let the pairing of [0038](0038-page-metadata-and-categories.md) §6 find a project's scope from its item.
- **Q5. Cross-tenant operands.** A scope on one tenant naming another tenant's scope. Refused now, since [0028](0028-tenancy-policy.md) §5 reads across tenants only anonymously and a scope's members are not a public form.
- **Q6. Sampling above the bound.** Whether a truncated scope should offer a random sample beside the stable prefix, which is more useful for seeing what a definition caught.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §11 | amends | 0005 A63 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §2 | extends | 0008 A25 |
| [0012](0012-api-requirements.md) §5 | §8 | extends | 0012 A40 |
| [0020](0020-change-feeds.md) §2, §3 | §7 | extends | 0020 A11 |
| [0041](0041-content-models.md) §3 | §3 | extends | 0041 A8 |
| [0045](0045-table-content-model.md) §4 | §6 | amends | 0045 A3 |
| [0049](0049-boards.md) §4, §14 | §6 | amends | 0049 A3 |
| [0056](0056-security-model.md) §3 | §1 | extends | 0056 A4 |

## References

- James Hare, [Three new concepts for organizing work on Wikipedia: Workspaces, Buckets, Sprints](https://harej.co/posts/2023/08/three-new-concepts-for-organizing-work-on-wikipedia-workspaces-buckets-sprints/) (2023): the Pageset API this ADR implements as scopes
- [PetScan](https://petscan.wmcloud.org/), the set algebra over categories, templates and SPARQL that editors use today
- [Listeria](https://www.wikidata.org/wiki/Wikidata:Listeria), the query-to-table bot that scopes and tables replace
- [0045](0045-table-content-model.md) §4 and Q1; [0049](0049-boards.md) §14: the inline scope objects this ADR names

## Amendment log

### A1. The scope builder

- **Date:** 2026-10-05
- **Source:** [0062](0062-workspaces.md) §5
- **Change:** extends §8
- **Summary:** The scope page gains a Builder tab: by example (shared statements with counts), facets and split, pickers for the other kinds, set algebra as chips, and a live count and sample through the preview route.

### A2. The `saved` kind

- **Date:** 2026-10-05
- **Source:** [0063](0063-query-namespace.md) §6
- **Change:** extends §4
- **Summary:** A scope over a saved query's result with parameters.

### A3. Schemas and the `conforms` kind

- **Date:** 2026-10-05
- **Source:** [0064](0064-entityschema-and-validation.md) §5–6
- **Change:** extends §4
- **Summary:** `schemas` on a definition binds entity schemas to the scope's members; `conforms` selects by the report.

### A4. Form and sense subjects

- **Date:** 2026-10-05
- **Source:** [0066](0066-lexemes.md) §5
- **Change:** extends §4
- **Summary:** Lexeme parts as subject types.

Replaced text (§4, in part):

> **Kinds.** A kind selects subjects. Each has a **subject type**, entity or page, which the scope takes from its kind; set algebra requires its operands to agree.
