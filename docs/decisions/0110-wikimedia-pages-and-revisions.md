# 0110. Wikimedia: a keyed type for Wikimedia pages and revisions

- **Status:** Proposed
- **Date:** 2026-11-15
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0026](0026-sitelinks.md), [0041](0041-content-models.md), [0048](0048-notation.md), [0078](0078-entity-sources.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0017](0017-entity-id-grammar.md), [0029](0029-resolver-namespaces.md), [0052](0052-page-repositories-and-title-inheritance.md)

## Context

Statements about Wikipedia articles, and citations of particular revisions, have nothing to point at. A page on a Wikimedia wiki is not an entity: a statement cannot be made about it, and it cannot be the value of a "cites work" statement, short of minting an item for each page. Derived statements cite the revision they came from by URL with `oldid` ([0071](0071-derived-statements-from-mirrored-pages.md) §6), which is a reference, not a value. Sitelinks reach only local pages once a page is served from a repository ([0052](0052-page-repositories-and-title-inheritance.md) §6, in [04](../architecture/04-entities-and-identifiers.md) §5.3).

Every Wikimedia page and revision already has a stable identifier: the wiki's database name and the page ID or revision ID that MediaWiki gave it. `enwiki` and page 12 is English Wikipedia's article *Anarchism*, whatever its title becomes; `enwiki` and a revision ID is one revision forever.

Four existing mechanisms were tried against this before the ADR was drafted, in the design discussion of 2026-11-12 to 2026-11-15:

- **A keyed type declared in an instance's configuration.** Not possible: a keyed type's grammar and normalizer are code, chosen by the type's name, and `keyed-types.toml` is embedded at build time ([0009](0009-keyed-entity-types-and-domain.md) §1). Notation schemes are allocated in the registry only, never by an instance ([0048](0048-notation.md) §2).
- **An entity source per wiki** ([0078](0078-entity-sources.md)). Possible without code, as `enwiki:P12` and `enwiki:R123`, but the upstream prefix must be letters, so the form cannot be `enwiki-p12`; it takes one source, one source number and one namespace per wiki; and a source's name shares the table of slugs with providers, so `enwiki` is refused once English Wikipedia's provider entry (pending in `registry/README.md`) is written.
- **A resolver** ([0029](0029-resolver-namespaces.md)). A resolver finds the item that carries a key; it would need an item per page, which is the minting this is meant to avoid. Its normalizers are code too.
- **A provider per wiki, with ranged page IDs** ([0052](0052-page-repositories-and-title-inheritance.md) §6). Those IDs exist only for wikis the instance serves pages from, and they are numbers, not something an editor types.

A keyed type fits. A keyed entity exists because its key is valid, needs no create, carries statements, and is referred to by a data type of its own ([04](../architecture/04-entities-and-identifiers.md) §3.3).

### Direction

James's direction, from the design discussion of 2026-11-12 to 2026-11-15:

- **"There's a namespace, Wikimedia:, and an identifier in three parts: database slug, hyphen, "r" for revision or "p" for page, and then the number. Wikimedia is the upstream provider, and the database code, r/p, and ID come together to uniquely identify a page or revision."**
- **"I think this most fits the conventions of a keyed type."**
- **"What if we get rid of the domain fallback?"** An earlier form accepted a domain in place of the database name; it is dropped, so the key is checked by its form alone.
- **"Draft the ADR for the Wikimedia keyed type using site aliases. Write a Python script to update the file based on the site matrix."**

## Decision

### 1. The `wikimedia` keyed type (extends 0009 §1)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.1, §3.11.*

### 2. What a Wikimedia key names (extends 0048 §3; amends 0078 §2)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.9, §3.11; [05](../architecture/05-providers-and-ingest.md) §6.2.*

### 3. Links come from the site aliases (uses 0026 §2)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.11.*

### 4. The site aliases are generated (extends 0026 §2)

*Current text: [06](../architecture/06-statements-and-properties.md) §4.3.*

### 5. Namespace and content model (extends 0008 §2, 0041 §3)

*Current text: [10](../architecture/10-pages-and-content-models.md) §1.4, §4.4.*

### 6. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **A notation scheme, `notation:wikimedia:enwiki-p12`.** It would cost a scheme and a grammar, with no namespace, data type or entity type variant, which is what 0048 made Notation for. It was not chosen because a notation is a string, not the thing it names ([04](../architecture/04-entities-and-identifiers.md) §3.9): `notation:osm:amenity=cafe` is a tag and not the café. A "cites work" value, or a statement about an article, needs to be the page or revision itself, as a Domain is the domain. Choosing the notation would make every statement about an article a statement about a string that names it.
- **One type for pages and another for revisions.** Two data types would let a property require a revision. The `p` or `r` in the key already tells them apart, and a property that must take one kind can say so with a constraint; one type keeps one namespace and one data type.
- **A domain in place of the database name** (`wikimedia:en.wikipedia.org-p12`), or accepted as input and rewritten to the database name. Rewriting needs the site aliases, which change and which tenants extend, so the same input could give different keys on different instances, while a keyed ID and its IRI must be the same everywhere ([04](../architecture/04-entities-and-identifiers.md) §3.5). Dropped (Direction).
- **The configuration-only routes and a provider per wiki,** set out in the Context.

## Consequences

- **A page or revision on any Wikimedia wiki can carry statements and be a value without an item being minted,** and a re-run ingest that names it is idempotent, as with any keyed type.
- **The key is checked by form, not existence.** `wikimedia:enwikii-p12` is valid, as `domain:example.invalid` is. A database name the site aliases do not list gets no link, and the UI says so; nothing is refused.
- **A page can have two IDs on one tenant:** `wikimedia:enwiki-p12`, and the provider-ranged page ID it has when the tenant serves English Wikipedia's pages (04 §5.3). They are different things, an entity and a page, and nothing joins them yet (Q3).
- **One more `wikibase-*` data type** that no Wikibase client knows, as [0041](0041-content-models.md) Q2 and [0048](0048-notation.md) Q4 already ask about.
- **The site aliases list every public Wikimedia wiki,** 1,029 of them, where nine were written by hand; `group` now reads `special` for Commons and Wikidata, which the hand-written seed had as `commons` and `wikidata`. Nothing reads `group`.
- **314 and 315 are allocated,** leaving 316–319 in the second block.

## Open questions

- **Q1. The label.** The draft derives it from the key (`enwiki-p12`), as a notation's is. The page's title would read better, but it changes when the page moves and needs a fetch; an editable label, as a keyword's is, would let an ingest store it as data. Which?
- **Q2. `Wikimedia` as a namespace name.** `wikimedia:` is an interwiki prefix on Wikimedia's own interwiki map, for the Foundation's wiki. MediaWiki lets a namespace win over an interwiki prefix of the same name, but wikitext copied from a Wikimedia wiki that links `[[wikimedia:…]]` would land in this namespace, and a client that applies MediaWiki's talk-page rule could reject `Wikimedia talk:enwiki-p12`, as [0048](0048-notation.md) Q1 asks of scheme names. The alternative is a name no interwiki map uses.
- **Q3. Two IDs for one page.** Whether a tenant that serves a wiki's pages ([0052](0052-page-repositories-and-title-inheritance.md)) shows its served page on the keyed entity's page, records the link as a statement, or leaves them apart.
- **Q4. A revision's page.** Whether an ingest that names a revision should also state which page it belongs to (`wikimedia:enwiki-r…` → `wikimedia:enwiki-p12`), and by which role. Revisions can move between pages when histories are merged, so it would be data, not part of the key.
- **Q5. Other MediaWiki farms.** Miraheze and others use database names too, some the same as Wikimedia's (`metawiki`). This type is Wikimedia's only. Whether a later type takes a farm prefix, or a site alias, for the rest.
- **Q6. Whether this ends 0048's rule of thumb.** 0048 made Notation so that outside vocabularies stop costing a keyed type each. This ADR argues a page is not a vocabulary term (Alternatives); James to confirm that this line holds and is the one to draw.

## References

- [MediaWiki API: Sitematrix](https://www.mediawiki.org/wiki/Extension:SiteMatrix/API)
- [Manual:Page table, `page_id`](https://www.mediawiki.org/wiki/Manual:Page_table) and [Manual:Revision table, `rev_id`](https://www.mediawiki.org/wiki/Manual:Revision_table)
- [Wikimedia: `?curid=` and `?oldid=` permanent links](https://www.mediawiki.org/wiki/Help:Permanent_link)
- [Wikimedia User-Agent policy](https://foundation.wikimedia.org/wiki/Policy:Wikimedia_Foundation_User-Agent_Policy)
