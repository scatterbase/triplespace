# 0072. Template mappings

- **Status:** Proposed
- **Date:** 2026-10-07
- **Updated:** 2026-10-09 (A1)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0015](0015-record-format-and-partition-registry.md), [0038](0038-page-metadata-and-categories.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0042](0042-template-expansion-and-parsoid.md), [0051](0051-page-redirects.md), [0053](0053-mirrored-pages.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [0071](0071-derived-statements-from-mirrored-pages.md), [0073](0073-lines-links-and-url-patterns.md)
- **Chapters:** [13](../architecture/13-mirrored-pages.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md)

## Context

[0038](0038-page-metadata-and-categories.md) chose categories as the first source of statements inferred from wikitext and set templates aside: its Context called inference from template calls premature, and its Q3 asked whether "a narrow, non-parsing recognition of template calls (name and parameters only) should ever feed mappings."

A wiki that uses templates as structure answers Q3. On the FamilySearch Research Wiki, every guide page opens with a call such as

```
{{CountrySidebar
|Country=Ireland
|Name=Ireland
|Type=Topic
|Topic Type=Records
|Records=Land and Property
|Content=Standardized
}}
```

and its resource lines name collections by template: `{{RecordSearch|1804886|Ireland Tithe Applotment Books, 1814-1855}}` is FamilySearch historical-records collection 1804886, and `{{FSC|185720|item|disp=…}}` is a FamilySearch Catalog entry. The parameters are already the data; nothing needs to be expanded to read them.

The extraction framework of [0071](0071-derived-statements-from-mirrored-pages.md) runs extractors over a page and writes what they derive. This ADR is the first extractor: it maps template calls to statements.

### Direction

James's direction, from the design discussion of 2026-10-07:

- **"Mapping template names/parameters/values"** is half of the simplified scope, because it is more generalizable than page-specific parsing.
- **The crates developed for this ADR and [0073](0073-lines-links-and-url-patterns.md) may later be used for the Wikipedia Citations Database**, whose citations are template calls (`{{cite web}}`, `{{cite book}}`) and links in `<ref>` tags. Not soon, but the crates should not be tied to Triplespace.

## Decision

### 1. Recognizing template calls without expanding them (settles 0038 Q3; uses 0042 §1)

*Current text: [13](../architecture/13-mirrored-pages.md) §7.1.*

### 2. The `template-mapping` config kind (extends 0015 §3)

*Current text: [13](../architecture/13-mirrored-pages.md) §7.2.*

### 3. Parsers

*Current text: [13](../architecture/13-mirrored-pages.md) §7.3.*

### 4. Value maps (extends 0015 §3)

*Current text: [13](../architecture/13-mirrored-pages.md) §7.4.*

### 5. Reports

*Current text: [21](../architecture/21-special-pages.md) §2.6.*

### 6. Drafting mappings from TemplateData and Cargo

*Current text: [13](../architecture/13-mirrored-pages.md) §7.5.*

### 7. The scanner and the extractor are separate crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.3.*

### 8. The first mappings, for illustration

*Current text: [13](../architecture/13-mirrored-pages.md) §7.6.*

## Alternatives considered

- **Expanding templates and reading the output** ([0042](0042-template-expansion-and-parsoid.md)). It would also see what templates compute, but it needs the repository's templates, modules and Lua; the parameters are the data, and the expanded HTML has lost their names.
- **Cargo or Semantic MediaWiki declarations as the only mappings.** Most templates declare neither; where a template does, §6 drafts from the declaration.
- **Mapping in the template mapping's own subject rather than through value maps.** A map shared across mappings and sources keeps one curated vocabulary per wiki.

## Consequences

- **0038 Q3 is settled**: template calls feed derived statements, never page statements.
- **Template-heavy wikis become data sources without expansion**, so a tenant that has not turned expansion on ([0042](0042-template-expansion-and-parsoid.md) §2) can extract from one.
- **Coverage is curated.** Unmapped strings and unreadable values are listed, not guessed, so the vocabulary grows by review.
- **The scanner and extractor are reusable outside Triplespace,** in particular by a WCD pipeline over `{{cite …}}` templates in `<ref>` tags.
- **Test plan.** The Ireland page's `CountrySidebar` yields two statements on the guide's subject; `{{RecordSearch|1804886|…}}` twice on a page yields one subject; a call through a template redirect matches; a named parameter with surrounding spaces and a positional one keep MediaWiki's trimming; an unbalanced call is reported and the next line still scans; an unmapped `Records` value is listed with its pages; `extraction draft` on a template with TemplateData lists its parameters.

## Open questions

- **Q1. Calls whose parameters are themselves calls**, such as a date built by `{{date|…}}`: whether parsers should read a nested call through a mapping of their own.
- **Q2. Positional parameters by TemplateData alias**, so a mapping can say `param = "collection"` for parameter 1.
- **Q3. Value maps from Wikidata**, by label search in the source's language, as suggestions for review.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §7 | amends | 0005 A75 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §2, §4 | extends | 0015 A32 |
| [0038](0038-page-metadata-and-categories.md) Q3 | §1 | settles | 0038 Q3 |

## References

- [Help:Templates](https://www.mediawiki.org/wiki/Help:Templates): positional and named parameters, whitespace trimming, duplicates
- [Extension:TemplateData](https://www.mediawiki.org/wiki/Extension:TemplateData): parameter types
- [Extension:Cargo/Storing data](https://www.mediawiki.org/wiki/Extension:Cargo/Storing_data): `#cargo_declare` field types and `#cargo_store`
- FamilySearch Research Wiki pages "Mexico Colonial Records" and "Ireland Land and Property" (wikitext supplied by James, 2026-10-07)

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [13](../architecture/13-mirrored-pages.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.
