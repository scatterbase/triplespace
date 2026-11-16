# 0105. Catalogues checked against the running code

- **Status:** Proposed
- **Date:** 2026-11-09
- **Updated:** 2026-11-09 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0016](0016-permissions-and-access-control.md), [0098](0098-pointers-from-birth.md)
- **Uses:** [0056](0056-security-model.md), [0099](0099-status-from-the-code.md)

## Context

By 2026-11-09 the registry catalogues ([0098](0098-pointers-from-birth.md) §5) were checked one way. The generators kept each catalogue consistent with the other registry files (every permission a group grants is catalogued) and derived a *Built* column where a contract or a constant said what was served (the routes from the OpenAPI file, the Action API modules from the paraminfo lists). Nothing compared a catalogue with what the code actually did. A permission the code asked about but no file listed went unnoticed: `Effective::holds` answers false for a name nobody holds, so a misspelt check denies everyone, `owner` included, and an undocumented one grants what the catalogue does not describe. The permission model's attack battery ([0056](0056-security-model.md) §16; `tools/acl_check.sh`) tests outcomes, not names.

[0099](0099-status-from-the-code.md) had made the code the record of what is built. This ADR applies the same idea to the catalogues: the code says what it uses, and the catalogue is checked against it.

### Direction

James's direction, from the design discussion of 2026-11-09:

- Checks in both directions between a catalogue and the code, **starting with permissions**, then settings and log events.
- **"Running code"**: the names come from the code as it runs, not from scanning its source.
- **"Report first, then fail"**: the check starts as a report, and becomes a CI failure later.

## Decision

### 1. A catalogue the code acts on is checked against the running code (extends 0098 §5)

*Changed by A2.*

*Current text: [24](../architecture/24-documentation.md) §11.*

### 2. The server reports the permissions it checks (extends 0016 §2)

*Changed by A1, A2.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2.*

### 3. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **Scanning the source for permission names.** About 150 checks pass a literal name and would be found; the rest pass a name computed at run time (`required_right`, a protection's right, a scoped check's permission), which a scan cannot follow. James chose the running code.
- **A typed permission list in the code** (an enum or constants the checks must use). It would make an unlisted name a build error, but policy is data in this codebase (22 §1.2 rule 3): permissions are registry values, and a tenant can define its own groups from them.

## Consequences

- ~~**An undocumented or misspelt permission check shows on every CI run** of the client checks and the attack battery, on the job's summary page, and fails the job once the report is made strict.~~ *Strict from A1, and the end-to-end suite probes too.*
- **An undocumented or misspelt permission check fails CI** in the client-compatibility and end-to-end jobs, and the report on each job's summary page names it (A1).
- **"Not observed" is not "not built".** A run sees only the paths its tests take, so the report's second list mixes permissions that are not built with permissions no test exercises; it is a prompt for tests as much as for code.
- **Checks that read a group's permission list are not observed.** Delegation (`userrights`, `ts-config` on a group) and the instance-key rule (`ts-keys`) look at a group's own permissions, not an actor's effective ones, and do not pass through `Effective`.
- **The site's checks of `meta=userinfo` rights are not observed**: `triplespace-ui` reads the names the API returns, and the server checked those names when it answered.

## Open questions

- **Q1.** ~~**When the report becomes a failure.** Proposed: once a CI run on `main` reports no unlisted permission, the report step gains `--strict`, by a change James approves. Whether the first strict runs should allow a short list of known exceptions instead.~~ *Settled by A1: strict now, with no list of exceptions.*
- **Q2.** ~~**Settings and log events.** The same mechanism fits both: the server reports each setting it reads and each log event it emits, compared with `settings.toml` and `log-events.toml`. Whether they come in one ADR or one each.~~ *Settled by [0107](0107-settings-and-log-events-checked-against-the-running-code.md) §2, §3: one ADR for both, with one probe for every catalogue.*
- **Q3.** ~~**The end-to-end tests.** The Playwright suite (`ui/e2e`) starts a server too and exercises the site's pages, which the client checks and the battery do not; probing it as well would observe more permissions.~~ *Settled by A1: the end-to-end job probes its API server and runs the strict report too.*

## References

- [0099](0099-status-from-the-code.md) §2; [0056](0056-security-model.md) §16; [0098](0098-pointers-from-birth.md) §5

## Amendment log

### A1. Strict, and the end-to-end suite probes too

- **Date:** 2026-11-09
- **Source:** Direct: James, design discussion of 2026-11-09
- **Change:** amends §2
- **Summary:** The permission report is strict from now: an unlisted permission, or a run that observed no check, fails the job, with no list of exceptions (settles Q1). The end-to-end job runs its API server with the probe on and the strict report after the Playwright suite (settles Q3), so the site's pages add to what is observed.

Replaced text ([09](../architecture/09-security-and-moderation.md) §2.2, as it stood):

> CI runs the client checks and the attack battery with it on, and `tools/permission_report.py` compares the names with this catalogue and with the `[permissions]` lists of `groups.toml`, on the job's summary page: a name in neither is unlisted, and fails nothing yet.

### A2. One probe for every catalogue

- **Date:** 2026-11-09
- **Source:** [0107](0107-settings-and-log-events-checked-against-the-running-code.md) §1
- **Change:** amends §1, §2
- **Summary:** The probe is one for every catalogue the code acts on: `--probe FILE` (`TRIPLESPACE_PROBE`) in both binaries replaces `triplespace-server --probe-permissions`, each line is `{kind} {name}` (`permission`, `setting`, `log-event`), and `tools/probe_report.py` replaces `tools/permission_report.py`, checking permissions, settings and log events strictly. Settles Q2: settings and log events in one ADR, 0107. Chapter text: [24](../architecture/24-documentation.md) §11; [09](../architecture/09-security-and-moderation.md) §2.2; [22](../architecture/22-crates-and-stack.md) §4.9.

Replaced text ([24](../architecture/24-documentation.md) §11, as it stood):

> The code, asked to, reports each name it uses as it runs (a permission it checks; later a setting it reads, a log event it emits), and a script compares that set with the registry file. … The permissions are the first ([09](../architecture/09-security-and-moderation.md) §2.2).

Replaced text ([09](../architecture/09-security-and-moderation.md) §2.2, as it stood):

> `triplespace-server --probe-permissions FILE` (`TRIPLESPACE_PROBE_PERMISSIONS`) installs one that appends each permission name to the file the first time it is checked; without it nothing is recorded. CI runs the client checks, the attack battery and the end-to-end suite with it on, and `tools/permission_report.py --strict` compares the names with this catalogue and with the `[permissions]` lists of `groups.toml`, on each job's summary page: a name missing from either is unlisted and fails the job, as does a run that observed no check at all ([0105](0105-catalogues-checked-against-the-running-code.md) A1).
