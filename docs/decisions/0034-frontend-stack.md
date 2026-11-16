# 0034. Frontend technology stack

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-11-15 (A21)
- **Author:** James Hare / Claude
- **Uses:** [0003](0003-statement-ui.md), [0012](0012-api-requirements.md), [0014](0014-caches-and-search.md), [0017](0017-entity-id-grammar.md), [0029](0029-resolver-namespaces.md), [0033](0033-backend-stack.md)

## Context

0003 and 0010 describe what the statement UI and the site UI show. 0010 left open whether pages are rendered on the server or the client, and how mobile layouts work. 0012 §1 requires the UI to use only the public API and to discover capabilities. 0014 caches pages at an HTTP layer with `ETag` and `Cache-Tag`.

James decided on 2026-09-27 that **the designs adopt Codex, Wikimedia's design system, as-is**. Codex ships Vue 3 components and CSS-only components that look identical to them; most Vue components have a CSS-only version, and the accessible interactive ones (Lookup, Combobox, Menu, Dialog, TypeaheadSearch) need Vue. It targets WCAG 2.1 AA with wide i18n and RTL support, and has been bundled with MediaWiki since 1.39.

The backend stack is in 0033.

## Decision

### 1. Principles

*Current text: [19](../architecture/19-site-ui.md) §1, §5.1.*

### 2. Server rendering, in `triplespace-ui`

*Current text: [19](../architecture/19-site-ui.md) §1.2, §5.2.*

### 3. Statement groups (uses 0003 §3)

*Current text: [19](../architecture/19-site-ui.md) §4.9.*

### 4. Interactive components

*Current text: [19](../architecture/19-site-ui.md) §3.10, §5.3.*

### 5. Fragments

*Current text: [20](../architecture/20-web-tier.md) §3.2.*

### 6. Rust in the browser, in `scatter-wasm`

*Current text: [19](../architecture/19-site-ui.md) §5.4.*

### 7. Source editor

*Current text: [19](../architecture/19-site-ui.md) §3.2, §5.5.*

### 8. Build

*Current text: [22](../architecture/22-crates-and-stack.md) §5.1.*

### 9. Internationalisation

*Current text: [19](../architecture/19-site-ui.md) §5.6.*

### 10. Accessibility and browser support

*Current text: [19](../architecture/19-site-ui.md) §5.7.*

### 11. Security

*Current text: [09](../architecture/09-security-and-moderation.md) §1.4.*

### 12. Testing

*Current text: [22](../architecture/22-crates-and-stack.md) §5.2.*

### 13. Prototyping path

*Current text: [19](../architecture/19-site-ui.md) §5.8.*

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

- **Q1. Codex upgrade cadence.** *Settled by A8.*
- **Q2. Night mode.** *Settled by [0086](0086-theming.md) §2.*
- **Q3.** Whether the Rust Codex builder becomes a published crate other projects can use.
- **Q4.** An interactive charting library for exploring a property's full view (0003), beyond the static SVGs.
- **Q5.** Visual regression testing.
- **Q6.** The maturity of the `banana-i18n` Rust crate (§9).

## Amendment log

### A1. Expansion preview on the server

- **Date:** 2026-09-30
- **Source:** 0042 §15
- **Change:** amends §6
- **Summary:** When the source needs expansion the browser cannot do, the preview comes from `action=parse` on the server instead.

Replaced text: §6 of this ADR.

### A2. A Lua mode

- **Date:** 2026-09-30
- **Source:** 0043 §13
- **Change:** extends §7
- **Summary:** A Lua mode for module pages.

### A3. The table grid editor

- **Date:** 2026-09-30
- **Source:** 0045 §11
- **Change:** extends §4
- **Summary:** A table grid editor (Codex Table, Lookup, TextInput; the statement value editor for cells) over the server-rendered grid of a `Table` page. Reading a table needs no JavaScript.

### A4. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** 0050 §13
- **Change:** consolidates §1–13
- **Summary:** A1–A3 were folded into the Decision. The title's em dash became a full stop, the open questions were numbered, the Author line was added, and the list this ADR kept under "Changes to other ADRs" (quoted below) was replaced by the generated table. That list also claimed changes to 0003 §3, 0012 §5 and 0014 §5 (renderers, the fragment routes, fragment caching); none was ever written into those ADRs, and nothing records them there, so the fragment routes and their caching are stated in §5 of this ADR alone. No decision changed. Before this, A1–A3 were blockquotes. The file before conversion is commit `0b26a3a`.

Replaced text: Changes to other ADRs of this ADR.

### A5. CSS linting and TemplateData in the editor

- **Date:** 2026-10-01
- **Source:** 0055 §2, §5
- **Change:** extends §6, §7
- **Summary:** `scatter-wasm` re-exports `scatter-css` for linting stylesheets; the source editor gains a CSS mode, Insert template… and a TemplateData parameter popup.

### A6. The web tier

- **Date:** 2026-10-03
- **Source:** 0057 §1, §2, §4, §8
- **Change:** amends §1, §2, §5, §8, §11; extends §13
- **Summary:** The server-side renderer is an API client like the browser: it calls the public HTTP API with the viewer's credentials, from `triplespace-web` over the network or from `triplespace-server` in-process, and never the API crates' handlers. Either binary serves the site and its assets. Fragment routes are gone: a component fetches its region with `action=render` and `region` on the page's own URL, which also ends the collision of `GET /page/{id}/render` with 0042 §12's render manifest. In development the web tier can be pointed at Wikidata directly. A consequence is added.

Replaced text: §1 of this ADR; §2 of this ADR; §5 of this ADR; §8 of this ADR; §11 of this ADR.

### A7. Codex, themed by tokens

- **Date:** 2026-10-03
- **Source:** Direct: James, design discussion of 2026-10-03
- **Change:** amends §1; extends §10
- **Summary:** Codex stays, for its accessible components, its right-to-left and language support and its CSS-only components, but no longer strictly as-is: the instance or a tenant may set Codex's design tokens to its own values as a theme (`ui.theme`), served as a stylesheet of custom properties and refused unless its colours meet WCAG 2.1 AA contrast. The site reads the theme from `siprop=triplespace`, since it reaches the instance only through the API, and serves it under a URL named by its hash. Components, markup and behaviour stay Codex's. James, asked whether to keep Codex at all: "I like your middle path with Codex. It has more to do with the accessibility and right-to-left benefits than pure visual or UX fidelity. Being able to get HTML-only too is a bonus."

Replaced text: §1 of this ADR.

### A8. The shipped default theme; the Codex release

- **Date:** 2026-10-03
- **Source:** Direct: James, design discussion of 2026-10-03
- **Change:** amends §1; extends §2, §8
- **Summary:** "Let's ship the canvases' palette and Newsreader headings as the shipped default theme." A site with no `ui.theme` wears `default` from the new `docs/registry/themes.toml`: Codex token values taken from the site and statement UI canvases, with Newsreader for headings (`font-family-heading-main`), IBM Plex Sans for text and IBM Plex Mono for IDs, every text pair at WCAG 2.1 AA and the boundaries of controls at 3:1; the canvases' light input borders were darkened to meet the latter. The typefaces are self-hosted under the OFL. "Codex latest release": Codex is pinned at its latest release, 2.7.0, and bumped deliberately, which settles Q1.

Replaced text: §1 of this ADR.

### A9. The wasm list lives in 0005 rule 7

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §6
- **Summary:** 0005 §3 rule 7 is the one wasm list: the nine crates its table marks. §6 and `cargo xtask wasm` read that list rather than keeping a list of their own; §6's four-item enumeration of what `scatter-wasm` re-exports is replaced by a reference to rule 7. (PENDING F23)

Replaced text: §6 of this ADR.

### A10. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** 0050 §14
- **Change:** relocates §1–§13

### A11. The interface language is negotiated

- **Date:** 2026-10-17
- **Source:** 0085 §2.2
- **Change:** amends §9
- **Summary:** A reader is served in their language without having to choose one: the interface language comes from `uselang`, then the reader's `language` preference, then the request's `Accept-Language` matched against the languages the interface has messages for, then the tenant's default (`site.language` unless `preferences.defaults` names another) and the farm's. Readers who are not logged in are served the same way. The chapter text is 19 §5.6.

Replaced text: 19 §5.6.

### A12. Palettes and type sets replace the flat theme

- **Date:** 2026-10-17
- **Source:** 0086 §1, §2, §5
- **Change:** amends §1; extends §11
- **Summary:** The flat theme of A7, one map of Codex token values in `ui.theme`, becomes two choices held as config records: a palette, with `light`, `dark`, `light-more` and `dark-more` colour variants and the shape tokens, each variant held to its contrast floor, and a type set, with the three typeface roles and the typefaces it serves. The shipped default of A8 is the palette `parchment` (the same light values, now with a walnut-brown dark variant and high-contrast variants) with the type set `default26`; for accessibility, the palettes `parchment-ob` and `parchment-co`, for colour-vision deficiencies, and the type sets `hyperlegible` and `opendyslexic` ship, and a reader may always choose them. A reader chooses mode, contrast, text size, palette and type set, the last two within what the wiki offers. One stylesheet per palette and one per type set hold their variants and modes under `data-*` attributes and media queries. `font-src` and `img-src` admit the media origin, where a tenant's typefaces and logo are served as site assets. Q2 is settled: Codex's dark tokens are the base every palette's `dark` overlays. `ui.theme` is retired. The chapter text is 19 §5.1, §5.2 and §5.9, 09 §1.4, 02 §1.4 and 23 §3.2.

Replaced text: 19 §5.1, §5.2; 02 §1.4; 23 §3.2.

### A13. `style` attributes are allowed

- **Date:** 2026-10-20
- **Source:** Direct: James, review of 2026-10-20
- **Change:** amends §11
- **Summary:** The site's Content Security Policy adds `style-src-attr 'unsafe-inline'`: `style` attributes apply, while stylesheets still come only from the instance's origin and `<style>` elements stay refused. Wikitext writes `style` attributes and MediaWiki's Sanitizer, which the renderer follows (0008 A39), checks each value, so blocking them took the styling off content the API had already accepted: infoboxes, navboxes, coloured table cells and hidden elements, above all on mirrored Wikipedia pages. A style attribute cannot run script; what it can still do is lay content over the page, as on MediaWiki, where moderation answers it. `style-src-attr` is in every current browser engine. The policy does not yet admit the `<style>` elements `<templatestyles>` emits and mirrored Wikipedia HTML carries (11 §6.3); whether they come in as a hash per element in each page's own policy or as stylesheets served from the origin is not decided here. The chapter text is 09 §1.4.

Replaced text: 09 §1.4, its first sentence.

### A14. Skins and message bundles

- **Date:** 2026-10-29
- **Source:** 0097 §8, §9, §10, §11, §12, §13
- **Change:** amends §2, §9
- **Summary:** The site's HTML is drawn by a skin the operator installs: MiniJinja templates over the view models `triplespace-ui` builds, under a versioned template contract, with nine template functions, named menus and link classification. The default skin is a skin; it first reproduces today's pages, and the built-in `askama` rendering and the Codex builder are then removed, the Codex components becoming the default skin's macros. Interface messages come in versioned bundles, the binary's in `i18n/` and each skin's in its own, a skin's keys carrying its name as a prefix. The chapter text is 19 §5.2, §5.6 and §5.10.

Replaced text: 19 §5.2.

Replaced text: 19 §5.6.

### A15. `uselang=qqx` shows message keys

- **Date:** 2026-11-04
- **Source:** Direct: Claude, while building template debugging (SCA-230), 2026-11-04; unconfirmed
- **Change:** extends §9
- **Summary:** `uselang=qqx` makes every interface message show its key, as MediaWiki's `qqx` does (the ShowMessageNames gadget on Wikipedia, Wikidata and Wikisource): `(key)`, or `(key: a, b)` with the parameters given. The calls:
  - The wiki's Interface overrides are not read under `qqx`, so the key shows even for an overridden message, and nothing is expanded (`PLURAL`, `GENDER`, `GRAMMAR`).
  - Messages handed to a script read `(key: $1, $2)`, one `$n` up to the highest the English message names, for the script to fill as it fills any message.
  - `qqx` is the page's `lang` and is written left to right; a term (label, description) is looked up along `qqx`'s chain, which ends at `mul` and English, so content shows as in English.
  - `{{int:key}}` in page text is not affected: a page renders once for every reader in the wiki's language (0042 A21), so `qqx` reaches only the site's own messages.

### A16. No sparklines

- **Date:** 2026-11-09
- **Source:** Direct: James, design discussion of 2026-11-09
- **Change:** amends §3
- **Summary:** Sparklines are dropped: no statement-group layout (19 §4.3) defines one, and nothing draws them. Charts and timeline strips stay SVG drawn in Rust. Chapter text: 19 §4.9.

Replaced text: 19 §4.9.

### A17. Thread fragments, the browser's language, and the media origin: the calls made building them

- **Date:** 2026-11-10
- **Source:** Direct: Claude, while building three of the gaps the implementation-claims audit found (SCA-312), 2026-11-10; unconfirmed
- **Change:** extends §5, §9, §11
- **Summary:** Three parts of this ADR are built as written, with these calls. (1) §5: a thread answers `action=render` with its posts in tree order and `region=post/{id}` with the one post, drawn by the macro its page uses; the header that says where the thread lives and its status is not part of either; a post the thread does not have is an unknown region; a talk page's `action=render` stays its document's; `region=row/{id}` waits for Table pages. (2) §9: the site matches `Accept-Language` itself for a reader who is not logged in, against the languages the interface has messages for, each tried as written and then by its primary subtag; a signed-in reader's language already came negotiated from the API. (3) §11: `meta=siteinfo` reports `media.base_url` as `triplespace.media.base_url` only where the wiki has set it, and the site adds its origin to `img-src` and `font-src` where it is not the wiki's own, on the pages it draws in the frame and on fragments; the origin alone is taken, so nothing a setting holds can add a directive. Not built: the page bundle and bulk labels routes of §1, which the API does not serve yet. Chapter text: 20 §3.2; 19 §5.6; 09 §1.4.

### A18. The job progress bar is built; alias chips wait on Codex

- **Date:** 2026-11-10
- **Source:** Direct: Claude, while building two of the Codex components SCA-312 lists, 2026-11-10; unconfirmed
- **Change:** extends §4
- **Summary:** A running job's page shows Codex's ProgressBar, indeterminate and inline, named by the progress the page says, since a job reports the stage and the pages done and no total. Not built: **Sync again** and **Run again** beside it, which need jobs that can be queued again (0104 §4). The term editor's aliases stay a text area, one alias a line: Codex 2.7.0's ChipInput was built in and taken out again, because its chip list is a `listbox` with no accessible name and, while it holds no chip, no option, which fails axe's `aria-input-field-name` and `aria-required-children` and so the accessibility bar of §10; neither can be set from outside the component. It can return when a Codex release passes, or if James accepts the two findings as an exception. Chapter text: 19 §5.3.

### A19. The Vite dev server: `server.vite` and `web.vite`

- **Date:** 2026-11-15
- **Source:** Direct: Claude, while building the development proxy to the Vite dev server (SCA-328), 2026-11-15; unconfirmed
- **Change:** extends §8
- **Summary:** The proxy to the Vite dev server is built, with these calls. (1) The option is `--vite` on both binaries (`server.vite`, `TRIPLESPACE_VITE`; `web.vite`, `TRIPLESPACE_WEB_VITE`), an origin such as `http://localhost:5173`; it exists only in a debug build, so a release binary has no such option and nothing of it. (2) With it set, a page links Vite's client and the source entries (`src/site.css`, `src/head.js`, `src/main.js`) under `/ui/assets/`, on the site's own origin, and the binary passes every request under `/ui/assets/` through to the dev server; what the dev server lacks (a type set's built font files) is served from `ui/dist`, as is everything when the dev server is not running. (3) The head script is linked as a module, since the dev server serves it with its imports; it therefore runs after the first paint, where the built one blocks. (4) The Content Security Policy is widened by three sources and no more: the dev server's WebSocket in `connect-src` (Vite's client connects to it directly, since the binary passes no WebSocket through), `'unsafe-inline'` in `style-src` (Vite applies a stylesheet a module imports as a `<style>` element), and `data:` in `font-src` (it serves a small font inline). Scripts still come from the site's own origin alone. (5) Templates and Rust are not reloaded: a template change still needs the web process restarted. Chapter text: 22 §5.1; 23 §3.6.

### A20. The header's search box suggests, on Codex's Lookup

- **Date:** 2026-11-15
- **Source:** Direct: Claude, while building the header's search suggestions (SCA-326), 2026-11-15; unconfirmed
- **Change:** amends §4
- **Summary:** The header's search box suggests as the reader types, with these calls. (1) It is built on Codex's Lookup inside the header's own form, not on Codex's TypeaheadSearch: 2.7.0's TypeaheadSearch draws each suggestion, and its footer, as a link inside an option, which axe refuses (`nested-interactive`), as it refused ChipInput (A18). A suggestion here is an option; choosing it goes to its page. TypeaheadSearch takes its place once a Codex release passes, or on James's decision to accept the finding as an exception. (2) The component, with Vue, loads the first time the box is focused or typed in and takes the plain form's place, keeping what was typed and the focus, so a reader who does not search downloads none of it; without JavaScript the form searches as before. (3) The suggestions: where the text is shaped as an entity ID that no suggestion names, **Go to** first, which is the search page's own go; then items, properties and domains by label or alias, at most four of each, then pages by title, ten in all; each entity with its description and its ID, a page marked as one; last, a line to the full results. The suggestions are flat, in that order, not grouped under headings. (4) Enter with no suggestion highlighted submits the form, as the plain one does. (5) The server writes the language, the kinds and the `ts-typeahead-*` messages beside the box. The bell's panel stays the site's own markup and script, not Codex's Popover: nothing was found wrong with it, and whether to rebuild it on Popover is James's to decide. Chapter text: 19 §5.3.

Replaced text: 19 §5.3.

### A21. `banana-i18n` in the components; `GENDER` and `GRAMMAR` stay degenerate on the server

- **Date:** 2026-11-15
- **Source:** Direct: Claude, while building per-skin message bundles and `banana-i18n` in the components (SCA-327), 2026-11-15; confirmed by James, 2026-11-15
- **Change:** extends §9
- **Summary:** **The components format their messages with the `banana-i18n` JavaScript library** (2.4.0, MIT, pinned), as this section says: the editing components, the grid and the header's search box read the messages the server hands them in their data block, unexpanded, and one shared formatter loads that set for the interface language, so `$n` and `{{PLURAL:}}` mean in the browser what they mean on the server. The calls: the server still hands each component only the messages it needs, from the same files, where the library could fetch whole files; `[[…]]` in a message stays text, since links are the site's to draw; a key the set lacks shows as `⧼key⧽`; the grid's block gains the interface language, which it lacked. A test holds every shipped message a component is handed to the text the components produced before. **On the server** `{{GENDER:}}` keeps taking the neutral form and `{{GRAMMAR:}}` keeps returning the word unchanged. English needs neither, no shipped message uses either, and the in-house implementation this section allows expands `PLURAL` with ICU4X's rules, which is what English messages do use. They are built, with a reader's gender from a preference and each language's grammar rules, before the first translation that calls one; James may ask for them sooner. Chapter text: 19 §5.6.
