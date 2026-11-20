# 0097. Versioning, and skins apart from the binary

- **Status:** Proposed
- **Date:** 2026-10-29
- **Updated:** 2026-11-15 (A13)
- **Author:** James Hare / Claude Opus
- **Uses:** [0012](0012-api-requirements.md), [0028](0028-tenancy-policy.md), [0050](0050-adr-format.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [0058](0058-packed-record-storage.md), [0085](0085-settings-and-preferences.md)

## Context

Nothing in Triplespace had a version policy. `[workspace.package]` set no `version`, so every crate was `0.0.0`. The log header carried a format version and payload types a `v0` tag ([0015](0015-record-format-and-partition-registry.md) §2), but nothing said what a new one meant. `meta=siteinfo` reported `api_version` as an integer, and the web tier refused to serve below the minimum it was built for ([0057](0057-web-tier.md) §9), but nothing said what a change to that integer promised. The registries carried `version = 1` with no rule for the next number.

The site's HTML was built in `triplespace-ui`: a frame and nine Codex component templates in `askama`, which compiles templates into the binary, and the rest, about 28,000 lines of Rust, writing HTML through string builders ([0034](0034-frontend-stack.md) §2). Changing how the site looked meant rebuilding the binary. Theming ([0086](0086-theming.md)) already let a tenant choose colours and typefaces by setting Codex's tokens, without touching markup.

Triplespace has no wiki-editable JavaScript: user and site scripts are excluded, and adoption drops `MediaWiki:Common.js` ([0095](0095-the-interface-namespace.md) §7). What MediaWiki communities build as gadgets and user scripts has to be built into the binary, done by whoever controls the HTML, or not done.

### Direction

James's direction, from the design discussion of 2026-10-29:

- **"I think we should implement the principles of semantic versioning. Major dot minor dot patch. A patch version addresses pre-existing functionality. You increment the minor version when you introduce a new feature without breaking backwards compatibility. You increment the major version when you break backwards compatibility. If the major version is 0, the compatibility contract is considered unstable."** Asked whether that meant Cargo's stricter reading of 0.x, where `0.3 → 0.4` may break and `0.3.1 → 0.3.2` may not: "The stricter reading you are referring to is what I meant."
- **"What compatibility and backwards compatibility means depends on the thing. … Where we are implementing someone else's contract, we should use their version numbers. Our own APIs should also follow the principles of semantic versioning."**
- **"We could uncouple the concept of a skin from the UI in general. The frontend binary fields requests and produces responses, but the HTML structure is a static document that could easily be uncoupled from it. Anything can be a skin so long as it is compatible with the template structure the binary is expecting. The default appearance would become a skin, and it would have its own versioning. A skin is different from a theme. Theming includes customizing the color palette and type set within the HTML presentation while a skin is the HTML presentation."**
- **"If we uncouple static assets from the binary, we could then separately version themes, skins, interface message bundles, and then finally the URL structure, templating structure, and API contract are what's versioned."**
- **"Skins can only be installed by instance operators."** "Since skins are deployed only by instance operators, there is a higher degree of trust."
- **"We should aim for a minimum number of highly useful functions that target the most desired features. What do people keep reinventing in gadgets and user scripts?"**
- **"I agree that theme tokens are part of the binary."**
- **"Readers can choose from the skins that the instance makes available. Whether a tenant can make a skin available or disable it depends on instance policy."** And, asked whether a release that writes a new record major should first ship it behind a switch so that every minor can be rolled back: "Yes."
- **"The patch number is incremented when a new version of something is published. Something is 'published' when it is compiled as a downloadable bundle, shipped to crates.io, or equivalent thing that marks that the code has stabilized. Once the patch is published, the patch number increases in the repository. That means that a site running directly off of git is going to be at minimum one patch version higher than someone using a published binary. If you add a new feature or make a compat-breaking change, you increase the minor or major version number once it happens, with the patch or minor version set to 0 accordingly. Ideally this sort of work is done on a branch so as to not conflict with existing stable code and stable deployments."**
- **"One way we can test the skinning system is by reimplementing the current frontend UI as a skin, with the goal of getting identical results between the built-in implementation and the skin version. Once equality is accomplished, the internal rendering can be removed."**

## Decision

### 1. Semantic versioning, read as Cargo reads it

*Current text: [22](../architecture/22-crates-and-stack.md) §3.5.*

### 2. What is versioned

*Current text: [22](../architecture/22-crates-and-stack.md) §3.5.*

### 3. Crates (extends 0005 §6)

*Current text: [22](../architecture/22-crates-and-stack.md) §3.4.*

### 4. Record formats (extends 0015 §2)

*Current text: [01](../architecture/01-log-and-records.md) §2.2.*

### 5. The REST API and `api_version` (amends 0057 §9)

*Current text: [20](../architecture/20-web-tier.md) §4.*

### 6. Contracts Triplespace implements

*Current text: [22](../architecture/22-crates-and-stack.md) §3.5.*

### 7. The product version (extends 0077 §6)

*Current text: [21](../architecture/21-special-pages.md) §5.6; [22](../architecture/22-crates-and-stack.md) §3.5.*

### 8. Skins (amends 0034 §2)

*Current text: [19](../architecture/19-site-ui.md) §5.2, §5.10; [20](../architecture/20-web-tier.md) §1.2; [21](../architecture/21-special-pages.md) §5.6; [23](../architecture/23-configuration-and-registry.md) §3.4, §3.6, §6.*

### 9. The template contract (extends 0034 §2)

*Current text: [19](../architecture/19-site-ui.md) §5.10.*

### 10. Functions a template may call (extends 0034 §2)

*Current text: [19](../architecture/19-site-ui.md) §5.10.*

### 11. Menus (extends 0034 §2)

*Current text: [19](../architecture/19-site-ui.md) §5.10.*

### 12. Link classification (extends 0034 §2)

*Current text: [19](../architecture/19-site-ui.md) §5.10.*

### 13. Message bundles (amends 0034 §9; extends 0095 §2)

*Current text: [19](../architecture/19-site-ui.md) §5.6.*

### 14. Theme tokens (extends 0086 §5)

*Current text: [19](../architecture/19-site-ui.md) §5.9.*

### 15. Caching and fragments (extends 0057 §6, §8)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §10.3; [19](../architecture/19-site-ui.md) §5.10; [20](../architecture/20-web-tier.md) §3.2.*

### 16. Reaching parity, then removing the built-in rendering

*Current text: [19](../architecture/19-site-ui.md) §5.10.*

### 17. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 18. Releases and the version in the repository

*Current text: [22](../architecture/22-crates-and-stack.md) §3.5.*

## Alternatives considered

- **One version for the whole product, nothing else.** MediaWiki has been 1.x for twenty years because a product major was never defined; operators learn nothing from the number, and a skin or client cannot say what it needs.
- **Calendar versions for the product.** Honest about a product's cadence, but they say nothing about whether an upgrade is safe, which is what an operator needs from the number.
- **Skins as Rust crates compiled in.** Keeps `askama`'s type checking, but every skin change is a rebuild, which is the coupling this ADR removes.
- **Jinja's full power, or logic-less templates.** MediaWiki's SkinMustache made skins logic-less over a documented set of data keys, which is what fixed its skins breaking on every release; Triplespace keeps that idea, a contract made of view models, but uses MiniJinja so that a trusted skin can loop, branch and define its own macros. The functions it adds are kept to §10's nine, since each is a contract.
- **Tenant-installed skins.** A skin is HTML around every reader's session, CSRF tokens and scripts included; tenants keep palettes and type sets (0086).

## Consequences

- **Every release states its bump and why**, from the contracts it changed (§7).
- **The site's look can change without a rebuild**, and an operator can run a skin of their own.
- **What gadgets do on MediaWiki has a home**: data in view models and menus, behaviour in an operator's skin.
- **Interactive components stay Codex's**: the Vue components and editors are the binary's, styled by Codex and the theme under any skin; a skin restyles them only through tokens.
- **The migration is large**: about 28,000 lines of `triplespace-ui` become view models and templates, and every view model it needs is found by doing it.
- **Rendering at runtime costs more than compiled templates.** MiniJinja's compiled templates are expected to keep a page within its budget; the corpus runs measure it.

## Open questions

- **Q1. Rollback.** *Settled by §4.*
- **Q2. Additive payload changes.** Whether the current payload encodings admit fields an older reader ignores, as §4's minor assumes, or whether every payload change is in practice a major until the encodings are made extensible.
- **Q3. A reader's own skin.** *Settled by §8.*
- **Q4. Skin licences.** Plain and Worksheet are GPL-3.0-or-later with the rest of the repository. Whether an operator's skin, which the binary interprets and does not link, must be.
- **Q5. Extra view-model fields.** Whether a skin may ask for data from a fixed list the binary offers (assessment, protection, short description, edit counts), fetched only when a skin asks for it; deferred until the default skin needs one.

## References

- [Semantic Versioning 2.0.0](https://semver.org/)
- [The Cargo Book: SemVer compatibility](https://doc.rust-lang.org/cargo/reference/semver.html), and `cargo-semver-checks`
- MediaWiki: `SkinMustache` and its template data, `skin.json` and its `requires`, and `mw.util.addPortletLink`
- Gadgets and user scripts on English Wikipedia and Wikidata: linkclassifier, MarkBlocked, markadmins, navigation popups, reference tooltips, comments in local time, prosesize
- [MiniJinja](https://github.com/mitsuhiko/minijinja)
- Codex's release policy for `@wikimedia/codex` and `@wikimedia/codex-design-tokens`

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** 0050 §14
- **Change:** relocates §1–§18

### A2. Plain and Worksheet; switching by group

- **Date:** 2026-10-30
- **Source:** Direct: James, design discussion of 2026-10-30
- **Change:** amends §8, §13, §16, §17; extends §9
- **Summary:** Two skins ship. **Plain** draws the site's HTML and loads no stylesheet of its own; its DOM (element structure, class names, `data-` attributes, landmarks) is a contract versioned as Plain's version, with Codex's class names where Codex has a component and `ts-` classes filling the gaps. **Worksheet**, the default (`skins.default` defaults to `worksheet`), is Plain plus Codex's and the site's stylesheets. A skin either extends Plain, naming it as parent and adding stylesheets and the files they use and nothing else, so that everything written against Plain's DOM works on it; or stands alone, implementing the template contract with its own DOM, free but outside that compatibility. Plain is useful unstyled to operators building their own look, to readers styling it client-side, and to readers using assistive technology. Plain is written first to reproduce today's HTML, and parity is measured on Worksheet. Feature work on the site pauses while the migration runs. The chapter text is 19 §5.2, §5.6 and §5.10.

Replaced text: 19 §5.10.

Replaced text: 19 §5.2.

Replaced text: 19 §5.6.

### A3. Fragments are compared as normalized DOM

- **Date:** 2026-10-30
- **Source:** Direct: Claude, while building the parity harness, 2026-10-30; unconfirmed
- **Change:** amends §16
- **Summary:** The parity comparison reads `action=render` fragments the way it reads pages, as normalized DOM, not byte for byte. §16 assumed that with `html::esc` as the skin's escaper "escaping never differs", but the built-in renderer already escapes two ways: `askama` writes `&#60;`, `&#62;`, `&#38;`, `&#34;` in the frame and the Codex component templates, and `html::esc` writes `&lt;`, `&gt;`, `&amp;`, `&quot;` in the string builders, so a fragment that holds a Codex component cannot be reproduced byte for byte by a skin with one escaper. A browser builds the same DOM from either spelling, and the normalizer (`triplespace_ui::parity`) keeps every other difference, whitespace where it renders included. The chapter text is 19 §5.10.

Replaced text: 19 §5.10.

### A4. Drawings are SVG the binary makes

- **Date:** 2026-10-30
- **Source:** Direct: Claude, while building the entity page's view models, 2026-10-30; unconfirmed
- **Change:** extends §9
- **Summary:** A view model carries data for the skin to draw, with one exception: a drawing, which the binary makes as SVG and the view model carries as HTML (`*_html` fields). The Series chart and the Timeline axis of the entity page (0003 §3, §9) are the two: their geometry is computed from the values, no skin would lay it out differently, and drawing it in a template would put arithmetic where the contract wants placement. Everything else on the entity page, values and links included, is drawn by the skin from the model. The chapter text is 19 §5.10.

### A5. The budget is judged in milliseconds where the ratio misleads

- **Date:** 2026-10-30
- **Source:** Direct: James, design discussion of 2026-10-30
- **Change:** amends §16
- **Summary:** The entity pages (group 3) reached parity on every gate but the timing: +9.4% at the median over the corpus, which is about 2.5 ms on the heaviest page (a 500-statement item), since a template engine costs about twice the string builders it replaces and the harness's in-memory API makes the render share, and so the ratio, a worst case. James: "The 9.4% above baseline is tolerable here. 'Pages with hundreds of statements' describes a scenario traditional Wikibase handles very poorly, and it appears we handle it with slight delay instead of catastrophically slowing down." A group over the 5% budget switches when the absolute cost is one no reader can perceive; the budget stays as the gate for a slowdown of another order. The chapter text is 19 §5.10.

Replaced text: 19 §5.10.

### A6. A part of a later group is carried as HTML until that group switches

- **Date:** 2026-10-30
- **Source:** Direct: Claude, while building the entity history and diff pages, 2026-10-30; unconfirmed
- **Change:** extends §9 and A4
- **Summary:** The History tab (group 4) lists its rows as `Special:RecentChanges` draws them (group 7), by the same code. Until group 7 switches, the tab's view model carries that table as HTML (`list_html`), the way a drawing is carried (A4), and the template places it; when group 7 switches, the field becomes the rows' model and the changes table a macro both draw. A field of this kind is transitional: it names the group it waits for, and the migration does not end with one in place. The chapter text is 19 §5.10.

### A7. The no-JavaScript forms share one set of controls

- **Date:** 2026-10-31
- **Source:** Direct: James, confirmed 2026-10-31; Claude's call while building group 8's view models (search, the special-page index, all pages, the reports and the redirecting pages), 2026-10-31
- **Change:** extends §9
- **Summary:** A form's controls are one vocabulary in Plain, not one per page: a labelled text field, an inline check box, a labelled menu and a fieldset of them, the model `FormItem` and the macro `item` in `changes.html`, which the changes filter form (group 7) introduced. `Special:AllPages`, `Special:PrefixIndex` and the redirecting pages' parameter form (group 8) draw their fields with it rather than with a `forms.html` of their own, since their markup is the same; the groups still to come (9–11) extend it where a control is missing rather than adding a parallel set. The chapter text is 19 §5.10.

### A8. One vocabulary of controls, laid out as a field or a row

- **Date:** 2026-11-01
- **Source:** Direct: James, design discussion of 2026-11-01
- **Change:** extends §9 and A7
- **Summary:** Group 9 brought the forms that edit a value of a registry type (preferences now, the wiki's and the instance's settings in group 10, the moderation forms in group 11), whose controls are drawn from the type with no label of their own, a row around each carrying the label, the key, the help, the row's errors and **Reset**. Claude first kept these controls apart from A7's labelled fields of the forms that work by `GET`, since their markup differed and merging them would change Plain's DOM; James: "It's fine to change Plain's DOM at this time unless there are other reasons to keep them apart." There were none, so Plain has one vocabulary of controls, the model `ControlModel` and the macro `control` in `forms.html` (a checkbox, which may carry its own label and value; a select; a number or text input; a text area; a value shown), and two layouts that place them: `item`, a `FormItem` of a `GET` form (a control under the label that wraps it, a checkbox with its label, a fieldset of items), and `setting`, a row of the preferences and settings forms. The typed controls keep the markup of the built-in `controls::render`, which the pages of groups 10 and 11 are compared against until they switch; the `GET` forms' fields, whose groups had switched, changed instead: a text field gains Codex's `cdx-text-input` wrapper, and the classes `ts-changes-form__field` and `ts-changes-form__kinds` became `ts-form-field` and `ts-form-group` (the old names stay in the stylesheet for the built-in pages of groups 11 and 12). The chapter text is 19 §5.10.

### A9. A radio button joins the controls, and a settings row carries where its value comes from

- **Date:** 2026-11-02
- **Source:** Direct: James, confirmed 2026-11-03; Claude's call while building the settings pages' templates (group 10), 2026-11-02
- **Change:** extends A8
- **Summary:** The wiki's and the instance's settings pages needed three things A8's vocabulary lacked, and each was added to it rather than drawn on the page. A **radio button** (the Visibility section's anyone, accounts on this wiki or a group) is a control of its own. A checkbox's or radio button's **own label** points at the control's ID where it has one, Codex's markup and the settings pages' (the **Lock** checkbox of Defaults for wikis), and wraps the control where it has none, as the `GET` forms' checkboxes do. The **settings row** carries the origin chips after the label (set on this wiki, inherited, locked, capped), **Lock** after the control, the locked chip after a value shown, and a help without an ID where no control points at it. One built-in difference was settled the shared layout's way before the comparison: Defaults for wikis drew a row's errors before its help and every other row after it, and now draws them after. The chapter text is 19 §5.10.

### A10. A field and an option join the layouts, and the moderation forms move onto them

- **Date:** 2026-11-03
- **Source:** Direct: James, confirmed 2026-11-03; Claude's call while building the moderation pages' templates (group 11), 2026-11-03
- **Change:** extends A8 and A9
- **Summary:** The moderation forms (Protect, Delete, Undelete, Block, Unblock, Move, UserRights and the sets of ProtectedPages) post, and lay their controls out as Codex's field does, the label above and the help under, which neither of A8's layouts is. Plain gains three layouts in `forms.html` over the same controls: `field`, a control under its label with its help (`FieldRow`); `check`, a checkbox carrying its own label with its help under it (`CheckRow`); and `ask`, a form that asks by `GET` for the page to go to, one `item` and its button (`AskForm`). The controls gain what these forms need: a checkbox may be disabled, shown rather than hidden where the viewer may not change it, and may carry a note at the end of its label (a membership's expiry, `ts-control__note`); an input may be required. Three built-in differences were settled the shared way before the comparison: the Block form's options and the UserRights groups were checkboxes without Codex's inline modifier, and are now inline as every other; the expiry note's class `ts-userrights__expiry` became `ts-control__note`; and the forms of Special:Undelete and Special:MovePage that ask for a page draw A8's `GET` field, the class `ts-form-field` and Codex's `cdx-text-input` wrapper. The deleted page's view, Special:ProtectedPages, Special:ProtectedTitles and Special:BlockList, whose code is group 11's, count in group 11 rather than 5 and 8. The chapter text is 19 §5.10.

### A11. A line of text is parts, and the last group's forms move onto the shared controls

- **Date:** 2026-11-03
- **Source:** Direct: James, confirmed 2026-11-03; Claude's call while building the last group's templates (group 12), 2026-11-03
- **Change:** extends §9, A6 and A10
- **Summary:** Group 12's pages (Special:Version and its subpages, AllMessages, the four information pages, Providers, BlankPage, ComparePages, NewItem and NewProperty, the term forms, Special:Account, the generated main page) built sentences and table cells whose text holds links, code and line breaks as HTML, and the Codex table's model carried a cell as HTML. A view model now carries such a line as **parts** (`Inline`: text, a link on or off the site, code, monospace text, a line break, a list that opens on demand), drawn by Plain's `parts` macro in `inline.html`; a message whose parameters are links is filled with parts rather than with HTML, a list in the reader's language is joined from parts, and a table cell is parts. A page that says one thing, a refusal, a title with no page or a call that failed, is the skin's `message.html` over a Codex message, the frame's own failure page included. The external issuers' buttons, carried as HTML on the login and creation pages until this group (A6), are a model and `account.html`'s `issuers` macro, which Special:Account draws too; no field of A6's kind remains. An input may carry `inputmode` (ComparePages' revision fields, numeric). Three built-in differences were settled the shared way before the comparison: the forms of AllMessages and ComparePages draw A8's `GET` field, the class `ts-form-field` and Codex's `cdx-text-input` wrapper, without `size`; and NewItem's and NewProperty's prefilled statements are inline checkboxes with their own label, as every other. The chapter text is 19 §5.10.

### A12. The built-in renderer is removed, and parity becomes Worksheet's check

- **Date:** 2026-11-03
- **Source:** Direct: James, confirmed 2026-11-03; Claude's call while removing the built-in renderer, 2026-11-03
- **Change:** amends §16; extends §9, A3 and A6
- **Summary:** Group 12 passed the gates of §16 with the built-in builders still in place, and switched: every page is drawn by the skin, and the built-in renderer is removed, its string builders, the Codex components' `askama` templates and the `askama` dependency (the components keep their models, which Plain's macros draw, and `tests/codex_macros.rs` holds each macro's markup as a snapshot in place of the builder it was compared with), with the deployment settings `server.renderer` and `web.renderer` (`--renderer`, `TRIPLESPACE_RENDERER`, `TRIPLESPACE_WEB_RENDERER`) and the end-to-end suite's run per renderer. The parity harness and `cargo xtask skin-parity` are kept over the same corpus as the check that Worksheet is Plain with its stylesheets: each case drawn with both skins, Worksheet's stylesheet links taken out, compared as A3 compares; the render times are reported as a watch rather than judged against a budget, since there is no built-in time to judge them by. No part of a page is carried as HTML for a later group (A6). The chapter text is 19 §5.10, 20 §1.2 and 23 §3.6.

Replaced text: 19 §5.10.

Replaced text: 19 §5.2; 20 §1.2 and 23 §3.6.

### A13. Plain's message bundle: which messages are a skin's

- **Date:** 2026-11-15
- **Source:** Direct: Claude, while building per-skin message bundles (SCA-327), 2026-11-15; confirmed by James, 2026-11-15
- **Change:** extends §8, §13
- **Summary:** A skin's bundle is built, with these calls. (1) **A skin's messages are the ones its own templates name with `msg()` and nothing else reads.** What the site's Rust words, the view models every skin draws, stays the binary's, as do the messages a script is handed and the ones a key is built for. By that rule six messages are Plain's and moved, renamed from `ts-` to `plain-`: the skip link, the footer line, the names of the site navigation and of the tabs, the tabs' overflow label and the Julian mark. The other reading, that every message of the site is Plain's and only errors, notifications, mail and API text are the binary's, would rename some two thousand keys; it is James's to choose, and `tools/skin_messages.py` is where the rule lives. (2) The bundle's version is `[messages] version` in `skin.toml`, 0.1.0 for Plain. (3) A skin is not loaded when a key of its bundle does not open with its name and a hyphen, when it has messages and gives no version, or when it extends another skin and brings messages or a `[messages]` table. (4) A skin's templates read the skin's bundle beside the binary's through `msg()`; a skin that extends Plain reads Plain's. (5) A wiki's Interface page overrides a skin's message by its key, and `meta=allmessages` lists the shipped skins' messages among the defaults; an installed skin's are not listed, since the API does not know the web tier's skins. (6) `banana-checker` checks Plain's bundle with the binary's. Chapter text: 19 §5.6, §5.10.
