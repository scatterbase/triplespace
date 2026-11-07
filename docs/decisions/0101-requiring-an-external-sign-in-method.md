# 0101. Requiring an external sign-in method

- **Status:** Accepted
- **Date:** 2026-11-06
- **Author:** James Hare / Claude Fable
- **Changes:** [0088](0088-external-identity-providers.md), [0089](0089-the-auth-host-and-instance-logins.md)
- **Uses:** [0085](0085-settings-and-preferences.md), [0087](0087-account-registration-and-challenges.md), [0100](0100-email-addresses-and-verification-codes.md)

## Context

[0088](0088-external-identity-providers.md) made the instance an OAuth 2.0 client of external identity providers, Wikimedia's central accounts first, and [0089](0089-the-auth-host-and-instance-logins.md) moved every sign-in method onto the instance login: a login is authenticated by a password ([07](../architecture/07-actors-and-accounts.md) §2.2), by external identities ([07](../architecture/07-actors-and-accounts.md) §2.7), or both. Both are built on a single-tenant instance, where the tenant's base is the auth base: the flow with PKCE and `state`, the first login's pending identity, adding and removing sign-in methods, and `ts-last-binding`, which keeps a login from removing its only way in.

What a login *must* hold was left to the person. `login.password` says whether the auth host offers passwords at all, and `ts-no-login-method` refuses a save that would leave a tenant with no method; but nothing lets an instance say that its people authenticate elsewhere, with the instance's own password only the bootstrap that `instance create` needs before any provider is registered. [0100](0100-email-addresses-and-verification-codes.md) §10 gave the shape of such a rule for email addresses: `email.required`, enforced at creation and as a floor of one thereafter.

### Direction

James's direction, from the design discussion of 2026-11-06:

- **"I would like to implement the ability to log in with Wikimedia via OAuth. It should also be possible for an instance to require logging in via an external ID provider. It cannot enact this requirement until every account has an ID provider associated with it, and once enacted, users must have a minimum of one ID provider. The idea is to have instances that rely on other services for authentication, with the password only used for bootstrapping."**

## Decision

### 1. Every login holds an external identity (amends 0088 §7; amends 0089 §8; extends 0089 §2)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7, §2.8.*

### 2. Enacting the requirement (extends 0088 §4; extends 0089 §15)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7, §2.8.*

### 3. The pages (extends 0089 §7)

*Current text: [19](../architecture/19-site-ui.md) §2.3.*

### 4. Settings, reporting and errors (uses 0085 §1; uses 0100 §10)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7.*

## Alternatives considered

- **Turning passwords off instead.** `login.password = false` already exists, and with every issuer offered it leaves a tenant admitting external methods only. But it says nothing about what a login holds: a login made with a password before the switch keeps nothing else and is locked out, and a login that lost its only identity has no way back. The requirement is about the login's bindings, and the two settings compose: an instance that wants external sign-in only sets both.
- **Enacting at once and warning the logins without an identity**, as `email.required` does. James asked for the precondition instead: a requirement that locks people out is not one an operator should be able to enact by mistake, and an instance with no production logins yet loses nothing by binding them first.
- **A per-tenant requirement.** A login is the instance's, and so are its bindings; a tenant that wants only external logins narrows `login.password` and `login.issuers`, which say what it admits, and needs no rule of its own about bindings.
- **Removing passwords when the requirement is enacted.** A password someone set is theirs; the requirement adds a floor and takes nothing away. Whether it is offered is `login.password`'s question.

## Consequences

- **An instance can rely on other services for authentication.** With `login.external_required` on and `login.password` off, no one authenticates to the instance with a secret the instance holds; the password is the bootstrap `instance create` needs and nothing more.
- **Nobody is locked out by enacting it.** The save is refused until every login has an identity, so the operator binds the first administrator's before the switch, and a login never loses its last identity afterwards.
- **A login may keep a password as a fallback** for the day its provider is down or its identity is lost, where `login.password` still offers it.
- **A provider whose credential is removed strands its logins** that hold no other identity and no offered password, as it did before; `oauth-client remove` is the operator's act, and Q2 asks whether it should say so.
- **Creating accounts for others with a password is refused** on such an instance, since it would make a login with no identity; an administrator invites the person to sign in instead.

## Open questions

- **Q1. Passwords for the owners only.** Whether an instance under the requirement should be able to keep passwords as a break-glass method for members of `owner` while offering them to no one else, which `login.password` cannot express.
- **Q2. Withdrawing a provider.** Whether `oauth-client remove` and a save of `login.issuers` that stops offering an issuer should refuse, or warn, while a login holds that issuer's identity and no other under the requirement.
- **Q3. A required issuer.** Whether an instance may name the issuer, or issuers, every login must hold, rather than any one the auth host offers.

## References

- [07](../architecture/07-actors-and-accounts.md) §2.7, §2.8
- [19](../architecture/19-site-ui.md) §2.3
- [23](../architecture/23-configuration-and-registry.md) §3.3
