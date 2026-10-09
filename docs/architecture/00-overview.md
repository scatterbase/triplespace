# 00. Overview

This chapter is the entry point to the architecture reference: what Triplespace is, in the four principles of [0000](../decisions/0000-init.md); the shape of the system, as one walk through the twenty-three chapters that follow, with the four crate layers and the two deployment profiles as the picture of what runs where; a table of every catalogue that spans ADRs and the chapter section that holds it; how to read a chapter and the ADR it was assembled from; and a glossary of the terms the chapters use without defining in place. It assumes nothing. Every chapter after it opens with what it assumes from the others, and the storage model of Part I ([01](01-log-and-records.md), [02](02-graphs-rdf-and-query.md), [03](03-storage-caches-and-search.md)) is what the rest is built on.

## 1. What Triplespace is

*Sources: [0000](../decisions/0000-init.md) §1, §2, §3, §4.*

Triplespace is Wikibase reimagined as an append-only log, implemented in Rust, intended to support high-speed data ingest and foreign entities alongside local ones. Four principles, fixed in [0000](../decisions/0000-init.md), hold through every later decision.

### 1.1 The log is the source of truth

*Sources: [0000](../decisions/0000-init.md) §1.*

Every change is appended to the log as a record. Nothing is updated in place. Current entity state, RDF output, and query indexes are all projections derived from the log.

### 1.2 High-speed ingest is a primary workload

*Sources: [0000](../decisions/0000-init.md) §2.*

Bulk loading is a first-class path, not a long series of individual edits. Ingesting a dataset means appending to the log at whatever rate the storage can sustain.

### 1.3 Foreign entities sit alongside local ones

*Sources: [0000](../decisions/0000-init.md) §3.*

A foreign entity can be referenced directly, without first being reified as a local item. Identifiers distinguish the two:

- **Local entities** keep Wikibase's single-letter prefixes (`Q`, `P`, and so on).
- **Foreign entities** use a three-letter prefix: a two-letter provider code followed by a one-letter type code.

**A prefix records who minted an ID, not who asserts its triples** ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §1). `WDQ123` was minted by Wikidata, and `Q5` by this instance. Which named graph a triple lives in says who asserts it, so an entity with a foreign ID can have triples in a provider's mirror graph, in the local graph, or in both.

**Provider codes and type codes are registry data.** `docs/registry/providers.toml` allocates them, and is authoritative ([0015](../decisions/0015-record-format-and-partition-registry.md) §5). The first three providers were Wikidata (`WD`: `WDQ` items, `WDP` properties, `WDL` lexemes, `WDM` Commons MediaInfo), Librarybase (`LB`) and OpenAlex (`OA`: `OAW` works, `OAA` authors and the rest). After the type code, an ID follows its type's grammar, which need not be digits ([0017](../decisions/0017-entity-id-grammar.md) §1–2), and OpenAlex keywords are a keyed type rather than `OAK` entities ([0017](../decisions/0017-entity-id-grammar.md) §5).

### 1.4 Hardest realistic case first

*Sources: [0000](../decisions/0000-init.md) §4.*

The design target is Wikidata-scale data and query load. Smaller Wikibase deployments should follow as corollaries and should not need separate design work.

## 2. The shape of the system

*Sources: chapters as linked.*

The chapters are in reading order. Each subject below is stated in two or three sentences, with what it rests on; the chapter named holds the detail.

### 2.1 Part I: the log

*Sources: chapters as linked.*

**[01 Log and records](01-log-and-records.md).** The append-only log is split into partitions, each with its own history policy, integrity policy, segment size and hash function. A record is a small header, which the Merkle tree commits to and which is never erased, and a body that is a list of salted parts, each erasable on its own; records are canonical CBOR, hashed with domain tags, and sealed into segments under signed checkpoints. Erasure, compaction, edit conflicts, verification and export are all defined at the level of the log, before any table or graph exists.

**[02 Graphs, RDF and the query service](02-graphs-rdf-and-query.md).** Every record names the graph it writes to: a local graph, one mirror graph per provider, derived graphs, source graphs, log graphs, and a metadata graph, with the main graph computed as a resolved view over the sources. RDF is an output, not the read path: dumps, the SPARQL Update stream that keeps a dump current, and a query service that is the only reader of a local quad store. It rests on the log of [01](01-log-and-records.md) and the `view` schema of [03](03-storage-caches-and-search.md).

**[03 Storage, caches and search](03-storage-caches-and-search.md).** Postgres holds the log and every projection the API and UI read, in four schemas (`log`, `view`, `private`, `ops`) with distinct reader roles; records may be packed on disk without changing their logical bytes. Over it sit the blob store, the cache layers with keys that carry generation, version, tenant and visibility, HTTP and response caching, and search on OpenSearch with a Postgres fallback. Every ADR's "Storage" and "Search" trailer lands here, and the `view` table catalogue (§5) is the one list of every table.

### 2.2 Part II: the data model

*Sources: chapters as linked.*

**[04 Entities and identifiers](04-entities-and-identifiers.md).** An entity ID takes one of three forms: a local ID with Wikibase's prefix, a foreign ID with a provider and type code, or a keyed ID whose key is a normalized natural key (`domain:en.wikipedia.org`). IDs from different namespaces that denote one thing form an identity cluster with one canonical member, and statements fuse under it in the resolved view. Lexemes, MediaInfo, the derived IDs of foreign pages and the per-tenant Lua ID space are here too; it rests on the log of [01](01-log-and-records.md) and the graphs of [02](02-graphs-rdf-and-query.md).

**[05 Providers and ingest](05-providers-and-ingest.md).** Who minted an ID is separate from who asserts a triple: the second is the source graph a record is written to, and the main graph is reconciled over them. One change-set format is submitted by three paths (an in-process ingester, a bulk-job endpoint, the edit API), recorded as jobs run by adapters, with bootstrap mode for terabyte loads; upstream deletion, correction, retention, shallow mirroring, key-mapped providers, the entity sources a tenant declares, the adoption of an existing Wikibase and the catalogue of providers follow. It rests on [01](01-log-and-records.md), [02](02-graphs-rdf-and-query.md), [03](03-storage-caches-and-search.md) and [04](04-entities-and-identifiers.md).

**[06 Statements and properties](06-statements-and-properties.md).** A statement is canonical Wikibase JSON; semantic roles let shape detection, resolvers and constraints name a property without knowing its ID. Property constraints are checked as a projection over the resolved view, resolvers are namespaces bound to a property, sitelinks are normalized URLs, and some pages carry statements of their own. It rests on the entity model of [04](04-entities-and-identifiers.md).

### 2.3 Part III: actors, tenancy, security

*Sources: chapters as linked.*

**[07 Actors and accounts](07-actors-and-accounts.md).** An actor is an issuer and a subject, named by an actor key, with its name as an attribute kept in actor records; local users log in through a built-in password issuer or delegated authentication, and accounts may be linked or vanished. Automated edits run under subsidiary accounts with API keys, grants and rate limits; the instance is an OAuth server; preferences and private state belong to an account and are never records. It rests on the record header of [01](01-log-and-records.md) and the `private` schema of [03](03-storage-caches-and-search.md).

**[08 Tenants and instances](08-tenants-and-instances.md).** An instance is one deployment; a tenant is one wiki on it, with its own base URI, partitions, configuration and users, and one tenant is primary, the one whose accounts operate the instance. A farm is an instance under a tenancy policy with farm identity; a tenant is a provider to every other tenant; a tenant can be moved, can leave or join a farm, and can be deleted. The instance's own acts on a tenant's data, prerogatives and provisions attested by the instance with an authority record, are here, with tenant isolation in the query service and in packed storage.

**[09 Security and moderation](09-security-and-moderation.md).** Permissions are named strings grouped into groups; memberships and blocks are actor records; ACLs attach a restriction of named permissions to a target by identifier, and deletion, hiding and suppression are `read` ACLs of the moderation kind, confidentiality a `read` ACL of the other. The security model gives one invariant, the visibility of a target as the set of `read` ACLs on it and on everything enclosing it, and the flow rule that derived output inherits its inputs' visibility; edit filters are CEL rules evaluated before append. It holds the one permission catalogue (§2.2) and rests on [01](01-log-and-records.md), [03](03-storage-caches-and-search.md), [07](07-actors-and-accounts.md) and [08](08-tenants-and-instances.md).

### 2.4 Part IV: pages and content

*Sources: chapters as linked.*

**[10 Pages and content models](10-pages-and-content-models.md).** Namespaces are registry entries with a kind; a page is identified by a page ID, with its title as an attribute, and one resolver handles every title. Every page has exactly one content model in its main slot, a registry entry that says where content comes from, how it is validated and rendered; the wikitext subset, links, legacy categories, page properties, redirects, moves and imports follow. It holds the namespace catalogue (§1.4) and the content-model catalogue (§4.4) that later chapters add rows to.

**[11 Rendering, templates and modules](11-rendering-templates-and-modules.md).** A `wikitext` page becomes HTML in two stages, expansion then rendering: the expander with its registry of variables, parser functions, tags and switches, and a built-in or Parsoid renderer. Every expansion produces a manifest that drives refresh and purging; foreign template repositories, TemplateStyles with its CSS sanitizer, TemplateData and Lua modules under the Scribunto contract complete it. It rests on the namespaces and content models of [10](10-pages-and-content-models.md).

**[12 Files and media](12-files-and-media.md).** A file is an upload record in the File namespace, stored in a storage scope, served behind an access check, and removed by one of two paths, the tenant's or the operator's. Foreign file repositories, media data types, files in wikitext and markdown, and MediaInfo as the `mediainfo` slot of a File page with `M{page ID}` as its entity ID are here. It rests on the blob store of [03](03-storage-caches-and-search.md) and the derived `M` ID of [04](04-entities-and-identifiers.md).

**[13 Mirrored pages](13-mirrored-pages.md).** A page repository is a source of pages a tenant serves by title without holding them; a title names a stack, the local page first and then each repository's. A foreign page is kept as a bundle in proxy or mirror mode, can be forked into a local page whose history is seeded afterwards, and merged with upstream in both directions; talk pages can follow a repository's, and extraction sources derive statements from mirrored pages through template mappings and line rules into derived graphs. It rests on the title resolver of [10](10-pages-and-content-models.md) and the expander of [11](11-rendering-templates-and-modules.md).

**[14 Discussions](14-discussions.md).** A thread is a page in the `Thread` namespace and a post is a record with four parts; talk pages are composite and attachment is by identifier. A board is a talk page that is its own subject; a thread has one home and any number of listings, may be pinned, and a proposal is a thread whose records carry a payload offered to the wiki its subject came from. It rests on the record parts of [01](01-log-and-records.md) and the page IDs of [10](10-pages-and-content-models.md).

**[15 Structured pages](15-structured-pages.md).** Some pages are definitions the site evaluates rather than text it shows: a scope names a set of subjects whose membership is a projection; a table names entities and properties and stores no values; a sprint names a scope, rules and a window, with tasks as a projection and claims as records. Workspaces are project pages that declare scopes and transclude blocks, built from kits; query pages are saved parameterized SPARQL; EntitySchema pages validate scopes. It rests on the content models of [10](10-pages-and-content-models.md), the projections of [03](03-storage-caches-and-search.md) and the query service of [02](02-graphs-rdf-and-query.md).

### 2.5 Part V: activity and exchange

*Sources: chapters as linked.*

**[16 Logs, feeds and notifications](16-logs-feeds-and-notifications.md).** Log event records live in log graphs, mirrored from a provider's public log or produced by the instance's own actions, and local events are projected from the records that caused them rather than written twice. History, recent changes, contributions and the log share one activity row; a feed is activity rows filtered by a target set, the watch set is private state, and a notification is an activity delivered to an inbox by an addressing rule. It holds the log-event catalogue (§2.3) and rests on [01](01-log-and-records.md), [03](03-storage-caches-and-search.md), [07](07-actors-and-accounts.md) and [09](09-security-and-moderation.md).

**[17 Federation and publication](17-federation-and-publication.md).** An instance publishes its local graph with checkpoints and proofs, and another instance's Triplespace adapter reads and verifies it; ActivityPub lets accounts, talk pages and boards be followed from the fediverse. Proposals are exported and pushed to the wiki an entity came from, publications write a scope's data to an external wiki as template calls, and dataset publication gives scope dumps and a schema.org profile. It rests on the integrity of [01](01-log-and-records.md), the adapters of [05](05-providers-and-ingest.md), the actors of [07](07-actors-and-accounts.md) and the proposals and scopes of [14](14-discussions.md) and [15](15-structured-pages.md).

### 2.6 Part VI: interfaces

*Sources: chapters as linked.*

**[18 API](18-api.md).** The MediaWiki Action API as Triplespace changes and extends it, the REST routes under `rest.php/triplespace/v0`, the ingest API, structured diffs, raw SPARQL and the MCP server, all following one set of rules, with the host selecting the tenant. The compatibility contracts in `docs/api/` are the reference it builds on. It holds the module catalogue (§2.3) and the route catalogue (§3.2); every ADR's "API" trailer lands here.

**[19 Site UI](19-site-ui.md).** The site as a reader or editor meets it: the frame every page wears, search, addresses, logging in, account settings, document pages, histories, diffs, recent changes and jobs, and the statement UI from shape detection through editing. Everything it shows it fetches through the API of [18](18-api.md); it is rendered by the frontend stack of §5. Every ADR's "UI" trailer lands here.

**[20 Web tier](20-web-tier.md).** The process that renders the site's pages is either `triplespace-server` with the site embedded or the separate `triplespace-web` binary; an edge proxy splits requests by a routing table, forms work without JavaScript, and fragments are fetched with `action=render`. Only the API judges forwarded headers, which places the web tier inside the deployment boundary of [09](09-security-and-moderation.md).

**[21 Special pages](21-special-pages.md).** One registry lists every name in the `Special` namespace, with aliases and where each page is served; reports are live projections over local graphs by default. The Wikibase compatibility forms, the identity and correction pages, `Special:Nuke`, `Special:Export`, `Special:Import` and `Special:Version` are described, and the catalogue (§8) lists every entry of `docs/registry/special-pages.toml`. It rests on [10](10-pages-and-content-models.md), [09](09-security-and-moderation.md) and [18](18-api.md).

### 2.7 Part VII: implementation

*Sources: chapters as linked.*

**[22 Crates and stack](22-crates-and-stack.md).** The Cargo workspace is four layers with dependencies pointing only downward, a crate map that CI checks, and rules: the core is pure, policy is data, each hashed type has one canonical encoding. The backend stack runs from toolchain to packaging; the frontend build and the two deployment profiles are here, with the component manifest `Special:Version` reads.

**[23 Configuration and registry](23-configuration-and-registry.md).** Configuration is records in a `config` partition, instance or tenant scope, of catalogued kinds; every named setting is listed once, and the registry files under `docs/registry/` are what an instance's configuration starts from. Deployment configuration and the CLI sit outside the log. It rests on the partitions of [01](01-log-and-records.md) and the tenant model of [08](08-tenants-and-instances.md).

### 2.8 What runs where: the four layers and the deployment profiles

*Sources: chapters as linked.*

The workspace is organized in four layers, and dependencies point only downward ([22](22-crates-and-stack.md) §1.1):

1. **Substrate.** The log and its integrity, projections, providers, identity clusters, normalizers, actors, document pages, threads, and the Activity Streams profile. Knows nothing about Wikibase.
2. **Wikibase domain.** The data model, its RDF and JSON forms, reconciliation, change sets, shape detection, edit filters, property constraints, and MediaWiki's log model. Knows nothing about Triplespace's APIs or deployment.
3. **Ingest.** Jobs, and one adapter per provider, including the verified adapter for another Triplespace instance. The only crates that name a specific provider.
4. **Triplespace surfaces.** The Postgres schemas and projections, the compatibility APIs, accounts, notifications, federation, the OAuth server, caches, search and the binaries. The only layer specific to Triplespace.

Layers 1 and 2 are the shared crates, prefixed `scatter-`; layer 3 is shared in principle but is where I/O begins; layer 4 is the `triplespace-` crates. Triplespace is the first product assembled from the lower three, and Scatterbase is expected to be the second ([22](22-crates-and-stack.md) §1.1, §1.3). The crate map is [22](22-crates-and-stack.md) §2.1.

The schema is the same everywhere. Two deployment profiles differ only in what is optional ([22](22-crates-and-stack.md) §6):

| | Small instance | Wikidata scale |
|---|---|---|
| Postgres | One server | Primary plus replicas; mirror child tables on their own tablespace |
| `term_prefix` index | Present; serves suggest and `wbsearchentities` | Absent; OpenSearch serves them |
| `entity_ref` | Present | Present; the largest `view` table |
| Shared cache | Optional | Required |
| Quad store | Optional | Optional; QLever from dumps |

This is §1.4's rule that smaller deployments follow as corollaries: they drop services, not tables. A small instance is the `triplespace` binary and Postgres; Valkey, OpenSearch, QLever, a Parsoid service and `triplespace-web` are what a larger instance adds ([22](22-crates-and-stack.md) §6, §4.1). The two ways to run the site, embedded or as a separate web tier, are [20](20-web-tier.md) §1.2.

## 3. Where things are catalogued

*Sources: chapters as linked.*

A table that spans ADRs appears once, in one chapter, and other chapters link to the row ([README.md](README.md), Conventions). The registry files under `docs/registry/` are authoritative where a catalogue and a registry differ; the catalogues are assembled from the ADRs.

| Catalogue | Where |
|---|---|
| Named graphs, with kind, scope, writer, history, integrity and export | [02](02-graphs-rdf-and-query.md) §1.2 |
| The registry of record: `graphs.toml` and the other registry files | [02](02-graphs-rdf-and-query.md) §1.4; [23](23-configuration-and-registry.md) §4 |
| The `view` schema, every table and what it serves | [03](03-storage-caches-and-search.md) §5 |
| Cache keys (L1), L0 entries, Valkey keys that are not caches, cache tags and search indexes | [03](03-storage-caches-and-search.md) §12, L1 keys in §12.1 |
| Projections and their order | [03](03-storage-caches-and-search.md) §6.1 |
| Registered providers | [05](05-providers-and-ingest.md) §8, the table in §8.1 |
| Rate-limit classes | [07](07-actors-and-accounts.md) §6.2 |
| Permissions, with their default groups | [09](09-security-and-moderation.md) §2.2 |
| ACL target kinds | [09](09-security-and-moderation.md) §4.2 |
| Namespaces, with their numbers, kinds and allowed models | [10](10-pages-and-content-models.md) §1.4 |
| Content models | [10](10-pages-and-content-models.md) §4.4 |
| Parser functions, variables, tags and switches, with their status | [11](11-rendering-templates-and-modules.md) §2.2 |
| Log event types and actions | [16](16-logs-feeds-and-notifications.md) §2.3 |
| Action API modules | [18](18-api.md) §2.3 |
| REST routes under `rest.php/triplespace/v0` | [18](18-api.md) §3.2 |
| Special pages, every entry of `special-pages.toml` | [21](21-special-pages.md) §8 |
| Crates | [22](22-crates-and-stack.md) §2.1 |
| Config kinds and their scope | [23](23-configuration-and-registry.md) §2.2 |
| Settings: tenant `site`, instance `site`, tenancy switches, config-record fields, deployment configuration, preference keys | [23](23-configuration-and-registry.md) §3 |

## 4. How to read the chapters and the ADRs

*Sources: chapters as linked.*

The conventions are in [README.md](README.md); this section says only what a reader meets on the page.

**A chapter describes; an ADR decides.** The ADRs in `docs/decisions/` remain the record of why and when: context, alternatives, consequences, open questions and the amendment log. A chapter holds what holds now, assembled from the Decision sections of every ADR that touches its subject, edited into one voice and one order, with duplicates merged. A change to the architecture is still an ADR, and the chapter changes in the same commit.

**Provenance lines.** The first line under every `##` and `###` heading is in italics and names the ADR sections the section was assembled from: *Sources: [0013](../decisions/0013-postgres-storage.md) §7; [0032](../decisions/0032-sparql-update-stream.md) §2.* Inline citations in the body use the same form. Section numbers in a chapter are permanent, like ADR sections, so that `02 §3.1` is a stable citation; chapters are never renumbered, and a new subject gets the next number.

**Pointers.** Since [0050](../decisions/0050-adr-format.md) §14 the ADRs no longer carry their Decision text: each Decision section keeps its heading and provenance line and its body is the line *Current text: [03](03-storage-caches-and-search.md) §4.5.*, naming the chapter sections that hold it; the ADR's header lists its chapters. A change to the architecture is still a decision logged in the ADR (fold, log, mark: 0050 §6), with the fold made in the chapter section the pointer names. Where two ADRs disagreed when the chapters were written, James decided, and each decision is an entry in the owning ADR's Amendment log dated 2026-10-08 or 2026-10-09; the audit that found them is [`../audits/architecture-consolidation-2026-10-08.md`](../audits/architecture-consolidation-2026-10-08.md).

**MAP.md and map.py.** `MAP.md` assigns every numbered Decision section of every ADR to the chapter or chapters that absorb it; it is generated by `map.py --write` and not edited by hand. `python3 architecture/map.py` from `docs/` checks that every ADR section is mapped, that every mapped section is cited in a provenance line of its chapter, that every provenance citation names a section that exists, that every heading has a provenance line, and that `MAP.md` is current; `check_adrs.py` checks the other direction, that every ADR pointer names a chapter section whose provenance cites it. A new ADR needs a `HOME` entry in `map.py`, and usually a few overrides for sections that belong elsewhere.

**Cross-cutting trailers.** Most ADRs end with Crates, API, UI, Storage, Permissions, Search, RDF, Log events, Rate limits, Tenants and Scatterbase sections. Those go to chapters [22](22-crates-and-stack.md), [18](18-api.md), [19](19-site-ui.md), [03](03-storage-caches-and-search.md), [09](09-security-and-moderation.md), [03](03-storage-caches-and-search.md), [02](02-graphs-rdf-and-query.md), [16](16-logs-feeds-and-notifications.md), [07](07-actors-and-accounts.md), [08](08-tenants-and-instances.md) and [22](22-crates-and-stack.md) respectively, and the ADR's home chapter links to them. So a feature's tables are in 03, its routes in 18, its permissions in 09 and its log events in 16, whatever chapter describes the feature.

**The compatibility contracts.** `docs/api/payloads.md` is the wire reference for records; `mediawiki-compat.md` and `wikibase-compat.md` beside it are the compatibility contracts that [18](18-api.md) builds on and does not repeat.

## 5. Glossary

*Sources: chapters as linked.*

Terms the chapters use without defining in place, each with the section that defines it. Where one word has two meanings, both are given.

**ACL.** An access-control entry attached to a target by identifier, restricting named permissions to a group until an expiry; page protection, where the target is a page. Payload type `scatter:v0/acl`; a graph ACL goes to the tenant `config` partition, every other target is moderation and goes to the tenant `log` partition. [09](09-security-and-moderation.md) §1.1, §4.1.

**Activity row.** The one row shape that history, recent changes, contributions and the log share, in `view.activity`; its `kind` is `edit`, `sync`, `upstream-edit`, `log`, `job` or `erased`. [16](16-logs-feeds-and-notifications.md) §3.1.

**Actor.** Who acts: an issuer, the authority that assigns the account, and a subject, the ID it assigned, written as an actor key such as `librarybase:42`. Actors are namespaced by issuer as entities are by provider; the issuer is not the provider. [07](07-actors-and-accounts.md) §1.1.

**Adapter.** A Rust trait implementation, one per provider type, that rewrites IDs (`Q`→`WDQ`), maps non-Wikibase sources onto properties and supplies canonical IRIs; the only crates that name a specific provider. [05](05-providers-and-ingest.md) §3.4; [22](22-crates-and-stack.md) §1.1.

**Adoption.** A tenant adopts a MediaWiki Wikibase when the tenant *is* that wiki, continuing on Triplespace: the source's entities are written into the tenant's `local` partition under the IDs the source minted, its accounts under their own numbers, and every sequence is set past the source's. [05](05-providers-and-ingest.md) §7.1; adopted accounts, [07](07-actors-and-accounts.md) §2.5.

**Attestation.** The part of a record's body that says who made the change: a CBOR map holding the actor and the job, the change tags and a client signature. It is in the body because it can identify people. The **instance attestation** is its second form, for records the instance writes into a tenant on its own authority. [01](01-log-and-records.md) §2.4; [08](08-tenants-and-instances.md) §8.3.

**Authority record.** A record in an instance partition that says what the instance decided and why; every written instance act has one, attested by the operator who acted, with the public reason in its comment part. [08](08-tenants-and-instances.md) §8.4.

**Binding.** A prerogative binds the guest tenant: it cannot be superseded, erased or hidden by the tenant, and only another instance act can change it. [08](08-tenants-and-instances.md) §8.5.

**Block.** A record that removes named permissions from one actor until an expiry; an actor record in the tenant's `actors` partition. A **global block** in `actors/{farm}` removes permissions from every tenant account linked to a farm account. [09](09-security-and-moderation.md) §1.1, §3.3, §3.7.

**Board.** A page in the `Board` namespace: a talk page that is its own subject, with a definition and a listing of the threads attached to it. It exists because someone created it. [14](14-discussions.md) §3.1.

**Bootstrap mode.** The initial-load path that writes records first and builds everything else afterwards: child tables with the primary key only, writers that `COPY` records in parallel in blocks of offsets, indexes and `view` tables built after. [05](05-providers-and-ingest.md) §3.8.

**Bundle.** (1) The export of one or more partitions with the key records needed to verify it, [01](01-log-and-records.md) §8. (2) The **page bundle** the instance keeps for a foreign page in proxy or mirror mode, [13](13-mirrored-pages.md) §2.1. (3) The **user data bundle** an account can download, [07](07-actors-and-accounts.md) §8.4.

**Canonical ID.** The highest-ranked member of an identity cluster, which the resolved view and statement IDs are rewritten to. For a keyed type, the prefix in lowercase and the key in the form the type's normalizer produces. [04](04-entities-and-identifiers.md) §4.4, §3.2.

**Change set.** The single internal type every write uses, submitted by an in-process or CLI ingester, by the HTTP bulk-job endpoint, or by the ordinary edit API as a batch of one. Its operations and wire format are [05](05-providers-and-ingest.md) §3.2, §3.9. [05](05-providers-and-ingest.md) §3.1.

**Checkpoint.** A signed statement of a partition's tree size and root hash, in the C2SP tlog-checkpoint format, signed by the instance key as a C2SP signed note. [01](01-log-and-records.md) §4.2.

**Claim.** The one write a sprint adds to the log: a person's record that they are working on a task. [15](15-structured-pages.md) §3.1, §3.4.

**Compaction.** Removing every record for a key except the newest, in a partition with the `latest` history policy. A compacted offset keeps its leaf hash, so the tree still folds and offsets are never reused. [01](01-log-and-records.md) §6, §4.1.

**Config kind.** The kind of a configuration record in a `config` partition, instance or tenant scope; each is catalogued with its scope. [23](23-configuration-and-registry.md) §2.2.

**Content model.** What every page has exactly one of in its main slot: a registry entry that decides where the page's content comes from, how it is serialized, validated and rendered, and whether `action=edit` can change it. [10](10-pages-and-content-models.md) §4.1.

**Derived graph.** A source graph of a tenant, one per extraction source, `derived/{source}`, holding the statements the source derives from mirrored pages. [13](13-mirrored-pages.md) §6.1; the graph catalogue, [02](02-graphs-rdf-and-query.md) §1.2.

**Document node.** The IRI that stands for the wiki record about a thing (`data:Q8`), to which revision metadata attaches; distinct from the concept IRI (`wd:Q8`), which stands for the thing itself. [02](02-graphs-rdf-and-query.md) §2.1.

**Document page.** A page identified by a page ID, minted by the instance in sequence and never reused, with its title as an attribute. [10](10-pages-and-content-models.md) §3.1.

**Domain.** (1) The keyed entity type whose key is a domain name, `domain:en.wikipedia.org`, hosted in the Domain namespace. [04](04-entities-and-identifiers.md) §3.6; [10](10-pages-and-content-models.md) §3.7. (2) A **storage domain**: the scope within which a packed fragment is stored once; by default each tenant is its own. [08](08-tenants-and-instances.md) §9.2.

**Edit filter.** A named rule, in CEL, evaluated against every candidate write in the tenant's `local` and `pages` partitions after the permission and ACL checks and before the append, with actions taken on a match. [09](09-security-and-moderation.md) §7.1.

**Entity source.** A graph of minted entities that one tenant reads, declared in that tenant's configuration as a `config` record of kind `entity-source` rather than in the provider registry; it adds one `source/{name}` partition and graph. [05](05-providers-and-ingest.md) §6.1, §6.4.

**Erasure.** A record. An `erase` record is appended to the same partition as its targets, names them by offset or by key, names the parts it erases, and gives a reason class (`legal`, `privacy`, `upstream`, `operational`). The header stays; the named parts go. [01](01-log-and-records.md) §5.1.

**Expunge.** The operator's destruction of taken-down material: a record of payload type `scatter:v0/expunge` in the instance `log` only, keyed by the takedown it follows. [12](12-files-and-media.md) §5.5.

**Extraction source.** Tenant configuration, a `config` record of kind `extraction`, that says which mirrored pages statements are derived from and how, through template mappings and line rules. [13](13-mirrored-pages.md) §6.2.

**Farm.** An instance that hosts several tenants under a tenancy policy. The **farm slug** is the instance's own slug, chosen at `instance create` and never changed; the **farm base** is where the instance holds what it keeps on behalf of every tenant; **farm identity** is the optional issuer whose code is the farm slug, whose accounts are the identity a person carries across tenants; the **farm partitions** are `actors/{farm}`, `accounts/{farm}` and `log/{farm}`. [08](08-tenants-and-instances.md) §1.1, §1.2, §3.5, §2.1.

**Feed.** Activity rows filtered by a target set and by filters, delivered as a page, as Atom, or as a stream. [16](16-logs-feeds-and-notifications.md) §4.1, §4.4.

**Fork.** An ordinary local page in the tenant's `pages` partition whose first record is a `create` carrying `forked_from`, naming the foreign page it came from; its history is seeded afterwards. [13](13-mirrored-pages.md) §3.1, §3.4.

**Generation and version.** Every cacheable thing has a **version** number that its cache key carries and that changes when it changes. An entity's or page's **generation** changes only on erasure or hiding, so that no entry at any version remains reachable. A `read` ACL of either kind bumps the generation. [03](03-storage-caches-and-search.md) §9.3, §9.4; [09](09-security-and-moderation.md) §5.1.

**Grant.** A restriction of what an API key or OAuth token may do below what its subsidiary may do; the effective permissions of a request are the subsidiary's intersected with the grants. [07](07-actors-and-accounts.md) §5.3.

**Hiding.** A `record` ACL with `parts` that restricts `read` on one revision's parts (content, comment, attestation), setting the visibility bits the API carries; deletion is the same on a page, suppression the same with a narrower group. [09](09-security-and-moderation.md) §6.1, §6.3.

**Identity cluster.** The set of IDs, from different namespaces, that denote one thing. Each member belongs to a namespace, meaning whoever minted the ID; a cluster has at most one member per namespace; one member is canonical. [04](04-entities-and-identifiers.md) §4.1, §4.2.

**Instance.** One deployment: one log, one Postgres, one set of mirror partitions, one instance key. Every instance has at least one tenant. [08](08-tenants-and-instances.md) §1.1.

**Instance act.** Anything the instance does to a tenant's data on its own authority rather than the tenant's, either **evaluated**, applying a rule at read or write time with nothing written, or **written**, appending a record to the tenant's partition under the instance attestation. A **prerogative** is a binding instance act; a **provision** is a starting value the tenant may change. [08](08-tenants-and-instances.md) §8.1.

**Instance partition.** A partition the instance holds on its own behalf rather than a tenant's: `config`, `log`, `mirror/{provider}`, `actors/{provider}`, `log/{provider}`, `files/{repo}`, and with farm identity the farm partitions. Instance records never live in a tenant's partitions. [01](01-log-and-records.md) §1.4; [08](08-tenants-and-instances.md) §2.1, §7.4.

**Issuer.** The authority that assigns an account; each tenant is one, with its slug as code, and so are Wikidata, Wikimedia central accounts, the farm, the built-in `password` issuer and providers without individual actors. Registered in `issuers.toml`. [07](07-actors-and-accounts.md) §1.1, §2.2; [23](23-configuration-and-registry.md) §4.2.

**Job.** The record of an ingest run or other background work, keyed by a job ID the instance mints, with further records under the same key for its finish, failure or revert; its actor is a subsidiary account, except for an instance job, whose actor is the instance. Jobs live in the local log. [05](05-providers-and-ingest.md) §3.3; [16](16-logs-feeds-and-notifications.md) §2.2.

**Keyed type.** An entity type whose ID is a normalized natural key, not a minted identifier: Domain, Keyword, notation. Each is a registry entry; a keyed entity exists because its key is valid, and its log key is a surrogate. [04](04-entities-and-identifiers.md) §3.1, §3.3, §3.4.

**Key-mapped provider.** A provider whose items of one kind are mapped onto a keyed type's keys through an identity property, rather than rewritten to prefixed IDs; internetdomains.wiki is one. [05](05-providers-and-ingest.md) §4.1.

**Kit.** A page of model `json` under `Project:Kits/` from which `Special:CreateWorkspace` builds a workspace and its components. [15](15-structured-pages.md) §4.6.

**Listing.** A talk page or board a thread appears on besides its home; home and listings together are the thread's **attachments**. [14](14-discussions.md) §3.5.

**Log event.** A record in a log graph with payload type `scatter:v0/logevent`, carrying MediaWiki's log type and action; a local log event is projected from the record that caused it, not written twice. [16](16-logs-feeds-and-notifications.md) §1.3, §2.1.

**Main graph, metadata graph.** The main graph (`resolved`) is the Wikibase-compatible RDF, computed by reconciling the source graphs; the metadata graph holds revision metadata and other projected triples. Neither is written to. [02](02-graphs-rdf-and-query.md) §1.5.

**Mirror graph, mirror partition.** The source graph, one per provider, written only by that provider's sync jobs, and the instance partition holding its records; compacted by default. [05](05-providers-and-ingest.md) §1.2; [01](01-log-and-records.md) §1.2.

**Namespace.** (1) A registry entry with MediaWiki's fields and a kind that says whether it holds pages. [10](10-pages-and-content-models.md) §1.1. (2) In an identity cluster, whoever minted an ID: the local instance, a provider, a keyed type or an entity source. [04](04-entities-and-identifiers.md) §4.1.

**Notification.** One inbox row: an activity, the recipient, the reason an addressing rule named it, and its seen and read state. The inbox is private state. [16](16-logs-feeds-and-notifications.md) §5.1, §5.3.

**Page repository.** A source of pages a tenant serves by title without holding them, configured as a `config` record of kind `page-repo` in a tenant's `config` or the instance's. [13](13-mirrored-pages.md) §1.1.

**Part.** One erasable element of a record's body. Every Triplespace payload type has at least Content, Comment and Attestation, in that order, and a type may declare more. [01](01-log-and-records.md) §2.3.

**Partition.** One sequence of records in the log, split by source graph, identified by 64 random bits, with its own history policy (`full` or `latest`), integrity policy (`logged` or `hashed`), segment size and hash function. [01](01-log-and-records.md) §1.2.

**Primary tenant.** The tenant whose accounts operate the instance; exactly one at every point in the log. It is an identity role, not a storage location, and gives no authority over other tenants. [08](08-tenants-and-instances.md) §7.1.

**Private state.** State that belongs to an account, that nobody else may see and that has no public history: sessions, keys, watches, the inbox, preferences. Rows in the `private` schema, never records, never projected, exported or placed on a feed. [07](07-actors-and-accounts.md) §1.6, §8.3; [03](03-storage-caches-and-search.md) §1.2.

**Projection.** Anything derived from the log: a `view` table, the RDF output, a search index, a scope's membership. Every `view` table belongs to a named projection; `ops.projection_state` records how far each has replayed, and a rebuild truncates and replays from offset 0. [03](03-storage-caches-and-search.md) §6.1; [01](01-log-and-records.md) §1.1.

**Proposal.** A thread homed on a subject's talk page whose records carry a proposal payload: what is offered, to which wiki, compiled from the local state at the moment of proposing. [14](14-discussions.md) §5.1.

**Provider.** A source of minted entities registered in `docs/registry/providers.toml` with a two-letter code and type codes; it names who minted an ID, and its mirror graph names what it asserts. [05](05-providers-and-ingest.md) §1.1, §8.1; [04](04-entities-and-identifiers.md) §1.

**Query service.** The one reader of a local quad store, with two backends, serving compiled queries over what the store holds. [02](02-graphs-rdf-and-query.md) §7.

**Record.** One entry in a partition: a header, which the Merkle tree commits to and which is never erased, and a body of salted parts. The header carries the key, the payload type and the global IDs. [01](01-log-and-records.md) §2.1, §2.2, §2.5.

**Resolved view.** The main graph, computed by applying a reconciliation policy over the source graphs at the level of Wikibase statements, still Wikibase-shaped and still rebuildable from the log. [05](05-providers-and-ingest.md) §1.4.

**Resolver.** A namespace bound to a property, so that a value of that property resolves to a page; a `config` record of kind `resolver`. [06](06-statements-and-properties.md) §3.1.

**Restriction, moderation and confidential.** A `read` ACL is one of two kinds, in one record: **moderation** means the target has been removed (deleted, hidden, suppressed); **confidential** means it exists for a group and for nobody else. They differ in intent, in the right that sets them and in what outsiders see. [09](09-security-and-moderation.md) §5.2.

**Role.** A semantic role a property is bound to, so that shape detection, resolvers and constraints refer to it without knowing its ID; a `config` record of kind `role`. [06](06-statements-and-properties.md) §1.

**Scope.** (1) A page in the `Scope` namespace whose content is a definition of a set of subjects; membership is a projection. A scope is content and is never a target of restriction. [15](15-structured-pages.md) §1.1. (2) The scope of configuration, instance or tenant. [23](23-configuration-and-registry.md) §1.3; [08](08-tenants-and-instances.md) §2.5. (3) A **storage scope** for files. [12](12-files-and-media.md) §2.

**Segment.** 2^k consecutive records of a partition, the exponent fixed at creation; a full segment is a complete subtree of the partition's tree, sealed with a manifest. [01](01-log-and-records.md) §4.1.

**Set.** A grouping that only `protect` can change, named, with members by ID, used as an ACL target so that several things are restricted together. Not a scope. [09](09-security-and-moderation.md) §4.7; [15](15-structured-pages.md) §1.1.

**Shallow mirror.** A mirror that holds the whole current state of each entity it holds and does not follow that entity's links: enough entities, never part of an entity. [05](05-providers-and-ingest.md) §5.1.

**Source graph.** A graph that records are written to: the local graph, the mirror graphs, the derived graphs and the `source/{name}` graphs of entity sources. The metadata graph and the resolved graph are projections, not sources. [05](05-providers-and-ingest.md) §1.2.

**Sprint, task.** A sprint is a `Project:` subpage of model `triplespace-sprint` whose definition names a scope, rules and a window; a task is one (sprint, rule, subject) triple the rule currently finds open, computed in `view.task`. [15](15-structured-pages.md) §3.1.

**Subsidiary.** A local account of kind `bot` whose actor record carries an operator, the actor key of the local account that owns it; the only kind of account that holds API keys, and the actor of every job. [07](07-actors-and-accounts.md) §4.1, §5.1.

**Table.** A page in the `Table` namespace whose content is a definition of which entities are its rows and which fields are its columns; it stores no values. [15](15-structured-pages.md) §2.1.

**Takedown.** The operator's removal of a file from service, recorded in the instance `log`, reversible by reinstatement. [12](12-files-and-media.md) §5.4.

**Tenant.** One wiki on an instance: a base URI, a slug, its own source partitions, its own configuration, its own users. A **tenant-relative ID** names a local entity of whichever tenant reads it. [08](08-tenants-and-instances.md) §1.1; [04](04-entities-and-identifiers.md) §2.3.

**Thread, post.** A thread is a page in the `Thread` namespace with a subject, a home, listings, a status and a history; a post is a record in the tenant's `pages` partition keyed by the thread's page ID, with four parts. [14](14-discussions.md) §1.1, §1.5.

**Title stack.** For a title in a `pages` namespace, every page that exists under it in inheritance order: the local page, then each page repository's page. [13](13-mirrored-pages.md) §1.2.

**Upstream revision record.** The record of a revision the instance learns about from a provider's backfill, history dump or live stream. [01](01-log-and-records.md) §2.6.

**Visibility, visibility set.** The visibility of a target is the set of `read` ACLs on it and on every target that encloses it; a principal may read the target when it holds `read` and satisfies every ACL in the set. A cache key whose value depends on what the viewer may read carries `{vis}`, a hash of the set with the tenant's **visibility epoch**. [09](09-security-and-moderation.md) §5.1; [03](03-storage-caches-and-search.md) §9.4.

**Watch set.** The targets an account watches; rows in `private.watch`, not records. [16](16-logs-feeds-and-notifications.md) §4.3.

**Workspace.** An ordinary `Project:` page in wikitext with a declaration of the scopes it is about and blocks, parser functions that render tables, counts, task boards, feeds and board listings over them. [15](15-structured-pages.md) §4.1.
