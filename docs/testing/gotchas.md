# Gotchas: the sandbox, the test environment and the checkout

What is not derivable from the ADRs, the chapters or the code and bites anyway. Add what you learn;
strike what stops being true. Implementation status is not kept here (24 §15: `tools/coverage.py
--todo`), and neither are decisions (the ADRs' logs and open questions, listed in
`docs/decisions/INDEX.md`).

## Known test-environment failures

- `triplespace-db --test db` `the_public_role_cannot_read_private` fails in the cloud sandbox
  ("permission denied to set role ts_server") on the base commit too; environmental, not a
  regression. It fails the same way on a Debian 13 VM prepared by `tools/dev_env.sh`
  (Postgres 17, 2026-11-04). The cause: since Postgres 16 a role that creates another holds it with
  ADMIN but not SET (`createrole_self_grant` is empty by default), and the test switches to `ts_server`, `ts_accounts` and
  `ts_verify`. Granting the test user SET on the three (`GRANT ts_server TO jh WITH SET TRUE,
  INHERIT FALSE`, likewise the others) makes it pass (2026-11-06). `cargo test --workspace` stops
  at the first failing test binary; use `--no-fail-fast` to run the rest.
- A test that creates a database registers it with
  `triplespace_db::scratch::drop_at_thread_end` right after `CREATE DATABASE`, and the database
  is dropped when the test's thread ends, whether it passed or panicked. A `cleanup()` call at
  the end of the test is skipped by a failing assert, which left about a hundred scratch
  databases behind before 2026-11-06. `scatter-log-postgres`, which cannot depend on
  `triplespace-db`, has its own copy in `tests/postgres.rs`.

- `triplespace-server --test pages` `librarybase_pages_are_adopted_with_their_history` fails on a
  Postgres whose `timezone` is not UTC (Postgres.app and Homebrew default to the machine's zone):
  `talk_pages_became_threads` compares `original::text`, a `timestamptz` rendered in the
  session's zone, against a UTC string (2026-11-06, Homebrew Postgres 18 in
  `America/Los_Angeles`). Everything after that check in the test is not run. A session
  option in the test URL does not reach it, since the test rebuilds the URL for its scratch
  database; `ALTER ROLE <you> SET timezone = 'UTC'` on the test server makes it pass.

## The cloud sandbox

- Its disk fills with `target/debug/incremental` (~10 GB); build with `CARGO_INCREMENTAL=0` or
  clear it. A full disk shows up as linker "Bus error". A full `cargo test --workspace` with debug
  info leaves ~28 GB of test binaries; `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`
  keeps it near 15 GB.
- Its Postgres stops when the container is recycled; a "Connection refused" in a test means rerun
  `eval "$(tools/dev_env.sh)"`.
- It cannot install the pinned Rust toolchain and falls back to stable (`dev_env.sh` says so);
  rustfmt and clippy may differ from CI's, and there is no wasm32 target, so `cargo xtask wasm`
  cannot run there. Say so when reporting.
- Its Playwright browsers (build 1243, `/opt/pw-browsers`) are the ones `@playwright/test` 1.63.0
  expects, which is the version `ui/package.json` pins, so `ui/e2e` runs there as it is. They
  cannot be downloaded: if the pin moves past 1.63, the sandbox's browsers no longer match.
- `pkill -f triplespace-server` from a sandbox shell kills the calling shell too (its command line
  contains the name); match on the binary path (`pkill -f "^/home/.*/triplespace-server"`), and
  start a test server and query it within one call, since background processes die when the call
  ends.
- The tree tarred into the sandbox must leave out `docs/samples/wikidata/big` (140 MB,
  gitignored) but keep the rest of `docs/samples` (about 9 MB): many tests read it, among them
  `triplespace-ui`'s `entity` tests and the parity corpus's `wikidata` set
  (`tests/common/fixture_api.rs`), which panic on a missing file without it.
- Moving `target/` into a fresh copy of the tree (to reuse a build) leaves binaries that
  bake `env!("CARGO_MANIFEST_DIR")` pointing at the old copy, and Cargo does not rebuild
  them: `triplespace-ui`'s tests then panic on missing fixtures, and `cargo xtask skin-parity`
  writes its report under the old path. `cargo clean -p` the crates that use it (`grep -rl
  'env!("CARGO_MANIFEST_DIR")' crates xtask`) and `xtask`, or build in place.
- `tools/client_compat.sh` runs `target/debug` binaries: build `-p triplespace-cli -p
  triplespace-server` first, or it runs stale ones.
- `pkill -f '<pattern>'` inside a longer shell command matches that command's own `bash -c`
  line and kills it (exit 144, no output). Stop a test server by process name instead
  (`kill $(pgrep -x triplespace-ser)`; the name is cut to 15 characters).
- `triplespace instance create` refuses an owner password under 8 characters only after it has
  written the instance, so a short test password leaves an instance nobody can log in to.
  Use 8 or more.
- A test adoption that fails leaves an empty `view`, and an empty `view` passes the rebuild
  equivalence diff vacuously. Check `adopt`'s stderr and the row counts before trusting
  `REBUILD-IDENTICAL`.

## The Librarybase samples

- `docs/samples/librarybase/dump/librarybase-wikitext.xml.gz` (MediaWiki 1.39, 2023) and
  `librarybase-sample.xml.gz` (the rebuilt wiki, 2026) are dumps of two builds of the wiki and
  share revision IDs (both hold revision 4) and user IDs under different names (user 3 is
  `Harej` in one, `Librarybase` in the other). They cannot be adopted into one tenant: the
  second `adopt` fails on `activity_revid`, or refuses the accounts (0035 §5). A test that
  needs both pages and entities uses two scratch databases, as `triplespace-server/tests/pages.rs`'s
  XML export tests do (2026-11-08).
- `tools/mw_import_check.sh` needs a container daemon: on the Mac the `docker` CLI is
  installed but Docker Desktop is not running by default (`docker info` fails), and the script
  then exits 3 without running. Start Docker Desktop first.

## Wikidata dumps

- `latest-all.json.*` is not in ID order: it is written in shards by page ID and opens Q31, Q8,
  Q23, with properties scattered through it. `head -n N` gives neither the first N items nor
  the properties they use; cut slices by ID with `tools/wikidata_zindex.py` (real-dumps.md §3.2).
- `latest-all` changes under a long download; fetch the dated file from
  `wikidatawiki/entities/<date>/` and check it against that directory's `sha1sums.txt`.
- dumps.wikimedia.org gives one connection about 5 MB/s and limits connections per IP. The
  your.org mirror (`dumps.wikimedia.your.org/wikidatawiki/entities/`) serves the same files;
  `aria2c` with 2 connections to each runs about 16 MB/s. ftp.acc.umu.se did not carry the
  entity dumps when looked for (2026-11-03).
- Take the `.bz2` over the `.gz`: `lbzip2` decompresses it on every core (~500 MB/s on 16),
  where gzip runs on one. A partly downloaded `.bz2` decompresses up to where it stops, which is
  enough to try a tool on the dump's real opening.
- `sync --dump …json.bz2` reads the `.bz2` through the `bzip2` crate, on one core: about
  35 MB/s of JSON, below what a bootstrap load takes (15 KB an entity at 2,000+/s). It also opens
  the dump twice (once to tell XML from JSON), so it cannot be fed `lbzip2`'s output through a
  FIFO or `/dev/stdin`. For a whole-dump load, recompress the zindex archive to gzip, which it
  reads at hundreds of MB/s: `zstd -dc zindex/entities.jsonl.zst | pigz -1 -p 14 >
  wd-full.json.gz` (211 GB, 43 min on 16 cores; the dump's order, one entity per line).
- A statement ID's part after the `$` is not always a UUID: Wikibase accepts any 8-4-4-4-12
  letters and digits, and Wikidata has IDs written by hand under that rule
  (`Q126373047$5627445f-43cb-ed6d-3adb-tracklists01`, and Q126373054). Until 0017 A13 the
  parser required hexadecimal, so the whole-dump load of 2026-11-06/07 counted those two
  entities unreadable; they load now. Look for the same pattern if a dump reports a statement
  ID it cannot parse.
- On the benchmark VM the `jh` role cannot `CREATE EXTENSION pg_stat_statements` (superuser
  only), so a run script's own attempt fails and its later `pg_stat_statements_reset()` with it.
  Create it as `postgres` (`sudo -u postgres psql -d <db> -c 'CREATE EXTENSION
  pg_stat_statements'`) right after `createdb`.
- A monitoring loop that reads `/proc/stat` with `read -r _ u n s i w _` overwrites any `n`
  it has already set: a sweep of 2026-11-08 logged every load at "141 entities" for 2¾ hours
  (the CPU's nice time) while the loads ran normally. Give such variables names nothing else
  uses, and check a monitor's first lines against the database before leaving it running.

## The device bridge and James's checkout

- The bridge's shell runs in a Linux VM with the folder mounted, not on the Mac: no `cargo` there.
  Builds and tests run in the sandbox; the Mac gets patches.
- Deleting files in the mounted folder is refused unless James grants it, so git commands that
  write (`fetch`, `commit`, `pull`, `checkout`) leave `.lock` files behind or fail on the files
  they change. Avoid them on the mount; `git show HEAD:path > path` restores a file.
- `git apply` on the mount prints "unable to unlink … Operation not permitted" for every file but
  writes the files; check with `shasum -a 256 -c` against a sums file from the sandbox.
- The Mac's Python is 3.10: `docs/check.sh` adds the vendored `tomli` wheel itself; a checker
  run directly needs `PYTHONPATH=docs/tomli-2.4.1-py3-none-any.whl`.
- `ui/e2e/run.sh` uses GNU `date -d`; macOS's `date` has no `-d`, so put a `date` that is
  MacPorts' or Homebrew's `gdate` first on `PATH` (a one-file shim directory does). The suite
  also needs `npx playwright install chromium` once, and a Postgres admin URL
  (`postgres://jh@localhost/postgres`): it creates its own `tse2e` database.

## The code

- An instance partition's `log.partition.tenant` is NULL, while the `view` tables write the
  instance as `''`: a join between them on the tenant needs
  `p.tenant IS NOT DISTINCT FROM nullif(v.tenant, '')`, or it silently finds nothing.
- A load (a dump sync or a crawl) writes no `view.activity` rows for its mirror records, only
  its job's (03 §4.6), and the REST change view (`/entity/{id}/changes/{p}/{o}`) reads the
  activity row, so a loaded record has none. A test that needs `sync` rows runs the follower
  (`scatter_ingest::run_follow`, as `triplespace-server/tests/site.rs` does; or
  `triplespace follow --once --api` against a stand-in wiki, as `ui/e2e/run.sh` does with
  `ui/e2e/wiki.mjs`).

- A `PgCx` dropped after a failed write without `store.rollback(cx)` can leave the pooled
  connection mid-transaction and hang the next write on the same key. Always roll back explicitly,
  as `editpage` does.
- Partition IDs are 64-bit and stored as `bigint` bit for bit (`ids::partition_from_db`): half of
  them read back negative. Anything built from a partition read as `i64` must `cast_unsigned`
  first.
- `owner` holds every permission and reads every group but the suppression group
  (`includes = ["*"]`); it is still in no other group for edit restrictions and the lock-out. A
  test that needs a logged-in reader outside a group uses `suppress` as the group, or a non-owner
  account; tests that need the owner in a group grant it (`grant` helper).
- Every request evaluates against the tenant's groups (`triplespace_accounts::groups::registry`),
  not `GroupRegistry::default_registry()`; new code that evaluates or lists groups should do the
  same.
- Membership records project into `view.activity`, whose `logid` is unique per tenant: a test
  writing membership records needs distinct log IDs.
- Projections in steps 6–7 (addressing; later RDF) never run inside a write: a test that reads
  their output calls `App::catch_up_async` first, or reads through a module that does
  (`meta=notifications`).
- Actor records share their key (the account) across payloads: a projection looking up "the
  previous record under the key" must filter by payload type. Actor keys of adopted accounts use
  the tenant's derived issuer code (`ActorKey::numeric(issuer, id)`), not the tenant slug.
- A real instance has no `board` group: `action=protect read=board` on it is `ts-locked-out`,
  where `/acl` says `ts-unknown-group`. Define a group over `/group` first. The tests' `grant`
  helper bypasses this.
- A test that registers accounts at the default challenge (20 bits, one more per account from
  the same address) spends most of its time in `scatter_challenge::solve`: a debug build hashes
  at about 0.2 million a second, so the first account costs five seconds and the third twenty.
  `moderation accountability` took 68 s against 4 s with the challenge at 8 bits (2026-11-06).
  Every harness that registers lowers `challenge.bits` on its tenant (`cheap_challenge` in the
  server tests, `settings set` in `ui/e2e/run.sh` and `tools/acl_check.sh`), and
  `[profile.dev.package."*"]` optimizes dependencies; a new harness must do the same.
- The attack battery is `tools/acl_check.sh`. Registration is throttled at six accounts a day per
  address and the challenge rises a bit per account: a live script that registers users must stay
  under six and lower `challenge.bits` on its test tenant. `autoconfirmed` is implied, never
  granted. Needles in a leak probe must be specific: the fixture holds an item labelled "The
  Secret life of Adolf Hitler", and a page title that contains the hidden user's name echoes it.
- A test that inserts into `view.acl` directly (bypassing records) must move the ACL frontier
  (`offset` and row count), or the hidden-set cache answers "nothing hidden" from before.
- `tests/moderation.rs` has helpers for more accounts: `instance(name)` (a fresh database per
  test), `account(router, name, password)` (`action=createaccount`, solving the challenge) and
  `grant(app, name, group)` (a membership record written through the pipeline).
- REST handlers that return the Action API's `ApiError` need `#[allow(clippy::result_large_err)]`
  (as `triplespace-api-rest/src/acl.rs` does module-wide).
- Entities have no `view.page` row: code that checks "does this page ID exist" must also look in
  `view.entity`.
- An XML dump's entity JSON carries no snak `datatype`; `sync` fills them from the dump's property
  pages (`DumpPropertyTypes`). The API's `wbgetentities` carries them, so the follower applies no
  typing; a mock wiki built from dump text must add them or every state reads as changed.
- `Index::observe` is idempotent (a follower re-reads items): the same item with the same key is
  not a conflict, and a changed key releases the old one.
- A colon-form ID that is neither a registered keyed type nor one of the tenant's sources is
  `invalid-entity-id` on reads (it used to answer as an empty keyed entity of an unknown type).
- Applied migrations are checksummed (`ops.migration`): never edit one, even a comment; add a new
  migration.
- Pywikibot's API cache can mask whether a fix worked; clear it between runs.
- The fuzz targets (`fuzz/`, 22 §4.9) need a nightly toolchain and cargo-fuzz; `fuzz/run.sh`
  seeds and runs one. On a Mac whose Command Line Tools ship an incomplete
  `usr/include/c++/v1`, libfuzzer-sys's C++ build fails with `'cassert' file not found`
  although the SDK has it: point cc-rs at the SDK's copy,
  `CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1"`. Linux CI needs nothing.
