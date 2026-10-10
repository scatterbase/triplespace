# 0010. Site UI

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-09 (A40)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md)
- **Uses:** [0000](0000-init.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [19](../architecture/19-site-ui.md)

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

*Current text: [19](../architecture/19-site-ui.md) §1.1.*

### 2. The frame

*Changed by A5, A9, A11, A12, A13, A16, A20, A22, A23, A27, A29, A30, A33.*

*Current text: [19](../architecture/19-site-ui.md) §1.2, §1.3, §1.4, §1.5.*

### 3. Search

*Changed by A4, A7, A18, A22, A27, A37.*

*Current text: [19](../architecture/19-site-ui.md) §2.1.*

### 4. Document pages

*Changed by A21, A22, A25, A26, A30, A31, A34, A39.*

*Current text: [19](../architecture/19-site-ui.md) §3.1, §3.2.*

### 5. History

*Changed by A38.*

*Current text: [19](../architecture/19-site-ui.md) §3.3.*

#### 5.1 What a history covers

*Changed by A23, A38.*

*Current text: [19](../architecture/19-site-ui.md) §3.3.*

#### 5.2 Rows

*Changed by A3, A5, A13, A24.*

*Current text: [19](../architecture/19-site-ui.md) §3.4.*

#### 5.3 The source switch

*Current text: [19](../architecture/19-site-ui.md) §3.5.*

#### 5.4 Mirror states the instance did not keep

*Changed by A38.*

*Current text: [19](../architecture/19-site-ui.md) §3.5.*

#### 5.5 Upstream edits, fetched live

*Current text: [19](../architecture/19-site-ui.md) §3.6.*

#### 5.6 Comparing

*Current text: [19](../architecture/19-site-ui.md) §3.7.*

#### 5.7 Narrower histories

*Current text: [19](../architecture/19-site-ui.md) §3.7.*

### 6. Diffs

*Current text: [19](../architecture/19-site-ui.md) §3.8.*

### 7. Recent changes

*Changed by A10, A13, A19.*

*Current text: [19](../architecture/19-site-ui.md) §3.9.*

### 8. Contributions

*Changed by A8, A14, A40.*

*Current text: [19](../architecture/19-site-ui.md) §2.5.*

### 9. Jobs

*Current text: [19](../architecture/19-site-ui.md) §3.10.*

### 10. Logging in

*Changed by A6.*

*Current text: [19](../architecture/19-site-ui.md) §2.3.*

### 11. Account settings

*Changed by A10, A11, A12, A14, A15, A17.*

*Current text: [19](../architecture/19-site-ui.md) §2.4.*

### 12. Addresses

*Changed by A3, A5, A10, A11, A19, A20, A27, A32.*

*Current text: [19](../architecture/19-site-ui.md) §2.2.*

### 13. Implementation

*Changed by A1, A2, A21, A32, A35, A38.*

*Current text: [19](../architecture/19-site-ui.md) §3.6, §5.2, §5.8.*

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

### A32. The web tier and `action=render`

- **Date:** 2026-10-03
- **Source:** [0057](0057-web-tier.md) §1, §2, §8, §14
- **Change:** extends §12, §13
- **Summary:** `index.php?title={title}&action=render` returns a page's content without the frame, as in MediaWiki, and `region` selects part of it; the site's components fetch regions this way after a save. `action=purge` is MediaWiki's confirmation form. `triplespace-ui` may run in the server or in a separate, stateless web tier, and in either reaches the instance only through the public API.

### A33. One tabs table

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §2
- **Summary:** §2's tabs table is the one tabs table for the site: the thread row gains Page data, and the document-page row keeps Talk; [0038](0038-page-metadata-and-categories.md) §7 refers to this table instead of carrying its own. (PENDING F15)

Replaced text (§2):

> | Thread, talk page | The posts as a tree, or the list of threads, with History ([0019](0019-discussions.md) §8) |

### A34. The About panel on entity pages

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §4
- **Summary:** The entity page has the "About this page" panel of §4 beside its "Where this comes from" panel ([0003](0003-statement-ui.md) §6); the scope count ([0060](0060-scopes.md) §8) and the proposal count ([0067](0067-proposals.md) §7) sit in it. (PENDING F16)

### A35. The prototype is `triplespace-ui`

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** supersedes §13
- **Summary:** The `ui/` prototype of [0003](0003-statement-ui.md) §10 is superseded: the prototype is `triplespace-ui` with `scatter-wasm`, server-rendered Rust ([0034](0034-frontend-stack.md) §3, §13). The fixture list and audit step of 0003 §10 stay; "later Rust implementation" and "lives in `ui/`" go, and §13's frontend bullet no longer extends a `ui/` prototype. (PENDING F17)

Replaced text (§13):

> - **The frontend extends the `ui/` prototype of [0003](0003-statement-ui.md) §10.** Statement diffs reuse `scatter-wikibase-shape`, and the page editor's preview reuses `scatter-wikitext` and the markdown renderer of `scatter-pages`, all built for `wasm32-unknown-unknown` ([0005](0005-crate-organization.md) §2).

### A36. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§13
- **Summary:** The Decision's current text now lives in the architecture chapters [19](../architecture/19-site-ui.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A37. The search box accepts any cluster member and lands in the local form

- **Date:** 2026-10-09
- **Source:** [0082](0082-source-form-and-the-shared-view.md) §2, §3
- **Change:** amends §3
- **Summary:** The title resolver the search box uses accepts any member of an identity cluster, so `WDQ42` and the local `Q9` it is clustered with land on the same entity page, shown in the site's local form; a non-canonical member no longer "lands on its canonical entity", since the page is served under whichever ID was asked for with values rewritten to the local preference. Search results are one per cluster, the member the local preference selects. 0082's table named §9 (Jobs); the fold is at [19](../architecture/19-site-ui.md) §2.1, whose provenance is §3 here, and the table and the ledger row were corrected. (REVIEW G4, G5)

Replaced text ([19](../architecture/19-site-ui.md) §2.1, as it stood):

> - **An ID or domain key jumps straight to its page.** Input that parses as an ID, a keyed ID or a bare key of a keyed type gets a "Go to" option as the first suggestion, and Enter follows it ([0017](../decisions/0017-entity-id-grammar.md) §1, §4). So does a resolver key, such as `DOI:10.1000/xyz`, which lands on the item it resolves to or on a disambiguation page in the site frame. Parsing and resolution use the title resolver ([0008](../decisions/0008-namespaces-and-document-pages.md) §3; [10](../architecture/10-pages-and-content-models.md)), so a non-canonical cluster member lands on its canonical entity ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §4). When a main-namespace page has the same title as the ID, it is offered as the next suggestion, "Page titled Q42" ([0038](../decisions/0038-page-metadata-and-categories.md) §8).

### A38. An entity's history has one backing, `view.entity_history`

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §5.4, §13; extends §5, §5.1
- **Summary:** The history of an entity is read from `view.entity_history (tenant, canonical_id, time, partition, offset, kind, member)`, which composition writes: one row per record in any source partition keyed to the entity or a member of its cluster, and one per upstream log event keyed to a member, each with its kind and the member. The page is one keyset query over that index, served by `GET /entity/{id}/history`; nothing is merged from `log.record` or `view.activity` at request time, and upstream log events are rows of it, not of `view.activity`. Under `full`, or for a retained entity, every observed state and every backfilled upstream revision is a row of it too, so "read from the log" is withdrawn; document page histories, recent changes and contributions stay on `view.activity`. The default mirrored provider log is `delete/*` and `protect/*`, the backfill covers held entities only, and `create/create` is derived from the first upstream revision ([0011](0011-logs.md) §3). The ledger named §3 (Search); the folds are at [19](../architecture/19-site-ui.md) §3.3, §3.5 and §5.2, whose provenance is §5, §5.1, §5.4 and §13 here, and the ledger row was corrected. (REVIEW G42)

Replaced text ([19](../architecture/19-site-ui.md) §3.5, as it stood):

> Under `full`, or for a retained entity, every observed state is a row, and backfilled upstream revisions are read from the log.

Replaced text ([19](../architecture/19-site-ui.md) §5.2, as it stood):

> `triplespace-ui` renders pages on the server, runs in the server or in the stateless web tier, and reaches the instance only through the public API ([0057](../decisions/0057-web-tier.md) §1–2; [20](../architecture/20-web-tier.md)). Statement diffs reuse `scatter-wikibase-shape`, and the page editor's preview reuses `scatter-wikitext` and the markdown renderer of `scatter-pages`, all built for `wasm32-unknown-unknown` ([0005](../decisions/0005-crate-organization.md) §2; [22](../architecture/22-crates-and-stack.md)). Recent changes, contributions and histories are served by the activity projection, `view.activity` in `triplespace-projections`, which grows with local activity and the number of jobs, not with the size of the mirrors ([0010](../decisions/0010-site-ui.md) §13; its inputs and tables are [03](../architecture/03-storage-caches-and-search.md) and [16](../architecture/16-logs-feeds-and-notifications.md)).

### A39. The About panel is its own response; counts are counters

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §4
- **Summary:** The "About this page" panel is its own API response with its own short `max-age`, independent of the page's or entity's `ETag`, because its numbers change when referrers, scopes and proposals change and the subject does not; its counts (`ref_count`, the revision counts) are counter columns maintained in batches by the projections, never counted on view, and a count above `ui.count_threshold` is shown as "10,000+". The page budget this belongs to, four API calls to first byte met by the bulk-labels route and the `GET /entity/{id}/page` bundle, is [0012](0012-api-requirements.md) §5 and [0057](0057-web-tier.md) §2. The ledger named §2 and §9; the fold is at [19](../architecture/19-site-ui.md) §3.1, whose provenance is §4 here, and the ledger row was corrected. (REVIEW G41)

### A40. A foreign actor's contributions are what `view.activity` holds

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §8
- **Summary:** `Special:Contributions/{issuer}:{id}` for a foreign actor lists what `view.activity` holds for that actor: page revisions imported with that attribution and the rows of this tenant's log that name the actor. Upstream revision records in `log/{provider}` are not indexed by actor, so they are not listed, and the page says so ("This instance does not index Wikidata's revisions by user"). For an actor of another tenant on the same instance the view is complete, since that tenant's `view.activity` indexes every record; for a Wikidata account it is only what reached this tenant's activity rows. The ledger names [0047](0047-special-pages.md) §2 and §4; the fold is at [19](../architecture/19-site-ui.md) §2.5, whose provenance is §8 here, and the ledger row was extended. (REVIEW G50)

Replaced text ([19](../architecture/19-site-ui.md) §2.5, as it stood):

> | `Special:Contributions/{issuer}:{id}` for a foreign actor | **What the instance holds:** upstream revision records keyed to that actor in `log/{provider}` ([0015](../decisions/0015-record-format-and-partition-registry.md) §4), and page revisions imported with that attribution. The page says whose user this is, links to their page on the issuer's site, and does not fetch upstream, because per-actor fetching is unbounded. |

> For an actor of another tenant on the same instance the third view is complete, since the instance holds every record. For a Wikidata account it is the sparse subset the instance has observed or backfilled, and says so. `/actor/{key}/contributions` serves the same three cases ([18](../architecture/18-api.md)).
