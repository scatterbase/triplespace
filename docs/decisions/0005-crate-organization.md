# 0005. Crate organization for reuse by Scatterbase

- **Status:** Proposed
- **Date:** 2026-09-25
- **Updated:** 2026-11-15 (A126)
- **Author:** James Hare / Claude Opus; revision by James Hare / Claude Fable
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

*Current text: [22](../architecture/22-crates-and-stack.md) §1.1.*

### 2. Crate map

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

### 3. Rules

*Current text: [22](../architecture/22-crates-and-stack.md) §1.2.*

### 4. Seams shared with Scatterbase

*Current text: [22](../architecture/22-crates-and-stack.md) §1.3.*

#### 4.1 Named graphs are registry entries

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.1, §1.2; [22](../architecture/22-crates-and-stack.md) §1.3.*

#### 4.2 Log partitions carry their own policy

*Current text: [01](../architecture/01-log-and-records.md) §1.2, §1.3, §4.1, §6; [22](../architecture/22-crates-and-stack.md) §1.3.*

#### 4.3 A record is a header plus a payload

*Current text: [01](../architecture/01-log-and-records.md) §2.1, §2.3, §2.4; [22](../architecture/22-crates-and-stack.md) §1.3.*

#### 4.4 Storage contracts

*Current text: [03](../architecture/03-storage-caches-and-search.md) §1.3; [22](../architecture/22-crates-and-stack.md) §1.3.*

### 5. One vocabulary under `https://scatter.red/terms/v0/` (amends 0001 §5)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §3.1.*

### 6. Naming, licensing and repository

*Current text: [22](../architecture/22-crates-and-stack.md) §3.2, §3.3, §3.4.*

### 7. Build order

*Current text: [22](../architecture/22-crates-and-stack.md) §3.1.*

### 8. Changelog of this revision

*Superseded by the Amendment log.* Each row of the changelog this section held is an entry there, among A1–A49.

## Consequences

- **Scatterbase can adopt the substrate in place of its own.** To do so it has to move from its own term types and `rio` to the `oxrdf` family, and replace its write-only `Storage` trait with `scatter-quadstore` for serving and `LogStore` for the log. Both changes are already on its Phase 1–2 path, and it gains a Postgres backend for the log without asking for one.
- **Scatterbase's planned Wikibase extension mostly exists already.** Its namespaces, data types and Wikidata import come from `scatter-providers`, `scatter-wikibase-*` and `scatter-adapter-wikidata`. What remains is binding them to signed claims.
- **Fifty-nine crates cost more upkeep than twenty-two.** The dependency check in CI is what keeps the table honest; every ADR that adds a crate has to update §2 in the same change. The alternative, extracting crates from a single crate later, is harder than starting split. *Sixty with `scatter-titles` (A92); the claim holds.*
- **Shared crates change for two products.** A breaking change in a `scatter-*` crate, in the `scatter:` vocabulary, or in the `log` schema has to work for both.
- ~~**Triplespace carries features it does not use yet.** Room for client signatures in the attestation slot exists in `scatter-log` before any Triplespace code needs it.~~ *Triplespace uses it: an actor may sign its own record with the client `signature` of [0015](0015-record-format-and-partition-registry.md) §1 (A81).*
- **The `scatter.wiki` domain is no longer part of the design.** The illustrations in 0001 now read as `scatter:`, and nothing mints terms under `scatter.wiki`.
- ~~**GPLv3 limits who can reuse the crates freely.** A third party that wants the Wikibase crates under other terms needs the commercial license. Clients in other languages avoid this by implementing the CC0 contract in `docs/` rather than binding to the crates, at the cost of reimplementing what they need, such as canonical CBOR and proof checking.~~ *There is no commercial licence: a third party takes the crates under GPL-3.0-or-later or implements the CC0 contract in `docs/` (A114).*
- **Earlier ADRs name crates that have moved.** 0008, 0009, 0010, 0011, 0012 and 0019 keep their original crate names in their text; §2 records where each now lives. *0008, 0009, 0011 and 0012 no longer do: their crate sections are stubs (0008 A1, 0009 A1, 0011 A1, 0012 A1), and 0010 §13 names the new homes. 0019 keeps them until it is converted ([0050](0050-adr-format.md) §13).*
- **Three crates read `private`.** The single-reader rule of 0013 §4 became a two-reader rule with `triplespace-notify` and a three-reader rule with `triplespace-federation` ([0022](0022-federation.md) §13); `triplespace-oauth` deliberately does not join them. The grant-level check and the privacy test of [0012](0012-api-requirements.md) §8 cover all three.

## Open questions

- **Q1. Log granularity** ([0000](0000-init.md)) has been settled in practice and not recorded: mirrors receive whole-entity `put`s ([0002](0002-source-graphs-and-mass-ingest.md) §8.2, [0012](0012-api-requirements.md) §2.2), local edits are statement-level operations, and both are change sets. Whether a local change set should ever carry a whole-entity replacement beyond the explicit base-revision case in 0002 §8.2 is the part that remains. *Narrowed 2026-09-28 by [0035](0035-adopting-a-wikibase.md) §3: `adopt` is the one whole-entity write to the local graph, confined to adoption jobs; whether an ordinary edit may replace an entity against a base revision is still open.*
- **Q2. Python bindings.** *Settled by A30.*
- **Q3. Publishing to crates.io,** and under which owner.
- **Q4. When to extract the shared repository.** The trigger in §6 is Scatterbase's first dependency. An earlier split may suit contributors.
- **Q5. Whether `scatter.wiki` should redirect** to `scatter.red/terms/v0/` or stay unused.
- **Q6. Graph IRIs.** *Settled by [0015](0015-record-format-and-partition-registry.md) §5.*
- **Q7. Whether `scatter-log-postgres` should offer a blocking API.** *Settled by [0033](0033-backend-stack.md) §4.* *Extended by A92.*

## References

- Scatterbase Project Plan, 22 September 2026 (Phases 1–4, divergences #3–#5)
- Scatterbase `docs/claim-envelopes.md` (namespace declarations)
- [Oxigraph and the `oxrdf` crates](https://github.com/oxigraph/oxigraph)
- [`sqlx`](https://github.com/launchbadge/sqlx), [`idna`](https://crates.io/crates/idna)

## Amendment log

### A1. Crates of 0006 §10

- **Date:** 2026-09-25
- **Source:** 0006 §10
- **Change:** amends §2, §4.2, §4.3
- **Summary:** `scatter-integrity` added (already in the original table); §4.3 rewritten to match 0006 §3

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A2. Crates of 0007 §8, §10

- **Date:** 2026-09-25
- **Source:** 0007 §8, §10
- **Change:** extends §2, §4.1
- **Summary:** `scatter-actors`, `triplespace-accounts` added; `private` export policy and the `accounts` and `actors` graphs added to §4.1

### A3. Crates of 0008 §4, §11

- **Date:** 2026-09-26
- **Source:** 0008 §4, §11
- **Change:** amends §2, §4.1
- **Summary:** `scatter-pages`, `scatter-wikitext`, `triplespace-titles` added; `pages` graph added to §4.1; wasm rule extended. `scatter-markdown` folded into `scatter-pages` as a feature

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A4. Crates of 0009 §12

- **Date:** 2026-09-26
- **Source:** 0009 §12
- **Change:** amends §2
- **Summary:** `scatter-adapter-internetdomains` added; `scatter-wikibase-model` and `scatter-wikibase-changeset` contents extended. `scatter-keyed` folded into `scatter-normalize`, which now builds for wasm and depends on `idna`

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A5. Crates of 0010 §13

- **Date:** 2026-09-26
- **Source:** 0010 §13
- **Change:** amends §2
- **Summary:** `triplespace-activity` added, then absorbed by 0013

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A6. Crates of 0011 §2, §10

- **Date:** 2026-09-26
- **Source:** 0011 §2, §10
- **Change:** extends §2, §4.1
- **Summary:** `scatter-mwlog` added; `scatter-adapter-wikidata` extended; `log` graphs added to §4.1

### A7. Crates of 0012 §9

- **Date:** 2026-09-26
- **Source:** 0012 §9
- **Change:** extends §2
- **Summary:** `triplespace-upstream` added; API crates, `scatter-wikibase-changeset` and `scatter-ingest` contents extended

### A8. Crates of 0013 §1, §8, §10

- **Date:** 2026-09-26
- **Source:** 0013 §1, §8, §10
- **Change:** amends §2, §3, §4.2, §4.4
- **Summary:** `LogStore` trait and Postgres backend; `scatter-log-postgres`, `triplespace-db`, `triplespace-projections`, `triplespace-rdf` added; `triplespace-activity` and `triplespace-revmeta` absorbed; §4.4 rewritten; rule 2 amended for the async backend; rule 9 added; §4.2 gains the Postgres backing

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A9. Crates of 0014 §9

- **Date:** 2026-09-26
- **Source:** 0014 §9
- **Change:** extends §2
- **Summary:** `triplespace-cache`, `triplespace-search` added; search-document builder placed in `scatter-wikibase-model`

### A10. Crates of 0015 §7

- **Date:** 2026-09-26
- **Source:** 0015 §7
- **Change:** amends §2, §3, §4.1, §4.2, §4.3
- **Summary:** Header fields 7–9, body parts, the `config` partition and `docs/registry/`: `scatter-log`, `scatter-log-postgres`, `scatter-integrity`, `scatter-providers`, `scatter-mwlog`, `scatter-adapter-wikidata`, `triplespace-projections`, `triplespace-rdf` contents extended; `config` graph added to §4.1; §4.3 rewritten; rule 3 names the `config` partition

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A11. Crates of 0016 §9

- **Date:** 2026-09-26
- **Source:** 0016 §9
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
- **Source:** 0017 §7
- **Change:** extends §2
- **Summary:** `scatter-adapter-musicbrainz` added; `scatter-wikibase-model`, `scatter-providers`, `scatter-normalize`, `scatter-wikibase-resolve`, `scatter-adapter-openalex` and `triplespace-titles` contents extended

### A14. Crates of 0018 §12

- **Date:** 2026-09-27
- **Source:** 0018 §12
- **Change:** extends §2, §4.1, §4.2
- **Summary:** 64-bit partition IDs, the `tenant` and `alias` config kinds and the key chain in `scatter-log`; `partition` as `bigint` and a `tenant` column in `scatter-log-postgres` and `triplespace-db`; read-time ID rewriting in `scatter-providers`; tenant issuers in `scatter-actors`; the GUID rewrite in `scatter-wikibase-resolve`; overlays in `triplespace-projections` and `triplespace-search`; host-to-tenant resolution, reclaiming and tenant export/import in the surfaces. §4.1 registry keyed by (tenant, name)

### A15. Crates of 0019 §13

- **Date:** 2026-09-27
- **Source:** 0019 §13
- **Change:** amends §2
- **Summary:** `scatter-threads` added, depending on `scatter-pages` only (0019's `scatter-log` dependency dropped); wiki links and the sanitizer added to `scatter-pages`'s `markdown` feature; `triplespace-projections`, `triplespace-titles`, `triplespace-rdf` and the API crates extended; `scatter-markdown` in 0019 §13 reads as `scatter-pages`

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A16. Crates of 0020 §8

- **Date:** 2026-09-27
- **Source:** 0020 §8
- **Change:** extends §2
- **Summary:** Feed queries added to `triplespace-projections`, which now also serves reads over `view.activity`; watch set and Atom token in `triplespace-accounts`; watchlist modules, Atom and the stream in the API crates

### A17. Crates of 0021 §10

- **Date:** 2026-09-27
- **Source:** 0021 §10
- **Change:** extends §2, §6, §7
- **Summary:** `scatter-activitypub` and `triplespace-notify` added; `triplespace-notify` joins `triplespace-accounts` as a reader of `private` (0013 §4, as revised); contact details and the notifier key in `triplespace-accounts`; Echo-shaped modules and ActivityPub endpoints in the API crates; RSA dependency noted in §6; build order gains step 8

### A18. Crates of 0022 §13

- **Date:** 2026-09-27
- **Source:** 0022 §13
- **Change:** amends §2, §7
- **Summary:** `triplespace-federation` added, taking ActivityPub delivery and the `ap_*` tables from `triplespace-notify` and becoming the third reader of `private`; remote verification in `scatter-integrity`; actor documents and inbound parsing in `scatter-activitypub`; local-graph reads, back-rewrite and verification in `scatter-adapter-triplespace`, which now depends on `scatter-integrity`; the source dump in `triplespace-rdf`; `/.well-known/tlog/` in the API crates; build order gains step 9

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A19. Crates of 0023 §13

- **Date:** 2026-09-27
- **Source:** 0023 §13
- **Change:** amends §2
- **Summary:** The `acl` payload type in `scatter-log`; read evaluation, `record` and `actor` targets and the autopatrol rule in `scatter-actors`; `delete`/`undelete` removed from `scatter-pages` and `scatter-threads`; `patrol` and `suppress` types in `scatter-mwlog`; ACL and patrol projections in `triplespace-projections`; reserved page IDs in `triplespace-titles`; the modules in the API crates

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A20. Crates of 0024 §13

- **Date:** 2026-09-27
- **Source:** 0024 §13
- **Change:** extends §2
- **Summary:** Operator, grants, block inheritance and rate-limit policy in `scatter-actors`, which embeds `grants.toml`; `newusers/create2` and `bot/transfer` in `scatter-mwlog`; subsidiaries and keys in `triplespace-accounts`; counters in `triplespace-cache`; login and key routes in the API crates; sync subsidiaries in `triplespace-cli`

### A21. Crates of 0025 §11

- **Date:** 2026-09-27
- **Source:** 0025 §11
- **Change:** extends §2
- **Summary:** `triplespace-oauth` added, reaching `private` through `triplespace-accounts`; the `pending` status in `scatter-actors`; `oauth/*` in `scatter-mwlog`; token tables and token authentication in `triplespace-accounts`; OAuth routes and the `oauth-pending` refusal in the API crates

### A22. Crates of 0026 §10

- **Date:** 2026-09-27
- **Source:** 0026 §10
- **Change:** extends §2
- **Summary:** URL normalizer in `scatter-normalize`; sitelinks as URLs and `sites.toml` in `scatter-wikibase-model`; RDF, reconciliation and the denied-host filter in the Wikibase crates; site-table resolution in `scatter-adapter-wikidata`; `view.sitelink` in `triplespace-projections`; sitelink modules and routes in the API crates

### A23. Crates of 0027 §9

- **Date:** 2026-09-27
- **Source:** 0027 §9
- **Change:** extends §2
- **Summary:** Portability classes in `triplespace-db`; preferences, `preferences.toml`, export, import and the private extract in `triplespace-accounts`; `triplespace-notify` reads preferences; `view-pin` in `triplespace-projections`; the extract in `triplespace-cli`

### A24. Crates of 0028 §14

- **Date:** 2026-09-27
- **Source:** 0028 §14
- **Change:** extends §2
- **Summary:** `scatter-adapter-triplespace` added; tenancy policy, global-group and global-block evaluation, templates and locks in `scatter-actors`, which embeds `tenancy.toml`; the `tenancy`, `template` and `provider-readers` kinds in `scatter-log`; farm accounts and the name registry in `triplespace-accounts`; the all-tenants target set and cross-tenant reads in `triplespace-projections`; farm-wide search in `triplespace-search`; `instance create --tenancy` in `triplespace-cli`

### A25. Crates of 0029 §9 and the 0023 §2 amendment

- **Date:** 2026-09-27
- **Source:** 0029 §9; 0023 §2, its amendment of 2026-09-27
- **Change:** extends §2
- **Summary:** Resolver grammars and `resolvers.toml` in `scatter-normalize`; the `resolver` kind and lookup in `triplespace-titles`; resolver routes in the API crates; statement and property ACL targets in `scatter-actors`

### A26. Crates of 0030 §13

- **Date:** 2026-09-27
- **Source:** 0030 §13
- **Change:** extends §2
- **Summary:** `scatter-filter` added (layer 2, pure, CEL); per-operation filtering in `scatter-ingest`; `view.filter`, `view.filter_hit`, the hits target set and the test job in `triplespace-projections`; the `filter` reason in `triplespace-notify`; refusal errors and evaluation in the write path in the API crates

### A27. Crates of 0031 §9

- **Date:** 2026-09-27
- **Source:** 0031 §9
- **Change:** extends §2
- **Summary:** `scatter-wikibase-constraints` added (layer 2, pure); the constraint projection, `view.constraint_violation`, `view.constraint_count`, `view.value_key` and the re-check job in `triplespace-projections`; `wbcheckconstraints` and the report routes in the API crates

### A28. Crates of 0032 §9

- **Date:** 2026-09-27
- **Source:** 0032 §9
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
- **Summary:** §6 License rewritten: Triplespace is GPL-3.0-or-later, `scatter-*` crates also commercially licensed, `docs/` CC0-1.0 apart from third-party snapshots, bindings in other languages Apache-2.0 and independent of Triplespace code; the per-dependency licence notes moved to 0033 §1 and §16; the Python-bindings open question settled

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A31. Third review pass

- **Date:** 2026-09-27
- **Source:** Direct: review, third pass of 2026-09-27
- **Change:** corrects §1, §2, §4.1, §4.3, §7
- **Summary:** Rows for `scatter-ingest`, `triplespace-projections`, `triplespace-accounts`, `triplespace-notify`, `triplespace-cache`, `triplespace-search`, the API crates and `triplespace-cli` brought up to date with 0026–0031, which the earlier passes had logged in this table but not written into §2; `scatter-ingest` depends on `scatter-filter`; the adapter rule under §2 admits the transitive `scatter-integrity` dependency of 0022; §1 names filters, constraints, federation and OAuth; §4.1's destructible row names the farm partitions of 0028; §4.3 names the fourth part, ACL, filter and hit payloads, tags and evidence; build order step 7 places 0030 and 0031

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A32. Backend dependencies

- **Date:** 2026-09-27
- **Source:** 0033 §4, §9.1–9.4, §10
- **Change:** amends §2
- **Summary:** `scatter-log-postgres` uses `tokio-postgres`, with a blocking API possible through the synchronous `postgres` crate; `scatter-wikitext` vendors `parse-wiki-text-2`; `scatter-pages` renders markdown with `comrak` and sanitizes with `ammonia`; `scatter-filter` uses `cel`, formerly `cel-interpreter`; the Wikidata adapter reads RevisionChest's `.mwrev.zst` files. 0033 listed these changes, and settled Q7, but §2 never received them until the migration.

Replaced text: §2 of this ADR.

### A33. Frontend crates

- **Date:** 2026-09-27
- **Source:** 0034 §2, §6
- **Change:** extends §2, §3; amends Consequences
- **Summary:** `triplespace-ui` renders pages on the server with `askama` and Codex markup; `scatter-wasm` binds the pure crates editors need for the browser. 0034 listed both as new crates in 0005, but §2 never received them until the migration.

Replaced text: Consequences of this ADR.

### A34. Crates of 0035 §8

- **Date:** 2026-09-28
- **Source:** 0035 §8
- **Change:** extends §2, §7
- **Summary:** The `adopt` operation in `scatter-wikibase-changeset`; the adoption job in `scatter-ingest`; adoption mode in `scatter-adapter-wikidata`; entity-ID sequences and floors in `scatter-log-postgres`; `import/*` for entities in `scatter-mwlog`; adopted-record provenance in `triplespace-rdf`; `instance create --adopt` and `adopt` in `triplespace-cli`; §7 step 4 names the adoption job

### A35. Crates of 0036 §7

- **Date:** 2026-09-28
- **Source:** 0036 §7
- **Change:** extends §2
- **Summary:** `scatter-adapter-openstreetmap` added; `scatter-normalize` (the `osm-tag` grammar) and `triplespace-titles` (the `OSM` namespace) contents extended

### A36. Crates of 0037 §8

- **Date:** 2026-09-28
- **Source:** 0037 §8
- **Change:** amends §2
- **Summary:** `scatter-adapter-gdelt` added; `scatter-providers` (per-type `id_grammar`, and the `gdelt-record` and `token` grammars, which 0037 §8 first placed in `scatter-normalize`; corrected 2026-09-28) contents extended

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A37. The hash guard

- **Date:** 2026-09-28
- **Source:** Direct: James, the hash guard of 2026-09-28 (0006 §2)
- **Change:** amends §2
- **Summary:** Wikibase's snak, reference and value-node hash emulation named in `scatter-wikibase-model`; the ingest-time recompute-and-compare with kept mismatches in `scatter-ingest` (0006 §2, as amended)

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A38. Crates of 0038 §15

- **Date:** 2026-09-29
- **Source:** 0038 §15
- **Change:** extends §2
- **Summary:** No new crate. Category extraction in `scatter-wikitext`; page subjects, page-statement IDs and local-host sitelinks by page ID in `scatter-wikibase-model`; page change sets in `scatter-wikibase-changeset`; the main and `Category` namespaces in `triplespace-titles`; category, mapping, status and page-statement projections in `triplespace-projections`; page statements in the main graph in `triplespace-rdf`; `categories` and `statement_keywords` in `triplespace-search`; routes and modules in the API crates

### A39. Crates of 0039 §23

- **Date:** 2026-09-30
- **Source:** 0039 §23
- **Change:** extends §2
- **Summary:** `scatter-blob` (substrate, async, shared with Scatterbase), `scatter-files` (pure) and `triplespace-files` added; the instance `log` and `files/{repo}` partitions in `scatter-log`; the `blob` target and file rights in `scatter-actors`; blob verification in `scatter-integrity`; `localMedia` in the Wikibase model and RDF crates; file embeds in `scatter-wikitext` and `scatter-pages`; the `upload` context in `scatter-filter`; file log types in `scatter-mwlog`; file tables, titles, upstream, cache, search, API and CLI rows extended

### A40. Crates of 0040 §11

- **Date:** 2026-09-30
- **Source:** 0040 §11
- **Change:** extends §2
- **Summary:** No new crate. The instance attestation in `scatter-log`; its verification and the authority extract in `scatter-integrity`; the `instance` issuer, instance rights and binding in `scatter-actors`; log parameters, projections and routes

### A41. Crates of 0041 §12

- **Date:** 2026-09-30
- **Source:** 0041 §12
- **Change:** extends §2
- **Summary:** No new crate. The content model registry and trait in `scatter-pages`; `triplespace-thread` in `scatter-threads`; the entity models and `mediainfo` in `scatter-wikibase-model`, which now depends on `scatter-pages`; the `pages` kind in `triplespace-titles`; projections and API rows extended

### A42. Crates of 0042 §19

- **Date:** 2026-09-30
- **Source:** 0042 §19
- **Change:** extends §2, §3, §7
- **Summary:** `scatter-wikitext-expand` (substrate, pure) and `triplespace-render` added; expanded-text rendering and extraction in `scatter-wikitext`; conditional namespaces in `triplespace-titles`; render tables, cache keys, search fields, modules and CLI commands; rule 2 names `scatter-wikitext-expand`; build order gains step 11

### A43. Crates of 0043 §17

- **Date:** 2026-09-30
- **Source:** 0043 §17
- **Change:** extends §2, §3
- **Summary:** `triplespace-scribunto` added (pure, GPL-only, links Lua through `mlua`); the `Scribunto` model in `scatter-pages`; Module namespaces in `triplespace-titles`; `view.entity_usage`, `ld:` keys and the Wikibase-client modules; rule 2 names it

### A44. Crates of 0044 §6

- **Date:** 2026-09-30
- **Source:** 0044 §6
- **Change:** extends §2
- **Summary:** No new crate. Tenant-relative IDs in `scatter-wikibase-model` and the `entity-id` normalizer; reserved doubled codes in `scatter-providers`

### A45. Crates of 0045 §13

- **Date:** 2026-09-30
- **Source:** 0045 §13
- **Change:** extends §2
- **Summary:** No new crate. The `triplespace-table` model in `scatter-wikibase-shape`, which gains a direct dependency on `scatter-pages`; its registry entry in `scatter-pages`; Table namespaces in `triplespace-titles`; table links in `triplespace-projections`; table modules and routes in the API crates

### A46. Crates of 0046 §10

- **Date:** 2026-09-30
- **Source:** 0046 §10
- **Change:** extends §2
- **Summary:** No new crate. The `primary` config kind and the `{farm base}/instance/` path for instance graph IRIs and origin lines in `scatter-log`; instance rights evaluated against the tenant primary at the record's offset, and `ts-primary`, in `scatter-actors`; the authority attester check in `scatter-integrity`; `primary/*` in `scatter-mwlog`; mirror sync job records and consumer events from the instance `log` in `triplespace-projections`; `--farm-slug` and `primary offer`, `withdraw` and `accept` in `triplespace-cli`

### A47. Crates of 0047 §14

- **Date:** 2026-09-30
- **Source:** 0047 §14
- **Change:** extends §2
- **Summary:** No new crate. `special-pages.toml` embedded by `triplespace-titles`, which resolves special-page names, aliases, section aliases and scopes; the `report` and `site_stats` projections, the new `page` and `actor` columns and the batch report runner in `triplespace-projections`; `records` exports in `scatter-integrity`; `specialpagealiases`, `list=querypage`, export and the report, Nuke and export routes in the API crates; `verify --records` in `triplespace-cli`

### A48. Crates of 0048 §8

- **Date:** 2026-10-01
- **Source:** 0048 §8
- **Change:** amends §2
- **Summary:** No new crate. The `notation` keyed type, the scheme registry with its normalizers and the `osm-tag` and `text` grammars in `scatter-normalize`; `notation` and `wikibase-notation` replace `osm-tag` and `wikibase-osm-tag` in `scatter-wikibase-model`; `skos:notation` triples in `scatter-wikibase-rdf`; `key_scheme` in `scatter-adapter-wikidata`; the `Notation` namespace in `triplespace-titles`; the `notation-scheme` check in the API crates

Replaced text: not recorded. 0005 was revised in place; the repository's history begins with commit `1e53c95` (2026-09-27).

### A49. Crates of 0049 §15

- **Date:** 2026-10-01
- **Source:** 0049 §15
- **Change:** extends §2
- **Summary:** No new crate. `attach`, `detach`, the home-and-listings fold and the `triplespace-board` model in `scatter-threads`; its registry entry in `scatter-pages`; namespaces 310 and 311 and `forwards_to` in `triplespace-titles`; `view.thread_attachment`, board `view.talk_page` rows and `thread/*` log events in `triplespace-projections`; `triplespace-rdf`, `triplespace-federation`, `triplespace-search` and the API crates extended

### A50. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** 0050 §13
- **Change:** consolidates §2, §3, §8
- **Summary:** The changelog of §8 became this log, one entry per row, in date order; §8 is a stub. The rows of 0033 and 0034, which those ADRs promised and §2 never received, were folded in. The header's `Revised` line read: "2026-09-26, to incorporate 0006 through 0018; 2026-09-27, to add the crates of 0019 through 0021, and later that day for 0022 through 0031, with a third pass that evening to bring every row up to date with those ten. §8 lists what changed. The dependency table in §2 is the one CI checks (§3, rule 1), so it is kept current here rather than spread across the amending ADRs." The open questions were numbered. The file before conversion is commit `0b26a3a`.

### A51. Page repositories

- **Date:** 2026-10-01
- **Source:** 0052 §10
- **Change:** extends §2
- **Summary:** No new crate. The text is in git (`tools/adr.py log 0005 A51 --diff`).

### A52. `scatter-adapter-mediawiki` and `triplespace-repos`

- **Date:** 2026-10-01
- **Source:** 0053 §12
- **Change:** amends §2
- **Summary:** Two crates added: the MediaWiki adapter in the ingest layer, which takes the RevisionChest `.mwrev.zst` reader from `scatter-adapter-wikidata` (which now depends on it), and `triplespace-repos` in the surfaces layer for fetching, rewriting, caching, mirroring and indexing repository pages; the `pages/{repo}` graph and `scatter:v0/mirrored-page` in `scatter-log`.

Replaced text: §2, `scatter-adapter-wikidata` of this ADR.

### A53. Forks

- **Date:** 2026-10-01
- **Source:** 0054 §12
- **Change:** extends §2
- **Summary:** No new crate. The text is in git (`tools/adr.py log 0005 A53 --diff`).

### A54. `scatter-css`

- **Date:** 2026-10-01
- **Source:** 0055 §9
- **Change:** extends §2, §3
- **Summary:** `scatter-css` added to the substrate as a pure, wasm-building crate, joining rule 2's list; `scatter-pages` (the `sanitized-css` model), `scatter-wikitext` (the two tags), `scatter-wikitext-expand` (page properties), `triplespace-render` (sheets and site styles), `triplespace-projections` (`view.page_prop`) and `scatter-wasm` (linting) extended.

### A55. The security model

- **Date:** 2026-10-01
- **Source:** 0056 §16
- **Change:** extends §2
- **Summary:** `scatter-actors` gains the `tenant` and `set` targets, visibility sets, the include subset test and the lock-out check; `triplespace-projections` the visibility tables; `triplespace-cache` the `{vis}` segment and epoch; `triplespace-search` `read_groups` and its filter; `scatter-wikitext-expand` the include rule; `triplespace-server` the deployment settings, host check and landing page; `triplespace-cli` `instance check`.

### A56. `LogStore` is asynchronous

- **Date:** 2026-10-02
- **Source:** Direct: James, decision of 2026-10-02 (`scatter-log`)
- **Change:** amends §3 (rule 2) and §4.2, and Q7
- **Summary:** `LogStore`'s methods return `Send` futures so that the Postgres backend can run an append inside the transaction that also applies projections (0013 §7), on a handle borrowing that transaction. `scatter-log` still depends on no runtime; its file backend does synchronous I/O inside the futures, and synchronous callers use any executor. ID allocation stays outside the trait: the write path fills revid, logid and page ID into the `Draft` from its own sequences. The `postgres` blocking wrapper of Q7 is no longer how a blocking API is offered, since it cannot share the async driver's transaction.

Replaced text (rule 2): "`scatter-log`'s file backend does synchronous I/O and nothing else. Other runtimes can then call the core without bridging."

### A57. `scatter-log-postgres` implements `CheckpointStore`

- **Date:** 2026-10-02
- **Source:** Direct: James, decision of 2026-10-02 (`scatter-log-postgres`)
- **Change:** amends §2
- **Summary:** Checkpoints and segment manifests are rows in the `log` schema (0013 §2), so the crate that owns that schema implements `scatter-integrity`'s `CheckpointStore` over them, and its row gains `scatter-integrity` as a dependency. Its row also names `log.merkle_node` and the per-tenant sequences, which 0013 §2 and §6 already placed in this schema.

Replaced text (§2, the `scatter-log-postgres` row's dependencies): "`scatter-log`, `tokio-postgres`".

### A58. `scatter-ingest`'s store trait and dependencies

- **Date:** 2026-10-03
- **Source:** Direct: James, implementation of 2026-10-03 (`scatter-ingest`)
- **Change:** amends §2
- **Summary:** `scatter-ingest` stays generic over storage: it drives a `LogStore`-shaped append and a projection `Backend` through one `IngestStore` trait, whose Postgres implementation lives in the surfaces (`triplespace-api-ingest`, which the CLI uses too) and whose in-memory implementation lives in the crate for tests. Its row gains `scatter-wikibase-model` (the hash guard needs the `Hasher`; floors and page IDs need entity types) and `scatter-providers` (a mirror operation's partition and provider-ranged revision ID come from the ID's provider), both substrate crates it reached only transitively before.

Replaced text (§2, the `scatter-ingest` row's dependencies): "`scatter-log`, `scatter-projection`, `scatter-wikibase-changeset`, `scatter-filter`".

### A59. `triplespace-accounts` builds the binding draft; the read module of `triplespace-projections`

- **Date:** 2026-10-03
- **Source:** Direct: James, decision of 2026-10-03 (`triplespace-accounts`, `triplespace-api-action`, `triplespace-server`, `scatter-adapter-internetdomains`)
- **Change:** extends §2
- **Summary:** `triplespace-accounts` depends on `scatter-log` as well, for the `scatter:v0/binding` draft a password setting pairs with (the caller appends it); its contents name the `private.session` store, token derivation and request resolution. `triplespace-projections` gains a read module (`read::current`) that assembles an entity's contributions, resolved view, page ID, latest revision and local offset for the API, so the Action API reads through the same code that projects.

### A60. The web tier

- **Date:** 2026-10-03
- **Source:** 0057 §1, §2, §10, §12, §13
- **Change:** amends §2, Consequences; extends §3, §7
- **Summary:** `triplespace-client` (the site's API client, its two transports and its response cache) and `triplespace-web` (the stateless web tier) are added. `triplespace-ui` gains the page handlers, special pages, the landing page, the routing table and `action=render`, and depends on `triplespace-client` and the pure crates it renders from instead of reaching any store. `triplespace-server` serves the site through it under `server.ui = embedded`, and judges forwarded headers by the trust walk of 0057 §10, by address or forwarder key. `triplespace-accounts` keeps forwarder key hashes and, without a shared cache, OAuth pending state; `triplespace-oauth` gives up the consent page for request handles; `triplespace-cli` manages forwarder keys. Rule 10 keeps the site's crates away from every store, and build step 12 places them. The crate count in the Consequences is brought up to date: it was already stale at fifty-four before these two.

Replaced text: §2, `triplespace-ui` of this ADR; §2, `triplespace-server` of this ADR.

Replaced text: §2, `triplespace-oauth` of this ADR; Consequences of this ADR.

### A61. Packed record storage

- **Date:** 2026-10-04
- **Source:** 0058 §1, §12
- **Change:** extends §2; amends §3
- **Summary:** `scatter-log` gains the stored-form codec and embeds `docs/registry/fragments.toml`; `scatter-log-postgres` gains packed storage (fragments, dictionaries, sweeps, the fragment cache) and depends on `zstd` and `hmac`; `triplespace-cli` gains the `storage` commands. Rule 9 distinguishes a change to what log records are, which is a format version, from a change to how they are stored, which keeps every logical byte and needs only a migration.

Replaced text: §3, rule 9 of this ADR.

### A62. The query service

- **Date:** 2026-10-04
- **Source:** 0059 §1, §8
- **Change:** extends §2; amends §4.4
- **Summary:** `triplespace-query` is new: the `QueryService` trait with embedded and remote backends, dataset computation, the prefix set, the query envelope and the cursor marker. `scatter-quadstore` gains `query(sparql, dataset)` behind its feature; `triplespace-projections` feeds the embedded store; the API crates gain `/sparql`; `triplespace-ui` gains `Special:Query`; the binaries gain the `query` commands, settings and rate class. §4.4's "nothing in the API reads from it" becomes "nothing reads from it but the query service".

Replaced text: §4.4, in part of this ADR.

### A63. Scopes

- **Date:** 2026-10-04
- **Source:** 0060 §11
- **Change:** extends §2, §3
- **Summary:** `scatter-scope` is new, pure and wasm: the `triplespace-scope` definition and its compilation. The text is in git (`tools/adr.py log 0005 A63 --diff`).

### A64. Sprints and tasks

- **Date:** 2026-10-04
- **Source:** 0061 §12
- **Change:** extends §2, §3
- **Summary:** `scatter-tasks` is new, pure and wasm: the sprint definition, the `scatter:v0/task` payload and the rule predicates. `triplespace-projections` gains the sprint and task tables and projection; the API crates the sprint and claim routes; `triplespace-ui` the sprint page and `Special:CreateSprint`; `triplespace-notify` the `task-resolved` reason. Rule 2's pure list gains `scatter-tasks`.

### A65. Workspaces

- **Date:** 2026-10-05
- **Source:** 0062 §10
- **Change:** extends §2
- **Summary:** Block parser functions and the `scoped` manifest kind in `scatter-wikitext-expand`; their data and the refresh enqueue in `triplespace-render`; the scope builder and `Special:CreateWorkspace` in `triplespace-ui`; the preview, facet, workspace and kit routes; the by-example and facet queries in `triplespace-query`; default kits written by `instance create`.

### A66. The Query namespace

- **Date:** 2026-10-05
- **Source:** 0063 §8
- **Change:** extends §2
- **Summary:** The `triplespace-sparql` definition and parameter binding in `scatter-scope`; the `query` rule; `#query`; running a definition in `triplespace-query`; the query routes and page.

### A67. EntitySchema and validation

- **Date:** 2026-10-05
- **Source:** 0064 §9
- **Change:** extends §2, §3
- **Summary:** `scatter-shex` is new and pure (rule 2); the `entityschema` entity type; the `schemas` key, `conforms` kind, `schema-violation` rule and `#conformance`; `view.schema_report` and its jobs; the EntitySchema modules, routes and pages.

### A68. MediaInfo captions and Commons

- **Date:** 2026-10-05
- **Source:** 0065 §5
- **Change:** extends §2
- **Summary:** The `terms` operation on File change sets; per-type provider source fields; `M` entities from Commons in the Wikidata adapter; `M`/`WDM` terms and the foreign-file view in the projections; the term modules on `M`; captions and the `thumb` column in the UI.

### A69. Lexemes

- **Date:** 2026-10-05
- **Source:** 0066 §10
- **Change:** extends §2
- **Summary:** The `lexeme` entity type, part IDs and the three data types in the model; part paths in change sets; OntoLex in the RDF crate; lexeme documents in the adapter; constraints; term kinds 4–6 in the projections; search types; the WikibaseLexeme modules; the lexeme page and special pages; `mw.wikibase.lexeme`.

### A70. Proposals

- **Date:** 2026-10-05
- **Source:** 0067 §9
- **Change:** extends §2
- **Summary:** Compilation to upstream change sets in `scatter-wikibase-changeset`; the three thread operations; `view.proposal` and retirement; the upstream OAuth client in `triplespace-accounts`; the push in `triplespace-upstream`; routes, UI and the notification reason.

### A71. Merging with upstream

- **Date:** 2026-10-05
- **Source:** 0068 §7
- **Change:** extends §2, §3
- **Summary:** `scatter-merge` is new, pure and wasm (rule 2); the `merge` field in `scatter-pages`; previews, dependency updates, adoption and re-follow in `triplespace-repos`; routes and `Special:MergeUpstream`.

### A72. Synchronized talk pages

- **Date:** 2026-10-06
- **Source:** 0069 §12
- **Change:** amends §2
- **Summary:** No new crate. The text is in git (`tools/adr.py log 0005 A72 --diff`).

Replaced text: §2, `triplespace-upstream` dependencies of this ADR.

### A73. Shallow entity mirroring

- **Date:** 2026-10-07
- **Source:** 0070 §11
- **Change:** extends §2
- **Summary:** The shallow mirror job in `scatter-ingest`; fetching by ID, the entity event filter and the closure walk in `scatter-adapter-wikidata`; enqueuing from `entity_ref` and term languages in `triplespace-projections`; shared event cursors in `triplespace-repos`; the label cache in `triplespace-cache`; the `mirror` commands.

### A74. Derived statements

- **Date:** 2026-10-07
- **Source:** 0071 §14
- **Change:** extends §2
- **Summary:** `triplespace-extraction` is new; the derived graph and `scatter:v0/derivation` in `scatter-log`; derived contributions in `scatter-wikibase-resolve` and the entity projection; the derivation tables; the extraction routes.

### A75. Template mappings

- **Date:** 2026-10-07
- **Source:** 0072 §7
- **Change:** extends §2
- **Summary:** `scatter-extract` is new, pure and wasm, and takes its configuration as data; the scan API for template calls in `scatter-wikitext`; the `templates` extractor and its config kinds in `triplespace-extraction`; `extraction draft`.

### A76. Lines, links and URL patterns

- **Date:** 2026-10-07
- **Source:** 0073 §7
- **Change:** extends §2
- **Summary:** Lines, table rows, headings, pseudo-headings and links in `scatter-wikitext`'s scan API; tracking-parameter removal in `scatter-normalize`; line rules and URL match patterns in `scatter-extract`; the `lines` extractor in `triplespace-extraction`.

### A77. Publications

- **Date:** 2026-10-07
- **Source:** 0074 §9
- **Change:** extends §2
- **Summary:** `triplespace-publish` is new; edits as a publication's account in `triplespace-upstream`; `private.publication_credential` in `triplespace-accounts`; routes and commands.

### A78. The MCP server

- **Date:** 2026-10-07
- **Source:** 0075 §7
- **Change:** extends §2
- **Summary:** `triplespace-mcp` is new; `triplespace-server` mounts `/mcp` per tenant.

### A79. Dataset publication

- **Date:** 2026-10-07
- **Source:** 0076 §7
- **Change:** extends §2
- **Summary:** The schema.org profile in `scatter-wikibase-rdf`; scope dumps, the dataset description and the sitemap in `triplespace-rdf`; the referrer, dataset and by-URL routes; `dump run`.

### A80. Entity sources

- **Date:** 2026-10-08
- **Source:** 0078 §12
- **Change:** extends §2
- **Summary:** The `entity-source` entry, its number range and IRI rewriting in `scatter-providers`; source IDs and provider-slug input in `scatter-wikibase-model`; fetching from a source's `api` in `scatter-adapter-wikidata`; the per-source fetch job and adoption's source declarations in `scatter-ingest`; the IRI rewrite in `scatter-adapter-triplespace` and `triplespace-projections`; source namespaces and the tenant range in `triplespace-titles`; routes and commands. No new crate.

### A81. The instance key signs checkpoints and manifests; an actor may sign its record

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §4.3
- **Summary:** "The server is the only signer" becomes: the instance key is the only signer of checkpoints and manifests; an actor may additionally sign its own record with the client `signature` of 0015 §1. 0006 §1 is amended the same way. The consequence that said Triplespace does not yet use the client-signature room is struck. (PENDING A2)

Replaced text: §4.3 of this ADR.

### A82. Rule 7 is the one wasm list

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §3
- **Summary:** Rule 7 is the one list of crates that must build for `wasm32-unknown-unknown`: the nine crates the table of §2 marks as building for `wasm32` (`scatter-merge`, `scatter-normalize`, `scatter-pages`, `scatter-wikitext`, `scatter-extract`, `scatter-css`, `scatter-wikibase-shape`, `scatter-scope`, `scatter-tasks`), not the four the rule named. 0034 §6 and `cargo xtask wasm` read rule 7 rather than keeping lists of their own. (PENDING F23)

Replaced text: §3, rule 7 of this ADR.

### A83. The pure list gains three crates

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §3
- **Summary:** Rule 2's list of pure crates gains `scatter-files`, `scatter-extract` and `scatter-vocab`, which the table of §2 already describes as pure. (PENDING F24)

Replaced text: §3, rule 2 of this ADR.

### A84. Crate clauses the table lacked

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §2
- **Summary:** The crate map folds in the clauses it lacked from the crate sections of 0056 §16, 0064 §9, 0057 §12, 0054 §12, 0055 §9, 0069 §12, 0067 §9, 0068 §7, 0037 §8 and 0036 §7; chapter 22 §2.2 lists them. No crate is added. (PENDING F26)

### A85. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** 0050 §14
- **Change:** relocates §1–§7

### A86. Derived issuer codes

- **Date:** 2026-10-09
- **Source:** 0079 §8
- **Change:** extends §2
- **Summary:** `scatter-actors` derives tenant issuer codes and the farm code from a founding record's leaf hash, keys `IssuerRegistry::for_tenant` and `for_farm` by code, and refuses a registered issuer code longer than 25 characters. Chapter text: 22 §2.1.

### A87. Tenant sources

- **Date:** 2026-10-09
- **Source:** 0080 §9
- **Change:** extends §2
- **Summary:** `scatter-providers` gains the `tenant` and `base` fields of an `entity-source`, `ts-source-is-provider` by issuer, `ts-source-private` and promotion matched by issuer code; `triplespace-projections` reads a tenant source on the same instance directly; `scatter-adapter-triplespace` writes a tenant source into `source/{name}` after checking the provider's founding record against the source's code.

### A88. Recovery keys and witnessing

- **Date:** 2026-10-09
- **Source:** 0081 §10
- **Change:** extends §2
- **Summary:** `scatter-log` gains the three chain-of-custody kinds; `scatter-integrity` recovery signatures, continuations, witness submission, fixed origins and the chain of custody in `verify`; `scatter-adapter-triplespace` acting on continuations; `triplespace-server` the tlog-witness endpoint and `triplespace-cli` the recovery-key, recovery-signing, continuing import and cancel commands.

### A89. Source form and the shared view

- **Date:** 2026-10-09
- **Source:** 0082 §7
- **Change:** extends §2
- **Summary:** The crates take on 0082's source form: no crate rewrites an entity ID in stored or hashed JSON, and the API crates rewrite IDs to the consumer's preferred form after the cache. No new crate. The rows are in git (`tools/adr.py log 0005 A89 --diff`). (REVIEW G1–G7)

### A90. The write path in three tiers

- **Date:** 2026-10-09
- **Source:** 0083 §1, §8
- **Change:** amends §4.2; extends §2
- **Summary:** An interactive write in Postgres appends and does the rest of the appending transaction (the base check, ID allocation, the written graph's state and the activity row) in one transaction on the async driver; it no longer "applies its projections" there, since composition and everything derived from it run after commit, and only the step-1 projections the write path reads stay inline (§4.2, at 01 §1.3). `scatter-ingest` is tier 1, with the bulk-append mode and block coordinator and the `graph_state` and `entity_revision` writes; `scatter-wikibase-resolve` composes as a pure function with `composed_from`, coalescing and a set-based form; `scatter-projection` holds the projection classes (log-replayed, view-derived), the side-effect-free rule, rebuild by swap, the cache epoch and the replay order; `triplespace-projections` runs the composition worker, compose-on-miss, the tier-3 consumers and their queues, `view.scope_trigger`, `from_render` on the page tables, the single-sequencer delta consumer and per-tenant epochs; the API crates compose the written entity inline for the write response and serve the per-partition activity resume token; `triplespace-db` gains `view.graph_state`, `view.entity_revision`, `view.scope_trigger`, `ops.tenant_epoch`, the `rdf_delta` coordinate index and the `composed_from` and `policy` columns. (REVIEW G8–G15)

Replaced text: 01 §1.3.

### A91. Wikibase writes against the resolved view

- **Date:** 2026-10-09
- **Source:** 0084 §6
- **Change:** extends §2
- **Summary:** `scatter-wikibase-changeset` diffs a submission against a resolved view and maps it to operations by ownership, refusing a value change on a mirror-owned statement with `ts-foreign-statement`; `scatter-ingest` performs the per-graph base check and the patch rule; `triplespace-api-action` carries the `wb*` modules over the diff, `wikibase-conflict-patched` and inbound `maxlag`, `triplespace-api-rest` the same for the REST write routes; `triplespace-ui` offers suppress-and-add as one action on a refused value change. (REVIEW G16, G17)

### A92. `scatter-titles`, `scatter-extract` in layer 2, payload types in their crates, the unit of work and the wasm closure

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** corrects §2, §3, §4; extends §7
- **Summary:** `scatter-extract` is a layer-2 crate, since it produces `scatter-wikibase-model`'s types. A `scatter:v0/*` payload type or config kind lives in the lowest crate whose product needs it (rule 11): `scatter-log` defines the `config` payload type and the kinds the log cannot start without, `scatter-pages` the page records, and a payload type or config kind that only Triplespace writes is declared by the Triplespace crate that writes it, so `scatter:v0/mirrored-page` and the `page-repo` kind are `triplespace-repos`'s, `scatter:v0/derivation` is `triplespace-extraction`'s, and the `tenancy`, `template` and `provider-readers` kinds are `triplespace-accounts`'s. `scatter-titles`, a new layer-1 pure crate, holds the namespace and special-page registries, title normalization and alias resolution, everything of a title that is a function of the registry and the string, and builds for `wasm32`, so that `triplespace-ui` and `scatter-wasm` can take it; `triplespace-titles` keeps the DB-backed resolver. The build order (§7) adds `scatter-titles` to step 1 and puts `triplespace-titles` over it in step 6. The ledger named §1 (the four layers); the layer move is a row of the table, and the chapter's rules section (22 §1.2) is §3 here, so the ledger row was corrected to §2, §3, §4 and §7. Chapter text: 22 §1.2, §1.3, §2.1. (REVIEW G51)

Replaced text: 22 §1.2.

Replaced text: 22 §1.3, §2.1.

### A93. The projection order is a partial order; Postgres before the file backend

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §7; extends §2
- **Summary:** The projection order is a partial order over the projections a build registers, with `ops.projection_state` rows for those only, so that a build without a projection has no row for it and nothing waits on it, and a later projection is added by registering it with its class and the projections it reads after; `scatter-projection` and `triplespace-projections` carry this. The build order names the first milestone's crate and projection set (the API-only single-tenant instance the three client libraries are tested against), and step 2 builds `scatter-log`'s formats, tree and trait with an in-memory backend, then `scatter-integrity`, then `scatter-log-postgres` and the conformance suite, and the `segments` file backend last against the same suite: Postgres is the backend the milestone serves from. The ledger named §3; the fold is at 22 §3.1 (§7 here) and in the table (§2), and the ledger row was corrected. (REVIEW G52)

Replaced text: 22 §3.1, §2.1.

### A94. No genesis record; several processes append

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §4.2
- **Summary:** A partition has no genesis record: its name, hash function, `k` and policies are its `graph:` record in the `config` partition, which the export bundle carries, and its first record is an ordinary record at offset 0. Several processes append to one partition: an appending transaction reads the tree's right edge under the partition's append lock and persists only the complete subtrees its append closes, rather than keeping the right edge in memory. Both are stated at 01 §1.2 and §4.1, whose provenance includes §4.2 here beside 0006 §4 and §5, where the decisions are logged in full (0006 A24, A25); the ledger rows were extended to name §4.2. (REVIEW G24, G25)

Replaced text: 01 §1.2, §4.1.

### A95. Crates for settings and preferences

- **Date:** 2026-10-17
- **Source:** 0085 §9
- **Change:** extends §2
- **Summary:** The crates take on 0085's settings and preferences; no new crate. Chapter text: 22 §2.1; the rows are in git (`tools/adr.py log 0005 A95 --diff`).

### A96. Crates for theming

- **Date:** 2026-10-17
- **Source:** 0086 §7
- **Change:** extends §2
- **Summary:** `triplespace-accounts` holds the `palette` and `typeset` config kinds, their validation, `extends` and contrast floors, and `follows = choice`; `triplespace-ui` builds a stylesheet per palette, with its variants, and per type set, with its `@font-face` rules and text-size modes, writes the reader's choices onto `<html>`, and honours the content classes of night mode; `scatter-css` keeps `html.skin-theme-clientpref-*` compounds before its prefix; `triplespace-api-action` reports palettes, type sets and the theme settings. The chapter rows are 22 §2.1.

### A97. The `scatter-challenge` crate

- **Date:** 2026-10-17
- **Source:** 0087 §10
- **Change:** extends §2
- **Summary:** A new pure crate, `scatter-challenge`, layer 1, building for `wasm32`, holds the proof-of-work challenge: the challenge string, its MAC given a key and a binding, the SHA-256 work function, the solver and the test vectors, shared with Scatterbase. The chapter text is 22 §2.1 and §2.2.

### A98. The challenge solver is not a WebAssembly module

- **Date:** 2026-10-18
- **Source:** 0087 A1
- **Change:** amends §2
- **Summary:** The browser solves a registration challenge with JavaScript in `triplespace-ui`, held to `scatter-challenge`'s test vectors, so `scatter-wasm` gains no module for it and no longer depends on `scatter-challenge`; `scatter-challenge` keeps the native solver. Compiling the crate for the browser is 0087 Q5. `triplespace-db` and `triplespace-accounts` gain `private.rate_event` (0013 A65). The chapter text is 22 §2.1 and §2.2.

Replaced text: 22 §2.1.

Replaced text: 22 §2.1.

### A99. The OAuth client for login

- **Date:** 2026-10-18
- **Source:** 0088 §10
- **Change:** extends §2
- **Summary:** `triplespace-accounts` gains the OAuth client for login through external issuers: flows, PKCE and `state`, the exchange and profile call, subject normalization, `private.oauth_client` sealed under the instance sealing key, `private.binding`, and vouching with the `reclaim` record. XChaCha20-Poly1305 (`chacha20poly1305`) joins the cryptography table. The chapter text is 22 §2.1 and §4.4.

### A100. The auth host and instance logins

- **Date:** 2026-10-18
- **Source:** 0089 §17
- **Change:** extends §2
- **Summary:** `triplespace-accounts` gains instance logins: their records and names, passwords and external bindings keyed by login, tenant and farm accounts' bindings to logins, the authorization route's handles and login codes, central sessions and logging out everywhere, and the migration. The chapter text is 22 §2.1.

### A101. `scatter-talk`

- **Date:** 2026-10-20
- **Source:** 0090 §12
- **Change:** amends §2
- **Summary:** A new pure crate, `scatter-talk`, holds the wikitext talk-page splitter (moved from `scatter-adapter-mediawiki`), the line-level blame and attribution, and DiscussionTools names and IDs, and embeds `talk-markup.toml`; it depends on `scatter-wikitext`. Chapter text: 22 §1.2, §2.1.

Replaced text: 22 §1.2.

Replaced text: 22 §2.1.

### A102. Closing and collapsing posts in the talk-page crates

- **Date:** 2026-10-21
- **Source:** 0090 A1
- **Change:** amends §2
- **Summary:** With wrapper templates converted into closing and collapsing posts rather than wrapper posts, `scatter-threads` gains `collapse` and `uncollapse` on any post and the `closed` status in place of wrapper posts; `scatter-talk` writes heading, closing and collapsing posts; `triplespace-ui` renders collapsed ranges. Chapter text: 22 §2.1.

Replaced text: 22 §2.1.

### A103. `scatter-talk-import`

- **Date:** 2026-10-22
- **Source:** 0090 A3
- **Change:** extends §2
- **Summary:** A new pure crate, `scatter-talk-import`, plans the thread records of a converted talk page for every import job, mapping `scatter-talk`'s conversion onto `scatter-threads`' operations; it depends on both, and on `sha2` for the section name hash. Chapter text: 22 §2.1.

### A104. `triplespace-notify` implements a projection

- **Date:** 2026-10-23
- **Source:** 0021 §1
- **Change:** corrects §2
- **Summary:** 0021 §1 makes addressing a projection, which `triplespace-notify` holds, so the crate implements `scatter-projection`'s trait over records it decodes with `scatter-log`; its row omitted both. The decision is unchanged. Chapter text: 22 §2.1.

Replaced text: 22 §2.1, `triplespace-notify`'s *Depends on*.

### A105. `scatter-adapter-wikibase`

- **Date:** 2026-10-26
- **Source:** 0091 §5
- **Change:** amends §2, §7
- **Summary:** `scatter-adapter-wikidata` is renamed `scatter-adapter-wikibase` and gains the key-mapped adapter, with a dependency on `scatter-normalize`; `scatter-adapter-internetdomains` is retired into it. Chapter text: 22 §2.1, §2.2, §3.1.

Replaced text: 22 §2.1, the row's name and opening of this ADR; 22 §2.1, the end of the row and its *Depends on* of this ADR; 22 §2.1, the retired row of this ADR; 22 §3.1 of this ADR.

### A106. The recent-changes follower

- **Date:** 2026-10-26
- **Source:** 0092 §8
- **Change:** amends §2
- **Summary:** The crates take on 0092's recent-changes follower; no new crate and no new edge. The `triplespace-upstream` row also stops naming `scatter-adapter-wikidata`, which 0091 §5 had renamed. Chapter text: 22 §2.1, §2.2; the rows are in git (`tools/adr.py log 0005 A106 --diff`).

Replaced text: 22 §2.1, `triplespace-upstream` of this ADR.

### A107. The talk-page conversion runs in `triplespace-cli`, not `scatter-ingest`

- **Date:** 2026-10-26
- **Source:** Direct: James, go-ahead of 2026-10-26 to convert adopted talk pages
- **Change:** corrects §2
- **Summary:** 0090 §10 has an adopted wiki's talk pages converted "within the adoption job". The thread writer is `triplespace-api-ingest`'s, a surface crate above the ingester, so `scatter-ingest` cannot call it (22 §1.2 rule 1): the conversion runs in `triplespace-cli`'s `adopt`, after the adoption job has written every subject page and account, in the same command and attributed to the same job. The `scatter-ingest` row said the ingester converts them; the `triplespace-cli` row now says what it does. The decision is unchanged. Chapter text: 22 §2.1.

Replaced text: 22 §2.1, `scatter-ingest` of this ADR.

### A108. The Interface namespace

- **Date:** 2026-10-28
- **Source:** 0095 §6
- **Change:** extends §2
- **Summary:** Five crates change for the Interface namespace (0095 §6): `scatter-titles` reads a namespace's registry local name; `triplespace-projections` reads a wiki's Interface pages anyone may read; `triplespace-api-action` checks `editinterface` and reports the overrides; `triplespace-client` reads them, the issuer code and the farm code; `triplespace-ui` applies them, names IDs and marks accounts and the Server Operator.

### A109. Adopting any MediaWiki

- **Date:** 2026-10-29
- **Source:** 0096 §6
- **Change:** extends §2
- **Summary:** `triplespace-cli adopt` adopts a source without Wikibase, registers the source's namespaces as tenant `namespace` records, and renames or archives one whose name is taken with `--namespace`, after the survey lists the conflicts (0096 §6). The Help namespace and the extension chips are registry data; the conversion of SMW and Cargo data is a script in `tools/`, not a crate.

### A110. Skins, versions and releases

- **Date:** 2026-10-29
- **Source:** 0097 §3, §17
- **Change:** amends §2; extends §6
- **Summary:** `triplespace-ui` renders through operator-installed skins: a skin loader with a requirement check, MiniJinja with the template functions, the menus and the link classification, and the view models every skin draws; `askama` and the Codex builder remain for the built-in rendering only until the default skin reaches parity, and go then (0097 §16). It gains `minijinja`, `semver` and `html5ever`. `triplespace-web` and `triplespace-server` embed the default skin and read a skin directory and the `renderer` switch; `triplespace-client` checks `api_version` as a semantic version; `triplespace-api-action` reports it and the contract versions; `triplespace-accounts` holds `skins.available`, `skins.default` and `ui.skin`. Each `scatter-*` crate is versioned on its own, checked by `cargo-semver-checks` once published; the other crates are `publish = false` and carry the product version. The chapter text is 22 §2.1, §3.4 and §3.5.

Replaced text: 22 §2.1.

### A111. The crate map is a registry file

- **Date:** 2026-10-30
- **Source:** 0098 §5
- **Change:** consolidates §2, §3
- **Summary:** The table of 22 §2.1 moved, cell for cell, into `docs/registry/crates.toml` (one `[[crate]]` entry per row: names, layer, a one-line summary, `depends`, `depends_any`, `depends_note`, `pure`, `wasm`, and the full `contents`); the chapter's table is now generated from it by `architecture/crates.py` and shows the summary, and `architecture/CRATES.md`, generated likewise, shows every entry in full. `cargo xtask deps` and `xtask wasm` read the file instead of the chapter, and the `pure` and `wasm` lists that were hard-coded in `xtask` and restated in 22 §1.2 rules 2 and 7 are the file's marks; the two rules now name the marks. Four cells that 0095 A3 had folded into the Crate column (`scatter-titles`, `triplespace-projections`, `triplespace-ui`, `triplespace-client`) were moved to those crates' contents. No decision changed. The last commit in which the chapter carried the table is `0e67cce`.

Replaced text: 22 §1.2 rule 2, §1.2 rule 7.

### A112. The map's history lives in its entries

- **Date:** 2026-10-30
- **Source:** 0098 §5; Direct: James, review of 2026-10-30
- **Change:** consolidates §2
- **Summary:** 22 §2.2, the narrative of which ADR introduced which crate, is retired: a chapter carries no history (24 §10.1), and every fact in it is carried by the entries of `crates.toml`, each of which cites the ADR sections that added or changed it, and by those ADRs' logs. Its citations moved to 22 §2.1's provenance line and the pointers of the ADRs it cited now name §2.1 alone. The retired crate names (`scatter-graphs`, `scatter-markdown`, `scatter-keyed`, `triplespace-activity`, `triplespace-revmeta`, `scatter-adapter-wikidata`, `scatter-adapter-internetdomains`) moved from a sentence in 22 §2.1 into `[[retired]]` entries of `crates.toml`, from which `crates.py` writes the sentence and `check_adrs.py` reads the names. No decision changed. The last commit before this entry is `928613b`.

Replaced text: 22 §2.1, §2.2.

### A113. `scatter-mail`

- **Date:** 2026-11-05
- **Source:** 0100 §11
- **Change:** extends §2
- **Summary:** A new substrate crate, `scatter-mail`, holds the message type, the `MailTransport` trait and the `log` and `smtp` transports, over `lettre`; `triplespace-notify` depends on it and stages and sends every outgoing email. `triplespace-accounts` gains a login's email addresses and their verification codes, and `triplespace-server` the delivery worker and the `/auth/email` routes.

### A114. No commercial licence

- **Date:** 2026-11-07
- **Source:** Direct: James, design discussion of 2026-11-07
- **Change:** amends §6
- **Summary:** James withdrew the plan to offer the `scatter-*` crates under a commercial licence. Code in the repository is GPL-3.0-or-later unless otherwise stated, the shared crates included; what states otherwise is vendored third-party code and assets (their own licences), `docs/` (CC0-1.0) and bindings in other languages (Apache-2.0). With nothing to relicense, contributions are accepted under GPL-3.0-or-later and no contributor licence agreement is asked for (James chose this when asked, the same day): `version.toml`'s `[[contributor]]` loses its `cla` field, and Special:Version drops the commercial sentence and the per-crate mark (0077 A7). The other ADRs whose text gave dual licensing as a reason are amended with it: 0033 A26, 0042 A23, 0043 A10, 0055 A7.

Replaced text: 22 §3.3.

### A115. `scatter-wikibase-rdf` depends on `scatter-normalize`, `oxttl` and `md-5`

- **Date:** 2026-11-07
- **Source:** Direct: Claude, while building `scatter-wikibase-rdf` (SCA-301), 2026-11-07; confirmed by James, 2026-11-08
- **Change:** extends §2
- **Summary:** The crate's row named only `scatter-wikibase-model`, `scatter-vocab` and `scatter-providers`. Building it took three more: `scatter-normalize`, whose keyed-type registry turns a keyed ID into its concept IRI (`https://scatter.red/domain/{key}`, 04 §3.5), as the provider registry does a foreign one; `oxttl`, which 02 §4.1 already names for writing N-Triples and Turtle without a store (0033 §8), added to the workspace at 0.2 to match `oxrdf` 0.3; and `md-5`, for naming blank nodes. All three are pure and build for wasm32. `oxrdf`, which the crate's terms are, is listed too. `triplespace-api-action` uses the crate for `Special:EntityData`'s RDF forms under its `lower` rule, with no change to its row.

### A116. `triplespace-rdf` is built; what it, the API and the CLI depend on

- **Date:** 2026-11-07
- **Source:** Direct: Claude, while building the dumps (SCA-301), 2026-11-07; confirmed by James, 2026-11-08
- **Change:** extends §2
- **Summary:** `triplespace-rdf` is built as the dump writer and depends on `triplespace-projections` (the composed read of each entity and the ACL chain), `scatter-actors` (the read decision), `scatter-wikibase-model`, `scatter-providers`, `scatter-normalize`, `oxrdf`, `flate2`, `sha2` and `tokio-postgres`, beside the edges its row already named; `scatter-mwlog` and `scatter-quadstore` stay named for the metadata graph and the local quad store. `scatter-wikibase-rdf` names its blank nodes by SHA-256 (0032 A13) and drops `md-5` for `sha2`. Three helpers move from `triplespace-api-action` into `triplespace-projections::read` so that `Special:EntityData` and the dumps share them: which statements are named as their upstream names them, keyed entities' derived terms, and ISO dates. `triplespace-api-action` uses `triplespace-rdf` to list dumps and `tokio-util`, already in the lockfile, to stream them; the CLI uses it for `triplespace dump`. All are within the rows' `lower` rules.

### A117. Export policies say which bundle scopes carry a partition

- **Date:** 2026-11-07
- **Source:** 0103 §2
- **Change:** amends §4.1
- **Summary:** `internal` was "served by the API but never exported" and `private` "never projected, placed on a feed or exported", which seemed to forbid a tenant move carrying `internal` partitions and a rehost carrying `private`. Each policy now names the scopes of 0006 §9 that carry it: `public` is exported, transferred and backed up; `internal` is transferred and backed up, never exported; `private` is backed up only, a transfer carrying its portable content, sealed. Chapter text: 02 §1.1.

Replaced text: 02 §1.1.

### A118. `triplespace-rdf` writes bundles too

- **Date:** 2026-11-07
- **Source:** Direct: Claude, while building the public bundle (SCA-302), 2026-11-07; confirmed by James, 2026-11-08
- **Change:** extends §2
- **Summary:** `triplespace-rdf` gains a `bundle` module, the partitions, origins, `key:` records and export-scope reader that `triplespace export` and the dump's public bundle share, and so depends on `scatter-integrity`, `scatter-log` and `scatter-log-postgres`; the reader takes the reader's permissions as a value, so the crate needs no `triplespace-accounts`. 0013 A81 says why the dump writes the bundle.

### A119. `scatter-mediawiki-xml`: MediaWiki functionality that is not particular to Wikibase

- **Date:** 2026-11-08
- **Source:** Direct: James, design discussion of 2026-11-08
- **Change:** extends §2
- **Summary:** Deciding the crate homes for export and import (SCA-298), James chose a new pure crate, `scatter-mediawiki-xml`, in the Wikibase domain layer, to generalize MediaWiki functionality that is not particular to Wikibase: MediaWiki's export XML (schema 0.11) read and written, the reader moving there from `scatter-adapter-wikibase`'s `xml_dump.rs`. The general import job of 0008 §9 lives in `scatter-ingest` and reads through it, so that imported pages go through the load channels every ingest uses and get their identifiers there; the export assembly stays in `triplespace-rdf` beside the dumps, bundles and `records` export (A116, A118; 0047 A31) and writes through it. Three edges: `scatter-adapter-wikibase`, `scatter-ingest` and `triplespace-rdf` depend on `scatter-mediawiki-xml`. Chapter text: 22 §2.1 (`crates.toml`).

### A120. Two edges for the XML export

- **Date:** 2026-11-08
- **Source:** Direct: Claude, while building the MediaWiki XML export (SCA-300), 2026-11-08; confirmed by James, 2026-11-08
- **Change:** extends §2
- **Summary:** `triplespace-rdf` depends on `scatter-pages`, for the content-model registry's formats the XML's `<format>` elements carry; `scatter-mediawiki-xml` depends on `sha1`, for MediaWiki's base-36 SHA-1 of a revision's text. Chapter text: 22 §2.1 (`crates.toml`).

### A121. The import's homes and edges

- **Date:** 2026-11-08
- **Source:** Direct: Claude, while building the MediaWiki XML import (SCA-303), 2026-11-08; confirmed by James, 2026-11-08
- **Change:** extends §2
- **Summary:** The talk-page conversion of 0090 §10 runs in `triplespace-api-ingest` (`talk_import`), not `triplespace-cli` as A107 placed it: still above the ingester (22 §1.2 rule 1), and now shared by `triplespace adopt`, `triplespace import` and `action=import`. `triplespace-api-ingest` also holds the import's reader (a dump page as what the job takes, classed by the adoption rules), the Postgres title index the merge reads, and the census wired to `scatter-wikitext-expand`'s scanner (`import`). `scatter-ingest` takes `scatter-mediawiki-xml` in Cargo for the census's page type, the edge A119 named; `scatter-wikitext-expand` gains the census scan over its registry, no new edge. `triplespace-api-action` depends on `reqwest`, to fetch another wiki's export, and on `scatter-talk` and `scatter-mediawiki-xml`; `triplespace-ui` on `base64`, for the file carried along a confirmation; axum's `multipart` feature serves both. Chapter text: 22 §2.1 (`crates.toml`).

### A122. The job runner's home

- **Date:** 2026-11-08
- **Source:** 0104 §4, §10
- **Change:** extends §2
- **Summary:** `triplespace-server` runs the job runner as background tasks claiming `ops.job_queue`, one per partition; `triplespace-cli` gains `job list`, `start`, `pause` and `cancel`, and both run a job directly where no server is up. No new crate and no new edge. Chapter text: 22 §2.1 (`crates.toml`).

### A123. The observer of permission checks

- **Date:** 2026-11-09
- **Source:** 0105 §2, §3
- **Change:** extends §2
- **Summary:** `scatter-actors` holds the observer of permission checks a process may install (a function called with each permission `Effective` is asked about); `triplespace-server` installs one with `--probe-permissions`, for which `scatter-actors` becomes a normal dependency of `triplespace-server` rather than a dev-dependency; the map's "any crate" for the binaries allows it. No new crate. Chapter text: 22 §2.1 (`crates.toml`).

### A124. The probe in the CLI library

- **Date:** 2026-11-09
- **Source:** 0107 §4
- **Change:** amends §2
- **Summary:** The probe moves into the `triplespace-cli` library (`probe`), which both binaries call: `--probe` replaces `triplespace-server --probe-permissions`. `triplespace-accounts` holds the observer of setting reads and `triplespace-projections` the observer of log events. `scatter-actors` returns to being a dev-dependency of `triplespace-server`, which no longer calls it. No new crate and no new edge. Chapter text: 22 §2.1 (`crates.toml`).

Replaced text (22 §2.1, `crates.toml`, the binaries' entry, as it stood):

> `triplespace-server --probe-permissions`, which reports every permission the server checks ([0105](0105-catalogues-checked-against-the-running-code.md) §2)

### A125. The observers of routes, modules and tenancy switches

- **Date:** 2026-11-09
- **Source:** 0108 §3
- **Change:** extends §2
- **Summary:** `triplespace-api-rest` holds the observer of the REST routes served, `triplespace-api-action` that of the Action API modules served, and `triplespace-accounts` and `scatter-actors` those of tenancy-switch reads; the server installs the API's two with the probe's `module` and `route`. No new crate and no new edge. Chapter text: 22 §2.1 (`crates.toml`).

### A126. The `remote` keyed type

- **Date:** 2026-11-15
- **Source:** 0110 §7
- **Change:** extends §2
- **Summary:** `scatter-normalize` holds the `remote` keyed type's grammar and normalizer and the host registry with its site grammars, embedding `hosts.toml`; `scatter-wikibase-model` holds the entity type, the `wikibase-remote` data type and the link through a host's site list. No new crate and no new edge. Chapter text: 22 §2.1 (`crates.toml`).
