# 0050. ADR format

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-11-16 (A12)
- **Author:** James Hare / Claude Opus
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

*Current text: [24](../architecture/24-documentation.md) §1.1.*

### 2. File, number and title

*Current text: [24](../architecture/24-documentation.md) §2.1.*

### 3. Header

*Current text: [24](../architecture/24-documentation.md) §2.2.*

### 4. Skeleton

*Current text: [24](../architecture/24-documentation.md) §2.3.*

### 5. Sections

*Current text: [24](../architecture/24-documentation.md) §3.1.*

### 6. Changing an ADR: fold, log, mark

*Current text: [24](../architecture/24-documentation.md) §4, §11.*

### 7. Relation verbs

*Current text: [24](../architecture/24-documentation.md) §5.*

### 8. The Amendment log

*Current text: [24](../architecture/24-documentation.md) §6.*

### 9. Consequences

*Current text: [24](../architecture/24-documentation.md) §7.*

### 10. Open questions

*Current text: [24](../architecture/24-documentation.md) §8.*

### 11. Citations

*Current text: [24](../architecture/24-documentation.md) §9.*

### 12. The checker

*Current text: [24](../architecture/24-documentation.md) §12, §13.*

### 13. Migration

*Current text: [24](../architecture/24-documentation.md) §14.*

### 14. Chapters: the current text lives outside the ADR

*Current text: [24](../architecture/24-documentation.md) §1, §3.2, §4, §5, §6, §10.1, §10.2, §10.3, §12, §14.*

## Alternatives considered

- **Immutable ADRs, superseded wholesale** (Nygard's original practice). Each of the 365 changes would have needed a superseding ADR, or a pile of them, and every citation of "0008 §2" would have needed redirecting. Rejected: it fits slow-moving decisions, and these change daily.
- **Keep the callouts and number them.** Cheap, and it makes every change citable, but the current decision would still have to be pieced together from the body and its callouts. Rejected for consistency: every change lands the same way (§6).
- **Strike replaced text in the body.** Readable for a sentence, unreadable for a rewritten table or a section changed nine times. Kept for list items whose history is itself informative: consequences and open questions.
- **Chapters that copy the ADRs and leave them whole** (considered 2026-10-08). The current text would be stated twice and drift apart; the chapters were built precisely because the ADRs had drifted from one another. Rejected: a section is either text or a pointer (A1).
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
- **An ADR is read with its chapters.** After A1 the Decision of a relocated ADR is a list of pointers; the text is in `docs/architecture/`, where a subject is stated once. The ADR keeps what only it can hold: why, when, by whom, what it replaced, and what is still open (A1).

## Open questions

- **Q1. Status.** *Settled by §3.*
- **Q2. Extensions as callouts.** *Settled by §6.*
- **Q3. Unconfirmed proposals.** *Settled by §1.*
- **Q4. The generated index.** *Settled by §12.*
- **Q5. Implemented in part.** *Settled by §3.*

## References

- Michael Nygard, [Documenting Architecture Decisions](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions) (2011)
- [MADR: Markdown Architectural Decision Records](https://adr.github.io/madr/)
- `docs/decisions/TEMPLATE.md`, the skeleton of §4
- `docs/decisions/check_adrs.py`
- `docs/decisions/INDEX.md`, generated by `check_adrs.py --index` (§12)
- `docs/architecture/README.md`, the chapters; `docs/architecture/map.py` and, until A2, `MAP.md` (§14)
- `docs/architecture/24-documentation.md`, the current text of this ADR (A3)
- `docs/audits/architecture-consolidation-2026-10-08.md`, the audit that produced the chapters' decisions (§14)

## Amendment log

### A1. Chapters and pointers

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussions of 2026-10-08 and 2026-10-09
- **Change:** extends §1, §5, §6, §12; extends §3
- **Summary:** The current text of every ADR moves to the architecture chapters of `docs/architecture/`, one subject per chapter, assembled from the Decision sections and checked against them by `map.py`; a Decision section a chapter holds becomes a pointer to the chapter section, keeping its heading and provenance line; the header gains `Chapters`; a change to a relocated section is folded in the chapter and logged and marked in the ADR; relocation is logged with the verb `relocates` and no Replaced text; the decisions made while the chapters were written are logged as direct decisions of 2026-10-08 in the ADRs they change; the checker gains checks 12–15. James's direction, 2026-10-08: "Rather than write duplicate copy, content that is currently in the ADRs can be copied to these documents, and the ADR documents focus more on changes over time"; "Store up the proposed ADR changes, and when we are done with the architecture document, we can update the ADRs in one pass"; and 2026-10-09: "pointers reflect the direction I want the ADR log to go." §14 states it.

### A2. Pointers from birth

- **Date:** 2026-10-30
- **Source:** 0098 §1, §2, §3, §4, §5, §6, §7
- **Change:** amends §3, §4, §5, §6, §7, §12, §14; extends §13
- **Summary:** A new ADR's Decision sections are pointers from the day it is written: its text goes straight into the chapters, and nothing is relocated later. The chapters' provenance lines are the only map (`MAP.md` and the mapping tables of `map.py` are gone). The `Chapters` header and the "Changes to other ADRs" section, both derivable, are retired. The index is one slim table. Catalogues are registry data with generated chapter tables. 0050's own text moves to chapter 24 with this entry, so the fold is in 24 §2–§5, §10–§14.

Replaced text: §4 of this ADR; §5 of this ADR; §6 of this ADR; §7 of this ADR; §12 of this ADR; §14 of this ADR.

### A3. Current text relocated to chapter 24

- **Date:** 2026-10-30
- **Source:** 0098 §6
- **Change:** relocates §1–§14

### A4. Status from the code

- **Date:** 2026-10-30
- **Source:** 0099 §1, §2, §3, §4, §5
- **Change:** amends §3, §8, §11, §12; extends §13
- **Summary:** The Status header is written from the code: `Proposed`, `Accepted` and `Accepted with proposed amendment` are computed by `check_adrs.py --index` from `implements NNNN §N (An)` claims in src and test citations, read by `tools/coverage.py`; `Superseded` and `Withdrawn` stay hand-written. Code citations gain the claim form. A decision made while building is logged by the agent in the same change with Source `Direct: Claude, while building …; unconfirmed`, and the index lists such entries. The index gains *Built*, *Open* and *Unconfirmed* columns; `coverage.py --todo` is the derived backlog. The folds are in 24 §2.2, §6, §9, §12, §13 and the new §15.

Replaced text: 24 §2.2, §6, §9, §13.

### A5. Replaced text is named, not quoted, where git holds it

- **Date:** 2026-11-15
- **Source:** Direct: James, design discussion of 2026-11-12 and 2026-11-15, and review of the dry run in pull requests #261 and #262
- **Change:** amends §1, §6, §8
- **Summary:** Replaced text names the sections it stood in rather than quoting them, because the commit that makes a change holds the text as it stood: an entry is written in the same change as its fold. `tools/adr.py log NNNN An --before` prints the sections as they stood and `--diff` what the change did, finding the commit each time from the one that added the entry's heading, so no hash is recorded. Text is still quoted where git does not hold it: changes made before the repository, or in place before the 0050 conversion of 2026-10-01, and files the tool does not read. A summary says what changed and why, with the chapter sections it changed, and no longer restates the chapter text; a direct decision's summary keeps the decision, its reasons and each call, and drops only what its fold states. Every existing entry was converted once, in this change: a quote was replaced by the sections' names only where `--before` was checked to print it (1,039 quotes; 18 are kept, where git does not hold the text), quotes that cited the wrong section were re-cited to the section that held the text (0005 A98, A101, A110; 0013 A67; 0029 A11; 0034 A14; 0041 A4; 0047 A9; 0054 A5; 0057 A9), `relocates` entries were shortened, and summaries that restated the chapter were shortened: by hand in 0005, 0012, 0013, 0015, 0018 and 0086, then in every ADR by leaving out the sentences whose words stand in the sections the entry's commit folded (80% of a sentence's content words for an entry from another ADR; 90% for a direct decision, which also keeps its first two sentences, its consequences and its labelled calls). A sentence giving a reason, a withdrawal or a correction was kept, as was every unconfirmed entry, every entry dated before 2026-10-01, and every summary that enumerates its calls. "Together with the current text, it lets every earlier state be read without git" is withdrawn. Chapter text: 24 §1.1, §4, §6.

Replaced text: 24 §1.1, §4, §6.

### A6. Replaced text lines name the sections alone; relocation entries have no summary

- **Date:** 2026-11-16
- **Source:** Direct: James, review of pull request #262, 2026-11-16
- **Change:** amends §8
- **Summary:** A *Replaced text* line is the sections' names and nothing else: "as it stood before this entry; in git (`tools/adr.py log NNNN An --before`)" said the same thing on every line, and the command follows from the entry's own ID, so the rule states it once. A `relocates` entry has no Summary, since all 98 carried the same three sentences, which the rule now states once; `check_adrs.py` no longer requires a Summary of an entry that only relocates. Every existing entry was converted in this change: 703 lines shortened and 98 summaries removed, about 80 KB. Nothing was removed that the rule does not state or the entry's ID does not give. Chapter text: 24 §6.

Replaced text: 24 §6.

### A7. The relocation entries no longer name a commit

- **Date:** 2026-11-16
- **Source:** Direct: James, review of pull request #262, 2026-11-16
- **Change:** corrects §13
- **Summary:** The account of the 2026-10-08 and 2026-10-09 migration said each `relocates` entry names the last commit that carried the text. A5 removed those hashes, so the sentence now says the entries named it until then and that the commit is found from git. No decision changed. Chapter text: 24 §14.

Replaced text: 24 §14.

### A8. A settled question keeps its ID and title; git holds what it asked

- **Date:** 2026-11-16
- **Source:** Direct: James, review of pull request #262, 2026-11-16
- **Change:** amends §1, §6, §10, §12
- **Summary:** A settled question is its ID, its title and what settled it, written in the change that settles it; it is no longer struck through with its text kept, because that commit holds the text, as 0050 A5 found for replaced text. `tools/adr.py questions NNNN Qn --before` prints the question as it was asked, finding the commit each time. The answer is not restated where the source states it (an ADR section, whose chapter text holds it, or a log entry); it stays in the note where nothing else holds it, and a withdrawal always keeps its reason. `check_adrs.py` and `OPEN.md` tell a settled question by its *Settled by* or *Withdrawn* note, and the checker rejects a struck question. Every settled question was converted in this change (230): 196 answers were cut to their source, as were 12 later extensions; 15 notes were already a bare source; 6 are withdrawals, kept whole; 5 are kept whole because their source is neither an ADR nor a log entry (0009 Q2 and Q3, 0033 Q2, 0108 Q1, 0109 Q2); and 8 answers are kept because they hold a code name or number that no chapter, registry or API document has (0001 Q1; 0004 Q1, Q4 and Q7; 0007 Q1; 0016 Q7; 0022 Q2; 0030 Q2). Six questions that had no title were given one from their opening words (0018 Q8, 0033 Q1 and Q2, 0034 Q1 and Q2, 0070 Q5). The three struck sub-items under 0001 Q3, which have no ID, are left as they are. Chapter text: 24 §1.1, §4, §8, §12.

Replaced text: 24 §1.1, §4, §8, §12.

### A9. The quotes A5's pass left under converted lines are removed

- **Date:** 2026-11-16
- **Source:** Direct: James, review of pull request #262, 2026-11-16
- **Change:** amends §8
- **Summary:** A5's pass replaced a quote's heading with the sections' names in 78 places where it left one or more of the quoted paragraphs standing below, 143 paragraphs in all, though `tools/adr.py log NNNN An --before` prints each of them. They are removed under A5's own rule, each only after checking that the command prints it: 91 paragraphs character for character, and 52 with only the path of a link differing, since a quote in an ADR links `0019-….md` where the chapter links `../decisions/0019-….md`. No rule changed; the one sentence of the rule that lists the passes that have edited entries now names this one. The 18 quotes A5 kept because git does not hold their text stay, under their quoted headings. Chapter text: 24 §6.

Replaced text: 24 §6.

### A10. The `Changes` header and the Changed-by lines are read from the logs

- **Date:** 2026-11-16
- **Source:** Direct: James, review of pull request #262, 2026-11-16
- **Change:** amends §3, §5, §6, §12, §14
- **Summary:** Two things an ADR stated twice are no longer written. The `*Changed by A3, A6.*` line under a section heading repeated the Change lines of the ADR's own log, and the checker held the two equal; the entries that changed a section are now read from the log, by `tools/coverage.py` for the status and by `tools/adr.py show NNNN §N` for a reader. The `Changes` header repeated, from the changing side, what the targets' log entries and settled questions record; `tools/adr.py changes NNNN` lists those. In 101 of 111 ADRs the header was exactly that list, and in the other ten it was shorter (0005's left out 38 ADRs whose logs name it), so the derived list is the fuller one. Removed in this change: 665 lines and 108 headers, 56 KB; `coverage.py`'s output is the same before and after. Section headings keep their verbs: `uses` is recorded nowhere else, and a log entry does not say which of its source's sections changed which target section. The checker now requires a heading's claim to amend, supersede, correct or settle to be matched by the target's log; a claim to extend is not enforced, because 17 headings claim an extension that the target never logged, which is left for a decision. An empty `*Changed by .*` line under 0011 §6 went with the rest. Chapter text: 24 §2.2, §3.1, §3.2, §4, §12.

Replaced text: 24 §2.2, §3.1, §3.2, §4, §12.

### A11. A heading's claim to extend is checked against the target's log

- **Date:** 2026-11-16
- **Source:** Direct: James, review of pull request #262, 2026-11-16
- **Change:** amends §12
- **Summary:** The checker requires a heading that says it extends another ADR to be matched by a log entry or settled question of that ADR, as A10 required for the other changing verbs. The 17 claims that had no match are resolved in this change. Eight were extensions the chapters already carried, and their entries are added: 0012 A94 and A95, 0010 A58, 0016 A45, 0052 A10, 0024 A22, 0013 A105 and A106; 0028 §11's heading now names the sections it extends. Nine were reliance, not change, and their headings now say `uses`: 0015 §6 (0009 §6), 0023 §5 (0006 Context), 0025 §4 (0030 §5), 0029 §5 (0026 §2), 0031 §2 (0004 §4, §7), 0032 §2 (0002 §2–3), 0032 §5 (0006 §7), 0053 §2 (0008 §8) and 0054 §3 (0035 §7); each target is in the ADR's `Uses`. A corrected heading is not logged in its own ADR. Chapter text: 24 §12.

Replaced text: 24 §12.

### A12. Citations in the Amendment log are plain

- **Date:** 2026-11-16
- **Source:** Direct: James, review of pull request #262, 2026-11-16
- **Change:** amends §8, §11
- **Summary:** In the Amendment log a citation of an ADR or a chapter section is written plain, `0038 §1` or `10 §4.3`, not as a link. The log is read through `tools/adr.py log`, which prints citations plain already, so the links' paths were read by no one there. Links elsewhere in an ADR stay, the pointers into the chapters among them, so moving from an ADR to its text on GitHub is as it was. The checker reads an entry's Source in the plain form, verifies that every ADR, chapter and section named on a Source or Replaced text line or in a Chapter text list exists, and rejects an ADR link in an entry. Every entry was converted in this change: 1,603 ADR links and 955 chapter links, 92 KB; `tools/adr.py log NNNN An --before` and `--diff` give the same result for each as before. Ten links stay: eight to a chapter named without a section and two whose path is not the plain form's. Quoted text is untouched. One row of the index changes: 0012 now lists 0006 among the ADRs that changed it, because a direct decision's Source there names 0006 A9 in plain form, as three other ADRs' did with a link. Chapter text: 24 §6, §9.

Replaced text: 24 §6, §9.
