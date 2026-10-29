# 0093. Fallback typefaces for every script

- **Status:** Proposed
- **Date:** 2026-10-28
- **Author:** James Hare / Claude Opus
- **Changes:** [0086](0086-theming.md)
- **Uses:** [0034](0034-frontend-stack.md), [0077](0077-special-version.md)

## Context

[0086](0086-theming.md) §3 gives a type set three stacks and the typefaces it serves, and lets it map language tags to stacks of their own, set on `:lang()` elements, "so a wiki whose typeface lacks a script names one that has it rather than leaving it to whatever the reader's device holds". The shipped type sets served Latin-script typefaces only, with per-language stacks for none, so any other script fell to the device.

On 2026-10-28 `default26` was set in Source Serif 4, Source Sans 3 and Lilex (0086 A4), chosen for their coverage: Latin, Greek, Cyrillic and Vietnamese. That leaves most of the world's scripts to the device. A Triplespace page often holds many scripts at once: a mirrored Wikidata item lists its labels, descriptions and aliases in every language it has, often more than a hundred, each tagged with its language (`lang`, and `dir="auto"`). Per-language stacks alone fit such a page poorly: a hundred tags, one rule each, in every type set.

What was known when this was decided:
- **Browsers fall back per character.** A stack's families are tried in order for each character, and a face declared with `unicode-range` is fetched only when the page has a character in its range that no earlier family has. A stack can name many families and cost a reader only the files for the scripts on the page, without any `lang` tag.
- **Noto covers almost every script** in families made to sit together on a line, all under the SIL OFL 1.1 and all on Fontsource, which the build already uses. The families called just Noto Sans and Noto Serif cover what Source covers, plus Devanagari in Noto Sans; every other script has its own family, such as Noto Sans Arabic or Noto Serif Georgian.
- **Wikimedia's language data** (`@wikimedia/language-data` 2.0.4, which MediaWiki's language selector reads) gives each of its 876 language codes a script: 56 scripts in all. Latin, Cyrillic and Greek are 3 of them. Chinese, Japanese and Korean take 5 (`Hans`, `Hant`, `Hani`, `Jpan`, `Kore`). SignWriting is 1. That leaves 47 others.
- **Chinese, Japanese and Korean are large and need `lang`.** A Noto CJK family is 3.5 to 4.6 MB a weight, sliced by Fontsource into about a hundred ranges. Their characters are shared across the three languages, which write many of them differently, so the right face depends on the text's language. Devices ship such fonts and choose among them by `lang`.
- **Fontsource's stylesheets for the families measured 259 KB**, because each family repeats Latin, Latin Extended and often Cyrillic, Greek, maths and symbol ranges. Those ranges are never used behind Source and Noto Sans. Kept to each script's own ranges, in WOFF2 only, they are 154 faces in 53 KB of minified CSS, 10 KB compressed, over 5.1 MB of font files.

### Direction

James's direction, from the design discussion of 2026-10-28:

- **"What serif and sans-serif fonts would you recommend if I wanted robust support for writing systems?"**
- **"Let's go with Source Serif 4 and Source Sans 3. For the gaps, Noto Serif and Noto Sans, then system fonts. Considerations for delivery are that in addition to browser language, a page may have many scripts on it simultaneously, like a mirrored Wikidata item with labels in over 100 languages."**
- **"Leave CJK as system fonts. Shared fallback layer. Include all 50 of the Noto fonts."** The 50 were the families for the scripts of Wikimedia's language data, estimated in the discussion; counted, they are 47 scripts served by 72 families (§2).

## Decision

### 1. A type set names a fallback layer (extends 0086 §3)

A **fallback layer** is a list of typefaces for the scripts a type set's own lack. The registry defines it, in `docs/registry/themes.toml` under `[fallback.{name}]`. A type set names one with `fallback = "{name}"`, and every type set that names it shares it:

- **Each typeface names the stacks it backs up** by their generic family (`stacks`: `serif`, `sans-serif`, `monospace`), and the Fontsource `subsets` it serves (§3). A role's stack takes the layer's families for its own generic family: the stack's first generic family, `system-ui` counting as `sans-serif`, or `sans-serif` if it names none. So a sans-serif heading stack, as in `hyperlegible`, takes the layer's sans families.
- **The families go in straight after the families the type set serves**, in the layer's order. If the stack serves none at its head, they go before its first generic family. A family the stack names already is not added again. So the order is the type set's own typefaces, then the layer's, then the device's: `'Source Sans 3', 'Noto Sans', 'Noto Sans Arabic', …, 'Noto Sans Symbols 2', system-ui, sans-serif`.
- **A layer may map language tags to stacks**, as a type set may, for scripts whose right face depends on the language rather than the characters. A layer's stack is written as `:lang({tag})` with the type set's text stack after it, `font-family: 'Noto Nastaliq Urdu', var(--font-family-base)`, unless the type set maps the same tag itself.
- **Only the registry defines layers.** An instance's or tenant's type set may name a shipped layer. A name the registry does not define is ignored when the stylesheet is built.
- **The layer's typefaces are shipped typefaces**, listed with their licences in `Special:Version` with those of each type set that names the layer ([0077](0077-special-version.md) §6).

`default26`, `hyperlegible` and `opendyslexic` name the layer `noto`. `basic` names none, since it serves no typeface by design (0086 A2).

### 2. The Noto layer (extends 0086 §1)

The layer `noto` serves, in this order:

- **Noto Sans and Noto Serif**, for Latin, Greek, Cyrillic and Vietnamese a type set's own families lack (Atkinson Hyperlegible has no Cyrillic, for instance).
- **A Noto family for each of the 47 scripts** of Wikimedia's language data that are not Latin, Greek, Cyrillic, Chinese, Japanese, Korean or SignWriting. They are ordered by how many languages use the script: Arabic, Devanagari, Myanmar, Ethiopic, Bengali, Hebrew, then the rest.
  - The sans stacks take the script's Noto Sans family. The serif stacks take its Noto serif where Noto has one (20 scripts, Arabic's being Noto Naskh Arabic), and otherwise its sans.
  - Tibetan has only Noto Serif Tibetan, which serves every stack.
  - The monospace stacks take the script families, not Noto Sans, so a character Lilex lacks goes to the device's monospace fonts.
- **Noto Sans Symbols and Noto Sans Symbols 2**, for symbols, arrows, Braille and the maths the type sets lack, in every stack.
- **Noto Nastaliq Urdu for `ur`**, as a stack for that language tag. Urdu is written in Nastaliq, and the same characters in Persian or Arabic are not.

**Weights 400 and 600**, or 400 alone where a family has no 600 (17 families). The browser takes the nearest weight for any other, so a 500 heading uses 400.

**Chinese, Japanese and Korean are left to the device**, as James decided. The stacks end in the device's families, which choose the regional forms by `lang`.

**SignWriting is left out.** It is written in two dimensions and needs a renderer, not a font.

**Noto Sans Math is left out.** Fontsource ships its glyphs under a Basic Latin range, so they could not be reached; Symbols 2 and the device's maths fonts cover maths.

In all, the layer has 72 families. `docs/registry/themes.toml` is the authority for the list.

### 3. Delivery (extends 0086 §5)

- **The layer's `@font-face` rules are in the stylesheet of each type set that names it**, after the type set's own. A reader who changes type set fetches the layer's rules again in the new type set's stylesheet, but no font file again.
- **Each face serves only the subsets the registry lists.** A script family serves its script's ranges, never the Latin, Cyrillic, Greek, Vietnamese, maths or symbol ranges Fontsource also packs into it, which Source, Noto Sans and the symbol families come before. So a face is fetched only for its own script.
- **WOFF2 only**, which every browser that honours `unicode-range` reads. The type sets' own typefaces keep WOFF beside WOFF2.
- **`font-display: swap`**, as for every shipped typeface (0086 §5). Text in a script shows at once in the device's font, then in Noto when the file arrives.
- **The rules are generated.** `ui/scripts/fallbacks.js` writes `ui/src/fallbacks/{name}.css` from the registry and the installed Fontsource packages, and a unit test fails while the file is stale. Vite builds it with hashed file names, and `triplespace-ui` reads it from the build's manifest, as it reads a type set's.
- **The cost**, measured on 2026-10-28: 154 faces, 53 KB of minified CSS (10 KB compressed) in each type set's stylesheet, cached as immutable. The build gains 5.1 MB of font files. A page fetches the files for its scripts alone, mostly 25 to 90 KB a weight each, in parallel, once per browser.

### 4. Crates (extends 0086 §7)

| Crate | Change |
|---|---|
| `triplespace-ui` | Reads the fallback layers from `themes.toml`; inserts a layer's families into a type set's stacks; writes the layer's `@font-face` rules and its stacks per language tag into the type set's stylesheet; lists the layer's typefaces with the type set's |
| `triplespace-accounts`, `triplespace-client` | A type set's `fallback`, reported by `siprop=triplespace` |

## Alternatives considered

- **Per-language stacks alone**, one rule per language tag. They depend on every element carrying its `lang`, and need a hundred rules in every type set. Per-character fallback needs neither, and per-language stacks remain for the cases where the language changes the face (§1).
- **The layer as a stylesheet of its own**, shared by every type set. It would be cached once across type sets, but needs a further link on every page and the head script to manage it, for 10 KB compressed.
- **Fontsource's stylesheets as they are**: 259 KB in front of every page, most of it ranges never used.
- **Bundling Noto CJK**: 70 to 100 MB in the build, which is embedded in the server binary, for faces whose right form depends on `lang`, which devices already handle.
- **`font-display: optional`** for the layer: no shift as faces arrive, but a slow face would be skipped for the whole visit, and a reader on a device without the script would see boxes until the next page.

## Consequences

- **A page in any script of Wikimedia's languages, except Chinese, Japanese, Korean and SignWriting, renders in a Noto face** on every device, whatever the device holds.
- **A mirrored item with labels in a hundred languages fetches its scripts' faces, about 20 to 30 files**, once per browser, and none for a script it does not hold.
- **Text in a script shifts as its face arrives**, on the first page that uses it.
- **Adding a script is a registry change**: a line in `themes.toml`, a dependency, and `npm run fallbacks`.
- **Every type set's stylesheet is 53 KB larger** where it names the layer.

## Open questions

- **Q1. Nastaliq line height.** Noto Nastaliq Urdu is tall, and may be clipped in Codex's line heights for controls and tables. To be checked on a page in Urdu.
- **Q2. Other languages written in Nastaliq**, such as Punjabi in Shahmukhi (`pnb`), Saraiki (`skr`) and Kashmiri (`ks`), whose readers may prefer it to Naskh.
- **Q3. SignWriting**, for American Sign Language (`ase`) and Brazilian Sign Language (`bzs`), which needs a renderer of its own.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0086](0086-theming.md) §1 | §2 | extends | 0086 A5 |
| [0086](0086-theming.md) §3 | §1 | extends | 0086 A5 |
| [0086](0086-theming.md) §5 | §3 | extends | 0086 A5 |
| [0086](0086-theming.md) §7 | §4 | extends | 0086 A5 |

## References

- [Noto](https://notofonts.github.io/), Google, under the SIL Open Font License 1.1; packaged by [Fontsource](https://fontsource.org/).
- `@wikimedia/language-data` 2.0.4, `data/language-data.json`: the script of each language code.
- CSS Fonts Level 4: `unicode-range` and the font matching algorithm, including the choice of the nearest weight.
