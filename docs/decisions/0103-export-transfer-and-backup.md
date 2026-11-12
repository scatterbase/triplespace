# 0103. Export, transfer and backup

- **Status:** Proposed
- **Date:** 2026-11-07
- **Updated:** 2026-11-12 (A4)
- **Author:** James Hare / Claude Opus
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0011](0011-logs.md), [0013](0013-postgres-storage.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0027](0027-preferences-and-portability.md), [0056](0056-security-model.md)
- **Uses:** [0039](0039-files-and-media.md), [0081](0081-recovery-keys-and-continuations.md)

## Context

Every graph has an export policy (0005 §4.1): `public`, published to external stores; `internal`, served by the API but never exported; `private`, never projected, placed on a feed or exported (0007 §8). The policies were written with publication in mind, and the documents then used "export" for every copy of the data that leaves the live database, which made them say more than intended:

- Moving a tenant (0018 §10) carries its `log` and `actors` partitions, which are `internal`, "as an export bundle", while export bundles follow each graph's export policy (0006 §9). Read literally, a move could not carry the tenant's own log.
- The move bundle holds hidden, suppressed and restricted parts in full, but only the private extract was sealed to the receiving instance (0027 §4); the bundle itself travelled however the operators chose.
- Rehosting an instance carries `private` (0018 §10), which "never exported" seemed to forbid.
- The full dump (0013 §8), served at `{base}/dumps/` (0022 §1), carries the `metadata` graph, which was `internal`, and was described as the instance's own export "for backups"; 0023 §7 called it "a full dump made by the instance for itself".

Building the dumps (SCA-301) and discussing how to relaunch an instance or a tenant elsewhere brought these together. The `metadata` graph was kept out of external stores to keep the external contract simple (0001's Consequences), not because it is sensitive: what it holds, revision metadata and log events, the API already shows, and Wikimedia publishes the same in its stub-meta-history dumps.

### Direction

James's direction, from the design discussion of 2026-11-07:

- **"I agree with naming the bundle scopes explicitly."**
- **"I am fine with keeping the grace period for trusted-move tenants in case the tenant has to move back."**
- **"Your described behavior of restricted content on tenants moved to new instances sounds correct; you need to verify you are who you say you are to get access to your old privileges."**
- **"I guess then the differentiation is between export and *backup*"**
- On reclassifying `metadata` as exportable: **"Yes, full dump only"**.
- Asked how a move the old operator does not sign should treat restricted content, James chose that a **`recovered` continuation carries the tenant's restricted content and an `unauthorized` one only the export scope**, with suppressed and confidential parts never travelling this way and restricted content held until the recovery delay has passed.
- Asked, James chose the name **transfer** for the middle scope, to **seal the whole transfer bundle** to the receiving instance's key, and to **state the rule** that no export carries a surrogate's raw value.

## Decision

### 1. Three scopes: export, transfer and backup (amends 0006 §9)

*Changed by A1, A2, A3, A4.*

*Current text: [01](../architecture/01-log-and-records.md) §8.2.*

### 2. Export policies say which scopes carry a partition (amends 0005 §4.1; amends 0007 §8; amends 0056 §6)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.1; [07](../architecture/07-actors-and-accounts.md) §1.6; [09](../architecture/09-security-and-moderation.md) §5.4.*

### 3. A transfer is sealed whole (amends 0018 §10; amends 0027 §4)

*Current text: [07](../architecture/07-actors-and-accounts.md) §8.5; [08](../architecture/08-tenants-and-instances.md) §6.2.*

### 4. Backups and rehosting carry everything (extends 0018 §10; amends 0023 §7)

*Changed by A2, A3.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §6.7; [09](../architecture/09-security-and-moderation.md) §6.5.*

### 5. The metadata graph is exportable, in the full dump only (amends 0001 §2; amends 0013 §8; amends 0011 §2)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.5, §4.1; [16](../architecture/16-logs-feeds-and-notifications.md) §1.2.*

### 6. No export carries a surrogate's raw value (extends 0007 §5)

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.4.*

### 7. A transfer without the old operator (amends 0018 §10; amends 0027 §4; uses 0081 §3, §4)

*Current text: [07](../architecture/07-actors-and-accounts.md) §8.5; [08](../architecture/08-tenants-and-instances.md) §6.3.*

## Alternatives considered

- **Keep "export" for every copy and add exceptions** ("never exported, except by a move or a backup"). It is what the documents had drifted into, and each exception had to be restated wherever a policy was.
- **Make the full dump a backup or transfer artifact**, unpublished, so that `metadata` could stay `internal`. The full dump is the only public RDF of an instance's history, and nothing in `metadata` needed hiding once visibility is applied.
- **Seal only the private extract**, as before. The partitions carry what the instance shows no anonymous reader, so the bundle is as sensitive as `private`.
- **Call the middle scope "move"**, after "moving a tenant". "Transfer" names what kind of copy it is, beside export and backup, rather than the act.

## Consequences

- **"Never exported" means what it says.** A rehost or a backup is not an export, so no export policy limits it; its limits are the custody rules of 20 §5.3.
- **A tenant's restricted content travels in full and stays restricted.** The ACLs, groups and memberships travel with it; logins do not, so a member regains access only by reclaiming the account (08 §6.4), and until then only the new instance's operators read it by prerogative.
- **The old instance keeps the moved tenant's partitions read-only for the grace period** (08 §6.6), so the tenant can move back; until then the old operator still holds restricted content, under backup custody.
- **The full dump publishes revision metadata and log events** in their visible form: names an account later vanishes from remain in dumps already downloaded, as they do in Wikimedia's dumps; the update stream carries the erasure to consumers that follow it (02 §6.3).
- **A community can leave a vanished or hostile host with its own restricted content**, if its recovery-key holders sign; a leaked backup without them yields only what an anonymous reader could already see, and the continuation says `unauthorized` to every reader. Nothing here stops a leak itself: an instance backup holds every tenant and `private`, and only encryption at rest and the custody rules of 20 §5.3 protect it.
- **A raw upstream IP, or any surrogate's raw value, never leaves the instance in an export**; transfers and backups carry it, since they carry the `actors` partition.

## Open questions

- **Q1.** ~~**A transfer without the old operator.** A non-cooperative move (08 §6.3) takes its bundle "from backups or from the old instance's public exports". Whether a backup handed over by someone other than the old operator counts as a transfer, with the sealing of §3, or must be reduced to the export scope first, is left open.~~ *Settled by §7: what a non-cooperative move carries depends on its continuation's mode. A backup yields only the tenant's own partitions. An `unauthorized` continuation carries the export scope; a `recovered` one also the tenant's read-restricted and hidden parts, held sealed until the recovery delay passes uncancelled; suppressed and confidential parts never travel this way.*
- **Q2.** ~~**The key `private` is encrypted to.** A2 encrypts a backup's `private` schema to a key the operator names; which kind of key (an instance key, a recovery key of [0081](0081-recovery-keys-and-continuations.md), or one of its own), the scheme, and how a restore is given the private key, are left open.~~ *Settled by A3: a backup key set of its own, any `k` of whose keys restore, `k` 1 by default.*

## References

- [docs/registry/graphs.toml](../registry/graphs.toml): the export policy of every graph.
- Wikimedia's XML dumps, which publish revision metadata with user names (`stub-meta-history`).

## Amendment log

### A1. A confidential target's records are exported as leaves alone

- **Date:** 2026-11-07
- **Source:** Direct: James, design discussion of 2026-11-07
- **Change:** amends §1
- **Summary:** §1 carried every part an anonymous reader may not read as its commitment, the header kept. A record's header names its key, payload type, time and revision, so for a target a confidential restriction hides, which is answered as a target that does not exist ([0056](0056-security-model.md) §5), the header itself would show that it exists. Asked, James chose that such a record is exported as its leaf alone, header included, as a compacted record is: the tree still folds and every other proof verifies, but nothing says what the offset held. Restricted and hidden targets keep their headers with their parts withheld. Chapter text: [01](../architecture/01-log-and-records.md) §8.

Replaced text ([01](../architecture/01-log-and-records.md) §8, as it stood):

> - an **export** publishes: a public bundle, a dump, the update stream, outbound federation. It holds only what the export policies of [02](../architecture/02-graphs-rdf-and-query.md) §1.1 allow, in its visible form: every part an anonymous reader may not read, hidden, suppressed, confidential or restricted, is carried as its commitment, as an erased part is, so every proof still verifies ([09](../architecture/09-security-and-moderation.md) §5.4);

### A2. A backup is the binary's own, with `private` encrypted

- **Date:** 2026-11-12
- **Source:** Direct: James, design discussion of 2026-11-12
- **Change:** extends §1; amends §4
- **Summary:** Rehosting was "a Postgres restore plus the rejects files and the `private` schema", which needs Postgres's tools and a role set-up by hand ([0013](0013-postgres-storage.md) A97). James decided, choosing among a snapshot copy, the log bundle and a hybrid: (1) **the backup is the log bundle by default**: it carries, beside every partition, the partition registry's rows, the `private` schema, the `ops` state no rebuild recovers, the rejects files and the file blobs, and a restore loads the log and rebuilds every projection; (2) **a snapshot backup is offered as well**: every table copied from one snapshot and restored without a rebuild, faster but not verifiable; (3) **`private` is encrypted to a key**, in either form (which key is Q2); (4) **a restore migrates forward** a backup written under an older schema; (5) the rejects files and blobs travel in the backup. A Postgres restore remains a way to rehost.

Replaced text ([08](../architecture/08-tenants-and-instances.md) §6.7, as it stood):

> Rehosting an instance whole is a Postgres restore plus the rejects files and the `private` schema.

### A3. The backup key set

- **Date:** 2026-11-12
- **Source:** Direct: James, design discussion of 2026-11-12
- **Change:** extends §1, §4
- **Summary:** A2 left open which key a backup's `private` is encrypted to (Q2). James decided, among the instance key, a recovery key of [0081](0081-recovery-keys-and-continuations.md) and a key of its own: (1) **a dedicated backup key set**, which may hold several keys: each backup's data key is split into a share per key, any `k` of which rebuild it, each share sealed to its key, so that making a backup needs only the public halves and restoring one needs `k` holders, none of whom reveals a private key; (2) the set is registered or replaced by the instance's `owner`, or, where the instance has no owner account, by its operator through the command line; (3) **`k` defaults to 1**: any one key restores, however many the set holds, and the operator may raise it; (4) a new set applies to later backups only; an older backup keeps its set, and re-sealing it to a new one is a direct act of an operator with the old keys, which the instance cannot do itself.

### A4. What a backup does with each `ops` table

- **Date:** 2026-11-12
- **Source:** Direct: Claude, while building the `ops` classification (SCA-355), 2026-11-12; confirmed by James, 2026-11-12, with (5) added
- **Change:** extends §1
- **Summary:** A2 carries "the `ops` state that no rebuild recovers" without naming it. The calls made: (1) **three classes**, each `ops` table's comment, `backup: carried | sealed | left`, checked against `triplespace_db::ops::OPS_TABLES` as `private` tables' portability is, so that no `ops` table exists unclassified; (2) **`delivery` is sealed**, carried but encrypted with `private` under A3's key set: its rows are outgoing messages complete with their addresses and bodies, verification codes among them; (3) **carried**: `job_queue`, uploads included, a job running at the backup's instant running again after a restore as after a runner's crash; `forwarder_key`; `repo_cursor`, a hint the version cursor could rebuild, carried as small and saving polling from the start; and `visibility_epoch`, part of every cache key; (4) **left**: `migration`, which the restore's migrations write; `projection_state` and `projection_work`, which the rebuild fills; and `purge`, purges for caches a restored instance does not have; (5) **a restore advances every tenant's `visibility_epoch`** to the restore's time in microseconds (James, in confirming): the old instance may have raised the epoch after the backup and cached entries under the very values the backup holds, as under any value from before, so neither carrying the value nor starting at 0 keeps a restored instance from meeting them; no epoch the old instance counted to can reach the restore's time, and later bumps count on from it.
