# 0006. Log integrity and erasure

- **Status:** Proposed
- **Date:** 2026-09-25
- **Updated:** 2026-11-13 (A34)
- **Author:** James Hare / Claude Opus
- **Changes:** [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md)

## Context

[0005](0005-crate-organization.md) leaves four things for this ADR to specify:

- the canonical encoding that hashes and signatures are computed over (0005 §3, rule 4);
- the integrity policy each log partition carries (0005 §4.2);
- the integrity fields of a record (0005 §4.3);
- how integrity interacts with compaction.

The design has to meet five requirements, which pull against each other:

1. **Tamper evidence.** Anyone holding an export and the instance's public key can check, on another machine, that the local graph's history has not been altered or reordered. This is also the exit test for Scatterbase's first kernel milestone.
2. **Compaction.** Mirror partitions keep only the latest record per entity, and an upstream tombstone erases mirrored data ([0002](0002-source-graphs-and-mass-ingest.md) §2, §5).
3. **Erasure from the local graph.** A legal takedown can require data to leave the log itself. [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md) §4 and [0002](0002-source-graphs-and-mass-ingest.md) all leave this open.
4. **Bulk ingest throughput.** Loads of hundreds of millions of records write segments in parallel ([0002](0002-source-graphs-and-mass-ingest.md) §8.6). Integrity cannot add a sequential step for each record.
5. **Reuse.** Scatterbase's signed claim partition has to fit the same record format without Triplespace implementing Scatterbase's claim rules.

Three terms are used precisely below:

| Term | Meaning | Where it is defined |
|---|---|---|
| **Hide** | Content stays in the log but is not exported and is shown only to administrators | [0001](0001-revision-metadata-rdf.md) §4 |
| **Strike** | A new record retracts earlier assertions; history keeps both | [0002](0002-source-graphs-and-mass-ingest.md) §8.2 (`remove`, `override`) |
| **Erase** | The content is destroyed; a verifiable trace that it existed remains | This ADR (§7) |

## Decision

### 1. Scope: the log, not the claim

*Changed by A5, A16.*

*Current text: [01](../architecture/01-log-and-records.md) §1.1.*

### 2. Encoding and hashing

*Changed by A3, A5, A7, A9, A11, A19, A26.*

*Current text: [01](../architecture/01-log-and-records.md) §1.2, §3.1, §3.2, §3.3.*

### 3. A record is a header and a body

*Changed by A3, A6, A20, A22, A23.*

*Current text: [01](../architecture/01-log-and-records.md) §1.2, §2.1, §2.2, §2.3, §2.4.*

### 4. Integrity policies

*Changed by A14, A24.*

*Current text: [01](../architecture/01-log-and-records.md) §1.2, §6.*

### 5. The Merkle tree and segments

*Changed by A2, A14, A24, A25, A34.*

*Current text: [01](../architecture/01-log-and-records.md) §4.1, §6.*

### 6. Checkpoints and keys

*Changed by A3, A6, A8, A12, A19, A22, A24, A25, A28, A29, A33, A34.*

*Current text: [01](../architecture/01-log-and-records.md) §4.2, §4.3.*

### 7. Erasure

*Changed by A1, A2, A3, A4, A10, A15.*

*Current text: [01](../architecture/01-log-and-records.md) §5.1, §5.2, §5.3, §5.4, §5.5, §6.*

### 8. Edit conflicts per entity

*Changed by A3, A20, A21.*

*Current text: [01](../architecture/01-log-and-records.md) §7.*

### 9. Verification and export

*Changed by A3, A6, A10, A11, A17, A19, A24, A27, A28, A29, A30, A31, A32.*

*Current text: [01](../architecture/01-log-and-records.md) §8.*

#### 9.1 What a bundle holds

*Current text: [01](../architecture/01-log-and-records.md) §8.1.*

#### 9.2 Scopes

*Current text: [01](../architecture/01-log-and-records.md) §8.2.*

#### 9.3 `verify`

*Current text: [01](../architecture/01-log-and-records.md) §8.3.*

#### 9.4 Inclusion proofs

*Current text: [01](../architecture/01-log-and-records.md) §8.4.*

#### 9.5 The authority extract

*Current text: [01](../architecture/01-log-and-records.md) §8.5.*

### 10. Crates

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **The local graph's history can be verified offline** from an export and a public key. Scatterbase's first milestone gets the same check without implementing claim rules.
- **Erasure no longer conflicts with the append-only log.** It leaves an auditable gap instead of a broken chain.
- **Erasure leaves some metadata.** An entity key, a time and a payload type survive. This is the trace MediaWiki's revision deletion and suppression leave (§7), so erasure is no weaker than the practice Wikimedia projects already follow.
- **Mirrors get weaker guarantees than the local graph.** Integrity holds within each segment, but completeness against upstream cannot be proved, by design.
- ~~**Storage must allow deleting one body inside a sealed segment.** This narrows the storage engine choice still open in [0000](0000-init.md).~~ *Holds, per part; the engine is Postgres, where erasure rewrites one row (A2; 0000 Q6).*
- **Each record costs on the order of a hundred extra bytes:** the header, the salt and the commitment. For a mirror of about 100 million entities under `latest`, that is on the order of 10 GB.
- **One hash function runs through both products.** Scatterbase's claim IDs, its blobs and this log all use SHA-256. Independent verifiers need only standard tools, such as `sha256sum` or Python's `hashlib`.
- **Hashing is not the fastest available.** BLAKE3 would be faster on large payloads. Most hashing in the log is over small inputs (headers and 65-byte tree nodes), where hardware-accelerated SHA-256 is competitive, and [0002](0002-source-graphs-and-mass-ingest.md) §8.6 expects parsing, not hashing, to limit ingest.
- ~~**CBOR becomes the storage format.** The JSON wire formats stay. The mapping from Wikibase JSON to CBOR has to be specified and tested against `docs/api/snapshots`.~~ *Specified: structural, and tested by a round trip over `docs/api/snapshots` (A7).*
- **Payload types are versioned contracts.** A change to a payload schema needs a new payload type or version, because old records must still decode strictly.

## Open questions

- **Q1.** ~~**Witnessing.** Whether to cosign checkpoints with external witnesses, and which ones.~~ *Settled by [0081](0081-recovery-keys-and-continuations.md) §6: yes, by any C2SP tlog-witness a tenant lists in `integrity.witnesses`, for its `config` partition by default and other partitions by choice.*
- **Q2. Timestamp anchoring.** Whether checkpoints should be anchored externally, which is Scatterbase's TS product code.
- **Q3.** ~~**Where checkpoints are served,** and whether the path is a well-known URL.~~ *Settled by [0022](0022-federation.md) §1: `{base}/.well-known/tlog/{partition}/checkpoint`, historical checkpoints and manifests beside it, and the key chain at `/.well-known/tlog/keys`.*
- **Q4.** ~~**Defaults** for segment size (k) and checkpoint cadence (N and T). Cadence is a `site` setting since [0015](0015-record-format-and-partition-registry.md) §3; the values are still to be chosen.~~ *Settled by A34: k = 16, N = 4,096 records, T = 60 seconds.*
- **Q5.** ~~**Reason classes.** Their vocabulary, and whether an `erase` record's reason is visible to everyone.~~ *Settled by [0016](0016-permissions-and-access-control.md) §6: `legal`, `privacy`, `upstream` ([0011](0011-logs.md) §5) and `operational`, extensible in `site` configuration; that a record was erased is public, the class and authority reference are visible to `ts-viewerasures`.*
- **Q6. Key compromise.** Revocation and recovery, shared with Scatterbase's identity work.
- **Q7.** ~~**The Wikibase JSON to CBOR mapping.** In particular, which values stay strings and how snak hashes are carried.~~ *Settled by A7: structural and JSON-faithful; every string stays a string; snak and reference hashes and `numeric-id` are dropped and recomputed on output.*
- **Q8. Prior-use acknowledgments for each term.** Whether they eventually move from the attestation into a log-level check shared by both products.

## References

- [RFC 6962 — Certificate Transparency](https://www.rfc-editor.org/rfc/rfc6962), §2.1 (Merkle hash trees)
- [RFC 9162 — Certificate Transparency Version 2.0](https://www.rfc-editor.org/rfc/rfc9162), §2.1
- [RFC 8949 — CBOR](https://www.rfc-editor.org/rfc/rfc8949), §4.2 (deterministically encoded CBOR)
- [MediaWiki revision deletion](https://www.mediawiki.org/wiki/Manual:RevisionDelete)
- [C2SP tlog-checkpoint](https://c2sp.org/tlog-checkpoint), [C2SP signed-note](https://c2sp.org/signed-note), [C2SP tlog-witness](https://c2sp.org/tlog-witness)
- Scatterbase `docs/claim-envelopes.md` (claim hash and signature preimages)

## Amendment log

### A1. The upstream reason class

- **Date:** 2026-09-26
- **Source:** [0011](0011-logs.md) §5
- **Change:** extends §7
- **Summary:** `upstream` joins the reason classes: it marks an erasure that follows upstream hiding something.

### A2. The log in Postgres

- **Date:** 2026-09-26
- **Source:** [0013](0013-postgres-storage.md) §1, §3
- **Change:** extends §5; amends §7
- **Summary:** A segment becomes an offset range in Postgres. Erasure rewrites the target row in the appending transaction; erased bytes linger until `VACUUM` and in WAL archives, which the runbook bounds. Compaction is a batched delete.

Replaced text (§7):

> - Each target's body is deleted: the salt, the payload and the attestation. Segments are rewritten with an erasure marker in place of each body.

### A3. Erasable parts and header IDs

- **Date:** 2026-09-26
- **Source:** [0015](0015-record-format-and-partition-registry.md) §1–3
- **Change:** amends §2, §3, §7, §9; extends §6, §8
- **Summary:** The body is a fixed list of erasable **parts** (content, comment, attestation, and any a payload type adds), each stored as `[salt, bytes]` with a 16-byte salt and its own leaf `H(0x04 ‖ i ‖ salt ‖ bytes)`; the commitment in header field 6 is `H(0x02 ‖ leaf_0 ‖ … ‖ leaf_{n-1})`, and erasure (§7) replaces a part with `[null, leaf]`. Three header fields follow field 6: the global revision ID, log ID and page ID. [0018](0018-tenants.md) §2 makes the partition field 64 random bits. The content hash `0x03` covers the content part alone. Erasure and verification work per part. The configuration partition is `config`. A `baserevid` on a foreign entity is checked across source partitions.

Replaced text (§2):

> Tags `0x00` and `0x01` are the leaf and node prefixes of RFC 6962, so the Merkle tree (§5) is the RFC 6962 tree unchanged. Tags `0x02` and `0x03` are Triplespace's own and never appear inside the tree.

Replaced text (§3):

> - **The body** holds everything that might have to be erased: the payload and the attestation.
>
> | 6 | Body commitment | bstr (32) | `H(0x02 ‖ body)` |
>
> The **body** is a CBOR array:
>
> | # | Field | Meaning |
> |---|---|---|
> | 0 | Salt | 32 random bytes |
> | 1 | Payload | The record's content: a change set for Triplespace, a claim for Scatterbase |
> | 2 | Attestation | Who is responsible for the record |
>
> **The salt keeps erased content from being confirmed.** Without it, anyone holding a header could test a guess at a short erased payload, such as a username or a date of birth, by hashing the guess and comparing it with the commitment. Once the salt is erased with the body, that test is no longer possible.
>
> **Deduplication uses the content hash** (`0x03`), which is computed over the payload without the salt.

Replaced text (§7):

> **Verification.** A verifier reports a missing body that an `erase` record accounts for as erased, naming the erasing offset. A missing body with no `erase` record is a failure.

Replaced text (§9):

> 2. **Bodies.** Every present body against its commitment, and every missing body against an `erase` record.

### A4. Who may erase

- **Date:** 2026-09-26
- **Source:** [0016](0016-permissions-and-access-control.md) §2, §6
- **Change:** amends §7
- **Summary:** Erasing needs `ts-erase`. The reason classes are `legal`, `privacy`, `upstream` and `operational`, extensible in `site` configuration; that a record was erased is public, its class and authority are visible to `ts-viewerasures`. This settled Q5.

Replaced text (§7):

> This settles the **mechanism** for erasure from the local graph, open since [0000](0000-init.md). **Who** may erase remains a permissions question.

### A5. Client signatures

- **Date:** 2026-09-27
- **Source:** [0015](0015-record-format-and-partition-registry.md) §1, its amendment of 2026-09-27
- **Change:** extends §1, §2
- **Summary:** The attestation part may carry a `signature` by the actor's own key over `H(0x05 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment))`; `0x05` is a new domain tag. The §1 table had been edited in place to say so.

### A6. Tenants: partitions, origin lines and moves

- **Date:** 2026-09-27
- **Source:** [0018](0018-tenants.md) §2, §10
- **Change:** amends §3, §6; extends §9
- **Summary:** Partition IDs are 64 random bits. Origin lines use the tenant's host. Each tenant's `config` carries the key chain. Moving a tenant is a key rotation naming the new instance's key and the final checkpoint, with the bundle carrying two extracts.

Replaced text (§3):

> | 1 | Partition | uint | Partition number, assigned when the partition is created |

Replaced text (§6):

> - **Origin line:** `{instance host}/log/{partition name}`, without a scheme. For segment manifests in a `hashed` partition, `{instance host}/log/{partition name}/segment/{n}`.

### A7. The JSON → CBOR mapping

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27
- **Change:** extends §2
- **Summary:** The CBOR of a change set, entity state or page operation is the JSON's structure and nothing else: every JSON string is a CBOR text string (quantity amounts and bounds, time values, coordinate decimals all stay the strings Wikibase gives them), JSON numbers are CBOR integers or floats exactly as JSON typed them, objects are maps with text keys, arrays are arrays, `true`/`false`/`null` are the CBOR simple values. No CBOR tags are used for Wikibase data. Two derivable fields are **dropped** at ingest and recomputed on output: the `hash` on snaks and references, and `numeric-id` beside `id` on entity values. `scatter-wikibase-model` implements Wikibase's own hash computation so that a recomputed reference hash equals Wikidata's for mirrored data, which is what [0003](0003-statement-ui.md) §5 and [0004](0004-identity-clusters-and-equivalence.md) §8 compare by. The test is a round trip: canonical JSON ([wikibase-compat.md](../api/wikibase-compat.md) §3) → CBOR → canonical JSON reproduces the bytes for every file in `docs/api/snapshots`. Because the content hash `0x03` and, since [0015](0015-record-format-and-partition-registry.md) §1 as amended, a client signature are computed over these bytes, this mapping is part of the record format. This settled Q7.

### A8. Where checkpoints are served

- **Date:** 2026-09-27
- **Source:** [0022](0022-federation.md) §1
- **Change:** extends §6
- **Summary:** `{base}/.well-known/tlog/{partition}/checkpoint`, historical checkpoints and manifests beside it, and the key chain at `/.well-known/tlog/keys`. This settled Q3.

### A9. The hash guard

- **Date:** 2026-09-28
- **Source:** Direct: James, decision of 2026-09-28
- **Change:** extends §2
- **Summary:** Dropping `hash` at ingest is safe only while the recomputation matches Wikibase's own, and nothing in a dump says whether it does. The hashes are not decoration: the RDF names reference nodes by the reference hash (`ref:{hash}`, [wikibase-compat.md](../api/wikibase-compat.md) §5.1) and value nodes by an `md5` of the serialized value (`v:{hash}`) that the JSON never carries, and the Action API takes snak and reference hashes as handles (`wbsetreference`, `wbremovereferences`, `wbremovequalifiers`). A faithful implementation is therefore required for local data and for RDF whatever happens to the JSON field; what this amendment adds is a check that it *is* faithful, made on every mirrored entity rather than only on the snapshots. **At ingest, every snak and reference hash is recomputed and compared with the one upstream sent.** When they are equal, the field is dropped as above. When they differ, **upstream's hash is kept in place**: the `hash` key stays in the CBOR map of that snak or reference, so the mapping is still structural, the content hash `0x03` covers it, and no schema changes. On output a present `hash` is emitted as stored and an absent one is recomputed, so what the instance serves equals what upstream served whether or not the implementation is right. Every mismatch is counted by value type in the job's finish record ([0011](0011-logs.md) §6.3), shown on the job page and in `GET /jobs/{id}` ([0012](0012-api-requirements.md) §5), and summed instance-wide as `hash_mismatches` in `siprop=triplespace` ([0012](0012-api-requirements.md) §4); a nonzero count means the emulation has drifted from Wikibase's `serialize()` or upstream has changed its algorithm, and the breakdown says which value types. The `site` setting `ingest.hash_mismatch` is `keep`, the default, or `fail`, which aborts the job at the first mismatch for an instance that would rather stop than store. A kept hash is never used to decide equality: statement fusion ([0004](0004-identity-clusters-and-equivalence.md) §8) and "identical references share a node" compare recomputed hashes, so a drift shows up as a count, never as a silently split node. Adopted entities ([0035](0035-adopting-a-wikibase.md) §3) pass through the same guard, since the source wiki's hashes come from the same code.

### A10. Files

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §8–10, §14
- **Change:** extends §7, §9
- **Summary:** As noted under each section before the migration:
  - §7: An upload record holds the SHA-256 of its file's bytes, which live in a blob store outside the log. Erasing its content part removes that reference, and the bytes are destroyed when no unerased record in their storage scope references them. An operator's **expunge** erases every reference to a hash in every tenant and destroys the bytes at once.
  - §9: Verification gains a blob check at two depths, `presence` and `full`, and export bundles gain `--blobs include|list|omit`. A missing object with no `erase` accounting for it is a failure, as a missing body is.

### A11. Instance attestations

- **Date:** 2026-09-30
- **Source:** [0040](0040-instance-prerogatives.md) §3, §8
- **Change:** extends §2, §9
- **Summary:** As noted under each section before the migration:
  - §2: Tag `0x06` prefixes the preimage of an instance attestation's signature: `H(0x06 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment) ‖ authority)`, signed by the instance key. Like `0x04` and `0x05` it never appears in the header tree.
  - §9: Level 2 also checks every instance attestation's signature against the key chain, with the key current when the record was appended. Bundles carry an **authority extract**: the authority records the tenant's instance attestations cite, with inclusion proofs and with the operator's attestation part withheld.

### A12. Instance origin lines

- **Date:** 2026-09-30
- **Source:** [0046](0046-primary-tenant.md) §7
- **Change:** amends §6
- **Summary:** An instance partition's origin line is `{farm host}/instance/log/{partition name}`, so that it cannot collide with a tenant's when the farm base is a tenant's base; a tenant partition's is `{tenant host}/log/{partition name}` ([0018](0018-tenants.md) §2).

Replaced text: the tenant-host origin line of A6, for instance partitions.

### A13. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–9
- **Summary:** A1–A12 were folded into the Decision, the open questions were numbered, and two consequences were struck. No decision changed. Before this, A3 (in part), A7, A9, A10, A11 and A12 were blockquotes; A5 had been written into §1 in place; the other entries were recorded only in other ADRs. The file before conversion is commit `0b26a3a`.

### A14. Compacted offsets keep their leaf

- **Date:** 2026-10-02
- **Source:** Direct: James, review of 2026-10-02 (`scatter-log`'s file backend, `docs/api/payloads.md` §10)
- **Change:** amends §4 and §5
- **Summary:** Compaction leaves a record's Merkle leaf in its slot, so the tree over every offset still folds after compaction and the `segments` file format needs no separate frontier. Decided when the file backend was written; the Postgres backend already keeps leaves in `log.merkle_node`. A consequence in §4: a segment manifest is unchanged by compaction, so no manifest lists the segments it replaces.

Replaced text (§4): "When compaction replaces segments, the new segment's manifest lists the segments it replaces." §5 gains its last bullet.

### A15. Packed record storage

- **Date:** 2026-10-04
- **Source:** [0058](0058-packed-record-storage.md) §6
- **Change:** extends §7
- **Summary:** In a packed partition, erasing a part also reclaims the fragments only that part used, by a candidate sweep in the erasure job, and a `legal` erasure runs a full sweep of the dedup domain, so shared storage never keeps an erased part alive once no live row holds the same bytes.

### A16. The instance key signs checkpoints and manifests; an actor may sign its record

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §1
- **Summary:** "The instance key is the only signer" becomes: the instance key is the only signer of checkpoints and segment manifests; an actor may additionally sign its own record with the client `signature` of [0015](0015-record-format-and-partition-registry.md) §1, which §1's table already admits. A signed checkpoint still attests only to order of appending, not to who wrote a record. [0005](0005-crate-organization.md) §4.3 is amended the same way. (PENDING A2)

Replaced text (§1):

> Triplespace's instance key is the only signer.

### A17. Bundles carry the key records of client-signed records

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §9
- **Summary:** An export bundle also carries the `scatter:v0/key` records (the `actors` partition, or the key records of every actor whose signatures appear) for each exported partition that holds client-signed records, so that `verify` level 2 can check every client signature with nothing outside the bundle. (PENDING A3)

### A18. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [01](../architecture/01-log-and-records.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A19. Recovery keys, continuations and witnessed key chains

- **Date:** 2026-10-09
- **Source:** [0081](0081-recovery-keys-and-continuations.md) §1, §2, §3, §5, §6, §8
- **Change:** extends §2, §9; amends §6
- **Summary:** Domain tag `0x07` is the preimage of a recovery signature (§2). A tenant may register recovery keys, held by its community, which authorize a **continuation**: a move the old instance did not sign, cosigning the old instance's final roots in one signature per partition instead of re-signing records (§6). A tenant partition's origin line is fixed at the partition's creation or the tenant's arrival on an instance, and an alias does not change it (§6). Witnesses are for the key chain: a tenant's `integrity.witnesses` cosign its `config` checkpoints by default, and an instance may itself be a witness under `witness.enabled` (§6). `verify` reports a tenant's chain of custody at level 1 (§9).

Replaced text (§6, in [01](../architecture/01-log-and-records.md) §4.2):

> - **Origin line,** without a scheme: `{tenant host}/log/{partition name}` for a tenant's partition ([0018](0018-tenants.md) §2),

Replaced text (§6, in [01](../architecture/01-log-and-records.md) §4.3):

> - Recovering from a compromised key is out of scope. It belongs to the identity work that Scatterbase and Triplespace share.
>
> **Witnesses are optional.** An instance may publish its checkpoints to external witnesses that implement [C2SP tlog-witness](https://c2sp.org/tlog-witness). Because the tree is RFC 6962 with SHA-256, a witness can verify consistency proofs between checkpoints and cosign them with no changes.

### A20. The base check is a step of the appending transaction; attestation carries cross-partition decisions

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §1, §6
- **Change:** extends §3, §8
- **Summary:** The base check (§8) is a step of tier 1, the appending transaction, made against the local graph alone, with `view.graph_state` as the local graph's current state; nothing composes inside the transaction. Where the write path decides something from the state of another partition at append time (autopatrol from the actor's memberships, the operator of a transfer), the decision is written into the attestation part of the record and a projection reads it there rather than reconstructing "as of" across partitions, which a vanish could make unreconstructible (§3). 0083's table did not list this ADR; the ledger's G8 names §8 here, and the table was extended. (REVIEW G8, G13)

### A21. `lastrevid`, the per-graph base check and patching

- **Date:** 2026-10-09
- **Source:** [0084](0084-wikibase-writes-against-the-resolved-view.md) §5
- **Change:** amends §8
- **Summary:** The base check is per source graph. A `baserevid` that decodes to the tenant's local partition is checked against the local partition's newest record for the key; one that decodes to a mirror partition asserts only that the key still has no local record, and a mirror advancing is never a conflict, because nothing a local assertion depends on changed. The one operation whose meaning depends on mirrored state, an `override` naming a mirrored statement, is checked by whether that statement still exists in the graph's current state. A stale local base is accepted with the warning `wikibase-conflict-patched` when the change set touches no statement UUID, term, alias or sitelink that a later local record touched; otherwise the write fails with `editconflict`, naming the current `lastrevid`, and HTTP 409 on REST; a base that names a state compaction has since replaced is an `editconflict`. "The check runs against the newest record for the key across the source partitions" is withdrawn, and `lastrevid` is the newest local record's ID when the key has one ([0015](0015-record-format-and-partition-registry.md) §2). (REVIEW G17)

Replaced text ([01](../architecture/01-log-and-records.md) §7, as it stood):

> - The log rejects the change set if a newer record for the key exists in the partition. The Action API reports this as `editconflict`, and the REST API as HTTP 409.

> - For a write to a foreign entity, `baserevid` is decoded to a partition and offset, and the check runs against the newest record for the key across the source partitions, not only the local one. A base that names a state compaction has since replaced is an `editconflict`.
>
> The check needs an index from each key to its latest offset. For the local partition, this is the version cursor.

### A22. The founding attestation

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §3, §6
- **Summary:** The attestation part has a third form, carried by exactly one record per `config` partition: the record at offset 0, the `key:` record that opens the partition's key chain. It names no actor; its `signature` is by the key the record itself registers, so the record is self-certifying; and for a tenant's `config` it carries the `authority` `(instance config, 0)`. The issuer or farm code is derived from the record's leaf afterwards, so nothing that depends on the code exists before it; offsets 1 and 2 of the instance `config` are attested by `instance:{farm code}` with the founding record as authority. A single-tenant instance therefore needs no authority record and no `ts-prerogative` path to exist before its first entity. The instance's first public key is registered under this form, and each tenant's key chain is opened by a founding record of the same form whose authority is the instance's (§6). The ledger names [0015](0015-record-format-and-partition-registry.md) §1 and §3, [0018](0018-tenants.md) §2, [0040](0040-instance-prerogatives.md) §3 and [0079](0079-derived-issuer-codes.md) §2 and §4; the chapter folds it at [01](../architecture/01-log-and-records.md) §2.4 and §4.3, whose provenance is §3 and §6 here, and the ledger row was extended. (REVIEW G22)

### A23. Server-filled values never enter a client-signed part

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §3
- **Summary:** A value the server mints into a record while appending it, such as a thread's talk-page ID and suffixed title or a sync job's `n`, is carried in the attestation map, so that the content and comment parts stay the bytes the client submitted and a client signature over them verifies. The ledger's verb is `amends`, but the chapter text under §3 ([01](../architecture/01-log-and-records.md) §2.4) said nothing this contradicts, so this entry extends; the ledger named §2, which holds the encoding, and was corrected to §3. (REVIEW G23)

### A24. No genesis record; manifests only for `hashed` partitions; a `logid` for every tenant-partition record

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §4; extends §5, §6, §9
- **Summary:** Non-`config` partitions have no genesis record: a partition's name, hash function, `k` and policies are its `graph:` record in `config`, which the export bundle carries (§9), and the partition's first record is an ordinary record at offset 0; in `config` itself that record is the founding record (A22). `log.segment_manifest` is keyed `(partition, segment)`, one per sealed segment and never re-signed, with no `replaces` and no `sealed_at` in the key. A `hashed` partition has segment manifests only and no checkpoints: the "checkpoint" a job writes at its end is the manifest of the last sealed segment plus an unsigned head offset, and records in the open segment are uncommitted until the segment is sealed; `log.checkpoint` rows exist for `logged` partitions only. Every record in `local`, `pages`, the tenant `log` and the tenant `actors` partitions takes a `logid` at append whether or not the catalogue projects it as an event (folded at [01](../architecture/01-log-and-records.md) §2.5, under [0015](0015-record-format-and-partition-registry.md) §2). The chapter section the genesis sentence stood in ([01](../architecture/01-log-and-records.md) §1.2) is shared by §2, §3 and §4 here; the policy it states is §4's. (REVIEW G24)

Replaced text ([01](../architecture/01-log-and-records.md) §1.2, as it stood):

> - a **segment size** 2^k and the **hash function**, fixed at creation. The hash function is named in the partition's genesis record; changing it means starting a new partition.

> | **`hashed`** | Every header, in one Merkle tree for each sealed segment, with a signed manifest for each segment | Allowed | Triplespace's mirror partitions; Scatterbase's foreign partition |

> **`hashed` partitions prove less.** A verifier can show that each record in a live segment is unaltered since the segment was sealed. A compacted offset keeps its leaf, so a segment's tree and its manifest are unchanged by compaction, and the holes are visible. A verifier cannot show that compaction kept the right records. That limit is deliberate: a mirror's source of truth is upstream, and [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2 lets a mirror forget.

### A25. Several processes append; the projection worker signs

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §5; extends §6
- **Summary:** An appending transaction reads the tree's right edge under the partition's append lock and persists only the complete subtrees its append closes, so two API replicas and a bulk job writing one partition never disagree about a node; the right edge is recomputed from the persisted nodes by whoever appends next. Checkpoints and segment manifests are signed by the projection worker under an advisory lock per partition, the only process that holds the instance key; an API replica or a bulk job appends and never signs, and a checkpoint covers only complete subtrees every appender has closed. "Appends in steady state keep the tree's right edge in memory" is withdrawn. The ledger named §6; the fold is at [01](../architecture/01-log-and-records.md) §4.1 (§5 here) and §4.2 (§6), and the ledger row was extended. (REVIEW G25)

Replaced text ([01](../architecture/01-log-and-records.md) §4.1, as it stood):

> - **Appends in steady state** keep the tree's right edge in memory, which is O(log n) hashes. Each append costs one leaf hash plus O(log n) node hashes in the worst case.

### A26. A nesting-depth limit for canonical CBOR

- **Date:** 2026-11-06
- **Source:** Direct: Claude, while building the CBOR depth limit, 2026-11-06; unconfirmed
- **Change:** extends §2
- **Summary:** Arrays and maps may nest at most 128 deep. The codec recurses once per level with nothing else to bound it, and fuzzing found that a release build on a 2 MiB thread stack, tokio's default for a worker, aborts the whole process at a few thousand levels, about 3 KB of nested empty-array heads or 200 KB of nested one-pair maps; records reach the decoder from imports and the ingest API, so that was a remote denial of service. An item that opens an array or map inside 128 containers is rejected at the level that crosses the limit, by the encoder and the decoder alike, so no stored record exceeds it and the bound is part of the format rather than a site setting. 128 is serde_json's recursion limit, so JSON that reaches the codec through its parser already meets it; Wikibase's deepest structure is about ten levels. Three calls were made while building and are for James to confirm: that a bound exists at all, its value, and that it is a fixed format constant instead of a waivable setting. Folded at [01](../architecture/01-log-and-records.md) §3.1.

### A27. Bundles have a scope: export, transfer or backup

- **Date:** 2026-11-07
- **Source:** [0103](0103-export-transfer-and-backup.md) §1
- **Change:** amends §9
- **Summary:** A bundle followed each graph's export policy by default, and a bundle that moved a tenant carried its extracts too, which left a move unable to carry the tenant's `internal` partitions. A bundle now has one of three scopes: an export, which publishes the visible form of what the export policies allow; a transfer, which hands one tenant to another operator, sealed, with every present part; and a backup, the operator's own copy of everything. Chapter text: [01](../architecture/01-log-and-records.md) §8.

Replaced text ([01](../architecture/01-log-and-records.md) §8, as it stood):

> Default exports follow each graph's export policy ([0005](../decisions/0005-crate-organization.md) §4.1). Bundles also take `--blobs include|list|omit` ([0039](../decisions/0039-files-and-media.md) §14). A bundle that moves a tenant carries its extracts as well ([0018](../decisions/0018-tenants.md) §10).

### A28. `triplespace checkpoint` signs until the worker does; `export` and `verify` on the command line

- **Date:** 2026-11-07
- **Source:** Direct: James, design discussion of 2026-11-07
- **Change:** extends §6, §9
- **Summary:** Building `export` and `verify` showed that nothing signed checkpoints: `log.checkpoint` existed and stayed empty, so a bundle had nothing for `verify` to check its tree against. Asked, James chose a command, `triplespace checkpoint`, that signs every partition's new head and every unsigned full segment with the instance key, run by hand, by cron or before an export, until the projection worker signs on a cadence once §6's N and T are chosen (Q4). Of the bundle scopes of [0103](0103-export-transfer-and-backup.md) §1, the backup scope is built with `export` and `verify`; the export scope, which withholds what an anonymous reader may not read, comes next, before a public bundle is served. Chapter text: [01](../architecture/01-log-and-records.md) §4.2, §8.

### A29. Origins fixed by the first checkpoint; what the commands check

- **Date:** 2026-11-07
- **Source:** Direct: Claude, while building `triplespace checkpoint`, `export` and `verify` (SCA-302), 2026-11-07; confirmed by James, 2026-11-08
- **Change:** extends §6, §9
- **Summary:** Calls made building A28. (1) Nothing recorded a partition's origin line, which must not change with an alias (01 §4.2): a partition's origin is taken from its first checkpoint or manifest and reused; the first is derived from the tenant's current host, or the farm's for an instance partition, so a tenant aliased before its first checkpoint would sign under its new host. Recording the origin at partition creation would be exact. (2) The signer takes `pg_advisory_xact_lock(hashtextextended('checkpoint:{partition}', 0))` per partition, the lock the worker is to take, and refuses a key file that is not the chain's current key, or a missing one, rather than generate a key as the other commands do. (3) A `logged` partition is signed when its head has passed its latest checkpoint; a `hashed` one gets a manifest for each full segment that has none, and no checkpoint. (4) `export --scope backup` takes every partition, or one tenant's with `--tenant`, `private` ones included, in one `REPEATABLE READ` snapshot, with the instance `config`'s `key:` records; file bytes wait for the blob store. (5) `verify` defaults to level 2, passes no actor keys (client signatures unchecked) and no part names beyond the three core parts, prints a line per partition, and exits non-zero on any problem; a body changed in the bundle already fails as the bundle is opened. Chapter text: [01](../architecture/01-log-and-records.md) §4.2, §8.

### A30. A bundle lists what it withholds

- **Date:** 2026-11-07
- **Source:** Direct: James, design discussion of 2026-11-07
- **Change:** extends §9
- **Summary:** An export-scope bundle ([0103](0103-export-transfer-and-backup.md) §1) leaves out what an anonymous reader may not read, and level 2 counted every missing part without an `erase` record as a failure. Asked, James chose that a bundle carries a withheld list, `withheld.cbor`, naming by partition and offset the parts it left out, or the whole record written as its leaf alone; `verify` accounts for what the list names and reports a `withheld` count, and anything missing that the list does not name still fails. The list says only which parts are absent, which the API's hidden flags already show. The manifest records the bundle's scope. Chapter text: [01](../architecture/01-log-and-records.md) §8.

### A31. How the export scope decides each record

- **Date:** 2026-11-07
- **Source:** Direct: Claude, while building `export --scope export` (SCA-302), 2026-11-07; confirmed by James, 2026-11-08
- **Change:** extends §9
- **Summary:** Calls made building A30 and [0103](0103-export-transfer-and-backup.md) A1. (1) The export scope is one tenant's (`--tenant` required) and covers its `public` partitions, leaving out any whose graph a restriction hides. (2) Each record is decided from its activity row, row by row with the rules the feed query applies in SQL, written once more as `Hidden::hides_activity` in `triplespace-projections` and tested against the same cases: a row on a target a confidential restriction hides is written as its leaf alone; a suppression event is withheld whole unless the reader is in the group it names; a row whose entity or page the reader may not read by its chain (deleted, or under a moderation restriction) has every part withheld, its header kept; otherwise the content, comment or attestation part is withheld as the row's visibility bits say, and the attestation of a hidden actor. The two copies of the feed's rules can drift; one shared implementation would remove that. (3) A record with no activity row is exported as it is: configuration records and records the activity projection does not cover. (4) `view.activity.read_groups` is written by nothing yet, so read restrictions are found through the chains instead. Chapter text: [01](../architecture/01-log-and-records.md) §8.

### A32. One statement of the row rules, for the feed and the export reader

- **Date:** 2026-11-08
- **Source:** Direct: James, review of A31 on 2026-11-08
- **Change:** extends §9
- **Summary:** A31 (2) wrote the feed's row rules a second time in Rust for the export scope, and said the two copies could drift. James chose one implementation. The SQL is it: the rules that hide a row by its target, a page's or a namespace's restriction, an entity's, its type's or its graph's, an ACL event's key, a revision-deletion event's record, and a suppression event's group (09 §6.5), are one statement over the activity row alone, which the feed appends to its `WHERE` and the export reader selects as boolean columns for each row it decides; one database test runs the pinned cases both ways and asserts they agree. Unifying them found three places the copies had drifted, now settled by the one text: a row with no target kind (an ACL or protection event) was dropped by the feed whenever anything was hidden, since its `NULL` fell through `NOT (…)`, and is kept, as the Rust copy kept it; a hidden `pages` graph switched off the entity, type, graph and ACL-key rules in the feed, and now replaces only the page and namespace rules; and a hidden entity namespace was found by the entity's type in the feed and by its ID's prefix in Rust, and the type lookup stands. A Rust rule table generating both the SQL and a row evaluator was the alternative, rejected because the rules need lookups the row does not carry, so each rule would still be two hand-written halves. Chapter text: 01 §8.2; 09 §6.5.

### A33. A partition's origin is recorded at creation

- **Date:** 2026-11-08
- **Source:** Direct: James, review of A29 on 2026-11-08
- **Change:** amends §6
- **Summary:** A29 (1) fixed a partition's origin line by its first checkpoint or manifest, derived from the tenant's host at that time, so a tenant aliased before its first checkpoint would sign under its new host, against 01 §4.2. James chose the exact fix: the origin is recorded when the partition is created, as a column of its registry row (`log.partition.origin`), derived from the same base the graph IRI is built from, by the one function the signer also uses for a row with none; every signer and bundle writer reads it from there, and only a row without one falls back to its first checkpoint, then to the current host. Two migrations backfill existing rows: the log schema's from the first checkpoint's or manifest's origin line, and the instance's from each tenant's current base in the registry, the best that can be done; a database migrated before its registry projection is current keeps the row empty and is derived at first signing. A field in the `graph:` config record was the alternative, rejected because it would have changed a hashed record's shape for something only the signer reads. Chapter text: 01 §4.2; 03 §2.1 (`log.partition`).

Replaced text ([01](../architecture/01-log-and-records.md) §4.2, as it stood):

> **A partition's origin is fixed by its first checkpoint or manifest** and reused for every later one; the first is derived from the tenant's current host, or the farm's ([0006](../decisions/0006-log-integrity-and-erasure.md) A29).

### A34. Defaults for the segment exponent and the checkpoint cadence

- **Date:** 2026-11-13
- **Source:** Direct: James, design discussion of 2026-11-13
- **Change:** extends §5; amends §6
- **Summary:** James took the three defaults Q4 left open. **k = 16**, segments of 65,536 records: it is what every partition made so far has, a `hashed` partition's open segment is unsigned until it is sealed and a small k keeps that short, and a Wikidata-sized mirror is under two thousand segments. **T = 60 seconds** and **N = 4,096 records** for the steady-state cadence of a `logged` partition: T bounds how long a saved edit waits for a checkpoint that an inclusion proof can cite, and since every checkpoint is kept it also caps a partition at 1,440 a day; N matters only under sustained writing between jobs, which checkpoint at their end. The worker signs only a partition that has grown. Chapter text: 01 §4.1, §4.2. The signing worker itself is not built; `triplespace checkpoint` signs until it is.

Replaced text ([01](../architecture/01-log-and-records.md) §4.2, as it stood):

> **Until that worker signs on a cadence**, whose N and T are still open (0006 Q4), `triplespace checkpoint` signs
