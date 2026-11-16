# 0077. Special:Version

- **Status:** Proposed
- **Date:** 2026-10-07
- **Updated:** 2026-11-15 (A8)
- **Author:** James Hare / Claude Opus
- **Uses:** [0005](0005-crate-organization.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0028](0028-tenancy-policy.md), [0039](0039-files-and-media.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0043](0043-lua-modules.md), [0046](0046-primary-tenant.md), [0056](0056-security-model.md), [0057](0057-web-tier.md), [0059](0059-query-service.md), [0075](0075-mcp-server.md)

## Context

[0047](0047-special-pages.md) serves `Special:Version` in a tenant form and a farm form (§3), and says only that it is built "from the registry, configuration and i18n" (§9). The registry entry pointed at [0016](0016-permissions-and-access-control.md) §7, which does not mention the page; 0016 A11 requires only that `instance.owner_of_record` is shown on it.

MediaWiki's `Special:Version` has a credit line naming its developers and "others", the GPL notice with the licence text at `Special:Version/License`, the full credits at `Special:Version/Credits`, then installed software (MediaWiki, PHP, the database server and its version, the search backend), entry point URLs, installed extensions and skins, parser extension tags and function hooks, and installed libraries. Tools read several of these sections, and `meta=siteinfo` reports the same data as `siprop=extensions`, `siprop=libraries` and `general.dbversion`.

Four facts shape the answer:

- **Git authors are not contributors.** Of the repository's commits, 77 have "Claude" as the git author, and 118 carry a `Co-Authored-By` trailer naming a model. A contributor list built from git history would name AI agents as contributors and could not tell who has signed the contributor licence agreement that the dual-licensed `scatter-*` crates need ([0005](0005-crate-organization.md) §6).
- **`origin` is recorded in three registries,** not one: `special-pages.toml`, `wikitext-functions.toml` and `content-models.toml`. Between them they name 32 extensions. They disagreed on spelling: `content-models.toml` wrote `wikibase` where `special-pages.toml` writes `Wikibase`, and `wikitext-functions.toml` wrote `timeline` and `wikihiero` for the extensions named EasyTimeline and WikiHiero.
- **`siprop=extensions` is read by clients to choose APIs.** Pywikibot's `has_extension()` reads it. The server reports `WikibaseRepository` today because the Wikibase clients need it ([docs/clients.md](../clients.md); [0033](0033-backend-stack.md) A12).
- **The binary is the conveyed work.** Triplespace is GPL-3.0-or-later, so the binary, the OCI image, and the JavaScript and wasm sent to browsers must each come with, or point to, their corresponding source (0033 §16). The third-party crates and packages linked into them carry licences, such as MIT, BSD and the SIL Open Font License of the default theme's typefaces, whose notices must be reproduced with the binary.

### Direction

James's direction, from the design discussion of 2026-10-07:

- **"I want the top line to be 'This wiki is powered by Triplespace, developed by Scatter LLC in Portland, Oregon.' If there were other contributors (there isn't yet), it would continue, ', with contributions from Alice, Bob...' followed by the usual GPL spiel."**
- **"From there, it should document the software services deployed (Postgres, QLever, OpenSearch, Parsoid, Valkey, things like that). Then it should document the crates in use, followed by the MediaWiki extensions that inspired features (as tracked in the special pages TOML file that tracks which extension is responsible for what special pages), and then finally, AI agents used (just the ones named in the ADR files so far)."**

And on the first proposal, the same day:

- **"I was thinking the contributors would be part of a TOML file, yes."**
- **Extension lineage comes from "everything that carries origin"**, not only the special pages.
- **"'Inspired by' does not mean 'installed'."**
- **The service version policy proposed (public by default, an instance setting to reduce it) is accepted.**
- **Funders:** none fund Triplespace today, but the credits should plan for them.
- **0033 and 0034 were written by Claude Opus or Claude Fable**; which one is not recorded.
- **The tenant form includes the tenant's own features.**

## Decision

### 1. One page, two forms (amends 0047 §9)

*Current text: [21](../architecture/21-special-pages.md) §5.1, §5.6, §8.*

### 2. The credit line, contributors and funders

*Current text: [21](../architecture/21-special-pages.md) §5.2.*

### 3. The licence notice and licence texts

*Current text: [21](../architecture/21-special-pages.md) §5.3.*

### 4. Installed software and services

*Current text: [21](../architecture/21-special-pages.md) §5.4.*

### 5. Entry points

*Current text: [21](../architecture/21-special-pages.md) §5.5.*

### 6. Crates and third-party components

*Current text: [21](../architecture/21-special-pages.md) §5.6; [22](../architecture/22-crates-and-stack.md) §7.2.*

### 7. Wikitext

*Current text: [21](../architecture/21-special-pages.md) §5.7.*

### 8. Inspired by

*Current text: [21](../architecture/21-special-pages.md) §5.8.*

### 9. AI agents

*Current text: [21](../architecture/21-special-pages.md) §5.9.*

### 10. API (extends 0012 §4 and §5)

*Current text: [18](../architecture/18-api.md) §2.2, §3.2, §8.*

### 11. Features

*Current text: [21](../architecture/21-special-pages.md) §5.10.*

### 12. The checker

*Current text: [22](../architecture/22-crates-and-stack.md) §7.3.*

### 13. Settings (extends 0015 §3)

*Current text: [23](../architecture/23-configuration-and-registry.md) §3.1, §3.3, §5.2.*

### 14. The registry file (extends 0015 §5)

*Current text: [23](../architecture/23-configuration-and-registry.md) §4.1, §4.6.*

### 15. The component manifest (extends 0033 §16)

*Current text: [22](../architecture/22-crates-and-stack.md) §7.1.*

## Alternatives considered

- **Contributors from git history.** It would credit AI agents as contributors, could not tell who has signed the contributor licence agreement, and names people as their git configuration does, not as they wish to be credited.
- **Lineage from `special-pages.toml` alone.** It misses ParserFunctions, Cite, TemplateStyles, Kartographer and every other extension whose trace is a wikitext function or a content model.
- **Reporting the lineage in `siprop=extensions`.** Clients would take it as installed and call modules that do not exist.
- **`cargo-about` as the whole manifest.** It collects crate licences well, but not npm packages or vendored code, and not the build facts. It may be used inside `cargo xtask manifest` for the crate part.
- **Translating the licence text.** The FSF's translations are unofficial and say so; the notice is translated, the licence is not.
- **Hiding service versions by default.** MediaWiki shows them, tools expect `dbversion`, and an operator who disagrees has `version.services`.

## Consequences

- **The page has one source for each fact,** and only the facts nothing else records are written by hand, in `version.toml`.
- **Every build carries its licence and its third-party notices,** whatever was shipped beside the binary, and points to its source.
- **"Inspired by" credits MediaWiki's extension authors without claiming their software,** and `siprop=extensions` stays a list clients can act on.
- **A new contributor is a registry edit with a CLA date,** reviewed like any other.
- **AI involvement is stated where readers look for credits,** and the checker keeps it from drifting from the ADRs.
- **A release needs `cargo xtask manifest`.** A build that skips it says so on the page.

## Open questions

- **Q1. Version probes for QLever and Parsoid.** Deferred by direction: both are shown without a version until a probe is chosen for each.
- **Q2. JavaScript licence labels.** Whether to add LibreJS-style labels, a table of every script with its licence and source, beside the header comments of §3.
- **Q3. Agents and their ADRs.** *Withdrawn: the page names no ADRs (§1).*
- **Q4. Translators.** If messages are translated on translatewiki.net, whether translators are credited on `Special:Version/Credits`, as MediaWiki credits them through each message file's `@metadata`.

## References

- [Special:Version on mediawiki.org](https://www.mediawiki.org/wiki/Special:Version), and its [Credits](https://www.mediawiki.org/wiki/Special:Version/Credits) and [License](https://www.mediawiki.org/wiki/Special:Version/License) subpages
- [API:Siteinfo](https://www.mediawiki.org/wiki/API:Siteinfo): `extensions`, `libraries`, `general.dbtype` and `dbversion`
- [How to use GNU licenses for your own software](https://www.gnu.org/licenses/gpl-howto.html): the notice of §3
- [Unofficial translations of the GPL](https://www.gnu.org/licenses/translations.html)
- [SIL Open Font License](https://openfontlicense.org/): the licence of IBM Plex and Newsreader
- [Pywikibot `APISite.has_extension`](https://doc.wikimedia.org/pywikibot/stable/api_ref/pywikibot.site.html)
- [cargo-about](https://github.com/EmbarkStudios/cargo-about)

## Amendment log

### A1. No ADRs on the page; QLever and Parsoid versions deferred

- **Date:** 2026-10-07
- **Source:** Direct: James, review of the draft, 2026-10-07
- **Change:** extends §1; amends §4, §8, §11
- **Summary:** "I don't think it's necessary to mention ADRs directly in Special:Version." The page names no ADRs: the services table loses its "Specified in" column, features are no longer linked to their ADRs, and an `inspired_by` entry shows what was taken but not its ADR. The `adr` fields in `version.toml` stay, for maintainers. Showing QLever's and Parsoid's versions is deferred: both are shown without one (Q1). Q3 is withdrawn.

Replaced text: §4 of this ADR; §8 of this ADR; §11 of this ADR.

### A2. Services not configured are not shown

- **Date:** 2026-10-07
- **Source:** Direct: James, review of the draft, 2026-10-07
- **Change:** amends §4
- **Summary:** "If a service is not configured, it shouldn't be shown." §4 lists only the services the instance is configured to use and the embedded components in use; a configured service is `connected` or `unreachable`.

Replaced text: §4 of this ADR.

### A3. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§15

### A4. Contract versions and skins

- **Date:** 2026-10-29
- **Source:** [0097](0097-versioning-and-skins.md) §7
- **Change:** extends §6
- **Summary:** Beside the product version, `Special:Version` and `GET /version` give the version of each contract the instance serves (record format and payload majors, `api_version`, the template contract, the Codex token release, each message bundle) and each installed skin with its version, licence and requirements, with any skin not loaded and the reason. The chapter text is [21](../architecture/21-special-pages.md) §5.6.

### A5. Licence files over REST

- **Date:** 2026-10-30
- **Source:** Direct: James, review of 2026-10-30, confirming the implementation calls of 2026-10-23 and 24 that the catalogue cross-checks surfaced
- **Change:** extends §10
- **Summary:** `GET /version/licenses/{id}` serves one component's licence, copyright and notice files, the route behind `Special:Version/License` (21 §7); it is in the OpenAPI contract and in `registry/routes.toml`.

### A6. Licensing: loose ends from building Special:Version

- **Date:** 2026-10-30
- **Source:** Direct: Claude, while building Special:Version, 2026-10; unconfirmed
- **Change:** extends §11
- **Summary:** Logged from the working notes on their retirement (0099 §5, 24 §15); each call stands as decided until James confirms or overturns it, and the chapter text that states it is folded on confirmation where it is not already there. The calls:
  - `OFL-1.1` was added to `deny.toml` provisionally and is not recorded in 0033; record it there or give the font licence its own ADR.
  - Whether to narrow the "Inspired by" credits.

### A7. No commercial licence

- **Date:** 2026-11-07
- **Source:** Direct: James, design discussion of 2026-11-07
- **Change:** amends §2, §3, §6
- **Summary:** With the commercial licence withdrawn ([0005](0005-crate-organization.md) A114), a contributor entry no longer carries the date of a contributor licence agreement, the sentence after the licence notice says only that the embedded documentation and registry data are CC0 (the message `ts-version-cc0` replaces `ts-version-dual-and-cc0`), and a `scatter-*` crate is no longer marked as also available commercially (`ts-version-commercial-mark` and the manifest's `commercial` field are removed).

Replaced text: [21](../architecture/21-special-pages.md) §5.2, §5.3; [22](../architecture/22-crates-and-stack.md) §7.2.

### A8. The licence header on built JavaScript and CSS

- **Date:** 2026-11-15
- **Source:** Direct: Claude, while building the `@license` headers (SCA-273), 2026-11-15; unconfirmed
- **Change:** extends §3
- **Summary:** Every JavaScript and CSS file the front-end build writes opens with `/*! @license GPL-3.0-or-later. Triplespace; licence texts and source: Special:Version */`. The calls: one header for a whole file, although a bundled file also holds third-party code under its own licence, which `Special:Version` lists with its texts; the comment opens with `/*!`, which minifiers keep; it is written after the files are named, so a file's hashed name does not cover it; font files, the manifest and the package list carry none. Not built: the header on wasm files, which the front-end build does not write. Chapter text: 21 §5.3.
