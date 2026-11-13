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

4. **Create a database and a `.env` file.** Every binary (`triplespace`, `triplespace-server`
   and `triplespace-web`) reads a `.env` file in the working directory before it reads its
   flags, so the instance's configuration lives in one place instead of your shell. Run them all
   from the directory that holds it.

   ```sh
   createdb triplespace
   cat > .env <<'END'
   TRIPLESPACE_DATABASE_URL=postgres://triplespace@localhost/triplespace
   TRIPLESPACE_KEY_FILE=triplespace.key
   TRIPLESPACE_LISTEN=127.0.0.1:8080
   TRIPLESPACE_MODE=development
   TRIPLESPACE_DEV_TENANT=example
   END
   ```

   Any flag that names an environment variable in its `--help` can be set here. A flag on the
   command line wins over a variable in the environment, and either wins over the file; the
   file never overrides what is already set. To read another file instead,
   `TRIPLESPACE_ENV_FILE=/etc/triplespace/server.env`. The repository's `.gitignore` already
   leaves out `.env` and `.env.*`: the file holds credentials, so keep it out of version control
   wherever it lives.

5. **Create the instance and its first wiki.** Run the setup, which asks its questions one at a
   time:

   ```sh
   triplespace instance setup
   ```

   It asks, in order:

   - **Postgres URL**, offering the one from `.env`. It checks that the database answers and
     holds no instance yet (if it already holds one, it offers to add a wiki instead).
   - **Instance key file**, offering `triplespace.key`. It is created with mode 0600 and signs
     the instance's log: keep it safe and back it up.
   - **Farm slug**: the instance's own permanent name.
   - **Providers to mirror**: every provider you may ever mirror, such as Wikidata. Choose
     generously: providers cannot be added after creation.
   - **The first wiki** (the primary tenant, whose accounts operate the instance): whether it
     is a new, empty wiki or an existing one to adopt (step 8), its slug, its base URL, and the
     name shown on its pages. Use `example` as the slug to match `TRIPLESPACE_DEV_TENANT` above.
   - **Its owner**: their account name, and a password, typed twice and never echoed. Leave it
     empty to set one later with `triplespace password set`.
   - **The farm base URL**, offering the first wiki's base.

   Each answer is checked as you give it, and a rejected one is asked again. Then it shows a
   summary and the equivalent `triplespace instance create` command line, and **writes nothing
   until you confirm**. Afterwards it offers to add more wikis (step 7).

   The setup needs a terminal. For a script, run the command line it showed you instead,
   for example:

   ```sh
   echo 'a long passphrase' > owner.pw
   triplespace instance create \
       --farm-slug myfarm \
       --tenant example --base https://example.wiki \
       --provider wikidata --provider internetdomains \
       --owner-name Alice --owner-password-file owner.pw
   ```

6. **Start the server.** It serves the API and the site, and takes its settings from `.env`.
   For local use, development mode answers any host as the `TRIPLESPACE_DEV_TENANT` wiki over
   plain HTTP:

   ```sh
   triplespace-server
   ```

   Then open <http://127.0.0.1:8080/> and log in as the owner.

   **In production**, set `TRIPLESPACE_MODE=production` and remove `TRIPLESPACE_DEV_TENANT`: the
   server then answers only for its wikis' hosts and expects a TLS-terminating proxy in front of
   each (see *Deploying behind a proxy* below). **Also give each part of the server its own
   login.** Connected as the database owner, as above, the server works, but the database does
   not keep the public API out of accounts' private data, and the server warns that it does not.
   Create a login for each part and grant it the part's role
   ([architecture 03](../architecture/03-storage-caches-and-search.md) §1.2):

   ```sql
   CREATE ROLE ts_server_login LOGIN PASSWORD '…' IN ROLE ts_server;
   CREATE ROLE ts_accounts_login LOGIN PASSWORD '…' IN ROLE ts_accounts;
   CREATE ROLE ts_notify_login LOGIN PASSWORD '…' IN ROLE ts_notify;
   ```

   The server's logins are not the owner's, so give the server a file of its own and keep `.env`
   for the `triplespace` commands (`migrate`, `instance setup`, `tenant setup`, `rebuild`),
   which run as the owner:

   ```sh
   cat > server.env <<'END'
   TRIPLESPACE_DATABASE_URL=postgres://ts_server_login:…@localhost/triplespace
   TRIPLESPACE_ACCOUNTS_DATABASE_URL=postgres://ts_accounts_login:…@localhost/triplespace
   TRIPLESPACE_NOTIFY_DATABASE_URL=postgres://ts_notify_login:…@localhost/triplespace
   TRIPLESPACE_KEY_FILE=triplespace.key
   TRIPLESPACE_LISTEN=127.0.0.1:8080
   TRIPLESPACE_MODE=production
   END
   chmod 600 .env server.env
   TRIPLESPACE_ENV_FILE=server.env triplespace-server
   ```

7. **Add another wiki (optional).** A second wiki (tenant) gets its own base URL, partitions,
   numbering and accounts. An instance with more than one wiki signs people in at its **auth
   host**, a host of its own that is neither a wiki's nor the farm's, and the server must know
   it. Put it in `.env` first (and in `server.env`, if the server has its own), so that the
   setup offers it and the server reads the same value:

   ```sh
   echo 'TRIPLESPACE_AUTH_BASE=https://auth.example.org' >> .env
   triplespace tenant setup
   ```

   It shows the instance and the wikis it already has, asks which owner is creating the new
   one if there is more than one, and confirms the auth base URL. Then it asks about the wiki as
   `instance setup` did: new or adopted, slug, base URL, name, and its owner's account and
   password. It shows a summary and the equivalent command, writes nothing until you confirm,
   and offers another wiki after each one. Restart the server afterwards, and check:

   ```sh
   triplespace tenant list
   ```

   Without a terminal, the same with flags (`--auth-base` is read from `.env` when omitted):

   ```sh
   triplespace tenant create --tenant other --base https://other.example \
       --name "Other wiki" --owner-name Bob --owner-password-file bob.pw
   ```

8. **Adopt an existing Wikibase (optional).** Adoption makes a wiki *become* a tenant, keeping
   its IDs, users and history. It is one-way: freeze the source first, because afterwards the
   tenant mints the next IDs. Create the tenant as an adopted wiki first: answer *An existing
   Wikibase or MediaWiki* in `instance setup` or `tenant setup`, which then asks for the source
   wiki's base URL and the owner's user ID *on the source*; or, with flags, `--adopt` and
   `--owner` as below. Then load the dump with `triplespace adopt`.

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
