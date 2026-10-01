# 0031. Property constraints

- **Status:** Proposed
- **Date:** 2026-09-27
- **Author:** James Hare / Claude Fable
- **Amended by:** [0047 — Special pages](0047-special-pages.md) (§4.2 amends §3: `ConstraintReport` defaults to the local graph)
- **Related:** [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (§2: constraints are evaluated on the resolved view of §3), [0003 — Statement UI](0003-statement-ui.md) (§1 binds constraint vocabulary by role, §7; §6 extends §9 with the constraint marker; the "constraints as hints" of §7 stand), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md) (§2 uses §4 and §7), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§9 amends §2: adds `scatter-wikibase-constraints`), [0010 — Site UI](0010-site-ui.md) (§6 extends §2), [0012 — API requirements for the site UI](0012-api-requirements.md) (§7 extends §4 and §5), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§5 extends §5.6 and §7), [0014 — Cache layers and search](0014-caches-and-search.md) (§5), [0018 — Tenants](0018-tenants.md) (§4 follows §6), [0020 — Change feeds](0020-change-feeds.md) (§6: not a feed), [0029 — Resolver namespaces](0029-resolver-namespaces.md) (§2: normalized values), [0030 — Edit filters](0030-edit-filters.md) (§1 settles its property-constraints open question; constraints report, filters gate), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

Wikidata's data quality rests on **property constraints**: statements on a property, using the property `P2302` ("property constraint"), that say what values, qualifiers, references and neighbours a statement using that property ought to have. A constraint is a claim about the data, not a rule the software enforces: the WikibaseQualityConstraints extension checks statements against them and shows a marker beside a violating value, and editors decide what to do. Roughly thirty constraint types exist, from *format* (a regular expression the value must match) and *single value* to *inverse*, *item requires statement* and *distinct values*; each may carry exceptions, a status (`mandatory` or `suggestion`), and a scope.

The earlier ADRs touch this three times without designing it. [0003](0003-statement-ui.md) §7 uses constraints "as hints" to seed a table's columns. [0030](0030-edit-filters.md) §2 keeps data-dependent rules out of edit filters and names property constraints as the place for them: "the value of P31 must be an instance of Q5" is not a gate on a write, it is a report on the data. And James's direction is that constraints stay what they are on Wikidata: **soft recommendations**, never a refusal.

The instance has two advantages over Wikidata here. It **mirrors** Wikidata's properties, so the constraints Wikidata's community has written arrive in the mirror graph as ordinary statements on `WDP31`, `WDP356` and the rest, and apply to any statement using those properties on any entity, local or mirrored, the moment the mirror lands. And it has a **resolved view** ([0002](0002-source-graphs-and-mass-ingest.md) §3) with indexes over identifiers and references ([0013](0013-postgres-storage.md) §5.2), which is what the expensive constraint types need.

## Decision

### 1. A constraint is a statement on a property, read by role (extends 0003 §7)

Constraints are **data**: statements on a property entity whose main property plays the role `constraint`, with the constraint type as the value and the constraint's parameters as qualifiers, exactly Wikidata's `P2302` model. Nothing new is stored to define one; a tenant that mirrors Wikidata has thousands the moment the mirror lands, and a tenant that does not writes them as statements on its own properties.

The vocabulary is bound by **roles** ([0003](0003-statement-ui.md) §7), so that a tenant may use Wikidata's items and properties or its own:

| Role | Wikidata | Meaning |
|---|---|---|
| `constraint` | P2302 | The property that carries constraints |
| `constraint-status` | P2316, with Q21502408 mandatory, Q62026391 suggestion | Severity |
| `constraint-exception` | P2303 | Entities exempt from this constraint |
| `constraint-scope` | P4680, with main / qualifier / reference values | Which snaks the constraint checks |
| `constraint-clarification` | P6607 | Text shown with a violation |
| one role per constraint type | Q21502404 format, Q19474404 single value, Q21502410 distinct values, Q21510865 value type, Q21503250 type, Q21510855 inverse, Q21510862 symmetric, Q21510856 item requires statement, Q21510864 value requires statement, Q21510851 allowed qualifiers, Q21510856 required qualifier, Q21514353 allowed units, Q21510860 range, Q21510859 one of, Q52558054 none of, Q21510854 citation needed, Q52848401 integer, Q51723761 no bounds, Q53869507 property scope, Q64006792 label in language, Q54554025 Commons link, Q55819106 lexeme requires language, and the rest | The constraint type |
| one role per parameter | P2306 property, P2305 item of property constraint, P2308 class, P2309 relation, P1793 format as a regular expression, P2313 maximum value, P2310 minimum value, P2312 maximum date, P2311 minimum date, P2305 allowed values, P2307 allowed units, P4155 separator, P2304 group by, P2429 expected completeness | The parameter |

A role that a tenant has not bound is a constraint type it cannot check; the checker reports "unrecognised constraint" for it and moves on. **Constraint via SPARQL** (Q21502838) is not supported: nothing on the read path runs SPARQL ([0013](0013-postgres-storage.md) §8), and the type is unsupported in Wikidata's own checker too.

**Constraints on mirrored properties are read from the resolved view**, so a tenant may override or suppress a Wikidata constraint locally ([0002](0002-source-graphs-and-mass-ingest.md) §7) and add its own, and the checker sees the result. The reference to a constraint's source is kept: a violation says whether the constraint came from Wikidata or from here.

### 2. Checking is a projection over the resolved view (extends 0004 §4, §7)

Violations are **derived facts**, not records. A **constraint projection** runs after resolution ([0013](0013-postgres-storage.md) §7, step 4), checks every statement of a changed entity against the constraints on its properties, and writes the results to `view.constraint_violation` (§5). It is rebuildable, like every projection, and it is never in the log, since a violation is a fact about the current data that the data's next change may remove.

**What it checks against.** The resolved view ([0002](0002-source-graphs-and-mass-ingest.md) §3): fused statements, canonical IDs ([0004](0004-identity-clusters-and-equivalence.md) §4), normalized values ([0004](0004-identity-clusters-and-equivalence.md) §7, [0029](0029-resolver-namespaces.md) §1). So a *distinct values* check on DOIs compares casefolded keys, a *value type* check follows a value to its canonical entity, and a *single value* check counts fused statements, not source-graph duplicates. Mirrored data is checked as local data is: a tenant sees Wikidata's own constraint violations on the items it mirrors, which is also what Wikidata's checker shows.

**Constraint types by what they need:**

| Needs | Types | Read from |
|---|---|---|
| The statement alone | format, one of, none of, integer, no bounds, range, allowed units, allowed qualifiers, required qualifier, citation needed, property scope, single value, single best value | The resolved JSON |
| Other statements of the same entity | item requires statement, label in language, lexeme requires language | The resolved JSON |
| The value's entity | value type, value requires statement | One entity read |
| The entity's classes | type (checks `P31`/`P279` chains up to a depth set in `site` configuration) | Entity reads along the chain, cached in L0 ([0014](0014-caches-and-search.md) §2) |
| Every statement with the same value | distinct values | `view.identifier` for identifier-typed properties; `view.statement_assertion` and a value index otherwise |
| The value entity's statements back | inverse, symmetric | `view.entity_ref` and one entity read |

**Incremental and batch.** An entity's own statements are re-checked when the entity changes, synchronously for interactive writes so that the editor sees the marker on reload ([0013](0013-postgres-storage.md) §7). Types that depend on *other* entities are re-checked for them too, through the reverse index, in the same way a cluster change re-resolves referrers ([0004](0004-identity-clusters-and-equivalence.md), Consequences): changing an item's `P31` re-checks *type* constraints on statements that point at it. A change to a constraint itself re-checks every statement using the property, as a batch job with progress ([0010](0010-site-ui.md) §9), since that is a full scan of one property. Bootstrap ([0013](0013-postgres-storage.md) §9) runs the constraint projection last, and a Wikidata-scale instance may run it for a chosen set of properties first.

**Exceptions and status** are applied as Wikidata applies them: an entity listed under `constraint-exception` produces no violation; a `suggestion` constraint produces a violation of severity `suggestion`, a `mandatory` one of severity `mandatory`, and the rest `normal`. **Nothing is refused.** A violation of any severity is a marker and a row, never an error on a write; the gate for writes is [0030](0030-edit-filters.md), which does not read the graph, and the two are kept apart on purpose.

### 3. Where violations appear

- **On the entity page** ([0003](0003-statement-ui.md) §9), a **constraint marker** beside a violating value: an icon by severity, whose popover names the constraint, its source (Wikidata or this instance), its clarification text, and the parameter that failed, with a link to the property's constraint statement. This is what WikibaseQualityConstraints shows, drawn with the components 0003 already has, and hidden when there are no violations ([0003](0003-statement-ui.md) §1, principle 1).
- **On the property page**, a **Constraints** tab listing each constraint with its violation count and a link to the report.
- **`Special:ConstraintReport/{property}`** and **`Special:ConstraintReport/{type}`**: violations by property or by type, paged, filterable by severity and source graph, with the entity, the statement and the failing value on each row. This is where a community works through a class of problems.
- **`Special:ConstraintReport/{entity}`**: everything on one entity, which is also what `wbcheckconstraints` returns.

> **Amended by [0047](0047-special-pages.md) §4.2.** The source-graph filter defaults to the local graph, as every report does. Widening it is live, since `view.constraint_violation` already covers the resolved view.

Violations are **not a feed** ([0020](0020-change-feeds.md)): they are facts about the present, not events, and a reader who wants to know when a violation appeared reads the entity's history. A count of violations by property is published for the dashboard-style view above.

### 4. Tenants (follows 0018 §6)

Constraints and violations follow the overlay rule of [0018](0018-tenants.md) §6: violations on shared rows are computed once for the instance and shown to every tenant that reads the provider; a tenant whose overlay changes an entity, or whose constraints differ from the shared ones because it overrides a mirrored constraint, has its own violation rows for that entity. A tenant that has not bound the constraint roles checks nothing and shows nothing.

### 5. Storage (extends 0013 §5.6 and §7)

```sql
CREATE TABLE view.constraint_violation (
  entity_id text NOT NULL, statement_id text NOT NULL,
  property text NOT NULL, constraint_type text NOT NULL, constraint_statement text NOT NULL,   -- the P2302 statement's GUID
  constraint_source text NOT NULL,             -- the graph the constraint came from
  severity text NOT NULL,                      -- mandatory | normal | suggestion
  snak_role smallint NOT NULL,                 -- 1 main, 2 qualifier, 3 reference
  detail jsonb,                                -- the failing parameter and value, for the popover
  checked_at timestamptz NOT NULL,
  PRIMARY KEY (statement_id, snak_role, constraint_statement)
);
CREATE INDEX cv_entity   ON view.constraint_violation (entity_id);
CREATE INDEX cv_property ON view.constraint_violation (property, constraint_type, severity);
CREATE TABLE view.constraint_count (property text NOT NULL, constraint_type text NOT NULL, severity text NOT NULL, violations bigint NOT NULL, PRIMARY KEY (property, constraint_type, severity));
```

The tenant column rule of [0013](0013-postgres-storage.md) §5 applies. The constraint projection runs in step 4 of 0013 §7, after `entity` and its dependents, and is synchronous for the changed entity's own statements. Parsed constraints per property are cached in L0 and invalidated by the property's `resolved_version` ([0014](0014-caches-and-search.md) §3). *Distinct values* on non-identifier data types needs a value index; `view.identifier` already serves `external-id`, and the projection maintains a small `view.value_key (property, value_key, entity_id, statement_id)` for the other data types a *distinct values* constraint names, populated only for those properties.

### 6. API (extends 0012 §4 and §5)

| Route or module | Behaviour |
|---|---|
| `wbcheckconstraints` | WikibaseQualityConstraints' module, in its shape: `id`, `claimid`, `constraintid`, `status`, returning per-statement results with `status` (`violation`, `warning`, `suggestion`, `compliance`, `exception`, `not-in-scope`, `todo` for an unbound type) and the constraint's clarification, so that the gadgets and tools that read it work |
| `GET /entity/{id}/constraints` | The same as JSON, with severity and source |
| `GET /constraints/report?property=&type=&severity=&source=` | The report of §3, paged with [0012](0012-api-requirements.md) §2.3's continuation |
| `GET /constraints/counts` | `view.constraint_count` |
| `POST /constraints/recheck` | Requests a re-check of a property's statements as a job; needs `ts-runjob` |

The provenance response ([0003](0003-statement-ui.md) §6, [0012](0012-api-requirements.md) §5) gains a `constraints` list per statement, so the statement UI needs one request, not two.

### 7. UI (extends 0010 §2)

The entity page's marker (§3) sits in the value cell of [0003](0003-statement-ui.md) §9 and follows its rule that nothing is drawn where nothing differs: an entity with no violations looks as it did. The property page gains the **Constraints** tab. `Special:ConstraintReport` uses the site frame with the filters of §3. Severity and the constraint's source are the two facts every row leads with, since "Wikidata says this is mandatory" and "we suggested this here" call for different responses.

### 8. Relationship to edit filters (settles 0030's open question)

Filters gate; constraints report. A filter ([0030](0030-edit-filters.md)) sees the change and the actor and refuses or tags a write in microseconds. A constraint sees the data and the graph around it and marks a statement after the fact. A tenant that wants a hard rule writes a filter; a tenant that wants to know where its data disagrees with its own norms writes a constraint. The two never read each other, and [0030](0030-edit-filters.md)'s "bounded lookup in v1" question stays open on its own terms.

### 9. Crates (amends 0005 §2)

| Layer | Crate | Change |
|---|---|---|
| Wikibase | `scatter-wikibase-constraints` *(new)* | Parsing constraints from a property's statements by role; one checker per constraint type over a resolved entity, with a `Lookup` trait for the types that need other entities or indexes; severity, scope and exceptions; the `wbcheckconstraints` result shape. Pure; the lookups are passed in |
| Triplespace | `triplespace-projections` | The constraint projection, `view.constraint_violation`, `view.constraint_count`, `view.value_key`; the property-wide re-check job |
| | `triplespace-api-action`, `triplespace-api-rest` | §6 |

The workspace goes from forty-one crates to forty-two.

## Consequences

- **Wikidata's constraints work on day one** for any tenant that mirrors Wikidata's properties, on local and mirrored statements alike, with no local authoring.
- **Constraints are data and violations are derived**, so nothing about them enters the log, and a rebuild recomputes every violation from the resolved view.
- **Nothing is ever refused by a constraint.** This keeps the write path's cost bounded ([0030](0030-edit-filters.md)) and keeps the community, not the software, in charge of what a violation means.
- **Normalization pays off again**: *distinct values* and *format* checks see the same canonical values that fusion and resolvers see, so case variants of one identifier are one value everywhere.
- **The expensive types are indexed, not scanned**: *distinct values*, *inverse* and *symmetric* read indexes the instance already keeps for other reasons; *type* walks class chains with a depth limit and a cache.
- **A property-wide re-check is a job**, with progress and lag, not a request.
- **One more crate and three tables**, and a marker the statement UI already had room for.

## Open questions

- **Constraint types beyond Wikidata's**, such as a resolver-aware "resolves uniquely" check ([0029](0029-resolver-namespaces.md)) or a Domain-hierarchy check ([0009](0009-keyed-entity-types-and-domain.md)).
- **Checking mirrored entities at Wikidata scale**: whether to check every mirrored statement or only entities a tenant has touched, and how often to re-check as mirrors sync.
- **Depth and cache limits** for the *type* check's class-chain walk.
- **Suggested fixes**: whether a violation should offer a one-click correction where the fix is unambiguous (a *format* violation with a known normalization, say), which would be an edit like any other.
- **Constraint via SPARQL** if a tenant runs a local quad store ([0013](0013-postgres-storage.md) §8).
- ~~**Whether `mandatory` violations should influence patrolling** ([0023](0023-moderation.md) §6), for example by clearing autopatrol on the write that introduced one; that would be the first thing to read the graph on the write path, and is deliberately not done here.~~ *Settled 2026-09-27 with [0030](0030-edit-filters.md): no. Nothing on the write path reads the graph; constraints report after the fact and a reviewer acts on the report.*

## References

- [Help:Property constraints portal](https://www.wikidata.org/wiki/Help:Property_constraints_portal) and [Extension:WikibaseQualityConstraints](https://www.mediawiki.org/wiki/Extension:WikibaseQualityConstraints)
- [API:wbcheckconstraints](https://www.wikidata.org/w/api.php?action=help&modules=wbcheckconstraints)
- [Wikidata: property constraint (P2302)](https://www.wikidata.org/wiki/Property:P2302), [constraint status (P2316)](https://www.wikidata.org/wiki/Property:P2316), [exception to constraint (P2303)](https://www.wikidata.org/wiki/Property:P2303)
- [0003 — Statement UI](0003-statement-ui.md) §7, [0030 — Edit filters](0030-edit-filters.md)
