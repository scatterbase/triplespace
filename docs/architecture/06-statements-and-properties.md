# 06. Statements and properties

This chapter describes what a statement means on a Triplespace instance once it is stored: the semantic roles that let shape detection, resolvers and constraints refer to a property without knowing its ID, property constraints and where their violations appear, resolver namespaces, sitelinks as normalized URLs and the pages they pair with items, and the pages that carry statements of their own. It assumes the log and change-set records of [01](01-log-and-records.md), the resolved view of [02](02-graphs-rdf-and-query.md), the `view` tables of [03](03-storage-caches-and-search.md) and the entity IDs, clusters and value normalizers of [04](04-entities-and-identifiers.md). The statement itself is canonical Wikibase JSON in source form: its ID keeps Wikibase's `<EntityId>$<UUID>` form ([04](04-entities-and-identifiers.md) §2.1) with the entity ID its source wrote, and the entity part is rewritten to the consumer's preferred form in responses ([04](04-entities-and-identifiers.md) §4.9, §4.12); its `unit`, `globe` and `calendarmodel` are prefixed entity IDs, expanded to IRIs on output ([04](04-entities-and-identifiers.md) §4.7); statements with the same key fuse in the resolved view ([04](04-entities-and-identifiers.md) §4.8); a `wikibase-notation` property's scheme is [04](04-entities-and-identifiers.md) §3.9. How statements are drawn and edited is [19](19-site-ui.md); the API routes that read and write them are [18](18-api.md).

## 1. Semantic roles

*Sources: [0003](../decisions/0003-statement-ui.md) §7; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5.2.*

Property IDs differ between Wikibases, so shape detection and the rank rules refer to **roles**. Each instance maps its own properties onto those roles. A role binding is a `config` record of kind `role` ([23](23-configuration-and-registry.md) §2.2), and the kind has **an instance-scope record beside the tenant records**. The instance record binds the roles that shared rows are computed under: the constraint checker computes shared-row violations under it (§2.4), and the adapters read it and never a tenant's, so that a provider's key map or role list means one property on every tenant ([04](04-entities-and-identifiers.md) §4.6; [05](05-providers-and-ingest.md) §8.8, §8.11). A tenant's record binds roles for its own statement UI, resolvers and constraints on its own data, and may add a local property beside a mirrored one for a role (§1.1); it may not rebind a role the instance record binds, and a tenant-scope write that tries is refused with `ts-instance-policy` ([04](04-entities-and-identifiers.md) §4.10). Where a tenant's bindings differ from the instance's, the difference produces overlay rows for what it touches, which on a Wikidata mirror is everything, so in practice a tenant on a farm does not rebind the constraint roles.

### 1.1 The roles of the statement UI

*Sources: [0003](../decisions/0003-statement-ui.md) §7.*

| Role | Used by | Wikidata property |
|---|---|---|
| `time-point` | Series shape, sort order | P585 point in time |
| `time-start` | Timeline shape | P580 start time |
| `time-end` | Timeline shape | P582 end time |
| `deprecation-reason` | The deprecated fold's labels; "Mark as deprecated…" | P2241 reason for deprecated rank |
| `reference-work` | Footnote titles | P248 stated in |
| `series-ordinal` | Numbered lists | P1545 series ordinal |

- **Mirrored properties.** On an instance that mirrors Wikidata, mirrored properties carry the `WDP` prefix, so the map reads `WDP585`, `WDP580` and so on.
- **Local and mirrored forms together.** One role can have both a local and a mirrored property: for example, `P12` and `WDP585` can both be `time-point`.
- **Constraints as hints.** Wikidata properties declare allowed and required qualifiers as property constraints (P2302). When a group has no statements, or only one or two, these constraints can seed the table's columns before any data exists. They are hints only: once there is enough data, detection on that data wins.
- **Linked forms.** If a local and a mirrored property are linked by `equivalent-property`, a role map can name either one ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §6, in [04](04-entities-and-identifiers.md) §4.6).

### 1.2 Roles bound by other ADRs

*Sources: [0003](../decisions/0003-statement-ui.md) §7.*

Other ADRs bind further roles the same way:

- adapters name the properties they map onto by role, bound in the instance record to registry-provider properties or to the provider's own property type ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §6, in [04](04-entities-and-identifiers.md) §4.6);
- a resolver names the property it looks up by role, such as `doi` (§3.1);
- the constraint vocabulary: `constraint`, `constraint-status`, `constraint-exception`, `constraint-scope`, `constraint-clarification`, and one role for each constraint type and each parameter (§2.1);
- `osm-tag`, `osm-tag-text` and `osm-key`, for OpenStreetMap keys and tags ([0036](../decisions/0036-openstreetmap-providers.md) §4, in [05](05-providers-and-ingest.md));
- GDELT's document and event properties, such as `document-url`, `published`, `theme`, `event-type` and `mentioned-in` ([0037](../decisions/0037-gdelt-provider.md) §3–4, in [05](05-providers-and-ingest.md));
- `subject-page` and `subject-thread`, the items that stand for a document page and a thread as values of Wikidata's *allowed entity types* constraint (Q52004125) (§5.3), and `thread-status`, the `string` property that carries a thread's projected status ([0038](../decisions/0038-page-metadata-and-categories.md) §9, in [14](14-discussions.md));
- `notation-scheme`, the property that restricts which notation schemes a `wikibase-notation` property takes ([0048](../decisions/0048-notation.md) §4, in [04](04-entities-and-identifiers.md) §3.9).

## 2. Property constraints

*Sources: [0031](../decisions/0031-property-constraints.md) §1; [0031](../decisions/0031-property-constraints.md) §2; [0031](../decisions/0031-property-constraints.md) §8; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5.2; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

Constraints are statements on a property entity, read by role; violations are derived facts that a consumer of composition computes, within its own lag, and that are never refused at write.

### 2.1 A constraint is a statement on a property, read by role

*Sources: [0031](../decisions/0031-property-constraints.md) §1.*

Constraints are statements on a property entity whose main property plays the role `constraint`, with the constraint type as the value and the constraint's parameters as qualifiers, exactly Wikidata's `P2302` model. Nothing new is stored to define one; a tenant that mirrors Wikidata has thousands the moment the mirror lands, and a tenant that does not writes them as statements on its own properties.

The vocabulary is bound by **roles** (§1), so that a tenant may use Wikidata's items and properties or its own:

| Role | Wikidata | Meaning |
|---|---|---|
| `constraint` | P2302 | The property that carries constraints |
| `constraint-status` | P2316, with Q21502408 mandatory, Q62026391 suggestion | Severity |
| `constraint-exception` | P2303 | Entities exempt from this constraint |
| `constraint-scope` | P4680, with main / qualifier / reference values | Which snaks the constraint checks |
| `constraint-clarification` | P6607 | Text shown with a violation |
| one role per constraint type | Q21502404 format, Q19474404 single value, Q21502410 distinct values, Q21510865 value type, Q21503250 type, Q21510855 inverse, Q21510862 symmetric, Q21503247 item requires statement, Q21510864 value requires statement, Q21510851 allowed qualifiers, Q21510856 required qualifier, Q21514353 allowed units, Q21510860 range, Q21510859 one of, Q52558054 none of, Q21510854 citation needed, Q52848401 integer, Q51723761 no bounds, Q53869507 property scope, Q64006792 label in language, Q54554025 Commons link, Q55819106 lexeme requires language, and the rest | The constraint type |
| one role per parameter | P2306 property, P2305 item of property constraint (also the allowed values), P2308 class, P2309 relation, P1793 format as a regular expression, P2313 maximum value, P2310 minimum value, P2312 maximum date, P2311 minimum date, P2307 allowed units, P4155 separator, P2304 group by, P2429 expected completeness | The parameter |

A role that a tenant has not bound is a constraint type it cannot check; the checker reports "unrecognised constraint" for it and moves on. **Constraint via SPARQL** (Q21502838) is not supported: nothing on the read path runs SPARQL ([0013](../decisions/0013-postgres-storage.md) §8, in [02](02-graphs-rdf-and-query.md) §4.1), and the type is unsupported in Wikidata's own checker too.

**Constraints on mirrored properties are read from the resolved view**, so a tenant may override or suppress a Wikidata constraint locally ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7, in [05](05-providers-and-ingest.md)) and add its own, and the checker sees the result. The reference to a constraint's source is kept: a violation says whether the constraint came from Wikidata or from here.

### 2.2 Checking is a projection over the resolved view

*Sources: [0031](../decisions/0031-property-constraints.md) §2; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

Violations are **derived facts**, not records. The **constraint projection** is a consumer of composition ([03](03-storage-caches-and-search.md)): it consumes "entity X, tenant T, composed to version N" events from its own queue, at its own rate and with its own lag, in nobody's transaction, checks every statement of the composed entity against the constraints on its properties, and writes the results to `view.constraint_violation` ([03](03-storage-caches-and-search.md) §4.4). It is a view-derived projection, rebuilt by a scan of the composed rows and never by a log replay, and it is never in the log, since a violation is a fact about the current data that the data's next change may remove. It is off during bootstrap and bulk modes ([05](05-providers-and-ingest.md) §3.7) until the operator turns it on.

**What it checks against.** The resolved view ([02](02-graphs-rdf-and-query.md) §1.5): fused statements, entity values compared by cluster ([04](04-entities-and-identifiers.md) §4.4, §4.7), normalized values ([04](04-entities-and-identifiers.md) §4.7, §3.1). So a *distinct values* check on DOIs compares casefolded keys, a *value type* check follows a value to its cluster's fused body, and a *single value* check counts fused statements, not source-graph duplicates.

**What it checks is `constraints.scope`**, a `site` setting with three values. **`local`**, the default, checks local-graph statements and the entities in the tenant's overlay: what the tenant has asserted or touched. **`overlay`** checks every statement of every entity in the tenant's overlay. **`all`** checks the mirrored data too, as Wikidata's own checker shows violations on the items it mirrors; it is a batch job with progress ([19](19-site-ui.md)), run when asked and never a step the projection takes on each event, since Wikidata's own checker is a days-long sampled job and this one would otherwise be run over 110 million entities on every sync.

**Constraint types by what they need:**

| Needs | Types | Read from |
|---|---|---|
| The statement alone | format, one of, none of, integer, no bounds, range, allowed units, allowed qualifiers, required qualifier, citation needed, property scope, single value, single best value | The resolved JSON |
| Other statements of the same entity | item requires statement, label in language, lexeme requires language | The resolved JSON |
| The value's entity | value type, value requires statement | One entity read |
| The entity's classes | type, value type (check `P31`/`P279` chains up to a depth set in `site` configuration) | The L0 class cache ([03](03-storage-caches-and-search.md) §9.2). A change to a class's `P279` does not invalidate stored results until the next composition of each subject; a *type* result can therefore be stale by the class cache's lifetime and the subject's composition lag |
| Every statement with the same value | distinct values | `view.identifier` for identifier-typed properties; `view.statement_assertion` and a value index otherwise |
| The value entity's statements back | inverse, symmetric | `view.entity_ref` and one entity read |

**Incremental and batch.** An entity's own statements are re-checked when the entity is composed, so a marker follows an interactive edit by the consumer's lag, milliseconds for one entity and honestly longer during a sync; the lag is shown on the consumer's own pages ([21](21-special-pages.md)). Types that depend on *other* entities (*inverse*, *symmetric*, *value requires statement*) are re-checked for the referring entity when it is next composed, not fanned out to referrers when the value changes, since nothing re-resolves referrers ([04](04-entities-and-identifiers.md) §4.4). A change to a constraint itself re-checks every statement using the property as a batch job with progress ([19](19-site-ui.md)), since that is a full scan of one property; a constraint-statement change on a mirrored property, which a sync can bring thousands of, enqueues such a batch rather than a scan in the consumer. Bootstrap ([05](05-providers-and-ingest.md) §3.8) leaves the projection off, and a Wikidata-scale instance that turns it on may run `all` for a chosen set of properties first.

**Exceptions and status** are applied as Wikidata applies them: an entity listed under `constraint-exception` produces no violation; a `suggestion` constraint produces a violation of severity `suggestion`, a `mandatory` one of severity `mandatory`, and the rest `normal`. **Nothing is refused.** A violation of any severity is a marker and a row, never an error on a write; the gate for writes is the edit filter, which does not read the graph, and the two are kept apart on purpose (§2.5).

### 2.3 Where violations appear

*Sources: [0031](../decisions/0031-property-constraints.md) §3.*

- **On the entity page**, a **constraint marker** beside a violating value: an icon by severity, whose popover names the constraint, its source (Wikidata or this instance), its clarification text, and the parameter that failed, with a link to the property's constraint statement. This is what WikibaseQualityConstraints shows, drawn with the components the statement UI already has ([19](19-site-ui.md)), and hidden when there are no violations.
- **On the property page**, a **Constraints** tab listing each constraint with its violation count and a link to the report.
- **`Special:ConstraintReport/{property}`** and **`Special:ConstraintReport/{type}`**: violations by property or by type, paged, filterable by severity and source graph, with the entity, the statement and the failing value on each row. The source-graph filter defaults to the local graph, as every report does ([21](21-special-pages.md)); widening it is live, since `view.constraint_violation` already covers the resolved view. This is where a community works through a class of problems.
- **`Special:ConstraintReport/{entity}`**: everything on one entity, which is also what `wbcheckconstraints` returns ([18](18-api.md)).

Violations are **not a feed** ([16](16-logs-feeds-and-notifications.md)): they are facts about the present, not events, and a reader who wants to know when a violation appeared reads the entity's history. A count of violations by property is published for the dashboard-style view above.

### 2.4 Tenants

*Sources: [0031](../decisions/0031-property-constraints.md) §4; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5.2.*

Constraints and violations follow the overlay rule of [08](08-tenants-and-instances.md) §4.1 and [03](03-storage-caches-and-search.md) §7: violations on shared rows are computed once for the instance, under the **instance-scope role bindings** (§1), by the `all` batch job of §2.2 when the operator runs it, and shown to every tenant that reads the provider; a tenant's own `constraints.scope` governs its local and overlay rows. A tenant whose overlay changes an entity, or whose constraints differ from the shared ones because it overrides a mirrored constraint or binds a local property beside a mirrored one, has its own violation rows for that entity. The instance binding is what gives the shared check a vocabulary to compute with; a tenant does not rebind the constraint roles (§1), and on an instance whose record leaves them unbound nothing is checked on shared rows.

### 2.5 Constraints and edit filters

*Sources: [0031](../decisions/0031-property-constraints.md) §8; [0031](../decisions/0031-property-constraints.md) §2.*

Filters gate; constraints report. A filter ([09](09-security-and-moderation.md) §7) sees the change and the actor and refuses or tags a write in microseconds. A constraint sees the data and the graph around it and marks a statement after the fact. A tenant that wants a hard rule writes a filter; a tenant that wants to know where its data disagrees with its own norms writes a constraint. The two never read each other, and the edit filters' "bounded lookup in v1" question stays open on its own terms.

## 3. Resolver namespaces

*Sources: [0029](../decisions/0029-resolver-namespaces.md) §1; [0029](../decisions/0029-resolver-namespaces.md) §2.*

A resolver is a namespace whose titles are keys of a property bound to it by role. Its registry file and namespace numbers are [23](23-configuration-and-registry.md) §4.4; its API and UI are [18](18-api.md) and [19](19-site-ui.md).

### 3.1 A resolver is a namespace bound to a property

*Sources: [0029](../decisions/0029-resolver-namespaces.md) §1.*

A resolver is a registry entry, a `config` record of kind `resolver` in the tenant `config` ([23](23-configuration-and-registry.md) §2.2), with defaults in `docs/registry/resolvers.toml`. It records:

| Field | Meaning | DOI |
|---|---|---|
| **Namespace** | The MediaWiki namespace whose titles are keys, of kind `resolver` (§3.2) | `DOI` |
| **Binding** | A **role** (§1) naming the property, or `sitelink` (§3.5) | `doi`, bound on a Wikidata-mirroring tenant to `WDP356` and to any local DOI property |
| **Grammar** | Which strings are valid keys ([04](04-entities-and-identifiers.md) §3.1) | `10\.\d{4,9}/.+`, after the normalizer |
| **Normalizer** | How input becomes the canonical key, including the **case rule** | Strip a leading `https://doi.org/`, `http://dx.doi.org/` or `doi:`; percent-decode; **casefold** |
| **External IRI** | Where the key resolves outside the instance | `https://doi.org/{key}` |
| **Label** | For the disambiguation page and the identity line | "DOI" |

**The binding is a role, so the property may come from any provider.** A tenant binds the role to the properties it holds, exactly as it binds `time-point` for the statement UI. Where two bound properties are joined by `equivalent-property` ([04](04-entities-and-identifiers.md) §4.6) they are already one predicate in the resolved view; where they are not, the resolver reads both. The normalizer of a resolver **is** the normalizer of its property's values ([04](04-entities-and-identifiers.md) §4.7): registering a resolver for a role sets the normalizer for every property bound to it, so that the index and the lookup agree by construction. A property whose values a resolver normalizes with casefolding therefore compares its values case-insensitively everywhere, in statement fusion as in lookup, which is the right answer for a DOI and is why a resolver declares its case rule rather than leaving it to each property. Registering a resolver for a role whose properties already hold values, or changing a resolver's normalizer, is a normalizer change: it enqueues the per-property rebuild job that recomputes the stored keys, which carry the normalizer's version, and bumps the rendering version of the update stream ([04](04-entities-and-identifiers.md) §4.7); the lookup reads rows of the new version as the job writes them.

**DOI is the first resolver, and is not a keyed type.** A DOI is a value an entity carries, not a subject to say things about, and a citation record stays an item with a DOI statement. The `doi:` prefix reserved for a keyed type is withdrawn.

### 3.2 The `resolver` namespace kind

*Sources: [0029](../decisions/0029-resolver-namespaces.md) §2.*

`resolver` joins the namespace kinds of [10](10-pages-and-content-models.md), beside `pages`, `reserved` and `virtual`: entity view, document, composite and thread namespaces are `pages` namespaces told apart by content model, and `resolver` stays a kind of its own because a resolver namespace holds no pages and so has no model. It holds no records either: every title in it is a key, and viewing it performs the lookup of §3.3. Its paired talk namespace is `reserved` and empty, as `Thread talk` is ([14](14-discussions.md)). Its **title normalizer** is the resolver's normalizer, and `meta=siteinfo` reports its `case` as `case-sensitive` whatever the case rule, because the server normalizes. Subpages are not allowed; a `/` inside a DOI is part of the key, and the resolver's grammar, not the namespace, decides what a key may contain.

**Resolver names and keyed-type names share one namespace.** `doi` cannot be both a resolver and a keyed type, so `doi:10.1000/xyz` in an API parameter can mean only one thing ([04](04-entities-and-identifiers.md) §2.1). The registry refuses a resolver whose name is a keyed type's, and the reverse. Notation scheme names join the same namespace ([04](04-entities-and-identifiers.md) §3.8): `osm:amenity=cafe` typed into `/resolve` means the notation, as `doi:10.1000/x` means the DOI resolver. So do registry provider slugs, `wikidata:Q42` being an input form of `WDQ42`, and, on one tenant only, the names of that tenant's entity sources ([0078](../decisions/0078-entity-sources.md) §3, in [05](05-providers-and-ingest.md)). A source may not take a name the registry has; a registry name added later that a tenant's source already uses is shadowed on that tenant. An entity source's namespace is a `resolver` namespace whose lookup is [0078](../decisions/0078-entity-sources.md) §5's.

### 3.3 Resolution

*Sources: [0029](../decisions/0029-resolver-namespaces.md) §3.*

Viewing `DOI:10.1000/xyz`, or calling `GET /resolve?q=doi:10.1000/xyz` ([18](18-api.md)), does the following:

1. **Normalize** the key with the resolver's normalizer and check it against the grammar. An invalid key is a page saying so.
2. **Look up** `(property, key)` in `view.identifier` ([03](03-storage-caches-and-search.md) §4.3) for every property bound to the role, in the viewer's tenant's resolved view, so that mirrored values count and deleted or hidden entities ([09](09-security-and-moderation.md) §6) do not.
3. **Decide:**

| Result | Response |
|---|---|
| Exactly one entity, holding exactly one value for the property | A **redirect** (HTTP 303) to the entity's page, `Item:Q5`, with the resolver named in the identity line ("Reached via DOI 10.1000/xyz") |
| No entity | A **disambiguation page** saying no entity carries the key, with the external link (`https://doi.org/…`) and, for editors, **Create an item with this DOI**, which opens `Special:NewItem` with `statement={property}:{key}` filled in ([21](21-special-pages.md)) |
| Several entities | A disambiguation page listing them with their labels and descriptions, in `Special:ItemDisambiguation`'s layout ([21](21-special-pages.md)), and a note that this may be a duplicate to merge ([04](04-entities-and-identifiers.md) §4.2) |
| One entity with several values for the property | A disambiguation page showing the entity and each of its values, since the key does not name the entity unambiguously |

The lookup never guesses: any case but the first shows a page. It is also cheap: one primary-key range read per bound property.

**Match keys.** A resolver's property is a natural candidate for a uniqueness constraint ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.5; `match_key` in [03](03-storage-caches-and-search.md) §4.3), and a tenant that adds one will see the "several entities" page only for values that predate the constraint. The two are kept separate: a match key refuses a duplicate at write; a resolver tolerates it at read and shows it.

### 3.4 Where resolvers appear

*Sources: [0029](../decisions/0029-resolver-namespaces.md) §4.*

- **Titles and links.** `[[DOI:10.1000/xyz]]` in a document page or a post ([10](10-pages-and-content-models.md), [14](14-discussions.md)) links to the resolver page and renders, where the lookup is unique, with the target entity's label. A `page_link` row records the link with the resolver title as its target, since the target entity may change as data changes.
- **Search** ([19](19-site-ui.md)). Input that normalizes to a valid key of some resolver gets a "Go to" suggestion beside the keyed-type and ID suggestions the title resolver already offers ([04](04-entities-and-identifiers.md) §2.1), labelled with the resolver: "Go to the item with DOI 10.1000/xyz".
- **The entity page.** A statement whose property is bound to a resolver shows the external link (the value cell already does this for `external-id` formatter URLs, [19](19-site-ui.md)) and a **permalink** in the resolver form, `DOI:10.1000/xyz`, which is the citable address for "the item with this DOI, whatever its ID".
- **RDF.** Nothing. A resolver page has no document node and emits no triples; the DOI is already in the main graph as the value of its property, with Wikibase's normalized form (`wdtn:`) carrying the external IRI ([wikibase-compat.md §5.5](../api/wikibase-compat.md)).

### 3.5 A resolver bound to sitelinks

*Sources: [0029](../decisions/0029-resolver-namespaces.md) §5.*

A resolver's binding may be `sitelink` instead of a role. Its key is then a URL, normalized as §4.1 normalizes sitelinks, and the lookup reads `view.sitelink` by `url_key`, which is unique by construction (§4.2), so the "several entities" case cannot arise. `URL:https://en.wikipedia.org/wiki/Douglas_Adams` reaches the item linked to that page. This is what `Special:ItemByTitle` and `Special:GoToLinkedPage` do on Wikibase, and both are served as aliases of it. It ships as a default resolver in `resolvers.toml`.

## 4. Sitelinks

*Sources: [0026](../decisions/0026-sitelinks.md) §1; [0026](../decisions/0026-sitelinks.md) §2.*

The `view.sitelink` table is [03](03-storage-caches-and-search.md) §4.3; the RDF a sitelink emits, including the page node of a page paired with an item, is [02](02-graphs-rdf-and-query.md) §5.5; the API modules and UI are [18](18-api.md) and [19](19-site-ui.md).

### 4.1 A sitelink is a normalized URL, with badges

*Sources: [0026](../decisions/0026-sitelinks.md) §1.*

A sitelink has a **URL**, a set of **badges** (item IDs, as in Wikibase) and an optional **title** for display. The URL is normalized when it is written, and the normalized form is the link's identity:

1. parse as a WHATWG URL; reject anything that does not parse;
2. the scheme must be in the tenant's allowed set (`sitelinks.schemes` in `site` configuration; default `https` and `http`);
3. lowercase the scheme; normalize the host with the Domain normalizer ([04](04-entities-and-identifiers.md) §3.6), so it is a valid Domain key in A-label form; drop a default port;
4. normalize percent-encoding in the path, query and fragment (RFC 3986 §6.2.2: uppercase hex digits, decode unreserved characters); an empty path becomes `/`;
5. keep the query and the fragment. A fragment names a section, and a section is a legitimate target.

Nothing else is folded: `/Foo` and `/Foo/` are different URLs, as they are to the server that serves them.

**A link to the tenant's own pages is stored by page ID,** not URL (§4.6). The normalizer is the `url` data type's key in the normalizer table of [04](04-entities-and-identifiers.md) §4.7, and the `url` data type used by statements gets it too.

**Badges** are item IDs, stored as written and compared by cluster like any entity value ([04](04-entities-and-identifiers.md) §4.7). Which items may be badges is `site` configuration, as `$wgWBRepoSettings['badgeItems']` is.

### 4.2 The host is the site ID; one link per host; one item per URL

*Sources: [0026](../decisions/0026-sitelinks.md) §2.*

**The site ID of a sitelink is its host**, in A-label form: `en.wikipedia.org`, `collections.example.museum`. This keeps both Wikibase invariants, with the host in place of the site:

- **An item has at most one sitelink per host.** Writing a second is a `sitelink-conflict`, as Wikibase reports it. An item that needs two pages on one host says so with statements, not sitelinks.
- **A normalized URL belongs to at most one item.** Writing it on a second item is the same error, naming the first. The check reads the resolved view, so a local write that collides with a mirrored link on another item is refused naming that item; how the resolved view decides between graphs that disagree is §4.5.

A sitelink to another tenant's or instance's entity page is a plain sitelink, asserting nothing about identity, like a link to any page. An editor who means "this is Librarybase's Q6" writes `same-as` to `LBQ6` ([04](04-entities-and-identifiers.md) §4.3, [08](08-tenants-and-instances.md) §4.1); the two are different statements and the UI does not convert one into the other.

### 4.3 Site aliases

*Sources: [0026](../decisions/0026-sitelinks.md) §2.*

**Site aliases keep Wikidata tooling working.** Tools and clients hard-code MediaWiki site IDs (`enwiki`, `dewikisource`, `commonswiki`) and pass titles, not URLs. A **site alias** maps a MediaWiki site ID to a host, an article path and a language, so that `enwiki` + `Douglas Adams` and `https://en.wikipedia.org/wiki/Douglas_Adams` are the same sitelink:

| Field | Example |
|---|---|
| Site ID | `enwiki` |
| Host | `en.wikipedia.org` |
| Article path | `/wiki/$1` |
| Language | `en` |

The defaults ship in `docs/registry/sites.toml` ([23](23-configuration-and-registry.md) §4.1), generated from Wikimedia's site matrix and committed; a tenant adds aliases for other MediaWiki hosts as `config` records of kind `site-alias` ([23](23-configuration-and-registry.md) §2.2). Where a host has an alias, the JSON `sitelinks` map is keyed by the alias's site ID and the entry carries the title in MediaWiki form; where it has none, the map is keyed by the host and `title` is the URL's path, query and fragment. Every entry carries `url`, as Wikibase's JSON already does. Input accepts either form everywhere.

A tenant registers a site alias for its own host too (§4.6).

### 4.4 Allow and deny lists

*Sources: [0026](../decisions/0026-sitelinks.md) §3.*

Which hosts may be linked is policy in two layers, both `config` records of kind `sitelink-policy` ([23](23-configuration-and-registry.md) §2.2):

| Scope | Record | Meaning |
|---|---|---|
| **Instance** (farm) | `sitelink-policy:deny`, in the instance `config` | Domain keys that no tenant may link. A spam blocklist. **It cannot be overridden by a tenant.** |
| **Tenant** | `sitelink-policy:mode` (`allow` or `deny`) and `sitelink-policy:list`, in the tenant `config` | In `allow` mode everything not on the list is permitted, and the list is the tenant's blocklist. In `deny` mode nothing not on the list is permitted, and the list is the tenant's allowlist. The default mode is `allow` with an empty list. |

**Matching is by Domain hierarchy.** A list entry matches a host equal to it or beneath it, so `wikipedia.org` covers every language edition and `example.org` covers `www.example.org`; this is the `key_parents` relation the search index already holds ([03](03-storage-caches-and-search.md) §11.1). A URL is permitted when the instance deny list does not match its host and the tenant's mode and list permit it.

**Lists are evaluated at write** and refuse a link with `ts-sitelink-denied`, naming the list. They are **also applied to the resolved view**: a sitelink in any source graph whose host is denied is left out of the resolved view, the RDF and the search document, with the source graph untouched, as suppressions ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3, in [05](05-providers-and-ingest.md)) already work. So a farm blocklist added after the fact removes spam from every tenant's view at once without touching any log, and a mirrored sitelink to a denied host is not shown either.

**Links that a list change leaves stranded** are listed for maintenance beside the corrections of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7 ([05](05-providers-and-ingest.md)), so an editor can remove or reconsider them. Changing a tenant's lists needs `ts-config`; the instance list, `ts-config` at the farm ([09](09-security-and-moderation.md) §2.3).

### 4.5 Reconciliation

*Sources: [0026](../decisions/0026-sitelinks.md) §4.*

The sitelink row of the reconciliation table ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3, in [05](05-providers-and-ingest.md)) reads:

| What | Rule |
|---|---|
| Sitelinks | Union by normalized URL across graphs. Where two graphs give one host different URLs for the same entity, the local graph wins, then the provider order of [04](04-entities-and-identifiers.md) §4.4. Badges are the union. A local `override` may suppress or replace a mirrored sitelink ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.2) |

**One URL across entities.** Where two entities hold one normalized URL in different graphs, a local link on one and a mirrored link on the other, the local link wins: the mirrored link is dropped from the resolved view, `view.sitelink` keeps the one-item-per-URL invariant of §4.2, and the dropped link is listed for maintenance beside the corrections of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7 ([05](05-providers-and-ingest.md) §2.5) so an editor can see what upstream says. Between two mirrored links the provider order decides. The sitelink projection never fails on such a conflict: a sync batch that brings a colliding link is applied, and the collision is resolved by the rule, not reported as an error of the batch. A local write that would collide with a mirrored link is refused at write naming the item (§4.2), so the rule only ever decides between what a sync brought and what a tenant already holds.

Mirrored sitelinks arrive as site IDs. The provider's adapter resolves them to URLs through the provider's own site table (Wikidata's `sites`, read once per sync) and writes URLs, so the mirror graph holds the same shape as the local graph and nothing downstream knows about site IDs.

### 4.6 Pages paired with items

*Sources: [0038](../decisions/0038-page-metadata-and-categories.md) §6; [0026](../decisions/0026-sitelinks.md) §1; [0026](../decisions/0026-sitelinks.md) §2.*

**A sitelink to the tenant's own host targets a page.** A tenant is served at one or more hosts ([08](08-tenants-and-instances.md) §1.1, §6.1). A sitelink in the local graph whose host is one of them is stored by **page ID**, not by URL. It is written as any sitelink is written, by site ID and title or by URL (§4.3), and the title is resolved to a page ID at write time. A title with no page is refused with `ts-sitelink-no-page`, as Wikibase refuses a link to a missing page. The URL and title in the canonical JSON are derived from the page's current title, so **a move does not break the link** and no record is written when a page moves.

**Targets** are document pages in any document namespace. Threads and talk pages cannot be sitelinked, and nor can a title whose primary is a page repository's page, which is refused with `ts-sitelink-foreign`, because a foreign page's ID changes when it is forked ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6, in [13](13-mirrored-pages.md)).

**The invariants of §4.2 give a one-to-one pairing.** For the per-host rule, all of a tenant's own hosts count as one host, so an item has at most one link to the tenant's pages; and a page ID belongs to at most one item, as a URL does. So a paired page leads to one item and the item back to one page, with no disambiguation.

**The tenant's own site alias.** A tenant registers a `site-alias` for its own host (§4.3) with a site ID of its choosing. Then `wbgetentities&sites={site ID}&titles=Douglas Adams`, `wbsetsitelink`, `Special:ItemByTitle` and the sitelink resolver (§3.5) all reach the item from the page's title. **The title becomes a way to reach the item**, with no new mechanism.

**Mirrored sitelinks to the tenant's host** are URLs in their mirror graph. The sitelink projection resolves such a URL to a page by title when it can, and the reconciliation of §4.5 (local wins per host) decides between it and a local link.

**A deleted page's sitelink leaves the resolved view** while the page is deleted, as a denied host's does (§4.4), and comes back when the deletion is retired. The record in the local graph is untouched.

## 5. Pages that carry statements

*Sources: [0038](../decisions/0038-page-metadata-and-categories.md) §1; [0038](../decisions/0038-page-metadata-and-categories.md) §2.*

The `view.page_statements` table is [03](03-storage-caches-and-search.md) §4.7; the Page data tab is [19](19-site-ui.md); the REST routes are [18](18-api.md); the RDF of a page node is [02](02-graphs-rdf-and-query.md) §5.4.

### 5.1 Which pages, and their records

*Sources: [0038](../decisions/0038-page-metadata-and-categories.md) §1.*

**Which pages.** Every document page (the `document` namespaces of [10](10-pages-and-content-models.md): main, `User`, `Project` and `Category`), every thread and every board ([14](14-discussions.md)), whose statements describe the board, not its threads. Not talk pages, which are composite and have no records of their own, and not entity views, whose subjects are entities with statements of their own.

**Records.** A page's statements are written as change sets: payload type `scatter:v0/changeset`, appended to the tenant's `pages` partition and keyed by the page ID, beside the page's `page` or `thread` records. A change set on a page may add, change and remove statements. It may not carry aliases or sitelinks; a page has a title, and its sitelinks are held by items (§4.6). It may not carry labels or descriptions either, **except on File pages**, where the MediaInfo captions are `add`, `remove` and `override` of terms on the subject `M{page ID}`, the same operations a term on an entity takes ([0065](../decisions/0065-mediainfo-captions-and-commons.md) §1, in [12](12-files-and-media.md)). Like every record in `pages`, it takes a revision ID ([03](03-storage-caches-and-search.md) §2.3) and needs a base offset ([01](01-log-and-records.md) §7). So **a page has one history**, in which text revisions and statement revisions interleave in the order they were made; a file page's uploads are records in the same partition, keyed by the page ID, so file versions share that history too ([12](12-files-and-media.md)).

**Moderation follows the page.** Protection and deletion are ACLs on the page ([09](09-security-and-moderation.md) §4), and they cover its statements: a protected page's statements need the same right as its text, and a deleted page's statements leave every view with it. Erasure and hiding apply to statement records as to any record.

### 5.2 The subject is the page ID

*Sources: [0038](../decisions/0038-page-metadata-and-categories.md) §1.*

Statements use the same properties and data types as entity statements, local and mirrored. Where a subject has to be written as a string, in a statement ID or in a `view` column ([03](03-storage-caches-and-search.md) §4.7), it is the page ID in decimal: statement IDs take the Wikibase form of [04](04-entities-and-identifiers.md) §2.1 as `{page ID}$<UUID>`. A string of digits alone never parses as an entity ID ([04](04-entities-and-identifiers.md) §2.1), since every local, foreign and keyed ID starts with a letter, so the two kinds of subject cannot be confused.

**No entity ID, except on File pages.** Page statements have no `M`-style or other entity ID, are not returned by `wbgetentities`, and cannot be edited through the Wikibase Action API modules. They are served by REST routes ([18](18-api.md)). In `prop=revisions`, a statement revision appears as MediaWiki shows a revision that changed only a secondary slot: the main text is unchanged, and the summary describes the change. File pages are the exception ([04](04-entities-and-identifiers.md) §5.2, [12](12-files-and-media.md)): their statements have the MediaInfo ID `M{page ID}` and are read and written through `wbgetentities` and the Wikibase statement modules, as on Commons, because tools written for Structured Data on Commons expect that contract; they appear in the `mediainfo` slot (`wikibase-mediainfo`) in `prop=revisions`, and aliases and sitelinks on `M` IDs are refused with `not-supported` ([0065](../decisions/0065-mediainfo-captions-and-commons.md) §1).

### 5.3 Asserted and projected statements

*Sources: [0038](../decisions/0038-page-metadata-and-categories.md) §2.*

A page's resolved statements are the union of two kinds:

| Kind | Source | Editable |
|---|---|---|
| **Asserted** | Change sets in `pages` (§5.1) | Yes, in the Page data tab ([19](19-site-ui.md)) and the REST routes ([18](18-api.md)) |
| **Projected** | Category mappings ([10](10-pages-and-content-models.md)) and a thread's status ([14](14-discussions.md)) | No. Changing the source changes them |

**Projected statements are derived facts,** like constraint violations (§2.2): never records, removed when their source goes away, rebuilt with the view. The provenance response ([19](19-site-ui.md)) names their source, such as "from Category:Articles needing cleanup" or "set by Example in reply 1234", in a `derived` entry in place of a graph.

**Identical statements fuse.** If an editor asserts a statement that a mapping also projects, the two are fused as statements from two graphs are ([04](04-entities-and-identifiers.md) §4.8), and the provenance lists both. Removing the category leaves the asserted statement. They are two independent claims, not one fact with two definitions, so nothing has to be reconciled.

**Constraints apply to pages.** The constraint projection checks page statements as it checks entity statements (§2.1). Wikidata's *allowed entity types* constraint (Q52004125) lists its allowed types as items; two roles, `subject-page` and `subject-thread`, name the items that stand for "document page" and "thread" (§1.2), so a tenant can mark a property as page-only or keep it off pages. Like every constraint, it reports and never refuses.
