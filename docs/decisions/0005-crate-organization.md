# 0005. Crate organization for reuse by Scatterbase

- **Status:** Proposed
- **Date:** 2026-09-25
- **Updated:** 2026-10-02 (A56)
- **Author:** James Hare / Claude Opus; revision by James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md)
- **Uses:** [0000](0000-init.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

Triplespace has no code yet. Its first commit will fix a crate layout, and that layout decides how much of the work can be reused elsewhere.

The main other consumer is **Scatterbase**, a kernel for a signed, append-only claim log over RDF. Scatterbase's project plan (22 September 2026) calls for several things that Triplespace's ADRs already design:

| Scatterbase needs | Plan phase | Triplespace counterpart |
|---|---|---|
| A durable, monotonic log position in place of wall-clock sequence IDs | 1 | The append-only log ([0000](0000-init.md) §1) |
| Named graphs as quads | 1 | Source and projection graphs ([0001](0001-revision-metadata-rdf.md) §2, [0002](0002-source-graphs-and-mass-ingest.md) §2) |
| A `lookup(s, p, o, g)` storage contract | 2 | The quad projection target ([0013](0013-postgres-storage.md) §8) |
| `EventListener` hooks for views and indexes | 2 | Projections rebuilt from the log |
| An entity namespace registry (`WDQ42` → `http://www.wikidata.org/entity/Q42`) | 4 | The provider registry ([0002](0002-source-graphs-and-mass-ingest.md) §4) |
| A Wikibase extension: data types, statements, truthy triples, a Wikidata importer | 4 | The Wikibase data model and RDF contract, reconciliation, and the Wikidata adapter |

The two systems differ in what a log record means:

- **In Triplespace,** a record is a change set ([0002](0002-source-graphs-and-mass-ingest.md) §8). Mirror partitions are compacted, and tombstones erase data.
- **In Scatterbase,** a record is a signed claim envelope. Accepted signed history cannot be rewritten.

Reuse only works if the shared code does not assume either meaning.

Since this ADR was first written, twenty-six further ADRs have added crates, changed the contents of listed ones, and moved two seams. The original table named twenty-two crates; the ADRs through 0018 name thirty-nine. The revision of 2026-09-26 consolidated three pairs (§8) and brought the count to thirty-six; [0019](0019-discussions.md) and [0021](0021-notifications.md) add three more, [0028](0028-tenancy-policy.md), [0030](0030-edit-filters.md) and [0031](0031-property-constraints.md) one each, and [0022](0022-federation.md) and [0025](0025-oauth-server.md) one each, for forty-four.

## Decision

### 1. Four layers

*Changed by A12, A31.*

The workspace is organized in four layers. Dependencies point only downward.

1. **Substrate.** The log and its integrity, projections, providers, identity clusters, normalizers, actors, document pages, threads, and the Activity Streams profile. Knows nothing about Wikibase.
2. **Wikibase domain.** The data model, its RDF and JSON forms, reconciliation, change sets, shape detection, edit filters, property constraints, and MediaWiki's log model. Knows nothing about Triplespace's APIs or deployment.
3. **Ingest.** Jobs, and one adapter per provider, including the verified adapter for another Triplespace instance. The only crates that name a specific provider.
4. **Triplespace surfaces.** The Postgres schemas and projections, the compatibility APIs, accounts, notifications, federation, the OAuth server, caches, search and the binaries. The only layer specific to Triplespace.

Layers 1 and 2 are the shared crates. Layer 3 is shared in principle (Scatterbase's Phase 4 needs the Wikidata adapter) but is where I/O begins. Triplespace is the first product assembled from the lower three, and Scatterbase is expected to be the second.

### 2. Crate map

*Changed by A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25, A26, A27, A28, A29, A30, A31, A32, A33, A34, A35, A36, A37, A38, A39, A40, A41, A42, A43, A44, A45, A46, A47, A48, A49, A51, A52, A53, A54, A55.*

| Layer | Crate | Contents | Depends on |
|---|---|---|---|
| Substrate | `scatter-vocab` | IRI constants for the `scatter:` vocabulary (§5) and the external vocabularies [0001](0001-revision-metadata-rdf.md) §6, [0007](0007-actor-identity.md) §9 and [0011](0011-logs.md) §8 reuse: PROV-O, PAV, SIOC, FOAF, Activity Streams 2.0, Dublin Core, schema.org, and the Wikibase ontology | `oxrdf` |
| | `scatter-log` | An append-only log split into partitions and generic over the record type; the `pages/{repo}` graph and the five-part `scatter:v0/mirrored-page` payload type ([0053](0053-mirrored-pages.md) §5), and the optional `text` part of `scatter:v0/upstream-revision` ([0054](0054-forking-a-mirrored-page.md) §3). The `LogStore` trait (`append`, `read`, `scan`, `erase_parts`, `compact`, `head`) and its file backend, `segments` ([0013](0013-postgres-storage.md) §1, §10). The record header and body formats, hash tags, segment layout and Merkle tree ([0006](0006-log-integrity-and-erasure.md) §2–5). **The graph and partition registry** (§4.1, §4.2), formerly `scatter-graphs`. Header fields 7–9, the provider-ranged revision ID and the body of erasable parts ([0015](0015-record-format-and-partition-registry.md) §1–2). The `config` payload type with kinds `key` and `graph`, the `membership` and `block` types ([0015](0015-record-format-and-partition-registry.md) §3, [0016](0016-permissions-and-access-control.md) §3), and the `acl` payload type, appended to `config` for graph targets and to `log` for moderation ([0023](0023-moderation.md) §3). The `tenancy`, `template` and `provider-readers` kinds and `scope` on `group` ([0028](0028-tenancy-policy.md) §14). Durable offsets, tombstones, bootstrap segment writing, and replay from any offset. Embeds `docs/registry/graphs.toml` as the default registry ([0015](0015-record-format-and-partition-registry.md) §5); the instance `log` partition and `files/{repo}` mirror partitions in the graph registry ([0039](0039-files-and-media.md) §10–11); the instance attestation, domain tag `0x06` and the refusal of submitted instance attestations ([0040](0040-instance-prerogatives.md) §3) | `oxrdf` |
| | `scatter-log-postgres` | `LogStore` over the `log` schema of [0013](0013-postgres-storage.md) §2, and that schema's migrations; the `page_id` column and the erased-parts bitmask ([0015](0015-record-format-and-partition-registry.md) §7); the per-type entity-ID sequences, sequence floors and an explicit page ID on append ([0035](0035-adopting-a-wikibase.md) §4). The one substrate crate that is async. A blocking API is possible through the synchronous `postgres` crate, a wrapper over the same driver, without a second implementation ([0033](0033-backend-stack.md) §4) | `scatter-log`, `tokio-postgres` |
| | `scatter-blob` | The blob store over `object_store`: the `fs` and `s3` backends, storage scopes and the content-addressed key layout, streaming SHA-256 and SHA-1, copy-if-absent, ranged and streamed reads, prefix deletion, and the listing the orphan sweep and `verify` walk ([0039](0039-files-and-media.md) §3–4). Shared with Scatterbase, whose blobs are the same objects. Async, like `scatter-log-postgres` | `object_store` |
| | `scatter-integrity` | Signed checkpoints, inclusion and consistency proofs, erasure bookkeeping, export bundles and `verify`, checked per part ([0006](0006-log-integrity-and-erasure.md) §6–9, [0015](0015-record-format-and-partition-registry.md) §1); verifying a remote provider's checkpoint, key chain, inclusion and consistency proofs from fetched bytes ([0022](0022-federation.md) §2); checking a record's client signature against the actor's key records in the bundle ([0015](0015-record-format-and-partition-registry.md) §1); blob verification at `presence` and `full` depth, and export bundles with `--blobs` ([0039](0039-files-and-media.md) §14); verifying instance attestations against the key chain, and the authority extract ([0040](0040-instance-prerogatives.md) §8) | `scatter-log`, `scatter-blob` |
| | `scatter-quadstore` | The `lookup(s, p, o, g)` trait with wildcards, `apply(deleted, inserted)` for a delta ([0032](0032-sparql-update-stream.md) §9), an N-Quads dump, and a reference Oxigraph implementation behind a feature. In Triplespace a consumer of `view.rdf_delta`, not the serving store ([0013](0013-postgres-storage.md) §8) | `oxrdf` |
| | `scatter-projection` | The projection trait, including a transactional variant that applies inside the appending transaction ([0013](0013-postgres-storage.md) §7, §10); projection positions per partition, rebuilding from offset 0, and lag reporting | `scatter-log` |
| | `scatter-providers` | Provider codes, slugs and numbers, type codes, canonical IRI templates, and ID rewriting such as `Q123` ↔ `WDQ123` ([0002](0002-source-graphs-and-mass-ingest.md) §4, [0015](0015-record-format-and-partition-registry.md) §2, §5). Embeds `docs/registry/providers.toml`; the reserved doubled codes `AA`–`ZZ` ([0044](0044-tenant-relative-ids.md) §2); a provider that mints no entities, with no code and no types ([0052](0052-page-repositories-and-title-inheritance.md) §1) | `oxrdf` |
| | `scatter-identity` | Identity clusters: one member per namespace, link tiers, `different-from`, held conflicts ([0004](0004-identity-clusters-and-equivalence.md) §1–5, §10). Generic over namespace and ID types | — |
| | `scatter-normalize` | Normalized value keys by data type and by property ([0004](0004-identity-clusters-and-equivalence.md) §7), including the URL normalizer for sitelinks and the `url` data type ([0026](0026-sitelinks.md) §1), and resolver grammars and normalizers with their case rule, embedding `docs/registry/resolvers.toml` ([0029](0029-resolver-namespaces.md) §1, §9). **The keyed-type registry, key grammars and normalizers, IRI templates and the surrogate allocator trait** ([0009](0009-keyed-entity-types-and-domain.md) §1–2, §6–7), formerly `scatter-keyed`; the notation scheme registry, embedding `docs/registry/notation-schemes.toml`, with its normalizers and grammars ([0048](0048-notation.md) §2). Builds for `wasm32` | `oxrdf`, `idna` |
| | `scatter-actors` | The issuer registry, actor keys, IRI templates, the surrogate allocator trait and the account-link rules ([0007](0007-actor-identity.md) §1–2, §5, §7); the permission model: groups, memberships, blocks, ACL targets and enclosure, and effective-permission evaluation for writes and for reads, with `parts` on `record` targets and the `actor` target ([0016](0016-permissions-and-access-control.md) §3–4, [0023](0023-moderation.md) §1–2); the autopatrol rule as a pure function over memberships as of a time (0023 §6); the operator attribute and `retired` status, block inheritance from the operator, grants intersected with permissions, and rate-limit policy evaluation ([0024](0024-subsidiary-accounts.md) §1, §3–5); evaluation over a linked farm account for global groups and global blocks, template and lock checks, and the tenancy policy type ([0028](0028-tenancy-policy.md) §1, §3–4, §8); the `statement` and `property` ACL targets and the predicate axis of enclosure ([0023](0023-moderation.md) §2, as amended); the `pending` status and its withholding of implicit `user` membership ([0025](0025-oauth-server.md) §3); the `scatter:v0/key` record and the current-key lookup for an actor, and the built-in `password` issuer ([0015](0015-record-format-and-partition-registry.md) §1 and [0007](0007-actor-identity.md) §3, as amended 2026-09-27). Embeds `docs/registry/issuers.toml`, `groups.toml`, `grants.toml` and `tenancy.toml`; the instance-scope `blob` ACL target, the content axis of enclosure, restriction to no group, and the file and operator rights ([0039](0039-files-and-media.md) §10, §21); the `instance` issuer and operator actor, instance rights, and prerogatives as a floor in evaluation ([0040](0040-instance-prerogatives.md) §2, §5, §9); the `tenant` and `set` ACL targets, the visibility set of a target and the two kinds of `read` restriction, the include subset test and the lock-out check ([0056](0056-security-model.md) §3–4, §6, §16) | `oxrdf`, `ed25519-dalek` |
| | `scatter-pages` | Page records and their operations (`create`, `edit`, `move`; `delete` and `undelete` are ACLs since [0023](0023-moderation.md) §4), page IDs, the content-model trait, and validation and rendering for `json`, `yaml`, `text` and, behind the `markdown` feature, CommonMark with wiki links and the HTML sanitizer ([0008](0008-namespaces-and-document-pages.md) §4–5, [0019](0019-discussions.md) §5). Formerly also `scatter-markdown`. Builds for `wasm32`; markdown image syntax with a file title as target ([0039](0039-files-and-media.md) §13); the content model registry, embedding `docs/registry/content-models.toml`, and the trait with source, slot, format, direct editing, serialization and diff ([0041](0041-content-models.md) §3, §5); the `Scribunto` model entry and its syntax-check hook ([0043](0043-lua-modules.md) §3); `forked_from` on `create` ([0054](0054-forking-a-mirrored-page.md) §1); the `page-repo` config kind, which replaces `template-repo`, and the `page-alternates` role ([0052](0052-page-repositories-and-title-inheritance.md) §1, §5); the `sanitized-css` model ([0055](0055-templatestyles-templatedata-and-page-properties.md) §1). Markdown is rendered by `comrak` and sanitized by `ammonia`, which `scatter-wikitext` shares ([0033](0033-backend-stack.md) §9.1–9.2) | `scatter-css` |
| | `scatter-wikitext` | Parsing and rendering the wikitext subset of [0008](0008-namespaces-and-document-pages.md) §8. The link resolver is passed in as a trait. Builds for `wasm32`; extraction of category links, sort keys, `DEFAULTSORT` and `__HIDDENCAT__`, and the leading-colon link ([0038](0038-page-metadata-and-categories.md) §3); file embeds with MediaWiki's options, `<gallery>`, `[[:File:…]]` and `[[Media:…]]` links ([0039](0039-files-and-media.md) §13); renders expanded text with its strip state, and extracts links, categories, file links, external links and indicators from it; `poem` and `indicator` ([0042](0042-template-expansion-and-parsoid.md) §7, §9); the redirect line and `mw-redirect` link classes ([0051](0051-page-redirects.md) §1–2); `<templatestyles>` and `<templatedata>`, with TemplateData's schema validation and parameter table ([0055](0055-templatestyles-templatedata-and-page-properties.md) §3, §5). The parser is a vendored `parse-wiki-text-2` ([0033](0033-backend-stack.md) §9.1) | `scatter-css` |
| | `scatter-wikitext-expand` | MediaWiki's preprocessor and frame expansion: the preprocessor tree, frames, strip markers, pre-save `subst:`, core variables and parser functions, ParserFunctions (`#expr`, `#time` and the rest, written from documentation and tests), the registry embedding `docs/registry/wikitext-functions.toml`, the `ExpandHost` trait, and the render manifest and limit-report types ([0042](0042-template-expansion-and-parsoid.md) §4–6, §10); `#invoke`, `#property` and `#statements` call out through the host ([0043](0043-lua-modules.md) §7); `SHORTDESC`, `__DISAMBIG__` and the other page properties, and the stylesheet lookup through the host ([0055](0055-templatestyles-templatedata-and-page-properties.md) §3, §6). Pure, with no C dependencies; the include rule through the host, a refused include being a missing page ([0056](0056-security-model.md) §6) | ICU4X crates ([0033](0033-backend-stack.md) §9.3) |
| | `scatter-css` | CSS Syntax Level 3 parsing, the TemplateStyles-policy sanitizer (`validate` and `sanitize`), selector scoping under `.mw-parser-output`, canonical serialization, and the allow-list embedding `docs/registry/css-properties.toml` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §2). Pure; builds for `wasm32` | `cssparser` ([0033](0033-backend-stack.md) §16) |
| | `scatter-threads` | The `scatter:v0/thread` payload type and its four parts; the fold from records to thread state (tree, subject, attachment, status); thread title minting; validation of operations against thread state ([0019](0019-discussions.md) §1–4; `delete` and `undelete` are ACLs since [0023](0023-moderation.md) §4); embeds `docs/registry/thread-statuses.toml` (0019 §6). Pure. 0019 §13 also names `scatter-log`; the dependency is not needed, since a payload type is a value the log stores, as `scatter-pages` already shows. Implements the `triplespace-thread` content model ([0041](0041-content-models.md) §5); the home-and-listings fold, `attach` and `detach`, and the `triplespace-board` model ([0049](0049-boards.md) §3–6); `status` on `create`, `mediaType`, `imported_from` and the import-supplied title date ([0054](0054-forking-a-mirrored-page.md) §5) | `scatter-pages` |
| | `scatter-files` | The `scatter:v0/upload` and `scatter:v0/expunge` payload types ([0039](0039-files-and-media.md) §2, §10); the `file-name` normalizer; the file-type registry, embedding `docs/registry/file-types.toml`; type detection and the script and SVG checks (§5); metadata extraction; the transform grammar and the raster and SVG thumbnailers (§6). Pure | `image`, `resvg` |
| | `scatter-activitypub` | The Activity Streams 2.0 profile of [0019](0019-discussions.md) §10 and [0021](0021-notifications.md): object building, actor, WebFinger and NodeInfo documents, HTTP Signatures (signing and verification) and inbox activity parsing ([0021](0021-notifications.md) §5, §10); Person, Group and Collection actor documents, `Announce`, `Update`, `Delete` and `Tombstone`, and inbound activity parsing with signature verification ([0022](0022-federation.md) §6–8). Pure | `serde_json`, a signature crate (§6) |
| Wikibase | `scatter-wikibase-model` | Entities, terms, sitelinks, statements, snaks, data values and data types; the three ID forms, table-driven by grammar and prefix ([0017](0017-entity-id-grammar.md) §1–3); the `domain`, `keyword` and `notation` entity types and the `wikibase-domain`, `wikibase-keyword` and `wikibase-notation` data types ([0009](0009-keyed-entity-types-and-domain.md) §3, [0017](0017-entity-id-grammar.md) §5, [0048](0048-notation.md) §1); statement keys and hashes, including Wikibase's own snak, reference and value-node hashes over an emulation of PHP `serialize()` ([0006](0006-log-integrity-and-erasure.md) §2, as amended 2026-09-28); canonical JSON ([wikibase-compat.md](../api/wikibase-compat.md) §2–3); sitelinks as URLs with badges, and the site-alias mapping between (site ID, title) and URL ([0026](0026-sitelinks.md) §1–2), embedding `docs/registry/sites.toml`; the search-document builder ([0014](0014-caches-and-search.md) §9); the page subject and `{page ID}$<UUID>` statement IDs, and sitelinks held by page ID for the tenant's own hosts ([0038](0038-page-metadata-and-categories.md) §1, §6); the `localMedia` data type ([0039](0039-files-and-media.md) §12); the entity content models, and the `mediainfo` entity type with derived `M` IDs and its serialization ([0041](0041-content-models.md) §5, §7); tenant-relative IDs (`QQQ5`) parsed to the local form ([0044](0044-tenant-relative-ids.md) §1) | `scatter-providers`, `scatter-normalize`, `scatter-pages` |
| | `scatter-wikibase-rdf` | RDF format 1.0.0, the truthy projection, both dump flavors, and EDTF ([wikibase-compat.md](../api/wikibase-compat.md) §4–7); the triple-set diff of two entity renderings, blank-node skolemization and SPARQL 1.1 Update serialization ([0032](0032-sparql-update-stream.md) §9); `localMedia` values as `Special:FilePath` IRIs ([0039](0039-files-and-media.md) §12); `skos:notation` and `skos:inScheme` for notations ([0048](0048-notation.md) §5) | `scatter-wikibase-model`, `scatter-vocab`, `scatter-providers` |
| | `scatter-wikibase-resolve` | Reconciliation across source graphs and statement fusion ([0002](0002-source-graphs-and-mass-ingest.md) §3, [0004](0004-identity-clusters-and-equivalence.md) §6–8); sitelink reconciliation by URL and host and the denied-host filter ([0026](0026-sitelinks.md) §3–4). The graph order and the sitelink lists are passed in as data, not read from a registry | `scatter-wikibase-model`, `scatter-identity`, `scatter-normalize` |
| | `scatter-wikibase-changeset` | The change-set operations and the NDJSON wire format ([0002](0002-source-graphs-and-mass-ingest.md) §8, [0004](0004-identity-clusters-and-equivalence.md) §9, [0009](0009-keyed-entity-types-and-domain.md) §10), the `put` fields of [0012](0012-api-requirements.md) §2.2, eager validation, and the `Adapter` trait with its optional key-mapping capability ([0009](0009-keyed-entity-types-and-domain.md) §9, §12); the `adopt` operation and its source fields ([0035](0035-adopting-a-wikibase.md) §3); change sets keyed by page ID, restricted to statements ([0038](0038-page-metadata-and-categories.md) §1) | `scatter-wikibase-model`, `scatter-providers` |
| | `scatter-wikibase-constraints` | Parsing constraints from a property's statements by role; one checker per constraint type over a resolved entity, with a `Lookup` trait for types that need other entities or indexes; severity, scope, exceptions; the `wbcheckconstraints` result shape ([0031](0031-property-constraints.md) §1–2, §9). Pure | `scatter-wikibase-model`, `scatter-normalize` |
| | `scatter-wikibase-shape` | Shape detection and the role map ([0003](0003-statement-ui.md) §3, §7). Builds for `wasm32`; the `triplespace-table` content model: definition types, schema validation, canonical serialization and the cell rule ([0045](0045-table-content-model.md) §3–5) | `scatter-wikibase-model`, `scatter-pages` |
| | `scatter-filter` | The `filter` and `filter-hit` payload types; the change-set, text and actor contexts and their derivation; the CEL environment with AbuseFilter-compatible functions; compiled-rule evaluation and the action decision ([0030](0030-edit-filters.md) §2–5). Pure; the `upload` context ([0039](0039-files-and-media.md) §5) | `scatter-wikibase-changeset`, `scatter-pages`, `scatter-threads`, `scatter-normalize`, `cel` ([0033](0033-backend-stack.md) §9.4) |
| | `scatter-mwlog` | The `logevent` payload type, MediaWiki log types and actions, typed parameters, visibility bits, both parameter formats, and the mapping to Activity Streams types ([0011](0011-logs.md) §3, §7–8, §10); the `upstream-revision` payload type ([0015](0015-record-format-and-partition-registry.md) §4); the `patrol` and `suppress` types and the `protect/*` and `delete/*` parameters of [0023](0023-moderation.md) §3, §6; the `oauth/*` actions ([0025](0025-oauth-server.md) §5); `import/upload` and `import/interwiki` for adopted entities ([0035](0035-adopting-a-wikibase.md) §6); `upload/*`, `takedown/*` and `import/upload` for files ([0039](0039-files-and-media.md) §16); `instance: true` and the authority link in log parameters ([0040](0040-instance-prerogatives.md) §7); `import/interwiki` for forks ([0054](0054-forking-a-mirrored-page.md) §1) | `scatter-vocab` |
| | `scatter-wasm` | `wasm-bindgen` bindings for the browser over the pure crates editors need: the shape classifier, `scatter-normalize`, and `scatter-wikitext` with the `scatter-pages` markdown renderer, for live preview. Lazy-loaded by editors only, never needed for reading ([0034](0034-frontend-stack.md) §6); re-exports `scatter-css` for linting stylesheets ([0055](0055-templatestyles-templatedata-and-page-properties.md) §9) | `scatter-wikibase-shape`, `scatter-normalize`, `scatter-wikitext`, `scatter-pages`, `scatter-css`, `wasm-bindgen` |
| Ingest | `scatter-ingest` | Jobs, version cursors, `upsert` and `snapshot` modes with the tombstone threshold, temporary refs, match keys, ID blocks, change summaries and optional deltas ([0012](0012-api-requirements.md) §2.2), and parallel decompression and parsing ([0002](0002-source-graphs-and-mass-ingest.md) §8.3–8.6); per-operation edit filtering of local jobs, with disallowed operations sent to the rejects file and counted separately ([0030](0030-edit-filters.md) §7); the adoption job with its preconditions, idempotent resume, sequence floors and adopted actor records ([0035](0035-adopting-a-wikibase.md) §2, §4, §5); the hash guard: recompute, compare, keep on mismatch, count by value type ([0006](0006-log-integrity-and-erasure.md) §2, as amended 2026-09-28) | `scatter-log`, `scatter-projection`, `scatter-wikibase-changeset`, `scatter-filter` |
| | `scatter-adapter-wikidata` | The Wikidata adapter: entity dumps and streams, actor splitting ([0007](0007-actor-identity.md) §5), the logging dump, `list=logevents` and log events from EventStreams, title-to-key mapping and the reconciliation sweep ([0011](0011-logs.md) §4–5, §10); upstream revision records from history dumps, `prop=revisions` and the stream ([0015](0015-record-format-and-partition-registry.md) §4); resolving upstream sitelink site IDs to URLs from the provider's site table ([0026](0026-sitelinks.md) §4); adoption mode, reading a MediaWiki+Wikibase dump or API for the tenant's own provider entry and emitting IDs in home form ([0035](0035-adopting-a-wikibase.md) §3); `key_scheme` in a key map, for mapping onto notations ([0048](0048-notation.md) §6); reading RevisionChest's `.mwrev.zst` files through `scatter-adapter-mediawiki`, where the reader now lives ([0033](0033-backend-stack.md) §10, [0053](0053-mirrored-pages.md) §12) | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers`, `scatter-actors`, `scatter-mwlog`, `scatter-adapter-mediawiki` |
| | `scatter-adapter-openalex` | The OpenAlex adapter, with its field-to-property mapping by role ([0004](0004-identity-clusters-and-equivalence.md) §6) | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers` |
| | `scatter-adapter-musicbrainz` | The MusicBrainz adapter: MBIDs under the `uuid` grammar, URL entities as statements ([0017](0017-entity-id-grammar.md) §6) | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers` |
| | `scatter-adapter-triplespace` | Reading a Triplespace provider tenant on another instance: bootstrap from its local-graph source dump, then its activity stream filtered to `source = local` as the version cursor, writing `put`, `redirect` and `tombstone` records; the back-rewrite of the reader's own IDs; verification per batch against the provider's checkpoint and key chain, storing the provider header; following `erase` and `delete/statement` as `upstream` erasure ([0028](0028-tenancy-policy.md) §5, [0022](0022-federation.md) §2) | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers`, `scatter-integrity` |
| | `scatter-adapter-internetdomains` | The key-mapped adapter for internetdomains.wiki ([0009](0009-keyed-entity-types-and-domain.md) §9, §12) | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers`, `scatter-normalize` |
| | `scatter-adapter-openstreetmap` | The OpenStreetMap map-data adapter: nodes, ways, relations, tags as statements ([0036](0036-openstreetmap-providers.md) §2, §4) | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers`, `scatter-normalize` |
| | `scatter-adapter-gdelt` | The GDELT 2.0 adapter: events, mentions and GKG files by 15-minute slot, with row filters ([0037](0037-gdelt-provider.md) §5, §6) | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers`, `scatter-normalize` |
| | `scatter-adapter-mediawiki` | Reading a MediaWiki wiki's pages: page bundles from the REST and Action APIs and the Wikimedia Enterprise API, the `page`, `redirect` and `all-titles` dumps for the title index, `mediawiki.page-change.v1` events, the independent `.mwrev.zst` reader for RevisionChest stores (moved from `scatter-adapter-wikidata`), upstream revision records for pages from a store or `prop=revisions`, and the talk-page splitter ([0053](0053-mirrored-pages.md) §12, [0054](0054-forking-a-mirrored-page.md) §3, §5, §12) | `scatter-providers`, `scatter-mwlog`, `scatter-pages`, `scatter-threads` |
| Triplespace | `triplespace-db` | The `view`, `private` and `ops` schemas, their migrations, roles and grants, connection pools, and replica routing by LSN ([0013](0013-postgres-storage.md) §4, §7, §10); the portability class on each `private` table ([0027](0027-preferences-and-portability.md) §2); `view.transclusion`, `view.render_state`, `ops.render_refresh` ([0042](0042-template-expansion-and-parsoid.md) §10) and `view.entity_usage` ([0043](0043-lua-modules.md) §10) | `scatter-log-postgres` |
| | `triplespace-projections` | Every `view` projection, in the order of [0013](0013-postgres-storage.md) §7: registry, surrogates, actors, entity sources, resolution, terms, identifiers, references, clusters, corrections, pages, links, activity, jobs, upstream revisions ([0015](0015-record-format-and-partition-registry.md) §4), groups, memberships, ACLs and blocks ([0016](0016-permissions-and-access-control.md) §9), threads, posts and talk pages ([0019](0019-discussions.md) §11), and the ACL and patrol projections with the `deleted`, `visibility`, `hidden`, `status` and `patrolled` flags they derive ([0023](0023-moderation.md) §10); the sitelink projection with the denied-host filter and re-resolution on policy change ([0026](0026-sitelinks.md) §6); `view-pin` records ([0027](0027-preferences-and-portability.md) §5); the all-tenants target set, reader lists at partition open and cross-tenant thread reads ([0028](0028-tenancy-policy.md) §14); `view.filter` and `view.filter_hit`, the filter-hits target set and the filter test job ([0030](0030-edit-filters.md) §11); the constraint projection with `view.constraint_violation`, `view.constraint_count` and `view.value_key`, and the property-wide re-check job ([0031](0031-property-constraints.md) §5); the delta projection writing `view.rdf_delta` inside the source-graph, resolution and metadata projections, with the tenant filter, overlay transitions, epoch, retention sweep and erasure purge ([0032](0032-sparql-update-stream.md) §2–5). The registry projection reads the `config` partition ([0015](0015-record-format-and-partition-registry.md) §3). Also the read side of activity: feed specifications, target-set expansion, the activity query for every target set and the related-changes joins ([0020](0020-change-feeds.md) §1–2, §8). Absorbs `triplespace-activity` ([0010](0010-site-ui.md) §13). Pure computation stays in the layer-1 and layer-2 crates; this crate reads records, writes rows and runs the queries over them; `view.page_category`, `view.category`, the category-mapping and thread-status projections, `view.page_statements` resolution, page subjects in `statement_assertion`, `entity_ref` and constraint checking, and the mapping re-run and make-it-real jobs ([0038](0038-page-metadata-and-categories.md) §2–5, §9–10); `view.file`, `view.file_version`, `view.blob` with reference counts, `view.file_link`, `view.file_block` and the metadata projection ([0039](0039-files-and-media.md) §20); the operator as performer, the Instance action label and `scatter:authority` ([0040](0040-instance-prerogatives.md) §7); `view.page.content_model` from the content model registry and the `content_model` registry kind ([0041](0041-content-models.md) §10); `page_link` rows from table definitions ([0045](0045-table-content-model.md) §8); `view.redirect` and `is_redirect` ([0051](0051-page-redirects.md) §5); `view.fork` ([0054](0054-forking-a-mirrored-page.md) §10); `view.page_prop` and its writers ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6); `set_member`, `read_groups` on activity rows, the visibility epoch and the restriction kind on `acl`, with re-projection on a `read` ACL change ([0056](0056-security-model.md) §14, §16) | `triplespace-db`, `scatter-projection`, any layer-1 or layer-2 crate (including `scatter-filter` and `scatter-wikibase-constraints`) |
| | `triplespace-rdf` | The streaming RDF projection: resolved view (the main graph), source graphs and metadata graph as N-Quads, for the Wikibase-compatible and full dumps, QLever and an optional local quad store ([0013](0013-postgres-storage.md) §8), with the IRI rules of [0015](0015-record-format-and-partition-registry.md) §4–6. Absorbs `triplespace-revmeta` ([0001](0001-revision-metadata-rdf.md), [0011](0011-logs.md) §10); the local-graph source dump ([0022](0022-federation.md) §1); cursor-stamped dumps, the skolemized dump form, the update batch files and `manifest.json` ([0032](0032-sparql-update-stream.md) §6–7); `prov:wasRevisionOf` and `pav:importedOn` on adopted records ([0035](0035-adopting-a-wikibase.md) §6); page statements and page nodes in the main graph, and the sitelink article node of a paired page ([0038](0038-page-metadata-and-categories.md) §11) | `triplespace-db`, `scatter-wikibase-rdf`, `scatter-vocab`, `scatter-mwlog`, `scatter-quadstore` |
| | `triplespace-titles` | The namespace registry, title normalization and the title resolver ([0008](0008-namespaces-and-document-pages.md) §1–3); reserved page IDs for create-protected titles ([0023](0023-moderation.md) §2); the `resolver` namespace kind, the lookup over `view.identifier` and `view.sitelink`, and the `/resolve` ordering ([0029](0029-resolver-namespaces.md) §2–3, §6); the main and `Category` namespaces, bare titles as main-namespace titles, and main-namespace titles last in the `/resolve` order ([0038](0038-page-metadata-and-categories.md) §4, §8); the `file` namespace kind and `file-name` normalizer, `Media:` resolution and fall-through to file repositories ([0039](0039-files-and-media.md) §1, §11); the `pages` kind with allowed and default models and the `uploads` flag, replacing the entity, document, file, composite and thread kinds ([0041](0041-content-models.md) §4); conditional namespaces (`enabled_by`), Template and Template talk, and the disabled-namespace refusal ([0042](0042-template-expansion-and-parsoid.md) §3); Module and Module talk with their title rules ([0043](0043-lua-modules.md) §2); tenant-relative IDs in the `entity-id` normalizer ([0044](0044-tenant-relative-ids.md) §5); Table and Table talk with the `/doc` rule ([0045](0045-table-content-model.md) §2); the `Notation` namespace and notation keys in the `/resolve` order ([0048](0048-notation.md) §2, §7); following redirects one hop ([0051](0051-page-redirects.md) §2); the title stack over `view.page` and `view.foreign_title`, `origin` views and `GET /page/stack` ([0052](0052-page-repositories-and-title-inheritance.md) §2–3, §10); `Special:Fork` ([0054](0054-forking-a-mirrored-page.md) §2) | `triplespace-db`, `scatter-pages`, `scatter-wikibase-model`, `scatter-identity`, `scatter-normalize` |
| | `triplespace-accounts` | Bindings, OAuth login, the linking flow, and the operational store for secrets; the only crate that reads the accounts partition, and one of the three that read the `private` schema ([0007](0007-actor-identity.md) §3, §7, §10; [0013](0013-postgres-storage.md) §4); resolves a session to its effective permissions and holds IP blocks in `private` ([0016](0016-permissions-and-access-control.md) §3, §9); the watch set, its Atom token, expiry sweep and auto-watch ([0020](0020-change-feeds.md) §3, §8); contact details, the notifier key and the verification flows ([0021](0021-notifications.md) §5, §8, §10); subsidiary accounts, `private.api_key`, bearer and bot-password authentication and session–key revocation ([0024](0024-subsidiary-accounts.md) §2, §4); the preference store with its key registry, embedding `docs/registry/preferences.toml`, the user data bundle's export and import, and applying a private extract on reclaim ([0027](0027-preferences-and-portability.md) §1, §3–4); farm accounts, the name registry, auto-creation and auto-linking, cascading rename and vanish, and the aggregated-inbox join ([0028](0028-tenancy-policy.md) §2, §7); `private.oauth_token` and `private.oauth_consumer`, token authentication beside key authentication, session–token revocation, and revoking tokens on retirement and transfer ([0025](0025-oauth-server.md) §5, §8); the per-hit IP row in the abuse-handling store ([0030](0030-edit-filters.md) §11); `private.password` and password login for the built-in issuer ([0007](0007-actor-identity.md) §3, as amended); signing-key registration, rotation and revocation for subsidiaries, and signature verification on the write path ([0024](0024-subsidiary-accounts.md) §4, as amended) | `triplespace-db`, `scatter-actors` |
| | `triplespace-notify` | The addressing projection, inbox storage and read state, bundling, the web, email and fediverse channels and the delivery queue in `ops` ([0021](0021-notifications.md) §1–5, §8, §10); reads the `notifications.*` preferences in place of a matrix table ([0027](0027-preferences-and-portability.md) §1); the aggregated bell over linked tenant accounts under farm identity ([0028](0028-tenancy-policy.md) §7); the `filter` reason ([0030](0030-edit-filters.md) §4). The second crate that reads `private` ([0013](0013-postgres-storage.md) §4). ActivityPub delivery itself moved to `triplespace-federation` ([0022](0022-federation.md) §13) | `triplespace-db`, `triplespace-projections`, `triplespace-accounts`, `scatter-activitypub` |
| | `triplespace-federation` | Per-actor ActivityPub keys and followers in `private` (`ap_key`, `ap_follower`), Person and Group actor endpoints, outbox fan-out through the `ops` queue, inbox handling, the `federated` surrogate flow and the `federation-policy` lists ([0022](0022-federation.md) §6–9). The third crate that reads `private` ([0013](0013-postgres-storage.md) §4) | `triplespace-db`, `triplespace-projections`, `triplespace-accounts`, `scatter-activitypub`, `scatter-actors` |
| | `triplespace-oauth` | The OAuth 2.0 authorization server: metadata, authorize, token, device, revoke and identify endpoints at native and MediaWiki paths, PKCE, the consent page, the consumer registry and `consumer-policy` evaluation, and the `oauth:{slug}` tag on the request path ([0025](0025-oauth-server.md) §1–4). Reaches `private` only through `triplespace-accounts` | `triplespace-accounts`, `triplespace-cache`, `scatter-actors` |
| | `triplespace-files` | The upload pipeline and stash; the media routes, signed URLs and response headers; thumbnail rendering and caching; reclamation, the orphan sweep and `ops.blob_delete`; takedown and expunge execution; the `tenant` and `mediawiki` file repositories in `proxy`, `link` and `mirror` modes, and the mirror job; file import and the adoption step ([0039](0039-files-and-media.md) §5–11, §14) | `triplespace-db`, `triplespace-projections`, `triplespace-cache`, `triplespace-upstream`, `scatter-blob`, `scatter-files`, `scatter-actors` |
| | `triplespace-upstream` | The live upstream client: cache, rate limits and the error mapping of [0012](0012-api-requirements.md) §6; `prop=imageinfo` and rendered descriptions from `mediawiki` file repositories ([0039](0039-files-and-media.md) §11) | `scatter-adapter-wikidata`, `scatter-mwlog`, `triplespace-cache` |
| | `triplespace-render` | The render pipeline: `ExpandHost` and the Lua host over the serving model, renderer choice, the Parsoid client and manifest collection for the render-scoped endpoint, the refresh job with its queue and invalidation fan-out, and foreign template repositories ([0042](0042-template-expansion-and-parsoid.md) §8, §10–11); aspect-matched invalidation from `view.entity_usage` and the unheld-entity expiry ([0043](0043-lua-modules.md) §10–11); `ts-inherited` and `extiw` link classes, and template, module and `#ifexist` lookups through the stack ([0052](0052-page-repositories-and-title-inheritance.md) §4, §10); stylesheet manifest entries, `<style>` deduplication and the site-styles route ([0055](0055-templatestyles-templatedata-and-page-properties.md) §3–4) | `triplespace-db`, `triplespace-projections`, `triplespace-titles`, `triplespace-cache`, `triplespace-upstream`, `scatter-wikitext`, `scatter-wikitext-expand`, `scatter-pages`, `triplespace-scribunto` |
| | `triplespace-repos` | Page repositories: bundle fetching through `triplespace-upstream`, the HTML rewrite, the title index job, the proxy caches, the `pages/{repo}` sync job and its projection, event following and invalidation, the `pages-{repo}` indexes and the repository routes ([0053](0053-mirrored-pages.md) §2–8, §12); the fork job, with history seeding, dependencies, talk conversion, thread moves, the file job and the upstream diff ([0054](0054-forking-a-mirrored-page.md) §3–8, §12) | `scatter-adapter-mediawiki`, `triplespace-upstream`, `triplespace-files`, `triplespace-render`, `triplespace-cache`, `triplespace-search`, `triplespace-projections` |
| | `triplespace-scribunto` | Lua 5.1 through `mlua` in Scribunto's sandbox; Scribunto's and WikibaseClient's Lua, vendored unchanged; the Rust side of their interfaces behind a host trait; the Lua-pattern matcher; data-module trees; the per-tenant Lua ID space; Wikibase Client formatting, `#property` and `#statements`; usage aspects ([0043](0043-lua-modules.md) §4–10). Pure, but GPL-3.0-or-later only and never dual-licensed, because the vendored Lua is GPL ([0043](0043-lua-modules.md) §14) | `scatter-wikitext-expand`, `scatter-wikibase-model`, `scatter-providers`, `mlua` |
| | `triplespace-cache` | Key construction, the in-process and Valkey layers, single flight, generations, HTTP cache headers and tag purges ([0014](0014-caches-and-search.md) §2–6, §9); the rate-limit counters by class ([0024](0024-subsidiary-accounts.md) §5), filter throttle counters ([0030](0030-edit-filters.md) §4) and OAuth authorization and device codes ([0025](0025-oauth-server.md) §8); the file keys and Cache-Tags of [0039](0039-files-and-media.md) §20; the render epoch in `p:` keys and dependency `Cache-Tag`s ([0042](0042-template-expansion-and-parsoid.md) §10); `ld:` keys for data modules ([0043](0043-lua-modules.md) §6); the `{vis}` key segment, the visibility epoch and its purge ([0056](0056-security-model.md) §7) | `triplespace-db` |
| | `triplespace-search` | Index mappings and analysis settings, the indexer projection, query builders, and the Postgres fallback ([0014](0014-caches-and-search.md) §7–9); per-provider and per-tenant indexes with the tenant-first fan-out ([0018](0018-tenants.md) §6) and farm-wide `msearch` by policy ([0028](0028-tenancy-policy.md) §9); `categories` and `statement_keywords` on `pages` ([0038](0038-page-metadata-and-categories.md) §12); file fields on `pages` for CirrusSearch's file keywords ([0039](0039-files-and-media.md) §19); rendered `text`, `source_text` and `hastemplate:` ([0042](0042-template-expansion-and-parsoid.md) §17); `read_groups` on documents, the `terms_set` filter and the fallback's predicate ([0056](0056-security-model.md) §8) | `triplespace-db`, `triplespace-projections`, `scatter-wikibase-model` |
| | `triplespace-ui` | Server rendering: `askama` templates, the builder that emits Codex CSS-only markup, the page frame of [0010](0010-site-ui.md) §2 shared by every page kind, and the pinned Codex assets ([0034](0034-frontend-stack.md) §2) | `scatter-wikibase-shape`, `askama` |
| | `triplespace-api-action`, `triplespace-api-rest`, `triplespace-api-ingest` | The Action API, the `triplespace/v0` REST API with its OpenAPI contract, and the bulk-job endpoint ([mediawiki-compat.md](../api/mediawiki-compat.md), [0002](0002-source-graphs-and-mass-ingest.md) §8.1, [0012](0012-api-requirements.md) §4–5, §9, [0016](0016-permissions-and-access-control.md) §7); the thread routes ([0019](0019-discussions.md) §9); watchlist modules, Atom and the server-sent event stream ([0020](0020-change-feeds.md) §4–5); the Echo-shaped modules, `/inbox`, and the ActivityPub and `.well-known` endpoints ([0021](0021-notifications.md) §5, §7); `protect`, `delete`, `undelete`, `revisiondelete`, `patrol` and the `/acl` extensions ([0023](0023-moderation.md) §8); bot-password `action=login`, the subsidiary and key routes and the `ratelimited` refusal ([0024](0024-subsidiary-accounts.md) §8); `/.well-known/tlog/`, `/dumps/local/` and the provider verification route ([0022](0022-federation.md) §1, §11); the OAuth token check and `oauth-pending` refusal in front of every write, consumer and authorization routes ([0025](0025-oauth-server.md) §9); `wbsetsitelink` by host or URL and the sitelink routes ([0026](0026-sitelinks.md) §7); `action=options` over the registered keys, the preference routes, export and import ([0027](0027-preferences-and-portability.md) §6); the tenancy, global-user, global-block and farm-feed routes at the farm base ([0028](0028-tenancy-policy.md) §11); `/resolve` ordering, `/resolvers` and `wbgetentities` by resolver and key ([0029](0029-resolver-namespaces.md) §6); the AbuseFilter-shaped modules, the filter routes and filter evaluation in the write path ([0030](0030-edit-filters.md) §9); `wbcheckconstraints` and the report routes ([0031](0031-property-constraints.md) §6); the SPARQL Update stream, `/updates` and `/updates/cursor` ([0032](0032-sparql-update-stream.md) §6); the page-statement, category and category-mapping routes, and the category query modules ([0038](0038-page-metadata-and-categories.md) §13); the file modules and routes, and the farm-base takedown routes ([0039](0039-files-and-media.md) §17); `ts-prerogative`, `Special:InstanceAction` and the farm-base instance-action list ([0040](0040-instance-prerogatives.md) §7); `defaultcontentmodel`, the `contentmodel` enumerations, `no-direct-editing` and the `changecontentmodel` codes, the `mediainfo` slot and the Wikibase modules on `M` IDs ([0041](0041-content-models.md) §8–9); `action=expandtemplates`, expansion in `action=parse`, the transclusion modules, the parser properties of `meta=siteinfo`, `linkclasses`, `allmessages`, `/page/{id}/render`, `as_of`, and the render-scoped endpoint ([0042](0042-template-expansion-and-parsoid.md) §8.2, §12, §14); `wbentityusage`, `wblistentityusage` and `wikibase_item` ([0043](0043-lua-modules.md) §16); table definitions in the Action API, the `/table/{pageid}` routes and the mapping from cell edits to change sets ([0045](0045-table-content-model.md) §6, §9–10) | Any lower crate |
| | `triplespace-server`, `triplespace-cli` | Binaries. `triplespace-server` also serves the special pages that `docs/registry/special-pages.toml` marks `served` ([0047](0047-special-pages.md)), including those of [0025](0025-oauth-server.md) §5. `triplespace-cli` creates the instance key, the `owner` group and the primary tenant's sync subsidiaries ([0015](0015-record-format-and-partition-registry.md) §3, [0016](0016-permissions-and-access-control.md) §3, [0024](0024-subsidiary-accounts.md) §1); `tenant export` and `tenant import` carry the private extract ([0027](0027-preferences-and-portability.md) §4); `instance create --tenancy`, `tenancy check` and `tenancy set` ([0028](0028-tenancy-policy.md) §14); `filter test` and `filter import` ([0030](0030-edit-filters.md) §13); `sparql-sync`, the reference consumer of the update stream ([0032](0032-sparql-update-stream.md) §6); `instance create` sets the first administrator's password ([0016](0016-permissions-and-access-control.md) §3, as amended); canonical-CBOR signing of writes with a subsidiary's key ([0024](0024-subsidiary-accounts.md) §4, as amended); `instance create --adopt --owner` and `adopt` ([0035](0035-adopting-a-wikibase.md) §2, §5); `triplespace-server` serves the media routes ([0039](0039-files-and-media.md) §7); `files import`, `files rescope`, `files verify`, `takedown`, `reinstate` and `expunge` ([0039](0039-files-and-media.md) §23); `render refresh` and `render rebuild` ([0042](0042-template-expansion-and-parsoid.md) §19); `instance create --farm-slug`, `primary offer`, `primary withdraw` and `primary accept` ([0046](0046-primary-tenant.md) §9); `triplespace-server` has `server.mode`, `server.trusted_proxies`, `server.admin_listen`, 421 on an unregistered host, the private tenant's landing page and the internal endpoint for Parsoid, and `triplespace-cli` has `instance check` with `--attest` and `--through` ([0056](0056-security-model.md) §10, §13, §16) | Any crate |

Adapters do not depend on `scatter-log` directly. An adapter turns upstream records into change sets; where those change sets go is the ingester's concern. The one exception is transitive: `scatter-adapter-triplespace` depends on `scatter-integrity`, and so on `scatter-log`'s header and proof types, because it verifies a remote provider's proofs ([0022](0022-federation.md) §2). It still never appends.

Crates named by earlier ADRs that no longer exist as separate crates: `scatter-graphs` (in `scatter-log`), `scatter-markdown` (in `scatter-pages`), `scatter-keyed` (in `scatter-normalize`), `triplespace-activity` (in `triplespace-projections`), `triplespace-revmeta` (in `triplespace-rdf`). References to them in [0008](0008-namespaces-and-document-pages.md) §11, [0009](0009-keyed-entity-types-and-domain.md) §12, [0010](0010-site-ui.md) §13, [0011](0011-logs.md) §10, [0012](0012-api-requirements.md) §9 and [0019](0019-discussions.md) §13 read as references to their new homes.

### 3. Rules

*Changed by A8, A10, A33, A42, A43, A54, A56.*

1. **Dependencies point only downward.** No `scatter-*` crate depends on a `triplespace-*` crate. No crate below the surfaces layer names Triplespace or a specific graph IRI, and only the adapters name a specific provider. CI checks the dependency graph against the table in §2.
2. **The core is pure.** `scatter-providers`, `scatter-identity`, `scatter-normalize`, `scatter-actors`, `scatter-pages`, `scatter-wikitext`, `scatter-threads`, `scatter-activitypub`, `scatter-filter`, `scatter-mwlog`, `scatter-wikitext-expand` ([0042](0042-template-expansion-and-parsoid.md) §4), `scatter-css` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §2), `triplespace-scribunto` ([0043](0043-lua-modules.md) §14) and every `scatter-wikibase-*` crate perform no I/O, use no async runtime, and depend on no tokio. `scatter-activitypub` signs and verifies; it never sends. Async appears only in `scatter-log-postgres`, `scatter-ingest` and the surfaces. `LogStore`'s methods return futures (§4.2), which is a type signature, not a runtime: `scatter-log` depends on `std::future` alone, and its file backend does synchronous I/O inside those futures and nothing else. Other runtimes can then call the core without bridging, and a synchronous caller drives a `LogStore` call with any executor.
3. **Policy is data.** The graph and partition registry, the provider registry, the issuer registry, the namespace registry, the keyed-type registry, the role map, the provider order, reconciliation rules and normalizer overrides are values passed in by the caller. They are not enums, constants or globals. [0004](0004-identity-clusters-and-equivalence.md) §9 records them in the log as configuration, in the `config` partition of [0015](0015-record-format-and-partition-registry.md) §3; their defaults come from `docs/registry/` ([0015](0015-record-format-and-partition-registry.md) §5), and [0013](0013-postgres-storage.md) §5.5 projects their current state.
4. **Each hashed type has exactly one canonical encoding,** produced by one function in the crate that owns the type. No other code path computes a hash or signature preimage for it. The encodings are specified in [0006](0006-log-integrity-and-erasure.md) §2.
5. **RDF terms and I/O use the `oxrdf` family** (`oxrdf`, `oxttl`, `oxrdfio`). Shared crates do not define their own term types.
6. **Cargo features select optional heavy dependencies only,** such as Oxigraph, a SIMD JSON parser, or the CommonMark renderer in `scatter-pages`. Features never switch product behavior.
7. **`scatter-wikibase-shape`, `scatter-wikitext`, `scatter-pages` and `scatter-normalize` must build for `wasm32-unknown-unknown`,** so that the statement UI ([0003](0003-statement-ui.md)), the page editor's preview ([0008](0008-namespaces-and-document-pages.md) §11), title normalization in the browser ([0009](0009-keyed-entity-types-and-domain.md) §12) and the classifier audit over a Wikidata dump run the same code as the server. CI builds that target. `scatter-wasm` ([0034](0034-frontend-stack.md) §6) is the one crate that binds them for the browser.
8. **Contracts are tested where they are implemented.** `scatter-wikibase-model` and `scatter-wikibase-rdf` use the files in `docs/api/snapshots` as golden fixtures. `scatter-log` and `scatter-log-postgres` share one conformance suite for `LogStore`.
9. **Schemas are contracts.** A change to the `log` schema needs a new payload or header version and a migration that keeps every existing header verifiable; a change to `view` needs a migration or a rebuild ([0013](0013-postgres-storage.md), Consequences). Migrations live with the crate that owns the schema.

### 4. Seams shared with Scatterbase

#### 4.1 Named graphs are registry entries

*Changed by A2, A3, A6, A10, A11, A14, A31.*

`scatter-log` does not enumerate graphs. Each deployment registers its own. A registry entry records:

- the graph IRI, and the **tenant** it belongs to, or none for an instance-level graph ([0018](0018-tenants.md) §2);
- its **kind**: a source, which log records write to, or a projection, which is derived;
- its **ACL**: which groups may write to it ([0016](0016-permissions-and-access-control.md) §4). This replaces the *writers* field of the first revision;
- its **history policy**: `full` or `latest` ([0002](0002-source-graphs-and-mass-ingest.md) §2);
- its **integrity policy**, `logged` or `hashed` ([0006](0006-log-integrity-and-erasure.md) §4);
- its **export policy**: `public`, published to external stores; `internal`, served by the API but never exported; or `private`, never projected, placed on a feed or exported ([0007](0007-actor-identity.md) §8).

The two products register different sets:

| Role | Triplespace | Scatterbase |
|---|---|---|
| Local assertions | `local` (source, `logged`, public) | `claim` |
| Data from elsewhere | `mirror/{provider}` (source, one per provider, `hashed`, public) | `foreign` |
| Provenance about records | `metadata` (projection) | `meta` (asserted) |
| Log events | `log` (source, `logged`, internal) and `log/{provider}` (source, `hashed`, internal) ([0011](0011-logs.md) §2) | part of `meta` |
| Document pages | `pages` (source, `logged`, public) ([0008](0008-namespaces-and-document-pages.md) §4) | none |
| Destructible data | `accounts` (source, `logged`, private); `actors` (source, `logged`, internal); `actors/{provider}` (source, `hashed`, internal) ([0007](0007-actor-identity.md) §8); and, where a farm has identity, `accounts/{farm}`, `actors/{farm}` and `log/{farm}` at the instance ([0028](0028-tenancy-policy.md) §2) | `mutable` |
| Instance configuration | `config` (source, `logged`, public) ([0015](0015-record-format-and-partition-registry.md) §3) | `server` |
| Reconciled view | `resolved` (projection) | none; current state is a policy projection over claims |

The table shows a correspondence, not an identity. Triplespace's `metadata` graph is derived and Scatterbase's `meta` graph is asserted, so they are not interchangeable.

#### 4.2 Log partitions carry their own policy

*Changed by A1, A8, A10, A14, A56.*

The log is split into partitions by source graph ([0002](0002-source-graphs-and-mass-ingest.md), Consequences). Each partition has:

- a **history policy** (`full` or `latest`, [0002](0002-source-graphs-and-mass-ingest.md) §2): keep every record, or compact to the latest record for each key;
- an **integrity policy:** `logged`, a Merkle log over the whole partition with signed checkpoints, or `hashed`, commitments for each segment only. [0006](0006-log-integrity-and-erasure.md) §4 specifies both;
- a **segment size** 2^k and the **hash function**, fixed at creation ([0006](0006-log-integrity-and-erasure.md) §2, §5).

Scatterbase's claim partition keeps everything and is `logged`. Triplespace's mirror partitions compact. `scatter-log` supports both from its first release. Adding either one later would change the on-disk format.

**A partition may be backed by files or by Postgres.** The `segments` backend writes one file per segment. The Postgres backend of [0013](0013-postgres-storage.md) §1–2 stores records as rows, and a segment is then the offset range `[n·2^k, (n+1)·2^k)`. Both implement `LogStore`, and export bundles use the file format whichever backend is live.

**`LogStore` is asynchronous and transaction-shaped.** Its methods (`create_partition`, `partitions`, `head`, `append`, `read`, `scan`, `erase_parts`, `compact`) return `Send` futures, because an interactive write in Postgres appends and applies its projections in one transaction on the async driver ([0013](0013-postgres-storage.md) §7): the Postgres backend implements the trait on a handle that borrows the caller's transaction, so the append composes with everything else in it, and the conformance suite (rule 8) exercises the same append the write path uses. The trait needs no runtime (rule 2). The store fills in only the header fields it alone can know, the partition, the offset and the commitment; the caller's `Draft` carries the time, the payload type, the key and the global IDs, which the write path allocates from its own per-tenant sequences in the same transaction ([0013](0013-postgres-storage.md) §6, [0015](0015-record-format-and-partition-registry.md) §2). A compacted offset keeps its Merkle leaf ([0006](0006-log-integrity-and-erasure.md) A14).

#### 4.3 A record is a header plus a payload

*Changed by A1, A10, A31.*

`scatter-log` stores records as a **header** and a **body**, as [0006](0006-log-integrity-and-erasure.md) §3 specifies them.

- The header holds the format version, partition, offset, the time appended, the payload type, a key, a commitment to the body, and the global revision, log and page IDs ([0015](0015-record-format-and-partition-registry.md) §2). It is what the Merkle tree commits to, and it is never erased.
- The body is a list of salted **parts**, each erasable on its own; the commitment is the root over their leaves ([0015](0015-record-format-and-partition-registry.md) §1).
- In Triplespace the parts are content, comment and attestation, and a payload type may declare more: the thread type adds a fourth, the post text ([0019](0019-discussions.md) §4). The content is a change set, a page or thread operation, an actor record, a log event, an upstream revision, a configuration record, a membership, a block, an ACL ([0023](0023-moderation.md) §3), a filter or a filter hit ([0030](0030-edit-filters.md) §5); the attestation holds the actor, the job ([0002](0002-source-graphs-and-mass-ingest.md) §8.3), the change tags ([0030](0030-edit-filters.md) §5) and, for a post that arrived from the fediverse, the signed remote activity as evidence ([0022](0022-federation.md) §8); and the server is the only signer.
- In Scatterbase the parts are the claim, its attribution, the client and server signatures and, later, prior-use acknowledgments.

Who attests a change is kept apart from what the change is. Everything above `scatter-log` reads payloads and never depends on the attestation format.

#### 4.4 Storage contracts

*Changed by A8, A28.*

`scatter-quadstore` is the storage contract from Scatterbase's plan: `lookup(s, p, o, g)` with wildcards, and since [0032](0032-sparql-update-stream.md) §9 `apply(deleted, inserted)`. Scatterbase's drivers implement it, and it is Scatterbase's serving store. In Triplespace it is a consumer of `view.rdf_delta` ([0013](0013-postgres-storage.md) §8, as amended): an instance that wants a local SPARQL endpoint applies the same deltas the SPARQL Update stream serializes, and nothing in the API reads from it.

Triplespace's serving store is the `view` schema of [0013](0013-postgres-storage.md) §5. It is not a shared seam. Scatterbase has no need of it, and its tables are Wikibase-shaped by design.

`LogStore` in `scatter-log` is the third contract, and the one both products share fully (§4.2).

Extra access paths, such as claim retrieval or chain traversal, are separate traits in the crates that need them. They are not added to `scatter-quadstore` or `LogStore`.

### 5. One vocabulary under `https://scatter.red/terms/v0/` (amends 0001 §5)

[0001](0001-revision-metadata-rdf.md) §5 put Triplespace's terms under `https://scatter.wiki`. Scatterbase already mints its terms under `https://scatter.red/terms/v0/`, with the prefix `scatter:`. Both projects now use that one namespace.

- **Triplespace's terms move to `https://scatter.red/terms/v0/`.** The illustrative `sw:` prefix in 0001 becomes `scatter:`. For example, `sw:Revision` becomes `scatter:Revision`, and `sw:addedIn` becomes `scatter:addedIn`.
- **`scatter-vocab` is the only place terms are defined.** Both products take their term IRIs from it.
- **`v0` means the vocabulary makes no stability promise yet.** Moving to `v1` is the compatibility commitment that 0001's Consequences describe, and it is made for both products at once.
- **The instance-data rule in 0001 §5 is unchanged.** Revision, user and graph IRIs belong to each instance's base URI. Identifiers derived only from content, which are the same on every instance, are not instance data. Scatterbase's `https://scatter.red/claim/` and `https://scatter.red/triple/` IRIs are of this kind, and so are the keyed-type IRIs of [0009](0009-keyed-entity-types-and-domain.md) §6, such as `https://scatter.red/domain/{key}`. They may stay under `scatter.red`.

This settles 0001 Q1, the exact namespace IRI: it is `https://scatter.red/terms/v0/`, the prefix is `scatter:`, and the namespace is versioned.

### 6. Naming, licensing and repository

*Changed by A17, A30.*

- **Names.** Shared crates carry the `scatter-` prefix. Crates that only Triplespace uses carry `triplespace-`. Shared Wikibase crates are named `scatter-wikibase-*`, so no crate name leads with "wikibase", which is Wikimedia Deutschland's project name. `scatter-mwlog` and `scatter-wikitext` model MediaWiki conventions without naming MediaWiki.
- **License** (decided 2026-09-27).
  - **Triplespace is GPL-3.0-or-later.** That covers every crate in this workspace and the `ui/` package.
  - **The shared `scatter-*` crates are dual-licensed.** Like the Scatterbase kernel, they are also available under a commercial license from the copyright holder. Dual licensing requires Scatter to hold copyright in every shared crate, so outside contributions need a contributor license agreement.
  - **Third-party dependencies must be compatible with GPLv3.** [0033](0033-backend-stack.md) §1 lists the accepted licences and §16 says how `cargo-deny` enforces them.
  - **`docs/` is CC0-1.0.** That includes the ADRs, the registry, the API contracts and the test vectors, so independent implementations can embed them. Third-party files in `docs/api/snapshots/` keep their own licences.
  - **Bindings in other languages are Apache-2.0.** Python comes first. They implement the API contract and the specifications in `docs/`. They do not link to Triplespace code, are not compiled against it, and do not port it.
  - **RSA for HTTP Signatures.** `scatter-activitypub` needs RSA because Mastodon accepts only RSA keys there; [0033](0033-backend-stack.md) §6 provides it through `aws-lc-rs`. The instance's Ed25519 key ([0006](0006-log-integrity-and-erasure.md) §6) cannot be used for this, which is one more reason the notifier key of [0021](0021-notifications.md) §5 is separate.
- **Repository.** Every crate lives in one Cargo workspace in the Triplespace repository, under `crates/`. Scatterbase takes Git dependencies pinned to a commit. The `scatter-*` crates move to their own repository when Scatterbase first depends on them.

### 7. Build order

*Changed by A12, A17, A18, A31, A34, A42.*

1. **Pure crates with golden fixtures:** `scatter-vocab`, `scatter-providers`, `scatter-normalize`, `scatter-actors`, `scatter-wikibase-model`, `scatter-wikibase-rdf`, `scatter-wikibase-shape`, `scatter-mwlog`, `scatter-pages`, `scatter-wikitext`. They need no log or storage, are testable now against `docs/api/snapshots` and the UI fixtures, and are what Scatterbase's Phase 4 needs.
2. **The log:** `scatter-log` with the `segments` backend, `scatter-integrity`, then `scatter-log-postgres` and the `LogStore` conformance suite.
3. **Projection plumbing:** `scatter-projection`, `triplespace-db`, and `scatter-quadstore` behind its feature.
4. **Ingest:** `scatter-wikibase-changeset`, `scatter-ingest` and `scatter-adapter-wikidata`, enough to bootstrap a mirror ([0013](0013-postgres-storage.md) §9), and the adoption job of [0035](0035-adopting-a-wikibase.md) §2–4, which a tenant continuing an existing Wikibase needs before step 5.
5. **Resolution:** `scatter-identity`, `scatter-wikibase-resolve`, and `triplespace-projections` through the resolution and term projections.
6. **Reading:** `triplespace-titles`, `triplespace-cache`, `triplespace-api-action` for `wbgetentities`, and `triplespace-server`. This is the first point at which the statement UI ([0003](0003-statement-ui.md) §10) can be pointed at Triplespace instead of Wikidata.
7. **The rest:** the remaining projections and adapters, `triplespace-rdf`, `triplespace-search`, `triplespace-accounts`, `triplespace-upstream`, `triplespace-api-rest`, `triplespace-api-ingest`, `triplespace-cli`; with the write path, `scatter-filter` and its projections ([0030](0030-edit-filters.md)), and after resolution `scatter-wikibase-constraints` and the constraint projection ([0031](0031-property-constraints.md)). Moderation ([0023](0023-moderation.md)), subsidiaries ([0024](0024-subsidiary-accounts.md)), sitelinks ([0026](0026-sitelinks.md)), preferences ([0027](0027-preferences-and-portability.md)), tenancy policy ([0028](0028-tenancy-policy.md)) and resolvers ([0029](0029-resolver-namespaces.md)) add no crate and land in the crates they extend.
8. **Discussion and notification:** `scatter-threads` and the thread projections and routes ([0019](0019-discussions.md)); the feed queries, watchlist and stream ([0020](0020-change-feeds.md)); then `scatter-activitypub` and `triplespace-notify` ([0021](0021-notifications.md)). Each depends on the activity projection of step 7 and on nothing later.
9. **Federation and delegated tools:** `triplespace-federation` ([0022](0022-federation.md)) and `triplespace-oauth` ([0025](0025-oauth-server.md)), which depend on step 8 and on `triplespace-accounts`; the verified Triplespace adapter follows once `scatter-integrity` can verify remote proofs.
10. **Files:** `scatter-blob` and `scatter-files`, then `triplespace-files` with the file projections and routes ([0039](0039-files-and-media.md)). They need step 6 for serving; the blob store itself has no dependency on the log and can be built and tested first.
11. **Expansion and Lua:** `scatter-wikitext-expand`, which is pure and can be built with step 1's crates, then `triplespace-scribunto` and `triplespace-render` with the refresh job and the Parsoid service ([0042](0042-template-expansion-and-parsoid.md), [0043](0043-lua-modules.md)). They need step 6 for reading, and step 7's search and upstream client.

### 8. Changelog of this revision

*Superseded by the Amendment log.* Each row of the changelog this section held is an entry there, among A1–A49.

## Consequences

- **Scatterbase can adopt the substrate in place of its own.** To do so it has to move from its own term types and `rio` to the `oxrdf` family, and replace its write-only `Storage` trait with `scatter-quadstore` for serving and `LogStore` for the log. Both changes are already on its Phase 1–2 path, and it gains a Postgres backend for the log without asking for one.
- **Scatterbase's planned Wikibase extension mostly exists already.** Its namespaces, data types and Wikidata import come from `scatter-providers`, `scatter-wikibase-*` and `scatter-adapter-wikidata`. What remains is binding them to signed claims.
- **Fifty-four crates cost more upkeep than twenty-two.** The dependency check in CI is what keeps the table honest; every ADR that adds a crate has to update §2 in the same change. The alternative, extracting crates from a single crate later, is harder than starting split.
- **Shared crates change for two products.** A breaking change in a `scatter-*` crate, in the `scatter:` vocabulary, or in the `log` schema has to work for both.
- **Triplespace carries features it does not use yet.** Room for client signatures in the attestation slot exists in `scatter-log` before any Triplespace code needs it.
- **The `scatter.wiki` domain is no longer part of the design.** The illustrations in 0001 now read as `scatter:`, and nothing mints terms under `scatter.wiki`.
- **GPLv3 limits who can reuse the crates freely.** A third party that wants the Wikibase crates under other terms needs the commercial license. Clients in other languages avoid this by implementing the CC0 contract in `docs/` rather than binding to the crates, at the cost of reimplementing what they need, such as canonical CBOR and proof checking.
- **Earlier ADRs name crates that have moved.** 0008, 0009, 0010, 0011, 0012 and 0019 keep their original crate names in their text; §2 records where each now lives. *0008, 0009, 0011 and 0012 no longer do: their crate sections are stubs (0008 A1, 0009 A1, 0011 A1, 0012 A1), and 0010 §13 names the new homes. 0019 keeps them until it is converted ([0050](0050-adr-format.md) §13).*
- **Three crates read `private`.** The single-reader rule of 0013 §4 became a two-reader rule with `triplespace-notify` and a three-reader rule with `triplespace-federation` ([0022](0022-federation.md) §13); `triplespace-oauth` deliberately does not join them. The grant-level check and the privacy test of [0012](0012-api-requirements.md) §8 cover all three.

## Open questions

- **Q1. Log granularity** ([0000](0000-init.md)) has been settled in practice and not recorded: mirrors receive whole-entity `put`s ([0002](0002-source-graphs-and-mass-ingest.md) §8.2, [0012](0012-api-requirements.md) §2.2), local edits are statement-level operations, and both are change sets. Whether a local change set should ever carry a whole-entity replacement beyond the explicit base-revision case in 0002 §8.2 is the part that remains. *Narrowed 2026-09-28 by [0035](0035-adopting-a-wikibase.md) §3: `adopt` is the one whole-entity write to the local graph, confined to adoption jobs; whether an ordinary edit may replace an entity against a base revision is still open.*
- **Q2.** ~~**Python bindings.** Scatterbase's decision record plans permissively licensed Python bindings. Bindings compiled against GPLv3 crates cannot be distributed under a permissive license unless the copyright holder grants an exception. Both projects are affected.~~ *Settled by A30: bindings are Apache-2.0 and independent implementations of the contract in `docs/`, never compiled against the crates.*
- **Q3. Publishing to crates.io,** and under which owner.
- **Q4. When to extract the shared repository.** The trigger in §6 is Scatterbase's first dependency. An earlier split may suit contributors.
- **Q5. Whether `scatter.wiki` should redirect** to `scatter.red/terms/v0/` or stay unused.
- **Q6.** ~~**Graph IRIs.** Their form, and whether they are fixed or per instance. Carried over from 0001 and 0002. The registry (§4.1) accepts either, and [0013](0013-postgres-storage.md) §2 stores one per partition.~~ *Settled by [0015](0015-record-format-and-partition-registry.md) §5: `{base}/graph/{name}`, per instance (per tenant since [0018](0018-tenants.md) §2), with the names fixed in `docs/registry/graphs.toml`.*
- **Q7.** ~~**Whether `scatter-log-postgres` should offer a blocking API** so that rule 2's async exception can be withdrawn.~~ *Settled by [0033](0033-backend-stack.md) §4: the synchronous `postgres` crate, a wrapper over the same driver, offers one without a second implementation. Revised by A56: `LogStore` itself is asynchronous, so a blocking caller drives it with an executor rather than through the `postgres` wrapper, which cannot join the write path's transaction.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0001](0001-revision-metadata-rdf.md) §2, §5, §6 | §5 | amends | 0001 A2 |
| [0001](0001-revision-metadata-rdf.md) Q1 | §5 | settles | 0001 Q1 |
| [0008](0008-namespaces-and-document-pages.md) §11 | §2 | supersedes | 0008 A1 |
| [0009](0009-keyed-entity-types-and-domain.md) §12 | §2 | supersedes | 0009 A1 |

## References

- Scatterbase Project Plan, 22 September 2026 (Phases 1–4, divergences #3–#5)
- Scatterbase `docs/claim-envelopes.md` (namespace declarations)
- [Oxigraph and the `oxrdf` crates](https://github.com/oxigraph/oxigraph)
- [`sqlx`](https://github.com/launchbadge/sqlx), [`idna`](https://crates.io/crates/idna)

## Amendment log

### A1. Crates of 0006 §10

- **Date:** 2026-09-25
- **Source:** [0006](0006-log-integrity-and-erasure.md) §10
- **Change:** amends §2, §4.2, §4.3
- **Summary:** `scatter-integrity` added (already in the original table); §4.3 rewritten to match 0006 §3

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A2. Crates of 0007 §8, §10

- **Date:** 2026-09-25
- **Source:** [0007](0007-actor-identity.md) §8, §10
- **Change:** extends §2, §4.1
- **Summary:** `scatter-actors`, `triplespace-accounts` added; `private` export policy and the `accounts` and `actors` graphs added to §4.1

### A3. Crates of 0008 §4, §11

- **Date:** 2026-09-26
- **Source:** [0008](0008-namespaces-and-document-pages.md) §4, §11
- **Change:** amends §2, §4.1
- **Summary:** `scatter-pages`, `scatter-wikitext`, `triplespace-titles` added; `pages` graph added to §4.1; wasm rule extended. `scatter-markdown` folded into `scatter-pages` as a feature

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A4. Crates of 0009 §12

- **Date:** 2026-09-26
- **Source:** [0009](0009-keyed-entity-types-and-domain.md) §12
- **Change:** amends §2
- **Summary:** `scatter-adapter-internetdomains` added; `scatter-wikibase-model` and `scatter-wikibase-changeset` contents extended. `scatter-keyed` folded into `scatter-normalize`, which now builds for wasm and depends on `idna`

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A5. Crates of 0010 §13

- **Date:** 2026-09-26
- **Source:** [0010](0010-site-ui.md) §13
- **Change:** amends §2
- **Summary:** `triplespace-activity` added, then absorbed by 0013

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A6. Crates of 0011 §2, §10

- **Date:** 2026-09-26
- **Source:** [0011](0011-logs.md) §2, §10
- **Change:** extends §2, §4.1
- **Summary:** `scatter-mwlog` added; `scatter-adapter-wikidata` extended; `log` graphs added to §4.1

### A7. Crates of 0012 §9

- **Date:** 2026-09-26
- **Source:** [0012](0012-api-requirements.md) §9
- **Change:** extends §2
- **Summary:** `triplespace-upstream` added; API crates, `scatter-wikibase-changeset` and `scatter-ingest` contents extended

### A8. Crates of 0013 §1, §8, §10

- **Date:** 2026-09-26
- **Source:** [0013](0013-postgres-storage.md) §1, §8, §10
- **Change:** amends §2, §3, §4.2, §4.4
- **Summary:** `LogStore` trait and Postgres backend; `scatter-log-postgres`, `triplespace-db`, `triplespace-projections`, `triplespace-rdf` added; `triplespace-activity` and `triplespace-revmeta` absorbed; §4.4 rewritten; rule 2 amended for the async backend; rule 9 added; §4.2 gains the Postgres backing

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A9. Crates of 0014 §9

- **Date:** 2026-09-26
- **Source:** [0014](0014-caches-and-search.md) §9
- **Change:** extends §2
- **Summary:** `triplespace-cache`, `triplespace-search` added; search-document builder placed in `scatter-wikibase-model`

### A10. Crates of 0015 §7

- **Date:** 2026-09-26
- **Source:** [0015](0015-record-format-and-partition-registry.md) §7
- **Change:** amends §2, §3, §4.1, §4.2, §4.3
- **Summary:** Header fields 7–9, body parts, the `config` partition and `docs/registry/`: `scatter-log`, `scatter-log-postgres`, `scatter-integrity`, `scatter-providers`, `scatter-mwlog`, `scatter-adapter-wikidata`, `triplespace-projections`, `triplespace-rdf` contents extended; `config` graph added to §4.1; §4.3 rewritten; rule 3 names the `config` partition

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A11. Crates of 0016 §9

- **Date:** 2026-09-26
- **Source:** [0016](0016-permissions-and-access-control.md) §9
- **Change:** amends §2, §4.1
- **Summary:** `scatter-actors`, `scatter-log`, `triplespace-projections`, `triplespace-accounts`, the API crates and `triplespace-cli` contents extended; the *writers* field of §4.1 becomes an ACL

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A12. The 2026-09-26 revision

- **Date:** 2026-09-26
- **Source:** Direct: revision by James Hare / Claude Fable, 2026-09-26
- **Change:** amends §1, §2, §7
- **Summary:** `scatter-graphs` folded into `scatter-log`, since the registry configures the partitions that crate owns; four layers named in §1; `scatter-projection` no longer depends on `scatter-quadstore`; "checkpoints" in `scatter-projection` renamed "positions" to avoid the signed checkpoints of 0006; build order rewritten

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A13. Crates of 0017 §7

- **Date:** 2026-09-27
- **Source:** [0017](0017-entity-id-grammar.md) §7
- **Change:** extends §2
- **Summary:** `scatter-adapter-musicbrainz` added; `scatter-wikibase-model`, `scatter-providers`, `scatter-normalize`, `scatter-wikibase-resolve`, `scatter-adapter-openalex` and `triplespace-titles` contents extended

### A14. Crates of 0018 §12

- **Date:** 2026-09-27
- **Source:** [0018](0018-tenants.md) §12
- **Change:** extends §2, §4.1, §4.2
- **Summary:** 64-bit partition IDs, the `tenant` and `alias` config kinds and the key chain in `scatter-log`; `partition` as `bigint` and a `tenant` column in `scatter-log-postgres` and `triplespace-db`; read-time ID rewriting in `scatter-providers`; tenant issuers in `scatter-actors`; the GUID rewrite in `scatter-wikibase-resolve`; overlays in `triplespace-projections` and `triplespace-search`; host-to-tenant resolution, reclaiming and tenant export/import in the surfaces. §4.1 registry keyed by (tenant, name)

### A15. Crates of 0019 §13

- **Date:** 2026-09-27
- **Source:** [0019](0019-discussions.md) §13
- **Change:** amends §2
- **Summary:** `scatter-threads` added, depending on `scatter-pages` only (0019's `scatter-log` dependency dropped); wiki links and the sanitizer added to `scatter-pages`'s `markdown` feature; `triplespace-projections`, `triplespace-titles`, `triplespace-rdf` and the API crates extended; `scatter-markdown` in 0019 §13 reads as `scatter-pages`

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A16. Crates of 0020 §8

- **Date:** 2026-09-27
- **Source:** [0020](0020-change-feeds.md) §8
- **Change:** extends §2
- **Summary:** Feed queries added to `triplespace-projections`, which now also serves reads over `view.activity`; watch set and Atom token in `triplespace-accounts`; watchlist modules, Atom and the stream in the API crates

### A17. Crates of 0021 §10

- **Date:** 2026-09-27
- **Source:** [0021](0021-notifications.md) §10
- **Change:** extends §2, §6, §7
- **Summary:** `scatter-activitypub` and `triplespace-notify` added; `triplespace-notify` joins `triplespace-accounts` as a reader of `private` ([0013](0013-postgres-storage.md) §4, as revised); contact details and the notifier key in `triplespace-accounts`; Echo-shaped modules and ActivityPub endpoints in the API crates; RSA dependency noted in §6; build order gains step 8

### A18. Crates of 0022 §13

- **Date:** 2026-09-27
- **Source:** [0022](0022-federation.md) §13
- **Change:** amends §2, §7
- **Summary:** `triplespace-federation` added, taking ActivityPub delivery and the `ap_*` tables from `triplespace-notify` and becoming the third reader of `private`; remote verification in `scatter-integrity`; actor documents and inbound parsing in `scatter-activitypub`; local-graph reads, back-rewrite and verification in `scatter-adapter-triplespace`, which now depends on `scatter-integrity`; the source dump in `triplespace-rdf`; `/.well-known/tlog/` in the API crates; build order gains step 9

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A19. Crates of 0023 §13

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §13
- **Change:** amends §2
- **Summary:** The `acl` payload type in `scatter-log`; read evaluation, `record` and `actor` targets and the autopatrol rule in `scatter-actors`; `delete`/`undelete` removed from `scatter-pages` and `scatter-threads`; `patrol` and `suppress` types in `scatter-mwlog`; ACL and patrol projections in `triplespace-projections`; reserved page IDs in `triplespace-titles`; the modules in the API crates

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A20. Crates of 0024 §13

- **Date:** 2026-09-27
- **Source:** [0024](0024-subsidiary-accounts.md) §13
- **Change:** extends §2
- **Summary:** Operator, grants, block inheritance and rate-limit policy in `scatter-actors`, which embeds `grants.toml`; `newusers/create2` and `bot/transfer` in `scatter-mwlog`; subsidiaries and keys in `triplespace-accounts`; counters in `triplespace-cache`; login and key routes in the API crates; sync subsidiaries in `triplespace-cli`

### A21. Crates of 0025 §11

- **Date:** 2026-09-27
- **Source:** [0025](0025-oauth-server.md) §11
- **Change:** extends §2
- **Summary:** `triplespace-oauth` added, reaching `private` through `triplespace-accounts`; the `pending` status in `scatter-actors`; `oauth/*` in `scatter-mwlog`; token tables and token authentication in `triplespace-accounts`; OAuth routes and the `oauth-pending` refusal in the API crates

### A22. Crates of 0026 §10

- **Date:** 2026-09-27
- **Source:** [0026](0026-sitelinks.md) §10
- **Change:** extends §2
- **Summary:** URL normalizer in `scatter-normalize`; sitelinks as URLs and `sites.toml` in `scatter-wikibase-model`; RDF, reconciliation and the denied-host filter in the Wikibase crates; site-table resolution in `scatter-adapter-wikidata`; `view.sitelink` in `triplespace-projections`; sitelink modules and routes in the API crates

### A23. Crates of 0027 §9

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §9
- **Change:** extends §2
- **Summary:** Portability classes in `triplespace-db`; preferences, `preferences.toml`, export, import and the private extract in `triplespace-accounts`; `triplespace-notify` reads preferences; `view-pin` in `triplespace-projections`; the extract in `triplespace-cli`

### A24. Crates of 0028 §14

- **Date:** 2026-09-27
- **Source:** [0028](0028-tenancy-policy.md) §14
- **Change:** extends §2
- **Summary:** `scatter-adapter-triplespace` added; tenancy policy, global-group and global-block evaluation, templates and locks in `scatter-actors`, which embeds `tenancy.toml`; the `tenancy`, `template` and `provider-readers` kinds in `scatter-log`; farm accounts and the name registry in `triplespace-accounts`; the all-tenants target set and cross-tenant reads in `triplespace-projections`; farm-wide search in `triplespace-search`; `instance create --tenancy` in `triplespace-cli`

### A25. Crates of 0029 §9 and the 0023 §2 amendment

- **Date:** 2026-09-27
- **Source:** [0029](0029-resolver-namespaces.md) §9; [0023](0023-moderation.md) §2, its amendment of 2026-09-27
- **Change:** extends §2
- **Summary:** Resolver grammars and `resolvers.toml` in `scatter-normalize`; the `resolver` kind and lookup in `triplespace-titles`; resolver routes in the API crates; statement and property ACL targets in `scatter-actors`

### A26. Crates of 0030 §13

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §13
- **Change:** extends §2
- **Summary:** `scatter-filter` added (layer 2, pure, CEL); per-operation filtering in `scatter-ingest`; `view.filter`, `view.filter_hit`, the hits target set and the test job in `triplespace-projections`; the `filter` reason in `triplespace-notify`; refusal errors and evaluation in the write path in the API crates

### A27. Crates of 0031 §9

- **Date:** 2026-09-27
- **Source:** [0031](0031-property-constraints.md) §9
- **Change:** extends §2
- **Summary:** `scatter-wikibase-constraints` added (layer 2, pure); the constraint projection, `view.constraint_violation`, `view.constraint_count`, `view.value_key` and the re-check job in `triplespace-projections`; `wbcheckconstraints` and the report routes in the API crates

### A28. Crates of 0032 §9

- **Date:** 2026-09-27
- **Source:** [0032](0032-sparql-update-stream.md) §9
- **Change:** extends §2, §4.4
- **Summary:** Diff, skolemization and Update serialization in `scatter-wikibase-rdf`; `apply` on `scatter-quadstore` and §4.4 reworded; the delta projection in `triplespace-projections`; stamped dumps and batch files in `triplespace-rdf`; the update routes in the API crates; `sparql-sync` in `triplespace-cli`

### A29. Signing keys and the password issuer

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** extends §2
- **Summary:** The built-in `password` issuer in `scatter-actors` and `triplespace-accounts`; the `key` actor record in `scatter-actors`, signature checking in `scatter-integrity` and on the write path in `triplespace-accounts`, signing in `triplespace-cli`; `scatter-actors` gains `ed25519-dalek`

### A30. Licence

- **Date:** 2026-09-27
- **Source:** Direct: James, licence decision of 2026-09-27
- **Change:** amends §2, §6
- **Summary:** §6 License rewritten: Triplespace is GPL-3.0-or-later, `scatter-*` crates also commercially licensed, `docs/` CC0-1.0 apart from third-party snapshots, bindings in other languages Apache-2.0 and independent of Triplespace code; the per-dependency licence notes moved to [0033](0033-backend-stack.md) §1 and §16; the Python-bindings open question settled

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A31. Third review pass

- **Date:** 2026-09-27
- **Source:** Direct: review, third pass of 2026-09-27
- **Change:** corrects §1, §2, §4.1, §4.3, §7
- **Summary:** Rows for `scatter-ingest`, `triplespace-projections`, `triplespace-accounts`, `triplespace-notify`, `triplespace-cache`, `triplespace-search`, the API crates and `triplespace-cli` brought up to date with 0026–0031, which the earlier passes had logged in this table but not written into §2; `scatter-ingest` depends on `scatter-filter`; the adapter rule under §2 admits the transitive `scatter-integrity` dependency of 0022; §1 names filters, constraints, federation and OAuth; §4.1's destructible row names the farm partitions of 0028; §4.3 names the fourth part, ACL, filter and hit payloads, tags and evidence; build order step 7 places 0030 and 0031

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A32. Backend dependencies

- **Date:** 2026-09-27
- **Source:** [0033](0033-backend-stack.md) §4, §9.1–9.4, §10
- **Change:** amends §2
- **Summary:** `scatter-log-postgres` uses `tokio-postgres`, with a blocking API possible through the synchronous `postgres` crate; `scatter-wikitext` vendors `parse-wiki-text-2`; `scatter-pages` renders markdown with `comrak` and sanitizes with `ammonia`; `scatter-filter` uses `cel`, formerly `cel-interpreter`; the Wikidata adapter reads RevisionChest's `.mwrev.zst` files. 0033 listed these changes, and settled Q7, but §2 never received them until the migration.

Replaced text (§2):

> | | `scatter-log-postgres` | … | `scatter-log`, `sqlx` |
> | | `scatter-filter` | … | …, a CEL crate (§6) |

### A33. Frontend crates

- **Date:** 2026-09-27
- **Source:** [0034](0034-frontend-stack.md) §2, §6
- **Change:** extends §2, §3; amends Consequences
- **Summary:** `triplespace-ui` renders pages on the server with `askama` and Codex markup; `scatter-wasm` binds the pure crates editors need for the browser. 0034 listed both as new crates in 0005, but §2 never received them until the migration.

Replaced text (Consequences):

> - **Fifty-two crates cost more upkeep than twenty-two.**

### A34. Crates of 0035 §8

- **Date:** 2026-09-28
- **Source:** [0035](0035-adopting-a-wikibase.md) §8
- **Change:** extends §2, §7
- **Summary:** The `adopt` operation in `scatter-wikibase-changeset`; the adoption job in `scatter-ingest`; adoption mode in `scatter-adapter-wikidata`; entity-ID sequences and floors in `scatter-log-postgres`; `import/*` for entities in `scatter-mwlog`; adopted-record provenance in `triplespace-rdf`; `instance create --adopt` and `adopt` in `triplespace-cli`; §7 step 4 names the adoption job

### A35. Crates of 0036 §7

- **Date:** 2026-09-28
- **Source:** [0036](0036-openstreetmap-providers.md) §7
- **Change:** extends §2
- **Summary:** `scatter-adapter-openstreetmap` added; `scatter-normalize` (the `osm-tag` grammar) and `triplespace-titles` (the `OSM` namespace) contents extended

### A36. Crates of 0037 §8

- **Date:** 2026-09-28
- **Source:** [0037](0037-gdelt-provider.md) §8
- **Change:** amends §2
- **Summary:** `scatter-adapter-gdelt` added; `scatter-providers` (per-type `id_grammar`, and the `gdelt-record` and `token` grammars, which 0037 §8 first placed in `scatter-normalize`; corrected 2026-09-28) contents extended

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A37. The hash guard

- **Date:** 2026-09-28
- **Source:** Direct: James, the hash guard of 2026-09-28 ([0006](0006-log-integrity-and-erasure.md) §2)
- **Change:** amends §2
- **Summary:** Wikibase's snak, reference and value-node hash emulation named in `scatter-wikibase-model`; the ingest-time recompute-and-compare with kept mismatches in `scatter-ingest` ([0006](0006-log-integrity-and-erasure.md) §2, as amended)

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A38. Crates of 0038 §15

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §15
- **Change:** extends §2
- **Summary:** No new crate. Category extraction in `scatter-wikitext`; page subjects, page-statement IDs and local-host sitelinks by page ID in `scatter-wikibase-model`; page change sets in `scatter-wikibase-changeset`; the main and `Category` namespaces in `triplespace-titles`; category, mapping, status and page-statement projections in `triplespace-projections`; page statements in the main graph in `triplespace-rdf`; `categories` and `statement_keywords` in `triplespace-search`; routes and modules in the API crates

### A39. Crates of 0039 §23

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §23
- **Change:** extends §2
- **Summary:** `scatter-blob` (substrate, async, shared with Scatterbase), `scatter-files` (pure) and `triplespace-files` added; the instance `log` and `files/{repo}` partitions in `scatter-log`; the `blob` target and file rights in `scatter-actors`; blob verification in `scatter-integrity`; `localMedia` in the Wikibase model and RDF crates; file embeds in `scatter-wikitext` and `scatter-pages`; the `upload` context in `scatter-filter`; file log types in `scatter-mwlog`; file tables, titles, upstream, cache, search, API and CLI rows extended

### A40. Crates of 0040 §11

- **Date:** 2026-09-30
- **Source:** [0040](0040-instance-prerogatives.md) §11
- **Change:** extends §2
- **Summary:** No new crate. The instance attestation in `scatter-log`; its verification and the authority extract in `scatter-integrity`; the `instance` issuer, instance rights and binding in `scatter-actors`; log parameters, projections and routes

### A41. Crates of 0041 §12

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §12
- **Change:** extends §2
- **Summary:** No new crate. The content model registry and trait in `scatter-pages`; `triplespace-thread` in `scatter-threads`; the entity models and `mediainfo` in `scatter-wikibase-model`, which now depends on `scatter-pages`; the `pages` kind in `triplespace-titles`; projections and API rows extended

### A42. Crates of 0042 §19

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §19
- **Change:** extends §2, §3, §7
- **Summary:** `scatter-wikitext-expand` (substrate, pure) and `triplespace-render` added; expanded-text rendering and extraction in `scatter-wikitext`; conditional namespaces in `triplespace-titles`; render tables, cache keys, search fields, modules and CLI commands; rule 2 names `scatter-wikitext-expand`; build order gains step 11

### A43. Crates of 0043 §17

- **Date:** 2026-09-30
- **Source:** [0043](0043-lua-modules.md) §17
- **Change:** extends §2, §3
- **Summary:** `triplespace-scribunto` added (pure, GPL-only, links Lua through `mlua`); the `Scribunto` model in `scatter-pages`; Module namespaces in `triplespace-titles`; `view.entity_usage`, `ld:` keys and the Wikibase-client modules; rule 2 names it

### A44. Crates of 0044 §6

- **Date:** 2026-09-30
- **Source:** [0044](0044-tenant-relative-ids.md) §6
- **Change:** extends §2
- **Summary:** No new crate. Tenant-relative IDs in `scatter-wikibase-model` and the `entity-id` normalizer; reserved doubled codes in `scatter-providers`

### A45. Crates of 0045 §13

- **Date:** 2026-09-30
- **Source:** [0045](0045-table-content-model.md) §13
- **Change:** extends §2
- **Summary:** No new crate. The `triplespace-table` model in `scatter-wikibase-shape`, which gains a direct dependency on `scatter-pages`; its registry entry in `scatter-pages`; Table namespaces in `triplespace-titles`; table links in `triplespace-projections`; table modules and routes in the API crates

### A46. Crates of 0046 §10

- **Date:** 2026-09-30
- **Source:** [0046](0046-primary-tenant.md) §10
- **Change:** extends §2
- **Summary:** No new crate. The `primary` config kind and the `{farm base}/instance/` path for instance graph IRIs and origin lines in `scatter-log`; instance rights evaluated against the tenant primary at the record's offset, and `ts-primary`, in `scatter-actors`; the authority attester check in `scatter-integrity`; `primary/*` in `scatter-mwlog`; mirror sync job records and consumer events from the instance `log` in `triplespace-projections`; `--farm-slug` and `primary offer`, `withdraw` and `accept` in `triplespace-cli`

### A47. Crates of 0047 §14

- **Date:** 2026-09-30
- **Source:** [0047](0047-special-pages.md) §14
- **Change:** extends §2
- **Summary:** No new crate. `special-pages.toml` embedded by `triplespace-titles`, which resolves special-page names, aliases, section aliases and scopes; the `report` and `site_stats` projections, the new `page` and `actor` columns and the batch report runner in `triplespace-projections`; `records` exports in `scatter-integrity`; `specialpagealiases`, `list=querypage`, export and the report, Nuke and export routes in the API crates; `verify --records` in `triplespace-cli`

### A48. Crates of 0048 §8

- **Date:** 2026-10-01
- **Source:** [0048](0048-notation.md) §8
- **Change:** amends §2
- **Summary:** No new crate. The `notation` keyed type, the scheme registry with its normalizers and the `osm-tag` and `text` grammars in `scatter-normalize`; `notation` and `wikibase-notation` replace `osm-tag` and `wikibase-osm-tag` in `scatter-wikibase-model`; `skos:notation` triples in `scatter-wikibase-rdf`; `key_scheme` in `scatter-adapter-wikidata`; the `Notation` namespace in `triplespace-titles`; the `notation-scheme` check in the API crates

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A49. Crates of 0049 §15

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §15
- **Change:** extends §2
- **Summary:** No new crate. `attach`, `detach`, the home-and-listings fold and the `triplespace-board` model in `scatter-threads`; its registry entry in `scatter-pages`; namespaces 310 and 311 and `forwards_to` in `triplespace-titles`; `view.thread_attachment`, board `view.talk_page` rows and `thread/*` log events in `triplespace-projections`; `triplespace-rdf`, `triplespace-federation`, `triplespace-search` and the API crates extended

### A50. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §2, §3, §8
- **Summary:** The changelog of §8 became this log, one entry per row, in date order; §8 is a stub. The rows of 0033 and 0034, which those ADRs promised and §2 never received, were folded in. The header's `Revised` line read: "2026-09-26, to incorporate [0006](0006-log-integrity-and-erasure.md) through [0018](0018-tenants.md); 2026-09-27, to add the crates of [0019](0019-discussions.md) through [0021](0021-notifications.md), and later that day for [0022](0022-federation.md) through [0031](0031-property-constraints.md), with a third pass that evening to bring every row up to date with those ten. §8 lists what changed. The dependency table in §2 is the one CI checks (§3, rule 1), so it is kept current here rather than spread across the amending ADRs." The open questions were numbered. The file before conversion is commit `0b26a3a`.

### A51. Page repositories

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §10
- **Change:** extends §2
- **Summary:** No new crate. The title stack, `origin` views and `GET /page/stack` in `triplespace-titles`; inherited and leaving link classes and stack lookups in `triplespace-render`; codeless providers in `scatter-providers`; the `page-repo` kind and the `page-alternates` role in `scatter-pages`.

### A52. `scatter-adapter-mediawiki` and `triplespace-repos`

- **Date:** 2026-10-01
- **Source:** [0053](0053-mirrored-pages.md) §12
- **Change:** amends §2
- **Summary:** Two crates added: the MediaWiki adapter in the ingest layer, which takes the RevisionChest `.mwrev.zst` reader from `scatter-adapter-wikidata` (which now depends on it), and `triplespace-repos` in the surfaces layer for fetching, rewriting, caching, mirroring and indexing repository pages; the `pages/{repo}` graph and `scatter:v0/mirrored-page` in `scatter-log`.

Replaced text (§2, `scatter-adapter-wikidata`):

> reading the `.mwrev.zst` revision files that RevisionChest, a separate binary, makes from Wikidata's history dumps ([0033](0033-backend-stack.md) §10) | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers`, `scatter-actors`, `scatter-mwlog` |

### A53. Forks

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §12
- **Change:** extends §2
- **Summary:** No new crate. `forked_from` in `scatter-pages`; `status` on `create`, `mediaType` and `imported_from` in `scatter-threads`; the `text` part of upstream revision records in `scatter-log`; page revisions from a store or the API and the talk-page splitter in `scatter-adapter-mediawiki`; the fork job in `triplespace-repos`; `Special:Fork` in `triplespace-titles`; `import/interwiki` for forks in `scatter-mwlog`; `view.fork` in `triplespace-projections`.

### A54. `scatter-css`

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §9
- **Change:** extends §2, §3
- **Summary:** `scatter-css` added to the substrate as a pure, wasm-building crate, joining rule 2's list; `scatter-pages` (the `sanitized-css` model), `scatter-wikitext` (the two tags), `scatter-wikitext-expand` (page properties), `triplespace-render` (sheets and site styles), `triplespace-projections` (`view.page_prop`) and `scatter-wasm` (linting) extended.

### A55. The security model

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §16
- **Change:** extends §2
- **Summary:** `scatter-actors` gains the `tenant` and `set` targets, visibility sets, the include subset test and the lock-out check; `triplespace-projections` the visibility tables; `triplespace-cache` the `{vis}` segment and epoch; `triplespace-search` `read_groups` and its filter; `scatter-wikitext-expand` the include rule; `triplespace-server` the deployment settings, host check and landing page; `triplespace-cli` `instance check`.

### A56. `LogStore` is asynchronous

- **Date:** 2026-10-02
- **Source:** Direct: James, decision of 2026-10-02 (`scatter-log`)
- **Change:** amends §3 (rule 2) and §4.2, and Q7
- **Summary:** `LogStore`'s methods return `Send` futures so that the Postgres backend can run an append inside the transaction that also applies projections ([0013](0013-postgres-storage.md) §7), on a handle borrowing that transaction. `scatter-log` still depends on no runtime; its file backend does synchronous I/O inside the futures, and synchronous callers use any executor. ID allocation stays outside the trait: the write path fills revid, logid and page ID into the `Draft` from its own sequences. The `postgres` blocking wrapper of Q7 is no longer how a blocking API is offered, since it cannot share the async driver's transaction.

Replaced text (rule 2): "`scatter-log`'s file backend does synchronous I/O and nothing else. Other runtimes can then call the core without bridging."
