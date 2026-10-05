# 0061. Sprints and tasks

- **Status:** Proposed
- **Date:** 2026-10-04
- **Updated:** 2026-10-05 (A2)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0015](0015-record-format-and-partition-registry.md), [0021](0021-notifications.md), [0041](0041-content-models.md), [0047](0047-special-pages.md)
- **Uses:** [0003](0003-statement-ui.md), [0007](0007-actor-identity.md), [0013](0013-postgres-storage.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0026](0026-sitelinks.md), [0030](0030-edit-filters.md), [0031](0031-property-constraints.md), [0038](0038-page-metadata-and-categories.md), [0042](0042-template-expansion-and-parsoid.md), [0049](0049-boards.md), [0059](0059-query-service.md), [0060](0060-scopes.md)

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

**A sprint is a subpage in the `Project` namespace whose content model is `triplespace-sprint`** (§2–3). Its definition (§4) names a scope, a list of rules and a window. **A task** is one (sprint, rule, subject) triple that the rule currently finds open for a member of the scope. Tasks are **computed**: `view.task` (§6) is a projection over the scope's members and the indexes each rule reads, and a task is resolved by whatever write makes the rule's condition false, whoever made it and whether or not they knew the sprint existed. Credit goes to that write's actor (§7).

The one thing a person asserts is **a claim**: "I am working on this" (§5). It is a record, because it is a statement by a person that others act on, and it has to be visible, attributable and reversible. It is the only write a sprint adds to the log. A sprint page's history is its definition's revisions and its claims.

This is the asserted/projected split of [0038](0038-page-metadata-and-categories.md) §2 and [0049](0049-boards.md) §14 once more: membership and task state are projected; the claim is asserted.

### 2. The `triplespace-sprint` model in the `Project` namespace (extends 0008 §2)

`Project` (4) gains `triplespace-sprint` among its allowed models. The default stays `wikitext`. **A `triplespace-sprint` page must be a subpage**: a title with no `/` in namespace 4, or any title in another namespace, is refused with `ts-sprint-title`. So `Project:WikiProject Women/Sprint March 2027` is a sprint; `Project:Sprint March 2027` is not; and a project's sprints sit under its page, where `Special:PrefixIndex` lists them. The restriction keeps the project page itself a document and keeps sprints findable, without a namespace.

A sprint is created by **`Special:CreateSprint`** (§9), which writes the page with the model set, or by creating a wikitext subpage and changing its model with `action=changecontentmodel` ([0041](0041-content-models.md) §8). Both are page `create` records in `pages`.

### 3. The `triplespace-sprint` content model (extends 0041 §3)

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `triplespace-sprint` | Triplespace | text | main | `application/json` | Yes | — (allowed in 4) |

A **text** model, as the table, board and scope models are ([0045](0045-table-content-model.md) §3, [0060](0060-scopes.md) §3): the definition is the page's content, revised and diffed as text. Validation checks the schema, the title rule of §2, the syntax of IDs and titles, the rule parameters (§4) and the limits; the pre-save transform canonicalizes IDs ([0044](0044-tenant-relative-ids.md) §1), titles and key order; plain text for search is the title, description and rule titles; render is the sprint page (§8).

### 4. The definition

*Changed by A1, A2.*

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
| `scope` | Yes | A scope title, or an inline kind object of [0060](0060-scopes.md) §4. The subjects the rules are evaluated over |
| `rules` | Yes | One to `sprints.max_rules` (default 20) rules, each with a `kind`, its parameters and an optional `title` shown on its tasks |
| `from`, `to` | No | The window, RFC 3339. Without `from`, the window starts at the definition's first revision; without `to`, it never ends: an open-ended sprint is a project's standing backlog |
| `board` | No | A board ([0049](0049-boards.md)) linked from the sprint page as its discussion. Without it, the sprint's own talk page serves |

**Rules.** A rule names a condition evaluated per member of the scope. Each has a **subject type** it applies to; members of the other type are skipped.

| Kind | Parameters | Open for a member when | Resolved when | Subject type | Reads |
|---|---|---|---|---|---|
| `missing-statement` | `property`; optional `value` | The member has no best-rank statement of the property, or none with that value | Such a statement exists | entity, or page with `subjects: "pages"` | `view.statement_assertion` |
| `constraint-violation` | optional `property`, `constraint_type`, `severity` | The member has at least one `view.constraint_violation` row matching the parameters. One task per (member, property), however many rows | No matching row remains | entity, or page | `view.constraint_violation` ([0031](0031-property-constraints.md) §5) |
| `missing-page` | optional `namespace` (default 0) | The member entity has no paired page in that namespace ([0038](0038-page-metadata-and-categories.md) §6) | A paired page exists | entity | the sitelink projection |
| `query` | `query` (a `Query:` title), `params` | The member is bound to `?item` in the saved query's result ([0063](0063-query-namespace.md) §6) | It is not, at the next refresh | the query's | the query service, on refresh |
| `schema-violation` | `schema` | The member's schema report says `conforms = false` ([0064](0064-entityschema-and-validation.md) §6) | It says `true` | the report's | `view.schema_report` |

A rule's parameters are validated for syntax, not existence, as a table's columns are. **Limits:** `sprints.max_rules` (20); `sprints.max_tasks` (site, default 100,000, as `scopes.max_members`): tasks are materialized in the scope's member order, rule by rule, and cut at the limit with the scope notice component of [0060](0060-scopes.md) §5 ("This sprint has more than 100,000 tasks; the first 100,000 are shown"); a truncated scope passes its notice through.

**What the rules do not cover,** on purpose: anything a person has to judge. "Needs copyediting" is a category or a page statement someone asserts, and a `category` scope with a `missing-statement` rule over a "reviewed" property expresses it; this ADR adds no free-text task.

### 5. Claims (extends 0015 §1)

**A claim is a record** of a new payload type, **`scatter:v0/task`**, in the tenant's `pages` partition, keyed to the sprint's page ID so that it appears in the sprint's history and is enclosed by the sprint page's ACLs ([0023](0023-moderation.md) §2). Three standard parts. Operations:

| Operation | Content part | Meaning |
|---|---|---|
| `claim` | `rule` (index into the definition), `subject` `{kind, id}`, optional `note` | The actor is working on the task |
| `release` | `rule`, `subject` | The actor's claim, or any claim if the actor may `edit` the sprint page and passes `force: true`, is withdrawn |

**Validation**, against the sprint's projected state: a `claim` is refused if the task is not open (`ts-task-not-open`), if it is claimed by someone else and the claim has not expired (`ts-task-claimed`), or if the actor already holds `sprints.max_claims` (default 10) live claims on the sprint; a `release` is refused if there is no live claim to release. **A claim expires** after `sprints.claim_ttl` (default 7 days) without a `release`: the projection treats it as released, and nothing is written. A task that is resolved while claimed stays credited to its resolver, not its claimant (§7); the claimant is told (§8).

**Permissions.** `claim` and `release` need `edit` on the sprint page, which is how a sprint restricts who may claim: protect the page. Anonymous and temporary accounts may not claim, since a claim is a promise with a name on it. Edit filters see a claim as a record with its content part ([0030](0030-edit-filters.md) §5). Rate class `edit`.

**Logs** (extends 0011 §6.1, §8): `task/claim` and `task/release`, projected from the records, visible to everyone; `as:Add` and `as:Remove` with `as:target` the sprint page and `as:object` the subject.

### 6. The task projection (extends 0013 §5.6 and §7)

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

**When it runs.** The task projection runs after the scope projection in step 7 of [0013](0013-postgres-storage.md) §7 and reads three kinds of change:

- **The scope changed.** Members added to the sprint's scope get a task per rule that finds them open; members removed lose their open and claimed tasks (resolved ones are kept, for credit). For a `query` scope this follows the refresh ([0060](0060-scopes.md) §5).
- **A member changed.** A write to a subject that is a member of a sprint's scope (found through `view.scope_member`'s inverted index, then `view.sprint.scope_page_id`) re-evaluates that sprint's rules for that subject: a task whose condition is now false is **resolved**, with the write's actor, time, record and revision; a resolved task whose condition is true again is **reopened**, its resolution cleared and `reopened` incremented. Constraint re-checks by the property-wide job ([0031](0031-property-constraints.md) §5) are writes by the job: a violation that disappears under a re-check resolves the task with the job as `resolved_by`, which the leaderboard (§7) does not count.
- **A claim record.** Sets or clears `claimed_by`; the state is `claimed` while a live claim exists on an open task.

All of this is inside the fan-out budget of 0013 §7 (as amended): a scope of a hundred thousand members with twenty rules is two million rows to compute when the sprint is saved, which is a job, and small per-write work after that. The sprint page shows "Tasks are being computed" until the job is done.

**What it does not do.** No activity row, no notification and no record for a task opening, resolving or reopening: these are projections of other people's writes, as [0060](0060-scopes.md) §5 says of membership. The one row a sprint adds to feeds is a claim.

### 7. Credit, and the window

**Whoever's write resolved the task is credited**, read from the record the projection was applying: `resolved_by` is that record's actor ([0007](0007-actor-identity.md) §1), `resolved_record` and `resolved_revid` point at it. There is no self-reporting and no sign-up: a person who never saw the sprint page and fixed a birth date on an item in scope counts, as the direction asks. Subsidiary accounts ([0024](0024-subsidiary-accounts.md)) are credited to the subsidiary, shown with its operator; jobs are credited to the job and not counted.

**The window frames the counting, not the computing.** Tasks are computed from the sprint's first revision, so that organizers see the backlog before the start. A resolution with `resolved_at` before `from` or after `to` is a resolved task (it is not open) but is **outside the window**, and the leaderboard and the progress figures count only resolutions inside it. The sprint page says "Starts in 3 days", "Day 12 of 31" or "Ended 2 April 2027".

**A revert takes the credit back.** When a task reopens, its resolution is cleared, and the actor's count falls. This is the correct answer for a reverted edit and the wrong one for a statement legitimately removed and re-added by someone else (the second resolver gets the credit; Q1).

### 8. The sprint page and notifications (extends 0021 §2)

*Changed by A2.*

**The page** shows, above the tabs: the description, the window and its state, a progress bar (resolved in window / total), one count per rule, and the **leaderboard**: actors by resolutions in the window, with their counts per rule, linking to their contributions filtered to the sprint's scope. Then tabs: **Tasks** (the board: Open, Claimed, Done; filter by rule, by claimant, by subject label; each task showing the subject's label and description, the rule's title, the claimant, and the rule's **action**), **Definition** (the JSON source editor), **Related changes** (the scope's, [0060](0060-scopes.md) §7), and the usual talk and history. A `board` named in the definition is linked beside the talk tab.

**Actions** open the existing editing surface positioned on the task:

| Rule | Action |
|---|---|
| `missing-statement` | The subject's page with the statement UI ([0003](0003-statement-ui.md)) adding a statement of the property, the value pre-filled when the rule names one |
| `constraint-violation` | The subject's page with the violating statement's popover open ([0031](0031-property-constraints.md) §3) |
| `missing-page` | The page editor for a new page in the rule's namespace, titled from the subject's label in the content language, with the sitelink to the subject written on save ([0038](0038-page-metadata-and-categories.md) §6), so the pairing that resolves the task is made in the same flow. A **Link existing page** alternative writes the sitelink alone |
| `query` | The subject's page |
| `schema-violation` | The subject's page at the first failing constraint, with the reason ([0064](0064-entityschema-and-validation.md) §6) |

**Claim** and **Release** buttons on each task write the records of §5. A claimed task shows who holds it and when the claim expires.

**Notifications** ([0021](0021-notifications.md) §2): one new reason, `task-resolved`, addressed to the claimant of a task that someone else's write resolved; the activity row is the resolving record, and the item carries the subject, the rule and the resolver. Nothing else notifies; a sprint's progress is read from its page and its scope's feed. The reason joins the `notifications.{reason}.{channel}` matrix of 0021 §3 under the `*` defaults.

**In a project page.** Transcluding a sprint's progress bar or task board into `Project:WikiProject Women` is the workspaces ADR's job, with tables and board listings ([0045](0045-table-content-model.md) Q5); nothing here prevents it.

### 9. `Special:CreateSprint` (extends 0047 §9)

A form, restricted to `createpage`: the parent project page (any namespace-4 page, defaulting to the page the form was opened from), the sprint's name, the scope (an existing scope by title, or a new inline `statement`/`category` kind), the rules from a picker with their parameters, the window, and an optional board to create alongside. It writes the sprint page with the model set, and the board if asked, and opens the sprint page. Listed under `pagetools`, origin `triplespace`. It is the essay's "form-based interface" for a sprint; the workspace form that also creates a scope, tables and a project page is the next ADR's.

### 10. API (extends 0012 §5)

| Area | Routes |
|---|---|
| Definition | `prop=revisions` and `action=edit`, as any text model; `action=changecontentmodel` to or from `triplespace-sprint` |
| Tasks | `GET /sprint/{pageid}` (counts, window, truncated, progress), `GET /sprint/{pageid}/tasks?state=&rule=&claimant=&after=&limit=`, `GET /sprint/{pageid}/leaderboard`, `GET /subject/{kind}/{id}/tasks` (open tasks about a subject, shown in About panels as "3 open tasks") |
| Claims | `POST /sprint/{pageid}/tasks/{rule}/{kind}/{id}/claim`, `…/release` (`force` for holders of `edit`); `list=logevents&letype=task` |
| Action API | `list=sprinttasks&stsprint=&ststate=` |

### 11. Storage, caches and search

`view.sprint` and `view.task` (§6); the initial computation as an `ops` job. Cache keys `sp:{page id}:{definition revid}` for the page's counts and leaderboard, purged when any task of the sprint changes state, which is the one hot key here and is cached at L1 for `sprints.cache_ttl` (default 30 s) to absorb a busy drive. Search indexes the plain text of §3; tasks are not indexed.

### 12. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-tasks` | **New, layer 2, pure, wasm.** The `triplespace-sprint` definition: schema, validation, canonicalization; the `scatter:v0/task` payload type, its operations and the fold to claim state with expiry; each rule as a pure predicate over the facts it is given (statements, violations, pairing) |
| `triplespace-projections` | `view.sprint` and `view.task`; the task projection of §6 after the scope projection; the initial computation job; credit and the window |
| `triplespace-api-rest`, `triplespace-api-action` | The `/sprint` routes, `/subject/…/tasks`, the claim routes; `list=sprinttasks` |
| `triplespace-ui` | The sprint page, task board, leaderboard and actions; `Special:CreateSprint`; "open tasks" in About panels |
| `triplespace-notify` | The `task-resolved` reason |

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
- **Test plan.** Rules: each of the three against fixtures with the condition true and false, and against page subjects where allowed. Projection: a statement write resolves a task within the same write; a revert reopens it and clears the credit; a scope change opens and drops tasks; a claim, an expired claim and a `force` release. Window: resolutions before, inside and after count as specified. Load: a 100,000-member scope with 20 rules computes as a job and the page reports progress.

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
