# 0053. Mirrored pages

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-09 (A5)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0022](0022-federation.md), [0033](0033-backend-stack.md), [0039](0039-files-and-media.md), [0042](0042-template-expansion-and-parsoid.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0040](0040-instance-prerogatives.md), [0047](0047-special-pages.md), [0051](0051-page-redirects.md), [0052](0052-page-repositories-and-title-inheritance.md), [0054](0054-forking-a-mirrored-page.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [09](../architecture/09-security-and-moderation.md), [13](../architecture/13-mirrored-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

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

*Current text: [13](../architecture/13-mirrored-pages.md) §2.1.*

### 2. Rewriting the HTML (extends 0008 §8; uses 0039 §11)

*Current text: [13](../architecture/13-mirrored-pages.md) §2.2.*

### 3. The title index (extends 0052 §8)

*Current text: [13](../architecture/13-mirrored-pages.md) §1.8, §2.3.*

### 4. Proxy mode: fetched on demand, cached, never logged (extends 0014 §4)

*Changed by A3.*

*Current text: [13](../architecture/13-mirrored-pages.md) §2.4.*

### 5. Mirror mode: the `pages/{repo}` partition (extends 0015 §3 and §5; uses 0002 §2, §5, 0006 §4)

*Changed by A1.*

*Current text: [13](../architecture/13-mirrored-pages.md) §1.6, §2.5, §2.6, §5.4.*

### 6. Following the repository: events and invalidation (settles 0042 Q5)

*Current text: [13](../architecture/13-mirrored-pages.md) §2.7.*

### 7. Search (extends 0014 §7)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §11.1, §11.2, §11.4, §12.5.*

### 8. Storage (extends 0013 §5.6)

*Changed by A2.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.12, §5.*

### 9. Attribution, licences and takedowns (extends 0039 §11)

*Changed by A4.*

*Current text: [13](../architecture/13-mirrored-pages.md) §2.8, §2.9.*

### 10. API (extends 0012 §4 and §5)

*Current text: [18](../architecture/18-api.md) §2.2, §2.3, §3.2, §5.2.*

### 11. Permissions and rate limits (extends 0024 §5)

*Current text: [07](../architecture/07-actors-and-accounts.md) §6.2; [09](../architecture/09-security-and-moderation.md) §8.8.*

### 12. Crates (amends 0005 §2; amends 0033 §10)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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

### A2. In proxy mode `view.foreign_page` indexes the L1 cache

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §8
- **Summary:** In `proxy` mode `view.foreign_page` indexes the **L1** (Valkey) cache, where [0014](0014-caches-and-search.md) §4 keys the bundle `fp:` (§4), not L2; a row is dropped when its L1 entry is evicted. The 0053 row of [0013](0013-postgres-storage.md) §5.6 is corrected the same way. (PENDING A7)

Replaced text (§8):

> `view.foreign_page` is instance scope. In `proxy` mode its rows are the pages whose bundles are in the L2 cache, and a row is dropped when its bundle is evicted; it is the one `view` table that is not a projection of any log, which the eviction rule and the `proxy` mode's definition (§4) make deliberate, as [0042](0042-template-expansion-and-parsoid.md) §10 made the render tables.

### A3. The field is `repo.cache_ttl` throughout

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §4
- **Summary:** The repository field is named `repo.cache_ttl` everywhere, in [0052](0052-page-repositories-and-title-inheritance.md) §1 (0052 A3) and in §4's L1 row and prose alike, the field PENDING C20 makes per-repository with a per-kind default (one hour for page bundles); §4 named it `cache_ttl` in three places and `repo.cache_ttl` in one. (PENDING F1)

Replaced text (§4):

> | L1 ([0014](0014-caches-and-search.md) §4) | `fp:{repo}:{ns}:{title}:{revid}` | The rewritten HTML and the metadata, compressed | `cache_ttl` ([0052](0052-page-repositories-and-title-inheritance.md) §1, default one hour), or until an event purges it |

> The frame is drawn per request as it is for a local page; the HTML inside it is the cached fragment, so a page is fetched and rewritten once per `cache_ttl` however many readers it has.

> When an entry passes `expires_at` the server checks the repository's latest revision ID (`prop=info`, the REST route's `ETag`, or the Enterprise version) before refetching, so an unchanged page costs one small request per `cache_ttl`.

### A4. The Delete action writes the hiding ACL

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §9
- **Summary:** A foreign page is hidden as a local page is deleted: the Delete action, held by an administrator (`delete`), writes the `read` ACL §9 describes, and [0052](0052-page-repositories-and-title-inheritance.md) §5 (0052 A4) lists Delete on a foreign page with that meaning. The ledger row's verb is amends, but §9 already says hiding is that ACL and names no action or right, so this entry extends it. (PENDING F2)

### A5. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§12
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [09](../architecture/09-security-and-moderation.md), [13](../architecture/13-mirrored-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
