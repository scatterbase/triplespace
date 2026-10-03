# Record and payload shapes

What `scatter-log` writes, byte for byte. The ADRs fix the record format (0006 §2–3, 0015
§1–2), name every payload type (`docs/registry/graphs.toml`) and say what each carries;
this document fixes the field names and shapes, which the ADRs leave to the crates. It is
the contract the log's test vectors are generated from. **Changing anything here after
the first instance is a format version** (header field 0).

Companion: [payloads-actors.md](payloads-actors.md) holds the six actor-side payloads
(`actor`, `link-account`, `membership`, `block`, `key`, `acl`), and
[wikibase-compat.md §3](wikibase-compat.md) the entity JSON that change sets carry.

## 1. Encoding

Every record, header and part is **core-deterministic CBOR** (RFC 8949 §4.2.1): shortest
integer and length encodings, definite lengths only, map keys sorted bytewise by their
encoded form (for text keys that is shorter first, then by UTF-8 bytes), no duplicate
keys, no tags, no floats where an integer will do, and floats in the shortest width that
round-trips (half, single or double; NaN is `f97e00`). JSON maps onto it structurally
(0006 §2): strings are text strings, JSON numbers are integers or floats as JSON typed
them (`51` is an integer, `51.0` a float; a float is parsed correctly rounded, never
approximately), objects are maps with text keys, `true`/`false`/`null` are the simple
values. The decoder is strict: a record whose re-encoding differs from its bytes is
rejected. Test vectors for this section and §2 are in [vectors/log-v1.json](vectors/log-v1.json),
checked by `vectors/check.py` and by `scatter-log`'s tests.

Conventions this document uses throughout:

- **Optional fields are absent, never `null`**, except where a shape says `or null`.
- **Times** are integers, microseconds since the Unix epoch, UTC (header field 3's unit).
  Upstream times a provider gave as strings are kept as the provider's string.
- **IDs** are the prefixed forms of 0017 §1 (`Q5`, `WDQ42`, `domain:example.org`),
  written as text. Page IDs and offsets are integers. Statement GUIDs are text.
- **Hashes** are 32-byte byte strings in CBOR; this document shows them as `<32 bytes>`.
- **Field names** are `snake_case` except where a Wikibase or MediaWiki name is being
  carried through (`numeric-id` never is; `snaks-order` inside entity JSON is).

## 2. The record

### 2.1 Header

A CBOR array of ten elements (0006 §3; 0015 §2). It is what the Merkle tree commits to,
and it never contains personal data.

```
[ 1,                      # 0  format version
  12345678901234567890,   # 1  partition: 64 random bits; 0 is the instance config partition
  41,                     # 2  offset in the partition, gapless from 0
  1790000000000042,       # 3  appended_at, µs since the epoch, UTC
  "scatter:v0/changeset", # 4  payload type
  "Q6",                   # 5  key: an identifier or null, never content
  <32 bytes>,             # 6  body commitment
  900,                    # 7  revision ID or null
  null,                   # 8  log ID or null
  12 ]                    # 9  page ID or null
```

Fields 7–9 are assigned in the appending transaction, before the leaf hash. Revision IDs
are `provider_number << 40 | n` (0015 §2); the instance's own are provider 0.

### 2.2 Body

A CBOR array of parts; every Triplespace payload type has at least three, in this order,
and a type may declare more after them:

| # | Part | Holds |
|---|---|---|
| 0 | `content` | The payload (§3–9) |
| 1 | `comment` | The edit summary or reason, one text string, or `null` |
| 2 | `attestation` | Who is responsible and how (§2.3) |

Each part is `[salt, bytes]`: a 16-byte random salt and the part's canonical CBOR as a
byte string. An erased part is `[null, leaf]`, the leaf being the 32-byte hash the part
had.

```
leaf_i     = SHA-256(0x04 ‖ i ‖ salt_i ‖ bytes_i)        i as one byte
commitment = SHA-256(0x02 ‖ leaf_0 ‖ leaf_1 ‖ … ‖ leaf_{n-1})   → header field 6
content    = SHA-256(0x03 ‖ bytes_0)                      the content hash, for dedup and cursors
preimage   = SHA-256(0x05 ‖ SHA-256(0x03 ‖ bytes_0) ‖ SHA-256(0x03 ‖ bytes_1))   what a client signs
```

The Merkle tree over headers is RFC 6962 with SHA-256: leaf `0x00 ‖ header bytes`,
node `0x01 ‖ left ‖ right`. `0x06` is the instance attestation's signature preimage
(0040 §3). No other tags exist.

### 2.3 Attestation

A map. The ordinary form:

```json
{"actor": "local:42"}
{"actor": "local:43", "job": 17, "tags": ["OAuth CID: 12"]}
{"actor": "local:43", "job": 17, "signature": {"key": "…z-base-32…", "alg": "ed25519", "sig": <64 bytes>}}
{"actor": "local:9", "evidence": <bytes>}
```

| Field | Meaning |
|---|---|
| `actor` | The actor key (0007 §1). For an upstream record, the upstream actor's key; for a revision whose user upstream hid, absent, with `hidden: true` |
| `job` | The job ID whose run wrote this record (§7); absent for an interactive edit |
| `tags` | Change tags (0030 §5): registered tag names, and consumer tags `OAuth CID: {id}` (0025 §4) |
| `signature` | A client signature over the preimage of §2.2 by the actor's current key (0015 §1, 0024 §4); `key` is the key ID |
| `evidence` | A fediverse post's signed remote activity, as received bytes (0022 §8) |
| `hidden` | `true` when upstream hid the user (0007 §5); then `actor` is absent |

The **instance attestation**, written only by the server for records the instance writes
into a tenant on its own authority (0015 §1; 0040 §3):

```json
{"actor": "instance:scatter",
 "authority": {"partition": 7, "offset": 3, "leaf": <32 bytes>},
 "job": 17,
 "binding": true,
 "signature": {"key": "…instance key ID…", "alg": "ed25519", "sig": <64 bytes>}}
```

`authority` names the instance record (an expunge, a template, a global filter's block)
the act cites; `binding` is `true` for a prerogative and `false` for a provision (0040 §3,
§5–6: a prerogative is an act that binds, and whether it binds is the whole distinction,
so the field is the boolean and not a kind name); the signature is by
the instance key over `SHA-256(0x06 ‖ content_hash ‖ comment_hash ‖ authority)`, where
`authority` is the canonical CBOR of that map. A submitted record carrying one is refused
with `ts-prerogative`.

## 3. `scatter:v0/changeset`

One record per operation, in the `local`, `mirror/{provider}` or `pages` partition,
keyed by its subject: the entity ID, or for a page's statements the page ID in decimal
(0038 §1). The content is the operation object. The NDJSON wire format of 0002 §8.7 is
the same object per line, under a `{"job": …}` first line; the differences between wire
and record are noted per operation (wire-only fields are resolved before the append).

Every operation has `op` and a subject, `id` (an entity) or `page` (a page ID):

| `op` | Partition | Subject | Fields |
|---|---|---|---|
| `put` | mirror | `id` | `entity`, `upstream`, `prev_upstream`, `first_seen`, `size`, `prev_size`, `changes`, `delta` |
| `tombstone` | mirror | `id` | `upstream` |
| `redirect` | mirror, local | `from` (the key) | `to` |
| `create` | local | `id` (allocated) | `entity`; `match`, `via` when it came from a `create-or-add` |
| `adopt` | local | `id` | `entity`, `source_revid`, `source_time`, `source_pageid` |
| `add` | local, pages | `id` or `page` | `labels`, `descriptions`, `aliases`, `claims`, `sitelinks`, `references`, `qualifiers`; `match`, `via`, `overwrite` when it came from a `create-or-add` |
| `remove` | local, pages | `id` or `page` | `statements`, `labels`, `descriptions`, `aliases`, `sitelinks`, `references`, `qualifiers`, `link` |
| `override` | local | `id` | `statement`, `rank`, `suppress` |
| `retain` | local | `id` | `policy` |
| `convert` | local | `id` (the foreign ID) | `local` |
| `same-as`, `different-from`, `equivalent-property` | local | the highest-ranked member (the key) | `ids` |

### 3.1 Mirror operations

```json
{"op": "put", "id": "WDQ42", "entity": {…canonical JSON…},
 "upstream": {"revid": 2148573921, "time": "2026-09-30T12:00:00Z"},
 "prev_upstream": {"revid": 2148570000}, "first_seen": 1780000000000000,
 "size": 48211, "prev_size": 47990,
 "changes": {"claims": {"P31": {"added": 1, "removed": 0, "changed": 0}},
             "labels": {"de": {"added": 0, "removed": 0, "changed": 1}},
             "sitelinks": {"enwiki": {"added": 0, "removed": 0, "changed": 1}}},
 "delta": {…}}
{"op": "put", "id": "OAW123", "entity": {…}, "upstream": {"version": "2026-08-30"}, "size": 2210}
{"op": "tombstone", "id": "WDQ77", "upstream": {"revid": 2148580000, "time": "…"}}
{"op": "redirect", "from": "WDQ99", "to": "WDQ42", "upstream": {"revid": 2148581000}}
```

- `entity` is the entity's canonical JSON in the **storage form**: `numeric-id` dropped,
  and `hash` present only where the ingest guard kept upstream's (0006 §2). IDs are in the
  provider form (`WDQ42`), rewritten by the adapter.
- `upstream` names the upstream version: `revid` (an integer; it is also header field 7's
  `n`) and `time` for a provider with revisions; `version` (the provider's own string, a
  dump date or `updated_date`) for one without. `prev_upstream` has the same shape.
- `first_seen`, `size`, `prev_size`, `changes` and `delta` are 0012 §2.2; `changes` maps
  `claims` by property, `labels`/`descriptions`/`aliases` by language and `sitelinks` by
  site to `{added, removed, changed}` counts; `delta` is the statement-level difference
  and is present only under `sync_deltas: full`. In bootstrap mode all five are absent.
- A keyed entity's mirror contribution is cleared with a `put` whose `entity` is the empty
  state, never a `tombstone` (0002 §5).

### 3.2 Local writes

```json
{"op": "create", "id": "Q900001", "entity": {…}}
{"op": "create", "id": "Q900002", "entity": {…}, "match": {"P356": "10.1234/x"}, "via": "create-or-add"}
{"op": "add", "id": "Q5", "claims": {…}, "match": {"P356": "10.1234/y"}, "via": "create-or-add"}
{"op": "add", "id": "Q5", "entity": {…}, "match": {"P356": "10.1234/y"}, "via": "create-or-add", "overwrite": true}
{"op": "adopt", "id": "Q6", "entity": {…}, "source_revid": 41877, "source_time": "2026-09-20T14:02:11Z", "source_pageid": 12}
{"op": "add", "id": "Q5", "claims": {"P50": [{…statement…}]}, "labels": {"fr": "Exemple"}, "aliases": {"en": ["Ex"]}}
{"op": "add", "id": "WDQ42", "references": {"WDQ42$C13E7A23-…": [{"snaks": {…}, "snaks-order": ["P248"]}]}}
{"op": "add", "page": 1234, "claims": {"P40": [{…}]}}
{"op": "remove", "id": "Q5", "statements": ["Q5$A8C2D6D3-…"], "labels": ["fr"], "aliases": {"en": ["Ex"]}, "sitelinks": ["enwiki"]}
{"op": "remove", "id": "WDQ42", "references": {"WDQ42$C13E7A23-…": ["eaf0a11b92f297234266b31f9331c3ebcfe09c1e"]}}
{"op": "remove", "id": "Q456", "link": {"op": "same-as", "ids": ["Q456", "OAW123"]}}
{"op": "override", "id": "OAW123", "statement": "OAW123$2F1C…", "rank": "deprecated"}
{"op": "override", "id": "OAW123", "statement": "OAW123$2F1C…", "suppress": true}
{"op": "retain", "id": "WDQ123", "policy": "retain"}
{"op": "convert", "id": "WDQ123", "local": "Q456"}
{"op": "same-as", "id": "Q456", "ids": ["Q456", "OAW123"]}
{"op": "different-from", "id": "Q1", "ids": ["Q1", "OAW7"]}
{"op": "equivalent-property", "id": "P12", "ids": ["P12", "WDP585"]}
{"op": "redirect", "from": "P12", "to": "WDP585"}
```

- **`create`** is strictly creation (0002 §8.2, A19): on the wire it carries `ref` (`$w1`)
  and no `id`; the server mints the ID, rewrites every `$ref` in the batch, and the record
  carries `id` and no `ref`. It never writes to an entity that exists; a keyed entity,
  which exists by its key, takes `add`.
- **`create-or-add`** is the wire operation that carries a `match` key (0002 §8.5). It is
  never a record: the server resolves it and appends what happened, a `create` when
  nothing matched or an `add` keyed by the matched entity when something did, each
  carrying `match` and `via: "create-or-add"` so the log says what was asked as well as
  what was done. With `overwrite: true` and a base revision, the matched entity's state is
  replaced: the record is an `add` carrying the whole `entity` and `overwrite: true`,
  which a projection applies as a replacement.
- **`adopt`** is 0035 §3 exactly; `source_time` is the source's own string.
- **`add`** merges: a term per language (`labels`/`descriptions` as `{lang: value}`,
  `aliases` as `{lang: [values]}`), statements under `claims` in Wikibase's statement
  shape, `sitelinks` as `{site: {title, badges}}`, and `references` or `qualifiers` onto an
  existing statement by GUID. Terms are also how a local term overrides a mirrored one,
  since the local graph wins (0002 §3); `override` is for statements only.
- **`remove`** retracts local assertions by GUID, language, alias value, site, reference
  hash, or qualifier hash; `link` retracts a `same-as`, `different-from` or
  `equivalent-property` by repeating it.
- **`override`** carries one of `rank` (`preferred`, `normal`, `deprecated`) or `suppress`
  (`true`); a later override on the same statement replaces it. The subject is the
  statement's entity; the record is keyed by it. On the wire `id` may be omitted, as the
  0002 §8.7 sketch does; the server fills it from `statement`.
- **`retain`**: on the wire a bulk `{"op": "retain", "ids": […], "policy": …}` fans out to
  one record per entity. `policy` is `cascade`, `orphan` or `retain`.
- **`convert`**: `local` is the minted ID; the record is keyed by the foreign ID, and the
  `same-as` it implies is written as a second record keyed by `local`.
- **`same-as`** and the other links are keyed by their highest-ranked member under the
  provider order of 0004 §4 (a keyed member first, then a local one, then providers in the
  instance's order): `id` names it and `ids` lists both. The key is the record's
  identifier for compaction and erasure, not the cluster's canonical ID, which the
  resolved view computes from the same order and which can change as members come and go.
  On the wire `id` may be omitted, as the 0004 §9 and 0009 §10 sketches do; the server
  fills it. An `id` that is given must be the highest-ranked member.
- **Temporary handles** (`$w1`; 0002 §8.5) are `$` followed by letters, digits or `_`, and
  are replaced only in ID positions: `id`, `ref`, `to`, `from`, `local`, `property`, the
  members of `ids` and `badges`, the keys of `claims`, `qualifiers`, `snaks` and `match`,
  and the `id` of an entity value. A label or string value that starts with `$` is text.
  The `entity` of a `create` or `create-or-add` carries the handle as its `id` on the wire;
  the server replaces it with the minted or matched ID.

### 3.3 Base revision

A local write's `baserevid` (0006 §8) is a request field, checked against the newest
record for the key before the append; it is **not** content and is not stored. The
record's own place in history is its offset and header field 7.

## 4. `scatter:v0/config`

In the instance and tenant `config` partitions, keyed `{kind}:{code}`. The content is the
entry; `null` retires it. Two kinds belong to the log itself:

```json
{"kind": "key", "key_id": "…z-base-32…", "alg": "ed25519", "public_key": <32 bytes>}
{"kind": "graph", "name": "mirror/wikidata", "kind_of": "source", "scope": "tenant",
 "history": "latest", "integrity": "hashed", "export": "public",
 "partition": 12345678901234567890, "segment_exponent": 16, "hash": "sha-256",
 "payload_types": ["scatter:v0/changeset", "scatter:v0/erase"]}
```

- `key:` — offset 0 of partition 0 is the instance's first key (0006 §6). A rotation is a
  later `key:` record whose **attestation** carries a `signature` by the *previous* key
  over the new record's preimage; a tenant move is such a rotation naming the new
  instance's key, with `final_checkpoint` (the tenant's last checkpoint, as text) in the
  content (0018 §10). Each tenant `config` begins with a copy of the current `key:` record
  and receives every later one (0018 §2).
- `graph:` — the registry row of `graphs.toml` with the instance's values filled in: the
  partition ID, the segment exponent *k* of a `hashed` partition, and the hash function
  (`sha-256`; the only value this version defines). `kind_of` carries the TOML `kind`
  because `kind` is the config kind. For `per_provider` and `per_repo` graphs the `name`
  is the instantiated one (`mirror/wikidata`). The `graph:` record precedes the
  partition's first record; `graph:config` is offset 1 of partition 0.

The instance-scope kinds of 0018 §3 and 0046 §2:

```json
{"kind": "tenant", "slug": "librarybase", "base": "https://librarybase.org", "adopted_from": "https://librarybase.org/", "provider": {"code": "LB", "number": 2}}
{"kind": "alias", "old_host": "domains.wikibase.cloud", "base": "https://internetdomains.wiki", "date": 1760000000000000}
{"kind": "primary", "slug": "librarybase"}
{"kind": "primary", "slug": "example", "offer": {"partition": 0, "offset": 211}}
```

Every other kind's entry is **the registry row as JSON**, with the TOML field names, and
is defined with the crate that embeds that registry: `provider` (`providers.toml`,
`scatter-providers`), `issuer` (`issuers.toml`), `group` (`groups.toml`, with `scope`
for a global group), `tenancy` (one switch: `{"kind": "tenancy", "value": "any"}`),
`keyed-type`, `resolver`, `notation-scheme`, `namespace`, `site-alias`, `thread-status`,
`content-model`, `special-page`; `site` is `{"kind": "site", "value": …}` with the
setting's own scalar or object; `role`, `reconcile`, `providers`, `sitelink-policy`,
`view-pin`, `template`, `provider-readers`, `federation-policy`, `consumer`,
`consumer-policy`, `tag`, `category-mapping`, `page-repo` and `reports` take the shapes
their ADRs list, written down with the crate that first reads them. The `kind` field is
always present in the content so a record is self-describing without its key.

## 5. `scatter:v0/logevent`

In the tenant `log`, the instance `log` and the provider logs `log/{provider}` (0011 §3),
keyed by the target's identifier (an entity ID, a keyed entity's surrogate, a page ID, an
actor key, a job ID) or `null`.

```json
{"type": "patrol", "action": "patrol", "time": 1790000000000000,
 "target": {"kind": "record", "partition": 7, "offset": 41, "revid": 900}}
{"type": "job", "action": "start", "time": 1790000000000000, "target": {"kind": "job", "id": 17},
 "params": {"source": "https://librarybase.org/", "version": "librarybase-20260928.json.gz", "mode": "adopt", "graph": "local", "adapter": "scatter-adapter-wikidata 0.0.1", "args": {…}}}
{"type": "job", "action": "finish", "time": 1790000001000000, "target": {"kind": "job", "id": 17},
 "params": {"counts": {"created": 0, "merged": 0, "unchanged": 0, "rejected": 3, "adopted": 349982},
            "hash_mismatches": {"time": 2}, "checkpoint": "librarybase.org/log/local\n350103\nBASE64ROOT\n\n— librarybase.org/log/local SIGNATURE\n"}}
{"type": "job", "action": "fail", "time": …, "target": {"kind": "job", "id": 17}, "params": {"error": "ts-adopt-not-empty"}}
{"type": "job", "action": "revert", "time": …, "target": {"kind": "job", "id": 17}, "params": {"by": 18}}
{"type": "delete", "action": "delete", "time": 1700000000000000, "upstream_logid": 123456,
 "target": {"kind": "entity", "id": "WDQ77"}, "params": {}, "visibility": 0}
```

| Field | Meaning |
|---|---|
| `type`, `action` | MediaWiki's log type and action strings |
| `time` | The event's time at its source |
| `target` | `{kind, …}`: `entity {id}`, `page {id}`, `actor {key}`, `job {id}`, `record {partition, offset, revid?}`, or `null` when hidden or unresolvable (0011 §3) |
| `upstream_logid` | Provider logs only |

The performer, local or upstream, is the attestation's `actor` and nowhere else, so that
hiding the user (0015 §4: erase the attestation part) touches nothing in the content.
| `params` | The typed parameters of 0011 §7, by type; unrecognized upstream parameters are kept under `raw` as given, minus names (0011 §6.2) |
| `visibility` | Provider logs only: the bits as upstream last reported them |

Jobs (0011 §6.3) are `type: job` events keyed by the job ID the instance mints: `start`
holds everything 0002 §8.3 lists (`source`, `version`, `mode`, `graph`, `adapter`,
`args`; an instance job's live in the instance `log`); `finish` holds the counts by
outcome, `hash_mismatches` by value type (0006 §2), and the checkpoint written at the end
as its signed-note text; `fail` holds the error code; `revert` names the reverting job.
The comment part carries the reason the job was started with.

## 6. `scatter:v0/erase`

In any partition, keyed by the erased key when erasing by key, otherwise `null` (0006 §7;
0015 §1).

```json
{"targets": {"offsets": [41, 42]}, "parts": ["attestation"], "reason": "privacy", "authority": "ticket:4411"}
{"targets": {"key": "local:42"}, "reason": "privacy", "authority": "vanish"}
{"targets": {"offsets": [9]}, "parts": ["content"], "reason": "upstream", "authority": "wikidatawiki:logid:123456"}
```

| Field | Meaning |
|---|---|
| `targets` | `{offsets: […]}` in this partition, or `{key}` for every record with that key |
| `parts` | The parts erased: `content`, `comment`, `attestation`, and any a type adds (`text`, `wikitext`, `html`); absent means all |
| `reason` | `legal`, `privacy`, `upstream`, `operational`, or a class `site` configuration adds |
| `authority` | A reference, as text: a ticket, a notice, an upstream log ID, or for an instance act the authority record `{partition}:{offset}` |

The comment part holds the public reason; the attestation names who erased. The reason
class and authority are shown to `ts-viewerasures` only (0016 §6).

## 7. `scatter:v0/upstream-revision`

In `log/{provider}`, keyed by the entity's prefixed ID or a keyed entity's surrogate
(0015 §4).

```json
{"revid": 2148573921, "parent_revid": 2148570000, "time": "2026-09-30T12:00:00Z",
 "size": 48211, "sha1": "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3", "content_model": "wikibase-item",
 "tags": ["wikidata-ui"], "minor": false, "bot": true,
 "observed": {"partition": 12345678901234567890, "offset": 77}}
```

The comment part is the upstream summary; the attestation's `actor` is the upstream actor
key, or `hidden: true`, with `job`. `observed` is present where the revision was also
observed as a `put`. A fourth part, `text`, holds a page revision's wikitext for a fork
(0054 §3); such records live in the tenant `log` keyed by the fork's page ID.

## 8. `scatter:v0/binding`

In `accounts` (private, never exported), keyed by the local actor (0007 §3).

```json
{"issuer": "wikimedia-central", "subject": "7654321"}
{"issuer": "password", "subject": "42"}
```

`subject` is normalized (`mw:CentralAuth:7654321` → `7654321`). Secrets are never here:
a password hash is `private.password`, a key is `private.api_key`.

## 9. Defined with their crates

| Payload type | Partition | Defined with | ADR |
|---|---|---|---|
| `scatter:v0/page` | `pages` | `scatter-pages` | 0008 §4 |
| `scatter:v0/thread` (four parts: + `text`) | `pages` | `scatter-threads` | 0019 §4 |
| `scatter:v0/upload` | `pages`, `files/{repo}` | `scatter-files` | 0039 §2 |
| `scatter:v0/filter`, `scatter:v0/filter-hit` | `log` | `scatter-filter` | 0030 §5 |
| `scatter:v0/expunge` | instance `log` | `scatter-files` | 0039 §10 |
| `scatter:v0/mirrored-page` (five parts: + `wikitext`, `html`) | `pages/{repo}` | `scatter-adapter-mediawiki` | 0053 §5 |

None is on the first milestone's path. Each follows §1–2 and adds its parts after the
three standard ones.

## 10. The `segments` file format

The file backend of `scatter-log` (0013 §1; 0005 §4.2), and the form every partition
takes in an export bundle (0006 §9):

```
{root}/
  {partition, 16 lower-case hex digits}/
    partition.cbor      {"format": 1, "partition": 12345678901234567890, "segment_exponent": 16}
    00000000.seg        a CBOR sequence (RFC 8742) of slots, one per offset
    00000001.seg        …
```

Segment `n` holds offsets `[n·2^k, (n+1)·2^k)` (0006 §5), so every file but the last is
full. A slot is the record `[header, body]` of §2, or, for an offset that compaction
removed, the record's 32-byte Merkle leaf as a byte string, so that the tree over every
offset still folds and offsets are never reused. An append goes to the end of the last
file; an erasure (§6) or a compaction rewrites the one file it touches, through a
temporary file and a rename. Every file is canonical CBOR (§1), so a bundle verifies
with nothing but the crate. Checkpoints and segment manifests are stored beside the
partition by `scatter-integrity`, which defines their names.

## 11. Checkpoints, manifests and export bundles

Checkpoints are [C2SP tlog-checkpoints](https://c2sp.org/tlog-checkpoint) signed as
[C2SP signed notes](https://c2sp.org/signed-note) by the instance key, under the origin
as the key name, with no extension lines (0006 §6):

```
librarybase.org/log/local
42
<base64 root>

— librarybase.org/log/local <base64(key hash[0..4] ‖ Ed25519 signature)>
```

The origin is `{tenant host}/log/{name}` for a tenant partition and
`{farm host}/instance/log/{name}` for an instance partition. A segment manifest in a
`hashed` partition is a checkpoint under `{origin}/segment/{n}` whose size is `2^k` and
whose root is the segment's own tree head; since a compacted offset keeps its leaf (§10;
0006 A14) a manifest is unchanged by compaction. Beside a `segments` partition they live
in `checkpoints/{size, 20 digits}.txt` and `manifests/{n, 8 digits}.txt`; the Postgres
backend keeps them in its own tables (0013 §2).

An **export bundle** (0006 §9) is a directory that is itself a `segments` root:

```
{dir}/
  bundle.cbor      {"format": 1, "created": µs, "partitions": [{"partition": …, "name": "local", "origin": "…"}]}
  keys.seg         the `key:` records of the signing config partition, as a CBOR sequence of records
  {partition}/     the partition's segments, checkpoints and manifests
```

`keys.seg` is the key chain: the first record is the trust anchor, each later one a
rotation whose attestation is signed by the key before it. `verify` (0006 §9) checks
level 1 — gapless offsets, headers that name their partition and offset, the tree head
over every leaf, every checkpoint's origin, signature, root and consistency with the one
before it, every manifest against its segment, and the chain — and level 2 — every body
against its commitment, every erased part against an `erase` record (§6) in the same
partition, every client signature against the actor keys supplied, and every instance
attestation against the key current when the record was appended. The authority extract
of 0040 §8 is not written yet.

## 12. Settled points

Decided 2026-10-02 (James), so that the first instance's bytes are not revisited:

1. **Link records** are keyed by the highest-ranked member under 0004 §4's order (§3.2).
2. **`create` is strictly creation**; `create-or-add` carries the match key and is
   resolved before the append into a `create` or an `add` marked `via` (§3.2; 0002 A19).
3. **No `performer` in a log event's content**: the attestation's `actor` is the one
   place a performer lives, local or upstream, so a hidden user's erasure is one part.
4. **`site` settings are one record per setting key**, `{"kind": "site", "value": …}`;
   a structured setting such as the rate-limit table is one key whose value is the table.
