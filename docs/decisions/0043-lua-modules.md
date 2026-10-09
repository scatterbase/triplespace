# 0043. Lua modules

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-09 (A7)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0033](0033-backend-stack.md), [0034](0034-frontend-stack.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md)
- **Uses:** [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0026](0026-sitelinks.md), [0038](0038-page-metadata-and-categories.md), [0044](0044-tenant-relative-ids.md)
- **Chapters:** [04](../architecture/04-entities-and-identifiers.md), [11](../architecture/11-rendering-templates-and-modules.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

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

*Changed by A3.*

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

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §4.2, §4.4, §7.9.*

### 11. Entities the tenant does not hold

*Changed by A6.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.10.*

### 12. Errors, limits and tracking categories

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.11.*

### 13. Editing modules (extends 0034 §7)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §7.12.*

### 14. Licensing (extends 0033 §1 and 0005 §3)

*Current text: [22](../architecture/22-crates-and-stack.md) §1.2, §3.3.*

### 15. Conformance

*Current text: [22](../architecture/22-crates-and-stack.md) §4.9.*

### 16. API (extends 0012 §4)

*Current text: [18](../architecture/18-api.md) §2.2, §2.3.*

### 17. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **Wikipedia's modules can run here,** against mirrored Wikidata, with nothing changed in the modules: bare IDs mean what their authors meant, by tenant configuration.
- **A tenant's own entities are never out of reach,** whatever `lua.ids` says, through the tenant-relative form.
- **The Scribunto contract is met by running Scribunto's own Lua.** What is reimplemented is the host side, which is smaller and better specified than the libraries.
- **Lua is the most expensive thing a page can do.** The ceiling, the limits and aspect-matched invalidation are what keep it affordable; a farm operator can turn it off.
- **The Lua runtime is GPL-only.** Scatterbase cannot take it under the commercial licence.
- **Unheld entities are silent misses.** A partial mirror makes some module output incomplete until a sync brings the entity in, and `render_state` makes that visible.

## Open questions

- **Q1. The debug console** (`action=scribunto-console`), and where its sessions live.
- **Q2. Fetching unheld entities on demand** (§11): a bounded, rate-limited fetch into the mirror, against 0012 §6's rule that live fetches are never written to the log.
- **Q3. `convertGrammar` and `gender` coverage,** which MediaWiki implements per language in code.
- **Q4.** ~~**Lexemes.** `mw.wikibase.lexeme` waits for the Lexeme namespace (146) to be implemented.~~ *Settled by [0066](0066-lexemes.md) §9: provided as the extension documents it.*
- **Q5. `mw.ext.data`** (Commons tabular and map data), which many Wikipedia modules use for lookup tables.
- **Q6. LuaJIT** for speed, if a differential run shows it is safe for real modules.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2, §3 | §17 | extends | 0005 A43 |
| [0008](0008-namespaces-and-document-pages.md) §5 | §2–3 | amends | 0008 A13 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §2–3 | extends | 0008 A13 |
| [0012](0012-api-requirements.md) §4 | §16 | extends | 0012 A27 |
| [0013](0013-postgres-storage.md) §5.6 | §10 | extends | 0013 A17 |
| [0014](0014-caches-and-search.md) §10 | §6 | extends | 0014 A8 |
| [0033](0033-backend-stack.md) §1 | §4, §14 | extends | 0033 A4 |
| [0034](0034-frontend-stack.md) §7 | §13 | extends | 0034 A2 |
| [0041](0041-content-models.md) §3 | §3 | extends | 0041 A2 |
| [0042](0042-template-expansion-and-parsoid.md) §2, §10 | §1, §10 | extends | 0042 A2 |

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

Replaced text (§17):

> | Crate | Change |
> |---|---|
> | `triplespace-scribunto` *(new)* | Pure; links Lua 5.1 through `mlua`. The interpreter and sandbox (§4); the vendored Scribunto and WikibaseClient Lua (§5, §7); the Rust side of their interfaces behind a host trait; the Lua-pattern matcher; data-module trees (§6); the ID translation of §8; Wikibase Client formatting, `#property` and `#statements` (§7); usage-aspect recording (§10). Depends on `scatter-wikitext-expand`, `scatter-wikibase-model`, `scatter-providers` and `mlua` |
> | `scatter-wikitext-expand` | `#invoke` calls out through `ExpandHost`; `#property` and `#statements` are registered (§7) |
> | `triplespace-render` | Implements the Lua host trait over the serving model; `view.entity_usage` and aspect-matched invalidation (§10); the unheld-entity expiry (§11). Gains a dependency on `triplespace-scribunto` |
> | `triplespace-titles` | Module and Module talk, with the title rules of §2 |
> | `triplespace-db` | The migration for `view.entity_usage` |
> | `triplespace-cache` | `ld:` keys (§6) |
> | `triplespace-api-action` | The modules of §16 |
> | `scatter-pages` | The `Scribunto` model's registry entry and syntax check hook (§3) |
>
> The workspace goes from fifty-one crates to fifty-two.

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

Replaced text (§3):

> **0008 §5's exclusion of executable models still stands for what it was about.** It excluded `css`, `javascript` and `sanitized-css` because they reach the reader's browser. A Lua module never does: it runs on the server, in a sandbox (§4), and what it returns is wikitext that goes through the expander and the sanitizer like any other. `Scribunto` is therefore allowed, and the other three stay excluded.

### A6. `render_state.unheld_misses`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §11
- **Summary:** The count of misses §11 says `render_state` keeps is a named column, `unheld_misses integer`, of `view.render_state` ([0042](0042-template-expansion-and-parsoid.md) §10, A15). (PENDING E23)

### A7. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§17
- **Summary:** The Decision's current text now lives in the architecture chapters [04](../architecture/04-entities-and-identifiers.md), [11](../architecture/11-rendering-templates-and-modules.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
