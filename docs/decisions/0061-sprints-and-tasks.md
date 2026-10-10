# 0061. Sprints and tasks

- **Status:** Proposed
- **Date:** 2026-10-04
- **Updated:** 2026-10-09 (A6)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0015](0015-record-format-and-partition-registry.md), [0021](0021-notifications.md), [0041](0041-content-models.md), [0047](0047-special-pages.md)
- **Uses:** [0003](0003-statement-ui.md), [0007](0007-actor-identity.md), [0013](0013-postgres-storage.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0026](0026-sitelinks.md), [0030](0030-edit-filters.md), [0031](0031-property-constraints.md), [0038](0038-page-metadata-and-categories.md), [0042](0042-template-expansion-and-parsoid.md), [0049](0049-boards.md), [0059](0059-query-service.md), [0060](0060-scopes.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [10](../architecture/10-pages-and-content-models.md), [15](../architecture/15-structured-pages.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md)

## Context

Wikipedia's work is organized in to-do lists, and "each to-do list created on-wiki is going to be its own silo, and Wikipedia is littered with these silos". James's 2023 essay proposed a **Taskset API** that would derive tasks from the wiki's own state rather than from a second database: a task to create an article is resolved when the article exists, a task to fix a tagged problem is resolved when the tag goes, so "the way you keep Taskset up to date is by keeping Wikipedia up to date". A **sprint** was the time-boxed campaign over such tasks, after WikiProject Women in Red's monthly themed drives: a scope, a goal, a period, and a running count of who did what.

Triplespace already computes the signals such tasks would read. The constraint projection holds every violation ([0031](0031-property-constraints.md) §5); the statement indexes say which subject lacks which property; the pairing of [0038](0038-page-metadata-and-categories.md) §6 says which item has no article; activity rows say who changed what and when ([0020](0020-change-feeds.md) §1). [0060](0060-scopes.md) gives the project its scope as a named, computed set. What is missing is the object that puts a rule set and a time window over a scope, and the one thing people assert about a task: that they are working on it.

### Direction

James's direction, from the design discussion of 2026-10-04:

- **A sprint is a `Project:` subpage** with its own content model, not a namespace.
- **Version 1 ships three rules**: `missing-statement`, `constraint-violation`, `missing-page`.
- **Anyone resolving a task in the window counts.** No sign-up.
- **Bots should not be necessary** for any of this.

## Decision

### 1. A sprint is a page; a task is a projection; a claim is a record

*Current text: [15](../architecture/15-structured-pages.md) §3.1.*

### 2. The `triplespace-sprint` model in the `Project` namespace (extends 0008 §2)

*Current text: [10](../architecture/10-pages-and-content-models.md) §1.4, §3.5; [15](../architecture/15-structured-pages.md) §3.2.*

### 3. The `triplespace-sprint` content model (extends 0041 §3)

*Current text: [15](../architecture/15-structured-pages.md) §3.2.*

### 4. The definition

*Changed by A1, A2, A4.*

*Current text: [15](../architecture/15-structured-pages.md) §3.3, §5.5.*

### 5. Claims (extends 0015 §1)

*Current text: [15](../architecture/15-structured-pages.md) §3.4; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 6. The task projection (extends 0013 §5.6 and §7)

*Changed by A6.*

*Current text: [15](../architecture/15-structured-pages.md) §3.5.*

### 7. Credit, and the window

*Current text: [15](../architecture/15-structured-pages.md) §3.6.*

### 8. The sprint page and notifications (extends 0021 §2)

*Changed by A2.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §5.2, §5.6.*

### 9. `Special:CreateSprint` (extends 0047 §9)

*Current text: [21](../architecture/21-special-pages.md) §6.2, §8.*

### 10. API (extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §2.3, §3.2.*

### 11. Storage, caches and search

*Changed by A3, A6.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.15, §5, §11.1, §12.1.*

### 12. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **Tasks as records.** A bot's worth of writes with no author, stale the moment the wiki moves, and a second thing to keep current. The essay's objection to task databases, applied to our own.
- **Sign-up as a record,** with credit limited to participants. Rejected by direction. A participants list can still be kept by the organizer as prose on the project page; the leaderboard is the record of who took part.
- **A `Sprint` namespace.** Rejected by direction; a sprint belongs under its project, and the subpage rule gives that for free.
- **Free-text tasks** ("rewrite the lead"). A task nobody can compute is a to-do item on a talk page, which threads already are. Adding them here would recreate the silo.
- **Credit to the claimant.** Rewards claiming over doing, and gives the wrong answer when someone else does the work. The claimant is notified instead.

## Consequences

- **A sprint is kept current by editing the wiki.** No bot, no self-reporting, no second database: the essay's Taskset, as a projection.
- **Women in Red's month works out of the box:** a scope (women writers without an article), one `missing-page` rule, a window, a board, and a leaderboard nobody maintains.
- **Credit is honest and sometimes blunt.** A revert removes it; a job earns none; the window frames it. Q1 holds the known rough edge.
- **The claim is the one new record type,** small and enclosed by the sprint page's ACLs, so protection and deletion of a sprint behave as for any page.
- ~~**Test plan.** Rules: each of the three against fixtures with the condition true and false, and against page subjects where allowed. Projection: a statement write resolves a task within the same write; a revert reopens it and clears the credit; a scope change opens and drops tasks; a claim, an expired claim and a `force` release. Window: resolutions before, inside and after count as specified. Load: a 100,000-member scope with 20 rules computes as a job and the page reports progress.~~ *A statement write resolves a task within the consumer's lag, not within the same write; the rest of the plan stands, and a rebuild from `view` reproducing the live resolutions is added to it (A6).*

## Open questions

- **Q1. Credit across a remove-and-re-add.** A statement removed by one person and re-added by another credits the second and reopens then re-resolves the task; whether the first resolver should keep a credit for a resolution that stood for some time.
- **Q2. More rules.** `missing-term lang`, `in-category` (maintenance tags), `stale-since date`, `missing-reference` (a statement of the property without a reference), `redlink` (a missing title linked from a member page, whose subject would be a title), and `proposal-open` once proposals exist. `missing-term` and `missing-reference` are a day's work each; `redlink` changes the subject type.
- **Q3. Goals.** Whether a definition may set a target ("500 articles") shown against progress, and whether a sprint may be marked closed before `to`.
- **Q4. Recurring sprints.** A monthly drive is a new page each month today; whether a definition may carry a recurrence that creates the next page, which would be the one job that writes a page.
- **Q5. Sprints over several tenants' subjects.** Refused now ([0028](0028-tenancy-policy.md) §5); a farm-wide drive would need a farm-base sprint reading each tenant's public tasks.
- **Q6. Claims on a protected subject.** A claim needs `edit` on the sprint page, not on the subject; whether a claim should be refused when the claimant could not edit the subject, which the statement UI will tell them anyway.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §12 | amends | 0005 A64 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §2 | extends | 0008 A26 |
| [0011](0011-logs.md) §6.1, §8 | §5 | extends | 0011 A19 |
| [0012](0012-api-requirements.md) §5 | §10 | extends | 0012 A41 |
| [0015](0015-record-format-and-partition-registry.md) §1 | §5 | extends | 0015 A29 |
| [0021](0021-notifications.md) §2 | §8 | extends | 0021 A9 |
| [0041](0041-content-models.md) §3 | §3 | extends | 0041 A9 |
| [0047](0047-special-pages.md) §9 | §9 | extends | 0047 A7 |

## References

- James Hare, [Three new concepts for organizing work on Wikipedia: Workspaces, Buckets, Sprints](https://harej.co/posts/2023/08/three-new-concepts-for-organizing-work-on-wikipedia-workspaces-buckets-sprints/) (2023): the Taskset API this ADR implements as a projection
- [Wikipedia:WikiProject Women in Red](https://en.wikipedia.org/wiki/Wikipedia:WikiProject_Women_in_Red), the monthly drives sprints are modelled on
- [Programs & Events Dashboard](https://outreachdashboard.wmflabs.org/) and Event Metrics, the sign-up-based tools whose credit model this ADR inverts
- [0031](0031-property-constraints.md) §5, [0038](0038-page-metadata-and-categories.md) §6, [0060](0060-scopes.md) §5: the signals the three rules read

## Amendment log

### A1. The `query` rule

- **Date:** 2026-10-05
- **Source:** [0063](0063-query-namespace.md) §6
- **Change:** extends §4
- **Summary:** A task open while the member appears in a saved query's result; the general rule Q2 asked for, as fresh as the refresh.

### A2. The `schema-violation` rule

- **Date:** 2026-10-05
- **Source:** [0064](0064-entityschema-and-validation.md) §6
- **Change:** extends §4, §8
- **Summary:** A task per member failing a schema, resolved when it conforms; its action opens the first failing constraint.

### A3. Listing keys carry a listing version

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §11
- **Summary:** The sprint listing key `sp:` (with `tp:` of [0049](0049-boards.md) §12 and `sc:` of [0060](0060-scopes.md) §10) carries a **listing version**, the maximum activity ID over the listing's members, computed in Postgres at read time, in place of purges on ordinary writes such as a task changing state; [0014](0014-caches-and-search.md) §1 principles 1 and 4 hold unchanged. Tag purges remain for erasure and hiding. (PENDING B4)

Replaced text (§11):

> Cache keys `sp:{page id}:{definition revid}` for the page's counts and leaderboard, purged when any task of the sprint changes state, which is the one hot key here and is cached at L1 for `sprints.cache_ttl` (default 30 s) to absorb a busy drive.

### A4. No `subjects` rule parameter

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §4
- **Summary:** `subjects` is not a rule parameter: a rule's subject type follows the sprint's scope. (PENDING F11)

Replaced text (§4):

> | `missing-statement` | `property`; optional `value` | The member has no best-rank statement of the property, or none with that value | Such a statement exists | entity, or page with `subjects: "pages"` | `view.statement_assertion` |

### A5. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§12
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [10](../architecture/10-pages-and-content-models.md), [15](../architecture/15-structured-pages.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A6. The task projection is a view-derived, tier-3 consumer

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §3, §6
- **Change:** amends §6; extends §11
- **Summary:** The task projection is a tier-3 consumer that follows the scope projection: it consumes the same composition events, after the scope consumer has applied them, in its own queue and outside every transaction, and is off during bootstrap and bulk modes; "inside the fan-out budget of 0013 §7" is withdrawn, and the small work after the initial job is per event, not per write. The projection is view-derived: populated by a scan of `view.scope_member`, `view.statement_assertion`, `view.constraint_violation`, `view.schema_report`, `view.page_category` and the claim records' rows, and by `view.activity` for credit, never by a log replay; "rebuild tasks for tenant T from `view`" recomputes every sprint's tasks and re-derives each task's resolution history from its subject's activity rows, so a rebuild reproduces the same `resolved_by`, `resolved_at`, `resolved_record` and `reopened`. (REVIEW G10)

Replaced text ([15](../architecture/15-structured-pages.md) §3.5, as it stood):

> **When it runs.** The task projection runs after the scope projection in step 7 of [0013](0013-postgres-storage.md) §7 ([03](../architecture/03-storage-caches-and-search.md) §6.1) and reads three kinds of change:

> All of this is inside the fan-out budget of 0013 §7: a scope of a hundred thousand members with twenty rules is two million rows to compute when the sprint is saved, which is a job, and small per-write work after that. The sprint page shows "Tasks are being computed" until the job is done.
