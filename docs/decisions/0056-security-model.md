# 0056. The security model

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-04 (A4)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0021](0021-notifications.md), [0023](0023-moderation.md), [0028](0028-tenancy-policy.md), [0033](0033-backend-stack.md), [0042](0042-template-expansion-and-parsoid.md), [0043](0043-lua-modules.md), [0051](0051-page-redirects.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0015](0015-record-format-and-partition-registry.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0022](0022-federation.md), [0024](0024-subsidiary-accounts.md), [0025](0025-oauth-server.md), [0030](0030-edit-filters.md), [0032](0032-sparql-update-stream.md), [0039](0039-files-and-media.md), [0040](0040-instance-prerogatives.md), [0045](0045-table-content-model.md), [0046](0046-primary-tenant.md), [0047](0047-special-pages.md), [0049](0049-boards.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

**MediaWiki's model is all or nothing.** `$wgGroupPermissions['*']['read']` decides whether anonymous readers see the wiki; `$wgWhitelistRead` names the pages they may see anyway, such as the main page and the login page. There is no finer grain in core. Extensions that add one (Lockdown, AccessControl, ApprovedRevs, the many "page security" extensions) restrict the page view and are then bypassed by everything else that reads a page: `action=raw`, `Special:Export`, the API, the search index, diffs, revision histories, recent changes and the Atom feeds, transclusion (`{{:Private page}}`), `Special:WhatLinksHere`, category listings, `Special:Random`, the parser cache, and a reverse proxy that cached the page before it was restricted. MediaWiki's own manual says so: *"MediaWiki is not designed to be a CMS, or to protect sensitive data"*, and lists these holes under "Security issues with authorization extensions". The architecture reads pages in many places and checks permissions in one, so an extension can only chase the readers.

**Triplespace already decided the opposite shape**, piece by piece, without naming it:

- Every permission decision is a function of records, and **ACLs restrict `read`** as well as write permissions, with conjunctive evaluation over enclosure ([0016](0016-permissions-and-access-control.md) §4). Deletion, hiding, suppression and username-hiding are `read` ACLs ([0023](0023-moderation.md) §1), and "a `read` ACL on a namespace makes it private" (0023 §2).
- **Every response is redacted for its viewer** ([0012](0012-api-requirements.md) §1, §8), there is a privacy test with a case for each kind of leak, and it is backed by database grants: the public role cannot read `private` ([0013](0013-postgres-storage.md) §4).
- **Shared caches hold only the public form**, and the search index holds only public data ([0014](0014-caches-and-search.md) §1, §7). The SPARQL Update stream and the dumps carry the public form ([0032](0032-sparql-update-stream.md) §1).
- **A tenant is a wiki with its own partitions, users and configuration** ([0018](0018-tenants.md) §1–4); nothing crosses a tenant boundary unless the tenancy policy says so ([0028](0028-tenancy-policy.md) §1), file storage is separate per tenant by default so that one tenant cannot observe another's uploads ([0039](0039-files-and-media.md) §4), and a "private tenant" is mentioned as one whose partitions carry the `private` export policy (0018 §5).
- The instance acts on a tenant's data only through recorded, signed **instance acts** ([0040](0040-instance-prerogatives.md) §1–3).
- A page that depends on a page a `read` ACL now covers is **re-rendered** ([0042](0042-template-expansion-and-parsoid.md) §10), and a tenant repository is read "with its ACLs evaluated as the source tenant's" ([0052](0052-page-repositories-and-title-inheritance.md) §7).

What no ADR says is the rule these are instances of, who the rule protects against whom, and what has to be true of a deployment for the rule to hold. Five gaps follow from that:

1. **There is no "private tenant".** 0018 §5 names one and nothing defines it: how a tenant becomes one, what its readers see, what leaves it.
2. **The only `read` restriction is moderation.** 0023 gives `read` ACLs to deletion and suppression, set with `delete`, `deleterevision`, `suppressrevision` and `hideuser`. A page that *exists* for a group and does not exist for everyone else, which the group edits, watches and searches, has no mechanism, and a private namespace built from a deletion-grade ACL would have no search and no feeds for its own readers.
3. **A restriction has one grain between a page and a namespace**: a page and its subpages. James asks for groupings of pages. Categories are the wrong tool: membership is content that any editor writes, so an editor could move a page out of a restricted category to expose it, or into one to hide it from review. Lockdown's category mode has exactly this flaw.
4. **Caches and search assume two viewers**, anonymous and administrator. A fifty-member private wiki would bypass every shared layer on every read and could not search itself.
5. **The deployment boundary is implicit.** Postgres `view`, OpenSearch, Valkey, the blob store and the backups all hold restricted data in the clear, and the model holds only if the server is the only thing that reads them. Nothing says so, and nothing checks.

### Direction

James's direction, from the design discussion of 2026-10-01:

- **"One concern is separation between tenants. In the most open scenario, all wikis are public wikis, with all pages being public. However, in the most closed scenario, many unrelated tenants (e.g. hosting customers) could share an instance and there should be no bleed-over between them (except preapproved global data like imported entities and mirrored wiki pages). Individual instances will want different settings."**
- **"MediaWiki's security model is all-or-nothing. You either have access to the whole wiki or you can't access anything not on the whitelist (like the main page). There are extensions that attempt to improve on this, but are ultimately limited by how easy it is to bypass MediaWiki's safeguards. Triplespace should not allow access to resources unless the requester is allowed, and this means a deployment will need to be set up a certain way to prevent bypassing the security."**
- **"Additionally, it should be possible to restrict individual pages or groupings of pages, not just a whole namespace."**

## Decision

### 1. The invariant

**Nothing leaves the instance that the principal receiving it may not read.** A response, a stream event, a search hit, a feed row, a log row, a notification, a rendered include, a cache entry, a dump, a federation activity and a file byte are each the result of evaluating `read` ([0016](0016-permissions-and-access-control.md) §4) for a **principal** against the **target** the data comes from. There is no route, job, cache, index or export that reads `view` or `log` on a principal's behalf without that evaluation, and there is no whitelist: MediaWiki's `$wgWhitelistRead` has no counterpart, because a restriction that admits exceptions by title is a restriction that content can escape.

**Default is deny for everything a restriction covers; default is public for everything else.** A tenant with no `read` restriction is a public wiki and nothing in this ADR costs it anything. A `read` restriction applies to its target and everything the target encloses, and nothing loosens it (0016 §4). The two scenarios of the Direction are the two ends of one setting: an instance of public tenants has no restrictions; a hosting instance gives each tenant a restriction on the tenant itself (§3), and from then on the tenants cannot see each other, except through the global data the tenancy policy already lets cross ([0028](0028-tenancy-policy.md) §1, §5): mirrored entities, page repositories and shared files, which are public at their source and cross as public form only (§6).

**Principals.** A principal is what a request acts as, after authentication:

| Principal | Groups | Capabilities |
|---|---|---|
| Anonymous, or a request before a temporary account exists | `universe` | `universe`'s permissions |
| A session of a tenant account | Its memberships, plus the global groups of its linked farm account where the policy has them ([0028](0028-tenancy-policy.md) §3) | Effective permissions (0016 §3) |
| A subsidiary acting through an API key or an OAuth token | The subsidiary's memberships | Effective permissions ∩ the credential's grants ([0024](0024-subsidiary-accounts.md) §3–4, [0025](0025-oauth-server.md) §1). A grant can withhold a capability; it cannot add a group, so it never widens what an ACL admits |
| Another tenant, or another instance, reading this tenant as a provider, page repository, template repository or file repository | `universe` | Public form only (§6) |
| A federation peer | `universe` for outbound; the `federated` surrogate's groups for inbound ([0022](0022-federation.md) §8) | As anonymous |
| The renderer, the indexer, the feed projection, the notifier | None of its own: a **derived-output** principal that carries the visibility of what it produces (§6) | Writes outputs tagged with their visibility; serves nothing itself |
| The instance operator | None in any tenant's UI | Instance acts ([0040](0040-instance-prerogatives.md)) and out-of-band access inside the boundary (§10) |

A session is per tenant host ([0018](0018-tenants.md) §4, §11): a cookie for `librarybase.org` is nothing on `example.wiki`, and a farm issuer gives a person a login on each tenant, not a session across them. Session cookies are `Secure`, `HttpOnly` and `SameSite=Lax`; every write carries a CSRF token as MediaWiki's do; bearer credentials are never accepted in a URL.

### 2. Visibility: the `read` restrictions that enclose a target

The **visibility** of a target *T* is the set of `read` ACLs on *T* and on every target that encloses *T*, along the axes [0016](0016-permissions-and-access-control.md) §4 and [0023](0023-moderation.md) §2 fix (containment, predicate, content) and the two targets this ADR adds (§3). A principal **may read** *T* when it holds `read` and satisfies every ACL in that set. Writing `groups(T)` for the groups those ACLs name, the test is membership in each. The visibility of a public target is empty.

The set is what everything downstream keys on: a cache entry is good for every principal that satisfies the set it was computed under (§7); a search document is returned only to principals that satisfy its set (§8); a derived output is readable only where every input's set is satisfied (§6).

**Two kinds of `read` restriction, one record.** The payload is `scatter:v0/acl` either way (0023 §3); the intent, the right that sets it and what outsiders see differ.

| | Moderation (0023) | Confidential (this ADR) |
|---|---|---|
| Means | The target has been removed: deleted, hidden, suppressed | The target exists for a group and for nobody else |
| Set with | `delete`, `deleterevision`, `suppressrevision`, `hideuser`, by target | `protect`, with `read=` in the restriction (§4) |
| Expiry | None on a deletion; as MediaWiki on a hide | Allowed, as on any protection |
| For the group | The deleted-page notice, the hidden history, `prop=deletedrevisions` | An ordinary live target: edited, watched, searched, fed, notified, rendered |
| For everyone else | MediaWiki's notices: "this page has been deleted", with the public deletion log entry; a revision marked hidden; `(username removed)` | **Absent** (§5): indistinguishable from a target that never existed |
| In search, feeds, notifications | Not indexed, not fed, not delivered, as now | Indexed, fed and delivered to the group, with the visibility set attached |
| Log event | Public, naming the target, as MediaWiki's deletion and protection logs are | Readable only by principals that may read the target (§5) |

Where both apply, nothing new is evaluated: a deleted confidential page is seen by the intersection of its deletion group and its confidential group, which is what conjunction gives.

### 3. Two targets: the tenant and the set (extends 0016 §4; extends 0023 §2)

*Changed by A3, A4.*

| Target kind | Key | Partition | Encloses | Set with |
|---|---|---|---|---|
| `tenant` | `acl:tenant:{slug}` | The tenant's `config` | Every partition of the tenant, and so every page, entity, thread, file, record and log event in it | `ts-config` on the tenant; a locked template under `config.template = locks` makes it a prerogative ([0028](0028-tenancy-policy.md) §8) |
| `set` | `acl:set:{id}` | The tenant's `log`, as every moderation-shaped ACL is ([0023](0023-moderation.md) §3) | Its **members**: pages (each with its subpages, when the namespace has them), entities and threads, listed by ID in the record's content | `protect` |

**A tenant's visibility is its `tenant` ACL.** A tenant with none is **public**. A tenant whose `tenant` ACL restricts `read` is **private**: readable by the group the ACL names, `user` by default, which is every account of the tenant ([0016](0016-permissions-and-access-control.md) §3), or any group the tenant chooses for a narrower membership. The ACL lives in `config` because it is not secret: that a tenant is private is the first thing a visitor learns (§5). A private tenant usually also removes `createaccount` from `universe`, so that accounts are made by its bureaucrats or by invitation; the tenant settings page offers both switches together. `temp` is irrelevant on a private tenant, since a temporary account is created by an edit and nobody outside `user` can read what they would edit.

What a private tenant does not have, because each of these is a public form of its data: a public dump or bundle ([0006](0006-log-integrity-and-erasure.md) §9; its partitions are exported as `private` for every purpose but the operator's own backups and a tenant move, [0018](0018-tenants.md) §10), an update stream ([0032](0032-sparql-update-stream.md) §6), a SPARQL endpoint fed from either or the query service ([0059](0059-query-service.md) §4, whose Q1 asks about a store of its own behind the evaluator), a provider code (0018 §5), `pages.share` or a file or template repository that other tenants read ([0042](0042-template-expansion-and-parsoid.md) §2, §11, [0052](0052-page-repositories-and-title-inheritance.md) §7), outbound federation ([0022](0022-federation.md)), a sitemap, or indexable pages: every response carries `noindex`. It may still *read* everything the tenancy policy lets it: Wikidata, a provider tenant, Wikipedia as a page repository. Public data flows in; nothing flows out.

**A set is a grouping that only `protect` can change.** Its record carries a `name`, shown where the restriction is shown (§13), and `members`. Members are IDs, never titles, because a key is never content ([0006](0006-log-integrity-and-erasure.md) §3) and a move must not change what is restricted. A set is flat: it does not contain sets or namespaces, which have ACLs of their own, and a target may be in any number of sets. Adding or removing a member is a new version of the record, needs `protect`, and projects as `protect/modify` with the member named; the set's `read` restriction is in the same record as its membership, so there is no moment at which a member is listed and unrestricted. The ID is taken from the page-ID sequence ([0015](0015-record-format-and-partition-registry.md) §2) as a create-protection reserves one ([0023](0023-moderation.md) §2); a set is not a page and has no title.

Why not a category, a title prefix or a tag: all three are content. A category link is written by whoever may edit the page; a prefix changes with a move; a tag on a revision is a claim by its author. A grouping that restricts reading has to be changed only by the right that sets restrictions, and only by ID.

**A scope ([0060](0060-scopes.md) §1) is not a set.** It is content: a page that anyone with `edit` may change, naming subjects by title and by query. It restricts nothing, is never a target, and shares no table with sets.

**Enclosure after this ADR**, for the containment axis: tenant ⊃ graph ⊃ namespace ⊃ page ⊃ subpage; tenant ⊃ set ⊃ member; a talk page or board ⊃ the threads homed there ([0049](0049-boards.md) §7); entity ⊃ statement; the predicate and content axes of 0023 §2 unchanged. Evaluation stays conjunctive and nothing loosens: a page in a public namespace of a private tenant is private; a public page added to a restricted set becomes restricted; a page in two sets is read by members of both groups.

### 4. Who may restrict what (amends 0016 §2 and §4; amends 0023 §1)

- **`protect` may restrict `read`.** `action=protect` and the Protect dialog accept `read` beside `edit` and `move`, with a group and an expiry, on a page, entity, namespace, thread, talk page, board or set. This is the confidential restriction of §2. The row for `protect` in [0016](0016-permissions-and-access-control.md) §2 says so, and 0016 §4's sentence that a `read` ACL needs `delete`, `deleterevision`, `suppressrevision` or `hideuser` now applies to the moderation kind only.
- **The actor must be in the group they restrict to.** A `read` restriction naming a group the actor is not a member of is refused with `ts-locked-out`. This is the check MediaWiki makes for protection levels and it keeps an administrator from hiding a page from themselves; it also keeps `owner` honest: `owner` holds every permission but is a member of no group it has not joined, and a confidential restriction naming `suppress` hides the target from `owner` as a suppression does today. A tenant that wants its owners to see everything puts them in the groups.
- **A tenant's visibility needs `ts-config`,** and the instance may lock it (§3). Making a public tenant private purges its public forms (§7) and withdraws it from every reader it had (§6); the settings page says so before it saves.
- **The tenancy policy decides whether tenants may restrict at all** (§11, `security.restrictions`). A community farm in the Wikimedia tradition sets `none`, and `protect` with `read=` is refused there with `ts-policy`.
- **Nothing else changes.** Deletion, hiding and suppression keep their rights and their public notices ([0023](0023-moderation.md) §1); graph ACLs stay `ts-config`; the `blob` target stays the operator's ([0039](0039-files-and-media.md) §10).

### 5. Absence: what a principal outside the group sees (extends 0012 §8)

**A target under a confidential restriction the principal does not satisfy is absent.** Every route answers as it would for a target that does not exist: `missing` in the Action API, 404 from REST with the body a missing title gets, a red link in rendered text, `nil` from Lua, an empty stack in title inheritance ([0052](0052-page-repositories-and-title-inheritance.md) §3). The response is the same status, shape and cache class as a genuine miss, so that a probe cannot tell them apart; [0039](0039-files-and-media.md) §7 already answers a file request this way, 404 and never 403. This differs from moderation deliberately: a deleted page says it was deleted, because MediaWiki users and tools expect that, and because a deletion is a public act; a confidential page says nothing, because its existence is the first fact being protected.

**Everything that lists, counts or links filters by the principal.** Recent changes, watchlists, histories, contributions and the activity stream ([0020](0020-change-feeds.md)), the logs ([0011](0011-logs.md)), search and suggestions (§8), category members and counts, `Special:WhatLinksHere` and `list=backlinks`, `list=allpages`, `Special:Random`, `prop=categoryinfo`, site statistics ([0047](0047-special-pages.md) §13), file usage, template usage, the report pages (0047 §4) and the farm-wide views ([0028](0028-tenancy-policy.md) §9) show rows whose target the principal may read and no others; counts are counts of those rows. `view.site_stats` counts public targets only, so a public number never moves when a private page is created.

**A log event whose target is restricted is restricted with it.** The `protect/protect` event that creates a confidential restriction, and every `move`, `delete`, `patrol`, `upload`, `thread` and filter-hit event on the target afterwards, is shown to principals that may read the target and absent for others. MediaWiki's protection log announces every protection by title; here that would announce every secret. An event whose target is a set names the set and its members, and is readable by those who may read the set. `Special:ProtectedPages` lists each restriction to principals that may read its target.

**A readable thing that refers to a restricted thing shows the reference, not the referent.** A wikilink in a readable page renders as a link to a missing page; an entity value whose entity is restricted shows the bare ID as a missing entity does; a sitelink from a public tenant to a private tenant's page is a URL that answers 404. The reference is the readable page's own content and is not hidden; what it points at is.

**What this does not hide**, stated so that nobody relies on it:

- **That a title is taken.** A `create` at a title that exists under a restriction the actor does not satisfy is refused with `ts-restricted-title`, which is also, from this ADR, the refusal for a create-protected title ([0023](0023-moderation.md) §2), so the two cases are indistinguishable. The actor learns that *something* reserves the title and nothing else. The alternative, two pages under one title, is not available; the leak is bounded by the `createpage` rate class and shows in the filter log.
- **Gaps.** Page, revision and log IDs are sequences per tenant ([0015](0015-record-format-and-partition-registry.md) §2), so a restricted edit leaves a gap, as an erasure does ([0016](0016-permissions-and-access-control.md) §6). Gaps reveal counts, not content.
- **Timing and size.** Nothing is padded or delayed.
- **The operator.** Everything inside the boundary of §10 is readable by whoever administers it, in the database and the backups. Instance acts are the only *UI* path, and they are recorded ([0040](0040-instance-prerogatives.md) §4); the rest is the operator's own controls and the hosting agreement, which this ADR cannot enforce and does not pretend to.

### 6. Flow: derived output inherits the visibility of its inputs (amends 0042 §3; extends 0043 §5; extends 0051 §2; amends 0028 §5)

Anything computed from more than one target can carry data from one into another: transclusion, `#invoke` and `mw.title.getContent`, `#property` and `mw.wikibase`, table rows ([0045](0045-table-content-model.md)), a thread listed on a board ([0049](0049-boards.md)), a followed redirect, TemplateData and TemplateStyles, a search document, a feed row, a diff, a notification. Two rules close every such path.

**Rule 1: an output is readable only where every input is.** A search document, feed row, diff, notification or rendered page carries the union of its inputs' visibility sets, and is served to a principal that satisfies all of them. For outputs produced once per request (a diff, an entity's resolved view with its labels) this is evaluation at serving time. For outputs produced once and stored (a rendered page, a search document, a feed row, a notification) the producer runs as a derived-output principal (§1) and writes the set beside the output; serving filters on it.

**Rule 2: an include may not widen.** A rendered page is produced once per visibility and served from cache (§7), so a page *A* may include a page or entity *B* only if `groups(B) ⊆ groups(A)`: every reader of *A* could read *B* anyway. Otherwise the include behaves as if *B* did not exist, for every viewer: a red link from `{{:B}}`, `nil` from `getContent`, an empty row in a table, a missing entity from `#property`, no stylesheet from a restricted `sanitized-css` page. The subset test runs in the expander through the host, is cached in L0, and is re-evaluated when either side's restrictions change, which is the refresh trigger [0042](0042-template-expansion-and-parsoid.md) §10 already has. The common cases cost nothing: a public template includes nothing restricted, and a restricted page includes public templates freely, because ∅ is a subset of everything. A thread listed on a board is shown on the board only to principals that may read the thread's home (0049 §7 already evaluates on the home); the listing is absent for others.

**Redirects.** The resolver follows a redirect ([0051](0051-page-redirects.md) §2) only when the principal may read the target; otherwise it answers with the redirect page itself, whose text names the target as a red link. The redirect page is content the principal may read; the target is not.

**Cross-tenant reads are anonymous reads.** A tenant reading another as a provider ([0018](0018-tenants.md) §5), a page repository ([0052](0052-page-repositories-and-title-inheritance.md) §7), a template repository ([0042](0042-template-expansion-and-parsoid.md) §11) or a file repository ([0039](0039-files-and-media.md) §11) is the `universe` principal of the source: it sees the source's public form and nothing under any restriction. This replaces [0028](0028-tenancy-policy.md) §5's "reading is all or nothing; a provider does not restrict individual entities": a provider may now keep confidential entities, and they are simply not part of what it provides. A private tenant provides nothing (§3). Reader lists (0028 §5) still decide *which* tenants may read; this decides *what* they read.

**Notifications.** A mention, a `talk` message or a `watch` change on a target the addressed account may not read addresses nobody ([0021](0021-notifications.md) §2): no "you were mentioned on a page you cannot see". A notification already delivered whose target becomes restricted is removed from the inbox by the purge of §7, as a hidden one is today. A watch on a target that becomes restricted stays in `private` and produces nothing until the account may read it again.

**Exports.** Public dumps, bundles under the default export policies and the update stream carry the ∅ form only, as they do now ([0032](0032-sparql-update-stream.md) §1); a confidential target is as absent from them as a suppressed one. The operator's full bundle and a tenant move carry everything ([0018](0018-tenants.md) §10). Outbound federation publishes activities for targets with empty visibility only; an inbound reply to a restricted thread is evaluated on the thread's home with the `federated` surrogate's groups ([0022](0022-federation.md) §8), which a tenant's ACL must name for it to pass.

**Edit filters** see the whole change, as they must ([0030](0030-edit-filters.md) §3); a hit's row in the filter log is shown to principals that may read the target, and a private filter's details stay with `abusefilter-view-private` as now. **Jobs** keep [0012](0012-api-requirements.md) §8's rule for rejects files: the actor and `ts-viewrejects`.

### 7. Caches are keyed by visibility (amends 0014 §1, §4)

**Principle 3 of [0014](0014-caches-and-search.md) §1 becomes: shared caches hold the form computed for a visibility set, keyed by that set, and the public form is the entry for the empty set.** An entry computed under set *V* may be served to any principal that satisfies *V*, so a private wiki of fifty members caches as well as a public one. Administrators' moderation views, which include hidden fields and erased gaps' reasons, still bypass every shared layer, as do `/account` and `/auth`.

- **Keys.** Every key in 0014 §4 whose value depends on what the viewer may read gains a `{vis}` segment: a short hash of the sorted group names of the visibility set and the tenant's visibility epoch (below), `-` for the empty set. `e:`, `t:`, `prov:`, `p:`, `d:`, `sug:` and `css:` carry it; `s:`, `rl:` and `up:` do not. A page's render carries the set of its own visibility, which by Rule 2 of §6 bounds every include.
- **L2 and the proxy.** Only a response with empty visibility carries public `Cache-Control`; every other response is `private, no-cache` with an `ETag`, as authenticated responses already are ([0012](0012-api-requirements.md) §8). The proxy and the CDN therefore never hold a restricted form; L1 may, because it is inside the boundary (§10).
- **Purges.** A `read` ACL written, changed or retired on target *T* bumps *T*'s version, which makes every entry for *T* unreachable (0014 §1, principle 1). On a namespace, set or tenant, it bumps the tenant's **visibility epoch**, a counter in `view.tenant` that is part of every `{vis}` hash for that tenant, so that every entry under the enclosing target lapses at once. The trade is coarse purges for correctness, and these ACLs change rarely. Both run through the same path as erasure (0014 §5), including the inbox purge of §6.
- **L0.** A session's effective permissions and group memberships are cached for seconds (0014 §2); the subset test of §6 and the visibility set of recently read targets join them.

### 8. Search indexes restricted content, with a filter (amends 0014 §7)

**Documents carry `read_groups`.** Each document in a tenant's index gains `read_groups`: the sorted group names of its target's visibility set, empty for a public target. A query adds one filter: `read_groups` is empty, or every value in it is among the principal's groups, which OpenSearch expresses as a `terms_set` query whose `minimum_should_match` is the field's length. Anonymous queries match the empty case only. `/suggest`, `wbsearchentities`, `list=search` and the farm-wide `msearch` ([0028](0028-tenancy-policy.md) §9) all apply it; the Postgres fallback (0014 §8) applies the same predicate in SQL.

**Moderation stays out of the index.** A deleted, hidden or suppressed target is not indexed, as now: the moderation kind has no readers who search for it. Only the confidential kind is indexed, so a private wiki's members can search their wiki.

**Consequences for the deployment.** The index now holds text that not everyone may read. It is inside the boundary of §10, the server is its only client, and nothing else, no dashboard and no reporting tool, is pointed at it. The per-provider shared indexes ([0018](0018-tenants.md) §6) hold public form only, because they are built from public data; the per-tenant indexes are where `read_groups` is non-empty.

**Re-indexing.** A `read` ACL change re-indexes the affected documents on the trigger of §7, with the same versioning 0014 §7 uses, and a document whose target becomes moderation-hidden is deleted as now.

Alternatives rejected: one index per group (combinatorial, and the group of a document is not fixed), and leaving restricted content out of the index (a private wiki that cannot search itself is not a wiki its members can use).

### 9. Writes

The write path of [0013](0013-postgres-storage.md) §7 is unchanged: authentication, grants, rate limit, ACLs, filters, append. Two rules make it consistent with reading:

- **A principal that may not read a target may not write to it,** whatever `edit` ACL it would satisfy. The refusal is the missing-target refusal (§5), not `protectedpage`.
- **A write may not reference what the writer may not read into a wider place.** This is Rule 2 of §6 on the way in: a transclusion, `#invoke`, redirect, set membership, board listing or thread attachment that would include a more restricted target into a less restricted one is accepted as text (an editor may type `{{:Board minutes}}`) and simply does not include, so the save never widens. The one place a write *moves* restricted content, forking or copying a page ([0054](0054-forking-a-mirrored-page.md)), copies only what the forking principal may read.

### 10. The deployment boundary (extends 0033 §12)

*Changed by A1, A2, A3.*

The evaluator runs in `triplespace-server`. Everything behind it holds restricted data in the clear: Postgres `log` and `view`, OpenSearch (§8), Valkey (§7), the blob store, the render and job queues, the backups, the access logs. **The model holds only inside a deployment where the Triplespace services are the only readers of those stores.** That is a property of how the instance is set up, and Triplespace checks what it can and says what it cannot.

**Requirements.** Each is a line in `triplespace-cli instance check`, which reports `pass`, `fail` or `attest`: the last for a requirement the server cannot verify from inside, which the operator confirms with `instance check --attest {name}` and which is recorded in the instance `config` with the operator and the date.

| # | Requirement | Why | Check |
|---|---|---|---|
| 1 | The database roles are those of [0013](0013-postgres-storage.md) §4: the public role has no privilege on `private`, no role Triplespace uses is a superuser, and no other application connects to `view` or `log` | Every row in `view` is readable to a database client; the evaluator is in the server, not the database | `pass`/`fail` on grants, read from `pg_catalog`; `attest` that nothing else holds a connection |
| 2 | OpenSearch is reachable only from the servers and requires credentials; no dashboard or other client is configured against it | The index holds restricted text (§8) | `fail` if the configured endpoint is a public address or accepts an unauthenticated request; `attest` for other clients |
| 3 | Valkey requires `AUTH` or runs on a loopback or private address, and persists, if at all, to a volume with the same protection as the database | Sessions and restricted forms live there (§7) | `fail` on a public address or an unauthenticated connection |
| 4 | The blob store is private. File bytes and thumbnails are served by the binary after `read` is evaluated, and a version only some may read only from a signed URL valid for minutes, exactly as [0039](0039-files-and-media.md) §7 has it; the media origin is separate from the wiki origin | A public bucket is a public dump of every tenant's files | `fail` if an object URL in the store answers 200 without a signature; `attest` for a media base that is same-origin with the wiki, which 0039 §7 recommends against |
| 5 | The server rejects a request whose `Host` is not a registered tenant base or the farm base, with 421, and believes `X-Forwarded-For`, `X-Forwarded-Host` and `X-Forwarded-Proto` only from hops it trusts: by address in `server.trusted_proxies` (the default), or by a forwarder key, read from the right ([0057](0057-web-tier.md) §10) | The host selects the tenant ([0018](0018-tenants.md) §11); IP blocks, rate limits and filter IP rows depend on the client address ([0016](0016-permissions-and-access-control.md) §3, [0024](0024-subsidiary-accounts.md) §5, [0030](0030-edit-filters.md) §11) | `pass`/`fail` by sending a request with an unregistered host, and one with a forged `X-Forwarded-For` from an untrusted address; `fail` if the server is behind a proxy and trusts no hop; `attest`, where trust is by address, that only trusted hops can reach the server's listener |
| 6 | The reverse proxy or CDN forwards `Host` unchanged, caches only responses with public `Cache-Control` and never one carrying `Set-Cookie` or answering a request with `Authorization` or a session cookie, strips inbound `X-Forwarded-*` from clients before setting its own, terminates TLS and sends HSTS | L2 may hold public form only (§7); a proxy that caches an authenticated page serves it to the next visitor | `attest`; the privacy test of §15 exercises the first two through the proxy when `instance check --through {url}` is given |
| 7 | A SPARQL endpoint, QLever or any other external index, and the embedded query store ([0059](0059-query-service.md) §2), is loaded from the public dump or the update stream, or from `view.rdf_delta`, which carries the same rows, never from `view` or `log` otherwise | Those carry ∅ form by construction ([0032](0032-sparql-update-stream.md) §1); a direct load bypasses the evaluator | `attest` |
| 8 | The Parsoid service and any other helper process reach content only through the server's internal endpoint, with a service credential, on a private address | A renderer that reads the database reads everything | `fail` if the configured Parsoid endpoint is a public address |
| 9 | `/metrics`, health and debug routes are served on `server.admin_listen`, a separate listener, never on the public one | They expose names, counts and timings | `fail` if the admin listener is the public one |
| 10 | Backups of Postgres, the blob store and Valkey's persistence are encrypted at rest and held with access equal to the database's | They are the whole instance, `private` included | `attest` |
| 11 | Secrets come from files or the environment, never from the command line ([0033](0033-backend-stack.md) §12), and the instance key is in a file readable by the server alone | A command line is visible to every process | `fail` on a world-readable key file |
| 12 | A web tier ([0057](0057-web-tier.md)) holds no credential of the instance's beyond an optional forwarder key, reads no store of it, and reaches the API over TLS unless the API's address resolves only to internal addresses; it appends to forwarded headers and judges none of them (line 5), and the Valkey of its response cache is held to line 3 | It sees every viewer's cookie in transit | `fail` if the web tier's cache names a Valkey that fails line 3, or its `web.api` is plain HTTP to an address that is not internal |
| 13 | The query service's remote store ([0059](0059-query-service.md) §4) is reachable only from the servers and requires credentials, and every query against it passes through the service, which fixes the tenant's dataset; the embedded store's directory is readable by the server alone | A shared store holds every public tenant's graphs; a query that reached it directly would choose its own dataset | `fail` if `query.endpoint` is a public address or accepts an unauthenticated request; `fail` on a world-readable `query.path`; `attest` for other clients |

**Modes.** `server.mode` is `production` or `development`. In `production`, the server refuses to start while any `fail` stands and logs every `attest` not yet given; `development` starts anyway and marks `siprop=triplespace` with `insecure: true`. `instance create` writes `production`.

**What the boundary is not.** It is not an encryption scheme: restricted content is plaintext in the stores, because rendering, searching and diffing need it, and because key management per group would move the problem rather than solve it (Alternatives). It is not a guarantee against the operator (§5). It is the statement that the server is the reference monitor, and the list of what has to be true for that to mean anything.

### 11. The tenancy policy: `security.restrictions` (extends 0028 §1; settles 0028 Q2)

One switch joins the policy of [0028](0028-tenancy-policy.md) §1:

| Switch | Values | `isolated` | `community` | `enterprise` |
|---|---|---|---|---|
| `security.restrictions` | `none`: tenants are public and `read` is restricted by moderation only; `tenant`: a tenant may be private as a whole, and nothing finer; `any`: tenants may also restrict pages, entities, namespaces and sets | `any` | `none` | `any` |

`isolated` is the hosting preset (0028 Context) and the single-tenant default: whoever operates the instance decides. `community` follows the Wikimedia practice that a public wiki has no private pages; a community farm that wants them sets `tenant` or `any`. The switch is reported in `siprop=triplespace` beside the others, and changing it from `any` to `none` is refused while any confidential restriction exists; `tenancy check` lists them.

**Global groups in ACLs** (0028 Q2): with `groups.global = inherited`, an ACL may name a global group. Group names are one namespace on a farm: a tenant cannot create a group with a global group's name, and a global group cannot take a name any tenant uses, which `tenancy check` verifies as it does account names. A hosting instance has no global groups, so there is no group that reads every tenant; a community farm's `steward` reads a private tenant only where that tenant's ACL names it, and an enterprise's `platform-admin` likewise. The read-everything role, where an organisation wants one, is a decision each tenant's ACL makes, or a locked template makes for it ([0028](0028-tenancy-policy.md) §8).

### 12. Instance acts

An instance act on restricted content is evaluated or written as [0040](0040-instance-prerogatives.md) §1 has it, with its authority record in the instance `log`; takedowns and expunges reach every tenant's files whatever their visibility ([0039](0039-files-and-media.md) §10). The authority record names the target; its attestation is visible to `ts-viewoperator` (0040 §9), and the record itself is readable at the farm base by principals that may read the target on its tenant and, since an operator needs to see what they did, by `ts-viewoperator`. The instance-action list at the farm base filters as every list does (§5).

### 13. API and UI (extends 0012 §4; extends 0016 §7; extends 0023 §8 and §9)

- **Action API.** `action=protect` accepts `read={group}` in `protections`, with `expiry` as for the others; MediaWiki clients that do not know it never send it. `prop=info&inprop=protection` reports a `read` restriction as a `protection` entry of type `read` to principals that may read the page, which is every principal that can ask about it. `list=protectedpages` gains `prtype=read`. A new `list=protectedsets` lists sets with their names, groups, expiries and members, filtered as §5 requires. `meta=siteinfo` on a private tenant answers an outsider with the `general` section's name, language and the fact that the wiki is private, and nothing else; `siprop=triplespace` reports `visibility`, `security.restrictions` and, in `development` mode, `insecure`.
- **REST**, under `triplespace/v0`: `GET`, `PUT` and `DELETE /acl/tenant/{slug}` and `/acl/set/{id}`, `POST /acl/set` to create a set, `POST /acl/set/{id}/members` and `DELETE /acl/set/{id}/members/{kind}/{id}`; `GET /acl/{kind}/{id}` on any target also returns `visibility`, the resolved set, to a principal that may read the target.
- **Site UI** ([0010](0010-site-ui.md)). The Protect dialog gains a **Read** row with a group picker and the lock-out warning of §4. **Sets** are managed from `Special:ProtectedPages`, which gains a Sets tab: create, rename, add and remove members, restrict. Every page, entity or thread whose visibility is non-empty shows a **Restricted** pill with a lock icon in its identity line, naming the groups on hover and the sets it is in; readers should know they are reading something not everyone can. The tenant settings page (`Special:Tenancy` on a farm, the site settings otherwise) has **Visibility**: *Anyone*, *Accounts on this wiki* (`user`), *A group…*, with the account-creation switch beside it and a note on what a private wiki gives up (§3). A visitor to a private tenant who may not read it sees a **landing page**: the site name and logo, `site.landing_text`, and Log in; nothing else of the tenant is served, special pages included, except `Special:UserLogin`, the OAuth routes and `/.well-known/`.
- **`instance check`** is also `GET /instance/check` at the farm base, for `owner`: the table of §10 with each line's state, and the attestations with who gave them and when. No special page is added; the CLI and the route are the operator's tools.

### 14. Storage (extends 0013 §5.6)

| Schema | Table | Serves |
|---|---|---|
| `view` | `set_member (tenant, set_id, kind, id)`, indexed by `(tenant, kind, id)` | The sets a target is in, one lookup per read; written by the ACL projection |
| `view` | `read_groups text[]` on `activity`, and `visibility_epoch integer` on `tenant` | Feed and log rows filtered by principal (§5); the purge epoch of §7 |
| `view` | `acl` (existing) gains the `tenant` and `set` target kinds and a `kind = confidential \| moderation` column, derived from the right that set the record | The two kinds of §2 |

The visibility set of a target is computed at read time from the enclosure chain: the tenant row, the namespace, the sets `set_member` names, the page or entity, and for a statement its property and entity, which is the one indexed query [0023](0023-moderation.md) §2 already describes with two more terms. It is cached in L0 per target version and tenant epoch. Feed rows and search documents store the set at projection time and are re-projected on change (§7, §8). Nothing is added to `private`; a watch, inbox row or preference is already the holder's alone, and filtering by what the holder may read happens on the public side of the join ([0020](0020-change-feeds.md) §3).

### 15. The privacy test (extends 0012 §8)

*Changed by A2.*

The test of [0012](0012-api-requirements.md) §8 gains these cases, each run for an anonymous principal, an account outside the group, a subsidiary of an account inside the group whose grants lack `basic`, and another tenant reading through every repository kind:

1. A confidential page, entity, thread, statement and property, a set, a namespace and a whole private tenant: every route that can name the target answers exactly as for a missing target; byte-equal bodies and equal status codes.
2. No row for the target in recent changes, watchlist, logs, contributions, category members, backlinks, `allpages`, reports, site statistics or the activity stream; no search hit or suggestion; no notification delivered; no row in the filter log.
3. No cache entry with `{vis} = -` holds the target's data; no public dump, bundle under default export policy, update-stream event or outbound federation activity contains it.
4. A public page that transcludes, invokes, redirects to or tabulates the target renders it as missing for every viewer, including a member of the group.
5. A `create` at the target's title fails with `ts-restricted-title` and nothing more.
6. `instance check` fails on: a public-role grant on `private`, a superuser role in the server's configuration, a store on a public address without credentials, an unregistered `Host` answered with anything but 421, an empty trust list behind a proxy, a public admin listener, a world-readable key file.
7. Making a public tenant private, and retiring that, purges and restores its public forms within the TTL ceiling, and no public cache or index entry survives the first.

Where the site is served by a web tier ([0057](0057-web-tier.md)), every case is also run through it: a confidential target's page, and its `action=render`, are byte-equal to a missing target's, and case 7 covers the web tier's response cache.

### 16. Crates (amends 0005 §2)

*Changed by A2.*

| Crate | Change |
|---|---|
| `scatter-actors` | The `tenant` and `set` ACL targets; the visibility set of a target and the two kinds of `read` restriction; the subset test of §6; the lock-out check of §4 |
| `triplespace-projections` | `set_member`, `read_groups` on activity rows, the visibility epoch, the confidential/moderation kind on `acl`; re-projection on `read` ACL change |
| `triplespace-cache` | The `{vis}` key segment, the epoch, and the purge of §7 |
| `triplespace-search` | `read_groups` on documents and the `terms_set` filter of §8; the fallback's SQL predicate |
| `scatter-wikitext-expand`, `triplespace-scribunto`, `triplespace-render` | The include rule of §6 through the host: a refused include is a missing page, `nil`, an empty row or no stylesheet |
| `triplespace-server` | `server.mode`, `server.trusted_proxies`, `server.admin_listen`, 421 on an unregistered host, the internal endpoint for Parsoid |
| `triplespace-ui` | The landing page, rendered wherever the site is served ([0057](0057-web-tier.md) §3) |
| `triplespace-cli` | `instance check`, `--attest`, `--through`; `tenancy check` extended to confidential restrictions and group names |

## Alternatives considered

- **MediaWiki's model: `read` for `*`, a whitelist of titles.** Rejected by the Direction. It is also what makes every page-restriction extension bypassable: the whitelist is a second rule evaluated in one place, while reads happen in many.
- **Deny rules and per-account ACLs.** A deny would reintroduce precedence, which [0016](0016-permissions-and-access-control.md) §4 removed; a per-account entry is a group of one and costs nothing to make. Groups only.
- **A separate "private wiki" mode** instead of a `tenant` target. It would be a second evaluation path with its own leaks. Making the tenant the root of enclosure gives private tenants every rule of this ADR for free, and makes "private wiki" and "private page" the same mechanism at different grains.
- **Categories or title prefixes as groupings.** Content that editors write; §3.
- **Postgres row-level security** with the principal in a session variable, as a second line behind the server. Attractive, and nothing here forbids adding it later (Q2). Rejected as the primary mechanism: the evaluator's three axes of enclosure, expiring ACLs, group inheritance from farm accounts and L0 caching would have to be duplicated in SQL and paid for on every row; and RLS does nothing for OpenSearch, Valkey, rendered pages or the blob store, which hold the same data.
- **Encrypting restricted content per group at rest.** Rendering, search, diffs and constraint checks need plaintext; the keys would live on the same servers; and a group's membership changes, which would mean re-encrypting. The boundary is the deployment (§10), stated and checked, not a cipher.
- **One search index per group, or no indexing of restricted content.** §8.
- **403 instead of absence.** Honest and simpler to debug, and it tells every probe which titles exist. Absence (§5), with the two leaks it cannot close named.
- **Including a more restricted target into a less restricted page, rendered per viewer.** Every page would need a rendering per combination of viewer groups, the render cache would be useless, and a page's visibility could no longer be computed from the page alone. The subset rule of §6 keeps one rendering per visibility, at the cost of a red link where an editor tried to include a secret into a public page, which is the behaviour wanted.

## Consequences

- **The rule is one sentence**, §1, and every route, cache, index, stream and export has the same thing to prove. The privacy test says how (§15).
- **A public wiki changes nothing.** Empty visibility sets, `-` in every cache key, an empty `read_groups`, and no `tenant` ACL: the cost of this ADR to the open scenario is a few bytes per key.
- **Hosting works with the existing presets.** `isolated` plus a `tenant` ACL per customer gives tenants that cannot see each other, cannot search each other, cannot transclude each other and cannot learn each other's titles, while all of them read Wikidata and Wikipedia. No operator superuser exists in the UI.
- **Pages and groupings can be confidential with one mechanism.** A `read` restriction set by `protect` on a page, a set, a namespace or a tenant, evaluated conjunctively over enclosure, as every other restriction is.
- **A private wiki is a usable wiki.** Its members search it (§8), cache it (§7), watch it and are notified from it (§6). The earlier model would have given them none of those.
- **Search and Valkey join the boundary.** They now hold restricted data; a deployment that exposes either exposes the wiki, and `instance check` says so.
- **Existence is protected, with two named leaks**: a taken title and a sequence gap (§5). Anyone relying on absence should know both.
- **Includes may not widen**, so a public page cannot be used to read a private one, and an editor who tries sees a red link. One rendering per visibility is what makes caching work.
- **Operators are inside the boundary.** This ADR constrains what the software serves, not what a database administrator can query; it says so rather than imply otherwise.
- **The first milestone is unchanged** (API-only, one user and their subsidiaries, no differentiated ACLs, set 2026-09-27). Lines 1, 5, 9 and 11 of §10 and the 421 rule are cheap and catch the likeliest mistakes, so they belong in the first server rather than later.
- **Broad `read` ACL changes purge broadly.** Making a namespace private lapses every cache entry of the tenant through the epoch (§7). Acceptable because it is rare; a tenant that flips visibility daily will notice.
- **Two more ACL targets, one more switch, one more CLI command, and a `{vis}` segment in cache keys.** No new partition, no new schema, nothing new in `private`.

## Open questions

- **Q1. A members-only query endpoint.** A private tenant has no SPARQL endpoint, because every endpoint is fed from the ∅ form (§3, §10 line 7). An organisation may want QLever over its private data for its own members; that would be an endpoint behind the evaluator, or a per-tenant instance loaded from the operator's full bundle inside the boundary. Not designed here.
- **Q2. Row-level security as a second line.** Whether to add Postgres RLS on `view` with the principal's groups in a session variable, so that a bug in the server's evaluator fails closed at the database. It would duplicate enclosure in SQL; the question is whether the duplication is worth it for the containment axis alone.
- **Q3. Share links.** A capability URL that lets a named outsider read one restricted page without an account, as document systems offer. It is a principal that is a secret, which this ADR has no place for; if wanted, it is a subsidiary with one grant and an expiry ([0024](0024-subsidiary-accounts.md)), and the question is whether that is enough.
- **Q4. Auditing restrictions one cannot read.** `Special:ProtectedPages` shows a restriction only to those who may read its target (§5), so a bureaucrat outside every group cannot count the tenant's confidential pages. Whether `protect` holders should see a count, or the names of sets without their members, trades audit against the existence leak.
- **Q5. Nested sets and namespace members.** Sets are flat and hold pages, entities and threads (§3). Whether a set may hold a namespace or another set, which would make the enclosure graph recursive, waits for a case that needs it.
- **Q6. `instance check` coverage.** Lines marked `attest` (§10) are the operator's word. Whether some can become checks, such as detecting other Postgres connections through `pg_stat_activity` on a schedule, or whether a signed deployment manifest is the right tool, is open.
- **Q7. Confidential restrictions on foreign and mirrored content.** A `read` ACL on a mirrored page's ranged ID hides it on the tenant ([0053](0053-mirrored-pages.md) §9), and the same works for a mirrored entity's overlay row; whether a tenant should be able to make a *subset* of Wikipedia visible to a group only, as a reading list, or whether that is a set of mirrored pages under this ADR already, needs a worked example.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §16 | extends | 0005 A55 |
| [0012](0012-api-requirements.md) §4, §8 | §5, §13, §15 | extends | 0012 A35 |
| [0013](0013-postgres-storage.md) §5.6 | §14 | extends | 0013 A26 |
| [0014](0014-caches-and-search.md) §1, §4, §7 | §7, §8 | amends | 0014 A13 |
| [0016](0016-permissions-and-access-control.md) §2, §4 | §4 | amends | 0016 A21 |
| [0016](0016-permissions-and-access-control.md) §4, §7 | §3, §13 | extends | 0016 A21 |
| [0018](0018-tenants.md) §5 | §3 | amends | 0018 A11 |
| [0021](0021-notifications.md) §2 | §6 | extends | 0021 A8 |
| [0023](0023-moderation.md) §1, §2 | §3, §4 | amends | 0023 A9 |
| [0023](0023-moderation.md) §2, §8, §9 | §3, §13 | extends | 0023 A9 |
| [0028](0028-tenancy-policy.md) §5 | §6 | amends | 0028 A9 |
| [0028](0028-tenancy-policy.md) §1 | §11 | extends | 0028 A9 |
| [0028](0028-tenancy-policy.md) Q2 | §11 | settles | 0028 Q2 |
| [0033](0033-backend-stack.md) §12 | §10 | extends | 0033 A8 |
| [0042](0042-template-expansion-and-parsoid.md) §3 | §6 | amends | 0042 A7 |
| [0043](0043-lua-modules.md) §5 | §6 | extends | 0043 A3 |
| [0051](0051-page-redirects.md) §2 | §6 | extends | 0051 A1 |

## References

- [Manual:Preventing access](https://www.mediawiki.org/wiki/Manual:Preventing_access) and [Security issues with authorization extensions](https://www.mediawiki.org/wiki/Security_issues_with_authorization_extensions), the list of bypasses this ADR closes by construction
- [Manual:$wgWhitelistRead](https://www.mediawiki.org/wiki/Manual:$wgWhitelistRead), [Extension:Lockdown](https://www.mediawiki.org/wiki/Extension:Lockdown)
- [OpenSearch: `terms_set` query](https://opensearch.org/docs/latest/query-dsl/term/terms-set/), the filter of §8; [OpenSearch Security: document-level security](https://opensearch.org/docs/latest/security/access-control/document-level-security/), the alternative of pushing the filter into the cluster
- [PostgreSQL: Row Security Policies](https://www.postgresql.org/docs/current/ddl-rowsecurity.html) (Q2)
- Anderson, *Security Engineering*, 3rd ed., ch. 9 (multilevel security), for the "no write up without read up" shape of §6 and §9
- `docs/registry/tenancy.toml` (§11), `docs/registry/groups.toml`

## Amendment log

### A1. Development mode's fallback tenant

- **Date:** 2026-10-03
- **Source:** Direct: James, decision of 2026-10-03 (`triplespace-accounts`, `triplespace-api-action`, `triplespace-server`, `scatter-adapter-internetdomains`)
- **Change:** extends §10
- **Summary:** In `development` mode the server may name a fallback tenant (`--dev-tenant`), which an unregistered `Host` (a developer's `localhost:8080`) is served as, with `siprop=general` reporting the request's own scheme and host as `server`; the session cookie then also drops `Secure`. In `production` an unregistered host is 421 and the setting is refused.

### A2. The web tier

- **Date:** 2026-10-03
- **Source:** [0057](0057-web-tier.md) §10, §12
- **Change:** amends §10, §16; extends §15
- **Summary:** Proxy trust is read from the right along a chain of proxies, and a hop is trusted by its address in `server.trusted_proxies`, the default, or by a forwarder key, for networks shared with workloads that are not trusted (line 5). A web tier that serves the site through the API is a hop inside the request path and outside the stores, and line 12 states what it must do. The privacy test also runs through the web tier. The landing page is rendered by `triplespace-ui`, wherever the site is served.

Replaced text (§10, line 5):

> | 5 | The server rejects a request whose `Host` is not a registered tenant base or the farm base, with 421, and trusts `X-Forwarded-For` and `X-Forwarded-Proto` only from `server.trusted_proxies` | The host selects the tenant ([0018](0018-tenants.md) §11); IP blocks, rate limits and filter IP rows depend on the client address ([0016](0016-permissions-and-access-control.md) §3, [0024](0024-subsidiary-accounts.md) §5, [0030](0030-edit-filters.md) §11) | `pass`/`fail` by sending a request with an unregistered host; `fail` if the server is behind a proxy and the trust list is empty |

Replaced text (§16):

> | `triplespace-server` | `server.mode`, `server.trusted_proxies`, `server.admin_listen`, 421 on an unregistered host, the landing page, the internal endpoint for Parsoid |

### A3. The query service

- **Date:** 2026-10-04
- **Source:** [0059](0059-query-service.md) §4
- **Change:** extends §3, §10
- **Summary:** A private tenant has no query service either (§3). The deployment boundary (§10) gains the embedded query store in line 7, fed from `view.rdf_delta`, and a line 13 for the shared remote store: reachable only from the servers, credentialed, and queried only through the service, which fixes the tenant's dataset.

Replaced text (§10, line 7, in part):

> | 7 | A SPARQL endpoint, QLever or any other external index is loaded from the public dump or the update stream, never from `view` or `log` |

### A4. Scopes are not sets

- **Date:** 2026-10-04
- **Source:** [0060](0060-scopes.md) §1
- **Change:** extends §3
- **Summary:** A scope is content and never a restriction target; the paragraph after the set rationale says so.
