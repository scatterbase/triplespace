# 14. Discussions

This chapter covers discussion: threads and posts, composite talk pages and attachment by identifier, the `Thread` namespace, the four parts of a thread record, markdown text, status and visibility; boards and a thread's one home and any number of listings; pinned threads; and proposals, threads whose records carry a payload offered to the wiki the subject came from. It assumes the log, record parts and erasure from [01](01-log-and-records.md), page IDs, namespaces and content models from [10](10-pages-and-content-models.md), ACLs and enclosure from [09](09-security-and-moderation.md), and foreign entities and clusters from [04](04-entities-and-identifiers.md) and [05](05-providers-and-ingest.md). Not here: rendering and the UI are in [19](19-site-ui.md); history, activity rows, log events, RDF and the Activity Streams profile in [16](16-logs-feeds-and-notifications.md); tables, caches and search in [03](03-storage-caches-and-search.md); the API in [18](18-api.md); permissions in [09](09-security-and-moderation.md) §8.1 and §8.10; exporting and pushing proposals, and federation, in [17](17-federation-and-publication.md); foreign threads and page proposals from a fork in [13](13-mirrored-pages.md); scoped boards in [15](15-structured-pages.md).

## 1. Threads and posts

*Sources: [0019](../decisions/0019-discussions.md) §1, §2, §3, §4, §5; [0033](../decisions/0033-backend-stack.md) §9.2; [0049](../decisions/0049-boards.md) §2, §5, §6; [0067](../decisions/0067-proposals.md) §3; [0069](../decisions/0069-synchronized-talk-pages.md) §4.*

### 1.1 A thread is a page; a post is a record

*Sources: [0019](../decisions/0019-discussions.md) §1.*

**A thread is a page in the `Thread` namespace (§1.4).** It has a page ID from the tenant's sequence ([0015](../decisions/0015-record-format-and-partition-registry.md) §2, [0018](../decisions/0018-tenants.md) §2), a subject, a home and any listings (§1.3), a status (§2) and a history. Its records live in the tenant's `pages` partition, keyed by the thread's page ID, with the payload type `scatter:v0/thread` (§1.5).

**A post is one record.** Its identity is its global revision ID, header field 7. The reply structure is a graph of record references: a reply carries the revision ID of the post it answers. Nothing is nested and nothing is rewritten when a reply arrives. The thread's current state, its tree, its subject, its attachment and its status, is a fold over its records, as an entity is a fold over its change sets.

**Full text per post, as 0008 §4 stores full text per revision.** A post's edit history is the chain of `edit` records naming its revision ID, and the diff between two versions is computed when it is read.

**The true parent is always stored.** The instance's indentation limit, `thread.max_depth` ([19](19-site-ui.md)), is a rendering rule, not a constraint on the data. Changing the limit later rewrites nothing.

Deleting a thread is not an operation: it is a `read` ACL on its page ID, written to the tenant `log` partition, and undeletion retires it; hiding one post is a `record` ACL on that post's record with `parts: [text]` ([0023](../decisions/0023-moderation.md) §4; [09](09-security-and-moderation.md) §6).

### 1.2 Operations

*Sources: [0019](../decisions/0019-discussions.md) §1; [0049](../decisions/0049-boards.md) §6; [0067](../decisions/0067-proposals.md) §3; [0069](../decisions/0069-synchronized-talk-pages.md) §4.*

Each record carries one operation:

| Operation | Meaning | Text part | Base offset |
|---|---|---|---|
| `create` | Mints the thread's page ID and title, attaches it to a talk page, its home, and is the opening post; may carry `also`, further targets listed at creation | The opening post | — |
| `post` | A post in the thread, top-level or in reply to another post | The post | — |
| `edit` | Replaces the **complete text** of one post | The new text | Required |
| `rename` | Changes the subject, and so the title (§1.4) | — | Required |
| `move` | Attaches the thread to a different home (§1.3); with `keep`, the old home stays as a listing | — | Required |
| `attach` | Lists the thread on a further talk page or board (§3.6) | — | Required |
| `detach` | Removes a listing (§3.6) | — | Required |
| `pin`, `unpin` | Pins or unpins the thread on one of its attachments (§4) | — | Required |
| `propose` | Makes the thread a proposal, or re-compiles one: kind, upstream target, base and payload (§5.3) | — | Required |
| `submit` | Records that the proposal was exported or pushed, with the upstream revision IDs (§5.3) | — | Required |
| `withdraw` | The proposer takes the proposal back (§5.3) | — | Required |

**Replies need no base offset.** Two people answering the same post at once both succeed. Every other operation except `create` and `post` carries the base offset of [0006](../decisions/0006-log-integrity-and-erasure.md) §8, and a mismatch is an `editconflict`. A `post` whose parent is not a post of the same thread, and any operation on a deleted thread, is rejected. The validation of `attach`, `detach`, `move` and `pin` against the thread's state is in §3.6 and §4.2; that of `propose` and `submit` in §5.3.

### 1.3 Talk pages are composite; attachment is by identifier

*Sources: [0019](../decisions/0019-discussions.md) §2; [0049](../decisions/0049-boards.md) §5.*

**A thread has exactly one home, and may have listings** (§3.5). The home is the attachment `create` sets and `move` changes; listings are further talk pages and boards the thread appears on. What the home alone decides and what every attachment shares is §3.7. A board (§3.1) is a `page` target whose talk page is itself. A target is named by identifier, never by title, so that a thread follows its subject through a rename or a move and the record's key is never content ([0006](../decisions/0006-log-integrity-and-erasure.md) §3):

| Target kind | Identifier | Talk page |
|---|---|---|
| `page` | A page ID, unambiguous within the tenant because every kind of page draws on one `pageid` space ([10](10-pages-and-content-models.md) §3.1): a local entity's or document page's, from the tenant's sequence below 2^40; a **mirrored entity's provider-ranged page ID**, `provider_number << 40 \| upstream page ID` ([0013](../decisions/0013-postgres-storage.md) §6; [0015](../decisions/0015-record-format-and-partition-registry.md) §2), so a mirrored entity has a talk page without any local record; or the provider-ranged page ID of a page a repository serves ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6–7) | The subject namespace's paired talk namespace |
| `actor` | A local actor key ([0007](../decisions/0007-actor-identity.md) §1) | `User talk` |

`User talk` attaches to the actor rather than to the user page because MediaWiki lets a user's talk page exist when the user page does not. The creation rule is that of 0008 §6: the account must be local, registered and not vanished.

**Talk namespaces are `pages` namespaces whose model is `triplespace-talk`,** a model whose source is *composite* ([0041](../decisions/0041-content-models.md) §4; [10](10-pages-and-content-models.md) §4.4). The talk page for a target is the ordered set of threads currently attached to it. It has no stored text and no records of its own. A talk title resolves through its subject: `Item talk:Q42` resolves `Item:Q42` with the title resolver of 0008 §3, following aliases and clusters, so `Item talk:WDQ123` redirects to `Item talk:Q456` exactly as the subject does; `User talk:Example` resolves the username to the actor key. The talk page of a subject that does not exist does not exist either.

**A talk page has a page ID** so that `prop=info`, `action=watch` and the activity row's `target` can name it. It is minted from the page-ID sequence by the first `create`, `move` or `attach` record that attaches a thread to that target, and that record carries it **in its attestation map** (§1.5), never in its content part: the ID is a value the server fills at append, and a server-filled value never enters a part the client signed over as submitted ([0015](../decisions/0015-record-format-and-partition-registry.md) §1; [01](01-log-and-records.md)), so the client's signature covers the content and comment parts as it sent them and the attestation carries what the server added. A rebuild reads the ID from the attestation map rather than deriving it. The appending transaction takes the same lock that allocates offsets ([0013](../decisions/0013-postgres-storage.md) §2), so two threads created at once on a fresh target agree on it.

**A `move` moves the whole thread.** Its posts leave the old home's history and join the new one's, and one `move/move` log event, with the source and target talk pages as parameters, appears in both pages' logs ([16](16-logs-feeds-and-notifications.md)). A thread is listed on further pages with `attach`, never by a second home.

**A mirrored entity's talk page is never loaded from upstream.** The talk page of a mirrored entity holds local threads only. They live in the tenant's `pages` partition, which is not what a tenant exposes when it is a provider to others ([0018](../decisions/0018-tenants.md) §5), so a local historical society's discussion of a Librarybase item stays on the society's wiki. The talk page of a mirrored entity offers a link to the upstream talk page for readers who want the provider's own discussion. A page inherited from a page repository has a local talk page the same way, on its ranged page ID, and when the page is forked the fork job moves its threads to the fork's talk page ([0054](../decisions/0054-forking-a-mirrored-page.md) §5). **A page repository's talk page is followed** when the repository's `talk` is `sync`: its sections are shown on the local talk page as foreign threads, mirrored records with upstream attribution and provider-ranged IDs, beside the local threads, and a fork may follow its upstream talk page the same way ([0069](../decisions/0069-synchronized-talk-pages.md) §1–3, §7; [13](13-mirrored-pages.md)).

### 1.4 The `Thread` namespace and thread titles

*Sources: [0019](../decisions/0019-discussions.md) §3; [0049](../decisions/0049-boards.md) §2.*

**`Thread` is a `pages` namespace whose model is `triplespace-thread`** ([0041](../decisions/0041-content-models.md) §4): pages composed from thread records. Its paired `Thread talk` is a `virtual` namespace that forwards to `Thread`: `Thread talk:X` resolves to `Thread:X`, since a thread is its own talk page (§3.2). Subpages are not allowed. The numbers are 214 and 215, in the Triplespace range of [0008](../decisions/0008-namespaces-and-document-pages.md) §2; LiquidThreads' 90 and 91 are not reused. The rows are in the namespace catalogue, [10](10-pages-and-content-models.md) §1.4.

**A thread's title is its creation date and its subject:** `Thread:2026-09-27/Why is P31 wrong here`. The date is the UTC date of the `create` record, or, for a thread an import creates, the date the import supplies ([0054](../decisions/0054-forking-a-mirrored-page.md) §5), and never changes. The subject is what the author typed, normalized as a `first-letter` title with MediaWiki's forbidden characters (`# < > [ ] | { }`) rejected. Two threads created on the same date with the same subject are told apart with a MediaWiki-style suffix: `…/Why is P31 wrong here (2)`. The client submits the subject in the content part; the server mints the title, suffix included, at append and carries it **in the attestation map** of the `create` or `rename` record (§1.5), as it carries the talk page ID it mints (§1.3), so the title index is a projection, a rebuild reproduces it from the attestation maps, and the client's signature stays over the content it submitted. A page `move`, by contrast, carries its new title in the content part, because the mover chose it.

The title says what the thread is about and when it began, it can be typed, and it never depends on which talk page the thread is attached to, so a `move` changes nothing about how the thread is cited. A `rename` does: the old title stops resolving, as 0008 §6 rules for user pages, because subjects can contain usernames. Stable references are `Special:Redirect/page/{page ID}` for a thread and `Special:PermanentLink/{revid}` for a post ([19](19-site-ui.md)).

### 1.5 The record: four parts

*Sources: [0019](../decisions/0019-discussions.md) §4; [0049](../decisions/0049-boards.md) §6; [0069](../decisions/0069-synchronized-talk-pages.md) §4; [0067](../decisions/0067-proposals.md) §3.*

The payload type `scatter:v0/thread` is in the `pages` partition's list ([registry](../registry/graphs.toml)). It declares **four** body parts. The first three keep the meaning and index that [0015](../decisions/0015-record-format-and-partition-registry.md) §1 gives every Triplespace payload type; the fourth is this type's own:

| # | Part | Holds | RevisionDelete bit |
|---|---|---|---|
| 0 | **Content** | The operation and its structure (below). Never the post's text | — |
| 1 | **Comment** | The summary for `edit`, `rename`, `move`, `attach`, `detach`, `pin`, `unpin`, `propose`, `submit` and `withdraw`; null for `create` and `post`, whose text *is* the contribution | comment |
| 2 | **Attestation** | As 0015 §1 | user |
| 3 | **Text** | The post's markdown source, for `create`, `post` and `edit`; null otherwise | text |

**Why a fourth part.** Erasure is per part. When a post's text has to be erased, the thread must keep its shape: the erased post still has a parent, and its replies still hang from it. If the parent pointer shared a part with the text, erasing the text would erase the tree. So the structure and the text are separate parts, and `erase` ([0006](../decisions/0006-log-integrity-and-erasure.md) §7, 0015 §1; [01](01-log-and-records.md)) accepts `text` in its `parts` field for this payload type. An erased post renders as a tombstone in its place ([16](16-logs-feeds-and-notifications.md)). The content hash `0x03` is computed over part 0 as before; it identifies the operation, not the prose.

0015 §1 therefore reads: every Triplespace payload type has *at least* three parts, in that order, and a type may declare more. `verify` already checks *n* parts.

**The content part** is a CBOR map using Activity Streams property names where AS2 has them ([16](16-logs-feeds-and-notifications.md)):

| Field | Present on | Meaning |
|---|---|---|
| `op` | all | The operation |
| `name` | `create`, `rename` | The subject |
| `target` | `create`, `move` | `{kind, id}` (§1.3) |
| `inReplyTo` | `post` | The parent post's revision ID, or null for a top-level post |
| `object` | `edit` | The revision ID of the post whose text this replaces |
| `status` | `post`, `create` | A status value (§2.1), when the post sets one; on `create`, a thread that arrives already closed, as an import's does ([0054](../decisions/0054-forking-a-mirrored-page.md) §5) |
| `mediaType` | `create`, `post`, `edit` (optional) | The text part's media type: `text/markdown` by default, `text/x-wiki` for a post an import writes ([0054](../decisions/0054-forking-a-mirrored-page.md) §5) |
| `imported_from` | `create` (optional) | The repository, talk page, revision and archive subpage an imported thread came from, and the hash of the section's DiscussionTools name where there is one ([0054](../decisions/0054-forking-a-mirrored-page.md) §5, [0069](../decisions/0069-synchronized-talk-pages.md) §7) |
| `also` | `create` (optional) | Further targets listed at creation, each a `target` (§3.6); their talk page IDs are in the attestation map |
| `target` | `attach`, `detach`, `pin`, `unpin` | The target (a `page` target only, for `attach` and `detach`); for `pin` and `unpin`, the attachment pinned or unpinned, any attachment, `actor` included (§4) |
| `pinned` | `create` (optional) | `true` pins the thread on its home from the start, as an import does for front matter (§4; [0069](../decisions/0069-synchronized-talk-pages.md) §7) |
| `keep` | `move` (optional) | `true` keeps the old home as a listing, with its pin if it had one (§3.6) |

The fields of `propose`, `submit` and `withdraw` are in §5.3.

**The attestation map carries what the server minted.** Beside the actor and the signature that [0015](../decisions/0015-record-format-and-partition-registry.md) §1 gives every attestation part ([01](01-log-and-records.md)), a thread record's attestation map holds the values the appending transaction filled in, which the client did not submit and did not sign over:

| Field | Present on | Meaning |
|---|---|---|
| `title` | `create`, `rename` | The minted title, with its suffix where one was needed (§1.4) |
| `talk` | `create`, `move`, `attach`, `detach`, `pin`, `unpin`, and each entry of `also` | The talk page's page ID for the target, minted by this record if the target had none (§1.3, §3.6) |

Mentions, links and the rendered HTML are never stored; they are derived from the text part (§1.6). Nothing in the content part names the record's own ID, which is assigned at append and read from the header, nor any other value the server mints.

### 1.6 Text: markdown, mentions and links

*Sources: [0019](../decisions/0019-discussions.md) §5; [0033](../decisions/0033-backend-stack.md) §9.2.*

**Posts are markdown** by default, rendered by the `markdown` content model of 0008 §5 ([10](10-pages-and-content-models.md) §4.4), with two additions that apply to that model everywhere; a post whose `mediaType` is `text/x-wiki`, which only an import writes, is rendered by the wikitext pipeline instead, with expansion, so the banners and templates of an imported talk page render through the title stack ([0054](../decisions/0054-forking-a-mirrored-page.md) §5; [11](11-rendering-templates-and-modules.md)):

- **Wiki links.** `[[Title]]`, `[[Title|text]]` and `[[Title#Section]]` are resolved by the title resolver of 0008 §3, as in the wikitext subset. An entity is mentioned with its namespace, `[[Item:Q42]]`, which renders with the entity's label, as Wikibase does; a bare `[[Q42]]` links to the main-namespace page of that title, in markdown as in wikitext ([0038](../decisions/0038-page-metadata-and-categories.md) §8). This is the mention syntax; it is what every editor already knows.

- **Raw HTML is sanitized** to the allow-list of 0008 §8. CommonMark passes HTML through; the renderer does not.

`~~~~` is not expanded. A post is attributed by its attestation, and the signature has nothing to add.

The markdown renderer, in `scatter-pages`, is `comrak`, with its wikilinks extension for `[[wiki links]]`, and `ammonia` for sanitization. Both build for wasm ([0033](../decisions/0033-backend-stack.md) §9.2; [22](22-crates-and-stack.md)).

**Rendered HTML is a projection,** produced when the post is read and cached by version ([0014](../decisions/0014-caches-and-search.md); [03](03-storage-caches-and-search.md) §9), never stored.

**Links become `page_link` rows** with `from_page` the thread's page ID ([10](10-pages-and-content-models.md) §5.2). "What links here" on `Item:Q42` therefore lists the threads that discuss it, alongside the project pages that link to it, and `list=backlinks` needs nothing new. Erasing a post's text removes its rows.

**A size limit** on the text part is site configuration.

## 2. Status and visibility

*Sources: [0019](../decisions/0019-discussions.md) §6; [0038](../decisions/0038-page-metadata-and-categories.md) §9.*

### 2.1 A thread's status is set by posting

*Sources: [0019](../decisions/0019-discussions.md) §6.*

**A thread's status is set by posting.** A `post` may carry a `status` value, and so may a `create`, for a thread that arrives already closed ([0054](../decisions/0054-forking-a-mirrored-page.md) §5). The thread's status is the value of its latest post that carries one; a thread with none is `open`. Posting `open` reopens a thread. The values are `config` records of kind `thread-status` in the tenant `config` ([0015](../decisions/0015-record-format-and-partition-registry.md) §3; [23](23-configuration-and-registry.md) §2.2), each with a label, a **category** (`open` or `closed`) and a display order, so an instance can add its own; the defaults ship in `docs/registry/thread-statuses.toml` and follow the labels of bug trackers and project tools: **Open**, **In progress** and **Blocked** (open), and **Resolved**, **Won't fix**, **Declined**, **Stale** and **Archived** (closed), the last for threads an import creates ([0054](../decisions/0054-forking-a-mirrored-page.md) §5). The category, not the name, drives the default visibility rule of §2.2 (closed threads collapse) and the `thread-status` notification of [0021](../decisions/0021-notifications.md) §2 ([16](16-logs-feeds-and-notifications.md) §5.2), which fires when a thread moves from an open to a closed status.

`open` is required and cannot be retired. A status is therefore a claim made in public, in the thread, by a named actor, and disputing it is another post. Anyone who may post may set one, with one exception: `declined` on a proposal thread, which only the proposer or a holder of `edit` on the subject's talk page may set (§5.4).

**Status stays per thread.** A thread resolved on one page is resolved everywhere it appears; a page that no longer wants a thread detaches it ([0049](../decisions/0049-boards.md) §7, in §3.7).

**There are no archive pages.** `/Archive 1` exists on MediaWiki because a talk page is a text file that grows. A talk page here is a query, and an old thread is simply collapsed.

### 2.2 Visibility is a viewer preference, not data

*Sources: [0019](../decisions/0019-discussions.md) §6.*

Each thread on a talk page is shown *visible*, *collapsed* or *hidden*, chosen by a rule over its status category and the age of its last post. The instance sets the defaults (for example: closed statuses collapsed; anything quiet for ninety days collapsed; nothing hidden), and a viewer overrides them in the `threads.visibility` preference ([0027](../decisions/0027-preferences-and-portability.md) §1; [07](07-actors-and-accounts.md) §8.1). The server renders the threads; the UI applies the rule; so the shared caches hold one public form ([0014](../decisions/0014-caches-and-search.md) §1) and no viewer's choice reaches them. Pinned threads are exempt from the age rule but not from the status rule (§4.3).

### 2.3 Threads carry statements; status as a projected statement

*Sources: [0038](../decisions/0038-page-metadata-and-categories.md) §9; [0019](../decisions/0019-discussions.md) §6.*

**Threads carry statements as pages do** ([0038](../decisions/0038-page-metadata-and-categories.md) §1; [06](06-statements-and-properties.md) §5): change sets keyed by the thread's page ID. Priority, component or an assignee are ordinary asserted statements.

**A thread's status is a projected statement.** §2.1 stays the one place a status is set: by a post, in public, by a named actor. The role `thread-status` ([06](06-statements-and-properties.md) §1.2) names the property that represents it. The property's data type is `string`, and its value is the status name from the `thread-status` registry. Statuses are configuration records, not items, and the UI shows the registry's label. When the role is bound, the projection writes one read-only statement per thread from its latest status-bearing post, with provenance naming the post and its author. Asserting a statement with that property on a thread is refused with `ts-derived-property`, because a second definition of status is exactly what this avoids. When the role is unbound, no statement is written and status works as §2.1 describes.

The rule that nothing about threads enters the main or resolved graph excepts thread statements, which are output as page statements are ([0038](../decisions/0038-page-metadata-and-categories.md) §11; [16](16-logs-feeds-and-notifications.md) §6.3).

**A proposal's state is a projected statement in the same way,** through the role `proposal-state` (§5.4).

## 3. Boards

*Sources: [0049](../decisions/0049-boards.md) §1, §2, §3, §4, §5, §6, §7; [0019](../decisions/0019-discussions.md) §2.*

### 3.1 A board is a talk page that is its own subject

*Sources: [0049](../decisions/0049-boards.md) §1.*

**A board is a page in the `Board` namespace (§3.2).** It has a page ID, a title chosen when it is created, a definition (§3.4) stored in its own `page` records, and a listing of the threads attached to it. It is what a talk page is, without a subject to discuss: when a thread attaches to a board, the target is `{kind: page, id: <the board's page ID>}` and the talk page is the board itself, so the record's `talk` field (§1.5) holds the same page ID and nothing is minted.

**A board exists because someone created it.** A talk page exists when its subject does, and is given a page ID by the first thread attached to it. A board's title is free text, so if boards came into being on first use, a typo would make one. Creating a board is a page `create` in namespace 310, which needs `createpage` ([0016](../decisions/0016-permissions-and-access-control.md) §2; [09](09-security-and-moderation.md) §8.1), as creating any page does. Attaching a thread to a board that does not exist, or is deleted, is refused.

**Everything else is what a document page does.** A board is renamed with a page `move` ([0008](../decisions/0008-namespaces-and-document-pages.md) §4), and its threads follow because they are attached by page ID. It is protected and deleted with ACLs ([0023](../decisions/0023-moderation.md)), with the effect on its threads given in §3.7. It carries page statements, as every document page does ([0038](../decisions/0038-page-metadata-and-categories.md) §1), and they are the board's metadata, not its threads'.

**Where boards fit.** Boards are for village pumps, noticeboards, help desks, project chat and announcement channels. `Project talk:` stays what every talk namespace is: discussion of the page beside it. The importer for existing wikitext talk pages ([10](10-pages-and-content-models.md) §6) has a natural destination: a village pump splits into threads homed on a board.

**No subpages.** `Board:Help/Images` would be a second way to express a hierarchy that scopes ([0049](../decisions/0049-boards.md) §14; [15](15-structured-pages.md)) express better. Subpages are off, so that turning them on later breaks no title.

### 3.2 The namespaces, and talk namespaces that forward

*Sources: [0049](../decisions/0049-boards.md) §2.*

| Number | Canonical name | Kind | Models | Notes |
|---|---|---|---|---|
| 310 | `Board` | `pages` | `triplespace-board` (§3.3) | Normalizer `first-letter`; case `first-letter`; no subpages; talk 311 |
| 311 | `Board talk` | `virtual` | — | Forwards to 310 |
| 215 | `Thread talk` | `virtual` | — | Forwards to 214 |

**310 and 311 are the first pair of the second Triplespace block** ([0008](../decisions/0008-namespaces-and-document-pages.md) §2; [10](10-pages-and-content-models.md) §1.3), since 210–219 is full.

**A forwarding namespace.** MediaWiki clients find a page's talk page at the next odd namespace number, so 311 has to be registered. It holds no pages. The registry has a field, `forwards_to`, on namespaces of kind `virtual`: every title in such a namespace resolves to the title with the same text in the namespace it names, normalized by that namespace's rules. `Board talk:General chat` resolves to `Board:General chat`. This is a step of the one title resolver of [0008](../decisions/0008-namespaces-and-document-pages.md) §3 ([10](10-pages-and-content-models.md) §2.2), the same step by which `Item:P31` resolves to `Property:P31`:

- A page view answers with HTTP 301. The mapping never changes, unlike a resolver's ([0029](../decisions/0029-resolver-namespaces.md)), which is why it is permanent rather than 303.
- The Action API reports the title as resolving to the board, as it reports `Item:WDQ123` resolving to `Item:Q456`. `titles=Board talk:General chat&redirects` returns the board.
- The forwarding title has no page ID, no records and no history. An ACL or watch naming it is refused, since there is nothing to restrict or watch. A link to it is a link to the board.

The kind is `virtual` rather than `reserved` because, under 0008 §2's numbering policy, `reserved` means a number that has no meaning here. A forwarding namespace has one. `Media` (−2) is the precedent for a `virtual` namespace that resolves titles to something else.

**`Thread talk` forwards too.** Clients assume every subject namespace has a talk namespace, and a thread is discussed in itself; forwarding serves that better than an empty namespace does: `Thread talk:2026-09-27/Why is P31 wrong here` takes a client to the thread.

**A board and a thread are their own talk pages.** On a `Board` or `Thread` page, `prop=info` with `inprop=talkid|subjectid` returns the page's own ID for both. `Special:TalkPage/Board:General chat` goes to the board, and `Special:NewSection/Board:General chat` opens the board's new-thread form ([0047](../decisions/0047-special-pages.md); [21](21-special-pages.md)). Neither page shows a Talk tab.

### 3.3 The `triplespace-board` content model

*Sources: [0049](../decisions/0049-boards.md) §3.*

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `triplespace-board` | Triplespace | text | main | `application/json` | Yes | 310 |

It is a **text** model ([0041](../decisions/0041-content-models.md) §5; [10](10-pages-and-content-models.md) §4.3), as `triplespace-table` is ([0045](../decisions/0045-table-content-model.md) §3; [15](15-structured-pages.md)). The definition is stored as text in the board's `page` records and can be changed with `action=edit`. The thread listing is generated, as a table's grid is. The row is in the content-model catalogue, [10](10-pages-and-content-models.md) §4.4.

| Operation (0041 §5) | For `triplespace-board` |
|---|---|
| Validate | Against the definition schema (§3.4). Content that fails is refused |
| Pre-save transform | Canonical JSON: keys in schema order, two-space indentation |
| Serialize | The canonical JSON |
| Plain text for search | Title, `description` and `header` |
| Diff | Line diff of the canonical JSON, with `header` split into its own lines so that an edit to the header diffs as markdown does |
| Render | `description`, then `header` rendered as markdown (§1.6: wiki links, sanitized HTML), then the thread listing ([19](19-site-ui.md)) |

**A board's history** is its own revisions and the composite history of the threads attached to it (§3.7; [16](16-logs-feeds-and-notifications.md)), in one sequence ordered by time. `prop=revisions` on a board title returns both kinds of row, each with its own content model.

### 3.4 The definition

*Sources: [0049](../decisions/0049-boards.md) §4.*

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
| `header` | No | Markdown shown above the listing. Its links become `page_link` rows from the board, as a post's do (§1.6) |
| `sort` | No | `attached` (the default) or `activity`. A viewer can switch without saving ([19](19-site-ui.md)) |
| `homes` | No | Default `true`. When `false`, the board is a **listing board**: threads may be listed on it (§3.5) but not started on it or moved to it. `Board:Requests for comment` is one: each request lives on the talk page it concerns and is listed here. A listing board shows no new-thread form |
| `scope` | No | Accepted at `version: 2` ([0060](../decisions/0060-scopes.md) §6): a scope page or an inline kind of 0060 §4, selecting the threads attached to the talk pages of the scope's members, or one of [0049](../decisions/0049-boards.md) §14's own kinds, `thread_statement` and `boards` ([15](15-structured-pages.md)). Refused at version 1 |

Unknown keys are refused. Changing `homes` to `false` does not move threads already homed on the board. It stops new ones.

### 3.5 A thread has one home and any number of listings

*Sources: [0049](../decisions/0049-boards.md) §5; [0019](../decisions/0019-discussions.md) §2.*

**Every thread has exactly one home.** The home is the thread's attachment of §1.3: `create` sets it and `move` changes it. A home is a `page` target (a subject's talk page, or a board whose `homes` is true) or an `actor` target (a user talk page).

**A thread may also have listings:** further talk pages and boards it appears on. A listing is always a `page` target. Home and listings together are the thread's **attachments**. A thread is never attached twice to the same talk page, and its home is never also one of its listings.

**Why a home.** Four things depend on a thread having one attachment, and none of them makes sense spread across several:

| Depends on it | Where | With several equal attachments |
|---|---|---|
| A talk page encloses its threads | [0016](../decisions/0016-permissions-and-access-control.md) §4 | Listing a thread on a protected board would lock it everywhere |
| Deleting a talk page deletes its threads | [0023](../decisions/0023-moderation.md) §2, §4 | Deleting a board would delete a thread from `Item talk:Q42` too |
| A post's `as:context`, and the `Group` that publishes the thread | [0019](../decisions/0019-discussions.md) §7, [0022](../decisions/0022-federation.md) §7 | No single answer |
| `view.thread.talk_page_id` | [0019](../decisions/0019-discussions.md) §11 | No single row |

So the home keeps every one of these (§3.7), and a listing adds only visibility.

**An actor target is home-only.** A user talk page can be a thread's home. It cannot be a listing, because every post in a thread attached to a user talk page sends that user a `talk` notification, with the banner [0021](../decisions/0021-notifications.md) §6 keeps until it is read. Listing a busy thread there would turn a cross-post into a stream of them. Telling a person about a thread is what a mention does (0021 §2; [16](16-logs-feeds-and-notifications.md) §5.2).

**A limit.** `thread.max_attachments`, a `site` setting (default 5; [23](23-configuration-and-registry.md) §3.2), counts the home and the listings.

### 3.6 Attaching and detaching: records and validation

*Sources: [0049](../decisions/0049-boards.md) §6; [0019](../decisions/0019-discussions.md) §1, §4.*

`attach` lists the thread on another talk page or board; `detach` removes one listing (§1.2). Both carry a summary in the comment part, as `move` does, `target` in the content part and the target's talk page ID in the attestation map (§1.5); `also` on `create` and `keep` on `move` are in the field table of §1.5.

**Validation**, against the thread's state:

- An `attach` or an `also` entry is refused if the target does not exist or is deleted, is an `actor` target, is already attached, or would take the thread past `thread.max_attachments`.
- A `detach` is refused unless the target is a listing. The home is changed with `move`, never removed.
- A `create` or `move` whose home would be a board with `homes: false` is refused.
- A `move` to a page that is currently a listing promotes it: the listing is gone and that page is the home. With `keep`, the old home becomes a listing, and the move is refused if the old home is an `actor` target or the limit would be passed.
- A `pin` and an `unpin` are validated as §4.2 says.

The fold of §1.1 yields the home and the listing set, each with the revision ID of the record that attached it and the time it was pinned there, if it is.

### 3.7 What the home decides, and what every attachment shares

*Sources: [0049](../decisions/0049-boards.md) §7.*

**The home alone:**

- **Encloses the thread.** The enclosure rule of [0016](../decisions/0016-permissions-and-access-control.md) §4 reads **"a talk page encloses the threads whose home it is"** ([09](09-security-and-moderation.md) §4.4). Protecting `Board:General chat` restricts posting in the threads homed there. It does not restrict posting in a thread homed on `Item talk:Q42` and listed on the board. A board's `edit` ACL does govern what can be attached to it or detached from it ([0049](../decisions/0049-boards.md) §13; [09](09-security-and-moderation.md) §8.1).
- **Takes the thread with it when deleted.** Deleting a talk page or a board deletes the threads homed there, by enclosure, as [0023](../decisions/0023-moderation.md) §4 says ([09](09-security-and-moderation.md) §6.2). A thread *listed* on a deleted page is untouched. The listing goes **dormant**: hidden from everyone outside the deletion group, and back if the page is undeleted. Nothing is written to the thread.
- **Is the thread's `as:context`** and the `Group` that announces it as its own ([16](16-logs-feeds-and-notifications.md) §6; [17](17-federation-and-publication.md)).
- **Is the only attachment that can be a user talk page**, so 0021's `talk` notification fires exactly as before.

**Every attachment alike:**

- **Listing.** The thread appears on the page, and in `GET /page/{id}/threads` ([18](18-api.md) §3.2).
- **History.** A talk page's or board's composite history ([16](16-logs-feeds-and-notifications.md)) includes the whole history of every thread attached to it, home or listing, and the attach and detach events. A detached thread takes its rows away, as a moved one does.
- **Feeds and watches.** [0020](../decisions/0020-change-feeds.md) §2's one-target set for a talk page or board is the page and every thread attached to it ([16](16-logs-feeds-and-notifications.md) §4.2). Watching `Board:Requests for comment` watches every request listed there, and with `notify` ([0021](../decisions/0021-notifications.md) §2) sends a notification for each post in them.
- **Search.** A thread can be found by any of its attachments ([03](03-storage-caches-and-search.md) §11).

**Status stays per thread,** as §2.1 says.

**Tenancy is unchanged.** Attachments are tenant-local: a thread cannot be listed on another tenant's talk page. [0028](../decisions/0028-tenancy-policy.md) §6's read-only view of a provider tenant's threads shows each thread once, under its home there ([08](08-tenants-and-instances.md)).

## 4. Pinned threads

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §4; [0049](../decisions/0049-boards.md) §6; [0019](../decisions/0019-discussions.md) §4.*

### 4.1 Pinning is a property of the attachment

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §4.*

**A thread may be pinned on any page it is attached to,** its home or a listing, separately on each. Pinning is a property of the attachment, not the thread: a request for comment pinned on `Board:Requests for comment` is not pinned on the item's talk page where it lives.

### 4.2 Records

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §4; [0049](../decisions/0049-boards.md) §6; [0019](../decisions/0019-discussions.md) §4.*

`pin` pins the thread on one of its attachments and `unpin` unpins it there; both require a base offset and have no text part (§1.2). Each carries `target` in the content part and the attachment's talk page ID in the attestation map, as `attach` does, and a summary in the comment part. A `create` may carry **`pinned: true`**, pinning the thread on its home from the start, which is how an import writes front matter ([0069](../decisions/0069-synchronized-talk-pages.md) §7; [13](13-mirrored-pages.md)).

A `pin` is refused on a page the thread is not attached to, on one where it is already pinned, or past `thread.max_pinned` (`site`, default 3; [23](23-configuration-and-registry.md) §3.2) pinned threads on that page; an `unpin` where it is not pinned. Detaching or moving a thread away from a page ends its pin there; a `move` with `keep` keeps it. The fold yields, for each attachment, the time it was pinned or none.

### 4.3 Rendering, logs and permissions

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §4.*

**Rendering.** Pinned threads come first on the page, in the order they were pinned, marked with a pin, and are **exempt from the age rule** of §2.2, so an announcement does not collapse for being old. The status rule still applies: a pinned thread that is resolved collapses, which is how a pinned call for comments ends without being unpinned. The rest of the rendering is in [19](19-site-ui.md).

**Logs, RDF and federation.** `pin` and `unpin` project as **`thread/pin`** and **`thread/unpin`** ([0011](../decisions/0011-logs.md) §6.1; [16](16-logs-feeds-and-notifications.md) §2.3), typed `as:Add` and `as:Remove` with `as:target` the page's **featured** collection, `{base}/page/{talk page ID}/featured`; the thread node gains `scatter:pinnedOn` ([16](16-logs-feeds-and-notifications.md) §6.3). A federated talk page or board ([0022](../decisions/0022-federation.md) §6; [17](17-federation-and-publication.md)) publishes the same collection as its `featured` property, which is how Mastodon and Lemmy expose pinned posts.

**Permissions** are those of `attach` ([0049](../decisions/0049-boards.md) §13; [09](09-security-and-moderation.md) §8.1): `move` on the thread and `edit` on the page, default `autoconfirmed`, so a board protected to `sysop` decides its own pins.

### 4.4 Talk pages get no front matter of their own

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §4.*

A board keeps the `header` of its definition (§3.4), which describes the board. A talk page gains no header, definition or banner slot: what Wikipedia's banners say about the subject (WikiProject scope and assessment, "this article is about a living person", former nominations) is subject metadata, whose place is page statements ([0038](../decisions/0038-page-metadata-and-categories.md) §1; [06](06-statements-and-properties.md) §5), and what remains (a talk-page FAQ, a standing notice) is a pinned thread.

**Foreign section 0 is pinned by construction.** The front matter of a followed talk page is a foreign thread ([0069](../decisions/0069-synchronized-talk-pages.md) §2; [13](13-mirrored-pages.md)) with `n` like any other, pinned on the local talk page, rendered from section 0's HTML so its banners render as upstream's do, and with Reply controls only on the comments DiscussionTools finds in it (people do sign comments above the first heading). Editing the banners themselves is **Edit on {repository}**, a link to upstream's section 0 editor; it is not done through the instance. Local pins cannot be placed on foreign threads.

## 5. Proposals

*Sources: [0067](../decisions/0067-proposals.md) §1, §2, §3, §5; [0038](../decisions/0038-page-metadata-and-categories.md) §9.*

### 5.1 A proposal is a thread with a payload

*Sources: [0067](../decisions/0067-proposals.md) §1.*

**A proposal is a thread** homed on the subject's talk page — the foreign entity's, or the fork's — whose records carry a **proposal payload** (§5.3): what is offered, to which wiki, compiled from the local state at the moment of proposing. The thread is where the proposal is discussed and where its **state** (§5.4) is shown; threads give watching, feeds, boards and notifications for free, and a listing board such as `Board:Proposals to Wikidata` with a `thread_statement` scope ([0049](../decisions/0049-boards.md) §14; [15](15-structured-pages.md)) lists them all.

Two kinds, one object:

| Kind | Subject | What is proposed | Compiled by |
|---|---|---|---|
| **Entity** | A foreign entity with local assertions ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7; [05](05-providers-and-ingest.md) §2.5) | A Wikibase change set: claims, ranks, terms | §5.2 |
| **Page** | A fork ([0054](../decisions/0054-forking-a-mirrored-page.md)) | Wikitext for a destination page on the repository | [0068](../decisions/0068-merging-with-upstream.md) §3 |

A page proposal's payload, destination and conflict handling are [0068](../decisions/0068-merging-with-upstream.md) §3's; its adoption test is 0068 §5's ([13](13-mirrored-pages.md)).

**The destination wiki** is the one the subject is mirrored from: the entity's provider (Wikidata for a `WD` subject), or the fork's repository; where a title's stack has several repositories ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §2), the first in `pages.repos` order.

**Not yet.** A proposal to any other wiki than the one the subject is mirrored from is an open question of [0067](../decisions/0067-proposals.md).

### 5.2 Compiling an entity proposal

*Sources: [0067](../decisions/0067-proposals.md) §2.*

**The proposer chooses which local assertions to offer**, from the entity page's overlay (the correction chips of [0003](../decisions/0003-statement-ui.md) §4; [19](19-site-ui.md)) or from `Special:Corrections` with checkboxes ([21](21-special-pages.md)); the default selection is every active assertion on the subject. Each is **translated** into upstream's terms:

| Local assertion ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7) | Proposed as |
|---|---|
| `add` statement | A new claim, with its qualifiers and references |
| `override` rank | The upstream statement's rank set to the local rank |
| `override` suppress | The upstream statement's rank set to **deprecated** (direction); never a removal, which is a human decision upstream |
| `override` term | The label, description or alias set |
| `add` on a local-only property or with a local-only value | **Untranslatable** (below) |

**Translation** goes through the cluster: a local or other-provider entity used as a value is replaced by its cluster's member on the destination wiki ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §4; [04](04-entities-and-identifiers.md)), a local property by its `equivalent-property` there (0004 §6), a mirrored `WDP…` by its bare upstream ID. Anything with no member there is **untranslatable**: listed on the proposal with the reason ("`P7` has no equivalent on Wikidata"; "`Q88` is not linked to a Wikidata item"), left out of the payload, and kept in the proposal's `omitted` list so that linking the entity later lets the proposer re-compile. A statement without a reference is **flagged**, not refused: Wikidata's norms want one, the proposer decides.

**The payload** is Wikibase's canonical JSON for `wbeditentity`'s `data` — `claims`, `labels`, `descriptions`, `aliases` with upstream IDs and GUIDs where a statement is changed — plus **QuickStatements V1** text generated from the same change set, so that the person can push with the tool they already use. Both are produced by `scatter-wikibase-changeset` ([22](22-crates-and-stack.md)) from the selection and the cluster map, deterministically, and stored in the record (§5.3).

### 5.3 Records

*Sources: [0067](../decisions/0067-proposals.md) §3.*

Three operations of the `scatter:v0/thread` payload type (§1.2) belong to proposals:

| Operation | Content part | Meaning |
|---|---|---|
| `propose` | `kind` (`entity` \| `page`); `target` `{wiki, id}` (the destination: provider and upstream ID, or repository and title); `base` (the upstream revision the payload was compiled against); `payload` (§5.2, or [0068](../decisions/0068-merging-with-upstream.md) §3's text and destination); `omitted`; `flags` | Turns the thread into a proposal, or re-compiles one; the latest `propose` is the proposal |
| `submit` | `method` (`export` \| `push`); `upstream` (the revision IDs the push made, or none for an export); `destination` (for a page) | The proposal was offered: exported for the person to push, or pushed as them ([17](17-federation-and-publication.md)) |
| `withdraw` | — | The proposer takes it back |

**A proposal's kind, destination, base, payload, omitted and flags are page metadata of the thread page** ([0038](../decisions/0038-page-metadata-and-categories.md) §1; [06](06-statements-and-properties.md) §5), set by the `propose` operation and projected to `view.proposal` (§5.4). They are not fields of `create`, whose `target` stays the thread's home (§1.5): opening a proposal thread is a `create` followed by a `propose`. Each of the three needs a base offset; `propose` and `withdraw` need to be by the thread's author or a holder of `edit` on the subject's talk page; `submit` by the person who pushed or exported. **Validation**: a `propose` on a subject that is not foreign (entity) or not a fork (page) is refused (`ts-proposal-subject`); a `submit` with `method: push` without a grant ([0067](../decisions/0067-proposals.md) §6; [17](17-federation-and-publication.md), [07](07-actors-and-accounts.md) §7.7) is refused.

**Logs**: `proposal/propose`, `proposal/submit`, `proposal/withdraw`, projected from the records, visible to everyone, typed `as:Offer` (propose, with `as:target` the upstream IRI), `as:Announce` (submit) and `as:Undo` (withdraw) ([16](16-logs-feeds-and-notifications.md) §2.3).

Export and push are in [17](17-federation-and-publication.md); the proposal thread's page and the routes in [19](19-site-ui.md) and [18](18-api.md) §3.2; permissions and `proposals.max_items` in [09](09-security-and-moderation.md) §8.10.

### 5.4 State, and closing the loop

*Sources: [0067](../decisions/0067-proposals.md) §5; [0038](../decisions/0038-page-metadata-and-categories.md) §9; [0083](../decisions/0083-write-path-in-three-tiers.md) §6.*

**A proposal's state is projected**, not set by a post:

| State | When |
|---|---|
| `draft` | A `propose` with no `submit` since |
| `offered` | A `submit` (either method), and upstream not yet seen to carry it |
| `adopted` | The mirror of the target (a `put`, [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4, or the page's revision through [0053](../decisions/0053-mirrored-pages.md) §6) carries every item of the payload: the claim present with the proposed value, the rank as proposed, the term as proposed; for a page, [0068](../decisions/0068-merging-with-upstream.md) §5's test |
| `partly adopted` | Some items carried, after `proposals.adoption_grace` (default 7 days) since the submit |
| `reverted` | Upstream carried the items and later does not |
| `declined` | Set by a post with the `declined` status (§2.1), when the destination community said no. On a proposal thread `declined` may be set only by the proposer or a holder of `edit` on the subject's talk page; any other `declined` post is refused with `ts-proposal-status` |
| `withdrawn` | A `withdraw` |

`proposals.adoption_grace` is a tenant `site` setting ([23](23-configuration-and-registry.md) §3.2).

`view.proposal (tenant, thread_id, kind, target, state, submitted_at, upstream_revids, adopted_at, items jsonb)` holds it, written by the mirror and page-mirror projections when the target changes and by the thread projection on records; it is a row of the schema map and of [03](03-storage-caches-and-search.md) §5. **The projection writes `view.proposal.state` and nothing else**: it has no side effect during replay, so a rebuild re-derives every state and re-runs no retirement ([0083](../decisions/0083-write-path-in-three-tiers.md) §6).

**The role `proposal-state`**, bound by the tenant to a `string` property as `thread-status` is (§2.3), projects `view.proposal.state` as a statement on the thread, so proposals can be scoped, listed and counted like any statement: a scope `statement: proposal-state = offered` over threads, a sprint rule, a board ([15](15-structured-pages.md)).

**Redundant assertions.** When a proposal becomes `adopted`, each local assertion it carried is now **redundant** in [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7's sense: upstream agrees ([05](05-providers-and-ingest.md) §2.5). With **`upstream.retire_adopted = true`** (`site`; the default, by direction; [23](23-configuration-and-registry.md) §3.2) the instance **retires** each redundant assertion — a `remove` of the local assertion as an **instance act** ([0040](../decisions/0040-instance-prerogatives.md) §7; [08](08-tenants-and-instances.md)), summary "Adopted upstream in revision {revid}; proposal {thread}" — so the entity's resolved view is upstream's and the overlay stops accumulating. **Retirement is an `ops` job, never a projection step.** When the projection sets the state to `adopted` it enqueues the retirement; the job re-checks the current composed state of the entity (that upstream still carries each item and the local assertion is still present) and only then appends the `remove`, carrying the proposal thread's page ID and the adopting upstream revision in its content part as an **idempotency key**, so a repeated enqueue, a retry or a rebuild appends nothing twice. The `ops` queue is not a projection target and is never truncated by a rebuild ([0083](../decisions/0083-write-path-in-three-tiers.md) §6). With `false`, they are listed in `Special:Corrections` as `redundant` with **Retire**. If the proposal later becomes `reverted`, a retired assertion is **offered for restoration** on the thread ("Upstream reverted this; restore the local statement?"), one click re-adding it; it is not restored automatically, since the revert may have been right.

**Notifications** ([0021](../decisions/0021-notifications.md) §2; [16](16-logs-feeds-and-notifications.md) §5.2): one reason, `proposal-state`, to the proposer when the state changes to `adopted`, `partly adopted`, `reverted` or `declined`; the activity row is the mirror record or status post that changed it.
