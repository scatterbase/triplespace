# 0014. Cache layers and search

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-11-21 (A30)
- **Author:** James Hare / Claude Fable
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

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.1, §13.*

### 2. Layers

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.2, §12.2.*

### 3. Versions

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.3.*

### 4. What L1 holds

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.4, §9.7, §12.1, §12.3.*

### 5. Erasure and hiding reach every layer

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.6, §12.4.*

### 6. HTTP caching

*Current text: [03](../architecture/03-storage-caches-and-search.md) §10.1, §10.3.*

### 7. Search on OpenSearch

*Current text: [03](../architecture/03-storage-caches-and-search.md) §11.1, §11.2, §11.3, §11.5, §12.5.*

### 8. Postgres fallback

*Current text: [03](../architecture/03-storage-caches-and-search.md) §5, §11.6.*

### 9. Crates

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 10. Caches, indexes and stores added by later ADRs

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.4, §9.5, §9.6, §9.7, §11.4, §12.1, §12.2, §12.3, §12.4, §12.5, §13.*

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
- **Q5. Session store.** *Settled by A14.*
- **Q6. Proxy support for tag purges.** Which reverse proxies and CDNs in the deployment stack honour `Cache-Tag`, and what `s-maxage` is safe where none does.
- **Q7. Suggest ranking for keyed types.** Whether domains should rank by `incoming_links` like items, or by their depth in the hierarchy.
- **Q8. Label caching per language at scale.** Whether `t:` keys for hundreds of languages are worth holding, or whether label batches should be cached per request language only.

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
- **Source:** 0005 §2, revision of 2026-09-26
- **Change:** supersedes §9
- **Summary:** 0005 §2 became the one crate table CI checks, and carries `triplespace-cache` and `triplespace-search` (0005 A9). The paragraph on mappings and pure document building, which followed the table, moved to §7 at conversion.

Replaced text: §9 of this ADR.

### A2. Prefixed IDs in search documents

- **Date:** 2026-09-27
- **Source:** 0017 §4
- **Change:** amends §7
- **Summary:** The `id` field of a search document carries the prefixed ID form; the `key` field keeps the bare key.

Replaced text: §7 of this ADR.

### A3. Additions gathered in §10

- **Date:** 2026-09-27
- **Source:** 0018 §6; 0019 §11; 0020 §5; 0021 §2; 0023 §2, §5, §10; 0024 §4–5; 0025 §5, §8; 0028 §9, §12; 0030 §11; 0031 §5; 0032 §5–6
- **Change:** amends §1, §5; extends §10
- **Summary:** The third-pass review of 2026-09-27 gathered into §10 what 0018 through 0032 add to the layers: tenant-aware keys and per-provider and per-tenant indexes (0018, which amends §4 and §7); thread keys and documents (0019); watchlists, inboxes and account routes never cached (0020, 0021, 0024, 0025); inbox and delta rows deleted on erasure (0021, 0032); deletion and hiding through the purge path, `view.acl` in L0, and statement and property read ACLs in the public form (0023); rate-limit classes and key-bearing sessions (0024); OAuth codes in Valkey and token-bearing sessions (0025); farm-wide search and the farm-account join (0028); filter rules and throttles (0030); parsed constraints (0031). Deletion and hiding by ACL falsified §1's claim that only erasure and upstream hiding reach caches; §1 and §5 were not corrected until this conversion.

Replaced text: §1 of this ADR; §5 of this ADR.

### A4. Page categories and statements in search

- **Date:** 2026-09-29
- **Source:** 0038 §12
- **Change:** extends §7
- **Summary:** `pages` gains `categories` (for `incategory:`) and `statement_keywords` over the page's statements (for `haswbstatement:`).

### A5. Files

- **Date:** 2026-09-30
- **Source:** 0039 §7, §19–20
- **Change:** extends §6, §7, §10
- **Summary:** Media serving with long `s-maxage` and tag purges; file fields in `pages`; file keys, `file_block` in L0, and file Cache-Tags. §10 had been given its Files paragraph in place.

### A6. Content model IDs

- **Date:** 2026-09-30
- **Source:** 0041 §10
- **Change:** amends §7
- **Summary:** `content_model` holds content model registry IDs; threads are `triplespace-thread`.

Replaced text: none in the body; `content_model` had no stated vocabulary.

### A7. The render epoch and rendered text

- **Date:** 2026-09-30
- **Source:** 0042 §10, §17
- **Change:** amends §3, §4; extends §5, §7, §10
- **Summary:** By section:
  - §3, §4: A rendered page's version also carries its **render epoch**, bumped when anything the render read changes, and its lifetime is capped by the render's `expires_at`. The `p:` key becomes `p:{pageid}:{gen}:{offset}:{epoch}:{renderer}:{lang}`.
  - §7: With expansion on, the `text` field of the `pages` index is the rendered text, and `source_text` keeps the source; `hastemplate:` reads `view.transclusion`. Recorded only in 0042 until this conversion.
  - §10: Erasing, deleting or hiding a dependency bumps the epoch of each page that read it and purges their tags. §10 had been given its Rendering paragraph in place.

Replaced text: §3 of this ADR; §4 of this ADR.

### A8. Data modules

- **Date:** 2026-09-30
- **Source:** 0043 §6
- **Change:** extends §10
- **Summary:** `ld:` keys for parsed data modules. §10 had been given them in place, in its Rendering paragraph.

### A9. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** 0050 §13
- **Change:** consolidates §1–10
- **Summary:** A1–A8 were folded into the Decision. The open questions were numbered. §9's mappings paragraph moved to §7. No decision changed. Before this, A4, A6 and the §3 part of A7 were blockquotes; A5, A8 and A7's §10 paragraph had been written into §10 in place; A2, A3 and A7's §7 part were recorded only in other ADRs and, for A3, in §10. The file before conversion is commit `0b26a3a`.

### A10. Redirect titles in search

- **Date:** 2026-10-01
- **Source:** 0051 §5
- **Change:** extends §7
- **Summary:** A redirect's title is indexed on its target's document in `redirect_titles`; redirect pages are documents of their own only when broken or external.

### A11. Bundles and the mirrored-page indexes

- **Date:** 2026-10-01
- **Source:** 0053 §4, §7
- **Change:** extends §4, §7
- **Summary:** `fp:` keys hold a page repository's rewritten bundles; a repository in `mirror` mode has one shared `pages-{repo}` index, searched beside the tenant's `pages` index under `search.inherited`; in `proxy` mode foreign pages are not indexed and the suggester reads the title index.

### A12. Sanitized CSS and short descriptions

- **Date:** 2026-10-01
- **Source:** 0055 §2, §6
- **Change:** extends §4, §7
- **Summary:** `css:` keys hold the sanitized form of `sanitized-css` pages; the `pages` index gains `short_description`.

### A13. Caches and the index keyed by visibility

- **Date:** 2026-10-01
- **Source:** 0056 §7, §8
- **Change:** amends §1, §4, §7
- **Summary:** Shared caches hold the form for a visibility set, keyed by it, so that a private wiki caches as a public one does; the public form is the empty set's entry and the only one L2 may hold. The L1 keys gain a `{vis}` segment with the tenant's visibility epoch. The text is in git (`tools/adr.py log 0014 A13 --diff`).

Replaced text: §1 of this ADR; §4 of this ADR; §7 of this ADR.

### A14. Sessions fall back to Postgres

- **Date:** 2026-10-03
- **Source:** Direct: James, decision of 2026-10-03 (`triplespace-accounts`, `triplespace-api-action`, `triplespace-server`, `scatter-adapter-internetdomains`)
- **Change:** extends §2
- **Summary:** An instance without a shared cache keeps sessions in `private.session` (0013 §5.6, as amended) rather than requiring Valkey for login; the cookie is `triplespace_session` (`HttpOnly`, `SameSite=Lax`, `Secure` outside development mode), an anonymous session lives an hour and a logged-in one thirty days from its last use, and expired rows are swept on login. When Valkey is present it may hold the same rows as `s:{session}`; the shape is the same.

### A15. The web tier's cache and composed pages

- **Date:** 2026-10-03
- **Source:** 0057 §5, §6
- **Change:** extends §2, §6
- **Summary:** The web tier may cache public API responses to its credential-free requests, in memory or in a named Valkey. It cannot be purged, and it sits beneath a CDN that can, so it revalidates every use, keeping without revalidation only responses whose `max-age` the API sets because no purge, erasure or moderation can change them, for at most 60 seconds; it never honours `s-maxage`. A page it composes is public only if all its inputs were, with an `ETag` and `Cache-Tag` derived from theirs.

### A16. The fragment cache

- **Date:** 2026-10-04
- **Source:** 0058 §11
- **Change:** extends §10
- **Summary:** Packed storage adds an L0 fragment cache: immutable fragments by domain and ref, and the refs known to be shared in the domain's current epoch. Erasure gains a seventh step, the fragment sweep and the cache drop that follows it.

### A17. Lexemes, schemas and captions in search

- **Date:** 2026-10-05
- **Source:** 0066 §5; 0064 §7; 0065 §1
- **Change:** extends §7
- **Summary:** Lexeme documents and the three lexeme search types; schema terms and text; captions as File page terms.

### A18. Scope and sprint listing keys

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §10
- **Summary:** The L1 key list of §10 gains the `sc:` rows of 0060 §10 and the `sp:` rows of 0061 §11, the scope and sprint listings. (PENDING A11)

### A19. Where session state lives

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §2
- **Summary:** Session state lives in Valkey when a shared cache is configured and in `private` otherwise (0025 §8; A14), so §2 no longer states Valkey as the one home of sessions; 0013 §4's "sessions' secrets" means that fallback. (PENDING A12)

Replaced text: §2 of this ADR.

### A20. Listing keys carry a listing version

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §10
- **Summary:** Listing keys (`tp:`, `sc:`, `sp:`) carry a **listing version**, the maximum activity ID over the listing's members, computed in Postgres at read time, in place of tag purges on ordinary writes and in place of the talk page's latest revision ID; §1's principles 1 and 4 hold unchanged. Tag purges remain for erasure and hiding. The same holds in 0049 §12, 0069 §10, 0060 §10 and 0061 §11. (PENDING B4)

Replaced text: §10 of this ADR.

### A21. Every viewer-dependent key carries `{vis}`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §10
- **Summary:** Rule: every key whose value depends on what the viewer may read carries `{vis}` (0056 §7), and §10's key list marks each key. Classification, to confirm: carry `{vis}`: `e:`, `t:`, `prov:`, `p:`, `d:`, `sug:`, `css:`, `post:`, `th:`, `tp:`, `fi:`, `rf:`, `ld:`, `sc:`, `sp:`; do not: `s:`, `rl:`, `up:`, `fp:` (a repository bundle is public upstream content). The thread keys of §10, which lacked the segment, gain it. (PENDING B5)

Replaced text: §10 of this ADR.

### A22. Rate-limit counter keys

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §4
- **Summary:** Rate-limit counters are `rl:{class}:{key}`, the class being a rate-limit class of 0024 §5 and the key the actor key or, for anonymous requests, the IP; 0024 §5 is corrected to the same form. (PENDING C4)

Replaced text: §4 of this ADR.

### A23. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** 0050 §14
- **Change:** relocates §1–§10

### A24. Source form in the cache, the index and the fallback

- **Date:** 2026-10-09
- **Source:** 0082 §1, §2, §3, §5.1, §6
- **Change:** amends §3, §7, §8, §10; extends §4
- **Summary:** L1 holds the internal form once per entity version, and the layer that rewrites entity IDs to the consumer's preferred form runs after the cache, on a hit on the way out, so there is one `e:` entry per version and `ETag` is the version of the stored form; `resolved_version` is new on every composition, including one caused by another member of the entity's cluster, and never by a referrer's change. A tenant document replaces the shared one in the index: the overlay projection sets `tenant_overlaid` on the shared document and a tenant's query excludes shared documents that name it, so no post-query dedup is needed. Chapter text: 03 §9.3, §11.1, §11.3, §11.4, §11.6, §12.1, §12.5. (REVIEW G3, G4, G5, G2, G7)

Replaced text: 03 §9.3, §11.1, §11.3, §11.4, §11.6, §12.1, §12.5.

### A25. The composer writes L1; the indexer is a tier-3 consumer; the cache epoch

- **Date:** 2026-10-09
- **Source:** 0083 §1, §2, §3, §6
- **Change:** amends §3, §7; extends §4, §10
- **Summary:** Read-your-writes follows from the write path's after-commit slot: the composer composes the written entity after commit, takes its new version, writes the resolved JSON into L1 and returns the body, so L1 is written by the composer after commit and never by the write path before it, and a rolled-back transaction leaves nothing in the cache. Every L1 key begins with an instance-wide cache epoch, `c{epoch}:`, a counter in `ops` that every projection rebuild bumps, distinct from a tenant's visibility epoch and the stream epochs; §12.1 writes the keys without it. The `read` rate class is counted for authenticated principals only once a per-process count passes a fraction of the limit, and never for a response served from L0 or L1. Chapter text: 03 §9.3, §11.3. (REVIEW G9, G10, G13, G15)

Replaced text: 03 §9.3, §11.3.

### A26. The public `{vis}` form carries the tenant epoch; enclosures, not groups, are stored

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §4, §5, §7, §10
- **Summary:** The `{vis}` segment's empty-set form is `-{epoch}` on a tenant key, so that a public entry cached before a namespace restriction lapses with it, and plain `-` on a shared key, which carries no tenant epoch because a shared row is under no tenant's namespace. Only a `namespace` or `tenant` `read` ACL bumps the tenant's visibility epoch; a `set` change bumps the generation of the members it touched. A stored search document or feed row holds `read_enclosures`, a reference to the enclosures its target is under, never a group list, and the groups those enclosures require are evaluated at serve time from L0, so a change on an enclosure rewrites no document; the per-tenant indexes are where `read_enclosures` is non-empty. The required privacy test is that an entry cached under the empty set before a namespace restriction is not served after it. (REVIEW G38)

Replaced text: 03 §9.4, §9.6, §11.5, §12.1.

### A27. Cache tags are exact and bounded

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §5, §6, §10
- **Summary:** A composed page carries exact `Cache-Tag`s for its subjects and its render-manifest pages only; label dependencies are not tagged, since a large item draws labels from thousands of entities, and label staleness is accepted up to `s-maxage`. An anonymous API response is tagged for the entities and pages it is about, not every one it drew on. The About panel of an entity page has its own response and `max-age`, since its counters change without the entity's version changing; the page budget of four API calls to first byte, the bulk-labels route and the `GET /entity/{id}/page` bundle that meet it are in chapters 18 and 20. (REVIEW G41)

Replaced text: 03 §9.6, §10.3, §12.4.

### A28. Address keys, the `createaccount` class and challenge keys

- **Date:** 2026-10-17
- **Source:** 0087 §3, §4
- **Change:** extends §4
- **Summary:** Rate-limit counters key an anonymous request on its address key, the IPv4 address or the IPv6 /64 (0024 A19); the `createaccount` class joins the counters; `ch:{tenant}:{mac}` marks a challenge spent until it expires, and `ch:{tenant}:bad:{key}` and `ch:{tenant}:badu:{actor}` count failed `password` logins for the `badlogin` trigger. The chapter text is 03 §12.1 and §12.3.

Replaced text: 03 §12.1, §12.3.

### A29. Central sessions and login codes in L1

- **Date:** 2026-10-18
- **Source:** 0089 §5, §9
- **Change:** amends §4
- **Summary:** `s:{session}` also records the login a session holds and, for a tenant session, the central session it came from. `sc:{central session}` lists the tenant sessions a central session opened, so that logging out everywhere ends them; `authr:{handle hash}` holds a tenant's request at the auth host for ten minutes, and `authc:{code hash}` a login code for 60 seconds. The chapter text is 03 §12.1.

Replaced text: 03 §12.1.

### A30. The Postgres fallback as built: ranking, the page text index, and the suggest response

- **Date:** 2026-11-21
- **Source:** Direct: Claude, while building search on Postgres (SCA-274), 2026-11-21; unconfirmed
- **Change:** amends §8
- **Summary:** The fallback's open points are settled as built. (1) Entity results are ordered by an exact match, then by a count of references over `view.entity_ref`, then by the shorter text, among the 2,000 best-matching terms; there is no `ref_count` column to order by yet. (2) A result is its cluster's canonical member where the caller may read it, else the member that matched. (3) The fallbacks of a requested language are the shorter prefixes of its code, then `mul` and `en`. (4) `view.page_text` holds the title's words at weight A and the first 256 KiB of the latest text at weight D, in the `simple` configuration, written by the page projection. (5) `list=search` matches a page by its words or by a prefix of its title, leaves out redirects, and reads `srnamespace=*` as the document namespaces, listing entities only for an entity namespace it names; its snippet is cut from the stored text with the commonest markup removed, not from a rendering. (6) `list=prefixsearch` stays a search of document pages: the table's "`list=prefixsearch` for entities" is dropped in favour of 0010 A51, by which entities are found by their labels. (7) `GET /suggest` takes `q`, a `language` chain as `GET /labels` does, and `limit`, and answers `{q, language, goto?, groups}`, groups by kind in a fixed order, a foreign hit naming its `provider` rather than providers being groups of their own. (8) Special:Search's Pages group reads `list=search`, and the header's box reads `/suggest`. The chapter text is 03 §11.6 and 19 §2.1.

Replaced text: 03 §11.6; 19 §2.1.

