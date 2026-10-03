# 0027. Preferences, private state and portability

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-02 (A3)
- **Author:** James Hare / Claude Fable
- **Changes:** [0003](0003-statement-ui.md), [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0018](0018-tenants.md), [0019](0019-discussions.md), [0020](0020-change-feeds.md), [0021](0021-notifications.md)
- **Uses:** [0016](0016-permissions-and-access-control.md), [0024](0024-subsidiary-accounts.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

Seven ADRs have each put a little of a person's settings somewhere. [0010](0010-site-ui.md) §11 has a Preferences section with interface and label languages and two defaults; [0012](0012-api-requirements.md) §4 gives them `action=options` keys; [0019](0019-discussions.md) §6 has a viewer's thread visibility rule; [0020](0020-change-feeds.md) §3 puts four auto-watch flags "beside the watch set" and leaves the rest open; [0021](0021-notifications.md) §3 has a reason-by-channel matrix in its own table; [0003](0003-statement-ui.md) §3 has shape pins whose home was never chosen. None of them is a record, because a preference is nobody's business but its holder's ([0007](0007-actor-identity.md) §8, [0013](0013-postgres-storage.md) §4), and none of them is rebuildable from the log for the same reason.

That raises the question James asked: what happens to all of it when a person, or a whole wiki, moves? [0018](0018-tenants.md) §10 moves a tenant's six partitions and says nothing private travels; [0020](0020-change-feeds.md) §3 tells a moving user to download their watchlist with `list=watchlistraw` and re-import it. That is a poor transition, and it is the only one the ADRs offer. A hosting change should be **trusty**: a person should arrive on the new instance with their settings, their watchlist and their notification choices intact, without having to know in advance which of a dozen things to export.

This ADR does three things: it names every kind of private state and puts preferences in one place; it defines a portable, documented **user data bundle** that a person can download and import; and it adds a **private extract** to the tenant move, so that a cooperative transition carries each account's state operator-to-operator and applies it when the account is reclaimed.

## Decision

### 1. One preference store, one key registry

**A preference is a key–value pair owned by an account**, stored in `private.preference (actor_key, key, value jsonb, updated)`. The keys are registered in `docs/registry/preferences.toml` ([0015](0015-record-format-and-partition-registry.md) §5), each with a type, a default and a description; an instance may set its own defaults in `site` configuration (`preferences.defaults`) and add keys for features it adds. A value that is not registered, or does not fit its type, is refused. This is MediaWiki's `user_properties` table with `$wgDefaultUserOptions`, under a schema.

The keys the earlier ADRs scattered are gathered here:

| Key | Type | From |
|---|---|---|
| `language`, `label-languages` | language code; list of codes | [0010](0010-site-ui.md) §11 |
| `ts-upstream-fold`, `ts-rc-syncs` | boolean | [0010](0010-site-ui.md) §11, [0012](0012-api-requirements.md) §4 |
| `autowatch.edit`, `autowatch.create`, `autowatch.post`, `autowatch.thread` | boolean | [0020](0020-change-feeds.md) §3 |
| `autowatch.notify` | boolean: auto-watches carry `notify` | [0021](0021-notifications.md) §2 |
| `feeds.period`, `feeds.syncs` | duration; boolean | [0020](0020-change-feeds.md) §1–2 |
| `threads.visibility` | the rule of [0019](0019-discussions.md) §6, as a map from status category and age to visible, collapsed or hidden | 0019 §6 |
| `notifications.{reason}.{channel}` | boolean, one key per cell of the matrix | [0021](0021-notifications.md) §3 |
| `shapes.pins` | a map from property ID to shape (§5) | [0003](0003-statement-ui.md) §3 |
| `timezone`, `date-format`, `editor.font`, `rc.limit`, and the other MediaWiki options Triplespace honours | as in MediaWiki | [mediawiki-compat.md](../api/mediawiki-compat.md) |

**The notification matrix moves here** (amends [0021](0021-notifications.md) §3): `private.notification_pref` is retired, and the addressing projection reads `notifications.*` keys instead. The four auto-watch flags of [0020](0020-change-feeds.md) §3 likewise; `private.watch` is unchanged.

Preferences belong to primary accounts. A subsidiary ([0024](0024-subsidiary-accounts.md)) has none of its own; where a preference would matter for it, such as the interface language of a notification about it, the operator's applies. Temporary accounts have preferences for the life of the account, as MediaWiki's do.

### 2. What private state is, and which of it is portable

*Changed by A3.*

Everything in `private` ([0013](0013-postgres-storage.md) §4, §5.6) belongs to one account, and this ADR classifies it by whether it may leave the instance with that account:

| Class | State | Why |
|---|---|---|
| **Portable** | Preferences (§1); the watch set with `seen` and expiries ([0020](0020-change-feeds.md) §3); contact details: email address and fediverse handle, with their verification state ([0021](0021-notifications.md) §5, §8); subsidiary **metadata**: names, operator, key labels, grants, IP ranges and expiries, never secrets ([0024](0024-subsidiary-accounts.md) §4); the inbox within its retention, with read state ([0021](0021-notifications.md) §3) | Chosen by the person, about the person, useful on any instance |
| **Re-established, not carried** | Bindings ([0007](0007-actor-identity.md) §3): the person logs in again and the identity provider proves who they are; API-key secrets and OAuth tokens ([0024](0024-subsidiary-accounts.md), [0025](0025-oauth-server.md)): reissued; the Atom watch token ([0020](0020-change-feeds.md) §4): reissued; sessions | Secrets are minted by an instance for itself, and a proof of identity is not transferable |
| **Never leaves** | IP addresses held for abuse handling and IP blocks ([0007](0007-actor-identity.md) §3, [0016](0016-permissions-and-access-control.md) §3); the notifier's private key ([0021](0021-notifications.md) §5) | Not the person's, or bound to one instance |

The classification is data: each `private` table is tagged in `triplespace-db`'s schema definition with its class, as a table comment `portability: {class}` that the crate checks against the live schema, and the export of §3 is generated from the tags, so a table added later cannot be forgotten. A table whose columns fall in two classes, as `api_key` does (portable metadata, a re-established secret), takes the stricter class, and the export reads only its portable columns. The privacy test of [0012](0012-api-requirements.md) §8 checks that nothing in the second or third class appears in a bundle.

### 3. The user data bundle

**Any account holder may download all of their portable state** as one document, and import one. The bundle is JSON with the payload type `scatter:v0/user-export`, versioned like every payload type ([0006](0006-log-integrity-and-erasure.md), Consequences), and documented in `docs/api/user-export.md`. It holds:

- the account's public identity: name, actor key, the instance's base URI and tenant slug, and the time of export;
- `preferences`: every set key;
- `watchlist`: target kind, target ID (with the entity's current canonical ID and, for pages and threads, the current title, so that a human can read it), `seen`, expiry, `notify`;
- `contacts`: email address and fediverse handle, each with `verified`;
- `subsidiaries`: for each, its name and actor key, and for each key its label, grants, IP ranges and expiry;
- `inbox`, optionally: the rows within retention, each as the activity row it points at plus reason and read state;
- a signature by the instance key ([0006](0006-log-integrity-and-erasure.md) §6) over the bundle, so a receiving instance can tell an export from a forgery, and the holder can prove where it came from.

**Export** is `GET /account/export` and **Download my data** on `Special:Account` (§7), rate-limited in the `account` class ([0024](0024-subsidiary-accounts.md) §5). A bundle that would be large because of the inbox is produced by a job in `ops` and fetched when ready. **Import** is `POST /account/import`, holder only: preferences overwrite, the watch set is a union (an entity ID that does not resolve here is kept and reported), contacts are stored **unverified** and a verification message is sent ([0021](0021-notifications.md) §4–5), subsidiaries are recreated as the importing account's with no keys, and inbox rows are stored read-only as history. An import is idempotent: importing the same bundle twice changes nothing.

**MediaWiki's forms stay.** `list=watchlistraw` and `action=watch` ([0020](0020-change-feeds.md) §5) and `action=options` (§6) keep working; the bundle is the form that covers everything at once.

### 4. The private extract in a tenant move (extends 0018 §10; extends 0007 §4)

A **cooperative** move ([0018](0018-tenants.md) §10) now carries three things beside the bundle: the retention extract, the binding digest, and a **private extract**: for every account of the tenant, its user data bundle of §3 (inbox included), encrypted so that only the receiving instance can read it. The receiving instance supplies, with the public key it already supplies for the key-chain rotation, an **encryption key** for the move; the extract is sealed to it; the old instance keeps no copy once the move completes. The extract travels operator to operator, as the binding digest does, never in the public bundle.

**Applied on reclaim.** A bundle is applied to an account when, and only when, that account is reclaimed by one of the three routes 0018 §10 gives, so nobody's settings appear under an account its holder has not yet proved is theirs. Contacts arrive **verified**, since the old instance had verified them and the move is cooperative; the fediverse handle's *follow* of the notifier has to be renewed, because the notifier is a new actor ([0021](0021-notifications.md) §5), and the account page says so. Keys are not in the extract; the operator reissues them.

**Unreclaimed accounts.** Bundles for accounts never reclaimed are deleted when the reclaim grace period ends, and the receiving instance logs the deletion count. An account can still be reclaimed afterwards; it simply arrives with defaults, and the person may import a bundle they downloaded themselves.

**Non-cooperative moves** have no private extract: `private` is not in backups the new operator can be handed responsibly, and 0018 §10 already limits that case to the public bundle. Self-service export and import (§3) are what a person has then, which is why **Download my data** exists at all and why the account page suggests it before a vanish.

**Rehosting an instance whole** carries `private` with it ([0018](0018-tenants.md) §10) and needs none of this.

### 5. Shape pins (settles 0003 Q2)

[0003](0003-statement-ui.md) §3 lets an editor pin a shape for a property "for the whole instance or for one entity" and left open where the pin lives. It lives in two places by scope:

- **A viewer's own pin** is the preference `shapes.pins` (§1): a map from property ID, or `{entity}:{property}`, to a shape. It affects only that viewer.
- **A tenant-wide pin** is a `config` record of kind `view-pin`, keyed `view-pin:{property}` or `view-pin:{entity}:{property}` ([0015](0015-record-format-and-partition-registry.md) §3), written with `ts-config`. It is logged, ordered and rebuildable like every registry entry, and the UI's view switch shows it as the default with the viewer's pin, if any, on top.

Neither is a statement, which is what 0003 required.

### 6. API (extends 0012 §4 and §5)

**Action API**: `action=options` with `change`, `optionname`, `optionvalue` and `reset`, over the registered keys, refusing unknown ones; `meta=userinfo&uiprop=options` returns the effective values with defaults filled in, as MediaWiki does. `list=watchlistraw` and `action=watch` are unchanged.

**REST**, under `rest.php/triplespace/v0`, holder only:

| Route | Meaning |
|---|---|
| `GET /account/preferences`, `PATCH /account/preferences` | Read and change preferences; `DELETE /account/preferences/{key}` resets one |
| `GET /account/export` | The bundle of §3, or `202` with a job to poll when it is being produced |
| `POST /account/import` | Import a bundle; the response lists what was applied, what needs verification, and what did not resolve |
| `GET /preferences/schema` | The registered keys, types and defaults (public) |

Permissions are [0021](0021-notifications.md) §9's: `editmyoptions` to change, `viewmyprivateinfo` to read contacts and to export; both default to `user`.

### 7. UI (extends 0010 §11)

The **Preferences** section of `Special:Account` ([0010](0010-site-ui.md) §11) becomes the home of every key in §1, grouped as MediaWiki groups them, and the **Notifications** section ([0021](0021-notifications.md) §6) reads and writes the `notifications.*` keys in place. A new **Your data** section, labelled Private, has **Download my data**, **Import data…**, and a plain account of what is and is not in a bundle (§2). The **Leave** section ([0010](0010-site-ui.md) §11) points at it before **Vanish**.

### 8. Storage (extends 0013 §4 and §5.6)

`private.preference (actor_key, key, value jsonb, updated, PRIMARY KEY (actor_key, key))`. `private.notification_pref` is dropped ([0021](0021-notifications.md) §3). `ops` gains the export-job rows for large bundles. Every `private` table carries its portability class (§2) in the schema definition. The private extract is not a table; it is produced at move time and destroyed on completion.

### 9. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed; the bundle format is a Triplespace surface, not a shared seam, so nothing enters a `scatter-*` crate. The table this section first gave is in A1.

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
