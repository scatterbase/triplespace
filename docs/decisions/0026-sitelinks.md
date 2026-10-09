# 0026. Sitelinks are URLs

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-10-09 (A6)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0009](0009-keyed-entity-types-and-domain.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0018](0018-tenants.md)
- **Uses:** [0016](0016-permissions-and-access-control.md), [0022](0022-federation.md), [Wikibase contract](../api/wikibase-compat.md), [MediaWiki API contract](../api/mediawiki-compat.md)
- **Chapters:** [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [04](../architecture/04-entities-and-identifiers.md), [06](../architecture/06-statements-and-properties.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

A Wikibase **sitelink** ties an item to one page on one wiki: `{"site": "enwiki", "title": "Douglas Adams", "badges": ["Q17437798"]}`. The `site` is an ID from the wiki's `sites` table, which the operator fills from Wikimedia's site matrix or by hand; the title is a page title on that wiki; the URL is derived from the site's article path. Two invariants follow: an item has at most one sitelink per site, and a (site, title) pair belongs to at most one item. The RDF form ([wikibase-compat.md §5.2](../api/wikibase-compat.md)) is a `schema:Article` node at the page's URL with `schema:about`, `schema:isPartOf`, `schema:inLanguage`, `schema:name` and `wikibase:badge`.

The 2026-09-27 review found that no ADR registers sites. [0002](0002-source-graphs-and-mass-ingest.md) §3 reconciles sitelinks "one per site" and [0013](0013-postgres-storage.md) §5.2 stores them keyed by `(site, title)`, but which site IDs exist, and what URL each maps to, was never settled, and the `site` config kind of [0015](0015-record-format-and-partition-registry.md) §3 is already taken by scalar settings.

James's proposal is to drop the site table: **any valid URL can be a sitelink.** A tenant that describes museum collections links a record page on the museum's site; a tenant that describes software links a repository. Wikipedia articles are one case among many. Which hosts a tenant permits is its own policy, and a farm may impose a spam blocklist over every tenant.

Two constraints shape the rest. The **host of a URL is a Domain** ([0009](0009-keyed-entity-types-and-domain.md)), with a normalizer, a hierarchy and a searchable parent list already defined, so allow and deny lists can be lists of Domain keys, and "everything linked to this site" is a question about a Domain. And **MediaWiki clients read `sitelinks` as a map keyed by site ID** ([wikibase-compat.md §3.1](../api/wikibase-compat.md)), so whatever a site ID becomes, an item must have at most one link per key.

## Decision

### 1. A sitelink is a normalized URL, with badges

*Changed by A3.*

*Current text: [06](../architecture/06-statements-and-properties.md) §4.1, §4.6.*

### 2. The host is the site ID; one link per host; one item per URL

*Changed by A2, A3.*

*Current text: [06](../architecture/06-statements-and-properties.md) §4.2, §4.3, §4.6.*

### 3. Allow and deny lists (extends 0015 §3 and 0018 §3)

*Current text: [06](../architecture/06-statements-and-properties.md) §4.4.*

### 4. Reconciliation (amends 0002 §3)

*Current text: [06](../architecture/06-statements-and-properties.md) §4.5.*

### 5. Sitelinks and Domains (extends 0009 §5)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.6.*

### 6. Storage (amends 0013 §5.2)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.3, §5.*

### 7. API (extends 0012 §4 and §5)

*Current text: [18](../architecture/18-api.md) §2.2, §2.3, §3.2, §3.4.*

### 8. RDF

*Changed by A5.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §5.5.*

### 9. UI (extends 0010 §2)

*Current text: [19](../architecture/19-site-ui.md) §6.4.*

### 10. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **No site table to administer.** A tenant links what it likes within its lists, and a farm keeps spam out for everyone.
- **Wikidata tooling keeps working** for the sites it knows, because aliases give those hosts their MediaWiki IDs and titles. For everything else the tooling sees a host as a site ID and a path as a title, which is a shape it has not seen but can read.
- **Both Wikibase invariants survive**, with host in place of site: one link per host per item, one item per URL.
- **Sitelinks join the Domain hierarchy.** "Everything linked to this site" is one indexed query, and a list entry for a zone covers its hosts.
- **Lists shape the resolved view**, so policy applies to mirrored data as well as local data without touching any source graph.
- **URL normalization is a compatibility commitment.** Changing it later changes which links are equal; it is fixed here and its edge cases are tested against a corpus before release.
- **Statements still carry URLs of their own** (the `url` data type). A sitelink says "this page is about this item"; a `url` statement says whatever its property says. The two are not merged.

## Open questions

- **Q1. Liveness.** Whether to check that a linked URL resolves, at write or periodically, and what to do when it stops.
- **Q2. Titles for non-wiki links.** Whether to fetch a page's `<title>` for display, or leave the path as the title.
- **Q3. Trailing slashes and case.** Whether hosts that treat `/Foo` and `/Foo/` alike should be foldable per host, and whether a site alias should declare case-insensitivity.
- **Q4.** ~~**Links to other tenants and instances.** A sitelink to another tenant's entity page is better expressed as a cluster link ([0004](0004-identity-clusters-and-equivalence.md), [0018](0018-tenants.md) §5); whether to refuse it, or convert it, is open.~~ *Settled by A2: it is a plain sitelink, asserting nothing about identity, like a link to any page. An editor who means "this is Librarybase's Q6" writes `same-as` to `LBQ6`; the two are different statements and the UI does not convert one into the other.*
- **Q5. Language for non-aliased hosts**, so that `schema:inLanguage` can be emitted more often.
- **Q6. Whether a sitelink should make its host's Domain present** (§5), for tenants that treat their linked sites as data.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0002](0002-source-graphs-and-mass-ingest.md) §3 | §4 | amends | 0002 A13 |
| [0004](0004-identity-clusters-and-equivalence.md) §7 | §1 | extends | 0004 A6 |
| [0005](0005-crate-organization.md) §2 | §10 | extends | 0005 A22 |
| [0009](0009-keyed-entity-types-and-domain.md) §5 | §5 | extends | 0009 A3 |
| [0010](0010-site-ui.md) §2 | §9 | extends | 0010 A16 |
| [0012](0012-api-requirements.md) §4, §5 | §7 | extends | 0012 A15 |
| [0013](0013-postgres-storage.md) §5, §5.2, §5.6 | §6 | amends | 0013 A9 |
| [0013](0013-postgres-storage.md) §4, §5.4, §5.5, §7, §8 | §6 | extends | 0013 A9 |
| [0015](0015-record-format-and-partition-registry.md) §3, §5 | §2–3 | extends | 0015 A9 |
| [0018](0018-tenants.md) §3 | §3 | extends | 0018 A2 |

## References

- [Wikibase data model: Sitelinks](https://www.mediawiki.org/wiki/Wikibase/DataModel#Sitelinks) and [Wikibase RDF: sitelinks](https://www.mediawiki.org/wiki/Wikibase/Indexing/RDF_Dump_Format#Sitelinks)
- [Manual:Sites table](https://www.mediawiki.org/wiki/Manual:Sites_table) and [API:Sitematrix](https://www.mediawiki.org/wiki/API:Sitematrix)
- [WHATWG URL Standard](https://url.spec.whatwg.org/) and [RFC 3986 §6.2.2 — syntax-based normalization](https://www.rfc-editor.org/rfc/rfc3986#section-6.2.2)
- [`url` crate](https://crates.io/crates/url)
- `docs/registry/sites.toml`

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §10
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A22).

Replaced text (§10):

> | Crate | Change |
> |---|---|
> | `scatter-normalize` | The URL normalizer (§1), built on the `url` crate, which already depends on `idna` |
> | `scatter-wikibase-model` | Sitelinks as URLs with badges and an optional title; the alias mapping between (site ID, title) and URL; `sitelink-conflict` checks. Embeds `docs/registry/sites.toml` |
> | `scatter-wikibase-rdf` | The RDF of §8 |
> | `scatter-wikibase-resolve` | Reconciliation by URL and host (§4); the denied-host filter (§3) |
> | `scatter-adapter-wikidata` | Resolving upstream site IDs to URLs from the provider's site table (§4) |
> | `triplespace-projections` | `view.sitelink` (§6); re-resolution on policy change |
> | `triplespace-api-action`, `triplespace-api-rest` | §7 |
>
> No crate is added. `sites.toml` is embedded by `scatter-wikibase-model` rather than `scatter-providers` because the alias is a Wikibase-compatibility concern, not a provider one: a site alias exists for hosts the instance never mirrors.

### A2. Decisions of 2026-09-27 (evening)

- **Date:** 2026-09-27
- **Source:** Direct: James, decisions of 2026-09-27 (evening)
- **Change:** extends §2
- **Summary:** Tenant sitelinks (decision 14): a sitelink to another tenant's entity page is a plain sitelink, asserting nothing about identity, like a link to any page. An editor who means "this is Librarybase's Q6" writes `same-as` to `LBQ6`; the two are different statements and the UI does not convert one into the other. This settled Q4.

### A3. Links to the tenant's own pages

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §6
- **Change:** amends §1; extends §2
- **Summary:** By section:
  - §1: A sitelink in the local graph whose host is one the tenant is served at is stored by page ID, not URL. Its URL and title are derived from the page's current title, so a move does not break it; a title with no page is refused with `ts-sitelink-no-page`.
  - §2: A tenant registers a site alias for its own host, so that `wbgetentities` by `sites` and `titles`, `wbsetsitelink` and `Special:ItemByTitle` reach an item from a local page's title. The one-link-per-host and one-item-per-URL invariants make that pairing one-to-one.

Replaced text (§1):

> Nothing else is folded: `/Foo` and `/Foo/` are different URLs, as they are to the server that serves them.

### A4. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–2, §10
- **Summary:** A1–A3 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A3 was two blockquotes and A2 a struck question with a note. The file before conversion is commit `0b26a3a`.

### A5. The page node is the sitelink node for a paired page

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §8
- **Summary:** For a page paired with an item ([0038](0038-page-metadata-and-categories.md) §6, §11) the `schema:Article` node is the page node `{base}/page/{id}`, also typed `schema:WebPage`, with the URL as `schema:url`; every other sitelink keeps wikibase-compat §5.2's shape, where the node is the page's URL. (PENDING A20)

Replaced text (§8):

> The RDF shape of [wikibase-compat.md §5.2](../api/wikibase-compat.md) is kept for every sitelink, so QLever and Wikidata tooling see what they expect:

### A6. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [04](../architecture/04-entities-and-identifiers.md), [06](../architecture/06-statements-and-properties.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
