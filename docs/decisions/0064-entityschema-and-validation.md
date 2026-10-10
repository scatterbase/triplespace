# 0064. EntitySchema and validation

- **Status:** Proposed
- **Date:** 2026-10-05
- **Updated:** 2026-10-09 (A4)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0047](0047-special-pages.md), [0060](0060-scopes.md), [0061](0061-sprints-and-tasks.md), [0062](0062-workspaces.md)
- **Uses:** [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0010](0010-site-ui.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0017](0017-entity-id-grammar.md), [0024](0024-subsidiary-accounts.md), [0031](0031-property-constraints.md), [0059](0059-query-service.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)
- **Chapters:** [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [15](../architecture/15-structured-pages.md), [18](../architecture/18-api.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md)

## Context

Wikidata's EntitySchema extension stores **ShEx** shapes as pages in namespace 640, each with an ID of the form `E1`, multilingual labels, descriptions and aliases, and a ShExC text. A schema says what an entity of some kind should look like — a human has a `P31` of `Q5`, a date of birth with a reference, at most one sex or gender — and Wikidata validates entities against schemas in external tools, one entity at a time. Property constraints ([0031](0031-property-constraints.md)) express per-property rules; a schema expresses a *shape*, across properties, for a class of entities, and is what a WikiProject means by its data model.

Triplespace reserved the `EntitySchema` model ([0041](0041-content-models.md) §3) and its special pages ([0047](0047-special-pages.md) §10), and 0041 Q6 asked that the namespace be reserved too. With scopes ([0060](0060-scopes.md)), sprints ([0061](0061-sprints-and-tasks.md)) and workspaces ([0062](0062-workspaces.md)) in place, a schema bound to a scope and validated as a projection is what turns "our data model" from a wiki page into a report, a task list and a filter.

### Direction

James's direction, from the design discussion of 2026-10-05:

- **Do what Wikidata does: namespace 640 and a minted entity type `E`.** Then the focus can be on validation.

## Decision

### 1. An entity type with a text body

*Current text: [15](../architecture/15-structured-pages.md) §6.1.*

### 2. The `EntitySchema` namespace (extends 0008 §2; settles 0041 Q6)

*Current text: [15](../architecture/15-structured-pages.md) §6.2.*

### 3. The `EntitySchema` content model (extends 0041 §3)

*Current text: [15](../architecture/15-structured-pages.md) §6.2.*

### 4. Validation: `scatter-shex`

*Changed by A2.*

*Current text: [15](../architecture/15-structured-pages.md) §6.3.*

### 5. Binding a schema to a scope, and the report (extends 0013 §5.6 and §7)

*Changed by A4.*

*Current text: [15](../architecture/15-structured-pages.md) §6.4.*

### 6. Where conformance shows (extends 0060 §4; extends 0061 §4; extends 0062 §3; extends 0042 §5)

*Current text: [15](../architecture/15-structured-pages.md) §3.3, §4.3, §6.5.*

### 7. Special pages, API and search (amends 0047 §10; extends 0047 §9; extends 0012 §5)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §11.1; [18](../architecture/18-api.md) §2.1, §2.3, §3.2, §6; [21](../architecture/21-special-pages.md) §6.4, §7, §8.*

### 8. RDF

*Changed by A1.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §5.9.*

### 9. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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
- ~~**Test plan.** `scatter-shex` against the ShEx test suite's subset and against a sample of Wikidata's schemas; depth-limited and exact results compared on a fixture; a subject's write re-validates within one write; a schema edit rechecks its scopes; the `conforms` scope equals the report; `wbgetentities` on `E` round-trips Wikidata's page JSON.~~ *A subject's write re-validates within the consumer's lag, never within the write; the rest of the plan stands (A4).*

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

## Amendment log

### A1. Entity schemas have no RDF

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §8
- **Summary:** Entity schemas have no RDF: no terms on an entity node, no `wikibase:EntitySchema` type. [wikibase-compat.md](../api/wikibase-compat.md) §2's description already says so and stands. (PENDING B8)

Replaced text (§8):

> The schema's **terms** go on its entity node in the dump as any entity's do (`rdfs:label`, `schema:description`, `skos:altLabel`), typed `wikibase:EntitySchema`, under the entity IRI rules of [0015](0015-record-format-and-partition-registry.md) §5.

### A2. Unsupported ShEx features: saved, not validated

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §4
- **Summary:** A schema using unsupported ShEx features (`IMPORT`, `EXTERNAL`, semantic actions, annotations other than `rdfs:label`/`rdfs:comment`, an undefined prefix) is saved and marked `unvalidatable`: "not validated", not "refused at save". (PENDING F12)

Replaced text (§4):

> **Refused at save**: `IMPORT`, `EXTERNAL`, semantic actions and annotations other than `rdfs:label`/`rdfs:comment`, and any prefix the tenant's dump does not define. A schema that uses them is saved but marked `unvalidatable`, with the reason on its page, since a mirrored Wikidata schema may use them and must still mirror.

### A3. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [15](../architecture/15-structured-pages.md), [18](../architecture/18-api.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A4. Validation runs in tier 3 only, under a binding cap

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §3, §6
- **Change:** amends §5
- **Summary:** Validation against a bound schema is a tier-3 consumer that follows the scope projection, consuming composition events in its own queue, outside every transaction, with its own lag shown on the schema page beside "checked at"; it never runs inline in a write or in a fan-out budget, and only the ad hoc check of §6 runs on request. Its triggers are the `schema:{id}` rows of `view.scope_trigger`. Binding a schema to a scope is refused at scope save with `ts-schema-binding-cap` when the sum over the tenant of bound-schema × scope size (each binding's scope `count`, or `scopes.max_members` for a scope not yet computed) would exceed `schemas.max_bound` (`site`, ceiling-bounded; default 1,000,000), unless an operator raises the ceiling; the refusal names the bindings that fill the cap. The projection is view-derived: a rebuild re-validates every bound subject from `view` as a job. The ledger names §9; the crate table did not change. (REVIEW G10)

Replaced text ([15](../architecture/15-structured-pages.md) §6.4, as it stood):

> **When it runs.** In step 7 of [0013](0013-postgres-storage.md) §7 after the scope projection ([03](../architecture/03-storage-caches-and-search.md) §6.1): a **subject's own write** re-validates the subject against every schema bound to a scope it is in, within the fan-out budget; **a scope's membership change** validates joining members and drops leaving ones; **a schema's revision** re-validates every member of every scope bound to it, as a job; and a periodic **`schemas.recheck`** job (default daily) re-validates subjects whose referenced nodes may have changed, which the subject's own write does not see. `schemas.max_subjects` (default `scopes.max_members`) bounds a schema's materialized report, truncated with the scope notice. The report writes no record and no activity row, as the constraint and scope projections do not.
