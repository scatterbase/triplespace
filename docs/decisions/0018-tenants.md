# 0018. Tenants

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-11-16 (A37)
- **Author:** James Hare / Claude Fable
- **Uses:** [0003](0003-statement-ui.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0016](0016-permissions-and-access-control.md), [0017](0017-entity-id-grammar.md), [0022](0022-federation.md), [0023](0023-moderation.md), [0028](0028-tenancy-policy.md)

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

*Current text: [08](../architecture/08-tenants-and-instances.md) §1.1, §1.2, §1.3, §7.1.*

### 2. Partitions (amends 0005 §4.1–4.2, 0006 §3, 0013 §2 and 0015 §3, §5)

*Current text: [08](../architecture/08-tenants-and-instances.md) §2.1, §2.2, §2.3, §2.4.*

### 3. Configuration (amends 0015 §3)

*Current text: [08](../architecture/08-tenants-and-instances.md) §2.4, §2.5.*

### 4. Identity is per tenant (amends 0007 §1 and §8)

*Current text: [08](../architecture/08-tenants-and-instances.md) §3.1, §3.2, §3.3, §3.4, §8.1.*

### 5. A tenant can be a provider (amends 0002 §4)

*Current text: [08](../architecture/08-tenants-and-instances.md) §4.1, §4.2, §4.3, §6.6.*

### 6. Shared views and tenant overlays (amends 0013 §5, 0014 §7)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.1, §7, §9.4, §11.4, §12.1, §12.5.*

### 7. Statement IDs follow the canonical ID (settles 0002 and 0004)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §4.9, §4.12.*

### 8. Contributions (settles 0010 and 0012; amends 0010 §8)

*Current text: [19](../architecture/19-site-ui.md) §2.5.*

### 9. Aliases

*Current text: [08](../architecture/08-tenants-and-instances.md) §6.1.*

### 10. Moving a tenant (extends 0006 §6 and §9)

*Current text: [08](../architecture/08-tenants-and-instances.md) §6.2, §6.3, §6.4, §6.7, §7.6; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 11. API and UI (amends 0012 §4–5)

*Current text: [18](../architecture/18-api.md) §1.2, §2.2; [19](../architecture/19-site-ui.md) §1.3, §3.10, §6.11.*

### 12. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **Tenants as paths under one host** (`example.com/tenant/wiki/Project:Home` instead of `tenant.example.com/wiki/Project:Home`). Rejected in the design discussion of 2026-11-16 (A37). The routing part is bounded work: the tenant would be chosen by host and first path segment instead of by host alone, and generated URLs, the web tier's routing table ([0057](0057-web-tier.md) §1), checkpoint origin lines (A22) and the auth host's handoff ([0089](0089-the-auth-host-and-instance-logins.md) §1) would each learn the prefix. The cost is that every tenant would share one browser origin. Browsers isolate by origin, not by path, and a cookie's `Path` is no boundary between pages of one origin, so a script running on one tenant's pages could act with a user's session on another's. Tenants may run their own interface content and styling ([0095](0095-the-interface-namespace.md), [0086](0086-theming.md)), so tenant isolation would rest on a second security model beside [0056](0056-security-model.md): refusing tenant-controlled active content in that mode, serving all user content from a sandboxed origin, or confining the mode to instances whose tenants trust one another. A wildcard DNS record and a wildcard certificate give every tenant a subdomain with one-time setup, which covers the usual reason to want paths.

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
- **Q2. Cross-tenant ACLs.** *Settled by [0028](0028-tenancy-policy.md) §5.*
- **Q3. Global blocks.** *Settled by [0028](0028-tenancy-policy.md) §4.*
- **Q4. Deleting a tenant.** *Settled by [0028](0028-tenancy-policy.md) §5.*
- **Q5. Overlay index cost** for a tenant that annotates a large share of a provider's entities.
- **Q6. Reclaim log event.** *Settled by A17 in part.*
- **Q7. The retention extract's integrity.** Records lifted from `hashed` partitions carry headers but not whole segments; the receiving instance re-appends them under its own key, which is the guarantee mirrors already have.
- **Q8. (Rest of Q6.) The `reclaim/reclaim` event's parameters.** *Settled by [0088](0088-external-identity-providers.md) §8.*
- **Q9.** (Rest of Q8.) Whether reclaiming should be reversible.

## References

- [0007 — Actor identity](0007-actor-identity.md), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md)
- [MediaWiki:Manual:Importing XML dumps](https://www.mediawiki.org/wiki/Manual:Importing_XML_dumps) (imported edits and local user rows)
- [Extension:CentralAuth](https://www.mediawiki.org/wiki/Extension:CentralAuth) and [Special:GlobalContributions](https://www.mediawiki.org/wiki/Extension:GlobalContributions)
- [Dublin Core Terms](https://www.dublincore.org/specifications/dublin-core/dcmi-terms/) (`dcterms:isReplacedBy`)
- [C2SP tlog-checkpoint](https://c2sp.org/tlog-checkpoint) (origin lines)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** 0005 §2
- **Change:** supersedes §12
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A14).

Replaced text: §12 of this ADR.

### A2. Sitelink policy lists

- **Date:** 2026-09-27
- **Source:** 0026 §3
- **Change:** extends §3
- **Summary:** An instance deny list, `sitelink-policy:deny` in the instance `config`, and tenant allow or deny lists in each tenant's.

### A3. The private extract

- **Date:** 2026-09-27
- **Source:** 0027 §4
- **Change:** extends §10
- **Summary:** A cooperative move carries a per-account private extract, sealed to the receiving instance and applied on reclaim.

### A4. Tenancy policy

- **Date:** 2026-09-27
- **Source:** 0028 §2–5
- **Change:** amends §4; extends §5
- **Summary:** Under a tenancy policy with farm identity, the farm *is* an issuer with accounts, but those accounts edit nothing: they are the identity a person carries across the farm, publicly linked to the tenant accounts created from them, and what global groups and global blocks key on. Every record is still attested by a tenant actor, and instance-level records by the primary tenant's, so nothing in this section's attribution rule changes. §5 gains `providers.between_tenants`, reader lists, cross-instance reading by the Triplespace adapter, and deletion as a move with no destination. This settled Q2, Q3 and Q4, and falsified the consequence that there is no farm-level account.

Replaced text: §4 of this ADR.

### A5. Adoption

- **Date:** 2026-09-28
- **Source:** 0035 §1
- **Change:** extends §1, §3, §5
- **Summary:** A tenant may be the continuation of a MediaWiki Wikibase that ran before Triplespace. *Adoption* writes that wiki's entities into the tenant's `local` partition under the IDs it minted, its accounts under their own numbers, and sets every sequence past the source's; the `tenant` record gains `adopted_from`. It is neither mirroring (nothing is prefixed, no mirror partition exists) nor a move under §10 (there is no bundle to verify). If the tenant is a provider, §5 applies to adopted entities unchanged. The `tenant` record gains `adopted_from`.

### A6. Files and the instance log

- **Date:** 2026-09-30
- **Source:** 0039 §4, §10
- **Change:** extends §2
- **Summary:** The instance gains its own `log` partition (internal) for operator records such as file takedowns, and `files/{repo}` mirror partitions for file repositories. File bytes live outside the log in a blob store whose **scope** is one per tenant by default (`files.separation = tenant`) or one for the instance.

### A7. Instance acts

- **Date:** 2026-09-30
- **Source:** 0040 §1–3, §8
- **Change:** amends §4; extends §10
- **Summary:** By section:
  - §4: The rule above assumed the instance never writes into a tenant's partitions. Every record in a tenant's partitions is now attested **either by one of the tenant's own actors, or by the instance**: an *instance act* written into the tenant carries an instance attestation, attributed to the operator actor `instance:{farm slug}`, signed by the instance key from the tenant's own key chain, and citing an authority record in an instance partition. Binding instance acts are *prerogatives*; the tenant cannot undo them.
  - §10: The bundle also carries an **authority extract**: every authority record its instance attestations cite, with an inclusion proof and with the operator's attestation withheld. Prerogatives of the old instance stop binding once the tenant is registered elsewhere; they stay in history, verifiable through the old key.

Replaced text: none; §4 assumed, without saying so, that the instance never writes into a tenant's partitions.

### A8. The primary tenant

- **Date:** 2026-09-30
- **Source:** 0046 §1–3, §5, §7
- **Change:** amends §1, §2, §3, §10
- **Summary:** By section:
  - §1: The primary tenant is **the tenant whose accounts operate the instance**: instance rights are evaluated there, and records the instance holds on its own behalf are attested by its actors as of each record's offset, but none of those records is stored in its partitions. The role is held by one `primary` record in the instance `config`, may be given to any tenant, including an operations tenant with no content, and is transferred by offer and acceptance.
  - §2: Instance graph IRIs are `{farm base}/instance/graph/{name}` and instance origin lines `{farm host}/instance/log/{name}`, so that the instance `config` and `log` cannot be confused with a tenant's when the farm base is a tenant's base.
  - §3: The `tenant` record has no primary flag; which tenant is primary is the single `primary` record of the instance `config`.
  - §10: The primary tenant cannot be frozen for a move, moved or deleted until the primary role has been transferred to another tenant (`ts-primary-tenant`); on a single-tenant instance, moving the only tenant is rehosting the instance.

Replaced text: §1 of this ADR; §2 of this ADR; §3 of this ADR.

### A9. Special page scopes

- **Date:** 2026-10-01
- **Source:** 0047 §3
- **Change:** extends §11
- **Summary:** Every special page has a scope in `docs/registry/special-pages.toml`: `farm` pages are served at the farm base, `tenant` pages at each tenant's base, and `both` pages at either.

### A10. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** 0050 §13
- **Change:** consolidates §1–12
- **Summary:** A1–A9 were folded into the Decision. The open questions were numbered. No decision changed. §3's tenant row named `acl` as a config kind, which 0023 §3 had made a payload type without amending this ADR; the row now says graph ACLs. Before this, A4–A9 were blockquotes, and A1–A3 were recorded only in other ADRs. The file before conversion is commit `0b26a3a`.

### A11. A private tenant is defined

- **Date:** 2026-10-01
- **Source:** 0056 §3
- **Change:** amends §5
- **Summary:** A private tenant is one whose `tenant` ACL restricts `read`. §5 had named one by its export policy without saying how a tenant becomes one; the export policy is now a consequence of the ACL.

Replaced text: §5 of this ADR.

### A12. Entity sources

- **Date:** 2026-10-08
- **Source:** 0078 §4, §8
- **Change:** extends §5, §10
- **Summary:** A provider tenant's references to its entity sources cross to a reader by IRI: to a registry provider's ID, to the reader's own source for the same IRIs, or else the statements using them are withheld and counted. Unpublished sources never cross. A move carries each `source/{name}` partition with the six.

Replaced text: §10 of this ADR.

### A13. Inference properties are instance configuration

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §3
- **Summary:** The identifier properties that drive tier-3 inference are **instance** configuration, held in an instance `reconcile` record, so tier-3 links stay instance-wide (§6). A tenant's `reconcile` record may switch inference off for the tenant and set its provider order and link properties, but may not add inference properties. §3's instance row therefore gains `reconcile`, and the tenant row's `reconcile` is read with that limit (0004 §3, 0015 §3). (PENDING B6)

Replaced text: §3 of this ADR.

### A14. Source partitions and farm partitions in the partition table

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §2
- **Summary:** A tenant also has one `source/{name}` partition per entity source (0078 §4), and the instance also has the farm partitions of 0028 §2. The fixed count of six goes: §2's table and its Scale paragraph read as six fixed partitions per tenant plus one per entity source, and the Consequences bullet that counted six is struck. (PENDING C6)

### A15. "Partition 0" is a designation

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §2
- **Summary:** "Partition 0" is a designation, not an ID. The ADRs say "the instance `config` partition": it is found by its registry name and has a random 64-bit ID like every other partition (§2); offset 0 of it holds the first `key:` record (0015 §3). (PENDING C7)

Replaced text: §2 of this ADR.

### A16. `instance` is a reserved slug

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §1
- **Summary:** `instance` is reserved in the shared namespace of slugs, provider slugs and issuer codes: no tenant, provider or issuer may take it, since it is the operator actor's issuer code (0040 §2) and the farm slug is registered beside it (0046 §6). Both ADRs cite §1 for the reservation; §1 now says so. (PENDING D1)

### A17. The reclaim log event

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §10
- **Summary:** The manual reclaim of §10 is the `reclaim/reclaim` log event, added to the event table of 0011 §6.1 with `takedown/expunge` and the no-redirect move. `reclaim/reclaim` projects from the reclaiming binding record in the tenant `log` and is visible to everyone. This settles the name and source half of Q6; its parameters and reversibility remain as Q8. (PENDING E27)

### A18. The tenant list

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §11
- **Summary:** The tenant list of §11 is the `tenants` array of `GET /tenancy` at the farm base, one entry per tenant with its slug, host, name and status (0046 §9), and `Special:Tenants` is its page. (PENDING E33)

### A19. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** 0050 §14
- **Change:** relocates §1–§12

### A20. Issuer codes derived from the founding record

- **Date:** 2026-10-09
- **Source:** 0079 §1, §3, §5, §7
- **Change:** amends §1, §4, §9, §10; extends §2, §11
- **Summary:** A tenant's issuer code, the code its actor keys carry, is no longer its slug: it is derived from the hash of the tenant's founding record, so it is fixed at creation, survives moves and erasure, and is allocated by no one. The slug becomes only a name on the instance, chosen afresh on arrival if taken, and `GET /tenancy` gives each tenant's code as `issuer`. Examples keep writing `librarybase:42` for the derived key. Chapter text: 08 §1.2, §3.1, §6.1, §6.3.

Replaced text: §1, in 08 §1.2; §4, in 08 §3.1; §9, in 08 §6.1; §10, in 08 §6.3.

### A21. Provider tenants are entity sources

- **Date:** 2026-10-09
- **Source:** 0080 §2, §3
- **Change:** amends §5, §10
- **Summary:** A tenant is read by another tenant as an entity source that names it by issuer code, not by taking a registry code: directly on the same instance, through `scatter-adapter-triplespace` elsewhere. A registry code becomes a promotion for a widely read tenant, and a move switches readers between the two ways of reading. Chapter text: 08 §4.1, §4.3, §6.3.

Replaced text: §5, in 08 §4.1.

Replaced text (§5, in 08 §4.3):

> …cannot take a code:

Replaced text: §10, in 08 §6.3.

### A22. Continuations and fixed origin lines

- **Date:** 2026-10-09
- **Source:** 0081 §3, §5
- **Change:** amends §2, §10
- **Summary:** A tenant partition's checkpoint origin line uses the tenant's host as it was when the partition was created or the tenant arrived on the instance, and an alias does not change it (§2). A non-cooperative move appends a `continuation`, `recovered` when the tenant's recovery keys sign it and `unauthorized` otherwise, instead of a rotation with no signature (§10).

Replaced text: §2, in 08 §2.1; §10, in 08 §6.3.

### A23. Shared views under the instance policy, and IDs rewritten on the way out

- **Date:** 2026-10-09
- **Source:** 0082 §1, §3, §5, §5.1, §5.2, §6
- **Change:** amends §3, §5, §6, §7
- **Summary:** The tenancy side of 0082: the shared row of every `view` table is computed under the instance's policy record, which a tenant narrows and never reorders (refused with `ts-instance-policy`); §5's read-side rewrite of a provider tenant's IDs is the same layer that rewrites a response to the consumer's preferred form; and a tenant's own links, derivations and documents overlay the shared rows rather than copying them. Chapter text: 08 §2.5, §4.1; 03 §7, §11.4; 04 §4.9. (REVIEW G1, G2, G5, G7)

Replaced text: 08 §2.5, §4.1; 03 §7, §11.4; 04 §4.9.

### A24. `unit`, `globe` and `calendarmodel` are expanded through the current IRI template

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §9
- **Summary:** The `unit` of a quantity, the `globe` of a coordinate and the `calendarmodel` of a time are stored as prefixed entity IDs (`"unit": "WDQ11573"`, `"unit": "Q7"`, `"unit": "1"` unchanged) and expanded through the current IRI template on output, as §9 already has projections emit every IRI from the current template after an `alias` record; the adapter rewrites them on ingest as it rewrites every other ID, the content hash is over the stored form, and they participate in `entity_ref` and in rewriting. The ledger's verb is amends; the contradicted text is 0003 §7's and 0004 §7's (chapter 04 §4.7), and §9's chapter text (08 §6.1) is unchanged, so this entry extends. (REVIEW G21)

### A25. The founding attestation

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §2
- **Summary:** The record at offset 0 of a tenant's `config`, the first copy of the instance key, carries the founding attestation of 0015 A52: no actor, a signature by the key it registers, and the authority `(instance config, 0)`, the instance's own founding record, which itself has no authority at all; the tenant's issuer code, or the farm code, is derived from the leaf afterwards, so nothing that needs the code has to exist before it. Every later copy, at each rotation, is a written instance act under the ordinary instance attestation. Offsets 1 and 2 of the instance `config`, the primary tenant's `tenant:` record and the first `primary` record, are attested by `instance:{farm code}` with the founding record as authority, so a single-tenant instance needs no authority record and no `ts-prerogative` path before its first entity. The ledger's verb is extends; the chapter sentence below said the founding copy carried the instance attestation, which the decision contradicts, so this entry amends. (REVIEW G22)

Replaced text: 08 §2.4.

### A26. A provider whose issuer is a tenant of this instance is never synced

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §5
- **Summary:** The binding of a registry code to a tenant is enforced on the instance that hosts the tenant: an instance refuses a sync job for a provider whose registry `issuer` is a tenant of this instance, since the tenant's `local` partition is the provider's graph and a mirror of it would be a stale copy; and it refuses to adopt or import a tenant under a code for which it holds a live mirror partition until that partition has been frozen, its `entity_source` rows dropped and composition switched to reading the tenant's `local`. (REVIEW G31)

### A27. Every tenant setting has a bound

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §3
- **Summary:** The settings catalogue gains a bound column, `tenant`, `tenant ≤ instance ceiling` (the ceiling a tenancy switch of the setting's own name) or `instance`, which refines §3's split of configuration by scope: pool sizes and intervals are deployment configuration, budgets and limits are ceiling-bounded, `query.isolation` and `query.max_concurrent` are instance-only, and a tenant-scope record of an instance-only setting is refused with `ts-instance-policy`. The ledger's verb is amends; §3's chapter text (08 §2.4–§2.5) is unchanged by it and the catalogue that changed is 0015 §3's (chapter 23 §3.1–§3.4, logged as 0015 A57), so this entry extends. (REVIEW G35)

### A28. A foreign actor's contributions are what `view.activity` holds

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §8
- **Summary:** `Special:Contributions/{issuer}:{id}` for a foreign actor lists what this tenant's `view.activity` holds for the actor, page revisions imported with that attribution and the rows of the tenant's log that name the actor; upstream revision records in `log/{provider}` are not indexed by actor, so they are not listed, and the page says so. For an actor of another tenant on the same instance the view is complete, since that tenant's `view.activity` indexes every record. The ledger names 0047 §2 and §4; the chapter text that changed is §8's (19 §2.5), so the entry is logged here as well. (REVIEW G50)

Replaced text: 19 §2.5.

### A29. Inherited settings do not travel

- **Date:** 2026-10-17
- **Source:** 0085 §2.3
- **Change:** extends §10
- **Summary:** A tenant's bundle carries the records it wrote. A setting it inherited from its instance's defaults, which 0085 §2.1 makes evaluated rather than copied, travels as no record: after a move it takes the new instance's default, and only the values the tenant set itself move with it. The chapter text is 08 §6.2.

### A30. A joining tenant's palettes and type sets are keyed

- **Date:** 2026-10-18
- **Source:** 0086 A1
- **Change:** extends §10
- **Summary:** When a tenant arrives on an instance, a palette or type set of its own whose name the registry or the instance uses is renamed `{name}~{slug}` with its slug there, with every reference to it, as a provision whose authority is a job record in the instance `log` listing the renames; preferences that arrive later with a reclaimed account are mapped through them. The move is never refused for a name. The chapter text is 08 §6.3.

### A31. Vouching for an adopted wiki's users

- **Date:** 2026-10-18
- **Source:** 0088 §8
- **Change:** extends §10
- **Summary:** An adopted MediaWiki wiki vouches through an issuer that reaches its users while the tenant's `reclaim.vouching` names it: a login the source confirms as user `n` reclaims `{code}:{n}`. Every reclaim writes a public `scatter:v0/reclaim` record with `method` `manual` or `vouched`, from which `reclaim/reclaim` projects (A17), which settles Q8's parameters. The chapter text is 08 §6.4.

### A32. The instance is its own single sign-on

- **Date:** 2026-10-18
- **Source:** 0089 §2, §4, §14
- **Change:** amends §4, §10
- **Summary:** An instance login on the auth host becomes the only way to log in, with each tenant account bound to a login in its tenant's `accounts` partition, so identity stays per tenant. Because that binding means nothing on another instance, a move's binding digest is computed over the external identities of each account's login, and a person reclaims a moved account by logging in with one of them. Chapter text: 08 §3.2, §6.2, §6.4.

Replaced text: 08 §3.2, §6.2, §6.4.

### A33. Multi-tenant round 1: the assumptions the implementation made

- **Date:** 2026-10-30
- **Source:** Direct: Claude, while building multi-tenant round 1 (b4fa37f), 2026-10; unconfirmed
- **Change:** extends §3
- **Summary:** Logged from the working notes on their retirement (0099 §5, 24 §15); each call stands as decided until James confirms or overturns it, and the chapter text that states it is folded on confirmation where it is not already there. The calls:
  - A `farm` instance config kind: `{"kind":"farm","slug","code","base"}`; no `farm` kind appears in the chapters yet.
  - `tenant:` records carry `issuer`.
  - Founding signature preimage: `0x05` for the instance; `0x06` + authority for a tenant; no `actor`.
  - Config order: founding `key:`, `graph:config`, `farm`, then `tenant:`, then `primary`. This conflicts with the chapter's "offsets 1 and 2 are `tenant:` and `primary`".
  - A slug may equal a registered code only when `--adopt`'s host is that issuer's site.
  - Tenant provisions are instance attestations with `binding: false`; `site.name` is owner-attested.
  - `tenant create`'s `tenant:` record is attested by an owner of the primary tenant.
  - Singleton kinds read as `kind:kind`.
  - `GET /tenancy` at a tenant base answers 400 `ts-instance-policy`.
  - Possibly stale: §10's "six partitions" omits `derived/{source}` (0071).

### A34. A tenant move is a sealed transfer; rehosting is a backup restored

- **Date:** 2026-11-07
- **Source:** 0103 §3, §4
- **Change:** amends §10
- **Summary:** A tenant moves as a transfer (0103 §1): every partition but `accounts` with every present part in full, restricted and hidden ones included, and its blobs, sealed whole to the key the receiving instance supplies, not only the private extract; the old instance keeps the partitions read-only for the grace period, so the tenant can move back (James, 2026-11-07). Chapter text: 08 §6.2, §6.7.

Replaced text: 08 §6.2.

### A35. What a move the old instance does not sign carries

- **Date:** 2026-11-07
- **Source:** 0103 §7
- **Change:** amends §10
- **Summary:** A non-cooperative move took its bundle "from backups or from the old instance's public exports" without saying what it may hold. An `unauthorized` continuation carries the export scope; a `recovered` one, signed by the tenant's recovery keys, also its read-restricted and hidden parts, held sealed until the recovery delay has passed uncancelled; suppressed and confidential parts never travel this way. Chapter text: 08 §6.3.

Replaced text: 08 §6.3.

### A36. Instances and tenants created by question and answer

- **Date:** 2026-11-12
- **Source:** Direct: Claude, while building interactive instance and tenant creation, 2026-11-12; unconfirmed
- **Change:** extends §1, §12
- **Summary:** James asked for an interactive creator for instances and tenants, the instance creator leading into the tenant creator for the first tenant, and the tenant creator runnable alone for more tenants. The calls made building it: the commands are `triplespace instance setup` and `triplespace tenant setup`, beside `instance create` and `tenant create`, rather than a flag on those or prompting for missing flags; they ask, check each answer as the commands do, show a summary with the equivalent command line, and write nothing until confirmed, then run the same code as the commands. The primary tenant's questions are asked inside `instance setup`, since `instance create` creates it in the instance's transaction; `instance setup` then offers `tenant setup`, and finding an instance already in the database offers it too. A base may be typed without its scheme (`https://` is assumed) and with a trailing `/` (dropped); the farm base defaults to the primary tenant's and, when different, needs a host of its own; the wiki's name defaults to the slug; the owner's name defaults to `Owner` and is checked as an account name, which `instance create` and `tenant create` do not do; with several `owner` members on the primary tenant, `tenant setup` asks which one acts. The password is typed twice, hidden, and may be left empty. `instance create` and `tenant create` now read the owner's password file before writing anything, so an unreadable file refuses the creation instead of leaving the owner without a password. `triplespace-cli` gains the `dialoguer` crate for the terminal prompts. Chapter text: 08 §1.1; 22 §2.1.

### A37. Tenants are told apart by host, not by path

- **Date:** 2026-11-16
- **Source:** Direct: James, design discussion of 2026-11-16
- **Change:** extends §1
- **Summary:** James considered an alternative mode in which tenants are subdirectories of one host (`example.com/tenant/wiki/Project:Home`) rather than hosts of their own (`tenant.example.com/wiki/Project:Home`), and rejected it as too costly. Every tenant's base has a host of its own, and the server tells tenants apart by host alone, as 0056 §10 already has it (requirement 5 of the deployment boundary). The deciding cost is that tenants sharing a host share a browser origin, which would need a second security model to keep one tenant's scripts from acting with a user's session on another; the reasoning is under *Alternatives considered*. Chapter text: 08 §1.1.
