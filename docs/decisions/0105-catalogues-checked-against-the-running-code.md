# 0105. Catalogues checked against the running code

- **Status:** Proposed
- **Date:** 2026-11-09
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

*Current text: [24](../architecture/24-documentation.md) §11.*

### 2. The server reports the permissions it checks (extends 0016 §2)

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2.*

### 3. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **Scanning the source for permission names.** About 150 checks pass a literal name and would be found; the rest pass a name computed at run time (`required_right`, a protection's right, a scoped check's permission), which a scan cannot follow. James chose the running code.
- **A typed permission list in the code** (an enum or constants the checks must use). It would make an unlisted name a build error, but policy is data in this codebase (22 §1.2 rule 3): permissions are registry values, and a tenant can define its own groups from them.

## Consequences

- **An undocumented or misspelt permission check shows on every CI run** of the client checks and the attack battery, on the job's summary page, and fails the job once the report is made strict.
- **"Not observed" is not "not built".** A run sees only the paths its tests take, so the report's second list mixes permissions that are not built with permissions no test exercises; it is a prompt for tests as much as for code.
- **Checks that read a group's permission list are not observed.** Delegation (`userrights`, `ts-config` on a group) and the instance-key rule (`ts-keys`) look at a group's own permissions, not an actor's effective ones, and do not pass through `Effective`.
- **The site's checks of `meta=userinfo` rights are not observed**: `triplespace-ui` reads the names the API returns, and the server checked those names when it answered.

## Open questions

- **Q1. When the report becomes a failure.** Proposed: once a CI run on `main` reports no unlisted permission, the report step gains `--strict`, by a change James approves. Whether the first strict runs should allow a short list of known exceptions instead.
- **Q2. Settings and log events.** The same mechanism fits both: the server reports each setting it reads and each log event it emits, compared with `settings.toml` and `log-events.toml`. Whether they come in one ADR or one each.
- **Q3. The end-to-end tests.** The Playwright suite (`ui/e2e`) starts a server too and exercises the site's pages, which the client checks and the battery do not; probing it as well would observe more permissions.

## References

- [0099](0099-status-from-the-code.md) §2; [0056](0056-security-model.md) §16; [0098](0098-pointers-from-birth.md) §5
