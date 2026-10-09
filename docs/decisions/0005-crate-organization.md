# 0005. Crate organization for reuse by Scatterbase

- **Status:** Proposed
- **Date:** 2026-09-25
- **Updated:** 2026-10-09 (A88)
- **Author:** James Hare / Claude Opus; revision by James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md)
- **Uses:** [0000](0000-init.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)
- **Chapters:** [01](../architecture/01-log-and-records.md), [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [22](../architecture/22-crates-and-stack.md)

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

*Current text: [22](../architecture/22-crates-and-stack.md) §1.1.*

### 2. Crate map

*Changed by A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25, A26, A27, A28, A29, A30, A31, A32, A33, A34, A35, A36, A37, A38, A39, A40, A41, A42, A43, A44, A45, A46, A47, A48, A49, A51, A52, A53, A54, A55, A57, A58, A59, A60, A61, A62, A63, A64, A65, A66, A67, A68, A69, A70, A71, A72, A73, A74, A75, A76, A77, A78, A79, A80, A84, A86, A87, A88.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 3. Rules

*Changed by A8, A10, A33, A42, A43, A54, A56, A60, A61, A63, A64, A67, A71, A82, A83.*

*Current text: [22](../architecture/22-crates-and-stack.md) §1.2.*

### 4. Seams shared with Scatterbase

*Current text: [22](../architecture/22-crates-and-stack.md) §1.3.*

#### 4.1 Named graphs are registry entries

*Changed by A2, A3, A6, A10, A11, A14, A31.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.1, §1.2; [22](../architecture/22-crates-and-stack.md) §1.3.*

#### 4.2 Log partitions carry their own policy

*Changed by A1, A8, A10, A14, A56.*

*Current text: [01](../architecture/01-log-and-records.md) §1.2, §1.3, §4.1, §6; [22](../architecture/22-crates-and-stack.md) §1.3.*

#### 4.3 A record is a header plus a payload

*Changed by A1, A10, A31, A81.*

*Current text: [01](../architecture/01-log-and-records.md) §2.1, §2.3, §2.4; [22](../architecture/22-crates-and-stack.md) §1.3.*

#### 4.4 Storage contracts

*Changed by A8, A28, A62.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §1.3; [22](../architecture/22-crates-and-stack.md) §1.3.*

### 5. One vocabulary under `https://scatter.red/terms/v0/` (amends 0001 §5)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §3.1.*

### 6. Naming, licensing and repository

*Changed by A17, A30.*

*Current text: [22](../architecture/22-crates-and-stack.md) §3.2, §3.3, §3.4.*

### 7. Build order

*Changed by A12, A17, A18, A31, A34, A42, A60.*

*Current text: [22](../architecture/22-crates-and-stack.md) §3.1.*

### 8. Changelog of this revision

*Superseded by the Amendment log.* Each row of the changelog this section held is an entry there, among A1–A49.

## Consequences

- **Scatterbase can adopt the substrate in place of its own.** To do so it has to move from its own term types and `rio` to the `oxrdf` family, and replace its write-only `Storage` trait with `scatter-quadstore` for serving and `LogStore` for the log. Both changes are already on its Phase 1–2 path, and it gains a Postgres backend for the log without asking for one.
- **Scatterbase's planned Wikibase extension mostly exists already.** Its namespaces, data types and Wikidata import come from `scatter-providers`, `scatter-wikibase-*` and `scatter-adapter-wikidata`. What remains is binding them to signed claims.
- **Fifty-nine crates cost more upkeep than twenty-two.** The dependency check in CI is what keeps the table honest; every ADR that adds a crate has to update §2 in the same change. The alternative, extracting crates from a single crate later, is harder than starting split.
- **Shared crates change for two products.** A breaking change in a `scatter-*` crate, in the `scatter:` vocabulary, or in the `log` schema has to work for both.
- ~~**Triplespace carries features it does not use yet.** Room for client signatures in the attestation slot exists in `scatter-log` before any Triplespace code needs it.~~ *Triplespace uses it: an actor may sign its own record with the client `signature` of [0015](0015-record-format-and-partition-registry.md) §1 (A81).*
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

### A57. `scatter-log-postgres` implements `CheckpointStore`

- **Date:** 2026-10-02
- **Source:** Direct: James, decision of 2026-10-02 (`scatter-log-postgres`)
- **Change:** amends §2
- **Summary:** Checkpoints and segment manifests are rows in the `log` schema ([0013](0013-postgres-storage.md) §2), so the crate that owns that schema implements `scatter-integrity`'s `CheckpointStore` over them, and its row gains `scatter-integrity` as a dependency. Its row also names `log.merkle_node` and the per-tenant sequences, which 0013 §2 and §6 already placed in this schema.

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
- **Source:** [0057](0057-web-tier.md) §1, §2, §10, §12, §13
- **Change:** amends §2, Consequences; extends §3, §7
- **Summary:** `triplespace-client` (the site's API client, its two transports and its response cache) and `triplespace-web` (the stateless web tier) are added. `triplespace-ui` gains the page handlers, special pages, the landing page, the routing table and `action=render`, and depends on `triplespace-client` and the pure crates it renders from instead of reaching any store. `triplespace-server` serves the site through it under `server.ui = embedded`, and judges forwarded headers by the trust walk of 0057 §10, by address or forwarder key. `triplespace-accounts` keeps forwarder key hashes and, without a shared cache, OAuth pending state; `triplespace-oauth` gives up the consent page for request handles; `triplespace-cli` manages forwarder keys. Rule 10 keeps the site's crates away from every store, and build step 12 places them. The crate count in the Consequences is brought up to date: it was already stale at fifty-four before these two.

Replaced text (§2, `triplespace-ui`):

> | | `triplespace-ui` | Server rendering: `askama` templates, the builder that emits Codex CSS-only markup, the page frame of [0010](0010-site-ui.md) §2 shared by every page kind, and the pinned Codex assets ([0034](0034-frontend-stack.md) §2) | `scatter-wikibase-shape`, `askama` |

Replaced text (§2, `triplespace-server`):

> Binaries. `triplespace-server` also serves the special pages that `docs/registry/special-pages.toml` marks `served` ([0047](0047-special-pages.md)), including those of [0025](0025-oauth-server.md) §5.

> `triplespace-server` has `server.mode`, `server.trusted_proxies`, `server.admin_listen`, 421 on an unregistered host, the private tenant's landing page and the internal endpoint for Parsoid,

Replaced text (§2, `triplespace-oauth`):

> PKCE, the consent page, the consumer registry

Replaced text (Consequences):

> - **Fifty-four crates cost more upkeep than twenty-two.**

### A61. Packed record storage

- **Date:** 2026-10-04
- **Source:** [0058](0058-packed-record-storage.md) §1, §12
- **Change:** extends §2; amends §3
- **Summary:** `scatter-log` gains the stored-form codec and embeds `docs/registry/fragments.toml`; `scatter-log-postgres` gains packed storage (fragments, dictionaries, sweeps, the fragment cache) and depends on `zstd` and `hmac`; `triplespace-cli` gains the `storage` commands. Rule 9 distinguishes a change to what log records are, which is a format version, from a change to how they are stored, which keeps every logical byte and needs only a migration.

Replaced text (§3, rule 9):

> 9. **Schemas are contracts.** A change to the `log` schema needs a new payload or header version and a migration that keeps every existing header verifiable; a change to `view` needs a migration or a rebuild ([0013](0013-postgres-storage.md), Consequences). Migrations live with the crate that owns the schema.

### A62. The query service

- **Date:** 2026-10-04
- **Source:** [0059](0059-query-service.md) §1, §8
- **Change:** extends §2; amends §4.4
- **Summary:** `triplespace-query` is new: the `QueryService` trait with embedded and remote backends, dataset computation, the prefix set, the query envelope and the cursor marker. `scatter-quadstore` gains `query(sparql, dataset)` behind its feature; `triplespace-projections` feeds the embedded store; the API crates gain `/sparql`; `triplespace-ui` gains `Special:Query`; the binaries gain the `query` commands, settings and rate class. §4.4's "nothing in the API reads from it" becomes "nothing reads from it but the query service".

Replaced text (§4.4, in part):

> and nothing in the API reads from it.

### A63. Scopes

- **Date:** 2026-10-04
- **Source:** [0060](0060-scopes.md) §11
- **Change:** extends §2, §3
- **Summary:** `scatter-scope` is new, pure and wasm: the `triplespace-scope` definition and its compilation. `scatter-wikibase-shape` gains the table `rows` kinds through it; `scatter-threads` gains board definition version 2; `triplespace-projections` gains the scope tables, projection, refresh job, target set and watch expansion; the API crates gain the scope routes; `triplespace-ui` the scope page and notice. Rule 2's pure list gains `scatter-scope`.

### A64. Sprints and tasks

- **Date:** 2026-10-04
- **Source:** [0061](0061-sprints-and-tasks.md) §12
- **Change:** extends §2, §3
- **Summary:** `scatter-tasks` is new, pure and wasm: the sprint definition, the `scatter:v0/task` payload and the rule predicates. `triplespace-projections` gains the sprint and task tables and projection; the API crates the sprint and claim routes; `triplespace-ui` the sprint page and `Special:CreateSprint`; `triplespace-notify` the `task-resolved` reason. Rule 2's pure list gains `scatter-tasks`.

### A65. Workspaces

- **Date:** 2026-10-05
- **Source:** [0062](0062-workspaces.md) §10
- **Change:** extends §2
- **Summary:** Block parser functions and the `scoped` manifest kind in `scatter-wikitext-expand`; their data and the refresh enqueue in `triplespace-render`; the scope builder and `Special:CreateWorkspace` in `triplespace-ui`; the preview, facet, workspace and kit routes; the by-example and facet queries in `triplespace-query`; default kits written by `instance create`.

### A66. The Query namespace

- **Date:** 2026-10-05
- **Source:** [0063](0063-query-namespace.md) §8
- **Change:** extends §2
- **Summary:** The `triplespace-sparql` definition and parameter binding in `scatter-scope`; the `query` rule; `#query`; running a definition in `triplespace-query`; the query routes and page.

### A67. EntitySchema and validation

- **Date:** 2026-10-05
- **Source:** [0064](0064-entityschema-and-validation.md) §9
- **Change:** extends §2, §3
- **Summary:** `scatter-shex` is new and pure (rule 2); the `entityschema` entity type; the `schemas` key, `conforms` kind, `schema-violation` rule and `#conformance`; `view.schema_report` and its jobs; the EntitySchema modules, routes and pages.

### A68. MediaInfo captions and Commons

- **Date:** 2026-10-05
- **Source:** [0065](0065-mediainfo-captions-and-commons.md) §5
- **Change:** extends §2
- **Summary:** The `terms` operation on File change sets; per-type provider source fields; `M` entities from Commons in the Wikidata adapter; `M`/`WDM` terms and the foreign-file view in the projections; the term modules on `M`; captions and the `thumb` column in the UI.

### A69. Lexemes

- **Date:** 2026-10-05
- **Source:** [0066](0066-lexemes.md) §10
- **Change:** extends §2
- **Summary:** The `lexeme` entity type, part IDs and the three data types in the model; part paths in change sets; OntoLex in the RDF crate; lexeme documents in the adapter; constraints; term kinds 4–6 in the projections; search types; the WikibaseLexeme modules; the lexeme page and special pages; `mw.wikibase.lexeme`.

### A70. Proposals

- **Date:** 2026-10-05
- **Source:** [0067](0067-proposals.md) §9
- **Change:** extends §2
- **Summary:** Compilation to upstream change sets in `scatter-wikibase-changeset`; the three thread operations; `view.proposal` and retirement; the upstream OAuth client in `triplespace-accounts`; the push in `triplespace-upstream`; routes, UI and the notification reason.

### A71. Merging with upstream

- **Date:** 2026-10-05
- **Source:** [0068](0068-merging-with-upstream.md) §7
- **Change:** extends §2, §3
- **Summary:** `scatter-merge` is new, pure and wasm (rule 2); the `merge` field in `scatter-pages`; previews, dependency updates, adoption and re-follow in `triplespace-repos`; routes and `Special:MergeUpstream`.

### A72. Synchronized talk pages

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §12
- **Change:** amends §2
- **Summary:** No new crate. Followed talk pages in `scatter-adapter-mediawiki` and `triplespace-repos`; pins in `scatter-threads` and `triplespace-projections`; the `follow` page operation in `scatter-pages`; upstream replies in `triplespace-upstream`, which now depends on `triplespace-db` and `triplespace-accounts` for `ops.upstream_post` and the grant; the grant flow in `triplespace-accounts`; `reply`, `featured` and search changes in their crates.

Replaced text (§2, `triplespace-upstream` dependencies):

> `scatter-adapter-wikidata`, `scatter-mwlog`, `triplespace-cache`

### A73. Shallow entity mirroring

- **Date:** 2026-10-07
- **Source:** [0070](0070-shallow-entity-mirroring.md) §11
- **Change:** extends §2
- **Summary:** The shallow mirror job in `scatter-ingest`; fetching by ID, the entity event filter and the closure walk in `scatter-adapter-wikidata`; enqueuing from `entity_ref` and term languages in `triplespace-projections`; shared event cursors in `triplespace-repos`; the label cache in `triplespace-cache`; the `mirror` commands.

### A74. Derived statements

- **Date:** 2026-10-07
- **Source:** [0071](0071-derived-statements-from-mirrored-pages.md) §14
- **Change:** extends §2
- **Summary:** `triplespace-extraction` is new; the derived graph and `scatter:v0/derivation` in `scatter-log`; derived contributions in `scatter-wikibase-resolve` and the entity projection; the derivation tables; the extraction routes.

### A75. Template mappings

- **Date:** 2026-10-07
- **Source:** [0072](0072-template-mappings.md) §7
- **Change:** extends §2
- **Summary:** `scatter-extract` is new, pure and wasm, and takes its configuration as data; the scan API for template calls in `scatter-wikitext`; the `templates` extractor and its config kinds in `triplespace-extraction`; `extraction draft`.

### A76. Lines, links and URL patterns

- **Date:** 2026-10-07
- **Source:** [0073](0073-lines-links-and-url-patterns.md) §7
- **Change:** extends §2
- **Summary:** Lines, table rows, headings, pseudo-headings and links in `scatter-wikitext`'s scan API; tracking-parameter removal in `scatter-normalize`; line rules and URL match patterns in `scatter-extract`; the `lines` extractor in `triplespace-extraction`.

### A77. Publications

- **Date:** 2026-10-07
- **Source:** [0074](0074-publishing-a-scope-to-an-external-wiki.md) §9
- **Change:** extends §2
- **Summary:** `triplespace-publish` is new; edits as a publication's account in `triplespace-upstream`; `private.publication_credential` in `triplespace-accounts`; routes and commands.

### A78. The MCP server

- **Date:** 2026-10-07
- **Source:** [0075](0075-mcp-server.md) §7
- **Change:** extends §2
- **Summary:** `triplespace-mcp` is new; `triplespace-server` mounts `/mcp` per tenant.

### A79. Dataset publication

- **Date:** 2026-10-07
- **Source:** [0076](0076-dataset-publication.md) §7
- **Change:** extends §2
- **Summary:** The schema.org profile in `scatter-wikibase-rdf`; scope dumps, the dataset description and the sitemap in `triplespace-rdf`; the referrer, dataset and by-URL routes; `dump run`.

### A80. Entity sources

- **Date:** 2026-10-08
- **Source:** [0078](0078-entity-sources.md) §12
- **Change:** extends §2
- **Summary:** The `entity-source` entry, its number range and IRI rewriting in `scatter-providers`; source IDs and provider-slug input in `scatter-wikibase-model`; fetching from a source's `api` in `scatter-adapter-wikidata`; the per-source fetch job and adoption's source declarations in `scatter-ingest`; the IRI rewrite in `scatter-adapter-triplespace` and `triplespace-projections`; source namespaces and the tenant range in `triplespace-titles`; routes and commands. No new crate.

### A81. The instance key signs checkpoints and manifests; an actor may sign its record

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §4.3
- **Summary:** "The server is the only signer" becomes: the instance key is the only signer of checkpoints and manifests; an actor may additionally sign its own record with the client `signature` of [0015](0015-record-format-and-partition-registry.md) §1. [0006](0006-log-integrity-and-erasure.md) §1 is amended the same way. The consequence that said Triplespace does not yet use the client-signature room is struck. (PENDING A2)

Replaced text (§4.3):

> - In Triplespace the parts are content, comment and attestation, and a payload type may declare more: the thread type adds a fourth, the post text ([0019](0019-discussions.md) §4). The content is a change set, a page or thread operation, an actor record, a log event, an upstream revision, a configuration record, a membership, a block, an ACL ([0023](0023-moderation.md) §3), a filter or a filter hit ([0030](0030-edit-filters.md) §5); the attestation holds the actor, the job ([0002](0002-source-graphs-and-mass-ingest.md) §8.3), the change tags ([0030](0030-edit-filters.md) §5) and, for a post that arrived from the fediverse, the signed remote activity as evidence ([0022](0022-federation.md) §8); and the server is the only signer.

### A82. Rule 7 is the one wasm list

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §3
- **Summary:** Rule 7 is the one list of crates that must build for `wasm32-unknown-unknown`: the nine crates the table of §2 marks as building for `wasm32` (`scatter-merge`, `scatter-normalize`, `scatter-pages`, `scatter-wikitext`, `scatter-extract`, `scatter-css`, `scatter-wikibase-shape`, `scatter-scope`, `scatter-tasks`), not the four the rule named. [0034](0034-frontend-stack.md) §6 and `cargo xtask wasm` read rule 7 rather than keeping lists of their own. (PENDING F23)

Replaced text (§3, rule 7):

> 7. **`scatter-wikibase-shape`, `scatter-wikitext`, `scatter-pages` and `scatter-normalize` must build for `wasm32-unknown-unknown`,** so that the statement UI ([0003](0003-statement-ui.md)), the page editor's preview ([0008](0008-namespaces-and-document-pages.md) §11), title normalization in the browser ([0009](0009-keyed-entity-types-and-domain.md) §12) and the classifier audit over a Wikidata dump run the same code as the server.

### A83. The pure list gains three crates

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §3
- **Summary:** Rule 2's list of pure crates gains `scatter-files`, `scatter-extract` and `scatter-vocab`, which the table of §2 already describes as pure. (PENDING F24)

Replaced text (§3, rule 2):

> 2. **The core is pure.** `scatter-providers`, `scatter-identity`, `scatter-normalize`, `scatter-actors`, `scatter-pages`, `scatter-wikitext`, `scatter-threads`, `scatter-activitypub`, `scatter-filter`, `scatter-mwlog`, `scatter-wikitext-expand` ([0042](0042-template-expansion-and-parsoid.md) §4), `scatter-css` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §2), `scatter-scope` ([0060](0060-scopes.md) §11), `scatter-tasks` ([0061](0061-sprints-and-tasks.md) §12), `scatter-shex` ([0064](0064-entityschema-and-validation.md) §9), `scatter-merge` ([0068](0068-merging-with-upstream.md) §7), `triplespace-scribunto` ([0043](0043-lua-modules.md) §14) and every `scatter-wikibase-*` crate perform no I/O, use no async runtime, and depend on no tokio.

### A84. Crate clauses the table lacked

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §2
- **Summary:** The crate map folds in the clauses it lacked from the crate sections of [0056](0056-security-model.md) §16, [0064](0064-entityschema-and-validation.md) §9, [0057](0057-web-tier.md) §12, [0054](0054-forking-a-mirrored-page.md) §12, [0055](0055-templatestyles-templatedata-and-page-properties.md) §9, [0069](0069-synchronized-talk-pages.md) §12, [0067](0067-proposals.md) §9, [0068](0068-merging-with-upstream.md) §7, [0037](0037-gdelt-provider.md) §8 and [0036](0036-openstreetmap-providers.md) §7; chapter 22 §2.2 lists them. No crate is added. (PENDING F26)

### A85. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§7
- **Summary:** The Decision's current text now lives in the architecture chapters [01](../architecture/01-log-and-records.md), [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A86. Derived issuer codes

- **Date:** 2026-10-09
- **Source:** [0079](0079-derived-issuer-codes.md) §8
- **Change:** extends §2
- **Summary:** `scatter-actors` derives tenant issuer codes and the farm code from a founding record's leaf hash, keys `IssuerRegistry::for_tenant` and `for_farm` by code, and refuses a registered issuer code longer than 25 characters. `triplespace-cli`'s `instance create` derives the farm code, and tenant creation each tenant's issuer code, once the founding record is appended.

### A87. Tenant sources

- **Date:** 2026-10-09
- **Source:** [0080](0080-tenants-as-entity-sources.md) §9
- **Change:** extends §2
- **Summary:** `scatter-providers` gains the `tenant` and `base` fields of an `entity-source`, `ts-source-is-provider` by issuer, `ts-source-private` and promotion matched by issuer code; `triplespace-projections` reads a tenant source on the same instance directly; `scatter-adapter-triplespace` writes a tenant source into `source/{name}` after checking the provider's founding record against the source's code.

### A88. Recovery keys and witnessing

- **Date:** 2026-10-09
- **Source:** [0081](0081-recovery-keys-and-continuations.md) §10
- **Change:** extends §2
- **Summary:** `scatter-log` gains the three chain-of-custody kinds; `scatter-integrity` recovery signatures, continuations, witness submission, fixed origins and the chain of custody in `verify`; `scatter-adapter-triplespace` acting on continuations; `triplespace-server` the tlog-witness endpoint and `triplespace-cli` the recovery-key, recovery-signing, continuing import and cancel commands.
