# 0051. Page redirects

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-30 (A8)
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

*Changed by A2.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §2.3.*

### 2. Following a redirect (extends 0008 §3)

*Changed by A1.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §2.4.*

### 3. Moves leave a redirect (amends 0008 §4)

*Changed by A3, A6, A7, A8.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §2.5, §3.2.*

### 4. Category, File and entity redirects (settles 0038 Q5; extends 0039 §1)

*Current text: [10](../architecture/10-pages-and-content-models.md) §2.6.*

### 5. Projection, caches and search (extends 0013 §5.4, 0008 §10 and 0014 §7)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.5, §4.7, §5, §6.1, §11.1, §12.1; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 6. Reports (extends 0047 §4.4; amends 0047 §10)

*Changed by A4.*

*Current text: [21](../architecture/21-special-pages.md) §2.4, §7, §8.*

### 7. API (extends 0012 §4)

*Current text: [18](../architecture/18-api.md) §2.3, §3.1, §3.2, §3.3, §3.4.*

### 8. Permissions (extends 0016 §2)

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §8.7.*

### 9. Filters and feeds (extends 0030 §2)

*Current text: [09](../architecture/09-security-and-moderation.md) §7.2; [16](../architecture/16-logs-feeds-and-notifications.md) §4.2.*

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

### A2. Redirect-capable namespaces come from the namespace catalogue

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §1
- **Summary:** §1's own list of the namespaces that may hold redirects is replaced by a reference to the namespace catalogue ([chapter 10 §1.4](../architecture/10-pages-and-content-models.md)): any namespace the catalogue lists with `wikitext` among its models may hold redirects, which includes the `/doc` pages of Module, Table, Scope and Query. The list ended with Table and omitted Scope and Query. (PENDING E15)

Replaced text (§1):

> Any namespace whose allowed models include `wikitext` may hold redirects ([0008](0008-namespaces-and-document-pages.md) §2): main, User, Project, File, Category, Template, Module (its `/doc` pages) and Table (its `/doc` pages).

### A3. A move without a redirect is `move/move` with `noredirect`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §3
- **Summary:** A move that leaves no redirect is logged as `move/move` with `noredirect: true` in its parameters (MediaWiki's `suppressredirect`); `move/move_redir` keeps MediaWiki's meaning, a move onto an existing redirect. The ledger row names §5, but the text it corrects is in §3, which is the section this entry names; §5 says nothing about the event. (PENDING E28)

Replaced text (§3):

> With `noredirect`, which needs `suppressredirect` (§8), only the `move` record is written and the event is `move/move_redir` where MediaWiki would so name it, or `move/move` with `noredirect` set.

### A4. The redirect reports are index-backed

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §6
- **Summary:** The four redirect reports are index-backed and live: their backing is the `index` row of [0047](0047-special-pages.md) §4.3, a keyset-paged query over `view.redirect` and its indexes, with no rows of their own, not a `projection` report. (PENDING F22)

Replaced text (§6):

> Each is a live projection ([0047](0047-special-pages.md) §4.3).

### A5. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [10](../architecture/10-pages-and-content-models.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [21](../architecture/21-special-pages.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A6. A redirect is left only where the old title may hold `wikitext`

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** corrects §3
- **Summary:** A move leaves a redirect only where the old title may hold `wikitext`, since only a `wikitext` page can be a redirect (§1): in namespace 0, User, Project, File, Template and Category, and at the `/doc` titles of Module, Table, Query and Scope. Where the old title cannot hold `wikitext` (a Table page moved to another Table title, a Scope, a Query, a Module's code page, a sprint subpage) the move behaves as `noredirect` without needing `suppressredirect`, and the `move/move` event says so with `noredirect: true`. Entity, keyed-type, Thread and Board namespaces disallow `move` altogether: an entity's title is its ID, a thread's title is minted from its subject and changed only by the thread's own `rename` record, and a board's title names the board, so `action=move` into, out of or within these namespaces is refused, as in a `virtual` or `resolver` namespace. The ledger names §2, but the text it corrects is §3's, which this entry names; §2 says nothing about moves. (REVIEW G43)

Replaced text ([10](../architecture/10-pages-and-content-models.md) §2.5, as it stood):

> **A move leaves a redirect at the old title by default,** as MediaWiki's does.

> - **Thread renames leave none** ([0019](0019-discussions.md) §3), for the reason user renames do.

### A7. The move record names the redirect it left

- **Date:** 2026-10-24
- **Source:** Direct: James, design discussion of 2026-10-24
- **Change:** amends §3
- **Summary:** A `move` record carries, beside the new title, the page ID of the redirect the move left at the old title, when it left one; the ID is minted before the move record is appended, and the redirect's `create` that follows in the same transaction takes it. The move's `move/move` event is projected from the move record alone, so without the field it could not say whether a redirect was left or which page it is (16 §2.3 asks for both): the alternatives were a projection that patches the move's row when the redirect's `create` arrives next, or a per-row lookup at read time, and both make a fact of the log recoverable only by reconstructing the transaction, and lose it if the `create` is erased. A `move` record without the field, every one written before this entry included, left no redirect. The old title in the event is read from the page's own history, the latest `create` or `move` before the record, rather than stored. Folded into 10 §2.5 and the record table of 10 §3.2.

Replaced text ([10](../architecture/10-pages-and-content-models.md) §2.5, as it stood):

> **A move is one record on the moved page.** Moving a page appends a `move` record with its new title, and nothing else about that page changes.

> 1. the page's `move` record with the new title (§3.2);
> 2. a `create` record for a **new page** at the old title, model `wikitext`, text `#REDIRECT [[New title]]`, attributed to the mover with the move's summary.

Replaced text ([10](../architecture/10-pages-and-content-models.md) §3.2, as it stood):

> | `move` | Sets a new title |

### A8. `action=move`: the calls made building it

- **Date:** 2026-10-30
- **Source:** Direct: Claude, while building `action=move`, 2026-10-24; unconfirmed
- **Change:** extends §3
- **Summary:** Logged from the working notes on their retirement (0099 §5, 24 §15); each call stands as decided until James confirms or overturns it, and the chapter text that states it is folded on confirmation where it is not already there. The calls:
  - The old title in the event is read from the page's own history, not stored in the record; it is `ts-from-ns`/`ts-from-title` beside MediaWiki's `target_ns`/`target_title`, and the event's `title` is the old title; `noredirect: true` and `suppressredirect: true` are both set when no redirect is left.
  - `move/move_redir` is not emitted: a move over a one-revision redirect deletes the redirect (its own `delete/delete`) and logs the move as `move/move`.
  - Rights checked: `move` and `edit` at the source, `createpage` and `edit` at the destination, plus `movefile`, `move-rootuserpages`, `move-categorypages`, `suppressredirect`, `move-subpages`, `delete-redirect`; the redirect's `create` at the old title is not separately checked for `createpage`.
  - Where a redirect may be left: the namespace's default model is `wikitext`, or it allows `wikitext` and the title ends in `/doc` (the `/doc` rule is hard-coded until `scatter-titles` parses `default_model_rules`).
  - Which pages move: any page whose model is a directly edited text model, to a namespace that allows that model; entities, threads, boards and talk pages are `immobilenamespace`.
  - `movetalk` answers `talkmove-error-code: ts-talk-follows` rather than silently.
  - `GET /activity?kind=log` includes the records that are also log events, as `list=logevents` already did.
  - `Special:MovePage/{title}` is the form; the redirect checkbox appears only to `suppressredirect` holders, the subpages box only to `move-subpages` holders.
