# 0089. The auth host and instance logins

- **Status:** Proposed
- **Date:** 2026-10-18
- **Updated:** 2026-10-30 (A1)
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

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.8; [20](../architecture/20-web-tier.md) §2.1, §5.3; [23](../architecture/23-configuration-and-registry.md) §3.6.*

### 2. Instance logins (amends 0007 §3; amends 0018 §4; extends 0015 §5; amends 0013 §4)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.2; [03](../architecture/03-storage-caches-and-search.md) §4.16, §12.1; [07](../architecture/07-actors-and-accounts.md) §2.1, §2.8; [08](../architecture/08-tenants-and-instances.md) §3.2.*

### 3. Sign-in methods belong to the login (amends 0088 §7; amends 0007 §3; amends 0013 §4)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.1, §2.2, §2.7, §2.8.*

### 4. Tenant accounts bind to a login (amends 0007 §3; amends 0088 §6; amends 0028 §2)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.1, §2.8; [08](../architecture/08-tenants-and-instances.md) §3.2.*

### 5. Logging in on a tenant (amends 0088 §5; amends 0056 §1; extends 0012 §5; amends 0013 §4; amends 0014 §4)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.16, §12.1; [07](../architecture/07-actors-and-accounts.md) §2.4, §2.8; [09](../architecture/09-security-and-moderation.md) §1.4; [18](../architecture/18-api.md) §3.2; [19](../architecture/19-site-ui.md) §2.3.*

### 6. The first visit to a tenant (amends 0088 §6; amends 0010 §10; amends 0087 §6)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.6, §2.7, §2.8; [19](../architecture/19-site-ui.md) §2.3.*

### 7. What the auth host serves (amends 0012 §4; extends 0047 §3; amends 0010 §10, §11)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.8; [18](../architecture/18-api.md) §2.3; [19](../architecture/19-site-ui.md) §2.3, §2.4; [21](../architecture/21-special-pages.md) §1.3.*

### 8. Creating a login (amends 0087 §2, §4)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.6; [19](../architecture/19-site-ui.md) §2.3.*

### 9. Logging out (amends 0056 §1; amends 0014 §4)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.16, §12.1; [07](../architecture/07-actors-and-accounts.md) §2.4, §2.8; [09](../architecture/09-security-and-moderation.md) §1.4; [18](../architecture/18-api.md) §2.3.*

### 10. One OAuth consumer per issuer (amends 0088 §3, §4, §5; amends 0013 §4)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7; [18](../architecture/18-api.md) §3.2.*

### 11. Tenant hosts keep `clientlogin` and bot passwords (amends 0012 §4)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.8; [18](../architecture/18-api.md) §2.3.*

### 12. A single-tenant instance (amends 0056 §10)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.8; [20](../architecture/20-web-tier.md) §5.3.*

### 13. Farm identity (amends 0028 §2)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.8; [08](../architecture/08-tenants-and-instances.md) §3.5, §3.6; [19](../architecture/19-site-ui.md) §2.3.*

### 14. Moving a tenant and vouching (amends 0018 §10; amends 0088 §8)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.5; [08](../architecture/08-tenants-and-instances.md) §6.2, §6.4.*

### 15. Migration

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.8.*

### 16. Settings, reporting and errors (uses 0085 §1; amends 0088 §9)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.8; [18](../architecture/18-api.md) §2.2.*

### 17. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

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

## References

- [Wikimedia's SUL3 and `auth.wikimedia.org`](https://www.mediawiki.org/wiki/MediaWiki_Platform_Team/SUL3)
- [RFC 7636: Proof Key for Code Exchange](https://www.rfc-editor.org/rfc/rfc7636)
- [MediaWiki's AuthManager](https://www.mediawiki.org/wiki/Manual:SessionManager_and_AuthManager)

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§17
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [20](../architecture/20-web-tier.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
