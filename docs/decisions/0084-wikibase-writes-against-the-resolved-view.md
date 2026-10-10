# 0084. Wikibase writes against the resolved view

- **Status:** Proposed
- **Date:** 2026-10-09
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [Wikibase data model and ontology contract](../api/wikibase-compat.md), [MediaWiki compatibility contract](../api/mediawiki-compat.md), [0082](0082-source-form-and-the-shared-view.md), [0083](0083-write-path-in-three-tiers.md)

## Context

The milestone that validates the core use case is a Wikidata-scale mirror with local data and other providers, edited through the Action API that Pywikibot, WikibaseIntegrator and WikidataIntegrator use. Milestone 1 built that API for a single-source tenant and the three libraries pass their checks in CI. The review of 2026-10-09 ([docs/audits/architecture-review-2026-10-09.md](../audits/architecture-review-2026-10-09.md), F-16-1) found that nothing yet says what a Wikibase write *means* when the entity the client fetched is the resolved view of several graphs. [0012](0012-api-requirements.md) §4 listed the write modules as additive deltas over the compatibility contract; [0002](0002-source-graphs-and-mass-ingest.md) §8.2 defined the local-graph operations `add`, `remove` and `override`, the last of which changes a rank or a term or suppresses a statement, not a value; [0004](0004-identity-clusters-and-equivalence.md) §9 said API writes resolve through the statement UUID. Between them lay the cases a bot hits on its first run: WikidataIntegrator and Pywikibot's `editEntity` send the whole entity, the full `claims` list as fetched with GUIDs, plus their additions, and Wikibase merges that; a changed value on a GUID a mirror graph owns had no representable outcome; `clear=true`, `wbremoveclaims`, `wbsetaliases` with `remove` and `wbsetsitelink` on mirrored content each needed a sentence.

Revision identity had its own contradictions (F-16-2, F-04-10). [0006](0006-log-integrity-and-erasure.md) §8 checked `baserevid` for a write to a foreign entity "across the source partitions", so on a `full` mirror following Wikidata's stream every local write to an actively edited item was a spurious `editconflict`, on hub items at Wikidata's edit rate, and under weekly dump syncs on every item the sync touched since the bot's read. The compatibility contract the API builds on says Wikibase *patches* around a stale base and fails only when the patch cannot apply. [0015](0015-record-format-and-partition-registry.md) §2 defined `lastrevid` as the newest record in any source partition "chosen by append time", which is neither monotonic nor from one space, while [0013](0013-postgres-storage.md) §5's `local_revid` column said the opposite. And no inbound `maxlag` was defined, so a bot's `maxlag=5` had nothing to throttle on during a bulk load.

James asked how often a tenant's writes would overlap Wikidata's. The answer is that it depends on how the mirror is fed more than on Wikidata's edit rate — rare per edit under a stream, bursty under dump syncs — and that the conflict protects nothing either way: a mirror `put` lives in a different graph from the local assertion and cannot have been what the client was reasoning about. `baserevid` exists to catch the client's stale view of local state.

### Direction

James's direction, from the design review of 2026-10-09:

- **"Value change on a mirror owned GUID should be refused."**
- On the conflict rule: **"how often is a given tenant going to have a totally overlapping write path with Wikidata?"** — and, on the per-graph answer and the three tiers together, **"I think your proposed solution is sound."**

## Decision

### 1. Every write is diffed against the resolved view first (amends 0012 §4; extends 0002 §8.2)

A `wb*` write, and a REST write that maps to one ([0012](0012-api-requirements.md) §5), is first **normalized** to the internal form ([0082](0082-source-form-and-the-shared-view.md) §2) and then **diffed against the tenant's resolved view of the entity** in source form, which is what the client fetched before the response rewrite. Content the submission carries unchanged — every mirrored statement a client echoes back, every term it did not touch — is dropped before a change set is built. An echoed entity with no changes is a no-op that returns the current entity and `lastrevid`, as Wikibase's "nochange" does. The diff needs the local graph's own state as well as the fused view, which [0083](0083-write-path-in-three-tiers.md) §4 stores.

### 2. What each module becomes (amends 0012 §4; uses 0002 §8.2)

The remainder of the diff maps to local-graph operations by **who owns what it touches**. A statement, term, alias or sitelink is *local-owned* when the local graph asserted it (its UUID, or the term, is in `view.graph_state` for `local`), *mirror-owned* when a mirror or source graph did.

| Submitted change | Local-owned | Mirror-owned |
|---|---|---|
| New statement, term, alias or sitelink | `add` | `add` (a local assertion beside the mirrored ones; fusion applies) |
| Changed value of a statement (main snak, qualifiers, references) | `add` of the new state under the same UUID | **refused, `ts-foreign-statement`** (§3) |
| Changed rank | `add` | `override{rank}` |
| Changed label, description or alias text | `add` | `override{term}` |
| Removed statement (`wbremoveclaims`, absent from an edited `claims` list with `clear`) | `remove` | `override{suppress}` |
| Removed alias (`wbsetaliases` with `remove`) | `remove` | `override{suppress}` of that alias |
| Replaced sitelink (`wbsetsitelink`, `wbeditentity` `sitelinks`) | `add` | `override{sitelink}`: the local link wins per host ([0026](0026-sitelinks.md) §5) |
| `clear=true` | `remove` of every local assertion | nothing: `clear` never touches a mirror graph |
| `wbsetclaim` on a GUID the entity does not hold | `add` with the submitted GUID | — |

`wbcreateclaim`, `wbsetclaimvalue`, `wbsetqualifier`, `wbremovequalifiers`, `wbsetreference`, `wbremovereferences`, `wbsetlabel`, `wbsetdescription`, `wbsetaliases`, `wbsetsitelink`, `wblinktitles`, `wbmergeitems` and `wbcreateredirect` each reduce to one or more rows of the table; the chapter ([18](../architecture/18-api.md)) lists them. `wbmergeitems` between a local and a mirrored item is a `same-as` ([0004](0004-identity-clusters-and-equivalence.md) §9), never a copy.

### 3. A value change on a mirror-owned statement is refused (amends 0002 §7 and §8.2 and 0003 §8; extends 0012 §4)

A client that changes the main snak, a qualifier or a reference of a statement a mirror graph owns is refused with **`ts-foreign-statement`**, naming the statement's graph and the two things the client can do instead: suppress it and add its own (`override{suppress}` plus `add`, which the UI offers as one action), or propose the change upstream ([0067](0067-proposals.md)). The alternative, silently suppressing and re-adding under a new GUID, would leave the client holding a GUID that no longer exists and bend [0004](0004-identity-clusters-and-equivalence.md) §9's rule that the UUID is the identity; a refusal is honest and a documented `override{value}` can be added later if the case proves common. Projected page statements keep their own refusal, `ts-derived-statement` ([0038](0038-page-metadata-and-categories.md) §2).

### 4. The response is the recomposed entity (extends 0012 §4; uses 0083 §2)

The write response carries the entity **recomposed after commit** ([0083](0083-write-path-in-three-tiers.md) §2) and rewritten to the request's form ([0082](0082-source-form-and-the-shared-view.md) §3), with `lastrevid` as §5 defines it. It is the stored entity, not the base plus the change, so the compatibility contract's quirk (a patched `wbeditentity` response missing concurrent additions) does not apply here.

### 5. Revision identity: `lastrevid`, the base check, patching and `maxlag` (amends 0006 §8, 0012 §2.1 and §4, 0013 §5.1 and 0015 §2; extends 0012 §2)

**`lastrevid`** of an entity is the revision ID of its newest **local** record when the key has one, and otherwise the provider-ranged ID of its newest mirror record. It is the `local_revid` column of `view.entity`; [0015](0015-record-format-and-partition-registry.md) §2's "newest record in any source partition, chosen by append time" is withdrawn. `prop=revisions` and the history view still show mirror records in their place; `lastrevid` is what a client bases a write on, and a client bases a write on local state.

**The base check is per source graph.** A `baserevid` that decodes to the tenant's local partition is checked against the local partition's newest record for the key, as [0006](0006-log-integrity-and-erasure.md) §8 always did. A `baserevid` that decodes to a mirror partition asserts only that the key still has **no local record**; a mirror advancing is never a conflict, because nothing a local assertion depends on changed. The one operation whose semantics depend on mirrored state, an `override` naming a specific mirrored statement, is checked by whether that statement still exists in the graph's current state, not by a revision number.

**Patching.** A stale local base is accepted when the change set touches no statement UUID, term, alias or sitelink that a later local record touched, and the response carries the warning `wikibase-conflict-patched`, as the compatibility contract describes; otherwise the write fails with `editconflict`, and the error names the current `lastrevid`. A base that names a state compaction has since replaced is `editconflict`. Statement modules on disjoint GUIDs therefore never conflict, as in Wikibase. A base is still required for a wholesale replacement ([0002](0002-source-graphs-and-mass-ingest.md) §8.2).

**`maxlag`.** The API honours inbound `maxlag` as MediaWiki does: the reported lag is the larger of replica lag ([0013](0013-postgres-storage.md) §7) and the tenant's local-partition composition lag ([0083](0083-write-path-in-three-tiers.md) §2) in seconds, so a bot's `maxlag=5` throttles it during a bulk load or a large sync.

### 6. Crates (extends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-wikibase-changeset` | The diff against a resolved view and the ownership-aware mapping of §2; `ts-foreign-statement` |
| `scatter-ingest` | The per-graph base check and the patch rule of §5 |
| `triplespace-api-action` | The `wb*` modules over §1–§4; `wikibase-conflict-patched`; `maxlag` |
| `triplespace-api-rest` | The same for the REST write routes |
| `triplespace-ui` | Suppress-and-add as one action on a refused value change |

## Alternatives considered

- **Merge the submitted `claims` list as `add` operations.** Copies every mirrored statement into the local graph on every bot write: the stale-copy problem [0002](0002-source-graphs-and-mass-ingest.md) §7 and [0004](0004-identity-clusters-and-equivalence.md) §9 exist to prevent.
- **Suppress-and-add on a value change to a mirrored statement.** Works, but changes the GUID the client holds and bends the UUID-is-identity rule; left as a possible later `override{value}`.
- **Keep the cross-partition base check.** Protects nothing (a mirror record is not what a local client reasoned about) and conflicts at the mirror's sync rate.
- **Strict rejection without patching.** Simpler, but a behavioural difference from Wikibase that a client cannot discover, and the libraries do not retry on `editconflict`.

## Consequences

- **The three client libraries are the acceptance test.** `editEntity` with an echoed entity is a no-op; a run that adds statements to actively edited Wikidata items no longer fails on `editconflict`; a value change on a mirrored statement fails with a code the libraries surface.
- **`lastrevid` is monotonic per entity within the local sequence** and only jumps to a ranged value for an entity with no local history, which is what [0013](0013-postgres-storage.md) §5 already assumed.
- **`clear=true` is safe on a fused entity.** It cannot remove mirrored content.
- **A refused value change is a visible seam** between Wikibase and Triplespace; the statement UI turns it into one action, and the API error says what to do.

## Open questions

- **Q1. `override{value}`.** Whether a documented value override on a mirrored statement, keeping the UUID, should exist once the refusal of §3 has been observed in use.
- **Q2. `ignoreconflicts` on `wbmergeitems`** between two local items follows Wikibase; between a local and a mirrored item the merge is a `same-as`, and whether the parameter means anything there is open.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §7 | §3 | amends | 0002 A33 |
| [0002](0002-source-graphs-and-mass-ingest.md) §8.2 | §1, §2, §3, §5 | amends | 0002 A33 |
| [0003](0003-statement-ui.md) §8 | §3, §5 | amends | 0003 A16 |
| [0004](0004-identity-clusters-and-equivalence.md) §9 | §2 | extends | 0004 A18 |
| [0005](0005-crate-organization.md) §2 | §6 | extends | 0005 A91 |
| [0006](0006-log-integrity-and-erasure.md) §8 | §5 | amends | 0006 A21 |
| [0012](0012-api-requirements.md) §2 | §5 | extends | 0012 A66 |
| [0012](0012-api-requirements.md) §2.1 | §5 | amends | 0012 A66 |
| [0012](0012-api-requirements.md) §4 | §1, §2, §3, §4, §5 | amends | 0012 A66 |
| [0013](0013-postgres-storage.md) §5.1 | §5 | amends | 0013 A54 |
| [0015](0015-record-format-and-partition-registry.md) §2 | §5 | amends | 0015 A48 |

## References

- [MediaWiki compatibility contract](../api/mediawiki-compat.md): `baserevid` and conflict handling, `wikibase-conflict-patched`, `maxlag`
- [docs/audits/architecture-review-2026-10-09.md](../audits/architecture-review-2026-10-09.md): findings F-16-1, F-16-2, F-04-10; decisions G16–G17
