# 0036. OpenStreetMap: map data, the wiki vocabulary and the `osm` notation scheme

- **Status:** Proposed
- **Date:** 2026-09-28
- **Updated:** 2026-10-09 (A6)
- **Author:** James Hare / Claude Sonnet
- **Changes:** [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0017](0017-entity-id-grammar.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0015](0015-record-format-and-partition-registry.md), [0031](0031-property-constraints.md), [0048](0048-notation.md)
- **Chapters:** [04](../architecture/04-entities-and-identifiers.md), [05](../architecture/05-providers-and-ingest.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md)

## Context

OpenStreetMap (OSM) has three entity types (node, way, relation), each with its own ID space, and a flat set of free-form string tags on every object. It has no property registry: `amenity`, `addr:street` and a key invented yesterday are equally valid. Where a vocabulary exists, it is community documentation on the OSM wiki, and the wiki runs a Wikibase for it (§5).

Two different things are therefore called "OSM data", and they have different sources, licences, ID schemes and revision models:

- **Map data**: nodes, ways and relations, from the planet file, replication diffs and the API.
- **The vocabulary**: what keys and tags mean, which are approved, what they imply or exclude, and which Wikidata concept each corresponds to. It lives only in the wiki's Wikibase.

The provider registry ([0015](0015-record-format-and-partition-registry.md) §5) gives every provider one API, one adapter, one issuer, one revision-ID range and one mirror graph. Neither source can be mirrored as a type of the other without breaking that.

Two existing mechanisms fit the rest. Keyed entity types ([0009](0009-keyed-entity-types-and-domain.md) §1, [0017](0017-entity-id-grammar.md) §5) give an entity an ID that is its natural key, which is what a tag is. Key-mapped providers ([0009](0009-keyed-entity-types-and-domain.md) §9) write an upstream item under its key, which is what the wiki's `P16` and `P19` are for.

## Decision

### 1. Two providers

*Changed by A6.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §8.1, §8.7.*

### 2. `OS`: map objects

*Changed by A4.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §8.7.*

### 3. The `osm` scheme: keys and tags as notations

*Changed by A2.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.8.*

### 4. Tags on map objects

*Changed by A2, A6.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §8.8.*

### 5. `OW`: the vocabulary

*Changed by A2.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §8.9.*

### 6. Tags are not concepts

*Changed by A2.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.9, §4.1.*

### 7. Registry, namespaces and crates

*Changed by A1, A2.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2; [23](../architecture/23-configuration-and-registry.md) §4.2, §4.3.*

## Consequences

- **The wiki's modelling is reused.** Triplespace stores no key-and-tag taxonomy of its own. Descriptions, status, applicability, `implies` and Wikidata links arrive as mirrored data.
- **Both providers can be mirrored independently.** A tenant may want the vocabulary without the map, since it is small, and it is about a few hundred `OWQ` and `OWP` entities plus the mapped keys and tags. It needs no map extract.
- **Undocumented tags still work.** Case 3 keeps every tag, and case 2 needs the wiki only for enrichment.
- **A tag-to-concept path exists** without merging identities: tag entity, `P7`, Wikidata item.
- **Case 2 depends on the `OW` mirror,** so the same map data projects differently with and without it. Projection is from the log, so adding the mirror later reprojects and needs no reload.
- **Two more registry numbers are spent,** and one more namespace pair (216/217). 218 and 219 remain in the Triplespace range.

## Open questions

- **Q1. Licences.** OSM map data is ODbL 1.0, with attribution and share-alike duties that may reach derived databases; the wiki's data-item licence has not been checked. What a mirror partition must carry (attribution, licence per graph), and whether an ODbL mirror can sit beside GPL and CC0 material ([0005](0005-crate-organization.md) §6), must be settled before the first load.
- **Q2. Scale.** The planet has billions of nodes. Whether an instance mirrors a full planet, an extract (a country, a bounding box, a tag filter) or resolves objects on demand is unspecified. The bootstrap mode of [0013](0013-postgres-storage.md) §9 would be needed at planet scale.
- **Q3. `wikidata=` on map objects.** Whether an object's `wikidata` tag is a tier-2 link that may join an identity cluster ([0004](0004-identity-clusters-and-equivalence.md) §6). It is sometimes wrong, and the `brand:wikidata` and `operator:wikidata` families are not identity links at all.
- **Q4. Roles.** The exact names and value types of `osm-tag`, `osm-tag-text` and `osm-key`, and the default key map, are implementation-time decisions, since no Triplespace property numbers exist yet.
- **Q5. Constraints.** Whether `P45` and `P44` should drive constraint checks on map objects ([0031](0031-property-constraints.md)), and how status (approved, discouraged) should weigh in.
- **Q6. Key and tag identity.** Whether a key and its tags should be two schemes, which is a registry change ([0048](0048-notation.md) §6). The concept base URI of the wiki and the DEPRECATED marker's representation are to be confirmed against a live dump.
- **Q7. Editing.** Whether local edits to a mirrored tag entity's descriptions overlay the wiki's ([0002](0002-source-graphs-and-mass-ingest.md) §7), and whether the wiki's items can be reached as `OWQ` when they are mapped.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0003](0003-statement-ui.md) §7 | §4 | extends | 0003 A5 |
| [0004](0004-identity-clusters-and-equivalence.md) §1 | §6 | extends | 0004 A7 |
| [0005](0005-crate-organization.md) §2 | §7 | extends | 0005 A35 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §3 | extends | 0008 A8 |
| [0009](0009-keyed-entity-types-and-domain.md) §9 | §5 | extends | 0009 A4 |
| [0017](0017-entity-id-grammar.md) §5 | §3 | extends | 0017 A3 |

## References

- [0009 — Keyed entity types and Domain](0009-keyed-entity-types-and-domain.md) §9
- [0017 — Entity ID grammar](0017-entity-id-grammar.md) §5
- [OpenStreetMap wiki: Data items](https://wiki.openstreetmap.org/wiki/Data_items)
- [OpenStreetMap wiki: Elements](https://wiki.openstreetmap.org/wiki/Elements)
- [OpenStreetMap copyright and licence](https://www.openstreetmap.org/copyright)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-28
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** extends §7
- **Summary:** 0005 §2 is the one crate table CI checks, and carries `scatter-adapter-openstreetmap` (0005 A35); §7 keeps its registry notes and names 0005 §2 for the crate.

### A2. Tags become the osm scheme of Notation

- **Date:** 2026-10-01
- **Source:** [0048](0048-notation.md) §1, §3–4, §6–8
- **Change:** amends §3, §4, §5, §6, §7; retitles
- **Summary:** By section:
  - §3: `osm-tag` is no longer a keyed type of its own. OSM keys and tags are notations in the `osm` scheme: `notation:osm:amenity` and `notation:osm:amenity=cafe`, titled `Notation:osm:amenity=cafe` in the `Notation` namespace (216/217, renamed from `OSM`), with data type `wikibase-notation` and content model `triplespace-notation`. The grammar above and the absence of a normalizer are unchanged; they are the `osm` scheme's `osm-tag` grammar and `exact` normalizer.
  - §4: The roles are unchanged. Their values are `notation:osm:k=v` and `notation:osm:k`, and the properties bound to `osm-tag` and `osm-key` carry the `notation-scheme` statement `osm`.
  - §5: Mapped items become `notation` entities in the `osm` scheme; the key map names `key_type = "notation"` and `key_scheme = "osm"`, and values pointing at mapped items become `wikibase-notation` values.
  - §6: The rule now belongs to the `notation` type: no notation of any scheme joins an identity cluster.
  - §7: `keyed-types.toml` gains `notation` instead of `osm-tag`, and the new `notation-schemes.toml` holds `osm`. 216/217 are `Notation` and `Notation talk`. `scatter-normalize` keeps the `osm-tag` grammar as the `osm` scheme's grammar, and `triplespace-titles` gains the `Notation` namespace instead of `OSM`.

Replaced text (§3):

> One keyed type covers both a key (`amenity`) and a tag (`amenity=cafe`), as Wikidata's P1282 is "OpenStreetMap tag or key". Keys never contain `=`, so the two forms cannot collide, and the wiki's `Key:` and `Tag:` page titles are the same strings.
>
> | | |
> |---|---|
> | Type name, ID prefix | `osm-tag`, `osmtag:` (`osmtag:amenity`, `osmtag:amenity=cafe`) |
> | Namespace | `OSM`, 216, with `OSM talk` at 217 ([0008](0008-namespaces-and-document-pages.md) §2) |
> | Data type | `wikibase-osm-tag`, value type `wikibase-entityid` |
> | IRI | `https://scatter.red/osm-tag/{key}`, content-derived and shared by every instance |
> | Grammar | A key of 1 to 255 characters, or a key, `=` and a value of 1 to 255 characters. No control characters, no `=` in the key, no leading or trailing whitespace, Unicode NFC |
> | Normalizer | None. The exact string, case-sensitive. Input that is not NFC is not a valid key |
> | Display | The string itself as the `mul` label. Descriptions and aliases are stored and reconciled like an item's |
> | Clusters | Never (§6) |
>
> **The keyword normalizer is not used.** Keyword ([0017](0017-entity-id-grammar.md) §5) casefolds and turns punctuation into hyphens, so `fast_food`, `fast-food` and `fast:food` would become one entity, and `Name` would merge with `name`. OSM tags are case-sensitive exact strings and those forms are different tags.
>
> **Grammar over policy.** The grammar admits any valid OSM string. Which strings become entities is decided by the adapters (§4, §5), not the grammar.
>
> One type shares one namespace, which leaves 218 to 219 free. Splitting keys from tags into two types is a registry change, not a design change, if a use for the difference appears.

Replaced text (§4):

> 2. **A documented tag.** If an `osm-tag` entity for exactly `k=v` exists in the `OW` mirror, or the wiki says the key takes well-known values, the object gets a statement with role `osm-tag` and value `osmtag:k=v`.
> 3. **Everything else.** The object gets a statement with role `osm-tag-text`, string value `v`, and a qualifier with role `osm-key` and value `osmtag:k`.

Replaced text (§5):

> The wiki's Wikibase models keys and tags as items and describes them with about fifty properties. The adapter maps two classes to `osm-tag`:
>
> Values pointing at such items become `wikibase-osm-tag` values.

Replaced text (§6):

> `osm-tag` has `clusters = false`, as `keyword` does ([0017](0017-entity-id-grammar.md) §5), for the same reason: `osmtag:amenity=cafe` is a tag, and a Wikidata item about cafés is the concept. `same-as` with an `osm-tag` on either side is rejected at write time,

Replaced text (§7):

> - `providers.toml`: `OS` (number 6) and `OW` (number 7). `issuers.toml`: `openstreetmap` and `osmwiki`. `keyed-types.toml`: `osm-tag`. `namespaces.toml`: `OSM` 216 and `OSM talk` 217. The registry README lists 216/217 in the mediawiki.org registration still to be filed.
> - **New crate:** `scatter-adapter-openstreetmap` (layer 3), depending on `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers` and `scatter-normalize`. `OW` needs none: it is `scatter-adapter-wikidata` parameterised by the provider entry, with the key-mapping capability that `scatter-adapter-internetdomains` needs as well.
> - `scatter-normalize` gains the `osm-tag` grammar. `triplespace-titles` gains the `OSM` namespace.

### A3. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §3–7
- **Summary:** A1–A2 were folded into the Decision; with A2 the title changed from "the `osm-tag` keyed type" to "the `osm` notation scheme", and §3's heading with it. The open questions were numbered. No decision changed. Before this, A2 was five blockquotes and a note in Q6. The file before conversion is commit `0b26a3a`.

### A4. Upstream deletion is the keyed-style clear, by registry flag

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §2
- **Summary:** §2 stands in substance: a provider may opt into the keyed-style clear, a `put` of the empty state, in place of a tombstone for an upstream deletion, by the registry flag `deletion = clear` in `providers.toml`, which OSM sets. [0002](0002-source-graphs-and-mass-ingest.md) §5 gains that option, so §2 now cites 0002 §5 for the clear rather than [0009](0009-keyed-entity-types-and-domain.md) §9, whose rule is for keyed types. (PENDING E9)

Replaced text (§2):

> - Each node, way and relation is a foreign item under its type code. The IDs are bare digits (`upstream_prefix = ""`, `id_grammar` `digits`), and the type code is what separates node 1 from way 1. Deleted IDs are never reused upstream; a deletion clears the mirror's contribution as in [0009](0009-keyed-entity-types-and-domain.md) §9.

### A5. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§7
- **Summary:** The Decision's current text now lives in the architecture chapters [04](../architecture/04-entities-and-identifiers.md), [05](../architecture/05-providers-and-ingest.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A6. The key map binds roles in the instance record, never a tenant's properties

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §4; extends §1
- **Summary:** Adapters never map onto a tenant's properties. The OSM key map still names properties by role, but the bindings it reads are the instance-scope `role` record's ([0082](0082-source-form-and-the-shared-view.md) §5.2), never a tenant's, and they name registry-provider properties (`WDP…`, `OWP…`) or properties of OpenStreetMap's own **property type**: a `[[provider.type]]` row with `entity_type = "property"` on a provider that publishes no properties of its own, its letter allocated when the adapter is built, whose properties the adapter mints under deterministic IDs with the mapping versioned with the adapter (§1, in [05](../architecture/05-providers-and-ingest.md) §8.1). Whatever a mapped key becomes is therefore the same property on every instance, and "no per-instance property numbers exist" holds by construction rather than by the case-3 fallback alone. The ledger named §3 (the `osm` scheme), whose chapter text did not change; the fold is in [05](../architecture/05-providers-and-ingest.md) §8.8, which holds §4. (REVIEW G26)

Replaced text ([05](../architecture/05-providers-and-ingest.md) §8.8, as it stood):

> The key map names properties **by role** ([0003](../decisions/0003-statement-ui.md) §7), like the identity property in §4.1 and OpenAlex's field mapping, so it never depends on a property's number.

> Case 3 is the fallback until they do, and it means no property is minted per key and no per-instance property numbers exist.
