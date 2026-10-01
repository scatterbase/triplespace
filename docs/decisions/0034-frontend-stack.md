# 0034 — Frontend technology stack

- **Status:** Proposed
- **Date:** 2026-09-27
- **Amended by:** [0042 — Template expansion and the Parsoid renderer](0042-template-expansion-and-parsoid.md) (§15 amends §6: pages that need expansion preview on the server), [0043 — Lua modules](0043-lua-modules.md) (§13 extends §7: a Lua mode), [0045 — Tables](0045-table-content-model.md) (§11 extends §4: the table grid editor)
- **Related:** 0003, 0005, 0008, 0010, 0012, 0014, 0019, 0020, 0021, 0027, 0033

## Context

0003 and 0010 describe what the statement UI and the site UI show. 0010 left open whether pages are rendered on the server or the client, and how mobile layouts work. 0012 §1 requires the UI to use only the public API and to discover capabilities. 0014 caches pages at an HTTP layer with `ETag` and `Cache-Tag`.

James decided on 2026-09-27 that **the designs adopt Codex, Wikimedia's design system, as-is**. Codex ships Vue 3 components and CSS-only components that look identical to them; most Vue components have a CSS-only version, and the accessible interactive ones (Lookup, Combobox, Menu, Dialog, TypeaheadSearch) need Vue. It targets WCAG 2.1 AA with wide i18n and RTL support, and has been bundled with MediaWiki since 1.39.

The backend stack is in 0033.

## Decision

### 1. Principles

1. **The server renders every page.** Reading never needs JavaScript. Rendered pages are cacheable at the L2 layer (0014 §3).
2. **JavaScript enhances regions, never whole pages.** An interactive component takes over one region of a server-rendered page. There is no client-side router.
3. **One renderer per thing.** Anything the server draws is drawn only by the server. A component that changes it fetches the server's rendering afterwards (§5) rather than drawing its own copy.
4. **Codex as-is.** Codex tokens, components and icons without overrides. Where a design needs something Codex lacks, it is built from Codex tokens and proposed upstream.
5. **The public API only** (0012 §1). The browser calls public routes. The server-side renderer calls the API crates' handler layer in-process, never `view` tables directly, so every page gets the same per-viewer redaction as the API.

### 2. Server rendering (`triplespace-ui`)

- **Templates:** `askama`, compiled and type-checked with the Rust code.
- **Codex markup:** a Rust builder module that emits Codex CSS-only component markup (buttons, fields, tables, cards, tabs, messages, chips, progress bars), following the approach of WMF's Codex PHP. Templates call the builder instead of writing Codex class names by hand, so a Codex markup change is one edit.
- **Frame:** the page frame of 0010 §2 (global header, identity line, title, tabs) is one template shared by every page kind.
- **Assets:** Codex CSS, design tokens and icons come from the pinned `@wikimedia/codex`, `@wikimedia/codex-design-tokens` and `@wikimedia/codex-icons` packages at build time (§8), with hashed file names, served by the binary with long-lived cache headers.
- **Mobile:** one responsive site built on Codex's breakpoints. No separate mobile domain or skin. Detailed mobile layouts stay open (0010).

### 3. Statement groups (0003)

- The layouts of 0003 (table, matrix, chart, timeline, chips, "Same for all N", footnoted shared references) are rendered **only on the server**.
- Tables use Codex's CSS-only Table.
- Charts, sparklines and timeline strips are **SVG generated in Rust** from the same view model the classifier produces. They cache like any other markup and need no JavaScript. No client charting library is used for these.
- The shape classifier lives in a pure, wasm-capable crate, so an editor can predict the layout a change will produce (§6).

### 4. Interactive components

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

Statement editing requires JavaScript, as on Wikidata. Reading, page source editing through a plain form, and the account pages work without it.

> **Extended by [0045](0045-table-content-model.md) §11.** A table grid editor (Codex Table, Lookup, TextInput; the statement value editor for cells) over the server-rendered grid of a `Table` page. Reading a table needs no JavaScript.

### 5. Fragment routes

After a component saves, it fetches the server's rendering of the changed region and swaps it in. The fragments are public API routes under `rest.php/triplespace/v0` (0012 §2), in the spirit of Wikibase's `wbformatvalue`:

- `GET /entity/{id}/render/statements/{property}`: one statement group.
- `GET /entity/{id}/render/terms`: the term box.
- `GET /page/{id}/render`: a document or thread body.

They carry the same `ETag`, `Cache-Tag` and redaction as full pages (0014 §3). Full page renders are assembled from the same fragment functions, so a fragment and its page cannot drift.

### 6. Rust in the browser (`scatter-wasm`)

One `wasm-bindgen` crate re-exports what editors need from the pure crates:

- the 0003 shape classifier;
- `scatter-normalize` (value normalization and keyed-ID grammars, 0017, 0029);
- `scatter-wikitext` and the `scatter-pages` markdown renderer, for live preview.

It is built with `wasm-bindgen-cli` and `wasm-opt`, lazy-loaded only by editors, and never needed for reading. This is one reason 0033 §9.1 chose a pure-Rust wikitext parser.

> **Amended by [0042](0042-template-expansion-and-parsoid.md) §15.** When the source needs expansion the browser cannot do, the preview comes from `action=parse` on the server instead.

### 7. Source editor

CodeMirror 6, which MediaWiki's CodeMirror extension also uses, with its wikitext mode and a markdown mode. Edit conflicts (0010 §5) are shown as line decorations. `[[` triggers link autocomplete through the suggest route (0012). `tree-sitter-wikitext` through `web-tree-sitter` is an optional later enhancement for structural highlighting.

> **Extended by [0043](0043-lua-modules.md) §13.** A Lua mode for module pages.

### 8. Build

- `ui/` is an npm workspace: TypeScript, Vue 3, Codex packages pinned to exact versions, CodeMirror 6.
- **Vite** builds the components into hashed ES modules and a manifest.
- The release build embeds `ui/dist` into the binary (`rust-embed`); templates read the manifest to emit `<script type="module">` tags. **Node is a build-time dependency only**; the deliverable stays one binary (0033 §1).
- In development, the server proxies asset requests to the Vite dev server for hot reload.
- Lint: ESLint with `eslint-config-wikimedia`, Stylelint with `stylelint-config-wikimedia`. Styles are plain CSS using Codex's CSS custom-property tokens; no Less.

### 9. Internationalisation

- Interface messages are **banana JSON** files in `i18n/` (`en.json`, `qqq.json` for documentation, one file per language), so the project can be registered with translatewiki.net like any MediaWiki extension.
- The server renders messages with the `banana-i18n` Rust crate; components use the `banana-i18n` JavaScript library. Both read the same files. The Rust crate is at 0.1.0; if it proves incomplete, a small in-house implementation of `PLURAL`, `GENDER`, `GRAMMAR` and `$n` with ICU4X plural rules replaces it.
- `banana-checker` runs in CI.
- Interface language comes from the `language` preference (0027) with `uselang` as an override; fallback chains follow MediaWiki's. Direction comes from the language and is set on `<html dir>`; Codex handles RTL.

### 10. Accessibility and browser support

- WCAG 2.1 AA, inherited from Codex and checked by axe in end-to-end tests (§12).
- Browser support follows MediaWiki's: modern browsers get the components; older browsers get the server-rendered pages without them.

### 11. Security

- A strict Content Security Policy: scripts only from the instance's own origin, no inline scripts, no `eval`. Initial data is passed in `application/json` blocks, which CSP does not execute.
- Components never insert HTML they built from user input. HTML they insert comes only from fragment routes, which the server has sanitized (0033 §9).

### 12. Testing

| Tool | Use |
|---|---|
| Vitest + `@vue/test-utils` | component tests |
| Playwright | end-to-end flows, including a JavaScript-disabled run of every read page |
| `@axe-core/playwright` | accessibility checks on every page kind |
| `insta` | snapshots of rendered templates and fragments |

### 13. Prototyping path

As the statement UI spec planned, the UI is built before the backend is complete:

1. `triplespace-ui` renders item pages from Wikidata's canonical JSON (`Special:EntityData`) through the shape classifier.
2. Provenance panels use fixtures.
3. The editing components write through the Action API to test.wikidata.org or the MediaWiki 1.43 reference install.

Nothing in the UI changes when the source becomes a Triplespace instance, because it only speaks the public API.

## Alternatives considered

- **Leptos (full-stack Rust, server rendering with islands).** One language and one component tree, but Codex's accessible interactive components would have to be rebuilt in Rust, and Leptos is still pre-1.0. It would also narrow the contributor pool to Rust developers.
- **Vue single-page app with Nuxt server rendering.** Needs a Node runtime in production, against 0033 §1.
- **htmx-style fragments only.** Covers updates but not accessible comboboxes, lookups and dialogs, which Codex already provides.
- **OOUI.** Superseded by Codex in MediaWiki.

## Consequences

- Wikimedia developers who know Vue and Codex can work on the editors without learning Rust; Rust developers own the templates and renderers.
- Pages read and cache without JavaScript, and the statement layouts exist in one implementation.
- The build needs Node, though the running system does not.
- Codex upgrades are deliberate: versions are pinned and bumped in their own commits, with snapshot and accessibility tests as the check.

## Changes to other ADRs

- **0010:** the server-or-client rendering question is settled by §1–§5; mobile is settled as one responsive site (§2), with detailed layouts still open.
- **0003:** renderers and chart drawing per §3.
- **0012:** fragment routes of §5 added to the REST route groups.
- **0014:** fragments cached and purged like pages (§5).
- **0005:** new crates `triplespace-ui` (surfaces layer) and `scatter-wasm` (wasm bindings over pure crates); `ui/` workspace noted; changelog row.

## Open questions

1. Codex upgrade cadence, and whether to track the version bundled with a MediaWiki release.
2. Night mode: adopt Codex's dark-mode tokens when they are stable.
3. Whether the Rust Codex builder becomes a published crate other projects can use.
4. An interactive charting library for exploring a property's full view (0003), beyond the static SVGs.
5. Visual regression testing.
6. The maturity of the `banana-i18n` Rust crate (§9).
