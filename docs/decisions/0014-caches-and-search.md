# 0014. Cache layers and search

- **Status:** Proposed
- **Date:** 2026-09-26
- **Author:** James Hare / Claude Fable
- **Amended by:** [0017 — Entity ID grammar](0017-entity-id-grammar.md), [0018 — Tenants](0018-tenants.md) (§6 amends §4 and §7: tenant-aware keys, per-provider and per-tenant indexes), [0019 — Discussions](0019-discussions.md) (§11 extends §4 and §7: thread keys and thread documents), [0020 — Change feeds](0020-change-feeds.md) (§5 applies §1 to watchlists), [0021 — Notifications](0021-notifications.md) (§2 extends §5: erasure purges inboxes), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§5 and §10 extend §5: deletion and hiding take the purge path; `view.acl` is L0-cached; the 2026-09-27 amendment of §2 adds statement and property read ACLs to the public form), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§5 gives the `rl:` counters of §4 their classes and windows; §4 has sessions carry a key ID), [0025 — The instance as an OAuth server](0025-oauth-server.md) (§8 keeps authorization and device codes in Valkey; sessions carry a token ID), [0028 — Tenancy policy](0028-tenancy-policy.md) (§9 extends §7: farm-wide search across tenant indexes, by policy; §12 caches the farm-account join per session), [0030 — Edit filters](0030-edit-filters.md) (§11: throttle counters in Valkey; compiled rules in L0), [0031 — Property constraints](0031-property-constraints.md) (§5: parsed constraints per property in L0), [0032 — The SPARQL Update stream](0032-sparql-update-stream.md) (§5 extends §5: erasure purges delta rows, a sixth step; §6 is never cached). §10 gathers these
- **Related:** [0003 — Statement UI](0003-statement-ui.md), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§9 amends §2), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (§7), [0007 — Actor identity](0007-actor-identity.md), [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md) (settles the search open question), [0009 — Keyed entity types and Domain](0009-keyed-entity-types-and-domain.md), [0010 — Site UI](0010-site-ui.md) (§3, §5.5), [0011 — Upstream and local logs](0011-logs.md) (§5), [0012 — API requirements for the site UI](0012-api-requirements.md) (§5, §6; §6 refines the caching rules of §8), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md), [MediaWiki API contract](../api/mediawiki-compat.md), [0025 — The instance as an OAuth server](0025-oauth-server.md) (§8 keeps authorization and device codes in Valkey)

## Context

[0013](0013-postgres-storage.md) makes Postgres the serving model. Two things stand between it and a site that holds up under Wikidata-scale read load and gives the search the UI asks for.

**Caching.** MediaWiki runs several layers: an in-process cache (APCu) for small hot data; a shared object cache (memcached, through `WANObjectCache`) with explicit purges and tombstones; a parser cache of rendered HTML keyed by revision and parser options; a CDN (Varnish, then ATS) in front of anonymous traffic, purged on every edit; and `ChronologyProtector` so that a user's reads follow their own writes across replicas. Wikibase adds caches for entity revisions and for terms. Most of this machinery exists because MediaWiki's cache keys name mutable things (a page title, an entity ID), so every write has to reach out and invalidate.

Triplespace's constraints differ in three ways:

1. **Everything derives from immutable log positions** ([0000](0000-init.md) §1). A cache key can name a version instead of a thing, and a versioned key never needs purging.
2. **Every response is redacted for its viewer** ([0012](0012-api-requirements.md) §8), and some graphs are private ([0007](0007-actor-identity.md) §8). A shared cache must never hold a form that one viewer may see and another may not.
3. **Erasure must reach every derived copy** ([0006](0006-log-integrity-and-erasure.md) §7 names search entries explicitly). Waiting for a TTL is not compliance.

**Search.** [0010](0010-site-ui.md) §3 asks for one box that searches entity labels, descriptions and aliases across every provider, domain keys and page titles, with suggestions grouped by kind, and a results page that also searches page text. [0012](0012-api-requirements.md) §5 gives it `GET /suggest` and `GET /resolve`. The MediaWiki contract needs `wbsearchentities`, `list=search` and `list=prefixsearch` ([mediawiki-compat.md](../api/mediawiki-compat.md) §3.3, §4.1). [0008](0008-namespaces-and-document-pages.md) leaves open whether one index covers entity terms and page text.

MediaWiki's search is CirrusSearch, which Wikimedia now runs on OpenSearch after its decision to replace Elasticsearch. Wikibase's is WikibaseCirrusSearch, which indexes per-language `labels` and `descriptions` fields with language analyzers, a cross-language `labels_all`, `statement_keywords` for `haswbstatement:` queries, and `sitelink_count`, `statement_count` and `incoming_links` for ranking. Its entity suggester, behind `wbsearchentities`, matches on `labels.{lang}.prefix` and `labels.{lang}.near_match` and rescores by those counts. Without it, Wikibase falls back to a prefix query on its SQL term store.

## Decision

### 1. Principles

1. **Keys name versions, not things.** A cache entry is keyed by the entity, page or record *and* the version of the data it was computed from. A write creates a new version and a new key. Old entries become unreachable and expire; nothing is purged on an ordinary edit.
2. **Mutable state lives only in Postgres.** The one mutable question, "what is the current version of X?", is answered by `view.entity.resolved_version` and `view.page.latest_offset` ([0013](0013-postgres-storage.md) §5), cached briefly in-process and nowhere else.
3. **Shared caches hold only the public form.** An entry is the response as an anonymous reader would see it: hidden fields removed, erased bodies absent. Administrators' views, which include hidden fields ([0012](0012-api-requirements.md) §8), bypass every shared layer. Nothing from a `private` graph is ever cached outside the holder's session.
4. **Erasure purges; everything else expires.** Erasure and upstream hiding ([0011](0011-logs.md) §5) are the only events that reach into caches. Every entry also has a TTL ceiling, so a purge that is missed is bounded in time.
5. **A small instance runs with no shared cache and no search service.** The layers below are optional above the in-process cache; the code paths are the same, with the shared layer absent and the Postgres fallback (§8) in place.

### 2. Layers

| Layer | Technology | Holds | Bounded by |
|---|---|---|---|
| **L0, in-process** | An in-memory LRU in each server process (`moka`) | Registries and role maps; property data types; the canonical ID of each cluster member; labels in the instance's hot languages; current versions of recently read entities and pages | Size, and a TTL of seconds for current versions |
| **L1, shared** | Valkey (Redis protocol) | §4: resolved entity JSON, term batches, rendered pages, provenance responses, structured diffs, suggestions, upstream fetches. Also rate-limit counters and sessions. | Per-key TTL ceilings (§5) |
| **L2, HTTP** | `ETag` and `Cache-Control` on anonymous responses, honoured by the reverse proxy and CDN in front of the instance | Whole responses | Short `s-maxage`, `stale-while-revalidate`, purge by tag on erasure |
| **Replicas** | Postgres streaming replicas | Everything in `view` | LSN routing ([0013](0013-postgres-storage.md) §7) |

L1 is the layer MediaWiki fills with memcached. Valkey is chosen over memcached for one reason: keys can be enumerated by prefix, so an erasure can delete every entry for an entity rather than rely on generation keys alone (§5). Sessions live in Valkey; OAuth tokens and bindings do not, they stay in Postgres `private` ([0013](0013-postgres-storage.md) §4).

### 3. Versions

Every cacheable thing has one version number, and the key carries it.

| Thing | Version | Where it comes from |
|---|---|---|
| Resolved entity | `resolved_version` | Bumped by the resolution projection on every re-resolution, including ones caused by another entity's cluster change ([0013](0013-postgres-storage.md) §5.1) |
| Document page | `latest_offset` | The page's newest record ([0013](0013-postgres-storage.md) §5.4) |
| Record-derived data: a diff, a change set, a proof | `(partition, offset)` | Immutable by construction |
| Label batch | The max `resolved_version` of the entities in the batch | Computed at lookup |
| Rendered page HTML | `latest_offset`, the renderer version ([0012](0012-api-requirements.md) §4, `action=parse`), and the interface language | |

**`ETag` is the version.** An entity response carries `ETag: "Q42:<resolved_version>:<generation>"`. `If-None-Match` is answered from L0's current-version cache without touching Postgres when the version is unchanged. The version also appears in the JSON response body, so a client that has just written can wait for the version its write returned.

**Read-your-writes** follows from [0013](0013-postgres-storage.md) §7. The write transaction commits the new version; the response reports it; the write path also writes the new resolved JSON into L1 so the editor's next read hits. Replica routing by LSN covers the Postgres side.

### 4. What L1 holds

| Key | Value | TTL ceiling |
|---|---|---|
| `e:{id}:{gen}:{ver}` | Resolved canonical JSON, compressed | 24 h |
| `t:{lang}:{id}:{gen}:{ver}` | Label and description in one language | 24 h |
| `prov:{id}:{gen}:{ver}` | The provenance response ([0003](0003-statement-ui.md) §6, [0012](0012-api-requirements.md) §5) | 24 h |
| `p:{pageid}:{gen}:{offset}:{renderer}:{lang}` | Rendered HTML of a document page | 24 h |
| `d:{partition}:{offset}:{gen}` | A structured diff ([0012](0012-api-requirements.md) §7) | 24 h |
| `sug:{lang}:{kinds}:{q}` | A suggestion list ([0012](0012-api-requirements.md) §5) | 60 s |
| `up:{provider}:{id}:{from}:{to}` | Upstream edits fetched live ([0012](0012-api-requirements.md) §6) | Configured; minutes |
| `rl:{scope}:{key}` | Rate-limit counters for upstream fetches and writes | The limit window |
| `s:{session}` | Session state | The session lifetime |

`{gen}` is the entity's or page's `generation` ([0013](0013-postgres-storage.md) §5), which changes only on erasure or hiding (§5). `{ver}` is the version of §3.

**Stampedes** are handled per process: concurrent requests for one missing key share one computation (single flight), and a short lock key in L1 keeps two servers from resolving the same large entity at once.

**What is not cached.** Activity lists, histories and contributions are served from `view.activity` with continuation, and are not cached above L2: their pages change with every edit and their cost is one index scan. Anything under `/account` and `/auth` is `no-store` ([0012](0012-api-requirements.md) §8) and never enters L1 or L2.

### 5. Erasure and hiding reach every layer

When an `erase` record is appended ([0006](0006-log-integrity-and-erasure.md) §7), or an upstream hiding is followed ([0011](0011-logs.md) §5), the projection that removes the derived data also:

1. **bumps `generation`** on every affected entity and page, so every existing key for them is unreachable at once;
2. **deletes the old keys** in L1 by prefix (`e:{id}:*`, `t:*:{id}:*`, `prov:{id}:*`, `p:{pageid}:*`, `d:{partition}:{offset}:*`), so the bytes leave memory rather than wait for the ceiling;
3. **purges L2 by tag.** Every anonymous response carries a cache tag for each entity and page it drew on (`Cache-Tag: entity:Q42, page:17`); the purge names the tags. A proxy or CDN that cannot purge by tag gets a shorter `s-maxage` in configuration instead;
4. **deletes the search documents** (§7).

A rename or vanish ([0007](0007-actor-identity.md) §4) bumps the generation of the actor's user pages and of every activity row's actor display, which is served uncached in any case.

The TTL ceilings in §4 are the bound on a missed purge. An instance under a legal deadline lowers them for the duration.

### 6. HTTP caching

For anonymous requests:

- `Cache-Control: public, max-age=0, s-maxage=60, stale-while-revalidate=300`, and `ETag` as in §3. Readers see at most a minute of staleness through a proxy, and none when they revalidate, because `If-None-Match` is cheap.
- `Vary: Accept, Accept-Language`. Cookies are absent on anonymous requests, so `Vary: Cookie` is not needed; an authenticated request is never served from L2.
- `Cache-Tag` as in §5.

MediaWiki purges its CDN on every edit because its `s-maxage` is long. Triplespace keeps `s-maxage` short and purges only on erasure. A proxy that can purge by tag may raise `s-maxage`; the default is chosen so that a proxy that cannot is still correct.

For authenticated requests: `Cache-Control: private, no-cache`, with `ETag` still present so the client revalidates.

### 7. Search on OpenSearch

**Two indexes, one query.** This settles [0008](0008-namespaces-and-document-pages.md)'s open question.

| Index | One document per | Fields |
|---|---|---|
| `entities` | Canonical entity in the resolved view: local, foreign and keyed, of every type | `id`, `type`, `provider`, `namespace` (MediaWiki number); `labels.{lang}` with `.prefix` and `.near_match` subfields; `labels_all`; `descriptions.{lang}`; `aliases.{lang}`; `key`, `key_ulabel` and `key_parents` for keyed types; `statement_keywords`; `sitelink_count`, `statement_count`, `incoming_links`; `resolved_version` |
| `pages` | Document page that is not deleted | `page_id`, `namespace`, `title` with `.prefix`, `text` (the content model rendered to plain text), `content_model`, `origin`, `latest_offset` |

Field semantics follow WikibaseCirrusSearch where a field exists there, so that its analysis configuration, its query builders and its ranking can be reused or compared:

- **`labels.{lang}`** holds the label and the aliases for that language, so a prefix match finds either. Language analyzers come from the same analysis configuration CirrusSearch uses, including the ICU plugin.
- **`labels_all`** holds every label and alias in every language, for a query with no language or a language with no label.
- **`statement_keywords`** holds `P31=Q5`-style tokens for item-valued and external-id statements in the resolved view, using canonical IDs, so `haswbstatement:` works. Properties excluded from it are registry configuration.
- **`incoming_links`** is the count of distinct referrers in `view.entity_ref`, resolved to canonical IDs. `sitelink_count` and `statement_count` are read from the resolved JSON.
- **Keyed types** ([0009](0009-keyed-entity-types-and-domain.md)): `key` is the A-label form, `key_ulabel` the display form, and `key_parents` is the list of the key and every parent by removing labels from the left, so `wikipedia.org` matches `en.wikipedia.org` and a search for a zone finds its hosts. This is the DNS hierarchy of 0009 §5, made searchable.
- **Non-canonical cluster members have no document.** Their terms are already merged into the canonical entity's resolved view. A search for a non-canonical ID is a `/resolve`, not a search.

**Queries:**

| Route | Query |
|---|---|
| `GET /suggest?q=` ([0010](0010-site-ui.md) §3) | One `msearch` with a request per kind: items and properties, foreign entities by type, keyed entities, and pages. Entities match `labels.{lang}.prefix` with the viewer's language fallback chain, rescored by `incoming_links` and `sitelink_count`; pages match `title.prefix`. If the input parses as an ID or a key, the title resolver's answer is the first suggestion and the index is not asked for it. |
| `wbsearchentities` | The entity request of `/suggest`, with `type` and `language` as given and the same continuation as Wikibase |
| `list=search` | Full text over both indexes, with `srnamespace` selecting which. Entity results use `labels_all` and `descriptions`; page results use `text` and `title`. `haswbstatement:` is honoured. |
| `list=prefixsearch` | `title.prefix` on `pages`, and `labels.{lang}.prefix` on `entities` for entity namespaces |

**Indexing is a projection.** The search indexer is the last consumer in [0013](0013-postgres-storage.md) §7's order:

- After the resolution projection writes an entity, it enqueues `(id, resolved_version)`. The indexer reads the resolved JSON and `entity_ref` count from `view`, builds the document, and sends it in bulk with `version_type: external` and `version: resolved_version`. Out-of-order deliveries are rejected by the index, so retries and parallel workers are safe.
- A page record does the same with `latest_offset`.
- An erased or tombstoned entity, a deleted page, or an entity that stops being canonical is deleted from the index with the same versioning.
- The index holds only public data: no hidden names, no erased bodies, nothing from an internal-only or private graph. Redaction is applied when the document is built, and the erasure path of §5 deletes documents.
- **Bootstrap** indexes from `view` in one pass after the resolution projection, with the refresh interval raised and replicas set to zero for the duration, as CirrusSearch's reindex does.
- **Reindexing** into a new index and swapping an alias is the only way a mapping changes. The alias is what queries name.

**Consistency.** The index is eventually consistent with `view`, by the projection's lag. A suggestion may name an entity whose page has moved on; the page is served from `view`, and the difference is the lag reported by `ops.projection_state`.

### 8. Postgres fallback

Where OpenSearch is absent, the same routes are served from Postgres:

| Route | Fallback |
|---|---|
| `/suggest`, `wbsearchentities`, `list=prefixsearch` for entities | `view.term` with the `term_prefix` index ([0013](0013-postgres-storage.md) §5.2): `lower(text) LIKE lower($q) || '%'` in the requested language and its fallbacks, ordered by a count from `entity_ref` |
| `list=prefixsearch` for pages | `view.page` on `(ns, title)` |
| `list=search` | A `tsvector` column on a `view.page_text` table with a GIN index; entities by `term_prefix` |

The fallback has no `haswbstatement:`, no ranking beyond incoming links, and no analysis. It is meant for instances small enough that a prefix scan is fast, which the profile table in [0013](0013-postgres-storage.md) §11 defines. `meta=siteinfo&siprop=triplespace` ([0012](0012-api-requirements.md) §4) reports `search: opensearch` or `search: postgres` so the UI knows which features to show.

### 9. Crates

| Layer | Crate | Contents | Depends on |
|---|---|---|---|
| Triplespace | `triplespace-cache` (new) | Key construction, the L0 and L1 clients, single flight, generation handling, the L2 headers and tag purges | `triplespace-db` |
| | `triplespace-search` (new) | The index mappings and analysis settings, the indexer projection, the query builders for §7, and the Postgres fallback of §8 | `triplespace-db`, `triplespace-projections` |

The mappings and analysis settings are committed as JSON under `docs/search/`, versioned, and named in the alias, so a running instance can report which mapping it serves. Building a document from the resolved JSON is pure and lives in `scatter-wikibase-model` as a function, so the audit tooling can produce the same documents without a server.

### 10. Caches, indexes and stores added by later ADRs

The ADRs after this one add to the layers of §2 without adding a layer. This section is the index, so that erasure (§5) and the privacy test have one list to walk.

**L1 keys** (extends §4):

| Key | Value | Added by |
|---|---|---|
| `post:{revid}:{gen}:{offset}` | A rendered post; `{offset}` is the record holding its current text | [0019](0019-discussions.md) §11 |
| `th:{pageid}:{gen}:{revid}` | A thread, keyed by its page ID and latest revision ID | 0019 §11 |
| `tp:{pageid}:{gen}:{revid}` | A talk page's thread listing, keyed likewise by the talk page ID | 0019 §11 |
| `t:`, `e:`, `prov:` and the rest of §4 | Gain a tenant component where the value is tenant-specific, and omit it where the value is shared, so a mirrored entity's JSON is cached once for every tenant that reads it unchanged | [0018](0018-tenants.md) §6 |

**L0 additions** (extends §2), all small, all invalidated by the record that changes them: the current ACLs of a target and its enclosures, including the `statement` and `property` targets ([0023](0023-moderation.md) §2, §10); the compiled CEL rule of every enabled filter ([0030](0030-edit-filters.md) §11); the parsed constraints of a property, keyed by its `resolved_version` ([0031](0031-property-constraints.md) §5); the class chain walked by a *type* constraint check, to its depth limit (0031 §2); the farm-account join behind global groups and blocks, per session ([0028](0028-tenancy-policy.md) §12).

**Valkey keys that are not caches** (extends §4): `rl:filter:{id}:{key}`, a filter's throttle counter with the filter's window as TTL ([0030](0030-edit-filters.md) §4, §11); `oauth:code:{hash}` and `oauth:device:{hash}`, authorization and device codes with a ten-minute TTL ([0025](0025-oauth-server.md) §8). Sessions (`s:`) record the API key ID or OAuth token ID that opened them, so revoking either can end them ([0024](0024-subsidiary-accounts.md) §4, [0025](0025-oauth-server.md) §5).

**Never cached above L2,** in addition to the activity lists of §4: the watchlist and every response under `/watchlist` ([0020](0020-change-feeds.md) §5), the inbox and everything under `/inbox` and `/notifications` ([0021](0021-notifications.md) §7), everything under `/account`, including preferences, subsidiaries, keys, authorizations and the data export ([0024](0024-subsidiary-accounts.md) §8, [0025](0025-oauth-server.md) §9, [0027](0027-preferences-and-portability.md) §6), all of which are `Cache-Control: private` or `no-store`. The everything feed's page and Atom forms are public and cache briefly at L2 (0020 §5). The stream ([0020](0020-change-feeds.md) §4) is not cached; it is a fan-out from the activity projection. Constraint reports and the filter log are feeds or lists over `view` and follow the activity-list rule. The SPARQL Update stream and `/updates` ([0032](0032-sparql-update-stream.md) §6) are range reads over `view.rdf_delta` and are not cached either; its batch files under `/dumps/updates/` are immutable once written and cache like dumps.

**Erasure** (extends §5) has a fifth step: **delete the inbox rows** whose activity was hidden or erased, in every inbox ([0021](0021-notifications.md) §2), and a sixth: **delete or rewrite the `rdf_delta` rows** within the retention window whose triples include the erased ones, and emit the deleting delta ([0032](0032-sparql-update-stream.md) §5). The end-to-end erasure test of the Consequences covers both. **Deletion and hiding** ([0023](0023-moderation.md) §4–5) take the same path: a `read` ACL on a page, entity, record, actor, statement or property bumps the target's generation, purges its keys and tags, deletes or re-sends its search document and clears its inbox rows, because the shared layers hold only the public form (§1) and the public form has changed; a `property` read ACL bumps every entity the property appears on, found through `view.statement_assertion` and the resolved JSON, which is the one moderation action with a fan-out. Protection purges nothing. A **sitelink or federation policy change** ([0026](0026-sitelinks.md) §3, [0022](0022-federation.md) §8) re-resolves the affected entities through the `host` index and bumps their versions, not their generations, since nothing was hidden for privacy.

**Search** (extends §7):

- **Indexes multiply by provider and tenant** ([0018](0018-tenants.md) §6): one shared `entities` index per provider, holding that provider's canonical entities as the shared view has them, and one index per tenant holding its local entities and its overlay documents. `/suggest` and `wbsearchentities` run one `msearch` across the tenant's own index and the indexes of its opted-in providers; where a tenant document and a shared document share an ID, the tenant's wins. `incoming_links` on a shared document is the instance-wide count.
- **A thread is one document in the `pages` index** ([0019](0019-discussions.md) §11): subject plus post text, with the `Thread` namespace; hidden and erased text is not indexed, and the document is re-sent with the thread's latest revision ID as its external version.
- **Farm-wide search** ([0028](0028-tenancy-policy.md) §9): under `search.farm_wide`, `/suggest` and `list=search` at the farm base run one `msearch` across every tenant's index and every provider index, each hit carrying its tenant, gated to global groups or to everyone by policy. Redaction stays per tenant.
- **Sitelinks and resolvers add nothing to the indexes.** A denied host's sitelink is left out of the entity document because it is left out of the resolved view ([0026](0026-sitelinks.md) §3); a resolver key is recognised by the title resolver before the index is asked ([0029](0029-resolver-namespaces.md) §4), as an ID is.

**Stores that are not caches**, listed here because they sit beside Valkey in the deployment: the email and fediverse delivery queue in `ops` ([0021](0021-notifications.md) §8), which is durable and survives a restart; and the rate-limit counters of §4, whose classes and windows are the site policy of [0024](0024-subsidiary-accounts.md) §5 (`edit`, `create`, `move`, `link`, `job`, `read`, `stream`, `atom`, `upstream`, `notify`, `account`), keyed by actor key or, for anonymous requests, by IP. Sessions stay in Valkey (§2) and record the API key that opened them, so revoking a key can end them ([0024](0024-subsidiary-accounts.md) §4).

## Consequences

- **Ordinary edits purge nothing.** Versioned keys turn MediaWiki's invalidation problem into an expiry problem. The only invalidation code path is the erasure path, which has to exist anyway.
- **Erasure has one more list to walk.** The projection that erases must also bump generations, delete L1 keys, purge L2 tags, delete index documents and, since [0021](0021-notifications.md), delete inbox rows. A test runs an erasure end to end and checks every layer.
- **The privacy test grows.** [0012](0012-api-requirements.md) §8's test now also asserts that no L1 key and no index document contains a hidden field or private data.
- **Two more services at scale.** Valkey and OpenSearch join Postgres. A small instance runs neither, with the fallback of §8 and no shared cache.
- **Search lags the truth.** The index is a projection with its own lag. The UI's suggestions and the page it lands on can disagree by that lag.
- **Wikidata-scale search is CirrusSearch-shaped by design.** The field names and analysis settings are borrowed so that Wikimedia's tuning carries over, and so that a query written for Wikidata's `wbsearchentities` behaves the same here. Where Triplespace adds fields, keyed types and providers, the borrowing stops.
- **Anonymous readers can see a minute of staleness** through a proxy that honours `s-maxage`, and none when they revalidate. This is the trade for not purging on every edit.

## Open questions

- **TTL ceilings and `s-maxage`** in §4 and §6 are starting values. They should be set once cache hit rates and erasure obligations are known.
- **Analysis configuration.** Whether to vendor CirrusSearch's per-language analyzer settings, which are extensive and depend on plugins, or start from OpenSearch's built-in language analyzers and grow.
- **`statement_keywords` scope.** Which properties are indexed for `haswbstatement:` at Wikidata scale; indexing all of them is what makes that field large.
- **Whether the pages index should hold entity pages' rendered text,** so that a full-text search over an entity's descriptions and statement values in the viewer's language is possible without a statement table.
- **Session store.** Valkey is chosen for sessions; whether they should fall back to Postgres when the shared cache is absent, or a small instance should require Valkey for login.
- **Proxy support for tag purges.** Which reverse proxies and CDNs in the deployment stack honour `Cache-Tag`, and what `s-maxage` is safe where none does.
- **Suggest ranking for keyed types.** Whether domains should rank by `incoming_links` like items, or by their depth in the hierarchy.
- **Label caching per language at scale.** Whether `t:` keys for hundreds of languages are worth holding, or whether label batches should be cached per request language only.

## References

- [Help:Extension:WikibaseCirrusSearch](https://www.mediawiki.org/wiki/Help:Extension:WikibaseCirrusSearch) and [Extension:CirrusSearch](https://www.mediawiki.org/wiki/Extension:CirrusSearch)
- [Wikimedia Search Platform decision record: search backend replacement technology](https://www.mediawiki.org/wiki/Wikimedia_Search_Platform/Decision_Records/Search_backend_replacement_technology) and [Help:CirrusSearch OpenSearch replicas](https://wikitech.wikimedia.org/wiki/Help:CirrusSearch_OpenSearch_replicas)
- [T119066: Add sitelink count to search index for Wikidata](https://phabricator.wikimedia.org/T119066)
- [Manual:Caching](https://www.mediawiki.org/wiki/Manual:Caching), [WANObjectCache](https://doc.wikimedia.org/mediawiki-core/master/php/classWikimedia_1_1ObjectCache_1_1WANObjectCache.html), [Manual:ChronologyProtector](https://www.mediawiki.org/wiki/Manual:ChronologyProtector), [Manual:$wgCdnMaxAge](https://www.mediawiki.org/wiki/Manual:$wgCdnMaxAge)
- [OpenSearch: optimistic concurrency and external versioning](https://docs.opensearch.org/latest/api-reference/document-apis/index-document/), [index aliases](https://docs.opensearch.org/latest/im-plugin/index-alias/)
- [Valkey](https://valkey.io/)
- [RFC 9111 — HTTP Caching](https://www.rfc-editor.org/rfc/rfc9111), [RFC 5861 — `stale-while-revalidate`](https://www.rfc-editor.org/rfc/rfc5861)
