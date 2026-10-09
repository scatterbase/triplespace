# 0056. The security model

- **Status:** Proposed
- **Date:** 2026-10-01
- **Updated:** 2026-10-09 (A12)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0021](0021-notifications.md), [0023](0023-moderation.md), [0028](0028-tenancy-policy.md), [0033](0033-backend-stack.md), [0042](0042-template-expansion-and-parsoid.md), [0043](0043-lua-modules.md), [0051](0051-page-redirects.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0015](0015-record-format-and-partition-registry.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0022](0022-federation.md), [0024](0024-subsidiary-accounts.md), [0025](0025-oauth-server.md), [0030](0030-edit-filters.md), [0032](0032-sparql-update-stream.md), [0039](0039-files-and-media.md), [0040](0040-instance-prerogatives.md), [0045](0045-table-content-model.md), [0046](0046-primary-tenant.md), [0047](0047-special-pages.md), [0049](0049-boards.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [20](../architecture/20-web-tier.md), [22](../architecture/22-crates-and-stack.md)

## Context

**MediaWiki's model is all or nothing.** `$wgGroupPermissions['*']['read']` decides whether anonymous readers see the wiki; `$wgWhitelistRead` names the pages they may see anyway, such as the main page and the login page. There is no finer grain in core. Extensions that add one (Lockdown, AccessControl, ApprovedRevs, the many "page security" extensions) restrict the page view and are then bypassed by everything else that reads a page: `action=raw`, `Special:Export`, the API, the search index, diffs, revision histories, recent changes and the Atom feeds, transclusion (`{{:Private page}}`), `Special:WhatLinksHere`, category listings, `Special:Random`, the parser cache, and a reverse proxy that cached the page before it was restricted. MediaWiki's own manual says so: *"MediaWiki is not designed to be a CMS, or to protect sensitive data"*, and lists these holes under "Security issues with authorization extensions". The architecture reads pages in many places and checks permissions in one, so an extension can only chase the readers.

**Triplespace already decided the opposite shape**, piece by piece, without naming it:

- Every permission decision is a function of records, and **ACLs restrict `read`** as well as write permissions, with conjunctive evaluation over enclosure ([0016](0016-permissions-and-access-control.md) §4). Deletion, hiding, suppression and username-hiding are `read` ACLs ([0023](0023-moderation.md) §1), and "a `read` ACL on a namespace makes it private" (0023 §2).
- **Every response is redacted for its viewer** ([0012](0012-api-requirements.md) §1, §8), there is a privacy test with a case for each kind of leak, and it is backed by database grants: the public role cannot read `private` ([0013](0013-postgres-storage.md) §4).
- **Shared caches hold only the public form**, and the search index holds only public data ([0014](0014-caches-and-search.md) §1, §7). The SPARQL Update stream and the dumps carry the public form ([0032](0032-sparql-update-stream.md) §1).
- **A tenant is a wiki with its own partitions, users and configuration** ([0018](0018-tenants.md) §1–4); nothing crosses a tenant boundary unless the tenancy policy says so ([0028](0028-tenancy-policy.md) §1), file storage is separate per tenant by default so that one tenant cannot observe another's uploads ([0039](0039-files-and-media.md) §4), and a "private tenant" is mentioned as one whose partitions carry the `private` export policy (0018 §5).
- The instance acts on a tenant's data only through recorded, signed **instance acts** ([0040](0040-instance-prerogatives.md) §1–3).
- A page that depends on a page a `read` ACL now covers is **re-rendered** ([0042](0042-template-expansion-and-parsoid.md) §10), and a tenant repository is read "with its ACLs evaluated as the source tenant's" ([0052](0052-page-repositories-and-title-inheritance.md) §7).

What no ADR says is the rule these are instances of, who the rule protects against whom, and what has to be true of a deployment for the rule to hold. Five gaps follow from that:

1. **There is no "private tenant".** 0018 §5 names one and nothing defines it: how a tenant becomes one, what its readers see, what leaves it.
2. **The only `read` restriction is moderation.** 0023 gives `read` ACLs to deletion and suppression, set with `delete`, `deleterevision`, `suppressrevision` and `hideuser`. A page that *exists* for a group and does not exist for everyone else, which the group edits, watches and searches, has no mechanism, and a private namespace built from a deletion-grade ACL would have no search and no feeds for its own readers.
3. **A restriction has one grain between a page and a namespace**: a page and its subpages. James asks for groupings of pages. Categories are the wrong tool: membership is content that any editor writes, so an editor could move a page out of a restricted category to expose it, or into one to hide it from review. Lockdown's category mode has exactly this flaw.
4. **Caches and search assume two viewers**, anonymous and administrator. A fifty-member private wiki would bypass every shared layer on every read and could not search itself.
5. **The deployment boundary is implicit.** Postgres `view`, OpenSearch, Valkey, the blob store and the backups all hold restricted data in the clear, and the model holds only if the server is the only thing that reads them. Nothing says so, and nothing checks.

### Direction

James's direction, from the design discussion of 2026-10-01:

- **"One concern is separation between tenants. In the most open scenario, all wikis are public wikis, with all pages being public. However, in the most closed scenario, many unrelated tenants (e.g. hosting customers) could share an instance and there should be no bleed-over between them (except preapproved global data like imported entities and mirrored wiki pages). Individual instances will want different settings."**
- **"MediaWiki's security model is all-or-nothing. You either have access to the whole wiki or you can't access anything not on the whitelist (like the main page). There are extensions that attempt to improve on this, but are ultimately limited by how easy it is to bypass MediaWiki's safeguards. Triplespace should not allow access to resources unless the requester is allowed, and this means a deployment will need to be set up a certain way to prevent bypassing the security."**
- **"Additionally, it should be possible to restrict individual pages or groupings of pages, not just a whole namespace."**

## Decision

### 1. The invariant

*Current text: [09](../architecture/09-security-and-moderation.md) §1.2, §1.3, §1.4, §3.4.*

### 2. Visibility: the `read` restrictions that enclose a target

*Current text: [09](../architecture/09-security-and-moderation.md) §5.1, §5.2.*

### 3. Two targets: the tenant and the set (extends 0016 §4; extends 0023 §2)

*Changed by A3, A4, A10.*

*Current text: [09](../architecture/09-security-and-moderation.md) §4.2, §4.4, §4.7.*

### 4. Who may restrict what (amends 0016 §2 and §4; amends 0023 §1)

*Current text: [09](../architecture/09-security-and-moderation.md) §4.5.*

### 5. Absence: what a principal outside the group sees (extends 0012 §8)

*Current text: [09](../architecture/09-security-and-moderation.md) §5.3.*

### 6. Flow: derived output inherits the visibility of its inputs (amends 0042 §3; extends 0043 §5; extends 0051 §2; amends 0028 §5)

*Current text: [09](../architecture/09-security-and-moderation.md) §5.4, §7.8.*

### 7. Caches are keyed by visibility (amends 0014 §1, §4)

*Changed by A6, A7, A9.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §9.1, §9.4, §9.6, §10.1, §12.1, §12.2.*

### 8. Search indexes restricted content, with a filter (amends 0014 §7)

*Changed by A8.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §11.3, §11.5, §11.6.*

### 9. Writes

*Current text: [09](../architecture/09-security-and-moderation.md) §5.5.*

### 10. The deployment boundary (extends 0033 §12)

*Changed by A1, A2, A3, A11.*

*Current text: [20](../architecture/20-web-tier.md) §5.1, §5.3, §5.4, §5.5.*

### 11. The tenancy policy: `security.restrictions` (extends 0028 §1; settles 0028 Q2)

*Current text: [08](../architecture/08-tenants-and-instances.md) §5.1, §5.2.*

### 12. Instance acts

*Current text: [09](../architecture/09-security-and-moderation.md) §5.6.*

### 13. API and UI (extends 0012 §4; extends 0016 §7; extends 0023 §8 and §9)

*Current text: [18](../architecture/18-api.md) §2.2, §2.3, §3.1, §3.2; [19](../architecture/19-site-ui.md) §1.3, §1.5, §6.3, §6.11.*

### 14. Storage (extends 0013 §5.6)

*Changed by A5, A6.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.5, §4.6, §4.14, §5, §12.2.*

### 15. The privacy test (extends 0012 §8)

*Changed by A2.*

*Current text: [09](../architecture/09-security-and-moderation.md) §5.8.*

### 16. Crates (amends 0005 §2)

*Changed by A2.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **MediaWiki's model: `read` for `*`, a whitelist of titles.** Rejected by the Direction. It is also what makes every page-restriction extension bypassable: the whitelist is a second rule evaluated in one place, while reads happen in many.
- **Deny rules and per-account ACLs.** A deny would reintroduce precedence, which [0016](0016-permissions-and-access-control.md) §4 removed; a per-account entry is a group of one and costs nothing to make. Groups only.
- **A separate "private wiki" mode** instead of a `tenant` target. It would be a second evaluation path with its own leaks. Making the tenant the root of enclosure gives private tenants every rule of this ADR for free, and makes "private wiki" and "private page" the same mechanism at different grains.
- **Categories or title prefixes as groupings.** Content that editors write; §3.
- **Postgres row-level security** with the principal in a session variable, as a second line behind the server. Attractive, and nothing here forbids adding it later (Q2). Rejected as the primary mechanism: the evaluator's three axes of enclosure, expiring ACLs, group inheritance from farm accounts and L0 caching would have to be duplicated in SQL and paid for on every row; and RLS does nothing for OpenSearch, Valkey, rendered pages or the blob store, which hold the same data.
- **Encrypting restricted content per group at rest.** Rendering, search, diffs and constraint checks need plaintext; the keys would live on the same servers; and a group's membership changes, which would mean re-encrypting. The boundary is the deployment (§10), stated and checked, not a cipher.
- **One search index per group, or no indexing of restricted content.** §8.
- **403 instead of absence.** Honest and simpler to debug, and it tells every probe which titles exist. Absence (§5), with the two leaks it cannot close named.
- **Including a more restricted target into a less restricted page, rendered per viewer.** Every page would need a rendering per combination of viewer groups, the render cache would be useless, and a page's visibility could no longer be computed from the page alone. The subset rule of §6 keeps one rendering per visibility, at the cost of a red link where an editor tried to include a secret into a public page, which is the behaviour wanted.

## Consequences

- **The rule is one sentence**, §1, and every route, cache, index, stream and export has the same thing to prove. The privacy test says how (§15).
- **A public wiki changes nothing.** Empty visibility sets, `-` in every cache key, an empty `read_groups`, and no `tenant` ACL: the cost of this ADR to the open scenario is a few bytes per key.
- **Hosting works with the existing presets.** `isolated` plus a `tenant` ACL per customer gives tenants that cannot see each other, cannot search each other, cannot transclude each other and cannot learn each other's titles, while all of them read Wikidata and Wikipedia. No operator superuser exists in the UI.
- **Pages and groupings can be confidential with one mechanism.** A `read` restriction set by `protect` on a page, a set, a namespace or a tenant, evaluated conjunctively over enclosure, as every other restriction is.
- **A private wiki is a usable wiki.** Its members search it (§8), cache it (§7), watch it and are notified from it (§6). The earlier model would have given them none of those.
- **Search and Valkey join the boundary.** They now hold restricted data; a deployment that exposes either exposes the wiki, and `instance check` says so.
- **Existence is protected, with two named leaks**: a taken title and a sequence gap (§5). Anyone relying on absence should know both.
- **Includes may not widen**, so a public page cannot be used to read a private one, and an editor who tries sees a red link. One rendering per visibility is what makes caching work.
- **Operators are inside the boundary.** This ADR constrains what the software serves, not what a database administrator can query; it says so rather than imply otherwise.
- **The first milestone is unchanged** (API-only, one user and their subsidiaries, no differentiated ACLs, set 2026-09-27). Lines 1, 5, 9 and 11 of §10 and the 421 rule are cheap and catch the likeliest mistakes, so they belong in the first server rather than later.
- **Broad `read` ACL changes purge broadly.** Making a namespace private lapses every cache entry of the tenant through the epoch (§7). Acceptable because it is rare; a tenant that flips visibility daily will notice.
- **Two more ACL targets, one more switch, one more CLI command, and a `{vis}` segment in cache keys.** No new partition, no new schema, nothing new in `private`.

## Open questions

- **Q1. A members-only query endpoint.** A private tenant has no SPARQL endpoint, because every endpoint is fed from the ∅ form (§3, §10 line 7). An organisation may want QLever over its private data for its own members; that would be an endpoint behind the evaluator, or a per-tenant instance loaded from the operator's full bundle inside the boundary. Not designed here.
- **Q2. Row-level security as a second line.** Whether to add Postgres RLS on `view` with the principal's groups in a session variable, so that a bug in the server's evaluator fails closed at the database. It would duplicate enclosure in SQL; the question is whether the duplication is worth it for the containment axis alone.
- **Q3. Share links.** A capability URL that lets a named outsider read one restricted page without an account, as document systems offer. It is a principal that is a secret, which this ADR has no place for; if wanted, it is a subsidiary with one grant and an expiry ([0024](0024-subsidiary-accounts.md)), and the question is whether that is enough.
- **Q4. Auditing restrictions one cannot read.** `Special:ProtectedPages` shows a restriction only to those who may read its target (§5), so a bureaucrat outside every group cannot count the tenant's confidential pages. Whether `protect` holders should see a count, or the names of sets without their members, trades audit against the existence leak.
- **Q5. Nested sets and namespace members.** Sets are flat and hold pages, entities and threads (§3). Whether a set may hold a namespace or another set, which would make the enclosure graph recursive, waits for a case that needs it.
- **Q6. `instance check` coverage.** Lines marked `attest` (§10) are the operator's word. Whether some can become checks, such as detecting other Postgres connections through `pg_stat_activity` on a schedule, or whether a signed deployment manifest is the right tool, is open.
- **Q7. Confidential restrictions on foreign and mirrored content.** A `read` ACL on a mirrored page's ranged ID hides it on the tenant ([0053](0053-mirrored-pages.md) §9), and the same works for a mirrored entity's overlay row; whether a tenant should be able to make a *subset* of Wikipedia visible to a group only, as a reading list, or whether that is a set of mirrored pages under this ADR already, needs a worked example.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §16 | extends | 0005 A55 |
| [0012](0012-api-requirements.md) §4, §8 | §5, §13, §15 | extends | 0012 A35 |
| [0013](0013-postgres-storage.md) §5.6 | §14 | extends | 0013 A26 |
| [0014](0014-caches-and-search.md) §1, §4, §7 | §7, §8 | amends | 0014 A13 |
| [0016](0016-permissions-and-access-control.md) §2, §4 | §4 | amends | 0016 A21 |
| [0016](0016-permissions-and-access-control.md) §4, §7 | §3, §13 | extends | 0016 A21 |
| [0018](0018-tenants.md) §5 | §3 | amends | 0018 A11 |
| [0021](0021-notifications.md) §2 | §6 | extends | 0021 A8 |
| [0023](0023-moderation.md) §1, §2 | §3, §4 | amends | 0023 A9 |
| [0023](0023-moderation.md) §2, §8, §9 | §3, §13 | extends | 0023 A9 |
| [0028](0028-tenancy-policy.md) §5 | §6 | amends | 0028 A9 |
| [0028](0028-tenancy-policy.md) §1 | §11 | extends | 0028 A9 |
| [0028](0028-tenancy-policy.md) Q2 | §11 | settles | 0028 Q2 |
| [0033](0033-backend-stack.md) §12 | §10 | extends | 0033 A8 |
| [0042](0042-template-expansion-and-parsoid.md) §3 | §6 | amends | 0042 A7 |
| [0043](0043-lua-modules.md) §5 | §6 | extends | 0043 A3 |
| [0051](0051-page-redirects.md) §2 | §6 | extends | 0051 A1 |

## References

- [Manual:Preventing access](https://www.mediawiki.org/wiki/Manual:Preventing_access) and [Security issues with authorization extensions](https://www.mediawiki.org/wiki/Security_issues_with_authorization_extensions), the list of bypasses this ADR closes by construction
- [Manual:$wgWhitelistRead](https://www.mediawiki.org/wiki/Manual:$wgWhitelistRead), [Extension:Lockdown](https://www.mediawiki.org/wiki/Extension:Lockdown)
- [OpenSearch: `terms_set` query](https://opensearch.org/docs/latest/query-dsl/term/terms-set/), the filter of §8; [OpenSearch Security: document-level security](https://opensearch.org/docs/latest/security/access-control/document-level-security/), the alternative of pushing the filter into the cluster
- [PostgreSQL: Row Security Policies](https://www.postgresql.org/docs/current/ddl-rowsecurity.html) (Q2)
- Anderson, *Security Engineering*, 3rd ed., ch. 9 (multilevel security), for the "no write up without read up" shape of §6 and §9
- `docs/registry/tenancy.toml` (§11), `docs/registry/groups.toml`

## Amendment log

### A1. Development mode's fallback tenant

- **Date:** 2026-10-03
- **Source:** Direct: James, decision of 2026-10-03 (`triplespace-accounts`, `triplespace-api-action`, `triplespace-server`, `scatter-adapter-internetdomains`)
- **Change:** extends §10
- **Summary:** In `development` mode the server may name a fallback tenant (`--dev-tenant`), which an unregistered `Host` (a developer's `localhost:8080`) is served as, with `siprop=general` reporting the request's own scheme and host as `server`; the session cookie then also drops `Secure`. In `production` an unregistered host is 421 and the setting is refused.

### A2. The web tier

- **Date:** 2026-10-03
- **Source:** [0057](0057-web-tier.md) §10, §12
- **Change:** amends §10, §16; extends §15
- **Summary:** Proxy trust is read from the right along a chain of proxies, and a hop is trusted by its address in `server.trusted_proxies`, the default, or by a forwarder key, for networks shared with workloads that are not trusted (line 5). A web tier that serves the site through the API is a hop inside the request path and outside the stores, and line 12 states what it must do. The privacy test also runs through the web tier. The landing page is rendered by `triplespace-ui`, wherever the site is served.

Replaced text (§10, line 5):

> | 5 | The server rejects a request whose `Host` is not a registered tenant base or the farm base, with 421, and trusts `X-Forwarded-For` and `X-Forwarded-Proto` only from `server.trusted_proxies` | The host selects the tenant ([0018](0018-tenants.md) §11); IP blocks, rate limits and filter IP rows depend on the client address ([0016](0016-permissions-and-access-control.md) §3, [0024](0024-subsidiary-accounts.md) §5, [0030](0030-edit-filters.md) §11) | `pass`/`fail` by sending a request with an unregistered host; `fail` if the server is behind a proxy and the trust list is empty |

Replaced text (§16):

> | `triplespace-server` | `server.mode`, `server.trusted_proxies`, `server.admin_listen`, 421 on an unregistered host, the landing page, the internal endpoint for Parsoid |

### A3. The query service

- **Date:** 2026-10-04
- **Source:** [0059](0059-query-service.md) §4
- **Change:** extends §3, §10
- **Summary:** A private tenant has no query service either (§3). The deployment boundary (§10) gains the embedded query store in line 7, fed from `view.rdf_delta`, and a line 13 for the shared remote store: reachable only from the servers, credentialed, and queried only through the service, which fixes the tenant's dataset.

Replaced text (§10, line 7, in part):

> | 7 | A SPARQL endpoint, QLever or any other external index is loaded from the public dump or the update stream, never from `view` or `log` |

### A4. Scopes are not sets

- **Date:** 2026-10-04
- **Source:** [0060](0060-scopes.md) §1
- **Change:** extends §3
- **Summary:** A scope is content and never a restriction target; the paragraph after the set rationale says so.

### A5. `view.acl` is as 0013 §5.4 defines it

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §14
- **Summary:** `view.acl` is as [0013](0013-postgres-storage.md) §5.4 (0013 A27) defines it: `(target, restrictions jsonb, read_kind, extra jsonb, "offset")`, PK `(target)`. The column §14 called `kind` is `read_kind`; [0023](0023-moderation.md) §10's column list is replaced by a reference to the same definition. (PENDING A4)

Replaced text (§14):

> | `view` | `acl` (existing) gains the `tenant` and `set` target kinds and a `kind = confidential \| moderation` column, derived from the right that set the record | The two kinds of §2 |

### A6. `view.tenant`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §7, §14
- **Summary:** The table that holds the visibility epoch of §7 is `CREATE TABLE view.tenant (tenant text PRIMARY KEY, visibility_epoch integer NOT NULL DEFAULT 0)`, a projection of the tenant's `read` ACL records, listed in [0013](0013-postgres-storage.md) §5.5; tenant configuration stays in `view.registry`. (PENDING B1)

### A7. A `read` ACL bumps the target's generation

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §7
- **Summary:** A `read` ACL written, changed or retired on a target bumps the target's **generation**, as [0014](0014-caches-and-search.md) §5 and §10 say for hiding, since both kinds of read restriction hide for privacy; a version bump is for changes that hide nothing (0014 §10's sitelink and federation examples). §7 said "version". (PENDING B2)

Replaced text (§7):

> - **Purges.** A `read` ACL written, changed or retired on target *T* bumps *T*'s version, which makes every entry for *T* unreachable (0014 §1, principle 1).

### A8. The Postgres search fallback filters after the scan

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §8
- **Summary:** The Postgres search fallback ([0014](0014-caches-and-search.md) §8) filters its results after the scan with the read-time visibility set; no `read_groups` column is added to `view.page`, `view.page_text` or `view.term`. `read_groups` is a field of the search documents only. (PENDING B3)

Replaced text (§8):

> the Postgres fallback (0014 §8) applies the same predicate in SQL.

### A9. Which keys carry `{vis}`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §7
- **Summary:** The rule: every key whose value depends on what the viewer may read carries `{vis}`, and [0014](0014-caches-and-search.md) §10 marks each key. The classification, to confirm: carry `{vis}`: `e:`, `t:`, `prov:`, `p:`, `d:`, `sug:`, `css:`, `post:`, `th:`, `tp:`, `fi:`, `rf:`, `ld:`, `sc:`, `sp:`; do not: `s:`, `rl:`, `up:`, `fp:` (a repository bundle is public upstream content). §7's list named only the keys 0014 §4 had when this ADR was written. (PENDING B5)

Replaced text (§7):

> `e:`, `t:`, `prov:`, `p:`, `d:`, `sug:` and `css:` carry it; `s:`, `rl:` and `up:` do not.

### A10. `createaccount` on a private tenant

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §3
- **Summary:** `createaccount` governs self-registration and creating an account for another, as in MediaWiki; its default is `universe`; a private tenant removes it from `universe`, which is what §3 already says and which stands. [0024](0024-subsidiary-accounts.md) §11's `autoconfirmed` default goes, and a subsidiary a new user creates is pending until approved ([0025](0025-oauth-server.md) §3). The ledger row's verb is corrects, but nothing in §3 is contradicted, so this entry extends it. (PENDING C12)

### A11. The web tier may hold its cache's Valkey password

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §10
- **Summary:** [0057](0057-web-tier.md) §10 (the newer) stands: the web tier may hold, beyond the forwarder key, the password of the Valkey its response cache uses, which may be the instance's L1 Valkey; line 12 of §10 is qualified accordingly, its "no credential of the instance's beyond an optional forwarder key" admitting that password. (PENDING F19)

Replaced text (§10):

> | 12 | A web tier ([0057](0057-web-tier.md)) holds no credential of the instance's beyond an optional forwarder key, reads no store of it, and reaches the API over TLS unless the API's address resolves only to internal addresses; it appends to forwarded headers and judges none of them (line 5), and the Valkey of its response cache is held to line 3 | It sees every viewer's cookie in transit | `fail` if the web tier's cache names a Valkey that fails line 3, or its `web.api` is plain HTTP to an address that is not internal |

### A12. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§16
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [20](../architecture/20-web-tier.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
