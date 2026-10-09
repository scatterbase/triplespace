# 0062. Workspaces

- **Status:** Proposed
- **Date:** 2026-10-05
- **Updated:** 2026-10-09 (A3)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0042](0042-template-expansion-and-parsoid.md), [0045](0045-table-content-model.md), [0047](0047-special-pages.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [0060](0060-scopes.md)
- **Uses:** [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0013](0013-postgres-storage.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md), [0024](0024-subsidiary-accounts.md), [0028](0028-tenancy-policy.md), [0031](0031-property-constraints.md), [0034](0034-frontend-stack.md), [0038](0038-page-metadata-and-categories.md), [0043](0043-lua-modules.md), [0049](0049-boards.md), [0052](0052-page-repositories-and-title-inheritance.md), [0059](0059-query-service.md), [0061](0061-sprints-and-tasks.md)
- **Chapters:** [15](../architecture/15-structured-pages.md), [18](../architecture/18-api.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md)

## Context

James's 2023 essay called its first concept **Workspaces**: "a system of templates that can be used to build WikiProjects or similar project areas, on-wiki", modular rather than turnkey, because the earlier WikiProject X meta-template "made the project templates very inflexible". Its stated goal: "there is no reason why it can't be as easy to start a WikiProject as it is to start a Facebook group." The essay reached for templates and bots because those could ship on Wikipedia in weeks; here the computed pieces are projections, and bots are not needed ([0060](0060-scopes.md), [0061](0061-sprints-and-tasks.md), by direction).

The pieces now exist as pages: a scope ([0060](0060-scopes.md)), tables over it ([0045](0045-table-content-model.md)), boards ([0049](0049-boards.md)), sprints ([0061](0061-sprints-and-tasks.md)), feeds ([0020](0020-change-feeds.md)) and queries ([0059](0059-query-service.md)). Three things are missing. **A way to define a scope without writing JSON or SPARQL**, since the scope is the one hard step and the one every other piece depends on. **A way to put the pieces on a page**: 0045 Q5 asks for tables in wikitext, and a project page is mostly prose, so the pieces have to live inside a document rather than replace it. **A way to start**: a form that creates the set, and a notion of a kit that a farm can share.

### Direction

James's direction, from the design discussions of 2026-10-04 and 2026-10-05:

- **Bots should not be necessary** for any of this.
- **One of the steps will be defining the applicable scopes.** Editors assemble a workspace from the features of 0059–0061; scope definition is the step to design around.

## Decision

### 1. A workspace is a project page that declares its scopes and transcludes blocks

*Current text: [15](../architecture/15-structured-pages.md) §4.1.*

### 2. Declaring scopes: roles (extends 0042 §5; extends 0055 §6)

*Current text: [15](../architecture/15-structured-pages.md) §4.2.*

### 3. Blocks (extends 0042 §5 and §10; settles 0045 Q5)

*Changed by A1, A2.*

*Current text: [15](../architecture/15-structured-pages.md) §4.3, §5.5.*

### 4. Kit templates: blocks behind TemplateData

*Current text: [15](../architecture/15-structured-pages.md) §4.4.*

### 5. The scope builder (extends 0060 §8)

*Current text: [15](../architecture/15-structured-pages.md) §4.5.*

### 6. Kits

*Current text: [15](../architecture/15-structured-pages.md) §4.6.*

### 7. `Special:CreateWorkspace` (extends 0047 §9)

*Current text: [21](../architecture/21-special-pages.md) §6.2, §6.3, §8.*

### 8. API (extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §3.2, §6.*

### 9. Moving, deleting, and going quiet

*Current text: [15](../architecture/15-structured-pages.md) §4.7.*

### 10. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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

### A3. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [15](../architecture/15-structured-pages.md), [18](../architecture/18-api.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
