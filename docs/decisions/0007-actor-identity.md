# 0007. Actor identity

- **Status:** Proposed
- **Date:** 2026-09-25
- **Updated:** 2026-10-09 (A20)
- **Author:** James Hare / Claude Opus
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md)
- **Chapters:** [02](../architecture/02-graphs-rdf-and-query.md), [07](../architecture/07-actors-and-accounts.md), [22](../architecture/22-crates-and-stack.md)

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

*Changed by A2, A3, A11, A13, A14.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.1.*

### 2. Actor IRIs

*Changed by A14.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.2.*

### 3. Local users and delegated authentication

*Changed by A2, A3, A8, A11, A12, A16, A19.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.1, §2.2, §2.3, §5.1, §8.3.*

### 4. Names are attributes, kept in actor records

*Changed by A1, A6, A7, A8, A9, A10, A11.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.3, §3.2.*

### 5. Actors with no stable numeric ID

*Changed by A6.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.4.*

### 6. Providers without individual actors

*Changed by A8, A18.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.5.*

### 7. Linking accounts is opt-in

*Changed by A6, A11.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §3.1.*

### 8. Graphs (amends 0005 §4.1)

*Changed by A2, A4, A5, A10, A11.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.6, §8.3.*

### 9. Vocabulary (extends 0001 §6)

*Changed by A17.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §3.2.*

### 10. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §1.3, §2.1, §2.2.*

## Consequences

- **0001 Q3, actor IRIs, is settled,** and so is the actor half of 0002 Q10, upstream revision and actor IRIs.
- **Renames and vanishing touch one place.** Revisions carry keys, not names, so a rename is one actor record and a vanish is one erasure. Attributions never need rewriting.
- **Adapters must split names from IDs at ingest.** A Wikidata backfill writes an actor record alongside the revision metadata, and each Wikidata sync can carry renames.
- **Foreign actor IRIs dereference to a live user page.** Anyone who follows `Special:Redirect/user/{id}` learns the account's current name from upstream. That is public upstream anyway, and it follows upstream renames and vanishes.
- **The same person has unlinked accounts by default.** A user who edits both locally and on Wikidata appears as two actors unless they link them. History views and contribution counts are per account.
- **Opt-in governs only links the instance asserts.** A user who picks the same username locally as on Wikidata makes the connection easy to guess. The instance may suggest the Wikimedia username when an account is created, but it has to say so, and the user chooses.
- **The accounts graph cannot be verified by third parties.** That is the cost of keeping bindings out of every export.
- **An erased link may already have been seen.** The metadata graph is internal only, but the API serves it. Consumers who read a link before it was removed may keep a copy, as 0006 §7 notes for exports generally.

## Open questions

- **Q1.** ~~**Erasing an attribution but keeping the content.** 0006 erases a whole body, so for a local change set it erases the payload and the attestation together. Right to vanish does not need this (§4), but a legal demand to cut the tie between an edit and even a numeric account would. A separate commitment for the attestation would allow it. That changes the record format, so it has to be decided before `scatter-log`'s first release.~~ *Settled by [0015](0015-record-format-and-partition-registry.md) §1: the body is a list of parts, each with its own salt and leaf; an `erase` with `parts: [attestation]` removes the actor and job and leaves the content and its IDs in place.*
- **Q2. Issuers in RDF.** Whether actor nodes should carry their issuer explicitly, for example `scatter:issuedBy`, or leave consumers to infer it from the IRI template.
- **Q3. Linking beyond Wikimedia.** Other identity providers, such as ORCID, would need their own proof of control. ORCID also names the people that OpenAlex authors refer to, so that proof would come close to the rule in §7 against linking actors to content entities.
- **Q4. Links between two foreign accounts.** Whether a user may link, say, a Wikidata account and a Miraheze account without either being local.
- **Q5.** ~~**Keys.** Scatterbase registers client signing keys and needs to bind them to actors. Key registration, rotation and compromise recovery remain with the identity work the two products share ([0006](0006-log-integrity-and-erasure.md) §6).~~ *Settled by [0015](0015-record-format-and-partition-registry.md) §1 and [0024](0024-subsidiary-accounts.md) §4, in their amendments of 2026-09-27: a `scatter:v0/key` actor record registers, rotates and revokes an actor's Ed25519 public key, and a subsidiary may sign its writes into the attestation part. Compromise of the instance key remains open in 0006.*
- **Q6.** ~~**Permissions.** Who may rename, hide or vanish local accounts, and who may remove a link.~~ *Settled by [0016](0016-permissions-and-access-control.md) §5: the holder for their own account; `renameuser`, `hideuser` and `ts-unlink` for others; nobody vanishes another's account. Hiding is an `actor` ACL since [0023](0023-moderation.md) §5.*
- **Q7.** ~~**Serving `Special:Redirect/user/{id}` locally.** Whether the MediaWiki compatibility surface should serve it as an alias of `{base}/user/{id}`.~~ *Settled by [0047](0047-special-pages.md) §9: served, with `revision`, `page`, `file` and `logid`, resolved locally.*

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0001](0001-revision-metadata-rdf.md) §6 | §9 | extends | 0001 A4 |
| [0001](0001-revision-metadata-rdf.md) Q3 | §2, §5. | settles | 0001 Q3 |
| [0002](0002-source-graphs-and-mass-ingest.md) Q10 | §2 | settles | 0002 Q10 |
| [0005](0005-crate-organization.md) §2, §4.1 | §8, §10 | extends | 0005 A2 |

## References

- [MediaWiki `Special:Redirect`](https://www.mediawiki.org/wiki/Help:Special_pages) (lookup by user, page, revision, file or log ID)
- [MediaWiki OAuth extension](https://github.com/wikimedia/mediawiki-extensions-OAuth), `src/UserStatementProvider.php` (the `sub` claim is the central user ID) and `src/Entity/AccessTokenEntity.php` (the `mw:{scope}:{id}` form)
- [Manual:Central ID](https://www.mediawiki.org/wiki/Manual:Central_ID)
- [Temporary accounts](https://www.wikidata.org/wiki/Help:Temporary_accounts)
- [SIOC Core Ontology](http://rdfs.org/sioc/spec/), [FOAF](http://xmlns.com/foaf/spec/), [W3C PROV-O](https://www.w3.org/TR/prov-o/)

## Amendment log

### A1. Upstream renames

- **Date:** 2026-09-26
- **Source:** [0011](0011-logs.md) §6.2
- **Change:** extends §4
- **Summary:** An upstream `renameuser` updates the foreign actor's record and is kept in the provider log without names, so past names are never kept in a graph that is not compacted.

### A2. Identity is per tenant

- **Date:** 2026-09-27
- **Source:** [0018](0018-tenants.md) §4
- **Change:** amends §1; extends §3, §8
- **Summary:** Each tenant is an issuer, coded by its slug; `local` reads as the current tenant's issuer. Bindings map a provider subject to a tenant account and live in the tenant's `accounts` partition. Attribution keeps the account and tenant an edit was made under.

Replaced text (§1):

> | This instance | `local` | Numeric | Local users (§3) |

### A3. The built-in password issuer

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** extends §1, §3
- **Summary:** One issuer is operated by the instance itself, `password`, registered in `issuers.toml` with `builtin = true`. Its subject for a local user is that user's own ID, so a password login is the binding `(password, 42) → local:42`, and the hash lives in `private.password` ([0013](0013-postgres-storage.md) §4), never in the log. It is an identity provider and nothing else: no actor is ever attributed to it, and it appears on `Special:UserLogin` ([0010](0010-site-ui.md) §10) as one button among the issuers. Whether it accepts logins is the `site` setting `login.password`; `triplespace-cli instance create` turns it on and sets the first administrator's password ([0016](0016-permissions-and-access-control.md) §3), and an instance that registers an external provider may turn it off or leave it as a fallback. This is the single-user fallback Scatterbase asks for, and it settles how an instance with no external identity provider is administered ([0024](0024-subsidiary-accounts.md), open questions). It reverses the choice above only to this extent: the instance consumes providers, and is one, for its own users, never for other sites ([0025](0025-oauth-server.md) leaves OpenID Connect open). The issuer table had been given a `password` row in place.

### A4. The watch set is private state

- **Date:** 2026-09-27
- **Source:** [0020](0020-change-feeds.md) §3
- **Change:** extends §8
- **Summary:** A watch is a row in the `private` schema, not a record, under the rules §8 and 0013 §4 set for private state.

### A5. Inboxes and contacts are private state

- **Date:** 2026-09-27
- **Source:** [0021](0021-notifications.md) §3, §5
- **Change:** extends §8
- **Summary:** Inboxes, contact details and the notifier's key are private state.

### A6. Fediverse actors and rel="me"

- **Date:** 2026-09-27
- **Source:** [0022](0022-federation.md) §6, §8
- **Change:** extends §4, §5, §7
- **Summary:** A `federated` actor kind for fediverse repliers, a surrogate whose record holds the remote actor IRI; and `rel="me"` links for public account links of an account that has opted in as a fediverse actor. Both had been written into §4, §5 and §7 in place.

### A7. Hiding is an ACL

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §5
- **Change:** extends §4
- **Summary:** The `hidden` status is set by an `actor` ACL restricted to `suppress`.

### A8. Subsidiary accounts

- **Date:** 2026-09-27
- **Source:** [0024](0024-subsidiary-accounts.md) §1–2, §4
- **Change:** extends §3, §4, §6
- **Summary:** A bot is a subsidiary account whose actor record names its operator; `retired` joins the statuses; a job's actor is always a subsidiary; API keys are the credentials of subsidiaries only; a primary account with subsidiaries vanishes only after transferring or retiring them. §4 and §6 had been edited in place to say so.

### A9. The pending status

- **Date:** 2026-09-27
- **Source:** [0025](0025-oauth-server.md) §3
- **Change:** extends §4
- **Summary:** A subsidiary created in an OAuth authorization is `pending` until approved. §4 had been edited in place.

### A10. Portability of private state

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §2, §4
- **Change:** extends §4, §8
- **Summary:** Private state is classified by whether it moves with a person; the account page suggests downloading one's data before a vanish.

### A11. Farm identity

- **Date:** 2026-09-27
- **Source:** [0028](0028-tenancy-policy.md) §2
- **Change:** extends §1, §3, §4, §7, §8
- **Summary:** The farm may be an issuer whose accounts edit nothing; tenant accounts are created from them under shared names and publicly linked, with consent given once at signup; renames and vanishing cascade from the farm account.

### A12. Adopted accounts

- **Date:** 2026-09-28
- **Source:** [0035](0035-adopting-a-wikibase.md) §4–5
- **Change:** extends §3
- **Summary:** On a tenant that adopts an existing Wikibase, the sequence starts past the source's highest user ID, and the source's accounts are written as `{slug}:{id}` actor records under their own numbers, without bindings, reclaimable as [0018](0018-tenants.md) §10 describes.

### A13. The instance as an actor

- **Date:** 2026-09-30
- **Source:** [0040](0040-instance-prerogatives.md) §2
- **Change:** extends §1
- **Summary:** A reserved issuer, `instance`, with actor model `provider-only`: the instance as operator, actor key `instance:{farm slug}`, IRI `{farm base}/operator`. It is the actor of every instance act written into a tenant; the person who carried the act out is recorded only on the act's authority record.

### A14. Instance-scope IRIs

- **Date:** 2026-09-30
- **Source:** [0046](0046-primary-tenant.md) §7
- **Change:** amends §1; extends §2
- **Summary:** Instance-scope IRIs are under `{farm base}/instance/`: the operator actor is `{farm base}/instance/operator`, and a farm account ([0028](0028-tenancy-policy.md) §2) is `{farm base}/instance/user/{id}`, so neither collides with a tenant's IRIs when the farm base is a tenant's base.

Replaced text: the operator IRI `{farm base}/operator` of A13.

### A15. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–8
- **Summary:** A1–A14 were folded into the Decision and the open questions were numbered. No decision changed. Before this, A3, A12, A13 and A14 were blockquotes; A3, A6, A8 and A9 had also been written into the text in place, without a note; the other entries were recorded only in other ADRs. The file before conversion is commit `0b26a3a`.

### A16. Setting a password; the key form in `private`

- **Date:** 2026-10-03
- **Source:** Direct: James, decision of 2026-10-03 (`triplespace-accounts`, `triplespace-api-action`, `triplespace-server`, `scatter-adapter-internetdomains`)
- **Change:** extends §3
- **Summary:** A password is set by `triplespace instance create --owner-password-file` (or the `TRIPLESPACE_OWNER_PASSWORD` environment variable) and `triplespace password set --password-file`, never on the command line (0033 §12); the hash is Argon2id with the `argon2` crate's defaults as a PHC string, so parameters travel with the hash; the binding record `{"issuer": "password", "subject": "{user id}"}` is appended to the tenant's `accounts` partition the first time an account gets a password. Actor keys in every `private` table are tenant-qualified (`librarybase:7`), as `view.actor` writes them; `local:{id}` is the relative form a tenant uses in its own records, not a storage key.

### A17. The minted-term list is illustrative

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §9
- **Summary:** The list in §9 of terms minted under `scatter:` is illustrative, not a catalogue: `scatter-vocab` is the catalogue of record for Triplespace's own vocabulary, and a term is defined there, not by appearing in §9 or in [0001](0001-revision-metadata-rdf.md) §6, which is amended the same way. (PENDING A21)

Replaced text (§9):

> Terms to mint under `scatter:`:

### A18. One actor record per provider

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §6
- **Summary:** A provider without individual actors has one actor record, of kind `provider`, in `actors/{provider}`, with agent type `prov:Organization`, written when the provider is registered; every attestation, OpenAlex's mirror records included, names an actor key. "No actor records are kept for such providers" becomes this. The job's own actor stays separate, as §6 says. [0013](0013-postgres-storage.md) §5.4 is amended the same way. (PENDING C1)

Replaced text (§6):

> No actor records are kept for such providers.

### A19. The right to see an IP in the abuse-handling store

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §3
- **Summary:** Seeing an IP address in the abuse-handling operational store requires a named right, so that the permission catalogue can list it and [0030](0030-edit-filters.md) §2, §5 and §12 can cite it. Proposed: `ts-viewip` *(new)*, default `sysop`; James to confirm the name and default. (PENDING D4)

### A20. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [07](../architecture/07-actors-and-accounts.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
