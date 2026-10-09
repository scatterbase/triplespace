# 0074. Publishing a scope to an external wiki

- **Status:** Proposed
- **Date:** 2026-10-07
- **Updated:** 2026-10-09 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0024](0024-subsidiary-accounts.md), [0027](0027-preferences-and-portability.md), [0040](0040-instance-prerogatives.md), [0060](0060-scopes.md), [0062](0062-workspaces.md), [0067](0067-proposals.md), [0070](0070-shallow-entity-mirroring.md), [0071](0071-derived-statements-from-mirrored-pages.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md)

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

*Current text: [17](../architecture/17-federation-and-publication.md) §4.1.*

### 2. Rows

*Current text: [17](../architecture/17-federation-and-publication.md) §4.2.*

### 3. Data pages

*Current text: [17](../architecture/17-federation-and-publication.md) §4.3.*

### 4. Writing to the destination

*Changed by A1.*

*Current text: [17](../architecture/17-federation-and-publication.md) §4.4.*

### 5. Generated pages from kits (uses 0062 §6)

*Current text: [17](../architecture/17-federation-and-publication.md) §4.5.*

### 6. The destination's template and table

*Current text: [17](../architecture/17-federation-and-publication.md) §4.6.*

### 7. Publications are not proposals (uses 0067 §1)

*Current text: [17](../architecture/17-federation-and-publication.md) §4.7.*

### 8. API, operations and storage (extends 0012 §5; extends 0013 §5.6)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.15, §5; [18](../architecture/18-api.md) §3.2.*

### 9. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

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

## Amendment log

### A1. A publication runs under a subsidiary account

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §4
- **Summary:** A publication runs under a subsidiary account that holds its credential, as every job runs under a subsidiary (PENDING C14), not as an instance-run job; 0067 §6's rule is scoped to proposals ("never pushes a proposal under its own name"), so a publication writing under the account the destination granted does not breach it. (PENDING F14)

Replaced text (§4):

> - **The job is an instance-run job per publication** ([0040](0040-instance-prerogatives.md) §6), with `job/start` and checkpoints, so its runs, edit counts and conflicts are on its job page.

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
