# 0008. Namespaces and document pages

- **Status:** Proposed
- **Date:** 2026-09-26
- **Author:** James Hare / Claude Opus
- **Amended by:** [0036 — OpenStreetMap providers](0036-openstreetmap-providers.md) (§3 allocates 216/217), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md), [0014 — Cache layers and search](0014-caches-and-search.md), [0005 — Crate organization, revision of 2026-09-26](0005-crate-organization.md) (§2 crate names), [0016 — Permissions and access control](0016-permissions-and-access-control.md), [0017 — Entity ID grammar](0017-entity-id-grammar.md), [0019 — Discussions](0019-discussions.md) (§2 makes talk namespaces composite; §3 registers `Thread`; settles the discussions open question), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§4 amends §4: `delete` and `undelete` are ACL records, not page operations; §2 settles the protection open question), [0029 — Resolver namespaces](0029-resolver-namespaces.md) (§2 extends §1 with the `resolver` kind and §3 with its normalizer). §2 was amended on 2026-09-27 with the numbering policy: reserved MediaWiki and Wikibase numbers, talk namespaces for implemented subjects only, 210–219 for Triplespace and 220–229 for resolvers, [0035 — Adopting an existing Wikibase as a tenant](0035-adopting-a-wikibase.md) (§4 extends §9: pages imported from the wiki a tenant adopted keep their source page IDs), [0038 — Page metadata, legacy categories and articles](0038-page-metadata-and-categories.md) (§4 and §8 amend §2: Category and the main namespace are implemented; §3 amends §8 and settles the categories open question; §1 extends §4: change sets on pages; §11 amends §10), [0039 — Files, blob storage and foreign file repositories](0039-files-and-media.md) (§1 amends §2: File, File talk and Media are implemented; §13 amends §8: file embedding and `Media:` links; §14 extends §9: file import), [0041 — Content models](0041-content-models.md) (§1 and §4 amend §1: a namespace names its allowed and default content models, and its kind says only whether it holds pages; §3 and §5 extend §5: the content model registry, sources, slots and the trait), [0042 — Template expansion and the Parsoid renderer](0042-template-expansion-and-parsoid.md) (§1 amends §8: expansion before rendering; §3 amends §2: Template and Template talk while expansion is on; §9 amends §10: links from expanded output; §13 amends §9: flattening becomes optional), [0043 — Lua modules](0043-lua-modules.md) (§2 amends §2: Module and Module talk while Lua is on; §3 amends §5: the server-run `Scribunto` model), [0044 — Tenant-relative IDs](0044-tenant-relative-ids.md) (§5 extends §3: the `entity-id` normalizer accepts tenant-relative IDs)
- **Related:** [0000 — Initial proposition](0000-init.md), [0001 — Revision metadata in RDF](0001-revision-metadata-rdf.md), [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (amends §2 and §4.1), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md), [0007 — Actor identity](0007-actor-identity.md), [0009 — Keyed entity types and Domain](0009-keyed-entity-types-and-domain.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

Triplespace's user experience is organized like MediaWiki's, because the goal is an experience compatible with Wikibase. MediaWiki organizes everything into **namespaces**. A page title such as `Item:Q6` names a namespace (`Item`) and a title within it (`Q6`). Namespace IDs, names and page titles appear throughout the API (`ns`, `title`, `pageid`), and clients such as Pywikibot read the namespace list from `meta=siteinfo` ([mediawiki-compat.md §4.0](../api/mediawiki-compat.md); the reference install's list is in `snapshots/mw-1.43.9-wb-REL1_43.siteinfo.json`).

In Wikibase every namespace holds documents: an item is a page whose content is its JSON serialization. In Triplespace that is no longer true:

- **Entity pages are composed views.** `Item:Q6` is a local item and `Property:WDP31` is a property mirrored from Wikidata. Neither is stored as a document. Each is a view over the resolved graph ([0002](0002-source-graphs-and-mass-ingest.md) §3).
- **Some pages really are documents.** An instance needs project pages (policies, documentation, help) and user pages, stored as text with revision history, as MediaWiki stores them. The first content to be moved in comes from project pages on Librarybase and similar Wikibase installs.

Scatterbase has the same split. It stores a **blob**, a sequence of arbitrary bytes, and leaves it to a **view** to decide what the bytes mean.

The goal is not to reimplement MediaWiki's page features. Templates, parser functions, Lua and categories are out of scope. Pages need basic markup and a choice of content model.

## Decision

### 1. A namespace is a registry entry

Namespaces are configuration: data passed in by the caller and recorded in the log ([0004](0004-identity-clusters-and-equivalence.md) §9, [0005](0005-crate-organization.md) §3 rule 3). Each entry records:

- the **MediaWiki fields**: numeric ID, canonical name, local name, aliases, the `case` rule, whether subpages are allowed, and the paired talk namespace;
- the **kind**, one of:
  - **entity view**: pages are composed from the resolved graph. The entry names the entity types the namespace hosts;
  - **document**: pages are stored text with a content model (§5). The entry names the allowed content models and the default;
  - **reserved**: the namespace is listed in `meta=siteinfo` but holds no pages;
  - **virtual**: pages are generated by the server (`Special`);
- the **title normalizer** (§3);
- the **creation rule**, where the kind needs one (§6).

**An entity namespace is keyed by entity type, not by provider.** `Item` hosts every item type: local `Q`, Wikidata `WDQ`, OpenAlex `OAW` and the rest. `Property` hosts `P` and `WDP`. This matches [0002](0002-source-graphs-and-mass-ingest.md) §4, where the prefixed ID is the working name in a page title such as `Item:WDQ123`.

> **Amended by [0041](0041-content-models.md) §1 and §4.** Every page has a content model, and a namespace entry names its allowed models and default model. The kind now says only whether the namespace holds pages: `pages`, `reserved`, `virtual` or `resolver`. Entity views, document pages, file pages, threads and talk pages are all `pages` namespaces; what they are made of is their model (`wikibase-item`, `wikitext`, `triplespace-thread`, `triplespace-talk` and so on), and a File namespace adds the `uploads` flag. `entity_types` leaves the entry, since the model names the entity type.

### 2. Namespace numbering

MediaWiki's canonical numbers are kept wherever a namespace has one, because clients hard-code them.

| ID | Canonical name | Kind | Notes |
|---|---|---|---|
| −1 | Special | Virtual | `Special:EntityData`, `Special:EntityPage`, `Special:Redirect` and the other special pages the API contract needs |
| 0 | (main) | Reserved | MediaWiki requires it to exist. Its use is open |
| 1 | Talk | Not enabled | The main namespace is reserved and not implemented (amendment below) |
| 2 | User | Document | §6 |
| 3 | User talk | Reserved | |
| 4 | Project | Document | §7 |
| 5 | Project talk | Reserved | |
| 120 | Item | Entity view | Wikibase's default number |
| 121 | Item talk | Reserved | |
| 122 | Property | Entity view | Wikibase's default number |
| 123 | Property talk | Reserved | |
| 210 | Domain | Entity view | [0009](0009-keyed-entity-types-and-domain.md); number allocated 2026-09-27 (amendment below) |
| 211 | Domain talk | Composite | |

**Talk namespaces are reserved, not implemented.** Their IDs are allocated and listed, because MediaWiki clients assume every subject namespace has a talk namespace at the next odd number. They hold no pages. How discussions work is left to its own ADR.

> **Amended by [0019](0019-discussions.md) §2–3.** Talk namespaces are now of kind `composite`: the talk page of a subject is the set of threads attached to it. Threads themselves are pages in a new `Thread` namespace of kind `thread`, whose number is pending registration.

Media (−2), File (6), MediaWiki (8), Template (10), Help (12) and Category (14) are not registered. Help content goes in `Project`.

> **Amended 2026-09-27: numbering policy.** Three rules replace the sentence above and settle the main-namespace open question.
>
> 1. **Every number MediaWiki core or any Wikibase extension uses is reserved.** It is listed in `docs/registry/namespaces.toml` with kind `reserved`, is never given another meaning, and is not implemented unless an ADR says so. The reserved set: Media (−2), the main namespace (0), File (6), MediaWiki (8), Template (10), Help (12), Category (14), Query (124) and Lexeme (146). The main namespace stays reserved and empty; help content goes in `Project` as before.
> 2. **Talk namespaces exist only for implemented subject namespaces.** They work as §2 and [0019](0019-discussions.md) §2 describe, at the next odd number, and are `composite`. A reserved-and-not-implemented namespace has no talk namespace enabled: Talk (1), File talk (7) and the rest are not registered.
> 3. **Triplespace's own namespaces take 210–219, and resolver prefixes ([0029](0029-resolver-namespaces.md)) take 220–229.** Allocated now: Domain 210/211 ([0009](0009-keyed-entity-types-and-domain.md) §11), Keyword 212/213 ([0017](0017-entity-id-grammar.md) §5), Thread 214/215 ([0019](0019-discussions.md) §3), OSM 216/217 ([0036](0036-openstreetmap-providers.md) §3), DOI 220/221 and URL 222/223 ([0029](0029-resolver-namespaces.md)). The ranges are registered on mediawiki.org's Extension default namespaces page, which as of 2026-09-27 lists nothing between 204 and 240. Allocation is a change to `namespaces.toml`, under the registry's rules ([0015](0015-record-format-and-partition-registry.md) §5).

> **Amended by [0038](0038-page-metadata-and-categories.md) §4 and §8.** Two reserved numbers are implemented, as rule 1 allows. The main namespace (0) is a `document` namespace for articles, with Talk (1) enabled; an unprefixed title in a link is a main-namespace title, and entities are linked with their namespace. Category (14) is a `document` namespace for category description pages, `wikitext` only, with Category talk (15) enabled.

> **Amended by [0039](0039-files-and-media.md) §1.** Three more reserved numbers are implemented: File (6) is a `file` namespace, a document namespace whose pages may also carry uploads, with File talk (7) enabled; Media (−2) is `virtual` and resolves to a file's current bytes.

> **Amended by [0042](0042-template-expansion-and-parsoid.md) §3 and [0043](0043-lua-modules.md) §2.** A reserved number can be implemented conditionally, by a setting named in its `namespaces.toml` entry (`enabled_by`). Template (10), with Template talk (11), is a `pages` namespace while the tenant's `wikitext.expansion` is on; Module (828) and Module talk (829), Scribunto's numbers, now registered, while `wikitext.lua` is on. Turning a setting off keeps the namespace's pages readable and refuses writes with `ts-namespace-disabled`.

### 3. Titles

**One resolver handles every title.** Page views, API `titles=` parameters, redirects and wiki links (§8) all go through it. It works in three steps:

1. **Split** the namespace prefix, matching canonical names, local names and aliases without regard to case, as MediaWiki does.
2. **Normalize** the title with the namespace's normalizer:
   - `first-letter` (MediaWiki's default): underscores become spaces and the first letter is uppercased. Used by `User` and `Project`.
   - `entity-id`: parses and uppercases an entity ID, so `Item:q42` becomes `Item:Q42`.
   - A keyed type's own normalizer ([0009](0009-keyed-entity-types-and-domain.md) §2).
3. **Resolve** the normalized title:
   - in an entity namespace, to an entity, following aliases and identity clusters to the canonical ID ([0004](0004-identity-clusters-and-equivalence.md) §4), exactly as the API resolves IDs;
   - in a document namespace, to a page ID (§4).

**A title that resolves somewhere else redirects there.** `Item:P31` redirects to `Property:P31`, and `Item:WDQ123` redirects to `Item:Q456` once `WDQ123` belongs to a cluster whose canonical ID is `Q456`. `Special:EntityPage/{id}` resolves any entity ID to its page, as it does in Wikibase.

**`meta=siteinfo` reports only MediaWiki's two case values,** `first-letter` and `case-sensitive`. A namespace whose normalizer is neither reports `case-sensitive`, and the server normalizes. A client that believed `first-letter` would rewrite titles the server does not accept.

> **Extended by [0044](0044-tenant-relative-ids.md) §5.** The `entity-id` normalizer accepts tenant-relative IDs: `Item:QQQ5` normalizes to `Item:Q5`.

### 4. Document pages

**A page is identified by a page ID, not by its title.** Page IDs are minted by the instance in sequence, start at 1 and are never reused. The title is an attribute of the page, in the same way a username is an attribute of an actor ([0007](0007-actor-identity.md) §4). This has three effects:

- **The log key is the page ID.** A log header key must be an identifier, never content ([0006](0006-log-integrity-and-erasure.md) §3). A title is content, and a user page title contains a username.
- **A move is one record.** Moving a page appends a record with its new title, and nothing else changes.
- **A title index is a projection.** It maps each current title to its page ID, and it is rebuilt from the log.

**Pages live in their own partition.** A `pages` source partition is added to the registry ([0005](0005-crate-organization.md) §4.1):

| Graph | Illustrative IRI | Kind | Written by | History | Integrity | Export |
|---|---|---|---|---|---|---|
| **Pages** | `{base}/graph/pages` | Source | Page edits, moves, deletions and imports | Full | `logged` | Public |

The partition holds page records, not quads. Its only RDF output is revision metadata (§10).

**Page record payloads.** Each record's payload type is `scatter:v0/page`, and it carries one operation:

| Operation | Meaning |
|---|---|
| `create` | Mints the page ID, sets the title and content model, and stores the first text |
| `edit` | Stores the page's **complete new text**, optionally with a new content model |
| `move` | Sets a new title |
| `delete` | Hides the page and its history from everyone but administrators. This is hiding in the sense of [0006](0006-log-integrity-and-erasure.md) (Context), not erasure |
| `undelete` | Reverses a `delete` |

> **Amended by [0023](0023-moderation.md) §4.** `delete` and `undelete` are no longer page operations. Deletion is a `read` ACL on the page, written to the tenant `log` partition, and undeletion retires it; the effects described here are unchanged. The `page` payload type carries `create`, `edit` and `move` only.

> **Extended by [0038](0038-page-metadata-and-categories.md) §1.** A page's statements are change sets (`scatter:v0/changeset`) keyed by its page ID in the same partition, restricted to statements. They take revision IDs, so a page's history interleaves text and statement revisions.

**Each revision stores the full text, not a diff.** Pages are small. Diffs are computed when they are read, as MediaWiki computes them. Because each record is self-contained, erasing one revision with an `erase` record ([0006](0006-log-integrity-and-erasure.md) §7) does not break the text of later ones.

**Edit conflicts use the base offset** ([0006](0006-log-integrity-and-erasure.md) §8). An `edit` carries the offset of the page's latest record that the client saw. A mismatch is reported as `editconflict`. A base is required for `edit`, `move` and `delete`.

### 5. Content models

A document page has one **content model**, which decides how its text is validated and rendered.

| Model ID | Validated as | Rendered as | Origin of the ID |
|---|---|---|---|
| `wikitext` | Any text | The subset in §8 | MediaWiki |
| `markdown` | Any text | CommonMark with GitHub-style tables | New |
| `json` | Well-formed JSON | Formatted, collapsible JSON | MediaWiki |
| `yaml` | Well-formed YAML | Formatted YAML | New |
| `text` | Any text | Preformatted text | MediaWiki |

- **The default comes from the namespace:** `wikitext` for `Project` and `User`.
- **A title suffix overrides the default** when a page is created, as in MediaWiki: `.md` gives `markdown`, `.json` gives `json`, `.yaml` or `.yml` gives `yaml`, and `.txt` gives `text`.
- **The model can be changed** with `action=changecontentmodel`, which appends an `edit` carrying the new model.
- **Content that fails validation is rejected** when it is saved.
- **Executable models are not supported.** `css`, `javascript` and `sanitized-css` are not registered. User-supplied scripts and styles are an injection risk that nothing on the roadmap needs.

**Content models are registry data.** The rendering code for each model implements a trait in `scatter-pages` (§11), so an instance can add models without changing the page format. In Scatterbase's terms, the stored text is the blob and the content model is the view.

> **Extended by [0041](0041-content-models.md) §2–3 and §5.** Content models now cover every page, not only document pages, and live in `docs/registry/content-models.toml`. IDs follow one rule: MediaWiki's and Wikibase extensions' IDs are kept and reserved, generic formats (`markdown`, `yaml`) take no prefix, and models unique to Triplespace take `triplespace-`. Each model has a **source** (text, entity, thread, composite or statements); only text models support direct editing, and `changecontentmodel` moves only between text models the namespace allows.

> **Amended by [0043](0043-lua-modules.md) §3.** The exclusion above is of models that reach the reader's browser. `Scribunto`, whose modules run on the server in a sandbox and return wikitext, is implemented; `css`, `javascript` and `sanitized-css` stay excluded.

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

Triplespace renders a fixed subset of wikitext. It does not implement MediaWiki's parser.

| Supported | Syntax |
|---|---|
| Headings | `== … ==` through `====== … ======` |
| Emphasis | `''…''`, `'''…'''` |
| Lists | `*`, `#`, `;` and `:`, nested |
| Internal links | `[[Title]]`, `[[Title\|text]]`, `[[Title#Section]]`, `[[/Subpage]]`, and links into any registered namespace, such as `[[Item:Q5]]` or `[[Domain:en.wikipedia.org]]` |
| External links | `[https://… text]` and bare URLs |
| Tables | `{\| … \|}` with header and data cells and simple attributes |
| Preformatted and code | `<pre>`, `<code>`, leading-space blocks, `<syntaxhighlight>` rendered as plain `<pre>` |
| Escaping | `<nowiki>` |
| Footnotes | `<ref>`, `<ref name=…>`, `<references />` |
| Page controls | `__NOTOC__`, `__TOC__`, `__FORCETOC__` |
| Signatures | `~~~~` and `~~~` are expanded when the page is saved, as in MediaWiki |
| Horizontal rule | `----` |
| Safe inline HTML | An allow-list of formatting tags, such as `<br>`, `<span>`, `<div>`, `<sup>` and `<sub>`, with attributes restricted to `class`, `id` and a sanitized `style` |

**Everything else is kept and shown, not dropped.**

- Template calls (`{{…}}`) and parser functions render as a visible placeholder showing the call.
- Category links (`[[Category:…]]`) are listed as plain text at the foot of the page. No category pages exist.

> **Amended by [0038](0038-page-metadata-and-categories.md) §3.** Category links in the latest revision of a `wikitext` page define its category membership, a projection like MediaWiki's `categorylinks`; `{{DEFAULTSORT:…}}` and `__HIDDENCAT__` are recognised; `[[:Category:…]]` is a plain link. The foot of the page lists categories as links to their pages in the `Category` namespace, with hidden categories collapsed.
- The stored source is never rewritten, so a page renders correctly if the subset grows later.

**Links are resolved by the title resolver** (§3). A link to an entity that exists renders with its label, as Wikibase does.

> **Amended by [0039](0039-files-and-media.md) §13.** The subset gains file embedding (`[[File:…]]` with MediaWiki's options and a caption), `[[:File:…]]` links to description pages, `[[Media:…]]` links to bytes, and `<gallery>`. External image URLs are never embedded.

> **Amended by [0042](0042-template-expansion-and-parsoid.md) §1 and §7.** Rendering has two stages. With the tenant's `wikitext.expansion` on, templates, parser functions and variables are expanded natively first, and the subset renders the expanded text; a tenant may instead render through Parsoid (0042 §8). With expansion off, template calls stay chips, as above.

### 9. Importing pages from another wiki

Pages are imported from a MediaWiki XML export with full history. An import is a job ([0002](0002-source-graphs-and-mass-ingest.md) §8.3), and the procedure is:

1. **Import every revision** as page records, in order. Each revision keeps its original timestamp as upstream metadata. The time the record is appended is still the time of the import.
2. **Attribute each revision to its original author** under [0007](0007-actor-identity.md) §5. An author on a registered issuer keeps that issuer's numeric ID. Any other author becomes an `imported` surrogate.
3. **Flatten templates in one extra revision.** Where the latest revision uses templates, a bot fetches the expanded text from the source wiki with `action=expandtemplates` and saves it as a new revision. History stays faithful to the source, and the current text needs no template engine.

> **Extended by [0035](0035-adopting-a-wikibase.md) §4.** When the source is the wiki the tenant adopted, imported pages keep their source page IDs, supplied in header field 9 as adopted entities' are, and the page-ID floor set at adoption already covers them.

**A template census comes first.** Before an import, the pages are scanned for templates, parser functions and tags outside §8. The census decides whether step 3 is enough, or whether the subset should grow first.

> **Extended by [0039](0039-files-and-media.md) §14.** Exports with `<upload>` elements import file versions, and `triplespace-cli files import` imports a directory as `importImages.php` does.

> **Amended by [0042](0042-template-expansion-and-parsoid.md) §13.** With expansion on, step 3 is optional: the census also reports which templates and modules the export contains, and the importer may bring Template and Module pages with their history instead of flattening.

### 10. Links and metadata

**A links projection** records every link from a document page to a page or entity, after resolution. It serves "What links here" for both kinds of page. So `Item:Q5` lists the project pages that link to it, and `list=backlinks` works across namespaces.

**Page revisions get revision nodes** in the metadata graph, as entity revisions do ([0001](0001-revision-metadata-rdf.md) §1, §6):

- The page's document node is `{base}/page/{page ID}`.
- Its revision nodes carry the actor, timestamp, summary, tags, flags, content model, size and hash, in the vocabulary of 0001.
- Nothing about document pages goes into the main or resolved graph. Page text is not RDF.

> **Amended by [0038](0038-page-metadata-and-categories.md) §11.** A page's statements go into the main graph with the page's document node as subject. Page text and category membership are still not RDF.

> **Amended by [0042](0042-template-expansion-and-parsoid.md) §9.** With expansion on, the links projection reads the expanded text, so links that templates emit count, and its rows are written by the refresh job of 0042 §10.

### 11. Crates (amends 0005 §2)

| Layer | Crate | Contents | Depends on |
|---|---|---|---|
| Substrate | `scatter-pages` | Page records and their operations, page IDs, the content-model trait, validation for `json`, `yaml` and `text` | — |
| | `scatter-wikitext` | Parsing and rendering the §8 subset. The link resolver is passed in as a trait | — |
| | `scatter-markdown` | Rendering for `markdown`, as a thin wrapper around an existing CommonMark crate | — |
| Triplespace | `triplespace-titles` | The namespace registry, title normalization and the title resolver (§1–3) | `scatter-pages`, `scatter-wikibase-model`, `scatter-identity`, `scatter-keyed` |

- `scatter-pages`, `scatter-wikitext` and `scatter-markdown` are pure, under [0005](0005-crate-organization.md) §3 rule 2.
- `scatter-wikitext` and `scatter-markdown` must build for `wasm32-unknown-unknown`, like `scatter-wikibase-shape`, so the editor's preview runs the same code as the server.
- The namespace registry is in the surfaces layer because namespaces are part of the MediaWiki compatibility surface. Scatterbase does not need them.

### 12. API surface

Document namespaces bring these core modules into scope ([mediawiki-compat.md §5](../api/mediawiki-compat.md)):

- **Reading:** `action=parse`, `action=compare`, `query&prop=revisions`, `prop=info`, `prop=links`, `list=allpages`, `list=backlinks`, `list=prefixsearch`, `list=recentchanges` and `list=usercontribs`.
- **Writing:** `action=edit`, `action=move`, `action=delete`, `action=undelete`, `action=changecontentmodel` and `action=import`.

Pywikibot reading and editing `Project` pages is the acceptance test for this surface.

## Consequences

- **Two kinds of page share one title space.** Links, redirects, backlinks and search behave the same whether a title names a view or a document.
- **Renaming a user touches only titles.** Page records and revisions are keyed by page ID and actor key, so nothing else has to be rewritten.
- **Old usernames stop working as links.** Links to `User:OldName` in page text break after a rename. That is the cost of not keeping erasable names in redirects.
- **Imported pages need a flattening pass.** Pages that depend heavily on templates can lose layout when they are expanded into static text.
- **Most of MediaWiki's page machinery stays out.** No templates, categories, transclusion or Lua. Content that needs them has to be rewritten.
- **Two new content model IDs.** `markdown` and `yaml` are not MediaWiki models. MediaWiki clients see them as unknown models and can only read or write the raw text.
- **Global revision IDs become urgent.** MediaWiki's `revid`, `oldid` and `lastrevid` come from one sequence shared by every page. Page edits and entity changes now both appear in `list=recentchanges`, so they need one sequence (open questions).

## Open questions

- ~~**Global revision IDs.** Log offsets are numbered per partition, but MediaWiki revision IDs are global. Either a global allocator issues revision IDs across partitions, or the API maps (partition, offset) pairs onto one sequence. This was already latent in [0001](0001-revision-metadata-rdf.md) and [0002](0002-source-graphs-and-mass-ingest.md).~~ *Settled by [0013](0013-postgres-storage.md) §6 and [0015](0015-record-format-and-partition-registry.md) §2: one sequence per tenant for revisions and one for log events, taken in the appending transaction and written into the header; mirror records take provider-ranged IDs.*
- ~~**Page IDs for entity views.** `wbgetentities` with `props=info` returns a `pageid`, and a composed view has none. The options are minting one the first time an entity is written to, or returning none and accepting that some clients break.~~ *Settled by [0013](0013-postgres-storage.md) §6 and [0015](0015-record-format-and-partition-registry.md) §2: a page ID is taken from the one sequence the first time a key is written in any partition and carried forward in header field 9, so entity views and document pages share one `pageid` space.*
- ~~**The main namespace.** Whether namespace 0 stays empty, becomes a document namespace, or hosts items as it does on Wikidata.~~ *Settled 2026-09-27 (§2 amendment): reserved and empty, like every namespace MediaWiki or Wikibase uses; its talk namespace is not enabled.*
- ~~**Discussions.** Talk namespaces are reserved (§2). How discussions work is its own ADR.~~ *Settled by [0019](0019-discussions.md): talk namespaces are composite views over threads, and a thread is a page in the `Thread` namespace.*
- ~~**Categories.** Whether category links should ever become data, for example as statements or as a list projection.~~ *Settled by [0038](0038-page-metadata-and-categories.md) §3 and §5: categories are defined only in wikitext and projected as a list; configured mappings turn membership into page statements.*
- ~~**Page protection and permissions.** Who may create, move, delete and protect pages, beyond the owner rule in §6.~~ *Who: settled by [0016](0016-permissions-and-access-control.md) §5. What protection and deletion are: settled by [0023](0023-moderation.md) §1–4, as ACLs on the page.*
- ~~**Search.** Whether one search index covers entity labels and page text together, as `list=search` would expect.~~ *Settled by [0014](0014-caches-and-search.md) §7: two indexes (`entities`, `pages`), one query, with a Postgres fallback (§8).*
- **Page redirects.** Whether document pages may be redirects (`#REDIRECT [[…]]`), and whether a move leaves one for `Project` pages, where no erasable names are involved.

## References

- [Manual:Namespace](https://www.mediawiki.org/wiki/Manual:Namespace) and [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces)
- [Manual:Content handlers](https://www.mediawiki.org/wiki/Manual:ContentHandler)
- [Help:Formatting](https://www.mediawiki.org/wiki/Help:Formatting)
- [API:Expandtemplates](https://www.mediawiki.org/wiki/API:Expandtemplates)
- [CommonMark](https://commonmark.org/) and the [GitHub Flavored Markdown spec](https://github.github.com/gfm/)
