# 0037. GDELT as a provider

- **Status:** Proposed
- **Date:** 2026-09-28
- **Author:** James Hare / Claude Sonnet
- **Related:** [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (§4 provider registry, §8.4 adapters), [0003 — Statement UI](0003-statement-ui.md) (§7 roles), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md) (§3 link tiers), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§2, §6 licences), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md), [0009 — Keyed entity types and Domain](0009-keyed-entity-types-and-domain.md) (§5 Domain values), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§9 bootstrap), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (§2, §5), [0017 — Entity ID grammar](0017-entity-id-grammar.md) (§2 grammars amended), [0026 — Sitelinks are URLs](0026-sitelinks.md) (§1 URL normalizer), [0029 — Resolver namespaces](0029-resolver-namespaces.md) (§5), [0036 — OpenStreetMap providers](0036-openstreetmap-providers.md)

## Context

GDELT (Global Database of Events, Language and Tone) publishes three tables every 15 minutes, as tab-separated files:

- **Events**: one row per coded event. It has a `GlobalEventID`, a day, two actors (CAMEO actor codes, names, country, group, ethnic, religion and type codes), an `EventCode` with base and root codes, a `QuadClass`, a Goldstein score, tone, three geographies (each with a type, name, FIPS country and ADM1 code, coordinates and a `FeatureID`) and a `SOURCEURL`.
- **Mentions**: one row per appearance of an event in an article, with the mention time, the document identifier, the sentence, character offsets, a `Confidence` percentage and the document's tone.
- **GKG** (Global Knowledge Graph): one record per article, with a `GKGRECORDID` (`YYYYMMDDHHMMSS-n`, or `-Tn` for a translated record), the source's common name, the document identifier, themes, locations, persons, organizations, tone, amounts, quotations and the GCAM emotion dictionaries.

The files are append-only, which fits the log well. Everything else about GDELT is a poor fit for the assumptions the rest of these ADRs make about a provider:

- **It mints almost no identifiers.** Actors are CAMEO codes plus a name string. People and organizations are name strings. A location is a coordinate plus a `FeatureID` that is a GNS or GNIS number, or a country or ADM1 code as text. There is no canonical IRI for anything.
- **It is machine-coded and noisy.** Events are extracted by software; the mention `Confidence` field exists because a coding may be wrong.
- **It is large.** The feed grows continuously, so an instance cannot assume it wants all of it.
- **Its content is other people's writing.** The articles belong to their publishers. GDELT's terms allow unrestricted use, redistribution and mirroring, provided any use cites the GDELT Project with a link to its website. That covers GDELT's data, and not the text of the articles.

The mechanisms these ADRs already provide fit the parts that are identifiable. Provider-minted types ([0002](0002-source-graphs-and-mass-ingest.md) §4) cover events, documents and the two finite vocabularies GDELT defines. Roles ([0003](0003-statement-ui.md) §7) let statements name properties without depending on property numbers. Domain values ([0009](0009-keyed-entity-types-and-domain.md) §5) fit the source of a document.

## Decision

### 1. One provider

`GD` (number 8, slug `gdelt`, issuer `gdelt`) is registered in `providers.toml` and `issuers.toml`. It has no individual actors (`actor_model = "provider-only"`, as OpenAlex), and `revision_ids = false`, so `n` in the revision-ID range is the mirror record's offset ([0015](0015-record-format-and-partition-registry.md) §2). The adapter is a new crate (§8).

The GDELT 1.0 files (events only, a different column set) are out of scope. This ADR covers the 2.0 files that begin in 2015.

### 2. Types and IDs

| Code | Type | ID | Notes |
|---|---|---|---|
| `E` | Event | `GDE1234567890` | `GlobalEventID`; grammar `digits` |
| `D` | Document | `GDD20150218230000-12`, `GDD20150218230000-T12` | `GKGRECORDID`; grammar `gdelt-record` |
| `C` | CAMEO event code | `GDC0311` | the code as GDELT prints it, leading zeros kept; grammar `digits`, canonical as given |
| `T` | GKG theme | `GDTTAX_FNCACT_MAYOR` | grammar `token` |

**Two grammars are new, and a grammar may be set on a type.** [0017](0017-entity-id-grammar.md) §2 defines `digits` and `uuid` at the provider level. This ADR adds `gdelt-record` (14 digits, `-`, an optional uppercase `T`, digits) and `token` (uppercase letters, digits and underscores, starting with a letter, at most 128 characters), and lets `id_grammar` be set on a single type, overriding the provider's. Disjointness from keyed types is unaffected: a keyed ID carries its `type:` prefix ([0017](0017-entity-id-grammar.md) §3).

**Canonical IRIs are Triplespace's.** GDELT publishes none, so `https://scatter.red/gdelt/{event|document|cameo|theme}/{id}` is minted here. It amends [0002](0002-source-graphs-and-mass-ingest.md) §4, where a foreign entity's canonical IRI is the provider's own. What `scatter.red` serves at those addresses is as open as it is for domains ([0009](0009-keyed-entity-types-and-domain.md)).

**Themes and CAMEO codes are provider items, not keyed types.** They are finite vocabularies that GDELT defines and lists (the Category List and the CAMEO tables). Keyed types are for open-ended natural keys that several providers share (Domain, Keyword, `osm-tag`). Each theme and CAMEO code carries its label and, for CAMEO, its position in the hierarchy (root, base, code) as `subclass of` statements to its parent code. No namespace pair is used.

### 3. What a document becomes

A document (`D`) is an item. It carries, by role:

| Role | Value | From |
|---|---|---|
| `document-url` | a URL, normalized by the sitelink normalizer ([0026](0026-sitelinks.md) §1) | `DocumentIdentifier` |
| `published` | a time | the GKG date |
| `source-domain` | a `wikibase-domain` value ([0009](0009-keyed-entity-types-and-domain.md) §5) | `SourceCommonName`, when it is a valid Domain key |
| `theme` | a `GDT` item | the enhanced themes, without offsets |
| `location` | a coordinate, with qualifiers for the name, FIPS country, ADM1 code and `FeatureID` | the enhanced locations, without offsets |
| `tone`, `positive-score`, `negative-score`, `polarity`, `word-count` | quantities | the V1.5 tone field |
| `source-language` | an ISO 639-2 code | the translation info, when present |

Documents are items, not resolver keys. The `url` resolver ([0029](0029-resolver-namespaces.md) §5) is bound to sitelinks, so it finds an item only when the URL is one of its sitelinks. Whether a `document-url` role should also be bound to that resolver is open.

**Version 1 mirrors only web sources** (`SourceCollectionIdentifier` 1). The other collections use other kinds of identifier (citations, DOIs), and they are left for later.

**People and organizations are off by default.** GKG lists the names it extracts from an article, and a private individual named in a news story is not a public figure. The adapter's `include_persons` and `include_organizations` options default to false. When on, a name is a string-valued statement on the document with no character offsets, never an entity, and never reconciled to Wikidata automatically. Erasure of a document ([0006](0006-log-integrity-and-erasure.md)) removes them with it.

**Not carried in version 1:** quotations (verbatim text the publisher owns), amounts, GCAM, images and embeds, `AllNames`, extracted dates and the extras XML.

### 4. What an event becomes

An event (`E`) is an item with statements, by role:

- **`event-type`**: a `GDC` item for the `EventCode`. Base and root codes follow from its `subclass of` chain, so they are not stored again.
- **`event-date`**: the `Day`.
- **`goldstein-scale`**, **`avg-tone`**, **`quad-class`**: quantities.
- **`event-actor`**, once per actor: the actor's name as a string (or its CAMEO code when the name is empty), with qualifiers for its ordinal (1 or 2), CAMEO actor code, country code, known-group code, ethnic and religion codes and type codes. Actors are strings, not entities: GDELT gives them no identity beyond a code and a name.
- **`location`**: the action geography as a coordinate, with the same qualifiers as a document's location.
- **`source-url`**: the row's `SOURCEURL`.
- **`mentioned-in`**, once per kept mention: the mention's document, with qualifiers for the confidence, mention time, sentence number, character offsets, and the document's tone at mention.

`NumMentions`, `NumSources` and `NumArticles` are not stored. They change as mentions arrive and the projection can count `mentioned-in` statements.

**Resolved references.** A `location` or `mentioned-in` value is written as a reference to another entity only when a lookup by identifier finds exactly one: a place by its `FeatureID` against the GNS or GNIS identifier properties that Wikidata mirrors carry (P2326 and P590), and a document by its normalized `document-url`. The rule is the shape of a tier-3 link ([0004](0004-identity-clusters-and-equivalence.md) §3) but produces a value, not a cluster. Otherwise the coordinate or URL stands alone. Nothing about an event ever joins an identity cluster: an event is not the place it happened or the article that reported it.

**The FIPS problem.** Geography fields use FIPS 10-4 country codes and actor countries use CAMEO codes, not ISO codes. The adapter keeps the codes as given and adds no mapping table. Wikidata's FIPS 10-4 property (P901) is the bridge, reached by ordinary identifier lookup when a tenant wants it.

### 5. Scope: a configured subset

Nothing is mirrored until an instance configures a filter. The adapter's configuration selects, independently for events and for documents:

- a date range;
- event root codes (`EventRootCode`) and `QuadClass`;
- countries and ADM1 regions of the action geography;
- themes;
- source domains.

A mention is kept only when its event is kept and its confidence reaches a configured threshold. A document is kept when it passes the document filter or is the target of a kept mention. Filtering is by row and is applied before anything is written, so filtered-out data never reaches the log.

A tenant whose purpose is the citation graph would filter documents by source domain and mirror few events. A tenant studying an event class would filter events by root code and country.

### 6. The feed

The adapter follows the GDELT 2.0 file lists: `lastupdate.txt` for the current slot, `masterfilelist.txt` for the backlog, and the translation list for translated records (`-T` IDs). Each 15-minute slot has an events export, a mentions file and a GKG file. The version cursor of [0002](0002-source-graphs-and-mass-ingest.md) §8.4 is the last slot fully ingested, so a slot is the unit of resumption, and re-running a slot is idempotent: the same IDs produce the same records. If GDELT ever re-emits an ID with different content, the adapter treats it as an upsert.

The codebook does not say whether events are revised or deleted after publication. The adapter assumes none are and reports a surprise, so nothing here depends on the answer.

Bootstrap of a large backlog uses the mode of [0013](0013-postgres-storage.md) §9. An instance that filters tightly will not need it.

### 7. Attribution and licence

GDELT's terms allow use, redistribution and mirroring in any form, and require a citation to the GDELT Project with a link to its website. The `mirror/gdelt` graph's metadata carries that citation, and every dump and page that shows GDELT data shows it. This settles GDELT's licence for a mirror ([0005](0005-crate-organization.md) §6). It does not extend to article text, which is why the adapter stores URLs, names, codes and scores and never text or quotations.

### 8. Registry and crates

- `providers.toml`: `GD` (number 8) with types `E`, `D`, `C`, `T`. `issuers.toml`: `gdelt`. No keyed type and no namespace are added.
- **New crate:** `scatter-adapter-gdelt` (layer 3), depending on `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers` and `scatter-normalize`.
- `scatter-providers` gains per-type `id_grammar` and the `gdelt-record` and `token` grammars, beside `digits` and `uuid`: the ID grammars of minted types live with the registry field that names them ([0017](0017-entity-id-grammar.md) §7), and `scatter-normalize` keeps the key grammars of keyed types. (Corrected 2026-09-28; this line first placed the two grammars in `scatter-normalize`.)

## Consequences

- **Only what GDELT identifies gets an identity.** Events, documents, CAMEO codes and themes are entities. Actors, people, organizations and places stay strings, coordinates or codes until an identifier lookup finds a single match.
- **Nothing is mirrored by default.** An unconfigured instance ingests no GDELT, and the cost is chosen, not discovered.
- **The append-only feed maps directly onto the log**, with a slot as the resumption unit.
- **Noise stays visible.** Mention confidence and tone are statements and qualifiers, not filters hidden in the adapter. Ranks carry no claim of accuracy.
- **A document's source is a Domain**, which ties it to the domain data of [0009](0009-keyed-entity-types-and-domain.md) and internetdomains.wiki, and to sitelink hosts ([0026](0026-sitelinks.md)).
- **A new ID grammar mechanism.** Grammars can now be set per type. That is a change to [0017](0017-entity-id-grammar.md) §2 and to how `scatter-providers` reads the registry.
- **The IRIs are ours.** Anything that cites `https://scatter.red/gdelt/…` depends on Scatter continuing to serve those addresses, since GDELT itself will never resolve them.

## Open questions

- **Volume.** The current daily size of each file kind, and what a reasonable filter costs in storage, have not been measured. This ADR does not state them because the codebook does not.
- **Theme and code lists.** The `token` grammar is inferred from the theme names the codebook shows (`TAX_…`, `WB_…`); the Category List and the CAMEO tables are to be checked against a real load, including where the lists live and how they change.
- **Document URLs as resolver keys.** Whether a `document-url` role should be bound to the `url` resolver of [0029](0029-resolver-namespaces.md), so that `URL:https://…` reaches a document.
- **Reconciling names.** Whether people, organizations and actor names should ever be offered as reconciliation suggestions to Wikidata, and if so by what process, with what review.
- **Offsets.** Whether character offsets on themes, locations and names are worth their size, since they let a reader find the passage in the article.
- **Other collections and history.** Non-web GKG collections, GDELT 1.0, and the other GDELT products (Web NGrams, the visual and television GKGs, the Global Entity Graph) are not covered.
- **GCAM.** The 2,300 emotion and theme dimensions per document are not stored. Whether any subset justifies a place.
- **Source domains.** `SourceCommonName` is not always a domain; whether other values are kept as strings or dropped.
- **IRIs.** What `scatter.red` serves at the four IRI families, and whether GDELT should be asked for canonical ones.

## References

- [GDELT terms of use](https://gdeltproject.org/about.html)
- [GDELT 2.0 event codebook](http://data.gdeltproject.org/documentation/GDELT-Event_Codebook-V2.0.pdf)
- [GDELT Global Knowledge Graph 2.1 codebook](http://data.gdeltproject.org/documentation/GDELT-Global_Knowledge_Graph_Codebook-V2.1.pdf)
- [0036 — OpenStreetMap providers](0036-openstreetmap-providers.md): the parallel treatment of a second free-text-tag source
