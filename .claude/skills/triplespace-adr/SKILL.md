---
name: triplespace-adr
description: Write a new Triplespace ADR, or change an existing ADR, architecture chapter or registry file, following the 0050 format (fold, log, mark), and validate it with the doc checkers. Use for any change to docs/decisions, docs/architecture or docs/registry.
---

# Changing the Triplespace architecture

The format is ADR 0050, `docs/decisions/0050-adr-format.md`. It is the authority; this skill is
the working procedure. Copy new ADRs from `docs/decisions/TEMPLATE.md`, whose comments repeat the
rules. Use the `triplespace-architecture` skill to find what you are changing.

## Lifecycle

1. **Research.** Read the home chapter, the ADRs its *Sources* lines cite (Context, Open
   questions, Amendment log), `INDEX.md`, and the relevant registry files. List every ADR and
   chapter section the change will touch.
2. **Confirm.** Put the specific choices to James: each option, its cost and your
   recommendation. Name every assumption. Do not draft past an unconfirmed choice.
3. **Draft** the ADR, or the direct change.
4. **Fold** the change into every affected ADR, chapter section and registry file (below).
5. **Validate** with the checkers (below), and fix every reported line.
6. **Leave it uncommitted** on the checked-out branch, or on a named feature branch if `main` is
   checked out, for James's review. Never commit or push without his go-ahead.

## Two kinds of change

- **A new ADR** decides something and changes other ADRs. It names them in its header's
  `Changes`, its section headings and its *Changes to other ADRs* table.
- **A direct decision** changes an ADR with no new ADR: James decides in discussion. The log
  entry's Source is `Direct: James, design discussion of YYYY-MM-DD`, and its Summary holds the
  decision in full, because nowhere else does.

## Writing a new ADR

- File `docs/decisions/NNNN-slug.md`, with the next free number (`ls docs/decisions | tail`).
  H1 `# NNNN. Title`. The number and slug never change.
- Header, in order, one line each, values never sentences:
  - `Status: Proposed`. It stays Proposed until the code implements every section.
  - `Date`.
  - `Updated` only once there is a log.
  - `Author: James Hare / Claude Opus`. The agent name must be an `[[agent]]` in
    `docs/registry/version.toml`; check 9 enforces this.
  - `Changes` and `Uses`: linked numbers in ascending order, omitted when empty.
  - `Chapters`: only once a section is a pointer.
- **Context** is historical: what was known and wanted. *Direction* quotes James's directives
  verbatim, with the date and occasion.
- **Decision** sections are `### N. Title` and `#### N.M Title`. They are numbered once and never
  renumbered or reused. A heading names what the section changes elsewhere, with the six verbs only:
  `### 2. Talk pages are composite (amends 0008 §2; extends 0008 §3)`.

  | Verb | Use when | Target log entry | Replaced text |
  |---|---|---|---|
  | `amends` | replaces or contradicts text | yes | required |
  | `extends` | adds, contradicts nothing | yes | none |
  | `supersedes` | a section or the ADR no longer holds | yes | required |
  | `corrects` | the text was wrong; the decision is unchanged | yes | required |
  | `settles` | answers an open question | no | — |
  | `uses` | relies on it, changes nothing | no | — |

  Do not use refines, specifies, adds, allocates, answers or builds on.
- Close with the trailer sections that apply: Crates (amends 0005 §2), API, UI, Storage,
  Permissions, Search, RDF, Log events, Rate limits, Tenants and Scatterbase.
- Then Alternatives considered (optional), Consequences, Open questions (`**Q1. Title.**`), the
  *Changes to other ADRs* table (one row per target section, giving the target's new log ID),
  and References.
- Citations: link the first mention of another ADR in each section,
  `[0008](0008-namespaces-and-document-pages.md) §2`. Name both ends of a relation outside a
  heading ("0049 §2 amends 0008 §2"). Never use dated forms ("as amended 2026-09-27") or
  placeholders (`§…`).

## Changing an existing ADR: fold, log, mark, answer (0050 §6, §14)

For each target ADR *Y*:

1. **Fold.** Rewrite the affected text so it states the current decision, citing the source in
   the sentence. **If *Y*'s section is a pointer** (*Current text: [NN] §x.y*), fold in that
   chapter section instead, in the same change. Every change is folded, extensions included. Never
   use a blockquote callout such as `> **Amended by …**`.
2. **Log.** Append `### An. Title` to *Y*'s Amendment log, with Date, Source, Change (verbs plus
   sections) and Summary. Quote removed or contradicted text verbatim as `Replaced text (§N):`.
   For a pointer section, quote the chapter text as it stood. IDs are contiguous and never edited.
3. **Mark.** Add `An` to the `*Changed by …*` provenance line of each changed section, creating
   the line if needed. Strike any Consequence the change falsifies (`~~**Claim.** …~~ *Why (An).*`).
   Set *Y*'s `Updated: YYYY-MM-DD (An)`. If *Y* is Accepted, its status becomes
   `Accepted with proposed amendment (An)`.
4. **Answer.** Strike any open question the change settles, and add *Settled by NNNN §N: answer.*
   This needs no log entry.

Nothing is deleted. A section that no longer holds becomes a stub, *Superseded by NNNN §M (An).*,
with its old text in the log entry.

## Chapters

- A chapter section opens with `*Sources: [NNNN](../decisions/…) §N; …*`. When your ADR's
  section contributes text to a chapter section, add its citation to that section's Sources line.
- New ADRs (0079 to 0090 so far) keep their full Decision text, and their current text is also
  folded into the chapters. Register the ADR in `docs/architecture/map.py`: a `HOME` chapter,
  plus `OVERRIDES` for sections that belong elsewhere or to several chapters ("13+14"). Then run
  `python3 architecture/map.py --write .` to regenerate `MAP.md`.
- **Relocation** replaces Decision bodies with pointers and logs a `relocates` entry. It runs as
  `pointers.py --commit <sha> NNNN`, which needs the commit that last carried the full text, so
  it is a separate step after James commits. Ask before running it.
- A chapter decides nothing. A disagreement found in a chapter is a decision for an ADR.
- New chapters take the next number; chapters and their sections are never renumbered.

## Registry files

- A registry change is a commit. Codes, numbers and slugs are never reused or changed: retire
  an entry with `retired = true`.
- The ADR states the rule and names the registry file as the authority. A table copied into an
  ADR says which log entry it is current to ("as of A19").
- The crates embed the files, and their parsers reject unknown fields. A field the parser does
  not accept yet is written as a **comment** beside the entry and listed under *Pending
  allocations* in `docs/registry/README.md`, until the crate learns it.
- Add or update the file's row in the registry README table (*Defined in*).
- `settings.toml` drives chapter 23's catalogue: run `python3 architecture/settings.py --write .`.
- Run `cargo build` for the embedding crate: a malformed TOML file fails the build.

## Validating

From `docs/`:

```sh
python3 decisions/check_adrs.py --index .   # also rewrites INDEX.md; any "   - " line is a failure
python3 architecture/map.py .               # every section mapped and cited, MAP.md current
python3 architecture/settings.py .          # if settings.toml changed
python3 api/vectors/check.py                # if payloads changed
```

`check_adrs.py` covers:
- links and § citations
- header order and symmetry
- heading verbs
- log IDs and Replaced text
- provenance lines matching the log
- *Changes to other ADRs* matching the targets' logs
- `NNNN An` and `Qn` citations
- crate names against chapter 22 §2.1
- `Special:` names against `special-pages.toml`
- agent names against `version.toml`
- pointers matching chapter Sources lines
- `INDEX.md` being current

Never hand-edit `INDEX.md` or `MAP.md`. Regenerate them.

## Bulk edits

To touch many ADRs at once, use a Python script with exact-match replacement that fails
loudly, rather than `sed`:

```python
import pathlib
def sub(path, old, new, count=1):
    p = pathlib.Path(path); t = p.read_text()
    assert t.count(old) == count, f"{path}: expected {count} of {old[:60]!r}, found {t.count(old)}"
    p.write_text(t.replace(old, new))
```

Then run the checkers and read the diff (`git diff --stat`, then the diff itself) before
reporting.

## Reporting back

Give James:
- the ADR number and title
- every ADR and chapter section changed, with the new log IDs
- the registry edits and pending allocations
- the checker results
- the assumptions and open questions that need his decision
