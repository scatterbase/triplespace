# 0018. Tenants

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A22)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0003](0003-statement-ui.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0016](0016-permissions-and-access-control.md), [0017](0017-entity-id-grammar.md), [0022](0022-federation.md), [0023](0023-moderation.md), [0028](0028-tenancy-policy.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [04](../architecture/04-entities-and-identifiers.md), [08](../architecture/08-tenants-and-instances.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

Every ADR so far describes one wiki: one local graph, one set of mirrors, one base URI, one user base. Librarybase running Triplespace at `https://librarybase.org` is that picture: `Q6` is its item, `WDQ42` is Wikidata's.

An instance should also be able to host several wikis, in the manner of a wiki farm. A second wiki at `https://example.wiki` on the same instance has its own `Q6`, which is a different thing from Librarybase's. It shares the instance's mirrors, because a second copy of Wikidata would be absurd, and it may want to refer to Librarybase's entities, which are as foreign to it as Wikidata's are.

Three open questions from earlier ADRs turn out to be tenant questions in disguise:

- **Statement IDs after conversion** ([0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md)): what a statement is called once its entity has been rewritten to another ID. On a farm, one tenant's converted entity is another tenant's foreign entity, so the rule has to hold across tenants.
- **Contributions of foreign actors** ([0010](0010-site-ui.md), [0012](0012-api-requirements.md)): what `Special:Contributions` shows for an account the wiki does not own. On a farm, most accounts in a tenant's history that are not its own belong to other tenants.
- **Who owns an account.** [0007](0007-actor-identity.md) §1 says accounts belong to the instance. MediaWiki's experience says they belong to the wiki: when a wiki imports pages from Wikipedia, MediaWiki creates local user rows for the imported usernames, and Wikipedia editors have found themselves with contribution histories on wikis they never visited. The source of an edit is part of its provenance and must travel with it.

This ADR adds tenants, makes each tenant its own issuer of identity, lets a tenant be a provider to others, and specifies how a tenant moves between instances.

## Decision

### 1. Instances and tenants

*Changed by A5, A8, A16, A20.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §1.1, §1.2, §1.3, §7.1.*

### 2. Partitions (amends 0005 §4.1–4.2, 0006 §3, 0013 §2 and 0015 §3, §5)

*Changed by A6, A8, A14, A15, A20, A22.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §2.1, §2.2, §2.3, §2.4.*

### 3. Configuration (amends 0015 §3)

*Changed by A2, A5, A8, A13.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §2.4, §2.5.*

### 4. Identity is per tenant (amends 0007 §1 and §8)

*Changed by A4, A7, A20.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §3.1, §3.2, §3.3, §3.4, §8.1.*

### 5. A tenant can be a provider (amends 0002 §4)

*Changed by A4, A5, A11, A12, A21.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §4.1, §4.2, §4.3, §6.6.*

### 6. Shared views and tenant overlays (amends 0013 §5, 0014 §7)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.1, §7, §9.4, §11.4, §12.1, §12.5.*

### 7. Statement IDs follow the canonical ID (settles 0002 and 0004)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.9.*

### 8. Contributions (settles 0010 and 0012; amends 0010 §8)

*Current text: [19](../architecture/19-site-ui.md) §2.5.*

### 9. Aliases

*Changed by A20.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §6.1.*

### 10. Moving a tenant (extends 0006 §6 and §9)

*Changed by A3, A7, A8, A12, A17, A20, A21, A22.*

*Current text: [08](../architecture/08-tenants-and-instances.md) §6.2, §6.3, §6.4, §6.7, §7.6; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 11. API and UI (amends 0012 §4–5)

*Changed by A9, A18, A20.*

*Current text: [18](../architecture/18-api.md) §1.2, §2.2; [19](../architecture/19-site-ui.md) §1.3, §3.10, §6.11.*

### 12. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **A single wiki is a one-tenant instance,** and nothing in the earlier ADRs changes for it except two header bytes and a key record in its own `config`.
- **Provenance travels.** Every edit names the wiki and account it was made under, for as long as it exists anywhere, and no import or migration can re-attribute it.
- **A wiki can leave.** Its partitions, IDs, actors and history move as a unit; its references from elsewhere keep working; its users reclaim their accounts rather than inherit them.
- **Tenants as providers cost nothing to host.** No sync, no mirror, no copy; the rewrite happens on read.
- **Two open questions from 0002/0004 and one from 0010/0012 close**, each with one rule.
- **Projections are more complex.** Every entity view is shared-or-overlay, and every cache key has to know which. That is the price of not copying Wikidata per tenant.
- ~~**Partition counts grow with tenants.** Six per tenant; the farm profile must measure Postgres at the intended scale.~~ *Six fixed partitions plus one `source/{name}` per entity source (A14); the measurement is still owed.*
- ~~**A farm's operators are a tenant's users.** There is no farm-level account; the primary tenant plays that role.~~ *A farm may have farm accounts, which edit nothing (A4); the primary tenant's accounts still operate the instance.*

## Open questions

- **Q1. Partition count at farm scale,** and whether small tenants should share physical tables with the tenant in the key while keeping logical partitions. [0028](0028-tenancy-policy.md) §2 adds three instance partitions per farm.
- **Q2.** ~~**Cross-tenant ACLs.** Whether a provider tenant may restrict which tenants read it, beyond public-or-private.~~ *Settled by [0028](0028-tenancy-policy.md) §5: reader lists, a `provider-readers` config record in the provider tenant's own `config`.*
- **Q3.** ~~**Global blocks.** Whether the primary tenant may block an identity-provider subject across every tenant, as farms do.~~ *Settled by [0028](0028-tenancy-policy.md) §4: a block on a farm account, by policy; IP blocks are farm-wide under every preset.*
- **Q4.** ~~**Deleting a tenant**, and what its former referrers see.~~ *Settled by [0028](0028-tenancy-policy.md) §5: a move with no destination; partitions kept read-only for a grace period, referrers see the last mirrored state with the provider marked gone, then erasure. The grace period's length is open there.*
- **Q5. Overlay index cost** for a tenant that annotates a large share of a provider's entities.
- **Q6.** ~~**Reclaim log event** name and parameters, and whether reclaiming should be reversible.~~ *Settled by A17 in part: `reclaim/reclaim`, projected from the reclaiming binding record in the tenant `log` and visible to everyone.*
- **Q7. The retention extract's integrity.** Records lifted from `hashed` partitions carry headers but not whole segments; the receiving instance re-appends them under its own key, which is the guarantee mirrors already have.
- **Q8.** (Rest of Q6.) The `reclaim/reclaim` event's parameters, and whether reclaiming should be reversible.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §4 | §5 | extends | 0002 A8 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q7 | §7 | settles | 0002 Q7 |
| [0004](0004-identity-clusters-and-equivalence.md) §3 | §5–6 | extends | 0004 A3 |
| [0004](0004-identity-clusters-and-equivalence.md) Q8 | §7 | settles | 0004 Q8 |
| [0005](0005-crate-organization.md) §2, §4.1, §4.2 | §12 | extends | 0005 A14 |
| [0006](0006-log-integrity-and-erasure.md) §3, §6 | §2, §10 | amends | 0006 A6 |
| [0006](0006-log-integrity-and-erasure.md) §9 | §2, §10 | extends | 0006 A6 |
| [0007](0007-actor-identity.md) §1 | §4 | amends | 0007 A2 |
| [0007](0007-actor-identity.md) §3, §8 | §4 | extends | 0007 A2 |
| [0010](0010-site-ui.md) §8 | §8 | amends | 0010 A8 |
| [0010](0010-site-ui.md) Q4 | §8 | settles | 0010 Q4 |
| [0012](0012-api-requirements.md) §2.1 | §2, §8, §11 | amends | 0012 A7 |
| [0012](0012-api-requirements.md) §4, §5 | §2, §8, §11 | extends | 0012 A7 |
| [0012](0012-api-requirements.md) Q3 | §8 | settles | 0012 Q3 |
| [0013](0013-postgres-storage.md) §2, §5, §6, §7 | §2–3, §6 | amends | 0013 A5 |
| [0014](0014-caches-and-search.md) §1, §5 | §6 | amends | 0014 A3 |
| [0014](0014-caches-and-search.md) §10 | §6 | extends | 0014 A3 |
| [0015](0015-record-format-and-partition-registry.md) §2, §3, §5 | §2–3, §5 | amends | 0015 A3 |

## References

- [0007 — Actor identity](0007-actor-identity.md), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md)
- [MediaWiki:Manual:Importing XML dumps](https://www.mediawiki.org/wiki/Manual:Importing_XML_dumps) (imported edits and local user rows)
- [Extension:CentralAuth](https://www.mediawiki.org/wiki/Extension:CentralAuth) and [Special:GlobalContributions](https://www.mediawiki.org/wiki/Extension:GlobalContributions)
- [Dublin Core Terms](https://www.dublincore.org/specifications/dublin-core/dcmi-terms/) (`dcterms:isReplacedBy`)
- [C2SP tlog-checkpoint](https://c2sp.org/tlog-checkpoint) (origin lines)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §12
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A14).

Replaced text (§12):

> | Crate | Change |
> |---|---|
> | `scatter-log` | 64-bit partition IDs (§2); the `tenant` and `alias` config kinds and the key chain (§2–3) |
> | `scatter-log-postgres`, `triplespace-db` | `partition` as `bigint`; the `tenant` column on `view` tables (§6); one child table per tenant partition |
> | `scatter-providers` | Rewriting a tenant's local IDs to its provider form when read (§5) |
> | `scatter-actors` | The tenant as issuer; `local` as the current tenant (§4) |
> | `scatter-wikibase-resolve` | The GUID rewrite (§7) |
> | `triplespace-projections` | Shared rows and tenant overlays; direct reads of provider tenants' partitions (§5–6) |
> | `triplespace-search` | Per-provider and per-tenant indexes and the fan-out (§6) |
> | `triplespace-titles`, `triplespace-api-*` | Host-to-tenant resolution (§11) |
> | `triplespace-accounts` | Bindings per tenant; reclaiming (§10) |
> | `triplespace-cli` | `tenant create`, `tenant export`, `tenant import`, the binding digest (§10) |
>
> No crate is added.

### A2. Sitelink policy lists

- **Date:** 2026-09-27
- **Source:** [0026](0026-sitelinks.md) §3
- **Change:** extends §3
- **Summary:** An instance deny list, `sitelink-policy:deny` in the instance `config`, and tenant allow or deny lists in each tenant's.

### A3. The private extract

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §4
- **Change:** extends §10
- **Summary:** A cooperative move carries a per-account private extract, sealed to the receiving instance and applied on reclaim.

### A4. Tenancy policy

- **Date:** 2026-09-27
- **Source:** [0028](0028-tenancy-policy.md) §2–5
- **Change:** amends §4; extends §5
- **Summary:** Under a tenancy policy with farm identity, the farm *is* an issuer with accounts, but those accounts edit nothing: they are the identity a person carries across the farm, publicly linked to the tenant accounts created from them, and what global groups and global blocks key on. Every record is still attested by a tenant actor, and instance-level records by the primary tenant's, so nothing in this section's attribution rule changes. §5 gains `providers.between_tenants`, reader lists, cross-instance reading by the Triplespace adapter, and deletion as a move with no destination. This settled Q2, Q3 and Q4, and falsified the consequence that there is no farm-level account.

Replaced text (§4):

> **The farm has no user base.** Instance-level records are attested by actors of the primary tenant. Memberships and blocks stay in each tenant's `actors` partition as [0016](0016-permissions-and-access-control.md) §3 has them, which is where a farm wants them: groups are per wiki.

### A5. Adoption

- **Date:** 2026-09-28
- **Source:** [0035](0035-adopting-a-wikibase.md) §1
- **Change:** extends §1, §3, §5
- **Summary:** A tenant may be the continuation of a MediaWiki Wikibase that ran before Triplespace. *Adoption* writes that wiki's entities into the tenant's `local` partition under the IDs it minted, its accounts under their own numbers, and sets every sequence past the source's; the `tenant` record gains `adopted_from`. It is neither mirroring (nothing is prefixed, no mirror partition exists) nor a move under §10 (there is no bundle to verify). If the tenant is a provider, §5 applies to adopted entities unchanged. The `tenant` record gains `adopted_from`.

### A6. Files and the instance log

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §4, §10
- **Change:** extends §2
- **Summary:** The instance gains its own `log` partition (internal) for operator records such as file takedowns, and `files/{repo}` mirror partitions for file repositories. File bytes live outside the log in a blob store whose **scope** is one per tenant by default (`files.separation = tenant`) or one for the instance.

### A7. Instance acts

- **Date:** 2026-09-30
- **Source:** [0040](0040-instance-prerogatives.md) §1–3, §8
- **Change:** amends §4; extends §10
- **Summary:** By section:
  - §4: The rule above assumed the instance never writes into a tenant's partitions. Every record in a tenant's partitions is now attested **either by one of the tenant's own actors, or by the instance**: an *instance act* written into the tenant carries an instance attestation, attributed to the operator actor `instance:{farm slug}`, signed by the instance key from the tenant's own key chain, and citing an authority record in an instance partition. Binding instance acts are *prerogatives*; the tenant cannot undo them.
  - §10: The bundle also carries an **authority extract**: every authority record its instance attestations cite, with an inclusion proof and with the operator's attestation withheld. Prerogatives of the old instance stop binding once the tenant is registered elsewhere; they stay in history, verifiable through the old key.

Replaced text: none; §4 assumed, without saying so, that the instance never writes into a tenant's partitions.

### A8. The primary tenant

- **Date:** 2026-09-30
- **Source:** [0046](0046-primary-tenant.md) §1–3, §5, §7
- **Change:** amends §1, §2, §3, §10
- **Summary:** By section:
  - §1: The primary tenant is **the tenant whose accounts operate the instance**: instance rights are evaluated there, and records the instance holds on its own behalf are attested by its actors as of each record's offset, but none of those records is stored in its partitions. The role is held by one `primary` record in the instance `config`, may be given to any tenant, including an operations tenant with no content, and is transferred by offer and acceptance.
  - §2: Instance graph IRIs are `{farm base}/instance/graph/{name}` and instance origin lines `{farm host}/instance/log/{name}`, so that the instance `config` and `log` cannot be confused with a tenant's when the farm base is a tenant's base.
  - §3: The `tenant` record has no primary flag; which tenant is primary is the single `primary` record of the instance `config`.
  - §10: The primary tenant cannot be frozen for a move, moved or deleted until the primary role has been transferred to another tenant (`ts-primary-tenant`); on a single-tenant instance, moving the only tenant is rehosting the instance.

Replaced text (§1):

> One tenant is the instance's **primary tenant**. Its actors attest instance-level records (§3, §4). On a single-tenant instance that is the only tenant; on a farm it is whichever tenant the operators run the farm from.

Replaced text (§2):

> | Instance | `config`, `mirror/{provider}`, `actors/{provider}`, `log/{provider}` | `{farm base}/graph/{name}` |
>
> Checkpoint origin lines ([0006](0006-log-integrity-and-erasure.md) §6) use the tenant's host, so Librarybase's local log signs as `librarybase.org/log/local`.

Replaced text (§3):

> | Instance `config` | `key`, `graph`, `provider`, `issuer`, `keyed-type`, and two new kinds: `tenant` (slug, base URI, primary flag, and, if the tenant is a provider, its code and number) and `alias` (§9) |

### A9. Special page scopes

- **Date:** 2026-10-01
- **Source:** [0047](0047-special-pages.md) §3
- **Change:** extends §11
- **Summary:** Every special page has a scope in `docs/registry/special-pages.toml`: `farm` pages are served at the farm base, `tenant` pages at each tenant's base, and `both` pages at either.

### A10. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–12
- **Summary:** A1–A9 were folded into the Decision. The open questions were numbered. No decision changed. §3's tenant row named `acl` as a config kind, which [0023](0023-moderation.md) §3 had made a payload type without amending this ADR; the row now says graph ACLs. Before this, A4–A9 were blockquotes, and A1–A3 were recorded only in other ADRs. The file before conversion is commit `0b26a3a`.

### A11. A private tenant is defined

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §3
- **Change:** amends §5
- **Summary:** A private tenant is one whose `tenant` ACL restricts `read`. §5 had named one by its export policy without saying how a tenant becomes one; the export policy is now a consequence of the ACL.

Replaced text (§5):

> - **A tenant without a code cannot be referenced.** Its IDs have no absolute form. A private tenant, whose partitions carry the `private` export policy, cannot take a code.

### A12. Entity sources

- **Date:** 2026-10-08
- **Source:** [0078](0078-entity-sources.md) §4, §8
- **Change:** extends §5, §10
- **Summary:** A provider tenant's references to its entity sources cross to a reader by IRI: to a registry provider's ID, to the reader's own source for the same IRIs, or else the statements using them are withheld and counted. Unpublished sources never cross. A move carries each `source/{name}` partition with the six.

Replaced text (§10):

> **What moves.** The tenant's six partitions (§2) as an export bundle

### A13. Inference properties are instance configuration

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §3
- **Summary:** The identifier properties that drive tier-3 inference are **instance** configuration, held in an instance `reconcile` record, so tier-3 links stay instance-wide (§6). A tenant's `reconcile` record may switch inference off for the tenant and set its provider order and link properties, but may not add inference properties. §3's instance row therefore gains `reconcile`, and the tenant row's `reconcile` is read with that limit ([0004](0004-identity-clusters-and-equivalence.md) §3, [0015](0015-record-format-and-partition-registry.md) §3). (PENDING B6)

Replaced text (§3):

> | Instance `config` | `key`, `graph`, `provider`, `issuer`, `keyed-type`, and three kinds of this ADR and [0046](0046-primary-tenant.md) §2: `tenant` (slug, base URI, `adopted_from` and, if the tenant is a provider, its code and number), `alias` (§9) and `primary` (§1). Instance-wide policy lists such as `sitelink-policy:deny` ([0026](0026-sitelinks.md) §3) and the other instance-scope kinds later ADRs add are listed in [0015](0015-record-format-and-partition-registry.md) §3 |

### A14. Source partitions and farm partitions in the partition table

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §2
- **Summary:** A tenant also has one `source/{name}` partition per entity source ([0078](0078-entity-sources.md) §4), and the instance also has the farm partitions of [0028](0028-tenancy-policy.md) §2. The fixed count of six goes: §2's table and its Scale paragraph read as six fixed partitions per tenant plus one per entity source, and the Consequences bullet that counted six is struck. (PENDING C6)

### A15. "Partition 0" is a designation

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §2
- **Summary:** "Partition 0" is a designation, not an ID. The ADRs say "the instance `config` partition": it is found by its registry name and has a random 64-bit ID like every other partition (§2); offset 0 of it holds the first `key:` record ([0015](0015-record-format-and-partition-registry.md) §3). (PENDING C7)

Replaced text (§2):

> The instance key is registered in the instance's `config` (partition 0), which does not travel with a tenant.

### A16. `instance` is a reserved slug

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §1
- **Summary:** `instance` is reserved in the shared namespace of slugs, provider slugs and issuer codes: no tenant, provider or issuer may take it, since it is the operator actor's issuer code ([0040](0040-instance-prerogatives.md) §2) and the farm slug is registered beside it ([0046](0046-primary-tenant.md) §6). Both ADRs cite §1 for the reservation; §1 now says so. (PENDING D1)

### A17. The reclaim log event

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §10
- **Summary:** The manual reclaim of §10 is the `reclaim/reclaim` log event, added to the event table of [0011](0011-logs.md) §6.1 with `takedown/expunge` and the no-redirect move. `reclaim/reclaim` projects from the reclaiming binding record in the tenant `log` and is visible to everyone. This settles the name and source half of Q6; its parameters and reversibility remain as Q8. (PENDING E27)

### A18. The tenant list

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §11
- **Summary:** The tenant list of §11 is the `tenants` array of `GET /tenancy` at the farm base, one entry per tenant with its slug, host, name and status ([0046](0046-primary-tenant.md) §9), and `Special:Tenants` is its page. (PENDING E33)

### A19. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§12
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [04](../architecture/04-entities-and-identifiers.md), [08](../architecture/08-tenants-and-instances.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A20. Issuer codes derived from the founding record

- **Date:** 2026-10-09
- **Source:** [0079](0079-derived-issuer-codes.md) §1, §3, §5, §7
- **Change:** amends §1, §4, §9, §10; extends §2, §11
- **Summary:** A tenant's issuer code, the code its actor keys carry, is no longer its slug: it is the first 26 characters of the lower-case base32 encoding of the Merkle leaf hash of the tenant's **founding record**, the `key:` record at offset 0 of its `config` partition (§2). It is fixed at creation, survives moves and erasure, and is allocated by no one. The slug becomes a name on the instance: unique there, held only in instance-scope records, in no actor key, and chosen afresh on arrival if taken (§1, §10). A change of base changes no actor key (§9). `verify` recomputes the code from the bundle (§10). The tenant list in `GET /tenancy` gives each tenant's code as `issuer` (§11). Examples keep writing `librarybase:42` for the derived key.

Replaced text (§1, in [08](../architecture/08-tenants-and-instances.md) §1.2):

> The slug names the tenant everywhere: in partition names, in actor keys (§3.1), and, if the tenant becomes a provider, in `providers.toml` (§4). Slugs, provider slugs and issuer codes are one namespace, allocated in the registry ([0015](0015-record-format-and-partition-registry.md) §5). No slug can be empty ([03](../architecture/03-storage-caches-and-search.md) §4.1).

Replaced text (§4, in [08](../architecture/08-tenants-and-instances.md) §3.1):

> Its issuer code is its slug, its actor model is numeric, and its actor IRI template is `{tenant base}/user/{id}`.

Replaced text (§9, in [08](../architecture/08-tenants-and-instances.md) §6.1):

> So `domains.wikibase.cloud` becoming `internetdomains.wiki` is one record and one triple, and every `internetdomains:…` actor key and every entity IRI ever emitted stays resolvable.

Replaced text (§10, in [08](../architecture/08-tenants-and-instances.md) §6.3):

> …and maps the tenant's `providers` list onto its own mirrors, which is trivial because slugs come from the same registry.

### A21. Provider tenants are entity sources

- **Date:** 2026-10-09
- **Source:** [0080](0080-tenants-as-entity-sources.md) §2, §3
- **Change:** amends §5, §10
- **Summary:** A tenant is read by another tenant as an entity source that names it by issuer code, not by taking a registry code: any public tenant can be read, on the same instance directly with its IDs rewritten to the reader's source name (`lb:Q6`) and ranged by the reader's number, elsewhere by `scatter-adapter-triplespace` into the reader's `source/{name}` (§5). A registry code is promotion for a widely read tenant, and its entry's `issuer` is the tenant's issuer code (§5). A move switches readers between direct reading and the adapter (§10).

Replaced text (§5, in [08](../architecture/08-tenants-and-instances.md) §4.1):

> A tenant becomes a provider by taking a code, a slug and a provider number in `providers.toml` ([0015](0015-record-format-and-partition-registry.md) §5), as Librarybase has `LB`, `librarybase` and 2. Nothing else is needed on the instance that hosts it: there is no sync job and no mirror partition, because the tenant's `local` partition is the source.
>
> …
>
> - **Opt-in.** A tenant reads a provider's graph only if the provider is in its `providers` list (§2.5). A tenant that has not opted into Librarybase never sees `LBQ6`. …
> - **A tenant without a code cannot be referenced.** Its IDs have no absolute form.
> - **Leaving the instance changes nothing for referrers.** When a provider tenant moves away (§6), the instance registers a real sync job against its new home (§4.4), and `LBQ6` means what it always meant; … The same code serves off-instance readers on other instances from the start, since the registry is global.

Replaced text (§5, in [08](../architecture/08-tenants-and-instances.md) §4.3):

> …cannot take a code:

Replaced text (§10, in [08](../architecture/08-tenants-and-instances.md) §6.3):

> If the tenant is a provider, the old instance replaces its direct read with a sync job against the new home (§4.4).

### A22. Continuations and fixed origin lines

- **Date:** 2026-10-09
- **Source:** [0081](0081-recovery-keys-and-continuations.md) §3, §5
- **Change:** amends §2, §10
- **Summary:** A tenant partition's checkpoint origin line uses the tenant's host as it was when the partition was created or the tenant arrived on the instance, and an alias does not change it (§2). A non-cooperative move appends a `continuation`, `recovered` when the tenant's recovery keys sign it and `unauthorized` otherwise, instead of a rotation with no signature (§10).

Replaced text (§2, in [08](../architecture/08-tenants-and-instances.md) §2.1):

> Checkpoint origin lines ([0006](0006-log-integrity-and-erasure.md) §6) use the tenant's host, so Librarybase's local log signs as `librarybase.org/log/local`;

Replaced text (§10, in [08](../architecture/08-tenants-and-instances.md) §6.3):

> **Non-cooperatively**, the bundle comes from backups, the rotation record carries no signature from the old key, verifiers are told the chain has an unsigned link, and reclaiming is manual.
