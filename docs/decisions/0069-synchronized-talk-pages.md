# 0069. Synchronized talk pages; pinned threads

- **Status:** Proposed
- **Date:** 2026-10-06
- **Updated:** 2026-10-09 (A4)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md), [0022](0022-federation.md), [0030](0030-edit-filters.md), [0049](0049-boards.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [0067](0067-proposals.md), [0068](0068-merging-with-upstream.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0027](0027-preferences-and-portability.md), [0038](0038-page-metadata-and-categories.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0047](0047-special-pages.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [13](../architecture/13-mirrored-pages.md), [14](../architecture/14-discussions.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md)

## Context

[0052](0052-page-repositories-and-title-inheritance.md) and [0053](0053-mirrored-pages.md) put foreign pages beside local ones under one title, and [0054](0054-forking-a-mirrored-page.md) made a foreign page local by forking it. Discussion did not come along on the same terms. A foreign page's talk page is local and holds local threads only ([0052](0052-page-repositories-and-title-inheritance.md) §7, [0019](0019-discussions.md) §2): a reader of MDWiki's copy of *Myocardial infarction* sees none of the discussion on English Wikipedia's `Talk:Myocardial infarction`, only a link to it. A fork converts the upstream talk page once, into closed threads, and stops ([0054](0054-forking-a-mirrored-page.md) §5); [0068](0068-merging-with-upstream.md) Q2 asked whether a pull should bring new upstream sections too.

The pieces for doing better exist. A mirrored page is a record in `pages/{repo}` that follows the repository's events ([0053](0053-mirrored-pages.md) §5–6). The splitter of 0054 §5 already turns a wikitext talk page into one thread per level-two section, with the frontmatter above the first heading as a thread of its own. [0067](0067-proposals.md) §6 specified the instance as an OAuth client of an upstream wiki, acting as the person under a grant they give, separately from login. And every Wikimedia wiki runs DiscussionTools, whose API names each section and each signed comment with a stable identifier and accepts a reply to any of them.

[0049](0049-boards.md) Q3 left pinned threads open, asked of boards: an announcements board needs a thread that stays first.

### Direction

James's direction, from the design discussion of 2026-10-06:

- **When a talk namespace is synchronized from a remote wiki, the sections are translated into threads, one per H2.**
- **Section 0 is front matter and is treated as a pinned thread,** so boards need a concept of pinned threads. **This does not give rise to a board front-matter concept:** on Wikipedia most of what front matter does would be covered by page-level statements, and the rest can be done in a pinned thread. For compatibility, Wikipedia talk page front matter is treated as a pinned thread.
- **With a connected account on the wiki being mirrored** (for example, logged in with a Wikimedia account while Wikipedia is mirrored), **a person can respond to threads as though on the wiki.**
- **A person can start a new thread and choose whether it is local-only or sent to the upstream wiki.** One sent upstream is automatically syndicated back to the synchronized talk page.
- **Whether forking an article permanently forks the talk page is up to the user.** A temporary fork, whose editors plan to contribute back, keeps up with the upstream talk page; a permanent fork has no further need to track it.

## Decision

### 1. A synchronized talk namespace (extends 0052 §1; amends 0052 §7)

*Current text: [13](../architecture/13-mirrored-pages.md) §1.1, §2.5, §5.1, §5.2.*

### 2. Foreign threads (extends 0053 §1 and §5)

*Current text: [13](../architecture/13-mirrored-pages.md) §2.5, §5.3, §5.4, §5.5.*

### 3. The talk page holds both (amends 0019 §2; extends 0019 §8)

*Current text: [13](../architecture/13-mirrored-pages.md) §5.6.*

### 4. Pinned threads (settles 0049 Q3; extends 0019 §1, §4, §8; extends 0049 §6, §8, §12, §13)

*Current text: [14](../architecture/14-discussions.md) §1.2, §1.5, §4.1, §4.2, §4.3, §4.4; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 5. Replying upstream (extends 0067 §6; extends 0030 §2)

*Current text: [13](../architecture/13-mirrored-pages.md) §5.7, §5.8, §5.9.*

### 6. Starting a thread: here or upstream

*Current text: [13](../architecture/13-mirrored-pages.md) §5.10.*

### 7. Forks: following the talk page or forking it (amends 0054 §1, §5; extends 0008 §4; settles 0068 Q2)

*Changed by A3.*

*Current text: [13](../architecture/13-mirrored-pages.md) §3.1, §3.2, §4.4, §5.11, §5.12.*

### 8. Feeds, watches and notifications (extends 0020 §2; extends 0021 §2)

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.2, §4.3, §5.2.*

### 9. API (extends 0012 §5)

*Changed by A2.*

*Current text: [18](../architecture/18-api.md) §2.3, §3.2.*

### 10. Storage (extends 0013 §5.6)

*Changed by A1.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.8, §5, §11.1, §12.1.*

### 11. Settings and permissions

*Current text: [09](../architecture/09-security-and-moderation.md) §8.1; [23](../architecture/23-configuration-and-registry.md) §2.2, §3.2, §3.3, §3.5.*

### 12. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **Importing upstream sections as local thread records,** continuously, as 0054 §5 does once. Every upstream edit to a section would have to become a local `edit` of a post someone here did not write, attributed to an importer; the threads would be copies that drift. Mirrored records with upstream attribution are what the repository already does for articles.
- **Splitting foreign sections into one local post per signed comment.** 0054's alternatives rejected this for imports, because signatures are heuristic. DiscussionTools' parse is the repository's own and is used here for reply targets and the tree, but the text is still rendered from the section's HTML, so a misparse costs a reply button, not a misattributed post.
- **Local replies inside foreign threads.** Rejected (§5): a thread whose halves live on two wikis is complete on neither.
- **Posting upstream under an instance account.** Rejected by 0067 §6 for the same reasons: attribution and responsibility belong to the person.
- **Login grants editing.** Rejected by 0067 §6; a Wikimedia login makes the grant one click instead.
- **A front matter slot on talk pages or boards.** Rejected by direction (§4): banners are statements about the subject, and the rest is a pinned thread.
- **Pinning as a key in a board's definition**, as 0049 Q3 suggested. Rejected: it works for boards only, a definition edit is the wrong history for "pinned this", and a talk page has no definition.
- **Forking the talk page by default.** Rejected (§7): following is reversible and forking is not.

## Consequences

- **A mirrored article brings its discussion.** A reader sees what upstream editors are saying, beside what local editors are saying, on one talk page, each marked by origin.
- **Someone with a Wikimedia account can take part in Wikipedia's discussion from the mirror,** with their own account, under Wikipedia's rules, and see the replies here and in their bell.
- **Local discussion stays local unless the person sends it.** Nothing local reaches upstream except a send, which names its destination.
- **Talk pages are the first place the instance writes to another wiki routinely.** The grant client of 0067 §6 is built now rather than later, and the instance's relationship with the repository's operators (its consumer registration, its edits' tag) is visible upstream from the first reply.
- **Forks can stay in conversation with upstream.** A temporary fork keeps upstream's talk page as long as it wants, and its proposals are discussed there. 0068 Q2 is settled.
- **Boards and talk pages have pins.** 0049 Q3 is settled, for talk pages and boards alike, as a property of each attachment.
- **Talk pages of followed subjects are always mirrored,** so `pages/{repo}` grows with talk pages even on proxied repositories; they are small next to the articles and are tombstoned when unused.
- **Thread identity on repositories without DiscussionTools is best effort.** Most third-party MediaWiki wikis lack it, and their foreign threads may split or merge when sections are reorganized.
- **Test plan.** A followed talk page with sections, front matter and an archived section: threads, pinned section 0, `archived` found by name after an archive bot's edit; a removed and restored section keeps its `n`; rebuild reproduces every `n`; a reply sends `discussiontoolsedit` with the comment's name and appears for its author at once and for everyone after the next `put`, moving `ops.upstream_post` to `synced`; an edit filter `disallow` on `upstream-post` stops the send; an upstream AbuseFilter warning is shown and confirmed; a person without a grant gets the connect offer; a fork with Keep following shows foreign threads on its talk page, Stop following converts them with front matter pinned and open, Follow again hides those already imported; `thread.max_pinned` is enforced and pins end on detach; `discussiontoolsedit` through the Action API reaches upstream.

## Open questions

- **Q1. Continuing a foreign discussion locally.** Whether a local thread started from **Discuss here instead** should carry a structured reference to the foreign thread (`about: {repo, n}`), shown on both, rather than only a link in its text.
- **Q2. Local actions on foreign threads.** Whether a foreign thread can be pinned, listed on a board ([0049](0049-boards.md) §5) or given thread statements here, which needs local records keyed by its ranged ID, as a foreign page's talk page already is.
- **Q3. Sending a local thread upstream.** Whether a local thread can later be posted upstream as a new section, with its local replies quoted or summarised, and what becomes of the local thread.
- **Q4. Upstream closure markers.** Whether `{{archive top}}`, `{{closed rfc top}}` and similar templates, which close a discussion without archiving it, should map to a closed status, which needs a per-repository list of templates.
- **Q5. Editing front matter.** Whether a person holding a grant may edit upstream's section 0 through the instance, and whether WikiProject banners there could be read into page statements on the local page ([0038](0038-page-metadata-and-categories.md)).
- **Q6. Re-following and local threads.** When a fork is re-followed ([0068](0068-merging-with-upstream.md) §5) and deleted, whether its local threads should move to the foreign page's talk page instead of being deleted by enclosure.
- **Q7. Other talk namespaces.** `User talk` on the repository is not followed: a local user page is local. Whether a person's own upstream user talk page should be followable from their account, as a private convenience.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §12 | amends | 0005 A72 |
| [0008](0008-namespaces-and-document-pages.md) §4 | §7 | extends | 0008 A30 |
| [0011](0011-logs.md) §6.1 | §4 | extends | 0011 A21 |
| [0012](0012-api-requirements.md) §5 | §9 | extends | 0012 A49 |
| [0019](0019-discussions.md) §2 | §3 | amends | 0019 A15 |
| [0019](0019-discussions.md) §1, §4, §8 | §3, §4 | extends | 0019 A15 |
| [0020](0020-change-feeds.md) §2 | §8 | extends | 0020 A12 |
| [0021](0021-notifications.md) §2 | §8 | extends | 0021 A11 |
| [0022](0022-federation.md) §7 | §4, §8 | extends | 0022 A6 |
| [0030](0030-edit-filters.md) §2 | §5 | extends | 0030 A10 |
| [0049](0049-boards.md) §6, §8, §12, §13 | §4 | extends | 0049 A4 |
| [0049](0049-boards.md) Q3 | §4 | settles | 0049 Q3 |
| [0052](0052-page-repositories-and-title-inheritance.md) §1 | §1 | extends | 0052 A1 |
| [0052](0052-page-repositories-and-title-inheritance.md) §7 | §1, §3 | amends | 0052 A1 |
| [0053](0053-mirrored-pages.md) §1, §5 | §1, §2 | extends | 0053 A1 |
| [0054](0054-forking-a-mirrored-page.md) §1, §5 | §7 | amends | 0054 A2 |
| [0067](0067-proposals.md) §6 | §5 | extends | 0067 A2 |
| [0068](0068-merging-with-upstream.md) Q2 | §7 | settles | 0068 Q2 |

## References

- [Extension:DiscussionTools](https://www.mediawiki.org/wiki/Extension:DiscussionTools) and its API modules `discussiontoolspageinfo`, `discussiontoolsedit` and `discussiontoolspreview`; [thread item IDs](https://www.mediawiki.org/wiki/Extension:DiscussionTools/How_it_works), which survive archiving
- [Help:Archiving a talk page](https://en.wikipedia.org/wiki/Help:Archiving_a_talk_page) and [User:ClueBot III](https://en.wikipedia.org/wiki/User:ClueBot_III), [User:Lowercase sigmabot III](https://en.wikipedia.org/wiki/User:Lowercase_sigmabot_III): the archive edits §2 follows
- [Wikipedia:Talk page layout](https://en.wikipedia.org/wiki/Wikipedia:Talk_page_layout): what front matter holds
- [OAuth/For Developers](https://www.mediawiki.org/wiki/OAuth/For_Developers), and the change tag Wikimedia wikis apply to OAuth edits
- Mastodon's `featured` collection, used for pinned posts

## Amendment log

### A1. Listing versions in place of tag purges

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §10
- **Summary:** Listing keys (`tp:`, `sc:`, `sp:`) carry a **listing version**, the maximum activity ID over the listing's members, computed in Postgres at read time, in place of tag purges on ordinary writes; 0014 §1 principles 1 and 4 hold unchanged. Tag purges remain for erasure and hiding. For this ADR: a `put` of a followed talk page no longer purges the `tp:` listings of the local talk pages that follow it by tag; the listing's version changes with the `put` instead. (PENDING B4)

Replaced text (§10):

> **Caches** ([0014](0014-caches-and-search.md) §7): a `put` of a followed talk page purges the `tp:` listing of every local talk page that follows it, by tag.

### A2. No `list=threads`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §9
- **Summary:** There is no `list=threads`; `origin=` is a parameter of `GET /page/{id}/threads` only. The Action API paragraph's `thorigin` on `list=threads` goes; the route table's `origin=local|upstream` filter stands. (PENDING E34)

Replaced text (§9):

> `list=threads` accepts `thorigin`.

### A3. Talk-destination proposals on a following fork

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §7
- **Summary:** 0069 (the newer) stands: a following fork sends a `talk`-destination proposal through §6 now, gated by §6's switch (`talk.upstream_post`), not by `proposals.push`; 0068 §4's "later phase" applies to page proposals only. The row's verb is amends, but nothing in §7 is contradicted (its "Proposals meet discussion" paragraph already sends the proposal by §6's mechanism), so the entry extends §7 with the gate; the contradicted text is in 0068 §4. (PENDING F3)

### A4. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§12
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [13](../architecture/13-mirrored-pages.md), [14](../architecture/14-discussions.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
