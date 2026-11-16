# 0107. Settings and log events checked against the running code

- **Status:** Accepted
- **Date:** 2026-11-09
- **Updated:** 2026-11-09 (A1)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0011](0011-logs.md), [0033](0033-backend-stack.md), [0085](0085-settings-and-preferences.md), [0105](0105-catalogues-checked-against-the-running-code.md)
- **Uses:** [0099](0099-status-from-the-code.md)

## Context

[0105](0105-catalogues-checked-against-the-running-code.md) made a catalogue the code acts on checkable against the running code, and applied it to permissions: the server, asked to, reported every permission it checked, and CI compared the names with `permissions.toml` and `groups.toml`, strictly from its A1. Its Q2 left settings and log events, which have the same failure: `Snapshot::resolve` answers `None` for a key `settings.toml` does not register, so a misspelt key silently falls back to whatever the caller does without a value, and a log event the activity projection writes under a type or action `log-events.toml` does not list is served by `list=logevents` and never documented.

Two things had changed shape by then. The CLI projects records as well as the server (`adopt`, `sync` and `instance create` write log events before any server starts in a test run), and a flag per catalogue would have multiplied flags, files and report scripts by three.

### Direction

James's direction, from the design discussion of 2026-11-09:

- Settings and log events **"the same way"** as permissions (0105 Q2), in one ADR.
- **One probe** for every catalogue: one flag and one file, replacing `--probe-permissions`.
- **The CLI honours the probe too.**
- **Strict at once if the local runs report nothing unlisted**, as permissions became in 0105 A1.

## Decision

### 1. One probe for the catalogues the code acts on (amends 0105 §1, §2; extends 0033 §15)

*Changed by A1.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2; [22](../architecture/22-crates-and-stack.md) §4.9; [24](../architecture/24-documentation.md) §11.*

### 2. Reads of a setting by key are observed (extends 0085 §1)

*Current text: [23](../architecture/23-configuration-and-registry.md) §3.1.*

### 3. The log events written are observed (extends 0011 §6.1)

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 4. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **A misspelt setting key or an undocumented log event fails CI**, in the client-compatibility and end-to-end jobs, as an unlisted permission already did.
- **A setting the settings pages list is not thereby observed.** `resolve_all` walks the registry rather than asking for keys, so only reads by key count, and "not observed" keeps its meaning.
- **Log events the CLI's projections write are observed** along with the server's, since both binaries take the probe.
- **Its first run found a bug.** The attack battery and the end-to-end suite both read `search.farm_wide` as a setting: Special:Version's feature list resolved every key of `version.toml` through the settings registry, but `search.farm_wide` is a tenancy switch ([0028](0028-tenancy-policy.md) §9), so it always read as its default. The version endpoint now reads a key the settings registry does not know from the instance's `tenancy:` record, and the run is clean.
- **The report is one script** for the three catalogues, and a fourth would be one more kind of line and one more file to compare with.

## Open questions

- **Q1.** ~~**Routes and modules.** The route and Action API catalogues already derive a *Built* column from the OpenAPI contract and the paraminfo lists. Whether the probe should also report the routes and modules a run served, which would make "built" mean "answered in a test run" rather than "declared".~~ *Settled by [0108](0108-routes-modules-and-tenancy-switches-checked-against-the-running-code.md) §1: yes; the server reports the routes and modules it serves, and the module catalogue lists every module served.*
- **Q2.** ~~**Tenancy switches.** The bug the first run found was a tenancy switch read as a setting. Whether the probe should also report the tenancy switches a run reads, compared with `tenancy.toml`.~~ *Settled by [0108](0108-routes-modules-and-tenancy-switches-checked-against-the-running-code.md) §2: yes; tenancy-switch reads are reported and checked against `tenancy.toml`.*

## References

- [0105](0105-catalogues-checked-against-the-running-code.md) §1–2, A1, Q2; [0099](0099-status-from-the-code.md) §2

## Amendment log

### A1. The probe reports tenancy switches, routes and modules too

- **Date:** 2026-11-09
- **Source:** [0108](0108-routes-modules-and-tenancy-switches-checked-against-the-running-code.md) §1, §2
- **Change:** amends §1
- **Summary:** The probe gains three kinds of line: `tenancy` for a tenancy switch read, and, from the server, `module` and `route` for an Action API module or a REST route served. Settles Q1 and Q2. Chapter text: [22](../architecture/22-crates-and-stack.md) §4.9; [24](../architecture/24-documentation.md) §11.

Replaced text ([22](../architecture/22-crates-and-stack.md) §4.9, as it stood):

> `log-event block/block` for a log event the activity projection wrote ([16](../architecture/16-logs-feeds-and-notifications.md) §2.3). Without it nothing is recorded.

Replaced text ([24](../architecture/24-documentation.md) §11, as it stood):

> (a permission it checks, a setting it reads, a log event it writes) … Permissions, settings and log events are checked so, strictly ([09](../architecture/09-security-and-moderation.md) §2.2; [23](../architecture/23-configuration-and-registry.md) §3.1; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3; [0107](0107-settings-and-log-events-checked-against-the-running-code.md) §1).
