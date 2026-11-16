# 0111. Creating a tenant by request

- **Status:** Proposed
- **Date:** 2026-11-16
- **Updated:** 2026-11-16 (A1)
- **Author:** James Hare / Claude Opus
- **Uses:** [0015](0015-record-format-and-partition-registry.md), [0079](0079-derived-issuer-codes.md), [0089](0089-the-auth-host-and-instance-logins.md)

## Context

A tenant after the first is created by `triplespace tenant create`, or `tenant setup`, at a
terminal on the server. The command does everything in one step, as the database owner and
with the instance key: it creates the tenant's partitions, each with a record table of its own;
it writes the tenant's founding record, signed by the instance key under the founding
attestation; it derives the issuer code; it appends the `tenant:` record to the instance
`config`, attested by the operator who acted, an `owner` of the primary tenant, as the authority
record of the creation ([0040](0040-instance-prerogatives.md) §4;
[0046](0046-primary-tenant.md) §4); and it writes the provisions.

James asked for a way to create tenants from the site: a special page on the primary tenant,
usable only by highly trusted users, that does what the setup wizard does, the domain being
configured beforehand. Two facts stood in the way of the page doing the work itself. The
server's database login, `ts_server`, may insert rows but not create tables, and a partition's
record table is created with the partition (03 §1.2). And only the CLI signs with the instance
key: the server holds the key only to derive session secrets. Giving the public server either
power would widen what a stolen session, or a compromised server process, could do.

[0040](0040-instance-prerogatives.md) §4 already separates deciding from doing for every other
written instance act: an operator attests an authority record that says what the instance
decided, and the instance carries out acts that cite it. Tenant deletion, reclamation and the
propagation of an expunge already work that way, through job records in the instance `log`.
Tenant creation was the exception, its authority record written partway through the work.

A tenant's owner account needs a way to sign in. The CLI asks for a password, but a form that
takes a password for someone else is poor practice. Logins are instance-wide
([0089](0089-the-auth-host-and-instance-logins.md) §2), and one login may be bound to accounts on
several tenants ([0089](0089-the-auth-host-and-instance-logins.md) §4), so a new owner account can
be bound to a login that already exists.

### Direction

James's direction, from the design discussion of 2026-11-16:

- **"What if we uncoupled the intent of creation from the actual creation? The owner (or
  adequately entitled user in the future) would create a record declaring the tenant created,
  which would be carried out by an on-server process once it verifies the record is
  legitimate?"**
- On the process: a separate long-running process with the owner database login and the key,
  `triplespace provision`, woken by `LISTEN`/`NOTIFY` as the mail delivery worker is: "Agreed".
- On approval: "carry out immediately".
- On the owner: naming an existing login and binding the new owner account to it, which avoids
  passwords in forms: "Ideal if we could do that".
- On whether the CLI goes through the same path: "Yes".
- On the draft's open questions: a delay or a second approval "can be offered later but is not a priority for now"; an invitation link for an owner with no login, "Yes"; a narrower database role for the provisioner, "Yes"; filing and carrying out freezing, deletion, moves and the acceptance of the primary role the same way, "Yes".

## Decision

### 1. A later tenant is created by request (amends 0018 §1)

*Current text: [08](../architecture/08-tenants-and-instances.md) §1.1, §1.4.*

### 2. The tenant request

*Current text: [08](../architecture/08-tenants-and-instances.md) §1.4.*

### 3. The provisioner (extends 0013 §4)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §1.2; [08](../architecture/08-tenants-and-instances.md) §1.4.*

### 4. Carrying out a request (amends 0040 §4; amends 0046 §4)

*Current text: [08](../architecture/08-tenants-and-instances.md) §1.4, §7.4, §8.4.*

### 5. The owner's login (uses 0089 §4)

*Current text: [08](../architecture/08-tenants-and-instances.md) §1.4.*

### 6. The CLI files requests too (amends 0018 §1)

*Current text: [08](../architecture/08-tenants-and-instances.md) §1.1.*

### 7. API and UI (extends 0018 §11; extends 0046 §9)

*Current text: [18](../architecture/18-api.md) §3.2; [19](../architecture/19-site-ui.md) §6.11.*

### 8. Permissions (extends 0046 §8)

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2.*

### 9. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 10. Other tenant acts are filed the same way (amends 0018 §10; amends 0028 §5; amends 0046 §5)

*Current text: [08](../architecture/08-tenants-and-instances.md) §1.4, §6.3, §6.6, §7.5.*

## Alternatives considered

- **The server creates the tenant itself.** A `SECURITY DEFINER` function owned by the database
  owner would let `ts_server` create a partition's record table, and the server would sign the
  founding record with the key it already holds. It gives the one-step page James first asked
  for, but makes the public server a holder of both powers, so that a stolen `owner` session
  creates tenants at once, with nothing between the intent and the act.
- **A queued request applied by hand.** The page files a request and an operator applies it
  later with the CLI. It keeps both powers off the server, but is no longer the wizard: someone
  has to be at a terminal for every tenant.

## Consequences

- **The public server needs no new powers.** Filing a request is an ordinary write. Only the
  provisioner holds the instance key for this purpose, and its database role, `ts_provision`, can
  create a partition's record table only through one function, not as the database owner.
- **A stolen `owner` session can file a request, but not hide one.** The request is in the
  instance `log`, attested by the actor whose session filed it, and the tenant it creates cites
  it. Because requests are carried out at once, it cannot be withdrawn in time (Q1).
- **No password for someone else crosses a form.** The owner of a new tenant is bound to a login
  named in the request, or invited to sign in with one.
- **Tenant creation from the site needs the provisioner running.** Without it, a request stays
  pending, and `Special:Tenants` shows it so.
- **Every later tenant has the same history.** Whether it came from the CLI, the site or the API,
  its `tenant:` record cites a `tenant/request` attested by the operator who asked.
- **Deployment gains a process.** The guide to hosting several wikis will need to describe it
  once it is built.

## Open questions

- **Q1. A delay or a second approval.** Requests are carried out at once. Whether an instance may
  choose instead to wait out a delay, during which a request can be withdrawn, or to require a
  second `owner`'s approval, as [0040](0040-instance-prerogatives.md) Q1 asks of expunge. It may be
  offered later; it is not a priority (James, 2026-11-16).

## References

- [PostgreSQL: `LISTEN`](https://www.postgresql.org/docs/current/sql-listen.html) and
  [`NOTIFY`](https://www.postgresql.org/docs/current/sql-notify.html)

## Amendment log

### A1. Calls made building the provisioner

- **Date:** 2026-11-16
- **Source:** Direct: Claude, while building the tenant request and the provisioner (SCA-423), 2026-11-16; unconfirmed
- **Change:** amends §2, §3, §4; extends §8
- **Summary:** The calls made building §2–§6:
  - **The request** is a `scatter:v0/logevent` of type `tenant`, action `request`, keyed `tenant:{slug}`. Its `params`, which the activity projection copies into the instance `log`'s public events, hold only the slug and the base; the wiki's name, the owner's name, the owner login source, and for the CLI's `--adopt` the source wiki and the owner's ID, are under `request`, which no projection reads, so the owner login source is never served.
  - **A refusal** is a `tenant/refuse` log event under the same key, its target the request (`{kind: record, partition, offset}`), its `params.reason` one of `ts-not-entitled`, `ts-tenant-invalid` (a slug, base or name failing the checks `tenant create` made, the comment saying which), `ts-auth-base-required` and `ts-owner-no-login`. `tenant/request` and `tenant/refuse` are registered in `log-events.toml`.
  - **The carried-out mark** is the `tenant:` record's `request`, the request's offset in the instance `log`.
  - **The operator's right is checked when the request is carried out**, against the rights held then, not reconstructed at the request's offset as §4 had it: a request is carried out as soon as it is seen, so the two agree, and where a provisioner was down they differ only in refusing a request whose filer has since lost the right.
  - **A second function.** A tenant also has sequences of its own in `log` (its revision, log, page, user and entity IDs), and creating one needs `CREATE` on `log`, which `ts_server` lacks; §3 named one function. `log.create_sequence(tenant, kind)`, `SECURITY DEFINER` and owned by the database owner like `log.create_record_table`, creates one, for a tenant that already has partitions and under the grammar `sequences.rs` checks.
  - **`ts_provision` is a member of `ts_server`,** so that `ts_server`'s grants and its row-level policy on `log.record` apply to it unchanged.
  - **The provisioner projects the new tenant on the accounts service's login,** not `ts_provision`'s: the projection pipeline ends in addressing, which writes `private.inbox`.
  - **`ts-createtenant` is also in `groups.toml`'s list of Triplespace permissions,** which 0111 §8 omitted: an `owner` holds `*`, and `*` is that list.
  - **The CLI** names the owner login source with `tenant create --owner-login-of {actor key}`; `triplespace provision` takes `--database`, `--accounts-database`, `--key-file`, `--auth-base` and `--once`, from the same environment variables as the server.

  Chapter text: 08 §1.4; 03 §1.2.

Replaced text: 08 §1.4, 03 §1.2.
