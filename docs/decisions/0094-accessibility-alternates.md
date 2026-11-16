# 0094. Accessibility alternates of palettes

- **Status:** Proposed
- **Date:** 2026-10-28
- **Updated:** 2026-10-30 (A1)
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

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.4; [19](../architecture/19-site-ui.md) §5.9.*

### 2. Which alternate a palette wears (extends 0086 §2)

*Current text: [19](../architecture/19-site-ui.md) §5.9.*

### 3. A palette's prerequisites (amends 0086 §2; extends 0086 §1)

*Current text: [18](../architecture/18-api.md) §3.2; [19](../architecture/19-site-ui.md) §5.9.*

### 4. Parchment's alternates replace the colour-vision palettes (amends 0086 §1, §2)

*Current text: [19](../architecture/19-site-ui.md) §5.9.*

### 5. The reader chooses an alternate (amends 0086 §4)

*Current text: [07](../architecture/07-actors-and-accounts.md) §8.1; [19](../architecture/19-site-ui.md) §2.7, §5.9.*

### 6. Serving (amends 0086 §5)

*Current text: [18](../architecture/18-api.md) §2.2; [19](../architecture/19-site-ui.md) §5.9.*

### 7. Crates (extends 0086 §7)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

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

## References

- Machado, Oliveira and Fernandes (2009), *A physiologically-based model for simulation of color vision deficiency*, by which the alternates' colours were chosen (0086 §2).
- WCAG 2.1: 1.4.1 (use of colour), 1.4.3, 1.4.6 and 1.4.11.

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§7
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [07](../architecture/07-actors-and-accounts.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
