# 0035. Adopting an existing Wikibase as a tenant

- **Status:** Proposed
- **Date:** 2026-09-28
- **Updated:** 2026-10-09 (A9)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md)
- **Uses:** [0024](0024-subsidiary-accounts.md), [Wikibase contract](../api/wikibase-compat.md)
- **Chapters:** [05](../architecture/05-providers-and-ingest.md), [07](../architecture/07-actors-and-accounts.md), [16](../architecture/16-logs-feeds-and-notifications.md), [22](../architecture/22-crates-and-stack.md)

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

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.1, §8.3.*

### 2. Preconditions and the job

*Changed by A2.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.2.*

### 3. The `adopt` operation (amends 0002 §8.2 and §8.5)

*Changed by A4, A6, A7.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.2, §3.9, §7.3.*

### 4. Sequence floors (extends 0013 §6; amends 0015 §2 and 0007 §3)

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.4.*

### 5. Accounts (amends 0007 §3 and 0016 §3; uses 0018 §4 and §10)

*Changed by A5, A8.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.5.*

### 6. Provenance and log events (amends 0011 §6.1; extends 0012 §5)

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.2, §2.3.*

### 7. History: shape fixed, content deferred

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.5, §8.3.*

### 8. Crates (amends 0005 §2 and §7)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2, §3.1.*

## Consequences

- **Librarybase keeps every number.** Entity, page, revision, log and user IDs survive, and every external citation of `Q6`, `pageid=12` or `oldid=41877` keeps its meaning.
- **Adoption is a one-way door.** The source must be read-only before the final dump, and afterwards the wiki changes only through edits here. There is no second adoption and no sync.
- **Floors leave gaps.** The first entity created after adoption takes a number above the source's counter, not above its highest surviving ID. That is the same rule Wikibase itself follows.
- **Adopted accounts exist without holders.** Every source user has a tenant account and a contributions page from the day of adoption, and none can log in until reclaimed. This is the same state a moved tenant's accounts are in under [0018](0018-tenants.md) §10.
- **Current-state adoption forecloses in-order history** (§7). A tenant that wants its history in `local`, revision by revision, must adopt with the history dump in hand.
- ~~**The log-granularity residue of 0005 narrows.** `adopt` is the whole-entity write to the local graph; whether an ordinary edit may replace an entity against a base revision remains open there.~~ *`create-or-add` with `overwrite` and a base revision replaces an entity; `adopt` is the one whole-entity write that needs no base revision (A6).*

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

### A5. Hidden source accounts get the `actor` ACL

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5
- **Summary:** For a source account whose name is hidden, adoption writes the `actor` ACL restricted to `suppress` beside the actor record, so that `hidden` has one origin, the ACL of [0007](0007-actor-identity.md) §4, on an adopted tenant as on any other. (PENDING C2)

### A6. `adopt` needs no base revision

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §3
- **Summary:** `adopt` is "the one whole-entity write that needs no base revision", not the one whole-entity write to the local graph: `create-or-add` with `overwrite` and a base revision still replaces an entity whole ([0002](0002-source-graphs-and-mass-ingest.md) §8.2). The consequence that said the residue of 0005 narrows is struck. (PENDING E4)

Replaced text (§3):

> This also narrows the residue [0005](0005-crate-organization.md) leaves open on log granularity: `adopt` is the one whole-entity write to the local graph, and it is confined to adoption jobs.

### A7. Lexemes are adoptable

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §3
- **Summary:** Strike "once the Lexeme namespace is implemented": `L` is adoptable, since [0066](0066-lexemes.md) decided lexemes. (PENDING E7)

Replaced text (§3):

> Any minted entity type the source has may be adopted (`Q`, `P`, and `L` once the Lexeme namespace is implemented).

### A8. `reclaim/reclaim` is a log event

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5
- **Summary:** The `reclaim/reclaim` event §5 names is a row of [0011](0011-logs.md) §6.1's table, beside `takedown/expunge` and the no-redirect move: it projects from the reclaiming binding record in the tenant `log` and is visible to everyone. (PENDING E27)

### A9. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [05](../architecture/05-providers-and-ingest.md), [07](../architecture/07-actors-and-accounts.md), [16](../architecture/16-logs-feeds-and-notifications.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
