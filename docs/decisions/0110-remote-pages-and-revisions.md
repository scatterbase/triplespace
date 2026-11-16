# 0110. Remote: a keyed type for pages and revisions on multi-site hosts

- **Status:** Proposed
- **Date:** 2026-11-15
- **Updated:** 2026-11-16 (A5)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0009](0009-keyed-entity-types-and-domain.md), [0026](0026-sitelinks.md), [0041](0041-content-models.md), [0048](0048-notation.md), [0052](0052-page-repositories-and-title-inheritance.md), [0078](0078-entity-sources.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0017](0017-entity-id-grammar.md), [0029](0029-resolver-namespaces.md), [0038](0038-page-metadata-and-categories.md), [0054](0054-forking-a-mirrored-page.md)

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
- **"Rename to Remote."** (2026-11-16, of the working name `hosted`.)
- **"If content syncing is enabled for that wiki, it goes to the synced copy of that wiki page. Otherwise, it falls back to working like an entity page."** (2026-11-16, of what `Remote:` shows on a tenant that serves the site.)
- **"Semantically, Remote:examplewiki-p12 can only refer to examplewiki, and if you fork, you get a new entity."** (2026-11-16, of forks.)
- **"This is the concept of page statements (as opposed to entity statements). If a remote wiki page is synced, it should have local page statements (e.g. projections from categories), and so you can access the data for the page that way. When there is no synced wiki page, the data is the only thing."** (2026-11-16, of how the data of a synced remote page is reached.)
- **"I accept your recommendations for Q9, Q11, Q12."** (2026-11-16.)

## Decision

### 1. The `remote` keyed type (extends 0009 §1)

*Changed by A1, A4.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.1, §3.11.*

### 2. Hosts

*Changed by A2, A3.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.12.*

### 3. What a remote key names (extends 0048 §3; amends 0078 §2)

*Changed by A3, A5.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.9, §3.11; [05](../architecture/05-providers-and-ingest.md) §6.2.*

### 4. Links come from a host's site list (uses 0026 §2)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.11, §3.12.*

### 5. Wikimedia's site list is generated (extends 0026 §2)

*Current text: [06](../architecture/06-statements-and-properties.md) §4.3.*

### 6. Namespace and content model (extends 0008 §2, 0041 §3)

*Changed by A2.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §1.4, §4.4.*

### 7. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 8. A remote key on a tenant that serves its site (uses 0052 §1, §6; uses 0054 §1)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.11.*

### 9. A synced remote page carries page statements (amends 0052 §7; settles 0052 Q1)

*Current text: [13](../architecture/13-mirrored-pages.md) §1.7.*

## Alternatives considered

- **One keyed type per host,** `wikimedia:enwiki-p12` and `miraheze:…`. Each host would cost a keyed type, a namespace pair, a data type and an entity type variant, which is what [0048](0048-notation.md) set out to stop for vocabularies; and a property that cites a page would need one data type per host. One type with the host in the key, as a notation carries its scheme, costs a registry entry per host.
- **A notation scheme per host,** `notation:wikimedia:enwiki-p12`. It would cost no type at all. It was not chosen because a notation is a string, not the thing it names ([04](../architecture/04-entities-and-identifiers.md) §3.9): `notation:osm:amenity=cafe` is a tag and not the café. A "cites work" value, or a statement about an article, needs to be the page or revision itself, as a Domain is the domain.
- **One type for pages and another for revisions.** Two data types would let a property require a revision. The `p` or `r` in the key already tells them apart, and a property that must take one kind can say so with a constraint; one type keeps one namespace and one data type.
- **The host as a provider in `providers.toml`.** A provider mints entities that are mirrored and carries codes, numbers and issuers ([05](../architecture/05-providers-and-ingest.md) §1.5); a host here mints nothing that Triplespace stores, and needs only a name, a site grammar and how to link. It gets its own small registry, as notation schemes do.
- **A domain in place of the site's name** (`remote:wikimedia:en.wikipedia.org-p12`), or accepted as input and rewritten. Rewriting needs a site list, which changes and which tenants extend, so the same input could give different keys on different instances, while a keyed ID and its IRI must be the same everywhere ([04](../architecture/04-entities-and-identifiers.md) §3.5). Dropped (Direction).
- **The configuration-only routes and a provider per site,** set out in the Context.

## Consequences

- **A page or revision on any registered host can carry statements and be a value without an item being minted,** and a re-run ingest that names it is idempotent, as with any keyed type.
- **A new host costs one registry entry,** and a grammar in code only when its sites are named in a way no existing grammar admits. Miraheze is drafted as a candidate.
- **The key is checked by form, not existence.** `remote:wikimedia:enwikii-p12` is valid, as `domain:example.invalid` is. A site the host's site list does not name gets no link, and the UI says so; nothing is refused.
- **IDs are long, and typed short.** `remote:wikimedia:enwiki-p12` is the ID and `Remote:wikimedia:enwiki-p12` the title. Host names join the table of keyed-type, resolver and scheme names, so `wikimedia:enwiki-p12` typed into `/resolve` or search finds it.
- **A page can have two IDs on one tenant:** `remote:wikimedia:enwiki-p12`, and the provider-ranged page ID it has when the tenant serves English Wikipedia's pages (04 §5.3). They are different things, an entity and a page, and nothing joins them yet (Q5).
- **One more `wikibase-*` data type** that no Wikibase client knows, as [0041](0041-content-models.md) Q2 and [0048](0048-notation.md) Q4 already ask about.
- **The site aliases list every public Wikimedia wiki,** 1,029 of them, where nine were written by hand; `group` now reads `special` for Commons and Wikidata, which the hand-written seed had as `commons` and `wikidata`. Nothing reads `group`.
- **314 and 315 are allocated,** leaving 316–319 in the second block.

## Open questions

- **Q1.** ~~**The name.** `hosted`, `Hosted` and `wikibase-hosted` were working names. Others considered: `page` (taken in sense by Triplespace's own pages, and `Page:` is ProofreadPage's namespace 104 on Wikisource), `site` and `farm` (a wiki term, which a non-wiki host would not use).~~ *Settled by §1: James chose `remote` on 2026-11-16.*
- **Q2.** ~~**The label.** Derived from the key, the page's title, or editable.~~ *Settled by A1: James decided on 2026-11-16 to build a label from the synced title, else the number and site, with a stored label overriding.*
- **Q3.** ~~**A namespace alias per host.** Whether a host may name a title alias such as `Wikimedia:`.~~ *Settled by A2: James approved an optional `alias` per host on 2026-11-16, `Wikimedia` the first.*
- **Q4.** ~~**Who may register a host.** The registry only, or instances too.~~ *Settled by A3: James approved on 2026-11-16 tenant-declared hosts on the entity-source pattern, with promotion to the registry.*
- **Q5.** ~~**Remote keys and page repositories.** Whether `Remote:wikimedia:enwiki-p12` resolves to the served page on a tenant that serves the site, and where it lands when the page is forked.~~ *Settled by §8: James decided on 2026-11-16 that it goes to the synced copy when the tenant syncs the site and works as an entity page otherwise, and that a fork is a new page the key never names.*
- **Q6.** ~~**A revision's page.** Whether and how an ingest states which page a revision belongs to.~~ *Settled by A4: James approved the `revision-of` role on 2026-11-16, projected later on synced sites.*
- **Q7.** ~~**Miraheze's site list.** How a Miraheze wiki's links are found.~~ *Withdrawn: James deferred Miraheze support on 2026-11-16; it stays a commented candidate in `hosts.toml`.*
- **Q8.** ~~**Whether this ends 0048's rule of thumb.**~~ *Settled by A5: James's justification of 2026-11-16, that Remote is an umbrella over keyed types whose things are self-validating in identity; 0048's rule stands for terms.*
- **Q9.** ~~**Which repository serves a site.** Whether a page-repository record or the provider entry names the host and site a repository serves.~~ *Settled by §8: James accepted on 2026-11-16 a `remote` field on the provider entry, derived from the hosts' site lists where it is empty.*
- **Q10.** ~~**Reaching the entity on a served site.** How the remote entity's statements are reached once `Remote:` shows the served page.~~ *Settled by §9: James decided on 2026-11-16 that a synced page's data is reached as its page statements.*
- **Q11.** ~~**One set of data or two.** Whether a synced page's page statements are the remote entity's statements, stored on the remote key, or a second set keyed by the ranged page ID.~~ *Settled by §9: James confirmed on 2026-11-16 one set, on the remote key.*
- **Q12.** ~~**Category projections on a synced page.** Whether a synced page's categories project page statements, given that they are not memberships in `view.page_category`.~~ *Settled by §9: James accepted on 2026-11-16 that projection waits on [0052](0052-page-repositories-and-title-inheritance.md) Q3; until then a synced page's page statements are asserted only.*

## References

- [MediaWiki API: Sitematrix](https://www.mediawiki.org/wiki/Extension:SiteMatrix/API)
- [Manual:Page table, `page_id`](https://www.mediawiki.org/wiki/Manual:Page_table) and [Manual:Revision table, `rev_id`](https://www.mediawiki.org/wiki/Manual:Revision_table)
- [Help:Permanent link (`?curid=`, `?oldid=`)](https://www.mediawiki.org/wiki/Help:Permanent_link)
- [Miraheze](https://meta.miraheze.org/)
- [Wikimedia User-Agent policy](https://foundation.wikimedia.org/wiki/Policy:Wikimedia_Foundation_User-Agent_Policy)

## Amendment log

### A1. Labels are built from what the tenant knows

- **Date:** 2026-11-16
- **Source:** Direct: James, design discussion of 2026-11-16
- **Change:** amends §1
- **Summary:** Settles Q2. A remote key's labels are editable, as a keyword's are, and a stored label always wins. Without one the UI builds a label: on a tenant whose page repository indexes the site's titles, the current title and the site ("Judith Butler (from en.wikipedia.org)"); otherwise the number and the site ("page ID 148809 (from en.wikipedia.org)", or "(from enwiki)" where the site list gives no domain); in Wikibase JSON and RDF, which carry stored labels only, the key. An option to index titles without serving pages would supply titles too. Chapter text: [04](../architecture/04-entities-and-identifiers.md) §3.11.

Replaced text ([04](../architecture/04-entities-and-identifiers.md) §3.11, as it stood):

> | Display | Derived: the key without the host as the `mul` label; `wbsetlabel` fails with `derived-label` as on a Domain (§3.6). Descriptions and aliases are stored and reconciled like an item's |

### A2. A host may name a title alias

- **Date:** 2026-11-16
- **Source:** Direct: James, design discussion of 2026-11-16
- **Change:** extends §2, §6
- **Summary:** Settles Q3. A host entry may carry `alias`, a title form that normalizes to the canonical `Remote:` title and takes no namespace number: `Wikimedia:enwiki-p12` is `Remote:wikimedia:enwiki-p12`. An alias colliding with a namespace name or alias or a site-alias interwiki prefix on the tenant is refused. Wikimedia's alias is `Wikimedia`; its interwiki prefix points to the Foundation's governance wiki, and the alias's talk titles avoid the interwiki prefix that `Remote talk:wikimedia:…` begins with. Chapter text: [04](../architecture/04-entities-and-identifiers.md) §3.12; [10](../architecture/10-pages-and-content-models.md) §1.4.

### A3. Tenants may declare hosts

- **Date:** 2026-11-16
- **Source:** Direct: James, design discussion of 2026-11-16
- **Change:** amends §2, §3
- **Summary:** Settles Q4. Besides registry hosts, a tenant may declare a host as a `remote-host` config record, on the entity-source pattern: its name joins the tenant's table of names under the sources' rules, its keys mean something only on that tenant, its IRIs are under the tenant's base, its site grammar is a built-in one, and it can be promoted to the registry, keeping its keys' form and recording each old IRI as `owl:sameAs` the new. A key under a tenant host is therefore not valid by its form alone on every instance, and is read as a source ID is. Chapter text: [04](../architecture/04-entities-and-identifiers.md) §3.11, §3.12; [05](../architecture/05-providers-and-ingest.md) §6.2, §6.3; [23](../architecture/23-configuration-and-registry.md) §2.2.

Replaced text ([04](../architecture/04-entities-and-identifiers.md) §3.12, as it stood):

> **Hosts are allocated in the registry only, never by an instance or a tenant,** for the reason schemes are (§3.8): a remote ID and its IRI are shared by every instance, so two instances that gave one name to different hosts would describe different pages with one IRI. **A new host is a registry change; a new site grammar is code.** A host is not a provider (`providers.toml`, [05](../architecture/05-providers-and-ingest.md) §1.5): it mints nothing Triplespace stores or mirrors, and needs only a name, a grammar and its links.

Replaced text ([04](../architecture/04-entities-and-identifiers.md) §3.11, the Grammar row, as it stood):

> | Grammar | A registered host's name; `:`; a site name in the host's site grammar (§3.12), which never admits `-`; `-`; `p` or `r`; a decimal number from 1 to 2^63 − 1 with no leading zeros |

Replaced text ([05](../architecture/05-providers-and-ingest.md) §6.2, as it stood):

> A keyed ID's key is valid by its form alone, the same on every instance and with nothing declared: a domain name, a phrase, a notation, or a host's page or revision number qualified by the host and the site ([04](../architecture/04-entities-and-identifiers.md) §3.11, [0110](../decisions/0110-remote-pages-and-revisions.md) §3).

### A4. The `revision-of` role

- **Date:** 2026-11-16
- **Source:** Direct: James, design discussion of 2026-11-16
- **Change:** extends §1
- **Summary:** Settles Q6. Which page a remote revision key belongs to is said by a statement whose property plays the role `revision-of`, asserted by ingests on the revision key with the page key as value; on a tenant that serves the site it is to be projected as well, once synced pages' projected statements exist. Chapter text: [04](../architecture/04-entities-and-identifiers.md) §3.11; [06](../architecture/06-statements-and-properties.md) §1.2.

### A5. Remote is an umbrella over keyed types

- **Date:** 2026-11-16
- **Source:** Direct: James, design discussion of 2026-11-16
- **Change:** extends §3
- **Summary:** Settles Q8 with James's justification: Remote is an umbrella over what would be one keyed type per host, and what qualifies those things as keyed is that they are self-validating in identity, which is the test any new keyed type must pass. 0048's rule stands for terms. Chapter text: [04](../architecture/04-entities-and-identifiers.md) §3.9.
