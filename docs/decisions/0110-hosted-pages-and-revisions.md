# 0110. Hosted: a keyed type for pages and revisions on multi-site hosts

- **Status:** Proposed
- **Date:** 2026-11-15
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0026](0026-sitelinks.md), [0041](0041-content-models.md), [0048](0048-notation.md), [0078](0078-entity-sources.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0017](0017-entity-id-grammar.md), [0029](0029-resolver-namespaces.md), [0052](0052-page-repositories-and-title-inheritance.md)

## Context

Statements about Wikipedia articles, and citations of particular revisions, have nothing to point at. A page on another site is not an entity: a statement cannot be made about it, and it cannot be the value of a "cites work" statement, short of minting an item for each page. Derived statements cite the revision they came from by URL with `oldid` ([0071](0071-derived-statements-from-mirrored-pages.md) §6), which is a reference, not a value. Sitelinks reach only local pages once a page is served from a repository ([0052](0052-page-repositories-and-title-inheritance.md) §6, in [04](../architecture/04-entities-and-identifiers.md) §5.3).

Many pages already have stable identifiers that need no minting. A **host** that runs many sites gives each site a short name and numbers each site's pages and revisions: Wikimedia's database names (`enwiki`) with MediaWiki's page and revision IDs, and Miraheze's, are the cases in view. `enwiki` and page 12 is English Wikipedia's article *Anarchism*, whatever its title becomes; `enwiki` and a revision ID is one revision forever. Nothing in that pattern is particular to MediaWiki: any host that names its sites and numbers their pages and revisions fits it.

Four existing mechanisms were tried before the ADR was drafted, in the design discussion of 2026-11-12 to 2026-11-15:

- **A keyed type declared in an instance's configuration.** Not possible: a keyed type's grammar and normalizer are code, chosen by the type's name, and `keyed-types.toml` is embedded at build time ([0009](0009-keyed-entity-types-and-domain.md) §1). Notation schemes are allocated in the registry only, never by an instance ([0048](0048-notation.md) §2).
- **An entity source per site** ([0078](0078-entity-sources.md)). Possible without code, as `enwiki:P12` and `enwiki:R123`, but the upstream prefix must be letters, so the form cannot be `enwiki-p12`; it takes one source, one source number and one namespace per site; and a source's name shares the table of slugs with providers, so `enwiki` is refused once English Wikipedia's provider entry (pending in `registry/README.md`) is written.
- **A resolver** ([0029](0029-resolver-namespaces.md)). A resolver finds the item that carries a key; it would need an item per page, which is the minting this is meant to avoid. Its normalizers are code too.
- **A provider per site, with ranged page IDs** ([0052](0052-page-repositories-and-title-inheritance.md) §6). Those IDs exist only for sites the instance serves pages from, and they are numbers, not something an editor types.

A keyed type fits. A keyed entity exists because its key is valid, needs no create, carries statements, and is referred to by a data type of its own ([04](../architecture/04-entities-and-identifiers.md) §3.3). The first draft of this ADR made it a type for Wikimedia alone; James asked for it to be stated for any host.

### Direction

James's direction, from the design discussion of 2026-11-12 to 2026-11-15:

- **"There's a namespace, Wikimedia:, and an identifier in three parts: database slug, hyphen, "r" for revision or "p" for page, and then the number. Wikimedia is the upstream provider, and the database code, r/p, and ID come together to uniquely identify a page or revision."**
- **"I think this most fits the conventions of a keyed type."**
- **"What if we get rid of the domain fallback?"** An earlier form accepted a domain in place of the site's name; it is dropped, so the key is checked by its form alone.
- **"Draft the ADR for the Wikimedia keyed type using site aliases. Write a Python script to update the file based on the site matrix."**
- **"It's a pattern usable by any provider that hosts multiple websites and identifies pages and revisions. This includes Miraheze. It could include any non-wiki that happens to fit that pattern. Could you re-frame the ADR in provider-neutral terms?"**

## Decision

### 1. The `hosted` keyed type (extends 0009 §1)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.1, §3.11.*

### 2. Hosts

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.12.*

### 3. What a hosted key names (extends 0048 §3; amends 0078 §2)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.9, §3.11; [05](../architecture/05-providers-and-ingest.md) §6.2.*

### 4. Links come from a host's site list (uses 0026 §2)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.11, §3.12.*

### 5. Wikimedia's site list is generated (extends 0026 §2)

*Current text: [06](../architecture/06-statements-and-properties.md) §4.3.*

### 6. Namespace and content model (extends 0008 §2, 0041 §3)

*Current text: [10](../architecture/10-pages-and-content-models.md) §1.4, §4.4.*

### 7. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **One keyed type per host,** `wikimedia:enwiki-p12` and `miraheze:…`. Each host would cost a keyed type, a namespace pair, a data type and an entity type variant, which is what [0048](0048-notation.md) set out to stop for vocabularies; and a property that cites a page would need one data type per host. One type with the host in the key, as a notation carries its scheme, costs a registry entry per host.
- **A notation scheme per host,** `notation:wikimedia:enwiki-p12`. It would cost no type at all. It was not chosen because a notation is a string, not the thing it names ([04](../architecture/04-entities-and-identifiers.md) §3.9): `notation:osm:amenity=cafe` is a tag and not the café. A "cites work" value, or a statement about an article, needs to be the page or revision itself, as a Domain is the domain.
- **One type for pages and another for revisions.** Two data types would let a property require a revision. The `p` or `r` in the key already tells them apart, and a property that must take one kind can say so with a constraint; one type keeps one namespace and one data type.
- **The host as a provider in `providers.toml`.** A provider mints entities that are mirrored and carries codes, numbers and issuers ([05](../architecture/05-providers-and-ingest.md) §1.5); a host here mints nothing that Triplespace stores, and needs only a name, a site grammar and how to link. It gets its own small registry, as notation schemes do.
- **A domain in place of the site's name** (`hosted:wikimedia:en.wikipedia.org-p12`), or accepted as input and rewritten. Rewriting needs a site list, which changes and which tenants extend, so the same input could give different keys on different instances, while a keyed ID and its IRI must be the same everywhere ([04](../architecture/04-entities-and-identifiers.md) §3.5). Dropped (Direction).
- **The configuration-only routes and a provider per site,** set out in the Context.

## Consequences

- **A page or revision on any registered host can carry statements and be a value without an item being minted,** and a re-run ingest that names it is idempotent, as with any keyed type.
- **A new host costs one registry entry,** and a grammar in code only when its sites are named in a way no existing grammar admits. Miraheze is drafted as a candidate.
- **The key is checked by form, not existence.** `hosted:wikimedia:enwikii-p12` is valid, as `domain:example.invalid` is. A site the host's site list does not name gets no link, and the UI says so; nothing is refused.
- **IDs are long, and typed short.** `hosted:wikimedia:enwiki-p12` is the ID and `Hosted:wikimedia:enwiki-p12` the title. Host names join the table of keyed-type, resolver and scheme names, so `wikimedia:enwiki-p12` typed into `/resolve` or search finds it.
- **A page can have two IDs on one tenant:** `hosted:wikimedia:enwiki-p12`, and the provider-ranged page ID it has when the tenant serves English Wikipedia's pages (04 §5.3). They are different things, an entity and a page, and nothing joins them yet (Q5).
- **One more `wikibase-*` data type** that no Wikibase client knows, as [0041](0041-content-models.md) Q2 and [0048](0048-notation.md) Q4 already ask about.
- **The site aliases list every public Wikimedia wiki,** 1,029 of them, where nine were written by hand; `group` now reads `special` for Commons and Wikidata, which the hand-written seed had as `commons` and `wikidata`. Nothing reads `group`.
- **314 and 315 are allocated,** leaving 316–319 in the second block.

## Open questions

- **Q1. The name.** `hosted`, `Hosted` and `wikibase-hosted` are working names. Others considered: `page` (taken in sense by Triplespace's own pages, and `Page:` is ProofreadPage's namespace 104 on Wikisource), `site` and `farm` (a wiki term, which a non-wiki host would not use).
- **Q2. The label.** The draft derives it from the key, as a notation's is. The page's title would read better, but it changes when the page moves and needs a fetch; an editable label, as a keyword's is, would let an ingest store it as data. Which?
- **Q3. A namespace alias per host.** The Direction's `Wikimedia:enwiki-p12` could be kept as a title form: a host entry naming an alias that reads as `Hosted:wikimedia:…`. An alias costs no number, but `wikimedia:` is an interwiki prefix on Wikimedia's own map, and a client that applies MediaWiki's talk-page rule could reject `Wikimedia talk:…`, as [0048](0048-notation.md) Q1 asks of scheme names.
- **Q4. Who may register a host.** The draft allows the registry only, because hosted IDs and IRIs are shared by every instance, as scheme names are (0048 §2). The first ask was for something an instance could deploy itself; an instance-declared host would need IDs that mean something only where it is declared, as entity sources' do.
- **Q5. Two IDs for one page.** Whether a tenant that serves a site's pages ([0052](0052-page-repositories-and-title-inheritance.md)) shows its served page on the keyed entity's page, records the link as a statement, or leaves them apart.
- **Q6. A revision's page.** Whether an ingest that names a revision should also state which page it belongs to (`hosted:wikimedia:enwiki-r…` → `hosted:wikimedia:enwiki-p12`), and by which role. Revisions can move between pages when histories are merged, so it would be data, not part of the key.
- **Q7. Miraheze's site list.** The candidate entry has the site grammar and link templates but no site list: a Miraheze wiki's domain is its subdomain of `miraheze.org` unless it has a custom one, so links need a list generated from Miraheze's own, as Wikimedia's is from the site matrix.
- **Q8. Whether this ends 0048's rule of thumb.** 0048 made Notation so that outside vocabularies stop costing a keyed type each. This ADR argues a page is not a vocabulary term (Alternatives); James to confirm that this line holds and is the one to draw.

## References

- [MediaWiki API: Sitematrix](https://www.mediawiki.org/wiki/Extension:SiteMatrix/API)
- [Manual:Page table, `page_id`](https://www.mediawiki.org/wiki/Manual:Page_table) and [Manual:Revision table, `rev_id`](https://www.mediawiki.org/wiki/Manual:Revision_table)
- [Help:Permanent link (`?curid=`, `?oldid=`)](https://www.mediawiki.org/wiki/Help:Permanent_link)
- [Miraheze](https://meta.miraheze.org/)
- [Wikimedia User-Agent policy](https://foundation.wikimedia.org/wiki/Policy:Wikimedia_Foundation_User-Agent_Policy)
