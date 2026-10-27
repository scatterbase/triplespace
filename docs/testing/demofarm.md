# Demofarm: one instance, three tenants, the Credit Unions Wiki

- **Goal:** exercise what Triplespace has built so far, end to end and on one instance, with a
  small real Wikibase: tenancy, adoption with history, the mirror sync (upsert, snapshot,
  redirects, rejects, the cursor), tenant overlays on mirrored entities, the Action API and the
  three client libraries, the site, discussions and watchlists, and rebuild equivalence.
- **Layout:** instance (farm) `demofarm`; primary tenant `control`; tenant `opencredit` **adopts**
  the Credit Unions Wiki; tenant `cumirror` **reads it as an entity source** named `cu` (0078), with
  tenant-specific IDs (`cu:Q8`), synced first from the dump and then from the live wiki at
  `creditunions.miraheze.org`. The Credit Unions Wiki is not registered as a provider: it gets no
  global code (James, 2026-10-26).
- **History:** written 2026-10-25 against `main` at `93941ca`; revised 2026-10-26 for
  [0091](../decisions/0091-one-wikibase-adapter.md) and for the entity-source route. Companion to
  [`real-dumps.md`](real-dumps.md), whose stage 0 (build, database, rebuild equivalence) is assumed
  here; add rows to the results log at the end of this file.
- **Where to run:** James's Mac, from the checkout, with a local Postgres. Phase 4 needs network
  access to Miraheze, which the cloud sandbox does not have. Work files go in `~/ts-test/demofarm/`.

## The dump

`creditunionswiki-20261004.xml` (37 MB, uncompressed, in `data/` in the checkout, which is gitignored;
copy it to the work directory and do not commit it). A full-history export of OpenCredit, MediaWiki 1.46.2.

| | |
|---|---|
| Pages | 5,083; page IDs 1–5,083, no gaps |
| Revisions | 5,088; revision IDs 1–5,088. Every entity has one revision; Main Page has six |
| Items | 5,070, Q1–Q5095 (25 numbers unused or deleted) |
| Properties | 12, P1–P12: `string` ×8, `quantity` ×4 (P6–P9), `wikibase-item` ×2 (P4, P12) |
| Wikitext | Main Page only (ns 0); no talk pages, no redirects, no files |
| Accounts | MediaWiki default (1, 1 edit), Harej (2, 24), Harej bot (12, 5,063); no IP edits |
| Statements | 58,337, every one with one reference (P12 → Q7, the NCUA dataset); no qualifiers |
| Terms | English labels on everything; descriptions on the class and unit items; no sitelinks |

What it exercises that Librarybase and internetdomains did not:

- **Miraheze's namespace numbers.** Item is 860 and Property 862, not 120/122. Entity pages are
  recognized by content model, so this should be harmless; check that it is.
- **Unit IRIs.** 20,252 quantity values have a unit pointing at the wiki's own items:
  `https://creditunions.miraheze.org/entity/Q6` (U.S. Dollars, 15,189) and `…/Q4` (members,
  5,063). Units are stored as raw IRIs today; audit item G21 (prefixed unit and globe IDs) is not
  applied. This is the first real dataset where that shows.
- **Empty maps serialized as lists.** 7 items have `"claims": []`, 5,066 have `"aliases": []`,
  all have `"sitelinks": []`.
- **A system account.** "MediaWiki default" (user 1) authored Main Page's first revision.
- **Full history on a wikitext page** (Main Page, six revisions by two users).

## What the build can and cannot do today

Read this first: points 1–3 decide what phase 0 has to build before phases 3 and 4 can run.

1. **The adapter is ready.** Since [0091](../decisions/0091-one-wikibase-adapter.md), `sync`
   chooses the adapter by the provider's `adapter` field, and `scatter-adapter-wikibase` reads any
   Wikibase dump, XML or JSON, with or without key-map rules. A Credit Unions dump synced through it
   under a scratch registry entry in the cloud sandbox (2026-10-26): 5,082 created, 0 rejected,
   21 s on a fresh database, then all unchanged on re-sync and on a snapshot.
2. **Entity sources (0078) are not built at all.** `cumirror`'s source is an `entity-source`
   record in its own `config`: a name (`cu`), a number from the tenant's range 2^12–2^13 − 1, a
   `source/cu` partition in the tenant, and IDs of the form `cu:Q8` that only `cumirror` reads
   ([05](../architecture/05-providers-and-ingest.md) §6). None of the record, the partition, the
   colon-form IDs, the source namespace or the reads exists yet. Phase 0 builds the part this test
   needs.
3. **0078 has no dump path.** Its `source/{name}` partition is written only by the source's
   **fetch job**, entity by entity through the source's `api` (05 §6.4, shallow mirroring). A dump
   sync into a source partition is not in the design. Phase 3 needs it, so phase 0 needs a small
   amendment to 0078 first (decision D1).
4. **Nothing follows a live wiki.** There is no API client in the ingest path, no recent-changes
   follower and no fetch job (0011 §1–5, 0078 §4, 0022 are unbuilt). Phase 4's "sync from the
   Miraheze wiki" is either the 0078 fetch job, if phase 0 builds it, or a fresh dump fetched
   through the API (P3) and synced like phase 3.
5. **Adoption expects a frozen source.** `adopt --frozen` is the operator's declaration; Miraheze
   will keep accepting edits, so `opencredit` is a fork as of 2026-10-04. That is fine for a
   test, and it is what makes phase 4's upstream edits possible.
6. **Known gaps this will hit:** G21 (unit IRIs stay raw Miraheze IRIs), `wbgetentities`
   `modified` is append time, the sync loads the whole dump into memory (fine at 5k), no `record`
   ACLs from adoption, no SPARQL endpoint.

## Decisions for James

- **D1. A dump sync into an entity source.** 0078 fills `source/{name}` only through its fetch
  job. Recommended: amend 0078 §4 so a source with a dump may also be loaded by the ordinary sync
  job (upsert and snapshot, the same cursor and rejects), its records attested as the fetch job's
  are. A source's history policy is `latest`, so the two writers never conflict over history. The
  alternative is to build the fetch job first and skip the dump.
- **D2. Names.** Tenant slugs `opencredit` (adopts) and `cumirror` (reads the source); source name
  `cu`, label "Credit Unions Wiki". A source has no issuer (05 §6.4), so its upstream actors are
  not recorded.
- **D3. How to fetch from the live wiki.** If the fetch job is not built, a tool that builds an XML
  export through the Action API (`export` on a page generator), so phase 4 goes through the same
  XML reader and datatype typing as phase 3, and merged items arrive as redirects. A JSON fetch
  through `wbgetentities` would lose redirects (a merge would read as a deletion).
- ~~**D1 (2026-10-25). Registry provider now, or wait for 0078?**~~ *Settled by James on
  2026-10-26: the Credit Unions Wiki is not registered; it is a tenant entity source.*

## Phase 0: prerequisite code (one branch, `demofarm-sources`)

- **P1. The entity-source slice of 0078 this test needs:**
  - the `entity-source` config record, written with `ts-config`, and a CLI to write it
    (`triplespace source add --tenant cumirror --name cu --label "Credit Unions Wiki" --type
    Q=item --type P=property --iri 'https://creditunions.miraheze.org/entity/{upstream_id}'
    --api https://creditunions.miraheze.org/w/api.php --revision-ids --licence CC-BY-SA-4.0`;
    the command name is a proposal), with the server assigning the number;
  - the `source/cu` partition in the tenant, opened when the record is written;
  - colon-form source IDs (`cu:Q8`, 05 §6.2) in the tenant's table of names (§6.3), parsed and
    canonicalized, as statement subjects, values and GUID prefixes;
  - `scatter-adapter-wikibase` parameterised by an entity-source record as well as a registry
    entry, rewriting `Q8` to `cu:Q8`;
  - `sync --tenant cumirror --source cu`, writing to `source/cu` (D1);
  - reads: `wbgetentities`, `Special:EntityData`, search and the entity page for `cu:` IDs on the
    reading tenant only, with tenant rows and no shared row (05 §6.4).
- **P2. ~~A generic Wikibase path in `sync`.~~** Done by [0091](../decisions/0091-one-wikibase-adapter.md).
- **P3. `tools/wikibase_fetch.py`** (only if the fetch job is not built). `--api URL --out
  FILE.xml.gz [--since ISO8601]`. Reads `siteinfo` (namespaces whose default model is
  `wikibase-item`/`wikibase-property`); without `--since`, `generator=allpages` over them; with
  `--since`, the titles from `list=recentchanges` (`rctype=edit|new|log`, those namespaces).
  Exports current revisions in batches of 50 (`action=query&export&exportnowrap`), and writes one
  well-formed dump with one `<siteinfo>`. Polite: `maxlag`, a User-Agent naming Scatter LLC, one
  request at a time.

**Pass:** fmt, clippy, the workspace tests with a database, `docs/check.sh`; `sync --source cu`
on a ten-entity slice prints created 10.

## Phase 1: the farm

```sh
cd ~/git/triplespace && git switch demofarm-sources
cargo build --release --workspace && (cd ui && npm ci && npm run build) \
  && cargo build --release -p triplespace-server -p triplespace-web
export PATH="$PWD/target/release:$PATH"
mkdir -p ~/ts-test/demofarm && cd ~/ts-test/demofarm
cp ~/git/triplespace/data/creditunionswiki-20261004.xml . && shasum -a 256 creditunionswiki-20261004.xml

# /etc/hosts: 127.0.0.1 demofarm.test control.demofarm.test opencredit.demofarm.test cumirror.demofarm.test auth.demofarm.test
createdb ts_demofarm
export TRIPLESPACE_DATABASE_URL=postgres://localhost/ts_demofarm
export TRIPLESPACE_KEY_FILE=$PWD/demofarm.key
export TRIPLESPACE_AUTH_BASE=http://auth.demofarm.test:8080
echo 'demofarm owner passphrase' > owner.pw && chmod 600 owner.pw

triplespace instance create --key-file demofarm.key \
    --farm-slug demofarm --farm-base http://demofarm.test:8080 \
    --tenant control --base http://control.demofarm.test:8080 --name Control \
    --owner-name James --owner-password-file owner.pw

triplespace tenant create --tenant opencredit --base http://opencredit.demofarm.test:8080 \
    --name OpenCredit --adopt https://creditunions.miraheze.org/ \
    --owner 2 --owner-name Harej --owner-password-file owner.pw

triplespace tenant create --tenant cumirror --base http://cumirror.demofarm.test:8080 \
    --name "Credit unions (mirror)" --owner-name James --owner-password-file owner.pw

triplespace source add --tenant cumirror --name cu --label "Credit Unions Wiki" \
    --type Q=item --type P=property \
    --iri 'https://creditunions.miraheze.org/entity/{upstream_id}' \
    --api https://creditunions.miraheze.org/w/api.php --revision-ids --licence CC-BY-SA-4.0

triplespace tenant list && triplespace status
```

Check the wiki's licence before writing the record; CC BY-SA 4.0 is Miraheze's default.

**Pass:**
- Three tenants, each with its own issuer code (0079), `owner` group and owner account; the farm
  slug `demofarm` is refused as a tenant slug.
- A second `instance create` on `ts_demofarm` refuses. `tenant create` without
  `TRIPLESPACE_AUTH_BASE` refuses for the second tenant.
- `status`: cumirror has a `source/cu` partition and the others do not; every projection is
  current. The source's number is 4096, the first of cumirror's range.
- `source add` again with the same name, or a name that is a provider slug (`wikidata`), refuses.

Start the server for the later phases:

```sh
triplespace-server --key-file demofarm.key --mode development --dev-tenant control \
    --listen 127.0.0.1:8080 &
for t in control opencredit cumirror; do
  curl -s "http://$t.demofarm.test:8080/w/api.php?action=query&meta=siteinfo&siprop=general|triplespace|providers&format=json" \
    | jq -c '{t: .query.general.sitename, providers: .query.providers}'
done
```

Each answers with its own `sitename`. None lists `cu` among the instance's providers; cumirror
lists it among its sources (05 §6.1).

## Phase 2: adopt on `opencredit`

```sh
/usr/bin/time -l triplespace adopt --tenant opencredit --dump creditunionswiki-20261004.xml \
    --source https://creditunions.miraheze.org/ --version creditunionswiki-20261004 --frozen \
    --counters item=5095,property=12 2> adopt.stderr | tee adopt.out
```

`wb_id_counters` is not readable without shell access on Miraheze; 5,095 and 12 are the dump's
highest IDs. If items have been created on the wiki since the dump, use the live highest Q
instead (`list=allpages&apnamespace=860&apdir=descending` sorts by title, so check
`Special:NewPages` or the fetch in phase 4).

**Checks** (as `real-dumps.md` §0.2 and §1.3, with this dump's numbers):

```sh
psql "$TRIPLESPACE_DATABASE_URL" <<'SQL'
SELECT type, count(*) FROM view.entity WHERE tenant='opencredit' GROUP BY type;   -- item 5070, property 12
SELECT count(*) FROM view.term WHERE tenant='opencredit';
SELECT actor_key, name FROM view.actor WHERE tenant='opencredit' ORDER BY actor_key; -- 1 MediaWiki default, 2 Harej, 12 Harej bot
SELECT sequencename, last_value FROM pg_sequences
  WHERE schemaname='log' AND sequencename LIKE 'opencredit.%';  -- item ≥5095, property ≥12, page ≥5083, revision ≥5088, user ≥12
SELECT job_id, status, counts FROM view.job WHERE tenant='opencredit';
SELECT count(*) FROM view.entity WHERE tenant IN ('control','cumirror');          -- 0
SQL
grep '^skipping page' adopt.stderr | cut -d: -f2- | sort | uniq -c
```

**Pass:**
- adopted 5,082 entities, 0 rejected, 0 unreadable; Main Page adopted as a wikitext page with
  all six revisions, attributed to MediaWiki default and Harej; no page skipped.
- Every page ID equals the dump's; `local_revid` is above the floor; the source revision is
  kept as `source_revid`.
- Re-running the same command: adopted 0, unchanged 5,082, fast.
- Negative tests from `real-dumps.md` §0.2: without `--frozen` it refuses; one entity edited in a
  copy of the dump gives exactly one reject and leaves the stored entity unchanged.
- **Unit IRIs:** `wbgetentities&ids=Q8` on opencredit shows `P6`/`P7` units as
  `https://creditunions.miraheze.org/entity/Q4`/`Q6`, not the tenant's own concept base. That is
  G21, unapplied; record it. Decide whether the adopted tenant's concept base should be the
  source's (0035) before G21 lands.

**API and clients**, against `http://opencredit.demofarm.test:8080`:

```sh
O=http://opencredit.demofarm.test:8080
triplespace subsidiary create --tenant opencredit --name "Harej bot 2" --operator Harej --group bot
triplespace subsidiary key --tenant opencredit --name "Harej bot 2" --label test \
    --grant editentity --grant highvolume --json > oc-key.json
SECRET=$(jq -r .secret oc-key.json)

tools/api_check.py --api $O --tenant opencredit --db "$TRIPLESPACE_DATABASE_URL" --sample 20 \
    --ids Q1 Q4 Q6 Q7 Q8 Q5095 P4 P6 P12 --compare https://creditunions.miraheze.org/
tools/api_check.py --api $O --tenant opencredit --ids Q8 \
    --wbi-login "Harej bot 2@test" --wbi-secret "$SECRET" --wbi-create-property
tools/pwb_check.py --api $O --tenant opencredit --login "Harej bot 2@test" --secret "$SECRET" \
    --item Q8 --item-floor 5095
tools/wdi_check.py --api $O --login "Harej bot 2@test" --secret "$SECRET" --item Q8 --item-floor 5095
```

- `--compare` needs network access to Miraheze. Expect content equal for the sample, `pageid`
  equal, `lastrevid` above the source's. The comparison passes on units only because they are not
  rewritten (G21).
- New items take Q5096 and up, never a reused number; a new property P13 and up.
- `wbsearchentities&search=TEXARKANA` finds Q8 ("MORRIS SHEPPARD TEXARKANA (TX)").

**Site, discussions and watchlist** (built on `main` since 2026-10-25): log in as Harej with
`clientlogin`; open `/wiki/Item:Q8` (statement groups, quantities with units, references to Q7),
`/wiki/Main_Page` and its History (six revisions, diffs); start a thread on `Item talk:Q8`, watch
Q8, reply from a second account that mentions Harej, and check the bell and
`Special:Watchlist`. Then run `tools/acl_check.sh` once on the branch to confirm nothing in P1
regressed the permission model.

**Rebuild equivalence** as in `real-dumps.md` §0.2. Record `rebuild`'s wall time.

## Phase 3: `cumirror` syncs the dump into its source

```sh
/usr/bin/time -l triplespace sync --tenant cumirror --source cu --dump creditunionswiki-20261004.xml \
    --version 20261004 --rejects cu-rejects.tsv 2> sync1.stderr | tee sync1.out
```

**Checks:**

```sh
psql "$TRIPLESPACE_DATABASE_URL" <<'SQL'
SELECT tenant, type, count(*) FROM view.entity WHERE id LIKE 'cu:%' GROUP BY 1, 2;  -- only cumirror: item 5070, property 12
SELECT entity_id, upstream_version FROM view.entity_source
  WHERE tenant='cumirror' AND graph='source/cu' ORDER BY entity_id LIMIT 5;
SQL
C=http://cumirror.demofarm.test:8080/w/api.php
curl -s "$C?action=wbgetentities&ids=cu:Q8|cu:P6&format=json" | jq .
```

**Pass:**
- created 5,082, rejected 0, unreadable 0; Main Page is skipped as a non-entity.
- Every snak carries `datatype`, typed from the dump's own properties (the XML has none).
- IDs are rewritten everywhere: `cu:Q8`'s `cu:P4` value is `cu:Q2`, its references cite `cu:P12`
  → `cu:Q7`, statement GUIDs carry the `cu:Q8$` prefix, `snaks-order` names `cu:P…`.
- **Units:** record what `cu:Q8`'s `cu:P6` unit is. Today it will still be
  `https://creditunions.miraheze.org/entity/Q4`; G21 wants `cu:Q4`.
- Revision IDs are `4096 << 40 | upstream revid`. No upstream actor is recorded (a source has no
  issuer); the job's actor is cumirror's operator.
- Rows are cumirror's only: no shared row, nothing in a provider index (05 §6.4).
- `/wiki/Item:cu:Q8` renders with the source chip ("Credit Unions Wiki");
  `wbsearchentities&search=TEXARKANA` on cumirror finds `cu:Q8`.
- **Re-sync** the same command: created 0, updated 0, unchanged 5,082 (the cursor).
- **Snapshot on the same dump** (`--snapshot --threshold 0`): swept 0, nothing tombstoned.
- Rebuild equivalence passes with both the adopted tenant and the source in the database.

**Local statements on source entities** (05 §2.5):

1. On cumirror, create a local string property "local note" (it becomes cumirror's `P1`; note
   that `P1`, `cu:P1` and opencredit's `P1` are three different properties).
2. Add `P1 = "overlay test"` to `cu:Q8`, and an English description to `cu:Q100`.
3. Read `cu:Q8` and `cu:Q100` on cumirror: the union, with the local description winning.
4. `source/cu` has no new record (the edit went to cumirror's `local`).
5. `wbsearchentities` on cumirror returns one document for `cu:Q100`, with the local description.

## Phase 4: `cumirror` syncs from the live wiki

**Upstream edits.** As Harej on `creditunions.miraheze.org` (keep a list with times and revision
IDs):

| # | Edit on Miraheze | Expected in the next sync |
|---|---|---|
| U1 | Change Q8's English label | `cu:Q8` updated; cumirror's `P1` statement survives |
| U2 | Change Q100's P6 (member count) | `cu:Q100` updated; cumirror's description still wins |
| U3 | Create a new credit union item | created, `cu:Q5096` or above |
| U4 | Delete Q5095 | snapshot: tombstoned; upsert: unchanged (upsert never deletes) |
| U5 | Merge Q101 into Q102 (`Special:MergeItems`) | `cu:Q101` a redirect to `cu:Q102`; `cu:Q102` updated |
| U6 | Create P13 with datatype `url` or `external-id`, use it on Q8 | `cu:P13` created; an `external-id` lands in `view.identifier` |
| U7 | Edit Main Page | skipped |

**Incremental, then full:**

```sh
tools/wikibase_fetch.py --api https://creditunions.miraheze.org/w/api.php \
    --since 2026-10-04T00:00:00Z --out cu-live-inc.xml.gz
triplespace sync --tenant cumirror --source cu --dump cu-live-inc.xml.gz --version live-inc-$(date +%Y%m%d)

tools/wikibase_fetch.py --api https://creditunions.miraheze.org/w/api.php --out cu-live-full.xml.gz
triplespace sync --tenant cumirror --source cu --dump cu-live-full.xml.gz --version live-$(date +%Y%m%d) \
    --snapshot --threshold 0     # must fail: one deletion over a threshold of 0
triplespace sync --tenant cumirror --source cu --dump cu-live-full.xml.gz --version live-$(date +%Y%m%d) \
    --snapshot --threshold 10
```

**Pass:**
- Incremental (upsert): created 2 (U3, U6), updated 3 (U1, U2, U5's target), redirected 1 (U5),
  everything not in the fetch untouched. Its `--since` fetch must not carry Q5095 (deleted).
- The `--threshold 0` snapshot fails the job and sweeps nothing.
- The full snapshot: created 0, updated 0, tombstoned 1 (`cu:Q5095`), unchanged the rest. Then
  the same command again: everything unchanged.
- `cu:Q101` resolves to `cu:Q102` through `wbgetentities` (`redirects`); if mirror redirects are
  not followed on read yet (the backlog's "following mirror redirects on read"), record it.
- cumirror's local statements survive both syncs; `cu:Q8`'s label is the new upstream one, its
  `P1` is still there.
- `opencredit` is unaffected: Q8 keeps its 2026-10-04 label (it is a fork).
- **Activity:** `GET /activity?source=source/cu` on cumirror lists the sync's changes; a watch on
  `cu:Q8` from phase 3 shows it as unseen (entity `watch` notifications for syncs are not built;
  record whether the watchlist row appears).
- If the 0078 fetch job was built instead of P3, the same edits arrive through it, and the
  expectations are the same except that an upsert never tombstones U4 until the daily sweep.
- Rebuild equivalence passes. Record wall times; at this size, setup cost dominates.

## Phase 5: cross-cutting probes

**Isolation matrix.** Fill in for each tenant (`control`, `opencredit`, `cumirror`):

| Probe | control | opencredit | cumirror |
|---|---|---|---|
| `wbgetentities` Q8 | missing | adopted Q8 | missing |
| `wbgetentities` cu:Q8 | invalid: `cu` names nothing here | invalid | the source entity, with cumirror's local statements |
| `wbsearchentities` TEXARKANA | none | Q8 | `cu:Q8` |
| `list=allusers` Harej | absent | present | absent |
| `list=recentchanges` | its own | adoption job | source syncs and local edits |
| `/wiki/Main_Page` | its own | adopted, six revisions | its own |

**Clustering.** `opencredit:Q8` and `cu:Q8` are the same credit union, but nothing links them:
P1 (charter number) is a `string`, not an `external-id`, and clusters (0004) are not built.
Record it as the motivating case for a tier-2 link on a charter number, not as a bug.

## Results log

One row per run.

| Run | Input (sha256, entities) | Wall | Peak RSS | DB size after | Created / adopted | Rejected | Unreadable | Rebuild identical? | Notes |
|---|---|---|---|---|---|---|---|---|---|
| 1 farm | | | | | | | | | |
| 2 adopt opencredit | `creditunionswiki-20261004.xml`, 5,082 entities / 5,083 pages | | | | | | | | |
| 2 api_check, clients | | | | | | | | | |
| 0 sandbox: sync under a scratch `CU` provider | `creditunionswiki-20261004.xml.gz`, 5,082 entities | 21.5 s | | | 5,082 | 0 | 0 | | 2026-10-26, release build, cloud sandbox: re-sync unchanged 5,082 (4.9 s); snapshot swept 0. The adapter path only, before any source code |
| 3 sync dump into `cu` | same | | | | | | | | |
| 3 re-sync, snapshot | same | | | | | | | | |
| 4 incremental | | | | | | | | | |
| 4 full snapshot | | | | | | | | | |
