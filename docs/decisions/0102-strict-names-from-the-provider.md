# 0102. Strict names: the provider's username is the account's name

- **Status:** Accepted
- **Date:** 2026-11-06
- **Updated:** 2026-11-07 (A2)
- **Author:** James Hare / Claude Fable
- **Changes:** [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0024](0024-subsidiary-accounts.md), [0087](0087-account-registration-and-challenges.md), [0088](0088-external-identity-providers.md), [0089](0089-the-auth-host-and-instance-logins.md)
- **Uses:** [0085](0085-settings-and-preferences.md), [0101](0101-requiring-an-external-sign-in-method.md)

## Context

[0089](0089-the-auth-host-and-instance-logins.md) §2 uncoupled authentication from identity: a person authenticates as an instance login, a tenant account binds to it, and the account's public name is chosen on the first visit, with the provider's username offered only as a suggestion and a warning that the same name makes accounts easy to connect ([19](../architecture/19-site-ui.md) §2.3). [0101](0101-requiring-an-external-sign-in-method.md) then let an instance require every login to hold an external identity, so that an instance can rely on another service for authentication.

An instance that relies on Wikimedia for who its people are may want to rely on it for what they are called as well: one name per person, the same on every tenant and the same as upstream, with no second namespace to police. [0007](0007-actor-identity.md) §4 keeps names as attributes of actor records, renameable and tombstoned, and [0010](0010-site-ui.md) §10 never reuses a name, so that `[[User:Name]]` in old text can never come to mean someone else. Subsidiary accounts ([0024](0024-subsidiary-accounts.md) §2) share the namespace with people. A MediaWiki username is at most 235 bytes at creation (`$wgMaxNameChars`, less the room a `User_talk:` subpage needs), under the same grammar as a local name.

### Direction

James's direction, from the design discussion of 2026-11-06, after 0101 was built:

- **"Would it be possible for them to be required to use the same usernames? In the event of multiple providers, there can be a slug prefix. This wouldn't be a problem with subsidiaries as the underlying bot accounts would not organically log in through OAuth and the slug prefix disambiguates them anyway. This is a strict mode that would be an alternative to the default that identity and auth are uncoupled."**
- On the four choices put to him: **"In strict mode, the assumption is that upstream is canonical on what a username means, so that means a username can be reassigned. At the end of the day, users are actually ID numbers with prefix slugs. I think one identity per login is consistent with the planned use case."** On the length of a prefixed name, he asked for Wikimedia's limit, which settles it (§2).

## Decision

### 1. The provider's username is the account's name (amends 0089 §6; extends 0087 §5; uses 0101 §1)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.8, §2.10; [08](../architecture/08-tenants-and-instances.md) §3.2; [18](../architecture/18-api.md) §2.3; [19](../architecture/19-site-ui.md) §2.3.*

### 2. The prefix is always there, and reserved (extends 0007 §4; extends 0024 §2)

*Changed by A1.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.10.*

### 3. Upstream is canonical (amends 0010 §10; extends 0007 §4)

*Changed by A1, A2.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.10; [19](../architecture/19-site-ui.md) §2.3.*

### 4. One identity per login (amends 0088 §7)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.7, §2.10; [18](../architecture/18-api.md) §2.3.*

### 5. Enacting strict names (uses 0101 §2)

*Changed by A1, A2.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.10.*

### 6. Settings, reporting and errors (uses 0085 §1)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.10; [23](../architecture/23-configuration-and-registry.md) §3.3.*

## Alternatives considered

- **A prefix only once a second issuer is offered.** Shorter names on the common single-provider instance, but offering a second issuer would then rename every account, and a subsidiary could sit on a name a provider username produces. James asked for the prefix as the disambiguator; making it unconditional is what lets it be reserved.
- **Keeping the tombstone against a reassigned upstream name.** Safer for old links, but it would lock the new holder of the name out of an instance whose whole premise is that upstream decides. James chose upstream as canonical.
- **A per-tenant mode.** Names are a tenant's, but the identity they derive from is the login's, and the prefix must be reserved across the instance for the derivation to be unambiguous; a tenant that does not want provider names does not join such an instance.
- **Several identities with one designated as the name's source.** More flexible, and more to explain; the planned use case has one provider per person.

## Consequences

- **An instance can delegate names as well as authentication.** A person is called on every tenant what they are called upstream, and nobody chooses, renames or reserves a name here.
- **Accounts are publicly connectable across tenants and to the provider.** That is the trade; the default mode keeps the warning and the choice.
- **A bot is told from a person by its name.** Under strict names every person's name carries an issuer's prefix and no subsidiary's may, so the namespace separates itself.
- **Names can be stale between logins**, since the instance learns a rename only when the person next authenticates.
- **Old links can change meaning**, exactly as they do upstream, when a provider reassigns a name; the tombstone yields only to the issuer that named it.
- **A second provider is a visible change**: its users arrive with a different prefix, and nothing already named moves.

## Open questions

- **Q1.** ~~**A rename by hand.** Whether `renameuser` is refused under strict names, or allowed and overwritten at the next login; refusing is simpler and is what "nobody chooses a name" implies.~~ *Settled by A2: refused, with `ts-strict-name`; unconfirmed.*
- **Q2. Farm accounts.** What a farm account ([08](../architecture/08-tenants-and-instances.md) §3.5) is named under strict names: the same derived name is the natural answer, and the farm name registry then holds one name per login.
- **Q3. A provider's username grammar.** Wikimedia's follows MediaWiki's, which the local grammar matches; a later provider ([0088](0088-external-identity-providers.md) Q1) may allow what the local grammar forbids, and would need a mapping or a refusal.
- **Q4. The stale window.** Whether an instance may ask the provider's users API for the current name on a schedule, so that renames land before the next login, at the cost of a request per account.

## References

- [07](../architecture/07-actors-and-accounts.md) §2.7, §2.10
- [19](../architecture/19-site-ui.md) §2.3
- [23](../architecture/23-configuration-and-registry.md) §3.3
- [Manual:$wgMaxNameChars](https://www.mediawiki.org/wiki/Manual:$wgMaxNameChars)

## Amendment log

### A1. The username kept on the identity; every registry prefix reserved; a held derived name

- **Date:** 2026-11-07
- **Source:** Direct: Claude, while building strict names, 2026-11-07; unconfirmed
- **Change:** amends §2; extends §3, §5
- **Summary:** Three calls made while building. (1) The username a provider's profile gives is kept on the identity's binding row (`private.binding.username`), refreshed at every login and set when an identity is bound, so that `logins check` and the save that enacts `login.names = provider` compute every account's derived name without asking the provider; an account whose identity has not authenticated since the column existed has no known derived name and blocks enactment until its holder signs in once. (2) The reserved prefixes are those of every issuer with an OAuth table in `issuers.toml`, not only of the issuers the auth host offers: the check is then static and instance-wide, and an issuer offered later finds its names free, at the cost of reserving a few prefixes no one here will use. (3) When the provider renames a person to a name another account on a tenant already holds, that account is left as it is and `logins check` lists it; the design does not say what should happen, and the reservation of prefixes makes the case rare.

Replaced text ([07](../architecture/07-actors-and-accounts.md) §2.10, as it stood):

> Every offered issuer's prefix is **reserved**: no hand-made name may begin with it, whether a subsidiary's (§4.2), one the CLI creates, or a chosen name on an instance that later turns strict names on (`ts-name-reserved`, as for a tombstoned name).

### A2. A rename by hand; the tombstone that yields

- **Date:** 2026-11-07
- **Source:** Direct: Claude, while building name tombstones and `renameuser`, 2026-11-07; unconfirmed
- **Change:** extends §3, §5
- **Summary:** Three calls made while building ([07](../architecture/07-actors-and-accounts.md) §2.10; [19](../architecture/19-site-ui.md) §2.3). (1) Q1: `renameuser` is refused under strict names with `ts-strict-name`, as "nobody chooses a name" implies, rather than allowed and overwritten at the next login. (2) With names chosen, `renameuser` may give an account the one prefixed name that is its own derived name, the name its login's identity username derives, which is how a bureaucrat renames the differing accounts that §5 lists before enacting the mode; every other prefixed name stays reserved (§2). (3) The tombstone a released name leaves remembers the issuer whose username it was derived from ([0010](0010-site-ui.md) A53), and the yield of §3 applies to that tombstone alone: a derived name whose tombstone names no issuer, or another, is refused like any other, and when the provider's rename lands on such a name the account keeps its name and `logins check` lists it, as it does for a name an account holds (A1).
