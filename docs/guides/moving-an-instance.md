# Moving an instance to another installation

You run a Triplespace instance and want it somewhere else: a new server, a new Postgres, a new
data centre. This guide takes the whole instance across (every wiki on it, its full history,
accounts, settings, keys and queued work) so that the new installation carries on where the
old one stopped. Nothing is renumbered and nothing is re-signed: every entity, revision, page and
user ID stays the same, and the log still verifies against the instance's signatures.

The design is in [architecture 08](../architecture/08-tenants-and-instances.md) §6.7
(*Rehosting an instance*) and [01](../architecture/01-log-and-records.md) §8; the decisions behind
it are [0103](../decisions/0103-export-transfer-and-backup.md).

!!! note "Moving one wiki is a different job"
    This guide moves an **instance**, everything a `triplespace` database holds. Moving a single
    wiki (tenant) out of an instance into someone else's is a **transfer**
    ([08](../architecture/08-tenants-and-instances.md) §6.2–6.3), which is not built yet:
    `triplespace export --scope transfer` says so. On an instance with only one wiki, moving
    the instance moves the wiki.

## How it works

The old installation writes a **backup**: a directory, called a bundle, holding every partition
of the log verbatim with its checkpoints, the registry of partitions, the operational state that
no replay of the log could recover (queued jobs, inbox read marks, forwarder keys and the like),
and, encrypted, the `private` schema: passwords, sessions, preferences, email addresses. The new
installation **restores** it into an empty database: it loads the log, checks every checkpoint
against it, rebuilds every view from the log, and puts the operational and private state back.

Two things are not in the bundle, and you carry them yourself:

- **The instance signing key** (`triplespace.key`). The new installation signs new checkpoints
  with it, as the same instance; the log's key chain names it.
- **Your configuration**: the environment variables or `.env` file, the server's flags, the
  database logins of each part of the server, and the proxy in front.

The `private` part of the backup is sealed to a **backup key set** that you register in advance.
Its secret keys live off the instance, with the people who may restore it, so a stolen backup
does not give away anyone's password hashes or email addresses. You can make a backup without
it (`--without-private`), but then nobody can log in on the new installation until their
password is set again.

## Before you start

You need:

- **On the new installation**: the Triplespace binaries, built as in the
  [Quickstart](quickstart.md) (steps 1–3), from the **same release as the old installation or a
  newer one**. A restore migrates a backup from an older schema forward, but refuses one written
  by a newer binary than itself. PostgreSQL, and an **empty** database to restore into.
- **On the old installation**: shell access as the database owner, and the instance key file.
- **Disk** for the bundle: roughly the size of the log, plus the private and operational tables.
  For a large mirror this is a lot; put it somewhere both machines can reach, or plan to copy it.
- **Downtime**. The backup is one consistent snapshot, but anything written to the old
  installation after it starts is not in it. Stop writes for the duration (step 3).

The examples use these variables. `OLD` and `NEW` are the owner's database URLs on each side,
the ones in each installation's `.env` that `triplespace instance setup` and `migrate` use, not
the server's own logins. Every command below takes `--database` explicitly, so that nothing
reads the wrong side's `.env` by accident.

```sh
export OLD=postgres://triplespace@old-db/triplespace
export NEW=postgres://triplespace@new-db/triplespace
```

## 1. Register a backup key set (once)

Skip this if your instance already has one: `triplespace backup-key show --database "$OLD"`
prints the set in force, or says that none is registered.

Each person who may restore the instance makes a key **on their own machine**, not on the
server:

```sh
triplespace backup-key generate --out ~/triplespace-backup.key
```

It writes the secret key to the file, readable only by you, and prints the public half,
`backup-key:…`. Keep the secret offline; whoever holds it can open the private part of every
backup sealed to it.

Then register the public halves as the instance's set. **The instance's owner account does
this**, at the farm base (on a fresh instance from the Quickstart, the first wiki's base URL),
as `PUT /w/rest.php/triplespace/v0/backup-keys` while logged in:

```json
{"keys": ["backup-key:…", "backup-key:…"], "threshold": 1, "token": "<csrf token>"}
```

The token is the CSRF token of the owner's session (`action=query&meta=tokens`). `threshold` is
how many of the keys a restore needs: `1` lets any single holder restore; `2` of three keys
means no one person can. On an instance with no owner account, the operator registers the set
from the command line instead:

```sh
triplespace backup-key register --database "$OLD" \
    --key backup-key:… --key backup-key:… --threshold 1
```

A backup is sealed to the set in force when it is made. If you later replace the set, older
backups stay sealed to the old one; `triplespace backup-key reseal` reseals one to the new set,
given enough shares of the old.

## 2. Check that the instance is healthy

```sh
triplespace status --database "$OLD"
```

Every projection should be current, and no migration pending. A backup carries the log, not the
views, so a view that is behind is rebuilt on the other side anyway; but a pending migration or
a stuck job is better found now than mid-move.

## 3. Stop writes

Stop `triplespace-server` (and `triplespace-web`, if you run it), and any `triplespace follow`
or `sync` that is running. Anything written after the backup starts would be lost, so nothing
should be able to write. Readers can be served a maintenance page by your proxy meanwhile.

## 4. Sign the log's heads

```sh
triplespace checkpoint --database "$OLD" --key-file /path/to/triplespace.key
```

This signs the current head of every partition that has grown since its last checkpoint. The
restore checks each checkpoint against the records it loads, and the new installation will go on
from these signed heads.

## 5. Write the backup

```sh
triplespace export --database "$OLD" --scope backup --out /backups/instance-20261113
```

The directory must not exist or must be empty. Without a registered key set this refuses (step 1);
`--without-private` writes a backup without its sealed part instead, and the bundle records that
it is one.

Then check the bundle on its own, without a database:

```sh
triplespace verify /backups/instance-20261113
```

It checks every partition's tree, checkpoints, key chain and record bodies, and exits non-zero on
any problem.

## 6. Copy what the bundle does not carry

To the new installation, by a channel you trust:

- the **bundle** directory;
- the **instance key**, `triplespace.key`, mode `0600`. Never put it in the bundle's directory,
  nor anywhere the backup is stored: the bundle's sealing protects `private`, not the key;
- your **configuration**: the `.env` file, and `server.env` if the server has its own (see the
  [Quickstart](quickstart.md), step 6), or whatever environment and flags you start
  `triplespace-server` with, and your proxy's configuration. Change the database URLs in them
  to the new database's;
- the server's **`data/` directory**, if you want what is in it: the dumps `triplespace dump`
  wrote (`TRIPLESPACE_DUMPS_DIR`), queued exports' files (`TRIPLESPACE_BLOB_DIR`), and mail
  written by the `log` transport. The log does not depend on any of them;
- any **rejects files** you kept from `triplespace sync --rejects …`, if you want them;
- **uploaded file bytes**, if your instance has any: the export notes that it does not include
  them yet.

## 7. Unseal the shares

Each key holder needed for the restore (as many as the set's threshold) opens their share of the
backup **on their own machine**, with their secret key:

```sh
triplespace backup-key unseal \
    --sealed /backups/instance-20261113/sealed/key.cbor \
    --key ~/triplespace-backup.key \
    --out alice.share
```

`sealed/key.cbor` is a small file; the holder needs only it, not the whole bundle. They send the
resulting share file to whoever runs the restore. A share opens this one backup and no other,
but treat it as a secret until the restore is done, then delete it.

Skip this step for a backup written with `--without-private`.

## 8. Restore into the new database

Create an empty database, as its owner. **Empty** means never migrated: do not run
`triplespace migrate` or `instance create` on it first.

```sh
createdb -h new-db triplespace
triplespace restore --database "$NEW" --bundle /backups/instance-20261113 \
    --share alice.share
```

Pass `--share` once per share the set's threshold needs. The restore reads every file of the
bundle through before it touches the database, so a bundle cut short or shares that do not open
it fail at once. Then it migrates the database (making the schemas, roles and grants), loads
every partition and checks its checkpoints, rebuilds every projection from the log, and loads the
operational and private state. It ends by printing how many partitions, records and tables it
restored.

The restore runs as one command, but not as one transaction. If it stops part way, drop the
database, create it again, and run the restore once more: it refuses a database that is not
empty.

To restore a backup without its private part, or to restore a sealed one without opening it,
pass `--without-private` instead of `--share`. Accounts then cannot log in until their
passwords are set again (`triplespace password set`), and inboxes and preferences are empty.

## 9. Give the server its logins

The restore created the roles each part of the server runs as (`ts_server`, `ts_accounts`,
`ts_notify`) on the new Postgres, but not the **logins** you made for them, which belong to the
Postgres server, not the database. If you ran the server under its own logins on the old
installation (Quickstart step 6), make them again on the new one:

```sql
CREATE ROLE ts_server_login LOGIN PASSWORD '…' IN ROLE ts_server;
CREATE ROLE ts_accounts_login LOGIN PASSWORD '…' IN ROLE ts_accounts;
CREATE ROLE ts_notify_login LOGIN PASSWORD '…' IN ROLE ts_notify;
```

and point the server's `TRIPLESPACE_DATABASE_URL`, `TRIPLESPACE_ACCOUNTS_DATABASE_URL` and
`TRIPLESPACE_NOTIFY_DATABASE_URL` (in `server.env`) at them.

## 10. Check, then start

```sh
triplespace status --database "$NEW"
```

Every partition's head should match the old installation's, and every projection be current.
Then start the server as you did on the old installation, with the same configuration, and
with `TRIPLESPACE_KEY_FILE` naming the instance key you copied.

Log in as an ordinary account and as the owner, open a page and an entity, and make a test edit.
Restart any `triplespace follow` you ran.

## 11. Switch over

Point DNS (or your proxy) for every wiki's host, the farm base and the auth base at the new
installation. Keep the **same base URLs**: they are part of each wiki's identity, in every IRI it
has published. Changing one is an alias record ([08](../architecture/08-tenants-and-instances.md)
§6.1), which the command line does not write yet.

Keep the old database, stopped, until you are satisfied with the new one, then decommission it.
Keep the bundle as your most recent backup, or delete it with the shares.

## Another way: a Postgres dump

`pg_dump` and `pg_restore` also move an instance, without the backup key set: faster for a very
large instance, but not checked against the log's signatures on the way, and it carries
`private` unencrypted, so the dump must be guarded as the database is. Steps 2–4, 6 (without
the bundle), 9, 10 and 11 still apply; in place of 5, 7 and 8:

```sh
pg_dump -Fc -f instance.dump "$OLD"
pg_restore --no-owner --no-privileges -d "$NEW" instance.dump
triplespace migrate --database "$NEW"
```

`pg_restore` reports two errors on a server that lacks the roles (`log.record`'s two row-level
security policies name them); `triplespace migrate` then makes the roles and gives every grant
again ([08](../architecture/08-tenants-and-instances.md) §6.7).

## If something goes wrong

| The restore says | Meaning |
|---|---|
| the database already has the schema(s) … | It has been migrated or restored into, or an earlier restore stopped part way. Drop it and create it again. |
| the backup's `private` is sealed: give *n* share file(s) | Too few `--share` files for the set's threshold (step 7), or none. |
| the backup's sealed … does not open | The shares are of another key set or another backup. |
| … does not say whether it has its sealed sections | The export did not finish. Write the backup again. |
| … is not the file the backup wrote, or does not match the records | The bundle is damaged or was copied incompletely. Run `triplespace verify` on it, and copy it again. |
| a refusal naming a migration | The backup was written by a newer release than the restoring binary. Build that release, or a newer one, on the new installation. |
