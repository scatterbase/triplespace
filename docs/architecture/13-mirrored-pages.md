# 13. Mirrored pages

This chapter covers pages a tenant serves without holding them: page repositories and the title stack that inherits from them, the bundle the instance keeps for a foreign page in `proxy` and `mirror` mode, forking a foreign page into a local one and seeding its history, merging a fork with upstream in both directions, talk pages that follow a repository's, and the statements an extraction source derives from mirrored pages through template mappings and line rules. It assumes the log and record format of [01](01-log-and-records.md), the graph catalogue of [02](02-graphs-rdf-and-query.md), the `view` tables and caches of [03](03-storage-caches-and-search.md), the provider-ranged page ID of [04](04-entities-and-identifiers.md), the change-set operations and shallow mirroring of [05](05-providers-and-ingest.md), the title resolver and redirects of [10](10-pages-and-content-models.md), the expander and template repositories of [11](11-rendering-templates-and-modules.md), file repositories of [12](12-files-and-media.md) and threads of [14](14-discussions.md). The permissions and rate classes are in [09](09-security-and-moderation.md) and [07](07-actors-and-accounts.md), the routes in [18](18-api.md), the settings in [23](23-configuration-and-registry.md) and the crates in [22](22-crates-and-stack.md).

## 1. Page repositories and title inheritance

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §1, §2, §3, §4, §5, §7.*

### 1.1 A page repository

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §1; [0069](../decisions/0069-synchronized-talk-pages.md) §1; [0080](../decisions/0080-tenants-as-entity-sources.md) §6.*

**A page repository is a source of pages a tenant serves by title without holding them.** It generalises the template repository of [0042](../decisions/0042-template-expansion-and-parsoid.md) §11 to any `pages` namespace, and it is configured as a file repository is ([0039](../decisions/0039-files-and-media.md) §11, in [12](12-files-and-media.md)): a `config` record of kind **`page-repo`**, keyed `page-repo:{name}`, in a tenant's `config` or in the instance `config`, where a tenant refers to it by name and a tenancy template may supply it ([0028](../decisions/0028-tenancy-policy.md) §8, in [08](08-tenants-and-instances.md)).

| Field | Meaning |
|---|---|
| `kind` | `tenant`: another tenant on this instance (§1.8). `mediawiki`: any MediaWiki Action API, a Wikipedia above all, or a Triplespace tenant elsewhere |
| `provider` | The provider registry entry the repository belongs to ([0015](../decisions/0015-record-format-and-partition-registry.md) §5, `providers.toml`). It supplies the provider **number**, for ranged IDs (§1.6); the **issuer**, for the actors of its revisions ([0007](../decisions/0007-actor-identity.md) §1); and its API endpoint and article path. For a `tenant` repository it is the tenant's own entry where it has one ([0018](../decisions/0018-tenants.md) §5), and otherwise the reading tenant's tenant source for it, declared first, which supplies the number, the issuer and the IRIs ([0080](../decisions/0080-tenants-as-entity-sources.md) §6). A provider that mints no entities has no `code` and no types; `scatter-providers` accepts such an entry, and English Wikipedia is one |
| `namespaces` | The repository's namespaces the tenant serves, by the repository's canonical names, with `main` for namespace 0. Each is served in the local namespace of the same canonical name. The default is `["Template", "Module"]`, which is what a template repository served; MDWiki's English Wikipedia entry is `["main", "Template", "Module", "Category"]` |
| `mode` | `proxy` (default): pages are fetched and cached (§2.4). `mirror`: pages are kept in an instance partition (§2.5) |
| `shadowed` | What becomes of this repository's page when a higher-ranked page exists under the same title: `offer` (default), it is offered as an alternate (§1.5); `hide`, it is not shown at all |
| `titles` | `index` (default for `mediawiki`): the repository's titles are mirrored into a title index (§2.3), so links, redirects and `#ifexist` are answered locally. `assume`: every title is presumed to exist, for a repository with no dump to index |
| `repo.cache_ttl` | How long fetched source and bundles are held, default one hour, as [0042](../decisions/0042-template-expansion-and-parsoid.md) §11 had it |
| `events` | Optional: an EventStreams endpoint for push invalidation (§2.7) |
| `talk` | `link` (default): a foreign page's talk page holds local threads and links to the repository's talk page (§1.7). `sync`: the repository's talk pages paired with the served namespaces are followed, mirrored whatever the `mode`, and their sections are shown as foreign threads (§5) |
| `licence`, `display_name` | The licence of the repository's text, as an SPDX identifier (`CC-BY-SA-4.0` for Wikimedia projects), required for `mediawiki`; and the name shown on origin chips and attribution lines |

`repo.cache_ttl` is a per-repository field of the `page-repo` or `file-repo` record, under that one name everywhere, with a default per kind: 7 days for files, one hour for template source and page bundles.

**The tenant's order is the inheritance.** The `site` setting **`pages.repos`** lists the repositories the tenant uses, by name, in order. It replaces `wikitext.template_repos`, and **`pages.share`** replaces `wikitext.share` ([0042](../decisions/0042-template-expansion-and-parsoid.md) §2): a tenant that sets it may serve its pages to other tenants as a `tenant` repository.

**The `template-repo` kind is retired.** A template repository is a page repository whose `namespaces` include `Template` and `Module`. Everything 0042 §11 says of how a foreign template is read, expanded, cached, attributed and trusted holds for a page repository ([11](11-rendering-templates-and-modules.md)); 0042 §11 now reads as the Template and Module case of this chapter. Data modules ([0043](../decisions/0043-lua-modules.md) §6) are keyed by repository, title and remote revision as before. Which talk namespaces `talk = sync` follows is §5.1.

### 1.2 A title names a stack

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §2.*

**For a title in a `pages` namespace, the stack is every page that exists under it, in inheritance order:**

1. the **local page**, if one exists and is not deleted ([0023](../decisions/0023-moderation.md) §4);
2. then, for each repository in `pages.repos` that serves the namespace, **that repository's page**, if its title index has the title (or `titles = assume`).

Each entry carries its **origin**: `local`, or the repository's name. **The first entry is the primary; the rest are alternates.** A title whose stack is empty does not exist.

Two things in the order are fixed and not configuration:

- **A local page is always first.** The tenant's own page outranks every repository's, as the local graph wins over every mirror ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3), a local file shadows a foreign one ([0039](../decisions/0039-files-and-media.md) §11) and a local template shadows a foreign one ([0042](../decisions/0042-template-expansion-and-parsoid.md) §11). This is the MDWiki rule: where the wiki has forked a page, its fork is the page.
- **Repositories rank in the order the tenant lists them.** A tenant that reads two Wikipedias decides which one answers first.

**A deleted local page leaves the stack,** so deleting a fork makes the repository's page primary again (§3.9). A create-protected title ([0023](../decisions/0023-moderation.md) §2) reserves a page ID and nothing else: the stack is unaffected, and what the protection prevents is the fork.

**A stack is computed, never stored.** The resolver reads `view.page` for the local entry and `view.foreign_title` ([03](03-storage-caches-and-search.md) §4.12) for the others; nothing records the order, so changing `pages.repos` changes every stack at once.

### 1.3 Resolution

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §3.*

**The title resolver resolves a `pages` title to its primary.** The resolve step of [0008](../decisions/0008-namespaces-and-document-pages.md) §3 ([10](10-pages-and-content-models.md)) reads: in a `pages` namespace, a title resolves to the primary of its stack, which is a local page ID or a foreign page named by repository and upstream page ID. Then [0051](../decisions/0051-page-redirects.md) §2 applies: if the primary is a redirect, local or foreign, the resolver resolves the **target title's stack**, one hop. So English Wikipedia's redirect *Heart attack* → *Myocardial infarction* lands a reader on MDWiki's fork of *Myocardial infarction* when there is one, and on Wikipedia's article when there is not.

**Everything that asks "which page is this title?" gets the primary:** page views, `titles=` in the API, transclusion (`{{Foo}}`, `{{:Foo}}`), `#ifexist`, Lua's `mw.title` ([0043](../decisions/0043-lua-modules.md) §5), link colouring, and the file lookup for a `File:` title in a served namespace. This is the lookup [0042](../decisions/0042-template-expansion-and-parsoid.md) §11 already gave templates, stated once for every namespace, and its rule that **lookup restarts at local at every level** is the same rule: each nested transclusion resolves its own stack, so a foreign `Template:Infobox` that calls `{{Infobox/row}}` gets the local `Template:Infobox/row` if one exists.

**Everything that asks "which pages exist under this title?" gets the stack:** the frame (§1.5), `GET /page/stack/{title}` ([18](18-api.md)) and the fork form (§3.2).

**A title index answers without the network.** With `titles = index`, whether a foreign page exists, and whether it is a redirect and to what, is a local read of `view.foreign_title`; rendering a page with five hundred links costs no requests to the repository. With `titles = assume`, every link to a served namespace is presumed to exist, and a reader who follows one that does not meets the repository's "not found", rendered as a missing page with a link upstream. `assume` is for repositories that publish no title dump, and it is why `index` is the default.

### 1.4 Links continue reading

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §4.*

**A link resolves through the stack, so a reader keeps reading.** In the wikitext subset, in markdown ([0019](../decisions/0019-discussions.md) §5) and in rewritten foreign HTML (§2.2), a wiki link to a title whose primary is foreign is an ordinary blue link to the local URL of that title. Following it shows the foreign page in this tenant's frame (§1.5), whose own links resolve the same way. The link carries the class `ts-inherited` and `data-ts-origin="{repository}"`, so a tenant's styles or a reader's preference can mark it, and nothing else distinguishes it. A title with an empty stack is a red link, as any missing page is. A link to a redirect carries `mw-redirect` ([0051](../decisions/0051-page-redirects.md) §2).

**Links out of the served namespaces leave the wiki.** A link in a foreign page to a namespace the repository does not serve here, `Wikipedia:Manual of Style` on a tenant that serves only `main`, `Template`, `Module` and `Category`, is rewritten to the repository's own URL and rendered as an interwiki link (`class="extiw"`), because there is nothing here for it to resolve to. A tenant's own pages link to the repository explicitly with its site alias as before, `[[enwiki:Foo]]` ([0026](../decisions/0026-sitelinks.md) §2), and that link is not inheritance.

**"What links here" counts links to inherited titles.** The links projection ([0008](../decisions/0008-namespaces-and-document-pages.md) §10, in [10](10-pages-and-content-models.md)) records a `page_link` row by title for a link whose primary is foreign, keyed by namespace and title as a link to a missing page is, so that when the title is forked the fork's backlinks are already there.

### 1.5 The frame: primary, origin and alternates

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §5.*

**The page view shows the primary** in the tenant's frame: the tenant's header, search, tabs and chrome, with the foreign content inside it as §2.2 renders it.

**The identity line names the origin.** A foreign primary carries an **origin chip**, the provider's chip from the registry as a mirrored entity carries one ([0010](../decisions/0010-site-ui.md) §2), and the line reads "From English Wikipedia, revision 1234567890 as of 2026-09-30 · CC BY-SA 4.0", linking the title upstream and the licence. A local primary with alternates reads as any local page does, and gains the control below.

**Alternates are offered or hidden.** When the stack has alternates, the identity line gains **Other versions**, a menu listing each alternate whose repository is `offer`: its origin, its latest revision and date, and, for a local page that is a fork, "Forked from this at revision N" (§3.7). An alternate whose repository is `hide` is not listed. When nothing is listable, the control is not drawn.

**Reading an alternate** is the title's URL with **`origin={repository}`**, served in the same frame with the identity line saying so and a link back to the primary. The response is `noindex` and its canonical link is the primary; it is never what a search engine or a link indexes. An `origin` that names a hidden alternate, or one the stack does not have, answers 404 with `ts-origin-hidden` or `ts-origin-missing`.

**A page may override its repositories' defaults.** A local primary may carry a page statement ([0038](../decisions/0038-page-metadata-and-categories.md) §1, in [06](06-statements-and-properties.md)) whose property is bound to the role **`page-alternates`**, a `string` property whose value is `offer` or `hide`. It applies to every alternate of that title and outranks the repositories' `shadowed` settings, so a wiki whose repositories default to `hide` can offer Wikipedia's article beneath one page as a supplement, and a wiki that defaults to `offer` can hide it under another. The role is registry data like `thread-status` ([0038](../decisions/0038-page-metadata-and-categories.md) §9); unbound, the repositories' defaults decide.

**Edit forks.** The Edit tab of a foreign primary opens the editor on the foreign page's wikitext and says, before anything is typed, that saving will create a local copy that no longer follows the repository (§3.2). Move and Change content model are not offered on a foreign page; **Delete** on a foreign page, held by an administrator (`delete`), hides it here by writing the `read` ACL of §2.9; **Protect…** on a title whose primary is foreign offers create-protection only, which is how a tenant stops a title being forked.

### 1.6 Identity

*Sources: [0053](../decisions/0053-mirrored-pages.md) §5; [0054](../decisions/0054-forking-a-mirrored-page.md) §3.*

A foreign page's `pageid` is the **provider-ranged** form of the repository's own page ID and its `lastrevid` the ranged form of the upstream revision the tenant holds; the derivation, and the refusal of a foreign primary as a sitelink target, are in [04](04-entities-and-identifiers.md) §5.3 ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6). A ranged revision ID is `number << 40 | upstream revision ID` ([0013](../decisions/0013-postgres-storage.md) §6), so `oldid=` with that number serves the revision and no local revision ever shares it. In `mirror` mode the `pages/{repo}` record carries the ranged page ID in header field 9 and the ranged revision ID in field 7 (§2.5), so a mirrored page's API identity is the same in both modes. What the API reports for a foreign primary is in [18](18-api.md).

### 1.7 Talk pages, statements, categories and feeds

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §7.*

**A foreign page's talk page is local.** Threads attach to `{kind: page, id: <the ranged page ID>}` ([0019](../decisions/0019-discussions.md) §2, in [14](14-discussions.md)), exactly as they attach to a mirrored entity. Under `talk = link` the talk page offers a link to the repository's own talk page for readers who want that discussion; under `talk = sync` it shows that discussion, the repository's sections as foreign threads beside the local ones, and a person holding an upstream grant may reply to them or start a section upstream (§5.7–5.10). When the page is forked, the fork job moves these threads to the fork's talk page (§5.12). Threads about a hidden alternate's talk page attach to the primary's.

**A foreign page carries no local statements.** Page statements are keyed by page ID in the tenant's `pages` partition ([0038](../decisions/0038-page-metadata-and-categories.md) §1), and a ranged ID could key them; but a page with local statements and no local text would be a third kind of entry, neither inherited nor forked, and the fork would have to carry the statements across a change of page ID. Local metadata about a foreign page waits for a decision on that shape. Until then, a fork is the way to say anything local about a page.

**Categories are shown, not joined.** A foreign page's categories, from its bundle, are listed at its foot as links to `Category:` titles, which resolve through the stack like any title. The page is not a member in `view.page_category`, whose rows are a projection over local pages ([0038](../decisions/0038-page-metadata-and-categories.md) §3, in [10](10-pages-and-content-models.md)); a category page lists its local members and, when the category is served by a repository, links to the members upstream.

**Feeds.** A foreign page has no activity rows in the tenant: its changes are the repository's. Watching a title whose primary is foreign watches its local talk page and, through the title, the fork when one is made. Upstream changes reach a watcher through the repository's own feeds, which the talk page links to. What a followed talk page contributes to feeds and watches is in [16](16-logs-feeds-and-notifications.md).

### 1.8 Tenant repositories

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §7; [0053](../decisions/0053-mirrored-pages.md) §3.*

**Tenant repositories** (`kind = tenant`) are read directly, as `tenant` file and template repositories are: the source tenant's `view.page` rows and records, with its ACLs evaluated as the source tenant's, so a page deleted there is missing here. The source must set `pages.share = on`; under `providers.between_tenants = operator` only the operator can; reader lists apply ([0028](../decisions/0028-tenancy-policy.md) §5, in [08](08-tenants-and-instances.md)); a private tenant cannot share. **The page is rendered here, in this tenant's context,** by this tenant's expander over this tenant's stacks, as an inherited template is expanded in the local page's context ([0042](../decisions/0042-template-expansion-and-parsoid.md) §11); the source tenant's rendered HTML is not reused. A `tenant` repository has no title index: the source tenant's `view.page` is read directly.

## 2. Mirrored pages

*Sources: [0053](../decisions/0053-mirrored-pages.md) §1, §2, §3, §4, §5, §6, §9.*

### 2.1 The page bundle

*Sources: [0053](../decisions/0053-mirrored-pages.md) §1.*

**What the instance holds for a foreign page is a bundle,** one per page per repository, for the latest upstream revision only:

| Part | Holds |
|---|---|
| **Identity** | The repository, the upstream namespace and title, the upstream page ID, the upstream revision ID and its timestamp, the content model, and the licence from the repository's entry |
| **Wikitext** | The revision's source, for the Edit tab, `prop=revisions&rvprop=content`, transclusion ([0042](../decisions/0042-template-expansion-and-parsoid.md) §11) and forking (§3.4) |
| **HTML** | The repository's rendering of the revision, rewritten (§2.2) and sanitized, ready to place in the frame |
| **Metadata** | Categories, templates and modules used, images used, interlanguage links, page properties (display title, short description, disambiguation, `noindex`), the redirect target if the page is one, and the `data-mw`-bearing section map; for a followed talk page, the thread items of `action=discussiontoolspageinfo` (§5.3) |
| **Freshness** | When it was retrieved, the upstream `ETag` or render ID, and `expires_at` |

**Three sources, chosen per repository with `source`:**

| Source | Reads | For |
|---|---|---|
| `rest` *(default)* | MediaWiki core's REST API, `GET {rest base}/v1/page/{title}/with_html`, which returns the source, the Parsoid HTML, the latest revision and the licence in one response; then one `action=query` for the metadata (`prop=categories\|templates\|images\|langlinks\|pageprops\|info`) | Any MediaWiki 1.35 or later with Parsoid, every Wikimedia wiki, and a Triplespace tenant elsewhere, which serves the same route ([0012](../decisions/0012-api-requirements.md) §5) |
| `enterprise` | The Wikimedia Enterprise On-demand API, `GET /v2/articles/{name}` filtered to the project, which returns the Parsoid HTML, the wikitext, the version, the licence, the categories, templates, redirects and the article's main Wikidata entity in one response | Wikimedia projects, where an instance holds Enterprise credentials; its Realtime stream is the `events` source of §2.7 |
| `action` | `action=parse` with `prop=text\|wikitext\|categories\|templates\|images\|langlinks\|properties\|revid`, the legacy parser's HTML | A MediaWiki without Parsoid's REST routes |

Credentials for `enterprise` are deployment configuration ([0033](../decisions/0033-backend-stack.md) §12), never a log record. Every source goes through the live upstream client ([0012](../decisions/0012-api-requirements.md) §6, `triplespace-upstream`): a proper User-Agent, `maxlag`, a per-repository concurrency cap (`repo.concurrency`, default 4) and a per-repository fetch budget (`repo.fetches_per_minute`, default 600), so a crawler reading an instance cannot turn it into a crawler of Wikipedia. **A bundle is never logged in `proxy` mode** (§2.4) and is a record in `mirror` mode (§2.5).

**Latest only.** A bundle holds one revision. A reader who wants an older one, or the history, is sent upstream, and the History tab fetches the upstream history live as [0010](../decisions/0010-site-ui.md) §5.5 does for entities, under the `upstream` rate class ([0024](../decisions/0024-subsidiary-accounts.md) §5, in [07](07-actors-and-accounts.md)), never logging it. History arrives here only with a fork.

### 2.2 Rewriting the HTML

*Sources: [0053](../decisions/0053-mirrored-pages.md) §2.*

**The repository's HTML is rewritten once, when the bundle is made, into HTML that belongs to this tenant.** The rewrite is a single pass over the DOM:

| What | Becomes |
|---|---|
| **Sanitization** | The same sanitizer and allow-list that Parsoid's own output passes through here ([0042](../decisions/0042-template-expansion-and-parsoid.md) §8.4): `typeof`, `about`, `data-mw`, `rel`, `property` and `id` are kept; `<script>`, event attributes and anything else outside the allow-list are dropped; `style` attributes are sanitized as [0008](../decisions/0008-namespaces-and-document-pages.md) §8 sanitizes them |
| **`<head>`, `<base>`** | Dropped. The HTML is a body fragment inside the tenant's frame (§1.5) |
| **Wiki links** (`rel="mw:WikiLink"`, `href="./Title"`; or the article path's `/wiki/Title` from the legacy parser) | A link to the **local** URL of the same title when its namespace is one the repository serves here, so the link resolves through the stack (§1.3–1.4) and the reader keeps reading; otherwise the repository's own URL with `class="extiw"`, a link that leaves the wiki. A link whose title the title index (§2.3) lacks gets `class="new"`, a red link, as upstream drew it; a link to a redirect keeps `mw-redirect` ([0051](../decisions/0051-page-redirects.md) §2) |
| **Interwiki and external links** (`mw:WikiLink/Interwiki`, `mw:ExtLink`) | Kept as they are, with `rel="nofollow"` as upstream has it |
| **Images** (`<img>`, `srcset`, `<figure>`, `mw:File`) | Served by the tenant's **file policy**: the `File:` link goes to the local file title, and the bytes follow the first file repository in `files.repos` that holds the file, in its mode ([0039](../decisions/0039-files-and-media.md) §11, in [12](12-files-and-media.md)): `proxy` fetches and serves them from the media base, `link` leaves the repository's URLs, `mirror` serves the copy. An `<img>` whose source is not a file in any repository, a rendered formula from a math service for instance, is fetched through the repository's `media_hosts` allow-list in `proxy` mode and dropped otherwise, since external image URLs are never embedded ([0008](../decisions/0008-namespaces-and-document-pages.md) §8) |
| **Categories** (`<link rel="mw:PageProp/Category">`) | Removed from the body and recorded in the bundle's metadata; the frame lists them at the foot as §1.7 says |
| **TemplateStyles** (`<style data-mw-deduplicate>`) | Passed through the CSS sanitizer of [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §2 ([11](11-rendering-templates-and-modules.md)) and kept, deduplicated by their key as Parsoid deduplicates them |
| **Sections** (`<section data-mw-section-id>`) | Kept. The frame draws its own section edit links from them, each opening the fork editor at that section (§3.2) |
| **`data-mw`, `typeof`, `about`** | Kept, so that a fork can later be edited over its HTML and so that the templates a page uses can be read from it without a second parse |
| **Indicators, `mw:PageProp/toc`, `displaytitle`** | Recorded as metadata and drawn by the frame as the tenant draws its own |

**The rewritten HTML depends on the repository's configuration and the title index, not on the stack.** A link is `/wiki/Title`; what the title shows is decided when it is followed. So a fork made after a page was cached changes nothing in the cached HTML, which is correct, and a title created locally after a page was cached turns a red link blue only when the cache entry is refreshed (§2.4), which is the one staleness the design accepts.

**The legacy parser's HTML** (`source = action`) is rewritten by the same rules, matching links by the article path and `title` attributes in place of `rel` and `./`; it carries no `data-mw`, no section IDs and no TemplateStyles keys, so section editing and fork-over-HTML are unavailable for pages from such a repository.

### 2.3 The title index

*Sources: [0053](../decisions/0053-mirrored-pages.md) §3.*

**A repository's titles are mirrored, so the stack is a local read.** For a repository with `titles = index`, an instance job, `pages index {repo}`, fills `view.foreign_title` ([03](03-storage-caches-and-search.md) §4.12) for the served namespaces:

| Step | Source |
|---|---|
| **Bootstrap** | The repository's `page` and `redirect` SQL dumps, or its `all-titles` dump with `list=allpages&apfilterredir=redirects` for the redirect targets; for a Wikimedia wiki, the Enterprise snapshot's metadata where credentials exist. Each row records the upstream page ID, whether the page is a redirect and its target, and the latest revision ID |
| **Keeping current** | The repository's events (§2.7): `page-create`, `page-delete`, `page-undelete`, `page-move` and a revision that changes whether the page is a redirect, for the served namespaces. Without events, `list=recentchanges` is paged from the last cursor on a schedule, as [0011](../decisions/0011-logs.md) §4 fills gaps in a log mirror |
| **Reconciliation** | A fresh dump replaces the index for a namespace in one transaction, and reports how many titles the events had missed |

`Special:Providers` ([0022](../decisions/0022-federation.md) §11, [0047](../decisions/0047-special-pages.md) §3, in [21](21-special-pages.md)) lists each page repository with the date of its index and its event lag, beside the data providers' sync lag.

### 2.4 Proxy mode: fetched on demand, cached, never logged

*Sources: [0053](../decisions/0053-mirrored-pages.md) §4.*

**With `mode = proxy`, a bundle is made the first time a page is read or transcluded** and kept in the caches:

| Layer | Key | Holds | Lifetime |
|---|---|---|---|
| L1 ([0014](../decisions/0014-caches-and-search.md) §4; [03](03-storage-caches-and-search.md) §12.1) | `fp:{repo}:{ns}:{title}:{revid}` | The rewritten HTML and the metadata, compressed | `repo.cache_ttl` (§1.1, default one hour), or until an event purges it |
| L2, the blob cache of [0039](../decisions/0039-files-and-media.md) §11 | `cache/{repo}/pages/{hash}` | The whole bundle, wikitext included | `repo.cache_ttl` with a size cap and least-recently-used eviction, shared with the repository's file cache |

**A rendered foreign page is the bundle's HTML inside the tenant's frame.** The frame is drawn per request as it is for a local page; the HTML inside it is the cached fragment, so a page is fetched and rewritten once per `repo.cache_ttl` however many readers it has. The response carries `Cache-Tag: repo:{repo}:{upstream page ID}` so that a purge reaches the HTTP layer ([0014](../decisions/0014-caches-and-search.md) §5–6).

**Revalidation is cheap.** When an entry passes `expires_at` the server checks the repository's latest revision ID (`prop=info`, the REST route's `ETag`, or the Enterprise version) before refetching, so an unchanged page costs one small request per `repo.cache_ttl`.

**Readers' addresses never reach the repository.** The server fetches, as it does for a proxied file. There is no `link` mode for pages: HTML is always rewritten here, so it is always fetched here.

**Nothing in `proxy` mode is a record.** The bundle is not in the log, has no revision ID of the tenant's, no offset and no proof; it is what the repository said when it was asked, as a proxied file is. This is what "cached HTML" means, and it is what most tenants will run. The `view.foreign_page` row that indexes a proxied bundle is in [03](03-storage-caches-and-search.md) §4.12.

### 2.5 Mirror mode: the `pages/{repo}` partition and its record

*Sources: [0053](../decisions/0053-mirrored-pages.md) §5; [0069](../decisions/0069-synchronized-talk-pages.md) §1, §2.*

**With `mode = mirror`, the bundles of a chosen set of pages are records in an instance partition,** so that the pages stay readable when the repository is down, are verifiable, and can be exported. The partition is registered in `graphs.toml` beside `files/{repo}` ([02](02-graphs-rdf-and-query.md)):

| Graph | Illustrative IRI | Kind | Scope | Written by | History | Integrity | Export |
|---|---|---|---|---|---|---|---|
| **Mirrored pages**, one per repository | `{farm base}/instance/graph/pages/enwiki` | Source | Instance | The repository's sync job | `latest` (an instance may set `full`) | `hashed` | Public |

**The record** has the payload type **`scatter:v0/mirrored-page`**, keyed by the **upstream page ID**, an identifier and never content ([0006](../decisions/0006-log-integrity-and-erasure.md) §3). Header field 9 carries the provider-ranged page ID and field 7 the ranged revision ID (§1.6), so a mirrored page's API identity is the same in both modes. It declares five parts ([0015](../decisions/0015-record-format-and-partition-registry.md) §1, in [01](01-log-and-records.md)):

| # | Part | Holds | RevisionDelete bit |
|---|---|---|---|
| 0 | Content | The bundle's identity and metadata (§2.1): title, revision, timestamp, model, categories, templates, images, interlanguage links, page properties, redirect target | — |
| 1 | Comment | The upstream edit summary | comment |
| 2 | Attestation | The upstream actor under the repository's issuer ([0007](../decisions/0007-actor-identity.md) §1, §5), and the sync job | user |
| 3 | Wikitext | The source | text |
| 4 | HTML | The rewritten HTML, compressed | text |

Two parts for the two forms, so that a takedown can erase the rendering and keep the source, or the reverse, and so that `full` history need not keep every rendering.

**A followed talk page** (§5.2) is mirrored here whatever the repository's `mode`. Its content part gains `threads`, the mapping of §5.4 from which foreign threads with stable IDs are derived; its comment authors go in the attestation part, and a `put` lists the threads it changed. Archive subpages are mirrored when a thread is found to have moved to one.

**Operations** follow a mirror graph's ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.2, in [05](05-providers-and-ingest.md)): **`put`** writes the bundle and is skipped when the upstream revision is not newer; a **`put`** with a new title records an upstream move, since MediaWiki keeps the page ID across moves; **`tombstone`** records an upstream deletion, and compaction erases the page. Upstream hiding of a revision's text, summary or user is followed with an `erase` of the part, reason class `upstream` ([0011](../decisions/0011-logs.md) §5, in [16](16-logs-feeds-and-notifications.md)); a suppressed page is tombstoned.

### 2.6 Which pages are mirrored; syncing, reads and export

*Sources: [0053](../decisions/0053-mirrored-pages.md) §5.*

**Which pages are mirrored** is the repository's `mirror.set`:

| Set | Mirrors |
|---|---|
| `on-demand` *(default)* | A page the first time it is read or transcluded on any tenant that lists the repository, kept while it is used and tombstoned after `mirror.grace` (default 90 days) without a read, as `files/{repo}` keeps the files that are used |
| `linked` | Every title linked from a local page of a listing tenant, to a depth of `mirror.depth` (default 1), with the templates and modules those pages use; refreshed as the links projection changes |
| `all` | Every page in the served namespaces, bootstrapped from a dump: the Enterprise HTML dump where credentials exist, which carries Parsoid HTML for every article, or the `pages-articles` dump with HTML fetched per page afterwards |

**Syncing** is an instance job per repository, run by the instance ([0040](../decisions/0040-instance-prerogatives.md) §6, in [08](08-tenants-and-instances.md)), following the repository's events (§2.7) for pages in the set and writing a `put` per changed page; without events it polls `list=recentchanges`. `Special:Providers` shows its lag.

**Reads** come from `view.foreign_page` ([03](03-storage-caches-and-search.md) §4.12), which the partition projects; the HTML part is served into the frame exactly as a proxied bundle is, through the same `fp:` cache key. A tenant reading a repository in `mirror` mode never fetches from the repository itself; a page outside the set under `on-demand` is fetched once, by the sync job, and is thereafter a record.

**Export.** `pages/{repo}` is public, so `Special:Export` ([0047](../decisions/0047-special-pages.md) §8, in [21](21-special-pages.md)) can write its pages in MediaWiki XML, latest revision each, marked as foreign; the `records` format carries them with proofs, which is something a copy of Wikipedia has never had.

### 2.7 Following the repository: events and invalidation

*Sources: [0053](../decisions/0053-mirrored-pages.md) §6.*

**A repository with `events` is followed; one without is polled or waits for its TTL.** The `events` field of a `page-repo` entry names the repository's EventStreams endpoint, and the instance follows one stream per repository, `mediawiki.page-change.v1` filtered by wiki and the served namespaces, which carries creates, edits, moves, deletes, undeletes and visibility changes; for an `enterprise` source the Realtime API's article stream is the same thing. Each event:

1. updates the title index (§2.3);
2. in `proxy` mode, purges the page's `fp:` key and its `Cache-Tag`, so the next read refetches; in `mirror` mode, queues a `put` for a page in the set;
3. for a Template or Module page, bumps the render epoch of every local page whose manifest names the repository and the title ([0042](../decisions/0042-template-expansion-and-parsoid.md) §10, in [11](11-rendering-templates-and-modules.md)), so a local page that uses a Wikipedia template re-renders when the template changes, without waiting for `repo.cache_ttl`;
4. for a `page-move` or `page-delete`, queues a refresh of the local pages that link to the title, so their link classes follow.

Events are debounced per page over `repo.debounce` (default 30 seconds), because a busy article is edited many times a minute. The stream position is kept in `ops.repo_cursor` and resumed with `Last-Event-ID`; a gap longer than the stream's retention is closed by paging `list=recentchanges`, as [0011](../decisions/0011-logs.md) §4 does.

`action=purge` on a foreign title drops its `fp:` entry and, in `mirror` mode, queues a `put`, under the `parse` rate class.

### 2.8 Attribution and licences

*Sources: [0053](../decisions/0053-mirrored-pages.md) §9.*

**Every foreign page says where it is from.** The identity line (§1.5) names the repository, the revision and the licence, and the foot of the page carries the attribution line reused wiki text requires: "From English Wikipedia, revision 1234567890 (30 September 2026), licensed CC BY-SA 4.0; the authors are listed in its history", with each a link. The HTML carries `<link rel="license">` and the `origin` field is in every API response ([18](18-api.md)). This is the attribution rule of 0042 §11 and 0039 §11, applied to whole pages.

**The repository's licence must be one the tenant can carry.** A tenant whose own content licence (`site` setting `content.licence`) is incompatible with a repository's is refused when it adds the repository to `pages.repos`, with `ts-licence-incompatible`: CC BY-SA text cannot be served under a CC0 wiki's terms. A tenant with no licence set is warned, not refused.

### 2.9 Hiding and takedowns

*Sources: [0053](../decisions/0053-mirrored-pages.md) §9.*

**Hiding a foreign page here** is the Delete action, held by an administrator (`delete`), which writes a `read` ACL on its ranged page ID ([0023](../decisions/0023-moderation.md) §2, in [09](09-security-and-moderation.md)) as deleting a local page does, and removes it from every stack on that tenant; nothing is written to the repository and nothing in the mirror partition changes. **An operator takedown** of a mirrored page is an `erase` of its parts in `pages/{repo}` with the instance's authority ([0040](../decisions/0040-instance-prerogatives.md), in [08](08-tenants-and-instances.md)), as a file takedown is ([0039](../decisions/0039-files-and-media.md) §10, in [12](12-files-and-media.md)). Upstream deletions and hidings are followed in both modes (§2.5, §2.7).

## 3. Forking a mirrored page

*Sources: [0054](../decisions/0054-forking-a-mirrored-page.md) §1, §2, §3, §4, §6, §7, §9; [0033](../decisions/0033-backend-stack.md) §10.*

### 3.1 A fork is a local page whose first record names its origin

*Sources: [0054](../decisions/0054-forking-a-mirrored-page.md) §1; [0069](../decisions/0069-synchronized-talk-pages.md) §7.*

**A fork is an ordinary local page in the tenant's `pages` partition,** with a page ID from the tenant's sequence, a `wikitext` model and the full history of any local page. What makes it a fork is its first record: a **`create`** whose content part carries **`forked_from`**:

```json
{"op":"create","title":"Myocardial infarction","model":"wikitext",
 "forked_from":{"repo":"enwiki","page_id":19927,"revid":1234567890,"time":"2026-09-30T14:02:11Z","talk":"follow"}}
```

- **Its text is the upstream revision's wikitext**, the one the editor was shown (§2.1), so the fork's base is exactly what was edited and the editor's change has a diff.
- **Its actor is the upstream revision's author,** resolved under the repository's issuer (§1.1, [0007](../decisions/0007-actor-identity.md) §5): a registered user by numeric ID, an IP edit as an `anonymous` surrogate, a hidden user as a marker. Its summary is the upstream summary; its upstream timestamp is kept as upstream metadata, as [0008](../decisions/0008-namespaces-and-document-pages.md) §9 keeps one. The record's own time is the time of the fork, and its attestation names the forking user and the fork job (§3.4), so the attribution is the upstream author's and the responsibility is the forker's, as for an import.
- **Its `talk` says what became of the talk page:** `follow`, the fork's talk page keeps following the repository's (§5.11), or `fork`, it was converted. The person chooses when forking; a later change is a `follow` page operation, a null revision (§5.11).
- **It is tagged `fork`** and projects as **`import/interwiki`** in the log ([0011](../decisions/0011-logs.md) §6.1; [16](16-logs-feeds-and-notifications.md)), with the repository as the interwiki and the upstream revision in its parameters, as an adopted entity projects as `import/upload` ([0035](../decisions/0035-adopting-a-wikibase.md) §6). The activity row is an `edit` with `new` set.

**The forker's own edit follows as the second record,** an `edit` whose base offset is the `create`'s, in the same transaction (§3.2). A fork made without a change (§3.3, `Special:Fork`) has only the first.

**The stack changes at once.** The title now has a local primary (§1.2); the repository's page is an alternate, offered or hidden as the repository and the page say. A fork tracks nothing: the repository's later revisions do not reach it, and nothing here ever writes to the repository. That is the direction, and §3.7 is how a fork sees what it has diverged from.

### 3.2 Forking on save

*Sources: [0054](../decisions/0054-forking-a-mirrored-page.md) §2; [0069](../decisions/0069-synchronized-talk-pages.md) §7.*

**The Edit tab of a foreign primary is the fork form.** It opens the source editor on the bundle's wikitext at the upstream revision the reader was shown, with the banner: *"This page is from English Wikipedia. Saving creates a local copy that this wiki owns. Later changes on English Wikipedia will not be applied to it automatically."* Section edit links on a mirrored page (§2.2) open the same form at the section. Preview runs through `action=parse` with the tenant's own expander ([0042](../decisions/0042-template-expansion-and-parsoid.md) §15), so the editor sees how the page will render **here**, over the tenant's stacks, before committing. The fork notice carries the talk-page choice of §5.11.

**Saving appends the `create` of §3.1 and the editor's `edit`, in one transaction with one base check:** the base is the upstream revision named in the form, and the save is refused with `editconflict` if the repository's page has moved on since the form was opened, exactly as a local edit conflicts, with the editor's text kept. On success the response reports the new page ID, the two revision IDs and the job (§3.4).

**The API forks only when asked.** `action=edit` on a title whose primary is foreign is refused with **`ts-fork-required`** unless the request carries **`tsfork=1`**, with **`tsforkrevid`** naming the upstream revision edited (default: the bundle's current). A bot that saves to inherited titles without knowing they are inherited therefore learns it at once rather than making a fork by accident; `nocreate` is refused the same way, since the title has no local page. With `tsfork=1`, `section=N` edits the bundle's section, `appendtext` and `prependtext` apply to the bundle's text, and `createonly` is satisfied. `Special:Fork` and `tsfork=1` take `tstalk=follow|fork`. `tsfork` is exposed to edit filters as `fork` in the page context ([0030](../decisions/0030-edit-filters.md) §2, in [09](09-security-and-moderation.md)), with the repository and upstream revision beside it, so a tenant can gate or tag forks. The REST route `POST /page/fork` is in [18](18-api.md); the permissions, `fork.allow` and the `fork` rate class are in [09](09-security-and-moderation.md) §8.8.

### 3.3 `Special:Fork`, and forking with expansion off

*Sources: [0054](../decisions/0054-forking-a-mirrored-page.md) §2.*

**`Special:Fork/{title}` forks without a change.** It is a form for the reader who wants a page kept as it is, protected from upstream, or ready for annotation: it shows the stack, the licence and the options of §3.6 and §5.11, and appends the `create` alone. The same options are collapsed under "Fork options" on the edit form, with the tenant's defaults. The page is in `docs/registry/special-pages.toml`, origin `triplespace`, group `pagetools`, restricted to `createpage` ([21](21-special-pages.md)).

**With expansion off,** the `create`'s text is the **flattened** text, fetched from the repository with `action=expandtemplates` as the import of [0008](../decisions/0008-namespaces-and-document-pages.md) §9 step 3 flattens it ([10](10-pages-and-content-models.md)), tagged `flattened`, because an unexpanded Wikipedia article on a tenant without expansion is a page of chips. With `wikitext.expansion = on` the text is the raw source and the dependencies of §3.6 make it render.

### 3.4 History is seeded afterwards

*Sources: [0054](../decisions/0054-forking-a-mirrored-page.md) §3.*

**The save never waits for history.** A Wikipedia article can have twenty thousand revisions, and the log cannot take them behind the page's `create` once it is appended ([0035](../decisions/0035-adopting-a-wikibase.md) §7, in [05](05-providers-and-ingest.md)). So the fork's history is **shape (b)** of 0035 §7, applied to a page: a job writes the upstream revisions as **upstream revision records** ([0015](../decisions/0015-record-format-and-partition-registry.md) §4, in [01](01-log-and-records.md)) into the tenant's `log` partition, keyed by the fork's page ID, and the history view folds them beneath the `create`.

**The record** is `scatter:v0/upstream-revision` as 0015 §4 defines it, with one addition: **a fourth part, `text`**, holding the revision's wikitext, so that hiding or erasing an old revision's content ([0011](../decisions/0011-logs.md) §5) touches the text and nothing else, as a post's text part is kept apart from its structure ([0019](../decisions/0019-discussions.md) §4). The content part carries the upstream revision and parent IDs, timestamp, size, SHA-1, model, tags and flags; the comment part the summary; the attestation the upstream actor under the repository's issuer, or a hidden marker, and the job. Header field 7 is the **provider-ranged** revision ID, `number << 40 | upstream revision ID` ([0013](../decisions/0013-postgres-storage.md) §6), so `oldid=` with that number serves the revision and no local revision ever shares it.

**The source is `fork.history_source`,** a `site` setting:

| Value | Reads |
|---|---|
| `chest:{path}` | A RevisionChest store for the repository, its `.mwrev.zst` files and index, through the reader in `scatter-adapter-mediawiki` ([0053](../decisions/0053-mirrored-pages.md) §12, in [22](22-crates-and-stack.md)); then the repository's API for revisions newer than the store, as a `dump:` backfill closes its gap ([0015](../decisions/0015-record-format-and-partition-registry.md) §4). This is the setting James intends for Wikipedia, where the Internet Archive's store exists |
| `api` *(default)* | The repository's `prop=revisions` with `rvprop=ids\|timestamp\|user\|userid\|comment\|size\|sha1\|tags\|flags\|content`, paged, under the repository's fetch budget (§2.1) |

**`fork.max_revisions`** (`site`, default 20,000) caps what is seeded with content. Beyond it the oldest revisions are seeded as metadata only, so the history is complete and diffable for the recent past and complete as a list for all of it; the History tab says so and links upstream for the rest.

**Serving.** `view.upstream_revision` ([0015](../decisions/0015-record-format-and-partition-registry.md) §4) gains rows whose `entity_id` is the page ID in decimal, as page statements use it ([0038](../decisions/0038-page-metadata-and-categories.md) §1). The History tab lists them beneath the `create`, each with the upstream actor linked to its IRI, under a fold "Imported from English Wikipedia" like the one for upstream edits ([0010](../decisions/0010-site-ui.md) §5.5), but stored. `prop=revisions` lists them with ranged IDs and serves their text; `action=compare` diffs any two revisions, upstream or local; `Special:Export` with full history writes them. While the job runs, the History tab shows "History is being seeded: 4,120 of 18,377 revisions", and the page is fully usable. The `view.fork` row that tracks the job's state is in [03](03-storage-caches-and-search.md) §4.12.

### 3.5 RevisionChest

*Sources: [0033](../decisions/0033-backend-stack.md) §10; [0054](../decisions/0054-forking-a-mirrored-page.md) §3.*

**RevisionChest runs as a separate binary** to turn Wikidata's XML history dumps into its `.mwrev.zst` revision files with an index (SQLite, Postgres or Parquet). It has no library target, so Triplespace does not link it. Its licence (GPL-3.0) is compatible with Triplespace's; keeping it a separate process is an architectural choice (fixed point 4), not a licensing one. `scatter-adapter-mediawiki` contains an **independent reader** for the `.mwrev.zst` format and its index, which the Wikidata adapter uses ([0053](../decisions/0053-mirrored-pages.md) §12). Reading a file format does not make the reader a derivative work. RevisionChest reads any MediaWiki wiki's dumps, and a store of a Wikipedia's history is what seeds a fork's revisions (§3.4).

A RevisionChest store is accepted wherever 0015 §4 accepts `dump:{path}` as a backfill source.

**Live upstream sync stays in Triplespace** ([0011](../decisions/0011-logs.md) §4, in [16](16-logs-feeds-and-notifications.md)): EventStreams, the logging dump and `list=logevents`. It is built so that it can later be contributed to RevisionChest. The reader and sync code must handle what RevisionChest's current `sync` does not, observed in its source on 2026-09-27:

- a revision whose content is hidden upstream is written as empty text, which would read as an empty entity; Triplespace treats hidden content as hidden ([0011](../decisions/0011-logs.md) §5);
- log events (deletion, suppression, revision visibility) are not fetched;
- no `maxlag`, and no resume beyond the 30-day recent-changes window;
- the sync path's index `offset_begin` appears to add a compressed file offset to an uncompressed header length; the reader locates revisions by zstd frame start, not by that offset, until this is confirmed.

Contributing upstream will need a library split in RevisionChest. The licences are compatible in both directions. Code moving from RevisionChest into the Wikidata adapter still needs the Internet Archive's agreement, because `scatter-*` crates are dual-licensed and Scatter must hold copyright in them ([0005](../decisions/0005-crate-organization.md) §6, in [22](22-crates-and-stack.md)). Code Triplespace contributes to RevisionChest needs no agreement.

If a RevisionChest store is used as the local source for upstream history ([0010](../decisions/0010-site-ui.md) §8's "fetch upstream history", or `fork.history_source = chest:` for pages), it must receive the same hiding sweep as the log ([0011](../decisions/0011-logs.md) §5), read for a page repository from its `revision-visibility-change` events and deletion log, and a revision the sweep hides is erased from the tenant's `log` with reason class `upstream`.

### 3.6 Dependencies: templates, modules and stylesheets

*Sources: [0054](../decisions/0054-forking-a-mirrored-page.md) §4.*

**With `wikitext.expansion = on`, the pages a fork needs to render come with it,** so that the fork renders here with the templates it was written against. The fork job reads the bundle's list of templates used (§2.1), which MediaWiki computes transitively (`prop=templates`): every Template, Module and TemplateStyles page (`Template:X/styles.css`, [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §2) the article's render transcluded. For each title:

- **if a local page exists under it, nothing happens:** the fork uses it, as it would have through the stack (§1.3), and a later fork of another article reuses the templates the first fork brought;
- **otherwise it is forked as §3.1 forks a page:** a `create` carrying `forked_from` with the repository's current revision of the template, attributed upstream, tagged `fork-dependency`, with its history seeded by the same job (§3.4). Module pages need `wikitext.lua = on` ([0043](../decisions/0043-lua-modules.md) §2, in [11](11-rendering-templates-and-modules.md)) and are skipped, and reported, without it.

**`fork.dependencies`** (`site`) chooses: **`copy`** (the default, and the direction) or **`inherit`**, which forks the article alone and leaves its templates to the repository, for a tenant that would rather its forks keep rendering with Wikipedia's current templates. **`fork.max_dependencies`** (default 2,000) stops a job that would copy more than that, which a citation-heavy article's module tree can approach; the pages not copied stay inherited, so the fork still renders, and the job reports what it left. This is the one place the design is forgiving by construction: an incomplete copy is not a broken page, because the stack answers for whatever is missing.

**Dependencies are forked once, not per fork.** A second fork whose dependencies are already local copies nothing and reports it. Dependency forks are listed on the fork job's page and on each dependency's identity line ("Forked with *Myocardial infarction*").

**TemplateStyles sheets** are forked with their template, and the fork of a Wikipedia template that uses `<templatestyles>` renders here through [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md). **Site styles** are not a page the repository holds in a form the tenant can fork (`MediaWiki:Common.css` is reserved here, [0008](../decisions/0008-namespaces-and-document-pages.md) §2); a tenant that wants Wikipedia's look supplies its own site-styles page ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §4).

### 3.7 What a fork knows about upstream

*Sources: [0054](../decisions/0054-forking-a-mirrored-page.md) §6.*

- **Provenance.** The fork's first revision node carries `prov:wasDerivedFrom` the upstream revision's IRI, `{article path}Special:Redirect/revision/{revid}` ([0015](../decisions/0015-record-format-and-partition-registry.md) §4), and `pav:importedFrom` the repository's page; `GET /page/{id}/provenance` reports the repository, the upstream page and revision, the job and the dependency forks. The seeded revisions carry the upstream vocabulary of [0001](../decisions/0001-revision-metadata-rdf.md) §1 ([02](02-graphs-rdf-and-query.md)) as any backfilled revision does.
- **The identity line** reads "Forked from English Wikipedia at revision 1234567890 (30 September 2026)", linking both; the **About this page** panel ([0010](../decisions/0010-site-ui.md) §4) shows the revisions imported, the dependencies copied, whether files were copied (§3.8), and **"English Wikipedia has 14 newer revisions"**, from a live `prop=info` on the repository, cached and rate-limited as an upstream fetch is ([0012](../decisions/0012-api-requirements.md) §6).
- **Compare with upstream.** `GET /page/{id}/upstream-diff` *(REST v0)* and a link in the panel diff the fork's **base** (the `create`'s text) against the repository's **current** wikitext, and the fork's current text against it, so an editor can see what upstream changed since the fork and what the fork changed since upstream. The diff is computed live and never stored. Its base is the fork's `create` or, once the fork has pulled, its latest `merge` record (§4.2). **Merge from upstream** applies upstream's changes as a guided three-way merge, and **Propose to the repository** compiles the fork's changes the other way (§4.2–4.3).
- **The attribution line** of §2.8 stays on a fork for as long as the page exists, reading "Forked from English Wikipedia; the original authors are listed in this page's history", which is what the licence asks and what the seeded history provides.

### 3.8 Files, on request

*Sources: [0054](../decisions/0054-forking-a-mirrored-page.md) §7.*

**A fork does not copy files.** Its `[[File:…]]` embeds keep resolving through the tenant's file repositories ([0039](../decisions/0039-files-and-media.md) §11, in [12](12-files-and-media.md)), as they did on the mirrored page.

**"Copy files used by this page"** in the About panel, and `POST /page/{id}/fork/files` *(REST v0)*, start a separate job that imports each file the page's render uses, as the directory import of [0039](../decisions/0039-files-and-media.md) §14 does: the latest version by default, or every version with `history=all`, each as an `upload` record with action `import`, attributed to its upstream uploader under the file repository's issuer, with the file's description page forked as §3.1 forks a page. A file already local, or one the tenant's file repository holds in `mirror` mode, is skipped and reported. The job needs `upload` and `reupload-shared` ([0039](../decisions/0039-files-and-media.md) §21; [09](09-security-and-moderation.md) §8.8), since each copy shadows a foreign file.

### 3.9 Deleting, moving and re-forking

*Sources: [0054](../decisions/0054-forking-a-mirrored-page.md) §9.*

- **Re-following upstream** is deleting a fork that no longer differs from the repository, offered under `fork.refollow = ask` and automated under `auto` (§4.5).
- **Deleting a fork** is a `read` ACL on its page ID ([0023](../decisions/0023-moderation.md) §4). The title's stack loses its local primary and the repository's page is primary again (§1.2): deleting a fork is how a tenant goes back to the mirror. The threads homed on its talk page go with it, by enclosure, and come back with undeletion. A new fork at a deleted fork's title is an ordinary create; the deleted page keeps its history, as a deleted page does.
- **Moving a fork** is an ordinary move and leaves a redirect ([0051](../decisions/0051-page-redirects.md) §3, in [10](10-pages-and-content-models.md)). The old title's redirect is now the local primary there, so a reader arriving by the old name lands on the fork, while the repository's page at that title becomes an alternate of a redirect, which is listed as such.
- **Redirects are not forked.** The repository's redirects to a forked title keep resolving through the stack to the fork ([0051](../decisions/0051-page-redirects.md) §2); a tenant that wants a redirect local creates it.
- **Forking a redirect** forks the redirect page, not its target; the form says so and offers the target.

## 4. Merging with upstream

*Sources: [0068](../decisions/0068-merging-with-upstream.md) §1, §2, §3, §4, §5.*

### 4.1 One three-way merge

*Sources: [0068](../decisions/0068-merging-with-upstream.md) §1.*

**A pure crate, `scatter-merge`** ([22](22-crates-and-stack.md)), implements a line-based three-way merge of wikitext, the `diff3` algorithm with the two-way diff of [0010](../decisions/0010-site-ui.md) §6 as its primitive, returning the merged text and a list of **hunks**: `clean` (one side changed), `identical` (both changed the same way) or **`conflict`** (both changed differently), each with its base, ours and theirs lines. Wikitext is line-oriented enough for this to work as it does for Wikipedia's own edit-conflict resolution; the merge is **text only**, never a parse, so templates and references are lines like any other, and a conflict inside a template call is a conflict.

The three inputs are always: **base** = the text of the fork's `create` record (§3.1), or of the last `merge` record (§4.2) when the fork has pulled before; **ours** = the fork's current text; **theirs** = the repository page's current wikitext, fetched as the upstream diff is (§3.7; [0012](../decisions/0012-api-requirements.md) §6), at a revision the result records.

### 4.2 Pulling: upstream's changes into the fork

*Sources: [0068](../decisions/0068-merging-with-upstream.md) §2.*

**Merge from upstream.** From the About panel or `Special:MergeUpstream/{title}` ([21](21-special-pages.md)), the editor sees the merge of §4.1 as a **guided edit**: clean hunks applied, each conflict shown with base, ours and theirs and three buttons (keep ours, take theirs, edit), and the whole result editable in the ordinary editor with preview before saving. Saving appends an ordinary `edit` record whose content part carries **`merge`** `{repository, upstream_revid, base_revid, conflicts_resolved}`; the summary reads "Merged English Wikipedia revision 1234567890" with the editor's comment. The record is the fork's new **base** for the next merge, so repeated pulls diff against what was last merged rather than against the original fork, and `GET /page/{id}/upstream-diff` (§3.7) reads the base from the latest `merge` record.

**Dependencies** (§3.6): a pull merges the article alone. The panel says when upstream's templates or modules the fork copied have newer revisions, and offers **Update dependencies**, which re-runs §3.6's copy for the listed titles as a job; a dependency a tenant has edited locally is listed, not overwritten, and gets its own `Special:MergeUpstream`.

**Permissions**: `edit` on the fork, as any edit; a pull is subject to edit filters ([0030](../decisions/0030-edit-filters.md), in [09](09-security-and-moderation.md)) and protection like any edit. **Rate**: the upstream fetch counts in `upstream` ([0024](../decisions/0024-subsidiary-accounts.md) §5); the edit in `edit`.

### 4.3 Pushing: the fork's changes as a page proposal

*Sources: [0068](../decisions/0068-merging-with-upstream.md) §3.*

**Propose to {repository}** on a fork compiles the other direction: base→ours applied onto theirs, by the same merge. The result is the **payload** of a page proposal ([0067](../decisions/0067-proposals.md) §3, in [14](14-discussions.md)): `{ text, base_revid, upstream_revid, destination, conflicts }`. A conflict here means upstream changed a passage the fork also changed; the proposer resolves it in the same guided view before the `propose` record is appended, because an unresolved conflict is not a proposal anyone can apply.

**The destination** within the wiki:

| `destination` | Writes | Default when |
|---|---|---|
| `article` | The destination page itself | The merge is clean and changes at most `proposals.direct_edit_lines` (default 50) lines |
| `talk` | A new section on the destination's talk page carrying the diff and a link to the proposal thread, as Wikipedia's edit-request convention does | Otherwise |
| `draft` | A page under the proposer's user space on the destination wiki, or its `Draft:` namespace where one exists | Chosen |

The proposer may override the default. **Attribution**: the push's edit summary links the fork's history ("Merged from {fork URL}; see its history for authors"), which is the "copied from" convention CC BY-SA asks for, and the proposal thread records that the summary was written. **Licence**: a push is refused with `ts-licence-incompatible` when the tenant's `content.licence` cannot be carried onto the repository's, §2.8's check with the roles reversed, and warned when the tenant has no licence set.

### 4.4 Export and push

*Sources: [0068](../decisions/0068-merging-with-upstream.md) §4; [0069](../decisions/0069-synchronized-talk-pages.md) §7.*

**Export** ([0067](../decisions/0067-proposals.md) §4, in [17](17-federation-and-publication.md)) for a page offers the merged **wikitext** (copy or download) and the **diff** against upstream's current text as unified diff or as the two-column view, with instructions for the destination (paste into the article's editor; post on talk; create the draft). **Push** ([0067](../decisions/0067-proposals.md) §6, the later phase, for page proposals only) performs the destination's edit as the person with the summary above.

**Proposals meet discussion.** A page proposal whose destination is `talk` is a new upstream section; on a fork that follows its talk page (§5.11), it is sent now by the new-thread mechanism of §5.10, gated by that mechanism's switch (`talk.upstream_post`) and not by `proposals.push`, and appears among the foreign threads when synced, where the discussion of the proposal happens on the repository and is read here.

### 4.5 Adoption, and re-following upstream

*Sources: [0068](../decisions/0068-merging-with-upstream.md) §5; [0054](../decisions/0054-forking-a-mirrored-page.md) §9.*

**A page proposal is adopted** when a later upstream revision, merged against the fork's base, leaves **no difference** between ours and theirs for the proposed hunks: upstream now says what the fork says there, whether the push was applied as-is or an editor there rewrote it to the same effect. The page-mirror projection (§2.7) runs the test when the destination page changes, and [0067](../decisions/0067-proposals.md) §5's state machine ([14](14-discussions.md)) takes it from there.

**Re-following upstream.** A fork that no longer differs from upstream, because its changes were adopted, or because its editors pulled everything and changed nothing, may stop being a fork. Deleting it restores the mirror as primary (§3.9); that is a decision with a setting:

| `fork.refollow` (site) | Behaviour |
|---|---|
| `ask` (default) | The About panel and the proposal thread offer **Re-follow upstream**, which deletes the fork with the summary "Re-following {repository}: no local changes remain" and links the fork's history from the mirror's talk page |
| `auto` | A fork whose text equals upstream's current text for `fork.refollow_after` (default 30 days) is deleted by a job, as an instance act ([0040](../decisions/0040-instance-prerogatives.md) §7), with the same summary and a notification to the fork's editors |
| `never` | The offer is not shown |

A re-followed fork's history is kept, as a deleted page's is; **Re-fork** from the mirror creates a new fork with a new `create`, and the old history is linked from the About panel as "previously forked, re-followed on {date}".

## 5. Synchronized talk pages

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §1, §2, §3, §5, §6, §7; [0054](../decisions/0054-forking-a-mirrored-page.md) §5.*

### 5.1 The `talk` field

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §1.*

**A page repository has a field, `talk`** (§1.1):

| `talk` | Behaviour |
|---|---|
| `link` *(default)* | A foreign page's talk page holds local threads, and links to the repository's talk page (§1.7) |
| `sync` | The repository's talk page for each served subject namespace is **followed**: its sections appear on the local talk page as **foreign threads** (§5.3), beside local ones (§5.6) |

The talk namespaces followed are the pairs of the entry's `namespaces`: `Talk` for `main`, `Template talk` for `Template`, and so on. A talk namespace is not listed in `namespaces` itself, because it is never served as pages: its pages are only ever shown as threads. Pinned threads, which a followed talk page's converted front matter uses, are in [14](14-discussions.md).

### 5.2 A followed talk page is always mirrored

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §1.*

**A followed talk page is always mirrored,** whatever the repository's `mode`. Threads need identities that last (§5.4), which a proxied bundle cannot give, so the talk page's bundle is a `scatter:v0/mirrored-page` record in `pages/{repo}` (§2.5) even where the articles are proxied. Which talk pages are mirrored follows `mirror.set = on-demand`: a talk page is mirrored the first time its local talk page is read, and also whenever something depends on it (a fork that follows it, §5.11; a watch, [16](16-logs-feeds-and-notifications.md); a post sent upstream, §5.8), and it is kept while any of these holds or for `mirror.grace` after the last read. It follows the repository's events like any mirrored page (§2.7), under the same debounce.

**Archive subpages are followed lazily.** `Talk:X/Archive 7` is mirrored when a thread is found to have moved there (§5.5) and when a reader opens the talk page's archived threads.

### 5.3 Foreign threads

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §2.*

**A foreign thread is one level-two section of a followed talk page,** with section 0, the text above the first heading, as one more ([14](14-discussions.md)). Its structure comes from the repository:

| Repository | Sections | Comments |
|---|---|---|
| Has DiscussionTools (every Wikimedia wiki) | `action=discussiontoolspageinfo&prop=threaditemshtml` at the bundle's revision: each heading with its **name** (`h-Example-20190412103000-…`) and each signed comment with its name, author, timestamp and parent | Yes: the thread is a tree of comments, as on the wiki |
| Does not | The splitter of §5.12: level-two headings, by position | No: the section is one block, replied to at its end |

DiscussionTools' names are what make the identity last. A heading's name is built from its first comment's author and timestamp, not its text or position, so it survives a retitled heading, sections above it being added or removed, and **an archive bot moving it to a subpage**: DiscussionTools' own permalinks rely on this.

### 5.4 Numbers, page IDs and the mapping in the record

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §2; [0053](../decisions/0053-mirrored-pages.md) §5.*

**Each foreign thread gets a number,** `n`, allocated per repository by the sync job the first time it sees the thread's name, and a **provider-ranged page ID** (§1.6) in the half of the range that upstream page IDs never reach:

> foreign thread page ID = `number << 40 | 1 << 39 | n`

so a foreign thread can be watched, linked with `Special:Redirect/page/{id}`, counted and searched like a local one, and its ID is the same on every tenant that lists the repository. A repository without DiscussionTools names its sections by position within a revision; the sync job carries `n` forward across revisions by matching heading text and position, and a section it cannot match is a new thread. Those identities are best effort, and the UI says so on such a repository.

**The mirrored-page record carries the mapping, never the names.** The talk page's `put` gains, in its content part, `threads`: for each section, `n`, the SHA-256 of the DiscussionTools name truncated to 16 bytes, the section index, and for each comment the hash of its name and of its parent's. The comment authors, resolved under the repository's issuer ([0007](../decisions/0007-actor-identity.md) §5), go in the **attestation part** beside the revision's actor, so upstream user-hiding erases them with it. The heading text and the comments' text are read from the wikitext and HTML parts, so erasing those parts erases them. A rebuild reproduces every `n` from the records, and a hash, not a name, keys the lookup, because names contain usernames ([0006](../decisions/0006-log-integrity-and-erasure.md) §3). The `view.foreign_thread` table is in [03](03-storage-caches-and-search.md) §4.8.

### 5.5 State, rendering and addressing

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §2.*

**State, followed from upstream:**

| State | When | Shown as |
|---|---|---|
| `open` | The section is on the talk page | Status **Open** |
| `archived` | It left the talk page and was found, by name, on an archive subpage the same actor edited within `talk.archive_window` (default ten minutes) of removing it, which is how archive bots and one-click archivers work | Status **Archived** ([0019](../decisions/0019-discussions.md) §6), collapsed by the default rule |
| `removed` | It left the talk page and was not found | Hidden by default, with "Removed from the talk page on {date}" and a link to the last upstream revision that had it |

A removed thread that reappears, by name, is `open` again. Status here is upstream's: a foreign thread's status changes only when upstream does, and nobody sets one locally.

**Rendering.** A foreign thread is rendered from the talk page's HTML part, section by section, as §2.2 rewrites it: upstream's signatures, indentation and templates as upstream renders them, inside the local thread frame, with the origin chip of §1.5 ("On English Wikipedia") and, where DiscussionTools marks the comments, a **Reply** control on each (§5.7). It has no `Thread:` title, since its title belongs upstream: its address is the local talk title with DiscussionTools' anchor, `Talk:Myocardial infarction#h-Example-20190412103000`, which resolves through the stack, and `GET /thread/{id}` serves it read-only with `origin` ([18](18-api.md)).

### 5.6 The talk page holds both

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §3.*

**A talk page on a followed subject is the union** of the local threads attached to it ([0019](../decisions/0019-discussions.md) §2, [0049](../decisions/0049-boards.md) §5, in [14](14-discussions.md)) and the foreign threads of the upstream talk page it follows:

| Local talk page | Follows |
|---|---|
| A foreign page's, on its ranged page ID (§1.7) | The repository's talk page for that page, under `talk = sync` |
| A fork's, following upstream (§5.11) | The repository's talk page for the page it was forked from, by upstream page ID, so a local move of the fork changes nothing |
| Any other | Nothing |

**0019 §2's "foreign talk pages are never loaded" reads as it was meant:** a mirrored *entity's* talk page holds local threads only, and a provider tenant's threads are never imported ([0028](../decisions/0028-tenancy-policy.md) §6 stands). A followed *page* repository's talk page is shown, because a page repository is something the tenant chose to present as its own reading, and its discussion is part of that page.

**Order.** Pinned threads first ([14](14-discussions.md)), then every other thread in one sequence: a local thread by the time it was attached here ([0049](../decisions/0049-boards.md) §8), a foreign thread by its first comment's timestamp, or, without DiscussionTools, the time the sync first saw it. A **filter** above the list offers All, Here and On {repository}. Each thread shows its origin; local threads carry no chip, as local pages carry none.

**Visibility** ([0019](../decisions/0019-discussions.md) §6) applies to both, by status category and age, so an archived Wikipedia section collapses as a resolved local thread does.

### 5.7 Replying upstream: the grant and the composer

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §5.*

**A person holding an upstream grant for the repository can reply to a foreign thread as though on the wiki.** The grant is [0067](../decisions/0067-proposals.md) §6's: a second authorization, separate from login, stored in `private.upstream_grant`, through the consumer the operator registered with the repository ([07](07-actors-and-accounts.md)). Posting needs the `editpage` grant, which 0067 §6 already requests. This is **the first use of that client**, so it is built with synchronized talk pages, not in 0067's later phase; `proposals.push` keeps its own switch.

- **A Wikimedia login is not a grant,** as 0067 §6 decided, but it makes one a click: the authorization page is on the same identity issuer the person just used ([0007](../decisions/0007-actor-identity.md) §3), already signed in. A person who logs in by password, or another issuer, can still connect an account on the repository the same way. The first Reply offers "Connect your English Wikipedia account to reply", and the composer then shows **"Posting as Example on English Wikipedia"**. The grant names the upstream account it acts as; a person may hold one per wiki.
- **Registered local accounts only.** A temporary account cannot hold a grant.

**The reply is written in wikitext,** the repository's language, in a composer that shows a preview rendered by the repository (`action=discussiontoolspreview`, or `action=parse` with the talk title as context), counted in the `upstream` rate class ([0024](../decisions/0024-subsidiary-accounts.md) §5). A local reply is markdown ([0019](../decisions/0019-discussions.md) §5); an upstream reply is not stored here as a post at all (§5.9), so no conversion is needed or attempted.

### 5.8 Sending

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §5.*

**Sending.** The instance calls, as the person:

| Repository | Reply to a comment | Reply to a section with no comments, or on a repository without DiscussionTools |
|---|---|---|
| DiscussionTools | `action=discussiontoolsedit&paction=addcomment&commentname={name}&wikitext=…`, which indents and signs as DiscussionTools does on the wiki | `paction=addcomment` with the heading's name |
| No DiscussionTools | — | `action=edit&section={index}&appendtext=` the text indented one level after the section's last line, with `~~~~` and `baserevid` the bundle's revision, after checking the section's heading at that index still matches |

Upstream decides everything upstream decides: protection, blocks, rate limits, CAPTCHAs, AbuseFilter. An upstream warning (an AbuseFilter `warn`, a CAPTCHA) is shown in the composer and the person may confirm and resend; a refusal is shown with upstream's message. The edit summary is DiscussionTools' own, and the repository tags the edit with the consumer's tag, which is how a Wikimedia wiki marks edits made through an OAuth tool.

**Local rules apply first.** The person must be able to post on the local talk page: `edit` on it, not blocked, the page not protected against them ([0016](../decisions/0016-permissions-and-access-control.md) §4, in [09](09-security-and-moderation.md)). **Edit filters** ([0030](../decisions/0030-edit-filters.md); [09](09-security-and-moderation.md) §7) see the outbound text in the text context with `action = upstream-post` and the variable `upstream_repo`, so a tenant can refuse to be a conduit for something its own wiki would refuse; a `disallow` stops the send and writes the filter-hit record as for any write.

### 5.9 What is kept, attribution, and replies that go nowhere else

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §5.*

**Nothing is written to the log for the send.** The reply is the person's speech on the repository, attributed there to their account there. It comes back here by syndication, in the talk page's next `put`, as an upstream revision by an upstream actor, which is exactly what it is. A local copy would be a second, divergent source of the same speech. What the instance keeps is operational: a row in **`ops.upstream_post`** ([03](03-storage-caches-and-search.md) §4.16) that tracks the send from `queued` to `sent`, with the upstream revision ID, to `synced` when a `put` at or after that revision lands, or `failed`. After a successful send the instance queues a fetch of the talk page at once rather than waiting for the event stream. Until it is synced, the reply shows **to its author only**, in place, marked "Sent to English Wikipedia; appearing here shortly". The text is dropped from the row when it is synced or has failed for seven days; the row itself, linking the local account to the upstream revision, is kept for `talk.upstream_audit_days` (default 90) for abuse handling, as IP addresses are kept ([0007](../decisions/0007-actor-identity.md) §3), and never shown publicly.

**Attribution follows.** The synced comment is by the upstream actor. When the person has linked that account publicly ([0007](../decisions/0007-actor-identity.md) §7, in [07](07-actors-and-accounts.md)), it carries "also {local name} here", as every linked account's contributions do; when they have not, nothing connects the two in public, and the grant does not change that.

**Replies to foreign threads go upstream or nowhere.** A foreign thread holds upstream's comments only; a local reply inside it would make a thread that is half here and half there, which neither side could read whole. A person without a grant sees **Discuss here instead**, which opens the new-thread form (§5.10) set to Here, quoting the thread's link.

### 5.10 Starting a thread: here or upstream

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §6.*

**The new-thread form on a talk page that follows upstream has a destination:**

| Destination | Writes | Composer |
|---|---|---|
| **Here only** | A local thread homed on this talk page: a `create` record ([0019](../decisions/0019-discussions.md) §1), with listings as usual | Markdown |
| **{Repository}** | A new section on the repository's talk page, by `action=discussiontoolsedit&paction=addtopic&sectiontitle=…&wikitext=…` or `action=edit&section=new` with `~~~~` on a repository without DiscussionTools, as the person, under §5.7–5.8's rules | Wikitext |

The destination is named in the form's submit button ("Post to English Wikipedia"), never implied. **`talk.new_thread_default`** (`site`) chooses which is selected when the form opens, `here` by default, and a viewer's choice is remembered in the preference `threads.destination` ([0027](../decisions/0027-preferences-and-portability.md) §1, in [07](07-actors-and-accounts.md)). Upstream is offered only to a person who holds a grant, or offers to connect one. A thread sent upstream is a foreign thread once synced, with the same `ops.upstream_post` lifecycle as a reply; a thread kept here is local and stays local.

### 5.11 Forks: following the talk page or forking it

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §7; [0054](../decisions/0054-forking-a-mirrored-page.md) §5.*

**When a page is forked, the person chooses what happens to its talk page.** The fork notice of §3.2 carries one choice:

| Choice | The fork's talk page |
|---|---|
| **Keep following** the talk page on {repository} *(selected by default)* | Follows the upstream talk page (§5.6): upstream's sections appear as foreign threads, replies and new sections can go upstream, and local threads sit beside them. Nothing is converted |
| **Fork the talk page** | The conversion of §5.12: upstream's sections and archives become closed local threads, and nothing more arrives from upstream |

**Following is the default because it is the reversible choice.** A fork that follows can stop at any time, and stopping performs the conversion then; a talk page forked at the start cannot be un-forked without leaving the converted threads beside the foreign ones they duplicate. A tenant that only ever forks permanently, as MDWiki does, sets **`fork.talk`** (`site`: `follow` *(default)* or `fork`) to choose the default, and the choice is still offered. On a repository with `talk = link`, only **Fork the talk page** is offered, and the conversion runs at once. `Special:Fork` and `tsfork=1` take `tstalk=follow|fork`.

**The choice is recorded on the fork.** The `create`'s `forked_from` carries `talk: "follow" | "fork"` (§3.1). A later change is a page operation, **`follow`**, in `scatter:v0/page` ([0008](../decisions/0008-namespaces-and-document-pages.md) §4, in [10](10-pages-and-content-models.md)): no text part, a content part `{talk: "follow" | "fork"}`, a summary, and the base offset of the page's latest record. It is a **null revision**, as MediaWiki writes for protection: a history row ("Stopped following the talk page on English Wikipedia") with the text unchanged. It needs `edit` on the fork. The fork's About panel (§3.7) shows the state with **Stop following** or **Follow again**; the route `POST /page/{fork ID}/talk-follow` is in [18](18-api.md).

- **Stopping** appends `follow` with `fork` and runs the conversion of §5.12 as a job, from the talk page as it then stands and its archives: each section becomes a closed local thread homed on the fork's talk page, with `imported_from` now also carrying the section's name hash; foreign front matter becomes a local thread created **pinned** ([14](14-discussions.md)) and `open`, not archived, since banners do not go stale as discussions do. Local threads are untouched.
- **Following again** appends `follow` with `follow`. Foreign threads whose name hash matches an imported thread's `imported_from` are not shown, since their content is already here; newer ones are.

A fork that follows its upstream talk page has every new section as it is written, so a pull (§4.2) has nothing to import; a fork that forked its talk page chose not to have them, and **Follow again** is the way back.

### 5.12 The conversion: sections become closed threads

*Sources: [0054](../decisions/0054-forking-a-mirrored-page.md) §5; [0069](../decisions/0069-synchronized-talk-pages.md) §7.*

**The conversion turns the repository's talk page into threads homed on the fork's talk page.** The fork job fetches `Talk:{title}` at the fork's upstream revision time, and every archive subpage under it (`Talk:{title}/Archive 1` and the rest, found with `list=allpages&apprefix=`), and splits each by its **level-two headings**:

| Piece | Becomes |
|---|---|
| **The text above the first heading**: the frontmatter of banners, WikiProject templates and talk headers | One thread, subject "*{title}*: talk page header", holding that text as its opening post, created **pinned** and **open** ([14](14-discussions.md)), since banners do not go stale as discussions do. The direction says to treat the frontmatter as a section, and this is how |
| **Each `== Section ==`**, with its subsections | One thread whose subject is the heading's text and whose opening post is the whole section's wikitext, signatures, indentation and subsections intact |
| **Each archive subpage** | The same, with each thread's content part naming the archive it came from |

The conversion runs at fork time only when the person chose **Fork the talk page** (§5.11).

**Every other thread is created closed.** The `create` record carries `status: archived`, a default status in `docs/registry/thread-statuses.toml` ([0019](../decisions/0019-discussions.md) §6, in [14](14-discussions.md)): category `closed`, label **Archived**, ordered after Stale. `status` may appear on `create` as on `post`, which says exactly what an import means: the thread arrived already over. Anyone who may post can reopen one.

**Local threads move.** Threads that readers had attached to the foreign page's talk page, on its ranged page ID (§1.7), are moved to the fork's talk page by the job, one `move` record each, so discussion that began before the fork continues on it.

**`Special:Import` uses the same conversion** ([0008](../decisions/0008-namespaces-and-document-pages.md) §9; [10](10-pages-and-content-models.md)) for any talk namespace in an XML export: a wikitext talk page is imported as closed threads on the subject's talk page, split by heading, with the frontmatter as the first. A village pump imported this way lands on a board ([0049](../decisions/0049-boards.md) §1) the importer names.

### 5.13 Imported posts: wikitext, attribution, dates and history

*Sources: [0054](../decisions/0054-forking-a-mirrored-page.md) §5.*

**The post is wikitext.** [0019](../decisions/0019-discussions.md) §5 made posts markdown. The thread payload's text part has a media type: the content part's **`mediaType`** is `text/markdown` by default and **`text/x-wiki`** for a post an import writes. A wikitext post renders through the tenant's wikitext pipeline, with expansion, so `{{WikiProject Medicine}}` and `{{Talk header}}` render through the stack (§1.3) without being forked, and `~~~~` signatures stay the text they were. No editor composes a wikitext post; only an import writes one, and editing such a post keeps its media type. The AS2 profile ([0019](../decisions/0019-discussions.md) §10, in [16](16-logs-feeds-and-notifications.md)) emits it as `as:source`'s `mediaType`.

**Attribution and dates.** A section has many authors, so the opening post is attributed to the forking user and the job, with the content part's **`imported_from`** naming the repository, the talk page, its revision and, for an archive, the subpage. [0019](../decisions/0019-discussions.md) §3 made a thread's title date the date of its `create` record; for an imported thread it is **the date the import supplies**: the earliest signature timestamp the section's text parses, or the talk page revision's time. So `Thread:2019-04-12/Merge proposal` is what the section was, not when it arrived, and the thread's `as:published` is the import time while its identity line reads "originally on English Wikipedia Talk:…, 12 April 2019".

**The talk page's own history comes too.** Its upstream revisions, and each archive's, are seeded as upstream revision records keyed by the **fork's talk page ID**, as §3.4 seeds the article's, so the talk page's composite history ([0019](../decisions/0019-discussions.md) §7) gains rows "upstream revision of Talk:…" with their text at `oldid=`, and nothing of what was said is lost to the splitting.

## 6. Deriving statements from mirrored pages

*Sources: [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §1, §2, §3, §4, §5, §6, §7, §8, §9, §10, §11.*

### 6.1 Derived graphs

*Sources: [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §1.*

**A derived graph holds the statements an extraction source derives from pages.** It is a source graph of the tenant, one per source, named `derived/{source}` ([02](02-graphs-rdf-and-query.md)):

| Graph | Illustrative IRI | Kind | Scope | Written by | History | Integrity | Export |
|---|---|---|---|---|---|---|---|
| **Derived**, one per extraction source | `{base}/graph/derived/familysearch` | Source | Tenant | Only that source's extraction job | `latest` (a tenant may set `full`) | `logged` | Public |

Its records are `scatter:v0/derivation` (§6.3). No editor and no other job writes to it; a correction is a local-graph assertion about the derived statement (§6.6), as for a mirror. Its subjects are local items, mirrored entities, or any entity the tenant can name; its statements resolve with the rest (§6.6).

### 6.2 Extraction sources

*Sources: [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §2; [0073](../decisions/0073-lines-links-and-url-patterns.md) §5.*

**An extraction source is tenant configuration**, a `config` record of kind `extraction` whose code is the source's name ([23](23-configuration-and-registry.md)):

| Field | Meaning |
|---|---|
| `repo` | The page repository the pages come from (§1.1), in `mirror` mode with the pages in its `mirror.set` (§2.6); or `local`, for the tenant's own `pages` |
| `namespaces`, `titles` | Which pages: namespace numbers, and optional title prefixes or patterns |
| `extractors` | Which extractors run, by name and version: `templates` (§7), `lines` (§8) |
| `mappings` | The template mappings this source applies (§7.2) |
| `lines` | The line settings of §8.5 |
| `page_subject` | How the item for the page itself is found or made: a match key built from the page (`{ property = "P…", value = "{url}" }`), its label (`{title}`), and constant statements such as `P31 = research guide` |
| `reference` | The properties of the reference added to every derived statement (§6.5) |
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

**A source runs over every page it selects** when it is created or changed, as a job with progress, as a category mapping does ([0038](../decisions/0038-page-metadata-and-categories.md) §5, in [10](10-pages-and-content-models.md)), and then **follows the repository**: each mirrored-page `put` for a selected page enqueues the page (§6.7).

### 6.3 The derivation record

*Sources: [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §3.*

**One record holds everything one extractor derived from one page revision.** Its payload type is `scatter:v0/derivation`, in `derived/{source}`, keyed by `{page ID}:{extractor}`, where the page ID is the page's provider-ranged ID (§1.6) or its local page ID. It declares four parts:

| # | Part | Holds |
|---|---|---|
| 0 | Content | The page (repository, upstream page ID, revision ID, title), the extractor's name and version, the source configuration's revision, and the **subjects**: for each, the entity ID and the statements derived about it, in Wikibase JSON |
| 1 | Comment | Null; the job's summary is on its log events |
| 2 | Attestation | The extraction job |
| 3 | Evidence | The **mentions**: for each place in the page a statement came from, its line number, heading path, raw wikitext, plain text, the links and template calls in it, and the subject it produced (§6.8) |

Evidence is a part of its own so that it can be erased on its own: it copies page text, which a takedown on the source page (§2.9) must be able to reach without losing the statements.

**Operations.** `put` replaces the previous derivation for the key, and is skipped when the page revision, the extractor version and the configuration revision are all unchanged. A page that leaves the selection, or is deleted or tombstoned upstream, gets a `put` with no subjects. There is no `tombstone`: the key is the page, not an entity, and nothing to retain is lost.

**Each extractor writes its own record.** A page has one derivation per extractor that ran on it, so adding an extractor, or re-running one at a new version, never rewrites another's statements. The `view.derivation` and `view.derivation_subject` tables and the `ops.extraction` queue are in [03](03-storage-caches-and-search.md) §4.12.

### 6.4 Subjects: found by identifier, or created

*Sources: [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §4.*

**A derived subject is found by its identifiers, or created as an ordinary item of the tenant.** The extractors (§7, §8) say, for each subject, its candidate **match keys** in order (an external identifier such as `P9999 = "1804886"`, or a normalized URL), a label, and whether it may be created. The job resolves each subject before writing the derivation:

1. **Found.** The first match key with a row in `view.match_key` ([0013](../decisions/0013-postgres-storage.md) §5.2, in [03](03-storage-caches-and-search.md)), or in `view.identifier` for a property not designated for `match_key`, names the subject. A key that names two different entities is a conflict: the subject is not resolved, and the job reports it with both IDs for a merge.
2. **Found earlier in the same job.** The job keeps a map from match key to the entity it created, so that two pages in one run that mention one collection do not create it twice before the projection has caught up.
3. **Created.** Otherwise the job appends a `create-or-add` ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.5, in [05](05-providers-and-ingest.md)) to the tenant's `local` graph, carrying the first match key as `match` and, as its content, the label in the source's `language` and one statement for each match key. Its `match` makes a concurrent creation by another job an `add`, not a duplicate. The created item is a local item like any other, attributed to the job.

The match key's property must be designated for `match_key` for step 3 to be safe: an undesignated identifier property may find a subject in step 1, through `view.identifier`, but never creates one, and a source whose mappings name an undesignated identifier property as a key to create by is refused at configuration with `ts-extraction-match-key`. `lines.identifiers` (§8.3) follows the same rule.

**A subject may be foreign.** A link that resolves to a Wikidata item, through a sitelink or a URL match pattern, names `WDQ…` as the subject. Its statements go on the foreign entity, which shallow mirroring fetches ([0070](../decisions/0070-shallow-entity-mirroring.md) §2.2, in [05](05-providers-and-ingest.md)).

**The guide page has a subject too**, found or created by `page_subject` (§6.2). Line subjects can then point at it (§8.5) and inherit its facets.

### 6.5 Statement IDs and references

*Sources: [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §5, §6.*

**A derived statement's ID is a name-based UUID of the source name, the subject, the property, the canonical main value and the hash of its qualifiers.** It is not tied to the page or the line:

- the same fact derived from two pages is one statement, with one reference per page;
- a page edited without changing the fact keeps the statement's ID, so a local correction against it (§6.6) survives re-extraction;
- [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.4's rule for sources with no IDs of their own is followed.

**Every derived statement carries a reference to the page revision it came from**, built from the source's `reference` properties: the page URL with `oldid` as reference URL, the revision's timestamp as retrieved, and the guide page's subject as stated-in where it has one. A statement derived from two pages has two references, merged by hash as 0002 §8.5 merges references.

### 6.6 Resolution and corrections

*Sources: [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §7, §8.*

**Derived graphs are sources in the resolved view, between the local graph and the mirrors** ([05](05-providers-and-ingest.md)).

- Statements, references and aliases: union, as for every source.
- Labels, descriptions and rank: the local graph wins wherever it says anything; then the tenant's derived graphs, in the order the `reconcile` entry lists them ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §9, in [04](04-entities-and-identifiers.md)), by default by source name; then the mirrors in provider order.
- Within one derived graph, the contributions of every derivation that names the subject are unioned; for one language, the derivation of the lowest page ID supplies the label, so the choice is stable.

The entity projection finds a subject's derivations through `view.derivation_subject` ([03](03-storage-caches-and-search.md) §4.12). A derivation's `put` re-resolves the subjects it names now and the subjects its previous version named.

**Corrections to derived statements are local-graph assertions, as for mirrored ones**: a rank override, an added statement, a suppression. `Special:Corrections` ([0047](../decisions/0047-special-pages.md) §6, in [21](21-special-pages.md)) lists derived graphs beside mirror graphs, filterable by source. Because statement IDs are stable (§6.5), a suppression of a wrong derived statement stays in force when the page is re-extracted, and is reported as redundant if the page stops yielding the statement.

### 6.7 Following pages

*Sources: [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §9.*

- **A page change** (a mirrored-page `put`, or a local page edit for `repo = local`) enqueues `(source, page)` in `ops.extraction`, for every source that selects the page. The extraction job drains the queue, runs the source's extractors on the page's wikitext, resolves subjects (§6.4) and appends a derivation `put` per extractor.
- **A change to the source's configuration, a mapping it names, or an extractor version** enqueues every page the source selects.
- **Template redirects and renames** in the repository change which mapping a call matches (§7.1). A `put` for a Template page re-enqueues the pages that call it, through the repository's `templates` metadata (§2.5).

### 6.8 Evidence, analysis later, and orphans

*Sources: [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §10, §11.*

**The evidence part is the input to later analysis.** It keeps each line's raw wikitext and plain text, its heading path and links, so that an analyzer (an LLM pass that reads years, access terms or populations out of "1850–1885 … index & images ($)") can run over stored evidence without fetching or re-parsing the page. Such an analyzer is **another extractor**: it writes its own derivation per page (§6.3), its statements resolve beside the others, and its references can name the extractor. No analyzer is specified.

**An item created by extraction that no derivation names any longer is an orphan.** It is never deleted automatically: someone may have edited it, linked to it or cited it. Orphans are listed per source (`GET /extraction/{source}/orphans`, [18](18-api.md)), in the order they were orphaned, for review: delete, merge or keep.

## 7. Template mappings

*Sources: [0072](../decisions/0072-template-mappings.md) §1, §2, §3, §4, §6, §8.*

### 7.1 Recognizing template calls without expanding them

*Sources: [0072](../decisions/0072-template-mappings.md) §1.*

**A template call is recognized from the parse tree, by name and parameters, and never expanded.** `scatter-wikitext`'s parser already yields template nodes; the scanner ([0072](../decisions/0072-template-mappings.md) §7, in [22](22-crates-and-stack.md)) reads them:

- **What is a call.** `{{Name|…}}` at any nesting depth, inside or outside tables, lists and `<ref>` tags. Parser functions (`{{#if:…}}`), magic words and variables (`{{PAGENAME}}`) are not calls, by the registry of [0042](../decisions/0042-template-expansion-and-parsoid.md) §5 ([11](11-rendering-templates-and-modules.md)). Text inside `<nowiki>`, `<pre>`, `<syntaxhighlight>`, `<math>` and comments is not scanned. A call inside `<ref>…</ref>` is recognized and marked `in_ref`.
- **Names** are normalized as MediaWiki normalizes titles: a leading `Template:` or the repository's localized namespace name is removed, underscores become spaces, runs of spaces collapse, and the first letter is upper-cased where the repository's namespace is `first-letter`. `subst:`, `safesubst:` and `msgnw:` prefixes are removed. A name that is itself a template call (`{{ {{X}} }}`) is not recognized.
- **Redirects.** A mapping names a template's canonical title. A call to a redirect to it matches, through the repository's mirrored Template pages and their redirect line ([0051](../decisions/0051-page-redirects.md) §1), so the source should mirror namespace 10; a mapping may also list `aliases`.
- **Parameters** follow MediaWiki: positional parameters are numbered from 1; a named parameter's name and value are trimmed of surrounding whitespace, a positional value is not; when a parameter is given twice the last wins, and the duplicate is reported. Each value is kept as raw wikitext and as **plain text**, in which `[[Target|text]]` becomes `text`, `[url text]` becomes `text`, bold and italic marks are removed, and a nested call stays as its raw wikitext unless the value's parser reads it (§7.3).
- **Unbalanced braces** end the call at the end of the line it opened on, and the call is reported as malformed with its line, so one broken call does not swallow the rest of the page.

Recognition of template calls by name and parameters feeds mappings, here through derived statements on items (§6.1). Page statements and category mappings ([0038](../decisions/0038-page-metadata-and-categories.md), in [06](06-statements-and-properties.md) and [10](10-pages-and-content-models.md)) are unchanged, and template expansion ([0042](../decisions/0042-template-expansion-and-parsoid.md)) is neither needed nor used.

### 7.2 The `template-mapping` config kind

*Sources: [0072](../decisions/0072-template-mappings.md) §2.*

**A mapping turns a template's calls into statements about a subject.** Mappings are tenant configuration, `config` records of kind `template-mapping` whose code is the mapping's name:

| Field | Meaning |
|---|---|
| `template`, `aliases` | The template's canonical title, and other names that count as it |
| `when` | Optional: conditions on parameters, `{ param = "Type", equals = "Topic" }`, `present`, `absent` or `matches` a pattern; all must hold |
| `subject` | `page`: the statements are about the page's own subject (§6.2, `page_subject`). `call`: each call is a subject of its own, found or created by `match` |
| `match` | For `subject = call`: the match keys, in order, each `{ property, param, parser }` (§7.3) |
| `label` | For `subject = call`: the parameter, or a pattern over parameters (`"{2}"`), that labels a created item |
| `statements` | A list of `{ property, param or value, parser, qualifiers }`: a parameter read through a parser, or a constant value, with optional qualifiers built the same way |
| `create` | For `subject = call`: whether a subject not found may be created (default `true`) |

**One call may match several mappings**, and each writes its statements; two mappings for one template with different `when` conditions are how a template with modes is mapped.

### 7.3 Parsers

*Sources: [0072](../decisions/0072-template-mappings.md) §3.*

**A parser turns a parameter's value into a Wikibase data value, or into nothing.** A value a parser cannot read yields no statement and is reported, per mapping and parameter, with examples ([0072](../decisions/0072-template-mappings.md) §5).

| Parser | Reads | Yields |
|---|---|---|
| `string` | Plain text | A string |
| `external-id` | Plain text | An external identifier, normalized by the property's normalizer ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §7, in [04](04-entities-and-identifiers.md)) |
| `url` | A URL, or the first external link in the value | A URL, normalized as §8.3 normalizes |
| `item` | Plain text, looked up in a value map (§7.4) named by `map` | An item |
| `wikilink` | The first internal link in the value, or the whole value as a title | The subject of that page, through the source's `page_subject` match key; otherwise the item paired with the title by a sitelink; otherwise nothing |
| `year`, `date` | `1850`, `1850-04-12`, `12 April 1850` and the repository's content-language month names | A time value of precision 9 or 11 |
| `year-range` | `1850-1885`, `1850–1885`, `1850 to 1885` | Two values, written to the `start` and `end` properties the mapping names, or as `start time`/`end time` qualifiers |
| `monolingual` | Plain text | Monolingual text in the source's `language` |

Any parser may take `split = ","` (or `";"`, `"<br>"`), which splits the value and parses each piece, and `trim` patterns to remove from the text before parsing.

### 7.4 Value maps

*Sources: [0072](../decisions/0072-template-mappings.md) §4.*

**A value map turns the strings a wiki uses into items.** It is a `config` record of kind `value-map`, whose code is the map's name, holding entries `"string" = value`. Lookup is on the plain text, case-folded, with whitespace collapsed and a trailing full stop removed; no other fuzziness. The FamilySearch link text "Probate", "Probate Records" and "Probate records" are three entries for one item.

**Unmapped strings are collected, not guessed.** Each extraction run records every string a map did not hold, with its count and three example pages, served at `GET /extraction/{source}/unmapped` ([18](18-api.md)). Curating the map is how coverage grows; a change to a map re-runs the sources that use it (§6.7).

### 7.5 Drafting mappings from TemplateData and Cargo

*Sources: [0072](../decisions/0072-template-mappings.md) §6.*

**A mapping can be drafted from what the wiki already declares about a template, and is then reviewed by a person.** `triplespace-cli extraction draft --source {source} --template {title}` ([23](23-configuration-and-registry.md)) reads the mirrored Template page:

- **TemplateData** ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §5, in [11](11-rendering-templates-and-modules.md)): parameter names, aliases, descriptions and types; `url` suggests the `url` parser, `date` the `date` parser, `wiki-page-name` the `wikilink` parser.
- **Cargo**: a `{{#cargo_declare:…}}` in the template gives field names and types, and its `{{#cargo_store:…}}` which parameter feeds which field; `URL`, `Date`, `Page` and `List (,) of …` suggest `url`, `date`, `wikilink` and `split`.
- **Usage**: the parameters actually used across the source's pages, with counts and sample values, from a dry run of the scanner.

The draft is TOML with every parameter listed and the property left blank where nothing suggests one. It is never applied automatically.

### 7.6 The first mappings, for illustration

*Sources: [0072](../decisions/0072-template-mappings.md) §8.*

With illustrative local IDs (P9001 FamilySearch collection ID, P9002 FamilySearch Catalog ID, P9003 FamilySearch Library call number, P9010 record type, P9011 covers jurisdiction):

```toml
[mapping.countrysidebar]
template   = "CountrySidebar"
when       = [ { param = "Type", equals = "Topic" } ]
subject    = "page"
statements = [
  { property = "P9011", param = "Country", parser = "item", map = "jurisdictions" },
  { property = "P9010", param = "Records", parser = "item", map = "record-types" },
]

[mapping.recordsearch]
template = "RecordSearch"
subject  = "call"
match    = [ { property = "P9001", param = "1", parser = "external-id" } ]
label    = "{2}"
statements = [ { property = "WDP123", value = { entity = "Q9100" } } ]   # publisher: FamilySearch (illustrative item)

[mapping.fsc-catalog]
template = "FSC"
when     = [ { param = "2", matches = "^(item|title-id)$" } ]
subject  = "call"
match    = [ { property = "P9002", param = "1", parser = "external-id" } ]
label    = "{disp}"

[mapping.fsc-callnumber]
template = "FSC"
when     = [ { param = "2", absent = true } ]
subject  = "call"
match    = [ { property = "P9003", param = "1", parser = "external-id" } ]
label    = "{disp}"
```

A `RecordSearch` call in a resource line is the line's resource; §8.5 adds the line's context to it.

## 8. Lines, links and URL patterns

*Sources: [0073](../decisions/0073-lines-links-and-url-patterns.md) §1, §2, §3, §4, §5, §6, §8.*

### 8.1 Lines

*Sources: [0073](../decisions/0073-lines-links-and-url-patterns.md) §1.*

**A line is the unit a resource is read from.** The scanner of §7.1 yields:

- **List items**, `*` and `#` at any depth, and `:` indented items. A nested item has its parent item as context, so "Also at: …" under a resource line is known to be about it.
- **Table rows** of any table, from `|-` to the next `|-` or `|}`. A row of `!` cells before the first data row is the table's header row, and its plain-text cells are the table's **column names**; a row-header cell (`! scope="row" |`) is a cell of its row.
- **Nothing else.** Paragraph text is not a line; it stays in the evidence of the lines near it only as the heading context below.

Each line carries its **heading path**: the page's headings above it, levels 2 to 6, and **pseudo-headings**, a paragraph that consists only of bold or italic text and is followed by a list or table, treated as a heading one level below the current one. `'''Deeds'''` in the Ireland page is a pseudo-heading under "Resources › Online Resources".

### 8.2 Links

*Sources: [0073](../decisions/0073-lines-links-and-url-patterns.md) §2.*

**Each line yields its links**, in order:

| Kind | From | Kept as |
|---|---|---|
| External | `[url text]`, and bare URLs in text | The URL and its text |
| Call subject | A template call that a mapping of §7 with `subject = call` matched, such as `{{RecordSearch|…}}` | The call's subject and label |
| Internal | `[[Target|text]]` | Evidence only |

A line inside `<ref>` tags is not scanned for resources by default (`lines.in_ref = false`); the links are still recognized and marked `in_ref`, which is what a citation extractor needs.

### 8.3 URL normalization and match patterns

*Sources: [0073](../decisions/0073-lines-links-and-url-patterns.md) §3.*

**A URL is normalized before it is matched**, by the URL normalizer that sitelinks use ([0026](../decisions/0026-sitelinks.md) §1, in [06](06-statements-and-properties.md)): lower-case scheme and host, IDNA host, default port and fragment removed, and a trailing slash removed from a non-root path. For matching only, a set of tracking parameters is also removed (`utm_*`, `fbclid`, `gclid`, `mc_cid`, `mc_eid`), configurable as `lines.strip_params`.

**Identifiers come from URL match patterns on identifier properties.** An identifier property's statements under the role `url-match-pattern` ([0003](../decisions/0003-statement-ui.md) §7, in [06](06-statements-and-properties.md)), with an optional qualifier under the role `url-match-replacement`, are its patterns. For Wikidata properties the roles bind to P8966 and P8967, and the patterns arrive with the properties, which are always at least mirrored on demand ([0070](../decisions/0070-shallow-entity-mirroring.md) §2.1, in [05](05-providers-and-ingest.md)). A tenant's own identifier properties carry statements under the same roles.

- The patterns of every identifier property a source lists in `lines.identifiers`, or of every property designated for `match_key` if it lists none, are compiled into one set. A pattern is PCRE as Wikidata writes it; one that uses lookaround or backreferences, which the matcher does not support, is skipped and reported.
- A URL that one pattern matches yields that property's identifier: the first capture group, or the replacement value with `\1`… substituted. The value is then normalized by the property's normalizer ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §7).
- A URL that several patterns match yields every identifier, ordered by the source's `lines.identifiers` list and then by pattern length, longest first. They become the subject's match keys in that order (§6.4); an identifier property not designated for `match_key` may find a subject but never create one.
- A URL no pattern matches yields its normalized form as the match key on `lines.url_property`, which must be designated for `match_key`. A source with no `url_property` skips such links and reports them, grouped by host, so the hosts that most need a pattern are visible.

### 8.4 Which links are resources

*Sources: [0073](../decisions/0073-lines-links-and-url-patterns.md) §4.*

**Every external link and call subject in a line is a resource, unless excluded.** `lines.ignore_hosts` lists hosts never treated as resources, such as the wiki itself and Wikipedia; `lines.first_link_only` (default `false`) takes only the first. A line with two resources ("… at Ancestry; *Also at: MyHeritage*") yields two subjects, each with the line's context; the two are not related to each other.

### 8.5 Line settings and statements

*Sources: [0073](../decisions/0073-lines-links-and-url-patterns.md) §5.*

**A line subject gets its identity, its mention and its context as statements.** The `lines` field of an `extraction` source (§6.2):

| Setting | Meaning |
|---|---|
| `identifiers` | The identifier properties whose patterns apply, in priority order (§8.3) |
| `url_property` | The property for unmatched URLs, designated for `match_key` |
| `ignore_hosts`, `first_link_only`, `in_ref`, `strip_params` | §8.2–8.4 |
| `mentioned_in` | The property linking a resource to the guide page's subject, such as P1343 described by source |
| `inherit` | Properties whose values on the guide page's subject are copied to each of its line subjects, such as the jurisdiction and record type a `CountrySidebar` gave the page |
| `headings` | A value map (§7.4) and property: a heading or pseudo-heading in a line's path that the map holds adds that statement, so `'''Tithe Applotment'''` adds the record type it maps to |
| `columns` | For table rows: a column name and a property with a parser of §7.3, so the "Years covered" cell becomes start and end times and the "Language" cell a language |

The statements on a line subject are, in order: its match-key identifiers and its label, on creation only (§6.4); `mentioned_in` the page subject; the inherited facets; the heading facets; the column facets. A call subject from §7 gets the same line statements beside its own mapping's. Every statement carries the source's reference (§6.5).

### 8.6 Evidence

*Sources: [0073](../decisions/0073-lines-links-and-url-patterns.md) §6.*

**Each line that yields a resource is a mention** in its derivation's evidence part (§6.3): its position, heading path, column names for a row, raw wikitext, plain text, links and the subjects it produced. Lines that yield nothing are not kept. The plain text is what a later analyzer reads for what the line rules do not: years in a bullet line's bold lead, "index & images", "($)".

### 8.7 The first source, for illustration

*Sources: [0073](../decisions/0073-lines-links-and-url-patterns.md) §8.*

```toml
[extraction.familysearch.lines]
identifiers  = ["P9001", "P9002", "P9020", "P9021", "P9022"]  # FamilySearch collection and catalog IDs; Ancestry, MyHeritage and Findmypast collection IDs
url_property = "P9030"                                   # URL, designated for match_key
ignore_hosts = ["www.familysearch.org/en/wiki", "en.wikipedia.org"]
mentioned_in = "WDP1343"
inherit      = ["P9011", "P9010"]
headings     = { map = "record-types", property = "P9010" }
columns      = { "Years covered" = { property = ["WDP580", "WDP582"], parser = "year-range" }, "Language" = { property = "WDP407", parser = "item", map = "languages" } }
```
