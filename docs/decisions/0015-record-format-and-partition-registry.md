# 0015. Record format and partition registry

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-07 (A36)
- **Author:** James Hare / Claude Fable
- **Changes:** [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md)
- **Uses:** [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0009](0009-keyed-entity-types-and-domain.md), [0016](0016-permissions-and-access-control.md)

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

The header of [0006](0006-log-integrity-and-erasure.md) §3 is unchanged. What changes is what its body commitment (field 6) commits to.

**A body is a fixed list of parts.** Each payload type declares how many parts it has and what each holds. Every Triplespace payload type has at least these three, in this order, and a type may declare more after them ([0019](0019-discussions.md) §4 declares a fourth, the post text):

| # | Part | Holds | MediaWiki's RevisionDelete bit |
|---|---|---|---|
| 0 | **Content** | The payload: a change set, a page operation and its text, an actor record, a log event, an upstream revision (§4), a configuration record (§3) | text |
| 1 | **Comment** | The edit summary or comment, as one string. Its parsed form ([0001](0001-revision-metadata-rdf.md) §6) is a projection | comment |
| 2 | **Attestation** | Who is responsible, and how: the actor key, the job ([0006](0006-log-integrity-and-erasure.md) §3), the change tags ([0030](0030-edit-filters.md) §5), an optional `evidence` field holding a signed remote activity ([0022](0022-federation.md) §8), and an optional client `signature` (below) | user |

The comment therefore leaves the change-set payload. A payload type with no comment, such as an actor record, carries a null in that part; the slot still exists, so every record has the same shape. `scatter:v0/upload`, in `pages`, has the three standard parts; its content part holds the SHA-256 of bytes kept outside the log, so erasing that part is what allows the bytes to be destroyed ([0039](0039-files-and-media.md) §2). `scatter:v0/task`, in `pages`, has the three standard parts and is keyed to a sprint's page ID; its content part names the rule and subject claimed ([0061](0061-sprints-and-tasks.md) §5). `scatter:v0/derivation`, in `derived/{source}`, has four parts, the fourth being **evidence**, the page text the statements came from, so that it can be erased on its own; it is keyed by page ID and extractor ([0071](0071-derived-statements-from-mirrored-pages.md) §3).

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

**Client signatures.** The attestation map may carry a **`signature`** field: `{key: <key ID>, alg: "ed25519", sig: <64 bytes>}`, a signature by the *actor's own key* over `H(0x05 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment))`, where `0x03` is the content hash of [0006](0006-log-integrity-and-erasure.md) §2 and `0x05` is a new domain tag that never appears in the header tree. The preimage is the canonical bytes of the two parts before the server salts them, so a client that produces canonical CBOR can sign what it submits and a verifier holding the record can check it; erasing either part makes the signature unverifiable, which 0006 §7 already accepts. **Keys are actor records:** a `scatter:v0/key` record in the tenant `actors` partition, keyed by the actor, whose content holds the key ID (the Base32z hash of the public key), algorithm, public key and validity; a later record rotates or revokes it. The server verifies a submitted signature against the actor's current key before appending and refuses a mismatch with `ts-bad-signature`; `verify` (0006 §9, level 2) checks every present signature against the key records in the bundle. Which actors may hold keys, and how they sign, is [0024](0024-subsidiary-accounts.md) §4: subsidiaries. The instance key (0006 §6) still signs every checkpoint; a client signature is in addition, never instead. This fills the slot 0006 §1 reserved and answers [0007](0007-actor-identity.md)'s keys question for Triplespace; Scatterbase's client and server signatures are the same field shape in its own part layout.

**The instance attestation.** The attestation part has a second form, the **instance attestation**, for records the instance writes into a tenant on its own authority: `actor` (`instance:{farm slug}`), `authority` (an instance record's partition, offset and leaf hash), `job`, `binding`, and a `signature` by the instance key. Only the server writes it; a submitted record carrying one is refused with `ts-prerogative`.

**Right to vanish** ([0007](0007-actor-identity.md) §4) and **unlinking** (0007 §7) are unchanged; they erase whole records. What this section adds is the case 0007 left open: a demand to cut the tie between a record and an account erases the attestation part only.

**Scatterbase** declares its own part list for its claim payload type: the claim, the attribution, the client signature, the server signature and, later, prior-use acknowledgments. The root rule is the same, so the same `verify` checks both products.

### 2. Global IDs live in the header (amends 0006 §3 and 0013 §6)

*Changed by A3, A15, A23.*

Three fields are appended to the header array of [0006](0006-log-integrity-and-erasure.md) §3:

| # | Field | Type | Meaning |
|---|---|---|---|
| 7 | Revision ID | uint or null | The global revision ID of [0012](0012-api-requirements.md) §2.1 |
| 8 | Log ID | uint or null | The global log ID of 0012 §2.1 |
| 9 | Page ID | uint or null | The page ID of the record's key ([0008](0008-namespaces-and-document-pages.md) §4, [0013](0013-postgres-storage.md) §6) |

They are assigned in the appending transaction, before the leaf hash is computed, so they are inside the Merkle tree. The sequences are per tenant, because MediaWiki clients expect one sequence per wiki ([0018](0018-tenants.md) §2). The rules of 0013 §6 stand for local records: a revision ID for every record in `local` and `pages`; a log ID for every record in the local log and every record that projects as a log event ([0011](0011-logs.md) §6.1), including local actor records.

**Mirror records get provider-ranged revision IDs (amends 0012 §2.1 and 0013 §6).** Those two ADRs gave mirror records no revision ID, which left `lastrevid`, `baserevid` and `schema:version` undefined for an entity with no local revision. Both are integers in the contracts (`baserevid` in the Action API; `schema:version "26"^^xsd:integer` in [wikibase-compat.md §5.2](../api/wikibase-compat.md)), so the provider cannot be a letter prefix as it is in entity IDs. It is a number prefix instead. The revision-ID space is 63 bits, partitioned by **provider number**:

```
revid = provider_number << 40  |  n
```

Provider number 0 is the instance itself, so local revision IDs are the plain sequence of 0013 §6. Each provider has a number in the registry (§5). For a provider that publishes revision IDs, *n* is the upstream revision ID; for one that does not, such as OpenAlex, *n* is the record's offset in the mirror partition. Forty bits hold a thousand billion upstream revisions, and the split is the same on every instance, so the ID is computed by the writer, needs no allocation, and never changes on rebuild. It works across tenants without a new rule: Librarybase's revision 900 is `900` at home and `LB_number << 40 | 900` when another tenant reads it ([0018](0018-tenants.md) §2, §5). A `put` therefore carries its revision ID in field 7 like any other record.

What this gives the compatibility surfaces:

- **`lastrevid`** of an entity is the revision ID of its newest record in any source partition, chosen by append time. For a purely mirrored entity that is its latest `put`; a local assertion about it takes over as the newest record.
- **`baserevid`** on a write to a foreign entity is decoded to (partition, offset), and the base-offset check of [0006](0006-log-integrity-and-erasure.md) §8 runs against the newest record for the key across the source partitions, not only the local one. A base that names a state compaction has since replaced is an `editconflict`, which is the right answer.
- **`schema:version`** on the document node (§6) emits the same integer. **`oldid=N`** and `Special:Diff/N` with a mirror revision ID resolve to that observed state and its sync diff ([0012](0012-api-requirements.md) §7). `prop=revisions` and `list=recentchanges` still list local revisions only, as [0012](0012-api-requirements.md) §4 requires.
- The UI and the REST `revid` field show the decoded form as well: "Wikidata revision 2148573921".

Log IDs are unchanged: upstream log events keep their upstream log ID in their content and get no local one, since `list=logevents` selects a provider explicitly ([0012](0012-api-requirements.md) §4, `leprovider`).

**Page IDs are carried forward.** A page ID is taken from one sequence the first time a key is written in any partition — or supplied by an adoption job, which carries the source wiki's page ID and has set the sequence past it ([0035](0035-adopting-a-wikibase.md) §4) — and every later record for that key, in every partition, repeats it in field 9. It is never derived from replay order. This replaces 0013 §6's rule that an entity's first record recovers it, which fails for a mirrored entity once compaction has removed that record. Bootstrap writers ([0013](0013-postgres-storage.md) §9) take page IDs in blocks from the coordinator, as they take offsets.

**A page a page repository serves has a provider-ranged page ID,** `provider_number << 40 | upstream page ID`, derived and never minted, so that a foreign page has a stable `pageid` on every tenant without any write; in `mirror` mode the `pages/{repo}` records carry it in field 9 and the ranged upstream revision ID in field 7 ([0052](0052-page-repositories-and-title-inheritance.md) §6, [0053](0053-mirrored-pages.md) §5). Local page IDs stay below 2^40.

**Why in the header.** An inclusion proof is about the leaf. MediaWiki clients cite revisions as `oldid=N`; with the ID inside the leaf, a permalink or `Special:Diff/N` is something a third party can verify, and an export bundle needs no sidecar. It also honours 0012 §2.1's requirement that IDs be "stored in the record": the sidecar column in 0013 §6 stored them beside it.

In [0013](0013-postgres-storage.md) §2, `revid`, `logid` and a new `page_id` column are denormalized from `header`, like the other header columns, and the sidecar in the segment file format is dropped. The `view.page_id` sequence of 0013 §6 becomes `log.page_id`. An erased record keeps all three IDs, as its header survives.

### 3. The `config` partition (amends 0005 §4.1; extends 0006 §4 and §6)

*Changed by A3, A4, A5, A6, A8, A9, A10, A11, A12, A13, A16, A19, A20, A21, A23, A24, A26, A27, A28, A30, A31, A32, A33, A34, A35, A36.*

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
| `reconcile` | `default` or a provider code | Provider order, an optional `order_by_type` override, link properties, identifier properties for inference, normalizer overrides, reconciliation rules | [0004](0004-identity-clusters-and-equivalence.md) §9 |
| `site` | A setting name | Site name, content languages, the recent-changes window, checkpoint cadence, and other scalar settings, among them the `wikitext.*` and `lua.*` settings of template expansion ([0042](0042-template-expansion-and-parsoid.md) §2, [0043](0043-lua-modules.md) §8–9), `pages.repos` and `pages.share` ([0052](0052-page-repositories-and-title-inheritance.md) §1), `search.inherited` and `content.licence` ([0053](0053-mirrored-pages.md) §7, §9), the `fork.*` settings ([0054](0054-forking-a-mirrored-page.md) §3–4, §8), `wikitext.site_styles` and `templatestyles.max_bytes` ([0055](0055-templatestyles-templatedata-and-page-properties.md) §2, §4), and `ui.theme`, the Codex token values a site is themed with ([0034](0034-frontend-stack.md) §1), the `entities.*` settings of shallow mirroring ([0070](0070-shallow-entity-mirroring.md) §2.1, §4, §6), the `mcp.*` settings ([0075](0075-mcp-server.md) §6), and `version.services` and `instance.source_url` ([0077](0077-special-version.md) §13) | [0006](0006-log-integrity-and-erasure.md) §6, [0010](0010-site-ui.md) §7 |
| `group` | The group name | A permission group; a global group carries `scope` ([0028](0028-tenancy-policy.md) §8) | [0016](0016-permissions-and-access-control.md) §3 |
| `tenant`, `alias` | Instance scope | A tenant; a base-URI change | [0018](0018-tenants.md) §3, §9 |
| `primary` | `primary` | Instance scope: the primary tenant's slug and, for a transfer, the offer it accepts; one current record | [0046](0046-primary-tenant.md) §2 |
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
| `forwarder` | The forwarder key ID | Instance scope: a forwarder key's label and dates, never its secret or hash; a null record revokes it | [0057](0057-web-tier.md) §10 |
| `consumer-policy` | `list` | Tenant scope: which approved consumers may be authorized, and which are auto-approved | [0025](0025-oauth-server.md) §2 |
| `tag` | The tag name | Tenant scope: a user-defined change tag, its description, whether it is active, and the group that may apply it | [0030](0030-edit-filters.md) §5 |
| `category-mapping` | The mapping name | Tenant scope: a category name or pattern, and the page statement its members get | [0038](0038-page-metadata-and-categories.md) §5 |
| `page-repo` | The repository name | Tenant or instance scope: a page repository, its kind (`tenant` or `mediawiki`), provider, served namespaces, mode, `shadowed`, `titles`, cache lifetime, events, licence and display name. Replaces `template-repo` (A23) | [0052](0052-page-repositories-and-title-inheritance.md) §1 |
| `reports` | `default` or a tenant slug | Instance scope: which reports run as batch, their mirror-graph widenings, the batch schedule and the row limit; written with `ts-config` at the farm base | [0047](0047-special-pages.md) §4.3 |
| `extraction` | The source name | Tenant scope: an extraction source: its repository, pages, extractors, mappings, line settings, page subject and reference properties | [0071](0071-derived-statements-from-mirrored-pages.md) §2 |
| `template-mapping` | The mapping name | Tenant scope: a template, conditions, subject, match keys, label and statements | [0072](0072-template-mappings.md) §2 |
| `value-map` | The map name | Tenant scope: strings to values, for parsers and heading facets | [0072](0072-template-mappings.md) §4 |
| `publication` | The publication name | Tenant scope: a destination wiki, account, scope, rows, data pages and kits | [0074](0074-publishing-a-scope-to-an-external-wiki.md) §1 |
| `jsonld-context` | The profile name | Tenant scope: the schema.org types, property terms, intervals and identifiers of a JSON-LD profile | [0076](0076-dataset-publication.md) §2 |

**Scope.** The kinds split by scope ([0018](0018-tenants.md) §3). The instance's `config` holds `key`, `graph`, `provider`, `issuer`, `keyed-type`, `tenant`, `alias`, `primary`, `tenancy`, `template`, `consumer` and `forwarder`, the instance lists of `sitelink-policy` and `federation-policy`, and global `group`s; each tenant's `config` holds the rest. A tenant's `config` begins with a `key:` record, the current instance key, and every `key:` record of the instance is appended to it as well, so a tenant's partitions verify from the tenant's bundle alone ([0018](0018-tenants.md) §2).

ACLs were first listed here as a config kind. Under [0023](0023-moderation.md) §3 they are records of their own payload type, `scatter:v0/acl`: graph ACLs are appended to `config`, and page, entity, record and actor ACLs to the tenant `log` partition.

**Order of the first records.** The `config` partition is partition 0. Its record at offset 0 is the instance's first `key:` record, as [0006](0006-log-integrity-and-erasure.md) §6 requires. Every other partition is created by a `graph:` record in `config`, appended before that partition's first record; that record is the "genesis record" 0006 §2 says names the partition's hash function. The `config` partition's own `graph:config` record follows its key record at offset 1.

**What this settles.** [0004](0004-identity-clusters-and-equivalence.md) §9's "configuration is recorded in the log" now names the partition. 0006 §4's `logged` policy for "the configuration partition" and §6's key registration have their home. [0006](0006-log-integrity-and-erasure.md) §9's export bundle carries "the configuration records that register and rotate keys" because the partition is public. `view.registry` is the projection of the latest record per key.

### 4. Upstream revision records (amends 0002 §8.3 and 0011 §2, §3, §5, §8)

*Changed by A14, A25.*

An upstream revision that the instance learns about from a backfill ([0002](0002-source-graphs-and-mass-ingest.md) §5), from a history dump, or from the live stream is recorded as an **upstream revision record**.

| | |
|---|---|
| Partition | The provider log, `log/{provider}` ([0011](0011-logs.md) §2). It is already `full`, `hashed` and internal, keyed by target with upstream IDs, and is where 0011 §5's redaction runs |
| Payload type | `scatter:v0/upstream-revision`, one of the types [0011](0011-logs.md) §3 admits in a log graph |
| Header key | The entity's prefixed ID, or its surrogate for a keyed type, as 0011 §3 |
| Content part | Upstream revision ID and parent ID; upstream timestamp; size and SHA-1; content model; change tags; minor and bot flags; and, where the revision was also observed as a state, the `(partition, offset)` of that `put` |
| Comment part | The upstream edit summary |
| Attestation part | The upstream actor key ([0007](0007-actor-identity.md) §1), or a hidden marker (0007 §5), and the job that brought the record in |
| Text part (optional, fourth) | For a **page** revision seeded by a fork, the revision's wikitext, kept apart so that hiding or erasing the content touches nothing else; such records live in the tenant's `log` partition keyed by the fork's page ID ([0054](0054-forking-a-mirrored-page.md) §3) |

A revision is identified by its upstream revision ID. Re-reading one the instance already holds changes nothing, as for log events ([0011](0011-logs.md) §4). Where the same revision was already observed as a state, 0002 §8.3's "enriches that revision's existing node" is a join on (provider, upstream revision ID).

**Content is a `put`.** When a backfill brings the content of an upstream revision, not only its metadata, the content is written as a `put` in the mirror partition carrying the upstream revision ID ([0002](0002-source-graphs-and-mass-ingest.md) §8.2), the same shape a sync writes. No new content record exists. Such `put`s are written only for entities exempt from compaction: retained entities and `full` mirrors.

**Content by default, from the API or a dump.** A `retain` backfill fetches **every upstream revision's content** as well as its metadata, so a rescued entity's whole history is diffable and exportable here and survives upstream deletion whole; `retain --history metadata` narrows it to `upstream-revision` records alone. The backfill job takes a **source**: `api`, the provider's revision API ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.8, under the `upstream` rate class), which is the default and suits a modest set; or `dump:{path}`, a provider history dump the operator has on hand, which is what a mass rescue of Wikidata or another very large dataset uses. A bulk `retain` whose set exceeds `retention.api_max_entities` (`site`, default 10,000) is refused for `api` unless forced, so a million-entity rescue is not attempted one request at a time. Either source writes the same records; a `dump` job records the dump's identity as its source version ([0002](0002-source-graphs-and-mass-ingest.md) §8.3), and revisions newer than the dump are fetched from the API to close the gap. The automatic retention of properties ([0002](0002-source-graphs-and-mass-ingest.md) §6) uses the API, since a property's history is small.

**Redaction becomes partial erasure (amends 0011 §5).** When upstream hides part of a revision or a log event, the sync job erases that part with reason class `upstream`, under §1: a hidden comment erases the comment part, a hidden user erases the attestation part, and hidden content erases the content part of the observed `put`. 0011 §5 no longer writes a redacted copy before erasing (0011 A3). The visibility bits a projection reports are derived from which parts are erased with reason `upstream`. Outright suppression, where an event or revision vanishes from upstream's public record, still erases all three parts.

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

An upstream revision node is `prov:specializationOf` the **upstream** document node, `https://www.wikidata.org/wiki/Special:EntityData/Q123`. This settles [0002](0002-source-graphs-and-mass-ingest.md) Q10.

### 5. Graph names and IRIs (settles 0001 Q2, 0002 Q1 and 0005 Q6)

*Changed by A2, A3, A4, A5, A7, A9, A10, A11, A12, A16, A17, A19, A20, A21, A23, A24, A26, A31, A36.*

**A graph's IRI is `{base}/graph/{name}`** for a tenant's partitions, where `{base}` is the tenant's base URI: the origin that serves its `/wiki/`, `/w/api.php` and `/entity/`, the same base that [0001](0001-revision-metadata-rdf.md) §5 gives its data ([0018](0018-tenants.md) §2). The instance's own partitions are under the reserved path `{farm base}/instance/graph/{name}`, so that they cannot be confused with a tenant's when the farm base is a tenant's base ([0046](0046-primary-tenant.md) §7). The IRI is therefore per instance, which it has to be: the metadata graph attributes triples to this instance's revisions, and two instances' full dumps ([0013](0013-postgres-storage.md) §8) must be loadable together without their `local` graphs colliding, especially now that [0009](0009-keyed-entity-types-and-domain.md) §6 gives Domain subjects the same IRI everywhere.

**Graph names are fixed by convention.** They are the partition names of [0013](0013-postgres-storage.md) §2, and the registry (§3, kind `graph`), keyed by (tenant, name), stores both name and IRI. Each tenant has its own `config`, `local`, `pages`, `log`, `actors` and `accounts`; the instance holds its `config` and `log`, every provider's partitions and, with farm identity, `actors/{farm}`, `accounts/{farm}` and `log/{farm}` under the per-issuer names ([0018](0018-tenants.md) §2, [0028](0028-tenancy-policy.md) §2, [0039](0039-files-and-media.md) §10):

| Name | Kind | Defined in |
|---|---|---|
| `config` | Source | §3 |
| `local` | Source | [0002](0002-source-graphs-and-mass-ingest.md) §2 |
| `pages` | Source | [0008](0008-namespaces-and-document-pages.md) §4 |
| `log` | Source | [0011](0011-logs.md) §2 |
| `actors`, `accounts` | Source | [0007](0007-actor-identity.md) §8 |
| `mirror/{provider}`, `actors/{provider}`, `log/{provider}` | Source, one set per provider | 0002 §2, 0007 §8, 0011 §2 |
| `log` (instance) | Source: the instance's own log, for operator records | [0039](0039-files-and-media.md) §10 |
| `files/{repo}` | Source, one per mirrored file repository | [0039](0039-files-and-media.md) §11 |
| `derived/{source}` | Source, one per extraction source of a tenant | [0071](0071-derived-statements-from-mirrored-pages.md) §1 |
| `resolved` | Projection: the main graph of [0001](0001-revision-metadata-rdf.md) §2 as [0002](0002-source-graphs-and-mass-ingest.md) §3 computes it | 0002 §3 |
| `metadata` | Projection | 0001 §2 |

`{provider}` is the provider's **slug**, a lower-case name such as `wikidata`, `openalex`, `librarybase` or `internetdomains`. The provider registry entry ([0002](0002-source-graphs-and-mass-ingest.md) §4) records the slug alongside the two-letter code used in IDs.

**Checkpoint origin lines stay `{host}/log/{name}`** ([0006](0006-log-integrity-and-erasure.md) §6), with the tenant's host for a tenant's partitions and `{farm host}/instance/log/{name}` for the instance's ([0018](0018-tenants.md) §2, [0046](0046-primary-tenant.md) §7). They are a transparency-log convention, not a graph IRI, and the two are deliberately different.

**The repository is the authority for names and codes.** The files under `docs/registry/` in the Triplespace repository are the registry of record:

| File | Lists | Defined in |
|---|---|---|
| `graphs.toml` | The reserved graph names above, with each one's kind, policies and payload type; `pages/{repo}`, the mirrored-page partition of a page repository in `mirror` mode, with its five-part payload type `scatter:v0/mirrored-page` ([0053](0053-mirrored-pages.md) §5); `derived/{source}`, a tenant's derived graph per extraction source, with the four-part `scatter:v0/derivation` ([0071](0071-derived-statements-from-mirrored-pages.md) §1, §3) | this section, §3 |
| `providers.toml` | Each provider's two-letter code, slug, **provider number** (§2), type codes with their upstream prefixes and IRI templates, issuer, whether it publishes revision IDs, and its `trust` mode and key-chain URL ([0022](0022-federation.md) §2) | [0000](0000-init.md) §3, [0002](0002-source-graphs-and-mass-ingest.md) §4 |
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
| `file-types.toml` | The permitted file types: extensions, MIME and media types, magic signatures, inline or attachment, thumbnailer | [0039](0039-files-and-media.md) §5 |
| `wikitext-functions.toml` | The variables, parser functions, tags and switches of template expansion | [0042](0042-template-expansion-and-parsoid.md) §5 |
| `special-pages.toml` | Every special page name with its MediaWiki name, aliases, scope and status | [0047](0047-special-pages.md) §1 |
| `css-properties.toml` | The CSS properties and at-rules the `scatter-css` sanitizer allows, with the module each came from | [0055](0055-templatestyles-templatedata-and-page-properties.md) §2 |
| `version.toml` | What `Special:Version` credits and lists that nothing else records: developer, contributors, funders, AI agents, services, feature switches, and the extensions an `origin` names | [0077](0077-special-version.md) §14 |

`scatter-log`, `scatter-providers`, `scatter-actors`, for special pages `triplespace-titles`, and for `version.toml` `triplespace-api-rest` embed these files and ship them as defaults; an instance's `config` partition (§3) starts from them and may diverge. Allocating a new provider code, slug, number or graph name is a change to the file, in a commit; a code is never reused. This is the same rule [0005](0005-crate-organization.md) §5 already applies to the `scatter:` vocabulary through `scatter-vocab`, and it settles [0000](0000-init.md) Q4. The code for internetdomains.wiki ([0009](0009-keyed-entity-types-and-domain.md) Q2) is allocated there when its adapter is written; the slug `internetdomains` is reserved now. MusicBrainz (`MB`, number 4) and the other providers registered since are allocated the same way ([0017](0017-entity-id-grammar.md) §6).

### 6. Document nodes for foreign entities (extends 0001 §1, 0002 §4 and 0009 §6)

*Changed by A14.*

Every entity on the instance has a document node under the instance's base, whatever minted its ID: `data:Q6`, `data:WDQ42` and `data:en.wikipedia.org` are all `{base}/wiki/Special:EntityData/{id}`, as [0009](0009-keyed-entity-types-and-domain.md) §6 already states for Domains. It is what local revisions of the entity specialize, and where the instance's own metadata about the record lives.

For a foreign entity, the document node also points upstream:

```turtle
# graph <{base}/graph/metadata>
data:WDQ42 pav:importedFrom <https://www.wikidata.org/wiki/Special:EntityData/Q42> ;
    pav:hasCurrentVersion <https://www.wikidata.org/wiki/Special:Redirect/revision/123> ;
    pav:retrievedFrom <{base}/job/17> ; pav:importedOn "…"^^xsd:dateTime .
```

`pav:importedFrom` is the term [0001](0001-revision-metadata-rdf.md) §6 reserved for this. Upstream revisions specialize the upstream document node (§4); the instance's document node reaches them through `pav:hasCurrentVersion` and the job.

**The document node is the stable handle.** In the resolved view, every member of a cluster keeps its own document node, and each carries `schema:about` pointing at the cluster's **current canonical** concept IRI ([0004](0004-identity-clusters-and-equivalence.md) §4), beside the redirect-form `owl:sameAs` between concept IRIs. A document node never moves, so `{base}/wiki/Special:EntityData/{id}` for any member ID is what external consumers are told to cite; the SPARQL Update stream ([0032](0032-sparql-update-stream.md)) rewrites the `schema:about` triple when the canonical member changes.

### 7. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed. The table this section first gave is in A1.

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
