# 0050. ADR format

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-30 (A4)
- **Author:** James Hare / Claude Opus
- **Changes:** [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md), [0022](0022-federation.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0025](0025-oauth-server.md), [0026](0026-sitelinks.md), [0027](0027-preferences-and-portability.md), [0028](0028-tenancy-policy.md), [0029](0029-resolver-namespaces.md), [0030](0030-edit-filters.md), [0031](0031-property-constraints.md), [0032](0032-sparql-update-stream.md), [0033](0033-backend-stack.md), [0034](0034-frontend-stack.md), [0035](0035-adopting-a-wikibase.md), [0036](0036-openstreetmap-providers.md), [0037](0037-gdelt-provider.md), [0038](0038-page-metadata-and-categories.md), [0039](0039-files-and-media.md), [0040](0040-instance-prerogatives.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0043](0043-lua-modules.md), [0044](0044-tenant-relative-ids.md), [0045](0045-table-content-model.md), [0046](0046-primary-tenant.md), [0047](0047-special-pages.md), [0048](0048-notation.md), [0049](0049-boards.md), [0051](0051-page-redirects.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [0056](0056-security-model.md), [0057](0057-web-tier.md), [0058](0058-packed-record-storage.md), [0059](0059-query-service.md), [0060](0060-scopes.md), [0061](0061-sprints-and-tasks.md), [0062](0062-workspaces.md), [0063](0063-query-namespace.md), [0064](0064-entityschema-and-validation.md), [0065](0065-mediainfo-captions-and-commons.md), [0066](0066-lexemes.md), [0067](0067-proposals.md), [0068](0068-merging-with-upstream.md), [0069](0069-synchronized-talk-pages.md), [0070](0070-shallow-entity-mirroring.md), [0071](0071-derived-statements-from-mirrored-pages.md), [0072](0072-template-mappings.md), [0073](0073-lines-links-and-url-patterns.md), [0074](0074-publishing-a-scope-to-an-external-wiki.md), [0075](0075-mcp-server.md), [0076](0076-dataset-publication.md), [0077](0077-special-version.md), [0078](0078-entity-sources.md), [0079](0079-derived-issuer-codes.md), [0080](0080-tenants-as-entity-sources.md), [0081](0081-recovery-keys-and-continuations.md), [0082](0082-source-form-and-the-shared-view.md), [0083](0083-write-path-in-three-tiers.md), [0084](0084-wikibase-writes-against-the-resolved-view.md), [0085](0085-settings-and-preferences.md), [0086](0086-theming.md), [0087](0087-account-registration-and-challenges.md), [0088](0088-external-identity-providers.md), [0089](0089-the-auth-host-and-instance-logins.md), [0090](0090-converting-talk-pages-comment-by-comment.md), [0091](0091-one-wikibase-adapter.md), [0092](0092-following-recent-changes.md), [0093](0093-fallback-typefaces.md), [0094](0094-accessibility-alternates.md), [0095](0095-the-interface-namespace.md), [0096](0096-adopting-any-mediawiki.md), [0097](0097-versioning-and-skins.md)
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

*Changed by A1.*

*Current text: [24](../architecture/24-documentation.md) §1.1.*

### 2. File, number and title

*Current text: [24](../architecture/24-documentation.md) §2.1.*

### 3. Header

*Changed by A1, A2, A4.*

*Current text: [24](../architecture/24-documentation.md) §2.2.*

### 4. Skeleton

*Changed by A2.*

*Current text: [24](../architecture/24-documentation.md) §2.3.*

### 5. Sections

*Changed by A1, A2.*

*Current text: [24](../architecture/24-documentation.md) §3.1.*

### 6. Changing an ADR: fold, log, mark

*Changed by A1, A2.*

*Current text: [24](../architecture/24-documentation.md) §4, §11.*

### 7. Relation verbs

*Changed by A2.*

*Current text: [24](../architecture/24-documentation.md) §5.*

### 8. The Amendment log

*Changed by A4.*

*Current text: [24](../architecture/24-documentation.md) §6.*

### 9. Consequences

*Current text: [24](../architecture/24-documentation.md) §7.*

### 10. Open questions

*Current text: [24](../architecture/24-documentation.md) §8.*

### 11. Citations

*Changed by A4.*

*Current text: [24](../architecture/24-documentation.md) §9.*

### 12. The checker

*Changed by A1, A2, A4.*

*Current text: [24](../architecture/24-documentation.md) §12, §13.*

### 13. Migration

*Changed by A2, A4.*

*Current text: [24](../architecture/24-documentation.md) §14.*

### 14. Chapters: the current text lives outside the ADR

*Changed by A2.*

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

- **Q1.** ~~**Status.** Whether to start using `Accepted`, and when (at the merge that implements an ADR, or at milestone 1). Every ADR is Proposed by the decision of 2026-09-26, so this is James's call.~~ *Settled by §3: an ADR is Accepted once it is implemented in code (Direction).*
- **Q2.** ~~**Extensions as callouts.** Whether a pure extension, which contradicts nothing, may stay a callout instead of being folded. It is cheaper, and the current text stays correct, but the reader has to assemble it again.~~ *Settled by §6: every change is folded, so the process is the same for all (Direction).*
- **Q3.** ~~**Unconfirmed proposals.** Whether sections Claude proposed and James has not yet confirmed should carry a mark. The review of 2026-09-26 asked for 0018 §6 and §10 to be read first for this reason.~~ *Settled by §1: no mark; what an ADR says is decided until a later change says otherwise (Direction).*
- **Q4.** ~~**The generated index.** Whether `INDEX.md` is committed and kept current by the checker, or built only in CI.~~ *Settled by §12: committed, regenerated with `--index`, and checked for staleness (Direction).*
- **Q5.** ~~**Implemented in part.** Code already implements parts of 0006, 0009, 0017, 0048 and others (`scatter-normalize`, `scatter-providers`, `scatter-vocab`, `scatter-wikibase-model`), but none of them whole. Whether an ADR becomes `Accepted` when its first section is implemented or its last, or whether implementation is marked per section. The same question arises when an amendment lands on an ADR that is already `Accepted` and the new text is not yet implemented.~~ *Settled by §3: `Accepted` when implemented in full; an amendment not yet implemented makes it `Accepted with proposed amendment (An, …)` until it is (Direction).*

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
- **Source:** [0098](0098-pointers-from-birth.md) §1, §2, §3, §4, §5, §6, §7
- **Change:** amends §3, §4, §5, §6, §7, §12, §14; extends §13
- **Summary:** A new ADR's Decision sections are pointers from the day it is written: its text goes straight into the chapters, and nothing is relocated later. The chapters' provenance lines are the only map (`MAP.md` and the mapping tables of `map.py` are gone). The `Chapters` header and the "Changes to other ADRs" section, both derivable, are retired. The index is one slim table. Catalogues are registry data with generated chapter tables. 0050's own text moves to chapter 24 with this entry, so the fold is in [24](../architecture/24-documentation.md) §2–§5, §10–§14.

Replaced text (§4):

> ```
> ## Changes to other ADRs     required when Changes is non-empty
> ```

Replaced text (§5):

> - **The first line of a changed section is its provenance line:** *Changed by A3, A6, A9.*, listing every log entry that changed it, in order. A section with no provenance line has not changed since it was written. A `consolidates` entry (§8) is not listed.

Replaced text (§6):

> The changing ADR names *Y* in its header's `Changes`, its section headings and its "Changes to other ADRs" table, which gives *Y*'s log ID.

Replaced text (§7):

> Six verbs, and no others, in headings, log entries and "Changes to other ADRs":

Replaced text (§12):

> 6. "Changes to other ADRs" rows that agree with the header's `Changes` and, for a target in this format, with the target's log.
>
> …
>
> **The index** lists every ADR with its title, status and last update, the ADRs it changes and the ADRs that change it. For an ADR in this format, what changes it is read from its log and its settled questions. For one in the old format, it is read from `Amended by`. What an ADR changes is the inverse, so the two columns cannot disagree. Every change that adds or amends an ADR commits the regenerated index with it.

Replaced text (§14):

> **The architecture chapters hold the current text.** `docs/architecture/` is the reference description of the system by subject: twenty-four chapters assembled from the Decision sections of every ADR, with the duplicates merged and one home for each table that spans ADRs. `docs/architecture/MAP.md`, generated by `docs/architecture/map.py`, assigns every numbered Decision section of every ADR to the chapter or chapters that absorb it.
>
> …
>
> A section `MAP.md` assigns to no chapter (this ADR; the changelog stubs of 0005 §8 and 0013 §12) keeps its text. A Decision section is either full text or a pointer, never both.
>
> **The header names the chapters.** A seventh field, `Chapters`, after `Uses` (§3): linked chapter numbers, ascending, every chapter that holds this ADR's current text. Required once any section is a pointer.
>
> …
>
> **A new ADR carries its full Decision** until a chapter absorbs it. Writing or amending an ADR is then two commits' worth of work in one: the decision in the ADR, the current text in the chapter, `map.py` run so that `MAP.md` names the new sections, and the pointer put in place.
>
> **The checker (§12) gains:**
>
> 12. A pointer names an existing chapter section, and that section's provenance line cites the pointing ADR section. `docs/architecture/map.py` checks the inverse: every mapped section is cited in its chapter, and `MAP.md` is current.
> 13. The `Chapters` header lists exactly the chapters the ADR's pointers name.

### A3. Current text relocated to chapter 24

- **Date:** 2026-10-30
- **Source:** [0098](0098-pointers-from-birth.md) §6
- **Change:** relocates §1–§14
- **Summary:** The Decision's current text, as amended by A2, now lives in [24](../architecture/24-documentation.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.

### A4. Status from the code

- **Date:** 2026-10-30
- **Source:** [0099](0099-status-from-the-code.md) §1, §2, §3, §4, §5
- **Change:** amends §3, §8, §11, §12; extends §13
- **Summary:** The Status header is written from the code: `Proposed`, `Accepted` and `Accepted with proposed amendment` are computed by `check_adrs.py --index` from `implements NNNN §N (An)` claims in src and test citations, read by `tools/coverage.py`; `Superseded` and `Withdrawn` stay hand-written. Code citations gain the claim form. A decision made while building is logged by the agent in the same change with Source `Direct: Claude, while building …; unconfirmed`, and the index lists such entries. The index gains *Built*, *Open* and *Unconfirmed* columns; `coverage.py --todo` is the derived backlog. The folds are in [24](../architecture/24-documentation.md) §2.2, §6, §9, §12, §13 and the new §15.

Replaced text ([24](../architecture/24-documentation.md) §2.2, as it stood):

> **Status follows the code.**
>
> - **`Proposed`** until every section of the Decision is implemented in code. An ADR implemented in part is still `Proposed`.
> - **`Accepted`** once it is implemented in full. The change that completes the implementation sets the status.
> - **`Accepted with proposed amendment (A7, A9)`** when a change lands on an `Accepted` ADR before the code implements it. The text is folded as always, so the chapters state the decision as it now stands, and the status names the log entries that the code does not yet implement. The change that implements an entry removes its ID. When the list is empty, the status is `Accepted` again.
> - **`Superseded by NNNN`** and **`Withdrawn`** come with the log entry that supersedes or withdraws the ADR.
>
> A change to a `Proposed` ADR leaves it `Proposed`.

Replaced text ([24](../architecture/24-documentation.md) §6, as it stood):

> - **Source** is another ADR's sections, or `Direct:` with who decided and on what occasion ("Direct: James, design discussion of 2026-10-01"). For a direct decision the Summary holds the decision in full, because nowhere else does.

Replaced text ([24](../architecture/24-documentation.md) §9, as it stood):

> - **Code cites ADR sections**, as `0013 §7` or `0013 §7 rule 2` in a comment; `tools/coverage.py` reads them and `tools/adr.py code 0013 §7` lists them.

Replaced text ([24](../architecture/24-documentation.md) §13, as it stood):

> `docs/decisions/INDEX.md` is generated by `check_adrs.py --index` and committed; a stale index fails the check. It is one table, one row per ADR: number, title, status, the date of its last change, the chapters its pointers name, and the ADRs that changed it (read from its log and its settled questions). ADR numbers in the last two columns are plain, not linked, so that a row is one line and an amendment changes one row. What an ADR changes is its own header's `Changes`, and is not repeated here.
>
> The index is for finding an ADR by title and seeing at a glance what has touched it. For anything finer — which section, which entry, which chapter text — `tools/adr.py` reads the ADRs and chapters directly.
