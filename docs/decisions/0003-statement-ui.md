# 0003. Statement UI

- **Status:** Proposed
- **Date:** 2026-09-25
- **Author:** James Hare / Claude Opus
- **Amended by:** [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md), [0012 — API requirements for the site UI](0012-api-requirements.md), [0027 — Preferences, private state and portability](0027-preferences-and-portability.md) (§5 settles the shape-pin open question), [0031 — Property constraints](0031-property-constraints.md) (§1 binds the constraint vocabulary by role through §7; §3 extends §9 with the constraint marker)
- **Related:** [0000 — Initial proposition](0000-init.md), [0001 — Revision metadata in RDF](0001-revision-metadata-rdf.md), [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md), [MediaWiki API contract](../api/mediawiki-compat.md)

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

1. **Show the exception, not the rule.** Rank, source graph, shared qualifiers and shared references are drawn only where they differ within a statement group. Anything that is the same everywhere is stated once, or not at all.
2. **Name the effect, not the mechanism.** UI text says "Best value", "Deprecated" and "Corrected here". Data-model terms such as rank, `PreferredRank` and graph appear in tooltips, the API and exports.
3. **The data picks the layout.** A group's shape (§3) is computed from its statements, so the same property can be a row of chips on one item and a table on another. Editors can override the result, but the override is a view preference, not data.
4. **Deterministic and testable.** Given the same entity JSON and role map (§7), shape detection always returns the same shape. This means the classifier can be run over a Wikidata dump and its results measured.
5. **Nothing is lost.** Every value that is folded away or hoisted stays one click away. Every shape has a plain list fallback that shows every statement, qualifier and reference.

### 2. The UI reads canonical Wikibase JSON

The UI takes three inputs:

- the entity's canonical JSON ([wikibase-compat.md](../api/wikibase-compat.md) §3);
- the labels of the entities and properties it links to;
- the data type of each property.

A stock Wikibase serves all three, so the same frontend can drive Triplespace or any Wikibase, Wikidata included. Information that only Triplespace has comes from a separate provenance response (§6). The canonical JSON stays the same as Wikibase's, as [0001](0001-revision-metadata-rdf.md) §3 requires.

A **statement group** is all the statements for one property on one entity, after reconciliation ([0002](0002-source-graphs-and-mass-ingest.md) §3).

### 3. Shape detection

Each statement group gets exactly one shape. The rules are checked in order, and the first one that matches wins.

Terms:

- **V** is the set of statements in the group with normal or preferred rank. Deprecated statements are handled separately (§4).
- **n** is the number of statements in V.
- **coverage(q)** is the fraction of statements in V that have at least one qualifier with property q.
- **Roles** such as `time-point` and `time-start` come from the role map (§7).

| Order | Shape | Matches when | Renders as |
|---|---|---|---|
| 1 | Single | n = 1 | The value as a headline, with its qualifiers inline as "key: value" |
| 2 | Timeline | n ≥ 2 and coverage(`time-start`) ≥ 0.8 | Rows on a shared time axis, newest first; the best value leads |
| 3 | Series | n ≥ 4, the main value is a `quantity`, and coverage(`time-point`) ≥ 0.8 | The best value as a headline, a line chart, and a switch to a table |
| 4 | Table | n ≥ 3 and some qualifier q has coverage(q) ≥ 0.8 | One column per qualifier key; a Matrix view where allowed (below) |
| 5 | Chips | No statement in V has qualifiers, and the main value is a `wikibase-item`, `string` or `external-id` | Chips that wrap onto new lines |
| 6 | List | Anything else | One row per statement, with its qualifiers inline beneath |

**Table columns.**

- Every qualifier key with coverage ≥ 0.5 becomes a column, in the order the keys first appear.
- A statement with no value for a column leaves that cell blank.
- Qualifier keys below 0.5 coverage are listed under the row as extra qualifiers.
- A key with the same value on every statement is hoisted instead (§5).

**Row order.** Rows are sorted by their column values, from left to right. When a column's value repeats down consecutive rows, only the first row of the run shows it. The hidden cells still carry the value as accessible text.

**Matrix.** A Matrix view is offered when all of these hold:

- the table has exactly two columns;
- each column has 12 or fewer distinct values;
- no two statements share the same pair of values.

In the Matrix, a cell with no statement reads "no value".

**Large groups.** When n > 25, the first 10 values are shown inline, followed by "Open full view". The full view is a page for that one property on that one entity, with paging, sorting and filtering. Because an item is a view and not a page, the full view is just another query.

**Overrides.** An editor can pin a different shape for a property, for the whole instance or for one entity. The pin is stored as a view preference, never as a statement.

### 4. Rank

Rank is shown only when it changes what a reader or a query sees.

| Ranks in the group | What is drawn |
|---|---|
| All normal | Nothing |
| One or more preferred | Preferred values lead the group, with a "Best value" badge. In the Single, Chips and List shapes, the other values fold under "N other values". In the Table and Series shapes they stay in place, and the preferred row carries the badge. |
| Some deprecated | They are taken out of V and folded under "N deprecated". Each is struck through and labeled with its `deprecation-reason` qualifier. |
| Deprecated only | The group reads "No current value", with the deprecated values expanded beneath. |

- The "Best value" badge has a tooltip: "Queries and infoboxes return only this value". This is what truthy triples mean in practice.
- Rank is set from each value's menu, through two items:
  - **"Make this the best value"**, which reads "Also a best value" when a preferred value already exists;
  - **"Mark as deprecated…"**, which asks for a reason and saves it as the `deprecation-reason` qualifier.
- A local rank override on a mirrored statement carries a "Corrected here" chip ([0002](0002-source-graphs-and-mass-ingest.md) §7). Expanding the chip shows:
  - the upstream value and its rank;
  - the ranks as exported;
  - two actions, "Report upstream" and "Undo correction".

  The chip also has a *redundant* state, for when upstream now agrees with the correction, and a *dangling* state, for when upstream has removed the statement the override points at.

### 5. Shared qualifiers and references

**Shared qualifiers.** When n ≥ 2 and a qualifier key has coverage 1.0 with the same value on every statement, the key is taken out of the rows. It is shown once, under "Same for all N", in the group footer. For the Series shape, it goes in the side panel instead.

**References** are compared by the `hash` that canonical JSON gives each reference.

- If one hash appears on every statement in V, that reference is shown once, as footnote 1, labeled "Cited by all N values".
- Otherwise, distinct references are numbered in the order they first appear in the group. Each row shows its footnote numbers, and the footnotes are listed at the foot of the group.
- Statements with no reference carry no marker. The footer counts them, for example "2 values have no source".

### 6. Provenance

Canonical JSON does not say which graph asserts a statement. Triplespace serves that separately, as a **provenance response** keyed by statement ID. For each statement it gives:

- the source graphs that assert it;
- whether it is overridden, and by what;
- its correction state (§4).

Where there is no provenance response, as on a stock Wikibase, the UI draws no provenance.

Where there is one:

- The item header names the **dominant source**, the graph that asserts the most statements. The header also has a toggle between the resolved view, each mirror graph on its own, and the local graph only.
- A statement gets a source chip (`WD`, `OA` or "Local") only when its graph is not the dominant one. A statement asserted by two graphs shows the dominant one, and the chip's hover lists both.
- A "Where this comes from" panel lists, for each graph:
  - how many statements it asserts;
  - when it last synced, for a mirror;
  - the entity's retention policy ([0002](0002-source-graphs-and-mass-ingest.md) §5);
  - the actions to change retention or convert the entity to a local one ([0002](0002-source-graphs-and-mass-ingest.md) §6).

### 7. Semantic roles

Property IDs differ between Wikibases, so shape detection and the rank rules refer to **roles**. Each instance maps its own properties onto those roles.

| Role | Used by | Wikidata property |
|---|---|---|
| `time-point` | Series shape, sort order | P585 point in time |
| `time-start` | Timeline shape | P580 start time |
| `time-end` | Timeline shape | P582 end time |
| `deprecation-reason` | The deprecated fold's labels; "Mark as deprecated…" | P2241 reason for deprecated rank |
| `reference-work` | Footnote titles | P248 stated in |

- **Mirrored properties.** On an instance that mirrors Wikidata, mirrored properties carry the `WDP` prefix, so the map reads `WDP585`, `WDP580` and so on.
- **Local and mirrored forms together.** One role can have both a local and a mirrored property: for example, `P12` and `WDP585` can both be `time-point`.
- **Constraints as hints.** Wikidata properties declare allowed and required qualifiers as property constraints (P2302). When a group has no statements, or only one or two, these constraints can seed the table's columns before any data exists. They are hints only: once there is enough data, detection on that data wins.

### 8. Editing

Editing happens inside the group, and the group keeps its shape. Every action maps to an existing Wikibase write, so the same UI can edit a stock Wikibase.

| UI action | Behavior | Write |
|---|---|---|
| Add value (Table) | A new row appears with one input per column. The first column carries over the value from the row above. A "Same as the other values" checkbox attaches the shared reference and is checked by default. | New statements in one `wbeditentity` |
| Paste rows | Tab-separated rows map to the columns in order. Each cell is parsed as its column's data type. | `wbparsevalue` for each cell, then one `wbeditentity` |
| Save N values | Every pending row is saved as one revision, with an automatic summary. | One `wbeditentity` |
| Make this the best value | Sets the rank to preferred | `wbsetclaim` |
| Mark as deprecated… | Asks for a reason, then sets the rank to deprecated and adds the reason qualifier | `wbsetclaim` |
| Add a qualifier column | Adds an empty column. The qualifier is written only to statements whose cell is filled in. | `wbsetqualifier` for each filled cell |
| Correct a mirrored value | Writes a local-graph override or addition; the mirrored statement is untouched | Triplespace `override` or `add` ([0002](0002-source-graphs-and-mass-ingest.md) §8.2) |

- **Keyboard.** Tab moves between cells, Enter adds another row, and Escape cancels the row being edited. Every input is labeled with its column's property.
- **Conflicts.** Each save carries `baserevid`. If the group has changed since the edit began, the pending rows are applied to the current data, and only the cells that clash are marked. The rest of the save goes through.

### 9. Components

The shapes are arrangements of eleven shared components, not separate widgets.

| Component | Contents |
|---|---|
| Property row | The property label (a link), the value count, and the group card |
| Group card | A header with the sort note, view switch, Add value button and menu; a body holding the shape; a footer with hoisted qualifiers and footnotes |
| View switch | A segmented control listing only the shapes this group allows; hidden when only one applies |
| Value cell | The value, formatted by data type; quantities use tabular numerals |
| Qualifier column | A header with the qualifier property's label, and a cell for each row |
| Footnote marker | A numbered pill linking to its footnote |
| Best value badge | A check icon and "Best value", with the tooltip from §4 |
| Fold | A disclosure button: "N other values" or "N deprecated" |
| Source chip | The provider code in monospace, or "Local" |
| Correction chip | Collapsed, expanded, redundant and dangling states (§4) |
| Value menu | Edit; Make this the best value; Mark as deprecated…; Copy statement ID; History of this value |

The visual tokens (color, type, spacing) on the canvas are placeholders. They are outside the scope of this ADR.

### 10. Prototype against Wikidata first

Because the UI reads canonical JSON (§2), it is built and tested against live Wikidata before Triplespace serves any data.

| Need | Source |
|---|---|
| Statements | `wbgetentities` (`claims`), or REST v1 `GET /entities/items/{id}` (`statements`), normalized to one internal form |
| Labels and property data types | `wbgetentities` with `props=labels` or `props=datatype`, up to 50 IDs per call ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.3) |
| Cross-origin reads | `origin=*` on anonymous Action API requests |
| Writes | The MediaWiki 1.43 + Wikibase reference install ([mediawiki-compat.md](../api/mediawiki-compat.md) §1.2), or test.wikidata.org |
| Provenance | Fixture files keyed by statement ID |

Steps:

1. **Normalize.** Fetch an entity, the labels it links to, and its property data types, and turn them into statement groups.
2. **Classify.** Write shape detection (§3) as a pure function. Test it with fixtures from the canvas examples:
   - P1082 (population) and P6 (head of government) on Q65, Los Angeles;
   - Pluto;
   - the capital of Kazakhstan;
   - an ionic-radius group.
3. **Render.** Build the six shapes, read-only, against live Wikidata.
4. **Audit.** Run the classifier over a sample of the Wikidata JSON dump. Measure how often each shape occurs, and set the thresholds in §3 from what the audit finds.
5. **Edit.** Point the same UI at the reference install and implement §8.
6. **Provenance.** Exercise §6 with fixtures. Switch to Triplespace's own response once it exists.

The prototype lives in `ui/` in this repository. The classifier is its own module, so that a later Rust implementation can reuse its test fixtures.

## Consequences

- **The frontend is decoupled from the backend.** It can be built, tested and demonstrated before Triplespace serves any data, and it also works on a stock Wikibase.
- **One property can look different on different items.** Consistency within an item is traded for fit to the data. The view switch and shape pins make up for it.
- **Rank is harder to find at first glance.** Power users used to the always-visible control have to open the value menu instead. The API and exports are unchanged.
- **Hoisting only catches identical references.** Two references that differ only in, say, their retrieved date have different hashes, so they are numbered separately. Near-duplicates may need their own rule later.
- **The provenance response is new API surface.** Once published, it becomes a compatibility commitment, just as the `scatter.wiki` vocabulary did in [0001](0001-revision-metadata-rdf.md).
- **The thresholds are heuristics.** Changing them after release changes how existing items are laid out. They should be settled by the audit before release.
- **The UI has to branch on the backend's capabilities.** Corrections and retention exist only on Triplespace. Against a stock Wikibase, those controls are hidden, not disabled.
- **Folding and repeat-suppression must stay accessible.** Hidden cells and folded values must remain in the accessibility tree, or screen-reader users lose data that sighted users can still see.

## Open questions

- **Thresholds.** The values in §3 (0.8 and 0.5 coverage; n ≥ 3 for Table, n ≥ 4 for Series, n > 25 for the full view) are starting guesses for the audit to replace.
- ~~**Where shape pins live.** One option is log records in a view-preferences graph; the other is instance configuration outside the log.~~ *Settled by [0027](0027-preferences-and-portability.md) §5: a viewer's pin is the `shapes.pins` preference; a tenant-wide pin is a `view-pin` config record.*
- ~~**The provenance response's form.** It could be a new REST route for each entity, or an Action API `prop` module.~~ *Settled by [0012](0012-api-requirements.md) §5: `GET /entity/{id}/provenance` under `triplespace/v0`, extended by [0004](0004-identity-clusters-and-equivalence.md) §8 (member statements) and [0031](0031-property-constraints.md) §6 (constraint violations).*
- **Several preferred values in Series and Timeline.** It is not settled whether the headline shows all of them or only the most recent.
- **Mixed units in one column,** such as km² and mi². The options are converting to a single unit or adding a unit column.
- **Overlapping intervals in Timeline,** such as co-holders of an office or disputed periods. The options are stacked lanes or marked rows.
- **Foreign values in cells.** It is not settled whether a cell holding an OpenAlex entity on a Wikidata-mirrored item shows its provider code inline or only on hover.

## References

- [Statement UI design canvas](https://claude.ai/artifact/69M2T2HYrkafzearFUoJ4X) (private until shared)
- [Wikidata: Help:Ranking](https://www.wikidata.org/wiki/Help:Ranking)
- [Wikidata: Help:Property constraints portal](https://www.wikidata.org/wiki/Help:Property_constraints_portal)
