# Triplespace

Triplespace is Wikibase reimagined as an append-only log, written in Rust. It aims to:

- **Load knowledge graphs fast.** Bulk ingest is a first-class path, not an afterthought: an
  empty mirror loads through a bootstrap path built for Wikidata-sized dumps.
- **Hold foreign entities alongside local ones.** Wikidata's `WDQ42`, OpenAlex's `OAW123` and a
  domain name as `domain:example.org` sit beside a wiki's own items without being copied into it,
  and local statements about them live in the wiki's own graph.
- **Keep every change verifiable and attributed.** Every write is a record in an append-only log
  whose history is Merkle-verified and checkpoints signed, with full attribution and support for
  erasure. The serving tables can
  be rebuilt from the log at any time.
- **Host many wikis on one instance**, each with its own URL, numbering, accounts and
  configuration.

**Compatibility.** Triplespace speaks the Wikibase data model and the Wikibase and MediaWiki
APIs, so existing tools work against it unchanged:

- The Action API at `/w/api.php`: `meta=siteinfo`, `meta=tokens`, `action=login` with bot
  passwords, `clientlogin`, `wbgetentities`, `wbsearchentities`, and the edit modules from
  `wbeditentity` to `wbremovereferences`.
- Entity documents at `/wiki/Special:EntityData/Q6.json` and `/entity/Q6`, in Wikibase's JSON
  serialization (Wikibase REL1_43, RDF format 1.0.0).
- **WikibaseIntegrator, Pywikibot and WikidataIntegrator** log in, read and edit unchanged; CI
  runs all three against every push ([`docs/clients.md`](docs/clients.md)).
- Existing Wikibase installs can be **adopted** from their XML dumps, keeping their `Q`/`P`
  numbers, page IDs, revision IDs, users and full edit history.

Anything not yet served answers with a MediaWiki-shaped error rather than silence, so a client's
failure names the gap. The contracts are in [`docs/api/`](docs/api/).

## ⚠️ Not ready for production

Triplespace is under active development and is **not yet suitable for production use**. In
particular:

- **The permission model (ACLs, groups, tenant isolation) has had limited testing.** It is
  exercised by one automated attack battery (`tools/acl_check.sh`) and has not been
  independently audited. Do not rely on it to protect private or sensitive data.
- On-disk formats and the database schema may still change between versions.
- Parts of the Wikibase and MediaWiki API surface are not yet served (see
  [`docs/api/`](docs/api/)).

Run it on data you can afford to reload, and report what breaks. Report security problems privately,
as [`SECURITY.md`](SECURITY.md) describes.

## Getting started

The [user guides](docs/guides/README.md) walk through running Triplespace:

- **[Quickstart](docs/guides/quickstart.md)**: build it, create an instance and its first wiki,
  start the server, and optionally add wikis, adopt an existing Wikibase or mirror a remote one;
  then bots and tools, and deploying behind a proxy.
- **[Moving an instance](docs/guides/moving-an-instance.md)**: take your instance, with its
  wikis, history, accounts and keys, from one Triplespace installation to another.

## Where things are

The documentation is also published, with search, at
[triplespace.readthedocs.io](https://triplespace.readthedocs.io/), including a page per
chapter of what the code has built of it.

| Path | Holds |
|---|---|
| `docs/guides/` | **User guides**: the [Quickstart](docs/guides/quickstart.md) and [moving an instance](docs/guides/moving-an-instance.md) to another installation. |
| `docs/architecture/` | **What holds now**, by subject. Start with [00](docs/architecture/00-overview.md), then [22](docs/architecture/22-crates-and-stack.md) for the crate map. |
| `docs/decisions/` | **Why and when**: the architecture decision records (ADRs), which point into the chapters for current text. |
| `docs/registry/` | Names and codes the crates embed: providers, namespaces, settings, routes, permissions. |
| `docs/api/` | Compatibility contracts with Wikibase and MediaWiki, and test vectors. |
| `docs/testing/` | Testing against real dumps, write-path performance, and environment pitfalls. |
| `crates/` | One Cargo workspace: `scatter-*` crates (shared with Scatterbase) and `triplespace-*` crates (the product). |
| `tools/` | Scripts outside the build: dump slicing, client-compatibility and ACL checks ([`tools/README.md`](tools/README.md)). |
| `xtask/` | Repository tasks: `deps`, `wasm`, `manifest`, `skin-parity`. |
| `ui/`, `i18n/` | The site's front end (Codex, Vite) and its interface messages. |

`tools/adr.py` looks things up across the docs: `where <phrase>`, `show 0013 §7`, `code 0013 §7`.

## Contributing

Contributions are welcome. James Hare (Scatter LLC) maintains Triplespace and decides what
merges.

**Before you start**, read [`AGENTS.md`](AGENTS.md). It is the working guide for humans and
agents alike: where things live, the crate rules, the documentation rules, and the full list of
checks. Larger changes start with the design: a new behaviour, crate, dependency or registry
code is decided in an ADR and written into the architecture chapters
(`.claude/skills/triplespace-adr/`).

**Before you open a pull request**, run the checks your change touches (CI runs all of them):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace     # set TRIPLESPACE_TEST_DATABASE_URL, or the DB tests pass vacuously
cargo xtask deps
docs/check.sh
(cd ui && npm run lint && npm test)
```

Run `docs/check.sh --write` after a code change as well as a documentation one: some generated
files are written from the code (which crates exist, each crate's first `//!` paragraph, the
`implements` claims of `docs/architecture/24-documentation.md` §15), and CI fails while they are
stale. Commit what it regenerates with the change.

**Conflicts in generated files fix themselves.** When `main` moves and your pull request's
generated files conflict with it (`INDEX.md`, `OPEN.md`, `CRATES.md`, the generated tables in the
chapters), a workflow rebases your branch onto `main`, regenerates them and pushes the rebased
branch (with `--force-with-lease`, so it never overwrites a push of yours), so run `git pull
--rebase` before you push again. A conflict in anything else is left for you. Locally, run
`python3 tools/resolve_generated.py` while a merge or rebase is stopped on such a conflict.

Say in the pull request which checks you ran and which you could not. In a fresh Linux sandbox,
`eval "$(tools/dev_env.sh)"` starts Postgres and sets the test database URL.

**Working with coding agents.** Much of Triplespace is drafted with AI coding agents, and the
repository is set up for them: `AGENTS.md` (which `CLAUDE.md` includes) is written as their
brief, and `.claude/skills/` holds skills for finding things in the design and for writing ADRs.
If you use an agent:

- **Point it at `AGENTS.md`** and let it follow the rules there. Agents draft; people decide.
- **Do not let it commit or push on its own.** Review its work before it leaves your machine.
- **Have it flag the calls it made.** An assumption or design choice made while building goes
  into the owning ADR's Amendment log marked `unconfirmed`, so it shows up in
  `docs/decisions/OPEN.md` for review, never buried in a commit message or a note.
- **Never hand-edit generated files** (`INDEX.md`, `OPEN.md`, `CRATES.md`, the generated tables
  in the chapters, the last column of a status table); regenerate them with `docs/check.sh
  --write`, after code changes as well as documentation ones, and resolve conflicts in them with
  `tools/resolve_generated.py`.
- **Test against real data and real clients** where it can: the Librarybase samples and
  `tools/client_compat.sh`, not only unit tests.
- **Disclose it.** Say in the pull request that an agent wrote or helped write the change. You
  remain responsible for what you submit.

**Security issues** go through GitHub's private vulnerability reporting, never a public issue.
[`SECURITY.md`](SECURITY.md) has the procedure, what to expect, and what is in scope.

## Licence

Code in this repository is GPL-3.0-or-later (`LICENSE`) unless otherwise stated. That covers
the shared `scatter-*` crates and the `triplespace-*` crates alike, `ui/`, `tools/` and `xtask/`;
there is no commercial licence. What states otherwise: `docs/` is CC0-1.0 (`docs/LICENSE`);
`docs/samples/` holds third-party captures under their own licences; vendored third-party code
and assets keep the licence that comes with them. Contributions are accepted under
GPL-3.0-or-later, and no contributor licence agreement is needed. See chapter 22 §3.3 and §3.4.
