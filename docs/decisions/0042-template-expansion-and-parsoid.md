# 0042. Template expansion and the Parsoid renderer

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-01 (A7)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0024](0024-subsidiary-accounts.md), [0028](0028-tenancy-policy.md), [0030](0030-edit-filters.md), [0033](0033-backend-stack.md), [0034](0034-frontend-stack.md), [0038](0038-page-metadata-and-categories.md)
- **Uses:** [0023](0023-moderation.md), [0039](0039-files-and-media.md), [0041](0041-content-models.md), [0043](0043-lua-modules.md), [0047](0047-special-pages.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

Triplespace renders a fixed subset of wikitext with a parser built into the binary ([0008](0008-namespaces-and-document-pages.md) §8, [0033](0033-backend-stack.md) §9.1). Everything outside the subset stays visible: a template call renders as a chip showing the call. Three earlier decisions depend on that:

| Where | What it assumes |
|---|---|
| [0008](0008-namespaces-and-document-pages.md) §9 | Imported pages are made renderable by one extra **flattening** revision, whose text the source wiki expanded with `action=expandtemplates` |
| [0038](0038-page-metadata-and-categories.md) §3 | "Templates are not parsed": a category that a template emits reaches a page only through flattening |
| `namespaces.toml` | Template (10) is reserved and not implemented |

That is enough for a wiki that starts native. It is not enough for one that wants to keep using MediaWiki's content as it is: an imported wiki that keeps editing its templates, or a mirror of a Wikipedia whose articles are mostly infoboxes, navboxes and citation templates.

**Parsoid on its own does not change this.** Parsoid is the wikitext-to-HTML engine inside MediaWiki. It does not expand templates. Every transclusion and parser function goes to its host's `DataAccess::preprocessWikitext`, which in MediaWiki is the legacy preprocessor, the place where ParserFunctions and Scribunto run. A Parsoid "instance" is therefore only as capable as the expansion behind it.

So the question splits in two:

1. **Expansion.** Turning source into expanded wikitext: templates, template arguments, parser functions, variables and, with [0043](0043-lua-modules.md), Lua.
2. **Rendering.** Turning expanded wikitext into HTML. Parsoid is the high-fidelity option; the built-in renderer is the light one.

James's direction, from the design discussion of 2026-09-30:

- **Expansion is native only.** No configuration hands expansion to another service.
- **A tenant can be configured to render through a Parsoid instance** for full wikitext rendering.
- **Links and categories come from expanded output.**
- **The Template namespace and its talk namespace are enabled when template expansion is enabled.**
- **Renders that depend on mutable remote content are acceptable,** because that is consistent behaviour: a page that uses a remote template shows what the template says now.

## Decision

### 1. Expansion, then rendering (amends 0008 §8 and 0033 §9.1)

**A `wikitext` page renders in two stages, as in MediaWiki:**

1. **Expansion** turns the source into expanded wikitext. It handles templates, template arguments, `<noinclude>`, `<includeonly>` and `<onlyinclude>`, comments, parser functions, variables and, when Lua is on, `{{#invoke:}}` ([0043](0043-lua-modules.md)). Extension tags pass through as tags behind strip markers. The result is what MediaWiki's `action=expandtemplates` returns.
2. **Rendering** turns the expanded text into HTML, with the built-in renderer of [0033](0033-backend-stack.md) §9.1 or with Parsoid (§8).

**Expansion is always native.** It is implemented in Rust (§4) and runs in the binary. When Parsoid renders, it asks Triplespace to expand (§8.1); it never expands on its own and never reaches another wiki's preprocessor.

**With expansion off, stage 1 is the identity.** Today's behaviour stands: template calls and parser functions render as chips, and 0038 §3 reads categories from the source.

**The source is still parsed as source.** Positions for conflict marking and link autocomplete ([0010](0010-site-ui.md) §4) come from the source, as now. Rendering and page metadata (§9) come from the expanded text.

### 2. Settings (extends 0015 §3; extends 0028 §1)

*Changed by A2, A5.*

**Per tenant**, as `site` settings ([0015](0015-record-format-and-partition-registry.md) §3):

| Setting | Values | Default | Meaning |
|---|---|---|---|
| `wikitext.expansion` | `off`, `on` | `off` | Expand templates and parser functions (§4); enables Template and Template talk (§3) |
| `wikitext.lua` | `off`, `on` | `off` | Lua modules ([0043](0043-lua-modules.md) §1). Requires `wikitext.expansion = on` |
| `wikitext.renderer` | `builtin`, `parsoid` | `builtin` | Which stage-2 renderer serves HTML (§7, §8). `parsoid` requires expansion on and a Parsoid service on the instance |
| `pages.repos` | A list of repository names | Empty | Page repositories, in inheritance order ([0052](0052-page-repositories-and-title-inheritance.md) §1); a template repository is one that serves `Template` and `Module` (§11). Replaces `wikitext.template_repos` (A5) |
| `pages.share` | `off`, `on` | `off` | Whether this tenant's pages may serve other tenants as a `tenant` repository ([0052](0052-page-repositories-and-title-inheritance.md) §7). Replaces `wikitext.share` (A5) |

A record that breaks a requirement in the table is refused when it is written, as any invalid `site` record is. Turning `wikitext.expansion` off also turns `wikitext.lua` off and `wikitext.renderer` back to `builtin`.

**Per instance**, a tenancy switch ([0028](0028-tenancy-policy.md) §1) caps what tenants may turn on:

| Switch | Values | `isolated` | `community` | `enterprise` |
|---|---|---|---|---|
| `wikitext.ceiling` | `subset`, `expansion`, `lua` | `lua` | `lua` | `lua` |

The operator lowers it to bound CPU on a crowded farm. Under `config.template = defaults` or `locks` ([0028](0028-tenancy-policy.md) §8), the `wikitext.*` settings can be provisioned into new tenants or locked, like any other `site` setting.

**The Parsoid service is deployment configuration** ([0033](0033-backend-stack.md) §12), not a log record, as the Postgres and OpenSearch addresses are: `parsoid.url`, `parsoid.timeout` (default 30 s) and `parsoid.concurrency`. Without `parsoid.url`, no tenant can choose `parsoid`.

**Changing any `wikitext.*` setting** bumps the render epoch of every `wikitext` page in the tenant and queues them for refresh (§10).

`meta=siteinfo&siprop=triplespace` reports the tenant's settings, the ceiling, and whether a Parsoid service is available.

### 3. The Template namespace (amends 0008 §2)

*Changed by A7.*

**While `wikitext.expansion` is on**, Template (10) is a `pages` namespace and Template talk (11) is enabled:

| Namespace | Kind | Models | Normalizer | Subpages | Talk |
|---|---|---|---|---|---|
| 10 Template | `pages` | `wikitext` | `first-letter` | Yes, as in MediaWiki | 11 |
| 11 Template talk | `pages` | `triplespace-talk` | — | — | — |

**While it is off**, 10 is `reserved` and 11 is not registered, as now. This is a new rule of 0008 §2: **a reserved number can be implemented conditionally**, by a setting named in its `namespaces.toml` entry (`enabled_by = "wikitext.expansion"`).

**Turning expansion off never removes pages.** Template pages that exist keep their records and history. The namespace refuses `create`, `edit` and `move` into it with `ts-namespace-disabled`, and its existing pages stay readable at their titles and can be deleted. Nothing transcludes them. Turning expansion on again restores everything. `meta=siteinfo` lists a disabled namespace while it still holds pages, so clients can read them.

**Any `wikitext` page can be transcluded**, as in MediaWiki, provided the include does not widen: a page *A* includes a page *B* only if every group a `read` restriction on *B* names is also named by one on *A*, and otherwise `{{:B}}` renders as a link to a missing page for every viewer, so that one rendering per visibility serves every reader of *A* ([0056](0056-security-model.md) §6). The forms are MediaWiki's: `{{Foo}}` is `Template:Foo`; `{{:Foo}}` is the main-namespace page; `{{User:Example/box}}` and `{{Project:Notice}}` name their namespace. A page of another text model transcludes its text, as MediaWiki's `TextContent` does. An entity, thread or talk page has no wikitext form and transcludes as a link to itself, `[[:Item:Q42]]`, which is what MediaWiki produces for content it cannot transclude.

### 4. The expander

*Changed by A6.*

**The expander is a new pure crate, `scatter-wikitext-expand`** (§19). It implements MediaWiki's preprocessor and frame expansion:

- **The preprocessor tree** of MediaWiki's `Preprocessor_Hash`: template, template-argument, comment, ignored, extension-tag and heading nodes, with `<onlyinclude>`, `<includeonly>` and `<noinclude>` resolved for transclusion or for viewing.
- **Frames:** numbered and named arguments, defaults (`{{{1|x}}}`), lazy argument expansion, and the parent chain that `#invoke` reads.
- **Title resolution in transclusion:** `Template:` by default; a leading colon for the main namespace; the `msg:`, `msgnw:` and `raw:` modifiers; `subst:` and `safesubst:` (§6); one level of redirect.
- **Loops and limits:** template loop detection, and MediaWiki's limits on expansion depth, template depth, node count, post-expand include size, template argument size and expensive function calls (§16).
- **Strip markers** for extension tags and `<nowiki>`, in MediaWiki's form, so that Lua's `mw.text.unstrip` and `mw.text.killMarkers` behave as they do on MediaWiki.

**It does no I/O.** Every fact it needs comes through a host trait, `ExpandHost`, implemented by `triplespace-render` (§19) over the serving model:

- page source by title as of a position, with the page ID and offset, or "missing";
- title facts (existence, redirect target, page ID, size, protection, content language);
- namespaces and site statistics;
- the render's one timestamp ("now"), fixed per render as MediaWiki fixes it per parse;
- file facts ([0039](0039-files-and-media.md));
- `#invoke` ([0043](0043-lua-modules.md)).

Given the same host answers, it produces the same output. The same code serves page views, `action=expandtemplates`, pre-save transform, server preview and Parsoid's callbacks.

**What expansion returns:** the expanded text, the strip state, the page properties it set (`DEFAULTSORT`, `DISPLAYTITLE`, `SHORTDESC`, `__DISAMBIG__`, behaviour switches), which the refresh job stores in `view.page_prop` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6), the render manifest (§10), the limit report and any errors.

**It is written from MediaWiki's documented behaviour and test files, not translated from MediaWiki's PHP.** That keeps the crate original code under the shared crates' dual licence ([0005](0005-crate-organization.md) §6). The same applies to `#expr` and `#time` (§5).

### 5. Variables, parser functions, tags and switches: a registry (extends 0015 §5)

*Changed by A6.*

**What the expander knows is registry data**, in a new file, `docs/registry/wikitext-functions.toml`, embedded by `scatter-wikitext-expand`. Each entry is a variable, parser function, extension tag or behaviour switch, with its synonyms, its origin (MediaWiki core, ParserFunctions, Scribunto, Wikibase Client, Cite, or another extension) and a status:

| Status | Meaning |
|---|---|
| `implemented` | Expanded or rendered as MediaWiki does |
| `chip` | Recognised and rendered as a visible chip showing the source, as unsupported syntax is now (0008 §8) |
| `ignored` | Recognised and without effect: a behaviour switch that does nothing here, or a tag that renders nothing |

**The registry is what `meta=siteinfo` reports** for `magicwords`, `functionhooks`, `extensiontags`, `variables` and `doubleunderscores`, so clients and Parsoid (§8) tokenize exactly what the expander handles. `chip` entries are reported too: they are installed in the sense that the parser recognises them.

**Initial coverage:**

| Group | Status |
|---|---|
| **Core variables:** page-name family (`PAGENAME`, `FULLPAGENAME`, `BASEPAGENAME`, `ROOTPAGENAME`, `SUBPAGENAME`, `TALKPAGENAME`, `SUBJECTPAGENAME` and the `E` forms), namespace family, revision family (`REVISIONID`, `REVISIONTIMESTAMP`, `REVISIONUSER`, `REVISIONSIZE` and the date parts), `CURRENT*` and `LOCAL*` time, statistics (`NUMBEROFPAGES`, `NUMBEROFARTICLES`, `NUMBEROFFILES`, `NUMBEROFUSERS`, `NUMBEROFACTIVEUSERS`, `NUMBEROFEDITS`), `PAGEID`, `SITENAME`, `SERVER`, `SERVERNAME`, `SCRIPTPATH`, `CONTENTLANGUAGE`, `DIRECTIONMARK`, `!` and `=` | `implemented` |
| **Core parser functions:** `ns`, `nse`, `lc`, `uc`, `lcfirst`, `ucfirst`, `urlencode`, `anchorencode`, `localurl`, `fullurl`, `canonicalurl` (and `E` forms), `filepath`, `formatnum`, `plural`, `grammar`, `gender`, `padleft`, `padright`, `int`, `#language`, `#special`, `#speciale`, `#tag`, `#formatdate`, `#bcp47`, `#dir`, `#interwikilink`, `#interlanguagelink`, `DEFAULTSORT`, `DISPLAYTITLE`, `PAGESINCATEGORY`, `PAGESIZE`, `PROTECTIONLEVEL`, `PROTECTIONEXPIRY`, `NUMBERINGROUP` | `implemented`. `CASCADINGSOURCES` is implemented and always empty, since there is no cascading protection ([0016](0016-permissions-and-access-control.md) §4) |
| **ParserFunctions:** `#expr`, `#if`, `#ifeq`, `#iferror`, `#ifexpr`, `#ifexist`, `#rel2abs`, `#switch`, `#time`, `#timel`, `#timef`, `#timefl`, `#titleparts` | `implemented`. The string functions (`#len`, `#pos` and the rest) are not registered, since ParserFunctions leaves them off by default; a call renders as literal text, as on MediaWiki |
| **Scribunto:** `#invoke` | `implemented` when `wikitext.lua` is on ([0043](0043-lua-modules.md)); otherwise `chip` |
| **Wikibase Client:** `#property`, `#statements` | `implemented` when expansion is on ([0043](0043-lua-modules.md) §7) |
| **Wikibase Client:** `SHORTDESC` | `implemented`, as the page property `wikibase-shortdesc` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6); `__EXPECTED_UNCONNECTED_PAGE__` is `ignored` |
| **Core:** `#REDIRECT` | `implemented` ([0051](0051-page-redirects.md) §1) |
| **Extension tags:** `templatestyles`, `templatedata` | `implemented` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §3, §5) |
| **Extension tags:** `nowiki`, `pre`, `ref`, `references`, `gallery`, `indicator`, `poem`; `syntaxhighlight` and `source` as plain `<pre>` | `implemented` |
| **Extension tags:** `math`, `chem`, `ce`, `score`, `timeline`, `graph`, `mapframe`, `maplink`, `categorytree`, `inputbox`, `imagemap`, `hiero`, `charinsert` | `chip` |
| **Extension tag:** `section` (Labeled Section Transclusion) | `ignored`: it renders nothing, as on MediaWiki. `#lst` and its relatives are not registered |
| **Behaviour switches:** `__NOTOC__`, `__TOC__`, `__FORCETOC__`, `__HIDDENCAT__` | `implemented` (0008 §8, [0038](0038-page-metadata-and-categories.md) §3) |
| **Behaviour switch:** `__DISAMBIG__` | `implemented`, as the page property `disambiguation` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6) |
| **Behaviour switches:** `__NOEDITSECTION__`, `__NEWSECTIONLINK__`, `__NONEWSECTIONLINK__`, `__NOGALLERY__`, `__INDEX__`, `__NOINDEX__`, `__STATICREDIRECT__`, `__EXPECTUNUSEDCATEGORY__`, `__NOCONTENTCONVERT__`, `__NOTITLECONVERT__` | `ignored` for rendering; each is stored as a page property where MediaWiki stores one ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6) |

**Messages.** `{{int:}}`, tracking-category names (§9) and Parsoid's `meta=allmessages` calls (§8.1) read MediaWiki core's message files for the languages the instance serves, shipped as data. The MediaWiki namespace (8) stays reserved, so no wiki can override a message.

**The core rows are generated, not hand-kept.** A script reads the reference install's `meta=siteinfo` with the five properties above, once ParserFunctions and Scribunto are installed there (§18), and writes the core and extension entries with their synonyms; the statuses are hand-edited. The file in the repository is the seed and the shape, as `sites.toml` is.

### 6. Pre-save transform (extends 0008 §8)

**With expansion on, `subst:` and `safesubst:` expand when a page is saved,** as MediaWiki's pre-save transform does, alongside `~~~~` (0008 §8). The saved text is the substituted text. `{{subst:#invoke:…}}` works when Lua is on. With expansion off, `subst:` stays in the text and renders as a chip.

### 7. The built-in renderer after expansion (amends 0010 §4)

*Changed by A6.*

The built-in renderer renders the **expanded** text with the subset of 0008 §8. After expansion:

- **No template calls remain to chip.** A missing template renders as MediaWiki renders it: a red link to `Template:Name`.
- **Chips are for what is still outside the subset:** extension tags with status `chip` (§5), and HTML or table attributes the sanitizer drops are dropped as now. `<templatestyles>` renders as a deduplicated `<style>` element holding the sheet's sanitized form ([0055](0055-templatestyles-templatedata-and-page-properties.md) §3).
- **Expansion errors** render as MediaWiki's error spans (`<strong class="error">…</strong>`) and add their tracking category (§9).

With expansion off, the "Template not rendered" chip of 0010 §4 is unchanged.

### 8. The Parsoid renderer

#### 8.1 Parsoid calls back into Triplespace

*Changed by A6.*

Parsoid's host interface, `SiteConfig`, `DataAccess` and `PageConfig`, has an implementation over a remote wiki's Action API in Parsoid itself (`Wikimedia\Parsoid\Config\Api\*`, the code behind `bin/parse.php --apiURL`). **Triplespace points those classes at the tenant's own Action API**, so Triplespace is Parsoid's host and the single source of truth. The requests Parsoid sends, and what serves them:

| Parsoid needs | Request | Served by |
|---|---|---|
| Site configuration | `meta=siteinfo`, `siprop=general\|protocols\|namespaces\|namespacealiases\|magicwords\|interwikimap\|languagevariants\|defaultoptions\|specialpagealiases\|extensiontags\|functionhooks\|variables\|doubleunderscores` | The registries; §5 for the parser properties; `interwikimap` from site aliases ([0026](0026-sitelinks.md) §2); `languagevariants` empty |
| Feature detection | `action=paraminfo&modules=query` | Existing |
| Template source | `prop=revisions`, `rvprop=content\|ids`, `rvslots=*`, `redirects` | Page records, then foreign repositories (§11) |
| Link classes (red links, redirects) | `prop=info`, `inprop=linkclasses` | The title resolver |
| Files | `prop=imageinfo` with `iibadfilecontexttitle` and `iiprop=mediatype\|mime\|size\|url\|badfile\|sha1\|timestamp\|canonicaltitle` | [0039](0039-files-and-media.md) §17 |
| Expansion | `action=expandtemplates`, `prop=wikitext\|modules\|jsconfigvars\|categories\|properties`, `showstrategykeys` | The expander (§4) |
| Tags Parsoid does not implement natively | `action=parse`, `prop=text\|modules\|jsconfigvars\|categories\|properties\|externallinks` | The built-in renderer, never Parsoid (§8.2) |
| Tracking-category names | `meta=allmessages` | Shipped messages (§5) |
| TemplateData | `action=templatedata` | `view.page_prop` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §5). Only Parsoid's HTML-to-wikitext direction uses it, and reading never calls it |

**Rejected alternatives:**

- **A sidecar MediaWiki holding copies of the templates.** Two sources of truth, a synchronisation job, and a Wikibase Client that cannot read Triplespace's entities.
- **Parsoid's experimental native template expansion.** It has neither ParserFunctions nor Scribunto, so it fails the case this ADR exists for.

#### 8.2 The render-scoped API

**Each Parsoid render gets its own API endpoint:** `{tenant base}/w/render/{token}/api.php`, where the token is 128 random bits, valid for `parsoid.timeout`, naming the tenant, the page and the render. Requests under it:

- **read as an anonymous reader**, because shared caches hold only the public form ([0014](0014-caches-and-search.md) §1);
- **are not rate-limited**, and serve only the read modules of §8.1; anything else is refused;
- **record what they read into the render's manifest** (§10), so a Parsoid render has the same dependencies as a built-in one;
- **never call Parsoid.** `action=parse` under a render token always uses the built-in renderer, so there is no recursion.

Expansion results are memoized within a render, since Parsoid calls `expandtemplates` once per top-level transclusion. Requests to the plain `api.php` are ordinary requests, whatever their origin.

#### 8.3 The Parsoid service (amends 0033 §1)

**Parsoid has no HTTP server of its own outside MediaWiki.** Triplespace therefore ships one, in `services/parsoid/`: a small PHP front controller that requires `wikimedia/parsoid` through Composer, builds the API-backed configuration for the endpoint each request names, and answers one route:

```
POST /transform/wikitext/to/html
{ "api": "{tenant base}/w/render/{token}/api.php",
  "title": "…", "revid": 123, "wikitext": "…", "page_language": "en" }
→ 200 { "html": "…", "data_parsoid": {…}, "data_mw": {…}, "parsoid_version": "…" }
```

- **It is a separate process,** stateless and horizontally scalable, deployed as its own OCI image. Its version of Parsoid is pinned to the one MediaWiki 1.43 bundles, so the differential tests (§18) compare like with like.
- **It is the one optional service that needs a PHP runtime.** 0033 §1's first principle still holds: a small instance runs with the binary and Postgres, and Parsoid joins Valkey, OpenSearch and QLever among the optional services.
- **It is not linked into the binary,** so Parsoid's GPL-2.0-or-later licence is a separate program's licence and 0033 §1's linking list does not apply to it.

#### 8.4 Output and failure

**Parsoid's HTML passes through Triplespace's sanitizer** with Parsoid's attribute allowlist (`typeof`, `about`, `data-mw`, `rel`, `property`), and links to entity titles get their labels, as the built-in renderer gives them ([0008](0008-namespaces-and-document-pages.md) §8).

**If Parsoid fails or times out, the page renders with the built-in renderer,** with a notice and a 60-second cache lifetime so that the next read tries again. Parsoid being unavailable never stops a page being read.

**Page metadata never depends on Parsoid.** Links, categories and the manifest come from the expander and the source parser (§9), so they are the same whichever renderer serves the HTML.

### 9. Links, categories and other metadata come from expanded output (amends 0038 §3; amends 0008 §10)

*Changed by A6.*

**While expansion is on**, everything a page links to or belongs to is read from its **expanded** text, as in MediaWiki:

- page and entity links ([0008](0008-namespaces-and-document-pages.md) §10);
- category membership, sort keys, `DEFAULTSORT` and `__HIDDENCAT__` ([0038](0038-page-metadata-and-categories.md) §3);
- file usage (`view.file_link`, [0039](0039-files-and-media.md) §20);
- external links, page properties and indicators.

A template's `<includeonly>` categories reach the pages that transclude it; its `<noinclude>` categories stay on the template page.

**Tracking categories join.** Expansion adds MediaWiki's tracking categories for its own conditions, named by MediaWiki's message keys and so by MediaWiki's names: `expansion-depth-exceeded-category`, `template-loop-category`, `post-expand-template-inclusion-category`, `post-expand-template-argument-category`, `node-count-exceeded-category`, `expensive-parserfunction-category` and `broken-file-category`, Scribunto's `scribunto-common-error-category` ([0043](0043-lua-modules.md) §12) and TemplateStyles' `templatestyles-stylesheet-error-category` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §3). This replaces 0038 §3's "out of scope" for the categories expansion itself detects.

**0038's rule that a category is defined in one place still holds.** The place is now the expanded text: the page's own text and the templates it uses. Categories remain a projection, never records.

**While expansion is off,** 0038 §3 is unchanged.

**These rows are written by the refresh job (§10), not by the page projection.** Computing them may read other pages and run Lua, which does not belong inside the appending transaction ([0013](0013-postgres-storage.md) §7). A page's links follow its edit after the refresh queue, as MediaWiki's deferred links update does.

### 10. The render manifest and refresh (extends 0013 §5.6 and 0014 §3–5)

*Changed by A2, A3, A6.*

**Every expansion produces a manifest:** what it read, and until when the result holds.

| Entry | Recorded as |
|---|---|
| A transcluded or tested page | Namespace and title, with page ID and offset, or **missing**, so that creating the page invalidates (red links, `#ifexist`, missing templates). A stylesheet loaded by `<templatestyles>` is one ([0055](0055-templatestyles-templatedata-and-page-properties.md) §3) |
| An entity | Entity ID and usage aspects ([0043](0043-lua-modules.md) §10) |
| A file | Title and the file version used |
| A foreign repository page | Repository, title, remote revision ID and expiry (§11) |
| A volatile input | Current time, site statistics, `PAGESINCATEGORY` and the like, with the earliest moment the output could change |

**Tables** (`view`, written by the refresh job):

| Table | Holds | MediaWiki counterpart |
|---|---|---|
| `transclusion` | `(page_id, target_ns, target_title, target_page_id NULL, repo NULL)` | `templatelinks` |
| `render_state` | `(page_id, epoch, rendered_offset, expires_at, renderer, settings_hash, limit_report, errors)` | `page_touched`, the parser cache's expiry, the limit report |
| `entity_usage` | Defined in [0043](0043-lua-modules.md) §10 | `wbc_entity_usage` |

The queue is `ops.render_refresh`: page IDs with a reason, deduplicated.

**These are the first `view` tables that are not a pure function of the log.** They are rebuilt by re-rendering every `wikitext` page, and where a render read a foreign repository or the clock, the rebuild may differ from the original. This amends [0013](0013-postgres-storage.md) §5.6's rule that every `view` table is rebuilt from the log, for these three tables and for the links and categories they now drive (§9).

**What triggers a refresh:**

1. A page's own new revision.
2. A change to a page another page depends on: a new revision, creation, deletion, undeletion, move, or a `read` ACL. Every page whose `transclusion` row names it, by page ID or by title, gets an epoch bump and a refresh, in batches, as MediaWiki's `HTMLCacheUpdate` and `refreshLinks` jobs do.
3. A change to an entity's `resolved_version`, for usage rows whose aspects the change touches ([0043](0043-lua-modules.md) §10).
4. `expires_at` passing, for volatile and foreign inputs. This is lazy: the next read finds the entry expired and renders again.
5. A change to any `wikitext.*` setting (§2).
6. `action=purge` with `forcelinkupdate` or `forcerecursivelinkupdate`.

**Caching (amends 0014 §3 and §4).** A rendered page's version gains the page's **render epoch**:

```
p:{pageid}:{gen}:{offset}:{epoch}:{renderer}:{lang}
```

Its lifetime is the lower of 0014 §4's ceiling and `expires_at`. A miss renders synchronously under single flight (0014 §4); there is no stale serving after an epoch bump, as MediaWiki renders afresh after `page_touched` moves.

**Erasure, deletion and hiding reach transcluding pages (extends 0014 §5).** Every rendered response carries a `Cache-Tag` for each page and entity in its manifest. Erasing, deleting or hiding a dependency bumps the epoch of every page that depends on it, deletes their `p:` keys, and purges their tags, because the public form of those pages has changed.

**Reports and statistics.** The refresh job applies report deltas (`view.report_entry`) in the same transaction as the `transclusion`, link, category and `entity_usage` rows it writes ([0047](0047-special-pages.md) §4.3). Site statistics, a volatile input here, are read from `view.site_stats` ([0047](0047-special-pages.md) §13).

### 11. Foreign template repositories (uses 0039 §11)

*Changed by A5.*

**A template repository is a page repository that serves `Template` and `Module`** ([0052](0052-page-repositories-and-title-inheritance.md) §1): a `config` record of kind `page-repo`, keyed `page-repo:{name}`, in a tenant's or the instance's `config`, listed in inheritance order in the tenant's `pages.repos`, and configured as file repositories are ([0039](0039-files-and-media.md) §11). The `template-repo` kind and `wikitext.template_repos` are retired (A5). The lookup below is the title stack of [0052](0052-page-repositories-and-title-inheritance.md) §3, as it applies to transclusion; `mirror` mode for a repository is [0053](0053-mirrored-pages.md) §5.

**Lookup is local first, then each repository in order.** It applies to Template and Module titles ([0043](0043-lua-modules.md) §2), including data modules. A local page shadows a foreign one of the same title; no special right is needed, since shadowing a template is an ordinary edit. **Lookup restarts at the local tenant at every level:** a foreign `Template:Infobox` that calls `{{Infobox/row}}` gets the local `Template:Infobox/row` if one exists. That is the consistent rule, and it is how a wiki adapts a borrowed template.

| Kind | What it is | How it is read |
|---|---|---|
| `tenant` | Another tenant on this instance | Directly, from the source tenant's pages, with its ACLs. The source must set `pages.share = on`; provider reader lists apply ([0028](0028-tenancy-policy.md) §5); a private tenant cannot share |
| `mediawiki` | Any MediaWiki Action API, a Wikipedia above all, or a Triplespace tenant elsewhere | `prop=revisions&rvprop=content\|ids&rvslots=main&redirects` through the live upstream client ([0012](0012-api-requirements.md) §6): User-Agent and `maxlag` set, rate-limited, cached, never logged |

**`mediawiki` repositories proxy.** Fetched source is cached for `repo.cache_ttl` (default one hour), and each manifest entry carries the remote revision ID and that expiry, so a page using a remote template refreshes when the cache does (§10). **The render shows the remote template as it is now**, which is the behaviour James chose: a borrowed template behaves like a local one that someone else edits.

**Foreign source is untrusted input,** exactly as an edit is: the same expander, limits and sanitizer apply.

**A remote template is expanded in the local page's context.** `{{PAGENAME}}` is the local page; its links resolve locally.

**Attribution.** A page that used a foreign repository lists it at its foot, "Uses templates from English Wikipedia", linking each foreign page used, because reused wiki text carries its licence's attribution requirement. Files use the same pattern ([0039](0039-files-and-media.md) §11).

### 12. Rendering a revision as of its time (extends 0012 §5)

**The Action API and MediaWiki's REST API keep MediaWiki's meaning:** `action=parse&oldid=` and `/v1/revision/{id}/html` render an old revision with the templates as they are now.

**`triplespace/v0` adds what an append-only log makes possible:** `GET /page/{id}/html?revision={revid}&as_of=revision` renders the revision with every local page it depends on **as of the revision's offset**.

- Local pages are exact, since every revision is in the log.
- Entities come from the tenant's view as of that offset where the log holds it, and current otherwise.
- Foreign repositories are current.

The response says which, with `as_of_exact`. An exact response is immutable and caches as record-derived data does ([0014](0014-caches-and-search.md) §3).

### 13. Import (amends 0008 §9)

**With expansion on, flattening (0008 §9 step 3) becomes optional.** The template census of 0008 §9 now also reports:

- which templates and modules the pages use, and which of those the export contains;
- which variables, functions and tags fall outside the registry (§5).

The importer may bring the Template and Module pages with full history, as ordinary pages, instead of flattening. Flattening remains the procedure for a tenant with expansion off, or by choice.

### 14. API (extends 0012 §4 and §5)

| Module or route | Behaviour |
|---|---|
| `action=expandtemplates` | MediaWiki's parameters: `text`, `title`, `revid`, `prop=wikitext\|categories\|properties\|volatile\|ttl\|modules\|jsconfigvars\|encodedjsconfigvars\|parsetree`, `includecomments`, `showstrategykeys`. With expansion off: `ts-expansion-off` |
| `action=parse` | Renders with expansion. Gains `prop=templates`, `limitreportdata`, `limitreporthtml` and `parsetree`. `pst`, `onlypst` and `preview` as in MediaWiki. `parsoid=1` returns Parsoid's HTML where the tenant has it configured |
| `prop=templates`, `prop=transcludedin`, `list=embeddedin`, `list=alltransclusions` | From `view.transclusion` |
| `meta=siteinfo` | `magicwords`, `functionhooks`, `extensiontags`, `variables`, `doubleunderscores` from the registry (§5); `protocols`, `interwikimap`, `languagevariants`, `defaultoptions` and `specialpagealiases` for Parsoid (§8.1) |
| `prop=info` | `inprop=linkclasses` |
| `meta=allmessages` | The shipped MediaWiki messages (§5) |
| `action=purge` | `forcelinkupdate` and `forcerecursivelinkupdate` queue refreshes (§10) |
| `GET /page/{id}/render` *(REST v0)* | The page's manifest summary: templates and modules used, entities and aspects, foreign repositories, `expires_at`, renderer, limit report and errors |
| `GET /page/{id}/html` *(REST v0)* | Gains `as_of` (§12) |

### 15. Editing and preview (amends 0010 §4; amends 0034 §6)

**The browser preview stays WebAssembly where it can.** When the source contains template calls, parser functions, variables or tags that the wasm renderer cannot render and expansion is on, the editor previews through `action=parse` with `pst` and `preview`, debounced, under the `parse` rate class (§16). Pages without such constructs keep the in-browser preview of [0034](0034-frontend-stack.md) §6.

**The "About this page" panel** ([0010](0010-site-ui.md) §4) gains "Templates used" (`prop=templates`) and the limit report. A Template page gains "Pages that use this template" (`list=embeddedin`).

### 16. Limits, rate limits and errors (extends 0024 §5)

**Limits** are deployment configuration with MediaWiki's names and MediaWiki's defaults, read from the reference install's `limitreport` output: maximum article size, post-expand include size, template argument size, expansion depth, node count, and expensive function count (shared with Lua, [0043](0043-lua-modules.md) §4). A tenant may lower them and not raise them.

**A new rate class, `parse`** ([0024](0024-subsidiary-accounts.md) §5), counts the expansions a client asks for directly: `action=parse` with `text`, `action=expandtemplates`, server preview, and the Lua console if it is ever added ([0043](0043-lua-modules.md) Q1). The default is 60 per minute for `user` and 600 per minute for `bot`. Page views are not counted; they are cached renders.

### 17. Search and filters (extends 0014 §7; amends 0030 §2)

- **Search.** With expansion on, the `text` field of the `pages` index is the rendered text, as CirrusSearch indexes it, and `source_text` keeps the source. A `hastemplate:` keyword reads `view.transclusion`.
- **Filters.** `added_links` and `removed_links` ([0030](0030-edit-filters.md) §2) come from the expanded text when expansion is on. They are computed only when an enabled filter reads them, as AbuseFilter computes its variables lazily.

### 18. Conformance (extends 0033 §15)

- **The preprocessor sections of MediaWiki's `parserTests.txt`**, and ParserFunctions' parser tests, run in CI as fixtures. They are GPL test data and are not compiled in, as 0033 §9.1 already arranges for the core file.
- **A differential `action=expandtemplates` test** against the MediaWiki 1.43 reference install over a corpus of imported pages, templates and modules. **The reference install gains ParserFunctions and Scribunto**, and keeps WikibaseClient, which [0043](0043-lua-modules.md) needs as well.
- **The Parsoid differential test of 0033 §9.1 now covers templated pages:** the same pages rendered by the reference install's Parsoid and by Triplespace with the Parsoid service, with DOMs normalized and compared.
- **Fuzzing** (`cargo-fuzz`) covers the preprocessor: random input must terminate within the limits and never panic.

### 19. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with `scatter-wikitext-expand`, `triplespace-render` and every change this section listed. `services/parsoid/` is a PHP program, not a crate. The table this section first gave is in A1.

## Consequences

- **Wikis built on templates can live here unchanged.** An imported wiki keeps editing its templates, and a Wikipedia mirror can render its infoboxes, through the built-in renderer at modest fidelity or through Parsoid at MediaWiki's.
- **Parsoid adds no second source of truth.** It reads everything from Triplespace, through a render-scoped API that records what it read.
- **Expansion has one implementation,** used for views, `expandtemplates`, pre-save transform, preview and Parsoid's callbacks, so the API and the page agree.
- **A render now depends on more than its page.** Template edits, entity changes, the clock and remote wikis move a page's output. The manifest, the epoch and the refresh queue make that explicit, and a template used on a million pages costs a million refreshes, as it does on MediaWiki.
- **Some `view` tables are no longer replayable.** Links, categories and transclusions on a tenant with expansion on are rebuilt by re-rendering, and with foreign repositories or volatile inputs the rebuild can differ. The log stays the source of truth for what was written; it is not the record of what was rendered.
- **Categories can come from templates.** 0038's projection now reads the expanded text, so categorizing by template works, and so do tracking categories for broken templates.
- **One optional service needs PHP.** Instances that never choose Parsoid never run it.
- ~~**Pages will look plainer than on Wikipedia** until site styles exist. Templates assume `MediaWiki:Common.css` and TemplateStyles, and neither is available (Q2, Q3).~~ *TemplateStyles and one site-styles page are available ([0055](0055-templatestyles-templatedata-and-page-properties.md) §3–4; A6).*
- **History can render as it was.** `as_of=revision` is something MediaWiki cannot offer, and it falls out of the append-only log.

## Open questions

- **Q1. Cascading protection.** With transclusion, protecting a page without protecting its templates protects little. `cascade=1` is still refused ([0023](0023-moderation.md) §8); whether to implement it as an enclosure over `view.transclusion`.
- **Q2.** ~~**TemplateStyles.** A `sanitized-css` model and a CSS sanitizer in Rust. 0008 §5 excluded it with the other style models; templates from Wikipedia depend on it.~~ *Settled by [0055](0055-templatestyles-templatedata-and-page-properties.md) §1–3: the `sanitized-css` model, the `scatter-css` sanitizer, and `<templatestyles>` implemented.*
- **Q3.** ~~**Site styles.** Whether a tenant may have any site CSS, given 0008 §5's exclusion of `css`.~~ *Settled by [0055](0055-templatestyles-templatedata-and-page-properties.md) §4: one designated `sanitized-css` page, named by `wikitext.site_styles`, scoped to rendered content.*
- **Q4.** ~~**Mirror mode for template repositories,** copying used foreign pages into an instance partition as 0039 §11's file mirrors do, for availability and verification.~~ *Settled by [0053](0053-mirrored-pages.md) §5: a page repository in `mirror` mode keeps its pages, templates and modules included, in the `pages/{repo}` instance partition.*
- **Q5.** ~~**Push invalidation from Wikimedia repositories.** Following EventStreams `recentchange` for the repository's Template and Module namespaces, instead of waiting for `repo.cache_ttl`.~~ *Settled by [0053](0053-mirrored-pages.md) §6: a repository's `events` stream bumps the render epoch of every page whose manifest names a changed foreign template.*
- **Q6. Editing through Parsoid HTML.** VisualEditor-style editing needs Parsoid's HTML-to-wikitext direction, page bundles stored per revision, and TemplateData.
- **Q7. Section editing of transcluded sections,** which MediaWiki numbers `T-1`.
- **Q8. Language variants.** `languagevariants` is empty and LanguageConverter is not implemented.
- **Q9. Labeled Section Transclusion** (`#lst`), which Wikisource depends on.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2, §3, §7 | §19 | extends | 0005 A42 |
| [0008](0008-namespaces-and-document-pages.md) §8, §9 | §1, §3, §7–9, §13 | amends | 0008 A12 |
| [0008](0008-namespaces-and-document-pages.md) §2, §10 | §1, §3, §7–9, §13 | extends | 0008 A12 |
| [0010](0010-site-ui.md) §4 | §7, §15 | amends | 0010 A26 |
| [0012](0012-api-requirements.md) §4, §5 | §12, §14 | extends | 0012 A26 |
| [0013](0013-postgres-storage.md) §5.6 | §10 | amends | 0013 A16 |
| [0013](0013-postgres-storage.md) §7 | §10 | extends | 0013 A16 |
| [0014](0014-caches-and-search.md) §3, §4 | §10, §17 | amends | 0014 A7 |
| [0014](0014-caches-and-search.md) §5, §7, §10 | §10, §17 | extends | 0014 A7 |
| [0015](0015-record-format-and-partition-registry.md) §3, §5 | §2, §5, §11 | extends | 0015 A19 |
| [0024](0024-subsidiary-accounts.md) §5 | §16 | extends | 0024 A5 |
| [0028](0028-tenancy-policy.md) §1 | §2 | extends | 0028 A6 |
| [0030](0030-edit-filters.md) §2 | §17 | amends | 0030 A6 |
| [0033](0033-backend-stack.md) §1, §9.1 | §1, §8, §18 | amends | 0033 A3 |
| [0033](0033-backend-stack.md) §15 | §1, §8, §18 | extends | 0033 A3 |
| [0034](0034-frontend-stack.md) §6 | §15 | amends | 0034 A1 |
| [0038](0038-page-metadata-and-categories.md) §3 | §9 | amends | 0038 A4 |

## References

- [Manual:Preprocessor](https://www.mediawiki.org/wiki/Manual:Preprocessor), [Help:Templates](https://www.mediawiki.org/wiki/Help:Templates), [Help:Magic words](https://www.mediawiki.org/wiki/Help:Magic_words), [Help:Extension:ParserFunctions](https://www.mediawiki.org/wiki/Help:Extension:ParserFunctions)
- [Parsoid](https://www.mediawiki.org/wiki/Parsoid); `wikimedia/parsoid` `src/Config/Api/{SiteConfig,DataAccess,PageConfig}.php` and `bin/parse.php` (`--apiURL`)
- [API:Expandtemplates](https://www.mediawiki.org/wiki/API:Expandtemplates), [API:Parse](https://www.mediawiki.org/wiki/API:Parse), [API:Siteinfo](https://www.mediawiki.org/wiki/API:Siteinfo)
- [Manual:Templatelinks table](https://www.mediawiki.org/wiki/Manual:Templatelinks_table), [Manual:Job queue](https://www.mediawiki.org/wiki/Manual:Job_queue) (`refreshLinks`, `htmlCacheUpdate`)
- [Help:Tracking categories](https://www.mediawiki.org/wiki/Help:Tracking_categories)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-30
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §19
- **Summary:** 0005 §2 is the one crate table CI checks, and carries the two new crates and every change this section listed (0005 A42).

Replaced text (§19):

> | Crate | Change |
> |---|---|
> | `scatter-wikitext-expand` *(new)* | Substrate, pure, no C dependencies. The preprocessor tree, frames, strip markers, pre-save `subst:`, core variables and parser functions, ParserFunctions, the registry embedding `docs/registry/wikitext-functions.toml`, the `ExpandHost` trait, and the manifest and limit-report types (§4–6, §10). Depends on the ICU4X crates already chosen ([0033](0033-backend-stack.md) §9.3) for dates, numbers and plurals |
> | `scatter-wikitext` | Renders expanded text with its strip state; extracts links, categories, file links, external links and indicators from expanded text (§7, §9); `poem` and `indicator` |
> | `triplespace-render` *(new)* | `ExpandHost` over the serving model; the render pipeline and renderer choice; the Parsoid client and the render-scoped endpoint's manifest collection (§8); the refresh job, `ops.render_refresh` and the invalidation fan-out (§10); foreign template repositories (§11). Depends on `triplespace-db`, `triplespace-projections`, `triplespace-titles`, `triplespace-cache`, `triplespace-upstream`, `scatter-wikitext`, `scatter-wikitext-expand` and `scatter-pages` |
> | `triplespace-titles` | Conditional namespaces (`enabled_by`) and the disabled-namespace refusal (§3) |
> | `triplespace-db` | Migrations for `view.transclusion`, `view.render_state` and `ops.render_refresh` |
> | `triplespace-cache` | The render epoch in `p:` keys; dependency `Cache-Tag`s (§10) |
> | `triplespace-search` | Rendered `text`, `source_text` and `hastemplate:` (§17) |
> | `triplespace-api-action`, `triplespace-api-rest` | The modules and routes of §14; the render-scoped endpoint (§8.2) |
> | `triplespace-cli` | `render refresh` (a tenant or the instance) and `render rebuild` (§10) |
>
> `services/parsoid/` is a PHP program, not a crate. The workspace goes from forty-nine crates to fifty-one.

### A2. Lua: the `wikitext.lua` setting and entity usage

- **Date:** 2026-09-30
- **Source:** [0043](0043-lua-modules.md) §1, §10
- **Change:** extends §2, §10
- **Summary:** `wikitext.lua` (`off`, `on`; default `off`; requires `wikitext.expansion = on`) joins the settings of §2, and the manifest records an entity with the usage aspects of 0043 §10, kept in `view.entity_usage`. Both were written into §2 and §10 before this ADR was first committed, together with 0043 (commit `644c2b6`).

### A3. Report deltas and site statistics

- **Date:** 2026-09-30
- **Source:** [0047](0047-special-pages.md) §4.3, §13
- **Change:** extends §10
- **Summary:** The refresh job applies report deltas (`view.report_entry`) in the same transaction as the `transclusion`, link, category and `entity_usage` rows it writes. Site statistics, a volatile input here, are read from `view.site_stats`.

### A4. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §10, §19
- **Summary:** A1–A3 were folded into the Decision. The open questions were numbered, and the headings of §1, §11 and §17 now use the verbs of 0050 §3 (amends, uses). No decision changed. Before this, A3 was a blockquote, A2 was written in place with no note, and A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A5. Template repositories become page repositories

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §1, §3
- **Change:** amends §2, §11
- **Summary:** The `template-repo` config kind is retired into `page-repo`, a repository that may serve any `pages` namespace; `wikitext.template_repos` becomes `pages.repos` and `wikitext.share` becomes `pages.share`. §11's lookup is the title stack of 0052 §3 applied to transclusion, and is otherwise unchanged. Nothing had been written under the old names.

Replaced text (§2):

> | `wikitext.template_repos` | A list of repository names | Empty | Foreign template repositories, in lookup order (§11) |
> | `wikitext.share` | `off`, `on` | `off` | Whether this tenant's Template and Module pages may serve other tenants as a `tenant` repository (§11) |

Replaced text (§11):

> **A template repository is a source of Template and Module pages that a tenant uses without copying them,** configured exactly as file repositories are ([0039](0039-files-and-media.md) §11): a `config` record of kind `template-repo`, keyed `template-repo:{name}`, in a tenant's or the instance's `config`, and listed in lookup order in the tenant's `wikitext.template_repos`.

### A6. TemplateStyles, TemplateData and page properties

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §3, §5–7
- **Change:** amends §5, §8.1; extends §4, §7, §9, §10
- **Summary:** `templatestyles` and `templatedata` move from `chip` to `implemented`; `__DISAMBIG__` from `ignored` to `implemented`; `SHORTDESC` and `#REDIRECT` are registered; the page properties expansion returns are stored in `view.page_prop`; `action=templatedata` is served to Parsoid; stylesheets are manifest entries and have a tracking category. Q2 and Q3 settled by 0055; Q4 and Q5 by [0053](0053-mirrored-pages.md) §5–6.

Replaced text (§5):

> | **Extension tags:** `templatestyles`, `templatedata`, `math`, `chem`, `ce`, `score`, `timeline`, `graph`, `mapframe`, `maplink`, `categorytree`, `inputbox`, `imagemap`, `hiero`, `charinsert` | `chip` |

> | **Behaviour switches:** `__NOEDITSECTION__`, `__NEWSECTIONLINK__`, `__NONEWSECTIONLINK__`, `__NOGALLERY__`, `__INDEX__`, `__NOINDEX__`, `__STATICREDIRECT__`, `__EXPECTUNUSEDCATEGORY__`, `__DISAMBIG__`, `__NOCONTENTCONVERT__`, `__NOTITLECONVERT__` | `ignored` |

Replaced text (§8.1):

> | TemplateData | `action=templatedata` | Not implemented. Only Parsoid's HTML-to-wikitext direction uses it, and reading never calls it |

### A7. An include may not widen

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §6
- **Change:** amends §3
- **Summary:** Transclusion is subject to the flow rule: a page includes another only if the included page's `read` groups are a subset of the including page's; a refused include is a red link for every viewer. The §10 refresh on a `read` ACL change already re-evaluates it.

Replaced text (§3):

> **Any `wikitext` page can be transcluded**, as in MediaWiki:
