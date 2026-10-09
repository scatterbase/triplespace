# 0006. Log integrity and erasure

- **Status:** Proposed
- **Date:** 2026-09-25
- **Updated:** 2026-10-09 (A19)
- **Author:** James Hare / Claude Opus
- **Changes:** [0000](0000-init.md), [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md)
- **Chapters:** [01](../architecture/01-log-and-records.md), [22](../architecture/22-crates-and-stack.md)

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

*Changed by A3, A5, A7, A9, A11, A19.*

*Current text: [01](../architecture/01-log-and-records.md) §1.2, §3.1, §3.2, §3.3.*

### 3. A record is a header and a body

*Changed by A3, A6.*

*Current text: [01](../architecture/01-log-and-records.md) §1.2, §2.1, §2.2, §2.3, §2.4.*

### 4. Integrity policies

*Changed by A14.*

*Current text: [01](../architecture/01-log-and-records.md) §1.2, §6.*

### 5. The Merkle tree and segments

*Changed by A2, A14.*

*Current text: [01](../architecture/01-log-and-records.md) §4.1, §6.*

### 6. Checkpoints and keys

*Changed by A3, A6, A8, A12, A19.*

*Current text: [01](../architecture/01-log-and-records.md) §4.2, §4.3.*

### 7. Erasure

*Changed by A1, A2, A3, A4, A10, A15.*

*Current text: [01](../architecture/01-log-and-records.md) §5.1, §5.2, §5.3, §5.4, §5.5, §6.*

### 8. Edit conflicts per entity

*Changed by A3.*

*Current text: [01](../architecture/01-log-and-records.md) §7.*

### 9. Verification and export

*Changed by A3, A6, A10, A11, A17, A19.*

*Current text: [01](../architecture/01-log-and-records.md) §8.*

### 10. Crates

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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
- **Q4. Defaults** for segment size (k) and checkpoint cadence (N and T). Cadence is a `site` setting since [0015](0015-record-format-and-partition-registry.md) §3; the values are still to be chosen.
- **Q5.** ~~**Reason classes.** Their vocabulary, and whether an `erase` record's reason is visible to everyone.~~ *Settled by [0016](0016-permissions-and-access-control.md) §6: `legal`, `privacy`, `upstream` ([0011](0011-logs.md) §5) and `operational`, extensible in `site` configuration; that a record was erased is public, the class and authority reference are visible to `ts-viewerasures`.*
- **Q6. Key compromise.** Revocation and recovery, shared with Scatterbase's identity work.
- **Q7.** ~~**The Wikibase JSON to CBOR mapping.** In particular, which values stay strings and how snak hashes are carried.~~ *Settled by A7: structural and JSON-faithful; every string stays a string; snak and reference hashes and `numeric-id` are dropped and recomputed on output.*
- **Q8. Prior-use acknowledgments for each term.** Whether they eventually move from the attestation into a log-level check shared by both products.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0000](0000-init.md) Q6 | §2–3 | settles | 0000 Q6 |
| [0001](0001-revision-metadata-rdf.md) §4 | §7 | amends | 0001 A3 |
| [0002](0002-source-graphs-and-mass-ingest.md) §8.4 | §2 | extends | 0002 A14 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q5 | §7 | settles | 0002 Q5 |
| [0005](0005-crate-organization.md) §2, §4.2, §4.3 | §10 | amends | 0005 A1 |
| [0005](0005-crate-organization.md) §2 | §2 | amends | 0005 A37 |

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
