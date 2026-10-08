# 0070. Shallow entity mirroring

- **Status:** Proposed
- **Date:** 2026-10-07
- **Author:** James Hare / Claude Opus
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0011](0011-logs.md), [0014](0014-caches-and-search.md), [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0022](0022-federation.md), [0040](0040-instance-prerogatives.md), [0053](0053-mirrored-pages.md), [0065](0065-mediainfo-captions-and-commons.md), [0067](0067-proposals.md), [0071](0071-derived-statements-from-mirrored-pages.md)

## Context

A tenant can only use a Wikidata item as a value, or correct one, once that item is in the Wikidata mirror graph. Today the only way in is a dump sync ([0002](0002-source-graphs-and-mass-ingest.md) §8.4). A full Wikidata sync is not yet practical: the measured path is per-entity, about 117 million items at 10–20 ms each is weeks, and a full mirror forces the open question of whether every mirrored term goes into `view.term` (`claude/adoption-results-and-wikidata-projection.md` §2).

Most tenants need only the entities they use. The first customer that needs this is a hosted semantic layer for the FamilySearch Research Wiki: its resources are Librarybase items whose places, record types, religions, languages and providers are Wikidata items, a few thousand of them, plus the administrative hierarchy above each place.

0002 §8.4 already lists "the closure of everything the local graph references" as a subset import, and [0065](0065-mediainfo-captions-and-commons.md) §2 already mirrors Commons' MediaInfo `on-demand`. [0053](0053-mirrored-pages.md) §5 has the same three-way choice for pages: `on-demand`, `linked`, `all`. This ADR generalizes those to every entity type of a Wikibase provider.

### Direction

James's direction, from the design discussion of 2026-10-07:

- **"Pending full Wikidata sync support, we might want to have a concept of shallow reification where you load enough of the Wikidata entity to see the information you need but not so much you load an entire graph subset."**
- **It should reuse the machinery of Wikipedia article mirroring where it can**, and make it possible to use Wikidata items as values on Librarybase items and to attach Librarybase statements to Wikidata items.

## Decision

### 1. Shallow means depth, not a slice (extends 0002 §8.4)

**A shallow mirror holds the whole current state of each entity it holds, and does not follow that entity's links.** "Enough of the entity" is read as *enough entities*, not *part of an entity*:

- Each mirrored entity is written as an ordinary mirror `put` ([0002](0002-source-graphs-and-mass-ingest.md) §8.2) of its full upstream JSON, with `lastrevid` as its version cursor, in the provider's mirror partition. Nothing distinguishes it from an entity a dump sync wrote.
- A partial entity is not written. A `put` of part of an entity would break two rules already in place: the `source` resolution kind needs a whole-state record ([0013](0013-postgres-storage.md) A28), and a later dump sync would read every statement the slice left out as removed upstream.
- The entities an entity refers to are **not** fetched because it refers to them, except along closure properties (§2.3). Their labels are needed to display it; those are read through the label cache of §4, which is not the mirror.

0002 §8.4's "closure of everything the local graph references" becomes one of the sets below, `linked`, with a depth of zero and the closure properties of §2.3.

### 2. Which entities are mirrored

#### 2.1 The setting

**`entities.mirror`** is a `site` setting, per provider and entity type, with four values ([0015](0015-record-format-and-partition-registry.md) §3):

| Value | Mirrors |
|---|---|
| `off` | Nothing; foreign IDs are shown from the label cache only |
| `on-demand` | An entity the first time it is read (an entity page, `wbgetentities`, a table cell) on any tenant that lists the provider ([0018](0018-tenants.md) §5) |
| `linked` *(default)* | Everything `on-demand` mirrors, and every entity used as a statement value, qualifier value, reference value or property in a graph of a listing tenant: `local`, `pages` and any derived graph ([0071](0071-derived-statements-from-mirrored-pages.md) §1) |
| `all` | The provider's whole type, by dump sync; what 0002 §8.4 already does |

The setting is written as `entities.mirror = { WD = { item = "linked", property = "linked", lexeme = "on-demand" } }`. Properties are always at least `on-demand`, since a statement cannot be validated or rendered without its property's data type. 0065 §2's `mediainfo.mirror` is this setting for Wikidata's type `M`, under its own name for compatibility; it keeps its default of `on-demand`.

#### 2.2 When `linked` fetches

**A write that mentions a foreign ID not yet mirrored enqueues it; it does not wait for it.** The `entity_ref` projection ([0013](0013-postgres-storage.md) §7, step 4) adds a row to `ops.entity_fetch` for each foreign entity ID it writes as a target that has no `view.entity_source` row for its provider. Validation at save time checks only the ID's grammar and its provider ([0017](0017-entity-id-grammar.md) §2), as it does today for an ID whose mirror has not caught up. The entity page shows the value from the label cache until the fetch lands, and the fetch re-renders the pages that use it through `view.entity_ref`.

A bulk job that will reference many foreign entities may prefetch them: the job's `start` names `prefetch = true`, and the ingester enqueues every foreign ID it validates before writing, so that the fetch runs alongside the job.

#### 2.3 Closure properties

**`entities.closure`** (site setting, per provider) lists properties whose values are fetched transitively from every mirrored entity, up to `entities.closure_depth` (default **6**). The default for Wikidata is:

| Property | Why |
|---|---|
| `P31` instance of | The type of a value, for display and for constraints |
| `P279` subclass of | Class hierarchies, so a query for a class finds its subclasses |
| `P131` located in the administrative territorial entity | Place hierarchies, so a query for a state finds its counties |
| `P17` country | The top of a place hierarchy |

The depth cap bounds the walk up `P279`, which is deep. A tenant may add properties (`P361` part of, `P1365`/`P1366` replaces/replaced by for historical jurisdictions) or remove them. Closure stops at an entity already mirrored, so the walk is incremental.

#### 2.4 Leaving the set

**A shallowly mirrored entity is never tombstoned for falling out of use.** A tombstone means "deleted upstream" and applies the entity's retention policy ([0002](0002-source-graphs-and-mass-ingest.md) §5), which under `cascade` retracts local assertions, so it cannot be reused for eviction. The set grows with use and is bounded by it. Eviction is Q1.

### 3. Fetching and records

**A fetch is an instance job per provider**, run by the instance ([0040](0040-instance-prerogatives.md) §6), like a page repository's sync job ([0053](0053-mirrored-pages.md) §5):

- It drains `ops.entity_fetch` in batches through `wbgetentities` (50 IDs per request, the API's limit for clients without `apihighlimits`) or `Special:EntityData/{id}.json` for a single entity, through the upstream client and its `upstream` rate class ([0012](0012-api-requirements.md) §6).
- Each entity becomes a `put` with the same fields a dump sync writes. An upstream redirect becomes a `redirect`, and a missing entity a `tombstone`, as 0002 §8.4 says.
- The job's records are one long-running job per provider, with a `job/start` when the instance starts it and periodic checkpoints, so `Special:Jobs` and `Special:Providers` show the set's size, the queue and the lag.

### 4. Labels of entities not mirrored

**Labels for display come from a cache, not the mirror.** Rendering a mirrored entity needs the labels of the entities its statements point at. The label cache is an L1/L2 entry per entity and language ([0014](0014-caches-and-search.md) §2), filled by `wbgetentities&props=labels|descriptions` in batches, with a lifetime of `entities.label_ttl` (default 7 days). A label from the cache is never a term in `view.term`, never in search, and never in RDF. An entity that is mirrored reads its terms from `view.term` instead.

### 5. Keeping mirrored entities current (uses 0053 §6)

**The set is followed by the provider's event stream, and checked by a sweep.**

- **Events.** For Wikidata, the instance follows `mediawiki.recentchange` filtered to `wikidata.org`, as [0053](0053-mirrored-pages.md) §6 follows `mediawiki.page-change.v1` for a page repository, with the same cursor table (`ops.repo_cursor`, keyed by provider), debounce and gap-closing by `list=recentchanges`. An event for an entity in the set (a `view.entity_source` row for the provider) enqueues a fetch. Events for entities outside the set are ignored.
- **Sweep.** Daily, the job asks `wbgetentities&props=info` for every entity in the set in batches of 50 and enqueues those whose `lastrevid` is newer than the cursor. The sweep catches anything the stream missed.

### 6. Terms of mirrored entities (extends 0013 §5.6)

**A mirrored entity's terms populate `view.term` in `entities.term_languages` only** (site setting; default: the tenant's content languages, `mul` and `en`). Its other languages stay in the record and are served by `wbgetentities`, which reads the record, but are not rows. A shallow mirror holds thousands or millions of entities, not 117 million, so the rows are affordable; the full-mirror question of what `all` writes to `view.term` stays open (Q2).

### 7. Becoming a full mirror

**Moving a type from `linked` to `all` starts a dump sync, which continues from the shallow records.** The version cursor already holds each shallow entity's `lastrevid`; the sync skips every entity whose cursor is current and writes a `put` for the rest. Nothing is replaced and nothing is deleted. Moving back from `all` to `linked` keeps every entity already mirrored (§2.4).

### 8. Local statements on mirrored entities

**Nothing new is needed.** A tenant's `add`, rank override, suppression and `retain` on a mirrored entity are local-graph assertions about a foreign subject ([0002](0002-source-graphs-and-mass-ingest.md) §7), resolved against the mirror by [0013](0013-postgres-storage.md) A28's resolution, as the internetdomains tests already do for Domains. Proposals ([0067](0067-proposals.md)) can offer them upstream. A local statement on a foreign entity that is not yet mirrored enqueues it (§2.2), and until it lands the resolved view is the local contribution alone.

### 9. API and operations (extends 0012 §5)

- `GET /provider/{code}/mirror` reports, per type, the setting, the set size, the queue length and the stream lag. `POST /provider/{code}/mirror/fetch` with a list of IDs enqueues them (the `ts-runjob` right).
- `meta=siteinfo&siprop=providers` gains `mirror` per type.
- `triplespace-cli mirror status`, `mirror fetch WDQ42 WDP31`, and `mirror sweep` run the same operations.

### 10. Storage (extends 0013 §5.6)

- `ops.entity_fetch (provider, entity_id, reason, enqueued, attempts)` with reasons `read`, `linked`, `closure`, `event`, `sweep`, `manual`; a unique key on `(provider, entity_id)` so a busy entity is fetched once.
- `ops.repo_cursor` gains rows keyed by provider for the entity stream.
- The set itself is `view.entity_source` rows for the provider (`tenant = ''`); no new table.

### 11. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-adapter-wikidata` | Fetching entities by ID through `wbgetentities` and `Special:EntityData`; the `recentchange` filter for entity pages; the closure walk |
| `scatter-ingest` | The shallow mirror job: drains `ops.entity_fetch`, writes `put`, `redirect` and `tombstone`; `prefetch` on job start |
| `triplespace-projections` | Enqueuing unmirrored foreign IDs from `entity_ref`; `entities.term_languages` in the term projection |
| `triplespace-repos` | Event following and cursors shared between page repositories and entity providers |
| `triplespace-cache` | The label cache for entities not mirrored |
| `triplespace-cli` | `mirror status`, `mirror fetch`, `mirror sweep` |

## Alternatives considered

- **Partial entities**: only labels, `P31` and a chosen property list. Smaller, but it breaks whole-state resolution and makes the upgrade to a full sync a diff instead of a no-op (§1).
- **A separate "reference" graph** for entities known only by label. The label cache does that job without a graph, and nothing about a label-only entity needs to be in the log.
- **Fetching neighbours to depth 1.** Every Wikidata item points at dozens of others; depth 1 multiplies the set by that factor for display alone. Closure properties take only the edges queries need.

## Consequences

- **Wikidata items work as values and subjects on a tenant before a full Wikidata sync exists.** Corrections and local statements on them work as they will after it.
- **The full sync becomes an upgrade, not a migration.** Shallow records are what a dump sync would have written.
- **The instance makes upstream requests in proportion to use.** The `upstream` rate class and the event stream keep that bounded; a tenant importing a large dataset should prefetch.
- **Display may lag a write** by the fetch queue's latency, showing a cached label until the entity lands.
- **Test plan.** A local statement with value `WDQ42` enqueues `Q42`, whose `put` lands with its `lastrevid`; `P131` closure from a county reaches its country and stops at the depth cap; an upstream edit arrives by the stream and by the sweep; a dump sync after a shallow mirror writes no `put` for an unchanged entity; an entity in the set is never tombstoned for disuse.

## Open questions

- **Q1. Eviction.** An entity mirrored `on-demand` and no longer read or referenced stays forever. Whether to add a compaction-only eviction that is not a tombstone, and how it interacts with retention.
- **Q2. Terms under `all`.** Whether a full mirror writes `view.term` rows for every language, a configured subset, or none, with search delegated (the adoption-results note, §2).
- **Q3. Other Wikibase providers.** The fetch path is Wikidata's API; whether a Wikibase Cloud provider or another Triplespace instance ([0022](0022-federation.md) §2) uses the same job with its own endpoint, and how a provider without an event stream is kept current beyond the sweep.
- **Q4. Lexemes and forms under `linked`.** A form or sense ID as a value fetches its lexeme; whether that should be the default for `lexeme`.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §8.4 | §1 | extends | 0002 A21 |
| [0005](0005-crate-organization.md) §2 | §11 | amends | 0005 A73 |
| [0012](0012-api-requirements.md) §5 | §9 | extends | 0012 A50 |
| [0013](0013-postgres-storage.md) §5.6 | §6, §10 | extends | 0013 A36 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §2.1 | extends | 0015 A30 |

## References

- [Wikibase API: `wbgetentities`](https://www.wikidata.org/w/api.php?action=help&modules=wbgetentities): batches of 50 IDs, `props=info` for `lastrevid`
- [Wikimedia EventStreams](https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams): `mediawiki.recentchange`
- `claude/adoption-results-and-wikidata-projection.md` (Triplespace project notes, 2026-10-06): why a full Wikidata sync is not yet practical, and the term-table question
- `claude/familysearch-semantic-layer-plan.md` (Triplespace project notes, 2026-10-07): the first use
