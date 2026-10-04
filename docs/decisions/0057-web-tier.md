# 0057. The web tier

- **Status:** Proposed
- **Date:** 2026-10-03
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0025](0025-oauth-server.md), [0033](0033-backend-stack.md), [0034](0034-frontend-stack.md), [0042](0042-template-expansion-and-parsoid.md), [0056](0056-security-model.md)
- **Uses:** [0003](0003-statement-ui.md), [0039](0039-files-and-media.md), [0045](0045-table-content-model.md), [0047](0047-special-pages.md), [0053](0053-mirrored-pages.md), [MediaWiki API contract](../api/mediawiki-compat.md)

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

1. **The web tier holds no state.** It has no database connection, no disk it writes and no instance key. Its only secrets are an optional forwarder key (§10) and the password of the Valkey its optional response cache may use (§5), which holds public responses only. Any replica can serve any request; a replica can be stopped at any moment and loses only its requests in flight.
2. **One origin.** The site and the API share each tenant host. The edge proxy splits requests by path (§3); the browser sees one origin, its cookies work for both, and the Vue components of 0034 §4 call `api.php` and `rest.php` directly with no CORS.
3. **The renderer is an API client.** It reaches the instance only through the public HTTP API, with the viewer's own credentials, so a page carries exactly the redaction the API applied (0012 §1.4–1.5). It never links an API crate's handlers and acts as no account of its own.
4. **One code path, two transports.** The same `triplespace-ui` router serves pages from `triplespace-web`, over HTTP, and from `triplespace-server`, over an in-process call into the API's own router (§2). Both send the same requests through the same middleware.

### 2. Two ways to run the site (amends 0033 §1, §17; extends 0033 §12; amends 0034 §2, §8)

- **`triplespace-server` embeds the site.** `server.ui` is `embedded` (the default) or `off`. Embedded, the server serves the web paths of §3 itself through `triplespace-ui`, calling its own API router as a `tower::Service` with no network hop. The embedded call carries the original request's headers and connection address unchanged, so the API sees the real client and the in-process path adds no `X-Forwarded-*`. A small instance stays one binary and Postgres (0033 §1). This is runtime configuration, not a Cargo feature, so the binary's behaviour is not chosen at build time ([0005](0005-crate-organization.md) §3 rule 6).
- **`triplespace-web` serves the site on its own.** A second binary from the same workspace, configured by the settings below. An HA deployment runs `triplespace-server` with `server.ui = off` behind the edge and `triplespace-web` beside it, each pool sized on its own.
- **Assets.** `ui/dist` is embedded in both binaries (0034 §8), so either serves the assets its pages name.
- **Node stays a build-time dependency only.**

`triplespace-web`'s settings:

| Setting | Meaning |
|---|---|
| `web.api` | The API's base URL on the internal network, one address or a load balancer's; `https` unless the address is internal (§4) |
| `web.listen`, `web.admin_listen` | The public listener; health and metrics, on a separate listener (0056 §10 line 9) |
| `web.mode` | `production` or `development`, as `server.mode` (0056 §10) |
| `web.forwarder_key_file` | Optional: the web tier's forwarder key (§10) |
| `web.cache` | `memory`, `valkey`, or `off` (§5); `web.cache.valkey_url` and `web.cache.valkey_password_file` for the second; `web.cache.max_ttl` |
| `web.upstream_host` | `forward` (the default) or, in `development` mode only, `fixed` (§4) |
| `web.assets_from` | Optional: a base URL that serves the hashed assets instead of the binary (§9) |

Secrets come from files, never the command line ([0033](0033-backend-stack.md) §12).

### 3. The routing table

The edge proxy routes by path prefix, longest prefix first. The authoritative list is `triplespace_ui::routes`; `triplespace-web routes --format nginx|haproxy|caddy` prints it as proxy configuration, and the embedded mode dispatches by the same list.

| Prefix | Served by |
|---|---|
| `/w/api.php`, `/api.php` | API |
| `/w/rest.php/`, `/rest.php/` | API |
| `/wiki/Special:EntityData/`, `/entity/` | API |
| `/.well-known/`, `/oauth/`, `/dumps/` | API |
| The media routes of [0039](0039-files-and-media.md) §7, where the media base shares the host | API |
| Everything else: `/`, `/wiki/`, `/w/index.php`, `/index.php`, `/ui/assets/` | Web tier |

The web tier is the default route, so an unknown path gets a 404 page in the site's frame. Special pages ([0047](0047-special-pages.md)), the OAuth pages (§13) and a private tenant's landing page (0056 §13) are pages like any other, rendered by `triplespace-ui`.

### 4. Talking to the API

- **Forwarded to the API:** `Cookie`, `Authorization`, `Accept-Language`, `User-Agent` and `X-Request-Id`; the `Host` the browser sent, as `X-Forwarded-Host`, with `X-Forwarded-Proto`; `X-Forwarded-For` as received, with the address of the peer that connected to the web tier appended; and `Triplespace-Forwarder` as received, with the web tier's own key appended when it has one. The web tier judges none of these: only the API decides what to believe (§10). Hop-by-hop headers are not forwarded.
- **Relayed back:** `Set-Cookie` from the API on form posts (§7), verbatim. The cookie names the tenant host, which is the web tier's host too.
- **Never logged:** cookies, bearer credentials, tokens, forwarder keys.
- **TLS unless the API is internal.** `web.api` may be plain `http` only when its host is a loopback, link-local or private address (RFC 1918, RFC 4193), or a name every resolved address of which is one. The web tier checks at startup and whenever it re-resolves the name, and refuses plain HTTP to anything else. A name such as `api.svc` or `api.internal` passes because of where it resolves, not because of how it is spelled.
- **Transport.** Pooled HTTP/2 to `web.api`, with timeouts; idempotent `GET`s may be retried once, writes never.
- **Development.** In `development` mode only, `web.upstream_host = fixed` sends `web.api`'s own host instead of the browser's, so the web tier can be pointed at www.wikidata.org or the MediaWiki 1.43 reference install, the prototyping path of 0034 §13; the features those lack are hidden by capability discovery (0012 §1.4).

### 5. The response cache (extends 0014 §2)

The web tier may cache API responses that are the same for everyone, in its own memory (`moka`) or in a Valkey named by `web.cache.valkey_url`. The CDN in front of it is purged by tag ([0014](0014-caches-and-search.md) §5) and the web tier's cache is not, so the cache never serves a response the API has since changed: a cache that cannot be purged, beneath one that can, would refill the CDN with what a purge or an erasure had just removed.

- **Only credential-free requests consult it.** A request the web tier sends without `Cookie` or `Authorization` may use the cache; any other request goes to the API.
- **Only public responses enter it.** A response with `Cache-Control: public`, an `ETag` and no `Set-Cookie`, keyed `web:{host}:{hash of method, path, query and Accept-Language}`.
- **Every use revalidates.** The web tier sends the cached `ETag` in `If-None-Match`; the API answers `304` while the response is current, and a new version, a purge (§14) or an erasure changes the `ETag` (0014 §3). The cache saves the API building and sending the body and the web tier parsing it, not the round trip.
- **Except what no purge can change.** A response whose `max-age` is greater than zero is used without revalidation for that long, never longer than `web.cache.max_ttl`, whose default and ceiling is 60 seconds, the default `s-maxage` of 0014 §6, whatever `s-maxage` a deployment configures. The API sends such a `max-age` only on responses that purging, erasure and moderation never change: `meta=siteinfo`, the registries and message bundles. The web tier never honours `s-maxage`, which is set for proxies that can be purged.
- **The Valkey is held to 0056 §10 line 3,** `AUTH` or a private address, whether it is the instance's L1 Valkey or a separate one, and `instance check` covers it.
- **The embedded site uses no response cache;** the API's own layers (0014 §2) already serve it.
- **Losing it costs only latency.** The replicas stay interchangeable.

A notice after a redirect ("Your edit was saved") travels as a short-lived cookie holding a message key from a fixed set, as MediaWiki's `postedit` does; it needs no signing and is not state.

### 6. A page's own cacheability (extends 0014 §6)

A page or fragment is composed from several API responses, and its HTTP caching follows from theirs:

- **It is public only if every response it used was public;** otherwise `private, no-cache`. This is 0056 §6 rule 1 applied to HTTP.
- **Its `ETag`** hashes the web tier's build ID, the interface language and the `ETag`s of the responses it used.
- **Its `Cache-Tag`** is the union of theirs, so the purges of 0014 §5 reach it.
- **It varies on `Cookie` and `Accept-Language`.**
- **A token never appears in public HTML.** Anonymous pages fetch `meta=tokens` from their components when they need one; a logged-in page is `private` and may carry its tokens in its data blocks.

### 7. Forms without JavaScript (extends 0012 §4)

Reading, logging in and out, creating an account, the account pages, page source editing and search work without JavaScript (0034 §4).

- **A form** carries the token the web tier fetched with the viewer's cookie (`logintoken`, `csrftoken`).
- **Its submission** goes to the web tier, which calls the matching API module and answers `303` to the result, or renders the form again with the API's error.
- **Sign-in through an issuer** (`/auth/{issuer}/start` and `callback`, 0012 §5) stays on the API and ends with a redirect to the `returnto` the web tier passed. `action=clientlogin`, `action=createaccount` and the auth routes accept only a `returnto` on the request's own origin, and ignore any other.

### 8. Fragments: `action=render` on the page's own URL (amends 0034 §5, §11; extends 0010 §12, §13)

**There are no fragment routes.** MediaWiki's `index.php?title={title}&action=render` returns a page's content without the skin. Triplespace keeps that meaning, at `/w/index.php`, `/index.php` and `/wiki/{title}?action=render`, which the routing table already sends to the web tier, and adds one parameter, `region`, for part of the content.

| Request | Returns |
|---|---|
| `action=render` on a document page or thread | The rendered body, as MediaWiki returns it |
| `action=render` on an entity | The content of the Statements tab, without the frame |
| `region=terms` | The term box |
| `region=statements/{P}` | One statement group in its layout ([0003](0003-statement-ui.md) §3) |
| `region=identifiers`, `region=sitelinks` | Those tabs' content |
| `region=post/{id}` | One post of a thread |
| `region=row/{id}` | One row of a `Table` page's grid ([0045](0045-table-content-model.md) §11) |

- **A region is part of its page.** Full pages are assembled from the region functions, and a region exists for `action=render` only once the page draws it, so the two cannot drift.
- **The region names are a contract; their markup is not.** The names in the table are documented with the site's URLs and kept until a log entry here retires one. The HTML inside a region changes with the site, as the HTML of MediaWiki's `action=render` does.
- **Errors.** An unknown region is `400` with a MediaWiki-shaped error body. A region with no content yet (a property the entity does not use) is `200` with the empty group's markup, so a component can add the first value. A missing or confidential target answers exactly as its page would (0056 §5).
- **Caching** follows §6, with its page's `Cache-Tag`s, so purging the page purges its regions.
- **Links are full URLs,** as MediaWiki's `action=render` makes them.
- **Build skew.** A component sends `X-Triplespace-UI-Build`, the build ID of the page it lives in. A web tier on another build answers `409` with no body, and the component reloads the page rather than swap in markup its loaded styles and scripts do not match; the write has already gone through the API, so the reload shows it. A request without the header, as from any third party, is always served.

### 9. Independent deployment (extends 0012 §4)

The web tier and the API are deployed separately, so each must tolerate the other's version.

- **Assets** have hashed names and are served `immutable`. A web release also carries the previous release's assets, or `web.assets_from` names a base that only ever accumulates, so a page rendered by the old release keeps loading during a rolling deploy.
- **The API's version.** `meta=siteinfo&siprop=triplespace` reports `api_version`, beside the capability list it already carries. The web tier hides what the API lacks (0012 §1.4) and reports itself not ready below the minimum `api_version` it was built for.
- **The editor's renderer.** `action=parse` reports its renderer version (0012 §4); where it differs from the build of `scatter-wasm` in the page, the editor previews through `action=parse` instead.

### 10. Proxy trust and the deployment boundary (amends 0056 §10; extends 0056 §15; extends 0015 §3)

- **Only the API judges forwarded headers.** The edge and the web tier append what they see (§4). The API reads `X-Forwarded-For` from the right, believing one more entry for each hop it trusts and stopping at the first it does not; the client is the last entry believed, or the peer itself when the peer is not trusted. `X-Forwarded-Host` and `X-Forwarded-Proto` are believed only from a trusted peer.
- **A hop is trusted by its address, or by a forwarder key.** Read from the right, a hop is trusted if its address (the peer's, or the entry the next hop appended) is in `server.trusted_proxies`, a list of addresses and CIDR ranges, as MediaWiki's `$wgCdnServers` and Caddy's `trusted_proxies` are. **Address trust is the default** and suits a deployment where only the edge, the web tier and the API share the network. Otherwise the hop is trusted if the next unused entry from the right of `Triplespace-Forwarder` is a valid **forwarder key**: the option for networks shared with workloads that are not trusted, such as a Kubernetes cluster where the web tier's addresses are the whole pod range and any pod there could otherwise forge a client address. The two mix freely.

```
client C → edge E → web W → API
X-Forwarded-For:       F, C, E      (F is forged by the client)
Triplespace-Forwarder: kE, kW
The API: W is trusted by kW, so believe E; E is trusted by kE, so believe C; no more → the client is C
```

- **A forwarder key means only "believe what this hop says about the client".** It acts as no account, reads nothing and grants nothing. `triplespace instance forwarder create --label {label}` prints a key once, as `{key id}.{secret}`. Its hash is kept in `private` by `triplespace-accounts`; a `forwarder` record in the instance `config` partition holds its ID, label and dates, never the secret or its hash, so issuing and revoking keys is in the log. `forwarder revoke` retires a key with a null record, and two keys may be valid at once for rotation. The edge adds its key as a fixed request header (Caddy's `header_up`); the web tier reads its own from `web.forwarder_key_file`. Over plain HTTP on an internal network (§4) a key is as exposed as the session cookies on the same link, which are worth more.
- **The web tier in the boundary.** It holds no credential of the instance's beyond an optional forwarder key and reads no store of it, so 0056 §10 lines 1, 2 and 4 do not apply to it and line 3 applies only to its cache's Valkey (§5). It sees every viewer's cookie in transit, so it is a hop like the edge.
- **`instance check`** sends a forged `X-Forwarded-For` from an untrusted address and fails if it is believed; it records an `attest` that only trusted hops can reach the API's listener, where trust is by address; and it fails a web tier configured with plain HTTP to an address that is not internal.
- **The privacy test runs through the web tier.** `instance check --through {url}` and the cases of 0056 §15 are run against the site's pages as well as the API, so a confidential target's page and its `action=render` are byte-equal to a missing one's.

### 11. Health and observability (uses 0033 §13)

- `/healthz` reports liveness and `/readyz` that the API is reachable and recent enough (§9), checked at most every few seconds; both on `web.admin_listen`.
- On `SIGTERM` the web tier stops accepting, finishes what is in flight, and exits.
- `X-Request-Id` is passed to the API, each API call is a `tracing` span, and the metrics are page latency by page kind, API calls per page, the response cache's revalidation and hit rates, and the API's error rate.

### 12. Crates (amends 0005 §2, Consequences; extends 0005 §3, §7)

| Crate | Change |
|---|---|
| `triplespace-client` | New. Typed requests and responses for the Action API modules and REST routes the site uses; the `Transport` trait with an HTTP transport (`reqwest`) and a `tower::Service` transport; header forwarding (§4) and the revalidating response cache (§5) |
| `triplespace-ui` | Gains the page handlers as an `axum` router, view models, special pages, the OAuth pages and the landing page, the routing table (§3), `action=render` and its regions (§8), the purge form (§14) and the composed-page cache rules (§6). Depends on `triplespace-client`, `scatter-wikibase-shape`, `scatter-wikibase-model`, `scatter-normalize`, `scatter-providers` and `askama` |
| `triplespace-web` | New. The binary of §2: configuration, embedded assets, `routes`, health and drain |
| `triplespace-accounts` | Forwarder key hashes in `private` and their check (§10) |
| `triplespace-oauth` | Request handles and the request routes in place of the consent page (§13) |
| `triplespace-server` | `server.ui` and the embedded site through `triplespace-ui` over the in-process transport; the right-to-left walk of §10 |
| `triplespace-cli` | `instance forwarder create`, `list` and `revoke` (§10) |

A new rule in 0005 §3: **the site's crates reach no store.** `triplespace-ui`, `triplespace-client` and `triplespace-web` depend on no crate that opens a connection to Postgres, Valkey or OpenSearch on the instance's behalf, and on no API crate; the response cache's Valkey client holds public API responses only. `cargo xtask deps` checks it.

### 13. The OAuth pages (amends 0025 §1, §3, §5, §8, §9)

The authorization server keeps its endpoints and their checks; its pages move to the site, as MediaWiki's `Special:OAuth/authorize` is a special page. The consent page, `Special:OAuth/device`, `Special:OAuthConsumers` and `Special:PendingSubsidiaries` are rendered by `triplespace-ui` over the routes of 0025 §9.

1. **`/oauth/authorize`** (at both paths) validates the request: the client, the redirect URI, the PKCE challenge, the requested grants, the consumer's status and the tenant's `consumer-policy`. It stores the validated request under a **request handle** with a ten-minute lifetime, where authorization codes are kept, and answers `303` to `Special:OAuth/authorize?request={handle}`. A request that fails validation is answered by the API as before and never redirected to an unverified URI.
2. **The consent page** reads `GET /oauth/requests/{handle}`: the consumer, the grants on offer and the subsidiaries the person may choose (0025 §3). Only the primary account in whose session the handle was created may read it; anyone else gets `404`.
3. **The decision** is a form post to `POST /oauth/requests/{handle}/approve`, with the CSRF token, the chosen or new subsidiary and any narrowed grants, or to `POST /oauth/requests/{handle}/deny`. The API checks everything again, issues the code, and returns the consumer's redirect URL, which the web tier answers with `303`.

- **The device flow** joins at step 2: `Special:OAuth/device` posts the user's code to `POST /oauth/requests`, which returns the handle of the pending device authorization.
- **The consent and device pages cannot be framed:** they are sent with `frame-ancestors 'none'`.
- **Request handles, authorization codes and device codes survive a change of replica.** Without a shared cache they are kept in `private` through `triplespace-accounts`, as sessions are ([0014](0014-caches-and-search.md) §2), not in process, so that any replica of an API pool can finish a flow another began.

### 14. Purging (amends 0042 §10, §14)

- **`index.php?title={title}&action=purge`** is a page of the site: the confirmation form MediaWiki shows, whose submission calls the API's `action=purge`.
- **A plain `action=purge` on a local page** bumps the page's render epoch, deletes its `p:` keys and purges its `Cache-Tag`, so the next read renders afresh; `forcelinkupdate` and `forcerecursivelinkupdate` also queue the refreshes 0042 §10 already gives them. On an entity it purges the entity's `Cache-Tag`. On a foreign title it does what [0053](0053-mirrored-pages.md) §6 says.
- **Any principal who may read the target** may purge it, by `POST` and without a token as in MediaWiki, counted in the `parse` rate class as 0053 has it for foreign titles.
- **The new epoch changes the page's `ETag`,** so the web tier's revalidation (§5) sees the change on the next request, and the CDN, purged by tag, refills from fresh content.

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
