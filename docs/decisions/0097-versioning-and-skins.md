# 0097. Versioning, and skins apart from the binary

- **Status:** Proposed
- **Date:** 2026-10-29
- **Updated:** 2026-10-30 (A1)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0015](0015-record-format-and-partition-registry.md), [0034](0034-frontend-stack.md), [0057](0057-web-tier.md), [0077](0077-special-version.md), [0086](0086-theming.md), [0095](0095-the-interface-namespace.md)
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

*Current text: [19](../architecture/19-site-ui.md) §5.2, §5.10; [21](../architecture/21-special-pages.md) §5.6; [23](../architecture/23-configuration-and-registry.md) §3.4.*

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

- **Q1.** ~~**Rollback.** Whether a minor that introduces a record major ships it behind a switch first, off by default for one minor, so that every minor can be rolled back.~~ *Settled by §4: yes; the release that writes it by default is a product major.*
- **Q2. Additive payload changes.** Whether the current payload encodings admit fields an older reader ignores, as §4's minor assumes, or whether every payload change is in practice a major until the encodings are made extensible.
- **Q3.** ~~**A reader's own skin.** Whether a reader may choose among the installed skins, as MediaWiki's skin preference allows, or the wiki's skin is the only choice.~~ *Settled by §8: readers choose among the skins the wiki makes available; what a tenant may make available or disable is the instance's default, ceiling and lock for `skins.available`.*
- **Q4. Skin licences.** The default skin is GPL-3.0-or-later with the rest of the repository. Whether an operator's skin, which the binary interprets and does not link, must be.
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
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§18
- **Summary:** The Decision's current text now lives in the architecture chapters [01](../architecture/01-log-and-records.md), [03](../architecture/03-storage-caches-and-search.md), [19](../architecture/19-site-ui.md), [20](../architecture/20-web-tier.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
