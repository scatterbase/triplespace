# 0050. ADR format

- **Status:** Proposed
- **Date:** 2026-10-01
- **Author:** James Hare / Claude Opus
- **Changes:** [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md)
- **Uses:** [0005](0005-crate-organization.md), [0013](0013-postgres-storage.md)

## Context

Fifty ADRs were written in six days, and they have become two things at once. Each is a **log**: what was decided, when, by whom and why. Each is also a **specification**, and later ADRs keep changing it: 365 links in "Amended by" headers record an ADR changed by another. The ADRs have to say what holds now without losing how it came to hold.

The format grew without a plan. In 0000–0049 as of commit `dc4774a`:

| Pattern | Where | Works | Fails |
|---|---|---|---|
| Fixed skeleton: Context, Decision, Consequences, Open questions, References | All 50 (References in 45) | Every ADR reads the same way | Consequences are never updated. 0008's still say "no templates, categories, transclusion or Lua", which stopped being true with 0038, 0042 and 0043 |
| Numbered sections, `### N.` and `#### N.M` | 551 sections | Citations by `§` are short and stable | One unnumbered section (0002); a placeholder `§…` in 0033 |
| A relation in the section heading, "(amends 0005 §2)" | 285 headings | Says at the point of change what the section changes | Twelve or more verbs: amends, extends, refines, settles, uses, follows, adds, fills, answers, specifies, realises, allocates |
| `Related` and `Amended by` header lines | 49 and 41 ADRs | Find every ADR that touches this one | Prose, averaging about 1,300 characters (the longest is 5,400). 57 entries without detail, 14 lists out of order, 7 with dated non-ADR changes spliced in. 0033 lists bare numbers |
| Blockquote callouts: "Amended by", "Extended by", "Refined by" | 169 in 38 ADRs | Show where a section changed | The text above a callout stays wrong, so the current decision is the body plus every callout, replayed in order. 0008 §2 has nine, not in date order |
| Dated self-amendments: "Amended 2026-09-27: numbering policy" | 17 | Decisions made in review get recorded | Only citable as "as amended 2026-09-27", which is ambiguous when one section was amended twice in a day |
| Consolidated revision with a changelog section and a `Revised` header | 0005 §8, 0013 §12 | Current state is readable | The changelog is a numbered Decision section. Rows cite sources that are not ADRs ("Hash guard of 2026-09-28") |
| Struck open question with *Settled by …* | 116 of 329 questions | History and answer side by side | Questions have no IDs and are cited as "0019, open questions". Some are partly struck in the middle of a sentence |
| A section listing changes to other ADRs | 0033, 0034, 0048 | An edit checklist for the amended files | Three different names; not required |
| "James's direction, from the design discussion of …" | 14 Contexts | Separates the owner's decisions from Claude's proposals | Free text |
| Registry data copied into a table | 0008 §2, 0010 (special pages) | Readable in place | Drifts from `docs/registry/*.toml`. 0047 had to declare 0010's table superseded |
| In-place edits without a record | 0008 §2 table cells "(amendment below)"; header-only amenders of 0008 (0013, 0014, 0016, 0017, 0029) | — | Leave no trace in the text. 0017's header says its change to 0008 §3 is in 0017 §4, but it is in §2 |

Three further facts shape the answer:

- **Citations are ambiguous.** "(§6 refines §8.3)" does not say which ADR each section number belongs to. All 31 problems `check_adrs.py` reports at `dc4774a` are this case, not broken references.
- **Git is not the history.** Every ADR up to 0037 arrived in one commit, `init`, on 2026-09-27. The file has to carry its own history.
- **Status says nothing.** Every ADR is Proposed, by James's decision in the review of 2026-09-26.

### Direction

James's direction, on the first draft of this ADR, 2026-10-01:

- **"ADRs are Accepted when implemented in code."**
- **"It's better if the amendment process is consistent."** (Asked whether a pure extension may stay a callout.)
- **Proposals are "considered confirmed until decided otherwise."** (Asked whether sections Claude proposed should carry an unconfirmed mark.)
- **"The cross-reference index should be committed to the repo as well."**

And later the same day, on implementation in part:

- **"ADRs become Accepted when they are implemented in full. If an amendment lands on an ADR that is already accepted, its status is 'accepted with proposed amendment' until it is fully Accepted again."**

## Decision

### 1. Current text and historical text

**Every part of an ADR is either current or historical, and the reader can tell which.**

- **Current:** the header, the Decision, the Consequences, the state of each open question, and "Changes to other ADRs". They say what holds now. Reading the Decision alone gives the decision as it stands.
- **Historical:** the Context, its Direction, and the Amendment log. They say what was known, wanted and changed, and they are never rewritten. References are only appended to.

**Nothing is deleted.** Text that stops being current moves to the Amendment log (§8) or is struck through in place (§9, §10).

**What an ADR says is decided, whoever drafted it.** A section Claude proposed carries no mark and has the same standing as one James wrote, until a later change says otherwise. That change is an amendment like any other.

### 2. File, number and title

- The file is `NNNN-slug.md`. The H1 is `# NNNN. Title`, with a full stop after the number: 0033 and 0034 change from an em dash.
- **The number and the slug never change.** A title that has gone stale, such as 0036's "the `osm-tag` keyed type", may be retitled with a log entry (`retitles`). Citations use the number, so nothing breaks.

### 3. Header

Fixed fields, in this order, one line each. Each value is a date, a status or a list of links, never a sentence.

| Field | Value | Required |
|---|---|---|
| `Status` | `Proposed`, `Accepted`, `Accepted with proposed amendment (An, …)`, `Superseded by NNNN` or `Withdrawn` (see below) | Yes |
| `Date` | The date the ADR was first written | Yes |
| `Updated` | The date and ID of the latest log entry: `2026-10-01 (A20)` | Once there is a log |
| `Author` | As now: `James Hare / Claude Opus` | Yes |
| `Changes` | Linked numbers, ascending: every ADR whose text this one changes | When non-empty |
| `Uses` | Linked numbers and contract documents, ascending: what this ADR relies on without changing | When non-empty |

**Status follows the code.**

- **`Proposed`** until every section of the Decision is implemented in code. An ADR implemented in part is still `Proposed`.
- **`Accepted`** once it is implemented in full. The change that completes the implementation sets the status.
- **`Accepted with proposed amendment (A7, A9)`** when a change lands on an `Accepted` ADR before the code implements it. The Decision text is folded as always, so it states the decision as it now stands, and the status names the log entries that the code does not yet implement. The change that implements an entry removes its ID. When the list is empty, the status is `Accepted` again.
- **`Superseded by NNNN`** and **`Withdrawn`** come with the log entry that supersedes or withdraws the ADR.

A change to a `Proposed` ADR leaves it `Proposed`.

**`Amended by`, `Related` and `Revised` are retired.** What changed this ADR is its Amendment log. What it changes is `Changes` and its section headings. What it relies on is `Uses`. Both directions for every ADR are in `docs/decisions/INDEX.md`, which the checker generates and which is committed (§12).

### 4. Skeleton

```
# NNNN. Title
(header)
## Context
### Direction            optional: the owner's directives, verbatim, with date and occasion
## Decision
### 1. …                 numbered; see §5
## Alternatives considered   optional
## Consequences
## Open questions
## Changes to other ADRs     required when Changes is non-empty
## References
## Amendment log             required once the ADR has been changed
```

An ADR in parts (0022) marks each part with a bold line before its first section, such as **Part A: Data federation**, so that every section stays a `###` under `## Decision` and numbering runs through the parts.

### 5. Sections

- **Every section of the Decision is numbered**, `### N. Title`, with subsections `#### N.M Title`.
- **Numbers are permanent.** A section is never renumbered and a number is never reused. A new section takes the next free number, even where it would fit better earlier.
- **A section that no longer holds becomes a stub.** It keeps its heading. Its body is the line *Superseded by NNNN §M (An).*, with at most one sentence of pointer, and its old text is in the log entry. See 0008 §11.
- **A heading names what the section changes in other ADRs**, with the verbs of §7: `### 2. Talk pages are composite (amends 0008 §2; extends 0008 §3)`.
- **The first line of a changed section is its provenance line:** *Changed by A3, A6, A9.*, listing every log entry that changed it, in order. A section with no provenance line has not changed since it was written. A `consolidates` entry (§8) is not listed.

### 6. Changing an ADR: fold, log, mark

When an ADR, or a decision made directly, changes the text of ADR *Y*, the same change does four things to *Y*:

1. **Fold.** Rewrite the affected text so that it states the current decision. Where a reader would want the detail, cite the source in the sentence, as the ADRs already do: "Deletion is an ACL ([0023](0023-moderation.md) §4)."
2. **Log.** Append an entry to *Y*'s Amendment log (§8). Text the change removes or contradicts goes into the entry as **Replaced text**, verbatim.
3. **Mark.** Add the entry's ID to the provenance line of each section it changed. Strike any consequence the change makes false (§9). If *Y* is `Accepted`, add the entry's ID to its status (§3).
4. **Answer.** If the change settles an open question, strike the question and say what settled it (§10). Settling a question needs no log entry: the struck question is the record.

The changing ADR names *Y* in its header's `Changes`, its section headings and its "Changes to other ADRs" table, which gives *Y*'s log ID.

**Every change is folded, whatever its kind.** An extension that contradicts nothing is folded and logged like an amendment, so a section always reads as one text. Blockquote callouts are not used for changes; a blockquote is for quotation.

**An ADR in the old format is converted before it is changed.** A new ADR that amends one not yet converted converts it first, in the same change or the one before, so that every change after this ADR lands the same way (§13).

**Registry data is stated once.** Where a file in `docs/registry/` holds a list (namespaces, special pages, content models, providers), the ADR states the rule and names the file as authoritative. A table copied from the registry for readability says which log entry it is current to ("as of A19"), and the change that edits the registry updates the table too.

### 7. Relation verbs

Six verbs, and no others, in headings, log entries and "Changes to other ADRs":

| Verb | Effect on the target | Log entry in the target | Replaced text |
|---|---|---|---|
| `amends` | Replaces or contradicts text | Yes | Required |
| `extends` | Adds text and contradicts nothing | Yes | None |
| `supersedes` | A whole section, or the ADR, no longer holds: it becomes a stub, or the Status changes | Yes | Required |
| `corrects` | The text was wrong about something already decided. The decision does not change | Yes | Required |
| `settles` | Answers an open question | No (§10) | — |
| `uses` | Relies on it and changes nothing | No | — |

The retired verbs map onto these. **refines, specifies, narrows**: `amends` if it contradicts, otherwise `extends`. **adds, fills, allocates**: `extends`. **answers**: `settles`. **follows, builds on, realises**: `uses`.

### 8. The Amendment log

```markdown
### A9. Articles, categories and page statements

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §1, §3, §4, §8, §11
- **Change:** amends §2, §8, §10; extends §4
- **Summary:** One short paragraph: what changed and why.

Replaced text (§8):

> - Category links (`[[Category:…]]`) are listed as plain text at the foot of the page. No category pages exist.
```

- **IDs are `A1`, `A2`, …,** assigned in the order entries are appended, and never renumbered. An entry is never edited, except to correct a typo or a broken link.
- **Source** is another ADR's sections, or `Direct:` with who decided and on what occasion ("Direct: James, design discussion of 2026-10-01"). For a direct decision the Summary holds the decision in full, because nowhere else does.
- **Change** uses the verbs of §7, plus two that apply only within the ADR: `retitles`, and `consolidates`, a rewrite that changes no decision (the migration of §13, or a cleanup pass). A `consolidates` entry quotes what it replaced, like any other entry, and names the last commit before it.
- **Replaced text** is the removed or contradicted text, quoted as it stood, under the section it came from. Together with the current text, it lets every earlier state be read without git.

### 9. Consequences

Consequences are claims about the system, so they are kept true. **A consequence that no longer holds is struck, and the strike says why:**

`- ~~**Imported pages need a flattening pass.** …~~ *Only on a tenant that does not expand templates (A12).*`

A consequence the change adds is appended as a new bullet. Consequences are not numbered.

### 10. Open questions

- **Every open question has an ID,** `Q1`, `Q2`, …, written before the strike so that it stays readable: `- **Q3.** ~~**The main namespace.** …~~ *Settled by A6: …*`. IDs are permanent.
- **A settled question is struck,** followed by *Settled by NNNN §N: answer.* or *Settled by An: answer.* for a direct decision. A question dropped without an answer is *Withdrawn: reason.*
- **A question settled in part is struck whole,** and what remains open is appended as a new question that names the old one: `- **Q9.** (Rest of Q4.) …`. Partial strikes in the middle of a sentence are not used.
- **An answer that later changes** is extended after the settlement, not rewritten (0008 Q3).

### 11. Citations

| Form | Means |
|---|---|
| `NNNN` | An ADR |
| `NNNN §N`, `NNNN §N.M`, `NNNN §N–M` | Sections, as they stand now |
| `NNNN §N rule K` | A numbered rule within a section, as now |
| `NNNN An` | An amendment, including the text it replaced |
| `NNNN Qn` | An open question |
| `NNNN Context`, `NNNN Consequences` | Those parts |

- **Inside an ADR, a bare `§N`, `An` or `Qn` means that ADR.** A reference to any other ADR names it, every time.
- **A relation outside a heading names both ends.** Write "0049 §2 amends 0008 §2", never "(§2 amends §2)". In a heading the section itself is the subject, so only the object is named.
- **The first mention of another ADR in each section is a link:** `[0008](0008-namespaces-and-document-pages.md) §2`.
- **`NNNN §N` always means the current text.** To cite text as it was, cite the entry that replaced it: "0008 A6". Dated forms ("as amended 2026-09-27"), sources without a number ("the hash guard of 2026-09-28") and placeholders (`§…`) are not used.

### 12. The checker

`check_adrs.py` recognises an ADR in this format by its header (no `Related` or `Amended by` field) and applies to it:

1. The header fields of §3, in order, with a valid Status. The IDs named in an `Accepted with proposed amendment` status are in the log.
2. Contiguous section numbers, every Decision subsection numbered, stubs present.
3. Heading relations that use the verbs of §7 and resolve.
4. Log IDs contiguous, the required fields present, and Replaced text where §7 requires it.
5. Provenance lines that agree with the log, in both directions.
6. "Changes to other ADRs" rows that agree with the header's `Changes` and, for a target in this format, with the target's log.
7. Contiguous `Q` IDs, each struck question with a *Settled by* or *Withdrawn*.
8. `NNNN An` and `NNNN Qn` citations that resolve.
9. For a target in this format, its log stands in for `Amended by` in the existing symmetry check.
10. No blockquote callout announces a change: `> **Amended by …**`, `> **Extended by …**` or `> **A5.** …`.
11. `docs/decisions/INDEX.md` is current. `check_adrs.py --index` rewrites it; without the flag, a stale index is reported.

Files still in the old format keep the existing checks until they are converted.

**The index** lists every ADR with its title, status and last update, the ADRs it changes and the ADRs that change it. For an ADR in this format, what changes it is read from its log and its settled questions. For one in the old format, it is read from `Amended by`. What an ADR changes is the inverse, so the two columns cannot disagree. Every change that adds or amends an ADR commits the regenerated index with it.

### 13. Migration

1. **Pilot.** 0008, the most-amended ADR (25 callouts and three amenders that left no mark in it), is converted with this ADR: A1–A19 are the amendments it had received, and A20 is the consolidation.
2. **Mechanical pass over the ADRs still in the old format**, by script and in one commit: titles as in §2, `Q` IDs, verbs as in §7, and relations in headers and parentheses qualified on both ends. `Related` and `Amended by` stay until each ADR is converted. No text of any decision changes, and reformatting is not logged.
3. **Conversion, one ADR per commit**, most-amended first: 0019, 0018, 0010, 0023, 0028, 0006, 0012, 0015, 0013 and onward. Each commit converts the header, turns every callout, dated self-amendment and unmarked inbound relation into a log entry (dated by its source, numbered in date order, the callout's text as its Summary verbatim), folds them, and closes with a `consolidates` entry. Body text the fold removes is quoted under the entry that made it untrue, or under the `consolidates` entry when no single amendment did. An ADR that a new ADR is about to amend jumps the queue (§6).
4. **0005 and 0013.** Their changelogs (0005 §8, 0013 §12) become their Amendment logs, and the two sections become stubs.
5. **Status.** Each ADR implemented in full becomes `Accepted` (§3). None is yet: the crates in the repository implement parts of 0006, 0009, 0017, 0048 and others.
6. **The old checks are removed** once no ADR uses the old format.

## Alternatives considered

- **Immutable ADRs, superseded wholesale** (Nygard's original practice). Each of the 365 changes would have needed a superseding ADR, or a pile of them, and every citation of "0008 §2" would have needed redirecting. Rejected: it fits slow-moving decisions, and these change daily.
- **Keep the callouts and number them.** Cheap, and it makes every change citable, but the current decision would still have to be pieced together from the body and its callouts. Rejected for consistency: every change lands the same way (§6).
- **Strike replaced text in the body.** Readable for a sentence, unreadable for a rewritten table or a section changed nine times. Kept for list items whose history is itself informative: consequences and open questions.
- **YAML front matter.** Easier for machines, but `check_adrs.py` deliberately has no dependencies, and long lists render poorly on GitHub. A bullet header with a fixed grammar gives the checker the same information.

## Consequences

- **The Decision is the decision.** 0008 §2 now gives the namespace table as it is, not as it was on 2026-09-26 with nine notes beneath it.
- **Every change is citable.** "0008 A6" replaces "0008 §2, as amended 2026-09-27".
- **History stays in the file.** Replaced text plus current text reconstructs every earlier state without git, which matters because git history before 2026-09-27 is one commit.
- **Amending costs more.** A rewrite and a log entry instead of a callout, in the same files as today.
- **Files grow.** Converted, 0008 grows from 34.6 KB to 48.9 KB. The text above the log shrinks to 30.4 KB, because the header lines and callouts are gone. The log, 18.5 KB, holds every callout verbatim and every passage the fold replaced, so nothing in the old file is lost.
- **Folding is an editorial act and can introduce errors.** The Replaced text, the source citation and a one-ADR-per-commit diff make each fold reviewable. The pilot found one existing error this way: 0017's header points at the wrong section.
- **Every amending commit touches `INDEX.md`.** It is generated, so the cost is running the checker with `--index`; a stale index fails the check.
- **The checker's false positives go away**, because every relation names both ends. Misdirected annotations like 0017's become errors it can find.

## Open questions

- **Q1.** ~~**Status.** Whether to start using `Accepted`, and when (at the merge that implements an ADR, or at milestone 1). Every ADR is Proposed by the decision of 2026-09-26, so this is James's call.~~ *Settled by §3: an ADR is Accepted once it is implemented in code (Direction).*
- **Q2.** ~~**Extensions as callouts.** Whether a pure extension, which contradicts nothing, may stay a callout instead of being folded. It is cheaper, and the current text stays correct, but the reader has to assemble it again.~~ *Settled by §6: every change is folded, so the process is the same for all (Direction).*
- **Q3.** ~~**Unconfirmed proposals.** Whether sections Claude proposed and James has not yet confirmed should carry a mark. The review of 2026-09-26 asked for 0018 §6 and §10 to be read first for this reason.~~ *Settled by §1: no mark; what an ADR says is decided until a later change says otherwise (Direction).*
- **Q4.** ~~**The generated index.** Whether `INDEX.md` is committed and kept current by the checker, or built only in CI.~~ *Settled by §12: committed, regenerated with `--index`, and checked for staleness (Direction).*
- **Q5.** ~~**Implemented in part.** Code already implements parts of 0006, 0009, 0017, 0048 and others (`scatter-normalize`, `scatter-providers`, `scatter-vocab`, `scatter-wikibase-model`), but none of them whole. Whether an ADR becomes `Accepted` when its first section is implemented or its last, or whether implementation is marked per section. The same question arises when an amendment lands on an ADR that is already `Accepted` and the new text is not yet implemented.~~ *Settled by §3: `Accepted` when implemented in full; an amendment not yet implemented makes it `Accepted with proposed amendment (An, …)` until it is (Direction).*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0000](0000-init.md) §3 | §13 | consolidates | 0000 A4 |
| [0001](0001-revision-metadata-rdf.md) §1–6 | §13 | consolidates | 0001 A14 |
| [0002](0002-source-graphs-and-mass-ingest.md) §3–9 | §13 | consolidates | 0002 A17 |
| [0003](0003-statement-ui.md) §2–9 | §13 | consolidates | 0003 A9 |
| [0004](0004-identity-clusters-and-equivalence.md) §1–10 | §13 | consolidates | 0004 A10 |
| [0005](0005-crate-organization.md) §2, §3, §8 | §13 | consolidates | 0005 A50 |
| [0006](0006-log-integrity-and-erasure.md) §1–9 | §13 | consolidates | 0006 A13 |
| [0007](0007-actor-identity.md) §1–8 | §13 | consolidates | 0007 A15 |
| [0008](0008-namespaces-and-document-pages.md) §1–5, §8–11 | §13 | consolidates | 0008 A20 |
| [0009](0009-keyed-entity-types-and-domain.md) §1–12 | §13 | consolidates | 0009 A7 |

## References

- Michael Nygard, [Documenting Architecture Decisions](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions) (2011)
- [MADR: Markdown Architectural Decision Records](https://adr.github.io/madr/)
- `docs/decisions/TEMPLATE.md`, the skeleton of §4
- `docs/decisions/check_adrs.py`
- `docs/decisions/INDEX.md`, generated by `check_adrs.py --index` (§12)
