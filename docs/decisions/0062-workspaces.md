# 0062. Workspaces

- **Status:** Proposed
- **Date:** 2026-10-05
- **Updated:** 2026-10-05 (A2)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0042](0042-template-expansion-and-parsoid.md), [0045](0045-table-content-model.md), [0047](0047-special-pages.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [0060](0060-scopes.md)
- **Uses:** [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0013](0013-postgres-storage.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md), [0024](0024-subsidiary-accounts.md), [0028](0028-tenancy-policy.md), [0031](0031-property-constraints.md), [0034](0034-frontend-stack.md), [0038](0038-page-metadata-and-categories.md), [0043](0043-lua-modules.md), [0049](0049-boards.md), [0052](0052-page-repositories-and-title-inheritance.md), [0059](0059-query-service.md), [0061](0061-sprints-and-tasks.md)

## Context

James's 2023 essay called its first concept **Workspaces**: "a system of templates that can be used to build WikiProjects or similar project areas, on-wiki", modular rather than turnkey, because the earlier WikiProject X meta-template "made the project templates very inflexible". Its stated goal: "there is no reason why it can't be as easy to start a WikiProject as it is to start a Facebook group." The essay reached for templates and bots because those could ship on Wikipedia in weeks; here the computed pieces are projections, and bots are not needed ([0060](0060-scopes.md), [0061](0061-sprints-and-tasks.md), by direction).

The pieces now exist as pages: a scope ([0060](0060-scopes.md)), tables over it ([0045](0045-table-content-model.md)), boards ([0049](0049-boards.md)), sprints ([0061](0061-sprints-and-tasks.md)), feeds ([0020](0020-change-feeds.md)) and queries ([0059](0059-query-service.md)). Three things are missing. **A way to define a scope without writing JSON or SPARQL**, since the scope is the one hard step and the one every other piece depends on. **A way to put the pieces on a page**: 0045 Q5 asks for tables in wikitext, and a project page is mostly prose, so the pieces have to live inside a document rather than replace it. **A way to start**: a form that creates the set, and a notion of a kit that a farm can share.

### Direction

James's direction, from the design discussions of 2026-10-04 and 2026-10-05:

- **Bots should not be necessary** for any of this.
- **One of the steps will be defining the applicable scopes.** Editors assemble a workspace from the features of 0059–0061; scope definition is the step to design around.

## Decision

### 1. A workspace is a project page that declares its scopes and transcludes blocks

**A workspace is an ordinary `Project:` page** ([0008](0008-namespaces-and-document-pages.md) §2) in wikitext, with two additions: a **declaration** of the scopes it is about (§2), and **blocks** (§3), parser functions that render a table, a count, a task board, a feed or a board listing over those scopes. Its sprints are its subpages ([0061](0061-sprints-and-tasks.md) §2); its tables, boards and scopes are pages named under it by convention (`Table:WikiProject Women/Assessment`, `Board:WikiProject Women`, `Scope:WikiProject Women`). Nothing is a new namespace or content model: a workspace is what a project page becomes when it declares a scope.

So a WikiProject page keeps its prose, its membership list and its style guide, and gains the lists and dashboards it used to get from bots, each kept current by the projections behind it.

### 2. Declaring scopes: roles (extends 0042 §5; extends 0055 §6)

**`{{#workspace: subjects=Scope:Women writers | articles=… | sources=… }}`**, once on the project page, declares the workspace's scopes by **role**. The expander records each as a page property, `workspace.{role}`, in `view.page_prop` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6), as `DEFAULTSORT` and `SHORTDESC` are recorded; the function itself renders nothing. A value is a scope title or an inline kind of [0060](0060-scopes.md) §4 in the function's parameter syntax (`subjects=statement:WDP31=WDQ5`).

| Role | Subject type | Default when not declared |
|---|---|---|
| `subjects` | entities | — (required for a workspace) |
| `articles` | pages | `pages_of` the subjects ([0060](0060-scopes.md) §4) |
| `sources` | entities | The entities cited in the subjects' references: a compiled query over the subjects' `prov:wasDerivedFrom` nodes, when the query service is on; absent otherwise |

A workspace usually has one scope that is really three; the editor declares the subjects and the rest is derived. A block names the role it reads (§3) and the derivation is invisible until overridden.

**Subpages inherit.** A block on `Project:WikiProject Women/Sprint March 2027` or `…/Participants` reads the nearest ancestor's `workspace.*` properties through the title's subpage chain; a subpage may declare its own `{{#workspace:}}` to narrow them (a task force is a subpage with `subjects=intersection:…`). A block that finds no declaration up the chain and names no scope renders the error chip "no workspace scope".

### 3. Blocks (extends 0042 §5 and §10; settles 0045 Q5)

*Changed by A1, A2.*

**A block is a parser function** registered in `wikitext-functions.toml` with origin `triplespace`, `expensive = true`, and a new flag **`scoped = true`**: its output depends on a scope's membership or a table's, board's or sprint's state, and the render manifest records that dependency (below). Each takes an optional `scope=` (a role name, a scope title or an inline kind), defaulting to the role in its row, and the display parameters named.

| Function | Renders | Default role | Parameters |
|---|---|---|---|
| `#table` | A table ([0045](0045-table-content-model.md) §5) | — | `1=Table:…` renders a saved table; or `columns=` (a comma list of `label:en`, `P31`, `sitelink:en.wikipedia.org`) with `scope=` renders an unsaved one; `sort=`, `rows=` up to `blocks.max_rows` |
| `#scope-count` | The member count, with the truncation notice when truncated | `subjects` | `scope=` |
| `#scope-members` | The first *n* members as a linked list, with "and 41,203 more" linking to the scope page | `subjects` | `scope=`, `rows=` |
| `#completeness` | For each property named, the share of members with a best-rank statement of it, as a bar per property | `subjects` | `properties=P21,P569,P19`, `scope=` |
| `#tasks` | A sprint's task board ([0061](0061-sprints-and-tasks.md) §8), optionally one state or rule | — | `1=Project:…/Sprint …`, `state=open`, `rule=` |
| `#sprint-progress` | A sprint's progress bar, counts and leaderboard | — | `1=…`, `leaderboard=5` |
| `#board` | A board's thread listing ([0049](0049-boards.md) §8), most recent first | — | `1=Board:…`, `rows=` |
| `#scope-changes` | The scope's related changes ([0060](0060-scopes.md) §7), last *n* | `subjects` | `scope=`, `rows=`, `days=` |
| `#query` | A saved query's result table ([0063](0063-query-namespace.md) §6) | — | `1=Query:…`, its parameters by name, `rows=` |
| `#conformance` | A schema's conformance counts and bar over a scope ([0064](0064-entityschema-and-validation.md) §6) | `subjects` | `1=E…`, `scope=` |

**Bounds.** `blocks.max_rows` (site, default 200) bounds every list a block renders; a block never renders a whole 100,000-member scope into a page, and every truncated block links to the page that shows the rest. `blocks.max_per_page` (default 20) bounds blocks per render, since each is a query against `view`. Blocks count against the expensive-function limit ([0042](0042-template-expansion-and-parsoid.md) §16).

**Dependencies.** The manifest gains an entry kind, **scoped block**: the scope page ID and its `(definition_revid, computed_at)`, or the table's, sprint's or board's page ID and revision, plus for `#tasks` and `#sprint-progress` the sprint's last task state change and for `#scope-changes` the volatile "now" rule. The refresh job of 0042 §10 re-renders a page whose block dependencies changed, as it re-renders for a changed template, with one addition: the scope projection and the task projection ([0060](0060-scopes.md) §5, [0061](0061-sprints-and-tasks.md) §6) **enqueue the refresh** of pages that depend on a scope or sprint they just changed, found through a new manifest index `view.render_dep_scoped (page_id, kind, target_id)`. A busy sprint re-renders its project page often; `blocks.refresh_debounce` (default 60 s) coalesces that.

**Lua.** `mw.ext.triplespace.scope(title)` returns a scope's members (bounded as above) and count, and `mw.ext.triplespace.table(title)` a table's resolved rows, with the same manifest entries; this is the Lua half of 0045 Q5 and keeps modules able to build what blocks don't offer. Both are expensive.

**Rendering.** Blocks render server-side at expansion time through the `ExpandHost` ([0042](0042-template-expansion-and-parsoid.md) §4), which gains the scope, table, sprint and board reads; the output is the same HTML the pages themselves render ([0045](0045-table-content-model.md) §11, [0061](0061-sprints-and-tasks.md) §8), so a table in a project page is editable in place where the viewer may edit, and a task's Claim button works where it is shown. Under the Parsoid renderer ([0042](0042-template-expansion-and-parsoid.md) §8) blocks arrive as the expander's HTML, as `#invoke` output does.

0045 Q5 is settled: `{{#table:Table:Journals}}` transcludes a table; `{{Table:Journals}}` (bare transclusion of a Table page) expands to the same, since the Table namespace's transclusion rule is defined as this function. Usage tracking is the manifest entry above.

### 4. Kit templates: blocks behind TemplateData

Blocks are the primitive; **the editor's surface is a template that wraps a block**, with TemplateData ([0055](0055-templatestyles-templatedata-and-page-properties.md) §5), so that the template dialog of the visual editor ([0034](0034-frontend-stack.md) §6) offers a form for "Assessment grid" with a scope picker and a properties list, and inserts `{{Workspace/Assessment grid|scope=articles}}` into the page. The templates live in the `Template` namespace under `Template:Workspace/…`, are ordinary templates a tenant may edit, and are what kits (§6) ship. A kit template's TemplateData parameter of type `triplespace-scope` (a new TemplateData type this ADR registers beside `wiki-page-name`) makes the dialog show the scope picker of §5.

The instance ships a default set in the primary tenant's `Template:Workspace/` on `instance create`: Assessment grid (quality and importance page statements over `articles`), Wanted articles (a `missing-page`-shaped table over `subjects`: label, sitelink count, "create"), Completeness, Open tasks, Recent changes, Discussion, Member sample, and Scope count. Each is a page a tenant may change; a farm shares them through page repositories ([0052](0052-page-repositories-and-title-inheritance.md)), as any template.

### 5. The scope builder (extends 0060 §8)

The scope page's editor ([0060](0060-scopes.md) §8) gains a **Builder** tab beside the JSON source, and the same builder is the scope picker in kit templates and `Special:CreateWorkspace`. Every path edits the one `members` object; the JSON tab always shows what the builder built.

- **By example.** The editor pastes two or more entity IDs. A compiled query ([0059](0059-query-service.md) §5) returns the best-rank statements they all share, each shown as a chip with the count of entities it would select alone and the count of the chips chosen together: "human 11.2M · female 2.1M · writer 190k · chosen together 41k". Choosing chips builds an `intersection` of `statement` kinds.
- **Facets.** For a draft with fewer than `scopes.facet_limit` members (default 1,000,000), the builder shows the distribution of the members' values for their most common properties, as clickable facets. A facet narrows (adds an `intersection`) or, in **split** mode, produces one child scope per value (`Scope:Women writers/France`, `…/Germany`), which is how task forces are made.
- **From a category, a scope, a table column or a query.** The remaining kinds of [0060](0060-scopes.md) §4 as pickers; "Try with scope" on `Special:Query` ([0059](0059-query-service.md) §6) lands here with a `query` kind.
- **Algebra.** Operands as chips joined by AND, OR and NOT, nested by drag; `include` and `exclude` as lists.
- **Live count and sample.** Every change calls `POST /scope/preview` (§8) and shows the count, twenty sample members with labels, and the truncation warning the moment a draft passes `scopes.max_members`.

**Without the query service**, by-example and facets fall back to single-pattern counts from the statement indexes (one chip at a time, no "chosen together"), and the builder says why. Everything else works.

### 6. Kits

**A kit is a page of model `json`** ([0008](0008-namespaces-and-document-pages.md) §5) under `Project:Kits/`, validated by `Special:CreateWorkspace` against a schema when offered:

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

Placeholders: `{{name}}`, `{{scope}}` (the `members` object or the scope title the wizard produced), `{{description}}`. A kit may name pages in any namespace the creating user may create in. **Built-in kits** — WikiProject, Editathon (a scope, one sprint with a window, its leaderboard, a board), Source audit (a `sources` role, a reliability table, citing counts) — ship as `Project:Kits/…` pages in the primary tenant, like the kit templates of §4, and reach a farm's tenants through page repositories ([0052](0052-page-repositories-and-title-inheritance.md)), which is the essay's "export to other wikis" done as content. A tenant adds kits by adding pages.

### 7. `Special:CreateWorkspace` (extends 0047 §9)

A wizard, restricted to `createpage`:

1. **Name**, and the project page to create or extend (an existing project page may be given a workspace by this form, which then only adds the declaration and blocks).
2. **Scope**, through the builder of §5, saved as `Scope:{name}` or kept inline.
3. **Kit**, from the kits the tenant can see, with a preview of every page it will create and the blocks the project page will carry; pages that already exist are shown as "kept" and not overwritten.
4. **Create.** Each page is an ordinary `create` by the user, in the `create` rate class ([0024](0024-subsidiary-accounts.md) §5), written in dependency order (scope, then tables and board, then sprint, then the project page) by one job so that a refused page stops the rest and reports why. The wizard then opens the project page.

`Special:CreateSprint` ([0061](0061-sprints-and-tasks.md) §9) stays the form for one more sprint under an existing workspace, and defaults its scope to the workspace's `subjects`.

### 8. API (extends 0012 §5)

| Route | Does |
|---|---|
| `POST /scope/preview` | Body: a `members` object. Returns count, truncated, a sample of 20 with labels, and for by-example the shared statements with counts. Compiles and runs without saving; rate class `query`; `query.timeout` applies |
| `GET /scope/{pageid}/facets?properties=` | Value distributions for the builder, bounded by `scopes.facet_limit` |
| `GET /workspace/{pageid}` | A project page's roles (from `view.page_prop`), the component pages its blocks and kit created (from `page_link`), and its sprints (subpages of model `triplespace-sprint`) |
| `POST /workspace` | Create from a kit: name, scope, kit title; returns the job |
| `GET /kits` | The kits visible to the tenant, with their schema-validated summaries |

### 9. Moving, deleting, and going quiet

Components are named under the project by convention, not by containment, so **moving a project page** does not move them. The move form, when the page carries `workspace.*` properties, lists the component pages that `page_link` finds and offers to move them with it, as `move-subpages` offers subpages ([0051](0051-page-redirects.md) §3); the blocks' references are by title (0060 Q3) and are rewritten in the same move. **Deleting** a project page deletes the page; its components stand until deleted. **A workspace nobody edits** costs a scope that keeps computing and some tables nobody opens: there is no banner to strip and no bot to retire, and the scope stays usable as an operand for the next group.

### 10. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-wikitext-expand` | `#workspace` and the block functions of §3 as registered parser functions whose data comes through `ExpandHost`; the `scoped` manifest entry kind; the `triplespace-scope` TemplateData type |
| `triplespace-render` | `ExpandHost` gains scope, table, sprint and board reads; renders blocks with the pages' own components; `view.render_dep_scoped` and the refresh enqueue from the scope and task projections; `mw.ext.triplespace.scope` and `.table` |
| `triplespace-ui` | The scope builder (Builder tab, picker), `Special:CreateWorkspace`, the kit preview |
| `triplespace-api-rest` | `/scope/preview`, `/scope/{pageid}/facets`, `/workspace`, `/kits` |
| `triplespace-query` | The by-example shared-statements query and the facet query |
| `triplespace-cli` | `instance create` writes the default kit templates and kits into the primary tenant |

## Alternatives considered

- **A workspace content model of its own,** holding a JSON definition plus prose. A WikiProject page is a document first; a JSON model would push its prose into a field and its lists out of the editor's hands. Page properties and parser functions keep it a page.
- **Scope roles as page statements** (0060 Q4). Statements need a property and a value type for a scope page, which does not exist yet; `{{#workspace:}}` into `page_prop` needs nothing new and is where `SHORTDESC` already lives. Q4 stays open for the day a scope is also an entity value.
- **Blocks only, no kits.** Every new project would start from a blank page; the essay's point is that starting should be one form.
- **Kits as a content model or config records.** A `json` page with a schema is enough, is editable, and inherits across a farm through page repositories like anything else.

## Consequences

- **Starting a project is a form**: a scope, a kit, a name. The result is ordinary pages, every one editable afterwards.
- **The scope is defined once**, by example or facet rather than by syntax, and every block, sprint and feed reads it.
- **Project pages stay documents.** Blocks sit in prose; the visual editor's template dialog is the editing surface.
- **Pages re-render when their scopes move**, through the manifest, debounced; a busy sprint's project page is the hot case and is bounded by `blocks.max_rows` and the debounce.
- **Test plan.** Expander: each block with and without a declaration, inheritance down a subpage chain, the bounds, and the manifest entries. Refresh: a statement write that changes a scope re-renders the one page whose block depends on it and no other. Builder: by-example chips agree with `COUNT` queries; facets agree with the member set; preview never writes. Wizard: a kit creates its pages in order and a refused page stops the rest. Transclusion: `{{Table:X}}` equals `{{#table:Table:X}}`.

## Open questions

- **Q1. Moving components.** Whether the move form's offer (§9) should be the default, and whether a component created by a kit should record its workspace in a page property so that the relation survives a renamed block.
- **Q2. Blocks in other namespaces.** Whether `#table` and `#scope-members` belong in articles (Listeria's use) under a tenant setting, given that an article's render then depends on a scope's refresh.
- **Q3. Aggregate tables.** The assessment grid's quality × importance cross-tab wants a table whose cells are counts over a scope, which is 0045 Q2's computed columns taken one step further; a table ADR, not this one.
- **Q4. Kits across tenants with different properties.** A kit written for Wikidata's properties (`WDP21`) fails on a tenant that does not mirror Wikidata; whether kits should name roles of the role map ([0003](0003-statement-ui.md) §7) and resolve them per tenant.
- **Q5. Workspace membership.** Whether "members of this project" should be computed from the leaderboards of its sprints and the editors of its scope, or stay a list people write.
- **Q6. The facet limit.** `scopes.facet_limit` at one million members is a guess at where `GROUP BY` on the embedded store stops being interactive; to be measured with 0059 Q5.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §10 | amends | 0005 A65 |
| [0012](0012-api-requirements.md) §5 | §8 | extends | 0012 A42 |
| [0042](0042-template-expansion-and-parsoid.md) §5, §10 | §2, §3 | extends | 0042 A9 |
| [0045](0045-table-content-model.md) §1 | §3 | extends | 0045 A4 |
| [0047](0047-special-pages.md) §9 | §7 | extends | 0047 A8 |
| [0055](0055-templatestyles-templatedata-and-page-properties.md) §6 | §2 | extends | 0055 A1 |
| [0060](0060-scopes.md) §8 | §5 | extends | 0060 A1 |

## References

- James Hare, [Three new concepts for organizing work on Wikipedia: Workspaces, Buckets, Sprints](https://harej.co/posts/2023/08/three-new-concepts-for-organizing-work-on-wikipedia-workspaces-buckets-sprints/) (2023)
- [Wikipedia:WikiProject X](https://en.wikipedia.org/wiki/Wikipedia:WikiProject_X), the meta-template approach the essay moved away from
- [Reasonator](https://reasonator.toolforge.org/) and [PetScan](https://petscan.wmcloud.org/): the "common statements" and set-algebra interactions the builder borrows
- [TemplateData](https://www.mediawiki.org/wiki/Extension:TemplateData): the parameter schema kit templates use
- [0042](0042-template-expansion-and-parsoid.md) §4, §10: the expander host and manifest that blocks extend

## Amendment log

### A1. `#query`

- **Date:** 2026-10-05
- **Source:** [0063](0063-query-namespace.md) §6
- **Change:** extends §3
- **Summary:** A block rendering a saved query's result with parameters.

### A2. `#conformance`

- **Date:** 2026-10-05
- **Source:** [0064](0064-entityschema-and-validation.md) §6
- **Change:** extends §3
- **Summary:** A block rendering a schema's conformance over a scope.
