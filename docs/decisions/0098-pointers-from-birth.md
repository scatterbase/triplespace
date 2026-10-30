# 0098. Pointers from birth

- **Status:** Proposed
- **Date:** 2026-10-30
- **Updated:** 2026-10-30 (A3)
- **Author:** James Hare / Claude Fable
- **Changes:** [0050](0050-adr-format.md)
- **Uses:** [0005](0005-crate-organization.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0016](0016-permissions-and-access-control.md), [0085](0085-settings-and-preferences.md)

## Context

[0050](0050-adr-format.md) §14 moved the current text of every ADR into the architecture chapters on 2026-10-09 and made each Decision section a pointer. It kept one exception: a new ADR carried its full Decision until a chapter absorbed it, and relocation was a separate step after the commit, because the `relocates` entry named the commit that last carried the text.

Three weeks later the exception had become the rule. The nineteen ADRs written since (0079–0097, 500 KB) all carried their text, and all of it was also folded into the chapters: 269 chapter provenance lines cited them. Every amendment to one of them was written twice (0086 A7, 0095 A3 and 0077 A3 in the last commit before this ADR), and no checker could tell when the two copies drifted, which was the state the chapters had been built to end. Relocation never ran because it waited on a commit the agent could not make from the device bridge.

A review of `docs/` on 2026-10-30 also counted the bookkeeping of one amendment: nine places, five of them derivable from the other four. The `Chapters` header restated the pointers; the "Changes to other ADRs" table restated the targets' log entries, which `INDEX.md` already inverted; `MAP.md` and the `HOME` and `OVERRIDES` tables of `map.py` restated the chapters' provenance lines, from which `pointers.py` already built the pointers. The two generated files, 85 KB and 78 KB, churned on every ADR commit. And the largest chapter sections were catalogues, not prose: the crate map (113 KB), the `view` schema (87 KB), the Action API (57 KB) and the settings catalogue (53 KB, the one already generated from a registry file).

0050 itself was process, had no chapter, and its rules were restated a third time in `docs/architecture/README.md`.

### Direction

James's direction, 2026-10-30, on the review:

- **"New policy that ADRs are written as pointers to the chapter sections they change. Please convert the ones that aren't compliant."**
- **"Information should have one source of truth."**
- **"Catalogues should be kept out of prose."**
- Open to reforming `INDEX.md`.
- On where 0050's own text should live: a new chapter 24, *Documentation*, absorbing 0050's Decision and the README's conventions, so that 0050 and this ADR are pointers like every other.

## Decision

### 1. Every Decision section is a pointer from birth (amends 0050 §5, §7, §14)

*Changed by A1.*

*Current text: [24](../architecture/24-documentation.md) §1, §3.2, §5.*

### 2. The chapters' provenance lines are the map (amends 0050 §14)

*Current text: [24](../architecture/24-documentation.md) §10.2, §12.*

### 3. The derivable header field and section are retired (amends 0050 §3, §4, §6, §12)

*Current text: [24](../architecture/24-documentation.md) §2.2, §2.3, §12.*

### 4. The index is one slim table (amends 0050 §12)

*Current text: [24](../architecture/24-documentation.md) §12, §13.*

### 5. Catalogues are registry data (extends 0050 §6; uses 0005 §2–3, 0011 §6, 0012 §4, §5, 0016 §2)

*Changed by A1, A3.*

*Current text: [24](../architecture/24-documentation.md) §11.*

### 6. Chapter 24 holds the process text (extends 0050 §14)

*Current text: [24](../architecture/24-documentation.md) §14.*

### 7. The conversion (extends 0050 §13)

*Current text: [24](../architecture/24-documentation.md) §14.*

## Alternatives considered

- **Keep full text in new ADRs and make relocation reliable** (run `pointers.py` as the last commit of every ADR branch, or fail CI when a merged ADR still carries text a chapter holds). Rejected: it keeps two copies alive for the life of a branch, and the reviewer still reads the chapter diff, since that is where the text is folded.
- **Keep the "Changes to other ADRs" table as the reviewer's checklist.** Its rows are the targets' log entries, verbatim; the section headings carry the same verbs and targets; and `INDEX.md` inverts the logs. Three copies of one relation. Rejected.
- **Move Amendment logs to sibling files** to halve ADR sizes. Rejected: the log is the one part of the record that must travel with the file, and `tools/adr.py log` already keeps it out of the read path.
- **Generate `INDEX.md` in CI and stop committing it.** Rejected by the direction of 2026-10-01 (0050 Context): the index is committed. Slimming it removes most of the churn.
- **Leave 0050 as the one ADR with text,** since it is process. Rejected (Direction): one rule with no exceptions is cheaper to check and to explain, and the README's restatement of 0050 was a third copy.

## Consequences

- **One amendment touches four places**, not nine: the chapter text and its provenance line, the target's log entry, the target's provenance line, and the amender's `Changes` header. `check.sh --write` regenerates the index.
- **The ADR carries no second copy of anything.** The chapters are the only current text; `git` and the Amendment log are the only history.
- **A new ADR is reviewed as a chapter diff**, spread over the chapters it touches. `tools/adr.py show NNNN` reads it back as one document. Reviewers who want the proposal in one piece read that, not the ADR file.
- **`MAP.md`, the `HOME` and `OVERRIDES` tables and the `Chapters` header are gone.** `map.py` derives everything from the provenance lines, and `pointers.py` writes pointer lines from them without logging.
- **`INDEX.md` shrinks from 78 KB to a fifth of that** and a row changes only when its ADR's status, date, chapters or amenders change.
- **The catalogues still have to move.** §5 states the rule; each table moves when its registry file and generator exist. Until then the chapter table is the authority, and chapters 22, 03 and 18 stay large.
- **Older log entries still say `relocates`, and 0050's older text still describes `MAP.md` and the `Chapters` header.** They are history: chapter 24 §14 says what happened, so a reader of 0013 A51 can follow it.

## Open questions

- **Q1.** ~~**Which catalogue moves first, and who reads it.** The crate map is read by `cargo xtask deps` from chapter 22 §2.1 today; moving it to `registry/crates.toml` changes that reader. The `view` schema, the route table and the permission catalogue have no code reader yet. Order and file names are open.~~ *Settled by A1: the crate map first, then permissions and log events; routes from OpenAPI; the `view` schema stays in prose for now (24 §11).*
- **Q2.** ~~**`Updated`.** The header's `Updated` restates the last log entry's ID and date, and the checker holds them equal. It is the one derivable field this ADR keeps, because it is the first thing a reader looks at. Whether it goes too is open.~~ *Settled by A1: kept; it is checker-verified and the first thing a reader sees.*
- **Q3.** ~~**Review of born-as-pointer ADRs.** Whether a pull request that adds an ADR should carry the assembled text (`adr.py show NNNN`) in its description, so that a reviewer on GitHub reads one document.~~ *Settled by A1: CI posts the assembled ADR (`adr.py show NNNN --full`) as a pull-request comment (24 §3.2).*

## References

- `docs/architecture/24-documentation.md`, the current text of this ADR and of 0050
- `docs/architecture/map.py`, `docs/architecture/pointers.py`, `docs/decisions/check_adrs.py`, as changed by this ADR
- `docs/decisions/INDEX.md`, generated by `check_adrs.py --index`
- `docs/architecture/settings.py` and `docs/registry/settings.toml`, the pattern §5 generalises ([0085](0085-settings-and-preferences.md) §1)
- The review of 2026-10-30: 98 ADRs, 2.4 MB, 50% Amendment log; 24 chapters, 2.4 MB; 884 of 1,025 Decision sections pointers before this ADR, 1,025 after

## Amendment log

### A1. The open questions settled

- **Date:** 2026-10-30
- **Source:** Direct: James, review of 2026-10-30
- **Change:** extends §1, §5
- **Summary:** Q1: the crate map moves to the registry first (`registry/crates.toml`, read by `cargo xtask deps`), then the permission and log-event catalogues; the REST route table is generated from the OpenAPI file; the `view` schema stays in prose until it has a reader. Q2: `Updated` is kept. Q3: CI posts the ADR assembled with its chapter text (`tools/adr.py show NNNN --full`, new) as a comment on any pull request that adds an ADR, updated on every push.

### A2. Four catalogues moved

- **Date:** 2026-10-30
- **Source:** Direct: James, review of 2026-10-30 (the order of A1, carried out)
- **Change:** consolidates §5
- **Summary:** 24 §11 now names the registry file behind each catalogue that has moved (the crate map, the REST routes, the permissions, the log events: 0005 A111, 0012 A82, 0016 A40, 0011 A35) and the two still in prose. The decision is unchanged. The last commit before this entry is `0e67cce`.

Replaced text ([24](../architecture/24-documentation.md) §11, as it stood):

> **A catalogue is registry data, not prose.** A table whose rows are entries rather than argument — the crate map (22 §2.1), the `view` schema (03 §4), the REST routes and Action API modules (18 §2, §3), the permission catalogue (09), the log-event catalogue (16) — belongs in a registry file with a generated chapter table, so that the chapter stays readable and the code can read the same file. Each move is a `consolidates` entry in the ADRs whose sections the table cites: no decision changes. Until a catalogue has moved, its chapter table is the authority. The order: the crate map first (`registry/crates.toml`; `cargo xtask deps` reads the file rather than the chapter table), then the permission and log-event catalogues as registry files; the REST route table is generated from the OpenAPI file in `../api/` rather than a new registry file; the `view` schema stays in prose until it has a reader, since the migrations are already its source.

### A3. What a catalogue move logs

- **Date:** 2026-10-30
- **Source:** Direct: James, review of 2026-10-30 (carrying out A1; the logging rule is Claude's practice from the four moves, for James to confirm)
- **Change:** amends §5
- **Summary:** 24 §11 no longer names each moved catalogue or an order (the registry README lists the files and the section each generates into; the Action API modules moved too, 0012 A83), and it says what a move logs: one `consolidates` entry in the ADR that owns the chapter section, quoting any chapter prose the move rewrote and naming the last commit that carried the table, rather than an entry in every ADR the table cites. That is what the five moves did (0005 A111, 0012 A82–A83, 0016 A40, 0011 A35).

Replaced text ([24](../architecture/24-documentation.md) §11, as it stood after A2):

> **A catalogue is registry data, not prose.** A table whose rows are entries rather than argument — the crate map (22 §2.1, `crates.toml`), the REST routes (18 §3.2, `routes.toml`, checked against the OpenAPI contract), the permission catalogue (09 §2.2, `permissions.toml` with `groups.toml`), the log-event catalogue (16 §2.3, `log-events.toml`), and still to move, the Action API modules (18 §2) and the `view` schema (03 §4) — belongs in a registry file with a generated chapter table, so that the chapter stays readable and the code can read the same file. Each move is a `consolidates` entry in the ADRs whose sections the table cites: no decision changes. Until a catalogue has moved, its chapter table is the authority. The order: the crate map first (`registry/crates.toml`; `cargo xtask deps` reads the file rather than the chapter table), then the permission and log-event catalogues as registry files; the REST route table is generated from the OpenAPI file in `../api/` rather than a new registry file; the `view` schema stays in prose until it has a reader, since the migrations are already its source.
