# 0068. Merging a fork with upstream, in both directions

- **Status:** Proposed
- **Date:** 2026-10-05
- **Updated:** 2026-10-09 (A2)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0047](0047-special-pages.md), [0054](0054-forking-a-mirrored-page.md), [0067](0067-proposals.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0030](0030-edit-filters.md), [0040](0040-instance-prerogatives.md), [0042](0042-template-expansion-and-parsoid.md), [0051](0051-page-redirects.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md)
- **Chapters:** [13](../architecture/13-mirrored-pages.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

A fork ([0054](0054-forking-a-mirrored-page.md)) is a local page whose first record holds the upstream text it started from. It knows what upstream has done since — the About panel counts newer revisions and `GET /page/{id}/upstream-diff` shows both sides — but "applying upstream's changes is a manual edit; a merge tool is Q1", and whether a tenant may **re-follow** upstream for a fork nobody changed was the other half of that question. [0067](0067-proposals.md) now wants the same machinery pointed the other way: a page proposal is the fork's changes applied to upstream's current text.

Both are the same computation. A fork has a **base** (the text it was forked from), **ours** (its current text) and **theirs** (upstream's current text). Pulling is base→theirs applied onto ours; pushing is base→ours applied onto theirs. One three-way merge serves both, and one record shape marks that it happened.

### Direction

James's direction, from the design discussions of 2026-10-04 and 2026-10-05:

- **The default destination is the wiki being mirrored**; where several, the first repository.
- Proposals are threads ([0067](0067-proposals.md)).

## Decision

### 1. One three-way merge (settles 0054 Q1)

*Current text: [13](../architecture/13-mirrored-pages.md) §4.1.*

### 2. Pulling: upstream's changes into the fork (amends 0054 §6)

*Current text: [13](../architecture/13-mirrored-pages.md) §4.2.*

### 3. Pushing: the fork's changes as a page proposal (extends 0067 §1 and §3)

*Current text: [13](../architecture/13-mirrored-pages.md) §4.3.*

### 4. Export and push

*Changed by A1.*

*Current text: [13](../architecture/13-mirrored-pages.md) §4.4.*

### 5. Adoption, and re-following upstream (amends 0054 §9)

*Current text: [13](../architecture/13-mirrored-pages.md) §4.5.*

### 6. UI and API (extends 0012 §5; extends 0047 §9)

*Current text: [18](../architecture/18-api.md) §2.3, §3.2; [19](../architecture/19-site-ui.md) §3.1, §6.10.*

### 7. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **A parse-aware merge** that understands templates and sections. Better conflicts, far more code, and wrong wherever the parser and the repository's disagree; Wikipedia resolves its own edit conflicts on lines.
- **Rebasing the fork's history onto upstream's**, as a version control system would. A wiki's history is append-only ([0006](0006-log-integrity-and-erasure.md)); the `merge` record moves the base without rewriting anything.
- **Automatic pulls** into forks nobody has changed. Equivalent to re-following with extra steps; `fork.refollow = auto` is the honest form.
- **Pushing to the article by default** regardless of size. A large push to a live article is how a fork gets reverted wholesale; the talk-page default for large changes follows the destination's own etiquette.

## Consequences

- **0054 Q1 is settled in both halves**: a merge tool, and a re-follow decision with a setting.
- **One algorithm, one record** serves pulling, pushing and adoption; the fork's base moves with each pull, so diffs stay small.
- **A fork can end well**: adopted upstream and re-followed, with its history kept and linked.
- **Test plan.** `scatter-merge` against a diff3 fixture set (clean, identical, conflict, conflict at file ends, CRLF); a pull writes `merge` and the next `upstream-diff` uses the new base; a push's payload applied to upstream's text reproduces ours where unconflicted; adoption detected after an upstream edit equal to the push; re-follow under `ask` deletes with the summary and links the history; `ts-licence-incompatible` on a CC0 tenant pushing to CC BY-SA.

## Open questions

- **Q1. Section-level proposals.** Proposing one section of a fork rather than the whole page, which the destination's edit-request convention prefers and which a line merge can scope by heading.
- **Q2.** ~~**Merging talk threads.** A fork's imported threads ([0054](0054-forking-a-mirrored-page.md) §5) and the upstream talk page diverge; whether a pull should import new upstream sections as threads.~~ *Settled by [0069](0069-synchronized-talk-pages.md) §7: no; a fork chooses to follow the upstream talk page, which then shows every new section as it is written, or to fork it, and may switch either way later.*
- **Q3. Conflicts in dependencies.** A locally edited template with upstream changes needs its own merge; whether `Update dependencies` should open those merges in sequence.
- **Q4. Partial re-follow.** A fork whose only difference is a local category or short description; whether those could survive as page statements on the mirror ([0052](0052-page-repositories-and-title-inheritance.md) Q1) so the fork can be re-followed.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §7 | amends | 0005 A71 |
| [0012](0012-api-requirements.md) §5 | §6 | extends | 0012 A48 |
| [0047](0047-special-pages.md) §9 | §6 | extends | 0047 A11 |
| [0054](0054-forking-a-mirrored-page.md) §6, §9 | §2, §5 | amends | 0054 A1 |
| [0067](0067-proposals.md) §1, §3 | §3 | extends | 0067 A1 |

## References

- [diff3](https://www.gnu.org/software/diffutils/manual/html_node/Comparing-Three-Files.html) and Khanna, Kunal & Pierce, *A Formal Investigation of Diff3* (2007)
- [Wikipedia:Copying within Wikipedia](https://en.wikipedia.org/wiki/Wikipedia:Copying_within_Wikipedia): the attribution convention the push summary follows; [Wikipedia:Edit requests](https://en.wikipedia.org/wiki/Wikipedia:Edit_requests)
- [0054](0054-forking-a-mirrored-page.md) §6 and Q1, the diff this ADR turns into a merge

## Amendment log

### A1. Talk-destination proposals push now

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §4
- **Summary:** [0069](0069-synchronized-talk-pages.md), the newer, stands: a following fork sends a `talk`-destination proposal through 0069 §6 now, gated by 0069 §6's switch; this section's "later phase" applies to page proposals only. (PENDING F3)

Replaced text (§4):

> **Push** ([0067](0067-proposals.md) §6, the later phase) performs the destination's edit as the person with the summary above.

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§7
- **Summary:** The Decision's current text now lives in the architecture chapters [13](../architecture/13-mirrored-pages.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
