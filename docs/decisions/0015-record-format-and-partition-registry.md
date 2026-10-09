# 0015. Record format and partition registry

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-09 (A45)
- **Author:** James Hare / Claude Fable
- **Changes:** [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md)
- **Uses:** [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0009](0009-keyed-entity-types-and-domain.md), [0016](0016-permissions-and-access-control.md)
- **Chapters:** [01](../architecture/01-log-and-records.md), [02](../architecture/02-graphs-rdf-and-query.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md)

## Context

The cross-reference review of 2026-09-26 found five decisions that touch the record header or the partition registry and were either deferred to "a later ADR" or left implicit. Each one changes the on-disk format if it is made after the first release. [0005](0005-crate-organization.md) §4.2 says as much about partition policies, and [0007](0007-actor-identity.md) says its attestation question "has to be decided before `scatter-log`'s first release". They are:

1. **Erasing an attribution while keeping the content.** [0006](0006-log-integrity-and-erasure.md) §3 commits to the whole body with one hash, so a body is erased whole. [0007](0007-actor-identity.md) notes that a legal demand to cut the tie between an edit and an account, without removing the edit, cannot be met.
2. **Whether global revision and log IDs enter the hashed header.** [0013](0013-postgres-storage.md) §6 assigns them in the appending transaction but stores them outside the header, with a sidecar in the segment file format.
3. **The configuration partition.** [0006](0006-log-integrity-and-erasure.md) §4 and §6 rely on one, for the instance key among other things, but no registry table in [0005](0005-crate-organization.md) §4.1, [0007](0007-actor-identity.md) §8, [0008](0008-namespaces-and-document-pages.md) §4 or [0011](0011-logs.md) §2 registers it.
4. **Backfilled upstream revisions.** [0002](0002-source-graphs-and-mass-ingest.md) §5 and §8.3 backfill the upstream revision history of retained entities; [0010](0010-site-ui.md) §5.4 reads it from the log, [0011](0011-logs.md) §5 redacts it, and [0012](0012-api-requirements.md) §6 serves it. No ADR gives those revisions a payload type, a partition, an IRI or a table.
5. **Graph IRIs**, open since [0001](0001-revision-metadata-rdf.md) and carried by [0002](0002-source-graphs-and-mass-ingest.md) and [0005](0005-crate-organization.md).

Two smaller gaps are closed with them: the document node of a foreign entity on the instance, and who allocates provider codes.

## Decision

### 1. The body is a tree of erasable parts (amends 0006 §3 and §7)

*Changed by A4, A5, A13, A14, A17, A18, A29, A31.*

*Current text: [01](../architecture/01-log-and-records.md) §2.3, §2.4, §5.1, §5.3, §8.*

### 2. Global IDs live in the header (amends 0006 §3 and 0013 §6)

*Changed by A3, A15, A23, A37.*

*Current text: [01](../architecture/01-log-and-records.md) §2.2, §2.5, §5.3, §7.*

### 3. The `config` partition (amends 0005 §4.1; extends 0006 §4 and §6)

*Changed by A3, A4, A5, A6, A8, A9, A10, A11, A12, A13, A16, A19, A20, A21, A23, A24, A26, A27, A28, A30, A31, A32, A33, A34, A35, A36, A37, A39, A40, A41, A42, A44.*

*Current text: [23](../architecture/23-configuration-and-registry.md) §1.1, §1.2, §1.3, §1.4, §2.1, §2.2, §2.3, §2.4, §3.1, §3.2, §3.7, §5.2.*

### 4. Upstream revision records (amends 0002 §8.3 and 0011 §2, §3, §5, §8)

*Changed by A14, A25.*

*Current text: [01](../architecture/01-log-and-records.md) §2.6, §5.4.*

### 5. Graph names and IRIs (settles 0001 Q2, 0002 Q1 and 0005 Q6)

*Changed by A2, A3, A4, A5, A7, A9, A10, A11, A12, A16, A17, A19, A20, A21, A23, A24, A26, A31, A36, A37, A38, A43.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.2, §1.3, §1.4.*

### 6. Document nodes for foreign entities (extends 0001 §1, 0002 §4 and 0009 §6)

*Changed by A14.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §2.1, §5.6.*

### 7. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **The record format is fixed.** With this ADR, [0006](0006-log-integrity-and-erasure.md) and [0013](0013-postgres-storage.md) §2, the header and body layouts have no known pending change. A later change needs a format version.
- **Erasure matches MediaWiki's three bits exactly.** Text, comment and user can each be hidden or erased on their own, so every RevisionDelete case has an answer, and [0011](0011-logs.md) §5 no longer needs redacted copies.
- **Each record carries two more salts and three more header fields.** About 40 bytes on the ~100 of 0006's estimate; for a 100-million-entity mirror, on the order of 4 GB.
- **Permalinks are verifiable.** An inclusion proof for `oldid=N` proves that revision N is the record it claims to be.
- **Rebuild is deterministic.** Page IDs are read from headers, never derived, and [0013](0013-postgres-storage.md) Q7 closes.
- **Configuration has history, integrity and an export.** Every registry change is a signed, ordered record, and `view.registry` is rebuildable like any projection. Administrators change configuration through the API, not files.
- **Scatterbase can adopt §1, §3 and §5 unchanged.** Its claim type declares its own parts; its `server` graph is the `config` partition under another name; its graph names come from the same registry file.
- **Backfilled history grows the provider log.** The audit [0011](0011-logs.md) already calls for should measure revision volume for retained entities, not only log events.
- **Two document nodes per foreign entity.** Consumers of the metadata graph follow `pav:importedFrom` to reach upstream's own record of the entity.

## Open questions

- **Q1.** ~~**Content backfill by default.** Whether retaining an entity should fetch the content of every upstream revision, or only the metadata, given the cost noted in [0002](0002-source-graphs-and-mass-ingest.md) Consequences.~~ *Settled by A13: content by default, from the provider's API for modest sets and from a history dump the operator supplies for large ones; `--history metadata` opts out.*
- **Q2. Whether `site` settings should each be a record,** or one record per settings group.
- **Q3. Revision IDs beyond 2^40 upstream.** No provider is near it; if one ever is, its number is re-registered with a wider split, which is a format version.
- **Q4.** ~~**Reason classes.** `upstream`, `legal` and `privacy` are now used; the full vocabulary and its visibility are settled in [0016](0016-permissions-and-access-control.md) §6.~~ *Settled by [0016](0016-permissions-and-access-control.md) §6: `legal`, `privacy`, `upstream` and `operational`, extensible in `site` configuration; the class and authority reference are visible to `ts-viewerasures` only.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0000](0000-init.md) §3 | §5 | amends | 0000 A2 |
| [0000](0000-init.md) Q4 | §5 | settles | 0000 Q4 |
| [0000](0000-init.md) Q6 | §1–2 | settles | 0000 Q6 |
| [0001](0001-revision-metadata-rdf.md) §1 | §6 | extends | 0001 A7 |
| [0001](0001-revision-metadata-rdf.md) Q2 | §5 | settles | 0001 Q2 |
| [0001](0001-revision-metadata-rdf.md) Q5 | §4 | settles | 0001 Q5 |
| [0002](0002-source-graphs-and-mass-ingest.md) §5, §8.3 | §4 | extends | 0002 A6 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q1 | §5 | settles | 0002 Q1 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q2 | §3 | settles | 0002 Q2 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q5 | §1 | settles | 0002 Q5 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q10 | §4 | settles | 0002 Q10 |
| [0005](0005-crate-organization.md) §2, §3, §4.1, §4.2, §4.3 | §7 | amends | 0005 A10 |
| [0005](0005-crate-organization.md) Q6 | §5 | settles | 0005 Q6 |
| [0006](0006-log-integrity-and-erasure.md) §2, §3, §7, §9 | §1–3 | amends | 0006 A3 |
| [0006](0006-log-integrity-and-erasure.md) §6, §8 | §1–3 | extends | 0006 A3 |
| [0006](0006-log-integrity-and-erasure.md) §1, §2 | §1 | extends | 0006 A5 |
| [0007](0007-actor-identity.md) Q1 | §1 | settles | 0007 Q1 |
| [0007](0007-actor-identity.md) Q5 | §1 | settles | 0007 Q5 |
| [0008](0008-namespaces-and-document-pages.md) Q1 | §2 | settles | 0008 Q1 |
| [0008](0008-namespaces-and-document-pages.md) Q2 | §2 | settles | 0008 Q2 |
| [0010](0010-site-ui.md) §5.2, §12 | §2 | amends | 0010 A3 |
| [0010](0010-site-ui.md) Q3 | §2 | settles | 0010 Q3 |
| [0010](0010-site-ui.md) Q9 | §5 | settles | 0010 Q9 |
| [0011](0011-logs.md) §3, §5 | §1, §4 | amends | 0011 A3 |
| [0011](0011-logs.md) Q1 | §2 | settles | 0011 Q1 |
| [0012](0012-api-requirements.md) §2.1, §4 | §1–2, §4 | amends | 0012 A4 |
| [0012](0012-api-requirements.md) §6 | §1–2, §4 | extends | 0012 A4 |
| [0012](0012-api-requirements.md) Q1 | §2 | settles | 0012 Q1 |
| [0013](0013-postgres-storage.md) §2, §3, §6 | §1–2, §4, §7 | amends | 0013 A2 |
| [0013](0013-postgres-storage.md) §5.6 | §1–2, §4, §7 | extends | 0013 A2 |
| [0013](0013-postgres-storage.md) Q3 | §2 | settles | 0013 Q3 |
| [0013](0013-postgres-storage.md) Q7 | §2 | settles | 0013 Q7 |

## References

- [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md)
- [MediaWiki RevisionDelete](https://www.mediawiki.org/wiki/Manual:RevisionDelete) (the three independent bits)
- [MediaWiki `Special:Redirect`](https://www.mediawiki.org/wiki/Help:Special_pages) (lookup by revision, user, page or log ID)
- [PAV ontology](https://pav-ontology.github.io/pav/) (`pav:importedFrom`, `pav:hasCurrentVersion`)
- Scatterbase decision record (`server` graph, server-key registration claim)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-26
- **Source:** [0005](0005-crate-organization.md) §2, revision of 2026-09-26
- **Change:** supersedes §7
- **Summary:** 0005 §2 became the one crate table CI checks, and carries every change this section listed (0005 A10).

Replaced text (§7):

> | Crate | Change |
> |---|---|
> | `scatter-log` | Body parts and part leaves (§1); header fields 7–9 and the provider-ranged revision ID (§2); the `config` payload type with kinds `key` and `graph` (§3); `erase` with `parts`, and `LogStore::erase_bodies` becomes `erase_parts`; `docs/registry/graphs.toml` embedded as the default registry (§5) |
> | `scatter-log-postgres` | The `page_id` column; `erased` as a bitmask of parts; partial erasure as a body rewrite ([0013](0013-postgres-storage.md) §3) |
> | `scatter-integrity` | `verify` per part (§1) |
> | `scatter-providers` | Provider slugs and numbers; `docs/registry/providers.toml` embedded as the default registry (§5) |
> | `scatter-actors` | `docs/registry/issuers.toml` embedded as the default issuer registry (§5) |
> | `scatter-mwlog` | The `upstream-revision` payload type beside `logevent` (§4) |
> | `scatter-adapter-wikidata` | Emits upstream revision records from history dumps, `prop=revisions` and the stream (§4) |
> | `triplespace-projections` | The `upstream_revision` projection; the registry projection reads `config` (§3, §4) |
> | `triplespace-rdf` | The IRI rules of §4–6 |
>
> No crate is added.

### A2. Providers registered

- **Date:** 2026-09-27
- **Source:** [0017](0017-entity-id-grammar.md) §6
- **Change:** extends §5
- **Summary:** MusicBrainz is registered in `providers.toml` with code `MB`, slug `musicbrainz` and provider number 4, by the allocation rule of §5; later providers follow. The registry file changed, not this text, until the conversion added a sentence.

### A3. Tenants

- **Date:** 2026-09-27
- **Source:** [0018](0018-tenants.md) §2–3, §5
- **Change:** amends §2, §3, §5
- **Summary:** The ID sequences are per tenant, and the provider-ranged revision ID carries a tenant's revisions to other tenants. The config kinds split by instance and tenant scope, and a tenant's `config` carries the instance's key chain. Graph names are per tenant: each tenant has its own source partitions under its base, and the registry is keyed by (tenant, name). The `tenant` and `alias` kinds and the `providers` list had been given rows in place.

Replaced text (§2):

> They are assigned in the appending transaction, before the leaf hash is computed, so they are inside the Merkle tree. The rules of 0013 §6 stand for local records:

Replaced text: not recorded. 0015 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

Replaced text (§5):

> **A graph's IRI is `{base}/graph/{name}`.** `{base}` is the instance's base URI: the origin that serves `/wiki/`, `/w/api.php` and `/entity/`, the same base that [0001](0001-revision-metadata-rdf.md) §5 gives instance data.
>
> **Graph names are fixed by convention.** They are the partition names of [0013](0013-postgres-storage.md) §2, and the registry (§3, kind `graph`) stores both name and IRI:

### A4. Threads

- **Date:** 2026-09-27
- **Source:** [0019](0019-discussions.md) §4, §6
- **Change:** extends §1, §3, §5
- **Summary:** `scatter:v0/thread` declares a fourth part, the post text; §1 had been given the clause in place. With 0019's amendment of 2026-09-27, thread statuses are `config` records of kind `thread-status`, with defaults in `thread-statuses.toml`; both tables had been given rows in place.

### A5. Evidence and federation policy

- **Date:** 2026-09-27
- **Source:** [0022](0022-federation.md) §2, §8, §10
- **Change:** amends §1; extends §3, §5
- **Summary:** The attestation part may carry `evidence`, a signed remote activity. `federation-policy` is a config kind. `providers.toml` gains `trust` and a key-chain URL. The rows and the clause had been written in place.

Replaced text (§1):

> | 2 | **Attestation** | Who is responsible, and how: the actor key, the job ([0006](0006-log-integrity-and-erasure.md) §3), since [0030](0030-edit-filters.md) §5 the change tags, and since [0022](0022-federation.md) §8 an optional `evidence` field holding a signed remote activity | user |

### A6. ACLs are a payload type

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §3
- **Change:** amends §3
- **Summary:** `acl` is the payload type `scatter:v0/acl`, in `config` for graph targets and in the tenant `log` for the rest, not a config kind. The row was removed and the note under the table written in place.

Replaced text: not recorded. 0015 was revised in place before the repository's history begins (commit `1e53c95`, 2026-09-27).

### A7. Grants

- **Date:** 2026-09-27
- **Source:** [0024](0024-subsidiary-accounts.md) §4
- **Change:** extends §5
- **Summary:** `grants.toml` lists the API-key grants. The row had been added in place.

### A8. OAuth consumers

- **Date:** 2026-09-27
- **Source:** [0025](0025-oauth-server.md) §2
- **Change:** extends §3
- **Summary:** The `consumer` and `consumer-policy` kinds. The rows had been added in place.

### A9. Sitelinks

- **Date:** 2026-09-27
- **Source:** [0026](0026-sitelinks.md) §2–3
- **Change:** extends §3, §5
- **Summary:** The `site-alias` and `sitelink-policy` kinds, and `sites.toml`. The rows had been added in place.

### A10. Preferences

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §1, §5
- **Change:** extends §3, §5
- **Summary:** The `view-pin` kind, and `preferences.toml`. The rows had been added in place.

### A11. Tenancy policy

- **Date:** 2026-09-27
- **Source:** [0028](0028-tenancy-policy.md) §1–2, §5, §8
- **Change:** extends §3, §5
- **Summary:** The `tenancy`, `template` and `provider-readers` kinds; `scope` on `group`; `tenancy.toml`; the farm's three instance partitions `actors/{farm}`, `accounts/{farm}` and `log/{farm}`. The rows had been added in place; `scope` on `group` and the farm partitions were recorded only in 0028 until this conversion.

### A12. Resolvers

- **Date:** 2026-09-27
- **Source:** [0029](0029-resolver-namespaces.md) §1
- **Change:** extends §3, §5
- **Summary:** The `resolver` kind, and `resolvers.toml`. The rows had been added in place.

### A13. Change tags

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §5
- **Change:** amends §1; extends §3
- **Summary:** The attestation part carries the change tags. With 0030's amendment of 2026-09-27, a user-defined tag is a `config` record of kind `tag`. The clause and the row had been written in place.

Replaced text: the attestation row as A5 quotes it, before the change tags were added.

### A14. Decisions of 2026-09-27 (evening)

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** extends §1, §4, §6
- **Summary:** By section:
  - §1: Client signatures in the attestation part (decision 4). The attestation map may carry a **`signature`** field: `{key: <key ID>, alg: "ed25519", sig: <64 bytes>}`, a signature by the *actor's own key* over `H(0x05 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment))`, where `0x03` is the content hash of [0006](0006-log-integrity-and-erasure.md) §2 and `0x05` is a new domain tag that never appears in the header tree. The preimage is the canonical bytes of the two parts before the server salts them, so a client that produces canonical CBOR can sign what it submits and a verifier holding the record can check it; erasing either part makes the signature unverifiable, which 0006 §7 already accepts. **Keys are actor records:** a `scatter:v0/key` record in the tenant `actors` partition, keyed by the actor, whose content holds the key ID (the Base32z hash of the public key), algorithm, public key and validity; a later record rotates or revokes it. The server verifies a submitted signature against the actor's current key before appending and refuses a mismatch with `ts-bad-signature`; `verify` (0006 §9, level 2) checks every present signature against the key records in the bundle. Which actors may hold keys, and how they sign, is [0024](0024-subsidiary-accounts.md) §4 as amended: subsidiaries. The instance key (0006 §6) still signs every checkpoint; a client signature is in addition, never instead. This fills the slot 0006 §1 reserved and answers [0007](0007-actor-identity.md)'s keys question for Triplespace; Scatterbase's client and server signatures are the same field shape in its own part layout.
  - §4: Content by default, from the API or a dump (decision 10). A `retain` backfill fetches **every upstream revision's content** as well as its metadata, so a rescued entity's whole history is diffable and exportable here and survives upstream deletion whole; `retain --history metadata` narrows it to `upstream-revision` records alone. The backfill job takes a **source**: `api`, the provider's revision API ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.8, under the `upstream` rate class), which is the default and suits a modest set; or `dump:{path}`, a provider history dump the operator has on hand, which is what a mass rescue of Wikidata or another very large dataset uses. A bulk `retain` whose set exceeds `retention.api_max_entities` (`site`, default 10,000) is refused for `api` unless forced, so a million-entity rescue is not attempted one request at a time. Either source writes the same records; a `dump` job records the dump's identity as its source version ([0002](0002-source-graphs-and-mass-ingest.md) §8.3), and revisions newer than the dump are fetched from the API to close the gap. The automatic retention of properties ([0002](0002-source-graphs-and-mass-ingest.md) §6, as amended) uses the API, since a property's history is small. This settled Q1.
  - §6: The document node is the stable handle (decision 12). In the resolved view, every member of a cluster keeps its own document node, and each carries `schema:about` pointing at the cluster's **current canonical** concept IRI ([0004](0004-identity-clusters-and-equivalence.md) §4), beside the redirect-form `owl:sameAs` between concept IRIs. A document node never moves, so `{base}/wiki/Special:EntityData/{id}` for any member ID is what external consumers are told to cite; the SPARQL Update stream ([0032](0032-sparql-update-stream.md)) rewrites the `schema:about` triple when the canonical member changes.

### A15. Adoption supplies page IDs

- **Date:** 2026-09-28
- **Source:** [0035](0035-adopting-a-wikibase.md) §4
- **Change:** amends §2
- **Summary:** An adoption job supplies the page ID of an adopted record, carrying the source wiki's page ID, and has set the sequence past it. The sentence had been edited in place.

Replaced text (§2):

> **Page IDs are carried forward.** A page ID is taken from one sequence the first time a key is written in any partition, and every later record for that key, in every partition, repeats it in field 9. It is never derived from replay order.

### A16. Category mappings

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §1, §5
- **Change:** extends §3, §5
- **Summary:** The `category-mapping` kind; `scatter:v0/changeset` in the `pages` partition. The row had been added in place.

### A17. Files

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §2, §5, §10–11
- **Change:** extends §1, §5
- **Summary:** `scatter:v0/upload`, in `pages`, has the three standard parts. Its content part holds the SHA-256 of bytes kept outside the log, so erasing that part is what allows the bytes to be destroyed. The instance `log` partition, the `files/{repo}` partitions and `file-types.toml`; §5 had been given these rows in place.

### A18. The instance attestation

- **Date:** 2026-09-30
- **Source:** [0040](0040-instance-prerogatives.md) §3
- **Change:** extends §1
- **Summary:** The attestation part has a second form, the **instance attestation**, for records the instance writes into a tenant on its own authority: `actor` (`instance:{farm slug}`), `authority` (an instance record's partition, offset and leaf hash), `job`, `binding`, and a `signature` by the instance key. Only the server writes it; a submitted record carrying one is refused with `ts-prerogative`.

### A19. Template expansion

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §2, §5, §11
- **Change:** extends §3, §5
- **Summary:** `site` settings `wikitext.expansion`, `wikitext.lua`, `wikitext.renderer`, `wikitext.template_repos` and `wikitext.share`, and, with [0043](0043-lua-modules.md) §8–9, `lua.ids` and `lua.client_site`. The registry of §5 gains `wikitext-functions.toml`. The `template-repo` kind; its row had been added in place.

### A20. The primary tenant

- **Date:** 2026-09-30
- **Source:** [0046](0046-primary-tenant.md) §2, §7
- **Change:** amends §5; extends §3
- **Summary:** The `primary` config kind, whose row had been added in place; the `tenant` record loses its primary flag. Instance graph IRIs are `{farm base}/instance/graph/{name}` and instance origin lines `{farm host}/instance/log/{name}`, which was recorded only in 0046 until this conversion.

Replaced text (§5):

> **Checkpoint origin lines stay `{host}/log/{name}`** ([0006](0006-log-integrity-and-erasure.md) §6). They are a transparency-log convention, not a graph IRI, and the two are deliberately different.

### A21. Special pages and reports

- **Date:** 2026-10-01
- **Source:** [0047](0047-special-pages.md) §1, §4.3
- **Change:** extends §3, §5
- **Summary:** By section:
  - §3: An instance-scope kind `reports`, with code `default` or a tenant slug, holds which reports run as batch, their mirror-graph widenings, the batch schedule and the row limit. It is written with `ts-config` at the farm base.
  - §5: `special-pages.toml` registers every special page name with its MediaWiki name, aliases, scope and status, embedded by `triplespace-titles`.

### A22. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–7
- **Summary:** A1–A21 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A14, A17 (in part), A18, A19 and A21 were blockquotes; most of the rows A3–A13, A16 and A20 name had been added to the tables of §3 and §5 in place; the rest were recorded only in other ADRs. The file before conversion is commit `0b26a3a`.

### A23. Page repositories

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §1, §6
- **Change:** extends §2, §3, §5
- **Summary:** The `page-repo` config kind replaces `template-repo`; `pages.repos` and `pages.share` join the `site` settings; a page a repository serves has a provider-ranged page ID, derived from its upstream page ID; a provider that mints no entities may have no code.

Replaced text (§3):

> | `template-repo` | The repository name | Tenant or instance scope: a foreign template repository, its kind (`tenant` or `mediawiki`), endpoint and cache lifetime | [0042](0042-template-expansion-and-parsoid.md) §11 |

### A24. The `pages/{repo}` partition

- **Date:** 2026-10-01
- **Source:** [0053](0053-mirrored-pages.md) §5, §7, §9
- **Change:** extends §3, §5
- **Summary:** `graphs.toml` gains `pages/{repo}`, an instance mirror partition per page repository in `mirror` mode, with the payload type `scatter:v0/mirrored-page` and its five parts; `search.inherited` and `content.licence` join the `site` settings.

### A25. Upstream revision records with text

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §3
- **Change:** extends §4
- **Summary:** `scatter:v0/upstream-revision` may carry a fourth part, `text`, for a page revision seeded by a fork; such records are keyed by the fork's page ID in the tenant's `log` partition and take provider-ranged revision IDs.

### A26. `css-properties.toml` and the style settings

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §2, §4, §7
- **Change:** extends §3, §5
- **Summary:** The registry gains `css-properties.toml`, embedded by `scatter-css`; `wikitext.site_styles` and `templatestyles.max_bytes` join the `site` settings, and the `fork.*` settings of 0054 are listed with them.

### A27. Forwarder keys

- **Date:** 2026-10-03
- **Source:** [0057](0057-web-tier.md) §10
- **Change:** extends §3
- **Summary:** The instance `config` partition gains the `forwarder` kind: each forwarder key's ID, label and dates, so that issuing and revoking the keys that let a proxy vouch for a client's address is in the log. The secret's hash is kept in `private`.

### A28. Themes

- **Date:** 2026-10-03
- **Source:** [0034](0034-frontend-stack.md) §1
- **Change:** extends §3
- **Summary:** `ui.theme` joins the `site` settings: the Codex token values the instance or a tenant themes its site with.

### A29. The `scatter:v0/task` payload type

- **Date:** 2026-10-04
- **Source:** [0061](0061-sprints-and-tasks.md) §5
- **Change:** extends §1
- **Summary:** `scatter:v0/task`, in `pages`, three standard parts, keyed to the sprint page; operations `claim` and `release`. Added to `graphs.toml`.

### A30. Shallow mirroring settings

- **Date:** 2026-10-07
- **Source:** [0070](0070-shallow-entity-mirroring.md) §2.1
- **Change:** extends §3
- **Summary:** `entities.mirror`, `entities.closure`, `entities.closure_depth`, `entities.label_ttl` and `entities.term_languages` join the `site` settings.

### A31. Derived graphs and extraction sources

- **Date:** 2026-10-07
- **Source:** [0071](0071-derived-statements-from-mirrored-pages.md) §1–3
- **Change:** extends §1, §3, §5
- **Summary:** The four-part `scatter:v0/derivation` payload type; the `extraction` config kind; the `derived/{source}` graph, one per extraction source of a tenant. Added to `graphs.toml`.

### A32. Template mappings and value maps

- **Date:** 2026-10-07
- **Source:** [0072](0072-template-mappings.md) §2, §4
- **Change:** extends §3
- **Summary:** The `template-mapping` and `value-map` config kinds.

### A33. Publications

- **Date:** 2026-10-07
- **Source:** [0074](0074-publishing-a-scope-to-an-external-wiki.md) §1
- **Change:** extends §3
- **Summary:** The `publication` config kind.

### A34. MCP settings

- **Date:** 2026-10-07
- **Source:** [0075](0075-mcp-server.md) §6
- **Change:** extends §3
- **Summary:** The `mcp.*` settings join the `site` settings.

### A35. JSON-LD profiles

- **Date:** 2026-10-07
- **Source:** [0076](0076-dataset-publication.md) §2
- **Change:** extends §3
- **Summary:** The `jsonld-context` config kind.

### A36. Special:Version

- **Date:** 2026-10-07
- **Source:** [0077](0077-special-version.md) §13, §14
- **Change:** extends §3, §5
- **Summary:** The instance `site` settings `version.services` and `instance.source_url`; the registry file `version.toml`, embedded by `triplespace-api-rest`.

### A37. Entity sources

- **Date:** 2026-10-08
- **Source:** [0078](0078-entity-sources.md) §1, §4
- **Change:** extends §2, §3, §5
- **Summary:** Provider numbers from 2^22 to 2^23 − 1 are left out of the registry and assigned per tenant to entity sources. A tenant-scope config kind, `entity-source`, keyed by the source's name. A tenant-scope graph per source, `source/{name}`, with the policies of `mirror/{provider}`.

### A38. Policies for every reserved graph

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5
- **Summary:** The reserved-graph table of §5 carries kind, integrity and export policy for every graph, including `files/{repo}`, `derived/{source}`, `source/{name}`, `pages/{repo}` and the instance `log`, so that `graphs.toml` and this ADR agree without reading the TOML. (PENDING A18)

### A39. Inference properties are instance configuration

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §3
- **Summary:** The identifier properties that drive tier-3 inference are **instance** configuration, held in an instance `reconcile` record, so that tier-3 links stay instance-wide; a tenant's `reconcile` may switch inference off for itself and set provider order and link properties, but not add inference properties. `reconcile` therefore has both an instance record and tenant records, and §3's scope paragraph no longer leaves it to the tenants alone ([0004](0004-identity-clusters-and-equivalence.md) §3, [0018](0018-tenants.md) §3). (PENDING B6)

Replaced text (§3):

> | `reconcile` | `default` or a provider code | Provider order, an optional `order_by_type` override, link properties, identifier properties for inference, normalizer overrides, reconciliation rules | [0004](0004-identity-clusters-and-equivalence.md) §9 |

> **Scope.** The kinds split by scope ([0018](0018-tenants.md) §3). The instance's `config` holds `key`, `graph`, `provider`, `issuer`, `keyed-type`, `tenant`, `alias`, `primary`, `tenancy`, `template`, `consumer` and `forwarder`, the instance lists of `sitelink-policy` and `federation-policy`, and global `group`s; each tenant's `config` holds the rest. A tenant's `config` begins with a `key:` record, the current instance key, and every `key:` record of the instance is appended to it as well, so a tenant's partitions verify from the tenant's bundle alone ([0018](0018-tenants.md) §2).

### A40. The `config` partition has no fixed number

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §3
- **Summary:** "Partition 0" is a designation, not an ID: the instance `config` partition is found by its registry name and has a random 64-bit ID like every other partition ([0018](0018-tenants.md) §2); offset 0 of it holds the first `key:` record. The same correction applies in 0018 §2. (PENDING C7)

Replaced text (§3):

> **Order of the first records.** The `config` partition is partition 0. Its record at offset 0 is the instance's first `key:` record, as [0006](0006-log-integrity-and-erasure.md) §6 requires. Every other partition is created by a `graph:` record in `config`, appended before that partition's first record; that record is the "genesis record" 0006 §2 says names the partition's hash function. The `config` partition's own `graph:config` record follows its key record at offset 1.

### A41. The instance kinds are listed once

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §3
- **Summary:** §3's scope paragraph is the one list of instance-level config kinds, gaining `reports` and `page-repo` (tenant or instance) beside `forwarder`; [0013](0013-postgres-storage.md) §5.5 refers to it instead of carrying its own list. (PENDING C18)

Replaced text (§3):

> **Scope.** The kinds split by scope ([0018](0018-tenants.md) §3). The instance's `config` holds `key`, `graph`, `provider`, `issuer`, `keyed-type`, `tenant`, `alias`, `primary`, `tenancy`, `template`, `consumer` and `forwarder`, the instance lists of `sitelink-policy` and `federation-policy`, and global `group`s; each tenant's `config` holds the rest. A tenant's `config` begins with a `key:` record, the current instance key, and every `key:` record of the instance is appended to it as well, so a tenant's partitions verify from the tenant's bundle alone ([0018](0018-tenants.md) §2).

### A42. `file-repo`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §3
- **Summary:** The kind table of §3 gains a `file-repo` row, tenant or instance scope: a mirrored file repository ([0039](0039-files-and-media.md) §11), logged there as a 0039 amendment. (PENDING C19)

### A43. Four more registry files

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5
- **Summary:** The registry-files table of §5 gains `content-models.toml`, `notation-schemes.toml`, `themes.toml` and `fragments.toml`; chapter 02 §1.4 follows. (PENDING D2)

### A44. Unscoped setting keys are tenant `site` settings

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §3
- **Summary:** Rule: a dotted setting key an ADR names without a scope is a tenant `site` setting. Chapter 23 §3 marks the keys it settles. (PENDING D3)

### A45. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§7
- **Summary:** The Decision's current text now lives in the architecture chapters [01](../architecture/01-log-and-records.md), [02](../architecture/02-graphs-rdf-and-query.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
