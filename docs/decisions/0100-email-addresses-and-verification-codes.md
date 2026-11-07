# 0100. Email addresses and verification codes

- **Status:** Proposed
- **Date:** 2026-11-05
- **Updated:** 2026-11-06 (A3)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0021](0021-notifications.md), [0027](0027-preferences-and-portability.md), [0089](0089-the-auth-host-and-instance-logins.md)
- **Uses:** [0012](0012-api-requirements.md), [0085](0085-settings-and-preferences.md), [0088](0088-external-identity-providers.md)

## Context

[0021](0021-notifications.md) §4 made email one of the three channels of notifications, delivered to "an address held in `private` and verified by a token link", and §8 gave it a row of `private.email` keyed by actor, a route pair `PUT`/`DELETE /notifications/email`, and a place under Contacts on each tenant's `Special:Account`. None of it was built: the instance had no way to send mail at all, no transport, no sender address and no delivery queue.

[0089](0089-the-auth-host-and-instance-logins.md) has since moved authentication off tenant accounts and onto **instance logins** on the auth host: passwords, external sign-in methods and sessions belong to the login, and a person with accounts on three tenants has one login. An address keyed by tenant actor would have to be verified once per tenant, and the security mail that sign-in methods will need (TOTP, password reset, a new sign-in) would have no single address to go to.

Two things were wanted before anything else that involves email: a way to give the instance an address and prove it is yours, and a way for the instance to send mail that a developer can watch without a mail server. SCA-279 asks for both, as the ground for email notifications and the security notices TOTP will need.

A verification link has a known weakness: mail scanners, link-preview services and some clients fetch every URL in a message, so a link can be "clicked" by a machine before the person reads it.

### Direction

James's direction, from the design discussion of 2026-11-05:

- **"Let's work out a basic system that lets you associate an email address with an account, then the site emails you asking for confirmation, and then you can enter a six-digit code to verify the email address. (I like this better than a link since links can be clicked on automatically.) This is the basic necessary system before adding other things that involve email."**
- **"What if they were staged in the dev log sink and then an email-sending trait that can plug into either SMTP as a baseline or HTTP mail API?"**
- **"Allow verification of multiple email addresses. One of them is the primary. The primary can't be removed until a new primary is designated."**
- **"In order to have no email at all, you need to remove all OAuth connections. This removes the restriction from removing your primary email address."**
- On the questions put to him, he chose: the address belongs to the **login**, not to each tenant account; a code valid for **one hour**, spent by **five** wrong entries; notification mail goes **always to the primary**; security notices go to **every verified address**; and this work builds the trait with the **log** sink and **SMTP**, leaving each HTTP mail API to later work.
- On the draft's open questions, of the OAuth connections: **"For Q1, external sign-in methods was actually the main use case I had in mind, being able to sign in to your account without having to set a password. Oh yeah, that's the other condition to removing all your email addresses: you have to have a password set, since you won't have the option to log in with OAuth."**
- Of the limit and shared addresses: **"Q4, I would put the limit at 3. If an email address is used on several logins, it can't be used for logging in, which is an issue if we want to enable logging in by email address, which is kind of what logging in via external provider is."**
- **"Also, usernames cannot be in the form of an email address, to avoid confusion."**
- On whether a login with an external sign-in method must have an address: **"As I understand, having an email address is not strictly required to have a Triplespace account (though some instances may want to mandate it), so warning people that their account will be unrecoverable if they lose their password should be enough."**
- On the HTTP mail APIs, he accepted the recommendation put to him: Postmark first, built together with bounce and complaint handling, and SMTP to any provider until then.
- **"An instance may require email. It is enforced at account creation and then they are prevented from having <1 email addresses. There are no existing production instances so I am not very concerned about existing logins with no addresses unless you mean something else. It should be a sender address per instance, but the display names can be different depending on whether it's an instance-related notification or a wiki (tenant) related notification."**

## Decision

### 1. An address belongs to the login (amends 0021 §4, §8; extends 0089 §2)

*Changed by A1, A2, A3.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.9.*

### 2. Verification by a six-digit code (amends 0021 §4)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.9, §6.2; [16](../architecture/16-logs-feeds-and-notifications.md) §5.4.*

### 3. Several addresses, one primary (extends 0089 §2)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.9.*

### 4. Security notices (extends 0021 §4)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.9.*

### 5. Staging and sending mail (amends 0021 §8)

*Changed by A2.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §5.7; [23](../architecture/23-configuration-and-registry.md) §3.6.*

### 6. UI (amends 0021 §6; extends 0089 §7)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.8; [19](../architecture/19-site-ui.md) §2.4.*

### 7. API (amends 0021 §7)

*Changed by A1, A2, A3.*

*Current text: [18](../architecture/18-api.md) §3.2.*

### 8. Storage (amends 0021 §8)

*Changed by A2.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.16, §5.*

### 9. Portability (amends 0027 §2, §3)

*Current text: [07](../architecture/07-actors-and-accounts.md) §8.3, §8.4.*

### 10. Settings (uses 0085 §1)

*Changed by A2.*

*Current text: [23](../architecture/23-configuration-and-registry.md) §3.3.*

### 11. Crates (extends 0005 §2)

*Changed by A2.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **A verification link**, as 0021 §4 had it. One click for the person, but a scanner or preview that fetches every URL verifies the address for them, and anyone who sees the message can use the link. James chose a code entered in a session of the login.
- **An address per tenant account**, as 0021 §8 had it. Verified once per tenant, with nowhere to send mail about the login itself. James chose the login.
- **One address per login**, replaced in place. Simpler, but a change of address is then a moment with no verified address, and a session thief who replaces it cuts the holder off. James chose several verified addresses with one primary.
- **Sending inline, in the request.** No queue to run, but a slow or failing relay holds up the page and a failure loses the message. Every message is staged in `ops.delivery` instead, which 0021 §8 had already planned for the channel.
- **One address on several logins**, with nothing revealing it. Simpler, but an address could then never name a login, which signing in by email address needs. James chose one login per verified address.
- **Amazon SES's HTTP API first.** The cheapest per message, but its API needs AWS request signing and its bounces arrive through SNS; SES is reached through `smtp` instead. Postmark's API is one JSON request and its bounces one webhook.
- **An HTTP mail API as the baseline.** Simpler to operate for some hosts, but ties a deployment to a provider; SMTP is what every provider and every self-hosted relay accepts, and HTTP APIs plug into the same trait.

## Consequences

- **A person verifies an address once per instance**, however many tenants they have accounts on, and every tenant account bound to the login is reachable by email through it.
- **No tenant learns a person's address.** The tenant's account page links to the auth host instead of showing it.
- **Development and the e2e suite need no mail server**: the `log` transport writes each message where a test can read the code.
- **An instance that configures no transport sends nothing**, and says so where addresses are managed; nothing else changes.
- **An address that bounces or complains stops receiving mail** until its holder confirms it again, so a dead or hostile mailbox costs the instance its sender reputation once, not on every digest (A2).
- **Email addresses leave the user data bundle**, since they belong to the login and no tenant owns them; a tenant move or an import no longer carries them.
- **An address is optional unless the instance requires one.** A login without one is warned that it cannot be recovered, and is otherwise unaffected; under `email.required` no login made by signing up is ever without one.
- **One sender address serves every tenant**, so an instance configures and authenticates (SPF, DKIM) one domain; the display name tells instance mail from a wiki's.
- **A verified address names one login**, which keeps signing in by email address open as a later sign-in method.
- **A login that signs in only through an external issuer cannot drop its last address**, so the instance can always reach a person who has no password.
- **Clients see no address.** Neither `/oauth/identify` nor `meta=userinfo` reports one, as before.

## Open questions

None.

## Amendment log

### A1. The login an address belongs to, until instance logins exist

- **Date:** 2026-11-06
- **Source:** Direct: Claude, while building the email addresses of SCA-279, 2026-11-06; unconfirmed
- **Change:** extends §1, §7
- **Summary:** [0089](0089-the-auth-host-and-instance-logins.md) §2's instance logins and `private.login` are not built, so an address cannot yet be keyed by a login ID. `private.email.login` holds the login's identifier as text, and until logins exist that identifier is the actor key of the account whose session adds the address; on a single-tenant instance, where the tenant's base is the auth base, that is one login per person. The `/auth/email` routes and the Email addresses section are served on the tenant's host, as `GET /account` is, rather than at a separate auth base, and a request by API key is refused (`permissiondenied`): addresses are managed in a session of the login. The migration of 0089 §15 rekeys the rows from actor keys to login IDs when it is built. The alternative, building logins first, would have put the email work behind a larger one that nothing else in SCA-279 needs.

### A2. The Postmark transport, built with bounce and complaint handling

- **Date:** 2026-11-06
- **Source:** Direct: Claude, while building SCA-296, 2026-11-06; unconfirmed
- **Change:** amends §5; extends §1, §7, §8, §10, §11
- **Summary:** The first HTTP mail API is built, as §5 planned: `mail.transport = postmark` sends one JSON request per message with the server token from `mail.postmark.token_file`, through `reqwest` (a new edge of `scatter-mail`); `200` is accepted, `429` and `5xx` temporary, `422` with an API error code and `401`, `413`, `415` permanent. Codes and security notices go on `mail.postmark.stream` (`outbound`); notifications and digests on `mail.postmark.notification_stream` (`broadcast`), a broadcast stream the operator creates, because Postmark's terms keep bulk mail off the transactional stream. Postmark's bounce and spam-complaint webhooks post to `POST /mail/postmark`, authenticated by the HTTP Basic password `mail.postmark.webhook_secret_file` holds, since Postmark signs nothing. **What a bounce does to an address**, the decision SCA-296 asked for: a hard bounce or a complaint marks the address *undeliverable* on every login that holds it verified, with the reason and time (two columns of `private.email`); the address keeps its verification and its primary role, nothing more is sent to it, and the page says so beside it. The holder clears the mark by confirming the address again with a new code; after a complaint the page suggests removing it, since Postmark will refuse it for good. This was chosen over removing the address (a lost primary, with no word to the holder) and over a warning alone (every digest would then cost sender reputation); it is reversible by the holder and needs no operator. A transient or soft bounce changes nothing. `ops.delivery` gained a `failed` flag beside the attempts, so a permanent failure keeps its row and error until expiry without being retried.

Replaced text ([16](../architecture/16-logs-feeds-and-notifications.md) §5.7, as it stood):

> HTTP mail APIs are further implementations of the trait, each with a transport name of its own and sending through the egress path of [0012](0012-api-requirements.md) §6. What one adds over SMTP is structured delivery results and bounce and complaint webhooks, so each is built together with that handling. The first is `postmark`, whose message streams keep codes and security notices apart from notification digests; until it is built, a provider is reached through `smtp`, which every one of them accepts.
>
> **The sender** is one address for the whole instance, `mail.from`, required unless the transport is `none`. Its display name says whom a message is from: the instance's display name (the operator's, [08](../architecture/08-tenants-and-instances.md) §8.2) on mail about the login, codes and security notices, and the tenant's `site.name` on a tenant's notifications and digests. Bounces and complaints are not processed yet.
### A3. Addresses keyed by the login

- **Date:** 2026-11-06
- **Source:** Direct: Claude, while building instance logins, 2026-11-06; unconfirmed
- **Change:** amends §1, §7 (A1)
- **Summary:** Instance logins exist ([0089](0089-the-auth-host-and-instance-logins.md) A3), so an address's row names the login by its ID, as §1 always said, and A1's interim keying by actor key is over; the `login` column stays text and holds the decimal ID. What A1 said of where the routes are served stands for the reason 0089 §12 gives: on a single-tenant instance the tenant's base is the auth base. A request by API key is still refused.

Replaced text ([07](../architecture/07-actors-and-accounts.md) §2.9, as it stood):

> Until instance logins exist (§2.8 is not built), the login an address belongs to is the account whose session acts, identified by its actor key, and the routes and the page are served on the tenant's host as the account page is; the migration to logins rekeys the rows (0100 A1).

## References

- [RFC 5322: Internet Message Format](https://www.rfc-editor.org/rfc/rfc5322)
- [RFC 3834: Recommendations for Automatic Responses to Electronic Mail](https://www.rfc-editor.org/rfc/rfc3834) (`Auto-Submitted`)
- [RFC 8314: Cleartext Considered Obsolete](https://www.rfc-editor.org/rfc/rfc8314) (implicit TLS for submission)
- [`lettre`](https://lettre.rs/)
- [MediaWiki's `Special:ChangeEmail` and email confirmation](https://www.mediawiki.org/wiki/Manual:$wgEmailAuthentication)
