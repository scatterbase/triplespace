# NNNN. Title

<!--
Format: chapter 24 of docs/architecture/ (ADRs 0050 and 0098). Copy to NNNN-slug.md. Delete these
comments and any optional part you do not use.
Header values are dates, statuses or lists of links, never sentences.
Omit Updated until the first log entry; omit Changes and Uses when empty.
Status is Proposed until the ADR is implemented in full, then Accepted. A change to an Accepted ADR that the
code does not implement yet makes it: Accepted with proposed amendment (An, …), until it does (24 §2.2).
The Decision's text does not go here. Write it into the chapter sections it belongs to, each with a
*Sources:* line citing this ADR's section (24 §3.2); then run `python3 architecture/pointers.py NNNN`
from docs/ to fill the pointers below, and `docs/check.sh --write` to regenerate INDEX.md and check.
-->

- **Status:** Proposed
- **Date:** YYYY-MM-DD
- **Updated:** YYYY-MM-DD (An)
- **Author:** James Hare / Claude …
- **Changes:** [NNNN](NNNN-slug.md), [NNNN](NNNN-slug.md)
- **Uses:** [NNNN](NNNN-slug.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

<!-- Historical: what was known and wanted when this was decided. Never rewritten later. -->

### Direction

<!-- Optional. The owner's directives, verbatim, with the date and occasion. -->

James's direction, from the design discussion of YYYY-MM-DD:

- **…**

## Decision

### 1. Title

<!--
Numbered and permanent: never renumbered or reused. Subsections are #### 1.1 Title.
The body is the one line pointers.py writes, naming every chapter section whose Sources line cites §1:
*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.5.*
-->

### 2. Title (amends NNNN §N; extends NNNN §M)

<!--
A heading names what the section changes in other ADRs, with these verbs only:
amends, extends, supersedes, corrects, settles, uses.
Inside this ADR a bare §N, An or Qn means this ADR. Always name any other ADR: [0008](0008-….md) §2.
Each target gets a log entry (fold, log, mark: 24 §4) whose Source names this section.
-->

### 3. Crates (amends 0005 §2)

<!-- Trailers (Crates, API, UI, Storage, Permissions, …) are sections like any other; their text goes to
     the chapter that holds that catalogue (24 §10.3). -->

## Alternatives considered

<!-- Optional. -->

## Consequences

- **…** …

<!-- When a later change makes one false: - ~~**Claim.** …~~ *Why it no longer holds (An).* -->

## Open questions

- **Q1. Title.** …

<!--
Settled:   - **Q1.** ~~**Title.** …~~ *Settled by [NNNN](…) §N: answer.*
Withdrawn: - **Q1.** ~~**Title.** …~~ *Withdrawn: reason.*
Partly:    strike the whole question, then append - **Qn.** (Rest of Q1.) …
-->

## References

- …

## Amendment log

<!--
Added when another ADR or a direct decision first changes this one. Append only.
When a change lands here:
  1. Fold: rewrite the chapter section the pointer names so it says what holds now, citing the source,
     and add the changing section to that chapter section's Sources line.
  2. Log: add the entry below, with any removed text quoted verbatim as Replaced text.
  3. Mark: add the ID to the provenance line of each changed section:  *Changed by A1, A3.*
  4. Strike consequences it falsifies, and settle questions it answers (questions need no entry).
  5. Update the header's Updated line; if the ADR is Accepted, add the entry to its status; run check.sh --write.
Every change is folded, extensions included: no callouts.
-->

### A1. Short title of the change

- **Date:** YYYY-MM-DD
- **Source:** [NNNN](NNNN-slug.md) §N  <!-- or: Direct: James, design discussion of YYYY-MM-DD -->
- **Change:** amends §2; extends §3  <!-- amends, extends, supersedes, corrects, retitles, consolidates -->
- **Summary:** What changed and why, in a short paragraph. For a direct decision, the decision in full.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §4.5, as it stood):

> The text this change removed or contradicted, as it stood in the chapter.
