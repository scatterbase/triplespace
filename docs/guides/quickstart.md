# Quickstart

From a checkout of the repository to a running instance with its first wiki, then the optional
steps: more wikis, adopting an existing Wikibase, mirroring a remote one. Every command runs from
the repository root unless a step says otherwise.

## Install and run

You need Rust (the toolchain pinned in `rust-toolchain.toml` installs itself through `rustup`),
Node 22 or later, and PostgreSQL (CI uses 18). A small instance needs Postgres and nothing else.

1. **Build the site's front end.** This installs the pinned packages and writes `ui/dist`, which
   the server embeds.

   ```sh
   cd ui
   npm ci && npm run build
   ```

2. **Generate the component manifest.** From the repository root, record the crates, frontend
   packages and licences that `Special:Version` lists. The binaries embed it at build time, so
   run this after the front end and before the Rust build.

   ```sh
   cd ..
   cargo xtask manifest
   ```

3. **Build the binaries.** This produces `triplespace` (the command line), `triplespace-server`
   (the API and site) and `triplespace-web` (the optional stateless web tier) in
   `target/release/`.

   ```sh
   cargo build --workspace --release
   export PATH="$PWD/target/release:$PATH"
   ```

4. **Create a database and point Triplespace at it.** Every binary reads
   `TRIPLESPACE_DATABASE_URL`, or a `.env` file in the working directory.

   ```sh
   createdb triplespace
   export TRIPLESPACE_DATABASE_URL=postgres://triplespace@localhost/triplespace
   ```

5. **Create the instance and its first wiki.** This writes the schema, the instance signing key
   (`triplespace.key`, mode 0600; keep it safe) and the primary tenant with its owner account.
   The farm slug is the instance's own permanent name. List here every provider you may later
   mirror: providers cannot be added after creation.

   The easiest way is to answer its questions: `triplespace instance setup` asks for each of
   these in turn, checks every answer, shows the equivalent `instance create` command, and
   creates nothing until you confirm. Afterwards it offers to add more wikis. The same, with
   flags:

   ```sh
   echo 'a long passphrase' > owner.pw
   triplespace instance create \
       --farm-slug myfarm \
       --tenant example --base https://example.wiki \
       --provider wikidata --provider internetdomains \
       --owner-name Alice --owner-password-file owner.pw
   ```

6. **Start the server.** It serves the API and the site. For local use, development mode answers
   any host as one tenant over plain HTTP; in production it expects a TLS-terminating proxy in
   front for each tenant's host.

   ```sh
   triplespace-server --listen 127.0.0.1:8080 --mode development --dev-tenant example
   ```

   Then open <http://127.0.0.1:8080/> and log in as `Alice`.

   **In production, give each part of the server its own login.** Connected as the owner, as
   above, the server works, but the database does not keep the public API out of accounts'
   private data, and the server warns that it does not. Create a login for each part, grant it
   the part's role, and give the server a URL for each ([architecture 03](../architecture/03-storage-caches-and-search.md) §1.2):

   ```sql
   CREATE ROLE ts_server_login LOGIN PASSWORD '…' IN ROLE ts_server;
   CREATE ROLE ts_accounts_login LOGIN PASSWORD '…' IN ROLE ts_accounts;
   CREATE ROLE ts_notify_login LOGIN PASSWORD '…' IN ROLE ts_notify;
   ```

   ```sh
   export TRIPLESPACE_DATABASE_URL=postgres://ts_server_login:…@localhost/triplespace
   export TRIPLESPACE_ACCOUNTS_DATABASE_URL=postgres://ts_accounts_login:…@localhost/triplespace
   export TRIPLESPACE_NOTIFY_DATABASE_URL=postgres://ts_notify_login:…@localhost/triplespace
   ```

   These are the server's. Run `triplespace` commands (`migrate`, `instance create`, `rebuild`)
   as the owner, by passing its URL: `--database postgres://triplespace@localhost/triplespace`.

7. **Add another wiki (optional).** A second tenant gets its own base URL, partitions, numbering
   and accounts. It needs the instance's auth host, a host of its own that is neither a tenant's
   nor the farm's; give the server the same `TRIPLESPACE_AUTH_BASE`. `triplespace tenant setup`
   asks for all of it at a terminal; with flags:

   ```sh
   export TRIPLESPACE_AUTH_BASE=https://auth.example.org
   triplespace tenant create --tenant other --base https://other.example \
       --name "Other wiki" --owner-name Bob --owner-password-file bob.pw
   triplespace tenant list
   ```

8. **Adopt an existing Wikibase (optional).** Adoption makes a wiki *become* a tenant, keeping
   its IDs, users and history. It is one-way: freeze the source first, because afterwards the
   tenant mints the next IDs. The tenant must be created with `--adopt` (at `instance create` or
   `tenant create`), and `--owner` is the owner's user ID *on the source*.

   ```sh
   # On the frozen source wiki: a full-history XML dump, and its ID counters
   php maintenance/run.php dumpBackup --full --include-files=no | gzip > mywiki-20261101.xml.gz
   mysql -N -e 'SELECT * FROM wb_id_counters' <db>

   # Here
   triplespace tenant create --tenant mywiki --base https://mywiki.example \
       --adopt https://mywiki.example/ --owner 7 --owner-name Alice --owner-password-file owner.pw
   triplespace adopt --tenant mywiki --dump mywiki-20261101.xml.gz \
       --source https://mywiki.example/ --version mywiki-20261101 --frozen \
       --counters item=350000,property=1200
   ```

   Add `--entity-source wikidata=WD` if the source used federated properties from Wikidata.
   Re-running over the same dump is a no-op.

9. **Mirror a remote Wikibase (optional).** Syncing loads a provider's entity dump into its
   mirror, under the provider's prefix (`WDQ42`). A first load into an empty mirror uses the fast
   bootstrap path. Then follow the wiki's recent changes to stay current.

   ```sh
   triplespace sync --provider wikidata --dump wikidata-20261026-all.json.bz2 --version 20261026
   triplespace follow --provider wikidata            # keeps polling; --once to catch up and stop
   ```

   `--snapshot` declares a dump complete, so entities it no longer carries are tombstoned.
   `triplespace upstream-login set` stores a bot password for the follower to use. A `.bz2`
   dump is decompressed on one core; for a full Wikidata load, recompress it to `.gz` first
   (`lbzip2 -dc … | pigz -1`).

10. **Check the result.** `status` shows every partition's head and whether each projection is
    current; `rebuild` replays every projection from the log.

    ```sh
    triplespace status
    ```

## Bots and tools

Bots log in as **subsidiary accounts** of a person, never as the person:

```sh
triplespace subsidiary create --tenant example --name "Alice bot" --operator Alice --group bot
triplespace subsidiary key --tenant example --name "Alice bot" --label laptop \
    --grant editentity --grant highvolume
```

The key is printed once. `lgname=Alice bot@laptop` with the secret is a MediaWiki bot password;
`Authorization: Bearer {key ID}.{secret}` is the same credential without a session.
[Client libraries](../clients.md) has what each client library needs.

## Deploying behind a proxy

In production mode the server answers only for its tenants' hosts, the farm base and the auth
base (anything else gets 421), and reads `X-Forwarded-For` only from proxies it trusts:
`--trusted-proxy 10.0.0.2,10.0.1.0/24`, or a forwarder key (`triplespace instance forwarder
create`) that the proxy sends as `Triplespace-Forwarder`. To scale the site apart from the API,
run the server with `--ui off` and `triplespace-web --api http://api.svc:8080` beside it;
`triplespace-web routes --format caddy` (or `nginx`, `haproxy`) prints which paths go where.
Chapter [18](../architecture/18-api.md) and the `--help` of each binary have the details.

## Next

- [Moving an instance to another installation](moving-an-instance.md), when this one needs a new
  server or a new database.
