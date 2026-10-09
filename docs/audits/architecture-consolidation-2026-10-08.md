# Architecture consolidation audit, 2026-10-08 (waves A–D)

Findings from writing chapters [01](../architecture/01-log-and-records.md)–[04](../architecture/04-entities-and-identifiers.md) (wave A) and [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [23](../architecture/23-configuration-and-registry.md) (wave B, the same day) of `docs/architecture/` from the ADRs ([PLAN](../architecture/PLAN.md) step 3). Each inconsistency is also marked in the chapter where it sits, as a `> **Inconsistency.**` note, and the note stays until an ADR settles it. Nothing here was fixed in the chapters or the ADRs: each row is a candidate for a direct decision logged under 0050 §6 and §8, or for a new ADR. "Resolution" is a proposal, not a decision.

The audit was made by Claude Fable from the ADR text as of 2026-10-08. **Decided 2026-10-08:** James accepted every proposed resolution (A1–A21, B1–B9). The chapters now state the resolved form, each place marked `> **Pending.**`, and the ADR changes are listed in [`../architecture/PENDING.md`](../architecture/PENDING.md) for one later pass. The six citation defects were fixed in the ADRs the same day; the other defects are in PENDING.md as T1–T3.

## Inconsistencies

### Chapter 01, Log and records

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 2.4 | 0005 §4.3, 0006 §1 vs 0015 §1 | 0005 §4.3 and 0006 §1 say the instance key is "the only signer"; 0015 §1 defines a client `signature` in the attestation made by the actor's own key, and 0006 §1's own table points at it. | Amend 0005 §4.3 and 0006 §1 to "the instance key is the only signer of checkpoints; an actor may additionally sign its own record (0015 §1)". |
| 6 | 0006 §4, §5 (A14) vs 0013 §3 | 0006 since A14: a compacted offset keeps its leaf, so a segment's tree and manifest are unchanged by compaction. 0013 §3: "The compaction worker re-signs the manifest of each segment it touched, listing the manifest it replaces." | Strike the re-signing sentence from 0013 §3; A14 should have carried through. Manifests are immutable once sealed. |
| 8 | 0006 §9 vs 0015 §1 | 0006 §9 lists a bundle's contents and says it "needs nothing else to verify"; 0015 §1 has `verify` check client signatures against `scatter:v0/key` records in the tenant `actors` partition, which 0006 §9 does not list. | Amend 0006 §9's bundle list to include the `key` records of the `actors` partition whenever an exported partition holds client-signed records. |

### Chapter 02, Graphs, RDF and the query service

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.2 | 0005 §4.1 vs 0015 §5 | 0005 §4.1 gives kind, integrity and export policy for eleven graphs; 0015 §5 adds `files/{repo}`, `derived/{source}`, `source/{name}`, `pages/{repo}` and the instance `log` with no policies in Decision text, only "see `graphs.toml`". | Carry the policies of every reserved graph name in one ADR table (0005 §4.1 or 0015 §5), so the catalogue is complete without reading the TOML. |
| 3.2 | 0001 §6, 0007 §9 vs 0001 §2, 0005 §5, 0011 §8, 0040 §7, 0049 §9, 0059 §2 | 0001 §6 and 0007 §9 list the terms minted under `scatter:`; later sections use `scatter:Revision`, `scatter:addedIn`, `scatter:logType`, `scatter:logAction`, the `as:Activity` subclasses, `scatter:authority`, `scatter:listedOn`, `scatter:subject`, `scatter:cursor`, none of which the list names. | State in 0001 §6 that the list is illustrative and `scatter-vocab` is the catalogue of record, or extend the list by amendment as terms are added. |
| 5.5 | 0026 §8 vs 0038 §11 | 0026 §8 keeps wikibase-compat §5.2's shape with the sitelink URL as the `schema:Article` subject "for every sitelink"; 0038 §11 makes the `schema:Article` node the page node `{base}/page/{id}` for a page paired with an item. | Amend 0026 §8 to state the paired-page exception and that the page node carries both types, with the URL via `schema:url`. |
| 5.9 | 0064 §8 vs 0066 §6, 0001 §3, §5 | 0064 §8 puts a schema's terms, typed `wikibase:EntitySchema`, on its entity node in the dump; 0066 §6 says entity schemas stay out of wikibase-compat's RDF; 0001 §3 adds no vocabulary to the main graph and 0001 §5 never mints under `wikibase:`. | Say in 0064 §8 which graph and dump the terms are in and whether `wikibase:EntitySchema` is an existing ontology term; if not, mint `scatter:EntitySchema` in the metadata graph or document the exception in wikibase-compat.md. |
| 6.2 | 0032 §2 vs 0013 §8 | 0032 §2 says each side of every delta, the metadata graph's included, is rendered by `scatter-wikibase-rdf` per wikibase-compat §4–5; 0013 §8 says the metadata graph is produced by `triplespace-rdf`; wikibase-compat.md defines no metadata vocabulary. | Amend 0032 §2: `scatter-wikibase-rdf` renders resolved and source-graph sides, `triplespace-rdf` renders metadata sides. |
| 7.2 | 0032 §1 vs 0059 §2–3 | 0032 §1's `full` subscription takes `metadata=local` (default) or `metadata=all`; 0059 §2–3 load and follow `full` for both backends without naming the modifier. | State in 0059 §2 which `metadata=` form the query service's store holds, and whether the embedded worker applies `all` since it reads `rdf_delta` directly. |

### Chapter 03, Storage, caches and search

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 4.1 | 0013 §5 vs 0052 §8, 0053 §8, 0069 §10, 0032 §3 | 0013 §5: every `view` table other than the entity tables is per tenant outright and carries `tenant` in its key. `foreign_title`, `foreign_page`, `foreign_thread` are instance scope with no `tenant`; `rdf_delta` has shared rows with `tenant = ''`. | Amend 0013 §5 to name a third class, instance-scope tables, and list them. |
| 4.5 | 0013 §5.4 vs 0023 §10 vs 0056 §14 | `view.acl` is `(target, restrictions jsonb, read_kind, extra jsonb, "offset")` PK `(target)` in 0013; `(target_kind, target_id, permission, parts, "group", expires, partition, "offset")` in 0023; 0056 adds `kind = confidential \| moderation` where 0013 already has `read_kind`. | Make 0013 §5.4's SQL (A27, the latest) authoritative; rewrite 0023 §10's column list and 0056 §14's row to match, with `kind` as `read_kind`. |
| 4.6 | 0013 §5.5 vs 0028 §12, 0013 §5.6 | §5.5's list of instance-level registry kinds omits `provider-readers`, which 0028 §12 and 0013 §5.6 both put there. | Add `provider-readers` to the 0013 §5.5 list. |
| 4.6 | 0056 §7, §14 vs 0013 §5 | 0056 puts `visibility_epoch` on `view.tenant`; 0013 §5 defines no `view.tenant` table (tenants are a `tenant` kind in `view.registry`). | Add `CREATE TABLE view.tenant` to 0013 §5.5, or move `visibility_epoch` into the `tenant` registry record's `config`. |
| 4.10 | 0047 §13 vs 0013 §5 | 0047's `report_entry`, `report_state`, `site_stats` declare `tenant bigint`; 0013 §5 defines `tenant text NOT NULL DEFAULT ''`. | Change 0047 §13 to `tenant text NOT NULL DEFAULT ''`. |
| 4.12 | 0053 §8, 0013 §5.6 vs 0014 §2, §4 | 0053: `foreign_page` in `proxy` mode indexes bundles in the L2 cache. 0014 §4 lists the bundle key `fp:` as an L1 (Valkey) key; L2 is the HTTP/web-tier layer. | Change 0053 §8 and the 0013 §5.6 row to L1. |
| 4.16 | 0022 §10 vs 0021 §8, 0013 §4, §5.6 | 0022 names the delivery queue `private.ap_outbox_queue` while calling it "the `ops` delivery queue"; everything else places the queue in `ops`. | Correct 0022 §10 to `ops`, naming the `ops` table once in 0021 §8. |
| 5 | 0013 §5.6 vs 0019 §11, 0049 §12, 0069 §10, 0060 §10, 0061 §11, 0065 §4, 0058 §2, 0033 §4, 0014 §8; 0013 §5.4 vs 0047 §13 | 0013 §5.6 presents itself as the one map of the schema but omits `thread_attachment`, `foreign_thread`, `ops.upstream_post`, `scope`, `scope_member`, `sprint`, `task`, `file.mediainfo_id`, the 0058 `log`/`ops` tables and columns, `ops.migration`, `view.page_text`; the `view.page` SQL in §5.4 lacks 0047's four columns and `view.actor` lacks `last_active`. | Add the missing rows to 0013 §5.6 (chapter 03 §5 is a ready draft) and the 0047 columns to the §5.4 SQL. |
| 6.1 | 0013 §7 vs 0051 §5, 0055 §6, 0071 §12 | 0013 §7's step-2 list omits `redirect`, `page_prop` and `derivation`, each of which its own ADR and its 0013 §5.6 row place in step 2. | Add the three to step 2 of 0013 §7. |
| 9.2 | 0014 §2, §4 vs 0013 §4 vs 0025 §8 | 0014: sessions live in Valkey (`s:`). 0013 §4: `private` holds "sessions' secrets". 0025: without a shared cache, sessions are held in `private`. | State in 0014 §2 that session state is in Valkey when present and in `private` otherwise; clarify "sessions' secrets" in 0013 §4. |
| 9.4 | 0056 §7, 0014 §4 vs 0014 §10 and later trailers | The `{vis}` key list names `e:`, `t:`, `prov:`, `p:`, `d:`, `sug:`, `css:` (carry) and `s:`, `rl:`, `up:` (do not); `post:`, `th:`, `tp:`, `fp:`, `fi:`, `rf:`, `ld:`, `sc:`, `sp:` are in neither list though their values depend on the viewer. | Extend 0056 §7's list (or 0014 §10) to classify every key. |
| 9.6 | 0014 §5, §10 vs 0056 §7 | 0014: a `read` ACL that deletes or hides bumps `generation`. 0056: a `read` ACL written, changed or retired bumps the target's version. | Decide one, likely `generation` for moderation-kind ACLs and version for confidential-kind changes, and say so in both. |
| 11.6 | 0056 §8 vs 0014 §8, 0013 §5, 0056 §14 | 0056 §8: the Postgres search fallback applies the `read_groups` predicate in SQL. No fallback table (`term`, `page`, `page_text`) has a `read_groups` column; 0056 §14 adds one to `activity` only. | Add `read_groups` to `view.page` and `view.page_text`, or say the fallback filters after the scan using the read-time visibility set. |
| 12.1 | 0014 §10 vs 0060 §10, 0061 §11 | 0014 §10 claims to be the one list of L1 keys; `sc:` and `sp:` are not in it. | Add the two rows to 0014 §10. |
| 12.1 | 0014 §1 (principles 1, 4) vs 0049 §12, 0069 §10, 0060 §10, 0061 §11 | 0014: keys name versions, nothing is purged on an ordinary edit, only erasure purges. 0049/0069 purge `tp:` by tag on any thread record or upstream put; 0060 purges `sc:` on recomputation; 0061 purges `sp:` on a task state change. | Re-key `tp:`/`sc:`/`sp:` by a version so the purges are unnecessary, or amend 0014 §1 to allow tag purges for listing keys. |

### Chapter 04, Entities and identifiers

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 3.1 | 0009 §1 vs 0048 §6, `keyed-types.toml` | 0009 §1 enumerates a keyed-type registry entry's fields with no cluster field; 0048 §6 carries `clusters = false` over from 0036, and the registry file records `clusters = false`, `label` and `defined_in`. | Amend 0009 §1 to add `clusters` (default true) and `label`, so the field list matches the registry schema. |
| 3.5 | 0009 §6 vs 0004 §4, 0017 §1, §3 | 0009 §6 gives a Domain's document node as `…/Special:EntityData/{key}` (bare key); 0004 §4 gives every cluster member's document node as `…/Special:EntityData/{id}`; 0017 §3's list of 0009 sections that now read with the `domain:` prefix omits §6. | Amend 0009 §6 to `Special:EntityData/{id}` (`domain:en.wikipedia.org`). |
| 4.2 | 0004 §1, §2, §4 vs `providers.toml`, 0009 §8, 0078 §2 | 0004 names the namespaces as the local instance, Wikidata and OpenAlex, bounds a cluster at three members "today", and defaults the provider order to Wikidata then OpenAlex; the registry allocates eight providers, every keyed type and every entity source is a namespace. | Amend 0004 §2 to bound by "the namespaces the tenant reads" and 0004 §4 to define the default order (provider-number order in `providers.toml`). |
| 4.3 | 0004 §3 vs 0004 §4, §9 | §3: tier-3 links are instance-wide on a farm, but use the identifier properties "the tenant's `reconcile` record names"; §4 and §9 call provider order, link properties and normalizer overrides instance configuration. | Decide whether `reconcile` is instance or tenant configuration (0015 §3 and 0018 should agree); if tenant, tier-3 links cannot be instance-wide. |
| 5.1 | 0066 §4 vs 0004 §2, §10 | 0066 §4 merges two local lexemes by `same-as` plus conversion, the source becoming a cluster member; 0004 §2 allows at most one member per namespace and makes a same-namespace duplicate a `redirect`; 0004 §10 refuses cross-namespace merges on `Special:MergeItems`. | Amend 0004 §2 to allow a same-namespace lexeme merge as a cluster, or restate 0066 §4 as a `redirect` whose parts keep their IDs under the target's page. |

## Defects in the ADR text

Not contradictions, but things the checker does not catch and a reader trips on.

| Where | Defect | Fix |
|---|---|---|
| 0014 §10 | Literal placeholder "`{L} §6, §11`" where 0058 §6 and §11 are meant. | Replace with the citation. |
| 0033 §8 | Cites "0032 §8" for the local quad store as a consumer; that is 0032 §6, and §8 is "What this stream is not". | Correct the citation. |
| 0036 §6, 0048 §3 | Cite "0004 §6" for the tiers of links; the tiers are 0004 §3, §6 is Properties. | Correct the citations. |
| 0049 §12, 0069 §10 | Cite "Caches (0014 §7)"; §7 is the search section, the caches are §4 and §10. | Correct the citations. |
| 0032 §6 vs 0059 §2 | 0059 §2 adds a marker triple `<{base}/.well-known/query> scatter:cursor` that `sparql-sync` writes after each batch; 0032 §6 still describes the client's cursor as living only in its state file. Compatible, but 0032 §6 is no longer complete on its own. | Extend 0032 §6 by amendment. |
| 0001 §1, 0011 §8 | `{base}/record/{partition}/{offset}` is both a post revision node (0001 §1) and a local log event IRI (0011 §8). Not contradictory, since a record is one or the other, but neither says so. | A sentence in either. |
| 0015 §5 | Its registry-files table is mapped to chapter 02 alone; chapter 23's scope ("the registry files and what each holds") will want it too. | When 23 is written, one of the two becomes a link. |

## Placement decisions made while writing

These are choices the chapter map left open; recorded so that later chapters match them.

- 0013 §3 (erasure and compaction in Postgres) is mapped to 01 alone, so its SQL is in 01 §5.2 and §6; chapter 03 links rather than repeats.
- 0058 is split: stored-form semantics and the commitment check on read in 01 §2.7; fragments, dictionaries, sweeps in 03 §3.
- 0046 §4 (instance records in instance partitions) is in 01 §1.4; the primary tenant itself is left to 08.
- 0015 §4's `view.upstream_revision` DDL is in 01 §2.6; 03 §5 links to it.
- 0000 §3 is mapped to 00 but chapter 04 §1 draws on its identifier half with a citation, since the chapter needs an opening.
- 0033 §9.3 (identifier libraries) sits in 04 §3.10 beside the keyed-type normalizers it serves.
- 0028 §12's query-store paragraph is kept short in 03 §13 with a pointer to 02.
- Shared trailers (0040 §7, 0049 §9, 0064 §7, 0065 §4, 0074 §8, 0076 §6, 0052 §6) are split by sentence between the chapters named in MAP.md, each citing the whole section.

# Wave B

Chapters 07, 08, 09 and 23, written after the wave A decisions. Twenty inconsistencies, **decided by James the same day** (PENDING.md rows C1–C20; defects D1–D5): `createaccount` is MediaWiki's right for both self-registration and creating an account for another, default `universe`; jobs run under subsidiaries, `ts-runjob` defaults to `bot`; `store` isolation with a remote backend is refused only on a farm; "partition 0" is a designation and the ADRs will name the instance `config` partition instead; one `provider` actor record per provider; the dotted-key scope rule. The chapters say the decided form, marked `Pending`. The same conventions as above.

## Inconsistencies

### Chapter 07, Actors and accounts

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.5 | 0007 §4 vs 0007 §6 | §4 lists `provider` among the kinds an actor record may hold, pointing at §6; §6 says no actor records are kept for providers without individual actors. | Strike `provider` from 0007 §4's kind list (and the `view.actor.kind` comment in 0013 §5.4), or have §6 say a `provider` actor record is written per provider and what it holds. |
| 2.5 | 0007 §4 vs 0035 §5 | §4: the `hidden` status is set by an `actor` ACL restricted to `suppress`; 0035 §5 writes `hidden` into an adopted account's first actor record directly, with no ACL. | 0035 §5 to say adoption also writes the `actor` ACL for a source-hidden account, so the status has one origin. |
| 5.3 | 0024 §4 vs 0075 §5 | 0024 §4's grant table has `basic` (always included, covers `read`); 0075 §5 requires "the `read` grant". | 0075 §5 to say `basic`. |
| 6.3 | 0024 §5 vs 0014 §4 | Rate-limit counters are `rl:{class}:{actor key}` in 0024 §5 and `rl:{scope}:{key}` in 0014 §4. | One spelling in both: `rl:{class}:{key}`, key being the actor key or, anonymously, the IP. |
| 8.3 | 0007 §3 vs 0027 §2 | 0007 §3 puts the password hash in `private.password`; 0027 §2 says every `private` table carries a portability class but gives this one none. | Add `private.password` to 0027 §2's "re-established, not carried" row. |

### Chapter 08, Tenants and instances

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 2.1 | 0018 §2 vs 0018 §10, 0078 §4 | 0018 §2's table gives a tenant six partitions and counts "six list partitions per tenant"; 0018 §10 and 0078 §4 add `source/{name}` per entity source. | Add a `source/{name}` row (and the farm partitions of 0028 §2 to the instance row) and drop the fixed count. |
| 2.2 | 0018 §2 vs 0015 §3 | 0018 §2 makes every partition ID 64 random bits, yet calls the instance `config` "partition 0", as 0015 §3 does. | Say the instance `config` partition has a random ID like every other and is found by its registry name, offset 0 holding the first `key:` record; or reserve ID 0 for it and say so. |
| 6.6 | 0040 §6 vs 0058 §8 | 0040 §6: tenant deletion writes `erase` records over every partition; 0058 §8 has the partitions "dropped" and cites 0039 §9 (file reclamation). | Fix 0058 §8's citation and say whether deletion ends with the partitions dropped after the `erase` records. |
| 9.1 | 0059 §4 vs 0028 §1 | 0059 §4: the `isolated` preset sets `query.isolation = store`, which the `remote` backend refuses; 0028 §1 makes every single-tenant instance `isolated` and has no `query.isolation` row. | Make `query.isolation` a row of 0028 §1's table with `isolated = store` and `remote` overriding to `dataset`, or leave the preset at the default. |

### Chapter 09, Security and moderation

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.1 | 0016 §1 vs 0016 §2 and later | 0016 §1's table names Scatterbase permissions `view`, `view_deleted`, `block_user`; everything else uses the MediaWiki names, and nothing uses the Scatterbase ones. | Say in 0016 §1 that the MediaWiki names are the permission names and the mapping is historical, or drop the column. |
| 2.2 | 0016 §2 vs 0023 §11, 0030 §5 | 0016 §2's table of later permissions lacks `browsearchive` (0023 §11), `managechangetags` and `changetags` (0030 §5). | Add the three (and check `groups.toml`). |
| 2.2 | 0024 §11 vs 0056 §3, 0025 §10 | 0024 §11 gives `createaccount` to `autoconfirmed`, for creating a subsidiary; 0056 §3 assumes `universe` holds it and that it governs self-registration. | Decide what `createaccount` governs: self-registration (`universe` by default, 0024 §11 wrong) or subsidiaries only (0056 §3 names another switch). |
| 2.2 | 0039 §21 vs 0054 §8 | `upload` defaults to `autoconfirmed` in 0039 §21; 0054 §8's "Copying files" row defaults to `user`. | Correct 0054 §8 to `autoconfirmed`, or say copying a fork's files does not need `upload`. |
| 2.2 | 0016 §2 vs 0046 §8 | `ts-runjob` is `bot`, `sysop` in 0016 §2; `bot` alone for instance jobs in 0046 §8. | Say in 0046 §8 whether the primary tenant's `sysop` may submit an instance job (0047 §12 gave `sysop` a `job` rate row, suggesting yes). |
| 4.6 | 0016 §4 vs 0016 §3, 0028 §3 | 0016 §4's default `actors` graph ACL speaks of "`rights` records"; the payload type is `scatter:v0/membership`. | Change 0016 §4 to "`membership` and `block` records". |
| 5.2 | 0016 §2, §5 vs 0023 §1, §11 | 0016 says `deletedhistory`/`deletedtext` govern seeing hidden things; 0023 says the ACL's group decides and these rights are only reported to clients, gating nothing. | Reword 0016 §2's row and §5's answer to 0023 §11's form. |
| 5.7 | 0012 §8 vs 0023 §5, 0014 §5 | 0012 §8: "only erasure purges" public caches; 0023 §5 and 0014 §5 purge on deletion or hiding by `read` ACL. | Amend 0012 §8 to "erasure, and deletion or hiding by `read` ACL". |

### Chapter 23, Configuration and registry

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.3 | 0015 §3 (paragraph) vs 0015 §3 (table) vs 0013 §5.5 | 0015 §3's scope paragraph names the instance kinds and gives "the rest" to tenants, but its own table marks `reports` instance scope and `page-repo` tenant-or-instance; 0013 §5.5's list omits `forwarder` and `reports`. | Make 0015 §3's paragraph the single list, with `reports`, `forwarder` and `page-repo` added, and have 0013 §5.5 reference it. |
| 2.2 | 0039 §11 vs 0015 §3 | 0039 §11 defines a `file-repo` config kind that 0015 §3's table never gained. | Add the row, logged as a 0039 amendment of 0015 §3. |
| 3.2 | 0039 §11 vs 0042 §11, 0053 §4 | `repo.cache_ttl` defaults to 7 days (files) and one hour (template source); 0053 §4 uses it for page bundles with no default. | Split the key per repository kind, or make it a per-repository field with a per-kind default in 0052 §1. |

## Defects in the ADR text (wave B)

| Where | Defect | Fix |
|---|---|---|
| 0040 §2 | Says `instance` is reserved "(0018 §1)"; 0018 §1 does not say so (0046 §6 restates the reservation citing 0040 §2). | Reserve it in 0018 §1 or cite 0040 §2 as the source. |
| 0023 §11 | The `protect` row still says `edit` and `move` restrictions only; 0056 §4 added the confidential `read` restriction to 0016 §2 and 0023 §1 but not here. | One-word fix. |
| 0016 §2 | The list of instance rights omits `ts-viewoperator`, which 0040 §9 adds under "Instance rights". | Add it. |
| 0015 §5 vs `registry/README.md` | 0015 §5's registry-files table (chapter 02 §1.4) lists 17 files; the README lists 21 (`content-models.toml`, `notation-schemes.toml`, `themes.toml`, `fragments.toml` missing). | Add the four rows to 0015 §5 (and chapter 02 §1.4). |
| 0015 §3 | Many dotted settings (`scopes.max_depth`, `sprints.max_claims`, `query.timeout`, …) are named without a scope; chapter 23 catalogues them as "not stated". | A one-line rule in 0015 §3: a dotted key named without a scope is a tenant `site` setting. |
| 0030 §2, §5, §12; 0007 §3 | "The right the abuse-handling store already requires" for seeing an IP is never named. | Name it (0007 §3), so the permission catalogue can list it. |
| 0059 §2 | Does not say whether `query.backend`, `query.path`, `query.endpoint` are `site` records or deployment configuration. | Say which. |

## Placement decisions (wave B)

- The rate-class catalogue is 07 §6.2; the permission catalogue is 09 §2.2; the config-kind catalogue is 23 §2.2 and the settings catalogue 23 §3 (built from every ADR, not only 23's sources).
- 0042 §16's expansion limits, `proposals.max_items` and `mcp.max_results` sit in 07 §6.4 as "limits that are not rates".
- 0046 §6 (farm slug) is in 08 §1.2 rather than with the primary tenant, since farm identity needs it first; 0028 §6–7 are in 08 §10 with the other per-feature trailers.
- 0016 §5 (a change-tracking table) is kept as 09 §2.4 with the question column dropped.
- 0033 §9.4 (`cel`) and 0034 §11 (CSP) sit in 09 §7.3 and §1.4 beside what they serve.
- `query.backend/path/endpoint` are under deployment configuration in 23 §3.6, with a sentence that the ADR does not say.

# Wave C

Chapters 05, 06, 10, 11, 12, 16 and 18, written after the wave A and B decisions. Forty inconsistencies, **decided by James the same day** (PENDING.md rows E1–E40): among them, the newer 0036 rule for OSM deletions stands as a provider opt-in; a no-redirect move is `move/move` with `noredirect`; the tenant list rides on `GET /tenancy`; scopes are addressed by page ID everywhere. The registry README's allocation paragraph was fixed directly. The chapters say the decided form, marked `Pending`. The same conventions as above.

## Inconsistencies


### Chapter 05, Providers and ingest

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 2.1 | 0002 §5 vs 0002 §6 (and 23 §3.2) | 0002 §5: "Each instance sets a default" retention policy. 0002 §6: the related `retention.auto_properties` is tenant `site` configuration; chapter 23 lists it as a tenant `site` setting. | Make the retention default a tenant `site` setting (`retention.default`, say) and amend 0002 §5 to say "each tenant"; consistent with D3. |
| 2.6 | 0012 §2.2 vs `providers.toml` | 0012 §2.2: `delta` is stored only when "the provider's registry entry sets `sync_deltas: full`", default `summary`. `providers.toml` has no `sync_deltas` field on any provider. | Add `sync_deltas` (default `summary`) to the provider entry schema and `providers.toml` header, or move it to the provider's adapter configuration and amend 0012 §2.2. |
| 3.2 | 0002 §8.2 vs 0035 §3 | 0002 §8.2: re-running `adopt` "skips what is present". 0035 §3: an entity already present is skipped when the content hash matches and rejected otherwise. | Keep 0035 §3's rule (skip on matching hash, reject otherwise) and amend the 0002 §8.2 paragraph to match. |
| 3.2 | 0002 §8.2, 0035 §3 vs 0002 §8.2 (`create-or-add` row) | `adopt` is "the one whole-entity write to the local graph"; but `create-or-add` with `overwrite` and an explicit base revision "replaces the matched entity's state", and 0002 §8.2 allows replacing a local entity wholesale with an explicit base revision. | Reword to "the one whole-entity write that needs no base revision" (or "the one first-write of a whole entity") in 0002 §8.2 and 0035 §3. |
| 3.9 | 0002 §8.7 vs 0002 §8.2, §8.5 | The local bulk job sketch shows `{"op":"create","ref":"$w1","match":{…}}`. §8.2 and §8.5 say `create` takes no match key and always mints; an operation with a match key is `create-or-add`. | Change the sketch's op to `create-or-add` (the sketch predates A15/A19). |
| 5.5 | 0002 §8.3 vs 0070 §3 | 0002 §8.3: every ingest job's actor is a subsidiary account so the operator is known. 0070 §3: the shallow-mirror fetch is an instance job run by the instance (0040 §6), with no subsidiary named. | State in 0070 §3 that the fetch job's actor is the instance (0040 §2's instance actor) and amend 0002 §8.3 to except instance jobs; or give the fetch job a subsidiary of the primary tenant's operators. |
| 7.3 | 0035 §3 vs `providers.toml` / 0066 | 0035 §3: `L` may be adopted "once the Lexeme namespace is implemented". `providers.toml` marks the Lexeme namespace "implemented by 0066"; chapter 04 §5.1 describes lexemes. | Strike the qualifier in 0035 §3; `L` is adoptable. |
| 8.1 | 0002 §4 vs `providers.toml`; 0037 §1 vs `providers.toml` | 0002 §4: the registry records "for each type code … the adapter that imports the type". `providers.toml`: `adapter` is one field per provider. 0037 §1: GDELT is registered with `actor_model = "provider-only"`. `providers.toml`: no `actor_model` field; a provider without individual actors has an `issuer` that is the provider itself (0007 §6). | Amend 0002 §4 to "for each provider" for the adapter, and amend 0037 §1 to say what the TOML does (`issuer = "gdelt"`), or add `actor_model` to the TOML schema. Note that OS also has `issuer == slug` yet has individual actors (changeset authors), so `issuer == slug` cannot be the test for "provider-only"; an explicit field would be clearer. |
| 8.7 | 0036 §2 vs 0002 §5, §8.2, 0009 §9 | 0036 §2: deletion of an OSM node/way/relation "clears the mirror's contribution as in 0009 §9" (a `put` of an empty state). 0009 §9 reserves that for keyed entities; map objects are ordinary foreign items, for which 0002 §5/§8.2 say upstream deletion is a `tombstone` that applies the retention policy. | Decide whether OSM objects are tombstoned (then fix 0036 §2's citation and wording) or whether 0036 intends the keyed rule for them because deleted OSM IDs are never reused (then amend 0002 §5 to allow a provider to opt into the clear-not-tombstone rule). |

### Chapter 06, Statements and properties

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 2.1 | 0031 §1 (internal) | The role table assigns Q21510856 to both *item requires statement* and *required qualifier*, and P2305 to both *item of property constraint* and *allowed values*. | Correct the table against Wikidata: *item requires statement* is Q21503247; *allowed values* has no property of its own on Wikidata (P2305 carries them), so drop that parameter row or name it as P2305's second use. |
| 5.2 | 0038 §1 (internal, A9 vs the unamended sentence); 0065 §1 | 0038 §1 says a File page's change set carries a `terms` operation writing MediaInfo captions, then says terms and sitelinks on `M` IDs are refused with `not-supported` until page terms are settled (Q4). 0065 §1 says labels and descriptions on an `M` ID are accepted; aliases and sitelinks stay refused. | Amend 0038 §1's last sentence to "aliases and sitelinks on `M` IDs are refused with `not-supported`", as 0065 §1 already settles. |

### Chapter 10, Pages and content models

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.4 | 0008 §2 (allocation table, Project row) vs 0061 §2 | 0008 §2 gives Project (4) the default model "`wikitext`; `triplespace-sprint` on subpages". 0061 §2 says `triplespace-sprint` is only *allowed* on Project subpages, "the default stays `wikitext`", and a sprint is created by `Special:CreateSprint` or `changecontentmodel`. | Amend 0008 §2's Project row to "`wikitext`; `triplespace-sprint` allowed on subpages" (0061 is the later, more specific text). |
| 1.4 | `docs/registry/README.md` (Pending allocations) vs 0060 §2, 0008 §2 | README says "224–229 and 312–319 are free"; 0060 §2 and 0008 §2 allocate 312/313 to Scope. README's allocation list also stops at Board 310/311 and omits Scope, Query 124/125, Lexeme 146/147, EntitySchema 640/641 and Module 828/829. | Update the registry README's pending-allocation paragraph to list Scope 312/313 and say 314–319 are free; check `namespaces.toml` (not present in the upload) agrees. |
| 1.4 | 0029 §2 vs 0049 §2 | 0029 §2: a resolver's talk namespace is `reserved` and empty "as `Thread talk` is (0019 §3)". 0049 §2 made Thread talk (215) `virtual`, forwarding to 214. | Drop the comparison in 0029 §2; the rule itself (resolver talk is `reserved`, 0008 §2 rule 2, 0078 §5) is unchanged. |
| 2.3 | 0051 §1 vs 0060 §2, 0063 §2 | 0051 §1 lists the namespaces whose allowed models include `wikitext` (and so may hold redirects): main, User, Project, File, Category, Template, Module (/doc), Table (/doc). 0060 §2 and 0063 §2 later allow `wikitext` for `/doc` titles in Scope (312) and Query (124). | Amend 0051 §1's list to add Scope and Query `/doc` pages, or replace the list with a reference to the namespace catalogue. |
| 4.2 | 0041 §2 vs 0041 §3, 0064 §3, 0066 §3, 0063 §3 | 0041 §2 lists `wikibase-lexeme` and `EntitySchema` as "reserved now and not implemented". 0041 §3 (as amended), 0064 §3 and 0066 §3 implement both. 0063 §3 says `wikibase-query` "stays reserved" under 0041's rule, but 0041 §2's list does not name it. | Amend 0041 §2's reserved-and-unimplemented list to `css`, `javascript`, `unknown`, `wikibase-query`; note that `wikibase-lexeme` and `EntitySchema` were implemented by 0066 and 0064. |
| 4.4 | 0041 §3 ("Default in" column) vs 0042 §3, 0043 §2, 0045 §2, 0060 §2, 0063 §2 | 0041 §3 gives `wikitext` the namespaces 0, 2, 4, 6, 14 and allows `json` only in 0, 2, 4. 0042 §3 makes `wikitext` Template (10)'s model; 0043 §2 allows `wikitext` and `json` in Module (828); 0045, 0060, 0063 allow `wikitext` in 218, 312, 124 for `/doc`. | Amend 0041 §3's `wikitext` and `json` rows (add 10; add 828, 218, 312, 124 as allowed), or make the namespace catalogue the one place that says where a model is allowed and have 0041 §3 carry defaults only. |
| 4.5 | 0043 §3 vs 0055 §1 (and 0008 §5, 0041 §3 as amended) | 0043 §3 says 0008 §5 excluded `css`, `javascript` and `sanitized-css` and "the other three stay excluded". 0055 §1 narrows the exclusion to `css` and `javascript` and registers `sanitized-css`; 0008 §5 and 0041 §3 now say so. | Amend 0043 §3's last paragraph to "the other two stay excluded" with a pointer to 0055 §1. |

### Chapter 11, Rendering, templates and modules

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.2 | 0033 §9.1 vs 0042 §4, §5, §9 (and 0039) | 0033 §9.1 says `File:` and `Image:` links render as chips because the File namespace is "reserved, not implemented, 0008 §2"; 0042 §4 gives the expander file facts from 0039, §5 implements `filepath` and `broken-file-category`, §9 records file usage in `view.file_link`, all assuming files render. | Amend 0033 §9.1 (or mark it as amended by 0039) so the chip list no longer names `File:` and `Image:` links; the files-in-wikitext rule is 0039's. |
| 1.2 | 0033 §9.1 vs 0042 §5, §7 | 0033 §9.1 chips galleries and "other extension tags" and ignores behaviour switches; 0042 §5 marks `gallery`, `indicator`, `poem`, `syntaxhighlight`/`source` (as `<pre>`) and `__NOTOC__`, `__TOC__`, `__FORCETOC__`, `__HIDDENCAT__` `implemented`, citing 0008 §8 and 0038 §3, and §7 says only tags of status `chip` chip after expansion. Neither says whether the 0008 §8 subset itself widened or these are rendered only while expansion is on. | State in 0042 §5 (or 0033 §9.1) that the registry's `implemented` tags and switches are rendered by the built-in renderer regardless of `wikitext.expansion`, and strike the stale sentences of 0033 §9.1; or say they apply only with expansion on. |
| 1.3 | 0042 §3 vs 0055 §1 | 0042 §3's Template namespace table lists `wikitext` as the namespace's only model; 0055 §1 makes `sanitized-css` the default model of Template (10) for `.css` titles and amends 0008 §5 but not 0042 §3. | Add `sanitized-css` to 0042 §3's Models column (default by the `.css` suffix rule), as 0055 §1 did for 0008 §5. |
| 7.2 | 0042 §3 vs 0043 §2 | 0042 §3 says that while expansion is off Template talk (11) "is not registered"; 0043 §2 says 828 and 829 are "registered and reserved, as 0042 §3 does for Template", entered in `namespaces.toml` with `enabled_by`. | Reword 0042 §3 to "10 is `reserved` and 11 is registered but disabled" (both entered in `namespaces.toml` with `enabled_by`), matching 0043 §2. |
| 7.10 | 0043 §11 vs 0042 §10 | 0043 §11 says `render_state` counts unheld-entity misses; 0042 §10's `render_state` column list has no such column. | Either add a column (e.g. `unheld_misses`) to 0042 §10's `render_state`, or say in 0043 §11 that the count lives in `limit_report`. |

### Chapter 12, Files and media

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 8.4 | 0041 §7 vs 0065 §1, §4 (and 0038 §11) | 0041 §7: a File page's RDF "stays as 0038 §11 has it, with the page node as subject"; 0038 §11 defines only the page node `{base}/page/{page ID}`. 0065 §1 and §4: captions go on "the `M` node (0038 §11)", a node 0038 §11 never defines. | Amend 0065 §1 and §4 to say the caption triples (`rdfs:label`, `schema:description`) are emitted on the page node `{base}/page/{page ID}`, the same subject as the page's statements, and drop "`M` node"; or, if an `M`-named node is intended, define it in 0038 §11 and reconcile 0041 §7. |

### Chapter 16, Logs, feeds and notifications

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.2 | 0011 §2 vs 0011 §6.1, §6.3; 0025 §5; 0039 §10, §16 | 0011 §2's log-graph table lists two graphs, the local log and one provider log per provider; 0011 §6.1 and §6.3, 0025 §5 and 0039 §10/§16 write job records, OAuth consumer records, `blob` ACLs, expunge records and takedown events to an "instance `log`" the table does not list (`graphs.toml` carries it as `log/{farm}`). | Add an instance-log row to 0011 §2 (kind, written by, history, integrity, export), or have §2 refer to the graph catalogue of 0015 §5 / chapter 02 §1.2 for it (which A18 already extends). |
| 1.3 | 0011 §3 vs 0039 §10 and `graphs.toml` | 0011 §3 lists the payload types the log graphs admit as `logevent`, `upstream-revision`, `acl`, `filter`, `filter-hit` and `erase`; 0039 §10 adds `scatter:v0/expunge` in the instance `log`, and `graphs.toml` lists it for the `log` graph. | Amend 0011 §3's list to include `expunge` (instance log only), citing 0039 §10. |
| 2.3 | 0011 §6.1, §9 vs 0039 §16, 0018 §10, 0035 §5, 0051 §5 | 0011 §6.1 is the table of local log events and §9 adds "the new types in §6.1" to `list=logevents`; `takedown/expunge` (0039 §16), `reclaim/reclaim` (0018 §10, 0035 §5) and `move/move_redir` (0051 §5) are named only in those ADRs. `reclaim/reclaim` names no record it is projected from and no visibility. | Add the three rows to 0011 §6.1; for `reclaim/reclaim`, say it is a `logevent` written directly to the tenant `log` (no other record) and visible to everyone, or that it projects from the reclaimed account's binding record. |
| 2.3 | 0051 §5 (internal) | A `noredirect` move is logged as `move/move_redir` "where MediaWiki would so name it, or `move/move` with `noredirect` set", without choosing; MediaWiki's `move_redir` names a move over a redirect, not a move that leaves none. | Settle on `move/move` with `noredirect` in the parameters, and use `move/move_redir` only for a move onto an existing redirect (0051 §5's "moving over a redirect" case). |
| 2.3 | 0011 §7 vs 0023 §3 | 0011 §7 types a `protect/*` parameter as, per restriction, the level and the expiry, and whether protection cascades; 0023 §3 types it as, per restriction, the group and the expiry, and names no cascade. | Amend 0011 §7 to say group (an ACL restricts to a group, 0016 §4) and either drop cascading or say 0023 does not implement it; the upstream (mirrored) `protect/*` events keep MediaWiki's level, which the row could say. |
| 2.3 | 0011 §6.1 vs 0011 §8 | 0011 §6.1 types `thread/pin` and `thread/unpin` as `as:Add`/`as:Remove` with `as:target` the page's featured collection; 0011 §8's type table has no row for them, nor for `patrol/*`, `abusefilter/*`, `upload/*`, `takedown/*`, `key/*`, `tag/*`, `managetags/*`, `bot/*`, `block/*`, `rights/*` or `oauth/*`, although every log event is a `prov:Activity` there. | Add rows to 0011 §8 for every type in §6.1 (chapter 02 §5.2 would follow), or state that unlisted types are `prov:Activity` with `scatter:logType`/`scatter:logAction` only. |
| 5.2 | 0021 §2 and 0020 §3 vs 0060 §7 | 0021 §2's `watch` reason is "generalized to every watchable target", and 0020 §3 gives every `private.watch` row the `notify` column, `scope` and `rows` kinds included; 0060 §7 says a scope watch "is a watchlist filter, not a subscription to each member" and that 0021 is unchanged, without saying what `notify` on a `scope` or `rows` watch does. | Say in 0060 §7 (and 0020 §3) that `notify` is ignored, or refused, for `scope` and `rows` watches; or define that it delivers one bundled notification per feed window rather than one per member. |
| 5.2 | 0040 §7 vs 0021 §2 | 0040 §7 assumes a notification "that an act triggers on a guest (a rename, a block)" naming the instance operator; 0021 §2's `rights` reason covers a block, but no reason in the table addresses a renamed account. | Add a `rename` reason to 0021 §2 (the renamed account is addressed when an actor record changes its name, performer the renamer or the instance operator), or amend 0040 §7 to name only blocks and rights changes. |

### Chapter 18, API

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.2 | 0018 §11 vs every REST trailer | 0018 §11 names "the tenant list" as an instance-level route served at the farm base; no ADR gives it a path, and 0028 §11 (`/tenancy`, `/farm/users/{id}`) and 0046 §9 (`/tenancy` includes the slugs) do not list tenants. | Name the route in 0018 §11 (proposed `GET /tenants` at the farm base, the list `Special:Tenants` shows) or strike the parenthesis. |
| 2.3 | 0069 §9, 0012 §5 vs 0019 §9 | 0069 §9 says "`list=threads` accepts `thorigin`" and 0012 §5 repeats it; no ADR defines a `list=threads` module (0019 §9 reads threads through `GET /page/{id}/threads` and the ordinary page modules). | Either define `list=threads` (parameters, what it lists, which ADR owns it) or change 0069 §9 and 0012 §5 to say `origin=` on `GET /page/{id}/threads` only. |
| 3.2 | 0060 §8, 0062 §8 vs 0076 §6, 0012 §5 | The scope routes address a scope by page ID (`/scope/{pageid}`, `/members`, `/facets`, `/refresh`); the dataset routes address it by title under the same prefix (`/scope/{title}/dumps`, `/scope/{title}/dataset.jsonld`). | Make 0076 §2 and §6 take `{pageid}` like 0060 §8, or state that `/scope/` accepts either and how a title is distinguished from a numeric ID. |
| 3.2 | 0019 §9 vs 0012 §5 | 0019 §9's route table lists `GET /page/{id}/history` as "unchanged", assuming 0012 §5 defines it; 0012 §5 has only `GET /entity/{id}/history`. 0019 §7 is where the page route is actually defined. | Add `GET /page/{id}/history` to 0012 §5's page table (citing 0019 §7) and change 0019 §9's "unchanged" to a definition. |
| 3.2 | 0028 §11 vs 0016 §7 | 0028 §11 adds "the global group and block routes mirroring 0016 §7's" at the farm base; 0016 §7 defines no group or block REST routes, only `/acl/{kind}/{id}` and the Action API modules. | Either name the farm-base routes in 0028 §11 (or say they are `action=globalblock`, `list=globalblocks` and the global-group Action API modules only), or add group and block REST routes to 0016 §7. |
| 3.2 | 0016 §7 vs 0012 §5, 0023 §8 | 0016 §7 defines `GET` and `PUT /acl/{kind}/{id}`; 0012 §5 and 0023 §8 cite 0016 §7 for `GET`, `PUT` and `DELETE`. | Add `DELETE` to 0016 §7 (retiring an ACL), which 0023 §8 already assumes. |
| 3.2 | 0012 §5 (index of later routes) vs 0028 §11, 0047 §11, 0056 §13, 0046 §9, 0071 §13/0072 §4 | 0012 §5's index claims to say where every later ADR's routes are, but has no row for 0028 §11, 0047 §11, 0056 §13 or 0046 §9, and attributes `GET /extraction/{source}/unmapped` to 0071 §13, which does not list it (0072 §4 defines it). | Add the four rows to 0012 §5 and cite 0072 §4 in the Extraction row; or drop the index now that chapter 18 §3.2 is the catalogue. |
| 3.3 | 0029 §6 vs 0012 §5 | 0029 §6's `/resolve` order has a step for notation keys (0048 §2) before resolver-prefixed strings; 0012 §5's order has no such step and says a bare string that normalizes to a key of some keyed type is offered as a suggestion, not resolved, "since bare keys are not IDs" (0017 §1). Notation is a keyed type (0048), so the two rules give a notation key different outcomes. | Fold 0029 §6's order into 0012 §5 and say whether notation keys are the exception to the bare-key rule (0048 §2 presumably makes them so) or are suggestions like domain names. |

## Defects in the ADR text (wave C)

| Where | Defect | Fix |
|---|---|---|
| `registry/README.md` | The pending-allocation paragraph says 312–319 are free; 312/313 are Scope (`namespaces.toml` and 0060 §2 agree), and the allocation list omits Scope, Query, Lexeme, EntitySchema and Module. | Update the paragraph. |
| 0012 §5 | Its index of later ADRs' routes is incomplete and misattributes one route; chapter 18 §3.2 is now the catalogue. | Drop the index in favour of a link, or complete it. |
| 0011 §8 | The RDF type table covers a fraction of the log types in 0011 §6.1. | State the rule for unlisted types (`prov:Activity` with `scatter:logType`/`scatter:logAction`). |

## Placement decisions (wave C)

- The provider catalogue is 05 §8, built from `providers.toml` with a subsection per provider that has an ADR; MusicBrainz is linked to 04 §2.2.
- The namespace catalogue is 10 §1.4 and the content-model catalogue 10 §4.4; the parser-function registry table is 11 §2.2; the log-event catalogue is 16 §2.3; the Action API and REST catalogues are 18 §2.3 and §3.2.
- 0038 §6 (pages paired with items) sits in 06 §4.6 under sitelinks, which it amends.
- 0048 §4 (a property's scheme) is held by 04 §3.9; 06 links.
- 0061 §8 (sprint page and notifications) is in 16 §5.6; chapters 15 and 19 will link.
- 0012 §2.1–2.2 appear in 18 §1.3 only as the API-facing facts; 01 and 05 hold them.

# Wave D

Chapters 13, 14, 15, 17, 19, 20, 21, 22 and 00, written after the wave A–C decisions. Twenty-seven inconsistencies, **decided by James the same day** (PENDING.md rows F1–F29, the last two being `view.proposal` in the schema map and the missing `thread-statuses.toml`): a proposal's fields are page metadata of the thread page; the newer ADR wins in 13 §4.4 and 20 §5.5; a foreign page is hidden through the administrator's Delete; fan-out has its own `federation` rate class; publications run under a subsidiary; the entity page gets the "About this page" panel; the prototype is server-rendered Rust; unsupported ShEx saves as `unvalidatable`. The chapters say the decided form, marked `Pending`. Chapter 00 found none; its findings list four deliberately overloaded words (scope, domain, namespace, bundle) whose meanings the glossary separates.

## Inconsistencies


### Chapter 13, Mirrored pages

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.1 | 0052 §1 vs 0053 §4 (and 0042 §11, 0039 §11; PENDING C20) | 0052 §1's `page-repo` table names the field `cache_ttl` ("how long fetched source and bundles are held, default one hour"); 0053 §4's L1 row cites it as `cache_ttl` (0052 §1) while its L2 row uses `repo.cache_ttl`, the key 0042 §11 and 0039 §11 name and C20 makes a per-repository field. Nothing says the two names are one field. | Fold into C20: rename 0052 §1's row to `repo.cache_ttl` (or state that `cache_ttl` is the record field `repo.cache_ttl` refers to) and make 0053 §4 use one name in both rows. PENDING C20's text and chapter 23 §3.5 say "0052 §1 does not list the field"; it does, under the shorter name. |
| 2.9 | 0052 §5 vs 0053 §9 (with 0054 §9, 0023 §4) | 0052 §5: Move, Delete, Protect and Change content model are not offered on a foreign page. 0053 §9: hiding a foreign page is a `read` ACL on its ranged page ID, "as deleting a local page does"; and deleting a local page or fork is itself a `read` ACL (0054 §9, 0023 §4). | Say in 0052 §5 which control writes the hiding `read` ACL on a foreign page (a Hide action, or Delete after all), or in 0053 §9 that the hide is written through the ACL interface rather than the page's tabs. |
| 4.4 | 0068 §4 vs 0069 §7, §5 | 0068 §4 puts Push (performing the destination's edit as the person) in 0067 §6's later phase. 0069 §7 sends a `talk`-destination page proposal from a fork that follows by 0069 §6's new-section mechanism, which 0069 §5 builds now as the first use of the upstream client while saying `proposals.push` keeps its own switch. Neither says whether such a send is gated by `proposals.push` (off by default, refused until pushing is implemented) or by `talk.upstream_post`. | Amend 0069 §7 to say which switch gates the send of a `talk`-destination proposal through §6, and whether 0068 §4's "later phase" still applies to the `talk` destination on a following fork. |
| 6.4 | 0071 §4 vs 0071 §4 (steps 1 and 3) and 0073 §3 | 0071 §4 refuses at configuration a source whose mappings name an identifier property not designated for `match_key` (`ts-extraction-match-key`), but step 1 looks an undesignated property up in `view.identifier`, and 0073 §3 lets `lines.identifiers` list any identifier property with patterns, whose values become the subject's match keys and may reach step 3 (creation). | Either extend the refusal to `lines.identifiers` and `url_property` (so step 1's `view.identifier` branch is unreachable and can go), or say that an undesignated key may find but never create. |

### Chapter 14, Discussions

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.5 | 0019 §4 vs 0049 §6, 0069 §4 (and 0067 §3 silent) | 0019 §4's Comment row names `edit`, `rename`, `move`, `attach` and `detach` as the operations that carry a summary. 0049 §6 and 0069 §4 say `pin` and `unpin` carry a summary in the comment part too. 0067 §3 does not say whether `propose`, `submit` and `withdraw` carry one. | Amend 0019 §4's Comment row to list every operation that carries a summary (add `pin`, `unpin`, and decide for `propose`, `submit`, `withdraw`; `propose` and `submit` plausibly do, `withdraw` plausibly does), so the part table is the one place that says it. |
| 1.5 | 0019 §4 vs 0049 §6, 0069 §4 | 0019 §4: the `target` of `attach`, `detach`, `pin` and `unpin` is "a `page` target only". 0049 §6: "a `page` target only, for `attach` and `detach`". 0069 §4: a thread may be pinned on its home, and 0049 §5 lets the home be an `actor` target (a user talk page). | Correct 0019 §4's row to 0049 §6's wording: `page` only for `attach` and `detach`; `pin` and `unpin` name any attachment, `actor` included, so front matter can be pinned on a user talk page. |
| 5.3 | 0019 §4 vs 0067 §3 | 0019 §4: a `create` record's `target` is `{kind, id}`, the thread's home. 0067 §3: `propose`'s `target` is `{wiki, id}`, and "a `create` may carry `propose`'s fields directly". A `create` that opens a proposal thread therefore has two meanings for one field name and neither ADR separates them. | Rename the proposal fields on the record, e.g. nest them under one `proposal` map (`proposal: {kind, target, base, payload, omitted, flags}`) in both `propose` and `create`, so `create`'s `target` stays the home; amend 0067 §3 and add the row to 0019 §4's field table. |
| 5.4 | 0019 §6 vs 0067 §5 | 0019 §6: "Anyone who may post may set [a status]". 0067 §5: the `declined` state of a proposal is "set by a post with the `declined` status, by the proposer or an `edit` holder", with no refusal stated for anyone else. | Either 0067 §5 states a validation rule (a `declined` post on a proposal thread by anyone other than the proposer or an `edit` holder on the subject's talk page is refused, with an error name) and 0019 §6 notes the exception, or 0067 §5 drops the restriction and `declined` is a status like any other. |

### Chapter 15, Structured pages

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 15 | 0060 §4 (as extended by 0066 §5 / 0060 A4) vs 0060 §5 | §4 gives a scope the subject types `form` and `sense` beside entity and page; §5's `view.scope_member.kind` is commented `'entity' \ | 'page'` and says nothing about how a lexeme part is keyed | Amend 0060 §5: `kind` is `'entity' \ | 'page' \ | 'form' \ | 'sense'`, with `id` the part ID in stored form; mirror the same in 0061 §6's `subject_kind` and 0064 §5's `subject_kind` |
| 15 | 0065 §3 vs 0045 §4 | 0065 §3 refers to a table definition's `render` key and a `thumb` column kind (`field: "thumbnail"`, `width`); 0045 §4 defines no `render` key and lists only `label`/`description`/`aliases`, `statements` and `sitelink` as column fields | Amend 0045 §4 (logged as a 0065 amendment): add a `thumbnail` field with `width` to the columns table, and either define `render` or strike the word from 0065 §3 |
| 15 | 0061 §4 (rules table, internal) vs 0060 §4 | 0061 §4 lists `missing-statement`'s parameters as `property` and optional `value` but gives its subject type as "entity, or page with `subjects: "pages"`"; `subjects: "pages"` is defined by 0060 §4 on the `statement` scope kind, not on a rule | Amend 0061 §4: either add `subjects` as an optional rule parameter, or say the rule's subject type follows the sprint's scope (page members get page statements checked) and drop the key from the column |
| 15 | 0064 §4 (internal) | "Refused at save: `IMPORT`, `EXTERNAL`, semantic actions and annotations other than `rdfs:label`/`rdfs:comment`, and any prefix the tenant's dump does not define" is followed by "A schema that uses them is saved but marked `unvalidatable`" | Amend 0064 §4: "Not validated" rather than "Refused at save"; the save succeeds and the schema is marked `unvalidatable` (which is what the next sentence and 0064 §5's `NULL conforms` already assume) |

### Chapter 17, Federation and publication

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 17 | 0022 §7 vs 0024 §5 | 0022 §7 rate-limits ActivityPub fan-out (`Announce`, `Create`, `Update`, `Delete` to followers' inboxes) in the `notify` class; 0024 §5 defines `notify` as "outbound notification requests initiated by the actor: verification messages, handle registrations", 5 / 5 per hour, which does not describe a per-post fan-out and would throttle an active talk page to five deliveries an hour. | Either widen 0024 §5's `notify` row to name ActivityPub fan-out with a limit that fits it, or add a `federation` class for outbound deliveries and have 0022 §7 cite it; a server-initiated fan-out to N followers should probably not be counted against the posting actor at all. |
| 17 | 0067 §6 vs 0074 §4 (and §7) | 0067 §6: "The instance never holds a shared upstream account and never pushes under its own name"; every grant is a row of `private.upstream_grant` keyed by `actor_key`. 0074 §4: a publication writes with a destination bot password the instance keeps in `private.publication_credential`, "or an OAuth grant held for the publication", from an instance-run job (0040 §6), with no actor named for that grant. 0074 §7 says publications and proposals share only the upstream client, but 0067 §6 states its rule without an exception. | Amend 0067 §6 to scope the rule to proposals ("never pushes a proposal under its own name") and point at 0074 §4 as the publication case; in 0074 §4 say which actor a publication's OAuth grant belongs to (presumably a subsidiary the publication is run under, per C14), so it fits `upstream_grant (actor_key, …)`. |

### Chapter 19, Site UI

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.3 | 0010 §2 (tabs table) vs 0038 §7 (tabs table) | 0010 §2 gives a document page the tabs Read, Edit, Page data, History, Links here, Talk and a thread "the posts as a tree … with History"; 0038 §7 gives a document page Read, Edit, Page data, History, Links here (no Talk) and a thread "The thread, Page data, History". 0010 §2's own Page data bullet says the tab shows "a document page's or thread's own statements", so a thread has Page data by 0010's text but not by its table. | Make 0010 §2 the one tabs table: add Page data to the thread row, keep Talk on the document-page row; replace 0038 §7's table with a reference to 0010 §2. |
| 3.1 | 0060 §8, 0067 §7 vs 0010 §4, 0003 §6 | 0060 §8 ("In 3 scopes") and 0067 §7 ("2 proposals: …") put counts in the entity page's "About panel"; 0010 §4 defines "About this page" for document pages only, and 0003 §6 gives the entity page an item header and a "Where this comes from" panel, with no About panel. (0068 §6's "fork's About panel" is a page, so it is fine.) | Amend 0003 §6 or 0010 §4 to give the entity page an About panel (counts: scopes, proposals, page-statement link) alongside "Where this comes from", or re-point 0060 §8 and 0067 §7 at the item header. |
| 5.8 | 0003 §10, 0010 §13 vs 0034 §3, §13 | 0003 §10: a `ui/` prototype fetches `wbgetentities`/REST v1, renders the six shapes itself, and the classifier is a module of it that "a later Rust implementation" may reuse; 0010 §13: "the frontend extends the `ui/` prototype". 0034 §3: the shapes are rendered only on the server by `triplespace-ui`, the classifier is a pure Rust crate; 0034 §13: the prototype path is `triplespace-ui` rendering from `Special:EntityData`, and "nothing in the UI changes". | Amend 0003 §10 (fold into 0034 §13): the prototype is `triplespace-ui` + `scatter-wasm`; keep the fixture list and audit step; strike "later Rust implementation" and "lives in `ui/`"; 0010 §13 should cite 0034 §13 rather than "the `ui/` prototype". |

### Chapter 20, Web tier

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 5.1 | 0056 §10 line 6 vs 0057 §10 (example) and 0057 §4 | 0056 §10 line 6 requires the edge proxy to strip inbound `X-Forwarded-*` from clients before setting its own, so a client-forged entry never reaches the API. 0057 §10's worked example has the edge pass the client-forged `F` through to the API, and 0057 §4 has the web tier forward `X-Forwarded-For` "as received", appending. | Add one sentence to 0057 §10 saying the right-to-left reading is the API's defence when line 6, which is attested rather than enforced, does not hold, and that a conforming edge produces `C, E` rather than `F, C, E`; the example then illustrates defence in depth, not the expected shape. |
| 5.5 | 0057 §10 and 0056 §10 line 12 vs 0057 §1 and 0057 §5 | 0057 §10 and 0056 §10 line 12 say the web tier "holds no credential of the instance's beyond an optional forwarder key, reads no store of it". 0057 §1 lists the password of its cache's Valkey among its secrets, and 0057 §5 (chapter 03 §10.2) allows that Valkey to be "the instance's L1 Valkey or a separate one". A web tier whose cache names the instance's L1 Valkey holds one of the instance's credentials and reads one of its stores. | Either require a separate Valkey for the web tier's cache (strike "whether it is the instance's L1 Valkey" from 0057 §5), or qualify 0057 §10 and 0056 §10 line 12 with "beyond an optional forwarder key and, when `web.cache = valkey`, the password of that Valkey, which holds public responses only". The second is the smaller change and matches 0057 §1. |

### Chapter 21, Special pages

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.1 | 0047 §1 vs `special-pages.toml` | 0047 §1's field table lists `name`, `mediawiki_name`, `origin`, `status`, `scope`, `group`, `aliases`, `section_aliases`, `restricted`, `report`, `adr`, and its `origin` list names eleven extensions. The TOML also uses `listed` (unlisted pages), `requires` (added by 0047 A6 for `Special:Query`), a free-text `note`, and the origin `GlobalContributions` (which `version.toml` lists as an extension). | Amend 0047 §1: add `listed`, `requires` and `note` rows to the field table and `GlobalContributions` to the `origin` list (or say "an extension's name as `version.toml` lists it", as 0077 §8 does). |
| 2.4 | 0047 §4.4 vs `special-pages.toml` | The ADR names the report `RandomRootPage`; the registry's canonical name is `RandomRootpage` (MediaWiki's first English alias; `mediawiki_name = "Randomrootpage"`). | Correct 0047 §4.4 to `RandomRootpage`, the registry's name. Resolution is unaffected (aliases match case-insensitively), but check 5 of `check_adrs.py` should be case-sensitive on canonical names or the drift will recur. |
| 2.4 | 0051 §6 vs 0047 §4.4 and the TOML | 0051 §6 says each of `ListRedirects`, `BrokenRedirects`, `DoubleRedirects`, `RandomRedirect` "is a live projection (0047 §4.3)"; 0047 §4.4 (A3) and the TOML give all four the `index` backing. | Correct 0051 §6 to "index-backed, live" (0047 §4.3's `index` row); no rows in `view.report_entry` are kept for them. |

### Chapter 22, Crates and stack

| Chapter § | ADR sections in tension | What each says | Proposed resolution |
|---|---|---|---|
| 1.2 | 0005 §3 rule 7; 0033 §2; 0005 §2; 0055 §9 | Rule 7 names four crates that must build for `wasm32-unknown-unknown` and 0033 §2 has CI build "every rule-7 crate"; the crate table marks five more (`scatter-merge`, `scatter-extract`, `scatter-css`, `scatter-scope`, `scatter-tasks`) as building for `wasm32`, and 0055 §9 adds `scatter-css` to "the wasm list of 0034 §6", not to rule 7. | Make rule 7 the one wasm list (nine crates) and have 0034 §6 and `cargo xtask wasm` read it. |
| 1.2 | 0005 §3 rule 2; 0005 §2; 0072 §7 | Rule 2's pure list omits `scatter-files`, `scatter-extract` and `scatter-vocab`; the table marks `scatter-files` and `scatter-extract` "Pure", and 0072 §7 says `scatter-extract` is pure. | Add the three to rule 2 (or state that the table's "Pure" is the list and rule 2 names examples). |
| 2.1 | 0059 §8 vs 0005 §1, §2, §6 | 0059 §8 introduces `triplespace-query` as "New, layer 3"; 0005 §1's layer 3 is Ingest, and the table (and the `triplespace-` name rule of §6) place it in the surfaces layer. | Correct 0059 §8 to "layer 4" (surfaces). |
| 2.1 | 0056 §16, 0064 §9, 0057 §12, 0054 §12, 0055 §9, 0069 §12, 0067 §9, 0068 §7, 0037 §8, 0036 §7 vs 0005 §2 | Trailer rows the folded table lacks: the §6 include rule on `triplespace-scribunto` and `triplespace-render` and the extended `tenancy check` (0056); `#conformance` on `triplespace-render` (0064); the OAuth pages and purge form on `triplespace-ui` (0057); the API crates for 0054 §11, 0055 §5–6 and 0069 §9; 0069's `triplespace-ui` changes, "Propose" on `Special:Corrections` (0067) and the About panel actions (0068); `id_grammar` and the `gdelt-record`/`token` grammars on `scatter-providers` (0037); the `osm-tag` grammar on `scatter-normalize` (0036). | Add the missing clauses to the table rows in one A-series amendment of 0005 §2. |
| 4.1 | 0033 §1, §17; 0059 §2 vs 0005 §2 | 0033 §1 and 0059 §2 call the server "the `triplespace` binary"; 0033 §17 packages "the server binary and the web binary" and an image "with both binaries"; the table has three binary crates (`triplespace-server`, `triplespace-cli`, `triplespace-web`) and nothing says what the CLI's binary is called or where it ships. (The repository README calls the CLI `triplespace` and the server `triplespace-server`.) | Amend 0033 §1 to name `triplespace-server`, and §17 to package three binaries, naming the CLI binary (`triplespace`, per the README). |

## Also noted (wave D)

- `docs/registry/thread-statuses.toml` was reported missing by chapter 14's writer; it exists (the writers had a partial copy of the registry). F29 records that nothing was created.
- `view.proposal`, defined inline in 0067 §5, is absent from 0013 §5.6 and from chapter 03 §5's catalogue and the A9 row; add it to both.
- 0072 §5's mapping-run reports sit in chapter 21 §2.6 but are extraction machinery; 13 may be the better home.
- 0010 §13 is mapped wholly to 19 though two of its bullets belong to 03/16 and 05.
- The crate table lists three binary crates where 0033 §1/§17 and the README speak of one `triplespace` binary and two packages (22's note 5).

# Ledger

The rows below are the decisions as James made them, by ID; each became an Amendment-log entry in the ADR it names, and the entries cite these IDs as "(PENDING A1)" and so on (the ledger lived in `docs/architecture/PENDING.md` while the chapters were being written, and moved here when the ADR pass applied it on 2026-10-09). A row's "ADR and section" is where the entry is; the Decision column is what the entry's Summary says.

## Drift

| ID | ADR and section to change | Change | Decision |
|---|---|---|---|
| A1 | 0013 §3 | corrects | Strike "The compaction worker re-signs the manifest of each segment it touched, listing the manifest it replaces." A compacted offset keeps its leaf (0006 A14), so a sealed manifest never changes. |
| A2 | 0005 §4.3; 0006 §1 | amends | "The server is the only signer" becomes: the instance key is the only signer of checkpoints and manifests; an actor may additionally sign its own record with the client `signature` of 0015 §1. |
| A3 | 0006 §9 | extends | A bundle also carries the `scatter:v0/key` records (the `actors` partition, or the key records of every actor whose signatures appear) for each exported partition that holds client-signed records, so `verify` level 2 needs nothing outside the bundle. |
| A4 | 0023 §10; 0056 §14 | corrects | `view.acl` is as 0013 §5.4 (A27) defines it: `(target, restrictions jsonb, read_kind, extra jsonb, "offset")`, PK `(target)`. 0023 §10's column list is replaced by a reference to it; 0056 §14's `kind` is `read_kind`. |
| A5 | 0013 §5.6 (0028 row) | corrects | `provider-readers` is **tenant** scope, a record in the provider tenant's own `config` (0015 §3, 0028 §5); the 0028 row of 0013 §5.6 says so instead of listing it beside the instance-level `tenancy` and `template` kinds. (Corrected 2026-10-08: first written as "add to 0013 §5.5's instance-level list", which 0015 §3 and 0028 §5 contradict.) |
| A6 | 0047 §13 | corrects | `tenant bigint` → `tenant text NOT NULL DEFAULT ''` on `report_entry`, `report_state`, `site_stats`. |
| A7 | 0053 §8; 0013 §5.6 (0053 row) | corrects | In `proxy` mode `foreign_page` indexes the **L1** (Valkey) cache, where 0014 §4 keys the bundle `fp:`; not L2. |
| A8 | 0022 §10; 0021 §8 | corrects | The delivery queue is one `ops` table; 0021 §8 gives it a name (proposed `ops.delivery`, since 0021 never names it) and 0022 §10 uses that name in place of `private.ap_outbox_queue`. |
| A9 | 0013 §5.6; 0013 §5.4 | extends | Add rows for `view.thread_attachment`, `view.foreign_thread`, `ops.upstream_post`, `view.scope`, `view.scope_member`, `view.sprint`, `view.task`, `file.mediainfo_id`, the 0058 `log`/`ops` tables and columns, `ops.migration`, `view.page_text` (the catalogue in chapter 03 §5 is the draft). Add `len`, `latest_at`, `revisions`, `random` to the `view.page` SQL and `last_active` to `view.actor`. |
| A10 | 0013 §7 | corrects | Step 2 also names `redirect` (0051 §5), `page_prop` (0055 §6) and `derivation` (0071 §12). |
| A11 | 0014 §10 | extends | Add the `sc:` (0060 §10) and `sp:` (0061 §11) rows to the L1 key list. |
| A12 | 0014 §2; 0013 §4 | amends | Session state lives in Valkey when a shared cache is configured and in `private` otherwise (0025 §8); 0013 §4's "sessions' secrets" means that fallback. |
| A13 | 0013 §5 | extends | A third class of `view` table, **instance scope**, with no `tenant` column: `foreign_title`, `foreign_page`, `foreign_thread`; and `rdf_delta`'s shared rows carry `tenant = ''`. |
| A14 | 0009 §1 | extends | A keyed-type registry entry also records `clusters` (default true; false for keyword and notation) and `label`. |
| A15 | 0009 §6 | corrects | A Domain's document node is `{base}/wiki/Special:EntityData/{id}` with the prefixed ID (`domain:en.wikipedia.org`), per 0017 §1; 0017 §3's list of updated 0009 sections gains §6. |
| A16 | 0004 §1, §2, §4 | amends | A cluster has at most one member per namespace the tenant reads (registry providers, keyed types, entity sources), not "three today"; the default provider order is provider-number order in `providers.toml`, overridable by `reconcile`. |
| A17 | 0004 §4, §9 | corrects | `reconcile` is tenant configuration (0018 §3), not "an instance's configuration"; the inference properties are the exception, see B6. |
| A18 | 0015 §5 | extends | The reserved-graph table carries kind, integrity and export policy for every graph, including `files/{repo}`, `derived/{source}`, `source/{name}`, `pages/{repo}` and the instance `log`, so `graphs.toml` and the ADR agree without reading the TOML. |
| A19 | 0032 §2 | corrects | `scatter-wikibase-rdf` renders the resolved-view and source-graph sides of a delta; `triplespace-rdf` renders the metadata-graph sides (0013 §8). |
| A20 | 0026 §8 | amends | For a page paired with an item (0038 §6, §11) the `schema:Article` node is the page node `{base}/page/{id}`, also typed `schema:WebPage`, with the URL as `schema:url`; every other sitelink keeps wikibase-compat §5.2's shape. |
| A21 | 0001 §6; 0007 §9 | amends | The list of terms minted under `scatter:` is illustrative; `scatter-vocab` is the catalogue of record. |

## Choices

| ID | ADR and section to change | Change | Decision |
|---|---|---|---|
| B1 | 0013 §5.5; 0056 §7, §14 | extends | `CREATE TABLE view.tenant (tenant text PRIMARY KEY, visibility_epoch integer NOT NULL DEFAULT 0)`, a projection of the tenant's `read` ACL records; configuration stays in `view.registry`. |
| B2 | 0056 §7 | corrects | A `read` ACL written, changed or retired on a target bumps the target's **generation**, as 0014 §5 and §10 say for hiding, since both kinds of read restriction hide for privacy; a version bump is for changes that hide nothing (0014 §10's sitelink and federation examples). |
| B3 | 0056 §8 | amends | The Postgres search fallback filters results after the scan with the read-time visibility set; no `read_groups` column is added to `view.page`, `view.page_text` or `view.term`. |
| B4 | 0049 §12; 0069 §10; 0060 §10; 0061 §11; 0014 §10 | amends | Listing keys (`tp:`, `sc:`, `sp:`) carry a **listing version**, the maximum activity ID over the listing's members, computed in Postgres at read time, in place of tag purges on ordinary writes; 0014 §1 principles 1 and 4 hold unchanged. Tag purges remain for erasure and hiding. |
| B5 | 0056 §7; 0014 §10 | amends | Rule: every key whose value depends on what the viewer may read carries `{vis}`. 0014 §10 marks each key. Classification to confirm: carry `{vis}`: `e:`, `t:`, `prov:`, `p:`, `d:`, `sug:`, `css:`, `post:`, `th:`, `tp:`, `fi:`, `rf:`, `ld:`, `sc:`, `sp:`; do not: `s:`, `rl:`, `up:`, `fp:` (a repository bundle is public upstream content). |
| B6 | 0004 §3; 0015 §3 (`reconcile`); 0018 §3 | amends | The identifier properties that drive tier-3 inference are **instance** configuration (the instance `reconcile` record), so tier-3 links stay instance-wide; a tenant's `reconcile` may switch inference off for itself and set provider order and link properties, but not add inference properties. |
| B7 | 0066 §4; 0004 §2 unchanged | corrects | Merging two local lexemes is a `redirect`, as for items: the source's forms and senses move to the target with new numbers and each part leaves a redirect; no cluster is formed within the local namespace. |
| B8 | 0064 §8; wikibase-compat.md §2 | corrects | Entity schemas have no RDF: no terms on an entity node, no `wikibase:EntitySchema` type. wikibase-compat.md's description already says so and stands. |
| B9 | 0059 §2, §3 | extends | The query service's store holds `full` with `metadata=local` by default for both backends; `query.metadata = all` is a query-service setting that makes the embedded worker apply every delta and the QLever loader take the `all` dump. |

## Wave B (decided 2026-10-08, James)

| ID | ADR and section to change | Change | Decision |
|---|---|---|---|
| C1 | 0007 §6; 0013 §5.4 | amends | One actor record per provider, kind `provider`, in `actors/{provider}`, agent type `prov:Organization`, written when the provider is registered; every attestation, OpenAlex's mirror records included, names an actor key. 0007 §6's "no actor records are kept" becomes this. |
| C2 | 0035 §5 | extends | Adoption writes the `actor` ACL restricted to `suppress` for a source-hidden account, so `hidden` has one origin (0007 §4). |
| C3 | 0075 §5 | corrects | "the `read` grant" → `basic` (0024 §4), which every key and token carries. |
| C4 | 0024 §5; 0014 §4 | corrects | Rate-limit counters are `rl:{class}:{key}` in both, the key being the actor key or, for anonymous requests, the IP. |
| C5 | 0027 §2 | extends | `private.password` is in the "re-established, not carried" class. |
| C6 | 0018 §2; 0028 §2 | extends | A tenant also has one `source/{name}` partition per entity source (0078 §4) and the instance the farm partitions of 0028 §2; the fixed count of six goes. |
| C7 | 0018 §2; 0015 §3 | corrects | "Partition 0" is a designation, not an ID: the ADRs say "the instance `config` partition", found by its registry name, with a random 64-bit ID like every other; offset 0 of it holds the first `key:` record. |
| C8 | 0058 §8 | corrects | Cite 0028 §5 and 0040 §6 for tenant deletion, and say the partitions are dropped after the `erase` records, so the domain's child table can go. |
| C9 | 0059 §4; 0028 §1 | amends | `store` with `remote` is refused only on a farm; on a single-tenant instance the remote store holds one tenant's graphs and satisfies `store`. `query.isolation` is a row of 0028 §1's switch table: `isolated = store`, the shared presets `dataset`. |
| C10 | 0016 §1 | amends | The MediaWiki names are the permission names; the Scatterbase column (`view`, `view_deleted`, `block_user`) is historical. |
| C11 | 0016 §2 | extends | Add `browsearchive` (0023 §11), `managechangetags` and `changetags` (0030 §5) to the later-ADRs table, and `ts-viewoperator` (0040 §9) to the instance-rights list; check `groups.toml`. |
| C12 | 0024 §11; 0056 §3 | corrects | `createaccount` governs self-registration and creating an account for another, as in MediaWiki; default `universe`; a private tenant removes it from `universe` (0056 §3 stands). 0024 §11's `autoconfirmed` default goes; a subsidiary a new user creates is pending until approved (0025 §3). |
| C13 | 0054 §8 | corrects | "Copying files" needs `upload`, default `autoconfirmed` (0039 §21), not `user`. |
| C14 | 0016 §2; 0046 §8; 0047 §12 | amends | Jobs run under subsidiaries: `ts-runjob` defaults to `bot` everywhere, 0016 §2 drops `sysop`; an administrator runs a job by creating or approving a subsidiary they operate; 0047 §12's `sysop` job rate row goes. (James: "administrators should be able to run jobs under subsidiaries", read as this.) |
| C15 | 0016 §4 | corrects | "`rights` and `block` records" → "`membership` and `block` records" (payload type `scatter:v0/membership`). |
| C16 | 0016 §2, §5 | corrects | `deletedhistory`/`deletedtext` are reported to clients by members of the deletion group and gate nothing (0023 §11); 0016 §5's answer to "see hidden usernames" is "membership in the deletion group". |
| C17 | 0012 §8 | corrects | Public caches are purged by erasure, and by deletion or hiding by `read` ACL (0023 §5, 0014 §5). |
| C18 | 0015 §3; 0013 §5.5 | amends | 0015 §3's scope paragraph is the one list of instance kinds, gaining `reports`, `forwarder` and `page-repo` (tenant or instance); 0013 §5.5 refers to it. |
| C19 | 0015 §3 | extends | A `file-repo` row (0039 §11), tenant or instance scope, logged as a 0039 amendment. |
| C20 | 0039 §11; 0042 §11; 0052 §1 | amends | `repo.cache_ttl` is a per-repository field of the `page-repo`/`file-repo` record with a per-kind default: 7 days for files, one hour for template source and page bundles. |
| D1 | 0018 §1 | extends | `instance` is reserved in the shared namespace of slugs and issuer codes (0040 §2 and 0046 §6 cite 0018 §1 for it; 0018 §1 does not yet say so). |
| D2 | 0015 §5 | extends | Add `content-models.toml`, `notation-schemes.toml`, `themes.toml` and `fragments.toml` to the registry-files table (chapter 02 §1.4 follows). |
| D3 | 0015 §3 | extends | Rule: a dotted setting key an ADR names without a scope is a tenant `site` setting. (Chapter 23 §3 marks the keys it settles.) |
| D4 | 0007 §3; 0030 §2, §5, §12 | extends | Name the right that seeing an IP in the abuse-handling store requires, so the permission catalogue can list it. Proposed: `ts-viewip` *(new)*, default `sysop`, James to confirm. |
| D5 | 0059 §2 | extends | `query.backend`, `query.path` and `query.endpoint` are deployment configuration (0033 §12), not `site` records. |

Applied directly on 2026-10-08, no log entry: 0023 §11's `protect` row now names the confidential `read` restriction 0056 §4 added; `registry/README.md`'s allocation paragraph now lists Scope 312/313 and the namespaces outside the 210–229 and 310–329 ranges (was E13).

## Wave C (decided 2026-10-08, James)

| ID | ADR and section to change | Change | Decision |
|---|---|---|---|
| E1 | 0002 §5 | corrects | The retention default is a tenant `site` setting (`retention.default`), "each tenant" not "each instance" (D3). |
| E2 | 0012 §2.2; `providers.toml` | extends | `sync_deltas` (`summary` default, `full`) is a field of the provider entry; add it to the TOML schema and header. |
| E3 | 0002 §8.2 | corrects | Re-running `adopt` skips an entity whose content hash matches and rejects one that differs (0035 §3). |
| E4 | 0002 §8.2; 0035 §3 | corrects | `adopt` is "the one whole-entity write that needs no base revision"; `create-or-add` with `overwrite` and a base still replaces. |
| E5 | 0002 §8.7 | corrects | The local bulk sketch's `create` with a `match` key becomes `create-or-add`. |
| E6 | 0002 §8.3; 0070 §3 | amends | Instance jobs (shallow-mirror fetches, 0040 §6) are excepted from "every job's actor is a subsidiary": their actor is the instance (0040 §2). |
| E7 | 0035 §3 | corrects | Strike "once the Lexeme namespace is implemented"; `L` is adoptable (0066). |
| E8 | 0002 §4; 0037 §1; `providers.toml` | amends | `adapter` is one field per provider; `actor_model` (`individual` or `provider-only`) is an explicit provider field, since `issuer == slug` is not the test (OSM has authors). |
| E9 | 0002 §5; 0036 §2 | amends | 0036 stands: a provider may opt into the keyed-style clear (a `put` of the empty state) in place of a tombstone for upstream deletion, by a registry flag `deletion = clear`; OSM sets it. 0002 §5 gains the option; 0036 §2 cites 0002 §5 rather than 0009 §9. |
| E10 | 0031 §1 | corrects | Role table: *item requires statement* is Q21503247; P2305 is the parameter of both *item of property constraint* and *allowed values* (one row, two uses). |
| E11 | 0038 §1 | corrects | Only aliases and sitelinks on `M` IDs are refused with `not-supported` (0065 §1). |
| E12 | 0008 §2 (Project row) | corrects | `triplespace-sprint` is *allowed* on Project subpages; the default stays `wikitext` (0061 §2). |
| E14 | 0029 §2 | corrects | Drop "as Thread talk is": Thread talk is `virtual` since 0049 §2; a resolver's talk stays `reserved`. |
| E15 | 0051 §1 | corrects | The list of redirect-capable namespaces is replaced by a reference to the namespace catalogue (chapter 10 §1.4), which includes Scope and Query `/doc` pages. |
| E16 | 0041 §2 | corrects | Reserved-and-unimplemented models: `css`, `javascript`, `unknown`, `wikibase-query`; `wikibase-lexeme` and `EntitySchema` are implemented (0066, 0064). |
| E17 | 0041 §3 | amends | The namespace catalogue (0008 §2 as amended; chapter 10 §1.4) is the one place that says where a model is allowed; 0041 §3's "Default in" column carries defaults only. |
| E18 | 0043 §3 | corrects | "the other two stay excluded" (`css`, `javascript`), pointing at 0055 §1 for `sanitized-css`. |
| E19 | 0033 §9.1 | corrects | `File:` and `Image:` links are no longer chips; files in wikitext are 0039 §13's rule. |
| E20 | 0033 §9.1; 0042 §5 | amends | Tags and switches the registry marks `implemented` are rendered by the built-in renderer whether or not expansion is on; 0033 §9.1's chip list is pruned to what stays `chip`. |
| E21 | 0042 §3 | corrects | Template (10) models: `wikitext`, and `sanitized-css` by the `.css` suffix rule (0055 §1). |
| E22 | 0042 §3 | corrects | While expansion is off, 10 is `reserved` and 11 registered but disabled, both in `namespaces.toml` with `enabled_by` (as 0043 §2 says for 828/829). |
| E23 | 0042 §10; 0043 §11 | extends | `render_state` gains `unheld_misses integer`, the count 0043 §11 describes. |
| E24 | 0065 §1, §4 | corrects | Caption triples are emitted on the page node `{base}/page/{page ID}` (0038 §11, 0041 §7); there is no "`M` node". |
| E25 | 0011 §2 | extends | Add the instance `log` (`log/{farm}`) row: kind, written by, history, integrity, export. |
| E26 | 0011 §3 | extends | Admitted payload types include `scatter:v0/expunge` (instance log only, 0039 §10). |
| E27 | 0011 §6.1; 0018 §10; 0035 §5 | extends | Add `takedown/expunge`, `reclaim/reclaim` and the no-redirect move to the table. `reclaim/reclaim` projects from the reclaiming binding record in the tenant `log` and is visible to everyone. |
| E28 | 0051 §5 | corrects | A move that leaves no redirect is logged as `move/move` with `noredirect: true` in its parameters (MediaWiki's `suppressredirect`); `move/move_redir` keeps MediaWiki's meaning, a move onto an existing redirect. |
| E29 | 0011 §7 | corrects | `protect/*` parameters are, per restriction, the group and the expiry, with no cascade (0023 §3); mirrored upstream `protect/*` events keep MediaWiki's level. |
| E30 | 0011 §8 | extends | Rule: a log type without a row is a `prov:Activity` carrying `scatter:logType` and `scatter:logAction` only; `thread/pin`/`unpin` get their `as:Add`/`as:Remove` row. |
| E31 | 0060 §7; 0020 §3 | amends | `notify` is refused on a `scope` or `rows` watch (`ts-watch-notify-unsupported`): those watches are feed filters, not subscriptions. |
| E32 | 0021 §2 | extends | A `rename` reason: the renamed account is addressed when its actor record changes its name; performer the renamer or the instance operator (0040 §7). |
| E33 | 0018 §11; 0046 §9 | extends | The tenant list is the `tenants` array of `GET /tenancy` at the farm base (slug, host, name, status), and `Special:Tenants` is its page. |
| E34 | 0069 §9; 0012 §5 | corrects | There is no `list=threads`; `origin=` is a parameter of `GET /page/{id}/threads` only. |
| E35 | 0076 §2, §6 | corrects | Scopes are addressed by page ID: `/scope/{pageid}/dumps`, `/scope/{pageid}/dataset.jsonld`. |
| E36 | 0012 §5; 0019 §9 | extends | `GET /page/{id}/history` is defined in 0012 §5's page table (from 0019 §7); 0019 §9 stops calling it unchanged. |
| E37 | 0028 §11 | corrects | The farm-base global group and block interface is the Action API (`action=globalblock`, `list=globalblocks`, the global-group modules); no REST routes are claimed from 0016 §7. |
| E38 | 0016 §7 | extends | `DELETE /acl/{kind}/{id}` retires an ACL (0023 §8 assumes it). |
| E39 | 0012 §5 | corrects | The index of later ADRs' routes is replaced by a link to chapter 18 §3.2. |
| E40 | 0012 §5; 0029 §6 | amends | `/resolve` order folds 0029 §6's step: a string that normalizes to a notation key (0048 §2) resolves, as the exception to the bare-key rule; any other bare key is a suggestion. |

## Wave D (decided 2026-10-09, James)

| ID | ADR and section to change | Change | Decision |
|---|---|---|---|
| F1 | 0052 §1; 0053 §4 | corrects | The repository field is named `repo.cache_ttl` in 0052 §1 too (it is the field C20 makes per-repository); one name everywhere. |
| F2 | 0052 §5; 0053 §9 | amends | A foreign page is hidden as a local page is deleted: the Delete action, held by an administrator (`delete`), writes the `read` ACL of 0053 §9; 0052 §5 lists Delete on a foreign page with that meaning. |
| F3 | 0068 §4; 0069 §7 | amends | 0069 (the newer) stands: a following fork sends a `talk`-destination proposal through 0069 §6 now, gated by 0069 §6's switch; 0068 §4's "later phase" applies to page proposals only. |
| F4 | 0071 §4; 0073 §3 | amends | An undesignated `match_key` property may find a subject (step 1 via `view.identifier`) but never create one; the refusal in step 3 stands and `lines.identifiers` follows the same rule. |
| F5 | 0019 §4 | corrects | The Comment row lists every operation that carries a summary: add `pin`, `unpin`, `propose`, `submit`, `withdraw`. |
| F6 | 0019 §4 | corrects | `target` is `page` only for `attach` and `detach`; `pin` and `unpin` name any attachment, `actor` included (0049 §6, 0069 §4). |
| F7 | 0067 §3; 0019 §4; 0038 §1 | amends | A proposal's kind, destination, base, payload, omitted and flags are **page metadata of the thread page**, set by the `propose` operation and projected to `view.proposal`; they are not fields of `create`, whose `target` stays the thread's home. |
| F8 | 0067 §5; 0019 §6 | amends | On a proposal thread `declined` may be set only by the proposer or a holder of `edit` on the subject's talk page; any other `declined` post is refused with `ts-proposal-status`; 0019 §6 notes the exception. |
| F9 | 0060 §5 | corrects | `view.scope_member.kind` admits `form` and `sense` beside `entity` and `page`; a lexeme part is keyed by its part ID. |
| F10 | 0045 §4 | extends | The columns table gains a `thumbnail` field with `width` (0065 §3); `render` is struck from 0065 §3. |
| F11 | 0061 §4 | corrects | `subjects` is not a rule parameter: a rule's subject type follows the sprint's scope. |
| F12 | 0064 §4 | corrects | A schema using unsupported ShEx features is saved and marked `unvalidatable` ("not validated", not "refused at save"). |
| F13 | 0022 §7; 0024 §5 | amends | ActivityPub fan-out has its own rate class, `federation`, counted per instance against the delivery queue, never against the posting actor. |
| F14 | 0074 §4; 0067 §6 | amends | A publication runs under a subsidiary account that holds its credential (C14); 0067 §6's rule is scoped to proposals ("never pushes a proposal under its own name"). |
| F15 | 0010 §2; 0038 §7 | corrects | 0010 §2 is the one tabs table: Page data on the thread row, Talk on the document-page row; 0038 §7 refers to it. |
| F16 | 0003 §6; 0010 §4 | extends | The entity page has the "About this page" panel of 0010 §4 beside its "Where this comes from" panel; the scope count (0060 §8) and proposal count (0067 §7) sit in it. |
| F17 | 0003 §10; 0010 §13 | supersedes | The prototype is `triplespace-ui` with `scatter-wasm`, server-rendered Rust (0034 §3, §13); the fixture list and audit step stay; "later Rust implementation" and "lives in `ui/`" go. |
| F18 | 0057 §10 | extends | The right-to-left reading of forwarded headers is the API's defence when 0056 §10 line 6 does not hold; a conforming edge produces `C, E`, the example shows defence in depth. |
| F19 | 0056 §10 (line 12); 0057 §10 | amends | 0057 (the newer) stands: the web tier may hold, beyond the forwarder key, the password of the Valkey its cache uses, which may be the instance's L1 Valkey; line 12 is qualified accordingly. |
| F20 | 0047 §1 | extends | The field table gains `listed`, `requires` and `note`; `origin` may be an extension's name as `version.toml` lists it (0077 §8). |
| F21 | 0047 §4.4 | corrects | `RandomRootpage`, the registry's spelling; `check_adrs.py` check 5 becomes case-sensitive on canonical names. |
| F22 | 0051 §6 | corrects | The redirect reports are index-backed and live (0047 §4.3's `index` row). |
| F23 | 0005 §3 (rule 7); 0034 §6 | corrects | Rule 7 is the one wasm list, the nine crates of the table; 0034 §6 and `cargo xtask wasm` read it. |
| F24 | 0005 §3 (rule 2) | corrects | The pure list gains `scatter-files`, `scatter-extract`, `scatter-vocab`. |
| F25 | 0059 §8 | corrects | `triplespace-query` is in the surfaces layer (layer 4). |
| F26 | 0005 §2 | extends | Fold the clauses the table lacks from 0056 §16, 0064 §9, 0057 §12, 0054 §12, 0055 §9, 0069 §12, 0067 §9, 0068 §7, 0037 §8, 0036 §7 (chapter 22 §2.2 lists them). |
| F27 | 0033 §1, §17 | corrects | The server binary is `triplespace-server`; packaging ships three binaries, the CLI's binary being `triplespace`. |
| F28 | 0013 §5.6 (A9) | extends | `view.proposal` (0067 §5) is a row of the schema map and of chapter 03 §5. |
| F29 | 0019 §6 | extends | `thread-statuses.toml`, which 0019 §6 names, holds the shipped statuses; the audit's report that it was missing was wrong (the writers had a partial copy of the registry). |

## Text, not design (not yet applied: T1 and T2 await a log entry in 0032 and 0001/0011; T3 is settled by chapter 23 §4 linking 02 §1.4)

Items from the audit's defects table that add a sentence rather than fix a citation. The six citation fixes (0014 §10, 0033 §8, 0036 §6, 0048 §3, 0049 §12, 0069 §10) were applied directly on 2026-10-08 without log entries, as corrections of citations only.

| ID | ADR and section | Change |
|---|---|---|
| T1 | 0032 §6 | Say that `sparql-sync` also writes the marker triple `<{base}/.well-known/query> scatter:cursor` after each batch (0059 §2). |
| T2 | 0001 §1; 0011 §8 | Say that `{base}/record/{partition}/{offset}` names a post revision or a local log event, by the record's payload type. |
| T3 | 0015 §5; chapter 23 | When chapter 23 is written, one of 02 §1.4 and 23 holds the registry-files table and the other links. |
