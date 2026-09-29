# 0015. Record format and partition registry

- **Status:** Proposed
- **Date:** 2026-09-26
- **Author:** James Hare / Claude Fable
- **Amended by:** [0017 — Entity ID grammar](0017-entity-id-grammar.md), [0018 — Tenants](0018-tenants.md), [0019 — Discussions](0019-discussions.md) (§4: a payload type may declare more than three parts), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§3 amends §3: `acl` is the payload type `scatter:v0/acl`, in `config` for graph targets and in `log` for the rest, not a config kind), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§4 extends §5 with `grants.toml`), [0026 — Sitelinks are URLs](0026-sitelinks.md) (§2–3 extend §3 with the `site-alias` and `sitelink-policy` kinds and §5 with `sites.toml`), [0027 — Preferences, private state and portability](0027-preferences-and-portability.md) (§5 extends §3 with `view-pin`; §1 extends §5 with `preferences.toml`), the 2026-09-27 amendment of [0019](0019-discussions.md) §6 (the `thread-status` kind and `thread-statuses.toml`), [0028 — Tenancy policy](0028-tenancy-policy.md) (§1, §5 and §8 extend §3 with the `tenancy`, `template` and `provider-readers` kinds and `scope` on `group`; §2 extends §5 with `tenancy.toml` and the farm's three instance partitions), [0029 — Resolver namespaces](0029-resolver-namespaces.md) (§1 extends §3 with the `resolver` kind and §5 with `resolvers.toml`), and [0030 — Edit filters](0030-edit-filters.md) (§5 amends §1: the attestation part carries change tags), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§8 amends §1: the attestation part may carry `evidence`; §8 extends §3 with `federation-policy`; §2 and §10 extend §5: `providers.toml` gains `trust`), [0025 — The instance as an OAuth server](0025-oauth-server.md) (§2 extends §3 with `consumer` and `consumer-policy`), [0035 — Adopting an existing Wikibase as a tenant](0035-adopting-a-wikibase.md) (§4 amends §2: an adoption job supplies the page ID of an adopted record)
- **Related:** [0000 — Initial proposition](0000-init.md) (§5 settles the provider-code registry open question), [0001 — Revision metadata in RDF](0001-revision-metadata-rdf.md) (§6 extends §1; §5 settles the graph-IRI open question), [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (amends §8.3; §4 settles the upstream-revision-IRI open question; §5 settles graph IRIs), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md) (§9), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (amends §2, §4.1, §4.2, §4.3; settles the graph-IRI open question), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (amends §2, §3, §6, §7, §9), [0007 — Actor identity](0007-actor-identity.md) (§1 settles the attestation-erasure open question), [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md) (§4), [0009 — Keyed entity types and Domain](0009-keyed-entity-types-and-domain.md) (§6, §7), [0011 — Upstream and local logs](0011-logs.md) (amends §2, §3, §5, §8), [0012 — API requirements for the site UI](0012-api-requirements.md) (§2 amends §2.1 and settles the remaining global-ID question; §3, §6), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (amends §2, §3, §5, §6; settles the header and `page_id` open questions)

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

The header of [0006](0006-log-integrity-and-erasure.md) §3 is unchanged. What changes is what its body commitment (field 6) commits to.

**A body is a fixed list of parts.** Each payload type declares how many parts it has and what each holds. Every Triplespace payload type has at least these three, in this order, and a type may declare more after them ([0019](0019-discussions.md) §4 declares a fourth, the post text):

| # | Part | Holds | MediaWiki's RevisionDelete bit |
|---|---|---|---|
| 0 | **Content** | The payload: a change set, a page operation and its text, an actor record, a log event, an upstream revision (§4), a configuration record (§3) | text |
| 1 | **Comment** | The edit summary or comment, as one string. Its parsed form ([0001](0001-revision-metadata-rdf.md) §6) is a projection | comment |
| 2 | **Attestation** | Who is responsible, and how: the actor key, the job ([0006](0006-log-integrity-and-erasure.md) §3), since [0030](0030-edit-filters.md) §5 the change tags, and since [0022](0022-federation.md) §8 an optional `evidence` field holding a signed remote activity | user |

The comment therefore leaves the change-set payload. A payload type with no comment, such as an actor record, carries a null in that part; the slot still exists, so every record has the same shape.

**Each part is salted and hashed on its own.** Part *i* is stored as the CBOR array `[salt, bytes]`, where the salt is 16 random bytes. Its leaf is `H(0x04 ‖ i ‖ salt ‖ bytes)`, with *i* as one byte. The body commitment in the header is the root over the leaves in order:

```
commitment = H(0x02 ‖ leaf_0 ‖ leaf_1 ‖ … ‖ leaf_{n-1})
```

Tag `0x02` keeps the meaning it had in 0006 §2 (body commitment); `0x04` is new and never appears in the header tree. The content hash `0x03`, used for deduplication and version cursors, is computed over the content part's bytes alone, as before. The salt shrinks from 32 to 16 bytes; 128 random bits are enough to stop a guess at a short erased part from being confirmed, and there are now three salts per record.

**Erasure is per part.** An `erase` record ([0006](0006-log-integrity-and-erasure.md) §7) gains a `parts` field naming any subset of the payload type's parts (`content`, `comment` and `attestation`, plus any the type adds); the default is all of them. Erasing a part replaces its `[salt, bytes]` with `[null, leaf]`, so the root, the header and every checkpoint still verify. What each erasure removes from projections:

| Part erased | Effect | Action API flags ([0012](0012-api-requirements.md) §4) |
|---|---|---|
| Content | As 0006 §7: the change, page text, event parameters or record body are gone | `texthidden`, `erased` |
| Comment | The summary disappears from history, recent changes, diffs and the metadata graph | `commenthidden`, `erased` |
| Attestation | The actor and job are removed. The record stays in the entity's history and keeps its IDs (§2), attributed to no one | `userhidden`, `erased` |

Hiding ([0001](0001-revision-metadata-rdf.md) §4) is unchanged: it is a projection-level flag with the same three bits, and the content stays in the log. Erasure of a part is the irreversible form of hiding that part.

**Verification** ([0006](0006-log-integrity-and-erasure.md) §9, level 2) checks each part: a present part hashes to a leaf that reproduces the commitment; a missing part carries its leaf and is named by an `erase` record. Anything else is a failure.

> **Amended 2026-09-27: client signatures in the attestation part.** The attestation map may carry a **`signature`** field: `{key: <key ID>, alg: "ed25519", sig: <64 bytes>}`, a signature by the *actor's own key* over `H(0x05 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment))`, where `0x03` is the content hash of [0006](0006-log-integrity-and-erasure.md) §2 and `0x05` is a new domain tag that never appears in the header tree. The preimage is the canonical bytes of the two parts before the server salts them, so a client that produces canonical CBOR can sign what it submits and a verifier holding the record can check it; erasing either part makes the signature unverifiable, which 0006 §7 already accepts. **Keys are actor records:** a `scatter:v0/key` record in the tenant `actors` partition, keyed by the actor, whose content holds the key ID (the Base32z hash of the public key), algorithm, public key and validity; a later record rotates or revokes it. The server verifies a submitted signature against the actor's current key before appending and refuses a mismatch with `ts-bad-signature`; `verify` (0006 §9, level 2) checks every present signature against the key records in the bundle. Which actors may hold keys, and how they sign, is [0024](0024-subsidiary-accounts.md) §4 as amended: subsidiaries. The instance key (0006 §6) still signs every checkpoint; a client signature is in addition, never instead. This fills the slot 0006 §1 reserved and answers [0007](0007-actor-identity.md)'s keys question for Triplespace; Scatterbase's client and server signatures are the same field shape in its own part layout.

**Right to vanish** ([0007](0007-actor-identity.md) §4) and **unlinking** (0007 §7) are unchanged; they erase whole records. What this section adds is the case 0007 left open: a demand to cut the tie between a record and an account erases the attestation part only.

**Scatterbase** declares its own part list for its claim payload type: the claim, the attribution, the client signature, the server signature and, later, prior-use acknowledgments. The root rule is the same, so the same `verify` checks both products.

### 2. Global IDs live in the header (amends 0006 §3 and 0013 §6)

Three fields are appended to the header array of [0006](0006-log-integrity-and-erasure.md) §3:

| # | Field | Type | Meaning |
|---|---|---|---|
| 7 | Revision ID | uint or null | The global revision ID of [0012](0012-api-requirements.md) §2.1 |
| 8 | Log ID | uint or null | The global log ID of 0012 §2.1 |
| 9 | Page ID | uint or null | The page ID of the record's key ([0008](0008-namespaces-and-document-pages.md) §4, [0013](0013-postgres-storage.md) §6) |

They are assigned in the appending transaction, before the leaf hash is computed, so they are inside the Merkle tree. The rules of 0013 §6 stand for local records: a revision ID for every record in `local` and `pages`; a log ID for every record in the local log and every record that projects as a log event ([0011](0011-logs.md) §6.1), including local actor records.

**Mirror records get provider-ranged revision IDs (amends 0012 §2.1 and 0013 §6).** Those two ADRs gave mirror records no revision ID, which left `lastrevid`, `baserevid` and `schema:version` undefined for an entity with no local revision. Both are integers in the contracts (`baserevid` in the Action API; `schema:version "26"^^xsd:integer` in [wikibase-compat.md §5.2](../api/wikibase-compat.md)), so the provider cannot be a letter prefix as it is in entity IDs. It is a number prefix instead. The revision-ID space is 63 bits, partitioned by **provider number**:

```
revid = provider_number << 40  |  n
```

Provider number 0 is the instance itself, so local revision IDs are the plain sequence of 0013 §6. Each provider has a number in the registry (§5). For a provider that publishes revision IDs, *n* is the upstream revision ID; for one that does not, such as OpenAlex, *n* is the record's offset in the mirror partition. Forty bits hold a thousand billion upstream revisions, and the split is the same on every instance, so the ID is computed by the writer, needs no allocation, and never changes on rebuild. A `put` therefore carries its revision ID in field 7 like any other record.

What this gives the compatibility surfaces:

- **`lastrevid`** of an entity is the revision ID of its newest record in any source partition, chosen by append time. For a purely mirrored entity that is its latest `put`; a local assertion about it takes over as the newest record.
- **`baserevid`** on a write to a foreign entity is decoded to (partition, offset), and the base-offset check of [0006](0006-log-integrity-and-erasure.md) §8 runs against the newest record for the key across the source partitions, not only the local one. A base that names a state compaction has since replaced is an `editconflict`, which is the right answer.
- **`schema:version`** on the document node (§6) emits the same integer. **`oldid=N`** and `Special:Diff/N` with a mirror revision ID resolve to that observed state and its sync diff ([0012](0012-api-requirements.md) §7). `prop=revisions` and `list=recentchanges` still list local revisions only, as [0012](0012-api-requirements.md) §4 requires.
- The UI and the REST `revid` field show the decoded form as well: "Wikidata revision 2148573921".

Log IDs are unchanged: upstream log events keep their upstream log ID in their content and get no local one, since `list=logevents` selects a provider explicitly ([0012](0012-api-requirements.md) §4, `leprovider`).

**Page IDs are carried forward.** A page ID is taken from one sequence the first time a key is written in any partition — or supplied by an adoption job, which carries the source wiki's page ID and has set the sequence past it ([0035](0035-adopting-a-wikibase.md) §4) — and every later record for that key, in every partition, repeats it in field 9. It is never derived from replay order. This replaces 0013 §6's rule that an entity's first record recovers it, which fails for a mirrored entity once compaction has removed that record. Bootstrap writers ([0013](0013-postgres-storage.md) §9) take page IDs in blocks from the coordinator, as they take offsets.

**Why in the header.** An inclusion proof is about the leaf. MediaWiki clients cite revisions as `oldid=N`; with the ID inside the leaf, a permalink or `Special:Diff/N` is something a third party can verify, and an export bundle needs no sidecar. It also honours 0012 §2.1's requirement that IDs be "stored in the record": the sidecar column in 0013 §6 stored them beside it.

In [0013](0013-postgres-storage.md) §2, `revid`, `logid` and a new `page_id` column are denormalized from `header`, like the other header columns, and the sidecar in the segment file format is dropped. The `view.page_id` sequence of 0013 §6 becomes `log.page_id`. An erased record keeps all three IDs, as its header survives.

### 3. The `config` partition (amends 0005 §4.1; fills 0006 §4 and §6)

A source partition is registered for instance configuration. It corresponds to Scatterbase's `server` graph in the table of [0005](0005-crate-organization.md) §4.1, and the two share one record shape so that Scatterbase can adopt it.

| Graph | IRI | Kind | Written by | History | Integrity | Export |
|---|---|---|---|---|---|---|
| **Config** | `{base}/graph/config` | Source | Administrators, through the configuration API ([0016](0016-permissions-and-access-control.md)) | Full | `logged` | Public |

**Records.** The payload type is `scatter:v0/config`. The header key is `{kind}:{code}`, which is the primary key of `view.registry` in [0013](0013-postgres-storage.md) §5.5. The content part holds the entry; the comment part holds the reason for the change; the attestation names who made it. A later record for the same key replaces the entry, and a record whose content is null retires it.

Two kinds are defined by `scatter-log`, because the log cannot start without them:

| Kind | Code | Entry |
|---|---|---|
| `key` | The signed-note key ID ([0006](0006-log-integrity-and-erasure.md) §6) | The instance's public key. Rotation is a new `key:` record whose attestation is signed by the previous key |
| `graph` | The graph name (§5) | The registry entry of 0005 §4.1: IRI, kind, writers, history, integrity and export policies, and, for a partition, its number, segment exponent k and hash function |

The remaining kinds are Triplespace's, and each product declares its own:

| Kind | Code | Entry | Defined in |
|---|---|---|---|
| `provider` | The provider code | The provider registry entry | [0002](0002-source-graphs-and-mass-ingest.md) §4, [0011](0011-logs.md) §4 |
| `issuer` | The issuer code | The issuer registry entry | [0007](0007-actor-identity.md) §1 |
| `namespace` | The namespace number | The namespace entry | [0008](0008-namespaces-and-document-pages.md) §1 |
| `keyed-type` | The type name | The keyed-type entry | [0009](0009-keyed-entity-types-and-domain.md) §1 |
| `role` | The role name | The properties bound to a role | [0003](0003-statement-ui.md) §7, [0004](0004-identity-clusters-and-equivalence.md) §6 |
| `reconcile` | `default` or a provider code | Provider order, an optional `order_by_type` override, link properties, identifier properties for inference, normalizer overrides, reconciliation rules | [0004](0004-identity-clusters-and-equivalence.md) §9, as amended 2026-09-27 |
| `site` | A setting name | Site name, content languages, the recent-changes window, checkpoint cadence, and other scalar settings | [0006](0006-log-integrity-and-erasure.md) §6, [0010](0010-site-ui.md) §7 |
| `group` | The group name | A permission group | [0016](0016-permissions-and-access-control.md) §3 |
| `tenant`, `alias` | Instance scope | A tenant; a base-URI change | [0018](0018-tenants.md) §3, §9 |
| `providers` | `list` | Tenant scope: the providers whose graphs contribute to this tenant's resolved view and search; the default is Wikidata | [0018](0018-tenants.md) §3, §5 |
| `thread-status` | The status name | A thread status with its category and order | [0019](0019-discussions.md) §6 |
| `site-alias` | A MediaWiki site ID | The host, article path and language of an aliased site | [0026](0026-sitelinks.md) §2 |
| `sitelink-policy` | `deny` (instance), `mode` or `list` (tenant) | Sitelink allow and deny lists | [0026](0026-sitelinks.md) §3 |
| `view-pin` | `{property}` or `{entity}:{property}` | A tenant-wide shape pin | [0027](0027-preferences-and-portability.md) §5 |
| `tenancy` | A switch name | Instance scope: the tenancy policy | [0028](0028-tenancy-policy.md) §1 |
| `template` | `{kind}:{code}` | Instance scope: a tenant-config record new tenants start from; `tenancy:locked` lists the locked keys | [0028](0028-tenancy-policy.md) §8 |
| `provider-readers` | `mode` or `list` | Tenant scope: which tenants may read this provider tenant | [0028](0028-tenancy-policy.md) §5 |
| `resolver` | The resolver name | A resolver namespace: binding, grammar, normalizer with case rule, external IRI | [0029](0029-resolver-namespaces.md) §1 |
| `federation-policy` | `deny` (instance), `mode` or `list` (tenant) | Fediverse domain allow and deny lists | [0022](0022-federation.md) §8 |
| `consumer` | The consumer slug | Instance scope: a registered OAuth consumer, its owner, redirect URIs, requested grants and status | [0025](0025-oauth-server.md) §2 |
| `consumer-policy` | `list` | Tenant scope: which approved consumers may be authorized, and which are auto-approved | [0025](0025-oauth-server.md) §2 |
| `tag` | The tag name | Tenant scope: a user-defined change tag, its description, whether it is active, and the group that may apply it | [0030](0030-edit-filters.md) §5, as amended 2026-09-27 |

ACLs were first listed here as a config kind. Under [0023](0023-moderation.md) §3 they are records of their own payload type, `scatter:v0/acl`: graph ACLs are appended to `config`, and page, entity, record and actor ACLs to the tenant `log` partition.

**Order of the first records.** The `config` partition is partition 0. Its record at offset 0 is the instance's first `key:` record, as [0006](0006-log-integrity-and-erasure.md) §6 requires. Every other partition is created by a `graph:` record in `config`, appended before that partition's first record; that record is the "genesis record" 0006 §2 says names the partition's hash function. The `config` partition's own `graph:config` record follows its key record at offset 1.

**What this settles.** [0004](0004-identity-clusters-and-equivalence.md) §9's "configuration is recorded in the log" now names the partition. 0006 §4's `logged` policy for "the configuration partition" and §6's key registration have their home. [0006](0006-log-integrity-and-erasure.md) §9's export bundle carries "the configuration records that register and rotate keys" because the partition is public. `view.registry` is the projection of the latest record per key.

### 4. Upstream revision records (amends 0002 §8.3 and 0011 §2, §3, §5, §8)

An upstream revision that the instance learns about from a backfill ([0002](0002-source-graphs-and-mass-ingest.md) §5), from a history dump, or from the live stream is recorded as an **upstream revision record**.

| | |
|---|---|
| Partition | The provider log, `log/{provider}` ([0011](0011-logs.md) §2). It is already `full`, `hashed` and internal, keyed by target with upstream IDs, and is where 0011 §5's redaction runs |
| Payload type | `scatter:v0/upstream-revision`. [0011](0011-logs.md) §3's rule that every record in a log graph is a `logevent` is amended to admit this second type |
| Header key | The entity's prefixed ID, or its surrogate for a keyed type, as 0011 §3 |
| Content part | Upstream revision ID and parent ID; upstream timestamp; size and SHA-1; content model; change tags; minor and bot flags; and, where the revision was also observed as a state, the `(partition, offset)` of that `put` |
| Comment part | The upstream edit summary |
| Attestation part | The upstream actor key ([0007](0007-actor-identity.md) §1), or a hidden marker (0007 §5), and the job that brought the record in |

A revision is identified by its upstream revision ID. Re-reading one the instance already holds changes nothing, as for log events ([0011](0011-logs.md) §4). Where the same revision was already observed as a state, 0002 §8.3's "enriches that revision's existing node" is a join on (provider, upstream revision ID).

**Content is a `put`.** If a backfill brings the content of an upstream revision, not only its metadata, the content is written as a `put` in the mirror partition carrying the upstream revision ID ([0002](0002-source-graphs-and-mass-ingest.md) §8.2), the same shape a sync writes. No new content record exists. Such `put`s are written only for entities exempt from compaction: retained entities and `full` mirrors.

> **Amended 2026-09-27: content by default, from the API or a dump.** A `retain` backfill fetches **every upstream revision's content** as well as its metadata, so a rescued entity's whole history is diffable and exportable here and survives upstream deletion whole; `retain --history metadata` narrows it to `upstream-revision` records alone. The backfill job takes a **source**: `api`, the provider's revision API ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.8, under the `upstream` rate class), which is the default and suits a modest set; or `dump:{path}`, a provider history dump the operator has on hand, which is what a mass rescue of Wikidata or another very large dataset uses. A bulk `retain` whose set exceeds `retention.api_max_entities` (`site`, default 10,000) is refused for `api` unless forced, so a million-entity rescue is not attempted one request at a time. Either source writes the same records; a `dump` job records the dump's identity as its source version ([0002](0002-source-graphs-and-mass-ingest.md) §8.3), and revisions newer than the dump are fetched from the API to close the gap. The automatic retention of properties ([0002](0002-source-graphs-and-mass-ingest.md) §6, as amended) uses the API, since a property's history is small.

**Redaction becomes partial erasure (amends 0011 §5).** When upstream hides part of a revision or a log event, the sync job erases that part with reason class `upstream`, under §1: a hidden comment erases the comment part, a hidden user erases the attestation part, and hidden content erases the content part of the observed `put`. The redacted copy that 0011 §5 appended before erasing is no longer written. The visibility bits a projection reports are derived from which parts are erased with reason `upstream`. Outright suppression, where an event or revision vanishes from upstream's public record, still erases all three parts.

**Serving.** [0013](0013-postgres-storage.md) §5 gains one table, kept apart from `view.activity` because it grows with backfilled history rather than local activity:

```sql
CREATE TABLE view.upstream_revision (
  provider text NOT NULL, upstream_revid bigint NOT NULL,
  entity_id text NOT NULL, parent_revid bigint, time timestamptz NOT NULL,
  actor_key text, comment text, tags text[] NOT NULL DEFAULT '{}',
  minor boolean, bot boolean, size integer, sha1 text, visibility smallint NOT NULL DEFAULT 0,
  observed_partition bigint, observed_offset bigint,
  partition bigint NOT NULL, "offset" bigint NOT NULL,          -- bigint since 0018 §2
  PRIMARY KEY (provider, upstream_revid)
);
CREATE INDEX upstream_revision_entity ON view.upstream_revision (entity_id, time DESC);
```

[0012](0012-api-requirements.md) §6's "served from the log instead" reads from it; those rows are `upstream-edit` activity rows with `record` present. The live fetch runs only for the interval the table does not cover.

**IRIs.** The revision node of any state that carries an upstream revision ID, observed or backfilled, is the upstream revision:

| Revision | IRI |
|---|---|
| Upstream revision, observed or backfilled | `{article path}Special:Redirect/revision/{revid}` on the provider's wiki, for example `https://www.wikidata.org/wiki/Special:Redirect/revision/123`. This parallels `Special:Redirect/user/{id}` ([0007](0007-actor-identity.md) §2) and `Special:Redirect/logid/{id}` ([0011](0011-logs.md) §8) |
| Local revision | `{base}/revision/{revid}` ([0001](0001-revision-metadata-rdf.md) §1), which §2 now makes well-defined |
| Observed state from a provider with no revision IDs, such as OpenAlex | `{base}/record/{partition}/{offset}`, the record IRI of 0011 §8 |

An upstream revision node is `prov:specializationOf` the **upstream** document node, `https://www.wikidata.org/wiki/Special:EntityData/Q123`. This settles [0002](0002-source-graphs-and-mass-ingest.md)'s open question about upstream revision IRIs.

### 5. Graph names and IRIs (settles the open question of 0001, 0002 and 0005)

**A graph's IRI is `{base}/graph/{name}`.** `{base}` is the instance's base URI: the origin that serves `/wiki/`, `/w/api.php` and `/entity/`, the same base that [0001](0001-revision-metadata-rdf.md) §5 gives instance data. The IRI is therefore per instance, which it has to be: the metadata graph attributes triples to this instance's revisions, and two instances' full dumps ([0013](0013-postgres-storage.md) §8) must be loadable together without their `local` graphs colliding, especially now that [0009](0009-keyed-entity-types-and-domain.md) §6 gives Domain subjects the same IRI everywhere.

**Graph names are fixed by convention.** They are the partition names of [0013](0013-postgres-storage.md) §2, and the registry (§3, kind `graph`) stores both name and IRI:

| Name | Kind | Defined in |
|---|---|---|
| `config` | Source | §3 |
| `local` | Source | [0002](0002-source-graphs-and-mass-ingest.md) §2 |
| `pages` | Source | [0008](0008-namespaces-and-document-pages.md) §4 |
| `log` | Source | [0011](0011-logs.md) §2 |
| `actors`, `accounts` | Source | [0007](0007-actor-identity.md) §8 |
| `mirror/{provider}`, `actors/{provider}`, `log/{provider}` | Source, one set per provider | 0002 §2, 0007 §8, 0011 §2 |
| `resolved` | Projection: the main graph of [0001](0001-revision-metadata-rdf.md) §2 as [0002](0002-source-graphs-and-mass-ingest.md) §3 computes it | 0002 §3 |
| `metadata` | Projection | 0001 §2 |

`{provider}` is the provider's **slug**, a lower-case name such as `wikidata`, `openalex`, `librarybase` or `internetdomains`. The provider registry entry ([0002](0002-source-graphs-and-mass-ingest.md) §4) records the slug alongside the two-letter code used in IDs.

**Checkpoint origin lines stay `{host}/log/{name}`** ([0006](0006-log-integrity-and-erasure.md) §6). They are a transparency-log convention, not a graph IRI, and the two are deliberately different.

**The repository is the authority for names and codes.** The files under `docs/registry/` in the Triplespace repository are the registry of record:

| File | Lists | Defined in |
|---|---|---|
| `graphs.toml` | The reserved graph names above, with each one's kind, policies and payload type | this section, §3 |
| `providers.toml` | Each provider's two-letter code, slug, **provider number** (§2), type codes with their upstream prefixes and IRI templates, issuer, whether it publishes revision IDs, and, since [0022](0022-federation.md) §2, its `trust` mode and key-chain URL | [0000](0000-init.md) §3, [0002](0002-source-graphs-and-mass-ingest.md) §4 |
| `issuers.toml` | The issuer codes and actor models | [0007](0007-actor-identity.md) §1 |
| `namespaces.toml` | The default namespace numbers and kinds | [0008](0008-namespaces-and-document-pages.md) §2 |
| `keyed-types.toml` | The keyed entity types | [0009](0009-keyed-entity-types-and-domain.md) §1 |
| `groups.toml` | The default groups, their permissions, and the default graph ACLs | [0016](0016-permissions-and-access-control.md) §2–4 |
| `grants.toml` | The API-key grants and the permissions each covers | [0024](0024-subsidiary-accounts.md) §4 |
| `sites.toml` | Site aliases: MediaWiki site IDs, hosts, article paths and languages, for sitelink compatibility | [0026](0026-sitelinks.md) §2 |
| `thread-statuses.toml` | The default thread statuses, with category and order | [0019](0019-discussions.md) §6 |
| `preferences.toml` | The registered preference keys, types and defaults | [0027](0027-preferences-and-portability.md) §1 |
| `tenancy.toml` | The tenancy switches, the three presets and their global groups | [0028](0028-tenancy-policy.md) §1 |
| `resolvers.toml` | The default resolvers (`doi`, `url`) and drafted candidates | [0029](0029-resolver-namespaces.md) §8 |

`scatter-log`, `scatter-providers` and `scatter-actors` embed these files and ship them as defaults; an instance's `config` partition (§3) starts from them and may diverge. Allocating a new provider code, slug, number or graph name is a change to the file, in a commit; a code is never reused. This is the same rule [0005](0005-crate-organization.md) §5 already applies to the `scatter:` vocabulary through `scatter-vocab`, and it settles [0000](0000-init.md)'s open question about who allocates provider codes. The code for internetdomains.wiki ([0009](0009-keyed-entity-types-and-domain.md), open questions) is allocated there when its adapter is written; the slug `internetdomains` is reserved now.

### 6. Document nodes for foreign entities (extends 0001 §1, 0002 §4 and 0009 §6)

Every entity on the instance has a document node under the instance's base, whatever minted its ID: `data:Q6`, `data:WDQ42` and `data:en.wikipedia.org` are all `{base}/wiki/Special:EntityData/{id}`, as [0009](0009-keyed-entity-types-and-domain.md) §6 already states for Domains. It is what local revisions of the entity specialize, and where the instance's own metadata about the record lives.

For a foreign entity, the document node also points upstream:

```turtle
# graph <{base}/graph/metadata>
data:WDQ42 pav:importedFrom <https://www.wikidata.org/wiki/Special:EntityData/Q42> ;
    pav:hasCurrentVersion <https://www.wikidata.org/wiki/Special:Redirect/revision/123> ;
    pav:retrievedFrom <{base}/job/17> ; pav:importedOn "…"^^xsd:dateTime .
```

`pav:importedFrom` is the term [0001](0001-revision-metadata-rdf.md) §6 reserved for this. Upstream revisions specialize the upstream document node (§4); the instance's document node reaches them through `pav:hasCurrentVersion` and the job.

> **Amended 2026-09-27: the document node is the stable handle.** In the resolved view, every member of a cluster keeps its own document node, and each carries `schema:about` pointing at the cluster's **current canonical** concept IRI ([0004](0004-identity-clusters-and-equivalence.md) §4), beside the redirect-form `owl:sameAs` between concept IRIs. A document node never moves, so `{base}/wiki/Special:EntityData/{id}` for any member ID is what external consumers are told to cite; the SPARQL Update stream ([0032](0032-sparql-update-stream.md)) rewrites the `schema:about` triple when the canonical member changes.

### 7. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-log` | Body parts and part leaves (§1); header fields 7–9 and the provider-ranged revision ID (§2); the `config` payload type with kinds `key` and `graph` (§3); `erase` with `parts`, and `LogStore::erase_bodies` becomes `erase_parts`; `docs/registry/graphs.toml` embedded as the default registry (§5) |
| `scatter-log-postgres` | The `page_id` column; `erased` as a bitmask of parts; partial erasure as a body rewrite ([0013](0013-postgres-storage.md) §3) |
| `scatter-integrity` | `verify` per part (§1) |
| `scatter-providers` | Provider slugs and numbers; `docs/registry/providers.toml` embedded as the default registry (§5) |
| `scatter-actors` | `docs/registry/issuers.toml` embedded as the default issuer registry (§5) |
| `scatter-mwlog` | The `upstream-revision` payload type beside `logevent` (§4) |
| `scatter-adapter-wikidata` | Emits upstream revision records from history dumps, `prop=revisions` and the stream (§4) |
| `triplespace-projections` | The `upstream_revision` projection; the registry projection reads `config` (§3, §4) |
| `triplespace-rdf` | The IRI rules of §4–6 |

No crate is added.

## Consequences

- **The record format is fixed.** With this ADR, [0006](0006-log-integrity-and-erasure.md) and [0013](0013-postgres-storage.md) §2, the header and body layouts have no known pending change. A later change needs a format version.
- **Erasure matches MediaWiki's three bits exactly.** Text, comment and user can each be hidden or erased on their own, so every RevisionDelete case has an answer, and [0011](0011-logs.md) §5 no longer needs redacted copies.
- **Each record carries two more salts and three more header fields.** About 40 bytes on the ~100 of 0006's estimate; for a 100-million-entity mirror, on the order of 4 GB.
- **Permalinks are verifiable.** An inclusion proof for `oldid=N` proves that revision N is the record it claims to be.
- **Rebuild is deterministic.** Page IDs are read from headers, never derived, and 0013's last open question closes.
- **Configuration has history, integrity and an export.** Every registry change is a signed, ordered record, and `view.registry` is rebuildable like any projection. Administrators change configuration through the API, not files.
- **Scatterbase can adopt §1, §3 and §5 unchanged.** Its claim type declares its own parts; its `server` graph is the `config` partition under another name; its graph names come from the same registry file.
- **Backfilled history grows the provider log.** The audit [0011](0011-logs.md) already calls for should measure revision volume for retained entities, not only log events.
- **Two document nodes per foreign entity.** Consumers of the metadata graph follow `pav:importedFrom` to reach upstream's own record of the entity.

## Open questions

- ~~**Content backfill by default.** Whether retaining an entity should fetch the content of every upstream revision, or only the metadata, given the cost noted in [0002](0002-source-graphs-and-mass-ingest.md) Consequences.~~ *Settled 2026-09-27 (§4 amendment): content by default, from the provider's API for modest sets and from a history dump the operator supplies for large ones; `--history metadata` opts out.*
- **Whether `site` settings should each be a record,** or one record per settings group.
- **Revision IDs beyond 2^40 upstream.** No provider is near it; if one ever is, its number is re-registered with a wider split, which is a format version.
- ~~**Reason classes.** `upstream`, `legal` and `privacy` are now used; the full vocabulary and its visibility are settled in [0016](0016-permissions-and-access-control.md) §6.~~ *Settled by [0016](0016-permissions-and-access-control.md) §6: `legal`, `privacy`, `upstream` and `operational`, extensible in `site` configuration; the class and authority reference are visible to `ts-viewerasures` only.*

## References

- [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md)
- [MediaWiki RevisionDelete](https://www.mediawiki.org/wiki/Manual:RevisionDelete) (the three independent bits)
- [MediaWiki `Special:Redirect`](https://www.mediawiki.org/wiki/Help:Special_pages) (lookup by revision, user, page or log ID)
- [PAV ontology](https://pav-ontology.github.io/pav/) (`pav:importedFrom`, `pav:hasCurrentVersion`)
- Scatterbase decision record (`server` graph, server-key registration claim)
