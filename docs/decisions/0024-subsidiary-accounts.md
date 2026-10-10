# 0024. Subsidiary accounts, API keys and rate limits

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A17)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

Every automated edit in the earlier ADRs is attributed to a **bot account** with an **operator**. [0007](0007-actor-identity.md) §6 makes a local bot a `prov:SoftwareAgent` that `prov:actedOnBehalfOf` its operator's account; [0010](0010-site-ui.md) §8 names the operator on the bot's contributions page; [0012](0012-api-requirements.md) §4 exposes it as `usprop=operator`; [0021](0021-notifications.md) §2 routes a bot's job and mention notifications to its operator; [0016](0016-permissions-and-access-control.md) §3 gives the `bot` group the bot flag and `ts-runjob`. None of them says where the operator relation is recorded, how a bot authenticates, or how fast it may edit.

Two things are missing, and the 2026-09-27 review named the first as a gap:

1. **Non-interactive authentication.** [0007](0007-actor-identity.md) §3 makes login a redirect to an identity provider, which a script cannot follow, and [0012](0012-api-requirements.md) §4 offers only that flow. Pywikibot editing `Project` pages is the acceptance test of [0008](0008-namespaces-and-document-pages.md) §12, and Pywikibot logs in with a bot password or an OAuth consumer. [0016](0016-permissions-and-access-control.md) §3 has `triplespace-cli` issue `owner` "a login token" and says nothing about anyone else.
2. **Rate limits.** Left open by [0012](0012-api-requirements.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md) and [0021](0021-notifications.md); [0014](0014-caches-and-search.md) §4 reserves `rl:` counters in Valkey for them without saying what they count.

MediaWiki's answer to the first is two mechanisms bolted on later: **bot passwords**, a per-account credential with a label, a restricted set of *grants* and optional IP ranges, logged in with `action=login` as `Name@label`; and **OAuth consumers**, for tools acting on a user's behalf. Both authenticate *the same account* the human uses, so a bot's edits are either mixed into a person's history or made from a second account the person registered by hand and that the wiki cannot tell is theirs. Wikimedia's bot policy then rebuilds the relation socially: the bot's user page must name its operator, and a bureaucrat grants the bot flag after review.

James's direction is to make the relation structural: a bot is a **subsidiary account**. `User:Example` creates `User:ExampleBot`; the subsidiary is a real account with its own history, its own user page and its own API key; the wiki knows who operates it; bureaucrats approve it for bot activity and can move it to another operator; and site policy sets how fast bots and non-bots may edit. Primary accounts hold no API keys. Automated editing is done by subsidiaries or not at all.

## Decision

### 1. A subsidiary is a local account with an operator (amends 0007 §4 and §6)

*Changed by A2, A6, A17.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.3, §1.5, §4.1.*

### 2. Creating, approving and transferring

*Changed by A2, A17.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.3, §3.2, §4.2, §4.3, §4.4.*

### 3. What a subsidiary inherits (amends 0016 §3)

*Current text: [07](../architecture/07-actors-and-accounts.md) §4.5.*

### 4. API keys (extends 0007 §3)

*Changed by A2, A3, A10.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.4, §5.1, §5.2, §5.3, §5.4, §5.5.*

### 5. Rate limits (settles 0012, 0016, 0020 and 0021)

*Changed by A4, A5, A7, A9, A11, A12, A14, A16.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §6.1, §6.2, §6.3.*

### 6. Jobs, attribution and notifications (amends 0002 §8.3; extends 0021 §2)

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.5, §4.6.*

### 7. Tenants (uses 0018 §4 and §10)

*Current text: [08](../architecture/08-tenants-and-instances.md) §6.2, §10.3.*

### 8. API (extends 0012 §4 and §5)

*Changed by A10.*

*Current text: [18](../architecture/18-api.md) §1.5, §2.2, §2.3, §3.1, §3.2.*

### 9. Site UI (extends 0010 §8 and §11)

*Current text: [19](../architecture/19-site-ui.md) §1.3, §1.5, §2.4, §2.5.*

### 10. Storage (extends 0013 §4 and §5)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.5, §4.6, §4.16, §5, §12.3, §13.*

### 11. Permissions (extends 0016 §2)

*Changed by A13, A17.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §8.4.*

### 12. Scatterbase

*Changed by A3.*

*Current text: [22](../architecture/22-crates-and-stack.md) §1.3.*

### 13. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **Every automated edit has a named agent and a named operator**, structurally, on every instance that ever holds it. Wikimedia's bot policy achieves this by asking; here the wiki knows.
- **Pywikibot works** as a subsidiary with a bot-password-shaped key, which was the acceptance test of 0008 §12 and had no path before this ADR.
- **A person's login and a bot's credentials never touch.** Revoking a key, or a compromised script, cannot affect an interactive account, and a compromised identity provider cannot authenticate a bot.
- **Approval is a group membership**, so the bot flag, its rate and its autopatrol arrive together and are one bureaucrat action, already logged.
- **Rate limits have a home**, and the question of bot speed is one row in one table.
- **Blocks compose downward.** Blocking a person blocks their bots; this is the only inheritance, and it is the one that matters for abuse.
- **No cross-tenant bots**, as there are no cross-tenant accounts. A farm bot is one subsidiary per tenant.
- **Third-party tools acting for a person are not covered here.** QuickStatements-style OAuth consumers need the instance to be an OAuth server, which is [0025](0025-oauth-server.md). The rule that carries over is fixed now: a tool acts as one of the person's subsidiaries, never as the primary account, so "automated editing is done by subsidiaries" holds for delegated tools as for scripts.
- **One more registry file** (`grants.toml`) and one more private table.

## Open questions

- **Q1.** ~~**OAuth server for delegated tools.**~~ *Settled by [0025](0025-oauth-server.md): the tool acts as a subsidiary chosen or created at authorization, a created one is `pending` until a bureaucrat approves it, and its edits carry the `oauth:{slug}` tag; it never acts as the primary account.*
- **Q2. Name patterns.** Whether to require a suffix by default, and what the rules for account names are ([0010](0010-site-ui.md) Q10, settled by 0010 A6).
- **Q3.** ~~**Per-key change tags.** Whether the key's label should appear as a change tag on the edits it made, so an operator can tell which deployment did what. It would put the label, chosen by the operator, into records.~~ *Settled by [0025](0025-oauth-server.md) §4: keys carry no tag; a label is private state, and an operator who wants deployments told apart uses one subsidiary per deployment.*
- **Q4.** ~~**Bootstrap login for `owner`.** [0016](0016-permissions-and-access-control.md) §3 has `triplespace-cli` create `local:1` as the sole member of `owner` and issue it "a login token", for an instance with no identity provider configured. §1 and §4 here say a primary account authenticates only through a binding and holds no key or token. The two have to be reconciled before the first `instance create`.~~ *Settled by A3, with the built-in `password` issuer of 0007 A3: a built-in `password` issuer, opt-in through `login.password`, whose binding for `local:1` the CLI creates. A primary account still holds no key or token; a password is a binding to an issuer the instance happens to run.*
- **Q5. Adminbots.** Whether a subsidiary may be added to `sysop` at all, or only to a narrower group, as some Wikimedia projects require.
- **Q6. Key rotation.** Whether to enforce an expiry by default, and whether to notify the operator before one.
- **Q7. Default limits** in §5, once measured against real bot and human editing rates.
- **Q8. Subsidiaries of subsidiaries** are refused; whether an organisation account (a primary account held by a group of people) needs a different shape is a policy question for instances that want one.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §8.3 | §6 | amends | 0002 A9 |
| [0005](0005-crate-organization.md) §2 | §13 | extends | 0005 A20 |
| [0007](0007-actor-identity.md) §3, §4, §6 | §1–2, §4 | extends | 0007 A8 |
| [0007](0007-actor-identity.md) Q5 | §4 | settles | 0007 Q5 |
| [0010](0010-site-ui.md) §8, §11 | §9 | extends | 0010 A14 |
| [0010](0010-site-ui.md) Q9 | §5 | settles | 0010 Q9 |
| [0011](0011-logs.md) §6.1 | §2, §4 | extends | 0011 A8 |
| [0012](0012-api-requirements.md) §4, §5, §6, §8 | §4–5, §8 | extends | 0012 A13 |
| [0012](0012-api-requirements.md) Q7 | §5 | settles | 0012 Q7 |
| [0013](0013-postgres-storage.md) §5, §5.2, §5.6 | §10 | amends | 0013 A9 |
| [0013](0013-postgres-storage.md) §4, §5.4, §5.5, §7, §8 | §10 | extends | 0013 A9 |
| [0014](0014-caches-and-search.md) §1, §5 | §4–5 | amends | 0014 A3 |
| [0014](0014-caches-and-search.md) §10 | §4–5 | extends | 0014 A3 |
| [0015](0015-record-format-and-partition-registry.md) §5 | §4 | extends | 0015 A7 |
| [0016](0016-permissions-and-access-control.md) §2, §3 | §3, §5, §11 | amends | 0016 A7 |
| [0016](0016-permissions-and-access-control.md) §2 | §3, §5, §11 | extends | 0016 A7 |
| [0016](0016-permissions-and-access-control.md) Q3 | §5 | settles | 0016 Q3 |
| [0019](0019-discussions.md) Q8 | §5 | settles | 0019 Q8 |
| [0020](0020-change-feeds.md) §7 | §5 | amends | 0020 A4 |
| [0020](0020-change-feeds.md) Q5 | §5 | settles | 0020 Q5 |
| [0021](0021-notifications.md) §2, §8 | §5–6 | extends | 0021 A3 |

## References

- [Manual:Bot passwords](https://www.mediawiki.org/wiki/Manual:Bot_passwords) and [Special:BotPasswords](https://www.mediawiki.org/wiki/Special:BotPasswords) (labels, grants, IP ranges, `Name@label` login)
- [Manual:$wgGrantPermissions](https://www.mediawiki.org/wiki/Manual:$wgGrantPermissions) and [Special:ListGrants](https://www.mediawiki.org/wiki/Special:ListGrants)
- [Manual:$wgRateLimits](https://www.mediawiki.org/wiki/Manual:$wgRateLimits) and [API:Ratelimits](https://www.mediawiki.org/wiki/API:Userinfo)
- [Wikidata:Bots](https://www.wikidata.org/wiki/Wikidata:Bots) and [Wikipedia:Bot policy](https://en.wikipedia.org/wiki/Wikipedia:Bot_policy) (operator disclosure, approval, adminbots)
- [Manual:Log actions](https://www.mediawiki.org/wiki/Manual:Log_actions) (`newusers/create2`)
- [Pywikibot: BotPasswords](https://www.mediawiki.org/wiki/Manual:Pywikibot/BotPasswords)
- [RFC 6750 — Bearer token usage](https://www.rfc-editor.org/rfc/rfc6750)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §13
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A20).

Replaced text (§13):

> | Crate | Change |
> |---|---|
> | `scatter-actors` | The `operator` attribute and the `retired` status on actor records; block inheritance from the operator in effective-permission evaluation (§3); grants and their intersection with permissions (§4); rate-limit policy evaluation as a pure function over the `site` table and an actor's groups (§5). Embeds `docs/registry/grants.toml` |
> | `scatter-mwlog` | `newusers/create2` and the new `bot/transfer` action |
> | `triplespace-accounts` | Subsidiary creation, transfer, retirement; `private.api_key`; bearer and bot-password authentication; session–key binding and revocation (§2, §4) |
> | `triplespace-cache` | The `rl:` counters with per-class windows (§5) |
> | `triplespace-api-action`, `triplespace-api-rest` | §8, and the `ratelimited` refusal in front of every write |
> | `triplespace-cli` | Creates the primary tenant's sync subsidiaries beside `local:1` (§1) |
>
> No crate is added.

### A2. OAuth tokens and pending subsidiaries

- **Date:** 2026-09-27
- **Source:** [0025](0025-oauth-server.md) §3–4
- **Change:** extends §1, §2; amends §4
- **Summary:** A subsidiary created in an OAuth authorization is `pending` until a bureaucrat approves it; tokens are subsidiary credentials like keys, and their grants intersect the permissions as a key's do; keys carry no tag, since a label is private state. §2 and §4 had been given the clauses in place. This settled Q1 and Q3.

Replaced text: not recorded; §4 had spoken of keys alone, and the sentence on tokens was added in place.

### A3. Decisions of 2026-09-27 (evening)

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** extends §4; amends §12
- **Summary:** Client signing keys (decision 4). Beside its API keys, a subsidiary may hold one or more **signing keys**: Ed25519 keypairs the operator generates, whose public half is registered as a `scatter:v0/key` record in the `actors` partition ([0015](0015-record-format-and-partition-registry.md) §1, as amended) and whose private half the instance never sees. The operator registers, rotates and revokes them from the subsidiary's keys section and `POST /account/subsidiaries/{name}/signing-keys`; registration and revocation project as `key/register` and `key/revoke` log events ([0011](0011-logs.md) §6.1), public like the operator relation. A write submitted with a `signature` (as a field of the change set or page operation, or an `X-Triplespace-Signature` header on a REST write) is verified against the actor's current key before the append, and the record's attestation part carries it. A signature never grants anything and is never required by default; `site` configuration may require one for the `bot` group (`signing.require_for_bot`), which is what an instance that wants third-party-verifiable bot attribution turns on. History rows and "About this edit" ([0010](0010-site-ui.md) §5–6) show a *signed* mark with the key ID, and the record and proof routes ([0012](0012-api-requirements.md) §5) return the signature so a third party can check it against the export bundle. `triplespace-cli` and the reference SDK produce the canonical CBOR of the content and comment parts and sign it; Pywikibot does not, and works unsigned. Primary accounts hold no signing key, for the reason they hold no API key: a person's edits are attributed by session, a program's by credential. §12's attestation half is therefore no longer open. The bootstrap login for `owner` (decision 1) is the built-in `password` issuer recorded as 0007 A3: a primary account still holds no key or token; a password is a binding. This settled Q4.

Replaced text (§12):

> Scatterbase's decision record wants client signing keys bound to actors ([0007](0007-actor-identity.md), open questions). A subsidiary is the actor such a key would bind to, and `private.api_key` is where the *authentication* half of that lives today; the *attestation* half, a public key whose signature travels in the record, is still the identity work the two products share. The grants table and the operator attribute are Scatterbase's to reuse; rate limits are Triplespace's.

### A4. File rate classes

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §15
- **Change:** extends §5
- **Summary:** Three classes for files, with MediaWiki's names: `upload`, `renderfile` (thumbnails rendered on a miss) and `renderfile-nonstandard`.

### A5. The parse class

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §16
- **Change:** extends §5
- **Summary:** A class `parse` counts expansions a client asks for directly: `action=parse` with `text`, `action=expandtemplates` and server preview. Default 60 / 600 per minute.

### A6. Instance-job subsidiaries follow the primary role

- **Date:** 2026-09-30
- **Source:** [0046](0046-primary-tenant.md) §5, §8
- **Change:** amends §1
- **Summary:** An instance job needs a subsidiary of an account on the tenant that is primary when the job is submitted (`ts-runjob` is an instance right for such jobs). When the role is transferred, `triplespace-cli primary accept` creates sync subsidiaries on the new primary; jobs already running finish under theirs.

Replaced text (§1):

> The instance's own sync and ingest jobs run as subsidiaries of the primary tenant's operators ([0018](0018-tenants.md) §4), created by `triplespace-cli` beside `local:1` ([0016](0016-permissions-and-access-control.md) §3).

### A7. The export class and a sysop row for jobs

- **Date:** 2026-10-01
- **Source:** [0047](0047-special-pages.md) §8, §12
- **Change:** amends §5
- **Summary:** A class `export` counts exports (default 30 / 300 per hour for `user` / `bot`). The `job` class gains a `sysop` row, 10 per hour, so that an administrator who is not a bot can start a Nuke or a job revert.

Replaced text (§5):

> | `job` | Bulk-job submissions ([0002](0002-source-graphs-and-mass-ingest.md) §8.1) | 0 / 10 per hour |

### A8. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–13
- **Summary:** A1–A7 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A3's signing keys and A4–A7 were blockquotes, and A2 had been written in place. The file before conversion is commit `0b26a3a`.

### A9. The `fork` rate class

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §8
- **Change:** extends §5
- **Summary:** Forks started count in a new class, 5 per hour for `user` and 100 for `bot`; the seeding runs under the repository's fetch budget, not the user's.

### A10. Key format, labels, login responses, bearer tokens

- **Date:** 2026-10-03
- **Source:** Direct: James, decision of 2026-10-03 (`triplespace-accounts`, `triplespace-api-action`, `triplespace-server`, `scatter-adapter-internetdomains`)
- **Change:** extends §4, §8
- **Summary:** A key ID is 16 random bytes and a secret 32, both URL-safe base64 without padding, presented as `{key ID}.{secret}`; the stored hash is the SHA-256 of the secret, a fast hash being right for a 256-bit random secret. A label is 1–64 ASCII letters, digits, `-`, `_` or `.`, unique per subsidiary, and never `@`. `action=login` answers in MediaWiki's shape: `{"login": {"result": "Success", "lguserid", "lgusername"}}`, `NeedToken` with a fresh `token` (an anonymous session is opened for it), `WrongToken`, or `Failed` with a `reason`; a primary account's name is `Failed` with `"code": "ts-use-oauth"`. A bearer request is stateless but still carries a CSRF token on writes (0056 §1): `meta=tokens` with a bearer header derives it from the key ID, as a session's is derived from the session ID. `meta=userinfo` for a subsidiary carries `operator` (the operator's actor key) and `grants` (the key's) as top-level fields. `triplespace subsidiary create|key|keys|revoke` are the operator's CLI, until the account page exists.

### A11. The `query` rate class

- **Date:** 2026-10-04
- **Source:** [0059](0059-query-service.md) §6
- **Change:** extends §5
- **Summary:** A `query` class, 30 / 300 per minute, counting SPARQL queries started and on-demand scope refreshes. Concurrency is bounded instance-wide by `query.max_concurrent` (0059 §6), which is not a rate limit.

### A12. One counter key form

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §5
- **Summary:** Rate-limit counters are `rl:{class}:{key}` here and in [0014](0014-caches-and-search.md) §4 alike, the key being the actor key or, for anonymous requests, the IP; §5 had written the form as `rl:{class}:{actor key}` and 0014 §4 differently. (PENDING C4)

Replaced text (§5):

> **Counters** are the `rl:{class}:{actor key}` keys of [0014](0014-caches-and-search.md) §4 in Valkey, keyed by IP for anonymous requests, with the window as TTL.

### A13. `createaccount` as in MediaWiki

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §11
- **Summary:** `createaccount` governs self-registration and creating an account for another, as in MediaWiki, and so also creating a subsidiary (§2); its default group is `universe`, and a private tenant removes it from `universe` ([0056](0056-security-model.md) §3 stands). §11's `autoconfirmed` default goes. A subsidiary a new user creates is `pending` until approved ([0025](0025-oauth-server.md) §3), which is what kept the right from needing a threshold. (PENDING C12)

Replaced text (§11):

> | `createaccount` | Creating a subsidiary (§2) | `autoconfirmed` |

### A14. The `federation` rate class

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §5
- **Summary:** ActivityPub fan-out has its own rate class, `federation`, counted per instance against the delivery queue (`ops.delivery`, [0021](0021-notifications.md) §8) and never against the posting actor; it joins the class table, and the `notify` class keeps only the requests an actor initiates ([0022](0022-federation.md) §7, 0022 A8). The counters sentence, which keyed every class by actor or IP, no longer holds for this class. (PENDING F13)

Replaced text (§5): the counters sentence as A12 quotes it.

### A15. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§13
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A16. `read` is counted lazily, and `maxlag` includes composition lag

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §1
- **Change:** extends §5
- **Summary:** The `read` rate class is counted for authenticated principals only once a per-process count of their reads has passed a fraction of the limit, and never for a response served from L0 or L1, so the cost of the class on a cached page is nothing; anonymous reads are counted by IP as before. The chapter also has inbound `maxlag` answer for the larger of replica lag and the tenant's local-partition composition lag ([0084](0084-wikibase-writes-against-the-resolved-view.md) §5), not for the actor's rate. (REVIEW G15)

### A17. Hand-made subsidiaries are never pending; the sync subsidiaries start in `bot`

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** corrects §2, §11; extends §1
- **Summary:** `pending` is an OAuth-only status: a subsidiary created on the OAuth consent page is pending until approved, and a subsidiary created by hand from `Special:Account` or `POST /account/subsidiaries` is a member of `user` from its first record, whoever its operator is and however new their account; "a subsidiary a new user creates is pending until approved" is struck. The MCP requirement is the `basic` grant, which every key and token carries. `triplespace-cli instance create` creates the instance's sync subsidiaries beside `local:1` and puts them in the `bot` group at creation, as `primary accept` does on a transfer. `view.actor` gains `editcount` and `created_at`, maintained by the activity projection from local `edit` rows, erased included and job rows excluded, from which the implicit `autoconfirmed` group is computed. (REVIEW G40)

Replaced text ([07](../architecture/07-actors-and-accounts.md) §4.2, as it stood):

> Temporary accounts cannot create subsidiaries; nor can subsidiaries. A subsidiary a new user creates is pending until approved (§4.3).

Replaced text ([07](../architecture/07-actors-and-accounts.md) §4.3, as it stood):

> **`pending` is an actor status** (§1.3). A subsidiary created in an OAuth authorization is `pending` instead until approved.

Replaced text ([09](../architecture/09-security-and-moderation.md) §8.4, as it stood):

> `userrights` covers approval of a pending subsidiary, and creating one in the consent page is `createaccount`; a subsidiary a new user creates is pending until approved.
