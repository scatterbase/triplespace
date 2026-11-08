# 0104. Batches and the job runner

- **Status:** Proposed
- **Date:** 2026-11-08
- **Updated:** 2026-11-08 (A3)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0016](0016-permissions-and-access-control.md), [0047](0047-special-pages.md), [0083](0083-write-path-in-three-tiers.md)
- **Uses:** [0007](0007-actor-identity.md), [0024](0024-subsidiary-accounts.md), [0030](0030-edit-filters.md), [0047](0047-special-pages.md)

## Context

By 2026-11-08 every ingest run was a job in the log ([0002](0002-source-graphs-and-mass-ingest.md) §8.3; [0011](0011-logs.md) §6.3): a start record keyed by an instance-minted ID, records carrying the ID in their attestation, the `job:{id}` tag on activity rows, a feed filter by job, and `Special:Jobs/{id}` designed in [0010](0010-site-ui.md) §9 with **Revert this job…** among its actions. But no job runner existed: jobs ran only where the command line ran them, the import and export of [0008](0008-namespaces-and-document-pages.md) §9 and [0047](0047-special-pages.md) §8 ran on the request path and refused anything over a threshold, `Special:Jobs` was named and not built, and nothing reverted a job. Three tickets waited on that: the runner itself, editors' bulk edits through the ingest API with a review page, and bulk creation from a table or a CSV file, with QuickStatements syntax as a further input.

Wikidata's tools solved a related problem outside the wiki: an **edit group** ([Wikidata:Edit groups](https://www.wikidata.org/wiki/Wikidata:Edit_groups)) is a batch ID a tool writes into each edit summary, which a separate service indexes so that a batch can be found, reviewed and undone as one. The ID is the tool's own; nothing stops anyone from writing someone else's into a summary.

### Direction

James's direction, from the design discussion of 2026-11-08:

- **"Users submitting batch jobs \[should\] include their own batch IDs. The idea is to implement Wikidata:Edit groups as metadata built into the job. Batches are keyed by ID and an arbitrary value provided by the user. It can be anything so long as it uniquely identifies the batch of work. Since it's a potential vandalism vector it's hashed before being persisted in the log. This would allow bot operators and similar to identify their own contributions without waiting on being assigned an identifier. A batch can be rejected, at which point further edits with that batch ID are blocked, and existing ones may be reverted with one click."**
- **"Batch values are keyed by operator. If an identical string is used by two different operators, they are different batches."**
- **"Batch keys can be accepted on any write."**
- Rejection lives in the write path as a job state; a revert appends per-record inverses, attributed to the reverter, and skips a record changed since; both agreed on 2026-11-08.

## Decision

### 1. A batch is a job the submitter keys (amends 0002 §8.3; extends 0011 §6.3)

*Changed by A2.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.3; [16](../architecture/16-logs-feeds-and-notifications.md) §2.2.*

### 2. Rejecting a batch (extends 0011 §6.3)

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.2.*

### 3. Reverting a job, record by record (amends 0002 §8.3 and 0010 §9)

*Changed by A2.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.3; [19](../architecture/19-site-ui.md) §3.10.*

### 4. The runner (extends 0083 §5)

*Changed by A1.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.2; [21](../architecture/21-special-pages.md) §4.3.*

### 5. Permissions (extends 0016 §2)

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2.*

### 6. API (extends 0012 §4 and §5)

*Changed by A2.*

*Current text: [18](../architecture/18-api.md) §2.3, §3.2.*

### 7. Log events (extends 0011 §6.1)

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 8. Storage (extends 0013 §5.6)

*Changed by A1, A2.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.12.*

### 9. UI (extends 0010 §9)

*Current text: [19](../architecture/19-site-ui.md) §3.10.*

### 10. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 11. Creating items from a table (extends 0002 §8.5 and 0047 §5)

*Changed by A3.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.5; [21](../architecture/21-special-pages.md) §6.6.*

## Alternatives considered

- **A change tag `batch:{hash}` instead of a job.** Tags already exist, the feed filters by them and a filter could match one; but a batch also wants a page, a state and a revert, which are a job's, and a second mechanism beside jobs would be a parallel concept where one suffices.
- **Hashing the value alone.** Anyone could then submit under another operator's value and be reverted with it, or pass for their bot. Binding the hash to the operator makes the same string two batches under two operators, which is what James chose.
- **An edit filter as the rejection.** A rejected batch is a fact about one job; a filter rule naming a hash would be a second record of it, and filters are a tenant's policy over content, not the moderation of one job.
- **Reverting a job as one inverse change set.** Per-record inverses let a record someone has edited since be skipped and listed, as 0010 §9 already asked.
- **A separate worker binary.** 0083 §5's "one worker per partition" reads naturally as a process; but one binary with the worker as background tasks is simpler to run, and a second binary can host the same tasks later without a change to the jobs.

## Consequences

- **A bot operator needs no registry to be found.** Their batches are theirs by construction, and the raw value never reaches the log.
- **A batch is a moderation unit.** Rejecting one stops it in the write path and reverts it in one action, record by record.
- **Imports and exports leave the request path** once the runner carries them, and the thresholds of [0047](0047-special-pages.md) §8 become the line between streaming and a job.
- **One more state on a job.** `rejected` and `closed` join `running`, `finished`, `failed` and `interrupted`.

## Open questions

- **Q1. Reverting page and thread records.** A page revision reverts by restoring the previous text as an edit; a post in a thread has no inverse but hiding its text, as Nuke does ([0047](0047-special-pages.md) §7). Whether a batch revert should hide posts, or list them.
- **Q2. Idle batches.** A keyed batch stays open until its submitter closes it or it is rejected. Whether an idle batch should close itself after a period, so that `Special:Jobs` does not fill with open batches.
- **Q3. The queue beyond one process.** The runner claims queued jobs with the database's row locks, so several server processes could share the queue; nothing partitions the work between them yet, and a job that one process started is finished by that process or found interrupted.

## References

- [Wikidata:Edit groups](https://www.wikidata.org/wiki/Wikidata:Edit_groups)
- [0002](0002-source-graphs-and-mass-ingest.md) §8.3, §8.5; [0011](0011-logs.md) §6.3; [0010](0010-site-ui.md) §9; [0083](0083-write-path-in-three-tiers.md) §5

## Amendment log

### A1. Calls made while building the runner

- **Date:** 2026-11-08
- **Source:** Direct: Claude, while building the job runner (SCA-172), 2026-11-08; unconfirmed
- **Change:** extends §4 and §8
- **Summary:** The line between the request path and a job for an import is a tenant setting, `import.sync_pages`, 100 pages by default, beside `export.sync_revisions`; the runner is paused by an instance setting, `jobs.paused`, which `triplespace job pause` and `resume` set. The queue row carries the upload (`payload`), the job's actor, a `heartbeat` the runner refreshes, the running job's `progress` and a `cancel` flag beside the columns §8 named; a claim whose heartbeat is two minutes stale is a dead run, which the next claim marks `job/interrupt`, naming as `by` the new job ID the work runs again under at once, since the jobs here are idempotent; a job that fails on its input gets its `job/fail` record and its row is deleted, not retried. The runner carries `import` in this phase: the follower, sync and adoption jobs still run where the command line runs them, which coverage shows against §4. A job's end is delivered to its actor's inbox, or its operator's, as 16 §5.2 lists under `job`, the one notification an account gets of its own action. Chapter text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.2; [03](../architecture/03-storage-caches-and-search.md) §4.12; [23](../architecture/23-configuration-and-registry.md) (`settings.toml`).

### A2. Calls made while building the batches

- **Date:** 2026-11-08
- **Source:** Direct: Claude, while building the batches (0104 §1–§3), 2026-11-08; unconfirmed
- **Change:** extends §1, §3, §6 and §8
- **Summary:** (1) `batch=` is taken by the entity write modules, `action=edit`, `action=move` and `action=delete`, and by the thread create and post routes as a `batch` field; the response carries the job as `batch`. The other writes (uploads, ACLs, settings, watches) do not take it yet, which coverage shows against §6. (2) The batch's job is opened in the write's own transaction, with `batch`, the hash, and `operator` in its parameters, source `batch` and graph `local` whatever it writes; `view.job` gains a `batch` column with an index for the write that looks the hash up. (3) A revert runs under the runner as the kind `revert`, so `POST /jobs/{id}/revert` answers with the queued revert job, and the records skipped are listed on that job's page, not in the response as §6 had it from 0012 §5; a running job is rejected or closed before it is reverted, and a rejection may queue the revert in the same call. (4) The inverses built: an entity record's state before it restored by the diff that interactive edits use, moved into `triplespace-api-ingest`; the deletion of an entity or page the job created; a page revision's previous text as an edit. A move, a deletion and a thread post are listed as having no inverse yet (Q1), as is a page's content-model change. A revert's `job/revert` on the reverted job names the revert as `reverted_by` and how many records were skipped. (5) A rejection notifies the job's owner as the `job` reason does its end. Chapter text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.2; [05](../architecture/05-providers-and-ingest.md) §3.3; [03](../architecture/03-storage-caches-and-search.md) §4.1; [18](../architecture/18-api.md) §3.2 (`routes.toml`).

### A3. Calls made while building `Special:CreateEntities`

- **Date:** 2026-11-08
- **Source:** Direct: Claude, while building the table-based creation of items (SCA-277, SCA-281), 2026-11-08; unconfirmed
- **Change:** extends §11
- **Summary:** The table's columns are named by header convention rather than a mapping step: `label`, `description`, `aliases` with a language after a colon, else the wiki's; a property by ID. A cell is parsed in the plain forms the form takes (a number, a year or date, `text@lang`, `lat, lon`), shared with `Special:NewItem`, since `wbparsevalue` is not built. The rows are created as interactive writes under a keyed batch, one `wbeditentity` each, not as a queued `stream` job, so the report can name each row's ID at once; at most 500 rows a run. Match keys, references between rows, updating existing items and other entity types are left for the queued path. Chapter text: [21](../architecture/21-special-pages.md) §6.6; [05](../architecture/05-providers-and-ingest.md) §3.5.
