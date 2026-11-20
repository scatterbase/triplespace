# 0113. The site's rendering in six crates

- **Status:** Accepted
- **Date:** 2026-11-20
- **Author:** James Hare / Claude Fable
- **Uses:** [0010](0010-site-ui.md), [0034](0034-frontend-stack.md), [0047](0047-special-pages.md), [0057](0057-web-tier.md), [0097](0097-versioning-and-skins.md)

## Context

[0005](0005-crate-organization.md) §2 gave the site one rendering crate, `triplespace-ui`, and [0057](0057-web-tier.md) §12 added `triplespace-client` below it and `triplespace-web` above. Every page the site has gained since went into that one crate. By November 2026 it held 49,700 lines of `src` (55,600 with its tests), about a fifth of the workspace's Rust and twice the next-largest crate, `triplespace-api-action` at 23,600. The special pages alone were 29,000 lines, 58% of it.

A crate is Rust's unit of compilation and the only boundary the compiler enforces. Inside the one crate nothing stopped a module from reaching any other, and two tangles had grown:

- **`pages` was two things.** It held the dispatcher, which calls every page kind, and the handler toolkit every page kind calls back into (about 500 call sites), so nearly every module was in one cycle.
- **Shared widgets were filed under the page that first drew them.** Entity, document and talk pages reached into the special pages for the form controls, the change list and the value parsers, and some 25 modules imported the pager's link type from the entity history.

Both were undone inside the crate on 2026-11-19 and 2026-11-20, as moves with no change of behaviour: the dispatcher became its own module, and the shared widgets moved to modules of their own beside the frame. After that no module referred to one in a higher layer, and only one reference crossed between the groups of special pages this ADR names, which was moved down too. Build times were not measured before this ADR was drafted (Q1).

### Direction

James's question, opening the design discussion of 2026-11-19:

- **"triplespace-ui as a single crate represents a significant percentage of all Rust code. Are there opportunities to split it up into smaller crates to make it more maintainable?"**

And his answers of 2026-11-20 to the choices put while drafting: the crates under the router are named `triplespace-ui-kit`, `triplespace-ui-pages` and `triplespace-ui-special`; the special pages are split into three crates now, by accounts, administration and the rest, not left as one; and the integration tests and the skin-parity corpus stay in `triplespace-ui`. On reading the draft the same day he confirmed all five names, `triplespace-ui-accounts` and `triplespace-ui-admin` among them; that `Special:Block`, `Special:UserRights` and `Special:RenameUser` sit with the account pages ("This makes sense"); and that the heading of 19 §5.2 may be updated. Asked whether the forms that create and edit entities should sit in `triplespace-ui-pages` instead, he kept them in `triplespace-ui-special`.

## Decision

### 1. Six crates in four layers

*Current text: [19](../architecture/19-site-ui.md) §5.2; [22](../architecture/22-crates-and-stack.md) §2.4.*

### 2. The tests that draw whole pages stay with the router (uses 0097 §16)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.4.*

### 3. Rule 10 covers the crates the site is assembled from (amends 0005 §3)

*Current text: [22](../architecture/22-crates-and-stack.md) §1.2.*

### 4. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **Leave it one crate, untangled.** The layering inside the crate already gives most of the readability. It was not chosen because nothing keeps it: the next widget drawn by a second page is one `use` away from a new cycle, and a change to any page still recompiles all 49,700 lines.
- **Three crates under the router: kit, pages, one for the special pages.** This was the recommendation put to James. He chose to split the special pages now: at 29,000 lines one special-page crate would still be the largest in the workspace.
- **A crate per special page, or per ADR.** The pages are largely independent, so it is possible, but 38 crates for one site is more manifests and map entries than boundaries worth having. Three groups follow who uses the pages: an account holder, an operator, any reader.
- **The forms that create and edit entities in `triplespace-ui-pages`.** The dependency graph allows it: `Special:NewItem` and `Special:NewProperty`, the term forms and `Special:CreateEntities` (about 6,500 lines) depend only on the kit, the entity page and the moderation gate. They stay in `triplespace-ui-special` because they are served by name, as every special page is, while the pages crate holds what a title's namespace selects; the two crates are then of a size, about 11,700 and 11,600 lines, where moving the forms would make the pages crate about 18,000.
- **Integration tests beside each page kind.** Faster test runs per crate, but the fixture API and the parity corpus would have to be published as a test-support crate, and a page is only what a reader gets once the router, the frame and the skin have drawn it.

## Consequences

- **Six crates where there was one.** Five new manifests and three new entries in the crate map; the site's dependency edges are checked by `cargo xtask deps` like every other.
- **A change to a special page no longer recompiles the entity page.** A change to the kit still recompiles everything above it, so what goes in the kit matters more than before.
- **The layering is enforced by the compiler.** A widget two layers draw has to move down before the second can use it.
- **`triplespace-web` and `triplespace-server` do not change.** They depend on `triplespace-ui` alone, whose public surface is the same.
- **`implements` claims move with the code.** The claims and the first `//!` paragraphs the generated views read (24 §15) are in the new crates, and `docs/check.sh --write` rewrote the views in the change that moved the modules.
- **The crate map's entry for `triplespace-ui` is divided among the six.** Each clause went to the crate that holds, or will hold, what it names; where a clause names a special page beside its page kind, the clause stays with the page kind and the special page is `triplespace-ui-special`'s. The allowance to depend on `scatter-normalize` and `scatter-providers`, which no code uses yet, went to `triplespace-ui-pages`.
- **About thirty items became public.** Functions and types that were `pub(crate)` or private in the one crate and are used from a higher crate are `pub` in the lower one; `Site` and the handler toolkit are the kit's public API.

## Open questions

- **Q1. Whether the split shortens rebuilds enough to notice.** Build times were not measured. If a clean and an incremental build of the site are measured before the code moves, the three special-page crates can be weighed against one.
- **Q2. Whether the forms that create and edit entities are special pages or part of the pages crate.** *Settled by §1.*
- **Q3. A check of the layers before the crates exist.** *Withdrawn: the crates exist, so the compiler and `cargo xtask deps` hold the layers.*
- **Q4. The heading of 19 §5.2.** *Settled by §1: retitled "Server rendering, in `triplespace-ui` and the crates under it", as James allowed on 2026-11-20.*

## References

- [22](../architecture/22-crates-and-stack.md) §1.2, §2.1, §2.4: the rules, the map and the six crates.
- [20](../architecture/20-web-tier.md): the web tier the crates make up.
- `docs/registry/crates.toml`: the crate map.
