# 0097. Versioning, and skins apart from the binary

- **Status:** Proposed
- **Date:** 2026-10-29
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

**Every contract Triplespace defines has a version `MAJOR.MINOR.PATCH`.** A patch fixes what already exists; a minor adds without breaking; a major breaks. **The leftmost non-zero number is the compatibility boundary**, as Cargo reads it: under 1.0, `0.3 → 0.4` may break and `0.3.1 → 0.3.2` may not. A contract at major 0 is unstable in that sense only: it may break at any minor, never at a patch.

**What "breaks" means is defined per contract** (§2), always from the point of view of whoever depends on it: a client, an operator, a skin author, a reader of old records.

**A contract Triplespace implements but does not own keeps its owner's version** (§6). Triplespace never assigns its own number to someone else's contract.

**When a number moves** is §18: a patch when something is published, a minor or major when the change is made.

### 2. What is versioned

| Contract | Owner | Who depends on it | A major is |
|---|---|---|---|
| Each published `scatter-*` crate | Triplespace | Scatterbase, other Rust code | A public API break under Cargo's SemVer rules (§3) |
| Record formats: header layout, payload types | Triplespace | Every reader of the log, forever (§4) | A record older readers cannot interpret |
| REST API and `siprop=triplespace` | Triplespace | The web tier, tools (§5) | A removed or changed field, route or behaviour |
| Registry and configuration schemas | Triplespace | Operators, the crates that embed them | A renamed or removed key, a changed meaning |
| URL scheme: routes and special-page names | Triplespace | Every link, bookmark and tool | A route or name that changes or goes away |
| Template contract (§9) | Triplespace | Skins | A removed or renamed view-model field or template, a new obligation |
| Theme tokens (§14) | Codex | Palettes and type sets | Codex's own major |
| Each skin (§8) | Its author | Operators, user CSS | Its own markup hooks or message keys change |
| Each message bundle (§13) | Its owner | Interface overrides, translators | A key renamed or removed |
| MediaWiki Action API, Wikibase JSON and RDF, XML export, MCP, ActivityPub, OAuth | Their owners | Clients written for them | Their owners' versions (§6) |
| **Triplespace, the product** | Triplespace | Operators (§7) | An upgrade that needs operator action |

### 3. Crates (extends 0005 §6)

**`scatter-*` crates are versioned one by one**, since they are the ones meant for use outside the workspace ([0005](0005-crate-organization.md) §6). Their contract is their public Rust API, with the rules of the Cargo SemVer reference, and `cargo-semver-checks` checks each one in CI against its last published version once it has one. **`triplespace-*` crates, `xtask` and the binaries are `publish = false`** and carry the product version (§7) from `[workspace.package]`. Until a `scatter-*` crate is first published it carries the product version too.

### 4. Record formats (extends 0015 §2)

**Every reader reads every record ever written.** In an append-only log no record is rewritten, so backward compatibility, reading what older writers wrote, is not a property a change may give up: it is a requirement on every release. What a record format version describes is **forward** compatibility: whether an older binary can read what a newer one writes, which matters for rollback, replicas and federated peers.

- **The payload type's tag carries its major**: `scatter:v0/changeset` is major 0 of the changeset. A change older readers can safely ignore is a minor and keeps the tag. A change older readers cannot interpret is a major and takes a new tag, `scatter:v1/changeset`; both tags are read from then on.
- **The header's format version** (01 §2.2) is the header layout's major. A minor change to the header is not possible, since every field is positional and covered by the commitment, so any change is a new format version.
- **The stored form is not a format** ([0058](0058-packed-record-storage.md) §1): changing it is never a version of anything.
- **A new record major ships first behind a switch, off by default**, in a minor release: that release reads it and can write it, and nothing writes it unless the operator turns the switch on, so the minor can still be rolled back. **The release that writes it by default is a product major** (§7), and comes no earlier than the next minor.

All payload types are at major 0 until the product reaches 1.0; nothing written before then is promised to older readers.

### 5. The REST API and `api_version` (amends 0057 §9)

**`api_version` becomes a version string, `MAJOR.MINOR.PATCH`**, the version of Triplespace's own API surface: the REST routes and the `triplespace` siteinfo property. The integer reported before is its minor at major 0, so `3` becomes `"0.3.0"`. **The web tier serves an API of the same major and at least the minor it was built against**, and reports itself not ready otherwise. The REST base path already carries the major, `triplespace/v0` (18), and `siprop=triplespace` reports it; a new major is served at a new base path beside the old one while clients move.

### 6. Contracts Triplespace implements

**Each is reported in its owner's terms, and a change of target is a Triplespace major only if it drops what a client relied on:**

- **MediaWiki's Action API**: the MediaWiki release whose behaviour the modules match, in `generator`, with `formatversion=2` (18 §2.2; [docs/clients.md](../clients.md)).
- **Wikibase JSON and RDF**: the Wikibase release the snapshots in `docs/api/snapshots` were taken from, and the RDF format version Wikibase reports.
- **XML export**: export schema 0.11 (21).
- **MCP, ActivityPub, HTTP Signatures, OAuth**: the protocol versions each negotiates ([0075](0075-mcp-server.md); [0022](0022-federation.md) §8; [0025](0025-oauth-server.md)).
- **Codex's tokens** (§14).

### 7. The product version (extends 0077 §6)

**Triplespace's own version is the operator's upgrade contract:**

- **Patch:** a drop-in replacement, which can be rolled back to the release before it.
- **Minor:** new features and settings. Upgrading needs nothing but a restart, existing configuration and installed skins keep working, and it can be rolled back to the minor before it, since a new record major is off until the operator turns it on (§4).
- **Major:** the operator must act, or something they rely on goes away: a setting removed or renamed, a record major written by default (§4), a REST major (§5), a template-contract or token-contract major (§9, §14), a dropped implemented contract (§6), or a new required service.

**A release's bump is derived, not chosen**: it is the largest bump among the contracts it ships. `Special:Version` and `GET /version` report the product version and beside it the version of each contract in §2 that the instance serves: the record format and payload majors, `api_version`, the template contract, the token contract (the Codex release), each installed skin and each message bundle, so the product number need not carry everything.

### 8. Skins (amends 0034 §2)

**A skin is the site's HTML presentation**: templates, the skin's own CSS, JavaScript, images and messages, and a manifest. **A theme is not a skin**: a palette and a type set set Codex's tokens inside whatever HTML the skin produces (0086 §1), and a skin consumes them. The binary answers requests, calls the API, builds each page's view model and hands it to the skin's template; the skin turns it into HTML.

- **Only the operator installs a skin.** Skins are files in a directory named by deployment configuration (`server.skins` for `triplespace-server`, `web.skins` for `triplespace-web`), read at start and on reload. No tenant, user or API call can add or change one, and a skin is trusted as operator configuration is: it may ship JavaScript, which is served from the site's origin under the policy of 0034 §11 like the site's own.
- **The manifest**, `skin.toml`: name, version, licence, and what it requires: `template = "^0.4"`, `tokens = "^2"` (the Codex token release, §14). A skin whose requirements the binary does not meet is not loaded, and the reason is logged and shown on `Special:Version`.
- **The default skin is a skin.** It lives in the repository as `skins/default/`, is embedded in both binaries with `rust-embed`, and is used where no other skin is installed or chosen. An installed skin of the same name replaces it.
- **Which skins a wiki makes available is instance policy**, expressed with the settings machinery that already exists ([0085](0085-settings-and-preferences.md) §2.1; 23 §1.4, §3.1). `skins.available` is a tenant setting, a list of installed skins, bounded by a ceiling: the tenancy switch of the same name ([0028](0028-tenancy-policy.md) §1), the installed skins a tenant may offer, `all` in every preset. The instance's default for `skins.available` is what every wiki offers until it sets its own. So an instance that locks the setting lets no tenant change it; one whose ceiling equals its default lets a tenant disable skins but add none; and one whose ceiling is wider lets a tenant make available any skin under it. A skin that is no longer installed drops out of every list.
- **Each wiki has a default skin**, `skins.default`, one of the skins it makes available, for readers who have not chosen (registry default `default`).
- **Readers choose among the wiki's available skins**: a signed-in reader with the preference `ui.skin`, per wiki unless marked **Use on every wiki** as `ui.palette` is (0086 §4), and anyone for one request with `useskin=`, as in MediaWiki. A reader not logged in has no stored choice, since a skin changes the HTML itself and a public page is cached the same for every reader; the theme's choices, which only swap stylesheets, are unaffected (0086 §5). A choice naming a skin the wiki no longer offers falls back to the wiki's default.
- The settings, the switch and the preference are registered in `settings.toml`, `tenancy.toml` and `preferences.toml`.
- **The engine is MiniJinja**: templates are parsed once when the skin is loaded, rendered with autoescape on, and have no access to the file system, the network or the environment.

### 9. The template contract (extends 0034 §2)

**The binary and a skin agree on three things**, and together they are the template contract, versioned under §1:

1. **The templates:** one per page kind (the frame, an entity page, a document page, a history, a diff, each special page and so on), by name.
2. **Their view models:** the fields each template receives, with their types. A field is added in a minor; removed, renamed or changed in a major.
3. **The skin's obligations:** what the HTML must give back to the binary and to content. Adding one is a major. They are:
   - **Forms work without JavaScript** ([0057](0057-web-tier.md) §7): each form's action, method, field names and CSRF token as the view model gives them.
   - **Content hooks:** parsed content inside `.mw-parser-output`; `skin-theme-clientpref-*` and the reader's `data-mode`, `data-contrast`, `data-text-size` and `data-vision` on `<html>` (0086 §5; [0055](0055-templatestyles-templatedata-and-page-properties.md) §6); the theme's stylesheet links in the order the view model gives them.
   - **Mount points** for the interactive components and the challenge solver, with the `data-` attributes they read (19 §5.3; [0087](0087-account-registration-and-challenges.md) §10).
   - **Fragments:** the regions `action=render` returns, by name (0057 §8), each drawn by one template the page also draws, so a region and its page cannot drift.
   - **Accessibility:** the landmarks, headings and skip link the site's axe tests check (19 §5.7).

The contract is written in `docs/api/skin-contract.md`, beside the other contracts, and tested where it is implemented: in `triplespace-ui`, against the default skin.

### 10. Functions a template may call (extends 0034 §2)

**A function does no I/O.** MiniJinja calls functions synchronously, and a page is held to four API calls (0057 A4), so every function is pure over the view model and the state the binary already holds in memory: messages, the registries and the reader's settings. A skin that needs data the view model lacks needs a template-contract change, not a function.

**MiniJinja's built-in filters and tests are available**, versioned with the crate. **Triplespace adds nine functions**, chosen for what gadgets and user scripts on Wikimedia wikis keep rebuilding: link classification (linkclassifier, MarkBlocked, markadmins), labels for bare IDs, user information, local times, sizes and deltas, extra menu links, and permission-gated controls.

| Function | Returns | In place of |
|---|---|---|
| `msg(key, …params)` | A message in the page's language, with `PLURAL`, `GENDER` and fallback, through the wiki's Interface overrides (0095 §2) | Hand-rolled i18n |
| `url(route, …params)` | A URL from a route's **name** in the routing table (0057 §3), never from a string | Hard-coded paths |
| `link(target, text?)` | A link to a page, entity, user or special page, with its classification as classes and `data-` attributes (§12); an entity's label from the view model | linkclassifier, MarkBlocked, label gadgets |
| `user(actor)` | A user link with talk and contributions links, groups and block state | User-info scripts |
| `time(ts, style)` | A time in the reader's zone and language: `datetime`, `date`, `relative`, `iso` | Local-time and clock scripts |
| `number(n, style)` | A localised number: `plain`, `bytes`, `delta` (signed, with a class for sign and size) | Prosesize, history deltas |
| `can(right, target?)` | Whether the reader holds a right, from the view model | `wgUserGroups` checks |
| `feature(name)` | Whether a wiki feature is on | Feature sniffing |
| `asset(path)` | The content-hashed URL of a file in the skin | Cache-busting |

**Codex components are the default skin's macros**, not functions: `button`, `chip`, `card`, `field`, `message` and the rest are written in the skin, so another skin may use other components without asking the binary.

### 11. Menus (extends 0034 §2)

**Menus are data.** The view model carries named menus, each a list of items with a route or URL, a message key, an icon hint and the right it needs: `header`, `user-menu`, `page-tabs`, `page-actions`, `tools`, `title-meta` and `footer`. The binary fills them from the features, the registries and the operator's list of extra links, and filters them by the reader's rights; the skin decides where each menu goes and how it looks. This is MediaWiki's `addPortletLink`, the most-used gadget API, as configuration. `title-meta` is where facts about the page appear near its title.

### 12. Link classification (extends 0034 §2)

**Every link the binary or `link()` emits says what it points to**, so that CSS and a skin's JavaScript never have to fetch it: `data-ts-target` (the normalized title or entity ID) and `data-ts-kind` (`page`, `entity`, `user`, `special` or `external`), and, as classes, MediaWiki's where MediaWiki has one (`new`, `mw-redirect`, `mw-disambig`, `extiw`) and Triplespace's own otherwise (foreign, mirrored, blocked user, bot, the user's groups). The attribute and class names are part of the template contract.

### 13. Message bundles (amends 0034 §9; extends 0095 §2)

**Messages come in bundles**, each a banana-JSON directory with its own version: the binary's (`i18n/`: errors, notifications, mail, API text) and one per skin (the skin's `i18n/`). **A skin's keys carry its name as a prefix**, `default-…`, as MediaWiki's skins prefix theirs, so a key belongs to one bundle and an Interface override (0095 §2) names it unambiguously. Switching skins leaves the overrides of the old skin's keys in place and inert. Renaming or removing a key is a major of its bundle; a new translation is a patch.

### 14. Theme tokens (extends 0086 §5)

**The token contract is Codex's**: a palette and a type set set Codex's tokens (0086 §1), so by §1 the contract's version is the release of `@wikimedia/codex-design-tokens` the binary pins (0034 §8). **The binary owns which release that is**, since it validates every palette and type set, contrast floors included, and skins declare which Codex token major they are written for (§8). Moving to a Codex major that renames or removes tokens is a product major (§7) unless the binary maps the old names onto the new ones for palettes already written.

### 15. Caching and fragments (extends 0057 §6, §8)

**The build ID names the skin.** A composed page's `ETag` already hashes the web tier's build ID, and a component already sends the build ID of the page it lives in as `X-Triplespace-UI-Build`, so that a web tier on another build answers `409` and the component reloads rather than splice in markup that does not match (03 §10.3; 20 §3.2). The build ID becomes the binary's build with the name and version of the skin that drew the page, so a skin upgrade or a change of skin is build skew like any other. A page drawn with `useskin=` is keyed by its own URL; one drawn by a signed-in reader's `ui.skin` is private, as every signed-in page is. Changing a wiki's `skins.default` or upgrading a skin purges the wiki's pages.

### 16. Reaching parity, then removing the built-in rendering

**The default skin is first written to reproduce today's HTML**, and the built-in rendering is removed only once it does.

- **`renderer`**, deployment configuration (`server.renderer`, `web.renderer`): `builtin` (until parity, the default), `skin`, or `compare`, which renders each page both ways, serves the built-in result and logs each route whose results differ.
- **Equality is of normalized DOM**: both results are parsed with `html5ever` and serialized canonically, so whitespace and attribute order do not count. Fragments are compared byte for byte.
- **The skin's escaper is `html::esc`'s**, installed as MiniJinja's escape callback, so escaping is not a source of differences.
- **The corpus** is every route of the routing table over a fixture wiki, rendered as an anonymous reader, a signed-in user and an administrator; for each content model; in a right-to-left language; in night mode; with each feature switch on and off; and on a private tenant. The Playwright and axe suite (`ui/e2e`) also runs against both renderers.
- **The order** is the frame, then entity pages, then the rest route by route. A route moves to the skin when it is equal on the corpus and has run under `compare` on a staging instance without a difference.
- **Then** `askama` and the string builders are removed, and `renderer` with them.

**The template contract stays at major 0 until a second skin**, structurally different from the default, renders every route. Equality shows the contract is enough for one skin shaped like the code it replaced; only a second skin shows it is a contract.

### 17. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `triplespace-ui` | Loads skins: the manifest, the requirement check, MiniJinja with the functions of §10, the menus of §11 and the link classification of §12; builds view models; the `compare` renderer. Gains `minijinja`, `semver` and `html5ever`; loses `askama` after §16 |
| `triplespace-web`, `triplespace-server` | Embed `skins/default/`; read the skin directory; `renderer` |
| `triplespace-api-action` | `api_version` as `MAJOR.MINOR.PATCH` (§5); the contract versions on `GET /version` (§7) |
| `triplespace-client` | Parses `api_version` and checks it by §5 |
| `xtask` | `cargo semver-checks` over the published `scatter-*` crates (§3) |
| `triplespace-accounts` | The settings `skins.available` and `skins.default`, the `skins.available` ceiling, and the preference `ui.skin` (§8) |

### 18. Releases and the version in the repository

**A version is published** when it is built as a downloadable bundle (a release binary or image), shipped to crates.io, or released by any equivalent act that marks the code as stable. Each publication is tagged in git: `vX.Y.Z` for the product, `{crate}-vX.Y.Z` for a crate published on its own, `skin-{name}-vX.Y.Z` and `i18n-{bundle}-vX.Y.Z` for a skin or message bundle released apart from the product.

- **Once something is published, its patch number goes up in the repository** at once. The repository therefore always carries a version that has not been published, and a site running directly from git reports at least one patch above the last published release; `Special:Version` also gives the build's commit and its `modified` flag (0077 §6), which say exactly what it runs.
- **A feature raises the minor, and a breaking change the major, when the change is made**, setting the numbers after it to 0: `1.4.3` becomes `1.5.0` or `2.0.0`. Under major 0, by §1, a breaking change raises the minor, and so does a feature: `0.4.3` becomes `0.5.0`.
- **Bumps since the last publication do not stack.** If the repository is already at `1.5.0` because of a feature, a second feature leaves it there; a breaking change after it moves it to `2.0.0`. Two branches that each raised the minor merge at the same number.
- **A contract published inside the product** (the API, the template contract, the record formats, the registry schemas) follows the same rules, except that its patch goes up only the first time it changes after a publication, so a release that left a contract alone does not renumber it.
- **Features and breaking changes are developed on branches**, so that `main` stays fit to publish and stable deployments are not disturbed; the version bump is made on the branch, with the change. **A fix to a published line** that `main` has moved past is made on a `release/X.Y` branch cut from that line's tag, and published from there.

The product starts at `0.1.0`: `[workspace.package]` gains `version = "0.1.0"`, which the first publication will carry.

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

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §17 | amends | 0005 A110 |
| [0005](0005-crate-organization.md) §6 | §3 | extends | 0005 A110 |
| [0015](0015-record-format-and-partition-registry.md) §2 | §4 | extends | 0015 A63 |
| [0034](0034-frontend-stack.md) §2 | §8, §9, §10, §11, §12 | amends | 0034 A14 |
| [0034](0034-frontend-stack.md) §9 | §13 | amends | 0034 A14 |
| [0057](0057-web-tier.md) §6 | §15 | extends | 0057 A9 |
| [0057](0057-web-tier.md) §8 | §15 | extends | 0057 A9 |
| [0057](0057-web-tier.md) §9 | §5 | amends | 0057 A9 |
| [0077](0077-special-version.md) §6 | §7 | extends | 0077 A4 |
| [0086](0086-theming.md) §5 | §14 | extends | 0086 A7 |
| [0095](0095-the-interface-namespace.md) §2 | §13 | extends | 0095 A3 |

## References

- [Semantic Versioning 2.0.0](https://semver.org/)
- [The Cargo Book: SemVer compatibility](https://doc.rust-lang.org/cargo/reference/semver.html), and `cargo-semver-checks`
- MediaWiki: `SkinMustache` and its template data, `skin.json` and its `requires`, and `mw.util.addPortletLink`
- Gadgets and user scripts on English Wikipedia and Wikidata: linkclassifier, MarkBlocked, markadmins, navigation popups, reference tooltips, comments in local time, prosesize
- [MiniJinja](https://github.com/mitsuhiko/minijinja)
- Codex's release policy for `@wikimedia/codex` and `@wikimedia/codex-design-tokens`
