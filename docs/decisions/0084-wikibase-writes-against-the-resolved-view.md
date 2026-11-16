# 0084. Wikibase writes against the resolved view

- **Status:** Proposed
- **Date:** 2026-10-09
- **Updated:** 2026-10-30 (A1)
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

*Current text: [00](../architecture/00-overview.md) §2.6; [18](../architecture/18-api.md) §2.5.*

### 2. What each module becomes (amends 0012 §4; uses 0002 §8.2)

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.2; [12](../architecture/12-files-and-media.md) §8.4; [18](../architecture/18-api.md) §2.3, §2.5.*

### 3. A value change on a mirror-owned statement is refused (amends 0002 §7 and §8.2 and 0003 §8; extends 0012 §4)

*Current text: [05](../architecture/05-providers-and-ingest.md) §2.5, §3.2; [18](../architecture/18-api.md) §2.5; [19](../architecture/19-site-ui.md) §4.7.*

### 4. The response is the recomposed entity (extends 0012 §4; uses 0083 §2)

*Current text: [18](../architecture/18-api.md) §2.5.*

### 5. Revision identity: `lastrevid`, the base check, patching and `maxlag` (amends 0006 §8, 0012 §2.1 and §4, 0013 §5.1 and 0015 §2; extends 0012 §2)

*Current text: [00](../architecture/00-overview.md) §2.6; [01](../architecture/01-log-and-records.md) §2.5, §7; [03](../architecture/03-storage-caches-and-search.md) §4.2; [05](../architecture/05-providers-and-ingest.md) §3.2; [07](../architecture/07-actors-and-accounts.md) §6.3; [18](../architecture/18-api.md) §1.3, §1.5, §2.5; [19](../architecture/19-site-ui.md) §4.7.*

### 6. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

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

## References

- [MediaWiki compatibility contract](../api/mediawiki-compat.md): `baserevid` and conflict handling, `wikibase-conflict-patched`, `maxlag`
- [docs/audits/architecture-review-2026-10-09.md](../audits/architecture-review-2026-10-09.md): findings F-16-1, F-16-2, F-04-10; decisions G16–G17

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§6
- **Summary:** The Decision's current text now lives in the architecture chapters [00](../architecture/00-overview.md), [01](../architecture/01-log-and-records.md), [03](../architecture/03-storage-caches-and-search.md), [05](../architecture/05-providers-and-ingest.md), [07](../architecture/07-actors-and-accounts.md), [12](../architecture/12-files-and-media.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
