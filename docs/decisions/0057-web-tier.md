# 0057. The web tier

- **Status:** Proposed
- **Date:** 2026-10-03
- **Updated:** 2026-10-09 (A3)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0025](0025-oauth-server.md), [0033](0033-backend-stack.md), [0034](0034-frontend-stack.md), [0042](0042-template-expansion-and-parsoid.md), [0056](0056-security-model.md)
- **Uses:** [0003](0003-statement-ui.md), [0039](0039-files-and-media.md), [0045](0045-table-content-model.md), [0047](0047-special-pages.md), [0053](0053-mirrored-pages.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [11](../architecture/11-rendering-templates-and-modules.md), [20](../architecture/20-web-tier.md), [22](../architecture/22-crates-and-stack.md)

## Context

[0034](0034-frontend-stack.md) renders every page on the server, in `triplespace-ui`, and embeds the frontend's assets in the one binary. Its §1.5 has the renderer call "the API crates' handler layer in-process", so rendering lives inside `triplespace-server` and scales with it. [0033](0033-backend-stack.md) §1 makes one binary and Postgres the whole of a small instance.

An instance that serves readers at volume, or that must stay up while one machine fails, wants page rendering on machines of its own: rendering is CPU-bound and stateless in principle, while the API holds database pools, the write path and the evaluator of [0056](0056-security-model.md). Rendering in-process ties the two together.

Four facts constrain how they come apart:

1. **Sessions are per tenant host** (0056 §1): `HttpOnly`, `SameSite=Lax` cookies and a CSRF token on every write. A UI on another origin would need credentialed CORS and cookies scoped across hosts, which 0056 does not allow.
2. **Every response is redacted for its viewer** ([0012](0012-api-requirements.md) §1.5), and 0012 §1.4 already requires that the UI use only the public API.
3. **0034 §5's fragment routes** sit under `rest.php/triplespace/v0`, though the web tier would own their renderers, and `GET /page/{id}/render` there collides with the render manifest of [0042](0042-template-expansion-and-parsoid.md) §12.
4. **The server's proxy trust** (0056 §10 line 5) names trusted proxies, but the first server does not yet check the peer address against them, and does not read `X-Forwarded-For`. A web tier in front of the server is such a proxy, and in a shared Kubernetes cluster its addresses are the whole pod range.

The implementation plan is the project document `web-frontend-plan.md` of 2026-10-03, whose decisions this ADR records.

### Direction

James's direction, from the design discussion of 2026-10-03:

- **"I want a stateless web frontend that I can deploy separately from the Triplespace server (e.g. in a high availability setup). The frontend interacts with the API."**
- **"Yes, same host for UI and server. The in-process option for triplespace-server is good. Web servers can cache public API responses in memory or on a specified Valkey server."**
- **"Let's do option D."** Asked where fragments are served from: a prefix of the web tier's own, under `rest.php/triplespace/v0` with a proxy rule, by the API server, or as MediaWiki's `action=render` on the page's own URL with a region parameter (D).
- **"Your recommendation for Q2 and Q4 are good. For Q3, it should be TLS unless it's an address on a private network or something like "api.svc" where it's an internal address."** Q2 was the OAuth consent page (§13); Q4 the stability of region names (§8).
- **"Your recommended solution for Q1 is good, including for 0042. I like the resolved address rule for Q3."** Q1 was how the response cache meets `action=purge` and erasure (§5, §14).
- **"I think address trust by default with keys as an option is good."** Asked how the API should decide whose `X-Forwarded-For` to believe (§10).

## Decision

### 1. Principles (amends 0034 §1)

*Current text: [20](../architecture/20-web-tier.md) §1.1.*

### 2. Two ways to run the site (amends 0033 §1, §17; extends 0033 §12; amends 0034 §2, §8)

*Current text: [20](../architecture/20-web-tier.md) §1.2.*

### 3. The routing table

*Current text: [20](../architecture/20-web-tier.md) §2.1.*

### 4. Talking to the API

*Current text: [20](../architecture/20-web-tier.md) §2.2.*

### 5. The response cache (extends 0014 §2)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §10.2.*

### 6. A page's own cacheability (extends 0014 §6)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §10.3, §12.4.*

### 7. Forms without JavaScript (extends 0012 §4)

*Current text: [20](../architecture/20-web-tier.md) §3.1.*

### 8. Fragments: `action=render` on the page's own URL (amends 0034 §5, §11; extends 0010 §12, §13)

*Current text: [20](../architecture/20-web-tier.md) §3.2.*

### 9. Independent deployment (extends 0012 §4)

*Current text: [20](../architecture/20-web-tier.md) §4.*

### 10. Proxy trust and the deployment boundary (amends 0056 §10; extends 0056 §15; extends 0015 §3)

*Changed by A1, A2.*

*Current text: [20](../architecture/20-web-tier.md) §5.1, §5.2, §5.5.*

### 11. Health and observability (uses 0033 §13)

*Current text: [22](../architecture/22-crates-and-stack.md) §4.7.*

### 12. Crates (amends 0005 §2, Consequences; extends 0005 §3, §7)

*Current text: [22](../architecture/22-crates-and-stack.md) §1.2, §2.1, §2.2, §3.1.*

### 13. The OAuth pages (amends 0025 §1, §3, §5, §8, §9)

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.4, §7.2, §7.4.*

### 14. Purging (amends 0042 §10, §14)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §4.6.*

## Alternatives considered

- **A separate UI origin** (`ui.example.org`). Clean to deploy, but it needs credentialed CORS and session cookies across hosts, which 0056 §1 rules out, and it doubles every tenant's hostnames.
- **The web tier only, with no embedded site.** Every instance would then need two processes and a proxy that splits paths, against 0033 §1's small instance.
- **The web tier reading Postgres replicas directly.** Fast, but it puts the web tier inside the boundary of 0056 §10 with the whole of `view` readable, and it would duplicate the evaluator and redaction that the API applies.
- **Fragments under a prefix of the web tier's own** (`/ui/fragment/…`). Workable, but it invents a URL scheme where MediaWiki already has one.
- **Fragments under `rest.php/triplespace/v0`, with a proxy rule sending them to the web tier.** One namespace served by two tiers, with a regular expression in every proxy's configuration, and an OpenAPI contract describing routes the API does not serve.
- **Fragments rendered by the API server.** During a rolling deploy a fragment and the page it lands in come from different releases, and the API binary takes on the templates and the Codex builder.
- **A response cache that honours `s-maxage`, like a proxy.** Cheaper per request, but beneath a CDN that is purged by tag it undoes the purge: the CDN's next miss is answered from the stale copy and cached again.
- **Pushing purges to every web replica.** Saves the revalidation round trip, but couples the web tier to the instance's Valkey and its purge path, and a replica that misses a message while reconnecting serves stale pages. Kept as Q5.
- **Forwarder keys as the only proxy trust.** Robust on shared networks, but unfamiliar to operators who expect a trusted-proxy list, and unnecessary where only the instance's services share the network.

## Consequences

- **Page rendering scales on its own.** Web replicas are added or removed without touching the API, the database or the sessions.
- **A small instance is unchanged:** one binary, Postgres, and the site served by the server.
- **Redaction cannot diverge between the site and the API,** because the site is an API client with the viewer's credentials; a third-party client can show everything the site shows (0012 §1.4).
- **Each page costs several internal round trips** that the in-process design of 0034 avoided, and the response cache saves work rather than round trips. Parallel calls, batched `action=query` and HTTP/2 keep-alive are the mitigations.
- **No cache beneath the CDN can undo a purge or an erasure,** because the web tier revalidates everything a purge can change.
- **The web tier's statelessness is checked, not promised:** by the dependency rule of §12 and by running the end-to-end suite across two replicas with no affinity.
- **Fragments keep MediaWiki's address.** A MediaWiki tool calling `action=render` on a document page gets what it expects, and REST v0 loses its three `…/render` routes.
- **Proxy trust by address must be fixed in the first server** before any proxy, web tier or not, is put in front of it.
- **OAuth flows survive an API pool,** because their pending state is no longer held in one process.

## Open questions

- **Q1.** ~~**Purging the response cache by tag.** When `web.cache` is the instance's L1 Valkey, the erasure path of 0014 §5 could delete `web:` entries by tag and lift the `max_ttl` bound; whether that is worth the coupling.~~ *Settled by §5 and §14: the cache is not purged; every use revalidates, except responses whose `max-age` the API sets because no purge can change them, capped at 60 seconds; a plain purge bumps the render epoch (Direction). Pushing purges is Q5.*
- **Q2.** ~~**The OAuth consent page.** It is HTML the authorization server serves ([0025](0025-oauth-server.md) §1). Whether it moves to the web tier as a special page that posts the decision back to the API, as MediaWiki's `Special:OAuth/authorize` does.~~ *Settled by §13: it moves, with the device and consumer pages, over request handles (Direction).*
- **Q3.** ~~**Transport security between the tiers.** Whether the web tier reaches `web.api` over plain HTTP/2 on a private network or TLS inside it, and whether `instance check` should require the latter.~~ *Settled by §4: TLS unless the API's host resolves only to internal addresses (Direction). Trust by certificate is Q6.*
- **Q4.** ~~**`action=render` regions as a contract.** Whether the region names of §8 are documented for third parties with a stability promise, or stay internal to the site.~~ *Settled by §8: the names are a contract, the markup is not (Direction).*
- **Q5. Pushing purges to the web tier.** If revalidation round trips show up in measurements, the API could publish purged tags on a Valkey channel that every web replica follows; whether that is worth the coupling and the risk of a missed message.
- **Q6. Trusting proxies by certificate.** Where a service mesh gives each workload a certificate (Envoy's `x-forwarded-client-cert`), the API could trust a hop by its certificate identity, as a third option beside addresses and forwarder keys.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2, Consequences | §12 | amends | 0005 A60 |
| [0005](0005-crate-organization.md) §3, §7 | §12 | extends | 0005 A60 |
| [0010](0010-site-ui.md) §12, §13 | §8, §14 | extends | 0010 A32 |
| [0012](0012-api-requirements.md) §4, §5 | §7, §9, §13 | extends | 0012 A37 |
| [0014](0014-caches-and-search.md) §2, §6 | §5, §6 | extends | 0014 A15 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §10 | extends | 0015 A27 |
| [0025](0025-oauth-server.md) §1, §3, §5, §8, §9 | §13 | amends | 0025 A4 |
| [0033](0033-backend-stack.md) §1, §17 | §2 | amends | 0033 A10 |
| [0033](0033-backend-stack.md) §12 | §2 | extends | 0033 A10 |
| [0034](0034-frontend-stack.md) §1, §2, §5, §8, §11 | §1, §2, §8 | amends | 0034 A6 |
| [0034](0034-frontend-stack.md) §13 | §4 | extends | 0034 A6 |
| [0042](0042-template-expansion-and-parsoid.md) §10, §14 | §14 | amends | 0042 A8 |
| [0056](0056-security-model.md) §10, §16 | §10, §12 | amends | 0056 A2 |
| [0056](0056-security-model.md) §15 | §10 | extends | 0056 A2 |

## References

- [Manual:Parameters to index.php](https://www.mediawiki.org/wiki/Manual:Parameters_to_index.php), for `action=render` and `action=purge`
- [Manual:$wgCdnServers](https://www.mediawiki.org/wiki/Manual:$wgCdnServers), MediaWiki's list of trusted proxies, which the right-to-left rule of §10 follows
- [Caddy: `trusted_proxies`](https://caddyserver.com/docs/caddyfile/options#trusted-proxies) and [`reverse_proxy` headers](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy#headers)
- [RFC 7239](https://www.rfc-editor.org/rfc/rfc7239), the `Forwarded` header, which has the same trust question
- Project document `web-frontend-plan.md` (2026-10-03), the implementation plan

## Amendment log

### A1. Right-to-left reading is defence in depth

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §10
- **Summary:** The right-to-left reading of forwarded headers is the API's defence when [0056](0056-security-model.md) §10 line 6 does not hold, that is, when the edge has not stripped the client's inbound `X-Forwarded-*` before appending its own. A conforming edge produces `X-Forwarded-For: C, E`; the example in §10, with the forged `F` surviving, shows defence in depth, not the expected input. (PENDING F18)

### A2. The web tier may hold its cache's Valkey password

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §10
- **Summary:** This ADR (the newer) stands over [0056](0056-security-model.md) §10 line 12 (0056 A11): the web tier may hold, beyond the forwarder key, the password of the Valkey its response cache uses (§5), which may be the instance's L1 Valkey; "no credential of the instance's beyond an optional forwarder key" is qualified by that one, and 0056 §10 line 12 is qualified the same way. (PENDING F19)

Replaced text (§10):

> - **The web tier in the boundary.** It holds no credential of the instance's beyond an optional forwarder key and reads no store of it, so 0056 §10 lines 1, 2 and 4 do not apply to it and line 3 applies only to its cache's Valkey (§5).

### A3. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§14
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [11](../architecture/11-rendering-templates-and-modules.md), [20](../architecture/20-web-tier.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
