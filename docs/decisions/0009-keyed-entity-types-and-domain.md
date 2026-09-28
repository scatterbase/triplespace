# 0009. Keyed entity types and Domain

- **Status:** Proposed
- **Date:** 2026-09-26
- **Author:** James Hare / Claude Opus
- **Amended by:** [0005 — Crate organization, revision of 2026-09-26](0005-crate-organization.md) (§2 crate names), [0017 — Entity ID grammar](0017-entity-id-grammar.md), [0026 — Sitelinks are URLs](0026-sitelinks.md) (§5 extends §5: sitelink hosts are Domains, and the Domain page counts them; §3 uses §2 and §8 for allow and deny lists), [0029 — Resolver namespaces](0029-resolver-namespaces.md) (§1 settles the DOI open question)
- **Related:** [0000 — Initial proposition](0000-init.md), [0002 — Source graphs and mass ingest](0002-source-graphs-and-mass-ingest.md) (amends §4 and §5), [0004 — Identity clusters and equivalence](0004-identity-clusters-and-equivalence.md) (amends §4), [0005 — Crate organization for reuse by Scatterbase](0005-crate-organization.md) (amends §2), [0006 — Log integrity and erasure](0006-log-integrity-and-erasure.md), [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

Every entity type in Triplespace so far has a **minted** ID. `Q5` was minted by the instance, and `WDQ42` was minted by Wikidata ([0002](0002-source-graphs-and-mass-ingest.md) §1). Two IDs can denote the same thing, and [0004](0004-identity-clusters-and-equivalence.md) exists to work out when they do.

Some things already have an identifier that is unique by construction. An Internet domain name is one: `en.wikipedia.org` denotes exactly one domain, and the name is the identity criterion. Minting `Q77` for it only creates a second identifier that has to be kept in step with the first.

[internetdomains.wiki](https://internetdomains.wiki/) is a Wikibase about Internet domains. Each of its domain items carries the domain name as a value, one to one. Triplespace should be able to mirror it as a remote Wikibase ([0002](0002-source-graphs-and-mass-ingest.md)) without using its item IDs as the names of domains.

Domains are also a bulk-ingest target. Citation data carries URLs, and every URL names a domain. Ingest should be able to say things about a domain without first looking up or allocating an ID for it.

## Decision

### 1. Keyed entity types

A **keyed entity type** is an entity type whose ID is a normalized natural key, not a minted identifier. Each keyed type is a registry entry, recorded in the log as configuration ([0004](0004-identity-clusters-and-equivalence.md) §9). The entry records:

- the **type name**, used as the `type` in Wikibase JSON;
- the **namespace** its pages live in ([0008](0008-namespaces-and-document-pages.md) §1);
- the **key grammar**: which strings are valid keys;
- the **normalizer**: how input is turned into the one canonical key;
- the **IRI template** (§6);
- the **display rule**: how the key is shown as a label (§5);
- the **data type** used to refer to entities of this type (§3).

**Domain is the only keyed type registered by this ADR.** The mechanism is general so that others, such as DOIs or ISBNs, can be added as registry entries later. Normalizers come from `scatter-normalize` ([0004](0004-identity-clusters-and-equivalence.md) §7).

### 2. The Domain key

**The key is the domain name in lowercase A-label form** (the ASCII form, with internationalized labels in Punycode), with no trailing dot.

The normalizer:

1. strips one trailing dot;
2. applies [UTS #46](https://www.unicode.org/reports/tr46/) processing, non-transitional, with `CheckHyphens`, `CheckBidi`, `CheckJoiners` and `UseSTD3ASCIIRules` on;
3. converts every label to its A-label;
4. lowercases the result.

A key is valid when:

- the whole name is at most 253 octets and each label at most 63;
- it has at least two labels, **or** it is a single label that is alphabetic ASCII or begins with `xn--`. A single label is therefore always shaped like a top-level domain.

**`www.` is not stripped.** `www.example.org` and `example.org` are different names.

**Names with underscores are not keys.** `UseSTD3ASCIIRules` rejects them, so service labels such as `_dmarc.example.org` are not Domains. Domain means a host or zone name.

**Either form is accepted as input.** `Domain:bücher.example`, `Domain:XN--BCHER-KVA.example` and `Domain:xn--bcher-kva.example.` all normalize to `xn--bcher-kva.example`. Page titles redirect to the normalized form ([0008](0008-namespaces-and-document-pages.md) §3). The U-label form is used for display (§5).

### 3. IDs, JSON and values

**The entity ID is the key itself.** In Wikibase JSON a Domain is:

```json
{"type": "domain", "id": "en.wikipedia.org", "labels": {…}, "descriptions": {…}, "claims": {…}, "lastrevid": …}
```

**Keys cannot be confused with entity IDs.** Every other entity ID is an uppercase letter prefix followed by digits (`Q5`, `WDQ42`, `OAW123`), and entity ID parsing is case-insensitive. A valid Domain key either contains a dot, which no entity ID does, or is a single alphabetic label with no digits (§2). So one ID parser tells them apart: a string that matches the entity ID grammar is an entity ID, and anything else is tried as a keyed type.

**Referring to a Domain uses a new data type, `wikibase-domain`.** Its value type is `wikibase-entityid`, with an `id` and no `numeric-id`:

```json
{"value": {"entity-type": "domain", "id": "en.wikipedia.org"}, "type": "wikibase-entityid"}
```

Wikibase already has entity values without a `numeric-id`: the Lexeme extension's forms and senses (`L1-F1`, `L1-S1`) are written this way.

**Statement IDs follow the Wikibase form** `<EntityId>$<UUID>`, so a local statement on a Domain has an ID like `en.wikipedia.org$C7C2A847-…`. `$` cannot occur in a key.

### 4. A Domain exists because its key is valid

**Every valid key denotes a Domain, whether or not anything has been said about it.** There is no operation that creates one.

- **The first `add` is enough.** Change sets refer to a Domain by its key. They never allocate an ID, need a temporary ref, or need a match key ([0002](0002-source-graphs-and-mass-ingest.md) §8.5). Re-running an ingest is idempotent by construction.
- **Valid and present are different.** A Domain is **present** when some graph asserts something about it. `wbgetentities` returns an empty entity (its derived label, §5, and no statements) for a valid key that is not present, and an `invalid-entity-id` error for an invalid one. It never returns `missing` for a valid key. Page listings such as `list=allpages` show only present Domains.
- **There is no delete.** Retracting every local assertion leaves the Domain valid and, if no mirror says anything, not present. Erasure by key works as for any other entity ([0006](0006-log-integrity-and-erasure.md) §7).
- **There are no merges or redirects between Domains,** and no `same-as` between two of them. Two different keys are two different names. That `wikipedia.com` forwards to `wikipedia.org` is a fact about HTTP, recorded as a statement, not an identity.
- **`convert` does not apply.** A Domain is already canonical (§8).

### 5. Terms

- **The label is derived from the key and cannot be edited.** The resolved view emits one label in the language `mul`, whose value is the U-label form of the key. Wikidata uses `mul` for labels that hold in every language. Labels in source graphs are kept there, and the per-graph views ([0003](0003-statement-ui.md) §6) still show them, but the resolved view ignores them. `wbsetlabel` on a Domain fails with a Triplespace error code, `derived-label`.
- **The A-label is a derived alias** when it differs from the U-label.
- **Descriptions and other aliases** are stored and reconciled like an item's ([0002](0002-source-graphs-and-mass-ingest.md) §3).
- **Domains have no sitelinks.**

**The page shows the DNS hierarchy.** The header of `Domain:en.wikipedia.org` links its parents, `org` and `wikipedia.org`, by removing labels from the left. This is navigation, not data: no statements are derived and nothing is emitted in RDF. The Public Suffix List is not used, because the core should not depend on an external list that changes.

### 6. IRIs under `scatter.red`

**A Domain's concept IRI is `https://scatter.red/domain/{key}`,** for example `https://scatter.red/domain/en.wikipedia.org`. The key is used in A-label form, so the IRI is plain ASCII.

The IRI depends only on the key, so every instance mints the same one. Under [0005](0005-crate-organization.md) §5, identifiers derived only from content are not instance data and may live under `scatter.red`, as Scatterbase's claim and triple IRIs do. Two Triplespace instances that describe the same domain therefore describe the same RDF subject, and their data can be merged without an alignment step.

**The document node is still instance data.** Revision metadata attaches to an entity's document node, not to the concept ([0001](0001-revision-metadata-rdf.md) §1). A Domain's document node is `{base}/wiki/Special:EntityData/{key}` on each instance, so each instance's history stays its own.

**The keyed type's IRI template is registry data.** The template for Domain is `https://scatter.red/domain/{key}`. A later keyed type may use an external IRI instead, such as `https://doi.org/{key}` for DOIs.

### 7. Log keys are surrogates

A domain name can be personal data: a person's own name registered as a domain, for example. A log header key survives erasure ([0006](0006-log-integrity-and-erasure.md) §3, §7), so a key must not be the domain name itself. A hash of the name is no better, because domain names can be enumerated from zone files and any hash can be checked against them.

**Each present keyed entity gets a surrogate for log headers.** The rules are:

- The instance mints the surrogates in sequence, one per keyed type, the first time a key is written to.
- The mapping from surrogate to key is itself a log record, whose body can be erased.
- Change-set payloads refer to the entity by its key. They are in the body, which can be erased.
- An index projected from the mapping records resolves keys to surrogates when a change set is appended.

This is the same pattern as the actor surrogates in [0007](0007-actor-identity.md) §5. Erasing a Domain by key erases the mapping record along with everything else keyed to the surrogate, and only an opaque number remains.

### 8. Keyed types rank first in identity clusters (amends 0004 §4)

A keyed type is a namespace of its own in the sense of [0004](0004-identity-clusters-and-equivalence.md) §1. Its keys are minted by the thing itself, not by a provider. A cluster holds at most one member from it, as from any other namespace.

**A keyed member always ranks first,** above a local member. The canonical order becomes: keyed type, then local, then the providers in the instance's order. So:

- If a local item `Q77` about en.wikipedia.org existed before Domains did, `same-as` between `Q77` and `en.wikipedia.org` makes the Domain canonical. `Q77` resolves to it and gets the redirect-form `owl:sameAs` in RDF.
- Mirror-derived links (tier 2) can join a Domain to a mirrored entity through a configured link property, as for any other cluster.
- A cluster whose links would join two Domains is a conflict, and the links are held ([0004](0004-identity-clusters-and-equivalence.md) §10).

The cluster rules do not apply to the provider mapping in §9. That mapping defines which Domain an upstream item is, so it is resolved by the adapter before any cluster is built.

### 9. Key-mapped providers (amends 0002 §4 and §5)

internetdomains.wiki is mirrored like any other Wikibase, with one difference: its domain items are **mapped by key**, not rewritten to prefixed IDs.

**Provider registry entry.** The provider gets a two-letter code (open questions) for the parts of it that are not domains. Its adapter configuration names, by role ([0003](0003-statement-ui.md) §7), the **identity property**: the upstream property whose value is the domain name.

**What the adapter does:**

- **An upstream item with one valid identity value** is written to the mirror graph with the normalized Domain key as its subject. Its upstream item ID never appears as a subject.
- **Values that point at such items** are rewritten to `wikibase-domain` values with the same key.
- **Upstream items without an identity value,** such as registrars, organizations or classes, stay ordinary foreign items under the provider's prefix.
- **An identity value that does not normalize to a valid key** is reported as a reject. The item stays an ordinary foreign item.
- **An item with more than one identity value** is reported and not mapped.

**The upstream ID becomes an alias.** A mapping index records upstream ID ↔ key, together with the version cursor ([0002](0002-source-graphs-and-mass-ingest.md) §8.4). Sync jobs use it, because upstream reports redirects and deletions by item ID. The API and the title resolver use it too, so the prefixed upstream ID resolves to the Domain page.

**The canonical IRI is the key's, not the provider's.** This amends [0002](0002-source-graphs-and-mass-ingest.md) §4, where a foreign entity's canonical IRI is the provider's own. For a mapped item, the resolved view uses `https://scatter.red/domain/{key}` and emits `<upstream item IRI> owl:sameAs <https://scatter.red/domain/{key}>`. Nodes that come from upstream, such as statement nodes, keep their upstream IRIs as before.

**Upstream changes:**

| Upstream event | Effect |
|---|---|
| An item's identity value changes, for example a typo is fixed | The mirror's state for the old key is cleared, and the item's state is written under the new key. Local assertions about the old key stay where they are, because the old name still exists |
| An item is deleted | The mirror's state for that key is cleared (below) |
| Two items are merged | The redirect updates the mapping index. If both carried the same key, the duplicate is resolved |
| Two items carry the same identity value | A conflict. Neither item's statements are attached to the key until the duplicate is resolved upstream. The conflict is listed with the others ([0004](0004-identity-clusters-and-equivalence.md) §10) |

**Retention works differently for keyed entities** (amends [0002](0002-source-graphs-and-mass-ingest.md) §5). When upstream deletes a domain item, the domain does not stop existing. So:

- **The mirror's contribution is cleared, and nothing else happens.** Local assertions are never retracted or hidden because of an upstream deletion. `cascade` and `orphan` do not apply to keyed entities.
- **`retain` still applies.** It keeps the mirrored history and materializes the last mirrored state into the local graph when the deletion arrives, as in 0002 §5.
- The record that clears the mirror state is a `put` of an empty state, not a `tombstone`. A `tombstone` applies retention policies that assume the entity is gone.

### 10. Operations

The operations of [0002](0002-source-graphs-and-mass-ingest.md) §8.2 and [0004](0004-identity-clusters-and-equivalence.md) §9 apply to Domains as follows:

| Operation | Applies to a Domain |
|---|---|
| `put` | Yes, from a key-mapped adapter (§9) |
| `tombstone` | No. Upstream deletion is an empty `put` (§9) |
| `redirect` | No (§4) |
| `create`, `convert` | No (§4) |
| `add`, `remove`, `override` | Yes |
| `retain` | Yes (§9) |
| `same-as`, `different-from` | Yes, with an entity of another namespace (§8). Never between two Domains |

A local bulk job:

```
{"job":{"source":"citation-domains","graph":"local"}}
{"op":"add","id":"en.wikipedia.org","claims":{"P12":[…]}}
{"op":"add","id":"Q5","claims":{"P40":[{"mainsnak":{"datavalue":{"value":{"entity-type":"domain","id":"doi.org"},"type":"wikibase-entityid"},…}}]}}
{"op":"same-as","ids":["Q77","en.wikipedia.org"]}
```

### 11. The Domain namespace

| Field | Value |
|---|---|
| Canonical name | `Domain` |
| Kind | Entity view, hosting type `domain` ([0008](0008-namespaces-and-document-pages.md) §1) |
| Normalizer | The Domain normalizer (§2) |
| Reported `case` | `case-sensitive` ([0008](0008-namespaces-and-document-pages.md) §3) |
| Subpages | No. A `/` is not valid in a key |
| Talk namespace | `Domain talk`, composite since [0019](0019-discussions.md) §2 |
| Number | 210, with `Domain talk` at 211 ([0008](0008-namespaces-and-document-pages.md) §2, as amended 2026-09-27) |

### 12. Crates (amends 0005 §2)

| Layer | Crate | Contents | Depends on |
|---|---|---|---|
| Substrate | `scatter-keyed` | The keyed-type registry, key grammars and normalizers, IRI templates, and the surrogate allocator trait | `oxrdf`, `idna` |

- `scatter-keyed` is pure ([0005](0005-crate-organization.md) §3 rule 2) and builds for `wasm32-unknown-unknown`, so the UI normalizes titles with the same code as the server.
- UTS #46 processing comes from the `idna` crate, which `url` uses.
- `scatter-wikibase-model` gains the `domain` entity type, the `wikibase-domain` data type, and the ID parser in §3.
- The key-mapping rules in §9 go in `scatter-wikibase-changeset`'s `Adapter` trait as an optional capability. The adapter for internetdomains.wiki is a new crate, `scatter-adapter-internetdomains`.

## Consequences

- **Domains need no ID allocation.** Bulk ingest of domains is idempotent and needs no lookups, which suits URL-heavy citation data.
- **Two instances share Domain IRIs.** Their RDF about the same domain uses the same subject without an alignment step.
- **The `scatter.red` domain carries more weight.** It now names data subjects, not just vocabulary terms and content-derived claim IDs. What it serves at those IRIs becomes a commitment of its own.
- **Stock Wikibase clients will not understand Domain values.** `wikibase-domain` is a new data type and `domain` a new entity type. Tools like Pywikibot read the raw JSON but cannot format or validate these values.
- **Every present Domain costs a surrogate and a mapping record.** Erasability is worth that, but it adds an index lookup to every write.
- **Canonical IRIs can move to a Domain.** When a local item is linked to a Domain, its canonical IRI changes (§8). Consumers have to follow `owl:sameAs`, as 0004 already requires.
- **internetdomains.wiki's data quality shows through.** Duplicate identity values hold data back until they are resolved upstream, and invalid values leave items unmapped.

## Open questions

- ~~**Namespace number.** An unused subject and talk pair has to be chosen and registered on mediawiki.org's [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces) page.~~ *Settled: 210/211, in the 210–219 range ([0008](0008-namespaces-and-document-pages.md) §2, as amended). Filing the registration on mediawiki.org is a pending action in the registry README, not a decision.*
- **Provider code** for internetdomains.wiki's non-domain items, under the provider registry that is still open in [0000](0000-init.md).
- **The identity property.** Which internetdomains.wiki property carries the domain name, and whether its values are stored as A-labels or U-labels.
- **Resolving duplicates locally.** Whether the instance may choose which upstream item maps to a key while a duplicate stays unresolved upstream, and with what operation.
- **What `scatter.red` serves** at `https://scatter.red/domain/{key}`: nothing, a 303 redirect to an instance, or a small document.
- ~~**The next keyed types.** DOI is the obvious candidate. Its normalizer exists ([0004](0004-identity-clusters-and-equivalence.md) §7), but its IRI would be `https://doi.org/{key}`, and a DOI can contain almost any character, so its key grammar and its ID disjointness need their own analysis.~~ *Settled by [0017](0017-entity-id-grammar.md) §5 (Keyword is the second keyed type; disjointness comes from the `type:` prefix) and [0029](0029-resolver-namespaces.md) §1 (DOI is a resolver, not a keyed type: it names a thing that already has an item). 0029's Consequences state the rule for the next identifier.*
- **Tier-2 link properties.** Which Wikidata properties, if any, should link Wikidata items to Domains.

## References

- [UTS #46: Unicode IDNA Compatibility Processing](https://www.unicode.org/reports/tr46/)
- [RFC 5890](https://www.rfc-editor.org/rfc/rfc5890) (IDNA definitions: A-label and U-label), [RFC 1035](https://www.rfc-editor.org/rfc/rfc1035) §2.3.4 (size limits)
- [ICANN gTLD Applicant Guidebook](https://newgtlds.icann.org/en/applicants/agb), string requirements (ASCII top-level labels consist of letters only)
- [Wikibase JSON format](https://doc.wikimedia.org/Wikibase/master/php/docs_topics_json.html) (entity values without `numeric-id`)
- [`idna` crate](https://crates.io/crates/idna)
- [internetdomains.wiki](https://internetdomains.wiki/)
