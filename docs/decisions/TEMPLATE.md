# NNNN. Title

<!--
Format: ADR 0050. Copy to NNNN-slug.md. Delete these comments and any optional part you do not use.
Header values are dates, statuses or lists of links, never sentences.
Omit Updated until the first log entry; omit Changes and Uses when empty. Chapters (0050 §14) lists the
architecture chapters that hold this ADR's current text, once any Decision section is a pointer.
Status is Proposed until the ADR is implemented in full, then Accepted. A change to an Accepted ADR that the
code does not implement yet makes it: Accepted with proposed amendment (An, …), until it does (0050 §3).
After any change to an ADR, run `python3 decisions/check_adrs.py --index .` from docs/ and commit INDEX.md.
-->

- **Status:** Proposed
- **Date:** YYYY-MM-DD
- **Updated:** YYYY-MM-DD (An)
- **Author:** James Hare / Claude …
- **Changes:** [NNNN](NNNN-slug.md), [NNNN](NNNN-slug.md)
- **Uses:** [NNNN](NNNN-slug.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [18](../architecture/18-api.md)

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
A new ADR carries its full text here until a chapter of docs/architecture/ absorbs it (0050 §14). Then the
body becomes one line naming the chapter sections that hold it, and `pointers.py` writes it:
*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.5.*
-->

### 2. Title (amends NNNN §N; extends NNNN §M)

<!--
A heading names what the section changes in other ADRs, with these verbs only:
amends, extends, supersedes, corrects, settles, uses.
Inside this ADR a bare §N, An or Qn means this ADR. Always name any other ADR: [0008](0008-….md) §2.
-->

### 3. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-…` | … |

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

## Changes to other ADRs

<!-- Required when the header lists Changes. One row per target section. -->

| Target | By | Change | Target's log |
|---|---|---|---|
| [NNNN](NNNN-slug.md) §N | §2 | amends | NNNN An |

## References

- …

## Amendment log

<!--
Added when another ADR or a direct decision first changes this one. Append only.
When a change lands here:
  1. Fold: rewrite the affected text so it says what holds now, citing the source. For a section that is a
     pointer, the fold is made in the chapter section the pointer names (0050 §14).
  2. Log: add the entry below, with any removed text quoted verbatim as Replaced text.
  3. Mark: add the ID to the provenance line of each changed section:  *Changed by A1, A3.*
  4. Strike consequences it falsifies, and settle questions it answers (questions need no entry).
  5. Update the header's Updated line; if the ADR is Accepted, add the entry to its status; regenerate INDEX.md.
Every change is folded, extensions included: no callouts.
-->

### A1. Short title of the change

- **Date:** YYYY-MM-DD
- **Source:** [NNNN](NNNN-slug.md) §N  <!-- or: Direct: James, design discussion of YYYY-MM-DD -->
- **Change:** amends §2; extends §3  <!-- amends, extends, supersedes, corrects, retitles, consolidates -->
- **Summary:** What changed and why, in a short paragraph. For a direct decision, the decision in full.

Replaced text (§2):

> The text this change removed or contradicted, as it stood.
