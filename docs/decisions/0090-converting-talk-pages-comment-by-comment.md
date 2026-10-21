# 0090. Converting talk pages comment by comment

- **Status:** Proposed
- **Date:** 2026-10-20
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0013](0013-postgres-storage.md), [0019](0019-discussions.md), [0047](0047-special-pages.md), [0054](0054-forking-a-mirrored-page.md), [0069](0069-synchronized-talk-pages.md)
- **Uses:** [0007](0007-actor-identity.md), [0015](0015-record-format-and-partition-registry.md), [0035](0035-adopting-a-wikibase.md), [0049](0049-boards.md), [0053](0053-mirrored-pages.md), [0085](0085-settings-and-preferences.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

[0054](0054-forking-a-mirrored-page.md) §5 converts a wikitext talk page into threads section by section. Each level-two section becomes one thread, created `archived`, whose opening post is the whole section as wikitext. The text above the first heading becomes a pinned, open thread. Every post is attributed to the person who started the job. The same conversion serves **Fork the talk page**, **Stop following** ([0069](0069-synchronized-talk-pages.md) §7) and `Special:Import` ([0008](0008-namespaces-and-document-pages.md) §9). Two questions were left open: whether imported posts could carry their signers' names (0054 Q4), and how an adopted wiki's talk pages would come across ([0008](0008-namespaces-and-document-pages.md) A41 deferred them).

A section-sized post keeps everything that was said, but it is not a discussion. Nobody can reply to one comment, the tree that indentation drew is lost, and everyone who took part is credited to whoever ran the import. The research of 2026-10-20 (`claude/talk-page-conversion.md` in the project notes) asked how far a comment-level conversion can get with simple rules, against nine English Wikipedia pages chosen for range, and one user talk page with its full history. The pages were Wikipedia:Reference desk/Science, Wikipedia:Administrators' noticeboard/Incidents, Talk:Barack Obama/Archive 1 and Archive 84, Wikipedia:Requests for adminship/Vermont, Talk:Jesus/Archive 1 and Archive 139, Talk:Gdańsk/Vote and User talk:Jimbo Wales/Archive 256; the history was User talk:Harej's 4,677 revisions from 2004 to 2026. `tools/talk_dumps.py` fetched them, with DiscussionTools' own parse of each revision as the reference.

**DiscussionTools.** The extension parses a rendered page into headings and comments. A comment ends at a signature, a user link followed by a timestamp in the wiki's current date format. Its level comes from indentation, and it gets a name and an ID derived from its author, its timestamp and its parent. It does not recognise older date formats, so it finds no comments on Talk:Gdańsk/Vote (2005, `18 Feb 2005` and `Feb 18, 2005`) and one on Talk:Jesus/Archive 1 (2002–03, mostly undated signatures). It merges unsigned text into the next signed comment, which on Talk:Barack Obama/Archive 1 credits Shakam's comments to HailFire.

**A simple splitter.** A prototype of about 150 lines of Python works on the wikitext with the rules of §2:
- **Comments:** of the 2,177 comments DiscussionTools finds on the nine pages, it finds 2,176.
- **Parents:** it agrees on the parent for 2,059 of the 2,111 that can be compared (97.5%). Most of the rest are cases that are ambiguous in the text itself, where a comment's first line and its signature line are indented differently, or DiscussionTools' own structural quirks.
- **Old pages:** with the older date formats and undated signatures it finds 894 comments on Talk:Gdańsk/Vote, 86 on Talk:Jesus/Archive 1 and 124 on Talk:Barack Obama/Archive 1, against DiscussionTools' 0, 1 and 86.
- **False positives:** the undated rule adds none on the seven modern pages.
- **Markup across comments:** six wrapper template pairs and two tables enclosed more than one comment among 3,195 comments.

**History.** A line-level blame over User talk:Harej's history ran in about eight seconds. Of the 3,132 signatures that first appear in some revision:
- 85.8% were added by the signer's own account.
- 8.9% were added by an account with another name. These are mostly renames and alternate accounts: 73 by `Messedrocker`, the page owner's former name. The rest are comments moved from elsewhere.
- 4.8% were delivered by bots and signed by the sender.
- 13 were added by SineBot signing for someone else.
- 98.8% of self-signed comments carry the same minute as their revision.
- 5.0% of comments had their signature line changed later by their signer.

Neither source is enough alone. A signature name looked up in today's user table gets usurped names wrong, since `Drecker` in 2006 is not today's `Drecker`. History credits a moved comment to whoever moved it. Archives are created by bots copying text, so the edits that wrote an archive's comments are in the parent talk page's history. Seeding stores text only for the last 100 revisions ([0054](0054-forking-a-mirrored-page.md) A5), so an attribution pass cannot rely on stored history.

### Direction

James's direction, from the design discussion of 2026-10-20:

- **"While getting as close to full coverage of wiki talk page foibles is ideal, it should not come at the expense of making sure that the most common scenarios are covered well."** LiquidThreads and Flow are not a focus.
- **"One post per comment is right. Fall back to whole-section post when having trouble parsing the page, or whole-page post for truly unsalvageable cases."**
- **"Your solution for headings sounds reasonable. Good solution for attribution as well."**
- **"I feel like signature date formats should be part of a universal registry rather than per-wiki, since the same formats will exist on multiple wikis."**
- **"I think your finding that you can stream the data from RevisionChest and use it without persisting everything is correct, allowing us to preserve attribution."**
- **"Since the 100 revision limit is arbitrary, I propose making it possible to waive and include every revision, for projects intending to operate at that scale. It's a sensible default to prevent surprise blow-ups but at some point I will want to aim for completion."** And: "I like 'all' as an explicit indicator."
- **"DiscussionTools comment IDs are worth preserving."**
- **"Testing against DiscussionTools is certainly encouraged."**

## Decision

### 1. One post per comment (amends 0054 §5; settles 0054 Q4)

**The conversion keeps 0054 §5's threads and splits their posts.** As before:
- Each level-two section of a talk page and its archives becomes one thread, homed on the talk page and created `archived`.
- The thread's title date is its earliest comment's timestamp, or the talk page revision's time.
- A project-wide discussion page lands on a board the importer names ([0049](0049-boards.md) §1).

What changes is the posts. **Each comment in the section becomes one post.** The first comment is the thread's `create`, and each later comment a `post`, appended in the order the page shows them. Each post's `inReplyTo` is the post it answers (§2). Document order puts every parent before its replies, so a parent's revision ID is known when its reply is appended. Posts are wikitext (`mediaType: text/x-wiki`), as 0054 §5 made them. A post's text is the comment as it was written, signature included, with **its own indentation removed**: the prefix of `:`, `*` and `#` on its first line is stripped from each of its lines. Its depth now comes from the tree, and any deeper markers inside it stay.

**The text above the first heading** stays one pinned, open thread, as before, when it holds banners and notices. Where it also holds signed comments, as talk pages from before headings were customary do, it is divided at the first comment. The part before stays the pinned header. The comments from there on become an archived thread whose subject is "*{title}*: before the first heading", split as any section is.

Where the split fails, the conversion falls back to a post per section, then to a post for the page (§4). Who each post is attributed to is §6, which settles 0054 Q4: posts carry their authors, not the importer.

### 2. Finding comments (amends 0054 §5)

The splitter reads the wikitext, never the rendered page, so it works on a dump as well as on a live wiki.

- **A signature** is a user link followed by a timestamp:
  - The link is to the user page, the user talk page or `Special:Contributions/`, in any of the wiki's names and aliases for them, as its `meta=siteinfo` reports.
  - The timestamp is in a format the registry lists (§5).
  - No more than 100 rendered characters may separate the two, DiscussionTools' limit; "rendered" counts link labels and text, not markup.
  - Character references are decoded first, since temporary account names are written `&#126;2025-…`.
- **A registered unsigned template** (`{{unsigned|Name|date}}` and its relatives, §5) signs the text before it for the user it names.
- **An undated signature**, the form of 2001 to about 2005, is a dash followed by a user link that ends the line (§5).
- **Several signatures on one line:** the first ends the comment, and later ones are notes inside it. This is the struck comment with another editor's annotation, `<s>…</s> <small>Block-evasion by … ~~~~</small>`.
- **A comment's text** is everything in its section since the previous comment ended, less blank lines at either end. Unsigned text before a signed comment is part of that comment unless history says otherwise (§6). Unsigned text after the last comment of a section is a post of its own, attributed by §6.
- **A comment's depth** is the indentation of its first line. **Its parent** is the nearest earlier comment in the same section or subsection with a smaller depth. A comment at depth 0 has no parent, or the heading post of its subsection (§3). After a registered outdent template (`{{outdent}}`, `{{od}}`, §5), the next comment is a reply to the comment before it, whatever its depth.

These rules are the common case. The cases the research found beyond them are left to the fallbacks (§4) or open (Q4).

### 3. Headings, and wrappers that close a discussion (amends 0054 §5; extends 0019 §4)

- **Level 2** starts a thread, as before.
- **Level 1 is not structure.** A level-one heading's text is dropped and the sections under it are threads like any other. This is how the reference desks date their questions (`= October 7 =`). A level-one heading with comments directly under it and no level-two heading is read as level two.
- **Levels 3 to 6 become heading posts.** A heading post is a `post` with `heading` set to the level and the heading's wikitext as its text. Its `inReplyTo` is the heading post of the enclosing subsection, or null. The comments directly under the heading reply to it. A request for adminship's Support, Oppose and Neutral sections become three heading posts with the votes as their replies. A heading post is attributed to whoever added the heading line (§6); without history, to the author of the first comment under it.

**A wrapper** is a registered template pair (§5) that collapses or closes a discussion: `{{archive top}}`…`{{archive bottom}}`, `{{hat}}`…`{{hab}}`, `{{collapse top}}`…`{{collapse bottom}}` and their relatives. Noticeboard and RfC archives are mostly closed discussions wrapped this way.
- **A wrapper that encloses a whole section or a whole subsection is lifted.** Its two calls are removed from the posts' text and become a **wrapper post**: a `post` whose text is the opening call, usually the closer's signed statement, and whose `wrap` field holds the closing call. It is the first post of its section or subsection, and is attributed to the closer, the signature in the opening call, by §6. The posts it encloses keep their own parents; nothing replies to the wrapper.
- **A wrapper that encloses only part of a section** is not lifted, and the section falls back (§4).

Heading posts and wrapper posts are written only by imports, as wikitext posts are, and render as what they were. A heading post renders as a heading at its depth. A wrapper post renders by expanding its opening call, a placeholder and its closing call once, then putting the enclosed posts' rendered HTML at the placeholder. So `{{archive top}}`'s box closes around the discussion it closed.

### 4. When the split fails: the section, then the page (amends 0054 §5)

**A section falls back to one post**, 0054 §5's rule, holding the section's wikitext, when any of these holds:
- it holds no signature the splitter recognises;
- a comment boundary would fall inside a construct that opens before the boundary and closes after it: a table, an HTML block element left open, a template call spanning lines, or a wrapper not lifted by §3;
- the splitter's result fails its own checks: every line belongs to exactly one post, and every reply's parent precedes it.

The fallback post is attributed as §6 attributes unsigned text, and its `imported_from` names the reason.

**A page falls back to one post** when it cannot be divided at all: the splitter fails on it, or the page exceeds the job's size limit. The page becomes one archived thread, "*{title}*: imported talk page", whose opening post is the page's wikitext, attributed to the person who started the job.

A talk page in a content model other than wikitext is not converted. It is counted `deferred`, as pages in reserved namespaces are. Every fallback is counted in the job's report by reason, so the pages that degraded can be found and, when the splitter improves, converted again (Q7).

### 5. Signature conventions: a universal registry

**`docs/registry/talk-markup.toml` lists the conventions of wikitext talk pages,** and it is universal: an entry applies on every wiki where it matches, because the same formats recur across wikis. It holds:

| Table | Entries |
|---|---|
| `[[timestamp]]` | A timestamp format as a MediaWiki date-format string (`H:i, j F Y`, `H:i, F j, Y`, `H:i, j M Y`, `H:i, M j, Y`, `H:i, Y F j`, `Y-m-d H:i`, …), each optionally followed by a zone in parentheses (` (T)`). It covers the content-language defaults and the user date preferences that signatures used until about 2005 |
| `[[months]]` | Per language code, the month names and abbreviations a format's `F` and `M` match |
| `[[undated]]` | The marks that may precede an undated signature's user link (`--`, `—`, `–`, `-`, `~`) |
| `[[template]]` | Template names by role, with the language they are used in: `unsigned` (signs the text before it for the user it names), `outdent`, `wrapper` (an opening and closing pair, §3) and `autosign` (the marker a signing bot leaves) |

**A wiki configures nothing.** The splitter takes everything wiki-specific from `meta=siteinfo` (or a dump's `<siteinfo>`): the content language, which selects the month names; the timezone, in which a timestamp without a zone is read, with the tz database's daylight-saving rules; and the namespace names and aliases and special-page aliases that a user link may use. A registered format is tried on every wiki. The research found no false matches on the modern pages, and a format only counts as a signature next to a user link.

The file starts with English: the formats English-language Wikimedia wikis have used since 2002, and the English template names. Other languages are added by commits, from MediaWiki's localisation data. It is embedded by `scatter-talk` (§12).

### 6. Attribution (amends 0054 §5; uses 0007 §5)

**Every imported post records two things** in its `imported_from` (§8):
- `signed_as`: the user its signature names, if it has one;
- `added_by`: the upstream revision that added its signature line, or its first line if unsigned, and that revision's actor, where history was read (§7).

**Its author is resolved from both:**

| Case | Author |
|---|---|
| History and signature agree: the same account, or the signature's name is one this account signs with | That account. An account's names are those it signed with in its own additions at least twice in the history read, and its former names in the repository's rename log where that is readable. This catches renames and usurped names, which a lookup of today's names gets wrong |
| The signature names someone else: a signing bot (a registered `autosign` marker or `unsigned` template added by another editor), a delivered message, a moved comment | The signature's user. `added_by` keeps the bot or the mover |
| No signature, with history | The actor whose revision added the text |
| No signature, no history | Merged into the next signed comment (§2); after the last comment, the person who started the job |
| A fallback post (§4) | The section's first signer with history or without, as for a comment; failing that, the person who started the job |

**Actors are named as their source names them.**
- A fork's conversion names upstream actors under the repository's issuer, as its seeded upstream revisions do ([0054](0054-forking-a-mirrored-page.md) §3).
- `Special:Import` names them as it names imported revisions' authors ([0007](0007-actor-identity.md) §5).
- An adoption names them as the adopted actors they already are ([0035](0035-adopting-a-wikibase.md) §5).

A post's attestation names its author and the job, as an upstream revision record names its upstream actor and the job. A hidden upstream user is written as the hidden marker those records use.

**Its time** is the signature's timestamp, or, for unsigned text, the adding revision's. It is kept in `imported_from.timestamp`; the record's own time is the import's.

### 7. Attribution reads history; it does not store it (amends 0054 §3, §5)

**The conversion reads the talk page's whole history** from the same source the seeding job uses, `fork.history_source` (a RevisionChest store or the repository's API). For an adoption it reads the dump, and for `Special:Import` the export's own revisions. It computes a line-level blame: each line is credited to the revision that first added it, and a line moved or restored keeps its origin. It keeps nothing but each post's `added_by`.

**The texts it reads are not stored.** What the log keeps of the talk page's history is decided by seeding alone, under `fork.content_revisions` ([0054](0054-forking-a-mirrored-page.md) §3, A5), so attribution costs a read, not storage.

**Archives are attributed from the parent's history.** An archive's comments were written on the parent talk page and copied by a bot. The blame over the parent's history credits them by matching their lines, and the archive's own history is not read.

**The read counts against the job's budget,** `fork.max_revisions` (§11). Beyond it, older comments are attributed by their signatures alone. A source with no history, an export of current revisions only, is attributed by signatures throughout.

### 8. The record (amends 0019 §4; extends 0054 §5)

The thread payload ([0019](0019-discussions.md) §4) gains three content-part fields and widens one. All four are written only by imports, and a `post` from an editor that carries any of them is refused.

| Field | Present on | Meaning |
|---|---|---|
| `imported_from` | `create`, and now `post` (optional) | On `create`, as before: the repository, talk page, revision and archive subpage, and the hash of the section's DiscussionTools name. On every imported post: the DiscussionTools `name` and `id` (§9), `signed_as`, `added_by` (upstream revision ID and actor), `timestamp`, and `fallback` with its reason when the post is a section or page fallback (§4) |
| `heading` | `post` (optional) | 3 to 6: the post is a heading post, its text the heading (§3) |
| `wrap` | `post` (optional) | The closing call of a wrapper post, whose text is the opening call (§3) |
| `inReplyTo` | `post` | As before; it may now name a heading post |

The text part stays the post's wikitext, so erasing a comment's text leaves the tree intact, as for any post.

### 9. DiscussionTools names and IDs (extends 0047 §9; extends 0013 §5)

**Every imported post keeps the name and ID DiscussionTools gives its comment,** and every thread and heading post keeps its heading's.
- **Name:** `c-{author}-{timestamp}`, the same on every page and revision where the comment appears.
- **ID:** the name plus the parent, `-{parent author}-{parent timestamp}` or `-{heading anchor}`. It is unique within one page, with `-1`, `-2` added on collision.

Both are derived, not assigned. DiscussionTools' own tables are an index filled by parsing revisions; they assign nothing.

**Where they come from:**
- **From upstream, where it runs DiscussionTools:** the conversion asks `action=discussiontoolspageinfo&prop=threaditemshtml` for the converted revision of the page and of each archive, one request each, and matches the IDs to posts by name.
- **Derived, elsewhere:**
  - The author has spaces made underscores, is truncated to 80 bytes and is trimmed of underscores.
  - The timestamp is ISO with milliseconds before DiscussionTools' format switch (`$wgDiscussionToolsTimestampFormatSwitchTime`, 2022-07-12 by default, read in the wiki's local timezone), and `YmdHis` from then on.
  - Heading anchors come from the wikitext renderer.
  - The legacy ID, without underscore trimming, is kept where it differs.

From the prototype's parse, `tools/talk_compare.py` derives every name, and 2,051 of 2,113 IDs (97.1%); the misses follow the parent disagreements.

**Links keep working.**
- On the converted talk page and on the thread page, each imported post's HTML carries its ID, and its legacy ID where that differs, as an anchor, and each heading carries its heading ID. So `Talk:X#c-…` and `Talk:X#h-…` links from elsewhere land on the post.
- `Special:GoToComment/{name or ID}` finds an imported post by name or ID and redirects to it on its thread, through `Special:PermanentLink/{revid}`. Where several posts share a name, it lists them. It is DiscussionTools' page, served with its meaning (origin `DiscussionTools`, tenant scope, group `redirects`).
- `view.post` gains `dt_name`, `dt_id` and `original`, the imported timestamp, with an index on `dt_name` for the special page ([0013](0013-postgres-storage.md) §5).

### 10. Where the conversion runs (amends 0008 §9; amends 0069 §7; uses 0035)

The same conversion, by `scatter-talk` (§12), serves every way a wikitext talk page arrives:

- **Fork the talk page** and **Stop following** ([0054](0054-forking-a-mirrored-page.md) §5, [0069](0069-synchronized-talk-pages.md) §7), from the repository, with history from `fork.history_source`.
- **`Special:Import`** of talk namespaces ([0008](0008-namespaces-and-document-pages.md) §9), with history from the export when it carries full history.
- **Adoption.** An adopted wiki's talk pages are converted within the adoption job ([0035](0035-adopting-a-wikibase.md) §2), with history from the dump and actors as adopted. They are no longer counted `deferred`. Librarybase's talk pages are the first case.

### 11. `all`: waiving the history limits (amends 0054 §3)

**`fork.content_revisions`, its instance ceiling, and `fork.max_revisions` accept `all`.** The defaults stay 100 and 20,000, because they guard against a surprise. An operator who means a tenant to keep complete histories sets the ceiling to `all` once, and the tenant then sets `fork.content_revisions = all`: every seeded revision gets its text. `fork.max_revisions = all` lets one job read and seed every revision. It also lets §7's attribution read every revision, whatever the content setting.

### 12. Crates (amends 0005 §2; amends 0054 §12)

| Crate | Change |
|---|---|
| `scatter-talk` | **New, pure.** The talk-page splitter (§1–§4), moved here from `scatter-adapter-mediawiki` so that adoption and `Special:Import` use it without the adapter; the line-level blame over a stream of revisions and the attribution of §6–§7; DiscussionTools names and IDs (§9); embeds `docs/registry/talk-markup.toml` (§5). Depends on `scatter-wikitext` for heading anchors and markup balance |
| `scatter-threads` | Heading posts, wrapper posts, `imported_from` on `post`, and refusing those fields on an editor's `post` (§3, §8) |
| `scatter-adapter-mediawiki` | Loses the splitter; its `discussiontoolspageinfo` reader also serves §9's IDs |
| `scatter-ingest` | Converts an adopted wiki's talk pages within the adoption job, with blame over the dump (§10) |
| `triplespace-repos` | The fork job's and stop-following conversion's calls into `scatter-talk`, and the history stream for attribution (§7) |
| `triplespace-projections` | `dt_name`, `dt_id` and `original` on `view.post` (§9) |
| `triplespace-ui` | Rendering heading and wrapper posts, anchors, the imported post's byline, and `Special:GoToComment` (§3, §9) |

### 13. Testing

**`scatter-talk` has golden tests against DiscussionTools.**
- **The fixtures** are made by `tools/talk_dumps.py`: a page's revision as a dump, with `discussiontoolspageinfo`'s parse of that same revision.
- **The comparison:** the test compares comments by name and parents by name.
- **The reference implementation:** `tools/talk_split.py` is the Python prototype of §2, and `tools/talk_compare.py` its comparison. The Rust splitter is held to the prototype's agreement or better.
- **Known differences:** where the splitter is right and DiscussionTools is not, such as old date formats or a subsection's first comment, the case goes in a known-differences list with its reason. The test fails when one of them starts matching, as the Librarybase parse goldens do.
- **Attribution:** it is tested on a page with full history, against the counts of the research.

## Alternatives considered

- **Keep one post per section** (0054 §5 as it stood). Rejected by James's direction; it stays as the fallback (§4).
- **Use DiscussionTools' parse as the splitter.** Rejected:
  - It recognises only the current date format.
  - It merges unsigned text.
  - It is unavailable for dumps, for adoption and for wikis without the extension.
  - It cannot attribute.

  It is kept as the source of IDs where available (§9) and as the test oracle (§13).
- **Signatures only.** Rejected: renames and usurped names are attributed wrongly, and unsigned text not at all.
- **History only.** Rejected: moved comments and delivered messages go to the mover and the bot.
- **Store the history the attribution reads.** Rejected: it is the cost A5 removed, and attribution needs only the result.
- **Per-wiki signature formats.** Rejected by James's direction: the formats are shared, and what is per-wiki comes from siteinfo.
- **Drop wrapper templates.** Rejected: the closing statement and the box are part of what a closed discussion was. They are lifted instead (§3).
- **Split partial wrappers into a container post with replies.** Rejected: the enclosed comments would change parents. The section falls back instead.

## Consequences

- **Imported discussions are discussions.** Each comment can be read, linked, watched and replied to on its own. The tree indentation drew is kept, and the posts are attributed to the people who wrote them.
- **Old pages convert better than DiscussionTools reads them,** because the registry knows the older formats and undated signatures.
- **No section is worse off than 0054 §5 left it.** A section the splitter cannot handle becomes the post it would have been.
- **Adopting a wiki brings its talk pages,** so Librarybase's are no longer left behind.
- **Inbound links survive.** `#c-…` anchors and `Special:GoToComment` resolve to the imported posts.
- **Converting a talk page reads its whole history once.** Through the API that is about one request per 50 revisions, unless a RevisionChest store or a dump is at hand. The log stores no more than seeding decides.
- **`all` makes complete histories a setting,** not a code change.
- **Other languages need registry entries** before their old signatures are recognised. Modern ones need only a month-name table.

## Open questions

- **Q1. Later edits as `edit` records.** In the research, 5.0% of comments had their signature line changed later by their author. Whether the conversion should append those versions as `edit` records on the post, rather than importing the latest text only.
- **Q2. Closures as statuses.** Whether a lifted wrapper should also set a status on its thread or heading post, with 0069's open question on closure templates.
- **Q3. Temporary accounts.** How `~2025-…` temporary accounts are named as foreign actors.
- **Q4. Plain-text signatures.** `--MichaelTinkler` and similar, with no link, are found only through history today; whether the registry should recognise them where an account of that name exists.
- **Q5. Archive titles.** Whether `Talk:X/Archive 5` should resolve on a converted talk page to the threads imported from it, so that `Talk:X/Archive 5#c-…` links keep working, not only the talk page's own.
- **Q6. Fixtures.** Whether Wikipedia text committed as test fixtures needs attribution beyond the dump's own contributors.
- **Q7. Converting again.** How a talk page converted by an older splitter is re-converted when the splitter improves, without duplicating threads.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §12 | amends | 0005 A101 |
| [0008](0008-namespaces-and-document-pages.md) §9 | §10 | amends | 0008 A43 |
| [0013](0013-postgres-storage.md) §5 | §9 | extends | 0013 A69 |
| [0019](0019-discussions.md) §4 | §3, §8 | extends | 0019 A24 |
| [0047](0047-special-pages.md) §9 | §9 | extends | 0047 A25 |
| [0054](0054-forking-a-mirrored-page.md) §3 | §7, §11 | amends | 0054 A6 |
| [0054](0054-forking-a-mirrored-page.md) §5 | §1–§9 | amends | 0054 A6 |
| [0054](0054-forking-a-mirrored-page.md) §12 | §12 | amends | 0054 A6 |
| [0069](0069-synchronized-talk-pages.md) §7 | §10 | amends | 0069 A5 |

## References

- [Extension:DiscussionTools](https://www.mediawiki.org/wiki/Extension:DiscussionTools); its source, `includes/CommentParser.php` (`computeName`, `computeId`, `truncateForId`, `buildThreads`), `includes/ThreadItem/CommentItemTrait.php` (`getTimestampString`) and `sql/discussiontools_persistent.json`, as of 2026-10-20
- [API:Discussiontoolspageinfo](https://www.mediawiki.org/wiki/Extension:DiscussionTools/API) and `Special:GoToComment`
- [Help:Signatures](https://en.wikipedia.org/wiki/Help:Signatures), [Template:Unsigned](https://en.wikipedia.org/wiki/Template:Unsigned), [Template:Outdent](https://en.wikipedia.org/wiki/Template:Outdent), [Template:Archive top](https://en.wikipedia.org/wiki/Template:Archive_top)
- [Manual:$wgDefaultUserOptions](https://www.mediawiki.org/wiki/Manual:$wgDefaultUserOptions) (`date`) and MediaWiki's per-language `$dateFormats`
- `tools/talk_dumps.py`, `tools/talk_split.py`, `tools/talk_compare.py`; the samples of 2026-10-21 under `target/talk-samples/`
- Project note `claude/talk-page-conversion.md`
