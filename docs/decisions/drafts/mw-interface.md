# Draft: MediaWiki's `mw` interface in the browser

- **Status:** Draft
- **Date:** 2026-11-23
- **Author:** James Hare / Claude Opus
- **Uses:** [0008](../0008-namespaces-and-document-pages.md), [0034](../0034-frontend-stack.md), [0056](../0056-security-model.md), [0095](../0095-the-interface-namespace.md), [0097](../0097-versioning-and-skins.md)

This draft has no number and decides nothing: it describes the choices about MediaWiki's
`mw` JavaScript interface and their costs, without recommending any. Once the choices are
made, it takes the next free number, the chosen text goes into the chapters, and the
options not chosen move to *Alternatives considered*.

## Context

**Plain draws MediaWiki's names** ([0097](../0097-versioning-and-skins.md) A14, A17): the
IDs and classes Monobook, Vector and Minerva share, and portlets whose items are keyed
(`ca-edit`, `pt-logout`, `t-info`), from menus the binary fills
([0097](../0097-versioning-and-skins.md) §11). `mw.*` was deferred.

**MediaWiki promises only `mw`.** Its frontend stable interface policy (2023) holds that HTML
and CSS are not stable, and that browser code should use what the `mw` object offers
(`mw.util.$content`, `mw.util.addPortletLink`, `mw.util.addSubtitle`, `mw.hook`) rather than
DOM queries. A gadget written to the policy therefore needs `mw`, not Plain's names; older
gadgets use both.

**Triplespace runs no wiki-hosted JavaScript.**
- [0008](../0008-namespaces-and-document-pages.md) §5 refuses the `javascript` and `css`
  content models as an injection risk; only `sanitized-css` is supported.
- [0095](../0095-the-interface-namespace.md) §1 makes the Interface namespace plain-text
  messages only, so there is no `MediaWiki:Common.js`.
- The site runs under a Content Security Policy of `script-src 'self'` with no inline script
  and no `eval` ([0034](../0034-frontend-stack.md) §11; 09 §1.4).
- A skin may ship JavaScript, but only the operator installs skins
  ([0097](../0097-versioning-and-skins.md) §8).
- A wiki's site styles are scoped to `.mw-parser-output` (11 §6.4) and never reach the
  frame.

So today `mw` would have no wiki-hosted callers. Its possible callers are readers' own
userscripts (run by a browser extension), operator skins, and wiki-hosted gadgets or user
scripts if a later decision allows them.

**What exists to build on.**
- The Action API is MediaWiki's, so `mw.Api` is a thin layer.
- The frame already writes data for the browser as `<script type="application/json">`
  blocks (`ts-theme`, `ts-search-data`, `ts-appearance-data`), which the policy allows.
- Interface messages, with a wiki's overrides, are available to the frame.
- `editinterface` exists for the Interface namespace
  ([0095](../0095-the-interface-namespace.md) §4).

### Direction

James's direction, from the design discussions of 2026-11-22 and 2026-11-23:

- **"Support for mw.\* is deferred."** (2026-11-22, deciding A14.)
- **"Also, what would it take to implement our own mw.\* interface?"** (2026-11-23)
- **"Start a draft ADR (marked as Draft) about the mw.\* interface describing the different
  options without choosing any"** (2026-11-23)

## The choices

Each choice is lettered, and its options are numbered. Some depend on earlier ones: B, C and
D arise only for options A3 and A4.

### A. Whose code calls `mw`

The question the rest depends on: what code runs on a Triplespace page and expects `mw`.

- **A1. Nobody's, for now.** `mw` stays deferred, and Plain's names serve styles and tools
  that read the page. Costs nothing; gadgets written to MediaWiki's policy do not run.
- **A2. Readers' own userscripts and operator skins.** `mw` is served on every page (or on
  request, E below); no wiki-hosted script exists. The security model does not change, since
  only the instance's own code and what a reader chose to run in their own browser call it.
  Its value depends on readers bringing scripts written for Wikimedia wikis, and on how
  browser extensions inject scripts under the site's Content Security Policy, which this
  draft has not tested.
- **A3. A2, and gadgets a wiki's administrators approve.** Scripts kept on the wiki, edited
  only by holders of a dedicated right, offered to readers as gadgets that are on by default
  or chosen in preferences. Closest to how Wikimedia wikis work. It reverses
  [0008](../0008-namespaces-and-document-pages.md) §5 for one place and one right, and needs
  B, C and D.
- **A4. A3, and per-user scripts** (`User:X/common.js`), each run only for its owner, as
  MediaWiki does. A script that runs as the reader can act as the reader, so a compromised or
  borrowed script takes over the account; MediaWiki accepts this, and Triplespace's security
  model ([0056](../0056-security-model.md)) has so far not.

### B. Where wiki-hosted scripts live (A3, A4)

- **B1. A `javascript` content model in the Interface namespace**, as `MediaWiki:Gadget-*.js`
  is. Amends [0008](../0008-namespaces-and-document-pages.md) §5 and
  [0095](../0095-the-interface-namespace.md) §1 (plain text only). Pages copied from a
  Wikimedia wiki keep their titles.
- **B2. A namespace of its own for gadgets**, as the Gadgets extension's later `Gadget` and
  `Gadget definition` namespaces are, a script and a JSON definition per gadget. Leaves the
  Interface namespace plain text. Needs two namespace numbers from the registry.
- **B3. Not on the wiki at all**: gadgets are files the operator installs, as skins are.
  No editing, history or review on the wiki; no new content model; only A2's security model.
  Wiki administrators cannot add one.

How gadgets are listed is a further choice: MediaWiki's `Gadgets-definition` text format, which
wikis can copy; a JSON definition per gadget; or a registry file.

### C. Who may change a script (A3, A4)

- **C1. `editinterface`**, the right that already edits the Interface namespace. Fewest new
  parts; a holder of it can then run code in every reader's browser.
- **C2. A right of its own**, MediaWiki's `editsitejs` (with `editsitecss`), held by a small
  group like MediaWiki's `interface-admin`, apart from those who edit messages. A new
  permission and group in `permissions.toml` and `groups.toml`.
- **C3. C2, with conditions on the holder**: a second factor required to hold or use it, a
  review before a change goes live, or both. What "review" means (a second holder's approval,
  a delay) is itself a choice.

For A4, whether a user's own scripts need any right beyond owning the page.

### D. Serving, the security policy and turning it off (A3, A4)

- **D1. Same-origin files.** A script is served from the site's own origin under a
  content-hashed URL, versioned by its page's latest offset as the site styles are (11 §6.4),
  so `script-src 'self'` stands and no inline script is needed. Editing a gadget refreshes no
  rendered page.
- **D2. A separate origin for wiki code** (a sandbox host per tenant), keeping scripts out of
  the site's own origin. Sessions and the API would then need cross-origin rules, and most
  gadgets assume they run on the wiki's origin.

Under either, separately:

- **Turning it off.** A tenant setting or tenancy switch that allows gadgets at all; a
  reader's preference to run none; and MediaWiki's `safemode=1`, which loads a page without
  wiki-hosted code, so that a reader can recover from a broken gadget.
- **Private wikis and logged-out readers.** Whether gadgets run for readers who are not
  signed in, and whether a private wiki's gadgets are served to outsiders, as its palettes
  and type sets are.

### E. How much of `mw` is provided

Tiers, each adding to the one before; what each lets run is an estimate to be measured
against real gadgets.

- **E1. Page facts and menus:** `mw.config` (`wgPageName`, `wgTitle`, `wgNamespaceNumber`,
  `wgCanonicalNamespace`, `wgAction`, `wgArticleId`, `wgRevisionId`, `wgUserName`,
  `wgUserGroups`, `wgServer`, `wgScript`, `wgArticlePath`, and `wbEntityId` on entity pages),
  `mw.util` (`getUrl`, `wikiScript`, `getParamValue`, `$content`, `addPortletLink` over the
  keyed portlets, `addSubtitle`), and `mw.hook` firing `wikipage.content`. Enough for scripts
  that add a link or read the page.
- **E2. Talking to the wiki:** `mw.Api` over `api.php` with tokens, `mw.user`, `mw.message`
  and `mw.msg` over the site's messages and a wiki's overrides, `mw.notify`, `mw.storage`,
  `mw.cookie`, and an `mw.loader` that resolves the common module names
  (`mediawiki.api`, `mediawiki.util`) without loading anything. Enough for most small gadgets.
- **E3. MediaWiki's larger libraries:** `mw.Title`, and shims for `jquery.ui` and OOUI, which
  large gadgets (Twinkle, HotCat, navigation popups) depend on. Large, and the shims would
  chase MediaWiki's versions.
- **E4. Wikibase's:** the `wikibase` globals and `wbEntity` that Wikidata scripts read,
  beside Wikibase's entity-page markup, which Plain does not draw. A layer of its own.

### F. jQuery

- **F1. Ship jQuery** (about 30 KB compressed), as MediaWiki does, since nearly every gadget
  assumes `$`.
- **F2. Ship none**, with `mw` written without it; scripts that use `$` do not run.
- **F3. Ship it only on pages that run a gadget or a userscript asks for `mw`**, if E's
  loading can know that in advance.

### G. When `mw` is loaded

- **G1. On every page**, so a userscript finds it as on a Wikimedia wiki. Adds its size to
  every page.
- **G2. Only on a wiki that has gadgets switched on, or for a reader who has turned it on**
  in preferences (for their userscripts).
- **G3. On demand**: a small stub on every page that loads the rest when first used.
  Scripts that read `mw.config` synchronously at start need at least that part present.

### H. Where the code and the data come from

- **H1. The site's bundle (`ui/`)**, with the frame writing a `mw-config` JSON block: every
  skin gets it, and the template contract adds the block to a skin's obligations.
- **H2. Plain's own script**, so that only skins that extend Plain have it, and a skin that
  stands alone chooses.
- In either, whether `addPortletLink` writes into the DOM by Plain's names, or asks the skin
  through a small interface each skin implements, so that a skin that stands alone can
  support it.

### I. What Triplespace promises about its `mw`

- **I1. MediaWiki's policy, adopted:** everything on `mw` not marked private is stable, and a
  change follows MediaWiki's deprecation process (`mw.log.deprecate`, one version's warning
  before removal).
- **I2. A versioned contract of its own**, with a version reported to scripts, and the parts
  of MediaWiki's `mw` that are not provided listed.
- How it is versioned beside the template contract and Plain's version
  ([0097](../0097-versioning-and-skins.md) §9).

## Consequences

These hold whichever options are chosen.

- **Plain's names stay.** `mw` is the stable route MediaWiki recommends, and the DOM names
  remain for scripts and styles that do not use it
  ([0097](../0097-versioning-and-skins.md) A17).
- **The keyed menus are what `addPortletLink` writes into**, and `mw.config` is filled from
  data the frame already has; neither needs a new API call, so a page stays within its four
  ([20](../../architecture/20-web-tier.md)).
- **Any of A3 and A4 changes the security model**, and comes with an amendment to
  [0008](../0008-namespaces-and-document-pages.md) §5 and to [0056](../0056-security-model.md).

## Open questions

- **Q1. How browser extensions inject userscripts under the site's policy.** Whether
  Tampermonkey and similar run page-context scripts on a site with `script-src 'self'` and no
  inline script, which decides how much A2 is worth. Untested.
- **Q2. Which gadgets to measure against.** A list of real gadgets and user scripts, small and
  large, from Wikimedia wikis and Wikidata, to say what each tier of E lets run.
- **Q3. Mirrored pages.** Whether a tenant that mirrors a Wikimedia wiki's pages may mirror its
  gadgets too, or only copies them by hand, as it copies `Common.css` into its site styles.

## References

- MediaWiki: the frontend stable interface policy (`Stable_interface_policy/Frontend`), and
  the recommendations for gadget developers on Wikimedia wikis
- MediaWiki: the Gadgets extension, `MediaWiki:Gadgets-definition` and the `Gadget` and
  `Gadget definition` namespaces
- MediaWiki: the `interface-admin` group and the `editsitejs`, `editsitecss`, `edituserjs`
  rights; `safemode`
- MediaWiki: the `mw` reference on doc.wikimedia.org (`mw.config`, `mw.util`, `mw.hook`,
  `mw.Api`, `mw.loader`, `mw.Title`)
- [0097](../0097-versioning-and-skins.md) A14, A17; chapter 19 §5.10
