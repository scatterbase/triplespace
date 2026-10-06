# 0053. Mirrored pages

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-06 (A1)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0022](0022-federation.md), [0033](0033-backend-stack.md), [0039](0039-files-and-media.md), [0042](0042-template-expansion-and-parsoid.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0040](0040-instance-prerogatives.md), [0047](0047-special-pages.md), [0051](0051-page-redirects.md), [0052](0052-page-repositories-and-title-inheritance.md), [0054](0054-forking-a-mirrored-page.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

[0052](0052-page-repositories-and-title-inheritance.md) lets a title resolve to a page that belongs to a repository. This ADR says what that page is made of, where it comes from, how long it is kept and how it is shown.

The first stage of James's plan is simple: **a mirrored page is cached HTML fetched from the original wiki, latest revision only.** So long as a page is only being read, nothing needs its history, its talk page or the thousands of templates and modules behind it. Rendering a Wikipedia article from its wikitext would need all of those: the article's own text, every template it uses, every module those templates invoke, the TemplateStyles sheets, and Wikipedia's site styles, all expanded and rendered here. [0042](0042-template-expansion-and-parsoid.md) makes that possible for a page the tenant chooses to own, and [0054](0054-forking-a-mirrored-page.md) is where it happens. For a page that is merely read, Wikipedia has already rendered it, as Parsoid HTML, with a stable contract: `data-mw` on every transclusion, `typeof` on every construct, `./Title` links, `rel="mw:PageProp/Category"` on categories, `<section data-mw-section-id>` around sections. The same HTML is what Wikipedia's own mobile apps and reader tools consume.

Two things are already decided for files and templates, and are reused here: a `mediawiki` file repository fetches through the server and caches, or mirrors into an instance partition ([0039](0039-files-and-media.md) §11), and a template repository's fetched source is cached, rendered as it is now, and recorded in the render manifest with an expiry ([0042](0042-template-expansion-and-parsoid.md) §10–11). Both are "proxy or mirror, never hotlink by default, record what you read".

### Direction

James's direction, from the design discussion of 2026-10-01:

- **In the first stage, the mirrored wiki is stored as cached HTML fetched from the original wiki. Latest revisions only.** So long as the page is simply being mirrored, there is no need to hydrate a full page history or discussion history.

## Decision

### 1. The page bundle

*Changed by A1.*

**What the instance holds for a foreign page is a bundle,** one per page per repository, for the latest upstream revision only:

| Part | Holds |
|---|---|
| **Identity** | The repository, the upstream namespace and title, the upstream page ID, the upstream revision ID and its timestamp, the content model, and the licence from the repository's entry |
| **Wikitext** | The revision's source, for the Edit tab, `prop=revisions&rvprop=content`, transclusion ([0042](0042-template-expansion-and-parsoid.md) §11) and forking ([0054](0054-forking-a-mirrored-page.md) §3) |
| **HTML** | The repository's rendering of the revision, rewritten (§2) and sanitized, ready to place in the frame |
| **Metadata** | Categories, templates and modules used, images used, interlanguage links, page properties (display title, short description, disambiguation, `noindex`), the redirect target if the page is one, and the `data-mw`-bearing section map; for a followed talk page, the thread items of `action=discussiontoolspageinfo` ([0069](0069-synchronized-talk-pages.md) §2) |
| **Freshness** | When it was retrieved, the upstream `ETag` or render ID, and `expires_at` |

**Three sources, chosen per repository with `source`:**

| Source | Reads | For |
|---|---|---|
| `rest` *(default)* | MediaWiki core's REST API, `GET {rest base}/v1/page/{title}/with_html`, which returns the source, the Parsoid HTML, the latest revision and the licence in one response; then one `action=query` for the metadata (`prop=categories\|templates\|images\|langlinks\|pageprops\|info`) | Any MediaWiki 1.35 or later with Parsoid, every Wikimedia wiki, and a Triplespace tenant elsewhere, which serves the same route ([0012](0012-api-requirements.md) §5) |
| `enterprise` | The Wikimedia Enterprise On-demand API, `GET /v2/articles/{name}` filtered to the project, which returns the Parsoid HTML, the wikitext, the version, the licence, the categories, templates, redirects and the article's main Wikidata entity in one response | Wikimedia projects, where an instance holds Enterprise credentials; its Realtime stream is the `events` source of §6 |
| `action` | `action=parse` with `prop=text\|wikitext\|categories\|templates\|images\|langlinks\|properties\|revid`, the legacy parser's HTML | A MediaWiki without Parsoid's REST routes |

Credentials for `enterprise` are deployment configuration ([0033](0033-backend-stack.md) §12), never a log record. Every source goes through the live upstream client ([0012](0012-api-requirements.md) §6, `triplespace-upstream`): a proper User-Agent, `maxlag`, a per-repository concurrency cap (`repo.concurrency`, default 4) and a per-repository fetch budget (`repo.fetches_per_minute`, default 600), so a crawler reading an instance cannot turn it into a crawler of Wikipedia. **A bundle is never logged in `proxy` mode** (§4) and is a record in `mirror` mode (§5).

**Latest only.** A bundle holds one revision. A reader who wants an older one, or the history, is sent upstream, and the History tab fetches the upstream history live as [0010](0010-site-ui.md) §5.5 does for entities, under the `upstream` rate class ([0024](0024-subsidiary-accounts.md) §5), never logging it. History arrives here only with a fork.

### 2. Rewriting the HTML (extends 0008 §8; uses 0039 §11)

**The repository's HTML is rewritten once, when the bundle is made, into HTML that belongs to this tenant.** The rewrite is a single pass over the DOM:

| What | Becomes |
|---|---|
| **Sanitization** | The same sanitizer and allow-list that Parsoid's own output passes through here ([0042](0042-template-expansion-and-parsoid.md) §8.4): `typeof`, `about`, `data-mw`, `rel`, `property` and `id` are kept; `<script>`, event attributes and anything else outside the allow-list are dropped; `style` attributes are sanitized as [0008](0008-namespaces-and-document-pages.md) §8 sanitizes them |
| **`<head>`, `<base>`** | Dropped. The HTML is a body fragment inside the tenant's frame ([0052](0052-page-repositories-and-title-inheritance.md) §5) |
| **Wiki links** (`rel="mw:WikiLink"`, `href="./Title"`; or the article path's `/wiki/Title` from the legacy parser) | A link to the **local** URL of the same title when its namespace is one the repository serves here, so the link resolves through the stack ([0052](0052-page-repositories-and-title-inheritance.md) §3–4) and the reader keeps reading; otherwise the repository's own URL with `class="extiw"`, a link that leaves the wiki. A link whose title the title index (§3) lacks gets `class="new"`, a red link, as upstream drew it; a link to a redirect keeps `mw-redirect` ([0051](0051-page-redirects.md) §2) |
| **Interwiki and external links** (`mw:WikiLink/Interwiki`, `mw:ExtLink`) | Kept as they are, with `rel="nofollow"` as upstream has it |
| **Images** (`<img>`, `srcset`, `<figure>`, `mw:File`) | Served by the tenant's **file policy**: the `File:` link goes to the local file title, and the bytes follow the first file repository in `files.repos` that holds the file, in its mode ([0039](0039-files-and-media.md) §11): `proxy` fetches and serves them from the media base, `link` leaves the repository's URLs, `mirror` serves the copy. An `<img>` whose source is not a file in any repository, a rendered formula from a math service for instance, is fetched through the repository's `media_hosts` allow-list in `proxy` mode and dropped otherwise, since external image URLs are never embedded ([0008](0008-namespaces-and-document-pages.md) §8) |
| **Categories** (`<link rel="mw:PageProp/Category">`) | Removed from the body and recorded in the bundle's metadata; the frame lists them at the foot as [0052](0052-page-repositories-and-title-inheritance.md) §7 says |
| **TemplateStyles** (`<style data-mw-deduplicate>`) | Passed through the CSS sanitizer of [0055](0055-templatestyles-templatedata-and-page-properties.md) §2 and kept, deduplicated by their key as Parsoid deduplicates them |
| **Sections** (`<section data-mw-section-id>`) | Kept. The frame draws its own section edit links from them, each opening the fork editor at that section ([0054](0054-forking-a-mirrored-page.md) §2) |
| **`data-mw`, `typeof`, `about`** | Kept, so that a fork can later be edited over its HTML ([0042](0042-template-expansion-and-parsoid.md) Q6) and so that the templates a page uses can be read from it without a second parse |
| **Indicators, `mw:PageProp/toc`, `displaytitle`** | Recorded as metadata and drawn by the frame as the tenant draws its own |

**The rewritten HTML depends on the repository's configuration and the title index, not on the stack.** A link is `/wiki/Title`; what the title shows is decided when it is followed. So a fork made after a page was cached changes nothing in the cached HTML, which is correct, and a title created locally after a page was cached turns a red link blue only when the cache entry is refreshed (§4), which is the one staleness the design accepts.

**The legacy parser's HTML** (`source = action`) is rewritten by the same rules, matching links by the article path and `title` attributes in place of `rel` and `./`; it carries no `data-mw`, no section IDs and no TemplateStyles keys, so section editing and fork-over-HTML are unavailable for pages from such a repository.

### 3. The title index (extends 0052 §8)

**A repository's titles are mirrored, so the stack is a local read.** For a repository with `titles = index`, an instance job, `pages index {repo}`, fills `view.foreign_title` ([0052](0052-page-repositories-and-title-inheritance.md) §8) for the served namespaces:

| Step | Source |
|---|---|
| **Bootstrap** | The repository's `page` and `redirect` SQL dumps, or its `all-titles` dump with `list=allpages&apfilterredir=redirects` for the redirect targets; for a Wikimedia wiki, the Enterprise snapshot's metadata where credentials exist. Each row records the upstream page ID, whether the page is a redirect and its target, and the latest revision ID |
| **Keeping current** | The repository's events (§6): `page-create`, `page-delete`, `page-undelete`, `page-move` and a revision that changes whether the page is a redirect, for the served namespaces. Without events, `list=recentchanges` is paged from the last cursor on a schedule, as [0011](0011-logs.md) §4 fills gaps in a log mirror |
| **Reconciliation** | A fresh dump replaces the index for a namespace in one transaction, and reports how many titles the events had missed |

`Special:Providers` ([0022](0022-federation.md) §11, [0047](0047-special-pages.md) §3) lists each page repository with the date of its index and its event lag, beside the data providers' sync lag.

A `tenant` repository has no index: the source tenant's `view.page` is read directly ([0052](0052-page-repositories-and-title-inheritance.md) §7).

### 4. Proxy mode: fetched on demand, cached, never logged (extends 0014 §4)

**With `mode = proxy`, a bundle is made the first time a page is read or transcluded** and kept in the caches:

| Layer | Key | Holds | Lifetime |
|---|---|---|---|
| L1 ([0014](0014-caches-and-search.md) §4) | `fp:{repo}:{ns}:{title}:{revid}` | The rewritten HTML and the metadata, compressed | `cache_ttl` ([0052](0052-page-repositories-and-title-inheritance.md) §1, default one hour), or until an event purges it |
| L2, the blob cache of [0039](0039-files-and-media.md) §11 | `cache/{repo}/pages/{hash}` | The whole bundle, wikitext included | `repo.cache_ttl` with a size cap and least-recently-used eviction, shared with the repository's file cache |

**A rendered foreign page is the bundle's HTML inside the tenant's frame.** The frame is drawn per request as it is for a local page; the HTML inside it is the cached fragment, so a page is fetched and rewritten once per `cache_ttl` however many readers it has. The response carries `Cache-Tag: repo:{repo}:{upstream page ID}` so that a purge reaches the HTTP layer ([0014](0014-caches-and-search.md) §5–6).

**Revalidation is cheap.** When an entry passes `expires_at` the server checks the repository's latest revision ID (`prop=info`, the REST route's `ETag`, or the Enterprise version) before refetching, so an unchanged page costs one small request per `cache_ttl`.

**Readers' addresses never reach the repository.** The server fetches, as it does for a proxied file. There is no `link` mode for pages: HTML is always rewritten here, so it is always fetched here.

**Nothing in `proxy` mode is a record.** The bundle is not in the log, has no revision ID of the tenant's, no offset and no proof; it is what the repository said when it was asked, as a proxied file is. This is what "cached HTML" means, and it is what most tenants will run.

### 5. Mirror mode: the `pages/{repo}` partition (extends 0015 §3 and §5; uses 0002 §2, §5, 0006 §4)

*Changed by A1.*

**With `mode = mirror`, the bundles of a chosen set of pages are records in an instance partition,** so that the pages stay readable when the repository is down, are verifiable, and can be exported. The partition is registered in `graphs.toml` beside `files/{repo}`:

| Graph | Illustrative IRI | Kind | Scope | Written by | History | Integrity | Export |
|---|---|---|---|---|---|---|---|
| **Mirrored pages**, one per repository | `{farm base}/instance/graph/pages/enwiki` | Source | Instance | The repository's sync job | `latest` (an instance may set `full`) | `hashed` | Public |

**The record** has the payload type **`scatter:v0/mirrored-page`**, keyed by the **upstream page ID**, an identifier and never content ([0006](0006-log-integrity-and-erasure.md) §3). Header field 9 carries the provider-ranged page ID and field 7 the ranged revision ID, which are the numbers [0052](0052-page-repositories-and-title-inheritance.md) §6 derives, so a mirrored page's API identity is the same in both modes. It declares five parts ([0015](0015-record-format-and-partition-registry.md) §1):

| # | Part | Holds | RevisionDelete bit |
|---|---|---|---|
| 0 | Content | The bundle's identity and metadata (§1): title, revision, timestamp, model, categories, templates, images, interlanguage links, page properties, redirect target | — |
| 1 | Comment | The upstream edit summary | comment |
| 2 | Attestation | The upstream actor under the repository's issuer ([0007](0007-actor-identity.md) §1, §5), and the sync job | user |
| 3 | Wikitext | The source | text |
| 4 | HTML | The rewritten HTML, compressed | text |

Two parts for the two forms, so that a takedown can erase the rendering and keep the source, or the reverse, and so that `full` history need not keep every rendering.

**A followed talk page** ([0069](0069-synchronized-talk-pages.md) §1–2) is mirrored here whatever the repository's `mode`. Its content part gains `threads`: for each section, its number `n` in the repository, the hash of its DiscussionTools name, its section index, and its comments' name hashes and parents, from which [0069](0069-synchronized-talk-pages.md) §2 derives foreign threads with stable IDs; its comment authors go in the attestation part, and a `put` lists the threads it changed. Archive subpages are mirrored when a thread is found to have moved to one.

**Operations** follow a mirror graph's ([0002](0002-source-graphs-and-mass-ingest.md) §8.2): **`put`** writes the bundle and is skipped when the upstream revision is not newer; a **`put`** with a new title records an upstream move, since MediaWiki keeps the page ID across moves; **`tombstone`** records an upstream deletion, and compaction erases the page. Upstream hiding of a revision's text, summary or user is followed with an `erase` of the part, reason class `upstream` ([0011](0011-logs.md) §5); a suppressed page is tombstoned.

**Which pages are mirrored** is the repository's `mirror.set`:

| Set | Mirrors |
|---|---|
| `on-demand` *(default)* | A page the first time it is read or transcluded on any tenant that lists the repository, kept while it is used and tombstoned after `mirror.grace` (default 90 days) without a read, as `files/{repo}` keeps the files that are used |
| `linked` | Every title linked from a local page of a listing tenant, to a depth of `mirror.depth` (default 1), with the templates and modules those pages use; refreshed as the links projection changes |
| `all` | Every page in the served namespaces, bootstrapped from a dump: the Enterprise HTML dump where credentials exist, which carries Parsoid HTML for every article, or the `pages-articles` dump with HTML fetched per page afterwards |

**Syncing** is an instance job per repository, run by the instance ([0040](0040-instance-prerogatives.md) §6), following the repository's events (§6) for pages in the set and writing a `put` per changed page; without events it polls `list=recentchanges`. `Special:Providers` shows its lag.

**Reads** come from `view.foreign_page` (§8), which the partition projects; the HTML part is served into the frame exactly as a proxied bundle is, through the same `fp:` cache key. A tenant reading a repository in `mirror` mode never fetches from the repository itself; a page outside the set under `on-demand` is fetched once, by the sync job, and is thereafter a record.

**Export.** `pages/{repo}` is public, so `Special:Export` ([0047](0047-special-pages.md) §8) can write its pages in MediaWiki XML, latest revision each, marked as foreign; the `records` format carries them with proofs, which is something a copy of Wikipedia has never had.

### 6. Following the repository: events and invalidation (settles 0042 Q5)

**A repository with `events` is followed; one without is polled or waits for its TTL.** The `events` field of a `page-repo` entry names the repository's EventStreams endpoint, and the instance follows one stream per repository, `mediawiki.page-change.v1` filtered by wiki and the served namespaces, which carries creates, edits, moves, deletes, undeletes and visibility changes; for an `enterprise` source the Realtime API's article stream is the same thing. Each event:

1. updates the title index (§3);
2. in `proxy` mode, purges the page's `fp:` key and its `Cache-Tag`, so the next read refetches; in `mirror` mode, queues a `put` for a page in the set;
3. for a Template or Module page, bumps the render epoch of every local page whose manifest names the repository and the title ([0042](0042-template-expansion-and-parsoid.md) §10), so a local page that uses a Wikipedia template re-renders when the template changes, which **settles 0042 Q5** without waiting for `cache_ttl`;
4. for a `page-move` or `page-delete`, queues a refresh of the local pages that link to the title, so their link classes follow.

Events are debounced per page over `repo.debounce` (default 30 seconds), because a busy article is edited many times a minute. The stream position is kept in `ops.repo_cursor` and resumed with `Last-Event-ID`; a gap longer than the stream's retention is closed by paging `list=recentchanges`, as [0011](0011-logs.md) §4 does.

`action=purge` on a foreign title drops its `fp:` entry and, in `mirror` mode, queues a `put`, under the `parse` rate class.

### 7. Search (extends 0014 §7)

- **In `proxy` mode a foreign page is not indexed.** Its text is a cache entry, not something the tenant holds. The suggester and `list=prefixsearch` read the title index for served namespaces after the local hits, each row marked with its origin ([0052](0052-page-repositories-and-title-inheritance.md) §6), so a reader can reach any inherited title by typing it.
- **In `mirror` mode a repository has one shared index,** `pages-{repo}`, at instance scope like a provider's `entities` index ([0018](0018-tenants.md) §6), indexing each mirrored page's title, text (the HTML stripped), categories and redirect titles. `list=search` and the search box run one `msearch` across the tenant's `pages` index and the `pages-{repo}` indexes of the repositories it lists in `mirror` mode, local hits first; the `site` setting **`search.inherited`** (`on` by default where a mirrored repository is listed) turns the foreign indexes off for tenants that want search to mean "what this wiki holds".

### 8. Storage (extends 0013 §5.6)

```sql
CREATE TABLE view.foreign_page (                 -- the bundles the instance holds (§1), both modes
  repo text NOT NULL, upstream_page_id bigint NOT NULL,
  ns integer NOT NULL, title text NOT NULL,
  revid bigint NOT NULL, revision_time timestamptz NOT NULL,
  content_model text NOT NULL,
  is_redirect boolean NOT NULL DEFAULT false, redirect_ns integer, redirect_title text, redirect_fragment text,
  metadata bytea NOT NULL,                        -- categories, templates, images, langlinks, page properties, sections; compressed
  retrieved timestamptz NOT NULL, expires_at timestamptz NOT NULL,
  partition bigint, "offset" bigint,              -- the mirrored-page record in mirror mode; NULL in proxy mode
  last_read timestamptz,                          -- for mirror.set = on-demand and the proxy cache's grace
  PRIMARY KEY (repo, upstream_page_id)
);
CREATE UNIQUE INDEX foreign_page_title ON view.foreign_page (repo, ns, title);

CREATE TABLE ops.repo_cursor (                   -- the event-stream position per repository (§6)
  repo text PRIMARY KEY, last_event_id text, last_event_time timestamptz, lag_seconds integer
);
CREATE TABLE ops.page_fetch (                    -- fetch and put queue, deduplicated (§4–6)
  repo text NOT NULL, ns integer NOT NULL, title text NOT NULL, reason text NOT NULL, queued timestamptz NOT NULL,
  PRIMARY KEY (repo, ns, title)
);
```

`view.foreign_page` is instance scope. In `proxy` mode its rows are the pages whose bundles are in the L2 cache, and a row is dropped when its bundle is evicted; it is the one `view` table that is not a projection of any log, which the eviction rule and the `proxy` mode's definition (§4) make deliberate, as [0042](0042-template-expansion-and-parsoid.md) §10 made the render tables. In `mirror` mode it is the projection of `pages/{repo}` and is rebuilt from it.

### 9. Attribution, licences and takedowns (extends 0039 §11)

**Every foreign page says where it is from.** The identity line ([0052](0052-page-repositories-and-title-inheritance.md) §5) names the repository, the revision and the licence, and the foot of the page carries the attribution line reused wiki text requires: "From English Wikipedia, revision 1234567890 (30 September 2026), licensed CC BY-SA 4.0; the authors are listed in its history", with each a link. The HTML carries `<link rel="license">` and the `origin` field is in every API response ([0052](0052-page-repositories-and-title-inheritance.md) §6). This is the attribution rule of 0042 §11 and 0039 §11, applied to whole pages.

**The repository's licence must be one the tenant can carry.** A tenant whose own content licence (`site` setting `content.licence`) is incompatible with a repository's is refused when it adds the repository to `pages.repos`, with `ts-licence-incompatible`: CC BY-SA text cannot be served under a CC0 wiki's terms. A tenant with no licence set is warned, not refused.

**Hiding a foreign page here** is a `read` ACL on its ranged page ID ([0023](0023-moderation.md) §2), which removes it from every stack on that tenant, as deleting a local page does; nothing is written to the repository and nothing in the mirror partition changes. **An operator takedown** of a mirrored page is an `erase` of its parts in `pages/{repo}` with the instance's authority ([0040](0040-instance-prerogatives.md)), as a file takedown is ([0039](0039-files-and-media.md) §10). Upstream deletions and hidings are followed in both modes (§5, §6).

### 10. API (extends 0012 §4 and §5)

| Module or route | Behaviour |
|---|---|
| `action=parse` on a foreign title | The rewritten HTML (§2), `categories`, `templates`, `images`, `langlinks`, `properties` and `revid` from the bundle; `prop=wikitext` the source |
| `prop=revisions&rvprop=content` | The bundle's wikitext, as [0052](0052-page-repositories-and-title-inheritance.md) §6 has it |
| `action=purge` | §6 |
| `GET /page/{id}/html` *(REST v0)* | The rewritten HTML; `as_of` is refused for a foreign page with `ts-foreign-history` |
| `GET /page/{id}/render` *(REST v0)* | For a foreign page: the repository, upstream revision, `retrieved`, `expires_at` and the templates and images the bundle records |
| `GET /repo/{name}` *(REST v0)* | The repository's status: mode, set, index date, event lag, cache and mirror sizes |
| `meta=siteinfo&siprop=triplespace` | `page_repos` gains `index_as_of`, `event_lag` and, in `mirror` mode, `mirrored_pages` |

### 11. Permissions and rate limits (extends 0024 §5)

Reading a foreign page needs `read`. Fetches are server-initiated and are bounded per repository (§1), not per reader; a reader who forces one, with `action=purge`, counts in the `parse` class. Live history fetches count in `upstream`. Writing a repository's mirror set, mode or events, and running `pages index`, need `ts-config` on the scope that holds the record; the sync job is an instance job.

### 12. Crates (amends 0005 §2; amends 0033 §10)

| Crate | Change |
|---|---|
| `scatter-adapter-mediawiki` *(new, ingest)* | Reading a MediaWiki wiki's pages: bundles from the REST and Action APIs and the Enterprise API, the `page` and `redirect` dumps and `all-titles` for the title index, `mediawiki.page-change.v1` events, and the **`.mwrev.zst` reader for RevisionChest stores**, which moves here from `scatter-adapter-wikidata` ([0033](0033-backend-stack.md) §10) so that both adapters share it; upstream revision records for pages ([0054](0054-forking-a-mirrored-page.md) §3). Depends on `scatter-providers`, `scatter-mwlog`, `scatter-pages` |
| `triplespace-repos` *(new, surfaces)* | Page repositories: bundle fetching through `triplespace-upstream`, the HTML rewrite (§2), the title index job (§3), the proxy caches (§4), the `pages/{repo}` sync job and its projection (§5), event following and invalidation (§6), the `pages-{repo}` indexes (§7) and the repository routes (§10). Depends on `scatter-adapter-mediawiki`, `triplespace-upstream`, `triplespace-files` (the blob cache and file policy), `triplespace-render` (manifest invalidation), `triplespace-cache`, `triplespace-search` |
| `scatter-log` | The `pages/{repo}` graph and the `scatter:v0/mirrored-page` payload type with its five parts |
| `scatter-adapter-wikidata` | The `.mwrev.zst` reader leaves it for `scatter-adapter-mediawiki`, on which it now depends |

## Alternatives considered

- **Rendering foreign pages from wikitext here,** through the expander and the template repository. Rejected for the reading stage: it needs the whole template and module tree of the repository, Lua for nearly every article, TemplateStyles and site styles, and it would still differ from upstream's rendering in every place Triplespace's parser does. It is exactly the work a fork takes on ([0054](0054-forking-a-mirrored-page.md)), for the pages a tenant chooses to own.
- **Hotlinking:** an `<iframe>` or a redirect to the repository. Rejected: the reader leaves the frame, the links do not resolve through the stack, and the repository sees every reader.
- **Mirroring HTML without rewriting.** Rejected: unrewritten links leave the wiki, and unsanitized HTML from another site is an injection risk whatever the site.
- **One mode.** `proxy` alone cannot survive the repository being down or prove what was served; `mirror` alone is too heavy for a tenant that only wants Wikipedia's links to work. File repositories have both for the same reasons.
- **Fetching with the reader's browser.** Rejected: the repository would see every reader, and the page could not be rewritten or sanitized here.

## Consequences

- **A Wikipedia article reads here as it reads there,** infobox, citations and styles included, because Wikipedia rendered it. Nothing about the tenant's own parser limits what a mirrored page can show.
- **Mirroring is cheap by default.** `proxy` mode holds only what is being read, for an hour, and nothing in the log; the title index is the one standing cost.
- **`mirror` mode gives what no Wikipedia copy has had:** a verifiable, exportable, locally served record of each page, following upstream's edits, deletions and hidings.
- **Freshness is bounded by `cache_ttl` or by the event stream.** With events, a mirrored page follows Wikipedia within the debounce window; without, within the hour.
- **Template changes upstream now invalidate local renders promptly** (0042 Q5), as a by-product of following the stream.
- **The rendered HTML is Wikipedia's, so its classes are Wikipedia's.** Infoboxes and navboxes assume Wikipedia's site styles, which [0055](0055-templatestyles-templatedata-and-page-properties.md) §4 lets a tenant supply; until a tenant does, mirrored pages look plainer than upstream.
- **One `view` table is not a projection** in `proxy` mode, by design, as the render tables already are.
- **Two new crates.** `scatter-adapter-mediawiki` is the first adapter for a wiki that is not a Wikibase, and RevisionChest's format moves to it.

## Open questions

- **Q1. Enterprise as the default for Wikimedia wikis.** Whether an instance with Enterprise credentials should prefer `enterprise` for every Wikimedia repository automatically, given its one-call bundle and its Realtime stream.
- **Q2. Mirroring categories' members.** `mirror.set = all` holds every page of a category; whether `view.page_category` should then include mirrored members for the listing tenants ([0052](0052-page-repositories-and-title-inheritance.md) Q3).
- **Q3. Interlanguage links.** A bundle carries the repository's language links; whether the frame should show them as sitelinks of the paired item, as upstream's are, or as links out.
- **Q4. `full` history for a mirror.** The partition allows it; whether any tenant wants every rendering of every mirrored page kept, and what `Special:Export` should write for it.
- **Q5. Pre-rendering the frame.** Whether the rewritten HTML could be served from the HTTP cache with the frame rendered client-side, for mirrors whose read traffic dwarfs everything else.
- **Q6. Non-Parsoid repositories.** How far section editing and fork-over-HTML should degrade for a repository whose HTML comes from the legacy parser, or whether such repositories should be served from wikitext through the expander instead.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §12 | amends | 0005 A52 |
| [0012](0012-api-requirements.md) §4, §5 | §10 | extends | 0012 A32 |
| [0013](0013-postgres-storage.md) §5.6 | §8 | extends | 0013 A23 |
| [0014](0014-caches-and-search.md) §4, §7 | §4, §7 | extends | 0014 A11 |
| [0015](0015-record-format-and-partition-registry.md) §3, §5 | §5 | extends | 0015 A24 |
| [0033](0033-backend-stack.md) §10 | §12 | amends | 0033 A6 |
| [0039](0039-files-and-media.md) §11 | §2, §9 | extends | 0039 A6 |
| [0042](0042-template-expansion-and-parsoid.md) Q4, Q5 | §5, §6 | settles | 0042 Q4, Q5 |
| [0022](0022-federation.md) §11 | §3 | extends | 0022 A5 |

## References

- [Parsoid's HTML specification](https://www.mediawiki.org/wiki/Specs/HTML) and [`data-mw`](https://www.mediawiki.org/wiki/Specs/data-mw): the contract §2 rewrites
- [MediaWiki REST API](https://www.mediawiki.org/wiki/API:REST_API): `GET /v1/page/{title}/with_html`
- [Wikimedia Enterprise](https://enterprise.wikimedia.com/docs/): the On-demand, Realtime and Snapshot APIs
- [EventStreams](https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams) and the [`mediawiki/page/change` schema](https://schema.wikimedia.org/#!/primary/jsonschema/mediawiki/page/change)
- [Wikimedia Enterprise HTML dumps](https://dumps.wikimedia.org/other/enterprise_html/) and the [`page` and `redirect` SQL dumps](https://meta.wikimedia.org/wiki/Data_dumps/What%27s_available_for_download)
- [Creative Commons Attribution-ShareAlike 4.0](https://creativecommons.org/licenses/by-sa/4.0/) §3(a), on attribution, and [Wikipedia:Reusing Wikipedia content](https://en.wikipedia.org/wiki/Wikipedia:Reusing_Wikipedia_content)

## Amendment log

### A1. Followed talk pages

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §1, §2
- **Change:** extends §1, §5
- **Summary:** A followed talk page's bundle carries DiscussionTools' thread items; it is mirrored in `pages/{repo}` whatever the `mode`, with `threads` in the content part (numbers, name hashes, comment structure), comment authors in the attestation part, and the changed threads listed per `put`.
