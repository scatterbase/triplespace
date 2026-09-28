# 0019. Discussions

- **Status:** Proposed
- **Date:** 2026-09-27
- **Author:** James Hare / Claude Fable
- **Amended by:** [0020 — Change feeds](0020-change-feeds.md) (§3 settles §8's watching), [0021 — Notifications](0021-notifications.md) (settles the notifications open question), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§4 amends §1: thread `delete` and `undelete` are ACL records; §6 makes posts patrollable), [0027 — Preferences, private state and portability](0027-preferences-and-portability.md) (§1 makes the visibility rule of §6 the `threads.visibility` preference), [0028 — Tenancy policy](0028-tenancy-policy.md) (§6 settles the cross-tenant discussion open question). §6 was amended on 2026-09-27 to name the default statuses and their registry file
- **Related:** [0001 — Revision metadata in RDF](0001-revision-metadata-rdf.md) (§7 extends §1 and §6), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§13 amends §2), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (§1 uses §8; §4 uses §7), [0007 — Actor identity](0007-actor-identity.md) (§2, §4), [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md) (§2 amends §2; §3 extends §1 and §3; §5 extends §5, §8 and §10; settles the discussions open question), [0010 — Site UI](0010-site-ui.md) (§8), [0011 — Upstream and local logs](0011-logs.md) (§7 extends §6.1 and §8), [0012 — API requirements for the site UI](0012-api-requirements.md) (§7 uses §3; §9 extends §4 and §5), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§11 extends §5), [0014 — Cache layers and search](0014-caches-and-search.md) (§11), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (§4 amends §1; §2 uses §2), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§12 extends §2 and amends §4), [0018 — Tenants](0018-tenants.md) (§2), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§6–8 use §7 and §10: threads and posts as ActivityPub objects; inbound replies as posts)

## Context

[0008](0008-namespaces-and-document-pages.md) §2 reserved a talk namespace beside every subject namespace and left discussions to their own ADR. This is that ADR.

The design principle of 0008 is that a namespace's kind says what a page is made of. `Item:` and `Property:` are composite views over the resolved graph; `Project:` and `User:` are blobs of stored text. A talk page is a third thing: **a composite view over threads**. Each thread is its own page with its own history. A post refers to the post it answers. A thread is attached to one talk page and can be moved to another. The talk page's history is the histories of its threads, presented as one.

MediaWiki has tried this shape twice. **LiquidThreads** put each thread in a `Thread:` namespace and moved threads between talk pages. **Flow** (Structured Discussions) put each topic on a `Topic:` page, made the talk page a board of topics, and let topics move between boards. Both are gone, and the reasons are design inputs here: topics had opaque UUID titles and URLs nobody could cite; the board's history was hard to read; watching a board and watching a topic did not compose; and because the content was not text, the rest of the wiki's tooling (history, diffs, contributions, backlinks, search) treated it as foreign. **DiscussionTools**, which is what Wikimedia wikis use now, keeps talk pages as wikitext and overlays structure by parsing signatures and indentation. That is not available here: Triplespace has no wikitext parser beyond the subset of 0008 §8, and it does not want its discussions to depend on `~~~~`.

Two goals from the earlier ADRs constrain the design. Every unit of contribution has to be a record with a global revision ID ([0015](0015-record-format-and-partition-registry.md) §2), so that history, contributions, recent changes, diffs, permalinks, hiding and erasure all work without anything new. And the instance loads *data*, not the discussion behind it: foreign talk pages are never mirrored.

[0011](0011-logs.md) §8 already types local log events with Activity Streams 2.0. Posts and threads are described with the same vocabulary.

## Decision

### 1. A thread is a page; a post is a record

**A thread is a page in the `Thread` namespace (§3).** It has a page ID from the tenant's sequence ([0015](0015-record-format-and-partition-registry.md) §2, [0018](0018-tenants.md) §2), a subject, an attachment (§2), a status (§6) and a history. Its records live in the tenant's `pages` partition, keyed by the thread's page ID, with the payload type `scatter:v0/thread` (§4).

**A post is one record.** Its identity is its global revision ID, header field 7. The reply structure is a graph of record references: a reply carries the revision ID of the post it answers. Nothing is nested and nothing is rewritten when a reply arrives. The thread's current state, its tree, its subject, its attachment and its status, is a fold over its records, as an entity is a fold over its change sets.

**Operations.** Each record carries one:

| Operation | Meaning | Text part | Base offset |
|---|---|---|---|
| `create` | Mints the thread's page ID and title, attaches it to a talk page, and is the opening post | The opening post | — |
| `post` | A post in the thread, top-level or in reply to another post | The post | — |
| `edit` | Replaces the **complete text** of one post | The new text | Required |
| `rename` | Changes the subject, and so the title (§3) | — | Required |
| `move` | Attaches the thread to a different talk page (§2) | — | Required |
| `delete` | Hides the thread and every post in it from everyone but administrators, as 0008 §4 hides a page | — | Required |
| `undelete` | Reverses a `delete` | — | Required |

> **Amended by [0023](0023-moderation.md) §4.** `delete` and `undelete` are no longer thread operations. Deleting a thread is a `read` ACL on its page ID, written to the tenant `log` partition, and undeletion retires it; hiding one post is a `record` ACL on that post's record with `parts: [text]`. The `thread` payload type carries `create`, `post`, `edit`, `rename` and `move`, and the sentence below about "any operation on a deleted thread other than `undelete`" reads as "any operation on a deleted thread".

**Full text per post, as 0008 §4 stores full text per revision.** A post's edit history is the chain of `edit` records naming its revision ID, and the diff between two versions is computed when it is read.

**Replies need no base offset.** Two people answering the same post at once both succeed. `edit`, `rename`, `move`, `delete` and `undelete` carry the base offset of [0006](0006-log-integrity-and-erasure.md) §8, and a mismatch is an `editconflict`. A `post` whose parent is not a post of the same thread, and any operation on a deleted thread other than `undelete`, is rejected.

**The true parent is always stored.** The instance's indentation limit (§8) is a rendering rule, not a constraint on the data. Changing the limit later rewrites nothing.

### 2. Talk pages are composite; attachment is by identifier (amends 0008 §2)

**A thread is attached to exactly one target.** The target is named by identifier, never by title, so that a thread follows its subject through a rename or a move and the record's key is never content ([0006](0006-log-integrity-and-erasure.md) §3):

| Target kind | Identifier | Talk page |
|---|---|---|
| `page` | A page ID: an entity's ([0015](0015-record-format-and-partition-registry.md) §2, carried forward, so a mirrored entity has one) or a document page's | The subject namespace's paired talk namespace |
| `actor` | A local actor key ([0007](0007-actor-identity.md) §1) | `User talk` |

`User talk` attaches to the actor rather than to the user page because MediaWiki lets a user's talk page exist when the user page does not. The creation rule is that of 0008 §6: the account must be local, registered and not vanished.

**Talk namespaces change kind from `reserved` to `composite`.** The talk page for a target is the ordered set of threads currently attached to it. It has no stored text and no records of its own. A talk title resolves through its subject: `Item talk:Q42` resolves `Item:Q42` with the title resolver of 0008 §3, following aliases and clusters, so `Item talk:WDQ123` redirects to `Item talk:Q456` exactly as the subject does; `User talk:Example` resolves the username to the actor key. The talk page of a subject that does not exist does not exist either.

**A talk page has a page ID** so that `prop=info`, `action=watch` and the activity row's `target` can name it. It is minted from the page-ID sequence by the first `create` or `move` record that attaches a thread to that target, and that record carries it in its content part (§4), so a rebuild reads it rather than deriving it. The appending transaction takes the same lock that allocates offsets ([0013](0013-postgres-storage.md) §2), so two threads created at once on a fresh target agree on it.

**A `move` moves the whole thread.** Its posts leave the old talk page's history and join the new one's, and one `move/move` log event, with the source and target talk pages as parameters, appears in both pages' logs (§7). A thread cannot be attached to two pages; cross-posting is a link.

**Foreign talk pages are never loaded.** The talk page of a mirrored entity holds local threads only. They live in the tenant's `pages` partition, which is not what a tenant exposes when it is a provider to others ([0018](0018-tenants.md) §5), so a local historical society's discussion of a Librarybase item stays on the society's wiki. The talk page of a mirrored entity offers a link to the upstream talk page for readers who want the provider's own discussion.

### 3. The `Thread` namespace and thread titles (extends 0008 §1 and §3)

**`Thread` is a registered namespace of a new kind, `thread`:** pages composed from thread records. Its paired `Thread talk` is `reserved` and empty, because clients assume every subject namespace has one. Subpages are not allowed. The numbers are 214 and 215, in the Triplespace range of [0008](0008-namespaces-and-document-pages.md) §2 as amended on 2026-09-27; LiquidThreads' 90 and 91 are not reused.

**A thread's title is its creation date and its subject:** `Thread:2026-09-27/Why is P31 wrong here`. The date is the UTC date of the `create` record and never changes. The subject is what the author typed, normalized as a `first-letter` title with MediaWiki's forbidden characters (`# < > [ ] | { }`) rejected. Two threads created on the same date with the same subject are told apart with a MediaWiki-style suffix: `…/Why is P31 wrong here (2)`. The `create` and `rename` records carry the minted title, as a page `move` carries its new title, so the title index is a projection and a rebuild reproduces it.

This is what Flow lacked. The title says what the thread is about and when it began, it can be typed, and it never depends on which talk page the thread is attached to, so a `move` changes nothing about how the thread is cited. A `rename` does: the old title stops resolving, as 0008 §6 rules for user pages, because subjects can contain usernames. Stable references are `Special:Redirect/page/{page ID}` for a thread and `Special:PermanentLink/{revid}` for a post (§8).

### 4. The record: four parts (amends 0015 §1)

The payload type `scatter:v0/thread` is added to the `pages` partition's list ([registry](../registry/graphs.toml)). It declares **four** body parts. The first three keep the meaning and index that [0015](0015-record-format-and-partition-registry.md) §1 gives every Triplespace payload type; the fourth is new:

| # | Part | Holds | RevisionDelete bit |
|---|---|---|---|
| 0 | **Content** | The operation and its structure (below). Never the post's text | — |
| 1 | **Comment** | The summary for `edit`, `rename`, `move`, `delete` and `undelete`; null for `create` and `post`, whose text *is* the contribution | comment |
| 2 | **Attestation** | As 0015 §1 | user |
| 3 | **Text** | The post's markdown source, for `create`, `post` and `edit`; null otherwise | text |

**Why a fourth part.** Erasure is per part. When a post's text has to be erased, the thread must keep its shape: the erased post still has a parent, and its replies still hang from it. If the parent pointer shared a part with the text, erasing the text would erase the tree. So the structure and the text are separate parts, and `erase` ([0006](0006-log-integrity-and-erasure.md) §7, 0015 §1) accepts `text` in its `parts` field for this payload type. An erased post renders as a tombstone in its place (§7). The content hash `0x03` is computed over part 0 as before; it identifies the operation, not the prose.

0015 §1 therefore now reads: every Triplespace payload type has *at least* three parts, in that order, and a type may declare more. `verify` already checks *n* parts.

**The content part** is a CBOR map using Activity Streams property names where AS2 has them (§10):

| Field | Present on | Meaning |
|---|---|---|
| `op` | all | The operation |
| `name` | `create`, `rename` | The subject |
| `title` | `create`, `rename` | The minted title (§3) |
| `target` | `create`, `move` | `{kind, id}` (§2) |
| `talk` | `create`, `move` | The talk page's page ID, minted by this record if the target had none |
| `inReplyTo` | `post` | The parent post's revision ID, or null for a top-level post |
| `object` | `edit` | The revision ID of the post whose text this replaces |
| `status` | `post` | A status value (§6), when the post sets one |

Mentions, links and the rendered HTML are never stored; they are derived from the text part (§5). Nothing in the content part names the record's own ID, which is assigned at append and read from the header.

### 5. Text: markdown, mentions and links (extends 0008 §5, §8 and §10)

**Posts are markdown**, rendered by the `markdown` content model of 0008 §5, with two additions that apply to that model everywhere:

- **Wiki links.** `[[Title]]`, `[[Title|text]]` and `[[Title#Section]]` are resolved by the title resolver of 0008 §3, as in the wikitext subset. `[[Q42]]` renders with the entity's label, as Wikibase does. This is the mention syntax; it is what every editor already knows.
- **Raw HTML is sanitized** to the allow-list of 0008 §8. CommonMark passes HTML through; the renderer does not.

`~~~~` is not expanded. A post is attributed by its attestation, and the signature has nothing to add.

**Rendered HTML is a projection,** produced when the post is read and cached by version ([0014](0014-caches-and-search.md)), never stored.

**Links become `page_link` rows** with `from_page` the thread's page ID. "What links here" on `Item:Q42` therefore lists the threads that discuss it, alongside the project pages that link to it, and `list=backlinks` needs nothing new. Erasing a post's text removes its rows.

**A size limit** on the text part is site configuration.

### 6. Status and visibility

**A thread's status is set by posting.** A `post` may carry a `status` value. The thread's status is the value of its latest post that carries one; a thread with none is `open`. Posting `open` reopens a thread. The values are registry data, so an instance can add its own; the defaults are `open`, `resolved`, `rejected` and `stale`. A status is therefore a claim made in public, in the thread, by a named actor, and disputing it is another post. Anyone who may post may set one.

> **Amended 2026-09-27.** Statuses are `config` records of kind `thread-status` in the tenant `config` ([0015](0015-record-format-and-partition-registry.md) §3), each with a label, a **category** (`open` or `closed`) and a display order; the defaults ship in `docs/registry/thread-statuses.toml` and follow the labels of bug trackers and project tools: **Open**, **In progress** and **Blocked** (open), and **Resolved**, **Won't fix**, **Declined** and **Stale** (closed). `rejected` is renamed `declined`. The category, not the name, drives the default visibility rule below (closed threads collapse) and the `thread-status` notification of [0021](0021-notifications.md) §2, which fires when a thread moves from an open to a closed status. `open` is required and cannot be retired.

**Visibility is a viewer preference, not data.** Each thread on a talk page is shown *visible*, *collapsed* or *hidden*, chosen by a rule over its status and the age of its last post. The instance sets the defaults (for example: `resolved`, `rejected` and `stale` collapsed; anything quiet for ninety days collapsed; nothing hidden), and a viewer overrides them. The server renders the threads; the UI applies the rule; so the shared caches hold one public form ([0014](0014-caches-and-search.md) §1) and no viewer's choice reaches them.

**There are no archive pages.** `/Archive 1` exists on MediaWiki because a talk page is a text file that grows. A talk page here is a query, and an old thread is simply collapsed.

### 7. History, activity, logs and RDF (extends 0001 §1 and §6, 0011 §6.1 and §8)

**Every thread record is an activity row** ([0012](0012-api-requirements.md) §3) of kind `edit` on the thread's page ID, with the operation in the parsed summary. A thread's history is its rows. `Special:Contributions` and `list=recentchanges` show posts as edits to `Thread:` pages, with the talk page in the row's `target`.

**A talk page's history is composite:** the rows of every thread currently attached to it, plus the attach and move events, in one sequence ordered by time. It is served by `GET /page/{id}/history` for the talk page's ID and by `prop=revisions` on the talk title, where each row is a revision with its own global revision ID. A thread that moves away takes its rows with it.

**Log events**, projected from the records as 0011 §6.1 requires: `create` → `create/create`; `rename` and `move` → `move/move`, with the old and new title or the source and target talk pages as parameters; `delete` and `undelete` → `delete/delete` and `delete/restore`. `post` and `edit` are revisions, not log events.

**Hiding and erasure** are the existing mechanisms. `deleterevision` hides a post's text, comment or actor; `erase` with `parts: [text]` removes the text for good. In either case the post stays in the tree as a tombstone: its place, its replies and its timestamp remain, its text does not.

**RDF**, in the metadata graph only, using the AS2 profile of §10:

- **A thread** is `{base}/page/{page ID}`, an `as:OrderedCollection` of its posts, with `as:name` the subject, `as:context` the talk page's node, `as:published`, and `scatter:status`.
- **A post** is the record's IRI `{base}/record/{partition}/{offset}`, an `as:Note` with `as:inReplyTo`, `as:context` (the thread), `as:attributedTo` (omitted when hidden, with `scatter:attributionHidden` as [0007](0007-actor-identity.md) §9), `as:published`, `as:updated` when edited, `as:content` as HTML with `as:mediaType "text/html"`, `as:source` with the markdown and `as:mediaType "text/markdown"`, and `as:tag` nodes of type `as:Mention` for each resolved wiki link. A hidden or erased post is an `as:Tombstone` with `as:formerType as:Note`.
- **A talk page** is `{base}/page/{talk page ID}`, an `as:OrderedCollection` of its threads, with `scatter:subject` the document node it discusses.
- **Events** are typed as 0011 §8 types them: `as:Create`, `as:Move`, `as:Delete`, `as:Undo`.

The post's revision node of [0001](0001-revision-metadata-rdf.md) §1 is the same IRI; the AS2 properties are added to it. Nothing about threads enters the main or resolved graph.

### 8. Rendering and the UI (extends 0010)

- **A thread page** shows its posts as a tree, indented to the instance's `thread.max_depth`. A post deeper than that is shown at the limit with a "replying to" link to its parent; its stored parent is unaffected (§1). Each post has an anchor `#post-{revid}`, a permalink (`Special:PermanentLink/{revid}`), reply, edit and history controls, and the hiding controls of 0010 for those who hold the rights.
- **A talk page** lists its threads, by default in creation order with the newest last, as MediaWiki readers expect, with a switch to order by last activity. Each thread shows its subject, status, participant count, last activity, and its posts or a collapsed summary per §6. A "new thread" form creates one attached to this page.
- **Subject pages** get the talk tab of 0010, with the thread count.
- **Watching** a subject watches its talk page and the threads attached to it; a thread can be watched on its own. The watchlist is [0020](0020-change-feeds.md).
- **Editing another actor's post** is permitted only with `ts-editpost` (§12) and always shows an "edited by" note, because a post is speech.

### 9. API (extends 0012 §4 and §5)

**REST**, under `rest.php/triplespace/v0`, with the post shape of §10:

| Route | Meaning |
|---|---|
| `GET /thread/{page ID}` | The thread: subject, target, talk page, status, and its posts as `as:Note` objects in tree order |
| `POST /thread` | `create` |
| `POST /thread/{page ID}/post` | `post`, with `inReplyTo` and optional `status` |
| `PUT /thread/{page ID}/post/{revid}` | `edit` |
| `POST /thread/{page ID}/rename`, `/move`, `/delete`, `/undelete` | The remaining operations, each with a base offset |
| `GET /page/{talk page ID}/threads` | The attached threads with status and last activity, without their posts |
| `GET /page/{id}/history` | Unchanged; composite for a talk page ID (§7) |

**Action API**, additively under 0012 §1. Reading works as for any page: `prop=revisions`, `prop=info`, `action=parse`, `list=recentchanges`, `list=usercontribs`, `list=backlinks`, `action=watch`. `action=delete` and `action=undelete` on a `Thread:` title work. `action=edit` on a `Thread:` or talk title is refused with a new error code, `ts-structured`, so that a bot learns at once that these pages are not text. DiscussionTools' `action=discussiontoolsedit` (`paction=addtopic|addcomment`) can be supported later on the same records, with `commentid` values of the form `c-{revid}` (open questions).

### 10. The Activity Streams profile

Posts, threads and talk pages are described with the Activity Streams 2.0 vocabulary because 0011 §8 already uses it for events and because it is the natural JSON shape for a post. It is used as a **fixed profile**, as Mastodon uses it: the REST API and the RDF export emit these properties and no others, the OpenAPI document of 0012 §1 pins the shape, and nothing performs JSON-LD expansion. The rules:

- `id` is assembled from the header (the record IRI) and never appears in the content part.
- `attributedTo` and `published` come from the attestation part and the header, never from the content part.
- `content` is HTML with `mediaType` explicit; `source` carries the markdown. AS2's default of HTML for `content` is never relied on.
- `inReplyTo` and `context` are IRIs of records and pages of this tenant.
- `tag` holds `Mention` objects with `href` and `name`, derived from the text.
- `status` is `scatter:status`, a `scatter:` extension in the manner of 0011 §8, since AS2 has no term for it.

**Federation is out of scope.** This ADR adopts the vocabulary, not the protocol. Nothing here prevents an inbox and outbox later, and nothing here requires them.

### 11. Storage, caches and search (extends 0013 §5 and 0014)

Threads and talk pages get rows in `view.page` (namespace, title, content model `thread` or `talk`, latest offset and revision ID, deleted, generation), so the title index and `page_link` cover them without change. Beside it:

```sql
CREATE TABLE view.thread (
  page_id bigint PRIMARY KEY REFERENCES view.page,
  target_kind smallint NOT NULL, target_id text NOT NULL, talk_page_id bigint NOT NULL,
  subject text NOT NULL, status text NOT NULL DEFAULT 'open',
  created timestamptz NOT NULL, last_post timestamptz NOT NULL, post_count integer NOT NULL
);
CREATE INDEX thread_talk ON view.thread (talk_page_id, created);
CREATE TABLE view.post (
  revid bigint PRIMARY KEY, thread_id bigint NOT NULL REFERENCES view.thread,
  parent_revid bigint, actor text, published timestamptz NOT NULL, updated timestamptz,
  latest_offset bigint NOT NULL,           -- the record holding the current text
  hidden smallint NOT NULL DEFAULT 0, erased boolean NOT NULL DEFAULT false
);
CREATE INDEX post_thread ON view.post (thread_id, published);
CREATE TABLE view.talk_page (
  page_id bigint PRIMARY KEY REFERENCES view.page,
  target_kind smallint NOT NULL, target_id text NOT NULL, UNIQUE (target_kind, target_id)
);
```

These are projections in the sense of 0013 §7, applied synchronously for interactive writes. **Caches** follow 0014: a rendered post is keyed by (revid, latest offset, generation); a thread by (page ID, latest revision ID, generation); a talk page listing by the same for its talk page ID; erasure bumps the thread's generation and purges by tag. **Search** indexes each thread as one document in the `pages` index, subject plus post text, with its namespace; hidden and erased text is not indexed.

### 12. Permissions (extends 0016 §2; amends 0016 §4)

| Permission | Governs | Default groups |
|---|---|---|
| `edit` | `create`, `post` (including a status), and `edit` of one's own posts | `user`, `temp` |
| `ts-editpost` *(new)* | `edit` of another actor's post | `sysop` |
| `move` | `rename` and `move` | `autoconfirmed` |
| `delete`, `undelete` | `delete`, `undelete` | `sysop` |
| `deleterevision` | Hiding a post's text, comment or actor | `sysop` |
| `ts-erase` | Erasing a post's text | `suppress` |

**Enclosure gains one rule:** a talk page encloses the threads attached to it. Protecting `Item talk:Q42` with `acl:page:{talk page ID}` therefore restricts every thread on it, and a thread moved elsewhere leaves that protection behind. Namespace ACLs on talk namespaces apply as they do to any namespace.

### 13. Crates (amends 0005 §2)

| Layer | Crate | Contents | Depends on |
|---|---|---|---|
| Substrate | `scatter-threads` | The `scatter:v0/thread` payload type and its four parts; the fold from records to thread state (tree, subject, attachment, status); title minting (§3); validation of operations against thread state. Pure | `scatter-log`, `scatter-pages` |
| | `scatter-markdown` | Gains wiki links and the HTML sanitizer (§5), for every use of the model | — |
| Triplespace | `triplespace-titles` | The `thread` and `composite` namespace kinds and talk-title resolution (§2–3) | as 0008 §11 |
| | `triplespace-projections` | `view.thread`, `view.post`, `view.talk_page`, and `page_link` rows from posts | as 0013 |
| | `triplespace-rdf` | The AS2 nodes of §7 | as 0013 |

## Consequences

- **A post is a revision, so everything that works on revisions works on posts.** History, diffs, contributions, recent changes, permalinks, hiding, erasure, inclusion proofs and export bundles need no discussion-specific code. This is the property Flow and LiquidThreads lacked.
- **Talk pages are not text.** Bots that edit, archive or template talk pages do not work; `action=edit` tells them so. Existing wikitext talk pages cannot be imported as they are (open questions).
- **The three-part uniformity of 0015 is loosened.** One payload type has four parts. Verification, erasure and the storage schema were already written for *n* parts, so the cost is one sentence and one more `parts` value.
- **Thread names are readable and stable across moves, and renames do not redirect.** A link to a renamed thread breaks, as a link to a renamed user page does. Permalinks by revision or page ID do not.
- **Status is social.** Anyone may mark a thread resolved, and anyone may reopen it, in public, with attribution. The instance decides only what the marks are called and how the UI folds them.
- **No archives, no `/Archive N` subpages, no lost history.** Old threads are collapsed, not moved.
- **Discussion stays local.** A mirrored entity's talk page is this wiki's; it is never fed upstream or to another tenant, and no upstream discussion is fetched.
- **The AS2 vocabulary costs nothing now and keeps federation possible later.**

## Open questions

- ~~**Namespace numbers** for `Thread` and `Thread talk` (registry README, pending allocations).~~ *Settled: 214/215 ([0008](0008-namespaces-and-document-pages.md) §2, as amended).*
- **Temporary accounts.** Whether a `temp` account gets a `User talk` page for messages, as MediaWiki gives one. [0021](0021-notifications.md) asks the same of their notifications.
- ~~**Notifications**: whether `[[User:Example]]` in a post notifies that user, and what a reply to one's post delivers. Watching is settled by [0020](0020-change-feeds.md).~~ *Settled by [0021](0021-notifications.md) §2: `mention`, `reply`, `talk` and `thread-status` reasons.*
- **DiscussionTools compatibility.** Whether to implement `discussiontoolsedit` and `discussiontoolspageinfo` on these records, and how far Convenient Discussions and similar scripts can be made to work.
- **Importing existing talk pages.** A Wikibase's `Item_talk:` and a project's `Project_talk:` pages are wikitext. One option is a job that splits a page by `==` heading into threads with one post each, attributed to the section's signatures where they parse and to an `imported` surrogate otherwise.
- ~~**Cross-tenant discussion.** Whether a tenant may show, read-only, another tenant's threads about an entity it mirrors from that tenant.~~ *Settled by [0028](0028-tenancy-policy.md) §6: by policy, read-only.*
- ~~**Federation.** Inbox, outbox and actor keys, if ever.~~ *Settled by [0022](0022-federation.md) Part B: opt-in `Person` actors, talk pages as `Group` actors that `Announce` threads, inbound replies as posts by `federated` surrogates; the inbound protocol details stay open there.*
- ~~**Limits.** The post size limit, the depth default, and rate limits on posting.~~ *Settled as to where they live: the size limit and `thread.max_depth` are `site` settings (§5, §8); posting is rate-limited in the `edit` and `create` classes of [0024](0024-subsidiary-accounts.md) §5. The values are tuning.*

## References

- [Extension:LiquidThreads](https://www.mediawiki.org/wiki/Extension:LiquidThreads) and [Extension:StructuredDiscussions](https://www.mediawiki.org/wiki/Extension:StructuredDiscussions) (Flow)
- [Extension:DiscussionTools](https://www.mediawiki.org/wiki/Extension:DiscussionTools) and [API:Discussiontoolsedit](https://www.mediawiki.org/wiki/Extension:DiscussionTools/API)
- [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces)
- [Activity Streams 2.0](https://www.w3.org/TR/activitystreams-core/) and the [Activity Vocabulary](https://www.w3.org/TR/activitystreams-vocabulary/)
- [ActivityPub](https://www.w3.org/TR/activitypub/)
- [CommonMark](https://commonmark.org/)
