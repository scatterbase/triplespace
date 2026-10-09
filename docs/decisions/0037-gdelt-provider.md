# 0037. GDELT as a provider

- **Status:** Proposed
- **Date:** 2026-09-28
- **Updated:** 2026-10-09 (A6)
- **Author:** James Hare / Claude Sonnet
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0005](0005-crate-organization.md), [0017](0017-entity-id-grammar.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0009](0009-keyed-entity-types-and-domain.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0026](0026-sitelinks.md), [0029](0029-resolver-namespaces.md), [0036](0036-openstreetmap-providers.md), [0048](0048-notation.md)
- **Chapters:** [05](../architecture/05-providers-and-ingest.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md)

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

*Changed by A5.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §8.1, §8.10.*

### 2. Types and IDs

*Changed by A3.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §8.10.*

### 3. What a document becomes

*Current text: [05](../architecture/05-providers-and-ingest.md) §8.11.*

### 4. What an event becomes

*Current text: [05](../architecture/05-providers-and-ingest.md) §8.12.*

### 5. Scope: a configured subset

*Current text: [05](../architecture/05-providers-and-ingest.md) §8.13.*

### 6. The feed

*Current text: [05](../architecture/05-providers-and-ingest.md) §8.13.*

### 7. Attribution and licence

*Current text: [05](../architecture/05-providers-and-ingest.md) §8.13.*

### 8. Registry and crates

*Changed by A1, A2.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2; [23](../architecture/23-configuration-and-registry.md) §4.2.*

## Consequences

- **Only what GDELT identifies gets an identity.** Events, documents, CAMEO codes and themes are entities. Actors, people, organizations and places stay strings, coordinates or codes until an identifier lookup finds a single match.
- **Nothing is mirrored by default.** An unconfigured instance ingests no GDELT, and the cost is chosen, not discovered.
- **The append-only feed maps directly onto the log**, with a slot as the resumption unit.
- **Noise stays visible.** Mention confidence and tone are statements and qualifiers, not filters hidden in the adapter. Ranks carry no claim of accuracy.
- **A document's source is a Domain**, which ties it to the domain data of [0009](0009-keyed-entity-types-and-domain.md) and internetdomains.wiki, and to sitelink hosts ([0026](0026-sitelinks.md)).
- **A new ID grammar mechanism.** Grammars can now be set per type. That is a change to [0017](0017-entity-id-grammar.md) §2 (0017 A4) and to how `scatter-providers` reads the registry.
- **The IRIs are ours.** Anything that cites `https://scatter.red/gdelt/…` depends on Scatter continuing to serve those addresses, since GDELT itself will never resolve them.

## Open questions

- **Q1. Volume.** The current daily size of each file kind, and what a reasonable filter costs in storage, have not been measured. This ADR does not state them because the codebook does not.
- **Q2. Theme and code lists.** The `token` grammar is inferred from the theme names the codebook shows (`TAX_…`, `WB_…`); the Category List and the CAMEO tables are to be checked against a real load, including where the lists live and how they change.
- **Q3. Document URLs as resolver keys.** Whether a `document-url` role should be bound to the `url` resolver of [0029](0029-resolver-namespaces.md), so that `URL:https://…` reaches a document.
- **Q4. Reconciling names.** Whether people, organizations and actor names should ever be offered as reconciliation suggestions to Wikidata, and if so by what process, with what review.
- **Q5. Offsets.** Whether character offsets on themes, locations and names are worth their size, since they let a reader find the passage in the article.
- **Q6. Other collections and history.** Non-web GKG collections, GDELT 1.0, and the other GDELT products (Web NGrams, the visual and television GKGs, the Global Entity Graph) are not covered.
- **Q7. GCAM.** The 2,300 emotion and theme dimensions per document are not stored. Whether any subset justifies a place.
- **Q8. Source domains.** `SourceCommonName` is not always a domain; whether other values are kept as strings or dropped.
- **Q9. IRIs.** What `scatter.red` serves at the four IRI families, and whether GDELT should be asked for canonical ones.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §4 | §2 | amends | 0002 A18 |
| [0003](0003-statement-ui.md) §7 | §3–4 | extends | 0003 A6 |
| [0005](0005-crate-organization.md) §2 | §8 | amends | 0005 A36 |
| [0017](0017-entity-id-grammar.md) §2 | §2 | amends | 0017 A4 |

## References

- [GDELT terms of use](https://gdeltproject.org/about.html)
- [GDELT 2.0 event codebook](http://data.gdeltproject.org/documentation/GDELT-Event_Codebook-V2.0.pdf)
- [GDELT Global Knowledge Graph 2.1 codebook](http://data.gdeltproject.org/documentation/GDELT-Global_Knowledge_Graph_Codebook-V2.1.pdf)
- [0036 — OpenStreetMap providers](0036-openstreetmap-providers.md): the parallel treatment of a second free-text-tag source

## Amendment log

### A1. The grammars live in `scatter-providers`

- **Date:** 2026-09-28
- **Source:** Direct: James, correction of 2026-09-28 (commit `115d7b1`)
- **Change:** corrects §8
- **Summary:** The `gdelt-record` and `token` grammars live in `scatter-providers`, with the registry field that names them, not in `scatter-normalize`, which keeps the key grammars of keyed types. The line was corrected in place with a note, now removed.

Replaced text (§8):

> - `scatter-normalize` gains the `gdelt-record` and `token` grammars. `scatter-providers` gains per-type `id_grammar`.

### A2. Crate table

- **Date:** 2026-09-28
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** extends §8
- **Summary:** 0005 §2 is the one crate table CI checks, and carries `scatter-adapter-gdelt` and the `scatter-providers` additions (0005 A36); §8 keeps its registry notes and names 0005 §2 for the crate.

### A3. `osm-tag` becomes the `osm` scheme of Notation

- **Date:** 2026-10-01
- **Source:** [0048](0048-notation.md) §1, §6
- **Change:** amends §2
- **Summary:** The keyed type `osm-tag`, named among the open-ended natural keys that keyed types are for, is now the `osm` scheme of the `notation` keyed type (0036 A2). The parenthesis was edited in place with a note, now folded.

Replaced text (§2):

> Keyed types are for open-ended natural keys that several providers share (Domain, Keyword, `osm-tag`).

### A4. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §2, §8
- **Summary:** A1–A3 were folded into the Decision. The open questions were numbered, and the change this ADR makes to 0002 §4 (GDELT's canonical IRIs are minted here), which 0002 had not recorded, was written there as 0002 A18. No decision changed. Before this, A1 and A3 were in-place edits with notes, and A2 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A5. `adapter` and `actor_model` are provider fields

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §1
- **Summary:** In `providers.toml`, `adapter` is one field per provider, naming the adapter crate (`scatter-adapter-gdelt` for `GD`), and `actor_model` (`individual` or `provider-only`) is an explicit provider field, because `issuer == slug` is not the test for it: OSM's issuer is its slug and it has individual authors ([0002](0002-source-graphs-and-mass-ingest.md) §4). The row's verb is `amends`, but §1 already states `actor_model = "provider-only"` and names the adapter crate, and nothing in it is contradicted, so here it is an extension: §1's registration gains the `adapter` field. (PENDING E8)

### A6. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [05](../architecture/05-providers-and-ingest.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
