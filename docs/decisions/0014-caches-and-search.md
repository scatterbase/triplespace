# 0014. Cache layers and search

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-09 (A27)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md)
- **Uses:** [0000](0000-init.md), [0003](0003-statement-ui.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0009](0009-keyed-entity-types-and-domain.md), [0011](0011-logs.md), [0013](0013-postgres-storage.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [22](../architecture/22-crates-and-stack.md)

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

*Changed by A3, A13.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.1, §13.*

### 2. Layers

*Changed by A14, A15, A19.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.2, §12.2.*

### 3. Versions

*Changed by A7, A24, A25.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.3.*

### 4. What L1 holds

*Changed by A7, A11, A12, A13, A22, A24, A25, A26.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.4, §9.7, §12.1, §12.3.*

### 5. Erasure and hiding reach every layer

*Changed by A3, A7, A26, A27.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.6, §12.4.*

### 6. HTTP caching

*Changed by A5, A15, A27.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §10.1, §10.3.*

### 7. Search on OpenSearch

*Changed by A2, A4, A5, A6, A7, A10, A11, A12, A13, A17, A24, A25, A26.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §11.1, §11.2, §11.3, §11.5, §12.5.*

### 8. Postgres fallback

*Changed by A24.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §5, §11.6.*

### 9. Crates

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

### 10. Caches, indexes and stores added by later ADRs

*Changed by A3, A5, A7, A8, A16, A18, A20, A21, A24, A25, A26, A27.*

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
- **Q5.** ~~**Session store.** Valkey is chosen for sessions; whether they should fall back to Postgres when the shared cache is absent, or a small instance should require Valkey for login.~~ *Settled by A14: sessions fall back to `private.session` in Postgres when the shared cache is absent; a small instance needs nothing but Postgres to log in.*
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

### A13. Caches and the index keyed by visibility

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §7, §8
- **Change:** amends §1, §4, §7
- **Summary:** Shared caches hold the form for a visibility set, keyed by it, so that a private wiki caches as a public one does; the public form is the empty set's entry and the only one L2 may hold. The L1 keys gain a `{vis}` segment with the tenant's visibility epoch. The search index holds content under confidential restrictions with a `read_groups` field and a `terms_set` filter per query; moderation-hidden content stays out.

Replaced text (§1):

> 3. **Shared caches hold only the public form.** An entry is the response as an anonymous reader would see it: hidden fields removed, erased bodies absent. Administrators' views, which include hidden fields ([0012](0012-api-requirements.md) §8), bypass every shared layer. Nothing from a `private` graph is ever cached outside the holder's session.

Replaced text (§4):

> `{gen}` is the entity's or page's `generation` ([0013](0013-postgres-storage.md) §5), which changes only on erasure or hiding (§5). `{ver}` is the version of §3.

Replaced text (§7):

> - The index holds only public data: no hidden names, no erased bodies, nothing from an internal-only or private graph. Redaction is applied when the document is built, and the erasure path of §5 deletes documents.

### A14. Sessions fall back to Postgres

- **Date:** 2026-10-03
- **Source:** Direct: James, decision of 2026-10-03 (`triplespace-accounts`, `triplespace-api-action`, `triplespace-server`, `scatter-adapter-internetdomains`)
- **Change:** extends §2
- **Summary:** An instance without a shared cache keeps sessions in `private.session` (0013 §5.6, as amended) rather than requiring Valkey for login; the cookie is `triplespace_session` (`HttpOnly`, `SameSite=Lax`, `Secure` outside development mode), an anonymous session lives an hour and a logged-in one thirty days from its last use, and expired rows are swept on login. When Valkey is present it may hold the same rows as `s:{session}`; the shape is the same.

### A15. The web tier's cache and composed pages

- **Date:** 2026-10-03
- **Source:** [0057](0057-web-tier.md) §5, §6
- **Change:** extends §2, §6
- **Summary:** The web tier may cache public API responses to its credential-free requests, in memory or in a named Valkey. It cannot be purged, and it sits beneath a CDN that can, so it revalidates every use, keeping without revalidation only responses whose `max-age` the API sets because no purge, erasure or moderation can change them, for at most 60 seconds; it never honours `s-maxage`. A page it composes is public only if all its inputs were, with an `ETag` and `Cache-Tag` derived from theirs.

### A16. The fragment cache

- **Date:** 2026-10-04
- **Source:** [0058](0058-packed-record-storage.md) §11
- **Change:** extends §10
- **Summary:** Packed storage adds an L0 fragment cache: immutable fragments by domain and ref, and the refs known to be shared in the domain's current epoch. Erasure gains a seventh step, the fragment sweep and the cache drop that follows it.

### A17. Lexemes, schemas and captions in search

- **Date:** 2026-10-05
- **Source:** [0066](0066-lexemes.md) §5; [0064](0064-entityschema-and-validation.md) §7; [0065](0065-mediainfo-captions-and-commons.md) §1
- **Change:** extends §7
- **Summary:** Lexeme documents and the three lexeme search types; schema terms and text; captions as File page terms.

### A18. Scope and sprint listing keys

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §10
- **Summary:** The L1 key list of §10 gains the `sc:` rows of [0060](0060-scopes.md) §10 and the `sp:` rows of [0061](0061-sprints-and-tasks.md) §11, the scope and sprint listings. (PENDING A11)

### A19. Where session state lives

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §2
- **Summary:** Session state lives in Valkey when a shared cache is configured and in `private` otherwise ([0025](0025-oauth-server.md) §8; A14), so §2 no longer states Valkey as the one home of sessions; [0013](0013-postgres-storage.md) §4's "sessions' secrets" means that fallback. (PENDING A12)

Replaced text (§2):

> L1 is the layer MediaWiki fills with memcached. Valkey is chosen over memcached for one reason: keys can be enumerated by prefix, so an erasure can delete every entry for an entity rather than rely on generation keys alone (§5). Sessions live in Valkey; OAuth tokens and bindings do not, they stay in Postgres `private` ([0013](0013-postgres-storage.md) §4).

### A20. Listing keys carry a listing version

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §10
- **Summary:** Listing keys (`tp:`, `sc:`, `sp:`) carry a **listing version**, the maximum activity ID over the listing's members, computed in Postgres at read time, in place of tag purges on ordinary writes and in place of the talk page's latest revision ID; §1's principles 1 and 4 hold unchanged. Tag purges remain for erasure and hiding. The same holds in [0049](0049-boards.md) §12, [0069](0069-synchronized-talk-pages.md) §10, [0060](0060-scopes.md) §10 and [0061](0061-sprints-and-tasks.md) §11. (PENDING B4)

Replaced text (§10):

> | `tp:{pageid}:{gen}:{revid}` | A talk page's thread listing, keyed likewise by the talk page ID | 0019 §11 |

### A21. Every viewer-dependent key carries `{vis}`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §10
- **Summary:** Rule: every key whose value depends on what the viewer may read carries `{vis}` ([0056](0056-security-model.md) §7), and §10's key list marks each key. Classification, to confirm: carry `{vis}`: `e:`, `t:`, `prov:`, `p:`, `d:`, `sug:`, `css:`, `post:`, `th:`, `tp:`, `fi:`, `rf:`, `ld:`, `sc:`, `sp:`; do not: `s:`, `rl:`, `up:`, `fp:` (a repository bundle is public upstream content). The thread keys of §10, which lacked the segment, gain it. (PENDING B5)

Replaced text (§10):

> | `post:{revid}:{gen}:{offset}` | A rendered post; `{offset}` is the record holding its current text | [0019](0019-discussions.md) §11 |
> | `th:{pageid}:{gen}:{revid}` | A thread, keyed by its page ID and latest revision ID | 0019 §11 |
> | `tp:{pageid}:{gen}:{revid}` | A talk page's thread listing, keyed likewise by the talk page ID | 0019 §11 |

### A22. Rate-limit counter keys

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §4
- **Summary:** Rate-limit counters are `rl:{class}:{key}`, the class being a rate-limit class of [0024](0024-subsidiary-accounts.md) §5 and the key the actor key or, for anonymous requests, the IP; 0024 §5 is corrected to the same form. (PENDING C4)

Replaced text (§4):

> | `rl:{scope}:{key}` | Rate-limit counters for upstream fetches and writes | The limit window |

### A23. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A24. Source form in the cache, the index and the fallback

- **Date:** 2026-10-09
- **Source:** [0082](0082-source-form-and-the-shared-view.md) §1, §2, §3, §5.1, §6
- **Change:** amends §3, §7, §8, §10; extends §4
- **Summary:** L1 holds the internal form once per entity version, and the layer that rewrites entity IDs to the consumer's preferred form runs after the cache, on a hit on the way out, so there is one `e:` entry per version and `ETag` is the version of the stored form; `resolved_version` is new on every composition, including one caused by another member of the entity's cluster, and never by a referrer's change. The `entities` index holds every entity under the ID its source wrote, every document carries `cluster_id`, `statement_keywords` is in source form with a `haswbstatement:` constant expanded to the member set at query time, `incoming_links` is `view.entity.ref_count` summed over the cluster, and results collapse by cluster, one per cluster, the member the consumer's preference selects; a search for any member's ID is a `/resolve`. A tenant document replaces the shared one in the index: the overlay projection sets `tenant_overlaid` on the shared document and a tenant's query excludes shared documents that name it, so no post-query dedup is needed. The Postgres fallback is the small profile's search: `term_prefix` is partial over the tenant's rows, ordered by `ref_count` and collapsed by `cluster_id`, and an instance holding a full mirror has no prefix index over mirrored terms at all; the `t:` label cache serves every language `view.term` does not hold. (REVIEW G3, G4, G5, G2, G7)

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §9.3, as it stood):

> | Resolved entity | `resolved_version` | Bumped by the resolution projection on every re-resolution, including ones caused by another entity's cluster change (§4.2) |

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §11.1, as it stood):

> | `entities` | Canonical entity in the resolved view: local, foreign and keyed, of every type | `id`, `type`, `provider`, `namespace` (MediaWiki number); `labels.{lang}` with `.prefix` and `.near_match` subfields; `labels_all`; `descriptions.{lang}`; `aliases.{lang}`; `key`, `key_ulabel` and `key_parents` for keyed types; `statement_keywords`; `sitelink_count`, `statement_count`, `incoming_links`; `resolved_version` |
>
> - **`statement_keywords`** holds `P31=Q5`-style tokens for item-valued and external-id statements in the resolved view, using canonical IDs, so `haswbstatement:` works.
> - **`incoming_links`** is the count of distinct referrers in `view.entity_ref`, resolved to canonical IDs. `sitelink_count` and `statement_count` are read from the resolved JSON.
> - **Non-canonical cluster members have no document.** Their terms are already merged into the canonical entity's resolved view. A search for a non-canonical ID is a `/resolve`, not a search.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §11.3, as it stood):

> - An erased or tombstoned entity, a deleted page, or an entity that stops being canonical is deleted from the index with the same versioning.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §11.4, as it stood):

> **Indexes multiply by provider and tenant** ([0018](../decisions/0018-tenants.md) §6): one shared `entities` index per provider, holding that provider's canonical entities as the shared view has them, and one index per tenant holding its local entities and its overlay documents. `/suggest` and `wbsearchentities` run one `msearch` across the tenant's own index and the indexes of its opted-in providers; where a tenant document and a shared document share an ID, the tenant's wins.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §11.6, as it stood):

> | `/suggest`, `wbsearchentities`, `list=prefixsearch` for entities | `view.term` with the `term_prefix` index (§4.3): `lower(text) LIKE lower($q) || '%'` in the requested language and its fallbacks, ordered by a count from `entity_ref` |
>
> The fallback has no `haswbstatement:`, no ranking beyond incoming links, and no analysis. It is meant for instances small enough that a prefix scan is fast, which the profile table of [0013](../decisions/0013-postgres-storage.md) §11 (in [22](../architecture/22-crates-and-stack.md)) defines.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §12.1, as it stood):

> | `e:{id}:{gen}:{ver}` | Resolved canonical JSON, compressed | 24 h | [0014](../decisions/0014-caches-and-search.md) §4 | Yes |
> | `t:{lang}:{id}:{gen}:{ver}` | Label and description in one language | 24 h | 0014 §4 | Yes |

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §12.5, as it stood):

> | `entities`, one per provider | Instance | That provider's canonical entities as the shared view has them; public form only |

### A25. The composer writes L1; the indexer is a tier-3 consumer; the cache epoch

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §1, §2, §3, §6
- **Change:** amends §3, §7; extends §4, §10
- **Summary:** Read-your-writes follows from the write path's after-commit slot: the composer composes the written entity after commit, takes its new version, writes the resolved JSON into L1 and returns the body, so L1 is written by the composer after commit and never by the write path before it, and a rolled-back transaction leaves nothing in the cache. The search indexer is a tier-3 consumer with its own queue, rate and lag, off during bootstrap and bulk modes: it consumes the composition event `(id, tenant, resolved_version)`, reads the composed JSON and `ref_count` from `view`, and its lag is reported from `ops.projection_work`; bootstrap indexes from `view` after the set-based composition. Every L1 key begins with an instance-wide cache epoch, `c{epoch}:`, a counter in `ops` that every projection rebuild bumps, distinct from a tenant's visibility epoch and the stream epochs; §12.1 writes the keys without it. The `read` rate class is counted for authenticated principals only once a per-process count passes a fraction of the limit, and never for a response served from L0 or L1. (REVIEW G9, G10, G13, G15)

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §9.3, as it stood):

> **Read-your-writes** follows from §6.2. The write transaction commits the new version; the response reports it; the write path also writes the new resolved JSON into L1 so the editor's next read hits. Replica routing by LSN covers the Postgres side.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §11.3, as it stood):

> The search indexer is the last consumer in §6.1's order:
>
> - After the resolution projection writes an entity, it enqueues `(id, resolved_version)`. The indexer reads the resolved JSON and `entity_ref` count from `view`, builds the document, and sends it in bulk with `version_type: external` and `version: resolved_version`. Out-of-order deliveries are rejected by the index, so retries and parallel workers are safe.
> - **Bootstrap** indexes from `view` in one pass after the resolution projection, with the refresh interval raised and replicas set to zero for the duration, as CirrusSearch's reindex does.
>
> **Consistency.** The index is eventually consistent with `view`, by the projection's lag. A suggestion may name an entity whose page has moved on; the page is served from `view`, and the difference is the lag reported by `ops.projection_state`.

### A26. The public `{vis}` form carries the tenant epoch; enclosures, not groups, are stored

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §4, §5, §7, §10
- **Summary:** The `{vis}` segment's empty-set form is `-{epoch}` on a tenant key, so that a public entry cached before a namespace restriction lapses with it, and plain `-` on a shared key, which carries no tenant epoch because a shared row is under no tenant's namespace. Only a `namespace` or `tenant` `read` ACL bumps the tenant's visibility epoch; a `set` change bumps the generation of the members it touched. A stored search document or feed row holds `read_enclosures`, a reference to the enclosures its target is under, never a group list, and the groups those enclosures require are evaluated at serve time from L0, so a change on an enclosure rewrites no document; the per-tenant indexes are where `read_enclosures` is non-empty. The required privacy test is that an entry cached under the empty set before a namespace restriction is not served after it. (REVIEW G38)

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §9.4, as it stood):

> **Visibility.** Every key whose value depends on what the viewer may read also carries a **`{vis}`** segment, written after `{gen}`: a short hash of the sorted group names of the visibility set together with the tenant's visibility epoch, `-` for the empty set.
>
> A `read` ACL on a namespace, set or tenant bumps the epoch and every entry under it lapses at once (§9.6).

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §9.6, as it stood):

> On a namespace, set or tenant, it bumps the tenant's **visibility epoch**, a counter in `view.tenant` that is part of every `{vis}` hash for that tenant, so that every entry under the enclosing target lapses at once. The trade is coarse purges for correctness, and these ACLs change rarely.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §11.5, as it stood):

> **Documents carry `read_groups`.** Each document in a tenant's index gains `read_groups`: the sorted group names of its target's visibility set, empty for a public target. A query adds one filter: `read_groups` is empty, or every value in it is among the principal's groups, which OpenSearch expresses as a `terms_set` query whose `minimum_should_match` is the field's length.
>
> The per-provider shared indexes hold public form only, because they are built from public data; the per-tenant indexes are where `read_groups` is non-empty.
>
> **Re-indexing.** A `read` ACL change re-indexes the affected documents on the trigger of §9.6, with the same versioning §11.3 uses, and a document whose target becomes moderation-hidden is deleted.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §12.1, as it stood):

> | `e:`, `t:`, `prov:`, `p:`, `d:`, `sug:`, `css:` | Gain a `{vis}` segment after `{gen}` (§9.4) | | [0056](../decisions/0056-security-model.md) §7 | Yes (§4) |

### A27. Cache tags are exact and bounded

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §5, §6, §10
- **Summary:** A composed page carries exact `Cache-Tag`s for its subjects and its render-manifest pages only; label dependencies are not tagged, since a large item draws labels from thousands of entities, and label staleness is accepted up to `s-maxage`. An anonymous API response is tagged for the entities and pages it is about, not every one it drew on. The About panel of an entity page has its own response and `max-age`, since its counters change without the entity's version changing; the page budget of four API calls to first byte, the bulk-labels route and the `GET /entity/{id}/page` bundle that meet it are in chapters 18 and 20. (REVIEW G41)

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §9.6, as it stood):

> 3. **purges L2 by tag.** Every anonymous response carries a cache tag for each entity and page it drew on (`Cache-Tag: entity:Q42, page:17`); the purge names the tags.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §10.3, as it stood):

> - **Its `Cache-Tag`** is the union of theirs, so the purges of §9.6 reach it.

Replaced text ([03](../architecture/03-storage-caches-and-search.md) §12.4, as it stood):

> Every anonymous response carries `Cache-Tag: entity:{id}, page:{page id}` for each entity and page it drew on (§9.6); every rendered page carries a tag for each page and entity in its manifest ([0042](../decisions/0042-template-expansion-and-parsoid.md) §10); every media response carries `blob:{scope}:{h}` and `file:{tenant}:{page id}` ([0039](../decisions/0039-files-and-media.md) §20); a page the web tier composes carries the union of its components' tags (§10.3).
