# 0026. Sitelinks are URLs

- **Status:** Proposed
- **Date:** 2026-09-27
- **Author:** James Hare / Claude Fable
- **Related:** [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (§4 amends the sitelink row of §3), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md) (§1 extends §7 with a URL normalizer), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (§10 amends §2), [0009 — Keyed entity types and Domain](0009-keyed-entity-types-and-domain.md) (§3 uses §2 and §8; §5 extends §5), [0010 — Site UI](0010-site-ui.md) (§9 extends §2), [0012 — API requirements for the site UI](0012-api-requirements.md) (§7 extends §4 and §5), [0013 — Postgres as the log store and serving model](0013-postgres-storage.md) (§6 amends `view.sitelink` in §5.2), [0015 — Record format and partition registry](0015-record-format-and-partition-registry.md) (§3 extends §3 with the `sitelink-policy` kind; §2 extends §5 with `sites.toml`), [0016 — Permissions and access control](0016-permissions-and-access-control.md) (§9), [0018 — Tenants](0018-tenants.md) (§3 extends §3: tenant and instance lists), [Wikibase data model and ontology contract](../api/wikibase-compat.md) (§2, §3.1, §5.2), [MediaWiki API contract](../api/mediawiki-compat.md) (§4.1, `wbsetsitelink`), [0022 — Federation: verified data sync and ActivityPub](0022-federation.md) (§8 reuses §3's list shape for `federation-policy`)

## Context

A Wikibase **sitelink** ties an item to one page on one wiki: `{"site": "enwiki", "title": "Douglas Adams", "badges": ["Q17437798"]}`. The `site` is an ID from the wiki's `sites` table, which the operator fills from Wikimedia's site matrix or by hand; the title is a page title on that wiki; the URL is derived from the site's article path. Two invariants follow: an item has at most one sitelink per site, and a (site, title) pair belongs to at most one item. The RDF form ([wikibase-compat.md §5.2](../api/wikibase-compat.md)) is a `schema:Article` node at the page's URL with `schema:about`, `schema:isPartOf`, `schema:inLanguage`, `schema:name` and `wikibase:badge`.

The 2026-09-27 review found that no ADR registers sites. [0002](0002-source-graphs-and-mass-ingest.md) §3 reconciles sitelinks "one per site" and [0013](0013-postgres-storage.md) §5.2 stores them keyed by `(site, title)`, but which site IDs exist, and what URL each maps to, was never settled, and the `site` config kind of [0015](0015-record-format-and-partition-registry.md) §3 is already taken by scalar settings.

James's proposal is to drop the site table: **any valid URL can be a sitelink.** A tenant that describes museum collections links a record page on the museum's site; a tenant that describes software links a repository. Wikipedia articles are one case among many. Which hosts a tenant permits is its own policy, and a farm may impose a spam blocklist over every tenant.

Two constraints shape the rest. The **host of a URL is a Domain** ([0009](0009-keyed-entity-types-and-domain.md)), with a normalizer, a hierarchy and a searchable parent list already defined, so allow and deny lists can be lists of Domain keys, and "everything linked to this site" is a question about a Domain. And **MediaWiki clients read `sitelinks` as a map keyed by site ID** ([wikibase-compat.md §3.1](../api/wikibase-compat.md)), so whatever a site ID becomes, an item must have at most one link per key.

## Decision

### 1. A sitelink is a normalized URL, with badges

A sitelink has a **URL**, a set of **badges** (item IDs, as in Wikibase) and an optional **title** for display. The URL is normalized when it is written, and the normalized form is the link's identity:

1. parse as a WHATWG URL; reject anything that does not parse;
2. the scheme must be in the tenant's allowed set (`sitelinks.schemes` in `site` configuration; default `https` and `http`);
3. lowercase the scheme; normalize the host with the Domain normalizer of [0009](0009-keyed-entity-types-and-domain.md) §2, so it is a valid Domain key in A-label form; drop a default port;
4. normalize percent-encoding in the path, query and fragment (RFC 3986 §6.2.2: uppercase hex digits, decode unreserved characters); an empty path becomes `/`;
5. keep the query and the fragment. A fragment names a section, and a section is a legitimate target.

Nothing else is folded: `/Foo` and `/Foo/` are different URLs, as they are to the server that serves them. The normalizer is added to [0004](0004-identity-clusters-and-equivalence.md) §7's table as the `url` data type's key, and the `url` data type used by statements gets it too.

**Badges** are item IDs in the resolved view's canonical form ([0004](0004-identity-clusters-and-equivalence.md) §4). Which items may be badges is `site` configuration, as `$wgWBRepoSettings['badgeItems']` is.

### 2. The host is the site ID; one link per host; one item per URL

**The site ID of a sitelink is its host**, in A-label form: `en.wikipedia.org`, `collections.example.museum`. This keeps both Wikibase invariants, with the host in place of the site:

- **An item has at most one sitelink per host.** Writing a second is a `sitelink-conflict`, as Wikibase reports it. An item that needs two pages on one host says so with statements, not sitelinks.
- **A normalized URL belongs to at most one item.** Writing it on a second item is the same error, naming the first.

**Site aliases keep Wikidata tooling working.** Tools and clients hard-code MediaWiki site IDs (`enwiki`, `dewikisource`, `commonswiki`) and pass titles, not URLs. A **site alias** maps a MediaWiki site ID to a host, an article path and a language, so that `enwiki` + `Douglas Adams` and `https://en.wikipedia.org/wiki/Douglas_Adams` are the same sitelink:

| Field | Example |
|---|---|
| Site ID | `enwiki` |
| Host | `en.wikipedia.org` |
| Article path | `/wiki/$1` |
| Language | `en` |

The defaults ship in `docs/registry/sites.toml` ([0015](0015-record-format-and-partition-registry.md) §5), generated from Wikimedia's site matrix and committed; a tenant adds aliases for other MediaWiki hosts as `config` records of kind `site-alias`. Where a host has an alias, the JSON `sitelinks` map is keyed by the alias's site ID and the entry carries the title in MediaWiki form; where it has none, the map is keyed by the host and `title` is the URL's path, query and fragment. Every entry carries `url`, as Wikibase's JSON already does. Input accepts either form everywhere.

### 3. Allow and deny lists (extends 0015 §3 and 0018 §3)

Which hosts may be linked is policy in two layers, both `config` records of kind `sitelink-policy`:

| Scope | Record | Meaning |
|---|---|---|
| **Instance** (farm) | `sitelink-policy:deny`, in the instance `config` | Domain keys that no tenant may link. A spam blocklist. **It cannot be overridden by a tenant.** |
| **Tenant** | `sitelink-policy:mode` (`allow` or `deny`) and `sitelink-policy:list`, in the tenant `config` | In `allow` mode everything not on the list is permitted, and the list is the tenant's blocklist. In `deny` mode nothing not on the list is permitted, and the list is the tenant's allowlist. The default mode is `allow` with an empty list. |

**Matching is by Domain hierarchy.** A list entry matches a host equal to it or beneath it, so `wikipedia.org` covers every language edition and `example.org` covers `www.example.org`; this is the `key_parents` relation [0014](0014-caches-and-search.md) §7 already indexes. A URL is permitted when the instance deny list does not match its host and the tenant's mode and list permit it.

**Lists are evaluated at write** and refuse a link with `ts-sitelink-denied`, naming the list. They are **also applied to the resolved view**: a sitelink in any source graph whose host is denied is left out of the resolved view, the RDF and the search document, with the source graph untouched, as suppressions of [0002](0002-source-graphs-and-mass-ingest.md) §3 already work. So a farm blocklist added after the fact removes spam from every tenant's view at once without touching any log, and a mirrored sitelink to a denied host is not shown either.

**Links that a list change leaves stranded** are listed for maintenance beside the corrections of [0002](0002-source-graphs-and-mass-ingest.md) §7, so an editor can remove or reconsider them. Changing a tenant's lists needs `ts-config`; the instance list, `ts-config` at the farm ([0018](0018-tenants.md) §11).

### 4. Reconciliation (amends 0002 §3)

The sitelink row of [0002](0002-source-graphs-and-mass-ingest.md) §3 read "sitelinks (one per site): the local graph wins wherever it says anything." It now reads:

| What | Rule |
|---|---|
| Sitelinks | Union by normalized URL across graphs. Where two graphs give one host different URLs for the same entity, the local graph wins, then the provider order of [0004](0004-identity-clusters-and-equivalence.md) §4. Badges are the union. A local `override` may suppress a mirrored sitelink ([0002](0002-source-graphs-and-mass-ingest.md) §8.2) |

Mirrored sitelinks arrive as site IDs. The provider's adapter resolves them to URLs through the provider's own site table (Wikidata's `sites`, read once per sync) and writes URLs, so the mirror graph holds the same shape as the local graph and nothing downstream knows about site IDs.

### 5. Sitelinks and Domains (extends 0009 §5)

Every sitelink's host is a valid Domain key by construction (§1). A sitelink does **not** make the Domain present ([0009](0009-keyed-entity-types-and-domain.md) §4): linking a page asserts nothing about its host. But the host index of §6 answers "which entities link to this site", and the Domain page's header ([0009](0009-keyed-entity-types-and-domain.md) §5) shows the count with a link to the list, beneath the DNS hierarchy it already shows. A `key_parents` query gives the count for a zone.

### 6. Storage (amends 0013 §5.2)

```sql
CREATE TABLE view.sitelink (
  url_key   text PRIMARY KEY,                  -- the normalized URL (§1); one item per URL
  entity_id text NOT NULL,
  host      text NOT NULL,                     -- Domain key; the site ID (§2)
  site_id   text,                              -- alias site ID where one applies (§2)
  title     text,                              -- display title
  badges    text[] NOT NULL DEFAULT '{}',
  UNIQUE (entity_id, host)                     -- one link per host per item (§2)
);
CREATE INDEX sitelink_host ON view.sitelink (host);
```

The tenant column rule of [0013](0013-postgres-storage.md) §5 applies. `view.registry` holds the policy records and aliases like every config kind. The denied-host filter is applied by the resolution projection, so `view.sitelink` holds only permitted links; a policy change re-runs it for the affected hosts, which the `host` index finds.

### 7. API (extends 0012 §4 and §5)

**Action API**, additively under [0012](0012-api-requirements.md) §1, keeping MediaWiki's meaning for aliased sites:

| Module | Behaviour |
|---|---|
| `wbsetsitelink` | `linksite` is a site alias ID or a host; `linktitle` is a title (aliased site) or a path (host). A new `linkurl` takes the whole URL and needs neither. `badges` as in Wikibase. Errors: `sitelink-conflict`, `ts-sitelink-denied`, `ts-sitelink-scheme` |
| `wbgetentities` | `sitefilter` accepts alias IDs and hosts; `sites` and `titles` (lookup by sitelink) accept both forms, and a `urls` parameter looks up by URL |
| `wbeditentity` | `sitelinks` entries may carry `url` in place of `site`+`title` |
| `meta=siteinfo` | `siprop=triplespace` reports `sitelinks: {schemes, mode, alias_count}`; no site table is reported, since there is none |
| Wikibase REST v1 | `/entities/items/{id}/sitelinks/{site_id}` accepts alias IDs and hosts |

**REST**, under `rest.php/triplespace/v0`: `GET /sitelinks?host=` lists the entities linked to a host or, with `parents=1`, to it and every host beneath it; `GET /sitelinks/resolve?url=` gives the entity for a URL; `GET`, `PUT` and `DELETE /entity/{id}/sitelinks/{host}`. `Special:ItemByTitle` accepts an alias ID and title or a URL.

### 8. RDF

The RDF shape of [wikibase-compat.md §5.2](../api/wikibase-compat.md) is kept for every sitelink, so QLever and Wikidata tooling see what they expect:

```turtle
<https://collections.example.museum/object/1234> a schema:Article ;
    schema:about wd:Q42 ;
    schema:isPartOf <https://collections.example.museum/> ;
    schema:name "object/1234" ;
    wikibase:badge wd:Q17437798 .
```

`schema:inLanguage` is emitted only where an alias supplies a language; `schema:name` carries a language tag only then. `wikibase:sitelinks` on the entity counts links in the resolved view.

### 9. UI (extends 0010 §2)

The **Sitelinks** tab ([0010](0010-site-ui.md) §2) groups links by host, showing the host as a Domain chip linking to `Domain:{host}`, the title or path, and badges. Adding a link takes a URL and, for an aliased site, offers the site-and-title form with title autocomplete against that site's API, as Wikibase does. A denied host is refused with the list named.

### 10. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-normalize` | The URL normalizer (§1), built on the `url` crate, which already depends on `idna` |
| `scatter-wikibase-model` | Sitelinks as URLs with badges and an optional title; the alias mapping between (site ID, title) and URL; `sitelink-conflict` checks. Embeds `docs/registry/sites.toml` |
| `scatter-wikibase-rdf` | The RDF of §8 |
| `scatter-wikibase-resolve` | Reconciliation by URL and host (§4); the denied-host filter (§3) |
| `scatter-adapter-wikidata` | Resolving upstream site IDs to URLs from the provider's site table (§4) |
| `triplespace-projections` | `view.sitelink` (§6); re-resolution on policy change |
| `triplespace-api-action`, `triplespace-api-rest` | §7 |

No crate is added. `sites.toml` is embedded by `scatter-wikibase-model` rather than `scatter-providers` because the alias is a Wikibase-compatibility concern, not a provider one: a site alias exists for hosts the instance never mirrors.

## Consequences

- **No site table to administer.** A tenant links what it likes within its lists, and a farm keeps spam out for everyone.
- **Wikidata tooling keeps working** for the sites it knows, because aliases give those hosts their MediaWiki IDs and titles. For everything else the tooling sees a host as a site ID and a path as a title, which is a shape it has not seen but can read.
- **Both Wikibase invariants survive**, with host in place of site: one link per host per item, one item per URL.
- **Sitelinks join the Domain hierarchy.** "Everything linked to this site" is one indexed query, and a list entry for a zone covers its hosts.
- **Lists shape the resolved view**, so policy applies to mirrored data as well as local data without touching any source graph.
- **URL normalization is a compatibility commitment.** Changing it later changes which links are equal; it is fixed here and its edge cases are tested against a corpus before release.
- **Statements still carry URLs of their own** (the `url` data type). A sitelink says "this page is about this item"; a `url` statement says whatever its property says. The two are not merged.

## Open questions

- **Liveness.** Whether to check that a linked URL resolves, at write or periodically, and what to do when it stops.
- **Titles for non-wiki links.** Whether to fetch a page's `<title>` for display, or leave the path as the title.
- **Trailing slashes and case.** Whether hosts that treat `/Foo` and `/Foo/` alike should be foldable per host, and whether a site alias should declare case-insensitivity.
- ~~**Links to other tenants and instances.** A sitelink to another tenant's entity page is better expressed as a cluster link ([0004](0004-identity-clusters-and-equivalence.md), [0018](0018-tenants.md) §5); whether to refuse it, or convert it, is open.~~ *Settled 2026-09-27: it is a plain sitelink, asserting nothing about identity, like a link to any page. An editor who means "this is Librarybase's Q6" writes `same-as` to `LBQ6`; the two are different statements and the UI does not convert one into the other.*
- **Language for non-aliased hosts**, so that `schema:inLanguage` can be emitted more often.
- **Whether a sitelink should make its host's Domain present** (§5), for tenants that treat their linked sites as data.

## References

- [Wikibase data model: Sitelinks](https://www.mediawiki.org/wiki/Wikibase/DataModel#Sitelinks) and [Wikibase RDF: sitelinks](https://www.mediawiki.org/wiki/Wikibase/Indexing/RDF_Dump_Format#Sitelinks)
- [Manual:Sites table](https://www.mediawiki.org/wiki/Manual:Sites_table) and [API:Sitematrix](https://www.mediawiki.org/wiki/API:Sitematrix)
- [WHATWG URL Standard](https://url.spec.whatwg.org/) and [RFC 3986 §6.2.2 — syntax-based normalization](https://www.rfc-editor.org/rfc/rfc3986#section-6.2.2)
- [`url` crate](https://crates.io/crates/url)
- `docs/registry/sites.toml`
