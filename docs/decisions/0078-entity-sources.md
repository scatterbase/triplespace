# 0078. Entity sources: foreign graphs a tenant declares

- **Status:** Proposed
- **Date:** 2026-10-08
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0015](0015-record-format-and-partition-registry.md), [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0029](0029-resolver-namespaces.md), [0035](0035-adopting-a-wikibase.md), [0070](0070-shallow-entity-mirroring.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0007](0007-actor-identity.md), [0009](0009-keyed-entity-types-and-domain.md), [0022](0022-federation.md), [0024](0024-subsidiary-accounts.md), [0026](0026-sitelinks.md), [0043](0043-lua-modules.md), [0044](0044-tenant-relative-ids.md), [0048](0048-notation.md), [0052](0052-page-repositories-and-title-inheritance.md), [0056](0056-security-model.md), [0065](0065-mediainfo-captions-and-commons.md), [0071](0071-derived-statements-from-mirrored-pages.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

A foreign graph reaches a tenant today only through the provider registry ([0015](0015-record-format-and-partition-registry.md) §5): a two-letter code, a slug and a provider number in `providers.toml`, allocated by a commit to the Triplespace repository and carried by every instance. That is right for Wikidata, OpenAlex or MusicBrainz, which many tenants read. It is wrong for a graph that one tenant needs. Librarybase including Miraheze Communities' Wikibase (`https://communities.miraheze.org/entity/Q1`) would need a registry commit, a code from a global space, and a public record that Librarybase uses it, before the first value could be written.

Wikibase already has the concept. A federated install declares **entity sources**, each with a concept base URI and an interwiki prefix, and writes their IDs as `wikidata:Q42`. [0017](0017-entity-id-grammar.md) §1 chose the colon for keyed IDs partly because it is this form. [0035](0035-adopting-a-wikibase.md) §3 rewrites an adopted wiki's entity-source prefixes to provider form, `WDQ42`, but only where the source is a registry provider; a prefix naming any other source has no rule.

The first design discussed, on 2026-10-07, reserved a block of provider codes, `YA` through `YZ`, whose meaning each tenant would set. It is under Alternatives. James then proposed the design this ADR records, and asked for a check against the ADRs. The check found that 0008 defines no range for a tenant's own namespaces, which James had assumed; §5 defines one.

### Direction

James's direction, from the design discussion of 2026-10-07 and 2026-10-08:

- **"There will be times when a tenant wants to incorporate data from a foreign graph, but the foreign graph in question isn't of broad enough usefulness to become part of the global graph namespace."**
- **"Tenants can have a very large number of custom wiki namespaces. Say, for example, Librarybase wanted to include Miraheze Communities as part of its graph, and I did not want to add Miraheze Communities to the registry. I can allocate a custom namespace "MHC" that resolves to the entity, for example MHC:Q1 for <https://communities.miraheze.org/entity/Q1>. If at some point an identifier needs to be assigned to it so that it can be fully incorporated into the local graph, a local entity ID can be minted, and the "keyed" identifier becomes more like a resolver."**
- **"The authority of what [the prefix] means comes from that tenant, but outside of the immediate context of that tenant, it should be converted into the real meaning of the triple anyway."** The mechanism is the one that turns `WDQ42` into `http://www.wikidata.org/entity/Q42`.
- **"It's good to have deterministic behavior around this"**, of possible clashes between `MHC:Q1`, `Item:MHQ1` and `Item:MHC1`.
- **The namespace comes from the tenant's own range, not the Triplespace blocks.**
- Agreed in the same discussion: one name moves through three states (a pointer, a mirror, then a member of a local item's cluster) without a third mechanism; a tenant's name wins inside the tenant; identity across tenants is the IRI; a name, once it has data, is never pointed at another graph.

## Decision

### 1. An entity source is a provider a tenant declares (extends 0015 §3)

**An entity source is a graph of minted entities that one tenant reads, declared in that tenant's configuration instead of the registry.** It is a `config` record of kind **`entity-source`**, keyed `entity-source:{name}`, in the tenant `config` ([0015](0015-record-format-and-partition-registry.md) §3), written with `ts-config`. Its fields are a provider registry entry's ([0002](0002-source-graphs-and-mass-ingest.md) §4, `providers.toml`), cut to what a source of entities needs:

| Field | Meaning |
|---|---|
| `name` | The prefix of its IDs (§2): `[a-z][a-z0-9-]*`, at most 32 characters, the grammar of notation scheme names ([0048](0048-notation.md) §2). Example: `mhc` |
| `label` | The name shown on source chips and origin lines: "Miraheze Communities" |
| `types` | One entry per entity type the source mints, with the fields of a `[[provider.type]]`: `upstream_prefix` (`Q`), `entity_type` (`item`), `id_grammar` ([0017](0017-entity-id-grammar.md) §2; default `digits`), and `iri`, the concept IRI template over `{upstream_id}` (§7) |
| `namespace` | The number of the source's namespace (§5), with its canonical name and aliases |
| `api`, `entity_data` | Optional: the source's Action API endpoint and `Special:EntityData` base, as [0065](0065-mediainfo-captions-and-commons.md) §5's per-type source fields. Without `api`, the source can be named but not mirrored |
| `revision_ids` | Whether the source publishes revision IDs, as in `providers.toml` |
| `events` | Optional: an EventStreams endpoint and filter, as a page repository's ([0052](0052-page-repositories-and-title-inheritance.md) §1) |
| `role` | Optional: a role ([0003](0003-statement-ui.md) §7) whose property holds the source's upstream ID as an `external-id`, for the lookup of §5 |
| `licence` | The licence of the source's data, as an SPDX identifier; required once anything is mirrored (§4) |
| `number` | Assigned by the server on the first record (§4); not writable |
| `promoted_to` | Empty, or the registry provider code that replaced the source (§9) |

**What it is for.** A graph that one tenant, or a few, need. A graph that many tenants need belongs in the registry, and §9 moves a source there without losing anything.

### 2. Source IDs (amends 0017 §1)

**A source's entities have IDs of the form `{name}:{upstream ID}`:** `mhc:Q1`, `mhc:P31`. The rest is the source's own ID, verbatim, so the ID pastes into the source unchanged, as an MBID does ([0017](0017-entity-id-grammar.md) §6).

**A source ID is a foreign ID in the colon shape, not a keyed ID.** A keyed ID names a thing whose key is minted by the thing itself; a source's entity was minted by a provider, the source ([0009](0009-keyed-entity-types-and-domain.md) §1). Everything [0002](0002-source-graphs-and-mass-ingest.md) says of a foreign entity holds: the prefix records identity origin, the entity may have triples in the source's mirror partition (§4) and in the tenant's `local` graph, and the source is a namespace in [0004](0004-identity-clusters-and-equivalence.md) §1's sense, so a cluster holds at most one of its entities. A source entity therefore needs no surrogate ([0009](0009-keyed-entity-types-and-domain.md) §7): its log header key is its ID, as `WDQ42`'s is.

- **Parsing.** A colon form is told apart by the kind of its prefix in the tenant's table of names (§3). For a source, the rest must be the `upstream_prefix` of one declared type followed by a string in that type's grammar, which also gives the entity type. A source that declares `lexeme` takes the part suffix of [0017](0017-entity-id-grammar.md) §1 (`mhc:L3-F1`).
- **Canonical form.** The name in lowercase, and the rest by the type's grammar with its upstream prefix in uppercase: input `MHC:q1` is `mhc:Q1`. Input is case-insensitive, as for every ID ([0017](0017-entity-id-grammar.md) §2).
- **Statement IDs and values** take the ID as any entity's do: `mhc:Q1$C7C2A847-…`, and `{"entity-type": "item", "id": "mhc:Q1"}`, which is the value form a federated Wikibase writes.
- **Pages.** A source entity's page is in the namespace of its entity type, `Item:mhc:Q1` or `Property:mhc:P31`, since entity namespaces are keyed by type, not by provider ([0008](0008-namespaces-and-document-pages.md) §1). The source's own namespace (§5) is how a reader reaches it by a short title.
- **Relative.** `mhc:Q1` means the source's entity on the tenant where it is read, as `Q5` means the tenant's own ([0044](0044-tenant-relative-ids.md) §1). It reaches another tenant only through the rewriting of §8.

### 3. One table of names per tenant (extends 0029 §2 and §6)

**Every colon prefix on a tenant is looked up in one table.** [0029](0029-resolver-namespaces.md) §2 made keyed-type, resolver and notation scheme names one namespace in the registry. Two kinds join it:

- **Registry provider slugs.** `wikidata:Q42` is an **input form** of `WDQ42` everywhere, canonicalized as `QQQ5` is ([0044](0044-tenant-relative-ids.md) §1): never stored, never output. This is what [0035](0035-adopting-a-wikibase.md) §3 already does to an adopted wiki's federated IDs, extended to all input. A slug with no entity types (a page provider, [0052](0052-page-repositories-and-title-inheritance.md) §1) is reserved but forms no IDs.
- **The tenant's entity sources**, on that tenant only. Two tenants may each have an `mhc` that names different graphs.

**The rules are fixed, and none of them guesses:**

1. **A source may not take a name the registry has.** A declaration whose name is a keyed type, resolver, notation scheme or provider slug in the registry the instance runs is refused with `ts-source-name-taken`.
2. **A tenant's source shadows a registry name added later.** If a registry update adds a name a tenant already uses for a source, the source keeps it on that tenant. The registry's thing stays reachable there by its other forms: its IRI, its provider code, or, for a keyed type, its namespace title. A reference from elsewhere that uses the shadowed name is matched by IRI (§8) and never read as the source. `triplespace-cli registry check` lists every shadowed name after an update.
3. **Colon forms never collide with minted forms.** `mhc:Q1`, the provider-form ID `MHC1` (a provider `MH`, type `C`) and `Item:MHQ1` are three different strings, parsed by three rules ([0017](0017-entity-id-grammar.md) §1). Whether two of them name the same entity is a question of identity, which §9 answers: on one tenant, they never both do.
4. **The tenant-relative form is untouched.** `QQQ5` has no colon.

`GET /resolve` ([0029](0029-resolver-namespaces.md) §6) tries source IDs with the other entity IDs, first; the table says which kind a prefix is, so the order among colon forms decides nothing.

### 4. Numbers, the mirror partition and fetching (extends 0015 §2 and §5, 0018 §10, 0070 §2.1, §3 and §10)

**Provider numbers from 2^22 to 2^23 − 1 are reserved for entity sources.** The server assigns the lowest one not yet used on the tenant when the source's first record is written, and records it in the entry. A number is unique on its tenant and is never reused there, even after the source is retired. Uniqueness per tenant is enough: revision and page IDs are per-tenant sequences ([0015](0015-record-format-and-partition-registry.md) §2), and no other tenant reads a source's records (§8). So `revid = number << 40 | n` and the derived page IDs work unchanged, and a move carries the number in the entry. The registry keeps the 4,194,303 numbers below the range.

**A source's records live in a tenant partition, `source/{name}`.** It is a source graph of tenant scope, with the policies of `mirror/{provider}`: history `latest` (a tenant may set `full`), integrity `hashed`, export public unless the tenant is private ([0056](0056-security-model.md) §3), payloads `scatter:v0/changeset` and `scatter:v0/erase`. Only the source's fetch job writes to it, and the job attests its records, as an extraction job attests a derived graph's ([0071](0071-derived-statements-from-mirrored-pages.md) §1, §3). Its records are mirror `put`, `redirect` and `tombstone` ([0002](0002-source-graphs-and-mass-ingest.md) §8.2). A tenant move carries it with the tenant's other partitions ([0018](0018-tenants.md) §10).

**Shallow mirroring applies as written, with the source's name in place of a provider code** ([0070](0070-shallow-entity-mirroring.md) §2–7):

- `entities.mirror = { mhc = { item = "linked", property = "linked" } }`. The default is `linked` for a source with `api` and `off` for one without. `off` is the pointer state of the Direction: the source's IDs are written and shown, from the label cache where there is an `api` and as bare IDs otherwise, and nothing is fetched.
- `entities.closure` takes the source's name too. Its default for a source is empty, since nothing is known about which of its properties form hierarchies.
- **The fetch job is the tenant's, one per source**, not an instance job: it drains `ops.entity_fetch` rows for `(tenant, source)` through the source's `api` in the `upstream` rate class ([0024](0024-subsidiary-accounts.md) §5), follows `events` where the source declares them, and otherwise relies on the daily sweep.
- The label cache keys a source entity's labels by tenant ([0018](0018-tenants.md) §6, caching).

**Views.** A source entity has tenant rows only. Nothing about it is computed from shared graphs, so [0018](0018-tenants.md) §6 never gives it a shared row, and it is indexed in the tenant's search index, never in a provider index.

**No issuer.** Shallow mirroring keeps no upstream history ([0070](0070-shallow-entity-mirroring.md) §1), so no upstream actor is recorded and a source needs no issuer. Mirroring a source's history is Q1.

### 5. The source's namespace (extends 0008 §2 and §3)

**A tenant's own namespaces take numbers from 3000 upward,** the numbers mediawiki.org's Extension default namespaces page leaves to system administrators, or from 100–199, MediaWiki's block for site-specific namespaces, except the numbers `namespaces.toml` lists (120–125, 146 and 147 among them). An adopted wiki ([0035](0035-adopting-a-wikibase.md)) also keeps the numbers it already gave its namespaces. A tenant `namespace` record with a number outside these ranges that `namespaces.toml` does not list is refused with `ts-namespace-number`. This is rule 4 of [0008](0008-namespaces-and-document-pages.md) §2; the Triplespace blocks of rule 3 stay the registry's.

**Each source has one namespace there, of kind `resolver`,** with its talk number `reserved`, as a resolver's is ([0029](0029-resolver-namespaces.md) §2). The tenant chooses its canonical name, `MHC` or `Miraheze Communities`. **The source's name is always one of its aliases**, so `[[mhc:Q1]]` and `[[MHC:Q1]]` are both titles in it. A source whose namespace name or alias equals another namespace's name or alias on the tenant, or an interwiki prefix from the tenant's site aliases ([0026](0026-sitelinks.md) §2), is refused with `ts-namespace-name-taken`. MediaWiki's precedence of namespace over interwiki therefore never decides anything. The title normalizer is the source's ID normalizer of §2.

**Viewing `MHC:Q1`, or resolving `mhc:Q1`, does the following:**

1. **The entity is known to the tenant.** If `mhc:Q1` has a row in the tenant's resolved view (it is mirrored, the tenant asserts something about it, or it is in a cluster), the response is a redirect (HTTP 303) to its canonical page. That is `Item:mhc:Q1`, or `Item:Q500` once the entity is in Q500's cluster (§6), by [0008](0008-namespaces-and-document-pages.md) §3.
2. **An item carries its ID.** Otherwise, if the source names a `role`, the upstream ID is looked up as that property's value exactly as [0029](0029-resolver-namespaces.md) §3 looks up a DOI, with the same responses: one entity redirects, several show a disambiguation page.
3. **Nothing is known.** Otherwise a page shows the ID, the external link (the concept IRI, or the source's entity page where the source has `entity_data`), and for editors **Create an item for this** (§6). If the source has `api` and its mirror setting is not `off`, the fetch is enqueued, and the page says so.

`MHC:Q1` is therefore the citable address of "the source's Q1, whatever it ends up as", as a resolver permalink is ([0029](0029-resolver-namespaces.md) §4). A wiki link to it renders with the entity's label from the view or the label cache.

### 6. Minting a local item (uses 0004 §3 and §4)

**Create an item for this writes one change set:** a `create` of a local item, `Q500`, with the source entity's label where one is known; a tier-1 `same-as` between `Q500` and `mhc:Q1` ([0004](0004-identity-clusters-and-equivalence.md) §3); and, where the source names a `role`, a statement with that property and the upstream ID. A local member ranks above every provider's ([0004](0004-identity-clusters-and-equivalence.md) §4), so `Q500` is canonical unless the cluster has a keyed member. Among providers, a source that the tenant's `reconcile` order does not name ranks after every registry provider.

After that, `mhc:Q1` is a cluster member. Its title and every reference to it resolve to `Q500`. In the mirror states its mirrored statements keep contributing to Q500's resolved view, as a Wikidata item's do; in the pointer state it contributes only its name. This is what the Direction calls the identifier becoming "more like a resolver": a member of a cluster, which the identity machinery already has, and not a new mechanism. Removing the `same-as` undoes it.

### 7. IRIs and RDF (uses 0015 §6)

**A source entity's concept IRI is its type's `iri` template applied to the upstream ID**, `https://communities.miraheze.org/entity/Q1`. This is the triple's meaning outside the tenant. RDF and every dump write it as the subject and as every value. The source's name appears in no concept IRI, as Wikibase's own prefix names are not the contract ([wikibase-compat.md](../api/wikibase-compat.md) §5.1); only the tenant's own document node for the entity carries the ID form.

**A source declared without an `iri` is unpublished.** Its concept IRI is then `{base}/entity/{name}:{upstream ID}` under the tenant's base, which means nothing outside the tenant; §8 treats its references as unreadable elsewhere.

The document node is `data:mhc:Q1`, `{base}/wiki/Special:EntityData/mhc:Q1`, with `pav:importedFrom` pointing at the source's `entity_data` URL where there is one ([0015](0015-record-format-and-partition-registry.md) §6). The tenant's Wikibase-compatible JSON dump writes the ID form; the tenant's source table is published at `GET /entity-sources` (§10), so that a reader of the dump can map it.

### 8. Across tenants and instances (extends 0018 §5)

**A reference to a source entity crosses a tenant boundary by IRI.** When tenant B reads provider tenant A's `local` graph ([0018](0018-tenants.md) §5), or another instance reads it through `scatter-adapter-triplespace` ([0022](0022-federation.md) §2, which rewrites as 0018 §5 does), a reference to A's `mhc:Q1` is rewritten by the first rule that applies:

1. **A's source is unpublished.** The reference is not readable, as a tenant without a code cannot be referenced ([0018](0018-tenants.md) §5).
2. **The IRI is a registry provider's.** It becomes that provider's ID, `MHQ1`, once the registry has Miraheze Communities as `MH`.
3. **B has a source with the same IRI template.** It becomes B's ID for the entity, `mhcom:Q1`, whatever B named its source.
4. **Otherwise it is unbound.** Every statement whose subject, value, qualifier value or reference value is the reference is **withheld** from B's resolved view. B's `Special:Providers` counts withheld statements per IRI base, so B's administrators can see what declaring a source would add. Nothing is guessed or partly shown.

B never reads A's `source/{name}` partition: the source's data comes from the source, and B fetches it itself once it declares one. The rewrite reads A's source table from A's `config` on the same instance, and from A's public `GET /entity-sources` on another. References to B's own entities still come back as bare IDs ([0022](0022-federation.md) §2).

### 9. Binding is permanent; promotion

**A source's name, number and type IRI templates are fixed by its first record.** A later record may change its label, `api`, `entity_data`, `events`, `role`, licence, namespace names and aliases, and its mirror settings; one that changes the name, the number or a template is refused with `ts-source-binding`. A retired source, one whose record is null, keeps its name and number reserved on the tenant for good, and its IDs keep parsing and resolving read-only. Records in the log carry `mhc:Q1` in their headers and content, so its meaning cannot change under them.

**A source may not duplicate a registry provider.** A declaration with a type IRI template equal to a registry provider's is refused with `ts-source-is-provider`, and the tenant adds the provider to its `providers` list instead ([0018](0018-tenants.md) §5). When the registry later gains such a provider, the tenant may not add it to its `providers` list while the source is unpromoted (`ts-source-is-provider` again), and `registry check` lists the source as promotable. So on one tenant, `mhc:Q1` and `MHQ1` never both name an entity.

**Promotion.** When the registry later allocates a provider whose IRI templates equal a source's, the tenant writes the source's record with `promoted_to = "MH"`, which is refused if the templates differ. From then on:

- `mhc:` is an input form of `MH` on the tenant, as a provider slug is (§3), and `MHC:Q1` redirects to `Item:MHQ1`.
- Projections rewrite `mhc:Q1` to `MHQ1` when they read the tenant's records, as [0018](0018-tenants.md) §5 rewrites a provider tenant's IDs on read. Statement IDs follow [0018](0018-tenants.md) §7: the UUID is kept and the prefix becomes the canonical ID.
- `source/mhc` is frozen and kept. The provider joins the tenant's `providers` list, and mirroring continues in the instance's `mirror/{slug}`.
- Nothing in the log is rewritten.

### 10. API and operations (extends 0012 §5)

| Route, module or command | Behaviour |
|---|---|
| `GET /entity-sources` | The tenant's sources: name, label, namespace, types with IRI templates (absent for an unpublished source), `promoted_to`. Public, as `GET /resolvers` is |
| `meta=siteinfo&siprop=triplespace` | Gains `entity_sources`, the same list |
| `wbgetentities&ids=mhc:Q1` | The entity, from the tenant's view. A source entity not yet held is `missing`, and is enqueued when the source's mirror setting is `on-demand` or above |
| `GET /resolve?q=mhc:Q1` | §5's lookup, with `kind: entity-source` |
| `GET /provider/{name}/mirror`, `POST /provider/{name}/mirror/fetch` | [0070](0070-shallow-entity-mirroring.md) §9's routes, taking a source name where they take a provider code. Lowercase source names and uppercase provider codes never clash |
| `triplespace-cli source add`, `source list`, `source retire`, `source promote`; `registry check` | The operations of §1, §9 and §3 |

Errors: `ts-source-name-taken`, `ts-source-is-provider`, `ts-source-binding`, `ts-namespace-number`, `ts-namespace-name-taken`.

### 11. Adopted wikis' unregistered sources (extends 0035 §3)

**An adopted wiki's entity-source prefix that names no registry provider becomes an entity source of the tenant under the same name**, declared by the adoption job before the first `adopt`, from the source wiki's own entity-source table (concept base URI, entity types). IDs in that prefix are then kept as written, `mhc:Q1`, instead of having no rule. If the name is taken (§3, rule 1), the adoption's preconditions fail ([0035](0035-adopting-a-wikibase.md) §2) and name it.

### 12. Crates (extends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-providers` | The `entity-source` entry and its validation: name grammar, IRI templates, the refusals of §3 and §9, the number range of §4; rewriting between source IDs and IRIs (§7–8) |
| `scatter-wikibase-model` | The source ID form, parsed against the tenant's table of names passed in as data ([0005](0005-crate-organization.md) §3, rule 3); provider slugs as an input form (§3) |
| `scatter-adapter-wikidata` | Fetching from a source's own `api` and `entity_data` with the source's prefix in place of a provider code |
| `scatter-ingest` | The fetch job per source, writing to `source/{name}`; declaring sources during adoption (§11) |
| `scatter-adapter-triplespace` | The rewrite by IRI of §8, reading a provider tenant's `GET /entity-sources` |
| `triplespace-projections` | The rewrite of §8 when reading another tenant, with the withheld-statement counts; the promotion rewrite of §9 |
| `triplespace-titles` | Source namespaces, the namespace-number and name rules of §5, and the lookup of §5 |
| `triplespace-api-rest` | `GET /entity-sources`; `entity_sources` in siteinfo; source names in the mirror routes |
| `triplespace-cli` | `source add`, `list`, `retire`, `promote`; `registry check` |

## Alternatives considered

- **A reserved block of provider codes whose meaning each tenant sets** (the first design, 2026-10-07). It gives 25 codes, not 26, since `YY` is reserved for the tenant-relative form ([0044](0044-tenant-relative-ids.md) §2). It also gives `YAQ5`, a string in the provider form whose meaning is otherwise global, a meaning per tenant that nothing in the string shows. The colon form is Wikibase's own form for this, has no cap, and keeps the source's ID verbatim.
- **Registering every graph.** A commit and a global code per graph, a public record of what each tenant reads, and every instance carrying the entry. Promotion (§9) is still the path for a graph that turns out to be widely used.
- **A keyed type per source.** Wrong by [0009](0009-keyed-entity-types-and-domain.md) §1's test: the source, not the thing, minted the ID. Surrogates and content-derived IRIs would solve problems a source does not have.
- **The source's namespace hosting its entities as pages**, `MHC:Q1` holding an item and `MHC:P31` a property. That would break [0008](0008-namespaces-and-document-pages.md) §1, where the model names the entity type and entity namespaces are keyed by type.
- **A tenant-qualified form across tenants**, `librarybase/mhc:Q1`. The IRI already carries the meaning, and a form that names a tenant would tie the data to where it was first written.

## Consequences

- **A tenant can read any Wikibase without a registry change,** naming its source and its namespace itself.
- **Two tenants' `mhc:Q1` may be different things.** Meaning crosses a tenant only as an IRI, and a reader that has not declared the same source does not see statements that use it. The withheld count shows what declaring one would add.
- **Clashes with the registry are possible only for names the registry adds later,** and then only shadow on the tenant that has the source.
- **Promotion keeps every record.** It costs a rewrite on read for that tenant's old records, as a provider tenant's IDs already cost every reader.
- **The registry's provider numbers halve**, to 4,194,303, which is far beyond the registry's use.
- **0008 gains a tenant range**, which the namespaces tenants already create needed and did not have.
- **Test plan.** Declaring `mhc` with a Wikibase API; a local statement with value `mhc:Q1` enqueues `Q1`, whose `put` lands in `source/mhc` with a ranged revision ID; `MHC:Q1` redirects to `Item:mhc:Q1`, and after Create an item for this to `Item:Q500`; with mirror `off`, `MHC:Q1` shows the external link and fetches nothing; a declaration named `doi`, or with Wikidata's IRI template, is refused; a namespace numbered 120 or 2000 is refused; `wikidata:Q42` in `wbgetentities` returns `WDQ42`; another tenant reading Librarybase without the source withholds the statement and counts it, and with a source `mhcom` for the same IRIs reads `mhcom:Q1`; promotion to a test provider rewrites on read and leaves the log unchanged.

## Open questions

- **Q1. Upstream history.** Whether a source may mirror its edit history, which needs an issuer per source ([0007](0007-actor-identity.md) §1 already says another Wikibase or Miraheze wiki gets one, keyed by its wiki ID) and `log/{name}` and `actors/{name}` partitions of tenant scope.
- **Q2. Verified sources.** A Triplespace instance as a source with `trust = verified` ([0022](0022-federation.md) §2) needs its key chain in the tenant's configuration.
- **Q3. Collisions by construction.** Whether to reserve a syntactic space for tenant names, so that rule 2 of §3 can never apply.
- **Q4. Page repositories.** Whether a page repository's `provider` ([0052](0052-page-repositories-and-title-inheritance.md) §1) may name an entity source, so a tenant can serve an unregistered wiki's pages without a registry entry.
- **Q5. Lua.** Whether [0043](0043-lua-modules.md) §8's letter remapping may map a letter to a source.
- **Q6. Withholding granularity.** §8 withholds a whole statement when one qualifier or reference value is unbound. Whether to drop only that qualifier or reference instead.
- **Q7. Sources that are not Wikibases.** Adapters for a source with a SPARQL endpoint or a dump but no Wikibase API.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §12 | extends | 0005 A80 |
| [0008](0008-namespaces-and-document-pages.md) §2, §3 | §5 | extends | 0008 A31 |
| [0012](0012-api-requirements.md) §5 | §10 | extends | 0012 A56 |
| [0015](0015-record-format-and-partition-registry.md) §2, §3, §5 | §1, §4 | extends | 0015 A37 |
| [0017](0017-entity-id-grammar.md) §1 | §2, §3 | amends | 0017 A10 |
| [0018](0018-tenants.md) §5, §10 | §4, §8 | extends | 0018 A12 |
| [0029](0029-resolver-namespaces.md) §2, §6 | §3 | extends | 0029 A7 |
| [0035](0035-adopting-a-wikibase.md) §3 | §11 | extends | 0035 A4 |
| [0070](0070-shallow-entity-mirroring.md) §2.1, §3, §10 | §4 | extends | 0070 A1 |
| [0070](0070-shallow-entity-mirroring.md) Q3 | §4 | settles | 0070 Q3 |

## References

- [Wikibase: Entity sources](https://doc.wikimedia.org/Wikibase/master/php/docs_topics_entitysources.html): concept base URIs, interwiki prefixes and federated IDs such as `wikidata:Q42`
- [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces): "The namespaces in 100-199 are reserved for site-specific namespaces"; "Namespaces using numbers from 3000 and higher are meant to be used by system administrators" (read 2026-10-08)
- [Manual:Using custom namespaces](https://www.mediawiki.org/wiki/Manual:Using_custom_namespaces): numbering conventions for wiki-local namespaces
- [Miraheze Communities](https://communities.miraheze.org/): the example source
- `claude/familysearch-semantic-layer-plan.md` (Triplespace project notes, 2026-10-07): the shallow mirroring this ADR reuses
