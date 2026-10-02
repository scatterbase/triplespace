# 0019. Discussions

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-01 (A13)
- **Author:** James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0018](0018-tenants.md), [0022](0022-federation.md)

## Context

[0008](0008-namespaces-and-document-pages.md) §2 reserved a talk namespace beside every subject namespace and left discussions to their own ADR. This is that ADR.

The design principle of 0008 is that a namespace's kind says what a page is made of. `Item:` and `Property:` are composite views over the resolved graph; `Project:` and `User:` are blobs of stored text. A talk page is a third thing: **a composite view over threads**. Each thread is its own page with its own history. A post refers to the post it answers. A thread is attached to one talk page and can be moved to another. The talk page's history is the histories of its threads, presented as one.

MediaWiki has tried this shape twice. **LiquidThreads** put each thread in a `Thread:` namespace and moved threads between talk pages. **Flow** (Structured Discussions) put each topic on a `Topic:` page, made the talk page a board of topics, and let topics move between boards. Both are gone, and the reasons are design inputs here: topics had opaque UUID titles and URLs nobody could cite; the board's history was hard to read; watching a board and watching a topic did not compose; and because the content was not text, the rest of the wiki's tooling (history, diffs, contributions, backlinks, search) treated it as foreign. **DiscussionTools**, which is what Wikimedia wikis use now, keeps talk pages as wikitext and overlays structure by parsing signatures and indentation. That is not available here: Triplespace has no wikitext parser beyond the subset of 0008 §8, and it does not want its discussions to depend on `~~~~`.

Two goals from the earlier ADRs constrain the design. Every unit of contribution has to be a record with a global revision ID ([0015](0015-record-format-and-partition-registry.md) §2), so that history, contributions, recent changes, diffs, permalinks, hiding and erasure all work without anything new. And the instance loads *data*, not the discussion behind it: foreign talk pages are never mirrored.

[0011](0011-logs.md) §8 already types local log events with Activity Streams 2.0. Posts and threads are described with the same vocabulary.

## Decision

### 1. A thread is a page; a post is a record

*Changed by A4, A10.*

**A thread is a page in the `Thread` namespace (§3).** It has a page ID from the tenant's sequence ([0015](0015-record-format-and-partition-registry.md) §2, [0018](0018-tenants.md) §2), a subject, a home and any listings (§2), a status (§6) and a history. Its records live in the tenant's `pages` partition, keyed by the thread's page ID, with the payload type `scatter:v0/thread` (§4).

**A post is one record.** Its identity is its global revision ID, header field 7. The reply structure is a graph of record references: a reply carries the revision ID of the post it answers. Nothing is nested and nothing is rewritten when a reply arrives. The thread's current state, its tree, its subject, its attachment and its status, is a fold over its records, as an entity is a fold over its change sets.

**Operations.** Each record carries one:

| Operation | Meaning | Text part | Base offset |
|---|---|---|---|
| `create` | Mints the thread's page ID and title, attaches it to a talk page, its home, and is the opening post; may carry `also`, further targets listed at creation | The opening post | — |
| `post` | A post in the thread, top-level or in reply to another post | The post | — |
| `edit` | Replaces the **complete text** of one post | The new text | Required |
| `rename` | Changes the subject, and so the title (§3) | — | Required |
| `move` | Attaches the thread to a different home (§2); with `keep`, the old home stays as a listing | — | Required |
| `attach` | Lists the thread on a further talk page or board ([0049](0049-boards.md) §6) | — | Required |
| `detach` | Removes a listing ([0049](0049-boards.md) §6) | — | Required |

Deleting a thread is not an operation: it is a `read` ACL on its page ID, written to the tenant `log` partition, and undeletion retires it; hiding one post is a `record` ACL on that post's record with `parts: [text]` ([0023](0023-moderation.md) §4).

**Full text per post, as 0008 §4 stores full text per revision.** A post's edit history is the chain of `edit` records naming its revision ID, and the diff between two versions is computed when it is read.

**Replies need no base offset.** Two people answering the same post at once both succeed. `edit`, `rename`, `move`, `attach` and `detach` carry the base offset of [0006](0006-log-integrity-and-erasure.md) §8, and a mismatch is an `editconflict`. A `post` whose parent is not a post of the same thread, and any operation on a deleted thread, is rejected.

**The true parent is always stored.** The instance's indentation limit (§8) is a rendering rule, not a constraint on the data. Changing the limit later rewrites nothing.

### 2. Talk pages are composite; attachment is by identifier (amends 0008 §2)

*Changed by A6, A9, A10, A12.*

**A thread has exactly one home, and may have listings.** The home is the attachment `create` sets and `move` changes; listings are further talk pages and boards the thread appears on, up to `thread.max_attachments` ([0049](0049-boards.md) §5). Only the home encloses the thread, is its `as:context`, and may be a user talk page; listing, composite history and watches treat every attachment alike ([0049](0049-boards.md) §7). A board ([0049](0049-boards.md) §1) is a `page` target whose talk page is itself. A target is named by identifier, never by title, so that a thread follows its subject through a rename or a move and the record's key is never content ([0006](0006-log-integrity-and-erasure.md) §3):

| Target kind | Identifier | Talk page |
|---|---|---|
| `page` | A page ID: an entity's ([0015](0015-record-format-and-partition-registry.md) §2, carried forward, so a mirrored entity has one), a document page's, or the provider-ranged page ID of a page a repository serves ([0052](0052-page-repositories-and-title-inheritance.md) §6–7) | The subject namespace's paired talk namespace |
| `actor` | A local actor key ([0007](0007-actor-identity.md) §1) | `User talk` |

`User talk` attaches to the actor rather than to the user page because MediaWiki lets a user's talk page exist when the user page does not. The creation rule is that of 0008 §6: the account must be local, registered and not vanished.

**Talk namespaces are `pages` namespaces whose model is `triplespace-talk`,** a model whose source is *composite* ([0041](0041-content-models.md) §4). The talk page for a target is the ordered set of threads currently attached to it. It has no stored text and no records of its own. A talk title resolves through its subject: `Item talk:Q42` resolves `Item:Q42` with the title resolver of 0008 §3, following aliases and clusters, so `Item talk:WDQ123` redirects to `Item talk:Q456` exactly as the subject does; `User talk:Example` resolves the username to the actor key. The talk page of a subject that does not exist does not exist either.

**A talk page has a page ID** so that `prop=info`, `action=watch` and the activity row's `target` can name it. It is minted from the page-ID sequence by the first `create`, `move` or `attach` record that attaches a thread to that target, and that record carries it in its content part (§4), so a rebuild reads it rather than deriving it. The appending transaction takes the same lock that allocates offsets ([0013](0013-postgres-storage.md) §2), so two threads created at once on a fresh target agree on it.

**A `move` moves the whole thread.** Its posts leave the old home's history and join the new one's, and one `move/move` log event, with the source and target talk pages as parameters, appears in both pages' logs (§7). A thread is listed on further pages with `attach`, never by a second home.

**Foreign talk pages are never loaded.** The talk page of a mirrored entity holds local threads only. They live in the tenant's `pages` partition, which is not what a tenant exposes when it is a provider to others ([0018](0018-tenants.md) §5), so a local historical society's discussion of a Librarybase item stays on the society's wiki. The talk page of a mirrored entity offers a link to the upstream talk page for readers who want the provider's own discussion. A page inherited from a page repository has a local talk page the same way, on its ranged page ID, and when the page is forked the fork job moves its threads to the fork's talk page ([0054](0054-forking-a-mirrored-page.md) §5).

### 3. The `Thread` namespace and thread titles (extends 0008 §1 and §3)

*Changed by A9, A10, A13.*

**`Thread` is a `pages` namespace whose model is `triplespace-thread`** ([0041](0041-content-models.md) §4): pages composed from thread records. Its paired `Thread talk` is a `virtual` namespace that forwards to `Thread`: `Thread talk:X` resolves to `Thread:X`, since a thread is its own talk page ([0049](0049-boards.md) §2). Subpages are not allowed. The numbers are 214 and 215, in the Triplespace range of [0008](0008-namespaces-and-document-pages.md) §2; LiquidThreads' 90 and 91 are not reused.

**A thread's title is its creation date and its subject:** `Thread:2026-09-27/Why is P31 wrong here`. The date is the UTC date of the `create` record, or, for a thread an import creates, the date the import supplies ([0054](0054-forking-a-mirrored-page.md) §5), and never changes. The subject is what the author typed, normalized as a `first-letter` title with MediaWiki's forbidden characters (`# < > [ ] | { }`) rejected. Two threads created on the same date with the same subject are told apart with a MediaWiki-style suffix: `…/Why is P31 wrong here (2)`. The `create` and `rename` records carry the minted title, as a page `move` carries its new title, so the title index is a projection and a rebuild reproduces it.

This is what Flow lacked. The title says what the thread is about and when it began, it can be typed, and it never depends on which talk page the thread is attached to, so a `move` changes nothing about how the thread is cited. A `rename` does: the old title stops resolving, as 0008 §6 rules for user pages, because subjects can contain usernames. Stable references are `Special:Redirect/page/{page ID}` for a thread and `Special:PermanentLink/{revid}` for a post (§8).

### 4. The record: four parts (amends 0015 §1)

*Changed by A10, A13.*

The payload type `scatter:v0/thread` is added to the `pages` partition's list ([registry](../registry/graphs.toml)). It declares **four** body parts. The first three keep the meaning and index that [0015](0015-record-format-and-partition-registry.md) §1 gives every Triplespace payload type; the fourth is new:

| # | Part | Holds | RevisionDelete bit |
|---|---|---|---|
| 0 | **Content** | The operation and its structure (below). Never the post's text | — |
| 1 | **Comment** | The summary for `edit`, `rename`, `move`, `attach` and `detach`; null for `create` and `post`, whose text *is* the contribution | comment |
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
| `status` | `post`, `create` | A status value (§6), when the post sets one; on `create`, a thread that arrives already closed, as an import's does ([0054](0054-forking-a-mirrored-page.md) §5) |
| `mediaType` | `create`, `post`, `edit` (optional) | The text part's media type: `text/markdown` by default, `text/x-wiki` for a post an import writes ([0054](0054-forking-a-mirrored-page.md) §5) |
| `imported_from` | `create` (optional) | The repository, talk page, revision and archive subpage an imported thread came from ([0054](0054-forking-a-mirrored-page.md) §5) |
| `also` | `create` (optional) | Further targets listed at creation, each `{target, talk}` ([0049](0049-boards.md) §6) |
| `target`, `talk` | `attach`, `detach` | The target (a `page` target only) and its talk page ID, minted by an `attach` if the target had none ([0049](0049-boards.md) §6) |
| `keep` | `move` (optional) | `true` keeps the old home as a listing ([0049](0049-boards.md) §6) |

Mentions, links and the rendered HTML are never stored; they are derived from the text part (§5). Nothing in the content part names the record's own ID, which is assigned at append and read from the header.

### 5. Text: markdown, mentions and links (extends 0008 §5, §8 and §10)

*Changed by A8, A13.*

**Posts are markdown** by default, rendered by the `markdown` content model of 0008 §5, with two additions that apply to that model everywhere; a post whose `mediaType` is `text/x-wiki`, which only an import writes, is rendered by the wikitext pipeline instead, with expansion, so the banners and templates of an imported talk page render through the title stack ([0054](0054-forking-a-mirrored-page.md) §5):

- **Wiki links.** `[[Title]]`, `[[Title|text]]` and `[[Title#Section]]` are resolved by the title resolver of 0008 §3, as in the wikitext subset. An entity is mentioned with its namespace, `[[Item:Q42]]`, which renders with the entity's label, as Wikibase does; a bare `[[Q42]]` links to the main-namespace page of that title, in markdown as in wikitext ([0038](0038-page-metadata-and-categories.md) §8). This is the mention syntax; it is what every editor already knows.

- **Raw HTML is sanitized** to the allow-list of 0008 §8. CommonMark passes HTML through; the renderer does not.

`~~~~` is not expanded. A post is attributed by its attestation, and the signature has nothing to add.

**Rendered HTML is a projection,** produced when the post is read and cached by version ([0014](0014-caches-and-search.md)), never stored.

**Links become `page_link` rows** with `from_page` the thread's page ID. "What links here" on `Item:Q42` therefore lists the threads that discuss it, alongside the project pages that link to it, and `list=backlinks` needs nothing new. Erasing a post's text removes its rows.

**A size limit** on the text part is site configuration.

### 6. Status and visibility

*Changed by A3, A5, A7, A8, A13.*

**A thread's status is set by posting.** A `post` may carry a `status` value, and so may a `create`, for a thread that arrives already closed ([0054](0054-forking-a-mirrored-page.md) §5). The thread's status is the value of its latest post that carries one; a thread with none is `open`. Posting `open` reopens a thread. The values are `config` records of kind `thread-status` in the tenant `config` ([0015](0015-record-format-and-partition-registry.md) §3), each with a label, a **category** (`open` or `closed`) and a display order, so an instance can add its own; the defaults ship in `docs/registry/thread-statuses.toml` and follow the labels of bug trackers and project tools: **Open**, **In progress** and **Blocked** (open), and **Resolved**, **Won't fix**, **Declined**, **Stale** and **Archived** (closed), the last for threads an import creates ([0054](0054-forking-a-mirrored-page.md) §5). The category, not the name, drives the default visibility rule below (closed threads collapse) and the `thread-status` notification of [0021](0021-notifications.md) §2, which fires when a thread moves from an open to a closed status. `open` is required and cannot be retired. A status is therefore a claim made in public, in the thread, by a named actor, and disputing it is another post. Anyone who may post may set one.

**Visibility is a viewer preference, not data.** Each thread on a talk page is shown *visible*, *collapsed* or *hidden*, chosen by a rule over its status category and the age of its last post. The instance sets the defaults (for example: closed statuses collapsed; anything quiet for ninety days collapsed; nothing hidden), and a viewer overrides them in the `threads.visibility` preference ([0027](0027-preferences-and-portability.md) §1). The server renders the threads; the UI applies the rule; so the shared caches hold one public form ([0014](0014-caches-and-search.md) §1) and no viewer's choice reaches them.

**A thread's status is also a projected statement** when the `thread-status` role is bound: threads carry statements as document pages do, and the projection writes one read-only statement per thread from its latest status-bearing post; this section stays the one place a status is set, and asserting that property on a thread is refused ([0038](0038-page-metadata-and-categories.md) §9).

**There are no archive pages.** `/Archive 1` exists on MediaWiki because a talk page is a text file that grows. A talk page here is a query, and an old thread is simply collapsed.

### 7. History, activity, logs and RDF (extends 0001 §1 and §6, 0011 §6.1 and §8)

*Changed by A4, A8, A10.*

**Every thread record is an activity row** ([0012](0012-api-requirements.md) §3) of kind `edit` on the thread's page ID, with the operation in the parsed summary. A thread's history is its rows. `Special:Contributions` and `list=recentchanges` show posts as edits to `Thread:` pages, with the talk page in the row's `target`.

**A talk page's or board's history is composite:** the rows of every thread currently attached to it, home or listing, plus the `thread/attach`, `thread/detach` and move events, in one sequence ordered by time ([0049](0049-boards.md) §7, §9). It is served by `GET /page/{id}/history` for the talk page's ID and by `prop=revisions` on the talk title, where each row is a revision with its own global revision ID. A thread that moves away takes its rows with it.

**Log events**, projected from the records as 0011 §6.1 requires: `create` → `create/create`; `rename` and `move` → `move/move`, with the old and new title or the source and target talk pages as parameters; `attach` and `detach` → `thread/attach` and `thread/detach` ([0049](0049-boards.md) §9). `delete/delete` and `delete/restore` come from the thread's `read` ACL ([0023](0023-moderation.md) §4). `post` and `edit` are revisions, not log events. Posts and thread operations are patrollable, as every `edit` row in `pages` is ([0023](0023-moderation.md) §6).

**Hiding and erasure** are the existing mechanisms. `deleterevision` hides a post's text, comment or actor; `erase` with `parts: [text]` removes the text for good. In either case the post stays in the tree as a tombstone: its place, its replies and its timestamp remain, its text does not.

**RDF**, in the metadata graph only, using the AS2 profile of §10:

- **A thread** is `{base}/page/{page ID}`, an `as:OrderedCollection` of its posts, with `as:name` the subject, `as:context` its home's node, `scatter:listedOn` for each listing ([0049](0049-boards.md) §9), `as:published`, and `scatter:status`.
- **A post** is the record's IRI `{base}/record/{partition}/{offset}`, an `as:Note` with `as:inReplyTo`, `as:context` (the thread), `as:attributedTo` (omitted when hidden, with `scatter:attributionHidden` as [0007](0007-actor-identity.md) §9), `as:published`, `as:updated` when edited, `as:content` as HTML with `as:mediaType "text/html"`, `as:source` with the markdown and `as:mediaType "text/markdown"`, and `as:tag` nodes of type `as:Mention` for each resolved wiki link. A hidden or erased post is an `as:Tombstone` with `as:formerType as:Note`.
- **A talk page** is `{base}/page/{talk page ID}`, an `as:OrderedCollection` of its threads, with `scatter:subject` the document node it discusses.
- **Events** are typed as 0011 §8 types them: `as:Create`, `as:Move`, `as:Delete`, `as:Undo`.

The post's revision node of [0001](0001-revision-metadata-rdf.md) §1 is the same IRI; the AS2 properties are added to it. Nothing about threads enters the main or resolved graph, except thread statements, which are output in the main graph as page statements are ([0038](0038-page-metadata-and-categories.md) §9, §11).

### 8. Rendering and the UI (extends 0010)

*Changed by A2, A10.*

- **A thread page** shows its posts as a tree, indented to the instance's `thread.max_depth`. A post deeper than that is shown at the limit with a "replying to" link to its parent; its stored parent is unaffected (§1). Each post has an anchor `#post-{revid}`, a permalink (`Special:PermanentLink/{revid}`), reply, edit and history controls, and the hiding controls of 0010 for those who hold the rights.
- **A talk page** lists its threads, by default in the order they were attached to it, which is creation order unless a thread was moved or listed there, with the newest last, as MediaWiki readers expect, and a switch to order by last activity ([0049](0049-boards.md) §8). Each thread shows its subject, status, home and listings, participant count, last activity, and its posts or a collapsed summary per §6. A "new thread" form creates one attached to this page.
- **Subject pages** get the talk tab of 0010, with the thread count.
- **Watching** a subject watches its talk page and the threads attached to it; a thread can be watched on its own. The watchlist is [0020](0020-change-feeds.md).
- **Editing another actor's post** is permitted only with `ts-editpost` (§12) and always shows an "edited by" note, because a post is speech.

### 9. API (extends 0012 §4 and §5)

*Changed by A4, A9, A10.*

**REST**, under `rest.php/triplespace/v0`, with the post shape of §10:

| Route | Meaning |
|---|---|
| `GET /thread/{page ID}` | The thread: subject, `home`, `listings`, status, and its posts as `as:Note` objects in tree order |
| `POST /thread` | `create` |
| `POST /thread/{page ID}/post` | `post`, with `inReplyTo` and optional `status` |
| `PUT /thread/{page ID}/post/{revid}` | `edit` |
| `POST /thread/{page ID}/rename`, `/move`, `/attach`, `/detach` | The remaining operations, each with a base offset; `keep` on `/move`, and `also` on `POST /thread` ([0049](0049-boards.md) §11) |
| `GET /page/{talk page ID}/threads` | The attached threads with status, `home` and last activity, without their posts |
| `GET /page/{id}/history` | Unchanged; composite for a talk page ID (§7) |

**Action API**, additively under 0012 §1. Reading works as for any page: `prop=revisions`, `prop=info`, `action=parse`, `list=recentchanges`, `list=usercontribs`, `list=backlinks`, `action=watch`. `action=delete` and `action=undelete` on a `Thread:` title work. `action=edit` on a `Thread:` or talk title is refused with MediaWiki's own `no-direct-editing` (`apierror-no-direct-editing`), because neither model supports direct editing ([0041](0041-content-models.md) §8), so that a bot learns at once that these pages are not text. DiscussionTools' `action=discussiontoolsedit` (`paction=addtopic|addcomment`) can be supported later on the same records, with `commentid` values of the form `c-{revid}` (open questions).

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

*Changed by A9, A10.*

Threads and talk pages get rows in `view.page` (namespace, title, content model `triplespace-thread` or `triplespace-talk` ([0041](0041-content-models.md) §10), latest offset and revision ID, deleted, generation), so the title index and `page_link` cover them without change. Beside it:

```sql
CREATE TABLE view.thread (
  page_id bigint PRIMARY KEY REFERENCES view.page,
  target_kind smallint NOT NULL, target_id text NOT NULL, talk_page_id bigint NOT NULL,   -- the home
  subject text NOT NULL, status text NOT NULL DEFAULT 'open',
  created timestamptz NOT NULL, last_post timestamptz NOT NULL, post_count integer NOT NULL
);
CREATE TABLE view.thread_attachment (          -- every attachment, home or listing (0049 §12)
  thread_id bigint NOT NULL REFERENCES view.thread, talk_page_id bigint NOT NULL, attached timestamptz NOT NULL,
  PRIMARY KEY (thread_id, talk_page_id)
);
CREATE INDEX thread_attachment_talk ON view.thread_attachment (talk_page_id, attached);
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

Boards have `view.talk_page` rows too. These are projections in the sense of 0013 §7, applied synchronously for interactive writes. **Caches** follow 0014: a rendered post is keyed by (revid, latest offset, generation); a thread by (page ID, latest revision ID, generation); a talk page listing by the same for its talk page ID; erasure bumps the thread's generation and purges by tag. **Search** indexes each thread as one document in the `pages` index, subject plus post text, with its namespace; hidden and erased text is not indexed.

### 12. Permissions (extends 0016 §2; amends 0016 §4)

*Changed by A4, A10.*

| Permission | Governs | Default groups |
|---|---|---|
| `edit` | `create`, `post` (including a status), and `edit` of one's own posts | `user`, `temp` |
| `ts-editpost` *(new)* | `edit` of another actor's post | `sysop` |
| `move` | `rename`, `move`, `attach` and `detach` | `autoconfirmed` |
| `delete`, `undelete` | The thread's `read` ACL ([0023](0023-moderation.md) §4) | `sysop` |
| `deleterevision` | Hiding a post's text, comment or actor | `sysop` |
| `ts-erase` | Erasing a post's text | `suppress` |

**Enclosure gains one rule:** a talk page encloses the threads whose *home* it is, not those listed on it ([0049](0049-boards.md) §7). Protecting `Item talk:Q42` with `acl:page:{talk page ID}` therefore restricts every thread homed on it, and a thread moved elsewhere leaves that protection behind. `attach` and `detach` need `move` on the thread and `edit` on the target ([0049](0049-boards.md) §13). Namespace ACLs on talk namespaces apply as they do to any namespace.

### 13. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with `scatter-threads` and every change this section listed. The table this section first gave is in A1.

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

- **Q1.** ~~**Namespace numbers** for `Thread` and `Thread talk` (registry README, pending allocations).~~ *Settled by `docs/registry/namespaces.toml`, under the numbering policy of 0008 A6: 214/215.*
- **Q2. Temporary accounts.** Whether a `temp` account gets a `User talk` page for messages, as MediaWiki gives one. [0021](0021-notifications.md) asks the same of their notifications.
- **Q3.** ~~**Notifications**: whether `[[User:Example]]` in a post notifies that user, and what a reply to one's post delivers. Watching is settled by [0020](0020-change-feeds.md).~~ *Settled by [0021](0021-notifications.md) §2: `mention`, `reply`, `talk` and `thread-status` reasons.*
- **Q4. DiscussionTools compatibility.** Whether to implement `discussiontoolsedit` and `discussiontoolspageinfo` on these records, and how far Convenient Discussions and similar scripts can be made to work.
- **Q5.** ~~**Importing existing talk pages.** A Wikibase's `Item_talk:` and a project's `Project_talk:` pages are wikitext. One option is a job that splits a page by `==` heading into threads with one post each, attributed to the section's signatures where they parse and to an `imported` surrogate otherwise.~~ *Settled by [0054](0054-forking-a-mirrored-page.md) §5: split by level-two heading with the frontmatter first, one wikitext post per section attributed to the importer and the job, each thread created `archived` and dated from its earliest signature.*
- **Q6.** ~~**Cross-tenant discussion.** Whether a tenant may show, read-only, another tenant's threads about an entity it mirrors from that tenant.~~ *Settled by [0028](0028-tenancy-policy.md) §6: by policy, read-only.*
- **Q7.** ~~**Federation.** Inbox, outbox and actor keys, if ever.~~ *Settled by [0022](0022-federation.md) Part B: opt-in `Person` actors, talk pages as `Group` actors that `Announce` threads, inbound replies as posts by `federated` surrogates; the inbound protocol details stay open there.*
- **Q8.** ~~**Limits.** The post size limit, the depth default, and rate limits on posting.~~ *Settled by [0024](0024-subsidiary-accounts.md) §5, as to where they live: the size limit and `thread.max_depth` are `site` settings (§5, §8); posting is rate-limited in the `edit` and `create` classes. The values are tuning.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0001](0001-revision-metadata-rdf.md) §1, §6 | §7 | extends | 0001 A8 |
| [0005](0005-crate-organization.md) §2 | §13 | amends | 0005 A15 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §2, §3, §5 | amends | 0008 A3 |
| [0008](0008-namespaces-and-document-pages.md) §1, §3, §5, §8, §10 | §2, §3, §5 | extends | 0008 A3 |
| [0008](0008-namespaces-and-document-pages.md) Q4 | — | settles | 0008 Q4 |
| [0010](0010-site-ui.md) §2 | §8 | extends | 0010 A9 |
| [0010](0010-site-ui.md) Q11 | §8 | settles | 0010 Q11 |
| [0011](0011-logs.md) §6.1 | §7 | extends | 0011 A5 |
| [0012](0012-api-requirements.md) §4, §5 | §9 | extends | 0012 A8 |
| [0013](0013-postgres-storage.md) §5.6, §6, §7 | §2, §11 | extends | 0013 A6 |
| [0014](0014-caches-and-search.md) §1, §5 | §11 | amends | 0014 A3 |
| [0014](0014-caches-and-search.md) §10 | §11 | extends | 0014 A3 |
| [0015](0015-record-format-and-partition-registry.md) §1, §3, §5 | §4, §6 | extends | 0015 A4 |
| [0016](0016-permissions-and-access-control.md) §2 | §12 | extends | 0016 A2 |
| [0016](0016-permissions-and-access-control.md) §4 | §12 | amends | 0016 A2 |

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
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §13
- **Summary:** 0005 §2 is the one crate table CI checks, and carries `scatter-threads` and every change this section listed (0005 A15).

Replaced text (§13):

> | Layer | Crate | Contents | Depends on |
> |---|---|---|---|
> | Substrate | `scatter-threads` | The `scatter:v0/thread` payload type and its four parts; the fold from records to thread state (tree, subject, attachment, status); title minting (§3); validation of operations against thread state. Pure | `scatter-log`, `scatter-pages` |
> | | `scatter-markdown` | Gains wiki links and the HTML sanitizer (§5), for every use of the model | — |
> | Triplespace | `triplespace-titles` | The `thread` and `composite` namespace kinds and talk-title resolution (§2–3) | as 0008 §11 |
> | | `triplespace-projections` | `view.thread`, `view.post`, `view.talk_page`, and `page_link` rows from posts | as 0013 |
> | | `triplespace-rdf` | The AS2 nodes of §7 | as 0013 |

### A2. Watching

- **Date:** 2026-09-27
- **Source:** [0020](0020-change-feeds.md) §3
- **Change:** extends §8
- **Summary:** Watching a subject watches its talk page and the threads attached to it; a thread can be watched on its own. §8 had been given the sentence in place.

### A3. Notifications

- **Date:** 2026-09-27
- **Source:** [0021](0021-notifications.md) §2
- **Change:** extends §6
- **Summary:** `mention`, `reply`, `talk` and `thread-status` reasons; the `thread-status` notification fires when a thread moves from an open to a closed status. This settled Q3.

### A4. Deletion is an ACL; posts are patrollable

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §4, §6
- **Change:** amends §1, §7, §12; extends §9
- **Summary:** `delete` and `undelete` are no longer thread operations. Deleting a thread is a `read` ACL on its page ID, written to the tenant `log` partition, and undeletion retires it; hiding one post is a `record` ACL on that post's record with `parts: [text]`. The `thread` payload type carries `create`, `post`, `edit`, `rename` and `move`, and the sentence below about "any operation on a deleted thread other than `undelete`" reads as "any operation on a deleted thread". Posts and thread operations are patrollable.

Replaced text (§1):

> | `create` | Mints the thread's page ID and title, attaches it to a talk page, and is the opening post | The opening post | — |
> | `post` | A post in the thread, top-level or in reply to another post | The post | — |
> | `edit` | Replaces the **complete text** of one post | The new text | Required |
> | `rename` | Changes the subject, and so the title (§3) | — | Required |
> | `move` | Attaches the thread to a different talk page (§2) | — | Required |
> | `delete` | Hides the thread and every post in it from everyone but administrators, as 0008 §4 hides a page | — | Required |
> | `undelete` | Reverses a `delete` | — | Required |
>
> **Replies need no base offset.** Two people answering the same post at once both succeed. `edit`, `rename`, `move`, `delete` and `undelete` carry the base offset of [0006](0006-log-integrity-and-erasure.md) §8, and a mismatch is an `editconflict`. A `post` whose parent is not a post of the same thread, and any operation on a deleted thread other than `undelete`, is rejected.

Replaced text (§7):

> **Log events**, projected from the records as 0011 §6.1 requires: `create` → `create/create`; `rename` and `move` → `move/move`, with the old and new title or the source and target talk pages as parameters; `delete` and `undelete` → `delete/delete` and `delete/restore`. `post` and `edit` are revisions, not log events.

Replaced text (§12):

> | `delete`, `undelete` | `delete`, `undelete` | `sysop` |

### A5. Visibility as a preference

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §1
- **Change:** amends §6
- **Summary:** The visibility rule of §6 is the `threads.visibility` preference.

Replaced text (§6):

> **Visibility is a viewer preference, not data.** Each thread on a talk page is shown *visible*, *collapsed* or *hidden*, chosen by a rule over its status and the age of its last post. The instance sets the defaults (for example: `resolved`, `rejected` and `stale` collapsed; anything quiet for ninety days collapsed; nothing hidden), and a viewer overrides them.

### A6. Cross-tenant discussion

- **Date:** 2026-09-27
- **Source:** [0028](0028-tenancy-policy.md) §6
- **Change:** extends §2
- **Summary:** A tenant may show, read-only and by policy, a provider tenant's threads about an entity it mirrors. This settled Q6.

### A7. Thread statuses are configuration

- **Date:** 2026-09-27
- **Source:** Direct: James, review of 2026-09-27 (third pass)
- **Change:** amends §6
- **Summary:** Statuses are `config` records of kind `thread-status` in the tenant `config` ([0015](0015-record-format-and-partition-registry.md) §3), each with a label, a **category** (`open` or `closed`) and a display order; the defaults ship in `docs/registry/thread-statuses.toml` and follow the labels of bug trackers and project tools: **Open**, **In progress** and **Blocked** (open), and **Resolved**, **Won't fix**, **Declined** and **Stale** (closed). `rejected` is renamed `declined`. The category, not the name, drives the default visibility rule below (closed threads collapse) and the `thread-status` notification of [0021](0021-notifications.md) §2, which fires when a thread moves from an open to a closed status. `open` is required and cannot be retired.

Replaced text (§6):

> The values are registry data, so an instance can add its own; the defaults are `open`, `resolved`, `rejected` and `stale`. A status is therefore a claim made in public,

### A8. Articles and thread statements

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §8–9, §11
- **Change:** amends §5, §7; extends §6
- **Summary:** By section:
  - §5: With articles in the main namespace, `[[Q42]]` links to the main-namespace page titled `Q42`, in markdown as in wikitext. An entity is mentioned with its namespace: `[[Item:Q42]]`, which still renders with the entity's label.
  - §6: Threads carry statements, as document pages do. When the `thread-status` role is bound, a thread's status is also a projected, read-only statement, derived from the latest status-bearing post; this section stays the one place a status is set, and asserting that property on a thread is refused.
  - §7: Except thread statements, which are output in the main graph as page statements are.

Replaced text (§5):

> - **Wiki links.** `[[Title]]`, `[[Title|text]]` and `[[Title#Section]]` are resolved by the title resolver of 0008 §3, as in the wikitext subset. `[[Q42]]` renders with the entity's label, as Wikibase does. This is the mention syntax; it is what every editor already knows.

Replaced text (§7):

> The post's revision node of [0001](0001-revision-metadata-rdf.md) §1 is the same IRI; the AS2 properties are added to it. Nothing about threads enters the main or resolved graph.

### A9. Content models

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §4, §8, §10
- **Change:** amends §2, §3, §9, §11
- **Summary:** By section:
  - §2: The kind `composite` is gone: a talk namespace is a `pages` namespace whose model is `triplespace-talk`, a model whose source is *composite* (no records of its own).
  - §3: The kind `thread` is gone: `Thread` is a `pages` namespace whose model is `triplespace-thread`.
  - §9: `action=edit` on a `Thread:` or talk title is refused with MediaWiki's own `no-direct-editing` (`apierror-no-direct-editing`), because neither model supports direct editing. `ts-structured` is retired.
  - §11: The `content_model` values are the registry IDs `triplespace-thread` and `triplespace-talk`, not `thread` and `talk`.

Replaced text (§2):

> **Talk namespaces change kind from `reserved` to `composite`.** The talk page for a target is the ordered set of threads currently attached to it. It has no stored text and no records of its own.

Replaced text (§3):

> **`Thread` is a registered namespace of a new kind, `thread`:** pages composed from thread records. Its paired `Thread talk` is `reserved` and empty, because clients assume every subject namespace has one. Subpages are not allowed. The numbers are 214 and 215, in the Triplespace range of [0008](0008-namespaces-and-document-pages.md) §2 as amended on 2026-09-27; LiquidThreads' 90 and 91 are not reused.

Replaced text (§9):

> `action=edit` on a `Thread:` or talk title is refused with a new error code, `ts-structured`, so that a bot learns at once that these pages are not text.

### A10. Boards: one home, any number of listings

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §2, §5–9, §11–13
- **Change:** amends §1, §2, §3, §7, §8, §12; extends §4, §9, §11
- **Summary:** By section:
  - §1: Two operations join the table: `attach` lists the thread on a further talk page or board, and `detach` removes a listing. Both need a base offset. `create` may carry `also`, further targets listed at creation, and `move` may carry `keep`, which keeps the old home as a listing.
  - §2: A thread has exactly one **home**, the attachment this section describes, and may have **listings** on further talk pages and boards, up to `thread.max_attachments`. "A thread cannot be attached to two pages; cross-posting is a link" is replaced. Only the home encloses the thread, is its `as:context`, and may be a user talk page; listing, composite history and watches treat every attachment alike. A board ([0049](0049-boards.md) §1) is a `page` target whose talk page is itself.
  - §3: `Thread talk` (215) is a `virtual` namespace that forwards to `Thread`: `Thread talk:X` resolves to `Thread:X`. A thread is its own talk page.
  - §4: `also` on `create`; `target` and `talk` on `attach` and `detach`; `keep` on `move`.
  - §7: A talk page's or board's history includes every thread attached to it, home or listing, and the `thread/attach` and `thread/detach` events. A thread's `as:context` is its home, and it carries `scatter:listedOn` for each listing.
  - §8: A talk page lists its threads in the order they were attached to it, which is creation order unless a thread was moved or listed there. A thread shows its home and its listings.
  - §9: `POST /thread/{page ID}/attach` and `/detach`; `also` on `POST /thread` and `keep` on `/move`; `home` and `listings` in `GET /thread/{page ID}`; `home` on each thread of `GET /page/{id}/threads`.
  - §11: `view.thread_attachment` holds every attachment, home or listing, and serves the listing query in place of the `thread_talk` index; `view.thread.talk_page_id` is the home. Boards have `view.talk_page` rows.
  - §12: A talk page encloses the threads whose *home* it is, not those listed on it. `attach` and `detach` need `move` on the thread and `edit` on the target.

Replaced text (§1):

> **A thread is a page in the `Thread` namespace (§3).** It has a page ID from the tenant's sequence ([0015](0015-record-format-and-partition-registry.md) §2, [0018](0018-tenants.md) §2), a subject, an attachment (§2), a status (§6) and a history.

Replaced text (§2):

> **A thread is attached to exactly one target.** The target is named by identifier,
>
> It is minted from the page-ID sequence by the first `create` or `move` record that attaches a thread to that target,
>
> **A `move` moves the whole thread.** Its posts leave the old talk page's history and join the new one's, and one `move/move` log event, with the source and target talk pages as parameters, appears in both pages' logs (§7). A thread cannot be attached to two pages; cross-posting is a link.

Replaced text (§7):

> **A talk page's history is composite:** the rows of every thread currently attached to it, plus the attach and move events, in one sequence ordered by time.
>
> - **A thread** is `{base}/page/{page ID}`, an `as:OrderedCollection` of its posts, with `as:name` the subject, `as:context` the talk page's node, `as:published`, and `scatter:status`.

Replaced text (§8):

> - **A talk page** lists its threads, by default in creation order with the newest last, as MediaWiki readers expect, with a switch to order by last activity. Each thread shows its subject, status, participant count, last activity, and its posts or a collapsed summary per §6.

Replaced text (§9):

> | `POST /thread/{page ID}/rename`, `/move`, `/delete`, `/undelete` | The remaining operations, each with a base offset |

Replaced text (§11):

> CREATE INDEX thread_talk ON view.thread (talk_page_id, created);

Replaced text (§12):

> **Enclosure gains one rule:** a talk page encloses the threads attached to it. Protecting `Item talk:Q42` with `acl:page:{talk page ID}` therefore restricts every thread on it, and a thread moved elsewhere leaves that protection behind.

### A11. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–13
- **Summary:** A1–A10 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A4 (in part), A7, A8, A9 and A10 were eighteen blockquotes, and A2, A3, A5 and A6 were recorded in the header or in other ADRs. The file before conversion is commit `0b26a3a`.

### A12. Talk pages of inherited pages

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §7
- **Change:** extends §2
- **Summary:** A page a repository serves has a local talk page on its provider-ranged page ID, as a mirrored entity has one; the fork job moves those threads to the fork's talk page.

### A13. Imported threads: wikitext posts, closed on creation, dated by the import

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §5
- **Change:** amends §3, §4, §5, §6
- **Summary:** `status` may appear on `create`; the content part gains `mediaType` (`text/markdown` or `text/x-wiki`, the latter written only by imports and rendered by the wikitext pipeline) and `imported_from`; a thread an import creates takes the date the import supplies for its title; the default statuses gain **Archived** (closed). Q5 settled.

Replaced text (§3):

> The date is the UTC date of the `create` record and never changes.

Replaced text (§4):

> | `status` | `post` | A status value (§6), when the post sets one |

Replaced text (§5):

> **Posts are markdown**, rendered by the `markdown` content model of 0008 §5, with two additions that apply to that model everywhere:

Replaced text (§6):

> **A thread's status is set by posting.** A `post` may carry a `status` value.

> **Resolved**, **Won't fix**, **Declined** and **Stale** (closed).
