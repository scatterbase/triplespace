# 0031. Property constraints

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A8)
- **Author:** James Hare / Claude Fable
- **Changes:** [0003](0003-statement-ui.md), [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0030](0030-edit-filters.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0018](0018-tenants.md), [0020](0020-change-feeds.md), [0029](0029-resolver-namespaces.md), [Wikibase contract](../api/wikibase-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [06](../architecture/06-statements-and-properties.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

Wikidata's data quality rests on **property constraints**: statements on a property, using the property `P2302` ("property constraint"), that say what values, qualifiers, references and neighbours a statement using that property ought to have. A constraint is a claim about the data, not a rule the software enforces: the WikibaseQualityConstraints extension checks statements against them and shows a marker beside a violating value, and editors decide what to do. Roughly thirty constraint types exist, from *format* (a regular expression the value must match) and *single value* to *inverse*, *item requires statement* and *distinct values*; each may carry exceptions, a status (`mandatory` or `suggestion`), and a scope.

The earlier ADRs touch this three times without designing it. [0003](0003-statement-ui.md) §7 uses constraints "as hints" to seed a table's columns. [0030](0030-edit-filters.md) §2 keeps data-dependent rules out of edit filters and names property constraints as the place for them: "the value of P31 must be an instance of Q5" is not a gate on a write, it is a report on the data. And James's direction is that constraints stay what they are on Wikidata: **soft recommendations**, never a refusal.

The instance has two advantages over Wikidata here. It **mirrors** Wikidata's properties, so the constraints Wikidata's community has written arrive in the mirror graph as ordinary statements on `WDP31`, `WDP356` and the rest, and apply to any statement using those properties on any entity, local or mirrored, the moment the mirror lands. And it has a **resolved view** ([0002](0002-source-graphs-and-mass-ingest.md) §3) with indexes over identifiers and references ([0013](0013-postgres-storage.md) §5.2), which is what the expensive constraint types need.

## Decision

### 1. A constraint is a statement on a property, read by role (extends 0003 §7)

*Changed by A4, A6, A8.*

*Current text: [06](../architecture/06-statements-and-properties.md) §2.1.*

### 2. Checking is a projection over the resolved view (extends 0004 §4, §7)

*Changed by A7.*

*Current text: [06](../architecture/06-statements-and-properties.md) §2.2, §2.5.*

### 3. Where violations appear

*Changed by A2.*

*Current text: [06](../architecture/06-statements-and-properties.md) §2.3.*

### 4. Tenants (uses 0018 §6)

*Changed by A6.*

*Current text: [06](../architecture/06-statements-and-properties.md) §2.4.*

### 5. Storage (extends 0013 §5.6 and §7)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.4, §5, §6.1, §12.2.*

### 6. API (extends 0012 §4 and §5)

*Current text: [18](../architecture/18-api.md) §2.3, §3.1, §3.2.*

### 7. UI (extends 0010 §2)

*Current text: [19](../architecture/19-site-ui.md) §4.8, §6.4.*

### 8. Relationship to edit filters (settles 0030 Q1)

*Current text: [06](../architecture/06-statements-and-properties.md) §2.5.*

### 9. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- ~~**Wikidata's constraints work on day one** for any tenant that mirrors Wikidata's properties, on local and mirrored statements alike, with no local authoring.~~ *On local statements and the tenant's overlay by default; mirrored statements are checked only under `constraints.scope = all`, as a batch job (A7).*
- **Constraints are data and violations are derived**, so nothing about them enters the log, and a rebuild recomputes every violation from the resolved view.
- **Nothing is ever refused by a constraint.** This keeps the write path's cost bounded ([0030](0030-edit-filters.md)) and keeps the community, not the software, in charge of what a violation means.
- **Normalization pays off again**: *distinct values* and *format* checks see the same canonical values that fusion and resolvers see, so case variants of one identifier are one value everywhere.
- **The expensive types are indexed, not scanned**: *distinct values*, *inverse* and *symmetric* read indexes the instance already keeps for other reasons; *type* walks class chains with a depth limit and a cache.
- **A property-wide re-check is a job**, with progress and lag, not a request.
- **One more crate and three tables**, and a marker the statement UI already had room for.

## Open questions

- **Q1. Constraint types beyond Wikidata's**, such as a resolver-aware "resolves uniquely" check ([0029](0029-resolver-namespaces.md)) or a Domain-hierarchy check ([0009](0009-keyed-entity-types-and-domain.md)).
- **Q2.** ~~**Checking mirrored entities at Wikidata scale**: whether to check every mirrored statement or only entities a tenant has touched, and how often to re-check as mirrors sync.~~ *Settled by [0083](0083-write-path-in-three-tiers.md) §3: `constraints.scope` is `local` by default (local-graph statements and the tenant's overlay), `overlay`, or `all`, and `all` is a batch job with progress, never a step the consumer takes per event; an entity is re-checked when it is composed, and a constraint change on a mirrored property enqueues a batch.*
- **Q3. Depth and cache limits** for the *type* check's class-chain walk.
- **Q4. Suggested fixes**: whether a violation should offer a one-click correction where the fix is unambiguous (a *format* violation with a known normalization, say), which would be an edit like any other.
- **Q5. Constraint via SPARQL** if a tenant runs a local quad store ([0013](0013-postgres-storage.md) §8).
- **Q6.** ~~**Whether `mandatory` violations should influence patrolling** ([0023](0023-moderation.md) §6), for example by clearing autopatrol on the write that introduced one; that would be the first thing to read the graph on the write path, and is deliberately not done here.~~ *Settled by [0030](0030-edit-filters.md) A3: no. Nothing on the write path reads the graph; constraints report after the fact and a reviewer acts on the report.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0003](0003-statement-ui.md) §6, §7 | §1, §3, §6 | extends | 0003 A4 |
| [0003](0003-statement-ui.md) §9 | §1, §3, §6 | amends | 0003 A4 |
| [0005](0005-crate-organization.md) §2 | §9 | extends | 0005 A27 |
| [0010](0010-site-ui.md) §2, §12 | §7 | extends | 0010 A20 |
| [0012](0012-api-requirements.md) §4, §5 | §6 | extends | 0012 A19 |
| [0013](0013-postgres-storage.md) §5, §5.2, §5.6 | §5 | amends | 0013 A9 |
| [0013](0013-postgres-storage.md) §4, §5.4, §5.5, §7, §8 | §5 | extends | 0013 A9 |
| [0014](0014-caches-and-search.md) §1, §5 | §5 | amends | 0014 A3 |
| [0014](0014-caches-and-search.md) §10 | §5 | extends | 0014 A3 |
| [0030](0030-edit-filters.md) §2 | §1, §8 | extends | 0030 A2 |
| [0030](0030-edit-filters.md) Q1 | — | settles | 0030 Q1 |

## References

- [Help:Property constraints portal](https://www.wikidata.org/wiki/Help:Property_constraints_portal) and [Extension:WikibaseQualityConstraints](https://www.mediawiki.org/wiki/Extension:WikibaseQualityConstraints)
- [API:wbcheckconstraints](https://www.wikidata.org/w/api.php?action=help&modules=wbcheckconstraints)
- [Wikidata: property constraint (P2302)](https://www.wikidata.org/wiki/Property:P2302), [constraint status (P2316)](https://www.wikidata.org/wiki/Property:P2316), [exception to constraint (P2303)](https://www.wikidata.org/wiki/Property:P2303)
- [0003 — Statement UI](0003-statement-ui.md) §7, [0030 — Edit filters](0030-edit-filters.md)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §9
- **Summary:** 0005 §2 is the one crate table CI checks, and carries `scatter-wikibase-constraints` and every change this section listed (0005 A27).

Replaced text (§9):

> | Layer | Crate | Change |
> |---|---|---|
> | Wikibase | `scatter-wikibase-constraints` *(new)* | Parsing constraints from a property's statements by role; one checker per constraint type over a resolved entity, with a `Lookup` trait for the types that need other entities or indexes; severity, scope and exceptions; the `wbcheckconstraints` result shape. Pure; the lookups are passed in |
> | Triplespace | `triplespace-projections` | The constraint projection, `view.constraint_violation`, `view.constraint_count`, `view.value_key`; the property-wide re-check job |
> | | `triplespace-api-action`, `triplespace-api-rest` | §6 |
>
> The workspace goes from forty-one crates to forty-two.

### A2. Reports default to the local graph

- **Date:** 2026-10-01
- **Source:** [0047](0047-special-pages.md) §4.2
- **Change:** amends §3
- **Summary:** The source-graph filter defaults to the local graph, as every report does. Widening it is live, since `view.constraint_violation` already covers the resolved view.

Replaced text (§3):

> - **`Special:ConstraintReport/{property}`** and **`Special:ConstraintReport/{type}`**: violations by property or by type, paged, filterable by severity and source graph, with the entity, the statement and the failing value on each row. This is where a community works through a class of problems.

### A3. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §3, §9
- **Summary:** A1–A2 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A2 was a blockquote. The file before conversion is commit `0b26a3a`.

### A4. Role table corrections

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §1
- **Summary:** In the role table, *item requires statement* is Q21503247, not Q21510856 (which is *required qualifier*, listed again later in the same row); and P2305 is the parameter of both *item of property constraint* and *allowed values*, so the parameter row lists it once with its two uses rather than twice. The decision does not change; the IDs were wrong. (PENDING E10)

Replaced text (§1):

> | one role per constraint type | Q21502404 format, Q19474404 single value, Q21502410 distinct values, Q21510865 value type, Q21503250 type, Q21510855 inverse, Q21510862 symmetric, Q21510856 item requires statement, Q21510864 value requires statement, Q21510851 allowed qualifiers, Q21510856 required qualifier, Q21514353 allowed units, Q21510860 range, Q21510859 one of, Q52558054 none of, Q21510854 citation needed, Q52848401 integer, Q51723761 no bounds, Q53869507 property scope, Q64006792 label in language, Q54554025 Commons link, Q55819106 lexeme requires language, and the rest | The constraint type |
> | one role per parameter | P2306 property, P2305 item of property constraint, P2308 class, P2309 relation, P1793 format as a regular expression, P2313 maximum value, P2310 minimum value, P2312 maximum date, P2311 minimum date, P2305 allowed values, P2307 allowed units, P4155 separator, P2304 group by, P2429 expected completeness | The parameter |

### A5. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [06](../architecture/06-statements-and-properties.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A6. Shared-row violations under the instance role bindings

- **Date:** 2026-10-09
- **Source:** [0082](0082-source-form-and-the-shared-view.md) §5, §5.2
- **Change:** amends §4; extends §1
- **Summary:** The `role` config kind has an instance-scope record beside the tenant records, bound in the instance's policy record (`reconcile:default`). Violations on shared rows are computed once for the instance under those bindings, which gives §4's "once for the instance" a vocabulary to compute with; adapters read the instance bindings and never a tenant's. A tenant does not rebind a role the instance record binds: a tenant-scope write that tries is refused with `ts-instance-policy`, and a tenant whose bindings differ gets overlay rows for what the difference touches, which on a Wikidata mirror is everything, so in practice a tenant on a farm does not rebind the constraint roles. On an instance whose record leaves the roles unbound, nothing is checked on shared rows. §1's chapter text ([06](../architecture/06-statements-and-properties.md) §2.1) is unchanged, so for §1 this is an extension; 0082's table gives the §1 row as `amends`. (REVIEW G1)

Replaced text ([06](../architecture/06-statements-and-properties.md) §2.4, as it stood):

> Constraints and violations follow the overlay rule of [08](../architecture/08-tenants-and-instances.md) §4.1 and [03](../architecture/03-storage-caches-and-search.md) §7: violations on shared rows are computed once for the instance and shown to every tenant that reads the provider; a tenant whose overlay changes an entity, or whose constraints differ from the shared ones because it overrides a mirrored constraint, has its own violation rows for that entity. A tenant that has not bound the constraint roles checks nothing and shows nothing.

### A7. The constraint projection is a consumer of composition, scoped

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §3
- **Change:** amends §2
- **Summary:** The constraint projection is a tier-3 consumer of "entity X, tenant T, composed to version N" events, with its own queue, rate and lag, in nobody's transaction, off during bootstrap and bulk modes until the operator turns it on; it is view-derived, rebuilt by a scan of the composed rows and never by a log replay. An entity's statements are re-checked when it is composed, so a marker follows an interactive edit by the consumer's lag, not synchronously; types that depend on other entities are re-checked for the referring entity when it is next composed, never fanned out to referrers. `constraints.scope` is a `site` setting: `local` (the default: local-graph statements and entities in the tenant's overlay), `overlay`, or `all`, and `all` is a batch job with progress, never a per-event step. A constraint-statement change on a mirrored property enqueues a batch rather than a scan. `type` and `value type` are checked with the L0 class cache, and a `P279` change does not invalidate stored results until the next composition of each subject. Bootstrap leaves the projection off. This settles Q2. (REVIEW G10)

Replaced text ([06](../architecture/06-statements-and-properties.md) §2, as it stood):

> Constraints are statements on a property entity, read by role; violations are derived facts that a projection computes and that are never refused at write.

Replaced text ([06](../architecture/06-statements-and-properties.md) §2.2, as it stood):

> Violations are **derived facts**, not records. A **constraint projection** runs after resolution ([03](../architecture/03-storage-caches-and-search.md) §6.1, step 4), checks every statement of a changed entity against the constraints on its properties, and writes the results to `view.constraint_violation` ([03](../architecture/03-storage-caches-and-search.md) §4.4). It is rebuildable, like every projection, and it is never in the log, since a violation is a fact about the current data that the data's next change may remove.

> **What it checks against.** The resolved view ([02](../architecture/02-graphs-rdf-and-query.md) §1.5): fused statements, canonical IDs ([04](../architecture/04-entities-and-identifiers.md) §4.4), normalized values ([04](../architecture/04-entities-and-identifiers.md) §4.7, §3.1). So a *distinct values* check on DOIs compares casefolded keys, a *value type* check follows a value to its canonical entity, and a *single value* check counts fused statements, not source-graph duplicates. Mirrored data is checked as local data is: a tenant sees Wikidata's own constraint violations on the items it mirrors, which is also what Wikidata's checker shows.

> | The entity's classes | type (checks `P31`/`P279` chains up to a depth set in `site` configuration) | Entity reads along the chain, cached in L0 ([03](../architecture/03-storage-caches-and-search.md) §9.2) |

> **Incremental and batch.** An entity's own statements are re-checked when the entity changes, synchronously for interactive writes so that the editor sees the marker on reload ([03](../architecture/03-storage-caches-and-search.md) §6.2). Types that depend on *other* entities are re-checked for them too, through the reverse index, in the same way a cluster change re-resolves referrers ([04](../architecture/04-entities-and-identifiers.md) §4): changing an item's `P31` re-checks *type* constraints on statements that point at it. A change to a constraint itself re-checks every statement using the property, as a batch job with progress ([19](../architecture/19-site-ui.md)), since that is a full scan of one property. Bootstrap ([05](../architecture/05-providers-and-ingest.md)) runs the constraint projection last, and a Wikidata-scale instance may run it for a chosen set of properties first.

### A8. Adapters read the instance role record

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §1
- **Summary:** Adapters never map onto a tenant's properties. A provider that needs vocabulary of its own has a provider property type in the registry (a `[[provider.type]]` row with `entity_type = "property"`, its letter allocated when the adapter is built), minted by the adapter under deterministic IDs with the mapping versioned with the adapter; the `role` config kind's instance-scope record (A6) is what adapters and shared-row consumers read, so a role bound to a mirrored property means one property on every tenant and the constraint checker and the adapters agree on it. The sentence "otherwise onto a local property … the adapter can create them on first import" is struck from [0004](0004-identity-clusters-and-equivalence.md) §6. §1's chapter text is unchanged; the fold is in [06](../architecture/06-statements-and-properties.md) §1 and [04](../architecture/04-entities-and-identifiers.md) §4.6. (REVIEW G26)
