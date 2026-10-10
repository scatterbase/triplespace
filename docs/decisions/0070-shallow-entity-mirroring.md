# 0070. Shallow entity mirroring

- **Status:** Proposed
- **Date:** 2026-10-07
- **Updated:** 2026-10-09 (A7)
- **Author:** James Hare / Claude Opus
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0011](0011-logs.md), [0014](0014-caches-and-search.md), [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0022](0022-federation.md), [0040](0040-instance-prerogatives.md), [0053](0053-mirrored-pages.md), [0065](0065-mediainfo-captions-and-commons.md), [0067](0067-proposals.md), [0071](0071-derived-statements-from-mirrored-pages.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [05](../architecture/05-providers-and-ingest.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

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

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.1.*

### 2. Which entities are mirrored

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.2.*

#### 2.1 The setting

*Changed by A1.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.2, §8.2.*

#### 2.2 When `linked` fetches

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.3.*

#### 2.3 Closure properties

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.3, §8.2.*

#### 2.4 Leaving the set

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.4.*

### 3. Fetching and records

*Changed by A1, A2, A5.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.5.*

### 4. Labels of entities not mirrored

*Changed by A4.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.6.*

### 5. Keeping mirrored entities current (uses 0053 §6)

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.7, §8.2.*

### 6. Terms of mirrored entities (extends 0013 §5.6)

*Changed by A4.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.8.*

### 7. Becoming a full mirror

*Changed by A6, A7.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.9.*

### 8. Local statements on mirrored entities

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.10.*

### 9. API and operations (extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §2.2, §3.2.*

### 10. Storage (extends 0013 §5.6)

*Changed by A1.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.12, §5.*

### 11. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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
- **Q2.** ~~**Terms under `all`.** Whether a full mirror writes `view.term` rows for every language, a configured subset, or none, with search delegated (the adoption-results note, §2).~~ *Settled by [0082](0082-source-form-and-the-shared-view.md) §6: the instance's term languages only, with the other languages read from the record through the label cache, search delegated to the index and no `term_prefix` on a full mirror.*
- **Q3.** ~~**Other Wikibase providers.** The fetch path is Wikidata's API; whether a Wikibase Cloud provider or another Triplespace instance ([0022](0022-federation.md) §2) uses the same job with its own endpoint, and how a provider without an event stream is kept current beyond the sweep.~~ *Settled by [0078](0078-entity-sources.md) §4, in part: a Wikibase declared as a tenant's entity source uses the same job with its own endpoint, and without an event stream is kept current by the sweep alone.*
- **Q4. Lexemes and forms under `linked`.** A form or sense ID as a value fetches its lexeme; whether that should be the default for `lexeme`.
- **Q5.** (Rest of Q3.) Whether a registry Wikibase provider other than Wikidata, or another Triplespace instance ([0022](0022-federation.md) §2), uses the same job with its own endpoint.

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

## Amendment log

### A1. Entity sources

- **Date:** 2026-10-08
- **Source:** [0078](0078-entity-sources.md) §4
- **Change:** extends §2.1, §3, §10
- **Summary:** A tenant's entity source takes `entities.mirror` and `entities.closure` under its name, with `linked` as the default where it has an `api` and `off` where it does not, and an empty closure list. Its fetch is a tenant job per source, writing to `source/{name}`, following the source's events if any and otherwise the sweep. `ops.entity_fetch` gains a `tenant` column.

Replaced text (§10):

> - `ops.entity_fetch (provider, entity_id, reason, enqueued, attempts)` with reasons `read`, `linked`, `closure`, `event`, `sweep`, `manual`; a unique key on `(provider, entity_id)` so a busy entity is fetched once.

### A2. Instance jobs act as the instance

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §3
- **Summary:** Instance jobs (the shallow-mirror fetches of §3, run by the instance under 0040 §6) are excepted from 0002 §8.3's "every job's actor is a subsidiary": their actor is the instance (0040 §2). The row's verb is amends, but §3 never says who the fetch job's actor is, so the entry extends it; the contradicted sentence is in 0002 §8.3. (PENDING E6)

### A3. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§11
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [05](../architecture/05-providers-and-ingest.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A4. `view.term` holds the instance's languages, for shallow and full mirrors alike

- **Date:** 2026-10-09
- **Source:** [0082](0082-source-form-and-the-shared-view.md) §6
- **Change:** amends §4, §6
- **Summary:** `view.term` holds rows for local entities, for entities in any tenant overlay, and for mirrored entities in the instance's term languages only: `entities.term_languages` is instance configuration for the shared rows, set in the instance policy record, and a tenant's setting of the same name only narrows the languages its own display offers. A mirrored entity's other languages stay in the record at `entity_source.offset`, served to display through the L1 label cache of §4, which thus serves unmirrored entities and the languages `view.term` does not hold alike; a value shown under one member of a cluster may take its label from another member's rows, with precedence from the policy order. The same rule holds for a full mirror: the budget is rows ≈ mirrored entities × |instance languages| × term kinds, `wbsearchentities` at Wikidata scale is the search index, and `term_prefix` is never created on an instance holding a full mirror. Settles Q2. (REVIEW G7)

Replaced text ([05](../architecture/05-providers-and-ingest.md) §5.8, as it stood):

> **A mirrored entity's terms populate `view.term` in `entities.term_languages` only** (site setting; default: the tenant's content languages, `mul` and `en`). Its other languages stay in the record and are served by `wbgetentities`, which reads the record, but are not rows. A shallow mirror holds thousands or millions of entities, not 117 million, so the rows are affordable.

> Not yet: what a full mirror (`all`) writes to `view.term` is an open question of [0070](0070-shallow-entity-mirroring.md).

Replaced text ([05](../architecture/05-providers-and-ingest.md) §5.6, as it stood):

> An entity that is mirrored reads its terms from `view.term` instead.

### A5. A fetch miss for an unknown entity writes no record

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §3
- **Summary:** Only an entity already in the set, one with a `view.entity_source` row for the provider, that upstream reports missing becomes a `tombstone`. A fetch miss for an entity with no `entity_source` row writes no record: a typo'd or never-existing ID is not a deletion, nothing is tombstoned and no retention policy runs; the referring value renders unresolved, and the queue row is dropped. The rest of the row (a mirror tombstone carries nothing about retention; each tenant's policy is applied per overlay as a tenant-partition record under the instance attestation with a `retention/apply` event; the compaction exemption is any tenant's `retain` row; `cascade` is never a default) is [0002](0002-source-graphs-and-mass-ingest.md) §5's. The ledger names §5 too, whose text did not change. (REVIEW G28)

Replaced text ([05](../architecture/05-providers-and-ingest.md) §5.5, as it stood):

> - Each entity becomes a `put` with the same fields a dump sync writes. An upstream redirect becomes a `redirect`, and a missing entity a `tombstone`, as §3.4 says.

### A6. A provider that is a tenant of this instance is never synced

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §7
- **Summary:** The instance refuses a sync job, shallow or full, for a provider whose registry `issuer` is a tenant of this instance: that tenant's `local` partition is the provider's graph, read directly. The companion rule, that an instance refuses to adopt a tenant under a code with a live mirror partition until that partition is frozen, is [0080](0080-tenants-as-entity-sources.md) A2's. (REVIEW G31)

### A7. Moving to `all` is a bulk append into the live partition

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §5
- **Change:** extends §7
- **Summary:** The dump sync that continues from the shallow records skips every entity whose cursor is current in bulk per block, evaluated against `view.entity_source` before a block is written, and writes the rest as a bulk append into the live partition: the partition lock once per block, tier 2 composing the block set-based. (REVIEW G12)
