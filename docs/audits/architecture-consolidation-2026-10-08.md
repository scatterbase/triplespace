# Architecture consolidation audit, 2026-10-08 (wave A)

Findings from writing chapters [01](../architecture/01-log-and-records.md)–[04](../architecture/04-entities-and-identifiers.md) of `docs/architecture/` from the ADRs ([PLAN](../architecture/PLAN.md) step 3). Each inconsistency is also marked in the chapter where it sits, as a `> **Inconsistency.**` note, and the note stays until an ADR settles it. Nothing here was fixed in the chapters or the ADRs: each row is a candidate for a direct decision logged under 0050 §6 and §8, or for a new ADR. "Resolution" is a proposal, not a decision.

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
