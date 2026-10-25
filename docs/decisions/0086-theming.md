# 0086. Theming: palettes, type sets and modes

- **Status:** Proposed
- **Date:** 2026-10-17
- **Updated:** 2026-10-25 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0015](0015-record-format-and-partition-registry.md), [0018](0018-tenants.md), [0034](0034-frontend-stack.md), [0039](0039-files-and-media.md), [0040](0040-instance-prerogatives.md), [0055](0055-templatestyles-templatedata-and-page-properties.md)
- **Uses:** [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [0077](0077-special-version.md), [0085](0085-settings-and-preferences.md)

## Context

[0034](0034-frontend-stack.md) A7 kept Codex for its accessible components and let the instance or a tenant give Codex's design tokens values of its own, as one flat theme in the `ui.theme` setting, checked for WCAG 2.1 AA contrast and served as a stylesheet named by its hash. A8 shipped the canvases' colours with Newsreader headings over IBM Plex Sans and IBM Plex Mono as the default, `default` in `docs/registry/themes.toml`. That theme had one set of colours, so it had no dark mode ([0034](0034-frontend-stack.md) Q2), no high-contrast mode, and no choice for the reader, and colours and typefaces could not be chosen apart.

Codex 2.7.0 ships the tokens a dark mode needs (`theme-wikimedia-ui-mode-dark.css`) and the text-size modes MediaWiki's reading preferences use (`mode-small`, `mode-large`, `mode-x-large`). Wikipedia's night mode has taught its editors a convention for content: TemplateStyles rules keyed on `html.skin-theme-clientpref-night` and `html.skin-theme-clientpref-os`, and the classes `notheme`, `skin-invert` and `skin-invert-image`; mirrored and forked pages ([0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md)) bring that convention with them.

[0085](0085-settings-and-preferences.md) settled the machinery this ADR uses: the settings registry and how a value resolves from instance to tenant, preferences that resolve from the reader down to the farm, the Appearance menu and how a reader who is not logged in keeps choices in the browser, and site assets, the files that are public because the site needs them, such as a typeface.

### Direction

James's direction, from the design discussion of 2026-10-16 and 2026-10-17:

- **"It would be great if an instance, or a tenant on an instance, or a user, can specify a preferred palette and font set. In terms of palettes, you can have different ones for light mode, dark mode, accessibility, and other things for customization. Different instances/tenants/users will also want different font sets depending on branding, accessibility, and other needs."**
- **"Yes, palette and type set are separate choices. Palette with no dark variant falls back to the default dark mode. Accessibility choices cannot be locked. I would defer on personal palettes. Yes, ship a dark variant. I want the current palette Triplespace has to be called "parchment"."**
- **"For the theming ADR, palette and type-set definitions live in config records. The default type-set should be called "default26" and I think the dark variant should be some kind of brown; you can fill in the gaps from there."**
- **"Palette and type set are per-wiki unless the user specifies they want it to follow across all wikis."**
- On the walnut-brown dark variant, shown beside the light one: **"The color scheme looks good to me. You may draft when ready."**
- On accessibility: **"What if instead of variants for color-vision deficiencies we had specialized palettes? I do like including a dyslexia type set. Atkinson Hyperlegible is SIL OFL 1.1."**
- **"I would call the alternative palette for those with red-green deficiencies "parchment-ob", named after the orange and blue used instead. … As for type, I would start with opendyslexic and add other options later."**
- On the blue–yellow palette, shown simulated beside `parchment` and `parchment-ob`: **"The colors are good and parchment-co is a good name."**
- On the draft: **"Thank you for adding the extends rule; it's a good way of implementing variants. You are right that color cannot be the only cue. ui.theme can be retired with no migration. Palette and type set should be like the logo, favicon, fonts, etc. in that they are inherently public."**
- **"Your font licence allow list is good. Yes, widen the security policy to allow fonts and images from the media domain. Wiki palettes and type sets cannot have names which conflict with the instance's. Palettes and type sets are not changeable by users or instance operators."**

## Decision

### 1. Palettes and type sets are configuration (amends 0034 §1; extends 0015 §3, 0012 §5, 0011 §6.1, 0018 §10 and 0040 §6)

*Changed by A1, A2.*

**A theme becomes two choices**: a **palette**, which sets Codex's colour tokens per variant (§2) and the shape tokens (radii and spacing), and a **type set**, which sets the typeface tokens and serves the typefaces (§3). Neither sets a component's markup or behaviour.

**Both are config records.** The kinds are `palette`, keyed `palette:{name}`, and `typeset`, keyed `typeset:{name}`, at instance or tenant scope, written with `ts-config`. `docs/registry/themes.toml` holds the shipped ones in the same shape and is copied into an instance's `config` and synced like every registry file ([23](../architecture/23-configuration-and-registry.md) §4.1). A tenant offers the registry's, the instance's and its own. **A name means one thing on an instance**, as James decided: a tenant may not write a palette or type set under a name the registry or the instance uses, and the instance may not write one under a name the registry or any of its tenants uses, so a reader's choice never names two different records. **The shipped palettes and type sets are fixed**: neither a reader nor an operator changes them, as James decided; only `registry sync` writes their records, so `parchment`, the **default palette**, always has all four variants, and the accessibility palettes and type sets cannot be withdrawn. A palette or type set that `theme.palette` or `theme.typeset` names cannot be deleted. A record is validated when it is written: every key a token Codex defines and of the right family (colour tokens in a palette's variants, radius and spacing tokens on the palette, `font-family-*` tokens on a type set), every value a plain CSS value, the name free (above), `extends` naming a palette it may extend (§2), and every variant meeting its contrast floor (§2); a record that fails is refused with `ts-invalid-setting`.

**A name taken later is keyed**, as James decided (A1). A tenant's palette or type set whose name the instance or the registry comes to use is renamed `{name}~{slug}`, keyed with the tenant's slug ([08](../architecture/08-tenants-and-instances.md) §1.2), as `foo` becomes `foo~librarybase`; the instance's own, which only the registry can take from it, is keyed with the farm slug. The bare name goes to the instance or the registry. A name is taken later in three ways:

- **A tenant joins** an instance that uses one of its names, or whose registry does: the join keys the tenant's ([08](../architecture/08-tenants-and-instances.md) §6.3).
- **The instance writes** a palette or type set under a name a tenant uses: the write is refused with `ts-invalid-setting`, naming the tenants, until the operator either keys theirs, by repeating the write with `key=tenants` (`--key-tenants` in the CLI, a confirmation on the settings page), which keys them and writes the instance's in one act, or chooses another name.
- **A release's `themes.toml` adds** a name the instance or a tenant uses: `registry sync` keys theirs and is never refused for it, since a shipped name is fixed and every wiki must offer the accessibility palettes and type sets.

**Keying renames every reference in the same act**, so no setting or choice changes what it shows: the record is written under its keyed name and the old one deleted, and with it the `theme.palette` and `theme.typeset` values that name it (the tenant's own, and the instance's defaults and locks), the `extends` of the palettes that extend it, and the `ui.palette` and `ui.typeset` preferences of tenant and farm accounts. In a tenant, keying is a written provision ([08](../architecture/08-tenants-and-instances.md) §8.6): its records carry the instance attestation, its authority is a job record in the instance `log` that lists the renames, and the tenant may later rename the record to any name that is free. **`~` is reserved for keying**: no one writes a new name containing it, and a keyed record is updated and deleted under its keyed name. Once written, a keyed name is a name like any other: it stays when the tenant leaves, and is keyed again, its key replaced by the tenant's slug there, only if it is taken where the tenant arrives.

**Writing them** goes through `PUT` and `DELETE /palette/{name}` and `/typeset/{name}` on REST v0, read with `GET /palettes` and `GET /typesets` by anyone, a private tenant's outsiders included (§5), with `scope=instance` at the farm base as for settings ([0085](0085-settings-and-preferences.md) §3), and through a **Palettes and type sets** tab of the settings pages, which previews each variant. A write is one change set that projects as a `settings/change` event naming the record.

**What ships:** the palette **`parchment`**, the canvases' colours named on 2026-10-17, with a walnut-brown dark variant and high-contrast variants of both; the accessibility palettes **`parchment-ob`** and **`parchment-co`**, for colour-vision deficiencies (§2); the type set **`default26`**, Newsreader, IBM Plex Sans and IBM Plex Mono; and the accessibility type sets **`hyperlegible`** and **`opendyslexic`** (§3). Beside them, as options a wiki or a reader may choose and not as defaults, ship the palette **`encyclopedia`** and the type set **`basic`**, inspired by Vector 2022, the skin of Wikipedia, as James decided (A2): `encyclopedia` is Codex's own colours, which Vector 2022 wears, changed only where they miss the floors of §2, with high-contrast variants in Codex's hues, and `basic` is Vector 2022's default stacks on Wikimedia wikis, serving no typeface (§3).

**`ui.theme` is retired.** Its flat token map is replaced by the records above and the settings of §4; the setting stays in `settings.toml` as retired, and `instance check` warns where a record of it remains.

### 2. Variants and contrast (amends 0010 §2; settles 0034 Q2)

A palette has up to four variants, each a set of colour tokens:

| Variant | Overlays | Text pairs | Control boundaries and focus |
|---|---|---|---|
| `light` (required) | Codex's light values | 4.5:1 (WCAG 2.1 AA) | 3:1 (WCAG 1.4.11) |
| `dark` | Codex's dark values (`theme-wikimedia-ui-mode-dark`) | 4.5:1 | 3:1 |
| `light-more` | the palette's `light` | 7:1 (WCAG 1.4.6, AAA) | 4.5:1 |
| `dark-more` | the palette's `dark` | 7:1 | 4.5:1 |

**A variant is checked as it is served**, merged over Codex's values for its mode, so a colour the palette leaves to Codex is checked too. The pairs are those `triplespace-ui` checks today (text, subtle and placeholder text on the page and on subtle panels, links and visited links, the warning chip, links on the progressive tint, control borders and the focus outline), and three more groups: button labels, which Codex draws in `color-inverted-fixed`, white in every mode, on `background-color-progressive` with its hover and active states and on `background-color-destructive`; and the error, destructive and success colours on the page and on their subtle backgrounds.

**Fallbacks.** A palette without `dark` takes the default palette's `dark` in full, so a branded palette never turns dark mode off, as James decided. A palette without `light-more` or `dark-more` takes the default palette's, so every wiki has a high-contrast mode whatever its brand.

**A palette may extend another** (`extends`): each of its variants is laid over the same variant of that palette as served, rather than over Codex's, so it states only the colours it changes; its `light-more` and `dark-more` take its own `light` and `dark` first, and a variant missing from both falls back as above. A palette may extend a shipped palette or one at its own scope, and not one that itself extends another; a palette that another extends cannot be deleted.

**Accessibility palettes.** A palette marked `accessibility = true` can always be chosen by a reader (§4). In place of colour-vision variants of every palette, James chose palettes made for a deficiency, which a reader chooses as they would any palette. Two ship, both extending `parchment` and changing only the colours that carry meaning (errors and destructive actions, success, warnings, visited links, and the subtle backgrounds of each), with all four variants. Their colours were chosen by simulating the deficiency they serve (Machado, Oliveira and Fernandes, 2009) and are held to the same floors as any palette:

- **`parchment-ob`**, for red–green deficiencies (protanopia and deuteranopia), named for the orange and blue it uses: errors, destructive actions and removed text in vermilion (`#A84300` in `light`), success and added text in cerulean (`#087691`), kept apart from link blue, warnings in umber and visited links in plum. Its lowest text pairs are 4.62:1 and 4.63:1 in `light` and `dark`, and at least 7.15:1 in the high-contrast variants.
- **`parchment-co`**, for blue–yellow deficiency (tritanopia), named for its crimson and olive: errors in crimson (`#B3263E`), success in olive (`#587114`), which a tritanope tells from link blue, and warnings in an umber they do not confuse with errors. Its lowest text pairs are 4.85:1 and 4.63:1, and at least 7.17:1.

**Colour is never the only cue** (WCAG 1.4.1): what the site draws tells states apart by text, icon or mark as well, such as the `−` and `+` of a diff beside `color-destructive` and `color-success` on their subtle backgrounds, so an accessibility palette makes the colour a cue the reader can see, not the only one. A wiki that wants its brand in a colour-vision palette writes one that extends its own and marks it `accessibility`.

**Everything the site draws takes its colours from tokens**, so every variant reaches it: diffs, charts, timelines, the sync strip, constraint marks and chips. A test asserts that `triplespace-ui`'s templates and stylesheets hold no colour literal outside the theme. **Provider chips**, whose colours come from `providers.toml` ([0010](0010-site-ui.md) §2), swap their two colours in the dark variants: the pale background becomes the text and the dark text the background, which keeps the pair's contrast exactly; a provider entry may give `chip_color_dark` and `chip_background_dark` where the swap does not suit it.

**`parchment`'s dark variant** is walnut brown: page `#1E1813`, subtle panels `#271F19`, text `#EDE4D6`, subtle text `#BBAD99`, links `#9DB6EA`, visited links `#C5B3EC`, control borders `#8E7E6A`, and primary buttons a mid blue, `#4C73C2`, under white labels. Its lowest text pair is that button label, 4.6:1, and its lowest boundary 4.47:1; in the high-contrast variants every text pair is at least 7.1:1 and every boundary at least 6.8:1. Every value is in `themes.toml`.

### 3. Type sets (extends 0039 §5)

*Changed by A2.*

A type set gives three roles, `font-family-base`, `font-family-heading-main` and `font-family-monospace`, each a stack that ends in generic families, and lists the typefaces it serves:

- **A shipped typeface** is a build asset, served from the site's own origin as [0034](0034-frontend-stack.md) §11 requires, and listed with its licence in `Special:Version` ([0077](0077-special-version.md)).
- **An instance's or tenant's typeface** is a set of font files, each named by the record as a **site asset** ([0085](0085-settings-and-preferences.md) §8): a `File:` pinned by SHA-1, WOFF2 or WOFF (both join the permitted file types, [0039](0039-files-and-media.md) §5), with its family, weight and style, and its licence, which must be `OFL-1.1`, `Apache-2.0` or `UFL-1.0`. A tenant's type set names the tenant's files; an instance's names the primary tenant's ([08](../architecture/08-tenants-and-instances.md) §7). Being named by a type set is what makes the file public, for as long as it is named. The licence is declared by whoever writes the record and shown with the file in the wiki's **Public files**; it is not verified against the font (Q6). Fonts are fetched across origins with CORS, so the site-asset route answers them with `Access-Control-Allow-Origin: *`; they are public already.
- **Per-language stacks**: a type set may map language tags to stacks (`ar`, `hi`, `zh-Hans`), set as `font-family` directly on elements matching `:lang()`, so they follow the language of the text itself, interface or content, and a wiki whose typeface lacks a script names one that has it rather than leaving it to whatever the reader's device holds.
- **`accessibility = true`** marks a type set chosen for legibility, as it marks a palette (§2). Such a type set can always be chosen by a reader (§4). `hyperlegible` ships as one: Atkinson Hyperlegible, by the Braille Institute, under the SIL OFL 1.1, for text and headings, with IBM Plex Mono. `opendyslexic` ships as another: OpenDyslexic, by Abbie Gonzalez, under the SIL OFL 1.1, with OpenDyslexic Mono for code, the first of the type sets for readers with dyslexia James intends (Q7). Studies have not shown that it speeds reading; it ships because readers who prefer it ask for it by name.

**A type set may serve no typeface** and name only families a device may have, ending in generic ones, as `basic` does (A2): it gives MediaWiki core's stacks for Vector 2022, `sans-serif` for text, `'Linux Libertine', 'Georgia', 'Times', 'Source Serif Pro', serif` for headings and `monospace, monospace` for code, so it looks on each device as Wikipedia does there and costs no download.

**Text size is not part of a type set.** It is the reader's choice of Codex's text-size modes (§4), so it applies whatever type set is worn.

### 4. Who chooses (uses 0085 §1)

**Settings** ([23](../architecture/23-configuration-and-registry.md) §3.2), resolved as every setting is, with instance defaults and locks:

| Setting | Values | Default |
|---|---|---|
| `theme.palette` | a palette the tenant offers | `parchment` |
| `theme.typeset` | a type set the tenant offers | `default26` |
| `theme.choice` | `any`, `accessibility` | `any`: readers may choose any palette and type set offered; `accessibility`: only the accessibility palettes and type sets and the choices below |

**Preferences** in the `appearance` group, all `browser`, so a reader who is not logged in may set them through the Appearance menu ([0085](0085-settings-and-preferences.md) §6):

| Key | Values | Default | Follows |
|---|---|---|---|
| `ui.mode` | `auto`, `light`, `dark` | `auto`: the device's `prefers-color-scheme` | farm |
| `ui.contrast` | `auto`, `more` | `auto`: the device's `prefers-contrast` | farm |
| `ui.text-size` | `small`, `medium`, `large`, `x-large` | `medium` | farm |
| `ui.palette` | a palette the tenant offers | none: `theme.palette` | choice |
| `ui.typeset` | a type set the tenant offers | none: `theme.typeset` | choice |

**Accessibility choices are never locked.** `ui.mode`, `ui.contrast`, `ui.text-size` and the accessibility palettes and type sets are offered whatever `theme.choice` and any instance lock say; only the other palettes and type sets, which carry a wiki's brand, can be withheld. A reader who chooses an accessibility palette leaves the wiki's own palette for it.

**`follows = "choice"`** joins `account` and `farm` in `preferences.toml` ([0085](0085-settings-and-preferences.md) §1): the value is the tenant account's, and the reader may mark it **Use on every wiki**, which stores it on the farm account and clears the tenant account's, as James decided for palette and type set. A tenant account's own value is read before the farm account's. A palette or type set is chosen by name and resolved in each wiki's own catalogue; a name the wiki does not offer, or has deleted, resolves as unset there, so the wiki's default applies. Without farm identity, `choice` is `account`.

**Locks and `preferences.defaults`.** An instance lock on `theme.palette` or `theme.typeset` fixes the wiki's default; what readers may choose is `theme.choice`'s alone. `ui.palette` and `ui.typeset` have no entry in `preferences.defaults`, since the wiki's default is the setting.

### 5. Serving (amends 0034 §1; extends 0034 §11; amends 0012 §4)

**One stylesheet per palette and one per type set**, so their number grows as palettes plus type sets, not as their product. Each is named under `/ui/theme/` by the record's offset and the hash of its text, cached as immutable, and rebuilt from the log for any earlier offset, so a cached page never loses its styles when a palette changes.

- **A palette's stylesheet** holds `light` and the shape tokens on `:root`; `dark` under `:root[data-mode=dark]` and, within `@media (prefers-color-scheme: dark)`, under `:root:not([data-mode=light])`; `dark-more` where dark is in effect and `:root[data-contrast=more]` or, within `@media (prefers-contrast: more)`, `:root:not([data-contrast=normal])`; and `light-more` likewise where light is in effect. Each variant applies only in its own mode.
- **A type set's stylesheet** holds the three roles, the per-language stacks, the `@font-face` rules (`font-display: swap`) and Codex's text-size modes under `:root[data-text-size=…]`.

**The reader's choices are attributes on `<html>`**: `data-mode`, `data-contrast` and `data-text-size`, written by the server for a signed-in reader, whose page is private, and by the head script of [0085](0085-settings-and-preferences.md) §6 for a reader who is not logged in, whose page stays public and carries the tenant's default stylesheet. MediaWiki's classes are set beside them, `skin-theme-clientpref-night` for a chosen dark mode, `skin-theme-clientpref-os` for `auto` and `skin-theme-clientpref-day` for a chosen light mode, so content written for Wikipedia's night mode works (§6); a public page is served with `skin-theme-clientpref-os` and no `data-mode`, so it follows the device until the head script says otherwise. Where a reader not logged in has chosen another palette or type set, the head script swaps the stylesheet links before the first paint, from a JSON block on the page that maps the tenant's palettes and type sets to their stylesheet URLs; the default sheets may already be in flight, a cost of one cached request. **Without JavaScript**, the media queries alone follow the device's dark-mode and contrast settings.

**The security policy admits the media origin**, as James decided: `font-src` and `img-src`, today `'self'` and `'self' data:`, add the origin of `media.base_url` where it differs from the site's, so a tenant's typefaces and logo, and its files in content, load from it; scripts and styles stay `'self'` ([0034](0034-frontend-stack.md) §11).

**`siprop=triplespace` reports the theme**: the palettes and type sets the tenant offers (name, label, which variants a palette has and the palette it extends, whether a palette or type set is for accessibility), `theme.palette`, `theme.typeset` and `theme.choice`, and the stylesheet URL of each offered palette and type set, in place of `ui.theme`'s token map. **Palettes and type sets are public**, as James decided, like the logo, the favicon and the typefaces ([0085](0085-settings-and-preferences.md) §8): an outsider of a private tenant is given the theme in full, the records, the settings and the stylesheets, which `/ui/theme/` serves to anyone. They are what the pages an outsider can reach are made of, and a reader who needs an accessibility palette or type set needs it on the landing page and the login form before signing in.

### 6. Content written for night mode (amends 0055 §2)

**`scatter-css` keeps the night-mode selectors that TemplateStyles keeps.** A selector whose first compound is `html` with only the classes `skin-theme-clientpref-night`, `skin-theme-clientpref-os` or `skin-theme-clientpref-day` keeps that compound before the `.mw-parser-output` prefix, so `html.skin-theme-clientpref-night .infobox` becomes `html.skin-theme-clientpref-night .mw-parser-output .infobox`, and works under the classes of §5. Every other selector that escapes the prefix (`html`, `body`, `:root`) still matches nothing.

**The frame honours Wikipedia's content classes** in the dark variants: `notheme` keeps an element's own light colours, on a light surface; `skin-invert` and `skin-invert-image` invert an element or image (`filter: invert(1) hue-rotate(180deg)`), for diagrams drawn black on white. Colours written inline in content (`style="background:#f8f9fa"`) are outside any theme's reach; a mirrored page shows them as its authors wrote them, as Wikipedia does.

### 7. Crates (extends 0005 §2)

*Changed by A2.*

| Crate | Change |
|---|---|
| `triplespace-accounts` | The `palette` and `typeset` config kinds, their validation, `extends` and contrast floors; `follows = "choice"` |
| `triplespace-ui` | Reads palettes and type sets from `themes.toml` and the API; a stylesheet per palette with its variants and per type set with its `@font-face` rules and text-size modes; the `<html>` attributes and classes; the stylesheet data block; the Appearance controls for the `ui.*` preferences; the Palettes and type sets tab; the content classes of §6 |
| `scatter-css` | The night-mode selector rule of §6 |
| `triplespace-api-action`, `triplespace-api-rest` | `siprop=triplespace`'s palettes, type sets and theme settings; the palette and type-set routes |
| `triplespace-files` | WOFF2 and WOFF uploads; `Access-Control-Allow-Origin` on font assets |

The registry part of §1 is in place: `themes.toml` holds `parchment` with its four variants, the accessibility palettes `parchment-ob` and `parchment-co`, which extend it, the palette `encyclopedia` (A2), and the type sets `default26`, `hyperlegible`, `opendyslexic` and `basic` (A2), and `triplespace-ui` reads the default pair from it. The other palettes and type sets are not yet served, and the Atkinson Hyperlegible and OpenDyslexic files are not yet in the build.

## Alternatives considered

- **Palettes and type sets as pages**, with a JSON content model, would give them history, diffs and talk pages. James chose config records: a palette is configuration, its history is the config log, and records need no page stack.
- **Codex's dark tokens as they are**, as the dark mode of every palette. They are cool grey; a warm palette in light mode turning grey in dark mode reads as a different site. They remain the base a palette's `dark` overlays.
- **A dark mode derived by computation**, inverting lightness in OKLCH. It passes contrast checks unpredictably and gives a brand no say; a palette states its dark colours, and one that does not takes the default's.
- **Colour-vision variants of every palette**, beside `light` and `dark` (Q1 as first drafted). Every branded palette would need them or borrow the default's, and a reader would choose a deficiency and a palette apart. James chose palettes made for a deficiency: one choice, which a wiki may extend for its brand.
- **Text size inside the type set.** A reader who needs larger text needs it whatever the wiki's typeface is.

## Consequences

- **Every wiki has a dark mode and a high-contrast mode**, its own or the default palette's, and every reader can reach both, logged in or not, with or without JavaScript.
- **A reader with a colour-vision deficiency or dyslexia has a palette or type set for it on every wiki**, which no instance or tenant can withhold; where they choose one, it takes the place of the wiki's brand.
- **A wiki's brand is two records and two settings.** An instance can set or lock the defaults for every wiki through [0085](0085-settings-and-preferences.md) §2.1.
- **A private wiki's palettes, type sets, stylesheets and typeface files are public**, as its logo is: they are what its landing page is made of, and a wiki that would keep a palette's colours or a typeface secret has to keep them off the wiki.
- **Content written for Wikipedia's night mode renders as its authors intended**, through the classes MediaWiki uses.
- **Every colour the site draws is a token**, which a test enforces; a design that needs a new colour needs a token for it in every variant.
- **`ui.theme` records written before this ADR stop applying**, without migration, as James decided; `instance check` names them.
- **A palette or type set can be renamed under a wiki by its instance**, to `{name}~{slug}`, when the instance or a release takes its name; every setting and reader's choice that named it is rewritten with it, and the wiki's history shows the instance's act (A1).
- **Pages written without night mode in mind read poorly in the dark variants** where they colour backgrounds inline: light backgrounds under the dark variant's light text. Wikipedia has the same problem; `notheme` and night-mode TemplateStyles are the remedies its editors use.

## Open questions

- **Q1.** ~~**Named variants beyond the four**, such as a palette for a colour-vision deficiency, and how a reader would choose one.~~ *Withdrawn: James chose accessibility palettes on 2026-10-17 (§2).*
- **Q2. TemplateStyles' exact rule** for `html`-prefixed selectors, to be checked against css-sanitizer's source so §6 keeps what TemplateStyles keeps and no more.
- **Q3.** ~~**A type set for readers with dyslexia**, and which typeface, given the mixed evidence for dyslexia-specific typefaces and their licences.~~ *Settled by §3: James chose on 2026-10-17 to ship `opendyslexic` first; further options are Q7.*
- **Q4.** ~~**Bundling Atkinson Hyperlegible**: the licence of the files added to the build is to be confirmed as OFL-1.1 when they are added, and Atkinson Hyperlegible Next considered.~~ *Withdrawn: James confirmed on 2026-10-17 that Atkinson Hyperlegible is under the SIL OFL 1.1.*
- **Q5. Personal palettes**, written by a reader for themselves, deferred by James.
- **Q6. Verifying a font's licence** against the licence fields inside the font file, rather than trusting the record's declaration.
- **Q7. Further type sets for readers with dyslexia**, such as Lexend, and whether a type set may set letter, word and line spacing, which the British Dyslexia Association's style guide stresses more than the typeface.
- **Q8.** ~~**A name taken by another level later**: a release whose `themes.toml` adds a name an instance or tenant already uses, or a tenant joining an instance that uses one of its names. Proposed: `registry sync` and the join are refused with the colliding records named, so the operator renames them first, since renaming a record silently would change what `theme.palette` and readers' preferences mean.~~ *Settled by A1: the colliding tenant's record is keyed `{name}~{slug}`, or the instance's with the farm slug when the registry takes its name, with every reference rewritten in the same act; an instance write of a name a tenant uses is refused until it keys the tenant's or chooses another name.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §7 | extends | 0005 A96 |
| [0010](0010-site-ui.md) §2 | §2 | amends | 0010 A42 |
| [0011](0011-logs.md) §6.1 | §1 | extends | 0011 A32 |
| [0012](0012-api-requirements.md) §4 | §5 | amends | 0012 A76 |
| [0012](0012-api-requirements.md) §5 | §1 | extends | 0012 A76 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §1 | extends | 0015 A60 |
| [0018](0018-tenants.md) §10 | §1 (A1) | extends | 0018 A30 |
| [0034](0034-frontend-stack.md) §1 | §1, §5 | amends | 0034 A12 |
| [0034](0034-frontend-stack.md) §11 | §5 | extends | 0034 A12 |
| [0039](0039-files-and-media.md) §5 | §3 | extends | 0039 A14 |
| [0040](0040-instance-prerogatives.md) §6 | §1 (A1) | extends | 0040 A8 |
| [0034](0034-frontend-stack.md) Q2 | §2 | settles | 0034 Q2 |
| [0055](0055-templatestyles-templatedata-and-page-properties.md) §2 | §6 | amends | 0055 A4 |

## References

- Codex design tokens 2.7.0: `theme-wikimedia-ui.css`, `theme-wikimedia-ui-mode-dark.css`, and the text-size modes.
- MediaWiki's night mode and its client preferences: the `skin-theme-clientpref-*` classes, and the `notheme`, `skin-invert` and `skin-invert-image` classes for content.
- WCAG 2.1: 1.4.3 (contrast, minimum), 1.4.6 (contrast, enhanced), 1.4.11 (non-text contrast).
- Atkinson Hyperlegible, Braille Institute of America.

## Amendment log

### A1. A name taken later is keyed

- **Date:** 2026-10-18
- **Source:** Direct: James, design discussion of 2026-10-18
- **Change:** amends §1
- **Summary:** Where a palette's or type set's name comes to collide across levels, the colliding record is renamed rather than the change refused, which settles Q8. James's direction: "if an instance has a pre-existing palette or type set "foo", and a tenant with their own "foo" wants to be imported, the conflict would resolve by keying the name to the tenant e.g. "foo~tenantname". If an instance wants to introduce a new palette or type set with a name in use by a tenant, it is blocked until the instance renames it with the key or gives it a new name altogether." James then chose, on 2026-10-18: the key is the tenant's slug, and a keyed name stays with the tenant when it leaves; when the instance takes a name a tenant uses, it is the tenant's record that is keyed and the instance takes the bare name; `registry sync` keys a colliding instance or tenant record, the instance's with the farm slug, rather than being refused; and keying in a tenant is a written provision that rewrites every reference to the record in the same act (`theme.palette` and `theme.typeset`, `extends`, and readers' `ui.palette` and `ui.typeset`). Claude proposed reserving `~` for keying, `key=tenants` as the way an instance write keys the tenants' records, and a job record listing the renames as the authority, which also maps the preferences of accounts reclaimed after a join. The join is in [08](../architecture/08-tenants-and-instances.md) §6.3 ([0018](0018-tenants.md) A30) and the written act in [08](../architecture/08-tenants-and-instances.md) §8.6 ([0040](0040-instance-prerogatives.md) A8).

Replaced text (§1):

> **A name means one thing on an instance**, as James decided: a tenant may not write a palette or type set under a name the registry or the instance uses, and the instance may not write one under a name the registry or any of its tenants uses, so a reader's choice never names two different records (Q8 for names that collide when the registry gains one or a tenant joins).

### A2. Encyclopedia and Basic, inspired by Vector 2022

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-24 and 2026-10-25
- **Change:** extends §1, §3, §7
- **Summary:** A palette and a type set inspired by Vector 2022 ship as options beside the defaults, which stay `parchment` and `default26`. James asked on 2026-10-24 for "a theme and type set option (not the default) inspired by Vector 2022", and on 2026-10-25, having seen it on the site: "I would like for the palette to be called "encyclopedia" and the type set "basic". It should use the same default font stack that Vector 2022 uses on Wikimedia wikis." The palette **`encyclopedia`** is Codex's light and dark values, which Vector 2022 wears, with only the tokens that miss the floors of §2 changed: `border-color-base` in `light` from `#A2A9B1` (2.37:1) to `#8C959F`, and in `dark` the placeholder (`#797F85`) and the focus outline (`#6485D1`), which Codex leaves at their light values; its `light-more` and `dark-more` deepen or lighten Codex's hues to 7:1 for text and 4.5:1 for boundaries, and its radius is Codex's `2px`. The type set **`basic`** is MediaWiki core's defaults as Vector 2022 uses them (`mediawiki.skin.defaults.less`, `mediawiki.skinning/elements.less`): `sans-serif` for text, `'Linux Libertine', 'Georgia', 'Times', 'Source Serif Pro', serif` for headings and `monospace, monospace` for code; it serves no typeface, which §3 now allows. Claude first drafted both as `vector22`, with Libertinus Serif served for headings; James chose the names and the stacks. The values are in `themes.toml`.
