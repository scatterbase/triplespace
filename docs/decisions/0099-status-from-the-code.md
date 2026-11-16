# 0099. Status from the code

- **Status:** Proposed
- **Date:** 2026-10-30
- **Updated:** 2026-11-09 (A3)
- **Author:** James Hare / Claude Fable
- **Changes:** [0050](0050-adr-format.md)
- **Uses:** [0098](0098-pointers-from-birth.md)

## Context

[0050](0050-adr-format.md) §3 made an ADR `Accepted` "once it is implemented in full" and said "the change that completes the implementation sets the status". Nothing did. Three weeks and 99 ADRs later every one was `Proposed`, though the code implemented parts of dozens and all of a few, because setting the status meant someone deciding, by hand, that a section was complete, and then keeping three places in step: the header, the Project's `implementation-backlog.md` and its `open-decisions.md`.

The code already carried most of the evidence. `tools/coverage.py` read the `0013 §7` citations in comments and classed each as src, test, or "not built yet"; at 2026-10-30 it counted 768 top-level Decision sections, 108 cited by code and tests, 215 by code only, 346 by nothing. But a citation says only that the code is *about* a section. Completion had no mark, so the status could not be computed, and the backlog was a hand-written list of what the citations did not show.

The Project notes had the same shape: `implementation-backlog.md` was 60 KB of what was built and what remained, by tier and patch file, and `open-decisions.md` held some hundred "calls made while building, for James to confirm or overturn", decisions Claude made in code and recorded nowhere in the ADRs, though [0050](0050-adr-format.md) §1 says a section Claude drafted is decided until a later change says otherwise.

### Direction

James's direction, 2026-10-30, after the catalogue moves:

- **"Rather than have to synchronize states, the ideal is that the code carries the implementation status of the decision it encodes."**
- On what counts as implemented in full: an `implements` claim in src **and** a test citing the section.
- On the Status header: generated from the code, not checked against it.
- On the Project notes: retire both in favour of generated lists, keeping only the gotchas; the existing "confirm or overturn" calls are logged as unconfirmed entries in their ADRs.

## Decision

### 1. The code claims what it implements (amends 0050 §11)

*Changed by A3.*

*Current text: [24](../architecture/24-documentation.md) §9, §15.*

### 2. The status is written from the code (amends 0050 §3, §12)

*Changed by A2.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1; [24](../architecture/24-documentation.md) §2.2, §12, §15.*

### 3. The index is the status page (amends 0050 §12)

*Changed by A1.*

*Current text: [24](../architecture/24-documentation.md) §13, §15.*

### 4. Decisions made while building are logged, unconfirmed (amends 0050 §8)

*Current text: [24](../architecture/24-documentation.md) §6.*

### 5. The working notes are retired (extends 0050 §13)

*Changed by A2.*

*Current text: [24](../architecture/24-documentation.md) §14, §15.*

## Alternatives considered

- **A test citing the section as the only completion mark.** No new comment form, and the strongest evidence. Rejected as the sole mark (Direction): the `implements` line is where the writer asserts that they checked the section's text against the code, which a test's citation does not say; both are required.
- **An `implements` claim without the entry ID.** Simpler to write, but a later log entry would leave the claim silently wrong; with the ID, `coverage.py` reports the section as *amended* and the status names the entries (0050 §3 already had that state, by hand).
- **A per-section status table kept in the ADR or the chapter.** The thing the direction rules out: a second place to keep in step.
- **Checking the header against the code instead of writing it.** Keeps the header human-written, at the price of every status change being two edits. Rejected (Direction).
- **Keeping `implementation-backlog.md` and generating only its status sections.** Rejected (Direction): once the lists are derived, what remains is the gotchas, which belong in the repository where the agents that need them work.

## Consequences

- **Every ADR is `Proposed` today**, and stays so until claims are written: the status is now honest rather than uniform. ADRs with small, finished crates (`scatter-merge`, `scatter-challenge`, the fallback typefaces) will go `Accepted` within a few changes; the large ones will show their fraction in *Built*.
- **Finishing a section costs one comment and one test.** The comment is `implements NNNN §N (An)` at the completing code; the test need only cite the section.
- **A change to an `Accepted` ADR shows up in the status by itself**, as `Accepted with proposed amendment (An)`, the moment the entry lands, and clears when the claim's entry ID moves.
- **`coverage.py --todo` is the backlog.** It lists every section not done and what it needs; nothing else needs updating when work lands.
- **Calls made while building are in the ADRs from the day they are made**, with the word *unconfirmed* in their Source; INDEX lists them. James confirms by editing one line.
- **The Project notes are stubs** that point here; the gotchas moved to `docs/testing/gotchas.md`.
- **The claim rule can be gamed** by writing `implements` without checking. The index names the status as "what the code says", the checker refuses malformed claims, and a claim is a reviewable line in a diff; that is the extent of the guard.

## Open questions

- **Q1. Trailers.** Trailer sections (Crates, API, UI, Storage, …) are left out of the status. Whether an ADR's *API* trailer should count through the catalogues' *Built* columns (every route or module it names built) is open; today the catalogues show it and the status does not.
- **Q2. Tests for non-code sections.** A section about crate layout or naming is "tested" by the check that enforces it (`xtask deps` cites 22 §1.2), which `coverage.py` classes as src. Whether such a check should count as the test too, or the rule stays strict, is open.

## References

- `docs/architecture/24-documentation.md` §2.2, §6, §9, §12, §13, §15
- `tools/coverage.py` (`--check`, `--status`, `--todo`), `docs/decisions/check_adrs.py` (`--index` writes Status and the unconfirmed list)
- `docs/testing/gotchas.md`
- The Project notes retired: `claude/implementation-backlog.md`, `claude/open-decisions.md` (stubs after 2026-10-30)

## Amendment log

### A1. One page for what waits on a decision

- **Date:** 2026-10-30
- **Source:** Direct: James, review of 2026-10-30
- **Change:** extends §3
- **Summary:** Everything that waits on a decision is one generated, committed page, `docs/decisions/OPEN.md`: every open question across the ADRs with its first sentence, every unconfirmed log entry (moved there from INDEX, which keeps the counts), and the registry's pending allocations. What waits on work stays a command, `coverage.py --todo`, which now leads with the code's not-built-yet comments, because it changes with every code commit. The fold is in [24](../architecture/24-documentation.md) §13.

Replaced text ([24](../architecture/24-documentation.md) §13, as it stood):

> `docs/decisions/INDEX.md` is generated by `check_adrs.py --index` and committed; a stale index fails the check. It is the one status page of the design: one row per ADR with its number, title, the status the code says (§15), *Built* (done sections over top-level Decision sections), the number of open questions, the number of unconfirmed log entries (§6), the date of its last change, the chapters its pointers name, and the ADRs that changed it. Below the table, every unconfirmed entry, with its date and summary, so that what waits on James is one list. ADR numbers in the last column are plain, not linked, so that a row is one line.
>
> The index is for finding an ADR and seeing at a glance what has touched it, how much of it is built, and what is still open. For anything finer — which section, which entry, which chapter text, which code — `tools/adr.py` and `tools/coverage.py` read the ADRs, the chapters and the code directly: `coverage.py --todo` lists every section not yet done and what it needs, which is the implementation backlog, derived rather than kept.

### A2. What other documents say is built comes from the code

- **Date:** 2026-11-09
- **Source:** Direct: Claude, while building the crate map's as-built view and the status tables James asked for in discussion of 2026-11-09; confirmed by James, 2026-11-09
- **Change:** extends §2, §5
- **Summary:** The status is written from the code outside the ADRs too. The crate map's generated views show what the code says beside the design: the table in [22](../architecture/22-crates-and-stack.md) §2.1 gains a *Built* column (whether each crate is in the workspace), and each entry of `CRATES.md` ends with an *As built* paragraph, the first paragraph of the crate's `//!` docs and the sections its src claims, from the new `coverage.py --crates`. The `contents` field of `crates.toml` is unchanged: it is the current text of the ADRs' Crates trailers, so it stays the design. A document that relies on parts of the design names them in a status table, whose last column `coverage.py --blocks --write` writes from the code and `check.sh` checks; `docs/testing/real-dumps.md`'s hand-kept list of what the code can and cannot do, a remnant of the working notes §5 retired, which had gone stale (it called 0013 §9 bootstrap mode and `verify` unbuilt), is the first replaced. The fold is in [24](../architecture/24-documentation.md) §15 and [22](../architecture/22-crates-and-stack.md) §2.1.

### A3. Product scripts under tools/ are source

- **Date:** 2026-11-09
- **Source:** Direct: James, design discussion of 2026-11-09
- **Change:** extends §1
- **Summary:** A script under `tools/` that is part of the product rather than a check of it says so near its top, "Product code (24 §15)", and `coverage.py` reads it as src, so it can carry an `implements` claim; every other script there is still a test. The documentation site's hooks (`tools/docs_site.py`, 0109) are the first. Settles 0109 Q2. Chapter text: [24](../architecture/24-documentation.md) §15.

Replaced text ([24](../architecture/24-documentation.md) §15, as it stood):

> It is written once, at the code that completes the section, in src (library or binary code, a migration) and never in a test.
