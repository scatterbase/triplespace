# 0049. Boards; threads on several pages

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-09 (A6)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0011](0011-logs.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0022](0022-federation.md), [0023](0023-moderation.md), [0038](0038-page-metadata-and-categories.md), [0041](0041-content-models.md)
- **Uses:** [0012](0012-api-requirements.md), [0014](0014-caches-and-search.md), [0021](0021-notifications.md), [0028](0028-tenancy-policy.md), [0045](0045-table-content-model.md), [0047](0047-special-pages.md)

## Context

[0019](0019-discussions.md) made a talk page a composite view over threads, and attached each thread to exactly one target: a subject's talk page, or a user's. Two kinds of discussion do not fit.

**Discussion with no subject.** A wiki needs a place for general questions, announcements, requests and chat: what Wikipedia calls the village pump and its noticeboards, and Wikidata calls Project chat. On MediaWiki these are wikitext pages in the project namespace that grow until a bot archives them. Under 0019 the nearest equivalent is `Project talk:Village pump`, which needs a `Project:Village pump` page to exist only so that its talk page can, and which mixes "discussion about this page" with "discussion that has nowhere else to go".

**Discussion with several subjects.** A proposed merge concerns two items. A property proposal concerns the property, the items it would be used on and the project page that collects proposals. A request for comment is held in one place and announced on many. 0019 §2 answered all of these with "cross-posting is a link". In practice the link is a note posted by hand, the people watching the second page never see the replies, and on Wikipedia a bot (Legobot) maintains the listings that requests for comment depend on.

James's direction, from the design discussion of 2026-10-01:

- **A `Board` namespace**: a talk page that stands alone, for discussion not attached to a page or entity.
- **`Board talk` is reserved, but redirects to the board**: `Board talk:General chat` forwards to `Board:General chat`. `Thread talk` gets the same treatment.
- **A thread can be attached to several talk pages and boards.** At first those are chosen by hand. In time, a board could also gather threads automatically by scope.
- **A thread has one home and any number of listings.** Only the home governs protection and deletion.
- **Anyone who can create pages can create a board.**
- **A user talk page can be a thread's home but never a listing.**

## Decision

### 1. A board is a talk page that is its own subject

*Current text: [14](../architecture/14-discussions.md) §3.1.*

### 2. The namespaces, and talk namespaces that forward (amends 0008 §2, extends 0008 §3; amends 0019 §3)

*Current text: [14](../architecture/14-discussions.md) §1.4, §3.2.*

### 3. The `triplespace-board` content model (extends 0041 §3)

*Current text: [14](../architecture/14-discussions.md) §3.3.*

### 4. The definition

*Changed by A3.*

*Current text: [14](../architecture/14-discussions.md) §3.4.*

### 5. A thread has one home and any number of listings (amends 0019 §1–2)

*Current text: [14](../architecture/14-discussions.md) §1.3, §3.5.*

### 6. Records (amends 0019 §1 and §4)

*Changed by A4.*

*Current text: [14](../architecture/14-discussions.md) §1.2, §1.5, §3.6, §4.2.*

### 7. What the home decides, and what every attachment shares (amends 0016 §4, 0019 §12, 0023 §2 and §4; extends 0020 §2)

*Current text: [14](../architecture/14-discussions.md) §3.7.*

### 8. Rendering and the UI (amends 0019 §8)

*Changed by A4.*

*Current text: [19](../architecture/19-site-ui.md) §6.1.*

### 9. Logs and RDF (extends 0011 §6.1 and §8; extends 0019 §7)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §5.2, §5.3; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3, §6.1, §6.3.*

### 10. Federation (extends 0022 §6 and §7)

*Current text: [17](../architecture/17-federation-and-publication.md) §2.1, §2.2, §2.3.*

### 11. API (extends 0019 §9)

*Current text: [18](../architecture/18-api.md) §2.3, §3.2.*

### 12. Storage, caches and search (extends 0019 §11)

*Changed by A4, A5.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.8, §5, §11.1, §12.1.*

### 13. Permissions (extends 0019 §12)

*Changed by A4.*

*Current text: [09](../architecture/09-security-and-moderation.md) §8.1.*

### 14. Scopes, later

*Changed by A3.*

*Current text: [15](../architecture/15-structured-pages.md) §1.5.*

### 15. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **Discussion with no subject has a place.** A wiki gets a village pump, help desks and noticeboards without a placeholder page, and without archives ([0019](0019-discussions.md) §6).
- **A thread can appear wherever it is relevant, with one history.** A merge discussion is on both items' talk pages, and both sets of watchers see the replies. There is one status, one tree and one place to moderate.
- **Listing boards replace listing bots.** A request for comment lives on the talk page it concerns and is listed on `Board:Requests for comment`. When it is resolved, it collapses there too.
- **Protection and deletion stay predictable.** Only the home encloses, so listing a thread somewhere can never lock it or delete it.
- **Cross-posting is now a write.** It needs `move` and `edit` on the target, it is logged, and it is capped. That is the price of its being more than a link.
- **Two talk namespaces now redirect instead of standing empty.** A client that follows "talk page" from a board or a thread arrives back where it started. A client that assumes the talk page is a different page from the subject sees `talkid` equal to the page's own ID.
- **A busy board's history is long.** It includes the whole history of every thread listed on it, as a talk page's includes its threads'.
- **Listing order is attachment order,** which differs from creation order only for threads that moved or were listed.

## Open questions

- **Q1. `Group` handles for boards.** 0022 §6 addresses a talk page's `Group` by its subject's title. A board's title is free text, and could match an entity ID or a username.
- **Q2. Scopes.** *Settled by [0060](0060-scopes.md) §6.*
- **Q3. Pinned threads.** *Settled by [0069](0069-synchronized-talk-pages.md) §4.*
- **Q4. Detaching one's own thread.** Whether a thread's author may remove their thread from a listing without `move`.
- **Q5. Board moderators.** Whether a board needs a group of its own beyond what ACLs on it express.
- **Q6. DiscussionTools** (0019 Q4): which page a `discussiontoolspageinfo` call on a listing page reports for a listed thread.
- **Q7.** (Rest of Q2.) Whether a scoped board's members are announced and notify watchers as listed ones do.

## References

- [Wikipedia:Village pump](https://en.wikipedia.org/wiki/Wikipedia:Village_pump) and [Wikipedia:Requests for comment/All](https://en.wikipedia.org/wiki/Wikipedia:Requests_for_comment/All), the bot-maintained listing that listing boards replace
- [Extension:StructuredDiscussions](https://www.mediawiki.org/wiki/Extension:StructuredDiscussions) (Flow boards)
- [Activity Vocabulary](https://www.w3.org/TR/activitystreams-vocabulary/): `Add`, `Remove`, `Announce`, `Undo`
- [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces)

## Amendment log

### A1. Crate table

- **Date:** 2026-10-01
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §15
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A49).

Replaced text: §15 of this ADR.

### A2. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §15
- **Summary:** A1 was folded into the Decision. The open questions were numbered. No decision changed. Before this, A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A3. Scopes

- **Date:** 2026-10-04
- **Source:** [0060](0060-scopes.md) §6
- **Change:** amends §4, §14
- **Summary:** The `scope` key is accepted at board definition version 2 and takes a scope page or an inline kind of 0060 §4, meaning the threads attached to the talk pages of the selected subjects, or the board kinds `thread_statement` (this ADR's former `statement`) and `boards`. `subjects` is 0060's `statement`; `mentions` is withdrawn for 0060 Q2's `links`. Q2 is settled in part; Q7 holds the rest.

Replaced text: §14, in part of this ADR; §4 of this ADR; §14 of this ADR.

### A4. Pinned threads

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §4
- **Change:** extends §6, §8, §12, §13
- **Summary:** The `pin` and `unpin` operations, and `pinned` on `create`, pin a thread on one of its attachments; pinned threads come first, in pin order, exempt from the age rule; `thread.max_pinned` (default 3); `view.thread_attachment.pinned`; the rights of `attach`. A detach or a move without `keep` ends the pin. Q3 settled.

### A5. Listing keys carry a listing version

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §12
- **Summary:** The `tp:` listing key, like the `sc:` and `sp:` listing keys of [0060](0060-scopes.md) §10 and [0061](0061-sprints-and-tasks.md) §11, carries a **listing version**: the maximum activity ID over the listing's members, computed in Postgres at read time. This replaces the tag purge of the `tp:` listing on every record on a thread; [0014](0014-caches-and-search.md) §1 principles 1 and 4 hold unchanged. Tag purges remain for erasure and hiding. (PENDING B4)

Replaced text: §12 of this ADR.

### A6. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§15
