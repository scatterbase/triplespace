# 0071. Derived statements from mirrored pages

- **Status:** Proposed
- **Date:** 2026-10-07
- **Author:** James Hare / Claude Opus
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0011](0011-logs.md), [0018](0018-tenants.md), [0038](0038-page-metadata-and-categories.md), [0047](0047-special-pages.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0060](0060-scopes.md), [0070](0070-shallow-entity-mirroring.md), [0072](0072-template-mappings.md), [0073](0073-lines-links-and-url-patterns.md), [Record and payload shapes](../api/payloads.md)

## Context

A wiki whose pages describe things in a regular way, with templates and with lists of links, holds structured data that no one has written as data. The FamilySearch Research Wiki is the first case: its guide pages carry a `{{CountrySidebar}}` call naming the country and topic, and list research resources one per bullet line or table row, each line a link to a collection at FamilySearch, Ancestry, Findmypast or elsewhere. Its owners want that structure as data, to answer questions by place, period, record type and provider, for people and for AI agents.

Triplespace can already mirror such a wiki's pages ([0053](0053-mirrored-pages.md) §5) and keep them current (0053 §6). What is missing is a place for statements derived from those pages, and the rules for writing them:

- **Not the `local` graph.** Derived statements are a function of a page revision. When the page changes they must be replaced, and editors' own statements must not be disturbed by that. Mixing the two in `local` would need every re-extraction to find and retract its own earlier statements among people's.
- **Not page statements.** [0038](0038-page-metadata-and-categories.md) §2 gives a page statements about itself, and [0052](0052-page-repositories-and-title-inheritance.md) Q1 keeps local statements off foreign pages. The statements here are about the things a page describes: a microfilm collection, an online index, the guide page as a resource. They belong on items.
- **Not a mirror.** A mirror holds what a provider asserts about its own entities. A guide page asserts nothing in Wikibase form; the instance derives the statements, about local items it creates or finds.

The resources become ordinary items of the tenant, matched by their identifiers, as `create-or-add` already provides ([0002](0002-source-graphs-and-mass-ingest.md) §8.5). A keyed entity type per identifier scheme was considered and rejected by James: a namespace for every scheme does not scale.

### Direction

James's direction, from the design discussion of 2026-10-07:

- **Simplify the scope to parsing external links from bullet lines and mapping template names, parameters and values**, which is more generalizable. Analysis by LLMs is a separate workflow, pending an LLM analysis module.
- **Resources are Librarybase items, not keyed entities.** "I am not sure we can create a keyed namespace every time we want to link to the same resource repeatedly."
- **FamilySearch does not run Triplespace.** The genealogy data is a scope, and the wiki receives data derived from it.

## Decision

### 1. Derived graphs (extends 0002 §2)

**A derived graph holds the statements an extraction source derives from pages.** It is a source graph of the tenant, one per source, named `derived/{source}`:

| Graph | Illustrative IRI | Kind | Scope | Written by | History | Integrity | Export |
|---|---|---|---|---|---|---|---|
| **Derived**, one per extraction source | `{base}/graph/derived/familysearch` | Source | Tenant | Only that source's extraction job | `latest` (a tenant may set `full`) | `logged` | Public |

Its records are `scatter:v0/derivation` (§3). No editor and no other job writes to it; a correction is a local-graph assertion about the derived statement (§8), as for a mirror. Its subjects are local items, mirrored entities, or any entity the tenant can name; its statements resolve with the rest (§7).

### 2. Extraction sources (extends 0015 §3)

**An extraction source is tenant configuration**, a `config` record of kind `extraction` whose code is the source's name:

| Field | Meaning |
|---|---|
| `repo` | The page repository the pages come from ([0052](0052-page-repositories-and-title-inheritance.md) §1), in `mirror` mode with the pages in its `mirror.set` ([0053](0053-mirrored-pages.md) §5); or `local`, for the tenant's own `pages` |
| `namespaces`, `titles` | Which pages: namespace numbers, and optional title prefixes or patterns |
| `extractors` | Which extractors run, by name and version: `templates` ([0072](0072-template-mappings.md)), `lines` ([0073](0073-lines-links-and-url-patterns.md)) |
| `mappings` | The template mappings this source applies (0072 §2) |
| `lines` | The line settings of 0073 §5 |
| `page_subject` | How the item for the page itself is found or made: a match key built from the page (`{ property = "P…", value = "{url}" }`), its label (`{title}`), and constant statements such as `P31 = research guide` |
| `reference` | The properties of the reference added to every derived statement (§6) |
| `language` | The language of labels taken from page text |

Written as TOML for illustration, with illustrative local IDs:

```toml
[extraction.familysearch]
repo        = "familysearch"
namespaces  = [0]
extractors  = ["templates@1", "lines@1"]
mappings    = ["countrysidebar", "recordsearch", "fsc"]
language    = "en"
page_subject = { match = { property = "P9030", value = "{url}" }, label = "{title}", statements = [ { property = "WDP31", value = { entity = "Q900" } } ] }
reference   = { url = "WDP854", retrieved = "WDP813", stated_in = "WDP248" }
```

**A source runs over every page it selects** when it is created or changed, as a job with progress, as a category mapping does ([0038](0038-page-metadata-and-categories.md) §5), and then **follows the repository**: each mirrored-page `put` for a selected page enqueues the page (§9).

### 3. The derivation record (extends 0015 §1)

**One record holds everything one extractor derived from one page revision.** Its payload type is `scatter:v0/derivation`, in `derived/{source}`, keyed by `{page ID}:{extractor}`, where the page ID is the page's provider-ranged ID ([0052](0052-page-repositories-and-title-inheritance.md) §6) or its local page ID. It declares four parts:

| # | Part | Holds |
|---|---|---|
| 0 | Content | The page (repository, upstream page ID, revision ID, title), the extractor's name and version, the source configuration's revision, and the **subjects**: for each, the entity ID and the statements derived about it, in Wikibase JSON |
| 1 | Comment | Null; the job's summary is on its log events |
| 2 | Attestation | The extraction job |
| 3 | Evidence | The **mentions**: for each place in the page a statement came from, its line number, heading path, raw wikitext, plain text, the links and template calls in it, and the subject it produced (§10) |

Evidence is a part of its own so that it can be erased on its own: it copies page text, which a takedown on the source page ([0053](0053-mirrored-pages.md) §9) must be able to reach without losing the statements.

**Operations.** `put` replaces the previous derivation for the key, and is skipped when the page revision, the extractor version and the configuration revision are all unchanged. A page that leaves the selection, or is deleted or tombstoned upstream, gets a `put` with no subjects. There is no `tombstone`: the key is the page, not an entity, and nothing to retain is lost.

**Each extractor writes its own record.** A page has one derivation per extractor that ran on it, so adding an extractor, or re-running one at a new version, never rewrites another's statements.

### 4. Subjects: found by identifier, or created (uses 0002 §8.5)

**A derived subject is found by its identifiers, or created as an ordinary item of the tenant.** The extractors (0072, 0073) say, for each subject, its candidate **match keys** in order (an external identifier such as `P9999 = "1804886"`, or a normalized URL), a label, and whether it may be created. The job resolves each subject before writing the derivation:

1. **Found.** The first match key with a row in `view.match_key` ([0013](0013-postgres-storage.md) §5.2), or in `view.identifier` for a property not designated for `match_key`, names the subject. A key that names two different entities is a conflict: the subject is not resolved, and the job reports it with both IDs for a merge.
2. **Found earlier in the same job.** The job keeps a map from match key to the entity it created, so that two pages in one run that mention one collection do not create it twice before the projection has caught up.
3. **Created.** Otherwise the job appends a `create-or-add` ([0002](0002-source-graphs-and-mass-ingest.md) §8.5) to the tenant's `local` graph, carrying the first match key as `match` and, as its content, the label in the source's `language` and one statement for each match key. Its `match` makes a concurrent creation by another job an `add`, not a duplicate. The created item is a local item like any other, attributed to the job.

The match key's property must be designated for `match_key` for step 3 to be safe; a source whose mappings name an undesignated identifier property is refused at configuration with `ts-extraction-match-key`.

**A subject may be foreign.** A link that resolves to a Wikidata item, through a sitelink or a URL match pattern, names `WDQ…` as the subject. Its statements go on the foreign entity, which shallow mirroring fetches ([0070](0070-shallow-entity-mirroring.md) §2.2).

**The guide page has a subject too**, found or created by `page_subject` (§2). Line subjects can then point at it (0073 §5) and inherit its facets.

### 5. Statement IDs

**A derived statement's ID is a name-based UUID of the source name, the subject, the property, the canonical main value and the hash of its qualifiers.** It is not tied to the page or the line:

- the same fact derived from two pages is one statement, with one reference per page (§6);
- a page edited without changing the fact keeps the statement's ID, so a local correction against it (§8) survives re-extraction;
- [0002](0002-source-graphs-and-mass-ingest.md) §8.4's rule for sources with no IDs of their own is followed.

### 6. References

**Every derived statement carries a reference to the page revision it came from**, built from the source's `reference` properties: the page URL with `oldid` as reference URL, the revision's timestamp as retrieved, and the guide page's subject as stated-in where it has one. A statement derived from two pages has two references, merged by hash as 0002 §8.5 merges references.

### 7. Resolution (amends 0002 §3)

**Derived graphs are sources in the resolved view, between the local graph and the mirrors.**

- Statements, references and aliases: union, as for every source.
- Labels, descriptions and rank: the local graph wins wherever it says anything; then the tenant's derived graphs, in the order the `reconcile` entry lists them ([0004](0004-identity-clusters-and-equivalence.md) §9), by default by source name; then the mirrors in provider order.
- Within one derived graph, the contributions of every derivation that names the subject are unioned; for one language, the derivation of the lowest page ID supplies the label, so the choice is stable.

The entity projection finds a subject's derivations through `view.derivation_subject` (§11). A derivation's `put` re-resolves the subjects it names now and the subjects its previous version named.

### 8. Corrections (uses 0002 §7)

**Corrections to derived statements are local-graph assertions, as for mirrored ones**: a rank override, an added statement, a suppression. `Special:Corrections` ([0047](0047-special-pages.md) §6) lists derived graphs beside mirror graphs, filterable by source. Because statement IDs are stable (§5), a suppression of a wrong derived statement stays in force when the page is re-extracted, and is reported as redundant if the page stops yielding the statement.

### 9. Following pages

- **A page change** (a mirrored-page `put`, or a local page edit for `repo = local`) enqueues `(source, page)` in `ops.extraction`, for every source that selects the page. The extraction job drains the queue, runs the source's extractors on the page's wikitext, resolves subjects (§4) and appends a derivation `put` per extractor.
- **A change to the source's configuration, a mapping it names, or an extractor version** enqueues every page the source selects.
- **Template redirects and renames** in the repository change which mapping a call matches (0072 §1). A `put` for a Template page re-enqueues the pages that call it, through the repository's `templates` metadata (0053 §5).

### 10. Evidence, and analysis later

**The evidence part is the input to later analysis.** It keeps each line's raw wikitext and plain text, its heading path and links, so that an analyzer (an LLM pass that reads years, access terms or populations out of "1850–1885 … index & images ($)") can run over stored evidence without fetching or re-parsing the page. Such an analyzer is **another extractor**: it writes its own derivation per page (§3), its statements resolve beside the others, and its references can name the extractor. No analyzer is specified here.

### 11. Orphans

**An item created by extraction that no derivation names any longer is an orphan.** It is never deleted automatically: someone may have edited it, linked to it or cited it. Orphans are listed per source (`GET /extraction/{source}/orphans`), in the order they were orphaned, for review: delete, merge or keep.

### 12. Storage (extends 0013 §5.6)

- `view.derivation (tenant, source, page_id, extractor, upstream_revid, config_rev, offset, subjects)`, one row per key, the projection of `derived/{source}`.
- `view.derivation_subject (tenant, subject, source, page_id, extractor)`, primary key on all five, indexed by `(tenant, subject)`: the reverse index the entity projection reads (§7) and that `GET /entity/{id}/derivations` serves.
- `ops.extraction (tenant, source, page_id, reason, enqueued, attempts)`, unique on `(tenant, source, page_id)`.

The derivation projection runs in step 2 of [0013](0013-postgres-storage.md) §7, beside `entity_source`; resolution of the subjects it names runs in step 4.

### 13. API (extends 0012 §5)

| Route | Returns |
|---|---|
| `GET /extraction/{source}` | The source's configuration, page count, queue length, last run, conflicts (§4) and orphan count |
| `POST /extraction/{source}/run` | Enqueues every page of the source, or the pages named; the `ts-runjob` right |
| `GET /extraction/{source}/orphans` | §11 |
| `GET /page/{id}/derivations` | The derivations of a page, with subjects and evidence |
| `GET /entity/{id}/derivations` | The derivations that name an entity, with the mentions that produced each statement |

On the entity page, a derived statement's reference links to the page revision; the statement's source chip names the extraction source as a mirror's names its provider.

### 14. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-log` | The `derived/{source}` graph and the four-part `scatter:v0/derivation` payload type in the graph registry |
| `scatter-wikibase-resolve` | Derived contributions in resolution order (§7) |
| `triplespace-extraction` | **New.** The extraction job: the queue, running extractors from `scatter-extract` ([0072](0072-template-mappings.md), [0073](0073-lines-links-and-url-patterns.md)), subject resolution and `create-or-add`, the derivation `put`, orphans and conflicts; the `extraction` config kind |
| `triplespace-projections` | `view.derivation`, `view.derivation_subject`; derived contributions in the entity projection; enqueuing pages from mirrored-page `put`s |
| `triplespace-api-rest` | The routes of §13 |

## Alternatives considered

- **Keyed entity types per identifier scheme** (`fs-collection:1804886`). Deterministic identity with no lookup, but a namespace, registry entry and normalizer for every scheme a page links to. Rejected by direction.
- **Derived statements in the `local` graph**, with a reference naming the page, and re-extraction retracting the previous ones by statement ID. One graph fewer, but machine statements and people's then share a history, a re-extraction rewrites the local log, and a person's edit to a derived statement is overwritten by the next re-extraction.
- **One record per subject**, keyed by entity, holding every page's contribution. Resolution reads one record, but every page edit must read, merge and rewrite the record of every subject it mentions, and a busy subject's record grows with every page that cites it.
- **Page statements on the mirrored page** ([0038](0038-page-metadata-and-categories.md) §1). The statements are about the resources, not the page, and [0052](0052-page-repositories-and-title-inheritance.md) Q1 keeps local statements off foreign pages.

## Consequences

- **A mirrored wiki becomes a source of data** about the things it describes, on items the tenant can query, scope ([0060](0060-scopes.md)), correct and export.
- **Re-extraction is safe.** A page edit replaces only that page's derivations; people's statements and corrections are untouched.
- **Item creation is idempotent per identifier,** but two identifiers for one thing make two items until someone merges them. Merging needs `wbmergeitems`, which the server does not implement yet.
- **`match_key` designation moves onto the critical path.** Every identifier property an extraction source keys on must be designated.
- **The evidence part copies page text into the tenant's log,** under the repository's licence ([0053](0053-mirrored-pages.md) §9) and erasable on its own.
- **Test plan.** A page with two lines naming one collection makes one item with two references; a second page in the same job naming it adds a third without creating a duplicate; a page edit that drops a line removes that line's statements and nothing else; a local suppression of a derived statement survives re-extraction; a page deletion empties its derivations and leaves an orphan listed; a rebuild from the log reproduces every derived statement and created item.

## Open questions

- **Q1. Merging items.** Two items created for one resource under different identifiers need `wbmergeitems` and a redirect; whether extraction should then learn the merge, so that the other identifier matches the survivor.
- **Q2. Statements about the source wiki's own pages**, such as which guides link to which, beyond `page_subject`: whether a `links` extractor belongs here.
- **Q3. A source over several repositories**, so that two wikis' guides contribute to one graph.
- **Q4. Confidence.** Whether derived statements carry an extractor confidence (a qualifier, or a field in the derivation), which an LLM analyzer would want and the deterministic extractors would not.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §2 | §1 | extends | 0002 A22 |
| [0002](0002-source-graphs-and-mass-ingest.md) §3 | §7 | amends | 0002 A22 |
| [0005](0005-crate-organization.md) §2 | §14 | amends | 0005 A74 |
| [0012](0012-api-requirements.md) §5 | §13 | extends | 0012 A51 |
| [0013](0013-postgres-storage.md) §5.6 | §12 | extends | 0013 A37 |
| [0015](0015-record-format-and-partition-registry.md) §1, §3, §5 | §1, §2, §3 | extends | 0015 A31 |

## References

- `claude/familysearch-semantic-layer-plan.md` (Triplespace project notes, 2026-10-07): the first use, and the decisions behind this ADR
- FamilySearch Research Wiki pages "Mexico Colonial Records" and "Ireland Land and Property" (wikitext supplied by James, 2026-10-07): the two layouts, table rows and bullet lines, that the extractors are designed against
