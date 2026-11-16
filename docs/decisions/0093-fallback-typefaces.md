# 0093. Fallback typefaces for every script

- **Status:** Proposed
- **Date:** 2026-10-28
- **Updated:** 2026-10-30 (A1)
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

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.4; [19](../architecture/19-site-ui.md) §5.9.*

### 2. The Noto layer (extends 0086 §1)

*Current text: [19](../architecture/19-site-ui.md) §5.9.*

### 3. Delivery (extends 0086 §5)

*Current text: [19](../architecture/19-site-ui.md) §5.2, §5.9.*

### 4. Crates (extends 0086 §7)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

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

## References

- [Noto](https://notofonts.github.io/), Google, under the SIL Open Font License 1.1; packaged by [Fontsource](https://fontsource.org/).
- `@wikimedia/language-data` 2.0.4, `data/language-data.json`: the script of each language code.
- CSS Fonts Level 4: `unicode-range` and the font matching algorithm, including the choice of the nearest weight.

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§4
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
