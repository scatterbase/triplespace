# 0018. Tenants

- **Status:** Proposed
- **Date:** 2026-09-27
- **Author:** James Hare / Claude Fable
- **Amended by:** [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§7 follows §4 and §10: a subsidiary belongs to its operator's tenant, and there are no cross-tenant bots), [0026 — Sitelinks are URLs](0026-sitelinks.md) (§3 extends §3: an instance deny list and tenant allow or deny lists), [0027 — Preferences, private state and portability](0027-preferences-and-portability.md) (§4 extends §10: a cooperative move carries a per-account private extract, applied on reclaim), [0028 — Tenancy policy](0028-tenancy-policy.md) (§2 amends §4: with farm identity the farm has accounts that edit nothing and tenant accounts are created and linked from them; §5 extends §5 with reader lists and cross-instance sync; settles the cross-tenant ACL, global block and tenant-deletion open questions)
- **Related:** [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (§5 amends §4; §7 settles the statement-IDs-after-conversion open question), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md) (§6 extends §3; §7 settles the statement-IDs open question), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (amends §2, §4.1 and §4.2), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (§2 amends the partition field of §3; §10 extends §6 and §9), [0007 — Actor identity](0007-actor-identity.md) (§4 amends §1 and §8), [0010 — Site UI](0010-site-ui.md) (§8 amends §8 and settles the contributions-of-foreign-actors open question), [0012 — API requirements for the site UI](0012-api-requirements.md) (§8 settles the contributions open question; §11 amends §4–5), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (amends §2 and §5), [0014 — Cache layers and search](0014-caches-and-search.md) (§6 amends §7), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (amends §3 and §5; §2 extends §2), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§3), [0017 — Entity ID grammar](0017-entity-id-grammar.md) (§1), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§9 follows §5: data federation is between tenants wherever hosted), [0025 — The instance as an OAuth server](0025-oauth-server.md) (§7 follows §4: a token acts in one tenant)

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

An **instance** is one deployment: one log, one Postgres, one set of mirror partitions, one instance key. A **tenant** is one wiki on it: a base URI, a slug, its own source partitions (§2), its own configuration (§3), its own users (§4). Every instance has at least one tenant. An instance that hosts one wiki is the picture the earlier ADRs drew, and nothing in them changes for it.

Each tenant has a **base URI**, which is the `{base}` of every earlier ADR for that tenant's data. The instance has a **farm base** for what it holds on behalf of every tenant: the mirror graphs, the instance configuration and the sync jobs. On a single-tenant instance the farm base is the tenant's base.

One tenant is the instance's **primary tenant**. Its actors attest instance-level records (§3, §4). On a single-tenant instance that is the only tenant; on a farm it is whichever tenant the operators run the farm from.

The slug names the tenant everywhere: in partition names, in actor keys (§4), and, if the tenant becomes a provider, in `providers.toml` (§5). Slugs, provider slugs and issuer codes are one namespace, allocated in the registry ([0015](0015-record-format-and-partition-registry.md) §5).

### 2. Partitions (amends 0005 §4.1–4.2, 0006 §3, 0013 §2 and 0015 §3, §5)

**Each tenant has its own source partitions**, and the instance holds the shared ones:

| Scope | Partitions | Graph IRI |
|---|---|---|
| Tenant | `local`, `pages`, `log`, `config`, `actors`, `accounts` | `{tenant base}/graph/{name}` |
| Instance | `config`, `mirror/{provider}`, `actors/{provider}`, `log/{provider}` | `{farm base}/graph/{name}` |

The graph registry is keyed by (tenant, name), with the instance's own partitions under a null tenant. The names of [0015](0015-record-format-and-partition-registry.md) §5 are unchanged; what a name denotes now depends on the tenant. Checkpoint origin lines ([0006](0006-log-integrity-and-erasure.md) §6) use the tenant's host, so Librarybase's local log signs as `librarybase.org/log/local`.

**Per-tenant sequences.** The revision, log and page IDs of [0015](0015-record-format-and-partition-registry.md) §2 are per tenant, because MediaWiki clients expect one sequence per wiki. The provider-ranged revision ID then works across tenants without a new rule: Librarybase's revision 900 is `900` at home and `LB_number << 40 | 900` when another tenant reads it (§5).

**Partition IDs are 64-bit and random (amends 0006 §3 and 0013 §2).** The header's partition field was "a number assigned when the partition is created", stored as a `smallint`. It is inside the hash, so a partition cannot be renumbered, and two instances would assign the same small numbers. From this ADR a partition's ID is 64 random bits drawn when it is created and never changes. The registry maps it to (tenant, name); `log.partition.partition` and `log.record.partition` become `bigint`; six more bytes per header. This is what makes a tenant's partitions importable by another instance without rehashing (§10).

**The tenant's `config` partition carries a key chain (amends 0015 §3).** The instance key is registered in the instance's `config` (partition 0), which does not travel with a tenant. So every `key:` record of the instance, at registration and at every rotation, is also appended to each tenant's `config` as a `key:` record. A tenant's partitions then verify from the tenant's bundle alone.

**Scale.** A farm with thousands of tenants has six list partitions per tenant in `log.record`. Postgres tolerates that; whether it performs well is a measurement the farm profile of [0013](0013-postgres-storage.md) §11 has to add before it is relied on.

### 3. Configuration (amends 0015 §3)

The config kinds of [0015](0015-record-format-and-partition-registry.md) §3 split by scope:

| Scope | Kinds |
|---|---|
| Instance `config` | `key`, `graph`, `provider`, `issuer`, `keyed-type`, and two new kinds: `tenant` (slug, base URI, primary flag, and, if the tenant is a provider, its code and number) and `alias` (§9) |
| Tenant `config` | `namespace`, `role`, `reconcile`, `site`, `group`, `acl` ([0016](0016-permissions-and-access-control.md)), the key chain of §2, and `providers`: the list of providers whose graphs contribute to this tenant's resolved view and search (§6). The default is Wikidata. |

A tenant's `config` partition begins, as the instance's does, with a `key:` record: the current instance key, copied in as the first entry of its key chain. Instance-level records are attested by actors of the primary tenant (§4).

### 4. Identity is per tenant (amends 0007 §1 and §8)

**Each tenant is an issuer.** Its issuer code is its slug, its actor model is numeric, and its actor IRI template is `{tenant base}/user/{id}`. An edit on Librarybase is attested by `librarybase:42`; the same person editing example.wiki is `example:17`, a different actor. `local` in the earlier ADRs is read as "the current tenant's issuer" and is no longer an issuer code of its own.

**Authentication is uncoupled from identity, as OAuth is.** Bindings ([0007](0007-actor-identity.md) §3) map an identity provider's subject to a tenant account and live in the tenant's `accounts` partition. One Wikimedia account may be bound to `librarybase:42` and to `example:17`; that makes them the same person's accounts, not the same actor, and they are linked only by the opt-in act of 0007 §7. A farm that wants single sign-on registers itself as an identity provider (an issuer with `login = true`), which gives one login per tenant account and merges nothing.

**Attribution keeps the account and the tenant it was made under**, as 0007 §7 requires. An edit made on Librarybase stays `librarybase:42` on every instance that ever holds it and under every domain Librarybase ever uses. An edit imported from another wiki ([0008](0008-namespaces-and-document-pages.md) §9) is attributed to that wiki's issuer and numeric ID, or to an `imported` surrogate (0007 §5), and never to an account on the importing tenant; no account is created by an import, and a contributions page for such an actor says whose user they are and that these are imported edits (§8). This is the provenance MediaWiki loses.

**The farm has no user base.** Instance-level records are attested by actors of the primary tenant. Memberships and blocks stay in each tenant's `actors` partition as [0016](0016-permissions-and-access-control.md) §3 has them, which is where a farm wants them: groups are per wiki.

> **Amended by [0028](0028-tenancy-policy.md) §2–4.** Under a tenancy policy with farm identity, the farm *is* an issuer with accounts, but those accounts edit nothing: they are the identity a person carries across the farm, publicly linked to the tenant accounts created from them, and what global groups and global blocks key on. Every record is still attested by a tenant actor, and instance-level records by the primary tenant's, so nothing in this section's attribution rule changes.

### 5. A tenant can be a provider (amends 0002 §4)

**A tenant's local graph is, to every other tenant, a foreign source graph.** A tenant becomes a provider by taking a code, a slug and a provider number in `providers.toml` ([0015](0015-record-format-and-partition-registry.md) §5), as Librarybase has `LB`, `librarybase` and 2. Nothing else is needed on the instance that hosts it: there is no sync job and no mirror partition, because the tenant's `local` partition is the source.

- **IDs are rewritten when read, not when written.** Librarybase stores `Q6`; example.wiki's projections read Librarybase's partition and rewrite `Q6` to `LBQ6`, `P12` to `LBP12`, exactly as an adapter rewrites `Q42` to `WDQ42` at ingest ([0002](0002-source-graphs-and-mass-ingest.md) §4). References to global entities, `WDQ42` or `domain:x`, pass through unchanged.
- **`https://example.wiki/entity/LBQ6` is an alias** of `https://librarybase.org/entity/Q6`, which is canonical, as 0002 §4 already says for foreign entities.
- **Reconciliation is unchanged.** A Librarybase assertion about `WDQ42` is, on example.wiki, a foreign assertion with an `LB` source chip ([0003](0003-statement-ui.md) §6), and example.wiki's local graph wins over it ([0002](0002-source-graphs-and-mass-ingest.md) §3). Librarybase's `same-as` and `convert` records are tier-1 links for Librarybase and tier-2 links for everyone else ([0004](0004-identity-clusters-and-equivalence.md) §3), ranked where the reading tenant's provider order puts `LB`.
- **Opt-in.** A tenant reads a provider's graph only if the provider is in its `providers` list (§3). A tenant that has not opted into Librarybase never sees `LBQ6`.
- **A tenant without a code cannot be referenced.** Its IDs have no absolute form. A private tenant, whose partitions carry the `private` export policy, cannot take a code.
- **Leaving the instance changes nothing for referrers.** When a provider tenant moves away (§10), the instance registers a real sync job against its new home, and `LBQ6` means what it always meant. The same code serves off-instance readers on other instances from the start, since the registry is global.

### 6. Shared views and tenant overlays (amends 0013 §5, 0014 §7)

Everything derived from shared source graphs alone is computed once, for the instance. Everything a tenant's own graph changes is an **overlay** keyed by tenant.

- **Entities.** `view.entity` and its dependents gain a `tenant` column. A row with a null tenant (the empty string, since a key cannot hold `NULL`; [0013](0013-postgres-storage.md) §5, as amended 2026-09-27) is the shared row for a mirrored entity, computed from the shared graphs. A tenant that has local assertions about the entity, a cluster link touching it, or a correction on it gets its own `(tenant, id)` row; a tenant with none reads the shared one. This is [0013](0013-postgres-storage.md) §5.1's "materialize only where it differs", applied per tenant. `view.term`, `view.identifier`, `view.entity_ref`, `view.statement_assertion` and `view.correction` follow the same rule; `view.page`, `view.activity`, `view.job` and the actor tables are per tenant outright.
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

**What moves.** The tenant's six partitions (§2) as an export bundle ([0006](0006-log-integrity-and-erasure.md) §9), except `accounts`, which is private and never exported ([0007](0007-actor-identity.md) §8). The bundle carries the tenant's IDs (in headers), its surrogates ([0009](0009-keyed-entity-types-and-domain.md) §7), its actors, memberships and blocks, its configuration and its key chain. Two extracts go with it: the **retention extract**, every record in the instance's `mirror/*` and `log/{provider}` partitions keyed to an entity the tenant has set `retain` on, including backfilled upstream revisions, which [0002](0002-source-graphs-and-mass-ingest.md) §5 warns may be unrecoverable later; and the **binding digest**, an HMAC of each (issuer, subject) in the tenant's `accounts`, keyed with a secret passed operator to operator, so the receiving instance learns nothing until a subject logs in. Views, indexes and caches are rebuilt on arrival.

**Procedure, cooperatively.** The old instance freezes the tenant read-only and writes a final checkpoint. The new instance supplies its public key, and the old instance appends a `key:` rotation record to the tenant's key chain naming the new key and the final checkpoint, signed by the old key as [0006](0006-log-integrity-and-erasure.md) §6 rotation is. The new instance runs `verify` on the bundle, registers the six partitions under their existing IDs, replays projections and rebuilds search, re-appends the retention extract as a job with `pav:retrievedFrom` naming the old instance, and maps the tenant's `providers` list onto its own mirrors, which is trivial because slugs come from the same registry. DNS moves, or an alias record is written (§9). If the tenant is a provider, the old instance replaces its direct read with a sync job against the new home (§5).

**Account reclaiming.** Bindings do not travel, so a user proves on the new instance that they are `librarybase:42` in one of three ways: by logging in with the same identity provider, which the new instance checks against the binding digest; by the old instance vouching, acting as an identity provider for a grace period; or by a bureaucrat recording a manual reclaim as a `reclaim/reclaim` log event, which is MediaWiki's practice of confirming identity on the home wiki. An account never reclaimed keeps its name and its history and cannot log in.

**Non-cooperatively**, the bundle comes from backups, the rotation record carries no signature from the old key, verifiers are told the chain has an unsigned link, and reclaiming is manual.

**Rehosting an instance** whole is a Postgres restore plus the rejects files and the `private` schema. No ID, key or attestation changes, and a change of farm base is an alias.

### 11. API and UI (amends 0012 §4–5)

- **The host selects the tenant.** Every route is served per tenant base; `meta=siteinfo` describes the tenant, and `siprop=triplespace` names it and its opted-in providers. Instance-level routes (`/jobs` for sync jobs, the tenant list) are served at the farm base and, on a single-tenant instance, at the tenant's.
- **`Special:GlobalContributions`** (§8) and, for administrators, **`Special:Tenants`** at the farm base.
- **Jobs.** Mirror sync jobs are instance jobs, attested by the primary tenant's actors and listed at the farm base; local bulk jobs are tenant jobs.
- **Identity line.** A foreign tenant's entity shows "Item from Librarybase" with the `LB` chip ([0010](0010-site-ui.md) §2), as a Wikidata item shows `WD`.

### 12. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-log` | 64-bit partition IDs (§2); the `tenant` and `alias` config kinds and the key chain (§2–3) |
| `scatter-log-postgres`, `triplespace-db` | `partition` as `bigint`; the `tenant` column on `view` tables (§6); one child table per tenant partition |
| `scatter-providers` | Rewriting a tenant's local IDs to its provider form when read (§5) |
| `scatter-actors` | The tenant as issuer; `local` as the current tenant (§4) |
| `scatter-wikibase-resolve` | The GUID rewrite (§7) |
| `triplespace-projections` | Shared rows and tenant overlays; direct reads of provider tenants' partitions (§5–6) |
| `triplespace-search` | Per-provider and per-tenant indexes and the fan-out (§6) |
| `triplespace-titles`, `triplespace-api-*` | Host-to-tenant resolution (§11) |
| `triplespace-accounts` | Bindings per tenant; reclaiming (§10) |
| `triplespace-cli` | `tenant create`, `tenant export`, `tenant import`, the binding digest (§10) |

No crate is added.

## Consequences

- **A single wiki is a one-tenant instance,** and nothing in the earlier ADRs changes for it except two header bytes and a key record in its own `config`.
- **Provenance travels.** Every edit names the wiki and account it was made under, for as long as it exists anywhere, and no import or migration can re-attribute it.
- **A wiki can leave.** Its partitions, IDs, actors and history move as a unit; its references from elsewhere keep working; its users reclaim their accounts rather than inherit them.
- **Tenants as providers cost nothing to host.** No sync, no mirror, no copy; the rewrite happens on read.
- **Two open questions from 0002/0004 and one from 0010/0012 close**, each with one rule.
- **Projections are more complex.** Every entity view is shared-or-overlay, and every cache key has to know which. That is the price of not copying Wikidata per tenant.
- **Partition counts grow with tenants.** Six per tenant; the farm profile must measure Postgres at the intended scale.
- **A farm's operators are a tenant's users.** There is no farm-level account; the primary tenant plays that role.

## Open questions

- **Partition count at farm scale,** and whether small tenants should share physical tables with the tenant in the key while keeping logical partitions. [0028](0028-tenancy-policy.md) §2 adds three instance partitions per farm.
- ~~**Cross-tenant ACLs.** Whether a provider tenant may restrict which tenants read it, beyond public-or-private.~~ *Settled by [0028](0028-tenancy-policy.md) §5: reader lists, a `provider-readers` config record in the provider tenant's own `config`.*
- ~~**Global blocks.** Whether the primary tenant may block an identity-provider subject across every tenant, as farms do.~~ *Settled by [0028](0028-tenancy-policy.md) §4: a block on a farm account, by policy; IP blocks are farm-wide under every preset.*
- ~~**Deleting a tenant**, and what its former referrers see.~~ *Settled by [0028](0028-tenancy-policy.md) §5: a move with no destination; partitions kept read-only for a grace period, referrers see the last mirrored state with the provider marked gone, then erasure. The grace period's length is open there.*
- **Overlay index cost** for a tenant that annotates a large share of a provider's entities.
- **Reclaim log event** name and parameters, and whether reclaiming should be reversible.
- **The retention extract's integrity.** Records lifted from `hashed` partitions carry headers but not whole segments; the receiving instance re-appends them under its own key, which is the guarantee mirrors already have.

## References

- [0007 — Actor identity](0007-actor-identity.md), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md)
- [MediaWiki:Manual:Importing XML dumps](https://www.mediawiki.org/wiki/Manual:Importing_XML_dumps) (imported edits and local user rows)
- [Extension:CentralAuth](https://www.mediawiki.org/wiki/Extension:CentralAuth) and [Special:GlobalContributions](https://www.mediawiki.org/wiki/Extension:GlobalContributions)
- [Dublin Core Terms](https://www.dublincore.org/specifications/dublin-core/dcmi-terms/) (`dcterms:isReplacedBy`)
- [C2SP tlog-checkpoint](https://c2sp.org/tlog-checkpoint) (origin lines)
