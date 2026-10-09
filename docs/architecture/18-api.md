# 18. API

This chapter describes the programming interfaces Triplespace serves: the rules every surface follows, the MediaWiki Action API as Triplespace changes and extends it, the REST routes under `rest.php/triplespace/v0` as one table, the ingest API, structured diffs and rendering as of a revision, raw SPARQL, and the MCP server. It assumes the log and its records from [01](01-log-and-records.md), the graphs and the query service from [02](02-graphs-rdf-and-query.md), the serving tables, caches and search from [03](03-storage-caches-and-search.md), entity IDs from [04](04-entities-and-identifiers.md), ingest from [05](05-providers-and-ingest.md), actors, keys and OAuth from [07](07-actors-and-accounts.md), tenants and bases from [08](08-tenants-and-instances.md), and permissions, ACLs and redaction from [09](09-security-and-moderation.md). The site UI that consumes these interfaces is [19](19-site-ui.md); the web tier that routes to them is [20](20-web-tier.md). The compatibility contracts in [`../api/`](../api/) are the reference this chapter builds on and does not repeat.

## 1. The API surface

*Sources: [0012](../decisions/0012-api-requirements.md) §1, §2; [0018](../decisions/0018-tenants.md) §11; [0044](../decisions/0044-tenant-relative-ids.md) §3.*

### 1.1 Rules

*Sources: [0012](../decisions/0012-api-requirements.md) §1; [0008](../decisions/0008-namespaces-and-document-pages.md) §12.*

1. **MediaWiki modules keep MediaWiki's meaning.** They describe changes made on this instance, one revision at a time. Pywikibot and other MediaWiki clients must work without changes. Reading and editing `Project` pages remains the acceptance test ([0008](../decisions/0008-namespaces-and-document-pages.md) §12).
2. **Changes to MediaWiki modules are additive.** They may add parameters, enum values and output fields that appear only when requested. They never change what an existing request returns.
3. **Data only Triplespace has goes in a new REST namespace, `rest.php/triplespace/v0`.**
   - `v0` means what it means for the vocabulary ([0005](../decisions/0005-crate-organization.md) §5): no stability promise yet.
   - An OpenAPI document, committed as `docs/api/triplespace-v0.openapi.json`, is the normative contract. The Wikibase REST API's OpenAPI document plays the same role ([mediawiki-compat.md](../api/mediawiki-compat.md) §6.2).
4. **The UI uses only the public API.** Anything the UI can show, a third-party client can show too.
   - The UI finds out which Triplespace-only features exist through capability discovery (§2.2, `siprop=triplespace`).
   - Against a stock Wikibase, those features are hidden.
5. **Every response is redacted for its viewer.**
   - Hidden and erased fields follow [0001](../decisions/0001-revision-metadata-rdf.md) §4, [0006](../decisions/0006-log-integrity-and-erasure.md) §7 and [0011](../decisions/0011-logs.md) §5 ([09](09-security-and-moderation.md)).
   - Data from a `private` graph ([0007](../decisions/0007-actor-identity.md) §8) is returned only to the account holder, in their own session.

### 1.2 Bases: the host selects the tenant

*Sources: [0018](../decisions/0018-tenants.md) §11; [0012](../decisions/0012-api-requirements.md) §5; [0028](../decisions/0028-tenancy-policy.md) §11; [0046](../decisions/0046-primary-tenant.md) §9; [0079](../decisions/0079-derived-issuer-codes.md) §7.*

**The host selects the tenant.** Every route is served per tenant base; `meta=siteinfo` describes the tenant, and `siprop=triplespace` names it and its opted-in providers. Instance-level routes (`/jobs` for sync jobs, the tenant list) are served at the farm base and, on a single-tenant instance, at the tenant's. The tenant list is the `tenants` array of `GET /tenancy` at the farm base (slug, issuer code as `issuer`, host, name, status), and `Special:Tenants` is its page. Mirror sync jobs are instance jobs, attested by the primary tenant's actors and listed at the farm base; local bulk jobs are tenant jobs ([08](08-tenants-and-instances.md)).

At the farm base, `meta=siteinfo&siprop=triplespace` reports the tenancy preset and switches, the farm slug, the farm code and the primary tenant's slug; `GET /tenancy` returns the policy and includes both slugs and the farm code, so a client can turn any actor key it meets into a tenant of the instance ([0079](../decisions/0079-derived-issuer-codes.md) §7) ([0028](../decisions/0028-tenancy-policy.md) §11, [0046](../decisions/0046-primary-tenant.md) §9). The UI halves of these sections, `Special:Tenants`, `Special:Tenancy` and the farm special pages, are in [19](19-site-ui.md) and [21](21-special-pages.md); the CLI commands of 0046 §9 (`instance create --farm-slug {slug}`; `primary offer {tenant}`, `primary withdraw`, `primary accept`) are in [23](23-configuration-and-registry.md). The refusals of 0046 §9 are `ts-primary-tenant` (moving, deleting or freezing the primary tenant) and `ts-no-offer` (accepting without an open offer).

### 1.3 Numeric IDs, ordering, paging and size

*Sources: [0012](../decisions/0012-api-requirements.md) §2, §2.3, §2.4.*

MediaWiki clients depend on numeric IDs from one sequence per wiki: `revid`, `parentid`, `oldid`, `baserevid`, `lastrevid`, `logid`, and `fromrev` and `torev` in `action=compare`. Diff URLs, `prop=revisions`, `list=recentchanges`, `list=logevents`, edit conflicts and the core REST history routes depend on them. The IDs are Postgres sequences, one set per tenant, taken in the appending transaction and written into the record's hashed header; the sequence increases with append order, gaps are acceptable, and erasure does not free an ID. Mirror records get a provider-ranged revision ID, `provider_number << 40 | n`, so that `lastrevid`, `baserevid` and `schema:version` are defined for an entity with no local revision; they are still not revisions of this wiki, and upstream log events keep their upstream log ID and get no local one. The sequences themselves are [01](01-log-and-records.md) §2.5 and [03](03-storage-caches-and-search.md) §2.3. The mirror metadata a `put` carries for the history view (`prev_upstream`, `first_seen`, `size`, `prev_size`, `changes`, `delta`) and the `sync_deltas` provider setting are [05](05-providers-and-ingest.md); §5.1 says how a diff uses them.

**Ordering and paging.** Lists that merge several partitions need one order that every client sees the same way.

- **Every activity row has a time and a position.** The time is when the event happened at its source. The position is the (partition, offset) of the record the row comes from.
- **Rows are ordered by (time, partition, offset).** This is a total order.
- **Continuation tokens are opaque.** Each encodes the last sort key returned and a hash of the filters. This follows MediaWiki's `continue` protocol ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.6).
- **A sync row's time** is when it was observed on the instance.
- **Upstream edits and upstream log events** keep their upstream timestamps.
- **Imported page revisions** keep their original timestamps ([0008](../decisions/0008-namespaces-and-document-pages.md) §9).

**Size.**

- **An entity's size** is the byte length of its canonical JSON in the record's source graph after the change. For the local graph, that is the local assertions about the entity. For a mirror, it is the mirrored state.
- **A size change** is measured against the same source's previous state.
- **The resolved view's size is not reported.** It depends on every source at once, and a source's earlier states may have been compacted.
- **A document page's size** is measured as in MediaWiki.

### 1.4 Where a tenant-relative entity ID is accepted

*Sources: [0044](../decisions/0044-tenant-relative-ids.md) §3.*

Tenant-relative IDs ([04](04-entities-and-identifiers.md) §2.3) are accepted everywhere an entity ID is accepted on input:

- the Action API: `wbgetentities&ids=`, entity values and IDs in `wbeditentity` and the statement modules, `Special:EntityPage/` and `Special:EntityData/`;
- REST under `triplespace/v0` and the Wikibase REST API's path segments;
- the change-set wire format of bulk jobs ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8), canonicalized during validation;
- `GET /resolve` and the search box's "Go to";
- Lua and `#property`/`#statements` ([0043](../decisions/0043-lua-modules.md) §8).

### 1.5 Refusals every write module shares

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §7; [0024](../decisions/0024-subsidiary-accounts.md) §8; [0030](../decisions/0030-edit-filters.md) §9; [0025](../decisions/0025-oauth-server.md) §9; [0012](../decisions/0012-api-requirements.md) §4.*

Every write module returns `permissiondenied` with the missing permission or grant named; `ratelimited` when over a limit ([07](07-actors-and-accounts.md) §6); and `abusefilter-warning` (with `abusefilter_token`) or `abusefilter-disallowed` when a filter acts ([09](09-security-and-moderation.md) §7). The OAuth refusals are `oauth-pending`, `oauth-consumer-disabled`, `oauth-not-allowed` when a tenant's `consumer-policy` denies the consumer, and the standard OAuth error responses at the token endpoint ([07](07-actors-and-accounts.md) §7). `action=edit`, `wbeditentity` and the other write modules map `baserevid` to base offsets ([0006](../decisions/0006-log-integrity-and-erasure.md) §8), and an `editconflict` error includes the current revision's ID, so the client can merge a document page three ways.

## 2. The Action API

*Sources: [0012](../decisions/0012-api-requirements.md) §4; [0008](../decisions/0008-namespaces-and-document-pages.md) §12; [0041](../decisions/0041-content-models.md) §8.*

### 2.1 Modules in scope

*Sources: [0008](../decisions/0008-namespaces-and-document-pages.md) §12; [0012](../decisions/0012-api-requirements.md) §4; [0041](../decisions/0041-content-models.md) §8; [0066](../decisions/0066-lexemes.md) §8; [0064](../decisions/0064-entityschema-and-validation.md) §7; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4; [0019](../decisions/0019-discussions.md) §9.*

Document namespaces bring these core modules into scope ([mediawiki-compat.md §5](../api/mediawiki-compat.md)):

- **Reading:** `action=parse`, `action=compare`, `query&prop=revisions`, `prop=info`, `prop=links`, `list=allpages`, `list=backlinks`, `list=prefixsearch`, `list=recentchanges` and `list=usercontribs`.
- **Writing:** `action=edit`, `action=move`, `action=delete`, `action=undelete`, `action=changecontentmodel` and `action=import`.

Pywikibot reading and editing `Project` pages is the acceptance test for this surface. Threads, boards, tables, scopes, sprints and query pages are read as any page: `prop=revisions`, `prop=info`, `action=parse`, `list=recentchanges`, `list=usercontribs`, `list=backlinks`, `action=watch`; `action=delete` and `action=undelete` on a `Thread:` title work. A definition page's content is read and written through `prop=revisions` and `action=edit` as any text model's.

The Wikibase modules accept `M` IDs as [0041](../decisions/0041-content-models.md) §7 describes, `E` IDs (`wbgetentities&ids=E1` returns the JSON of [0064](../decisions/0064-entityschema-and-validation.md) §1 with `type: "entityschema"`; `wbsearchentities&type=entityschema`; `wbsetlabel`, `wbsetdescription`, `wbsetaliases` on `E` IDs; `action=entityschema&text=` for the body), the term modules on `M` and `wbgetentities` on `WDM`, and WikibaseLexeme's modules with its parameters and errors — `wbladdform`, `wblremoveform`, `wbleditformelements`, `wbladdsense`, `wblremovesense`, `wbleditsenseelements`, `wblmergelexemes` — plus `wbeditentity` with `new=lexeme` or on `L` IDs, the statement modules on lexeme and part IDs, `wbgetentities` returning the full lexeme JSON for a lexeme ID and the part for a part ID, and `wbsearchentities` with the three types (`lexeme|form|sense`). Each lexeme module appends a change set whose operations address parts by ID; `scatter-wikibase-changeset` gains paths into `forms` and `senses`. Page statements are not in the Action API, except File pages', which are MediaInfo entities ([0038](../decisions/0038-page-metadata-and-categories.md) §13). Cell edits made with the Wikibase modules directly are ordinary entity edits: a bot can edit what a table shows without going through the table ([0045](../decisions/0045-table-content-model.md) §9).

### 2.2 `meta=siteinfo`

*Sources: [0012](../decisions/0012-api-requirements.md) §4; [0018](../decisions/0018-tenants.md) §11; [0016](../decisions/0016-permissions-and-access-control.md) §7; [0024](../decisions/0024-subsidiary-accounts.md) §8; [0025](../decisions/0025-oauth-server.md) §9; [0026](../decisions/0026-sitelinks.md) §7; [0028](../decisions/0028-tenancy-policy.md) §11; [0039](../decisions/0039-files-and-media.md) §17; [0041](../decisions/0041-content-models.md) §8; [0042](../decisions/0042-template-expansion-and-parsoid.md) §14; [0043](../decisions/0043-lua-modules.md) §16; [0045](../decisions/0045-table-content-model.md) §9; [0046](../decisions/0046-primary-tenant.md) §9; [0047](../decisions/0047-special-pages.md) §11; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6; [0053](../decisions/0053-mirrored-pages.md) §10; [0056](../decisions/0056-security-model.md) §13; [0070](../decisions/0070-shallow-entity-mirroring.md) §9; [0075](../decisions/0075-mcp-server.md) §6; [0077](../decisions/0077-special-version.md) §10; [0078](../decisions/0078-entity-sources.md) §10; [0079](../decisions/0079-derived-issuer-codes.md) §7.*

`meta=siteinfo` describes the tenant the host selects. On a private tenant it answers an outsider with the `general` section's name, language and the fact that the wiki is private, and nothing else. Namespaces come from the registry ([0008](../decisions/0008-namespaces-and-document-pages.md) §1).

| `siprop` | What Triplespace reports |
|---|---|
| `general` | `maxuploadsize`; `dbtype` is `postgres`, and `dbversion` is PostgreSQL's version unless `version.services` is `names` or `off`, in which case it is absent; `generator` is unchanged ([docs/clients.md](../clients.md)) |
| `namespaces` | `defaultcontentmodel` on every `pages` namespace whose default is not `wikitext`, as MediaWiki reports it: `wikibase-item` on 120, `triplespace-domain` on 210, `triplespace-thread` on 214, `triplespace-talk` on each enabled talk namespace, `triplespace-table` on 218; Module and Module talk while enabled |
| `providers` *(new)* | Codes, entity types, namespaces, IRI templates, whether upstream history can be fetched, and `mirror` per type |
| `issuers` *(new)* | The issuers allowed for login, without any secrets |
| `triplespace` *(new)* | Capabilities; `api_version`, which the web tier checks against the minimum it was built for ([0057](../decisions/0057-web-tier.md) §9); the REST base path; the tenant, with its slug and issuer code, and its opted-in providers; the search backend, `opensearch` or `postgres`; `theme`, the tenant's `ui.theme` token values, which the site serves as its stylesheet and which an outsider of a private tenant is not given, its landing page wearing Codex's own values; `hash_mismatches`, the instance-wide count of upstream hashes kept because the recomputation differed ([0006](../decisions/0006-log-integrity-and-erasure.md) A9); the rate-limit classes and the grant set; whether `subsidiaries.oauth_requires_approval` is on, so a tool can tell the person in advance that its account will await approval; `sitelinks: {schemes, mode, alias_count}` (no site table is reported, since there is none); the tenancy preset and switches; the farm slug, the farm code and the primary tenant's slug; `contentmodels`: each implemented model with its source, slot, format and direct-editing flag, so the UI finds out from the API which pages can change format, `Scribunto` among them, and `triplespace-table` with the limits `tables.max_rows` and `tables.max_columns`; the media base, separation and same-origin flag; `lua.ids` and `lua.client_site`; `page_repos`: each repository's name, kind, provider, served namespaces, mode, `shadowed`, `titles` and licence, as `meta=filerepoinfo` lists file repositories, with `index_as_of`, `event_lag` and, in `mirror` mode, `mirrored_pages`; `visibility`, `security.restrictions` and, in `development` mode, `insecure`; `entity_sources`, the list `GET /entity-sources` returns; the MCP endpoint when `mcp.enabled` is on; `build`: the version, commit and `modified` flag |
| `usergroups` | Every group with its permissions, reporting `universe` as `*` |
| `extensions` | Only extensions whose API Triplespace serves. An extension is listed when at least one module that [mediawiki-compat.md](../api/mediawiki-compat.md) attributes to it is served: `WikibaseRepository` today, and `WikibaseClient`, `Echo`, `AbuseFilter`, `OAuth`, `TemplateData` and the others as their modules are built. Each entry carries Triplespace's version, URL and licence, with `Triplespace` itself as a last entry of type `other`. A client that finds an extension listed will call its modules |
| `libraries` | Third-party crates and frontend packages as `{name, version}`, as MediaWiki lists its Composer and npm libraries |
| `magicwords`, `functionhooks`, `extensiontags`, `variables`, `doubleunderscores` | From the registry ([0042](../decisions/0042-template-expansion-and-parsoid.md) §5) |
| `protocols`, `interwikimap`, `languagevariants`, `defaultoptions`, `specialpagealiases` | For Parsoid ([0042](../decisions/0042-template-expansion-and-parsoid.md) §8.1); `specialpagealiases` from the special-page registry ([0047](../decisions/0047-special-pages.md) §2) |
| `fileextensions` | [0039](../decisions/0039-files-and-media.md) §17 |

### 2.3 The module catalogue

*Sources: [0012](../decisions/0012-api-requirements.md) §4; [0016](../decisions/0016-permissions-and-access-control.md) §7; [0019](../decisions/0019-discussions.md) §9; [0020](../decisions/0020-change-feeds.md) §5; [0021](../decisions/0021-notifications.md) §7; [0023](../decisions/0023-moderation.md) §8; [0024](../decisions/0024-subsidiary-accounts.md) §8; [0025](../decisions/0025-oauth-server.md) §9; [0026](../decisions/0026-sitelinks.md) §7; [0027](../decisions/0027-preferences-and-portability.md) §6; [0028](../decisions/0028-tenancy-policy.md) §11; [0029](../decisions/0029-resolver-namespaces.md) §6; [0030](../decisions/0030-edit-filters.md) §9; [0031](../decisions/0031-property-constraints.md) §6; [0038](../decisions/0038-page-metadata-and-categories.md) §13; [0039](../decisions/0039-files-and-media.md) §17; [0041](../decisions/0041-content-models.md) §8; [0042](../decisions/0042-template-expansion-and-parsoid.md) §14; [0043](../decisions/0043-lua-modules.md) §16; [0045](../decisions/0045-table-content-model.md) §9; [0047](../decisions/0047-special-pages.md) §11; [0049](../decisions/0049-boards.md) §11; [0051](../decisions/0051-page-redirects.md) §7; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6; [0053](../decisions/0053-mirrored-pages.md) §10; [0054](../decisions/0054-forking-a-mirrored-page.md) §11; [0056](../decisions/0056-security-model.md) §13; [0060](../decisions/0060-scopes.md) §8; [0061](../decisions/0061-sprints-and-tasks.md) §10; [0063](../decisions/0063-query-namespace.md) §7; [0064](../decisions/0064-entityschema-and-validation.md) §7; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4; [0066](../decisions/0066-lexemes.md) §8; [0067](../decisions/0067-proposals.md) §7; [0068](../decisions/0068-merging-with-upstream.md) §6; [0069](../decisions/0069-synchronized-talk-pages.md) §9; [0078](../decisions/0078-entity-sources.md) §10.*

Every module Triplespace adds or changes, with the parameters, values and output fields it adds, in three tables: `action=` modules, `action=query` modules (`meta=`, `prop=`, `list=` and generators), and the Wikibase and WikibaseLexeme modules. Modules offered in another extension's shape (Echo, AbuseFilter, OAuth, DiscussionTools, WikibaseQualityConstraints, CentralAuth) keep that extension's parameters, because the tools and the Wikipedia apps speak them. `meta=siteinfo` is §2.2.

**`action=` modules**

| Module | What Triplespace adds or changes | ADR |
|---|---|---|
| `action=abusefiltercheckmatch`, `action=abusefilterchecksyntax` | AbuseFilter's modules, over CEL | [0030](../decisions/0030-edit-filters.md) §9 |
| `action=abusefilterevalexpression`, `action=abusefilterunblockautopromote` | AbuseFilter's modules | [0030](../decisions/0030-edit-filters.md) §9 |
| `action=acquiretempusername` | Temporary accounts ([0007](../decisions/0007-actor-identity.md) §3) | [0012](../decisions/0012-api-requirements.md) §4 |
| `action=block`, `action=unblock` | As in MediaWiki. `action=block` gains `hidename`, which writes the actor ACL beside the block | [0016](../decisions/0016-permissions-and-access-control.md) §7; [0023](../decisions/0023-moderation.md) §8 |
| `action=changecontentmodel` | Between text models the namespace allows, including to or from `triplespace-sprint`. A page whose current model is not text: `changecontentmodel-nodirectediting`. A new model the namespace does not allow, including every non-text model: `changecontentmodel-cannotbeused`. The same model: `nochanges`. These are the codes MediaWiki core gives. On a foreign primary: `ts-foreign-page` | [0041](../decisions/0041-content-models.md) §8; [0061](../decisions/0061-sprints-and-tasks.md) §10; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `action=clientlogin`, `action=createaccount`, `meta=authmanagerinfo` | Offer a redirect-based OAuth flow for each issuer allowed for login, and MediaWiki's own `username`/`password` fields where the built-in `password` issuer is on. These are not offered to subsidiaries. `returnto` is honoured only on the request's own origin, here and on the `/auth` routes, and ignored otherwise ([0057](../decisions/0057-web-tier.md) §7). Unchanged by the OAuth server | [0012](../decisions/0012-api-requirements.md) §4; [0025](../decisions/0025-oauth-server.md) §9 |
| `action=compare` | For document pages, as in MediaWiki. For entities, a diff of the canonical JSON of the local graph's assertions: the "JSON" view of [19](19-site-ui.md); the statement view is served over REST (§5.1). `contentmodel` accepts text models only; `fromslots` and `toslots` take `main` and `mediainfo`. Seeded fork revisions appear with their ranged IDs | [0012](../decisions/0012-api-requirements.md) §4; [0041](../decisions/0041-content-models.md) §8; [0054](../decisions/0054-forking-a-mirrored-page.md) §11 |
| `action=delete` | Pages, threads and entity pages. `deletetalk=1` writes the second record of [0023](../decisions/0023-moderation.md) §2. Gains `oldimage` for one old file version. Addresses a redirect page itself. On a foreign primary: `ts-foreign-page` | [0023](../decisions/0023-moderation.md) §8; [0039](../decisions/0039-files-and-media.md) §17; [0051](../decisions/0051-page-redirects.md) §7; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `action=discussiontoolsedit`, `action=discussiontoolspageinfo` | On a followed talk title, `discussiontoolsedit` with `paction=addcomment` or `addtopic` is the same operation as `POST /thread/{id}/upstream-reply` and `POST /page/{id}/upstream-topic`, so DiscussionTools-based clients (the reply tool, Convenient Discussions) work on a followed talk page as on the wiki; `discussiontoolspageinfo` on that title returns the foreign threads' items as upstream does, with local threads omitted | [0069](../decisions/0069-synchronized-talk-pages.md) §9 |
| `action=echomarkread`, `action=echomarkseen` | Echo's modules: `echomarkread` with `list` or `all` | [0021](../decisions/0021-notifications.md) §7 |
| `action=edit` | Text models only; any other model, a `Thread:` or talk title included, is refused with MediaWiki's own `no-direct-editing` (`apierror-no-direct-editing`), so that a bot learns at once that these pages are not text. A `contentmodel` the namespace does not allow is refused as MediaWiki refuses one its handler cannot use on the title. On a `Board` title it edits the definition (`contentmodel=triplespace-board`); a table, scope, sprint or query definition is edited as any text model's, and an invalid table definition is refused as MediaWiki refuses content its handler rejects, with the failing schema path in the message (the exact code is pinned by a contract test against MediaWiki 1.43's `json` model); the pre-save transform applies. Saving a page whose text begins with a redirect line makes it a redirect; `redirect=no` is implied on the title. On a foreign primary it starts a fork: `tsfork`, `tsforkrevid`; `ts-fork-required`; the response carries `fork: {pageid, create_revid, job}`. `tsmerge={upstream revid}` carries the `merge` field, so bots that pull can say so. On a followed talk title: `ts-upstream-talk`, which names the routes | [0012](../decisions/0012-api-requirements.md) §4; [0019](../decisions/0019-discussions.md) §9; [0041](../decisions/0041-content-models.md) §8; [0045](../decisions/0045-table-content-model.md) §9; [0049](../decisions/0049-boards.md) §11; [0051](../decisions/0051-page-redirects.md) §7; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6; [0054](../decisions/0054-forking-a-mirrored-page.md) §11; [0060](../decisions/0060-scopes.md) §8; [0061](../decisions/0061-sprints-and-tasks.md) §10; [0063](../decisions/0063-query-namespace.md) §7; [0068](../decisions/0068-merging-with-upstream.md) §6; [0069](../decisions/0069-synchronized-talk-pages.md) §9 |
| `action=entityschema` | `text=`: the schema body | [0064](../decisions/0064-entityschema-and-validation.md) §7 |
| `action=expandtemplates` | MediaWiki's parameters: `text`, `title`, `revid`, `prop=wikitext\|categories\|properties\|volatile\|ttl\|modules\|jsconfigvars\|encodedjsconfigvars\|parsetree`, `includecomments`, `showstrategykeys`. With expansion off: `ts-expansion-off`. Runs Lua when `wikitext.lua` is on | [0042](../decisions/0042-template-expansion-and-parsoid.md) §14; [0043](../decisions/0043-lua-modules.md) §16 |
| `action=filerevert` | `filename`, `archivename`, `comment`: writes a `revert` upload. Needs `reupload` | [0039](../decisions/0039-files-and-media.md) §17 |
| `action=globalblock` | At the farm base, with CentralAuth's meaning | [0028](../decisions/0028-tenancy-policy.md) §11 |
| `action=linkaccount`, `action=unlinkaccount`, `action=removeauthenticationdata` | Manage **bindings**, the private sign-in methods of [0007](../decisions/0007-actor-identity.md) §3, because that is what these modules mean in MediaWiki. They never create or remove the public account links of [0007](../decisions/0007-actor-identity.md) §7. Removing the last binding is refused | [0012](../decisions/0012-api-requirements.md) §4 |
| `action=login` | The bot-password form only, for subsidiaries' API keys. A primary account's name is refused with `ts-use-oauth`; a wrong label or secret is `Failed`. Unchanged by the OAuth server | [0012](../decisions/0012-api-requirements.md) §4; [0024](../decisions/0024-subsidiary-accounts.md) §8; [0025](../decisions/0025-oauth-server.md) §9 |
| `action=move` | `noredirect`, `movesubpages`, `movetalk` (accepted, no effect) and the move-over-redirect rule of [0051](../decisions/0051-page-redirects.md) §3; the response reports `redirectcreated`. File pages move with `movefile`; the bytes do not move, since nothing is keyed by title. On a foreign primary: `ts-foreign-page` | [0051](../decisions/0051-page-redirects.md) §7; [0039](../decisions/0039-files-and-media.md) §17; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `action=oauthconsumer` | `do=propose\|update\|approve\|reject\|disable` manages consumers, under [0025](../decisions/0025-oauth-server.md) §10's permissions | [0025](../decisions/0025-oauth-server.md) §9 |
| `action=options` | `change`, `optionname`, `optionvalue` and `reset`, over the registered keys, refusing unknown ones. New preference keys `ts-upstream-fold` and `ts-rc-syncs` | [0012](../decisions/0012-api-requirements.md) §4; [0027](../decisions/0027-preferences-and-portability.md) §6 |
| `action=paraminfo` | Every `contentmodel` enumeration lists the implemented models, as MediaWiki lists its registered handlers ([mediawiki-compat.md](../api/mediawiki-compat.md) treats these values as site-specific) | [0041](../decisions/0041-content-models.md) §8 |
| `action=parse` | Accepts the `markdown` and `yaml` content models; `contentmodel` accepts text models only. Reports the renderer version, so that the in-browser preview can detect a mismatch. Renders with expansion; gains `prop=templates`, `limitreportdata`, `limitreporthtml` and `parsetree`; `pst`, `onlypst` and `preview` as in MediaWiki; `parsoid=1` returns Parsoid's HTML where the tenant has it configured; runs Lua when `wikitext.lua` is on. `redirects` parses the target. On a foreign title: the rewritten HTML, `categories`, `templates`, `images`, `langlinks`, `properties` and `revid` from the bundle, `prop=wikitext` the source. On a table: the grid's first page as static HTML, without editing controls | [0012](../decisions/0012-api-requirements.md) §4; [0041](../decisions/0041-content-models.md) §8; [0042](../decisions/0042-template-expansion-and-parsoid.md) §14; [0043](../decisions/0043-lua-modules.md) §16; [0045](../decisions/0045-table-content-model.md) §9; [0051](../decisions/0051-page-redirects.md) §7; [0053](../decisions/0053-mirrored-pages.md) §10 |
| `action=patrol` | `revid` or `rcid`; writes the patrol event, or the unpatrol event with `unpatrol=1` | [0023](../decisions/0023-moderation.md) §8 |
| `action=protect` | `protections=edit=autoconfirmed\|move=sysop`, `expiry`, `reason`; writes or retires the ACL. `read={group}` in `protections`, with `expiry` as for the others, writes a confidential restriction; MediaWiki clients that do not know it never send it. `cascade=1` is refused with `ts-no-cascade`, since there is no cascading protection. On a title that does not exist, reserves a page ID ([09](09-security-and-moderation.md) §4.8). On an entity page, restricts `edit` on the entity target (an `entity` ACL). On a foreign primary, refused with `ts-foreign-page` except create-protection | [0016](../decisions/0016-permissions-and-access-control.md) §7; [0023](../decisions/0023-moderation.md) §8; [0056](../decisions/0056-security-model.md) §13; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `action=purge` | A plain purge bumps the render epoch and purges the page's tag; `forcelinkupdate` and `forcerecursivelinkupdate` also queue refreshes. By `POST` without a token, for any principal who may read the page, in the `parse` rate class ([0057](../decisions/0057-web-tier.md) §14). On a mirrored page, refetches the bundle ([0053](../decisions/0053-mirrored-pages.md) §6) | [0042](../decisions/0042-template-expansion-and-parsoid.md) §14; [0053](../decisions/0053-mirrored-pages.md) §10 |
| `action=query&export` | Export | [0047](../decisions/0047-special-pages.md) §11 |
| `action=revisiondelete` | `type=revision\|logging`, `ids`, `hide=content\|comment\|user`, `show=…`, `suppress=yes\|no\|nochange`, `reason`; writes or amends `record` ACLs. `type=archive` is accepted and treated as `revision`, since deleted revisions are not moved. Gains `type=oldimage` and `type=filearchive` | [0023](../decisions/0023-moderation.md) §8; [0039](../decisions/0039-files-and-media.md) §17 |
| `action=setnotificationtimestamp` | As in MediaWiki | [0020](../decisions/0020-change-feeds.md) §5 |
| `action=tag` | Gains the tags of [0030](../decisions/0030-edit-filters.md) §5 | [0030](../decisions/0030-edit-filters.md) §9 |
| `action=templatedata` | TemplateData's module | [0012](../decisions/0012-api-requirements.md) §4 ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §5–6) |
| `action=undelete` | Pages, threads and entity pages. With `timestamps` or `revisions`, the partial form of [0023](../decisions/0023-moderation.md) §3. Gains `fileids`. Addresses a redirect page itself | [0023](../decisions/0023-moderation.md) §8; [0039](../decisions/0039-files-and-media.md) §17; [0051](../decisions/0051-page-redirects.md) §7 |
| `action=upload` | `filename`, `file`, `text`, `comment`, `ignorewarnings`, `stash`, `filekey`, chunked `offset`/`filesize`/`chunk`, `checkstatus`; `url` only where enabled. Warnings and errors as [0039](../decisions/0039-files-and-media.md) §5 | [0039](../decisions/0039-files-and-media.md) §17 |
| `action=userrights` | As in MediaWiki | [0016](../decisions/0016-permissions-and-access-control.md) §7 |
| `action=watch`, `action=unwatch` | With `expiry` | [0020](../decisions/0020-change-feeds.md) §5 |

**`action=query` modules**

| Module | What Triplespace adds or changes | ADR |
|---|---|---|
| `meta=allmessages` | The shipped MediaWiki messages ([0042](../decisions/0042-template-expansion-and-parsoid.md) §5) | [0042](../decisions/0042-template-expansion-and-parsoid.md) §14 |
| `meta=filerepoinfo` | [0039](../decisions/0039-files-and-media.md) §11's repositories | [0039](../decisions/0039-files-and-media.md) §17 |
| `meta=globaluserinfo` | At the farm base, with CentralAuth's meaning | [0028](../decisions/0028-tenancy-policy.md) §11 |
| `meta=notifications` | In Echo's shape: `notprop=list\|count\|seenTime`, `notfilter=read\|!read`, `notsections`, `notlimit` and continuation, returning Echo's model format. `notwikis` has its real meaning under `notifications.cross_tenant = home` and is otherwise ignored | [0021](../decisions/0021-notifications.md) §7; [0028](../decisions/0028-tenancy-policy.md) §11 |
| `meta=userinfo` | `uiprop=rights\|groups\|blockinfo`; `uiprop=hasmsg` reports an unread `talk` notification; `uiprop=ratelimits` reports the caller's limits by class (MediaWiki's shape); a subsidiary's `userinfo` carries `operator` and the key's `grants`; for a token-authenticated request it returns the subsidiary, with `uiprop=operator` and `uiprop=oauth` giving the consumer slug and the token's grants; `uiprop=options` returns the effective values with defaults filled in, as MediaWiki does | [0016](../decisions/0016-permissions-and-access-control.md) §7; [0020](../decisions/0020-change-feeds.md) §5; [0021](../decisions/0021-notifications.md) §7; [0024](../decisions/0024-subsidiary-accounts.md) §8; [0025](../decisions/0025-oauth-server.md) §9; [0027](../decisions/0027-preferences-and-portability.md) §6 |
| `prop=categories` | `clprop=sortkey\|hidden` and `clshow`; from the bundle for a foreign primary | [0038](../decisions/0038-page-metadata-and-categories.md) §13; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `prop=categoryinfo` | As in MediaWiki | [0038](../decisions/0038-page-metadata-and-categories.md) §13 |
| `prop=deletedrevisions` | The hidden records, for members of the group only; `drvslots` as `rvslots` | [0023](../decisions/0023-moderation.md) §8; [0041](../decisions/0041-content-models.md) §8 |
| `prop=description`, `inprop=displaytitle` | Page properties | [0012](../decisions/0012-api-requirements.md) §4 ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §5–6) |
| `prop=imageinfo` | `iiprop=timestamp\|user\|userid\|comment\|parsedcomment\|canonicaltitle\|url\|size\|dimensions\|sha1\|mime\|mediatype\|metadata\|commonmetadata\|extmetadata\|archivename\|bitdepth\|badfile`, `iiurlwidth`/`iiurlheight` for `thumburl`; `iiprop=sha256` is added. Hidden and erased fields are flagged as [0023](../decisions/0023-moderation.md) §5 flags them | [0039](../decisions/0039-files-and-media.md) §17 |
| `prop=images`, `prop=fileusage`, `prop=duplicatefiles` | Over `view.file`, `view.file_version` and `view.file_link`, per tenant; `prop=images` from the bundle for a foreign primary | [0039](../decisions/0039-files-and-media.md) §17; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `prop=info` | `contentmodel` for every page ID: document, entity, thread, talk and file pages. `inprop=protection` reports the page's or entity's edit and move ACLs as MediaWiki `protection` entries, and a `read` restriction as an entry of type `read` to principals that may read the page, which is every principal that can ask about it. `inprop=watched\|notificationtimestamp`. `inprop=linkclasses`, reporting `mw-redirect`. The `redirect` flag. `origin`: the repository name, `local` for a local page; `forkedfrom` beside it. For a foreign primary: `pageid`, `lastrevid`, `touched`, `length`, `contentmodel` (the upstream page's), `redirect` | [0016](../decisions/0016-permissions-and-access-control.md) §7; [0020](../decisions/0020-change-feeds.md) §5; [0023](../decisions/0023-moderation.md) §8; [0041](../decisions/0041-content-models.md) §8; [0042](../decisions/0042-template-expansion-and-parsoid.md) §14; [0051](../decisions/0051-page-redirects.md) §7; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6; [0054](../decisions/0054-forking-a-mirrored-page.md) §11; [0056](../decisions/0056-security-model.md) §13 |
| `prop=langlinks`, `prop=links` | From the bundle for a foreign primary | [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `prop=pageprops` | `wikibase_item` for pages connected through the client site, as Wikibase Client sets it, in Lua form; `ppprop`; from the bundle for a foreign primary | [0043](../decisions/0043-lua-modules.md) §16; [0012](../decisions/0012-api-requirements.md) §4; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `prop=redirects`, `generator=redirects` | The redirects to each page, from `view.redirect` | [0051](../decisions/0051-page-redirects.md) §7 |
| `prop=revisions` | Local revisions only: the local graph for entities, the `pages` partition for document pages. An erased part sets its MediaWiki hidden flag, `texthidden` for the content, `commenthidden` for the comment, `userhidden` for the attestation, and a new `erased` flag is set with it; that a record was erased is shown to everyone. New `rvprop` values: `source`, `job` and `operator`. `rvslots` takes `main` and, on File pages, `mediainfo`; `rvprop=contentmodel` is per slot; `rvprop=content` returns the serialization of [0041](../decisions/0041-content-models.md) §5. A talk page's rows are its threads' revisions and carry their content. A definition page's rows carry the definition as of each revision (a table's with `contentmodel` `triplespace-table`, `contentformat` `application/json`). For a foreign primary: the latest upstream revision, with `rvprop=content` the wikitext, `user` the upstream actor under its issuer and `comment` the upstream summary; older revisions are not held, and a request for them is answered with the latest and a warning, `ts-foreign-history`, naming the upstream history URL. Seeded fork revisions appear with ranged IDs. A deleted local entity's page answers as a deleted page | [0012](../decisions/0012-api-requirements.md) §4; [0023](../decisions/0023-moderation.md) §8; [0041](../decisions/0041-content-models.md) §8; [0045](../decisions/0045-table-content-model.md) §9; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6; [0053](../decisions/0053-mirrored-pages.md) §10; [0054](../decisions/0054-forking-a-mirrored-page.md) §11; [0060](../decisions/0060-scopes.md) §8; [0061](../decisions/0061-sprints-and-tasks.md) §10; [0063](../decisions/0063-query-namespace.md) §7 |
| `prop=stashimageinfo`, `list=mystashedfiles` | The viewer's own stash | [0039](../decisions/0039-files-and-media.md) §17 |
| `prop=templates`, `prop=transcludedin`, `list=embeddedin`, `list=alltransclusions` | From `view.transclusion`; `prop=templates` from the bundle for a foreign primary | [0042](../decisions/0042-template-expansion-and-parsoid.md) §14; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `prop=wbentityusage`, `list=wblistentityusage` | From `view.entity_usage`, with Wikibase's aspect codes and IDs in the tenant's Lua form | [0043](../decisions/0043-lua-modules.md) §16 |
| `list=abusefilters` | `abfprop=id\|description\|pattern\|actions\|hits\|comments\|lasteditor\|lastedittime\|status\|private`; `pattern`, `comments` and private filters redacted for viewers without the rights | [0030](../decisions/0030-edit-filters.md) §9 |
| `list=abuselog` | `aflprop=ids\|filter\|user\|title\|action\|details\|result\|timestamp\|revid`; redacted likewise | [0030](../decisions/0030-edit-filters.md) §9 |
| `list=allcategories` | As in MediaWiki | [0038](../decisions/0038-page-metadata-and-categories.md) §13 |
| `list=alldeletedrevisions`, `list=deletedrevs` | The hidden records, for members of the group only | [0023](../decisions/0023-moderation.md) §8 |
| `list=allimages`, `list=imageusage` | Over the file tables, per tenant | [0039](../decisions/0039-files-and-media.md) §17 |
| `list=allpages`, `generator=allpages` | `apfilterredir`: `all`, `redirects` or `nonredirects`. `apnamespace=310` lists boards. Local pages, then, with `tsorigin=all`, the title index's titles for served namespaces, each row carrying `origin`; without the parameter, local pages only, so a bot that walks a wiki gets what the wiki holds | [0051](../decisions/0051-page-redirects.md) §7; [0049](../decisions/0049-boards.md) §11; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `list=allredirects`, `generator=allredirects` | Every redirect into a namespace, with `arprop=ids\|title\|fragment\|interwiki` | [0051](../decisions/0051-page-redirects.md) §7 |
| `list=backlinks` | `blredirect` expands through redirects, as MediaWiki does. Includes tables, through the links of [0045](../decisions/0045-table-content-model.md) §8 | [0051](../decisions/0051-page-redirects.md) §7; [0045](../decisions/0045-table-content-model.md) §9 |
| `list=blocks` | As in MediaWiki | [0016](../decisions/0016-permissions-and-access-control.md) §7 |
| `list=categorymembers`, `generator=categorymembers` | `cmtype` and `cmsort` | [0038](../decisions/0038-page-metadata-and-categories.md) §13 |
| `list=filearchive` | Deleted versions, for the deletion group | [0039](../decisions/0039-files-and-media.md) §17 |
| `list=globalallusers`, `list=globalblocks` | At the farm base, with CentralAuth's meaning | [0028](../decisions/0028-tenancy-policy.md) §11 |
| `list=logevents` | Serves the events of [16](16-logs-feeds-and-notifications.md), with the new types `erase`, `link`, `retention`, `convert` and `job`, and the action `renameuser/vanish`. `leprovider` selects local events (the default) or a provider's. `letype=task`; `letype=proposal`; forks under `letype=import`, as `import/interwiki`. Redacted per viewer | [0012](../decisions/0012-api-requirements.md) §4; [0054](../decisions/0054-forking-a-mirrored-page.md) §11; [0061](../decisions/0061-sprints-and-tasks.md) §10; [0067](../decisions/0067-proposals.md) §7 |
| `list=oauthconsumers` | Lists consumers | [0025](../decisions/0025-oauth-server.md) §9 |
| `list=pageswithprop` | Page properties | [0012](../decisions/0012-api-requirements.md) §4 ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §5–6) |
| `list=prefixsearch` | With `tsorigin=all`, the title index's titles as `list=allpages` | [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `list=proposals` | Proposals | [0067](../decisions/0067-proposals.md) §7 |
| `list=protectedpages`, `list=protectedtitles`, `list=protectedsets` | `list=protectedtitles` lists reserved titles; `list=protectedpages` the rest, and gains `prtype=read`. `list=protectedsets` *(new)* lists sets with their names, groups, expiries and members, filtered as [0056](../decisions/0056-security-model.md) §5 requires | [0016](../decisions/0016-permissions-and-access-control.md) §7; [0023](../decisions/0023-moderation.md) §8; [0056](../decisions/0056-security-model.md) §13 |
| `list=querypage`, `generator=querypage` | `qppage` accepts the `mediawiki_name` of every served MediaWiki report, and the names of served Wikibase and Triplespace reports. A deferred or declined name is refused with `ts-report-unavailable`, which says which. `qpoffset` continues. A batch report returns `cached`, `cachedtimestamp` and `maxresults`. `generator=querypage` works as in MediaWiki | [0047](../decisions/0047-special-pages.md) §11 |
| `list=recentchanges` | One row per local revision, as in MediaWiki. Job changes carry the `bot` flag and a `job:{id}` tag. `rcjob` filters by job. Mirror records never appear; each mirror job run appears as a `job` log event. `rctype=external` is not used, because Wikibase clients already give it a meaning. `rcshow=patrolled\|!patrolled\|autopatrolled` and `rcprop=patrolled`. `rcscope` serves a scope's related changes ([0060](../decisions/0060-scopes.md) §7) | [0012](../decisions/0012-api-requirements.md) §4; [0023](../decisions/0023-moderation.md) §8; [0060](../decisions/0060-scopes.md) §8 |
| `list=scopemembers` | `smscope=` | [0060](../decisions/0060-scopes.md) §8 |
| `list=sprinttasks` | `stsprint=`, `ststate=` | [0061](../decisions/0061-sprints-and-tasks.md) §10 |
| `list=tags` | Gains the tags of [0030](../decisions/0030-edit-filters.md) §5 | [0030](../decisions/0030-edit-filters.md) §9 |
| `list=usercontribs` | By local account. Job changes appear with their tags. `ucjob` filters by job | [0012](../decisions/0012-api-requirements.md) §4 |
| `list=users` | `usprop=linkedaccounts` (public links, [0007](../decisions/0007-actor-identity.md) §7), `usprop=operator` (for bots), `usprop=subsidiaries` (a primary account's subsidiaries), `usprop=groups\|rights\|blockinfo`. The existing `cancreate` checks names at first login | [0012](../decisions/0012-api-requirements.md) §4; [0016](../decisions/0016-permissions-and-access-control.md) §7; [0024](../decisions/0024-subsidiary-accounts.md) §8 |
| `list=watchlist`, `list=watchlistraw` | `wlprop`, `wlshow`, `wltype`, `wlexcludeuser`, and `wlowner` with `wltoken` for the Atom token; `rcshow=patrolled\|!patrolled\|autopatrolled` and `rcprop=patrolled` as on `list=recentchanges` | [0020](../decisions/0020-change-feeds.md) §5; [0023](../decisions/0023-moderation.md) §8; [0027](../decisions/0027-preferences-and-portability.md) §6 |
| `titles=`, `pageids=`, `redirects` | `redirects` resolves the page set through redirects, one hop, and reports each in the `redirects` array with `from`, `to` and `tofragment`, as MediaWiki does. The page set resolves through the stack and through foreign redirects; a ranged `pageid` resolves to the repository page it names | [0051](../decisions/0051-page-redirects.md) §7; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |

There is no `list=threads` module; threads are read through `GET /page/{id}/threads`, whose `origin=` parameter selects local or upstream threads, and the page modules.

**Wikibase and WikibaseLexeme modules**

| Module | What Triplespace adds or changes | ADR |
|---|---|---|
| `wbcheckconstraints` | WikibaseQualityConstraints' module, in its shape: `id`, `claimid`, `constraintid`, `status`, returning per-statement results with `status` (`violation`, `warning`, `suggestion`, `compliance`, `exception`, `not-in-scope`, `todo` for an unbound type) and the constraint's clarification, so that the gadgets and tools that read it work | [0031](../decisions/0031-property-constraints.md) §6 |
| `wbeditentity` | `sitelinks` entries may carry `url` in place of `site`+`title`. `new=lexeme`, or on `L` IDs. Terms on `M` | [0026](../decisions/0026-sitelinks.md) §7; [0066](../decisions/0066-lexemes.md) §8; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4 |
| `wbgetentities` | `sitefilter` accepts alias IDs and hosts; `sites` and `titles` (lookup by sitelink) accept both forms, and find a page's paired item; a `urls` parameter looks up by URL. A `resolver` and `key` pair of parameters, mirroring `sites`+`titles`, returns the entity when the lookup is unique and `missing` with a `candidates` list otherwise. `missing` for a deleted local entity. On `WDM`, `E` and `L` IDs as §2.1. `ids=mhc:Q1`: a source entity from the tenant's view; one not yet held is `missing`, and is enqueued when the source's mirror setting is `on-demand` or above | [0026](../decisions/0026-sitelinks.md) §7; [0029](../decisions/0029-resolver-namespaces.md) §6; [0023](../decisions/0023-moderation.md) §8; [0038](../decisions/0038-page-metadata-and-categories.md) §13; [0064](../decisions/0064-entityschema-and-validation.md) §7; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4; [0066](../decisions/0066-lexemes.md) §8; [0078](../decisions/0078-entity-sources.md) §10 |
| `wbsearchentities` | `type=entityschema`; `type=lexeme\|form\|sense` | [0064](../decisions/0064-entityschema-and-validation.md) §7; [0066](../decisions/0066-lexemes.md) §8 |
| `wbsetlabel`, `wbsetdescription`, `wbsetaliases` | Accepted on `M` and on `E` IDs | [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4; [0064](../decisions/0064-entityschema-and-validation.md) §7 |
| `wbsetsitelink` | `linksite` is a site alias ID or a host; `linktitle` is a title (aliased site) or a path (host). `linkurl` takes the whole URL and needs neither. `badges` as in Wikibase. Errors: `sitelink-conflict`, `ts-sitelink-denied`, `ts-sitelink-scheme`; a title whose primary is foreign is refused as a sitelink target with `ts-sitelink-foreign` | [0026](../decisions/0026-sitelinks.md) §7; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `wbladdform`, `wblremoveform`, `wbleditformelements`, `wbladdsense`, `wblremovesense`, `wbleditsenseelements`, `wblmergelexemes` | WikibaseLexeme's modules, with its parameters and errors | [0066](../decisions/0066-lexemes.md) §8 |
| The statement modules | On lexeme and part IDs, and on `M` IDs | [0066](../decisions/0066-lexemes.md) §8; [0041](../decisions/0041-content-models.md) §8 |

The Wikibase-compatibility forms of [21](21-special-pages.md) use the existing `wb*` modules ([0047](../decisions/0047-special-pages.md) §11); `Special:ItemByTitle` and `Special:GoToLinkedPage` are served by the sitelink resolver, and `Special:ItemByTitle` accepts an alias ID and title or a URL ([0029](../decisions/0029-resolver-namespaces.md) §6, [0026](../decisions/0026-sitelinks.md) §7).

### 2.4 Declined and deferred

*Sources: [0012](../decisions/0012-api-requirements.md) §4; [0019](../decisions/0019-discussions.md) §9; [0039](../decisions/0039-files-and-media.md) §17; [0066](../decisions/0066-lexemes.md) §8.*

**Declined.** `action=imagerotate` is not offered; rotation is a new upload. `rctype=external` is not used. `action=login` refuses a primary account's name with `ts-use-oauth`. **Not yet.** DiscussionTools' `action=discussiontoolsedit` (`paction=addtopic|addcomment`) on local threads can be supported later on the same records, with `commentid` values of the form `c-{revid}`; on followed talk titles it is served already (§2.3). The Wikibase REST API has no lexeme routes yet, so none are mirrored.

## 3. REST under `rest.php/triplespace/v0`

*Sources: [0012](../decisions/0012-api-requirements.md) §5; [0041](../decisions/0041-content-models.md) §9.*

### 3.1 Conventions the routes share

*Sources: [0012](../decisions/0012-api-requirements.md) §5; [0020](../decisions/0020-change-feeds.md) §5; [0023](../decisions/0023-moderation.md) §8; [0024](../decisions/0024-subsidiary-accounts.md) §8; [0030](../decisions/0030-edit-filters.md) §9; [0039](../decisions/0039-files-and-media.md) §17; [0041](../decisions/0041-content-models.md) §9; [0051](../decisions/0051-page-redirects.md) §7; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6; [0056](../decisions/0056-security-model.md) §13; [0031](../decisions/0031-property-constraints.md) §6.*

**Every route is served per tenant base,** which the host selects; instance-level routes are served at the farm base and, on a single-tenant instance, at the tenant's as well (§1.2). The OpenAPI document of §1.1 is the contract.

**Page summaries.** A page summary (`GET /page/{id}` and the routes that embed one) carries `content_model` for every page and, for File pages, `mediainfo_id`; `redirect_to` for a redirect; and `origin`, `upstream` (page ID, revision, URL) and `stack` (every origin under the title, in order, with `shown: true` or `false`), with a provider-ranged ID accepted for a foreign page.

**Activity rows.** The activity row ([16](16-logs-feeds-and-notifications.md)) gains `patrolled`, and its `visibility` field already reports hidden parts; it gains `filters`, the filters a record matched with their actions, for rows the viewer may see. The provenance response gains a `constraints` list per statement, so the statement UI needs one request, not two.

**Visibility.** `GET /acl/{kind}/{id}` on any target also returns `visibility`, the resolved set, to a principal that may read the target. Every response is redacted for its viewer; a `read` ACL is one more input to that redaction, and nothing behind a `read` ACL is returned to a viewer outside its group, from any route, cache or index ([09](09-security-and-moderation.md) §5).

**Caching.** Watchlist responses carry `Cache-Control: private` and never enter the shared caches; the everything feed's page and Atom forms are public and cache briefly at L2. `/account/subsidiaries` is `no-store` like everything under `/account`; no route returns a key's hash, and no route but the issuing `POST` returns a secret. No URL, thumbnail URL, hash or metadata of a file version the viewer may not read is returned by any route, and no media route serves its bytes ([03](03-storage-caches-and-search.md) §10). When each binding was last used is stored in the operational store, never in the log.

### 3.2 The route catalogue

*Sources: [0012](../decisions/0012-api-requirements.md) §5; [0016](../decisions/0016-permissions-and-access-control.md) §7; [0019](../decisions/0019-discussions.md) §9; [0020](../decisions/0020-change-feeds.md) §5; [0021](../decisions/0021-notifications.md) §7; [0022](../decisions/0022-federation.md) §11; [0023](../decisions/0023-moderation.md) §8; [0024](../decisions/0024-subsidiary-accounts.md) §8; [0025](../decisions/0025-oauth-server.md) §9; [0026](../decisions/0026-sitelinks.md) §7; [0027](../decisions/0027-preferences-and-portability.md) §6; [0028](../decisions/0028-tenancy-policy.md) §11; [0029](../decisions/0029-resolver-namespaces.md) §6; [0030](../decisions/0030-edit-filters.md) §9; [0031](../decisions/0031-property-constraints.md) §6; [0038](../decisions/0038-page-metadata-and-categories.md) §13; [0039](../decisions/0039-files-and-media.md) §17; [0042](../decisions/0042-template-expansion-and-parsoid.md) §12, §14; [0045](../decisions/0045-table-content-model.md) §10; [0047](../decisions/0047-special-pages.md) §11; [0049](../decisions/0049-boards.md) §11; [0051](../decisions/0051-page-redirects.md) §7; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6; [0053](../decisions/0053-mirrored-pages.md) §10; [0054](../decisions/0054-forking-a-mirrored-page.md) §11; [0056](../decisions/0056-security-model.md) §13; [0059](../decisions/0059-query-service.md) §6; [0060](../decisions/0060-scopes.md) §8; [0061](../decisions/0061-sprints-and-tasks.md) §10; [0062](../decisions/0062-workspaces.md) §8; [0063](../decisions/0063-query-namespace.md) §7; [0064](../decisions/0064-entityschema-and-validation.md) §7; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4; [0066](../decisions/0066-lexemes.md) §8; [0067](../decisions/0067-proposals.md) §7; [0068](../decisions/0068-merging-with-upstream.md) §6; [0069](../decisions/0069-synchronized-talk-pages.md) §9; [0070](../decisions/0070-shallow-entity-mirroring.md) §9; [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §13; [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §8; [0076](../decisions/0076-dataset-publication.md) §6; [0077](../decisions/0077-special-version.md) §10; [0078](../decisions/0078-entity-sources.md) §10; [0079](../decisions/0079-derived-issuer-codes.md) §7; [0080](../decisions/0080-tenants-as-entity-sources.md) §8.*

Every route under `rest.php/triplespace/v0`, sorted by path. Each row keeps the ADR's own spelling of the path parameter (`{id}`, `{pageid}`, `{page ID}` name the same page ID; `{thread}` a thread's page ID). "Holder only" means the account holder in their own session; "farm base" means served at the farm base only. Routes the ADRs place at fixed paths outside `rest.php` are listed where [0012](../decisions/0012-api-requirements.md) §5 lists them and marked.

| Route | Purpose | ADR |
|---|---|---|
| `/.well-known/oauth-authorization-server` | OAuth 2.0 server metadata ([07](07-actors-and-accounts.md) §7.2); served at both the `/oauth` path and MediaWiki's | [0012](../decisions/0012-api-requirements.md) §5; [0025](../decisions/0025-oauth-server.md) §9 |
| `/.well-known/tlog/{partition}/checkpoint` | What an instance publishes for verified sync, with `/dumps/local/` and the record and proof routes; the full publication table is [17](17-federation-and-publication.md) | [0012](../decisions/0012-api-requirements.md) §5; [0022](../decisions/0022-federation.md) §11 |
| `GET /account` | Holder only. Name, bindings (private), links and preferences | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /account/authorizations`, `DELETE /account/authorizations/{subsidiary}/{slug}` | For the account page | [0025](../decisions/0025-oauth-server.md) §9 |
| `GET /account/export` | Holder only. The user data bundle ([07](07-actors-and-accounts.md) §8.4), or `202` with a job to poll when it is being produced; `viewmyprivateinfo` | [0027](../decisions/0027-preferences-and-portability.md) §6 |
| `POST /account/import` | Holder only. Import a bundle; the response lists what was applied, what needs verification, and what did not resolve | [0027](../decisions/0027-preferences-and-portability.md) §6 |
| `POST /account/links` | Starts the linking flow. The issuer requires the user to log in again ([0007](../decisions/0007-actor-identity.md) §7) | [0012](../decisions/0012-api-requirements.md) §5 |
| `DELETE /account/links/{actor}` | Unlinks, by erasing the link | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /account/pending-subsidiaries` | For `Special:PendingSubsidiaries` | [0025](../decisions/0025-oauth-server.md) §9 |
| `GET /account/preferences`, `PATCH /account/preferences`, `DELETE /account/preferences/{key}` | Holder only. Read and change preferences; `DELETE` resets one; `editmyoptions` to change | [0027](../decisions/0027-preferences-and-portability.md) §6 |
| `POST /account/rename`, `POST /account/vanish` | [0007](../decisions/0007-actor-identity.md) §4, [0008](../decisions/0008-namespaces-and-document-pages.md) §6 ([07](07-actors-and-accounts.md) §3.2) | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /account/subsidiaries`, `POST /account/subsidiaries` | Holder only. List and create | [0024](../decisions/0024-subsidiary-accounts.md) §8 |
| `POST /account/subsidiaries/{name}/rename`, `/retire`, `/reactivate` | The operator's ownership actions | [0024](../decisions/0024-subsidiary-accounts.md) §8 |
| `GET /account/subsidiaries/{name}/keys`, `POST …/keys`, `DELETE …/keys/{id}` | Keys: labels, grants, IP ranges, expiry, last used; issue (the secret is in this response and never again); revoke | [0024](../decisions/0024-subsidiary-accounts.md) §8 |
| `POST /acl/set`, `GET`, `PUT`, `DELETE /acl/set/{id}`, `POST /acl/set/{id}/members`, `DELETE /acl/set/{id}/members/{kind}/{id}` | Create a set; the set's ACL; add and remove members | [0056](../decisions/0056-security-model.md) §13 |
| `GET`, `PUT`, `DELETE /acl/tenant/{slug}` | The tenant's ACL | [0056](../decisions/0056-security-model.md) §13 |
| `GET`, `PUT`, `DELETE /acl/{kind}/{id}` | ACLs for graph and entity targets, which `action=protect` cannot express fully, and `record` and `actor` kinds with a `parts` list; `DELETE` retires the ACL; `GET` also returns `visibility` | [0016](../decisions/0016-permissions-and-access-control.md) §7; [0023](../decisions/0023-moderation.md) §8; [0056](../decisions/0056-security-model.md) §13 |
| `GET /activity` | Recent changes as the UI shows them: one row per job, mirror syncs on request, and grouping by page. Filters: `kind`, namespace and period, applied to every set; `target={kind}:{id}`, `related={kind}:{id}&direction=out\|in`, `set=watchlist`, `set=farm` at the farm base, and `syncs=on\|off` | [0012](../decisions/0012-api-requirements.md) §5; [0020](../decisions/0020-change-feeds.md) §5; [0028](../decisions/0028-tenancy-policy.md) §11 |
| `GET /activity/stream` | The same parameters, as server-sent events | [0020](../decisions/0020-change-feeds.md) §5 |
| `POST /activity/{partition}/{offset}/patrol` | Marks a row patrolled | [0023](../decisions/0023-moderation.md) §8 |
| `GET /actor/{key}` | The public profile: current name, kind, status, linked accounts, and `operator` for a subsidiary or `subsidiaries` for a primary account | [0012](../decisions/0012-api-requirements.md) §5; [0024](../decisions/0024-subsidiary-accounts.md) §8 |
| `GET /actor/{key}/contributions` | Contributions by account, including jobs run on the account's behalf | [0012](../decisions/0012-api-requirements.md) §5 |
| `POST /actor/{key}/transfer` | Bureaucrat: transfer with the new operator's acceptance | [0024](../decisions/0024-subsidiary-accounts.md) §8 |
| `GET /auth/{issuer}/start`, `GET /auth/{issuer}/callback` | The login redirect flow; `returnto` honoured only on the request's own origin | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /auth/pending` | At first login: the pending identity's username, so the page can offer it as a suggestion. Only that session can read it | [0012](../decisions/0012-api-requirements.md) §5 |
| `POST /auth/pending/create` | Creates the account with the chosen name. It may also create a link, using the login that just took place as proof | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /blob/{sha256}/references` | For the deletion group, within the tenant | [0039](../decisions/0039-files-and-media.md) §17 |
| `GET /category-mappings`, `GET`, `PUT`, `DELETE /category-mappings/{name}` | Mappings, as configuration records | [0038](../decisions/0038-page-metadata-and-categories.md) §13 |
| `POST /category-mappings/{name}/make-real` | Starts the make-it-real job | [0038](../decisions/0038-page-metadata-and-categories.md) §13 |
| `GET /category/{title}/members?type=&from=` | Members by sort key | [0038](../decisions/0038-page-metadata-and-categories.md) §13 |
| `GET /constraints/counts` | `view.constraint_count` | [0031](../decisions/0031-property-constraints.md) §6 |
| `GET /constraints/report?property=&type=&severity=&source=` | The constraint report, paged with §1.3's continuation | [0031](../decisions/0031-property-constraints.md) §6 |
| `POST /constraints/recheck` | Requests a re-check of a property's statements as a job; needs `ts-runjob` | [0031](../decisions/0031-property-constraints.md) §6 |
| `GET /corrections?provider=&state=` | Local corrections by state, for reporting upstream ([05](05-providers-and-ingest.md)) | [0012](../decisions/0012-api-requirements.md) §5 |
| `/dumps/local/` | The local-graph source dump ([17](17-federation-and-publication.md)) | [0012](../decisions/0012-api-requirements.md) §5; [0022](../decisions/0022-federation.md) §11 |
| `/dumps/updates/` | The dumps of the SPARQL Update stream ([02](02-graphs-rdf-and-query.md) §6) | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /entity-sources` | The tenant's sources: name, label, namespace, types with IRI templates (absent for an unpublished source), `promoted_to`, and for a tenant source its `tenant` and `base`, with its types as read from the provider. Public, as `GET /resolvers` is | [0078](../decisions/0078-entity-sources.md) §10; [0080](../decisions/0080-tenants-as-entity-sources.md) §8 |
| `GET /entity/E1` | An entity schema | [0064](../decisions/0064-entityschema-and-validation.md) §7 |
| `GET /entity/L1`, `GET /entity/L1-F1` | A lexeme with its parts; a part alone | [0066](../decisions/0066-lexemes.md) §8 |
| `GET /entity/{id}/changes/{partition}/{offset}` | The structured diff of one change set (§5.1) | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /entity/{id}/compare?source=&from=&to=` | Two states of one source | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /entity/{id}/constraints` | The constraint results as JSON, with severity and source | [0031](../decisions/0031-property-constraints.md) §6 |
| `POST /entity/{id}/convert`, `POST /entity/{id}/retention` | Single-entity forms of `convert` and `retain` ([05](05-providers-and-ingest.md)) | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /entity/{id}/derivations` | The derivations that name an entity, with the mentions that produced each statement | [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §13 |
| `GET /entity/{id}/history` | Activity rows across the whole cluster, including log events keyed to its members. Filters: `source`, `kind`, and a time range | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /entity/{id}/proposals` | The entity's proposals | [0067](../decisions/0067-proposals.md) §7 |
| `GET /entity/{id}/provenance` | The provenance response of [0003](../decisions/0003-statement-ui.md) §6, plus the data for the identity line: who minted the ID, cluster members and the canonical ID, statement counts by graph, sync state (last sync, upstream version, `first_seen`), retention and history policies, and the backlink count. Each statement carries its constraint results. For an adopted entity, the minter is the source wiki, with the adoption job and `source_revid` | [0012](../decisions/0012-api-requirements.md) §5; [0031](../decisions/0031-property-constraints.md) §6 |
| `GET /entity/{id}/referrers` | Entities referring to this one, from `view.entity_ref` ([17](17-federation-and-publication.md)) | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /entity/{id}/schemas` | The schemas bound to the entity through its scopes, with its results | [0064](../decisions/0064-entityschema-and-validation.md) §7 |
| `GET`, `PUT`, `DELETE /entity/{id}/sitelinks/{host}` | One sitelink by host | [0026](../decisions/0026-sitelinks.md) §7 |
| `POST /entity/{id}/undo/{partition}/{offset}` | Appends the inverse change set. The base is required | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /entity/{id}/upstream?from=&to=` | Upstream edits fetched live ([05](05-providers-and-ingest.md)) | [0012](../decisions/0012-api-requirements.md) §5 |
| `POST /export`, `GET /export/{id}` | Streams an export, or starts an export job and returns its ID; an export job's state and, when ready, its download | [0047](../decisions/0047-special-pages.md) §11 |
| `POST /expunge` | Farm base, operator rights. Takes down and expunges in one request | [0039](../decisions/0039-files-and-media.md) §17 |
| `GET /extraction/{source}` | The source's configuration, page count, queue length, last run, conflicts and orphan count | [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §13 |
| `GET /extraction/{source}/orphans` | Orphans ([13](13-mirrored-pages.md)) | [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §13 |
| `POST /extraction/{source}/run` | Enqueues every page of the source, or the pages named; the `ts-runjob` right | [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §13 |
| `GET /extraction/{source}/unmapped` | The strings a value map did not hold ([0072](../decisions/0072-template-mappings.md) §4) | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /farm/users/{id}` | Farm base. A farm account and its linked tenant accounts, public | [0028](../decisions/0028-tenancy-policy.md) §11 |
| `GET /file/{title}` | The current version, in MediaWiki REST's `v1/file` shape | [0039](../decisions/0039-files-and-media.md) §17 |
| `GET /file/{title}/mediainfo` | The resolved view (local or overlaid foreign) with `foreign` | [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4 |
| `GET /file/{title}/usage` | Pages and entities using the file | [0039](../decisions/0039-files-and-media.md) §17 |
| `GET /file/{title}/versions`, `POST /file/{title}/versions/{revid}/erase` | The versions; erase one | [0039](../decisions/0039-files-and-media.md) §17 |
| `GET`, `POST`, `PUT /filters[/{id}]` | Edit filters | [0030](../decisions/0030-edit-filters.md) §9 |
| `GET /filters/log` | Hits, with the feed parameters of `GET /activity` | [0030](../decisions/0030-edit-filters.md) §9 |
| `POST /filters/test` | Returns a job | [0030](../decisions/0030-edit-filters.md) §9 |
| `GET /filters/{id}/history`, `GET /filters/{id}/hits` | A filter's history and hits | [0030](../decisions/0030-edit-filters.md) §9 |
| `GET /inbox` | The inbox as an `as:OrderedCollection` of activities, each with `scatter:reason` and read state, paginated with §1.3's continuation | [0021](../decisions/0021-notifications.md) §7 |
| `POST /inbox/read`, `POST /inbox/seen` | Mark items read; set the seen time | [0021](../decisions/0021-notifications.md) §7 |
| `GET /instance/check` | Farm base, for `owner`: the `instance check` table ([08](08-tenants-and-instances.md) §8) with each line's state, and the attestations with who gave them and when | [0056](../decisions/0056-security-model.md) §13 |
| `GET /jobs` | Filters: status, graph and actor. Mirror syncs are instance jobs, listed at the farm base; bulk jobs are the tenant's | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /jobs/{id}` | The job record and status: counts by outcome, the hash-mismatch count by value type ([0006](../decisions/0006-log-integrity-and-erasure.md) A9), source and adapter versions, and checkpoint. While running: progress and projection lag. For `snapshot` jobs: the sweep count and its threshold | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /jobs/{id}/changes?outcome=` | The records the job wrote, paged | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /jobs/{id}/events` | Server-sent events for progress | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /jobs/{id}/rejects` | Downloads the rejects file. Requires a right ([09](09-security-and-moderation.md)) | [0012](../decisions/0012-api-requirements.md) §5 |
| `POST /jobs/{id}/rerun` | Runs the job again with its stored parameters | [0012](../decisions/0012-api-requirements.md) §5 |
| `POST /jobs/{id}/revert` | Starts the inverse job. The response lists the entities edited since the job, whose later edits are kept. Reverting a Nuke is this route | [0012](../decisions/0012-api-requirements.md) §5; [0047](../decisions/0047-special-pages.md) §11 |
| `GET /jsonld/by-url` | The profiled JSON-LD of the entities a URL identifies ([17](17-federation-and-publication.md)) | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /kits` | The kits visible to the tenant, with their schema-validated summaries | [0062](../decisions/0062-workspaces.md) §8 |
| `PUT /notifications/email`, `DELETE …` | Register (sends the verification link) and remove | [0021](../decisions/0021-notifications.md) §7 |
| `PUT /notifications/fediverse`, `DELETE …` | Register (resolves the handle, sends the confirmation message) and remove | [0021](../decisions/0021-notifications.md) §7 |
| `GET /notifications/preferences`, `PUT …` | The notification matrix | [0021](../decisions/0021-notifications.md) §7 |
| `GET /notifications/verify/{token}` | The link target for both verifications | [0021](../decisions/0021-notifications.md) §7 |
| `GET /nuke/preview?target=&tag=&subsidiaries=&from=&until=&pattern=&namespace=` | The Nuke candidates, grouped | [0047](../decisions/0047-special-pages.md) §11 |
| `POST /nuke` | Starts the job; returns its ID | [0047](../decisions/0047-special-pages.md) §11 |
| `/oauth/authorize`, `/oauth/token`, `/oauth/device`, `/oauth/revoke`, `/oauth/identify` | The instance's own OAuth 2.0 server, whose tokens are credentials of a subsidiary, never of a primary account; at both the `/oauth` path and MediaWiki's ([07](07-actors-and-accounts.md) §7.2) | [0012](../decisions/0012-api-requirements.md) §5; [0025](../decisions/0025-oauth-server.md) §9 |
| `GET`, `POST /oauth/consumers`, `GET`, `PATCH /oauth/consumers/{slug}`, `POST /oauth/consumers/{slug}/status` | Consumers | [0025](../decisions/0025-oauth-server.md) §9 |
| `POST /oauth/requests`, `GET /oauth/requests/{handle}`, `POST /oauth/requests/{handle}/approve`, `/deny` | The validated authorization request the consent page reads and decides (`POST /oauth/requests` from a device code), readable only in the session that began it ([0057](../decisions/0057-web-tier.md) §13) | [0025](../decisions/0025-oauth-server.md) §9 |
| `POST /page/fork` | `{title, repo, revid, text?, summary?, dependencies?, talk?}`: a fork with or without an edit; returns the page, revisions and job | [0054](../decisions/0054-forking-a-mirrored-page.md) §11 |
| `GET /page/stack/{title}` | The stack of a title: each entry's origin, page ID, latest revision and whether it is offered | [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `GET /page/{id}` | A page summary (§3.1) | [0041](../decisions/0041-content-models.md) §9; [0051](../decisions/0051-page-redirects.md) §7; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 |
| `GET /page/{pageid}/categories` | Categories with sort keys and the hidden flag | [0038](../decisions/0038-page-metadata-and-categories.md) §13 |
| `GET /page/{id}/derivations` | The derivations of a page, with subjects and evidence | [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §13 |
| `GET /page/{id}/fork` | The `view.fork` row: origin, state, counts, dependencies, files | [0054](../decisions/0054-forking-a-mirrored-page.md) §11 |
| `POST /page/{id}/fork/files` | Copies the fork's files ([13](13-mirrored-pages.md)) | [0054](../decisions/0054-forking-a-mirrored-page.md) §11 |
| `GET /page/{id}/history` | A page's history as activity rows; composite for a talk page ID: the rows of every thread attached to it, home or listing, plus the attach, detach and move events, in one sequence ordered by time ([0019](../decisions/0019-discussions.md) §7, [14](14-discussions.md)) | [0012](../decisions/0012-api-requirements.md) §5; [0019](../decisions/0019-discussions.md) §9 |
| `GET /page/{id}/html` | The page's HTML; `?revision={revid}&as_of=revision` renders as of the revision's offset (§5.2). For an inherited page, the rewritten HTML, and `as_of` is refused with `ts-foreign-history` | [0042](../decisions/0042-template-expansion-and-parsoid.md) §12, §14; [0053](../decisions/0053-mirrored-pages.md) §10 |
| `GET /page/{id}/merge-preview?direction=pull\|push` | The merge with hunks, never stored | [0068](../decisions/0068-merging-with-upstream.md) §6 |
| `POST /page/{id}/merge` | A pull: body is the resolved text, writes the `edit` with `merge` | [0068](../decisions/0068-merging-with-upstream.md) §6 |
| `GET /page/{id}/proposals` | The page's proposals | [0067](../decisions/0067-proposals.md) §7 |
| `GET /page/{pageid}/provenance` | Import origin, and the revision count by origin: imported, bot or local; the provenance of the page's statements, including `derived` entries; for an adopted page, the adoption job; the fork's origin and job | [0012](../decisions/0012-api-requirements.md) §5; [0038](../decisions/0038-page-metadata-and-categories.md) §13; [0054](../decisions/0054-forking-a-mirrored-page.md) §11 |
| `POST /page/{id}/refollow` | Re-follow upstream | [0068](../decisions/0068-merging-with-upstream.md) §6 |
| `GET /page/{id}/render` | The render manifest summary: templates and modules used, entities and aspects, foreign repositories, `expires_at`, renderer, limit report and errors. For a foreign page: the repository, upstream revision, `retrieved`, `expires_at` and the templates and images the bundle records | [0042](../decisions/0042-template-expansion-and-parsoid.md) §14; [0053](../decisions/0053-mirrored-pages.md) §10 |
| `GET`, `POST /page/{pageid}/statements` | The resolved statements; add an asserted statement. The statement routes follow the Wikibase REST API's statement routes, with the page ID in place of an entity ID; unchanged for File pages | [0038](../decisions/0038-page-metadata-and-categories.md) §13; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4 |
| `GET`, `PUT`, `PATCH`, `DELETE /page/{pageid}/statements/{statement_id}` | One statement. Writes to a projected statement are refused with `ts-derived-statement` | [0038](../decisions/0038-page-metadata-and-categories.md) §13 |
| `POST /page/{fork ID}/talk-follow` | `{talk: follow\|fork, base}` | [0069](../decisions/0069-synchronized-talk-pages.md) §9 |
| `GET /page/{talk page or board ID}/threads` | Every attached thread with status, `home` (`true` or `false`) and last activity, without their posts; foreign threads, each with `origin` `{repo, n, state, anchor}` and no title; filter `origin=local\|upstream` | [0019](../decisions/0019-discussions.md) §9; [0049](../decisions/0049-boards.md) §11; [0069](../decisions/0069-synchronized-talk-pages.md) §9 |
| `GET /page/{id}/upstream-diff` | `against=base` or `current` | [0054](../decisions/0054-forking-a-mirrored-page.md) §11 |
| `POST /page/{talk page ID}/upstream-topic` | `{subject, wikitext}`; 202 with an upstream-post ID | [0069](../decisions/0069-synchronized-talk-pages.md) §9 |
| `GET /preferences/schema` | The registered keys, types and defaults (public) | [0027](../decisions/0027-preferences-and-portability.md) §6 |
| `POST /proposal` | Subject, selection → compiles and opens the thread | [0067](../decisions/0067-proposals.md) §7 |
| `GET /proposal/{thread}` | State, payload, omitted, flags | [0067](../decisions/0067-proposals.md) §7 |
| `POST /proposal/{thread}/export?format=qs\|json\|wikitext\|diff`, `POST /proposal/{thread}/push`, `POST /proposal/{thread}/withdraw` | Export, push and withdraw | [0067](../decisions/0067-proposals.md) §7 |
| `GET /provider/{code}/mirror` | Per type, the setting, the set size, the queue length and the stream lag; a source's name in place of `{code}`, since lowercase source names and uppercase provider codes never clash | [0070](../decisions/0070-shallow-entity-mirroring.md) §9; [0078](../decisions/0078-entity-sources.md) §10 |
| `POST /provider/{code}/mirror/fetch` | With a list of IDs, enqueues them (the `ts-runjob` right) | [0070](../decisions/0070-shallow-entity-mirroring.md) §9; [0078](../decisions/0078-entity-sources.md) §10 |
| `GET /providers/{slug}/verification` | Last checkpoint, key chain, failures | [0012](../decisions/0012-api-requirements.md) §5; [0022](../decisions/0022-federation.md) §11 |
| `GET /publication/{name}` | Configuration, data pages with their last revision and state, conflicts, last run | [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §8 |
| `POST /publication/{name}/run` | Regenerate now (the `ts-runjob` right) | [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §8 |
| `GET /query/{pageid}/params` | The parameter schema, for forms and for the block editor | [0063](../decisions/0063-query-namespace.md) §7 |
| `GET /query/{pageid}/run?{param}=…&format=` | Runs with parameters; `Query-Cursor` and `Query-Truncated` headers as `/sparql`; formats as `/sparql` plus the labelled table as JSON. Rate class `query`; concurrency under `query.max_concurrent`; `read` to run | [0063](../decisions/0063-query-namespace.md) §7 |
| `GET /record/{partition}/{offset}/checkpoint` | The earliest checkpoint that includes the record | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /record/{partition}/{offset}/proof?checkpoint=` | An inclusion proof ([01](01-log-and-records.md) §8). Both record routes return the record's signature, where it has one | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /repo/{name}` | The repository's status: mode, set, index date, event lag, cache and mirror sizes | [0053](../decisions/0053-mirrored-pages.md) §10 |
| `GET /reports` | Every report with its backing, mode, graph scope, lag or `computed_at` | [0047](../decisions/0047-special-pages.md) §11 |
| `GET /reports/{name}?graphs=&limit=&continue=` | A report's rows, with its own filters (`language`, `type`, `datatype`, `state` and so on) | [0047](../decisions/0047-special-pages.md) §11 |
| `GET /resolve?q=` | Resolves any string to its canonical page through the title resolver (§3.3) | [0012](../decisions/0012-api-requirements.md) §5; [0029](../decisions/0029-resolver-namespaces.md) §6; [0051](../decisions/0051-page-redirects.md) §7; [0078](../decisions/0078-entity-sources.md) §10 |
| `GET /resolvers` | The tenant's resolvers with their grammars and bindings (public) | [0029](../decisions/0029-resolver-namespaces.md) §6 |
| `GET /resolvers/{name}/{key}` | The lookup as JSON, with `303` semantics available by `Accept` | [0029](../decisions/0029-resolver-namespaces.md) §6 |
| `POST /schema/{id}/check` | With a subject (ad hoc; rate class `query`) | [0064](../decisions/0064-entityschema-and-validation.md) §7 |
| `GET /schema/{id}/report?conforms=&after=` | The schema's report | [0064](../decisions/0064-entityschema-and-validation.md) §7 |
| `POST /scope/preview` | Body: a `members` object. Returns count, truncated, a sample of 20 with labels, and for by-example the shared statements with counts. Compiles and runs without saving; rate class `query`; `query.timeout` applies | [0062](../decisions/0062-workspaces.md) §8 |
| `GET /scope/{pageid}` | Count, truncated, cursor, `computed_at` | [0060](../decisions/0060-scopes.md) §8 |
| `GET /scope/{pageid}/export` | TSV of IDs | [0060](../decisions/0060-scopes.md) §8 |
| `GET /scope/{pageid}/facets?properties=` | Value distributions for the builder, bounded by `scopes.facet_limit` | [0062](../decisions/0062-workspaces.md) §8 |
| `GET /scope/{pageid}/members?after=&limit=` | Paged, kind and ID, labels on request | [0060](../decisions/0060-scopes.md) §8 |
| `POST /scope/{pageid}/refresh` | Recomputes a scope with a `query` operand; needs `edit` on the page; rate class `query` | [0060](../decisions/0060-scopes.md) §8 |
| `GET /scope/{pageid}/dataset.jsonld` | The scope as a schema.org `Dataset` ([17](17-federation-and-publication.md)) | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /scope/{pageid}/dumps`, `GET /scope/{pageid}/dumps/{file}` | The current dumps and their manifests; serves one | [0076](../decisions/0076-dataset-publication.md) §6 |
| `GET /sitelinks?host=` | The entities linked to a host or, with `parents=1`, to it and every host beneath it | [0026](../decisions/0026-sitelinks.md) §7 |
| `GET /sitelinks/resolve?url=` | The entity for a URL | [0026](../decisions/0026-sitelinks.md) §7 |
| `GET`, `POST /sparql` | The SPARQL 1.1 Protocol for queries only, over the tenant's dataset (§6) | [0059](../decisions/0059-query-service.md) §6 |
| `GET /sparql/prefixes` | The prefix set as Turtle | [0059](../decisions/0059-query-service.md) §6 |
| `GET /sprint/{pageid}` | Counts, window, truncated, progress | [0061](../decisions/0061-sprints-and-tasks.md) §10 |
| `GET /sprint/{pageid}/leaderboard` | The leaderboard | [0061](../decisions/0061-sprints-and-tasks.md) §10 |
| `GET /sprint/{pageid}/tasks?state=&rule=&claimant=&after=&limit=` | The tasks | [0061](../decisions/0061-sprints-and-tasks.md) §10 |
| `POST /sprint/{pageid}/tasks/{rule}/{kind}/{id}/claim`, `…/release` | Claim and release (`force` for holders of `edit`) | [0061](../decisions/0061-sprints-and-tasks.md) §10 |
| `GET /subject/{kind}/{id}/scopes` | The scopes a subject is in | [0060](../decisions/0060-scopes.md) §8 |
| `GET /subject/{kind}/{id}/tasks` | Open tasks about a subject | [0061](../decisions/0061-sprints-and-tasks.md) §10 |
| `GET /suggest?q=` | Suggestions grouped across entity types, providers, keyed types and page titles | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /table/{pageid}/export?format=tsv\|csv` | The current values as one file: an ID column, then one column per table column, multiple best values joined by `; ` | [0045](../decisions/0045-table-content-model.md) §10 |
| `GET /table/{pageid}/rows?offset=&limit=&sort=&lang=` | The rows, paged: for each, the ID as written and as shown, the row state, and per column the best values in Wikibase JSON with the fold count, constraint flags and whether the viewer may edit the cell. `revid=` reads the definition as of that revision; the values are always current | [0045](../decisions/0045-table-content-model.md) §10 |
| `POST /table/{pageid}/rows/{id}` | One row's cell edits with `baserevid` and an optional summary. The server maps them to one change set and returns the new revision and the row as `GET` would. The grid's save path, and usable by bots | [0045](../decisions/0045-table-content-model.md) §10 |
| `GET`, `POST /takedowns`, `GET /takedowns/{id}`, `POST /takedowns/{id}/reinstate`, `POST /takedowns/{id}/expunge` | Farm base, for the operator rights of [0039](../decisions/0039-files-and-media.md) §21 | [0039](../decisions/0039-files-and-media.md) §17 |
| `GET /tenancy` | Farm base. The tenancy policy, including the farm slug, the farm code and the primary tenant's slug, and the `tenants` array (slug, issuer code as `issuer`, host, name, status), the tenant list `Special:Tenants` shows (§1.2) | [0028](../decisions/0028-tenancy-policy.md) §11; [0046](../decisions/0046-primary-tenant.md) §9 |
| `POST /thread` | `create`; accepts `also` | [0019](../decisions/0019-discussions.md) §9; [0049](../decisions/0049-boards.md) §11 |
| `GET /thread/{page ID}` | The thread: subject, `home` and `listings` (each with the talk page ID, title and time attached), status, and its posts as `as:Note` objects in tree order. For a foreign thread, read-only: the origin, state, section, comment tree (names omitted; hashes and DiscussionTools anchors given) and the upstream revision | [0019](../decisions/0019-discussions.md) §9; [0049](../decisions/0049-boards.md) §11; [0069](../decisions/0069-synchronized-talk-pages.md) §9 |
| `POST /thread/{page ID}/attach`, `POST /thread/{page ID}/detach` | `attach` and `detach`, each with a target and a base offset | [0019](../decisions/0019-discussions.md) §9; [0049](../decisions/0049-boards.md) §11 |
| `POST /thread/{page ID}/move` | `move`, with a base offset; accepts `keep` | [0019](../decisions/0019-discussions.md) §9; [0049](../decisions/0049-boards.md) §11 |
| `POST /thread/{id}/pin`, `POST /thread/{id}/unpin` | `{target, base}` | [0069](../decisions/0069-synchronized-talk-pages.md) §9 |
| `POST /thread/{page ID}/post` | `post`, with `inReplyTo` and optional `status` | [0019](../decisions/0019-discussions.md) §9 |
| `PUT /thread/{page ID}/post/{revid}` | `edit` | [0019](../decisions/0019-discussions.md) §9 |
| `POST /thread/{page ID}/rename` | `rename`, with a base offset | [0019](../decisions/0019-discussions.md) §9 |
| `POST /thread/{id}/upstream-reply` | `{parent, wikitext}`, where `parent` is a comment anchor or the thread; answers 202 with an upstream-post ID | [0069](../decisions/0069-synchronized-talk-pages.md) §9 |
| `GET /updates/stream`, `GET /updates`, `GET /updates/cursor` | The SPARQL Update stream, which keeps a triplestore current with a dump; public, rate-limited in the `stream` class ([02](02-graphs-rdf-and-query.md) §6) | [0012](../decisions/0012-api-requirements.md) §5 |
| `GET /upstream-post/{id}` | The send's state and upstream revision ID; visible to its author | [0069](../decisions/0069-synchronized-talk-pages.md) §9 |
| `GET /version` | Everything `Special:Version` shows, as one JSON document, at each tenant base and at the farm base. Public, cached like any public response, and subject to `version.services`. On a private tenant, an outsider gets what the farm base would show, without the tenant's features or entry points | [0077](../decisions/0077-special-version.md) §10 |
| `GET /watchlist` | The holder's watch set with expiries | [0020](../decisions/0020-change-feeds.md) §5 |
| `POST /watchlist/seen` | Update `seen` for one target or all | [0020](../decisions/0020-change-feeds.md) §5 |
| `GET /watchlist/token`, `POST /watchlist/token` | Read and reset the Atom token | [0020](../decisions/0020-change-feeds.md) §5 |
| `PUT /watchlist/{kind}/{id}`, `DELETE …` | Watch, with an optional `expires`; unwatch | [0020](../decisions/0020-change-feeds.md) §5 |
| `POST /workspace` | Create from a kit: name, scope, kit title; returns the job | [0062](../decisions/0062-workspaces.md) §8 |
| `GET /workspace/{pageid}` | A project page's roles (from `view.page_prop`), the component pages its blocks and kit created (from `page_link`), and its sprints (subpages of model `triplespace-sprint`) | [0062](../decisions/0062-workspaces.md) §8 |

The bulk-job submission endpoint is unchanged (§4). The MCP endpoint `{base}/mcp` is outside `rest.php` (§7). The ActivityPub endpoints and WebFinger are outside `triplespace/v0`, at the paths ActivityPub and WebFinger fix, for each actor kind ([17](17-federation-and-publication.md)).

Every `/scope/` route addresses a scope by page ID, the dataset and dump routes included.

The farm-base global group and block interface is the Action API — `action=globalblock`, `list=globalblocks` and the global-group modules (§2.3) — and no REST route; the catalogue has no row for one.

This table is the one index of the routes later ADRs added; [0012](../decisions/0012-api-requirements.md) §5 links to it.

### 3.3 `GET /resolve`

*Sources: [0012](../decisions/0012-api-requirements.md) §5; [0029](../decisions/0029-resolver-namespaces.md) §6; [0051](../decisions/0051-page-redirects.md) §7; [0078](../decisions/0078-entity-sources.md) §10.*

`GET /resolve?q=` resolves any string to its canonical page through the title resolver ([10](10-pages-and-content-models.md)). It tries, in order: entity IDs such as `WDQ65`, source IDs among them (`mhc:Q1`, answered with `kind: entity-source`); keyed-type IDs; notation keys ([0048](../decisions/0048-notation.md) §2); then resolver-prefixed strings (`doi:10.1000/xyz`) and bare strings that normalize to a valid key of exactly one resolver; and last main-namespace titles, so that a bare ID still reaches its entity first, and a main-namespace page with the same title is returned in an `also` field. For a resolver match the response carries `kind: resolver`, the resolver, the normalized key and either the target entity or the candidate list. It reports `redirected_from` when it followed a redirect. A string that normalizes to a notation key ([0048](../decisions/0048-notation.md) §2) resolves, as the exception to the bare-key rule; any other bare string that normalizes to a key of some keyed type, such as a domain name, is offered as a suggestion, since bare keys are not IDs ([0017](../decisions/0017-entity-id-grammar.md) §1).

### 3.4 MediaWiki core REST and the Wikibase REST API

*Sources: [0012](../decisions/0012-api-requirements.md) §4; [0026](../decisions/0026-sitelinks.md) §7; [0041](../decisions/0041-content-models.md) §9; [0042](../decisions/0042-template-expansion-and-parsoid.md) §12; [0044](../decisions/0044-tenant-relative-ids.md) §3; [0051](../decisions/0051-page-redirects.md) §7; [0066](../decisions/0066-lexemes.md) §8.*

The MediaWiki core and Wikibase REST APIs keep MediaWiki's meaning. Core REST `/v1/page/{title}/history` lists local revisions only, as `prop=revisions` does. Page objects in MediaWiki REST's shape (`GET /v1/page/{title}/bare` and its relatives) carry `content_model` for every page, as MediaWiki's do. `GET /v1/page/{title}` and relatives follow one redirect hop, with `redirect=no` to stop, answering as MediaWiki REST does. `/v1/revision/{id}/html` renders an old revision with the templates as they are now. The Wikibase REST API's `/entities/items/{id}/sitelinks/{site_id}` accepts alias IDs and hosts, and its path segments accept tenant-relative IDs. The Wikibase REST API has no lexeme routes yet, so none are mirrored. `GET /file/{title}` under `triplespace/v0` uses MediaWiki REST's `v1/file` shape.

## 4. The ingest API

*Sources: [0012](../decisions/0012-api-requirements.md) §5; [0044](../decisions/0044-tenant-relative-ids.md) §3; [0045](../decisions/0045-table-content-model.md) §10; [0066](../decisions/0066-lexemes.md) §8.*

The change-set format and its three entry points, among them the HTTP bulk-job endpoint, are [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8 and [05](05-providers-and-ingest.md). The bulk-job submission endpoint is unchanged by the routes of §3; the job routes of §3.2 (`/jobs`, `/jobs/{id}` and its `changes`, `rejects`, `events`, `revert` and `rerun`) report on and control the jobs it starts. Tenant-relative IDs in the wire format are canonicalized during validation. Routes that write on an entity's or page's behalf map to the same change sets: `POST /table/{pageid}/rows/{id}` maps one row's cell edits to one change set, and each lexeme module appends a change set whose operations address parts by ID.

## 5. Diffs and rendering as of a revision

*Sources: [0012](../decisions/0012-api-requirements.md) §7; [0042](../decisions/0042-template-expansion-and-parsoid.md) §12.*

### 5.1 Structured diffs

*Sources: [0012](../decisions/0012-api-requirements.md) §7, §2, §4.*

`GET /entity/{id}/changes/{partition}/{offset}` returns the structured diff of one change set: the statement view of [0010](../decisions/0010-site-ui.md) §6, which `action=compare` does not give ([0012](../decisions/0012-api-requirements.md) §4). A diff response holds:

- **The change set's operations,** as stored.
- **Before and after, as statement groups.** For each property the change touched, the group in canonical JSON before and after the change, with the neighbouring values for context. "Before" is the source graph's state just before the record. Terms and sitelinks are returned as keyed lists.
- **An effect block,** in three parts:
  - `resolved`: the groups and values that changed in the resolved view;
  - `truthy`: the truthy triples added and removed;
  - `upstream`: whether anything was written upstream, and the correction's state.
- **An integrity block:** the record's coordinates and the checkpoint that includes it.

**The effect is computed against the other sources as they are now,** and labelled that way. Their earlier states may have been compacted, so the resolved view at the time of the change cannot, in general, be rebuilt.

**For a sync,** the diff comes from `delta` where it was stored, which is only when the provider's registry entry sets `sync_deltas: full`. Otherwise it comes from `changes`, the per-property summary of counts added, removed and changed, with a link to the provider's own diff between `prev_upstream` and the new version; for Wikidata, that is `index.php?diff={new}&oldid={prev}`. The default is `sync_deltas: summary`. In bootstrap mode these fields are empty ([05](05-providers-and-ingest.md)).

### 5.2 Rendering a revision as of its time

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §12; [0053](../decisions/0053-mirrored-pages.md) §10.*

**The Action API and MediaWiki's REST API keep MediaWiki's meaning:** `action=parse&oldid=` and `/v1/revision/{id}/html` render an old revision with the templates as they are now.

**`triplespace/v0` adds what an append-only log makes possible:** `GET /page/{id}/html?revision={revid}&as_of=revision` renders the revision with every local page it depends on **as of the revision's offset**.

- Local pages are exact, since every revision is in the log.
- Entities come from the tenant's view as of that offset where the log holds it, and current otherwise.
- Foreign repositories are current.

The response says which, with `as_of_exact`. An exact response is immutable and caches as record-derived data does ([03](03-storage-caches-and-search.md) §9). For a foreign page, `as_of` is refused with `ts-foreign-history`.

## 6. Raw SPARQL for people

*Sources: [0059](../decisions/0059-query-service.md) §6; [0063](../decisions/0063-query-namespace.md) §7; [0062](../decisions/0062-workspaces.md) §8; [0064](../decisions/0064-entityschema-and-validation.md) §7.*

**`GET` and `POST /sparql`**, at the tenant base, implement the SPARQL 1.1 Protocol for queries: `query=`, result formats `application/sparql-results+json` (default), `+xml`, `text/csv`, `text/tab-separated-values`, and for `CONSTRUCT`/`DESCRIBE` the RDF formats `triplespace-rdf` already writes. The dataset is the tenant's ([02](02-graphs-rdf-and-query.md) §7, [08](08-tenants-and-instances.md) §9); `FROM` is refused; `SERVICE` is refused in version 1; Update is not served, as the protocol's update operation is simply absent. `query.timeout` (default 30 s) and `query.max_results` (default 100,000 solutions) bound every query, and a result cut at either says so in a `Query-Truncated: timeout | results` header, since the result formats have no place for a warning. The response carries `Query-Cursor: {epoch}:{seq}`.

**Prefixes.** The prefixes of the Wikibase-compatible dump ([wikibase-compat.md](../api/wikibase-compat.md) §4) are pre-declared, with the tenant's own `wd:`, `wdt:`, `p:`, `ps:`, `pq:` and the rest bound to the tenant's IRIs, as the Wikidata Query Service pre-declares Wikidata's; a query may redeclare any of them. The prefix set is served at `GET /sparql/prefixes` as Turtle, so that a client can show it.

**Rate limits**: a class, `query`, 30 / 300 per minute (`user` / `bot`), counting queries started; a subsidiary that needs more asks for it as any does ([07](07-actors-and-accounts.md) §6). Concurrency is bounded instance-wide by `query.max_concurrent` (default 8), beyond which a query waits up to its own timeout. **Permission**: `read`. **Caching**: a `GET /sparql` response is cacheable at L2 for `query.cache_ttl` (default 60 s) by its full URL, since the store holds public form only and the dataset is a function of the tenant; `POST` is not cached.

The same class, limits and headers govern `GET /query/{pageid}/run`, `POST /scope/preview`, `POST /scope/{pageid}/refresh` and `POST /schema/{id}/check` (§3.2). `Special:Query`, the editor over `/sparql`, is [21](21-special-pages.md); it reads nothing but `/sparql`.

## 7. The MCP server

*Sources: [0075](../decisions/0075-mcp-server.md) §1, §2, §3, §4, §6.*

### 7.1 One endpoint per tenant

*Sources: [0075](../decisions/0075-mcp-server.md) §1, §6.*

**Each tenant with `mcp.enabled` serves MCP at `{base}/mcp`**, over the protocol's Streamable HTTP transport, from `triplespace-server` like every other API surface ([0033](../decisions/0033-backend-stack.md) §3). `mcp.enabled` is a `site` setting, default off; `mcp.scope`, `mcp.scopes`, `mcp.facets`, `mcp.instructions` and `mcp.max_results` are `site` settings with it ([23](23-configuration-and-registry.md)). `meta=siteinfo&siprop=triplespace` reports the endpoint when enabled, and the REST root lists it, so a client that knows the wiki can find the server.

**A default scope narrows every tool.** `mcp.scope` names the scope the tools search within by default; `mcp.scopes` lists other scopes a client may select per session with `?scope={title}` on the endpoint URL. With no scope, the tools range over the tenant's resolved view.

### 7.2 Tools, read-only

*Sources: [0075](../decisions/0075-mcp-server.md) §2.*

| Tool | Answers | Backed by |
|---|---|---|
| `search` | Entities by name in a language, with labels, descriptions and match type | The search index ([03](03-storage-caches-and-search.md) §11), as `wbsearchentities` |
| `get_entity` | One entity: labels, descriptions, statements with property and value labels, identifiers with their URLs, and the canonical IRI | The resolved view |
| `find` | Entities in the scope matching filters: `{property, value}`, `{property, values}` (any of), `{property, from, to}` for times and quantities, and `{path}` for transitive paths as [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §2 writes them; paged by cursor | A compiled query ([02](02-graphs-rdf-and-query.md) §7.4) over the scope |
| `facets` | For a filter, the values of the configured facet properties among the matches, with counts | `/scope/{id}/facets` |
| `mentions` | The pages an entity was derived from, with the lines that mention it | `GET /entity/{id}/derivations` |
| `run_query` | A saved `Query:` page by title, with its parameters | [0063](../decisions/0063-query-namespace.md) |
| `sparql` | A SELECT query, under the query service's limits | `/sparql` (§6) |

Every result carries labels in the language the client asks for (`lang`, default the tenant's content language), the canonical IRI of every entity, and the URL of every page. `find`, `facets`, `run_query` and `sparql` need the query service; on a tenant without one, the server lists only `search`, `get_entity` and `mentions`.

**Not yet.** No tool writes in this version.

### 7.3 Vocabulary and resources

*Sources: [0075](../decisions/0075-mcp-server.md) §3, §4.*

**An agent learns which properties matter from the server, not from the property list.**

- **`mcp.facets`** lists the properties offered as filters and facets, each with a short description written for the agent ("record type: the kind of records a resource holds, such as probate or census"). The descriptions of `find` and `facets` are generated from it, with the properties' labels and IDs.
- **`mcp.instructions`** is free text returned as the server's instructions at initialization: what the tenant holds, what a typical question looks like, what the facets mean.

The server exposes, as MCP resources:

- each scope it serves: its description, size, facet properties, and the URLs of its dumps ([17](17-federation-and-publication.md)), so that an agent that wants everything downloads a dump instead of paging `find`;
- the facet property list, with each property's data type and, for items, its most common values.

Access control for the server is [09](09-security-and-moderation.md) §8.11.

## 8. Compatibility

*Sources: [0012](../decisions/0012-api-requirements.md) §1; [0008](../decisions/0008-namespaces-and-document-pages.md) §12; [0025](../decisions/0025-oauth-server.md) §9; [0077](../decisions/0077-special-version.md) §10; [0041](../decisions/0041-content-models.md) §8.*

What MediaWiki and Wikibase clients rely on is specified in [`../api/mediawiki-compat.md`](../api/mediawiki-compat.md) and [`../api/wikibase-compat.md`](../api/wikibase-compat.md), which this chapter does not repeat: the Action API modules and the `continue` protocol, the core and Wikibase REST APIs and their OpenAPI documents, the JSON serialization and the RDF mapping. `mediawiki-compat.md` carries a section on the OAuth 2.0 routes and their MediaWiki paths, noting the 1.0a gap, and attributes each served module to the extension that `siprop=extensions` reports (§2.2); it treats `contentmodel` values as site-specific, which is why `action=paraminfo` enumerates the implemented models. Pywikibot reading and editing `Project` pages is the acceptance test.
