# 0010. Site UI

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-01 (A31)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md)
- **Uses:** [0000](0000-init.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

[0003](0003-statement-ui.md) specifies how statements are drawn on an entity page. The rest of the site is still unspecified:

- the frame around every page;
- search;
- document pages ([0008](0008-namespaces-and-document-pages.md));
- history and diffs;
- recent changes and contributions;
- import jobs;
- logging in and account settings.

[0000](0000-init.md) notes that Triplespace does not come with MediaWiki's editing UI, accounts, permissions or watchlists. [0008](0008-namespaces-and-document-pages.md) organizes the user experience like MediaWiki's, so users trained on MediaWiki should find the same places: page tabs, `Special:` pages and familiar URLs.

Copying MediaWiki's special pages is not enough, because three things differ.

1. **A page's history is not one sequence.** An entity page is a view over several source graphs ([0002](0002-source-graphs-and-mass-ingest.md) §3), and over every member of an identity cluster ([0004](0004-identity-clusters-and-equivalence.md)). Its records sit in several partitions with different history policies:
   - Under `latest`, a mirror keeps only the newest state of each entity.
   - Upstream history lives on the provider's own site.
   - An erased record leaves only its header ([0006](0006-log-integrity-and-erasure.md) §7).
2. **Most changes are not edits by people.** A single Wikidata sync or citation import can write millions of records. MediaWiki's recent changes has one row per revision. At this volume, that would make the list useless.
3. **Identity is plural and names are erasable.** Each issuer has its own accounts. Linking accounts is opt-in, and sign-in bindings are private ([0007](0007-actor-identity.md)). The UI has to say which things are public and which are private.

The design exploration is on the [site UI canvas](https://claude.ai/artifact/XNbmnySA1RViHpnH2ecFQY). It uses the visual language of the [statement UI canvas](https://claude.ai/artifact/69M2T2HYrkafzearFUoJ4X). This ADR records the rules the site follows.

## Decision

### 1. Principles

*Changed by A22.*

These extend the principles of [0003](0003-statement-ui.md) §1. That ADR's principles still apply: show the exception, not the rule; name the effect, not the mechanism; nothing is lost.

1. **One frame for every page.** An entity, a document page, a user, a job and a special page all get the same header: an identity line, the title, and tabs. A reader learns the frame once. A frame's tabs show data only about what its identity line names; a page's statements are never drawn in its item's frame, or the reverse, and crossing from one to the other is a link in the identity line ([0038](0038-page-metadata-and-categories.md) §7).

2. **Show the exception, applied to activity.** Mirror syncs and bulk jobs are routine. Edits made by people on this instance are the exception, and they are listed one by one. A marker that would be the same on every row is not drawn. Examples:
   - A page in its namespace's default content model shows no model.
   - A local-only item shows no source switch.
3. **Group by cause.** A change that one job or one sync made is shown as part of that job or sync. It is never shown as millions of separate rows.
4. **Say what is missing.** Nothing is silently absent. Each of these is shown in place, as a labelled gap:
   - mirror states that compaction dropped;
   - erased records;
   - hidden usernames;
   - templates that were not rendered;
   - categories from an imported wiki.
5. **Read sources together; compare them apart.** Lists merge all sources by time, and each row carries its source. Comparing two states works only within one source (§5.6).
6. **Label privacy where it is decided.** Any control that shares something about a person carries a **Public** or **Private** label, next to the control. It is never only in a help page.
7. **Every change can be traced and undone.** A row names the account that made the change. A bot also names its operator ([0007](0007-actor-identity.md) §6). An edit can be undone, a job reverted, and any record checked against a signed checkpoint.
8. **Fetch upstream history; don't copy it.** Upstream history the instance does not keep is fetched live from the provider when a reader asks, and marked as such. It is never written to the log (§5.5).

### 2. The frame

*Changed by A5, A9, A11, A12, A13, A16, A20, A22, A23, A27, A29, A30.*

**The global header** holds:

- the site name, linking home;
- one search box (§3);
- links to Recent changes, Jobs and Special pages;
- a **New** menu for creating items, properties and pages;
- the account menu, or **Log in**;
- the notifications **bell**, with the unseen count, opening the inbox as a panel ([0021](0021-notifications.md) §6).

**Special pages** links to `Special:SpecialPages`, the index of every page `docs/registry/special-pages.toml` marks served, grouped, and filtered by the viewer's rights. The **New** menu opens `Special:NewItem` and `Special:NewProperty` for entities, and the edit form for pages ([0047](0047-special-pages.md) §5, §9).

**The page header** has three parts:

1. **An identity line.** It shows a chip for the ID or namespace, the kind of page, and provenance where there is any, such as "Foreign item, minted by Wikidata" or "First 14 revisions imported from Librarybase".
2. **The title.**
3. **The page's tabs:**

| Page kind | Tabs |
|---|---|
| Entity view | Statements, Identifiers, Sitelinks, Labels in other languages, History, Links here, and Talk ([0019](0019-discussions.md) §8); a property adds Constraints ([0031](0031-property-constraints.md) §7) |
| Document page | Read, Edit, Page data ([0038](0038-page-metadata-and-categories.md) §7), History, Links here, and Talk |
| Thread, talk page | The posts as a tree, or the list of threads, with History ([0019](0019-discussions.md) §8) |
| File page | The file, its versions, its usage and metadata ([0039](0039-files-and-media.md) §18) |
| User | User page, Contributions, Jobs, Links here |
| Job | Summary, Changes, Rejected |
| Special page | None |

- **Entities have no Edit tab.** They are edited in place ([0003](0003-statement-ui.md) §8).
- **Less common actions** go in the page's overflow menu. For a document page these are Move, Change content model, **Protect…** ([0016](0016-permissions-and-access-control.md) §7) and **Delete…**, with an option to delete the talk page too; a deleted page shows **Undelete…** to those who may ([0023](0023-moderation.md) §9).
- **Links here** is served by the links projection ([0008](0008-namespaces-and-document-pages.md) §10).
- **The Page data tab** shows a document page's or thread's own statements in the statement UI, with projected statements read-only. A page paired with an item through a sitelink shows "About: {label} ({ID})" in its identity line, and the item shows "Article: {title}" ([0038](0038-page-metadata-and-categories.md) §7).
- **The Sitelinks tab** groups links by host, each host a Domain chip linking to `Domain:{host}` ([0026](0026-sitelinks.md) §9).
- **A deleted page or entity** shows, to everyone outside the deleting group, the frame with the deletion log entry in place of the content ([0023](0023-moderation.md) §9).
- **A file page** shows, for a taken-down version, the operator's notice in place of the file ([0039](0039-files-and-media.md) §18).
- **A talk page's header** shows its `Group`'s fediverse handle when its namespace is federated, and a mirrored entity's identity line carries a `verified` chip when its provider is verified ([0022](0022-federation.md) §11).
- **A page served by a page repository** carries the repository's origin chip and an identity line naming the upstream revision and licence; a title with alternates gains **Other versions**, listing each alternate its repositories and the page's `page-alternates` statement allow, reached with `origin={repository}`; its Edit tab is the fork form ([0052](0052-page-repositories-and-title-inheritance.md) §5). A **fork** reads "Forked from English Wikipedia at revision N", and a page followed through a redirect shows "(Redirected from …)" ([0054](0054-forking-a-mirrored-page.md) §6, [0051](0051-page-redirects.md) §2).

**Chips** are the shared marks for kinds of thing. The set is kept small.

| Chip | Form | Meaning |
|---|---|---|
| Provider code | Monospace, one colour per provider, taken from the provider registry | The entity or row comes from that provider's graph |
| Local | Pill | Made on this instance |
| Namespace | Neutral tag | `Project`, `User`, `Special`, and so on |
| Job | Dark tag | A row or page that stands for an import job |
| Log | Outlined tag | A log action, not a content change |
| Corrected here | Outlined pill | A local correction of a mirrored value ([0003](0003-statement-ui.md) §4) |
| Best value | As in [0003](0003-statement-ui.md) §9 | |
| Public / Private | Pill, with a lock icon on Private | The visibility of an account setting (§1.6) |

### 3. Search

*Changed by A4, A7, A18, A22, A27.*

- **One box covers everything.** It searches entity labels, descriptions and aliases across every provider, domain keys ([0009](0009-keyed-entity-types-and-domain.md)), and page titles.
- **Suggestions are grouped by kind:** items and properties, foreign entities by type (for example "Sources and works"), domains, and pages. Each suggestion shows its label, its description, and its ID chip.
- **An ID or domain key jumps straight to its page.** Input that parses as an ID, a keyed ID or a bare key of a keyed type gets a "Go to" option as the first suggestion, and Enter follows it ([0017](0017-entity-id-grammar.md) §1, §4). So does a resolver key, such as `DOI:10.1000/xyz`, which lands on the item it resolves to or on a disambiguation page in the site frame ([0029](0029-resolver-namespaces.md) §7). Parsing and resolution use the title resolver ([0008](0008-namespaces-and-document-pages.md) §3), so a non-canonical cluster member lands on its canonical entity ([0004](0004-identity-clusters-and-equivalence.md) §4). When a main-namespace page has the same title as the ID, it is offered as the next suggestion, "Page titled Q42" ([0038](0038-page-metadata-and-categories.md) §8).

- **The full results page** is `Special:Search`, with MediaWiki's parameters ([0047](0047-special-pages.md) §9). It also searches page text, through a second index queried together with the first ([0014](0014-caches-and-search.md) §7).

### 4. Document pages

*Changed by A21, A22, A25, A26, A30, A31.*

**Read**

- Renders the page's content model ([0008](0008-namespaces-and-document-pages.md) §5, §8).
- **Syntax outside the wikitext subset stays visible:**
  - With the tenant's expansion off, a template call renders as a "Template not rendered" chip showing the call. With it on, templates are expanded before rendering, only constructs outside the subset render as chips, and a missing template is a red link ([0042](0042-template-expansion-and-parsoid.md) §7).
  - Categories are listed at the foot of the page as links to their category pages, with hidden categories collapsed ([0038](0038-page-metadata-and-categories.md) §3).

- **Links to entities** render with the entity's label.
- **An "About this page" panel** shows the last edit, the revision count by origin (imported, bot, local), where the page came from, the content model, and the backlink count; with expansion on, also "Templates used" and the limit report, and on a Template page "Pages that use this template" ([0042](0042-template-expansion-and-parsoid.md) §7). On a fork it also shows the revisions imported, the dependencies copied, whether files were copied, how many newer revisions upstream has, **Compare with upstream** and **Copy files used by this page** ([0054](0054-forking-a-mirrored-page.md) §6–7). A page's heading is its `displaytitle` where one is set, and a disambiguation page is marked as one ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6).

**Edit**

- **The source and a live preview sit side by side.** The preview runs the server's own renderer compiled to WebAssembly (`scatter-wasm`, [0034](0034-frontend-stack.md) §6). A page that needs expansion previews through `action=parse` on the server instead ([0042](0042-template-expansion-and-parsoid.md) §15).
- **A Format selector** changes the content model (`action=changecontentmodel`). It, and the model shown beside the title, appear only for text models, and the selector only where the namespace allows more than one model ([0041](0041-content-models.md) §10).
- **Link autocomplete** opens after `[[` and suggests titles and entities in any namespace, through the title resolver. For an entity, it inserts the link and the entity's label.
- **Insert template…** searches Template titles through the stack and builds a parameter form from the template's TemplateData; a parameter popup lists a template's parameters when the cursor is inside a call ([0055](0055-templatestyles-templatedata-and-page-properties.md) §5). **The Edit tab of an inherited page** opens the editor on the upstream wikitext with the fork banner, and saving forks ([0054](0054-forking-a-mirrored-page.md) §2).
- **Saving** asks for a summary and a minor-edit flag, and sends the base offset ([0008](0008-namespaces-and-document-pages.md) §4). On a conflict, the editor keeps the user's text and marks only the lines that clash.

### 5. History

#### 5.1 What a history covers

*Changed by A23.*

The history of an entity page lists every record whose key is:

- the entity itself; or
- any member of its identity cluster ([0004](0004-identity-clusters-and-equivalence.md)), with a chip showing which member.

It covers records in every source partition:

- local change sets;
- mirror records;
- log events such as retention changes, `convert`, redirects and `erase`.

A document page's history lists its records in the `pages` partition. A file page's interleaves uploads with text and statement revisions ([0039](0039-files-and-media.md) §18).

#### 5.2 Rows

*Changed by A3, A5, A13, A24.*

| Row | Shown as |
|---|---|
| Local edit | One row: time, Local chip, parsed summary, tags, size change, and actor. It links to its diff. |
| Mirror sync | One row per observed state: time, provider chip, "Synced from Wikidata", the upstream revision, the job, and a one-line account of what changed. It links to its diff. |
| Upstream edit | Folded inside the sync that brought it in (§5.5), with the upstream actor linked to their upstream IRI ([0007](0007-actor-identity.md) §2). |
| Erased record | A gap row showing only its time and source: "An edit was erased." Its reason class is shown to holders of `ts-viewerasures` ([0016](0016-permissions-and-access-control.md) §6). |
| Hidden actor | The row, with "(username hidden)" in place of the name. |
| Log event | A Log chip and a sentence describing the effect. It has no diff. |
| Instance act | The instance operator as its actor, an **Instance action** label, the public reason and a link to `Special:InstanceAction/{partition}/{offset}` at the farm base ([0040](0040-instance-prerogatives.md) §7) |

**Rows are identified by time and source,** and may show revision numbers: local and mirror records have global revision IDs ([0013](0013-postgres-storage.md) §6, [0015](0015-record-format-and-partition-registry.md) §2).

**Hiding and patrol.** History rows carry RevisionDelete checkboxes for holders of `deleterevision`, and a suppress option for holders of `suppressrevision`; a hidden part is drawn in place and labelled, as a hidden actor is. The patrol marker and **Mark as patrolled** appear for holders of `patrol` ([0023](0023-moderation.md) §9).

**Linked accounts are marked.** An upstream actor that a local user has linked ([0007](0007-actor-identity.md) §7) carries an "also {name} here" badge. Unlinked accounts are never matched by name.

#### 5.3 The source switch

- **A switch above the list** offers All sources, Made here, and one entry per mirror that contributes to the entity. The default is All sources.
- **It is hidden when only one source contributes.** A purely local item's history then looks like MediaWiki's.

#### 5.4 Mirror states the instance did not keep

Under the `latest` history policy ([0002](0002-source-graphs-and-mass-ingest.md) §2), only the newest mirrored state exists. The history says so in its side panel ("Only the latest state is kept here"), and it offers two actions:

- a link to the full history on the provider;
- **Keep full history here…**, which sets `retain` and starts the upstream backfill ([0002](0002-source-graphs-and-mass-ingest.md) §5).

Under `full`, or for a retained entity, every observed state is a row, and backfilled upstream revisions are read from the log.

The foot of the list states where history on this instance begins, for example "first mirrored on {date}; that state was replaced by later syncs".

#### 5.5 Upstream edits, fetched live

- **A checkbox shows or hides upstream edits** inside each sync: "Show Wikidata's own edits inside each sync". A user preference sets its default.
- **Where the instance has not backfilled the upstream revisions a sync covers, the server fetches them from the provider's API.** It uses the endpoint in the provider registry, and the interval runs from the previous observed upstream revision to the current one.
- **The results are cached briefly and rate-limited, and never written to the log.** The fold is labelled "fetched from wikidata.org, not stored here".
- **If the provider cannot be reached,** the fold says so. The rest of the history is unaffected.
- **Providers that publish no revision history,** such as OpenAlex, get no fold.

#### 5.6 Comparing

- **Compare works within one source.** A reader can compare two local revisions, or two observed states of a mirror that keeps them.
- **The resolved view at a past time cannot be compared.** It depends on mirror states that compaction may have dropped. This is why the compare controls appear only on rows whose source keeps history.
- **A comparison between two local revisions** shows the change in the local graph's own assertions, drawn as in §6.

#### 5.7 Narrower histories

- **"History of this value"** in a value's menu ([0003](0003-statement-ui.md) §9) opens the same list, filtered to one statement ID.
- **A log view** lists only log events for the entity.

### 6. Diffs

**The unit** is either one change set or two states of one source (§5.6).

**Diffs are drawn as statements.**

- Changed values are shown in their group's shape ([0003](0003-statement-ui.md) §3), with the unchanged values on either side for context, and a "Show all" link.
- Changed cells carry insertion and deletion marks.
- A rank change is drawn as "Normal → Deprecated". An added qualifier is marked as new.
- Terms and sitelinks are diffed as keyed lists.
- A document page gets a line diff of its source, as in MediaWiki.

**Alternative views.** A switch offers JSON (a diff of the canonical JSON) and Change set (the record's operations as stored).

**"What this changes"** states the effect in three places:

- **Readers here:** what the resolved view now shows.
- **Queries:** whether the truthy triples changed.
- **Upstream:** whether anything was written upstream. For a correction, the answer is no, and the page offers "Report to Wikidata…".

**"About this edit"** lists:

- the graph written to;
- the statement ID;
- the size change;
- the log checkpoint that includes the record, with a link to get an inclusion proof ([0006](0006-log-integrity-and-erasure.md) §9).

**Actions.** Undo appends the inverse change set, with the record as its base.

### 7. Recent changes

*Changed by A10, A13, A19.*

**Row kinds:**

| Kind | One row per |
|---|---|
| Item and property edits | Local change set |
| Page edits | Page record |
| Jobs | Local job, however many entities it touched |
| Log actions | Log event: links, erasures, retention, account creation |
| Mirror syncs | Sync job run, never per entity |

- **Filters** toggle each kind, and the `patrolled` filter shows patrol state ([0023](0023-moderation.md) §9). Mirror syncs are off by default. While they are off, a strip above the list summarises running and recent syncs, and offers to show them.
- **"Group by page"** collapses consecutive edits to the same page into one row, with a count.
- **The time window is configurable.** The default follows MediaWiki's `$wgRCMaxAge`, 90 days.
- **Every feed has an Atom form and a stream** beside the page ([0020](0020-change-feeds.md) §4).
- **A refused write** shows the edit filter's public message in place, and filter tags are chips on the rows they mark ([0030](0030-edit-filters.md) §10).

**The Action API stays per revision.** `list=recentchanges` and `list=usercontribs` return local changes one revision at a time, as MediaWiki clients expect.

- **Changes made by a job** carry the bot flag and a `job:{id}` change tag, so `rcshow=!bot` hides them.
- **Mirror records are not in `list=recentchanges`.** Each mirror job run appears in `list=logevents` under a new log type, `job`.

### 8. Contributions

*Changed by A8, A14.*

- **Contributions are per account** ([0007](0007-actor-identity.md), Consequences), and an account belongs to one tenant. `Special:GlobalContributions/{name}` shows the same for every account linked to it on the tenants the viewer's tenant has opted into. For a foreign actor, `Special:Contributions/{issuer}:{id}` shows what the instance holds and says so; it never fetches upstream ([0018](0018-tenants.md) §8).
- **The header** shows the account's linked accounts, as links to their upstream pages. Only links the holder created are shown.
- **Jobs run for the user** appear as single rows, and can be filtered out.
- **A bot account** names its operator, and a primary account's header lists its subsidiaries ([0024](0024-subsidiary-accounts.md) §9).
- **Pages are addressed by current name.** Old names do not resolve ([0008](0008-namespaces-and-document-pages.md) §6).

### 9. Jobs

`Special:Jobs` lists running and recent jobs. `Special:Jobs/{id}` is a job's page, with these parts:

- **Identity line:** the Job chip, the job ID, the graph it wrote to, and its status.
- **Header:** the actor and the account it ran for, the start and end times, the mode, and the actions.
- **Counts:** created, added to (found by match key), unchanged, and rejected. Each count links to a filtered Changes tab.
- **Rejected records:** the line, the match key and the reason for each, and a link to download the rejects file ([0002](0002-source-graphs-and-mass-ingest.md) §8.5).
- **Where it came from:** the source and its version, the adapter and its version, the match key, the graph, the checkpoint and inclusion proof, and projection lag ([0002](0002-source-graphs-and-mass-ingest.md) §8.3).
- **A running job** shows progress, and how far the indexes are behind.
- **A `snapshot` job** shows how many entities it removed, next to the safety threshold ([0002](0002-source-graphs-and-mass-ingest.md) §8.4).

**Actions:**

- **Revert this job…** (local jobs) appends the inverse change sets as a new job ([0002](0002-source-graphs-and-mass-ingest.md) §8.3). Where someone has edited an affected entity since, that later edit is kept, and those entities are listed first for review.
- **Sync again** (mirror jobs) is the mirror's form of a revert.
- **Run again** repeats a local job with the same parameters. Match keys make this idempotent.

### 10. Logging in

*Changed by A6.*

**`Special:UserLogin`**

- Offers one button for each issuer that the issuer registry allows as an identity provider ([0007](0007-actor-identity.md) §1). It says what the instance learns: which account the user holds, and nothing else.
- Explains temporary accounts, and says that IP addresses are never shown or published ([0007](0007-actor-identity.md) §3).
- States that nobody else can see which service a user logs in with.

**The first login** continues to `Special:CreateAccount`, where the user chooses a public name.

**Name rules.** A local account name follows MediaWiki's username grammar exactly, so that imported attributions, CentralAuth names and Pywikibot round-trip: Unicode letters, digits and spaces; none of `@ # : / < > [ ] | { }`, control characters or leading, trailing or doubled spaces; at most 255 bytes; normalized to NFC with underscores read as spaces and the first letter uppercased. **Uniqueness is case-insensitive**, as the `actor_local_name` index of [0013](0013-postgres-storage.md) §5.4 already has it. **A name is never reused.** Every name a local account has ever held, whether renamed away or vanished from, stays reserved as a *name tombstone*, so that `[[User:OldName]]` in old page text or a post can never come to name a different person; the only way to have a name is to have always had it. Because a vanished account's names are erased from the log ([0007](0007-actor-identity.md) §4), the tombstone is not a projection: it is a keyed hash of the lower-cased name in `private.name_tombstone` ([0013](0013-postgres-storage.md) §5.6), written when a name is released, holding no actor key and no plaintext, and revealing nothing but that a candidate name is reserved, which the "name taken" check reveals anyway. `Special:CreateAccount` and `renameuser` refuse a tombstoned name with `ts-name-reserved`. The subsidiary conventions of [0024](0024-subsidiary-accounts.md) §2 and [0025](0025-oauth-server.md) §3 apply on top of this grammar, and the farm name registry of [0028](0028-tenancy-policy.md) §2 is this rule applied across tenants, tombstones included.

- **The name field starts empty.** The identity provider's username may be offered as a button, labelled with where it came from. Next to it, the page says that using the same name makes the two accounts easy to connect ([0007](0007-actor-identity.md), Consequences).
- **An optional, unchecked box** offers to link the user's account on the identity provider's wiki, for example Wikidata. It says the link is public. If it is checked, the link is created by the flow in [0007](0007-actor-identity.md) §7. The authentication that just took place serves as the fresh proof of control, because it happened in the same request flow.

### 11. Account settings

*Changed by A10, A11, A12, A14, A15, A17.*

`Special:Account` is visible only to its holder. It has these sections:

| Section | Label | Contents |
|---|---|---|
| Name | Public | The current name and **Rename…**. The page warns that user pages move and that links using the old name stop working ([0008](0008-namespaces-and-document-pages.md) §6). |
| Sign-in methods | Private | Each binding, with when it was added and last used. **Remove** is disabled for the last binding, and says why. **Add** lists the issuers allowed as identity providers. |
| Linked accounts | Public | Each link, with its date and **Unlink…**. **Link another account** requires logging in to the provider again. The page warns that unlinking erases the link from history, and that copies taken earlier may keep it ([0007](0007-actor-identity.md) §7). |
| Preferences | — | Every preference key, grouped as MediaWiki groups them ([0027](0027-preferences-and-portability.md) §7), including interface and label language, and the defaults for the upstream-edits fold (§5.5) and the mirror-sync filter (§7) |
| Notifications | Private | Reasons by channel, the email address and the fediverse handle with their verification states ([0021](0021-notifications.md) §6) |
| Watchlist token | Private | The Atom watchlist token, shown and resettable ([0020](0020-change-feeds.md) §4) |
| Subsidiaries | Public | One's subsidiary accounts; each one's **keys** are Private: label, grants, IP ranges, expiry, last used, Revoke and Issue key ([0024](0024-subsidiary-accounts.md) §9) |
| Connected applications | Private | Each OAuth authorization one's subsidiaries hold, with **Revoke** ([0025](0025-oauth-server.md) §5) |
| Fediverse | Public, and Private for keys | Make this account followable, the follower count, `rel="me"` links ([0022](0022-federation.md) §11) |
| Your data | Private | **Download my data** and **Import data…**, with what a bundle does and does not hold ([0027](0027-preferences-and-portability.md) §7) |
| Leave | — | **Vanish this account…**, with a plain account of what vanishing does ([0007](0007-actor-identity.md) §4), including that copies of posts delivered to other servers may persist ([0022](0022-federation.md) §6), and a suggestion to download one's data first ([0027](0027-preferences-and-portability.md) §7) |

`Special:Preferences` redirects to the Preferences section.

### 12. Addresses

*Changed by A3, A5, A10, A11, A19, A20, A27.*

MediaWiki's URL forms are kept so that links and tools keep working. **`docs/registry/special-pages.toml` is authoritative** for special pages: it lists each with its aliases, scope and status ([0047](0047-special-pages.md) §1). The table names the addresses this ADR relied on, and its rows are entries there.

| Page | Address |
|---|---|
| Any page | `/wiki/{title}` and `index.php?title={title}` |
| History | `index.php?title={title}&action=history` |
| Edit (document pages) | `index.php?title={title}&action=edit` |
| Diff | `Special:Diff/N` and `index.php?diff=…&oldid=N`, for local and mirror revisions alike ([0013](0013-postgres-storage.md) §6, [0015](0015-record-format-and-partition-registry.md) §2) |
| Links here | `Special:WhatLinksHere/{title}` |
| Recent changes | `Special:RecentChanges` |
| Contributions | `Special:Contributions/{name}` |
| Jobs | `Special:Jobs`, `Special:Jobs/{id}` (new) |
| Log in, first login | `Special:UserLogin`, `Special:CreateAccount` |
| Account | `Special:Account` (new) |
| Providers, consumers, pending subsidiaries | `Special:Providers` ([0022](0022-federation.md) §11), `Special:OAuthConsumers`, `Special:PendingSubsidiaries` ([0025](0025-oauth-server.md) §5) |
| Log | `Special:Log` |
| Groups, rights, blocks and protection | `Special:ListGroupRights`, `Special:UserRights`, `Special:Block`, `Special:BlockList`, `Special:ProtectedPages` ([0016](0016-permissions-and-access-control.md) §7) |
| Watchlist, notifications | `Special:Watchlist`, `Special:Notifications` ([0020](0020-change-feeds.md) §4, [0021](0021-notifications.md) §6) |
| Edit filters, constraints | `Special:EditFilter`, `Special:EditFilterLog`, `Special:Tags` ([0030](0030-edit-filters.md) §10), `Special:ConstraintReport` ([0031](0031-property-constraints.md) §7) |

### 13. Implementation

*Changed by A1, A2, A21.*

- **A new activity projection** lives in the surfaces layer ([0005](0005-crate-organization.md) §1), as `view.activity` in `triplespace-projections` ([0013](0013-postgres-storage.md) §5.5, §10). It is a time-ordered index with these inputs:
  - every record in the local, `pages` and `actors` partitions;
  - one entry for each job;
  - for each mirror partition, a pointer from each entity to its latest synced state.

  It serves recent changes, contributions, histories, `list=recentchanges`, `list=usercontribs` and `list=logevents`. Like every projection, it can be rebuilt from the log. It grows with local activity and the number of jobs, not with the size of the mirrors.
- **The upstream history fetch** is a server component. It reads the provider registry's API endpoint and reuses the adapter's parsing of upstream revisions. Its cache is operational state, not log data.
- **The frontend extends the `ui/` prototype of [0003](0003-statement-ui.md) §10.** Statement diffs reuse `scatter-wikibase-shape`, and the page editor's preview reuses `scatter-wikitext` and the markdown renderer of `scatter-pages`, all built for `wasm32-unknown-unknown` ([0005](0005-crate-organization.md) §2). Pages are rendered on the server by `triplespace-ui`, and editors load `scatter-wasm` ([0034](0034-frontend-stack.md) §2, §6).

## Consequences

- **History and recent changes stay usable at Wikidata scale.** A sync of millions of entities is one row.
- **Users trained on MediaWiki find the usual places,** with three deliberate differences:
  - Recent changes has no per-entity mirror rows.
  - Compare is limited to one source.
  - ~~Revision numbers are hidden for now.~~ *No longer: rows may show them (A3).*
- **The UI and the Action API differ on purpose.** The UI groups changes by job, and `list=recentchanges` does not. Patrol tools see job changes as bot edits carrying a job tag.
- **Live upstream fetches depend on the provider.** They are subject to its rate limits and availability, and the fold has to handle failure gracefully. Retaining an entity removes that dependency for it.
- **"Private" becomes a promise the UI makes.** Anything labelled Private must stay out of every projection, feed and export ([0007](0007-actor-identity.md) §8). A test should check that each label matches the export policy of the graph behind it.
- **The activity projection is new work.** Recent changes, histories and contributions all depend on it.
- **The set of chips needs curating.** Each new kind of row or badge competes with the ones in §2.

## Open questions

- **Q1.** ~~**Watchlists and notifications.** [0000](0000-init.md) lists watchlists among what Triplespace must rebuild or delegate. It is not settled whether a watch on an entity also covers its mirror syncs.~~ *Settled by [0020](0020-change-feeds.md) §2–3 and [0021](0021-notifications.md): a watch covers syncs by default, and notifications are 0021's.*
- **Q2.** ~~**Patrolling.** Whether local edits get patrol status, and how that interacts with job rows.~~ *Settled by [0023](0023-moderation.md) §6: MediaWiki-shaped; a job is patrolled as one row.*
- **Q3.** ~~**Global revision IDs** ([0008](0008-namespaces-and-document-pages.md)). They decide how diff URLs look, and whether rows show revision numbers.~~ *Settled by [0013](0013-postgres-storage.md) §6 and [0015](0015-record-format-and-partition-registry.md) §2; `oldid=N` and `Special:Diff/N` work for local and mirror revisions alike, and rows may show revision numbers.*
- **Q4.** ~~**Contributions of foreign actors.** Whether `Special:Contributions` should list, for a Wikidata account, the upstream edits the instance has observed or backfilled.~~ *Settled by [0018](0018-tenants.md) §8: what the instance holds, and no more; the page says so and never fetches upstream.*
- **Q5.** ~~**Visibility of erasure reasons** ([0006](0006-log-integrity-and-erasure.md), open questions).~~ *Settled by [0016](0016-permissions-and-access-control.md) §6: the gap row is public; the reason class is for `ts-viewerasures`.*
- **Q6.** ~~**Rendering.** Whether pages are rendered on the server, in a client app, or both. Reading pages should work without JavaScript.~~ *Settled by [0034](0034-frontend-stack.md) §1–5: on the server, with Codex markup; JavaScript only where editing needs it.*
- **Q7.** ~~**Mobile layouts.** None of the pages has a narrow-screen layout yet.~~ *Settled by [0034](0034-frontend-stack.md) §2, in part: one responsive site on Codex's breakpoints, with no separate mobile site. Detailed layouts are Q12.*
- **Q8.** ~~**Permissions.** Who may revert a job, see hidden usernames, erase, or run `retain` from the history page.~~ *Settled by [0016](0016-permissions-and-access-control.md) §5: `ts-revertjob`, `deletedhistory`, `ts-erase`, `ts-retain`.*
- **Q9.** ~~**Upstream fetch budget.** Cache lifetime, per-user and per-instance rate limits, and whether any provider other than Wikidata supports the fold.~~ *Settled by [0024](0024-subsidiary-accounts.md) §5 and [0015](0015-record-format-and-partition-registry.md) §5: rate limits are the `upstream` class; the cache lifetime is a [0014](0014-caches-and-search.md) tuning value; which providers support the fold is a registry flag per provider (`revision_ids`), set as each adapter is written.*
- **Q10.** ~~**Rules for local account names.** Which characters are allowed, how names are normalized, and how names that collide with names held by a vanished account are handled. [0024](0024-subsidiary-accounts.md) §2, [0025](0025-oauth-server.md) and [0028](0028-tenancy-policy.md) §2 all defer to this.~~ *Settled by A6: MediaWiki's grammar verbatim, case-insensitive uniqueness, and no name ever reused, by name tombstones in the actor projection.*
- **Q11.** ~~**Discussions.** Talk namespaces are reserved ([0008](0008-namespaces-and-document-pages.md) §2), and their UI comes with that ADR.~~ *Settled by [0019](0019-discussions.md) §8.*
- **Q12. Detailed mobile layouts.** (Rest of Q7.) Narrow-screen layouts for the pages of this ADR.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §13 | amends | 0005 A5 |

## References

- [Site UI design canvas](https://claude.ai/artifact/XNbmnySA1RViHpnH2ecFQY) (private until shared)
- [Statement UI design canvas](https://claude.ai/artifact/69M2T2HYrkafzearFUoJ4X) (private until shared)
- [Help:Recent changes](https://www.mediawiki.org/wiki/Help:Recent_changes), [Help:Page history](https://www.mediawiki.org/wiki/Help:Page_history), [Help:Diff](https://www.mediawiki.org/wiki/Help:Diff)
- [Manual:$wgRCMaxAge](https://www.mediawiki.org/wiki/Manual:$wgRCMaxAge)
- [Temporary accounts](https://www.mediawiki.org/wiki/Trust_and_Safety_Product/Temporary_Accounts)

## Amendment log

### A1. Crate names

- **Date:** 2026-09-26
- **Source:** [0005](0005-crate-organization.md) §2, revision of 2026-09-26
- **Change:** amends §13
- **Summary:** `scatter-markdown` was folded into `scatter-pages`; the crate names in §13 read as their new homes.

Replaced text (§13):

> - **The frontend extends the `ui/` prototype of [0003](0003-statement-ui.md) §10.** Statement diffs reuse `scatter-wikibase-shape`, and the page editor's preview reuses `scatter-wikitext` and `scatter-markdown`, all built for `wasm32-unknown-unknown`.

### A2. The activity projection in Postgres

- **Date:** 2026-09-26
- **Source:** [0013](0013-postgres-storage.md) §5.5, §10
- **Change:** amends §13
- **Summary:** The activity projection is `view.activity`, written by `triplespace-projections`, which absorbs `triplespace-activity`.

Replaced text (§13):

> - **A new activity projection** lives in the surfaces layer ([0005](0005-crate-organization.md) §1), in a new crate, `triplespace-activity`. It is a time-ordered index with these inputs:

### A3. Global revision IDs

- **Date:** 2026-09-26
- **Source:** [0013](0013-postgres-storage.md) §6; [0015](0015-record-format-and-partition-registry.md) §2
- **Change:** amends §5.2, §12
- **Summary:** Local and mirror records have global revision IDs, so rows may show revision numbers and `oldid=N` and `Special:Diff/N` work for both. This settled Q3.

Replaced text (§5.2):

> **Rows are identified by time and source.** Revision numbers are not shown until the question of global revision IDs is settled ([0008](0008-namespaces-and-document-pages.md), open questions).

Replaced text (§12):

> | Diff | `Special:Diff/…` and `index.php?diff=…&oldid=…`. The ID form depends on global revision IDs (open questions). |

### A4. One search, two indexes

- **Date:** 2026-09-26
- **Source:** [0014](0014-caches-and-search.md) §7
- **Change:** amends §3
- **Summary:** Entity labels and page text are two indexes queried together.

Replaced text (§3):

> - **The full results page** also searches page text. Whether one index serves both labels and text is still open in [0008](0008-namespaces-and-document-pages.md).

### A5. Groups, protection and erasure reasons

- **Date:** 2026-09-26
- **Source:** [0016](0016-permissions-and-access-control.md) §6–7
- **Change:** amends §5.2; extends §2, §12
- **Summary:** The overflow menu gains **Protect…**; group, rights, block and protection pages appear where MediaWiki users expect them; an erased row's reason class is visible to holders of `ts-viewerasures`. 0016 §5–6 also settled Q5 and Q8.

Replaced text (§5.2):

> | Erased record | A gap row showing only its time and source: "An edit was erased." Its reason class is shown only if [0006](0006-log-integrity-and-erasure.md) makes it public. |

### A6. Name rules

- **Date:** 2026-09-27
- **Source:** Direct: James, review of 2026-09-27
- **Change:** extends §10
- **Summary:** A local account name follows MediaWiki's username grammar exactly, so that imported attributions, CentralAuth names and Pywikibot round-trip: Unicode letters, digits and spaces; none of `@ # : / < > [ ] | { }`, control characters or leading, trailing or doubled spaces; at most 255 bytes; normalized to NFC with underscores read as spaces and the first letter uppercased. **Uniqueness is case-insensitive**, as the `actor_local_name` index of [0013](0013-postgres-storage.md) §5.4 already has it. **A name is never reused.** Every name a local account has ever held, whether renamed away or vanished from, stays reserved as a *name tombstone*, so that `[[User:OldName]]` in old page text or a post can never come to name a different person; the only way to have a name is to have always had it. Because a vanished account's names are erased from the log ([0007](0007-actor-identity.md) §4), the tombstone is not a projection: it is a keyed hash of the lower-cased name in `private.name_tombstone` ([0013](0013-postgres-storage.md) §5.6), written when a name is released, holding no actor key and no plaintext, and revealing nothing but that a candidate name is reserved, which the "name taken" check reveals anyway. `Special:CreateAccount` and `renameuser` refuse a tombstoned name with `ts-name-reserved`. The subsidiary conventions of [0024](0024-subsidiary-accounts.md) §2 and [0025](0025-oauth-server.md) §3 apply on top of this grammar, and the farm name registry of [0028](0028-tenancy-policy.md) §2 is this rule applied across tenants, tombstones included. This settled Q10.

### A7. Keyed IDs in the search box

- **Date:** 2026-09-27
- **Source:** [0017](0017-entity-id-grammar.md) §1, §4
- **Change:** amends §3
- **Summary:** Keyed IDs carry their type prefix; a bare key pasted into the search box is offered as its keyed ID.

Replaced text (§3):

> - **An ID or domain key jumps straight to its page.** Input that parses as an ID or domain key gets a "Go to" option as the first suggestion, and Enter follows it.

### A8. Contributions across tenants

- **Date:** 2026-09-27
- **Source:** [0018](0018-tenants.md) §8
- **Change:** amends §8
- **Summary:** Contributions are per tenant account; `Special:GlobalContributions` follows linked accounts; a foreign actor's page shows what the instance holds and never fetches upstream. This settled Q4.

Replaced text (§8):

> - **Contributions are per account** ([0007](0007-actor-identity.md), Consequences).

### A9. Discussions

- **Date:** 2026-09-27
- **Source:** [0019](0019-discussions.md) §8
- **Change:** extends §2
- **Summary:** Subject pages get a Talk tab with the thread count; thread and talk pages are new page kinds. This settled Q11.

### A10. Feeds

- **Date:** 2026-09-27
- **Source:** [0020](0020-change-feeds.md) §4
- **Change:** extends §7, §11, §12
- **Summary:** Each feed has a page, an Atom form and a stream; `Special:Watchlist` is labelled Private; the Atom token is shown and resettable in `Special:Account`. With 0021, this settled Q1.

### A11. Notifications

- **Date:** 2026-09-27
- **Source:** [0021](0021-notifications.md) §6
- **Change:** extends §2, §11, §12
- **Summary:** The bell in the global header; `Special:Notifications`; a Notifications section of `Special:Account`; a notify toggle beside the watch star; the persistent banner for an unread talk message.

### A12. Federation

- **Date:** 2026-09-27
- **Source:** [0022](0022-federation.md) §6, §11
- **Change:** extends §2, §11
- **Summary:** `Special:Providers`; a `verified` chip on mirrored entities; a Fediverse section of `Special:Account`; the talk page's handle; the vanish page says copies delivered elsewhere may persist, which §11 had been given in place.

### A13. Moderation

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §9
- **Change:** extends §2, §5.2, §7
- **Summary:** **Protect…** and **Delete…** in the overflow menu; the view of a deleted page; RevisionDelete checkboxes and suppression in history; the patrol marker and filter; the log and moderation special pages. 0023 §6 also settled Q2.

### A14. Subsidiaries

- **Date:** 2026-09-27
- **Source:** [0024](0024-subsidiary-accounts.md) §9
- **Change:** extends §8, §11
- **Summary:** A Subsidiaries section of `Special:Account`, Public, with each subsidiary's keys Private; a subsidiary's page names its operator; contributions headers name operators and list subsidiaries.

### A15. Connected applications

- **Date:** 2026-09-27
- **Source:** [0025](0025-oauth-server.md) §5
- **Change:** extends §11
- **Summary:** A Connected applications section of `Special:Account`, Private, with Revoke; `Special:OAuthConsumers` and `Special:PendingSubsidiaries`, which §12's table had been given in place.

### A16. Sitelinks by host

- **Date:** 2026-09-27
- **Source:** [0026](0026-sitelinks.md) §9
- **Change:** extends §2
- **Summary:** The Sitelinks tab groups links by host, each a Domain chip; adding a link takes a URL; a denied host is refused with the list named.

### A17. Preferences and your data

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §7
- **Change:** amends §11
- **Summary:** The Preferences section becomes the home of every preference key; a Your data section, Private, offers download and import; the Leave section suggests downloading first.

Replaced text (§11):

> `Special:Account` is visible only to its holder. It has five sections:
>
> | Preferences | — | Interface and label language, and the defaults for the upstream-edits fold (§5.5) and the mirror-sync filter (§7) |

### A18. Resolver keys

- **Date:** 2026-09-27
- **Source:** [0029](0029-resolver-namespaces.md) §7
- **Change:** extends §3
- **Summary:** A resolver key in the search box lands on the item it resolves to, or on a disambiguation page in the site frame.

### A19. Edit filters

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §10
- **Change:** extends §7, §12
- **Summary:** `Special:EditFilter`, `Special:EditFilterLog` and `Special:Tags`; a refused write shows the filter's public message; filter tags are chips.

### A20. Constraints

- **Date:** 2026-09-27
- **Source:** [0031](0031-property-constraints.md) §7
- **Change:** extends §2, §12
- **Summary:** The property page gains a Constraints tab; `Special:ConstraintReport` uses the site frame.

### A21. Server rendering

- **Date:** 2026-09-27
- **Source:** [0034](0034-frontend-stack.md) §1–6
- **Change:** amends §4, §13
- **Summary:** Pages render on the server with Codex markup; editors load `scatter-wasm` for live preview. This settled Q6, and the responsive-site half of Q7.

Replaced text (§4):

> - **The source and a live preview sit side by side.** The preview runs the server's own renderer compiled to WebAssembly ([0008](0008-namespaces-and-document-pages.md) §11).

### A22. Articles, categories and page data

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §3, §7, §8
- **Change:** amends §4; extends §1, §2, §3
- **Summary:** As noted under each section before the migration:
  - §1: A frame's tabs show data only about what its identity line names. A page's statements are never drawn in its item's frame, or the reverse; crossing from one to the other is a link in the identity line.
  - §2: Document pages and threads gain a **Page data** tab: the page's own statements in the statement UI, with projected statements read-only. A page paired with an item through a sitelink shows "About: {label} ({ID})" in its identity line, and the item shows "Article: {title}".
  - §3: When a main-namespace page has the same title as the ID, it is offered as the next suggestion, "Page titled Q42".
  - §4: Categories are listed as links to their category pages, with hidden categories collapsed.

Replaced text (§4):

>   - A template call renders as a "Template not rendered" chip showing the call.
>   - Category links are listed at the foot of the page as plain text, under "Categories from the source wiki".

### A23. File pages

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §18
- **Change:** extends §2, §5.1
- **Summary:** A file page's frame shows the file, its version history, its usage and metadata, and, for a taken-down version, the operator's notice in place of the file. Histories (§5) interleave uploads with text and statement revisions.

### A24. Instance acts in history

- **Date:** 2026-09-30
- **Source:** [0040](0040-instance-prerogatives.md) §7
- **Change:** extends §5.2
- **Summary:** A row for an instance act shows the instance operator as its actor, an **Instance action** label, the public reason and a link to `Special:InstanceAction/{partition}/{offset}` at the farm base.

### A25. The Format selector

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §10
- **Change:** amends §4
- **Summary:** The Format selector and the model shown beside the title appear only for text models, and the selector only where the namespace allows more than one model.

Replaced text (§4):

> - **A Format selector** changes the content model (`action=changecontentmodel`).

### A26. Template expansion

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §7, §15
- **Change:** amends §4
- **Summary:** With expansion on, templates are expanded before rendering and only constructs outside the subset render as chips; a missing template is a red link. Pages that need expansion preview through `action=parse` on the server. The About panel gains "Templates used" and the limit report; a Template page gains "Pages that use this template".

Replaced text: the chip rule quoted under A22, which applies only with expansion off.

### A27. Special pages

- **Date:** 2026-09-30
- **Source:** [0047](0047-special-pages.md) §1, §5, §9
- **Change:** extends §2, §3, §12
- **Summary:** As noted under each section before the migration:
  - §2: **Special pages** links to `Special:SpecialPages`, the index of every page `docs/registry/special-pages.toml` marks served, grouped, and filtered by the viewer's rights. The **New** menu opens `Special:NewItem` and `Special:NewProperty` for entities, and the edit form for pages.
  - §3: The full results page is `Special:Search`, with MediaWiki's parameters.
  - §12: `docs/registry/special-pages.toml` supersedes this table: it lists every special page with its aliases, scope (§3 there) and status. The rows above are entries in it.

### A28. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–13
- **Summary:** A1–A27 were folded into the Decision, the open questions were numbered, Q7's open half became Q12, and the consequence that hid revision numbers was struck. No decision changed. Before this, A6 and A22–A27 were blockquotes; A12 and A15 had been written into §11 and §12 in place; the other entries were recorded only in this ADR's header or in other ADRs. The file before conversion is commit `0b26a3a`.

### A29. Origin chips and Other versions

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §5
- **Change:** extends §2
- **Summary:** A page served by a page repository carries an origin chip and an identity line naming the upstream revision and licence; a title with alternates gains the Other versions menu; the Edit tab of an inherited page is the fork form.

### A30. Forks in the frame and the About panel

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §2, §6–7
- **Change:** extends §2, §4
- **Summary:** The fork's identity line, the About panel's fork information and controls, the fork banner on the edit form, and the redirect notice of 0051 §2.

### A31. Templates in the editor, display titles

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §5–6
- **Change:** extends §4
- **Summary:** Insert template… and the parameter popup from TemplateData; `displaytitle` as the heading and the disambiguation mark from page properties.
