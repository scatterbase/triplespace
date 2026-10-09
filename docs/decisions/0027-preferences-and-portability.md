# 0027. Preferences, private state and portability

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A6)
- **Author:** James Hare / Claude Fable
- **Changes:** [0003](0003-statement-ui.md), [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0018](0018-tenants.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md)
- **Uses:** [0016](0016-permissions-and-access-control.md), [0024](0024-subsidiary-accounts.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

Seven ADRs have each put a little of a person's settings somewhere. [0010](0010-site-ui.md) §11 has a Preferences section with interface and label languages and two defaults; [0012](0012-api-requirements.md) §4 gives them `action=options` keys; [0019](0019-discussions.md) §6 has a viewer's thread visibility rule; [0020](0020-change-feeds.md) §3 puts four auto-watch flags "beside the watch set" and leaves the rest open; [0021](0021-notifications.md) §3 has a reason-by-channel matrix in its own table; [0003](0003-statement-ui.md) §3 has shape pins whose home was never chosen. None of them is a record, because a preference is nobody's business but its holder's ([0007](0007-actor-identity.md) §8, [0013](0013-postgres-storage.md) §4), and none of them is rebuildable from the log for the same reason.

That raises the question James asked: what happens to all of it when a person, or a whole wiki, moves? [0018](0018-tenants.md) §10 moves a tenant's six partitions and says nothing private travels; [0020](0020-change-feeds.md) §3 tells a moving user to download their watchlist with `list=watchlistraw` and re-import it. That is a poor transition, and it is the only one the ADRs offer. A hosting change should be **trusty**: a person should arrive on the new instance with their settings, their watchlist and their notification choices intact, without having to know in advance which of a dozen things to export.

This ADR does three things: it names every kind of private state and puts preferences in one place; it defines a portable, documented **user data bundle** that a person can download and import; and it adds a **private extract** to the tenant move, so that a cooperative transition carries each account's state operator-to-operator and applies it when the account is reclaimed.

## Decision

### 1. One preference store, one key registry

*Current text: [07](../architecture/07-actors-and-accounts.md) §8.1.*

### 2. What private state is, and which of it is portable

*Changed by A3, A4, A5.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §8.3.*

### 3. The user data bundle

*Current text: [07](../architecture/07-actors-and-accounts.md) §6.2, §8.4.*

### 4. The private extract in a tenant move (extends 0018 §10; extends 0007 §4)

*Current text: [07](../architecture/07-actors-and-accounts.md) §3.2, §8.5.*

### 5. Shape pins (settles 0003 Q2)

*Current text: [07](../architecture/07-actors-and-accounts.md) §8.2.*

### 6. API (extends 0012 §4 and §5)

*Current text: [18](../architecture/18-api.md) §2.3, §3.2.*

### 7. UI (extends 0010 §11)

*Current text: [19](../architecture/19-site-ui.md) §2.4.*

### 8. Storage (extends 0013 §4 and §5.6)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.16, §5.*

### 9. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **Every setting has one home and one schema**, so `action=options`, the account page and the bundle cannot disagree about what a preference is.
- **A person can leave with their state**, on their own terms, and arrive with it on a cooperative move without doing anything but reclaiming their account. This is the trusty transition [0018](0018-tenants.md) §10 was missing.
- **Nothing private is ever in a public bundle**, and the classification is enforced by the schema, so a new private table has to declare what it is before it can exist.
- **Secrets never travel.** Keys and tokens are reissued and bindings are re-proved; a compromised export cannot log anyone in.
- **Two more open questions close**: shape pins ([0003](0003-statement-ui.md) Q2) and the rest of preferences ([0020](0020-change-feeds.md) Q3).
- **Exports are a compatibility surface.** The bundle's payload type is versioned like a record's; a change needs a new version and an importer that reads the old one.

## Open questions

- **Q1. The encryption scheme for the private extract**: age-style sealing to an X25519 key supplied for the move is the obvious choice, and the receiving instance's Ed25519 signing key must not be reused for it.
- **Q2. Inbox in the self-service bundle by default**, or only on request, given its size and its references to records the new instance may not hold.
- **Q3. Per-device preferences** (a phone and a desktop), which MediaWiki also lacks.
- **Q4. Anonymous preferences**, held in a cookie for readers who are not logged in, as MediaWiki's client-side preferences are.
- **Q5. Retention of an unreclaimed account's extract**: the grace period of [0018](0018-tenants.md) §10 is not yet a number.
- **Q6. Third-party import**: whether to read a MediaWiki `list=watchlistraw` dump or an Echo preference export directly, for people arriving from a Wikibase.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0003](0003-statement-ui.md) §3 | §5 | extends | 0003 A2 |
| [0003](0003-statement-ui.md) Q2 | §5 | settles | 0003 Q2 |
| [0005](0005-crate-organization.md) §2 | §9 | extends | 0005 A23 |
| [0007](0007-actor-identity.md) §4, §8 | §2, §4 | extends | 0007 A10 |
| [0010](0010-site-ui.md) §11 | §7 | amends | 0010 A17 |
| [0012](0012-api-requirements.md) §4, §5 | §6 | extends | 0012 A16 |
| [0013](0013-postgres-storage.md) §5, §5.2, §5.6 | §8 | amends | 0013 A9 |
| [0013](0013-postgres-storage.md) §4, §5.4, §5.5, §7, §8 | §8 | extends | 0013 A9 |
| [0015](0015-record-format-and-partition-registry.md) §3, §5 | §1, §5 | extends | 0015 A10 |
| [0018](0018-tenants.md) §10 | §4 | extends | 0018 A3 |
| [0019](0019-discussions.md) §6 | §1 | amends | 0019 A5 |
| [0020](0020-change-feeds.md) §3 | §1, §3–4 | amends | 0020 A5 |
| [0020](0020-change-feeds.md) Q3 | §1 | settles | 0020 Q3 |
| [0020](0020-change-feeds.md) Q5 | §1 | settles | 0020 Q5 |
| [0021](0021-notifications.md) §3 | §1–4, §8 | amends | 0021 A4 |

## References

- [Manual:User_properties table](https://www.mediawiki.org/wiki/Manual:User_properties_table) and [Manual:$wgDefaultUserOptions](https://www.mediawiki.org/wiki/Manual:$wgDefaultUserOptions)
- [API:Options](https://www.mediawiki.org/wiki/API:Options) and [API:Userinfo](https://www.mediawiki.org/wiki/API:Userinfo)
- [Wikimedia: Right to data portability](https://foundation.wikimedia.org/wiki/Policy:Privacy_policy) (privacy policy)
- [age encryption](https://age-encryption.org/) (sealing to an X25519 recipient)
- [0018 — Tenants](0018-tenants.md) §10, [0020 — Change feeds](0020-change-feeds.md) §3, [0021 — Notifications](0021-notifications.md) §3

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §9
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A23).

Replaced text (§9):

> | Crate | Change |
> |---|---|
> | `triplespace-db` | The portability class on each `private` table (§2) |
> | `triplespace-accounts` | `private.preference` and the key registry, embedding `docs/registry/preferences.toml`; the user data bundle, export and import (§3); applying a private extract on reclaim (§4) |
> | `triplespace-notify` | Reads `notifications.*` preferences in place of `notification_pref` |
> | `triplespace-projections` | `view-pin` records (§5) |
> | `triplespace-api-action`, `triplespace-api-rest` | §6 |
> | `triplespace-cli` | `tenant export` produces the private extract; `tenant import` seals and stores it for reclaim (§4) |
>
> No crate is added. The bundle format is a Triplespace surface, not a shared seam, so nothing enters a `scatter-*` crate.

### A2. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §9
- **Summary:** The open questions were numbered and §9 became a stub. No decision changed; nothing had amended this ADR. The file before conversion is commit `0b26a3a`.

### A3. Portability classes as table comments

- **Date:** 2026-10-02
- **Source:** Direct: James, review of 2026-10-02 (`triplespace-db`)
- **Change:** amends §2
- **Summary:** The class is a table comment `portability: portable | re-established | never-leaves`, checked by `triplespace-db` against its own list so that an unclassified or misclassified table fails; a table with columns of two classes takes the stricter one and the export reads only its portable columns.

Replaced text: none; §2 gains two sentences.

### A4. Upstream grants

- **Date:** 2026-10-05
- **Source:** [0067](0067-proposals.md) §6
- **Change:** extends §2
- **Summary:** `private.upstream_grant` is re-established, not carried.

### A5. Passwords are re-established

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §2
- **Summary:** `private.password` is in the "re-established, not carried" class: a password hash is a secret the instance holds for itself, so it does not travel in a bundle or a private extract and the person sets a new one on reclaim. (PENDING C5)

### A6. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
