# 0073. Lines, links and URL patterns

- **Status:** Proposed
- **Date:** 2026-10-07
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0013](0013-postgres-storage.md), [0026](0026-sitelinks.md), [0070](0070-shallow-entity-mirroring.md), [0071](0071-derived-statements-from-mirrored-pages.md), [0072](0072-template-mappings.md)

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

**A line is the unit a resource is read from.** The scanner of [0072](0072-template-mappings.md) §7 yields:

- **List items**, `*` and `#` at any depth, and `:` indented items. A nested item has its parent item as context, so "Also at: …" under a resource line is known to be about it.
- **Table rows** of any table, from `|-` to the next `|-` or `|}`. A row of `!` cells before the first data row is the table's header row, and its plain-text cells are the table's **column names**; a row-header cell (`! scope="row" |`) is a cell of its row.
- **Nothing else.** Paragraph text is not a line; it stays in the evidence of the lines near it only as the heading context below.

Each line carries its **heading path**: the page's headings above it, levels 2 to 6, and **pseudo-headings**, a paragraph that consists only of bold or italic text and is followed by a list or table, treated as a heading one level below the current one. `'''Deeds'''` in the Ireland page is a pseudo-heading under "Resources › Online Resources".

### 2. Links

**Each line yields its links**, in order:

| Kind | From | Kept as |
|---|---|---|
| External | `[url text]`, and bare URLs in text | The URL and its text |
| Call subject | A template call that a mapping of [0072](0072-template-mappings.md) with `subject = call` matched, such as `{{RecordSearch|…}}` | The call's subject and label |
| Internal | `[[Target|text]]` | Evidence only (Q1) |

A line inside `<ref>` tags is not scanned for resources by default (`lines.in_ref = false`); the links are still recognized and marked `in_ref`, which is what a citation extractor needs.

### 3. URL normalization and match patterns (uses 0026 §1)

**A URL is normalized before it is matched**, by the URL normalizer that sitelinks use ([0026](0026-sitelinks.md) §1): lower-case scheme and host, IDNA host, default port and fragment removed, and a trailing slash removed from a non-root path. For matching only, a set of tracking parameters is also removed (`utm_*`, `fbclid`, `gclid`, `mc_cid`, `mc_eid`), configurable as `lines.strip_params`.

**Identifiers come from URL match patterns on identifier properties.** An identifier property's statements under the role `url-match-pattern` ([0003](0003-statement-ui.md) §7), with an optional qualifier under the role `url-match-replacement`, are its patterns. For Wikidata properties the roles bind to P8966 and P8967, and the patterns arrive with the properties, which are always at least mirrored on demand ([0070](0070-shallow-entity-mirroring.md) §2.1). A tenant's own identifier properties carry statements under the same roles.

- The patterns of every identifier property a source lists in `lines.identifiers`, or of every property designated for `match_key` if it lists none, are compiled into one set. A pattern is PCRE as Wikidata writes it; one that uses lookaround or backreferences, which the matcher does not support, is skipped and reported.
- A URL that one pattern matches yields that property's identifier: the first capture group, or the replacement value with `\1`… substituted. The value is then normalized by the property's normalizer ([0004](0004-identity-clusters-and-equivalence.md) §7).
- A URL that several patterns match yields every identifier, ordered by the source's `lines.identifiers` list and then by pattern length, longest first. They become the subject's match keys in that order ([0071](0071-derived-statements-from-mirrored-pages.md) §4).
- A URL no pattern matches yields its normalized form as the match key on `lines.url_property`, which must be designated for `match_key`. A source with no `url_property` skips such links and reports them, grouped by host, so the hosts that most need a pattern are visible.

### 4. Which links are resources

**Every external link and call subject in a line is a resource, unless excluded.** `lines.ignore_hosts` lists hosts never treated as resources, such as the wiki itself and Wikipedia; `lines.first_link_only` (default `false`) takes only the first. A line with two resources ("… at Ancestry; *Also at: MyHeritage*") yields two subjects, each with the line's context; the two are not related to each other (Q2).

### 5. Line settings and statements

**A line subject gets its identity, its mention and its context as statements.** The `lines` field of an `extraction` source ([0071](0071-derived-statements-from-mirrored-pages.md) §2):

| Setting | Meaning |
|---|---|
| `identifiers` | The identifier properties whose patterns apply, in priority order (§3) |
| `url_property` | The property for unmatched URLs, designated for `match_key` |
| `ignore_hosts`, `first_link_only`, `in_ref`, `strip_params` | §2–4 |
| `mentioned_in` | The property linking a resource to the guide page's subject, such as P1343 described by source |
| `inherit` | Properties whose values on the guide page's subject are copied to each of its line subjects, such as the jurisdiction and record type a `CountrySidebar` gave the page |
| `headings` | A value map ([0072](0072-template-mappings.md) §4) and property: a heading or pseudo-heading in a line's path that the map holds adds that statement, so `'''Tithe Applotment'''` adds the record type it maps to |
| `columns` | For table rows: a column name and a property with a parser of 0072 §3, so the "Years covered" cell becomes start and end times and the "Language" cell a language |

The statements on a line subject are, in order: its match-key identifiers and its label, on creation only ([0071](0071-derived-statements-from-mirrored-pages.md) §4); `mentioned_in` the page subject; the inherited facets; the heading facets; the column facets. A call subject from [0072](0072-template-mappings.md) gets the same line statements beside its own mapping's. Every statement carries the source's reference (0071 §6).

### 6. Evidence

**Each line that yields a resource is a mention** in its derivation's evidence part ([0071](0071-derived-statements-from-mirrored-pages.md) §3): its position, heading path, column names for a row, raw wikitext, plain text, links and the subjects it produced. Lines that yield nothing are not kept. The plain text is what a later analyzer reads for what this ADR does not: years in a bullet line's bold lead, "index & images", "($)".

### 7. Crates (amends 0005 §2)

| Crate | Change |
|---|---|
| `scatter-wikitext` | The scan API's lines: list items with depth and parent, table rows with header cells and column names, headings and pseudo-headings, external links, bare URLs, `in_ref` |
| `scatter-normalize` | Removing configured query parameters, for matching only |
| `scatter-extract` | Line rules; the compiled URL match-pattern set, with PCRE translation and the unsupported-pattern report; identifiers from URLs; heading and column facets |
| `triplespace-extraction` | The `lines` extractor; loading patterns from properties under the two roles; the unmatched-host report |

`scatter-extract` takes patterns as data, so a caller outside Triplespace can load Wikidata's patterns for DOI, ISBN, PubMed and the rest and map citation URLs to identifiers with the same code.

### 8. The first source, for illustration

```toml
[extraction.familysearch.lines]
identifiers  = ["P9001", "P9002", "P9020", "P9021", "P9022"]  # FamilySearch collection and catalog IDs; Ancestry, MyHeritage and Findmypast collection IDs
url_property = "P9030"                                   # URL, designated for match_key
ignore_hosts = ["www.familysearch.org/en/wiki", "en.wikipedia.org"]
mentioned_in = "WDP1343"
inherit      = ["P9011", "P9010"]
headings     = { map = "record-types", property = "P9010" }
columns      = { "Years covered" = { property = ["WDP580", "WDP582"], parser = "year-range" }, "Language" = { property = "WDP407", parser = "item", map = "languages" } }
```

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
