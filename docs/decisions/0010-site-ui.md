# 0010. Site UI

- **Status:** Proposed
- **Date:** 2026-09-26
- **Author:** James Hare / Claude Opus
- **Amended by:** [0013 — Postgres as the log store and serving model](0013-postgres-storage.md), [0005 — Crate organization, revision of 2026-09-26](0005-crate-organization.md) (§2 crate names), [0016 — Permissions and access control](0016-permissions-and-access-control.md), [0017 — Entity ID grammar](0017-entity-id-grammar.md), [0018 — Tenants](0018-tenants.md), [0019 — Discussions](0019-discussions.md) (§8 extends §2 with the talk tab and thread pages; settles the discussions open question), [0020 — Change feeds](0020-change-feeds.md) (settles the watchlists open question; §4 extends §11), [0021 — Notifications](0021-notifications.md) (§6 extends §2 and §11; settles the notifications open question), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§9 extends §2, §5 and §7; §6 settles the patrolling open question), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§9 extends §8 and §11: subsidiaries and keys on the account page; operators on contributions), [0026 — Sitelinks are URLs](0026-sitelinks.md) (§9 extends §2: the Sitelinks tab groups by host), [0027 — Preferences, private state and portability](0027-preferences-and-portability.md) (§7 extends §11: Preferences as the home of every key; a Your data section), [0029 — Resolver namespaces](0029-resolver-namespaces.md) (§7 extends §3: resolver keys in the search box), [0030 — Edit filters](0030-edit-filters.md) (§10 extends §2 and §7: filter pages, tag chips, refusal messages), [0031 — Property constraints](0031-property-constraints.md) (§7 extends §2: the property page's Constraints tab and `Special:ConstraintReport`), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§11 extends §2 and §11: `Special:Providers`, the verified chip, a Fediverse section, the talk-page handle; §6 amends the vanish page), [0025 — The instance as an OAuth server](0025-oauth-server.md) (§5 extends §11: Connected applications; `Special:OAuthConsumers` and `Special:PendingSubsidiaries`), [0038 — Page metadata, legacy categories and articles](0038-page-metadata-and-categories.md) (§7 extends §1 and §2: one subject per frame, the Page data tab and the subject links; §3 amends §4: categories as links; §8 extends §3: "Page titled …" beside an ID's "Go to"), [0039 — Files, blob storage and foreign file repositories](0039-files-and-media.md) (§18 extends §2 and §5: the file page; uploads in history), [0040 — Instance prerogatives](0040-instance-prerogatives.md) (§7 extends §5: instance acts in history), [0041 — Content models](0041-content-models.md) (§10 refines §4: the Format selector appears only for text models, and only where the namespace allows more than one), [0042 — Template expansion and the Parsoid renderer](0042-template-expansion-and-parsoid.md) (§7 amends §4: chips after expansion; §15 amends §4: server preview and templates used), [0047 — Special pages](0047-special-pages.md) (§1, §5 and §9 extend §2, §3 and §12: `Special:SpecialPages`, `Special:Search`, the New menu's forms; the registry supersedes §12's address table)
- **Related:** [0000 — Initial proposition](0000-init.md), [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md), [0003 — Statement UI](0003-statement-ui.md), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§13 amends §2), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md), [0007 — Actor identity](0007-actor-identity.md), [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md), [MediaWiki API contract](../api/mediawiki-compat.md)

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

These extend the principles of [0003](0003-statement-ui.md) §1. That ADR's principles still apply: show the exception, not the rule; name the effect, not the mechanism; nothing is lost.

1. **One frame for every page.** An entity, a document page, a user, a job and a special page all get the same header: an identity line, the title, and tabs. A reader learns the frame once.

   > **Extended by [0038](0038-page-metadata-and-categories.md) §7.** A frame's tabs show data only about what its identity line names. A page's statements are never drawn in its item's frame, or the reverse; crossing from one to the other is a link in the identity line.
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

**The global header** holds:

- the site name, linking home;
- one search box (§3);
- links to Recent changes, Jobs and Special pages;
- a **New** menu for creating items, properties and pages;
- the account menu, or **Log in**.

> **Extended by [0047](0047-special-pages.md) §5 and §9.** **Special pages** links to `Special:SpecialPages`, the index of every page `docs/registry/special-pages.toml` marks served, grouped, and filtered by the viewer's rights. The **New** menu opens `Special:NewItem` and `Special:NewProperty` for entities, and the edit form for pages.

**The page header** has three parts:

1. **An identity line.** It shows a chip for the ID or namespace, the kind of page, and provenance where there is any, such as "Foreign item, minted by Wikidata" or "First 14 revisions imported from Librarybase".
2. **The title.**
3. **The page's tabs:**

| Page kind | Tabs |
|---|---|
| Entity view | Statements, Identifiers, Sitelinks, Labels in other languages, History, Links here |
| Document page | Read, Edit, History, Links here |
| User | User page, Contributions, Jobs, Links here |
| Job | Summary, Changes, Rejected |
| Special page | None |

- **Entities have no Edit tab.** They are edited in place ([0003](0003-statement-ui.md) §8).
- **Less common actions** go in the page's overflow menu. For a document page these are Move, Change content model and Delete.
- **Links here** is served by the links projection ([0008](0008-namespaces-and-document-pages.md) §10).

> **Extended by [0038](0038-page-metadata-and-categories.md) §7.** Document pages and threads gain a **Page data** tab: the page's own statements in the statement UI, with projected statements read-only. A page paired with an item through a sitelink shows "About: {label} ({ID})" in its identity line, and the item shows "Article: {title}".

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

> **Extended by [0039](0039-files-and-media.md) §18.** A file page's frame shows the file, its version history, its usage and metadata, and, for a taken-down version, the operator's notice in place of the file. Histories (§5) interleave uploads with text and statement revisions.

### 3. Search

- **One box covers everything.** It searches entity labels, descriptions and aliases across every provider, domain keys ([0009](0009-keyed-entity-types-and-domain.md)), and page titles.
- **Suggestions are grouped by kind:** items and properties, foreign entities by type (for example "Sources and works"), domains, and pages. Each suggestion shows its label, its description, and its ID chip.
- **An ID or domain key jumps straight to its page.** Input that parses as an ID or domain key gets a "Go to" option as the first suggestion, and Enter follows it. Parsing and resolution use the title resolver ([0008](0008-namespaces-and-document-pages.md) §3), so a non-canonical cluster member lands on its canonical entity ([0004](0004-identity-clusters-and-equivalence.md) §4).

  > **Extended by [0038](0038-page-metadata-and-categories.md) §8.** When a main-namespace page has the same title as the ID, it is offered as the next suggestion, "Page titled Q42".
- **The full results page** also searches page text. Whether one index serves both labels and text is still open in [0008](0008-namespaces-and-document-pages.md).

> **Extended by [0047](0047-special-pages.md) §9.** The full results page is `Special:Search`, with MediaWiki's parameters.

### 4. Document pages

**Read**

- Renders the page's content model ([0008](0008-namespaces-and-document-pages.md) §5, §8).
- **Syntax outside the wikitext subset stays visible:**
  - A template call renders as a "Template not rendered" chip showing the call.
  - Category links are listed at the foot of the page as plain text, under "Categories from the source wiki".

  > **Amended by [0038](0038-page-metadata-and-categories.md) §3.** Categories are listed as links to their category pages, with hidden categories collapsed.
- **Links to entities** render with the entity's label.
- **An "About this page" panel** shows the last edit, the revision count by origin (imported, bot, local), where the page came from, the content model, and the backlink count.

**Edit**

- **The source and a live preview sit side by side.** The preview runs the server's own renderer compiled to WebAssembly ([0008](0008-namespaces-and-document-pages.md) §11).
- **A Format selector** changes the content model (`action=changecontentmodel`).
- **Link autocomplete** opens after `[[` and suggests titles and entities in any namespace, through the title resolver. For an entity, it inserts the link and the entity's label.
- **Saving** asks for a summary and a minor-edit flag, and sends the base offset ([0008](0008-namespaces-and-document-pages.md) §4). On a conflict, the editor keeps the user's text and marks only the lines that clash.

> **Refined by [0041](0041-content-models.md) §10.** The Format selector and the model shown beside the title appear only for text models, and the selector only where the namespace allows more than one model.

> **Amended by [0042](0042-template-expansion-and-parsoid.md) §7 and §15.** With expansion on, templates are expanded before rendering and only constructs outside the subset render as chips; a missing template is a red link. Pages that need expansion preview through `action=parse` on the server. The About panel gains "Templates used" and the limit report; a Template page gains "Pages that use this template".

### 5. History

#### 5.1 What a history covers

The history of an entity page lists every record whose key is:

- the entity itself; or
- any member of its identity cluster ([0004](0004-identity-clusters-and-equivalence.md)), with a chip showing which member.

It covers records in every source partition:

- local change sets;
- mirror records;
- log events such as retention changes, `convert`, redirects and `erase`.

A document page's history lists its records in the `pages` partition.

#### 5.2 Rows

| Row | Shown as |
|---|---|
| Local edit | One row: time, Local chip, parsed summary, tags, size change, and actor. It links to its diff. |
| Mirror sync | One row per observed state: time, provider chip, "Synced from Wikidata", the upstream revision, the job, and a one-line account of what changed. It links to its diff. |
| Upstream edit | Folded inside the sync that brought it in (§5.5), with the upstream actor linked to their upstream IRI ([0007](0007-actor-identity.md) §2). |
| Erased record | A gap row showing only its time and source: "An edit was erased." Its reason class is shown only if [0006](0006-log-integrity-and-erasure.md) makes it public. |
| Hidden actor | The row, with "(username hidden)" in place of the name. |
| Log event | A Log chip and a sentence describing the effect. It has no diff. |

**Rows are identified by time and source.** Revision numbers are not shown until the question of global revision IDs is settled ([0008](0008-namespaces-and-document-pages.md), open questions).

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

> **Extended by [0040](0040-instance-prerogatives.md) §7.** A row for an instance act shows the instance operator as its actor, an **Instance action** label, the public reason and a link to `Special:InstanceAction/{partition}/{offset}` at the farm base.

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

**Row kinds:**

| Kind | One row per |
|---|---|
| Item and property edits | Local change set |
| Page edits | Page record |
| Jobs | Local job, however many entities it touched |
| Log actions | Log event: links, erasures, retention, account creation |
| Mirror syncs | Sync job run, never per entity |

- **Filters** toggle each kind. Mirror syncs are off by default. While they are off, a strip above the list summarises running and recent syncs, and offers to show them.
- **"Group by page"** collapses consecutive edits to the same page into one row, with a count.
- **The time window is configurable.** The default follows MediaWiki's `$wgRCMaxAge`, 90 days.

**The Action API stays per revision.** `list=recentchanges` and `list=usercontribs` return local changes one revision at a time, as MediaWiki clients expect.

- **Changes made by a job** carry the bot flag and a `job:{id}` change tag, so `rcshow=!bot` hides them.
- **Mirror records are not in `list=recentchanges`.** Each mirror job run appears in `list=logevents` under a new log type, `job`.

### 8. Contributions

- **Contributions are per account** ([0007](0007-actor-identity.md), Consequences).
- **The header** shows the account's linked accounts, as links to their upstream pages. Only links the holder created are shown.
- **Jobs run for the user** appear as single rows, and can be filtered out.
- **A bot account** names its operator.
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

**`Special:UserLogin`**

- Offers one button for each issuer that the issuer registry allows as an identity provider ([0007](0007-actor-identity.md) §1). It says what the instance learns: which account the user holds, and nothing else.
- Explains temporary accounts, and says that IP addresses are never shown or published ([0007](0007-actor-identity.md) §3).
- States that nobody else can see which service a user logs in with.

**The first login** continues to `Special:CreateAccount`, where the user chooses a public name.

> **Amended 2026-09-27: name rules.** A local account name follows MediaWiki's username grammar exactly, so that imported attributions, CentralAuth names and Pywikibot round-trip: Unicode letters, digits and spaces; none of `@ # : / < > [ ] | { }`, control characters or leading, trailing or doubled spaces; at most 255 bytes; normalized to NFC with underscores read as spaces and the first letter uppercased. **Uniqueness is case-insensitive**, as the `actor_local_name` index of [0013](0013-postgres-storage.md) §5.4 already has it. **A name is never reused.** Every name a local account has ever held, whether renamed away or vanished from, stays reserved as a *name tombstone*, so that `[[User:OldName]]` in old page text or a post can never come to name a different person; the only way to have a name is to have always had it. Because a vanished account's names are erased from the log ([0007](0007-actor-identity.md) §4), the tombstone is not a projection: it is a keyed hash of the lower-cased name in `private.name_tombstone` ([0013](0013-postgres-storage.md) §5.6), written when a name is released, holding no actor key and no plaintext, and revealing nothing but that a candidate name is reserved, which the "name taken" check reveals anyway. `Special:CreateAccount` and `renameuser` refuse a tombstoned name with `ts-name-reserved`. The subsidiary conventions of [0024](0024-subsidiary-accounts.md) §2 and [0025](0025-oauth-server.md) §3 apply on top of this grammar, and the farm name registry of [0028](0028-tenancy-policy.md) §2 is this rule applied across tenants, tombstones included.

- **The name field starts empty.** The identity provider's username may be offered as a button, labelled with where it came from. Next to it, the page says that using the same name makes the two accounts easy to connect ([0007](0007-actor-identity.md), Consequences).
- **An optional, unchecked box** offers to link the user's account on the identity provider's wiki, for example Wikidata. It says the link is public. If it is checked, the link is created by the flow in [0007](0007-actor-identity.md) §7. The authentication that just took place serves as the fresh proof of control, because it happened in the same request flow.

### 11. Account settings

`Special:Account` is visible only to its holder. It has five sections:

| Section | Label | Contents |
|---|---|---|
| Name | Public | The current name and **Rename…**. The page warns that user pages move and that links using the old name stop working ([0008](0008-namespaces-and-document-pages.md) §6). |
| Sign-in methods | Private | Each binding, with when it was added and last used. **Remove** is disabled for the last binding, and says why. **Add** lists the issuers allowed as identity providers. |
| Linked accounts | Public | Each link, with its date and **Unlink…**. **Link another account** requires logging in to the provider again. The page warns that unlinking erases the link from history, and that copies taken earlier may keep it ([0007](0007-actor-identity.md) §7). |
| Preferences | — | Interface and label language, and the defaults for the upstream-edits fold (§5.5) and the mirror-sync filter (§7) |
| Leave | — | **Vanish this account…**, with a plain account of what vanishing does ([0007](0007-actor-identity.md) §4), including, since [0022](0022-federation.md) §6, that copies of posts delivered to other servers may persist |

`Special:Preferences` redirects to the Preferences section.

### 12. Addresses

MediaWiki's URL forms are kept so that links and tools keep working.

| Page | Address |
|---|---|
| Any page | `/wiki/{title}` and `index.php?title={title}` |
| History | `index.php?title={title}&action=history` |
| Edit (document pages) | `index.php?title={title}&action=edit` |
| Diff | `Special:Diff/…` and `index.php?diff=…&oldid=…`. The ID form depends on global revision IDs (open questions). |
| Links here | `Special:WhatLinksHere/{title}` |
| Recent changes | `Special:RecentChanges` |
| Contributions | `Special:Contributions/{name}` |
| Jobs | `Special:Jobs`, `Special:Jobs/{id}` (new) |
| Log in, first login | `Special:UserLogin`, `Special:CreateAccount` |
| Account | `Special:Account` (new) |
| Providers, consumers, pending subsidiaries | `Special:Providers` ([0022](0022-federation.md) §11), `Special:OAuthConsumers`, `Special:PendingSubsidiaries` ([0025](0025-oauth-server.md) §5) |
| Log | `Special:Log` |

> **Amended by [0047](0047-special-pages.md) §1.** `docs/registry/special-pages.toml` supersedes this table: it lists every special page with its aliases, scope (§3 there) and status. The rows above are entries in it.

### 13. Implementation

- **A new activity projection** lives in the surfaces layer ([0005](0005-crate-organization.md) §1), in a new crate, `triplespace-activity`. It is a time-ordered index with these inputs:
  - every record in the local, `pages` and `actors` partitions;
  - one entry for each job;
  - for each mirror partition, a pointer from each entity to its latest synced state.

  It serves recent changes, contributions, histories, `list=recentchanges`, `list=usercontribs` and `list=logevents`. Like every projection, it can be rebuilt from the log. It grows with local activity and the number of jobs, not with the size of the mirrors.
- **The upstream history fetch** is a server component. It reads the provider registry's API endpoint and reuses the adapter's parsing of upstream revisions. Its cache is operational state, not log data.
- **The frontend extends the `ui/` prototype of [0003](0003-statement-ui.md) §10.** Statement diffs reuse `scatter-wikibase-shape`, and the page editor's preview reuses `scatter-wikitext` and `scatter-markdown`, all built for `wasm32-unknown-unknown`.

## Consequences

- **History and recent changes stay usable at Wikidata scale.** A sync of millions of entities is one row.
- **Users trained on MediaWiki find the usual places,** with three deliberate differences:
  - Recent changes has no per-entity mirror rows.
  - Compare is limited to one source.
  - Revision numbers are hidden for now.
- **The UI and the Action API differ on purpose.** The UI groups changes by job, and `list=recentchanges` does not. Patrol tools see job changes as bot edits carrying a job tag.
- **Live upstream fetches depend on the provider.** They are subject to its rate limits and availability, and the fold has to handle failure gracefully. Retaining an entity removes that dependency for it.
- **"Private" becomes a promise the UI makes.** Anything labelled Private must stay out of every projection, feed and export ([0007](0007-actor-identity.md) §8). A test should check that each label matches the export policy of the graph behind it.
- **The activity projection is new work.** Recent changes, histories and contributions all depend on it.
- **The set of chips needs curating.** Each new kind of row or badge competes with the ones in §2.

## Open questions

- ~~**Watchlists and notifications.** [0000](0000-init.md) lists watchlists among what Triplespace must rebuild or delegate. It is not settled whether a watch on an entity also covers its mirror syncs.~~ *Watchlists settled by [0020](0020-change-feeds.md) §2–3 (a watch covers syncs by default); notifications by [0021](0021-notifications.md).*
- ~~**Patrolling.** Whether local edits get patrol status, and how that interacts with job rows.~~ *Settled by [0023](0023-moderation.md) §6: MediaWiki-shaped; a job is patrolled as one row.*
- ~~**Global revision IDs** ([0008](0008-namespaces-and-document-pages.md)). They decide how diff URLs look, and whether rows show revision numbers.~~ *Settled by [0013](0013-postgres-storage.md) §6 and [0015](0015-record-format-and-partition-registry.md) §2; `oldid=N` and `Special:Diff/N` work for local and mirror revisions alike, and rows may show revision numbers.*
- ~~**Contributions of foreign actors.** Whether `Special:Contributions` should list, for a Wikidata account, the upstream edits the instance has observed or backfilled.~~ *Settled by [0018](0018-tenants.md) §8: what the instance holds, and no more; the page says so and never fetches upstream.*
- ~~**Visibility of erasure reasons** ([0006](0006-log-integrity-and-erasure.md), open questions).~~ *Settled by [0016](0016-permissions-and-access-control.md) §6: the gap row is public; the reason class is for `ts-viewerasures`.*
- **Rendering.** Whether pages are rendered on the server, in a client app, or both. Reading pages should work without JavaScript.
- **Mobile layouts.** None of the pages has a narrow-screen layout yet.
- ~~**Permissions.** Who may revert a job, see hidden usernames, erase, or run `retain` from the history page.~~ *Settled by [0016](0016-permissions-and-access-control.md) §5: `ts-revertjob`, `deletedhistory`, `ts-erase`, `ts-retain`.*
- **Upstream fetch budget.** ~~Cache lifetime, per-user and per-instance rate limits,~~ and whether any provider other than Wikidata supports the fold. *Rate limits are the `upstream` class of [0024](0024-subsidiary-accounts.md) §5; the cache lifetime is a [0014](0014-caches-and-search.md) tuning value. Which providers support the fold is a registry flag per provider (`revision_ids`, [0015](0015-record-format-and-partition-registry.md) §5) and is settled per provider as each adapter is written.*
- ~~**Rules for local account names.** Which characters are allowed, how names are normalized, and how names that collide with names held by a vanished account are handled. [0024](0024-subsidiary-accounts.md) §2, [0025](0025-oauth-server.md) and [0028](0028-tenancy-policy.md) §2 all defer to this.~~ *Settled 2026-09-27 (§10 amendment): MediaWiki's grammar verbatim, case-insensitive uniqueness, and no name ever reused, by name tombstones in the actor projection.*
- ~~**Discussions.** Talk namespaces are reserved ([0008](0008-namespaces-and-document-pages.md) §2), and their UI comes with that ADR.~~ *Settled by [0019](0019-discussions.md) §8.*

## References

- [Site UI design canvas](https://claude.ai/artifact/XNbmnySA1RViHpnH2ecFQY) (private until shared)
- [Statement UI design canvas](https://claude.ai/artifact/69M2T2HYrkafzearFUoJ4X) (private until shared)
- [Help:Recent changes](https://www.mediawiki.org/wiki/Help:Recent_changes), [Help:Page history](https://www.mediawiki.org/wiki/Help:Page_history), [Help:Diff](https://www.mediawiki.org/wiki/Help:Diff)
- [Manual:$wgRCMaxAge](https://www.mediawiki.org/wiki/Manual:$wgRCMaxAge)
- [Temporary accounts](https://www.mediawiki.org/wiki/Trust_and_Safety_Product/Temporary_Accounts)
