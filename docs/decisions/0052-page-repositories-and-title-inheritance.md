# 0052. Page repositories and title inheritance

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-09 (A5)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0019](0019-discussions.md), [0038](0038-page-metadata-and-categories.md), [0042](0042-template-expansion-and-parsoid.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0007](0007-actor-identity.md), [0009](0009-keyed-entity-types-and-domain.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0026](0026-sitelinks.md), [0028](0028-tenancy-policy.md), [0039](0039-files-and-media.md), [0041](0041-content-models.md), [0043](0043-lua-modules.md), [0051](0051-page-redirects.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [04](../architecture/04-entities-and-identifiers.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [13](../architecture/13-mirrored-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

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

*Changed by A1, A2, A3.*

*Current text: [13](../architecture/13-mirrored-pages.md) §1.1.*

### 2. A title names a stack

*Current text: [13](../architecture/13-mirrored-pages.md) §1.2.*

### 3. Resolution (amends 0008 §3; extends 0042 §11)

*Current text: [13](../architecture/13-mirrored-pages.md) §1.3.*

### 4. Links continue reading (extends 0008 §8)

*Current text: [13](../architecture/13-mirrored-pages.md) §1.4.*

### 5. The frame: primary, origin and alternates (extends 0010 §2)

*Changed by A4.*

*Current text: [13](../architecture/13-mirrored-pages.md) §1.5.*

### 6. Identity: provider-ranged page IDs, and the API (extends 0015 §2 and 0013 §6; extends 0012 §4–5)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §5.3; [18](../architecture/18-api.md) §2.2, §2.3, §3.1, §3.2.*

### 7. Talk pages, statements, categories and feeds

*Changed by A1.*

*Current text: [13](../architecture/13-mirrored-pages.md) §1.7, §1.8.*

### 8. Storage (extends 0013 §5.6)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.12, §5, §9.7, §12.*

### 9. Permissions and tenancy (extends 0016 §2)

*Current text: [08](../architecture/08-tenants-and-instances.md) §5.3, §10.6; [09](../architecture/09-security-and-moderation.md) §8.8.*

### 10. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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

## Amendment log

### A1. Followed talk pages

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §1, §3
- **Change:** extends §1; amends §7
- **Summary:** A `page-repo` gains `talk`: `link` (as before) or `sync`, under which the repository's talk pages are followed and their sections shown on the local talk page as foreign threads, with replies and new sections sent upstream by people holding a grant.

Replaced text (§7):

> Threads attach to `{kind: page, id: <the ranged page ID>}` ([0019](0019-discussions.md) §2), exactly as they attach to a mirrored entity, and the talk page offers a link to the repository's own talk page for readers who want that discussion.

### A2. `repo.cache_ttl` is per repository, with a per-kind default

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §1
- **Summary:** `repo.cache_ttl` is a per-repository field of the `page-repo` record, as it is of the `file-repo` record ([0039](0039-files-and-media.md) §11), with a per-kind default: 7 days for files, one hour for template source and page bundles. The field was named `cache_ttl` here with a flat default of one hour. (PENDING C20)

Replaced text (§1):

> | `cache_ttl` | How long fetched source and bundles are held, default one hour, as [0042](0042-template-expansion-and-parsoid.md) §11 had it |

### A3. The field is `repo.cache_ttl` here too

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §1
- **Summary:** The repository field is named `repo.cache_ttl` in §1 too, the field A2 (PENDING C20) makes per-repository; one name everywhere, as [0053](0053-mirrored-pages.md) §4 already has it for the L2 row. §1's table called it `cache_ttl`. (PENDING F1)

Replaced text (§1):

> | `cache_ttl` | How long fetched source and bundles are held, default one hour, as [0042](0042-template-expansion-and-parsoid.md) §11 had it |

### A4. Delete on a foreign page hides it

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §5
- **Summary:** A foreign page is hidden as a local page is deleted: the Delete action, held by an administrator (`delete`), writes the `read` ACL of [0053](0053-mirrored-pages.md) §9. §5 lists Delete among the actions offered on a foreign page, with that meaning; Move, Protect and Change content model stay unoffered, Protect… offering create-protection only as before. (PENDING F2)

Replaced text (§5):

> Move, Delete, Protect and Change content model are not offered on a foreign page;

### A5. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [04](../architecture/04-entities-and-identifiers.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [13](../architecture/13-mirrored-pages.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
