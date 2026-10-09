# 0033. Backend technology stack

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A17)
- **Author:** James Hare / Claude
- **Changes:** [0005](0005-crate-organization.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0019](0019-discussions.md), [0021](0021-notifications.md), [0022](0022-federation.md), [0025](0025-oauth-server.md), [0027](0027-preferences-and-portability.md), [0030](0030-edit-filters.md), [0032](0032-sparql-update-stream.md), [0034](0034-frontend-stack.md)
- **Chapters:** [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [04](../architecture/04-entities-and-identifiers.md), [09](../architecture/09-security-and-moderation.md), [11](../architecture/11-rendering-templates-and-modules.md), [13](../architecture/13-mirrored-pages.md), [14](../architecture/14-discussions.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md)

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

*Changed by A1, A3, A4, A10, A11, A16.*

*Current text: [22](../architecture/22-crates-and-stack.md) §3.3, §4.1, §6.*

### 2. Language and toolchain

*Changed by A12.*

*Current text: [22](../architecture/22-crates-and-stack.md) §4.2.*

### 3. Async runtime and HTTP server

*Current text: [22](../architecture/22-crates-and-stack.md) §4.3.*

### 4. Postgres

*Changed by A9.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §1.4, §5.*

### 5. Cache and search clients

*Changed by A2.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §8, §9.2.*

### 6. Encoding, hashing and cryptography

*Current text: [22](../architecture/22-crates-and-stack.md) §4.4.*

### 7. HTTP client and upstream access

*Current text: [22](../architecture/22-crates-and-stack.md) §4.5.*

### 8. RDF

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §4.1, §6.5.*

### 9. Text and markup

*Current text: [22](../architecture/22-crates-and-stack.md) §4.6.*

#### 9.1 Wikitext, in `scatter-wikitext` (uses 0008 §5)

*Changed by A3, A14, A15.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §1.1, §1.2, §1.5, §8; [22](../architecture/22-crates-and-stack.md) §4.6.*

#### 9.2 Markdown, in `scatter-pages` (uses 0019 §5)

*Current text: [14](../architecture/14-discussions.md) §1.6; [22](../architecture/22-crates-and-stack.md) §4.6.*

#### 9.3 Identifiers, URLs and case

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.10; [22](../architecture/22-crates-and-stack.md) §4.6.*

#### 9.4 Edit filter language (uses 0030 §3)

*Current text: [09](../architecture/09-security-and-moderation.md) §7.3; [22](../architecture/22-crates-and-stack.md) §4.6.*

### 10. Wikidata history dumps: RevisionChest

*Changed by A6, A7.*

*Current text: [13](../architecture/13-mirrored-pages.md) §3.5.*

### 11. QLever

*Changed by A11.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §6.5, §7.2.*

### 12. Configuration and CLI

*Changed by A8, A10.*

*Current text: [23](../architecture/23-configuration-and-registry.md) §3.1, §3.6, §5.1.*

### 13. Observability

*Current text: [22](../architecture/22-crates-and-stack.md) §4.7.*

### 14. Error handling

*Current text: [22](../architecture/22-crates-and-stack.md) §4.8.*

### 15. Testing

*Changed by A3.*

*Current text: [22](../architecture/22-crates-and-stack.md) §4.9.*

### 16. Licensing and supply chain

*Changed by A1, A13.*

*Current text: [22](../architecture/22-crates-and-stack.md) §3.3, §3.4, §7.1.*

### 17. Packaging

*Changed by A10, A16.*

*Current text: [22](../architecture/22-crates-and-stack.md) §4.10.*

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

### A13. The component manifest

- **Date:** 2026-10-07
- **Source:** [0077](0077-special-version.md) §15
- **Change:** extends §16
- **Summary:** `cargo xtask manifest` records the build and every component linked into or sent from the binaries, with their licence texts, for `Special:Version` and its licence subpages; release builds require it.

### A14. File links are not chips

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §9.1
- **Summary:** `File:` and `Image:` links are no longer rendered as chips, and the File namespace is no longer "reserved, not implemented": files in wikitext follow [0039](0039-files-and-media.md) §13's rule. The chip list of §9.1 loses that entry. (PENDING E19)

Replaced text (§9.1):

> Everything else renders as a **visible chip** showing its source: templates, parser functions, magic words and variables, `File:` and `Image:` links (namespace reserved, not implemented, 0008 §2), galleries, and other extension tags.

### A15. Implemented tags and switches render without expansion

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §9.1
- **Summary:** Tags and behaviour switches the registry marks `implemented` are rendered by the built-in renderer whether or not the tenant's expansion is on; §9.1's chip list is pruned to what the registry keeps as `chip`, and behaviour switches are no longer ignored wholesale ([0042](0042-template-expansion-and-parsoid.md) §5). (PENDING E20)

Replaced text (§9.1):

> Everything else renders as a **visible chip** showing its source: templates, parser functions, magic words and variables, `File:` and `Image:` links (namespace reserved, not implemented, 0008 §2), galleries, and other extension tags. Categories are listed as plain text (0010). Behaviour switches (`__NOTOC__` and the like) are ignored.

### A16. Three binaries

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §1, §17
- **Summary:** The server binary is `triplespace-server`, not `triplespace`; `triplespace` is the CLI's binary. Packaging ships three binaries, `triplespace-server`, `triplespace-web` and `triplespace`, and the OCI image carries all three. Principle 1's "one binary" still means that a small instance runs one server process beside Postgres. (PENDING F27)

Replaced text (§1):

> 1. **One binary, one required service.** A small instance runs with the `triplespace` binary and Postgres, and the binary serves the site itself.

Replaced text (§17):

> - The server binary and the web binary for each target, built for Linux (x86-64, arm64, glibc) and macOS for development.
> - An OCI image with both binaries, the frontend assets embedded in each (0034 §8), and nothing else; the web tier runs from the same image with another entry point ([0057](0057-web-tier.md) §2).

### A17. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§17
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [04](../architecture/04-entities-and-identifiers.md), [09](../architecture/09-security-and-moderation.md), [11](../architecture/11-rendering-templates-and-modules.md), [13](../architecture/13-mirrored-pages.md), [14](../architecture/14-discussions.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
