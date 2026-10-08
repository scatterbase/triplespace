# 0008. Namespaces and document pages

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-08 (A31)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0009](0009-keyed-entity-types-and-domain.md)
- **Uses:** [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0009](0009-keyed-entity-types-and-domain.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

Triplespace's user experience is organized like MediaWiki's, because the goal is an experience compatible with Wikibase. MediaWiki organizes everything into **namespaces**. A page title such as `Item:Q6` names a namespace (`Item`) and a title within it (`Q6`). Namespace IDs, names and page titles appear throughout the API (`ns`, `title`, `pageid`), and clients such as Pywikibot read the namespace list from `meta=siteinfo` ([mediawiki-compat.md §4.0](../api/mediawiki-compat.md); the reference install's list is in `snapshots/mw-1.43.9-wb-REL1_43.siteinfo.json`).

In Wikibase every namespace holds documents: an item is a page whose content is its JSON serialization. In Triplespace that is no longer true:

- **Entity pages are composed views.** `Item:Q6` is a local item and `Property:WDP31` is a property mirrored from Wikidata. Neither is stored as a document. Each is a view over the resolved graph ([0002](0002-source-graphs-and-mass-ingest.md) §3).
- **Some pages really are documents.** An instance needs project pages (policies, documentation, help) and user pages, stored as text with revision history, as MediaWiki stores them. The first content to be moved in comes from project pages on Librarybase and similar Wikibase installs.

Scatterbase has the same split. It stores a **blob**, a sequence of arbitrary bytes, and leaves it to a **view** to decide what the bytes mean.

The goal is not to reimplement MediaWiki's page features. Templates, parser functions, Lua and categories are out of scope. Pages need basic markup and a choice of content model.

## Decision

### 1. A namespace is a registry entry

*Changed by A3, A5, A10, A11, A19.*

Namespaces are configuration: data passed in by the caller and recorded in the log ([0004](0004-identity-clusters-and-equivalence.md) §9, [0005](0005-crate-organization.md) §3 rule 3). The entries are in `docs/registry/namespaces.toml` ([0015](0015-record-format-and-partition-registry.md) §5). Each entry records:

- the **MediaWiki fields**: numeric ID, canonical name, local name, aliases, the `case` rule, whether subpages are allowed, and the paired talk namespace;
- the **kind**, which says only whether the namespace holds pages ([0041](0041-content-models.md) §4):
  - **`pages`**: it holds pages. The entry names the allowed content models and the default model (§5). A namespace whose pages may also carry uploads adds `uploads = true` ([0039](0039-files-and-media.md) §2);
  - **`reserved`**: the namespace is listed in `meta=siteinfo` but holds no pages (§2, rule 1);
  - **`virtual`**: titles are generated or resolved by the server. `Special` and `Media` ([0039](0039-files-and-media.md) §1) are virtual, and so is a namespace with **`forwards_to`**, every title of which resolves to the same title in the namespace it names (§3, [0049](0049-boards.md) §2);
  - **`resolver`**: every title is a key, and viewing it performs a lookup ([0029](0029-resolver-namespaces.md) §2–3). It holds no pages, so it has no model;
- the **title normalizer** (§3);
- the **creation rule**, where the kind needs one (§6);
- **`enabled_by`**, where a namespace is implemented only while a tenant setting is on (§2, rule 1).

**An entity namespace is keyed by entity type, not by provider.** `Item` hosts every item type: local `Q`, Wikidata `WDQ`, OpenAlex `OAW` and the rest. `Property` hosts `P` and `WDP`. What makes a namespace an entity namespace is its model: the entity type's model (`wikibase-item`, `wikibase-property`, [0041](0041-content-models.md) §3) names the type, so the entry carries no list of entity types. This matches [0002](0002-source-graphs-and-mass-ingest.md) §4, where the prefixed ID is the working name in a page title such as `Item:WDQ123`.

### 2. Namespace numbering

*Changed by A3, A6, A8, A9, A10, A12, A13, A15, A17, A18, A19, A25, A26, A27, A28, A29, A31.*

MediaWiki's canonical numbers are kept wherever a namespace has one, because clients hard-code them. Four rules govern every other number (A6, A17, A31):

1. **Every number MediaWiki core or any Wikibase extension uses is reserved.** It is listed with kind `reserved`, is never given another meaning, and is not implemented unless an ADR says so. An ADR may implement one conditionally, by a tenant setting named in the entry's `enabled_by` ([0042](0042-template-expansion-and-parsoid.md) §3). Turning the setting off keeps the namespace's pages readable and refuses writes with `ts-namespace-disabled`.
2. **Talk namespaces exist only for implemented subject namespaces,** at the next odd number. A talk namespace is a `pages` namespace with the model `triplespace-talk` ([0019](0019-discussions.md) §2, [0041](0041-content-models.md) §4). Where a page is its own talk page, as a board and a thread are, the talk number is `virtual` and forwards to the subject namespace ([0049](0049-boards.md) §2). A resolver's talk number is `reserved` ([0029](0029-resolver-namespaces.md) §2). A reserved, unimplemented subject namespace has no talk namespace.
3. **Triplespace's own namespaces take 210–219, then 310–319. Resolver prefixes ([0029](0029-resolver-namespaces.md)) take 220–229, then 320–329.** A first block is used up before its second. All four blocks are registered on mediawiki.org's Extension default namespaces page, which listed nothing in 204–240 on 2026-09-27 and nothing in 310–329 on 2026-10-01. Allocation is a change to `namespaces.toml`, under the registry's rules ([0015](0015-record-format-and-partition-registry.md) §5).
4. **A tenant's own namespaces take 3000 and above,** the numbers mediawiki.org's Extension default namespaces page leaves to system administrators, **or 100–199,** MediaWiki's block for site-specific namespaces, except the numbers `namespaces.toml` lists. An adopted wiki keeps the numbers it already gave its namespaces ([0035](0035-adopting-a-wikibase.md)). A tenant `namespace` record with any other number not in `namespaces.toml` is refused with `ts-namespace-number`; one whose name or alias equals another namespace's name or alias on the tenant, or an interwiki prefix from its site aliases ([0026](0026-sitelinks.md) §2), is refused with `ts-namespace-name-taken`. An entity source's namespace is one of these ([0078](0078-entity-sources.md) §5).

**Allocations.** `namespaces.toml` is authoritative. This table matches it as of A19:

| Numbers | Names | Kind | Default model | Source |
|---|---|---|---|---|
| −2 | Media | `virtual` | — | [0039](0039-files-and-media.md) §1 |
| −1 | Special | `virtual` | — | §1 |
| 0, 1 | (main), Talk | `pages` | `wikitext` | [0038](0038-page-metadata-and-categories.md) §8 |
| 2, 3 | User, User talk | `pages` | `wikitext` | §6 |
| 4, 5 | Project, Project talk | `pages` | `wikitext`; `triplespace-sprint` on subpages | §7; [0061](0061-sprints-and-tasks.md) §2 |
| 6, 7 | File, File talk | `pages`, `uploads` | `wikitext` | [0039](0039-files-and-media.md) §1 |
| 8 | MediaWiki | `reserved` | — | Rule 1 |
| 10, 11 | Template, Template talk | `pages` while `wikitext.expansion` is on | `wikitext` | [0042](0042-template-expansion-and-parsoid.md) §3 |
| 12 | Help | `reserved` | — | Rule 1. Help content goes in `Project` |
| 14, 15 | Category, Category talk | `pages` | `wikitext` | [0038](0038-page-metadata-and-categories.md) §4 |
| 120, 121 | Item, Item talk | `pages` | `wikibase-item` | Wikibase's default number |
| 122, 123 | Property, Property talk | `pages` | `wikibase-property` | Wikibase's default number |
| 124, 125 | Query, Query talk | `pages` | `triplespace-sparql`; `wikitext` for `/doc` subpages | [0063](0063-query-namespace.md) §2: Wikibase's number, with Wikibase's meaning |
| 146, 147 | Lexeme, Lexeme talk | `pages` | `wikibase-lexeme` | [0066](0066-lexemes.md) §2 |
| 210, 211 | Domain, Domain talk | `pages` | `triplespace-domain` | [0009](0009-keyed-entity-types-and-domain.md) §11 |
| 212, 213 | Keyword, Keyword talk | `pages` | `triplespace-keyword` | [0017](0017-entity-id-grammar.md) §5 |
| 214, 215 | Thread, Thread talk | `pages`; 215 forwards to 214 | `triplespace-thread` | [0019](0019-discussions.md) §3, [0049](0049-boards.md) §2 |
| 216, 217 | Notation, Notation talk | `pages` | `triplespace-notation` | [0036](0036-openstreetmap-providers.md) §3 as OSM; [0048](0048-notation.md) §7 |
| 218, 219 | Table, Table talk | `pages` | `triplespace-table`; `wikitext` for `/doc` subpages | [0045](0045-table-content-model.md) §2 |
| 220, 221 | DOI, DOI talk | `resolver`; 221 `reserved` | — | [0029](0029-resolver-namespaces.md) |
| 222, 223 | URL, URL talk | `resolver`; 223 `reserved` | — | [0029](0029-resolver-namespaces.md) |
| 310, 311 | Board, Board talk | `pages`; 311 forwards to 310 | `triplespace-board` | [0049](0049-boards.md) §2 |
| 312, 313 | Scope, Scope talk | `pages` | `triplespace-scope`; `wikitext` for `/doc` subpages | [0060](0060-scopes.md) §2 |
| 640, 641 | EntitySchema, EntitySchema talk | `pages` | `EntitySchema` | [0064](0064-entityschema-and-validation.md) §2: the extension's numbers |
| 828, 829 | Module, Module talk | `pages` while `wikitext.lua` is on | `Scribunto`; `wikitext` for `/doc` subpages | [0043](0043-lua-modules.md) §2, Scribunto's numbers |

Talk namespaces default to `triplespace-talk`. 210–219 is full; the next resolver takes 224.

### 3. Titles

*Changed by A2, A3, A5, A10, A11, A14, A19, A21, A22, A31.*

**One resolver handles every title.** Page views, API `titles=` parameters, redirects and wiki links (§8) all go through it. It works in three steps:

1. **Split** the namespace prefix, matching canonical names, local names and aliases without regard to case, as MediaWiki does.
2. **Normalize** the title with the namespace's normalizer:
   - `first-letter` (MediaWiki's default): underscores become spaces and the first letter is uppercased. Used by `User` and `Project`.
   - `entity-id`: parses an entity ID, uppercases its prefix and applies the ID grammar's canonical form to the rest ([0017](0017-entity-id-grammar.md) §2), so `Item:q42` becomes `Item:Q42` and `Item:mbab10BBBFC-…` becomes `Item:MBAb10bbbfc-…`. It also accepts tenant-relative IDs: `Item:QQQ5` normalizes to `Item:Q5` ([0044](0044-tenant-relative-ids.md) §5).
   - `file-name`: `first-letter` with MediaWiki's file-name rules ([0039](0039-files-and-media.md) §1).
   - A keyed type's own normalizer ([0009](0009-keyed-entity-types-and-domain.md) §2).
   - A resolver's normalizer ([0029](0029-resolver-namespaces.md) §2).
   - An entity source's ID normalizer, in its namespace ([0078](0078-entity-sources.md) §2, §5).
3. **Resolve** the normalized title:
   - in an entity namespace, to an entity, following aliases and identity clusters to the canonical ID ([0004](0004-identity-clusters-and-equivalence.md) §4), exactly as the API resolves IDs;
   - in a forwarding namespace, to the same title in the namespace named by `forwards_to` ([0049](0049-boards.md) §2);
   - in a `resolver` namespace, by the resolver's lookup ([0029](0029-resolver-namespaces.md) §3), or, in an entity source's namespace, by the source's ([0078](0078-entity-sources.md) §5);
   - in any other `pages` namespace, to the **primary of the title's stack** ([0052](0052-page-repositories-and-title-inheritance.md) §2): the local page ID, or, where no local page exists and a page repository in `pages.repos` serves the namespace and has the title, that repository's page ([0052](0052-page-repositories-and-title-inheritance.md) §3). A primary that is a **redirect** ([0051](0051-page-redirects.md) §1), local or foreign, is followed one hop, to the stack of its target title, unless the request says `redirect=no` ([0051](0051-page-redirects.md) §2).

**A title that resolves somewhere else redirects there.** `Item:P31` redirects to `Property:P31`, and `Item:WDQ123` redirects to `Item:Q456` once `WDQ123` belongs to a cluster whose canonical ID is `Q456`. `Special:EntityPage/{id}` resolves any entity ID to its page, as it does in Wikibase. A forwarding title is a permanent redirect: `Board talk:X` answers 301 to `Board:X`, and has no page ID ([0049](0049-boards.md) §2).

**Thread titles are minted, not chosen:** the UTC date the thread was created and its subject, `Thread:2026-09-27/Why is P31 wrong here` ([0019](0019-discussions.md) §3).

**`meta=siteinfo` reports only MediaWiki's two case values,** `first-letter` and `case-sensitive`. A namespace whose normalizer is neither reports `case-sensitive`, and the server normalizes. A client that believed `first-letter` would rewrite titles the server does not accept.

### 4. Document pages

*Changed by A4, A9, A21, A23, A30.*

**A page is identified by a page ID, not by its title.** Page IDs are minted by the instance in sequence, start at 1 and are never reused. The title is an attribute of the page, in the same way a username is an attribute of an actor ([0007](0007-actor-identity.md) §4). This has three effects:

- **The log key is the page ID.** A log header key must be an identifier, never content ([0006](0006-log-integrity-and-erasure.md) §3). A title is content, and a user page title contains a username.
- **A move is one record on the moved page.** Moving a page appends a record with its new title, and nothing else about that page changes. By default a second page is created at the old title, a redirect to the new one ([0051](0051-page-redirects.md) §3).
- **A title index is a projection.** It maps each current title to its page ID, and it is rebuilt from the log.

**Pages live in their own partition.** A `pages` source partition is added to the registry ([0005](0005-crate-organization.md) §4.1):

| Graph | Illustrative IRI | Kind | Written by | History | Integrity | Export |
|---|---|---|---|---|---|---|
| **Pages** | `{base}/graph/pages` | Source | Page edits, moves and imports | Full | `logged` | Public |

The partition holds page records, not quads. Its RDF output is revision metadata and page statements (§10).

**Page record payloads.** Each record's payload type is `scatter:v0/page`, and it carries one operation:

| Operation | Meaning |
|---|---|
| `create` | Mints the page ID, sets the title and content model, and stores the first text; may carry `forked_from`, naming the repository, page and revision a fork was taken from ([0054](0054-forking-a-mirrored-page.md) §1) |
| `edit` | Stores the page's **complete new text**, optionally with a new content model |
| `move` | Sets a new title |
| `follow` | On a fork, whether its talk page follows the repository's (`follow`) or has been forked (`fork`); no text, so it is a null revision in the page's history, as MediaWiki writes for protection ([0069](0069-synchronized-talk-pages.md) §7) |

**Deletion is an ACL, not a page operation** ([0023](0023-moderation.md) §4). Deleting a page writes a `read` ACL on its page ID to the tenant `log` partition, and undeleting retires the ACL. The page and its history are hidden from everyone but administrators. This is hiding in the sense of [0006](0006-log-integrity-and-erasure.md) (Context), not erasure.

**A page's statements are change sets** (`scatter:v0/changeset`), keyed by its page ID in the same partition and restricted to statements ([0038](0038-page-metadata-and-categories.md) §1). They take revision IDs, so a page's history interleaves text and statement revisions.

**Each revision stores the full text, not a diff.** Pages are small. Diffs are computed when they are read, as MediaWiki computes them. Because each record is self-contained, erasing one revision with an `erase` record ([0006](0006-log-integrity-and-erasure.md) §7) does not break the text of later ones.

**Edit conflicts use the base offset** ([0006](0006-log-integrity-and-erasure.md) §8). An `edit` carries the offset of the page's latest record that the client saw. A mismatch is reported as `editconflict`. A base is required for `edit` and `move`.

### 5. Content models

*Changed by A3, A11, A13, A24.*

**Every page has one content model** ([0041](0041-content-models.md) §1). The model decides where the page's content comes from, how the content is validated and serialized, and how it is rendered. Models are registry data in `docs/registry/content-models.toml` ([0041](0041-content-models.md) §2). IDs follow one rule: MediaWiki's and Wikibase extensions' IDs are kept and reserved, generic formats take no prefix, and models unique to Triplespace take `triplespace-`.

**Each model has a source:** text, entity, thread, composite or statements ([0041](0041-content-models.md) §5). Only text models are stored in the page's own records and support direct editing. This ADR introduced five of them:

| Model ID | Validated as | Rendered as | Origin of the ID |
|---|---|---|---|
| `wikitext` | Any text | The subset in §8 | MediaWiki |
| `markdown` | Any text | CommonMark with GitHub-style tables. Wiki links are resolved by the title resolver (§3), and raw HTML is sanitized to the allow-list of §8 ([0019](0019-discussions.md) §5) | New |
| `json` | Well-formed JSON | Formatted, collapsible JSON | MediaWiki |
| `yaml` | Well-formed YAML | Formatted YAML | New |
| `text` | Any text | Preformatted text | MediaWiki |

- **The default comes from the namespace:** `wikitext` for `Project` and `User`.
- **A title suffix overrides the default** when a page is created, as in MediaWiki: `.md` gives `markdown`, `.json` gives `json`, `.yaml` or `.yml` gives `yaml`, and `.txt` gives `text`.
- **The model can be changed** with `action=changecontentmodel`, which appends an `edit` carrying the new model. It moves a page only between text models its namespace allows.
- **Content that fails validation is rejected** when it is saved.
- **Models whose code reaches the reader's browser unsanitized are not supported.** `css` and `javascript` are not registered: user-supplied scripts and unsanitized styles are an injection risk. `sanitized-css`, TemplateStyles' model, is supported, because every sheet passes a sanitizer that scopes it to rendered content before it reaches a browser ([0055](0055-templatestyles-templatedata-and-page-properties.md) §1–2). `Scribunto`, whose modules run on the server in a sandbox and return wikitext, is supported while a tenant has Lua on ([0043](0043-lua-modules.md) §3).

**Rendering code is per model.** Each model implements the content-model trait in `scatter-pages` ([0005](0005-crate-organization.md) §2), so an instance can add models without changing the page format. In Scatterbase's terms, the stored text is the blob and the content model is the view.

### 6. The User namespace

**A user page belongs to a local user, identified by user ID.** The root title `User:Example` is resolved through the local actor records ([0007](0007-actor-identity.md) §4) to the actor key, such as `local:42`. The page's owner is that actor.

- **User pages can be created only for local users that exist.** The user must be registered, not temporary, and not vanished. `User:Nobody` cannot be created when no local user is called Nobody. Subpages follow the same rule, through their root.
- **Foreign actors have no user pages here.** Links to them go to their upstream page through the IRI in [0007](0007-actor-identity.md) §2.
- **Titles follow renames.** When a user is renamed, every page under their root is moved to the new name by the same process, in one batch of `move` records. The same happens when a user vanishes: the pages move to the placeholder name. The content is not erased automatically. It can be erased on request with an `erase` record ([0006](0006-log-integrity-and-erasure.md) §7).
- **Old names do not resolve.** A rename leaves no redirect from the old title. A redirect would keep the old name visible, and names are erasable ([0007](0007-actor-identity.md) §4). This differs from MediaWiki, which leaves a redirect after a move.
- **Subpages with `json` or `yaml` content are editable only by their owner and administrators,** as MediaWiki protects `.json` user subpages. Other user pages follow the instance's ordinary edit permissions.

### 7. The Project namespace

- **The canonical name is `Project`, and the local name is the site name,** as in MediaWiki. On the reference install that is `Triplespace Ref` ([siteinfo snapshot](../api/snapshots/mw-1.43.9-wb-REL1_43.siteinfo.json)). Both resolve.
- **Subpages are allowed.**
- **Pages can be created by anyone with the right to edit.** No owner is recorded.

### 8. The wikitext subset

*Changed by A3, A9, A10, A12, A21, A22.*

Triplespace renders a fixed subset of wikitext. It does not implement MediaWiki's parser.

**Rendering has two stages** ([0042](0042-template-expansion-and-parsoid.md) §1, §7). When the tenant's `wikitext.expansion` setting is on, templates, parser functions and variables are expanded natively first, and the subset renders the expanded text. A tenant may instead render through Parsoid ([0042](0042-template-expansion-and-parsoid.md) §8). When expansion is off, the subset renders the stored text.

| Supported | Syntax |
|---|---|
| Headings | `== … ==` through `====== … ======` |
| Emphasis | `''…''`, `'''…'''` |
| Lists | `*`, `#`, `;` and `:`, nested |
| Internal links | `[[Title]]`, `[[Title\|text]]`, `[[Title#Section]]`, `[[/Subpage]]`, and links into any registered namespace, such as `[[Item:Q5]]` or `[[Domain:en.wikipedia.org]]`. An unprefixed title is a main-namespace title, so an entity is linked with its namespace ([0038](0038-page-metadata-and-categories.md) §8) |
| Categories | `[[Category:…]]`, `{{DEFAULTSORT:…}}` and `__HIDDENCAT__`; `[[:Category:…]]` is a plain link ([0038](0038-page-metadata-and-categories.md) §3) |
| Files | `[[File:…]]` embedding with MediaWiki's options and a caption, `[[:File:…]]` links to description pages, `[[Media:…]]` links to bytes, and `<gallery>` ([0039](0039-files-and-media.md) §13) |
| External links | `[https://… text]` and bare URLs |
| Tables | `{\| … \|}` with header and data cells and simple attributes |
| Preformatted and code | `<pre>`, `<code>`, leading-space blocks, `<syntaxhighlight>` rendered as plain `<pre>` |
| Escaping | `<nowiki>` |
| Footnotes | `<ref>`, `<ref name=…>`, `<references />` |
| Page controls | `__NOTOC__`, `__TOC__`, `__FORCETOC__` |
| Signatures | `~~~~` and `~~~` are expanded when the page is saved, as in MediaWiki |
| Horizontal rule | `----` |
| Redirects | `#REDIRECT [[Target]]` as the first line makes the page a redirect ([0051](0051-page-redirects.md) §1) |
| Safe inline HTML | An allow-list of formatting tags, such as `<br>`, `<span>`, `<div>`, `<sup>` and `<sub>`, with attributes restricted to `class`, `id` and a sanitized `style` |

**Everything else is kept and shown, not dropped.**

- Template calls (`{{…}}`) and parser functions that are not expanded render as a visible placeholder showing the call.
- External image URLs are never embedded.
- The stored source is never rewritten, so a page renders correctly if the subset grows later.

**Category links define category membership.** The category links in the latest revision of a `wikitext` page are its membership, a projection like MediaWiki's `categorylinks`. The foot of the page lists the categories as links to their pages in the `Category` namespace, with hidden categories collapsed ([0038](0038-page-metadata-and-categories.md) §3).

**Links are resolved by the title resolver** (§3). A link to an entity that exists renders with its label, as Wikibase does. A link to a redirect carries the class `mw-redirect` ([0051](0051-page-redirects.md) §2); a link to a title whose primary is a repository's page is an ordinary link carrying `ts-inherited`, and a link into a namespace no repository serves here leaves the wiki as an interwiki link ([0052](0052-page-repositories-and-title-inheritance.md) §4).

### 9. Importing pages from another wiki

*Changed by A7, A10, A12, A16, A23.*

Pages are imported from a MediaWiki XML export with full history. An import is a job ([0002](0002-source-graphs-and-mass-ingest.md) §8.3), and the procedure is:

1. **Import every revision** as page records, in order. Each revision keeps its original timestamp as upstream metadata. The time the record is appended is still the time of the import.
2. **Attribute each revision to its original author** under [0007](0007-actor-identity.md) §5. An author on a registered issuer keeps that issuer's numeric ID. Any other author becomes an `imported` surrogate.
3. **Flatten templates in one extra revision, where the tenant does not expand them.** Where the latest revision uses templates, a bot fetches the expanded text from the source wiki with `action=expandtemplates` and saves it as a new revision. History stays faithful to the source, and the current text needs no template engine. With `wikitext.expansion` on, this step is optional: the importer may bring the Template and Module pages with their history instead ([0042](0042-template-expansion-and-parsoid.md) §13).

**A template census comes first.** Before an import, the pages are scanned for templates, parser functions and tags outside §8. The census decides whether step 3 is enough, or whether the subset should grow first. It also reports which templates and modules the export contains ([0042](0042-template-expansion-and-parsoid.md) §13).

**Page IDs on an adopted wiki.** When the source is the wiki the tenant adopted, imported pages keep their source page IDs, supplied in header field 9 as adopted entities' are, and the page-ID floor set at adoption already covers them ([0035](0035-adopting-a-wikibase.md) §4).

**Files.** Exports with `<upload>` elements import file versions, and `triplespace-cli files import` imports a directory as `importImages.php` does ([0039](0039-files-and-media.md) §14).

**Talk namespaces are converted to threads.** A wikitext talk page in an export is imported as threads homed on its subject's talk page, split by level-two heading with the frontmatter first, each created closed with the `archived` status and holding its section's wikitext as the opening post ([0054](0054-forking-a-mirrored-page.md) §5); a project-wide discussion page lands on a board the importer names.

**The forms.** `Special:Import` starts this job after showing the census. `Special:Export` is its counterpart: it writes MediaWiki XML of pages and local entities, and a verifiable `records` format ([0047](0047-special-pages.md) §8).

### 10. Links and metadata

*Changed by A3, A9, A12, A21, A22.*

**A links projection** records every link from a document page to a page or entity, after resolution. It serves "What links here" for both kinds of page. So `Item:Q5` lists the project pages that link to it, and `list=backlinks` works across namespaces. Links in thread posts are rows too, so it also lists the threads that discuss it ([0019](0019-discussions.md) §5). On a tenant with expansion on, the projection reads the expanded text, so links that templates emit count, and its rows are written by the refresh job of [0042](0042-template-expansion-and-parsoid.md) §10. A redirect's row points at its target, so "What links here" lists a page's redirects ([0051](0051-page-redirects.md) §5); a link to a title whose primary is a repository's page is recorded by namespace and title, as a link to a missing page is, so a later fork inherits its backlinks ([0052](0052-page-repositories-and-title-inheritance.md) §4).

**Page revisions get revision nodes** in the metadata graph, as entity revisions do ([0001](0001-revision-metadata-rdf.md) §1, §6):

- The page's document node is `{base}/page/{page ID}`.
- Its revision nodes carry the actor, timestamp, summary, tags, flags, content model, size and hash, in the vocabulary of 0001.
- A page's statements go into the main graph, with the page's document node as subject ([0038](0038-page-metadata-and-categories.md) §11). Page text and category membership are not RDF.

### 11. Crates (amends 0005 §2)

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks. The table this section first gave is in A1.

### 12. API surface

Document namespaces bring these core modules into scope ([mediawiki-compat.md §5](../api/mediawiki-compat.md)):

- **Reading:** `action=parse`, `action=compare`, `query&prop=revisions`, `prop=info`, `prop=links`, `list=allpages`, `list=backlinks`, `list=prefixsearch`, `list=recentchanges` and `list=usercontribs`.
- **Writing:** `action=edit`, `action=move`, `action=delete`, `action=undelete`, `action=changecontentmodel` and `action=import`.

Pywikibot reading and editing `Project` pages is the acceptance test for this surface.

## Consequences

- **Two kinds of page share one title space.** Links, redirects, backlinks and search behave the same whether a title names a view or a document.
- **Renaming a user touches only titles.** Page records and revisions are keyed by page ID and actor key, so nothing else has to be rewritten.
- **Old usernames stop working as links.** Links to `User:OldName` in page text break after a rename. That is the cost of not keeping erasable names in redirects.
- ~~**Imported pages need a flattening pass.** Pages that depend heavily on templates can lose layout when they are expanded into static text.~~ *Only on a tenant that does not expand templates (A12).*
- ~~**Most of MediaWiki's page machinery stays out.** No templates, categories, transclusion or Lua. Content that needs them has to be rewritten.~~ *No longer holds. Categories are implemented (A9). Templates, transclusion (A12) and Lua (A13) are available to a tenant that turns them on.*
- **Two new content model IDs.** `markdown` and `yaml` are not MediaWiki models. MediaWiki clients see them as unknown models and can only read or write the raw text.
- ~~**Global revision IDs become urgent.** MediaWiki's `revid`, `oldid` and `lastrevid` come from one sequence shared by every page. Page edits and entity changes now both appear in `list=recentchanges`, so they need one sequence (open questions).~~ *Met by [0013](0013-postgres-storage.md) §6 and [0015](0015-record-format-and-partition-registry.md) §2 (Q1).*

## Open questions

- **Q1.** ~~**Global revision IDs.** Log offsets are numbered per partition, but MediaWiki revision IDs are global. Either a global allocator issues revision IDs across partitions, or the API maps (partition, offset) pairs onto one sequence. This was already latent in [0001](0001-revision-metadata-rdf.md) and [0002](0002-source-graphs-and-mass-ingest.md).~~ *Settled by [0013](0013-postgres-storage.md) §6 and [0015](0015-record-format-and-partition-registry.md) §2: one sequence per tenant for revisions and one for log events, taken in the appending transaction and written into the header; mirror records take provider-ranged IDs.*
- **Q2.** ~~**Page IDs for entity views.** `wbgetentities` with `props=info` returns a `pageid`, and a composed view has none. The options are minting one the first time an entity is written to, or returning none and accepting that some clients break.~~ *Settled by [0013](0013-postgres-storage.md) §6 and [0015](0015-record-format-and-partition-registry.md) §2: a page ID is taken from the one sequence the first time a key is written in any partition and carried forward in header field 9, so entity views and document pages share one `pageid` space.*
- **Q3.** ~~**The main namespace.** Whether namespace 0 stays empty, becomes a document namespace, or hosts items as it does on Wikidata.~~ *Settled by A6: reserved and empty, like every namespace MediaWiki or Wikibase uses; its talk namespace is not enabled.* *Changed by [0038](0038-page-metadata-and-categories.md) §8 (A9): a `pages` namespace for articles, with Talk (1) enabled.*
- **Q4.** ~~**Discussions.** Talk namespaces are reserved (§2). How discussions work is its own ADR.~~ *Settled by [0019](0019-discussions.md): talk namespaces are composite views over threads, and a thread is a page in the `Thread` namespace.*
- **Q5.** ~~**Categories.** Whether category links should ever become data, for example as statements or as a list projection.~~ *Settled by [0038](0038-page-metadata-and-categories.md) §3 and §5: categories are defined only in wikitext and projected as a list; configured mappings turn membership into page statements.*
- **Q6.** ~~**Page protection and permissions.** Who may create, move, delete and protect pages, beyond the owner rule in §6.~~ *Settled by [0016](0016-permissions-and-access-control.md) §5 (who) and [0023](0023-moderation.md) §1–4 (what protection and deletion are: ACLs on the page).*
- **Q7.** ~~**Search.** Whether one search index covers entity labels and page text together, as `list=search` would expect.~~ *Settled by [0014](0014-caches-and-search.md) §7: two indexes (`entities`, `pages`), one query, with a Postgres fallback (§8).*
- **Q8.** ~~**Page redirects.** Whether document pages may be redirects (`#REDIRECT [[…]]`), and whether a move leaves one for `Project` pages, where no erasable names are involved.~~ *Settled by [0051](0051-page-redirects.md) §1 and §3: a `wikitext` page whose text begins with a redirect line is a redirect, and a move leaves one everywhere except user renames and thread renames.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2, §4.1 | §4, §11 | amends | 0005 A3 |
| [0009](0009-keyed-entity-types-and-domain.md) Q1 | §2 | settles | 0009 Q1 |

## References

- [Manual:Namespace](https://www.mediawiki.org/wiki/Manual:Namespace) and [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces)
- [Manual:Content handlers](https://www.mediawiki.org/wiki/Manual:ContentHandler)
- [Help:Formatting](https://www.mediawiki.org/wiki/Help:Formatting)
- [API:Expandtemplates](https://www.mediawiki.org/wiki/API:Expandtemplates)
- [CommonMark](https://commonmark.org/) and the [GitHub Flavored Markdown spec](https://github.github.com/gfm/)

## Amendment log

### A1. The crate table moves to 0005

- **Date:** 2026-09-26
- **Source:** [0005](0005-crate-organization.md) §2, revision of 2026-09-26
- **Change:** supersedes §11
- **Summary:** 0005 §2 became the one crate table CI checks. `scatter-markdown` was folded into `scatter-pages` as a feature, and `scatter-keyed` into `scatter-normalize`.

Replaced text (§11):

> | Layer | Crate | Contents | Depends on |
> |---|---|---|---|
> | Substrate | `scatter-pages` | Page records and their operations, page IDs, the content-model trait, validation for `json`, `yaml` and `text` | — |
> | | `scatter-wikitext` | Parsing and rendering the §8 subset. The link resolver is passed in as a trait | — |
> | | `scatter-markdown` | Rendering for `markdown`, as a thin wrapper around an existing CommonMark crate | — |
> | Triplespace | `triplespace-titles` | The namespace registry, title normalization and the title resolver (§1–3) | `scatter-pages`, `scatter-wikibase-model`, `scatter-identity`, `scatter-keyed` |
>
> - `scatter-pages`, `scatter-wikitext` and `scatter-markdown` are pure, under [0005](0005-crate-organization.md) §3 rule 2.
> - `scatter-wikitext` and `scatter-markdown` must build for `wasm32-unknown-unknown`, like `scatter-wikibase-shape`, so the editor's preview runs the same code as the server.
> - The namespace registry is in the surfaces layer because namespaces are part of the MediaWiki compatibility surface. Scatterbase does not need them.

### A2. Entity IDs canonicalize by their grammar

- **Date:** 2026-09-27
- **Source:** [0017](0017-entity-id-grammar.md) §2
- **Change:** amends §3
- **Summary:** The `entity-id` normalizer uppercases only the prefix and applies the type's ID grammar to the rest, so case-sensitive UUID IDs survive. Until the migration, nothing in this ADR recorded the change. 0017's header attributed it to 0017 §4, but the text is in 0017 §2.

Replaced text (§3):

> - `entity-id`: parses and uppercases an entity ID, so `Item:q42` becomes `Item:Q42`.

### A3. Discussions

- **Date:** 2026-09-27
- **Source:** [0019](0019-discussions.md) §2, §3, §5
- **Change:** amends §2; extends §1, §3, §5, §8, §10
- **Summary:** Talk namespaces are now of kind `composite`: the talk page of a subject is the set of threads attached to it. Threads themselves are pages in a new `Thread` namespace of kind `thread`, whose number is pending registration.
  Not noted here before the migration: 0019 §3 and §5 also gave threads minted titles, gave the `markdown` model wiki links and HTML sanitized to §8's allow-list, and made links in posts `page_link` rows. A11 later replaced the `composite` and `thread` kinds with models.

Replaced text (§2):

> **Talk namespaces are reserved, not implemented.** Their IDs are allocated and listed, because MediaWiki clients assume every subject namespace has a talk namespace at the next odd number. They hold no pages. How discussions work is left to its own ADR.

### A4. Deletion becomes an ACL

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §4
- **Change:** amends §4
- **Summary:** `delete` and `undelete` are no longer page operations. Deletion is a `read` ACL on the page, written to the tenant `log` partition, and undeletion retires it; the effects described here are unchanged. The `page` payload type carries `create`, `edit` and `move` only.

Replaced text (§4):

> | `delete` | Hides the page and its history from everyone but administrators. This is hiding in the sense of [0006](0006-log-integrity-and-erasure.md) (Context), not erasure |
> | `undelete` | Reverses a `delete` |

> A base is required for `edit`, `move` and `delete`.

> | **Pages** | `{base}/graph/pages` | Source | Page edits, moves, deletions and imports | Full | `logged` | Public |

### A5. The `resolver` kind

- **Date:** 2026-09-27
- **Source:** [0029](0029-resolver-namespaces.md) §2
- **Change:** extends §1, §3
- **Summary:** A `resolver` namespace holds no pages and no records. Every title in it is a key, normalized by the resolver's normalizer, and viewing the title performs a lookup. Its talk number is `reserved`. Before the migration this was recorded only in this ADR's header.

### A6. Numbering policy

- **Date:** 2026-09-27
- **Source:** Direct: James, review of 2026-09-26/27
- **Change:** amends §2
- **Summary:** Three rules replace the sentence below and settle the main-namespace question (Q3):
  1. **Every number MediaWiki core or any Wikibase extension uses is reserved.** It is listed in `docs/registry/namespaces.toml` with kind `reserved`, is never given another meaning, and is not implemented unless an ADR says so. The reserved set: Media (−2), the main namespace (0), File (6), MediaWiki (8), Template (10), Help (12), Category (14), Query (124) and Lexeme (146). The main namespace stays reserved and empty; help content goes in `Project` as before.
  2. **Talk namespaces exist only for implemented subject namespaces.** They work as §2 and [0019](0019-discussions.md) §2 describe, at the next odd number, and are `composite`. A reserved-and-not-implemented namespace has no talk namespace enabled: Talk (1), File talk (7) and the rest are not registered.
  3. **Triplespace's own namespaces take 210–219, and resolver prefixes ([0029](0029-resolver-namespaces.md)) take 220–229.** Allocated now: Domain 210/211 ([0009](0009-keyed-entity-types-and-domain.md) §11), Keyword 212/213 ([0017](0017-entity-id-grammar.md) §5), Thread 214/215 ([0019](0019-discussions.md) §3), OSM 216/217 ([0036](0036-openstreetmap-providers.md) §3), DOI 220/221 and URL 222/223 ([0029](0029-resolver-namespaces.md)). The ranges are registered on mediawiki.org's Extension default namespaces page, which as of 2026-09-27 lists nothing between 204 and 240. Allocation is a change to `namespaces.toml`, under the registry's rules ([0015](0015-record-format-and-partition-registry.md) §5).

Replaced text (§2):

> Media (−2), File (6), MediaWiki (8), Template (10), Help (12) and Category (14) are not registered. Help content goes in `Project`.

### A7. Adopted wikis keep page IDs

- **Date:** 2026-09-28
- **Source:** [0035](0035-adopting-a-wikibase.md) §4
- **Change:** extends §9
- **Summary:** When the source is the wiki the tenant adopted, imported pages keep their source page IDs, supplied in header field 9 as adopted entities' are, and the page-ID floor set at adoption already covers them.

### A8. OSM namespaces

- **Date:** 2026-09-28
- **Source:** [0036](0036-openstreetmap-providers.md) §3
- **Change:** extends §2
- **Summary:** OSM and OSM talk were allocated 216/217. A18 renamed them.

### A9. Articles, categories and page statements

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §1, §3, §4, §8, §11
- **Change:** amends §2, §4, §8, §10
- **Summary:** As noted under each section before the migration:
  - §2: Two reserved numbers are implemented, as rule 1 allows. The main namespace (0) is a `document` namespace for articles, with Talk (1) enabled; an unprefixed title in a link is a main-namespace title, and entities are linked with their namespace. Category (14) is a `document` namespace for category description pages, `wikitext` only, with Category talk (15) enabled.
  - §4: A page's statements are change sets (`scatter:v0/changeset`) keyed by its page ID in the same partition, restricted to statements. They take revision IDs, so a page's history interleaves text and statement revisions.
  - §8: Category links in the latest revision of a `wikitext` page define its category membership, a projection like MediaWiki's `categorylinks`; `{{DEFAULTSORT:…}}` and `__HIDDENCAT__` are recognised; `[[:Category:…]]` is a plain link. The foot of the page lists categories as links to their pages in the `Category` namespace, with hidden categories collapsed.
  - §10: A page's statements go into the main graph with the page's document node as subject. Page text and category membership are still not RDF.

Replaced text (§2, from A6 rule 1):

> The main namespace stays reserved and empty; help content goes in `Project` as before.

Replaced text (§4):

> The partition holds page records, not quads. Its only RDF output is revision metadata (§10).

Replaced text (§8 and §10):

> - Category links (`[[Category:…]]`) are listed as plain text at the foot of the page. No category pages exist.

> - Nothing about document pages goes into the main or resolved graph. Page text is not RDF.

### A10. Files and media

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §1, §13, §14
- **Change:** extends §1, §2, §3, §8, §9
- **Summary:** As noted under each section before the migration:
  - §2: Three more reserved numbers are implemented: File (6) is a `file` namespace, a document namespace whose pages may also carry uploads, with File talk (7) enabled; Media (−2) is `virtual` and resolves to a file's current bytes.
  - §8: The subset gains file embedding (`[[File:…]]` with MediaWiki's options and a caption), `[[:File:…]]` links to description pages, `[[Media:…]]` links to bytes, and `<gallery>`. External image URLs are never embedded.
  - §9: Exports with `<upload>` elements import file versions, and `triplespace-cli files import` imports a directory as `importImages.php` does.
  - Not noted here before the migration: the `file` kind in §1, which A11 replaced with `pages` and `uploads = true`, and the `file-name` normalizer in §3.

### A11. Namespaces name models, not kinds

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §1–5
- **Change:** amends §1, §3, §5
- **Summary:** As noted under each section before the migration:
  - §1: Every page has a content model, and a namespace entry names its allowed models and default model. The kind now says only whether the namespace holds pages: `pages`, `reserved`, `virtual` or `resolver`. Entity views, document pages, file pages, threads and talk pages are all `pages` namespaces; what they are made of is their model (`wikibase-item`, `wikitext`, `triplespace-thread`, `triplespace-talk` and so on), and a File namespace adds the `uploads` flag. `entity_types` leaves the entry, since the model names the entity type.
  - §5: Content models now cover every page, not only document pages, and live in `docs/registry/content-models.toml`. IDs follow one rule: MediaWiki's and Wikibase extensions' IDs are kept and reserved, generic formats (`markdown`, `yaml`) take no prefix, and models unique to Triplespace take `triplespace-`. Each model has a **source** (text, entity, thread, composite or statements); only text models support direct editing, and `changecontentmodel` moves only between text models the namespace allows.
  - Not noted here before the migration: the resolve step of §3 named document namespaces, a kind this entry retires.

Replaced text (§1):

> - the **kind**, one of:
>   - **entity view**: pages are composed from the resolved graph. The entry names the entity types the namespace hosts;
>   - **document**: pages are stored text with a content model (§5). The entry names the allowed content models and the default;
>   - **reserved**: the namespace is listed in `meta=siteinfo` but holds no pages;
>   - **virtual**: pages are generated by the server (`Special`);

Replaced text (§3):

> - in a document namespace, to a page ID (§4).

Replaced text (§5):

> A document page has one **content model**, which decides how its text is validated and rendered.

> **Content models are registry data.** The rendering code for each model implements a trait in `scatter-pages` (§11), so an instance can add models without changing the page format.

### A12. Template expansion

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §1, §3, §7–9, §13
- **Change:** amends §8, §9; extends §2, §10
- **Summary:** As noted under each section before the migration:
  - §2: A reserved number can be implemented conditionally, by a setting named in its `namespaces.toml` entry (`enabled_by`). Template (10), with Template talk (11), is a `pages` namespace while the tenant's `wikitext.expansion` is on; Module (828) and Module talk (829), Scribunto's numbers, now registered, while `wikitext.lua` is on. Turning a setting off keeps the namespace's pages readable and refuses writes with `ts-namespace-disabled`.
  - §8: Rendering has two stages. With the tenant's `wikitext.expansion` on, templates, parser functions and variables are expanded natively first, and the subset renders the expanded text; a tenant may instead render through Parsoid (0042 §8). With expansion off, template calls stay chips, as above.
  - §9: With expansion on, step 3 is optional: the census also reports which templates and modules the export contains, and the importer may bring Template and Module pages with their history instead of flattening.
  - §10: With expansion on, the links projection reads the expanded text, so links that templates emit count, and its rows are written by the refresh job of 0042 §10.

Replaced text (§8):

> - Template calls (`{{…}}`) and parser functions render as a visible placeholder showing the call.

Replaced text (§9):

> 3. **Flatten templates in one extra revision.** Where the latest revision uses templates, a bot fetches the expanded text from the source wiki with `action=expandtemplates` and saves it as a new revision. History stays faithful to the source, and the current text needs no template engine.

### A13. Lua modules

- **Date:** 2026-09-30
- **Source:** [0043](0043-lua-modules.md) §2–3
- **Change:** amends §5; extends §2
- **Summary:** As noted under each section before the migration:
  - §2: A reserved number can be implemented conditionally, by a setting named in its `namespaces.toml` entry (`enabled_by`). Template (10), with Template talk (11), is a `pages` namespace while the tenant's `wikitext.expansion` is on; Module (828) and Module talk (829), Scribunto's numbers, now registered, while `wikitext.lua` is on. Turning a setting off keeps the namespace's pages readable and refuses writes with `ts-namespace-disabled`.
  - §5: The exclusion above is of models that reach the reader's browser. `Scribunto`, whose modules run on the server in a sandbox and return wikitext, is implemented; `css`, `javascript` and `sanitized-css` stay excluded.

Replaced text (§5):

> - **Executable models are not supported.** `css`, `javascript` and `sanitized-css` are not registered. User-supplied scripts and styles are an injection risk that nothing on the roadmap needs.

### A14. Tenant-relative IDs in titles

- **Date:** 2026-09-30
- **Source:** [0044](0044-tenant-relative-ids.md) §5
- **Change:** extends §3
- **Summary:** The `entity-id` normalizer accepts tenant-relative IDs: `Item:QQQ5` normalizes to `Item:Q5`.

### A15. Table namespaces

- **Date:** 2026-09-30
- **Source:** [0045](0045-table-content-model.md) §2
- **Change:** extends §2
- **Summary:** Table (218) and Table talk (219), the last free pair in 210–219. Table pages hold `triplespace-table` definitions, with `wikitext` for `/doc` subpages as in Module.

### A16. Import and export forms

- **Date:** 2026-09-30
- **Source:** [0047](0047-special-pages.md) §8
- **Change:** extends §9
- **Summary:** `Special:Import` is the form that starts this job, after showing the census. `Special:Export` writes MediaWiki XML of pages and local entities, the counterpart of this import, and a verifiable `records` format.

### A17. Second ranges

- **Date:** 2026-10-01
- **Source:** Direct: James, design discussion of 2026-10-01
- **Change:** extends §2
- **Summary:** Rule 3 gains a second block for each purpose, because 210–219 is full and 220–229 has three pairs left. Triplespace's own namespaces continue in **310–319**, and resolver prefixes in **320–329**. A first block is used up before its second: the next resolver takes 224. Nothing is allocated in either new block yet. James checked mediawiki.org's Extension default namespaces page on 2026-10-01 and found nothing registered in 310–329; both blocks are filed together with 210–229.

### A18. Notation namespaces

- **Date:** 2026-10-01
- **Source:** [0048](0048-notation.md) §7
- **Change:** amends §2
- **Summary:** 216 and 217 are renamed from OSM and OSM talk to Notation and Notation talk. They hold the same pages, OSM keys and tags, now as notations in the `osm` scheme (`Notation:osm:amenity=cafe`), and will hold other schemes' notations as schemes are added.

Replaced text: the names OSM and OSM talk for 216/217, in A6 rule 3 and A8.

### A19. Boards, and talk namespaces that forward

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §2
- **Change:** amends §2; extends §1, §3
- **Summary:** As noted under each section before the migration:
  - §2: Board (310) and Board talk (311), the first pair of the second block. Board talk is `virtual` and **forwards** to Board: every title in it resolves to the same title in 310, because a board is its own talk page. Thread talk (215) changes from `reserved` to forwarding to Thread (214) for the same reason.
  - §3: A `virtual` namespace with `forwards_to` resolves every title to the same title in the namespace it names, as a permanent redirect: `Board talk:X` to `Board:X`, `Thread talk:X` to `Thread:X`. Forwarding titles have no page IDs.

Replaced text: Thread talk (215) as `reserved` and empty, from [0019](0019-discussions.md) §3 (A3).

### A20. Consolidated under 0050

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–5, §8–11
- **Summary:** A1–A19 were folded into the Decision text, the open questions were numbered, and falsified consequences were struck. No decision changed. Before this, the amendments were blockquotes under the sections they changed, which A3–A19 now quote, and A2, A5 and A8 were recorded only in other ADRs. The file before consolidation is commit `dc4774a`.

Replaced text (§2). The table as it stood, with rows 1, 210 and 211 already edited in place:

> | ID | Canonical name | Kind | Notes |
> |---|---|---|---|
> | −1 | Special | Virtual | `Special:EntityData`, `Special:EntityPage`, `Special:Redirect` and the other special pages the API contract needs |
> | 0 | (main) | Reserved | MediaWiki requires it to exist. Its use is open |
> | 1 | Talk | Not enabled | The main namespace is reserved and not implemented (amendment below) |
> | 2 | User | Document | §6 |
> | 3 | User talk | Reserved | |
> | 4 | Project | Document | §7 |
> | 5 | Project talk | Reserved | |
> | 120 | Item | Entity view | Wikibase's default number |
> | 121 | Item talk | Reserved | |
> | 122 | Property | Entity view | Wikibase's default number |
> | 123 | Property talk | Reserved | |
> | 210 | Domain | Entity view | [0009](0009-keyed-entity-types-and-domain.md); number allocated 2026-09-27 (amendment below) |
> | 211 | Domain talk | Composite | |

### A21. Page redirects

- **Date:** 2026-10-01
- **Source:** [0051](0051-page-redirects.md) §1–3, §5
- **Change:** amends §4; extends §3, §8, §10
- **Summary:** A `wikitext` page whose text begins with `#REDIRECT [[Target]]` is a redirect; the resolver follows one hop unless told `redirect=no`; a move leaves a redirect page at the old title by default, so a move is one record on the moved page and a `create` of a second; links to redirects carry `mw-redirect`; the links projection records a redirect's target. Q8 settled.

Replaced text (§4):

> - **A move is one record.** Moving a page appends a record with its new title, and nothing else changes.

### A22. Page repositories and title inheritance

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §3–4
- **Change:** amends §3; extends §8, §10
- **Summary:** A `pages` title resolves to the primary of its stack: the local page, else the first page repository in `pages.repos` that serves the namespace and has the title. Links to inherited titles are ordinary links carrying `ts-inherited`; links into namespaces no repository serves leave the wiki; the links projection records links to inherited titles by title.

Replaced text (§3):

> - in any other `pages` namespace, to a page ID (§4).

### A23. Forks, and talk pages imported as threads

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §1, §5
- **Change:** extends §4, §9
- **Summary:** A `create` may carry `forked_from`, naming the repository, page and revision a fork was taken from. A wikitext talk page in an import is converted into closed threads on the subject's talk page, split by level-two heading.

### A24. `sanitized-css` is supported

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §1
- **Change:** amends §5
- **Summary:** The exclusion of style models is narrowed to `css` and `javascript`, which reach the browser as written. `sanitized-css`, TemplateStyles' model, is implemented behind a sanitizer that scopes every sheet to rendered content.

Replaced text (§5):

> - **Models whose code reaches the reader's browser are not supported.** `css`, `javascript` and `sanitized-css` are not registered. User-supplied scripts and styles are an injection risk that nothing on the roadmap needs. `Scribunto`, whose modules run on the server in a sandbox and return wikitext, is supported while a tenant has Lua on ([0043](0043-lua-modules.md) §3).

### A25. Scope

- **Date:** 2026-10-04
- **Source:** [0060](0060-scopes.md) §2
- **Change:** extends §2
- **Summary:** Scope (312) and Scope talk (313), the second pair of the 310–319 block, with the `/doc` rule of Table.

### A26. Sprints in Project

- **Date:** 2026-10-04
- **Source:** [0061](0061-sprints-and-tasks.md) §2
- **Change:** extends §2
- **Summary:** `Project` (4) allows `triplespace-sprint`, for subpages only; the default stays `wikitext`.

Replaced text (§2, the Project row):

> | 4, 5 | Project, Project talk | `pages` | `wikitext` | §7 |

### A27. Query

- **Date:** 2026-10-05
- **Source:** [0063](0063-query-namespace.md) §2
- **Change:** amends §2
- **Summary:** Query (124) moves from `reserved` to `pages` with model `triplespace-sparql`, keeping Wikibase's meaning for the number; Query talk (125) is added.

Replaced text (§2):

> | 124 | Query | `reserved` | — | Rule 1 |

### A28. EntitySchema

- **Date:** 2026-10-05
- **Source:** [0064](0064-entityschema-and-validation.md) §2
- **Change:** extends §2
- **Summary:** EntitySchema (640) and EntitySchema talk (641), the extension's numbers, hosting the `entityschema` entity type.

### A29. Lexeme

- **Date:** 2026-10-05
- **Source:** [0066](0066-lexemes.md) §2
- **Change:** amends §2
- **Summary:** Lexeme (146) moves from `reserved` to `pages` with model `wikibase-lexeme`; Lexeme talk (147) is added.

Replaced text (§2):

> | 146 | Lexeme | `reserved` | — | Rule 1 |

### A30. The `follow` operation

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §7
- **Change:** extends §4
- **Summary:** A fork's page records gain `follow`, a null revision recording whether the fork's talk page follows the repository's or has been forked.

### A31. A tenant range, and entity-source namespaces

- **Date:** 2026-10-08
- **Source:** [0078](0078-entity-sources.md) §5
- **Change:** extends §2, §3
- **Summary:** Rule 4: a tenant's own namespaces take 3000 and above, or 100–199 except registered numbers, and an adopted wiki keeps its numbers; other numbers are refused with `ts-namespace-number`, and names that equal another namespace's or an interwiki prefix with `ts-namespace-name-taken`. Until now nothing said which numbers a tenant's namespaces may take. An entity source's namespace is a `resolver` namespace with the source's ID normalizer and the source's lookup.

Replaced text (§2):

> MediaWiki's canonical numbers are kept wherever a namespace has one, because clients hard-code them. Three rules govern every other number (A6, A17):
