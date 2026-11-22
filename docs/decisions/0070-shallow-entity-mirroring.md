# 0070. Shallow entity mirroring

- **Status:** Proposed
- **Date:** 2026-10-07
- **Updated:** 2026-11-21 (A13)
- **Author:** James Hare / Claude Opus
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

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.1.*

### 2. Which entities are mirrored

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.2.*

#### 2.1 The setting

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.2, §8.2.*

#### 2.2 When `linked` fetches

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.3.*

#### 2.3 Closure properties

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.3, §8.2.*

#### 2.4 Leaving the set

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.4.*

### 3. Fetching and records

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.5.*

### 4. Labels of entities not mirrored

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.6.*

### 5. Keeping mirrored entities current (uses 0053 §6)

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.7, §8.2.*

### 6. Terms of mirrored entities (extends 0013 §5.6)

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.8.*

### 7. Becoming a full mirror

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.9.*

### 8. Local statements on mirrored entities

*Current text: [05](../architecture/05-providers-and-ingest.md) §5.10.*

### 9. API and operations (extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §2.2, §3.2.*

### 10. Storage (extends 0013 §5.6)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.12, §5.*

### 11. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

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
- **Q2. Terms under `all`.** *Settled by [0082](0082-source-form-and-the-shared-view.md) §6.*
- **Q3. Other Wikibase providers.** *Settled by [0078](0078-entity-sources.md) §4, in part.*
- **Q4. Lexemes and forms under `linked`.** A form or sense ID as a value fetches its lexeme; whether that should be the default for `lexeme`.
- **Q5. (Rest of Q3.) Other registry Wikibase providers.** *Settled by [0092](0092-following-recent-changes.md) §1, for Wikibases.*
- **Q6.** (Rest of Q5.) Whether another Triplespace instance ([0022](0022-federation.md) §2) uses the same job, or reads its log and checkpoints instead (0092 Q2).
- **Q7. Terms of a full Wikidata mirror.** Whether mirror entities populate `view.term` at all before any full Wikidata mirror (`docs/testing/performance.md` §6). (From the working notes, 0099 §5.)

## References

- [Wikibase API: `wbgetentities`](https://www.wikidata.org/w/api.php?action=help&modules=wbgetentities): batches of 50 IDs, `props=info` for `lastrevid`
- [Wikimedia EventStreams](https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams): `mediawiki.recentchange`
- `claude/adoption-results-and-wikidata-projection.md` (Triplespace project notes, 2026-10-06): why a full Wikidata sync is not yet practical, and the term-table question
- `claude/familysearch-semantic-layer-plan.md` (Triplespace project notes, 2026-10-07): the first use

## Amendment log

### A1. Entity sources

- **Date:** 2026-10-08
- **Source:** 0078 §4
- **Change:** extends §2.1, §3, §10
- **Summary:** A tenant's entity source takes `entities.mirror` and `entities.closure` under its name, with `linked` as the default where it has an `api` and `off` where it does not, and an empty closure list. Its fetch is a tenant job per source, writing to `source/{name}`, following the source's events if any and otherwise the sweep. `ops.entity_fetch` gains a `tenant` column.

Replaced text: §10 of this ADR.

### A2. Instance jobs act as the instance

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §3
- **Summary:** Instance jobs (the shallow-mirror fetches of §3, run by the instance under 0040 §6) are excepted from 0002 §8.3's "every job's actor is a subsidiary": their actor is the instance (0040 §2). The row's verb is amends, but §3 never says who the fetch job's actor is, so the entry extends it; the contradicted sentence is in 0002 §8.3. (PENDING E6)

### A3. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** 0050 §14
- **Change:** relocates §1–§11

### A4. `view.term` holds the instance's languages, for shallow and full mirrors alike

- **Date:** 2026-10-09
- **Source:** 0082 §6
- **Change:** amends §4, §6
- **Summary:** `view.term` holds rows for local entities, for entities in any tenant overlay, and for mirrored entities in the instance's term languages only: `entities.term_languages` is instance configuration for the shared rows, set in the instance policy record, and a tenant's setting of the same name only narrows the languages its own display offers. Settles Q2. Chapter text: 05 §5.8, §5.6. (REVIEW G7)

Replaced text: 05 §5.8.

Replaced text: 05 §5.6.

### A5. A fetch miss for an unknown entity writes no record

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §3
- **Summary:** Only an entity already in the set, one with a `view.entity_source` row for the provider, that upstream reports missing becomes a `tombstone`. A fetch miss for an entity with no `entity_source` row writes no record: a typo'd or never-existing ID is not a deletion, nothing is tombstoned and no retention policy runs; the referring value renders unresolved, and the queue row is dropped. The rest of the row (a mirror tombstone carries nothing about retention; each tenant's policy is applied per overlay as a tenant-partition record under the instance attestation with a `retention/apply` event; the compaction exemption is any tenant's `retain` row; `cascade` is never a default) is 0002 §5's. The ledger names §5 too, whose text did not change. (REVIEW G28)

Replaced text: 05 §5.5.

### A6. A provider that is a tenant of this instance is never synced

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §7
- **Summary:** The instance refuses a sync job, shallow or full, for a provider whose registry `issuer` is a tenant of this instance: that tenant's `local` partition is the provider's graph, read directly. The companion rule, that an instance refuses to adopt a tenant under a code with a live mirror partition until that partition is frozen, is 0080 A2's. (REVIEW G31)

### A7. Moving to `all` is a bulk append into the live partition

- **Date:** 2026-10-09
- **Source:** 0083 §5
- **Change:** extends §7
- **Summary:** The dump sync that continues from the shallow records skips every entity whose cursor is current in bulk per block, evaluated against `view.entity_source` before a block is written, and writes the rest as a bulk append into the live partition: the partition lock once per block, tier 2 composing the block set-based. (REVIEW G12)

### A8. A provider without events is followed by its recent changes

- **Date:** 2026-10-26
- **Source:** 0092 §1–§5, §7
- **Change:** extends §3, §5, §7, §9; amends §10
- **Summary:** A provider or entity source with an `api` and no `events` is followed by polling `list=recentchanges`, as the Wikidata Query Service's updater did: deduped by `rcid`, coalesced by title, read by time near the present, with a tail pass, every `follow.interval`. Chapter text: 05 §5.5, §5.7, §5.9; 03 §4.12; 18 §3.2.

Replaced text: 05 §5.7 of this ADR; 03 §4.12 of this ADR; 18 §3.2 of this ADR.

### A9. The fetch job is the follower's poll

- **Date:** 2026-11-21
- **Source:** Direct: James, 2026-11-21, choosing to fold the fetch job into the follower over a separate `fetch` job kind; the other calls below are Claude's while building it, confirmed by James, 2026-11-21
- **Change:** amends §3; extends §9, §10
- **Summary:** §3 makes the fetch a long-running job per provider beside the follower, but the follower (0092) already reads `wbgetentities` fifty IDs at a time, writes under one job a day and saves its position with each batch; a second job would repeat that and take turns with it on the load lock. The fetch job is now the follower's poll. (1) Each poll takes the oldest 500 rows of `ops.entity_fetch` for its provider or source, less those the batch still being written took, reads them with the entities the recent changes named, and the batch's last unit of work deletes their rows, each unless it was queued again after it was taken; a follower with rows still waiting is not caught up and polls again at once. (2) A follower with no position, no load and nothing in its graph starts from now when rows wait, instead of refusing: a shallow mirror has no load to start from. (3) A row names the entity as the upstream knows it (`Q42`), so a source's `lb:Q42` and a provider's `WDQ42` queue as `Q42`; the reasons gain `crawl` (0092 §4). (4) `triplespace fetch` queues entities by hand, reason `manual`. (5) `GET /provider/{code}/mirror` reports `queued`, counted to at most 10,000. (6) The queue is carried in a backup, as queued jobs are. A miss for an entity the mirror does not hold already wrote nothing (A5), and still does. (7) A request for queued entities that fails does not fail the poll: its IDs are asked one at a time, and one that still fails counts an attempt on its row and stays queued, until at the fifth it is dropped as a reject of the follower's job, so one bad ID cannot stop the queue. A request for the entities the recent changes named still fails the poll, as before. Chapter text: 05 §5.5; 03 §4.12; 18 §3.2.

Replaced text: 05 §5.5 of this ADR ("A fetch is an instance job per provider", a job beside the follower).

### A10. A linked entity is queued by a projection of its own

- **Date:** 2026-11-21
- **Source:** Direct: Claude, while building the fetch queue on James's go-ahead (2026-11-21, "Projection code is fine to change now"); confirmed by James, 2026-11-21
- **Change:** amends §2.2; extends §2.1
- **Summary:** §2.2 has the `entity_ref` projection, a tier-2 step of composition, queue each foreign target it writes, but nothing writes `view.entity_ref` yet, and its rows are keyed by page ID, which an entity no mirror holds does not have. The queuing is now a projection of its own, `fetch_queue`, in step 5 after activity, in the write's transaction: for each change set on a tenant graph that is not a mirror (`local`, `pages`, a derived graph) it collects the foreign and colon-form IDs the operation names (a statement's, a qualifier's or a reference's property or value, the subject of an `add`, `remove` or `override`, the IDs of a `same-as` or `retain`), reads them through the tenant's table of names (0078 §3), and queues with the reason `linked` each whose provider or source has an `api` and is not retired, whose `entities.mirror` for its provider code or source name and entity type is `linked` or `all` (`linked` when unset; the tenant's value, else the instance's), and which `view.entity_source` does not hold. A provider's entity is queued under `''`, a source's under the tenant. It writes no rows of its own and resets nothing; a replay queues again only what is still not mirrored. Not yet: `on-demand` (`read`), closure (§2.3), `prefetch`, and re-rendering the pages that use an entity when its fetch lands. Chapter text: 05 §5.3; 03 §6.1.

Replaced text: 05 §5.3 of this ADR ("The `entity_ref` projection, a tier-2 step of composition, adds a row to `ops.entity_fetch` for each foreign entity ID it writes as a target that has no `view.entity_source` row for its provider").

### A11. The follower keeps to the set and walks the closure; settings held per provider

- **Date:** 2026-11-21
- **Source:** Direct: Claude, while building the rest of shallow mirroring on James's go-ahead (2026-11-21, "You can finish the rest of SCA-174"); confirmed by James, 2026-11-21
- **Change:** amends §5, §2.3; extends §2.1, §10
- **Summary:** §5 follows only the set under `linked` and `on-demand` and §2.3 walks closure properties to `entities.closure_depth`, but the follower fetched every recent change, nothing walked the closure, and `entities.mirror` and `entities.closure`, which §2.1 and §2.3 hold per provider, could not be written: the settings registry checked one value of the type. The calls made: (1) The follower fetches a recent change of an entity only when its type is followed whole, or when the mirror holds the entity (a `view.entity_source` row in its graph). A type is followed whole under `all`, and, where `entities.mirror` names nothing for the type, when the mirror has been loaded (a dump sync or a crawl has recorded an end): a load makes a mirror whole, and a mirror loaded before shallow mirroring, such as Internet Domains', keeps following new entities without a setting. A registry provider's follower reads the instance's settings; a source's, its tenant's, else the instance's. (2) The closure is walked by the follower, not by a projection: each state it writes queues the values of its closure properties that are entities of the same provider or source, with the reason `closure`, one step deeper than the entity, unless the mirror holds them; an entity queued for itself, or named by a recent change, is at depth 0, and nothing deeper than `entities.closure_depth` is queued. `ops.entity_fetch` gains `depth` for it. A follower that queued closure values is not caught up, so `follow --once` fetches them in the same run. (3) `entities.closure` for `WD` defaults to P31, P279, P131 and P17 at the instance and at a tenant; for any other provider and for a source it is empty. (4) A setting entry in `settings.toml` may say `per = "provider"`, whose value is a map from a provider's code or a source's name to a value of its type, or `per = "provider-type"`, a map from provider to a map from entity type to one; `entities.closure` is the first and `entities.mirror` the second. The settings page edits such a value as JSON. (5) A follower says on stderr what each poll takes from the queue. Chapter text: 05 §5.3, §5.7; 23 §3.1; 03 §4.12.

Replaced text: 05 §5.3 ("Not yet: the re-rendering, `on-demand` fetches on read, closure and `prefetch`."); 05 §5.7 ("under `linked` and `on-demand`, those for entities already in the set (a `view.entity_source` row)").

### A12. The mirror route reports each type, and queues by hand

- **Date:** 2026-11-21
- **Source:** Direct: Claude, while building the rest of shallow mirroring on James's go-ahead (2026-11-21, "You can finish the rest of SCA-174"); confirmed by James, 2026-11-21
- **Change:** extends §9
- **Summary:** §9 has `GET /provider/{code}/mirror` report each type's setting and set size and `POST /provider/{code}/mirror/fetch` queue IDs with `ts-runjob`, but neither was built. The calls made: (1) `types` is a map from each entity type the provider or source registers to `setting`, the `entities.mirror` value its follower applies (A11: the one set, else `all` once loaded, else `linked`, or `off` for a source without an `api`; a property at least `on-demand`), and `size`, how many entities of the type its graph holds, counted to at most 10,000 as `queued` is, rather than kept as a counter. (2) `POST /provider/{code}/mirror/fetch` takes `{"ids": [...]}` with a CSRF token, each ID as the provider writes it (`Q42`) or as it is stored (`WDQ42`, `lb:Q42`), queues them with the reason `manual` as `triplespace fetch` does, and answers `queued` (rows added) and `asked`; an ID of another provider is `400 ts-not-this-provider`, a provider or source without an `api` `409 ts-no-api`. Chapter text: 18 §3.2 (the route table, from `routes.toml`, and the contract).

Replaced text: none.

### A13. A read queues what is not held; the label cache is in the process; nothing to re-render

- **Date:** 2026-11-21
- **Source:** Direct: Claude, while building the rest of shallow mirroring on James's go-ahead (2026-11-21, "You can finish the rest of SCA-174"); confirmed by James, 2026-11-21
- **Change:** amends §4, §2.2; extends §2.1
- **Summary:** §2.1 has `on-demand` fetch an entity the first time it is read, §4 serves the labels of entities not mirrored from an L1/L2 cache, and §2.2 has a fetch re-render the pages that use the entity through `view.entity_ref` and a bulk job `prefetch`; none was built, there is no L1 (Valkey) layer yet, nothing writes `view.entity_ref`, and no bulk job reads a change-set file with a `start` line. The calls made: (1) A read that finds a foreign or source entity no graph holds, through `wbgetentities` or any REST entity route (the entity page's bundle among them), queues it with the reason `read` when its provider or source has an `api` and is not retired and the reading tenant's `entities.mirror` for its type is `on-demand`, `linked` (the default) or `all`, a property at least `on-demand`; the answer is `missing` or `404` as before and does not wait. Anyone who can read may cause such a row; an ID the provider does not have costs one request and is dropped (A5). (2) The label cache lives in the serving process: an entry per entity as stored, language chain and, for a source's entity, tenant, filled by `wbgetentities&props=labels&languages={chain}` and kept for `entities.label_ttl`; an entity upstream reports missing is kept as having no labels. `GET /labels` takes from it what `view.term` and the records do not answer. At most 100 entities are asked upstream per call, each request within 3 seconds, and what is not answered is shown as its bare ID; these requests carry no `maxlag`, since a reader is waiting for them, which the API's etiquette leaves to work no one waits for. A shared L1 replaces it when one is built. (3) There is nothing to re-render: no page is cached by the instance, a page is composed from `view` on each request, and a label's staleness in an HTTP cache is accepted up to `s-maxage` (03 §10.3). `view.entity_ref` is not needed for it. (4) `prefetch` is not built, since no bulk job of change sets reads a `start` line yet; the `fetch_queue` projection (A10) already queues each block's linked entities as the block commits, so the fetch runs beside a bulk write without it. Chapter text: 05 §5.3, §5.6.

Replaced text: 05 §5.3 ("Not yet: the re-rendering, `on-demand` fetches on read and `prefetch`.").
