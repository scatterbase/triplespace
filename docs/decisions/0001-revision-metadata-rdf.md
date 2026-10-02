# 0001. Revision metadata in RDF

- **Status:** Proposed
- **Date:** 2026-09-24
- **Updated:** 2026-10-01 (A14)
- **Author:** James Hare / Claude Opus
- **Changes:** [0000](0000-init.md)
- **Uses:** [0000](0000-init.md), [MediaWiki API contract](../api/mediawiki-compat.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

Wikibase's RDF export describes only the **current state** of each entity. The only revision metadata it carries is on the entity's document node: `data:Q8 schema:version "26"`, which is the revision ID, and `schema:dateModified` (see [wikibase-compat.md §5.2](../api/wikibase-compat.md)).

MediaWiki records much more than that. The 1.43 Action API exposes:

- **Per revision:**
  - revision ID and parent ID
  - timestamp
  - actor: user, user ID, anonymous IP or temporary account
  - edit summary
  - minor-edit flag
  - change tags
  - size and SHA-1
  - slots and their content models
  - visibility flags, which mark hidden text, summaries or usernames
- **From recent changes:** the bot flag and patrol status.
- **From the logs:** deletions, protections, merges, moves, imports and suppressions.

Two of these carry more structure than they appear to:

- **Wikibase edit summaries are machine-readable.** `/* wbsetlabel-add:1|es */ Douglas Adams, test` records the operation (`wbsetlabel-add`), its arguments (`1|es`), an automatic summary and the user's own comment.
- **Revert tags carry revert details.** The `mw-undo`, `mw-rollback` and `mw-manual-revert` tags record which revisions were undone, and how.

Under [0000](0000-init.md), the append-only log is the source of truth and RDF is a projection of it. Triplespace needs a way to project this revision metadata into RDF for its own use, without changing the Wikibase-compatible RDF that external stores and tools consume.

## Decision

### 1. Revision metadata describes the document, not the thing

*Changed by A7, A8.*

Revision metadata attaches to the entity's **document node** (`data:Q8`) and to **revision nodes** hanging off it. It never attaches to the concept (`wd:Q8`). A revision describes a version of the wiki record. It says nothing about the person, work or place the record describes.

Revisions are first-class nodes with their own IRIs, minted under the instance's base URI alongside Wikibase's `s:`, `ref:` and `v:` namespaces. For example, `{base}/revision/{revid}`. Each revision node:

- is linked to its document with `prov:specializationOf`;
- is linked to its parent with `prov:wasRevisionOf`.

**Every entity has a document node on the instance, whatever minted its ID** ([0015](0015-record-format-and-partition-registry.md) §6). `data:WDQ42` is `{base}/wiki/Special:EntityData/WDQ42`, as `data:Q6` is. For a foreign entity the document node also points upstream, with `pav:importedFrom`, `pav:hasCurrentVersion` and the job that retrieved it.

**A post's revision node is its record's IRI,** `{base}/record/{partition}/{offset}`, and carries the Activity Streams properties of [0019](0019-discussions.md) §7.

### 2. Two named graphs: main and metadata

*Changed by A1, A2, A6, A11.*

Triplespace's RDF projection is a set of quads spread across named graphs. Two are described here. The others are the source graphs of [0002](0002-source-graphs-and-mass-ingest.md) §2, and the registry names them all ([0015](0015-record-format-and-partition-registry.md) §5).

| Graph | Contents | Consumers |
|---|---|---|
| **Main graph** (`resolved`) | The Wikibase-compatible RDF described in [wikibase-compat.md](../api/wikibase-compat.md), computed by reconciling the source graphs ([0002](0002-source-graphs-and-mass-ingest.md) §3) | External triplestores (QLever) and any Wikibase or Wikidata tool, through the Wikibase-compatible dump ([0013](0013-postgres-storage.md) §8) |
| **Metadata graph** | Revision nodes, actors, summaries, tags, flags, log events, and the links from main-graph nodes to the revisions that produced them | The instance's own SPARQL endpoint and its full dumps ([0013](0013-postgres-storage.md) §8) |

**Exporting to QLever means exporting the main graph only.** The Wikibase-compatible dump never contains the metadata graph. A full dump is the instance's own export and carries both ([0013](0013-postgres-storage.md) §8). The application's history views, attribution, diffs and moderation read the `view.activity` projection and the records, not RDF.

**Consumers can stay current.** For every graph an instance publishes, it also publishes the changes to that graph as a stream of SPARQL 1.1 Update requests that continues from a dump ([0032](0032-sparql-update-stream.md) §1).

**Attributing triples to revisions.** Wikibase already reifies statements, references and values as IRI nodes (`s:`, `ref:`, `v:`). The metadata graph can therefore refer to them directly, for example `s:Q8-C13E7A23-… scatter:addedIn <…/revision/26>`. This attributes the main graph's content to revisions without adding anything to the main graph.

### 3. The standard Wikibase output is unchanged

*Changed by A11, A12.*

No vocabulary is added to the main graph or to `Special:EntityData`-equivalent output. That output stays identical in shape to Wikibase's RDF format 1.0.0, including its known departures from `ontology.owl` (see [wikibase-compat.md §4.5](../api/wikibase-compat.md)). All new vocabulary lives in the metadata graph.

**The main graph gains subjects, not vocabulary.** Statements about document pages and threads are output in it, in Wikibase's statement shape, with the page node `{base}/page/{page ID}` as subject. Every predicate is Wikibase's or schema.org's ([0038](0038-page-metadata-and-categories.md) §11).

**The plain dump keeps Wikibase's blank nodes.** A second form of the dump, with blank nodes replaced by skolem IRIs, is what the update stream continues from ([0032](0032-sparql-update-stream.md) §7).

### 4. Deleted and suppressed data is an application-policy concern

*Changed by A3, A9.*

Revision deletion and suppression are handled as policy by the Triplespace application. The RDF model does not encode them:

- Deleted or suppressed content, summaries and usernames are **not exported to RDF stores**.
- They are **not shown to users**, except administrators.
- The append-only log still holds them. Administrator views read from the log or the application, not from exported RDF.

Hiding is set by a `record` ACL on the parts it hides ([0023](0023-moderation.md) §5). A legal takedown that requires erasure from the log itself is the `erase` record of [0006](0006-log-integrity-and-erasure.md) §7.

### 5. Triplespace's own vocabulary lives under `https://scatter.red/terms/v0/`

*Changed by A2.*

Terms Triplespace defines are minted under `https://scatter.red/terms/v0/`, with the prefix `scatter:`, the namespace Scatterbase already uses ([0005](0005-crate-organization.md) §5). They are never minted under `http://wikiba.se/ontology#`, which belongs to Wikimedia Deutschland. `v0` means the vocabulary makes no stability promise yet. Moving to `v1` is a compatibility commitment, made for both products at once.

The **vocabulary** namespace is distinct from **instance data** IRIs. Revision, user and graph IRIs belong to each Triplespace instance's own base URI. Only ontology terms live under `scatter.red/terms/`. Identifiers derived only from content, which are the same on every instance, are not instance data, and may also live under `scatter.red`.

### 6. Reuse existing vocabularies first

*Changed by A2, A4, A5, A8, A10, A13.*

New terms are minted only where no established vocabulary fits.

| Need | Vocabulary | Terms |
|---|---|---|
| Revision chain, time, author | PROV-O | `prov:wasRevisionOf`, `prov:specializationOf`, `prov:generatedAtTime`, `prov:wasAttributedTo`, `prov:SoftwareAgent`, `prov:invalidatedAtTime` |
| Versions and current version | PAV | `pav:version`, `pav:previousVersion`, `pav:hasCurrentVersion` |
| Provenance of foreign entities (0000 §3) | PAV | `pav:importedFrom`, `pav:retrievedFrom`, `pav:importedOn` |
| User accounts | SIOC | `sioc:UserAccount`, `sioc:has_creator`, `sioc:has_modifier` |
| Account names, holders and delegation ([0007](0007-actor-identity.md) §9) | SIOC, FOAF, PROV-O | `sioc:name`, `sioc:account_of`, `foaf:Agent`, `prov:Organization`, `prov:actedOnBehalfOf` |
| Log events: delete, protect, move, import and the rest ([0011](0011-logs.md) §8). A merge is its revisions and a redirect, not a log event | Activity Streams 2.0, with `scatter:` subclasses of `as:Activity` where none fits | `as:Delete`, `as:Update`, `as:Move`, `as:Undo`, `as:actor`, `as:object`, `as:published` |
| Posts, threads and talk pages ([0019](0019-discussions.md) §7) | Activity Streams 2.0 | `as:Note`, `as:OrderedCollection`, `as:inReplyTo`, `as:context` |
| General metadata | Dublin Core Terms, schema.org | `dcterms:isVersionOf`, `dcterms:replaces`, `schema:version` |

Terms to mint under `scatter:`:

- the parsed edit-summary operation and its arguments
- the full comment text
- change tags, which a record keeps in its attestation part ([0030](0030-edit-filters.md) §5)
- revert details
- the minor, bot and patrolled flags
- slot roles and content models
- size and SHA-1
- the temporary, anonymous and imported actor classes, and `scatter:attributionHidden` ([0007](0007-actor-identity.md) §9)

Illustrative shape, using a revision from the reference install:

```turtle
# graph <…/graph/metadata>
data:Q1 pav:hasCurrentVersion rev:9 .
rev:9 a prov:Entity, scatter:Revision ;
    prov:specializationOf data:Q1 ;
    prov:wasRevisionOf rev:8 ;
    prov:generatedAtTime "2026-09-25T00:49:03Z"^^xsd:dateTime ;
    prov:wasAttributedTo <…/user/…> ;
    scatter:summaryOperation "wbsetlabel-add" ; scatter:summaryArgs "1|es" ;
    scatter:comment "/* wbsetlabel-add:1|es */ Douglas Adams, test" ;
    scatter:minor false ; scatter:size 853 ; scatter:sha1 "eb5635fd…" ; scatter:contentModel "wikibase-item" .
```

**Every revision node carries `scatter:contentModel`, whatever the page:** entity revisions their entity type's model, thread revisions `triplespace-thread`, and statement revisions of File pages `wikibase-mediainfo` ([0041](0041-content-models.md) §10).

## Consequences

- **The external contract stays simple.** QLever and Wikibase-compatible consumers see standard Wikibase RDF and nothing else.
- ~~**The store must handle quads.** The application's own triplestore needs quad or named-graph support. The QLever export path only needs a single-graph dump.~~ *No longer holds: the application reads `view` tables, not RDF, and a local quad store is optional (A6). The QLever export is still a single-graph dump.*
- **Labels, sitelinks and truthy triples can't be attributed this way.** Terms, sitelinks and `wdt:` triples have no node in the main graph that the metadata graph could point at. Per-triple history for these must come from the log's change sets, or wait for RDF 1.2 triple terms, which are still a Candidate Recommendation as of April 2026.
- **The metadata graph is derived.** It is a projection of the log like the main graph, so it can be dropped and rebuilt. It is not a second source of truth.
- ~~**Triplespace owns a published vocabulary.** The `scatter.wiki` namespace should dereference to a published ontology document, and changing it becomes a compatibility commitment of its own.~~ *Holds for `https://scatter.red/terms/v0/`, which Triplespace shares with Scatterbase; moving to `v1` is that commitment (A2).*

## Open questions

- **Q1.** ~~**Exact namespace IRI.** For example `https://scatter.wiki/ontology#` or `https://scatter.wiki/ns/`. Also the preferred prefix name, and whether it is versioned.~~ *Settled by [0005](0005-crate-organization.md) §5: `https://scatter.red/terms/v0/`, prefix `scatter:`, versioned; `scatter.wiki` is not used.*
- **Q2.** ~~**Graph IRIs** for the main and metadata graphs, and whether they are per instance or fixed.~~ *Settled by [0015](0015-record-format-and-partition-registry.md) §5: `{base}/graph/{name}`, per instance (per tenant, [0018](0018-tenants.md) §2), with names fixed in `docs/registry/graphs.toml`; the main graph is `resolved`.*
- **Q3.** ~~**Actor IRIs.**~~ *Settled by [0007](0007-actor-identity.md) §2 and §5.*
  - ~~Pseudonymising anonymous IPs and temporary accounts.~~ *Temporary accounts are ordinary numeric actors; an upstream IP gets an instance-minted surrogate `{base}/actor/{n}` (0007 §5).*
  - ~~Referring to imported users (`imported>Name`).~~ *Resolved to the source issuer's numeric ID where the issuer is registered, else an `imported` surrogate (0007 §5).*
  - ~~Referring to users from foreign sources.~~ *`{article path}Special:Redirect/user/{id}` on the issuer's wiki (0007 §2).*
- **Q4. How far to parse edit summaries.** Wikibase's summary keys and argument layouts form an unversioned convention. [0012](0012-api-requirements.md) §3 fixes the parsed shape (operation, arguments, automatic text, user comment) and [0015](0015-record-format-and-partition-registry.md) §1 makes the parsed form a projection of the comment part; which keys are recognised remains open.
- **Q5.** ~~**What a "revision" is for data that never passed through MediaWiki.** This covers bulk ingest and foreign entities. The candidate is the log record or offset, with PAV import provenance.~~ *Settled by [0002](0002-source-graphs-and-mass-ingest.md) §8.3 (the entity's change within a job, with thin revision nodes pointing at the job) and [0015](0015-record-format-and-partition-registry.md) §4 (an observed or backfilled upstream revision is `Special:Redirect/revision/{revid}` on the provider; a state with no upstream revision ID is the record IRI `{base}/record/{partition}/{offset}`).*
- **Q6. Per-triple change sets in RDF.** Whether change sets should also be projected into RDF, following the Wikidata History Query Service model of per-revision additions and deletions graphs, or stay in the log only.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0000](0000-init.md) Q5 | §3 | settles | 0000 Q5 |

## References

- [W3C PROV-O](https://www.w3.org/TR/prov-o/)
- [PAV ontology](https://pav-ontology.github.io/pav/)
- [SIOC Core Ontology](http://rdfs.org/sioc/spec/)
- [Activity Streams 2.0 Vocabulary](https://www.w3.org/TR/activitystreams-vocabulary/)
- [RDF 1.2 Concepts (Candidate Recommendation, April 2026)](https://www.w3.org/TR/rdf12-concepts/)
- Pellissier Tanon & Suchanek, [Querying the Edit History of Wikidata](https://link.springer.com/chapter/10.1007/978-3-030-32327-1_32) (ESWC 2019)
- Schmelzeisen et al., [Wikidated 1.0](https://arxiv.org/abs/2112.05003) (2021)

## Amendment log

### A1. The main graph becomes a resolved view

- **Date:** 2026-09-24
- **Source:** [0002](0002-source-graphs-and-mass-ingest.md) §3
- **Change:** amends §2
- **Summary:** The main graph is computed by applying a reconciliation policy over the source graphs. It is still Wikibase-shaped, still what is exported to QLever, and still rebuildable from the log; the source graphs may be exported too.

Replaced text (§2):

> Triplespace's RDF projection is a set of quads spread across two named graphs:

### A2. One vocabulary under scatter.red

- **Date:** 2026-09-25
- **Source:** [0005](0005-crate-organization.md) §5
- **Change:** amends §2, §5, §6
- **Summary:** Triplespace's terms move to `https://scatter.red/terms/v0/`, the namespace Scatterbase already uses, with the prefix `scatter:`. The illustrative `sw:` becomes `scatter:`. `v0` makes no stability promise; `v1` is the commitment of this ADR's Consequences, made for both products at once. Content-derived identifiers that are the same everywhere may stay under `scatter.red`. This settled Q1, and makes the vocabulary consequence hold for the new namespace.

Replaced text (§5):

> ### 5. Triplespace's own vocabulary lives under `https://scatter.wiki`
>
> Terms Triplespace defines are minted under `https://scatter.wiki`. They are never minted under `http://wikiba.se/ontology#`, which belongs to Wikimedia Deutschland.
>
> The **vocabulary** namespace is distinct from **instance data** IRIs. Revision, user and graph IRIs belong to each Triplespace instance's own base URI. Only ontology terms live under `scatter.wiki`. The prefix `sw:` is used in this document for illustration.

Replaced text: every `sw:` prefix, now `scatter:` (`sw:addedIn`, `sw:Revision`, `sw:summaryOperation` and the rest), and the heading "Terms to mint under `https://scatter.wiki`:".

### A3. Erasure from the log

- **Date:** 2026-09-25
- **Source:** [0006](0006-log-integrity-and-erasure.md) §7
- **Change:** amends §4
- **Summary:** A legal takedown that requires data to leave the log is the `erase` record: headers stay, bodies are destroyed. Before the migration this was recorded only in 0006.

Replaced text (§4):

> This narrows, but does not resolve, the deletion question raised in [0000](0000-init.md). Legal takedowns that require erasure from the log itself remain open.

### A4. Actor vocabulary

- **Date:** 2026-09-25
- **Source:** [0007](0007-actor-identity.md) §9
- **Change:** extends §6
- **Summary:** SIOC, FOAF and PROV-O for accounts, holders and delegation; `scatter:TemporaryAccount`, `scatter:AnonymousActor`, `scatter:ImportedActor` and `scatter:attributionHidden` minted. 0007 §2 and §5 also settled Q3.

### A5. Log events in RDF

- **Date:** 2026-09-26
- **Source:** [0011](0011-logs.md) §8
- **Change:** amends §6
- **Summary:** Log events are projected into the metadata graph only, typed as Activity Streams activities where one fits and as `scatter:` subclasses of `as:Activity` where none does. Merges are removed from the list of log events: a Wikibase merge is its revisions and the redirect.

Replaced text (§6):

> | Log events (delete, protect, merge, move, import) | Activity Streams 2.0 | `as:Delete`, `as:Update`, `as:Move`, `as:actor`, `as:object`, `as:published` |

### A6. RDF becomes an output, not the read path

- **Date:** 2026-09-26
- **Source:** [0013](0013-postgres-storage.md) §8
- **Change:** amends §2
- **Summary:** The resolved view, source graphs and metadata graph stream as N-Quads from Postgres; no intermediate quad store is needed. A Wikibase-compatible dump holds the resolved view only; a full dump, the instance's own export, carries both. The application's history, attribution, diffs and moderation views come from `view.activity` and the records. The consumers column of §2 was restated accordingly.

Replaced text (§2):

> | **Main graph** | Exactly the Wikibase-compatible RDF described in [wikibase-compat.md](../api/wikibase-compat.md) | External triplestores (QLever) and any Wikibase or Wikidata tool |
> | **Metadata graph** | Revision nodes, actors, summaries, tags, flags, log events, and the links from main-graph nodes to the revisions that produced them | The Triplespace application only |
>
> **Exporting to QLever means exporting the main graph only.** The metadata graph supports the application: history views, attribution, diffs and moderation. It is not published to external RDF stores.

### A7. Document nodes for foreign entities

- **Date:** 2026-09-26
- **Source:** [0015](0015-record-format-and-partition-registry.md) §6
- **Change:** extends §1
- **Summary:** Every entity has a document node under the instance's base, whatever minted its ID; a foreign entity's also points upstream with `pav:importedFrom`, `pav:hasCurrentVersion` and the retrieving job. 0015 §5 also settled Q2.

### A8. Posts and threads

- **Date:** 2026-09-27
- **Source:** [0019](0019-discussions.md) §7
- **Change:** extends §1, §6
- **Summary:** Threads, posts and talk pages are Activity Streams nodes in the metadata graph. A post's revision node is its record IRI, with the AS2 properties added.

### A9. Hiding is a record

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §5
- **Change:** extends §4
- **Summary:** Hiding a revision's content, comment or attestation is a `record` ACL with `parts`, setting the visibility bits the API and the projections carry. Before the migration this was recorded only in 0023.

### A10. Where change tags live

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §5
- **Change:** extends §6
- **Summary:** Change tags are stored in a record's attestation part.

### A11. The SPARQL Update stream

- **Date:** 2026-09-27
- **Source:** [0032](0032-sparql-update-stream.md) §1, §7
- **Change:** extends §2, §3
- **Summary:** Each published graph is also published as a stream of SPARQL 1.1 Update requests that continues from a dump. Blank nodes are skolemized in the stream and in a second dump form; the plain dump of §3 is unchanged.

### A12. Page statements in the main graph

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §11
- **Change:** amends §3
- **Summary:** Statements about document pages and threads are output in the main graph, in Wikibase's statement shape, with the page node `{base}/page/{page ID}` as subject. This adds subjects, not vocabulary: every predicate is Wikibase's or schema.org's.

Replaced text (§3):

> Nothing is added to the main graph or to `Special:EntityData`-equivalent output. That output stays identical in shape to Wikibase's RDF format 1.0.0, including its known departures from `ontology.owl` (see [wikibase-compat.md §4.5](../api/wikibase-compat.md)). All new vocabulary lives in the metadata graph.

### A13. Content model on every revision

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §10
- **Change:** extends §6
- **Summary:** Every revision node carries `sw:contentModel`, whatever the page: entity revisions their entity type's model, thread revisions `triplespace-thread`, and statement revisions of File pages `wikibase-mediainfo`.

### A14. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–6
- **Summary:** A1–A13 were folded into the Decision, the open questions were numbered, and two consequences that A2 and A6 made false were struck. No decision changed. Before this, A12 and A13 were blockquotes under §3 and §6, and the other entries were recorded only in this ADR's header or in other ADRs. The file before conversion is commit `0b26a3a`.
