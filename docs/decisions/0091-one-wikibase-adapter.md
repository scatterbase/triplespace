# 0091. One Wikibase adapter; key maps are configuration

- **Status:** Proposed
- **Date:** 2026-10-26
- **Updated:** 2026-10-30 (A2)
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

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.4, §8.1, §8.3, §8.7.*

### 2. A key map is a list of rules (amends 0009 §9; amends 0036 §5)

*Current text: [05](../architecture/05-providers-and-ingest.md) §4.1, §8.6, §8.9; [23](../architecture/23-configuration-and-registry.md) §4.2.*

### 3. What a mapped item becomes comes from its keyed type (extends 0009 §9)

*Current text: [05](../architecture/05-providers-and-ingest.md) §4.1.*

### 4. Syncing a Wikibase dump (extends 0002 §8.4)

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.4.*

### 5. Crates (amends 0005 §2, §7; amends 0009 §12; amends 0036 §7)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2, §3.1.*

## Alternatives considered

- **Keep `scatter-adapter-internetdomains` and give the generic crate its own key mapping for `OW`.** This leaves two implementations of one rule set (index, conflicts, aliases, drop), which would drift apart.
- **A key-mapping plugin per keyed type.** Rejected: what differs between keyed types is data the keyed-type registry already holds (normalizer, entity type, data type), not behaviour.
- **Keep the flat form as a shorthand for one unconditional rule.** Rejected: two accepted forms for one thing, and the code and checks would carry both.
- **Keep the name `scatter-adapter-wikidata`.** Less churn, but it misnames the crate every Wikibase provider uses, adoption included.

## Consequences

- **A new Wikibase provider needs a registry entry and no code.** The Credit Unions Wiki in `docs/testing/demofarm.md` is the first.
- **Job headers record the adapter that ran** (`adapter_version`). Jobs before this carry `scatter-adapter-internetdomains` or `scatter-adapter-wikidata`. Records are never rewritten, so those names stay in the log as history.
- ~~**The registry runs ahead of the code only where nothing reads it.** The four Wikibase entries' `adapter` fields name `scatter-adapter-wikibase` now, since no code reads the field. internetdomains.wiki's key map keeps its flat form until the adapter reads rules, because `scatter-adapter-internetdomains` and `scatter-providers`' tests read it today. The rule form, and `OW`'s `class_property`, are written as comments beside the entries and listed as a pending allocation in `docs/registry/README.md`.~~ *The code landed with the registry change, so the entries took the rule form at once (A1).*
- ~~**`cargo xtask deps` reads the crate map in chapter 22**, which now names `scatter-adapter-wikibase`. It fails from this change until the crate is renamed, so the two land together.~~ *The crate was renamed in the same change (A1).*
- **ADRs that name `scatter-adapter-wikidata` or `scatter-adapter-internetdomains`** read as naming `scatter-adapter-wikibase` ([22](../architecture/22-crates-and-stack.md) §2.1).

## Open questions

- **Q1. Subclasses in a class rule.** A rule matches an item's direct class only. A wiki whose mapped items are typed by subclasses of the class would need closure along a subclass property, configured like the closure properties of [0070](0070-shallow-entity-mirroring.md) §2.2. Nothing registered needs it yet.

## References

- `crates/scatter-wikibase-changeset/src/adapter.rs`, `crates/scatter-adapter-internetdomains/src/lib.rs`, `crates/scatter-adapter-wikidata/src/`, `crates/triplespace-cli/src/sync.rs`, as of `10dc4d8`
- `docs/registry/providers.toml`, `docs/registry/keyed-types.toml`
- `docs/testing/demofarm.md`

## Amendment log

### A1. The registry and the code changed together

- **Date:** 2026-10-26
- **Source:** Direct: James, go-ahead to implement, 2026-10-26
- **Change:** corrects Consequences
- **Summary:** The implementation renamed the crate, retired `scatter-adapter-internetdomains` and moved internetdomains.wiki's key map to the rule form in one change, so the two consequences that described a gap between the registry, the crate map and the code no longer hold. The decision is unchanged.

Replaced text (Consequences, as it stood):

> - **The registry runs ahead of the code only where nothing reads it.** The four Wikibase entries' `adapter` fields name `scatter-adapter-wikibase` now, since no code reads the field. internetdomains.wiki's key map keeps its flat form until the adapter reads rules, because `scatter-adapter-internetdomains` and `scatter-providers`' tests read it today. The rule form, and `OW`'s `class_property`, are written as comments beside the entries and listed as a pending allocation in `docs/registry/README.md`.
> - **`cargo xtask deps` reads the crate map in chapter 22**, which now names `scatter-adapter-wikibase`. It fails from this change until the crate is renamed, so the two land together.

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§5
- **Summary:** The Decision's current text now lives in the architecture chapters [05](../architecture/05-providers-and-ingest.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
