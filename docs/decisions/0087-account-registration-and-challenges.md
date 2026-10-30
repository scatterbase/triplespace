# 0087. Account registration and challenges

- **Status:** Proposed
- **Date:** 2026-10-17
- **Updated:** 2026-10-30 (A5)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0024](0024-subsidiary-accounts.md), [0028](0028-tenancy-policy.md), [0047](0047-special-pages.md), [0057](0057-web-tier.md)
- **Uses:** [0025](0025-oauth-server.md), [0027](0027-preferences-and-portability.md), [0030](0030-edit-filters.md), [0034](0034-frontend-stack.md), [0035](0035-adopting-a-wikibase.md), [0056](0056-security-model.md), [0085](0085-settings-and-preferences.md)

## Context

Self-registration was decided and not built. `createaccount` governs self-registration and creating an account for another, as in MediaWiki, with `universe` as its default group, and a private tenant takes it from `universe` ([0024](0024-subsidiary-accounts.md) A13; [0056](0056-security-model.md) A10). `action=createaccount` and `meta=authmanagerinfo` are in the Action API's scope with MediaWiki's `username` and `password` fields where the built-in `password` issuer is on ([0012](0012-api-requirements.md) §4; [18](../architecture/18-api.md) §2.3), `Special:CreateAccount` is where a first login chooses its name ([0010](0010-site-ui.md) §10), and creating an account works without JavaScript ([0057](0057-web-tier.md) §7). The code issues the `createaccount` token and nothing more. `docs/registry/groups.toml` still gives `createaccount` to `autoconfirmed`, the default A13 retired.

Nothing stood between a script and that form. A MediaWiki wiki that opens registration installs ConfirmEdit, whose CAPTCHAs are triggered by account creation, by failed logins and by edits that add links, and which exempts holders of `skipcaptcha` (`bot` and `sysop` by default). A wiki that wants each account reviewed installs ConfirmAccount, which takes `createaccount` from `*` and `user`, serves a request form at `Special:RequestAccount` and a review queue at `Special:ConfirmAccounts`, and gives `confirmaccount`, `requestips` and `lookupcredentials` to `bureaucrat`. Wikimedia wikis also limit account creation to six per IP address per day (`$wgAccountCreationThrottle`). The hosted CAPTCHA services that ConfirmEdit can also use send each visitor's details to a third party.

Two gaps in what was settled came to light on the way. Temporary accounts ([0007](0007-actor-identity.md) §3) are registrations too: each anonymous editor's first save creates one, and a script can make thousands. And the rate-limit counters of [0024](0024-subsidiary-accounts.md) §5 key an anonymous request on its IP address, which for IPv6 counts nothing, since a single connection is commonly given a /64, and every address in it is the caller's to use.

### Direction

James's direction, from the design discussion of 2026-10-17:

- **"I would like to offer the ability to register new user accounts through the web frontend (and if I am not mistaken, the MediaWiki action API supports account creation via API). Whether this is allowed depends on the instance and tenant."**
- **"Often times, MediaWiki instances have to install something to prevent bot signups. Could Triplespace perhaps ship with its own proof-of-work gate or some other mechanism to deter automated signups? (You can choose a different strategy if you like.) This could be disabled for instances where this kind of mechanism is unnecessary."**
- On the proposal put to him (a general challenge with triggers, a SHA-256 proof of work, adaptive difficulty, refusal or approval without JavaScript, an approval queue, `skipcaptcha`, a per-address throttle, settings resolved through [0085](0085-settings-and-preferences.md), a honeypot field, the API's fields, a pluggable challenge): **"When account creation is set to nojs, attempted registrations from nojs clients should be rejected with explanation. The badlogin and tempaccount triggers can be in this ADR. The IPv6 /64 change should be referenced in this ADR and then noted as a correction. I believe there are MediaWiki extensions that handle account approval; you should look into how they manage the permission."**

## Decision

### 1. Who may register (extends 0007 §3)

*Changed by A2.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.6.*

### 2. Challenges and their triggers (extends 0007 §3; amends 0012 §4)

*Changed by A3.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §12.3; [07](../architecture/07-actors-and-accounts.md) §2.3, §2.6; [18](../architecture/18-api.md) §1.5.*

### 3. The proof of work (extends 0013 §4; extends 0014 §4)

*Changed by A1.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §5, §12.3; [07](../architecture/07-actors-and-accounts.md) §2.6.*

### 4. Difficulty, the throttle and addresses (extends 0024 §5; corrects 0024 §5; amends 0024 §5; extends 0013 §4)

*Changed by A1, A3, A4.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §12.1, §12.3; [07](../architecture/07-actors-and-accounts.md) §6.2, §6.3.*

### 5. In the site, and without JavaScript (amends 0057 §7; extends 0010 §10)

*Changed by A1.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.6; [19](../architecture/19-site-ui.md) §2.3; [20](../architecture/20-web-tier.md) §3.1.*

### 6. Account requests and approval (extends 0010 §10; extends 0011 §6.1; extends 0012 §5; extends 0013 §4; extends 0016 §2; extends 0028 §2; extends 0047 §1)

*Changed by A3.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §5; [07](../architecture/07-actors-and-accounts.md) §2.6; [08](../architecture/08-tenants-and-instances.md) §3.6; [09](../architecture/09-security-and-moderation.md) §2.2; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3; [18](../architecture/18-api.md) §3.2; [19](../architecture/19-site-ui.md) §2.3; [21](../architecture/21-special-pages.md) §8.*

### 7. Exemptions (extends 0016 §2)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.6; [09](../architecture/09-security-and-moderation.md) §2.2.*

### 8. Settings (uses 0085 §1)

*Current text: [23](../architecture/23-configuration-and-registry.md) §3.2.*

### 9. The API (amends 0012 §4; extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §1.5, §2.2, §2.3.*

### 10. Crates (extends 0005 §2; amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **A third-party CAPTCHA** (reCAPTCHA, hCaptcha, Turnstile). The strongest against a determined operator, but each sends every visitor's address and browser details to a company the wiki has no agreement with, and the image and audio puzzles are among the worst barriers the web puts in front of disabled readers. An instance may still add one later behind the trigger interface of §2.
- **QuestyCaptcha**, a question the wiki writes. Very effective on a small wiki, because nobody writes a bot for one wiki's question, and it works without JavaScript. But its answers would have to be secret, and a `site` setting is public ([23](../architecture/23-configuration-and-registry.md) §1.1); it would need a secret store of its own (Q3).
- **Requests as pending accounts**, an actor record with status `pending` as an OAuth-created subsidiary has ([0025](0025-oauth-server.md) §3). It would reuse a status, but every request, spam included, would put a name in the log that a rejection would then have to erase. ConfirmAccount keeps requests out of the user table for the same reason.
- **Queueing a request when the browser cannot run the challenge**, instead of refusing it. James chose refusal with an explanation: a queue fed by every script that ignores the challenge would be the queue a reviewer has to clear.
- **A minimum time to fill in the form**, beside the honeypot. The proof of work already takes longer than a script would wait.
- **Telling the requester by email.** See §6: the address is unverified, and mailing it on request makes the instance a relay.

## Consequences

- **A tenant can open registration safely by default.** With nothing configured, every password registration, first anonymous save and run of failed logins costs a client a second or two of computation, six accounts a day per address, and a honeypot.
- **Registration needs JavaScript wherever a challenge applies.** A reader without it is told why and what to do; an administrator can create the account for them. A tenant that needs JavaScript-free registration turns the `createaccount` trigger off and relies on the throttle and the honeypot, or on approval.
- **The cost is a speed bump, not a wall.** An operator who writes a solver pays about a CPU-second per account, which the throttle and the rising difficulty multiply. A wiki that is targeted turns approval on.
- **Shared addresses pay more.** A school or a carrier-grade NAT behind one IPv4 address meets the throttle and the higher difficulty sooner; an event organizer holds `noratelimit` and `skipcaptcha`, as on Wikimedia wikis.
- **IPv6 callers are counted by /64 in every rate-limit class**, which makes the anonymous limits mean what they say; two hosts sharing one /64 share a limit.
- **The throttle and the `badlogin` window hold across replicas and restarts** without a shared cache, at the cost of a row per counted act in `private.rate_event` (A1).
- **Rejected requests leave no trace in the log or in any export**, and approved ones enter it as an ordinary account created by its reviewer.

## Open questions

- **Q1. The default difficulty**, to be fixed by measuring the WebAssembly solver on a mid-range phone and a desktop, and a native solver on one server core, before this ADR is accepted.
- **Q2. A difficulty that rises with the instance's total rate of registration**, not only an address's, for a flood spread over many addresses.
- **Q3. A secret store for a question challenge**, and whether a tenant may add one as a fourth kind beside the proof of work, for registration without JavaScript.
- **Q4. Challenging registrations through an external issuer** whose own signups a tenant does not trust, as a field of the issuer's entry.
- **Q5. Solving in WebAssembly**, with `scatter-challenge` itself compiled for the browser as a lazily loaded module of `scatter-wasm`, which would make the work about two or three times faster for the person and so allow a higher difficulty for the same wait. It needs `'wasm-unsafe-eval'` in `script-src` ([0034](0034-frontend-stack.md) §11), which lets any script on the page compile WebAssembly, and a WebAssembly build before the frontend's. The measurement of Q1 is of the JavaScript solver until this is decided.

## References

- MediaWiki's AuthManager and `action=createaccount`, `meta=authmanagerinfo`, and the `UI` status for further input.
- [Extension:ConfirmEdit](https://www.mediawiki.org/wiki/Extension:ConfirmEdit): `$wgCaptchaTriggers` (`createaccount`, `badlogin`, `badloginperuser`, `addurl`), `$wgCaptchaBadLoginAttempts` (3) and `$wgCaptchaBadLoginExpiration` (300 s), the per-user equivalents (20, 600 s), and `skipcaptcha` (`bot`, `sysop`).
- [Extension:ConfirmAccount](https://www.mediawiki.org/wiki/Extension:ConfirmAccount): `Special:RequestAccount`, `Special:ConfirmAccounts`, `confirmaccount`, `requestips` and `lookupcredentials` for `bureaucrat`, `$wgRejectedAccountMaxAge` (one week), `$wgConfirmAccountRejectAge` (one month), and `$wgConfirmAccountCaptchas`.
- [Manual:$wgAccountCreationThrottle](https://www.mediawiki.org/wiki/Manual:$wgAccountCreationThrottle): accounts per IP address per day, 6 on Wikimedia wikis, with `noratelimit` exempt.
- Hashcash (Adam Back, 2002), and the browser proofs of work that follow it, such as ALTCHA and Anubis.

## Amendment log

### A1. A JavaScript solver, and the counts in Postgres

- **Date:** 2026-10-18
- **Source:** Direct: James, design discussion of 2026-10-18
- **Change:** amends §3, §5, §10; extends §4
- **Summary:** Two decisions taken before building, with Q5 opened for the rest. **The browser's solver is JavaScript**, in a Web Worker, with a SHA-256 of its own, held to `scatter-challenge`'s test vectors by a unit test; solving with the crate compiled to WebAssembly was the plan, but the security policy's `script-src 'self'` forbids compiling WebAssembly, and `scatter-wasm` does not exist yet (Q5). **The `createaccount` and `badlogin` counts are kept in Postgres** where there is no shared cache, in `private.rate_event`, rather than in process, so that they hold across the replicas of an API pool and across restarts over windows of a day and ten minutes. James's direction: "Start with the JavaScript solver now. Add the per-address counts to Postgres per your recommendation." Claude proposed the table's shape, the per-account key of `badlogin`, and Q5. The chapter text is [07](../architecture/07-actors-and-accounts.md) §2.6 and §6.3, [03](../architecture/03-storage-caches-and-search.md) §5, [19](../architecture/19-site-ui.md) §2.3 and [22](../architecture/22-crates-and-stack.md) §2.1 and §2.2; [0005](0005-crate-organization.md) A98, [0013](0013-postgres-storage.md) A65 and [0024](0024-subsidiary-accounts.md) A20 record the changes to their sections.

Replaced text (§3):

> **SHA-256, not a memory-hard function.** A browser computes it with the same Rust code the server verifies with, compiled to WebAssembly (§5), and a server checks it in microseconds. Argon2id would narrow the advantage of a GPU, but in WebAssembly on a low-end phone it is slow and asks for tens of megabytes, which would make registration hardest for the readers with the least.

Replaced text (§5):

> in a Web Worker running the `scatter-challenge` solver from `scatter-wasm` as a third lazily loaded module.

Replaced text (§10):

> | `scatter-challenge` *(new)* | Layer 1, pure, builds for `wasm32`: the challenge string, its MAC given a key and a binding, the work function, the solver, and the test vectors. Shared with Scatterbase, which has the same problem at its own registration |
> | `scatter-wasm` | A third lazily loaded module, the solver, for the Web Worker |
> | `triplespace-accounts` | Issuing and checking challenges; the spent set, in Valkey or `private.challenge_spent`; the `badlogin` counters; `private.account_request`, approval and rejection, and the pending and rejected login answers |
> | `triplespace-ui` | The worker and progress note; the `<noscript>` explanations; the honeypot field; the request form and `Special:ConfirmAccounts`; the open-request count |
> | `triplespace-db` | `private.account_request` and `private.challenge_spent` |

### A2. Registering through an external issuer

- **Date:** 2026-10-18
- **Source:** [0088](0088-external-identity-providers.md) §6
- **Change:** extends §1
- **Summary:** A first login through an external issuer keeps the identity as the session's pending identity; `createaccount` with `createpreservestate` and a name creates the account bound to it, with no password and no challenge, or under `account.approval` files a request whose credential is the issuer and subject. The chapter text is [07](../architecture/07-actors-and-accounts.md) §2.7.

### A3. Challenges at the auth host

- **Date:** 2026-10-18
- **Source:** [0089](0089-the-auth-host-and-instance-logins.md) §6, §8
- **Change:** amends §2, §4, §6
- **Summary:** Passwords belong to instance logins, created and used on the auth host. The `createaccount` trigger guards creating a login with a password; `badlogin` guards a password login at the auth base and through `clientlogin` on a tenant, and its per-name counter counts by login, under the empty tenant, so failures everywhere add up. Creating a tenant account from a login is not challenged, since the login was challenged or vouched for when it was made, but the throttle counts it. On the auth host the triggers, the difficulty and the throttle resolve as the instance defaults of their settings, and the farm code stands in a challenge for the tenant's. An account request's credential is the login, or under farm identity the farm subject. The chapter text is [07](../architecture/07-actors-and-accounts.md) §2.6.

Replaced text (§2):

> | `createaccount` | Creating an account, or filing a request for one (§6), with the built-in `password` issuer, through `action=createaccount` or `Special:CreateAccount` | ConfirmEdit's `createaccount` |

Replaced text (§2):

> | `badlogin` | `action=clientlogin` with the `password` issuer, once the caller's address has failed 3 times within 5 minutes or the named account has failed 20 times within 10 minutes | ConfirmEdit's `badlogin` and `badloginperuser`, with their defaults |

Replaced text (§2):

> **What triggers do not cover.** A registration through an external issuer is not challenged: the issuer has already decided who may hold an account there, and a tenant that does not trust an issuer's signups does not allow it for login.

Replaced text (§2):

> **One solution serves one act.** A challenge is issued for one trigger, one tenant and one session,

Replaced text (§4):

> The key is an address key, or for the per-account `badlogin` counter the account's case-folded name, so the table holds what the cache would hold

Replaced text (§4):

> and the two `badlogin` counters, by address key and by account,

Replaced text (§6):

> the credential the account will be created with (a password hash, or an issuer and subject)

### A4. The account-creation limit is a setting

- **Date:** 2026-10-24
- **Source:** Direct: James, design discussion of 2026-10-24
- **Change:** amends §4
- **Summary:** Different wikis need different registration limits, so the `createaccount` class's limit is the tenant setting `account.creation_limit` (default 6, the Wikimedia figure; 1 to 10000), beside the difficulty settings `challenge.bits` and `challenge.max_bits` that §4 already has; the window stays a day and `noratelimit` stays exempt. A test tenant sets all three low enough to run quickly and high enough to show the rise and the limit working (`tools/acl_check.sh` uses 10, 14 and 20).

Replaced text (§4):

> **`createaccount` is a rate-limit class** (07 §6.2), counting accounts created and requests filed (§6) by the caller's address key, whoever the caller is: 6 per day for every group, as on Wikimedia wikis, with `noratelimit` exempt as there.

New text:

> **`createaccount` is a rate-limit class** (07 §6.2), counting accounts created and requests filed (§6) by the caller's address key, whoever the caller is: `account.creation_limit` per day for every group, 6 by default as on Wikimedia wikis, with `noratelimit` exempt as there.

### A5. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [20](../architecture/20-web-tier.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
