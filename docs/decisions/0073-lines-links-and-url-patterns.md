# 0073. Lines, links and URL patterns

- **Status:** Proposed
- **Date:** 2026-10-07
- **Updated:** 2026-10-09 (A2)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0013](0013-postgres-storage.md), [0026](0026-sitelinks.md), [0070](0070-shallow-entity-mirroring.md), [0071](0071-derived-statements-from-mirrored-pages.md), [0072](0072-template-mappings.md)
- **Chapters:** [13](../architecture/13-mirrored-pages.md), [22](../architecture/22-crates-and-stack.md)

## Context

The second half of the extraction scope of [0071](0071-derived-statements-from-mirrored-pages.md) is the research bibliography itself: lists of resources, one per line, each line a link and some words. Two FamilySearch Research Wiki pages show the two layouts:

- **"Ireland Land and Property"** lists resources as bullet lines under headings and bold pseudo-headings (`'''Deeds'''`, `'''Griffith's Valuation'''`): `*'''1850-1885''' [https://www.ancestry.com/search/collections/61939/ Ireland, Encumbered Estates, 1850-1885] at Ancestry — index & images ($)`.
- **"Mexico Colonial Records"** lists them as rows of a wikitable whose header names the columns: Record collection, Years covered, Record type, Language, Who is in the records.

In both, the link names the resource, and most links carry an identifier: `ancestry.com/search/collections/61939/` is Ancestry collection 61939. Wikidata already curates how to read such URLs: an external-identifier property can carry a **URL match pattern** (P8966), a regular expression with the identifier as its first capture group, optionally with a **URL match replacement value** (P8967). Those patterns serve Wikidata's own "paste a URL" tools.

[0072](0072-template-mappings.md) handles the resources named by template calls. This ADR handles the rest: lines, the links in them, and the identifiers in the links.

### Direction

James's direction, from the design discussion of 2026-10-07:

- **"Parsing external links from bullet lines"** is the other half of the simplified scope. Reading years, access terms and populations from a line's words is left to a later analysis module.
- **The crates may later serve the Wikipedia Citations Database**, whose citations are links and templates in `<ref>` tags.

## Decision

### 1. Lines

*Current text: [13](../architecture/13-mirrored-pages.md) §8.1.*

### 2. Links

*Current text: [13](../architecture/13-mirrored-pages.md) §8.2.*

### 3. URL normalization and match patterns (uses 0026 §1)

*Changed by A1.*

*Current text: [13](../architecture/13-mirrored-pages.md) §8.3.*

### 4. Which links are resources

*Current text: [13](../architecture/13-mirrored-pages.md) §8.4.*

### 5. Line settings and statements

*Current text: [13](../architecture/13-mirrored-pages.md) §6.2, §8.5.*

### 6. Evidence

*Current text: [13](../architecture/13-mirrored-pages.md) §8.6.*

### 7. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.3.*

### 8. The first source, for illustration

*Current text: [13](../architecture/13-mirrored-pages.md) §8.7.*

## Alternatives considered

- **A pattern registry of our own** instead of patterns on properties. Wikidata already curates hundreds of patterns, and a property is where a reader looks for how its values are written.
- **Reading the prose of a line** (years, access, price) with rules. Every wiki words these differently, and the FamilySearch pages already disagree with themselves; that is the analysis module's job, over stored evidence.
- **Treating every link on a page as a resource.** Navigation, footers and "see also" links would become resources; lines are where the bibliography is.

## Consequences

- **Any wiki that lists links in lists or tables is a source**, with identifiers wherever Wikidata, or the tenant, has a pattern for the host.
- **Coverage is visible by host.** Unmatched URLs grouped by host show which patterns to add.
- **Resources inherit their page's facets**, so a collection linked from "Ireland Land and Property" is about Ireland and land records without anyone saying so twice; a heading or column can refine that.
- **The scanner and matcher serve citation extraction too**, for a later WCD pipeline.
- **Test plan.** The Ireland page yields one subject per external link and `RecordSearch` call, with "Deeds" lines carrying the deeds record type from their pseudo-heading; `ancestry.com/search/collections/61939/` and `search.ancestry.com/search/db.aspx?dbid=61403` both yield Ancestry collection IDs by pattern; the two `RecordSearch|1804886` lines make one subject; the Mexico table's rows yield subjects with start and end years from the "Years covered" column; a URL with `utm_source` matches as without it; a lookaround pattern is skipped and reported.

## Open questions

- **Q1. Internal links as relations.** A line linking another guide page (`[[Ireland Estate Records|…]]`) says the two guides are related; whether a `links` setting should write that.
- **Q2. Resources on one line.** "Also at: MyHeritage" names a second access point for the same records; whether to relate them, and by which property, or leave it to the `based on` link of the plan.
- **Q3. Definition lists and plain paragraphs** of links, which some wikis use for bibliographies.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §7 | amends | 0005 A76 |

## References

- [Property:P8966](https://www.wikidata.org/wiki/Property:P8966) URL match pattern, and [Property:P8967](https://www.wikidata.org/wiki/Property:P8967) URL match replacement value
- [Help:Lists](https://www.mediawiki.org/wiki/Help:Lists) and [Help:Tables](https://www.mediawiki.org/wiki/Help:Tables): list and table syntax
- FamilySearch Research Wiki pages "Mexico Colonial Records" and "Ireland Land and Property" (wikitext supplied by James, 2026-10-07)

## Amendment log

### A1. Undesignated identifiers in `lines.identifiers` find but never create

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §3
- **Summary:** An undesignated `match_key` property may find a subject (0071 §4 step 1, via `view.identifier`) but never create one; the refusal in 0071 §4 step 3 stands, and `lines.identifiers` follows the same rule: an identifier a URL yields under an undesignated property in the list is a lookup key for finding the subject, not a creation key. (PENDING F4)

Replaced text (§3):

> They become the subject's match keys in that order ([0071](0071-derived-statements-from-mirrored-pages.md) §4).

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [13](../architecture/13-mirrored-pages.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
