# 0091. One Wikibase adapter; key maps are configuration

- **Status:** Proposed
- **Date:** 2026-10-26
- **Author:** James Hare / Claude Opus
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0009](0009-keyed-entity-types-and-domain.md), [0036](0036-openstreetmap-providers.md)
- **Uses:** [0003](0003-statement-ui.md), [0017](0017-entity-id-grammar.md), [0035](0035-adopting-a-wikibase.md), [0048](0048-notation.md), [0070](0070-shallow-entity-mirroring.md), [0078](0078-entity-sources.md)

## Context

Two crates read Wikibases. [0009](0009-keyed-entity-types-and-domain.md) §12 made `scatter-adapter-internetdomains` for internetdomains.wiki. That wiki's domain items are **key-mapped**: an item with one valid domain name in `P1` is stored as `domain:{key}`, not `XDQ{n}` (0009 §9). Every other Wikibase provider names `scatter-adapter-wikidata`: Wikidata, Librarybase and the OpenStreetMap wiki, and also adoption ([0035](0035-adopting-a-wikibase.md) §3) and entity sources ([0078](0078-entity-sources.md) §4). The registry describes that crate as "any MediaWiki+Wikibase; the adapter is parameterised by this entry".

[0036](0036-openstreetmap-providers.md) then gave the OpenStreetMap wiki (`OW`) key maps of its own. Two classes of item are mapped onto notations, and the crate map's note on 0036 says `OW` "is `scatter-adapter-wikidata` parameterised by the provider entry, with the key-mapping capability that `scatter-adapter-internetdomains` needs as well". The one capability so had two homes. The registry also had two forms for it:
- internetdomains: flat `identity_property` and `key_type` fields on `adapter_config`.
- `OW`: a list of `[[provider.adapter_config.key_map]]` rules, each naming a class.

The code, as of `main` at `10dc4d8`:
- **`scatter-wikibase-changeset`** holds the `Adapter` trait, `WikibaseAdapter` (ID rewriting for any registered Wikibase provider, adoption mode and entity-source prefixes) and the `KeyMap` trait.
- **`scatter-adapter-wikidata`** reads JSON and XML dumps and turns entities into mirror states (`mirror_entity`). It also types an XML dump's snaks from the dump's own property pages, and holds adoption.
- **`scatter-adapter-internetdomains`** holds:
  - the domain normalizer call;
  - the two-pass index, with key conflicts and merge aliases (0009 A13);
  - dropping the identity property on a mapped item;
  - retyping a value that points at a mapped item as `wikibase-domain`.

  Only the first is about domains.
- **`triplespace sync`** refuses every provider but internetdomains. Mirroring another Wikibase therefore needs code, even when its registry entry is complete. `docs/testing/demofarm.md` ran into this with the Credit Unions Wiki.

### Direction

James's question, from the design discussion of 2026-10-26:

- **"What would it take for a generic Wikibase adapter that can be used on any Wikibase? Could internetdomains then be made to use that crate?"**

His choices in the same discussion:
- a new ADR rather than direct amendments;
- renaming `scatter-adapter-wikidata` to `scatter-adapter-wikibase`;
- one `key_map` rule list as the only form of a key map.

## Decision

### 1. One adapter for every Wikibase provider (amends 0002 §4, §8.4; amends 0036 §1)

**Every provider whose upstream is a MediaWiki+Wikibase is read by `scatter-adapter-wikibase`.** This covers Wikidata, Librarybase, internetdomains.wiki, the OpenStreetMap wiki and any Wikibase registered later. The crate is parameterised by the provider's registry entry:
- its code and types, for ID rewriting;
- `revision_ids` and `issuer`;
- `api` and `article_path`;
- `adapter_config`.

**Registering a Wikibase provider is a registry entry, not code.** Behaviour that only Wikidata has is switched on by what the entry declares, never by the slug. That covers Commons' `M` type, lexemes, EntitySchema from namespace 640 and EventStreams.

**Adapters are one per kind of upstream, not one per provider.** A provider whose upstream is not a Wikibase keeps an adapter of its own: OpenAlex, MusicBrainz, OpenStreetMap's map data and GDELT. A Triplespace instance keeps `scatter-adapter-triplespace`. The `adapter` field stays one per provider entry ([0002](0002-source-graphs-and-mass-ingest.md) A28); it names which of these crates reads the provider.

### 2. A key map is a list of rules (amends 0009 §9; amends 0036 §5)

A key-mapped Wikibase provider's `adapter_config` holds one `[[provider.adapter_config.key_map]]` rule per class of mapped item:

| Field | Meaning |
|---|---|
| `identity_property` | Required. The upstream property, in the wiki's own form (`P1`), whose value is the key |
| `key_type` | Required. The keyed type the item is mapped onto (`keyed-types.toml`) |
| `key_scheme` | For `key_type = "notation"`, the scheme ([0048](0048-notation.md) §6) |
| `class` | Optional. An upstream item ID. The rule then applies only to items that have it as a value of `class_property`. Without it, the rule applies to every item |

Two fields sit on `adapter_config` itself, beside the rules:

| Field | Meaning |
|---|---|
| `class_property` | The upstream property that states an item's class, the wiki's "instance of". Required when any rule names a `class` |
| `identity_property_on_mapped` | `drop` (the default) or `keep`, for every rule: whether the identity statement stays in a mapped item's mirrored state |

**The rule list is the only form.** internetdomains.wiki's key map is one rule with no class: `identity_property = "P1"`, `key_type = "domain"`. The OpenStreetMap wiki's key map is two rules, one for `Q7` (key) and one for `Q2` (tag), with its "instance of" property as `class_property`. The flat `identity_property` and `key_type` fields on `adapter_config` are retired.

**The identity and class properties are named by their upstream IDs, not by role.** They are the provider's own properties, fixed by the registry entry and read before any ID is rewritten. A role binding ([0003](0003-statement-ui.md) §7) would add a lookup with nothing to choose between. Roles stay the way an adapter names the properties it maps *onto* ([0036](0036-openstreetmap-providers.md) A6).

**Which rule applies to an item.** An item is in a rule's scope when the rule names no class, or when the item has the rule's `class` as a direct value of `class_property`. Subclasses are not followed (Q1). Then:

- **No rule in scope gives the item an identity value.** It stays an ordinary foreign item.
  - It is reported as a reject when a rule in scope names a class: an item of a mapped class without its identity value is an upstream data problem.
  - It is not reported otherwise, as for internetdomains.wiki's registrars.
- **One rule gives it identity values.** That rule maps it as [0009](0009-keyed-entity-types-and-domain.md) §9 provides: one valid value maps it; an invalid value, or more than one value, is a reject, and the item stays foreign.
- **Two or more rules give it identity values.** It is a reject, reported as matched by several key-map rules, and it stays foreign.

Key conflicts, merge aliases and upstream changes are as [05](../architecture/05-providers-and-ingest.md) §4.2 has them, across all of a provider's rules. A key is a keyed ID, `{type}:{key}`, so two rules that map onto different keyed types never collide.

### 3. What a mapped item becomes comes from its keyed type (extends 0009 §9)

The adapter hard-codes no keyed type. A rule's `key_type` supplies everything that differs between them:
- **The normalizer**, from `scatter-normalize`; for a notation, its scheme's.
- **The mapped state's entity type.**
- **The data type of a value that points at a mapped item:** `wikibase-domain`, `wikibase-keyword` or `wikibase-notation`.
- **The IRI.**

A rule naming a keyed type, or a notation scheme, that the build does not know is a configuration error when the adapter is built, not a reject per item. Everything else [0009](0009-keyed-entity-types-and-domain.md) §9 and [05](../architecture/05-providers-and-ingest.md) §4 say of a Domain mapped from internetdomains.wiki holds for every keyed type mapped this way.

### 4. Syncing a Wikibase dump (extends 0002 §8.4)

**`triplespace sync` chooses the adapter by the provider's `adapter` field**, not by its slug.

`scatter-adapter-wikibase` reads a JSON or an XML dump. It makes a first pass over the whole dump in either of two cases:
- **The entry has key-map rules.** The index needs every item before any value can be rewritten to a key.
- **The dump is XML.** Its snaks carry no `datatype`, so the pass collects the types from the dump's own property pages, as adoption does ([0035](0035-adopting-a-wikibase.md) §3). It also collects the redirects.

A JSON dump of a provider without key maps is read once. That is what lets a full Wikidata dump stream in one pass. Rejects and unreadable entities are reported as they are now.

### 5. Crates (amends 0005 §2, §7; amends 0009 §12; amends 0036 §7)

| Crate | Change |
|---|---|
| `scatter-adapter-wikibase` | **Renamed** from `scatter-adapter-wikidata`; everything that crate held stays. It gains the key-mapped adapter: the rule list and its scope (§2), the two-pass index with key conflicts and merge aliases, the identity drop, and values retyped by keyed type (§3). It depends on `scatter-normalize` for the keyed types' normalizers |
| `scatter-adapter-internetdomains` | **Retired.** Its code moves into `scatter-adapter-wikibase` (§2–§3) |
| `scatter-wikibase-changeset` | Unchanged: the `Adapter` and `KeyMap` traits and `WikibaseAdapter` stay where they are |
| `triplespace-cli` | `sync` dispatches on `adapter` (§4) |

## Alternatives considered

- **Keep `scatter-adapter-internetdomains` and give the generic crate its own key mapping for `OW`.** This leaves two implementations of one rule set (index, conflicts, aliases, drop), which would drift apart.
- **A key-mapping plugin per keyed type.** Rejected: what differs between keyed types is data the keyed-type registry already holds (normalizer, entity type, data type), not behaviour.
- **Keep the flat form as a shorthand for one unconditional rule.** Rejected: two accepted forms for one thing, and the code and checks would carry both.
- **Keep the name `scatter-adapter-wikidata`.** Less churn, but it misnames the crate every Wikibase provider uses, adoption included.

## Consequences

- **A new Wikibase provider needs a registry entry and no code.** The Credit Unions Wiki in `docs/testing/demofarm.md` is the first.
- **Job headers record the adapter that ran** (`adapter_version`). Jobs before this carry `scatter-adapter-internetdomains` or `scatter-adapter-wikidata`. Records are never rewritten, so those names stay in the log as history.
- **The registry runs ahead of the code only where nothing reads it.** The four Wikibase entries' `adapter` fields name `scatter-adapter-wikibase` now, since no code reads the field. internetdomains.wiki's key map keeps its flat form until the adapter reads rules, because `scatter-adapter-internetdomains` and `scatter-providers`' tests read it today. The rule form, and `OW`'s `class_property`, are written as comments beside the entries and listed as a pending allocation in `docs/registry/README.md`.
- **`cargo xtask deps` reads the crate map in chapter 22**, which now names `scatter-adapter-wikibase`. It fails from this change until the crate is renamed, so the two land together.
- **ADRs that name `scatter-adapter-wikidata` or `scatter-adapter-internetdomains`** read as naming `scatter-adapter-wikibase` ([22](../architecture/22-crates-and-stack.md) §2.1).

## Open questions

- **Q1. Subclasses in a class rule.** A rule matches an item's direct class only. A wiki whose mapped items are typed by subclasses of the class would need closure along a subclass property, configured like the closure properties of [0070](0070-shallow-entity-mirroring.md) §2.2. Nothing registered needs it yet.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §4 | §1 | amends | 0002 A40 |
| [0002](0002-source-graphs-and-mass-ingest.md) §8.4 | §1, §4 | amends | 0002 A40 |
| [0005](0005-crate-organization.md) §2 | §5 | amends | 0005 A105 |
| [0005](0005-crate-organization.md) §7 | §5 | amends | 0005 A105 |
| [0009](0009-keyed-entity-types-and-domain.md) §9 | §2, §3 | amends | 0009 A15 |
| [0009](0009-keyed-entity-types-and-domain.md) §12 | §5 | amends | 0009 A15 |
| [0036](0036-openstreetmap-providers.md) §1 | §1 | amends | 0036 A7 |
| [0036](0036-openstreetmap-providers.md) §5 | §2 | amends | 0036 A7 |
| [0036](0036-openstreetmap-providers.md) §7 | §2 | amends | 0036 A7 |

## References

- `crates/scatter-wikibase-changeset/src/adapter.rs`, `crates/scatter-adapter-internetdomains/src/lib.rs`, `crates/scatter-adapter-wikidata/src/`, `crates/triplespace-cli/src/sync.rs`, as of `10dc4d8`
- `docs/registry/providers.toml`, `docs/registry/keyed-types.toml`
- `docs/testing/demofarm.md`
