# 0069. Synchronized talk pages; pinned threads

- **Status:** Proposed
- **Date:** 2026-10-06
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md), [0022](0022-federation.md), [0030](0030-edit-filters.md), [0049](0049-boards.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [0067](0067-proposals.md), [0068](0068-merging-with-upstream.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0027](0027-preferences-and-portability.md), [0038](0038-page-metadata-and-categories.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0047](0047-special-pages.md), [MediaWiki API contract](../api/mediawiki-compat.md)

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

**A page repository gains a field, `talk`:**

| `talk` | Behaviour |
|---|---|
| `link` *(default)* | As before: a foreign page's talk page holds local threads, and links to the repository's talk page ([0052](0052-page-repositories-and-title-inheritance.md) §7) |
| `sync` | The repository's talk page for each served subject namespace is **followed**: its sections appear on the local talk page as **foreign threads** (§2), beside local ones (§3) |

The talk namespaces followed are the pairs of the entry's `namespaces`: `Talk` for `main`, `Template talk` for `Template`, and so on. A talk namespace is not listed in `namespaces` itself, because it is never served as pages: its pages are only ever shown as threads.

**A followed talk page is always mirrored,** whatever the repository's `mode`. Threads need identities that last (§2), which a proxied bundle cannot give, so the talk page's bundle is a `scatter:v0/mirrored-page` record in `pages/{repo}` ([0053](0053-mirrored-pages.md) §5) even where the articles are proxied. Which talk pages are mirrored follows `mirror.set = on-demand`: a talk page is mirrored the first time its local talk page is read, and also whenever something depends on it (a fork that follows it, §7; a watch, §8; a post sent upstream, §5), and it is kept while any of these holds or for `mirror.grace` after the last read. It follows the repository's events like any mirrored page ([0053](0053-mirrored-pages.md) §6), under the same debounce.

**Archive subpages are followed lazily.** `Talk:X/Archive 7` is mirrored when a thread is found to have moved there (§2) and when a reader opens the talk page's archived threads.

### 2. Foreign threads (extends 0053 §1 and §5)

**A foreign thread is one level-two section of a followed talk page,** with section 0, the text above the first heading, as one more (§4). Its structure comes from the repository:

| Repository | Sections | Comments |
|---|---|---|
| Has DiscussionTools (every Wikimedia wiki) | `action=discussiontoolspageinfo&prop=threaditemshtml` at the bundle's revision: each heading with its **name** (`h-Example-20190412103000-…`) and each signed comment with its name, author, timestamp and parent | Yes: the thread is a tree of comments, as on the wiki |
| Does not | 0054 §5's splitter: level-two headings, by position | No: the section is one block, replied to at its end |

DiscussionTools' names are what make the identity last. A heading's name is built from its first comment's author and timestamp, not its text or position, so it survives a retitled heading, sections above it being added or removed, and **an archive bot moving it to a subpage**: DiscussionTools' own permalinks rely on this.

**Each foreign thread gets a number,** `n`, allocated per repository by the sync job the first time it sees the thread's name, and a **provider-ranged page ID** ([0052](0052-page-repositories-and-title-inheritance.md) §6) in the half of the range that upstream page IDs never reach:

> foreign thread page ID = `number << 40 | 1 << 39 | n`

so a foreign thread can be watched, linked with `Special:Redirect/page/{id}`, counted and searched like a local one, and its ID is the same on every tenant that lists the repository. A repository without DiscussionTools names its sections by position within a revision; the sync job carries `n` forward across revisions by matching heading text and position, and a section it cannot match is a new thread. Those identities are best effort, and the UI says so on such a repository.

**The mirrored-page record carries the mapping, never the names.** The talk page's `put` gains, in its content part, `threads`: for each section, `n`, the SHA-256 of the DiscussionTools name truncated to 16 bytes, the section index, and for each comment the hash of its name and of its parent's. The comment authors, resolved under the repository's issuer ([0007](0007-actor-identity.md) §5), go in the **attestation part** beside the revision's actor, so upstream user-hiding erases them with it. The heading text and the comments' text are read from the wikitext and HTML parts, so erasing those parts erases them. A rebuild reproduces every `n` from the records, and a hash, not a name, keys the lookup, because names contain usernames ([0006](0006-log-integrity-and-erasure.md) §3).

**State, followed from upstream:**

| State | When | Shown as |
|---|---|---|
| `open` | The section is on the talk page | Status **Open** |
| `archived` | It left the talk page and was found, by name, on an archive subpage the same actor edited within `talk.archive_window` (default ten minutes) of removing it, which is how archive bots and one-click archivers work | Status **Archived** ([0019](0019-discussions.md) §6), collapsed by the default rule |
| `removed` | It left the talk page and was not found | Hidden by default, with "Removed from the talk page on {date}" and a link to the last upstream revision that had it |

A removed thread that reappears, by name, is `open` again. Status here is upstream's: a foreign thread's status changes only when upstream does, and nobody sets one locally.

**Rendering.** A foreign thread is rendered from the talk page's HTML part, section by section, as [0053](0053-mirrored-pages.md) §2 rewrites it: upstream's signatures, indentation and templates as upstream renders them, inside the local thread frame, with the origin chip of [0052](0052-page-repositories-and-title-inheritance.md) §5 ("On English Wikipedia") and, where DiscussionTools marks the comments, a **Reply** control on each (§5). It has no `Thread:` title, since its title belongs upstream: its address is the local talk title with DiscussionTools' anchor, `Talk:Myocardial infarction#h-Example-20190412103000`, which resolves through the stack, and `GET /thread/{id}` serves it read-only with `origin`.

### 3. The talk page holds both (amends 0019 §2; extends 0019 §8)

**A talk page on a followed subject is the union** of the local threads attached to it ([0019](0019-discussions.md) §2, [0049](0049-boards.md) §5) and the foreign threads of the upstream talk page it follows:

| Local talk page | Follows |
|---|---|
| A foreign page's, on its ranged page ID ([0052](0052-page-repositories-and-title-inheritance.md) §7) | The repository's talk page for that page, under `talk = sync` |
| A fork's, following upstream (§7) | The repository's talk page for the page it was forked from, by upstream page ID, so a local move of the fork changes nothing |
| Any other | Nothing |

**0019 §2's "foreign talk pages are never loaded" now reads as it was meant:** a mirrored *entity's* talk page holds local threads only, and a provider tenant's threads are never imported ([0028](0028-tenancy-policy.md) §6 stands). A followed *page* repository's talk page is shown, because a page repository is something the tenant chose to present as its own reading, and its discussion is part of that page.

**Order.** Pinned threads first (§4), then every other thread in one sequence: a local thread by the time it was attached here ([0049](0049-boards.md) §8), a foreign thread by its first comment's timestamp, or, without DiscussionTools, the time the sync first saw it. A **filter** above the list offers All, Here and On {repository}. Each thread shows its origin; local threads carry no chip, as local pages carry none.

**Visibility** ([0019](0019-discussions.md) §6) applies to both, by status category and age, so an archived Wikipedia section collapses as a resolved local thread does.

### 4. Pinned threads (settles 0049 Q3; extends 0019 §1, §4, §8; extends 0049 §6, §8, §12, §13)

**A thread may be pinned on any page it is attached to,** its home or a listing, separately on each. Pinning is a property of the attachment, not the thread: a request for comment pinned on `Board:Requests for comment` is not pinned on the item's talk page where it lives.

**Records.** Two operations join `scatter:v0/thread`:

| Operation | Meaning | Text part | Base offset |
|---|---|---|---|
| `pin` | Pins the thread on one of its attachments | — | Required |
| `unpin` | Unpins it there | — | Required |

Each carries `target` and `talk` in the content part, as `attach` does, and a summary in the comment part. A `create` may carry **`pinned: true`**, pinning the thread on its home from the start, which is how an import writes front matter (§7). A `pin` on a page the thread is not attached to, or past `thread.max_pinned` (`site`, default 3) pinned threads on that page, is refused. Detaching or moving a thread away from a page ends its pin there; a `move` with `keep` keeps it. The fold yields, for each attachment, the time it was pinned or none.

**Rendering.** Pinned threads come first on the page, in the order they were pinned, marked with a pin, and are **exempt from the age rule** of [0019](0019-discussions.md) §6, so an announcement does not collapse for being old. The status rule still applies: a pinned thread that is resolved collapses, which is how a pinned call for comments ends without being unpinned.

**Logs, RDF and federation.** `pin` and `unpin` project as **`thread/pin`** and **`thread/unpin`** ([0011](0011-logs.md) §6.1), typed `as:Add` and `as:Remove` with `as:target` the page's **featured** collection, `{base}/page/{talk page ID}/featured`; the thread node gains `scatter:pinnedOn`. A federated talk page or board ([0022](0022-federation.md) §6) publishes the same collection as its `featured` property, which is how Mastodon and Lemmy expose pinned posts.

**Permissions** are those of `attach` ([0049](0049-boards.md) §13): `move` on the thread and `edit` on the page, default `autoconfirmed`, so a board protected to `sysop` decides its own pins.

**Talk pages get no front matter of their own.** A board keeps the `header` of its definition ([0049](0049-boards.md) §4), which describes the board. A talk page gains no header, definition or banner slot: what Wikipedia's banners say about the subject (WikiProject scope and assessment, "this article is about a living person", former nominations) is subject metadata, whose place is page statements ([0038](0038-page-metadata-and-categories.md) §1), and what remains (a talk-page FAQ, a standing notice) is a pinned thread.

**Foreign section 0 is pinned by construction.** The front matter of a followed talk page is a foreign thread (§2) with `n` like any other, pinned on the local talk page, rendered from section 0's HTML so its banners render as upstream's do, and with Reply controls only on the comments DiscussionTools finds in it (people do sign comments above the first heading). Editing the banners themselves is **Edit on {repository}**, a link to upstream's section 0 editor; it is not done through the instance (Q5). Local pins cannot be placed on foreign threads (Q2).

### 5. Replying upstream (extends 0067 §6; extends 0030 §2)

**A person holding an upstream grant for the repository can reply to a foreign thread as though on the wiki.** The grant is [0067](0067-proposals.md) §6's: a second authorization, separate from login, stored in `private.upstream_grant`, through the consumer the operator registered with the repository. Posting needs the `editpage` grant, which 0067 §6 already requests. This section is **the first use of that client**, so it is built with this ADR, not in 0067's later phase; `proposals.push` keeps its own switch.

- **A Wikimedia login is not a grant,** as 0067 §6 decided, but it makes one a click: the authorization page is on the same identity issuer the person just used ([0007](0007-actor-identity.md) §3), already signed in. A person who logs in by password, or another issuer, can still connect an account on the repository the same way. The first Reply offers "Connect your English Wikipedia account to reply", and the composer then shows **"Posting as Example on English Wikipedia"**. The grant names the upstream account it acts as; a person may hold one per wiki.
- **Registered local accounts only.** A temporary account cannot hold a grant.

**The reply is written in wikitext,** the repository's language, in a composer that shows a preview rendered by the repository (`action=discussiontoolspreview`, or `action=parse` with the talk title as context), counted in the `upstream` rate class ([0024](0024-subsidiary-accounts.md) §5). A local reply is markdown ([0019](0019-discussions.md) §5); an upstream reply is not stored here as a post at all (below), so no conversion is needed or attempted.

**Sending.** The instance calls, as the person:

| Repository | Reply to a comment | Reply to a section with no comments, or on a repository without DiscussionTools |
|---|---|---|
| DiscussionTools | `action=discussiontoolsedit&paction=addcomment&commentname={name}&wikitext=…`, which indents and signs as DiscussionTools does on the wiki | `paction=addcomment` with the heading's name |
| No DiscussionTools | — | `action=edit&section={index}&appendtext=` the text indented one level after the section's last line, with `~~~~` and `baserevid` the bundle's revision, after checking the section's heading at that index still matches |

Upstream decides everything upstream decides: protection, blocks, rate limits, CAPTCHAs, AbuseFilter. An upstream warning (an AbuseFilter `warn`, a CAPTCHA) is shown in the composer and the person may confirm and resend; a refusal is shown with upstream's message. The edit summary is DiscussionTools' own, and the repository tags the edit with the consumer's tag, which is how a Wikimedia wiki marks edits made through an OAuth tool.

**Local rules apply first.** The person must be able to post on the local talk page: `edit` on it, not blocked, the page not protected against them ([0016](0016-permissions-and-access-control.md) §4). **Edit filters** ([0030](0030-edit-filters.md)) see the outbound text in the text context with `action = upstream-post` and the variable `upstream_repo`, so a tenant can refuse to be a conduit for something its own wiki would refuse; a `disallow` stops the send and writes the filter-hit record as for any write.

**Nothing is written to the log for the send.** The reply is the person's speech on the repository, attributed there to their account there. It comes back here by syndication, in the talk page's next `put`, as an upstream revision by an upstream actor, which is exactly what it is. A local copy would be a second, divergent source of the same speech. What the instance keeps is operational: a row in **`ops.upstream_post`** (§10) that tracks the send from `queued` to `sent`, with the upstream revision ID, to `synced` when a `put` at or after that revision lands, or `failed`. After a successful send the instance queues a fetch of the talk page at once rather than waiting for the event stream. Until it is synced, the reply shows **to its author only**, in place, marked "Sent to English Wikipedia; appearing here shortly". The text is dropped from the row when it is synced or has failed for seven days; the row itself, linking the local account to the upstream revision, is kept for `talk.upstream_audit_days` (default 90) for abuse handling, as IP addresses are kept ([0007](0007-actor-identity.md) §3), and never shown publicly.

**Attribution follows.** The synced comment is by the upstream actor. When the person has linked that account publicly ([0007](0007-actor-identity.md) §7), it carries "also {local name} here", as every linked account's contributions do; when they have not, nothing connects the two in public, and the grant does not change that.

**Replies to foreign threads go upstream or nowhere.** A foreign thread holds upstream's comments only; a local reply inside it would make a thread that is half here and half there, which neither side could read whole. A person without a grant sees **Discuss here instead**, which opens the new-thread form (§6) set to Here, quoting the thread's link (Q1).

### 6. Starting a thread: here or upstream

**The new-thread form on a talk page that follows upstream has a destination:**

| Destination | Writes | Composer |
|---|---|---|
| **Here only** | A local thread homed on this talk page: a `create` record ([0019](0019-discussions.md) §1), with listings as usual | Markdown |
| **{Repository}** | A new section on the repository's talk page, by `action=discussiontoolsedit&paction=addtopic&sectiontitle=…&wikitext=…` or `action=edit&section=new` with `~~~~` on a repository without DiscussionTools, as the person, under §5's rules | Wikitext |

The destination is named in the form's submit button ("Post to English Wikipedia"), never implied. **`talk.new_thread_default`** (`site`) chooses which is selected when the form opens, `here` by default, and a viewer's choice is remembered in the preference `threads.destination` ([0027](0027-preferences-and-portability.md) §1). Upstream is offered only to a person who holds a grant, or offers to connect one. A thread sent upstream is a foreign thread once synced, with the same `ops.upstream_post` lifecycle as a reply; a thread kept here is local and stays local (Q3).

### 7. Forks: following the talk page or forking it (amends 0054 §1, §5; extends 0008 §4; settles 0068 Q2)

**When a page is forked, the person chooses what happens to its talk page.** The fork notice of [0054](0054-forking-a-mirrored-page.md) §2 gains one choice:

| Choice | The fork's talk page |
|---|---|
| **Keep following** the talk page on {repository} *(selected by default)* | Follows the upstream talk page (§3): upstream's sections appear as foreign threads, replies and new sections can go upstream, and local threads sit beside them. Nothing is converted |
| **Fork the talk page** | 0054 §5's conversion: upstream's sections and archives become closed local threads, and nothing more arrives from upstream |

**Following is the default because it is the reversible choice.** A fork that follows can stop at any time, and stopping performs the conversion then; a talk page forked at the start cannot be un-forked without leaving the converted threads beside the foreign ones they duplicate. A tenant that only ever forks permanently, as MDWiki does, sets **`fork.talk`** (`site`: `follow` *(default)* or `fork`) to choose the default, and the choice is still offered. On a repository with `talk = link`, only **Fork the talk page** is offered, as before. `Special:Fork` and `tsfork=1` take `tstalk=follow|fork`.

**The choice is recorded on the fork.** 0054 §1's `forked_from` gains `talk: "follow" | "fork"`. A later change is a new page operation, **`follow`**, in `scatter:v0/page` ([0008](0008-namespaces-and-document-pages.md) §4): no text part, a content part `{talk: "follow" | "fork"}`, a summary, and the base offset of the page's latest record. It is a **null revision**, as MediaWiki writes for protection: a history row ("Stopped following the talk page on English Wikipedia") with the text unchanged. It needs `edit` on the fork. The fork's About panel ([0054](0054-forking-a-mirrored-page.md) §6) shows the state with **Stop following** or **Follow again**.

- **Stopping** appends `follow` with `fork` and runs 0054 §5's conversion as a job, from the talk page as it then stands and its archives: each section becomes a closed local thread homed on the fork's talk page, with `imported_from` now also carrying the section's name hash; foreign front matter becomes a local thread created **pinned** (§4) and `open`, not archived, since banners do not go stale as discussions do. Local threads are untouched.
- **Following again** appends `follow` with `follow`. Foreign threads whose name hash matches an imported thread's `imported_from` are not shown, since their content is already here; newer ones are.

**0054 §5 is amended to match:** the converted front matter thread is created pinned and open, in both the immediate and the later conversion; and the conversion runs at fork time only when the person chose **Fork the talk page**.

**0068 Q2 is settled by following.** A fork that follows its upstream talk page has every new section as it is written, so a pull ([0068](0068-merging-with-upstream.md) §2) has nothing to import; a fork that forked its talk page chose not to have them, and **Follow again** is the way back.

**Proposals meet discussion.** A page proposal whose destination is `talk` ([0068](0068-merging-with-upstream.md) §3) is a new upstream section; on a fork that follows, it is sent by §6's mechanism and appears among the foreign threads when synced, where the discussion of the proposal happens on the repository and is read here.

### 8. Feeds, watches and notifications (extends 0020 §2; extends 0021 §2)

- **One target.** [0020](0020-change-feeds.md) §2's one-target set for a talk page that follows upstream includes the `put` records of the followed talk page, each a row "{n} upstream revisions to Talk:…" naming the threads it changed (the `put`'s content part lists them). A foreign thread is its own target: the `put` rows that changed it. The `syncs` filter's defaults apply: on for watchlists and histories, off for recent changes.
- **Watching** a foreign page's title, a fork that follows, or a foreign thread keeps its talk page mirrored (§1).
- **Notifications.** `watch` with `notify` ([0021](0021-notifications.md) §2) fires on these rows as on any. **`reply`** is extended: a synced comment whose parent comment is by the upstream account a person's grant acts as addresses that person, so a reply on Wikipedia to something posted through the instance reaches the poster's bell here, as it reaches their Echo there. Sending a reply or a new section through the instance auto-watches the foreign thread with `notify` when the person's auto-watch preference for threads they post in is on.
- **Federation.** A talk page's `Group` ([0022](0022-federation.md) §7) announces local threads only. Foreign threads are the repository's speech, not the tenant's to publish, and are never announced, in the same way that entity data never travels on ActivityPub.

### 9. API (extends 0012 §5)

| Route | Meaning |
|---|---|
| `GET /page/{talk page ID}/threads` | Gains foreign threads, each with `origin` `{repo, n, state, anchor}` and no title; filter `origin=local|upstream` |
| `GET /thread/{id}` | For a foreign thread, read-only: the origin, state, section, comment tree (names omitted; hashes and DiscussionTools anchors given) and the upstream revision |
| `POST /thread/{id}/upstream-reply` | `{parent, wikitext}`, where `parent` is a comment anchor or the thread; answers 202 with an upstream-post ID |
| `POST /page/{talk page ID}/upstream-topic` | `{subject, wikitext}`; 202 with an upstream-post ID |
| `GET /upstream-post/{id}` | The send's state and upstream revision ID; visible to its author |
| `POST /thread/{id}/pin`, `POST /thread/{id}/unpin` | `{target, base}` (§4) |
| `POST /page/{fork ID}/talk-follow` | `{talk: follow|fork, base}` (§7) |

**Action API.** `action=discussiontoolsedit` with `paction=addcomment` or `addtopic` on a followed talk title is the same operation as the two upstream routes, so DiscussionTools-based clients (the reply tool, Convenient Discussions) work on a followed talk page as on the wiki; `discussiontoolspageinfo` on that title returns the foreign threads' items as upstream does, with local threads omitted (0019 Q4 is otherwise unchanged). `action=edit` on a followed talk title is refused with `ts-upstream-talk`, which names the routes. `list=threads` accepts `thorigin`.

### 10. Storage (extends 0013 §5.6)

```sql
CREATE TABLE view.foreign_thread (               -- instance scope; projection of the followed talk pages' puts (§2)
  repo text NOT NULL, n bigint NOT NULL,           -- page ID = provider number << 40 | 1 << 39 | n
  name_hash bytea NOT NULL,                        -- 16 bytes of SHA-256 of DiscussionTools' heading name
  talk_upstream_page_id bigint NOT NULL,           -- the talk page it belongs to
  at_upstream_page_id bigint NOT NULL,             -- where it is now: the talk page or an archive subpage
  section integer NOT NULL, pinned boolean NOT NULL DEFAULT false,   -- section 0 is pinned
  state text NOT NULL,                             -- open | archived | removed
  first_comment timestamptz, last_comment timestamptz, comments integer NOT NULL DEFAULT 0,
  partition bigint NOT NULL, "offset" bigint NOT NULL,              -- the put that last changed it
  PRIMARY KEY (repo, n)
);
CREATE UNIQUE INDEX foreign_thread_name ON view.foreign_thread (repo, name_hash);
CREATE INDEX foreign_thread_talk ON view.foreign_thread (repo, talk_upstream_page_id, first_comment);

CREATE TABLE ops.upstream_post (                 -- tenant scope; the send of §5–6
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  actor_key text NOT NULL, repo text NOT NULL, upstream_user_id bigint NOT NULL,
  talk_upstream_page_id bigint NOT NULL, thread_n bigint, parent_anchor text,  -- NULL thread_n: a new section
  subject text, wikitext text,                     -- dropped once synced, or seven days after failing
  state text NOT NULL,                             -- queued | sent | synced | failed
  upstream_revid bigint, error jsonb,
  created timestamptz NOT NULL, updated timestamptz NOT NULL
);

ALTER TABLE view.thread_attachment ADD COLUMN pinned timestamptz;  -- §4
```

The talk listing reads `view.thread_attachment` and `view.foreign_thread` and merges them by time. **Caches** ([0014](0014-caches-and-search.md) §7): a `put` of a followed talk page purges the `tp:` listing of every local talk page that follows it, by tag. **Search**: foreign threads are documents in the repository's `pages-{repo}` index ([0053](0053-mirrored-pages.md) §7) with the thread fields of [0019](0019-discussions.md) §11, so a talk-page search covers both.

### 11. Settings and permissions

| Setting | Scope | Default | Meaning |
|---|---|---|---|
| `talk` on a `page-repo` | config record | `link` | §1 |
| `talk.archive_window` | instance | 10 minutes | §2 |
| `talk.upstream_post` | `site` | `on` where any repository has `talk = sync` | Whether replies and new sections may be sent upstream at all (§5–6) |
| `talk.new_thread_default` | `site` | `here` | §6 |
| `talk.upstream_audit_days` | `site` | 90 | §5 |
| `thread.max_pinned` | `site` | 3 | §4 |
| `fork.talk` | `site` | `follow` | §7 |

| Action | Needs | Default groups |
|---|---|---|
| Reply upstream, start an upstream thread | `edit` on the local talk page; a grant for the repository; `talk.upstream_post` on | `user` |
| `pin`, `unpin` | `move` on the thread and `edit` on the page | `autoconfirmed` |
| `follow` on a fork | `edit` on the fork | `user`, `temp` |

### 12. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-threads` | `pin`, `unpin` and `pinned` on `create`; the pinned time per attachment in the fold; `imported_from`'s name hash |
| `scatter-pages` | `talk` in `forked_from`; the `follow` operation |
| `scatter-adapter-mediawiki` | The `discussiontoolspageinfo` reader and its thread-item tree; name hashing; the heading-position matcher for repositories without DiscussionTools; the archive search of §2 |
| `triplespace-repos` | Following talk pages: their mirroring and `put`s with `threads`, `n` allocation, state, `view.foreign_thread`; the talk-follow choice at fork time and the stop-following conversion job |
| `triplespace-upstream` | Replies and new sections as the person (`discussiontoolsedit`, `action=edit`), previews, warnings and errors, `ops.upstream_post` |
| `triplespace-accounts` | The upstream grant flow of [0067](0067-proposals.md) §6, built now |
| `triplespace-projections` | `pinned` in `view.thread_attachment`; `thread/pin` and `thread/unpin` events; one-target rows for followed talk pages |
| `triplespace-notify` | `reply` for synced comments under a grant's upstream account |
| `triplespace-federation` | The `featured` collection; foreign threads never announced |
| `triplespace-search` | Foreign threads in `pages-{repo}` |
| `triplespace-api-action`, `triplespace-api-rest` | §9 |
| `triplespace-ui` | The merged talk page, origin filter, destination choice, wikitext composer with upstream preview, pins, the fork notice's talk choice and About panel controls |

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
