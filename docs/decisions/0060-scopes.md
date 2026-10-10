# 0060. Scopes

- **Status:** Proposed
- **Date:** 2026-10-04
- **Updated:** 2026-10-09 (A10)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0020](0020-change-feeds.md), [0041](0041-content-models.md), [0045](0045-table-content-model.md), [0049](0049-boards.md), [0056](0056-security-model.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0023](0023-moderation.md), [0026](0026-sitelinks.md), [0038](0038-page-metadata-and-categories.md), [0042](0042-template-expansion-and-parsoid.md), [0044](0044-tenant-relative-ids.md), [0059](0059-query-service.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [15](../architecture/15-structured-pages.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

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

*Current text: [15](../architecture/15-structured-pages.md) §1.1.*

### 2. The `Scope` namespace (extends 0008 §2)

*Current text: [15](../architecture/15-structured-pages.md) §1.2.*

### 3. The `triplespace-scope` content model (extends 0041 §3)

*Current text: [15](../architecture/15-structured-pages.md) §1.2.*

### 4. The definition

*Changed by A2, A3, A4, A9.*

*Current text: [15](../architecture/15-structured-pages.md) §1.3, §5.5.*

### 5. Membership is a projection

*Changed by A7, A9.*

*Current text: [15](../architecture/15-structured-pages.md) §1.4.*

### 6. Scopes in tables and boards (amends 0045 §4; amends 0049 §4 and §14; settles 0045 Q1 and 0049 Q2)

*Current text: [15](../architecture/15-structured-pages.md) §1.5, §2.3, §2.6.*

### 7. Feeds and watching (extends 0020 §2 and §3; settles 0045 Q4)

*Changed by A6.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.2, §4.3, §5.2.*

### 8. The scope page, links and the API (extends 0012 §5)

*Changed by A1.*

*Current text: [18](../architecture/18-api.md) §2.3, §3.2; [19](../architecture/19-site-ui.md) §3.1, §6.9.*

### 9. Permissions and filters

*Current text: [09](../architecture/09-security-and-moderation.md) §7.2, §8.10.*

### 10. Storage, caches and search

*Changed by A5, A9.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.15, §5, §6.1, §11.1, §12.1.*

### 11. Crates (amends 0005 §2)

*Changed by A10.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **Scopes as page statements on a Project page.** A WikiProject would carry its scope as statements. But a definition with nested operands and user SPARQL is a document, not a statement, and a statement is content that the resolver reads into every consumer; the page model keeps the definition editable as JSON and gives it a history.
- **Leaving kinds inline in tables and boards, no named scope.** Every table over a project's articles would repeat the project's definition, and no two would agree for long. The named scope is the essay's whole point.
- **Members as records**, so that joining a scope is an event. Rejected as [0049](0049-boards.md) §14 rejected it: a bot's worth of writes with no author, and clutter when the project goes quiet.
- **Refusing a scope over the bound.** The direction was to cut it off with a notice: a too-large scope is usually a mistake in a definition, and seeing the first hundred thousand members shows what the mistake was; and it stays usable as an operand.

## Consequences

- **A project's scope is defined once and read everywhere**: tables, boards, feeds, watches, and the sprints and workspaces to come. Nothing is written to the members.
- **Set algebra over scopes** gives the "mash together existing lists" of the essay; the query service does the work for any operand it has to.
- ~~**Two speeds are visible.** Incremental scopes are current within fan-out; query scopes show their refresh time and lag. The notice component makes both, and truncation, read the same everywhere.~~ *Incremental scopes are current within the tier-3 consumer's lag, shown beside `computed_at`, not within a fan-out budget (A9).*
- **The 310 block has three free pairs left** (314–319). Sprints are a `Project:` subpage model by direction and take none.
- ~~**Test plan.** Definitions: every kind, nesting to the depth limit, a cycle through three scopes, a missing referenced scope. Projection: an incremental scope follows a statement add and remove within one write; a `difference` with a truncated operand is exact; a page-subject `statement` scope follows page statements. Feeds: related changes of a scope equals the union of its members' one-target feeds; a `rows` watch on an `ids` table equals a watch on each row.~~ *An incremental scope follows a write within the consumer's lag, not within the write, and a `difference` with a truncated operand is exact only where the operand is probeable; the rest of the plan stands (A9).*

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

### A5. Listing keys carry a listing version

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §10
- **Summary:** The scope listing key `sc:` (with `tp:` of [0049](0049-boards.md) §12 and `sp:` of [0061](0061-sprints-and-tasks.md) §11) carries a **listing version**, the maximum activity ID over the listing's members, computed in Postgres at read time, in place of purges on ordinary writes; [0014](0014-caches-and-search.md) §1 principles 1 and 4 hold unchanged. Tag purges remain for erasure and hiding. (PENDING B4)

Replaced text (§10):

> Cache keys: `sc:{page id}:{definition revid}:{computed_at}` for member pages, purged on recomputation; the scope page itself follows the page rules of [0014](0014-caches-and-search.md).

### A6. `notify` refused on scope and rows watches

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §7
- **Summary:** `notify` is refused on a `scope` or `rows` watch (`ts-watch-notify-unsupported`): those watches are feed filters, not subscriptions. The ledger row says amends; this ADR's §7 already calls a scope watch a watchlist filter and not a subscription, and nothing in it accepts `notify`, so the refusal extends it. (PENDING E31)

### A7. Form and sense members

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §5
- **Summary:** `view.scope_member.kind` admits `form` and `sense` beside `entity` and `page`, as A4's form and sense subjects need; a lexeme part is keyed by its part ID. (PENDING F9)

Replaced text (§5):

> ```
>   kind     text   NOT NULL,                -- 'entity' | 'page'
> ```

### A8. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§11
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [15](../architecture/15-structured-pages.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A9. Membership is a tier-3 consumer; `view.scope_trigger`; the initial computation is a job

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §3, §6
- **Change:** amends §5, §10; extends §4
- **Summary:** The scope projection no longer runs in step 7 of 0013 §7 within a fan-out budget: it is a tier-3 consumer of composition events, in its own queue with its own rate and lag, never inside the appending or composing transaction, off during bootstrap and bulk modes. A definition compiles at save to rows of `view.scope_trigger (tenant, trigger_kind, key, scope_page_id)` — `property:P31`, `category:{page}`, `table-row:{page}`, `sitelink`, `schema:{id}` — held in L0, so an event that matches no trigger is one hash probe and nothing more. The initial computation is always a job: the save compiles the triggers, writes `view.scope` with `computed_at` null and enqueues the computation; a rebuild is the same job for every scope of a tenant from `view`, the projection being view-derived. A truncated scope is "the first N found, refilled by a job when it falls below N"; it is a full operand only where it can be probed (`statement`, `category`, `ids`, `conforms`), and an `intersection` or `difference` whose non-probeable operand (`query`, `saved`, `column`, `pages_of`, `entities_of`) is truncated is refused at save with `ts-scope-truncated-operand`; a scope whose operand becomes truncated later is marked `stale`. Incremental scopes are current within the consumer's lag, shown beside `computed_at`. (REVIEW G10)

Replaced text ([15](../architecture/15-structured-pages.md) §1.4, as it stood):

> **Two speeds.** `ids`, `statement`, `category`, `column`, `pages_of`, `entities_of` and the set algebra over them are **incremental**: the scope projection runs in step 7 of [0013](0013-postgres-storage.md) §7 ([03](../architecture/03-storage-caches-and-search.md) §6.1), after the statement, category and page projections, and applies the effect of each write to the scopes it touches, found through the inverted index above and through `page_link` for the kinds that name a property, category or table. These scopes are current within the fan-out budget of 0013 §7, like referrers. `query` kinds, and any set algebra with a `query` operand, are **refreshed**: a job runs the compiled query at `scopes.refresh` (default 15 min) and whenever the definition is saved or a person asks (`POST /scope/{pageid}/refresh`, [18](../architecture/18-api.md) §3.2), and replaces the members in one transaction with the cursor the service reported. Which deltas affect an arbitrary query is not knowable, so these scopes are as fresh as the last refresh, and say so.

> **The bound.** Members are materialized in a deterministic order, entity IDs then page IDs, each ascending, so that the prefix kept is stable between computations. When a computation yields more than `scopes.max_members`, the first `scopes.max_members` are kept and `truncated` is set. **The scope page, and every table, board, feed and watch that uses the scope, shows the notice**: "This scope has more than 100,000 members; the first 100,000 are shown." A truncated scope is still a full operand: `intersection` and `difference` are computed over the operands' complete sets where the operand is incremental, and by the query service where any operand is a query, and only the result is bounded. So `Scope:Humans` is useless to list and fine to intersect with.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §4.15, as it stood):

> - `view.scope` and `view.scope_member` as in [0060](0060-scopes.md) §5; the scope projection in step 7 of §6.1 and the refresh job in `ops` ([0060](0060-scopes.md) §10).

### A10. Constant expansion in compiled SPARQL

- **Date:** 2026-10-09
- **Source:** [0082](0082-source-form-and-the-shared-view.md) §7
- **Change:** extends §11
- **Summary:** `scatter-scope` gains the expansion of entity constants in compiled SPARQL to the cluster's member set through the query service's rewriting layer, with property paths left untouched, so a `query` scope written with any member of a cluster finds the same subjects. (REVIEW G4)
