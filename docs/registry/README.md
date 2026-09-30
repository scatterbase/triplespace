# Registry

This directory is the registry of record for the names and codes that Triplespace and Scatterbase share. [ADR 0015](../decisions/0015-record-format-and-partition-registry.md) §5 makes the repository the authority for them, as [0005](../decisions/0005-crate-organization.md) §5 already made `scatter-vocab` the authority for the `scatter:` vocabulary.

| File | Lists | Embedded by | Defined in |
|---|---|---|---|
| `graphs.toml` | Reserved graph names, their kind, policies and payload type | `scatter-log` | 0015 §3, §5 |
| `providers.toml` | Provider codes, slugs, numbers, type codes and ID grammars, IRI templates, issuers, trust mode and key-chain URL | `scatter-providers` | 0000 §3, 0002 §4, 0015 §2, §5, 0017 §2, 0022 §2 |
| `issuers.toml` | Issuer codes and actor models | `scatter-actors` | 0007 §1 |
| `namespaces.toml` | Default namespace numbers and kinds, the reserved MediaWiki and Wikibase numbers, and the 210–229 ranges | `triplespace-titles` | 0008 §2 (amended), 0009 §11, 0017 §5, 0019 §3, 0029, 0038 §4, §8 |
| `keyed-types.toml` | Keyed entity types and their ID prefixes | `scatter-normalize` | 0009 §1, 0017 §3, §5 |
| `groups.toml` | Default groups, their permissions, and the default graph ACLs | `scatter-actors` | 0016 §2–4, 0019 §12, 0020 §7, 0021 §9, 0022 §12, 0023 §11, 0024 §11, 0025 §10, 0030 §12 |
| `grants.toml` | API-key grants and the permissions each covers; also the OAuth scopes a consumer may request | `scatter-actors` | 0024 §4, 0025 §2 |
| `sites.toml` | Site aliases: MediaWiki site IDs, hosts, article paths and languages, for sitelink compatibility (generated from the Wikimedia site matrix) | `scatter-wikibase-model` | 0026 §2 |
| `thread-statuses.toml` | Default thread statuses, with category and order | `scatter-threads` | 0019 §6 (amended) |
| `preferences.toml` | Registered preference keys, types and defaults | `triplespace-accounts` | 0027 §1 |
| `tenancy.toml` | Tenancy policy switches and the `isolated`, `community` and `enterprise` presets, with their global groups | `scatter-actors` | 0028 §1 |
| `resolvers.toml` | Resolver namespaces: binding, grammar, normalizer and case rule, external IRI (`doi`, `url`; candidates drafted) | `scatter-normalize` | 0029 §1, §8 |

## Rules

- **A change here is a commit.** Allocating a provider code, slug or number, a graph name, a namespace number or a group is done by editing the file. Nothing is allocated at runtime.
- **Codes and numbers are never reused.** A retired entry stays in the file with `retired = true`.
- **These are defaults, not state.** An instance's `config` partition (0015 §3) starts from these files and may diverge: it may add providers, rename groups or change permissions. What it may not do is reuse a provider code or number for something else, because IDs and revision IDs computed from them are shared between instances.
- **The crates embed the files** (`include_str!`) and parse them at build time, so a malformed file fails the build, and the defaults an instance starts from are the defaults the code was tested with.
- **Provider number 0 is the current tenant.** It is reserved in `providers.toml` and never assigned to a provider.
- **Slugs are one namespace.** A tenant's slug ([0018](../decisions/0018-tenants.md) §1), a provider's slug and an issuer's code are the same word for the same thing; a tenant that becomes a provider keeps its slug and takes a code and a number here.

## Pending allocations

- Filing the registration of 210–229 on mediawiki.org's [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces) page. The numbers are allocated in `namespaces.toml` ([0008](../decisions/0008-namespaces-and-document-pages.md) §2, as amended 2026-09-27): Domain 210/211, Keyword 212/213, Thread 214/215, OSM 216/217, DOI 220/221, URL 222/223; 218–219 and 224–229 are free. As of 2026-09-27 the page lists nothing between 204 and 240.
