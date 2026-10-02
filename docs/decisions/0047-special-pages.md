# 0047. Special pages

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-01 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0029](0029-resolver-namespaces.md), [0031](0031-property-constraints.md), [0042](0042-template-expansion-and-parsoid.md), [0046](0046-primary-tenant.md)
- **Uses:** [0019](0019-discussions.md), [0021](0021-notifications.md), [0028](0028-tenancy-policy.md), [0034](0034-frontend-stack.md), [0038](0038-page-metadata-and-categories.md), [0039](0039-files-and-media.md), [0040](0040-instance-prerogatives.md)

## Context

About sixty special pages are named across [0001](0001-revision-metadata-rdf.md) to [0046](0046-primary-tenant.md). Each ADR named one where MediaWiki users would look for it: 0010 §12's address table, 0016 §7, 0023 §9, 0028 §11, 0039 §18 and so on. [0005](0005-crate-organization.md) §2 says `triplespace-server` "serves the special pages the ADRs name". That makes the set whatever the ADRs happened to mention, and leaves four problems.

1. **Pages the ADRs rely on but never name.**
   - [0010](0010-site-ui.md) §2 puts a **Special pages** link in the global header, but specifies no index for it.
   - 0010 §3 describes "the full results page" of search without giving it an address.
   - 0010 §2's **New** menu and [0029](0029-resolver-namespaces.md) §3's "Create an item with this DOI" both open an item form that is not specified.
   - Move, revision deletion and unblocking exist as actions ([0008](0008-namespaces-and-document-pages.md) §4, [0023](0023-moderation.md) §8) but have no pages.
   - Identity conflicts are "listed for review" ([0004](0004-identity-clusters-and-equivalence.md) §10), as are local corrections ([0002](0002-source-graphs-and-mass-ingest.md) §7), but neither list has an address.
2. **Standard pages nobody has decided.** MediaWiki 1.43 core registers 138 special pages, 8 of them conditionally. Wikibase REL1_43 registers 20 in the repository and 3 in the client. Most of them are maintenance reports, which tools, gadgets and help pages link to by name. No ADR says which of them Triplespace serves.
3. **Renamed pages have lost their MediaWiki names.** Some examples:
   - `Special:EditFilter` replaces AbuseFilter's `Special:AbuseFilter` ([0030](0030-edit-filters.md)).
   - `Special:OAuthConsumers` and a section of `Special:Account` replace the OAuth extension's consumer pages ([0025](0025-oauth-server.md) §5).
   - Bot passwords became subsidiaries' keys ([0024](0024-subsidiary-accounts.md) §4).

   Only `Special:Preferences` was given a redirect (0010 §11). [0016](0016-permissions-and-access-control.md) Consequences promises that "MediaWiki clients and users see MediaWiki", and that promise does not hold at these addresses.
4. **Reports at mirror scale.** MediaWiki computes its expensive reports offline in miser mode (`$wgMiserMode`, `updateSpecialPages.php` into `querycache`). A report over everything an instance holds would include hundreds of millions of mirrored entities. Such a report costs a great deal and tells a wiki's editors little about their own wiki.

Wikibase REL1_43 no longer registers `Special:EntitiesWithoutLabel` or `Special:EntitiesWithoutDescription`, which earlier releases had. Tools still link to them.

James's direction, from the design discussion of 2026-09-30:

- Add the Wikibase compatibility forms.
- Reports are **local graphs only by default**, as the general rule for report pages.
- Reports are **live projections by default**, and otherwise configured as batch.
- Build a **native Special:Nuke** and a **Special:Export**.
- **LinkSearch** stays outstanding until the external-links table is designed.
- **No EmailUser**.
- **Mute** and the **redirect reports** are deferred.
- **Aliases are configured in the registry**.

## Decision

### 1. One registry of special pages (extends 0015 §5)

**`docs/registry/special-pages.toml` lists every special page name the instance knows**, including pages it serves, pages it defers or declines, and names it reserves. It is embedded by `triplespace-titles`, which already resolves titles in the `Special` namespace ([0008](0008-namespaces-and-document-pages.md) §3). Each entry has these fields:

| Field | Meaning |
|---|---|
| `name` | The canonical name. Where MediaWiki or an extension has the page, this is the first English alias MediaWiki gives it, so `RecentChanges` and not `Recentchanges`, and `MostLinkedFiles` and not `Mostimages`. |
| `mediawiki_name` | MediaWiki's internal name, where it differs from `name`. `meta=siteinfo&siprop=specialpagealiases` reports it as `realname`, and `list=querypage` accepts it as `qppage`. |
| `origin` | `mediawiki`, an extension's name (`Wikibase`, `WikibaseClient`, `Nuke`, `AbuseFilter`, `OAuth`, `Echo`, `CentralAuth`, `GlobalBlocking`, `WikibaseQualityConstraints`, `WikibaseLexeme`, `EntitySchema`), or `triplespace` |
| `status` | `served`, `deferred` (with `until`), `declined` (with `reason`) or `reserved` |
| `scope` | `tenant`, `farm` or `both` (§3) |
| `group` | The heading under which `Special:SpecialPages` lists it. These are MediaWiki's group keys plus `wikibaserepo`, `wikibaseclient`, `identity` and `instance`. |
| `aliases` | Other names that reach the page (§2) |
| `section_aliases` | MediaWiki pages whose function is part of this page, each mapped to a subpage or an anchor (§2) |
| `restricted` | The permission without which the page is not listed and refuses to act, as MediaWiki's restricted special pages do |
| `report` | For reports only: `backing`, the tables it reads, and its default graph scope (§4) |
| `adr` | Where the page is specified |

**Rules**, as for the other registry files:

- Names are never removed. A page that stops being served changes status.
- A name is never given a second meaning.
- **Every `Special:` name an ADR uses must be in the file**, as a `name`, an alias or a section alias. `check_adrs.py` gains a fifth check that enforces this.
- Upstream page names that this ADR does not decide can be added later as `deferred`.

**Statuses:**

- **`served`:** the instance serves the page. "Served" records a decision, not an implementation. Implementation is tracked in the milestones.
- **`deferred`:** undecided, with the open question it waits on.
- **`declined`:** decided against, with the reason. Requests get MediaWiki's "no such special page" response.
- **`reserved`:** the name belongs to an entity type Triplespace has reserved but not implemented. These are Lexeme and EntitySchema ([0009](0009-keyed-entity-types-and-domain.md), [0041](0041-content-models.md)). The name will not be used for anything else.

### 2. Names, aliases and section aliases

**An alias resolves to its page.**

- Aliases are matched case-insensitively, as MediaWiki matches them.
- A `GET` for an alias is redirected (301) to the canonical name, with the subpage and query string kept. A `POST` is served in place. This is how MediaWiki's `SpecialPageFactory::executePath` handles a name that is not the page's local name.
- Aliases cover MediaWiki's other English names (`Special:Random` and `Special:RandomPage`). They also cover the MediaWiki names of pages Triplespace renamed:

| Triplespace page | Aliases |
|---|---|
| `EditFilter` ([0030](0030-edit-filters.md)) | `AbuseFilter` |
| `EditFilterLog` | `AbuseLog` |
| `OAuthConsumers` ([0025](0025-oauth-server.md) §5) | `OAuthListConsumers`, `OAuthConsumerRegistration`, `OAuthManageConsumers` |
| `GlobalGroupMembership` ([0028](0028-tenancy-policy.md)) | `GlobalUserRights` |
| `GlobalBlockList` | `ListGlobalBlocks` |

**A section alias redirects to the part of a page that does the job.** The target is a subpage or an anchor. These are mostly sections of `Special:Account` ([0010](0010-site-ui.md) §11 and its extensions):

| MediaWiki page | Goes to |
|---|---|
| `Preferences`, `ResetTokens`, `DisplayNotificationsConfiguration` | `Account#preferences`, `Account#preferences`, `Account#notifications` |
| `ChangeCredentials`, `RemoveCredentials`, `ChangePassword`, `LinkAccounts`, `UnlinkAccounts` | `Account#sign-in-methods`. MediaWiki's "linked accounts" are external sign-in providers, which are bindings here ([0007](0007-actor-identity.md) §3). Triplespace's own **Linked accounts** section is a different thing (0007 §7). |
| `ChangeEmail`, `Confirmemail`, `Invalidateemail` | `Account#notifications`, where the email address and its verification live ([0021](0021-notifications.md) §4, §6) |
| `BotPasswords` | `Account#subsidiaries` ([0024](0024-subsidiary-accounts.md) §4) |
| `OAuthManageMyGrants` | `Account#connected-applications` ([0025](0025-oauth-server.md) §5) |
| `NotificationsMarkRead` | `Notifications` |
| `Listadmins`, `Listbots` | `ListUsers/sysop`, `ListUsers/bot` |

**`siprop=specialpagealiases`** reports one entry per served page. Its `realname` is `mediawiki_name`, or `name` where there is no `mediawiki_name`. Its aliases are `name` followed by `aliases`. Each section alias is reported as an entry of its own, so a client asking for `Preferences` finds it.

**Localized aliases** are messages in `i18n/` ([0034](0034-frontend-stack.md) §9), in the shape of MediaWiki's `*.alias.php` files. The first alias in the tenant's content language is the name the UI links to. Every alias in every language resolves. The registry holds the English names, which are the ones a tool can rely on.

### 3. Where a page is served (extends 0018 §11 and 0046 §7)

**A page is served where its subject lives.**

- **`farm`:** pages about instance-scope things. These are tenants and the tenancy policy, farm accounts, global groups and global blocks, instance acts and takedowns:
  - `Tenants`, `GlobalUsers`, `GlobalGroupMembership`, `GlobalBlock`, `GlobalBlockList`, `RemoveGlobalBlock`, `GlobalRecentChanges`;
  - `InstanceAction`, `Takedowns`, `Takedown`;
  - `OAuthConsumers`, whose approval is an instance right ([0046](0046-primary-tenant.md) §8).
- **`both`:** pages with a tenant form and an instance form.
  - `Log`: a tenant's log, or the public records of the instance `log` (0046 §4).
  - `Jobs`: tenant bulk jobs, or instance jobs such as mirror syncs (0046 §4).
  - `Contributions`: at the farm base, the operator's acts for `ts-viewoperator` ([0040](0040-instance-prerogatives.md) §7).
  - `Providers`: every provider at the farm base; at a tenant, the providers it has opted into, with its own lag.
  - `Tenancy`: the policy at the farm base, and a tenant's own view of it with 0046 §9's **Accept**.
  - `Version` and `SpecialPages`.
- **`tenant`:** everything else, including `GlobalContributions`, which [0018](0018-tenants.md) §8 serves on a tenant for the accounts linked to it.

A `farm` page requested at a tenant base is redirected to the farm base.

**Where the farm base is also a tenant's base**, the two sets of pages share a host. That is always so on a single-tenant instance, and it may be so on a farm ([0046](0046-primary-tenant.md) §7). Names are unique in the registry, so nothing collides.

A `both` page at such a host shows both forms together, with a `scope=tenant|instance` filter that defaults to both. 0046 §7 says special pages "keep their paths". This is how two pages keep one path.

### 4. Reports

#### 4.1 What a report is

**A report lists the things that meet a condition, sorted and paged.** Reports include:

- MediaWiki's QueryPage pages: the `qppage` list of `list=querypage` and its unlisted siblings;
- Wikibase's lists (`ItemsWithoutSitelinks`, `ListProperties`, `UnconnectedPages`, `PagesWithBadges`, `EntityUsage`);
- Triplespace's own lists (§6).

Feeds ([0020](0020-change-feeds.md)), logs, forms and pages about a single thing are not reports. Every report has a `report` table in the registry.

#### 4.2 Local graphs by default

**A report reads the tenant's own partitions** (`local`, `pages`, `log`, `actors`, `config` and its file versions) and nothing else by default. It does not read `mirror/*`, `log/{provider}`, `files/{repo}` or any other tenant's partitions. This is the general rule for report pages.

- **Page and file reports are unaffected.** Pages and uploads are always the tenant's own.
- **Entity reports list local subjects.** The rows are local entities, and local assertions where a report is about statements. Conditions are evaluated on the **resolved view**, as a reader sees it. For example:
  - `EntitiesWithoutLabel` lists local entities that have no label in the language. A local item fused with a labelled Wikidata item ([0004](0004-identity-clusters-and-equivalence.md) §4) has a label, so it is not listed.
  - `ListProperties` lists local properties.
  - `ItemsWithoutSitelinks` lists local items.

  The report answers "what is missing among the things this wiki made, as its readers see them".
- **Widening takes a `graphs=` parameter** that names source graphs.
  - It is served live only where the report's backing already covers those graphs. `ConstraintReport` is the main case: `view.constraint_violation` covers the resolved view ([0031](0031-property-constraints.md) §2, §5), and `IdentityConflicts` (§6) is another.
  - Elsewhere, widening must be configured by the instance's operators in its `reports` configuration (§4.3), and a report configured for mirror graphs is always **batch** for that graph set. No live projection is maintained at mirror scale.
- **`ConstraintReport` defaults to the local graph** (amends 0031 §3). Its existing source-graph filter widens it.

#### 4.3 Live projections by default; batch by configuration

Every report has one of three backings:

| Backing | What serves it | Freshness |
|---|---|---|
| `index` | A keyset-paged query over a `view` table and one of its indexes. The report has no rows of its own. | Live, at the lag of the table it reads |
| `projection` | Rows in `view.report_entry` (§13), maintained incrementally by the `report` projection as the tables it reads change | Live, at that projection's lag |
| `batch` | Rows in `view.report_entry`, recomputed by a scheduled `ops` job | As of the last run |

**No report is batch by default.** The registry gives each report `index` or `projection`, and §4.4 lists them.

**Whether a report is batch is an instance setting, not a tenant's.** The mode exists to manage the instance's load. A small instance runs every report live and needs no setup. A large instance plans batch runs for its expensive reports. That is the operators' call, made where they manage the rest of the instance's capacity.

**Settings live in an instance-scope `config` kind, `reports`** (extends [0015](0015-record-format-and-partition-registry.md) §3). The code is `default`, or a tenant's slug for an override that applies to that tenant only. For example, a farm can run one large tenant's reports in batch while every other tenant stays live. An override replaces the default key by key. The entry holds:

| Key | Meaning | Default |
|---|---|---|
| `batch` | Report names to run as batch. Any `projection` report can be listed. | None |
| `widen` | Report name → the mirror graphs it also covers (§4.2). Each widening is a batch run for that graph set. | None |
| `schedule` | When batch runs start | Daily |
| `batch_limit` | Rows kept per batch report, as MediaWiki's `$wgQueryCacheLimit` | 1,000 |

The settings are written with `ts-config` at the farm base, which is an instance right ([0040](0040-instance-prerogatives.md) §9). A tenant's administrators cannot change them. They see each report's mode on the report page and in `GET /reports` (§11). On a single-tenant instance the operators and the wiki's owners are usually the same people, so nothing changes for them.

Changing a mode writes nothing into any tenant's partitions, so it is not an instance act ([0040](0040-instance-prerogatives.md) §1). When a report moves from live to batch, its next run fills its rows. When it moves from batch to live, the `report` projection rebuilds its rows for that tenant.

A batch report's page says when it was computed, as MediaWiki's cached reports do. `list=querypage` returns `cached`, `cachedtimestamp` and `maxresults` for it.

**How the projection keeps up.**

- The `report` projection runs after the tables it reads, in step 5 of [0013](0013-postgres-storage.md) §7. Each change to an input key recomputes only the entries that key affects. For example, a new `page_link` row to a missing title adds one to that title's `WantedPages` count.
- Report entries are **fan-out**, not part of a write's own rows. They are applied by the projection worker under 0013 §7's synchronous budget, and their lag is reported with the other projections'.
- Some inputs are written by the refresh job rather than by replay ([0042](0042-template-expansion-and-parsoid.md) §10): `transclusion`, `entity_usage`, and links and categories under expansion. For those, the refresh job applies the report deltas in the same transaction as the rows it writes.

**Rebuilds** (amends 0013 §5.6's rule).

- `projection` entries are a function of the `view` tables they read, and are rebuilt with them.
- `batch` entries are a dated snapshot. A rebuild empties them, and the next run fills them.

As with 0042 §10's tables, this is stated rather than left to be assumed.

#### 4.4 The reports

**Index-backed:**

| Report | Reads |
|---|---|
| `AllPages`, `PrefixIndex` | `view.page (ns, title)` |
| `Categories`, `TrackingCategories` | `view.category`; 0042 §9's tracking-category names |
| `ShortPages`, `LongPages` | `view.page.len` (§13) |
| `AncientPages` | `view.page.latest_at` (§13) |
| `FewestRevisions`, `MostRevisions` | `view.page.revisions` (§13) |
| `NewPages` | Creations in `view.activity`, with the `patrolled` filter ([0023](0023-moderation.md) §6) |
| `ProtectedPages`, `ProtectedTitles` | `view.acl`; reserved rows in `view.page` (0023 §10) |
| `ListFiles`, `NewFiles`, `MIMESearch`, `FileDuplicateSearch` | `view.file`, `view.file_version` ([0039](0039-files-and-media.md) §20) |
| `ListUsers`, `ActiveUsers` | `view.actor`; `view.actor.last_active` (§13); active means within `$wgActiveUserDays`, default 30 |
| `ListProperties` | `view.entity` by type and datatype |
| `EntityUsage` | `view.entity_usage` ([0043](0043-lua-modules.md) §10) |
| `PagesWithBadges` | `view.sitelink` for the tenant's own host ([0038](0038-page-metadata-and-categories.md) §6) |
| `ConstraintReport` | `view.constraint_violation` ([0031](0031-property-constraints.md) §5) |
| `Corrections`, `IdentityConflicts` | `view.correction`; `view.link` (§6) |
| `Random`, `RandomInCategory`, `RandomRootPage` | `view.page.random` (§13), as MediaWiki's `page_random` |

**Projection-backed:**

| Report | Entry key and sort |
|---|---|
| `WantedPages`, `WantedCategories`, `WantedTemplates`, `WantedFiles` | A missing target; links, members, transclusions or uses to it |
| `MostLinkedPages`, `MostLinkedCategories`, `MostTranscludedPages`, `MostLinkedFiles`, `MostCategories` | A page or file; the count |
| `DeadendPages`, `LonelyPages` | A page with no outgoing links; a page with no incoming links and no transclusions |
| `UncategorizedPages`, `UncategorizedCategories`, `UncategorizedTemplates`, `UncategorizedFiles` | A page in no category |
| `UnusedCategories`, `UnusedTemplates`, `UnusedFiles` | A category with no members; a template nothing transcludes; a file nothing uses |
| `ListDuplicatedFiles`, `MediaStatistics` | A hash with more than one current file; a MIME type and its count and bytes |
| `ItemsWithoutSitelinks` | A local item with no sitelink |
| `EntitiesWithoutLabel`, `EntitiesWithoutDescription` | A local entity and a language, for each language in `reports.term_languages` (default: the content language). Other languages are added to the setting, not computed on request. |
| `UnconnectedPages` | A main-namespace page with no item paired through 0038 §6's own-host sitelink |

Reports about templates are listed only while `wikitext.expansion` is on (0042 §3). Before that the Template namespace is reserved.

#### 4.5 Who sees what

**Report entries are stored in their public form** ([0014](0014-caches-and-search.md) §1, principle 3). Nothing behind a `read` ACL becomes an entry: deleted pages and entities, hidden names, and pages in a namespace with a read ACL.

A report never lists what an anonymous reader could not see. Every viewer sees the same report, as on MediaWiki. On a tenant whose graphs are private, the report page itself is behind the graph ACL ([0016](0016-permissions-and-access-control.md) §4).

### 5. Wikibase compatibility forms

The forms are `NewItem`, `NewProperty`, `SetLabel`, `SetDescription`, `SetAliases`, `SetLabelDescriptionAliases`, `SetSiteLink`, `MergeItems` and `RedirectEntity`. Each is served at its Wikibase address, with Wikibase's subpage and query forms (`Special:SetLabel/Q42/en`, `Special:SetSiteLink/Q42/enwiki`, `?id=…&language=…`), because tools and gadgets link to them.

**They are server-rendered forms that need no JavaScript.** [0034](0034-frontend-stack.md) §10 gives older browsers the server-rendered pages without components. That leaves those browsers without the statement UI's editing, and these forms are their editing path.

**They write through the same handlers as `wbsetlabel`, `wbsetsitelink`, `wbmergeitems` and the rest** (0034 §1.5). Permissions, edit filters, rate limits and base offsets are therefore the same as for the API. The forms show the current value and nothing else, so they are not a second renderer of entities (0034 §1.3).

- **On a foreign entity,** `SetLabel`, `SetDescription` and `SetAliases` write a local term correction ([0002](0002-source-graphs-and-mass-ingest.md) §7; kind `term` in `view.correction`). `SetSiteLink` writes a local sitelink ([0026](0026-sitelinks.md)). Both are what the statement UI writes for the same edit.
- **`NewItem` and `NewProperty`** are the targets of the **New** menu (0010 §2).
  - They take Wikibase's `label`, `description`, `aliases` and `lang`.
  - They also take a Triplespace parameter, `statement={property}:{value}`, repeatable, which prefills statements. [0029](0029-resolver-namespaces.md) §3's "Create an item with this DOI" opens `NewItem` with the resolver's property and key filled in.
  - `NewProperty` needs `property-create` and a `datatype`.
- **`MergeItems`** (`fromid`, `toid`, `ignoreconflicts`):
  - **Two local entities:** Wikibase's merge. Content moves to the target, the source is emptied and becomes a redirect, and both get ordinary revisions ([0011](0011-logs.md) Context).
  - **Two foreign entities in one namespace:** a local `redirect` and nothing more ([0004](0004-identity-clusters-and-equivalence.md) §2). Upstream's content is not moved.
  - **Across namespaces:** refused with `ts-cross-namespace-merge`, which links to `Special:LinkEntities` with both IDs filled in (§6).
  - Needs `item-merge`.
- **`RedirectEntity`** writes a `redirect` (0004 §9), including the cross-namespace property redirect amended there on 2026-09-27. Needs `item-redirect`.

### 6. Identity and correction pages

Three pages give addresses to things 0002 and 0004 already describe:

- **`Special:LinkEntities`** *(new)* is the form for `same-as`, `different-from` and `equivalent-property` ([0004](0004-identity-clusters-and-equivalence.md) §9). It needs `ts-link` and counts in the `link` rate class ([0024](0024-subsidiary-accounts.md) §5). It is prefilled from a refused merge and from a conflict row.
- **`Special:IdentityConflicts`** *(new)* is 0004 §10's review list. It shows held links (`view.link` with `status = 'held'`) and the duplicate-key conflicts of [0009](0009-keyed-entity-types-and-domain.md) §9.
  - Each row offers 0004 §10's resolutions as actions: a local `same-as`, `different-from`, a merging `redirect`, or deprecating the source statement.
  - By §4.2's rule it lists conflicts that involve the tenant's own records or entities it has local assertions on. `graphs=all` lists every held link in the tenant's view. That is live, because the table already holds them.
- **`Special:Corrections`** *(new)* is 0002 §7's maintenance list of local corrections.
  - It can be filtered by state (`active`, `redundant`, `dangling`; [0003](0003-statement-ui.md) §4), kind (`rank`, `suppress`, `term`) and upstream graph.
  - It offers **Retire** for redundant corrections, singly or in bulk, and review for dangling ones.
  - 0002 §7 notes that this list "doubles as a list of fixes to report upstream", so it can be exported per upstream graph.

All three are in the `identity` group of `Special:SpecialPages`.

### 7. Special:Nuke

**MediaWiki's Nuke** lists pages that one user created, taken from recent changes, and deletes the selected pages one deletion at a time. It takes `target`, `pattern`, `namespace` and `limit`, and needs the `nuke` right, held by `sysop`. Triplespace serves `Special:Nuke` at that address with that right and those parameters. It extends Nuke in three ways that the log makes cheap.

**What it covers.** Everything the target did on the tenant, by kind:

| The target's contribution | What Nuke does |
|---|---|
| A page, thread or local entity it created | Deletes it: a `read` ACL naming the deletion group ([0023](0023-moderation.md) §4). With `deletetalk`, the talk page too. |
| A file it uploaded first | Deletes the file page (0039 §9) |
| A new version of someone else's file | Reverts the file to the previous version ([0039](0039-files-and-media.md) §2, `upload/revert`) |
| Change sets on others' entities, or on page statements ([0038](0038-page-metadata-and-categories.md) §1) | Appends the inverse change sets. Later edits by others are kept, as a job revert keeps them ([0010](0010-site-ui.md) §9). |
| Text edits to others' pages | Rolls back, restoring the latest revision by another actor, where the target's edits are the latest. Otherwise the page is listed and left alone, as rollback does. |
| Posts in others' threads | Hides the post's `text` part with a `record` ACL (0023 §5), since only a whole thread is deleted ([0019](0019-discussions.md) §1) and a post's text is its record's `text` part (0019 §4) |
| Edit summaries, optionally | Hides the `comment` part of each of the target's records. Needs `deleterevision`. |

Foreign entities are never deleted (0023 §1). The target's local assertions about them are reverted, and upstream's content is untouched.

**Who it targets.**

- An account, by default together with its subsidiaries ([0024](0024-subsidiary-accounts.md) §1), so that a spammer's bots go with it. Temporary accounts are accounts.
- Alternatively a **change tag**. An `oauth:{slug}` tag ([0025](0025-oauth-server.md) §4) cleans up after a tool that misbehaved.
- IP addresses are private state ([0016](0016-permissions-and-access-control.md) §3) and are not a target.

**When.** The window defaults to the recent-changes window, `rc.max_age` (0010 §7). MediaWiki's Nuke can see no further back than that, because it reads `recentchanges`. Here the log is complete, so the window can be extended to the target's whole history. `pattern` (a title pattern) and `namespace` narrow it further.

**How it runs.**

1. **Preview.** The page lists every candidate, grouped by the kinds above, with counts and a checkbox each, all checked as in MediaWiki. Candidates that will be left alone are marked with the reason, such as a page edited since.
2. **Confirm.** Above `nuke.confirm_threshold` items (`site` setting, default 500), the form asks for the target's name to be typed, as the snapshot safety threshold does ([0002](0002-source-graphs-and-mass-ingest.md) §8.4).
3. **One job.** Submitting starts a job with mode `nuke` ([0011](0011-logs.md) §6.3).
   - Its actor is the administrator. Its parameters are the target, the filters, the window, the selection and the reason.
   - Every record it writes carries the job ID in its attestation: ACLs in the tenant `log`, inverse change sets, page records and record ACLs.
   - Each write goes through the write path as the administrator's own would.

**What others see.**

- Recent changes shows **one row** for the job (0010 §7). A patroller marks it once (0023 §6).
- The deletion log still gets one `delete/delete` event per deleted page, each tagged `job:{id}`. A tool reading `list=logevents` sees what it would see after MediaWiki's Nuke.

**Undo.** **Revert this job…** on the job's page (0010 §9) does three things:

- retires the ACLs the job wrote;
- appends the inverse of its inverse change sets;
- restores the revisions it rolled back where nobody has edited since, and lists the rest.

It needs `ts-revertjob` and, because it retires deletions, `undelete`.

**Blocking is separate.** A block is an actor record with its own form ([0016](0016-permissions-and-access-control.md) §3). The Nuke page links to `Special:Block` for the target and offers **Block after nuking**, which opens that form prefilled.

### 8. Special:Export and Special:Import

**`Special:Export` keeps MediaWiki's form and parameters:**

- a list of titles;
- `addcat` with a category, and, where the `site` setting `export.from_namespaces` is on (MediaWiki's `$wgExportFromNamespaces`), `addns` with a namespace;
- `curonly` or full history;
- `templates`;
- `wpDownload`.

`action=query&export` and `exportnowrap` give the same output.

**The default format is MediaWiki XML**, export schema version 0.11, which carries content models and slots.

- **Pages.** Each page record is a `<revision>`, with its content model and format ([0041](0041-content-models.md)).
  - A part hidden from the viewer is written as MediaWiki writes revision-deleted content (`<text deleted="deleted"/>`, and likewise `comment` and `contributor`). An erased part is written the same way.
  - Full history is governed by `export.history` (default on) and `export.max_history` (revisions per page, default 1,000), MediaWiki's `$wgExportAllowHistory` and `$wgExportMaxHistory`.
- **Templates and modules.** With `templates`, the pages that `view.transclusion` lists for the selection ([0042](0042-template-expansion-and-parsoid.md) §10) are added.
- **Local entities** are exported as Wikibase exports entity pages. The model is `wikibase-item` or `wikibase-property`. Each revision is one local change set, and its text is the canonical JSON of the entity's **local-graph** state at that revision ([wikibase-compat.md](../api/wikibase-compat.md)). A MediaWiki Wikibase with free IDs can import them, which is the reverse of adoption ([0035](0035-adopting-a-wikibase.md)).
- **Foreign entities are not exported.** Their content is upstream's. `Special:EntityData` serves their resolved view.
- **Files.** With `files`, file versions are added as `<upload>` elements with their contents, which 0039 §14's import accepts. The total is capped by `export.max_bytes`. Over the cap, URLs are written in place of contents.
- **Not in the XML:**
  - page statements ([0038](0038-page-metadata-and-categories.md) §1), which no Wikibase slot carries (Q4);
  - threads and composite talk pages. They are left out by default, and with `threads` they are written with their own content models, which a MediaWiki importer refuses.

**A second format, `records`, is verifiable.**

- It contains the selected pages' and local entities' records from `pages` and `local`, each with an inclusion proof against the latest checkpoint. It also carries that checkpoint and the key-chain records needed to check it ([0006](0006-log-integrity-and-erasure.md) §9).
- A part withheld from the viewer appears as its commitment only, as an erased part does, so every proof still verifies.
- `log`-partition records are left out, because the `log` partition is internal (0023 §7).
- `triplespace-cli verify --records` checks such an export. A third party can then confirm that an export is exactly what the wiki holds, which no MediaWiki export can show.

**Size.**

- An export up to `export.sync_revisions` (default 5,000 revisions) streams.
- A larger one becomes an `ops` export job, as [0027](0027-preferences-and-portability.md) §3's data bundle does. The page then links to it when it is ready.
- Exports count in a new rate class, `export` ([0024](0024-subsidiary-accounts.md) §5), default 30 / 300 per hour for `user` / `bot`.
- Export needs only `read`, and every response is redacted per viewer ([0012](0012-api-requirements.md) §8).

**`Special:Import` is the form for [0008](0008-namespaces-and-document-pages.md) §9's import job.**

- **Upload an XML file** (`importupload`), or **import from another wiki** named by site alias ([0026](0026-sitelinks.md) §2) in place of MediaWiki's interwiki sources (`import`).
- It runs the template census first, shows its report, and on confirmation starts the job and goes to `Special:Jobs/{id}`.
- It accepts MediaWiki XML only. Data moves between instances by a tenant move ([0018](0018-tenants.md) §10) or verified sync ([0022](0022-federation.md)), not by importing `records`.

### 9. Other pages (extends 0010 §2, §3 and §12)

These are served with MediaWiki's or Wikibase's meaning and parameters. The notes say what they read or write.

| Page | Notes |
|---|---|
| `SpecialPages` | The index the header links to (0010 §2), grouped by `group`. Restricted pages are listed only to holders of the right. At the farm base it also lists the `farm` pages. |
| `Search` | The full results page of 0010 §3: `search`, `fulltext`, `ns{n}`, `profile`, `offset`, `limit`. `go` follows 0010 §3's **Go to** rule. |
| `MovePage` | The overflow menu's **Move** (0010 §2); a `move` record ([0008](0008-namespaces-and-document-pages.md) §4) |
| `RevisionDelete` | The history dialog of 0023 §9 as a page: `type`, `target`, `ids` |
| `Unblock`, `RemoveGlobalBlock` | Retire a block, or a global block at the farm base ([0028](0028-tenancy-policy.md)) |
| `EditWatchlist` | The watch set, with `/raw` and `/clear` ([0020](0020-change-feeds.md) §3) |
| `ChangeContentModel` | [0041](0041-content-models.md)'s `action=changecontentmodel` |
| `ExpandTemplates` | [0042](0042-template-expansion-and-parsoid.md)'s expander, in the `parse` rate class; listed while `wikitext.expansion` is on |
| `ComparePages` | Any two pages or revisions, with 0010 §6's diff |
| `Redirect` | `user/{id}`, `revision/{id}`, `page/{id}`, `file/{name}` and `logid/{id}`, resolved locally. `file` was already served by 0039 §7. This settles [0007](0007-actor-identity.md) Q7. |
| `RenameUser` | Renaming another account ([0007](0007-actor-identity.md) §4; `renameuser`) |
| `UserLogout`, `PasswordReset` | Session end; a reset for the built-in `password` issuer (0007 §3), sent by the email channel of 0021 |
| `UploadStash` | One's stashed uploads (0039) |
| `ItemDisambiguation` | Wikibase's label lookup (`/{language}/{label}`). 0029 §3's resolver disambiguation uses its layout. |
| `Statistics` | `view.site_stats` (§13), which also serves `siprop=statistics`, the `NUMBEROF*` variables ([0042](0042-template-expansion-and-parsoid.md) §5) and `mw.site.stats` ([0043](0043-lua-modules.md)) |
| `Version`, `ApiSandbox`, `ApiHelp`, `AllMessages`, `NamespaceInfo`, `PasswordPolicies`, `ListDatatypes`, `AvailableBadges`, `MyLanguageFallbackChain`, `Blankpage` | From the registry, configuration and i18n |
| `MyPage`, `MyTalk`, `MyContributions`, `MyLanguage`, `MyLog`, `MyUploads`, `AllMyUploads`, `EditPage`, `PageHistory`, `PageInfo`, `Purge`, `DeletePage`, `ProtectPage`, `NewSection`, `TalkPage`, `EditTags` | MediaWiki's redirecting pages, to the matching page or action. `NewSection` opens the talk page's new-thread form ([0019](0019-discussions.md)). |

### 10. Deferred, declined and reserved

| Page | Status | Until, or why |
|---|---|---|
| `LinkSearch` | deferred | An external-links table. [0042](0042-template-expansion-and-parsoid.md) §9 extracts external links, but no table holds them. |
| `PagesWithProp` | deferred | A page-properties table. 0042 §4 returns page properties, but none are stored. |
| `ListRedirects`, `BrokenRedirects`, `DoubleRedirects`, `RandomRedirect` | deferred | Page redirects ([0008](0008-namespaces-and-document-pages.md) Q8) |
| `Mute` | deferred | Undecided |
| `BookSources` | deferred | A bound ISBN resolver (`resolvers.toml` has a draft). It would then be an alias of that resolver. |
| `AutoblockList` | deferred | Autoblocks, which [0016](0016-permissions-and-access-control.md) does not specify |
| `PageLanguage` | deferred | Per-page content language, which nothing specifies |
| `EditRecovery` | deferred | The source editor's own recovery ([0034](0034-frontend-stack.md) §7) |
| `PageData` | deferred | Whether to alias it to the document node `{base}/page/{id}` (0008 §10) |
| `EmailUser` | declined | No user-to-user email |
| `Unwatchedpages` | declined | The watch set is private state ([0020](0020-change-feeds.md) §3). An aggregate over it would make the report projection a reader of `private`. |
| `WithoutInterwiki`, `MostInterwikis`, `GoToInterwiki` | declined | Interlanguage links are sitelinks ([0026](0026-sitelinks.md)). Interwiki prefixes come from site aliases, and their links render as their targets. |
| `MergeHistory` | declined | It would move revisions between pages, which the log cannot do |
| `Lockdb`, `Unlockdb` | declined | Read-only is a tenant freeze ([0018](0018-tenants.md) §10) or an instance act ([0040](0040-instance-prerogatives.md)) |
| `RunJobs`, `JavaScriptTest`, `AuthenticationPopupSuccess`, `Contribute` | declined | Internal to MediaWiki, a development tool, MediaWiki's popup login flow, and a skin feature |
| `DispatchStats` | declined | Wikibase's client-change dispatching has no counterpart. `Special:Providers` reports sync lag. |
| `NewLexeme`, `MergeLexemes`, `NewEntitySchema`, `EntitySchemaText`, `SetEntitySchemaLabelDescriptionAliases` | reserved | Lexeme and EntitySchema are reserved, not implemented |

### 11. API (extends 0012 §4 and §5)

**Action API.**

- `meta=siteinfo&siprop=specialpagealiases` reports from the registry (§2).
- `list=querypage`:
  - `qppage` accepts the `mediawiki_name` of every served MediaWiki report, and the names of served Wikibase and Triplespace reports.
  - A deferred or declined name is refused with `ts-report-unavailable`, which says which.
  - `qpoffset` continues. A batch report returns `cached`, `cachedtimestamp` and `maxresults`.
  - `generator=querypage` works as in MediaWiki.
- The forms of §5 use the existing `wb*` modules. Export is `action=query&export`.

**REST**, under `rest.php/triplespace/v0`:

| Route | |
|---|---|
| `GET /reports` | Every report with its backing, mode, graph scope, lag or `computed_at` |
| `GET /reports/{name}?graphs=&limit=&continue=` | A report's rows, with its own filters (`language`, `type`, `datatype`, `state` and so on) |
| `GET /nuke/preview?target=&tag=&subsidiaries=&from=&until=&pattern=&namespace=` | The candidates of §7, grouped |
| `POST /nuke` | Starts the job; returns its ID. Reverting is `POST /jobs/{id}/revert`. |
| `POST /export` | Streams an export, or starts an export job and returns its ID |
| `GET /export/{id}` | An export job's state and, when ready, its download |

### 12. Permissions (extends 0016 §2)

| Permission | Governs | Default groups |
|---|---|---|
| `nuke` *(MediaWiki's, from the Nuke extension)* | Running Nuke (§7). Each write it makes also needs its own right: `delete`, and `deleterevision` for hiding. | `sysop` |

- `nuke` joins the `delete` grant in `grants.toml`, so a subsidiary or OAuth consumer granted deletion can nuke.
- Configuring report modes and widening needs `ts-config` at the farm base, an instance right (§4.3).
- Export needs `read`. The forms need the rights their API modules already need.

The `job` rate class gains a `sysop` row, 10 per hour (amends [0024](0024-subsidiary-accounts.md) §5, which gave only `user` and `bot`). Without that row an administrator who is not a bot could not start a Nuke or a job revert.

### 13. Storage (amends 0013 §5 and §7)

**`view.page` gains four columns.** They are maintained by the page projection:

- `len integer`: bytes of the latest text, as MediaWiki's `page_len`;
- `latest_at timestamptz`;
- `revisions integer`;
- `random double precision`: drawn at creation, as `page_random`.

Each has an index with `ns`. **`view.actor` gains `last_active timestamptz`.**

**Report tables:**

```sql
CREATE TABLE view.report_entry (                -- §4.3: projection and batch reports
  tenant bigint NOT NULL, report text NOT NULL, scope text NOT NULL DEFAULT 'local',  -- graph set (§4.2)
  key text NOT NULL, sort_value bigint, detail jsonb,
  PRIMARY KEY (tenant, report, scope, key)
);
CREATE INDEX report_entry_order ON view.report_entry (tenant, report, scope, sort_value DESC, key);
CREATE TABLE view.report_state (
  tenant bigint NOT NULL, report text NOT NULL, scope text NOT NULL,
  mode text NOT NULL,                           -- projection | batch
  computed_at timestamptz, row_count bigint,
  PRIMARY KEY (tenant, report, scope)
);
CREATE TABLE view.site_stats (                  -- §9: MediaWiki's site_stats
  tenant bigint PRIMARY KEY,
  pages bigint, articles bigint, edits bigint, files bigint, users bigint, active_users bigint, entities bigint
);
```

`articles` counts main-namespace pages with at least one link, MediaWiki's default `$wgArticleCountMethod`.

**Projection order:**

- The `report` and `site_stats` projections run in step 5 of 0013 §7, after `activity` and `page_link`. The refresh job applies their deltas for the tables it writes (§4.3).
- Batch runs and export jobs are `ops` work, beside the export jobs 0013 §5.6 already lists.

### 14. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed. The table this section first gave is in A1.

## Consequences

- **One list answers whether Triplespace has a given special page,** and, where it does not, why not. The checker keeps the ADRs and the list from drifting apart.
- **MediaWiki's names keep working** for renamed pages and for pages folded into `Special:Account`. A help page or tool that links to `Special:AbuseFilter` or `Special:BotPasswords` lands in the right place.
- **Reports describe the wiki, not its mirrors.** A tenant mirroring Wikidata gets `ItemsWithoutSitelinks` for its own items, not for a hundred million of Wikidata's. Mirror-scale reports are possible, but only as configured batch runs.
- **Reports are live without miser mode.** The price is one more projection and some fan-out per link and category change. The instance's `reports` configuration is the escape hatch where a tenant's write rate makes that too costly. Operators can apply it to the whole instance or to one tenant.
- **Nuke is a job, so it is one row, one patrol mark and one undo.** It reaches entities, statements, files and posts as well as pages, and it can go back further than recent changes.
- **Exports can be verified.** The `records` format lets someone who holds an export prove what the wiki held, which suits Scatterbase's purposes as well.
- **Some familiar pages wait on other designs.** `LinkSearch` and `PagesWithProp` wait on tables that 0042 implies but never defines. The redirect reports wait on [0008](0008-namespaces-and-document-pages.md) Q8.

## Open questions

- **Q1. The external-links table.** Its shape (MediaWiki's `externallinks` stores reversed domains for prefix search) decides `LinkSearch`, `list=exturlusage`, and edit-filter variables over added links.
- **Q2. A page-properties table.** It would serve `PagesWithProp`, `list=pageswithprop` and `prop=pageprops`.
- **Q3. Global Nuke.** Whether a global group at the farm base ([0028](0028-tenancy-policy.md) §3) may nuke a farm account across every tenant it is linked to, as one job per tenant or as an instance act ([0040](0040-instance-prerogatives.md)).
- **Q4. Page statements in exports.** Whether to carry them in MediaWiki XML as an extra slot, which only Triplespace would read, or only in `records`.
- **Q5. Mute.** Whether `Special:Mute` and user-level muting of notifications ([0021](0021-notifications.md)) and email should exist at all.
- **Q6. Reports over mirror graphs.** Which batch reports over a mirror are worth offering by default, for example mirrored properties without a local constraint, and whether their snapshots should be shared across tenants that mirror the same provider.
- **Q7. Thresholds.** The defaults are placeholders: `nuke.confirm_threshold` (500), `export.sync_revisions` (5,000), `export.max_history` (1,000) and the `reports` `batch_limit` (1,000).

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §7 | §6 | extends | 0002 A16 |
| [0004](0004-identity-clusters-and-equivalence.md) §10 | §5–6 | extends | 0004 A8 |
| [0005](0005-crate-organization.md) §2 | §14 | extends | 0005 A47 |
| [0007](0007-actor-identity.md) Q7 | §9 | settles | 0007 Q7 |
| [0008](0008-namespaces-and-document-pages.md) §9 | §8 | extends | 0008 A16 |
| [0010](0010-site-ui.md) §2, §3, §12 | §1, §5, §9 | extends | 0010 A27 |
| [0013](0013-postgres-storage.md) §5.6 | §4.3, §13 | amends | 0013 A19 |
| [0013](0013-postgres-storage.md) §7 | §4.3, §13 | extends | 0013 A19 |
| [0015](0015-record-format-and-partition-registry.md) §3, §5 | §1, §4.3 | extends | 0015 A21 |
| [0016](0016-permissions-and-access-control.md) §2 | §12 | extends | 0016 A16 |
| [0018](0018-tenants.md) §11 | §3 | extends | 0018 A9 |
| [0023](0023-moderation.md) §9, §11 | §7, §12 | extends | 0023 A6 |
| [0024](0024-subsidiary-accounts.md) §5 | §8, §12 | amends | 0024 A7 |
| [0029](0029-resolver-namespaces.md) §3 | §5, §9 | extends | 0029 A4 |
| [0031](0031-property-constraints.md) §3 | §4.2 | amends | 0031 A2 |
| [0042](0042-template-expansion-and-parsoid.md) §10 | §4.3, §13 | extends | 0042 A3 |
| [0046](0046-primary-tenant.md) §7 | §3 | extends | 0046 A2 |

## References

- [MediaWiki `SpecialPageFactory`, REL1_43](https://github.com/wikimedia/mediawiki/blob/REL1_43/includes/specialpage/SpecialPageFactory.php) (the core list and its groups) and [`MessagesEn.php`](https://github.com/wikimedia/mediawiki/blob/REL1_43/languages/messages/MessagesEn.php) (English aliases)
- [Wikibase `extension-repo.json` and `extension-client.json`, REL1_43](https://github.com/wikimedia/mediawiki-extensions-Wikibase/tree/REL1_43) (repository and client special pages)
- [Help:Special pages](https://www.mediawiki.org/wiki/Help:Special_pages), [Manual:$wgMiserMode](https://www.mediawiki.org/wiki/Manual:$wgMiserMode), [Manual:updateSpecialPages.php](https://www.mediawiki.org/wiki/Manual:UpdateSpecialPages.php), [Manual:$wgQueryCacheLimit](https://www.mediawiki.org/wiki/Manual:$wgQueryCacheLimit)
- [API:Querypage](https://www.mediawiki.org/wiki/API:Querypage), [API:Siteinfo](https://www.mediawiki.org/wiki/API:Siteinfo) (`specialpagealiases`, `statistics`)
- [Extension:Nuke](https://www.mediawiki.org/wiki/Extension:Nuke)
- [Help:Export](https://www.mediawiki.org/wiki/Help:Export), [Manual:$wgExportMaxHistory](https://www.mediawiki.org/wiki/Manual:$wgExportMaxHistory)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-30
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §14
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A47).

Replaced text (§14):

> | Crate | Change |
> |---|---|
> | `triplespace-titles` | Embeds `special-pages.toml`. Resolves `Special:` names, aliases (case-folded) and section aliases, and applies the farm/tenant scope rules (§1–3). |
> | `triplespace-projections` | The `report` and `site_stats` projections, the new `page` and `actor` columns, and the batch report runner (§4, §13) |
> | `scatter-integrity` | Building and verifying a `records` export: a selection of records with inclusion proofs (§8) |
> | `triplespace-api-action`, `triplespace-api-rest` | `specialpagealiases`, `list=querypage`, `export`, and the routes of §11 |
> | `triplespace-server` | Serves the pages the registry marks `served`, including the forms of §5 and the pages of §6–9 |
> | `triplespace-cli` | `verify --records` |
>
> No crate is added.

### A2. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §14–15
- **Summary:** A1 was folded into the Decision. The open questions were numbered, and the list this ADR kept as §15, "Changes to other ADRs" (quoted below), was replaced by the generated table, so the Decision now ends at §14. No decision changed. Before this, A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

Replaced text (§15):

> - **[0002](0002-source-graphs-and-mass-ingest.md) §7 and [0004](0004-identity-clusters-and-equivalence.md) §10:** the correction list and the conflict list are `Special:Corrections` and `Special:IdentityConflicts` (§6).
> - **[0005](0005-crate-organization.md) §2:** the crate changes listed above.
> - **[0007](0007-actor-identity.md):** the open question on serving `Special:Redirect/user/{id}` locally is settled: it is served, with the other `Redirect` forms (§9).
> - **[0008](0008-namespaces-and-document-pages.md) §9:** `Special:Import` is the form for the import job, and `Special:Export` its counterpart (§8).
> - **[0010](0010-site-ui.md) §2, §3 and §12:** the Special pages link goes to `Special:SpecialPages`, the **New** menu to `NewItem` and `NewProperty`, and the full results page is `Special:Search`. The address table is now the registry.
> - **[0013](0013-postgres-storage.md) §5.6 and §7:** the tables, columns and projection order of §13.
> - **[0015](0015-record-format-and-partition-registry.md) §3 and §5, and [0016](0016-permissions-and-access-control.md) §2:** the `reports` config kind, a new registry file, and `nuke`.
> - **[0018](0018-tenants.md) §11 and [0046](0046-primary-tenant.md) §7:** the scope rule of §3.
> - **[0023](0023-moderation.md) §9 and §11:** `Special:Nuke` and `nuke`.
> - **[0024](0024-subsidiary-accounts.md) §5:** the `export` class and the `sysop` row of the `job` class.
> - **[0029](0029-resolver-namespaces.md) §3:** "Create an item with this DOI" opens `Special:NewItem` with the statement filled in.
> - **[0031](0031-property-constraints.md) §3:** `ConstraintReport` defaults to the local graph.
> - **[0042](0042-template-expansion-and-parsoid.md) §10:** the refresh job applies report deltas for the tables it writes (§4.3); `view.site_stats` serves the `NUMBEROF*` variables (§13).
