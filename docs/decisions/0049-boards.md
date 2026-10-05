# 0049. Boards; threads on several pages

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-04 (A3)
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

**A board is a page in the `Board` namespace (§2).** It has a page ID, a title chosen when it is created, a definition (§4) stored in its own `page` records, and a listing of the threads attached to it. It is what a talk page is, without a subject to discuss: when a thread attaches to a board, the target is `{kind: page, id: <the board's page ID>}` and the talk page is the board itself, so the record's `talk` field ([0019](0019-discussions.md) §4) holds the same page ID and nothing is minted.

**A board exists because someone created it.** A talk page exists when its subject does, and is given a page ID by the first thread attached to it. A board's title is free text, so if boards came into being on first use, a typo would make one. Creating a board is a page `create` in namespace 310, which needs `createpage` ([0016](0016-permissions-and-access-control.md) §2), as creating any page does. Attaching a thread to a board that does not exist, or is deleted, is refused.

**Everything else is what a document page does.** A board is renamed with a page `move` ([0008](0008-namespaces-and-document-pages.md) §4), and its threads follow because they are attached by page ID. It is protected and deleted with ACLs ([0023](0023-moderation.md)), with the effect on its threads given in §7. It carries page statements, as every document page does ([0038](0038-page-metadata-and-categories.md) §1), and they are the board's metadata, not its threads'.

**Where boards fit.** Boards are for village pumps, noticeboards, help desks, project chat and announcement channels. `Project talk:` stays what every talk namespace is: discussion of the page beside it. The importer for existing wikitext talk pages (0019 Q5) gains a natural destination: a village pump splits into threads homed on a board.

**No subpages.** `Board:Help/Images` would be a second way to express a hierarchy that scopes (§14) express better. Subpages are off, so that turning them on later breaks no title.

### 2. The namespaces, and talk namespaces that forward (amends 0008 §2, extends 0008 §3; amends 0019 §3)

| Number | Canonical name | Kind | Models | Notes |
|---|---|---|---|---|
| 310 | `Board` | `pages` | `triplespace-board` (§3) | Normalizer `first-letter`; case `first-letter`; no subpages; talk 311 |
| 311 | `Board talk` | `virtual` | — | Forwards to 310 |
| 215 | `Thread talk` | `virtual` *(was `reserved`)* | — | Forwards to 214 |

**310 and 311 are the first pair of the second Triplespace block** ([0008](0008-namespaces-and-document-pages.md) §2, by 0008 A17), since 210–219 is full.

**A forwarding namespace.** MediaWiki clients find a page's talk page at the next odd namespace number, so 311 has to be registered. It holds no pages. The registry gains a field, `forwards_to`, on namespaces of kind `virtual`: every title in such a namespace resolves to the title with the same text in the namespace it names, normalized by that namespace's rules. `Board talk:General chat` resolves to `Board:General chat`. This is a step of the one title resolver of [0008](0008-namespaces-and-document-pages.md) §3, the same step by which `Item:P31` resolves to `Property:P31`:

- A page view answers with HTTP 301. The mapping never changes, unlike a resolver's ([0029](0029-resolver-namespaces.md)), which is why it is permanent rather than 303.
- The Action API reports the title as resolving to the board, as it reports `Item:WDQ123` resolving to `Item:Q456`. `titles=Board talk:General chat&redirects` returns the board.
- The forwarding title has no page ID, no records and no history. An ACL or watch naming it is refused, since there is nothing to restrict or watch. A link to it is a link to the board.

The kind is `virtual` rather than `reserved` because, under 0008 §2's numbering policy, `reserved` means a number that has no meaning here. A forwarding namespace has one. `Media` (−2) is the precedent for a `virtual` namespace that resolves titles to something else.

**`Thread talk` forwards too.** 0019 §3 kept 215 `reserved` and empty "because clients assume every subject namespace has one", and a thread is discussed in itself. That reasoning holds, and forwarding serves it better than an empty namespace does: `Thread talk:2026-09-27/Why is P31 wrong here` now takes a client to the thread.

**A board and a thread are their own talk pages.** On a `Board` or `Thread` page, `prop=info` with `inprop=talkid|subjectid` returns the page's own ID for both. `Special:TalkPage/Board:General chat` goes to the board, and `Special:NewSection/Board:General chat` opens the board's new-thread form ([0047](0047-special-pages.md)). Neither page shows a Talk tab.

### 3. The `triplespace-board` content model (extends 0041 §3)

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `triplespace-board` | Triplespace | text | main | `application/json` | Yes | 310 |

It is a **text** model ([0041](0041-content-models.md) §5), as `triplespace-table` is ([0045](0045-table-content-model.md) §3). The definition is stored as text in the board's `page` records and can be changed with `action=edit`. The thread listing is generated, as a table's grid is.

| Operation (0041 §5) | For `triplespace-board` |
|---|---|
| Validate | Against the definition schema (§4). Content that fails is refused |
| Pre-save transform | Canonical JSON: keys in schema order, two-space indentation |
| Serialize | The canonical JSON |
| Plain text for search | Title, `description` and `header` |
| Diff | Line diff of the canonical JSON, with `header` split into its own lines so that an edit to the header diffs as markdown does |
| Render | `description`, then `header` rendered as markdown ([0019](0019-discussions.md) §5: wiki links, sanitized HTML), then the thread listing (§8) |

**A board's history** is its own revisions and the composite history of the threads attached to it ([0019](0019-discussions.md) §7, as §9 extends it), in one sequence ordered by time. `prop=revisions` on a board title returns both kinds of row, each with its own content model.

### 4. The definition

*Changed by A3.*

```json
{
  "version": 1,
  "description": "General discussion about this wiki that has no better home",
  "header": "Questions about one item belong on its talk page.\n\nPlease read [[Project:Etiquette]] before posting.",
  "sort": "attached",
  "homes": true
}
```

| Key | Required | Meaning |
|---|---|---|
| `version` | Yes | `1` |
| `description` | No | Plain text, one line, shown under the title and in lists of boards |
| `header` | No | Markdown shown above the listing. Its links become `page_link` rows from the board, as a post's do ([0019](0019-discussions.md) §5) |
| `sort` | No | `attached` (the default, §8) or `activity`. A viewer can switch without saving |
| `homes` | No | Default `true`. When `false`, the board is a **listing board**: threads may be listed on it (§5) but not started on it or moved to it. `Board:Requests for comment` is one: each request lives on the talk page it concerns and is listed here. A listing board shows no new-thread form |
| `scope` | No | Accepted at `version: 2` ([0060](0060-scopes.md) §6): a scope page or an inline kind of 0060 §4, selecting the threads attached to the talk pages of the scope's members, or one of this ADR's own kinds, `thread_statement` and `boards` (§14). Refused at version 1 |

Unknown keys are refused. Changing `homes` to `false` does not move threads already homed on the board. It stops new ones.

### 5. A thread has one home and any number of listings (amends 0019 §1–2)

**Every thread has exactly one home.** The home is what [0019](0019-discussions.md) §2 called the thread's attachment: `create` sets it and `move` changes it. A home is a `page` target (a subject's talk page, or a board whose `homes` is true) or an `actor` target (a user talk page).

**A thread may also have listings:** further talk pages and boards it appears on. A listing is always a `page` target. Home and listings together are the thread's **attachments**. A thread is never attached twice to the same talk page, and its home is never also one of its listings.

**0019 §2's "a thread cannot be attached to two pages; cross-posting is a link" is replaced.** Everything else in 0019 §2 stands: attachment is by identifier, talk titles resolve through their subjects, a talk page's ID is minted by the first record that attaches a thread to it (now including `attach`, §6), and foreign talk pages are never loaded.

**Why a home.** Four things in earlier ADRs depend on a thread having one attachment, and none of them makes sense spread across several:

| Depends on it | Where | With several equal attachments |
|---|---|---|
| A talk page encloses its threads | [0016](0016-permissions-and-access-control.md) §4 | Listing a thread on a protected board would lock it everywhere |
| Deleting a talk page deletes its threads | [0023](0023-moderation.md) §2, §4 | Deleting a board would delete a thread from `Item talk:Q42` too |
| A post's `as:context`, and the `Group` that publishes the thread | [0019](0019-discussions.md) §7, [0022](0022-federation.md) §7 | No single answer |
| `view.thread.talk_page_id` | [0019](0019-discussions.md) §11 | No single row |

So the home keeps every one of these (§7), and a listing adds only visibility.

**An actor target is home-only.** A user talk page can be a thread's home, as in 0019. It cannot be a listing, because every post in a thread attached to a user talk page sends that user a `talk` notification, with the banner [0021](0021-notifications.md) §6 keeps until it is read. Listing a busy thread there would turn a cross-post into a stream of them. Telling a person about a thread is what a mention does (0021 §2).

**A limit.** `thread.max_attachments`, a `site` setting (default 5), counts the home and the listings.

### 6. Records (amends 0019 §1 and §4)

Two operations join the `scatter:v0/thread` payload type:

| Operation | Meaning | Text part | Base offset |
|---|---|---|---|
| `attach` | Lists the thread on another talk page or board | — | Required |
| `detach` | Removes one listing | — | Required |

Both carry a summary in the comment part, as `move` does. **Content part fields**, beside those of 0019 §4:

| Field | Present on | Meaning |
|---|---|---|
| `also` | `create` (optional) | A list of further targets, each `{target, talk}`, listed at creation |
| `target`, `talk` | `attach`, `detach` | The target (a `page` target only) and its talk page ID, minted by an `attach` if the target has none |
| `keep` | `move` (optional) | `true` keeps the old home as a listing |

**Validation**, against the thread's state:

- An `attach` or an `also` entry is refused if the target does not exist or is deleted, is an `actor` target, is already attached, or would take the thread past `thread.max_attachments`.
- A `detach` is refused unless the target is a listing. The home is changed with `move`, never removed.
- A `create` or `move` whose home would be a board with `homes: false` is refused.
- A `move` to a page that is currently a listing promotes it: the listing is gone and that page is the home. With `keep`, the old home becomes a listing, and the move is refused if the old home is an `actor` target or the limit would be passed.

The fold of 0019 §1 now yields the home and the listing set, each with the revision ID of the record that attached it.

### 7. What the home decides, and what every attachment shares (amends 0016 §4, 0019 §12, 0023 §2 and §4; extends 0020 §2)

**The home alone:**

- **Encloses the thread.** 0016 §4's "a talk page encloses the threads attached to it" now reads **"the threads whose home it is"**. Protecting `Board:General chat` restricts posting in the threads homed there. It does not restrict posting in a thread homed on `Item talk:Q42` and listed on the board. A board's `edit` ACL does govern what can be attached to it or detached from it (§13).
- **Takes the thread with it when deleted.** Deleting a talk page or a board deletes the threads homed there, by enclosure, as [0023](0023-moderation.md) §4 says. A thread *listed* on a deleted page is untouched. The listing goes **dormant**: hidden from everyone outside the deletion group, and back if the page is undeleted. Nothing is written to the thread.
- **Is the thread's `as:context`** (§9) and the `Group` that announces it as its own (§10).
- **Is the only attachment that can be a user talk page**, so 0021's `talk` notification fires exactly as before.

**Every attachment alike:**

- **Listing.** The thread appears on the page, and in `GET /page/{id}/threads` (§11).
- **History.** A talk page's or board's composite history ([0019](0019-discussions.md) §7) includes the whole history of every thread attached to it, home or listing, and the attach and detach events. A detached thread takes its rows away, as a moved one does.
- **Feeds and watches.** [0020](0020-change-feeds.md) §2's one-target set for a talk page or board is the page and every thread attached to it. Watching `Board:Requests for comment` watches every request listed there, and with `notify` ([0021](0021-notifications.md) §2) sends a notification for each post in them.
- **Search.** A thread can be found by any of its attachments (§12).

**Status stays per thread.** A thread resolved on one page is resolved everywhere it appears. A page that no longer wants a thread detaches it.

**Tenancy is unchanged.** Attachments are tenant-local: a thread cannot be listed on another tenant's talk page. [0028](0028-tenancy-policy.md) §6's read-only view of a provider tenant's threads shows each thread once, under its home there.

### 8. Rendering and the UI (amends 0019 §8)

- **Order.** A talk page or board lists its threads in the order they were attached *to that page*, newest last, with the switch to last activity of 0019 §8. A thread is attached to its home when it is created or moved there, and to a listing when it is listed. This replaces 0019 §8's creation order, so a thread listed on a board today appears as new there rather than among threads from last year. For a thread that never moves and is never listed, the two orders agree.
- **Where a thread lives.** A thread's header shows its home first, marked as home, then its listings, as "Also on …" links. On a listing page, the summary in the listing says "Started on Item talk:Q42".
- **Attaching.** The new-thread form has an "Also post to…" field that takes titles of talk pages and boards, up to the limit. A thread page has **List on…** and, beside each listing, **Remove from here**, for those who hold the rights (§13). Moving a thread offers "Keep listed on the old page".
- **A board page** has the tabs Board, Definition (the source editor of [0034](0034-frontend-stack.md), as for a table) and History. It shows the new-thread form when `homes` is true.

### 9. Logs and RDF (extends 0011 §6.1 and §8; extends 0019 §7)

| Log type/action | Comes from | Visible to |
|---|---|---|
| `thread/attach` (new) | An `attach` record, and each entry in a `create` record's `also` | Everyone |
| `thread/detach` (new) | A `detach` record | Everyone |

Each event names the thread and the talk page or board, and appears in the logs of both. A `move` with `keep` is one `move/move` event with `keep` among its parameters.

**Types** ([0011](0011-logs.md) §8): `thread/attach` is `as:Add`, with `as:object` the thread and `as:target` the talk page or board. `thread/detach` is `as:Remove`, with `as:origin` the page it left. Activity Streams has exactly these terms.

**Nodes** ([0019](0019-discussions.md) §7):

- A thread's `as:context` stays its home. It gains `scatter:listedOn` for each listing.
- A talk page's `as:OrderedCollection` holds every thread attached to it, home or listing.
- A **board** is `{base}/page/{board page ID}`, an `as:OrderedCollection` of its threads with `as:name` the title and `as:summary` the description. It has no `scatter:subject`.

### 10. Federation (extends 0022 §6 and §7)

- **A board can be a `Group` actor.** `Board` (310) is a namespace like any other in `federation.talk_pages`. The handle of a board's `Group` is open (Open questions).
- **A listing announces too.** When a thread is listed on a page whose `Group` is federated, that `Group` `Announce`s the thread, and then every post in it, as the home's `Group` does. A `detach` sends `Undo` of that `Announce`. A follower of both `Group`s receives the same objects twice, and fediverse software collapses them by `id`. This is how Lemmy cross-posts reach their communities.
- **Inbound replies are governed by the home.** A remote reply to a post is a post in the thread, whichever `Group` it reached the follower through, so 0022 §8's per-namespace ACL is evaluated on the thread's home, by enclosure (§7).

### 11. API (extends 0019 §9)

| Route | Meaning |
|---|---|
| `POST /thread` | `create`, which now accepts `also` |
| `POST /thread/{page ID}/attach`, `POST /thread/{page ID}/detach` | `attach` and `detach`, each with a target and a base offset |
| `POST /thread/{page ID}/move` | Accepts `keep` |
| `GET /thread/{page ID}` | Gains `home` and `listings`, each with the talk page ID, title and time attached |
| `GET /page/{talk page or board ID}/threads` | Every attached thread, each with `home: true` or `false` |

A board's definition is read and written with the page routes of [0012](0012-api-requirements.md) §5, as any text page's is. In the Action API, `action=edit` on a `Board` title edits the definition (`contentmodel=triplespace-board`), `list=allpages&apnamespace=310` lists boards, and `action=edit` on a `Thread` title is still refused with `no-direct-editing`.

### 12. Storage, caches and search (extends 0019 §11)

```sql
CREATE TABLE view.thread_attachment (
  thread_id bigint NOT NULL REFERENCES view.thread,
  talk_page_id bigint NOT NULL,
  home boolean NOT NULL,
  attached timestamptz NOT NULL,
  attached_revid bigint NOT NULL,          -- the record that attached it here
  PRIMARY KEY (thread_id, talk_page_id)
);
CREATE INDEX thread_attachment_talk ON view.thread_attachment (talk_page_id, attached);
CREATE UNIQUE INDEX thread_attachment_home ON view.thread_attachment (thread_id) WHERE home;
```

- `view.thread.talk_page_id` stays, and is the home. The listing query reads `view.thread_attachment`, which replaces the `thread_talk` index.
- **A board has a `view.talk_page` row** from its creation, with `target_kind` `page` and `target_id` its own page ID, so every query that starts from a talk page works on boards.
- **Caches** ([0014](0014-caches-and-search.md) §7): any record on a thread purges the `tp:` listing of every page it is attached to, by tag.
- **Search**: a thread's document gains the IDs of all its attachments as a field, so a search can be limited to a board or talk page.

### 13. Permissions (extends 0019 §12)

| Action | Needs | Default groups |
|---|---|---|
| Create a board | `createpage` in 310 | `user`, `temp` |
| Edit a board's definition | `edit` on the board | `user`, `temp` |
| `create` with `also` | `edit` on the home and on each listed target | `user`, `temp` |
| `attach`, `detach` | `move` on the thread, and `edit` on the target | `autoconfirmed` |

`edit` on the target means that a board protected to `sysop` decides for itself what is listed on it and what is removed. A tenant that wants fewer boards restricts `createpage` in namespace 310 with an ACL ([0023](0023-moderation.md) §1).

### 14. Scopes, later

*Changed by A3.*

Automatic scopes are not part of this ADR, but one rule is fixed now so that they fit: **attachments are records, and scope membership is a projection.** This is the same split [0038](0038-page-metadata-and-categories.md) §2 makes between asserted and projected statements.

- A scoped board's threads are its attachments, plus the threads its scope selects, minus any its definition excludes.
- Selection by scope writes nothing to the thread. It does not enclose, it is not a `thread/attach` event, and it ends when the scope stops selecting the thread.
- The scope is the definition's `scope` key: an object holding exactly one scope kind, as a table's `rows` is ([0045](0045-table-content-model.md) §4). Version 1 refuses it; version 2 accepts it ([0060](0060-scopes.md) §6).

The kinds are those of [0060](0060-scopes.md) §4, each selecting the threads attached to the talk pages of the subjects it selects, and two of this ADR's own:

| Kind | Selects |
|---|---|
| `thread_statement` | Threads carrying a given statement ([0038](0038-page-metadata-and-categories.md) §9), such as a topic property bound to a role |
| `boards` | The threads of other boards, which is how boards nest |

`mentions`, once a candidate here, is withdrawn in favour of 0060 Q2's `links` kind. `thread_statement` is where manual and automatic meet: a person states a thread's topic once, and every board scoped to that topic shows it.

### 15. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed. The table this section first gave is in A1.

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
- **Q2.** ~~**Scopes** (§14): which kinds come first, whether a scoped board's members are announced and notify watchers as listed ones do, and the bound on a scope's size for feeds (`feeds.related_limit`, [0020](0020-change-feeds.md) §2).~~ *Settled by [0060](0060-scopes.md) §6: the kinds are 0060 §4's plus `thread_statement` and `boards`, and the bound is `scopes.max_members`; the announcement question is Q7.*
- **Q3. Pinned threads.** Whether a board's definition may name threads to show first, as an announcements board needs.
- **Q4. Detaching one's own thread.** Whether a thread's author may remove their thread from a listing without `move`.
- **Q5. Board moderators.** Whether a board needs a group of its own beyond what ACLs on it express.
- **Q6. DiscussionTools** (0019 Q4): which page a `discussiontoolspageinfo` call on a listing page reports for a listed thread.
- **Q7.** (Rest of Q2.) Whether a scoped board's members are announced and notify watchers as listed ones do.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §15 | extends | 0005 A49 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §2 | amends | 0008 A19 |
| [0008](0008-namespaces-and-document-pages.md) §1, §3 | §2 | extends | 0008 A19 |
| [0011](0011-logs.md) §6.1, §8 | §9 | extends | 0011 A16 |
| [0016](0016-permissions-and-access-control.md) §4 | §7 | amends | 0016 A17 |
| [0019](0019-discussions.md) §1, §2, §3, §7, §8, §12 | §2, §5–9, §11–13 | amends | 0019 A10 |
| [0019](0019-discussions.md) §4, §9, §11 | §2, §5–9, §11–13 | extends | 0019 A10 |
| [0020](0020-change-feeds.md) §2 | §7 | amends | 0020 A9 |
| [0022](0022-federation.md) §6, §7 | §10 | extends | 0022 A3 |
| [0023](0023-moderation.md) §2, §4 | §7 | amends | 0023 A7 |
| [0038](0038-page-metadata-and-categories.md) §1 | §1 | extends | 0038 A5 |
| [0041](0041-content-models.md) §3 | §3 | extends | 0041 A5 |

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

Replaced text (§15):

> No new crate.
>
> | Crate | Change |
> |---|---|
> | `scatter-threads` | `attach`, `detach`, `also` and `keep`; the fold to a home and a listing set; their validation (§6). The `triplespace-board` model: definition types, schema validation and canonical serialization (§3–4). Pure |
> | `scatter-pages` | The `triplespace-board` registry entry |
> | `triplespace-titles` | Namespaces 310 and 311; `forwards_to` in the resolver; 215 changes kind (§2) |
> | `triplespace-projections` | `view.thread_attachment`; `view.talk_page` rows for boards; `page_link` rows from board headers; `thread/*` log events |
> | `triplespace-rdf` | The nodes and event types of §9 |
> | `triplespace-federation` | Announces from listings, and `Undo` on detach (§10) |
> | `triplespace-search` | The attachments field (§12) |
> | `triplespace-api-action`, `triplespace-api-rest` | §11 |

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

Replaced text (§14, in part):

> Version 1 refuses it. A version that accepts it raises `version`.

Replaced text (§4):

> | `scope` | — | Reserved for automatic scopes (§14). A definition carrying it is refused in version 1 |

Replaced text (§14):

> Candidate kinds, roughly cheapest first:
>
> | Kind | Selects |
> |---|---|
> | `subjects` | Threads attached to the talk pages of entities with a given statement (`P31` = `Q5`), from the statement indexes |
> | `statement` | Threads carrying a given statement ([0038](0038-page-metadata-and-categories.md) §9), such as a topic property bound to a role |
> | `mentions` | Threads whose posts link to matching entities, from `page_link` |
> | `boards` | The threads of other boards, which is how boards nest |
>
> `statement` is where manual and automatic meet: a person states a thread's topic once, and every board scoped to that topic shows it.
