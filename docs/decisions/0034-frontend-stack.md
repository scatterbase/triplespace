# 0034. Frontend technology stack

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-03 (A7)
- **Author:** James Hare / Claude
- **Changes:** [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0003](0003-statement-ui.md), [0012](0012-api-requirements.md), [0014](0014-caches-and-search.md), [0017](0017-entity-id-grammar.md), [0029](0029-resolver-namespaces.md), [0033](0033-backend-stack.md)

## Context

0003 and 0010 describe what the statement UI and the site UI show. 0010 left open whether pages are rendered on the server or the client, and how mobile layouts work. 0012 §1 requires the UI to use only the public API and to discover capabilities. 0014 caches pages at an HTTP layer with `ETag` and `Cache-Tag`.

James decided on 2026-09-27 that **the designs adopt Codex, Wikimedia's design system, as-is**. Codex ships Vue 3 components and CSS-only components that look identical to them; most Vue components have a CSS-only version, and the accessible interactive ones (Lookup, Combobox, Menu, Dialog, TypeaheadSearch) need Vue. It targets WCAG 2.1 AA with wide i18n and RTL support, and has been bundled with MediaWiki since 1.39.

The backend stack is in 0033.

## Decision

### 1. Principles

*Changed by A6, A7.*

1. **The server renders every page.** Reading never needs JavaScript. Rendered pages are cacheable at the L2 layer (0014 §3).
2. **JavaScript enhances regions, never whole pages.** An interactive component takes over one region of a server-rendered page. There is no client-side router.
3. **One renderer per thing.** Anything the server draws is drawn only by the server. A component that changes it fetches the server's rendering afterwards (§5) rather than drawing its own copy.
4. **Codex components, themed by tokens.** Codex's components, icons and markup are used without overrides. Their look comes from Codex's design tokens, and the instance or a tenant may give those tokens values of its own as a **theme** (`ui.theme`, [0015](0015-record-format-and-partition-registry.md) §3): colours, typefaces, radii and spacing, never a component's markup or behaviour. A theme is served as a stylesheet of CSS custom properties, so it needs no inline style under §11, and it is refused when its colours fail WCAG 2.1 AA contrast for the token pairs Codex uses for text, borders and focus. Without a theme, Codex's own values apply. Where a design needs something Codex lacks, it is built from Codex tokens and proposed upstream. Codex is used for its accessible components, its right-to-left and language support, and CSS-only components that need no JavaScript, more than for its look.
5. **The public API only** (0012 §1). The browser calls public routes, and so does the server-side renderer: it calls the public HTTP API with the viewer's own credentials, over the network from `triplespace-web` or through an in-process call into the API's router from `triplespace-server`, and never links an API crate's handlers or reads `view` ([0057](0057-web-tier.md) §1). Every page therefore carries exactly the API's per-viewer redaction.

### 2. Server rendering, in `triplespace-ui`

*Changed by A6.*

- **Templates:** `askama`, compiled and type-checked with the Rust code.
- **Codex markup:** a Rust builder module that emits Codex CSS-only component markup (buttons, fields, tables, cards, tabs, messages, chips, progress bars), following the approach of WMF's Codex PHP. Templates call the builder instead of writing Codex class names by hand, so a Codex markup change is one edit.
- **Frame:** the page frame of 0010 §2 (global header, identity line, title, tabs) is one template shared by every page kind.
- **Assets:** Codex CSS, design tokens and icons come from the pinned `@wikimedia/codex`, `@wikimedia/codex-design-tokens` and `@wikimedia/codex-icons` packages at build time (§8), with hashed file names, served with long-lived cache headers by whichever binary serves the site: `triplespace-web`, or `triplespace-server` with `server.ui = embedded` ([0057](0057-web-tier.md) §2).
- **Mobile:** one responsive site built on Codex's breakpoints. No separate mobile domain or skin. Detailed mobile layouts stay open (0010).

### 3. Statement groups (uses 0003 §3)

- The layouts of 0003 (table, matrix, chart, timeline, chips, "Same for all N", footnoted shared references) are rendered **only on the server**.
- Tables use Codex's CSS-only Table.
- Charts, sparklines and timeline strips are **SVG generated in Rust** from the same view model the classifier produces. They cache like any other markup and need no JavaScript. No client charting library is used for these.
- The shape classifier lives in a pure, wasm-capable crate, so an editor can predict the layout a change will produce (§6).

### 4. Interactive components

*Changed by A3.*

Vue 3 with Codex's Vue components, written in TypeScript, mounted into placeholders in the server-rendered HTML. Each component reads its initial data from a `<script type="application/json">` block the server writes beside the placeholder.

| Component | Codex parts | Region |
|---|---|---|
| Statement value editor, rank menu | Lookup, Combobox, Menu, Dialog, TextInput | one value or one statement |
| Term editor (labels, descriptions, aliases) | TextInput, ChipInput | the term box |
| Source editor with live preview (0008, 0010) | CodeMirror 6 (§7), Tabs | the edit view |
| Search box with "Go to" suggestions (0010, 0029) | TypeaheadSearch | the global header |
| Notifications bell and inbox (0021) | Popover, Menu | the account area |
| Live updates switch (0020) | ToggleSwitch | feed pages |
| Recent changes filters | Checkbox, Lookup, MultiselectLookup | the filter panel |
| Job progress (0012) | ProgressBar | `Special:Jobs/{id}` |
| User, group and consumer pickers on special pages | Lookup | the form field |

Statement editing requires JavaScript, as on Wikidata. Reading, page source editing through a plain form, and the account pages work without it. A **table grid editor** (Codex Table, Lookup, TextInput; the statement value editor for cells) sits over the server-rendered grid of a `Table` page; reading a table needs no JavaScript ([0045](0045-table-content-model.md) §11).

### 5. Fragments

*Changed by A6.*

After a component saves, it fetches the server's rendering of the changed region from the page's own URL and swaps it in: MediaWiki's `action=render`, with a `region` parameter that names part of the content, such as `index.php?title=Item:Q42&action=render&region=statements/P1082` or `region=terms` ([0057](0057-web-tier.md) §8). The web tier serves it like the page, so there are no fragment routes in the API.

A region carries the same `ETag`, `Cache-Tag` and redaction as its page ([0014](0014-caches-and-search.md) §3, 0057 §6). Full pages are assembled from the same region functions, so a region and its page cannot drift. A component sends the build ID of its page, and a web tier on another build answers `409` so that the component reloads the page instead (0057 §8).

### 6. Rust in the browser, in `scatter-wasm`

*Changed by A1, A5.*

One `wasm-bindgen` crate re-exports what editors need from the pure crates:

- the 0003 shape classifier;
- `scatter-normalize` (value normalization and keyed-ID grammars, 0017, 0029);
- `scatter-wikitext` and the `scatter-pages` markdown renderer, for live preview;
- `scatter-css`, so the editor lints a `sanitized-css` page as it is typed ([0055](0055-templatestyles-templatedata-and-page-properties.md) §2).

It is built with `wasm-bindgen-cli` and `wasm-opt`, lazy-loaded only by editors, and never needed for reading. This is one reason 0033 §9.1 chose a pure-Rust wikitext parser. When the source needs expansion the browser cannot do, the preview comes from `action=parse` on the server instead ([0042](0042-template-expansion-and-parsoid.md) §15).

### 7. Source editor

*Changed by A2, A5.*

CodeMirror 6, which MediaWiki's CodeMirror extension also uses, with its wikitext mode, a markdown mode and a Lua mode for module pages ([0043](0043-lua-modules.md) §13). Edit conflicts (0010 §5) are shown as line decorations. `[[` triggers link autocomplete through the suggest route (0012). `tree-sitter-wikitext` through `web-tree-sitter` is an optional later enhancement for structural highlighting. A CSS mode serves `sanitized-css` pages. **Insert template…** builds a form from a template's TemplateData, and a parameter popup lists a call's parameters ([0055](0055-templatestyles-templatedata-and-page-properties.md) §5).

### 8. Build

*Changed by A6.*

- `ui/` is an npm workspace: TypeScript, Vue 3, Codex packages pinned to exact versions, CodeMirror 6.
- **Vite** builds the components into hashed ES modules and a manifest.
- The release build embeds `ui/dist` into both binaries that can serve the site, `triplespace-web` and `triplespace-server` (`rust-embed`); templates read the manifest to emit `<script type="module">` tags. **Node is a build-time dependency only**; a small instance stays one binary (0033 §1), and an instance that runs the web tier separately runs two ([0057](0057-web-tier.md) §2).
- In development, whichever binary serves the site proxies asset requests to the Vite dev server for hot reload.
- Lint: ESLint with `eslint-config-wikimedia`, Stylelint with `stylelint-config-wikimedia`. Styles are plain CSS using Codex's CSS custom-property tokens; no Less.

### 9. Internationalisation

- Interface messages are **banana JSON** files in `i18n/` (`en.json`, `qqq.json` for documentation, one file per language), so the project can be registered with translatewiki.net like any MediaWiki extension.
- The server renders messages with the `banana-i18n` Rust crate; components use the `banana-i18n` JavaScript library. Both read the same files. The Rust crate is at 0.1.0; if it proves incomplete, a small in-house implementation of `PLURAL`, `GENDER`, `GRAMMAR` and `$n` with ICU4X plural rules replaces it.
- `banana-checker` runs in CI.
- Interface language comes from the `language` preference (0027) with `uselang` as an override; fallback chains follow MediaWiki's. Direction comes from the language and is set on `<html dir>`; Codex handles RTL.

### 10. Accessibility and browser support

*Changed by A7.*

- WCAG 2.1 AA, inherited from Codex and checked by axe in end-to-end tests (§12), and kept under a theme by the contrast check that admits it (§1).
- Browser support follows MediaWiki's: modern browsers get the components; older browsers get the server-rendered pages without them.

### 11. Security

*Changed by A6.*

- A strict Content Security Policy: scripts only from the instance's own origin, no inline scripts, no `eval`. Initial data is passed in `application/json` blocks, which CSP does not execute.
- Components never insert HTML they built from user input. HTML they insert comes only from `action=render` (§5), which is built from `askama`'s escaped templates and from HTML the API has sanitized (0033 §9).

### 12. Testing

| Tool | Use |
|---|---|
| Vitest + `@vue/test-utils` | component tests |
| Playwright | end-to-end flows, including a JavaScript-disabled run of every read page |
| `@axe-core/playwright` | accessibility checks on every page kind |
| `insta` | snapshots of rendered templates and fragments |

### 13. Prototyping path

*Changed by A6.*

As the statement UI spec planned, the UI is built before the backend is complete:

1. `triplespace-ui` renders item pages from Wikidata's canonical JSON (`Special:EntityData`) through the shape classifier.
2. Provenance panels use fixtures.
3. The editing components write through the Action API to test.wikidata.org or the MediaWiki 1.43 reference install.

Nothing in the UI changes when the source becomes a Triplespace instance, because it only speaks the public API. In development, `triplespace-web` with `web.upstream_host = fixed` is pointed at Wikidata or the reference install directly ([0057](0057-web-tier.md) §4).

## Alternatives considered

- **Leptos (full-stack Rust, server rendering with islands).** One language and one component tree, but Codex's accessible interactive components would have to be rebuilt in Rust, and Leptos is still pre-1.0. It would also narrow the contributor pool to Rust developers.
- **Vue single-page app with Nuxt server rendering.** Needs a Node runtime in production, against 0033 §1.
- **htmx-style fragments only.** Covers updates but not accessible comboboxes, lookups and dialogs, which Codex already provides.
- **OOUI.** Superseded by Codex in MediaWiki.

## Consequences

- Wikimedia developers who know Vue and Codex can work on the editors without learning Rust; Rust developers own the templates and renderers.
- Pages read and cache without JavaScript, and the statement layouts exist in one implementation.
- The build needs Node, though the running system does not.
- Page rendering can run on machines of its own, as `triplespace-web`, and scale apart from the API ([0057](0057-web-tier.md)).
- Codex upgrades are deliberate: versions are pinned and bumped in their own commits, with snapshot and accessibility tests as the check.

## Open questions

- **Q1.** Codex upgrade cadence, and whether to track the version bundled with a MediaWiki release.
- **Q2.** Night mode: adopt Codex's dark-mode tokens when they are stable.
- **Q3.** Whether the Rust Codex builder becomes a published crate other projects can use.
- **Q4.** An interactive charting library for exploring a property's full view (0003), beyond the static SVGs.
- **Q5.** Visual regression testing.
- **Q6.** The maturity of the `banana-i18n` Rust crate (§9).

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2, §3 | §2, §6 | extends | 0005 A33 |
| [0005](0005-crate-organization.md) Consequences | §2, §6 | amends | 0005 A33 |
| [0010](0010-site-ui.md) §4, §13 | §1–6 | amends | 0010 A21 |
| [0010](0010-site-ui.md) Q6 | §1–5 | settles | 0010 Q6 |
| [0010](0010-site-ui.md) Q7 | §2 | settles | 0010 Q7 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §1 | extends | 0015 A28 |

## Amendment log

### A1. Expansion preview on the server

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §15
- **Change:** amends §6
- **Summary:** When the source needs expansion the browser cannot do, the preview comes from `action=parse` on the server instead.

Replaced text (§6):

> It is built with `wasm-bindgen-cli` and `wasm-opt`, lazy-loaded only by editors, and never needed for reading. This is one reason 0033 §9.1 chose a pure-Rust wikitext parser.

### A2. A Lua mode

- **Date:** 2026-09-30
- **Source:** [0043](0043-lua-modules.md) §13
- **Change:** extends §7
- **Summary:** A Lua mode for module pages.

### A3. The table grid editor

- **Date:** 2026-09-30
- **Source:** [0045](0045-table-content-model.md) §11
- **Change:** extends §4
- **Summary:** A table grid editor (Codex Table, Lookup, TextInput; the statement value editor for cells) over the server-rendered grid of a `Table` page. Reading a table needs no JavaScript.

### A4. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–13
- **Summary:** A1–A3 were folded into the Decision. The title's em dash became a full stop, the open questions were numbered, the Author line was added, and the list this ADR kept under "Changes to other ADRs" (quoted below) was replaced by the generated table. That list also claimed changes to 0003 §3, 0012 §5 and 0014 §5 (renderers, the fragment routes, fragment caching); none was ever written into those ADRs, and nothing records them there, so the fragment routes and their caching are stated in §5 of this ADR alone. No decision changed. Before this, A1–A3 were blockquotes. The file before conversion is commit `0b26a3a`.

Replaced text (Changes to other ADRs):

> - **0010:** the server-or-client rendering question is settled by §1–§5; mobile is settled as one responsive site (§2), with detailed layouts still open.
> - **0003:** renderers and chart drawing per §3.
> - **0012:** fragment routes of §5 added to the REST route groups.
> - **0014:** fragments cached and purged like pages (§5).
> - **0005:** new crates `triplespace-ui` (surfaces layer) and `scatter-wasm` (wasm bindings over pure crates); `ui/` workspace noted; changelog row.

### A5. CSS linting and TemplateData in the editor

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §2, §5
- **Change:** extends §6, §7
- **Summary:** `scatter-wasm` re-exports `scatter-css` for linting stylesheets; the source editor gains a CSS mode, Insert template… and a TemplateData parameter popup.

### A6. The web tier

- **Date:** 2026-10-03
- **Source:** [0057](0057-web-tier.md) §1, §2, §4, §8
- **Change:** amends §1, §2, §5, §8, §11; extends §13
- **Summary:** The server-side renderer is an API client like the browser: it calls the public HTTP API with the viewer's credentials, from `triplespace-web` over the network or from `triplespace-server` in-process, and never the API crates' handlers. Either binary serves the site and its assets. Fragment routes are gone: a component fetches its region with `action=render` and `region` on the page's own URL, which also ends the collision of `GET /page/{id}/render` with 0042 §12's render manifest. In development the web tier can be pointed at Wikidata directly. A consequence is added.

Replaced text (§1):

> 5. **The public API only** (0012 §1). The browser calls public routes. The server-side renderer calls the API crates' handler layer in-process, never `view` tables directly, so every page gets the same per-viewer redaction as the API.

Replaced text (§2):

> - **Assets:** Codex CSS, design tokens and icons come from the pinned `@wikimedia/codex`, `@wikimedia/codex-design-tokens` and `@wikimedia/codex-icons` packages at build time (§8), with hashed file names, served by the binary with long-lived cache headers.

Replaced text (§5):

> ### 5. Fragment routes
>
> After a component saves, it fetches the server's rendering of the changed region and swaps it in. The fragments are public API routes under `rest.php/triplespace/v0` (0012 §2), in the spirit of Wikibase's `wbformatvalue`:
>
> - `GET /entity/{id}/render/statements/{property}`: one statement group.
> - `GET /entity/{id}/render/terms`: the term box.
> - `GET /page/{id}/render`: a document or thread body.
>
> They carry the same `ETag`, `Cache-Tag` and redaction as full pages (0014 §3). Full page renders are assembled from the same fragment functions, so a fragment and its page cannot drift.

Replaced text (§8):

> - The release build embeds `ui/dist` into the binary (`rust-embed`); templates read the manifest to emit `<script type="module">` tags. **Node is a build-time dependency only**; the deliverable stays one binary (0033 §1).
> - In development, the server proxies asset requests to the Vite dev server for hot reload.

Replaced text (§11):

> - Components never insert HTML they built from user input. HTML they insert comes only from fragment routes, which the server has sanitized (0033 §9).

### A7. Codex, themed by tokens

- **Date:** 2026-10-03
- **Source:** Direct: James, design discussion of 2026-10-03
- **Change:** amends §1; extends §10
- **Summary:** Codex stays, for its accessible components, its right-to-left and language support and its CSS-only components, but no longer strictly as-is: the instance or a tenant may set Codex's design tokens to its own values as a theme (`ui.theme`), served as a stylesheet of custom properties and refused unless its colours meet WCAG 2.1 AA contrast. Components, markup and behaviour stay Codex's. James, asked whether to keep Codex at all: "I like your middle path with Codex. It has more to do with the accessibility and right-to-left benefits than pure visual or UX fidelity. Being able to get HTML-only too is a bonus."

Replaced text (§1):

> 4. **Codex as-is.** Codex tokens, components and icons without overrides. Where a design needs something Codex lacks, it is built from Codex tokens and proposed upstream.
