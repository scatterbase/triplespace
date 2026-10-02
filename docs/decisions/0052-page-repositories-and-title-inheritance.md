# 0052. Page repositories and title inheritance

- **Status:** Proposed
- **Date:** 2026-10-01
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0019](0019-discussions.md), [0038](0038-page-metadata-and-categories.md), [0042](0042-template-expansion-and-parsoid.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0007](0007-actor-identity.md), [0009](0009-keyed-entity-types-and-domain.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0026](0026-sitelinks.md), [0028](0028-tenancy-policy.md), [0039](0039-files-and-media.md), [0041](0041-content-models.md), [0043](0043-lua-modules.md), [0051](0051-page-redirects.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

Triplespace lets local and foreign **entities** exist side by side: `Item:Q6` is minted here, `Item:WDQ42` is Wikidata's, both are items, and where they say different things the local graph wins ([0000](0000-init.md) §3, [0002](0002-source-graphs-and-mass-ingest.md) §3). James wants the same for **articles**: local pages in the main namespace alongside pages that belong to another wiki, under the same titles, with a tenant-defined order deciding which one a reader sees first, or at all.

The use case is [MDWiki](https://mdwiki.org/), which selectively forks English Wikipedia's medical articles. A reader of MDWiki's *Myocardial infarction* follows a link to *Aspirin*; MDWiki has no article of its own there, so the reader keeps reading Wikipedia's, in MDWiki's interface, and the next link may lead back to an MDWiki article. Where MDWiki has forked a page there is no need to show Wikipedia's; sometimes a wiki will want the upstream page offered as a supplement to its own.

Two earlier decisions already give a tenant pages it does not own, each for one namespace and each with "first match wins":

| Where | What it serves | Lookup |
|---|---|---|
| [0039](0039-files-and-media.md) §11, file repositories | Files, as InstantCommons does | Local, then each repository in order; a local file shadows a foreign one |
| [0042](0042-template-expansion-and-parsoid.md) §11, template repositories | Template and Module pages | Local, then each repository in order, restarting at local at every level of transclusion |

Both are the mechanism this ADR needs, restricted to one namespace and to one answer per title. What neither gives is a namespace where several pages coexist under one title and a reader can be told about the others. Nor does either say what a foreign page *is* to the API: a template read from Wikipedia has no page ID here, no `prop=info`, no talk page.

Entities had an ID to share. Pages have only a title. A title is a key, and a keyed entity type ([0009](0009-keyed-entity-types-and-domain.md)) is the precedent: `domain:wikipedia.org` is one Domain whatever provider says something about it, and each provider's contribution is reconciled under it. A title in the main namespace is the same kind of key. Each repository contributes a page under it; the tenant's order reconciles them.

This ADR gives the mechanism. [0053](0053-mirrored-pages.md) says how a foreign page's content is fetched, stored and rendered. [0054](0054-forking-a-mirrored-page.md) says how a foreign page becomes a local one. [0051](0051-page-redirects.md) gives the redirects none of it works without.

### Direction

James's direction, from the design discussion of 2026-10-01:

- **Local articles in the main namespace exist alongside remote articles,** the way local and foreign entities do.
- **Multiple articles from multiple providers can coexist under the same title.** The one presented first, or at all, depends on **tenant-defined inheritance**.
- **In the event of conflict, the lesser-ranked pages can be presented as alternative pages to read, or not shown at all.** There is no need to link to a Wikipedia page when there is a local MDWiki page; sometimes a remote page is wanted as a supplement to a local one.
- **Clicking on links lets you continue reading** the remote wiki from within the local interface.

## Decision

### 1. A page repository (amends 0042 §2 and §11; extends 0015 §3)

**A page repository is a source of pages a tenant serves by title without holding them.** It generalises the template repository of [0042](0042-template-expansion-and-parsoid.md) §11 to any `pages` namespace, and it is configured as a file repository is ([0039](0039-files-and-media.md) §11): a `config` record of kind **`page-repo`**, keyed `page-repo:{name}`, in a tenant's `config` or in the instance `config`, where a tenant refers to it by name and a tenancy template may supply it ([0028](0028-tenancy-policy.md) §8).

| Field | Meaning |
|---|---|
| `kind` | `tenant`: another tenant on this instance (§7). `mediawiki`: any MediaWiki Action API, a Wikipedia above all, or a Triplespace tenant elsewhere |
| `provider` | The provider registry entry the repository belongs to ([0015](0015-record-format-and-partition-registry.md) §5, `providers.toml`). It supplies the provider **number**, for ranged IDs (§6); the **issuer**, for the actors of its revisions ([0007](0007-actor-identity.md) §1); and its API endpoint and article path. For a `tenant` repository it is the tenant's own entry ([0018](0018-tenants.md) §5). A provider that mints no entities has no `code` and no types; `scatter-providers` accepts such an entry, and English Wikipedia is one |
| `namespaces` | The repository's namespaces the tenant serves, by the repository's canonical names, with `main` for namespace 0. Each is served in the local namespace of the same canonical name. The default is `["Template", "Module"]`, which is what a template repository served; MDWiki's English Wikipedia entry is `["main", "Template", "Module", "Category"]` |
| `mode` | `proxy` (default): pages are fetched and cached ([0053](0053-mirrored-pages.md) §4). `mirror`: pages are kept in an instance partition ([0053](0053-mirrored-pages.md) §5) |
| `shadowed` | What becomes of this repository's page when a higher-ranked page exists under the same title: `offer` (default), it is offered as an alternate (§5); `hide`, it is not shown at all |
| `titles` | `index` (default for `mediawiki`): the repository's titles are mirrored into a title index ([0053](0053-mirrored-pages.md) §3), so links, redirects and `#ifexist` are answered locally. `assume`: every title is presumed to exist, for a repository with no dump to index |
| `cache_ttl` | How long fetched source and bundles are held, default one hour, as [0042](0042-template-expansion-and-parsoid.md) §11 had it |
| `events` | Optional: an EventStreams endpoint for push invalidation ([0053](0053-mirrored-pages.md) §6) |
| `licence`, `display_name` | The licence of the repository's text, as an SPDX identifier (`CC-BY-SA-4.0` for Wikimedia projects), required for `mediawiki`; and the name shown on origin chips and attribution lines |

**The tenant's order is the inheritance.** The `site` setting **`pages.repos`** lists the repositories the tenant uses, by name, in order. It replaces `wikitext.template_repos`, and **`pages.share`** replaces `wikitext.share` ([0042](0042-template-expansion-and-parsoid.md) §2): a tenant that sets it may serve its pages to other tenants as a `tenant` repository. Nothing has been written under the old names, so this is a change to the documents and the registry only.

**The `template-repo` kind is retired.** A template repository is a page repository whose `namespaces` include `Template` and `Module`. Everything 0042 §11 says of how a foreign template is read, expanded, cached, attributed and trusted holds for a page repository and is not repeated here; 0042 §11 now reads as the Template and Module case of this ADR. Data modules ([0043](0043-lua-modules.md) §6) are keyed by repository, title and remote revision as before.

### 2. A title names a stack

**For a title in a `pages` namespace, the stack is every page that exists under it, in inheritance order:**

1. the **local page**, if one exists and is not deleted ([0023](0023-moderation.md) §4);
2. then, for each repository in `pages.repos` that serves the namespace, **that repository's page**, if its title index has the title (or `titles = assume`).

Each entry carries its **origin**: `local`, or the repository's name. **The first entry is the primary; the rest are alternates.** A title whose stack is empty does not exist.

Two things in the order are fixed and not configuration:

- **A local page is always first.** The tenant's own page outranks every repository's, as the local graph wins over every mirror ([0002](0002-source-graphs-and-mass-ingest.md) §3), a local file shadows a foreign one ([0039](0039-files-and-media.md) §11) and a local template shadows a foreign one ([0042](0042-template-expansion-and-parsoid.md) §11). This is the MDWiki rule: where the wiki has forked a page, its fork is the page.
- **Repositories rank in the order the tenant lists them.** A tenant that reads two Wikipedias decides which one answers first.

**A deleted local page leaves the stack,** so deleting a fork makes the repository's page primary again ([0054](0054-forking-a-mirrored-page.md) §9). A create-protected title ([0023](0023-moderation.md) §2) reserves a page ID and nothing else: the stack is unaffected, and what the protection prevents is the fork.

**A stack is computed, never stored.** The resolver reads `view.page` for the local entry and `view.foreign_title` (§8) for the others; nothing records the order, so changing `pages.repos` changes every stack at once.

### 3. Resolution (amends 0008 §3; extends 0042 §11)

**The title resolver resolves a `pages` title to its primary.** The resolve step of [0008](0008-namespaces-and-document-pages.md) §3 reads: in a `pages` namespace, a title resolves to the primary of its stack, which is a local page ID or a foreign page named by repository and upstream page ID. Then [0051](0051-page-redirects.md) §2 applies: if the primary is a redirect, local or foreign, the resolver resolves the **target title's stack**, one hop. So English Wikipedia's redirect *Heart attack* → *Myocardial infarction* lands a reader on MDWiki's fork of *Myocardial infarction* when there is one, and on Wikipedia's article when there is not.

**Everything that asks "which page is this title?" gets the primary:** page views, `titles=` in the API, transclusion (`{{Foo}}`, `{{:Foo}}`), `#ifexist`, Lua's `mw.title` ([0043](0043-lua-modules.md) §5), link colouring, and the file lookup for a `File:` title in a served namespace. This is the lookup [0042](0042-template-expansion-and-parsoid.md) §11 already gave templates, stated once for every namespace, and its rule that **lookup restarts at local at every level** is the same rule: each nested transclusion resolves its own stack, so a foreign `Template:Infobox` that calls `{{Infobox/row}}` gets the local `Template:Infobox/row` if one exists.

**Everything that asks "which pages exist under this title?" gets the stack:** the frame (§5), `GET /page/stack/{title}` (§6) and the fork form ([0054](0054-forking-a-mirrored-page.md) §2).

**A title index answers without the network.** With `titles = index`, whether a foreign page exists, and whether it is a redirect and to what, is a local read of `view.foreign_title`; rendering a page with five hundred links costs no requests to the repository. With `titles = assume`, every link to a served namespace is presumed to exist, and a reader who follows one that does not meets the repository's "not found", rendered as a missing page with a link upstream. `assume` is for repositories that publish no title dump, and it is why `index` is the default.

### 4. Links continue reading (extends 0008 §8)

**A link resolves through the stack, so a reader keeps reading.** In the wikitext subset, in markdown ([0019](0019-discussions.md) §5) and in rewritten foreign HTML ([0053](0053-mirrored-pages.md) §2), a wiki link to a title whose primary is foreign is an ordinary blue link to the local URL of that title. Following it shows the foreign page in this tenant's frame (§5), whose own links resolve the same way. The link carries the class `ts-inherited` and `data-ts-origin="{repository}"`, so a tenant's styles or a reader's preference can mark it, and nothing else distinguishes it. A title with an empty stack is a red link, as any missing page is. A link to a redirect carries `mw-redirect` ([0051](0051-page-redirects.md) §2).

**Links out of the served namespaces leave the wiki.** A link in a foreign page to a namespace the repository does not serve here, `Wikipedia:Manual of Style` on a tenant that serves only `main`, `Template`, `Module` and `Category`, is rewritten to the repository's own URL and rendered as an interwiki link (`class="extiw"`), because there is nothing here for it to resolve to. A tenant's own pages link to the repository explicitly with its site alias as before, `[[enwiki:Foo]]` ([0026](0026-sitelinks.md) §2), and that link is not inheritance.

**"What links here" counts links to inherited titles.** The links projection ([0008](0008-namespaces-and-document-pages.md) §10) records a `page_link` row by title for a link whose primary is foreign, keyed by namespace and title as a link to a missing page is, so that when the title is forked the fork's backlinks are already there.

### 5. The frame: primary, origin and alternates (extends 0010 §2)

**The page view shows the primary** in the tenant's frame: the tenant's header, search, tabs and chrome, with the foreign content inside it as [0053](0053-mirrored-pages.md) §2 renders it.

**The identity line names the origin.** A foreign primary carries an **origin chip**, the provider's chip from the registry as a mirrored entity carries one ([0010](0010-site-ui.md) §2), and the line reads "From English Wikipedia, revision 1234567890 as of 2026-09-30 · CC BY-SA 4.0", linking the title upstream and the licence. A local primary with alternates reads as any local page does, and gains the control below.

**Alternates are offered or hidden.** When the stack has alternates, the identity line gains **Other versions**, a menu listing each alternate whose repository is `offer`: its origin, its latest revision and date, and, for a local page that is a fork, "Forked from this at revision N" ([0054](0054-forking-a-mirrored-page.md) §6). An alternate whose repository is `hide` is not listed. When nothing is listable, the control is not drawn.

**Reading an alternate** is the title's URL with **`origin={repository}`**, served in the same frame with the identity line saying so and a link back to the primary. The response is `noindex` and its canonical link is the primary; it is never what a search engine or a link indexes. An `origin` that names a hidden alternate, or one the stack does not have, answers 404 with `ts-origin-hidden` or `ts-origin-missing`.

**A page may override its repositories' defaults.** A local primary may carry a page statement ([0038](0038-page-metadata-and-categories.md) §1) whose property is bound to the role **`page-alternates`**, a `string` property whose value is `offer` or `hide`. It applies to every alternate of that title and outranks the repositories' `shadowed` settings, so a wiki whose repositories default to `hide` can offer Wikipedia's article beneath one page as a supplement, and a wiki that defaults to `offer` can hide it under another. The role is registry data like `thread-status` ([0038](0038-page-metadata-and-categories.md) §9); unbound, the repositories' defaults decide.

**Edit forks.** The Edit tab of a foreign primary opens the editor on the foreign page's wikitext and says, before anything is typed, that saving will create a local copy that no longer follows the repository ([0054](0054-forking-a-mirrored-page.md) §2). Move, Delete, Protect and Change content model are not offered on a foreign page; **Protect…** on a title whose primary is foreign offers create-protection only, which is how a tenant stops a title being forked.

### 6. Identity: provider-ranged page IDs, and the API (extends 0015 §2 and 0013 §6; extends 0012 §4–5)

**A foreign page has a derived page ID.** Its `pageid` is the **provider-ranged** form of the repository's own page ID, `provider_number << 40 | upstream page ID`, computed as mirror records' revision IDs are ([0015](0015-record-format-and-partition-registry.md) §2, [0013](0013-postgres-storage.md) §6). It is derived, not minted, as a File page's `M` ID is ([0041](0041-content-models.md) §7): nothing allocates it, it is the same on every tenant that reads the repository, and it never collides with a local page ID, which comes from the tenant's sequence below 2^40. Its `lastrevid` is the ranged form of the upstream revision ID the tenant currently holds. In `mirror` mode the same numbers are what the `pages/{repo}` records carry in header fields 9 and 7 ([0053](0053-mirrored-pages.md) §5).

**What the API reports for a foreign primary.** Reading works as for any page, from the held bundle ([0053](0053-mirrored-pages.md) §1):

| Module or route | Behaviour |
|---|---|
| `prop=info` | `pageid`, `lastrevid`, `touched`, `length`, `contentmodel` (the upstream page's), `redirect`; and, additively, **`origin`**: the repository name. A local page reports `origin: local` |
| `prop=revisions` | The latest upstream revision, with `rvprop=content` the wikitext, `user` the upstream actor under its issuer and `comment` the upstream summary. Older revisions are not held: a request for them is answered with the latest and a warning, `ts-foreign-history`, naming the upstream history URL. The site UI's History tab fetches them live instead, as [0010](0010-site-ui.md) §5.5 fetches upstream edits, never logging them ([0012](0012-api-requirements.md) §6) |
| `action=parse`, `prop=categories`, `prop=templates`, `prop=links`, `prop=images`, `prop=langlinks`, `prop=pageprops` | From the bundle |
| `list=allpages`, `list=prefixsearch`, `generator=allpages` | Local pages, then, with **`tsorigin=all`**, the title index's titles for served namespaces, each row carrying `origin`. Without the parameter they list local pages only, so a bot that walks a wiki gets what the wiki holds |
| `titles=`, `pageids=`, `redirects` | Resolve through the stack and through foreign redirects; a ranged `pageid` resolves to the repository page it names |
| `action=edit`, `action=delete`, `action=move`, `action=protect` (except create-protection), `action=changecontentmodel` | `action=edit` on a foreign primary starts a fork ([0054](0054-forking-a-mirrored-page.md) §2). The others are refused with `ts-foreign-page`: there is nothing local to act on |
| `GET /page/{id}` *(REST v0)* | Accepts a ranged ID; the summary carries `origin`, `upstream` (page ID, revision, URL) and `stack` (every origin under the title, in order, with `shown: true` or `false`) |
| `GET /page/stack/{title}` *(REST v0)* | The stack of a title: each entry's origin, page ID, latest revision and whether it is offered |
| `meta=siteinfo&siprop=triplespace` | Gains `page_repos`: each repository's name, kind, provider, served namespaces, mode, `shadowed`, `titles` and licence, as `meta=filerepoinfo` lists file repositories |

**Sitelinks target local pages only.** A sitelink to the tenant's own host is stored by page ID ([0038](0038-page-metadata-and-categories.md) §6), and a foreign page's ID changes when it is forked, so a title whose primary is foreign is refused as a sitelink target with `ts-sitelink-foreign`. An item that wants an article here gets a fork (Q2).

### 7. Talk pages, statements, categories and feeds

**A foreign page's talk page is local.** Threads attach to `{kind: page, id: <the ranged page ID>}` ([0019](0019-discussions.md) §2), exactly as they attach to a mirrored entity, and the talk page offers a link to the repository's own talk page for readers who want that discussion. When the page is forked, the fork job moves these threads to the fork's talk page ([0054](0054-forking-a-mirrored-page.md) §5). Threads about a hidden alternate's talk page attach to the primary's.

**A foreign page carries no local statements.** Page statements are keyed by page ID in the tenant's `pages` partition ([0038](0038-page-metadata-and-categories.md) §1), and a ranged ID could key them; but a page with local statements and no local text would be a third kind of entry, neither inherited nor forked, and the fork would have to carry the statements across a change of page ID. Local metadata about a foreign page waits for a decision on that shape (Q1). Until then, a fork is the way to say anything local about a page.

**Categories are shown, not joined.** A foreign page's categories, from its bundle, are listed at its foot as links to `Category:` titles, which resolve through the stack like any title. The page is not a member in `view.page_category`, whose rows are a projection over local pages ([0038](0038-page-metadata-and-categories.md) §3); a category page lists its local members and, when the category is served by a repository, links to the members upstream (Q3).

**Feeds.** A foreign page has no activity rows in the tenant: its changes are the repository's. Watching a title whose primary is foreign watches its local talk page and, through the title, the fork when one is made. Upstream changes reach a watcher through the repository's own feeds, which the talk page links to (Q4).

**Tenant repositories** (`kind = tenant`) are read directly, as `tenant` file and template repositories are: the source tenant's `view.page` rows and records, with its ACLs evaluated as the source tenant's, so a page deleted there is missing here. The source must set `pages.share = on`; under `providers.between_tenants = operator` only the operator can; reader lists apply ([0028](0028-tenancy-policy.md) §5); a private tenant cannot share. **The page is rendered here, in this tenant's context,** by this tenant's expander over this tenant's stacks, as an inherited template is expanded in the local page's context ([0042](0042-template-expansion-and-parsoid.md) §11); the source tenant's rendered HTML is not reused.

### 8. Storage (extends 0013 §5.6)

```sql
CREATE TABLE view.foreign_title (            -- the title index of a repository (§2–3; filled by 0053 §3)
  repo text NOT NULL,                          -- the repository name; instance scope, shared by every tenant that lists it
  ns integer NOT NULL, title text NOT NULL,    -- the repository's own namespace number and normalized title
  upstream_page_id bigint NOT NULL,
  is_redirect boolean NOT NULL DEFAULT false,
  redirect_ns integer, redirect_title text, redirect_fragment text,
  lastrevid bigint, touched timestamptz,
  PRIMARY KEY (repo, ns, title)
);
CREATE INDEX foreign_title_page ON view.foreign_title (repo, upstream_page_id);
```

The table is instance scope, like `files/{repo}` and the shared entity views ([0018](0018-tenants.md) §6): a repository's titles are the same for every tenant, and English Wikipedia's are tens of millions of rows kept once. Which tenants read which repository is `pages.repos`. `view.page` is unchanged: foreign pages have no row in it, and the resolver joins the two.

**Caches** ([0014](0014-caches-and-search.md) §4): a stack is not cached; its two reads are indexed. A rendered foreign page is cached as [0053](0053-mirrored-pages.md) §4 says. The `p:` key of a local page gains nothing, since a local primary renders as before.

### 9. Permissions and tenancy (extends 0016 §2)

| Action | Needs |
|---|---|
| Writing a `page-repo` record, `pages.repos` or `pages.share` | `ts-config`, as every `config` kind does ([0016](0016-permissions-and-access-control.md) §2) |
| Reading a foreign page | `read`; the repository's own content is public by definition |
| Setting `page-alternates` on a page | `edit` on the page, as any page statement ([0038](0038-page-metadata-and-categories.md) §14) |
| Forking | [0054](0054-forking-a-mirrored-page.md) §8 |

An instance-scope `page-repo` is shared machinery under `config.template` ([0028](0028-tenancy-policy.md) §8): provisioned under `defaults`, locked under `locks`, as file repositories are.

### 10. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `triplespace-titles` | The stack (§2–3): resolving a `pages` title to its primary over `view.page` and `view.foreign_title`, following redirects through stacks, the `origin` query, and `GET /page/stack` |
| `triplespace-render` | Link classes `ts-inherited` and `extiw` for inherited and leaving links (§4); `ExpandHost` answers template, module and `#ifexist` lookups from the stack, which is what [0042](0042-template-expansion-and-parsoid.md) §11 already had it do for templates |
| `scatter-providers` | A provider entry with no `code` and no types (§1) |
| `scatter-pages` | The `page-repo` config kind replaces `template-repo` in the registry; the `page-alternates` role |

## Alternatives considered

- **Interwiki links only,** as MediaWiki does: `[[w:Aspirin]]` leaves the wiki. Rejected: it is what MDWiki has today and what James wants to replace; the reader leaves the interface and the link graph breaks at every unforked page.
- **One repository ranked above local pages,** for a wiki that wants upstream to win over stale forks. Rejected: every precedent puts the local source first, and a wiki that wants upstream's text deletes its fork, which puts the repository's page back at the top (§2).
- **A foreign page as a local page with a `foreign` content model,** given a tenant page ID on first view. Rejected: a page ID is minted by a write ([0015](0015-record-format-and-partition-registry.md) §2), a view is not a write, and a mirror of millions of titles would mint millions of IDs. The derived ranged ID costs nothing and is the same everywhere.
- **Storing the stack.** Rejected: the order is one setting, and a table of stacks would have to be rewritten whenever it changed.
- **A per-page override as a behaviour switch** (`__HIDEALTERNATES__`). Rejected: it only works in wikitext, and [0038](0038-page-metadata-and-categories.md)'s direction is native page metadata over text magic.

## Consequences

- **A wiki can read another wiki from inside itself.** Every link resolves locally, every unforked title shows the upstream page in the tenant's frame, and every forked title shows the fork, with the upstream page a menu away when the tenant wants it there.
- **Templates and modules lose nothing.** 0042 §11's lookup is a case of §3; the setting and the config kind are renamed before any instance exists.
- **Foreign pages have stable identity in the API.** A bot that reads `prop=info` on an inherited title gets a page ID that means the same thing tomorrow and on another tenant, and `origin` tells it what it is reading.
- **The title index is the price of local link resolution.** English Wikipedia's titles and redirects are on the order of fifty million rows at instance scope; [0053](0053-mirrored-pages.md) §3 keeps them current. A repository without an index degrades to `assume`.
- **Nothing local can be said about a foreign page except by forking it.** That is deliberate for now (Q1).
- **Sitelinks cannot reach inherited titles.** A tenant that wants its items linked to articles forks the articles.
- **Two settings and a config kind change name** before any code reads them.

## Open questions

- **Q1. Local metadata on foreign pages.** Whether a title whose primary is foreign may carry local page statements, keyed by the ranged ID, and how a fork carries them across the change of page ID. The file-page precedent ([0039](0039-files-and-media.md) §11, "a local page for a foreign file") suggests yes.
- **Q2. Sitelinks to inherited titles.** Whether a sitelink may target a foreign primary by title rather than page ID, breaking when the title is forked, or whether forking should re-target it.
- **Q3. Foreign category membership.** Whether a `Category:` page served by a repository should list the repository's members, fetched live or from the mirror, and whether mirrored pages' categories should join `view.page_category` in `mirror` mode.
- **Q4. Watching foreign pages.** Whether a watch on an inherited title should deliver the repository's changes to the page, which needs the repository's events ([0053](0053-mirrored-pages.md) §6) to become activity rows or notifications.
- **Q5. Automatic ranking.** Whether a repository could be ranked per page rather than per tenant, for example by a page statement naming the preferred origin, which would make the stack partly data.
- **Q6. Namespaces with different canonical names.** `Wikipedia:` is English Wikipedia's `Project` namespace. Serving it as this tenant's `Project` is wrong, and serving it under its own name needs a namespace that does not exist here. Whether a repository may map a namespace onto a new local one.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §10 | extends | 0005 A51 |
| [0008](0008-namespaces-and-document-pages.md) §3 | §3 | amends | 0008 A22 |
| [0008](0008-namespaces-and-document-pages.md) §8, §10 | §4 | extends | 0008 A22 |
| [0010](0010-site-ui.md) §2 | §5 | extends | 0010 A29 |
| [0012](0012-api-requirements.md) §4, §5 | §6 | extends | 0012 A31 |
| [0013](0013-postgres-storage.md) §5.6, §6 | §6, §8 | extends | 0013 A22 |
| [0015](0015-record-format-and-partition-registry.md) §2, §3, §5 | §1, §6 | extends | 0015 A23 |
| [0019](0019-discussions.md) §2 | §7 | extends | 0019 A12 |
| [0038](0038-page-metadata-and-categories.md) §6 | §6 | extends | 0038 A7 |
| [0042](0042-template-expansion-and-parsoid.md) §2, §11 | §1, §3 | amends | 0042 A5 |

## References

- [MDWiki](https://mdwiki.org/) and [WikiProject Medicine's offline and mirror work](https://en.wikipedia.org/wiki/Wikipedia:WikiProject_Medicine)
- [Manual:$wgForeignFileRepos](https://www.mediawiki.org/wiki/Manual:$wgForeignFileRepos) and [InstantCommons](https://www.mediawiki.org/wiki/InstantCommons), the pattern 0039 §11 and this ADR follow
- [Global templates](https://www.mediawiki.org/wiki/Global_templates), the proposal MediaWiki never shipped
- [Manual:Interwiki](https://www.mediawiki.org/wiki/Manual:Interwiki) and [Help:Interwiki linking](https://www.mediawiki.org/wiki/Help:Interwiki_linking), for what §4 replaces
- [API:Info](https://www.mediawiki.org/wiki/API:Info) and [API:Filerepoinfo](https://www.mediawiki.org/wiki/API:Filerepoinfo)
