# 0006. Log integrity and erasure

- **Status:** Proposed
- **Date:** 2026-09-25
- **Author:** James Hare / Claude Opus
- **Amended by:** [0011 — Upstream and local logs](0011-logs.md), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md), [0016 — Permissions and access control](0016-permissions-and-access-control.md), [0018 — Tenants](0018-tenants.md), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§1 settles where checkpoints are served), [0039 — Files, blob storage and foreign file repositories](0039-files-and-media.md) (§8 and §10 extend §7: erasing an upload's content part destroys its bytes once unreferenced; §14 extends §9: blob verification and bundles with blobs), [0040 — Instance prerogatives](0040-instance-prerogatives.md) (§3 extends §2: domain tag `0x06`; §8 extends §9: verifying instance attestations and the authority extract), [0046 — The primary tenant](0046-primary-tenant.md) (§7 amends §6: instance partitions' origin lines are `{farm host}/instance/log/{name}`)
- **Related:** [0000 — Initial proposition](0000-init.md) (§7 settles the removal mechanism its Consequences call for), [0001 — Revision metadata in RDF](0001-revision-metadata-rdf.md) (§4), [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (§2, §5, §8; §7 settles the legal-erasure open question), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (amends §2, §4.2 and §4.3; specifies §3 rule 4)

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

This ADR specifies integrity at the level of the log. It does not specify claim-level rules.

| Concern | Level | Covered here |
|---|---|---|
| Canonical encoding, record hashes, Merkle log, signed checkpoints | Log | Yes |
| Erasure with a verifiable gap | Log | Yes |
| Edit conflicts per entity | Log | Yes (§8) |
| Client signatures, attribution keys, key registration by clients | Claim | No. The attestation slot (§3) has room for them. *Since 2026-09-27 the slot is filled: [0015](0015-record-format-and-partition-registry.md) §1 defines the `signature` field and the `key` record, and [0024](0024-subsidiary-accounts.md) §4 lets subsidiaries use them* |
| Prior-use acknowledgments for each term | Claim | No. The attestation slot has room for them |
| Claim IDs and their preimages | Claim | No. Scatterbase specifies them |

Triplespace's instance key is the only signer. A signed checkpoint means "this instance attests that these records were appended in this order". It does not prove which person wrote a record.

### 2. Encoding and hashing

**Encoding.** Record headers, record bodies and payloads are stored in the **core deterministic encoding of CBOR** ([RFC 8949](https://www.rfc-editor.org/rfc/rfc8949) §4.2.1). The encoding rules are:

- the shortest form for every integer and length;
- definite lengths only;
- map keys sorted by their encoded bytes, with no duplicates;
- only the CBOR tags listed in the payload type's schema.

The stored bytes are the preimage, so no separate canonicalization step exists. The decoder is strict: it re-encodes every item it reads and rejects the item if the bytes differ. A payload that decodes but is not canonical is therefore a verification failure, not a variant.

The NDJSON wire format of [0002](0002-source-graphs-and-mass-ingest.md) §8.7 is unchanged. Ingest converts it to CBOR before appending. Values that JSON carries as strings, such as Wikibase quantity amounts, stay strings.

> **Amended 2026-09-27: the JSON → CBOR mapping is structural.** The CBOR of a change set, entity state or page operation is the JSON's structure and nothing else: every JSON string is a CBOR text string (quantity amounts and bounds, time values, coordinate decimals all stay the strings Wikibase gives them), JSON numbers are CBOR integers or floats exactly as JSON typed them, objects are maps with text keys, arrays are arrays, `true`/`false`/`null` are the CBOR simple values. No CBOR tags are used for Wikibase data. Two derivable fields are **dropped** at ingest and recomputed on output: the `hash` on snaks and references, and `numeric-id` beside `id` on entity values. `scatter-wikibase-model` implements Wikibase's own hash computation so that a recomputed reference hash equals Wikidata's for mirrored data, which is what [0003](0003-statement-ui.md) §5 and [0004](0004-identity-clusters-and-equivalence.md) §8 compare by. The test is a round trip: canonical JSON ([wikibase-compat.md](../api/wikibase-compat.md) §3) → CBOR → canonical JSON reproduces the bytes for every file in `docs/api/snapshots`. Because the content hash `0x03` and, since [0015](0015-record-format-and-partition-registry.md) §1 as amended, a client signature are computed over these bytes, this mapping is part of the record format.

> **Amended 2026-09-28: the hash guard.** Dropping `hash` at ingest is safe only while the recomputation matches Wikibase's own, and nothing in a dump says whether it does. The hashes are not decoration: the RDF names reference nodes by the reference hash (`ref:{hash}`, [wikibase-compat.md](../api/wikibase-compat.md) §5.1) and value nodes by an `md5` of the serialized value (`v:{hash}`) that the JSON never carries, and the Action API takes snak and reference hashes as handles (`wbsetreference`, `wbremovereferences`, `wbremovequalifiers`). A faithful implementation is therefore required for local data and for RDF whatever happens to the JSON field; what this amendment adds is a check that it *is* faithful, made on every mirrored entity rather than only on the snapshots. **At ingest, every snak and reference hash is recomputed and compared with the one upstream sent.** When they are equal, the field is dropped as above. When they differ, **upstream's hash is kept in place**: the `hash` key stays in the CBOR map of that snak or reference, so the mapping is still structural, the content hash `0x03` covers it, and no schema changes. On output a present `hash` is emitted as stored and an absent one is recomputed, so what the instance serves equals what upstream served whether or not the implementation is right. Every mismatch is counted by value type in the job's finish record ([0011](0011-logs.md) §6.3), shown on the job page and in `GET /jobs/{id}` ([0012](0012-api-requirements.md) §5), and summed instance-wide as `hash_mismatches` in `siprop=triplespace` ([0012](0012-api-requirements.md) §4); a nonzero count means the emulation has drifted from Wikibase's `serialize()` or upstream has changed its algorithm, and the breakdown says which value types. The `site` setting `ingest.hash_mismatch` is `keep`, the default, or `fail`, which aborts the job at the first mismatch for an instance that would rather stop than store. A kept hash is never used to decide equality: statement fusion ([0004](0004-identity-clusters-and-equivalence.md) §8) and "identical references share a node" compare recomputed hashes, so a drift shows up as a count, never as a silently split node. Adopted entities ([0035](0035-adopting-a-wikibase.md) §3) pass through the same guard, since the source wiki's hashes come from the same code.

**Hashing.** Every hash is **SHA-256**. This matches Scatterbase's claim IDs and content-addressed blobs, and it makes the Merkle tree exactly the RFC 6962 tree, so existing transparency-log tooling can check it. Each use prefixes its input with a one-byte domain tag, so no hash can be mistaken for another kind:

| Tag | Use |
|---|---|
| `0x00` | Merkle leaf: the hash of a record header (§3) |
| `0x01` | Merkle interior node: the hash of two child hashes |
| `0x02` | Body commitment (§3) |
| `0x03` | Content hash of a payload alone, for deduplication and version cursors ([0002](0002-source-graphs-and-mass-ingest.md) §8.4) |

Tags `0x00` and `0x01` are the leaf and node prefixes of RFC 6962, so the Merkle tree (§5) is the RFC 6962 tree unchanged. Tags `0x02` and `0x03` are Triplespace's own and never appear inside the tree.

The hash function is fixed for a partition when the partition is created, and is named in its genesis record. Changing it means starting a new partition.

**Text forms.** Checkpoints carry hashes in base64, as C2SP requires (§6). IRIs and identifiers carry them in Base32z, matching Scatterbase.

> **Extended by [0040](0040-instance-prerogatives.md) §3.** Tag `0x06` prefixes the preimage of an instance attestation's signature: `H(0x06 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment) ‖ authority)`, signed by the instance key. Like `0x04` and `0x05` it never appears in the header tree.

### 3. A record is a header and a body

This refines [0005](0005-crate-organization.md) §4.3. A record has two parts:

- **The header** is small, is never erased, and is what the Merkle tree commits to. It must not contain personal data.
- **The body** holds everything that might have to be erased: the payload and the attestation.

The **header** is a CBOR array, in this order:

| # | Field | Type | Meaning |
|---|---|---|---|
| 0 | Format version | uint | Version of this layout |
| 1 | Partition | uint | Partition number, assigned when the partition is created |
| 2 | Offset | uint | Position in the partition, starting at 0, with no gaps |
| 3 | Appended at | uint | Microseconds since the Unix epoch, UTC |
| 4 | Payload type | text | For example, `scatter:v0/changeset` |
| 5 | Key | text or null | The entity or subject ID used for compaction and indexing. It must be an identifier, never content |
| 6 | Body commitment | bstr (32) | `H(0x02 ‖ body)` |

The **body** is a CBOR array:

| # | Field | Meaning |
|---|---|---|
| 0 | Salt | 32 random bytes |
| 1 | Payload | The record's content: a change set for Triplespace, a claim for Scatterbase |
| 2 | Attestation | Who is responsible for the record |

> **Amended by [0015](0015-record-format-and-partition-registry.md) §1–2.** The body is a fixed list of erasable **parts** (content, comment, attestation, and any a payload type adds), each stored as `[salt, bytes]` with a 16-byte salt and its own leaf `H(0x04 ‖ i ‖ salt ‖ bytes)`; the commitment in header field 6 is `H(0x02 ‖ leaf_0 ‖ … ‖ leaf_{n-1})`, and erasure (§7) replaces a part with `[null, leaf]`. Three header fields follow field 6: the global revision ID, log ID and page ID. [0018](0018-tenants.md) §2 makes the partition field 64 random bits.

**The salt keeps erased content from being confirmed.** Without it, anyone holding a header could test a guess at a short erased payload, such as a username or a date of birth, by hashing the guess and comparing it with the commitment. Once the salt is erased with the body, that test is no longer possible.

**The attestation is in the body because it can identify people.** In Triplespace it is a CBOR map holding the actor and the job ([0002](0002-source-graphs-and-mass-ingest.md) §8.3). In Scatterbase it holds the attribution and the client and server signatures, and it can later hold prior-use acknowledgments. Nothing below the claim level reads it.

**Deduplication uses the content hash** (`0x03`), which is computed over the payload without the salt. It lives in indexes such as the version cursor, which are projections, never in the header. Erasing a record removes its content hash from those indexes too (§7).

### 4. Integrity policies

This specifies the integrity policy of [0005](0005-crate-organization.md) §4.2. Each partition has one of two policies, fixed when the partition is created:

| Policy | What is committed | Compaction | Used for |
|---|---|---|---|
| **`logged`** | Every header, in one Merkle tree for the whole partition, with signed checkpoints (§5, §6) | Never | Triplespace's local and configuration partitions; Scatterbase's claim, meta and server partitions |
| **`hashed`** | Every header, in one Merkle tree for each sealed segment, with a signed manifest for each segment | Allowed | Triplespace's mirror partitions; Scatterbase's foreign partition |

**`logged` partitions prove that history is complete and in order.** A verifier can show that no record was added, removed, altered or reordered between any two checkpoints.

**`hashed` partitions prove less.** A verifier can show that each record in a live segment is unaltered since the segment was sealed. When compaction replaces segments, the new segment's manifest lists the segments it replaces. A verifier cannot show that compaction kept the right records. That limit is deliberate: a mirror's source of truth is upstream, and [0002](0002-source-graphs-and-mass-ingest.md) §2 lets a mirror forget.

**Retained mirror entities stay in `hashed` partitions.** They are exempt from compaction ([0002](0002-source-graphs-and-mass-ingest.md) §5), but they get no extra commitment. Their durable copy is the record that materializes them into the local graph when the tombstone arrives, and that record is in a `logged` partition.

A mirror with the `full` history policy is still `hashed`. It simply never compacts.

### 5. The Merkle tree and segments

- **The tree is the RFC 6962 Merkle tree** (as updated by [RFC 9162](https://www.rfc-editor.org/rfc/rfc9162) §2.1), with SHA-256 as the hash. Leaves are `H(0x00 ‖ header)`. Inclusion proofs and consistency proofs follow the RFC.
- **Segments hold 2^k records,** except the last one. The exponent k is fixed for each partition when it is created. A full segment is then a complete subtree of the partition's tree.
- **Bulk ingest hashes in parallel.** In bootstrap mode ([0002](0002-source-graphs-and-mass-ingest.md) §8.6), each segment's leaves and subtree root are computed independently. The partition root is then folded from the segment roots. The only sequential step runs once per segment.
- **Appends in steady state** keep the tree's right edge in memory, which is O(log n) hashes. Each append costs one leaf hash plus O(log n) node hashes in the worst case.
- **In a `hashed` partition,** each segment's tree stands alone. Its root is what the segment manifest signs.

### 6. Checkpoints and keys

**Format.** Checkpoints use the [C2SP tlog-checkpoint](https://c2sp.org/tlog-checkpoint) format, signed as a [C2SP signed note](https://c2sp.org/signed-note):

- **Origin line:** `{instance host}/log/{partition name}`, without a scheme. For segment manifests in a `hashed` partition, `{instance host}/log/{partition name}/segment/{n}`.
- **Tree size**, in decimal.
- **Root hash**, in base64.
- **No extension lines.** C2SP recommends against them because monitors cannot audit them.
- **Signature:** the instance's Ed25519 key.

> **Amended by [0046](0046-primary-tenant.md) §7.** An instance partition's origin line is `{farm host}/instance/log/{partition name}`, so that it cannot collide with a tenant's when the farm base is a tenant's base; a tenant partition's is `{tenant host}/log/{partition name}` ([0018](0018-tenants.md) §2).

**When checkpoints are written:**

- at the end of every job ([0002](0002-source-graphs-and-mass-ingest.md) §8.3);
- after every `atomic` batch;
- in steady state, at least every N records or T seconds, whichever comes first. N and T are instance configuration.

Records appended after the latest checkpoint are durable but not yet signed.

Checkpoints are stored beside their partition. They are not log records, since a checkpoint cannot be a leaf of the tree it signs. Every checkpoint is kept, so consistency between any two of them can be proved.

**Keys.**

- The instance's first public key is registered in the first record of the configuration partition. This is the same shape as Scatterbase's server-key registration claim.
- **Rotation** is a configuration record naming the new key, signed by the old key inside its attestation. Checkpoints identify their key by the signed-note key ID.
- Recovering from a compromised key is out of scope. It belongs to the identity work that Scatterbase and Triplespace share.

**Witnesses are optional.** An instance may publish its checkpoints to external witnesses that implement [C2SP tlog-witness](https://c2sp.org/tlog-witness). Because the tree is RFC 6962 with SHA-256, a witness can verify consistency proofs between checkpoints and cosign them with no changes.

### 7. Erasure

**Erasure is a record.** An `erase` record is appended to the same partition as its targets. It names its targets in one of two ways:

- a list of offsets;
- a key, meaning every record in the partition with that key, for erasing a whole entity.

It also carries a reason class (for example, `legal` or `privacy`) and a reference to the authority for the erasure. Its own attestation records who erased.

**Effect on storage.**

- Each target's body is deleted: the salt, the payload and the attestation. Segments are rewritten with an erasure marker in place of each body.
- Headers are kept, so the Merkle tree and every existing checkpoint still verify.
- Projections replay the `erase` record and remove everything derived from the targets: resolved and source-graph triples, metadata-graph revision nodes, content hashes in the version cursor, and search entries. Rebuilding from the log excludes erased bodies automatically.

**What remains after erasure:** each target's header. That is its partition, offset, time appended, payload type, key and body commitment. The commitment is salted (§3), so the erased content cannot be confirmed from it. That something was recorded about key K at time t remains visible.

**This follows MediaWiki's precedent.** Revision deletion and suppression keep the revision's row (its ID, page and timestamp) and hide its content, summary or username. Erasure keeps the same kind of trace, and goes further in one respect. MediaWiki keeps an unsalted SHA-1 of deleted text in its database, which could confirm a guess at that text. An erased record's commitment is salted, so it cannot.

**Verification.** A verifier reports a missing body that an `erase` record accounts for as erased, naming the erasing offset. A missing body with no `erase` record is a failure.

**Later records.** Change sets are deltas, so erasing a record can leave later records that refer to what it created. Replay skips erased records, and projections apply the remaining ones as best they can. To remove an entity entirely, erase by key.

**Copies outside the instance.** The `erase` record travels on the feed like any other record. Mirrors and exports taken before the erasure can comply only if they process it. Backups must be handled operationally in the same way. This ADR cannot enforce either.

**Mirror partitions.** Upstream tombstones followed by compaction still erase mirrored data ([0002](0002-source-graphs-and-mass-ingest.md) §5). `erase` is also available in a mirror partition, for a takedown that cannot wait for the next compaction. Compaction later drops the header too.

**Scatterbase.** Erasing a signed claim's body makes its signatures unverifiable, while its leaf remains. Whether a claim may be erased is Scatterbase policy. Its decision record routes destructible data to the `mutable` graph.

This settles the **mechanism** for erasure from the local graph, open since [0000](0000-init.md). **Who** may erase remains a permissions question.

> **Extended by [0039](0039-files-and-media.md) §8–10.** An upload record holds the SHA-256 of its file's bytes, which live in a blob store outside the log. Erasing its content part removes that reference, and the bytes are destroyed when no unerased record in their storage scope references them. An operator's **expunge** erases every reference to a hash in every tenant and destroys the bytes at once.

### 8. Edit conflicts per entity

A change set in the local graph may carry a **base**: the offset of the latest record for that key that the client saw.

- The log rejects the change set if a newer record for the key exists in the partition. The Action API reports this as `editconflict`, and the REST API as HTTP 409.
- A base is **required** for replacing a local entity wholesale ([0002](0002-source-graphs-and-mass-ingest.md) §8.2).
- A base is **optional** for everything else, as `baserevid` is in Wikibase. Bulk jobs that merge by default do not send one.

The check needs an index from each key to its latest offset. For the local partition, this is the version cursor.

Scatterbase's prior-use acknowledgments are the same check applied to each term instead of each entity. They are not implemented here. They can be carried in the attestation and checked by Scatterbase's own acceptance code.

### 9. Verification and export

**An export bundle** holds, for each exported partition:

- its segments, with headers and bodies;
- every checkpoint and segment manifest;
- the configuration records that register and rotate keys.

It needs nothing else to verify. Default exports follow each graph's export policy ([0005](0005-crate-organization.md) §4.1).

**`verify` checks three levels:**

1. **Structure.** Strict CBOR decoding, leaf hashes, the root at every checkpoint, checkpoint signatures, consistency proofs between successive checkpoints, and gapless offsets.
2. **Bodies.** Every present body against its commitment, and every missing body against an `erase` record.
3. **Projections** (optional and expensive). Rebuild projections from the log and compare them with the stored ones.

**Inclusion proofs** are available for any record against any later checkpoint. With one, a third party can check that a particular revision is in the log without holding the whole log.

> **Extended by [0039](0039-files-and-media.md) §14.** Verification gains a blob check at two depths, `presence` and `full`, and export bundles gain `--blobs include|list|omit`. A missing object with no `erase` accounting for it is a failure, as a missing body is.

> **Extended by [0040](0040-instance-prerogatives.md) §8.** Level 2 also checks every instance attestation's signature against the key chain, with the key current when the record was appended. Bundles carry an **authority extract**: the authority records the tenant's instance attestations cite, with inclusion proofs and with the operator's attestation part withheld.

### 10. Crates

- **`scatter-log`** owns the header and body formats, the two policies, segment layout, and the Merkle tree.
- **A new crate, `scatter-integrity`,** owns checkpoint signing and parsing, inclusion and consistency proofs, erasure bookkeeping, export bundles and `verify`. It depends on `scatter-log`.

[0005](0005-crate-organization.md) §2 is updated to add it.

## Consequences

- **The local graph's history can be verified offline** from an export and a public key. Scatterbase's first milestone gets the same check without implementing claim rules.
- **Erasure no longer conflicts with the append-only log.** It leaves an auditable gap instead of a broken chain.
- **Erasure leaves some metadata.** An entity key, a time and a payload type survive. This is the trace MediaWiki's revision deletion and suppression leave (§7), so erasure is no weaker than the practice Wikimedia projects already follow.
- **Mirrors get weaker guarantees than the local graph.** Integrity holds within each segment, but completeness against upstream cannot be proved, by design.
- **Storage must allow deleting one body inside a sealed segment.** This narrows the storage engine choice still open in [0000](0000-init.md).
- **Each record costs on the order of a hundred extra bytes:** the header, the salt and the commitment. For a mirror of about 100 million entities under `latest`, that is on the order of 10 GB.
- **One hash function runs through both products.** Scatterbase's claim IDs, its blobs and this log all use SHA-256. Independent verifiers need only standard tools, such as `sha256sum` or Python's `hashlib`.
- **Hashing is not the fastest available.** BLAKE3 would be faster on large payloads. Most hashing in the log is over small inputs (headers and 65-byte tree nodes), where hardware-accelerated SHA-256 is competitive, and [0002](0002-source-graphs-and-mass-ingest.md) §8.6 expects parsing, not hashing, to limit ingest.
- **CBOR becomes the storage format.** The JSON wire formats stay. The mapping from Wikibase JSON to CBOR has to be specified and tested against `docs/api/snapshots`.
- **Payload types are versioned contracts.** A change to a payload schema needs a new payload type or version, because old records must still decode strictly.

## Open questions

- **Witnessing.** Whether to cosign checkpoints with external witnesses, and which ones.
- **Timestamp anchoring.** Whether checkpoints should be anchored externally, which is Scatterbase's TS product code.
- ~~**Where checkpoints are served,** and whether the path is a well-known URL.~~ *Settled by [0022](0022-federation.md) §1: `{base}/.well-known/tlog/{partition}/checkpoint`, historical checkpoints and manifests beside it, and the key chain at `/.well-known/tlog/keys`.*
- **Defaults** for segment size (k) and checkpoint cadence (N and T). Cadence is a `site` setting since [0015](0015-record-format-and-partition-registry.md) §3; the values are still to be chosen.
- ~~**Reason classes.** Their vocabulary, and whether an `erase` record's reason is visible to everyone.~~ *Settled by [0016](0016-permissions-and-access-control.md) §6: `legal`, `privacy`, `upstream` ([0011](0011-logs.md) §5) and `operational`, extensible in `site` configuration; that a record was erased is public, the class and authority reference are visible to `ts-viewerasures`.*
- **Key compromise.** Revocation and recovery, shared with Scatterbase's identity work.
- ~~**The Wikibase JSON to CBOR mapping.** In particular, which values stay strings and how snak hashes are carried.~~ *Settled 2026-09-27 (§2 amendment): structural and JSON-faithful; every string stays a string; snak and reference hashes and `numeric-id` are dropped and recomputed on output.*
- **Prior-use acknowledgments for each term.** Whether they eventually move from the attestation into a log-level check shared by both products.

## References

- [RFC 6962 — Certificate Transparency](https://www.rfc-editor.org/rfc/rfc6962), §2.1 (Merkle hash trees)
- [RFC 9162 — Certificate Transparency Version 2.0](https://www.rfc-editor.org/rfc/rfc9162), §2.1
- [RFC 8949 — CBOR](https://www.rfc-editor.org/rfc/rfc8949), §4.2 (deterministically encoded CBOR)
- [MediaWiki revision deletion](https://www.mediawiki.org/wiki/Manual:RevisionDelete)
- [C2SP tlog-checkpoint](https://c2sp.org/tlog-checkpoint), [C2SP signed-note](https://c2sp.org/signed-note), [C2SP tlog-witness](https://c2sp.org/tlog-witness)
- Scatterbase `docs/claim-envelopes.md` (claim hash and signature preimages)
