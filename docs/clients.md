# Wikibase client libraries against Triplespace

What the common Wikibase client libraries need from a server, what Triplespace does about it,
and what a user of each library has to know. The reference for the API itself is
[`api/mediawiki-compat.md`](api/mediawiki-compat.md); this page is about the clients. Each
claim below is checked by `tools/client_compat.sh` in CI (job *Client compatibility*) against a
server loaded from the Librarybase sample fixture, with the client versions pinned in
`tools/requirements-compat.txt`.

## Support matrix

| Client | Version checked | Login | Read | Search | Edit with `baserevid` | New entity above the floor | Check |
|---|---|---|---|---|---|---|---|
| [WikibaseIntegrator](https://github.com/LeMyst/WikibaseIntegrator) | 0.12.15 | bot password | ✓ | ✓ | ✓ (stale → `editconflict`) | ✓ item and property | `tools/api_check.py --wbi-login` |
| [Pywikibot](https://www.mediawiki.org/wiki/Manual:Pywikibot) | 11.8.0 | bot password | ✓ | ✓ | ✓ (stale → `editconflict`) | ✓ item | `tools/pwb_check.py` |
| [WikidataIntegrator](https://github.com/SuLab/WikidataIntegrator) | 0.9.30 | bot password | ✓ | ✓ | `delete_statement` ✓; `write()` sends none (see below) | ✓ item | `tools/wdi_check.py` |

"Bot password" is the credential `triplespace subsidiary key` issues (0024 §4): `lgname` is
`{subsidiary}@{label}`, `lgpassword` is the secret. The same secret as `Authorization: Bearer
{key ID}.{secret}` works without a session for clients that prefer that.

## What the server does for them

Each library probes the server before it reads or writes, and each probe is a thing the server
has to answer the way MediaWiki does. These were found by running the libraries, not by reading
the reference, and each is covered by a unit test in `triplespace-api-action` (`paraminfo_tests`)
as well as by the CI job.

- **`meta=siteinfo` `general`.** Pywikibot indexes `thumblimits`, `imagelimits` and
  `magiclinks` as objects keyed by index and fails with a `KeyError` without them; it also
  reads `linktrail` and `misermode`. The `generator` string is parsed as a MediaWiki version
  and must be ≥ 1.31, so Triplespace reports `MediaWiki 1.43.9 (Triplespace {version})`: the
  reference release it is measured against (`api/mediawiki-compat.md` §1), then its own.
- **`action=paraminfo`.** Pywikibot asserts on the structure, not just the presence, of the
  modules it asks about: `main` with `action` as a submodule-typed parameter; `query` with
  `prop`, `list` and `meta` as submodule parameters carrying `limit`, and a `generator`
  parameter; `query+tokens` with `type`; `wbsearchentities` with `language` as a list of the
  term languages. Triplespace generates these from its own module tables (`ACTION_MODULES`,
  `QUERY_MODULES`), so paraminfo and dispatch cannot disagree.
- **`meta=wikibase`.** Pywikibot's data repository discovery needs
  `repo.url.{base,scriptpath,articlepath}`; without it `site.data_repository()` is `None` and
  nothing Wikibase works.
- **Snak datatypes.** All three libraries read `datatype` from every snak. An XML dump's
  stored JSON carries none (Wikibase adds them at output), so adoption types each snak from
  the dump's own property pages (`scatter-adapter-wikidata::adoption::type_snaks`); a snak on a
  property the dump does not define stays untyped and is counted on the job's summary line.
  Pywikibot and WikidataIntegrator also *fetch* a statement's property, so an item whose
  property is missing fails to load in both. A partial dump therefore needs every property
  page it refers to — `tools/dump_slice.py --all-properties`, or `--defined-only` to drop the
  items it cannot type.
- **`editconflict`.** A write whose `baserevid` is not the entity's latest is refused with
  `editconflict` and `currentrevid` (0006 §8), which is what all three libraries' retry and
  conflict handling expects.

## What a user of each library has to know

**WikibaseIntegrator** — nothing beyond `MEDIAWIKI_API_URL`; `wbi_login.Login` with the bot
password works unchanged.

**Pywikibot** — a family file is needed, as for any third-party Wikibase; `tools/pwb_check.py`
writes a minimal one (`WikibaseFamily`, one code, `/w` script path, `http` or `https`) and can
be copied. Two cache notes:

- Pywikibot caches `paraminfo` and `siteinfo` on disk for **30 days** under its `apicache`
  directory. After a server upgrade that changes either, clear that directory (or set
  `config.base_dir` to a fresh one, as the check does), or the old answers keep being used.
- `PYWIKIBOT_NO_USER_CONFIG=1` plus settings made in code is enough; a `user-config.py` is
  optional.

**WikidataIntegrator** — four things, none of them Triplespace-specific but all of them
blocking:

- 0.9.30 imports `pkg_resources`, which setuptools 80 removed: install with
  `pip install "setuptools<80"` (79.0.1 works) or the import fails with
  `ModuleNotFoundError: No module named 'pkg_resources'` even though the package is present.
- `WDItemEngine`'s constructor queries a SPARQL endpoint (the mapping-relation helper and the
  distinct-value-constraint lookup) and retries with exponential backoff **forever** by
  default (`BACKOFF_MAX_TRIES = None`). Triplespace has no SPARQL endpoint yet (0059), so set
  `wdi_config["BACKOFF_MAX_TRIES"]` to a small number and `wdi_config["BACKOFF_MAX_VALUE"]` to a
  second or two, or the first `WDItemEngine(...)` never returns. With `core_props=set()` the
  constructor skips the lookups it can.
- The default `sparql_endpoint_url` is **Wikidata's**. Point it at the Triplespace server (it
  will 404 and WDI continues) or the constructor silently asks Wikidata about your items.
- `write()` sends no `baserevid`, so WDI has no edit-conflict protection on `wbeditentity`; a
  concurrent edit between its read and its write is overwritten. `delete_statement()` does
  send one and is refused on a stale revision. On an API error WDI prints the error body and
  returns `None` rather than raising.

## Running the checks yourself

```sh
cargo build -p triplespace-cli -p triplespace-server
python3 -m venv .venv && .venv/bin/pip install -r tools/requirements-compat.txt
TRIPLESPACE_TEST_DATABASE_URL=postgres://… PYTHON=.venv/bin/python tools/client_compat.sh
```

The script builds a throwaway database, adopts the sample fixture, issues a bot key, starts a
server on `127.0.0.1:18280` with its site off, and runs the three checks; every file it makes
is in a temporary directory removed on exit. `CHECKS="pwb"` runs one; `KEEP_DB=1` keeps the
database for a look. Against a running instance of your own, each check also runs on its own —
see `tools/README.md`.
