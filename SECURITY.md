# Security policy

Triplespace is pre-release software and is not yet suitable for production use (see the
[README](README.md)). Security reports are still very welcome, and
the earlier they come, the better.

## Reporting a vulnerability

**Report privately through GitHub:** open the repository's
[Security tab](https://github.com/scatterbase/triplespace/security) and choose **Report a
vulnerability**, or go straight to the
[new advisory form](https://github.com/scatterbase/triplespace/security/advisories/new).
Only you and the maintainers can see the report.

**Do not** report a vulnerability in a public issue, pull request, discussion or commit message.

This is currently the only reporting channel. If you cannot use GitHub, open a public issue that
says only that you have a security report and need another way to send it, with no details, and
we will arrange one.

Please include, as far as you can:

- what the problem is and what an attacker could do with it;
- the commit or version you tested;
- steps to reproduce, ideally against a fresh local instance (the README's Quickstart);
- any proof-of-concept requests, scripts or logs;
- how you would like to be credited, or whether you would rather not be named.

## What to expect

Triplespace is maintained by a small team, so these are targets rather than guarantees:

- **Acknowledgement** within 7 days.
- **An initial assessment** (whether we can reproduce it and how severe we think it is) within 14
  days.
- **A fix or mitigation**, coordinated with you through the advisory, with progress updates
  along the way. We aim to disclose within **90 days** of the report, sooner when a fix is ready,
  and will agree any extension with you.
- **Disclosure** as a published GitHub security advisory, with a CVE where one is warranted,
  crediting you unless you ask us not to.

Please keep the details private until the advisory is published.

## Scope

Only the `main` branch is supported; there are no released versions yet.

We are most interested in:

- **The permission model**: ACLs on pages, namespaces, sets and tenants; groups and grants;
  anything that lets an account read or change what it should not. This has had limited testing
  (one automated attack battery, `tools/acl_check.sh`) and no independent audit.
- **Tenant isolation**: anything that lets one wiki on an instance read, change or affect another's
  data, accounts or configuration.
- **Authentication and credentials**: passwords, sessions, bot passwords and subsidiary API keys,
  OAuth clients, registration and login challenges, forwarder keys and trusted-proxy handling.
- **Log integrity**: forging, altering or hiding records, or producing a history whose Merkle
  proofs or signed checkpoints verify when they should not.
- **Erasure**: data that remains readable after it has been erased.
- **Injection and the site**: SQL injection, cross-site scripting, request forgery, and
  server-side request forgery through the dump readers, the follower or entity sources.
- **Denial of service** from a single cheap request, such as a query or dump that consumes
  unbounded memory or CPU.

Out of scope:

- Problems that require the instance signing key (`triplespace.key`), database credentials or
  shell access on the server.
- Volumetric denial of service, and findings from automated scanners without a demonstrated
  impact.
- Vulnerabilities in dependencies that Triplespace does not expose; please report those
  upstream (we still welcome a note if Triplespace is affected).
- The sample data under `docs/samples/`.

## Testing safely

Test against an instance you run yourself; the Quickstart sets one up in a few minutes. Do not
test against instances run by other people without their permission, do not access or change
data that is not yours, and stop and report as soon as you have shown the problem. We will not
pursue anyone who reports in good faith and follows this policy.
