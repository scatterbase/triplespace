# 0012. API requirements for the site UI

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-09 (A63)
- **Author:** James Hare / Claude Opus
- **Changes:** [0000](0000-init.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0005](0005-crate-organization.md)
- **Uses:** [0001](0001-revision-metadata-rdf.md), [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [01](../architecture/01-log-and-records.md), [05](../architecture/05-providers-and-ingest.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

## Context

[0010](0010-site-ui.md) specifies the site UI, and [0011](0011-logs.md) the log events it shows. [mediawiki-compat.md](../api/mediawiki-compat.md) records the MediaWiki and Wikibase APIs that Triplespace must stay compatible with. Those APIs leave two gaps.

1. **Data that only Triplespace has.** MediaWiki has no API for any of these:
   - source graphs and provenance;
   - identity clusters;
   - mirror syncs and upstream history;
   - jobs;
   - corrections;
   - integrity proofs;
   - account links and private sign-in bindings.
2. **MediaWiki modules over composed data.** `prop=revisions` and `list=recentchanges` assume that each page has one sequence of revisions, numbered from one sequence per wiki. An entity page is a view over several partitions ([0002](0002-source-graphs-and-mass-ingest.md) §3), and log offsets are numbered per partition.

This ADR sets the rules for filling both gaps, and lists what the UI needs from the API. It also names two decisions that must come first (§2).

## Decision

### 1. Rules for the API surface

*Current text: [18](../architecture/18-api.md) §1.1, §8.*

### 2. Prerequisites

*Current text: [18](../architecture/18-api.md) §1.3, §5.1.*

#### 2.1 Global revision and log IDs

*Changed by A2, A4, A7.*

*Current text: [01](../architecture/01-log-and-records.md) §2.5, §5.3.*

#### 2.2 Mirror metadata that survives compaction (amends [0002](0002-source-graphs-and-mass-ingest.md) §8.2)

*Changed by A58.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §2.6, §8.2.*

#### 2.3 Ordering and paging

*Current text: [18](../architecture/18-api.md) §1.3.*

#### 2.4 Size

*Current text: [18](../architecture/18-api.md) §1.3.*

### 3. The activity row

*Changed by A5, A12, A18.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §3.1.*

### 4. Action API changes

*Changed by A3, A4, A5, A7, A8, A9, A10, A12, A13, A14, A15, A16, A17, A18, A19, A21, A23, A24, A25, A26, A27, A30, A31, A32, A33, A34, A35, A36, A37, A38, A55.*

*Current text: [18](../architecture/18-api.md) §1.5, §2.1, §2.2, §2.3, §2.4, §3.4, §5.1.*

### 5. REST routes under `rest.php/triplespace/v0`

*Changed by A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25, A26, A28, A30, A31, A32, A33, A37, A39, A40, A41, A42, A43, A44, A45, A46, A47, A48, A49, A50, A51, A52, A53, A54, A55, A56, A59, A60, A61, A62.*

*Current text: [18](../architecture/18-api.md) §1.2, §3.1, §3.2, §3.3, §4.*

### 6. Fetching upstream edits live

*Changed by A4, A13.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §2.7.*

### 7. Structured diffs

*Current text: [18](../architecture/18-api.md) §5.1.*

### 8. Redaction, caching and permissions

*Changed by A3, A5, A12, A13, A24, A35, A57.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.1, §2.2, §5.3, §5.7, §5.8.*

### 9. Crates

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **MediaWiki clients keep working.** Everything that only Triplespace can do sits in a separate namespace. That namespace can change freely while it is `v0`.
- **The UI works against a stock Wikibase too.** It discovers which Triplespace features exist and hides the rest.
- **There are two API families to maintain,** plus an OpenAPI contract that has to be kept in step with the code.
- **Sync ingest does more work.** Each changed entity is compared with its previous mirrored state to produce `changes`, and the `put` record grows by a few fields. Full deltas are opt-in because of their size.
- ~~**Global revision and log IDs block much of this ADR.** They need their own decision before the MediaWiki history and change modules are implemented.~~ *Decided by [0013](0013-postgres-storage.md) §6 and [0015](0015-record-format-and-partition-registry.md) §2 (A2, A4).*
- **Redaction per viewer complicates caching.** Only anonymous responses are cached publicly.
- **Upstream fetches put load on the provider.** The rate limits and the cap in §6 are part of being a good API citizen, not optional tuning.

## Open questions

- **Q1.** ~~**The global ID mechanism** (§2.1): a central allocator, IDs reserved in blocks for each partition, or another scheme.~~ *Settled by [0013](0013-postgres-storage.md) §6 (Postgres sequences per tenant, taken in the appending transaction; blocks from the bootstrap coordinator) and [0015](0015-record-format-and-partition-registry.md) §2 (in the header; provider-ranged for mirror records).*
- **Q2. The default for `sync_deltas`,** once ingest cost and storage have been measured.
- **Q3.** ~~**Contributions of foreign actors:** whether `/actor/{key}/contributions` covers upstream edits that were backfilled or fetched live.~~ *Settled by [0018](0018-tenants.md) §8: backfilled and observed records the instance holds; never fetched live.*
- **Q4. The effect of a diff:** whether it should be "as of the change" wherever that can be computed, as it can for the local graph alone.
- **Q5.** ~~**A watchlist API,** which waits on the watchlist decision in [0010](0010-site-ui.md).~~ *Settled by [0020](0020-change-feeds.md) §5: MediaWiki's watchlist modules and the `/watchlist` routes.*
- **Q6.** ~~**A change feed for downstream consumers.** Whether Triplespace offers an EventStreams-like stream, and whether it follows the same redaction and log model ([mediawiki-compat.md](../api/mediawiki-compat.md) §7).~~ *Settled by [0020](0020-change-feeds.md) §4: the everything feed as server-sent events, with the same redaction; [0022](0022-federation.md) §1 publishes it for verified sync.*
- **Q7.** ~~**Rate limit values and cache lifetimes** for upstream fetches.~~ *Settled by [0024](0024-subsidiary-accounts.md) §5, in part: rate limits are site policy by action class, with `upstream` as one class; its default is a starting value. Cache lifetimes are Q9.*
- **Q8.** ~~**Which groups hold which rights** (§8).~~ *Settled by [0016](0016-permissions-and-access-control.md) §2, and extended per ADR since; `docs/registry/groups.toml` is the registry of record.*
- **Q9. Cache lifetimes for upstream fetches.** (Rest of Q7.) A [0014](0014-caches-and-search.md) tuning item.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0000](0000-init.md) Q5 | §1 | settles | 0000 Q5 |
| [0002](0002-source-graphs-and-mass-ingest.md) §8.2 | §2.2 | extends | 0002 A4 |
| [0003](0003-statement-ui.md) Q3 | §5 | settles | 0003 Q3 |
| [0005](0005-crate-organization.md) §2 | §9 | extends | 0005 A7 |

## References

- [MediaWiki API contract](../api/mediawiki-compat.md)
- [API:Continue](https://www.mediawiki.org/wiki/API:Continue), [API:Etiquette](https://www.mediawiki.org/wiki/API:Etiquette)
- [Manual:User rights](https://www.mediawiki.org/wiki/Manual:User_rights)
- [OpenAPI 3.1](https://spec.openapis.org/oas/v3.1.0)

## Amendment log

### A1. Crate names

- **Date:** 2026-09-26
- **Source:** [0005](0005-crate-organization.md) §2, revision of 2026-09-26
- **Change:** supersedes §9
- **Summary:** 0005 §2 became the one crate table CI checks; `triplespace-activity` was absorbed by `triplespace-projections`.

Replaced text (§9):

> | Crate | Change |
> |---|---|
> | `triplespace-api-rest` | The `triplespace/v0` routes and the OpenAPI contract |
> | `triplespace-api-action` | The additive module changes in §4 |
> | `triplespace-api-ingest` | The job routes of §5 |
> | `triplespace-activity` ([0010](0010-site-ui.md) §13) | Serves history, activity, contributions and `list=logevents` |
> | `triplespace-upstream` (new, surfaces) | The live upstream client: cache, rate limits and the error mapping in §6 |
> | `triplespace-accounts` ([0007](0007-actor-identity.md) §10) | The login and account routes |
> | `scatter-wikibase-changeset` | The new `put` fields of §2.2 |
> | `scatter-ingest` | Computing the change summaries and optional deltas |

### A2. Global IDs are Postgres sequences

- **Date:** 2026-09-26
- **Source:** [0013](0013-postgres-storage.md) §6
- **Change:** amends §2.1
- **Summary:** The revision, log and page IDs are Postgres sequences, taken in the appending transaction; blocks come from the bootstrap coordinator. This settled Q1, with 0015 §2, and falsified the consequence that the IDs block this ADR.

Replaced text (§2.1):

> The mechanism is left to its own ADR, which should be written before any of these modules is implemented. It must meet these requirements:

### A3. Caching and search

- **Date:** 2026-09-26
- **Source:** [0014](0014-caches-and-search.md) §1, §5–6, §8
- **Change:** amends §8; extends §4
- **Summary:** Shared caches hold only the public form, for a short `s-maxage`, and only erasure purges; authenticated responses are `private, no-cache`. `siprop=triplespace` reports the search backend. The privacy test also covers cache entries and search documents.

Replaced text (§8):

> - Anonymous responses carry `ETag` and may be cached publicly.
> - Authenticated responses vary by session.

### A4. Global IDs in the header

- **Date:** 2026-09-26
- **Source:** [0015](0015-record-format-and-partition-registry.md) §1–2, §4
- **Change:** amends §2.1, §4; extends §6
- **Summary:** The sequences are Postgres sequences, one set per tenant ([0018](0018-tenants.md) §2), taken in the appending transaction and written into header fields 7–9. Mirror records *do* carry a revision ID, provider-ranged (`provider_number << 40 | n`), computed by the writer, so that `lastrevid`, `baserevid` and `schema:version` are defined for an entity with no local revision; the requirement above is amended accordingly. Erasure is per part, and each erased part sets its own hidden flag. The live fetch of §6 reads backfilled upstream revisions from their records.

Replaced text (§2.1):

> - **IDs are assigned when a record is appended, and stored in the record.** An ID that came from replay order would change whenever projections were rebuilt in a different interleaving.
> - **Mirror records get no local revision ID.** They are not revisions of this wiki. They carry the upstream revision ID instead.

Replaced text (§4):

> Erased revisions come back with MediaWiki's hidden flags (`texthidden`, `commenthidden`, `userhidden`) and a new `erased` flag.

### A5. Permissions

- **Date:** 2026-09-26
- **Source:** [0016](0016-permissions-and-access-control.md) §2, §6–7
- **Change:** amends §8; extends §3, §4, §5
- **Summary:** Groups hold the rights of §8, and `docs/registry/groups.toml` is the registry of record; the permission modules and the `/acl` routes are added; an erasure's reason class is visible to `ts-viewerasures`. This settled Q8.

Replaced text (§8):

> **Rights.** Rights are named here. Which groups hold them is left open.

### A6. Bare keys are not IDs

- **Date:** 2026-09-27
- **Source:** [0017](0017-entity-id-grammar.md) §1
- **Change:** amends §5
- **Summary:** `GET /resolve` offers a bare string that normalizes to a keyed type's key as a suggestion, since it is not an ID.

Replaced text (§5):

> | `GET /resolve?q=` | Resolves any string to its canonical page through the title resolver ([0008](0008-namespaces-and-document-pages.md) §3). This includes bare IDs such as `WDQ65` and domain keys. |

### A7. Tenants

- **Date:** 2026-09-27
- **Source:** [0018](0018-tenants.md) §2, §8, §11
- **Change:** amends §2.1; extends §4, §5
- **Summary:** The ID sequences are per tenant. The host selects the tenant; instance-level routes, including mirror sync jobs, are served at the farm base. This settled Q3: contributions cover the records the instance holds, never fetched live.

Replaced text (§2.1):

> - **One sequence for revisions and one for log events.** Each spans every partition that holds this instance's revisions or events: `local`, `pages`, `log`, and the actor records that project as log events.

### A8. Thread routes

- **Date:** 2026-09-27
- **Source:** [0019](0019-discussions.md) §9
- **Change:** extends §4, §5
- **Summary:** The thread routes, and a refusal of `action=edit` on thread and talk titles, then with `ts-structured`.

### A9. Watchlists and the change stream

- **Date:** 2026-09-27
- **Source:** [0020](0020-change-feeds.md) §4–5
- **Change:** extends §4, §5
- **Summary:** MediaWiki's watchlist modules, the feed parameters of `GET /activity`, `GET /activity/stream` and the `/watchlist` routes. This settled Q5 and Q6.

### A10. Notifications

- **Date:** 2026-09-27
- **Source:** [0021](0021-notifications.md) §7
- **Change:** extends §4, §5
- **Summary:** Echo's modules, the `/inbox` and `/notifications` routes.

### A11. Verified sync

- **Date:** 2026-09-27
- **Source:** [0022](0022-federation.md) §1, §11
- **Change:** extends §5
- **Summary:** `/dumps/local/`, `/.well-known/tlog/`, provider verification. §5 had been given this row in place, among the holder-only routes.

### A12. Moderation

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §8
- **Change:** extends §3, §4, §5, §8
- **Summary:** The protection, deletion, hiding and patrol modules; `record` and `actor` ACL kinds; the patrol route; `patrolled` in the activity row; a privacy-test case per ACL target kind.

### A13. Subsidiaries, keys and rate limits

- **Date:** 2026-09-27
- **Source:** [0024](0024-subsidiary-accounts.md) §4–5, §8
- **Change:** extends §4, §5, §6, §8
- **Summary:** Bot-password login for subsidiaries, keys and rate-limit reporting, the subsidiary routes, and signatures returned by the record routes. The `clientlogin` row had been given the `action=login` clause in place. This settled the rate-limit half of Q7.

### A14. The OAuth server

- **Date:** 2026-09-27
- **Source:** [0025](0025-oauth-server.md) §1, §9
- **Change:** extends §4, §5
- **Summary:** The OAuth 2.0 endpoints, consumer and authorization routes. §5 had been given the endpoint row in place, among the holder-only routes.

### A15. Sitelinks

- **Date:** 2026-09-27
- **Source:** [0026](0026-sitelinks.md) §7
- **Change:** extends §4, §5
- **Summary:** `wbsetsitelink` by host or URL, sitelink routes.

### A16. Preferences and portability

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §6
- **Change:** extends §4, §5
- **Summary:** `action=options` over registered keys, and the account routes for preferences, export and import.

### A17. Resolvers

- **Date:** 2026-09-27
- **Source:** [0029](0029-resolver-namespaces.md) §6
- **Change:** extends §4, §5
- **Summary:** `/resolve` tries resolver keys, `/resolvers` routes, and `resolver` and `key` in `wbgetentities`.

### A18. Edit filters

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §5, §9
- **Change:** extends §3, §4, §5
- **Summary:** The AbuseFilter-shaped modules and filter routes; the activity row's `tags` get a source, and the row gains `filters`.

### A19. Constraints

- **Date:** 2026-09-27
- **Source:** [0031](0031-property-constraints.md) §6
- **Change:** extends §4, §5
- **Summary:** `wbcheckconstraints` and the report routes; the provenance response gains `constraints`.

### A20. The SPARQL Update stream

- **Date:** 2026-09-27
- **Source:** [0032](0032-sparql-update-stream.md) §6
- **Change:** extends §5
- **Summary:** `/updates/stream`, `/updates`, `/updates/cursor` and the batch files under `/dumps/updates/`. §5 had been given this row in place, among the holder-only routes.

### A21. The hash guard

- **Date:** 2026-09-28
- **Source:** Direct: James, decision of 2026-09-28, recorded as 0006 A9
- **Change:** extends §4, §5
- **Summary:** `siprop=triplespace` reports `hash_mismatches`, and `GET /jobs/{id}` the count by value type. §4 and §5 had been edited in place.

### A22. Adopted entities

- **Date:** 2026-09-28
- **Source:** [0035](0035-adopting-a-wikibase.md) §6
- **Change:** extends §5
- **Summary:** The provenance routes report an adopted entity's or page's source wiki, adoption job and `source_revid`.

### A23. Page statements and categories

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §8, §13
- **Change:** extends §4, §5
- **Summary:** Page statements (`/page/{pageid}/statements`, following the Wikibase REST API's statement routes), categories, category members and category mappings. In the Action API: `prop=categories`, `list=categorymembers`, `prop=categoryinfo` and `list=allcategories`. `/resolve` tries main-namespace titles last.

### A24. Files

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §17
- **Change:** extends §4, §5, §8
- **Summary:** The file modules (`action=upload`, `action=filerevert`, `prop=imageinfo`, `meta=filerepoinfo` and the rest), the `/file` and `/blob` routes, and the farm-base `/takedowns` routes.

### A25. Content models

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §8–9
- **Change:** extends §4, §5
- **Summary:** As noted under each section before the migration:
  - §4: Every page has a content model: `meta=siteinfo` reports `defaultcontentmodel` where it is not `wikitext`, and `siprop=triplespace` gains `contentmodels`; `prop=info` and `prop=revisions` report a model for every page; `rvslots` takes `main` and, on File pages, `mediainfo`. `action=edit` on a non-text model is refused with `no-direct-editing`, and `action=changecontentmodel` uses MediaWiki's `changecontentmodel-nodirectediting`, `changecontentmodel-cannotbeused` and `nochanges`.
  - §5: Page objects carry `content_model` for every page, and File page summaries carry `mediainfo_id`.

### A26. Template expansion and rendering

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §12, §14
- **Change:** extends §4, §5
- **Summary:** As noted under each section before the migration:
  - §4: `action=expandtemplates`; expansion, `prop=templates` and the limit report in `action=parse`; `prop=templates`, `prop=transcludedin`, `list=embeddedin` and `list=alltransclusions`; `magicwords`, `functionhooks`, `extensiontags`, `variables` and `doubleunderscores` in `meta=siteinfo`; `inprop=linkclasses`; `meta=allmessages`; and, with Lua, `prop=wbentityusage`, `list=wblistentityusage` and `wikibase_item` in `prop=pageprops`.
  - §5: `GET /page/{id}/render` returns a page's render manifest, and `GET /page/{id}/html` takes `as_of=revision` to render a revision with its dependencies as of the revision's offset.

### A27. Lua

- **Date:** 2026-09-30
- **Source:** [0043](0043-lua-modules.md) §16
- **Change:** extends §4
- **Summary:** `prop=wbentityusage`, `list=wblistentityusage` and `wikibase_item` in `prop=pageprops`.

### A28. Tables

- **Date:** 2026-09-30
- **Source:** [0045](0045-table-content-model.md) §10
- **Change:** extends §5
- **Summary:** Tables: `GET /table/{pageid}/rows` (the grid's rows, paged, with resolved cells), `POST /table/{pageid}/rows/{id}` (one row's cell edits as one change set) and `GET /table/{pageid}/export`.

### A29. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §2–9
- **Summary:** A1–A28 were folded into the Decision; the additions of later ADRs are indexed in two tables, one in §4 and one in §5, which point to the ADR that specifies each module or route. The open questions were numbered; Q7, partly struck, was struck whole, and its rest is Q9. No decision changed. Before this, A2 and A4 were one blockquote, A23–A28 were blockquotes, A11, A13–A14, A20 and A21 had been edited in place without a note, and the other entries were recorded only in other ADRs. The file before conversion is commit `0b26a3a`.

### A30. Redirects

- **Date:** 2026-10-01
- **Source:** [0051](0051-page-redirects.md) §7
- **Change:** extends §4, §5
- **Summary:** MediaWiki's redirect parameters and modules, and `redirect_to` and `redirected_from` in REST.

### A31. Page repositories

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §6
- **Change:** extends §4, §5
- **Summary:** `origin` in `prop=info` and page summaries, provider-ranged page IDs, `tsorigin=all`, `ts-foreign-page`, `page_repos` in siteinfo, and `GET /page/stack/{title}`.

### A32. Mirrored pages

- **Date:** 2026-10-01
- **Source:** [0053](0053-mirrored-pages.md) §10
- **Change:** extends §4, §5
- **Summary:** Parse, revision content, purge, HTML and render routes for inherited pages, and `GET /repo/{name}`.

### A33. Forks

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §11
- **Change:** extends §4, §5
- **Summary:** `tsfork` on `action=edit`, seeded revisions in the revision modules, `forkedfrom`, and the fork routes.

### A34. TemplateData and page properties

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §5–6
- **Change:** extends §4
- **Summary:** `action=templatedata`, `prop=pageprops`, `list=pageswithprop`, `prop=description` and `inprop=displaytitle`, served from `view.page_prop`.

### A35. Absence, and the security model's test cases

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §5, §13, §15
- **Change:** extends §4, §8
- **Summary:** A target under a confidential `read` restriction is absent for principals outside its group, not redacted: the missing-target answer from every route, and omission from every list, count and log. The privacy test gains 0056 §15's cases, and `instance check`'s failures. `action=protect` takes `read=`; `list=protectedsets` is added.

### A36. What the write modules write; the API paths

- **Date:** 2026-10-03
- **Source:** Direct: James, decision of 2026-10-03 (`triplespace-accounts`, `triplespace-api-action`, `triplespace-server`, `scatter-adapter-internetdomains`)
- **Change:** extends §4
- **Summary:** A write module computes the entity as the editor wants it and writes the difference from the current state (0002 §8.1, a batch of one): a new entity is a `create`; additions are one `add`; retractions one `remove`; a changed statement is a `remove` of its GUID then an `add` under the same GUID; a mirrored statement the editor removes or changes is suppressed with an `override` and, if changed, re-added under a fresh local GUID; a Domain's first local write is an `add` (0009 §10). All the records of one edit go in one transaction with the base offset carried from record to record, so `baserevid` yields either the whole edit or `editconflict`, which carries `currentrevid`; an unknown `baserevid` is `nosuchrevid`; conflicts are never patched around. The module reports the last record's revision as `lastrevid`. `api.php` is served at `/w/api.php` and `/api.php` (`scriptpath` is `/w`, `articlepath` `/wiki/$1`); `Special:EntityData/{id}.json` and `/entity/{id}` (303 to the former) are routes of the server.

### A37. `api_version` and same-origin `returnto`

- **Date:** 2026-10-03
- **Source:** [0057](0057-web-tier.md) §7, §9
- **Change:** extends §4, §5
- **Summary:** `siprop=triplespace` reports `api_version`, so a web tier deployed apart from the API can tell whether the API is recent enough. `action=clientlogin`, `action=createaccount` and the `/auth` routes honour `returnto` only on the request's own origin, so the web tier's forms can return the user to the page they came from without opening a redirect. The OAuth routes gain the request handle of 0057 §13: `POST /oauth/requests`, `GET /oauth/requests/{handle}` and its `approve` and `deny`, which the site's consent and device pages use.

### A38. The theme in `siteinfo`

- **Date:** 2026-10-03
- **Source:** [0034](0034-frontend-stack.md) §1
- **Change:** extends §4
- **Summary:** `siprop=triplespace` reports `theme`, the tenant's `ui.theme` token values, so the site, which reaches the instance only through the API, can serve the tenant's theme. An outsider of a private tenant is not given it, since 0056 §13 gives an outsider only the name, language and privacy of the wiki; its landing page uses Codex's own values.

### A39. SPARQL queries

- **Date:** 2026-10-04
- **Source:** [0059](0059-query-service.md) §6
- **Change:** extends §5
- **Summary:** `/sparql` implements the SPARQL 1.1 Protocol for queries over the tenant's dataset, with the cursor and truncation headers, and `/sparql/prefixes` serves the pre-declared prefix set.

### A40. Scope routes

- **Date:** 2026-10-04
- **Source:** [0060](0060-scopes.md) §8
- **Change:** extends §5
- **Summary:** The scope routes: count and cursor, paged members, export, refresh, and the scopes a subject is in; `list=scopemembers` and the `rcscope` feed parameter in the Action API.

### A41. Sprint routes

- **Date:** 2026-10-04
- **Source:** [0061](0061-sprints-and-tasks.md) §10
- **Change:** extends §5
- **Summary:** The sprint routes: counts and window, paged tasks, leaderboard, claim and release, and the open tasks about a subject; `list=sprinttasks` in the Action API.

### A42. Workspace routes

- **Date:** 2026-10-05
- **Source:** [0062](0062-workspaces.md) §8
- **Change:** extends §5
- **Summary:** Scope preview and facets for the builder; a workspace's roles and components; creation from a kit; the kit list.

### A43. Query page routes

- **Date:** 2026-10-05
- **Source:** [0063](0063-query-namespace.md) §7
- **Change:** extends §5
- **Summary:** Running a `Query:` page with parameters, and its parameter schema.

### A44. EntitySchema routes and modules

- **Date:** 2026-10-05
- **Source:** [0064](0064-entityschema-and-validation.md) §7
- **Change:** extends §5
- **Summary:** The schema entity, its report, the ad hoc check, the schemas bound to an entity, and the Action API modules on `E`.

### A45. MediaInfo terms

- **Date:** 2026-10-05
- **Source:** [0065](0065-mediainfo-captions-and-commons.md) §4
- **Change:** extends §5
- **Summary:** Term modules accepted on `M`; `WDM` through `wbgetentities`; the file's resolved MediaInfo route.

### A46. Lexeme modules and routes

- **Date:** 2026-10-05
- **Source:** [0066](0066-lexemes.md) §8
- **Change:** extends §5
- **Summary:** WikibaseLexeme's modules, lexeme search types, and the lexeme and part REST routes.

### A47. Proposal routes

- **Date:** 2026-10-05
- **Source:** [0067](0067-proposals.md) §7
- **Change:** extends §5
- **Summary:** Opening, reading, exporting, pushing and withdrawing proposals; a subject's proposals.

### A48. Merge routes

- **Date:** 2026-10-05
- **Source:** [0068](0068-merging-with-upstream.md) §6
- **Change:** extends §5
- **Summary:** Merge previews in either direction, pulls, re-follow, and `tsmerge`.

### A49. Followed talk pages and pins

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §9
- **Change:** extends §5
- **Summary:** Routes for foreign threads, upstream replies and sections, pins and a fork's talk-follow state; DiscussionTools modules on followed talk titles.

### A50. Shallow mirroring routes

- **Date:** 2026-10-07
- **Source:** [0070](0070-shallow-entity-mirroring.md) §9
- **Change:** extends §5
- **Summary:** Mirror status and manual fetch per provider; `mirror` in `siprop=providers`.

### A51. Extraction routes

- **Date:** 2026-10-07
- **Source:** [0071](0071-derived-statements-from-mirrored-pages.md) §13
- **Change:** extends §5
- **Summary:** Extraction sources, runs, orphans and unmapped strings; the derivations of a page and of an entity.

### A52. Publication routes

- **Date:** 2026-10-07
- **Source:** [0074](0074-publishing-a-scope-to-an-external-wiki.md) §8
- **Change:** extends §5
- **Summary:** A publication's status and a manual run.

### A53. The MCP endpoint

- **Date:** 2026-10-07
- **Source:** [0075](0075-mcp-server.md) §6
- **Change:** extends §5
- **Summary:** `{base}/mcp` per tenant, reported in `siteinfo` and the REST root.

### A54. Dataset routes

- **Date:** 2026-10-07
- **Source:** [0076](0076-dataset-publication.md) §3, §6
- **Change:** extends §5
- **Summary:** Referrers by property, dataset descriptions, scope dumps, lookup by URL, and the schema.org profile of `Special:EntityData`.

### A55. Special:Version

- **Date:** 2026-10-07
- **Source:** [0077](0077-special-version.md) §10
- **Change:** extends §4, §5
- **Summary:** `GET /version`; `siprop=extensions` lists only extensions whose modules are served, `siprop=libraries` the third-party components, `general.dbtype` and `dbversion` PostgreSQL subject to `version.services`, and `siprop=triplespace` the build.

### A56. Entity sources

- **Date:** 2026-10-08
- **Source:** [0078](0078-entity-sources.md) §10
- **Change:** extends §5
- **Summary:** `GET /entity-sources`, public, lists the tenant's entity sources with their IRI templates; siteinfo gains `entity_sources`; source IDs work in `wbgetentities` and `/resolve`; the shallow-mirroring routes take a source's name where they take a provider code.

### A57. What purges a public cache

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §8
- **Summary:** Public caches are purged by erasure, and also by deletion or hiding by `read` ACL ([0023](0023-moderation.md) §5, [0014](0014-caches-and-search.md) §5); "only erasure purges them" was wrong about what 0014 §5 and 0023 already decided. (PENDING C17)

Replaced text (§8):

> - Anonymous responses carry `ETag` and may be cached publicly, for a minute at a shared proxy; only erasure purges them ([0014](0014-caches-and-search.md) §5–6).

### A58. `sync_deltas` in the provider registry

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §2.2
- **Summary:** `sync_deltas` (`summary` by default, or `full`) is a field of the provider's entry in `docs/registry/providers.toml`; it is added to the TOML schema and the file's header. (PENDING E2)

### A59. No `list=threads`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §5
- **Summary:** There is no `list=threads` module; `origin=` is a parameter of `GET /page/{id}/threads` only, so the followed-talk-pages row of §5's index no longer names `thorigin` on `list=threads` ([0069](0069-synchronized-talk-pages.md) §9 is corrected the same way). (PENDING E34)

Replaced text (§5):

> | Followed talk pages and pins | Foreign threads in `GET /page/{id}/threads` (`origin=`) and `GET /thread/{id}`; `POST /thread/{id}/upstream-reply`, `POST /page/{id}/upstream-topic`, `GET /upstream-post/{id}`; `POST /thread/{id}/pin`, `/unpin`; `POST /page/{id}/talk-follow`; `discussiontoolsedit` and `discussiontoolspageinfo` on followed talk titles, `thorigin` on `list=threads`, `ts-upstream-talk` | [0069](0069-synchronized-talk-pages.md) §9 |

### A60. `GET /page/{id}/history`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5
- **Summary:** `GET /page/{id}/history`, the activity rows (§3) of a document page or thread, is defined in §5's page table, from [0019](0019-discussions.md) §7; 0019 §9 stops calling it unchanged. (PENDING E36)

### A61. The index of later routes moves to chapter 18

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §5
- **Summary:** §5's index of routes added by later ADRs is replaced by a link to [chapter 18](../architecture/18-api.md) §3.2, which holds the one route catalogue; the index had drifted from the ADRs it pointed at. (PENDING E39)

Replaced text (§5):

> **Routes added by later ADRs.** Each ADR specifies its routes in full; this table says where.
>
> | Area | Routes | ADR |
> |---|---|---|
> | Access control | `GET`, `PUT` and `DELETE /acl/{kind}/{id}`, for graph, entity, record and actor targets | [0016](0016-permissions-and-access-control.md) §7, [0023](0023-moderation.md) §8 |
> | Patrol | `POST /activity/{partition}/{offset}/patrol` | [0023](0023-moderation.md) §8 |
> | Threads | `/thread` and its operations; `GET /page/{id}/threads` | [0019](0019-discussions.md) §9 |
> | Watchlist and streams | `GET /activity/stream`, server-sent events for any feed; `/watchlist` and its Atom token | [0020](0020-change-feeds.md) §4–5 |
> | Notifications | `/inbox`; `/notifications/…` | [0021](0021-notifications.md) §7 |
> | Verified sync | `/.well-known/tlog/{partition}/checkpoint`, `/dumps/local/`, `GET /providers/{slug}/verification`: what an instance publishes for verified sync, and how a reader reports it | [0022](0022-federation.md) §1, §11 |
> | Subsidiaries and keys | `/account/subsidiaries/…` and their keys; `POST /actor/{key}/transfer` | [0024](0024-subsidiary-accounts.md) §8 |
> | OAuth | `/oauth/authorize`, `/oauth/token`, `/oauth/device`, `/oauth/revoke`, `/oauth/identify`, `/.well-known/oauth-authorization-server`: the instance's own OAuth 2.0 server, whose tokens are credentials of a subsidiary, never of a primary account. `/oauth/consumers/…`; `/account/authorizations`, `/account/pending-subsidiaries`; `/oauth/requests/{handle}`, read and decided by the site's consent page ([0057](0057-web-tier.md) §13) | [0025](0025-oauth-server.md) §1, §9 |
> | Sitelinks | `GET /sitelinks`, `GET /sitelinks/resolve`, `/entity/{id}/sitelinks/{host}` | [0026](0026-sitelinks.md) §7 |
> | Preferences and portability | `/account/preferences`, `GET /account/export`, `POST /account/import`, `GET /preferences/schema` | [0027](0027-preferences-and-portability.md) §6 |
> | Resolvers | `GET /resolvers`, `GET /resolvers/{name}/{key}` | [0029](0029-resolver-namespaces.md) §6 |
> | Edit filters | `/filters/…`, `GET /filters/log` | [0030](0030-edit-filters.md) §9 |
> | Constraints | `GET /entity/{id}/constraints`, `GET /constraints/report`, `GET /constraints/counts`, `POST /constraints/recheck` | [0031](0031-property-constraints.md) §6 |
> | SPARQL Update stream | `GET /updates/stream`, `GET /updates`, `GET /updates/cursor`, `/dumps/updates/`: keeps a triplestore current with a dump; public, rate-limited in the `stream` class | [0032](0032-sparql-update-stream.md) §6 |
> | SPARQL queries | `GET` and `POST /sparql`, the SPARQL 1.1 Protocol for queries only, over the tenant's dataset, with `Query-Cursor` and `Query-Truncated` headers; `GET /sparql/prefixes`; rate class `query` | [0059](0059-query-service.md) §6 |
> | Page statements and categories | `/page/{pageid}/statements`, following the Wikibase REST API's statement routes; `GET /page/{pageid}/categories`, `GET /category/{title}/members`, `/category-mappings/…` | [0038](0038-page-metadata-and-categories.md) §13 |
> | Files | `/file/{title}` and its versions and usage, `GET /blob/{sha256}/references`; `/takedowns` and `POST /expunge` at the farm base | [0039](0039-files-and-media.md) §17 |
> | Content models | Page summaries carry `content_model`, and File page summaries `mediainfo_id` | [0041](0041-content-models.md) §9 |
> | Rendering | `GET /page/{id}/render`, the render manifest; `GET /page/{id}/html` with `as_of=revision`, rendering a revision with its dependencies as of the revision's offset | [0042](0042-template-expansion-and-parsoid.md) §12, §14 |
> | Tables | `GET /table/{pageid}/rows`, `POST /table/{pageid}/rows/{id}`, `GET /table/{pageid}/export` | [0045](0045-table-content-model.md) §10 |
> | Scopes | `GET /scope/{pageid}`, `GET /scope/{pageid}/members`, `GET /scope/{pageid}/export`, `POST /scope/{pageid}/refresh`, `GET /subject/{kind}/{id}/scopes`; in the Action API `list=scopemembers` and `rcscope` | [0060](0060-scopes.md) §8 |
> | Sprints | `GET /sprint/{pageid}`, `/tasks`, `/leaderboard`; `POST /sprint/{pageid}/tasks/{rule}/{kind}/{id}/claim` and `/release`; `GET /subject/{kind}/{id}/tasks`; in the Action API `list=sprinttasks` and `letype=task` | [0061](0061-sprints-and-tasks.md) §10 |
> | Workspaces | `POST /scope/preview` (count, sample and shared statements for an unsaved definition; rate class `query`), `GET /scope/{pageid}/facets`, `GET /workspace/{pageid}`, `POST /workspace` (create from a kit), `GET /kits` | [0062](0062-workspaces.md) §8 |
> | Query pages | `GET /query/{pageid}/run?{param}=…` with `/sparql`'s headers and formats plus the labelled table; `GET /query/{pageid}/params` | [0063](0063-query-namespace.md) §7 |
> | EntitySchema | `GET /entity/E1`; `GET /schema/{id}/report`, `POST /schema/{id}/check`, `GET /entity/{id}/schemas`; Action API `wbgetentities` and the term modules on `E`, `wbsearchentities&type=entityschema`, `action=entityschema` | [0064](0064-entityschema-and-validation.md) §7 |
> | MediaInfo | `wbsetlabel`, `wbsetdescription` and `wbeditentity` terms on `M` accepted; `wbgetentities` on `WDM`; `GET /file/{title}/mediainfo` | [0065](0065-mediainfo-captions-and-commons.md) §4 |
> | Lexemes | WikibaseLexeme's modules (`wbladdform` … `wblmergelexemes`), `wbeditentity` with `new=lexeme`, `wbsearchentities&type=lexeme|form|sense`; REST `GET /entity/L1` and `GET /entity/L1-F1` | [0066](0066-lexemes.md) §8 |
> | Proposals | `POST /proposal`, `GET /proposal/{thread}`, `POST /proposal/{thread}/export`, `/push`, `/withdraw`, `GET /entity/{id}/proposals`, `GET /page/{id}/proposals`; `list=proposals`, `letype=proposal` | [0067](0067-proposals.md) §7 |
> | Merging with upstream | `GET /page/{id}/merge-preview?direction=`, `POST /page/{id}/merge`, `POST /page/{id}/refollow`; `tsmerge` on `action=edit` | [0068](0068-merging-with-upstream.md) §6 |
> | Followed talk pages and pins | Foreign threads in `GET /page/{id}/threads` (`origin=`) and `GET /thread/{id}`; `POST /thread/{id}/upstream-reply`, `POST /page/{id}/upstream-topic`, `GET /upstream-post/{id}`; `POST /thread/{id}/pin`, `/unpin`; `POST /page/{id}/talk-follow`; `discussiontoolsedit` and `discussiontoolspageinfo` on followed talk titles, `thorigin` on `list=threads`, `ts-upstream-talk` | [0069](0069-synchronized-talk-pages.md) §9 |
> | Redirects | `redirect_to` in page summaries; `redirected_from` from `GET /resolve` | [0051](0051-page-redirects.md) §7 |
> | Page repositories | `GET /page/{id}` with a provider-ranged ID, carrying `origin`, `upstream` and `stack`; `GET /page/stack/{title}` | [0052](0052-page-repositories-and-title-inheritance.md) §6 |
> | Mirrored pages | `GET /page/{id}/html` and `/render` for an inherited page; `GET /repo/{name}` | [0053](0053-mirrored-pages.md) §10 |
> | Forks | `POST /page/fork`, `GET /page/{id}/fork`, `GET /page/{id}/upstream-diff`, `POST /page/{id}/fork/files`; the fork's origin in `GET /page/{id}/provenance` | [0054](0054-forking-a-mirrored-page.md) §11 |
> | Shallow mirroring | `GET /provider/{code}/mirror`, `POST /provider/{code}/mirror/fetch`; `mirror` per type in `siprop=providers` | [0070](0070-shallow-entity-mirroring.md) §9 |
> | Entity sources | `GET /entity-sources`; `entity_sources` in `siprop=triplespace`; source IDs in `wbgetentities` and `/resolve`; a source's name in place of `{code}` in the shallow-mirroring routes | [0078](0078-entity-sources.md) §10 |
> | Extraction | `GET /extraction/{source}`, `POST /extraction/{source}/run`, `GET /extraction/{source}/orphans`, `GET /extraction/{source}/unmapped`; `GET /page/{id}/derivations`, `GET /entity/{id}/derivations` | [0071](0071-derived-statements-from-mirrored-pages.md) §13 |
> | Publications | `GET /publication/{name}`, `POST /publication/{name}/run` | [0074](0074-publishing-a-scope-to-an-external-wiki.md) §8 |
> | MCP | The MCP endpoint `{base}/mcp`, outside `rest.php`, reported by `siprop=triplespace` and the REST root | [0075](0075-mcp-server.md) §1, §6 |
> | Datasets | `GET /entity/{id}/referrers`, `GET /scope/{title}/dataset.jsonld`, `GET /scope/{title}/dumps` and `/dumps/{file}`, `GET /jsonld/by-url`; the `profile` parameter of `Special:EntityData/{id}.jsonld` | [0076](0076-dataset-publication.md) §2–6 |
> | Version | `GET /version`: everything `Special:Version` shows, per tenant base and at the farm base | [0077](0077-special-version.md) §10 |

### A62. Notation keys in `/resolve`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §5
- **Summary:** The order `/resolve` tries folds in [0029](0029-resolver-namespaces.md) §6's step: a string that normalizes to a notation key ([0048](0048-notation.md) §2) resolves, as the one exception to the bare-key rule; any other bare key of a keyed type is a suggestion, since bare keys are not IDs ([0017](0017-entity-id-grammar.md) §1). (PENDING E40)

Replaced text (§5):

> | `GET /resolve?q=` | Resolves any string to its canonical page through the title resolver ([0008](0008-namespaces-and-document-pages.md) §3). It tries, in order: entity IDs such as `WDQ65`; keyed-type IDs; resolver-prefixed strings (`doi:10.1000/xyz`) and bare strings that normalize to a key of exactly one resolver ([0029](0029-resolver-namespaces.md) §6); then main-namespace titles, with a same-titled page returned in `also` ([0038](0038-page-metadata-and-categories.md) §8). A bare string that normalizes to a key of some keyed type, such as a domain name, is offered as a suggestion, since bare keys are not IDs ([0017](0017-entity-id-grammar.md) §1). |

### A63. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [01](../architecture/01-log-and-records.md), [05](../architecture/05-providers-and-ingest.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
