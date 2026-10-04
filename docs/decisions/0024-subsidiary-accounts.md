# 0024. Subsidiary accounts, API keys and rate limits

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-03 (A10)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

Every automated edit in the earlier ADRs is attributed to a **bot account** with an **operator**. [0007](0007-actor-identity.md) §6 makes a local bot a `prov:SoftwareAgent` that `prov:actedOnBehalfOf` its operator's account; [0010](0010-site-ui.md) §8 names the operator on the bot's contributions page; [0012](0012-api-requirements.md) §4 exposes it as `usprop=operator`; [0021](0021-notifications.md) §2 routes a bot's job and mention notifications to its operator; [0016](0016-permissions-and-access-control.md) §3 gives the `bot` group the bot flag and `ts-runjob`. None of them says where the operator relation is recorded, how a bot authenticates, or how fast it may edit.

Two things are missing, and the 2026-09-27 review named the first as a gap:

1. **Non-interactive authentication.** [0007](0007-actor-identity.md) §3 makes login a redirect to an identity provider, which a script cannot follow, and [0012](0012-api-requirements.md) §4 offers only that flow. Pywikibot editing `Project` pages is the acceptance test of [0008](0008-namespaces-and-document-pages.md) §12, and Pywikibot logs in with a bot password or an OAuth consumer. [0016](0016-permissions-and-access-control.md) §3 has `triplespace-cli` issue `owner` "a login token" and says nothing about anyone else.
2. **Rate limits.** Left open by [0012](0012-api-requirements.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md) and [0021](0021-notifications.md); [0014](0014-caches-and-search.md) §4 reserves `rl:` counters in Valkey for them without saying what they count.

MediaWiki's answer to the first is two mechanisms bolted on later: **bot passwords**, a per-account credential with a label, a restricted set of *grants* and optional IP ranges, logged in with `action=login` as `Name@label`; and **OAuth consumers**, for tools acting on a user's behalf. Both authenticate *the same account* the human uses, so a bot's edits are either mixed into a person's history or made from a second account the person registered by hand and that the wiki cannot tell is theirs. Wikimedia's bot policy then rebuilds the relation socially: the bot's user page must name its operator, and a bureaucrat grants the bot flag after review.

James's direction is to make the relation structural: a bot is a **subsidiary account**. `User:Example` creates `User:ExampleBot`; the subsidiary is a real account with its own history, its own user page and its own API key; the wiki knows who operates it; bureaucrats approve it for bot activity and can move it to another operator; and site policy sets how fast bots and non-bots may edit. Primary accounts hold no API keys. Automated editing is done by subsidiaries or not at all.

## Decision

### 1. A subsidiary is a local account with an operator (amends 0007 §4 and §6)

*Changed by A2, A6.*

A **subsidiary account** is a local account of kind `bot` ([0007](0007-actor-identity.md) §4) whose actor record carries one more attribute, the **operator**: the actor key of the local account that owns it. Everything else about it is an ordinary account: a sequential numeric ID ([0007](0007-actor-identity.md) §3), an actor IRI `{base}/user/{id}`, a name that is an attribute, a user page it owns ([0008](0008-namespaces-and-document-pages.md) §6), contributions of its own ([0010](0010-site-ui.md) §8), and memberships and blocks in the `actors` partition ([0016](0016-permissions-and-access-control.md) §3).

| | Primary account | Subsidiary account |
|---|---|---|
| Kind | `registered` | `bot` |
| Authenticates by | An identity provider, through a binding ([0007](0007-actor-identity.md) §3) | An API key (§4) or an OAuth token ([0025](0025-oauth-server.md) §3), and nothing else |
| Bindings | One or more | None. A subsidiary cannot log in interactively |
| API keys | **None** | One or more |
| Operator | — | A primary account of the same tenant |
| May own subsidiaries | Yes | No. One level only |
| Created by | Its holder, at first login ([0010](0010-site-ui.md) §10) | Its operator (§2), or an OAuth authorization on the operator's behalf ([0025](0025-oauth-server.md) §3) |

**The operator is an attribute, not a link.** It is written in the subsidiary's actor record and projected to `view.actor.operator`; [0007](0007-actor-identity.md) §6's `prov:actedOnBehalfOf` is emitted from it. It is not an account link in the sense of 0007 §7, which is opt-in and about one person's several accounts on several issuers: an operator relation is asserted at creation, public, and about control. Nor does it enter any identity cluster ([0004](0004-identity-clusters-and-equivalence.md)); actors never do.

**The operator relation is public.** A subsidiary's user page identity line reads "Bot operated by Example" ([0010](0010-site-ui.md) §2), its contributions header names the operator, and the operator's own contributions page lists their subsidiaries. This is what Wikimedia's bot policy asks operators to write on a user page by hand.

**Bot accounts are subsidiaries, and only subsidiaries.** A local account of kind `bot` always has an operator. The instance's own sync and ingest jobs run as subsidiaries of accounts on the tenant that is primary when the job is submitted ([0018](0018-tenants.md) §4, [0046](0046-primary-tenant.md) §5), created by `triplespace-cli` beside `local:1` ([0016](0016-permissions-and-access-control.md) §3); `ts-runjob` is an instance right for such jobs ([0046](0046-primary-tenant.md) §8). When the role is transferred, `triplespace-cli primary accept` creates sync subsidiaries on the new primary, and jobs already running finish under theirs. "The account that ran a job" ([0002](0002-source-graphs-and-mass-ingest.md) §8.3, [0011](0011-logs.md) §6.3) is therefore always a subsidiary, and its operator is always known.

### 2. Creating, approving and transferring

*Changed by A2.*

**Creation.** A primary account holding `createaccount` (§11) creates a subsidiary from `Special:Account` (§9) or `POST /account/subsidiaries` (§8). The request names the subsidiary; the instance appends its first actor record with kind `bot` and the creator as operator, mints its user ID, and projects `newusers/create2`, MediaWiki's existing action for an account created by another user, with the operator as performer and the subsidiary as target. The name follows the same rules as any account name ([0010](0010-site-ui.md) §10); the form suggests `{Operator}Bot`, and an instance may require a pattern in `site` configuration (`subsidiaries.name_pattern`, default none) and cap the number per operator (`subsidiaries.max_per_account`, default 10). Temporary accounts cannot create subsidiaries; nor can subsidiaries.

**Approval for bot activity is membership in `bot`.** A new subsidiary is a member of `user` like any registered account and nothing more (one created in an OAuth authorization is `pending` instead until approved, [0025](0025-oauth-server.md) §3). It edits under the operator's supervision at a non-bot rate (§5), its edits are not flagged, and they are patrolled like anyone's ([0023](0023-moderation.md) §6). When the community has reviewed it, a bureaucrat adds it to the `bot` group with `userrights`, which is already a `membership` record ([0016](0016-permissions-and-access-control.md) §3) projected as `rights/rights`. That is the bot flag: the `bot` right, `ts-runjob`, `autopatrol`, and the bot rate limits. Nothing in the software decides what "reviewed" means; a `Project:Bot requests` page or a thread is the community's, as on Wikimedia projects. An instance that wants every subsidiary flagged at creation puts `bot` in the default groups of new subsidiaries in `site` configuration.

**Transfer of ownership** is a new actor record for the subsidiary naming the new operator, written by a bureaucrat holding `ts-transferaccount` (§11) with the reason in the comment part. It projects as a new log event, `bot/transfer`, whose parameters are the old and new operator keys. History is untouched: every past edit stays attributed to the subsidiary, and the metadata graph emits `prov:actedOnBehalfOf` for the operator as of each revision's time, since actor records are ordered. The new operator must be a primary account of the same tenant and must have accepted, so the request is made by the receiving account and confirmed by the bureaucrat, or the reverse; either order writes one record.

**Vanishing.** A primary account with subsidiaries cannot vanish ([0007](0007-actor-identity.md) §4) until each has been transferred or **deactivated**: every key revoked and a new actor record with status `retired`, a status added to 0007 §4's list. A retired subsidiary keeps its history and its operator, and can be reactivated by its operator by issuing a key. Vanishing then proceeds; the placeholder record replaces the operator's name wherever the retired subsidiary shows it, as 0007 §4 already arranges for the vanished account's own history.

### 3. What a subsidiary inherits (amends 0016 §3)

**Permissions: nothing.** A subsidiary's effective permissions are the union over *its own* groups, minus its own blocks ([0016](0016-permissions-and-access-control.md) §3). An administrator's subsidiary is not an administrator; a bot that needs `delete` or `protect` is given those groups by a bureaucrat like any account, which is how Wikimedia handles adminbots.

**Blocks: the operator's, downward.** An actor's effective permissions now also subtract what the **operator's** active blocks remove, so blocking a person blocks their bots. This is the one addition to 0016 §3's rule that "a block is the only negative rule", and it adds no second kind of rule: the block record is the same, and evaluation reads one more actor's blocks. A block on a subsidiary does not reach its operator. `hideuser` ([0023](0023-moderation.md) §5) is likewise not inherited in either direction.

**Ownership rules** ([0016](0016-permissions-and-access-control.md) §2) gain two: the operator may edit the subsidiary's user pages, including its `json` and `yaml` subpages, and may issue and revoke its keys, retire and reactivate it, and rename it. Nobody else may, short of `renameuser`, `userrights` and `ts-transferaccount`.

### 4. API keys (extends 0007 §3)

*Changed by A2, A3, A10.*

**A key is a credential of a subsidiary and of nothing else.** A primary account cannot hold one, so revoking a key never touches a person's login, and every automated edit is attributable to a named agent with a named operator. This also settles the acceptance test of [0008](0008-namespaces-and-document-pages.md) §12: Pywikibot logs in as a subsidiary.

**Keys are private state** ([0007](0007-actor-identity.md) §3, [0013](0013-postgres-storage.md) §4). `private.api_key` holds, per key: the subsidiary's actor key; a **key ID**; a **label** chosen by the operator ("toolforge", "laptop"); the **hash** of the secret; its **grants** (below); optional **IP ranges**; created, expires and last-used times; and revoked-at. The secret itself is shown once at issue and stored only as a hash. Nothing about a key is ever a log record: a key ID is not content, but it is not provenance either, and the attestation of a record carries the actor key alone ([0006](0006-log-integrity-and-erasure.md) §3, [0015](0015-record-format-and-partition-registry.md) §1).

**Two ways to present a key**, both authenticating the same subsidiary with the same grants:

| Form | Where | Why |
|---|---|---|
| `Authorization: Bearer {key ID}.{secret}` | Every Action API and REST request | The native form. Stateless; no session |
| `action=login` with `lgname={subsidiary name}@{label}` and `lgpassword={secret}`, then the session cookie | Action API | MediaWiki's **bot password** shape ([mediawiki-compat.md](../api/mediawiki-compat.md) §5.1), so Pywikibot and every tool that speaks it work unchanged. The label selects the key; the secret is checked against its hash; the session records the key ID |

`action=clientlogin` and the OAuth redirect flow ([0012](0012-api-requirements.md) §4) are not offered to subsidiaries, and `action=login` is not offered to primary accounts; each form of authentication belongs to one kind of account. Tokens issued by the instance's own OAuth server ([0025](0025-oauth-server.md)) are, like keys, credentials of a subsidiary and never of a primary account.

**Grants** restrict what a key may do below what the subsidiary may do. The effective permissions of a request are the subsidiary's permissions (§3) **intersected with** the permissions its key's grants cover. Grants use MediaWiki's names where MediaWiki has them, so a Pywikibot user recognises them:

| Grant | Covers |
|---|---|
| `basic` | `read`, and every read route. Always included |
| `highvolume` | Bot rate limits (§5) and the `bot` flag, if the subsidiary holds `bot` |
| `editpage` | `edit` on document pages, threads and posts |
| `createeditmovepage` | `editpage` plus `createpage` and `move` |
| `editentity` *(Triplespace)* | `edit`, `item-term`, `property-term`, `item-redirect`, `item-merge` on entities; `ts-link` |
| `editprotected` | Edits admitted by protection ACLs the subsidiary's groups satisfy ([0023](0023-moderation.md) §1) |
| `patrol` | `patrol` |
| `delete`, `protect`, `blockusers` | The administrative permissions of those names |
| `viewdeleted` | `deletedhistory`, `deletedtext` |
| `ts-jobs` *(Triplespace)* | `ts-runjob`, `ts-revertjob`, `ts-viewrejects`, `ts-retain`, `ts-convert` |

A grant covers a permission only if the subsidiary holds it; a grant is never a way to gain one. The set is registry data (`docs/registry/grants.toml`, [0015](0015-record-format-and-partition-registry.md) §5), so an instance may add a grant for a permission it adds.

**Signing keys.** Beside its API keys, a subsidiary may hold one or more **signing keys**: Ed25519 keypairs the operator generates, whose public half is registered as a `scatter:v0/key` record in the `actors` partition ([0015](0015-record-format-and-partition-registry.md) §1) and whose private half the instance never sees. The operator registers, rotates and revokes them from the subsidiary's keys section and `POST /account/subsidiaries/{name}/signing-keys`; registration and revocation project as `key/register` and `key/revoke` log events ([0011](0011-logs.md) §6.1), public like the operator relation. A write submitted with a `signature` (as a field of the change set or page operation, or an `X-Triplespace-Signature` header on a REST write) is verified against the actor's current key before the append, and the record's attestation part carries it. A signature never grants anything and is never required by default; `site` configuration may require one for the `bot` group (`signing.require_for_bot`), which is what an instance that wants third-party-verifiable bot attribution turns on. History rows and "About this edit" ([0010](0010-site-ui.md) §5–6) show a *signed* mark with the key ID, and the record and proof routes ([0012](0012-api-requirements.md) §5) return the signature so a third party can check it against the export bundle. `triplespace-cli` and the reference SDK produce the canonical CBOR of the content and comment parts and sign it; Pywikibot does not, and works unsigned. Primary accounts hold no signing key, for the reason they hold no API key: a person's edits are attributed by session, a program's by credential.

**Revocation is immediate.** Revoking a key marks it revoked and invalidates every session opened with it, which is why sessions record the key ID. **Expiry** is optional; the account page warns before it. **IP ranges**, when set, are checked on every request, as bot-password restrictions are. **Last used** is written to `private` at most once a minute per key, as [0012](0012-api-requirements.md) §5 already treats binding use.

### 5. Rate limits (settles 0012, 0016, 0020 and 0021)

*Changed by A4, A5, A7, A9.*

**Rate limits are site policy, by action class and group.** They are `site` configuration ([0015](0015-record-format-and-partition-registry.md) §3) in the shape of MediaWiki's `$wgRateLimits`: for each **action class**, for each group, a count per window. The classes:

| Class | Counts | Default (`user` / `bot`) |
|---|---|---|
| `edit` | Local change sets and page and thread records | 90 / 3,000 per minute |
| `create` | New entities, pages and threads | 30 / 1,000 per minute |
| `move` | Page and thread renames and moves | 8 / 100 per minute |
| `link` | `same-as`, `different-from`, `equivalent-property` | 30 / 1,000 per minute |
| `job` | Bulk-job submissions ([0002](0002-source-graphs-and-mass-ingest.md) §8.1) | 0 / 10 per hour; `sysop` 10 per hour, so that an administrator who is not a bot can start a Nuke or a job revert ([0047](0047-special-pages.md) §12) |
| `read` | API requests of any kind, by key or session | 5,000 / 50,000 per minute |
| `stream`, `atom` | Open streams and Atom fetches ([0020](0020-change-feeds.md) §4) | 5 / 20 concurrent; 60 / 600 per hour |
| `upstream` | Live upstream fetches ([0012](0012-api-requirements.md) §6) | 30 / 30 per minute |
| `notify` | Outbound notification requests initiated by the actor: verification messages, handle registrations ([0021](0021-notifications.md) §5) | 5 / 5 per hour |
| `account` | Creating subsidiaries, issuing keys, login attempts | 10 / — per hour |
| `upload`, `renderfile`, `renderfile-nonstandard` | Uploads, per actor and per IP; thumbnails rendered on a miss, per IP; renders whose transform is not in `files.thumb_widths` ([0039](0039-files-and-media.md) §15) | per 0039 §15 |
| `parse` | Expansions a client asks for directly: `action=parse` with `text`, `action=expandtemplates` and server preview ([0042](0042-template-expansion-and-parsoid.md) §16) | 60 / 600 per minute |
| `export` | Exports ([0047](0047-special-pages.md) §8) | 30 / 300 per hour |
| `fork` | Forks started, each of which is a job that may fetch and write thousands of revisions ([0054](0054-forking-a-mirrored-page.md) §8) | 5 / 100 per hour |

The defaults are starting values, as [0014](0014-caches-and-search.md)'s TTLs are. `universe` and `temp` get limits below `user`'s; the `newbie` distinction MediaWiki draws is the `autoconfirmed` group here, which may carry its own row. **The most permissive limit among an actor's groups applies**, as in MediaWiki, and the `noratelimit` right (§11) exempts an actor from every class but `job`. **Which limit a bot gets is exactly what the `bot` group's row says**, so "how fast bots may edit versus non-bots" is one table an instance edits with `ts-config`, and an unapproved subsidiary edits at `user`'s rate.

**Counters** are the `rl:{class}:{actor key}` keys of [0014](0014-caches-and-search.md) §4 in Valkey, keyed by IP for anonymous requests, with the window as TTL. An instance without a shared cache counts in process, approximately, which is what the small profile of [0013](0013-postgres-storage.md) §11 accepts. A request over its limit is refused before anything is appended: HTTP 429 with `Retry-After`, and the Action API's `ratelimited` error. `maxlag` ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.8) is unchanged and orthogonal: it answers for replication lag, not for the actor's rate.

**Bulk jobs are limited at submission, not per record.** A job's throughput is the ingester's ([0002](0002-source-graphs-and-mass-ingest.md) §8.6); what a subsidiary is limited in is how many jobs it may start, so a runaway script cannot queue a thousand.

### 6. Jobs, attribution and notifications (amends 0002 §8.3; extends 0021 §2)

A job's **actor** is the subsidiary that submitted it; its **operator** is read from the actor record, and `requested_by` ([0012](0012-api-requirements.md) §3, [0011](0011-logs.md) §6.3) is kept only for the case where an administrator runs a job on someone else's behalf. The activity row's `operator` field ([0012](0012-api-requirements.md) §3) is filled for every row whose actor is a subsidiary, not only for jobs. In the metadata graph the subsidiary is a `prov:SoftwareAgent` with `prov:actedOnBehalfOf` its operator as of the revision's time, as [0007](0007-actor-identity.md) §6 already specifies.

[0021](0021-notifications.md) §2's routing stands: a mention of a subsidiary and the `job` reason for its jobs reach the operator's inbox, and a subsidiary has no inbox of its own. A `rights` notification for a subsidiary, its approval or a block, also goes to the operator. A `talk` message on the subsidiary's user talk page reaches the operator, since that is who can answer it.

### 7. Tenants (uses 0018 §4 and §10)

Identity is per tenant, so a subsidiary belongs to the tenant its operator belongs to, and a bot that works on two tenants of a farm is two subsidiaries with two operators and two sets of keys. There is no cross-tenant bot, as there is no cross-tenant account. When a tenant moves ([0018](0018-tenants.md) §10), the operator relation travels in the actor records and keys do not, since nothing private does; the operator reissues keys after reclaiming their own account.

### 8. API (extends 0012 §4 and §5)


*Changed by A10.*
**Action API**, additively under [0012](0012-api-requirements.md) §1:

| Module | Behaviour |
|---|---|
| `action=login` | Bot-password form only (§4). A primary account's name is refused with `ts-use-oauth`; a wrong label or secret is `Failed` |
| `meta=userinfo` | `uiprop=ratelimits` reports the caller's limits by class (MediaWiki's shape); a subsidiary's `userinfo` carries `operator` and the key's `grants` |
| `list=users` | `usprop=operator` ([0012](0012-api-requirements.md) §4) and a new `usprop=subsidiaries`, listing a primary account's subsidiaries |
| `meta=siteinfo` | `siprop=triplespace` reports the rate-limit classes and the grant set |
| Every write module | `ratelimited` when over a limit (§5); `permissiondenied` names the missing permission or grant |

**REST**, under `rest.php/triplespace/v0`, holder only except where noted:

| Route | Meaning |
|---|---|
| `GET /account/subsidiaries`, `POST /account/subsidiaries` | List and create (§2) |
| `POST /account/subsidiaries/{name}/rename`, `/retire`, `/reactivate` | The operator's ownership actions (§3) |
| `GET /account/subsidiaries/{name}/keys`, `POST …/keys`, `DELETE …/keys/{id}` | Keys: labels, grants, IP ranges, expiry, last used; issue (the secret is in this response and never again); revoke (§4) |
| `POST /actor/{key}/transfer` | Bureaucrat: transfer with the new operator's acceptance (§2) |
| `GET /actor/{key}` | Gains `operator` for subsidiaries and `subsidiaries` for primary accounts (public) |

`/account/subsidiaries` is `no-store` like everything under `/account` ([0012](0012-api-requirements.md) §8), and the privacy test of 0012 §8 gains the rule that no route returns a key's hash, and no route but the issuing `POST` returns a secret.

### 9. Site UI (extends 0010 §8 and §11)

- **`Special:Account`** gains a **Subsidiaries** section. The list of one's subsidiaries is **Public** ([0010](0010-site-ui.md) §1, principle 6), since the operator relation is; each subsidiary's **keys** are **Private**: label, grants, IP ranges, expiry, last used, Revoke, and Issue key, which shows the secret once with a warning. Retire and Rename sit with each subsidiary; Create subsidiary suggests `{Name}Bot`.
- **A subsidiary's user page** shows "Bot operated by {Name}" in the identity line ([0010](0010-site-ui.md) §2), with a Bot chip when it holds `bot`, and a **Request bot approval** link that goes wherever `site` configuration points (`subsidiaries.request_page`, a project page or a talk page).
- **Contributions** ([0010](0010-site-ui.md) §8): a subsidiary's header names and links its operator; a primary account's header lists its subsidiaries.
- **`Special:UserRights`** on a subsidiary shows its operator and the approval history; **`Special:TransferAccount`** (new) is the bureaucrat's transfer form.
- **`Special:ListUsers&group=bot`** is the bot list, as on MediaWiki.

### 10. Storage (extends 0013 §4 and §5)

- **`view.actor`** gains `operator text` (a local actor key, null for primary accounts) and the status value `retired`.
- **`private.api_key`** as §4 describes: `(actor_key, key_id, label, hash, grants text[], ip_ranges cidr[], created, expires, last_used, revoked_at)`, unique on `(actor_key, label)`. Read by `triplespace-accounts` only; sessions in Valkey ([0014](0014-caches-and-search.md) §2) carry `key_id` so revocation can find them.
- **Rate-limit configuration** is the `site` config records of §5, projected into `view.registry` like every setting; **counters** are Valkey keys, never rows.
- **`docs/registry/grants.toml`** lists the grants of §4 and the permissions each covers; `scatter-actors` embeds it.

### 11. Permissions (extends 0016 §2)

| Permission | Governs | Default groups |
|---|---|---|
| `createaccount` | Creating a subsidiary (§2) | `autoconfirmed` |
| `ts-transferaccount` *(new)* | Transferring a subsidiary to another operator (§2) | `bureaucrat` |
| `noratelimit` | Exemption from every rate-limit class but `job` (§5) | `sysop` |
| `bot` | As [0016](0016-permissions-and-access-control.md) §2: the bot flag; now also the `bot` row of the rate-limit table | `bot` |

`userrights` already covers approval. `docs/registry/groups.toml` is updated with the three permissions and the `bot` group's description.

### 12. Scatterbase

*Changed by A3.*

Scatterbase's decision record wants client signing keys bound to actors ([0007](0007-actor-identity.md) Q5). A subsidiary is the actor such a key binds to: `private.api_key` is the *authentication* half, and the signing keys of §4, registered as `key` records whose signature travels in the record ([0015](0015-record-format-and-partition-registry.md) §1), are the *attestation* half; Scatterbase's client signature is the same field shape in its own part layout. The grants table and the operator attribute are Scatterbase's to reuse; rate limits are Triplespace's.

### 13. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed. The table this section first gave is in A1.

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
