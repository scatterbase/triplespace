# 0014. Cache layers and search

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-01 (A12)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md)
- **Uses:** [0000](0000-init.md), [0003](0003-statement-ui.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0009](0009-keyed-entity-types-and-domain.md), [0011](0011-logs.md), [0013](0013-postgres-storage.md), [MediaWiki API contract](../api/mediawiki-compat.md)

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

*Changed by A3.*

1. **Keys name versions, not things.** A cache entry is keyed by the entity, page or record *and* the version of the data it was computed from. A write creates a new version and a new key. Old entries become unreachable and expire; nothing is purged on an ordinary edit.
2. **Mutable state lives only in Postgres.** The one mutable question, "what is the current version of X?", is answered by `view.entity.resolved_version` and `view.page.latest_offset` ([0013](0013-postgres-storage.md) §5), cached briefly in-process and nowhere else.
3. **Shared caches hold only the public form.** An entry is the response as an anonymous reader would see it: hidden fields removed, erased bodies absent. Administrators' views, which include hidden fields ([0012](0012-api-requirements.md) §8), bypass every shared layer. Nothing from a `private` graph is ever cached outside the holder's session.
4. **Erasure purges; everything else expires.** Erasure, upstream hiding ([0011](0011-logs.md) §5), and deletion and hiding by `read` ACL ([0023](0023-moderation.md) §5) are the only events that reach into caches. Every entry also has a TTL ceiling, so a purge that is missed is bounded in time.
5. **A small instance runs with no shared cache and no search service.** The layers below are optional above the in-process cache; the code paths are the same, with the shared layer absent and the Postgres fallback (§8) in place.

### 2. Layers

| Layer | Technology | Holds | Bounded by |
|---|---|---|---|
| **L0, in-process** | An in-memory LRU in each server process (`moka`) | Registries and role maps; property data types; the canonical ID of each cluster member; labels in the instance's hot languages; current versions of recently read entities and pages | Size, and a TTL of seconds for current versions |
| **L1, shared** | Valkey (Redis protocol) | §4 and §10: resolved entity JSON, term batches, rendered pages, provenance responses, structured diffs, suggestions, upstream fetches. Also rate-limit counters and sessions. | Per-key TTL ceilings (§5) |
| **L2, HTTP** | `ETag` and `Cache-Control` on anonymous responses, honoured by the reverse proxy and CDN in front of the instance | Whole responses | Short `s-maxage`, `stale-while-revalidate`, purge by tag on erasure |
| **Replicas** | Postgres streaming replicas | Everything in `view` | LSN routing ([0013](0013-postgres-storage.md) §7) |

L1 is the layer MediaWiki fills with memcached. Valkey is chosen over memcached for one reason: keys can be enumerated by prefix, so an erasure can delete every entry for an entity rather than rely on generation keys alone (§5). Sessions live in Valkey; OAuth tokens and bindings do not, they stay in Postgres `private` ([0013](0013-postgres-storage.md) §4).

### 3. Versions

*Changed by A7.*

Every cacheable thing has one version number, and the key carries it.

| Thing | Version | Where it comes from |
|---|---|---|
| Resolved entity | `resolved_version` | Bumped by the resolution projection on every re-resolution, including ones caused by another entity's cluster change ([0013](0013-postgres-storage.md) §5.1) |
| Document page | `latest_offset` | The page's newest record ([0013](0013-postgres-storage.md) §5.4) |
| Record-derived data: a diff, a change set, a proof | `(partition, offset)` | Immutable by construction |
| Label batch | The max `resolved_version` of the entities in the batch | Computed at lookup |
| Rendered page HTML | `latest_offset`, the render epoch, the renderer version ([0012](0012-api-requirements.md) §4, `action=parse`), and the interface language | The epoch is bumped when anything the render read changes, and the entry's lifetime is capped by the render's `expires_at` ([0042](0042-template-expansion-and-parsoid.md) §10) |

**`ETag` is the version.** An entity response carries `ETag: "Q42:<resolved_version>:<generation>"`. `If-None-Match` is answered from L0's current-version cache without touching Postgres when the version is unchanged. The version also appears in the JSON response body, so a client that has just written can wait for the version its write returned.

**Read-your-writes** follows from [0013](0013-postgres-storage.md) §7. The write transaction commits the new version; the response reports it; the write path also writes the new resolved JSON into L1 so the editor's next read hits. Replica routing by LSN covers the Postgres side.

### 4. What L1 holds

*Changed by A7, A11, A12.*

| Key | Value | TTL ceiling |
|---|---|---|
| `e:{id}:{gen}:{ver}` | Resolved canonical JSON, compressed | 24 h |
| `t:{lang}:{id}:{gen}:{ver}` | Label and description in one language | 24 h |
| `prov:{id}:{gen}:{ver}` | The provenance response ([0003](0003-statement-ui.md) §6, [0012](0012-api-requirements.md) §5) | 24 h |
| `p:{pageid}:{gen}:{offset}:{epoch}:{renderer}:{lang}` | Rendered HTML of a document page | 24 h, or the render's `expires_at` if sooner |
| `d:{partition}:{offset}:{gen}` | A structured diff ([0012](0012-api-requirements.md) §7) | 24 h |
| `sug:{lang}:{kinds}:{q}` | A suggestion list ([0012](0012-api-requirements.md) §5) | 60 s |
| `up:{provider}:{id}:{from}:{to}` | Upstream edits fetched live ([0012](0012-api-requirements.md) §6) | Configured; minutes |
| `rl:{scope}:{key}` | Rate-limit counters for upstream fetches and writes | The limit window |
| `s:{session}` | Session state | The session lifetime |
| `fp:{repo}:{ns}:{title}:{revid}` | A page repository's bundle: rewritten HTML and metadata ([0053](0053-mirrored-pages.md) §4) | The repository's `cache_ttl`, or until an event purges it |
| `css:{pageid}:{gen}:{offset}` | The sanitized form of a `sanitized-css` page ([0055](0055-templatestyles-templatedata-and-page-properties.md) §2) | 24 h |

`{gen}` is the entity's or page's `generation` ([0013](0013-postgres-storage.md) §5), which changes only on erasure or hiding (§5). `{ver}` is the version of §3.

**Stampedes** are handled per process: concurrent requests for one missing key share one computation (single flight), and a short lock key in L1 keeps two servers from resolving the same large entity at once.

**What is not cached.** Activity lists, histories and contributions are served from `view.activity` with continuation, and are not cached above L2: their pages change with every edit and their cost is one index scan. Anything under `/account` and `/auth` is `no-store` ([0012](0012-api-requirements.md) §8) and never enters L1 or L2.

### 5. Erasure and hiding reach every layer

*Changed by A3, A7.*

When an `erase` record is appended ([0006](0006-log-integrity-and-erasure.md) §7), an upstream hiding is followed ([0011](0011-logs.md) §5), or a `read` ACL deletes or hides something ([0023](0023-moderation.md) §5), the projection that removes the derived data also:

1. **bumps `generation`** on every affected entity and page, so every existing key for them is unreachable at once;
2. **deletes the old keys** in L1 by prefix (`e:{id}:*`, `t:*:{id}:*`, `prov:{id}:*`, `p:{pageid}:*`, `d:{partition}:{offset}:*`), so the bytes leave memory rather than wait for the ceiling;
3. **purges L2 by tag.** Every anonymous response carries a cache tag for each entity and page it drew on (`Cache-Tag: entity:Q42, page:17`); the purge names the tags. A proxy or CDN that cannot purge by tag gets a shorter `s-maxage` in configuration instead;
4. **deletes the search documents** (§7).

§10 adds the steps later ADRs need: inbox rows, delta rows, files and rendering.

A rename or vanish ([0007](0007-actor-identity.md) §4) bumps the generation of the actor's user pages and of every activity row's actor display, which is served uncached in any case.

The TTL ceilings in §4 are the bound on a missed purge. An instance under a legal deadline lowers them for the duration.

### 6. HTTP caching

*Changed by A5.*

For anonymous requests:

- `Cache-Control: public, max-age=0, s-maxage=60, stale-while-revalidate=300`, and `ETag` as in §3. Readers see at most a minute of staleness through a proxy, and none when they revalidate, because `If-None-Match` is cheap.
- `Vary: Accept, Accept-Language`. Cookies are absent on anonymous requests, so `Vary: Cookie` is not needed; an authenticated request is never served from L2.
- `Cache-Tag` as in §5.

MediaWiki purges its CDN on every edit because its `s-maxage` is long. Triplespace keeps `s-maxage` short and purges only on erasure. A proxy that can purge by tag may raise `s-maxage`; the default is chosen so that a proxy that cannot is still correct.

For authenticated requests: `Cache-Control: private, no-cache`, with `ETag` still present so the client revalidates.

Media bytes are the exception to the short `s-maxage`: a public file version is served with a long one, because any change that makes it unreadable purges its tags ([0039](0039-files-and-media.md) §7, §10 below).

### 7. Search on OpenSearch

*Changed by A2, A4, A5, A6, A7, A10, A11, A12.*

**Two indexes, one query.** This settles [0008](0008-namespaces-and-document-pages.md) Q7.

| Index | One document per | Fields |
|---|---|---|
| `entities` | Canonical entity in the resolved view: local, foreign and keyed, of every type | `id`, `type`, `provider`, `namespace` (MediaWiki number); `labels.{lang}` with `.prefix` and `.near_match` subfields; `labels_all`; `descriptions.{lang}`; `aliases.{lang}`; `key`, `key_ulabel` and `key_parents` for keyed types; `statement_keywords`; `sitelink_count`, `statement_count`, `incoming_links`; `resolved_version` |
| `pages` | Document page that is not deleted | `page_id`, `namespace`, `title` with `.prefix`, `text` (the content model rendered to plain text; with template expansion on, the rendered text, and `source_text` the source, [0042](0042-template-expansion-and-parsoid.md) §17), `content_model` (a content model registry ID; threads are `triplespace-thread`, [0041](0041-content-models.md) §10), `origin`, `latest_offset`, `categories` (for `incategory:`) and `statement_keywords` over the page's statements (for `haswbstatement:`, [0038](0038-page-metadata-and-categories.md) §12), `redirect_titles` (the titles of the page's redirects, [0051](0051-page-redirects.md) §5; redirect pages themselves are not documents unless broken or external) and `short_description` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6) |
| `pages-{repo}` | A page a repository in `mirror` mode holds, one shared index per repository at instance scope ([0053](0053-mirrored-pages.md) §7) | `title`, `text` (the HTML stripped), `categories`, `redirect_titles`, `origin`; searched beside the tenant's `pages` index when `search.inherited` is on |

Field semantics follow WikibaseCirrusSearch where a field exists there, so that its analysis configuration, its query builders and its ranking can be reused or compared:

- **`labels.{lang}`** holds the label and the aliases for that language, so a prefix match finds either. Language analyzers come from the same analysis configuration CirrusSearch uses, including the ICU plugin.
- **`labels_all`** holds every label and alias in every language, for a query with no language or a language with no label.
- **`statement_keywords`** holds `P31=Q5`-style tokens for item-valued and external-id statements in the resolved view, using canonical IDs, so `haswbstatement:` works. Properties excluded from it are registry configuration.
- **`incoming_links`** is the count of distinct referrers in `view.entity_ref`, resolved to canonical IDs. `sitelink_count` and `statement_count` are read from the resolved JSON.
- **Keyed types** ([0009](0009-keyed-entity-types-and-domain.md)): `id` is the prefixed ID, `domain:en.wikipedia.org`, as everywhere an entity ID appears ([0017](0017-entity-id-grammar.md) §4); `key` keeps the bare key in its A-label form, `key_ulabel` the display form, and `key_parents` is the list of the key and every parent by removing labels from the left, so `wikipedia.org` matches `en.wikipedia.org` and a search for a zone finds its hosts. This is the DNS hierarchy of 0009 §5, made searchable.
- **Non-canonical cluster members have no document.** Their terms are already merged into the canonical entity's resolved view. A search for a non-canonical ID is a `/resolve`, not a search.

**Queries:**

| Route | Query |
|---|---|
| `GET /suggest?q=` ([0010](0010-site-ui.md) §3) | One `msearch` with a request per kind: items and properties, foreign entities by type, keyed entities, and pages. Entities match `labels.{lang}.prefix` with the viewer's language fallback chain, rescored by `incoming_links` and `sitelink_count`; pages match `title.prefix`. If the input parses as an ID or a key, the title resolver's answer is the first suggestion and the index is not asked for it. |
| `wbsearchentities` | The entity request of `/suggest`, with `type` and `language` as given and the same continuation as Wikibase |
| `list=search` | Full text over both indexes, with `srnamespace` selecting which. Entity results use `labels_all` and `descriptions`; page results use `text` and `title`. `haswbstatement:` is honoured, and `hastemplate:` reads `view.transclusion` ([0042](0042-template-expansion-and-parsoid.md) §17). |
| `list=prefixsearch` | `title.prefix` on `pages`, and `labels.{lang}.prefix` on `entities` for entity namespaces |

**Indexing is a projection.** The search indexer is the last consumer in [0013](0013-postgres-storage.md) §7's order:

- After the resolution projection writes an entity, it enqueues `(id, resolved_version)`. The indexer reads the resolved JSON and `entity_ref` count from `view`, builds the document, and sends it in bulk with `version_type: external` and `version: resolved_version`. Out-of-order deliveries are rejected by the index, so retries and parallel workers are safe.
- A page record does the same with `latest_offset`.
- An erased or tombstoned entity, a deleted page, or an entity that stops being canonical is deleted from the index with the same versioning.
- The index holds only public data: no hidden names, no erased bodies, nothing from an internal-only or private graph. Redaction is applied when the document is built, and the erasure path of §5 deletes documents.
- **Bootstrap** indexes from `view` in one pass after the resolution projection, with the refresh interval raised and replicas set to zero for the duration, as CirrusSearch's reindex does.
- **Reindexing** into a new index and swapping an alias is the only way a mapping changes. The alias is what queries name.

**Mappings.** The mappings and analysis settings are committed as JSON under `docs/search/`, versioned, and named in the alias, so a running instance can report which mapping it serves. Building a document from the resolved JSON is pure and lives in `scatter-wikibase-model` as a function, so the audit tooling can produce the same documents without a server.

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

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with both crates this section named. The table this section first gave is in A1; the paragraph that followed it is now in §7.

### 10. Caches, indexes and stores added by later ADRs

*Changed by A3, A5, A7, A8.*

The ADRs after this one add to the layers of §2 without adding a layer, and change §4 and §7 where they say so ([0018](0018-tenants.md) §6 makes keys tenant-aware and indexes per provider and tenant). This section is the index, so that erasure (§5) and the privacy test have one list to walk.

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

**Files** ([0039](0039-files-and-media.md) §7, §20): L1 keys `fi:{tenant}:{page id}:{gen}` for a file's imageinfo and `rf:{repo}:{name}` for a foreign file's; `file_block` held whole in L0; Cache-Tags `blob:{scope}:{h}` and `file:{tenant}:{page id}` on every media response, purged by deletion, hiding, takedown and erasure. Thumbnails and proxied foreign bytes are caches kept in the blob store, not in Valkey. Responses to signed media URLs are `private, no-store`. File pages in the `pages` index gain `file_*` fields for CirrusSearch's file keywords (0039 §19).

**Stores that are not caches**, listed here because they sit beside Valkey in the deployment: the email and fediverse delivery queue in `ops` ([0021](0021-notifications.md) §8), which is durable and survives a restart; and the rate-limit counters of §4, whose classes and windows are the site policy of [0024](0024-subsidiary-accounts.md) §5 (`edit`, `create`, `move`, `link`, `job`, `read`, `stream`, `atom`, `upstream`, `notify`, `account`), keyed by actor key or, for anonymous requests, by IP. Sessions stay in Valkey (§2) and record the API key that opened them, so revoking a key can end them ([0024](0024-subsidiary-accounts.md) §4).

**Rendering** ([0042](0042-template-expansion-and-parsoid.md) §10, [0043](0043-lua-modules.md) §6): the render epoch in `p:` keys; `ld:{tenant}:{pageid}:{gen}:{offset}` for parsed data modules. Every rendered page carries a `Cache-Tag` for each page and entity in its manifest, and erasing, deleting or hiding a dependency bumps the epoch of each page that read it and purges their tags.

## Consequences

- **Ordinary edits purge nothing.** Versioned keys turn MediaWiki's invalidation problem into an expiry problem. The only invalidation code path is the erasure path, which has to exist anyway.
- **Erasure has one more list to walk.** The projection that erases must also bump generations, delete L1 keys, purge L2 tags, delete index documents and, since [0021](0021-notifications.md), delete inbox rows. A test runs an erasure end to end and checks every layer.
- **The privacy test grows.** [0012](0012-api-requirements.md) §8's test now also asserts that no L1 key and no index document contains a hidden field or private data.
- **Two more services at scale.** Valkey and OpenSearch join Postgres. A small instance runs neither, with the fallback of §8 and no shared cache.
- **Search lags the truth.** The index is a projection with its own lag. The UI's suggestions and the page it lands on can disagree by that lag.
- **Wikidata-scale search is CirrusSearch-shaped by design.** The field names and analysis settings are borrowed so that Wikimedia's tuning carries over, and so that a query written for Wikidata's `wbsearchentities` behaves the same here. Where Triplespace adds fields, keyed types and providers, the borrowing stops.
- **Anonymous readers can see a minute of staleness** through a proxy that honours `s-maxage`, and none when they revalidate. This is the trade for not purging on every edit.

## Open questions

- **Q1. TTL ceilings and `s-maxage`** in §4 and §6 are starting values. They should be set once cache hit rates and erasure obligations are known.
- **Q2. Analysis configuration.** Whether to vendor CirrusSearch's per-language analyzer settings, which are extensive and depend on plugins, or start from OpenSearch's built-in language analyzers and grow.
- **Q3. `statement_keywords` scope.** Which properties are indexed for `haswbstatement:` at Wikidata scale; indexing all of them is what makes that field large.
- **Q4. Whether the pages index should hold entity pages' rendered text,** so that a full-text search over an entity's descriptions and statement values in the viewer's language is possible without a statement table.
- **Q5. Session store.** Valkey is chosen for sessions; whether they should fall back to Postgres when the shared cache is absent, or a small instance should require Valkey for login.
- **Q6. Proxy support for tag purges.** Which reverse proxies and CDNs in the deployment stack honour `Cache-Tag`, and what `s-maxage` is safe where none does.
- **Q7. Suggest ranking for keyed types.** Whether domains should rank by `incoming_links` like items, or by their depth in the hierarchy.
- **Q8. Label caching per language at scale.** Whether `t:` keys for hundreds of languages are worth holding, or whether label batches should be cached per request language only.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §9 | extends | 0005 A9 |
| [0008](0008-namespaces-and-document-pages.md) Q7 | §7 | settles | 0008 Q7 |
| [0010](0010-site-ui.md) §3 | §7 | amends | 0010 A4 |
| [0012](0012-api-requirements.md) §8 | §1, §5–6, §8 | amends | 0012 A3 |
| [0012](0012-api-requirements.md) §4 | §1, §5–6, §8 | extends | 0012 A3 |

## References

- [Help:Extension:WikibaseCirrusSearch](https://www.mediawiki.org/wiki/Help:Extension:WikibaseCirrusSearch) and [Extension:CirrusSearch](https://www.mediawiki.org/wiki/Extension:CirrusSearch)
- [Wikimedia Search Platform decision record: search backend replacement technology](https://www.mediawiki.org/wiki/Wikimedia_Search_Platform/Decision_Records/Search_backend_replacement_technology) and [Help:CirrusSearch OpenSearch replicas](https://wikitech.wikimedia.org/wiki/Help:CirrusSearch_OpenSearch_replicas)
- [T119066: Add sitelink count to search index for Wikidata](https://phabricator.wikimedia.org/T119066)
- [Manual:Caching](https://www.mediawiki.org/wiki/Manual:Caching), [WANObjectCache](https://doc.wikimedia.org/mediawiki-core/master/php/classWikimedia_1_1ObjectCache_1_1WANObjectCache.html), [Manual:ChronologyProtector](https://www.mediawiki.org/wiki/Manual:ChronologyProtector), [Manual:$wgCdnMaxAge](https://www.mediawiki.org/wiki/Manual:$wgCdnMaxAge)
- [OpenSearch: optimistic concurrency and external versioning](https://docs.opensearch.org/latest/api-reference/document-apis/index-document/), [index aliases](https://docs.opensearch.org/latest/im-plugin/index-alias/)
- [Valkey](https://valkey.io/)
- [RFC 9111 — HTTP Caching](https://www.rfc-editor.org/rfc/rfc9111), [RFC 5861 — `stale-while-revalidate`](https://www.rfc-editor.org/rfc/rfc5861)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-26
- **Source:** [0005](0005-crate-organization.md) §2, revision of 2026-09-26
- **Change:** supersedes §9
- **Summary:** 0005 §2 became the one crate table CI checks, and carries `triplespace-cache` and `triplespace-search` (0005 A9). The paragraph on mappings and pure document building, which followed the table, moved to §7 at conversion.

Replaced text (§9):

> | Layer | Crate | Contents | Depends on |
> |---|---|---|---|
> | Triplespace | `triplespace-cache` (new) | Key construction, the L0 and L1 clients, single flight, generation handling, the L2 headers and tag purges | `triplespace-db` |
> | | `triplespace-search` (new) | The index mappings and analysis settings, the indexer projection, the query builders for §7, and the Postgres fallback of §8 | `triplespace-db`, `triplespace-projections` |

### A2. Prefixed IDs in search documents

- **Date:** 2026-09-27
- **Source:** [0017](0017-entity-id-grammar.md) §4
- **Change:** amends §7
- **Summary:** The `id` field of a search document carries the prefixed ID form; the `key` field keeps the bare key.

Replaced text (§7):

> - **Keyed types** ([0009](0009-keyed-entity-types-and-domain.md)): `key` is the A-label form, `key_ulabel` the display form,

### A3. Additions gathered in §10

- **Date:** 2026-09-27
- **Source:** [0018](0018-tenants.md) §6; [0019](0019-discussions.md) §11; [0020](0020-change-feeds.md) §5; [0021](0021-notifications.md) §2; [0023](0023-moderation.md) §2, §5, §10; [0024](0024-subsidiary-accounts.md) §4–5; [0025](0025-oauth-server.md) §5, §8; [0028](0028-tenancy-policy.md) §9, §12; [0030](0030-edit-filters.md) §11; [0031](0031-property-constraints.md) §5; [0032](0032-sparql-update-stream.md) §5–6
- **Change:** amends §1, §5; extends §10
- **Summary:** The third-pass review of 2026-09-27 gathered into §10 what 0018 through 0032 add to the layers: tenant-aware keys and per-provider and per-tenant indexes (0018, which amends §4 and §7); thread keys and documents (0019); watchlists, inboxes and account routes never cached (0020, 0021, 0024, 0025); inbox and delta rows deleted on erasure (0021, 0032); deletion and hiding through the purge path, `view.acl` in L0, and statement and property read ACLs in the public form (0023); rate-limit classes and key-bearing sessions (0024); OAuth codes in Valkey and token-bearing sessions (0025); farm-wide search and the farm-account join (0028); filter rules and throttles (0030); parsed constraints (0031). Deletion and hiding by ACL falsified §1's claim that only erasure and upstream hiding reach caches; §1 and §5 were not corrected until this conversion.

Replaced text (§1):

> 4. **Erasure purges; everything else expires.** Erasure and upstream hiding ([0011](0011-logs.md) §5) are the only events that reach into caches.

Replaced text (§5):

> When an `erase` record is appended ([0006](0006-log-integrity-and-erasure.md) §7), or an upstream hiding is followed ([0011](0011-logs.md) §5), the projection that removes the derived data also:

### A4. Page categories and statements in search

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §12
- **Change:** extends §7
- **Summary:** `pages` gains `categories` (for `incategory:`) and `statement_keywords` over the page's statements (for `haswbstatement:`).

### A5. Files

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §7, §19–20
- **Change:** extends §6, §7, §10
- **Summary:** Media serving with long `s-maxage` and tag purges; file fields in `pages`; file keys, `file_block` in L0, and file Cache-Tags. §10 had been given its Files paragraph in place.

### A6. Content model IDs

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §10
- **Change:** amends §7
- **Summary:** `content_model` holds content model registry IDs; threads are `triplespace-thread`.

Replaced text: none in the body; `content_model` had no stated vocabulary.

### A7. The render epoch and rendered text

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §10, §17
- **Change:** amends §3, §4; extends §5, §7, §10
- **Summary:** By section:
  - §3, §4: A rendered page's version also carries its **render epoch**, bumped when anything the render read changes, and its lifetime is capped by the render's `expires_at`. The `p:` key becomes `p:{pageid}:{gen}:{offset}:{epoch}:{renderer}:{lang}`.
  - §7: With expansion on, the `text` field of the `pages` index is the rendered text, and `source_text` keeps the source; `hastemplate:` reads `view.transclusion`. Recorded only in 0042 until this conversion.
  - §10: Erasing, deleting or hiding a dependency bumps the epoch of each page that read it and purges their tags. §10 had been given its Rendering paragraph in place.

Replaced text (§3):

> | Rendered page HTML | `latest_offset`, the renderer version ([0012](0012-api-requirements.md) §4, `action=parse`), and the interface language | |

Replaced text (§4):

> | `p:{pageid}:{gen}:{offset}:{renderer}:{lang}` | Rendered HTML of a document page | 24 h |

### A8. Data modules

- **Date:** 2026-09-30
- **Source:** [0043](0043-lua-modules.md) §6
- **Change:** extends §10
- **Summary:** `ld:` keys for parsed data modules. §10 had been given them in place, in its Rendering paragraph.

### A9. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–10
- **Summary:** A1–A8 were folded into the Decision. The open questions were numbered. §9's mappings paragraph moved to §7. No decision changed. Before this, A4, A6 and the §3 part of A7 were blockquotes; A5, A8 and A7's §10 paragraph had been written into §10 in place; A2, A3 and A7's §7 part were recorded only in other ADRs and, for A3, in §10. The file before conversion is commit `0b26a3a`.

### A10. Redirect titles in search

- **Date:** 2026-10-01
- **Source:** [0051](0051-page-redirects.md) §5
- **Change:** extends §7
- **Summary:** A redirect's title is indexed on its target's document in `redirect_titles`; redirect pages are documents of their own only when broken or external.

### A11. Bundles and the mirrored-page indexes

- **Date:** 2026-10-01
- **Source:** [0053](0053-mirrored-pages.md) §4, §7
- **Change:** extends §4, §7
- **Summary:** `fp:` keys hold a page repository's rewritten bundles; a repository in `mirror` mode has one shared `pages-{repo}` index, searched beside the tenant's `pages` index under `search.inherited`; in `proxy` mode foreign pages are not indexed and the suggester reads the title index.

### A12. Sanitized CSS and short descriptions

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §2, §6
- **Change:** extends §4, §7
- **Summary:** `css:` keys hold the sanitized form of `sanitized-css` pages; the `pages` index gains `short_description`.
