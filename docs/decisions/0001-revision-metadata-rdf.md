# 0001. Revision metadata in RDF

- **Status:** Proposed
- **Date:** 2026-09-24
- **Amended by:** [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md), [0007 — Actor identity](0007-actor-identity.md), [0011 — Upstream and local logs](0011-logs.md), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md), [0019 — Discussions](0019-discussions.md) (§7 extends §1 and §6: posts and threads as Activity Streams nodes in the metadata graph), [0030 — Edit filters](0030-edit-filters.md) (§5 gives the change tags of §6 their place in the record: the attestation part), [0032 — The SPARQL Update stream](0032-sparql-update-stream.md) (§1 extends §2: the main graph's consumers can follow a SPARQL Update stream as well as load a dump; §7 skolemizes blank nodes in that stream and in a second dump form, leaving the plain dump of §3 unchanged)
- **Author:** James Hare / Claude Opus
- **Related:** [0000 — Initial proposition](0000-init.md), [MediaWiki API contract](../api/mediawiki-compat.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

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

Revision metadata attaches to the entity's **document node** (`data:Q8`) and to **revision nodes** hanging off it. It never attaches to the concept (`wd:Q8`). A revision describes a version of the wiki record. It says nothing about the person, work or place the record describes.

Revisions are first-class nodes with their own IRIs, minted under the instance's base URI alongside Wikibase's `s:`, `ref:` and `v:` namespaces. For example, `{base}/revision/{revid}`. Each revision node:

- is linked to its document with `prov:specializationOf`;
- is linked to its parent with `prov:wasRevisionOf`.

### 2. Two named graphs: main and metadata

Triplespace's RDF projection is a set of quads spread across two named graphs:

| Graph | Contents | Consumers |
|---|---|---|
| **Main graph** | Exactly the Wikibase-compatible RDF described in [wikibase-compat.md](../api/wikibase-compat.md) | External triplestores (QLever) and any Wikibase or Wikidata tool |
| **Metadata graph** | Revision nodes, actors, summaries, tags, flags, log events, and the links from main-graph nodes to the revisions that produced them | The Triplespace application only |

**Exporting to QLever means exporting the main graph only.** The metadata graph supports the application: history views, attribution, diffs and moderation. It is not published to external RDF stores.

**Attributing triples to revisions.** Wikibase already reifies statements, references and values as IRI nodes (`s:`, `ref:`, `v:`). The metadata graph can therefore refer to them directly, for example `s:Q8-C13E7A23-… sw:addedIn <…/revision/26>`. This attributes the main graph's content to revisions without adding anything to the main graph.

### 3. The standard Wikibase output is unchanged

Nothing is added to the main graph or to `Special:EntityData`-equivalent output. That output stays identical in shape to Wikibase's RDF format 1.0.0, including its known departures from `ontology.owl` (see [wikibase-compat.md §4.5](../api/wikibase-compat.md)). All new vocabulary lives in the metadata graph.

### 4. Deleted and suppressed data is an application-policy concern

Revision deletion and suppression are handled as policy by the Triplespace application. The RDF model does not encode them:

- Deleted or suppressed content, summaries and usernames are **not exported to RDF stores**.
- They are **not shown to users**, except administrators.
- The append-only log still holds them. Administrator views read from the log or the application, not from exported RDF.

This narrows, but does not resolve, the deletion question raised in [0000](0000-init.md). Legal takedowns that require erasure from the log itself remain open.

### 5. Triplespace's own vocabulary lives under `https://scatter.wiki`

Terms Triplespace defines are minted under `https://scatter.wiki`. They are never minted under `http://wikiba.se/ontology#`, which belongs to Wikimedia Deutschland.

The **vocabulary** namespace is distinct from **instance data** IRIs. Revision, user and graph IRIs belong to each Triplespace instance's own base URI. Only ontology terms live under `scatter.wiki`. The prefix `sw:` is used in this document for illustration.

### 6. Reuse existing vocabularies first

New terms are minted only where no established vocabulary fits.

| Need | Vocabulary | Terms |
|---|---|---|
| Revision chain, time, author | PROV-O | `prov:wasRevisionOf`, `prov:specializationOf`, `prov:generatedAtTime`, `prov:wasAttributedTo`, `prov:SoftwareAgent`, `prov:invalidatedAtTime` |
| Versions and current version | PAV | `pav:version`, `pav:previousVersion`, `pav:hasCurrentVersion` |
| Provenance of foreign entities (0000 §3) | PAV | `pav:importedFrom`, `pav:retrievedFrom`, `pav:importedOn` |
| User accounts | SIOC | `sioc:UserAccount`, `sioc:has_creator`, `sioc:has_modifier` |
| Log events (delete, protect, merge, move, import) | Activity Streams 2.0 | `as:Delete`, `as:Update`, `as:Move`, `as:actor`, `as:object`, `as:published` |
| General metadata | Dublin Core Terms, schema.org | `dcterms:isVersionOf`, `dcterms:replaces`, `schema:version` |

Terms to mint under `https://scatter.wiki`:

- the parsed edit-summary operation and its arguments
- the full comment text
- change tags
- revert details
- the minor, bot and patrolled flags
- slot roles and content models
- size and SHA-1

Illustrative shape, using a revision from the reference install:

```turtle
# graph <…/graph/metadata>
data:Q1 pav:hasCurrentVersion rev:9 .
rev:9 a prov:Entity, sw:Revision ;
    prov:specializationOf data:Q1 ;
    prov:wasRevisionOf rev:8 ;
    prov:generatedAtTime "2026-09-25T00:49:03Z"^^xsd:dateTime ;
    prov:wasAttributedTo <…/user/…> ;
    sw:summaryOperation "wbsetlabel-add" ; sw:summaryArgs "1|es" ;
    sw:comment "/* wbsetlabel-add:1|es */ Douglas Adams, test" ;
    sw:minor false ; sw:size 853 ; sw:sha1 "eb5635fd…" ; sw:contentModel "wikibase-item" .
```

## Consequences

- **The external contract stays simple.** QLever and Wikibase-compatible consumers see standard Wikibase RDF and nothing else.
- **The store must handle quads.** The application's own triplestore needs quad or named-graph support. The QLever export path only needs a single-graph dump.
- **Labels, sitelinks and truthy triples can't be attributed this way.** Terms, sitelinks and `wdt:` triples have no node in the main graph that the metadata graph could point at. Per-triple history for these must come from the log's change sets, or wait for RDF 1.2 triple terms, which are still a Candidate Recommendation as of April 2026.
- **The metadata graph is derived.** It is a projection of the log like the main graph, so it can be dropped and rebuilt. It is not a second source of truth.
- **Triplespace owns a published vocabulary.** The `scatter.wiki` namespace should dereference to a published ontology document, and changing it becomes a compatibility commitment of its own.

## Open questions

- ~~**Exact namespace IRI.** For example `https://scatter.wiki/ontology#` or `https://scatter.wiki/ns/`. Also the preferred prefix name, and whether it is versioned.~~ *Settled by [0005](0005-crate-organization.md) §5: `https://scatter.red/terms/v0/`, prefix `scatter:`, versioned; `scatter.wiki` is not used.*
- ~~**Graph IRIs** for the main and metadata graphs, and whether they are per instance or fixed.~~ *Settled by [0015](0015-record-format-and-partition-registry.md) §5: `{base}/graph/{name}`, per instance (per tenant, [0018](0018-tenants.md) §2), with names fixed in `docs/registry/graphs.toml`; the main graph is `resolved`.*
- ~~**Actor IRIs.**~~ *Settled by [0007](0007-actor-identity.md) §2 and §5.*
  - ~~Pseudonymising anonymous IPs and temporary accounts.~~ *Temporary accounts are ordinary numeric actors; an upstream IP gets an instance-minted surrogate `{base}/actor/{n}` (0007 §5).*
  - ~~Referring to imported users (`imported>Name`).~~ *Resolved to the source issuer's numeric ID where the issuer is registered, else an `imported` surrogate (0007 §5).*
  - ~~Referring to users from foreign sources.~~ *`{article path}Special:Redirect/user/{id}` on the issuer's wiki (0007 §2).*
- **How far to parse edit summaries.** Wikibase's summary keys and argument layouts form an unversioned convention. [0012](0012-api-requirements.md) §3 fixes the parsed shape (operation, arguments, automatic text, user comment) and [0015](0015-record-format-and-partition-registry.md) §1 makes the parsed form a projection of the comment part; which keys are recognised remains open.
- ~~**What a "revision" is for data that never passed through MediaWiki.** This covers bulk ingest and foreign entities. The candidate is the log record or offset, with PAV import provenance.~~ *Settled by [0002](0002-source-graphs-and-mass-ingest.md) §8.3 (the entity's change within a job, with thin revision nodes pointing at the job) and [0015](0015-record-format-and-partition-registry.md) §4 (an observed or backfilled upstream revision is `Special:Redirect/revision/{revid}` on the provider; a state with no upstream revision ID is the record IRI `{base}/record/{partition}/{offset}`).*
- **Per-triple change sets in RDF.** Whether change sets should also be projected into RDF, following the Wikidata History Query Service model of per-revision additions and deletions graphs, or stay in the log only.

## References

- [W3C PROV-O](https://www.w3.org/TR/prov-o/)
- [PAV ontology](https://pav-ontology.github.io/pav/)
- [SIOC Core Ontology](http://rdfs.org/sioc/spec/)
- [Activity Streams 2.0 Vocabulary](https://www.w3.org/TR/activitystreams-vocabulary/)
- [RDF 1.2 Concepts (Candidate Recommendation, April 2026)](https://www.w3.org/TR/rdf12-concepts/)
- Pellissier Tanon & Suchanek, [Querying the Edit History of Wikidata](https://link.springer.com/chapter/10.1007/978-3-030-32327-1_32) (ESWC 2019)
- Schmelzeisen et al., [Wikidated 1.0](https://arxiv.org/abs/2112.05003) (2021)
