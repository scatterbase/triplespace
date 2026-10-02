# 0051. Page redirects

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-01 (A1)
- **Author:** James Hare / Claude Fable
- **Changes:** [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0030](0030-edit-filters.md), [0038](0038-page-metadata-and-categories.md), [0039](0039-files-and-media.md), [0047](0047-special-pages.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0011](0011-logs.md), [0023](0023-moderation.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

[0008](0008-namespaces-and-document-pages.md) Q8 left page redirects open: whether document pages may be redirects, and whether a move leaves one. Nothing on the roadmap needed them until now. Librarybase's project pages are few and rarely renamed, and an entity has aliases instead: a `redirect` record in the local or a mirror graph retires an ID in favour of another, and the title resolver follows it to the canonical ID ([0002](0002-source-graphs-and-mass-ingest.md) §8.2, [0004](0004-identity-clusters-and-equivalence.md) §4). That machinery is for IDs, not titles, and it is not what MediaWiki means by a redirect.

Mirroring and forking Wikipedia articles ([0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md)) needs MediaWiki's redirects. An English Wikipedia article has on average more than one redirect to it, and a popular one has dozens: alternative spellings, former titles, the names of sections that were once articles. Links in article text go through them as a matter of course. A reader who follows a link from a mirrored page to `Heart attack` has to arrive at `Myocardial infarction`, on the local fork if there is one. Without redirects a mirror is full of dead ends, and a fork of a page cannot keep the links that point at its old names.

MediaWiki's design is simple and clients depend on its details: a redirect is a page whose text begins with `#REDIRECT [[Target]]`, the `redirect` table caches the target, page views follow one hop and say so, `action=query&redirects` resolves them, moves leave one behind, and three maintenance reports list them. [0047](0047-special-pages.md) §10 deferred those reports on this question.

James's direction, from the design discussion of 2026-10-01, was for the mirroring and forking goal as a whole; redirects are the first thing it needs that no ADR had settled.

## Decision

### 1. A redirect is a wikitext page whose text says so (settles 0008 Q8)

**A page is a redirect when its content model is `wikitext` and its text begins with a redirect line,** `#REDIRECT [[Target]]`, exactly as MediaWiki decides it: the `redirect` magic word, with the localized synonyms MediaWiki's message files give it, followed by a wiki link, optionally with a fragment, `#REDIRECT [[Myocardial infarction#Signs and symptoms]]`. Whatever follows the first line is kept and shown on the redirect page itself (MediaWiki allows categories and `{{R from …}}` templates there) but has no other effect. The magic word is registered in `docs/registry/wikitext-functions.toml` with status `implemented`.

**Only `wikitext` pages can be redirects.** `markdown`, `json`, `yaml`, `text`, `Scribunto`, `triplespace-table` and `triplespace-board` pages never are, whatever their first line says, as MediaWiki's non-wikitext content handlers never produce one. Any namespace whose allowed models include `wikitext` may hold redirects ([0008](0008-namespaces-and-document-pages.md) §2): main, User, Project, File, Category, Template, Module (its `/doc` pages) and Table (its `/doc` pages).

**The target may be any title.** A redirect may point into any registered namespace: another article, a Project page, a category, or an entity page, `#REDIRECT [[Item:Q42]]`. The target is resolved by the title resolver like any link, so a redirect to `Item:WDQ123` lands on the cluster's canonical page ([0004](0004-identity-clusters-and-equivalence.md) §4). A target with an interwiki prefix, `#REDIRECT [[enwiki:Foo]]`, is an **external redirect**: it is recorded, listed and rendered as a link, and never followed automatically, as MediaWiki does not follow one unless `$wgDisableHardRedirects` is off and the prefix is marked local.

**A redirect is still a page.** It has a page ID, records, history, a talk page, statements and categories like any other page in its namespace. Being a redirect is a property of its latest text, read by the projection (§5), never a record of its own.

### 2. Following a redirect (extends 0008 §3)

*Changed by A1.*

**The title resolver follows one hop, and only to a target the principal may read.** When the target is under a confidential `read` restriction the principal does not satisfy, the resolver answers with the redirect page itself, whose text names the target as a link to a missing page ([0056](0056-security-model.md) §6). Its resolve step ([0008](0008-namespaces-and-document-pages.md) §3) gains a fourth rule: when a `pages` namespace title resolves to a page that is a redirect, the resolver resolves the target title and answers with *that* page, carrying the redirect's page ID alongside as `redirected_from`. It follows **exactly one** hop, as MediaWiki does: a redirect to a redirect is a **double redirect**, served as the second redirect page, and listed by the report of §6. There is therefore no loop to detect. A redirect whose target does not resolve is a **broken redirect**, served as the redirect page itself, with the target as a red link.

**`redirect=no` stops it.** A page view with `redirect=no`, the API's page-set parameters without `redirects`, `action=edit`, `action=move`, `action=delete`, `prop=info` and every write address the redirect page itself, as in MediaWiki. Only reads that ask to follow are followed.

**A followed view is the target with a notice.** The page view answers HTTP 200 with the target's content and a "(Redirected from *Old title*)" line under the title, linking to the redirect with `redirect=no`. It is not an HTTP redirect: the URL stays what the reader typed or followed, and a reader who wants the redirect page can reach it from the notice. This differs from a forwarding namespace ([0049](0049-boards.md) §2), which answers 301, because a page redirect is content that can change and be edited, and a forwarding title is a fixed rule with no page.

**Fragments compose.** A redirect with a fragment lands the reader at that section; a link with its own fragment keeps its own, as MediaWiki does.

**Links to redirects are marked.** The wikitext subset and the markdown model render a link whose target is a redirect with the class `mw-redirect`, and `prop=info&inprop=linkclasses` reports it, so Parsoid ([0042](0042-template-expansion-and-parsoid.md) §8.1) and the site UI draw it as MediaWiki does. A link is never rewritten to its target; the stored source and the rendered `href` keep the redirect's title.

**Transclusion follows one hop too,** which [0042](0042-template-expansion-and-parsoid.md) §4 already provides for `{{Foo}}` where `Template:Foo` is a redirect. `#ifexist` answers true for a redirect, as it does for any page. A redirect page itself is never transcluded as its target: `{{:Foo}}` with `Foo` a redirect transcludes `Foo`'s target, one hop, as MediaWiki does.

**Foreign redirects** are followed by the same rule, with the target looked up in the title stack: a redirect in a page repository's title index resolves to the stack of its target title, so a Wikipedia redirect lands on a local fork when one exists ([0052](0052-page-repositories-and-title-inheritance.md) §4).

### 3. Moves leave a redirect (amends 0008 §4)

**A move leaves a redirect at the old title by default,** as MediaWiki's does. `action=move` and the Move form append, in one transaction with one base check:

1. the page's `move` record with the new title ([0008](0008-namespaces-and-document-pages.md) §4);
2. a `create` record for a **new page** at the old title, model `wikitext`, text `#REDIRECT [[New title]]`, attributed to the mover with the move's summary.

The log event is one `move/move` ([0011](0011-logs.md) §6.1) whose parameters carry the old and new titles and, as MediaWiki's do, whether a redirect was left and its page ID. With `noredirect`, which needs `suppressredirect` (§8), only the `move` record is written and the event is `move/move_redir` where MediaWiki would so name it, or `move/move` with `noredirect` set.

**0008 §4's "a move is one record" now reads "one record on the moved page".** The redirect is a second page, with its own ID and its own history, as it is in MediaWiki; nothing about the moved page is rewritten.

**Exceptions, kept from earlier ADRs:**

- **User renames leave no redirect** ([0008](0008-namespaces-and-document-pages.md) §6): the batch of `move` records a rename or a vanishing writes carries no `create`, because names are erasable and a redirect would keep the old one visible. A user moving one of their own subpages by hand is an ordinary move and leaves one.
- **Talk pages do not move,** because they are composite and attached by identifier ([0019](0019-discussions.md) §2): the threads of a moved page's talk page are already its new title's threads. `movetalk` is accepted and does nothing, and no talk redirect is created, since a talk title always resolves through its subject.
- **Thread renames leave none** ([0019](0019-discussions.md) §3), for the reason user renames do.

**Subpages.** `movesubpages` moves every subpage under the old title in the same batch, each leaving its own redirect, in namespaces that allow subpages. It needs `move-subpages` (§8).

**Moving over a redirect.** A move onto a title that is occupied is refused with `articleexists`, with one exception that MediaWiki makes and clients expect: when the occupying page is a redirect with **one revision** whose target is the page being moved, the move proceeds. The occupying redirect is deleted first, which here is a `read` ACL on its page ID ([0023](0023-moderation.md) §4) written in the same transaction and logged as `delete/delete` with the move as its reason, so the title index frees the title before the `move` record takes it. This needs `delete-redirect` (§8). A redirect with more history, or pointing elsewhere, is not displaced: the mover edits or deletes it first.

### 4. Category, File and entity redirects (settles 0038 Q5; extends 0039 §1)

**Category redirects.** A `Category:` page that is a redirect is followed when it is **viewed**, with the notice of §2, and nowhere else: membership is defined by the text of each member ([0038](0038-page-metadata-and-categories.md) §3), so the members of `Category:Old` stay in `Category:Old`, and a redirect page's own member list is shown beneath the notice when it has any, as MediaWiki shows it. Moving members is a job for a bot that edits their text, or for a category mapping's make-it-real job once the old category is mapped to the same statement ([0038](0038-page-metadata-and-categories.md) §5). Wikipedia's soft category redirects remain what they are there: a template, `{{Category redirect}}`, that expands to a notice. This settles 0038 Q5: hard redirects follow for viewing, soft ones stay templates, and neither moves a member.

**File redirects.** A `File:` page that is a redirect redirects its file too: the file lookup of [0039](0039-files-and-media.md) §11 follows a local redirect page one hop before trying repositories, as MediaWiki's `RepoGroup::findFile` does, so `[[File:Old name.jpg]]` embeds the renamed file. `movefile` leaves a redirect as any move does.

**Entity namespaces have no page redirects.** `Item:`, `Property:` and the keyed-type namespaces hold composed views with no text, so §1 cannot apply, and ID aliases ([0004](0004-identity-clusters-and-equivalence.md) §4) already do the work. `Special:Redirect/…` is unrelated: it maps IDs to pages ([0047](0047-special-pages.md)).

### 5. Projection, caches and search (extends 0013 §5.4, 0008 §10 and 0014 §7)

```sql
CREATE TABLE view.redirect (                   -- MediaWiki's redirect table (§1)
  page_id bigint PRIMARY KEY REFERENCES view.page,
  target_ns integer, target_title text NOT NULL,   -- target_ns NULL for an external redirect
  fragment text, interwiki text,                   -- interwiki set for an external redirect
  target_page_id bigint                            -- resolved at projection; NULL when broken or external
);
CREATE INDEX redirect_target ON view.redirect (target_ns, target_title);
```

- **`view.page` gains `is_redirect boolean NOT NULL DEFAULT false`,** so `prop=info` and `list=allpages&apfilterredir` need no join.
- **Written in step 2 with `page`** ([0038](0038-page-metadata-and-categories.md) §10), because it reads only the page's latest text, and under the synchronous budget, so a saved redirect works on the next request. `target_page_id` is refreshed when the target is created, deleted, undeleted or moved, by the same title-index maintenance that keeps `page_link` targets current.
- **The links projection** ([0008](0008-namespaces-and-document-pages.md) §10) records a `page_link` row from the redirect to its target, so "What links here" on the target lists its redirects, `list=backlinks&blredirect` expands through them, and `prop=redirects` is a query on `view.redirect`.
- **Caches** ([0014](0014-caches-and-search.md)): a followed view is cached under the **target's** key, since its HTML is the target's with a notice whose text is the request's title; the notice is added outside the cached fragment. The title resolver's lookup of `view.redirect` is one indexed read.
- **Search** ([0014](0014-caches-and-search.md) §7): a redirect page is not indexed as a document. Its title is added to its target's document in the `pages` index, in a `redirect_titles` field, as CirrusSearch's `redirect` field does, so that a search for *Heart attack* finds *Myocardial infarction* and the suggester offers the redirect title with "→ Myocardial infarction". Broken and external redirects are indexed as documents of their own, so they can be found and fixed.

### 6. Reports (extends 0047 §4.4; amends 0047 §10)

Four pages leave the deferred table of [0047](0047-special-pages.md) §10 and are served, with the report backing of 0047 §4:

| Page | Lists | Backing |
|---|---|---|
| `Special:ListRedirects` | Every redirect with its target | `view.redirect` joined to `view.page` |
| `Special:BrokenRedirects` | Redirects whose `target_page_id` is null and that are not external | The same |
| `Special:DoubleRedirects` | Redirects whose target is itself a redirect, with the final target | `view.redirect` self-joined |
| `Special:RandomRedirect` | One redirect at random, as `Special:Random` picks a page | `view.redirect` |

All four read local graphs by default, as every report does ([0047](0047-special-pages.md) §4.2). Each is a live projection ([0047](0047-special-pages.md) §4.3). `special-pages.toml` is updated accordingly.

### 7. API (extends 0012 §4)

| Module or parameter | Behaviour |
|---|---|
| `action=query&redirects` | Resolves the page set through redirects, one hop, and reports each in the `redirects` array with `from`, `to` and `tofragment`, as MediaWiki does |
| `prop=info` | `redirect` flag; `inprop=linkclasses` reports `mw-redirect` (§2) |
| `prop=redirects`, `generator=redirects` | The redirects to each page, from `view.redirect` |
| `list=allredirects`, `generator=allredirects` | Every redirect into a namespace, with `arprop=ids\|title\|fragment\|interwiki` |
| `list=allpages&apfilterredir` | `all`, `redirects` or `nonredirects` |
| `list=backlinks&blredirect` | Expands through redirects, as MediaWiki does |
| `action=parse&redirects` | Parses the target |
| `action=edit` | Saving a page whose text begins with a redirect line makes it a redirect; `redirect=no` is implied on the title |
| `action=move` | `noredirect`, `movesubpages`, `movetalk` (accepted, no effect) and the move-over-redirect rule of §3; the response reports `redirectcreated` |
| `action=delete`, `action=undelete` | Address the redirect page itself |
| MediaWiki REST `GET /v1/page/{title}` and relatives | Follow one hop with `redirect=no` to stop, answering as MediaWiki REST does |
| `GET /page/{id}` *(REST v0)* | A page summary gains `redirect_to` for a redirect; `GET /resolve` ([0029](0029-resolver-namespaces.md) §6) reports `redirected_from` when it followed one |

### 8. Permissions (extends 0016 §2)

| Permission | Governs | Default groups |
|---|---|---|
| `suppressredirect` | `noredirect` on a move (§3) | `sysop`, `bot` |
| `delete-redirect` | Moving over a one-revision redirect that points at the moved page (§3) | `user` |
| `move-subpages` | `movesubpages` (§3) | `user` |
| `move-rootuserpages`, `move-categorypages` | Moving a user's root page, and moving category pages, as MediaWiki gates them | `user` |

These are MediaWiki's rights with MediaWiki's default groups, so bots that check them behave as they do elsewhere. `docs/registry/groups.toml` is updated.

### 9. Filters and feeds (extends 0030 §2)

- **Edit filters** ([0030](0030-edit-filters.md) §2) see `new_redirect` and `old_redirect` in the page context, derived from the text, so a filter can catch a page turned into a redirect.
- **Recent changes** ([0020](0020-change-feeds.md)): a move's redirect `create` is one more row, tagged `move-redirect`, grouped with the move in the UI as [0010](0010-site-ui.md) §1 groups a job's rows.

## Alternatives considered

- **A `redirect` page operation**, a record of its own as entities have. Rejected: MediaWiki clients write redirects by saving text, and a bot that saves `#REDIRECT [[X]]` through `action=edit` must produce one. Reading it from the text keeps that contract with nothing to reconcile, which is the rule [0038](0038-page-metadata-and-categories.md) §3 gives categories.
- **An HTTP 301 for a followed view**, as forwarding namespaces answer. Rejected: MediaWiki answers 200 with the notice, readers expect the notice, and a redirect page is editable content that a 301 would hide.
- **Following redirect chains to their end.** Rejected: MediaWiki follows one hop, its reports assume one hop, and a chain of hops is a loop risk for no benefit over fixing double redirects.
- **Redirects in markdown** (a first line `→ [[Target]]`, say). Rejected: it would invent a convention, and markdown pages that want to point elsewhere can link.

## Consequences

- **Wikipedia's link graph works here.** Links through redirects land where they should, on local pages and on inherited ones alike, and a fork keeps the names that point at it.
- **Moves are two records, not one.** The moved page's history is unchanged; the redirect is a separate page that can be edited, deleted or pointed elsewhere, as on MediaWiki.
- **Clients that handle redirects on MediaWiki handle them here.** `redirects`, `prop=info`'s flag, `allredirects`, `apfilterredir` and `noredirect` all have MediaWiki's meaning.
- **The title resolver does one more read** for every `pages` title it resolves, against an indexed table.
- **Category redirects do not move members.** That is MediaWiki's behaviour, and a mapping's make-it-real job is the native way to retire a category.
- **User renames still break old links.** 0008 §6 stands; nothing here changes the erasability argument.

## Open questions

- **Q1. `$wgFixDoubleRedirects`.** Whether a move should offer to retarget the redirects that pointed at the old title, as MediaWiki's job does when that setting is on.
- **Q2. Soft redirects as data.** Whether `{{R from …}}` categorisation on Wikipedia redirects should map to page statements through category mappings, so a fork can query its redirect types.
- **Q3. Section redirects and anchors.** Whether a redirect whose fragment names a section that no longer exists should be reported, which needs section anchors in a projection.
- **Q4. Redirect pages in search as documents.** Whether a tenant may choose to index redirect pages themselves, for wikis that put content on them.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0008](0008-namespaces-and-document-pages.md) §4 | §3 | amends | 0008 A21 |
| [0008](0008-namespaces-and-document-pages.md) §3, §10 | §2, §5 | extends | 0008 A21 |
| [0008](0008-namespaces-and-document-pages.md) Q8 | §1 | settles | 0008 Q8 |
| [0012](0012-api-requirements.md) §4 | §7 | extends | 0012 A30 |
| [0013](0013-postgres-storage.md) §5.4 | §5 | extends | 0013 A21 |
| [0014](0014-caches-and-search.md) §7 | §5 | extends | 0014 A10 |
| [0016](0016-permissions-and-access-control.md) §2 | §8 | extends | 0016 A19 |
| [0030](0030-edit-filters.md) §2 | §9 | extends | 0030 A8 |
| [0038](0038-page-metadata-and-categories.md) Q5 | §4 | settles | 0038 Q5 |
| [0039](0039-files-and-media.md) §1 | §4 | extends | 0039 A5 |
| [0047](0047-special-pages.md) §10 | §6 | amends | 0047 A3 |
| [0047](0047-special-pages.md) §4.4 | §6 | extends | 0047 A3 |

## References

- [Help:Redirects](https://www.mediawiki.org/wiki/Help:Redirects) and [Manual:Redirect table](https://www.mediawiki.org/wiki/Manual:Redirect_table)
- [API:Redirects](https://www.mediawiki.org/wiki/API:Redirects), [API:Allredirects](https://www.mediawiki.org/wiki/API:Allredirects) and [API:Move](https://www.mediawiki.org/wiki/API:Move)
- MediaWiki core 1.43: `MovePage` (`delete-redirect`, `move-subpages`, `suppressredirect`), `RedirectLookup`, `WikitextContentHandler::getRedirectTarget`
- [Wikipedia:Redirect](https://en.wikipedia.org/wiki/Wikipedia:Redirect) and [Wikipedia:Categorizing redirects](https://en.wikipedia.org/wiki/Wikipedia:Categorizing_redirects) (`{{R from …}}`)
- CirrusSearch's `redirect` field, for §5

## Amendment log

### A1. A redirect is followed only to a readable target

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §6
- **Change:** extends §2
- **Summary:** Following requires `read` on the target; otherwise the redirect page is served as itself, with the target a red link.
