# 0087. Account registration and challenges

- **Status:** Proposed
- **Date:** 2026-10-17
- **Updated:** 2026-10-18 (A1)
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

**`createaccount` decides whether an account may be made at all.** It stays as [0024](0024-subsidiary-accounts.md) A13 left it: `universe` by default, taken from `universe` by a private tenant ([0056](0056-security-model.md) A10), and taken from `universe` by any tenant that closes registration; that is the whole of "registration closed", and no setting repeats it. `groups.toml` moves `createaccount` from `autoconfirmed` to `universe`, as A13 decided.

**How an account comes to exist** is then three things a tenant chooses. The first two are settings that resolve through the instance's defaults and locks ([0085](0085-settings-and-preferences.md) §2.1), so an instance can impose either on every tenant:

- whether a **challenge** stands in front of the form (§2–§4);
- whether a new account is made at once or **requested** and approved by a person (§6);
- which identity providers may register, which is already the issuer registry and `login.password` ([0007](0007-actor-identity.md) §3).

**Both doors register.** A person registers with the built-in `password` issuer, choosing a name and a password in one form, or by a first login through an external issuer, which continues to `Special:CreateAccount` to choose a name ([0010](0010-site-ui.md) §10). `action=createaccount` serves both, as MediaWiki's AuthManager does.

### 2. Challenges and their triggers (extends 0007 §3; amends 0012 §4)

**A challenge is a piece of work the instance asks of a client before an act that creates an identity or guesses at one.** It is not a test that the client is a person. It makes every account cost a few seconds of computation, which is nothing to a person and a great deal to a script that wants thousands, and it is something a generic spambot written for MediaWiki's forms does not know how to do. What a determined operator can still do, the throttle (§4), approval (§6) and the edit filters of [0030](0030-edit-filters.md) answer.

**Three triggers**, named as ConfirmEdit names its own where it has the same one, listed in the setting `challenge.triggers` (§8):

| Trigger | The act it guards | Counterpart |
|---|---|---|
| `createaccount` | Creating an account, or filing a request for one (§6), with the built-in `password` issuer, through `action=createaccount` or `Special:CreateAccount` | ConfirmEdit's `createaccount` |
| `tempaccount` | The write that would create a temporary account ([0007](0007-actor-identity.md) §3): an anonymous request's first save | None in ConfirmEdit; its `edit` trigger, narrowed to the save that creates an identity |
| `badlogin` | `action=clientlogin` with the `password` issuer, once the caller's address has failed 3 times within 5 minutes or the named account has failed 20 times within 10 minutes | ConfirmEdit's `badlogin` and `badloginperuser`, with their defaults |

**What triggers do not cover.** A registration through an external issuer is not challenged: the issuer has already decided who may hold an account there, and a tenant that does not trust an issuer's signups does not allow it for login. A subsidiary's bot-password login (`action=login`) is not challenged, since its secret is 32 random bytes and cannot be guessed at any rate. ConfirmEdit's `addurl` and `create` have no counterpart: links added by new accounts are edit filters' work ([0030](0030-edit-filters.md)), and a trigger on every edit would put a computation in front of every anonymous save rather than the first.

**One solution serves one act.** A challenge is issued for one trigger, one tenant and one session, and a solution is accepted once. A `tempaccount` solution makes the temporary account, which then holds the session, so the anonymous editor is asked once; a `badlogin` solution admits one login attempt.

### 3. The proof of work (extends 0013 §4; extends 0014 §4)

*Changed by A1.*

**The work is to find a nonce.** A challenge is a string, `1.{tenant code}.{trigger}.{bits}.{expires}.{salt}.{mac}`: the format version; the tenant's issuer code ([08](../architecture/08-tenants-and-instances.md) §1.2); the trigger; the difficulty in bits; the expiry, in seconds since the epoch, 10 minutes after issue; 16 random bytes; and a MAC. A **solution** is a decimal number `n` of at most 20 digits such that SHA-256 of the challenge, a colon and `n` (`{challenge}:{n}`, in UTF-8) begins with at least `bits` zero bits. Finding one takes `2^bits` hashes on average; checking one takes one. Anyone can write a solver in a few lines from this paragraph, and the test vectors that ship with the crate (§10) fix the details.

**The MAC binds the challenge to its session.** It is the first 16 bytes of HMAC-SHA256, base64url without padding, over the rest of the string, a zero byte and the session ID, under a key derived from the instance signing seed as the CSRF tokens' key is, with its own domain separator. The session ID is not in the string, so a challenge passed to another session fails as a CSRF token would.

**Checking a solution** is, in order: the MAC, the expiry, the tenant and trigger against the act, the hash, and that the challenge has not been spent. A challenge that passes is recorded as **spent** until its expiry, in Valkey as `ch:{tenant}:{mac}` when a shared cache is configured ([03](../architecture/03-storage-caches-and-search.md) §12.3), and otherwise in `private.challenge_spent` (`mac`, `tenant`, `expires`), swept as sessions are, so that every replica of an API pool refuses a replay. A failure answers as §9 says and spends nothing.

**SHA-256, not a memory-hard function.** A browser computes it in a few lines of JavaScript (§5), and a server checks it in microseconds. Argon2id would narrow the advantage of a GPU, but in a browser on a low-end phone it is slow and asks for tens of megabytes, which would make registration hardest for the readers with the least.

### 4. Difficulty, the throttle and addresses (extends 0024 §5; corrects 0024 §5; amends 0024 §5; extends 0013 §4)

*Changed by A1.*

**The difficulty is a setting, and it rises with repetition.** A challenge's `bits` is `challenge.bits` (default 20, about a million hashes) plus one for each account created from the caller's address key in the last 24 hours, up to `challenge.max_bits` (default 24, sixteen times the work). The count is the `createaccount` counter below, so it costs no state of its own. Setting `challenge.max_bits` to `challenge.bits` turns the rise off. The defaults are a starting point, to be fixed by measurement before the ADR is accepted (Q1): the target is a second or two on a mid-range phone.

**`createaccount` is a rate-limit class** ([07](../architecture/07-actors-and-accounts.md) §6.2), counting accounts created and requests filed (§6) by the caller's address key, whoever the caller is: 6 per day for every group, as on Wikimedia wikis, with `noratelimit` exempt as there. An event organizer who creates accounts for participants holds `noratelimit`, as Wikimedia's account creators hold its counterpart. Over the limit, `action=createaccount` answers `FAIL` with MediaWiki's message code `acct_creation_throttle_hit`.

**An address key is an IPv4 address or an IPv6 /64.** This corrects [0024](0024-subsidiary-accounts.md) §5 for every rate-limit class, not only this one: the counters that key an anonymous request on "the IP" key it instead on the address for IPv4 and on its /64 prefix for IPv6, since a host commonly holds a whole /64 and can rotate through it at no cost. The `badlogin` counters and the difficulty's count use the same key. Addresses and their prefixes stay where [0007](0007-actor-identity.md) §3 keeps them, in the cache and in `private`, never in the log.

**Where these counts are kept** (A1). The `createaccount` counter, which the difficulty also reads, and the two `badlogin` counters, by address key and by account, are kept in Valkey when a shared cache is configured, as every class is ([07](../architecture/07-actors-and-accounts.md) §6.3), and otherwise in Postgres, in `private.rate_event`: one row per counted act, `(tenant, class, key, at)`, read as the number of rows inside the window and swept once older than the longest window, as sessions are. Counting them in process, as an instance without a shared cache counts the other classes, would let a script that spreads its attempts over the replicas of an API pool, or waits for a restart, start again from nothing, and these windows are a day and ten minutes long, not a minute. The key is an address key, or for the per-account `badlogin` counter the account's case-folded name, so the table holds what the cache would hold and is in no export ([0027](0027-preferences-and-portability.md) §2).

### 5. In the site, and without JavaScript (amends 0057 §7; extends 0010 §10)

*Changed by A1.*

**The browser does the work while the person types.** `Special:CreateAccount`, `Special:UserLogin` when `badlogin` applies, and the editor when the save would create a temporary account start solving as soon as the page loads, in a Web Worker running a **solver written in JavaScript**, a SHA-256 of its own over a reused buffer, since the Web Crypto API answers each hash asynchronously and would make a million of them slow (A1). A unit test holds it to the test vectors of `scatter-challenge`, so it and the server agree on every detail of the work. Solving with the crate itself, compiled to WebAssembly, would be about two or three times as fast, but needs `'wasm-unsafe-eval'` in the security policy's `script-src` ([0034](0034-frontend-stack.md) §11) and a WebAssembly build in front of the frontend's (Q5). The form shows the work as a progress note in a polite live region, and its submit button waits only if the person is faster than the solver. Nothing about the person leaves the instance, and there is nothing to see or hear, so the challenge asks nothing of a reader using a screen reader, switch access or a magnifier beyond the wait.

**Without JavaScript, an act that a challenge guards is refused, with an explanation.** This amends [0057](0057-web-tier.md) §7's promise that creating an account works without JavaScript: it does where no challenge applies. Where one does, the page says, in a `<noscript>` block and again as the form's error if it is submitted, what the instance asks and why: that the wiki asks the browser for a short computation to slow automated sign-ups, that nothing is sent anywhere else, and what the person can do instead, which is to turn JavaScript on for the site, to ask an administrator to create the account for them (§7), or, for a failed login, to wait out the window. The message is `ts-challenge-nojs`, with `ts-challenge-nojs-tempaccount` for an anonymous save, which also says that logging in avoids it. A refused request files nothing and spends nothing.

**A honeypot field** is on `Special:CreateAccount` in every configuration: a text input placed off-screen, labelled "Leave this field empty", with `tabindex="-1"` and `autocomplete="off"`, outside the accessibility tree. The web tier drops a submission that fills it, answering as it answers a refused creation, without calling the API. It catches the form-fillers that a tenant with no challenge would otherwise meet, and it is the web tier's own check, since the API has no form to hide a field in.

### 6. Account requests and approval (extends 0010 §10; extends 0011 §6.1; extends 0012 §5; extends 0013 §4; extends 0016 §2; extends 0028 §2; extends 0047 §1)

**With `account.approval` on, registering files a request** instead of creating an account, as ConfirmAccount's request form does. The model is ConfirmAccount's, with the permission names it uses, so a Wikimedia or Miraheze administrator recognizes them; it differs where the log does.

**The request lives in `private`, not the log.** `private.account_request` holds the request's ID and tenant, the chosen name and its case-folded key, the credential the account will be created with (a password hash, or an issuer and subject), the person's statement, the caller's address key, the time it was filed, and, once decided, the decision, its reviewer, time and reason; its portability class is "never leaves" ([0027](0027-preferences-and-portability.md) §2). **An account that is never approved never enters the log**: a spam name, often abusive, never has to be erased, because it was never written. A pending request holds its name, so the name-taken check of [19](../architecture/19-site-ui.md) §2.3 refuses it to anyone else.

**The form** is `Special:CreateAccount` itself, which says that the wiki reviews new accounts; `Special:RequestAccount` is an alias of it. It asks for the name and credential as before, and a statement of up to 1,000 characters about who the person is and what they want to do, as ConfirmAccount's biography asks; `account.request_statement` makes it `off`, `optional` (the default) or `required`. The challenge (§2) and the throttle (§4) apply to filing a request as to creating an account, as ConfirmAccount applies ConfirmEdit's CAPTCHA to its requests.

**Reviewing** is `Special:ConfirmAccounts`, restricted to `confirmaccount`, which lists open requests oldest first with their names, statements and ages, and the address key to holders of `ts-viewip` ([0007](0007-actor-identity.md) §3), which takes the place of ConfirmAccount's `requestips`. A reviewer **approves** a request, which creates the account with its credential's binding and the first actor record as one change set, logged as `newusers/create2` with the reviewer as performer, MediaWiki's event for an account created by someone else; or **rejects** it with a reason, which stays in `private`. Holders of `confirmaccount` see the number of open requests in the account menu, read from `private.account_request`. ConfirmAccount's `lookupcredentials` has no counterpart, because nothing of a request is kept once it is decided, and its `confirmaccount-notify` none either, because a request is not an activity: a notification is an activity row delivered to an inbox ([16](../architecture/16-logs-feeds-and-notifications.md) §5.1), and a request is kept out of the log on purpose.

**`confirmaccount`** is a new permission, ConfirmAccount's name, with `bureaucrat` as its default group, as there. In approval mode, an account created for another person is created at once only when its creator holds `confirmaccount`; anyone else's, a `createaccount` holder's included, is filed as a request, which closes the door ConfirmAccount leaves open by default to sysops.

**The person learns the outcome by logging in.** A login with the credential of a pending request answers `FAIL` with `ts-account-pending`; one of a rejected request answers `ts-account-rejected` with the reviewer's reason, for 7 days after the decision, as ConfirmAccount keeps rejected requests a week; after that the row is deleted and the name is free. An open request expires after `account.request_expiry` (default 30 days, ConfirmAccount's age for treating a request as rejected) and is deleted likewise. No email is sent: the instance would be mailing an address nobody had verified, at the request of anyone who typed it in.

**Under farm identity** ([0028](0028-tenancy-policy.md) §2), signing up to the farm is not affected, since the farm has no passwords and its accounts edit nothing. A tenant with `account.approval` on does not create a tenant account on a farm account's first visit: it files a request whose credential is the farm subject, and the tenant account is created, bound and linked as before when it is approved.

**REST routes**: `GET /account/requests`, `POST /account/requests/{id}/approve` and `POST /account/requests/{id}/reject` (with `reason`), with `confirmaccount`, `no-store` like everything under `/account`.

### 7. Exemptions (extends 0016 §2)

**`skipcaptcha`**, ConfirmEdit's permission, with its defaults, `bot` and `sysop`, exempts its holder from every trigger. It matters where a logged-in account acts: an administrator creating an account for someone who cannot run the challenge is the way §5 points people to, so the administrator holds both `createaccount` and `skipcaptcha`. Anonymous callers, the only ones `tempaccount` and `badlogin` meet, cannot hold it.

**The instance's own acts are not challenged**: `triplespace-cli instance create` and the owner's account, adoption ([0035](0035-adopting-a-wikibase.md) §5), farm auto-creation, and the approval of a request, whose filing was.

### 8. Settings (uses 0085 §1)

Each is a `tenant` setting in the `accounts` group of `settings.toml`, resolved through the instance's defaults and locks ([0085](0085-settings-and-preferences.md) §2.1):

| Setting | Values | Default |
|---|---|---|
| `challenge.triggers` | A list of `createaccount`, `tempaccount`, `badlogin` | All three. The empty list turns challenges off |
| `challenge.bits` | 8 to 32 | 20 |
| `challenge.max_bits` | `challenge.bits` to 32 | 24 |
| `account.approval` | `on`, `off` | `off` |
| `account.request_statement` | `off`, `optional`, `required` | `optional` |
| `account.request_expiry` | A duration | 30 days |

An instance that wants a challenge on every wiki it hosts locks `challenge.triggers`; one whose wikis do not need it sets the instance default to the empty list.

### 9. The API (amends 0012 §4; extends 0012 §5)

**AuthManager's fields carry the challenge.** Where a trigger applies to the caller, `meta=authmanagerinfo` with `amirequestsfor=create` (or `login`, under `badlogin`) lists a required request, `TriplespaceChallengeAuthenticationRequest`, with three fields: `tschallengeid`, hidden, holding the challenge; `tschallengeinfo`, of type `null`, labelled with what to compute; and `tschallengesolution`, a string. The request's `metadata` gives `{"algorithm": "sha256-leading-zero-bits", "bits": n, "expires": t}`. A `createaccount` or `clientlogin` whose solution is missing, wrong, stale or spent answers `status: UI` with `messagecode: ts-challenge-required` and a fresh challenge, so the client continues with `createcontinue` or `logincontinue`, as ConfirmEdit answers a wrong CAPTCHA.

**Writes carry it as two parameters.** A write module whose save would create a temporary account without a solution answers the error `ts-challenge-required`, with `challenge: {id, algorithm, bits, expires}`; the client resends with `tschallengeid` and `tschallengesolution`. `ts-challenge-required` joins the refusals every write module shares ([18](../architecture/18-api.md) §1.5).

**Approval** changes one answer: with `account.approval` on, a `createaccount` that would have created an account answers `FAIL` with `messagecode: ts-account-requested`, since no account exists to log in to, and the message says what happens next.

**`siprop=triplespace` reports** `challenge.triggers`, the algorithm and `challenge.bits`, and `account.approval`, so a client knows before it asks; `meta=userinfo&uiprop=cancreateaccount` answers as in MediaWiki, from `createaccount` alone.

**Clients.** `triplespace-client` solves a challenge natively, and `triplespace-cli challenge solve {challenge}` prints a solution, so a script registering a single account through the API, or a test, can. A client that cannot run the work, such as Pywikibot, which has no account creation, is unaffected.

### 10. Crates (extends 0005 §2; amends 0005 §2)

*Changed by A1.*

| Crate | Change |
|---|---|
| `scatter-challenge` *(new)* | Layer 1, pure, builds for `wasm32`: the challenge string, its MAC given a key and a binding, the work function, the native solver, and the test vectors. Shared with Scatterbase, which has the same problem at its own registration |
| `triplespace-accounts` | Issuing and checking challenges; the spent set, in Valkey or `private.challenge_spent`; the `createaccount` and `badlogin` counters, in Valkey or `private.rate_event`; `private.account_request`, approval and rejection, and the pending and rejected login answers |
| `triplespace-api-action` | The AuthManager request and fields; `action=createaccount`, with challenge, throttle and approval; the `tempaccount` refusal and parameters on write modules; `siprop=triplespace` |
| `triplespace-api-rest` | `/account/requests` |
| `triplespace-ui` | The JavaScript solver, its worker and progress note, and its test against the crate's vectors; the `<noscript>` explanations; the honeypot field; the request form and `Special:ConfirmAccounts`; the open-request count |
| `triplespace-client`, `triplespace-cli` | The native solver; `challenge solve` |
| `triplespace-db` | `private.account_request`, `private.challenge_spent` and `private.rate_event` |
| `triplespace-cache` | The `createaccount` class and the address key of §4 for every class |

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

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §10 | extends | 0005 A97 |
| [0005](0005-crate-organization.md) §2 | §10 (A1) | amends | 0005 A98 |
| [0007](0007-actor-identity.md) §3 | §1, §2 | extends | 0007 A24 |
| [0010](0010-site-ui.md) §10 | §5, §6 | extends | 0010 A43 |
| [0011](0011-logs.md) §6.1 | §6 | extends | 0011 A33 |
| [0012](0012-api-requirements.md) §4 | §2, §9 | amends | 0012 A77 |
| [0012](0012-api-requirements.md) §5 | §6, §9 | extends | 0012 A77 |
| [0013](0013-postgres-storage.md) §4 | §3, §6 | extends | 0013 A64 |
| [0013](0013-postgres-storage.md) §4 | §4 (A1) | extends | 0013 A65 |
| [0014](0014-caches-and-search.md) §4 | §3, §4 | extends | 0014 A28 |
| [0016](0016-permissions-and-access-control.md) §2 | §6, §7 | extends | 0016 A34 |
| [0024](0024-subsidiary-accounts.md) §5 | §4 | extends | 0024 A19 |
| [0024](0024-subsidiary-accounts.md) §5 | §4 | corrects | 0024 A19 |
| [0024](0024-subsidiary-accounts.md) §5 | §4 (A1) | amends | 0024 A20 |
| [0028](0028-tenancy-policy.md) §2 | §6 | extends | 0028 A22 |
| [0047](0047-special-pages.md) §1 | §6 | extends | 0047 A23 |
| [0057](0057-web-tier.md) §7 | §5 | amends | 0057 A6 |

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
