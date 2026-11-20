# 0112. Triplespace links RevisionChest's library crates

- **Status:** Proposed
- **Date:** 2026-11-20
- **Author:** James Hare / Claude Opus
- **Uses:** [0011](0011-logs.md), [0015](0015-record-format-and-partition-registry.md), [0054](0054-forking-a-mirrored-page.md), [0092](0092-following-recent-changes.md)

## Context

[0033](0033-backend-stack.md) began from four fixed points, the fourth being that RevisionChest, the Internet Archive's tool for turning a wiki's XML history dumps into `.mwrev.zst` revision bundles, "stays a separate binary", and that Triplespace matures its own upstream sync, to be contributed to RevisionChest later. 0033 §10 held that it had no library target, so Triplespace did not link it, and planned an independent reader of the bundle format in `scatter-adapter-mediawiki` ([0053](0053-mirrored-pages.md) §12). That reader was never written.

By November 2026 the two projects carried overlapping code:

- **Two MediaWiki XML readers.** RevisionChest's `mwdump` streams one revision at a time, from `.xml`, `.bz2` (on several cores), `.gz` or `.7z`. Triplespace's `scatter-mediawiki-xml` ([0005](0005-crate-organization.md) A119) hands over a whole page with all its revisions, which does not work for a full-history dump where one page has a very large number of revisions; it alone read content slots and hidden comments, and wrote dumps.
- **Two recent-changes followers.** Triplespace's poller (`scatter-adapter-mediawiki`'s `recentchanges`, [0092](0092-following-recent-changes.md) §1) has settle and tail windows, deduplication by `rcid` and log rows. RevisionChest's `sync` had none of these, and resumed only within the 30 days the wiki keeps recent changes.

RevisionChest had by then been split into four crates, with library targets: `mwdump`, `mwrev` (the bundle format), `revisionchest-meta` and the binary. `mwrev` was published to crates.io at 0.1.0 on 2026-11-20; `mwdump` gained slots, hidden comments and Triplespace's writer, and the poller became the crate `mwrecent`, with coalescing by title optional, both awaiting publication. All are GPL-3.0-or-later, as Triplespace is.

### Direction

James's direction, recorded in SCA-445 on 2026-11-19:

- **"James decided on 2026-11-19 that Triplespace depends on its crates."**

And his answers of 2026-11-20 to the choices put while drafting: Triplespace's crates depend on `mwdump` directly, with no wrapper crate; bundles are read through `mwrev` in `scatter-adapter-mediawiki`; live sync beyond the poller is still meant to move to RevisionChest later; and `mwdump` uses the workspace's `quick-xml` before Triplespace's callers move to it.

## Decision

### 1. Triplespace depends on RevisionChest's library crates (amends 0033 §10)

*Current text: [13](../architecture/13-mirrored-pages.md) §3.5; [22](../architecture/22-crates-and-stack.md) §4.11.*

### 2. One reader and writer of MediaWiki's export XML: `mwdump`

*Current text: [22](../architecture/22-crates-and-stack.md) §4.11.*

### 3. The recent-changes poller is `mwrecent` (uses 0092 §1)

*Current text: [13](../architecture/13-mirrored-pages.md) §3.5; [22](../architecture/22-crates-and-stack.md) §4.11.*

### 4. No independent bundle reader (amends 0053 §12)

*Current text: [13](../architecture/13-mirrored-pages.md) §3.4, §3.5.*

### 5. The two stores stay separate

*Current text: [13](../architecture/13-mirrored-pages.md) §3.5.*

### 6. Crates (amends 0005 §2; amends 0053 §12; amends 0092 §8)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **Keep both readers and both pollers.** Each project fixes its own bugs, and the overlap grows: RevisionChest's `sync` lacked what Triplespace's poller already had, and Triplespace's reader could not stream a full-history page.
- **Move RevisionChest's code into the `scatter-*` crates.** RevisionChest is the Internet Archive's project, used outside Triplespace, and its binary needs the readers; the code would have to live in two places again.
- **A thin `scatter-mediawiki-xml` over `mwdump`.** One place to absorb `mwdump`'s API changes, at the cost of a crate that only re-exports; James chose direct dependencies.

## Consequences

- **Triplespace's build takes three crates from another project's release cycle.** A breaking release of `mwdump`, `mwrev` or `mwrecent` reaches Triplespace as a Dependabot pull request of its own (22 §3.4), and their public types are settled before they are published.
- **Fixes go to one place.** A bug in reading a dump or following a wiki is fixed in RevisionChest, and both RevisionChest's `build` and `sync` and Triplespace's import, adoption and followers get it.
- **The switch-over is behaviour-preserving.** Follow and adopt runs against the Librarybase samples give the same results before and after; `mwdump` and `scatter-mediawiki-xml` were compared field by field on the sample dumps before this ADR.

## Open questions

- **Q1. The retired crate in the map.** `crates.toml`'s `[[retired]]` entries name the crate that now holds a retired one, and the checker requires it to be in the map; `mwdump` is not a workspace crate. When `scatter-mediawiki-xml` is deleted, either its entry goes with no `[[retired]]` row, or the map learns external successors.

## References

- [RevisionChest](https://github.com/internetarchive/RevisionChest): `mwdump`, `mwrev`, `mwrecent`, `revisionchest-meta` and the binary
- [`mwrev` on crates.io](https://crates.io/crates/mwrev)
- SCA-441 and its sub-issues: SCA-442 (publish `mwrev`), SCA-443 (`mwrecent`), SCA-444 (`mwdump`'s slots, hidden comments and writer), SCA-449 (`sync` on `mwrecent`), SCA-450 (publish `mwdump` and `mwrecent`), SCA-454 (the switch-over)
