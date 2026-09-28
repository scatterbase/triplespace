# 0025. The instance as an OAuth server

- **Status:** Proposed
- **Date:** 2026-09-27
- **Author:** James Hare / Claude Fable
- **Related:** [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§11 amends §2), [0007 — Actor identity](0007-actor-identity.md) (§3 amends §4: the `pending` status; §3 of that ADR is how a person logs in, this ADR is how a tool acts), [0010 — Site UI](0010-site-ui.md) (§5 extends §11), [0011 — Upstream and local logs](0011-logs.md) (§5 extends §6.1), [0012 — API requirements for the site UI](0012-api-requirements.md) (§9 extends §4 and §5), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§8 extends §4 and §5.6), [0014 — Cache layers and search](0014-caches-and-search.md) (§8 uses §2 and §4), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (§2 extends §3 with the `consumer` and `consumer-policy` kinds), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§3 refines §3; §10 extends §2), [0018 — Tenants](0018-tenants.md) (§7 follows §4), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§3 extends §2 and refines §4; §4 settles its per-key-tag question; §6 uses §5; this ADR is the delegated-tool case its open questions leave), [0028 — Tenancy policy](0028-tenancy-policy.md) (§7 uses §8), [0030 — Edit filters](0030-edit-filters.md) (§4 writes the tag §5 reserves), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

[0024](0024-subsidiary-accounts.md) gives every bot a subsidiary account and an API key, and rules that a primary account holds no key. It leaves out **third-party tools acting for a person**: QuickStatements, OpenRefine, the Wikidata game and its descendants, and every other application a person signs into with their own account and then uses to make edits. On Wikimedia projects these use the OAuth extension. The tool is a registered **consumer**; the person authorizes it once; the tool then edits *as the person*, and the only trace of the tool is a change tag, `OAuth CID: n`, on each edit. A QuickStatements batch of ten thousand edits sits in a person's contributions between their hand edits, made under the same account, with the same rights and the same rate limit.

James has set one rule in advance: **a tool's edits are attributed to a subsidiary, never to the primary account.** Even when a person authorizes QuickStatements with their own login, what QuickStatements acts as is one of that person's subsidiaries. The distinction 0024 draws, human edits under a primary account and automated edits under a subsidiary, holds for delegated tools as it holds for scripts, and a primary account never holds a credential a program can use, whether that credential is an API key or an OAuth token.

The second rule follows from the first: **using a semi-automated tool requires approval.** On Wikimedia projects the bot policy asks that large semi-automated runs be approved, and QuickStatements batches routinely are not. Here the subsidiary a tool acts as is created *in the authorization flow* and is **pending** until a bureaucrat approves it, so the approval that Wikimedia asks for socially is the same `userrights` action 0024 already uses for the bot flag. A tenant that does not want this can turn it off; the default is that it is on.

This ADR makes the instance an OAuth 2.0 authorization server under those two rules. It does not make the instance an identity provider for other sites: logging *in* remains [0007](0007-actor-identity.md) §3, and this ADR is only about tools acting *on* the instance.

## Decision

### 1. An OAuth 2.0 authorization server, at MediaWiki's paths

The instance is an **OAuth 2.0 authorization server** ([RFC 6749](https://www.rfc-editor.org/rfc/rfc6749)) offering:

| Flow | For | Notes |
|---|---|---|
| Authorization code with PKCE ([RFC 7636](https://www.rfc-editor.org/rfc/rfc7636)) | Web tools with a redirect URI | The only flow for public clients; confidential clients may also present a client secret |
| Device authorization ([RFC 8628](https://www.rfc-editor.org/rfc/rfc8628)) | Command-line and desktop tools | The person enters a short code on `Special:OAuth/device` |
| Refresh token | Both | Refresh tokens rotate on use; a reused refresh token revokes the whole authorization |
| Token revocation ([RFC 7009](https://www.rfc-editor.org/rfc/rfc7009)) | Both | Also from the account page (§5) |

The client-credentials flow is **not offered**: a tool acting for nobody is a bot with an API key ([0024](0024-subsidiary-accounts.md) §4), and MediaWiki's owner-only consumers have no counterpart here. Implicit grant and resource-owner-password are not offered either.

**Tokens** are opaque bearer strings ([RFC 6750](https://www.rfc-editor.org/rfc/rfc6750)) in the shape of 0024's keys, `{token ID}.{secret}`, presented as `Authorization: Bearer` on any Action API or REST request. Token IDs carry a prefix that distinguishes them from key IDs so the authentication layer knows which table to check; from there a token and a key are the same thing, a credential of a subsidiary with grants (§3). Access tokens live four hours, refresh tokens until revoked or unused for a year; both are `site` settings (`oauth.access_ttl`, `oauth.refresh_ttl`). Tokens are not JWTs: an opaque token is revocable at once, and nothing about a subsidiary needs to be readable from the token without asking the instance.

**Endpoints.** Native paths, and the paths of MediaWiki's OAuth extension so that tools written for it work by changing only the host:

| Native | MediaWiki-compatible | Purpose |
|---|---|---|
| `/.well-known/oauth-authorization-server` | — | Metadata ([RFC 8414](https://www.rfc-editor.org/rfc/rfc8414)): endpoints, PKCE methods, the grant names as `scopes_supported` |
| `/oauth/authorize` | `/w/rest.php/oauth2/authorize` | Consent page (§3); requires an interactive session of a **primary** account |
| `/oauth/token` | `/w/rest.php/oauth2/access_token` | Code, device and refresh exchanges |
| `/oauth/device` | — | Device authorization request |
| `/oauth/revoke` | — | Revocation by the client |
| `/oauth/identify` | `/w/rest.php/oauth2/resource/profile` | Who the token acts as (below) |

**`/oauth/identify`** returns the **subsidiary** the token acts as, and its operator: `sub` (the subsidiary's user ID, [0007](0007-actor-identity.md) §3), `username` (its name), `groups`, `rights` (its effective permissions), `grants` (the token's), `blocked`, `registered`, and an `operator` object with the primary account's `sub` and `username`. The MediaWiki-compatible `profile` route returns the same fields in the extension's shape, so a tool that greets the person by `username` greets the subsidiary; a tool that wants the person's own name reads `operator.username`. There is no `email` or `realname` field: the instance holds neither ([0007](0007-actor-identity.md) §3, [0021](0021-notifications.md) §3), and a tool that needs to reach the person notifies the subsidiary, which routes to the operator ([0024](0024-subsidiary-accounts.md) §6).

**OAuth 1.0a is not offered** in this ADR. Pywikibot and several older tools speak only 1.0a; Pywikibot also speaks bot passwords, which are 0024's keys. Whether a 1.0a shim is worth its signing code is an open question below.

### 2. Consumers are instance configuration (extends 0015 §3)

A **consumer** is a registered tool. Its record is a `config` record of kind **`consumer`** in the **instance** `config` partition ([0015](0015-record-format-and-partition-registry.md) §3, [0028](0028-tenancy-policy.md) §1), keyed `consumer:{slug}`, holding: the slug; a display name and description; the developer's actor key (its **owner**); one or more redirect URIs, or `device` for a device-flow-only tool; whether the client is public or confidential; the **grants** it requests, from `docs/registry/grants.toml` ([0024](0024-subsidiary-accounts.md) §4); a callback-is-prefix flag in the manner of MediaWiki's; and a **status**: `proposed`, `approved`, `disabled` or `rejected`. Config records are append-only, so a consumer's history is its record history and a change of redirect URI or grants is a new record.

**Registered once per instance, because a consumer is one thing.** Its redirect URI, its secret and its developer do not vary by tenant, and a developer should not register QuickStatements forty times on a forty-tenant farm. **Approval is also instance-level**: a holder of `mwoauthmanageconsumer` (§10) moves a consumer from `proposed` to `approved`, `rejected` or `disabled`, and only an approved consumer may be authorized by anyone. Confidential clients' secrets are hashed into `private.oauth_consumer` (§8) and shown once at registration; the owner may rotate them.

**Enablement is per tenant.** A tenant `config` record of kind **`consumer-policy`**, in the shape of [0026](0026-sitelinks.md) §3's lists, holds `allow` and `deny` lists of consumer slugs and, per slug, an optional `auto_approve` flag (§3). With no record, every approved consumer is available and none is auto-approved. A farm may write a `consumer-policy` template or lock it ([0028](0028-tenancy-policy.md) §8), which is how an enterprise limits its tenants to the tools it has vetted.

A consumer whose owner vanishes ([0007](0007-actor-identity.md) §4) is disabled; a disabled consumer's tokens stop working at once (§5). `Special:OAuthConsumers` lists consumers, their status and their requested grants, and is where registration and approval happen (§5).

### 3. Authorization selects or creates a subsidiary, and a created one is pending (amends 0007 §4; extends 0024 §2; refines 0024 §4)

The consent page at `/oauth/authorize` is shown to a **primary account** with an interactive session; a subsidiary cannot reach it, since it cannot log in ([0024](0024-subsidiary-accounts.md) §1), and a temporary account is refused. It shows the consumer's name and description, the grants requested, intersected with what the consumer was approved for, and **which subsidiary the tool will act as**:

- **An existing subsidiary** of the person, offered if it is **approved** (below) and not retired. The tool then acts as an account the community has already reviewed, at whatever rate its groups allow.
- **A new subsidiary**, created here. The form suggests `{Operator}-{slug}` (`Example-quickstatements`), subject to the same name rules and `subsidiaries.name_pattern` and `subsidiaries.max_per_account` as [0024](0024-subsidiary-accounts.md) §2, and creation requires `createaccount` as there. The instance appends the actor record with kind `bot`, the person as operator, and status **`pending`**, and projects `newusers/create2` as for any subsidiary.

The person may also narrow the grants below what the consumer asked for, as MediaWiki's consent page allows. What results is an **authorization**: a refresh token and its first access token, both **credentials of the chosen subsidiary**, with **grants** = requested ∩ approved ∩ consented, stored in `private.oauth_token` (§8) beside the subsidiary's keys. The **effective permissions** of a request made with a token are the subsidiary's permissions ([0024](0024-subsidiary-accounts.md) §3) intersected with the permissions the token's grants cover, the rule 0024 §4 already applies to keys. A grant never adds a permission the subsidiary lacks.

**`pending` is a new actor status** ([0007](0007-actor-identity.md) §4, beside 0024's `retired`). A pending subsidiary exists, owns its user page, shows "Bot operated by Example (pending approval)" on it, and can read as `*` reads, but its **implicit membership in `user` is withheld**: its effective permissions are those of `*` and nothing more, whether the request comes with a token or with a key its operator issued. **Approval is `userrights`**: a bureaucrat adds the subsidiary to a group with an explicit `membership` record ([0016](0016-permissions-and-access-control.md) §3), projected `rights/rights`, and the first such record ends the pending status. The natural groups are `user`, meaning approved to act at a non-bot rate and patrolled like anyone, and `bot`, meaning approved and flagged, which is 0024 §2's approval in one step. A subsidiary is **approved** when it holds an explicit membership in any group; `user` may be granted explicitly for this purpose, and only for this purpose. A subsidiary created by hand under 0024 §2 is a member of `user` implicitly and is not pending, as before; it becomes eligible for the picker above when a bureaucrat has approved it in the same way, so the approval requirement cannot be sidestepped by creating the account first and binding the tool to it second.

**While pending**, a write made with the subsidiary's token or key is refused with `oauth-pending` (Action API) or HTTP 403 with that code, and the response names `Special:PendingSubsidiaries`. Reads work. The consumer receives its tokens at authorization regardless, so a tool can complete its login and show the person that approval is awaited, rather than failing in the middle of the flow. `Special:PendingSubsidiaries` lists pending subsidiaries with operator, consumer and date for holders of `userrights`; a `Project:Bot requests` page or thread remains the community's, as in 0024. A bureaucrat who declines **retires** the subsidiary ([0024](0024-subsidiary-accounts.md) §2), which also revokes its tokens (§5); the operator may retire it too. Pending subsidiaries unapproved after `subsidiaries.pending_ttl` (default 90 days) are retired by `triplespace-accounts` on a schedule, with the retirement record's comment saying so.

**A tenant may relax this.** The `site` setting **`subsidiaries.oauth_requires_approval`** (default `true`) makes an OAuth-created subsidiary pending; with `false`, it is created as 0024 §2 creates one, a member of `user`, active at once, and the picker offers every unretired subsidiary. Per consumer, a tenant may set `auto_approve` in its `consumer-policy` (§2), which has the same effect for that consumer only, so a tenant can wave through a reference-fixing gadget and still gate QuickStatements. Neither setting touches the bot flag: `bot` is a bureaucrat's decision in every case.

**One authorization per (subsidiary, consumer).** Authorizing the same consumer again for the same subsidiary replaces the earlier tokens. A subsidiary may be bound to several consumers, each with its own tokens and grants; the tag of §4 keeps their edits apart.

### 4. Attribution and the consumer tag (extends 0030 §5; settles 0024's per-key-tag question)

Every record written with a token is attributed to the **subsidiary**, with `prov:actedOnBehalfOf` its operator as for any subsidiary ([0007](0007-actor-identity.md) §6, [0024](0024-subsidiary-accounts.md) §6). The attestation part of the record ([0015](0015-record-format-and-partition-registry.md) §1, as [0030](0030-edit-filters.md) §5 widened it) carries the change tag **`oauth:{slug}`**, so a contributions page can show and filter which tool made an edit and the metadata graph can emit it. This is the tag 0030 §5 reserves for this ADR, and it corresponds to MediaWiki's `OAuth CID: n` with the slug in place of the number.

**API keys carry no tag.** 0024 left open whether a key's label should appear on the edits it made. It should not: a label is private state chosen by the operator for their own bookkeeping, and a tag is public and enters records. A consumer's slug is different in kind, since the consumer is registered, approved and listed publicly. An operator who wants to tell deployments apart uses one subsidiary per deployment, which is what subsidiaries are for.

The tag is written by the request path, not by the client: a client cannot omit it, add it to a request made with a key, or supply another consumer's slug. Edit filters ([0030](0030-edit-filters.md) §2) see the tag in `tags` and `user_kind = bot`, so a filter can throttle or warn on a particular tool. `Special:Contributions` gains a tag filter row for `oauth:*`, and `Special:RecentChanges` already filters by tag ([0010](0010-site-ui.md) §7).

### 5. Management, revocation and log events (extends 0010 §11 and 0011 §6.1)

**Connected applications**, a section of `Special:Account` ([0010](0010-site-ui.md) §11), Private, lists each authorization the person's subsidiaries hold: consumer, subsidiary, grants, authorized and last-used dates, and **Revoke**. A subsidiary's own user page gains no such list; authorizations are private state of the operator. `Special:OAuthConsumers` is the consumer registry: a developer registers and updates their own; a holder of `mwoauthmanageconsumer` approves, rejects and disables; everyone reads it.

**Revocation is immediate**, as for keys: revoking an authorization marks its tokens revoked and drops every session opened with them, which is why sessions record the token ID ([0024](0024-subsidiary-accounts.md) §4). Tokens are also revoked when the subsidiary is retired or transferred ([0024](0024-subsidiary-accounts.md) §2, since the new operator did not consent), when the operator is blocked in a way that removes `edit` (they simply fail, since blocks reach subsidiaries), when the consumer is disabled, and when the operator vanishes, which requires the subsidiary to have been retired or transferred first anyway.

**Log events.** The consumer lifecycle is public and is logged: `oauth/propose`, `oauth/update`, `oauth/approve`, `oauth/reject`, `oauth/disable`, with the consumer slug as target and the reason in the comment part, written to the primary tenant's `log` partition, or `log/{farm}` where [0028](0028-tenancy-policy.md) §2 creates one. **Authorizations and revocations are not logged**: an authorization is private state, like the issue of a key ([0024](0024-subsidiary-accounts.md) §4), and nothing about a token is ever a log record. What is public is what the subsidiary then does, and its creation and approval, which `newusers/create2` and `rights/rights` already cover.

### 6. Rate limits (uses 0024 §5)

A request made with a token is limited as the **subsidiary** is limited: its groups pick the row of the rate-limit table, so an approved-but-unflagged tool account edits at `user`'s rate and a community that has flagged one as `bot` gets bot rates for it. Authorization attempts, consent, and device-code entry count under the **`account`** class against the primary account; token refreshes count under `read` against the subsidiary. There is no per-consumer limit: a consumer is code, and what is limited is the account it acts as.

### 7. Tenants (follows 0018 §4 and 0024 §7; uses 0028 §8)

A token is a credential of one subsidiary, and a subsidiary belongs to one tenant ([0024](0024-subsidiary-accounts.md) §7), so a token acts in one tenant. A person who uses a tool in two tenants authorizes it twice, once on each tenant's host, and gets two subsidiaries or chooses two existing ones. The consent page is served on the tenant host where the person is logged in, and the tokens work only against that tenant's API; the same consumer record serves both. On a farm with a shared account database ([0028](0028-tenancy-policy.md) §2) this is unchanged: the farm actor logs in, but the subsidiary created is a tenant actor with the farm actor's tenant account as operator.

Consumer registration and approval are instance-level (§2), so under the `isolated` preset the instance operator approves consumers and every tenant may then allow or deny them; under `community`, holders of `mwoauthmanageconsumer` in a global group do; under `enterprise`, the operator approves and locks a `consumer-policy` template.

### 8. Storage (extends 0013 §4 and §5.6; uses 0014 §2 and §4)

- **`view.actor.status`** gains the value `pending`.
- **Consumers** are `config` records and project into `view.registry` like every other setting. **`private.oauth_consumer`** `(slug, secret_hash, created, rotated_at)` holds confidential clients' secrets.
- **`private.oauth_token`** `(actor_key, token_id, consumer_slug, grants text[], refresh_hash, access_hash, issued, access_expires, refresh_expires, last_used, revoked_at)`, unique on `(actor_key, consumer_slug)`. Read and written by `triplespace-accounts`, which stores tokens as it stores keys; `triplespace-oauth` (§11) speaks the protocol and calls it, so the set of crates that read `private` is unchanged. `last_used` is written at most once a minute per token, as for keys.
- **Authorization codes and device codes** are Valkey keys, `oauth:code:{hash}` and `oauth:device:{hash}`, with the ten-minute TTL as their expiry ([0014](0014-caches-and-search.md) §2); they are never rows. An instance without a shared cache holds them in process, which the small profile accepts. Sessions opened with a token carry `token_id` beside 0024's `key_id`.
- **The `oauth:{slug}` tag** is in the attestation part of each record it applies to ([0030](0030-edit-filters.md) §11); no table is added for it.

Nothing here enters a header ([0006](0006-log-integrity-and-erasure.md) §3): a token, a consumer and a grant are operational state, and the attestation names the subsidiary only.

### 9. API (extends 0012 §4 and §5)

The endpoints of §1, at both paths. Additionally:

- **Action API.** `action=login` and `action=clientlogin` are unchanged. `meta=userinfo` for a token-authenticated request returns the subsidiary, with `uiprop=operator` ([0024](0024-subsidiary-accounts.md) §8) and a new `uiprop=oauth` giving the consumer slug and the token's grants. `action=query&list=oauthconsumers` lists consumers; `action=oauthconsumer` with `do=propose|update|approve|reject|disable` manages them, under §10's permissions.
- **REST.** `GET/POST /oauth/consumers`, `GET/PATCH /oauth/consumers/{slug}`, `POST /oauth/consumers/{slug}/status`; `GET /account/authorizations` and `DELETE /account/authorizations/{subsidiary}/{slug}` for the account page; `GET /account/pending-subsidiaries` for `Special:PendingSubsidiaries`.
- **Refusals.** `oauth-pending` (§3); `oauth-consumer-disabled`; `oauth-not-allowed` when a tenant's `consumer-policy` denies the consumer; and the standard OAuth error responses at the token endpoint.
- **`meta=siteinfo&siprop=triplespace`** reports whether `subsidiaries.oauth_requires_approval` is on, so a tool can tell the person in advance that its account will await approval.

`docs/api/mediawiki-compat.md` gains a section on the OAuth 2.0 routes and their MediaWiki paths, noting the 1.0a gap.

### 10. Permissions (extends 0016 §2; refines 0016 §3)

| Permission | Governs | Default groups |
|---|---|---|
| `mwoauthproposeconsumer` | Registering a consumer (§2) | `autoconfirmed` |
| `mwoauthupdateownconsumer` | Updating and rotating the secret of a consumer one owns (§2) | `autoconfirmed` |
| `mwoauthmanageconsumer` | Approving, rejecting and disabling consumers (§2) | `bureaucrat` |
| `mwoauthmanagemygrants` | Viewing and revoking authorizations of one's own subsidiaries (§5) | `user` |
| `mwoauthviewprivate` | Viewing any account's authorizations and token metadata, for abuse investigation (§5) | `sysop` |

The names are MediaWiki's so a Wikimedia administrator recognises them. `mwoauthsuppress` and `mwoauthviewsuppressed` are not offered: consumer records are `config`, and hiding one is disabling it. Approval of a pending subsidiary is `userrights`, as in 0024, and creating one in the consent page is `createaccount`. `docs/registry/groups.toml` gains the five permissions.

The one refinement to 0016 §3's evaluation is §3's: an actor with status `pending` does not receive the implicit `user` membership.

### 11. Crates (amends 0005 §2)

| Layer | Crate | Change |
|---|---|---|
| Substrate | `scatter-actors` | The `pending` status and its effect on implicit membership (§3); the grant-intersection rule already exists for keys and is reused unchanged |
| | `scatter-mwlog` | The `oauth/*` actions (§5) |
| Triplespace | `triplespace-oauth` *(new)* | The authorization server: metadata, authorize, token, device, revoke and identify endpoints at both paths; PKCE; consent page; consumer registry and policy evaluation; the `oauth:{slug}` tag on the request path (§1–4). Depends on `triplespace-accounts` for every read or write of `private` and on `scatter-actors` for permissions |
| | `triplespace-accounts` | `private.oauth_token` and `private.oauth_consumer`; token authentication beside key authentication; session–token binding; revocation on retirement and transfer (§5, §8) |
| | `triplespace-api-action`, `triplespace-api-rest` | §9; the `oauth-pending` refusal in front of every write |
| | `triplespace-server` | `Special:OAuthConsumers`, `Special:PendingSubsidiaries`, `Special:OAuth/device` and the Connected applications section, served from `triplespace-oauth` (§5) |

The workspace goes from forty-three crates to forty-four.

### 12. Scatterbase

Nothing here is Scatterbase's. A crawler has no users to delegate for. Consumer records and grants are ordinary `config` and registry data and need no Scatterbase reader.

## Consequences

- **No tool ever edits as a person.** A person's contributions are theirs alone; every tool's edits are a subsidiary's, named and operated, and tagged with the tool. This is what 0024 promised for scripts, extended to the case where the person is present.
- **Semi-automated editing requires approval by default**, through the same `userrights` action that already grants the bot flag, so a community reviews QuickStatements accounts the way it reviews bots, and can wave individual tools through per tenant when it trusts them.
- **A tool's failure mode is a pending account, not a rejected login.** Authorization completes; writes wait; the tool can say so.
- **Tokens and keys are one mechanism** in the request path: a credential of a subsidiary, with grants, checked by hash, revocable at once, never in a header.
- **One registration serves a farm**, and each tenant chooses what to allow, which fits all three tenancy presets without a switch of their own.
- **Wikimedia-written tools need a host change**, not a rewrite, provided they speak OAuth 2.0; 1.0a tools do not work until the open question is settled.
- **One more crate, two `private` tables, one status, five permissions**, and no new log partition or record kind beyond two `config` kinds.

## Open questions

- **OAuth 1.0a shim.** Whether to serve `Special:OAuth/initiate|authorize|token` with HMAC-SHA1 signing for Pywikibot's OAuth login and other 1.0a-only tools, or to point them at bot passwords ([0024](0024-subsidiary-accounts.md) §4) and leave 1.0a unimplemented. The signing code is small; the cost is a second token shape to revoke and audit.
- **OpenID Connect.** Whether `/oauth/identify` should grow into an OIDC `userinfo` with an ID token, so that other sites can offer "log in with this instance". Deliberately outside this ADR; it would make the instance an identity provider, and [0007](0007-actor-identity.md) §3's choice was to consume providers, not to be one.
- **Consent for grant changes.** When a consumer's approved grants widen, whether existing authorizations keep their narrower grants (as here) or the person is asked again on next use.
- **Auto-approval by track record.** Whether a tenant should be able to auto-approve a subsidiary whose operator already has an approved one, or whose operator holds a given group, rather than only per consumer.
- **Pending-subsidiary notifications.** Whether creation of a pending subsidiary should notify bureaucrats through [0021](0021-notifications.md), as a `rights`-like event, or only appear on `Special:PendingSubsidiaries`.
- **Subsidiary name suggestion**: `{Operator}-{slug}` versus `{Operator}Bot` for all tools, pending 0024's name-pattern question.

## References

- [RFC 6749 — OAuth 2.0](https://www.rfc-editor.org/rfc/rfc6749), [RFC 6750 — Bearer token usage](https://www.rfc-editor.org/rfc/rfc6750), [RFC 7636 — PKCE](https://www.rfc-editor.org/rfc/rfc7636), [RFC 8628 — Device authorization grant](https://www.rfc-editor.org/rfc/rfc8628), [RFC 7009 — Token revocation](https://www.rfc-editor.org/rfc/rfc7009), [RFC 8414 — Authorization server metadata](https://www.rfc-editor.org/rfc/rfc8414)
- [Extension:OAuth](https://www.mediawiki.org/wiki/Extension:OAuth), [OAuth/For Developers](https://www.mediawiki.org/wiki/OAuth/For_Developers) (the `rest.php/oauth2/*` paths, the `profile` resource, grants as scopes), [OAuth/Owner-only consumers](https://www.mediawiki.org/wiki/OAuth/Owner-only_consumers)
- [Help:QuickStatements](https://www.wikidata.org/wiki/Help:QuickStatements), [OpenRefine: Wikibase reconciliation and editing](https://openrefine.org/docs/manual/wikibase/overview)
- [Wikidata:Bots](https://www.wikidata.org/wiki/Wikidata:Bots) (semi-automated editing and approval)
- [0007 — Actor identity](0007-actor-identity.md), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md)
