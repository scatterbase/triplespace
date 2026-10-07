# 0033. Backend technology stack

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-06 (A12)
- **Author:** James Hare / Claude
- **Changes:** [0005](0005-crate-organization.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0019](0019-discussions.md), [0021](0021-notifications.md), [0022](0022-federation.md), [0025](0025-oauth-server.md), [0027](0027-preferences-and-portability.md), [0030](0030-edit-filters.md), [0032](0032-sparql-update-stream.md), [0034](0034-frontend-stack.md)

## Context

The earlier ADRs fix what Triplespace does and name a few technologies along the way: Postgres as the log store and serving model (0013), moka and Valkey for caches and OpenSearch for search (0014), CEL for edit filters (0030), Oxigraph only behind `scatter-quadstore` (0013 §8, 0032). They never gather the stack in one place, and most library choices are still implicit.

Four points were fixed before this ADR:

1. The deliverable is a Rust binary.
2. Postgres is the backing store.
3. QLever is a target export destination (0032).
4. RevisionChest (internetarchive/RevisionChest) prepares Wikidata history dumps. It stays a separate binary. Triplespace matures its own upstream sync, which can later be contributed to RevisionChest.

This ADR records the rest. The frontend is in 0034.

## Decision

### 1. Principles

*Changed by A1, A3, A4, A10, A11.*

1. **One binary, one required service.** A small instance runs with the `triplespace` binary and Postgres, and the binary serves the site itself. Valkey, OpenSearch, QLever and a Parsoid service are optional services a larger instance adds (0013 §11 profiles, 0014 §1, [0042](0042-template-expansion-and-parsoid.md) §8.3), and the query service of [0059](0059-query-service.md) §2 runs embedded in the binary by default, moving to QLever only when an instance chooses; Parsoid is a separate PHP program never linked into the binary, and the only one that needs a PHP runtime. A larger instance may also run `triplespace-web`, a second binary from the same workspace that renders the site, holds no state and reaches the instance only through its API, so that page rendering scales apart from the API ([0057](0057-web-tier.md) §2). Nothing needs a message broker, a JVM or a Node runtime.
2. **Pure crates stay pure.** Crates on 0005 rule 2's pure list do no I/O and pull in no async runtime. The ones on rule 7's wasm list must build for `wasm32-unknown-unknown`, so they avoid C dependencies.
3. **GPLv3-compatible licences inside the binary.** Triplespace is GPL-3.0-or-later ([0005](0005-crate-organization.md) §6).
   - **Accepted.** Every third-party crate linked into `triplespace` carries one of these licences: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0, BlueOak-1.0.0 (`minicbor`), CDLA-Permissive-2.0 (root-certificate data in `webpki-roots`), LGPL, GPL-2.0-or-later or GPL-3.0.
   - **Denied.** Licences that cannot be combined with GPLv3: the pre-3.0 OpenSSL/SSLeay licence, GPL-2.0-only and non-commercial terms. AGPL is also denied, as a policy choice, so that its network clause never reaches the combined work.
   - **Enforcement.** `cargo-deny` enforces both lists in CI.
   - **Lua.** Lua 5.1 is linked through `mlua` (MIT) in `triplespace-scribunto`, which vendors Scribunto's and WikibaseClient's GPL-2.0-or-later Lua and is therefore never dual-licensed ([0043](0043-lua-modules.md) §4, §14).
4. **Prefer a small, well-understood dependency to a framework.** Where a protocol surface is small (OAuth server, HTTP Signatures, OpenSearch's REST API), Triplespace implements it over general-purpose crates rather than adopting a framework that would dictate structure.

### 2. Language and toolchain

*Changed by A12.*

- Stable Rust, edition 2024, pinned in `rust-toolchain.toml`. The minimum supported Rust version moves deliberately, in its own commit.
- One Cargo workspace for all 0005 crates.
- CI runs `cargo nextest`, `cargo clippy -D warnings`, `cargo deny check`, `cargo hack --feature-powerset` on crates with features, and a `wasm32-unknown-unknown` build of every rule-7 crate.
- CI also runs the Wikibase client libraries — WikibaseIntegrator, Pywikibot and WikidataIntegrator, at pinned versions — against a server loaded from a sample of the Librarybase dump (`tools/client_compat.sh`, `docs/clients.md`). The compatibility 0012 promises is a measured property, not a reading of the reference contract, and a client's probes (`siteinfo`, `paraminfo`, `meta=wikibase`) are part of the API surface the tests hold fixed.

### 3. Async runtime and HTTP server

| Concern | Choice |
|---|---|
| Runtime | `tokio` (multi-threaded) |
| HTTP server | `axum` on `hyper`, with `tower` middleware |
| Middleware | `tower-http` (compression, request IDs, timeouts, CORS for the documented public routes) |
| Server-sent events | `axum::response::sse` for `/activity/stream` (0020), `/updates/stream` (0032), job progress (0012) |

The write path of 0013 §7 (auth → grants → rate limit → ACLs → filters → base offset → append → projections) is built as tower layers in that order, so the order is visible in one place and testable on its own. Redaction for the viewer (0012 §1) is a response layer shared by every route.

### 4. Postgres

*Changed by A9.*

- **Version:** 17 is the minimum; 18 is the target. The schema uses features from 15 and later (`UNIQUE NULLS NOT DISTINCT`), 14 (lz4 TOAST) and none from extensions. **No Postgres extension is required**, so the small profile runs on any managed Postgres.
- **Driver:** `tokio-postgres` with `deadpool-postgres` for pooling. It gives binary `COPY` for bootstrap (0013 §9), pipelining, and exact control of the append transaction and its row lock (0013 §2). The synchronous `postgres` crate, a wrapper over the same driver, answered [0005](0005-crate-organization.md) Q7 on a blocking API for `scatter-log-postgres`; since 0005 A56 `LogStore` is itself asynchronous, because the append shares the write path's transaction (0013 §7), and a blocking caller drives it with an executor instead.
- **Queries:** hand-written SQL in each owning crate. No ORM. Query shapes are checked by integration tests against a real database (§15), not by compile-time macros, because several crates share one schema and migrations run in a fixed cross-crate order.
- **Migrations:** each owning crate embeds its SQL files (0005 rule 9). A small runner in `triplespace-db` applies them in 0005's build order, each in its own transaction, and records each with a checksum in `ops.migration`; a recorded migration whose SQL has changed is refused, since a schema change is a new migration, never an edit (0005 rule 9). `refinery` is an acceptable substitute if the in-house runner stops being small.
- **Queues and background work:** `ops` tables claimed with `FOR UPDATE SKIP LOCKED`, woken with `LISTEN/NOTIFY`. This covers the ActivityPub delivery queue (0021), exports (0027), filter tests (0030), constraint re-checks (0031) and jobs (0012). No broker.

### 5. Cache and search clients

*Changed by A2.*

| Service | Client | Notes |
|---|---|---|
| In-process cache (L0) | `moka` | As 0014. |
| Valkey (L1) | `redis` (redis-rs) with its tokio connection manager | Works against Valkey unchanged. Prefix deletion for erasure (0014 §5) uses `SCAN` + `UNLINK`. |
| OpenSearch | `reqwest` with typed request and response structs | The surface used (index, bulk, msearch, aliases, `version_type: external`) is small; a typed client module in `triplespace-search` is easier to keep current than the official client crate. |
| Blob storage | `object_store` (Apache Arrow), local-filesystem and S3 backends, in `scatter-blob` | [0039](0039-files-and-media.md) §3. To be confirmed against the licence allowlist of §1 like every dependency |
| Thumbnails | `image` for raster formats, `resvg` for SVG, in `scatter-files` | [0039](0039-files-and-media.md) §6 |

### 6. Encoding, hashing and cryptography

| Concern | Choice | Notes |
|---|---|---|
| Canonical CBOR (0006 §2, 0015 §1) | `minicbor` | Encoding is written by hand to the canonical rules, with no derive magic in the hashed path. Round-trip tests over Wikidata snapshots (decision #5 of 2026-09-27). `no_std`-capable and wasm-friendly. |
| Hashing | `sha2` | Merkle leaves and nodes (0006). |
| Instance and client signatures | `ed25519-dalek` | 0006 key chain; subsidiary signing keys (0024 §4). |
| HMAC | `hmac` | Binding digests (0018 §10), Atom tokens. |
| RSA for ActivityPub HTTP Signatures (0021 §5, 0022) | `aws-lc-rs` | Constant-time RSA signing. Not the `rsa` crate. |
| TLS | `rustls` with the `aws-lc-rs` provider | One crypto provider for TLS and RSA signing. `ring` is the fallback if musl builds (§17) become a requirement. |
| Password issuer (0007 §1, decision #1) | `argon2` | Argon2id, parameters in site config. |
| Sealing private extracts ([0027](0027-preferences-and-portability.md) Q1) | `age` (X25519 recipients) | Proposed here; 0027 Q1 stays open until James agrees. Never the Ed25519 instance key. |
| Randomness | `getrandom`, `rand_core` | |
| Token and ID encoding | `base64` (URL-safe, no padding), `ulid` only where 0015 names ULIDs | |

### 7. HTTP client and upstream access

- `reqwest` with rustls for the Wikidata Action API, WebFinger, ActivityPub delivery and cross-instance reads (0022).
- `eventsource-stream` over `reqwest` for Wikimedia EventStreams (`recentchange`, `revision-visibility-change`; 0011 §4).
- Every upstream request carries a policy-compliant `User-Agent` and `maxlag`. The upstream fetch budget (0010, 0012 §9) is enforced with `governor` token buckets per provider.

### 8. RDF

- `oxrdf` and `oxttl`, the Oxigraph project's standalone model and serializer crates, in `scatter-wikibase-rdf` and `triplespace-rdf`. They serialize N-Quads, N-Triples and Turtle for dumps and for the SPARQL Update stream (0032) without a store.
- The `oxigraph` store is used only behind `scatter-quadstore` (Scatterbase, and the optional local quad store that consumes the update stream, 0032 §8).

### 9. Text and markup

#### 9.1 Wikitext, in `scatter-wikitext` (uses 0008 §5)

*Changed by A3.*

**Parser.** Vendor `parse-wiki-text-2` (a maintained fork of Fredrik Portström's `parse_wiki_text`, MIT-style licence without a notice clause) into `scatter-wikitext`. It is pure Rust, gives every node start and end positions, recognises templates, parameters, tags, tables, lists, links, images, categories and comments, reports warnings for malformed markup, and bounds its own running time. Its `Configuration` (namespaces, extension tags, URL protocols) is generated from `namespaces.toml`.

Vendoring, rather than a crates.io dependency, is because the fork's last release is over a year old and the crate becomes part of Triplespace's content model. Changes are offered upstream where they are general.

**What the parser must do.** Parse any wikitext without failing, because imported history predates the template-flattening revision (0008 §8); recognise what it will not render; keep positions for line-level conflict marking and link autocomplete (0010); extract `page_link` rows and categories; build for wasm so the preview (0034) runs the same code as the server.

**Renderer.** Written in `scatter-wikitext`. It shares with the markdown path (`scatter-pages`, `markdown` feature) the `ammonia` sanitizer, wiki-link resolution (entity links rendered with labels, red links) and `page_link` extraction, so both content models render and link-index the same way.

**Rendered subset:**

| Kind | Rendered |
|---|---|
| Blocks | headings, paragraphs, lists (`*`, `#`, `;`, `:`), `----`, tables, leading-space `<pre>` |
| Inline | bold and italic (MediaWiki's apostrophe algorithm), internal links through `namespaces.toml`, external links, bare URLs, `<nowiki>`, comments (dropped) |
| HTML | MediaWiki Sanitizer's allowlisted tags and attributes |
| References | `<ref>`, `<ref name>`, `<references />` |
| Pre-save transform | `~~~`, `~~~~`, `~~~~~` on wikitext pages (not posts, 0019 §5) |

Everything else renders as a **visible chip** showing its source: templates, parser functions, magic words and variables, `File:` and `Image:` links (namespace reserved, not implemented, 0008 §2), galleries, and other extension tags. Categories are listed as plain text (0010). Behaviour switches (`__NOTOC__` and the like) are ignored. Magic links (`ISBN`, `RFC`, `PMID`) are off.

**Conformance.** Two test sources:

- A subset of MediaWiki's `tests/parser/parserTests.txt` covering only the rendered subset, run in CI as a fixture. The file is GPL-2.0-or-later, which is compatible with Triplespace's licence; it is test data and is not compiled into the binary.
- A differential test against Parsoid on the MediaWiki 1.43 reference install (0000): sampled pages rendered by both, DOMs normalized and compared for the supported constructs.

**Expanded text.** With a tenant's expansion on, the parser renders and extracts from **expanded** text, produced by `scatter-wikitext-expand`; a tenant may instead render through a Parsoid service that calls back into Triplespace for expansion ([0042](0042-template-expansion-and-parsoid.md) §1, §8). Parsoid stays rejected as an in-process parser.

**Not chosen as the core parser:** `tree-sitter-wikitext` (Wikimedia, MIT). It produces a concrete syntax tree that would need lowering, and its C core complicates `wasm32-unknown-unknown` builds. It remains available for editor highlighting in the browser (0034 §7). `wikitext-parser` (approximate, unmaintained) and Parsoid or mwparserfromhell (not in-process) were also rejected.

#### 9.2 Markdown, in `scatter-pages` (uses 0019 §5)

`comrak`, with its wikilinks extension for `[[wiki links]]`, and `ammonia` for sanitization. Both build for wasm.

#### 9.3 Identifiers, URLs and case

| Concern | Choice |
|---|---|
| URLs (0026 §1) | `url` (WHATWG parsing) |
| IDNA / UTS 46 (0009) | `idna` |
| Case folding (DOI resolver, 0029) and plural rules | `icu_casemap`, `icu_plurals` (ICU4X; wasm-friendly) |
| Unicode normalization | `unicode-normalization` |

#### 9.4 Edit filter language (uses 0030 §3)

The `cel` crate. This is the crate formerly published as `cel-interpreter`; 0030 §7 and 0005 are amended to the new name.

### 10. Wikidata history dumps: RevisionChest

*Changed by A6, A7.*

**RevisionChest runs as a separate binary** to turn Wikidata's XML history dumps into its `.mwrev.zst` revision files with an index (SQLite, Postgres or Parquet). It has no library target, so Triplespace does not link it. Its licence (GPL-3.0) is compatible with Triplespace's; keeping it a separate process is an architectural choice (fixed point 4), not a licensing one. `scatter-adapter-mediawiki` contains an **independent reader** for the `.mwrev.zst` format and its index, which the Wikidata adapter uses ([0053](0053-mirrored-pages.md) §12; it was first placed in the Wikidata adapter, A6). Reading a file format does not make the reader a derivative work. RevisionChest reads any MediaWiki wiki's dumps, and a store of a Wikipedia's history is what seeds a fork's revisions ([0054](0054-forking-a-mirrored-page.md) §3).

A RevisionChest store is accepted wherever 0015 §4 accepts `dump:{path}` as a backfill source.

**Live upstream sync stays in Triplespace** (0011 §4): EventStreams, the logging dump and `list=logevents`. It is built so that it can later be contributed to RevisionChest. The reader and sync code must handle what RevisionChest's current `sync` does not, observed in its source on 2026-09-27:

- a revision whose content is hidden upstream is written as empty text, which would read as an empty entity; Triplespace treats hidden content as hidden (0011 §5);
- log events (deletion, suppression, revision visibility) are not fetched;
- no `maxlag`, and no resume beyond the 30-day recent-changes window;
- the sync path's index `offset_begin` appears to add a compressed file offset to an uncompressed header length; the reader locates revisions by zstd frame start, not by that offset, until this is confirmed.

Contributing upstream will need a library split in RevisionChest. The licences are compatible in both directions. Code moving from RevisionChest into the Wikidata adapter still needs the Internet Archive's agreement, because `scatter-*` crates are dual-licensed and Scatter must hold copyright in them ([0005](0005-crate-organization.md) §6). Code Triplespace contributes to RevisionChest needs no agreement.

If a RevisionChest store is used as the local source for upstream history (0010 §8's "fetch upstream history", or `fork.history_source = chest:` for pages, [0054](0054-forking-a-mirrored-page.md) §3), it must receive the same hiding sweep as the log (0011 §5), read for a page repository from its `revision-visibility-change` events and deletion log.

### 11. QLever

*Changed by A11.*

QLever is an export destination, and optionally the remote backend of the query service ([0059](0059-query-service.md) §2). It is never a required dependency: the service's default backend is Oxigraph embedded in the binary. It reached full SPARQL 1.1 compliance, including Update and the Graph Store Protocol, in June 2025, and its own tooling keeps a Wikidata index current from a change stream (`qlever update-wikidata`). 0032's stream matches that model.

- `triplespace-cli sparql-sync` (0032 §6) is the reference consumer and is tested against QLever in CI (§15).
- A `qlever update-triplespace` command in qlever-control, pointed at `/updates/stream`, is to be offered upstream.
- The `resolved` subscription is the default for a QLever that serves outside consumers only. A QLever that is the query service's remote backend loads `full`, with its named graphs, since the service isolates tenants by dataset ([0059](0059-query-service.md) §4); 0059's test plan carries the `full` benchmark this line once deferred.

### 12. Configuration and CLI

*Changed by A8, A10.*

- `clap` for `triplespace-cli` and the server's flags.
- `serde` + `toml` for `docs/registry/` files, embedded at build time by the crates that need them.
- `figment` for layered instance configuration: file, then environment. Secrets are read from files (`--token-file`, `--*-file`), never from command-line values.
- `server.mode` (`production` or `development`), `server.trusted_proxies`, `server.admin_listen` and the registered-host check, and `triplespace-cli instance check` with `--attest` and `--through`, which verify the deployment requirements of [0056](0056-security-model.md) §10; in `production` the server refuses to start while a requirement it can test fails.
- `server.ui` (`embedded` or `off`) chooses whether the server serves the site; `triplespace-web` takes the `web.*` settings of [0057](0057-web-tier.md) §2 by the same rules, its forwarder key and its cache's Valkey password from files. `triplespace-cli instance forwarder create`, `list` and `revoke` manage forwarder keys ([0057](0057-web-tier.md) §10).

### 13. Observability

- `tracing` and `tracing-subscriber` for structured logs.
- `opentelemetry` with the OTLP exporter for traces.
- `metrics` with the Prometheus exporter. Required metrics: projection lag per projection, append latency, sync lag per provider, upstream fetch budget used, delivery queue depth, cache hit rates per layer, update-stream consumer lag.

### 14. Error handling

`thiserror` in library crates, `anyhow` only in `triplespace-cli` and test code. API errors map to MediaWiki-shaped error codes (0012) in one place in the API crates.

### 15. Testing

*Changed by A3.*

| Tool | Use |
|---|---|
| `cargo-nextest` | test runner |
| `proptest` | canonical CBOR, Merkle proofs, normalizers, filter evaluation |
| `insta` | snapshots of JSON, RDF and rendered HTML |
| `testcontainers` | Postgres, Valkey, OpenSearch and QLever in integration tests |
| `cargo-fuzz` | wikitext and markdown parsers, CBOR decoding, HTTP Signature parsing, CEL compilation, inbound ActivityPub bodies |
| MediaWiki 1.43 reference install, with WikibaseClient, ParserFunctions and Scribunto | API compatibility (0012), Parsoid differential tests (§9.1), differential `action=expandtemplates` tests and Lua conformance ([0042](0042-template-expansion-and-parsoid.md) §18, [0043](0043-lua-modules.md) §15), Pywikibot acceptance (0008 §12) |

The `LogStore` conformance suite (0005 rule 8) runs against both the file and Postgres implementations.

### 16. Licensing and supply chain

*Changed by A1.*

- `cargo-deny` checks licences (allowlist in §1), bans (no duplicate crypto providers, no `openssl-sys`), advisories and sources.
- `parse-wiki-text-2`'s non-standard licence text is recorded as a clarify entry.
- Workspace crates and `ui/package.json` declare `GPL-3.0-or-later`, set once in `[workspace.package]`.
- `aws-lc-sys` is required at 0.39 or later. Earlier versions included code under the OpenSSL licence, which is incompatible with GPLv3. For the same reason, if `ring` replaces it (§6), `ring` must be 0.17.9 or later.
- The repository root carries the GPLv3 text in `LICENSE`; `docs/LICENSE` carries CC0-1.0 ([0005](0005-crate-organization.md) §6).

### 17. Packaging

*Changed by A10.*

- The server binary and the web binary for each target, built for Linux (x86-64, arm64, glibc) and macOS for development.
- An OCI image with both binaries, the frontend assets embedded in each (0034 §8), and nothing else; the web tier runs from the same image with another entry point ([0057](0057-web-tier.md) §2).
- A `compose.yaml` for development with Postgres, and optional Valkey, OpenSearch and QLever services.
- musl static builds are not a goal; if they become one, the crypto provider moves from `aws-lc-rs` to `ring` (§6).

## Consequences

- A small instance needs Postgres and nothing else, on any managed Postgres.
- Crypto has one provider across TLS and signatures, so audits and FIPS questions have one answer.
- `scatter-wikitext` owns a vendored parser and a renderer, which is real code to maintain; in return the preview and the server agree by construction.
- Triplespace's own sync code has to reach RevisionChest's quality for dumps before it can be contributed back.
- The binary, the OCI image, and the JavaScript and wasm served to browsers are all object code conveyed under GPLv3 §6. Each must come with its corresponding source, or point to where it can be obtained.
- GPLv3, not AGPL: an operator who modifies Triplespace and only runs it as a service is not required to publish the changes.

## Open questions

- **Q1.** ~~Triplespace's own licence. It decides whether RevisionChest code can ever flow in, and the `cargo-deny` allowlist.~~ *Settled by A1: GPL-3.0-or-later; `docs/` CC0-1.0; bindings in other languages Apache-2.0 ([0005](0005-crate-organization.md) §6).*
- **Q2.** ~~`minicbor` versus `ciborium` is confirmed once 0006's test vectors exist.~~ *Settled by the `scatter-log` vectors, 2026-10-02: `minicbor` stays, for strict decoding of primitives; the canonical encoder is `scatter-log`'s own. The vectors are `docs/api/vectors/log-v1.json`, derived by an independent Python implementation and checked by the crate.*
- **Q3.** Whether `parse-wiki-text-2` changes are upstreamed or the vendored copy diverges for good.
- **Q4.** MSRV policy: how far behind stable.
- **Q5.** `aws-lc-rs` versus `ring` if musl or FIPS requirements appear.
- **Q6.** Whether `refinery` replaces the in-house migration runner.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §4, §9.1–9.4, §10 | amends | 0005 A32 |
| [0005](0005-crate-organization.md) Q7 | §4 | settles | 0005 Q7 |

## Amendment log

### A1. The licence

- **Date:** 2026-09-27
- **Source:** Direct: James, licence decision of 2026-09-27
- **Change:** amends §1, §16
- **Summary:** Triplespace is GPL-3.0-or-later, `docs/` CC0-1.0, and bindings in other languages Apache-2.0 and independent of Triplespace code (recorded in full as 0005 A30). Principle 3 and §16 were written to it in place. This settled Q1.

Replaced text: not recorded; the sections were revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A2. Blob storage and thumbnails

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §3, §6
- **Change:** extends §5
- **Summary:** Blob storage uses `object_store` (Apache Arrow), with its local-filesystem and S3 backends, in the new `scatter-blob` crate; thumbnails use `image` for raster formats and `resvg` for SVG, in `scatter-files`. Each is to be confirmed against §16 by `cargo deny` when the crates are added.

### A3. Expansion and the Parsoid service

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §1, §8, §18
- **Change:** amends §1, §9.1; extends §15
- **Summary:** By section:
  - §1: The Parsoid service, a separate PHP program never linked into the binary, joins the optional services; it is the only one that needs a PHP runtime.
  - §9.1: With a tenant's expansion on, the parser renders and extracts from **expanded** text, produced by `scatter-wikitext-expand`; a tenant may instead render through a Parsoid service that calls back into Triplespace for expansion. Parsoid stays rejected as an in-process parser.
  - §15: The reference install gains ParserFunctions and Scribunto, beside WikibaseClient, for differential `action=expandtemplates` tests and Lua conformance ([0043](0043-lua-modules.md) §15).

Replaced text (§1):

> 1. **One binary, one required service.** A small instance runs with the `triplespace` binary and Postgres. Valkey, OpenSearch and QLever are optional services a larger instance adds (0013 §11 profiles, 0014 §1). Nothing needs a message broker, a JVM or a Node runtime.

### A4. Lua

- **Date:** 2026-09-30
- **Source:** [0043](0043-lua-modules.md) §4, §14
- **Change:** extends §1
- **Summary:** Lua 5.1 is linked through `mlua` (MIT) in `triplespace-scribunto`, which vendors Scribunto's and WikibaseClient's GPL-2.0-or-later Lua and is therefore never dual-licensed.

### A5. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–17
- **Summary:** A1–A4 were folded into the Decision. The title's em dash became a full stop, the open questions were numbered, the Author line was added, and the list this ADR kept under "Changes to other ADRs" (quoted below) was replaced by the generated table; the placeholder "0027 §…" in §6 now cites 0027 Q1. No decision changed. Before this, A2–A4 were blockquotes. 0005 A32 records the dependency notes this ADR gave 0005 §2. The file before conversion is commit `0b26a3a`.

Replaced text (Changes to other ADRs):

> - **0005:** dependency notes on `scatter-wikitext` (vendored parser), `scatter-pages` (`comrak`, `ammonia`), `scatter-filter` (`cel`), `scatter-log-postgres` (blocking API via `postgres`), the Wikidata adapter (`.mwrev.zst` reader); changelog row.
> - **0008:** §… points at §9.1 for the parser, subset and conformance tests.
> - **0030:** §7 crate name `cel-interpreter` → `cel`.
> - **0005 open question** on a blocking Postgres API: settled by §4.
> - **0005 §6:** licence paragraph rewritten with the licence decision of 2026-09-27, and the Python-bindings open question settled.
> - **0027 open question** on the extract sealing scheme: proposed by §6, pending confirmation.

### A6. The RevisionChest reader moves to `scatter-adapter-mediawiki`

- **Date:** 2026-10-01
- **Source:** [0053](0053-mirrored-pages.md) §12
- **Change:** amends §10
- **Summary:** The `.mwrev.zst` reader lives in the new MediaWiki adapter, on which the Wikidata adapter now depends, so that Wikipedia page revisions and Wikidata entity revisions share it.

Replaced text (§10):

> The Wikidata adapter contains an **independent reader** for the `.mwrev.zst` format and its index. Reading a file format does not make the reader a derivative work.

### A7. RevisionChest stores for page history

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §3
- **Change:** amends §10
- **Summary:** A RevisionChest store of a Wikipedia's history is the source a fork's revisions are seeded from, and receives the hiding sweep read from the repository's events.

Replaced text (§10):

> If a RevisionChest store is used as the local source for upstream history (0010 §8's "fetch upstream history"), it must receive the same hiding sweep as the log (0011 §5).

### A8. Deployment checks

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §10
- **Change:** extends §12
- **Summary:** The server gains `server.mode`, `server.trusted_proxies`, `server.admin_listen` and a 421 on an unregistered host; the CLI gains `instance check`, which tests or records attestation for each requirement of the deployment boundary.

### A9. Checksummed migrations

- **Date:** 2026-10-02
- **Source:** Direct: James, review of 2026-10-02 (`triplespace-db`)
- **Change:** amends §4
- **Summary:** The runner records each migration with a checksum and refuses one whose SQL has changed since it was applied; each runs in its own transaction with its bookkeeping row.

Replaced text (§4): "A small runner in `triplespace-db` applies them in 0005's build order and records them in `ops.migration`."

### A10. The web tier

- **Date:** 2026-10-03
- **Source:** [0057](0057-web-tier.md) §2
- **Change:** amends §1, §17; extends §12
- **Summary:** The server binary serves the site itself, so a small instance is still one binary and Postgres; a larger one may add `triplespace-web`, a stateless second binary that renders the site through the API. `server.ui` chooses whether the server serves the site, and the web tier's settings, its forwarder key among them, follow §12's rules. The image carries both binaries.

Replaced text (§1):

> 1. **One binary, one required service.** A small instance runs with the `triplespace` binary and Postgres. Valkey, OpenSearch, QLever and a Parsoid service are optional services a larger instance adds (0013 §11 profiles, 0014 §1, [0042](0042-template-expansion-and-parsoid.md) §8.3); Parsoid is a separate PHP program never linked into the binary, and the only one that needs a PHP runtime. Nothing needs a message broker, a JVM or a Node runtime.

Replaced text (§17):

> - One binary per target, built for Linux (x86-64, arm64, glibc) and macOS for development.
> - An OCI image with the binary, the embedded frontend assets (0034 §8) and nothing else.

### A11. The query service

- **Date:** 2026-10-04
- **Source:** [0059](0059-query-service.md) §2
- **Change:** amends §1, §11
- **Summary:** The query service runs embedded (Oxigraph, in the binary) by default and may use QLever as a remote backend, so the one-binary rule of §1 holds with the service on, and QLever becomes an optional runtime service as well as an export destination. A QLever that backs the service loads the `full` form.

Replaced text (§1, item 1, in part):

> Valkey, OpenSearch, QLever and a Parsoid service are optional services a larger instance adds (0013 §11 profiles, 0014 §1, [0042](0042-template-expansion-and-parsoid.md) §8.3); Parsoid is a separate PHP program

Replaced text (§11):

> QLever is an export destination, not a runtime dependency. It reached full SPARQL 1.1 compliance

> - The `resolved` subscription is the default for QLever; `full`, with its named graphs, is benchmarked before being recommended.

### A12. Client compatibility in CI

- **Date:** 2026-10-06
- **Source:** Direct: James, Librarybase acceptance run of 2026-10-06 (test plan stage 1)
- **Change:** extends §2
- **Summary:** CI runs WikibaseIntegrator, Pywikibot and WikidataIntegrator, at pinned versions, against a server adopted from a Librarybase sample fixture; any failing check fails the build. Running the libraries found server requirements the reference contract does not state (Pywikibot's `siteinfo` and `paraminfo` structure, `meta=wikibase`), which are now part of the surface the job holds fixed.
