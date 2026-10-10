# 0054. Forking a mirrored page

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-09 (A5)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0024](0024-subsidiary-accounts.md), [0030](0030-edit-filters.md), [0033](0033-backend-stack.md), [0039](0039-files-and-media.md), [0047](0047-special-pages.md)
- **Uses:** [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0023](0023-moderation.md), [0035](0035-adopting-a-wikibase.md), [0042](0042-template-expansion-and-parsoid.md), [0043](0043-lua-modules.md), [0049](0049-boards.md), [0051](0051-page-redirects.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [09](../architecture/09-security-and-moderation.md), [13](../architecture/13-mirrored-pages.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

## Context

A mirrored page ([0053](0053-mirrored-pages.md)) is read, never written. The second stage of James's plan is what happens when someone presses Edit on one: the page becomes the tenant's own. MDWiki's articles began this way, as copies of Wikipedia's medical articles that its editors then revised, and they stopped following Wikipedia the moment they were copied.

Triplespace has done this twice for entities. **Retention** keeps a mirrored entity when upstream deletes it and backfills its whole upstream history ([0002](0002-source-graphs-and-mass-ingest.md) §5, [0015](0015-record-format-and-partition-registry.md) §4); **conversion** makes a foreign entity local ([0002](0002-source-graphs-and-mass-ingest.md) §6). A fork is both at once, for a page: the page's history comes here, and the page becomes local. Two more decisions fix the shape. [0008](0008-namespaces-and-document-pages.md) §9 imports pages with full history and original attribution, from an XML export; and [0035](0035-adopting-a-wikibase.md) §7 settled that a page's current state can be written first and its history backfilled afterwards as upstream-revision records, because nothing can be appended into a partition behind a page's newest record.

Two sources of history exist. RevisionChest ([0033](0033-backend-stack.md) §10) turns a wiki's history dumps into `.mwrev.zst` files with an index, and its `sync` keeps them current from recent changes; the Internet Archive runs it over Wikipedia. The repository's own API gives any revision one request at a time. A fork needs both: the store for the bulk, the API for what the store does not yet have.

A page's talk page comes with it, and here the two systems differ most: a Wikipedia talk page is a wikitext file of signed, indented sections and archive subpages; a Triplespace talk page is threads ([0019](0019-discussions.md)). 0019 Q5 asked how such a page is imported, and suggested splitting it by heading.

### Direction

James's direction, from the design discussion of 2026-10-01:

- **Forking occurs when someone hits the edit button and saves a local copy.** They should be advised that saving will result in a fork, and that updates from the remote wiki will not be automatically synced.
- **The revisions are seeded from RevisionChest,** including the revision history of that page, any embedded templates and modules, stylesheets, and so on.
- **Files are not transferred by default,** but this can be requested in a separate step.
- **The talk page and its history are transferred as well,** converted into a talk page board, with each section converted into a thread marked as closed, and the frontmatter on top of the talk page treated as such.

## Decision

### 1. A fork is a local page whose first record names its origin (extends 0008 §4 and 0011 §6.1)

*Changed by A2.*

*Current text: [13](../architecture/13-mirrored-pages.md) §3.1; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 2. Forking on save (extends 0010 §4; extends 0012 §4)

*Current text: [13](../architecture/13-mirrored-pages.md) §3.2, §3.3.*

### 3. History is seeded afterwards, from a RevisionChest store or the API (extends 0015 §4; extends 0035 §7; amends 0033 §10)

*Changed by A5.*

*Current text: [13](../architecture/13-mirrored-pages.md) §1.6, §3.4, §3.5.*

### 4. Dependencies: templates, modules and stylesheets (uses 0042 §11 and §13, 0043 §2, 0055 §2)

*Changed by A5.*

*Current text: [13](../architecture/13-mirrored-pages.md) §3.6.*

### 5. The talk page: sections become closed threads (amends 0019 §3, §4, §5, §6; settles 0019 Q5; extends 0008 §9)

*Changed by A2, A5.*

*Current text: [13](../architecture/13-mirrored-pages.md) §5.11, §5.12, §5.13.*

### 6. What a fork knows about upstream (extends 0010 §2 and §4; uses 0001 §1)

*Changed by A1.*

*Current text: [13](../architecture/13-mirrored-pages.md) §3.7.*

### 7. Files, on request (extends 0039 §14)

*Current text: [13](../architecture/13-mirrored-pages.md) §3.8.*

### 8. The job, permissions and limits (extends 0016 §2; extends 0024 §5)

*Changed by A3.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §6.2; [09](../architecture/09-security-and-moderation.md) §8.8.*

### 9. Deleting, moving and re-forking

*Changed by A1.*

*Current text: [13](../architecture/13-mirrored-pages.md) §3.9, §4.5.*

### 10. Storage (extends 0013 §5.6)

*Changed by A5.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.12, §5.*

### 11. API (extends 0012 §4 and §5)

*Current text: [18](../architecture/18-api.md) §2.3, §3.2.*

### 12. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **History first, synchronously,** as [0008](0008-namespaces-and-document-pages.md) §9 imports: fetch every revision, append them in order, then the editor's change. Rejected: a save that waits minutes for a long history, holds the editor's text in limbo, and fails whole on a transient upstream error is not "hit edit and save". Shape (b) was already settled for adoption and costs nothing new.
- **A fork as a local layer over the foreign page,** local text keyed by the ranged ID, as local statements layer over a foreign entity. Rejected: the direction is a copy that stops following upstream, with its own history; a layer would be the opposite, and could not carry the seeded revisions.
- **Forking the dependencies lazily,** when a template is first edited locally. Rejected as the default: the direction is that the templates and modules come with the fork, so that the fork renders as it was written; `inherit` is offered for tenants that want the other behaviour.
- **One post per signature** when splitting a talk page, as DiscussionTools would parse it. Rejected: signature parsing is heuristic, Wikipedia's indentation conventions are inconsistent, and a thread per section with the section intact loses nothing and misattributes nothing. Finer splitting can come later over the same records ([0019](0019-discussions.md) Q4).
- **Converting talk sections to markdown.** Rejected: lossy, and the banners and templates on a talk page are its content; wikitext posts render them through the stack.
- **A `Board` per imported talk page,** reading the direction's "talk page board" literally. Rejected: a board is a talk page with no subject ([0049](0049-boards.md) §1), and these threads have one, the fork. A village pump imported through `Special:Import` does land on a board (§5).

## Consequences

- **Forking is an edit, not an import.** Anyone who can edit can fork, the history and attribution come along, and the licence's attribution requirement is met by the history itself.
- **The save is immediate; the history arrives behind it.** A fork is usable the moment it exists, and its history fills in over minutes, from a RevisionChest store where one exists.
- **A fork stops following upstream, and knows it.** The About panel counts upstream's newer revisions and the diff shows them; applying them is an editor's decision.
- **Templates come along, once.** The first fork of a citation-heavy article may copy a thousand pages; the next copies none. An incomplete copy still renders, through inheritance.
- **Talk pages survive the crossing as threads.** Old discussion is closed, dated as it was, searchable, and rendered with its banners; nothing is lost, and nothing pretends to be a post by someone who did not write it here.
- **Posts can be wikitext.** One payload field, written only by imports, so every rendering path for posts now has two models to handle.
- **The `log` partition grows with forks.** Twenty thousand revisions with text for one article is tens of megabytes; `fork.max_revisions` and metadata-only seeding bound it.
- **Two new `site` settings families** (`fork.*`) and one rate class.

## Open questions

- **Q1.** ~~**Merging upstream changes.** A three-way merge of upstream's diff since the fork onto the fork's current text, as a guided edit; and whether a tenant may choose to re-follow upstream for a fork nobody has changed.~~ *Settled by [0068](0068-merging-with-upstream.md) §2 and §5: `Special:MergeUpstream` and `fork.refollow`.*
- **Q2. Forking into another namespace.** Whether a repository's page may be forked under a different local title, which the stack's title identity does not allow today.
- **Q3. Partial seeding by date.** Whether `fork.max_revisions` should be a span of years rather than a count, which is what readers of a history usually want.
- **Q4. Signature-level attribution.** Whether, where a section's signatures parse cleanly, the import should attribute the thread's opening post to the first signer under the repository's issuer, as 0019 Q5 suggested, instead of to the forker.
- **Q5. Forking across tenants.** A `tenant` repository's page is forked by the same job, reading the source tenant's records directly; whether the source tenant should be told, or its page linked back.
- **Q6. Images in the fork's history.** Old revisions reference files that may no longer exist upstream; whether the file job should offer to fetch them from the repository's deleted-file archive where it is readable.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §12 | extends | 0005 A53 |
| [0008](0008-namespaces-and-document-pages.md) §4, §9 | §1, §5 | extends | 0008 A23 |
| [0010](0010-site-ui.md) §2, §4 | §2, §6 | extends | 0010 A30 |
| [0011](0011-logs.md) §6.1 | §1 | extends | 0011 A18 |
| [0012](0012-api-requirements.md) §4, §5 | §2, §11 | extends | 0012 A33 |
| [0013](0013-postgres-storage.md) §5.6 | §10 | extends | 0013 A24 |
| [0015](0015-record-format-and-partition-registry.md) §4 | §3 | extends | 0015 A25 |
| [0016](0016-permissions-and-access-control.md) §2 | §8 | extends | 0016 A20 |
| [0019](0019-discussions.md) §3, §4, §5, §6 | §5 | amends | 0019 A13 |
| [0019](0019-discussions.md) Q5 | §5 | settles | 0019 Q5 |
| [0024](0024-subsidiary-accounts.md) §5 | §8 | extends | 0024 A9 |
| [0030](0030-edit-filters.md) §2 | §2 | extends | 0030 A9 |
| [0033](0033-backend-stack.md) §10 | §3 | amends | 0033 A7 |
| [0039](0039-files-and-media.md) §14 | §7 | extends | 0039 A7 |
| [0047](0047-special-pages.md) §9 | §2 | extends | 0047 A4 |

## References

- [RevisionChest](https://github.com/internetarchive/RevisionChest): `build` from XML history dumps, `sync` from recent changes, the `.mwrev.zst` format and its index
- [API:Revisions](https://www.mediawiki.org/wiki/API:Revisions) and [API:Edit](https://www.mediawiki.org/wiki/API:Edit)
- [Help:Import](https://www.mediawiki.org/wiki/Help:Import) and [Help:Export](https://www.mediawiki.org/wiki/Help:Export), the `interwiki` import and its log entry
- [Wikipedia:Copying within Wikipedia](https://en.wikipedia.org/wiki/Wikipedia:Copying_within_Wikipedia) and [Wikipedia:Reusing Wikipedia content](https://en.wikipedia.org/wiki/Wikipedia:Reusing_Wikipedia_content), on attribution through history
- [Help:Archiving a talk page](https://en.wikipedia.org/wiki/Help:Archiving_a_talk_page), for the archive subpages §5 reads
- [Extension:DiscussionTools](https://www.mediawiki.org/wiki/Extension:DiscussionTools), for the finer parsing not taken

## Amendment log

### A1. Merging and re-following

- **Date:** 2026-10-05
- **Source:** [0068](0068-merging-with-upstream.md) §2, §5
- **Change:** amends §6, §9
- **Summary:** The upstream diff's base follows `merge` records; merging and proposing replace the manual edit; re-following is a setting. Q1 settled.

Replaced text (§6, in part):

> The diff is computed live and never stored. Applying upstream's changes is a manual edit; a merge tool is Q1.

### A2. Following the talk page

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §7
- **Change:** amends §1, §5
- **Summary:** Forking asks whether the talk page follows upstream (the default where the repository's `talk` is `sync`) or is forked; `forked_from` records `talk`, and the `follow` page operation changes it. The conversion of §5 runs at fork time only for **Fork the talk page**, and later on **Stop following**. The front matter thread is created pinned and open.

Replaced text (§5):

> **The repository's talk page is converted into threads homed on the fork's talk page.**

> | **The text above the first heading**: the frontmatter of banners, WikiProject templates and talk headers | One thread, subject "*{title}*: talk page header", holding that text as its opening post. The direction says to treat the frontmatter as a section, and this is how |

> **Every thread is created closed.**

### A3. Copying files needs `upload`, default `autoconfirmed`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §8
- **Summary:** "Copying files" (§7) needs `upload`, whose default is `autoconfirmed` ([0039](0039-files-and-media.md) §21), not `user`; §8's permissions table had the default wrong. `reupload-shared` stays as 0039 §21 has it. (PENDING C13)

Replaced text (§8):

> | Copying files (§7) | `upload`, `reupload-shared` | `user`; `reupload-shared` as [0039](0039-files-and-media.md) §21 has it |

### A4. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§12
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [09](../architecture/09-security-and-moderation.md), [13](../architecture/13-mirrored-pages.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A5. Seeding is metadata-only by default, with content for the last 100 revisions

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §3, §4, §5; extends §10
- **Summary:** Every upstream revision is seeded as metadata (content, comment and attestation parts, no `text` part), so the history is complete as a list; the `text` part is seeded for the last `fork.content_revisions` (`site`, default 100) revisions only, which a tenant may raise up to the instance ceiling, and an article whose editors want its whole history diffable here asks the job for it explicitly. `fork.max_revisions` (default 20,000) becomes the per-job budget, the number of upstream revisions one job seeds, metadata included, beyond which the oldest are not seeded and the History tab links upstream. Seeding is never done for dependencies or archives: a dependency fork carries its upstream revision in `forked_from` and nothing more, a converted talk archive's threads carry `imported_from`, and their history is read upstream; a forked talk page's own history follows the same default. `view.upstream_revision` carries a `tenant` column leading its key, since two tenants may fork one article, and admits page subjects in the key form page subjects take across `view`. (REVIEW G37)

Replaced text ([13](../architecture/13-mirrored-pages.md) §3.4, as it stood):

> **`fork.max_revisions`** (`site`, default 20,000) caps what is seeded with content. Beyond it the oldest revisions are seeded as metadata only, so the history is complete and diffable for the recent past and complete as a list for all of it; the History tab says so and links upstream for the rest.

> **Serving.** `view.upstream_revision` ([0015](0015-record-format-and-partition-registry.md) §4) gains rows whose `entity_id` is the page ID in decimal, as page statements use it ([0038](0038-page-metadata-and-categories.md) §1).

Replaced text ([13](../architecture/13-mirrored-pages.md) §3.6, as it stood):

> - **otherwise it is forked as §3.1 forks a page:** a `create` carrying `forked_from` with the repository's current revision of the template, attributed upstream, tagged `fork-dependency`, with its history seeded by the same job (§3.4).

Replaced text ([13](../architecture/13-mirrored-pages.md) §5.12, as it stood):

> **The talk page's own history comes too.** Its upstream revisions, and each archive's, are seeded as upstream revision records keyed by the **fork's talk page ID**, as §3.4 seeds the article's, so the talk page's composite history ([0019](0019-discussions.md) §7) gains rows "upstream revision of Talk:…" with their text at `oldid=`, and nothing of what was said is lost to the splitting.
