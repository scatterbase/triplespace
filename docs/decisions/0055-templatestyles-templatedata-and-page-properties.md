# 0055. TemplateStyles, TemplateData and page properties

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-05 (A1)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0034](0034-frontend-stack.md), [0038](0038-page-metadata-and-categories.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0047](0047-special-pages.md)
- **Uses:** [0016](0016-permissions-and-access-control.md), [0023](0023-moderation.md), [0028](0028-tenancy-policy.md), [0033](0033-backend-stack.md), [0039](0039-files-and-media.md), [0043](0043-lua-modules.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

A fork of a Wikipedia article ([0054](0054-forking-a-mirrored-page.md)) brings its templates and modules. Expanded here, they render as Wikipedia's parser would render them, with three exceptions that [0042](0042-template-expansion-and-parsoid.md) left open and that an article shows at once:

| Gap | What breaks without it | Where 0042 left it |
|---|---|---|
| **TemplateStyles.** Since 2018 Wikipedia's infoboxes, navboxes, hatnotes, citation templates, sidebars, galleries and most tables carry their CSS in `Template:X/styles.css`, loaded by `<templatestyles src="X/styles.css" />` | Infoboxes render as unstyled tables; navboxes as lists; nothing lines up | Q2: "a `sanitized-css` model and a CSS sanitizer in Rust". [0008](0008-namespaces-and-document-pages.md) §5 excluded style models for the injection risk |
| **Site styles.** What TemplateStyles does not carry is in `MediaWiki:Common.css`: the `.infobox` base classes, `.hatnote`, `.navbox` colours, `.mw-parser-output` rules | The same, for whatever a template left to the site | Q3: "whether a tenant may have any site CSS" |
| **TemplateData.** `<templatedata>` on a template's documentation page describes its parameters as JSON; the template editor, VisualEditor and Parsoid's HTML-to-wikitext direction read it through `action=templatedata` | No parameter help in the editor; Parsoid's call is answered "not implemented" ([0042](0042-template-expansion-and-parsoid.md) §8.1) | Q6, in part |

Behind the third gap is a fourth that touches every page: **page properties**. MediaWiki's parser emits them (`displaytitle`, `defaultsort`, `templatedata`, `disambiguation`, `noindex`, the short description) into its `page_props` table, and `prop=pageprops`, `action=templatedata`, `Special:PagesWithProp` and the page title itself read them from there. [0042](0042-template-expansion-and-parsoid.md) §4 has the expander return "the page properties it set", and [0047](0047-special-pages.md) §10 deferred `PagesWithProp` because "none are stored".

The injection risk 0008 §5 named is real and bounded. Scripts are not bounded: `javascript` stays excluded. CSS can be sanitized, and TemplateStyles exists precisely because Wikimedia needed template authors to ship styles without `editsitecss`: its sanitizer parses the sheet, drops what is not on an allow-list, forbids external `url()`s, and scopes every selector under `.mw-parser-output` so a sheet can style content and nothing else. The same sanitizer, with the same rules, makes site styles safe enough to allow.

James's direction, from the design discussion of 2026-10-01, was that the ADR set should cover "the remaining needed functionality from Wikipedia: template data, template styles, and probably some other things". This is that ADR.

## Decision

### 1. The `sanitized-css` content model (amends 0008 §5; extends 0041 §3)

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `sanitized-css` | TemplateStyles | text | main | `text/css` | Yes | Template (10), for titles ending in `.css` |

It is a text model under [0041](0041-content-models.md) §5, stored in the page's own records and edited with `action=edit`. TemplateStyles' own ID is kept, under [0041](0041-content-models.md) §2's rule for a Wikibase or MediaWiki extension's model, so that an export of Wikipedia's `Template:Infobox/styles.css` imports here with its model intact.

**Where it is allowed.** Template (10), as TemplateStyles' default `$wgTemplateStylesNamespaces`, where a title ending in `.css` takes it by the suffix rule of [0008](0008-namespaces-and-document-pages.md) §5; and Project (4), for the site-styles page of §4. The title rule joins Module's in [0043](0043-lua-modules.md) §3: all of a namespace's models stay text models, so [0041](0041-content-models.md) §4's single-source rule holds. `action=changecontentmodel` moves a page to or from it within those namespaces.

**0008 §5's exclusion is narrowed.** It read "models whose code reaches the reader's browser are not supported: `css`, `javascript` and `sanitized-css`". It now excludes `css` and `javascript`, which reach the browser as written; `sanitized-css` reaches it only through §2, which is the difference TemplateStyles was built on.

**The model's operations** ([0041](0041-content-models.md) §5):

| Operation | For `sanitized-css` |
|---|---|
| Validate | The text parses under CSS Syntax Level 3, and the sanitizer (§2) accepts every rule. A sheet with a syntax error, a forbidden at-rule, a disallowed property or an external URL is **refused at save**, with each error's line, as TemplateStyles refuses one. Nothing that will be dropped at render is ever stored |
| Serialize | The text |
| Plain text for search | The text; the `pages` index marks it `content_model: sanitized-css` |
| Diff | Line diff |
| Render | The page view shows the sheet as highlighted source, with its sanitized form beneath when they differ (they do not, after validation) and the pages that use it ("Pages that use this stylesheet", from `view.transclusion`) |

### 2. The sanitizer: `scatter-css` (extends 0005 §3; uses 0033 §9.1)

**A new pure crate, `scatter-css`, parses and sanitizes CSS.** It builds for `wasm32-unknown-unknown` so the editor can lint a sheet as it is typed ([0034](0034-frontend-stack.md) §6). It is written from the CSS Syntax Level 3 specification and from TemplateStyles' documented rules and test cases, not translated from its PHP, so that it is original code under the shared crates' licence ([0005](0005-crate-organization.md) §6).

**What it accepts** is TemplateStyles' policy, which Wikipedia's sheets are written against:

| | Allowed | Dropped, or refused at save |
|---|---|---|
| **At-rules** | `@media`, `@supports`, `@font-feature-values`, `@keyframes` (with its rules sanitized), `@page` | `@import`, `@charset`, `@namespace`, `@font-face`, and any at-rule not listed |
| **Properties** | The allow-list TemplateStyles' css-sanitizer publishes: CSS 2.1 and the Level 3 and 4 modules it names (backgrounds, borders, box alignment, flexbox, grid, fonts, text, transforms, transitions, animations, multi-column, writing modes, and so on), kept in `docs/registry/css-properties.toml` with the module each came from | Unknown and vendor-specific properties, `behavior`, `-moz-binding`, `expression()`, `position: fixed` |
| **Values** | Standard syntax per property, with `!important` | Anything the property's grammar rejects |
| **`url()`** | A local file's URL, resolved through the tenant's file lookup ([0039](0039-files-and-media.md) §11) and written as the media base URL, so a sheet may use an uploaded or inherited image; `data:` URIs for images under a size cap | Every other URL. Nothing a sheet references can leave the instance |
| **Selectors** | Any selector, **prefixed with `.mw-parser-output`** so that it matches only rendered content and never the frame, as TemplateStyles prefixes them; a `wrapper` attribute on the tag (§3) prefixes further | Selectors that escape the prefix (`html`, `body`, `:root`), which are rewritten to match nothing |
| **Size** | Up to `templatestyles.max_bytes` (`site`, default 100 KB, TemplateStyles' `$wgTemplateStylesMaxStylesheetSize`) | Larger sheets are refused |

**Two modes.** `validate` reports every error and is what saving uses. `sanitize` drops what it cannot accept and reports what it dropped; it is what mirrored HTML ([0053](0053-mirrored-pages.md) §2) and the site-styles page (§4) use at render, so that a sheet written for another wiki's allow-list still renders its acceptable rules here.

**The output is canonical:** a serialized sheet with comments removed, so that two sheets that sanitize to the same text deduplicate (§3), and the hash of the output is stable. The sanitized form is cached by the page's ID and offset, L1 key `css:{pageid}:{gen}:{offset}` ([0014](0014-caches-and-search.md) §4), since every page that uses a template sheet renders the same sanitized text.

**Conformance.** TemplateStyles' css-sanitizer test suite runs in CI as fixtures (GPL-2.0-or-later test data, not compiled in, as the parser tests are, [0033](0033-backend-stack.md) §15), and the differential test of [0042](0042-template-expansion-and-parsoid.md) §18 compares the `<style>` elements the reference install emits for the same templates.

### 3. `<templatestyles>` (amends 0042 §5; extends 0042 §7, §9 and §10)

**The tag is `implemented`** in `docs/registry/wikitext-functions.toml`. In the expansion stage ([0042](0042-template-expansion-and-parsoid.md) §1) `<templatestyles src="Infobox/styles.css" />` resolves `src` as a title, `Template:` by default and any namespace with a prefix, **through the stack** ([0052](0052-page-repositories-and-title-inheritance.md) §3), so a fork that left its sheets to the repository still finds them, and a local sheet shadows a foreign one. The tag emits, in place, a `<style data-mw-deduplicate="TemplateStyles:r{hash}">` element holding the sheet's sanitized form, with `wrapper="…"` prefixing every selector further, as TemplateStyles does. **One `<style>` per distinct sheet per page:** the renderer keeps the first and drops repeats by their key, which is what MediaWiki and Parsoid do, so a page with two hundred citations loads the citation sheet once.

**Errors** are TemplateStyles': a `src` that does not resolve, or that resolves to a page whose model is not `sanitized-css`, renders an error span ([0042](0042-template-expansion-and-parsoid.md) §7) and adds the tracking category `templatestyles-stylesheet-error-category` ([0042](0042-template-expansion-and-parsoid.md) §9). A page of the right model never errors at render, since it was validated at save.

**The manifest records the sheet** ([0042](0042-template-expansion-and-parsoid.md) §10) as a transcluded page, local with its page ID and offset or foreign with its repository, revision and expiry, so an edit to a sheet refreshes every page that uses it, and `view.transclusion` lists them. `prop=templates` and "Templates used" ([0042](0042-template-expansion-and-parsoid.md) §15) include sheets, as MediaWiki's do.

**Both renderers.** The built-in renderer renders the element as above. Parsoid asks the render-scoped `action=parse` for tags it does not implement natively ([0042](0042-template-expansion-and-parsoid.md) §8.1), and gets the same element with `modules` empty. **Mirrored HTML** carries Wikipedia's `<style>` elements already; [0053](0053-mirrored-pages.md) §2 passes them through `sanitize` and keeps their keys, so a mirrored page and a fork of it deduplicate the same way.

### 4. Site styles: one designated page (settles 0042 Q3)

**A tenant may designate one `sanitized-css` page as its site styles,** with the `site` setting **`wikitext.site_styles`** naming a title in the Project namespace, `Project:Site styles.css` by convention. The MediaWiki namespace (8) stays reserved ([0008](0008-namespaces-and-document-pages.md) §2); the page is an ordinary Project page with the model of §1, protected as the tenant sees fit with an ACL ([0023](0023-moderation.md) §1), and its history is a page history.

**It is applied to every rendered page** of the tenant, local and mirrored, as a `<link rel="stylesheet">` in the frame to `{base}/w/styles/site.css?v={offset}`, served from the sanitized form in the `css:` cache with a long lifetime, versioned by the page's latest offset. Nothing about a page's render depends on it, so editing the site styles bumps no render epoch and refreshes no page; the next request simply links the new version. The sanitizer's scoping is what makes this allowable where 0008 §5 refused `css`: the sheet reaches only `.mw-parser-output`, and the frame, the editor and every special page are out of its reach by construction.

**What it is for.** The classes Wikipedia's templates leave to `MediaWiki:Common.css`, `.infobox`, `.hatnote`, `.navbox`, `.mw-collapsible`, `.reflist`, which a tenant that mirrors or forks Wikipedia copies by hand from upstream's sheet, under its licence, into this page; and a tenant's own look for its content. A sheet copied from a MediaWiki `Common.css` loses, at save, whatever the sanitizer refuses, and the save reports each line, which is the right moment to learn it.

**What it is not.** It is not a skin: the frame's own styles are the instance's ([0034](0034-frontend-stack.md)), and no tenant CSS reaches them. It is not per-user: user styles (`User:X/common.css`) remain excluded with `css`. It is one page, not a cascade of `MediaWiki:*.css` pages per skin and per gadget.

### 5. `<templatedata>` and `action=templatedata` (amends 0042 §5 and §8.1; extends 0034 §7)

**The tag is `implemented`.** `<templatedata>` holds a JSON object in TemplateData's schema: `description`, `params` with each parameter's `label`, `description`, `type`, `required`, `suggested`, `default`, `example`, `aliases`, `deprecated` and `autovalue`, `paramOrder`, `format`, `sets` and `maps`, with localized strings as language maps. **It is validated at save** against the schema shipped as data from the extension's `TemplateDataValidator`: a page whose `<templatedata>` is invalid is refused with the extension's own error messages (`templatedata-invalid-param`, `templatedata-invalid-type` and the rest), as TemplateData refuses it. A page may carry one `<templatedata>` block; a second is an error. With expansion off the tag is still implemented, since it needs no other page.

**It renders as the parameter table** TemplateData renders: description, then one row per parameter with label, description, type, status and default, in `paramOrder`, localized to the interface language where the JSON carries that language. On a `/doc` subpage that is what readers see; on the template itself, the same.

**It is stored as the page property `templatedata`** (§6), extracted when the page is saved, synchronously, so that `action=templatedata` answers for a template as soon as its documentation is saved. Because `/doc` pages are transcluded by their templates under `<noinclude>`, the property is **also written to the transcluding template** by the refresh job, as the extension's parser hook does, so `action=templatedata&titles=Template:Infobox` finds the data that lives on `Template:Infobox/doc`.

**`action=templatedata`** is served from `view.page_prop`, with MediaWiki's parameters: `titles` and page-set generators, `includeMissingTitles`, `doNotIgnoreMissingTitles`, `lang` (resolving the language maps), `redirects`, and `format`. For a title whose primary is foreign ([0052](0052-page-repositories-and-title-inheritance.md) §3) it serves the repository's data, from the bundle's `pageprops` ([0053](0053-mirrored-pages.md) §1) or, where the bundle lacks it, from the repository's own `action=templatedata`, cached; so the editor's parameter help works for inherited templates. **Parsoid's call** ([0042](0042-template-expansion-and-parsoid.md) §8.1, "not implemented") is now served, which is the prerequisite its HTML-to-wikitext direction needs ([0042](0042-template-expansion-and-parsoid.md) Q6).

**The editor uses it.** The source editor ([0034](0034-frontend-stack.md) §7) gains **Insert template…**: a search over Template titles through the stack, then a form built from the template's TemplateData, writing the call in the data's `format`; and, with the cursor inside a template call, a parameter popup listing the template's parameters with their descriptions. Without TemplateData a template inserts as `{{Name}}` and the popup says so. This is what TemplateWizard and VisualEditor's template dialog do with the same data.

### 6. Page properties: `view.page_prop` (extends 0013 §5.6; extends 0042 §4 and §10; amends 0047 §10)

*Changed by A1.*

**Page properties are stored,** in the table MediaWiki has and 0047 §10 was waiting for:

```sql
CREATE TABLE view.page_prop (                   -- MediaWiki's page_props (§6)
  page_id bigint NOT NULL REFERENCES view.page,
  name text NOT NULL,
  value bytea NOT NULL,                           -- text, or compact JSON for templatedata; compressed over 1 KB
  sortkey double precision,                       -- for numeric properties, as MediaWiki's pp_sortkey
  PRIMARY KEY (page_id, name)
);
CREATE INDEX page_prop_name ON view.page_prop (name, sortkey, page_id);
```

**Who writes which property:**

| Property | Set by | Written by |
|---|---|---|
| `templatedata` | `<templatedata>` (§5) | The page projection, in step 2, synchronously; copied to the transcluding template by the refresh job |
| `defaultsort` | `{{DEFAULTSORT:}}` ([0038](0038-page-metadata-and-categories.md) §3) | The page projection, in step 2, as the category sort key already is |
| `workspace.{role}` | `{{#workspace:}}` on a project page ([0062](0062-workspaces.md) §2) | The refresh job, from the expander's page properties; read by blocks on the page and its subpages |
| `displaytitle` | `{{DISPLAYTITLE:}}` | The refresh job ([0042](0042-template-expansion-and-parsoid.md) §10), from the expander's page properties, under MediaWiki's `$wgRestrictDisplayTitle` rule: the value must normalize to the page's title, else it is ignored |
| `wikibase-shortdesc` | `{{SHORTDESC:…}}`, the Wikibase Client magic word Wikipedia's `{{Short description}}` template wraps, now `implemented` in the registry under origin Wikibase Client | The refresh job |
| `disambiguation` | `__DISAMBIG__`, whose status changes from `ignored` to `implemented` | The refresh job |
| `noindex`, `index`, `notoc`, `forcetoc`, `noeditsection`, `newsectionlink`, `nonewsectionlink`, `nogallery`, `staticredirect`, `expectunusedcategory`, `hiddencat` | The behaviour switches, as MediaWiki stores each as a property | The refresh job (`hiddencat` also in step 2, as 0038 §3 reads it) |
| `wikibase_item` | The page's paired item ([0038](0038-page-metadata-and-categories.md) §6), as Wikibase Client sets it, so tools that read `prop=pageprops&ppprop=wikibase_item` find it | The sitelink projection |

Properties the refresh job writes are written in the same transaction as the links, categories and transclusions of a render ([0042](0042-template-expansion-and-parsoid.md) §10), and are therefore among the `view` tables that render re-creates rather than replays. A property the latest render did not set is deleted.

**What reads them.**

- **The frame** shows `displaytitle` as the page's heading, where set, and marks a `disambiguation` page as one; `noindex` sets the response's robots meta.
- **`prop=pageprops`** with `ppprop`; **`list=pageswithprop`** and **`generator=pageswithprop`** with `pwppropname`; **`prop=info&inprop=displaytitle`**; **`prop=description`**, which returns `wikibase-shortdesc` where set and the paired item's description otherwise, as Wikipedia's does; **`action=templatedata`** (§5).
- **`Special:PagesWithProp`** leaves the deferred table of [0047](0047-special-pages.md) §10 and is served as a report over `view.page_prop`, with the extension's form: property name, then the pages carrying it.
- **Search** ([0014](0014-caches-and-search.md) §7): the `pages` index gains `short_description`, shown in the suggester beneath the title, as Wikipedia's suggester shows it.
- **RDF** ([0038](0038-page-metadata-and-categories.md) §11): the page node gains `schema:description` from `wikibase-shortdesc`, in the content language, beside its `schema:name`. This does not settle [0038](0038-page-metadata-and-categories.md) Q4, page terms; it gives the short description MediaWiki's shape, which is a page property set in the text, and leaves whether pages get terms of their own open.

**Foreign pages** have no rows here; their properties are in the bundle's metadata ([0053](0053-mirrored-pages.md) §1), and `prop=pageprops` on an inherited title serves them from there.

### 7. The registry (amends 0042 §5)

`docs/registry/wikitext-functions.toml` changes:

| Entry | Was | Now |
|---|---|---|
| `templatestyles` (tag, TemplateStyles) | `chip` | `implemented` (§3) |
| `templatedata` (tag, TemplateData) | `chip` | `implemented` (§5) |
| `__DISAMBIG__` (switch, Disambiguator) | `ignored` | `implemented` (§6) |
| `SHORTDESC` (parser function, Wikibase Client) | — | `implemented` (§6) |
| `__EXPECTED_UNCONNECTED_PAGE__` (switch, Wikibase Client) | — | `ignored` |

`docs/registry/css-properties.toml` is new (§2). `content-models.toml` gains `sanitized-css` (§1). `special-pages.toml` marks `PagesWithProp` served (§6).

### 8. Permissions

| Action | Needs |
|---|---|
| Editing a `sanitized-css` page in Template | `edit`, as any template; the sanitizer is the safeguard, which is TemplateStyles' design |
| Editing the site-styles page | `edit` on the page, under whatever ACL the tenant puts on it; the default tenancy template protects it to `sysop` ([0028](0028-tenancy-policy.md) §8) |
| Setting `wikitext.site_styles` or `templatestyles.max_bytes` | `ts-config` |
| Changing a page's model to or from `sanitized-css` | `editcontentmodel` |

### 9. Crates (extends 0005 §2 and §3)

| Crate | Change |
|---|---|
| `scatter-css` *(new, pure, wasm)* | CSS Syntax Level 3 parsing, the sanitizer's `validate` and `sanitize`, selector scoping, canonical serialization, and the embedded `css-properties.toml` (§2). It joins the pure crates of [0005](0005-crate-organization.md) §3 rule 2 and the wasm list of [0034](0034-frontend-stack.md) §6 |
| `scatter-pages` | The `sanitized-css` model (§1), depending on `scatter-css` |
| `scatter-wikitext` | `<templatestyles>` and `<templatedata>` rendering; TemplateData's schema validation and parameter table (§3, §5) |
| `scatter-wikitext-expand` | `SHORTDESC`, `__DISAMBIG__` as a property, the sheet lookup through the host (§3, §6) |
| `triplespace-render` | Sheet manifest entries, deduplication, the site-styles link and route (§3–4) |
| `triplespace-projections` | `view.page_prop` and its writers (§6) |
| `scatter-wasm` | Re-exports `scatter-css` for the editor's linting |
| API crates | `action=templatedata`, `prop=pageprops`, `list=pageswithprop`, `prop=description`, `inprop=displaytitle` (§5–6) |

## Alternatives considered

- **Keeping every style model excluded,** and relying on mirrored HTML for Wikipedia's look. Rejected: a fork renders here, from wikitext, and without its sheets it is an unstyled page; and the direction asks for template styles by name.
- **Allowing `css` as MediaWiki does for `MediaWiki:Common.css`,** unsanitized, to administrators. Rejected: 0008 §5's reason stands for unsanitized CSS, which can load external resources, restyle the frame and exfiltrate through `url()`. One sanitized page gives what a wiki needs without that.
- **Using `lightningcss` or `cssparser` as the sanitizer.** They parse; neither implements an allow-list policy, scoping or TemplateStyles' rules, and the policy is the point. `scatter-css` may use `cssparser` for tokenization (MPL-2.0, compatible under [0033](0033-backend-stack.md) §16) and adds the rest.
- **Storing TemplateData as page statements** rather than a property. Rejected: `action=templatedata` is a MediaWiki contract keyed by page property, every client reads it that way, and the data describes the template's text, not a fact about the page.
- **A `view.page_prop` written only by the refresh job.** Rejected for `templatedata` and `defaultsort`: an editor who saves documentation should get `action=templatedata` on reload, and both need no other page.

## Consequences

- **A forked Wikipedia article looks like one.** Its infoboxes, navboxes and citations carry their styles with them, through sheets that are validated on save and deduplicated on render.
- **A tenant can supply the rest of Wikipedia's look** in one page, safely scoped, and can protect that page as it protects anything.
- **The editor can help with templates.** Parameter forms and popups come from TemplateData, for local and inherited templates alike, and Parsoid's editing direction has the call it needs.
- **Page properties exist,** so `prop=pageprops`, `pageswithprop`, `prop=description`, display titles and short descriptions work as on MediaWiki, and `PagesWithProp` is served.
- **The style models split three ways:** `css` and `javascript` excluded as before; `sanitized-css` allowed through a sanitizer; no user styles.
- **One more pure crate, one more registry file,** and a CSS property allow-list to keep current with TemplateStyles'.
- **Mirrored pages and forks of them style identically,** since both pass through the same sanitizer with the same keys.

## Open questions

- **Q1. User styles and gadgets.** Whether `User:X/common.css` should ever be allowed through the same sanitizer, which would be the first per-user page that changes rendering, and what of gadgets, which are scripts and stay out.
- **Q2. Per-skin styles.** MediaWiki keeps `Vector.css`, `Minerva.css` and so on; whether the site-styles page should take `@media` or a skin selector to serve different layouts, or whether one sheet is enough.
- **Q3. The property allow-list's drift.** TemplateStyles adds properties as css-sanitizer grows; whether `css-properties.toml` should be generated from its published list on a schedule, as `sites.toml` is from the site matrix.
- **Q4. PageImages.** Wikipedia's `page_image_free` property drives link previews and search thumbnails; whether the refresh job should choose one as PageImages does.
- **Q5. TemplateData for Lua-driven templates.** `#invoke` templates often carry TemplateData on the template, not the module; whether the module's documentation should be read as a fallback.
- **Q6. Short descriptions as page terms.** Whether `wikibase-shortdesc` should become the page's description under [0038](0038-page-metadata-and-categories.md) Q4, with a page statement or term as its native form.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2, §3 | §9 | extends | 0005 A54 |
| [0008](0008-namespaces-and-document-pages.md) §5 | §1 | amends | 0008 A24 |
| [0010](0010-site-ui.md) §4 | §5, §6 | extends | 0010 A31 |
| [0012](0012-api-requirements.md) §4 | §5, §6 | extends | 0012 A34 |
| [0013](0013-postgres-storage.md) §5.6 | §6 | extends | 0013 A25 |
| [0014](0014-caches-and-search.md) §4, §7 | §2, §6 | extends | 0014 A12 |
| [0015](0015-record-format-and-partition-registry.md) §3, §5 | §2, §4, §7 | extends | 0015 A26 |
| [0034](0034-frontend-stack.md) §6, §7 | §2, §5 | extends | 0034 A5 |
| [0038](0038-page-metadata-and-categories.md) §11 | §6 | extends | 0038 A8 |
| [0041](0041-content-models.md) §3 | §1 | extends | 0041 A7 |
| [0042](0042-template-expansion-and-parsoid.md) §5, §8.1 | §3, §5, §7 | amends | 0042 A6 |
| [0042](0042-template-expansion-and-parsoid.md) §4, §7, §9, §10 | §3, §6 | extends | 0042 A6 |
| [0042](0042-template-expansion-and-parsoid.md) Q2, Q3 | §1–4 | settles | 0042 Q2, Q3 |
| [0047](0047-special-pages.md) §10 | §6 | amends | 0047 A5 |
| [0047](0047-special-pages.md) §4.4 | §6 | extends | 0047 A5 |
| [0047](0047-special-pages.md) Q2 | §6 | settles | 0047 Q2 |

## References

- [Extension:TemplateStyles](https://www.mediawiki.org/wiki/Extension:TemplateStyles) and [Help:TemplateStyles](https://www.mediawiki.org/wiki/Help:TemplateStyles); [wikimedia/css-sanitizer](https://www.mediawiki.org/wiki/Css-sanitizer), whose rules and tests §2 follows
- [CSS Syntax Module Level 3](https://www.w3.org/TR/css-syntax-3/)
- [Extension:TemplateData](https://www.mediawiki.org/wiki/Extension:TemplateData), [Help:TemplateData](https://www.mediawiki.org/wiki/Help:TemplateData) and [API:Templatedata](https://www.mediawiki.org/wiki/API:Templatedata); [Extension:TemplateWizard](https://www.mediawiki.org/wiki/Extension:TemplateWizard)
- [Manual:Page_props table](https://www.mediawiki.org/wiki/Manual:Page_props_table), [API:Pageprops](https://www.mediawiki.org/wiki/API:Pageprops) and [API:Pageswithprop](https://www.mediawiki.org/wiki/API:Pageswithprop)
- [Help:Magic words](https://www.mediawiki.org/wiki/Help:Magic_words) (`DISPLAYTITLE`, `__DISAMBIG__`) and [Wikibase Client's `SHORTDESC`](https://www.mediawiki.org/wiki/Extension:Wikibase_Client), with [Wikipedia:Short description](https://en.wikipedia.org/wiki/Wikipedia:Short_description)
- [Wikipedia's `MediaWiki:Common.css`](https://en.wikipedia.org/wiki/MediaWiki:Common.css), for what §4 is for

## Amendment log

### A1. Workspace roles as page properties

- **Date:** 2026-10-05
- **Source:** [0062](0062-workspaces.md) §2
- **Change:** extends §6
- **Summary:** `workspace.{role}` page properties, set by `{{#workspace:}}` and stored by the refresh job.
