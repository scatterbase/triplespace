# 0030. Edit filters

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-06 (A10)
- **Author:** James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md), [0023](0023-moderation.md), [0028](0028-tenancy-policy.md)
- **Uses:** [0007](0007-actor-identity.md), [0022](0022-federation.md), [0024](0024-subsidiary-accounts.md), [0025](0025-oauth-server.md), [0029](0029-resolver-namespaces.md)

## Context

MediaWiki's AbuseFilter lets a wiki write rules that run against every edit before it is saved: a small expression language over variables describing the edit and the editor (`added_lines`, `summary`, `user_age`, `user_groups`, …), a set of actions (log, tag, warn, disallow, throttle, block, degroup), a log of hits, private filters whose rules vandals cannot read, and a way to test a rule against recent changes. Its name undersells it: wikis use it to catch typos, enforce formatting, tag categories of change and slow down new accounts as much as to stop abuse. This ADR calls it an **edit filter**, and keeps MediaWiki's `abusefilter-*` right and module names only where compatibility needs them.

On a Wikibase, AbuseFilter is awkward for a reason that does not apply here. It sees an edit as **text**: `new_wikitext` on Wikidata is a JSON serialization of the entity, and filter authors end up matching serialized JSON with regular expressions. Triplespace never has text where structure exists. Every write is a typed change set ([0002](0002-source-graphs-and-mass-ingest.md) §8) or a typed page or thread operation ([0008](0008-namespaces-and-document-pages.md) §4, [0019](0019-discussions.md) §1), and every write already passes through one place, the appending transaction ([0013](0013-postgres-storage.md) §7), where a base-offset check and the ACLs of [0023](0023-moderation.md) run before anything is appended.

James named two dimensions, **abusive statements** and **abusive page contents**. They turn out to be two **contexts** over one engine. [0016](0016-permissions-and-access-control.md) left abuse filters open as "neither permissions nor ACLs", and [0028](0028-tenancy-policy.md) asked where farm-wide filters would live. This ADR answers both.

## Decision

### 1. A filter is a rule with actions, evaluated before append

An **edit filter** is a named rule that is evaluated against every candidate write in the tenant's `local` and `pages` partitions, in the appending transaction, after the permission and ACL checks and before the append. It has:

- a **rule**: an expression in the language of §3 over the context of §2, which evaluates to true (the filter **matches**) or false;
- **actions** taken on a match (§4);
- **contexts** it applies to: `changeset`, `text`, or both;
- a **scope**: which of the tenant's own writes it sees, by default all, with exempt groups (§4) and whether it sees job writes (§7);
- a **privacy** flag: a private filter's rule, notes and hit details are visible only to holders of `abusefilter-view-private` (§5);
- an **enabled** flag, a **name**, **notes** for other filter editors, and a **public message** shown to the editor on `warn` and `disallow`.

Filters run in order of ID. Every match is recorded (§5); the actions decide what else happens. A write that no filter disallows proceeds to append exactly as before this ADR.

**What is never filtered:** mirror partitions, since upstream data is upstream's and a mirror is faithful by design ([0002](0002-source-graphs-and-mass-ingest.md) §2); `config`, `actors` and `accounts` records, which are administrative actions with their own permissions; and moderation records ([0023](0023-moderation.md)), for the same reason. A filter governs what editors and their bots write.

### 2. Three contexts and one actor

*Changed by A2, A3, A4, A6, A8, A9, A10.*

A rule sees a **context**: a typed set of variables derived from the candidate record and the actor. Variable names follow AbuseFilter's where AbuseFilter has one, so that a filter author can port a rule by hand and the API of §9 can answer in MediaWiki's shape.

**The change-set context** (`changeset`), for writes to `local`:

| Variable | Meaning |
|---|---|
| `action` | `edit` or `create`; the change-set operations in detail under `ops` |
| `entity_id`, `entity_type`, `entity_new` | The subject, its type, whether this write creates it |
| `ops` | The operations of [0002](0002-source-graphs-and-mass-ingest.md) §8.2 and [0004](0004-identity-clusters-and-equivalence.md) §9 as a list: kind, property, and the snak or term it adds, removes or overrides |
| `properties_added`, `properties_removed` | Property IDs touched, as main snak, qualifier or reference, each with its role |
| `values_added`, `values_removed` | Data values by type, in **normalized form** ([0004](0004-identity-clusters-and-equivalence.md) §7, [0029](0029-resolver-namespaces.md) §1): `external-id` and `string` as text, `wikibase-item` as canonical IDs, `url` as normalized URLs with a `host`, `quantity` and `time` as their parts |
| `labels_added`, `descriptions_added`, `aliases_added`, and `_removed` | Term text by language, so a rule can regex a description in `en` |
| `sitelinks_added`, `sitelinks_removed` | URLs and their hosts ([0026](0026-sitelinks.md) §1–2) |
| `rank_changes` | Statement GUID, old rank, new rank |
| `statement_count`, `statement_delta`, `reference_delta` | Before and after |
| `summary` | The comment part |

**The text context** (`text`), for writes to `pages`:

| Variable | Meaning |
|---|---|
| `action` | `edit`, `create`, `move`, `rename`, `post`, `reply`, `thread-create`, or `upstream-post` for a reply or section about to be sent to a repository as the person, which a `disallow` stops ([0069](0069-synchronized-talk-pages.md) §5) |
| `upstream_repo` | For `upstream-post`, the repository it would be sent to |
| `page_namespace`, `page_title`, `page_id`, `content_model` | As MediaWiki names them |
| `old_redirect`, `new_redirect` | Whether the text before and after the edit begins with a redirect line ([0051](0051-page-redirects.md) §9) |
| `fork`, `fork_repo`, `fork_revid` | Whether the edit forks an inherited title, and from which repository and upstream revision ([0054](0054-forking-a-mirrored-page.md) §2) |
| `old_text`, `new_text`, `added_lines`, `removed_lines`, `edit_delta` | The full texts and a line diff, as AbuseFilter gives them |
| `added_links`, `removed_links` | Resolved wiki links and mentions ([0008](0008-namespaces-and-document-pages.md) §10, [0019](0019-discussions.md) §5); with template expansion on, from the expanded text, computed only when an enabled filter reads them ([0042](0042-template-expansion-and-parsoid.md) §17) |
| `added_external_links`, `added_external_hosts` | External URLs and their hosts |
| `thread_id`, `thread_target`, `post_depth`, `in_reply_to` | For posts ([0019](0019-discussions.md) §1–2) |
| `summary`, `new_title` | The comment part; the target of a move or rename |

**The upload context** (`upload`), for file uploads: AbuseFilter's file variables and `file_sha256`; `action` is `upload` or `stashupload` ([0039](0039-files-and-media.md) §5).

**The actor context**, present in every context:

| Variable | Meaning |
|---|---|
| `user_name`, `user_id`, `user_kind` | The actor's current name, ID and kind ([0007](0007-actor-identity.md) §4); `federated` for a fediverse surrogate ([0022](0022-federation.md) §8) |
| `user_groups`, `user_rights` | As of now, including global groups ([0028](0028-tenancy-policy.md) §3) |
| `user_age`, `user_editcount` | Seconds since the first actor record; local edits to date |
| `user_is_subsidiary`, `user_operator` | [0024](0024-subsidiary-accounts.md) §1; a rule can treat a bot by its operator's standing |
| `user_blocked_elsewhere` | Whether the operator or the farm account is blocked ([0024](0024-subsidiary-accounts.md) §3, [0028](0028-tenancy-policy.md) §4) |
| `rate` | The actor's current counts in each rate-limit class ([0024](0024-subsidiary-accounts.md) §5) |
| `ip`, `ip_in_range(cidr)` | The request's address, available to the rule and never written anywhere but the abuse-handling store of [0007](0007-actor-identity.md) §3 |
| `timestamp`, `job_id`, `api_key_label` | When; the job if any ([0002](0002-source-graphs-and-mass-ingest.md) §8.3); the key label if the actor is a subsidiary ([0024](0024-subsidiary-accounts.md) §4) |

**Rules see the change and the actor, and nothing else.** A rule cannot read the graph: "the value of P31 must be an instance of Q5" is a property constraint, a separate reporting mechanism as on Wikidata ([0031](0031-property-constraints.md)), which reports and never gates. This bounds the cost of a filtered write to the cost of building the context from the record in hand, and the bound is permanent: no `has_statement()` or other lookup will enter the rule language.

### 3. The rule language is CEL

Rules are written in the **Common Expression Language** (CEL): non-Turing-complete, statically typed, evaluated in bounded time, with a Rust implementation and a syntax that reads much as AbuseFilter's does. Regular expressions, string functions, list comprehensions and the AbuseFilter conveniences filter authors expect (`contains_any`, `count`, `norm`, `rmwhitespace`, `ccnorm`) are provided as CEL functions. A rule that does not type-check is refused when the filter is saved, with the error; a rule is compiled once and cached until the filter changes.

AbuseFilter's own language is **not** reimplemented. Its Wikidata rules mostly work by matching `new_wikitext`, which has no analogue here, and its text-context rules port to CEL by hand in minutes because the variables keep their names. `Special:EditFilter/import` accepts an AbuseFilter export and produces a draft CEL rule with the variables mapped and the rest marked for the author.

```
// Disallow new accounts adding a description in English that is an external link
user_age < 86400 && !("autoconfirmed" in user_groups)
  && descriptions_added.exists(d, d.language == "en" && d.text.matches("https?://"))
```

```
// Tag any edit that adds a DOI whose prefix is not Crossref-registered-looking
values_added.exists(v, v.property == "doi" && !v.text.matches("^10\\.[0-9]{4,9}/"))
```

### 4. Actions

*Changed by A3.*

| Action | Effect | Requires |
|---|---|---|
| `log` | Record the hit (§5). Every match is logged; this action only says nothing else happens | — |
| `tag` | Add change tags to the record (§5) | — |
| `warn` | Refuse the write once with the public message; the same write resubmitted with the returned token within an hour proceeds, and the hit records that it was warned and resubmitted | — |
| `disallow` | Refuse the write with the public message; the hit records the attempted change | — |
| `throttle` | Count matches per (filter, actor or IP) in a `rl:filter:{id}:{key}` counter ([0024](0024-subsidiary-accounts.md) §5); the filter's other actions fire only once the count exceeds the filter's threshold in its window | — |
| `unpatrol` | Strip autopatrol from this write, so it lands in the review queue whoever made it ([0023](0023-moderation.md) §6) | — |
| `notify` | Deliver the hit to the inbox of every member of a named group ([0021](0021-notifications.md) §1), as a new reason `filter` | — |
| `block` | Append a `block` record for the actor ([0016](0016-permissions-and-access-control.md) §3) with the filter's duration and message | `abusefilter-modify-restricted` on the filter's author |
| `degroup` | Remove the actor from every group but `user` | `abusefilter-modify-restricted` |

**Exempt groups.** A filter may name groups whose members it does not apply to; `bot` is the usual one, and a filter that names none applies to everyone including administrators, as AbuseFilter does.

**Every enabled filter evaluates every write,** and every match is a hit, as AbuseFilter does; a `disallow` does not stop later filters. The write is refused once, naming the first disallowing filter's message, so hit counts and the test tool stay complete.

`warn`, `disallow`, `block` and `degroup` are **refusals**: the write is not appended, so no record exists for it. The Action API reports them as `abusefilter-warning` and `abusefilter-disallowed`, the REST API as `403` with the same codes, in every case naming the filter's public message and, for `warn`, the resubmission token.

### 5. Records: filters, hits and tags (extends 0011 §2, §6.1; amends 0015 §1)

*Changed by A3.*

**A filter is a record in the tenant `log` partition**, payload type `scatter:v0/filter`, keyed `filter:{id}` where the ID is minted in sequence. Its content part holds the rule, contexts, scope, actions, exempt groups, privacy and enabled flags, name and notes; its comment part the reason for the change; its attestation who changed it. A later record for the same key replaces the filter, so a filter's history is its records, and `Special:EditFilter/history/{id}` is a history page. Filters live in `log`, not `config`, for the reason moderation ACLs do ([0023](0023-moderation.md) §3): `config` is publicly exported, and a **private filter** must not be. A private filter's records are behind a `read` ACL ([0023](0023-moderation.md) §1) restricted to holders of `abusefilter-view-private`, written with the filter. Each change projects as `abusefilter/create` or `abusefilter/modify` in the local log ([0011](0011-logs.md) §6.1).

**A hit is a record**, payload type `scatter:v0/filter-hit`, in the tenant `log` partition, keyed `filter:{id}` as well so that a filter's hits and its history are one key. Its content holds: the filter ID and the version matched; the actions taken; the context's non-personal variables; for a write that was appended, the record's coordinates and revision ID; for a refused write, the **attempted change set or text**, so that reviewers can see what was stopped. Its attestation is the actor. IP is not in the hit: it is in the abuse-handling store of [0007](0007-actor-identity.md) §3 under that store's retention, keyed by the hit's coordinates, and shown on the hit page only to holders of the right that store already requires. A hit of a private filter is visible only to `abusefilter-log-private`; every other hit is public, as AbuseFilter's log is. Hits are activity rows of kind `log` with `log_type = abusefilter`, `log_action = hit`, so the filter log is a feed (§6). Hits are erasable per part like any record; the attempted content of a refused write is the part a suppressor erases when it should never have been kept. A filter record may set **`hit_retention`**: a daily sweep erases ([0006](0006-log-integrity-and-erasure.md) §7, reason class `operational`) its hit records older than that whose actions were only `log` or `tag`, by the filter's key; hits that warned, refused, throttled, blocked or degrouped are never swept, and headers remain, as erasure always leaves them.

**Change tags get a home (amends 0015 §1).** [0001](0001-revision-metadata-rdf.md) §6 and [0012](0012-api-requirements.md) §3 carry change tags without saying where a record stores them. They go in the **attestation part**, whose meaning widens from "who is responsible" to "who, and how": the actor, the job, and now the tags. A tag is a short registered string; `tag` actions add the filter's tags, `job:{id}` ([0010](0010-site-ui.md) §7) and the OAuth consumer tag of [0025](0025-oauth-server.md) are tags of the same kind, and MediaWiki's `mw-undo` and `mw-manual-revert` are written by the undo path of [0010](0010-site-ui.md) §6. Tags are in the erasable body, since a tag can be an accusation.

**The tag registry.** A user-defined tag is a tenant `config` record of kind `tag` ([0015](0015-record-format-and-partition-registry.md) §3): its name is the key, and the content holds a description, an `active` flag and the group whose members may apply it by hand with `action=tag`. The prefixes `job:`, `oauth:`, `filter:` and `mw-` are reserved for the paths that write them and cannot be registered. `managetags/*` events project from the records and `tag/update` from manual application ([0011](0011-logs.md) §6.1); `list=tags` and `Special:Tags` read the registry; a filter's `tag` action names a registered, active tag. The right is `managechangetags` (`sysop`); applying a tag by hand needs `changetags`, held by `user`, subject to the tag's own group.

### 6. The filter log is a feed (extends 0020 §2)

**Filter hits** join the target sets of [0020](0020-change-feeds.md) §2: the hits of one filter, of all filters, or of one actor, filtered by action taken, delivered as a page (`Special:EditFilterLog`), Atom or stream like any feed, and redacted per viewer as the privacy rules of §5 require. A hit row links to the filter, the actor, the record if one exists and the diff, and shows the public variables. `list=abuselog` serves the same rows in MediaWiki's shape (§9).

**Testing.** `Special:EditFilter/test` evaluates a draft rule against the activity rows of a period, rebuilding each row's context from its record and the actor's state as of its time, the same as-of evaluation autopatrol uses ([0023](0023-moderation.md) §6). It reports which rows would have matched. It runs asynchronously as a job in `ops`, since a period may hold many rows, and it never takes an action.

### 7. Bulk jobs (extends 0002 §8.5)

Filters apply to **each operation** of a bulk job in the `local` partition, in the job's stream, unless the filter exempts the job's group. `disallow` sends the operation to the job's **rejects file** ([0002](0002-source-graphs-and-mass-ingest.md) §8.5) with the filter named as the reason, and the job continues; the job's finish record ([0011](0011-logs.md) §6.3) counts filter rejects separately. `warn` and `throttle` do not apply to jobs, since there is nobody to warn and the job's own rate is [0024](0024-subsidiary-accounts.md) §5's business; `tag`, `unpatrol`, `notify` and `log` apply as usual, and a `block` or `degroup` on the job's actor stops the job. An `atomic` batch ([0002](0002-source-graphs-and-mass-ingest.md) §8.5) fails whole if any operation is disallowed, since it was asked to.

Cost: building a change-set context per operation is a walk over a change set already in memory, and a compiled CEL rule evaluates in microseconds. A job of a million operations under ten filters spends seconds on them. Ingest of mirrors, which is where the billions are, is never filtered.

### 8. Farm-wide filters (extends 0028 §1)

*Changed by A5.*

The tenancy policy gains a switch, `filters.global` (`none` or `inherited`; `none` in the `isolated` preset, `inherited` in `community` and `enterprise`). Under `inherited`, filters in the **instance `log`** partition ([0040](0040-instance-prerogatives.md) §6) with `scope = global` run on every tenant before the tenant's own filters, in ID order, and a tenant cannot exclude them, as with global groups ([0028](0028-tenancy-policy.md) §3). A `block` or `degroup` a global filter appends to a tenant's `actors` is an instance prerogative, binding for its duration: a tenant unblock lifts only the tenant's own block. Their hits are recorded in the tenant where the write happened, with the filter's home noted, so each tenant's log shows what stopped its editors and the farm's filter editors see every hit through the all-tenants set ([0028](0028-tenancy-policy.md) §9). Writing a global filter needs `abusefilter-modify` held through a global group. This settles 0028 Q1 without a new partition.

### 9. API (extends 0012 §4 and §5)

**Action API**, in AbuseFilter's shape because tools and the Wikipedia apps speak it: `list=abusefilters` (`abfprop=id|description|pattern|actions|hits|comments|lasteditor|lastedittime|status|private`; `pattern`, `comments` and private filters redacted for viewers without the rights), `list=abuselog` (`aflprop=ids|filter|user|title|action|details|result|timestamp|revid`; redacted likewise), `action=abusefiltercheckmatch` and `action=abusefilterchecksyntax` (over CEL), `action=abusefilterevalexpression`, `action=abusefilterunblockautopromote`. Every write module can return `abusefilter-warning` (with `abusefilter_token`) and `abusefilter-disallowed`. `list=tags` and `action=tag` gain the tags of §5.

**REST**, under `rest.php/triplespace/v0`: `GET`, `POST`, `PUT /filters[/{id}]`, `GET /filters/{id}/history`, `GET /filters/{id}/hits`, `POST /filters/test` (returns a job), `GET /filters/log` with the feed parameters of [0020](0020-change-feeds.md) §5. The activity row ([0012](0012-api-requirements.md) §3) gains `filters`: the filters a record matched with their actions, for rows the viewer may see.

### 10. UI (extends 0010 §2 and §7)

`Special:EditFilter` lists filters with hit counts and last-hit times; `Special:EditFilter/{id}` edits one, with the CEL editor, a syntax check, a **Test** tab running §6's test over a chosen period, the actions form and the privacy switch; `Special:EditFilter/history/{id}`; `Special:EditFilterLog` with the feed filters of §6; `Special:Tags`. A refused write shows the filter's public message in place, with the resubmit control for `warn`. A tagged row shows its tags as chips ([0010](0010-site-ui.md) §2). The UI says **edit filter** everywhere; the `abusefilter-*` names appear only in rights and API modules.

### 11. Storage (extends 0013 §5.6 and §7)

- `view.filter (id, tenant, version_offset, name, enabled, private, scope, contexts, rule, actions jsonb, exempt_groups, hits bigint, last_hit)`: the projection of the latest record per filter key; the compiled rule is held in L0 per process and rebuilt on change.
- `view.filter_hit (partition, offset, filter_id, filter_version, actor_key, actions, target_kind, target_id, record_partition, record_offset, refused boolean, time)`, indexed by filter and by actor, for the log page and `list=abuselog`; the context variables and attempted content are read from the hit record itself.
- Filters are evaluated in the appending transaction (step 0 of [0013](0013-postgres-storage.md) §7's write path, with the ACL checks), and hits are appended in the same transaction as the write they concern, or alone for a refusal. The `filter` projection runs in step 1, since the write path reads it.
- Throttle counters are `rl:filter:{id}:{key}` in Valkey ([0014](0014-caches-and-search.md) §4) with the filter's window as TTL.
- The abuse-handling store of [0007](0007-actor-identity.md) §3 gains a row per hit `(hit partition, hit offset, ip, expires)`, in `private`, under the store's retention.

### 12. Permissions (extends 0016 §2; settles 0016 Q5)

MediaWiki's right names are kept for compatibility, though the UI says edit filter:

| Permission | Governs | Default groups |
|---|---|---|
| `abusefilter-view`, `abusefilter-log` | Seeing public filters and public hits | `universe` |
| `abusefilter-view-private`, `abusefilter-log-private` | Seeing private filters' rules and hit details | `sysop` |
| `abusefilter-log-detail` | Seeing the full context and attempted content of a hit | `sysop` |
| `abusefilter-modify` | Creating and changing filters and their `log`, `tag`, `warn`, `disallow`, `throttle`, `unpatrol` and `notify` actions | `sysop` |
| `abusefilter-modify-restricted` | The `block` and `degroup` actions | `bureaucrat` |
| `abusefilter-revert` | Reverting every change a filter matched in a period, as one job ([0010](0010-site-ui.md) §9) | `sysop` |
| `abusefilter-bypass` | Exemption from every filter, held by no default group | — |

The IP shown on a hit page needs the right the abuse-handling store already requires.

### 13. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with `scatter-filter` and every change this section listed. The table this section first gave is in A1.

## Consequences

- **Rules see structure, not serialized JSON.** A Wikidata-style vandalism rule on descriptions, values or identifiers is a one-line CEL expression over typed variables, which is what AbuseFilter could never offer a Wikibase.
- **Two dimensions, one engine.** Statements and page text differ only in context; actions, records, hits, testing, rights and the API are shared.
- **Every hit is a record and every filter has history**, so the filter log is a feed, filters can be audited, and a refused write's content is kept for review and erasable when it should not be.
- **Change tags finally have a home**, in the attestation part, which closes a gap open since 0001.
- **Bots are filtered by default and still run**: a disallowed operation goes to the rejects file rather than killing the job, and a filter may exempt `bot` when that is the policy.
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
