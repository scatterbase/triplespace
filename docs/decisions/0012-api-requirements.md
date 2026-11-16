# 0012. API requirements for the site UI

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-11-15 (A93)
- **Author:** James Hare / Claude Opus
- **Uses:** [0001](0001-revision-metadata-rdf.md), [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [MediaWiki API contract](../api/mediawiki-compat.md)

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

*Current text: [01](../architecture/01-log-and-records.md) §2.5, §5.3.*

#### 2.2 Mirror metadata that survives compaction (amends [0002](0002-source-graphs-and-mass-ingest.md) §8.2)

*Current text: [05](../architecture/05-providers-and-ingest.md) §2.6, §8.2.*

#### 2.3 Ordering and paging

*Current text: [18](../architecture/18-api.md) §1.3.*

#### 2.4 Size

*Current text: [18](../architecture/18-api.md) §1.3.*

### 3. The activity row

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §3.1.*

### 4. Action API changes

*Current text: [18](../architecture/18-api.md) §1.5, §2.1, §2.2, §2.3, §2.4, §3.4, §5.1.*

### 5. REST routes under `rest.php/triplespace/v0`

*Current text: [17](../architecture/17-federation-and-publication.md) §1.1; [18](../architecture/18-api.md) §1.2, §3.1, §3.2, §3.3, §4.*

### 6. Fetching upstream edits live

*Current text: [05](../architecture/05-providers-and-ingest.md) §2.7.*

### 7. Structured diffs

*Current text: [18](../architecture/18-api.md) §5.1.*

### 8. Redaction, caching and permissions

*Current text: [09](../architecture/09-security-and-moderation.md) §2.1, §2.2, §5.3, §5.7, §5.8.*

### 9. Crates

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **MediaWiki clients keep working.** Everything that only Triplespace can do sits in a separate namespace. That namespace can change freely while it is `v0`.
- **The UI works against a stock Wikibase too.** It discovers which Triplespace features exist and hides the rest.
- **There are two API families to maintain,** plus an OpenAPI contract that has to be kept in step with the code.
- **Sync ingest does more work.** Each changed entity is compared with its previous mirrored state to produce `changes`, and the `put` record grows by a few fields. Full deltas are opt-in because of their size.
- ~~**Global revision and log IDs block much of this ADR.** They need their own decision before the MediaWiki history and change modules are implemented.~~ *Decided by [0013](0013-postgres-storage.md) §6 and [0015](0015-record-format-and-partition-registry.md) §2 (A2, A4).*
- **Redaction per viewer complicates caching.** Only anonymous responses are cached publicly.
- **Upstream fetches put load on the provider.** The rate limits and the cap in §6 are part of being a good API citizen, not optional tuning.

## Open questions

- **Q1. The global ID mechanism.** *Settled by [0013](0013-postgres-storage.md) §6 (Postgres sequences per tenant, taken in the appending transaction; blocks from the bootstrap coordinator) and [0015](0015-record-format-and-partition-registry.md) §2 (in the header; provider-ranged for mirror records).*
- **Q2. The default for `sync_deltas`,** once ingest cost and storage have been measured.
- **Q3. Contributions of foreign actors.** *Settled by [0018](0018-tenants.md) §8.* *Extended by [0010](0010-site-ui.md) A40.*
- **Q4. The effect of a diff.** *Settled by [0083](0083-write-path-in-three-tiers.md) §4.*
- **Q5. A watchlist API,.** *Settled by [0020](0020-change-feeds.md) §5.*
- **Q6. A change feed for downstream consumers.** *Settled by [0020](0020-change-feeds.md) §4.*
- **Q7. Rate limit values and cache lifetimes.** *Settled by [0024](0024-subsidiary-accounts.md) §5, in part.*
- **Q8. Which groups hold which rights.** *Settled by [0016](0016-permissions-and-access-control.md) §2, and extended per ADR since; `docs/registry/groups.toml` is the registry of record.*
- **Q9. Cache lifetimes for upstream fetches.** (Rest of Q7.) A [0014](0014-caches-and-search.md) tuning item.

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

Replaced text: §9 of this ADR.

### A2. Global IDs are Postgres sequences

- **Date:** 2026-09-26
- **Source:** [0013](0013-postgres-storage.md) §6
- **Change:** amends §2.1
- **Summary:** The revision, log and page IDs are Postgres sequences, taken in the appending transaction; blocks come from the bootstrap coordinator. This settled Q1, with 0015 §2, and falsified the consequence that the IDs block this ADR.

Replaced text: §2.1 of this ADR.

### A3. Caching and search

- **Date:** 2026-09-26
- **Source:** [0014](0014-caches-and-search.md) §1, §5–6, §8
- **Change:** amends §8; extends §4
- **Summary:** Shared caches hold only the public form, for a short `s-maxage`, and only erasure purges; authenticated responses are `private, no-cache`. `siprop=triplespace` reports the search backend. The privacy test also covers cache entries and search documents.

Replaced text: §8 of this ADR.

### A4. Global IDs in the header

- **Date:** 2026-09-26
- **Source:** [0015](0015-record-format-and-partition-registry.md) §1–2, §4
- **Change:** amends §2.1, §4; extends §6
- **Summary:** The sequences are Postgres sequences, one set per tenant ([0018](0018-tenants.md) §2), taken in the appending transaction and written into header fields 7–9. Mirror records *do* carry a revision ID, provider-ranged (`provider_number << 40 | n`), computed by the writer, so that `lastrevid`, `baserevid` and `schema:version` are defined for an entity with no local revision; the requirement above is amended accordingly. Erasure is per part, and each erased part sets its own hidden flag. The live fetch of §6 reads backfilled upstream revisions from their records.

Replaced text: §2.1 of this ADR; §4 of this ADR.

### A5. Permissions

- **Date:** 2026-09-26
- **Source:** [0016](0016-permissions-and-access-control.md) §2, §6–7
- **Change:** amends §8; extends §3, §4, §5
- **Summary:** Groups hold the rights of §8, and `docs/registry/groups.toml` is the registry of record; the permission modules and the `/acl` routes are added; an erasure's reason class is visible to `ts-viewerasures`. This settled Q8.

Replaced text: §8 of this ADR.

### A6. Bare keys are not IDs

- **Date:** 2026-09-27
- **Source:** [0017](0017-entity-id-grammar.md) §1
- **Change:** amends §5
- **Summary:** `GET /resolve` offers a bare string that normalizes to a keyed type's key as a suggestion, since it is not an ID.

Replaced text: §5 of this ADR.

### A7. Tenants

- **Date:** 2026-09-27
- **Source:** [0018](0018-tenants.md) §2, §8, §11
- **Change:** amends §2.1; extends §4, §5
- **Summary:** The ID sequences are per tenant. The host selects the tenant; instance-level routes, including mirror sync jobs, are served at the farm base. This settled Q3: contributions cover the records the instance holds, never fetched live.

Replaced text: §2.1 of this ADR.

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
- **Summary:** The API side of 0057's web tier: `siprop=triplespace` reports `api_version`, so a web tier deployed apart from the API can check it; `returnto` is honoured only on the request's own origin, so login forms can return the user without opening a redirect; the OAuth routes gain 0057 §13's request handle for the consent and device pages.

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

Replaced text: §8 of this ADR.

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

Replaced text: §5 of this ADR.

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

Replaced text: §5 of this ADR.

### A62. Notation keys in `/resolve`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §5
- **Summary:** The order `/resolve` tries folds in [0029](0029-resolver-namespaces.md) §6's step: a string that normalizes to a notation key ([0048](0048-notation.md) §2) resolves, as the one exception to the bare-key rule; any other bare key of a keyed type is a suggestion, since bare keys are not IDs ([0017](0017-entity-id-grammar.md) §1). (PENDING E40)

Replaced text: §5 of this ADR.

### A63. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9

### A64. Any cluster member is accepted; responses are rewritten to the request's form

- **Date:** 2026-10-09
- **Source:** [0082](0082-source-form-and-the-shared-view.md) §2, §3
- **Change:** amends §4; extends §5
- **Summary:** The API side of 0082's source form: every entry point accepts any member of an identity cluster and normalizes it before anything runs, and a layer on the way out rewrites IDs to the consumer's preferred form (`prefer`), using exact matches only. Chapter text: [18](../architecture/18-api.md) §1.4, §1.6, §2.3, §3.1. (REVIEW G4, G5)

Replaced text: [18](../architecture/18-api.md) §2.3.

### A65. Revision states are rows; the stream's resume token is per partition

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §4, §7
- **Change:** amends §7; extends §2.1, §2.3, §4
- **Summary:** The API side of 0083's revision state: a past state is one row of `view.entity_revision`, with nothing replayed, and rendering as of a revision is exact for local content and current for mirrored contributions, since mirror states are not kept under `latest`. "From the tenant's view as of that offset where the log holds it, and current otherwise" is withdrawn. The activity stream's `Last-Event-ID` is a vector of per-partition high-water marks. 0083's table said `amends` for §4; nothing there was contradicted, so §4 is extended here and the table was corrected. Chapter text: [18](../architecture/18-api.md) §2.3. (REVIEW G11, G14)

Replaced text: [18](../architecture/18-api.md) §5.2.

### A66. Wikibase writes against the resolved view; `lastrevid`, the base check, patching and `maxlag`

- **Date:** 2026-10-09
- **Source:** [0084](0084-wikibase-writes-against-the-resolved-view.md) §1, §2, §3, §4, §5
- **Change:** amends §2.1, §4; extends §2
- **Summary:** The API side of 0084: a `wb*` write, or a REST write that maps to one, is diffed against the tenant's resolved view before a change set is built, so echoed mirrored content is dropped and only what is local-owned is written; a value change on a mirror-owned statement is refused with `ts-foreign-statement`. `lastrevid` is the entity's newest local revision when it has one, the base check is per source graph, and a stale local base that touches nothing a later edit touched is patched rather than refused. "The check runs against the newest record for the key across the source partitions" is withdrawn from §2.1 ([01](../architecture/01-log-and-records.md) §2.5). Chapter text: [18](../architecture/18-api.md) §2.5. (REVIEW G16, G17)

Replaced text: [01](../architecture/01-log-and-records.md) §2.5; [18](../architecture/18-api.md) §1.5, §2.3.

### A67. Page IDs are per tenant; a mirrored entity's is provider-ranged

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §2.1
- **Summary:** A local page ID is taken from the tenant's own sequence the first time a key is written in a tenant partition, stays below 2^40, and is repeated in field 9 by every later record for the key; a mirrored entity's page ID is `provider_number << 40 | upstream page ID` where the provider publishes one and `provider_number << 40 | mirror offset` otherwise, computed by the writer with no allocation, so `log."instance.page_id"` is not used for entities, and `view.entity`'s unique key on the page ID is `(tenant, page_id)`; bootstrap writers of a tenant partition take page IDs in blocks. The ledger names [0013](0013-postgres-storage.md) §6, [0015](0015-record-format-and-partition-registry.md) §2, [0052](0052-page-repositories-and-title-inheritance.md) §6 and [0008](0008-namespaces-and-document-pages.md) §4; the sentence stood at [01](../architecture/01-log-and-records.md) §2.5, whose provenance is §2.1 here, and the ledger row was extended. (REVIEW G18)

Replaced text: [01](../architecture/01-log-and-records.md) §2.5.

### A68. Every ID a client sees is below 2^53

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §2.1; extends §2
- **Summary:** Registry provider numbers are below 2^12, and a tenant assigns its entity-source numbers from 2^12 to 2^13 − 1, so `number << 40 | n` stays under 2^53 for every `n` below 2^40 and a ranged `revid` or `pageid` survives a JavaScript `Number` and every JSON parser, which `formatversion=2` has no string form to protect; the range 2^22 to 2^23 − 1 is withdrawn. The ledger names [0078](0078-entity-sources.md) §4, [0015](0015-record-format-and-partition-registry.md) §2 and §5 and [0080](0080-tenants-as-entity-sources.md) §1; the range stood at [01](../architecture/01-log-and-records.md) §2.5 (§2.1 here) and the rule is restated at [18](../architecture/18-api.md) §1.3 (§2), and the ledger row was extended. (REVIEW G20)

Replaced text: [01](../architecture/01-log-and-records.md) §2.5.

### A69. Every tenant-partition record takes a `logid` at append

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §2.1
- **Summary:** A log ID is assigned to every record in `local`, `pages`, the tenant `log` and the tenant `actors` partitions at append, whether or not the log-event catalogue projects that record as an event, so that the rule is a function of the partition and never of what a later projection chooses to show. The ledger named [0011](0011-logs.md) §2; the rule stands at [01](../architecture/01-log-and-records.md) §2.5, whose provenance is §2.1 here and [0015](0015-record-format-and-partition-registry.md) §2, and the ledger row was corrected. (REVIEW G24)

Replaced text: [01](../architecture/01-log-and-records.md) §2.5.

### A70. The page bundle, bulk labels and bounded counts

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §5
- **Summary:** An entity page reaches first byte within a budget of four API calls, met by two routes: `GET /labels?ids=&language=`, bulk labels for up to 1,000 entity and property IDs in the given language chain and nothing else, each ID resolved through its cluster, public form only and cacheable by language; and `GET /entity/{id}/page`, the page bundle of the entity JSON, the provenance response and the identity-line facts in one response, cached by `resolved_version`. The provenance response's backlink count is read from the `ref_count` counter column of `view.entity`, updated in batches and reported as a bounded count (`10000+`) above a threshold. The in-process transport resolves the session once per page and is exempt from rate counting ([0057](0057-web-tier.md) §2); the About panel has its own response and `max-age` ([0010](0010-site-ui.md) §4). (REVIEW G41)

### A71. `GET /entity/{id}/history` reads `view.entity_history`

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §5
- **Summary:** `GET /entity/{id}/history` returns the entity's history as activity rows read from `view.entity_history`, the one backing of a mirrored entity's history: every record in any source partition and every upstream log event keyed to any member of its cluster, in one keyset query; nothing is merged from `view.activity`, which holds no upstream log event, and `list=logevents` with `leprovider` reads a provider's events from the same table. The ledger names [0013](0013-postgres-storage.md) §5, [0011](0011-logs.md) §4 and §9 and [0010](0010-site-ui.md) §5 and §13; the route is [18](../architecture/18-api.md) §3.2 (§5 here), and the ledger row was extended. (REVIEW G42)

Replaced text: [18](../architecture/18-api.md) §3.2.

### A72. `siteinfo` is a function of configuration; live counters move to their routes

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** corrects §4; extends §5
- **Summary:** Every `siprop` is computed from the tenant's registry and settings and cached by (registry offset, tenant, visibility); no part of it counts rows or reads a queue. The live numbers move to where they belong: the instance-wide hash-mismatch count and projection lag to `GET /version`, a provider's set size, queue length and stream lag to `GET /provider/{code}/mirror`, and a page repository's index date, event lag and mirrored-page count to `GET /repo/{name}`, all maintained as counters and never by `COUNT(*)`; `hash_mismatches`, `index_as_of`, `event_lag` and `mirrored_pages` leave `siprop=triplespace`. `private.watch` gains `watch_target (target_kind, target_id) WHERE notify` and `private.upstream_grant` stores encrypted tokens, not hashes, under [0020](0020-change-feeds.md) §6 and [0022](0022-federation.md) §8. Chapter text: [18](../architecture/18-api.md) §2.2. (REVIEW G47)

Replaced text: [18](../architecture/18-api.md) §2.2.

### A73. `rckind`, and `source`, `job` and `operator` on the feed modules

- **Date:** 2026-10-15
- **Source:** Direct: James, review of the first activity-feeds pull request, 2026-10-15
- **Change:** extends §4
- **Summary:** `list=recentchanges`, `list=usercontribs` and `list=logevents` are three target sets over the one feed query of [0020](0020-change-feeds.md) §1, and their rows carry what the query already holds: `rcprop`, `ucprop` and `leprop` gain `source` (the graph), `job` and `operator`, the values §4 had given `prop=revisions` alone, so a bot that reads recent changes learns a row's graph and job without a second call; and `rckind=edit|sync|log|job|erased` selects the row kinds of `view.activity` directly, which is how a client asks for the mirror syncs that `rctype` leaves out, since `rctype=external` keeps its Wikibase-client meaning. The chapter rows are [18](../architecture/18-api.md) §2.3.

Replaced text: [18](../architecture/18-api.md) §2.3.

### A74. `rvprop=record` names a revision's record

- **Date:** 2026-10-16
- **Source:** Direct: James, review of the activity-feeds pull requests, 2026-10-15
- **Change:** extends §4
- **Summary:** `prop=revisions` gains `rvprop=record`, which returns the revision's `{partition, offset}`. A diff is a record ([0083](0083-write-path-in-three-tiers.md) §4; [18](../architecture/18-api.md) §5.1), and the site addresses a diff by revision ID as MediaWiki does (`?diff={revid}`), so the site resolves the ID to the record through this field with one `prop=revisions` call rather than a route of its own. MediaWiki clients never ask for it. The chapter row is [18](../architecture/18-api.md) §2.3.

### A75. Settings routes, the site's identity in `siteinfo`, and its cache key

- **Date:** 2026-10-17
- **Source:** [0085](0085-settings-and-preferences.md) §2.1, §3, §7
- **Change:** amends §4; extends §5
- **Summary:** The API side of 0085's settings: `general.sitename`, `general.lang` and `general.logo` report the `site.*` settings instead of fixed values; `siteinfo` is cached by the instance's registry offset too, because a tenant's settings resolve through the instance's defaults; REST v0 gains the settings routes, with instance settings at the farm base only, so a single-tenant instance keeps the two apart. Chapter text: [18](../architecture/18-api.md) §2.2, §3.2.

Replaced text: [18](../architecture/18-api.md) §2.2, §3.2.

### A76. `siprop=triplespace` reports palettes and type sets

- **Date:** 2026-10-17
- **Source:** [0086](0086-theming.md) §5
- **Change:** amends §4; extends §5
- **Summary:** The API side of 0086's theming: `siprop=triplespace` reports the palettes and type sets a tenant offers in place of `ui.theme`'s token map, and REST v0 gains routes to list and manage them. Chapter text: [18](../architecture/18-api.md) §2.2, §3.2.

Replaced text: [18](../architecture/18-api.md) §2.2.

### A77. Challenges in AuthManager and write modules; account requests

- **Date:** 2026-10-17
- **Source:** [0087](0087-account-registration-and-challenges.md) §2, §6, §9
- **Change:** amends §4; extends §5
- **Summary:** The API side of 0087's registration: where a challenge trigger applies, account creation, login and a write that would create a temporary account answer `ts-challenge-required` until a challenge is solved; account creation is throttled and, with `account.approval` on, waits for approval; REST gains the request routes. Chapter text: [18](../architecture/18-api.md) §1.5, §2.2, §2.3, §3.2.

Replaced text: [18](../architecture/18-api.md) §2.3.

### A78. External issuers in AuthManager and the `/auth` routes

- **Date:** 2026-10-18
- **Source:** [0088](0088-external-identity-providers.md) §5, §7
- **Change:** extends §4; amends §5
- **Summary:** The API side of 0088's external identity providers, through MediaWiki's AuthManager flow: each issuer is an authentication request answered with a redirect, an identity with no account restarts into `createaccount`, and `unlinkaccount` refuses to remove the last way to log in. `POST /auth/pending/create` is withdrawn in favour of `createaccount`. Chapter text: [18](../architecture/18-api.md) §2.2, §2.3, §3.2.

Replaced text: [18](../architecture/18-api.md) §3.2.

### A79. The auth host's API

- **Date:** 2026-10-18
- **Source:** [0089](0089-the-auth-host-and-instance-logins.md) §5, §7, §9, §10, §11, §16
- **Change:** amends §4; extends §5
- **Summary:** The API side of 0089's auth host: at the auth base the Action API offers only its authentication modules, acting on instance logins; on a tenant with an auth host, login opens a tenant session only, and the credential modules answer `ts-auth-host`, since credentials live on the auth host. Chapter text: [18](../architecture/18-api.md) §2.2, §2.3, §3.2.

Replaced text: [18](../architecture/18-api.md) §2.2, §2.3, §3.2.

### A80. The provenance response links the upstream revision

- **Date:** 2026-10-27
- **Source:** [0003](0003-statement-ui.md) A17 (Direct: James, design discussion of 2026-10-27)
- **Change:** extends §5
- **Summary:** Each graph of `GET /entity/{id}/provenance` carries `upstream_url`, the page of its upstream revision, where the provider is a MediaWiki that publishes revision IDs. Chapter text: [18](../architecture/18-api.md) §3.2.

### A81. The Interface namespace

- **Date:** 2026-10-28
- **Source:** [0095](0095-the-interface-namespace.md) §5
- **Change:** extends §4
- **Summary:** `siprop=triplespace` reports the wiki's interface overrides and names, `interface` (0095 §5).

### A82. The route catalogue is a registry file

- **Date:** 2026-10-30
- **Source:** [0098](0098-pointers-from-birth.md) §5
- **Change:** consolidates §5
- **Summary:** The table of 18 §3.2 moved into `docs/registry/routes.toml` (route, purpose, sources), with the method shorthand of some rows (`GET`, `PUT`, `DELETE /acl/set/{id}`) expanded to one `METHOD /path` each. `architecture/catalogues.py` generates the table, derives its new *Built* column from the OpenAPI contract, and fails when the contract serves a route the catalogue lacks. That check found four built routes no ADR section names: `GET`/`PUT`/`DELETE /group/{name}` (delegated administration, 0016 A38), `GET /acl/empty-groups` (0056 Q4), `GET /purges` (the purge queue, 0056 §9) and `GET /version/licenses/{id}` (0077 §12); they are catalogued with those citations and the note that no ADR section names them yet. `GET /entity/{id}/page` and `GET /labels` had no source cell; they are A70's. No decision changed. The last commit in which the chapter carried the table is `0e67cce`.

Replaced text: [18](../architecture/18-api.md) §3.2, the lead.

### A83. The Action API catalogue is a registry file

- **Date:** 2026-10-30
- **Source:** [0098](0098-pointers-from-birth.md) §5
- **Change:** consolidates §4
- **Summary:** The three tables of 18 §2.3 moved into `docs/registry/action-api.toml` (kind, modules, what changes, sources); `architecture/catalogues.py` generates them and derives a *Built* column from the module lists `action=paraminfo` serves (`ACTION_MODULES` and `QUERY_MODULES` in `triplespace-api-action`). The "statement modules" row now names its eight `wb*` modules. No decision changed. The last commit in which the chapter carried the tables is `0e67cce`.

Replaced text: [18](../architecture/18-api.md) §2.3, the lead.

### A84. The record and proof routes' shapes

- **Date:** 2026-11-07
- **Source:** Direct: James, design discussion of 2026-11-07
- **Change:** extends §5
- **Summary:** Asked how to build the record and proof routes of §5 and 17 §1.1 (SCA-302), James chose: `GET /record/{partition}/{offset}` returns JSON wrapping the record's canonical CBOR, with the header decoded and the parts and signature beside it, rather than the bytes alone or JSON alone; inclusion and consistency proofs use the RFC 6962/9162 shapes in JSON (`leaf_index`, `tree_size`, `audit_path`), rather than a format of Triplespace's own; and the multi-proof `GET /record/proofs` is built now in its simple form, every node the range's proofs need sent once with each leaf's path as positions in that list, rather than deferred or compressed further. Chapter text: [17](../architecture/17-federation-and-publication.md) §1.1; [18](../architecture/18-api.md) §3.2.

### A85. What the record routes show, and to whom

- **Date:** 2026-11-07
- **Source:** Direct: Claude, while building the record and proof routes (SCA-302), 2026-11-07; confirmed by James, 2026-11-08
- **Change:** extends §5
- **Summary:** Calls made building the routes of A84. (1) A partition is served only when it is published to the caller: the tenant's own, export policy `public`, of no graph a confidential restriction hides from the caller; any other answers `partition-not-found`, as one that does not exist. The instance's own partitions, at the farm base, are not served yet. (2) A record is shown in the export scope's form for the caller ([0103](0103-export-transfer-and-backup.md) §1; [0006](0006-log-integrity-and-erasure.md) A31), decided by the same reader with the caller's rights: a target the caller may not read gives its parts as commitments, a confidential target's record is its leaf alone with every other field null, as a compacted record is ([0103](0103-export-transfer-and-backup.md) A1), and a revision-deleted part is withheld even from a caller who may read it elsewhere, since the routes serve the published form and the reader decides by the row's visibility bits alone. (3) Partition IDs are decimal strings in JSON, being 64 random bits; hashes and bytes are base64. (4) The multi-proof covers at most 1000 records. (5) A `hashed` partition has no checkpoint or proof routes (`no-checkpoints`); its record route works. (6) Responses are cacheable as `Public`, not `Stable`, since a checkpoint of a given size may gain witness cosignatures. Chapter text: [17](../architecture/17-federation-and-publication.md) §1.1; [18](../architecture/18-api.md) §3.2.

### A86. `batch=` on every write, and the job routes

- **Date:** 2026-11-08
- **Source:** [0104](0104-batches-and-the-job-runner.md) §6
- **Change:** extends §4 and §5
- **Summary:** The API side of 0104's batches: every write takes `batch=` and answers with the job's ID, a write naming a rejected batch is refused with `ts-batch-rejected`, and REST gains the job list and the reject and close routes. Chapter text: [18](../architecture/18-api.md) §2.3 (`action-api.toml`), §3.2 (`routes.toml`).

### A87. Every module served is catalogued; routes and modules are checked against the running code

- **Date:** 2026-11-09
- **Source:** [0108](0108-routes-modules-and-tenancy-switches-checked-against-the-running-code.md) §1
- **Change:** amends §4; extends §5
- **Summary:** The module catalogue lists every module the Action API serves, unchanged ones included, and CI fails a run that serves a module or route its catalogue lacks. Chapter text: [18](../architecture/18-api.md) §2.3, §3.2.

Replaced text: [18](../architecture/18-api.md) §2.3.

### A88. A catalogue row is built when everything it names is served

- **Date:** 2026-11-09
- **Source:** Direct: James, design discussion of 2026-11-09
- **Change:** amends §4, §5
- **Summary:** The *Built* column of the module and route catalogues ticks a row only when every module or route the row names is served, not when any one is: a row naming `action=linkaccount` beside two modules not served is not built. Chapter text: [18](../architecture/18-api.md) §2.3, §3.2.

Replaced text: [18](../architecture/18-api.md) §2.3, §3.2.

### A89. A route served through a more general one is built

- **Date:** 2026-11-09
- **Source:** Direct: Claude, while building the route catalogue's Built column, 2026-11-09; unconfirmed
- **Change:** amends §5
- **Summary:** A catalogue route is served when the contract serves it or a more general route whose parameter takes its literal segment: `GET /acl/set/{id}` is served by `GET /acl/{kind}/{id}`, which the router routes it to and which reads `set` as a target kind. The three set-ACL routes had read as unbuilt. Chapter text: [18](../architecture/18-api.md) §3.2.

Replaced text: [18](../architecture/18-api.md) §3.2.

### A90. An action that changes nothing warns

- **Date:** 2026-11-09
- **Source:** Direct: James, design discussion of 2026-11-09 ("The warning should apply anywhere an action results in a no op where it normally wouldn't"); which modules that is Claude, while building it, 2026-11-09; unconfirmed
- **Change:** extends §4
- **Summary:** An Action API module that would normally append a record, and writes nothing because what it was asked for is already so, succeeds with a warning under its own name in the `warnings` block, giving the reason and "Nothing was written." Calls made in the building: the modules are the twelve Wikibase write modules (an edit that leaves the entity as it is, [0002](0002-source-graphs-and-mass-ingest.md) A48 included), `action=edit` (beside `nochange`), `action=protect`, `action=revisiondelete` and `action=userrights`; a module that already refuses the case as MediaWiki does keeps its error; `action=watch`, `action=options`, `action=echomarkread`, `action=echomarkseen` and `action=setnotificationtimestamp`, whose effect is private state and idempotent by nature, do not warn; re-protecting a page with the settings it has, and adding a group a user already holds, still write their records and so are not no-ops. The REST routes are unchanged. Chapter text: [18](../architecture/18-api.md) §1.5.

### A91. A69's log ID rule holds for records appended from the change that builds it

- **Date:** 2026-11-13
- **Source:** Direct: James, design discussion of 2026-11-13
- **Change:** extends §2.1
- **Summary:** A69 gives every record of `local`, `pages`, the tenant `log` and the tenant `actors` partitions a log ID at append, and the write path does not do so yet for a changeset record. Asked whether logs already written are then written again, James: "existing local logs could use their existing log id so long as there isn't a risk of conflict." There is none: every log ID of a tenant is drawn from the tenant's one sequence, so a record appended later takes a number above every one issued. So a record already in such a partition keeps its header as it is, with the log ID it has or with none, since the ID is in the hashed header and adding one would rewrite the log; the rule holds for records appended from the change that builds it, and nothing, a verifier or an index, may assume a log ID on an earlier one. Chapter text: 01 §2.5.

### A92. The page bundle and bulk labels are built: their shapes and the public form

- **Date:** 2026-11-15
- **Source:** Direct: Claude, while building the page bundle and bulk labels routes (SCA-325), 2026-11-15; unconfirmed
- **Change:** extends §5
- **Summary:** The two routes of A70 are served, with these calls. **`GET /entity/{id}/page`:** the body is `{id, entity, provenance}`, the entity as `wbgetentities` serves it with every part and language and the provenance response unchanged, which already carries the identity-line facts; `datatypes` is added where a snak names a property without its data type (a statement written through the API without one), with that property's, so that the page tells an identifier from a statement without fetching each property; `visibility` is added for a restricted entity, as `ts-visibility` reports it, and `redirects: {from, to}` where the ID given is a key-mapped provider's own item ID. It answers `404` and `400` as `/provenance` does, the `404` public and tagged for a caller with no credential, as `wbgetentities` serves `missing`, and is cached by its content under the tag `entity:{id}`. **`GET /labels`:** (1) `language` carries the whole chain, separated by `|`, most wanted first, and the route adds no fallback, so the response is a function of its URL; the site sends its own term chain. (2) The answer is `{language, labels}`, keyed by each ID as given, each label with the language it was found in; an ID with no label in the chain, or that names no entity, is left out; an ID that is not one is a `400`. (3) The public form is what a caller with no account may read: an entity under a `read` restriction, a deleted one, and every entity of a private wiki is left out, whoever asks. So a member of a private wiki, and a group reading an entity confidential to it, get no label from this route, and the page shows the ID. (4) Labels are read from `view.term`; where the chain asks first for a language outside the instance's term languages, a mirrored entity's record is read for it, one entity at a time, since the L1 label cache of 03 §12.1 is not built. (5) A keyed entity's derived label stands in for a `mul` label it lacks, also for a keyed ID no graph holds. (6) The response carries no `Cache-Tag` per entity, as 20 §6 says of label dependencies. (7) The site asks for up to 1,000 IDs in one `GET`, at most 48 KiB of IDs in the URL; long keyed IDs split a call sooner. **The site** draws an entity page and an entity diff from the bundle and the bulk labels; a reader's page makes three API calls (`siteinfo`, the bundle, the labels). A signed-in viewer's makes five, since who they are and their inbox count are still two calls, and the watch star adds its own: folding those into one call is not done. Catalogue text: `routes.toml`, shown in 18 §3.2.

### A93. The viewer's session facts are one call

- **Date:** 2026-11-15
- **Source:** Direct: Claude, while bringing a signed-in entity page within four API calls (SCA-325), 2026-11-15; unconfirmed
- **Change:** extends §5
- **Summary:** A signed-in viewer's entity page now makes four API calls, where A92 left it at five and more with the watch star: `siteinfo`, the page bundle, the bulk labels, and one call for the session facts. That call is one `action=query` with `meta=userinfo|notifications`; on a page that shows the watch star it also carries `meta=tokens` (`watch` and `csrf`) and `prop=info` with `inprop=watched|notificationtimestamp` for the page's title, so the star's state, its form's token and the token that marks the page's changes seen come with it. The calls: (1) the frame makes this call on every page for a viewer with a credential, and only the entity page asks about a title with it; another page that shows the star still asks for it separately. (2) The API refuses a whole query when one part is not the viewer's to ask, as `meta=notifications` is not an anonymous viewer's, so a request whose cookie no longer names an account is asked again for `meta=userinfo` alone: a stale cookie costs a second call, not the page. (3) A part the API does not serve is absent and the page draws without it. (4) Marking a page's unseen changes seen remains a write of its own, made when there are any. `siteinfo` is counted among the four here; 19 §5.1 names the About panel's response as one of the four, which is not built. Chapter text: 19 §5.1.
