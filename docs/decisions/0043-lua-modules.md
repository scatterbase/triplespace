# 0043. Lua modules

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-11-07 (A10)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0033](0033-backend-stack.md), [0034](0034-frontend-stack.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md)
- **Uses:** [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0026](0026-sitelinks.md), [0038](0038-page-metadata-and-categories.md), [0044](0044-tenant-relative-ids.md)

## Context

[0042](0042-template-expansion-and-parsoid.md) makes template expansion native. The templates that matter most on Wikimedia wikis are thin wrappers around **Lua modules**, run by the Scribunto extension through `{{#invoke:Module|function|…}}`: citation templates, infoboxes, navboxes, coordinates, dates, and nearly everything that reads Wikidata. Without Lua, expansion stops at the first `#invoke`.

**What Scribunto is.** Scribunto runs modules in Lua 5.1 inside a sandbox, LuaSandbox or a standalone interpreter, with limits on memory and CPU. Its libraries are mostly **Lua code** (`mw.lua`, `mw.text`, `mw.title`, `mw.html`, `mw.ustring` and the rest), which call into a table of **PHP callbacks** for anything that needs the wiki: frames, titles, page content, messages, language rules. The LuaStandalone engine already runs that split across a pipe, so the boundary is well defined. On a client wiki, WikibaseClient adds `mw.wikibase` in the same way, and the parser functions `{{#property:}}` and `{{#statements:}}`.

So the contract a reimplementation has to meet is **Scribunto's Lua libraries, unchanged, with the PHP side replaced**.

**Bare entity IDs are the hard part for Triplespace.** Modules from Wikipedia hard-code Wikidata IDs: `P31`, `Q5`, `P569`. In Triplespace a bare `Q5` is a *local* ID ([0017](0017-entity-id-grammar.md) §1), and Wikidata's Q5 is `WDQ5`. A module written for Wikipedia that runs on a Triplespace mirror of Wikipedia would read the wrong entities.

James's direction, from the design discussion of 2026-09-30:

- **The Module namespace and its talk namespace are enabled when Lua is enabled.**
- **Each tenant configures what `P`, `Q` and the other letters mean in the context of Lua.** A Wikipedia mirror, for example, will want them to mean Wikidata.
- **As a supplement, `QQQ`, `PPP` and so on always mean the tenant's own data.** This is the tenant-relative form of [0044](0044-tenant-relative-ids.md), which is also useful wherever someone needs three-letter prefixes across the board.

## Decision

### 1. Lua is a tenant setting on top of expansion (extends 0042 §2)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §2.2, §7.1.*

### 2. The Module namespace (amends 0008 §2)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.2.*

### 3. The `Scribunto` content model (extends 0041 §3; amends 0008 §5)

*Changed by A5.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.3.*

### 4. The runtime: Lua 5.1 in a sandbox (extends 0033 §1)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.4.*

### 5. The Scribunto contract: Scribunto's Lua, Triplespace's host

*Changed by A3, A9.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.5.*

### 6. Data modules (extends 0014 §4)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.6.*

### 7. Wikibase Client: `mw.wikibase`, `#property` and `#statements`

*Changed by A4.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §2.2, §7.7.*

### 8. The Lua ID space, per tenant (uses 0017 §1 and 0044 §1)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §5.4, §6.*

### 9. The client site (uses 0026 §2 and 0038 §6)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.8.*

### 10. Usage tracking and invalidation (extends 0042 §10)

*Changed by A8.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §4.2, §4.4, §7.9.*

### 11. Entities the tenant does not hold

*Changed by A6.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.10.*

### 12. Errors, limits and tracking categories

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.11.*

### 13. Editing modules (extends 0034 §7)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.12.*

### 14. Licensing (extends 0033 §1 and 0005 §3)

*Changed by A10.*

*Current text: [22](../architecture/22-crates-and-stack.md) §1.2, §3.3.*

### 15. Conformance

*Current text: [22](../architecture/22-crates-and-stack.md) §4.9.*

### 16. API (extends 0012 §4)

*Current text: [18](../architecture/18-api.md) §2.2, §2.3.*

### 17. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **Wikipedia's modules can run here,** against mirrored Wikidata, with nothing changed in the modules: bare IDs mean what their authors meant, by tenant configuration.
- **A tenant's own entities are never out of reach,** whatever `lua.ids` says, through the tenant-relative form.
- **The Scribunto contract is met by running Scribunto's own Lua.** What is reimplemented is the host side, which is smaller and better specified than the libraries.
- **Lua is the most expensive thing a page can do.** The ceiling, the limits and aspect-matched invalidation are what keep it affordable; a farm operator can turn it off.
- ~~**The Lua runtime is GPL-only.** Scatterbase cannot take it under the commercial licence.~~ *There is no commercial licence; every crate is GPL-3.0-or-later (A10).*
- **Unheld entities are silent misses.** A partial mirror makes some module output incomplete until a sync brings the entity in, and `render_state` makes that visible.

## Open questions

- **Q1. The debug console** (`action=scribunto-console`), and where its sessions live.
- **Q2. Fetching unheld entities on demand** (§11): a bounded, rate-limited fetch into the mirror, against 0012 §6's rule that live fetches are never written to the log.
- **Q3. `convertGrammar` and `gender` coverage,** which MediaWiki implements per language in code.
- **Q4.** ~~**Lexemes.** `mw.wikibase.lexeme` waits for the Lexeme namespace (146) to be implemented.~~ *Settled by [0066](0066-lexemes.md) §9: provided as the extension documents it.*
- **Q5. `mw.ext.data`** (Commons tabular and map data), which many Wikipedia modules use for lookup tables.
- **Q6. LuaJIT** for speed, if a differential run shows it is safe for real modules.

## References

- [Extension:Scribunto](https://www.mediawiki.org/wiki/Extension:Scribunto) and its [Lua reference manual](https://www.mediawiki.org/wiki/Extension:Scribunto/Lua_reference_manual)
- Scribunto `extension.json` (licence, namespaces 828/829, engine defaults) and `includes/Engines/LuaCommon/LuaEngine.php` (the `mw` callbacks and `LIBRARY_SPECS`)
- [Extension:Wikibase Client/Lua](https://www.mediawiki.org/wiki/Extension:Wikibase_Client/Lua); WikibaseClient `client/includes/DataAccess/Scribunto/mw.wikibase.lua`
- [Wikibase usage tracking](https://doc.wikimedia.org/Wikibase/master/php/docs_topics_usagetracking.html)
- [mlua](https://github.com/mlua-rs/mlua); [Lua 5.1 reference manual](https://www.lua.org/manual/5.1/)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-30
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §17
- **Summary:** 0005 §2 is the one crate table CI checks, and carries `triplespace-scribunto` and every change this section listed (0005 A43).

Replaced text: §17 of this ADR.

### A2. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §17
- **Summary:** A1 was folded into the Decision. The open questions were numbered, and §14's heading now says it extends 0005 §3, as 0005 A43 records. No decision changed. Before this, A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A3. `getContent` may not widen

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §6
- **Change:** extends §5
- **Summary:** `mw.title.getContent` and `exists` treat a page the rendering page may not include, under the flow rule, as missing.

### A4. `mw.wikibase.lexeme`

- **Date:** 2026-10-05
- **Source:** [0066](0066-lexemes.md) §9
- **Change:** extends §7
- **Summary:** The lexeme library, as WikibaseLexeme documents it; Q4 settled.

### A5. `sanitized-css` is no longer excluded

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §3
- **Summary:** [0008](0008-namespaces-and-document-pages.md) §5's exclusion now covers `css` and `javascript` only: `sanitized-css` is implemented ([0055](0055-templatestyles-templatedata-and-page-properties.md) §1; [0041](0041-content-models.md) A7). §3's paragraph reads "the other two stay excluded" (`css`, `javascript`) and points at 0055 §1 for `sanitized-css`. (PENDING E18)

Replaced text: §3 of this ADR.

### A6. `render_state.unheld_misses`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §11
- **Summary:** The count of misses §11 says `render_state` keeps is a named column, `unheld_misses integer`, of `view.render_state` ([0042](0042-template-expansion-and-parsoid.md) §10, A15). (PENDING E23)

### A7. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§17

### A8. Usage invalidation is a tier-3 consumer of composition

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §3
- **Change:** amends §10
- **Summary:** Invalidation no longer hangs off "when an entity's `resolved_version` changes": the composer emits "entity X, tenant T, composed to version N" after each composed row commits, and the entity-usage consumer reads those events in its own queue, with its own rate and lag, never inside the appending or composing transaction. The consumer is off while no tenant on the instance has `wikitext.expansion` on, and off during bootstrap and bulk modes until the operator turns it on; its lag is reported on its own page, and a page's render may be behind an entity by that lag. Chapter text: [11](../architecture/11-rendering-templates-and-modules.md) §7.9, §4.4. (REVIEW G10)

Replaced text: [11](../architecture/11-rendering-templates-and-modules.md) §7.9, §4.4.

### A9. The Interface namespace

- **Date:** 2026-10-28
- **Source:** [0095](0095-the-interface-namespace.md) §2
- **Change:** amends §5
- **Summary:** `mw.message` reads messages under the wiki's Interface pages (0095 §2).

Replaced text: [11](../architecture/11-rendering-templates-and-modules.md) §7.5.

### A10. No commercial licence

- **Date:** 2026-11-07
- **Source:** Direct: James, design discussion of 2026-11-07
- **Change:** amends §14
- **Summary:** The `scatter-*` crates are no longer offered under a commercial licence ([0005](0005-crate-organization.md) A114), so the vendored Lua is no longer kept out of them for that reason. `triplespace-scribunto` stays where it is, and the Lua vendored into it keeps its upstream licences.

Replaced text: [22](../architecture/22-crates-and-stack.md) §3.3.

