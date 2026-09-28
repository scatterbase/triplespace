# 0007. Actor identity

- **Status:** Proposed
- **Date:** 2026-09-25
- **Amended by:** [0011 — Upstream and local logs](0011-logs.md), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md), [0016 — Permissions and access control](0016-permissions-and-access-control.md), [0018 — Tenants](0018-tenants.md), [0020 — Change feeds](0020-change-feeds.md) (§3 extends §8: the watch set is private state beside the accounts graph), [0021 — Notifications](0021-notifications.md) (§3 and §5 extend §8: inboxes, contact details and the notifier key are private state), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§5 amends §4: the `hidden` status is set by an `actor` ACL), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§1 amends §4 and §6: a bot is a subsidiary account whose actor record names its operator, with a `retired` status; §4 extends §3: API keys are the credentials of subsidiaries only), [0027 — Preferences, private state and portability](0027-preferences-and-portability.md) (§2 classifies the private state of §3 and §8 by portability; §4 extends vanish with the data export), [0028 — Tenancy policy](0028-tenancy-policy.md) (§2 extends §1, §3 and §7: the farm as an issuer whose accounts edit nothing; tenant accounts created and publicly linked from them under a shared-names policy, with the link consent given once at farm signup), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§8 extends §5 with the `federated` actor kind; §6 extends §7 with `rel="me"` links), [0025 — The instance as an OAuth server](0025-oauth-server.md) (§3 amends §4: the `pending` status of a subsidiary created in an OAuth authorization)
- **Author:** James Hare / Claude Opus
- **Related:** [0001 — Revision metadata in RDF](0001-revision-metadata-rdf.md) (§4, §6; §9 extends §6, and §2 settles the actor-IRI open question), [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (§8.3; §2 settles the actor half of the upstream-IRI open question), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (amends §2 and §4.1), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md) (§3, §7)

## Context

Every change in Triplespace is attributed to an **actor**: the account, person or system responsible for it. [0001](0001-revision-metadata-rdf.md) attaches actors to revision nodes with `prov:wasAttributedTo`, but it leaves their IRIs open. It asks three questions:

- how to pseudonymise anonymous IPs and temporary accounts;
- how to refer to imported users (`imported>Name`);
- how to refer to users from foreign sources.

[0002](0002-source-graphs-and-mass-ingest.md) makes the last question concrete. It backfills upstream revisions with their upstream actors, and it records an actor on every import job (§8.3). [0005](0005-crate-organization.md) §4.3 and [0006](0006-log-integrity-and-erasure.md) §3 put the actor in each record's erasable attestation.

Identity in Triplespace is **plural**. No single user database covers every source:

- **The instance has its own users.** Authentication is usually delegated, for example to Wikimedia through OAuth, but the accounts belong to the instance.
- **Each foreign graph has its own users.** Wikidata attributes every revision to one of its accounts. Another Wikibase has a separate user table.
- **Some sources have no individual users.** OpenAlex publishes no per-change attribution and no revision history.

Wikidata and Wikibase have no convention for users in RDF. Their RDF export describes entities, not the accounts that edited them.

Four constraints follow from how MediaWiki treats accounts:

1. **Usernames are not identifiers.** Users are renamed, and a rename keeps the account's numeric ID.
2. **Usernames must be erasable.** Wikimedia's right to vanish renames an account to a placeholder. Suppression can hide a username from everyone except oversighters.
3. **Numeric IDs are per wiki.** A Wikidata revision records Wikidata's local `user_id`. Wikimedia's global account system, CentralAuth, assigns a different **central ID** to the same account.
4. **OAuth returns the central ID.** The `sub` claim of Wikimedia OAuth's identify response is the CentralAuth ID. Newer access-token JWTs spell it `mw:CentralAuth:<id>`. Neither is the Wikidata `user_id` that the person's Wikidata revisions carry.

## Decision

### 1. An actor is an issuer and a subject

Actors are namespaced by **issuer**, as entities are namespaced by provider in [0004](0004-identity-clusters-and-equivalence.md) §1. The issuer is the authority that assigns the account. The subject is the ID it assigned.

**The issuer is not the provider.** One provider may involve several issuers, and some issuers provide no content:

| Issuer | Code | Actor model | Actors come from |
|---|---|---|---|
| This instance | `local` | Numeric | Local users (§3) |
| Wikidata | `wikidatawiki` | Numeric | Wikidata revisions and logs, in the Wikidata mirror |
| Wikimedia central accounts | `wikimedia-central` | Numeric, never published | Wikimedia OAuth only (§3) |
| OpenAlex | `openalex` | Provider only | None. Changes are attributed to OpenAlex as a whole (§6). |
| Password on this instance (amendment, §3) | `password` | Numeric, the tenant's own user IDs | None. A login provider only |

Another Wikibase or Miraheze wiki gets an issuer of its own, keyed by its wiki ID.

**The issuer registry is configuration.** Like the provider registry ([0002](0002-source-graphs-and-mass-ingest.md) §4), it is data passed in by the caller and recorded in the log ([0004](0004-identity-clusters-and-equivalence.md) §9). Each entry records:

- the issuer code;
- the actor model: `numeric` or `provider-only`;
- the actor IRI template (§2);
- the API used to resolve names and central IDs, where there is one;
- which providers' adapters attribute actors to it;
- whether it may serve as an identity provider for local login.

**For Wikibase issuers, the subject is the numeric user ID.** A name is used only where no ID exists (§5).

**An actor key** is the compact form `{issuer}:{id}`, such as `wikidatawiki:12345` or `local:42`. It is what log headers carry. [0006](0006-log-integrity-and-erasure.md) §3 requires a header key to be an identifier, never content. An actor key is an identifier and never contains a name.

### 2. Actor IRIs

| Actor | IRI | Example |
|---|---|---|
| Local user | `{base}/user/{id}` | `{base}/user/42` |
| User of a MediaWiki issuer | `{article path}Special:Redirect/user/{id}` | `https://www.wikidata.org/wiki/Special:Redirect/user/12345` |
| Actor with no stable ID (§5) | `{base}/actor/{n}`, a surrogate minted by the instance | `{base}/actor/7` |
| Provider as a whole (§6) | The agent IRI in the provider registry | The provider's Wikidata item, for example |
| Wikimedia central account | None | Central IDs are never published (§3) |

**The upstream IRI is canonical for foreign actors**, as it is for foreign entities ([0002](0002-source-graphs-and-mass-ingest.md) §4). `Special:Redirect/user/{id}` is minted in the issuer's own URL space, and MediaWiki supports it on every wiki. It survives renames, and it dereferences to the account's current user page. An issuer whose wiki has no such page names another template in the registry.

**An IRI never contains a name or an IP address.** Both can change and both can be erased. The IRI must still be valid afterwards.

**Local user IRIs are instance data**, under the instance's base URI ([0001](0001-revision-metadata-rdf.md) §5, [0005](0005-crate-organization.md) §5).

### 3. Local users and delegated authentication

**The instance mints its own user IDs.** They are sequential, start at 1 and are never reused. A local user's identity is `local:{id}`, whatever the user logged in with.

**Logging in uses a binding.** A binding maps an identity-provider subject to a local user, for example `(wikimedia-central, 7654321) → local:42`. The rules are:

- A local user may hold several bindings, and may add or remove them.
- A provider subject is bound to at most one local user.
- Tokens and other secrets are never written to the log. They live in an operational store.
- The `sub` claim is normalized. The legacy form `7654321` and the newer form `mw:CentralAuth:7654321` are the same subject.

**Keying local users on OAuth's `sub` is rejected.** An instance could then never change or add identity providers. The central ID is also not what Wikidata attributes edits to (Context, 4).

> **Amended 2026-09-27: the built-in password issuer.** One issuer is operated by the instance itself, `password`, registered in `issuers.toml` with `builtin = true`. Its subject for a local user is that user's own ID, so a password login is the binding `(password, 42) → local:42`, and the hash lives in `private.password` ([0013](0013-postgres-storage.md) §4), never in the log. It is an identity provider and nothing else: no actor is ever attributed to it, and it appears on `Special:UserLogin` ([0010](0010-site-ui.md) §10) as one button among the issuers. Whether it accepts logins is the `site` setting `login.password`; `triplespace-cli instance create` turns it on and sets the first administrator's password ([0016](0016-permissions-and-access-control.md) §3), and an instance that registers an external provider may turn it off or leave it as a fallback. This is the single-user fallback Scatterbase asks for, and it settles how an instance with no external identity provider is administered ([0024](0024-subsidiary-accounts.md), open questions). It reverses the choice above only to this extent: the instance consumes providers, and is one, for its own users, never for other sites ([0025](0025-oauth-server.md) leaves OpenID Connect open).

**Bindings are private.** They go to a new `accounts` graph (§8), which is never projected, exported or placed on a feed. The instance may use a binding for its own decisions, such as requiring a Wikimedia account in good standing to edit. It never shows which provider a user logs in with.

**Logging in does not link identities.** A binding proves that the user controls a Wikimedia account. It does not mean the user consents to that account being linked to their local account in the graph. Linking is a separate act that only the user can perform (§7).

**Anonymous local editing uses temporary accounts.** If an instance allows editing without logging in, each such editor gets a local temporary account with a numeric ID. Local change sets never carry an IP address as their actor. IP addresses collected for abuse handling stay in an operational store and are never written to the log.

### 4. Names are attributes, kept in actor records

**Change sets and revisions carry actor keys, never names.** Each actor's name, kind and status live in **actor records**: log records whose header key is the actor key. The payload type is `scatter:v0/actor`, and the body holds:

- the actor's kind: `registered`, `temporary`, `bot`, `anonymous` (§5), `imported` (§5), `provider` (§6) or `federated` (§5, since [0022](0022-federation.md) §8);
- its current name, if it has one;
- its status: `active`, `renamed`, `vanished` or `hidden` (and, for a subsidiary account, `retired`, [0024](0024-subsidiary-accounts.md) §2, and `pending`, [0025](0025-oauth-server.md) §3);
- for surrogates only, the raw value the surrogate stands for (§5);
- for a `bot`, its **operator**: the actor key of the primary account that owns it ([0024](0024-subsidiary-accounts.md) §1).

This puts every name and IP address in one place per actor, so each can be erased in one place.

**Local actors** are recorded in a new `actors` graph (§8) with full history. A rename appends a new record.

**Foreign actors** are recorded in one `actors/{provider}` graph per provider, with history policy `latest`. Adapters split what upstream sends. Wikidata's XML dumps and API give a name and an ID for each revision. The ID goes into the revision, and the name goes into an actor record. When upstream renames or vanishes an account, the next sync appends an actor record with the new name. Compaction then removes the old name.

**RDF carries only the current name.** In the metadata graph, an actor node gets `sioc:name` from its latest actor record, and no past names. The name is not emitted if the actor is hidden, as 0001 §4 already requires.

**Right to vanish** is handled like this:

1. An `erase` record ([0006](0006-log-integrity-and-erasure.md) §7) erases the local actor's records by key. That removes every past name and every link (§7).
2. A new actor record gives the placeholder name, with status `vanished`.
3. Revisions stay attributed to `{base}/user/{id}`, which never contained the name.

This matches Wikimedia's practice: a vanished user's edits stay attributed to the renamed account.

### 5. Actors with no stable numeric ID

| Case | Treatment |
|---|---|
| **Upstream temporary accounts** | Temporary accounts are ordinary user rows with numeric IDs, so they get normal issuer IRIs with kind `temporary`. The IP behind a temporary account is never requested or stored. |
| **Upstream IP edits,** made before temporary accounts | The IP gets a surrogate, `{base}/actor/{n}`, of kind `anonymous`. The IP is kept only in the surrogate's actor record. The same IP maps to the same surrogate through an index projected from actor records. The IP is shown in history views as upstream shows it, and is never projected to RDF. |
| **Imported edits** (`imported>Name`, or another interwiki prefix) | At import time, the adapter resolves the name to the source wiki's numeric ID if the source is a registered issuer, and records the actor under that issuer. Otherwise the actor gets a surrogate of kind `imported`, whose record holds the prefix and the name. |
| **Upstream revisions whose user is hidden** (`userhidden`) | The revision carries a `hidden` marker and no actor key. MediaWiki hides the user ID as well as the name, so there is nothing more to record. |
| **States with no revision metadata** | Observed states from JSON dumps carry no actor ([0002](0002-source-graphs-and-mass-ingest.md) §8.3). Their actor is known only after a backfill from the API or the XML history dumps. |
| **Fediverse actors** replying to a federated talk page | A surrogate `{base}/actor/{n}` of kind `federated`, whose actor record holds the remote actor IRI and its last-seen name; the IRI is content, never a key ([0022](0022-federation.md) §8). |

**Surrogates are minted by the instance, in sequence.** They carry no information about what they stand for. If a surrogate's actor record is erased, the surrogate stays behind as an opaque anonymous actor, and its revisions stay grouped. An IP seen after that erasure gets a new surrogate.

### 6. Providers without individual actors

OpenAlex publishes no per-change attribution. Its changes are attributed to:

- the provider as a whole, an agent of type `prov:Organization` whose IRI comes from the provider registry;
- the import job that brought the change in ([0002](0002-source-graphs-and-mass-ingest.md) §8.3), which is a `prov:Activity`.

No actor records are kept for such providers.

**The job's own actor is separate.** A job records who ran it on this instance. That is a local actor, usually a bot account. It is not the upstream actor. A local bot account is a `prov:SoftwareAgent` with `prov:actedOnBehalfOf` pointing at its operator's local account. Under [0024](0024-subsidiary-accounts.md) §1 every bot account is a **subsidiary** of a primary account, the operator is an attribute of its actor record, and a job's actor is always such a subsidiary.

### 7. Linking accounts is opt-in

A user can **link** their local account to a foreign account they control, so that their attribution carries across sources. Nothing links accounts except the user's own request.

**Creating a link:**

1. The user asks to link an account through an identity provider they hold a binding with (§3). The instance makes them authenticate with that provider again during the request.
2. The instance resolves the account on the target issuer. For Wikidata, it takes the username from the identify response, queries `list=users&usprop=centralids` on Wikidata, and checks that the returned CentralAuth ID equals the binding's subject. The link target is the Wikidata `user_id` that query returns.
3. It appends a `link-account` record to the local `actors` graph, with the local actor's key as its header key.

**Rules:**

- Only the account holder can create a link. Administrators can remove a link, for example after an account is compromised, but cannot create one.
- A local user may link several foreign accounts, such as a main account and a bot account. Each one requires its own proof of control.
- A foreign account can be linked to at most one local user.
- A link is never inferred: not from a binding, not from a matching username, and not from any statement in the data.

**Removing a link erases it.** An `unlink` request appends an `erase` record ([0006](0006-log-integrity-and-erasure.md) §7) with reason class `privacy`. That erases the `link-account` record. A strike would leave the link in history, and the history would disclose the same thing. After erasure, what remains is the trace 0006 allows: a record keyed to the local actor existed at time t.

**Projection.** Only a link that exists appears in RDF. Both accounts point at one holder node in the metadata graph:

```turtle
# graph <{base}/graph/metadata>
<{base}/user/42> a sioc:UserAccount ;
    sioc:name "Example" ;
    sioc:account_of <{base}/user/42#holder> .
<https://www.wikidata.org/wiki/Special:Redirect/user/12345> a sioc:UserAccount ;
    sioc:account_of <{base}/user/42#holder> .
<{base}/user/42#holder> a foaf:Agent .
```

The holder node exists only while a link does. An unlinked local account has no `sioc:account_of`.

**Actor links are not identity clusters.** [0004](0004-identity-clusters-and-equivalence.md) clusters content entities and rewrites them to a canonical ID. Actor links do neither:

- An attribution always keeps the IRI of the account the change was made under.
- No `owl:sameAs` is emitted between accounts. Two accounts held by one person are still two accounts.
- An actor is never linked to a content entity. That includes an `OAA` author and a Wikidata item about a person. Wikidata's P4174 ("Wikimedia username") and similar properties are ordinary data and are never used to link actors.
- When an account has opted in as a fediverse actor ([0022](0022-federation.md) §6), its public account links are emitted as `rel="me"` links in both directions, so a Mastodon profile and a wiki account can verify each other.

### 8. Graphs (amends 0005 §4.1)

Three kinds of graph are added to Triplespace's registry. The first two fill the "destructible data" row of 0005 §4.1.

| Graph | Illustrative IRI | Kind | Written by | History | Integrity | Export |
|---|---|---|---|---|---|---|
| **Accounts** | `{base}/graph/accounts` | Source | Login and account management only | Full | `logged` | **Private** |
| **Actors** | `{base}/graph/actors` | Source | Account management, renames, links | Full | `logged` | Internal only |
| **Foreign actors**, one per provider | `{base}/graph/actors/wikidata` | Source | That provider's sync jobs | `latest` | `hashed` | Internal only |

**`private` is a new export policy.** A private graph is never projected into any graph, never placed on a feed, and never included in an export bundle ([0006](0006-log-integrity-and-erasure.md) §9), including one made for verification. Its integrity can be checked only by the instance itself.

The metadata graph ([0001](0001-revision-metadata-rdf.md) §2) projects from the actor graphs: actor nodes, current names, kinds and links. It never reads the accounts graph.

### 9. Vocabulary (extends 0001 §6)

Existing vocabularies are reused first:

| Need | Vocabulary | Terms |
|---|---|---|
| Accounts and names | SIOC | `sioc:UserAccount`, `sioc:name`, `sioc:account_of` |
| Holder of linked accounts | FOAF | `foaf:Agent` |
| Attribution and delegation | PROV-O | `prov:wasAttributedTo`, `prov:Organization`, `prov:SoftwareAgent`, `prov:actedOnBehalfOf` |

Terms to mint under `scatter:`:

- `scatter:TemporaryAccount`, `scatter:AnonymousActor` and `scatter:ImportedActor`, as subclasses of `prov:Agent`;
- `scatter:attributionHidden`, a flag on a revision whose actor is hidden.

### 10. Crates (amends 0005 §2)

- **`scatter-actors`** is a new substrate crate. It holds the issuer registry, actor keys, IRI templates, the surrogate allocator trait and the link rules. It is pure, following [0005](0005-crate-organization.md) §3 rule 2, and depends only on `oxrdf`.
- **Adapters** turn upstream actor fields into actor keys and actor records.
- **`triplespace-accounts`** is a new surface crate. It holds bindings, OAuth login, the linking flow and the operational store for secrets. It is the only crate that reads the accounts graph.

## Consequences

- **0001's open question about actor IRIs is settled,** and so is the actor half of 0002's open question about upstream revision and actor IRIs.
- **Renames and vanishing touch one place.** Revisions carry keys, not names, so a rename is one actor record and a vanish is one erasure. Attributions never need rewriting.
- **Adapters must split names from IDs at ingest.** A Wikidata backfill writes an actor record alongside the revision metadata, and each Wikidata sync can carry renames.
- **Foreign actor IRIs dereference to a live user page.** Anyone who follows `Special:Redirect/user/{id}` learns the account's current name from upstream. That is public upstream anyway, and it follows upstream renames and vanishes.
- **The same person has unlinked accounts by default.** A user who edits both locally and on Wikidata appears as two actors unless they link them. History views and contribution counts are per account.
- **Opt-in governs only links the instance asserts.** A user who picks the same username locally as on Wikidata makes the connection easy to guess. The instance may suggest the Wikimedia username when an account is created, but it has to say so, and the user chooses.
- **The accounts graph cannot be verified by third parties.** That is the cost of keeping bindings out of every export.
- **An erased link may already have been seen.** The metadata graph is internal only, but the API serves it. Consumers who read a link before it was removed may keep a copy, as 0006 §7 notes for exports generally.

## Open questions

- ~~**Erasing an attribution but keeping the content.** 0006 erases a whole body, so for a local change set it erases the payload and the attestation together. Right to vanish does not need this (§4), but a legal demand to cut the tie between an edit and even a numeric account would. A separate commitment for the attestation would allow it. That changes the record format, so it has to be decided before `scatter-log`'s first release.~~ *Settled by [0015](0015-record-format-and-partition-registry.md) §1: the body is a list of parts, each with its own salt and leaf; an `erase` with `parts: [attestation]` removes the actor and job and leaves the content and its IDs in place.*
- **Issuers in RDF.** Whether actor nodes should carry their issuer explicitly, for example `scatter:issuedBy`, or leave consumers to infer it from the IRI template.
- **Linking beyond Wikimedia.** Other identity providers, such as ORCID, would need their own proof of control. ORCID also names the people that OpenAlex authors refer to, so that proof would come close to the rule in §7 against linking actors to content entities.
- **Links between two foreign accounts.** Whether a user may link, say, a Wikidata account and a Miraheze account without either being local.
- ~~**Keys.** Scatterbase registers client signing keys and needs to bind them to actors. Key registration, rotation and compromise recovery remain with the identity work the two products share ([0006](0006-log-integrity-and-erasure.md) §6).~~ *Settled 2026-09-27 by the amendments of [0015](0015-record-format-and-partition-registry.md) §1 and [0024](0024-subsidiary-accounts.md) §4: a `scatter:v0/key` actor record registers, rotates and revokes an actor's Ed25519 public key, and a subsidiary may sign its writes into the attestation part. Compromise of the instance key remains open in 0006.*
- ~~**Permissions.** Who may rename, hide or vanish local accounts, and who may remove a link.~~ *Settled by [0016](0016-permissions-and-access-control.md) §5: the holder for their own account; `renameuser`, `hideuser` and `ts-unlink` for others; nobody vanishes another's account. Hiding is an `actor` ACL since [0023](0023-moderation.md) §5.*
- **Serving `Special:Redirect/user/{id}` locally.** Whether the MediaWiki compatibility surface should serve it as an alias of `{base}/user/{id}`.

## References

- [MediaWiki `Special:Redirect`](https://www.mediawiki.org/wiki/Help:Special_pages) (lookup by user, page, revision, file or log ID)
- [MediaWiki OAuth extension](https://github.com/wikimedia/mediawiki-extensions-OAuth), `src/UserStatementProvider.php` (the `sub` claim is the central user ID) and `src/Entity/AccessTokenEntity.php` (the `mw:{scope}:{id}` form)
- [Manual:Central ID](https://www.mediawiki.org/wiki/Manual:Central_ID)
- [Temporary accounts](https://www.wikidata.org/wiki/Help:Temporary_accounts)
- [SIOC Core Ontology](http://rdfs.org/sioc/spec/), [FOAF](http://xmlns.com/foaf/spec/), [W3C PROV-O](https://www.w3.org/TR/prov-o/)
