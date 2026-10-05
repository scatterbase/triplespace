# 0043. Lua modules

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-05 (A4)
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

**`wikitext.lua = on`** ([0042](0042-template-expansion-and-parsoid.md) §2) enables Lua for a tenant. It requires `wikitext.expansion = on` and is capped by the instance's `wikitext.ceiling`. While it is off, `{{#invoke:}}` renders as a chip.

### 2. The Module namespace (amends 0008 §2)

**While `wikitext.lua` is on**, Module (828) and Module talk (829), Scribunto's numbers, are enabled:

| Namespace | Kind | Models | Default | Normalizer | Subpages | Talk |
|---|---|---|---|---|---|---|
| 828 Module | `pages` | `Scribunto`, `wikitext`, `json` | `Scribunto`, with the title rules below | `first-letter` | Yes | 829 |
| 829 Module talk | `pages` | `triplespace-talk` | `triplespace-talk` | — | — | — |

**Title rules for the default model**, as Scribunto applies them: a title ending in `/doc` is `wikitext` (the module's documentation page); a title ending in `.json` is `json` (data for `mw.loadJsonData`); anything else is `Scribunto`. All three are text models, so the namespace satisfies [0041](0041-content-models.md) §4's single-source rule.

**While Lua is off**, 828 and 829 are registered and reserved, as 0042 §3 does for Template: they are entered in `namespaces.toml` with `enabled_by = "wikitext.lua"`, so the numbers are never given another meaning. Unlike core's reserved numbers, they are not listed in `meta=siteinfo` while disabled and empty, because MediaWiki without Scribunto does not list them and clients use their presence to detect Lua. Disabling Lua leaves module pages in place, refuses writes with `ts-namespace-disabled`, and leaves them readable, as 0042 §3 says for templates.

### 3. The `Scribunto` content model (extends 0041 §3; amends 0008 §5)

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `Scribunto` | Scribunto | text | main | `text/plain` | Yes | 828, by the title rules of §2 |

The ID is Scribunto's own, capitalized as Scribunto registers it, because clients compare it ([0041](0041-content-models.md) §2).

**Saving checks syntax,** as Scribunto does: a module that does not compile is refused with the Lua error message, and nothing is appended. The pinned error shape comes from a contract test against the reference install (§15).

**Rendering a module page** shows its source with syntax highlighting, and transcludes its `/doc` page above it, as Scribunto does.

**0008 §5's exclusion of executable models still stands for what it was about.** It excluded `css`, `javascript` and `sanitized-css` because they reach the reader's browser. A Lua module never does: it runs on the server, in a sandbox (§4), and what it returns is wikitext that goes through the expander and the sanitizer like any other. `Scribunto` is therefore allowed, and the other three stay excluded.

### 4. The runtime: Lua 5.1 in a sandbox (extends 0033 §1)

**Lua is PUC-Rio Lua 5.1.5, through `mlua`** (features `lua51` and `vendored`): the interpreter LuaSandbox embeds, and the language version modules are written for. Luau and Lua 5.4 were rejected: modules depend on 5.1's semantics.

**One interpreter per render**, as Scribunto keeps one engine per parse. Every `#invoke` on the page shares it; each module loads into its own environment, and `require` caches within the render. It runs on the blocking thread pool, never on an async worker.

**The sandbox:**

- **Environment.** Modules see exactly the globals Scribunto gives them, the standard library as its reference manual restricts it. A conformance test enumerates the global environment on the reference install and here, and the two must match (§15).
- **No precompiled chunks.** `load` and `loadstring` accept source text only. Lua 5.1 bytecode is not verified and is a known escape, so this is Triplespace's own rule whatever the reference does.
- **Memory.** A cap on the interpreter's allocator, Scribunto's default of 50 MiB.
- **CPU.** One budget per render covering Lua and the host callbacks it makes, Scribunto's default of 7 seconds, enforced by an instruction-count hook that checks a monotonic clock.
- **Expensive functions** share 0042 §16's count with the expander, as Scribunto shares the parser's.
- **Time.** `os.time` and `os.date` record a volatile input in the manifest ([0042](0042-template-expansion-and-parsoid.md) §10), as Scribunto's wrappers set the parser cache TTL. `os.clock` has reduced precision, as Scribunto reduces it.

The limits are deployment configuration, as 0042 §16's are; a tenant may lower them and not raise them.

### 5. The Scribunto contract: Scribunto's Lua, Triplespace's host

*Changed by A3.*

**Scribunto's Lua libraries are vendored unchanged** from its `REL1_43` branch, matching the reference install, into `triplespace-scribunto` (§17), with a `VENDOR.md` naming the upstream commit. Fixes that are general go upstream.

**The PHP side is reimplemented in Rust,** behind a host trait that `triplespace-render` implements ([0042](0042-template-expansion-and-parsoid.md) §19).

**The base `mw` interface** is Scribunto's fifteen callbacks: `loadPackage`, `loadPHPLibrary`, `frameExists`, `newChildFrame`, `getExpandedArgument`, `getAllExpandedArguments`, `expandTemplate`, `callParserFunction`, `preprocess`, `incrementExpensiveFunctionCount`, `isSubsting`, `getFrameTitle`, `setTTL`, `addWarning` and `loadJsonData`. Frame callbacks are calls into the expander of 0042 §4, so `frame:expandTemplate`, `frame:preprocess`, `frame:callParserFunction` and `frame:extensionTag` expand exactly as a template would. The 100-frame limit is Scribunto's.

**The libraries**, and where the Rust side gets what it needs:

| Library | Host side |
|---|---|
| `mw.site` | Namespaces from the registry; statistics from the tenant; `interwikiMap` from site aliases ([0026](0026-sitelinks.md) §2) |
| `mw.title` | Title parsing and facts from `triplespace-titles`; `getContent` follows the lookup of 0042 §11 for Template and Module titles and reads local pages otherwise, records a dependency, and returns `nil` for a page whose `read` groups are not a subset of the rendering page's, as `exists` answers false for it ([0056](0056-security-model.md) §6); expensive accessors (`exists` on another page, `protectionLevels`, `redirectTarget`) count as MediaWiki counts them |
| `mw.text` | `jsonEncode` and `jsonDecode` with Scribunto's flags; `unstrip`, `unstripNoWiki` and `killMarkers` over 0042 §4's strip state; `nowiki`, `tag` and entity tables in Rust |
| `mw.ustring` | Scribunto's pure-Lua implementation, with its PHP fast paths (`find`, `match`, `gmatch`, `gsub`) reimplemented by a **native Lua-pattern matcher over code points**. Translating patterns to `regex` was rejected, since `regex` has neither `%b` nor back-references. Normalization through ICU4X |
| `mw.language` | `formatNum`, `formatDate` (the code behind `#time`, [0042](0042-template-expansion-and-parsoid.md) §5), plurals and case mapping through ICU4X; `fetchLanguageName(s)` from MediaWiki's language-name data, shipped as data; `convertGrammar` and `gender` as MediaWiki defines them for each language, returning the word unchanged where it defines nothing |
| `mw.message` | MediaWiki core's messages shipped as data ([0042](0042-template-expansion-and-parsoid.md) §5); no overrides, since the MediaWiki namespace is reserved |
| `mw.uri` | URLs under the tenant base |
| `mw.html` | Pure Lua; nothing on the host |
| `mw.hash` | The algorithms modules use in practice, through RustCrypto: `md5`, `sha1`, the SHA-2 and SHA-3 families, `crc32b` and `fnv1a32`/`fnv1a64`. `listAlgorithms` reports exactly those |
| Other libraries on the pinned branch, such as `mw.svg` | Implemented when the pinned branch has them |
| `mw.ext.ParserFunctions.expr` | The `#expr` evaluator of 0042 §5 |

**Other `mw.ext` libraries are absent**, as on a wiki without the extension: `mw.ext.data` (tabular data on Commons), `mw.ext.TitleBlacklist` and the rest. Modules already test for them.

### 6. Data modules (extends 0014 §4)

`mw.loadData` and `mw.loadJsonData` return read-only tables, so the same data can serve every page that loads it. **Triplespace caches the parsed data across renders**, keyed by the data page's ID and offset, as an L1 entry `ld:{tenant}:{pageid}:{gen}:{offset}`, and builds each render's table from the cached tree instead of running the module again. MediaWiki caches it only within one parse. A foreign data module ([0042](0042-template-expansion-and-parsoid.md) §11) is keyed by repository, title and remote revision instead.

### 7. Wikibase Client: `mw.wikibase`, `#property` and `#statements`

*Changed by A4.*

**The tenant is its own Wikibase client.** Its modules read its resolved view: local entities and those of the providers it reads ([0018](0018-tenants.md) §3), as `wbgetentities` serves them, with redirects followed as Wikibase Client follows them.

**`mw.wikibase` and `mw.wikibase.entity` are vendored from WikibaseClient's `REL1_43`,** and their callbacks are implemented in Rust: `getEntity`, `getEntityStatements`, `getEntityId`, `getEntityUrl`, `getLabel`, `getLabelByLanguage`, `getDescription`, `getDescriptionByLanguage`, `getSiteLinkPageName`, `getBadges`, `isValidEntityId`, `entityExists`, `renderSnak`, `formatValue`, `renderSnaks`, `formatValues`, `resolvePropertyId`, `orderProperties`, `getPropertyOrder` and `getReferencedEntityId`. `getGlobalSiteId` is §9's client site.

**Entities reach Lua in Wikibase's Lua serialization:** its JSON with arrays numbered from 1, after the ID translation of §8. Arbitrary access, any entity other than the page's own, counts as an expensive function, as Wikibase Client counts it, and is capped by Wikibase Client's `entityAccessLimit` at its default.

**`{{#property:}}` and `{{#statements:}}`** are implemented with Wikibase Client's parameters (`from=` for arbitrary access) and its output: plain text for `#property`, rich wikitext for `#statements`.

**Formatting** (`renderSnak`, `formatValue`, `#statements`) produces the wikitext Wikibase Client produces for each data type, pinned by contract tests against the reference install. Triplespace's own data types, such as `wikibase-domain`, `wikibase-keyword` and `localMedia`, format as links to their pages.

**`mw.wikibase.lexeme`** ([0066](0066-lexemes.md) §9) is provided as WikibaseLexeme documents it, with its `entity` classes for lexemes, forms and senses, once Lexeme (146) is implemented; usage is tracked per lexeme ID. This settles Q4.

### 8. The Lua ID space, per tenant (uses 0017 §1 and 0044 §1)

**Each tenant says what a bare one-letter ID means in its Lua,** with the `site` setting **`lua.ids`**: a map from a type letter to `local` or to a provider code. The default is empty: every letter means `local`, so a bare `Q5` means the local Q5, as everywhere else in Triplespace. A mapped provider must mint that type letter (`providers.toml`) and be one the tenant reads (`providers`, [0018](0018-tenants.md) §3); a record that names any other provider is refused.

**A Wikipedia mirror maps the Wikidata letters:**

```toml
# site setting lua.ids
Q = "WD"
P = "WD"
L = "WD"
M = "WD"
```

**`#property` and `#statements` read the same setting.** They are Wikibase Client surfaces like `mw.wikibase`, and a template that mixes `{{#property:P31}}` with a module call must mean one property by both.

**Translation is at the boundary,** both ways, for every ID that crosses it:

| Written in Lua (or a parser-function argument) | Means |
|---|---|
| A bare ID, `Q5`, whose letter is mapped to provider `WD` | `WDQ5` |
| A bare ID whose letter is unmapped | The local `Q5` |
| The tenant-relative form, `QQQ5` ([0044](0044-tenant-relative-ids.md) §1) | The local `Q5`, **always**, whatever the mapping |
| A full foreign ID, `LBQ6`, or a keyed ID, `domain:example.org` | Itself |

| An entity reaching Lua | Is written |
|---|---|
| An entity of the provider its letter is mapped to (`WDQ5` under `Q = "WD"`) | Bare: `Q5` |
| A local entity whose letter is mapped elsewhere | Tenant-relative: `QQQ5` |
| A local entity whose letter is unmapped | Bare: `Q5` |
| Any other foreign or keyed entity | Its full ID: `LBQ6`, `domain:example.org` |

The output rule applies everywhere an ID appears in what the host returns: an entity's `id`; statement ID prefixes; the property keys of `claims`, `qualifiers` and reference `snaks`; `property` fields; `qualifiers-order` and `snaks-order`; entity values' `id`, with `numeric-id` its digits; and the results of `getEntityIdForCurrentPage`, `getEntityIdForTitle`, `resolvePropertyId`, `getReferencedEntityId`, `orderProperties` and `getPropertyOrder`.

**The mapping is a bijection** between the strings Lua sees and canonical IDs, so a module that reads an ID and passes it back always reaches the same entity. Nothing translated is ever stored: manifests, usage rows and caches hold canonical IDs.

`resolvePropertyId` resolves a label among the properties of the provider the `P` letter names, or the local ones when `P` is unmapped.

**On a Wikipedia mirror**, Module:Wd, Module:WikidataIB and infobox modules work unchanged against mirrored Wikidata, and the tenant's own items stay reachable as `QQQ…`.

### 9. The client site (uses 0026 §2 and 0038 §6)

**A Wikibase client knows which site it is,** and that decides which item a page is connected to. The `site` setting **`lua.client_site`** names a site ID with a site alias ([0026](0026-sitelinks.md) §2). The default is the tenant's own alias, which makes the connected item the paired item of [0038](0038-page-metadata-and-categories.md) §6.

`getGlobalSiteId` returns it. `getEntityIdForCurrentPage` and `getEntityIdForTitle` find the item whose sitelink on that site's host matches the title under the alias's article path. `getSitelink` with no site argument uses it. The `S` and `T` usage aspects (§10) refer to it.

**A Wikipedia mirror sets `lua.client_site = "enwiki"`.** Its page `Douglas Adams` is then connected to the item with the sitelink `https://en.wikipedia.org/wiki/Douglas_Adams`, which is `WDQ42`, and Lua sees `Q42`.

### 10. Usage tracking and invalidation (extends 0042 §10)

**Every entity read records Wikibase's usage aspects** in the render manifest, with Wikibase Client's codes:

| Aspect | Meaning |
|---|---|
| `S` | Sitelinks |
| `L.{lang}` | The label in one language; `L` for all |
| `D.{lang}` | The description in one language; `D` for all |
| `T` | The page's own sitelink on the client site (§9) |
| `C.{property}` | Statements of one property; `C` for all |
| `O` | Anything else, such as aliases |
| `X` | Everything |

**They are stored in `view.entity_usage`** `(page_id, entity_id, aspect, modifier)`, with canonical IDs, written by 0042 §10's refresh job, and the counterpart of Wikibase's `wbc_entity_usage`.

**When an entity's `resolved_version` changes**, the change is classified from its structured diff ([0012](0012-api-requirements.md) §7): which languages' labels and descriptions, which properties' statements, which sitelinks, and whether it was a redirect or a change of cluster. Only pages whose aspects match get an epoch bump and a refresh. A label edit in German does not re-render pages that read only English labels.

### 11. Entities the tenant does not hold

**An entity the tenant's view does not hold is missing to Lua,** as an entity that does not exist is: `getEntity` returns `nil` and `entityExists` returns `false`. That happens when a provider is mirrored only in part ([0002](0002-source-graphs-and-mass-ingest.md) §8.3). The render records the miss with a short expiry, `lua.unheld_ttl` (default 10 minutes), so the page picks the entity up once a sync brings it in, and `render_state` counts the misses so an operator can see which unheld entities pages ask for. Fetching them on demand is open.

### 12. Errors, limits and tracking categories

**A script error renders as Scribunto renders it:** a "Lua error" span whose details open on click, and the tracking category `scribunto-common-error-category` ([0042](0042-template-expansion-and-parsoid.md) §9). Limit overruns (memory, CPU, expensive functions, entity access) are script errors with Scribunto's messages. The limit report gains Scribunto's `scribunto-limitreport-timeusage` and `scribunto-limitreport-memusage`.

### 13. Editing modules (extends 0034 §7)

The source editor gains CodeMirror's Lua mode. **Previewing a module** runs a chosen page with the edited module in its place, server-side, under the `parse` rate class ([0042](0042-template-expansion-and-parsoid.md) §16), as Scribunto's "Preview page with this module" does. Editing needs `edit`, as in MediaWiki; protection works as for any page.

### 14. Licensing (extends 0033 §1 and 0005 §3)

**The vendored Lua is GPL code:** Scribunto is "GPL-2.0-or-later AND MIT", WikibaseClient GPL-2.0-or-later. Both are on 0033 §1's accepted list, so the binary may include them.

**They cannot go into a `scatter-*` crate,** because the shared crates are dual-licensed and Scatter holds copyright in every line of them ([0005](0005-crate-organization.md) §6). So the Lua runtime is a Triplespace crate, `triplespace-scribunto`, under GPL-3.0-or-later only. Scatterbase can use it on GPL terms, not on commercial ones.

**The crate is pure** (no I/O, no async runtime), and 0005 §3 rule 2 names it. Through `mlua` it links Lua's C code, so it cannot build for wasm and is not on rule 7's list. Lua 5.1 and `mlua` are MIT.

### 15. Conformance

- **Scribunto's own Lua-side test modules**, the `*Tests.lua` files beside its PHPUnit suite, run against the Rust host, library by library.
- **WikibaseClient's Lua tests** run the same way, against a fixture tenant whose entities match theirs.
- **The global environment test** of §4.
- **A differential `action=expandtemplates` test** over pages that call widely used modules from English Wikipedia (citations, infoboxes, coordinates, `Module:Wd`), on the reference install with Scribunto and WikibaseClient ([0042](0042-template-expansion-and-parsoid.md) §18) and on Triplespace, with the same entities on both.
- **Fuzzing the sandbox.** Generated Lua must end within the limits, never panic the host, and never reach a global outside the enumerated set.

### 16. API (extends 0012 §4)

| Module | Behaviour |
|---|---|
| `prop=wbentityusage`, `list=wblistentityusage` | From `view.entity_usage` (§10), with Wikibase's aspect codes and IDs in the tenant's Lua form (§8) |
| `prop=pageprops` | `wikibase_item` for pages connected through the client site (§9), as Wikibase Client sets it, in Lua form |
| `meta=siteinfo` | Module and Module talk while enabled; `Scribunto` among `contentmodels`; `siprop=triplespace` reports `lua.ids` and `lua.client_site` |
| `action=expandtemplates`, `action=parse` | Run Lua when `wikitext.lua` is on ([0042](0042-template-expansion-and-parsoid.md) §14) |

### 17. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with `triplespace-scribunto` and every change this section listed. The table this section first gave is in A1.

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
