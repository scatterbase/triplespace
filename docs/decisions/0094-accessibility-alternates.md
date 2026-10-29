# 0094. Accessibility alternates of palettes

- **Status:** Proposed
- **Date:** 2026-10-28
- **Author:** James Hare / Claude Opus
- **Changes:** [0086](0086-theming.md)
- **Uses:** [0085](0085-settings-and-preferences.md)

## Context

[0086](0086-theming.md) §2 gave readers with a colour-vision deficiency palettes of their own: `parchment-rg` for red–green deficiencies and `parchment-by` for blue–yellow, each extending `parchment` and changing only the colours that carry meaning. James had chosen them on 2026-10-17 in place of colour-vision variants of every palette (0086 Q1), so that a reader made one choice.

That choice put them in the Appearance menu's palette select beside `parchment` and `encyclopedia`, as three versions of one palette. A reader who needs the red–green colours could wear them only on `parchment`: choosing `encyclopedia`, or any palette a wiki writes for its brand, took them away, unless that wiki wrote a colour-vision palette of its own (0086 §2, last paragraph). The colour-vision need is the reader's, the brand is the wiki's, and one select made the reader trade one for the other.

### Direction

James's direction, from the design discussion of 2026-10-28, reviewing the site's header and Appearance menu:

- **"Instead of having separate drop down boxes for parchment, parchment for red-green colorblindness, and parchment for blue-yellow colorblindness, we allow themes to define keyed alternates. So in the case of Parchment, it would register a variant for -rg and a variant for -by. Themes can inherit accessibility alternates from other themes; a theme that depends on other themes cannot be installed unless the prerequisite themes are available. Then, in the Appearance widget, you can select an accessibility override."**

Asked how the keys and the override work, James chose (2026-10-28): the keys are the registry's, and the override is a preference that follows the farm account like contrast; a palette with no alternate for a key takes the default palette's, checked when it is written; a palette inherits alternates through `extends`, or names another with `alternates-from`, and either makes that palette a prerequisite; and palettes only, with the accessibility type sets left as they are.

In the same review James asked for the header and the menu to change as follows, which need no change to any ADR: the header offers **Log in** alone, without a separate link to create an account (19 §1.2 already says so; the login page links to account creation); the **New** and **Appearance** menus open from icon buttons, named by their labels; the wiki's own palette and type set are labelled with "(default)", as "Parchment (default)", and the type set `default26`, then labelled *Default 26*, is labelled *Source* ("Instead of "Default 26," could the default type set be called Source?"), its name unchanged; and colour mode, contrast and text size are rows of icon buttons, one chosen, in place of selects.

## Decision

### 1. Alternates are keyed by the registry (amends 0086 §2; extends 0086 §1)

**An accessibility alternate** is a set of colours a palette wears for a reader with a particular need, whatever else they have chosen. **Its keys are the registry's**: `themes.toml` lists them under `[alternate.{key}]`, each with a label, and only a release adds one, so a reader's override means the same on every palette and every wiki. Two ship:

| Key | For | Label |
|---|---|---|
| `rg` | red–green colour-vision deficiencies (protanopia and deuteranopia) | *Red–green (protanopia and deuteranopia)* |
| `by` | blue–yellow colour-vision deficiency (tritanopia) | *Blue–yellow (tritanopia)* |

**A palette keys an alternate** under `alternate.{key}` in its record, with up to the four variants of 0086 §2. Each variant of the alternate is laid over the same variant of the palette as served, so it states only the colours it changes; its `light-more` and `dark-more` take its own `light` and `dark` first, and the palette's own high-contrast values where it has neither. A palette may key only the registry's keys. **Every variant with an alternate on is checked to the floors of 0086 §2**, as served, so an alternate cannot lower a palette's contrast.

### 2. Which alternate a palette wears (extends 0086 §2)

For each key, a palette wears, first match wins:

1. its own alternate for the key;
2. the alternate of the palette it `extends`;
3. the alternate of the palette its `alternates-from` names, a field for a palette that wants another's alternates without its colours;
4. the default palette's (`parchment`'s).

The fourth rule means **every palette has every alternate**, as every palette has a dark mode (0086 §2): a wiki's brand never takes a reader's colours away. A palette whose colours fail the floors with an inherited alternate on cannot be written until it keys its own for that key. `encyclopedia` keys none, and passes with `parchment`'s.

### 3. A palette's prerequisites (amends 0086 §2; extends 0086 §1)

**A palette depends on** the palette it `extends` and the one its `alternates-from` names: its prerequisites. **A palette whose prerequisite is not offered where it is written cannot be written**: a write naming one is refused, naming it, as a palette may name a shipped palette or one at its own scope (0086 §2), and the registry's own palettes name only registry palettes. **A prerequisite cannot be deleted** while a palette that depends on it remains, as 0086 §2 already held for `extends`. Keying a palette under a new name (0086 §1) renames the `alternates-from` that name it, as it renames `extends`.

### 4. Parchment's alternates replace the colour-vision palettes (amends 0086 §1, §2)

**`parchment` keys `rg` and `by`**, with the colours of the palettes `parchment-rg` and `parchment-by` unchanged: the same values over the same variants, so a reader wearing an alternate sees exactly what that palette showed. A test holds the two equal.

**The names `parchment-rg` and `parchment-by` are retired**, kept in `themes.toml` with `retired = true` and `replaced_by` naming `palette.parchment` and the alternate, so they are never given another meaning. **They are read, not migrated**: a `theme.palette` or `ui.palette` that names one reads as `parchment`, and a `ui.palette` that names one reads besides as its alternate where the reader has chosen none, on the server and in the browser's stored choices alike. A reader's next save of the Appearance menu writes the new values.

### 5. The reader chooses an alternate (amends 0086 §4)

**The preference `ui.vision`**, *Colour vision*, takes `none` or a registry key, defaults to `none`, is `browser`, and follows the farm account, as contrast does, since the need is the reader's and not a wiki's. It is offered in the Appearance menu and on `Special:Preferences`, and like colour mode, contrast and text size **it is never withheld**, whatever `theme.choice` or a lock says. It applies over whichever palette the reader wears.

`theme.choice = accessibility` withholds the brand palettes and type sets as before; it offers the accessibility type sets, and the alternates of the wiki's palette.

### 6. Serving (amends 0086 §5)

**The alternates are in the palette's stylesheet**, so choosing one fetches nothing: after the palette's own rules, each alternate's rules repeat them with `[data-vision="{key}"]` on `:root`, one attribute more specific than the rule each overlays, so the cascade among them is the palette's. The `dark` rule of an alternate also holds the palette's dark value of every colour its `light` sets and its `dark` does not, so no light colour shows in dark mode. **`data-vision`** is written on `<html>` beside `data-mode`, by the server for a signed-in reader and by the head script for one who is not, and only for a key other than `none`.

**`siprop=triplespace` reports** each palette's alternates and `alternates-from` with its variants and `extends`; the keys are the registry's.

### 7. Crates (extends 0086 §7)

| Crate | Change |
|---|---|
| `triplespace-accounts` | The `alternate` and `alternates-from` fields of a palette; the registry's keys and retired names; a palette's prerequisites, those missing from a catalogue, and its dependents; a retired `theme.palette` read as its replacement |
| `triplespace-client` | The fields, as `siprop=triplespace` reports them |
| `triplespace-ui` | Which alternate a palette wears; the alternates' rules in the palette's stylesheet and their contrast check; `data-vision`; `ui.vision` in the Appearance menu; retired names read in the reader's choices |

The palette routes that would refuse a write for a missing prerequisite are not built yet (0086 §7); `triplespace-accounts` gives them the check.

## Alternatives considered

- **Keep the colour-vision palettes** (0086 §2 as it stood). A reader would keep choosing between their colours and a wiki's brand.
- **Keys each palette names freely.** An override chosen on one palette would mean nothing on the next, and a reader's farm-wide choice could not follow them. The registry's keys make `rg` mean red–green everywhere.
- **Offer only the alternates a palette keys itself.** A wiki's own palette would offer no colour-vision colours until its authors wrote them; the default palette's, checked against it, is the floor, as with dark mode.
- **Alternates of type sets too**, such as a dyslexia override that swaps in OpenDyslexic whatever the type set. A type set is chosen whole, and the accessibility type sets already are a reader's choice that no wiki can withhold; James kept this to palettes for now (Q1).
- **A separate stylesheet per alternate.** It would be one more request when a reader turns one on, and twice the files; the alternates' rules are a small part of each palette's.

## Consequences

- **A reader's colour-vision colours follow them to any palette on any wiki**, and no longer stand in for the wiki's palette; the palette select lists palettes only.
- **A wiki's brand palette has the colour-vision colours without writing them**, the default palette's, or its own where those fail its contrast or it wants its hues.
- **Every palette's stylesheet is larger** by its alternates' rules, which hold only the colours each alternate changes.
- **A palette that names another cannot exist without it**, and its prerequisite cannot be deleted from under it.
- **Stored choices that name a retired palette keep working** without a migration, until the reader saves the menu.

## Open questions

- **Q1. Alternates of type sets**, such as a dyslexia override, deferred by James on 2026-10-28.
- **Q2. More keys**, such as an achromatopsia (monochromacy) alternate, which a release would add to the registry and to `parchment`.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0086](0086-theming.md) §1 | §1, §3, §4 | amends | 0086 A6 |
| [0086](0086-theming.md) §2 | §1, §2, §3, §4 | amends | 0086 A6 |
| [0086](0086-theming.md) §4 | §5 | amends | 0086 A6 |
| [0086](0086-theming.md) §5 | §6 | amends | 0086 A6 |
| [0086](0086-theming.md) §7 | §7 | extends | 0086 A6 |

## References

- Machado, Oliveira and Fernandes (2009), *A physiologically-based model for simulation of color vision deficiency*, by which the alternates' colours were chosen (0086 §2).
- WCAG 2.1: 1.4.1 (use of colour), 1.4.3, 1.4.6 and 1.4.11.
