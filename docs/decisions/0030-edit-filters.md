# 0030. Edit filters

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A13)
- **Author:** James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md), [0023](0023-moderation.md), [0028](0028-tenancy-policy.md)
- **Uses:** [0007](0007-actor-identity.md), [0022](0022-federation.md), [0024](0024-subsidiary-accounts.md), [0025](0025-oauth-server.md), [0029](0029-resolver-namespaces.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [05](../architecture/05-providers-and-ingest.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

MediaWiki's AbuseFilter lets a wiki write rules that run against every edit before it is saved: a small expression language over variables describing the edit and the editor (`added_lines`, `summary`, `user_age`, `user_groups`, …), a set of actions (log, tag, warn, disallow, throttle, block, degroup), a log of hits, private filters whose rules vandals cannot read, and a way to test a rule against recent changes. Its name undersells it: wikis use it to catch typos, enforce formatting, tag categories of change and slow down new accounts as much as to stop abuse. This ADR calls it an **edit filter**, and keeps MediaWiki's `abusefilter-*` right and module names only where compatibility needs them.

On a Wikibase, AbuseFilter is awkward for a reason that does not apply here. It sees an edit as **text**: `new_wikitext` on Wikidata is a JSON serialization of the entity, and filter authors end up matching serialized JSON with regular expressions. Triplespace never has text where structure exists. Every write is a typed change set ([0002](0002-source-graphs-and-mass-ingest.md) §8) or a typed page or thread operation ([0008](0008-namespaces-and-document-pages.md) §4, [0019](0019-discussions.md) §1), and every write already passes through one place, the appending transaction ([0013](0013-postgres-storage.md) §7), where a base-offset check and the ACLs of [0023](0023-moderation.md) run before anything is appended.

James named two dimensions, **abusive statements** and **abusive page contents**. They turn out to be two **contexts** over one engine. [0016](0016-permissions-and-access-control.md) left abuse filters open as "neither permissions nor ACLs", and [0028](0028-tenancy-policy.md) asked where farm-wide filters would live. This ADR answers both.

## Decision

### 1. A filter is a rule with actions, evaluated before append

*Changed by A13.*

*Current text: [09](../architecture/09-security-and-moderation.md) §7.1.*

### 2. Three contexts and one actor

*Changed by A2, A3, A4, A6, A8, A9, A10, A11, A13.*

*Current text: [09](../architecture/09-security-and-moderation.md) §7.2.*

### 3. The rule language is CEL

*Current text: [09](../architecture/09-security-and-moderation.md) §7.3.*

### 4. Actions

*Changed by A3.*

*Current text: [09](../architecture/09-security-and-moderation.md) §7.4.*

### 5. Records: filters, hits and tags (extends 0011 §2, §6.1; amends 0015 §1)

*Changed by A3, A11, A13.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §7.5, §7.6; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 6. The filter log is a feed (extends 0020 §2)

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.2, §4.6.*

### 7. Bulk jobs (extends 0002 §8.5)

*Changed by A13.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.6.*

### 8. Farm-wide filters (extends 0028 §1)

*Changed by A5.*

*Current text: [09](../architecture/09-security-and-moderation.md) §7.7.*

### 9. API (extends 0012 §4 and §5)

*Current text: [18](../architecture/18-api.md) §1.5, §2.3, §3.1, §3.2.*

### 10. UI (extends 0010 §2 and §7)

*Current text: [19](../architecture/19-site-ui.md) §1.5, §3.9, §6.5.*

### 11. Storage (extends 0013 §5.6 and §7)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.13, §4.16, §5, §6.1, §6.2, §12.2, §12.3.*

### 12. Permissions (extends 0016 §2; settles 0016 Q5)

*Changed by A11.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §7.8.*

### 13. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **Rules see structure, not serialized JSON.** A Wikidata-style vandalism rule on descriptions, values or identifiers is a one-line CEL expression over typed variables, which is what AbuseFilter could never offer a Wikibase.
- **Two dimensions, one engine.** Statements and page text differ only in context; actions, records, hits, testing, rights and the API are shared.
- **Every hit is a record and every filter has history**, so the filter log is a feed, filters can be audited, and a refused write's content is kept for review and erasable when it should not be.
- **Change tags finally have a home**, in the attestation part, which closes a gap open since 0001.
- ~~**Bots are filtered by default and still run**: a disallowed operation goes to the rejects file rather than killing the job, and a filter may exempt `bot` when that is the policy.~~ *A filter's scope excludes job writes by default (`jobs = false`); a filter that opts in sees them, and its disallowed operations still go to the rejects file (A13).*
- ~~**Farm filters need no new partition**; the primary tenant's `log` and a policy switch suffice.~~ *Global filters live in the instance `log`, which exists for other reasons (A5).*
- **The write path gains a step.** Building a context and evaluating compiled rules costs microseconds per write; the test tool costs a job, not a request.
- **Private filters stay private** because they are moderation records in an internal partition behind a read ACL, not configuration in a public one.

## Open questions

- **Q1.** ~~**Property constraints**: data-dependent rules that need the graph ("P31 values must be instances of Q5") are a reporting mechanism of their own, as on Wikidata, and the obvious next ADR.~~ *Settled by [0031](0031-property-constraints.md): soft recommendations, never a gate.*
- **Q2.** ~~**Where tags are registered.** §5 makes a change tag "a short registered string" and relies on [0011](0011-logs.md) §7's `tag/*` log for management, but 0011 §4 does not mirror `tag/*` or `managetags/*` and §6.1 projects neither.~~ *Settled by A3: a tenant `config` record of kind `tag` ([0015](0015-record-format-and-partition-registry.md) §3), keyed by the tag name, holding a description, an `active` flag and the group that may apply it by hand. System tags (`job:*`, `oauth:*`, `filter:*`, `mw-*`) are reserved prefixes no record may claim and are written only by the paths that own them. `managetags/create`, `/delete`, `/activate` and `/deactivate` project from these records ([0011](0011-logs.md) §6.1); `action=tag` and `Special:Tags` write `tag/update` events and read them; `list=tags` lists them; a filter's `tag` action may name only a registered, active tag. Writing a `tag` record needs `managechangetags`, held by `sysop`, added to `groups.toml`.*
- **Q3.** ~~**Lookups in v1**: whether a bounded `has_statement()` should ever enter the rule language, once constraints exist and the need is clearer.~~ *Settled by A3: no. A filter sees the change and the actor and nothing else; a rule that needs the graph is a constraint ([0031](0031-property-constraints.md)), which reports. The bound in §2 is permanent, and with it [0031](0031-property-constraints.md)'s question about violations touching patrolling is closed the same way.*
- **Q4. Importing Wikidata's filters**: how far the `import` mapping can go for rules that regex `new_wikitext`.
- **Q5. `ccnorm` and friends**: which of AbuseFilter's normalization functions to carry, and whether CEL's Unicode handling needs help.
- **Q6.** ~~**Filter ordering and short-circuiting**: whether a `disallow` should stop later filters from evaluating (AbuseFilter continues, for logging).~~ *Settled by A3: every enabled filter evaluates every write and every match is a hit, as AbuseFilter does; the write is refused once, naming the first disallowing filter's message. Hit counts and the test tool therefore stay complete.*
- **Q7.** ~~**Hit retention**: hits are records and never expire; whether a tenant may compact old hits of high-volume `log`-only filters.~~ *Settled by A3: a filter record may set `hit_retention`; a daily sweep erases ([0006](0006-log-integrity-and-erasure.md) §7, reason class `operational`) its hit records older than that whose actions were only `log` or `tag`, by the filter's key. Hits that warned, refused, throttled, blocked or degrouped are never swept. Headers remain, as erasure always leaves them.*
- **Q8. Autopromote**: MediaWiki's `abusefilter-unblockautopromote` presumes autopromotion; whether `autoconfirmed` thresholds ([0016](0016-permissions-and-access-control.md) Q2) interact with filters.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0001](0001-revision-metadata-rdf.md) §6 | §5 | extends | 0001 A10 |
| [0002](0002-source-graphs-and-mass-ingest.md) §8.5 | §7 | extends | 0002 A12 |
| [0005](0005-crate-organization.md) §2 | §13 | extends | 0005 A26 |
| [0010](0010-site-ui.md) §7, §12 | §10 | extends | 0010 A19 |
| [0011](0011-logs.md) §3, §6.1 | §5 | extends | 0011 A10 |
| [0011](0011-logs.md) Q7 | §5 | settles | 0011 Q7 |
| [0012](0012-api-requirements.md) §3, §4, §5 | §5, §9 | extends | 0012 A18 |
| [0013](0013-postgres-storage.md) §5, §5.2, §5.6 | §11 | amends | 0013 A9 |
| [0013](0013-postgres-storage.md) §4, §5.4, §5.5, §7, §8 | §11 | extends | 0013 A9 |
| [0014](0014-caches-and-search.md) §1, §5 | §11 | amends | 0014 A3 |
| [0014](0014-caches-and-search.md) §10 | §11 | extends | 0014 A3 |
| [0015](0015-record-format-and-partition-registry.md) §1 | §5 | amends | 0015 A13 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §5 | extends | 0015 A13 |
| [0016](0016-permissions-and-access-control.md) §2 | §12 | extends | 0016 A10 |
| [0016](0016-permissions-and-access-control.md) Q5 | — | settles | 0016 Q5 |
| [0020](0020-change-feeds.md) §2 | §6 | extends | 0020 A7 |
| [0021](0021-notifications.md) §2 | §4 | extends | 0021 A6 |
| [0023](0023-moderation.md) §3, §6 | §4–5 | extends | 0023 A3 |
| [0023](0023-moderation.md) Q3 | — | settles | 0023 Q3 |
| [0028](0028-tenancy-policy.md) §1 | §8 | extends | 0028 A3 |
| [0028](0028-tenancy-policy.md) Q1 | §8 | settles | 0028 Q1 |

## References

- [Extension:AbuseFilter](https://www.mediawiki.org/wiki/Extension:AbuseFilter), [Rules format](https://www.mediawiki.org/wiki/Extension:AbuseFilter/Rules_format) and [Actions](https://www.mediawiki.org/wiki/Extension:AbuseFilter/Actions)
- [API:Abusefilters](https://www.mediawiki.org/wiki/API:Abusefilters) and [API:Abuselog](https://www.mediawiki.org/wiki/API:Abuselog)
- [Wikidata:Abuse filter](https://www.wikidata.org/wiki/Wikidata:Abuse_filter)
- [Common Expression Language](https://cel.dev/) and the [`cel-interpreter`](https://crates.io/crates/cel-interpreter) crate
- [Manual:Tags](https://www.mediawiki.org/wiki/Manual:Tags)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §13
- **Summary:** 0005 §2 is the one crate table CI checks, and carries `scatter-filter` and every change this section listed (0005 A26).

Replaced text (§13):

> | Layer | Crate | Change |
> |---|---|---|
> | Wikibase | `scatter-filter` *(new)* | The `filter` and `filter-hit` payload types; the change-set, text and actor context types and their derivation from a change set, a page or thread operation and an actor state; the CEL environment with the AbuseFilter-compatible functions; compiled-rule evaluation and the action decision. Pure. Depends on `scatter-wikibase-changeset`, `scatter-pages`, `scatter-threads`, `scatter-normalize` and the CEL crate |
> | Ingest | `scatter-ingest` | Per-operation filtering with rejects (§7) |
> | Triplespace | `triplespace-projections` | `view.filter`, `view.filter_hit`; the filter-hits target set; the test job |
> | | `triplespace-accounts` | The per-hit IP row in the abuse-handling store |
> | | `triplespace-notify` | The `filter` reason |
> | | `triplespace-api-action`, `triplespace-api-rest` | §9; the refusal errors on every write module; filter evaluation in the write path |
> | | `triplespace-cli` | `filter test`, `filter import` |
>
> The workspace goes from forty crates to forty-one.

### A2. Property constraints report, filters gate

- **Date:** 2026-09-27
- **Source:** [0031](0031-property-constraints.md) §1, §8
- **Change:** extends §2
- **Summary:** Data-dependent rules are property constraints, soft recommendations that never gate a write. This settled Q1.

### A3. Decisions of 2026-09-27 (evening)

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** amends §5; extends §2, §4
- **Summary:** By section:
  - §5: The tag registry (decision 2). A user-defined tag is a tenant `config` record of kind `tag` ([0015](0015-record-format-and-partition-registry.md) §3): its name is the key, and the content holds a description, an `active` flag and the group whose members may apply it by hand with `action=tag`. The prefixes `job:`, `oauth:`, `filter:` and `mw-` are reserved for the paths that write them and cannot be registered. `managetags/*` events project from the records and `tag/update` from manual application ([0011](0011-logs.md) §6.1); `list=tags` and `Special:Tags` read the registry; a filter's `tag` action names a registered, active tag. The right is `managechangetags` (`sysop`); applying a tag by hand needs `changetags`, held by `user`, subject to the tag's own group. A filter record may set `hit_retention` (decision 15): a daily sweep erases its hit records older than that whose actions were only `log` or `tag`; hits that warned, refused, throttled, blocked or degrouped are never swept.
  - §2: No graph reads in filters, ever (decision 15): the bound of §2 is permanent, and 0031's question about violations touching patrolling closes the same way.
  - §4: Every enabled filter evaluates every write and every match is a hit, as AbuseFilter does; the write is refused once, naming the first disallowing filter's message (decision 15).

Replaced text (§2):

> A rule cannot read the graph: "the value of P31 must be an instance of Q5" is a property constraint, a separate reporting mechanism as on Wikidata, and is left to its own ADR (open questions). This bounds the cost of a filtered write to the cost of building the context from the record in hand.

### A4. The upload context

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §5
- **Change:** extends §2
- **Summary:** A third context, `upload`, with AbuseFilter's file variables and `file_sha256`; `action` is `upload` or `stashupload`.

### A5. Global filters in the instance log

- **Date:** 2026-09-30
- **Source:** [0040](0040-instance-prerogatives.md) §6
- **Change:** amends §8
- **Summary:** Global filters are records in the **instance `log`**, not the primary tenant's, since the instance now has a log of its own; evaluation order is unchanged. A `block` or `degroup` a global filter appends to a tenant's `actors` is an instance prerogative, binding for its duration: a tenant unblock lifts only the tenant's own block. Hits are unchanged: they stay in the tenant's `log`, attested by the actor whose write was hit.

Replaced text (§8):

> Under `inherited`, filters in the **primary tenant's** `log` partition with `scope = global` run on every tenant before the tenant's own filters, in ID order, and a tenant cannot exclude them, as with global groups ([0028](0028-tenancy-policy.md) §3).

### A6. Link variables from expanded text

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §17
- **Change:** amends §2
- **Summary:** With expansion on, `added_links` and `removed_links` come from the expanded text, computed only when an enabled filter reads them.

Replaced text (§2):

> | `added_links`, `removed_links` | Resolved wiki links and mentions ([0008](0008-namespaces-and-document-pages.md) §10, [0019](0019-discussions.md) §5) |

### A7. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–13
- **Summary:** A1–A6 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A3's tag registry and A4–A6 were blockquotes, the rest of A3 struck questions with notes, and A2 a struck question. The file before conversion is commit `0b26a3a`.

### A8. Redirect variables

- **Date:** 2026-10-01
- **Source:** [0051](0051-page-redirects.md) §9
- **Change:** extends §2
- **Summary:** `old_redirect` and `new_redirect` in the page context.

### A9. Fork variables

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §2
- **Change:** extends §2
- **Summary:** `fork`, `fork_repo` and `fork_revid` in the page context, so a tenant can gate or tag forks.

### A10. Upstream posts

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §5
- **Change:** extends §2
- **Summary:** The text context covers a reply or section about to be sent upstream, as `action = upstream-post` with `upstream_repo`.

### A11. The right to see an IP is named

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §2, §5, §12
- **Summary:** The right that seeing an IP in the abuse-handling store of [0007](0007-actor-identity.md) §3 requires is named, so the permission catalogue can list it: `ts-viewip` *(new)*, default `sysop`, James to confirm. §2's `ip` variable, §5's hit page and §12's closing sentence all refer to that store's right without naming it; they now name `ts-viewip`, and §12's table gains its row. (PENDING D4)

### A12. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§13
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [05](../architecture/05-providers-and-ingest.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A13. Filters are a step of tier 1; job writes are opted into

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §1
- **Change:** amends §7; extends §1, §2, §5
- **Summary:** Edit filters stay in the appending transaction, as a step of tier 1, since a filter is a function of the written record and the actor and nothing composes while it runs (§1). A filter's scope **excludes job writes by default** (`jobs = false`); a filter that opts in sees each operation of a bulk job in the `local` partition, unless it exempts the job's group, and `disallow` still sends the operation to the rejects file (§7). Hits from a job's writes aggregate into one hit record per (filter, job), carrying a count and the coordinates of the first N records matched, as the job's rejects file already summarizes refusals (§5). The `rate` variable is computed lazily, read from the counters only when an enabled filter's rule names it (§2). The `read` rate class of [0024](0024-subsidiary-accounts.md) §5 is counted for authenticated principals only once a per-process count passes a fraction of the limit, and never for a response served from L0 or L1. 0083's table gives this change as `extends 0030 §7`; the chapter text of §7 said every filter applied to every operation of a job unless it exempted the job's group, which the default now contradicts, so it is logged as an amendment. (REVIEW G8, G15)

Replaced text ([05](../architecture/05-providers-and-ingest.md) §3.6, as it stood):

> Filters ([09](../architecture/09-security-and-moderation.md)) apply to **each operation** of a bulk job in the `local` partition, in the job's stream, unless the filter exempts the job's group.

> A job of a million operations under ten filters spends seconds on them.
