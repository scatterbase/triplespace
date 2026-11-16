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

**Nothing is deleted.** Text that stops being current moves to the Amendment log (§6) or is struck through in place (§7); a settled question keeps its ID and title, and the commit that settles it holds what it asked (§8). A chapter's text is never deleted either: the log entry that removes it names the sections it stood in, and the entry's commit holds it (§6).

**What an ADR says is decided, whoever drafted it.** A section Claude proposed carries no mark and has the same standing as one James wrote, until a later change says otherwise. That change is an amendment like any other.

## 2. The ADR file

*Sources: [0050](../decisions/0050-adr-format.md) §2, §3, §4; [0098](../decisions/0098-pointers-from-birth.md) §3.*

### 2.1 File, number and title

*Sources: [0050](../decisions/0050-adr-format.md) §2.*

- The file is `NNNN-slug.md`. The H1 is `# NNNN. Title`, with a full stop after the number.
- **The number and the slug never change.** A title that has gone stale may be retitled with a log entry (`retitles`). Citations use the number, so nothing breaks.

### 2.2 Header

*Sources: [0050](../decisions/0050-adr-format.md) §3; [0098](../decisions/0098-pointers-from-birth.md) §3; [0099](../decisions/0099-status-from-the-code.md) §2.*

Fixed fields, in this order, one line each. Each value is a date, a status or a list of links, never a sentence.

| Field | Value | Required |
|---|---|---|
| `Status` | `Proposed`, `Accepted`, `Accepted with proposed amendment (An, …)`, `Superseded by NNNN` or `Withdrawn` (see below) | Yes |
| `Date` | The date the ADR was first written | Yes |
| `Updated` | The date and ID of the latest log entry: `2026-10-01 (A20)` | Once there is a log |
| `Author` | `James Hare / Claude …` | Yes |
| `Uses` | Linked numbers and contract documents, ascending: what this ADR relies on without changing | When non-empty |

**Status follows the code, and the code writes it.** `Proposed`, `Accepted` and `Accepted with proposed amendment (A7, A9)` are computed from the implementation claims in the code (§15) by `check_adrs.py --index`, which rewrites the header line; a hand-edited one is a check failure, like a stale index.

- **`Proposed`** until every top-level section of the Decision is done: claimed `implements` in src, cited by a test, and unchanged since the claim.
- **`Accepted`** once every section is done.
- **`Accepted with proposed amendment (A7, A9)`** when every section is done or was changed after its claim; the status names the entries the code has not caught up with. The commit that re-checks the code against an entry and moves the claim's entry ID removes it from the list.
- **`Superseded by NNNN`** and **`Withdrawn`** are decisions, not implementation: they are written by hand with the log entry that supersedes or withdraws the ADR, and the checker leaves them alone.

**There is no field for what the ADR changes, none for what changes it, and none for the chapters it points at.** What changes an ADR is its Amendment log. What it changes is recorded where the change lands: the log entries and settled questions of the other ADRs name it as their source, and `tools/adr.py changes NNNN` lists them; its section headings say the same in brief (§3.1). Which chapters hold its text is read from its pointers. `INDEX.md` (§13) lists what changed each ADR and its chapters, and the checker generates it. The retired fields `Amended by`, `Related`, `Revised`, `Chapters` and `Changes` are not written.

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
- **Which entries changed a section is read from the log,** not written under the heading: they are the entries whose Change line names the section (§6), other than a `consolidates`, `retitles` or `relocates` entry. A section no entry names has not changed since it was written. `tools/adr.py show NNNN §N` prints them with the section (*Changed by A3, A6, A9.*), and `tools/coverage.py` reads them the same way (§15).

### 3.2 Every section is a pointer

*Sources: [0050](../decisions/0050-adr-format.md) §14; [0098](../decisions/0098-pointers-from-birth.md) §1.*

**A Decision section's text lives in the chapters, from the day the ADR is written.** The section keeps its heading and its number, and its body is the one line

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
2. **Log.** Append an entry to *Y*'s Amendment log (§6). The entry names the chapter sections whose text the change removes or contradicts as **Replaced text**; the commit that makes the change holds that text, so it is not quoted (§6).
3. **Mark.** Strike any consequence the change makes false (§7). If *Y* is `Accepted`, add the entry's ID to its status (§2.2). Set *Y*'s `Updated`.
4. **Answer.** If the change settles an open question, replace its text with what settled it, keeping its ID and title (§8). Settling a question needs no log entry: the settled question is the record.

The changing ADR names *Y* in its section headings; the target's log entry names the changing section as its Source, and that entry is the record: what an ADR changes is read from the entries and settled questions that name it (§2.2).

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

*Sources: [0050](../decisions/0050-adr-format.md) §8, §14; [0099](../decisions/0099-status-from-the-code.md) §4.*

```markdown
### A9. Articles, categories and page statements

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §1, §3, §4, §8, §11
- **Change:** amends §2, §8, §10; extends §4
- **Summary:** What changed and why, in a few sentences. Chapter text: [10](../architecture/10-pages-and-content-models.md) §4.3.

Replaced text: [10](../architecture/10-pages-and-content-models.md) §4.3.
```

- **IDs are `A1`, `A2`, …,** assigned in the order entries are appended, and never renumbered. An entry is never edited, except to correct a typo or a broken link, and twice by 0050: by A5, which replaced quoted text that git holds with the sections' names and shortened summaries that restated the chapter, by A6, which shortened the *Replaced text* lines and removed the summaries of the `relocates` entries, and by A9, which removed the quotes A5's pass had left under lines it had already converted.
- **Source** is another ADR's sections, or `Direct:` with who decided and on what occasion ("Direct: James, design discussion of 2026-10-01"). For a direct decision the Summary holds the decision itself, with who made it and why, because the fold says neither.
- **A decision made while building is logged by the agent that made it,** in the same change as the code, with Source `Direct: Claude, while building <what>, YYYY-MM-DD; unconfirmed`. It is decided like any other entry (§1.1) and folded like any other; the word *unconfirmed* is what the index (§13) lists it by. James confirms it by editing the Source line (`Direct: James, confirmed YYYY-MM-DD; Claude's call while building …`) or overturns it with a new entry. Notes outside the ADRs are not where such calls are kept.
- **Change** uses the verbs of §5.
- **Summary** says what changed and why: the reason, anything the change withdraws or corrects, and the chapter sections it changed (*Chapter text: …*). It does not restate the chapter text, which the diff shows. For a direct decision it holds the decision, its reasons, its consequences and each call it made, and leaves out only detail the fold states; an unconfirmed entry is kept whole until James confirms it, since he confirms it by reading it. An entry from another ADR need not repeat that ADR's reasons. An entry whose commit does not hold the change (see below) keeps its summary whole. A `relocates` entry (§5) has no Summary: every one says the same thing, which is that the Decision's current text moved to the chapter sections its pointers name, that the ADR kept its headings, provenance lines, Context, Consequences, Open questions and log, and that no decision changed; `tools/adr.py log NNNN An --diff` shows the text as it stood in the ADR.
- **Replaced text** names the sections whose text the change removed or contradicted, as they stood before the entry, and nothing more: *Replaced text: [10](…) §4.3.* The entry is written in the same change as the fold, so the commit that adds the entry's heading holds the text, and its parent holds the text as it stood. `tools/adr.py log NNNN An --before` prints those sections as they stood and `--diff` what the change did to them, word by word. The tool finds the commit each time, so no entry records a hash, uncommitted work needs none, and a rebase changes nothing; when the commit also carries other entries, or only gave an older entry its heading, the tool says so.
- **Text is quoted only where git does not hold it:** text edited before the repository held it (entries dated before 2026-09-27, or edited in place before the 0050 conversion of 2026-10-01), or a file `adr.py` does not read, such as a registry file. Such text is quoted verbatim, headed by the section it stood in (*Replaced text (10 §4.3, as it stood):*).
- **The log is where history is recorded, and git holds the text it names.** It is read through `tools/adr.py log NNNN [An] [--before | --diff]`, so its size costs a reader nothing until the history is wanted.

## 7. Consequences

*Sources: [0050](../decisions/0050-adr-format.md) §9.*

Consequences are claims about the system, so they are kept true. **A consequence that no longer holds is struck, and the strike says why:**

`- ~~**Imported pages need a flattening pass.** …~~ *Only on a tenant that does not expand templates (A12).*`

A consequence the change adds is appended as a new bullet. Consequences are not numbered.

## 8. Open questions

*Sources: [0050](../decisions/0050-adr-format.md) §10.*

- **Every open question has an ID and a title,** `- **Q3. The main namespace.** …`, with `Q1`, `Q2`, … assigned in order. IDs are permanent.
- **A settled question keeps its ID and title and names what settled it:** `- **Q3. The main namespace.** *Settled by A6.*`, in the change that settles it. The source is an ADR or its section (*Settled by 0038 §8.*), a section of the same ADR (*Settled by §3.*) or, for a direct decision, the log entry that records it (*Settled by A6.*). The text it asked is not kept in the file and not struck through: the commit that settles it holds it, and `tools/adr.py questions NNNN Qn --before` prints the question as it was asked, finding the commit each time as §6 does.
- **The answer is not restated** where the source states it, which the chapter section or the log entry does. It stays in the note, after a colon (*Settled by NNNN §N: answer.*), only where nothing else holds it: the source is not an ADR or a log entry (a registry change, a test vector, a decision in discussion that no entry records), or the answer has a detail the source's text lacks. A question dropped without an answer is *Withdrawn: reason.*, and the reason always stays, since only the note holds it.
- **A question settled in part is settled whole,** and what remains open is appended as a new question that names the old one: `- **Q9.** (Rest of Q4.) …`.
- **An answer that later changes** is extended after the settlement, not rewritten: *Settled by A6.* *Changed by 0038 §8 (A9).* (0008 Q3).

## 9. Citations

*Sources: [0050](../decisions/0050-adr-format.md) §11; [0099](../decisions/0099-status-from-the-code.md) §1.*

| Form | Means |
|---|---|
| `NNNN` | An ADR |
| `NNNN §N`, `NNNN §N.M`, `NNNN §N–M` | ADR sections, as they stand now: the chapter text their pointers name |
| `NNNN §N rule K` | A numbered rule within a section, as now |
| `NNNN An` | An amendment, including the text it replaced |
| `NNNN Qn` | An open question |
| `NNNN Context`, `NNNN Consequences` | Those parts |
| `NN §N`, `NN §N.M` | A chapter section |
| `implements NNNN §N (An)` | In code only: the section is built in full as it stood at An (§15) |

- **Inside an ADR, a bare `§N`, `An` or `Qn` means that ADR.** A reference to any other ADR names it, every time. Inside a chapter, a bare `§N` means that chapter.
- **A relation outside a heading names both ends.** Write "0049 §2 amends 0008 §2", never "(§2 amends §2)". In a heading the section itself is the subject, so only the object is named.
- **The first mention of another ADR in each section is a link:** `[0008](0008-namespaces-and-document-pages.md) §2` from an ADR, `[0008](../decisions/0008-namespaces-and-document-pages.md) §2` from a chapter.
- **`NNNN §N` always means the current text.** To cite text as it was, cite the entry that replaced it: "0008 A6". Dated forms ("as amended 2026-09-27"), sources without a number and placeholders (`§…`) are not used.
- **Code cites ADR sections**, as `0013 §7` or `0013 §7 rule 2` in a comment, to say what it is about; it claims a section built with `implements 0013 §7 (A41)` (§15). `tools/coverage.py` reads both and `tools/adr.py code 0013 §7` lists them.

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

*Sources: [0050](../decisions/0050-adr-format.md) §6; [0098](../decisions/0098-pointers-from-birth.md) §5; [0105](../decisions/0105-catalogues-checked-against-the-running-code.md) §1; [0107](../decisions/0107-settings-and-log-events-checked-against-the-running-code.md) §1; [0108](../decisions/0108-routes-modules-and-tenancy-switches-checked-against-the-running-code.md) §1.*

**Registry data is stated once.** Where a file in [`../registry/`](../registry/) holds a list (namespaces, special pages, content models, providers, settings…), the chapter states the rule and names the file as authoritative. A table copied from a registry file for readability is generated from it, as chapter 23's settings catalogue is generated from `settings.toml` by `settings.py --write`, and never edited by hand.

**A catalogue is registry data, not prose.** A table whose rows are entries rather than argument — a crate map, a route or module catalogue, a permission or log-event catalogue, a schema — belongs in a registry file with a generated chapter table, so that the chapter stays readable and the code can read the same file. [`../registry/README.md`](../registry/README.md) lists the files and the chapter section each generates into; a table a chapter still carries by hand is the authority until it moves. The `view` schema (03 §4) stays in prose until it has a reader, since the migrations are its source. A move changes no decision and no text: it is one `consolidates` entry in the ADR that owns the chapter section, quoting any chapter prose the move rewrote and naming the last commit that carried the table, and the moved entries keep their citations.

**A catalogue the code acts on is checked against the running code, both ways** ([0105](../decisions/0105-catalogues-checked-against-the-running-code.md) §1). The code, asked to, reports each name it uses as it runs (a permission it checks, a setting or tenancy switch it reads, a log event it writes, a route or module it serves), and a script compares that set with the registry file ([22](22-crates-and-stack.md) §4.9). A name the code used that the file lacks is **unlisted**: the code does something the documentation does not say, or misspells it. An entry the runs never used is **not observed**: not built, or not exercised, since a run sees only the paths its tests take. Such a check starts as a report on the CI run's summary page and becomes a failure once a run reports nothing unlisted and James makes it strict. Permissions, settings, log events, tenancy switches, REST routes and Action API modules are checked so, strictly ([09](09-security-and-moderation.md) §2.2; [23](23-configuration-and-registry.md) §3.1; [16](16-logs-feeds-and-notifications.md) §2.3; [08](08-tenants-and-instances.md) §5.1; [18](18-api.md) §2.3, §3.2; [0107](../decisions/0107-settings-and-log-events-checked-against-the-running-code.md) §1; [0108](../decisions/0108-routes-modules-and-tenancy-switches-checked-against-the-running-code.md) §1, §2).

## 12. The checkers

*Sources: [0050](../decisions/0050-adr-format.md) §12, §14; [0098](../decisions/0098-pointers-from-birth.md) §2, §3, §4; [0099](../decisions/0099-status-from-the-code.md) §2.*

`docs/check.sh` runs every checker and exits non-zero on any failure; `docs/check.sh --write` regenerates the generated files first (`INDEX.md`, chapter 23's settings tables). CI runs the former.

**`decisions/check_adrs.py`** applies to every ADR:

1. The header fields of §2.2, in order. `Status` is the one the code says (§15); `--index` rewrites it, and without `--index` a stale one is reported. `Updated` names the last log entry.
2. Contiguous section numbers, every Decision subsection numbered, stubs present.
3. Heading relations that use the verbs of §5 and resolve.
4. Log IDs contiguous, the required fields present, and Replaced text where §5 requires it.
5. No `*Changed by …*` line under a Decision heading and no `Changes` header field, both being read from the logs; a stub's entry is one whose Change names the section.
6. An ADR that a section heading says is amended, superseded, corrected or settled has a log entry or a settled question naming this ADR as its source.
7. Contiguous `Q` IDs; no question struck through; each settled question in the form of §8, with its title.
8. `NNNN An` and `NNNN Qn` citations that resolve.
9. No blockquote callout announces a change.
10. **Every Decision section is a pointer or a stub**, the pointer names existing chapter sections, and each named section's provenance line cites the pointing ADR section (or its parent).
11. Crate names in headings and tables are in the crate map, and special-page names match the registry case-sensitively.
12. `INDEX.md` and `OPEN.md` are current (§13).

**`tools/coverage.py --check`** fails on a malformed implementation claim (§15): one on a section that does not exist, one on a changed section that names no entry, or one naming an entry that did not change the section.

**`architecture/map.py`** applies to the chapters:

1. Every chapter in `README.md`'s table exists, and every chapter file is in the table.
2. Every `##` and `###` heading is followed by a provenance line, and every citation in one names an ADR section that exists.
3. **Every numbered Decision section of every ADR, other than a stub, is cited by at least one provenance line**: nothing an ADR decided is missing from the chapters.
4. Links to ADRs use `../decisions/NNNN-slug.md`.
5. Every *Pending* marker in a chapter has a row in `PENDING.md`, and every row a marker.

**`architecture/settings.py`** checks chapter 23's settings tables against `registry/settings.toml`, and `api/vectors/check.py` the log test vectors.

## 13. The index and the open list

*Sources: [0050](../decisions/0050-adr-format.md) §12; [0098](../decisions/0098-pointers-from-birth.md) §4; [0099](../decisions/0099-status-from-the-code.md) §3.*

`docs/decisions/INDEX.md` is generated by `check_adrs.py --index` and committed; a stale index fails the check. It is the one status page of the design: one row per ADR with its number, title, the status the code says (§15), *Built* (done sections over top-level Decision sections), the number of open questions, the number of unconfirmed log entries (§6), the date of its last change, the chapters its pointers name, and the ADRs that changed it. ADR numbers in the last column are plain, not linked, so that a row is one line.

**`docs/decisions/OPEN.md`, generated and committed beside it, is everything that waits on a decision:** every open question of every ADR (§8) with its first sentence, every unconfirmed log entry (§6), and the registry's pending allocations (`registry/README.md`). It is a view: a question is settled in its ADR, an entry confirmed on its Source line, an allocation made in the registry, and the page regenerated. Nothing waiting on James is kept anywhere else.

**What waits on work is a command, not a page:** `tools/coverage.py --todo` lists the code's own not-built-yet comments and every section not done with what it needs (§15). It changes with every code commit, so it is not committed; INDEX's *Built* column is the committed summary. For anything finer — which section, which entry, which chapter text, which code — `tools/adr.py` and `tools/coverage.py` read the ADRs, the chapters and the code directly.

## 14. Migrations

*Sources: [0050](../decisions/0050-adr-format.md) §13, §14; [0098](../decisions/0098-pointers-from-birth.md) §5, §6, §7; [0099](../decisions/0099-status-from-the-code.md) §5.*

History, kept here so that the ADRs' older log entries can be read.

- **2026-10-01 to 2026-10-07, the 0050 format.** 0008 was converted as a pilot; a mechanical pass gave the remaining ADRs titles, `Q` IDs, the six verbs and two-ended relations; then each ADR was converted in its own commit, every callout and dated self-amendment becoming a log entry, closing with a `consolidates` entry. 0005 §8 and 0013 §12, the two changelog sections, became stubs and their rows became log entries. The old-format checks were removed when no ADR used it.
- **2026-10-08 and 2026-10-09, the chapters.** Chapters 00–23 were drafted from the ADRs as they then stood; the 116 inconsistencies the consolidation found were decided by James and logged as direct decisions in the owning ADRs (`docs/audits/architecture-consolidation-2026-10-08.md`); then each ADR's Decision text was replaced by pointers, logged with a `relocates` entry. Each entry named the last commit that carried the text until 0050 A5 removed the hashes; the commit is now found from git (§6).
- **2026-10-30, pointers from birth.** The nineteen ADRs written after the relocation (0079–0097), which had carried their text beside the chapters' copy, were relocated the same way (commit `2e2a7c8` holds the text). 0050 was relocated into this chapter. The `Chapters` header and the "Changes to other ADRs" section, both derivable, were removed from every ADR by a mechanical pass that is not logged; `MAP.md` and the mapping tables of `map.py` were deleted in favour of the provenance lines; `pointers.py` was retired; and the index was slimmed. From then on an ADR is born as pointers.
- **2026-10-30, later the same day: catalogues and status.** The crate map, the REST routes, the Action API modules, the permission catalogue and the log-event catalogue moved from chapter prose into registry files with generated tables (§11; 0005 A111, 0012 A82–A83, 0016 A40, 0011 A35), 22 §2.2's history narrative was retired (0005 A112), and the implementation status became the code's to carry (§15; 0099): the `implements` claim, the generated `Status` header, the index as status page, unconfirmed log entries for calls made while building, and the retirement of the working notes outside the repository.

## 15. Implementation status

*Sources: [0099](../decisions/0099-status-from-the-code.md) §1, §2, §3, §5.*

**The code carries the implementation status of the decisions it encodes.** Nothing else records what is built: not the ADR header, which is written from the code (§2.2), not a backlog, which is derived (§13), not a note. The record is two kinds of citation in code comments, read by `tools/coverage.py` from `crates/`, `xtask/`, `ui/src`, `ui/e2e` and `tools/`:

- **About:** `0013 §7`, `0090 §5–6`, `03 §4.5`. The code is about the section. Evidence of work, not of completion; a comment whose sentence says the thing is not built yet ("not yet built", "deferred", "TODO") counts as that.
- **A claim:** `implements 0013 §7 (A41)`. The section is built in full, as it stood at log entry A41. It is written once, at the code that completes the section, in src (library or binary code, a migration, or a script under `tools/` that says near its top it is "Product code (24 §15)", [0099](../decisions/0099-status-from-the-code.md) A3) and never in a test; every other script under `tools/` is a test. `implements 0013 §7.3 (A41)` and `implements 0013 §7 rule 2` claim a part; a section whose subsections are all claimed is claimed. A section never changed (no log entry names it, §3.1) is claimed without an entry; one that has been changed must name the entry it is current to, so that a later change makes the claim stale rather than silently wrong. A claim on a chapter section, `implements 03 §6.1`, claims every ADR section its provenance line names: the current text is what was built.

**A section is done** when a src claim covers it, a test cites it (about or claim), and no log entry changed it after the claim. It is *amended* when entries came after the claim: the code is re-checked against them and the claim's entry moved, or the code changed. It is *untested* when claimed with no test citing it, *partial* when claimed in part or only cited as about, *not built* when cited only as such, and *none* when nothing cites it. Trailer sections (§10.3) and stubs are left out: a trailer's subject is claimed under its home section, and the catalogues show their own *Built* columns from the contract and the code (§11).

**The status of an ADR follows** (§2.2), and `check_adrs.py --index` writes it. `coverage.py --check`, run by `check.sh`, refuses a malformed claim; `coverage.py --todo` lists what is not done and what it needs; `coverage.py NNNN` shows one ADR section by section; `coverage.py NNNN §N` the citing lines; `coverage.py --blocks` checks the status tables (below).

**Other documents take what is built from the code too** ([0099](../decisions/0099-status-from-the-code.md) §2, §5). The crate map's generated views carry it beside the design: a *Built* column in [22](22-crates-and-stack.md) §2.1 and an *As built* paragraph for each crate in `CRATES.md`, from the crate's own `//!` docs and its claims (`coverage.py --crates`). A document that relies on parts of the design, such as a test plan or a runbook, names them in a **status table**: a Markdown table between `<!-- coverage.py: status -->` and `<!-- /coverage.py -->`, whose rows are written by hand, each naming one ADR or chapter section, Action API module or crate in its first cell, and whose last cell `coverage.py --blocks --write` writes from the code (a chapter section's status combines those of the sections its Sources line names). `check.sh` refuses a stale table. Such a document keeps no hand-written list of what works and what does not; [`../testing/real-dumps.md`](../testing/real-dumps.md) is the first.

**Writing the claim is part of finishing the work.** A change that completes a section adds its `implements` line and the test that cites it, in the same change; a change that implements a log entry on an `Accepted` ADR moves the claim's entry ID. A change that makes a decision while building logs it, unconfirmed, in the owning ADR (§6). Neither is written for work someone else did: a claim asserts that the writer checked the section's text against the code.

**Before this rule** (2026-10-30), every ADR was `Proposed` by hand and the citations were all *about*; the claims are added as each crate is next worked on, and an ADR becomes `Accepted` when its last claim lands. The working notes that had tracked implementation and open calls outside the repository were retired the same day; their non-derivable remainder, the sandbox and test-environment gotchas, is `docs/testing/gotchas.md`.

## 16. The documentation site

*Sources: [0109](../decisions/0109-the-documentation-site.md) §1, §2, §3, §4.*

**The documentation is published as a site** ([0109](../decisions/0109-the-documentation-site.md) §1): [triplespace.readthedocs.io](https://triplespace.readthedocs.io/), built by Read the Docs with MkDocs and the Material theme from `docs/` at every push to `main` and every pull request (`mkdocs.yml`, `.readthedocs.yaml`, `docs/requirements.txt`). Each page is the file of the same name in `docs/`, rendered; `docs/samples/`, `docs/audits/`, the scripts and the ADR template are left out. CI builds the site with `mkdocs build --strict`, so a broken link or a failing generator cannot merge. The site carries no text of its own beyond its settings.

**What the site generates is never committed** ([0109](../decisions/0109-the-documentation-site.md) §2). A page the site derives from another source is produced when the site is built, from that source, by the hooks in `tools/docs_site.py`, and exists nowhere else, so no copy of it can fall out of date: the navigation (from the files that exist), the home page (from the opening of the repository's `README.md`), the As built pages and the REST reference below. The generated files committed before the site (`INDEX.md`, `OPEN.md`, `CRATES.md` and the chapter tables of §11) stay committed, because they are read in the repository where no build runs, and stay checked: CI refuses a stale one (§12).

**As built** ([0109](../decisions/0109-the-documentation-site.md) §3) is a page per chapter, beside it on the site: under each of the chapter's numbered headings, each ADR section its Sources line names, with the status the code gives it (§15), the code that claims it and the code's own "not yet built" comments, linked to the commit the site was built from, and then what the code says at each of those places, the comment paragraph beside it, in the code's own words. An overview page counts each chapter's sections by status. The status is `tools/coverage.py`'s, imported, not recomputed, so the site and the command line cannot disagree.

**The REST reference** ([0109](../decisions/0109-the-documentation-site.md) §4) is generated from the OpenAPI contract, `docs/api/triplespace-v0.openapi.json`, which the server's tests hold to the router operation by operation: one entry per operation, with its summary, description, parameters and responses.

