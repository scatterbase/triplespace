# 0067. Proposals: offering local changes to the wiki they came from

- **Status:** Proposed
- **Date:** 2026-10-05
- **Updated:** 2026-10-09 (A6)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0019](0019-discussions.md), [0021](0021-notifications.md), [0025](0025-oauth-server.md), [0027](0027-preferences-and-portability.md), [0038](0038-page-metadata-and-categories.md), [0047](0047-special-pages.md)
- **Uses:** [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0040](0040-instance-prerogatives.md), [0049](0049-boards.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [0060](0060-scopes.md), [0061](0061-sprints-and-tasks.md)
- **Chapters:** [07](../architecture/07-actors-and-accounts.md), [09](../architecture/09-security-and-moderation.md), [14](../architecture/14-discussions.md), [16](../architecture/16-logs-feeds-and-notifications.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

Triplespace lets a tenant assert things about entities it does not own: a local `add`, `override` or `remove` on a Wikidata item is a correction in the local graph ([0002](0002-source-graphs-and-mass-ingest.md) §7), and a fork is a local article that stopped following Wikipedia ([0054](0054-forking-a-mirrored-page.md)). Some of those people will want to offer the change back. 0002 §7 saw it coming — "the set of local corrections doubles as a list of fixes to report upstream" — and gave `Special:Corrections` an export per upstream graph ([0047](0047-special-pages.md) §6), but nothing follows the export: whether upstream took the fix, whether the local correction is now redundant, what to do if upstream reverted it.

Two things make this tractable. A local overlay on a foreign entity **already is a diff against upstream**, and the identity clusters of [0004](0004-identity-clusters-and-equivalence.md) and `equivalent-property` (0004 §6) already translate local subjects, values and properties into upstream's. And the instance already logs in people through Wikimedia OAuth ([0007](0007-actor-identity.md) §3) and is an OAuth server itself ([0025](0025-oauth-server.md)); becoming an OAuth *client* with edit scopes is the inverse of what it has.

### Direction

James's direction, from the design discussions of 2026-10-04 and 2026-10-05:

- **Proposals are threads.**
- **OAuth push is a later phase of development**, specified now.
- **`upstream.retire_adopted` is the default.**
- **The default destination is the wiki being mirrored**; where a title has several repositories, the first.
- **Suppress compiles to deprecate.**

## Decision

### 1. A proposal is a thread with a payload

*Changed by A1.*

*Current text: [14](../architecture/14-discussions.md) §5.1.*

### 2. Compiling an entity proposal

*Current text: [14](../architecture/14-discussions.md) §5.2.*

### 3. Records (extends 0019 §1; extends 0011 §6.1 and §8)

*Changed by A1, A3.*

*Current text: [14](../architecture/14-discussions.md) §1.2, §1.5, §5.3; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 4. Export: version 1

*Current text: [17](../architecture/17-federation-and-publication.md) §3.1.*

### 5. State, and closing the loop (amends 0002 §7; extends 0038 §9; extends 0021 §2)

*Changed by A4.*

*Current text: [14](../architecture/14-discussions.md) §5.4.*

### 6. Push as the person: the later phase (extends 0025; extends 0027 §2)

*Changed by A2, A5.*

*Current text: [17](../architecture/17-federation-and-publication.md) §3.2.*

### 7. Pages, UI and API (extends 0012 §5; extends 0047 §6)

*Current text: [18](../architecture/18-api.md) §2.3, §3.2; [19](../architecture/19-site-ui.md) §1.3, §3.1, §6.10.*

### 8. Permissions and limits

*Current text: [07](../architecture/07-actors-and-accounts.md) §6.4, §7.7; [09](../architecture/09-security-and-moderation.md) §7.2, §8.10.*

### 9. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Alternatives considered

- **A record kind of its own** for proposals. It would duplicate threads' discussion, watching, feeds, listing and notifications; three operations on the thread payload are enough.
- **A proposal's state as a thread status** set by posts. Adoption is observed by the sync, not asserted by a person; a projected state with a `declined` post for the one human verdict keeps 0019 §6's rule that statuses are set in public by named actors.
- **Pushing under a tenant bot account.** Rejected by direction and by [0025](0025-oauth-server.md)'s principle: attribution, bot policy, and the instance holding a credential for other people's work.
- **Removing suppressed statements upstream.** A suppression here is a local judgement; deprecation upstream keeps the statement and the history, and deletion stays a decision made there.
- **Export only, forever.** Enough for many tenants, but a wiki whose editors fix Wikidata every day should not have to paste; the push is specified so the record shape is right from the start.

## Consequences

- **Local corrections have somewhere to go**, and come back as adopted or reverted: the overlay stops being a pile of unsynchronized opinions.
- **Attribution is the person's** in both phases; the instance is a tool.
- **Version 1 needs no credential**, and `Special:Corrections` loses nothing: its export becomes a proposal.
- **The default retires adopted assertions**, so a tenant that proposes a lot converges on upstream by itself; a tenant that wants to keep its overlay sets `upstream.retire_adopted = false`.
- **Test plan.** Translation: a value in a cluster, a local property with an equivalent, an untranslatable value, a suppressed statement → deprecated rank; the QuickStatements text round-trips through QuickStatements' own parser; a `put` carrying the proposed claim moves the state to `adopted` and retires the assertion as an instance act; a later `put` without it moves to `reverted` and offers restoration; `submit` without a grant refused while `proposals.push` is off.

## Open questions

- **Q1. Other destinations.** Proposing to a wiki the subject is not mirrored from — a local entity offered to Wikidata as a new item — which is creation, not correction, and needs the created ID written back as a `same-as`.
- **Q2. New items.** Whether a proposal may create an upstream entity (QuickStatements' `CREATE`) for a local entity with no cluster member, with the loop linking the result.
- **Q3. Partial adoption's grace period** and whether a partly adopted proposal should re-offer its remainder automatically.
- **Q4. Declines from upstream.** Detecting a reverted push as a decline when the reverting edit's summary says so, rather than waiting for a human to mark it.
- **Q5. Pushing qualifiers and references** onto statements upstream already has, which `wbeditentity` can do only by resending the statement whole.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §7 | §5 | amends | 0002 A20 |
| [0005](0005-crate-organization.md) §2 | §9 | amends | 0005 A70 |
| [0011](0011-logs.md) §6.1, §8 | §3 | extends | 0011 A20 |
| [0012](0012-api-requirements.md) §5 | §7 | extends | 0012 A47 |
| [0019](0019-discussions.md) §1 | §3 | extends | 0019 A14 |
| [0021](0021-notifications.md) §2 | §5 | extends | 0021 A10 |
| [0025](0025-oauth-server.md) §1 | §6 | extends | 0025 A5 |
| [0027](0027-preferences-and-portability.md) §2 | §6 | extends | 0027 A4 |
| [0038](0038-page-metadata-and-categories.md) §9 | §5 | extends | 0038 A10 |
| [0047](0047-special-pages.md) §6 | §7 | extends | 0047 A11 |

## References

- [Help:QuickStatements](https://www.wikidata.org/wiki/Help:QuickStatements), the V1 syntax; [Wikibase API: wbeditentity](https://www.wikidata.org/w/api.php?action=help&modules=wbeditentity)
- [OAuth/For Developers](https://www.mediawiki.org/wiki/OAuth/For_Developers) and [OAuth 2.0 on Wikimedia](https://www.mediawiki.org/wiki/OAuth/Owner-only_consumers): scopes and the authorization flow
- [Wikidata:Bots](https://www.wikidata.org/wiki/Wikidata:Bots) and `maxlag`
- [0002](0002-source-graphs-and-mass-ingest.md) §7, [0047](0047-special-pages.md) §6: the correction list this ADR completes; [0054](0054-forking-a-mirrored-page.md) §6: the upstream diff the page kind builds on

## Amendment log

### A1. Page proposals

- **Date:** 2026-10-05
- **Source:** [0068](0068-merging-with-upstream.md) §3
- **Change:** extends §1, §3
- **Summary:** The page kind's payload and destination are defined by 0068.

### A2. The grant serves talk pages

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §5
- **Change:** extends §6
- **Summary:** The upstream grant and client are built with 0069, whose upstream replies and sections are their first use; `proposals.push` stays off until this section is implemented.

### A3. A proposal's fields are page metadata of the thread page

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §3
- **Summary:** A proposal's kind, destination, base, payload, omitted and flags are **page metadata of the thread page**, set by the `propose` operation and projected to `view.proposal`; they are not fields of `create`, whose `target` stays the thread's home ([0019](0019-discussions.md) §4). (PENDING F7)

Replaced text (§3):

> A `create` may carry `propose`'s fields directly (as 0049's `create` carries `also`), so one record opens a proposal thread.

### A4. Who may set `declined`

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §5
- **Summary:** On a proposal thread `declined` may be set only by the proposer or a holder of `edit` on the subject's talk page; any other `declined` post is refused with `ts-proposal-status`. [0019](0019-discussions.md) §6 notes the exception. (PENDING F8)

Replaced text (§5):

> | `declined` | Set by a post with the `declined` status ([0019](0019-discussions.md) §6), by the proposer or an `edit` holder, when the destination community said no |

### A5. Publications run under a subsidiary; the rule is scoped to proposals

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §6
- **Summary:** A publication ([0074](0074-publishing-a-scope-to-an-external-wiki.md) §4) runs under a subsidiary account that holds its credential (PENDING C14); §6's rule is scoped to proposals: the instance never pushes a proposal under its own name. (PENDING F14)

Replaced text (§6):

> **The instance never holds a shared upstream account** and never pushes under its own name: attribution belongs to the person, the destination's bot policy applies to them, and a tenant's reputation upstream is its editors'.

### A6. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [07](../architecture/07-actors-and-accounts.md), [09](../architecture/09-security-and-moderation.md), [14](../architecture/14-discussions.md), [16](../architecture/16-logs-feeds-and-notifications.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
