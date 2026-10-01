# 0012. API requirements for the site UI

- **Status:** Proposed
- **Date:** 2026-09-26
- **Author:** James Hare / Claude Opus
- **Amended by:** [0013 — Postgres as the log store and serving model](0013-postgres-storage.md), [0014 — Cache layers and search](0014-caches-and-search.md), [0005 — Crate organization, revision of 2026-09-26](0005-crate-organization.md) (§2 crate names), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md), [0016 — Permissions and access control](0016-permissions-and-access-control.md), [0017 — Entity ID grammar](0017-entity-id-grammar.md), [0018 — Tenants](0018-tenants.md), [0019 — Discussions](0019-discussions.md) (§9 extends §4 and §5: thread routes and the `ts-structured` error), [0020 — Change feeds](0020-change-feeds.md) (settles the change-feed open question; §5 extends §4 and §5), [0021 — Notifications](0021-notifications.md) (§7 extends §4 and §5), [0023 — Protection, deletion, hiding and patrolling](0023-moderation.md) (§8 extends §3, §4 and §5), [0024 — Subsidiary accounts, API keys and rate limits](0024-subsidiary-accounts.md) (§4 and §8 extend §4 and §5: bot-password login for subsidiaries, keys and rate-limit reporting; §5 settles the rate-limit open question), [0026 — Sitelinks are URLs](0026-sitelinks.md) (§7 extends §4 and §5: `wbsetsitelink` by host or URL, sitelink routes), [0027 — Preferences, private state and portability](0027-preferences-and-portability.md) (§6 extends `action=options` in §4 and the account routes of §5 with export and import), [0029 — Resolver namespaces](0029-resolver-namespaces.md) (§6 extends `/resolve` and `wbgetentities` in §4–5), [0030 — Edit filters](0030-edit-filters.md) (§9 extends §4 and §5 with the AbuseFilter-shaped modules and filter routes; §5 gives the activity row's `tags` a source and adds `filters`), [0031 — Property constraints](0031-property-constraints.md) (§6 extends §4 and §5 with `wbcheckconstraints` and the report routes; the provenance response gains `constraints`), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§11 extends §5: `/dumps/local/`, `/.well-known/tlog/`, provider verification), [0025 — The instance as an OAuth server](0025-oauth-server.md) (§9 extends §4 and §5: the OAuth 2.0 endpoints, consumer and authorization routes), [0032 — The SPARQL Update stream](0032-sparql-update-stream.md) (§6 extends §5: `/updates/stream`, `/updates`, `/updates/cursor` and the batch files under `/dumps/updates/`), [0038 — Page metadata, legacy categories and articles](0038-page-metadata-and-categories.md) (§13 extends §4 and §5: page-statement and category routes, the category query modules), [0039 — Files, blob storage and foreign file repositories](0039-files-and-media.md) (§17 extends §4 and §5: the file modules and routes), [0041 — Content models](0041-content-models.md) (§8 extends §4: `defaultcontentmodel`, `contentmodel` for every page, the `mediainfo` slot, MediaWiki's `no-direct-editing` and `changecontentmodel` codes; §9 extends §5: `content_model` on page objects), [0042 — Template expansion and the Parsoid renderer](0042-template-expansion-and-parsoid.md) (§14 extends §4 and §5: expansion modules and routes; §12 extends §5: rendering as of a revision), [0043 — Lua modules](0043-lua-modules.md) (§16 extends §4: entity-usage modules)
- **Related:** [0001 — Revision metadata in RDF](0001-revision-metadata-rdf.md), [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (amends §8.2), [0003 — Statement UI](0003-statement-ui.md) (§5 settles the provenance-response open question), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§9 amends §2), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md), [0007 — Actor identity](0007-actor-identity.md), [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md), [0010 — Site UI](0010-site-ui.md), [0011 — Upstream and local logs](0011-logs.md), [MediaWiki API contract](../api/mediawiki-compat.md)

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

MediaWiki clients depend on numeric IDs from one sequence per wiki. These include `revid`, `parentid`, `oldid`, `baserevid`, `lastrevid`, `logid`, and `fromrev` and `torev` in `action=compare`.

Many parts of this ADR depend on them:

- diff URLs;
- `prop=revisions`;
- `list=recentchanges`;
- `list=logevents`;
- edit conflicts;
- the core REST history routes.

The mechanism is left to its own ADR, which should be written before any of these modules is implemented. It must meet these requirements:

- **One sequence for revisions and one for log events.** Each spans every partition that holds this instance's revisions or events: `local`, `pages`, `log`, and the actor records that project as log events.
- **IDs are assigned when a record is appended, and stored in the record.** An ID that came from replay order would change whenever projections were rebuilt in a different interleaving.
- **The sequence increases with append order.** Gaps are acceptable.
- **Mirror records get no local revision ID.** They are not revisions of this wiki. They carry the upstream revision ID instead.
- **Erasure does not free an ID.** An erased record keeps its ID, as its header survives ([0006](0006-log-integrity-and-erasure.md) §7).

> **Settled by [0013](0013-postgres-storage.md) §6 and [0015](0015-record-format-and-partition-registry.md) §2.** The sequences are Postgres sequences, one set per tenant ([0018](0018-tenants.md) §2), taken in the appending transaction and written into header fields 7–9. Mirror records *do* carry a revision ID, provider-ranged (`provider_number << 40 | n`), computed by the writer, so that `lastrevid`, `baserevid` and `schema:version` are defined for an entity with no local revision; the requirement above is amended accordingly.

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
| `tags`, `flags` | Change tags; the minor, bot and new flags |
| `size`, `delta` | §2.4 |
| `changes` | For syncs: the summary of §2.2 |
| `correction` | For corrections: the state in [0003](0003-statement-ui.md) §4 (active, redundant or dangling) |
| `visibility` | Which fields are hidden. For `erased` rows, the reason class, if the viewer may see it. |
| `links` | Diff, history, job, and upstream URLs |

### 4. Action API changes

All of these changes are additive (§1.2).

| Module | Requirement |
|---|---|
| `meta=siteinfo` | New `siprop` values:<br>• `providers`: codes, entity types, namespaces, IRI templates, and whether upstream history can be fetched.<br>• `issuers`: the issuers allowed for login, without any secrets.<br>• `triplespace`: capabilities, the REST base path and, since 2026-09-28, `hash_mismatches`, the instance-wide count of upstream hashes kept because the recomputation differed ([0006](0006-log-integrity-and-erasure.md) §2, as amended).<br>Namespaces come from the registry ([0008](0008-namespaces-and-document-pages.md) §1). |
| `prop=revisions`, core REST `/v1/page/{title}/history` | Local revisions only: the local graph for entities, the `pages` partition for document pages. Erased revisions come back with MediaWiki's hidden flags (`texthidden`, `commenthidden`, `userhidden`) and a new `erased` flag. New `rvprop` values: `source`, `job` and `operator`. |
| `list=recentchanges` | One row per local revision, as in MediaWiki. Job changes carry the `bot` flag and a `job:{id}` tag. A new `rcjob` parameter filters by job. Mirror records never appear. Each mirror job run appears as a `job` log event ([0011](0011-logs.md) §6.3). `rctype=external` is not used, because Wikibase clients already give it a meaning. |
| `list=usercontribs` | By local account. Job changes appear with their tags. A new `ucjob` parameter filters by job. |
| `list=logevents` | Serves the events of [0011](0011-logs.md), with the new types `erase`, `link`, `retention`, `convert` and `job`, and the action `renameuser/vanish`. A new `leprovider` parameter selects local events (the default) or a provider's. Redacted per viewer. |
| `list=users` | New `usprop` values: `linkedaccounts` (public links, [0007](0007-actor-identity.md) §7) and `operator` (for bots). The existing `cancreate` checks names at first login. |
| `action=parse` | Accepts the `markdown` and `yaml` content models ([0008](0008-namespaces-and-document-pages.md) §5). Reports the renderer version, so that the in-browser preview can detect a mismatch. |
| `action=compare` | For document pages, as in MediaWiki. For entities, a diff of the canonical JSON of the local graph's assertions: the "JSON" view in [0010](0010-site-ui.md) §6. The statement view is served over REST (§5). |
| `action=edit`, `wbeditentity` and the other write modules | Map `baserevid` to base offsets ([0006](0006-log-integrity-and-erasure.md) §8). An `editconflict` error includes the current revision's ID, so the client can merge a document page three ways. |
| `action=options` | New preference keys: `ts-upstream-fold` and `ts-rc-syncs` ([0010](0010-site-ui.md) §11) |
| `action=clientlogin`, `action=createaccount`, `meta=authmanagerinfo` | Offer a redirect-based OAuth flow for each issuer allowed for login, and MediaWiki's own `username`/`password` fields where the built-in `password` issuer is on ([0007](0007-actor-identity.md) §3, as amended); `action=login` stays the bot-password form of [0024](0024-subsidiary-accounts.md) §4 |
| `action=linkaccount`, `action=unlinkaccount`, `action=removeauthenticationdata` | Manage **bindings**, the private sign-in methods of [0007](0007-actor-identity.md) §3, because that is what these modules mean in MediaWiki. They never create or remove the public account links of [0007](0007-actor-identity.md) §7. Removing the last binding is refused. |
| `action=acquiretempusername` | Temporary accounts ([0007](0007-actor-identity.md) §3) |

> **Extended by [0041](0041-content-models.md) §8.** Every page has a content model: `meta=siteinfo` reports `defaultcontentmodel` where it is not `wikitext`, and `siprop=triplespace` gains `contentmodels`; `prop=info` and `prop=revisions` report a model for every page; `rvslots` takes `main` and, on File pages, `mediainfo`. `action=edit` on a non-text model is refused with `no-direct-editing`, and `action=changecontentmodel` uses MediaWiki's `changecontentmodel-nodirectediting`, `changecontentmodel-cannotbeused` and `nochanges`.

> **Extended by [0042](0042-template-expansion-and-parsoid.md) §14 and [0043](0043-lua-modules.md) §16.** `action=expandtemplates`; expansion, `prop=templates` and the limit report in `action=parse`; `prop=templates`, `prop=transcludedin`, `list=embeddedin` and `list=alltransclusions`; `magicwords`, `functionhooks`, `extensiontags`, `variables` and `doubleunderscores` in `meta=siteinfo`; `inprop=linkclasses`; `meta=allmessages`; and, with Lua, `prop=wbentityusage`, `list=wblistentityusage` and `wikibase_item` in `prop=pageprops`.

### 5. REST routes under `rest.php/triplespace/v0`

**Entities**

| Route | Purpose |
|---|---|
| `GET /entity/{id}/provenance` | The provenance response of [0003](0003-statement-ui.md) §6, plus the data for the identity line: who minted the ID, cluster members and the canonical ID, statement counts by graph, sync state (last sync, upstream version, `first_seen`), retention and history policies, and the backlink count |
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
| `GET /page/{pageid}/provenance` | Import origin, and the revision count by origin: imported, bot or local ([0010](0010-site-ui.md) §4) |

> **Extended by [0038](0038-page-metadata-and-categories.md) §13.** Page statements (`/page/{pageid}/statements`, following the Wikibase REST API's statement routes), categories, category members and category mappings. In the Action API: `prop=categories`, `list=categorymembers`, `prop=categoryinfo` and `list=allcategories`.

**Activity and accounts, public**

| Route | Purpose |
|---|---|
| `GET /activity` | Recent changes as the UI shows them: one row per job, mirror syncs on request, and grouping by page. Filters: `kind`, namespace and period. |
| `GET /actor/{key}` | The public profile: current name, kind, status, linked accounts and operator |
| `GET /actor/{key}/contributions` | Contributions by account, including jobs run on the account's behalf |

**Jobs**

| Route | Purpose |
|---|---|
| `GET /jobs` | Filters: status, graph and actor |
| `GET /jobs/{id}` | The job record and status: counts by outcome, the hash-mismatch count by value type ([0006](0006-log-integrity-and-erasure.md) §2, as amended 2026-09-28), source and adapter versions, and checkpoint. While running: progress and projection lag. For `snapshot` jobs: the sweep count and its threshold. |
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
| `GET /resolve?q=` | Resolves any string to its canonical page through the title resolver ([0008](0008-namespaces-and-document-pages.md) §3). This includes bare IDs such as `WDQ65` and domain keys. |

**Integrity**

| Route | Purpose |
|---|---|
| `GET /record/{partition}/{offset}/checkpoint` | The earliest checkpoint that includes the record |
| `GET /record/{partition}/{offset}/proof?checkpoint=` | An inclusion proof ([0006](0006-log-integrity-and-erasure.md) §9) |

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
| `/oauth/authorize`, `/oauth/token`, `/oauth/device`, `/oauth/revoke`, `/oauth/identify`, `/.well-known/oauth-authorization-server` | The instance's own OAuth 2.0 server: tokens are credentials of a subsidiary, never of the primary account ([0025](0025-oauth-server.md) §1, §9) |
| `/.well-known/tlog/{partition}/checkpoint`, `/dumps/local/`, `GET /providers/{slug}/verification` | What an instance publishes for verified sync, and how a reader reports it ([0022](0022-federation.md) §1, §11) |
| `GET /updates/stream`, `GET /updates`, `GET /updates/cursor`, `/dumps/updates/` | The SPARQL Update stream that keeps a triplestore current with a dump ([0032](0032-sparql-update-stream.md) §6); public, rate-limited in the `stream` class |

When each binding was last used is stored in the operational store, never in the log.

> **Extended by [0039](0039-files-and-media.md) §17.** The file modules (`action=upload`, `action=filerevert`, `prop=imageinfo`, `meta=filerepoinfo` and the rest), the `/file` and `/blob` routes, and the farm-base `/takedowns` routes.

> **Extended by [0041](0041-content-models.md) §9.** Page objects carry `content_model` for every page, and File page summaries carry `mediainfo_id`.

> **Extended by [0042](0042-template-expansion-and-parsoid.md) §12 and §14.** `GET /page/{id}/render` returns a page's render manifest, and `GET /page/{id}/html` takes `as_of=revision` to render a revision with its dependencies as of the revision's offset.

### 6. Fetching upstream edits live

- **The server fetches, not the client.** It uses the API endpoint in the provider registry, with a proper User-Agent and `maxlag` ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.8).
- **The interval** runs from `prev_upstream` to the upstream version of the sync (§2.2).
- **Rows** are `upstream-edit` activity rows. Actors are identified by their upstream IRIs ([0007](0007-actor-identity.md) §2), summaries are parsed as in [0001](0001-revision-metadata-rdf.md), and an actor linked to a local account carries a `linked` flag.
- **Results are cached briefly and rate-limited,** per user and per instance. They are never written to the log.
- **Errors:**

| Error | When |
|---|---|
| `upstream-unavailable` | HTTP 503 with `Retry-After`, when the provider cannot be reached |
| `upstream-unsupported` | The provider publishes no history, as OpenAlex does not |
| `upstream-truncated` | The interval holds more edits than the cap. The first rows are returned, with a link to the provider's history. |

- **Retained and fully mirrored entities** are served from the log instead ([0002](0002-source-graphs-and-mass-ingest.md) §8.3), and their rows are marked as stored.

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

**Redaction.**

- Administrators' views include hidden fields, read from the log ([0001](0001-revision-metadata-rdf.md) §4).
- Everyone else gets the redacted form.
- Erased bodies are never returned to anyone. They no longer exist.

**Caching.**

- Anonymous responses carry `ETag` and may be cached publicly.
- Authenticated responses vary by session.
- Routes under `/account` and `/auth` are `no-store`.

**Rights.** Rights are named here. Which groups hold them is left open.

- MediaWiki's own rights are reused where they exist: `deletedhistory`, `suppressrevision`, `deletelogentry`.
- New rights:
  - `ts-retain`, `ts-convert`;
  - `ts-runjob`, `ts-revertjob`, `ts-viewrejects`;
  - `ts-erase`, `ts-viewerasures`.
- `meta=userinfo&uiprop=rights` reports them.

**Privacy test.** An automated test checks two things:

- No route returns data from a `private` graph, except `/account` and `/auth/pending` in the holder's own session.
- No internal-only graph appears in any export.

This makes the Public and Private labels in the UI ([0010](0010-site-ui.md) §1.6) enforceable.

### 9. Crates

| Crate | Change |
|---|---|
| `triplespace-api-rest` | The `triplespace/v0` routes and the OpenAPI contract |
| `triplespace-api-action` | The additive module changes in §4 |
| `triplespace-api-ingest` | The job routes of §5 |
| `triplespace-activity` ([0010](0010-site-ui.md) §13) | Serves history, activity, contributions and `list=logevents` |
| `triplespace-upstream` (new, surfaces) | The live upstream client: cache, rate limits and the error mapping in §6 |
| `triplespace-accounts` ([0007](0007-actor-identity.md) §10) | The login and account routes |
| `scatter-wikibase-changeset` | The new `put` fields of §2.2 |
| `scatter-ingest` | Computing the change summaries and optional deltas |

## Consequences

- **MediaWiki clients keep working.** Everything that only Triplespace can do sits in a separate namespace. That namespace can change freely while it is `v0`.
- **The UI works against a stock Wikibase too.** It discovers which Triplespace features exist and hides the rest.
- **There are two API families to maintain,** plus an OpenAPI contract that has to be kept in step with the code.
- **Sync ingest does more work.** Each changed entity is compared with its previous mirrored state to produce `changes`, and the `put` record grows by a few fields. Full deltas are opt-in because of their size.
- **Global revision and log IDs block much of this ADR.** They need their own decision before the MediaWiki history and change modules are implemented.
- **Redaction per viewer complicates caching.** Only anonymous responses are cached publicly.
- **Upstream fetches put load on the provider.** The rate limits and the cap in §6 are part of being a good API citizen, not optional tuning.

## Open questions

- ~~**The global ID mechanism** (§2.1): a central allocator, IDs reserved in blocks for each partition, or another scheme.~~ *Settled by [0013](0013-postgres-storage.md) §6 (Postgres sequences per tenant, taken in the appending transaction; blocks from the bootstrap coordinator) and [0015](0015-record-format-and-partition-registry.md) §2 (in the header; provider-ranged for mirror records).*
- **The default for `sync_deltas`,** once ingest cost and storage have been measured.
- ~~**Contributions of foreign actors:** whether `/actor/{key}/contributions` covers upstream edits that were backfilled or fetched live.~~ *Settled by [0018](0018-tenants.md) §8: backfilled and observed records the instance holds; never fetched live.*
- **The effect of a diff:** whether it should be "as of the change" wherever that can be computed, as it can for the local graph alone.
- ~~**A watchlist API,** which waits on the watchlist decision in [0010](0010-site-ui.md).~~ *Settled by [0020](0020-change-feeds.md) §5: MediaWiki's watchlist modules and the `/watchlist` routes.*
- ~~**A change feed for downstream consumers.** Whether Triplespace offers an EventStreams-like stream, and whether it follows the same redaction and log model ([mediawiki-compat.md](../api/mediawiki-compat.md) §7).~~ *Settled by [0020](0020-change-feeds.md) §4: the everything feed as server-sent events, with the same redaction; [0022](0022-federation.md) §1 publishes it for verified sync.*
- **Rate limit values and cache lifetimes** for upstream fetches. *Rate limits: settled by [0024](0024-subsidiary-accounts.md) §5, with `upstream` as one class; its default is a starting value. Cache lifetimes remain a [0014](0014-caches-and-search.md) tuning item.*
- ~~**Which groups hold which rights** (§8).~~ *Settled by [0016](0016-permissions-and-access-control.md) §2, and extended per ADR since; `docs/registry/groups.toml` is the registry of record.*

## References

- [MediaWiki API contract](../api/mediawiki-compat.md)
- [API:Continue](https://www.mediawiki.org/wiki/API:Continue), [API:Etiquette](https://www.mediawiki.org/wiki/API:Etiquette)
- [Manual:User rights](https://www.mediawiki.org/wiki/Manual:User_rights)
- [OpenAPI 3.1](https://spec.openapis.org/oas/v3.1.0)
