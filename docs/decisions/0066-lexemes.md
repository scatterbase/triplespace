# 0066. Lexemes

- **Status:** Proposed
- **Date:** 2026-10-05
- **Author:** James Hare / Claude Fable
- **Changes:** [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0017](0017-entity-id-grammar.md), [0041](0041-content-models.md), [0043](0043-lua-modules.md), [0047](0047-special-pages.md), [0060](0060-scopes.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0009](0009-keyed-entity-types-and-domain.md), [0010](0010-site-ui.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0023](0023-moderation.md), [0031](0031-property-constraints.md), [0032](0032-sparql-update-stream.md), [0035](0035-adopting-a-wikibase.md), [0045](0045-table-content-model.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

Wikidata has held lexicographical data since 2018: **lexemes** (`L1`), each with lemmas, a language and a lexical category, **forms** (`L1-F1`) with representations and grammatical features, and **senses** (`L1-S1`) with glosses, all three carrying statements. Over a million lexemes are in the dumps, and every Wikidata test import so far has skipped them: `Lexeme` (146) is `reserved` ([0008](0008-namespaces-and-document-pages.md) §2), the `wikibase-lexeme` model is `reserved` ([0041](0041-content-models.md) §3), the Wikidata provider's type `L` says "mirroring lexemes requires implementing it" ([0017](0017-entity-id-grammar.md), `providers.toml`), and `wikibase-compat.md` §2 puts lexemes, forms and senses out of scope. The ID grammar of [0017](0017-entity-id-grammar.md) §1 — one letter and digits — has no place for `L1-F1`; [0009](0009-keyed-entity-types-and-domain.md) noted forms and senses as the one Wikibase precedent for entity values without a numeric ID. [0031](0031-property-constraints.md) §1 already names the `lexeme requires language` constraint; [0043](0043-lua-modules.md) Q4 waits for `mw.wikibase.lexeme`.

### Direction

James's direction, from the design discussion of 2026-10-05:

- **Forms and senses are treated the way Wikidata treats them.**
- **Lexemes in this round**, since they affect a Wikidata test import.

## Decision

### 1. Lexemes as Wikidata has them

**`lexeme` is an entity type**, local `L1`, foreign `WDL1`, hosted in namespace 146 (§2) under the `wikibase-lexeme` model (§3). Its data model is WikibaseLexeme's, in its JSON serialization, unchanged:

| Part | Fields | Statements |
|---|---|---|
| **Lexeme** | `lemmas` (multilingual text, one per language code or variant), `language` (an item), `lexicalCategory` (an item) | Yes |
| **Form** `L1-F1` | `representations` (multilingual text), `grammaticalFeatures` (items) | Yes |
| **Sense** `L1-S1` | `glosses` (multilingual text) | Yes |

**Forms and senses are parts of the lexeme, not entities.** One page, one record stream, one revision history; a form's or sense's statements are in the lexeme's resolved JSON under `forms[].claims` and `senses[].claims`, with statement IDs `L1-F1$<uuid>`. They have no page, no terms table rows of their own beyond §5, no cluster membership (§4), and no ACL target of their own: the lexeme encloses them ([0023](0023-moderation.md) §2). A form or sense **is an entity value**: the data types `wikibase-lexeme`, `wikibase-form` and `wikibase-sense` are added to `scatter-wikibase-model`, with `{"entity-type": "form", "id": "L1-F1"}` as Wikibase writes them, usable on items and lexemes alike. Lexeme **terms** are lemmas, not labels: a lexeme has no `labels`, and the places that show "the label" show the lemmas joined by `/` as Wikidata does.

### 2. The `Lexeme` namespace (amends 0008 §2)

| Number | Canonical name | Kind | Allowed models | Default model |
|---|---|---|---|---|
| 146 | `Lexeme` | `pages` *(was `reserved`)* | `wikibase-lexeme` | `wikibase-lexeme` |
| 147 | `Lexeme talk` | `pages` | `triplespace-talk` | `triplespace-talk` |

WikibaseLexeme's number, implemented with its meaning. Titles are lexeme IDs (`Lexeme:L1`, `Lexeme:WDL1`); a form or sense ID in a title (`Lexeme:L1-F1`) resolves to the lexeme's page with the part's anchor, as `Special:EntityPage/L1-F1` does on Wikidata.

### 3. The `wikibase-lexeme` content model (extends 0041 §3)

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `wikibase-lexeme` | WikibaseLexeme | entity (`lexeme`) | main | `application/json` | No | 146 |

An entity model; edits go through the modules of §8 and the UI of §7. 0043 Q4 and the "reserved" notes in `providers.toml` and `content-models.toml` are resolved by this section.

### 4. IDs and clusters (extends 0017 §1; extends 0004 §1)

**0017 §1's local form gains a sub-entity suffix**, on `L` only: a local lexeme ID is `L` and digits, optionally followed by `-F` and digits or `-S` and digits. The foreign form follows: `WDL1-F1`. The input form is `LLL1-F1`. The suffix is part of the ID wherever an ID is stored, compared or shown; `L1-F1` and `L1` are different IDs of one record stream. Statement IDs are `<part ID>$<uuid>`. No other type takes a suffix; a provider type that minted one would need an ID grammar of its own ([0017](0017-entity-id-grammar.md) §2).

**Clusters are of lexemes.** `same-as` between lexemes forms an identity cluster as between items ([0004](0004-identity-clusters-and-equivalence.md) §1); `same-as` between forms or senses, or between a part and a lexeme, is refused (`ts-cluster-part`). The canonical lexeme's parts are the cluster's parts; a non-canonical member's forms and senses are **not merged** into the canonical lexeme's (Q1), so a reference to `WDL1-F1` resolves to that form under the canonical lexeme's page, never to a renumbered part. **Merging lexemes** (`Special:MergeLexemes`, §7) is `same-as` plus the conversion of [0004](0004-identity-clusters-and-equivalence.md) §5 when both are local, which is what WikibaseLexeme's merge does by hand: the source's forms and senses are appended to the target with new part numbers and the source becomes a redirect; here the source becomes a cluster member and its parts keep their IDs.

### 5. Terms, search and storage (extends 0013 §5.6; extends 0014 §7; extends 0060 §4)

`view.term` ([0013](0013-postgres-storage.md) §5) gains three term kinds: **4 lemma**, **5 representation**, **6 gloss**, with `entity_id` the lexeme's ID for lemmas and the part's ID for representations and glosses, and `lang` the language code or variant. The term projection writes them in step 4 with the other terms, so label lookups, prefix search without OpenSearch and the term-based APIs see them with no new table.

**Search** ([0014](0014-caches-and-search.md) §7): the entity index gains lexeme documents with `lemma`, `language`, `lexical_category`, `representation` and `gloss` fields; `wbsearchentities&type=lexeme` matches lemmas and shows "lemma (language, category)" as Wikidata does, `type=form` matches representations and `type=sense` glosses, each returning the part ID. The search box's entity suggester offers lexemes under their own heading.

**Storage** otherwise as any entity: `view.entity` rows for `L` with the resolved JSON including parts; `view.statement_assertion` rows with part IDs as subjects, so a `statement` scope over `P5137` (item for this sense) selects senses ([0060](0060-scopes.md) §4 gains `form` and `sense` as subject types alongside `entity`); `entity_ref` from statements on parts.

### 6. RDF (extends wikibase-compat.md §5)

Lexemes are emitted as Wikidata emits them, with the OntoLex vocabulary, so the Wikibase-compatible dump and the stream carry them unchanged and QLever's Wikidata index layout applies:

| Subject | Triples |
|---|---|
| Lexeme `wd:L1` | `a ontolex:LexicalEntry`; `wikibase:lemma` and `rdfs:label` for each lemma; `dct:language wd:Q1860`; `wikibase:lexicalCategory wd:Q1084`; `ontolex:lexicalForm wd:L1-F1`; `ontolex:sense wd:L1-S1`; statements as on any entity |
| Form `wd:L1-F1` | `a ontolex:Form`; `ontolex:representation` per language; `wikibase:grammaticalFeature wd:Q…`; statements |
| Sense `wd:L1-S1` | `a ontolex:LexicalSense`; `skos:definition` per language; statements |

Part IRIs follow the entity IRI rules of [0015](0015-record-format-and-partition-registry.md) §5 with the suffix in the local part. `wikibase-compat.md` gains §5.6 "Lexemes" with the mapping as observed from Wikidata's dump, and §2's exclusion of lexemes, forms and senses is lifted; entity schemas stay out of its RDF ([0064](0064-entityschema-and-validation.md) §8).

### 7. The UI and special pages (extends 0047 §9)

**The lexeme page** has its own layout: a header with the lemmas, language and lexical category (each editable in place); the lexeme's statements; then **Forms**, each a card with its representations and grammatical features and its statements; then **Senses**, each with its glosses and statements. Every statement block is the statement UI of [0003](0003-statement-ui.md), with the part as subject, so shape detection, ranks, roles and provenance work unchanged. Add form, add sense, remove and reorder are actions on the page. A part's anchor is its ID.

**Special pages** move from `reserved` to `served`: `NewLexeme` (lemma, language, category; appends the first record) and `MergeLexemes` (§4). The entity suggester, `Special:Search`, `Special:EntityPage` and `Special:EntityData` take lexeme and part IDs.

### 8. API (extends 0012 §5)

**Action API**: WikibaseLexeme's modules, with its parameters and errors — `wbladdform`, `wblremoveform`, `wbleditformelements`, `wbladdsense`, `wblremovesense`, `wbleditsenseelements`, `wblmergelexemes` — plus `wbeditentity` with `new=lexeme` or on `L` IDs, the statement modules on lexeme and part IDs, `wbgetentities` returning the full lexeme JSON for a lexeme ID and the part for a part ID, and `wbsearchentities` with the three types (§5). Each module appends a change set whose operations address parts by ID; `scatter-wikibase-changeset` gains paths into `forms` and `senses`.

**REST** (`triplespace/v0`): `GET /entity/L1` with parts; `GET /entity/L1-F1` the part alone; the Wikibase REST API has no lexeme routes yet, so none are mirrored.

### 9. Mirroring, constraints, Lua and tables (settles 0043 Q4)

- **The Wikidata adapter** parses `"type": "lexeme"` documents from the same JSON dumps and EventStreams it already reads, writing `put` records for `WDL` as for any entity ([0002](0002-source-graphs-and-mass-ingest.md) §4); the RevisionChest reader covers them. A Wikidata test import now includes lexemes, which is the reason for this round.
- **Constraints** ([0031](0031-property-constraints.md)): `lexeme requires language`, `lexeme requires lexical category`, `lexeme value requires lexical category` and the form/sense scoped constraint types evaluate over the lexeme's resolved JSON; the `scope` constraint's `as qualifier`/`as reference` parameters gain WikibaseLexeme's `as main value on a form/sense` meaning.
- **Lua**: `mw.wikibase.lexeme` and the `mw.wikibase.lexeme.entity.*` classes as the extension documents them ([0043](0043-lua-modules.md) Q4 settled); usage tracking per lexeme ID.
- **Tables and scopes**: `L` and part IDs as rows; term columns `lemma:{lang}`, `representation:{lang}`, `gloss:{lang}`; the `language` and `lexicalCategory` fields as columns.
- **Adoption** ([0035](0035-adopting-a-wikibase.md) §2): a Wikibase with lexemes adopts its `L` entities under their IDs, as 0035 anticipated.

### 10. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-wikibase-model` | The `lexeme` entity type with forms and senses; the `wikibase-lexeme`, `wikibase-form` and `wikibase-sense` data types; part IDs |
| `scatter-wikibase-changeset` | Paths into `forms` and `senses`; the add/remove/edit-elements operations |
| `scatter-wikibase-rdf` | The OntoLex mapping of §6 |
| `scatter-wikibase-shape` | Lexeme, form and sense term columns in tables |
| `scatter-identity` | The part-ID grammar; `ts-cluster-part` |
| `scatter-adapter-wikidata` | Lexeme documents from dumps and streams |
| `scatter-wikibase-constraints` | The lexeme constraint types and scopes |
| `triplespace-projections` | Term kinds 4–6; part subjects in `statement_assertion` and `entity_ref` |
| `triplespace-search` | Lexeme documents and the three search types |
| `triplespace-api-action`, `triplespace-api-rest` | The modules and routes of §8 |
| `triplespace-ui` | The lexeme page, `NewLexeme`, `MergeLexemes`, the suggester heading |
| `triplespace-scribunto` | `mw.wikibase.lexeme` |

## Alternatives considered

- **Forms and senses as entities of their own**, with pages and cluster membership. Rejected by direction and by Wikibase: a form has no existence apart from its lexeme, and tools address parts through the lexeme's JSON.
- **Lemmas as labels.** Tempting for the UI, wrong for the data: a lexeme may have several lemmas in one language's variants, and `rdfs:label` is emitted beside `wikibase:lemma`, not instead of it.
- **A leaner RDF vocabulary.** Rejected: the dump has to load into a Wikidata-shaped QLever index unchanged, and OntoLex is what the ecosystem reads.
- **Merging parts across a cluster.** Renumbering a member's forms under the canonical lexeme would change IDs that statements elsewhere point at; kept as Q1.

## Consequences

- **A Wikidata test import is complete**: every entity type in the dumps mirrors.
- **Lexicographical data is first-class** — pages, statements, search, RDF, Lua, tables and scopes — with nothing bent: forms and senses are parts, lemmas are not labels, the vocabulary is OntoLex.
- **The ID grammar has one suffix form**, on one letter, and clusters stay at the lexeme level.
- **Test plan.** JSON round-trip of Wikidata lexemes through `put`, `wbgetentities` and the dump against the originals; the ShEx-free compat checklist of `wikibase-compat.md` §7 gains lexeme items; part-ID parsing and refusal of suffixes on other letters; `same-as` on parts refused; search by lemma, representation and gloss; each WikibaseLexeme module against the extension's own API tests; the lexeme page renders a 50-form lexeme within the page budget.

## Open questions

- **Q1. Parts across a cluster.** Whether the canonical lexeme's page should list a non-canonical member's forms and senses alongside its own, and how a `wikibase-form` value pointing at a member's form displays.
- **Q2. Lemma-based title resolution.** `Lexeme:run` is not a title today; whether a lemma resolver ([0029](0029-resolver-namespaces.md)) should disambiguate by language and category.
- **Q3. Senses as scope subjects in the builder** ([0062](0062-workspaces.md) §5): by-example over senses finds `P5137` patterns; whether the builder should offer part subject types.
- **Q4. Lexeme-specific notation** for languages without items (`mis`-coded lemmas), which Wikidata handles with special items; whether a Notation ([0048](0048-notation.md)) fits better here.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0004](0004-identity-clusters-and-equivalence.md) §1 | §4 | extends | 0004 A11 |
| [0005](0005-crate-organization.md) §2 | §10 | amends | 0005 A69 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §2 | amends | 0008 A29 |
| [0012](0012-api-requirements.md) §5 | §8 | extends | 0012 A46 |
| [0013](0013-postgres-storage.md) §5.6 | §5 | extends | 0013 A35 |
| [0014](0014-caches-and-search.md) §7 | §5 | extends | 0014 A17 |
| [0017](0017-entity-id-grammar.md) §1 | §4 | extends | 0017 A9 |
| [0041](0041-content-models.md) §3 | §3 | extends | 0041 A13 |
| [0043](0043-lua-modules.md) §7 | §9 | extends | 0043 A4 |
| [0047](0047-special-pages.md) §9 | §7 | extends | 0047 A10 |
| [0060](0060-scopes.md) §4 | §5 | extends | 0060 A4 |

## References

- [Extension:WikibaseLexeme](https://www.mediawiki.org/wiki/Extension:WikibaseLexeme); [Wikibase/DataModel](https://www.mediawiki.org/wiki/Wikibase/DataModel) §Lexeme; the extension's `docs/` on JSON and RDF
- [Wikidata:Lexicographical data](https://www.wikidata.org/wiki/Wikidata:Lexicographical_data) and the [RDF dump format](https://www.mediawiki.org/wiki/Wikibase/Indexing/RDF_Dump_Format#Lexemes)
- [OntoLex-Lemon](https://www.w3.org/2016/05/ontolex/)
- [0017](0017-entity-id-grammar.md) §1, [0004](0004-identity-clusters-and-equivalence.md) §1: the grammar and cluster rules this ADR extends
