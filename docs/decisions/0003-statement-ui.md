# 0003. Statement UI

- **Status:** Proposed
- **Date:** 2026-09-25
- **Updated:** 2026-10-09 (A13)
- **Author:** James Hare / Claude Opus
- **Uses:** [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [MediaWiki API contract](../api/mediawiki-compat.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)
- **Chapters:** [06](../architecture/06-statements-and-properties.md), [19](../architecture/19-site-ui.md)

## Context

Wikibase's entity page draws every statement the same way:

- the main value;
- its qualifiers underneath, one key–value row each;
- a references disclosure;
- a rank control beside it.

This fits Triplespace badly in three ways.

1. **Rank is always drawn, and it rarely matters.** Every statement shows a three-state rank control. Most statement groups hold only normal-rank statements, so the control carries no information. The words "preferred" and "deprecated" mean nothing to a passer-by. What rank *does* does matter: it picks the value that queries return, and it keeps a wrong value on record without presenting it as true.
2. **Tabular data is drawn as a stack.** Many statement groups are tables in disguise, because every value carries the same qualifier keys. Examples:
   - population by point in time;
   - heads of government by start and end time;
   - ionic radius by charge number and coordination number.

   The stacked layout repeats every qualifier label and every reference on every value. Five ionic-radius values that cite the same ISBN take about 1,100 px at 1×.
3. **The UI assumes one page.** A Wikibase item is one MediaWiki page, and its UI assumes everything fits on that page. Under [0000](0000-init.md) and [0002](0002-source-graphs-and-mass-ingest.md), an item is a resolved view over several source graphs. It can link out to further views, such as the full set of values for one property, or a single graph's statements on their own. It also has to show where its statements come from.

Triplespace does not come with MediaWiki's editing UI ([0000](0000-init.md), Consequences), so it needs its own. The design exploration is on the [statement UI canvas](https://claude.ai/artifact/69M2T2HYrkafzearFUoJ4X). This ADR records the rules that UI follows.

## Decision

### 1. Principles

*Current text: [19](../architecture/19-site-ui.md) §1.1, §4.1.*

### 2. The UI reads canonical Wikibase JSON

*Changed by A7.*

*Current text: [19](../architecture/19-site-ui.md) §1.4, §4.2.*

### 3. Shape detection

*Changed by A2, A10.*

*Current text: [19](../architecture/19-site-ui.md) §4.3.*

### 4. Rank

*Current text: [19](../architecture/19-site-ui.md) §4.4.*

### 5. Shared qualifiers and references

*Current text: [19](../architecture/19-site-ui.md) §4.5.*

### 6. Provenance

*Changed by A1, A4, A11.*

*Current text: [19](../architecture/19-site-ui.md) §4.6.*

### 7. Semantic roles

*Changed by A1, A3, A4, A5, A6, A7, A8, A10.*

*Current text: [06](../architecture/06-statements-and-properties.md) §1.1, §1.2.*

### 8. Editing

*Current text: [19](../architecture/19-site-ui.md) §4.7.*

### 9. Components

*Changed by A4.*

*Current text: [19](../architecture/19-site-ui.md) §4.8.*

### 10. Prototype against Wikidata first

*Changed by A12.*

*Current text: [19](../architecture/19-site-ui.md) §5.8.*

## Consequences

- **The frontend is decoupled from the backend.** It can be built, tested and demonstrated before Triplespace serves any data, and it also works on a stock Wikibase.
- **One property can look different on different items.** Consistency within an item is traded for fit to the data. The view switch and shape pins make up for it.
- **Rank is harder to find at first glance.** Power users used to the always-visible control have to open the value menu instead. The API and exports are unchanged.
- **Hoisting only catches identical references.** Two references that differ only in, say, their retrieved date have different hashes, so they are numbered separately. Near-duplicates may need their own rule later.
- ~~**The provenance response is new API surface.** Once published, it becomes a compatibility commitment, just as the `scatter.wiki` vocabulary did in [0001](0001-revision-metadata-rdf.md).~~ *Holds; the vocabulary it compares with is now `scatter:` under `https://scatter.red/terms/v0/` (0001 A2).*
- **The thresholds are heuristics.** Changing them after release changes how existing items are laid out. They should be settled by the audit before release.
- **The UI has to branch on the backend's capabilities.** Corrections and retention exist only on Triplespace. Against a stock Wikibase, those controls are hidden, not disabled.
- **Folding and repeat-suppression must stay accessible.** Hidden cells and folded values must remain in the accessibility tree, or screen-reader users lose data that sighted users can still see.

## Open questions

- **Q1.** ~~**Thresholds.** The values in §3 (0.8 and 0.5 coverage; n ≥ 3 for Table, n ≥ 4 for Series, n > 25 for the full view) are starting guesses for the audit to replace.~~ *Settled by A10: the audit of 2026-10-04 kept every value (Direct).*
- **Q2.** ~~**Where shape pins live.** One option is log records in a view-preferences graph; the other is instance configuration outside the log.~~ *Settled by [0027](0027-preferences-and-portability.md) §5: a viewer's pin is the `shapes.pins` preference; a tenant-wide pin is a `view-pin` config record.*
- **Q3.** ~~**The provenance response's form.** It could be a new REST route for each entity, or an Action API `prop` module.~~ *Settled by [0012](0012-api-requirements.md) §5: `GET /entity/{id}/provenance` under `triplespace/v0`, extended by [0004](0004-identity-clusters-and-equivalence.md) §8 (member statements) and [0031](0031-property-constraints.md) §6 (constraint violations).*
- **Q4. Several preferred values in Series and Timeline.** It is not settled whether the headline shows all of them or only the most recent.
- **Q5. Mixed units in one column,** such as km² and mi². The options are converting to a single unit or adding a unit column.
- **Q6. Overlapping intervals in Timeline,** such as co-holders of an office or disputed periods. The options are stacked lanes or marked rows.
- **Q7. Foreign values in cells.** It is not settled whether a cell holding an OpenAlex entity on a Wikidata-mirrored item shows its provider code inline or only on hover.

## References

- [Statement UI design canvas](https://claude.ai/artifact/69M2T2HYrkafzearFUoJ4X) (private until shared)
- [Wikidata: Help:Ranking](https://www.wikidata.org/wiki/Help:Ranking)
- [Wikidata: Help:Property constraints portal](https://www.wikidata.org/wiki/Help:Property_constraints_portal)

## Amendment log

### A1. Fused statements and linked properties

- **Date:** 2026-09-25
- **Source:** [0004](0004-identity-clusters-and-equivalence.md) §6, §8
- **Change:** extends §6, §7
- **Summary:** The provenance response lists every member statement of a fused statement and its graph. Adapters name their local targets by role. A role map may name either of two properties linked by `equivalent-property`.

### A2. Where shape pins live

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §5
- **Change:** extends §3
- **Summary:** A viewer's pin is the `shapes.pins` preference; a tenant-wide pin is a `view-pin` config record. This settled Q2.

### A3. Resolvers bound by role

- **Date:** 2026-09-27
- **Source:** [0029](0029-resolver-namespaces.md) §1
- **Change:** extends §7
- **Summary:** A resolver namespace is bound to a property by a role, such as `doi`, or to sitelinks.

### A4. Constraints

- **Date:** 2026-09-27
- **Source:** [0031](0031-property-constraints.md) §1, §3, §6
- **Change:** extends §6, §7; amends §9
- **Summary:** The constraint vocabulary is bound by roles. A violating value carries a constraint marker in its value cell, drawn only where there is a violation. The provenance response gains a `constraints` list per statement, so the statement UI needs one request. The constraints-as-hints rule of §7 stands.

Replaced text (§9):

> The shapes are arrangements of eleven shared components, not separate widgets.

### A5. OpenStreetMap roles

- **Date:** 2026-09-28
- **Source:** [0036](0036-openstreetmap-providers.md) §4
- **Change:** extends §7
- **Summary:** `osm-tag`, `osm-tag-text` and `osm-key` are registered with the other roles; the OpenStreetMap adapter's key map names properties by role.

### A6. GDELT roles

- **Date:** 2026-09-28
- **Source:** [0037](0037-gdelt-provider.md) §3–4
- **Change:** extends §7
- **Summary:** GDELT documents and events carry their statements by role: `document-url`, `published`, `source-domain`, `theme`, `location`, the tone roles, `event-type`, `event-date`, `event-actor`, `mentioned-in` and the rest.

### A7. Page and thread roles

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §2, §9
- **Change:** extends §2, §7
- **Summary:** Three roles are added. `subject-page` and `subject-thread` name the items that stand for a document page and a thread as values of Wikidata's *allowed entity types* constraint (Q52004125). `thread-status` names the `string` property that carries a thread's projected status. The statement UI also serves page statements, with the page as subject, in the Page data tab of [0010](0010-site-ui.md) §2.

### A8. The notation-scheme role

- **Date:** 2026-10-01
- **Source:** [0048](0048-notation.md) §4
- **Change:** extends §7
- **Summary:** A new role, `notation-scheme`, names the `string` property whose values restrict which schemes a `wikibase-notation` property accepts. A value in another scheme is refused on the write path.

### A9. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §2–9
- **Summary:** A1–A8 were folded into the Decision, the open questions were numbered, and the consequence that named the `scatter.wiki` vocabulary was struck. No decision changed. Before this, A7 was a blockquote under §7, and the other entries were recorded only in this ADR's header or in other ADRs. The file before conversion is commit `0b26a3a`.

### A10. The thresholds kept; the series-ordinal role

- **Date:** 2026-10-04
- **Source:** Direct: James, 2026-10-04, on the shape audit ([docs/audits/0003-shapes-2026-10-04.md](../audits/0003-shapes-2026-10-04.md))
- **Change:** extends §3, §7
- **Summary:** "Keep the thresholds. A series ordinal role is a good idea." The audit ran `scatter-wikibase-shape` over 50,000 Wikidata items drawn at random. Every coverage threshold of §3 fell in a low-density region, and the minimum sizes were the levers that moved the most groups, so all the values stay as written. 92% of Tables had one column, mostly scholarly author lists ordered by P1545. A new role, `series-ordinal`, makes such a Table a numbered list. Numeric strings in a column compare as numbers. This settles Q1.

### A11. The "About this page" panel on the entity page

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §6
- **Summary:** The entity page has the "About this page" panel of [0010](0010-site-ui.md) §4 beside its "Where this comes from" panel; the scope count ([0060](0060-scopes.md) §8) and the proposal count ([0067](0067-proposals.md) §7) sit in the "About this page" panel. 0010 §4 is extended the same way. (PENDING F16)

### A12. The prototype is `triplespace-ui`

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** supersedes §10
- **Summary:** The prototype is `triplespace-ui` with `scatter-wasm`, server-rendered Rust ([0034](0034-frontend-stack.md) §3, §13), built and tested against live Wikidata as §10 says; the fixture list and the audit step stay. The closing paragraph goes: the prototype does not live in `ui/`, and there is no "later Rust implementation" distinct from it. 0010 §13 is superseded the same way. (PENDING F17)

Replaced text (§10):

> The prototype lives in `ui/` in this repository. The classifier is its own module, so that a later Rust implementation can reuse its test fixtures.

### A13. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [06](../architecture/06-statements-and-properties.md), [19](../architecture/19-site-ui.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
