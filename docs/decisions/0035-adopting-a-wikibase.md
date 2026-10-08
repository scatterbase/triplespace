# 0035. Adopting an existing Wikibase as a tenant

- **Status:** Proposed
- **Date:** 2026-09-28
- **Updated:** 2026-10-08 (A4)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md)
- **Uses:** [0024](0024-subsidiary-accounts.md), [Wikibase contract](../api/wikibase-compat.md)

## Context

The first deployment is Librarybase: a MediaWiki+Wikibase with about 350,000 entities whose `Q` and `P` numbers are cited outside it, in URLs, in citation data and, once other tenants read it, as `LBQ` and `LBP` ([0018](0018-tenants.md) §5); whose user IDs attribute its history; and whose revision IDs are in `oldid=` permalinks.

[0018](0018-tenants.md) §1 and §5 settle what those numbers *mean* on Triplespace: the tenant's `local` partition is the source of provider `LB`; `Q6` at home is `LBQ6` to every other tenant and instance; the rewrite happens on read; a provider the instance itself hosts has no mirror partition and no sync job. Nothing here changes that.

What no ADR gives is a way to *write* `Q6` into the tenant's `local` partition with that number. The three ways into a local partition all allocate:

- the editor path, where `wbeditentity` with `new=item` mints the next ID;
- bulk `create` ([0002](0002-source-graphs-and-mass-ingest.md) §8.5), where the server allocates IDs and the caller names new entities by temporary refs and match keys;
- wholesale replacement, allowed only against an explicit base revision ([0002](0002-source-graphs-and-mass-ingest.md) §8.2), which a first load has no base for.

[0018](0018-tenants.md) §10 moves a tenant between two Triplespace instances, as an export bundle whose headers already carry every ID; a MediaWiki wiki has no log, no headers and no key chain. [0008](0008-namespaces-and-document-pages.md) §9 imports *pages* from a MediaWiki XML export with full history and original attribution, and says nothing about entities.

One alternative needs no new operation and was rejected: load the old wiki into `mirror/librarybase` as if it were any foreign Wikibase, and rely on the read rewrite of 0018 §5 to show `LBQ6` as `Q6` at home. It puts the wiki's own data in a mirror partition, which 0018 §5 says a hosted provider tenant never has; every later edit becomes an overlay on a mirror of oneself; and it still needs a materialize-into-local step, which is the operation this ADR defines under another name.

## Decision

### 1. Adoption (extends 0018 §1 and §5)

A tenant **adopts** a MediaWiki Wikibase when the tenant *is* that wiki, continuing on Triplespace: the same identity, the same IDs, the same accounts, usually the same host. Adoption writes the source's entities into the tenant's `local` partition under the IDs the source minted, its accounts under their own numbers, and sets every ID sequence past the source's.

Adoption is neither of the two things it resembles:

- **Not mirroring.** The source is not a foreign provider to the tenant. Nothing is rewritten to a prefixed form, no `mirror/*` partition is created, and no version cursor is kept. If the tenant is a provider to others (`LB`), an adopted `Q6` is `LBQ6` to them from the moment it lands, by [0018](0018-tenants.md) §5, with nothing further to do.
- **Not a move** under [0018](0018-tenants.md) §10. There is no bundle to verify and no key chain to continue. The adopted records are new records of this instance, attested by this instance.

The tenant's `tenant` config record ([0018](0018-tenants.md) §3) gains an **`adopted_from`** field: the source wiki's base URL and the date of adoption. If the host changes, an `alias` record ([0018](0018-tenants.md) §9) is written as for any base-URI change; if Triplespace replaces MediaWiki at the same host, nothing is.

### 2. Preconditions and the job

*Changed by A2.*

An adoption is a job ([0002](0002-source-graphs-and-mass-ingest.md) §8.3, [0011](0011-logs.md) §6.3) with mode `adopt`, run by a subsidiary of the tenant's operators ([0024](0024-subsidiary-accounts.md) §6). It is refused unless:

1. **the source is frozen.** The operator declares the source read-only and takes the final dump; adoption is a one-way door (§3), and anything edited on the source after the dump is lost;
2. **the tenant's slug is the issuer** whose numeric IDs the source's users become ([0018](0018-tenants.md) §4), which is to say the tenant was created for this source;
3. **the tenant's `local` partition holds no entity records** other than those written by earlier adoption jobs for the same source. This is what makes a failed adoption resumable (§3) and everything else an ordinary edit;
4. **the job's operator holds `ts-runjob` and `ts-config`,** since the job seeds sequences (§4).

The job record carries the source's base URL, the dump's identity and date as the source version, the adapter version, the floors it set (§4), and the counts by outcome. It projects as `job/start` and `job/finish` like any job. Adopting a wiki with files imports every file version from its file tables and upload directory, deleted versions included, onto file pages that keep their source page IDs ([0039](0039-files-and-media.md) §14).

### 3. The `adopt` operation (amends 0002 §8.2 and §8.5)

*Changed by A4.*

One operation is added to the table of [0002](0002-source-graphs-and-mass-ingest.md) §8.2:

| Operation | Graph | Meaning |
|---|---|---|
| `adopt` | Local | Writes an entity's whole state as its first local record, under the ID the source minted. Accepted only from an adoption job (§2). An entity already present is skipped when the content hash matches and rejected otherwise; `adopt` never updates. |

```
{"job":{"source":"https://librarybase.org/","version":"librarybase-20260928.json.gz","mode":"adopt","graph":"local"}}
{"op":"adopt","id":"Q6","entity":{…},"source_revid":41877,"source_time":"2026-09-20T14:02:11Z","source_pageid":12}
{"op":"adopt","id":"P12","entity":{…},"source_revid":903,"source_time":"2024-01-03T09:15:40Z","source_pageid":40}
```

- **`id`** is in local form ([0017](0017-entity-id-grammar.md) §1): the source's own ID, with no prefix. Any minted entity type the source has may be adopted (`Q`, `P`, and `L` once the Lexeme namespace is implemented).
- **`entity`** is the source's canonical JSON ([wikibase-compat.md §3](../api/wikibase-compat.md)). IDs the source wrote with an entity-source prefix, such as `wikidata:Q42` from a federated install ([wikibase-compat.md §5.1](../api/wikibase-compat.md)), are rewritten to the provider form, `WDQ42`, by the rule of [0002](0002-source-graphs-and-mass-ingest.md) §4, using the source's own entity-source table. A prefix whose entity source is not a registry provider becomes an entity source of the tenant under the same name, declared by the job before the first `adopt` from that table, and its IDs are kept as written, `mhc:Q1`; if the name is taken, the preconditions (§2) fail and name it ([0078](0078-entity-sources.md) §11). Every other ID is kept as written: the source's `Q` numbers *are* the tenant's.
- **`source_revid`, `source_time`, `source_pageid`** are the source's revision ID and timestamp for the adopted state and the page ID of the entity's page on the source. They are content, kept in the content part, and they give the record its provenance (§6) and its page ID (§4).
- **Statement GUIDs** are kept as the source wrote them. They already carry the entity's own ID as their prefix, which is the invariant [0018](0018-tenants.md) §7 preserves.

The record is an ordinary local revision: it takes the next revision ID from the tenant's sequence ([0015](0015-record-format-and-partition-registry.md) §2), its comment part is empty, and its attestation names the job's subsidiary and the job ([0015](0015-record-format-and-partition-registry.md) §1). The entity's `lastrevid` after adoption is that new local revision ID; `source_revid` is shown beside it as "adopted from revision N".

**Why a new operation.** `create` allocates an ID, merges statements into an existing entity and takes match keys; `adopt` does none of these. `put` is a mirror operation with skip-if-not-newer semantics and the `prev_upstream` fields of [0012](0012-api-requirements.md) §2.2; none of that applies to a first and only write. A separate name keeps the table of 0002 §8.2 honest and gives the permission and filter layers ([0016](0016-permissions-and-access-control.md), [0030](0030-edit-filters.md)) something to name. This also narrows the residue [0005](0005-crate-organization.md) leaves open on log granularity: `adopt` is the one whole-entity write to the local graph, and it is confined to adoption jobs.

**Re-running** an adoption job against the same dump skips every entity already present and is therefore how a partial adoption is completed. A newer dump is not a second adoption: once adopted, the wiki changes through edits.

### 4. Sequence floors (extends 0013 §6; amends 0015 §2 and 0007 §3)

Every sequence the source consumed is set past what it consumed, so that no number the source ever allocated is allocated again here. The floors are recorded in the job record. Setting a floor never lowers a sequence.

| Sequence | Floor | Why |
|---|---|---|
| **Entity IDs**, one per minted entity type per tenant (`log.item_id`, `log.property_id`, …), taken in the appending transaction as Wikibase's `wb_id_counters` are; the ID blocks of [0002](0002-source-graphs-and-mass-ingest.md) §8.5 reserve ranges from them. [0013](0013-postgres-storage.md) §6 did not list them and now does | The source's own counters, supplied by the operator from its `wb_id_counters` table (`--counters`); failing that, the highest ID in the dump, with a warning | Numbers the source allocated and then deleted must not be reused, and a dump does not show them |
| **Page IDs** (`log.page_id`). The `adopt` record carries `source_pageid` in header field 9 in place of a freshly taken number; [0015](0015-record-format-and-partition-registry.md) §2's rule that the ID is "taken from one sequence the first time a key is written" gains "or supplied by an adoption job" | The source's highest page ID | Clients cite `pageid`; document pages imported later from the same wiki under [0008](0008-namespaces-and-document-pages.md) §9 keep theirs the same way |
| **Revision IDs** (`log.revision_id`). Adopted records take fresh IDs above the floor | The source's highest revision ID | No local revision ever shares a number with a source revision, so `oldid={source revid}` resolves to the source revision once history is imported (§7) and to nothing before, never to an unrelated local revision |
| **Log IDs** (`log.log_id`) | The source's highest log ID | The same, for `logid` |
| **User IDs** ([0007](0007-actor-identity.md) §3: "sequential, start at 1") | The source's highest user ID | Source accounts keep their numbers (§5) |

### 5. Accounts (amends 0007 §3 and 0016 §3; uses 0018 §4 and §10)

**Source accounts become tenant accounts by number.** The tenant is the source's issuer ([0018](0018-tenants.md) §4), so the source's user 42 is `{slug}:42`. Adoption writes an actor record ([0007](0007-actor-identity.md) §4) for every account in the source's user list: kind `registered`, the current name, status `active`, and no binding. A source account whose name is hidden or that was vanished is written with the corresponding status and no name. Source group memberships become membership records ([0016](0016-permissions-and-access-control.md) §3), with one exception: a source bot account has no structural operator ([0024](0024-subsidiary-accounts.md) §1), so it is adopted as an ordinary account without the `bot` group, keeps its history, and its operator, once reclaimed, creates a subsidiary in its place.

**Adopted accounts cannot log in until reclaimed.** The source has no binding digest, so reclaiming is the manual path of [0018](0018-tenants.md) §10: a bureaucrat records a `reclaim/reclaim` log event, or, while the source wiki is still up, the source vouches by acting as an identity provider for a grace period. An account never reclaimed keeps its name and its history, exactly as after a move.

**The owner.** `triplespace-cli instance create` creates `local:1` as the sole member of `owner` ([0016](0016-permissions-and-access-control.md) §3). On an adopting tenant, `instance create --adopt {source base} --owner {user_id}` creates the owner under the source's user ID instead, with the same password binding, sets the user-ID floor from the source, and writes `adopted_from` (§1); the adoption job then finds the owner's actor record already present and leaves it. "`local:1`" in 0016 §3 reads as "the owner account" on such a tenant.

### 6. Provenance and log events (amends 0011 §6.1; extends 0012 §5)

- **An adopted entity was not created here.** Its first record projects as `import/upload` (from a dump) or `import/interwiki` (from the live API), never as `create/create`; this adds a row to the table of [0011](0011-logs.md) §6.1, beside the page import of [0008](0008-namespaces-and-document-pages.md) §9, and `list=logevents&letype=import` lists adopted entities as MediaWiki's own import log lists imported pages. The activity row ([0012](0012-api-requirements.md) §3) is an `edit` with `new` set, the `bot` flag and the `job:{id}` tag of [0012](0012-api-requirements.md) §4, and a `adopt` tag.
- **The metadata graph continues the wiki's history rather than starting it.** The adopted record's revision node carries `prov:wasRevisionOf <{base}/revision/{source_revid}>` — the source revision under the tenant's own base, which is the permalink the wiki always served — and `pav:importedOn`. This is the continuity rule [0002](0002-source-graphs-and-mass-ingest.md) §5 gives a rescued entity, applied to the wiki's own past. Until history is imported (§7) the IRI dereferences to nothing, and `oldid=N` for an N below the revision floor answers that revision N predates adoption and its history is not held, not that it does not exist.
- **`GET /entity/{id}/provenance`** ([0012](0012-api-requirements.md) §5) reports who minted the ID as this wiki, before Triplespace, with the adoption job and `source_revid`; `GET /page/{pageid}/provenance` does the same for adopted pages.

### 7. History: shape fixed, content deferred

This ADR adopts **current state only**. What fixes the shape of a later history import is a constraint, not a preference: a later job must not displace the current state. `entity_source.offset` is the entity's newest record by offset ([0013](0013-postgres-storage.md) §5.1) and the base-offset check reads the newest record for the key ([0006](0006-log-integrity-and-erasure.md) §8), so past revisions cannot be appended into `local` behind the current one. Two shapes satisfy it:

- **(a) History first, in order.** Each source revision is an `adopt` record with an explicit revision ID (the source's, below the floor), its original timestamp and its original actor, as [0008](0008-namespaces-and-document-pages.md) §9 steps 1–2 do for pages, and the last revision *is* the current state. It needs the XML history dump at adoption time, and it makes adoption one job.
- **(b) History after, as backfill.** Source revisions become `upstream-revision`-shaped records ([0015](0015-record-format-and-partition-registry.md) §4) in the tenant's own `log` partition, keyed by entity, metadata first and content where wanted; the history view folds them as it folds upstream history, and `oldid=N` serves them. It works after a current-state adoption and never touches `local`.

Adopting current state only forecloses (a) for that tenant. Librarybase accepts that: its first milestone is API-only and current-state, and (b) is then its path. The choice between the two for tenants that have not yet adopted is open (below).

### 8. Crates (amends 0005 §2 and §7)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed, and 0005 §7 places the adoption job in build step 4 beside the mirror bootstrap. The table this section first gave is in A1.

## Consequences

- **Librarybase keeps every number.** Entity, page, revision, log and user IDs survive, and every external citation of `Q6`, `pageid=12` or `oldid=41877` keeps its meaning.
- **Adoption is a one-way door.** The source must be read-only before the final dump, and afterwards the wiki changes only through edits here. There is no second adoption and no sync.
- **Floors leave gaps.** The first entity created after adoption takes a number above the source's counter, not above its highest surviving ID. That is the same rule Wikibase itself follows.
- **Adopted accounts exist without holders.** Every source user has a tenant account and a contributions page from the day of adoption, and none can log in until reclaimed. This is the same state a moved tenant's accounts are in under [0018](0018-tenants.md) §10.
- **Current-state adoption forecloses in-order history** (§7). A tenant that wants its history in `local`, revision by revision, must adopt with the history dump in hand.
- **The log-granularity residue of 0005 narrows.** `adopt` is the whole-entity write to the local graph; whether an ordinary edit may replace an entity against a base revision remains open there.

## Open questions

- **Q1. History for tenants that have not yet adopted** (§7): whether (a) or (b) is the default, or whether both are offered as modes of one job.
- **Q2. Pages in the same job.** Whether an adoption should carry the source's document pages ([0008](0008-namespaces-and-document-pages.md) §9) in the same run, sharing the page-ID floor, or leave them to a second job.
- **Q3. Vouching.** Whether a still-running source wiki can act as an identity provider for reclaiming, which needs an OAuth issuer on the source, or whether the manual `reclaim/reclaim` path is the only one a MediaWiki source offers.
- **Q4. Sitelinks.** A source's `wb_items_per_site` rows are site IDs and titles; they become URLs through the site aliases of [0026](0026-sitelinks.md) §2, and a site ID the registry does not know is a reject. Whether adoption should register unknown site IDs as aliases on the fly is open.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §8.2, §8.5 | §3 | extends | 0002 A15 |
| [0005](0005-crate-organization.md) §2, §7 | §8 | extends | 0005 A34 |
| [0007](0007-actor-identity.md) §3 | §4–5 | extends | 0007 A12 |
| [0008](0008-namespaces-and-document-pages.md) §9 | §4 | extends | 0008 A7 |
| [0011](0011-logs.md) §6.1 | §6 | extends | 0011 A12 |
| [0012](0012-api-requirements.md) §5 | §6 | extends | 0012 A22 |
| [0013](0013-postgres-storage.md) §6 | §4 | extends | 0013 A12 |
| [0015](0015-record-format-and-partition-registry.md) §2 | §4 | amends | 0015 A15 |
| [0016](0016-permissions-and-access-control.md) §3 | §5 | amends | 0016 A12 |
| [0018](0018-tenants.md) §1, §3, §5 | §1 | extends | 0018 A5 |

## References

- [0018 — Tenants](0018-tenants.md) §5 (a tenant as a provider) and §10 (moving a tenant)
- [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md) §9 (importing pages)
- [Manual:Importing XML dumps](https://www.mediawiki.org/wiki/Manual:Importing_XML_dumps) and [Manual:Log actions](https://www.mediawiki.org/wiki/Manual:Log_actions) (`import/upload`, `import/interwiki`)
- [Wikibase `wb_id_counters`](https://www.mediawiki.org/wiki/Wikibase/Schema/wb_id_counters)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-28
- **Source:** [0005](0005-crate-organization.md) §2, §7
- **Change:** supersedes §8
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed; its §7 build order places the adoption job (0005 A34).

Replaced text (§8):

> | Crate | Change |
> |---|---|
> | `scatter-wikibase-changeset` | The `adopt` operation and its source fields (§3) |
> | `scatter-ingest` | The adoption job: preconditions, idempotent resume, floors, actor records (§2, §4, §5) |
> | `scatter-adapter-wikidata` | Adoption mode: reading a MediaWiki+Wikibase dump or API parameterised by the tenant's own provider entry, emitting IDs in home form and rewriting only the source's entity-source prefixes (§3) |
> | `scatter-log-postgres`, `triplespace-db` | The entity-ID sequences; setting floors; an explicit page ID on append (§4) |
> | `scatter-mwlog` | `import/upload` and `import/interwiki` for entities (§6) |
> | `triplespace-rdf` | `prov:wasRevisionOf` and `pav:importedOn` on adopted records (§6) |
> | `triplespace-cli` | `instance create --adopt --owner`; `adopt` (§2, §5) |
>
> No crate is added. In the build order of [0005](0005-crate-organization.md) §7, the adoption job sits in step 4 beside the mirror bootstrap: a tenant continuing an existing Wikibase needs it before step 5.

### A2. Adoption brings the wiki's files

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §14
- **Change:** extends §2
- **Summary:** Adopting a wiki with files imports every file version from its file tables and upload directory, deleted versions included, onto file pages that keep their source page IDs.

### A3. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §2, §8
- **Summary:** A1–A2 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A2 was a blockquote. The file before conversion is commit `0b26a3a`.

### A4. Unregistered entity sources

- **Date:** 2026-10-08
- **Source:** [0078](0078-entity-sources.md) §11
- **Change:** extends §3
- **Summary:** An adopted wiki's entity-source prefix that names no registry provider is declared as an entity source of the tenant under the same name, and its IDs are kept as written. A prefix whose name the registry already has fails the preconditions. Before this, such a prefix had no rule.
