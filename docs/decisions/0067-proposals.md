# 0067. Proposals: offering local changes to the wiki they came from

- **Status:** Proposed
- **Date:** 2026-10-05
- **Updated:** 2026-10-06 (A2)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0019](0019-discussions.md), [0021](0021-notifications.md), [0025](0025-oauth-server.md), [0027](0027-preferences-and-portability.md), [0038](0038-page-metadata-and-categories.md), [0047](0047-special-pages.md)
- **Uses:** [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0020](0020-change-feeds.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0040](0040-instance-prerogatives.md), [0049](0049-boards.md), [0052](0052-page-repositories-and-title-inheritance.md), [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [0060](0060-scopes.md), [0061](0061-sprints-and-tasks.md)

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

**A proposal is a thread** ([0019](0019-discussions.md)) homed on the subject's talk page — the foreign entity's, or the fork's — whose records carry a **proposal payload** (§3): what is offered, to which wiki, compiled from the local state at the moment of proposing. The thread is where the proposal is discussed and where its **state** (§5) is shown; threads give watching, feeds, boards and notifications for free, and a listing board such as `Board:Proposals to Wikidata` with a `thread_statement` scope ([0049](0049-boards.md) §14) lists them all.

Two kinds, one object:

| Kind | Subject | What is proposed | Compiled by |
|---|---|---|---|
| **Entity** | A foreign entity with local assertions ([0002](0002-source-graphs-and-mass-ingest.md) §7) | A Wikibase change set: claims, ranks, terms | §2 |
| **Page** | A fork ([0054](0054-forking-a-mirrored-page.md)) | Wikitext for a destination page on the repository | [0068](0068-merging-with-upstream.md) §3 |

A page proposal's payload, destination and conflict handling are [0068](0068-merging-with-upstream.md) §3's; its adoption test is 0068 §5's.

**The destination wiki** is the one the subject is mirrored from: the entity's provider (Wikidata for a `WD` subject), or the fork's repository; where a title's stack has several repositories ([0052](0052-page-repositories-and-title-inheritance.md) §2), the first in `pages.repos` order. A proposal to any other wiki is Q1.

### 2. Compiling an entity proposal

**The proposer chooses which local assertions to offer**, from the entity page's overlay (the correction chips of [0003](0003-statement-ui.md) §4) or from `Special:Corrections` with checkboxes; the default selection is every active assertion on the subject. Each is **translated** into upstream's terms:

| Local assertion ([0002](0002-source-graphs-and-mass-ingest.md) §7) | Proposed as |
|---|---|
| `add` statement | A new claim, with its qualifiers and references |
| `override` rank | The upstream statement's rank set to the local rank |
| `override` suppress | The upstream statement's rank set to **deprecated** (direction); never a removal, which is a human decision upstream |
| `override` term | The label, description or alias set |
| `add` on a local-only property or with a local-only value | **Untranslatable** (below) |

**Translation** goes through the cluster: a local or other-provider entity used as a value is replaced by its cluster's member on the destination wiki ([0004](0004-identity-clusters-and-equivalence.md) §4), a local property by its `equivalent-property` there (0004 §6), a mirrored `WDP…` by its bare upstream ID. Anything with no member there is **untranslatable**: listed on the proposal with the reason ("`P7` has no equivalent on Wikidata"; "`Q88` is not linked to a Wikidata item"), left out of the payload, and kept in the proposal's `omitted` list so that linking the entity later lets the proposer re-compile. A statement without a reference is **flagged**, not refused: Wikidata's norms want one, the proposer decides.

**The payload** is Wikibase's canonical JSON for `wbeditentity`'s `data` — `claims`, `labels`, `descriptions`, `aliases` with upstream IDs and GUIDs where a statement is changed — plus **QuickStatements V1** text generated from the same change set, so that the person can push with the tool they already use ([0025](0025-oauth-server.md) Context). Both are produced by `scatter-wikibase-changeset` from the selection and the cluster map, deterministically, and stored in the record (§3).

### 3. Records (extends 0019 §1; extends 0011 §6.1 and §8)

*Changed by A1.*

Three operations join the `scatter:v0/thread` payload type, beside 0049's `attach` and `detach`:

| Operation | Content part | Meaning |
|---|---|---|
| `propose` | `kind` (`entity` \| `page`); `target` `{wiki, id}` (provider and upstream ID, or repository and title); `base` (the upstream revision the payload was compiled against); `payload` (§2, or [0068](0068-merging-with-upstream.md) §3's text and destination); `omitted`; `flags` | Turns the thread into a proposal, or re-compiles one; the latest `propose` is the proposal |
| `submit` | `method` (`export` \| `push`); `upstream` (the revision IDs the push made, or none for an export); `destination` (for a page) | The proposal was offered: exported for the person to push, or pushed as them (§6) |
| `withdraw` | — | The proposer takes it back |

A `create` may carry `propose`'s fields directly (as 0049's `create` carries `also`), so one record opens a proposal thread. Each needs a base offset; `propose` and `withdraw` need to be by the thread's author or a holder of `edit` on the subject's talk page; `submit` by the person who pushed or exported. **Validation**: a `propose` on a subject that is not foreign (entity) or not a fork (page) is refused (`ts-proposal-subject`); a `submit` with `method: push` without a grant (§6) is refused.

**Logs**: `proposal/propose`, `proposal/submit`, `proposal/withdraw`, projected from the records, visible to everyone, typed `as:Offer` (propose, with `as:target` the upstream IRI), `as:Announce` (submit) and `as:Undo` (withdraw).

### 4. Export: version 1

**Export is the first phase and always available.** From the proposal thread: **QuickStatements** (copy or download, with a link that opens QuickStatements with the text pre-filled where the tool supports it), **Wikibase JSON** (download), and for a page the **wikitext** and a **diff** ([0068](0068-merging-with-upstream.md) §4). Exporting appends a `submit` with `method: export` and no `upstream`, so that the proposal is marked offered and the loop of §5 starts watching upstream; the person is told to come back and paste the revision ID, or to let the detector find it. `Special:Corrections`' "export per upstream graph" ([0047](0047-special-pages.md) §6) becomes **Propose**: it opens a proposal thread for the selection, one per subject.

Nothing here needs a credential, a consumer registration or a policy conversation with the destination wiki. A tenant that never enables §6 has a working proposals feature.

### 5. State, and closing the loop (amends 0002 §7; extends 0038 §9; extends 0021 §2)

**A proposal's state is projected**, not set by a post:

| State | When |
|---|---|
| `draft` | A `propose` with no `submit` since |
| `offered` | A `submit` (either method), and upstream not yet seen to carry it |
| `adopted` | The mirror of the target (a `put`, [0002](0002-source-graphs-and-mass-ingest.md) §4, or the page's revision through [0053](0053-mirrored-pages.md) §6) carries every item of the payload: the claim present with the proposed value, the rank as proposed, the term as proposed; for a page, [0068](0068-merging-with-upstream.md) §5's test |
| `partly adopted` | Some items carried, after `proposals.adoption_grace` (default 7 days) since the submit |
| `reverted` | Upstream carried the items and later does not |
| `declined` | Set by a post with the `declined` status ([0019](0019-discussions.md) §6), by the proposer or an `edit` holder, when the destination community said no |
| `withdrawn` | A `withdraw` |

`view.proposal (tenant, thread_id, kind, target, state, submitted_at, upstream_revids, adopted_at, items jsonb)` holds it, written by the mirror and page-mirror projections when the target changes and by the thread projection on records. **The role `proposal-state`**, bound by the tenant to a `string` property as `thread-status` is ([0038](0038-page-metadata-and-categories.md) §9), projects the state as a statement on the thread, so proposals are scope-able: a scope `statement: proposal-state = offered` over threads, a sprint rule, a board.

**Redundant assertions.** When a proposal becomes `adopted`, each local assertion it carried is now **redundant** in 0002 §7's sense: upstream agrees. 0002 §7 said the application "can retire" it; this ADR says how. With **`upstream.retire_adopted = true`** (site; the default, by direction) the instance **retires** each redundant assertion at once — a `remove` of the local assertion as an **instance act** ([0040](0040-instance-prerogatives.md) §7), summary "Adopted upstream in revision {revid}; proposal {thread}" — so the entity's resolved view is upstream's and the overlay stops accumulating. With `false`, they are listed in `Special:Corrections` as `redundant` with **Retire**, as before. If the proposal later becomes `reverted`, a retired assertion is **offered for restoration** on the thread ("Upstream reverted this; restore the local statement?"), one click re-adding it; it is not restored automatically, since the revert may have been right.

**Notifications** ([0021](0021-notifications.md) §2): one reason, `proposal-state`, to the proposer when the state changes to `adopted`, `partly adopted`, `reverted` or `declined`; the activity row is the mirror record or status post that changed it.

### 6. Push as the person: the later phase (extends 0025; extends 0027 §2)

*Changed by A2.*

**Specified now, built later.** `proposals.push` (site, default `off`) is refused until the instance implements this section, and the ADR's status reads `Proposed` until it does ([0050](0050-adr-format.md) §3).

**The instance becomes an OAuth client of the destination wiki with edit scopes.** Wikimedia's OAuth 2 (`editpage`, `createeditmovepage`, `highvolume` where granted) through the identity issuer the tenant already uses for login ([0007](0007-actor-identity.md) §3), as a **second authorization** the person grants from the proposal thread ("Allow this wiki to edit Wikidata as you"), separate from login so that logging in never grants editing. The grant is stored in **`private.upstream_grant (actor_key, wiki, scopes text[], access_hash, refresh_hash, issued, expires)`**, portability class **re-established, not carried** ([0027](0027-preferences-and-portability.md) §2): it is reissued on a new instance and revocable from `Special:Preferences` and from the destination wiki's own settings. **One consumer per instance**, registered with the destination wiki by the operator with the instance's callback; on a farm the callback is the farm base and the tenant is carried in `state`, so a forty-tenant farm registers once.

**Pushing** appends the compiled payload as the person: `wbeditentity` with the JSON for an entity, or an edit to the destination page for a page ([0068](0068-merging-with-upstream.md) §4), with the summary "Proposed at {proposal URL} via {instance}" and the destination's `maxlag` honoured; the resulting revision IDs go into the `submit` record. Rate class `upstream` ([0024](0024-subsidiary-accounts.md) §5), counted per person. **The instance never holds a shared upstream account** and never pushes under its own name: attribution belongs to the person, the destination's bot policy applies to them, and a tenant's reputation upstream is its editors'. A subsidiary ([0024](0024-subsidiary-accounts.md)) may hold a grant of its own when its operator authorizes it, which is how a tenant's reconciliation bot pushes under a flagged upstream bot account that is also the operator's responsibility.

**Talk pages use the grant first.** Replies and new sections sent to a followed talk page ([0069](0069-synchronized-talk-pages.md) §5–6) are the first use of this client, which is therefore built with 0069; pushing a proposal still waits on `proposals.push`.

**For 0025**: the instance is now an OAuth server for its own API ([0025](0025-oauth-server.md)) *and* a client of other wikis' APIs for one purpose; §6's grant table sits beside 0025 §8's consumer and token tables, and the two never share a credential.

### 7. Pages, UI and API (extends 0012 §5; extends 0047 §6)

**The proposal thread** shows, above the posts: the kind and destination, the state chip with its history, the payload as a diff-shaped table (proposed claims with labels; for a page, the diff of [0068](0068-merging-with-upstream.md) §4), the omitted and flagged items, and the actions: **Re-compile**, **Export** (the formats of §4), **Push** (when §6 is on and the person holds a grant), **Withdraw**, **Mark declined**. **The entity page** gains "Propose to {wiki}" in the overflow menu and a proposal count in the About panel ("2 proposals: 1 adopted, 1 offered"). **`Special:Proposals`** (new; group `identity`) lists the tenant's proposals by state, wiki and proposer, and is what a board scoped to `proposal-state` shows without configuration.

**REST**: `POST /proposal` (subject, selection → compiles and opens the thread), `GET /proposal/{thread}` (state, payload, omitted, flags), `POST /proposal/{thread}/export?format=qs|json|wikitext|diff`, `POST /proposal/{thread}/push` (§6), `POST /proposal/{thread}/withdraw`, `GET /entity/{id}/proposals`, `GET /page/{id}/proposals`. **Action API**: `list=proposals`, and `letype=proposal`.

### 8. Permissions and limits

Proposing needs `edit` on the subject's talk page (to open a thread there) and `read` on the subject; exporting needs `read`; pushing needs a grant of one's own. `proposals.max_items` (default 500) bounds a payload; a larger selection is split into several proposals by the form. Edit filters ([0030](0030-edit-filters.md)) see `propose` and `submit` as records with their content parts, so a tenant can require a reference on every proposed claim.

### 9. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-wikibase-changeset` | Compiling a selection of local assertions into an upstream change set through a cluster map: translation, the untranslatable and flagged lists, Wikibase JSON and QuickStatements V1 output |
| `scatter-threads` | The `propose`, `submit` and `withdraw` operations and their fold |
| `triplespace-projections` | `view.proposal`; adoption and reversion detection in the mirror and page-mirror projections; the `proposal-state` role statement; retirement as an instance act |
| `triplespace-accounts` | The upstream OAuth client, grants and `private.upstream_grant` (§6, later phase) |
| `triplespace-upstream` | The push: `wbeditentity` and page edits as the person, `maxlag`, revision IDs back |
| `triplespace-api-action`, `triplespace-api-rest` | The routes and modules of §7 |
| `triplespace-ui` | The proposal thread view, "Propose to…", `Special:Proposals`, "Propose" on `Special:Corrections` |
| `triplespace-notify` | The `proposal-state` reason |

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
