# 0096. Adopting any MediaWiki, and converting Semantic MediaWiki and Cargo data

- **Status:** Proposed
- **Date:** 2026-10-29
- **Updated:** 2026-10-30 (A1)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0035](0035-adopting-a-wikibase.md), [0038](0038-page-metadata-and-categories.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0042](0042-template-expansion-and-parsoid.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [0063](0063-query-namespace.md), [0072](0072-template-mappings.md), [0077](0077-special-version.md), [0090](0090-converting-talk-pages-comment-by-comment.md)

## Context

[0035](0035-adopting-a-wikibase.md) lets a tenant adopt a MediaWiki *Wikibase*: the source frozen, its full-history dump, its accounts under their own numbers, its entities under the IDs the source minted, and, since 0035 A13, every page of the dump under its source page and revision IDs. Nothing in it needs the source to have entities, but every wiki it was written for has them. `triplespace-cli adopt` counts a page in a namespace the registry does not list as `deferred` ("namespace N is not registered here"), although [0008](0008-namespaces-and-document-pages.md) §2 rule 4 says that an adopted wiki keeps the numbers it gave its namespaces.

Many MediaWiki wikis without Wikibase keep structured data with **Semantic MediaWiki** (SMW) or **Cargo**. Both take it from page wikitext. SMW reads inline annotations (`[[Has author::Jane Doe]]`) and the `#set` and `#subobject` functions, usually written inside templates; Cargo reads `#cargo_store` calls in templates whose table `#cargo_declare` defines. Each keeps a store derived from the pages, SMW's exported as RDF and Cargo's as database tables, and queries it with `#ask` or `#cargo_query`. Values computed by template expansion, parser functions and Lua reach the store without being in any page's text. SMW adds namespaces of its own: Property (102), Concept (108) and `smw/schema` (112), each with a talk namespace. Page Forms, often used with either, adds Form (106). SMW's Property namespace has the name of Wikibase's (122).

[0072](0072-template-mappings.md) maps template calls to statements continuously, for wikis the tenant does not control, and drafts a mapping from a template's `#cargo_declare` (§6). It recognizes no SMW annotation, no `#set` or `#subobject`, and no value made by expansion. [0074](0074-publishing-a-scope-to-an-external-wiki.md) publishes a scope into a Cargo table, the other direction.

MediaWiki's Help namespace (12) was reserved by [0008](0008-namespaces-and-document-pages.md) §2, with help content going in Project, so a source wiki's Help pages could be neither adopted nor imported.

Miraheze publishes a monthly XML and image dump of each public wiki on the Internet Archive. The dumps carry neither SMW's store nor Cargo's tables.

### Direction

James's direction, from the design discussion of 2026-10-28:

- **"Let's say someone wanted to switch to Triplespace from Cargo or Semantic MediaWiki. I don't want to implement adapters for those in Triplespace. Rather, I'd like to develop a conversion utility that can take exports from those wikis and convert them into Triplespace's native shape."**
- **"I am not sure it's worth building a significant legacy support stack for Semantic MediaWiki within Triplespace."**
- **"I agree Cargo shouldn't also have a coexistence mode. With Cargo and Semantic MediaWiki, the use case here is a one-way conversion to Triplespace. I agree that attribution to the conversion should go to the conversion job. There should be some way to represent the historical content and historical authorship, even if things appear "broken". I think tools/ is a reasonable place for this code to live."**

And from the discussion of 2026-10-29:

- **"I support generalizing the adoption process to include any MediaWiki. For novel namespaces, it should be sufficient to just register those namespaces; for Triplespace I intentionally picked namespace numbers that would not overlap. If there is a conflict, either the namespace is given a new name to avoid conflict, or if it's a namespace of content you're planning on retiring anyway, you import it as "Project:Archive/Property:LegacySMWProperty" or something like that. Proposed registry data additions are fine."**
- **"Could we take the opportunity now to recognize the Help namespace as a namespace that works basically the same way as the Project namespace?"**

## Decision

### 1. Adopting any MediaWiki (extends 0035 §1 and §2)

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.1.*

### 2. The source's namespaces (extends 0035 §2; extends 0008 §2)

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.2, §7.6; [10](../architecture/10-pages-and-content-models.md) §1.3.*

### 3. The Help namespace (amends 0008 §2; extends 0038 §1)

*Current text: [06](../architecture/06-statements-and-properties.md) §5.1; [10](../architecture/10-pages-and-content-models.md) §1.4, §3.8.*

### 4. Extension syntax in adopted pages (uses 0042 §5)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §2.1.*

### 5. Converting Semantic MediaWiki and Cargo data (uses 0002 §8; uses 0063 §6)

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.7.*

#### 5.1 One conversion job, after adoption

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.7.*

#### 5.2 What the converter reads

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.7.*

#### 5.3 The mapping

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.7.*

#### 5.4 Writing the data

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.7.*

#### 5.5 Rewriting the pages

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.7.*

#### 5.6 The report

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.7.*

### 6. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **Continuous derivation**: an extraction source over the adopted pages ([0072](0072-template-mappings.md)), so that SMW and Cargo pages stay the source of their data. It would need SMW's annotations, `#set` and `#subobject` read in Triplespace, and would still miss values made by expansion. James ruled out coexistence: the conversion is one-way.
- **Adapters in Triplespace** for SMW's store and Cargo's tables. Ruled out by James's direction; the converter is a client of the public API instead.
- **Reading values from wikitext rather than the store.** It sees no value made by expansion, parser functions or Lua. The store holds what the wiki showed.
- **A page import rather than adoption** ([10](../architecture/10-pages-and-content-models.md) §6). It keeps neither page nor revision IDs nor accounts, so the references of §5.4 could not cite source revisions.
- **Rewriting the dump before adoption,** to rename namespaces or strip syntax. The history would no longer be the source's. Adoption takes the decisions as parameters, and the rewrite of §5.5 is one edit on top.
- **Registering SMW's namespaces in `namespaces.toml`.** Only the wikis that used SMW need them, so they are tenant records under rule 4, as James asked.
- **Relocating Help pages into Project.** James chose a Help namespace that works as Project does.

## Consequences

- **Any MediaWiki can become a tenant** with its page, revision, log and user IDs, its history and its accounts.
- **Triplespace has no SMW or Cargo code.** The conversion lives in a script in `tools/`, and the only trace of the extensions in the instance is registry data: their functions as chips.
- **History looks broken where it used the extensions, and is complete.** Old revisions show chips for their functions and red links for their annotations. Every revision is there, under its author.
- **Converted data is attributed to the conversion,** and each statement cites the source revision that stated it, which resolves on the tenant.
- **Every tenant gains a Help namespace,** listed in `meta=siteinfo` with its talk namespace.

## Open questions

- **Q1. Pages of models the instance lacks.** SMW's `smw/schema` pages, and a CSS or JavaScript page outside the Interface namespace, are deferred, as now. The open question is whether adoption should keep such pages as `text`, so that the history is complete.
- **Q2. Pages or items.** Whether the drafted mapping should propose `page` or `item` for a kind of subject by default, or leave every choice open as §5.3 does.
- **Q3. Which queries translate.** Which subset of `#ask` conditions and printouts, and of `#cargo_query`'s `where`, `join on` and `group by`, the converter expresses in SPARQL, and which result formats it carries to a `{{#query:}}` block.

## References

- Semantic MediaWiki: `i18n/extra/SemanticMediaWiki.magic.php` (its parser functions and behaviour switches) and `extension.json` (namespaces 102, 103, 108, 109, 112 and 113; the `smw/schema` content model), version 7.4.0-alpha; `dumpRDF.php`, `rebuildData.php` and the `ExportRDF` special page.
- Cargo: `Cargo.i18n.magic.php` (its parser functions), version 3.9.4; the `CargoExport` special page, `action=cargoquery` and `cargoRecreateData.php`.
- Page Forms: `languages/PFMagic.php` (its parser functions) and `extension.json` (namespaces 106 and 107), version 6.0.11.
- [Archive Team: Miraheze](https://wiki.archiveteam.org/index.php/Miraheze): Miraheze's monthly XML and image dumps on the Internet Archive.

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§6
- **Summary:** The Decision's current text now lives in the architecture chapters [05](../architecture/05-providers-and-ingest.md), [06](../architecture/06-statements-and-properties.md), [10](../architecture/10-pages-and-content-models.md), [11](../architecture/11-rendering-templates-and-modules.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
