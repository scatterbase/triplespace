# 0064. EntitySchema and validation

- **Status:** Proposed
- **Date:** 2026-10-05
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0047](0047-special-pages.md), [0060](0060-scopes.md), [0061](0061-sprints-and-tasks.md), [0062](0062-workspaces.md)
- **Uses:** [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0010](0010-site-ui.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0017](0017-entity-id-grammar.md), [0024](0024-subsidiary-accounts.md), [0031](0031-property-constraints.md), [0059](0059-query-service.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

Wikidata's EntitySchema extension stores **ShEx** shapes as pages in namespace 640, each with an ID of the form `E1`, multilingual labels, descriptions and aliases, and a ShExC text. A schema says what an entity of some kind should look like — a human has a `P31` of `Q5`, a date of birth with a reference, at most one sex or gender — and Wikidata validates entities against schemas in external tools, one entity at a time. Property constraints ([0031](0031-property-constraints.md)) express per-property rules; a schema expresses a *shape*, across properties, for a class of entities, and is what a WikiProject means by its data model.

Triplespace reserved the `EntitySchema` model ([0041](0041-content-models.md) §3) and its special pages ([0047](0047-special-pages.md) §10), and 0041 Q6 asked that the namespace be reserved too. With scopes ([0060](0060-scopes.md)), sprints ([0061](0061-sprints-and-tasks.md)) and workspaces ([0062](0062-workspaces.md)) in place, a schema bound to a scope and validated as a projection is what turns "our data model" from a wiki page into a report, a task list and a filter.

### Direction

James's direction, from the design discussion of 2026-10-05:

- **Do what Wikidata does: namespace 640 and a minted entity type `E`.** Then the focus can be on validation.

## Decision

### 1. An entity type with a text body

**`entityschema` is an entity type**, local `E1`, foreign `WDE1`, hosted in namespace 640 (§2) under the `EntitySchema` model (§3), as items are hosted in 120 under `wikibase-item`. It has **terms** — labels, descriptions, aliases — through the term machinery every entity has ([0013](0013-postgres-storage.md) §5, `view.term`), so it is searched, shown in the viewer's language and resolved by label like any entity. It has **no statements and no sitelinks**, as on Wikidata. Its content is a **text body**, `schemaText`, in ShExC.

The JSON is EntitySchema's own page content, so that a Wikidata schema mirrors unchanged:

```json
{ "id": "E1", "serializationVersion": "3.0", "type": "ShExC",
  "labels": { "en": "human" }, "descriptions": { "en": "a person" }, "aliases": {},
  "schemaText": "PREFIX wd: <http://www.wikidata.org/entity/> …\nstart = @<human>\n<human> { wdt:P31 [ wd:Q5 ] ; … }" }
```

**Mirroring.** Wikidata's schemas are not in its entity dumps; the Wikidata adapter reads them as pages of namespace 640 from the XML dump or `prop=revisions`, and writes `put` records for `WDE` entities as for any entity ([0002](0002-source-graphs-and-mass-ingest.md) §4). The Wikidata provider gains the type `E` in `providers.toml`. A mirrored schema is validated against local data like a local one (§5).

**Clusters.** Schemas may be `same-as` one another ([0004](0004-identity-clusters-and-equivalence.md)); the canonical schema's text is what is validated against. **Local overlays** ([0002](0002-source-graphs-and-mass-ingest.md) §7) on a foreign schema may change its terms; the text body is replaced whole, not overlaid, because a ShEx text has no statement-level parts.

### 2. The `EntitySchema` namespace (extends 0008 §2; settles 0041 Q6)

| Number | Canonical name | Kind | Allowed models | Default model |
|---|---|---|---|---|
| 640 | `EntitySchema` | `pages` | `EntitySchema` | `EntitySchema` |
| 641 | `EntitySchema talk` | `pages` | `triplespace-talk` | `triplespace-talk` |

The EntitySchema extension's numbers, implemented with its meaning, as [0063](0063-query-namespace.md) §2 does for 124. Titles are entity IDs (`EntitySchema:E1`, `EntitySchema:WDE1`), normalized as entity namespaces are ([0008](0008-namespaces-and-document-pages.md) §3); `Special:EntityPage/E1` resolves.

### 3. The `EntitySchema` content model (extends 0041 §3)

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `EntitySchema` | EntitySchema | entity (`entityschema`) | main | `application/json` | No | 640 |

An **entity** model: the page is the entity's view; edits go through the forms and modules of §7, never through `action=edit`, as for items. The content is the JSON of §1. `scatter-wikibase-model` gains the `entityschema` entity type: terms, and a `schema_text` body with a `schema_type` of `ShExC`.

### 4. Validation: `scatter-shex`

**A new pure crate, `scatter-shex`**, implements ShExC parsing and validation:

- **The language**: ShEx 2.1 as Wikidata's schemas use it — shapes, shape references, triple constraints with cardinalities, value sets (IRIs, literals, stems, exclusions), node constraints (datatypes, facets), `AND`/`OR`/`NOT`, `EXTRA`, `CLOSED`, `start`, and the `wikibase:`, `wd:`, `wdt:`, `p:`, `ps:`, `pq:`, `pr:`, `prov:` prefixes bound as the dump binds them ([wikibase-compat.md](../api/wikibase-compat.md) §5.1). **Refused at save**: `IMPORT`, `EXTERNAL`, semantic actions and annotations other than `rdfs:label`/`rdfs:comment`, and any prefix the tenant's dump does not define. A schema that uses them is saved but marked `unvalidatable`, with the reason on its page, since a mirrored Wikidata schema may use them and must still mirror.
- **The data**: validation runs over a `TripleSource` trait — `triples(subject) -> iterator` and `exists(iri)`. The default source renders the subject's triples through `scatter-wikibase-rdf` (format 1.0.0, full and truthy), exactly as the dump would, from the resolved JSON in `view`. A shape reference to another node (`wdt:P19 @<place>`) follows into that node's triples to **`schemas.max_depth`** (default 2); past the depth, a referenced shape is checked for its node constraints only, and the report says `depth_limited`. With the query service on ([0059](0059-query-service.md)), a tenant may set `schemas.source = query` to validate against the store instead, exact to any depth and lagging by the cursor.
- **The result**: `conforms`, and for a failure the **reasons**: which triple constraint of which shape failed on which node, in ShEx's result-shape-map form, which is what Wikidata's validators show and what the entity page can point at a statement.

Validation is pure and bounded: one subject, one schema, a depth, a triple budget (`schemas.max_triples`, default 10,000).

### 5. Binding a schema to a scope, and the report (extends 0013 §5.6 and §7)

**A schema applies where a scope says it does.** A scope definition ([0060](0060-scopes.md) §4) gains **`schemas`**: a list of schema IDs. Every member of the scope is validated against each. A schema has no statements, so the binding lives on the scope, and a schema's page lists the scopes bound to it through `page_link`. A tenant that wants "every human is checked against E10" writes a scope `statement: P31 = Q5` with `schemas: ["E10"]`.

```sql
CREATE TABLE view.schema_report (
  tenant text NOT NULL, schema_id text NOT NULL,
  subject_kind text NOT NULL, subject_id text NOT NULL,
  conforms boolean, reasons jsonb,          -- NULL conforms: not yet checked, or unvalidatable
  depth_limited boolean NOT NULL DEFAULT false,
  checked_at timestamptz, subject_revid bigint, schema_revid bigint,
  PRIMARY KEY (tenant, schema_id, subject_kind, subject_id)
);
CREATE INDEX ON view.schema_report (tenant, schema_id, conforms);
CREATE INDEX ON view.schema_report (tenant, subject_kind, subject_id);
```

**When it runs.** In step 7 of [0013](0013-postgres-storage.md) §7 after the scope projection: a **subject's own write** re-validates the subject against every schema bound to a scope it is in, within the fan-out budget; **a scope's membership change** validates joining members and drops leaving ones; **a schema's revision** re-validates every member of every scope bound to it, as a job; and a periodic **`schemas.recheck`** job (default daily) re-validates subjects whose referenced nodes may have changed, which the subject's own write does not see. `schemas.max_subjects` (default `scopes.max_members`) bounds a schema's materialized report, truncated with the scope notice. The report writes no record and no activity row, as the constraint and scope projections do not.

### 6. Where conformance shows (extends 0060 §4; extends 0061 §4; extends 0062 §3; extends 0042 §5)

| Surface | Form |
|---|---|
| **The schema page** | Tabs: Schema (the ShExC, highlighted), **Conformance** (counts per bound scope: conforming, failing, unchecked; the failing subjects with their first reason), Scopes (bound through `page_link`), talk, history |
| **The entity page** | A marker beside the identity line, "Does not conform to *human* (E10): missing `P569`", in the style of the constraint markers of [0031](0031-property-constraints.md) §3, with the reason pointing at the statement or its absence; **Check against a schema** in the overflow menu runs an ad hoc check against any schema, not just the bound ones |
| **Scope kind `conforms`** ([0060](0060-scopes.md) §4) | `{ "conforms": { "schema": "E10", "not": true } }`: the subjects of the schema's report with `conforms = false` (or true). A scope over a report, so "humans failing E10" is a scope and therefore a table, a feed and a watch |
| **Sprint rule `schema-violation`** ([0061](0061-sprints-and-tasks.md) §4) | `{ "kind": "schema-violation", "schema": "E10" }`: a task per member whose report says `false`; resolved when it says `true`. The task's action opens the entity page at the first failing constraint |
| **Block `#conformance`** ([0062](0062-workspaces.md) §3) | `{{#conformance:E10|scope=subjects}}`: the counts and a bar; a `scoped` block on the schema and the scope |

### 7. Special pages, API and search (amends 0047 §10; extends 0047 §9; extends 0012 §5)

**Special pages** move from `reserved` to `served`: `NewEntitySchema` (labels, description, aliases and text; appends the first record), `EntitySchemaText/{id}` (the ShExC as `text/shex`, which external validators fetch), `SetEntitySchemaLabelDescriptionAliases`. The schema page's own **Edit schema** form appends a new record with the changed text. A new page, **`CheckEntitySchema`** (origin `triplespace`), takes a schema and a subject and shows the result-shape map.

**Action API**: `wbgetentities&ids=E1` returns the JSON of §1 with `type: "entityschema"`, as EntitySchema's `wbgetentities` support does not exist on Wikidata but 0041 §7's MediaInfo precedent allows; `wbsearchentities&type=entityschema`; `wbsetlabel`, `wbsetdescription`, `wbsetaliases` on `E` IDs; `action=entityschema&text=` for the body. **REST**: `GET /entity/E1`; `GET /schema/{id}/report?conforms=&after=`; `POST /schema/{id}/check` with a subject (ad hoc; rate class `query`); `GET /entity/{id}/schemas` (the schemas bound to the entity through its scopes, with its results).

**Search** indexes a schema's terms as any entity's ([0014](0014-caches-and-search.md) §7), and the schema text as a plain-text field.

### 8. RDF

The schema's **terms** go on its entity node in the dump as any entity's do (`rdfs:label`, `schema:description`, `skos:altLabel`), typed `wikibase:EntitySchema`, under the entity IRI rules of [0015](0015-record-format-and-partition-registry.md) §5. The ShExC text is **not** in RDF; it is served at `Special:EntitySchemaText` and `GET /entity/E1`. Wikidata exports no schema RDF, so nothing is contradicted; Q3 asks whether the text belongs in the full dump.

### 9. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-shex` | **New, layer 2, pure.** ShExC parsing, the supported subset and its refusals, validation over `TripleSource` with depth and triple budgets, result-shape maps |
| `scatter-wikibase-model` | The `entityschema` entity type: terms and a `schema_text` body |
| `scatter-wikibase-rdf` | The schema entity node with its terms; the `TripleSource` over a rendered entity |
| `scatter-adapter-wikidata` | Schemas from namespace 640 pages, as `put` records for `WDE` |
| `scatter-scope` | The `schemas` key and the `conforms` kind |
| `scatter-tasks` | The `schema-violation` rule |
| `scatter-wikitext-expand`, `triplespace-render` | `#conformance` |
| `triplespace-projections` | `view.schema_report`, the validation projection, the recheck and schema-revision jobs |
| `triplespace-api-action`, `triplespace-api-rest` | The modules and routes of §7 |
| `triplespace-ui` | The schema page, the entity marker and ad hoc check, the special pages |

## Alternatives considered

- **A page in 640 without an entity type**, as the extension literally has it. Terms would be page terms, open since [0038](0038-page-metadata-and-categories.md) Q4; mirroring would need a page-shaped mirror. The entity type gives terms, search, clusters and mirroring with nothing new.
- **Validation against the query service only.** Exact, but the service is optional and lags; the default source renders from `view` and is always there, with the query source as the exact option.
- **Validation as a constraint type** of [0031](0031-property-constraints.md). A constraint belongs to a property; a schema belongs to a class of subjects. The report mirrors 0031's shape so the UI treats them alike, and nothing else is shared.
- **Binding on the schema** ("applies to Scope:X"). A schema has no statements, and the scope already is the thing that names a set of subjects.

## Consequences

- **A data model is a report.** Bound to a scope, a schema says continuously which subjects fail and why; a sprint turns that into tasks and a workspace shows it.
- **Wikidata's schemas mirror and run here**, against local data, unchanged; what they cannot express in our subset is marked, not lost.
- **Validation is bounded** by depth and triples, and exact only against the store; the report says which.
- **0041 Q6 is settled**, and the three reserved special pages are served.
- **Test plan.** `scatter-shex` against the ShEx test suite's subset and against a sample of Wikidata's schemas; depth-limited and exact results compared on a fixture; a subject's write re-validates within one write; a schema edit rechecks its scopes; the `conforms` scope equals the report; `wbgetentities` on `E` round-trips Wikidata's page JSON.

## Open questions

- **Q1. `IMPORT`.** Schemas importing other schemas by IRI are common on Wikidata; resolving an import to a local or mirrored `E` and validating the composition is the next step after the subset.
- **Q2. Validating pages.** Page subjects have statements ([0038](0038-page-metadata-and-categories.md) §1) and a `pages` scope may carry `schemas`; whether the page node's triples suffice for useful shapes.
- **Q3. Schema text in RDF.** A full-dump consumer may want the ShExC; `schema:text` on the node, or a separate shape graph.
- **Q4. Exact validation by default.** Whether `schemas.source = query` should be the default when the service is on, accepting the lag.
- **Q5. Autofix hints.** A failed cardinality or value set often has one obvious fix; whether the report may propose a change set the editor confirms.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §9 | amends | 0005 A67 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §2 | extends | 0008 A28 |
| [0012](0012-api-requirements.md) §5 | §7 | extends | 0012 A44 |
| [0013](0013-postgres-storage.md) §5.6, §7 | §5 | extends | 0013 A33 |
| [0041](0041-content-models.md) §3 | §3 | extends | 0041 A11 |
| [0042](0042-template-expansion-and-parsoid.md) §5 | §6 | extends | 0042 A10 |
| [0047](0047-special-pages.md) §9, §10 | §7 | amends | 0047 A9 |
| [0060](0060-scopes.md) §4 | §5, §6 | extends | 0060 A3 |
| [0061](0061-sprints-and-tasks.md) §4 | §6 | extends | 0061 A2 |
| [0062](0062-workspaces.md) §3 | §6 | extends | 0062 A2 |

## References

- [Extension:EntitySchema](https://www.mediawiki.org/wiki/Extension:EntitySchema); [Wikidata:WikiProject Schemas](https://www.wikidata.org/wiki/Wikidata:WikiProject_Schemas)
- [Shape Expressions Language 2.1](https://shex.io/shex-semantics/) and the [ShEx test suite](https://github.com/shexSpec/shexTest)
- [0031](0031-property-constraints.md): the report shape and UI markers this ADR mirrors; [0060](0060-scopes.md) §5: the projection pattern
