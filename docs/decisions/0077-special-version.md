# 0077. Special:Version

- **Status:** Proposed
- **Date:** 2026-10-07
- **Author:** James Hare / Claude Opus
- **Changes:** [0012](0012-api-requirements.md), [0015](0015-record-format-and-partition-registry.md), [0033](0033-backend-stack.md), [0047](0047-special-pages.md)
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

**`Special:Version` has these sections, in this order:**

1. The credit line and licence notice (§2, §3).
2. Installed software: Triplespace's build, then the services and embedded components (§4).
3. Features: the tenant's on its form, the instance's on the farm form (§11).
4. Entry points (§5).
5. Triplespace crates, then third-party components (§6).
6. Wikitext: extension tags, parser functions, variables and switches (§7).
7. Inspired by: MediaWiki and its extensions (§8).
8. AI agents (§9).
9. The instance: its name, and its owner of record where `instance.owner_of_record` is set (0016 A11).

Subpages: `Special:Version/Credits` (§2), `Special:Version/License` and `Special:Version/License/{component}` (§3).

**The tenant form and the farm form** differ only in §11's features and in §5's entry points, which are the tenant's at a tenant base and the instance's at the farm base. Where the two bases share a host, both forms show behind 0047 §3's `scope` filter.

**What is listed and what is derived.** `docs/registry/version.toml` (§14) holds what nothing else records: the developer, contributors, funders, AI agents, services, feature switches and extension pages. Everything else is derived: the build and its components from the component manifest (§15), lineage from the registries' `origin` fields, wikitext from `wikitext-functions.toml`, and features from configuration.

**The page is rendered by `triplespace-ui` from `GET /version`** (§10), since the site's crates reach the instance only through its API ([0005](0005-crate-organization.md) §3 rule 10). Where `triplespace-web` serves the site, it adds its own build to §4, so a web tier and an API server built from different commits are both shown.

### 2. The credit line, contributors and funders

**The credit line** is one sentence:

> This wiki is powered by [Triplespace](https://github.com/scatterbase/triplespace), developed by Scatter LLC in Portland, Oregon, with contributions from Alice, Bob and others.

- **The product, developer and place are message parameters,** from `[developer]` in `version.toml`, so translators never translate them. The messages are `ts-version-poweredby` (no contributors) and `ts-version-poweredby-contributors`.
- **Contributors are `[[contributor]]` entries in `version.toml`,** in order of first merged contribution. Each has a name as its holder wishes to be credited, the date of the first merged contribution, and the date the contributor licence agreement was signed; nothing is merged without one (0005 §6). Git history is not read.
- **The line names the first ten.** With more, it ends "and [[Special:Version/Credits|others]]", as MediaWiki's does. The list is formatted by the reader's language's list rules.
- **`Special:Version/Credits` lists every contributor**, then every funder, then the AI agents of §9.
- **Scatter LLC is the developer, not a contributor,** and AI agents are never contributors: they sign no agreement and are credited separately (§9).

**Funders are `[[funder]]` entries**: a name, what the support was for, and the dates it began and, if it has, ended. When there is at least one, a second sentence follows the credit line: "Development has been supported by {funders}." (`ts-version-funders`). A funder whose support has ended stays listed. There are none today.

### 3. The licence notice and licence texts

**The notice follows the credit line.** It is the FSF's recommended notice for GPLv3-or-later, with "Triplespace" for "this program", in the message `ts-version-license-info`:

> Triplespace is free software; you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation; either version 3 of the License, or (at your option) any later version.
>
> Triplespace is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
>
> You should have received [[Special:Version/License|a copy of the GNU General Public License]] along with this program; if not, see https://www.gnu.org/licenses/.

MediaWiki's `version-license-info` is not reused, because it names version 2.

Two more sentences follow: the `scatter-*` crates are also available from Scatter LLC under a commercial licence (0005 §6), and the documentation and registry data the binary embeds are dedicated to the public domain under CC0 1.0 (`docs/LICENSE`).

**The licence text is in the binary.** The repository root's `LICENSE` is embedded at build time (`include_str!`), so every build carries its own copy, whatever was or was not shipped beside it.

- **`Special:Version/License` serves it verbatim, in English only,** in a `<pre>` block with no interface translation. The FSF treats only the English text as the licence; the notice above is translated like any message.
- **`Special:Version/License/{component}`** serves a third-party component's licence texts and copyright notices from the manifest (§15): the texts a crate or package ships, not a generic text for its SPDX identifier. Each row of §6 links to its component's page.

**The source.** §4's build row links to the corresponding source:

- For a build from a clean tree, `{developer.source}/tree/{commit}`.
- `instance.source_url` (§13) replaces it, for an operator who publishes a modified build elsewhere.
- A build from a modified tree (the manifest's `modified` flag) without `instance.source_url` shows "modified build; source location not configured" to everyone, and `triplespace instance check` warns. The JavaScript and wasm sent to browsers are conveyed to every reader, so their source must be offered whatever the server's licence would otherwise require of a hosted service (0033 §16).
- The built JavaScript, CSS and wasm files carry a header comment with `@license GPL-3.0-or-later` and the path `Special:Version`.

### 4. Installed software and services

**The build row** shows Triplespace's version, the commit, whether the build is modified, the commit's date (not the build time, so builds stay reproducible), the rustc version, the target and the cargo features compiled in, from the manifest (§15). Where `triplespace-web` serves the site, a second row shows the web tier's build (§1).

**Then each service and embedded component** in `version.toml`'s `[[service]]` list, with:

| Column | Shows |
|---|---|
| Name and role | From the registry |
| State | `connected`, `configured but unreachable`, or `not configured`. An embedded component shows `in use` or `not in use` |
| Version | Read by the registry's `version` probe for a service (`SHOW server_version` for PostgreSQL; Valkey's `INFO server`; OpenSearch's `GET /`), from the manifest for an embedded component (Oxigraph), or a fixed string (Lua 5.1) |
| Specified in | The ADR |

The services are PostgreSQL, Valkey, OpenSearch, QLever, Parsoid and object storage, and the embedded components are Oxigraph and Lua; adding a service to the stack adds a registry entry.

**Probes run at startup and with each health check** and are cached in the process. Rendering the page never contacts a service, so it cannot be used to make the instance probe its services on demand.

**Disclosure** is the instance setting `version.services` (§13):

- **`full`** (default), as MediaWiki shows its database server's version to everyone;
- **`names`**: names, roles and states, without versions;
- **`off`**: the build row only.

Holders of `ts-config` at the farm base always see `full`. Whatever the setting, the page never shows a host, port, URL, bucket, index name, database name or credential.

### 5. Entry points

**The URLs a client needs, as MediaWiki's "Entry point URLs" lists them:** the article path, script path, `api.php` and `rest.php`, then Triplespace's: the `triplespace/v0` REST base, `/entity/` and `Special:EntityData`, the update stream (0032), the SPARQL endpoint while `query.enabled` is on ([0059](0059-query-service.md) §6), the MCP endpoint while `mcp.enabled` is on ([0075](0075-mcp-server.md) §1), and the OAuth endpoints (0025). Each is shown only where it is served, and at the base that serves it.

### 6. Crates and third-party components

**Triplespace crates:** each workspace crate linked into the binary, with its version and licence. A `scatter-*` crate is marked as also available under a commercial licence (0005 §6).

**Third-party components,** grouped and sorted by name, each with its version, licence expression, upstream URL and a link to its licence page (§3):

- **Crates:** the binary's dependencies for its target, as linked: normal dependencies only, not build or development dependencies.
- **Frontend packages:** the packages in `ui/package-lock.json` reachable from `dependencies` (Codex, Vue and the typefaces), not `devDependencies`, since only those reach the browser.
- **Vendored code:** source copied into the tree, such as Scribunto's and WikibaseClient's Lua in `triplespace-scribunto` (0043 §4) and `parse-wiki-text-2` in `scatter-wikitext` (0033 §9), each with its upstream and revision.

Native code that a `-sys` crate bundles (Lua 5.1 through `mlua`, RocksDB through Oxigraph, `aws-lc`) is covered by its crate's licence files, which the manifest collects whole.

### 7. Wikitext

**Extension tags, parser functions, variables and behaviour switches,** from `wikitext-functions.toml`, as MediaWiki's "Parser extension tags" and "Parser function hooks" list them. Each shows its origin and status (`implemented`, `chip` or `ignored`) and, on the tenant form, whether it is active there, given the settings its `requires` and `enabled_by` name ([0042](0042-template-expansion-and-parsoid.md) §2).

### 8. Inspired by

**Triplespace runs no MediaWiki extension.** This section lists the extensions whose features, names or APIs it implements, so that their users, tools and pages work here. It is headed "Inspired by", introduced by `ts-version-inspiredby-intro`, and never says "installed".

**It is derived from every registry that records an `origin`**: `special-pages.toml`, `wikitext-functions.toml` and `content-models.toml`. MediaWiki core comes first, then the extensions in alphabetical order. Each row links to the extension's page on mediawiki.org (`[[extension]]` in `version.toml`) and lists what came from it, by kind:

- **Special pages** that are `served`;
- **Content models** that are `implemented`;
- **Wikitext** entries, `implemented` ones first, then those that are only recognized (`chip` or `ignored`), labelled as such.

Entries that are `deferred`, `declined` or `reserved` are not shown. An extension with no shown entry is not listed.

**Extensions that leave no `origin`** in any registry, but from which Triplespace took something, carry `inspired_by` in `version.toml`: what was taken, and the ADR. The seed is CirrusSearch and WikibaseCirrusSearch (search fields and keywords, [0014](0014-caches-and-search.md)), CodeMirror (the source editor, 0034 §4) and DiscussionTools (stable names for sections and comments, 0069). An ADR that takes something from an extension adds an `origin` to the registry it fills or, failing that, an `inspired_by` entry.

**`origin` values** are `mediawiki`, `triplespace`, `generic` (content models only) or an extension's name as `version.toml` lists it. The three registries now agree: `wikibase` became `Wikibase` in `content-models.toml`, and `timeline` and `wikihiero` became `EasyTimeline` and `WikiHiero` in `wikitext-functions.toml`.

### 9. AI agents

**The AI agents named in the ADRs' Author lines,** from `[[agent]]` in `version.toml`: Claude Opus, Claude Fable and Claude Sonnet, made by Anthropic. Each shows the model versions recorded in commit trailers, where there are any (Claude Opus 5.5 and Claude Fable 5.1; none for Claude Sonnet). The section is introduced by `ts-version-agents-intro`: "Triplespace's design records and code were written with these AI agents."

- **The list is the ADRs' Author lines,** not the commit history, and the checker keeps them in step (§12).
- **0033 and 0034 name "Claude" without the model.** They were written by Claude Opus or Claude Fable; `[agent_attribution]` records them as unattributed rather than guessing.

### 10. API (extends 0012 §4 and §5)

**`GET /version`** under `rest.php/triplespace/v0` returns everything the page shows, as one JSON document, at each tenant base and at the farm base. It is public, cached like any public response ([0014](0014-caches-and-search.md)), and subject to `version.services`. On a private tenant ([0056](0056-security-model.md) §3), an outsider gets what the farm base would show, without the tenant's features or entry points, as with 0056 §13's reduced `meta=siteinfo`.

**`meta=siteinfo`:**

- **`siprop=extensions` lists only extensions whose API Triplespace serves.** An extension is listed when at least one module that [mediawiki-compat.md](../api/mediawiki-compat.md) attributes to it is served: `WikibaseRepository` today, and `WikibaseClient`, `Echo`, `AbuseFilter`, `OAuth`, `TemplateData` and the others as their modules are built. Each entry carries Triplespace's version, URL and licence, with `Triplespace` itself as a last entry of type `other`. §8's lineage is not reported here: a client that finds an extension listed will call its modules.
- **`siprop=libraries`** lists §6's third-party crates and frontend packages as `{name, version}`, as MediaWiki lists its Composer and npm libraries.
- **`general.dbtype`** is `postgres`, and `general.dbversion` is PostgreSQL's version unless `version.services` is `names` or `off`, in which case it is absent. `generator` is unchanged ([docs/clients.md](../clients.md)).
- **`siprop=triplespace`** gains `build`: the version, commit and `modified` flag.

### 11. Features

**The tenant form shows the tenant's feature switches:** the value of each `[[feature]]` setting of scope `tenant` in `version.toml` (`content.licence`, `wikitext.expansion`, `wikitext.lua`, `wikitext.renderer`, `pages.repos`, `search.inherited`, `query.enabled`, `mcp.enabled` and `proposals.push`), each linked to its ADR. It also shows the providers the tenant has opted into, linked to `Special:Providers`, its tenancy preset ([0028](0028-tenancy-policy.md) §1), and whether it is a private tenant (0056 §3).

**The farm form shows the instance's:** the settings of scope `instance` (`search.farm_wide` and `files.separation`), the tenancy policy's preset and the number of tenants.

An ADR that adds a setting turning a feature on or off adds it to `[[feature]]`.

### 12. The checker

`check_adrs.py` gains two checks:

- **Check 9.** Every agent an ADR's Author line names ("James Hare / Claude Opus", and "revision by James Hare / Claude Fable") is an `[[agent]]` in `version.toml`, and every `[[agent]]` is named by at least one ADR. An Author line naming only "Claude" is accepted for the ADRs in `[agent_attribution].unattributed` and reported for any other.
- **Check 10.** Every `origin` in the three registries is `mediawiki`, `triplespace`, `generic` or the `name` of an `[[extension]]`; no two names differ only in case; and every `[[extension]]` is named by an `origin` or carries `inspired_by`.

### 13. Settings (extends 0015 §3)

Two instance `site` settings, written with `ts-config` at the farm base:

| Setting | Values | Default |
|---|---|---|
| `version.services` | `full`, `names`, `off` (§4) | `full` |
| `instance.source_url` | A URL for the corresponding source of this build (§3) | Unset: `{developer.source}/tree/{commit}` for an unmodified build |

### 14. The registry file (extends 0015 §5)

**`docs/registry/version.toml`** lists the developer, contributors, funders, AI agents and their attribution, services, feature switches and extensions, as §2, §4, §8, §9 and §11 describe. It is embedded by `triplespace-api-rest`, like the other registries ([0015](0015-record-format-and-partition-registry.md) §5), and is CC0 like the rest of `docs/`.

### 15. The component manifest (extends 0033 §16)

**`cargo xtask manifest`** writes the component manifest that §3, §4 and §6 read:

- the build: version, commit, `modified` (a dirty tree), the commit date, rustc, target and features;
- the workspace crates and the third-party crates linked into each binary, from `cargo metadata` for the binary's target with normal dependencies only;
- the frontend packages reachable from `ui/package.json`'s `dependencies`, from `ui/package-lock.json`;
- vendored code, from a `vendor.toml` beside each vendored tree (name, upstream, revision, licence);
- for each component, its licence expression and the licence, copyright and notice files it ships (`LICENSE*`, `COPYING*`, `NOTICE*`, `AUTHORS*`), deduplicated by content.

**It shares `cargo-deny`'s allowlist** (0033 §1): a component whose licence is not on it fails the manifest as it fails `cargo deny`.

**`triplespace-server` and `triplespace-web` embed it** at build time. A build without one, such as an ordinary development build, embeds a stub, and §6 says "development build: components not listed". Release builds and the OCI image fail without a manifest.

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

- **Q1. Version probes for QLever and Parsoid.** Which endpoint, if any, reports each one's version; until one is chosen, they are shown without a version.
- **Q2. JavaScript licence labels.** Whether to add LibreJS-style labels, a table of every script with its licence and source, beside the header comments of §3.
- **Q3. Agents and their ADRs.** Whether each agent's row should list the ADRs it wrote, which would put the Author lines' data into `version.toml` or the manifest.
- **Q4. Translators.** If messages are translated on translatewiki.net, whether translators are credited on `Special:Version/Credits`, as MediaWiki credits them through each message file's `@metadata`.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0012](0012-api-requirements.md) §4 | §10 | extends | 0012 A55 |
| [0012](0012-api-requirements.md) §5 | §10 | extends | 0012 A55 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §13 | extends | 0015 A36 |
| [0015](0015-record-format-and-partition-registry.md) §5 | §14 | extends | 0015 A36 |
| [0033](0033-backend-stack.md) §16 | §15 | extends | 0033 A13 |
| [0047](0047-special-pages.md) §9 | §1 | amends | 0047 A12 |

## References

- [Special:Version on mediawiki.org](https://www.mediawiki.org/wiki/Special:Version), and its [Credits](https://www.mediawiki.org/wiki/Special:Version/Credits) and [License](https://www.mediawiki.org/wiki/Special:Version/License) subpages
- [API:Siteinfo](https://www.mediawiki.org/wiki/API:Siteinfo): `extensions`, `libraries`, `general.dbtype` and `dbversion`
- [How to use GNU licenses for your own software](https://www.gnu.org/licenses/gpl-howto.html): the notice of §3
- [Unofficial translations of the GPL](https://www.gnu.org/licenses/translations.html)
- [SIL Open Font License](https://openfontlicense.org/): the licence of IBM Plex and Newsreader
- [Pywikibot `APISite.has_extension`](https://doc.wikimedia.org/pywikibot/stable/api_ref/pywikibot.site.html)
- [cargo-about](https://github.com/EmbarkStudios/cargo-about)
