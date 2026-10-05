# 0054. Forking a mirrored page

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-05 (A1)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0024](0024-subsidiary-accounts.md), [0030](0030-edit-filters.md), [0033](0033-backend-stack.md), [0039](0039-files-and-media.md), [0047](0047-special-pages.md)
- **Uses:** [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0023](0023-moderation.md), [0035](0035-adopting-a-wikibase.md), [0042](0042-template-expansion-and-parsoid.md), [0043](0043-lua-modules.md), [0049](0049-boards.md), [0051](0051-page-redirects.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [MediaWiki API contract](../api/mediawiki-compat.md)

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

**A fork is an ordinary local page in the tenant's `pages` partition,** with a page ID from the tenant's sequence, a `wikitext` model and the full history of any local page. What makes it a fork is its first record: a **`create`** whose content part carries **`forked_from`**:

```json
{"op":"create","title":"Myocardial infarction","model":"wikitext",
 "forked_from":{"repo":"enwiki","page_id":19927,"revid":1234567890,"time":"2026-09-30T14:02:11Z"}}
```

- **Its text is the upstream revision's wikitext**, the one the editor was shown ([0053](0053-mirrored-pages.md) §1), so the fork's base is exactly what was edited and the editor's change has a diff.
- **Its actor is the upstream revision's author,** resolved under the repository's issuer ([0052](0052-page-repositories-and-title-inheritance.md) §1, [0007](0007-actor-identity.md) §5): a registered user by numeric ID, an IP edit as an `anonymous` surrogate, a hidden user as a marker. Its summary is the upstream summary; its upstream timestamp is kept as upstream metadata, as [0008](0008-namespaces-and-document-pages.md) §9 keeps one. The record's own time is the time of the fork, and its attestation names the forking user and the fork job (§3), so the attribution is the upstream author's and the responsibility is the forker's, as for an import.
- **It is tagged `fork`** and projects as **`import/interwiki`** in the log ([0011](0011-logs.md) §6.1), with the repository as the interwiki and the upstream revision in its parameters, as an adopted entity projects as `import/upload` ([0035](0035-adopting-a-wikibase.md) §6). The activity row is an `edit` with `new` set.

**The forker's own edit follows as the second record,** an `edit` whose base offset is the `create`'s, in the same transaction (§2). A fork made without a change (§2, `Special:Fork`) has only the first.

**The stack changes at once.** The title now has a local primary ([0052](0052-page-repositories-and-title-inheritance.md) §2); the repository's page is an alternate, offered or hidden as the repository and the page say. A fork tracks nothing: the repository's later revisions do not reach it, and nothing here ever writes to the repository. That is the direction, and §6 is how a fork sees what it has diverged from.

### 2. Forking on save (extends 0010 §4; extends 0012 §4)

**The Edit tab of a foreign primary is the fork form.** It opens the source editor on the bundle's wikitext at the upstream revision the reader was shown, with the banner: *"This page is from English Wikipedia. Saving creates a local copy that this wiki owns. Later changes on English Wikipedia will not be applied to it automatically."* Section edit links on a mirrored page ([0053](0053-mirrored-pages.md) §2) open the same form at the section. Preview runs through `action=parse` with the tenant's own expander ([0042](0042-template-expansion-and-parsoid.md) §15), so the editor sees how the page will render **here**, over the tenant's stacks, before committing.

**Saving appends the `create` of §1 and the editor's `edit`, in one transaction with one base check:** the base is the upstream revision named in the form, and the save is refused with `editconflict` if the repository's page has moved on since the form was opened, exactly as a local edit conflicts, with the editor's text kept. On success the response reports the new page ID, the two revision IDs and the job (§3).

**The API forks only when asked.** `action=edit` on a title whose primary is foreign is refused with **`ts-fork-required`** unless the request carries **`tsfork=1`**, with **`tsforkrevid`** naming the upstream revision edited (default: the bundle's current). A bot that saves to inherited titles without knowing they are inherited therefore learns it at once rather than making a fork by accident; `nocreate` is refused the same way, since the title has no local page. With `tsfork=1`, `section=N` edits the bundle's section, `appendtext` and `prependtext` apply to the bundle's text, and `createonly` is satisfied. `tsfork` is exposed to edit filters as `fork` in the page context ([0030](0030-edit-filters.md) §2), with the repository and upstream revision beside it, so a tenant can gate or tag forks.

**`Special:Fork/{title}` forks without a change.** It is a form for the reader who wants a page kept as it is, protected from upstream, or ready for annotation: it shows the stack, the licence and the options of §4–5, and appends the `create` alone. The same options are collapsed under "Fork options" on the edit form, with the tenant's defaults. The page is in `docs/registry/special-pages.toml`, origin `triplespace`, group `pagetools`, restricted to `createpage`.

**With expansion off,** the `create`'s text is the **flattened** text, fetched from the repository with `action=expandtemplates` as the import of [0008](0008-namespaces-and-document-pages.md) §9 step 3 flattens it, tagged `flattened`, because an unexpanded Wikipedia article on a tenant without expansion is a page of chips. With `wikitext.expansion = on` the text is the raw source and the dependencies of §4 make it render.

### 3. History is seeded afterwards, from a RevisionChest store or the API (extends 0015 §4; extends 0035 §7; amends 0033 §10)

**The save never waits for history.** A Wikipedia article can have twenty thousand revisions, and the log cannot take them behind the page's `create` once it is appended ([0035](0035-adopting-a-wikibase.md) §7). So the fork's history is **shape (b)** of 0035 §7, applied to a page: a job writes the upstream revisions as **upstream revision records** ([0015](0015-record-format-and-partition-registry.md) §4) into the tenant's `log` partition, keyed by the fork's page ID, and the history view folds them beneath the `create`.

**The record** is `scatter:v0/upstream-revision` as 0015 §4 defines it, with one addition: **a fourth part, `text`**, holding the revision's wikitext, so that hiding or erasing an old revision's content ([0011](0011-logs.md) §5) touches the text and nothing else, as a post's text part is kept apart from its structure ([0019](0019-discussions.md) §4). The content part carries the upstream revision and parent IDs, timestamp, size, SHA-1, model, tags and flags; the comment part the summary; the attestation the upstream actor under the repository's issuer, or a hidden marker, and the job. Header field 7 is the **provider-ranged** revision ID, `number << 40 | upstream revision ID` ([0013](0013-postgres-storage.md) §6), so `oldid=` with that number serves the revision and no local revision ever shares it.

**The source is `fork.history_source`,** a `site` setting:

| Value | Reads |
|---|---|
| `chest:{path}` | A RevisionChest store for the repository, its `.mwrev.zst` files and index, through the reader in `scatter-adapter-mediawiki` ([0053](0053-mirrored-pages.md) §12); then the repository's API for revisions newer than the store, as a `dump:` backfill closes its gap ([0015](0015-record-format-and-partition-registry.md) §4). This is the setting James intends for Wikipedia, where the Internet Archive's store exists |
| `api` *(default)* | The repository's `prop=revisions` with `rvprop=ids\|timestamp\|user\|userid\|comment\|size\|sha1\|tags\|flags\|content`, paged, under the repository's fetch budget ([0053](0053-mirrored-pages.md) §1) |

[0033](0033-backend-stack.md) §10's "if a RevisionChest store is used as the local source for upstream history" now covers pages: the store receives the same hiding sweep, read from the repository's `revision-visibility-change` events and deletion log ([0011](0011-logs.md) §5), and a revision the sweep hides is erased from the tenant's `log` with reason class `upstream`.

**`fork.max_revisions`** (`site`, default 20,000) caps what is seeded with content. Beyond it the oldest revisions are seeded as metadata only, so the history is complete and diffable for the recent past and complete as a list for all of it; the History tab says so and links upstream for the rest.

**Serving.** `view.upstream_revision` ([0015](0015-record-format-and-partition-registry.md) §4) gains rows whose `entity_id` is the page ID in decimal, as page statements use it ([0038](0038-page-metadata-and-categories.md) §1). The History tab lists them beneath the `create`, each with the upstream actor linked to its IRI, under a fold "Imported from English Wikipedia" like the one for upstream edits ([0010](0010-site-ui.md) §5.5), but stored. `prop=revisions` lists them with ranged IDs and serves their text; `action=compare` diffs any two revisions, upstream or local; `Special:Export` with full history writes them. While the job runs, the History tab shows "History is being seeded: 4,120 of 18,377 revisions", and the page is fully usable.

### 4. Dependencies: templates, modules and stylesheets (uses 0042 §11 and §13, 0043 §2, 0055 §2)

**With `wikitext.expansion = on`, the pages a fork needs to render come with it,** so that the fork renders here with the templates it was written against, as James directed. The fork job reads the bundle's list of templates used ([0053](0053-mirrored-pages.md) §1), which MediaWiki computes transitively (`prop=templates`): every Template, Module and TemplateStyles page (`Template:X/styles.css`, [0055](0055-templatestyles-templatedata-and-page-properties.md) §2) the article's render transcluded. For each title:

- **if a local page exists under it, nothing happens:** the fork uses it, as it would have through the stack ([0052](0052-page-repositories-and-title-inheritance.md) §3), and a later fork of another article reuses the templates the first fork brought;
- **otherwise it is forked as §1 forks a page:** a `create` carrying `forked_from` with the repository's current revision of the template, attributed upstream, tagged `fork-dependency`, with its history seeded by the same job (§3). Module pages need `wikitext.lua = on` ([0043](0043-lua-modules.md) §2) and are skipped, and reported, without it.

**`fork.dependencies`** (`site`) chooses: **`copy`** (the default, and the direction) or **`inherit`**, which forks the article alone and leaves its templates to the repository, for a tenant that would rather its forks keep rendering with Wikipedia's current templates. **`fork.max_dependencies`** (default 2,000) stops a job that would copy more than that, which a citation-heavy article's module tree can approach; the pages not copied stay inherited, so the fork still renders, and the job reports what it left. This is the one place the design is forgiving by construction: an incomplete copy is not a broken page, because the stack answers for whatever is missing.

**Dependencies are forked once, not per fork.** A second fork whose dependencies are already local copies nothing and reports it. Dependency forks are listed on the fork job's page and on each dependency's identity line ("Forked with *Myocardial infarction*").

**TemplateStyles sheets** are forked with their template, and the fork of a Wikipedia template that uses `<templatestyles>` renders here through [0055](0055-templatestyles-templatedata-and-page-properties.md). **Site styles** are not a page the repository holds in a form the tenant can fork (`MediaWiki:Common.css` is reserved here, [0008](0008-namespaces-and-document-pages.md) §2); a tenant that wants Wikipedia's look supplies its own site-styles page ([0055](0055-templatestyles-templatedata-and-page-properties.md) §4).

### 5. The talk page: sections become closed threads (amends 0019 §3, §4, §5, §6; settles 0019 Q5; extends 0008 §9)

**The repository's talk page is converted into threads homed on the fork's talk page.** The fork job fetches `Talk:{title}` at the fork's upstream revision time, and every archive subpage under it (`Talk:{title}/Archive 1` and the rest, found with `list=allpages&apprefix=`), and splits each by its **level-two headings**:

| Piece | Becomes |
|---|---|
| **The text above the first heading**: the frontmatter of banners, WikiProject templates and talk headers | One thread, subject "*{title}*: talk page header", holding that text as its opening post. The direction says to treat the frontmatter as a section, and this is how |
| **Each `== Section ==`**, with its subsections | One thread whose subject is the heading's text and whose opening post is the whole section's wikitext, signatures, indentation and subsections intact |
| **Each archive subpage** | The same, with each thread's content part naming the archive it came from |

**Every thread is created closed.** The `create` record carries `status: archived`, a new default status in `docs/registry/thread-statuses.toml` ([0019](0019-discussions.md) §6): category `closed`, label **Archived**, ordered after Stale. [0019](0019-discussions.md) §4 is amended so that `status` may appear on `create` as on `post`, which says exactly what an import means: the thread arrived already over. Anyone who may post can reopen one.

**The post is wikitext.** [0019](0019-discussions.md) §5 made posts markdown. The thread payload's text part gains a media type: the content part's **`mediaType`** is `text/markdown` by default and **`text/x-wiki`** for a post an import writes. A wikitext post renders through the tenant's wikitext pipeline, with expansion, so `{{WikiProject Medicine}}` and `{{Talk header}}` render through the stack ([0052](0052-page-repositories-and-title-inheritance.md) §3) without being forked, and `~~~~` signatures stay the text they were. No editor composes a wikitext post; only an import writes one, and editing such a post keeps its media type. The AS2 profile ([0019](0019-discussions.md) §10) emits it as `as:source`'s `mediaType`.

**Attribution and dates.** A section has many authors, so the opening post is attributed to the forking user and the job, with the content part's **`imported_from`** naming the repository, the talk page, its revision and, for an archive, the subpage. [0019](0019-discussions.md) §3 made a thread's title date the date of its `create` record; for an imported thread it is **the date the import supplies**: the earliest signature timestamp the section's text parses, or the talk page revision's time. So `Thread:2019-04-12/Merge proposal` is what the section was, not when it arrived, and the thread's `as:published` is the import time while its identity line reads "originally on English Wikipedia Talk:…, 12 April 2019".

**The talk page's own history comes too.** Its upstream revisions, and each archive's, are seeded as upstream revision records keyed by the **fork's talk page ID**, as §3 seeds the article's, so the talk page's composite history ([0019](0019-discussions.md) §7) gains rows "upstream revision of Talk:…" with their text at `oldid=`, and nothing of what was said is lost to the splitting.

**Local threads move.** Threads that readers had attached to the foreign page's talk page, on its ranged page ID ([0052](0052-page-repositories-and-title-inheritance.md) §7), are moved to the fork's talk page by the job, one `move` record each, so discussion that began before the fork continues on it.

**0019 Q5 is settled by this conversion,** and `Special:Import` ([0008](0008-namespaces-and-document-pages.md) §9) uses it for any talk namespace in an XML export: a wikitext talk page is imported as closed threads on the subject's talk page, split by heading, with the frontmatter as the first. A village pump imported this way lands on a board ([0049](0049-boards.md) §1) the importer names.

### 6. What a fork knows about upstream (extends 0010 §2 and §4; uses 0001 §1)

*Changed by A1.*

- **Provenance.** The fork's first revision node carries `prov:wasDerivedFrom` the upstream revision's IRI, `{article path}Special:Redirect/revision/{revid}` ([0015](0015-record-format-and-partition-registry.md) §4), and `pav:importedFrom` the repository's page; `GET /page/{id}/provenance` reports the repository, the upstream page and revision, the job and the dependency forks. The seeded revisions carry the upstream vocabulary of [0001](0001-revision-metadata-rdf.md) §1 as any backfilled revision does.
- **The identity line** reads "Forked from English Wikipedia at revision 1234567890 (30 September 2026)", linking both; the **About this page** panel ([0010](0010-site-ui.md) §4) shows the revisions imported, the dependencies copied, whether files were copied (§7), and **"English Wikipedia has 14 newer revisions"**, from a live `prop=info` on the repository, cached and rate-limited as an upstream fetch is ([0012](0012-api-requirements.md) §6).
- **Compare with upstream.** `GET /page/{id}/upstream-diff` *(REST v0)* and a link in the panel diff the fork's **base** (the `create`'s text) against the repository's **current** wikitext, and the fork's current text against it, so an editor can see what upstream changed since the fork and what the fork changed since upstream. The diff is computed live and never stored. Its base is the fork's `create` or, once the fork has pulled, its latest `merge` record. **Merge from upstream** applies upstream's changes as a guided three-way merge, and **Propose to the repository** compiles the fork's changes the other way ([0068](0068-merging-with-upstream.md) §2–3).
- **The attribution line** of [0053](0053-mirrored-pages.md) §9 stays on a fork for as long as the page exists, reading "Forked from English Wikipedia; the original authors are listed in this page's history", which is what the licence asks and what the seeded history provides.

### 7. Files, on request (extends 0039 §14)

**A fork does not copy files.** Its `[[File:…]]` embeds keep resolving through the tenant's file repositories ([0039](0039-files-and-media.md) §11), as they did on the mirrored page.

**"Copy files used by this page"** in the About panel, and `POST /page/{id}/fork/files` *(REST v0)*, start a separate job that imports each file the page's render uses, as the directory import of [0039](0039-files-and-media.md) §14 does: the latest version by default, or every version with `history=all`, each as an `upload` record with action `import`, attributed to its upstream uploader under the file repository's issuer, with the file's description page forked as §1 forks a page. A file already local, or one the tenant's file repository holds in `mirror` mode, is skipped and reported. The job needs `upload` and `reupload-shared` ([0039](0039-files-and-media.md) §21), since each copy shadows a foreign file.

### 8. The job, permissions and limits (extends 0016 §2; extends 0024 §5)

**Everything after the save is one `ops` job per fork,** `ops.fork`, like the export job of [0047](0047-special-pages.md) §8 and not a bulk ingest job, since no subsidiary runs it: it is attributed to the forking user, who started it by saving, and its record names the fork, the repository, the upstream revision and the counts. It runs the history seed (§3), the dependencies (§4) and the talk page (§5) in that order, reports on `Special:Jobs/{id}` ([0010](0010-site-ui.md) §9), and can be rerun to complete a partial seed; it never touches the fork's `create` or the forker's edit. The records it writes are attested to the forking user and the job, as an import's are to the importer.

| Action | Needs | Default |
|---|---|---|
| Fork by saving, or `Special:Fork` | `createpage` and `edit` on the title ([0016](0016-permissions-and-access-control.md) §2); the title not create-protected ([0023](0023-moderation.md) §2); `fork.allow` on | `user`, `temp` |
| The history, dependency and talk-page seeding | Nothing beyond the fork: it is part of what saving means here. **`import` is not needed**, which is the point: forking is an edit, not an administrator's import |
| Copying files (§7) | `upload`, `reupload-shared` | `user`; `reupload-shared` as [0039](0039-files-and-media.md) §21 has it |
| `fork.*` settings | `ts-config` | |

**`fork.allow`** (`site`, default `on` where `pages.repos` is set) lets a tenant run a pure mirror that nobody forks. **A new rate class, `fork`** ([0024](0024-subsidiary-accounts.md) §5), counts forks started: 5 per hour for `user`, 100 for `bot`, because each one is a job that may write thousands of records and fetch thousands of revisions. The seeding itself runs under the repository's fetch budget ([0053](0053-mirrored-pages.md) §1), not the user's.

### 9. Deleting, moving and re-forking

*Changed by A1.*

- **Re-following upstream** is deleting a fork that no longer differs from the repository, offered under `fork.refollow = ask` and automated under `auto` ([0068](0068-merging-with-upstream.md) §5).
- **Deleting a fork** is a `read` ACL on its page ID ([0023](0023-moderation.md) §4). The title's stack loses its local primary and the repository's page is primary again ([0052](0052-page-repositories-and-title-inheritance.md) §2): deleting a fork is how a tenant goes back to the mirror. The threads homed on its talk page go with it, by enclosure, and come back with undeletion. A new fork at a deleted fork's title is an ordinary create; the deleted page keeps its history, as a deleted page does.
- **Moving a fork** is an ordinary move and leaves a redirect ([0051](0051-page-redirects.md) §3). The old title's redirect is now the local primary there, so a reader arriving by the old name lands on the fork, while the repository's page at that title becomes an alternate of a redirect, which is listed as such.
- **Redirects are not forked.** The repository's redirects to a forked title keep resolving through the stack to the fork ([0051](0051-page-redirects.md) §2); a tenant that wants a redirect local creates it.
- **Forking a redirect** forks the redirect page, not its target; the form says so and offers the target.

### 10. Storage (extends 0013 §5.6)

```sql
CREATE TABLE view.fork (                        -- one row per fork, article or dependency (§1, §4)
  page_id bigint PRIMARY KEY REFERENCES view.page,
  repo text NOT NULL, upstream_page_id bigint NOT NULL, upstream_revid bigint NOT NULL,
  upstream_time timestamptz NOT NULL, forked timestamptz NOT NULL, forked_by text NOT NULL,
  kind text NOT NULL,                             -- article | dependency | talk | file-page
  parent_fork bigint,                             -- the article fork a dependency came with
  job_id bigint,
  history_state text NOT NULL,                    -- pending | seeding | complete | partial | metadata-only
  revisions_seeded integer NOT NULL DEFAULT 0, revisions_total integer,
  files_copied timestamptz
);
CREATE INDEX fork_repo_page ON view.fork (repo, upstream_page_id);
```

`view.upstream_revision` takes page subjects as §3 says. `ops.fork` is the job queue. `view.page` is unchanged: a fork is a page.

### 11. API (extends 0012 §4 and §5)

| Module or route | Behaviour |
|---|---|
| `action=edit` | `tsfork`, `tsforkrevid`; `ts-fork-required` (§2). The response carries `fork: {pageid, create_revid, job}` |
| `prop=revisions`, `action=compare`, `prop=info` | Seeded revisions with ranged IDs (§3); `prop=info` gains `forkedfrom` beside `origin` |
| `list=logevents&letype=import` | Forks, as `import/interwiki` (§1) |
| `POST /page/fork` *(REST v0)* | `{title, repo, revid, text?, summary?, dependencies?, talk?}`: a fork with or without an edit; returns the page, revisions and job |
| `GET /page/{id}/fork` *(REST v0)* | The `view.fork` row: origin, state, counts, dependencies, files |
| `GET /page/{id}/upstream-diff` *(REST v0)* | §6; `against=base` or `current` |
| `POST /page/{id}/fork/files` *(REST v0)* | §7 |
| `GET /page/{id}/provenance` *(REST v0)* | Gains the fork's origin and job (§6) |
| `Special:Fork/{title}` | §2 |

### 12. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-pages` | `forked_from` on `create` |
| `scatter-threads` | `status` on `create`; `mediaType` and `imported_from` in the content part; the title date an import supplies (§5) |
| `scatter-log` | The `text` part of `scatter:v0/upstream-revision` (§3) |
| `scatter-adapter-mediawiki` | Page revisions from a RevisionChest store and from `prop=revisions`, as upstream revision records with text; the talk-page splitter (§5): sections, archives, signature timestamps |
| `triplespace-repos` | The fork job (§8): seeding, dependencies, talk conversion, thread moves, the file job, the upstream diff |
| `triplespace-titles` | `Special:Fork` |
| `scatter-mwlog` | `import/interwiki` for a fork, with its parameters |
| API crates | §11 |

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
