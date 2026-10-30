# 0081. Recovery keys, continuations and witnessed key chains

- **Status:** Proposed
- **Date:** 2026-10-09
- **Updated:** 2026-10-30 (A1)
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

*Current text: [01](../architecture/01-log-and-records.md) §4.3; [08](../architecture/08-tenants-and-instances.md) §6.8.*

### 2. Recovery signatures (extends 0006 §2)

*Current text: [01](../architecture/01-log-and-records.md) §3.3.*

### 3. A continuation (amends 0018 §10; extends 0006 §6)

*Current text: [01](../architecture/01-log-and-records.md) §4.3; [08](../architecture/08-tenants-and-instances.md) §6.3, §6.8.*

### 4. The delay and cancellation

*Current text: [08](../architecture/08-tenants-and-instances.md) §6.8.*

### 5. Origin lines are fixed per host (amends 0006 §6 and 0018 §2)

*Current text: [01](../architecture/01-log-and-records.md) §4.2; [08](../architecture/08-tenants-and-instances.md) §2.1.*

### 6. Witnessing key chains (extends 0006 §6; settles 0006 Q1 and 0022 Q4)

*Current text: [01](../architecture/01-log-and-records.md) §4.2, §4.3.*

### 7. Readers (extends 0022 §2)

*Current text: [17](../architecture/17-federation-and-publication.md) §1.2.*

### 8. Verification (extends 0006 §9)

*Current text: [01](../architecture/01-log-and-records.md) §8.*

### 9. Configuration and API (extends 0015 §3)

*Current text: [17](../architecture/17-federation-and-publication.md) §1.1; [23](../architecture/23-configuration-and-registry.md) §2.1, §3.2, §3.3.*

### 10. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

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

## References

- [did:plc method specification](https://web.plc.directory/spec/v0.1/did-plc): rotation keys, the 72-hour recovery window, and the operation log
- [C2SP tlog-witness](https://c2sp.org/tlog-witness), [C2SP tlog-checkpoint](https://c2sp.org/tlog-checkpoint), [C2SP signed-note](https://c2sp.org/signed-note): the witness protocol, checkpoints and cosignature lines
- `claude/tenants-as-entity-sources.md` (Triplespace project notes, 2026-10-09): the analysis this ADR records

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [01](../architecture/01-log-and-records.md), [08](../architecture/08-tenants-and-instances.md), [17](../architecture/17-federation-and-publication.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
