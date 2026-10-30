# 0099. Status from the code

- **Status:** Proposed
- **Date:** 2026-10-30
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

*Current text: [24](../architecture/24-documentation.md) §9, §15.*

### 2. The status is written from the code (amends 0050 §3, §12)

*Current text: [24](../architecture/24-documentation.md) §2.2, §12, §15.*

### 3. The index is the status page (amends 0050 §12)

*Current text: [24](../architecture/24-documentation.md) §13, §15.*

### 4. Decisions made while building are logged, unconfirmed (amends 0050 §8)

*Current text: [24](../architecture/24-documentation.md) §6.*

### 5. The working notes are retired (extends 0050 §13)

*Current text: [24](../architecture/24-documentation.md) §15.*

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
