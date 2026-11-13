# 0104. Batches and the job runner

- **Status:** Proposed
- **Date:** 2026-11-08
- **Updated:** 2026-11-13 (A10)
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

*Changed by A1, A6, A7, A8, A9, A10.*

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

*Changed by A3, A4, A5.*

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

### A4. A grid, drafts, matches, references between rows and new properties

- **Date:** 2026-11-08
- **Source:** [0106](0106-create-entities-grid-and-drafts.md) §1, §2, §4, §5, §7, §8, §10
- **Change:** amends §11
- **Summary:** `Special:CreateEntities` keeps the pasted table as its canonical form and gains a grid over it, a mapping step for uploaded files, and drafts kept as Table pages with staged rows. The table's text conventions add `id`, `new:` property columns, `P…/ref:` reference columns, `row:n` references between rows, `|`-separated values, and a reference on every statement that may read a column. A row that matches an existing item by a match key or by label and description adds to it; properties a table declares are created first, by holders of `property-create` only; rows are written in the order their references require; a statement's own reference replaces the default. Of what §11 left out, match keys, references between rows, updating (matched) items and properties are taken; qualifiers and Lua column transforms are not.

Replaced text ([21](../architecture/21-special-pages.md) §6.6, as it stood):

> - **Preview first.** Nothing is written until the rows have been checked: the properties are looked up in one call, each cell parsed against its property's data type, and each row shown with what it holds and its problems; a row with no term at all, a cell that is not a value of its type, or a property that does not exist marks the row, which is left out. At most 500 rows a run; a larger table is split.

> - **Not taken yet** ([0104](../decisions/0104-batches-and-the-job-runner.md) §11): match keys, so every row creates (a re-run creates again); references from one row to an item another row creates; updating existing items; properties and other entity types. A table larger than a run allows waits on the queued batch of [05](../architecture/05-providers-and-ingest.md) §3.5.

Replaced text ([05](../architecture/05-providers-and-ingest.md) §3.5, as it stood):

> - **A table of new items** ([0104](../decisions/0104-batches-and-the-job-runner.md) §11; [21](../architecture/21-special-pages.md) §6.6) is the editor's form of bulk creation: `Special:CreateEntities` reads a pasted or uploaded table, a column per term or property and a row per item, checks every row before anything is written, and creates the rows as one keyed batch ([16](../architecture/16-logs-feeds-and-notifications.md) §2.2), one interactive `create` each, so that the run is one job with its revert.

### A5. The page is named `Special:CreateManyEntities`

- **Date:** 2026-11-10
- **Source:** Direct: James, 2026-11-10 (review of SCA-307)
- **Change:** amends §11
- **Summary:** The page that creates items from a table is `Special:CreateManyEntities`, which says what it is for beside `Special:NewItem`. `CreateEntities` stays in the special-page registry as an alias: a GET of it is sent on to the new name with its subpage, so links and drafts' addresses written before the change still work. Earlier ADR text and log entries keep the name they were written with.

Replaced text ([21](../architecture/21-special-pages.md) §6.6, as it stood):

> ### 6.6 `Special:CreateEntities`

### A6. Dumps join the queue, and a runner claims only its kinds

- **Date:** 2026-11-12
- **Source:** Direct: Claude, while building the dump job (SCA-309), 2026-11-12; unconfirmed
- **Change:** extends §4
- **Summary:** §4 has the runner run a dump, and A1 carried `import` alone, then `revert`. The calls made: (1) **`dump` is a kind of the queue**: its row names the tenant and, where it is not the tenant's `dumps.keep`, `keep`; the runner writes the job's `job/start` record in the tenant's `log`, with source `dump`, writes the tenant's dumps into `{dumps.dir}/{tenant}/` exactly as `triplespace dump` does, which stays, and writes `job/finish` naming the dump, its instant, its entity counts and each file with its size and digest; a dump that fails leaves no directory and gets its `job/fail`; (2) it is submitted by `triplespace job start dump --tenant {slug}`; **no route submits one and nothing schedules one**, since who may ask for a dump and how often one is made are not decided; (3) **a runner names the kinds it runs and the claim asks for those alone**: a row of a kind the runner does not run is never the row it looks at, where before such a row, once the oldest, kept every runner from every younger row; `triplespace-server`'s runners run `import`, `revert` and, into `--dumps-dir`, `dump`; this is also what will let adoption have a runner of its own ([0013](0013-postgres-storage.md) A102); (4) `triplespace-api-ingest` depends on `triplespace-rdf` for the dump, an edge the crate map already allows; (5) **still not carried**: an export over `export.sync_revisions`, which waits on where a job's output is kept, for how long and who may fetch it; and the adoption, sync and follower jobs, which the command line runs: each reads a dump file or holds a session where the command runs, and how a server is given one is not decided.

### A7. Claims are one per partition and capped, and a job that fails before it starts is a failed job

- **Date:** 2026-11-13
- **Source:** Direct: Claude, after a review of the job queue (2026-11-13); unconfirmed
- **Change:** extends §4
- **Summary:** A review of the queue found three defects and two risks in what A1 built, and these are the calls made in mending them. (1) **One running job per partition is enforced between claimers.** The row lock alone did not: a claimer skipped the row another had locked and not yet committed as claimed, saw no running job in the partition, and claimed a second row of it, with `--job-runners` above 1 or two processes. A claimer now decides for a partition only under a transaction-scoped advisory lock on it, taken without waiting; the statement after the lock sees every claim of the partition committed before it, and a claimer that finds the lock taken passes on to the next partition with a job waiting, where before it had one candidate row and answered that nothing waited. (2) **A job is claimed at most `jobs.max_attempts` times**, a new instance setting, 3 by default. `attempts` was counted and never read, so a job that took its process down was run again every two minutes for ever. A stale claim on a job that has had its attempts is not run: the dead run gets its `job/interrupt` as before, naming as `by` a new job ID under which a start and a `job/fail` record are written, or, where the dead run wrote no start record, the start and fail records are written under the ID the job was submitted with; the row is removed. The error names the setting. (3) **An ID answered at submission is always a job.** A job that failed before its kind's work wrote the start record (a file that is not XML, a file with nothing this wiki can import, a revert of a job that does not exist) left no record, and its page answered 404 for an ID the caller had been given. The runner now writes a start record for it from the queue row (an import: the row's source and version, mode `import`, graph `pages`, the summary; a revert: source `revert`, mode `revert`, the job reverted and the reason; a dump: source `dump`), and then the `job/fail`, so the job shows as `failed` with its error. (4) **A runner whose claim was taken says so and writes nothing more to the queue for the row.** A heartbeat that updates no row means the row was claimed again under a new ID or removed; the runner logs it, and at the job's end, where the row is no longer its own, writes neither a fail record nor the row's deletion. The work itself is not stopped: nothing here can cancel a running job, so a run whose heartbeat stalled for two minutes may still write beside the run that replaced it, as before; the jobs carried are idempotent, which is what that leans on. A heartbeat that could not be written is logged, where it was dropped. (5) **A pass that cannot read `jobs.paused` claims nothing**, where it had run as if not paused. Not changed: an import's talk conversion runs after the job's finish record, so a store error there leaves the job `finished` with the error in the server's log alone; mending it means writing the finish record after the conversion in the import shared by the command line, the request path and the runner, which is left for a decision. Chapter text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.2; [23](../architecture/23-configuration-and-registry.md) (`settings.toml`).

### A8. A job that lost its claim is stopped, and an import's talk pages are its own work

- **Date:** 2026-11-13
- **Source:** Direct: James, 2026-11-13, asked that the two things A7 left be fixed; the calls in how are Claude's, while fixing them; unconfirmed
- **Change:** amends §4
- **Summary:** A7 left two things. (1) **A job whose claim was taken is stopped.** The runner's heartbeat, on finding the row no longer its job's, ends the work where it stands; before, the work ran on beside the run that had taken the claim, each checking what the other had written in transactions of its own. What the stopped run had committed stays, and the other run goes on from it, as after a runner's death. (2) What made stopping safe: **a unit of work that is neither committed nor rolled back closes its connection**, where it had gone back to the pool still inside its transaction, for the next unit of work to run inside and commit with its own; this held for every unit of work an error returned past, not only for a stopped job. (3) A dump's files are written on a thread that is not stopped: it finishes them, and the stopped job records nothing of them; the run that took the claim writes the dump again. (4) **An import's talk pages are converted within the job**: after its pages, since a thread needs its subject page and authors, and before its finish record, which now carries the conversion's counts as `talk`; a conversion that cannot be written fails the job, with its `job/fail`, where it had left the job `finished` and the error in the server's log. This holds for `action=import`, the runner and `triplespace import` alike; an adoption converts after its job, as before. (5) A talk page whose unit of work is rolled back no longer leaves the surrogates it minted in the conversion's memory, where a later page would have attributed posts to an actor with no record.

Replaced text ([16](../architecture/16-logs-feeds-and-notifications.md) §2.2, as it stood):

> says so in the server's log and writes nothing more to the queue about the row, which is the other runner's; its work is not stopped (A7).

### A9. Exports, adoptions, syncs and followers join the queue

- **Date:** 2026-11-13
- **Source:** Direct: James, 2026-11-13, chose, of the options put to him: the blob store for a queued export's file; its submitter alone to fetch it, for seven days; a path on the server for an adoption's or a sync's dump; and one pass a row for a follower. The rest are Claude's calls, while building them (SCA-309); unconfirmed
- **Change:** extends §4
- **Summary:** A6 (5) left four kinds uncarried, each waiting on a decision. With James's four: (1) **the work of a kind may live above the queue's crate**: a runner is given it by the process that runs it, and claims the kind only then; the export's work is the REST API's, which reads a request as `POST /export` does, and the adoption's, the sync's and the follower's are the command line's own functions, run from a row whose parameters are the command's arguments; (2) **`export`**: `POST /export` over `export.sync_revisions` from a signed-in account on a server with a blob store answers `202` with the job and the URL of its file, where it answered `413`; the job reads as its submitter may read **when it runs**, not when it was asked for, and fails if the account is gone or may no longer read; it streams the XML, or writes the `records` document, to `{tenant}/job/{id}/` ([0039](0039-files-and-media.md) A16) and names the file, its size, what was missing and its notices in the finish record, which the job projection keeps; (3) **`GET /export/{id}`** answers the account that asked, and to anyone else that there is no such export: `202` while the job waits or runs, the file when it has finished, its error when it failed, and `410` once the file is older than **`export.keep_days`**, a tenant setting, 7 by default; the site serves the same at `Special:Export?job={id}`; (4) **an account has at most five exports queued or running** on a tenant; `POST /export` takes no token, as before, and is counted against no rate class, which is left as it was: whether a queued export should need a token, since a job is a write of a kind, is not decided; (5) **`adopt` and `sync`** are queued by an operator with `triplespace job start adopt` or `sync`, the dump a path the server process must be able to read; no route queues one; each runs under the ID the queue minted, which the job's header now carries to its start record; (6) **`follow`**: a row runs one pass and is put back, unclaimed, its attempts counted afresh, not to be claimed before `follow.interval` (or the row's own interval) has passed, which `ops.job_queue.not_before` holds; cancelling the row ends the following; the follower's records name its own daily job (0092 A6), not the row, whose ID `Special:Jobs` shows as queued for as long as the follower runs; (7) **followers have a runner of their own on the accounts login** ([0013](0013-postgres-storage.md) A103); (8) work whose future cannot cross threads runs on a thread of its own and **is stopped there when its claim is lost**, which now holds for a dump too, where A8 (3) left its thread running; (9) the server takes `--blob-dir` (`TRIPLESPACE_BLOB_DIR`), the `fs` backend's directory, which every replica and runner must share. Not built: a route or a schedule that queues a dump, an adoption or a sync, and stopping a follower from the site.

### A10. Queuing an export needs the session's token

- **Date:** 2026-11-13
- **Source:** Direct: James, 2026-11-13: "Require a token to queue an export."
- **Change:** extends §4
- **Summary:** A9 (4) left open whether queuing an export should need a token. James decided that it does. `POST /export` is still a read where it answers in place, and takes no token then; where the selection is over `export.sync_revisions` and would be queued, a request made with a session must carry the session's CSRF token as `token` in the body, as every write with a session does, and is refused `403 badtoken` without it, nothing queued; a request made with a bearer key needs none. The token is the request's and is not kept in the queue row. `Special:Export`'s form carries the token of a reader who is signed in, as `wpEditToken`, and passes it on; an anonymous reader's form carries none, and stays as cacheable as it was.
