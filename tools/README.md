# tools/

Scripts that are not part of the build. The Python ones need only the standard library unless
they say otherwise; every script prints its own usage with `--help`.

| Script | What it does |
|---|---|
| `wikidata-sample.py` | Draws a uniformly random sample of Wikidata items from a JSON dump, in the dump's shape, for the classifier audit (0003 §10). |
| `dump_slice.py` | Cuts a smaller, valid MediaWiki XML dump out of a Wikibase dump: the first N entities (`--limit`), or a reproducible random sample of items (`--sample N --seed S`), with `--all-properties` to carry every property page so each snak keeps its datatype, `--include Q1,P5` to force entities in, and `--defined-only` to drop items that use a property the slice lacks. Streams the input; `.gz` or plain `.xml` either side. The Librarybase sample fixture (below) is made with it. |
| `api_check.py` | Checks adopted entities through the Action API after `adopt` (test plan §1.3–1.4): `Special:EntityData`, `wbgetentities`, `wbsearchentities`, `view.identifier`, snak datatypes, and with `--compare` the source wiki's output. With `--wbi-login` it runs the WikibaseIntegrator section: bot login, label edit, statement add/remove, a stale `baserevid` refused, a new item (and property) above the counter floor. |
| `pwb_check.py` | The same story through Pywikibot: generates a family file and config in a temporary directory, then site info, paraminfo, data repository, login, read, property, search, `editLabels`, `addClaim`, `removeClaims`, stale `baserevid`, new item. Needs `pywikibot`. |
| `wdi_check.py` | The same through WikidataIntegrator, with its SPARQL backoff bounded and its endpoint pointed at the server: login, read, search, property, `set_label` + `write()`, `WDString` append, stale and current `delete_statement`, new item. Needs `wikidataintegrator` and `setuptools<80`. |
| `client_compat.sh` | Runs all three against a server it sets up itself (database, `instance create`, `adopt` of the sample fixture, `sync`, a bot key, `triplespace-server --ui off`), each as a hard pass/fail; CI's *Client compatibility* job. Needs the binaries built, `psql`, and a Python with `requirements-compat.txt` installed (`PYTHON=` to name it). Everything it writes is in a temporary directory removed on exit. `CHECKS="wbi pwb wdi"` selects, `KEEP_DB=1` keeps the database, `ITEM=Q5` names the item to edit (default: the lowest-numbered item), `FIXTURE=` another dump. |
| `requirements-compat.txt` | The pinned client versions `client_compat.sh` is run against. A bump is deliberate: run the script, then record the versions and verdict in the test plan's results log and `docs/clients.md`. |

The three checks take `--api` (default `http://127.0.0.1:8080`), the bot login as `--login
name@label` (`--wbi-login` for `api_check.py`), and the secret as `--secret` or
`TRIPLESPACE_WBI_SECRET`. Their writes are permanent on the tenant they run against, so point
them at a test database; `--no-write` (Pywikibot and WDI checks) keeps to reads.

## The Librarybase sample fixture

`crates/triplespace-server/tests/fixtures/librarybase-sample.xml.gz` is 200 random Librarybase
items with every property page, made from the frozen 2026-10-05 dump:

```sh
tools/dump_slice.py librarybase-20261005.xml.gz \
    crates/triplespace-server/tests/fixtures/librarybase-sample.xml.gz \
    --sample 200 --seed 1 --all-properties --include Q1 --defined-only
```

Made from the full dump, `--defined-only` drops nothing and `Q1` (the Librarybase item itself)
is in; made from a prefix slice it drops the items whose properties the slice does not
define. The same seed on the same dump gives the same fixture (the gzip header's timestamp
differs; the XML does not). `adopt` is run on it without `--counters`, so the floors come from
the sample's highest IDs; the checks only require new entities to land above them.
