# 0074. Publishing a scope to an external wiki

- **Status:** Proposed
- **Date:** 2026-10-07
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0024](0024-subsidiary-accounts.md), [0027](0027-preferences-and-portability.md), [0040](0040-instance-prerogatives.md), [0060](0060-scopes.md), [0062](0062-workspaces.md), [0067](0067-proposals.md), [0070](0070-shallow-entity-mirroring.md), [0071](0071-derived-statements-from-mirrored-pages.md)

## Context

[0071](0071-derived-statements-from-mirrored-pages.md) turns a mirrored wiki's pages into statements on items, and [0060](0060-scopes.md) gathers items into a scope. The wiki those pages came from may want the result back. The FamilySearch Research Wiki does: its owners want the relations written into the wiki, so that the wiki itself is immediately useful, and resource lists produced from data, not curated by hand.

The wiki runs MediaWiki 1.39 with the **Cargo** extension, and not Semantic MediaWiki. Cargo stores a row only when a page that calls a template with `{{#cargo_store:…}}` is parsed, so data reaches a Cargo table only through page wikitext; pages can then list rows with `{{#cargo_query:…}}`, and Cargo tables are readable over the wiki's API (`action=cargoquery`). The owners will not run Triplespace; they will create a Cargo table and grant a bot account.

[0067](0067-proposals.md) already sends changes upstream, as proposals a person makes to the wiki an entity came from. Writing machine-owned pages to a wiki that asked for them is a different act and needs its own rules.

### Direction

James's direction, from the design discussion of 2026-10-07:

- **"They want the semantic relations to be written back into the wiki so that the wiki itself is immediately useful."** And: "I really don't want to use Semantic MediaWiki."
- **"Use Triplespace to extract semantics from the wiki pages, write them back into Cargo tables, and then … automatically generate wiki pages representing the other axes of access. Those pages would use automatically generated resource lists based on Cargo queries."**
- **They eventually want dynamically produced reports**, rather than curating lists by hand.
- **"Triplespace reflects a broader business goal of not having to write one-offs for customers."** The sink is a general component, configured per customer.

## Decision

### 1. Publications (extends 0015 §3)

**A publication writes a scope's data to another wiki as pages of template calls.** It is tenant configuration, a `config` record of kind `publication` whose code is the publication's name:

| Field | Meaning |
|---|---|
| `destination` | The destination wiki's API endpoint and a display name |
| `account` | The credential the publication writes with (§4) |
| `scope` | The scope whose members are published ([0060](0060-scopes.md) §1) |
| `rows` | The row template and its fields (§2) |
| `pages` | How rows are grouped into data pages (§3) |
| `kits` | Generated pages (§5) |
| `debounce`, `edits_per_minute`, `overwrite` | §3–4 |

### 2. Rows

**Each scope member is one call of the destination's data template.** `rows` names the template and its fields; each field is a name on the destination and a path on the member:

| Path | Yields |
|---|---|
| `id`, `label`, `description` | The member's ID, and its label and description in the field's `lang` (default the publication's language) |
| A property (`P9010`) | The values of the member's best-rank statements, as labels for items and as text for the rest |
| A property with `as = "id"` or `as = "url"` | Item values as IDs, or as their canonical IRIs |
| A path, `P9011/WDP131*` | Values reached through a chain of properties; `*` follows a property transitively, over mirrored and local data, to `entities.closure_depth` ([0070](0070-shallow-entity-mirroring.md) §2.3) |
| `mentioned_in.url` | The URL of the page subject the member is mentioned in ([0071](0071-derived-statements-from-mirrored-pages.md) §4), so the row links back to its guide |

A field with several values is joined by its `join` (default `;`), which matches a Cargo `List (;) of …` field. **Transitive paths are how hierarchies reach Cargo**, which cannot follow them: a resource about Cuyahoga County gets `Cuyahoga County;Ohio;United States` in its jurisdiction field, and a query for Ohio finds it with `HOLDS`.

Values are written as plain text, escaped for a template parameter: `|` as `{{!}}`, `=` in a value unchanged (fields are always named), `{{` and `}}` and `[[` as HTML entities, newlines as spaces. A row's field order is the configuration's, and rows are sorted by member ID, so the same data always makes the same text.

### 3. Data pages

**Rows are written to machine-owned data pages, grouped by `pages.group_by`**: a path whose first value names the group (for FamilySearch, the guide page each resource is mentioned in, or its top jurisdiction), and `pages.title`, a pattern such as `"{group}/Semantic data"`. A member with no value for the path goes to `pages.ungrouped`. A group larger than `pages.max_rows` (default 500) is split into numbered pages.

Each data page holds a comment saying it is generated and from where, then its rows. **A data page is rewritten only when its text would change.** A change to a member's resolved state, or to the scope's membership, marks the member's group dirty; a dirty group is regenerated after `debounce` (default 1 hour), so a re-extraction of many pages produces one edit per data page. A group that becomes empty gets a page holding only the comment, not a deletion (Q1).

### 4. Writing to the destination

- **The account** is a bot account on the destination, with a bot password, or an OAuth grant held for the publication ([0067](0067-proposals.md) §6). A bot password is kept in `private.publication_credential`, encrypted at rest and never returned by any API, in the "re-established" portability class ([0027](0027-preferences-and-portability.md) §2).
- **Edits** go through the upstream client with `bot`, `maxlag=5`, the last known `basetimestamp`, and the summary "Updated from {scope} by {instance} (job {id})". They count against the publication's `edits_per_minute` (default 10) as well as the `upstream` rate class ([0024](0024-subsidiary-accounts.md) §5).
- **A data page someone else has edited is not overwritten.** Before writing, the job compares the page's latest revision with the last it wrote. With `overwrite = never` (default), a page edited by anyone other than the account is skipped and reported as a conflict until someone resolves it; `overwrite = always` writes anyway, for a destination that agrees the pages are machine-owned.
- **The job is an instance-run job per publication** ([0040](0040-instance-prerogatives.md) §6), with `job/start` and checkpoints, so its runs, edit counts and conflicts are on its job page.

### 5. Generated pages from kits (uses 0062 §6)

**A publication can create pages from a kit, once each.** A kit here is a [0062](0062-workspaces.md) §6 kit whose pages are written to the destination, not the tenant, with one more placeholder kind: `for_each`, a path over the scope's members whose distinct values each make one page.

```json
{ "for_each": "P9010",
  "title": "{{label}} records, all localities",
  "content": "{{Resource list|record_type={{label}}}}" }
```

A page is **created when missing** and never edited afterwards, unless `kits.update = true` and every revision of the page is the account's. A value that appears later makes its page then. The content is meant to be static wikitext whose list is a `{{#cargo_query:…}}`, usually through a template on the destination, so the page stays current as data pages change and needs no further edits.

### 6. The destination's template and table

**The publication can write the data template for an administrator to install; it never installs it.** `triplespace-cli publication template {name}` prints a template with a `{{#cargo_declare:…}}` built from the row fields, with types inferred from their paths (`String`, `List (;) of String`, `URL`, `Date`, `Page` for `mentioned_in.url`'s title), and the matching `{{#cargo_store:…}}`. Creating the table on Cargo's table administration page and protecting the data pages are the destination's acts.

### 7. Publications are not proposals (uses 0067 §1)

A proposal offers a change to the wiki an entity came from, for its community to adopt or decline, and closes the loop by watching for adoption. A publication writes pages its destination has agreed to receive, under an account the destination granted, and owns them. The two share the upstream client and nothing else: a publication has no thread, no state machine and no adoption test.

### 8. API, operations and storage (extends 0012 §5; extends 0013 §5.6)

- `GET /publication/{name}`: configuration, data pages with their last revision and state, conflicts, last run. `POST /publication/{name}/run`: regenerate now (the `ts-runjob` right).
- `triplespace-cli publication run | status | template`.
- `ops.publication_page (tenant, publication, title, group_key, last_revid, text_hash, written_at, state)` with states `current`, `dirty`, `conflict`; `private.publication_credential (tenant, publication, username, secret)`.

### 9. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `triplespace-publish` | **New.** Rows from scope members and paths, transitive paths, escaping and deterministic text, grouping, dirty tracking and debounce, the publication job, kits with `for_each`, the template generator; the `publication` config kind |
| `triplespace-upstream` | `action=edit` with a publication's bot password or grant, `basetimestamp` and the conflict check |
| `triplespace-accounts` | `private.publication_credential` |
| `triplespace-api-rest`, `triplespace-cli` | §8 |

## Alternatives considered

- **Semantic MediaWiki.** Ruled out by direction; it would also need the destination to adopt and maintain it.
- **A MediaWiki extension that reads Triplespace at parse time.** No page writes at all, but the destination must install and run new code, and its reports then depend on Triplespace being up. Kept as a later option (Q2).
- **Template parameters on the guide pages themselves**, written by a bot. The data lands next to the text it came from, but every guide page is edited by a machine, and an editor's next edit can break the data.
- **A Python script outside Triplespace.** It would work once, for one customer; the sink is meant to be configuration.

## Consequences

- **A wiki with Cargo can receive a Triplespace scope as data** and build dynamic reports on it with its own tools, with nothing installed beyond a template.
- **The destination keeps working if the service stops.** Data pages stay; they stop changing.
- **Hierarchies are flattened into lists at publication time**, so a destination's queries stay simple; a change high in a hierarchy dirties every group below it.
- **Human edits are respected**, at the cost of conflicts that someone must resolve.
- **Test plan.** A scope of three resources under two guides publishes two data pages; re-running with no change makes no edit; a changed label makes one edit to one page; a human edit to a data page stops writes to it and reports a conflict; a kit makes one page per record type and never edits them again; the generated template's `#cargo_declare` matches the row fields.

## Open questions

- **Q1. Deleting data pages** whose group is gone: blanking leaves stubs; deletion needs a right the destination may not grant.
- **Q2. A parse-time extension** for destinations willing to install one, which would make data pages unnecessary.
- **Q3. Other sinks** with the same row model: a Wikibase destination through `wbeditentity`, or a CSV for a destination without Cargo.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §9 | amends | 0005 A77 |
| [0012](0012-api-requirements.md) §5 | §8 | extends | 0012 A52 |
| [0013](0013-postgres-storage.md) §5.6 | §8 | extends | 0013 A38 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §1 | extends | 0015 A33 |

## References

- [Extension:Cargo](https://www.mediawiki.org/wiki/Extension:Cargo): `#cargo_declare`, `#cargo_store`, `#cargo_query`, `HOLDS` on list fields, `action=cargoquery`
- [Manual:Bot passwords](https://www.mediawiki.org/wiki/Manual:Bot_passwords)
- [Manual:Maxlag parameter](https://www.mediawiki.org/wiki/Manual:Maxlag_parameter)
- `claude/familysearch-semantic-layer-plan.md` (Triplespace project notes, 2026-10-07)
