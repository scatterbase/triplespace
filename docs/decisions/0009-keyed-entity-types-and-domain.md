# 0009. Keyed entity types and Domain

- **Status:** Proposed
- **Date:** 2026-09-26
- **Updated:** 2026-11-15 (A16)
- **Author:** James Hare / Claude Opus
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md)
- **Uses:** [0000](0000-init.md), [0006](0006-log-integrity-and-erasure.md), [0008](0008-namespaces-and-document-pages.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

Every entity type in Triplespace so far has a **minted** ID. `Q5` was minted by the instance, and `WDQ42` was minted by Wikidata ([0002](0002-source-graphs-and-mass-ingest.md) §1). Two IDs can denote the same thing, and [0004](0004-identity-clusters-and-equivalence.md) exists to work out when they do.

Some things already have an identifier that is unique by construction. An Internet domain name is one: `en.wikipedia.org` denotes exactly one domain, and the name is the identity criterion. Minting `Q77` for it only creates a second identifier that has to be kept in step with the first.

[internetdomains.wiki](https://internetdomains.wiki/) is a Wikibase about Internet domains. Each of its domain items carries the domain name as a value, one to one. Triplespace should be able to mirror it as a remote Wikibase ([0002](0002-source-graphs-and-mass-ingest.md)) without using its item IDs as the names of domains.

Domains are also a bulk-ingest target. Citation data carries URLs, and every URL names a domain. Ingest should be able to say things about a domain without first looking up or allocating an ID for it.

## Decision

### 1. Keyed entity types

*Changed by A2, A9, A16.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.1, §3.10.*

### 2. The Domain key

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.6.*

### 3. IDs, JSON and values

*Changed by A2.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.2.*

### 4. A Domain exists because its key is valid

*Changed by A2.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.3.*

### 5. Terms

*Changed by A2, A3.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.6.*

### 6. IRIs under `scatter.red`

*Changed by A10.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.5.*

### 7. Log keys are surrogates

*Changed by A8, A12.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.4.*

### 8. Keyed types rank first in identity clusters (amends 0004 §4)

*Changed by A2, A6.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3, §4.1, §4.4.*

### 9. Key-mapped providers (amends 0002 §4 and §5)

*Changed by A2, A4, A6, A13, A14, A15.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §2, §4.1, §4.2, §8.4, §8.6.*

### 10. Operations

*Changed by A2.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §3.3, §4.10.*

### 11. The Domain namespace

*Changed by A5.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §1.4, §3.7.*

### 12. Crates (amends 0005 §2)

*Changed by A1, A15.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **Domains need no ID allocation.** Bulk ingest of domains is idempotent and needs no lookups, which suits URL-heavy citation data.
- **Two instances share Domain IRIs.** Their RDF about the same domain uses the same subject without an alignment step.
- **The `scatter.red` domain carries more weight.** It now names data subjects, not just vocabulary terms and content-derived claim IDs. What it serves at those IRIs becomes a commitment of its own.
- **Stock Wikibase clients will not understand Domain values.** `wikibase-domain` is a new data type and `domain` a new entity type. Tools like Pywikibot read the raw JSON but cannot format or validate these values.
- **Every present Domain costs a surrogate and a mapping record.** Erasability is worth that, but it adds an index lookup to every write.
- **Canonical IRIs can move to a Domain.** When a local item is linked to a Domain, its canonical IRI changes (§8). Consumers have to follow `owl:sameAs`, as 0004 already requires.
- **internetdomains.wiki's data quality shows through.** Duplicate identity values hold data back until they are resolved upstream, and invalid values leave items unmapped.

## Open questions

- **Q1.** ~~**Namespace number.** An unused subject and talk pair has to be chosen and registered on mediawiki.org's [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces) page.~~ *Settled by [0008](0008-namespaces-and-document-pages.md) §2 (0008 A6): 210/211, in the 210–219 range. Filing the registration on mediawiki.org is a pending action in the registry README, not a decision.*
- **Q2.** ~~**Provider code** for internetdomains.wiki's non-domain items, under the provider registry that is still open in [0000](0000-init.md).~~ *Settled by a registry change of 2026-09-28: `XD` (`XDQ` items, `XDP` properties), slug `internetdomains`, provider number 5, in `docs/registry/providers.toml` (registry commit, 2026-09-28).*
- **Q3.** ~~**The identity property.** Which internetdomains.wiki property carries the domain name, and whether its values are stored as A-labels or U-labels.~~ *Settled by the provider's adapter configuration: `P1` (string), dropped from the mirrored state on mapped items. The form of its values is Q8.*
- **Q4. Resolving duplicates locally.** Whether the instance may choose which upstream item maps to a key while a duplicate stays unresolved upstream, and with what operation.
- **Q5. What `scatter.red` serves** at `https://scatter.red/domain/{key}`: nothing, a 303 redirect to an instance, or a small document.
- **Q6.** ~~**The next keyed types.** DOI is the obvious candidate. Its normalizer exists ([0004](0004-identity-clusters-and-equivalence.md) §7), but its IRI would be `https://doi.org/{key}`, and a DOI can contain almost any character, so its key grammar and its ID disjointness need their own analysis.~~ *Settled by [0017](0017-entity-id-grammar.md) §5 (Keyword is the second keyed type; disjointness comes from the `type:` prefix) and [0029](0029-resolver-namespaces.md) §1 (DOI is a resolver, not a keyed type: it names a thing that already has an item). 0029's Consequences state the rule for the next identifier.*
- **Q7. Tier-2 link properties.** Which Wikidata properties, if any, should link Wikidata items to Domains.
- **Q8. The identity property's form.** (Rest of Q3.) Whether `P1`'s values are stored as A-labels or U-labels.

## References

- [UTS #46: Unicode IDNA Compatibility Processing](https://www.unicode.org/reports/tr46/)
- [RFC 5890](https://www.rfc-editor.org/rfc/rfc5890) (IDNA definitions: A-label and U-label), [RFC 1035](https://www.rfc-editor.org/rfc/rfc1035) §2.3.4 (size limits)
- [ICANN gTLD Applicant Guidebook](https://newgtlds.icann.org/en/applicants/agb), string requirements (ASCII top-level labels consist of letters only)
- [Wikibase JSON format](https://doc.wikimedia.org/Wikibase/master/php/docs_topics_json.html) (entity values without `numeric-id`)
- [`idna` crate](https://crates.io/crates/idna)
- [internetdomains.wiki](https://internetdomains.wiki/)

## Amendment log

### A1. The crate table moves to 0005

- **Date:** 2026-09-26
- **Source:** [0005](0005-crate-organization.md) §2, revision of 2026-09-26
- **Change:** supersedes §12
- **Summary:** 0005 §2 became the one crate table CI checks; `scatter-keyed` was folded into `scatter-normalize`, which builds for wasm and depends on `idna`.

Replaced text: §12 of this ADR, as it stood before this entry; in git (`tools/adr.py log 0009 A1 --before`).

### A2. Keyed IDs carry their type

- **Date:** 2026-09-27
- **Source:** [0017](0017-entity-id-grammar.md) §1, §3, §5
- **Change:** amends §3, §4, §9, §10; extends §1, §5, §8
- **Summary:** A keyed ID is the type's name, a colon and the key: `domain:en.wikipedia.org`. This replaces the disjointness argument of §3; titles are unchanged, and a bare key is not an ID. Keyword is a second keyed type, with an editable display rule; keywords never join identity clusters; OpenAlex keywords are mapped by key.

Replaced text: §1 of this ADR; §3 of this ADR; §4 of this ADR; §9 of this ADR; §10 of this ADR, as it stood before this entry; in git (`tools/adr.py log 0009 A2 --before`).

### A3. Sitelinks and Domains

- **Date:** 2026-09-27
- **Source:** [0026](0026-sitelinks.md) §5
- **Change:** extends §5
- **Summary:** Every sitelink's host is a valid Domain key; a sitelink does not make the Domain present; the host index lets the Domain page count the entities that link there.

### A4. A second key-mapped provider

- **Date:** 2026-09-28
- **Source:** [0036](0036-openstreetmap-providers.md) §5
- **Change:** extends §9
- **Summary:** The OpenStreetMap wiki's tag items are mapped by key, as internetdomains.wiki's domain items are.

### A5. Namespaces name models

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §3–4
- **Change:** amends §11
- **Summary:** The `Domain` namespace is a `pages` namespace with the model `triplespace-domain`, and `Domain talk` uses `triplespace-talk`.

Replaced text: §11 of this ADR, as it stood before this entry; in git (`tools/adr.py log 0009 A5 --before`).

### A6. Notations

- **Date:** 2026-10-01
- **Source:** [0048](0048-notation.md) §3, §6
- **Change:** extends §8, §9
- **Summary:** Notations never join identity clusters; the OpenStreetMap wiki's items map onto notations in the `osm` scheme, with `key_scheme` in the key map.

### A7. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–12
- **Summary:** A1–A6 were folded into the Decision, the open questions were numbered, and the partly struck Q3 was split, its open half becoming Q8. No decision changed. Before this, every entry was recorded only in this ADR's header or in other ADRs, apart from the `composite` talk namespace, written into §11 in place. The file before conversion is commit `0b26a3a`.

### A8. Surrogate key form and mapping record

- **Date:** 2026-10-02
- **Source:** Direct: James, implementation of 2026-10-02 (`triplespace-projections`)
- **Change:** extends §7
- **Summary:** The header key of a keyed entity's records is `{type}#{n}`; the surrogate-to-key mapping is a `scatter:v0/keyed-surrogate` record in the instance `log`, keyed by the surrogate, appended before the first record under it; `view.keyed_surrogate` is projected from it at the instance tenant and a replay that cannot find a surrogate fails rather than drops data. A `put` written under a key-mapped subject carries `upstream_id`, the provider's own item ID, which is what `view.keyed_map` records (§9).

### A9. `clusters` and `label` in the registry entry

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §1
- **Summary:** A keyed-type registry entry also records `clusters`, whether entities of the type may join identity clusters (default true; false for Keyword and Notation, which [0004](0004-identity-clusters-and-equivalence.md) §1 keeps out of clusters), and `label`, the type's display name. (PENDING A14)

### A10. A Domain's document node carries the prefixed ID

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §6
- **Summary:** A Domain's document node is `{base}/wiki/Special:EntityData/{id}` with the prefixed ID, `domain:en.wikipedia.org`, as [0017](0017-entity-id-grammar.md) §1 requires for every keyed ID, not the bare key; 0017 §3's list of the 0009 sections it updated gains §6. (PENDING A15)

Replaced text: §6 of this ADR, as it stood before this entry; in git (`tools/adr.py log 0009 A10 --before`).

### A11. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§12
- **Summary:** The Decision's current text moved to the chapter sections its pointers name; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and log. No decision changed. The text as it stood here is in git (`tools/adr.py log 0009 A11 --diff`).

### A12. Surrogates are allocated under a lock in the appending transaction

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §7
- **Summary:** A surrogate is allocated in the appending transaction under an advisory lock on `(keyed_type, key)`, so two writers meeting a new key cannot mint two surrogates for it; the mapping record is appended in the same transaction as the first record that uses the surrogate, rather than "before" it; a bulk job reserves a block of surrogates up front, as it reserves ID blocks, so a key-mapped bootstrap of millions of keys is not millions of serialized appends; and the mapping is erased only when no live record in any partition of the instance is keyed to the surrogate, so one tenant's erasure never strips the key from another tenant's assertions. (REVIEW G29)

Replaced text: [04](../architecture/04-entities-and-identifiers.md) §3.4, as it stood before this entry; in git (`tools/adr.py log 0009 A12 --before`).

> This is the same pattern as the actor surrogates in [0007](../decisions/0007-actor-identity.md) §5 ([07](../architecture/07-actors-and-accounts.md)). Erasing a Domain by key erases the mapping record along with everything else keyed to the surrogate, and only an opaque number remains. The log never sees the key, only the surrogate; this holds for every keyed type, with one sequence per type.

> **The forms.** The header key of a keyed entity's records is `{type}#{n}`, `domain#17`: the `#` keeps it from being any entity ID, since no ID form contains one. The mapping record is a `scatter:v0/keyed-surrogate` record in the **instance `log`**, keyed by the same surrogate, with content `{keyed_type, surrogate, key}` ([payloads.md §3.4](../api/payloads.md)); surrogates are instance-wide, so the mapping does not belong to any tenant, and the record is appended before the first record under the surrogate. `view.keyed_surrogate` is projected from it at the instance tenant (`''`); a projection that folds a keyed entity's records looks the surrogate up there, and fails rather than guess when it is missing, which makes a rebuild that orders the instance `log` after a tenant partition fail loudly instead of silently dropping the tenant's assertions. The table is in [03](../architecture/03-storage-caches-and-search.md).

### A13. Merge aliases, and what a sync does when a key moves

- **Date:** 2026-10-12
- **Source:** Direct: James, choices of 2026-10-12 after the internetdomains.wiki sync test (stage 2 of the real-dump test plan)
- **Change:** extends §9
- **Summary:** The mapping index holds, beside each key's owner, the items upstream merged into the owner, as aliases of the key; the `put` under the key carries them as `upstream_aliases`, and the newest `put` under a key decides its rows. When an item becomes mapped, the foreign item it was is tombstoned; when an item merged into a mapped item had a foreign item, that is tombstoned too. A merge between two unmapped items is an ordinary mirror `redirect`, written once. A key whose owner a sync saw but which no item claims any longer is cleared with a `put` of the empty state, and the snapshot sweep clears keyed entities rather than tombstoning them, leaving one already cleared alone. A state re-written because the mapping moved, at an upstream revision already recorded, claims no revision ID; its version names the revision. The table and text are in [05](../architecture/05-providers-and-ingest.md) §3.4 and §4.2, the index in [03](../architecture/03-storage-caches-and-search.md), the record field in [payloads.md](../api/payloads.md) §3.1.

Replaced text: [05](../architecture/05-providers-and-ingest.md) §4.2, as it stood before this entry; in git (`tools/adr.py log 0009 A13 --before`).

### A14. How an upstream alias answers

- **Date:** 2026-10-16
- **Source:** Direct: James, reading `Item:XDQ1` on his instance, 2026-10-16
- **Change:** extends §9
- **Summary:** §9 and [05](../architecture/05-providers-and-ingest.md) §4.1 have the prefixed upstream ID of a mapped item resolve to the Domain through the mapping index; this fixes the shape. An entity read by such an ID (`wbgetentities`, `Special:EntityData`, the REST entity routes) that `view.entity` does not hold is looked up in `view.keyed_map` by provider slug and upstream ID (the tenant's row first, then the instance's), and when the map holds it the answer is the keyed entity under its own ID with Wikibase's `redirects: {from, to}` beside it, as an upstream merge would answer — the alias is the ID of an item that no longer exists as a subject (§9, A13). The site sends `Item:XDQ9` to the Domain page with a `302`, keeping the query. An ID the map does not hold stays an ordinary foreign item, or `missing`. `triplespace_projections::read::mapped_alias` is the one lookup.

### A15. Key maps are rules in the registry entry

- **Date:** 2026-10-26
- **Source:** [0091](0091-one-wikibase-adapter.md) §2, §3, §5
- **Change:** amends §9, §12
- **Summary:** A key map is a list of `[[provider.adapter_config.key_map]]` rules, each naming an identity property by its upstream ID (not by role), a keyed type and optionally a class, with `class_property` and `identity_property_on_mapped` beside them; internetdomains.wiki's is one rule with no class, and the flat fields are retired. `scatter-adapter-internetdomains` is retired into `scatter-adapter-wikibase`. Chapter text: [05](../architecture/05-providers-and-ingest.md) §4.1, §8.6; [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.

Replaced text: 05 §4.1 of this ADR; 05 §8.6 of this ADR; 22 §2.1, the row of this ADR, as it stood before this entry; in git (`tools/adr.py log 0009 A15 --before`).

### A16. The `remote` keyed type

- **Date:** 2026-11-15
- **Source:** [0110](0110-remote-pages-and-revisions.md) §1
- **Change:** extends §1
- **Summary:** A fourth keyed type, `remote`, names a page or revision on a site that a registered host runs, by the host, the site's name, `p` or `r`, and the number the host gave it: `remote:wikimedia:enwiki-p12`. Its normalizer reads nothing but the key. Chapter text: [04](../architecture/04-entities-and-identifiers.md) §3.1, §3.11.
