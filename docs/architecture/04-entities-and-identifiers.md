# 04. Entities and identifiers

This chapter describes what an entity ID is in Triplespace: the forms an ID takes, the grammar of each minted type, the keyed types whose ID is a natural key rather than a minted number, and the identity clusters that say when IDs from different namespaces denote one thing and which of them is canonical. It also covers lexemes and MediaInfo as entity types, the derived IDs of foreign pages, the per-tenant Lua ID space, and where an ID is stored and shown. It assumes the log and record model of [01](01-log-and-records.md), the graphs and resolved view of [02](02-graphs-rdf-and-query.md), and the `view` schema of [03](03-storage-caches-and-search.md). Who minted an ID is distinguished here from who asserts a triple about it; the source graphs, key-mapped providers and entity sources that the second question leads to are in [05](05-providers-and-ingest.md), and the statement model is in [06](06-statements-and-properties.md). The registry files `docs/registry/providers.toml`, `keyed-types.toml` and `notation-schemes.toml` are authoritative for every code, type and scheme this chapter names; the chapter cites them and does not repeat them.

## 1. Local and foreign entities

*Sources: [0000](../decisions/0000-init.md) §3; [0017](../decisions/0017-entity-id-grammar.md) §1.*

A foreign entity can be referenced directly, without first being reified as a local item. Identifiers distinguish the two: **local entities** keep Wikibase's single-letter prefixes (`Q`, `P`, and so on), and **foreign entities** use a three-letter prefix, a two-letter provider code followed by a one-letter type code. **A prefix records who minted an ID, not who asserts its triples.** `WDQ123` was minted by Wikidata, and `Q5` by this instance. Which named graph a triple lives in says who asserts it, so an entity with a foreign ID can have triples in a provider's mirror graph, in the local graph, or in both ([05](05-providers-and-ingest.md)).

**Provider codes and type codes are registry data.** `docs/registry/providers.toml` allocates them, and is authoritative. The first three providers were Wikidata (`WD`: `WDQ` items, `WDP` properties, `WDL` lexemes, `WDM` Commons MediaInfo), Librarybase (`LB`) and OpenAlex (`OA`: `OAW` works, `OAA` authors and the rest). After the type code, an ID follows its type's grammar, which need not be digits (§2.2), and OpenAlex keywords are a keyed type rather than `OAK` entities (§3.7).

## 2. The forms of an entity ID

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §1, §2, §6; [0044](../decisions/0044-tenant-relative-ids.md) §1, §2; [0066](../decisions/0066-lexemes.md) §4.*

### 2.1 The ID forms

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §1; [0066](../decisions/0066-lexemes.md) §4; [0044](../decisions/0044-tenant-relative-ids.md) §1.*

Every entity ID has one of these forms, told apart by its first character, by the presence of a colon and, for a colon form, by the kind of its prefix in the tenant's table of names ([0078](../decisions/0078-entity-sources.md) §3, in [05](05-providers-and-ingest.md)):

| Form | Shape | Examples | Minted by |
|---|---|---|---|
| **Local** | One uppercase letter and digits | `Q5`, `P12`, `L3` | This instance ([0000](../decisions/0000-init.md) §3) |
| **Local, lexeme part** | `L`, digits, then `-F` or `-S` and digits; on `L` only | `L3-F1`, `L3-S2` | This instance; the part belongs to the lexeme's record stream (§5.1). Foreign: `WDL3-F1`; input form `LLL3-F1` |
| **Foreign** | Two-letter provider code, one-letter type code, and the rest in the type's **ID grammar** (§2.2) | `WDQ42`, `OAW123`, `MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d` | A provider ([0000](../decisions/0000-init.md) §3, [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §1) |
| **Keyed** | The keyed type's name, a colon, and the normalized key | `domain:en.wikipedia.org`, `keyword:p53` | The thing itself (§3) |
| **Source** | An entity source's name, a colon, and the source's own ID in the type's grammar | `mhc:Q1`, `mhc:P31` | An entity source the tenant declares ([0078](../decisions/0078-entity-sources.md) §1–2). A foreign ID in the colon shape, relative to the tenant |

No local or provider-form ID contains a colon, and every keyed and source ID does, so they never collide, whatever a key looks like; a colon form's prefix names one kind of thing on a tenant, so keyed and source IDs never collide either ([0078](../decisions/0078-entity-sources.md) §3). A registry provider's slug before a colon, `wikidata:Q42`, is an input form of the provider-form ID, `WDQ42` ([0078](../decisions/0078-entity-sources.md) §3). The colon form is the one Wikibase already uses for entities from another repository (`wikidata:Q42` in a federated install), so clients that parse federated IDs parse these.

The lexeme-part suffix is part of the ID wherever an ID is stored, compared or shown; `L1-F1` and `L1` are different IDs of one record stream. No other type takes a suffix; a provider type that minted one would need an ID grammar of its own ([0066](../decisions/0066-lexemes.md) §4).

**Statement IDs** keep Wikibase's form, `<EntityId>$<UUID>`: `domain:en.wikipedia.org$C7C2A847-…`, `L1-F1$<uuid>`. **Entity values** carry the same ID: `{"entity-type": "domain", "id": "domain:en.wikipedia.org"}`. How a statement ID reads when its entity belongs to an identity cluster is §4.9.

**Titles are unchanged.** In an entity namespace the title is the namespace and the bare key or ID, as [0008](../decisions/0008-namespaces-and-document-pages.md) §1 and [0009](../decisions/0009-keyed-entity-types-and-domain.md) §11 say (both in [10](10-pages-and-content-models.md)): `Domain:en.wikipedia.org`, `Keyword:p53`, `Item:MBAb10bbbfc-…`. The namespace supplies what the colon supplies in an ID. `Special:EntityPage/{id}` takes the ID form.

**Bare keys are not IDs.** `wbgetentities&ids=en.wikipedia.org` is an `invalid-entity-id`. The title resolver and `GET /resolve` ([0012](../decisions/0012-api-requirements.md) §5) still recognise a bare string that normalizes to a valid key of some keyed type and offer it as a suggestion, so the search box's "Go to" ([0010](../decisions/0010-site-ui.md) §3) keeps working for a pasted domain name.

**Derived IDs.** `M` and digits, a local-form ID, names the statements of the File page with that page ID (WikibaseMediaInfo's contract). It is derived, not minted: never in `view.entity`, never in a cluster, never the value of an entity data type (§5.2). A foreign page's page ID is derived in the same sense (§5.3).

**An input form.** A type letter written three times and the rest of a local ID, `QQQ5`, is an **input form** of the local ID on the tenant that reads it (§2.3). It is canonicalized to `Q5` wherever IDs are accepted and never stored or returned, except as Lua output under a remapped letter (§5.4).

### 2.2 ID grammars for minted types

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §2; [0017](../decisions/0017-entity-id-grammar.md) §6; [0044](../decisions/0044-tenant-relative-ids.md) §2.*

Each foreign entity type declares an **ID grammar** in `providers.toml`. It says what may follow the three-letter prefix and what the canonical spelling is.

| Grammar | Rest of the ID | Canonical form | Used by |
|---|---|---|---|
| `digits` | One or more decimal digits | As given; leading zeros are significant where upstream has them (a CAMEO code is `0311`, [0037](../decisions/0037-gdelt-provider.md) §2). Providers that never mint them, such as Wikidata, never produce them | Wikidata, Librarybase, OpenAlex (the default) |
| `uuid` | An RFC 4122 UUID in its 36-character hyphenated text form | Lowercase | MusicBrainz |
| `gdelt-record` | 14 digits, `-`, an optional uppercase `T`, digits | As given | GDELT documents ([0037](../decisions/0037-gdelt-provider.md) §2) |
| `token` | Uppercase letters, digits and underscores, starting with a letter, at most 128 characters | As given | GDELT themes ([0037](../decisions/0037-gdelt-provider.md) §2) |

Input is case-insensitive in every position. **Canonicalizing an ID uppercases its prefix and applies the grammar's canonical form to the rest**, so `mbab10BBBFC-…` becomes `MBAb10bbbfc-…`. The `entity-id` normalizer of [0008](../decisions/0008-namespaces-and-document-pages.md) §3 therefore does not uppercase the whole ID, and the ID rewriting of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4 is: strip or add the prefix, and canonicalize the rest by the type's grammar. The rewriting of upstream IDs also honours `upstream_prefix`, so `keywords/…` and MusicBrainz's bare MBIDs both round-trip.

A grammar is declared per provider and may be set on a single type, overriding the provider's ([0037](../decisions/0037-gdelt-provider.md) §2). A new grammar is added by ADR, as a value the registry accepts. Nothing else in the system needs to know a grammar's rules: the ID parser in `scatter-wikibase-model` ([0005](../decisions/0005-crate-organization.md) §2) reads them from the registry passed in as data (0005 §3, rule 3).

**Reserved codes.** The twenty-six provider codes `AA` through `ZZ` are reserved in `providers.toml` and are never allocated, so that the tenant-relative input form (§2.3) is unambiguous: a provider with code `QQ` would make `QQQ5` mean two things. No allocated code is a doubled letter (`WD`, `LB`, `OA`, `MB`, `XD`, `OS`, `OW`, `GD`), so nothing moves. Reserving all twenty-six, not only the letters local types use today, means a type letter added later already has its tenant-relative form.

**MusicBrainz** is the provider whose grammar is `uuid`. It is registered in `providers.toml` with code `MB`, slug `musicbrainz` and provider number 4 ([0015](../decisions/0015-record-format-and-partition-registry.md) §2, §5). Every type has ID grammar `uuid`, so an ID is the prefix and the MBID verbatim: `MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d`. Keeping the MBID intact was preferred to a shorter encoding because the ID stays recognisable and pastes into musicbrainz.org unchanged.

| Code | Type | Code | Type |
|---|---|---|---|
| `A` | Artist | `P` | Place |
| `G` | Release group | `E` | Event |
| `R` | Release | `S` | Series |
| `C` | Recording | `I` | Instrument |
| `W` | Work | `N` | Genre |
| `L` | Label | `D` | Area |

MusicBrainz `url` entities are not mirrored as items; the adapter turns them into URL-valued statements on the entity that links them. Concept IRIs are `https://musicbrainz.org/{type}/{mbid}`. MusicBrainz has global edit numbers but no per-entity revision IDs, so `revision_ids` is false and a mirror record's revision ID uses its offset (0015 §2). Its issuer, `musicbrainz`, is provider-only ([0007](../decisions/0007-actor-identity.md) §6) until edit history is mirrored, since the data the adapter reads carries no editors. The adapter is the crate `scatter-adapter-musicbrainz` ([22](22-crates-and-stack.md)).

### 2.3 Tenant-relative IDs

*Sources: [0044](../decisions/0044-tenant-relative-ids.md) §1; [0044](../decisions/0044-tenant-relative-ids.md) §2; [0017](../decisions/0017-entity-id-grammar.md) §1; [0080](../decisions/0080-tenants-as-entity-sources.md) §7.*

**A type letter written three times, then the rest of a local ID, names that local entity of the tenant where it is read:**

| Written | Means, on the tenant reading it |
|---|---|
| `QQQ5` | `Q5` |
| `PPP31` | `P31` |
| `LLL3` | `L3` |
| `MMM1234` | `M1234`, the MediaInfo ID of File page 1234 ([0041](../decisions/0041-content-models.md) §7) |

It has the shape of a foreign ID whose provider code is the type letter doubled. **It is an input form of the local ID, not a fourth form:**

- **It is never stored.** It is canonicalized to the local form wherever an ID is accepted, before anything is validated, hashed, appended or cached. No record, `view` row, cache key, search document or dump contains it.
- **It is never the canonical output.** Responses use the local form, `Q5`. The one place the tenant-relative form is written out is Lua under a mapped letter (§5.4), where the bare form already means something else.
- **It is relative.** `QQQ5` means a different entity on each tenant, as `Q5` does. To name a tenant's entity from elsewhere, use its IRI; on a tenant that reads it, use that tenant's name for it, the registry code where there is one (`LBQ6`) and otherwise the tenant's source name (`lb:Q6`) ([0018](../decisions/0018-tenants.md) §5, in [08](08-tenants-and-instances.md); [0080](../decisions/0080-tenants-as-entity-sources.md) §7).

Input is case-insensitive, as every ID is (§2.2): `qqq5` is `Q5`. **The rest of the ID follows the local type's grammar**, which for every local type is `digits`. `QQQb10bbbfc-…` is not an ID.

## 3. Keyed entity types

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §1, §2, §3, §4, §5, §6, §7, §8, §10; [0017](../decisions/0017-entity-id-grammar.md) §3, §5; [0026](../decisions/0026-sitelinks.md) §5; [0033](../decisions/0033-backend-stack.md) §9.3; [0036](../decisions/0036-openstreetmap-providers.md) §3, §6; [0048](../decisions/0048-notation.md) §1, §2, §3, §4, §6.*

### 3.1 What a keyed type is

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §1; [0017](../decisions/0017-entity-id-grammar.md) §3; [0048](../decisions/0048-notation.md) §6.*

A **keyed entity type** is an entity type whose ID is a normalized natural key, not a minted identifier. Each keyed type is a registry entry, recorded in the log as configuration (§4.10). The entry records:

- the **type name**, used as the `type` in Wikibase JSON;
- the **namespace** its pages live in ([0008](../decisions/0008-namespaces-and-document-pages.md) §1);
- the **key grammar**: which strings are valid keys;
- the **normalizer**: how input is turned into the one canonical key;
- the **IRI template** (§3.5);
- the **display rule**: how the key is shown as a label, **derived** or **editable** (§3.6, §3.7);
- the **data type** used to refer to entities of this type (§3.2);
- the **ID prefix**: the type's name and a colon (§3.2);
- whether the type's entities **join identity clusters**, `clusters` (default true; false for keyword and notation, §4.1);
- a **label** for the type.

**Domain is the first keyed type registered.** The mechanism is general so that others can be added as registry entries: Keyword (§3.7) and Notation (§3.8) have been. `docs/registry/keyed-types.toml` is authoritative. Normalizers come from `scatter-normalize` (§3.10).

### 3.2 Keyed IDs, JSON and values

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §3; [0017](../decisions/0017-entity-id-grammar.md) §3.*

**The entity ID is the type's prefix and the key**, `domain:en.wikipedia.org`. The canonical ID is the prefix, in lowercase, and the key in the form the type's normalizer produces. A key may itself carry a scheme: the `notation` type's key is a scheme name, a colon and the string in the scheme's canonical form, so its ID is `notation:osm:amenity=cafe` (§3.8). In Wikibase JSON a Domain is:

```json
{"type": "domain", "id": "domain:en.wikipedia.org", "labels": {…}, "descriptions": {…}, "claims": {…}, "lastrevid": …}
```

**Referring to a keyed entity uses the type's data type,** `wikibase-domain` for a Domain. Its value type is `wikibase-entityid`, with an `id` and no `numeric-id`:

```json
{"value": {"entity-type": "domain", "id": "domain:en.wikipedia.org"}, "type": "wikibase-entityid"}
```

Wikibase already has entity values without a `numeric-id`: the Lexeme extension's forms and senses (`L1-F1`, `L1-S1`) are written this way.

**Statement IDs follow the Wikibase form** `<EntityId>$<UUID>`, so a local statement on a Domain has an ID like `domain:en.wikipedia.org$C7C2A847-…`. `$` cannot occur in a key.

The concept IRI is `https://scatter.red/domain/{key}` (§3.5): the prefix is part of the ID, not of the key. In a change set the prefixed form is the subject (§3.3), and a key-mapped upstream item is written under `domain:{key}` ([05](05-providers-and-ingest.md)).

### 3.3 A keyed entity exists because its key is valid

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §4; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §10; [0048](../decisions/0048-notation.md) §1.*

**Every valid key denotes a Domain, whether or not anything has been said about it.** There is no operation that creates one. The same holds for every keyed type: a notation exists because its key is valid, with no create, redirect or delete ([0048](../decisions/0048-notation.md) §1).

- **The first `add` is enough.** Change sets refer to a Domain by its key. They never allocate an ID, need a temporary ref, or need a match key ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.5). Re-running an ingest is idempotent by construction.
- **Valid and present are different.** A Domain is **present** when some graph asserts something about it. `wbgetentities` returns an empty entity (its derived label, §3.6, and no statements) for a valid ID that is not present, such as `domain:example.org`, and an `invalid-entity-id` error for an invalid one or a bare key. It never returns `missing` for a valid ID. Page listings such as `list=allpages` show only present Domains.
- **There is no delete.** Retracting every local assertion leaves the Domain valid and, if no mirror says anything, not present. Erasure by key works as for any other entity ([0006](../decisions/0006-log-integrity-and-erasure.md) §7, in [01](01-log-and-records.md)).
- **There are no merges or redirects between Domains,** and no `same-as` between two of them. Two different keys are two different names. That `wikipedia.com` forwards to `wikipedia.org` is a fact about HTTP, recorded as a statement, not an identity.
- **`convert` does not apply.** A Domain is already canonical (§4.4).

The operations of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.2 and §4.10 apply to Domains as follows:

| Operation | Applies to a Domain |
|---|---|
| `put` | Yes, from a key-mapped adapter ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §9, in [05](05-providers-and-ingest.md)) |
| `tombstone` | No. Upstream deletion is an empty `put` (0009 §9) |
| `redirect` | No |
| `create`, `convert` | No |
| `add`, `remove`, `override` | Yes |
| `retain` | Yes (0009 §9) |
| `same-as`, `different-from` | Yes, with an entity of another namespace (§4.1). Never between two Domains |

A local bulk job:

```
{"job":{"source":"citation-domains","graph":"local"}}
{"op":"add","id":"domain:en.wikipedia.org","claims":{"P12":[…]}}
{"op":"add","id":"Q5","claims":{"P40":[{"mainsnak":{"datavalue":{"value":{"entity-type":"domain","id":"domain:doi.org"},"type":"wikibase-entityid"},…}}]}}
{"op":"same-as","ids":["Q77","domain:en.wikipedia.org"]}
```

Upstream items can be key-mapped onto any keyed type; the mapping is in [05](05-providers-and-ingest.md).

### 3.4 Log keys are surrogates

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §7; [0017](../decisions/0017-entity-id-grammar.md) §3; [0048](../decisions/0048-notation.md) §1.*

A domain name can be personal data: a person's own name registered as a domain, for example. A log header key survives erasure ([0006](../decisions/0006-log-integrity-and-erasure.md) §3, §7), so a key must not be the domain name itself. A hash of the name is no better, because domain names can be enumerated from zone files and any hash can be checked against them.

**Each present keyed entity gets a surrogate for log headers.** The rules are:

- The instance mints the surrogates in sequence, one per keyed type, the first time a key is written to.
- The mapping from surrogate to key is itself a log record, whose body can be erased.
- Change-set payloads refer to the entity by its key. They are in the body, which can be erased.
- An index projected from the mapping records resolves keys to surrogates when a change set is appended.

This is the same pattern as the actor surrogates in [0007](../decisions/0007-actor-identity.md) §5 ([07](07-actors-and-accounts.md)). Erasing a Domain by key erases the mapping record along with everything else keyed to the surrogate, and only an opaque number remains. The log never sees the key, only the surrogate; this holds for every keyed type, with one sequence per type.

**The forms.** The header key of a keyed entity's records is `{type}#{n}`, `domain#17`: the `#` keeps it from being any entity ID, since no ID form contains one. The mapping record is a `scatter:v0/keyed-surrogate` record in the **instance `log`**, keyed by the same surrogate, with content `{keyed_type, surrogate, key}` ([payloads.md §3.4](../api/payloads.md)); surrogates are instance-wide, so the mapping does not belong to any tenant, and the record is appended before the first record under the surrogate. `view.keyed_surrogate` is projected from it at the instance tenant (`''`); a projection that folds a keyed entity's records looks the surrogate up there, and fails rather than guess when it is missing, which makes a rebuild that orders the instance `log` after a tenant partition fail loudly instead of silently dropping the tenant's assertions. The table is in [03](03-storage-caches-and-search.md).

A source entity (§2.1) needs no surrogate: its log header key is its ID, as `WDQ42`'s is ([0078](../decisions/0078-entity-sources.md) §2).

### 3.5 IRIs under `scatter.red`

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §6; [0017](../decisions/0017-entity-id-grammar.md) §3.*

**A Domain's concept IRI is `https://scatter.red/domain/{key}`,** for example `https://scatter.red/domain/en.wikipedia.org`. The key is used in A-label form, so the IRI is plain ASCII.

The IRI depends only on the key, so every instance mints the same one. Under [0005](../decisions/0005-crate-organization.md) §5, identifiers derived only from content are not instance data and may live under `scatter.red`, as Scatterbase's claim and triple IRIs do. Two Triplespace instances that describe the same domain therefore describe the same RDF subject, and their data can be merged without an alignment step.

**The document node is still instance data.** Revision metadata attaches to an entity's document node, not to the concept ([0001](../decisions/0001-revision-metadata-rdf.md) §1). A Domain's document node is `{base}/wiki/Special:EntityData/{id}` with the prefixed ID, `…/Special:EntityData/domain:en.wikipedia.org`, on each instance, as for every entity ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §4, [0017](../decisions/0017-entity-id-grammar.md) §1), so each instance's history stays its own.

**The keyed type's IRI template is registry data.** The template for Domain is `https://scatter.red/domain/{key}`. A later keyed type may use an external IRI instead, such as `https://doi.org/{key}` for DOIs. The templates in `keyed-types.toml` are `https://scatter.red/keyword/{key}` and `https://scatter.red/notation/{key}` for the other two types (§3.7, §3.8).

### 3.6 Domain

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §2; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §5; [0026](../decisions/0026-sitelinks.md) §5; [0048](../decisions/0048-notation.md) §3.*

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

**Either form is accepted as input.** `Domain:bücher.example`, `Domain:XN--BCHER-KVA.example` and `Domain:xn--bcher-kva.example.` all normalize to `xn--bcher-kva.example`. Page titles redirect to the normalized form ([0008](../decisions/0008-namespaces-and-document-pages.md) §3). The U-label form is used for display.

**Terms.**

- **The label is derived from the key and cannot be edited.** This is the Domain's **derived** display rule; a keyword's labels follow the **editable** rule instead (§3.7). The resolved view emits one label in the language `mul`, whose value is the U-label form of the key. Wikidata uses `mul` for labels that hold in every language. Labels in source graphs are kept there, and the per-graph views ([0003](../decisions/0003-statement-ui.md) §6) still show them, but the resolved view ignores them. `wbsetlabel` on a Domain fails with a Triplespace error code, `derived-label`.
- **The A-label is a derived alias** when it differs from the U-label.
- **Descriptions and other aliases** are stored and reconciled like an item's ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3).
- **Domains have no sitelinks.** Every sitelink's host is a valid Domain key by construction, though ([0026](../decisions/0026-sitelinks.md) §1, in [06](06-statements-and-properties.md)). A sitelink does **not** make its host's Domain present: linking a page asserts nothing about its host. But the host index of 0026 §6 answers "which entities link to this site", and the Domain page's header shows the count with a link to the list, beneath the DNS hierarchy it already shows. A `key_parents` query gives the count for a zone.

**The page shows the DNS hierarchy.** The header of `Domain:en.wikipedia.org` links its parents, `org` and `wikipedia.org`, by removing labels from the left. This is navigation, not data: no statements are derived and nothing is emitted in RDF. The Public Suffix List is not used, because the core should not depend on an external list that changes.

**Domain stays its own type** rather than a notation scheme (§3.8), for three reasons: Domains join identity clusters, and notations never do; its derived label is a conversion (the U-label), not the string itself; and sitelink hosts and source domains ([0037](../decisions/0037-gdelt-provider.md) §3) depend on it.

The Domain namespace (210, `Domain talk` 211) and its content model `triplespace-domain` are in the namespace table of [10](10-pages-and-content-models.md).

### 3.7 Keyword

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §5; [0048](../decisions/0048-notation.md) §3.*

A **keyword** is a short phrase describing what a work is about. It is the phrase, not the thing the phrase denotes.

| | |
|---|---|
| Type name, ID prefix | `keyword`, `keyword:` |
| Namespace | `Keyword`, 212, with `Keyword talk` at 213 ([0008](../decisions/0008-namespaces-and-document-pages.md) §2) |
| Data type | `wikibase-keyword`, with value type `wikibase-entityid` as `wikibase-domain` has |
| IRI | `https://scatter.red/keyword/{key}`, content-derived and so shared by every instance ([0005](../decisions/0005-crate-organization.md) §5, §3.5) |
| Grammar | 1 to 200 characters of letters, digits and single hyphens; at least one letter; no leading or trailing hyphen |
| Normalizer | Unicode NFKC; casefold; every run of whitespace or punctuation becomes one hyphen; leading and trailing hyphens are removed |

**Labels are editable, not derived.** A Domain's label is derived from its key because the key is the name. A keyword's key loses case and punctuation (`dna`, `type-1-diabetes`), so its labels and descriptions are stored and reconciled like an item's ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3). When no label exists in the requested language, the UI shows the key with hyphens as spaces. The keyed-type registry therefore distinguishes a **derived** display rule from an **editable** one.

**Keywords never join identity clusters** (§4.1): a Wikidata item about machine learning is the concept, and `keyword:machine-learning` is a phrase. A keyword may carry an ordinary statement pointing at the concept it usually denotes, through a role in the role map ([0003](../decisions/0003-statement-ui.md) §7); that is data, not identity. **Keyword stays its own type** beside notation: convergence is the point of a keyword, and a notation preserves the distinctions a keyword erases ([0048](../decisions/0048-notation.md) §3).

**OpenAlex keywords are mapped by key** ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §9, in [05](05-providers-and-ingest.md)), as internetdomains.wiki's items are mapped to Domains. The adapter turns `https://openalex.org/keywords/{slug}` into the key by running the slug through the normalizer, writes the entity's `display_name` as the mirror graph's English label, emits `<https://openalex.org/keywords/{slug}> owl:sameAs <https://scatter.red/keyword/{key}>`, and turns a work's `keywords` list into `wikibase-keyword` values with the score as a qualifier by role. The `OAK` type code stays reserved in the registry and no `OAK` entity is ever minted.

DOI is not a keyed type but a resolver ([0029](../decisions/0029-resolver-namespaces.md) §1, in [06](06-statements-and-properties.md)), and resolver names and keyed-type names share one namespace, so `doi:10.1000/xyz` can mean only one thing ([0029](../decisions/0029-resolver-namespaces.md) §2).

### 3.8 Notation and its schemes

*Sources: [0048](../decisions/0048-notation.md) §1; [0048](../decisions/0048-notation.md) §2; [0048](../decisions/0048-notation.md) §6; [0036](../decisions/0036-openstreetmap-providers.md) §3.*

A **notation** is a string that names something in an outside vocabulary, taken exactly as that vocabulary's own rule says. Its **scheme** says which vocabulary. The name follows SKOS, where `skos:notation` is "a string of characters … used to uniquely identify a concept within the scope of a given concept scheme".

| | |
|---|---|
| Type name, ID prefix | `notation`, `notation:` |
| Key | The scheme name, a colon, and the string in the scheme's canonical form: `osm:amenity=cafe` |
| ID | `notation:osm:amenity=cafe` (§3.2: the prefix and the key) |
| Namespace | `Notation`, 216, with `Notation talk` at 217 ([0008](../decisions/0008-namespaces-and-document-pages.md) §2). Titles read `Notation:osm:amenity=cafe` |
| Data type | `wikibase-notation`, value type `wikibase-entityid` |
| Content model | `triplespace-notation` ([10](10-pages-and-content-models.md)) |
| IRI | `https://scatter.red/notation/{key}`, content-derived and shared by every instance (§3.5): `https://scatter.red/notation/osm:amenity=cafe` |
| Grammar and normalizer | The scheme's |
| Display | Derived: the string, without the scheme, as the `mul` label; `wbsetlabel` fails with `derived-label` as on a Domain (§3.6). Descriptions and aliases are stored and reconciled like an item's |
| Clusters | Never (§3.9) |

**Parsing a key.** A scheme name never contains a colon, so the first colon of the key ends the scheme name, and everything after it is the string. A string may contain colons of its own, as `osm:addr:street=Main Street` does. The scheme name is matched case-insensitively and written in lowercase. The string is then normalized by the scheme's normalizer and checked against the scheme's grammar. A key whose scheme is not registered is not a valid key.

The rest of what §3 says of a Domain applies unchanged: a notation exists because its key is valid, with no create, redirect or delete (§3.3); log headers carry a surrogate, one sequence for the type, not the string (§3.4); upstream items can be key-mapped onto notations ([05](05-providers-and-ingest.md)).

**Schemes** are registry data, in `docs/registry/notation-schemes.toml`, embedded by `scatter-normalize`. Each entry has:

| Field | Meaning |
|---|---|
| `name` | Lowercase ASCII letters, digits and single hyphens, starting with a letter, at most 32 characters. No colon |
| `label` | The vocabulary's name, shown in the UI's identity line ("OSM tag or key") |
| `normalizer` | One of a fixed set: `exact` (the input must already be NFC and is not changed), `nfc`, `casefold` (NFC, then full Unicode case folding, as §3.7 and [0029](../decisions/0029-resolver-namespaces.md) §1 use it) and `ascii-lowercase` (NFC, then ASCII letters lowercased) |
| `grammar` | The name of a grammar implemented in `scatter-normalize`, checked after normalizing: `osm-tag` (below), or `text`, which is 1 to 255 characters with no control characters and no leading or trailing whitespace |
| `documentation` | Optional. A URL template for the vocabulary's own page about a string, `{string}` being the string |
| `defined_in` | The ADR that registered the scheme |
| `retired` | Optional. A retired scheme stays in the file so that its name is never given to another vocabulary |

**A new scheme is a registry change, and a new grammar is code.** A scheme that names a grammar `scatter-normalize` does not implement is rejected when the file is parsed, as an unimplemented keyed type is. The normalizers are a closed set so that the rule that decides identity is never free text.

**Schemes are allocated in the registry only, never by an instance or a tenant.** A notation's ID and IRI are shared by every instance, so two instances that gave the name `foo` to different vocabularies would describe different things with one IRI. This differs from resolvers, which a tenant may add ([0029](../decisions/0029-resolver-namespaces.md) §8), because a resolver mints no shared identifier. A vocabulary a tenant wants for itself is modelled with ordinary items.

**Scheme names join the one namespace of keyed-type and resolver names** ([0029](../decisions/0029-resolver-namespaces.md) §2). `osm:amenity=cafe` typed into `/resolve` or the search box means the notation, as `doi:10.1000/x` means the DOI resolver. `/resolve` tries such input as a notation key after keyed IDs and before resolver strings. No scheme may share a name with a keyed type or a resolver, and the registry tests check it. Entity sources a tenant declares join the same table of names on that tenant ([0078](../decisions/0078-entity-sources.md) §3, in [05](05-providers-and-ingest.md)).

**The `osm` scheme.** OSM keys (`amenity`) and tags (`amenity=cafe`) are notations in the `osm` scheme: one scheme covers both, as Wikidata's P1282 is "OpenStreetMap tag or key". Keys never contain `=`, so the two forms cannot collide, and the wiki's `Key:` and `Tag:` page titles are the same strings.

```toml
[[scheme]]
name = "osm"
label = "OSM tag or key"
normalizer = "exact"
grammar = "osm-tag"
defined_in = "0036 §3, 0048 §6"
```

| | |
|---|---|
| Type, scheme, ID | `notation`, scheme `osm`: `notation:osm:amenity`, `notation:osm:amenity=cafe` |
| Data type | `wikibase-notation`, on properties whose `notation-scheme` is `osm` (§3.9) |
| IRI | `https://scatter.red/notation/osm:{key}` |
| Grammar | The scheme's `osm-tag` grammar: a key of 1 to 255 characters, or a key, `=` and a value of 1 to 255 characters. No control characters, no `=` in the key, no leading or trailing whitespace, Unicode NFC |
| Normalizer | The scheme's `exact` normalizer: the exact string, case-sensitive. Input that is not NFC is not a valid key |

**The keyword normalizer is not used.** Keyword (§3.7) casefolds and turns punctuation into hyphens, so `fast_food`, `fast-food` and `fast:food` would become one entity, and `Name` would merge with `name`. OSM tags are case-sensitive exact strings and those forms are different tags.

**Grammar over policy.** The grammar admits any valid OSM string. Which strings become entities is decided by the adapters ([0036](../decisions/0036-openstreetmap-providers.md) §4, §5, in [05](05-providers-and-ingest.md)), not the grammar.

The `osm` scheme has no `documentation` template. The wiki's pages are `Key:amenity` and `Tag:amenity=cafe`, two forms one template cannot produce, and a mapped notation already carries the wiki's documentation-page statements (0036 §5). The roles `osm-tag`, `osm-tag-text` and `osm-key` of 0036 §4 take values `notation:osm:k=v` and `notation:osm:k`, and the properties bound to `osm-tag` and `osm-key` carry `notation-scheme` `osm` (§3.9); the `OW` items of `Q7` and `Q2` are key-mapped to `notation`, with the key prefixed `osm:`, and the key map gains `key_scheme = "osm"`.

One scheme shares one namespace with every other scheme, which leaves 218 to 219 free. Splitting keys from tags into two schemes is a registry change, not a design change, if a use for the difference appears.

**Not yet.** One scheme ships, `osm`. The file drafts two candidates, commented out as `resolvers.toml` drafts its candidates ([0029](../decisions/0029-resolver-namespaces.md) §8): `hashtag`, `casefold`, for ActivityPub hashtags on federated posts ([0022](../decisions/0022-federation.md) §8), which needs a `hashtag` grammar; and `media-type`, `ascii-lowercase`, for IANA media types such as `text/html`, the `mime` field of an upload ([0039](../decisions/0039-files-and-media.md) §2), which needs a `media-type` grammar.

### 3.9 What a notation is, and what it is not

*Sources: [0048](../decisions/0048-notation.md) §3; [0048](../decisions/0048-notation.md) §4; [0036](../decisions/0036-openstreetmap-providers.md) §6.*

Four mechanisms give strings an identity. The question that tells them apart is who decides that two strings are the same thing.

| Mechanism | Use it when | Example |
|---|---|---|
| **Notation** | An outside system defines the string, and its own rule decides sameness. The set is open: anyone may coin a new string. No authority mints an ID for it. | `notation:osm:amenity=cafe` |
| **Keyword** (§3.7) | The string is a phrase about a work, and variants should converge whoever writes them. Labels are editable because folding loses case | `keyword:machine-learning` |
| **Provider item** ([0037](../decisions/0037-gdelt-provider.md) §2) | One provider publishes and identifies a finite list | `GDTTAX_FNCACT_MAYOR`, a GDELT theme |
| **Resolver** ([0029](../decisions/0029-resolver-namespaces.md)) | The string identifies a thing that already has an item | A DOI |

**Notations never join identity clusters** (§4.1), as keywords do not, for the same reason: `notation:osm:amenity=cafe` is a tag, and a Wikidata item about cafés is the concept. A notation may carry an ordinary statement pointing at the concept it usually denotes; `P7` and `P12` stay statements, so a user can follow a tag to its concept without the phrase becoming canonical over the concept.

**A property's scheme.** With one data type for every scheme, the data type no longer limits which strings a property takes. A property whose values should be OSM tags must not accept hashtags. A role, `notation-scheme`, names the property that restricts the scheme ([0003](../decisions/0003-statement-ui.md) §7, in [06](06-statements-and-properties.md)). It is a `string` property, and a `wikibase-notation` property may carry one or more statements with it whose values are scheme names. A value whose scheme is not among them is refused on the write path with `ts-notation-scheme`, in the same check that refuses a value of the wrong data type. A `wikibase-notation` property with no such statement takes any registered scheme, which is what a general "has tag" property needs.

**This is a type check, not a constraint** ([0031](../decisions/0031-property-constraints.md)). Constraints report and never refuse (0031 §2), and they read the graph after the fact. The scheme belongs with the data type: it is part of what kind of value the property takes, read from the property itself when its data type is read. A change to the scheme statement applies to later writes, and values already stored are left as they are.

The UI's value editor ([0003](../decisions/0003-statement-ui.md) §8) offers the property's schemes. When it has exactly one, the editor hides the scheme, so an editor types `amenity=cafe` and the editor stores `notation:osm:amenity=cafe`.

### 3.10 Normalizers and the libraries behind them

*Sources: [0033](../decisions/0033-backend-stack.md) §9.3; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §1.*

Normalizers come from `scatter-normalize` ([22](22-crates-and-stack.md)), which also embeds `notation-schemes.toml`. The backend stack fixes the libraries that identifiers, URLs and case depend on:

| Concern | Choice |
|---|---|
| URLs (0026 §1) | `url` (WHATWG parsing) |
| IDNA / UTS 46 (0009) | `idna` |
| Case folding (DOI resolver, 0029) and plural rules | `icu_casemap`, `icu_plurals` (ICU4X; wasm-friendly) |
| Unicode normalization | `unicode-normalization` |

## 4. Identity clusters

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §1, §2, §3, §4, §5, §6, §7, §8, §9, §10; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §8, §10; [0018](../decisions/0018-tenants.md) §7; [0066](../decisions/0066-lexemes.md) §4.*

### 4.1 Clusters and namespaces

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §1; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §8; [0017](../decisions/0017-entity-id-grammar.md) §5; [0036](../decisions/0036-openstreetmap-providers.md) §6; [0048](../decisions/0048-notation.md) §3; [0066](../decisions/0066-lexemes.md) §4.*

IDs that denote the same thing form an **identity cluster**. Each member of a cluster belongs to a **namespace**, meaning whoever minted the ID: the local instance, Wikidata or OpenAlex. The namespace is the provider, not the type code. A Wikidata item can share a cluster with an OpenAlex work (`OAW`) or an OpenAlex author (`OAA`). An entity source a tenant declares is a namespace in this sense too ([0078](../decisions/0078-entity-sources.md) §2).

**Aliases are not members.** An ID retired by a redirect is an alias of its target. This covers an upstream merge recorded in a mirror graph and a local merge. Aliases are resolved to their targets before clusters are built.

**A keyed type is a namespace of its own.** Its keys are minted by the thing itself, not by a provider, and a cluster holds at most one member from it, as from any other namespace. The cluster rules do not apply to the provider mapping of key-mapped providers ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §9, in [05](05-providers-and-ingest.md)): that mapping defines which Domain an upstream item is, so it is resolved by the adapter before any cluster is built.

**Keyed entities that are names never join a cluster.** Keywords (§3.7) and notations (§3.9) are phrases, not concepts. Treating a phrase and the concept it denotes as one thing would make the phrase canonical over the concept, since keyed members rank first (§4.4). So `same-as` with a keyword or notation on either side is rejected at write time, as a data-type mismatch between properties is (§4.6), and no tier-2 or tier-3 link may produce one. Ranking first is for keyed types that name a thing, as a Domain names a host.

**Clusters are of lexemes, never of their parts.** `same-as` between lexemes forms an identity cluster as between items; `same-as` between forms or senses, or between a part and a lexeme, is refused with `ts-cluster-part`. A non-canonical lexeme's forms and senses keep their IDs under the canonical lexeme's page and are not merged into its own (§5.1).

### 4.2 Clusters are strictly one-to-one

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §2.*

**A cluster holds at most one member from each namespace.** A set of links that would put two members from one namespace into a cluster is a conflict (§4.11). The links involved are held, not applied. For example:

- `WDQ1 = OAW1` and `OAW1 = WDQ2` would join two Wikidata items.
- Both links are held, so `WDQ1`, `OAW1` and `WDQ2` stay out of each other's clusters until the conflict is resolved.

Holding every conflicting link, not only the later one, makes the result independent of the order in which links arrive.

**Duplicates within one namespace are merges.** Two IDs from the same provider that denote one thing are a duplicate within that provider. That is expressed by a `redirect`, an explicit merge ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7). It is never inferred from links.

**Clusters stay small.** A cluster can hold no more members than there are namespaces the tenant reads: the local instance, each registry provider, each keyed type (§4.1) and each entity source (§2.1). Chains of links cannot grow, and a cluster can be recomputed from its links whenever one of them changes.

### 4.3 Where links come from

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §3.*

Links come from three tiers of source. A higher tier beats a lower one when they conflict. Conflicting links within one tier are all held (§4.2).

| Tier | Source | Example | Default |
|---|---|---|---|
| 1. Asserted | A local-graph `same-as` (§4.10) | An editor links `Q456` and `OAW123` | Always on |
| 2. Mirror-derived | A mirrored statement of a configured link property | Wikidata's P10283 (OpenAlex ID) on a Wikidata item; OpenAlex's `ids.wikidata`, on the entity types that carry it | On for configured properties |
| 3. Inferred | Two entities from different namespaces share a value of a configured identifier property | Both carry the same DOI | Off |

**A mirror-derived link lasts only as long as its statement.** It disappears when:

- the mirror drops the statement;
- the entity is tombstoned;
- the local graph deprecates or suppresses the statement.

A link value that points at an alias follows the redirect. If Wikidata says an item's OpenAlex ID is `W1` and OpenAlex has merged `W1` into `W2`, the link goes to `OAW2`. Mirror-derived links can join a Domain to a mirrored entity through a configured link property, as for any other cluster ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §8).

**Inferred links need a unique value.** A value that appears on more than one entity in a namespace produces no link. Values are compared by their normalized keys (§4.7). An inferred link is applied at once, for the identifier properties the **instance's** `reconcile` record names; nothing is queued for review, and the registry ships none enabled. A tenant's `reconcile` record may switch inference off for that tenant, and sets its provider order and link properties (§4.10), but cannot add inference properties, so every tenant of a farm infers the same links from a shared graph.

**On a farm,** tier-2 and tier-3 links come from shared graphs and are instance-wide. Tier-1 links and `different-from` blocks are each tenant's own. A provider tenant's `same-as` and `convert` records are tier-2 links for every other tenant ([0018](../decisions/0018-tenants.md) §5–6, in [08](08-tenants-and-instances.md)).

**A `different-from` block beats links from every tier.** Two IDs joined by a local `different-from` never share a cluster. Wikidata's P1889 ("different from") needs no special handling. It relates two Wikidata items, which can never share a cluster anyway.

### 4.4 The canonical ID

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §4; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §8.*

**The canonical ID of a cluster is its highest-ranked member.**

- A keyed member ranks first, and a local member next. The canonical order is: keyed type, then local, then the providers in the instance's order.
- The tenant's `reconcile` record sets the order of the providers after them; the default is provider-number order in `providers.toml` (Wikidata first). The `reconcile` record ([0015](../decisions/0015-record-format-and-partition-registry.md) §3, in [23](23-configuration-and-registry.md)) may set a different order per entity type, `order_by_type`; a cluster's members share a type, so the order is always defined.

So if a local item `Q77` about en.wikipedia.org existed before Domains did, `same-as` between `Q77` and `domain:en.wikipedia.org` makes the Domain canonical. `Q77` resolves to it and gets the redirect-form `owl:sameAs` in RDF. A cluster whose links would join two Domains is a conflict, and the links are held (§4.11).

**The canonical ID is chosen when the resolved view is projected.**

- Source graphs keep IDs exactly as they were asserted.
- The resolved view ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3, in [02](02-graphs-rdf-and-query.md)) rewrites subjects and entity values to the canonical ID.
- Each non-canonical member is emitted in Wikibase's redirect form, `<member IRI> owl:sameAs <canonical IRI>`.
- The API resolves a non-canonical member to the canonical entity, as Wikibase resolves a redirect.
- The per-graph views ([0003](../decisions/0003-statement-ui.md) §6) still show each member's own data.

**The canonical ID changes when membership changes.** Examples:

- A Wikidata item is created for an OpenAlex work, and the new item becomes canonical.
- A local member is minted (§4.5) and becomes canonical.
- The canonical member is tombstoned under `cascade` or `orphan`. It leaves the cluster, and the next member becomes canonical. A retained member stays in the cluster.
- A local member is deleted. It leaves the cluster as a member tombstoned under `orphan` does. `same-as` and `different-from` records naming it are kept but not applied, and apply again if it is undeleted ([0023](../decisions/0023-moderation.md) §4, in [09](09-security-and-moderation.md)).

**The stable handle is the document node.** External consumers are told to cite the document node of any member, `{base}/wiki/Special:EntityData/{id}` ([0015](../decisions/0015-record-format-and-partition-registry.md) §6), which exists for every member and never moves. The resolved view carries `schema:about` from each document node to the current canonical concept IRI, and the update stream ([0032](../decisions/0032-sparql-update-stream.md), in [02](02-graphs-rdf-and-query.md)) keeps that triple current.

### 4.5 Conversion joins a cluster

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §5.*

`convert` mints a local entity and links it to the foreign entity with `same-as`. The local member is canonical, so the behavior of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §6 (in [05](05-providers-and-ingest.md)) follows from the cluster rules:

- `WDQ123` resolves to `Q456`;
- mirrored statements under `WDQ123` attach to `Q456`;
- the redirect-form `owl:sameAs` is emitted.

**A local alias that conflicts with an upstream redirect.** Suppose `WDQ123` has been converted to `Q456`, and Wikidata then merges `Q123` into `Q789`.

- `WDQ123` becomes an alias of `WDQ789`, so `WDQ789` takes its place as the cluster's Wikidata member.
- If `WDQ789` already belongs to another cluster, the merge would join the two clusters. One-to-one applies.
- For example, if `WDQ789` is already linked to `Q999`, the joined cluster would have two local members. The links are held as a conflict.
- The editor resolves it with a local `redirect` from `Q456` to `Q999`, or the reverse.

`convert` does not apply to a keyed entity, which is already canonical (§3.3).

### 4.6 Properties

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §6.*

**Wikidata properties are used as they are,** as mirrored `WDP` entities.

**OpenAlex properties are mapped by the adapter.** OpenAlex has no property entities. Its fields have names, not identifiers, and the set of fields is small and changes little. The OpenAlex adapter therefore maps each field:

- onto a Wikidata property, where one fits;
- otherwise onto a local property.

There is no OpenAlex property namespace or type code.

- **Enumerated values** such as work type are mapped in the same way, onto Wikidata items or local items.
- **Local targets are named by role.** Local property and item IDs differ between instances. The adapter therefore refers to its local targets by role, as [0003](../decisions/0003-statement-ui.md) §7 does. Each instance binds the roles to its own entities, and the adapter can create them on first import.
- **The mapping is versioned with the adapter** ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3).

**Any graph may use any property.** The OpenAlex mirror graph holds statements whose predicates are `WDP` or local properties. A graph records who asserts a triple, not whose vocabulary the triple uses. Mapped properties are affected by changes as follows:

- **Local properties** that an adapter maps onto are protected from deletion.
- **Wikidata properties** can change upstream. If one that an adapter maps onto is deleted upstream, or its data type changes, that mapping stops and the change is reported. The mapping then has to be revised.

**Properties form clusters as items do.** A local property may duplicate a Wikidata property, for example because it was created before Wikidata had one. A local `equivalent-property` (§4.10) links the two. Property clusters follow the same rules as item clusters:

- They are one-to-one.
- The local property is canonical.
- `different-from` blocks a link.

Property links come only from local assertions. Wikidata's P1628 ("equivalent property") points at external vocabularies, not at entities this instance holds, so it is ordinary data.

- **Data types must match.** A link between properties with different data types is rejected when it is written.
- **The resolved view uses the canonical property's predicates** for every statement in the cluster.
- **RDF output:** property entity IRIs get the redirect-form `owl:sameAs`, as items do. OWL equivalence holds between predicates, not between entity IRIs. So `owl:equivalentProperty` is emitted for each pair of predicates in the same family: `wdt:`, `p:`, `ps:`, `psv:`, `pq:`, `pqv:`, `pr:`, `prv:`, and the normalized forms.

### 4.7 Values

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §7.*

The resolved view compares values by a canonical form:

- **Entity values** compare by the canonical ID of their cluster.
- **Literal values** compare by a **normalized key**, derived per data type. A property can override its data type's normalizer.

| Data type | Normalized key |
|---|---|
| `external-id` | Per property. For example, a DOI is lowercased, and an ORCID loses its URL prefix. The default is the exact value. |
| `string` | Unicode NFC |
| `monolingualtext` | Unicode NFC, together with the language code |
| `time` | The value, precision and calendar model. Values with different precisions are different values. |
| `quantity` | The amount, unit and bounds |
| `url` | The normalized URL that sitelinks also use ([0026](../decisions/0026-sitelinks.md) §1) |
| Everything else | The exact value |

Stored values are never changed. A fused statement (§4.8) shows the value asserted by the highest-ranked graph.

**A coarser time does not fuse with a finer one.** "2019" and "2019-03-04" stay two values. The statement UI marks the coarser one as consistent with the finer one in the same group, and the resolved view keeps both, so it never claims a precision no source asserted.

### 4.8 Fusing equivalent statements

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §8.*

This extends the statement hash in [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.5. A statement's **key** is a hash of two things, both in canonical form (§4.4, §4.6, §4.7):

- the main snak;
- the qualifiers, in any order.

In the resolved view, statements with the same key fuse into one statement. This applies both across graphs and within a graph. Source graphs are not changed.

**References** are combined, with duplicates removed by the reference hash. Reference snaks are put into canonical form before hashing.

**Rank:**

- The local graph wins, as [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3 says.
- Otherwise the rank comes from the highest-ranked mirror graph, in the same provider order as §4.4.

**Statement ID:**

- The fused statement takes its ID and IRI from the highest-ranked graph that asserts it.
- The provenance response ([0003](../decisions/0003-statement-ui.md) §6) lists every member statement and its graph.

**Overrides:**

- A rank override on any member statement applies to the fused statement.
- A suppression removes only one member's contribution. To remove the fused statement, every member is suppressed, which is what the UI does.

**Non-equivalent values** are kept side by side, as 0002 §3 says.

### 4.9 Statement IDs follow the canonical ID

*Sources: [0018](../decisions/0018-tenants.md) §7.*

**Statement IDs in the log never change.** What clients see is derived: **the resolved view rewrites the entity-ID part of a statement GUID to the entity's canonical ID and keeps the UUID.** This is the same rewrite §4.4 applies to subjects and entity values.

- example.wiki converts `WDQ42` to `Q9` (§4.5). The mirror keeps writing `WDQ42$abc`; example.wiki shows `Q9$abc`.
- Librarybase does the same; example.wiki, reading Librarybase, shows `LBQ9$abc`.
- An unconverted cluster {`WDQ42`, `OAW123`} shows the OpenAlex statements as `WDQ42$oa-uuid`.

The UUID alone identifies a statement, so overrides ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7), the provenance response (§4.8, which lists member statements in their source form) and API writes all resolve through it. Every GUID a client sees has its entity's ID as its prefix, which is Wikibase's invariant. Re-minting statements on conversion was rejected: it would copy mirrored statements into the local graph and recreate the stale-copy problem 0002 §5 rejected for rescue.

### 4.10 Operations and configuration

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §9; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §10; [0066](../decisions/0066-lexemes.md) §4.*

| Operation | Graph | Meaning |
|---|---|---|
| `same-as` | Local | Links two items from different namespaces (§4.3, tier 1). |
| `different-from` | Local | Blocks two IDs from ever sharing a cluster (§4.3). |
| `equivalent-property` | Local | Links two properties from different namespaces (§4.6). |

`remove` retracts any of these. `same-as` and `different-from` apply to a Domain with an entity of another namespace, never between two Domains (§3.3), and `same-as` applies between lexemes but not their parts (§5.1).

`redirect` in the local graph records a merge within one namespace. **For properties only, a local `redirect` may cross namespaces:** `{"op":"redirect","from":"P12","to":"WDP585"}` retires `P12` and makes `WDP585` the canonical predicate. The resolved view is rewritten as §4.6 rewrites a linked pair, and existing local statements keep `P12` in the log and read as `WDP585`. It needs `property-create` and a matching data type. An item never redirects across namespaces, and a local item stays canonical.

```
{"op":"same-as","ids":["Q456","OAW123"]}
{"op":"different-from","ids":["WDQ1","OAW7"]}
{"op":"equivalent-property","ids":["P12","WDP585"]}
```

**Configuration is recorded in the log.** The following settings are the `reconcile` record's; the first, second and fourth are the tenant's, the third the instance's (§4.3):

- the provider order;
- the link properties for each provider;
- the identifier properties used for inference;
- normalizer overrides.

A change to any of them is appended to the log as a record, so the resolved view stays rebuildable from the log ([0000](../decisions/0000-init.md) §1). Keyed-type registry entries are recorded the same way (§3.1). The record kinds are in [23](23-configuration-and-registry.md).

### 4.11 Conflicts

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §10.*

A **conflict** is a set of links that is held instead of applied. Conflicts arise from:

- a one-to-one violation, whether from links or from an upstream redirect that joins two clusters;
- a link blocked by `different-from`.

Conflicts are listed for review, in the same way as the maintenance tooling for local corrections ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7). An editor resolves a conflict in one of these ways:

- with a local `same-as`, which outranks mirror-derived links;
- with `different-from`;
- with a `redirect` that merges a duplicate;
- by deprecating the mirrored statement a link came from.

**The forms.** The review list is `Special:IdentityConflicts`, with these resolutions as row actions. `Special:LinkEntities` is the form for `same-as`, `different-from` and `equivalent-property`; `Special:MergeItems` writes a merging `redirect` within one namespace and refuses a merge across namespaces ([0047](../decisions/0047-special-pages.md) §5–6, in [21](21-special-pages.md)).

## 5. Other entity types and derived IDs

*Sources: [0066](../decisions/0066-lexemes.md) §1, §2, §3, §4, §9; [0017](../decisions/0017-entity-id-grammar.md) §1; [0044](../decisions/0044-tenant-relative-ids.md) §1; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6; [0043](../decisions/0043-lua-modules.md) §8.*

### 5.1 Lexemes

*Sources: [0066](../decisions/0066-lexemes.md) §1; [0066](../decisions/0066-lexemes.md) §2; [0066](../decisions/0066-lexemes.md) §3; [0066](../decisions/0066-lexemes.md) §4; [0066](../decisions/0066-lexemes.md) §9.*

**`lexeme` is an entity type**, local `L1`, foreign `WDL1`, hosted in namespace 146 under the `wikibase-lexeme` model. Its data model is WikibaseLexeme's, in its JSON serialization, unchanged:

| Part | Fields | Statements |
|---|---|---|
| **Lexeme** | `lemmas` (multilingual text, one per language code or variant), `language` (an item), `lexicalCategory` (an item) | Yes |
| **Form** `L1-F1` | `representations` (multilingual text), `grammaticalFeatures` (items) | Yes |
| **Sense** `L1-S1` | `glosses` (multilingual text) | Yes |

**Forms and senses are parts of the lexeme, not entities.** One page, one record stream, one revision history; a form's or sense's statements are in the lexeme's resolved JSON under `forms[].claims` and `senses[].claims`, with statement IDs `L1-F1$<uuid>`. They have no page, no terms table rows of their own beyond 0066 §5 ([03](03-storage-caches-and-search.md)), no cluster membership, and no ACL target of their own: the lexeme encloses them ([0023](../decisions/0023-moderation.md) §2). A form or sense **is an entity value**: the data types `wikibase-lexeme`, `wikibase-form` and `wikibase-sense` are added to `scatter-wikibase-model`, with `{"entity-type": "form", "id": "L1-F1"}` as Wikibase writes them, usable on items and lexemes alike. Lexeme **terms** are lemmas, not labels: a lexeme has no `labels`, and the places that show "the label" show the lemmas joined by `/` as Wikidata does.

**The `Lexeme` namespace.**

| Number | Canonical name | Kind | Allowed models | Default model |
|---|---|---|---|---|
| 146 | `Lexeme` | `pages` | `wikibase-lexeme` | `wikibase-lexeme` |
| 147 | `Lexeme talk` | `pages` | `triplespace-talk` | `triplespace-talk` |

WikibaseLexeme's number, implemented with its meaning. Titles are lexeme IDs (`Lexeme:L1`, `Lexeme:WDL1`); a form or sense ID in a title (`Lexeme:L1-F1`) resolves to the lexeme's page with the part's anchor, as `Special:EntityPage/L1-F1` does on Wikidata.

**The `wikibase-lexeme` content model.**

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `wikibase-lexeme` | WikibaseLexeme | entity (`lexeme`) | main | `application/json` | No | 146 |

An entity model; edits go through the modules of 0066 §8 ([18](18-api.md)) and the UI of 0066 §7 ([19](19-site-ui.md)). Both rows belong to the tables of [10](10-pages-and-content-models.md).

**IDs and clusters.** The ID forms, with the `-F` and `-S` suffix, are in §2.1; the cluster rule, that clusters are of lexemes and never of their parts, is in §4.1. The canonical lexeme's parts are the cluster's parts; a non-canonical member's forms and senses are **not merged** into the canonical lexeme's, so a reference to `WDL1-F1` resolves to that form under the canonical lexeme's page, never to a renumbered part. **Merging lexemes** (`Special:MergeLexemes`, 0066 §7, in [21](21-special-pages.md)) when both are local is a `redirect`, as `Special:MergeItems` writes for items (§4.11): the source's forms and senses are moved to the target with new part numbers, each moved part leaves a redirect from its old ID, and the source lexeme becomes a redirect; no cluster is formed within the local namespace (§4.2). A local lexeme and a foreign one are joined by `same-as` like any other pair.

**Mirroring, constraints, Lua and tables.**

- **The Wikidata adapter** parses `"type": "lexeme"` documents from the same JSON dumps and EventStreams it already reads, writing `put` records for `WDL` as for any entity ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4, in [05](05-providers-and-ingest.md)); the RevisionChest reader covers them. A Wikidata test import includes lexemes.
- **Constraints** ([0031](../decisions/0031-property-constraints.md), in [06](06-statements-and-properties.md)): `lexeme requires language`, `lexeme requires lexical category`, `lexeme value requires lexical category` and the form/sense scoped constraint types evaluate over the lexeme's resolved JSON; the `scope` constraint's `as qualifier`/`as reference` parameters gain WikibaseLexeme's `as main value on a form/sense` meaning.
- **Lua** ([11](11-rendering-templates-and-modules.md)): `mw.wikibase.lexeme` and the `mw.wikibase.lexeme.entity.*` classes as the extension documents them; usage tracking per lexeme ID.
- **Tables and scopes** ([15](15-structured-pages.md)): `L` and part IDs as rows; term columns `lemma:{lang}`, `representation:{lang}`, `gloss:{lang}`; the `language` and `lexicalCategory` fields as columns.
- **Adoption** ([0035](../decisions/0035-adopting-a-wikibase.md) §2, in [05](05-providers-and-ingest.md)): a Wikibase with lexemes adopts its `L` entities under their IDs.

### 5.2 MediaInfo and the `M` ID

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §1; [0044](../decisions/0044-tenant-relative-ids.md) §1.*

`M` and digits is the derived ID of §2.1: `M1234` names the statements of File page 1234 ([0041](../decisions/0041-content-models.md) §7), and is never in `view.entity`, never in a cluster and never the value of an entity data type. Its tenant-relative input form is `MMM1234` (§2.3), and a tenant's Lua may map the `M` letter to a provider (§5.4). Commons MediaInfo mirrored from Wikidata is the foreign type `WDM` (§1). The `mediainfo` slot, captions and Commons as a source are in [12](12-files-and-media.md).

### 5.3 Provider-ranged page IDs

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 (shared with [18](18-api.md)).*

**A foreign page has a derived page ID.** Its `pageid` is the **provider-ranged** form of the repository's own page ID, `provider_number << 40 | upstream page ID`, computed as mirror records' revision IDs are ([0015](../decisions/0015-record-format-and-partition-registry.md) §2, [0013](../decisions/0013-postgres-storage.md) §6, in [03](03-storage-caches-and-search.md)). It is derived, not minted, as a File page's `M` ID is (§5.2): nothing allocates it, it is the same on every tenant that reads the repository, and it never collides with a local page ID, which comes from the tenant's sequence below 2^40. Its `lastrevid` is the ranged form of the upstream revision ID the tenant currently holds. In `mirror` mode the same numbers are what the `pages/{repo}` records carry in header fields 9 and 7 ([0053](../decisions/0053-mirrored-pages.md) §5, in [13](13-mirrored-pages.md)).

In the API, `titles=`, `pageids=` and `redirects` resolve through the stack and through foreign redirects, and a ranged `pageid` resolves to the repository page it names; `GET /page/{id}` accepts a ranged ID. What else the API reports for a foreign primary is in [18](18-api.md).

**Sitelinks target local pages only.** A sitelink to the tenant's own host is stored by page ID ([0038](../decisions/0038-page-metadata-and-categories.md) §6), and a foreign page's ID changes when it is forked, so a title whose primary is foreign is refused as a sitelink target with `ts-sitelink-foreign`. An item that wants an article here gets a fork ([13](13-mirrored-pages.md)).

### 5.4 The Lua ID space, per tenant

*Sources: [0043](../decisions/0043-lua-modules.md) §8.*

**Each tenant says what a bare one-letter ID means in its Lua,** with the `site` setting **`lua.ids`**: a map from a type letter to `local` or to a provider code. The default is empty: every letter means `local`, so a bare `Q5` means the local Q5, as everywhere else in Triplespace. A mapped provider must mint that type letter (`providers.toml`) and be one the tenant reads (`providers`, [0018](../decisions/0018-tenants.md) §3); a record that names any other provider is refused.

**A Wikipedia mirror maps the Wikidata letters:**

```toml
# site setting lua.ids
Q = "WD"
P = "WD"
L = "WD"
M = "WD"
```

**`#property` and `#statements` read the same setting.** They are Wikibase Client surfaces like `mw.wikibase`, and a template that mixes `{{#property:P31}}` with a module call must mean one property by both.

**Translation is at the boundary,** both ways, for every ID that crosses it:

| Written in Lua (or a parser-function argument) | Means |
|---|---|
| A bare ID, `Q5`, whose letter is mapped to provider `WD` | `WDQ5` |
| A bare ID whose letter is unmapped | The local `Q5` |
| The tenant-relative form, `QQQ5` (§2.3) | The local `Q5`, **always**, whatever the mapping |
| A full foreign ID, `LBQ6`, or a keyed ID, `domain:example.org` | Itself |

| An entity reaching Lua | Is written |
|---|---|
| An entity of the provider its letter is mapped to (`WDQ5` under `Q = "WD"`) | Bare: `Q5` |
| A local entity whose letter is mapped elsewhere | Tenant-relative: `QQQ5` |
| A local entity whose letter is unmapped | Bare: `Q5` |
| Any other foreign or keyed entity | Its full ID: `LBQ6`, `domain:example.org` |

The output rule applies everywhere an ID appears in what the host returns: an entity's `id`; statement ID prefixes; the property keys of `claims`, `qualifiers` and reference `snaks`; `property` fields; `qualifiers-order` and `snaks-order`; entity values' `id`, with `numeric-id` its digits; and the results of `getEntityIdForCurrentPage`, `getEntityIdForTitle`, `resolvePropertyId`, `getReferencedEntityId`, `orderProperties` and `getPropertyOrder`.

**The mapping is a bijection** between the strings Lua sees and canonical IDs, so a module that reads an ID and passes it back always reaches the same entity. Nothing translated is ever stored: manifests, usage rows and caches hold canonical IDs.

`resolvePropertyId` resolves a label among the properties of the provider the `P` letter names, or the local ones when `P` is unmapped.

**On a Wikipedia mirror**, Module:Wd, Module:WikidataIB and infobox modules work unchanged against mirrored Wikidata, and the tenant's own items stay reachable as `QQQ…`. The Lua environment itself is in [11](11-rendering-templates-and-modules.md).

## 6. Where IDs are stored and shown

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §4; [0044](../decisions/0044-tenant-relative-ids.md) §1; [0043](../decisions/0043-lua-modules.md) §8.*

The ID form of §2.1 is the one that appears everywhere an entity ID does: `view.entity.id` and every column that references it ([0013](../decisions/0013-postgres-storage.md) §5, in [03](03-storage-caches-and-search.md)), the `id` field of a search document ([0014](../decisions/0014-caches-and-search.md) §7, whose `key` field keeps the bare key), the `target_id` of an ACL ([0016](../decisions/0016-permissions-and-access-control.md) §4, in [09](09-security-and-moderation.md)), the ID chip in the UI ([0010](../decisions/0010-site-ui.md) §2, in [19](19-site-ui.md)), and canonical JSON. The UI's identity line names the kind of thing in words, "Domain" or "Keyword", and the chip shows the ID as it is; for a notation the identity line shows the scheme's `label` (§3.8).

The input forms are never stored: the tenant-relative form (§2.3) and a provider slug before a colon (§2.1) are canonicalized wherever an ID is accepted, and the one place a non-canonical form is written out is Lua under a mapped letter (§5.4), where nothing translated is stored either. In the log, a keyed entity appears only under its surrogate (§3.4).
