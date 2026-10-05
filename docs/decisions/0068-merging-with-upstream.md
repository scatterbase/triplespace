# 0068. Merging a fork with upstream, in both directions

- **Status:** Proposed
- **Date:** 2026-10-05
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0047](0047-special-pages.md), [0054](0054-forking-a-mirrored-page.md), [0067](0067-proposals.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0030](0030-edit-filters.md), [0040](0040-instance-prerogatives.md), [0042](0042-template-expansion-and-parsoid.md), [0051](0051-page-redirects.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md)

## Context

A fork ([0054](0054-forking-a-mirrored-page.md)) is a local page whose first record holds the upstream text it started from. It knows what upstream has done since — the About panel counts newer revisions and `GET /page/{id}/upstream-diff` shows both sides — but "applying upstream's changes is a manual edit; a merge tool is Q1", and whether a tenant may **re-follow** upstream for a fork nobody changed was the other half of that question. [0067](0067-proposals.md) now wants the same machinery pointed the other way: a page proposal is the fork's changes applied to upstream's current text.

Both are the same computation. A fork has a **base** (the text it was forked from), **ours** (its current text) and **theirs** (upstream's current text). Pulling is base→theirs applied onto ours; pushing is base→ours applied onto theirs. One three-way merge serves both, and one record shape marks that it happened.

### Direction

James's direction, from the design discussions of 2026-10-04 and 2026-10-05:

- **The default destination is the wiki being mirrored**; where several, the first repository.
- Proposals are threads ([0067](0067-proposals.md)).

## Decision

### 1. One three-way merge (settles 0054 Q1)

**A new pure crate, `scatter-merge`**, implements a line-based three-way merge of wikitext — the `diff3` algorithm with the two-way diff of [0010](0010-site-ui.md) §6 as its primitive — returning the merged text and a list of **hunks**: `clean` (one side changed), `identical` (both changed the same way) or **`conflict`** (both changed differently), each with its base, ours and theirs lines. Wikitext is line-oriented enough for this to work as it does for Wikipedia's own edit-conflict resolution; the merge is **text only**, never a parse, so templates and references are lines like any other, and a conflict inside a template call is a conflict.

The three inputs are always: **base** = the text of the fork's `create` record ([0054](0054-forking-a-mirrored-page.md) §1), or of the last `merge` record (§2) when the fork has pulled before; **ours** = the fork's current text; **theirs** = the repository page's current wikitext, fetched as the upstream diff is ([0054](0054-forking-a-mirrored-page.md) §6; [0012](0012-api-requirements.md) §6), at a revision the result records.

### 2. Pulling: upstream's changes into the fork (amends 0054 §6)

**Merge from upstream** replaces 0054 §6's "applying upstream's changes is a manual edit". From the About panel or `Special:MergeUpstream/{title}`, the editor sees the merge of §1 as a **guided edit**: clean hunks applied, each conflict shown with base, ours and theirs and three buttons (keep ours, take theirs, edit), and the whole result editable in the ordinary editor with preview before saving. Saving appends an ordinary `edit` record whose content part carries **`merge`** `{repository, upstream_revid, base_revid, conflicts_resolved}`; the summary reads "Merged English Wikipedia revision 1234567890" with the editor's comment. The record is the fork's new **base** for the next merge, so repeated pulls diff against what was last merged rather than against the original fork, and `GET /page/{id}/upstream-diff` ([0054](0054-forking-a-mirrored-page.md) §6) reads the base from the latest `merge` record.

**Dependencies** ([0054](0054-forking-a-mirrored-page.md) §4): a pull merges the article alone. The panel says when upstream's templates or modules the fork copied have newer revisions, and offers **Update dependencies**, which re-runs 0054 §4's copy for the listed titles as a job; a dependency a tenant has edited locally is listed, not overwritten, and gets its own `Special:MergeUpstream`.

**Permissions**: `edit` on the fork, as any edit; a pull is subject to edit filters ([0030](0030-edit-filters.md)) and protection like any edit. **Rate**: the upstream fetch counts in `upstream` ([0024](0024-subsidiary-accounts.md) §5); the edit in `edit`.

### 3. Pushing: the fork's changes as a page proposal (extends 0067 §1 and §3)

**Propose to {repository}** on a fork compiles the other direction: base→ours applied onto theirs, by the same merge. The result is the **payload** of a page proposal ([0067](0067-proposals.md) §3): `{ text, base_revid, upstream_revid, destination, conflicts }`. A conflict here means upstream changed a passage the fork also changed; the proposer resolves it in the same guided view before the `propose` record is appended, because an unresolved conflict is not a proposal anyone can apply.

**The destination** within the wiki:

| `destination` | Writes | Default when |
|---|---|---|
| `article` | The destination page itself | The merge is clean and changes at most `proposals.direct_edit_lines` (default 50) lines |
| `talk` | A new section on the destination's talk page carrying the diff and a link to the proposal thread, as Wikipedia's edit-request convention does | Otherwise |
| `draft` | A page under the proposer's user space on the destination wiki, or its `Draft:` namespace where one exists | Chosen |

The proposer may override the default. **Attribution**: the push's edit summary links the fork's history ("Merged from {fork URL}; see its history for authors"), which is the "copied from" convention CC BY-SA asks for, and the proposal thread records that the summary was written. **Licence**: a push is refused with `ts-licence-incompatible` when the tenant's `content.licence` cannot be carried onto the repository's — [0053](0053-mirrored-pages.md) §9's check with the roles reversed — and warned when the tenant has no licence set.

### 4. Export and push

**Export** ([0067](0067-proposals.md) §4) for a page offers the merged **wikitext** (copy or download) and the **diff** against upstream's current text as unified diff or as the two-column view, with instructions for the destination (paste into the article's editor; post on talk; create the draft). **Push** ([0067](0067-proposals.md) §6, the later phase) performs the destination's edit as the person with the summary above.

### 5. Adoption, and re-following upstream (amends 0054 §9)

**A page proposal is adopted** when a later upstream revision, merged against the fork's base, leaves **no difference** between ours and theirs for the proposed hunks: upstream now says what the fork says there, whether the push was applied as-is or an editor there rewrote it to the same effect. The page-mirror projection ([0053](0053-mirrored-pages.md) §6) runs the test when the destination page changes, and [0067](0067-proposals.md) §5's state machine takes it from there.

**Re-following upstream.** A fork that no longer differs from upstream — because its changes were adopted, or because its editors pulled everything and changed nothing — may stop being a fork. Deleting it restores the mirror as primary ([0054](0054-forking-a-mirrored-page.md) §9); this ADR makes that a decision with a setting:

| `fork.refollow` (site) | Behaviour |
|---|---|
| `ask` (default) | The About panel and the proposal thread offer **Re-follow upstream**, which deletes the fork with the summary "Re-following {repository}: no local changes remain" and links the fork's history from the mirror's talk page |
| `auto` | A fork whose text equals upstream's current text for `fork.refollow_after` (default 30 days) is deleted by a job, as an instance act ([0040](0040-instance-prerogatives.md) §7), with the same summary and a notification to the fork's editors |
| `never` | The offer is not shown |

A re-followed fork's history is kept, as a deleted page's is; **Re-fork** from the mirror creates a new fork with a new `create`, and the old history is linked from the About panel as "previously forked, re-followed on {date}".

### 6. UI and API (extends 0012 §5; extends 0047 §9)

**`Special:MergeUpstream/{title}`** (new; group `pagetools`; restricted to `edit` on the page) is the guided merge of §2, also reached from the About panel's "{n} newer revisions" line. **The fork's About panel** gains: the base revision (original or last merged), **Merge from upstream**, **Propose to {repository}**, **Re-follow upstream** under `ask`, and the dependency notice of §2.

**REST**: `GET /page/{id}/merge-preview?direction=pull|push` (the merge with hunks, never stored), `POST /page/{id}/merge` (a pull: body is the resolved text, writes the `edit` with `merge`), `POST /page/{id}/refollow`. The proposal routes are [0067](0067-proposals.md) §7's. **Action API**: `action=edit` accepts `tsmerge={upstream revid}` to carry the `merge` field, so bots that pull can say so.

### 7. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-merge` | **New, layer 1, pure, wasm.** Line-based three-way merge with hunk classification; the unified diff writer |
| `scatter-pages` | The `merge` field on an `edit`'s content part |
| `triplespace-repos` | Merge previews from the fork's base, current text and the repository's current revision; the dependency notice and update job; the adoption test for page proposals; the re-follow job |
| `triplespace-api-action`, `triplespace-api-rest` | The routes of §6 and `tsmerge` |
| `triplespace-ui` | `Special:MergeUpstream`, the guided conflict view, the About panel actions |

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
- **Q2. Merging talk threads.** A fork's imported threads ([0054](0054-forking-a-mirrored-page.md) §5) and the upstream talk page diverge; whether a pull should import new upstream sections as threads.
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
