# 0081. Recovery keys, continuations and witnessed key chains

- **Status:** Proposed
- **Date:** 2026-10-09
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0015](0015-record-format-and-partition-registry.md), [0018](0018-tenants.md), [0022](0022-federation.md), [0079](0079-derived-issuer-codes.md)
- **Uses:** [0016](0016-permissions-and-access-control.md), [0021](0021-notifications.md), [0027](0027-preferences-and-portability.md), [0040](0040-instance-prerogatives.md), [0080](0080-tenants-as-entity-sources.md)

## Context

A tenant moves cooperatively by a key rotation the old instance signs ([0018](0018-tenants.md) §10): the old key names the new instance's key and the tenant's final checkpoint, and every record before and after verifies. A **non-cooperative** move, made when the old host has gone or refuses, had one sentence: "the bundle comes from backups, the rotation record carries no signature from the old key, verifiers are told the chain has an unsigned link, and reclaiming is manual."

James asked on 2026-10-09 whether an export mode could re-sign the revisions as a batch under a new key. Working through it showed what a non-cooperative move does and does not lose:

- **The old host's signatures survive it.** The host signs checkpoints, which are roots over every record, not records one by one ([0006](0006-log-integrity-and-erasure.md) §6). A bundle carries every checkpoint ([0006](0006-log-integrity-and-erasure.md) §9), so everything up to the old host's last checkpoint still verifies against its key after the host has gone.
- **Re-signing each record would lose information.** It would replace "host A attested these, in this order" with "host B says so". The batch James had in mind is cheaper and keeps A's attestation: B cosigns A's final root for each partition, one signature per partition.
- **Two things are missing.** First, records appended after A's last checkpoint, which only B can vouch for. Second, authority. Anyone holding a tenant's public bundle can make the same cosignature and claim to be its continuation, and nothing in the chain says which claimant the wiki's community chose. Every key in a tenant's chain is a host's ([0018](0018-tenants.md) §2), so in key terms the host owns the wiki's identity. That conflicts with the principle of [0079](0079-derived-issuer-codes.md): the host vouches, the wiki names.

The same discussion found that witnessing the founding record itself adds little, since [0079](0079-derived-issuer-codes.md)'s code is self-certifying. What witnesses add is the *key chain*: a host showing different chains to different readers becomes detectable, and a witnessed checkpoint fixes when a record existed. A tenant's `config` partition is small and changes rarely, so witnessing it alone is cheap. That is a narrower answer to [0006](0006-log-integrity-and-erasure.md) Q1 and [0022](0022-federation.md) Q4 than witnessing every partition.

`did:plc` is the model: rotation keys held by the identity's owner, a recovery window during which a higher authority can override, and a public log that makes rival histories visible.

### Direction

James's direction, from the design discussion of 2026-10-09:

- **"The situation of non-cooperative moves seems like something that should be thought through more. The cryptographic provenance system is great to confirm that the move from Host A to Host B did not include any tampering in the process. However, if it's one done without the consent of the host, they are unable to sign the records in the way before."**
- **"I wonder if it's possible to add an export mode that would re-sign the revisions as a batch under a new key."**
- **"Make that the next ADR please"**: the continuation record, the recovery key and witnessing of key chains.

## Decision

### 1. The recovery key (extends 0006 §6; extends 0015 §3)

**A tenant may register recovery keys: keys its community holds, which no host ever sees.** It is a `config` record of kind **`recovery-key`**, code `recovery-key`, in the tenant's `config` partition, holding:

| Field | Meaning |
|---|---|
| `keys` | One or more Ed25519 public keys, each with its signed-note key ID |
| `threshold` | How many of `keys` must sign (§2) for an act of recovery: `k`, from 1 to the number of keys. Default 1 |
| `delay` | How long a recovered continuation waits before readers act on it (§4). Default 72 hours, at least 24 |
| `signatures` | The recovery signatures (§2) that authorize this record |

**Who signs which record:**

- **Registering the first set** is an act of the tenant's `owner` ([0016](0016-permissions-and-access-control.md) §3). The record carries a signature by **every** key it lists, as proof of possession.
- **Replacing the set**, including changing `threshold` or `delay`, carries signatures by `threshold` keys of the **current** set and by every newly listed key.
- **Removing it** (a null entry) carries signatures by `threshold` keys of the current set.

The server refuses a record that lacks them with `ts-recovery-signature`, and `verify` treats such a record as absent. A host can append records under any account, but it cannot forge these signatures. So once a set is registered, no host can change it, which is the point.

**Keys are generated off the instance.** `triplespace-cli recovery-key generate` writes the private key to a file on the holder's machine. Registration sends only public keys and signatures. A community is advised to register its set soon after the tenant is created, and to spread `k`-of-`n` keys across people.

### 2. Recovery signatures (extends 0006 §2)

A recovery signature is an Ed25519 signature over `H(0x07 ‖ code ‖ entry)`:

- `code` is the tenant's issuer code as ASCII ([0079](0079-derived-issuer-codes.md) §1).
- `entry` is the canonical CBOR of the record's content with the `signatures` field removed.
- `0x07` is a new domain tag, outside the header tree like `0x02`–`0x06`.

The signatures live in the content part, not the attestation, so that erasing a record's attestation, which can identify people, never makes its authority unverifiable. `config` records are public and are not erased in the ordinary course.

### 3. A continuation (amends 0018 §10; extends 0006 §6)

**A non-cooperative move appends a continuation instead of an unsigned rotation.** When an instance B imports a tenant whose old instance A did not sign a rotation, B appends to the tenant's `config` a record of kind **`continuation`**, code `continuation:{n}`, numbered from 1 per tenant. It holds:

| Field | Meaning |
|---|---|
| `from` | For each partition in the bundle: A's last signed checkpoint that B continues from, verbatim as a signed note (origin, size, root and A's signature lines, with any witness cosignatures) |
| `tail` | For each partition: the offset range of records after that checkpoint, which only B attests |
| `key` | B's instance public key, which becomes the current key of the tenant's chain |
| `origin_host` | The tenant's host on B, which names the new origin lines (§5) |
| `mode` | `recovered`, if `signatures` meets the threshold of the tenant's recovery-key record as of `from`; otherwise `unauthorized` |
| `signatures` | Recovery signatures (§2), when there are any |

The comment part holds B's reason.

**B signs it.** The record carries an instance attestation by B's operator ([0040](0040-instance-prerogatives.md) §3), whose authority is the import job's record in B's instance `log`. B's instance-key signature over it covers every root in `from`. That is the batch re-signing: one signature per partition, over A's own roots, leaving A's signatures in place. No record is re-signed and nothing in the log changes.

**The tail is attested by B alone.** Records after A's last checkpoint verify structurally: they extend A's tree, consistently with the checkpoint in `from`. But only B's checkpoints sign them, and `verify` reports their range as attested by B only (§8). The history and diff pages show nothing different; the distinction is one of provenance, not of content.

**B must continue from the latest checkpoint it can obtain.** That means the bundle's, A's still-served `/.well-known/tlog/…/checkpoint`, or a witness's (§6). A continuation from an older checkpoint than one A published is a **fork**: A's later records are lost to it, and `verify` reports both histories. A record cannot be invented to fill the gap, because the tree forbids it.

**Recovery signatures can be gathered before B is chosen.** The signed `entry` names B's key and host, so holders sign after B has prepared the import. `triplespace-cli tenant import --continue` writes the draft, `triplespace-cli recovery sign` signs it on a holder's machine, and the import appends the record once the threshold is met. Without signatures, B may still continue as `unauthorized`, which is today's unsigned link made explicit.

**Cooperative moves are unchanged.** Recovery keys may sign a cooperative rotation too, and doing so is recorded, but it is not required.

### 4. The delay and cancellation

**A recovered continuation waits.** B serves the tenant from the moment of import. But readers on other instances (§7) act on a `recovered` continuation only once the recovery-key record's `delay` has passed since the continuation's first witnessed checkpoint (§6). Without witnesses, the delay runs from the record's own time, which B controls. That is one reason to witness `config`.

**Within the delay, it can be cancelled.** A `config` record of kind **`continuation-cancel`**, code `continuation-cancel:{n}`, signed by `threshold` keys of the set that authorized continuation `n`, voids it, and readers never act on it. The cancel can be appended by any instance holding the tenant, including A if it is still running, and is carried to the others by witnesses and by readers that see it. This is the defence against a stolen key: the community has `delay` to notice and cancel. A thief who holds `threshold` keys can cancel the community's cancel by a new continuation, so `k > 1` is the stronger setting (Q1).

**A host has no veto.** A's instance key can neither cancel nor contest a recovered continuation. Leaving a host without its consent is the case this ADR exists for.

### 5. Origin lines are fixed per host (amends 0006 §6 and 0018 §2)

A tenant partition's checkpoint origin line was `{tenant host}/log/{partition name}`, read from the current host. **It is now fixed when the partition is created, or when the tenant arrives on an instance by a move or a continuation**, from the tenant's host at that moment. An `alias` record ([0018](0018-tenants.md) §9) does not change it.

This is the stability a witness needs. A witness tracks one log per origin and one key, and an origin that moved with every rename would look to it like a new log. After a move, the new origin begins at the size the rotation or continuation names. The record that links the two origins is the tenant's own key chain.

### 6. Witnessing key chains (extends 0006 §6; settles 0006 Q1 and 0022 Q4)

**A tenant may have its key chain witnessed.** The tenant `site` setting **`integrity.witnesses`** lists witnesses, each with the URL of a C2SP tlog-witness endpoint and the witness's signed-note verifier key. With it set:

- The instance submits every checkpoint of the tenant's `config` partition to each witness, with the consistency proof from the witness's last-seen size.
- It stores each cosignature beside the checkpoint, and serves it as an additional signature line at `/.well-known/tlog/config/checkpoint` and with the historical checkpoints.
- Other partitions are submitted only if listed in **`integrity.witness_partitions`**. The default is `config` alone, which carries the founding record, the key chain, the recovery-key records and every continuation.

A witness refuses a checkpoint inconsistent with one it has cosigned. So with witnesses, a host cannot show two key chains for one origin, and a continuation's `delay` runs on witnessed time. A key chain's founding record is witnessed with the first `config` checkpoint, which settles what [0079](0079-derived-issuer-codes.md) Q4 asked.

**A Triplespace instance can be a witness.** With the instance setting **`witness.enabled`**, `triplespace-server` serves the C2SP tlog-witness API at the farm base, for the origins and keys its operator lists. It keeps its last cosigned size and root per origin in durable state that is never rolled back. So federation partners may witness each other's key chains, which answers [0022](0022-federation.md) Q4.

Which witnesses a tenant uses is its own choice. Public witness networks that implement C2SP tlog-witness need no change, since the tree is RFC 6962 ([0006](0006-log-integrity-and-erasure.md) §6).

### 7. Readers (extends 0022 §2)

`scatter-adapter-triplespace` verifies a provider's key chain as it verifies records ([0022](0022-federation.md) §2), and for a tenant source checks its founding record ([0080](0080-tenants-as-entity-sources.md) §2). Meeting a continuation, it acts by mode:

- **`recovered`:** it follows B once the delay has passed with no cancel. Until then it keeps the provider where it was and reports the pending continuation on `Special:Providers`.
- **`unauthorized`:** it stops the sync job and notifies the job's operator ([0021](0021-notifications.md) §2, reason `job`). An administrator of the reading tenant may accept the continuation with `ts-config`, which is recorded as a `provider/accept-continuation` log event in the reader's `log` and resumes the sync. Nothing is followed silently.
- **A fork**, where A has published a checkpoint the continuation does not include: the adapter stops and reports both. An administrator chooses which to follow, by the same event.

### 8. Verification (extends 0006 §9)

Level 1 of `verify` ([0006](0006-log-integrity-and-erasure.md) §9) gains a **chain of custody** for the tenant:

- the tenant's origins in order, each with its key and the record that links it to the next: a signed rotation, a `recovered` continuation (with whether its delay has passed and whether it was cancelled) or an `unauthorized` one;
- each partition's tail ranges attested only by the host that appended them;
- the witness cosignatures on each `config` checkpoint;
- any fork.

Recovery-key records are checked against their signatures, and a record without valid ones is reported and ignored. A bundle verifies as before; the chain of custody is what it reports beside a pass.

### 9. Configuration and API (extends 0015 §3)

- **Kinds:** `recovery-key`, `continuation` and `continuation-cancel`, defined by `scatter-log` beside `key`, since Scatterbase's server identity needs the same three.
- **Tenant `site` settings:** `integrity.witnesses` (default empty) and `integrity.witness_partitions` (default `["config"]`).
- **Instance `site` setting:** `witness.enabled` (default off), with its list of witnessed origins and keys.
- **`/.well-known/tlog/keys`** ([0022](0022-federation.md) §1) serves the recovery-key, continuation and cancel records with the key records, so a reader has the whole chain of custody from one URL.

### 10. Crates (extends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-log` | The `recovery-key`, `continuation` and `continuation-cancel` kinds |
| `scatter-integrity` | Recovery signatures and tag `0x07`; continuation drafting and checking; witness submission and cosignature storage; the chain of custody in `verify`; origins fixed per host |
| `scatter-adapter-triplespace` | Acting on continuations by mode, the delay, cancels and forks |
| `triplespace-server`, `triplespace-cli` | The tlog-witness endpoint under `witness.enabled`; `recovery-key generate`, `register` and `replace`; `recovery sign`; `tenant import --continue`; `continuation cancel` |

## Alternatives considered

- **Re-sign every record under the new key.** It would erase the old host's attestation, cost a signature per record, and prove nothing a cosignature of the roots does not.
- **Accept unsigned links as they were.** Any holder of a public bundle could claim a wiki's identity, and readers could not tell the community's continuation from an impostor's.
- **Let the old host contest a recovery.** It would give the host the veto that this ADR exists to remove.
- **Make the recovery key the tenant's only key**, signing every checkpoint. Then a community would need a key online to run its wiki. The host's key signs daily operation; the community's key only authorizes changes of host.
- **A central directory of key chains**, as `did:plc` has. It would recentralise what [0079](0079-derived-issuer-codes.md) removed. Witnesses give the same split-view protection with no single operator.
- **Witness every partition.** It is possible with `integrity.witness_partitions`, but costs a submission per checkpoint of busy partitions, for little more than the key chain already gives.

## Consequences

- **A wiki's community can leave any host.** With recovery keys, a move without the host's consent is a signed link, as strong as a cooperative one.
- **Nothing is re-signed.** Every record keeps the attestation of the host that appended it, and the old host's signatures stay verifiable after it is gone.
- **Impostors are visible.** A continuation without the community's signatures is marked `unauthorized`, and readers do not follow it unless an administrator decides to.
- **Forks are explicit.** A rival history is shown as one, never merged and never silently chosen.
- **Witnessing becomes cheap and targeted:** one small partition per tenant, by any C2SP witness, including federation partners.
- **Communities must keep keys.** A lost recovery set cannot be replaced without `threshold` of its keys. A tenant without one is no worse off than before this ADR.
- **The instance key's own compromise is still [0006](0006-log-integrity-and-erasure.md) Q6.** A tenant with recovery keys can, however, continue to a new key on the same instance, which limits a compromise to the tenants without them.

## Open questions

- **Q1. Priority among keys.** Whether keys in a set should be ranked, as `did:plc` ranks rotation keys, so that a higher key can override a continuation signed by lower ones within the delay. That would end the cancel-and-reissue contest a thief with `threshold` keys could start.
- **Q2. Witness discovery.** Whether a tenant's witnesses should be listed in its `config`, so that a reader can find cosignatures independently of the host serving them.
- **Q3. Private data on a recovered move.** Bindings and the private extract do not travel without the old host ([0018](0018-tenants.md) §10). Whether users may carry their own sealed extract ([0027](0027-preferences-and-portability.md) §4) so that reclaiming after a recovery is not manual.
- **Q4. The instance's own recovery.** Whether the instance `config` chain should have recovery keys too, held by the operator offline.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §10 | extends | 0005 A88 |
| [0006](0006-log-integrity-and-erasure.md) §2 | §2 | extends | 0006 A19 |
| [0006](0006-log-integrity-and-erasure.md) §6 | §1, §3, §5, §6 | amends | 0006 A19 |
| [0006](0006-log-integrity-and-erasure.md) §9 | §8 | extends | 0006 A19 |
| [0006](0006-log-integrity-and-erasure.md) Q1 | §6 | settles | 0006 Q1 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §1, §9 | extends | 0015 A47 |
| [0018](0018-tenants.md) §2 | §5 | amends | 0018 A22 |
| [0018](0018-tenants.md) §10 | §3 | amends | 0018 A22 |
| [0022](0022-federation.md) §1 | §9 | extends | 0022 A10 |
| [0022](0022-federation.md) §2 | §7 | extends | 0022 A10 |
| [0022](0022-federation.md) Q4 | §6 | settles | 0022 Q4 |
| [0079](0079-derived-issuer-codes.md) Q4 | §6 | settles | 0079 Q4 |

## References

- [did:plc method specification](https://web.plc.directory/spec/v0.1/did-plc): rotation keys, the 72-hour recovery window, and the operation log
- [C2SP tlog-witness](https://c2sp.org/tlog-witness), [C2SP tlog-checkpoint](https://c2sp.org/tlog-checkpoint), [C2SP signed-note](https://c2sp.org/signed-note): the witness protocol, checkpoints and cosignature lines
- `claude/tenants-as-entity-sources.md` (Triplespace project notes, 2026-10-09): the analysis this ADR records
