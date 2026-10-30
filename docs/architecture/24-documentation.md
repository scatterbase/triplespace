# 24. Documentation

How the design is written down: the chapters of this directory, the ADRs of [`../decisions/`](../decisions/), the registry files of [`../registry/`](../registry/), and the checkers that hold them together. This chapter is process, not architecture: it describes nothing the software does. It assumes nothing from the other chapters.

## 1. Two kinds of document

*Sources: [0050](../decisions/0050-adr-format.md) §1, §14; [0098](../decisions/0098-pointers-from-birth.md) §1.*

**A chapter describes; an ADR decides.** The design is written down in two kinds of document, and every piece of text has exactly one home.

- **A chapter** (`docs/architecture/NN-slug.md`) holds the **current text**: what holds now, by subject, assembled from the Decision sections of every ADR that touches the subject, with duplicates merged and one home for each table that spans ADRs. A chapter decides nothing on its own: a change to the architecture is an ADR, and the chapter is where its text is folded.
- **An ADR** (`docs/decisions/NNNN-slug.md`) holds **why, when, by whom and what it replaced**: Context, Alternatives, Consequences, Open questions and the Amendment log. Its Decision is a list of numbered headings, each pointing at the chapter sections that hold its text (§3.2).

### 1.1 Current and historical text

*Sources: [0050](../decisions/0050-adr-format.md) §1.*

**Every part of an ADR is either current or historical, and the reader can tell which.**

- **Current:** the header, the Decision's headings and pointers, the Consequences, and the state of each open question. They say what holds now. Reading the Decision's pointers, and the chapter sections they name, gives the decision as it stands.
- **Historical:** the Context, its Direction, and the Amendment log. They say what was known, wanted and changed, and they are never rewritten. References are only appended to.

**Nothing is deleted.** Text that stops being current moves to the Amendment log (§6) or is struck through in place (§7, §8). A chapter's text is never deleted either: the log entry that removes it quotes it.

**What an ADR says is decided, whoever drafted it.** A section Claude proposed carries no mark and has the same standing as one James wrote, until a later change says otherwise. That change is an amendment like any other.

## 2. The ADR file

*Sources: [0050](../decisions/0050-adr-format.md) §2, §3, §4; [0098](../decisions/0098-pointers-from-birth.md) §3.*

### 2.1 File, number and title

*Sources: [0050](../decisions/0050-adr-format.md) §2.*

- The file is `NNNN-slug.md`. The H1 is `# NNNN. Title`, with a full stop after the number.
- **The number and the slug never change.** A title that has gone stale may be retitled with a log entry (`retitles`). Citations use the number, so nothing breaks.

### 2.2 Header

*Sources: [0050](../decisions/0050-adr-format.md) §3; [0098](../decisions/0098-pointers-from-birth.md) §3.*

Fixed fields, in this order, one line each. Each value is a date, a status or a list of links, never a sentence.

| Field | Value | Required |
|---|---|---|
| `Status` | `Proposed`, `Accepted`, `Accepted with proposed amendment (An, …)`, `Superseded by NNNN` or `Withdrawn` (see below) | Yes |
| `Date` | The date the ADR was first written | Yes |
| `Updated` | The date and ID of the latest log entry: `2026-10-01 (A20)` | Once there is a log |
| `Author` | `James Hare / Claude …` | Yes |
| `Changes` | Linked numbers, ascending: every ADR whose text this one changes | When non-empty |
| `Uses` | Linked numbers and contract documents, ascending: what this ADR relies on without changing | When non-empty |

**Status follows the code.**

- **`Proposed`** until every section of the Decision is implemented in code. An ADR implemented in part is still `Proposed`.
- **`Accepted`** once it is implemented in full. The change that completes the implementation sets the status.
- **`Accepted with proposed amendment (A7, A9)`** when a change lands on an `Accepted` ADR before the code implements it. The text is folded as always, so the chapters state the decision as it now stands, and the status names the log entries that the code does not yet implement. The change that implements an entry removes its ID. When the list is empty, the status is `Accepted` again.
- **`Superseded by NNNN`** and **`Withdrawn`** come with the log entry that supersedes or withdraws the ADR.

A change to a `Proposed` ADR leaves it `Proposed`.

**There is no field for what the ADR relies on in the other direction, and none for the chapters it points at.** What changes an ADR is its Amendment log; what it changes is `Changes` and its section headings; which chapters hold its text is read from its pointers. `INDEX.md` (§13) lists all three for every ADR, and the checker generates it. The retired fields `Amended by`, `Related`, `Revised` and `Chapters` are not written.

### 2.3 Skeleton

*Sources: [0050](../decisions/0050-adr-format.md) §4; [0098](../decisions/0098-pointers-from-birth.md) §3.*

```
# NNNN. Title
(header)
## Context
### Direction            optional: the owner's directives, verbatim, with date and occasion
## Decision
### 1. …                 numbered headings, each a pointer; see §3
## Alternatives considered   optional
## Consequences
## Open questions
## References
## Amendment log             required once the ADR has been changed
```

An ADR in parts (0022) marks each part with a bold line before its first section, such as **Part A: Data federation**, so that every section stays a `###` under `## Decision` and numbering runs through the parts. `docs/decisions/TEMPLATE.md` is the skeleton with its comments.

## 3. Decision sections

*Sources: [0050](../decisions/0050-adr-format.md) §5, §14; [0098](../decisions/0098-pointers-from-birth.md) §1.*

### 3.1 Numbering and headings

*Sources: [0050](../decisions/0050-adr-format.md) §5.*

- **Every section of the Decision is numbered**, `### N. Title`, with subsections `#### N.M Title`.
- **Numbers are permanent.** A section is never renumbered and a number is never reused. A new section takes the next free number, even where it would fit better earlier.
- **A section that no longer holds becomes a stub.** It keeps its heading. Its body is the line *Superseded by NNNN §M (An).*, with at most one sentence of pointer, and its old text is in the log entry. See 0008 §11.
- **A heading names what the section changes in other ADRs**, with the verbs of §5: `### 2. Talk pages are composite (amends 0008 §2; extends 0008 §3)`.
- **The first line of a changed section is its provenance line:** *Changed by A3, A6, A9.*, listing every log entry that changed it, in order. A section with no provenance line has not changed since it was written. A `consolidates` or `relocates` entry (§6) is not listed.

### 3.2 Every section is a pointer

*Sources: [0050](../decisions/0050-adr-format.md) §14; [0098](../decisions/0098-pointers-from-birth.md) §1.*

**A Decision section's text lives in the chapters, from the day the ADR is written.** The section keeps its heading, its number and its provenance line (§3.1), and its body is the one line

```
*Current text: [03](03-storage-caches-and-search.md) §4.5; [18](18-api.md) §3.2.*
```

naming every chapter section whose provenance line (§10.2) cites it. The heading stays so that every citation of the form `0013 §7` (§9) keeps resolving, and so that the Amendment log's Replaced text keeps a section to belong to. A Decision section is a pointer or a stub, never full text: the ADR never carries a second copy of what a chapter holds.

**Writing a new ADR is therefore one piece of work in two places.** The decision's text goes into the chapter sections it belongs to, new or existing, each with a provenance line citing the ADR section; the ADR gets the heading, the pointer, and everything only it can hold (§1). The pull request's chapter diff is the proposal as a reviewer reads it; `tools/adr.py show NNNN --full` assembles the ADR with every pointer replaced by its chapter text, and CI (`adr-preview` in `.github/workflows/ci.yml`) posts that assembly as a comment on any pull request that adds an ADR, updated on every push, so a reviewer on GitHub reads one document. Nothing is relocated later, and no step waits on a commit.

**A section in a part of an ADR that no chapter covers is a defect in the chapters, not an exception.** A new subject gets a new chapter (§10.1).

## 4. Changing a decision: fold, log, mark

*Sources: [0050](../decisions/0050-adr-format.md) §6, §14.*

When an ADR, or a decision made directly, changes the text of ADR *Y*, the same change does four things to *Y*:

1. **Fold.** Rewrite the affected text so that it states the current decision, in the chapter section that *Y*'s pointer names, in the same commit. Where a reader would want the detail, cite the source in the sentence: "Deletion is an ACL ([0023](../decisions/0023-moderation.md) §4)." Add the changing ADR's section to the chapter section's provenance line (§10.2).
2. **Log.** Append an entry to *Y*'s Amendment log (§6). Text the change removes or contradicts goes into the entry as **Replaced text**, verbatim, quoted from the chapter section as it stood and cited as that chapter section.
3. **Mark.** Add the entry's ID to the provenance line of each *Y* section it changed. Strike any consequence the change makes false (§7). If *Y* is `Accepted`, add the entry's ID to its status (§2.2). Set *Y*'s `Updated`.
4. **Answer.** If the change settles an open question, strike the question and say what settled it (§8). Settling a question needs no log entry: the struck question is the record.

The changing ADR names *Y* in its header's `Changes` and in its section headings; the target's log entry names the changing section as its Source. The index (§13) derives both directions from these, so they are stated once each.

**Every change is folded, whatever its kind.** An extension that contradicts nothing is folded and logged like an amendment, so a section always reads as one text. Blockquote callouts are not used for changes; a blockquote is for quotation.

**A direct decision** (one James makes in review or discussion, outside any ADR) lands the same way, with Source `Direct:` and the decision in full in the Summary (§6), and the fold made in the chapter.

## 5. Relation verbs

*Sources: [0050](../decisions/0050-adr-format.md) §7, §14; [0098](../decisions/0098-pointers-from-birth.md) §1.*

Six verbs, and no others, in headings and log entries:

| Verb | Effect on the target | Log entry in the target | Replaced text |
|---|---|---|---|
| `amends` | Replaces or contradicts text | Yes | Required |
| `extends` | Adds text and contradicts nothing | Yes | None |
| `supersedes` | A whole section, or the ADR, no longer holds: it becomes a stub, or the Status changes | Yes | Required |
| `corrects` | The text was wrong about something already decided. The decision does not change | Yes | Required |
| `settles` | Answers an open question | No (§8) | — |
| `uses` | Relies on it and changes nothing | No | — |

Three further verbs apply only within an ADR's own log: `retitles` (§2.1); `consolidates`, a rewrite that changes no decision, which quotes what it replaced like any other entry and names the last commit before it; and `relocates`, which the migrations of 2026-10-09 and 2026-10-30 used to move an ADR's text into the chapters (§14) and which no new entry uses, since an ADR is born as pointers.

The retired verbs map onto these. **refines, specifies, narrows**: `amends` if it contradicts, otherwise `extends`. **adds, fills, allocates**: `extends`. **answers**: `settles`. **follows, builds on, realises**: `uses`.

## 6. The Amendment log

*Sources: [0050](../decisions/0050-adr-format.md) §8, §14.*

```markdown
### A9. Articles, categories and page statements

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §1, §3, §4, §8, §11
- **Change:** amends §2, §8, §10; extends §4
- **Summary:** One short paragraph: what changed and why.

Replaced text ([10](../architecture/10-pages-and-content-models.md) §4.3, as it stood):

> - Category links (`[[Category:…]]`) are listed as plain text at the foot of the page. No category pages exist.
```

- **IDs are `A1`, `A2`, …,** assigned in the order entries are appended, and never renumbered. An entry is never edited, except to correct a typo or a broken link.
- **Source** is another ADR's sections, or `Direct:` with who decided and on what occasion ("Direct: James, design discussion of 2026-10-01"). For a direct decision the Summary holds the decision in full, because nowhere else does.
- **Change** uses the verbs of §5.
- **Replaced text** is the removed or contradicted text, quoted as it stood, headed by the chapter section it was taken from (*Replaced text (10 §4.3, as it stood):*), or by the ADR section for entries older than the chapters. Together with the current text, it lets every earlier state be read without git.
- **The log is the only place history accumulates.** It is read through `tools/adr.py log NNNN [An]`, so its size costs a reader nothing until the history is wanted.

## 7. Consequences

*Sources: [0050](../decisions/0050-adr-format.md) §9.*

Consequences are claims about the system, so they are kept true. **A consequence that no longer holds is struck, and the strike says why:**

`- ~~**Imported pages need a flattening pass.** …~~ *Only on a tenant that does not expand templates (A12).*`

A consequence the change adds is appended as a new bullet. Consequences are not numbered.

## 8. Open questions

*Sources: [0050](../decisions/0050-adr-format.md) §10.*

- **Every open question has an ID,** `Q1`, `Q2`, …, written before the strike so that it stays readable: `- **Q3.** ~~**The main namespace.** …~~ *Settled by A6: …*`. IDs are permanent.
- **A settled question is struck,** followed by *Settled by NNNN §N: answer.* or *Settled by An: answer.* for a direct decision. A question dropped without an answer is *Withdrawn: reason.*
- **A question settled in part is struck whole,** and what remains open is appended as a new question that names the old one: `- **Q9.** (Rest of Q4.) …`. Partial strikes in the middle of a sentence are not used.
- **An answer that later changes** is extended after the settlement, not rewritten (0008 Q3).

## 9. Citations

*Sources: [0050](../decisions/0050-adr-format.md) §11.*

| Form | Means |
|---|---|
| `NNNN` | An ADR |
| `NNNN §N`, `NNNN §N.M`, `NNNN §N–M` | ADR sections, as they stand now: the chapter text their pointers name |
| `NNNN §N rule K` | A numbered rule within a section, as now |
| `NNNN An` | An amendment, including the text it replaced |
| `NNNN Qn` | An open question |
| `NNNN Context`, `NNNN Consequences` | Those parts |
| `NN §N`, `NN §N.M` | A chapter section |

- **Inside an ADR, a bare `§N`, `An` or `Qn` means that ADR.** A reference to any other ADR names it, every time. Inside a chapter, a bare `§N` means that chapter.
- **A relation outside a heading names both ends.** Write "0049 §2 amends 0008 §2", never "(§2 amends §2)". In a heading the section itself is the subject, so only the object is named.
- **The first mention of another ADR in each section is a link:** `[0008](0008-namespaces-and-document-pages.md) §2` from an ADR, `[0008](../decisions/0008-namespaces-and-document-pages.md) §2` from a chapter.
- **`NNNN §N` always means the current text.** To cite text as it was, cite the entry that replaced it: "0008 A6". Dated forms ("as amended 2026-09-27"), sources without a number and placeholders (`§…`) are not used.
- **Code cites ADR sections**, as `0013 §7` or `0013 §7 rule 2` in a comment; `tools/coverage.py` reads them and `tools/adr.py code 0013 §7` lists them.

## 10. Chapters

*Sources: [0050](../decisions/0050-adr-format.md) §14; [0098](../decisions/0098-pointers-from-birth.md) §2.*

### 10.1 Numbering and scope

*Sources: [0050](../decisions/0050-adr-format.md) §14.*

`docs/architecture/` is the reference description of Triplespace by subject. Chapters are numbered for reading order and never renumbered; a new subject gets the next number. Sections within a chapter are `## 1.`, `### 1.1` and so on, permanent like ADR sections, so that `02 §3.1` is a stable citation. [`README.md`](README.md) lists the chapters, each with a sentence on what it covers, and nothing else: the conventions are here.

Chapter text is the current Decision text of its source sections, edited into one voice and one order, with duplicates merged. It carries no Context, Consequences, Open questions or Amendment log; those stay in the ADR. A disagreement found between chapters, or between a chapter and an ADR's log, is a decision for an ADR, not a note in a chapter.

### 10.2 Provenance lines are the map

*Sources: [0050](../decisions/0050-adr-format.md) §14; [0098](../decisions/0098-pointers-from-birth.md) §2.*

**Every chapter section names its sources.** The first line under a chapter heading is a provenance line: *Sources: [0013](../decisions/0013-postgres-storage.md) §7; [0032](../decisions/0032-sparql-update-stream.md) §2.* It names every ADR section whose text the chapter section holds, in the citation forms of §9.

**The provenance lines are the only map between ADRs and chapters.** Where an ADR section's text lives is read from the chapter sections that cite it, and nothing else records it: the pointer in the ADR (§3.2) is written from the provenance lines, and the checkers (§12) hold the two directions equal. There is no separate map file or mapping table to maintain.

### 10.3 Tables that span ADRs

*Sources: [0050](../decisions/0050-adr-format.md) §14.*

The namespace table, the content-model table, the REST route table, the permission catalogue, the log-event catalogue, the `view` schema table, the crate map and the settings catalogue each appear once, in one chapter, and other chapters link to the row. Most ADRs end with cross-cutting trailers: Crates, API, UI, Storage, Permissions, Search, RDF, Log events, Rate limits, Tenants and Scatterbase. Those go to chapters 22, 18, 19, 03, 09, 03, 02, 16, 07, 08 and 22 respectively, and the ADR's home chapter links to them. Which of these tables are kept as registry files rather than prose is §11.

## 11. Registry files and catalogues

*Sources: [0050](../decisions/0050-adr-format.md) §6; [0098](../decisions/0098-pointers-from-birth.md) §5.*

**Registry data is stated once.** Where a file in [`../registry/`](../registry/) holds a list (namespaces, special pages, content models, providers, settings…), the chapter states the rule and names the file as authoritative. A table copied from a registry file for readability is generated from it, as chapter 23's settings catalogue is generated from `settings.toml` by `settings.py --write`, and never edited by hand.

**A catalogue is registry data, not prose.** A table whose rows are entries rather than argument — a crate map, a route or module catalogue, a permission or log-event catalogue, a schema — belongs in a registry file with a generated chapter table, so that the chapter stays readable and the code can read the same file. [`../registry/README.md`](../registry/README.md) lists the files and the chapter section each generates into; a table a chapter still carries by hand is the authority until it moves. The `view` schema (03 §4) stays in prose until it has a reader, since the migrations are its source. A move changes no decision and no text: it is one `consolidates` entry in the ADR that owns the chapter section, quoting any chapter prose the move rewrote and naming the last commit that carried the table, and the moved entries keep their citations.

## 12. The checkers

*Sources: [0050](../decisions/0050-adr-format.md) §12, §14; [0098](../decisions/0098-pointers-from-birth.md) §2, §3, §4.*

`docs/check.sh` runs every checker and exits non-zero on any failure; `docs/check.sh --write` regenerates the generated files first (`INDEX.md`, chapter 23's settings tables). CI runs the former.

**`decisions/check_adrs.py`** applies to every ADR:

1. The header fields of §2.2, in order, with a valid Status. The IDs named in an `Accepted with proposed amendment` status are in the log. `Updated` names the last log entry.
2. Contiguous section numbers, every Decision subsection numbered, stubs present.
3. Heading relations that use the verbs of §5 and resolve.
4. Log IDs contiguous, the required fields present, and Replaced text where §5 requires it.
5. Provenance lines that agree with the log, in both directions.
6. The header's `Changes` agrees with the section headings, and every target in it has a log entry or a settled question naming this ADR as its source.
7. Contiguous `Q` IDs, each struck question with a *Settled by* or *Withdrawn*.
8. `NNNN An` and `NNNN Qn` citations that resolve.
9. No blockquote callout announces a change.
10. **Every Decision section is a pointer or a stub**, the pointer names existing chapter sections, and each named section's provenance line cites the pointing ADR section (or its parent).
11. Crate names in headings and tables are in the crate map, and special-page names match the registry case-sensitively.
12. `INDEX.md` is current (§13).

**`architecture/map.py`** applies to the chapters:

1. Every chapter in `README.md`'s table exists, and every chapter file is in the table.
2. Every `##` and `###` heading is followed by a provenance line, and every citation in one names an ADR section that exists.
3. **Every numbered Decision section of every ADR, other than a stub, is cited by at least one provenance line**: nothing an ADR decided is missing from the chapters.
4. Links to ADRs use `../decisions/NNNN-slug.md`.
5. Every *Pending* marker in a chapter has a row in `PENDING.md`, and every row a marker.

**`architecture/settings.py`** checks chapter 23's settings tables against `registry/settings.toml`, and `api/vectors/check.py` the log test vectors.

## 13. The index

*Sources: [0050](../decisions/0050-adr-format.md) §12; [0098](../decisions/0098-pointers-from-birth.md) §4.*

`docs/decisions/INDEX.md` is generated by `check_adrs.py --index` and committed; a stale index fails the check. It is one table, one row per ADR: number, title, status, the date of its last change, the chapters its pointers name, and the ADRs that changed it (read from its log and its settled questions). ADR numbers in the last two columns are plain, not linked, so that a row is one line and an amendment changes one row. What an ADR changes is its own header's `Changes`, and is not repeated here.

The index is for finding an ADR by title and seeing at a glance what has touched it. For anything finer — which section, which entry, which chapter text — `tools/adr.py` reads the ADRs and chapters directly.

## 14. Migrations

*Sources: [0050](../decisions/0050-adr-format.md) §13, §14; [0098](../decisions/0098-pointers-from-birth.md) §6, §7.*

History, kept here so that the ADRs' older log entries can be read.

- **2026-10-01 to 2026-10-07, the 0050 format.** 0008 was converted as a pilot; a mechanical pass gave the remaining ADRs titles, `Q` IDs, the six verbs and two-ended relations; then each ADR was converted in its own commit, every callout and dated self-amendment becoming a log entry, closing with a `consolidates` entry. 0005 §8 and 0013 §12, the two changelog sections, became stubs and their rows became log entries. The old-format checks were removed when no ADR used it.
- **2026-10-08 and 2026-10-09, the chapters.** Chapters 00–23 were drafted from the ADRs as they then stood; the 116 inconsistencies the consolidation found were decided by James and logged as direct decisions in the owning ADRs (`docs/audits/architecture-consolidation-2026-10-08.md`); then each ADR's Decision text was replaced by pointers, logged with a `relocates` entry naming the last commit that carried the text.
- **2026-10-30, pointers from birth.** The nineteen ADRs written after the relocation (0079–0097), which had carried their text beside the chapters' copy, were relocated the same way (commit `2e2a7c8` holds the text). 0050 was relocated into this chapter. The `Chapters` header and the "Changes to other ADRs" section, both derivable, were removed from every ADR by a mechanical pass that is not logged; `MAP.md` and the mapping tables of `map.py` were deleted in favour of the provenance lines; `pointers.py` was retired; and the index was slimmed. From then on an ADR is born as pointers.
