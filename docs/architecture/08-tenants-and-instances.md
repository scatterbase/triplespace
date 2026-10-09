# 08. Tenants and instances

This chapter describes how one deployment of Triplespace, an instance, hosts one or many wikis, its tenants: what an instance and a tenant each own, the partitions and configuration that split by scope, identity per tenant and the optional farm identity above it, a tenant read by other tenants as a provider, the tenancy policy and its three presets, aliases and the moving, leaving, joining and deletion of a tenant, the primary tenant whose accounts operate the instance and the farm slug that names the instance itself, the instance's own acts on a tenant's data (prerogatives and provisions, the instance attestation, authority records), tenant isolation in the query service and in packed storage, and what each later feature does at a tenant boundary. It assumes the log, partitions, records, attestations, key chains and export bundles of [01](01-log-and-records.md); the graphs, the update stream and the query service of [02](02-graphs-rdf-and-query.md); the `view` schema, the shared views and tenant overlays, the blob store and the caches of [03](03-storage-caches-and-search.md); the ID forms and identity clusters of [04](04-entities-and-identifiers.md); providers and ingest, including adoption, in [05](05-providers-and-ingest.md); actors, accounts, bindings and subsidiaries in [07](07-actors-and-accounts.md); and the permission catalogue, groups, global groups, blocks and ACLs in [09](09-security-and-moderation.md).

## 1. Instances and tenants

*Sources: [0018](../decisions/0018-tenants.md) §1; [0046](../decisions/0046-primary-tenant.md) §1, §6; [0040](../decisions/0040-instance-prerogatives.md) §2.*

### 1.1 An instance is one deployment; a tenant is one wiki on it

*Sources: [0018](../decisions/0018-tenants.md) §1.*

An **instance** is one deployment: one log, one Postgres, one set of mirror partitions, one instance key. A **tenant** is one wiki on it: a base URI, a slug, its own source partitions (§2), its own configuration (§2.5), its own users (§3). Every instance has at least one tenant. An instance that hosts one wiki is the picture the earlier ADRs drew, and nothing in them changes for it.

Each tenant has a **base URI**, which is the `{base}` of every earlier ADR for that tenant's data. The instance has a **farm base** for what it holds on behalf of every tenant: the mirror graphs, the instance configuration and the sync jobs. On a single-tenant instance the farm base is the tenant's base. Instance IRIs have their own path under the farm base, `{farm base}/instance/…` ([0046](../decisions/0046-primary-tenant.md) §7, in [02](02-graphs-rdf-and-query.md)).

One tenant is the instance's **primary tenant**: the tenant whose accounts operate the instance (§7). On a single-tenant instance it is the only tenant.

### 1.2 Slugs, codes and the farm slug

*Sources: [0018](../decisions/0018-tenants.md) §1; [0046](../decisions/0046-primary-tenant.md) §6; [0040](../decisions/0040-instance-prerogatives.md) §2; [0079](../decisions/0079-derived-issuer-codes.md) §1, §2, §3, §4.*

**The slug names the tenant on its instance**: in the instance-scope `tenant` and `graph` records that register it and its partitions ([23](23-configuration-and-registry.md) §1.3), in hostnames by convention, in the CLI and the API's tenant list, and, if the tenant becomes a provider, in `providers.toml` (§4). It is chosen when the tenant is created and is unique on the instance: tenant slugs, provider slugs and registered issuer codes are one namespace there, and a slug may not take a name the registry has ([0015](../decisions/0015-record-format-and-partition-registry.md) §5). No slug can be empty ([03](03-storage-caches-and-search.md) §4.1). **A slug is a name, not an identity** ([0079](../decisions/0079-derived-issuer-codes.md) §3): it is in no actor key and in no record of the tenant's own partitions, since the records that hold it are instance-scope and do not travel (§6.2). Two instances may each have a `librarybase`, and a tenant that arrives where its slug is taken is registered under another (§6.3).

**A tenant's issuer code is derived from its founding record** ([0079](../decisions/0079-derived-issuer-codes.md) §1, §2). The founding record is the record at offset 0 of the tenant's `config` partition, the `key:` record that opens its key chain (§2.4). The issuer code is the first 26 characters of the lower-case RFC 4648 base32 encoding, without padding, of that record's Merkle leaf hash, `H(0x00 ‖ header)` ([01](01-log-and-records.md) §3.3): 130 bits in `a`–`z` and `2`–`7`, such as `vk3q7zcx2m4hrt6wd5nbfy2lpa`. The leaf covers the partition's random ID and a commitment over salted parts, so no two founding records share it. The code is fixed at creation and never changes: a move appends to the key chain and never touches offset 0 (§6.3), and erasure leaves the leaf in place ([01](01-log-and-records.md) §5). Nothing allocates it, so no registry of instances is consulted. Anyone may copy a founding record and present its code, but not extend the key chain it names: the code names the tenant, and control of it is the key chain. Registered issuer codes are at most 25 characters, so none can equal a derived one ([07](07-actors-and-accounts.md) §1.1).

**The farm slug is the instance's own slug.** It is chosen at `instance create` (`--farm-slug`, required, with no default derived from a tenant), registered in the same namespace of slugs, provider slugs and issuer codes, and **never changes**. It differs from every tenant slug, the primary tenant's included. A single-tenant instance has one too. `instance` itself is reserved in the shared namespace, so no tenant or provider can take it. The farm slug is a name and is in no key.

**The farm code** is derived as a tenant's issuer code is, from the instance's founding record: its first `key:` record, at offset 0 of the instance `config` partition ([23](23-configuration-and-registry.md) §1.2; [0079](../decisions/0079-derived-issuer-codes.md) §4). Every instance has one, because the operator actor needs it whether or not a farm issuer exists. It is what the farm's keys carry: the operator's actor key `instance:{farm code}` (§8.2), the farm issuer's code (§3.5) and the per-issuer farm partition names `actors/{farm}`, `accounts/{farm}` and `log/{farm}` (§2.1), where `{farm}` is the farm code.

### 1.3 Adoption

*Sources: [0018](../decisions/0018-tenants.md) §1.*

A tenant may be the continuation of a MediaWiki Wikibase that ran before Triplespace. *Adoption* writes that wiki's entities into the tenant's `local` partition under the IDs it minted, its accounts under their own numbers, and sets every sequence past the source's; the `tenant` record gains `adopted_from`. It is neither mirroring (nothing is prefixed, no mirror partition exists) nor a move under §6 (there is no bundle to verify). If the tenant is a provider, §4 applies to adopted entities unchanged. The adoption procedure itself ([0035](../decisions/0035-adopting-a-wikibase.md)) is in [05](05-providers-and-ingest.md).

## 2. Partitions and configuration

*Sources: [0018](../decisions/0018-tenants.md) §2, §3; [0028](../decisions/0028-tenancy-policy.md) §2; [0046](../decisions/0046-primary-tenant.md) §2.*

### 2.1 Tenant partitions and instance partitions

*Sources: [0018](../decisions/0018-tenants.md) §2; [0028](../decisions/0028-tenancy-policy.md) §2; [0081](../decisions/0081-recovery-keys-and-continuations.md) §5.*

**Each tenant has its own source partitions**, and the instance holds the shared ones:

| Scope | Partitions | Graph IRI |
|---|---|---|
| Tenant | `local`, `pages`, `log`, `config`, `actors`, `accounts` | `{tenant base}/graph/{name}` |
| Instance | `config`, `log`, `mirror/{provider}`, `actors/{provider}`, `log/{provider}`, `files/{repo}` | `{farm base}/instance/graph/{name}` ([0046](../decisions/0046-primary-tenant.md) §7) |

A tenant also has one `source/{name}` partition per entity source it declares ([0078](../decisions/0078-entity-sources.md) §4, in [05](05-providers-and-ingest.md)), which moves with it (§6.2); there is no fixed count of partitions per tenant. With farm identity (§3.5) the instance also holds the three **farm partitions**, under the per-issuer names [0015](../decisions/0015-record-format-and-partition-registry.md) §5 already reserves: `actors/{farm}` (`logged`, `full`, internal), `accounts/{farm}` (`logged`, `full`, private) and `log/{farm}` (`logged`, `full`, internal). Their graph IRIs are under `{farm base}/instance/graph/` ([0046](../decisions/0046-primary-tenant.md) §7).

The graph registry is keyed by (tenant, name), with the instance's own partitions under a null tenant; in the `view` tables the instance is the empty string ([03](03-storage-caches-and-search.md) §4.1). The names of [0015](../decisions/0015-record-format-and-partition-registry.md) §5 are unchanged; what a name denotes now depends on the tenant. Checkpoint origin lines ([0006](../decisions/0006-log-integrity-and-erasure.md) §6) use the tenant's host as it was when the partition was created or the tenant arrived on the instance, and an `alias` (§6.1) does not change them, so Librarybase's local log signs as `librarybase.org/log/local` ([0081](../decisions/0081-recovery-keys-and-continuations.md) §5); an instance partition's is `{farm host}/instance/log/{name}`, so that the instance `config` and `log` cannot be confused with a tenant's when the farm base is a tenant's base ([0046](../decisions/0046-primary-tenant.md) §7).

**The instance's own `log` and files.** The instance has its own `log` partition (internal) for operator records such as file takedowns, and `files/{repo}` mirror partitions for file repositories. File bytes live outside the log in a blob store whose **scope** is one per tenant by default (`files.separation = tenant`) or one for the instance ([0039](../decisions/0039-files-and-media.md) §4, §10, in [12](12-files-and-media.md); the blob store is in [03](03-storage-caches-and-search.md) §8).

**Scale.** A farm with thousands of tenants has several list partitions per tenant in `log.record`. Postgres tolerates that; whether it performs well is a measurement the farm profile of [0013](../decisions/0013-postgres-storage.md) §11 has to add before it is relied on.

### 2.2 Partition IDs are 64-bit and random

*Sources: [0018](../decisions/0018-tenants.md) §2.*

The header's partition field was "a number assigned when the partition is created", stored as a `smallint`. It is inside the hash, so a partition cannot be renumbered, and two instances would assign the same small numbers. A partition's ID is 64 random bits drawn when it is created and never changes. The registry maps it to (tenant, name); `log.partition.partition` and `log.record.partition` are `bigint`; six more bytes per header. This is what makes a tenant's partitions importable by another instance without rehashing (§6). The instance `config` partition has a random ID like every other and is found by its registry name; offset 0 of it holds the instance's first `key:` record.

### 2.3 Per-tenant sequences

*Sources: [0018](../decisions/0018-tenants.md) §2.*

The revision, log and page IDs of [0015](../decisions/0015-record-format-and-partition-registry.md) §2 are per tenant, because MediaWiki clients expect one sequence per wiki. The provider-ranged revision ID then works across tenants without a new rule: Librarybase's revision 900 is `900` at home and `LB_number << 40 | 900` when another tenant reads it (§4).

### 2.4 The tenant's `config` partition carries a key chain

*Sources: [0018](../decisions/0018-tenants.md) §2, §3; [0079](../decisions/0079-derived-issuer-codes.md) §1.*

The instance key is registered in the instance `config` partition (§2.2), which does not travel with a tenant. So every `key:` record of the instance, at registration and at every rotation, is also appended to each tenant's `config` as a `key:` record. A tenant's partitions then verify from the tenant's bundle alone. A tenant's `config` partition begins, as the instance's does, with a `key:` record: the current instance key, copied in as the first entry of its key chain. These copies are written instance acts, carrying the instance attestation with the instance `config` record as authority (§8.6). The first of them, at offset 0, is the tenant's **founding record**: its leaf names the tenant, since the tenant's issuer code is derived from it (§1.2).

### 2.5 Configuration by scope

*Sources: [0018](../decisions/0018-tenants.md) §3; [0046](../decisions/0046-primary-tenant.md) §2.*

The config kinds of [0015](../decisions/0015-record-format-and-partition-registry.md) §3 split by scope:

| Scope | Kinds |
|---|---|
| Instance `config` | `key`, `graph`, `provider`, `issuer`, `keyed-type`, and three kinds of this ADR and [0046](../decisions/0046-primary-tenant.md) §2: `tenant` (slug, base URI, `adopted_from` and, if the tenant is a provider, its code and number), `alias` (§6.1) and `primary` (§7.2). Instance-wide policy lists such as `sitelink-policy:deny` ([0026](../decisions/0026-sitelinks.md) §3) and the other instance-scope kinds later ADRs add are listed in [0015](../decisions/0015-record-format-and-partition-registry.md) §3 |
| Tenant `config` | `namespace`, `role`, `reconcile`, `site`, `group`, graph ACLs ([0023](../decisions/0023-moderation.md) §3), the key chain of §2.4, `providers`: the list of providers whose graphs contribute to this tenant's resolved view and search ([0018](../decisions/0018-tenants.md) §6, in [03](03-storage-caches-and-search.md) §7), with Wikidata as the default, and the tenant-scope kinds later ADRs add, listed in [0015](../decisions/0015-record-format-and-partition-registry.md) §3 |

The `tenant` record carries no primary flag; the role is the `primary` record's (§7.2). Instance-level records are attested by actors of the primary tenant (§3.4).

`reconcile` is tenant configuration, with one exception: the identifier properties that drive tier-3 inference ([04](04-entities-and-identifiers.md) §4.3) are **instance** configuration, held in the instance `reconcile` record, so that tier-3 links stay instance-wide; a tenant's `reconcile` may switch inference off for itself and set provider order and link properties, but not add inference properties.

A tenant's configuration records project to `view.registry`; the tenant's `read` ACL records on a namespace, set or tenant target project to `view.tenant`, which holds nothing but the tenant's visibility epoch ([03](03-storage-caches-and-search.md) §4.6, §9.4).

## 3. Identity per tenant, and farm identity

*Sources: [0018](../decisions/0018-tenants.md) §4; [0028](../decisions/0028-tenancy-policy.md) §2.*

### 3.1 Each tenant is an issuer

*Sources: [0018](../decisions/0018-tenants.md) §4; [0079](../decisions/0079-derived-issuer-codes.md) §1.*

Its issuer code is the code derived from its founding record (§1.2), its actor model is numeric, and its actor IRI template is `{tenant base}/user/{id}`. An edit on Librarybase is attested by `librarybase:42`; the same person editing example.wiki is `example:17`, a different actor. `local` in the earlier ADRs is read as "the current tenant's issuer" and is no longer an issuer code of its own.

**Examples write the slug.** A 26-character code is hard to read in prose, so this and the other chapters write a tenant's actor keys with its slug: `librarybase:42` stands for `{Librarybase's issuer code}:42`, as the user part is already an opaque number. The `{tenant}` issuer template of `issuers.toml` is instantiated with the code.

### 3.2 Authentication is uncoupled from identity

*Sources: [0018](../decisions/0018-tenants.md) §4.*

Authentication is uncoupled from identity, as OAuth is. Bindings ([0007](../decisions/0007-actor-identity.md) §3) map an identity provider's subject to a tenant account and live in the tenant's `accounts` partition. One Wikimedia account may be bound to `librarybase:42` and to `example:17`; that makes them the same person's accounts, not the same actor, and they are linked only by the opt-in act of [0007](../decisions/0007-actor-identity.md) §7. A farm that wants single sign-on registers itself as an identity provider (an issuer with `login = true`), which gives one login per tenant account and merges nothing.

### 3.3 Attribution keeps the account and the tenant it was made under

*Sources: [0018](../decisions/0018-tenants.md) §4.*

Attribution keeps the account and the tenant it was made under, as [0007](../decisions/0007-actor-identity.md) §7 requires. An edit made on Librarybase stays `librarybase:42` on every instance that ever holds it and under every domain Librarybase ever uses. An edit imported from another wiki ([0008](../decisions/0008-namespaces-and-document-pages.md) §9) is attributed to that wiki's issuer and numeric ID, or to an `imported` surrogate ([0007](../decisions/0007-actor-identity.md) §5), and never to an account on the importing tenant; no account is created by an import, and a contributions page for such an actor says whose user they are and that these are imported edits ([0018](../decisions/0018-tenants.md) §8, in [19](19-site-ui.md)). This is the provenance MediaWiki loses.

### 3.4 The farm has no user base by default

*Sources: [0018](../decisions/0018-tenants.md) §4; [0028](../decisions/0028-tenancy-policy.md) §2.*

Instance-level records are attested by actors of the primary tenant. Memberships and blocks stay in each tenant's `actors` partition as [0016](../decisions/0016-permissions-and-access-control.md) §3 has them, which is where a farm wants them: groups are per wiki. Under a tenancy policy with farm identity, the farm *is* an issuer with accounts, but those accounts edit nothing: they are the identity a person carries across the farm, publicly linked to the tenant accounts created from them, and what global groups and global blocks key on ([0028](../decisions/0028-tenancy-policy.md) §2–4; global groups and blocks are in [09](09-security-and-moderation.md)). Every record is still attested by a tenant actor, and instance-level records by the primary tenant's, so the attribution rule of §3.3 does not change.

Every record in a tenant's partitions is attested **either by one of the tenant's own actors, or by the instance** (§8).

### 3.5 The farm as an issuer

*Sources: [0028](../decisions/0028-tenancy-policy.md) §2; [0079](../decisions/0079-derived-issuer-codes.md) §4.*

With `identity.farm_issuer` other than `none` (§5.1), the instance registers an issuer whose code is the **farm code** (§1.2); actor model `numeric`, `login = true`. Its accounts are **farm accounts**: actors `{farm}:{id}` with names, kinds and statuses like any account ([0007](../decisions/0007-actor-identity.md) §4), held in the three instance partitions `actors/{farm}`, `accounts/{farm}` and `log/{farm}` (§2.1). Farm account IRIs are `{farm base}/instance/user/{id}` ([0046](../decisions/0046-primary-tenant.md) §7). A farm account authenticates through bindings to identity providers exactly as a tenant account does ([0007](../decisions/0007-actor-identity.md) §3); the farm has no passwords of its own. **A farm account edits nothing.** It is a person's identity across the farm, not an actor on any tenant's data, and the rule of §3.4 stands: every record on a tenant is attested by that tenant's actor, and instance-level records by the primary tenant's.

What this preserves: `librarybase:42` is still the actor of every edit made on Librarybase, on every instance that ever holds it (§3.3). What it adds: `librarybase:42` and `example:17` are publicly the accounts of `{farm}:123`, so a farm can act on the person rather than on each account.

### 3.6 Shared names and automatic linking

*Sources: [0028](../decisions/0028-tenancy-policy.md) §2.*

With `identity.shared_names = on`, the farm keeps one **name registry**: a name held by any farm account or any tenant account is reserved farm-wide, and a tenant account can be created only under the name of the farm account creating it. A person who signs up to the farm chooses a name once. On first visit to a tenant that admits them, the tenant creates `{tenant}:{n}` under that name, binds it to the farm subject in the tenant's `accounts` partition, and, with `identity.auto_link = on`, appends the public `link-account` record of [0007](../decisions/0007-actor-identity.md) §7 between the tenant account and the farm account. The consent [0007](../decisions/0007-actor-identity.md) §7 requires is given once, at farm signup, where the form says in plain words that accounts created on the farm's wikis will be publicly linked to the farm account. A person who declines has no farm account and, under the `community` preset, no account at all.

**Turning shared names on later** is refused while any two tenants hold the same name for accounts that are not linked to one farm account, or any tenant name collides with a farm name. `triplespace-cli tenancy check` lists the collisions; each is resolved by renaming or linking. This is why the identity switches need `owner` (§5.1): the check is the cost of a farm that grew before it decided to be one.

### 3.7 Renames and vanishing cascade

*Sources: [0028](../decisions/0028-tenancy-policy.md) §2.*

Renaming a farm account renames every linked tenant account in one batch of actor records, one per tenant, as [0008](../decisions/0008-namespaces-and-document-pages.md) §6 already batches page moves. Vanishing a farm account vanishes each linked account under [0007](../decisions/0007-actor-identity.md) §4's procedure and then the farm account. Subsidiaries ([0024](../decisions/0024-subsidiary-accounts.md)) belong to tenant accounts and are unaffected: the operator relation is per tenant, and there is still no cross-tenant bot (§10.3).

**Cascades are instance acts.** The actor records a farm-account rename or vanish writes into each linked tenant are instance prerogatives: attributed to the instance operator, signed by the instance key, citing the farm account's record in `actors/{farm}` as authority (§8.6). A tenant bureaucrat cannot rename a linked account away from its farm name while shared names are on.

## 4. A tenant as a provider; reading across tenants

*Sources: [0018](../decisions/0018-tenants.md) §5; [0028](../decisions/0028-tenancy-policy.md) §5; [0080](../decisions/0080-tenants-as-entity-sources.md) §2, §3, §5.*

### 4.1 A tenant's local graph is, to every other tenant, a foreign source graph

*Sources: [0018](../decisions/0018-tenants.md) §5; [0080](../decisions/0080-tenants-as-entity-sources.md) §2, §3.*

**A tenant is read by another tenant as an entity source** ([0080](../decisions/0080-tenants-as-entity-sources.md) §1–2; [0078](../decisions/0078-entity-sources.md), in [05](05-providers-and-ingest.md) §6). The reader declares an `entity-source` record whose `tenant` field is the provider's issuer code (§1.2), and names it, `lb` say. Nothing is needed on the provider's side, and no registry entry: any public tenant can be read. On the same instance there is no sync job and no partition, because the provider's `local` partition is the source.

**A registry code is promotion.** A tenant that many tenants read may be given a code, a slug and a provider number in `providers.toml` ([0015](../decisions/0015-record-format-and-partition-registry.md) §5) by a commit, as Librarybase has `LB`, `librarybase` and 2. Its entry's `issuer` is the tenant's issuer code, which binds the code to the tenant wherever it is hosted, and its IDs, `LBQ6`, then mean one thing on every instance ([0080](../decisions/0080-tenants-as-entity-sources.md) §3). The rules below hold for both, with the reader's source name where there is no code.

- **IDs are rewritten when read, not when written.** Librarybase stores `Q6`; example.wiki's projections read Librarybase's partition and rewrite `Q6` to `lb:Q6` (or `LBQ6` for a promoted provider), `P12` to `lb:P12`, exactly as an adapter rewrites `Q42` to `WDQ42` at ingest ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4). Revision and page IDs are ranged by the reader's number for the source, or the provider number. References to global entities, `WDQ42` or `domain:x`, pass through unchanged.
- **References to the provider's entity sources are rewritten by IRI** ([0078](../decisions/0078-entity-sources.md) §8). Librarybase's `mhc:Q1` becomes a registry provider's ID where the IRI is a provider's, or the reader's own source ID where the reader declares a source with the same IRI templates; otherwise every statement that uses it is withheld from the reader and counted on its `Special:Providers`. An unpublished source's references never cross.
- **`https://example.wiki/entity/LBQ6` is an alias** of `https://librarybase.org/entity/Q6`, which is canonical, as [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4 already says for foreign entities.
- **Reconciliation is unchanged.** A Librarybase assertion about `WDQ42` is, on example.wiki, a foreign assertion with an `LB` source chip ([0003](../decisions/0003-statement-ui.md) §6), and example.wiki's local graph wins over it ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3). Librarybase's `same-as` and `convert` records are tier-1 links for Librarybase and tier-2 links for everyone else ([04](04-entities-and-identifiers.md) §4.3), ranked where the reading tenant's provider order puts `LB`.
- **Opt-in.** A tenant reads a provider's graph only if it declares it as a source, or, for a promoted provider, lists it in its `providers` list (§2.5). A tenant that has not opted into Librarybase never sees it. Which tenants may name which, and whether a provider may restrict its readers, are the tenancy switches of §4.2.
- **Every public tenant can be referenced.** Its entities' IRIs are their global names; a tenant that reads it names them by its source name or the registry code ([0080](../decisions/0080-tenants-as-entity-sources.md) §7).
- **Leaving the instance changes nothing for referrers.** When a provider tenant moves away (§6), readers on the old instance switch from reading it directly to the Triplespace adapter (§4.4), and `lb:Q6` and `LBQ6` mean what they always meant; a deleted tenant is one that moved away with no destination (§6.6). A source names the provider by issuer code, and a registry code by its entry's `issuer`, so neither depends on where the provider is hosted.

A provider tenant's entities appear in a reading tenant's update stream under the reading tenant's rewriting (§10.5).

### 4.2 Who may read whom; reader lists

*Sources: [0028](../decisions/0028-tenancy-policy.md) §5; [0018](../decisions/0018-tenants.md) §5; [0080](../decisions/0080-tenants-as-entity-sources.md) §5.*

**Who may read whom.** `providers.between_tenants` governs entity sources that name a tenant of the same instance ([0080](../decisions/0080-tenants-as-entity-sources.md) §5). With `operator`, only the farm operator can write such a source, or set `files.share` and `pages.share`; the hosting case, where tenants have no business reading each other unless the operator says so. With `opt-in`, any tenant may name any public tenant of the instance, which is what §4.1 describes. A source naming a tenant on another instance is an ordinary entity source, which no tenancy switch governs.

**Reader lists.** With `providers.reader_lists = on`, a provider tenant may restrict who reads it with a `config` record of kind `provider-readers` in its own `config`: `mode` (`allow` or `deny`) and a list of tenants by **issuer code**, never by slug, since the record travels with the provider and a slug may name another tenant after a move (§1.2), in the shape of [0026](../decisions/0026-sitelinks.md) §3's sitelink lists. A tenant not admitted sees the provider as if it had no code. It is a list, not a new ACL kind, evaluated where the reading tenant's projections open the provider's partition. Reading is anonymous: a reading tenant is the `universe` principal of the provider and sees its public form, so a provider may keep confidential entities, which are simply not part of what it provides, and a private tenant provides nothing ([0056](../decisions/0056-security-model.md) §6, in [09](09-security-and-moderation.md)). Reader lists decide which tenants may read; visibility decides what they read.

### 4.3 Private tenants

*Sources: [0018](../decisions/0018-tenants.md) §5; [0080](../decisions/0080-tenants-as-entity-sources.md) §5.*

A **private tenant**, one whose `tenant` ACL restricts `read` ([0056](../decisions/0056-security-model.md) §3, in [09](09-security-and-moderation.md)), cannot take a code or be read as a source (a source naming one is refused with `ts-source-private`): its partitions are exported as `private` for every purpose but the operator's own backups and a move (§6.2), and nothing of it is read by another tenant. It has no graphs in any query store (§9.1). Whether a tenant may be private at all is the `security.restrictions` switch (§5.2).

### 4.4 Across instances: the Triplespace adapter

*Sources: [0028](../decisions/0028-tenancy-policy.md) §5; [0080](../decisions/0080-tenants-as-entity-sources.md) §2.*

A provider tenant on another instance, or one that has moved away (§6), is read by a **Triplespace adapter**, `scatter-adapter-triplespace`, in the ingest layer ([0005](../decisions/0005-crate-organization.md) §2, in [22](22-crates-and-stack.md)). It bootstraps from the provider's local-graph source dump ([0022](../decisions/0022-federation.md) §1) and then follows the provider's activity stream ([0020](../decisions/0020-change-feeds.md) §4), filtered to `source = local`, with `Last-Event-ID` as its version cursor, rewriting references to the reader's own entities back to bare IDs and verifying each batch against the provider's checkpoint and key chain unless the provider is registered `trust = stream` ([0022](../decisions/0022-federation.md) §2), writing `put`, `redirect` and `tombstone` records into the reader's `source/{name}` partition for a tenant source, or a `mirror/{provider}` partition for a promoted provider, as any adapter does. For a tenant source it first checks that the founding record at offset 0 of the provider's key chain derives the source's `tenant` code (§1.2), which binds the code to the chain it verifies against. Same-instance reading is direct (§4.1), cross-instance reading is a sync over the stream, and a tenant that leaves the instance is switched from the first to the second with no change in what its readers see.

## 5. The tenancy policy and its presets

*Sources: [0028](../decisions/0028-tenancy-policy.md) §1, §8; [0056](../decisions/0056-security-model.md) §11; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §9.*

### 5.1 The policy is instance configuration, with three presets

*Sources: [0028](../decisions/0028-tenancy-policy.md) §1; [0056](../decisions/0056-security-model.md) §11.*

The **tenancy policy** is a set of switches, each a `config` record of kind `tenancy` in the instance `config` partition ([0015](../decisions/0015-record-format-and-partition-registry.md) §3), keyed `tenancy:{switch}`. Three **presets** ship in `docs/registry/tenancy.toml` and are what `triplespace-cli instance create --tenancy {preset}` writes; every switch can then be changed on its own with `ts-config` at the farm base, except the identity switches, which need `owner` because they are hard to reverse (§3.6). A single-tenant instance is the `isolated` preset and never notices any of this.

| Switch | Values | `isolated` | `community` | `enterprise` |
|---|---|---|---|---|
| `identity.farm_issuer` | `none`, `optional`, `required` | `none` | `required` | `optional` |
| `identity.required_issuer` | an issuer code, or none: every tenant account must bind to it | none | none | the corporate identity provider |
| `identity.shared_names` | `off`, `on` | `off` | `on` | `on` |
| `identity.auto_link` | `off`, `on` | `off` | `on` | `on` |
| `groups.global` | `none`, `inherited` | `none` | `inherited` | `inherited` |
| `blocks.global` | `ip`, `farm-actor` | `ip` | `farm-actor` | `farm-actor` |
| `providers.between_tenants` | `operator`, `opt-in` | `operator` | `opt-in` | `opt-in` |
| `providers.reader_lists` | `off`, `on` | `off` | `on` | `on` |
| `discussion.cross_tenant` | `off`, `read-only` | `off` | `read-only` | `read-only` |
| `notifications.cross_tenant` | `off`, `home` | `off` | `home` | `home` |
| `config.template` | `none`, `defaults`, `locks` | `none` | `defaults` | `locks` |
| `feeds.farm_wide` | `off`, `global-groups`, `everyone` | `off` | `global-groups` | `everyone` |
| `search.farm_wide` | `off`, `global-groups`, `everyone` | `off` | `off` | `everyone` |
| `filters.global` | `none`, `inherited` ([0030](../decisions/0030-edit-filters.md) §8) | `none` | `inherited` | `inherited` |
| `wikitext.ceiling` | `subset`, `expansion`, `lua`: how much of template expansion and Lua a tenant may turn on ([0042](../decisions/0042-template-expansion-and-parsoid.md) §2) | `lua` | `lua` | `lua` |
| `security.restrictions` | `none`: tenants are public and `read` is restricted by moderation only; `tenant`: a tenant may be private as a whole, and nothing finer; `any`: tenants may also restrict pages, entities, namespaces and sets ([0056](../decisions/0056-security-model.md) §11) | `any` | `none` | `any` |
| `query.isolation` | `dataset`, `store` (§9.1) | `store` | `dataset` | `dataset` |

Every switch that says "farm actor" or "global" requires `identity.farm_issuer` to be at least `optional`; the registry rejects a preset or a record that does not satisfy that. `meta=siteinfo&siprop=triplespace` reports the preset and the switches, so a client and the UI know what exists.

The identity switches are §3.5–3.7, the provider switches §4.2, `config.template` §5.3, `security.restrictions` §5.2, the discussion and notification switches §10.1–10.2; `groups.global`, `blocks.global` and `filters.global` are in [09](09-security-and-moderation.md), `feeds.farm_wide` and `search.farm_wide` ([0028](../decisions/0028-tenancy-policy.md) §9) in [16](16-logs-feeds-and-notifications.md) and [03](03-storage-caches-and-search.md) §11.4, `wikitext.ceiling` in [11](11-rendering-templates-and-modules.md).

### 5.2 `security.restrictions`, and global groups in ACLs

*Sources: [0056](../decisions/0056-security-model.md) §11.*

`isolated` is the hosting preset and the single-tenant default: whoever operates the instance decides. `community` follows the Wikimedia practice that a public wiki has no private pages; a community farm that wants them sets `tenant` or `any`. The switch is reported in `siprop=triplespace` beside the others, and changing it from `any` to `none` is refused while any confidential restriction exists; `tenancy check` lists them.

**Global groups in ACLs.** With `groups.global = inherited`, an ACL may name a global group. Group names are one namespace on a farm: a tenant cannot create a group with a global group's name, and a global group cannot take a name any tenant uses, which `tenancy check` verifies as it does account names. A hosting instance has no global groups, so there is no group that reads every tenant; a community farm's `steward` reads a private tenant only where that tenant's ACL names it, and an enterprise's `platform-admin` likewise. The read-everything role, where an organisation wants one, is a decision each tenant's ACL makes, or a locked template makes for it (§5.3).

### 5.3 Shared machinery: templates and locks

*Sources: [0028](../decisions/0028-tenancy-policy.md) §8; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §9.*

With `config.template = defaults`, the instance `config` may hold records of kind `template`, keyed `template:{kind}:{code}`, each holding a tenant-config record (namespaces, groups, `sitelink-policy`, `thread-status`, `site` settings, `view-pin`, and any other tenant kind of [0015](../decisions/0015-record-format-and-partition-registry.md) §3) that `tenant create` writes into a new tenant's `config`. A tenant may then change them. With `locks`, the instance also holds `tenancy:locked`, a list of `{kind}:{code}` keys; a tenant `config` record for a locked key is refused with `ts-locked`, and changing a locked template re-writes it into every tenant. The instance sitelink deny list ([0026](../decisions/0026-sitelinks.md) §3) is the one such rule every preset has; templates and locks generalise it to any setting an enterprise wants uniform.

**Template records are instance acts** (§8.1, §8.6): under `defaults` they are **provisions**, which the tenant may replace; under `locks` they are **prerogatives**, and `ts-locked` is a case of `ts-prerogative`.

An instance-scope `page-repo` ([0052](../decisions/0052-page-repositories-and-title-inheritance.md), in [13](13-mirrored-pages.md)) is shared machinery under `config.template`: provisioned under `defaults`, locked under `locks`, as file repositories are. The rest of [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §9, the permissions, is in [09](09-security-and-moderation.md).

### 5.4 Operator rules every preset has

*Sources: [0028](../decisions/0028-tenancy-policy.md) §8.*

File takedowns and expunges are a second operator rule every preset has, beside the sitelink deny list: they apply to every tenant, no tenant can override them, and they are made through the primary tenant or a global group ([0039](../decisions/0039-files-and-media.md) §10, in [12](12-files-and-media.md)).

## 6. Aliases, moving, leaving, joining and deleting a tenant

*Sources: [0018](../decisions/0018-tenants.md) §9, §10; [0028](../decisions/0028-tenancy-policy.md) §5, §10; [0040](../decisions/0040-instance-prerogatives.md) §8; [0024](../decisions/0024-subsidiary-accounts.md) §7.*

### 6.1 Aliases

*Sources: [0018](../decisions/0018-tenants.md) §9; [0079](../decisions/0079-derived-issuer-codes.md) §3.*

When a tenant, provider or issuer changes its base URI, the change is an `alias` record in the instance `config`, keyed by the old host and carrying the new base and the date. Its effects:

- Projections emit every IRI from the current template; nothing is rewritten in the log.
- The metadata graph carries one triple per alias, `<{old base}/> dcterms:isReplacedBy <{new base}/>`, using the term [0001](../decisions/0001-revision-metadata-rdf.md) §6 reserved. Consumers holding old dumps rewrite prefixes; nothing is emitted per actor or per entity.
- The old host serves redirects for as long as anyone controls it.

So `domains.wikibase.cloud` becoming `internetdomains.wiki` is one record and one triple, every entity IRI ever emitted stays resolvable, and no actor key changes, since actor keys carry the issuer code, not the host or the slug.

### 6.2 Moving a tenant: what moves

*Sources: [0018](../decisions/0018-tenants.md) §10; [0040](../decisions/0040-instance-prerogatives.md) §8; [0024](../decisions/0024-subsidiary-accounts.md) §7.*

The tenant's six partitions (§2.1), and the mirror partition of each of its entity sources, `source/{name}` ([0078](../decisions/0078-entity-sources.md) §4), as an export bundle ([01](01-log-and-records.md) §8), except `accounts`, which is private and never exported ([0007](../decisions/0007-actor-identity.md) §8). The bundle carries the tenant's IDs (in headers), its surrogates ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §7), its actors, memberships and blocks, its configuration and its key chain. Three extracts and a digest go with it:

- the **retention extract**, every record in the instance's `mirror/*` and `log/{provider}` partitions keyed to an entity the tenant has set `retain` on, including backfilled upstream revisions, which [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5 warns may be unrecoverable later;
- the **binding digest**, an HMAC of each (issuer, subject) in the tenant's `accounts`, keyed with a secret passed operator to operator, so the receiving instance learns nothing until a subject logs in;
- on a cooperative move, the **private extract**, every account's user data bundle sealed to a key the receiving instance supplies and applied only when the account is reclaimed ([0027](../decisions/0027-preferences-and-portability.md) §4, in [07](07-actors-and-accounts.md));
- and the **authority extract**, every authority record the bundle's instance attestations cite, with an inclusion proof against a signed checkpoint of the authority's partition. Each is carried with its attestation part withheld (`[null, leaf]`, as an erased part is stored under [0015](../decisions/0015-record-format-and-partition-registry.md) §1), so that the operator's identity does not leave the instance while the record still verifies; content and comment are withheld too where the act's kind makes them confidential (a `confidential` takedown, an expunge's hash, [0039](../decisions/0039-files-and-media.md) §10). This is the same arrangement as the retention extract: what the tenant's history depends on travels with it.

Prerogatives of the old instance stop binding once the tenant is registered elsewhere; they stay in history, verifiable through the old key (§8.7). Subsidiary API keys do not travel, since nothing private does; the operator relation travels in the actor records, and the operator reissues keys after reclaiming their own account (§10.3). Views, indexes and caches are rebuilt on arrival.

### 6.3 Procedure

*Sources: [0018](../decisions/0018-tenants.md) §10; [0079](../decisions/0079-derived-issuer-codes.md) §3, §5; [0080](../decisions/0080-tenants-as-entity-sources.md) §2; [0081](../decisions/0081-recovery-keys-and-continuations.md) §3.*

**Cooperatively.** The old instance freezes the tenant read-only and writes a final checkpoint. The new instance supplies its public key, and the old instance appends a `key:` rotation record to the tenant's key chain naming the new key and the final checkpoint, signed by the old key as [0006](../decisions/0006-log-integrity-and-erasure.md) §6 rotation is. The new instance runs `verify` on the bundle, registers the six partitions under their existing IDs, replays projections and rebuilds search, re-appends the retention extract as a job with `pav:retrievedFrom` naming the old instance, and maps the tenant's `providers` list onto its own mirrors, which is trivial because provider slugs come from the same registry. `verify` recomputes the tenant's issuer code from the founding record in the bundle and checks that the actor keys of the tenant's own actors use it (§1.2). The tenant is registered under that code and a slug of the new instance's choosing, its old one if free; nothing in the bundle changes when it is not. DNS moves, or an alias record is written (§6.1). If the tenant is read by other tenants, its readers on the old instance switch from reading it directly to the adapter (§4.4), and readers on the instance it joins switch the other way, freezing their `source/{name}` partitions; what they show does not change ([0080](../decisions/0080-tenants-as-entity-sources.md) §2).

**Non-cooperatively**, when the old instance has gone or refuses, the bundle comes from backups or from the old instance's public exports, and the new instance appends a **continuation** instead of a rotation (§6.8): `recovered` if the tenant's recovery keys sign it, `unauthorized` if not. Reclaiming is manual.

**The primary tenant cannot leave.** It cannot be frozen for a move, moved or deleted until the primary role has been transferred to another tenant (`ts-primary-tenant`, §7.6).

### 6.4 Account reclaiming

*Sources: [0018](../decisions/0018-tenants.md) §10.*

Bindings do not travel, so a user proves on the new instance that they are `librarybase:42` in one of three ways: by logging in with the same identity provider, which the new instance checks against the binding digest; by the old instance vouching, acting as an identity provider for a grace period; or by a bureaucrat recording a manual reclaim as a `reclaim/reclaim` log event, which is MediaWiki's practice of confirming identity on the home wiki. An account never reclaimed keeps its name and its history and cannot log in.

### 6.5 Leaving and joining a farm

*Sources: [0028](../decisions/0028-tenancy-policy.md) §10; [0040](../decisions/0040-instance-prerogatives.md) §8.*

When a tenant **leaves** a farm with farm identity, its accounts keep their names and their public links, which now point at farm accounts on another instance: the farm account IRI `{farm base}/instance/user/{id}` ([0046](../decisions/0046-primary-tenant.md) §7) still resolves, and the link is what [0007](../decisions/0007-actor-identity.md) §7 calls a foreign account link. Global groups and global blocks stop applying the moment the tenant is registered elsewhere, as the old instance's prerogatives stop binding (§8.7). Bindings do not travel and reclaiming proceeds as in §6.4; the farm may act as the vouching identity provider, which is the natural case.

When a tenant **joins** a farm with shared names, its accounts are checked against the farm's name registry (§3.6). Colliding names are renamed by the joining tenant's bureaucrats, or the person proves both are theirs and they are linked. Only then is the tenant's identity switched on.

### 6.6 Deleting a tenant

*Sources: [0028](../decisions/0028-tenancy-policy.md) §5; [0018](../decisions/0018-tenants.md) §5.*

Deleting a tenant is the same as moving one away without a destination: its partitions are retained read-only for a grace period set in `site` configuration, its providers switch nothing (there is no new home), and its former referrers see its entities as they would see an upstream that has gone silent: the last mirrored state, with the identity line saying the provider is gone. After the grace period the partitions are erased, an instance prerogative carried out by a job whose record in the instance `log` is its authority (§8.6): `erase` records are written over every partition, and the partitions are then dropped, so that the tenant's packed-storage domain and its child table go with them (§9.2). The primary tenant cannot be deleted until the primary role has been transferred (`ts-primary-tenant`, §7.6).

### 6.7 Rehosting an instance

*Sources: [0018](../decisions/0018-tenants.md) §10.*

Rehosting an instance whole is a Postgres restore plus the rejects files and the `private` schema. No ID, key or attestation changes, and a change of farm base is an alias (§6.1). On a single-tenant instance, moving the only tenant is rehosting the instance, and deleting it is decommissioning the instance (§7.6).

### 6.8 Recovery keys and continuations

*Sources: [0081](../decisions/0081-recovery-keys-and-continuations.md) §1, §3, §4.*

**A tenant's community may hold recovery keys, which no host ever sees.** A `config` record of kind `recovery-key` in the tenant's `config` holds one or more Ed25519 public keys, a `threshold` `k` (default 1) and a `delay` (default 72 hours, at least 24), with the **recovery signatures** that authorize it ([01](01-log-and-records.md) §3.3, tag `0x07`). Registering the first set is an act of `owner` and carries a signature by every listed key. Replacing the set, including its threshold or delay, carries signatures by `k` keys of the current set and by every new key. Removing it carries `k` signatures of the current set. A record without them is refused with `ts-recovery-signature`, and `verify` ignores one. A host can append records under any account but cannot forge these, so once a set is registered no host can change it. Keys are generated off the instance by `triplespace-cli recovery-key generate`.

**A move the old instance does not sign is a continuation.** The new instance B appends a `config` record of kind `continuation`, code `continuation:{n}`, holding:

- `from`: for each partition, the old instance A's last signed checkpoint, verbatim with any witness cosignatures;
- `tail`: for each partition, the offset range after that checkpoint, which only B attests;
- `key`: B's instance key, now the current key of the tenant's chain;
- `origin_host`: B's host for the new origin lines (§2.1);
- `mode`: `recovered` if its recovery signatures meet the threshold of the set in force at `from`, otherwise `unauthorized`;
- `signatures`: any recovery signatures.

B's instance attestation signs it ([0040](../decisions/0040-instance-prerogatives.md) §3), with B's import job as authority. That one signature per partition, over A's own roots, is the batch re-signing; no record is re-signed and A's checkpoint signatures stay verifiable. The tail verifies structurally against A's tree but is attested by B alone. B must continue from the latest A checkpoint it can obtain, from the bundle, from A's still-served checkpoints or from a witness. Continuing from an older one is a fork, which `verify` and readers report as such. `triplespace-cli tenant import --continue` writes the draft, which names B's key and host. Holders sign it on their own machines with `triplespace-cli recovery sign`, and the import appends it once the threshold is met. A cooperative move may also carry recovery signatures but does not need them.

**A recovered continuation waits.** B serves the tenant at once. But readers on other instances act on a `recovered` continuation only after the set's `delay` has passed since its first witnessed checkpoint, or since its own time if the tenant has no witnesses ([01](01-log-and-records.md) §4.3). Within the delay, a `continuation-cancel:{n}` record signed by `k` keys of the authorizing set voids it, whichever instance appends it. The old host's key can neither cancel nor contest it: leaving a host without its consent is what recovery keys are for.

## 7. The primary tenant

*Sources: [0046](../decisions/0046-primary-tenant.md) §1–6; [0018](../decisions/0018-tenants.md) §1, §10.*

### 7.1 The primary tenant is where the instance's operators have their accounts

*Sources: [0046](../decisions/0046-primary-tenant.md) §1; [0018](../decisions/0018-tenants.md) §1.*

**The primary tenant is the tenant whose accounts operate the instance.** An instance has exactly one at every point in its log (§7.2). The role is about identity: it says whose accounts may exercise instance rights, and whose actors may attest records the instance holds on its own behalf. It is **not a storage location** for instance records (§7.4), and it gives **no authority over other tenants' data** beyond what instance rights already give, exercised through instance acts (§8).

What the role carries:

| | |
|---|---|
| **Instance rights** | Evaluated on the primary tenant, or through a global group at the farm base ([0028](../decisions/0028-tenancy-policy.md) §3); held on any other tenant they grant nothing at instance scope ([0046](../decisions/0046-primary-tenant.md) §8, [0040](../decisions/0040-instance-prerogatives.md) §9, in [09](09-security-and-moderation.md)) |
| **Attestation of instance records** | Records in instance partitions that are not an upstream's or a farm account's are attested by actors of the primary tenant (§7.4), including the authority records of instance acts (§8.4) |
| **The instance's owners** | `owner` on the primary tenant is the instance's `owner`: the group that holds `ts-keys` and `ts-primary`. `owner` on any other tenant is that wiki's owner, whose `all_permissions` include instance rights that grant it nothing there |
| **The instance's bots** | Instance jobs (mirror syncs, provider backfills) run as subsidiaries of primary-tenant accounts ([0024](../decisions/0024-subsidiary-accounts.md) §1; §7.5) |
| **Bootstrap** | `triplespace-cli instance create` creates the instance key, the primary tenant, its `owner` and its sync subsidiaries ([0016](../decisions/0016-permissions-and-access-control.md) §3) |
| **Defaults** | The operator actor's display name defaults to the primary tenant's site name (§8.2); on a single-tenant instance the farm base is the primary tenant's base (§1.1) |

What it does not carry: the primary tenant's records are not instance records; its blocks, ACLs and groups apply to its own accounts and content only; it is a guest under §8.1, so an instance act on it is attested by the instance like any other; farm accounts, global groups and global blocks live in the farm partitions, not in it ([0028](../decisions/0028-tenancy-policy.md) §2–4); and it needs no provider code.

**"An actor of the primary tenant" means primary when the record was appended.** Evaluating an instance right, and verifying the attestation of an authority record (§8.4, §8.7), look up which tenant held the role at that record's offset in the instance `config` (§7.2). A record attested by an operator of a former primary tenant stays valid.

**On the primary tenant, its groups are the instance's groups for instance rights.** Whoever holds an instance right there exercises it across the farm: a primary-tenant `bureaucrat` approves OAuth consumers, as Meta's OAuth administrators do for Wikimedia. A farm whose primary tenant is a community wiki either confines instance rights to `owner` there, by its own group records, or uses an operations tenant (§7.3).

### 7.2 The `primary` record

*Sources: [0046](../decisions/0046-primary-tenant.md) §2.*

The role is held by one `config` record of an instance-scope kind, **`primary`**, keyed `primary`, whose content is the tenant's slug and, for a transfer, the offer it accepts (§7.5). The `tenant` record keeps slug, base URI and, for a provider tenant, its code and number, and carries no primary flag (§2.5).

`instance create` appends the first `primary` record after the instance `key:` and the primary tenant's `tenant:` record. The instance `config` partition is `logged` and `full`, so its history answers "which tenant was primary at offset *n*" without a separate table, which §7.1 relies on.

### 7.3 Any tenant may be primary

*Sources: [0046](../decisions/0046-primary-tenant.md) §3.*

The primary tenant may be a wiki with content, as Librarybase is on its own instance, or an **operations tenant** created only to hold the operators' accounts and the instance's bots, with no pages or entities. An operations tenant may be private (its partitions with the `private` export policy, §4.3), since nothing about the role needs to be public; the public face of instance acts is the operator actor and `Special:InstanceAction` at the farm base ([0040](../decisions/0040-instance-prerogatives.md) §7, in [21](21-special-pages.md)).

This is a choice each instance makes, not a tenancy switch. By archetype (§5.1): an `isolated` hosting farm will usually want an operations tenant, so that no customer's wiki carries its operators; a `community` farm will usually make its Meta-style wiki primary; an `enterprise` farm, its platform team's wiki.

### 7.4 Instance records live in instance partitions

*Sources: [0046](../decisions/0046-primary-tenant.md) §4.*

Every record in an instance partition is attested by an upstream actor (for what a sync job writes into `mirror/*`, `actors/{provider}`, `log/{provider}` and `files/{repo}`, the job's ID in the attestation naming the primary-tenant subsidiary that ran it), by a farm account (for its own records in `actors/{farm}`, `accounts/{farm}` and `log/{farm}`, and for what members of global groups write), or by an actor of the primary tenant, for everything else; the full rule, and the mirror sync job records and OAuth consumer events it sends to the instance `log`, are in [01](01-log-and-records.md) §1.4. It is the instance counterpart of §8.1's rule for a tenant's partitions. Nothing the instance holds on its own behalf is written into the primary tenant's partitions, so a transfer (§7.5) moves no records and splits no history. The instance `log` stays `internal`; its public records (job records, consumer events, the public reasons of authority records) are served at the farm base by the same projections as a tenant's log.

### 7.5 Transferring the role

*Sources: [0046](../decisions/0046-primary-tenant.md) §5; [0079](../decisions/0079-derived-issuer-codes.md) §4.*

**By offer and acceptance.** A member of `owner` on the primary tenant offers the role to another tenant on the instance with `ts-primary`: a `primary/offer` record in the instance `log` naming the tenant, with an expiry (default seven days). A member of `owner` **on the offered tenant** accepts by appending the new `primary` record, citing the offer. Acceptance is the receiving tenant's consent to its groups becoming the instance's (§7.1), and is the one write to an instance partition that an actor of a non-primary tenant may attest; it is refused without an open offer naming that tenant. The offerer may withdraw an open offer (`primary/withdraw`). The new `primary` record projects as `primary/transfer` in the instance `log`.

**Effects, from the offset of the new `primary` record:**

- Instance rights are evaluated on the new primary tenant. `owner` on the former primary is that wiki's owner and nothing more.
- Authority records and instance acts made before stay valid (§7.1); nothing is re-attested.
- Instance jobs already running finish under the subsidiaries that started them; new ones need subsidiaries of the new primary's accounts. `triplespace-cli primary accept` creates them beside the accepting account, as `instance create` does ([0024](../decisions/0024-subsidiary-accounts.md) §1), and the former primary's sync subsidiaries are left for that tenant's bureaucrats to retire.
- The operator actor keeps its key `instance:{farm code}` and its IRI. Its display name follows the new primary's site name unless the instance has set its own name.
- Farm accounts, global groups and global blocks are untouched; they never belonged to the primary tenant.

A transfer is not an instance act on either tenant: it writes nothing into either tenant's partitions.

### 7.6 The primary tenant cannot leave

*Sources: [0046](../decisions/0046-primary-tenant.md) §5; [0018](../decisions/0018-tenants.md) §10.*

Moving the primary tenant away, deleting it, or freezing it for a move is refused with `ts-primary-tenant` until the role has been transferred. On a single-tenant instance there is no tenant to transfer to: moving the only tenant is rehosting the instance (§6.7), and deleting it is decommissioning the instance.

## 8. Instance prerogatives

*Sources: [0040](../decisions/0040-instance-prerogatives.md) §1–6, §8; [0018](../decisions/0018-tenants.md) §4; [0028](../decisions/0028-tenancy-policy.md) §2, §8.*

### 8.1 Instance acts, prerogatives and provisions

*Sources: [0040](../decisions/0040-instance-prerogatives.md) §1; [0018](../decisions/0018-tenants.md) §4.*

An **instance act** is anything the instance does to a tenant's data on its own authority rather than the tenant's. It takes one of two forms, and the first is preferred:

- **Evaluated.** An instance rule is applied when a tenant's data is read or written, and nothing is written to the tenant. Global groups and blocks ([0028](../decisions/0028-tenancy-policy.md) §3–4), IP blocks, the instance sitelink deny list ([0026](../decisions/0026-sitelinks.md) §3), global filters' refusals ([0030](../decisions/0030-edit-filters.md) §8) and file takedowns ([0039](../decisions/0039-files-and-media.md) §10) are evaluated.
- **Written.** A record is appended to the tenant's own partition, because the tenant's log itself has to change: something must be erased from it, or a record the tenant's projections read must exist in it. A written act is used only when an evaluated one cannot do the job.

An instance act is a **prerogative** when it is **binding**: the tenant cannot refuse, undo or lower it (§8.5). It is a **provision** when it is not: the instance writes a starting value, and the tenant may change it like any of its own records. Templates written into a new tenant under `config.template = defaults` (§5.3) are provisions; locked templates are prerogatives.

**The rule.** Every record in a tenant's partitions is attested either by one of that tenant's actors, or by the instance under §8.3. Nothing else. The tenant whose partition it is, and every tenant on the instance including the primary one, is a **guest** of the instance for this purpose: a prerogative on the primary tenant is attested by the instance like any other.

### 8.2 The instance as an actor

*Sources: [0040](../decisions/0040-instance-prerogatives.md) §2; [0079](../decisions/0079-derived-issuer-codes.md) §4.*

The instance is an actor of a reserved issuer, **`instance`**, with actor model `provider-only` ([0007](../decisions/0007-actor-identity.md) §1, §6): changes are attributed to the instance as a whole, as OpenAlex's are to OpenAlex. Its actor key is `instance:{farm code}` (§1.2); its IRI is `{farm base}/instance/operator` ([0046](../decisions/0046-primary-tenant.md) §7); and its display name is the message "{instance name} operator" (by default the site name of the primary tenant).

**The person who acted is not the actor.** Every instance act has an **authority record** (§8.4) attested by the operator who carried it out, an actor of the primary tenant. That attestation is visible only to `ts-viewoperator` ([0040](../decisions/0040-instance-prerogatives.md) §9, in [09](09-security-and-moderation.md)). A guest's history, logs, RDF, feeds and notifications show the instance operator, as Wikimedia shows office actions under a role account. The reason is the one James gave: these are exceptional acts outside editorial scope, and the people who carry them out, often in response to legal notices, should not become the target of the guest community's disagreement with them.

**The operator actor edits nothing editorial.** It cannot be logged into, holds no group memberships on any tenant, and appears only on records written under §8.3.

### 8.3 The instance attestation

*Sources: [0040](../decisions/0040-instance-prerogatives.md) §3; [0079](../decisions/0079-derived-issuer-codes.md) §4.*

A written instance act carries, in the attestation part of the record appended to the guest ([01](01-log-and-records.md) §2.4), an **instance attestation**:

| Field | Holds |
|---|---|
| `actor` | `instance:{farm code}` |
| `authority` | The authority record's partition, offset and leaf hash ([0006](../decisions/0006-log-integrity-and-erasure.md) §5) |
| `job` | The job that wrote it, where one did (§8.4) |
| `binding` | `true` for a prerogative, `false` for a provision |
| `signature` | `{key: <instance key ID>, alg: "ed25519", sig: <64 bytes>}`, by the instance key current when the record is appended |

**The signature** is over `H(0x06 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment) ‖ authority)`, where `authority` is the canonical CBOR of the `authority` field. Tag `0x06` joins the table of [0006](../decisions/0006-log-integrity-and-erasure.md) §2 ([01](01-log-and-records.md) §3.3); like `0x04` and `0x05` ([0015](../decisions/0015-record-format-and-partition-registry.md) §1) it never appears in the header tree. It has the shape of a client signature ([0015](../decisions/0015-record-format-and-partition-registry.md) §1), with the instance key in place of an actor's and the authority bound into the preimage, so the same verification code checks both.

**Why the instance key.** It is the one key a guest's bundle already trusts: the first `key:` record of the guest's `config` and the key on every one of its checkpoints (§2.4). Signing instance acts with it adds no trust the guest does not already extend. It makes that trust auditable record by record, and it lets a third party holding only the guest's bundle tell an instance act from a tenant edit and check it, without the primary tenant's partitions.

**Only the instance writes them.** The write path refuses any submitted record whose attestation names the `instance` issuer, and any attestation with an `authority` field, with `ts-prerogative`. The server writes instance attestations itself, after checking the authority record and the operator's rights.

### 8.4 Authority records

*Sources: [0040](../decisions/0040-instance-prerogatives.md) §4.*

**Every written act has an authority record**, a record in an **instance partition** (§2.1) that says what the instance decided and why. It is attested by the operator who acted, and its comment part holds the public reason. The authority is whatever record already expresses the decision:

| Act | Authority record |
|---|---|
| Instance key registration and rotation | The `key:` record in the instance `config` |
| Tenant creation, template provisions and locked templates | The `tenant:`, `template:` and `tenancy:locked` records in the instance `config` |
| Farm account rename and vanish cascades | The actor record in `actors/{farm}` |
| Global filter `block` and `degroup` | The global filter record, in the instance `log` (§8.6), with the hit's coordinates in the guest record's content |
| Expunge | The `expunge` record in the instance `log` ([0039](../decisions/0039-files-and-media.md) §10) |
| Reclamation, tenant deletion, and any other scheduled or bulk act | The **job record** in the instance `log` |

**Jobs that act on the instance's authority record in the instance `log`.** [0011](../decisions/0011-logs.md) §6.3 puts every job record in the local log. A job that writes instance acts into guests (a reclamation run, the propagation of an expunge, the erasure of a deleted tenant, a cascade of renames) records its start, finish and counts in the instance `log`, keyed by job ID, and that job record is the authority its writes cite. Mirror syncs are unchanged in this respect; their job records also go to the instance `log` ([01](01-log-and-records.md) §1.4).

**Operators act from the primary tenant.** An authority record is attested by an actor of the primary tenant, or by a farm account acting through a global group at the farm base ([0028](../decisions/0028-tenancy-policy.md) §3), and only with an instance right ([0040](../decisions/0040-instance-prerogatives.md) §9, in [09](09-security-and-moderation.md)). "The primary tenant" means the tenant that held the role when the authority record was appended, as the history of the `primary` record shows (§7.1), and `verify` checks it so; an authority record attested by an operator of a former primary tenant stays valid after a transfer.

### 8.5 Binding

*Sources: [0040](../decisions/0040-instance-prerogatives.md) §5.*

A prerogative binds the guest in three ways:

1. **It cannot be superseded by the guest.** A guest write that would replace or retire a binding record is refused with `ts-prerogative`, naming the authority's public reason. Only another instance act, on the same or a later authority, can change it.
2. **It cannot be erased or hidden by the guest.** No part of a binding record can be the target of a guest's `erase` or `record` ACL ([0023](../decisions/0023-moderation.md) §2). The instance can erase it, as an instance act.
3. **Where the guest's own records share a key with it, the prerogative is a floor.** A block, a group membership or a setting can come from both the guest and the instance. They are evaluated as two layers, and the guest's layer cannot lower the instance's: a guest unblock lifts only the guest's own block, and a global filter's block holds until it expires or the instance lifts it. A guest may still add to it, by blocking longer or more widely.

A provision binds nothing: the guest's next record on the same key replaces it in the ordinary way, and the provision stays in history as the instance's.

### 8.6 The written acts

*Sources: [0040](../decisions/0040-instance-prerogatives.md) §6; [0028](../decisions/0028-tenancy-policy.md) §2, §8.*

| Act | Written into the guest | Binding | Notes |
|---|---|---|---|
| **Instance key** registration and rotation (§2.4) | `key:` records in `config` | Yes | The copies carry the instance attestation with the instance `config` record as authority |
| **Tenant creation** and template **provisions** (§5.3, `defaults`) | `config` records | No | |
| **Locked templates** (§5.3, `locks`) | `config` records, re-written on every change | Yes | `ts-locked` is a case of `ts-prerogative`; the code `ts-locked` is kept for compatibility |
| **Farm account rename and vanish** (§3.7) | Actor records in `actors` | Yes | A guest bureaucrat cannot rename a linked account away from its farm name while shared names are on |
| **Tenant deletion** (§6.6) | `erase` records over every partition, after the grace period | Yes | The job record in the instance `log` is the authority |
| **Global filter `block` and `degroup`** ([0030](../decisions/0030-edit-filters.md) §4, §8, in [09](09-security-and-moderation.md)) | `block` and `membership` records in `actors` | Yes, for the block's duration | Global filters live in the **instance `log`**; they remain `scope = global` filters with the same evaluation order. Hits stay in the guest's `log`, attested by the actor whose write was hit, since the hit records that actor's act |
| **Expunge** ([0039](../decisions/0039-files-and-media.md) §10, in [12](12-files-and-media.md)) | `erase` records over upload content parts | Yes | The erasures carry the instance attestation, with the `expunge` record as authority |
| **Reclamation** ([0039](../decisions/0039-files-and-media.md) §9, in [12](12-files-and-media.md)) | `erase` records over upload content parts | Yes | The reclamation job record in the instance `log` is the authority |

The evaluated acts of §8.1 write nothing into guests and need no attestation. Each has its authority record in an instance partition already (a global group, a block in `actors/{farm}`, the `sitelink-policy:deny` record, a `blob` ACL), and that record is attested by the operator who wrote it, visible under the rule of §8.2.

**A new instance act** needs only to say which form it takes, what its authority record is, and whether it binds. If it is written, it uses §8.3.

### 8.7 Verification and moving a tenant

*Sources: [0040](../decisions/0040-instance-prerogatives.md) §8.*

**`verify`** ([01](01-log-and-records.md) §8) level 2 checks every instance attestation: the signature against the guest's key chain, using the key that was current when the record was appended; that the record's `actor` and `authority` are well formed; and, where the authority extract (§6.2) is present, that the authority's leaf hash matches. An instance attestation with no extract entry is reported as **authority not included**, which is a warning, since the instance `log` is internal; a bad signature is a failure.

**After a move,** the old instance's acts stay verifiable through the old key, which stays in the guest's key chain. The new instance's acts are signed by the new key. A guest that leaves takes no prerogative with it as a rule: bindings from the old instance stop binding once the tenant is registered elsewhere, as global groups and blocks stop applying (§6.5). The records remain in history, attested by the old instance's operator.

## 9. Tenant isolation in the query service and in packed storage

*Sources: [0059](../decisions/0059-query-service.md) §4; [0058](../decisions/0058-packed-record-storage.md) §8.*

### 9.1 Query isolation: by dataset, or by store

*Sources: [0059](../decisions/0059-query-service.md) §4.*

**`query.isolation = dataset`** (the default). One store holds every public tenant's graphs. A tenant's **dataset** is the set of graphs its own `full` stream would carry: its resolved graph, its metadata graph, and the source graphs of the providers it has opted in to. The service fixes the dataset through the SPARQL Protocol, `default-graph-uri` and `named-graph-uri` ([SPARQL 1.1 Protocol](https://www.w3.org/TR/sparql11-protocol/) §2.1.4): the resolved graph is the default graph, so a query written for the Wikibase-compatible dump runs unchanged, and the rest are the named graphs, so `GRAPH ?g` ranges over them and nothing else. Oxigraph and QLever both honour the protocol's dataset. A query whose text carries `FROM` or `FROM NAMED` is refused (`ts-query-dataset`), because the protocol says the protocol's dataset overrides the query's and the service does not rely on a store getting that right. The dataset is computed from the tenant's provider policy at each request, so a tenant that drops a provider stops seeing it at once, before the store has forgotten anything.

**`query.isolation = store`.** A tenant may instead have its own embedded store, under `query.path/{tenant}`, fed from its own filtered deltas (the rows its `full` stream would carry), and queried with no other tenant's graphs present. This is for the tenant whose separation requirement is that its queries never touch a store holding anyone else's data, or that another tenant's fault never slows its queries. It costs that tenant its own copy of every provider graph it uses. `store` is **only ever embedded** on a farm: there the setting is refused with `remote`, because a store per tenant would mean a QLever per tenant. On a single-tenant instance the remote store holds one tenant's graphs and satisfies `store`. On a farm the two may mix: a tenant `config` setting `query.isolation` overrides the instance default, and a `store` tenant's graphs are excluded from the shared store.

**A private tenant** (§4.3) has no graphs in any store, as it has no stream and no public form. It has no query service in version 1.

The store is instance infrastructure, like OpenSearch ([0028](../decisions/0028-tenancy-policy.md) §9, §12): one service, every tenant's public data, isolation at query time. `query.isolation` is a row of the tenancy policy's switch table (§5.1): `store` under the `isolated` preset, `dataset` under the shared presets. The two lines this adds to the deployment boundary of [0056](../decisions/0056-security-model.md) §10 are in [09](09-security-and-moderation.md); the service itself and its backends are in [02](02-graphs-rdf-and-query.md) §7.

**Not yet.** Whether a `store` tenant behind the evaluator could have a query service while private is open ([0059](../decisions/0059-query-service.md) Q1).

### 9.2 Storage domains

*Sources: [0058](../decisions/0058-packed-record-storage.md) §8.*

A **domain** is the scope within which a fragment ([03](03-storage-caches-and-search.md) §3.3) is stored once.

- **By default each tenant is its own domain,** covering all its partitions, and the instance's shared partitions (those with a NULL tenant: the provider mirrors) form domain `''`. A tenant's rows refer only to its own domain's fragments.
- **An instance may choose one domain for everything** with `storage.dedup_scope = "instance"`, as [0039](../decisions/0039-files-and-media.md) §4 lets it share file storage. It is told what it gives up: a tenant's fragments can no longer be dropped with its partitions, and erasure sweeps cover every tenant's rows.
- **Keyed refs and separate domains keep storage inside the 0056 boundary.** No tenant's write can learn, even through timing, whether another tenant holds the same bytes. When a tenant's partitions are dropped after the `erase` records of its deletion (§6.6), its domain's child table goes with them. Moves ship logical bundles, which know nothing of fragments.
- **A `private` partition is never packed.** The accounts partition's child table is owned by the accounts role ([0013](../decisions/0013-postgres-storage.md) §4), and its content must not land in a table that `ts_server` reads.

Rows of a tenant's local graph do not refer to fragments in the shared domain, even where a local statement cites the same reference as the mirror. That would couple the shared domain's sweeps and erasures to every tenant's rows. Whether the saving is worth that coupling is open ([0058](../decisions/0058-packed-record-storage.md) Q2).

## 10. Features at a tenant boundary

*Sources: [0028](../decisions/0028-tenancy-policy.md) §6, §7; [0024](../decisions/0024-subsidiary-accounts.md) §7; [0025](../decisions/0025-oauth-server.md) §7; [0032](../decisions/0032-sparql-update-stream.md) §4; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §9.*

### 10.1 Discussion

*Sources: [0028](../decisions/0028-tenancy-policy.md) §6.*

With `discussion.cross_tenant = read-only`, the talk page of an entity mirrored from a provider tenant shows, beneath the local threads, the **provider tenant's threads about that entity**, read-only, with a link to reply there, provided the provider tenant's `discussion.share` setting (its `site` configuration, default on under this switch) allows it. The threads are read through the same partition access as the entity data (§4.1), never copied, and never fed into the reading tenant's feeds, search or notifications. Threads about entities the reading tenant does not mirror are not shown. Under `off`, [0019](../decisions/0019-discussions.md) §2's rule stands unchanged: a mirrored entity's talk page holds local threads only.

### 10.2 Notifications

*Sources: [0028](../decisions/0028-tenancy-policy.md) §7.*

With `notifications.cross_tenant = home`, a person's **bell aggregates the inboxes of every tenant account linked to their farm account**. The join is made by `triplespace-notify` under the accounts role, over the public link table and the private inboxes the person owns, and is what Echo's cross-wiki notifications are. A mention that resolves to a **farm account**, because the person has no account on the mentioning tenant or because the mentioner wrote the farm name, is delivered to the inbox of the person's **home tenant**: a farm preference ([0027](../decisions/0027-preferences-and-portability.md) §1, key `farm.home_tenant`, default the tenant where the farm account was created). Cross-tenant delivery for the `talk`, `reply`, `watch`, `rights` and `job` reasons needs nothing new, since each fires on the tenant that holds the account. Under `off`, [0021](../decisions/0021-notifications.md) §2's rule stands: only a tenant's own accounts have inboxes there, and a mention of `librarybase:42` on another tenant addresses nobody.

### 10.3 Subsidiary accounts and API keys

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §7.*

Identity is per tenant, so a subsidiary belongs to the tenant its operator belongs to, and a bot that works on two tenants of a farm is two subsidiaries with two operators and two sets of keys. There is no cross-tenant bot, as there is no cross-tenant account. When a tenant moves (§6.2), the operator relation travels in the actor records and keys do not, since nothing private does; the operator reissues keys after reclaiming their own account.

### 10.4 OAuth tokens and consumers

*Sources: [0025](../decisions/0025-oauth-server.md) §7.*

A token is a credential of one subsidiary, and a subsidiary belongs to one tenant (§10.3), so a token acts in one tenant. A person who uses a tool in two tenants authorizes it twice, once on each tenant's host, and gets two subsidiaries or chooses two existing ones. The consent page is served on the tenant host where the person is logged in, and the tokens work only against that tenant's API; the same consumer record serves both. On a farm with a shared account database (§3.5) this is unchanged: the farm actor logs in, but the subsidiary created is a tenant actor with the farm actor's tenant account as operator.

Consumer registration and approval are instance-level ([0025](../decisions/0025-oauth-server.md) §2, in [07](07-actors-and-accounts.md)), so under the `isolated` preset the instance operator approves consumers and every tenant may then allow or deny them; under `community`, holders of `mwoauthmanageconsumer` in a global group do; under `enterprise`, the operator approves and locks a `consumer-policy` template.

### 10.5 The update stream: shared deltas and overlay deltas

*Sources: [0032](../decisions/0032-sparql-update-stream.md) §4.*

A tenant's stream ([02](02-graphs-rdf-and-query.md) §6) is the sequence of changes to **what that tenant's resolved view shows**. Under the shared views of [03](03-storage-caches-and-search.md) §7, most of a tenant's view is shared rows computed once for the instance; only entities the tenant has touched have overlay rows.

- **A shared-row delta is stored once**, with the instance's empty-string tenant and `provider` set. It appears in the stream of every tenant whose `providers` list (§2.5) includes that provider and which has no overlay for the entity. A change to a Wikidata item is one row, read by a thousand tenants' streams.
- **An overlay delta is the tenant's own**, stored with its `tenant`. It is computed against the state *that tenant's stream last showed*: when a tenant first overlays an entity, the old state is the shared state, so the delta removes the shared triples that change and adds the overlay's; when an overlay goes away, the reverse. While an overlay exists, shared deltas for that entity are filtered out of the tenant's stream and the overlay is recomputed instead, which the overlay rule of [0013](../decisions/0013-postgres-storage.md) §7 already does.
- **The farm base** serves the instance-level source graphs only, `graphs=mirror/{provider}`, for a consumer that wants a provider's mirror without any tenant's view; there is no resolved view at the farm base.
- **A provider tenant's entities** appear in a reading tenant's stream under the reading tenant's rewriting (§4.1): `LBQ6` with Librarybase's canonical IRI, exactly as its dump shows them.

### 10.6 Page repositories

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §9.*

Writing a `page-repo` record, `pages.repos` or `pages.share` needs `ts-config`, as every `config` kind does ([0016](../decisions/0016-permissions-and-access-control.md) §2); reading a foreign page needs `read`, the repository's own content being public by definition. An instance-scope `page-repo` is shared machinery under `config.template` (§5.3).
