# 0025. The instance as an OAuth server

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A6)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0024](0024-subsidiary-accounts.md)
- **Uses:** [0018](0018-tenants.md), [0028](0028-tenancy-policy.md), [0030](0030-edit-filters.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

## Context

[0024](0024-subsidiary-accounts.md) gives every bot a subsidiary account and an API key, and rules that a primary account holds no key. It leaves out **third-party tools acting for a person**: QuickStatements, OpenRefine, the Wikidata game and its descendants, and every other application a person signs into with their own account and then uses to make edits. On Wikimedia projects these use the OAuth extension. The tool is a registered **consumer**; the person authorizes it once; the tool then edits *as the person*, and the only trace of the tool is a change tag, `OAuth CID: n`, on each edit. A QuickStatements batch of ten thousand edits sits in a person's contributions between their hand edits, made under the same account, with the same rights and the same rate limit.

James has set one rule in advance: **a tool's edits are attributed to a subsidiary, never to the primary account.** Even when a person authorizes QuickStatements with their own login, what QuickStatements acts as is one of that person's subsidiaries. The distinction 0024 draws, human edits under a primary account and automated edits under a subsidiary, holds for delegated tools as it holds for scripts, and a primary account never holds a credential a program can use, whether that credential is an API key or an OAuth token.

The second rule follows from the first: **using a semi-automated tool requires approval.** On Wikimedia projects the bot policy asks that large semi-automated runs be approved, and QuickStatements batches routinely are not. Here the subsidiary a tool acts as is created *in the authorization flow* and is **pending** until a bureaucrat approves it, so the approval that Wikimedia asks for socially is the same `userrights` action 0024 already uses for the bot flag. A tenant that does not want this can turn it off; the default is that it is on.

This ADR makes the instance an OAuth 2.0 authorization server under those two rules. It does not make the instance an identity provider for other sites: logging *in* remains [0007](0007-actor-identity.md) §3, and this ADR is only about tools acting *on* the instance.

## Decision

### 1. An OAuth 2.0 authorization server, at MediaWiki's paths

*Changed by A4, A5.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §7.1, §7.2, §7.7.*

### 2. Consumers are instance configuration (extends 0015 §3)

*Current text: [07](../architecture/07-actors-and-accounts.md) §7.3, §7.6.*

### 3. Authorization selects or creates a subsidiary, and a created one is pending (amends 0007 §4; extends 0024 §2; amends 0024 §4)

*Changed by A4.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §1.3, §4.2, §4.3, §5.3, §7.4.*

### 4. Attribution and the consumer tag (extends 0030 §5; settles 0024 Q3)

*Current text: [07](../architecture/07-actors-and-accounts.md) §4.6, §5, §7.5.*

### 5. Management, revocation and log events (extends 0010 §11 and 0011 §6.1)

*Changed by A2, A4.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.4, §4.4, §7.6; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 6. Rate limits (uses 0024 §5)

*Current text: [07](../architecture/07-actors-and-accounts.md) §6.1, §6.2, §7.*

### 7. Tenants (uses 0018 §4, 0024 §7 and 0028 §8)

*Current text: [08](../architecture/08-tenants-and-instances.md) §10.4.*

### 8. Storage (extends 0013 §4 and §5.6; uses 0014 §2 and §4)

*Changed by A4.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.5, §4.6, §4.16, §5, §12.3.*

### 9. API (extends 0012 §4 and §5)

*Changed by A4.*

*Current text: [18](../architecture/18-api.md) §1.5, §2.2, §2.3, §3.2, §8.*

### 10. Permissions (extends 0016 §2; amends 0016 §3)

*Changed by A2.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §2.3, §3.4, §8.4.*

### 11. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

### 12. Scatterbase

*Current text: [22](../architecture/22-crates-and-stack.md) §1.3.*

## Consequences

- **No tool ever edits as a person.** A person's contributions are theirs alone; every tool's edits are a subsidiary's, named and operated, and tagged with the tool. This is what 0024 promised for scripts, extended to the case where the person is present.
- **Semi-automated editing requires approval by default**, through the same `userrights` action that already grants the bot flag, so a community reviews QuickStatements accounts the way it reviews bots, and can wave individual tools through per tenant when it trusts them.
- **A tool's failure mode is a pending account, not a rejected login.** Authorization completes; writes wait; the tool can say so.
- **Tokens and keys are one mechanism** in the request path: a credential of a subsidiary, with grants, checked by hash, revocable at once, never in a header.
- **One registration serves a farm**, and each tenant chooses what to allow, which fits all three tenancy presets without a switch of their own.
- **Wikimedia-written tools need a host change**, not a rewrite, provided they speak OAuth 2.0; 1.0a tools do not work until Q1 is settled.
- **One more crate, two `private` tables, one status, five permissions**, and no new log partition or record kind beyond two `config` kinds.

## Open questions

- **Q1. OAuth 1.0a shim.** Whether to serve `Special:OAuth/initiate|authorize|token` with HMAC-SHA1 signing for Pywikibot's OAuth login and other 1.0a-only tools, or to point them at bot passwords ([0024](0024-subsidiary-accounts.md) §4) and leave 1.0a unimplemented. The signing code is small; the cost is a second token shape to revoke and audit.
- **Q2. OpenID Connect.** Whether `/oauth/identify` should grow into an OIDC `userinfo` with an ID token, so that other sites can offer "log in with this instance". Deliberately outside this ADR; it would make the instance an identity provider, and [0007](0007-actor-identity.md) §3's choice was to consume providers, not to be one.
- **Q3. Consent for grant changes.** When a consumer's approved grants widen, whether existing authorizations keep their narrower grants (as here) or the person is asked again on next use.
- **Q4. Auto-approval by track record.** Whether a tenant should be able to auto-approve a subsidiary whose operator already has an approved one, or whose operator holds a given group, rather than only per consumer.
- **Q5. Pending-subsidiary notifications.** Whether creation of a pending subsidiary should notify bureaucrats through [0021](0021-notifications.md), as a `rights`-like event, or only appear on `Special:PendingSubsidiaries`.
- **Q6. Subsidiary name suggestion**: `{Operator}-{slug}` versus `{Operator}Bot` for all tools, pending 0024 Q2.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §11 | extends | 0005 A21 |
| [0007](0007-actor-identity.md) §4 | §3 | extends | 0007 A9 |
| [0010](0010-site-ui.md) §11 | §5 | extends | 0010 A15 |
| [0011](0011-logs.md) §6.1 | §5 | extends | 0011 A9 |
| [0012](0012-api-requirements.md) §4, §5 | §1, §9 | extends | 0012 A14 |
| [0013](0013-postgres-storage.md) §5, §5.2, §5.6 | §8 | amends | 0013 A9 |
| [0013](0013-postgres-storage.md) §4, §5.4, §5.5, §7, §8 | §8 | extends | 0013 A9 |
| [0014](0014-caches-and-search.md) §1, §5 | §5, §8 | amends | 0014 A3 |
| [0014](0014-caches-and-search.md) §10 | §5, §8 | extends | 0014 A3 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §2 | extends | 0015 A8 |
| [0016](0016-permissions-and-access-control.md) §2 | §3, §10 | extends | 0016 A8 |
| [0016](0016-permissions-and-access-control.md) §3 | §3, §10 | amends | 0016 A8 |
| [0024](0024-subsidiary-accounts.md) §1, §2 | §3–4 | extends | 0024 A2 |
| [0024](0024-subsidiary-accounts.md) §4 | §3–4 | amends | 0024 A2 |
| [0024](0024-subsidiary-accounts.md) Q1 | — | settles | 0024 Q1 |
| [0024](0024-subsidiary-accounts.md) Q3 | §4 | settles | 0024 Q3 |

## References

- [RFC 6749 — OAuth 2.0](https://www.rfc-editor.org/rfc/rfc6749), [RFC 6750 — Bearer token usage](https://www.rfc-editor.org/rfc/rfc6750), [RFC 7636 — PKCE](https://www.rfc-editor.org/rfc/rfc7636), [RFC 8628 — Device authorization grant](https://www.rfc-editor.org/rfc/rfc8628), [RFC 7009 — Token revocation](https://www.rfc-editor.org/rfc/rfc7009), [RFC 8414 — Authorization server metadata](https://www.rfc-editor.org/rfc/rfc8414)
- [Extension:OAuth](https://www.mediawiki.org/wiki/Extension:OAuth), [OAuth/For Developers](https://www.mediawiki.org/wiki/OAuth/For_Developers) (the `rest.php/oauth2/*` paths, the `profile` resource, grants as scopes), [OAuth/Owner-only consumers](https://www.mediawiki.org/wiki/OAuth/Owner-only_consumers)
- [Help:QuickStatements](https://www.wikidata.org/wiki/Help:QuickStatements), [OpenRefine: Wikibase reconciliation and editing](https://openrefine.org/docs/manual/wikibase/overview)
- [Wikidata:Bots](https://www.wikidata.org/wiki/Wikidata:Bots) (semi-automated editing and approval)
- [0007 — Actor identity](0007-actor-identity.md), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §11
- **Summary:** 0005 §2 is the one crate table CI checks, and carries `triplespace-oauth` and every change this section listed (0005 A21).

Replaced text (§11):

> | Layer | Crate | Change |
> |---|---|---|
> | Substrate | `scatter-actors` | The `pending` status and its effect on implicit membership (§3); the grant-intersection rule already exists for keys and is reused unchanged |
> | | `scatter-mwlog` | The `oauth/*` actions (§5) |
> | Triplespace | `triplespace-oauth` *(new)* | The authorization server: metadata, authorize, token, device, revoke and identify endpoints at both paths; PKCE; consent page; consumer registry and policy evaluation; the `oauth:{slug}` tag on the request path (§1–4). Depends on `triplespace-accounts` for every read or write of `private` and on `scatter-actors` for permissions |
> | | `triplespace-accounts` | `private.oauth_token` and `private.oauth_consumer`; token authentication beside key authentication; session–token binding; revocation on retirement and transfer (§5, §8) |
> | | `triplespace-api-action`, `triplespace-api-rest` | §9; the `oauth-pending` refusal in front of every write |
> | | `triplespace-server` | `Special:OAuthConsumers`, `Special:PendingSubsidiaries`, `Special:OAuth/device` and the Connected applications section, served from `triplespace-oauth` (§5) |
>
> The workspace goes from forty-three crates to forty-four.

### A2. The primary tenant

- **Date:** 2026-09-30
- **Source:** [0046](0046-primary-tenant.md) §4, §8
- **Change:** amends §5, §10
- **Summary:** By section:
  - §5: Consumer events are written to the **instance `log`**, not to the primary tenant's `log` or to `log/{farm}`: consumers are instance configuration, and their record must not move when the primary role does.
  - §10: `mwoauthmanageconsumer` is an **instance right** ([0040](0040-instance-prerogatives.md) §9): it is evaluated on the primary tenant or through a global group, and a bureaucrat of any other tenant holds it to no effect.

Replaced text (§5):

> with the consumer slug as target and the reason in the comment part, written to the primary tenant's `log` partition, or `log/{farm}` where [0028](0028-tenancy-policy.md) §2 creates one.

Replaced text (§10):

> | `mwoauthmanageconsumer` | Approving, rejecting and disabling consumers (§2) | `bureaucrat` |

### A3. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §5, §10–11
- **Summary:** A1–A2 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A2 was two blockquotes. The file before conversion is commit `0b26a3a`.

### A4. The OAuth pages move to the site

- **Date:** 2026-10-03
- **Source:** [0057](0057-web-tier.md) §13
- **Change:** amends §1, §3, §5, §8, §9
- **Summary:** The authorization server keeps its endpoints and checks; its pages become pages of the site, rendered by `triplespace-ui` wherever the site is served. `/oauth/authorize` validates the request, stores it under a ten-minute request handle and redirects to `Special:OAuth/authorize`, which reads the request and posts the decision back through the new `/oauth/requests` routes; the device page joins the same flow. Pending state (codes, device codes, request handles) falls back to `private` rather than process memory, so an API pool of several replicas can complete a flow.

Replaced text (§1):

> | `/oauth/authorize` | `/w/rest.php/oauth2/authorize` | Consent page (§3); requires an interactive session of a **primary** account |

Replaced text (§3):

> The consent page at `/oauth/authorize` is shown to a **primary account** with an interactive session;

Replaced text (§8):

> An instance without a shared cache holds them in process, which the small profile accepts.

### A5. The instance as an OAuth client

- **Date:** 2026-10-05
- **Source:** [0067](0067-proposals.md) §6
- **Change:** extends §1
- **Summary:** An edit-scoped grant on the destination wiki, separate from login, used only to push the person's proposals.

### A6. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§12
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
