# 0019. Discussions

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-11-05 (A31)
- **Author:** James Hare / Claude Fable
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0018](0018-tenants.md), [0022](0022-federation.md)

## Context

[0008](0008-namespaces-and-document-pages.md) §2 reserved a talk namespace beside every subject namespace and left discussions to their own ADR. This is that ADR.

The design principle of 0008 is that a namespace's kind says what a page is made of. `Item:` and `Property:` are composite views over the resolved graph; `Project:` and `User:` are blobs of stored text. A talk page is a third thing: **a composite view over threads**. Each thread is its own page with its own history. A post refers to the post it answers. A thread is attached to one talk page and can be moved to another. The talk page's history is the histories of its threads, presented as one.

MediaWiki has tried this shape twice. **LiquidThreads** put each thread in a `Thread:` namespace and moved threads between talk pages. **Flow** (Structured Discussions) put each topic on a `Topic:` page, made the talk page a board of topics, and let topics move between boards. Both are gone, and the reasons are design inputs here: topics had opaque UUID titles and URLs nobody could cite; the board's history was hard to read; watching a board and watching a topic did not compose; and because the content was not text, the rest of the wiki's tooling (history, diffs, contributions, backlinks, search) treated it as foreign. **DiscussionTools**, which is what Wikimedia wikis use now, keeps talk pages as wikitext and overlays structure by parsing signatures and indentation. That is not available here: Triplespace has no wikitext parser beyond the subset of 0008 §8, and it does not want its discussions to depend on `~~~~`.

Two goals from the earlier ADRs constrain the design. Every unit of contribution has to be a record with a global revision ID ([0015](0015-record-format-and-partition-registry.md) §2), so that history, contributions, recent changes, diffs, permalinks, hiding and erasure all work without anything new. And the instance loads *data*, not the discussion behind it: foreign talk pages are never mirrored.

[0011](0011-logs.md) §8 already types local log events with Activity Streams 2.0. Posts and threads are described with the same vocabulary.

## Decision

### 1. A thread is a page; a post is a record

*Current text: [14](../architecture/14-discussions.md) §1.1, §1.2, §3.6.*

### 2. Talk pages are composite; attachment is by identifier (amends 0008 §2)

*Current text: [14](../architecture/14-discussions.md) §1.3, §3.5.*

### 3. The `Thread` namespace and thread titles (extends 0008 §1 and §3)

*Current text: [14](../architecture/14-discussions.md) §1.4.*

### 4. The record: four parts (amends 0015 §1)

*Current text: [14](../architecture/14-discussions.md) §1.5, §3.6, §4.2.*

### 5. Text: markdown, mentions and links (extends 0008 §5, §8 and §10)

*Current text: [14](../architecture/14-discussions.md) §1.6.*

### 6. Status and visibility

*Current text: [14](../architecture/14-discussions.md) §2.1, §2.2, §2.3, §2.4.*

### 7. History, activity, logs and RDF (extends 0001 §1 and §6, 0011 §6.1 and §8)

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.3, §6.1, §6.3.*

### 8. Rendering and the UI (extends 0010)

*Current text: [19](../architecture/19-site-ui.md) §6.1.*

### 9. API (extends 0012 §4 and §5)

*Current text: [18](../architecture/18-api.md) §2.1, §2.3, §2.4, §3.2.*

### 10. The Activity Streams profile

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §6.2.*

### 11. Storage, caches and search (extends 0013 §5 and 0014)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.8, §5, §6.1, §11.1, §12.1.*

### 12. Permissions (extends 0016 §2; amends 0016 §4)

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §4.4, §8.1.*

### 13. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **A post is a revision, so everything that works on revisions works on posts.** History, diffs, contributions, recent changes, permalinks, hiding, erasure, inclusion proofs and export bundles need no discussion-specific code. This is the property Flow and LiquidThreads lacked.
- **Talk pages are not text.** Bots that edit, archive or template talk pages do not work; `action=edit` tells them so. Existing wikitext talk pages cannot be imported as they are (Q5).
- **The three-part uniformity of 0015 is loosened.** One payload type has four parts. Verification, erasure and the storage schema were already written for *n* parts, so the cost is one sentence and one more `parts` value.
- **Thread names are readable and stable across moves, and renames do not redirect.** A link to a renamed thread breaks, as a link to a renamed user page does. Permalinks by revision or page ID do not.
- **Status is social.** Anyone may mark a thread resolved, and anyone may reopen it, in public, with attribution. The instance decides only what the marks are called and how the UI folds them.
- **No archives, no `/Archive N` subpages, no lost history.** Old threads are collapsed, not moved.
- **Discussion stays local.** A mirrored entity's talk page is this wiki's; it is never fed upstream or to another tenant, and no upstream discussion is fetched.
- **The AS2 vocabulary costs nothing now and keeps federation possible later.**

## Open questions

- **Q1. Namespace numbers.** *Settled by `docs/registry/namespaces.toml`, under the numbering policy of 0008 A6.*
- **Q2. Temporary accounts.** Whether a `temp` account gets a `User talk` page for messages, as MediaWiki gives one. [0021](0021-notifications.md) asks the same of their notifications.
- **Q3. Notifications.** *Settled by [0021](0021-notifications.md) §2.*
- **Q4. DiscussionTools compatibility.** Whether to implement `discussiontoolsedit` and `discussiontoolspageinfo` on these records, and how far Convenient Discussions and similar scripts can be made to work.
- **Q5. Importing existing talk pages.** *Settled by [0054](0054-forking-a-mirrored-page.md) §5.*
- **Q6. Cross-tenant discussion.** *Settled by [0028](0028-tenancy-policy.md) §6.*
- **Q7. Federation.** *Settled by [0022](0022-federation.md) Part B.*
- **Q8. Limits.** *Settled by [0024](0024-subsidiary-accounts.md) §5, as to where they live.*

## References

- [Extension:LiquidThreads](https://www.mediawiki.org/wiki/Extension:LiquidThreads) and [Extension:StructuredDiscussions](https://www.mediawiki.org/wiki/Extension:StructuredDiscussions) (Flow)
- [Extension:DiscussionTools](https://www.mediawiki.org/wiki/Extension:DiscussionTools) and [API:Discussiontoolsedit](https://www.mediawiki.org/wiki/Extension:DiscussionTools/API)
- [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces)
- [Activity Streams 2.0](https://www.w3.org/TR/activitystreams-core/) and the [Activity Vocabulary](https://www.w3.org/TR/activitystreams-vocabulary/)
- [ActivityPub](https://www.w3.org/TR/activitypub/)
- [CommonMark](https://commonmark.org/)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** 0005 §2
- **Change:** supersedes §13
- **Summary:** 0005 §2 is the one crate table CI checks, and carries `scatter-threads` and every change this section listed (0005 A15).

Replaced text: §13 of this ADR.

### A2. Watching

- **Date:** 2026-09-27
- **Source:** 0020 §3
- **Change:** extends §8
- **Summary:** Watching a subject watches its talk page and the threads attached to it; a thread can be watched on its own. §8 had been given the sentence in place.

### A3. Notifications

- **Date:** 2026-09-27
- **Source:** 0021 §2
- **Change:** extends §6
- **Summary:** `mention`, `reply`, `talk` and `thread-status` reasons; the `thread-status` notification fires when a thread moves from an open to a closed status. This settled Q3.

### A4. Deletion is an ACL; posts are patrollable

- **Date:** 2026-09-27
- **Source:** 0023 §4, §6
- **Change:** amends §1, §7, §12; extends §9
- **Summary:** `delete` and `undelete` are no longer thread operations. Deleting a thread is a `read` ACL on its page ID, written to the tenant `log` partition, and undeletion retires it; hiding one post is a `record` ACL on that post's record with `parts: [text]`. The `thread` payload type carries `create`, `post`, `edit`, `rename` and `move`, and the sentence below about "any operation on a deleted thread other than `undelete`" reads as "any operation on a deleted thread". Posts and thread operations are patrollable.

Replaced text: §1 of this ADR; §7 of this ADR; §12 of this ADR.

### A5. Visibility as a preference

- **Date:** 2026-09-27
- **Source:** 0027 §1
- **Change:** amends §6
- **Summary:** The visibility rule of §6 is the `threads.visibility` preference.

Replaced text: §6 of this ADR.

### A6. Cross-tenant discussion

- **Date:** 2026-09-27
- **Source:** 0028 §6
- **Change:** extends §2
- **Summary:** A tenant may show, read-only and by policy, a provider tenant's threads about an entity it mirrors. This settled Q6.

### A7. Thread statuses are configuration

- **Date:** 2026-09-27
- **Source:** Direct: James, review of 2026-09-27 (third pass)
- **Change:** amends §6
- **Summary:** Statuses are `config` records of kind `thread-status` in the tenant `config` (0015 §3), each with a label, a **category** (`open` or `closed`) and a display order; the defaults ship in `docs/registry/thread-statuses.toml` and follow the labels of bug trackers and project tools: **Open**, **In progress** and **Blocked** (open), and **Resolved**, **Won't fix**, **Declined** and **Stale** (closed). `rejected` is renamed `declined`. The category, not the name, drives the default visibility rule below (closed threads collapse) and the `thread-status` notification of 0021 §2, which fires when a thread moves from an open to a closed status. `open` is required and cannot be retired.

Replaced text: §6 of this ADR.

### A8. Articles and thread statements

- **Date:** 2026-09-29
- **Source:** 0038 §8–9, §11
- **Change:** amends §5, §7; extends §6
- **Summary:** By section:
  - §5: With articles in the main namespace, `[[Q42]]` links to the main-namespace page titled `Q42`, in markdown as in wikitext. An entity is mentioned with its namespace: `[[Item:Q42]]`, which still renders with the entity's label.
  - §6: Threads carry statements, as document pages do. When the `thread-status` role is bound, a thread's status is also a projected, read-only statement, derived from the latest status-bearing post; this section stays the one place a status is set, and asserting that property on a thread is refused.
  - §7: Except thread statements, which are output in the main graph as page statements are.

Replaced text: §5 of this ADR; §7 of this ADR.

### A9. Content models

- **Date:** 2026-09-30
- **Source:** 0041 §4, §8, §10
- **Change:** amends §2, §3, §9, §11
- **Summary:** By section:
  - §2: The kind `composite` is gone: a talk namespace is a `pages` namespace whose model is `triplespace-talk`, a model whose source is *composite* (no records of its own).
  - §3: The kind `thread` is gone: `Thread` is a `pages` namespace whose model is `triplespace-thread`.
  - §9: `action=edit` on a `Thread:` or talk title is refused with MediaWiki's own `no-direct-editing` (`apierror-no-direct-editing`), because neither model supports direct editing. `ts-structured` is retired.
  - §11: The `content_model` values are the registry IDs `triplespace-thread` and `triplespace-talk`, not `thread` and `talk`.

Replaced text: §2 of this ADR; §3 of this ADR; §9 of this ADR.

### A10. Boards: one home, any number of listings

- **Date:** 2026-10-01
- **Source:** 0049 §2, §5–9, §11–13
- **Change:** amends §1, §2, §3, §7, §8, §12; extends §4, §9, §11
- **Summary:** By section:
  - §1: Two operations join the table: `attach` lists the thread on a further talk page or board, and `detach` removes a listing. Both need a base offset. `create` may carry `also`, further targets listed at creation, and `move` may carry `keep`, which keeps the old home as a listing.
  - §2: A thread has exactly one **home**, the attachment this section describes, and may have **listings** on further talk pages and boards, up to `thread.max_attachments`. "A thread cannot be attached to two pages; cross-posting is a link" is replaced. Only the home encloses the thread, is its `as:context`, and may be a user talk page; listing, composite history and watches treat every attachment alike. A board (0049 §1) is a `page` target whose talk page is itself.
  - §3: `Thread talk` (215) is a `virtual` namespace that forwards to `Thread`: `Thread talk:X` resolves to `Thread:X`. A thread is its own talk page.
  - §4: `also` on `create`; `target` and `talk` on `attach` and `detach`; `keep` on `move`.
  - §7: A talk page's or board's history includes every thread attached to it, home or listing, and the `thread/attach` and `thread/detach` events. A thread's `as:context` is its home, and it carries `scatter:listedOn` for each listing.
  - §8: A talk page lists its threads in the order they were attached to it, which is creation order unless a thread was moved or listed there. A thread shows its home and its listings.
  - §9: `POST /thread/{page ID}/attach` and `/detach`; `also` on `POST /thread` and `keep` on `/move`; `home` and `listings` in `GET /thread/{page ID}`; `home` on each thread of `GET /page/{id}/threads`.
  - §11: `view.thread_attachment` holds every attachment, home or listing, and serves the listing query in place of the `thread_talk` index; `view.thread.talk_page_id` is the home. Boards have `view.talk_page` rows.
  - §12: A talk page encloses the threads whose *home* it is, not those listed on it. `attach` and `detach` need `move` on the thread and `edit` on the target.

Replaced text: §1 of this ADR; §2 of this ADR; §7 of this ADR; §8 of this ADR; §9 of this ADR; §11 of this ADR; §12 of this ADR.

### A11. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** 0050 §13
- **Change:** consolidates §1–13
- **Summary:** A1–A10 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A4 (in part), A7, A8, A9 and A10 were eighteen blockquotes, and A2, A3, A5 and A6 were recorded in the header or in other ADRs. The file before conversion is commit `0b26a3a`.

### A12. Talk pages of inherited pages

- **Date:** 2026-10-01
- **Source:** 0052 §7
- **Change:** extends §2
- **Summary:** A page a repository serves has a local talk page on its provider-ranged page ID, as a mirrored entity has one; the fork job moves those threads to the fork's talk page.

### A13. Imported threads: wikitext posts, closed on creation, dated by the import

- **Date:** 2026-10-01
- **Source:** 0054 §5
- **Change:** amends §3, §4, §5, §6
- **Summary:** `status` may appear on `create`; the content part gains `mediaType` (`text/markdown` or `text/x-wiki`, the latter written only by imports and rendered by the wikitext pipeline) and `imported_from`; a thread an import creates takes the date the import supplies for its title; the default statuses gain **Archived** (closed). Q5 settled.

Replaced text: §3 of this ADR; §4 of this ADR; §5 of this ADR; §6 of this ADR.

### A14. Proposal operations

- **Date:** 2026-10-05
- **Source:** 0067 §3
- **Change:** extends §1
- **Summary:** `propose`, `submit` and `withdraw` join the operation table; a `create` may carry `propose`'s fields.

### A15. Followed talk pages and pinned threads

- **Date:** 2026-10-06
- **Source:** 0069 §3, §4, §7
- **Change:** amends §2; extends §1, §4, §8
- **Summary:** "Foreign talk pages are never loaded" is narrowed to mirrored entities: a page repository with `talk = sync` has its talk pages followed, and their sections are shown as foreign threads beside local ones, as are those of a fork that follows upstream. `pin` and `unpin` join the operations; `create` may carry `pinned`; `imported_from` may carry a section's name hash. Talk pages list pinned threads first and merge foreign threads by time.

Replaced text: §2 of this ADR.

### A16. `GET /page/{id}/history` is defined in 0012 §5

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §9
- **Summary:** `GET /page/{id}/history` is defined in 0012 §5's page table, which took it from §7; §9's route table cites that definition and no longer calls the route "Unchanged", since it is this ADR that gave the route its composite behaviour for a talk page ID. (PENDING E36)

### A17. The Comment part on every operation that carries a summary

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §4
- **Summary:** The Comment row of §4's parts table lists every operation that carries a summary: `edit`, `rename`, `move`, `attach`, `detach`, `pin`, `unpin`, `propose`, `submit` and `withdraw`; it stays null for `create` and `post`, whose text is the contribution. The row had not been extended when A10, A14 and A15 added the operations. (PENDING F5)

Replaced text: §4 of this ADR.

### A18. `pin` and `unpin` name any attachment

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §4
- **Summary:** In §4's content-part table, `target` is a `page` target only for `attach` and `detach`; `pin` and `unpin` name any attachment of the thread, an `actor` target included (0049 §6, 0069 §4). (PENDING F6)

Replaced text: §4 of this ADR.

### A19. A proposal's fields are page metadata set by `propose`

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §4
- **Summary:** A proposal's kind, destination, base, payload, omitted and flags are **page metadata of the thread page**, set by the `propose` operation's content part and projected to `view.proposal` (0067 §3, 0038 §1); they are not fields of `create`, whose `target` stays the thread's home. The ledger verb is amends, but §4's table never listed those fields on `create` (only A14's Summary said a `create` may carry them), so this entry extends §4 with the `propose` row and contradicts no text of it. (PENDING F7)

### A20. Who may set `declined` on a proposal thread

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §6
- **Summary:** On a proposal thread the `declined` status may be set only by the proposer or by a holder of `edit` on the subject's talk page; any other post carrying `declined` is refused with `ts-proposal-status` (0067 §5). §6's rule that anyone who may post may set a status holds for every other status and every other thread, and notes this exception. (PENDING F8)

Replaced text: §6 of this ADR.

### A21. `thread-statuses.toml` holds the shipped statuses

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §6
- **Summary:** `docs/registry/thread-statuses.toml` is the registry file of the default statuses §6 names, with name, label, category and order per status and `required = true` on `open`: **Open**, **In progress** and **Blocked** (open), and **Resolved**, **Won't fix**, **Declined**, **Stale** and **Archived** (closed). The consolidation audit had recorded the file as missing because the chapter writers were given a partial copy of the registry; it existed. Nothing is created. (PENDING F29)

### A22. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** 0050 §14
- **Change:** relocates §1–§13

### A23. Server-minted values move to the attestation map

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §2, §3, §4
- **Summary:** A value the server mints into a thread record while appending it never enters a client-signed part. The talk page ID a `create`, `move`, `attach`, `detach`, `pin` or `unpin` carries for its target, the talk page IDs of each `also` entry, and the minted title of a `create` or `rename`, suffix included, are carried in the record's attestation map rather than its content part, so that the content and comment parts stay the bytes the client submitted and a client signature over them verifies; the client submits the subject, the server mints the title; a rebuild reads the IDs and titles from the attestation maps; the content part carries `target` alone. A page `move`, by contrast, carries its new title in the content part, because the mover chose it. The ledger names §2; the title rule is §3's and the parts table is §4's, so all three are changed. (REVIEW G23)

Replaced text: 14 §1.3, §1.4, §1.5, §3.6, §4.2.

### A24. Heading posts, wrapper posts and imported posts' provenance

- **Date:** 2026-10-20
- **Source:** 0090 §3, §8
- **Change:** extends §4, §5
- **Summary:** The thread payload gains `heading` and `wrap` on `post`, for the heading posts and wrapper posts a talk-page conversion writes; `imported_from` may appear on `post` as well as `create`, carrying each imported post's DiscussionTools name and ID, `signed_as`, `added_by`, original timestamp and fallback reason; `inReplyTo` may name a heading post. Heading posts render as headings and wrapper posts around the posts they enclose. Chapter text: 14 §1.5, §1.6.

Replaced text: 14 §1.5.

Replaced text (14 §1.6): none; a paragraph on heading and wrapper posts is added before the one on `~~~~`.

### A25. The `closed` status, and collapsing a range of posts

- **Date:** 2026-10-21
- **Source:** Direct: James, design discussion of 2026-10-21
- **Change:** amends §4, §5; extends §6
- **Summary:** A default thread status `closed` (category `closed`) is added to `thread-statuses.toml`, for a discussion someone formally closed with a statement; a talk-page conversion writes it for a closing template (0090 A1). A post may carry `collapse: {first, last}`, the revision IDs of two posts of the same thread, and the range is shown collapsed beneath the post's text, its posts keeping their places and parents; a later post carrying `uncollapse` with the collapsing post's revision ID undoes it. Collapsing is native, a claim in public like a status: anyone who may post may collapse, and imports write collapsing posts for wrapper templates. The wrapper posts and `wrap` field of A24 are withdrawn. Chapter text: 14 §1.5, §1.6, §2.1, §2.4.

Replaced text: 14 §1.5, §1.6.

### A26. Rules settled while building `scatter-threads`

- **Date:** 2026-10-22
- **Source:** Direct: James, design discussion of 2026-10-22
- **Change:** extends §3, §4, §6
- **Summary:** Calls the pure crate made where the text was silent, each confirmed by James. **Subjects** (§3): underscores and runs of whitespace become one space, the ends are trimmed and the first letter is upper-cased; control characters are refused beside MediaWiki's forbidden ones, and a subject must leave room for the date and a ` (n)` suffix within the 255-byte title limit. **`imported_from` keys** (§4): `repository`, `page`, `revision`, `archive` and `section_hash` for a thread's source; `name`, `id`, `signed_as`, `added_by` (`revision`, `actor`), `timestamp`, `fallback` and `wrapper` for an imported comment; any other key is refused (James: "reasonable"). **Import-only fields** (§4): `imported_from`, `heading` and the wikitext media type are refused on an original `create` or `post` and accepted on an imported one (James: "If a field is only applicable for an import it should not be allowed for an original post, but if it genuinely is an imported post it should be fine"). **Media type** (§4): an `edit` carries its post's own media type, so a post keeps its markup for good. **Pins** (§4): `pinned: true` on `create` counts against `thread.max_pinned` on the home, as a `pin` does. **Statuses** (§6): a retired status can no longer be set, threads that carry it keep it, and a status the registry no longer lists reads as open; `closed` (A25) is a current default, not a retired one (James: "it was a gap in the original model"). **Collapses** (§6): ranges may nest but not cross; a partly overlapping range is refused. Chapter text: 14 §1.4, §1.5, §2.1, §2.4, §4.2.

### A27. A thread record that is a log event carries a log ID

- **Date:** 2026-10-22
- **Source:** Direct: James, "proceed to what you feel the best next step is", 2026-10-22; implementation calls for his review
- **Change:** extends §7
- **Summary:** Made while projecting thread records as activity rows. A thread record that is also a log event (`create`, `rename`, `move`, `attach`, `detach`, `pin`, `unpin`) takes a log ID from the tenant's sequence beside its revision ID, as a MediaWiki page creation has both a revision and a log entry; `post` and `edit` take none. Chapter text: 16 §6.1.

### A28. Post markup is a tenant setting; a trailing signature is removed on save

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-24
- **Change:** amends §4, §5; extends §8
- **Summary:** James: "I like the idea of quietly removing the tildes upon save. There isn't really a need to scold the user. Whether posts use markdown or wikitext can be an instance or tenant setting." With his choices of 2026-10-25 on the details: **Markup.** The tenant `site` setting `thread.markup` (`markdown` or `wikitext`, default `markdown`; a tenant sets it or inherits its instance's default, 0085 §2.1) names the media type of every original `create` and `post` written while it holds. A request that omits `mediaType` takes it; one that names the other is refused with `ts-bad-media-type` (James chose refusal over accepting either). `text/x-wiki` is therefore no longer import-only, which amends A26's list of import-only fields; `heading` and `imported_from` stay import-only. **Signatures.** When an original `create`, `post` or `edit` is written, a signature at the very end of its text (`~~~`, `~~~~` or `~~~~~`, with any whitespace and one `--`, `—` or `–` before it) is removed silently; tildes elsewhere are kept (James chose the trailing signature only). A wikitext post then receives the rest of the pre-save transform as a wikitext page does: `subst:`, `safesubst:` and signatures elsewhere in the text expand (James chose this over no transform). A markdown post receives nothing else. An original `create` or `edit` whose text is empty once its signature is removed is refused with `ts-empty-post`, and so is a `post` unless it sets a status or collapses or uncollapses a range (an implementation call for James's review). Imports keep their text verbatim. **UI** (§8): the composer shows the markup in use beneath its text area and removes the signature without comment. Chapter text: 14 §1.5, §1.6; 11 §1.2, §1.5; 13 §5.13 (0054 A7); 19 §6.1; 23 §3.2, §6 (`siprop=triplespace` reports it).

Replaced text: 14 §1.5, §1.6; 11 §1.2 and §1.5.

### A29. A talk page's revisions have content: its threads as wikitext

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-25
- **Change:** extends §7
- **Summary:** James chose, of three forms, a talk page's revision content as wikitext ("Talk page as wikitext"), and of two memberships, "attached now, as they stood". `rvprop=content` on a talk title gives the threads attached to it now, in the talk page's order, each as it stood after the revision (a thread not yet started is left out), written as a MediaWiki talk page: `== Subject ==` sections, posts indented with `:` by depth and ending in the signature `~~~~` would have written, posts in the markup they were written in, imported posts with their own signatures, heading posts as headings, `(comment removed)` and `(username removed)` for what the viewer may not see. Model `triplespace-talk`, format `text/x-wiki`, read-only. A thread that leaves takes its revisions with it, consistent with the composite history. Chapter text: 16 §6.1; 10 §4.3, §4.4 (0041 A17); `docs/registry/content-models.toml`.

### A30. A topic on a user talk page, by the user's name

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-25
- **Change:** extends §8, §9
- **Summary:** James asked for new topics on user talk pages in the site, with thread notifications (0021 A20). `POST /thread` takes an actor target by its account's name, `{kind: "actor", name}`, besides its key, resolved to a registered account or a bot on the tenant; a name none holds is `ts-no-target`, a 404. Chapter text: 18 §3.2; 19 §6.1.

### A31. A thread's actions from the page: the calls made building them

- **Date:** 2026-11-05
- **Source:** Direct: Claude, while building the thread actions (SCA-168), 2026-11-05; unconfirmed
- **Change:** amends §8; extends §9
- **Summary:** Folded into 19 §6.1 and 18 §3.2. The calls:
  - Every operation the records support besides posting is reached from the thread page's overflow menu, as `?action=thread&op={rename|move|attach|detach|pin|unpin|close|reopen}`: a form with a confirm button, which posts to the same address. The menu offers an item where the viewer's tenant-wide rights allow it (`move`, or `edit` for Close and Reopen) and the thread gives it something to do; the API decides the rest, page protection included, and a refusal comes back on the form.
  - Pin, Unpin and Remove a listing choose the attachment from a select, rather than a control beside each attachment on the thread page; a talk page's thread heads carry **Pin here** and **Unpin here**, which open the pin form with that page chosen and return to the talk page.
  - Move and List on take any title that leads to a talk page: the talk page, a board, or the subject itself. A title that cannot hold a thread is refused on the form.
  - **Close…** posts a status of the closed category (Resolved by default), with an optional closing statement; **Reopen…** posts `open`, with an optional comment. Both are posts, so a status remains a claim in the thread by a named actor.
  - The change routes (`rename`, `move`, `attach`, `detach`, `pin`, `unpin`) name a user talk page by its account, `{kind: "actor", name}`, as `POST /thread` does (A30), which is how the site's forms name one.

  Replaced text (19 §6.1, as it stood):

  > A thread page has **Pin here** and **Unpin** beside each attachment, for those who hold the rights ([0049](../decisions/0049-boards.md) §13).
  >
  > A thread page has **List on…** and, beside each listing, **Remove from here**, for those who hold the rights ([0049](../decisions/0049-boards.md) §13). Moving a thread offers "Keep listed on the old page".
