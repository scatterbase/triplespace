# 0114. Badges

- **Status:** Proposed
- **Date:** 2026-11-22
- **Updated:** 2026-11-22 (A1)
- **Author:** James Hare / Claude Opus
- **Uses:** [0003](0003-statement-ui.md), [0026](0026-sitelinks.md), [0070](0070-shallow-entity-mirroring.md), [0091](0091-one-wikibase-adapter.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

Wikibase has badges only on sitelinks: items such as *featured article* (Q17437796) that an editor stores in a sitelink's `badges`, from the list a wiki allows (`$wgWBRepoSettings['badgeItems']`). [0026](0026-sitelinks.md) §1 kept them so. By November 2026 Triplespace had two more marks of the same kind, each built on its own:

- the **"Best value"** badge on a preferred statement ([0003](0003-statement-ui.md) §4, §9), a check icon and fixed text, computed from rank;
- the OpenStreetMap wiki's **retired properties** ([0091](0091-one-wikibase-adapter.md) A3): properties whose English label opens with `(DEPRECATED)`. The adapter dropped their statements, and there was no way to show a reader that a property had been retired.

For the second, James chose on 2026-11-22 to mark a retired property with a statement, and to reuse Wikidata's concepts for it. Wikidata had no class covering a retired predicate from any source: *obsolete Wikidata property* (Q18644427) covers only Wikidata's own. James created *obsolete predicate* (Q142241643) as a subclass of *RDF predicate* and *deprecated feature*, and made Q18644427 a subclass of it. An OpenStreetMap key, a Wikidata property, or any other provider's property is then retired by being an instance of that one class.

The setting the sitelink chapter named for allowed badge items was never registered: nothing in `settings.toml` or the code held the list.

### Direction

James, in the design discussion of 2026-11-22:

- **"Marking as a statement on the property makes sense. Then you can auto-render a (deprecated) badge."**
- **"Statements with a deprecated property should be kept with the badge."**
- **"Rather than reify individual types of obsolete predicates, my idea is to create an item for 'obsolete predicate', make it the parent class of obsolete Wikidata property, and then that can be used for obsolete OSM keys and obsolete properties and keys from other providers as well."**
- **"Should we have a generic badge system? Deprecated predicates are one use cases of badges; Wikidata also uses badges to mark featured articles and such."**

He confirmed the design put to him that day: a badge is always an item; it differs only in what it marks (an entity, a sitelink or a statement) and where it comes from (stored, a class, or rank); a registry lists them; a tenant adds its own; and Wikidata's article badges stay on their sitelinks.

## Decision

### 1. A badge is an item

*Current text: [06](../architecture/06-statements-and-properties.md) §6, §6.1.*

### 2. What a badge marks, and where it comes from (uses 0003 §7)

*Current text: [06](../architecture/06-statements-and-properties.md) §1.2, §6.2.*

### 3. The badge registry and a tenant's own badges

*Current text: [06](../architecture/06-statements-and-properties.md) §6.3.*

### 4. Drawing badges (amends 0003 §4, §9)

*Current text: [19](../architecture/19-site-ui.md) §1.6, §4.4, §4.8, §6.4.*

### 5. Which items a sitelink may carry (amends 0026 §1)

*Current text: [06](../architecture/06-statements-and-properties.md) §4.1.*

## Alternatives considered

- **One class per source of retired predicates** (an "obsolete OpenStreetMap key" beside Q18644427). Rejected by James: one class, *obsolete predicate*, serves every provider.
- **A Triplespace-only item, or a flag on the mirrored state, for the mark.** Either needs no edit to Wikidata, but neither reuses Wikidata's vocabulary, and a flag is a second way of saying what a statement already says.
- **Keep the three marks separate.** Less work now, but each new mark would get its own look, its own tooltip rules and its own translations, which the item already carries.
- **Store computed badges.** Writing a `class` or `rank` badge into the entity would make the JSON differ from Wikibase's and would go stale when the class statements change.

## Consequences

- **One look for every badge.** "Best value", "(deprecated)" and *featured article* are drawn by one component, named and translated by their items.
- **Badges are data.** A tenant adds a badge for one of its own classes without code.
- **The entity JSON is unchanged.** Computed badges are drawn, never stored or exported; stored sitelink badges are Wikibase's.
- **Retired properties keep their statements.** The OpenStreetMap wiki's adapter writes the class statement instead of dropping the property's uses; that change is [0091](0091-one-wikibase-adapter.md)'s, recorded there when it is built.
- **The default badges depend on Wikidata items.** Their names come from Wikidata terms, so an instance that does not read Wikidata shows the badges by ID until a tenant's own items are put in their place.

## Open questions

- **Q1. Badges in the API.** *Settled by A1.*

## References

- Wikidata items checked on 2026-11-22: Q142241643 *obsolete predicate*, Q18644427 *obsolete Wikidata property*, Q71533031 *preferred rank*, and the sitelink badges listed in `badges.toml`.
- [Wikibase: badges](https://www.mediawiki.org/wiki/Wikibase/DataModel#Badges)

## Amendment log

### A1. Computed badges in the API, and what building them decided

- **Date:** 2026-11-22
- **Source:** Direct: James, 2026-11-22, answering Q1: the API returns computed badges for clients other than the site. Their fields (`ts-badges` on `wbgetentities` when `props` names it, `badges` in the page bundle), the registry's `message` field, the `not-badge` refusal, the bound of 200 class reads, reading the roles in Wikidata's mirrored form, and leaving the item's description, the icon and the link undrawn, Claude, while building them, 2026-11-22; confirmed by James, 2026-11-22
- **Change:** extends §2, §3, §4, §5
- **Summary:** `wbgetentities` reports the `class` badges of the entities it serves in a `ts-badges` map beside `entities` when `props` includes `ts-badges`, worked out before `props` filters the claims; the page bundle gives them in `badges` for the entity and each property its statements use. A registry entry may name an interface `message` whose text the badge shows in place of its item's label, with its `-tip` as the tooltip: `ts-best-value` and `ts-badge-deprecated`. A local sitelink naming an item that is not a stored badge is refused with `not-badge`. A walk reads the `instance-of` and `subclass-of` roles as `WDP31` and `WDP279` and at most 200 class entities a request. The site draws a badge's text and tooltip only. Chapter text: 06 §4.1, §6.2, §6.3; 19 §1.6; 18 §2.3, §3.2 (registry).

Replaced text: 06 §4.1, §6.2, §6.3; 19 §1.6.
