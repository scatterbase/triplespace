# Registry

This directory is the registry of record for the names and codes that Triplespace and Scatterbase share. [ADR 0015](../decisions/0015-record-format-and-partition-registry.md) §5 makes the repository the authority for them, as [0005](../decisions/0005-crate-organization.md) §5 already made `scatter-vocab` the authority for the `scatter:` vocabulary.

| File | Lists | Embedded by | Defined in |
|---|---|---|---|
| `graphs.toml` | Reserved graph names, their kind, policies and payload type | `scatter-log` | 0015 §3, §5, 0039 §10–11, 0053 §5 |
| `providers.toml` | Provider codes, slugs, numbers, type codes and ID grammars, IRI templates, issuers, trust mode and key-chain URL, chip colours; the reserved doubled codes `AA`–`ZZ`; page providers with no code, pending (English Wikipedia) | `scatter-providers` | 0000 §3, 0002 §4, 0010 §2, 0015 §2, §5, 0017 §2, 0022 §2, 0044 §2, 0052 §1 |
| `issuers.toml` | Issuer codes and actor models | `scatter-actors` | 0007 §1, 0054 §1 |
| `namespaces.toml` | Default namespace numbers and kinds, each `pages` namespace's allowed and default content models, the reserved MediaWiki and Wikibase numbers, and the 210–229 and 310–329 ranges | `triplespace-titles` | 0008 §2 (amended), 0009 §11, 0017 §5, 0019 §3, 0029, 0038 §4, §8, 0039 §1, 0041 §4, 0042 §3, 0043 §2, 0045 §2, 0048 §7, 0055 §1 |
| `content-models.toml` | Content models: ID, origin, source, slot, entity type, serialization format and direct editing; the reserved MediaWiki and Wikibase model IDs | `scatter-pages` | 0008 §5, 0041 §2–3, 0043 §3, 0045 §3, 0055 §1 |
| `keyed-types.toml` | Keyed entity types and their ID prefixes | `scatter-normalize` | 0009 §1, 0017 §3, §5, 0048 §1 |
| `notation-schemes.toml` | Notation schemes: name, label, normalizer and grammar, documentation template (`osm`; candidates drafted) | `scatter-normalize` | 0048 §2, §6 |
| `groups.toml` | Default groups, their permissions, and the default graph ACLs | `scatter-actors` | 0016 §2–4, 0019 §12, 0020 §7, 0021 §9, 0022 §12, 0023 §11, 0024 §11, 0025 §10, 0030 §12, 0047 §12, 0051 §8 |
| `grants.toml` | API-key grants and the permissions each covers; also the OAuth scopes a consumer may request | `scatter-actors` | 0024 §4, 0025 §2, 0047 §12 |
| `sites.toml` | Site aliases: MediaWiki site IDs, hosts, article paths and languages, for sitelink compatibility (generated from the Wikimedia site matrix) | `scatter-wikibase-model` | 0026 §2 |
| `thread-statuses.toml` | Default thread statuses, with category and order | `scatter-threads` | 0019 §6 (amended), 0054 §5 |
| `preferences.toml` | Registered preference keys, types and defaults | `triplespace-accounts` | 0027 §1 |
| `tenancy.toml` | Tenancy policy switches and the `isolated`, `community` and `enterprise` presets, with their global groups | `scatter-actors` | 0028 §1, 0042 §2 |
| `file-types.toml` | Permitted file types: extensions, MIME and MediaWiki media types, magic signatures, inline or attachment, thumbnailer, and which are allowed by default | `scatter-files` | 0039 §5 |
| `resolvers.toml` | Resolver namespaces: binding, grammar, normalizer and case rule, external IRI (`doi`, `url`; candidates drafted) | `scatter-normalize` | 0029 §1, §8 |
| `wikitext-functions.toml` | Wikitext variables, parser functions, extension tags and behaviour switches, with origin and status (`implemented`, `chip`, `ignored`); what `meta=siteinfo` reports as `magicwords`, `functionhooks`, `extensiontags`, `variables` and `doubleunderscores` (seed; generated from the reference install) | `scatter-wikitext-expand` | 0042 §5, 0043 §7, 0051 §1, 0055 §7 |
| `special-pages.toml` | Special pages: canonical and MediaWiki names, origin, status (`served`, `deferred`, `declined`, `reserved`), farm or tenant scope, `Special:SpecialPages` group, restriction, aliases and section aliases, and each report's backing and graph scope (names and English aliases copied from MediaWiki and Wikibase REL1_43) | `triplespace-titles` | 0047 §1–4, 0051 §6, 0054 §2, 0055 §6 |
| `css-properties.toml` | The CSS at-rules and properties the `sanitized-css` sanitizer allows, with the module each came from, the forbidden values and the `url()` policy (seed; generated from css-sanitizer's published set) | `scatter-css` | 0055 §2 |
| `themes.toml` | Themes as values for Codex design tokens, and the typefaces each serves; `default` is the theme Triplespace ships (the canvases' palette and Newsreader headings) | `triplespace-ui` | 0034 §1–2 |
| `fragments.toml` | Fragment paths for packed record storage: per payload type, the CBOR path patterns whose subtrees are stored once per dedup domain, the history condition, `min_bytes` and `max_depth`. Physical policy, never part of the record format | `scatter-log` | 0058 §3 |

## Rules

- **A change here is a commit.** Allocating a provider code, slug or number, a graph name, a namespace number or a group is done by editing the file. Nothing is allocated at runtime.
- **Codes and numbers are never reused.** A retired entry stays in the file with `retired = true`.
- **These are defaults, not state.** An instance's `config` partition (0015 §3) starts from these files and may diverge: it may add providers, rename groups or change permissions. What it may not do is reuse a provider code or number for something else, because IDs and revision IDs computed from them are shared between instances.
- **The crates embed the files** (`include_str!`) and parse them at build time, so a malformed file fails the build, and the defaults an instance starts from are the defaults the code was tested with.
- **Provider number 0 is the current tenant.** It is reserved in `providers.toml` and never assigned to a provider.
- **Doubled-letter provider codes are reserved.** `AA` through `ZZ` are never allocated, because `QQQ5` is the tenant-relative form of the local `Q5` ([0044](../decisions/0044-tenant-relative-ids.md) §2).
- **Slugs are one namespace.** A tenant's slug ([0018](../decisions/0018-tenants.md) §1), a provider's slug and an issuer's code are the same word for the same thing; a tenant that becomes a provider keeps its slug and takes a code and a number here.

## Pending allocations

- **English Wikipedia as a page provider** ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §1): slug `enwiki`, provider number 9, issuer `enwiki` (in `issuers.toml`), `pages = true`, no code and no types. The number is reserved here; the `[[provider]]` row is written once `scatter-providers` accepts an entry without a code.

- Filing the registration of 210–229 and 310–329 on mediawiki.org's [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces) page. The numbers are allocated in `namespaces.toml` ([0008](../decisions/0008-namespaces-and-document-pages.md) §2, as amended 2026-09-27 and 2026-10-01): Domain 210/211, Keyword 212/213, Thread 214/215, Notation 216/217 (allocated to OSM by 0036, renamed by [0048](../decisions/0048-notation.md) §7 before filing), Table 218/219 ([0045](../decisions/0045-table-content-model.md) §2), DOI 220/221, URL 222/223, Board 310/311 ([0049](../decisions/0049-boards.md) §2); 210–219 is full, 224–229 and 312–319 are free, and 320–329 (resolvers) holds nothing yet. As of 2026-09-27 the page lists nothing between 204 and 240, and as of 2026-10-01 nothing in 310–329.
