# 0036. OpenStreetMap: map data, the wiki vocabulary and the `osm` notation scheme

- **Status:** Proposed
- **Date:** 2026-09-28
- **Updated:** 2026-10-01 (A3)
- **Author:** James Hare / Claude Sonnet
- **Changes:** [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0017](0017-entity-id-grammar.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0015](0015-record-format-and-partition-registry.md), [0031](0031-property-constraints.md), [0048](0048-notation.md)

## Context

OpenStreetMap (OSM) has three entity types (node, way, relation), each with its own ID space, and a flat set of free-form string tags on every object. It has no property registry: `amenity`, `addr:street` and a key invented yesterday are equally valid. Where a vocabulary exists, it is community documentation on the OSM wiki, and the wiki runs a Wikibase for it (§5).

Two different things are therefore called "OSM data", and they have different sources, licences, ID schemes and revision models:

- **Map data**: nodes, ways and relations, from the planet file, replication diffs and the API.
- **The vocabulary**: what keys and tags mean, which are approved, what they imply or exclude, and which Wikidata concept each corresponds to. It lives only in the wiki's Wikibase.

The provider registry ([0015](0015-record-format-and-partition-registry.md) §5) gives every provider one API, one adapter, one issuer, one revision-ID range and one mirror graph. Neither source can be mirrored as a type of the other without breaking that.

Two existing mechanisms fit the rest. Keyed entity types ([0009](0009-keyed-entity-types-and-domain.md) §1, [0017](0017-entity-id-grammar.md) §5) give an entity an ID that is its natural key, which is what a tag is. Key-mapped providers ([0009](0009-keyed-entity-types-and-domain.md) §9) write an upstream item under its key, which is what the wiki's `P16` and `P19` are for.

## Decision

### 1. Two providers

| | Map data | Vocabulary |
|---|---|---|
| Code, slug, number | `OS`, `openstreetmap`, 6 | `OW`, `osmwiki`, 7 |
| Types | `N` node, `W` way, `R` relation | `Q` item, `P` property |
| IDs | `OSN123`, `OSW123`, `OSR123` | `OWQ7`, `OWP16` |
| Source | planet, replication diffs, API | `wiki.openstreetmap.org` Wikibase |
| Adapter | `scatter-adapter-openstreetmap` (new) | `scatter-adapter-wikidata`, parameterised, with key-mapping (0009 §9) |
| `revision_ids` | false | true |
| Issuer | `openstreetmap` (OSM user IDs) | `osmwiki` (wiki user IDs) |

Both are in `providers.toml` and `issuers.toml`. Five types under `OS` (adding `Q` and `P` for the wiki) was rejected: `revision_ids` is one flag per provider, the revision-ID range `provider_number << 40 | n` ([0015](0015-record-format-and-partition-registry.md) §2) needs one scheme for `n`, the issuers' user IDs are unrelated, and one mirror graph would mix two licences and two sync cursors.

### 2. `OS`: map objects

- Each node, way and relation is a foreign item under its type code. The IDs are bare digits (`upstream_prefix = ""`, `id_grammar` `digits`), and the type code is what separates node 1 from way 1. Deleted IDs are never reused upstream; a deletion clears the mirror's contribution as in [0009](0009-keyed-entity-types-and-domain.md) §9.
- `revision_ids = false`: OSM versions are per object and changesets are not a global revision sequence, so `n` is the mirror record's offset. The upstream version and changeset ID are kept as source metadata on the record, and the changeset author's numeric user ID is the actor (`openstreetmap:{uid}`).
- Canonical IRIs are `https://www.openstreetmap.org/{node|way|relation}/{id}`.
- Way and relation membership become references to `OSN`, `OSW` and `OSR` values, by role. A node's coordinates become a coordinate value. Geometry beyond that is not derived.

### 3. The `osm` scheme: keys and tags as notations

*Changed by A2.*

OSM keys (`amenity`) and tags (`amenity=cafe`) are **notations in the `osm` scheme** of the `notation` keyed type ([0048](0048-notation.md) §1, §6): one scheme covers both, as Wikidata's P1282 is "OpenStreetMap tag or key". Keys never contain `=`, so the two forms cannot collide, and the wiki's `Key:` and `Tag:` page titles are the same strings.

| | |
|---|---|
| Type, scheme, ID | `notation`, scheme `osm`: `notation:osm:amenity`, `notation:osm:amenity=cafe` |
| Namespace | `Notation`, 216, with `Notation talk` at 217 ([0008](0008-namespaces-and-document-pages.md) §2); titles read `Notation:osm:amenity=cafe` |
| Data type | `wikibase-notation`, value type `wikibase-entityid`, on properties whose `notation-scheme` is `osm` |
| Content model | `triplespace-notation` |
| IRI | `https://scatter.red/notation/osm:{key}`, content-derived and shared by every instance |
| Grammar | The scheme's `osm-tag` grammar: a key of 1 to 255 characters, or a key, `=` and a value of 1 to 255 characters. No control characters, no `=` in the key, no leading or trailing whitespace, Unicode NFC |
| Normalizer | The scheme's `exact` normalizer: the exact string, case-sensitive. Input that is not NFC is not a valid key |
| Display | The string itself, without the scheme, as the `mul` label. Descriptions and aliases are stored and reconciled like an item's |
| Clusters | Never (§6) |

**The keyword normalizer is not used.** Keyword ([0017](0017-entity-id-grammar.md) §5) casefolds and turns punctuation into hyphens, so `fast_food`, `fast-food` and `fast:food` would become one entity, and `Name` would merge with `name`. OSM tags are case-sensitive exact strings and those forms are different tags.

**Grammar over policy.** The grammar admits any valid OSM string. Which strings become entities is decided by the adapters (§4, §5), not the grammar.

One scheme shares one namespace with every other scheme, which leaves 218 to 219 free. Splitting keys from tags into two schemes is a registry change, not a design change, if a use for the difference appears.

### 4. Tags on map objects

*Changed by A2.*

A tag `k=v` on an `OS` object becomes one of three things. Nothing is dropped.

1. **A mapped key.** If `k` is in the adapter's key map, it becomes the mapped property, a term, or a link, with the native value type. `name` and `name:{lang}` become labels and `alt_name` aliases. `website` becomes a URL value. The key map names properties **by role** ([0003](0003-statement-ui.md) §7), like the identity property in [0009](0009-keyed-entity-types-and-domain.md) §9 and OpenAlex's field mapping, so it never depends on a property's number. It is adapter configuration with shipped defaults, and mapping another key later reprojects from the log.
2. **A documented tag.** If a notation for exactly `osm:k=v` exists in the `OW` mirror, or the wiki says the key takes well-known values, the object gets a statement with role `osm-tag` and value `notation:osm:k=v`.
3. **Everything else.** The object gets a statement with role `osm-tag-text`, string value `v`, and a qualifier with role `osm-key` and value `notation:osm:k`.

Free-text keys such as `name`, `phone` and `opening_hours` should have their own properties, by role, through case 1. Case 3 is the fallback until they do, and it means no property is minted per key and no per-instance property numbers exist. The roles `osm-tag`, `osm-tag-text` and `osm-key` are registered with the other roles of [0003](0003-statement-ui.md) §7, and the properties bound to `osm-tag` and `osm-key` carry the `notation-scheme` statement `osm` ([0048](0048-notation.md) §4). Without a mirrored `OW`, case 2 does not occur and every unmapped tag takes case 3.

### 5. `OW`: the vocabulary

*Changed by A2.*

The wiki's Wikibase models keys and tags as items and describes them with about fifty properties. The adapter maps two classes to notations in the `osm` scheme (the key map names `key_type = "notation"` and `key_scheme = "osm"`, [0048](0048-notation.md) §6):

| Class | Identity property | Becomes |
|---|---|---|
| `Q7` OpenStreetMap key | `P16` permanent key ID, stated never to change | `notation:osm:{key}` |
| `Q2` OpenStreetMap tag | `P19` permanent tag ID, `key=value` | `notation:osm:{key}={value}` |

The rules of [0009](0009-keyed-entity-types-and-domain.md) §9 apply. An item with one valid identity value is written under its key, and the upstream ID becomes an alias. Values pointing at such items become `wikibase-notation` values. An item with no valid identity value, or with several, is reported and stays `OWQ{n}`. `P16` and `P19` are dropped from a mapped item's mirrored state, since the key is the value.

**What the wiki adds to the tag entity**, all as mirrored statements:

- **Concept links.** `P7` (Wikidata equivalent) and `P12` (Wikidata concept) link a tag or key to a Wikidata item. They become ordinary statements, and never `same-as` (§6).
- **Relations between tags.** `P3` (subclass of), `P45` (implies), `P44` (incompatible with), `P51` (identical to), `P18` (different from) and `P20` (property different from), with targets rewritten to notation values where the target item is mapped. These are exactly the rule-shaped facts constraints ([0031](0031-property-constraints.md)) could use, and OSM has no other place for them.
- **Usage.** Applicability to nodes, ways, relations and areas (`P33`, `P34` and siblings), regional use (`P29`, `P30`), status, key type (`P9`) and documentation pages (`P31`).

Properties marked DEPRECATED upstream are skipped. The wiki documents only the keys and tags someone has written up; every other tag still exists as a keyed entity, without these statements.

### 6. Tags are not concepts

*Changed by A2.*

No notation of any scheme joins an identity cluster ([0048](0048-notation.md) §3), as a keyword does not ([0017](0017-entity-id-grammar.md) §5), for the same reason: `notation:osm:amenity=cafe` is a tag, and a Wikidata item about cafés is the concept. `same-as` with a notation on either side is rejected at write time, and no tier-2 or tier-3 link ([0004](0004-identity-clusters-and-equivalence.md) §6) may produce one. `P7` and `P12` stay statements, so a user can follow a tag to its concept without the phrase becoming canonical over the concept.

### 7. Registry, namespaces and crates

*Changed by A1, A2.*

- `providers.toml`: `OS` (number 6) and `OW` (number 7), the `OW` key map with `key_type = "notation"` and `key_scheme = "osm"`. `issuers.toml`: `openstreetmap` and `osmwiki`. `keyed-types.toml`: `notation`; `notation-schemes.toml`: `osm` ([0048](0048-notation.md) §7). `namespaces.toml`: `Notation` 216 and `Notation talk` 217. The registry README lists 216/217 in the mediawiki.org registration still to be filed.
- **New crate:** `scatter-adapter-openstreetmap` (layer 3), depending on `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers` and `scatter-normalize`; [0005](0005-crate-organization.md) §2 is the crate table CI checks. `OW` needs none: it is `scatter-adapter-wikidata` parameterised by the provider entry, with the key-mapping capability that `scatter-adapter-internetdomains` needs as well.
- `scatter-normalize` keeps the `osm-tag` grammar as the `osm` scheme's grammar. `triplespace-titles` gains the `Notation` namespace ([0048](0048-notation.md) §8).

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
