# Actor-side payloads

The content parts of the record payload types that `scatter-actors` defines. The ADRs name
the types and what each carries (0007 §4, §7; 0015 §1; 0016 §3–4; 0023 §3); the field
names and shapes are fixed here, by the crate's serde types, and are what `scatter-log`
encodes as CBOR (0006 §2: the JSON structure, nothing else). Times are integers,
microseconds since the Unix epoch, the unit of a header's `appended_at` (0013 §5).
Optional fields are left out when absent, never written as `null`.

| Payload type | Partition | Header key | Content |
|---|---|---|---|
| `scatter:v0/actor` | `actors`, `actors/{provider}` | The actor key | [`ActorRecord`](#actor) |
| `scatter:v0/link-account` | `actors` | The local actor key | [`AccountLink`](#link-account) |
| `scatter:v0/membership` | `actors`, `actors/{farm}` | The actor key | [`Membership`](#membership) |
| `scatter:v0/block` | `actors`, `actors/{farm}` | The actor key | [`Block`](#block) |
| `scatter:v0/key` | `actors` | The actor key | [`KeyRecord`](#key) |
| `scatter:v0/acl` | `config`, `log`, instance `log` | The target key `acl:{kind}:{id}` | [`Acl`](#acl), or `null` to retire |

An actor key is `{issuer}:{subject}`: `local:42`, `librarybase:42` outside the tenant,
`wikidatawiki:12345`, `instance:scatter`. It never contains a name (0007 §1).

## actor

```json
{"kind": "registered", "name": "Example", "status": "active"}
{"kind": "bot", "name": "Example-bot", "status": "pending", "operator": "local:42"}
{"kind": "anonymous", "status": "active", "raw": "192.0.2.1"}
{"kind": "federated", "status": "active", "raw": "https://mastodon.example/users/x"}
```

| Field | Values |
|---|---|
| `kind` | `registered`, `temporary`, `bot`, `anonymous`, `imported`, `provider`, `federated` |
| `name` | The current name; absent for a surrogate |
| `status` | `active` (default), `renamed`, `vanished`, `hidden`, `retired`, `pending` |
| `raw` | Surrogates only: the IP, the `prefix>Name`, or the remote actor IRI. Erasable with the record |
| `operator` | `bot` only, required: the actor key of the primary account of the same tenant |

Invariants (0007 §4–5; 0024 §1; 0025 §3): a `bot` has an operator and nothing else does;
the operator is a local account and not the subsidiary itself; `anonymous`, `imported` and
`federated` carry `raw` and no `name`; `retired` and `pending` are subsidiary statuses.
`hidden` is set by the projection from an `actor` ACL (0023 §5), not written by hand.

## link-account

```json
{"foreign": "wikidatawiki:12345"}
```

Only the holder creates one, having proved control; a foreign account links to at most
one local account; removal is an `erase` (0007 §7). The farm-signup link names the farm
account (0028 §2).

## membership

```json
{"group": "sysop", "action": "add"}
{"group": "bot", "action": "add", "expires": 1790000000000000}
{"group": "sysop", "action": "remove"}
```

The groups an actor is in at a moment is the fold of its records in log order up to that
moment: an addition holds until removed or expired. Projects as `rights/rights`.

## block

```json
{"action": "block", "removes": ["createpage", "edit"], "expires": 1790000000000000}
{"action": "block", "removes": "all-but-read"}
{"action": "unblock"}
```

`removes` is a list of permissions or the marker `"all-but-read"`, which resolves against
the registry's full permission list when evaluated, so a permission added later is
covered. The latest record wins; an expired block removes nothing. The instance's blocks
(a global filter's, 0040 §5) are a second layer under the same key that a tenant unblock
does not lift. Projects as `block/block`, `block/reblock`, `block/unblock`.

## key

```json
{"key_id": "…52 z-base-32 characters…", "alg": "ed25519", "public_key": <32 bytes>, "valid_from": 1790000000000000}
{"key_id": "…", "alg": "ed25519", "public_key": <32 bytes>, "valid_from": 1790000000000000, "valid_until": 1821536000000000}
{"key_id": "…", "alg": "ed25519", "public_key": <32 bytes>, "valid_from": 1790000000000000, "revoked": true}
```

`key_id` is the z-base-32 of the SHA-256 of the public key. `public_key` is a CBOR byte
string (a JSON array of numbers in a JSON rendering). A later record for the same `key_id`
rotates or revokes it; a revoked record keeps the key so that older signatures stay
checkable. A submitted signature is over `H(0x05 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment))`
(0015 §1).

## acl

```json
{"target": "acl:page:7", "restrictions": {"edit": {"group": "sysop", "expires": 1790000000000000}, "move": {"group": "sysop", "expires": 1790000000000000}}}
{"target": "acl:page:7", "restrictions": {"read": {"group": "sysop"}}, "read_kind": "moderation", "talk": true}
{"target": "acl:record:3:99", "restrictions": {"read": {"group": "suppress"}}, "read_kind": "moderation", "parts": ["comment"]}
{"target": "acl:page:12", "restrictions": {"createpage": {"group": "sysop"}}, "reserved": {"namespace": 0, "title": "Example"}}
{"target": "acl:set:8", "restrictions": {"read": {"group": "board"}}, "read_kind": "confidential", "name": "Board minutes", "members": [{"kind": "page", "id": 1}, {"kind": "entity", "id": "Q5"}]}
{"target": "acl:tenant:librarybase", "restrictions": {"read": {"group": "user"}}, "read_kind": "confidential"}
{"target": "acl:blob:123", "restrictions": {"read": {}, "upload": {}}, "read_kind": "moderation"}
```

| Field | Meaning |
|---|---|
| `target` | `acl:{kind}:{id}`; kinds `graph`, `namespace`, `page`, `entity`, `statement`, `property`, `record` (`{partition}:{offset}`), `actor`, `blob`, `tenant`, `set` |
| `restrictions` | Per permission: `group`, the group whose members may still perform it, absent only for a takedown (restricted to nobody); `expires` |
| `read_kind` | `moderation` or `confidential`; required when `read` is restricted, forbidden otherwise (0056 §2). Set by the write path from the right that authorized the record |
| `parts` | `record` targets: the parts hidden; empty means the whole record |
| `reserved` | Create-protection: the namespace and title reserved under the page ID |
| `talk` | A whole-page or whole-entity deletion that took the talk page with it |
| `name`, `members` | `set` targets: the set's name and members (`page` or `entity`, by ID) |

A record whose content is `null` retires the ACL. Where the record goes (0023 §3): `graph`
and `tenant` targets to the tenant `config`, `blob` to the instance `log`, everything else
to the tenant `log`. A confidential restriction cannot be put on a `graph`, `record`,
`actor` or `blob` target, and may name only a group its setter belongs to (0056 §4).
