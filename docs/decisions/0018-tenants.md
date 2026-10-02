# 0018. Tenants

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-01 (A11)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md)
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

*Changed by A5, A8.*

An **instance** is one deployment: one log, one Postgres, one set of mirror partitions, one instance key. A **tenant** is one wiki on it: a base URI, a slug, its own source partitions (§2), its own configuration (§3), its own users (§4). Every instance has at least one tenant. An instance that hosts one wiki is the picture the earlier ADRs drew, and nothing in them changes for it.

Each tenant has a **base URI**, which is the `{base}` of every earlier ADR for that tenant's data. The instance has a **farm base** for what it holds on behalf of every tenant: the mirror graphs, the instance configuration and the sync jobs. On a single-tenant instance the farm base is the tenant's base.

One tenant is the instance's **primary tenant**: the tenant whose accounts operate the instance ([0046](0046-primary-tenant.md) §1). Instance rights are evaluated there, and records the instance holds on its own behalf are attested by its actors as of each record's offset (§3, §4), but none of those records is stored in its partitions. The role is held by one `primary` record in the instance `config`, may be given to any tenant, including an operations tenant with no content, and is transferred by offer and acceptance ([0046](0046-primary-tenant.md) §2–3, §5). On a single-tenant instance it is the only tenant.

The slug names the tenant everywhere: in partition names, in actor keys (§4), and, if the tenant becomes a provider, in `providers.toml` (§5). Slugs, provider slugs and issuer codes are one namespace, allocated in the registry ([0015](0015-record-format-and-partition-registry.md) §5).

**Adoption.** A tenant may be the continuation of a MediaWiki Wikibase that ran before Triplespace. *Adoption* writes that wiki's entities into the tenant's `local` partition under the IDs it minted, its accounts under their own numbers, and sets every sequence past the source's; the `tenant` record gains `adopted_from`. It is neither mirroring (nothing is prefixed, no mirror partition exists) nor a move under §10 (there is no bundle to verify). If the tenant is a provider, §5 applies to adopted entities unchanged.

### 2. Partitions (amends 0005 §4.1–4.2, 0006 §3, 0013 §2 and 0015 §3, §5)

*Changed by A6, A8.*

**Each tenant has its own source partitions**, and the instance holds the shared ones:

| Scope | Partitions | Graph IRI |
|---|---|---|
| Tenant | `local`, `pages`, `log`, `config`, `actors`, `accounts` | `{tenant base}/graph/{name}` |
| Instance | `config`, `log`, `mirror/{provider}`, `actors/{provider}`, `log/{provider}`, `files/{repo}` | `{farm base}/instance/graph/{name}` ([0046](0046-primary-tenant.md) §7) |

The graph registry is keyed by (tenant, name), with the instance's own partitions under a null tenant. The names of [0015](0015-record-format-and-partition-registry.md) §5 are unchanged; what a name denotes now depends on the tenant. Checkpoint origin lines ([0006](0006-log-integrity-and-erasure.md) §6) use the tenant's host, so Librarybase's local log signs as `librarybase.org/log/local`; an instance partition's is `{farm host}/instance/log/{name}`, so that the instance `config` and `log` cannot be confused with a tenant's when the farm base is a tenant's base ([0046](0046-primary-tenant.md) §7).

**Per-tenant sequences.** The revision, log and page IDs of [0015](0015-record-format-and-partition-registry.md) §2 are per tenant, because MediaWiki clients expect one sequence per wiki. The provider-ranged revision ID then works across tenants without a new rule: Librarybase's revision 900 is `900` at home and `LB_number << 40 | 900` when another tenant reads it (§5).

**Partition IDs are 64-bit and random (amends 0006 §3 and 0013 §2).** The header's partition field was "a number assigned when the partition is created", stored as a `smallint`. It is inside the hash, so a partition cannot be renumbered, and two instances would assign the same small numbers. From this ADR a partition's ID is 64 random bits drawn when it is created and never changes. The registry maps it to (tenant, name); `log.partition.partition` and `log.record.partition` become `bigint`; six more bytes per header. This is what makes a tenant's partitions importable by another instance without rehashing (§10).

**The tenant's `config` partition carries a key chain (amends 0015 §3).** The instance key is registered in the instance's `config` (partition 0), which does not travel with a tenant. So every `key:` record of the instance, at registration and at every rotation, is also appended to each tenant's `config` as a `key:` record. A tenant's partitions then verify from the tenant's bundle alone.

**The instance's own `log` and files.** The instance has its own `log` partition (internal) for operator records such as file takedowns, and `files/{repo}` mirror partitions for file repositories. File bytes live outside the log in a blob store whose **scope** is one per tenant by default (`files.separation = tenant`) or one for the instance ([0039](0039-files-and-media.md) §4, §10).

**Scale.** A farm with thousands of tenants has six list partitions per tenant in `log.record`. Postgres tolerates that; whether it performs well is a measurement the farm profile of [0013](0013-postgres-storage.md) §11 has to add before it is relied on.

### 3. Configuration (amends 0015 §3)

*Changed by A2, A5, A8.*

The config kinds of [0015](0015-record-format-and-partition-registry.md) §3 split by scope:

| Scope | Kinds |
|---|---|
| Instance `config` | `key`, `graph`, `provider`, `issuer`, `keyed-type`, and three kinds of this ADR and [0046](0046-primary-tenant.md) §2: `tenant` (slug, base URI, `adopted_from` and, if the tenant is a provider, its code and number), `alias` (§9) and `primary` (§1). Instance-wide policy lists such as `sitelink-policy:deny` ([0026](0026-sitelinks.md) §3) and the other instance-scope kinds later ADRs add are listed in [0015](0015-record-format-and-partition-registry.md) §3 |
| Tenant `config` | `namespace`, `role`, `reconcile`, `site`, `group`, graph ACLs ([0023](0023-moderation.md) §3), the key chain of §2, `providers`: the list of providers whose graphs contribute to this tenant's resolved view and search (§6), with Wikidata as the default, and the tenant-scope kinds later ADRs add, listed in [0015](0015-record-format-and-partition-registry.md) §3 |

A tenant's `config` partition begins, as the instance's does, with a `key:` record: the current instance key, copied in as the first entry of its key chain. Instance-level records are attested by actors of the primary tenant (§4).

### 4. Identity is per tenant (amends 0007 §1 and §8)

*Changed by A4, A7.*

**Each tenant is an issuer.** Its issuer code is its slug, its actor model is numeric, and its actor IRI template is `{tenant base}/user/{id}`. An edit on Librarybase is attested by `librarybase:42`; the same person editing example.wiki is `example:17`, a different actor. `local` in the earlier ADRs is read as "the current tenant's issuer" and is no longer an issuer code of its own.

**Authentication is uncoupled from identity, as OAuth is.** Bindings ([0007](0007-actor-identity.md) §3) map an identity provider's subject to a tenant account and live in the tenant's `accounts` partition. One Wikimedia account may be bound to `librarybase:42` and to `example:17`; that makes them the same person's accounts, not the same actor, and they are linked only by the opt-in act of 0007 §7. A farm that wants single sign-on registers itself as an identity provider (an issuer with `login = true`), which gives one login per tenant account and merges nothing.

**Attribution keeps the account and the tenant it was made under**, as 0007 §7 requires. An edit made on Librarybase stays `librarybase:42` on every instance that ever holds it and under every domain Librarybase ever uses. An edit imported from another wiki ([0008](0008-namespaces-and-document-pages.md) §9) is attributed to that wiki's issuer and numeric ID, or to an `imported` surrogate (0007 §5), and never to an account on the importing tenant; no account is created by an import, and a contributions page for such an actor says whose user they are and that these are imported edits (§8). This is the provenance MediaWiki loses.

**The farm has no user base by default.** Instance-level records are attested by actors of the primary tenant. Memberships and blocks stay in each tenant's `actors` partition as [0016](0016-permissions-and-access-control.md) §3 has them, which is where a farm wants them: groups are per wiki. Under a tenancy policy with farm identity, the farm *is* an issuer with accounts, but those accounts edit nothing: they are the identity a person carries across the farm, publicly linked to the tenant accounts created from them, and what global groups and global blocks key on ([0028](0028-tenancy-policy.md) §2–4). Every record is still attested by a tenant actor, and instance-level records by the primary tenant's, so the attribution rule above does not change.

**Instance acts.** Every record in a tenant's partitions is attested **either by one of the tenant's own actors, or by the instance**: an *instance act* written into the tenant carries an instance attestation, attributed to the operator actor `instance:{farm slug}`, signed by the instance key from the tenant's own key chain, and citing an authority record in an instance partition. Binding instance acts are *prerogatives*; the tenant cannot undo them ([0040](0040-instance-prerogatives.md) §1–3).

### 5. A tenant can be a provider (amends 0002 §4)

*Changed by A4, A5, A11.*

**A tenant's local graph is, to every other tenant, a foreign source graph.** A tenant becomes a provider by taking a code, a slug and a provider number in `providers.toml` ([0015](0015-record-format-and-partition-registry.md) §5), as Librarybase has `LB`, `librarybase` and 2. Nothing else is needed on the instance that hosts it: there is no sync job and no mirror partition, because the tenant's `local` partition is the source.

- **IDs are rewritten when read, not when written.** Librarybase stores `Q6`; example.wiki's projections read Librarybase's partition and rewrite `Q6` to `LBQ6`, `P12` to `LBP12`, exactly as an adapter rewrites `Q42` to `WDQ42` at ingest ([0002](0002-source-graphs-and-mass-ingest.md) §4). References to global entities, `WDQ42` or `domain:x`, pass through unchanged.
- **`https://example.wiki/entity/LBQ6` is an alias** of `https://librarybase.org/entity/Q6`, which is canonical, as 0002 §4 already says for foreign entities.
- **Reconciliation is unchanged.** A Librarybase assertion about `WDQ42` is, on example.wiki, a foreign assertion with an `LB` source chip ([0003](0003-statement-ui.md) §6), and example.wiki's local graph wins over it ([0002](0002-source-graphs-and-mass-ingest.md) §3). Librarybase's `same-as` and `convert` records are tier-1 links for Librarybase and tier-2 links for everyone else ([0004](0004-identity-clusters-and-equivalence.md) §3), ranked where the reading tenant's provider order puts `LB`.
- **Opt-in.** A tenant reads a provider's graph only if the provider is in its `providers` list (§3). A tenant that has not opted into Librarybase never sees `LBQ6`. Under `providers.between_tenants = operator`, only the farm operator can make a tenant a provider; with `providers.reader_lists`, a provider tenant restricts who reads it with a `provider-readers` record in its own `config` ([0028](0028-tenancy-policy.md) §5).
- **A tenant without a code cannot be referenced.** Its IDs have no absolute form. A **private tenant**, one whose `tenant` ACL restricts `read` ([0056](0056-security-model.md) §3), cannot take a code: its partitions are exported as `private` for every purpose but the operator's own backups and a move (§10), and nothing of it is read by another tenant.
- **Leaving the instance changes nothing for referrers.** When a provider tenant moves away (§10), the instance registers a real sync job against its new home, run by the Triplespace adapter over the provider's dump and stream ([0028](0028-tenancy-policy.md) §5, [0022](0022-federation.md) §2), and `LBQ6` means what it always meant. A deleted tenant is one that moved away with no destination: its partitions stay read-only for a grace period, its former referrers see its last state with the provider marked gone, and then they are erased ([0028](0028-tenancy-policy.md) §5). The same code serves off-instance readers on other instances from the start, since the registry is global.

### 6. Shared views and tenant overlays (amends 0013 §5, 0014 §7)

Everything derived from shared source graphs alone is computed once, for the instance. Everything a tenant's own graph changes is an **overlay** keyed by tenant.

- **Entities.** `view.entity` and its dependents gain a `tenant` column. A row with a null tenant (the empty string, since a key cannot hold `NULL`; [0013](0013-postgres-storage.md) A11) is the shared row for a mirrored entity, computed from the shared graphs. A tenant that has local assertions about the entity, a cluster link touching it, or a correction on it gets its own `(tenant, id)` row; a tenant with none reads the shared one. This is [0013](0013-postgres-storage.md) §5.1's "materialize only where it differs", applied per tenant. `view.term`, `view.identifier`, `view.entity_ref`, `view.statement_assertion` and `view.correction` follow the same rule; `view.page`, `view.activity`, `view.job` and the actor tables are per tenant outright.
- **Clusters.** Tier-2 and tier-3 links come from shared graphs and are instance-wide; tier-1 links are the tenant's own, and its `different-from` blocks are too. A tenant's clusters are the shared clusters with its overlay applied, recomputed for the entities its links touch ([0004](0004-identity-clusters-and-equivalence.md) §2). A conflict is a conflict for the tenant that holds the links that cause it.
- **Search.** One shared `entities` index per provider, holding that provider's canonical entities as the shared view has them, and one index per tenant holding the tenant's local entities and its overlay documents. `/suggest` and `wbsearchentities` run one `msearch` across the tenant's own index and the indexes of its opted-in providers; where a tenant document and a shared document share an ID, the tenant's wins. `incoming_links` on shared documents is the instance-wide count. Duplicating a provider's index per tenant is not an option at Wikidata scale, so the overlay is the only shape that works.
- **Caching.** Cache keys ([0014](0014-caches-and-search.md) §4) gain the tenant where the value is tenant-specific and omit it where it is shared, so a mirrored entity's JSON is cached once for every tenant that reads it unchanged.

### 7. Statement IDs follow the canonical ID (settles 0002 and 0004)

**Statement IDs in the log never change.** What clients see is derived: **the resolved view rewrites the entity-ID part of a statement GUID to the entity's canonical ID and keeps the UUID.** This is the same rewrite [0004](0004-identity-clusters-and-equivalence.md) §4 already applies to subjects and entity values.

- example.wiki converts `WDQ42` to `Q9` ([0002](0002-source-graphs-and-mass-ingest.md) §6, [0004](0004-identity-clusters-and-equivalence.md) §5). The mirror keeps writing `WDQ42$abc`; example.wiki shows `Q9$abc`.
- Librarybase does the same; example.wiki, reading Librarybase, shows `LBQ9$abc`.
- An unconverted cluster {`WDQ42`, `OAW123`} shows the OpenAlex statements as `WDQ42$oa-uuid`.

The UUID alone identifies a statement, so overrides ([0002](0002-source-graphs-and-mass-ingest.md) §7), the provenance response ([0004](0004-identity-clusters-and-equivalence.md) §8, which lists member statements in their source form) and API writes all resolve through it. Every GUID a client sees has its entity's ID as its prefix, which is Wikibase's invariant. Re-minting statements on conversion was rejected: it would copy mirrored statements into the local graph and recreate the stale-copy problem 0002 §5 rejected for rescue.

### 8. Contributions (settles 0010 and 0012; amends 0010 §8)

Contributions are per account ([0007](0007-actor-identity.md), Consequences), and an account belongs to one tenant (§4). Three views follow:

| View | Shows |
|---|---|
| `Special:Contributions/{name}` on a tenant | That tenant's records attested by that tenant's actor: its `local`, `pages`, `log` and `actors` partitions |
| `Special:GlobalContributions/{name}` on a tenant | The same for each account **linked** to it (0007 §7), on every tenant of the instance the viewer's tenant has opted into. Nothing is matched by name. |
| `Special:Contributions/{issuer}:{id}` for a foreign actor | **What the instance holds:** upstream revision records keyed to that actor in `log/{provider}` ([0015](0015-record-format-and-partition-registry.md) §4), and page revisions imported with that attribution. The page says whose user this is, links to their page on the issuer's site, and does not fetch upstream, because per-actor fetching is unbounded. |

For an actor of another tenant on the same instance the third view is complete, since the instance holds every record. For a Wikidata account it is the sparse subset the instance has observed or backfilled, and says so. `/actor/{key}/contributions` ([0012](0012-api-requirements.md) §5) serves the same three cases.

### 9. Aliases

When a tenant, provider or issuer changes its base URI, the change is an `alias` record in the instance `config`, keyed by the old host and carrying the new base and the date. Its effects:

- Projections emit every IRI from the current template; nothing is rewritten in the log.
- The metadata graph carries one triple per alias, `<{old base}/> dcterms:isReplacedBy <{new base}/>`, using the term [0001](0001-revision-metadata-rdf.md) §6 reserved. Consumers holding old dumps rewrite prefixes; nothing is emitted per actor or per entity.
- The old host serves redirects for as long as anyone controls it.

So `domains.wikibase.cloud` becoming `internetdomains.wiki` is one record and one triple, and every `internetdomains:…` actor key and every entity IRI ever emitted stays resolvable.

### 10. Moving a tenant (extends 0006 §6 and §9)

*Changed by A3, A7, A8.*

**What moves.** The tenant's six partitions (§2) as an export bundle ([0006](0006-log-integrity-and-erasure.md) §9), except `accounts`, which is private and never exported ([0007](0007-actor-identity.md) §8). The bundle carries the tenant's IDs (in headers), its surrogates ([0009](0009-keyed-entity-types-and-domain.md) §7), its actors, memberships and blocks, its configuration and its key chain. Three extracts and a digest go with it: the **retention extract**, every record in the instance's `mirror/*` and `log/{provider}` partitions keyed to an entity the tenant has set `retain` on, including backfilled upstream revisions, which [0002](0002-source-graphs-and-mass-ingest.md) §5 warns may be unrecoverable later; and the **binding digest**, an HMAC of each (issuer, subject) in the tenant's `accounts`, keyed with a secret passed operator to operator, so the receiving instance learns nothing until a subject logs in; on a cooperative move, the **private extract**, every account's user data bundle sealed to a key the receiving instance supplies and applied only when the account is reclaimed ([0027](0027-preferences-and-portability.md) §4); and the **authority extract**, every authority record the bundle's instance attestations cite, with an inclusion proof and with the operator's attestation withheld ([0040](0040-instance-prerogatives.md) §8). Prerogatives of the old instance stop binding once the tenant is registered elsewhere; they stay in history, verifiable through the old key. Views, indexes and caches are rebuilt on arrival.

**Procedure, cooperatively.** The old instance freezes the tenant read-only and writes a final checkpoint. The new instance supplies its public key, and the old instance appends a `key:` rotation record to the tenant's key chain naming the new key and the final checkpoint, signed by the old key as [0006](0006-log-integrity-and-erasure.md) §6 rotation is. The new instance runs `verify` on the bundle, registers the six partitions under their existing IDs, replays projections and rebuilds search, re-appends the retention extract as a job with `pav:retrievedFrom` naming the old instance, and maps the tenant's `providers` list onto its own mirrors, which is trivial because slugs come from the same registry. DNS moves, or an alias record is written (§9). If the tenant is a provider, the old instance replaces its direct read with a sync job against the new home (§5).

**Account reclaiming.** Bindings do not travel, so a user proves on the new instance that they are `librarybase:42` in one of three ways: by logging in with the same identity provider, which the new instance checks against the binding digest; by the old instance vouching, acting as an identity provider for a grace period; or by a bureaucrat recording a manual reclaim as a `reclaim/reclaim` log event, which is MediaWiki's practice of confirming identity on the home wiki. An account never reclaimed keeps its name and its history and cannot log in.

**Non-cooperatively**, the bundle comes from backups, the rotation record carries no signature from the old key, verifiers are told the chain has an unsigned link, and reclaiming is manual.

**The primary tenant cannot leave.** It cannot be frozen for a move, moved or deleted until the primary role has been transferred to another tenant (`ts-primary-tenant`); on a single-tenant instance, moving the only tenant is rehosting the instance ([0046](0046-primary-tenant.md) §5).

**Rehosting an instance** whole is a Postgres restore plus the rejects files and the `private` schema. No ID, key or attestation changes, and a change of farm base is an alias.

### 11. API and UI (amends 0012 §4–5)

*Changed by A9.*

- **The host selects the tenant.** Every route is served per tenant base; `meta=siteinfo` describes the tenant, and `siprop=triplespace` names it and its opted-in providers. Instance-level routes (`/jobs` for sync jobs, the tenant list) are served at the farm base and, on a single-tenant instance, at the tenant's.
- **`Special:GlobalContributions`** (§8) and, for administrators, **`Special:Tenants`** at the farm base. Every special page has a scope in `docs/registry/special-pages.toml`: `farm` pages are served at the farm base, `tenant` pages at each tenant's base, and `both` pages at either ([0047](0047-special-pages.md) §3).
- **Jobs.** Mirror sync jobs are instance jobs, attested by the primary tenant's actors and listed at the farm base; local bulk jobs are tenant jobs.
- **Identity line.** A foreign tenant's entity shows "Item from Librarybase" with the `LB` chip ([0010](0010-site-ui.md) §2), as a Wikidata item shows `WD`.

### 12. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed. The table this section first gave is in A1.

## Consequences

- **A single wiki is a one-tenant instance,** and nothing in the earlier ADRs changes for it except two header bytes and a key record in its own `config`.
- **Provenance travels.** Every edit names the wiki and account it was made under, for as long as it exists anywhere, and no import or migration can re-attribute it.
- **A wiki can leave.** Its partitions, IDs, actors and history move as a unit; its references from elsewhere keep working; its users reclaim their accounts rather than inherit them.
- **Tenants as providers cost nothing to host.** No sync, no mirror, no copy; the rewrite happens on read.
- **Two open questions from 0002/0004 and one from 0010/0012 close**, each with one rule.
- **Projections are more complex.** Every entity view is shared-or-overlay, and every cache key has to know which. That is the price of not copying Wikidata per tenant.
- **Partition counts grow with tenants.** Six per tenant; the farm profile must measure Postgres at the intended scale.
- ~~**A farm's operators are a tenant's users.** There is no farm-level account; the primary tenant plays that role.~~ *A farm may have farm accounts, which edit nothing (A4); the primary tenant's accounts still operate the instance.*

## Open questions

- **Q1. Partition count at farm scale,** and whether small tenants should share physical tables with the tenant in the key while keeping logical partitions. [0028](0028-tenancy-policy.md) §2 adds three instance partitions per farm.
- **Q2.** ~~**Cross-tenant ACLs.** Whether a provider tenant may restrict which tenants read it, beyond public-or-private.~~ *Settled by [0028](0028-tenancy-policy.md) §5: reader lists, a `provider-readers` config record in the provider tenant's own `config`.*
- **Q3.** ~~**Global blocks.** Whether the primary tenant may block an identity-provider subject across every tenant, as farms do.~~ *Settled by [0028](0028-tenancy-policy.md) §4: a block on a farm account, by policy; IP blocks are farm-wide under every preset.*
- **Q4.** ~~**Deleting a tenant**, and what its former referrers see.~~ *Settled by [0028](0028-tenancy-policy.md) §5: a move with no destination; partitions kept read-only for a grace period, referrers see the last mirrored state with the provider marked gone, then erasure. The grace period's length is open there.*
- **Q5. Overlay index cost** for a tenant that annotates a large share of a provider's entities.
- **Q6. Reclaim log event** name and parameters, and whether reclaiming should be reversible.
- **Q7. The retention extract's integrity.** Records lifted from `hashed` partitions carry headers but not whole segments; the receiving instance re-appends them under its own key, which is the guarantee mirrors already have.

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
