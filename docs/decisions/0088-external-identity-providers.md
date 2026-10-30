# 0088. External identity providers

- **Status:** Proposed
- **Date:** 2026-10-18
- **Updated:** 2026-10-30 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0018](0018-tenants.md), [0027](0027-preferences-and-portability.md), [0035](0035-adopting-a-wikibase.md), [0057](0057-web-tier.md), [0067](0067-proposals.md), [0074](0074-publishing-a-scope-to-an-external-wiki.md), [0087](0087-account-registration-and-challenges.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0016](0016-permissions-and-access-control.md), [0025](0025-oauth-server.md), [0033](0033-backend-stack.md), [0034](0034-frontend-stack.md), [0079](0079-derived-issuer-codes.md), [0085](0085-settings-and-preferences.md)

## Context

[0007](0007-actor-identity.md) §3 decided that a person usually logs in through an identity provider, Wikimedia's OAuth first: a **binding** maps the provider's subject to a tenant account, the instance mints its own user IDs, and the `sub` claim of Wikimedia's OAuth is the CentralAuth ID, normalized from `mw:CentralAuth:{id}`. The issuer registry already lists `wikimedia-central` with `login = true`. `Special:UserLogin` offers a button for each issuer allowed for login, a first login continues to `Special:CreateAccount` to choose a name ([0010](0010-site-ui.md) §10), the account page lists sign-in methods, and `action=linkaccount` and `action=unlinkaccount` manage bindings ([0012](0012-api-requirements.md) §4). [0012](0012-api-requirements.md) §5 lists `/auth/{issuer}/start`, `/auth/{issuer}/callback`, `/auth/pending` and `/auth/pending/create`, and [0057](0057-web-tier.md) §7 keeps sign-in through an issuer on the API. [0087](0087-account-registration-and-challenges.md) §1 lets a first login through an external issuer register, unchallenged.

None of it is built. Only the built-in `password` issuer works. Four things were never decided:

- **Where the instance's own client credentials live.** Wikimedia issues a client ID and secret to whoever registers a consumer, for a callback URL fixed at registration. The registry is public configuration and a `site` setting is public ([23](../architecture/23-configuration-and-registry.md) §1.1), so neither can hold a secret. [0067](0067-proposals.md) A8 and [0074](0074-publishing-a-scope-to-an-external-wiki.md) §8 store other credentials "encrypted at rest" without saying with which key.
- **The endpoints.** An issuer's entry names a users API, not an authorization server.
- **Miraheze.** Its wikis share one CentralAuth, with OAuth on Meta, as Wikimedia's do; the registry has no issuer for it.
- **How adopted accounts are reclaimed while the source still runs.** [0018](0018-tenants.md) §10 lets the old wiki vouch by acting as an identity provider for a grace period, and [0035](0035-adopting-a-wikibase.md) Q3 asks whether a MediaWiki source can.

### Direction

James's direction, from the design discussion of 2026-10-18:

- **"Now that basic account creation is supported, I would like to support adding external identity providers via OAuth. In particular, I would for my instance to be able to support logging in with Wikimedia OAuth and Miraheze OAuth. I can take care of requesting OAuth grants from the Wikimedia and Miraheze sides."**
- On the questions put to him, he chose: client credentials in a `private` table set through the CLI, the secret encrypted; one set per instance with a per-tenant override; OAuth 2.0 with PKCE; and for the first implementation, login with first-login registration, adding and removing sign-in methods, and reclaiming adopted accounts by vouching. Upstream edit grants ([0067](0067-proposals.md) §6) stay out.
- On the draft: **"I would defer on implementing login with Miraheze until we figure out for sure how OAuth works there. My plan with Librarybase is for it to be deployed on Triplespace, so Librarybase would not be a source of external identity. Everything else is confirmed."**

## Decision

### 1. OAuth issuers in the registry (extends 0007 §1)

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.1, §2.7.*

### 2. The client (extends 0007 §3; uses 0025)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7, §7.7.*

### 3. Client credentials (extends 0013 §4; extends 0027 §2; extends 0067 §6; extends 0074 §8)

*Changed by A1.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7; [22](../architecture/22-crates-and-stack.md) §4.4.*

### 4. Which issuers a tenant offers (extends 0007 §3)

*Changed by A1.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7.*

### 5. Flows and the callback (amends 0012 §5; extends 0057 §7)

*Changed by A1.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7; [18](../architecture/18-api.md) §2.3, §3.2; [19](../architecture/19-site-ui.md) §2.3.*

### 6. Logging in, and the first login (extends 0007 §3; extends 0010 §10; extends 0087 §1, §6)

*Changed by A1.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7; [18](../architecture/18-api.md) §2.3; [19](../architecture/19-site-ui.md) §2.3.*

### 7. Sign-in methods (extends 0012 §4; extends 0013 §4)

*Changed by A1.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7; [18](../architecture/18-api.md) §2.3.*

### 8. Reclaiming an adopted account by vouching (settles 0035 Q3; extends 0018 §10; settles 0018 Q8 in part; extends 0011 §6.1)

*Changed by A1.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.5; [08](../architecture/08-tenants-and-instances.md) §6.4; [18](../architecture/18-api.md) §2.3.*

### 9. Settings (uses 0085 §1)

*Changed by A1.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7; [18](../architecture/18-api.md) §2.2; [23](../architecture/23-configuration-and-registry.md) §3.2.*

### 10. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **Credentials in the server's configuration or environment.** Simpler, but a restart to change, nothing per tenant, and a secret in a process environment that every child inherits. James chose the table.
- **One consumer per tenant only.** Correct for a farm of many hosts, but a single-host instance would register one consumer per tenant for no reason. The default with an override does both.
- **OAuth 1.0a and `Special:OAuth/identify`.** Wikimedia still serves it, but it needs request signing and a JWT check for nothing OAuth 2.0 does not give.
- **Keeping the provider's tokens** for later use. Login needs none, and an edit grant is a separate authorization with its own consumer ([0067](0067-proposals.md) §6).
- **The Action API door only.** The site would then pass the provider's code through the web tier, and a provider can return to only one registered URL, so the callback would have to be a page of the site that knows which module to continue. Keeping the callback on the API, as [0057](0057-web-tier.md) §7 already did, keeps the code in one tier.
- **Reclaiming silently by matching names.** A source username is not proof of anything; the central ID is.

## Consequences

- ~~**An instance offers Wikimedia login with one CLI command** once the operator holds the consumer, and a tenant on its own host registers its own.~~ *A tenant has no consumer of its own; one serves the instance (A1).*
- **Registering without JavaScript works through an external issuer**, which no challenge guards, so a tenant that keeps the `createaccount` challenge still has a door for readers without JavaScript.
- **No provider token is ever stored**, and no secret is ever returned by an API. A leaked database holds sealed client secrets, which the signing seed opens.
- **Sign-in methods leave no history.** Removing one erases its record, as unlinking erases a public link.
- **Vouching serves a wiki adopted while its MediaWiki keeps running.** Librarybase is not one: it moves onto Triplespace, so its accounts are reclaimed by a bureaucrat, and no issuer vouches for it.
- **A third-party client of the Action API can use the redirect flow only with a return URL on the tenant's origin**, which in practice means a client that drives a browser on the site.
- **An instance offers Wikimedia login on every tenant with one CLI command** once the operator holds the consumer, registered for the auth host's callback (A1).

## Open questions

- **Q1. Other providers.** ORCID, GitHub or a generic OpenID Connect provider would need an `oauth` table of another shape (an ID token instead of a profile call) and a subject rule of their own; [0007](0007-actor-identity.md) Q3 is the linking half.
- **Q2. Acting on the profile.** Whether a tenant may refuse a login whose provider account is blocked, or require a minimum edit count, from the fields §2 ignores.
- **Q3. Re-authentication.** Whether adding a sign-in method or the linking flow of [0007](0007-actor-identity.md) §7 should ask the provider to re-prompt (`prompt=login`), which MediaWiki's OAuth does not support today.
- **Q4. Miraheze.** Logging in with Miraheze's central accounts, as `miraheze-central`, once it is confirmed how Miraheze's OAuth 2.0 endpoints, consumer registration and `sub` claim behave.

## References

- [MediaWiki OAuth extension](https://www.mediawiki.org/wiki/Extension:OAuth): OAuth 2.0 at `rest.php/oauth2/authorize`, `access_token` and `resource/profile`; identity-only consumers; `src/UserStatementProvider.php` and `src/Entity/AccessTokenEntity.php` for `sub`.
- [Manual:Central ID](https://www.mediawiki.org/wiki/Manual:Central_ID): `CentralAuth` and `local` providers.
- MediaWiki's AuthManager in the API: `status: REDIRECT`, `RESTART`, the `*returnurl` and `*continue` parameters, `createpreservestate`, `action=linkaccount` and `action=unlinkaccount`.
- [RFC 6749](https://www.rfc-editor.org/rfc/rfc6749) and [RFC 7636](https://www.rfc-editor.org/rfc/rfc7636); XChaCha20-Poly1305 ([draft-irtf-cfrg-xchacha](https://datatracker.ietf.org/doc/draft-irtf-cfrg-xchacha/)).
- Miraheze's OAuth consumer registration on `meta.miraheze.org`, `Special:OAuthConsumerRegistration`.

## Amendment log

### A1. The auth host and instance logins

- **Date:** 2026-10-18
- **Source:** [0089](0089-the-auth-host-and-instance-logins.md) §3, §4, §5, §6, §7, §10, §14, §16
- **Change:** amends §3, §4, §5, §6, §7, §8, §9
- **Summary:** External identities authenticate instance logins, on the auth host, and no longer tenant accounts. `private.oauth_client` has one row per issuer for the whole instance: the per-tenant rows and the instance default they overrode are withdrawn with the CLI's `--tenant`, and the callback the operator registers is the auth host's. The auth host offers what the instance default of `login.issuers` lists, and a tenant's value says which methods it admits. A bound identity authenticates its login; an unbound one becomes the auth host's pending identity, from which a login is created. On a tenant, a login with no account is answered by an account request, by vouching where the central session was authenticated through the vouching issuer, or as the session's pending login, from which the account is created with `createpreservestate`. Sign-in methods are added and removed on the auth host, and `private.binding` indexes both a login's identities and each tenant account's binding to its login. `siprop=triplespace` reports the auth host.

Replaced text (§3):

> **`private.oauth_client`** holds one row per tenant and issuer: `(tenant, issuer, client_id, secret, callback, created, updated)`, primary key `(tenant, issuer)`, where `tenant = ''` is the **instance default**. Its portability class is "never leaves": a consumer belongs to the host it was registered for, and a moved tenant registers its own.

Replaced text (§3):

> **Resolution.** A tenant uses its own row for an issuer if it has one. Otherwise it uses the instance default, but only when the default's `callback` is the tenant's own callback URL (§5), or a prefix of it registered as one: a provider sends the person back only to the URL its consumer was registered with, so a default registered for the primary tenant's host does not serve a tenant on another host. An issuer with no usable row is not offered on that tenant (§4).

Replaced text (§3):

> and the associated data is the table name, the tenant and the issuer, so a sealed value

Replaced text (§3):

> - `triplespace-cli oauth-client set {issuer} [--tenant {slug}] --client-id {id}

Replaced text (§3):

> - `triplespace-cli oauth-client list [--tenant {slug}]` lists issuers, client IDs, callbacks and which rows each tenant resolves to;

Replaced text (§3):

> - `triplespace-cli oauth-client callback {issuer} [--tenant {slug}]` prints

Replaced text (§3):

> - `triplespace-cli oauth-client remove {issuer} [--tenant {slug}]`.

Replaced text (§4):

> A tenant offers an external issuer for login when the issuer has `login = true` and an `oauth` table, a credential resolves for it (§3), and the tenant's setting **`login.issuers`** lists it. `login.issuers` defaults to `*`, every issuer, so a credential is all an instance needs to offer one; a tenant narrows it to refuse an instance default, and an instance may lock it. `login.password` keeps governing the built-in issuer ([0007](0007-actor-identity.md) §3). A tenant that offers nothing has no way to log in, so a save that would leave `login.password` off and no issuer offered is refused with `ts-no-login-method`.

Replaced text (§4):

> An issuer that stops being offered does not unbind anyone: its bindings stay, and the account page shows them as unavailable.

Replaced text (§5):

> **The callback is one URL per tenant**, `{base}/w/rest.php/triplespace/v0/auth/{issuer}/callback`, served by the API, and it is the URL the operator registers with the provider.

Replaced text (§5):

> on the tenant's own origin (another is `ts-oauth-returnurl`)

Replaced text (§6):

> **A login whose identity is bound** opens a session for the account, as a password login does, and updates the binding's `last_used`. An account that may not log in is refused as it would be with a password.

Replaced text (§6):

> **A login whose identity is not bound**, in this order:

Replaced text (§6):

> 1. If an account request holds the identity as its credential,

Replaced text (§6):

> 2. If the tenant vouches through this issuer, §8 decides.

Replaced text (§6):

> 3. Otherwise the identity becomes **pending**: the flow is kept, bound to the session, until it expires.

Replaced text (§6):

> **Creating the account.** `action=createaccount` with `createpreservestate=1` and a `username`, in the session holding a pending identity, creates the account with a binding to that identity instead of a password; it asks for no password and no challenge, and the `createaccount` throttle counts it as it counts every registration ([0087](0087-account-registration-and-challenges.md) §4). Starting from `Special:CreateAccount` instead, the person types a name and presses the issuer's button: `createaccount` with the button checks the name first, so a name already taken costs no trip to the provider, and the flow then creates the account on `createcontinue`. Under `account.approval`, either files a request whose credential is the issuer and subject. A subject already bound on the tenant is refused with `ts-oauth-bound`, which says to log in instead.

Replaced text (§6):

> **The name.** The field starts empty, and the provider's username is offered as a button labelled with where it came from,

Replaced text (§7):

> **`private.binding`** is the index of external bindings that [0013](0013-postgres-storage.md) §4 left to be defined: `(tenant, issuer, subject, actor_key, partition, offset, added, last_used)`, unique on `(tenant, issuer, subject)`. The binding record in the tenant's `accounts` partition is the truth ([0007](0007-actor-identity.md) §8); the row is written in the same transaction as the record, and points at it so that it can be erased. A password binding's index is `private.password`, which is already keyed by account.

Replaced text (§7):

> **Adding** a sign-in method is the flow with purpose `link`, through `POST /auth/{issuer}/start` from `Special:Account` or `action=linkaccount`, in the session of a primary account. It appends a binding record. A subject bound to another account on the tenant is refused with `ts-oauth-bound-elsewhere`; one already bound to this account is a success that changes nothing.

Replaced text (§7):

> which `meta=authmanagerinfo&amirequestsfor=remove` lists for each of the account's bindings, or **Remove** on `Special:Account`.

Replaced text (§8):

> **The check.** On a login through that issuer whose identity is not bound (§6, step 2), the instance resolves the person's account on the source:

Replaced text (§8):

> If the source answers with user `n` and `{tenant code}:{n}` is an adopted account with no binding and no password, the account is **reclaimed**: one change set appends the binding record and a public

Replaced text (§8):

> if the source names no user, the login continues as an unbound one;

Replaced text (§9):

> `siprop=triplespace` reports the issuers the tenant offers under `login`,

Replaced text (Consequences):

> - **An instance offers Wikimedia login with one CLI command** once the operator holds the consumer, and a tenant on its own host registers its own.

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
