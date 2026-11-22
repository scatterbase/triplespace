# 0033. Backend technology stack

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-11-22 (A34)
- **Author:** James Hare / Claude
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

*Current text: [22](../architecture/22-crates-and-stack.md) §3.3, §4.1, §6.*

### 2. Language and toolchain

*Current text: [22](../architecture/22-crates-and-stack.md) §4.2.*

### 3. Async runtime and HTTP server

*Current text: [22](../architecture/22-crates-and-stack.md) §4.3.*

### 4. Postgres

*Current text: [03](../architecture/03-storage-caches-and-search.md) §1.4, §5.*

### 5. Cache and search clients

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

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §1.1, §1.2, §1.5, §8; [22](../architecture/22-crates-and-stack.md) §4.6.*

#### 9.2 Markdown, in `scatter-pages` (uses 0019 §5)

*Current text: [14](../architecture/14-discussions.md) §1.6; [22](../architecture/22-crates-and-stack.md) §4.6.*

#### 9.3 Identifiers, URLs and case

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.10; [22](../architecture/22-crates-and-stack.md) §4.6.*

#### 9.4 Edit filter language (uses 0030 §3)

*Current text: [09](../architecture/09-security-and-moderation.md) §7.3; [22](../architecture/22-crates-and-stack.md) §4.6.*

### 10. Wikidata history dumps: RevisionChest

*Current text: [13](../architecture/13-mirrored-pages.md) §3.5.*

### 11. QLever

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §6.5, §7.2.*

### 12. Configuration and CLI

*Current text: [23](../architecture/23-configuration-and-registry.md) §3.1, §3.6, §5.1.*

### 13. Observability

*Current text: [22](../architecture/22-crates-and-stack.md) §4.7.*

### 14. Error handling

*Current text: [22](../architecture/22-crates-and-stack.md) §4.8.*

### 15. Testing

*Current text: [22](../architecture/22-crates-and-stack.md) §4.9.*

### 16. Licensing and supply chain

*Current text: [22](../architecture/22-crates-and-stack.md) §3.3, §3.4, §7.1.*

### 17. Packaging

*Current text: [22](../architecture/22-crates-and-stack.md) §4.10.*

## Consequences

- A small instance needs Postgres and nothing else, on any managed Postgres.
- Crypto has one provider across TLS and signatures, so audits and FIPS questions have one answer.
- `scatter-wikitext` owns a vendored parser and a renderer, which is real code to maintain; in return the preview and the server agree by construction.
- ~~Triplespace's own sync code has to reach RevisionChest's quality for dumps before it can be contributed back.~~ *Triplespace depends on RevisionChest's crates, and its poller moved to RevisionChest as `mwrecent` (A33).*
- The binary, the OCI image, and the JavaScript and wasm served to browsers are all object code conveyed under GPLv3 §6. Each must come with its corresponding source, or point to where it can be obtained.
- GPLv3, not AGPL: an operator who modifies Triplespace and only runs it as a service is not required to publish the changes.

## Open questions

- **Q1. Triplespace's own licence.** *Settled by A1.*
- **Q2. `minicbor` versus `ciborium`.** *Settled by the `scatter-log` vectors, 2026-10-02: `minicbor` stays, for strict decoding of primitives; the canonical encoder is `scatter-log`'s own. The vectors are `docs/api/vectors/log-v1.json`, derived by an independent Python implementation and checked by the crate.*
- **Q3.** Whether `parse-wiki-text-2` changes are upstreamed or the vendored copy diverges for good.
- **Q4.** MSRV policy: how far behind stable.
- **Q5.** `aws-lc-rs` versus `ring` if musl or FIPS requirements appear.
- **Q6.** Whether `refinery` replaces the in-house migration runner.

## Amendment log

### A1. The licence

- **Date:** 2026-09-27
- **Source:** Direct: James, licence decision of 2026-09-27
- **Change:** amends §1, §16
- **Summary:** Triplespace is GPL-3.0-or-later, `docs/` CC0-1.0, and bindings in other languages Apache-2.0 and independent of Triplespace code (recorded in full as 0005 A30). Principle 3 and §16 were written to it in place. This settled Q1.

Replaced text: not recorded; the sections were revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A2. Blob storage and thumbnails

- **Date:** 2026-09-30
- **Source:** 0039 §3, §6
- **Change:** extends §5
- **Summary:** Blob storage uses `object_store` (Apache Arrow), with its local-filesystem and S3 backends, in the new `scatter-blob` crate; thumbnails use `image` for raster formats and `resvg` for SVG, in `scatter-files`. Each is to be confirmed against §16 by `cargo deny` when the crates are added.

### A3. Expansion and the Parsoid service

- **Date:** 2026-09-30
- **Source:** 0042 §1, §8, §18
- **Change:** amends §1, §9.1; extends §15
- **Summary:** By section:
  - §1: The Parsoid service, a separate PHP program never linked into the binary, joins the optional services; it is the only one that needs a PHP runtime.
  - §9.1: With a tenant's expansion on, the parser renders and extracts from **expanded** text, produced by `scatter-wikitext-expand`; a tenant may instead render through a Parsoid service that calls back into Triplespace for expansion. Parsoid stays rejected as an in-process parser.
  - §15: The reference install gains ParserFunctions and Scribunto, beside WikibaseClient, for differential `action=expandtemplates` tests and Lua conformance (0043 §15).

Replaced text: §1 of this ADR.

### A4. Lua

- **Date:** 2026-09-30
- **Source:** 0043 §4, §14
- **Change:** extends §1
- **Summary:** Lua 5.1 is linked through `mlua` (MIT) in `triplespace-scribunto`, which vendors Scribunto's and WikibaseClient's GPL-2.0-or-later Lua and is therefore never dual-licensed.

### A5. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** 0050 §13
- **Change:** consolidates §1–17
- **Summary:** A1–A4 were folded into the Decision. The title's em dash became a full stop, the open questions were numbered, the Author line was added, and the list this ADR kept under "Changes to other ADRs" (quoted below) was replaced by the generated table; the placeholder "0027 §…" in §6 now cites 0027 Q1. No decision changed. Before this, A2–A4 were blockquotes. 0005 A32 records the dependency notes this ADR gave 0005 §2. The file before conversion is commit `0b26a3a`.

Replaced text: Changes to other ADRs of this ADR.

### A6. The RevisionChest reader moves to `scatter-adapter-mediawiki`

- **Date:** 2026-10-01
- **Source:** 0053 §12
- **Change:** amends §10
- **Summary:** The `.mwrev.zst` reader lives in the new MediaWiki adapter, on which the Wikidata adapter now depends, so that Wikipedia page revisions and Wikidata entity revisions share it.

Replaced text: §10 of this ADR.

### A7. RevisionChest stores for page history

- **Date:** 2026-10-01
- **Source:** 0054 §3
- **Change:** amends §10
- **Summary:** A RevisionChest store of a Wikipedia's history is the source a fork's revisions are seeded from, and receives the hiding sweep read from the repository's events.

Replaced text: §10 of this ADR.

### A8. Deployment checks

- **Date:** 2026-10-01
- **Source:** 0056 §10
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
- **Source:** 0057 §2
- **Change:** amends §1, §17; extends §12
- **Summary:** The server binary serves the site itself, so a small instance is still one binary and Postgres; a larger one may add `triplespace-web`, a stateless second binary that renders the site through the API. `server.ui` chooses whether the server serves the site, and the web tier's settings, its forwarder key among them, follow §12's rules. The image carries both binaries.

Replaced text: §1 of this ADR; §17 of this ADR.

### A11. The query service

- **Date:** 2026-10-04
- **Source:** 0059 §2
- **Change:** amends §1, §11
- **Summary:** The query service runs embedded (Oxigraph, in the binary) by default and may use QLever as a remote backend, so the one-binary rule of §1 holds with the service on, and QLever becomes an optional runtime service as well as an export destination. A QLever that backs the service loads the `full` form.

Replaced text: §1, item 1, in part of this ADR; §11 of this ADR.

### A12. Client compatibility in CI

- **Date:** 2026-10-06
- **Source:** Direct: James, Librarybase acceptance run of 2026-10-06 (test plan stage 1)
- **Change:** extends §2
- **Summary:** CI runs WikibaseIntegrator, Pywikibot and WikidataIntegrator, at pinned versions, against a server adopted from a Librarybase sample fixture; any failing check fails the build. Running the libraries found server requirements the reference contract does not state (Pywikibot's `siteinfo` and `paraminfo` structure, `meta=wikibase`), which are now part of the surface the job holds fixed.

### A13. The component manifest

- **Date:** 2026-10-07
- **Source:** 0077 §15
- **Change:** extends §16
- **Summary:** `cargo xtask manifest` records the build and every component linked into or sent from the binaries, with their licence texts, for `Special:Version` and its licence subpages; release builds require it.

### A14. File links are not chips

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §9.1
- **Summary:** `File:` and `Image:` links are no longer rendered as chips, and the File namespace is no longer "reserved, not implemented": files in wikitext follow 0039 §13's rule. The chip list of §9.1 loses that entry. (PENDING E19)

Replaced text: §9.1 of this ADR.

### A15. Implemented tags and switches render without expansion

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §9.1
- **Summary:** Tags and behaviour switches the registry marks `implemented` are rendered by the built-in renderer whether or not the tenant's expansion is on; §9.1's chip list is pruned to what the registry keeps as `chip`, and behaviour switches are no longer ignored wholesale (0042 §5). (PENDING E20)

Replaced text: §9.1 of this ADR.

### A16. Three binaries

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §1, §17
- **Summary:** The server binary is `triplespace-server`, not `triplespace`; `triplespace` is the CLI's binary. Packaging ships three binaries, `triplespace-server`, `triplespace-web` and `triplespace`, and the OCI image carries all three. Principle 1's "one binary" still means that a small instance runs one server process beside Postgres. (PENDING F27)

Replaced text: §1 of this ADR; §17 of this ADR.

### A17. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** 0050 §14
- **Change:** relocates §1–§17

### A18. The profiles at Wikidata scale: terms, `entity_ref`, the query backend and dumps

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §1; extends §8
- **Summary:** The deployment-profile table of §1 changes in four rows, from 0082 §6 and the review's query-service row. `view.term` holds rows for local entities, for entities in any tenant overlay, and for mirrored entities in the instance's term languages only, in both profiles; its size at Wikidata scale is mirrored entities × the instance's term languages × term kinds, the budget `entities.term_languages` sets. `entity_ref` is keyed on integer page IDs with a role bitmask and, with `view.term`, is the largest `view` table; the Wikidata-scale profile may omit it and serve "Links here" and the backlink count from the query service. The embedded query backend is the small profile's: the service refuses to start it above a configured triple count, and the Wikidata-scale profile requires `query.backend = remote` (QLever from dumps), so principle 1's "moving to QLever only when an instance chooses" becomes "when an instance chooses and necessarily at Wikidata scale". A dump is taken from a replica under `pg_export_snapshot()`, with parallel workers sharing the one snapshot (§8, in 02 §4.1; an extension). The ledger's G49 row named 0032 §1 and 0013 §11 for the profile and dump parts; the chapter sections that hold them, 22 §6 and §4.1 and 02 §4.1, cite §1 and §8 of this ADR, and 0082's table does not name this ADR, so both are logged here as a direct decision. Chapter text: 22 §6, §4.1. (REVIEW G7, G49)

Replaced text: 22 §6.

Replaced text: 22 §4.1.

### A19. The write path as tower layers is tier 1; `LogStore`'s unit of work

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §3
- **Summary:** The tower layers of §3 build the appending transaction, tier 1 of the write path of 0083 §1, in its order: auth → grants → rate limit → ACLs → filters → normalize and diff → base check → ID allocation → append → graph state → activity row → commit. Composition of the written entity follows commit as a step of the handler, and nothing else of tiers 2 and 3 runs in a request; "projections" is no longer a layer of the request. `LogStore`'s methods are written as `impl Future + Send` in the trait (return-position `impl Trait`), `scatter-log` depends on `std::future` alone, its file backend does synchronous I/O inside those futures and a runtime caller wraps a file-backend call in `spawn_blocking`; `LogStore` carries an associated unit-of-work type, the transaction or handle within which an append and the rows written beside it commit together, which `Backend::apply_in` in `scatter-projection` and `IngestStore` in `scatter-ingest` take rather than naming a database, so the write path appends and projects in one Postgres transaction while the in-memory and file backends stay runtime-free. The crate-side parts of the same row (`scatter-extract` at layer 2, payload-type homes, `scatter-titles`, the wasm budget, the `FilterHook`) are logged in 0005. The tier-1 rewrite is 0083 §1's, which the chapter section cites and 0083's table does not name, so it is logged here as a direct decision. (REVIEW G8, G51)

Replaced text: 22 §4.3.

### A20. `triplespace migrate`, and a server that refuses a database behind its migrations

- **Date:** 2026-10-16
- **Source:** Direct: James, running the `entity-history` branch against an existing instance, 2026-10-16
- **Change:** extends §4
- **Summary:** Until now only `instance create` ran the migration runner, so a database an older binary created kept its schema while a newer binary read tables it did not have, and every such read answered an internal error. `triplespace migrate` applies the migrations the binary carries that `ops.migration` does not record, in build order, and prints each; `triplespace status` lists any that are pending; and `triplespace-server` checks `ops.migration` on startup and refuses to start, naming the pending migrations and the command, rather than serve errors. The runner gains `pending()` for both. A migration that adds a derived table the writes fill from then on (`0010_revision_state`) is followed by `triplespace rebuild` where the earlier rows are wanted (0083 A2); the runner does not rebuild on its own.

### A21. `server.auth_base`

- **Date:** 2026-10-18
- **Source:** 0089 §1, §12
- **Change:** extends §12
- **Summary:** The deployment setting `server.auth_base` names the auth host, where a person authenticates to the instance. The chapter text is 23 §3.6.

### A22. MIT-0 is accepted, for the vendored `parse-wiki-text-2`

- **Date:** 2026-10-19
- **Source:** Direct: James, review of 2026-10-19
- **Change:** amends §16
- **Summary:** MIT-0 joins the accepted licences. It is the licence of `parse-wiki-text-2`, vendored into `scatter-wikitext` (§9.1, "MIT-style licence without a notice clause"): MIT's grant without its notice condition. It places no condition on redistribution, so the code can go into the GPL binary and also be passed on under the `scatter-*` crates' commercial licence, which Scatter's copyright alone could not grant for code it did not write. `cargo-deny` records the crate's licence file as MIT-0 with a clarify entry, since the manifest names only the file. The chapter text is 22 §3.3, §3.4.

Replaced text: 22 §3.3, §3.4.

### A23. The `.env` file

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-25
- **Change:** extends §12
- **Summary:** An instance's deployment configuration can be kept in a `.env` file instead of ad-hoc environment variables and long command lines. `triplespace`, `triplespace-server` and `triplespace-web` read it into the environment with `dotenvy` when they start, before `clap` parses their flags, so every flag bound to a `TRIPLESPACE_*` variable, and every variable read directly such as `TRIPLESPACE_OWNER_PASSWORD`, can be set there. The chapter text is 23 §5.1.

### A24. mimalloc is the binaries' allocator

- **Date:** 2026-11-04
- **Source:** Direct: James, design discussion of 2026-11-04
- **Change:** extends §2
- **Summary:** The three binaries (`triplespace`, `triplespace-server`, `triplespace-web`) set mimalloc (the `mimalloc` crate, MIT, bundling the C library) as the global allocator, behind a default `mimalloc` feature; without it they use the system allocator, for heap tools. Library crates set none. Measured on the 2M Wikidata benchmark's 20k slice, the same binary under glibc, jemalloc and mimalloc by `LD_PRELOAD`: CPU −20% with jemalloc and −26% with mimalloc, wall about −9% and −16%, peak RSS 49 MB, 128 MB and 85 MB. mimalloc was chosen for the larger gain at the smaller memory cost, and because it builds on Linux, macOS and Windows where jemalloc does not support MSVC (`docs/testing/performance.md` §8).

### A25. TLS to Postgres when the URL says `sslmode=require`

- **Date:** 2026-10-31
- **Source:** Direct: Claude, while building TLS to a managed Postgres, 2026-10-31; unconfirmed
- **Change:** extends §4
- **Summary:** The pool and the CLI's single connection open TLS when the database URL's `sslmode` is `require`. They use `tokio-postgres-rustls` over rustls on the aws-lc-rs provider (§6), and `rustls-platform-verifier` checks the server's certificate against the platform's roots, as the web tier's HTTP client does. `disable`, and `prefer` (`tokio-postgres`'s default), stay plaintext as before, so a local server or one behind a TLS-terminating proxy is unaffected. This lets the small profile run on a managed Postgres that refuses plaintext, Neon among them, with no proxy. Two calls are open for James. First, `prefer` does not try TLS, unlike libpq; that keeps current deployments unchanged, but a URL that wants TLS has to say `require`. Second, `require` here also verifies the certificate, which is stricter than libpq's `require` and matches its `verify-full`. The chapter text is 03 §1.4 and 22 §4.4.

### A26. No commercial licence

- **Date:** 2026-11-07
- **Source:** Direct: James, design discussion of 2026-11-07
- **Change:** amends §10
- **Summary:** Code moving from RevisionChest into the Wikibase adapter no longer needs the Internet Archive's agreement, because the `scatter-*` crates are not offered under a commercial licence and Scatter need not hold copyright in every line of them (0005 A114); the code keeps its licence notice.

Replaced text: 13 §3.5.

### A27. The probe

- **Date:** 2026-11-09
- **Source:** 0107 §1
- **Change:** extends §15
- **Summary:** Testing gains the probe: `--probe FILE` in both binaries reports the permissions, settings and log events a run uses, and `tools/probe_report.py --strict` fails a CI job that used one its registry file does not list. Chapter text: 22 §4.9.

### A28. Dependabot keeps dependency versions current

- **Date:** 2026-11-09
- **Source:** Direct: Claude, while building the Dependabot configuration, 2026-11-09; unconfirmed
- **Change:** extends §16
- **Summary:** James asked for Dependabot to keep dependency versions up to date and for the policy to be recorded here; the choices below are Claude's, for him to confirm. `.github/dependabot.yml` opens version-update pull requests weekly for the Cargo workspace, `ui/`'s npm packages, `docs/requirements.txt` and the workflows' actions, and monthly for the fuzz workspace and `tools/requirements-compat.txt`. Minor and patch releases are grouped per ecosystem and majors come alone, so a breaking release is reviewed by itself. The client libraries are included one per pull request: their pin keeps a client release from changing CI's verdict on its own, and merging the pull request after its client-compatibility run is the deliberate bump (recording it in the test plan's log stays manual); `setuptools` stays below 80 for WikidataIntegrator. The Rust toolchain is excluded, since it moves in its own commit (§2). The generated-files workflow and the ADR preview skip Dependabot's pull requests, whose runs get a read-only token. Security updates are a repository setting, outside this file. Chapter text: 22 §3.4.

### A29. bzip2-1.0.6 is accepted, for `libbz2-rs-sys`

- **Date:** 2026-11-10
- **Source:** Direct: James, design discussion of 2026-11-10
- **Change:** amends §16
- **Summary:** bzip2-1.0.6 joins the accepted licences. It is the licence of bzip2 and libbzip2: a permissive BSD-style text that asks for the copyright notice to be kept, the origin not to be misrepresented, altered versions to be marked, and the author's name not to be used to endorse what is derived; it has no copyleft and no advertising clause. `bzip2` 0.6 reads the dumps (§10) through `libbz2-rs-sys`, a Rust port that declares this licence, where `bzip2` 0.5 used `bzip2-sys`, which bundles the C library under the same terms while declaring MIT or Apache-2.0: the code was already linked under this licence, and `cargo-deny` now sees it. The occasion was Dependabot's pull request for `bzip2` 0.6, which `cargo-deny` rejected. The chapter text is 22 §3.3.

Replaced text: 22 §3.3.

### A30. Dependabot ignores a major release that cannot be taken yet

- **Date:** 2026-11-10
- **Source:** Direct: Claude, while building the first week's Dependabot updates, 2026-11-10; unconfirmed
- **Change:** extends §16
- **Summary:** Three of the first week's pull requests could not be merged for a reason outside the repository, and Dependabot would reopen each weekly. `.github/dependabot.yml` now ignores the major releases of `getrandom` (the workspace names it only to turn on `wasm_js` for the copy `oxrdf`'s `rand` 0.9 pulls in, which is `getrandom` 0.3; 0.4 is a second copy without the feature and the wasm32 build of the rule-7 crates fails), `eslint` (`eslint-config-wikimedia` 0.32 is an eslintrc configuration with a peer of ESLint ^8.57, and ESLint 9 and later read only flat configuration) and `typescript` (`vue-tsc` 3.3 needs TypeScript below 7, which has no `lib/tsc`). Each entry carries its reason in the file and is removed when the reason goes. `@playwright/test` was held at 1.63.0 in the same change without an ignore entry: the cloud sandbox has only the browser build that release expects and cannot download another ([gotchas](../testing/gotchas.md)); whether to ignore it, or to take it and lose the sandbox's e2e run, is James's to choose. The chapter text is 22 §3.4.

### A31. A database ahead of the binary

- **Date:** 2026-11-13
- **Source:** Direct: Claude, fixing what a review of the migrations found, 2026-11-13; unconfirmed
- **Change:** extends §4
- **Summary:** The runner compared the database with the binary's own migrations only, so a database a newer binary had migrated looked to an older one like a database in step: the older server started on it, `triplespace migrate` said there was nothing to do and gave the older binary's grants, and a backup recorded the older binary's list. The calls made: (1) **a migration the database records and the binary lacks makes the database newer than the binary**; `triplespace-server` refuses to start on one, naming the migrations, as it refuses one behind it (A20), and the runner refuses to migrate one; (2) this also stops an older replica from starting during a rolling upgrade once the database is migrated, and a rollback of the binary alone, both of which ran before, on a schema the code did not know; whether either should be allowed, for migrations marked as safe for the version before, is not decided; (3) a backup records the database's own rows (0103 A13). The runner still takes no lock: two runs at once end with one of them failing on `ops.migration`'s key, its migration rolled back.

### A32. Blob storage no longer uses `object_store`

- **Date:** 2026-11-17
- **Source:** 0039 A17
- **Change:** amends §5
- **Summary:** Blob storage is `scatter-blob`'s own backend trait, with the `fs` backend built in the crate on tokio's filesystem calls; `object_store`, chosen in A2, is no longer a dependency, and with it `chrono` and `humantime` leave the dependency graph, both declared unmaintained (RUSTSEC-2026-0543, RUSTSEC-2026-0542). The `s3` backend's client is not yet chosen; it must pass §16's `cargo deny` check and not bring in `chrono`. The chapter text is 03 §8, §9.2.

Replaced text: 03 §8, §9.2.

### A33. Triplespace depends on RevisionChest's library crates

- **Date:** 2026-11-20
- **Source:** 0112 §1
- **Change:** amends §10
- **Summary:** Triplespace depends on three of RevisionChest's crates from crates.io: `mwrev` (the bundle format), `mwdump` (MediaWiki's export XML) and `mwrecent` (the recent-changes poller); RevisionChest's binary still runs on its own. §10, and fixed point 4 of the Context, held that RevisionChest stays a separate binary that Triplespace does not link, which was decided when it had no library target; it has four crates now, and James decided on 2026-11-19 that Triplespace depends on them. The two stores stay separate (0112 §5), and live sync beyond the poller is still meant to move to RevisionChest later (0112 §3). The list of what RevisionChest's `sync` lacked is gone from the chapter: hidden text and offsets are fixed in RevisionChest 0.2, and the rest is its `sync`'s to fix. Chapter text: 13 §3.5, 22 §4.11.

Replaced text: 13 §3.5.

### A34. A published crate embeds its registry files through symlinks

- **Date:** 2026-11-22
- **Source:** 0115 §2
- **Change:** extends §12
- **Summary:** A crate published on crates.io embeds each registry file it needs as `registry/{file}.toml`, a symlink under its own directory to `docs/registry/{file}.toml`, because `cargo package` takes only what is under the crate's directory and stores a symlink's target as a file. `docs/registry/` stays the only copy in the repository, and a test in the crate fails when the embedded text differs from it. The crates that are not published keep embedding `docs/registry/` directly. Chapter text: 23 §4.1, 22 §3.6.
