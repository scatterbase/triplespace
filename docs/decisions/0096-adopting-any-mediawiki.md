# 0096. Adopting any MediaWiki, and converting Semantic MediaWiki and Cargo data

- **Status:** Proposed
- **Date:** 2026-10-29
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

**Any MediaWiki wiki may be adopted, with Wikibase or without it.** The procedure is 0035's ([05](../architecture/05-providers-and-ingest.md) §7): the source frozen, its full-history dump, a tenant created for it, its accounts under their own numbers, its pages under their source page and revision IDs, its files, and the floors.

A source without Wikibase has no entities. The job writes nothing to `local`, takes no `--counters`, and sets no entity floor. It sets the page, revision, log and user floors as for any source. Every page of the dump comes in the same job ([0035](0035-adopting-a-wikibase.md) A13), with the source's namespaces registered as §2 says.

### 2. The source's namespaces (extends 0035 §2; extends 0008 §2)

**The job registers the source's namespaces.** It reads the namespaces in the dump's `<siteinfo>`, and treats each one that holds a page of the dump by its number:

1. **A number the registry lists for a `pages` namespace** keeps its pages there, under that namespace's own rules. This covers the main namespace, User, Project, File, Interface, Template, Help (§3), Category and the rest of [10](../architecture/10-pages-and-content-models.md) §1.4. A page of a model the namespace does not allow is `deferred`, as now.
2. **A number the registry does not list** becomes a tenant `namespace` record ([0008](0008-namespaces-and-document-pages.md) §2 rule 4). The job writes it before the namespace's first page:
   - the source's number;
   - the source's local name, as its canonical name;
   - the case rule the dump gives;
   - kind `pages`, with subpages;
   - the models `wikitext`, `markdown`, `json`, `yaml` and `text`, with `wikitext` the default.

   Where the source has a talk namespace for it, its number becomes a `pages` namespace of the model `triplespace-talk`, and its talk pages become threads, as every adopted talk page does ([0090](0090-converting-talk-pages-comment-by-comment.md) §10). SMW's Concept (108) and Page Forms' Form (106) come in this way, and their pages are wikitext. A page of a model the instance does not have, such as SMW's `smw/schema`, is `deferred`, as now (Q1).
3. **A number the registry lists for a namespace that holds no pages** (`virtual`, `resolver` or `reserved`) cannot take the source's pages. They are archived (below), or else deferred.

**A name that is taken is renamed or archived.** A `namespace` record whose name equals another namespace's name or alias on the tenant is refused with `ts-namespace-name-taken` (10 §1.3 rule 4). SMW's Property (102) is the common case: its number is free, and its name is Wikibase's (122). The operator decides each case by number, with `--namespace`:

- **`--namespace 102="SMW property"`** registers the namespace under the new name. Its pages keep their number, their page IDs and their history. A link written `[[Property:Has author]]`, in any revision, still names namespace 122.
- **`--namespace 102=archive`**, or `archive:{prefix}`, registers nothing. It moves the namespace's pages into Project, each titled with the prefix (by default `Archive/`) and then its source title: `Property:Has author` becomes `Project:Archive/Property:Has author`. A page keeps its source page ID, every revision and every author; only its title changes. Its talk page comes as threads on the archived page's talk page. A link to the old title in another page does not follow it. Archiving is for a namespace of content the wiki is retiring.

`archive` applies as well to a namespace under rule 1 or 3 whose pages would otherwise be deferred.

**Conflicts are found before anything is written.** The survey pass reads the dump for the floors and the accounts before the job writes anything ([0035](0035-adopting-a-wikibase.md) §2). It also lists every namespace whose name is taken, and every one under rule 3 that holds pages. **The job is refused until `--namespace` decides every taken name**, so that a one-way adoption never stops halfway on a name. Pages under rule 3 left undecided are deferred and counted. The job record carries the decisions with its other parameters, and the namespace records are written under the job's `ts-config` (0035 §2, precondition 4).

### 3. The Help namespace (amends 0008 §2; extends 0038 §1)

**Namespace 12 holds pages: Help, with Help talk at 13.** It keeps MediaWiki's number and name, and it is a document namespace like Project ([10](../architecture/10-pages-and-content-models.md) §3.5):

- `first-letter` titles and subpages;
- the models `wikitext`, `markdown`, `json`, `yaml` and `text`, with `wikitext` the default;
- anyone with the right to edit may create a page, and no owner is recorded;
- its pages carry statements ([0038](0038-page-metadata-and-categories.md) §1) and may be sitelink targets.

Help talk holds composite talk pages, as the talk namespace of every implemented subject namespace does.

**Three things of Project's are not Help's,** being tied to Project as the site's own namespace:

- Help's local name stays `Help` and does not follow the site name. A tenant's `namespace` record may still rename it.
- Help allows no `triplespace-sprint` page, since a sprint is a subpage of a project ([0061](0061-sprints-and-tasks.md) §2).
- Help allows no `sanitized-css`, since the site-styles page stays in Project ([0055](0055-templatestyles-templatedata-and-page-properties.md) §4).

`namespaces.toml` is the authority. An adopted wiki's Help pages come into it with their IDs and history, under §2 rule 1.

### 4. Extension syntax in adopted pages (uses 0042 §5)

**The functions of Semantic MediaWiki, Cargo and Page Forms are registered as chips**, with each extension as the origin, in `wikitext-functions.toml`:

- **SMW:** `#ask`, `#show`, `#info`, `#concept`, `#subobject`, `#smwdoc`, `#set`, `#set_recurring_event`, `#declare` and `#property_link`;
- **Cargo:** `#cargo_declare`, `#cargo_attach`, `#cargo_store`, `#cargo_query`, `#cargo_compound_query`, `#cargo_display_map` and `#recurring_event`;
- **Page Forms:** `#default_form`, `#forminput`, `#formlink`, `#formredlink`, `#queryformlink`, `#arraymap`, `#arraymaptemplate`, `#autoedit`, `#autoedit_rating`, `#template_params`, `#template_display` and `#page_name_formula`.

SMW's behaviour switches `__NOFACTBOX__` and `__SHOWFACTBOX__` are `ignored`. None of the three extensions registers an extension tag. Each is listed as an `[[extension]]` in `version.toml`, so that `Special:Version` shows where its syntax comes from, as it does for every origin ([0077](0077-special-version.md) §8); it is not installed.

**A revision that used them renders with its calls shown as chips** ([0042](0042-template-expansion-and-parsoid.md) §5), and Parsoid and other clients tokenize them as parser functions. Old revisions keep the syntax they were written with, and that is how the history stays readable without any of the three extensions in Triplespace.

**SMW's inline annotations are not syntax the instance knows.** `[[Has author::Jane Doe]]` in an old revision renders as a link to a page titled `Has author::Jane Doe`, which does not exist. The current text no longer has them once its data has been converted (§5.5).

### 5. Converting Semantic MediaWiki and Cargo data (uses 0002 §8; uses 0063 §6)

#### 5.1 One conversion job, after adoption

**A wiki that kept data in SMW or Cargo is adopted first (§1), and its data then comes in as one job.** The job is a set of change sets ([0002](0002-source-graphs-and-mass-ingest.md) §8; [05](../architecture/05-providers-and-ingest.md) §3), submitted through the bulk-job endpoint by a subsidiary of the tenant's operators.

**The conversion is attributed to that job.** Its actor is the subsidiary, and nothing it writes is attributed to the source's editors. Their authorship stays in the adopted page history, which holds every revision that wrote an annotation or a stored value.

**The conversion is one-way.** Triplespace keeps no coexistence mode, and holds no SMW or Cargo code: no annotation parser, no store and no query engine. The converter is a script in `tools/`, outside the build. It reads files and calls the instance's public API, as `api_check.py` does. No crate depends on it, and it can be removed when no wiki needs it. Continuous derivation from pages the tenant does not control stays [0072](0072-template-mappings.md)'s.

#### 5.2 What the converter reads

**The converter reads the source's store, not the wikitext that fed it.** The store holds the values the source wiki computed, including those made by template expansion, parser functions and Lua that no reader of wikitext would see.

- **For SMW,** it reads the RDF export, by `dumpRDF.php` or its `ExportRDF` special page. This carries the properties and their declared types, the subjects, the subobjects and the values.
- **For Cargo,** it reads each table's declaration from the `#cargo_declare` in its template, and each table's rows by its `CargoExport` special page, `action=cargoquery` or a database dump of its tables.

It also reads the full-history dump the adoption read. That gives it each page's ID and newest revision ID, and the text of the edits of §5.5.

**A store can be rebuilt outside Triplespace.** A source that offers only an XML dump, such as a Miraheze dump, has its store rebuilt first. The dump is imported into a MediaWiki with the same extensions, and the store is rebuilt with `rebuildData.php` for SMW or `cargoRecreateData.php` for Cargo.

#### 5.3 The mapping

**The converter drafts a mapping from the store's declarations, and a person reviews it before it is applied**, as 0072 §6 drafts and never applies a template mapping. The draft is TOML. It lists every SMW property or Cargo field with its declared type, its count and sample values. It suggests:

- an existing property, local or foreign, where one fits, such as one an SMW property's `Imported from` or `Equivalent URI` names;
- otherwise a new local property, of the data type its declared type suggests.

Cargo fields of the same name in two tables are suggested as one property.

| Declared type | Data type |
|---|---|
| SMW `Page`; Cargo `Page` | `wikibase-item`: the subject of the page it names (below) |
| SMW `Text`, `Code`, `Keyword`, `Telephone number`; Cargo `String`, `Text`, `Wikitext`, `Wikitext string`, `Searchtext` | `string`, or `monolingualtext` where the mapping gives a language |
| SMW `Monolingual text` | `monolingualtext` |
| SMW `Number`; Cargo `Integer`, `Float`, `Rating` | `quantity` without a unit |
| SMW `Quantity`, `Temperature` | `quantity`, its unit an item from a value map ([13](../architecture/13-mirrored-pages.md) §7.4) |
| SMW `Date`; Cargo `Date`, `Datetime`, `Start date`, `End date` | `time` |
| SMW `URL`, `Annotation URI`, `Email`; Cargo `URL`, `Email` | `url`, an email address as a `mailto:` URL |
| SMW `External identifier` | `external-id` |
| SMW `Geographic coordinates`; Cargo `Coordinates` | `globe-coordinate` |
| Cargo `File` | `localMedia` ([12](../architecture/12-files-and-media.md) §7) |
| SMW `Boolean`; Cargo `Boolean` | `wikibase-item`: the mapping's items for true and false |
| SMW `Record` | The statement of its first field, with its other fields as qualifiers |
| SMW `Reference` | The statement of its value, with its other fields as the statement's reference |
| Cargo `List (…) of` a type | One statement per value, of that type's data type |

A declaration of allowed values, SMW's `Allows value` or Cargo's `allowed values`, becomes a one-of constraint on the property ([06](../architecture/06-statements-and-properties.md) §2).

**The mapping says what each kind of subject becomes.** A kind is an SMW category, a Cargo table, or the subobjects of an SMW property. The draft leaves each choice to the reviewer (Q2):

- **`page`:** the page itself. Its statements are page statements ([0038](0038-page-metadata-and-categories.md) §1; [06](../architecture/06-statements-and-properties.md) §5), addressed by the source page ID the adoption kept.
- **`item`:** a local item labelled with the page's title, and paired with the page by a sitelink to the tenant's own host (06 §4.6), so that `wbgetentities` and every Wikibase client reach it.
- **For subobjects, and for a Cargo table with several rows per page:** `item`, an item of its own, linked from the page's subject by a property the mapping names; or `qualifiers`, one statement on the page's subject with the row's other fields as its qualifiers.

**A `Page` value is the subject of the page it names.** Where that page has no subject in the mapping, the property's mapping says what the value becomes: a new item labelled with the title, a string, or nothing. Every such value is reported.

**Categories are not converted.** They stay categories, and a category mapping ([10](../architecture/10-pages-and-content-models.md) §5.4) projects statements from them where the tenant wants it to.

#### 5.4 Writing the data

**Properties are written first, then subjects, as `create-or-add`.** The match key is a local `external-id` property the mapping names. Its value is the source's identifier of the subject:

- the page ID, for a page's subject;
- `{page ID}#{subobject name}`, for an SMW subobject;
- `{table}/{_ID}`, for a Cargo row.

A failed or partial job can therefore be re-run, and it adds nothing twice ([05](../architecture/05-providers-and-ingest.md) §3.5).

**Every statement cites the revision that stated it.** Its reference carries the properties the mapping's `reference` names, as an extraction source's does (13 §6.2):

- the URL `index.php?oldid={revision ID}` of the page's newest revision in the dump. It resolves on the tenant, because adoption kept the source's revision IDs (05 §7.3);
- the date of the dump.

**The values are the store's, as of the dump.** No statement history is reconstructed. Earlier values are in the adopted page history only.

#### 5.5 Rewriting the pages

**After the data job, the converter makes one edit to each page that carried storage or query syntax.** The edits are made by the same subsidiary, with a summary naming the job. Every revision before that edit is the source's, untouched.

- `#set`, `#subobject`, `#declare`, `#cargo_store`, `#cargo_attach` and `#cargo_declare` calls are removed. Their data is in statements, and the declarations are in the mapping.
- Inline annotations are unwrapped. `[[P::V]]` becomes `[[V]]`, and `[[P::V|text]]` becomes `[[V|text]]`, where `P` is a `Page` property. For any other type, the annotation becomes `V` or `text` as plain text. An annotation written to be hidden (`[[P::V| ]]`) is removed.
- An `#ask`, `#show` or `#cargo_query` whose query the converter can express in SPARQL becomes a `Query:` page, with a `{{#query:}}` block in its place ([0063](0063-query-namespace.md) §6; [15](../architecture/15-structured-pages.md) §5). Any other query stays, as a chip (§4), and is reported (Q3).
- Templates are edited the same way. A template whose output depended on queried data is reported, to be ported by hand.
- Form pages, and Page Forms calls in other pages, are left as they are, as chips.

#### 5.6 The report

The converter reports:

- per property or field: the statements written, and the values it could not read, with examples;
- the `Page` values that named a page with no subject;
- the pages it rewrote;
- the queries it translated, and those it left;
- the templates to port by hand.

### 6. Crates (extends 0005 §2)

| Crate | Change |
|---|---|
| `triplespace-cli` | `adopt` of a source without Wikibase (§1); the source's namespaces as tenant `namespace` records, and `--namespace` renames and archives, with the survey's list of conflicts (§2) |

§3 and §4 change registry data, which `scatter-titles` and `scatter-wikitext-expand` embed. The converter of §5 is a script in `tools/` and no crate.

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

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §6 | extends | 0005 A109 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §2 | extends | 0008 A46 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §3 | amends | 0008 A47 |
| [0035](0035-adopting-a-wikibase.md) §1, §2 | §1, §2 | extends | 0035 A15 |
| [0038](0038-page-metadata-and-categories.md) §1 | §3 | extends | 0038 A16 |

## References

- Semantic MediaWiki: `i18n/extra/SemanticMediaWiki.magic.php` (its parser functions and behaviour switches) and `extension.json` (namespaces 102, 103, 108, 109, 112 and 113; the `smw/schema` content model), version 7.4.0-alpha; `dumpRDF.php`, `rebuildData.php` and the `ExportRDF` special page.
- Cargo: `Cargo.i18n.magic.php` (its parser functions), version 3.9.4; the `CargoExport` special page, `action=cargoquery` and `cargoRecreateData.php`.
- Page Forms: `languages/PFMagic.php` (its parser functions) and `extension.json` (namespaces 106 and 107), version 6.0.11.
- [Archive Team: Miraheze](https://wiki.archiveteam.org/index.php/Miraheze): Miraheze's monthly XML and image dumps on the Internet Archive.
