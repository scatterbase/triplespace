# Triplespace

Wikibase reimagined as an append-only log, in Rust. Bulk ingest is a first-class path, and
foreign entities (Wikidata's `WDQ42`, OpenAlex's `OAW123`, a domain name as `domain:example.org`)
sit alongside local ones without being reified.

The design lives in [`docs/decisions/`](docs/decisions/) as architecture decision records; the
names and codes the code embeds live in [`docs/registry/`](docs/registry/); the compatibility
contracts with Wikibase and MediaWiki are in [`docs/api/`](docs/api/). Start with
[0000](docs/decisions/0000-init.md), then [0005](docs/decisions/0005-crate-organization.md) for
the crate map and build order.

## Layout

| Path | Holds |
|---|---|
| `crates/` | One Cargo workspace. `scatter-*` crates are shared with Scatterbase and know nothing about Triplespace; `triplespace-*` crates are the product (0005 §1). |
| `xtask/` | Repository tasks: `cargo xtask deps` checks the workspace dependency graph against the table in 0005 §2; `cargo xtask wasm` builds the crates 0005 rule 7 requires to build for `wasm32-unknown-unknown`. |
| `docs/` | ADRs, registry, API contracts and test vectors. CC0-1.0 (`docs/LICENSE`), so other implementations can embed them. |
| `ui/` | The frontend (0034). Not yet started. |

## Building

Stable Rust as pinned in `rust-toolchain.toml`, edition 2024. A small instance needs Postgres and
nothing else (0033 §1); Valkey, OpenSearch and QLever are optional services a larger one adds.

```
cargo build --workspace
cargo nextest run --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
cargo xtask deps
```

The tests that need a database read `TRIPLESPACE_TEST_DATABASE_URL` (a Postgres URL whose role
may create databases) and are vacuous without it. `python3 docs/decisions/check_adrs.py --index
docs` checks the ADRs and regenerates their index.

## Setting up an instance

Two binaries come out of the workspace: `triplespace`, the command line that creates and loads
an instance, and `triplespace-server`, which serves the Action API. Both take the database as
`--database` or `TRIPLESPACE_DATABASE_URL`.

```sh
createdb triplespace
export TRIPLESPACE_DATABASE_URL=postgres://triplespace@localhost/triplespace

# The instance and its first tenant: the schema, the instance signing key (written to
# triplespace.key, mode 0600), the instance and tenant partitions, the owner account.
echo 'a long passphrase' > owner.pw
triplespace instance create \
    --tenant librarybase --base https://librarybase.org \
    --provider internetdomains \
    --owner 1 --owner-name Alice --owner-password-file owner.pw
```

`instance create` is where the shape of the instance is fixed (0015 §3, 0018): the tenant slug
and base URL, the providers that get mirror partitions, and the owner, who holds every
permission (0016 §3). It refuses to run against a database that already holds an instance.
Everything it writes is a log record, so `triplespace status` shows the partitions it made and
`triplespace rebuild` reproduces the serving tables from them at any time.

Bots and tools log in as **subsidiary accounts** of a person (0024), never as the person:

```sh
triplespace subsidiary create --tenant librarybase --name "Alice bot" --operator Alice --group bot
triplespace subsidiary key --tenant librarybase --name "Alice bot" --label laptop \
    --grant editentity --grant highvolume
```

The second command prints the key once: `lgname=Alice bot@laptop` with the secret is a
MediaWiki bot password, and `Authorization: Bearer {key ID}.{secret}` is the same credential
without a session. WikibaseIntegrator logs in, reads and edits with it unchanged (the acceptance
test in `crates/triplespace-server/tests/` is that flow); Pywikibot speaks the same forms but has
not been run against it yet. `subsidiary
keys` lists a subsidiary's keys and `subsidiary revoke --key-id` ends one together with every
session it opened. `triplespace password set` sets or replaces a person's password.

Then serve it:

```sh
triplespace-server --listen 0.0.0.0:8080 --key-file triplespace.key
```

The server answers for the hosts of its registered tenant bases and the farm base (0056 §10);
anything else is 421, so it expects to sit behind a proxy that terminates TLS for those hosts
and forwards `Host` (`--trusted-proxy` names the proxies whose `X-Forwarded-*` headers are
believed). For a laptop, `--mode development --dev-tenant librarybase` serves any host as that
tenant over plain HTTP and marks the API as insecure in `meta=siteinfo&siprop=triplespace`.
`/healthz` answers on the public listener and on `--admin-listen` if given.

A client then uses the usual Wikibase surface at `/w/api.php` (or `/api.php`): `meta=siteinfo`,
`meta=tokens`, `action=login` with the bot password, `wbgetentities`, `wbsearchentities`, and the
edit modules from `wbeditentity` to `wbremovereferences`; `/wiki/Special:EntityData/Q6.json` and
`/entity/Q6` serve the entity documents. The owner logs in with `action=clientlogin`. Everything
not yet served answers with a MediaWiki-shaped error rather than silence, so a client's failure
names the gap.

## Loading a Wikibase

There are two ways a Wikibase's entities enter an instance, and they are not interchangeable.

**Adopting** (0035) makes an existing wiki *become* a tenant: its items and properties keep
their `Q` and `P` numbers, page IDs and user IDs, its accounts are recreated under their own
numbers, and every ID counter is floored above what the wiki used, so nothing is ever reissued.
It is a one-way door: the source must be frozen first, because after adoption the tenant mints
the next IDs. The input is the wiki's **MediaWiki XML dump**, not its JSON dump, because only
the XML carries page IDs, revision IDs, timestamps and contributors together:

```sh
# On the frozen source wiki:
php maintenance/dumpBackup.php --current --include-files=no | gzip > librarybase-20261003.xml.gz
mysql -e 'SELECT * FROM wb_id_counters'          # the counters, not the highest IDs in the dump

# Here. --adopt tells the tenant where it came from; --owner is the owner's user ID *on the source*.
triplespace instance create --tenant librarybase --base https://librarybase.org \
    --provider internetdomains --adopt https://librarybase.org/ \
    --owner 7 --owner-name Alice --owner-password-file owner.pw

triplespace adopt --tenant librarybase --dump librarybase-20261003.xml.gz \
    --source https://librarybase.org/ --version librarybase-20261003 --frozen \
    --counters item=350000,property=1200 \
    --entity-source wikidata=WD           # only if the source used federated properties
```

`adopt` reads the dump twice: a survey for the floors and the accounts, then the job, which
writes each entity's current state as an `adopt` record with the source revision ID, timestamp
and page ID as content (0035 §3). Re-running it over the same dump is a no-op; a changed dump is
a reject per differing entity, never an overwrite. Pages that are not entities (the main page,
templates), redirects and anything the model cannot parse are skipped and counted; the job's
first hundred rejects are on its `job/finish` record and in `view.job_reject`. Pass
`--counters` from `wb_id_counters` whenever you can: without it the floors come from the highest
IDs in the dump, and a number the source allocated and deleted could be reused (0035 §4).

**Syncing** (0002 §8.4) mirrors another Wikibase as a *provider*: its entities appear under the
provider's prefix (`XDQ20`) or, for a key-mapped provider, under the key they identify
(`domain:wikipedia.org`, 0009 §9), beside the tenant's own entities, and local assertions about
them live in the tenant's graph. The input is the provider's entity JSON dump (`dumpJson.php`)
or its XML dump; the adapter decides what each entity becomes:

```sh
triplespace sync --provider internetdomains --dump internetdomains-20261003.json.gz --version 20261003
```

For internetdomains.wiki the adapter maps every item with exactly one valid `P1` (domain name)
onto `domain:{key}`, rewrites values that point at mapped items into domain values, drops the
identity property from the mapped state, and reports as rejects the items whose `P1` is not a
valid domain name or is shared by two items (those stay `XDQ` items, their `P1` kept as data).
`--snapshot` declares the dump complete, so entities it no longer carries are tombstoned
afterwards, up to `--threshold`. Re-running a sync writes only what changed upstream.

After either, `triplespace status` shows every partition's head and whether each projection is
current, and the serving tables are there to inspect: `view.entity`, `view.term`,
`view.identifier`, `view.actor`, `view.job`, `view.job_reject`, `view.keyed_map`.

## Licence

Triplespace is GPL-3.0-or-later (`LICENSE`). The shared `scatter-*` crates are also available
under a commercial licence from Scatter LLC. `docs/` is CC0-1.0. See 0005 §6 and 0033 §16.
