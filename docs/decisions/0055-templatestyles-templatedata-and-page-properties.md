# 0055. TemplateStyles, TemplateData and page properties

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-09 (A2)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0034](0034-frontend-stack.md), [0038](0038-page-metadata-and-categories.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0047](0047-special-pages.md)
- **Uses:** [0016](0016-permissions-and-access-control.md), [0023](0023-moderation.md), [0028](0028-tenancy-policy.md), [0033](0033-backend-stack.md), [0039](0039-files-and-media.md), [0043](0043-lua-modules.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [09](../architecture/09-security-and-moderation.md), [10](../architecture/10-pages-and-content-models.md), [11](../architecture/11-rendering-templates-and-modules.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md)

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

*Current text: [10](../architecture/10-pages-and-content-models.md) §1.4; [11](../architecture/11-rendering-templates-and-modules.md) §6.1.*

### 2. The sanitizer: `scatter-css` (extends 0005 §3; uses 0033 §9.1)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §6.2, §8.*

### 3. `<templatestyles>` (amends 0042 §5; extends 0042 §7, §9 and §10)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §2.2, §4.2, §6.3.*

### 4. Site styles: one designated page (settles 0042 Q3)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §6.4.*

### 5. `<templatedata>` and `action=templatedata` (amends 0042 §5 and §8.1; extends 0034 §7)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §2.2, §3.1, §6.5.*

### 6. Page properties: `view.page_prop` (extends 0013 §5.6; extends 0042 §4 and §10; amends 0047 §10)

*Changed by A1.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §5.5.*

### 7. The registry (amends 0042 §5)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §2.2; [23](../architecture/23-configuration-and-registry.md) §4.5.*

### 8. Permissions

*Current text: [09](../architecture/09-security-and-moderation.md) §8.9.*

### 9. Crates (extends 0005 §2 and §3)

*Current text: [22](../architecture/22-crates-and-stack.md) §1.2, §2.1, §2.2.*

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

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [09](../architecture/09-security-and-moderation.md), [10](../architecture/10-pages-and-content-models.md), [11](../architecture/11-rendering-templates-and-modules.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
