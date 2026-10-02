# 0038. Page metadata, legacy categories and articles

- **Status:** Proposed
- **Date:** 2026-09-29
- **Updated:** 2026-10-01 (A8)
- **Author:** James Hare / Claude Opus
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0003](0003-statement-ui.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0019](0019-discussions.md), [0026](0026-sitelinks.md), [0029](0029-resolver-namespaces.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0009](0009-keyed-entity-types-and-domain.md), [0016](0016-permissions-and-access-control.md), [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0030](0030-edit-filters.md), [0031](0031-property-constraints.md), [0032](0032-sparql-update-stream.md), [0035](0035-adopting-a-wikibase.md), [0039](0039-files-and-media.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0049](0049-boards.md)

## Context

On Wikipedia, metadata about **concepts** is rich: Wikidata gives every subject typed, referenced, queryable statements. Metadata about **pages** is stuck with systems that are decades old. Categories mix three different kinds of fact into one flat list of names. Short descriptions are a template read back as a page property. Quality assessments are templates on talk pages. And the one piece of page quality Wikidata does record, the featured-article badge, is stored on the concept's sitelink, because the page has nowhere to keep it.

Triplespace can give pages first-class metadata from the start. It already has most of the pieces: page IDs and entity page IDs share one sequence ([0015](0015-record-format-and-partition-registry.md) §2), pages live in a logged partition with full history ([0008](0008-namespaces-and-document-pages.md) §4), and the statement machinery (the statement UI of [0003](0003-statement-ui.md), constraints of [0031](0031-property-constraints.md), filters of [0030](0030-edit-filters.md), RDF and search) does not depend much on what the subject is.

[0008](0008-namespaces-and-document-pages.md) left two questions that bear on this: whether category links should ever become data, and, until the 2026-09-27 amendment reserved it, what the main namespace is for. Categories matter because **the main use case for legacy categories is ingesting page revisions from MediaWiki**, where category membership, sort keys and `__HIDDENCAT__` are stored in the page text. And pages that are paired with items are articles, which MediaWiki keeps in namespace 0.

James's direction, from the design discussion of 2026-09-29:

- Pages get first-class metadata, served over the Triplespace REST API only. No legacy MediaWiki or Wikibase API consumer expects statements on a wiki page, so page statements get no entity ID.
- **Legacy categories are defined only in wikitext.** A category that could also be defined as data would need reconciliation between two sources. The way forward is to migrate from categories to Triplespace-native page metadata and entity concepts.
- Configured mappings can turn existing categories into page statements, so a tenant gets the native shape without first rewriting its pages. For now, mappings produce statements about the page only. MediaWiki categories do cross into facts about the subject, and that boundary may be crossed later, once there is a good UI for telling page metadata from concept metadata.
- Inferring statements from template calls is premature. The focus is inferring triples from categories.
- A sitelink from an item to a page on the tenant makes the page's title a way to reach the item. Data about the underlying concept and data about the page are displayed separately, to reinforce that they describe two different things.
- Articles go in namespace 0, and its talk namespace comes with it.
- Threads get statements too. A thread's status is naturally a statement about the thread.

## Decision

### 1. Pages carry statements (extends 0008 §4)

*Changed by A2, A3, A5.*

**Which pages.** Every document page (the `document` namespaces of [0008](0008-namespaces-and-document-pages.md) §1: main, `User`, `Project` and `Category`), every thread ([0019](0019-discussions.md) §1) and every board ([0049](0049-boards.md) §1), whose statements describe the board, not its threads. Not talk pages, which are composite and have no records of their own, and not entity views, whose subjects are entities with statements of their own.

**Records.** A page's statements are written as change sets: payload type `scatter:v0/changeset`, appended to the tenant's `pages` partition and keyed by the page ID, beside the page's `page` or `thread` records. A change set on a page may add, change and remove statements. It may not carry labels, descriptions, aliases or sitelinks; a page has a title, and its sitelinks are held by items (§6). Like every record in `pages`, it takes a revision ID ([0013](0013-postgres-storage.md) §6) and needs a base offset ([0006](0006-log-integrity-and-erasure.md) §8). So **a page has one history**, in which text revisions and statement revisions interleave in the order they were made; a file page's uploads are records in the same partition, keyed by the page ID, so file versions share that history too ([0039](0039-files-and-media.md) §2).

**The subject is the page ID.** Statements use the same properties and data types as entity statements, local and mirrored. Where a subject has to be written as a string, in a statement ID or in a `view` column (§10), it is the page ID in decimal: statement IDs take the Wikibase form of [0009](0009-keyed-entity-types-and-domain.md) §3 as `{page ID}$<UUID>`. A string of digits alone never parses as an entity ID under [0017](0017-entity-id-grammar.md) §1, since every local, foreign and keyed ID starts with a letter, so the two kinds of subject cannot be confused.

**No entity ID, except on File pages.** Page statements have no `M`-style or other entity ID, are not returned by `wbgetentities`, and cannot be edited through the Wikibase Action API modules. They are served by the REST routes of §13. In `prop=revisions`, a statement revision appears as MediaWiki shows a revision that changed only a secondary slot: the main text is unchanged, and the summary describes the change. File pages are the exception ([0041](0041-content-models.md) §6–7): their statements have the MediaInfo ID `M{page ID}` and are read and written through `wbgetentities` and the Wikibase statement modules, as on Commons, because tools written for Structured Data on Commons expect that contract; they appear in the `mediainfo` slot (`wikibase-mediainfo`) in `prop=revisions`, and terms and sitelinks on `M` IDs are refused with `not-supported` until page terms are settled (Q4).

**Moderation follows the page.** Protection and deletion are ACLs on the page ([0023](0023-moderation.md) §4), and they cover its statements: a protected page's statements need the same right as its text, and a deleted page's statements leave every view with it. Erasure and hiding apply to statement records as to any record.

### 2. A page's statements: asserted and projected

A page's resolved statements are the union of two kinds:

| Kind | Source | Editable |
|---|---|---|
| **Asserted** | Change sets in `pages` (§1) | Yes, in the Page data tab (§7) and the REST routes (§13) |
| **Projected** | Category mappings (§5) and a thread's status (§9) | No. Changing the source changes them |

**Projected statements are derived facts,** like constraint violations ([0031](0031-property-constraints.md) §2): never records, removed when their source goes away, rebuilt with the view. The provenance response ([0003](0003-statement-ui.md) §6) names their source, such as "from Category:Articles needing cleanup" or "set by Example in reply 1234", in a `derived` entry in place of a graph.

**Identical statements fuse.** If an editor asserts a statement that a mapping also projects, the two are fused as statements from two graphs are ([0004](0004-identity-clusters-and-equivalence.md) §8), and the provenance lists both. Removing the category leaves the asserted statement. They are two independent claims, not one fact with two definitions, so nothing has to be reconciled.

**Constraints apply to pages.** The constraint projection checks page statements as it checks entity statements ([0031](0031-property-constraints.md) §1). Wikidata's *allowed entity types* constraint (Q52004125) lists its allowed types as items; two roles, `subject-page` and `subject-thread`, name the items that stand for "document page" and "thread", so a tenant can mark a property as page-only or keep it off pages. Like every constraint, it reports and never refuses.

### 3. Legacy categories are defined only in wikitext (amends 0008 §8; settles 0008 Q5)

*Changed by A4.*

**Membership is read from the text.** A page is in a category when the **latest revision** of a page whose content model is `wikitext` contains a category link:

| Syntax | Meaning |
|---|---|
| `[[Category:Name]]` | The page is in `Category:Name`, with the page's default sort key |
| `[[Category:Name\|key]]` | The same, with `key` as the sort key |
| `{{DEFAULTSORT:key}}` | The page's default sort key. `DEFAULTSORTKEY` and `DEFAULTCATEGORYSORT` are aliases, as in MediaWiki. This is the one parser function the projection recognises |
| `__HIDDENCAT__` | On a page in `Category`: the category is hidden |
| `[[:Category:Name]]` | A link *to* the category page, not membership, as in MediaWiki. The leading-colon link is added to the subset of 0008 §8 |

Names are normalized by the `Category` namespace's `first-letter` normalizer (§4). A category page's own category links make it a subcategory of those categories.

**Categories are a projection, never records.** Membership is a `view` table (§10), like MediaWiki's `categorylinks`, rebuilt from the text. There is no API that adds a page to a category except editing its text, and there is no statement that means "member of a category". Removing a link from the text removes the membership at the next projection. This is the rule that keeps reconciliation out: **a category is defined in exactly one place.**

**Only `wikitext` has categories.** Pages in `markdown`, `json`, `yaml` and `text`, and threads, are never in a category. Native pages start with native metadata.

**A category needs no page.** A page can be in a category whose page does not exist, as on MediaWiki. The link renders red and the category still has members.

**Templates are not parsed while expansion is off.** For a tenant with `wikitext.expansion` off, categories that templates emit reach the text only through the flattening revision of [0008](0008-namespaces-and-document-pages.md) §9 step 3, whose `action=expandtemplates` runs on the source wiki and writes the category links and `__HIDDENCAT__` into the flattened text. A template call added after an import renders as a placeholder (0008 §8) and emits nothing, and categories MediaWiki's parser adds on its own, such as tracking categories for broken file links, are in no text. With expansion on ([0042](0042-template-expansion-and-parsoid.md) §9), membership is read from the **expanded** text: categories that templates emit count, `<includeonly>` categories reach transcluding pages, and expansion adds MediaWiki's tracking categories for its own conditions.

**Rendering.** The foot of a page lists its categories as links to their category pages, replacing 0008 §8's plain-text list. Hidden categories are listed separately, collapsed. The source text is still never rewritten.

### 4. The `Category` namespace (amends 0008 §2)

Category (14) is implemented as a `document` namespace. This is the exception the numbering policy's first rule allows ("not implemented unless an ADR says so").

| Field | Value |
|---|---|
| Normalizer and case | `first-letter` |
| Content models | `wikitext` only, since a category page's `__HIDDENCAT__` and parent categories are wikitext (§3) |
| Subpages | No |
| Creation | Anyone with `edit`, as `Project` (0008 §7) |
| Talk | Category talk (15), `composite`, under the policy's second rule |

**A category page shows its description and its members.** The members are listed in two groups, subcategories and pages, each sorted by sort key. The sort order is the `category.collation` `site` setting, whose default is MediaWiki's `uppercase`. Moving a category page does not move its members, as on MediaWiki: the old name keeps its members until their text changes.

### 5. Category mappings (extends 0015 §3)

**A mapping turns membership into a page statement.** Mappings are tenant configuration, `config` records of a new kind, `category-mapping`, whose code is the mapping's name:

| Field | Meaning |
|---|---|
| `category` or `pattern` | An exact category name, or a pattern over category names with named captures (below) |
| `property` | The statement's property, local or mirrored |
| `value` | The main value, as a canonical JSON data value, or a capture |
| `qualifiers` | Optional: a list of `{property, value}`, where a value may be a capture |

Written as TOML for illustration, with illustrative local IDs:

```toml
[mapping.cleanup]
pattern    = "Articles needing cleanup from {when:month-year}"
property   = "P40"                      # maintenance tag
value      = { entity = "Q812" }        # cleanup
qualifiers = [ { property = "P41", value = "{when}" } ]   # point in time

[mapping.cleanup-undated]
category = "All articles needing cleanup"
property = "P40"
value    = { entity = "Q812" }
```

**Captures** are written `{name:type}`. Four types exist: `year` and `month-year` (time values with precision 9 and 10, reading month names in the site's content language), `date` (precision 11) and `text` (a string). A category name whose capture does not parse does not match. This covers MediaWiki's dated maintenance categories, which are the common case.

**Semantics.**

- **Each membership that matches a mapping yields one projected statement** (§2) on the member page. A page in two matching categories gets two statements, which fuse if they are identical.
- **Hidden categories are mapped like any other.** Most maintenance categories are hidden.
- **Statement IDs are name-based UUIDs** of the page ID, the mapping name and the category name, following the rule of [0002](0002-source-graphs-and-mass-ingest.md) §8.4 for sources with no IDs of their own. They stay stable across rebuilds.
- **A change to a mapping re-runs it** over the members of every category it matches, as a job with progress, as a constraint change re-checks a property ([0031](0031-property-constraints.md) §2).
- **Mappings produce statements about the page only.** A category such as "1952 births" is a fact about the subject, and projecting it onto the linked item would let an article's text change the item's data. That boundary is not crossed until there is a UI that makes the difference plain (Q1).

**Migrating a page off a category is optional.** A page can keep its category and still have the native statement. When a tenant wants the text free of a category, the **make-it-real job** does it per mapping: for each member page, it appends a change set asserting the statement and a text revision removing the category link, in one transaction with one base check, grouped in history under the job ([0010](0010-site-ui.md) §1, principle 3). Because the asserted and projected statements fuse, there is no moment at which the page shows two versions of the fact.

### 6. Pages paired with items (amends 0026 §1; extends 0026 §2)

*Changed by A7.*

**A sitelink to the tenant's own host targets a page.** A tenant is served at one or more hosts ([0018](0018-tenants.md) §1, §9). A sitelink in the local graph whose host is one of them is stored by **page ID**, not by URL. It is written as any sitelink is written, by site ID and title or by URL ([0026](0026-sitelinks.md) §2), and the title is resolved to a page ID at write time. A title with no page is refused with `ts-sitelink-no-page`, as Wikibase refuses a link to a missing page. The URL and title in the canonical JSON are derived from the page's current title, so **a move does not break the link** and no record is written when a page moves.

**Targets** are document pages in any document namespace. Threads and talk pages cannot be sitelinked, and nor can a title whose primary is a page repository's page, which is refused with `ts-sitelink-foreign`, because a foreign page's ID changes when it is forked ([0052](0052-page-repositories-and-title-inheritance.md) §6).

**The invariants of 0026 §2 give a one-to-one pairing.** For the per-host rule, all of a tenant's own hosts count as one host, so an item has at most one link to the tenant's pages; and a page ID belongs to at most one item, as a URL does. So a paired page leads to one item and the item back to one page, with no disambiguation.

**The tenant's own site alias.** A tenant registers a `site-alias` for its own host ([0026](0026-sitelinks.md) §2) with a site ID of its choosing. Then `wbgetentities&sites={site ID}&titles=Douglas Adams`, `wbsetsitelink`, `Special:ItemByTitle` and the sitelink resolver ([0029](0029-resolver-namespaces.md) §5) all reach the item from the page's title. **The title becomes a way to reach the item**, with no new mechanism.

**Mirrored sitelinks to the tenant's host** are URLs in their mirror graph. The sitelink projection resolves such a URL to a page by title when it can, and the reconciliation of [0026](0026-sitelinks.md) §4 (local wins per host) decides between it and a local link.

**A deleted page's sitelink leaves the resolved view** while the page is deleted, as a denied host's does (0026 §3), and comes back when the deletion is retired. The record in the local graph is untouched.

### 7. One subject per frame (extends 0010 §1 and §2)

**A frame's tabs show data only about what its identity line names.** This is added to 0010's first principle. An item's statements are never drawn in a page's frame, and a page's statements never in an item's, so a reader always knows which thing a statement is about.

| Page kind | Tabs |
|---|---|
| Document page | Read, Edit, Page data, History, Links here |
| Thread | The thread (0019 §8), Page data, History |

**Page data** shows the page's resolved statements in the statement UI of [0003](0003-statement-ui.md), with the page as subject. Projected statements carry a source chip ("From Category:…", "Set in reply …") and no edit controls. Their value menu offers **Remove category**, which opens the editor on the text, or for a status, a link to the thread. The page's categories (§3) are listed here too, read-only, with a link to edit the text.

**Crossing between a page and its item is a visible move.** A paired page's identity line carries a subject link, "About: Douglas Adams (Q42)", which opens the item in its own frame. The item's identity line carries "Article: Douglas Adams", which opens the page. The **About this page** panel of 0010 §4 keeps its role and gains the page-statement count.

### 8. Namespace 0 holds articles (amends 0008 §2)

**The main namespace is implemented as a `document` namespace.** It is the second exception to the numbering policy's first rule. Articles, including every page imported from a wiki's main namespace, go here.

| Field | Value |
|---|---|
| Normalizer and case | `first-letter` |
| Content models | `wikitext` (default), `markdown`, `json`, `yaml`, `text` |
| Subpages | No, as MediaWiki's default for namespace 0 |
| Creation | Anyone with `edit` |
| Talk | Talk (1), `composite` |

**A bare title is a main-namespace title, in every content model.** With articles in namespace 0, an unprefixed title means a main-namespace page, as on MediaWiki: `[[Q42]]` links to the page titled `Q42`, in wikitext and in markdown alike, and an entity is linked with its namespace, `[[Item:Q42]]`, as the subset of 0008 §8 already shows. This amends [0019](0019-discussions.md) §5, where `[[Q42]]` in a markdown post meant the entity; one rule for every content model is easier to explain than a link whose target depends on the page's format. Any valid title may be created, including one that looks like an entity ID: `Item:Q42` and `Q42` are different titles, as on any Wikibase that keeps items out of the main namespace.

**ID shortcuts are not title resolution.** `/resolve` and the search box's "Go to" ([0010](0010-site-ui.md) §3) still try entity IDs, keyed IDs and resolver strings first, and main-namespace titles last ([0029](0029-resolver-namespaces.md) §6). When a main-namespace page has the same title as the ID, the search box offers it as the next suggestion ("Page titled Q42"), and `/resolve` returns it in an `also` field beside the entity.

### 9. Threads (extends 0019 §6; amends 0019 §7)

**Threads carry statements as pages do** (§1): change sets keyed by the thread's page ID. Priority, component or an assignee are ordinary asserted statements.

**A thread's status is a projected statement.** 0019 §6 stays the one place a status is set: by a post, in public, by a named actor. A new role, `thread-status`, names the property that represents it. The property's data type is `string`, and its value is the status name from the `thread-status` registry. Statuses are configuration records, not items, and the UI shows the registry's label. When the role is bound, the projection writes one statement per thread from its latest status-bearing post, with provenance naming the post and its author. Asserting a statement with that property on a thread is refused with `ts-derived-property`, because a second definition of status is exactly what this ADR avoids. When the role is unbound, no statement is written and status works as 0019 §6 describes.

0019 §7's rule that nothing about threads enters the main or resolved graph now excepts thread statements, which are output as page statements are (§11).

### 10. Storage (extends 0013 §5.6 and §7)

```sql
CREATE TABLE view.page_statements (             -- the resolved statements of a page (§1–2)
  page_id bigint PRIMARY KEY,
  version bigint NOT NULL,                      -- bumped on every change; the cache and ETag key
  statements bytea NOT NULL,                    -- canonical JSON statements, asserted ∪ projected, fused; compressed
  generation integer NOT NULL DEFAULT 0         -- bumped on erasure and hiding (0014 §5)
);

CREATE TABLE view.page_category (               -- categorylinks (§3)
  page_id bigint NOT NULL, category text NOT NULL,   -- the normalized name, without the prefix
  sortkey text NOT NULL,
  PRIMARY KEY (page_id, category)
);
CREATE INDEX page_category_members ON view.page_category (category, sortkey, page_id);

CREATE TABLE view.category (                    -- categoryinfo (§4)
  title text PRIMARY KEY, page_id bigint,       -- NULL when the category has no page
  pages integer NOT NULL, subcats integer NOT NULL,
  hidden boolean NOT NULL DEFAULT false
);
```

**Subjects in existing tables.** `view.statement_assertion.entity_id`, `view.entity_ref.source_id` and `view.constraint_violation.entity_id` hold a page subject as the page ID in decimal (§1); in `statement_assertion`, `graph` is `pages` for an asserted statement and `derived` for a projected one. So "Links here" on the item for *cleanup* lists the pages whose statements use it, and constraint reports include pages. Page statements are **not** written to `view.identifier`, `view.match_key` or `view.value_key`, so a resolver ([0029](0029-resolver-namespaces.md)) or a match key never lands on a page.

**Projection order.** `page_category` and `category` run in step 2, with `page`, since they read the latest text. The mapping and status projections, and the resolution of `page_statements`, run in step 4. The search and RDF projections pick up pages in step 7. A text edit's own categories and mapped statements are among its own rows under the synchronous budget, so an editor who adds a category sees the mapped statement on reload.

### 11. RDF (amends 0001 §3 and 0008 §10; uses 0032 §2)

*Changed by A8.*

**Page statements are in the main graph,** in Wikibase's statement shape (`p:`, `ps:`, `pq:`, `prov:wasDerivedFrom`, the truthy direct claims and statement nodes named from the statement ID), with the page's node `{base}/page/{page ID}` of 0008 §10 as subject. That node gets `a schema:WebPage`, `schema:name` (the current title), `schema:url` and, where the page sets a short description, `schema:description` from its `wikibase-shortdesc` page property ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6). For a page paired with an item (§6), the sitelink's `schema:Article` node is this same page node, not the URL, so a query can join an item's article with the article's statements. The URL remains available through `schema:url`.

0001 §3 said nothing is added to the main graph. This adds subjects, not vocabulary: every predicate is Wikibase's or schema.org's, and a Wikibase consumer that reads `p:`/`ps:` reads page statements unchanged. Projected statements are output like asserted ones. Categories themselves are not RDF; the triples they produce come through mappings. The SPARQL Update stream ([0032](0032-sparql-update-stream.md)) carries page statements' deltas like any other.

### 12. Search (extends 0014 §7)

The `pages` index gains two fields:

- `categories`, the page's category names, so that CirrusSearch's `incategory:` works;
- `statement_keywords`, as on `entities`, so that `haswbstatement:P40=Q812` finds the pages tagged for cleanup, whether the tag is asserted or projected.

### 13. API (extends 0012 §4 and §5)

*Changed by A3.*

**REST**, under `rest.php/triplespace/v0`. The statement routes follow the Wikibase REST API's statement routes, with the page ID in place of an entity ID:

| Route | Meaning |
|---|---|
| `GET`, `POST /page/{pageid}/statements` | The resolved statements; add an asserted statement |
| `GET`, `PUT`, `PATCH`, `DELETE /page/{pageid}/statements/{statement_id}` | One statement. Writes to a projected statement are refused with `ts-derived-statement` |
| `GET /page/{pageid}/provenance` | Extended with the statements' provenance, including `derived` entries (§2) |
| `GET /page/{pageid}/categories` | Categories with sort keys and the hidden flag |
| `GET /category/{title}/members?type=&from=` | Members by sort key |
| `GET`, `PUT`, `DELETE /category-mappings/{name}`, `GET /category-mappings` | Mappings, as configuration records |
| `POST /category-mappings/{name}/make-real` | Starts the make-it-real job (§5) |

**Action API.** `prop=categories` with `clprop=sortkey|hidden` and `clshow`; `list=categorymembers` and `generator=categorymembers` with `cmtype` and `cmsort`; `prop=categoryinfo`; `list=allcategories`. `wbgetentities` by `sites` and `titles` finds a page's paired item (§6). Page statements are not in the Action API, except File pages', which are MediaInfo entities ([0041](0041-content-models.md) §7; §1).

### 14. Permissions and filters

Asserting, changing and removing a page's statements needs `edit` on the page ([0016](0016-permissions-and-access-control.md) §5), subject to the page's ACLs (§1); for a thread, the right that posting needs. Category mappings are tenant configuration and need the configuration right every `config` kind needs. Edit filters ([0030](0030-edit-filters.md) §2) see a page's change sets in the change-set context, with the subject's kind (`entity` or `page`) and namespace exposed, so a filter can treat page metadata differently from item data.

### 15. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with every change this section listed. The table this section first gave is in A1.

## Consequences

- **Pages and concepts have metadata of the same kind, kept apart.** Page statements use the same properties, UI, constraints, search and SPARQL as item statements, and the display rule (§7) keeps each on its own subject.
- **Legacy categories keep working with no second source of truth.** Imported pages keep their categories exactly as their text defines them, `prop=categories` and `list=categorymembers` work, and mappings give them native form without touching the text.
- **A category change in the text changes data.** Removing `[[Category:Articles needing cleanup]]` removes a statement. That is intended, and the statement's provenance says where it came from.
- ~~**Template-emitted categories are frozen at import.** After the flattening revision, adding `{{Cleanup}}` does not categorize a page. Editors add the category link, or the native statement.~~ *Only with expansion off; with a tenant's expansion on, `{{Cleanup}}` categorizes the page (A4).*
- **Namespace 0 now holds pages.** Two numbers leave the reserved set (0 and 14) and their talk namespaces (1 and 15) are enabled.
- **The main graph has non-entity subjects.** Tools that assume every subject of `p:` is a `wikibase:Item` or `wikibase:Property` will meet `schema:WebPage` subjects. The local-host sitelink's article node is a page IRI rather than a URL.
- **Page history mixes text and statement revisions.** MediaWiki clients see statement revisions as revisions that left the main text unchanged.

## Open questions

- **Q1. Crossing to concept data.** Mappings from categories to statements about the paired item, once a UI can show that a statement on an item came from an article's categories.
- **Q2. Subject-level categories.** A report comparing categories such as "1952 births" with the paired item's statements, as a way to migrate them without mapping them.
- **Q3. Template calls after import.** Whether a narrow, non-parsing recognition of template calls (name and parameters only) should ever feed mappings.
- **Q4. Page terms.** Whether pages get a label or description, for a display title or short description, or whether those stay statements.
- **Q5.** ~~**Category redirects.** MediaWiki's soft category redirects are templates; hard redirects wait on [0008](0008-namespaces-and-document-pages.md) Q8.~~ *Settled by [0051](0051-page-redirects.md) §4: a hard redirect on a category page is followed for viewing only, membership stays with the name in each member's text, and soft redirects stay templates.*
- **Q6. Collation.** Whether `uppercase` is enough, or tenants need ICU collations per language.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0001](0001-revision-metadata-rdf.md) §3 | §11 | amends | 0001 A12 |
| [0003](0003-statement-ui.md) §2, §7 | §2, §9 | extends | 0003 A7 |
| [0005](0005-crate-organization.md) §2 | §15 | extends | 0005 A38 |
| [0008](0008-namespaces-and-document-pages.md) §2, §4, §8, §10 | §1, §3, §4, §8, §11 | amends | 0008 A9 |
| [0008](0008-namespaces-and-document-pages.md) Q5 | §3, §5 | settles | 0008 Q5 |
| [0010](0010-site-ui.md) §4 | §3, §7, §8 | amends | 0010 A22 |
| [0010](0010-site-ui.md) §1, §2, §3 | §3, §7, §8 | extends | 0010 A22 |
| [0012](0012-api-requirements.md) §4, §5 | §8, §13 | extends | 0012 A23 |
| [0013](0013-postgres-storage.md) §5.6, §7 | §10 | extends | 0013 A13 |
| [0014](0014-caches-and-search.md) §7 | §12 | extends | 0014 A4 |
| [0015](0015-record-format-and-partition-registry.md) §3, §5 | §1, §5 | extends | 0015 A16 |
| [0019](0019-discussions.md) §5, §7 | §8–9, §11 | amends | 0019 A8 |
| [0019](0019-discussions.md) §6 | §8–9, §11 | extends | 0019 A8 |
| [0026](0026-sitelinks.md) §1 | §6 | amends | 0026 A3 |
| [0026](0026-sitelinks.md) §2 | §6 | extends | 0026 A3 |
| [0029](0029-resolver-namespaces.md) §6 | §8 | extends | 0029 A2 |

## References

- [Help:Categories](https://www.mediawiki.org/wiki/Help:Categories), [Manual:categorylinks table](https://www.mediawiki.org/wiki/Manual:Categorylinks_table) and [Help:Magic words](https://www.mediawiki.org/wiki/Help:Magic_words) (`DEFAULTSORT`, `__HIDDENCAT__`)
- [API:Categories](https://www.mediawiki.org/wiki/API:Categories), [API:Categorymembers](https://www.mediawiki.org/wiki/API:Categorymembers) and [API:Categoryinfo](https://www.mediawiki.org/wiki/API:Categoryinfo)
- [Wikibase REST API](https://www.wikidata.org/wiki/Wikidata:REST_API) statement routes
- [Extension:WikibaseMediaInfo](https://www.mediawiki.org/wiki/Extension:WikibaseMediaInfo), for the design not taken (§1)
- [Wikidata: allowed entity types constraint (Q52004125)](https://www.wikidata.org/wiki/Q52004125)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-29
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §15
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A38).

Replaced text (§15):

> No new crate.
>
> | Crate | Adds |
> |---|---|
> | `scatter-wikitext` | Extraction of category links, sort keys, `DEFAULTSORT` and `__HIDDENCAT__` from the subset; the leading-colon link (§3). Still builds for `wasm32`, so the editor's preview shows categories |
> | `scatter-wikibase-model` | The page subject and `{page ID}$<UUID>` statement IDs (§1); sitelinks held by page ID for the tenant's own hosts (§6) |
> | `scatter-wikibase-changeset` | Change sets keyed by page ID, restricted to statements (§1) |
> | `triplespace-titles` | The main and `Category` namespaces; bare titles in the main namespace, and main-namespace titles last in the `/resolve` order with the `also` field (§8) |
> | `triplespace-projections` | `page_category`, `category`, the mapping and thread-status projections, `page_statements` resolution, page subjects in `statement_assertion`, `entity_ref` and constraint checking, the mapping re-run and make-it-real jobs (§2–5, §9–10) |
> | `triplespace-rdf` | Page statements and page nodes in the main graph; the sitelink article node for paired pages (§11) |
> | `triplespace-search` | `categories` and `statement_keywords` on `pages` (§12) |
> | `triplespace-api-rest`, `triplespace-api-action` | The routes and modules of §13 |

### A2. Uploads in the page's history

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §2
- **Change:** extends §1
- **Summary:** A file page's uploads are records in the same partition, keyed by the page ID, so text revisions, statement revisions and file versions share one history.

### A3. File pages' statements as MediaInfo entities

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §6–7
- **Change:** amends §1, §13
- **Summary:** By section:
  - §1: On **File pages** the statements have the MediaInfo ID `M{page ID}` and are read and written through `wbgetentities` and the Wikibase statement modules, as on Commons, because tools written for Structured Data on Commons expect that contract. They appear in the `mediainfo` slot (`wikibase-mediainfo`) in `prop=revisions`. Terms and sitelinks on `M` IDs are refused with `not-supported` until page terms are settled. Statements on every other page stay as written here: no entity ID and no Action API.
  - §13: File pages are the exception: their statements are in the Action API as MediaInfo entities.

Replaced text (§1):

> **No entity ID.** Page statements have no `M`-style or other entity ID, are not returned by `wbgetentities`, and cannot be edited through the Wikibase Action API modules. They are served by the REST routes of §13. In `prop=revisions`, a statement revision appears as MediaWiki shows a revision that changed only a secondary slot: the main text is unchanged, and the summary describes the change.

Replaced text (§13):

> `wbgetentities` by `sites` and `titles` finds a page's paired item (§6). Page statements are not in the Action API.

### A4. Categories from expanded text

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §9
- **Change:** amends §3
- **Summary:** While a tenant's `wikitext.expansion` is on, membership is read from the **expanded** text: categories that templates emit count, `<includeonly>` categories reach transcluding pages, and expansion adds MediaWiki's tracking categories for its own conditions. The paragraph above holds for tenants with expansion off. The consequence that template-emitted categories are frozen at import was struck for tenants with expansion on.

Replaced text (§3):

> **Templates are not parsed.** Categories that templates emit reach the text only through the flattening revision of [0008](0008-namespaces-and-document-pages.md) §9 step 3, whose `action=expandtemplates` runs on the source wiki and writes the category links and `__HIDDENCAT__` into the flattened text. A template call added after an import renders as a placeholder (0008 §8) and emits nothing. Categories MediaWiki's parser adds on its own, such as tracking categories for broken file links, are not in any text and are out of scope.

### A5. Boards carry statements

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §1
- **Change:** extends §1
- **Summary:** Boards (310) carry page statements, as document pages do. They describe the board, not its threads.

### A6. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–15
- **Summary:** A1–A5 were folded into the Decision. The open questions were numbered, and §11's heading, which said it extended 0032, now says it uses 0032 §2, as the Related line already said. No decision changed. Before this, A2–A5 were blockquotes, and A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A7. Sitelinks cannot target inherited titles

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §6
- **Change:** extends §6
- **Summary:** A title whose primary is a page repository's page is refused as a sitelink target with `ts-sitelink-foreign`, because the page ID a sitelink stores changes when the title is forked.

### A8. Short descriptions in RDF

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §6
- **Change:** extends §11
- **Summary:** The page node gains `schema:description` from the `wikibase-shortdesc` page property, where set. Page terms (Q4) stay open.
