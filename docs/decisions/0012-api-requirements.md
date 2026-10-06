# 0012. API requirements for the site UI

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-10-06 (A49)
- **Author:** James Hare / Claude Opus
- **Changes:** [0000](0000-init.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0005](0005-crate-organization.md)
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

1. **MediaWiki modules keep MediaWiki's meaning.** They describe changes made on this instance, one revision at a time. Pywikibot and other MediaWiki clients must work without changes. Reading and editing `Project` pages remains the acceptance test ([0008](0008-namespaces-and-document-pages.md) §12).
2. **Changes to MediaWiki modules are additive.** They may add parameters, enum values and output fields that appear only when requested. They never change what an existing request returns.
3. **Data only Triplespace has goes in a new REST namespace, `rest.php/triplespace/v0`.**
   - `v0` means what it means for the vocabulary ([0005](0005-crate-organization.md) §5): no stability promise yet.
   - An OpenAPI document, committed as `docs/api/triplespace-v0.openapi.json`, is the normative contract. The Wikibase REST API's OpenAPI document plays the same role ([mediawiki-compat.md](../api/mediawiki-compat.md) §6.2).
4. **The UI uses only the public API.** Anything the UI can show, a third-party client can show too.
   - The UI finds out which Triplespace-only features exist through capability discovery (§4, `siprop=triplespace`).
   - Against a stock Wikibase, those features are hidden, as [0003](0003-statement-ui.md) (Consequences) already requires.
5. **Every response is redacted for its viewer.**
   - Hidden and erased fields follow [0001](0001-revision-metadata-rdf.md) §4, [0006](0006-log-integrity-and-erasure.md) §7 and [0011](0011-logs.md) §5.
   - Data from a `private` graph ([0007](0007-actor-identity.md) §8) is returned only to the account holder, in their own session.

### 2. Prerequisites

#### 2.1 Global revision and log IDs

*Changed by A2, A4, A7.*

MediaWiki clients depend on numeric IDs from one sequence per wiki. These include `revid`, `parentid`, `oldid`, `baserevid`, `lastrevid`, `logid`, and `fromrev` and `torev` in `action=compare`.

Many parts of this ADR depend on them:

- diff URLs;
- `prop=revisions`;
- `list=recentchanges`;
- `list=logevents`;
- edit conflicts;
- the core REST history routes.

The IDs are Postgres sequences, one set per tenant, taken in the appending transaction and written into the record's hashed header, fields 7–9 ([0013](0013-postgres-storage.md) §6, [0015](0015-record-format-and-partition-registry.md) §2, [0018](0018-tenants.md) §2). They meet these requirements:

- **One sequence for revisions and one for log events, per tenant,** since MediaWiki clients expect one sequence per wiki. Each spans every partition that holds the tenant's revisions or events: `local`, `pages`, `log`, and the actor records that project as log events.
- **IDs are assigned when a record is appended, and stored in its header.** An ID that came from replay order would change whenever projections were rebuilt in a different interleaving. In the header, an inclusion proof covers the ID, and an export bundle needs no sidecar.
- **The sequence increases with append order.** Gaps are acceptable.
- **Mirror records get a provider-ranged revision ID,** `provider_number << 40 | n`, computed by the writer with no allocation, so that `lastrevid`, `baserevid` and `schema:version` are defined for an entity with no local revision. They are still not revisions of this wiki: `prop=revisions` and `list=recentchanges` list local revisions only (§4). Upstream log events keep their upstream log ID and get no local one ([0015](0015-record-format-and-partition-registry.md) §2).
- **Erasure does not free an ID.** An erased record keeps its ID, as its header survives ([0006](0006-log-integrity-and-erasure.md) §7).

#### 2.2 Mirror metadata that survives compaction (amends [0002](0002-source-graphs-and-mass-ingest.md) §8.2)

Under the `latest` history policy, a mirror keeps only the newest `put` for each entity. The history view ([0010](0010-site-ui.md) §5) needs facts about earlier states that compaction would otherwise destroy. Projections must be rebuildable from the log, so these facts have to be in the surviving record.

A `put` therefore carries these new fields:

| Field | Meaning |
|---|---|
| `prev_upstream` | The upstream version of the state this `put` replaces, such as Wikidata's `lastrevid`. It bounds the live fetch of upstream edits (§6). |
| `first_seen` | When the instance first mirrored the entity. It is copied forward from the previous `put`. |
| `size`, `prev_size` | The sizes of the new and previous states (§2.4) |
| `changes` | A summary of what changed. For each property, term kind (labels, descriptions, aliases, by language) and sitelink site: counts of values added, removed and changed. |
| `delta` | Optional. The statement-level difference. It is present only when the provider's registry entry sets `sync_deltas: full`. |

**The default is `sync_deltas: summary`.** Without a stored delta, the diff of a sync shows the summary and links to the provider's own diff between `prev_upstream` and the new version. For Wikidata, that is `index.php?diff={new}&oldid={prev}`.

**Summaries and deltas are computed at ingest,** by comparing with the entity's current mirrored state. In bootstrap mode ([0002](0002-source-graphs-and-mass-ingest.md) §8.6) there is no previous state, and these fields are left empty.

#### 2.3 Ordering and paging

Lists that merge several partitions need one order that every client sees the same way.

- **Every activity row has a time and a position.** The time is when the event happened at its source. The position is the (partition, offset) of the record the row comes from.
- **Rows are ordered by (time, partition, offset).** This is a total order.
- **Continuation tokens are opaque.** Each encodes the last sort key returned and a hash of the filters. This follows MediaWiki's `continue` protocol ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.6).
- **A sync row's time** is when it was observed on the instance.
- **Upstream edits and upstream log events** keep their upstream timestamps.
- **Imported page revisions** keep their original timestamps ([0008](0008-namespaces-and-document-pages.md) §9).

#### 2.4 Size

- **An entity's size** is the byte length of its canonical JSON in the record's source graph after the change. For the local graph, that is the local assertions about the entity. For a mirror, it is the mirrored state.
- **A size change** is measured against the same source's previous state.
- **The resolved view's size is not reported.** It depends on every source at once, and a source's earlier states may have been compacted.
- **A document page's size** is measured as in MediaWiki.

### 3. The activity row

*Changed by A5, A12, A18.*

History, recent changes, contributions and the log all use one row shape in the REST API.

| Field | Contents |
|---|---|
| `kind` | `edit`, `sync`, `upstream-edit`, `log`, `job` or `erased` |
| `time`, `appended` | When the event happened at its source; when this instance recorded it |
| `source` | The graph: `local`, `pages`, `mirror/wikidata`, `log`, `log/wikidata`, and so on |
| `record` | `{partition, offset}`. Absent for upstream edits fetched live. |
| `revid`, `logid` | Once §2.1 exists |
| `upstream` | The upstream revision or log ID, and its URL |
| `target` | The entity ID or page ID; the title; the label in the viewer's language; the cluster member the record is keyed to |
| `actor` | Actor key, IRI, current name, kind, and a hidden flag ([0007](0007-actor-identity.md) §1–5) |
| `operator`, `requested_by` | For bots and jobs ([0007](0007-actor-identity.md) §6, [0011](0011-logs.md) §6.3) |
| `job` | The job ID |
| `summary`, `comment` | The parsed summary: operation, arguments and automatic text ([0001](0001-revision-metadata-rdf.md) §6). The user's own comment is a separate field. |
| `tags`, `flags` | Change tags, which a record stores in its attestation part ([0030](0030-edit-filters.md) §5); the minor, bot and new flags |
| `patrolled` | Whether the row has been patrolled ([0023](0023-moderation.md) §8) |
| `filters` | The edit filters the record matched, with their actions, for rows the viewer may see ([0030](0030-edit-filters.md) §9) |
| `size`, `delta` | §2.4 |
| `changes` | For syncs: the summary of §2.2 |
| `correction` | For corrections: the state in [0003](0003-statement-ui.md) §4 (active, redundant or dangling) |
| `visibility` | Which fields are hidden. For `erased` rows, the reason class, if the viewer holds `ts-viewerasures` ([0016](0016-permissions-and-access-control.md) §6). |
| `links` | Diff, history, job, and upstream URLs |

### 4. Action API changes

*Changed by A3, A4, A5, A7, A8, A9, A10, A12, A13, A14, A15, A16, A17, A18, A19, A21, A23, A24, A25, A26, A27, A30, A31, A32, A33, A34, A35, A36, A37, A38.*

All of these changes are additive (§1.2). [0056](0056-security-model.md) §13 adds `read=` to `action=protect`, `prtype=read` to `list=protectedpages`, `list=protectedsets`, and the reduced `meta=siteinfo` an outsider receives from a private tenant.

| Module | Requirement |
|---|---|
| `meta=siteinfo` | Describes the tenant the host selects ([0018](0018-tenants.md) §11). New `siprop` values:<br>• `providers`: codes, entity types, namespaces, IRI templates, and whether upstream history can be fetched.<br>• `issuers`: the issuers allowed for login, without any secrets.<br>• `triplespace`: capabilities, `api_version`, which the web tier checks against the minimum it was built for ([0057](0057-web-tier.md) §9), the REST base path, the tenant and its opted-in providers ([0018](0018-tenants.md) §11), the search backend, `opensearch` or `postgres` ([0014](0014-caches-and-search.md) §8), `theme`, the tenant's `ui.theme` token values, which the site serves as its stylesheet ([0034](0034-frontend-stack.md) §1) and which an outsider of a private tenant is not given, its landing page wearing Codex's own values, and `hash_mismatches`, the instance-wide count of upstream hashes kept because the recomputation differed ([0006](0006-log-integrity-and-erasure.md) A9). Later ADRs add more (below).<br>Namespaces come from the registry ([0008](0008-namespaces-and-document-pages.md) §1). |
| `prop=revisions`, core REST `/v1/page/{title}/history` | Local revisions only: the local graph for entities, the `pages` partition for document pages. An erased part sets its MediaWiki hidden flag, `texthidden` for the content, `commenthidden` for the comment, `userhidden` for the attestation, and a new `erased` flag is set with it ([0015](0015-record-format-and-partition-registry.md) §1). That a record was erased is shown to everyone ([0016](0016-permissions-and-access-control.md) §6). New `rvprop` values: `source`, `job` and `operator`. |
| `list=recentchanges` | One row per local revision, as in MediaWiki. Job changes carry the `bot` flag and a `job:{id}` tag. A new `rcjob` parameter filters by job. Mirror records never appear. Each mirror job run appears as a `job` log event ([0011](0011-logs.md) §6.3). `rctype=external` is not used, because Wikibase clients already give it a meaning. |
| `list=usercontribs` | By local account. Job changes appear with their tags. A new `ucjob` parameter filters by job. |
| `list=logevents` | Serves the events of [0011](0011-logs.md), with the new types `erase`, `link`, `retention`, `convert` and `job`, and the action `renameuser/vanish`. A new `leprovider` parameter selects local events (the default) or a provider's. Redacted per viewer. |
| `list=users` | New `usprop` values: `linkedaccounts` (public links, [0007](0007-actor-identity.md) §7) and `operator` (for bots). The existing `cancreate` checks names at first login. |
| `action=parse` | Accepts the `markdown` and `yaml` content models ([0008](0008-namespaces-and-document-pages.md) §5). Reports the renderer version, so that the in-browser preview can detect a mismatch. |
| `action=compare` | For document pages, as in MediaWiki. For entities, a diff of the canonical JSON of the local graph's assertions: the "JSON" view in [0010](0010-site-ui.md) §6. The statement view is served over REST (§5). |
| `action=edit`, `wbeditentity` and the other write modules | Map `baserevid` to base offsets ([0006](0006-log-integrity-and-erasure.md) §8). An `editconflict` error includes the current revision's ID, so the client can merge a document page three ways. |
| `action=options` | New preference keys: `ts-upstream-fold` and `ts-rc-syncs` ([0010](0010-site-ui.md) §11). Only registered keys are accepted ([0027](0027-preferences-and-portability.md) §6) |
| `action=clientlogin`, `action=createaccount`, `meta=authmanagerinfo` | Offer a redirect-based OAuth flow for each issuer allowed for login, and MediaWiki's own `username`/`password` fields where the built-in `password` issuer is on ([0007](0007-actor-identity.md) A3). These are not offered to subsidiaries. `returnto` is honoured only on the request's own origin, here and on the `/auth` routes of §5, and ignored otherwise ([0057](0057-web-tier.md) §7) |
| `action=login` | The bot-password form only, for subsidiaries' API keys ([0024](0024-subsidiary-accounts.md) §4). A primary account's name is refused with `ts-use-oauth` |
| `action=linkaccount`, `action=unlinkaccount`, `action=removeauthenticationdata` | Manage **bindings**, the private sign-in methods of [0007](0007-actor-identity.md) §3, because that is what these modules mean in MediaWiki. They never create or remove the public account links of [0007](0007-actor-identity.md) §7. Removing the last binding is refused. |
| `action=acquiretempusername` | Temporary accounts ([0007](0007-actor-identity.md) §3) |

**Modules added by later ADRs.** Each ADR specifies its modules in full; this table says where.

| Area | Modules | ADR |
|---|---|---|
| Permissions | `siprop=usergroups`; `uiprop=rights\|groups\|blockinfo`; `usprop=groups\|rights\|blockinfo`; `action=userrights`, `action=block`, `action=unblock`; `list=blocks`. Every write module names the missing permission in `permissiondenied` | [0016](0016-permissions-and-access-control.md) §7 |
| Moderation | `action=protect`, `action=delete`, `action=undelete`, `action=revisiondelete`, `action=patrol`; `hidename` on `action=block`; `inprop=protection`, `list=protectedtitles`, `list=protectedpages`; `prop=deletedrevisions`, `list=alldeletedrevisions`, `list=deletedrevs`; `rcshow=patrolled` | [0023](0023-moderation.md) §8 |
| Threads | Read as any page. `action=edit` on a `Thread:` or talk title is refused with `no-direct-editing` | [0019](0019-discussions.md) §9, [0041](0041-content-models.md) §8 |
| Watchlist | `list=watchlist`, `list=watchlistraw`, `action=watch` and `unwatch` with `expiry`, `action=setnotificationtimestamp`, `inprop=watched\|notificationtimestamp` | [0020](0020-change-feeds.md) §5 |
| Notifications | Echo's `meta=notifications`, `action=echomarkread` and `action=echomarkseen`; `uiprop=hasmsg` | [0021](0021-notifications.md) §7 |
| Subsidiaries and rate limits | `uiprop=ratelimits`, and `operator` and `grants` for a subsidiary; `usprop=subsidiaries`; the rate-limit classes and the grant set in `siprop=triplespace`; `ratelimited` from every write module | [0024](0024-subsidiary-accounts.md) §8 |
| OAuth consumers | `uiprop=oauth`, `list=oauthconsumers`, `action=oauthconsumer`; whether approval is required, in `siprop=triplespace` | [0025](0025-oauth-server.md) §9 |
| Sitelinks | `wbsetsitelink` by alias ID, host or `linkurl`; `sitefilter`, `sites`, `titles` and `urls` in `wbgetentities`; `url` entries in `wbeditentity`; `sitelinks` in `siprop=triplespace` | [0026](0026-sitelinks.md) §7 |
| Preferences | `action=options` with `reset`; `uiprop=options` with defaults filled in | [0027](0027-preferences-and-portability.md) §6 |
| Resolvers | `resolver` and `key` in `wbgetentities` | [0029](0029-resolver-namespaces.md) §6 |
| Edit filters and tags | `list=abusefilters`, `list=abuselog`, `action=abusefiltercheckmatch` and the other AbuseFilter modules; `abusefilter-warning` and `abusefilter-disallowed` from every write module; `list=tags`, `action=tag` | [0030](0030-edit-filters.md) §9 |
| Constraints | `wbcheckconstraints` | [0031](0031-property-constraints.md) §6 |
| Categories | `prop=categories`, `list=categorymembers`, `prop=categoryinfo`, `list=allcategories` | [0038](0038-page-metadata-and-categories.md) §13 |
| Files | `action=upload`, `action=filerevert`, `prop=imageinfo`, `meta=filerepoinfo` and the rest | [0039](0039-files-and-media.md) §17 |
| Content models | `defaultcontentmodel`; `contentmodels` in `siprop=triplespace`; `contentmodel` for every page in `prop=info` and `prop=revisions`; `rvslots` with `main` and, on File pages, `mediainfo`; `no-direct-editing` from `action=edit` on a non-text model; `action=changecontentmodel` with MediaWiki's codes | [0041](0041-content-models.md) §8 |
| Templates | `action=expandtemplates`; expansion, `prop=templates` and the limit report in `action=parse`; `prop=transcludedin`, `list=embeddedin`, `list=alltransclusions`; `magicwords`, `functionhooks`, `extensiontags`, `variables` and `doubleunderscores` in `meta=siteinfo`; `inprop=linkclasses`; `meta=allmessages` | [0042](0042-template-expansion-and-parsoid.md) §14 |
| Lua | `prop=wbentityusage`, `list=wblistentityusage`, `wikibase_item` in `prop=pageprops` | [0043](0043-lua-modules.md) §16 |
| Redirects | `redirects` on `action=query` and `action=parse`; `redirect` and `linkclasses` in `prop=info`; `prop=redirects`, `list=allredirects`, `apfilterredir`, `blredirect`; `noredirect`, `movesubpages` and the move-over-redirect rule in `action=move` | [0051](0051-page-redirects.md) §7 |
| Page repositories | `origin` in `prop=info`; the latest upstream revision in `prop=revisions` with `ts-foreign-history` for older ones; `tsorigin=all` on `list=allpages` and `list=prefixsearch`; `ts-foreign-page` from writes to an inherited title; `page_repos` in `siprop=triplespace` | [0052](0052-page-repositories-and-title-inheritance.md) §6 |
| Mirrored pages | `action=parse` and `prop=revisions&rvprop=content` on an inherited title from its bundle; `action=purge` refetching | [0053](0053-mirrored-pages.md) §10 |
| Forks | `tsfork` and `tsforkrevid` on `action=edit`, `ts-fork-required`; seeded revisions with provider-ranged IDs in `prop=revisions` and `action=compare`; `forkedfrom` in `prop=info`; forks as `import/interwiki` in `list=logevents` | [0054](0054-forking-a-mirrored-page.md) §11 |
| TemplateData and page properties | `action=templatedata`; `prop=pageprops` with `ppprop`, `list=pageswithprop`, `prop=description`, `inprop=displaytitle` | [0055](0055-templatestyles-templatedata-and-page-properties.md) §5–6 |

### 5. REST routes under `rest.php/triplespace/v0`

*Changed by A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15, A16, A17, A18, A19, A20, A21, A22, A23, A24, A25, A26, A28, A30, A31, A32, A33, A37, A39, A40, A41, A42, A43, A44, A45, A46, A47, A48, A49.*

**Every route is served per tenant base,** which the host selects. Instance-level routes are served at the farm base and, on a single-tenant instance, at the tenant's as well ([0018](0018-tenants.md) §11).

**Entities**

| Route | Purpose |
|---|---|
| `GET /entity/{id}/provenance` | The provenance response of [0003](0003-statement-ui.md) §6, plus the data for the identity line: who minted the ID, cluster members and the canonical ID, statement counts by graph, sync state (last sync, upstream version, `first_seen`), retention and history policies, and the backlink count. Each statement carries its constraint results ([0031](0031-property-constraints.md) §6). For an adopted entity, the minter is the source wiki, with the adoption job and `source_revid` ([0035](0035-adopting-a-wikibase.md) §6) |
| `GET /entity/{id}/history` | Activity rows (§3) across the whole cluster, including log events keyed to its members. Filters: `source`, `kind`, and a time range. |
| `GET /entity/{id}/upstream?from=&to=` | Upstream edits fetched live (§6) |
| `GET /entity/{id}/changes/{partition}/{offset}` | The structured diff of one change set (§7) |
| `GET /entity/{id}/compare?source=&from=&to=` | Two states of one source ([0010](0010-site-ui.md) §5.6) |
| `POST /entity/{id}/undo/{partition}/{offset}` | Appends the inverse change set. The base is required. |
| `POST /entity/{id}/retention`, `POST /entity/{id}/convert` | Single-entity forms of `retain` and `convert` ([0002](0002-source-graphs-and-mass-ingest.md) §5–6) |
| `GET /corrections?provider=&state=` | Local corrections by state, for reporting upstream ([0002](0002-source-graphs-and-mass-ingest.md) §7) |

**Pages**

| Route | Purpose |
|---|---|
| `GET /page/{pageid}/provenance` | Import origin, and the revision count by origin: imported, bot or local ([0010](0010-site-ui.md) §4); the provenance of the page's statements ([0038](0038-page-metadata-and-categories.md) §13); for an adopted page, the adoption job ([0035](0035-adopting-a-wikibase.md) §6) |

**Activity and accounts, public**

| Route | Purpose |
|---|---|
| `GET /activity` | Recent changes as the UI shows them: one row per job, mirror syncs on request, and grouping by page. Filters: `kind`, namespace and period, and the feed sets of [0020](0020-change-feeds.md) §5: `target`, `related`, `set=watchlist` and `syncs`. |
| `GET /actor/{key}` | The public profile: current name, kind, status, linked accounts, and the operator of a subsidiary or the subsidiaries of a primary account ([0024](0024-subsidiary-accounts.md) §8) |
| `GET /actor/{key}/contributions` | Contributions by account, including jobs run on the account's behalf |

**Jobs**

| Route | Purpose |
|---|---|
| `GET /jobs` | Filters: status, graph and actor. Mirror syncs are instance jobs, listed at the farm base; bulk jobs are the tenant's ([0018](0018-tenants.md) §11) |
| `GET /jobs/{id}` | The job record and status: counts by outcome, the hash-mismatch count by value type ([0006](0006-log-integrity-and-erasure.md) A9), source and adapter versions, and checkpoint. While running: progress and projection lag. For `snapshot` jobs: the sweep count and its threshold. |
| `GET /jobs/{id}/changes?outcome=` | The records the job wrote, paged |
| `GET /jobs/{id}/rejects` | Downloads the rejects file. Requires a right (§8). |
| `GET /jobs/{id}/events` | Server-sent events for progress |
| `POST /jobs/{id}/revert` | Starts the inverse job ([0010](0010-site-ui.md) §9). The response lists the entities edited since the job, whose later edits are kept. |
| `POST /jobs/{id}/rerun` | Runs the job again with its stored parameters |

The bulk-job submission endpoint ([0002](0002-source-graphs-and-mass-ingest.md) §8.1) is unchanged.

**Search and titles**

| Route | Purpose |
|---|---|
| `GET /suggest?q=` | Suggestions grouped across entity types, providers, keyed types and page titles ([0010](0010-site-ui.md) §3) |
| `GET /resolve?q=` | Resolves any string to its canonical page through the title resolver ([0008](0008-namespaces-and-document-pages.md) §3). It tries, in order: entity IDs such as `WDQ65`; keyed-type IDs; resolver-prefixed strings (`doi:10.1000/xyz`) and bare strings that normalize to a key of exactly one resolver ([0029](0029-resolver-namespaces.md) §6); then main-namespace titles, with a same-titled page returned in `also` ([0038](0038-page-metadata-and-categories.md) §8). A bare string that normalizes to a key of some keyed type, such as a domain name, is offered as a suggestion, since bare keys are not IDs ([0017](0017-entity-id-grammar.md) §1). |

**Integrity**

| Route | Purpose |
|---|---|
| `GET /record/{partition}/{offset}/checkpoint` | The earliest checkpoint that includes the record |
| `GET /record/{partition}/{offset}/proof?checkpoint=` | An inclusion proof ([0006](0006-log-integrity-and-erasure.md) §9). Both routes return the record's signature, where it has one ([0024](0024-subsidiary-accounts.md) §4) |

**Account, holder only**

| Route | Purpose |
|---|---|
| `GET /account` | Name, bindings (private), links and preferences |
| `POST /account/rename`, `POST /account/vanish` | [0007](0007-actor-identity.md) §4, [0008](0008-namespaces-and-document-pages.md) §6 |
| `POST /account/links` | Starts the linking flow. The issuer requires the user to log in again ([0007](0007-actor-identity.md) §7). |
| `DELETE /account/links/{actor}` | Unlinks, by erasing the link |
| `GET /auth/{issuer}/start`, `GET /auth/{issuer}/callback` | The login redirect flow |
| `GET /auth/pending` | At first login: the pending identity's username, so the page can offer it as a suggestion. Only that session can read it. |
| `POST /auth/pending/create` | Creates the account with the chosen name. It may also create a link, using the login that just took place as proof ([0010](0010-site-ui.md) §10). |

When each binding was last used is stored in the operational store, never in the log.

**Routes added by later ADRs.** Each ADR specifies its routes in full; this table says where.

| Area | Routes | ADR |
|---|---|---|
| Access control | `GET`, `PUT` and `DELETE /acl/{kind}/{id}`, for graph, entity, record and actor targets | [0016](0016-permissions-and-access-control.md) §7, [0023](0023-moderation.md) §8 |
| Patrol | `POST /activity/{partition}/{offset}/patrol` | [0023](0023-moderation.md) §8 |
| Threads | `/thread` and its operations; `GET /page/{id}/threads` | [0019](0019-discussions.md) §9 |
| Watchlist and streams | `GET /activity/stream`, server-sent events for any feed; `/watchlist` and its Atom token | [0020](0020-change-feeds.md) §4–5 |
| Notifications | `/inbox`; `/notifications/…` | [0021](0021-notifications.md) §7 |
| Verified sync | `/.well-known/tlog/{partition}/checkpoint`, `/dumps/local/`, `GET /providers/{slug}/verification`: what an instance publishes for verified sync, and how a reader reports it | [0022](0022-federation.md) §1, §11 |
| Subsidiaries and keys | `/account/subsidiaries/…` and their keys; `POST /actor/{key}/transfer` | [0024](0024-subsidiary-accounts.md) §8 |
| OAuth | `/oauth/authorize`, `/oauth/token`, `/oauth/device`, `/oauth/revoke`, `/oauth/identify`, `/.well-known/oauth-authorization-server`: the instance's own OAuth 2.0 server, whose tokens are credentials of a subsidiary, never of a primary account. `/oauth/consumers/…`; `/account/authorizations`, `/account/pending-subsidiaries`; `/oauth/requests/{handle}`, read and decided by the site's consent page ([0057](0057-web-tier.md) §13) | [0025](0025-oauth-server.md) §1, §9 |
| Sitelinks | `GET /sitelinks`, `GET /sitelinks/resolve`, `/entity/{id}/sitelinks/{host}` | [0026](0026-sitelinks.md) §7 |
| Preferences and portability | `/account/preferences`, `GET /account/export`, `POST /account/import`, `GET /preferences/schema` | [0027](0027-preferences-and-portability.md) §6 |
| Resolvers | `GET /resolvers`, `GET /resolvers/{name}/{key}` | [0029](0029-resolver-namespaces.md) §6 |
| Edit filters | `/filters/…`, `GET /filters/log` | [0030](0030-edit-filters.md) §9 |
| Constraints | `GET /entity/{id}/constraints`, `GET /constraints/report`, `GET /constraints/counts`, `POST /constraints/recheck` | [0031](0031-property-constraints.md) §6 |
| SPARQL Update stream | `GET /updates/stream`, `GET /updates`, `GET /updates/cursor`, `/dumps/updates/`: keeps a triplestore current with a dump; public, rate-limited in the `stream` class | [0032](0032-sparql-update-stream.md) §6 |
| SPARQL queries | `GET` and `POST /sparql`, the SPARQL 1.1 Protocol for queries only, over the tenant's dataset, with `Query-Cursor` and `Query-Truncated` headers; `GET /sparql/prefixes`; rate class `query` | [0059](0059-query-service.md) §6 |
| Page statements and categories | `/page/{pageid}/statements`, following the Wikibase REST API's statement routes; `GET /page/{pageid}/categories`, `GET /category/{title}/members`, `/category-mappings/…` | [0038](0038-page-metadata-and-categories.md) §13 |
| Files | `/file/{title}` and its versions and usage, `GET /blob/{sha256}/references`; `/takedowns` and `POST /expunge` at the farm base | [0039](0039-files-and-media.md) §17 |
| Content models | Page summaries carry `content_model`, and File page summaries `mediainfo_id` | [0041](0041-content-models.md) §9 |
| Rendering | `GET /page/{id}/render`, the render manifest; `GET /page/{id}/html` with `as_of=revision`, rendering a revision with its dependencies as of the revision's offset | [0042](0042-template-expansion-and-parsoid.md) §12, §14 |
| Tables | `GET /table/{pageid}/rows`, `POST /table/{pageid}/rows/{id}`, `GET /table/{pageid}/export` | [0045](0045-table-content-model.md) §10 |
| Scopes | `GET /scope/{pageid}`, `GET /scope/{pageid}/members`, `GET /scope/{pageid}/export`, `POST /scope/{pageid}/refresh`, `GET /subject/{kind}/{id}/scopes`; in the Action API `list=scopemembers` and `rcscope` | [0060](0060-scopes.md) §8 |
| Sprints | `GET /sprint/{pageid}`, `/tasks`, `/leaderboard`; `POST /sprint/{pageid}/tasks/{rule}/{kind}/{id}/claim` and `/release`; `GET /subject/{kind}/{id}/tasks`; in the Action API `list=sprinttasks` and `letype=task` | [0061](0061-sprints-and-tasks.md) §10 |
| Workspaces | `POST /scope/preview` (count, sample and shared statements for an unsaved definition; rate class `query`), `GET /scope/{pageid}/facets`, `GET /workspace/{pageid}`, `POST /workspace` (create from a kit), `GET /kits` | [0062](0062-workspaces.md) §8 |
| Query pages | `GET /query/{pageid}/run?{param}=…` with `/sparql`'s headers and formats plus the labelled table; `GET /query/{pageid}/params` | [0063](0063-query-namespace.md) §7 |
| EntitySchema | `GET /entity/E1`; `GET /schema/{id}/report`, `POST /schema/{id}/check`, `GET /entity/{id}/schemas`; Action API `wbgetentities` and the term modules on `E`, `wbsearchentities&type=entityschema`, `action=entityschema` | [0064](0064-entityschema-and-validation.md) §7 |
| MediaInfo | `wbsetlabel`, `wbsetdescription` and `wbeditentity` terms on `M` accepted; `wbgetentities` on `WDM`; `GET /file/{title}/mediainfo` | [0065](0065-mediainfo-captions-and-commons.md) §4 |
| Lexemes | WikibaseLexeme's modules (`wbladdform` … `wblmergelexemes`), `wbeditentity` with `new=lexeme`, `wbsearchentities&type=lexeme|form|sense`; REST `GET /entity/L1` and `GET /entity/L1-F1` | [0066](0066-lexemes.md) §8 |
| Proposals | `POST /proposal`, `GET /proposal/{thread}`, `POST /proposal/{thread}/export`, `/push`, `/withdraw`, `GET /entity/{id}/proposals`, `GET /page/{id}/proposals`; `list=proposals`, `letype=proposal` | [0067](0067-proposals.md) §7 |
| Merging with upstream | `GET /page/{id}/merge-preview?direction=`, `POST /page/{id}/merge`, `POST /page/{id}/refollow`; `tsmerge` on `action=edit` | [0068](0068-merging-with-upstream.md) §6 |
| Followed talk pages and pins | Foreign threads in `GET /page/{id}/threads` (`origin=`) and `GET /thread/{id}`; `POST /thread/{id}/upstream-reply`, `POST /page/{id}/upstream-topic`, `GET /upstream-post/{id}`; `POST /thread/{id}/pin`, `/unpin`; `POST /page/{id}/talk-follow`; `discussiontoolsedit` and `discussiontoolspageinfo` on followed talk titles, `thorigin` on `list=threads`, `ts-upstream-talk` | [0069](0069-synchronized-talk-pages.md) §9 |
| Redirects | `redirect_to` in page summaries; `redirected_from` from `GET /resolve` | [0051](0051-page-redirects.md) §7 |
| Page repositories | `GET /page/{id}` with a provider-ranged ID, carrying `origin`, `upstream` and `stack`; `GET /page/stack/{title}` | [0052](0052-page-repositories-and-title-inheritance.md) §6 |
| Mirrored pages | `GET /page/{id}/html` and `/render` for an inherited page; `GET /repo/{name}` | [0053](0053-mirrored-pages.md) §10 |
| Forks | `POST /page/fork`, `GET /page/{id}/fork`, `GET /page/{id}/upstream-diff`, `POST /page/{id}/fork/files`; the fork's origin in `GET /page/{id}/provenance` | [0054](0054-forking-a-mirrored-page.md) §11 |

### 6. Fetching upstream edits live

*Changed by A4, A13.*

- **The server fetches, not the client.** It uses the API endpoint in the provider registry, with a proper User-Agent and `maxlag` ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.8).
- **The interval** runs from `prev_upstream` to the upstream version of the sync (§2.2).
- **Rows** are `upstream-edit` activity rows. Actors are identified by their upstream IRIs ([0007](0007-actor-identity.md) §2), summaries are parsed as in [0001](0001-revision-metadata-rdf.md), and an actor linked to a local account carries a `linked` flag.
- **Results are cached briefly and rate-limited,** per user and per instance, in the `upstream` rate-limit class ([0024](0024-subsidiary-accounts.md) §5). They are never written to the log.
- **Errors:**

| Error | When |
|---|---|
| `upstream-unavailable` | HTTP 503 with `Retry-After`, when the provider cannot be reached |
| `upstream-unsupported` | The provider publishes no history, as OpenAlex does not |
| `upstream-truncated` | The interval holds more edits than the cap. The first rows are returned, with a link to the provider's history. |

- **Retained and fully mirrored entities** are served from the log instead ([0002](0002-source-graphs-and-mass-ingest.md) §8.3), and their rows are marked as stored. They are read from the upstream revision records ([0015](0015-record-format-and-partition-registry.md) §4), as `upstream-edit` rows with `record` present; the live fetch covers only the interval those records do not.

### 7. Structured diffs

A diff response holds:

- **The change set's operations,** as stored.
- **Before and after, as statement groups.** For each property the change touched, the group in canonical JSON before and after the change, with the neighbouring values for context. "Before" is the source graph's state just before the record. Terms and sitelinks are returned as keyed lists.
- **An effect block,** in three parts:
  - `resolved`: the groups and values that changed in the resolved view;
  - `truthy`: the truthy triples added and removed;
  - `upstream`: whether anything was written upstream, and the correction's state.
- **An integrity block:** the record's coordinates and the checkpoint that includes it.

**The effect is computed against the other sources as they are now,** and labelled that way. Their earlier states may have been compacted, so the resolved view at the time of the change cannot, in general, be rebuilt.

**For a sync,** the diff comes from `delta` where it was stored. Otherwise it comes from `changes`, with a link to the provider's own diff (§2.2).

### 8. Redaction, caching and permissions

*Changed by A3, A5, A12, A13, A24, A35.*

**Redaction.**

- Administrators' views include hidden fields, read from the log ([0001](0001-revision-metadata-rdf.md) §4).
- Everyone else gets the redacted form.
- Erased bodies are never returned to anyone. They no longer exist.
- A target under a **confidential** `read` restriction the viewer does not satisfy is **absent**, not redacted: every route answers exactly as for a target that does not exist, lists and counts omit it, and a log event on it is omitted with it ([0056](0056-security-model.md) §5).

**Caching.**

- Anonymous responses carry `ETag` and may be cached publicly, for a minute at a shared proxy; only erasure purges them ([0014](0014-caches-and-search.md) §5–6).
- Authenticated responses are `private, no-cache`, with `ETag`, and never enter a shared cache ([0014](0014-caches-and-search.md) §1, §6).
- Routes under `/account` and `/auth` are `no-store`.

**Rights.** Rights are named here. Which groups hold them is [0016](0016-permissions-and-access-control.md) §2's, and `docs/registry/groups.toml` is the registry of record.

- MediaWiki's own rights are reused where they exist: `deletedhistory`, `suppressrevision`, `deletelogentry`.
- New rights:
  - `ts-retain`, `ts-convert`;
  - `ts-runjob`, `ts-revertjob`, `ts-viewrejects`;
  - `ts-erase`, `ts-viewerasures`.
- `meta=userinfo&uiprop=rights` reports them.

**Privacy test.** An automated test checks two things:

- No route returns data from a `private` graph, except `/account` and `/auth/pending` in the holder's own session.
- No internal-only graph appears in any export.

Later ADRs add cases:

- No cache entry or search document holds a hidden field or private data ([0014](0014-caches-and-search.md) Consequences).
- Nothing behind a `read` ACL reaches a viewer outside its group, from any route, cache or index ([0023](0023-moderation.md) §8).
- No route returns an API key's hash, and only the issuing `POST` returns its secret ([0024](0024-subsidiary-accounts.md) §8).
- No route returns the URL, hash or metadata of a file version the viewer may not read ([0039](0039-files-and-media.md) §17).
- The cases of [0056](0056-security-model.md) §15: a confidential target is byte-for-byte a missing target to every principal outside its group, from every route, list, feed, index, cache entry, export, include and notification; and `instance check` fails on each misconfiguration 0056 §10 names.

The test is backed by database grants: no query outside the holder-only services can read `private` ([0013](0013-postgres-storage.md) §4).

This makes the Public and Private labels in the UI ([0010](0010-site-ui.md) §1.6) enforceable.

### 9. Crates

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks; `triplespace-activity` was absorbed by `triplespace-projections`. The table this section first gave is in A1.

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
