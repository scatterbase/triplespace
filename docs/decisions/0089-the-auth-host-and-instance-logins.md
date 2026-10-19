# 0089. The auth host and instance logins

- **Status:** Proposed
- **Date:** 2026-10-18
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0018](0018-tenants.md), [0028](0028-tenancy-policy.md), [0033](0033-backend-stack.md), [0047](0047-special-pages.md), [0056](0056-security-model.md), [0057](0057-web-tier.md), [0087](0087-account-registration-and-challenges.md), [0088](0088-external-identity-providers.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0016](0016-permissions-and-access-control.md), [0024](0024-subsidiary-accounts.md), [0025](0025-oauth-server.md), [0034](0034-frontend-stack.md), [0046](0046-primary-tenant.md), [0079](0079-derived-issuer-codes.md), [0085](0085-settings-and-preferences.md)

## Context

[0018](0018-tenants.md) §4 made identity per tenant and said that authentication is uncoupled from identity: a binding maps an identity provider's subject to a tenant account, and "a farm that wants single sign-on registers itself as an identity provider". Nothing ever registered it. What [0007](0007-actor-identity.md) §3, [0087](0087-account-registration-and-challenges.md) and [0088](0088-external-identity-providers.md) built instead binds every credential to a tenant account:

- **A password belongs to a tenant account.** The built-in `password` issuer's subject is the tenant's own user ID, so a person with accounts on three tenants has three passwords and logs in three times.
- **An external identity is bound per tenant**, through a callback at each tenant's own host. A provider returns only to the URL its consumer was registered with, so [0088](0088-external-identity-providers.md) §3 gave each tenant on its own host a credential row of its own, and an instance of ten wikis on ten hosts registers ten consumers with Wikimedia.
- **Sessions are per tenant host** ([0056](0056-security-model.md) §1), and nothing joins them.
- **Farm identity** ([0028](0028-tenancy-policy.md) §2) gives a person one farm account and links their tenant accounts to it publicly, but a farm account still "authenticates through bindings to identity providers exactly as a tenant account does", and the farm has no passwords.

Wikimedia moved its logins to one host, `auth.wikimedia.org`, from which each wiki gets a session; Miraheze did the same with `auth.miraheze.org`. One host is one place to type a password, one callback to register with each provider, and one cookie to hold.

### Direction

James's direction, from the design discussion of 2026-10-18:

- **"A thought comes to mind: these days, Wikimedia has auth.wikimedia.org and Miraheze has auth.miraheze.org. What if instances with more than one tenants had a standalone auth domain that can be used in conjunction with the OAuth identity provider?"**
- **"It should use a dedicated host similar to how Wikimedia has auth.wikimedia.org. Password login should move to the auth host as well. To clarify with per-tenant accounts, it's my understanding that the system is supposed to uncouple authentication with the instance from identity on the tenant. That is to say, whether there is per-tenant identity or farm-wide identity, this is separate from the mechanism used to login in. I am not sure why a tenant would need its own OAuth consumer and I am disinclined to support it. A new ADR makes sense."**
- On the questions put to him, he chose: passwords become **instance logins**, which tenant accounts bind to like any issuer, so one password serves every tenant while actors stay per tenant, and existing password hashes migrate to logins; a **central session** that completes a tenant's login when the person presses "Log in" there, with no password prompt and no silent sign-in; an auth host that is **optional on a single-tenant instance** and required once there is a second tenant; and that creating a login, managing sign-in methods and logging out everywhere also move to the auth host, while bot passwords and the Action API's `clientlogin` stay on tenant hosts.

## Decision

### 1. The auth host (extends 0033 §12; amends 0056 §10; extends 0057 §3)

**An instance with more than one tenant has an auth host**, a host of its own named by the deployment setting **`server.auth_base`**, such as `https://auth.example.org`. It is read as the instance's other deployment configuration is ([0033](0033-backend-stack.md) §12), served by the same binary, and holds no wiki: no pages, no entities, no `/wiki/` content. It is where a person authenticates to the instance (§2), and nowhere else does.

**It is a registered host.** The server answers a request whose `Host` is not a registered tenant base, the farm base or the auth base with 421, so line 5 of the deployment boundary ([0056](0056-security-model.md) §10) names the auth base beside the other two. The auth base is not a tenant base and not the farm base; `instance check` fails on either.

**What it serves**, by the routing table of [0057](0057-web-tier.md) §3, which does not change shape:

| Prefix | At the auth base |
|---|---|
| `/w/api.php`, `/api.php` | The Action API's authentication modules only (§7) |
| `/w/rest.php/triplespace/v0/auth/…` | The routes of §5 and §10 |
| `/wiki/Special:UserLogin`, `Special:CreateAccount`, `Special:Account`, `Special:UserLogout` | The auth host's pages (§7) |
| `/ui/assets/` | The site's assets |
| Anything else | 404 in a plain frame |

**Single-tenant instances** need none (§12). `tenant create` refuses a second tenant while `server.auth_base` is unset, with `ts-auth-base-required`.

### 2. Instance logins (amends 0007 §3; amends 0018 §4; extends 0015 §5; amends 0013 §4)

**A login is how a person authenticates to the instance.** It has an instance-wide ID, `n`, and a **login name**, and it is authenticated by its **sign-in methods** (§3): a password, external identities, or both. A login is not an actor: nothing is ever attributed to it, it appears in no public record, and nobody but its holder sees that it exists. It is what [0018](0018-tenants.md) §4 meant by a farm registering itself as an identity provider, made the instance's only way to log in.

**The issuer `login`** is built in, beside `password` and `instance` (`builtin = true` in `issuers.toml`); its subjects are login IDs, it has no IRI, and its code is reserved in the shared namespace of slugs and issuer codes as every registered code is. A tenant account is bound to a login by the binding `(login, n)` (§4).

**Login names** follow the username grammar of [0010](0010-site-ui.md) §10 and are unique on the instance, case-insensitively. They are private: a login name is shown only to its holder, and a tenant account's public name is chosen separately (§6). A login name may be changed on the auth host; it leaves no tombstone, since no page text names it.

**Where logins live.** A new instance partition **`logins`** (source, `full`, `logged`, private, never exported or projected; graph IRI `{farm base}/instance/graph/logins`) holds one `scatter:v0/login` record per login, keyed `login:{n}`, whose content is `{"name": …}`, and the binding records of its external identities (§3). `private.login (id, name, name_key, created, last_used, partition, offset)` indexes it; IDs are a sequence of the instance, start at 1 and are never reused. Erasing a login (Q2) erases its records with reason class `privacy` ([0006](0006-log-integrity-and-erasure.md) §7).

### 3. Sign-in methods belong to the login (amends 0088 §7; amends 0007 §3; amends 0013 §4)

**A password is a login's.** The built-in `password` issuer's subject becomes the login ID, so a password is the binding `(password, n) → login:{n}`, and `private.password` is keyed by login. One password serves every tenant account bound to the login.

**An external identity is a login's.** A binding of `(wikimedia-central, 7654321)` maps the subject to a login, not to a tenant account: its record is in `logins`, keyed `login:{n}`, and its row in `private.binding` carries the empty tenant. A subject is bound to at most one login on the instance, so one Wikimedia account cannot open two logins.

**Adding and removing methods** are [0088](0088-external-identity-providers.md) §7's, done on the auth host (§7) to the login of the auth host's session: adding is the flow with purpose `link`; removing erases the binding record with reason class `privacy`; removing the last method is `ts-last-binding`; a subject bound to another login is `ts-oauth-bound-elsewhere`. **Changing the password** is `action=changeauthenticationdata` there, and removing it `action=removeauthenticationdata`. Changing or removing a method ends every other session of the login and every tenant session opened from them (§9), as MediaWiki resets sessions on a password change.

### 4. Tenant accounts bind to a login (amends 0007 §3; amends 0088 §6; amends 0028 §2)

**A tenant account is reached through a login.** Its binding in the tenant's `accounts` partition is `{"issuer": "login", "subject": "{n}"}`, indexed by a row of `private.binding` with the tenant and issuer `login`. A tenant account has at most one login binding, and a login at most one account on each tenant; subsidiaries ([0024](0024-subsidiary-accounts.md)) have none, since they authenticate with keys and bot passwords. The tenant's `accounts` partition no longer holds password or external bindings.

**Identity stays per tenant.** `librarybase:42` and `example:17` bound to one login are still two actors, the same person's accounts, linked publicly only by the opt-in act of [0007](0007-actor-identity.md) §7 or by farm identity (§13). Which login an account is bound to is private, as every binding is: it appears in no log, feed or export, and the instance never shows one tenant's account to another tenant.

### 5. Logging in on a tenant (amends 0088 §5; amends 0056 §1; extends 0012 §5; amends 0013 §4; amends 0014 §4)

**The tenant is a client of the auth host**, through [0088](0088-external-identity-providers.md) §5's flows with issuer `login`. Its doors and its callback, `/auth/login/callback` on the tenant's base, are the same routes; only the exchange differs, since both ends are one instance.

**The handshake**, on the site's door:

1. **"Log in"** on the tenant is a form posting to `POST /auth/login/start` with the session's `logintoken` (`purpose=login`, or `create` from `Special:CreateAccount`). It creates the flow, with its PKCE verifier, and answers `303` to the auth host's **authorization route**, `GET {auth base}/w/rest.php/triplespace/v0/auth/authorize`, with the tenant's issuer code, the `state`, the PKCE challenge and the purpose. The tenant's page widens its `form-action` to the auth host's origin, which `siprop=triplespace` reports (§16).
2. **The auth host** looks at its own session. If it holds a login, authenticated by a method the tenant admits (§16), it issues a **login code** and answers `303` to the tenant's callback with the code and `state`. If not, it keeps the request as a ten-minute handle (`authr:{handle hash}` in Valkey, or a row of `private.auth_request`) and answers `303` to its `Special:UserLogin` (§7), where the person authenticates with a password or an external issuer; the code is issued when that succeeds. The callback URL is computed from the tenant's registered base, never taken from the request, so the route redirects nowhere else.
3. **The tenant's callback** redeems the code in process, checking its tenant, its expiry and the PKCE verifier, and applies the login to the session the flow began in, as [0088](0088-external-identity-providers.md) §6 applies an identity: a login bound to an account on the tenant opens a session for it (§6 for one that is not).

**A login code** is 32 random bytes, base64url, valid for 60 seconds and spent on first use. It is kept as `authc:{code hash}` in Valkey where a shared cache is configured and as a row of `private.auth_code` otherwise, holding the login, the method that authenticated it (with the subject and the provider's username where it was an external issuer), the tenant, the PKCE challenge, the auth host's session and the time of authentication. It never leaves the instance except in the one redirect.

**On click only.** A tenant never sends a reader to the auth host on its own: no page view, frame or script probes the central session, and a reader who never presses "Log in" is never logged in. Pressing it while the auth host's session holds a login completes without a prompt, which is the central login James chose.

**The Action API's door** on a tenant lists the `login` issuer as a `TriplespaceIssuerAuthenticationRequest:login`, with the `REDIRECT` and `*continue` path of [0088](0088-external-identity-providers.md) §5 to the authorization route; it serves a client that drives a browser. A client that does not uses `clientlogin` with a username and password (§11).

**Sessions.** The auth host's session is its own, a `triplespace_session` cookie on the auth host, holding the login and how it was authenticated: the **central session**. A tenant session opened through a code records the login and the central session it came from, so that logging out everywhere can end it (§9); `s:{session}` gains both fields, and `private.session` two columns. A session is still per host, and a tenant's cookie is still nothing on another tenant ([0056](0056-security-model.md) §1); what joins them is a redirect the person starts.

### 6. The first visit to a tenant (amends 0088 §6; amends 0010 §10; amends 0087 §6)

**A login with no account on the tenant** is answered as [0088](0088-external-identity-providers.md) §6 answers an unbound identity, in this order: an account request whose credential is the login answers `ts-account-pending` or `ts-account-rejected`; vouching (§14) runs where `reclaim.vouching` names the issuer that authenticated the login; otherwise the login becomes the session's **pending login**, and the site continues to `Special:CreateAccount` (the API answers `RESTART`).

**Creating the account** is `action=createaccount` with `createpreservestate=1` and a `username`, as for a pending identity: the account is created bound to the login, under the tenant's name rules, throttle and approval, with no password and no challenge (§8). The name field starts empty; the login name is offered as a button, with the warning of [0010](0010-site-ui.md) §10 that the same name makes accounts easy to connect, and so is the username of the external identity that authenticated the login, where one did. Under farm identity with shared names the name is the farm account's (§13).

### 7. What the auth host serves (amends 0012 §4; extends 0047 §3; amends 0010 §10, §11)

**Pages**, in the instance's default skin, with no wiki around them:

- **`Special:UserLogin`**: the login name and password, where the instance default of `login.password` is on, and a button for each external issuer the auth host offers (§10). Reached from a tenant, it names the tenant ("Log in to Librarybase"), offers only the methods the tenant admits, and also accepts that tenant's account name in place of the login name. A login that has an account there continues to the tenant; one that does not is told it will choose a name there. Since the browser applies `form-action` to the redirect that follows the form, the page widens it to that tenant's origin, as it widens it to each issuer's authorization origin ([0088](0088-external-identity-providers.md) §5).
- **`Special:CreateAccount`**: creating a login (§8).
- **`Special:Account`**: the login's own page. Its sections are the login name, with **Rename…**; **Sign-in methods**, which is [0088](0088-external-identity-providers.md) §7's section moved here; **Accounts**, each tenant account the login is bound to, with its tenant, visible to the holder alone; and **Sessions**, with **Log out everywhere**. A tenant's `Special:Account` keeps every other section and links its sign-in methods to this page.
- **`Special:UserLogout`**: logging out of the auth host, and everywhere (§9).

**The Action API at the auth base** offers `query` with `meta=tokens|authmanagerinfo|userinfo|siteinfo`, `clientlogin`, `createaccount` (which creates a login), `linkaccount`, `unlinkaccount`, `changeauthenticationdata`, `removeauthenticationdata` and `logout`, each acting on the login; any other module is unknown there. `meta=userinfo` names the login, not an actor.

**On tenant hosts** with an auth host, `linkaccount`, `unlinkaccount`, `changeauthenticationdata` and `removeauthenticationdata` answer **`ts-auth-host`**, which carries the auth base, and `meta=authmanagerinfo` lists no external issuer and no password field for `create`; the pages that did these link to the auth host. `special-pages.toml` marks each page served at the auth base with `auth = true`, a field `triplespace-titles` does not yet accept (README, Pending allocations).

### 8. Creating a login (amends 0087 §2, §4)

**A login is created on the auth host**, by `Special:CreateAccount` there or `action=createaccount` at the auth base: with a login name and a password, or by a first authentication through an external issuer whose identity is bound to no login, which becomes the auth host's pending identity and asks for a login name as [0088](0088-external-identity-providers.md) §6 asked for an account name. A subject already bound is `ts-oauth-bound`, and a login name in use `ts-login-taken`.

**The challenges move with the password.** The `createaccount` trigger guards creating a login with a password, and `badlogin` guards a password login, on the auth host and in `clientlogin` on a tenant (§11). On the auth host they and the `createaccount` throttle resolve as the instance defaults of their settings ([0085](0085-settings-and-preferences.md) §2.1), since the auth host is no tenant; on a tenant, as the tenant's. The named-account counter of `badlogin` counts by login, so failures on every tenant and on the auth host add up.

**Creating a tenant account from a login is not challenged.** The login was challenged when it was made, or an external issuer vouched for it; the tenant's `createaccount` throttle counts the account, and the tenant's `createaccount` right and `account.approval` decide it, as before.

### 9. Logging out (amends 0056 §1; amends 0014 §4)

**On a tenant**, `Special:UserLogout` and `action=logout` end that tenant's session only; the page offers **Log out everywhere**, a link to the auth host.

**On the auth host**, `Special:UserLogout` ends the central session. **Log out everywhere** (`action=logout&everywhere=1` at the auth base, or the button on `Special:Account`) ends every central session of the login and every tenant session that records one of them: Valkey keeps the set `sc:{central session}` of the sessions each opened, and `private.session` finds them by column.

### 10. One OAuth consumer per issuer (amends 0088 §3, §4, §5; amends 0013 §4)

**A tenant has no OAuth consumer of its own.** `private.oauth_client` holds one row per issuer, `(issuer, client_id, secret, callback, created, updated)`; the `tenant` column and the instance default it stood for are withdrawn, as is the CLI's `--tenant`. The callback the operator registers is `{auth base}/w/rest.php/triplespace/v0/auth/{issuer}/callback`, the auth host's, and `triplespace-cli oauth-client callback {issuer}` prints it. External flows begin and end on the auth host, in its session, with purposes `login`, `create` (a login) and `link`.

**Which issuers are offered.** The auth host offers an external issuer that has `login = true`, an `oauth` table and a credential, and that the instance default of `login.issuers` lists. A tenant's own `login.issuers` and `login.password` now say which methods it **admits**: the auth host asks a person arriving from that tenant to authenticate with one of them, and issues a code only for a central session authenticated by one (§16).

### 11. Tenant hosts keep `clientlogin` and bot passwords (amends 0012 §4)

**`action=clientlogin`** on a tenant still takes `username` and `password`, so that Pywikibot and other clients keep working: the username is the tenant account's name, and the password is checked against the account's login, under the tenant's `badlogin` challenge. It opens a tenant session only; no central session is made.

**Bot passwords** ([0024](0024-subsidiary-accounts.md)) are credentials of a tenant's subsidiaries and stay on tenant hosts, unchanged. So do OAuth tokens ([0025](0025-oauth-server.md)): the consent page is served on the tenant host, which a person reaches logged in through §5.

**`action=createaccount` on a tenant** creates a tenant account from a pending login (§6) or through the `login` issuer's redirect, and no longer from a username and password; that is `ts-auth-host`.

### 12. A single-tenant instance (amends 0056 §10)

**Without `server.auth_base`, the tenant's base is the auth base.** The same code serves it: the tenant host's `Special:UserLogin` is the auth host's, its session is both the central session and the tenant's, the handshake of §5 completes in one request with no redirect, the provider's callback is `{tenant base}/w/rest.php/triplespace/v0/auth/{issuer}/callback`, and the modules of §7 are served there. A login still exists, and the tenant's account is still bound to it, so a second tenant changes nothing but where the person types their password.

**Setting an auth base later** moves the callbacks: `instance check` names every issuer whose registered callback is no longer the auth host's, and the operator registers each again. Sessions on the tenant host stay valid.

### 13. Farm identity (amends 0028 §2)

**Farm identity is identity, not authentication**, as James put it. A farm account binds to a login, `(login, n)` in `accounts/{farm}`, exactly as a tenant account does, and a login has at most one farm account. Tenant accounts under shared names bind to the farm subject as [0028](0028-tenancy-policy.md) §2 has it; the tenant reaches them from the login through the farm account. "The farm has no passwords of its own" becomes: the farm, like every tenant, holds no credential; logins do.

**Farm signup** is on the auth host: a login without a farm account is offered one there, with the linking consent of [0028](0028-tenancy-policy.md) §2 stated on the form. It is not challenged, since creating the login was.

### 14. Moving a tenant and vouching (amends 0018 §10; amends 0088 §8)

**The binding digest is computed over the logins' identities.** A tenant's `accounts` holds `(login, n)` bindings, which mean nothing on another instance, so the digest of [0018](0018-tenants.md) §10 is an HMAC of each external `(issuer, subject)` of the login bound to each account, listed under the account. On the receiving instance, a person whose login holds one of those identities reclaims the account by logging in; passwords never travelled and still do not.

**Vouching** ([0088](0088-external-identity-providers.md) §8) runs when the central session was authenticated through the issuer `reclaim.vouching` names, and checks the subject and provider's username the login code carries, since nothing the provider said is kept beyond the flow; a reclaimed account is bound to the login. A person whose login was authenticated otherwise is asked to log in through that issuer.

### 15. Migration

**Existing credentials become logins once**, by `triplespace-cli logins migrate`, an instance job the upgrade starts, idempotent, whose `--dry-run` prints its plan:

1. **Grouping.** Tenant accounts that share an external identity are one person's, so they are grouped by shared `(issuer, subject)`; each group, and each account with a password and no shared identity, becomes one login.
2. **The password.** A group with one password keeps it. A group with several keeps the primary tenant's account's, or else the oldest account's, and the others are dropped; every member of such a group can still log in by the identity they share.
3. **The name.** A login takes the name of the account whose password it kept, or of its oldest account, and `{name} ({tenant slug})` where that is taken.
4. **The records.** For each account, one change set in its tenant appends the `(login, n)` binding and erases the old password and external binding records with reason class `privacy`; the external identities are rewritten in `logins`, and `private.password` and `private.binding` are re-keyed. These are instance acts, with the job's record in the instance `log` as authority ([08](../architecture/08-tenants-and-instances.md) §8.6).

Sessions survive the migration. `instance create` and `triplespace-cli password set` make and set logins from then on; `password set` takes a login name, or a tenant and account name that it resolves to the login.

### 16. Settings, reporting and errors (uses 0085 §1; amends 0088 §9)

**No new setting.** `login.password` and `login.issuers` keep their scope and defaults, with the meaning of §10: their instance defaults are what the auth host offers, and a tenant's values are what it admits. A tenant that admits nothing the auth host offers has no way to log in, so `ts-no-login-method` also refuses a tenant save, or an instance default, that would leave it so.

**`siprop=triplespace`** reports `auth`, the auth base, and `auth_origin`, its origin, on every tenant; on a single-tenant instance without one, the tenant's own. At the auth base it reports the methods offered.

**Errors:** `ts-auth-base-required`, `ts-auth-host` and `ts-login-taken` are new; a login code that is unknown, expired, spent or for another tenant is `ts-oauth-state`, as a flow is.

### 17. Crates (extends 0005 §2)

| Crate | Change |
|---|---|
| `triplespace-accounts` | Logins, their names and records; passwords and external bindings keyed by login; the authorization route's handle and login codes; tenant account bindings to logins and the farm's; central sessions and logging out everywhere; `logins migrate` |
| `triplespace-api-action` | The auth base's module set; `ts-auth-host` on tenants; the `login` issuer request; `clientlogin` resolving a tenant account to its login; `logout&everywhere` |
| `triplespace-api-rest` | `GET /auth/authorize` and the auth host's flows; `/auth/login/start` and `/auth/login/callback` on tenants |
| `triplespace-ui` | The auth host's four pages; the tenant's "Log in" button and its `form-action`; the login sections moved off the tenant's `Special:Account` |
| `triplespace-server` | `server.auth_base`, the registered-host check and the auth base's routes; refusing a second tenant without it |
| `triplespace-cli` | `logins migrate`; `password set` by login; `oauth-client` without `--tenant` |
| `triplespace-db` | `private.login` and `private.auth_code`; `private.password` and `private.binding` re-keyed; `private.oauth_client` without `tenant`; the session columns |
| `scatter-actors` | The `login` issuer and `scatter:v0/login` |

## Alternatives considered

- **A login per tenant, joined by a central session.** Keeps passwords on tenant accounts and lets the auth host vouch only for the tenant you already chose. It cannot ask for a password without knowing the tenant first, and a person still has a password per wiki. James chose instance logins.
- **The farm account as the login.** Farm accounts are public actors that exist only under farm identity; logins must exist on every instance and must not be public. James was explicit that the mechanism of logging in is separate from whether identity is per tenant or farm-wide.
- **A per-tenant OAuth consumer.** [0088](0088-external-identity-providers.md) §3 offered it for tenants on other hosts; with one callback on the auth host it buys nothing, and James was disinclined to support it.
- **Silent sign-in**, where a tenant checks the auth host on every page view by redirect or frame. It would log a reader in without their asking, cost a round trip on every first view, and need third-party cookies or a redirect on every page. James chose "on click".
- **The auth host as an OpenID Connect provider over HTTP.** The tenant and the auth host are one process with one database, so the exchange happens in process; the flow keeps OAuth's shape (state, PKCE, a short-lived code) so that a later split costs only the transport.
- **Logins kept only in `private`.** Passwords are, but bindings were already records so that erasure is logged; logins keep that.

## Consequences

- **One consumer per provider for the whole instance**, registered once with the auth host's callback, whatever the number of tenants and hosts.
- **One password per person per instance**, and one place to type it; a tenant host never sees a password typed into a form, though `clientlogin` still takes one.
- **Accounts stay per tenant.** Attribution, groups, blocks and renames are untouched; a login binds accounts without linking them.
- **A tenant cannot hide which login holds an account from the instance**, which it never could; it remains hidden from everyone else.
- **Tenant hosts lose the API modules for managing credentials**, which clients that managed passwords through the API must call at the auth base.
- **0088's tenant-level credentials and bindings are rewritten** by the migration, and its per-tenant consumer rows are dropped.

## Open questions

- **Q1. Who may create a login.** Every login is challenged, but an instance of private or closed wikis may want to close login creation itself, or require approval of logins as tenants approve accounts.
- **Q2. Deleting a login.** What becomes of the tenant accounts bound to it: unbound and reclaimable, as after a move, or refusal while any remains.
- **Q3. The farm base.** Whether a farm base that is no tenant's logs in through the auth host, and which account its session holds: the primary tenant's account of the login, which instance acts need ([0046](0046-primary-tenant.md) §6).
- **Q4. A login's data.** What the user data bundle of [0027](0027-preferences-and-portability.md) §7 carries of a login, which no tenant owns.
- **Q5. Moving an account to another login**, for a person who ends up with two logins after the migration.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §17 | extends | 0005 A100 |
| [0007](0007-actor-identity.md) §3 | §2, §3, §4 | amends | 0007 A26 |
| [0010](0010-site-ui.md) §10 | §6, §7 | amends | 0010 A45 |
| [0010](0010-site-ui.md) §11 | §7 | amends | 0010 A45 |
| [0012](0012-api-requirements.md) §4 | §7, §11 | amends | 0012 A79 |
| [0012](0012-api-requirements.md) §5 | §5 | extends | 0012 A79 |
| [0013](0013-postgres-storage.md) §4 | §2, §3, §5, §10 | amends | 0013 A67 |
| [0014](0014-caches-and-search.md) §4 | §5, §9 | amends | 0014 A29 |
| [0015](0015-record-format-and-partition-registry.md) §5 | §2 | extends | 0015 A61 |
| [0018](0018-tenants.md) §4 | §2 | amends | 0018 A32 |
| [0018](0018-tenants.md) §10 | §14 | amends | 0018 A32 |
| [0028](0028-tenancy-policy.md) §2 | §4, §13 | amends | 0028 A23 |
| [0033](0033-backend-stack.md) §12 | §1 | extends | 0033 A21 |
| [0047](0047-special-pages.md) §3 | §7 | extends | 0047 A24 |
| [0056](0056-security-model.md) §1 | §5, §9 | amends | 0056 A17 |
| [0056](0056-security-model.md) §10 | §1, §12 | amends | 0056 A17 |
| [0057](0057-web-tier.md) §3 | §1 | extends | 0057 A8 |
| [0087](0087-account-registration-and-challenges.md) §2 | §8 | amends | 0087 A3 |
| [0087](0087-account-registration-and-challenges.md) §4 | §8 | amends | 0087 A3 |
| [0087](0087-account-registration-and-challenges.md) §6 | §6 | amends | 0087 A3 |
| [0088](0088-external-identity-providers.md) §3 | §10 | amends | 0088 A1 |
| [0088](0088-external-identity-providers.md) §4 | §10 | amends | 0088 A1 |
| [0088](0088-external-identity-providers.md) §5 | §5, §10 | amends | 0088 A1 |
| [0088](0088-external-identity-providers.md) §6 | §4, §6 | amends | 0088 A1 |
| [0088](0088-external-identity-providers.md) §7 | §3 | amends | 0088 A1 |
| [0088](0088-external-identity-providers.md) §8 | §14 | amends | 0088 A1 |
| [0088](0088-external-identity-providers.md) §9 | §16 | amends | 0088 A1 |

## References

- [Wikimedia's SUL3 and `auth.wikimedia.org`](https://www.mediawiki.org/wiki/MediaWiki_Platform_Team/SUL3)
- [RFC 7636: Proof Key for Code Exchange](https://www.rfc-editor.org/rfc/rfc7636)
- [MediaWiki's AuthManager](https://www.mediawiki.org/wiki/Manual:SessionManager_and_AuthManager)
