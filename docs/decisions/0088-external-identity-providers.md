# 0088. External identity providers

- **Status:** Proposed
- **Date:** 2026-10-18
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0018](0018-tenants.md), [0027](0027-preferences-and-portability.md), [0035](0035-adopting-a-wikibase.md), [0057](0057-web-tier.md), [0067](0067-proposals.md), [0074](0074-publishing-a-scope-to-an-external-wiki.md), [0087](0087-account-registration-and-challenges.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0016](0016-permissions-and-access-control.md), [0025](0025-oauth-server.md), [0079](0079-derived-issuer-codes.md), [0085](0085-settings-and-preferences.md)

## Context

[0007](0007-actor-identity.md) §3 decided that a person usually logs in through an identity provider, Wikimedia's OAuth first: a **binding** maps the provider's subject to a tenant account, the instance mints its own user IDs, and the `sub` claim of Wikimedia's OAuth is the CentralAuth ID, normalized from `mw:CentralAuth:{id}`. The issuer registry already lists `wikimedia-central` with `login = true`. `Special:UserLogin` offers a button for each issuer allowed for login, a first login continues to `Special:CreateAccount` to choose a name ([0010](0010-site-ui.md) §10), the account page lists sign-in methods, and `action=linkaccount` and `action=unlinkaccount` manage bindings ([0012](0012-api-requirements.md) §4). [0012](0012-api-requirements.md) §5 lists `/auth/{issuer}/start`, `/auth/{issuer}/callback`, `/auth/pending` and `/auth/pending/create`, and [0057](0057-web-tier.md) §7 keeps sign-in through an issuer on the API. [0087](0087-account-registration-and-challenges.md) §1 lets a first login through an external issuer register, unchallenged.

None of it is built. Only the built-in `password` issuer works. Four things were never decided:

- **Where the instance's own client credentials live.** Wikimedia and Miraheze issue a client ID and secret to whoever registers a consumer, for a callback URL fixed at registration. The registry is public configuration and a `site` setting is public ([23](../architecture/23-configuration-and-registry.md) §1.1), so neither can hold a secret. [0067](0067-proposals.md) A8 and [0074](0074-publishing-a-scope-to-an-external-wiki.md) §8 store other credentials "encrypted at rest" without saying with which key.
- **The endpoints.** An issuer's entry names a users API, not an authorization server.
- **Miraheze.** Its wikis share one CentralAuth, with OAuth on Meta, as Wikimedia's do; the registry has no issuer for it.
- **How adopted accounts are reclaimed while the source still runs.** [0018](0018-tenants.md) §10 lets the old wiki vouch by acting as an identity provider for a grace period, and [0035](0035-adopting-a-wikibase.md) Q3 asks whether a MediaWiki source can.

### Direction

James's direction, from the design discussion of 2026-10-18:

- **"Now that basic account creation is supported, I would like to support adding external identity providers via OAuth. In particular, I would for my instance to be able to support logging in with Wikimedia OAuth and Miraheze OAuth. I can take care of requesting OAuth grants from the Wikimedia and Miraheze sides."**
- On the questions put to him, he chose: client credentials in a `private` table set through the CLI, the secret encrypted; one set per instance with a per-tenant override; OAuth 2.0 with PKCE; and for the first implementation, login with first-login registration, adding and removing sign-in methods, and reclaiming adopted accounts by vouching. Upstream edit grants ([0067](0067-proposals.md) §6) stay out.

## Decision

### 1. OAuth issuers in the registry (extends 0007 §1)

**An issuer that serves login through OAuth carries an `oauth` table** in `docs/registry/issuers.toml`, beside `login = true`:

| Field | Meaning |
|---|---|
| `authorize` | The authorization endpoint |
| `token` | The token endpoint |
| `profile` | The endpoint that answers who the token's holder is |
| `subject` | `central`, the subject is a CentralAuth ID; or `local`, it is the wiki's own user ID |
| `register` | Where an operator registers a consumer, which the CLI names (§3) |

**`subject` normalizes the `sub` claim.** MediaWiki's OAuth extension answers `sub` as a bare number in its profile and as `mw:{provider}:{id}` in its tokens, the provider being `CentralAuth` on a wiki farm and `local` on a single wiki. Both forms are accepted, and the stored subject is the bare number; a `sub` naming the other provider is refused (`ts-oauth-failed`), since it would name a different numbering.

**`miraheze-central`** *(new)* is Miraheze's central accounts: actor model numeric, no IRI, central IDs never published, `login = true`, no providers, its OAuth on `meta.miraheze.org`. It is to Miraheze's wikis what `wikimedia-central` is to Wikimedia's. A Miraheze wiki that is a source keeps its own issuer, keyed by its wiki ID, for the actors its revisions name (0007 §1).

**`wikimedia-central`** gains its `oauth` table, on `meta.wikimedia.org`. A wiki that runs the OAuth extension without CentralAuth may be given a table of its own with `subject = "local"`; its login subjects are then its user IDs, which is what vouching for a single wiki needs (§8).

### 2. The client (extends 0007 §3; uses 0025)

**The instance is an OAuth 2.0 confidential client** using the authorization code grant with PKCE (`S256`, [RFC 7636](https://www.rfc-editor.org/rfc/rfc7636)), the flow [0025](0025-oauth-server.md) offers on the other side. The consumer it is registered as asks for identity only: on Wikimedia and Miraheze, "User identity verification only", without access to the real name or email address. The authorization request carries no `scope` beyond what the consumer was approved for.

**A flow learns one thing and keeps nothing.** The callback (§5) exchanges the code at `token`, presenting the client ID, the secret and the PKCE verifier, calls `profile` once with the access token, takes `sub` and `username`, and discards both tokens. Nothing the provider issued is stored: a login needs no continuing access, and a token that is never kept cannot leak. Every other field of the profile is ignored, `blocked` and `groups` included; a tenant that wants to act on them is Q2.

**Requests leave through the instance's egress** ([0012](0012-api-requirements.md) §6), with a 10-second timeout. A provider that cannot be reached answers the flow with `ts-oauth-failed`, and the person may try again; nothing is retried in the background.

### 3. Client credentials (extends 0013 §4; extends 0027 §2; extends 0067 §6; extends 0074 §8)

**`private.oauth_client`** holds one row per tenant and issuer: `(tenant, issuer, client_id, secret, callback, created, updated)`, primary key `(tenant, issuer)`, where `tenant = ''` is the **instance default**. Its portability class is "never leaves": a consumer belongs to the host it was registered for, and a moved tenant registers its own.

**Resolution.** A tenant uses its own row for an issuer if it has one. Otherwise it uses the instance default, but only when the default's `callback` is the tenant's own callback URL (§5), or a prefix of it registered as one: a provider sends the person back only to the URL its consumer was registered with, so a default registered for the primary tenant's host does not serve a tenant on another host. An issuer with no usable row is not offered on that tenant (§4).

**The secret is sealed.** It is encrypted with XChaCha20-Poly1305 under the **instance sealing key**, a subkey of the instance signing seed with its own domain separator (`ts-seal-v1`), as the CSRF and challenge keys are derived; the 24-byte nonce is stored with the ciphertext, and the associated data is the table name, the tenant and the issuer, so a sealed value copied to another row does not open. This is the "encryption at rest" of `private.publication_credential` ([0074](0074-publishing-a-scope-to-an-external-wiki.md) §8) and `private.upstream_grant` ([0067](0067-proposals.md) A8), which now use the same key with their own associated data. Rotating the signing seed makes sealed values unreadable; the operator sets them again, and the CLI says so when a value no longer opens.

**The CLI** writes the table; no API route does, and no route returns a secret:

- `triplespace-cli oauth-client set {issuer} [--tenant {slug}] --client-id {id} --secret-file {path}` (or `--secret-stdin`) seals and stores the row, after checking that the issuer has an `oauth` table;
- `triplespace-cli oauth-client list [--tenant {slug}]` lists issuers, client IDs, callbacks and which rows each tenant resolves to;
- `triplespace-cli oauth-client callback {issuer} [--tenant {slug}]` prints the callback URL to give the provider and the issuer's `register` URL;
- `triplespace-cli oauth-client remove {issuer} [--tenant {slug}]`.

### 4. Which issuers a tenant offers (extends 0007 §3)

A tenant offers an external issuer for login when the issuer has `login = true` and an `oauth` table, a credential resolves for it (§3), and the tenant's setting **`login.issuers`** lists it. `login.issuers` defaults to `*`, every issuer, so a credential is all an instance needs to offer one; a tenant narrows it to refuse an instance default, and an instance may lock it. `login.password` keeps governing the built-in issuer ([0007](0007-actor-identity.md) §3). A tenant that offers nothing has no way to log in, so a save that would leave `login.password` off and no issuer offered is refused with `ts-no-login-method`.

An issuer that stops being offered does not unbind anyone: its bindings stay, and the account page shows them as unavailable.

### 5. Flows and the callback (amends 0012 §5; extends 0057 §7)

**A flow** is a ten-minute handle: the tenant, the session it began in, the issuer, its **purpose** (`login`, `create` or `link`), the PKCE verifier, the name chosen for `create`, the `returnto`, the door it came through (below), and, once the provider has answered, the identity it learned or the error. It is a Valkey key, `oauthc:{state hash}`, when a shared cache is configured, and otherwise a row of `private.oauth_flow`, swept as sessions are, so that any replica of an API pool can finish a flow another began ([07](../architecture/07-actors-and-accounts.md) §2.4). The `state` sent to the provider is 32 random bytes, base64url; only its SHA-256 is stored.

**The callback is one URL per tenant**, `{base}/w/rest.php/triplespace/v0/auth/{issuer}/callback`, served by the API, and it is the URL the operator registers with the provider. It looks the flow up by `state`, refuses one whose issuer, tenant or expiry does not match (`ts-oauth-state`), records the provider's `error` (`access_denied` is `ts-oauth-denied`) or exchanges the code (§2), and stores the result on the flow. What happens next depends on the door.

**The site's door** is the REST routes of [0012](0012-api-requirements.md) §5, which the browser reaches directly, so it needs no JavaScript:

- **`POST /auth/{issuer}/start`**, with the session's `logintoken` for `login` or its `csrftoken` for `link` (`purpose`, `returnto`), creates the flow and answers `303` to the provider. It was `GET`; a `POST` with a token keeps another site from starting a flow in a reader's session.
- **`GET /auth/{issuer}/callback`** applies the result to the session the flow began in, and refuses a request whose session is not that one (`ts-oauth-state`). It answers `303` to `returnto` on success; to `Special:CreateAccount?returnto=…` when a login found no account (§6); and on an error, to `Special:UserLogin`, or to `Special:Account#sign-in-methods` for `link`, with `tserror={code}`.
- **`GET /auth/pending`** returns the issuer's name and the provider's username for a pending identity (§6), to its own session only.
- **`POST /auth/pending/create` is withdrawn.** An account is created from a pending identity by `action=createaccount` with `createpreservestate` (§6), so that the name rules, the throttle and approval have one writer.

**The Action API's door** is MediaWiki's AuthManager. For each offered issuer, `meta=authmanagerinfo` lists a request `TriplespaceIssuerAuthenticationRequest:{issuer}` with one field, `tsissuer-{issuer}`, of type `button`, labelled with the issuer's name, for `amirequestsfor=login`, `create` and `link`. `action=clientlogin`, `action=createaccount` or `action=linkaccount` with that field and a `loginreturnurl`, `createreturnurl` or `linkreturnurl` on the tenant's own origin (another is `ts-oauth-returnurl`) creates the flow and answers `status: REDIRECT` with `redirecttarget`, the provider's authorization URL, and the continue request, whose one field is `state`. The callback, for a flow from this door, answers `303` to the return URL with `state` appended; the client then calls `logincontinue`, `createcontinue` or `linkcontinue` with it, in the session that began the flow, and gets the result below. The code never leaves the instance.

### 6. Logging in, and the first login (extends 0007 §3; extends 0010 §10; extends 0087 §1, §6)

**A login whose identity is bound** opens a session for the account, as a password login does, and updates the binding's `last_used`. An account that may not log in is refused as it would be with a password. Neither challenge trigger applies ([0087](0087-account-registration-and-challenges.md) §2).

**A login whose identity is not bound**, in this order:

1. If an account request holds the identity as its credential, the answer is `ts-account-pending` or `ts-account-rejected`, as for a password ([0087](0087-account-registration-and-challenges.md) §6).
2. If the tenant vouches through this issuer, §8 decides.
3. Otherwise the identity becomes **pending**: the flow is kept, bound to the session, until it expires. The Action API answers `status: RESTART` with `authmanager-authn-no-local-user-link`, MediaWiki's answer when a provider authenticated someone who has no account; the site's door continues to `Special:CreateAccount`.

**Creating the account.** `action=createaccount` with `createpreservestate=1` and a `username`, in the session holding a pending identity, creates the account with a binding to that identity instead of a password; it asks for no password and no challenge. Starting from `Special:CreateAccount` instead, the person types a name and presses the issuer's button: `createaccount` with the button checks the name first, so a name already taken costs no trip to the provider, and the flow then creates the account on `createcontinue`. Under `account.approval`, either files a request whose credential is the issuer and subject. A subject already bound on the tenant is refused with `ts-oauth-bound`, which says to log in instead.

**The name.** The field starts empty, and the provider's username is offered as a button labelled with where it came from, with the warning that the same name makes the two accounts easy to connect, as [0010](0010-site-ui.md) §10 has it. The box that would also link the provider's account publicly ([0007](0007-actor-identity.md) §7) is not offered until the linking flow is built.

### 7. Sign-in methods (extends 0012 §4; extends 0013 §4)

**`private.binding`** is the index of external bindings that [0013](0013-postgres-storage.md) §4 left to be defined: `(tenant, issuer, subject, actor_key, partition, offset, added, last_used)`, unique on `(tenant, issuer, subject)`. The binding record in the tenant's `accounts` partition is the truth ([0007](0007-actor-identity.md) §8); the row is written in the same transaction as the record, and points at it so that it can be erased. A password binding's index is `private.password`, which is already keyed by account.

**Adding** a sign-in method is the flow with purpose `link`, through `POST /auth/{issuer}/start` from `Special:Account` or `action=linkaccount`, in the session of a primary account. It appends a binding record. A subject bound to another account on the tenant is refused with `ts-oauth-bound-elsewhere`; one already bound to this account is a success that changes nothing.

**Removing** one is `action=unlinkaccount` with `request=TriplespaceIssuerAuthenticationRequest:{issuer}`, which `meta=authmanagerinfo&amirequestsfor=remove` lists for each of the account's bindings, or **Remove** on `Special:Account`. It appends an `erase` of the binding record with reason class `privacy` ([0006](0006-log-integrity-and-erasure.md) §7) and deletes the row, so that no history keeps which provider the person used. Removing the last way to log in, counting the password, is refused with `ts-last-binding`.

**Neither is a public event.** Bindings are private, and adding or removing one appears in no log, feed or export.

### 8. Reclaiming an adopted account by vouching (settles 0035 Q3; extends 0018 §10; settles 0018 Q8 in part; extends 0011 §6.1)

**A tenant that adopted a wiki may let that wiki vouch** for its old users while it still runs, through the tenant setting **`reclaim.vouching`**: `off` (the default) or the code of an issuer that serves login. The grace period of [0018](0018-tenants.md) §10 is the time the setting is on; the operator turns it off when the source closes.

**The check.** On a login through that issuer whose identity is not bound (§6, step 2), the instance resolves the person's account on the source:

- with `subject = "local"`, the issuer is the source itself, and the subject is the source's user ID;
- with `subject = "central"`, it asks the source's users API (the `users_api` of the source's entry, the one whose `tenant` is this tenant, [0079](0079-derived-issuer-codes.md) §9) for `list=users&ususers={username}&usprop=centralids`, and takes the returned `userid` only if `centralids.CentralAuth` is the subject, as the linking flow of [0007](0007-actor-identity.md) §7 does for Wikidata.

If the source answers with user `n` and `{tenant code}:{n}` is an adopted account with no binding and no password, the account is **reclaimed**: one change set appends the binding record and a public `scatter:v0/reclaim` record in the tenant's `log` partition, keyed by the account, with `{"method": "vouched"}`, and the person is logged in with `ts-reclaimed`, which names the account. If the account is already held, the answer is `ts-reclaim-taken`, which says to ask a bureaucrat; if the source names no user, the login continues as an unbound one; if the source cannot be reached, `ts-reclaim-unavailable`, and nothing is created.

**The event.** `reclaim/reclaim` ([0018](0018-tenants.md) A17) projects from the `reclaim` record, not the binding record: the binding is in the private `accounts` partition, which nothing projects. Its parameter is `method`, `vouched` here and `manual` for a bureaucrat's reclaim, which writes the same record with itself as performer. Its performer here is the reclaimed account. This settles the parameters of 0018 Q8; whether a reclaim is reversible stays open.

### 9. Settings (uses 0085 §1)

Each is a `tenant` setting in the `accounts` group, resolved through the instance's defaults and locks:

| Setting | Values | Default |
|---|---|---|
| `login.issuers` | A list of issuer codes with `login = true` and an `oauth` table, or `*` | `*`, every such issuer |
| `reclaim.vouching` | `off`, or one such issuer code | `off` |

`siprop=triplespace` reports the issuers the tenant offers, by code and name, under `login`, so a client knows before it asks.

### 10. Crates (extends 0005 §2)

| Crate | Change |
|---|---|
| `triplespace-accounts` | The OAuth client: flows and their store, PKCE and `state`, the exchange and profile call, subject normalization; `private.oauth_client` and sealing; `private.binding`, adding, removing and `last_used`; logging in by binding; vouching and the `reclaim` record |
| `triplespace-api-action` | The issuer requests in `meta=authmanagerinfo`; the `REDIRECT` and continue paths of `clientlogin`, `createaccount` and `linkaccount`; `createpreservestate`; `unlinkaccount`; `siprop=triplespace` `login` |
| `triplespace-api-rest` | `POST /auth/{issuer}/start`, `GET /auth/{issuer}/callback`, `GET /auth/pending` |
| `triplespace-ui` | The issuer buttons on `Special:UserLogin` and `Special:CreateAccount`, the pending-identity form, and the sign-in methods section of `Special:Account` |
| `triplespace-cli` | `oauth-client set`, `list`, `callback`, `remove` |
| `triplespace-db` | `private.oauth_client`, `private.oauth_flow` and `private.binding` |
| `scatter-actors` | The `oauth` table of an issuer entry, and `miraheze-central` |

## Alternatives considered

- **Credentials in the server's configuration or environment.** Simpler, but a restart to change, nothing per tenant, and a secret in a process environment that every child inherits. James chose the table.
- **One consumer per tenant only.** Correct for a farm of many hosts, but a single-host instance would register one consumer per tenant for no reason. The default with an override does both.
- **OAuth 1.0a and `Special:OAuth/identify`.** Wikimedia still serves it, but it needs request signing and a JWT check for nothing OAuth 2.0 does not give.
- **Keeping the provider's tokens** for later use. Login needs none, and an edit grant is a separate authorization with its own consumer ([0067](0067-proposals.md) §6).
- **The Action API door only.** The site would then pass the provider's code through the web tier, and a provider can return to only one registered URL, so the callback would have to be a page of the site that knows which module to continue. Keeping the callback on the API, as [0057](0057-web-tier.md) §7 already did, keeps the code in one tier.
- **Reclaiming silently by matching names.** A source username is not proof of anything; the central ID is.

## Consequences

- **An instance offers Wikimedia and Miraheze login with two CLI commands** once the operator holds the consumers, and a tenant on its own host registers its own.
- **Registering without JavaScript works through an external issuer**, which no challenge guards, so a tenant that keeps the `createaccount` challenge still has a door for readers without JavaScript.
- **No provider token is ever stored**, and no secret is ever returned by an API. A leaked database holds sealed client secrets, which the signing seed opens.
- **Sign-in methods leave no history.** Removing one erases its record, as unlinking erases a public link.
- **Adopted Librarybase accounts can be reclaimed by their holders** while Librarybase runs, without a bureaucrat, through whichever issuer reaches Librarybase's users.
- **A third-party client of the Action API can use the redirect flow only with a return URL on the tenant's origin**, which in practice means a client that drives a browser on the site.

## Open questions

- **Q1. Other providers.** ORCID, GitHub or a generic OpenID Connect provider would need an `oauth` table of another shape (an ID token instead of a profile call) and a subject rule of their own; [0007](0007-actor-identity.md) Q3 is the linking half.
- **Q2. Acting on the profile.** Whether a tenant may refuse a login whose provider account is blocked, or require a minimum edit count, from the fields §2 ignores.
- **Q3. Re-authentication.** Whether adding a sign-in method or the linking flow of [0007](0007-actor-identity.md) §7 should ask the provider to re-prompt (`prompt=login`), which MediaWiki's OAuth does not support today.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §10 | extends | 0005 A99 |
| [0007](0007-actor-identity.md) §1 | §1 | extends | 0007 A25 |
| [0007](0007-actor-identity.md) §3 | §2, §4, §6 | extends | 0007 A25 |
| [0010](0010-site-ui.md) §10 | §6 | extends | 0010 A44 |
| [0011](0011-logs.md) §6.1 | §8 | extends | 0011 A34 |
| [0012](0012-api-requirements.md) §4 | §7 | extends | 0012 A78 |
| [0012](0012-api-requirements.md) §5 | §5 | amends | 0012 A78 |
| [0013](0013-postgres-storage.md) §4 | §3, §7 | extends | 0013 A66 |
| [0018](0018-tenants.md) §10 | §8 | extends | 0018 A31 |
| [0018](0018-tenants.md) Q8 | §8 | settles | 0018 Q8 |
| [0027](0027-preferences-and-portability.md) §2 | §3 | extends | 0027 A9 |
| [0035](0035-adopting-a-wikibase.md) Q3 | §8 | settles | 0035 Q3 |
| [0057](0057-web-tier.md) §7 | §5 | extends | 0057 A7 |
| [0067](0067-proposals.md) §6 | §3 | extends | 0067 A10 |
| [0074](0074-publishing-a-scope-to-an-external-wiki.md) §8 | §3 | extends | 0074 A3 |
| [0087](0087-account-registration-and-challenges.md) §1 | §6 | extends | 0087 A2 |

## References

- [MediaWiki OAuth extension](https://www.mediawiki.org/wiki/Extension:OAuth): OAuth 2.0 at `rest.php/oauth2/authorize`, `access_token` and `resource/profile`; identity-only consumers; `src/UserStatementProvider.php` and `src/Entity/AccessTokenEntity.php` for `sub`.
- [Manual:Central ID](https://www.mediawiki.org/wiki/Manual:Central_ID): `CentralAuth` and `local` providers.
- MediaWiki's AuthManager in the API: `status: REDIRECT`, `RESTART`, the `*returnurl` and `*continue` parameters, `createpreservestate`, `action=linkaccount` and `action=unlinkaccount`.
- [RFC 6749](https://www.rfc-editor.org/rfc/rfc6749) and [RFC 7636](https://www.rfc-editor.org/rfc/rfc7636); XChaCha20-Poly1305 ([draft-irtf-cfrg-xchacha](https://datatracker.ietf.org/doc/draft-irtf-cfrg-xchacha/)).
- Miraheze's OAuth consumer registration on `meta.miraheze.org`, `Special:OAuthConsumerRegistration`.
