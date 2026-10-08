# 0072. Template mappings

- **Status:** Proposed
- **Date:** 2026-10-07
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0015](0015-record-format-and-partition-registry.md), [0038](0038-page-metadata-and-categories.md)
- **Uses:** [0004](0004-identity-clusters-and-equivalence.md), [0042](0042-template-expansion-and-parsoid.md), [0051](0051-page-redirects.md), [0053](0053-mirrored-pages.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [0071](0071-derived-statements-from-mirrored-pages.md), [0073](0073-lines-links-and-url-patterns.md)

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

**A template call is recognized from the parse tree, by name and parameters, and never expanded.** `scatter-wikitext`'s parser already yields template nodes; the scanner of §7 reads them:

- **What is a call.** `{{Name|…}}` at any nesting depth, inside or outside tables, lists and `<ref>` tags. Parser functions (`{{#if:…}}`), magic words and variables (`{{PAGENAME}}`) are not calls, by the registry of [0042](0042-template-expansion-and-parsoid.md) §5. Text inside `<nowiki>`, `<pre>`, `<syntaxhighlight>`, `<math>` and comments is not scanned. A call inside `<ref>…</ref>` is recognized and marked `in_ref`.
- **Names** are normalized as MediaWiki normalizes titles: a leading `Template:` or the repository's localized namespace name is removed, underscores become spaces, runs of spaces collapse, and the first letter is upper-cased where the repository's namespace is `first-letter`. `subst:`, `safesubst:` and `msgnw:` prefixes are removed. A name that is itself a template call (`{{ {{X}} }}`) is not recognized.
- **Redirects.** A mapping names a template's canonical title. A call to a redirect to it matches, through the repository's mirrored Template pages and their redirect line ([0051](0051-page-redirects.md) §1), so the source should mirror namespace 10; a mapping may also list `aliases`.
- **Parameters** follow MediaWiki: positional parameters are numbered from 1; a named parameter's name and value are trimmed of surrounding whitespace, a positional value is not; when a parameter is given twice the last wins, and the duplicate is reported. Each value is kept as raw wikitext and as **plain text**, in which `[[Target|text]]` becomes `text`, `[url text]` becomes `text`, bold and italic marks are removed, and a nested call stays as its raw wikitext unless the value's parser reads it (§3).
- **Unbalanced braces** end the call at the end of the line it opened on, and the call is reported as malformed with its line, so one broken call does not swallow the rest of the page.

This settles 0038 Q3: recognition of template calls by name and parameters feeds mappings, here through derived statements on items ([0071](0071-derived-statements-from-mirrored-pages.md) §1). 0038's page statements and category mappings are unchanged, and template expansion ([0042](0042-template-expansion-and-parsoid.md)) is neither needed nor used.

### 2. The `template-mapping` config kind (extends 0015 §3)

**A mapping turns a template's calls into statements about a subject.** Mappings are tenant configuration, `config` records of kind `template-mapping` whose code is the mapping's name:

| Field | Meaning |
|---|---|
| `template`, `aliases` | The template's canonical title, and other names that count as it |
| `when` | Optional: conditions on parameters, `{ param = "Type", equals = "Topic" }`, `present`, `absent` or `matches` a pattern; all must hold |
| `subject` | `page`: the statements are about the page's own subject ([0071](0071-derived-statements-from-mirrored-pages.md) §2, `page_subject`). `call`: each call is a subject of its own, found or created by `match` |
| `match` | For `subject = call`: the match keys, in order, each `{ property, param, parser }` (§3) |
| `label` | For `subject = call`: the parameter, or a pattern over parameters (`"{2}"`), that labels a created item |
| `statements` | A list of `{ property, param or value, parser, qualifiers }`: a parameter read through a parser, or a constant value, with optional qualifiers built the same way |
| `create` | For `subject = call`: whether a subject not found may be created (default `true`) |

**One call may match several mappings**, and each writes its statements; two mappings for one template with different `when` conditions are how a template with modes is mapped.

### 3. Parsers

**A parser turns a parameter's value into a Wikibase data value, or into nothing.** A value a parser cannot read yields no statement and is reported, per mapping and parameter, with examples (§5).

| Parser | Reads | Yields |
|---|---|---|
| `string` | Plain text | A string |
| `external-id` | Plain text | An external identifier, normalized by the property's normalizer ([0004](0004-identity-clusters-and-equivalence.md) §7) |
| `url` | A URL, or the first external link in the value | A URL, normalized as [0073](0073-lines-links-and-url-patterns.md) §3 normalizes |
| `item` | Plain text, looked up in a value map (§4) named by `map` | An item |
| `wikilink` | The first internal link in the value, or the whole value as a title | The subject of that page, through the source's `page_subject` match key; otherwise the item paired with the title by a sitelink; otherwise nothing |
| `year`, `date` | `1850`, `1850-04-12`, `12 April 1850` and the repository's content-language month names | A time value of precision 9 or 11 |
| `year-range` | `1850-1885`, `1850–1885`, `1850 to 1885` | Two values, written to the `start` and `end` properties the mapping names, or as `start time`/`end time` qualifiers |
| `monolingual` | Plain text | Monolingual text in the source's `language` |

Any parser may take `split = ","` (or `";"`, `"<br>"`), which splits the value and parses each piece, and `trim` patterns to remove from the text before parsing.

### 4. Value maps (extends 0015 §3)

**A value map turns the strings a wiki uses into items.** It is a `config` record of kind `value-map`, whose code is the map's name, holding entries `"string" = value`. Lookup is on the plain text, case-folded, with whitespace collapsed and a trailing full stop removed; no other fuzziness. The FamilySearch link text "Probate", "Probate Records" and "Probate records" are three entries for one item.

**Unmapped strings are collected, not guessed.** Each extraction run records every string a map did not hold, with its count and three example pages, served at `GET /extraction/{source}/unmapped` ([0071](0071-derived-statements-from-mirrored-pages.md) §13). Curating the map is how coverage grows; a change to a map re-runs the sources that use it ([0071](0071-derived-statements-from-mirrored-pages.md) §9).

### 5. Reports

Each run reports, per mapping: calls matched, statements written, values a parser could not read, unmapped strings, malformed calls and duplicate parameters, each with examples. They are on the job's page and in `GET /extraction/{source}`.

### 6. Drafting mappings from TemplateData and Cargo

**A mapping can be drafted from what the wiki already declares about a template, and is then reviewed by a person.** `triplespace-cli extraction draft --source {source} --template {title}` reads the mirrored Template page:

- **TemplateData** ([0055](0055-templatestyles-templatedata-and-page-properties.md) §5): parameter names, aliases, descriptions and types; `url` suggests the `url` parser, `date` the `date` parser, `wiki-page-name` the `wikilink` parser.
- **Cargo**: a `{{#cargo_declare:…}}` in the template gives field names and types, and its `{{#cargo_store:…}}` which parameter feeds which field; `URL`, `Date`, `Page` and `List (,) of …` suggest `url`, `date`, `wikilink` and `split`.
- **Usage**: the parameters actually used across the source's pages, with counts and sample values, from a dry run of the scanner.

The draft is TOML with every parameter listed and the property left blank where nothing suggests one. It is never applied automatically.

### 7. The scanner and the extractor are separate crates (amends 0005 §2)

**Recognizing wikitext structure and mapping it to statements are separate, and neither depends on Triplespace.** This is what lets the Wikipedia Citations Database use them later:

- **`scatter-wikitext`** gains a **scan** API over its parse tree: template calls with normalized names, parameters (raw and plain), `in_ref` and source positions; and, for [0073](0073-lines-links-and-url-patterns.md), lines, table rows, headings and links. It needs no repository, no network and no log; a redirect map and the namespace case rule are passed in. It builds for `wasm32`, as it does already.
- **`scatter-extract`** (new) evaluates mappings, parsers and value maps over scanned calls, and produces subjects with match keys and statements in `scatter-wikibase-model`'s types. It is pure, builds for `wasm32`, and takes its configuration as data, so a caller outside Triplespace (a WCD pipeline mapping `{{cite web|doi=…}}` to a DOI) can use it with its own mappings and no Triplespace types beyond the Wikibase model.

| Crate | Change |
|---|---|
| `scatter-wikitext` | The scan API: template calls, parameters, positions, `in_ref`; name normalization with a supplied redirect map |
| `scatter-extract` | **New.** Template mappings, `when`, parsers, value maps, split and trim; the result type of subjects, match keys, statements and per-mapping reports. Pure; builds for `wasm32` |
| `triplespace-extraction` | The `templates` extractor over `scatter-extract`; the `template-mapping` and `value-map` config kinds; unmapped-string collection |
| `triplespace-cli` | `extraction draft` |

### 8. The first mappings, for illustration

With illustrative local IDs (P9001 FamilySearch collection ID, P9002 FamilySearch Catalog ID, P9003 FamilySearch Library call number, P9010 record type, P9011 covers jurisdiction):

```toml
[mapping.countrysidebar]
template   = "CountrySidebar"
when       = [ { param = "Type", equals = "Topic" } ]
subject    = "page"
statements = [
  { property = "P9011", param = "Country", parser = "item", map = "jurisdictions" },
  { property = "P9010", param = "Records", parser = "item", map = "record-types" },
]

[mapping.recordsearch]
template = "RecordSearch"
subject  = "call"
match    = [ { property = "P9001", param = "1", parser = "external-id" } ]
label    = "{2}"
statements = [ { property = "WDP123", value = { entity = "Q9100" } } ]   # publisher: FamilySearch (illustrative item)

[mapping.fsc-catalog]
template = "FSC"
when     = [ { param = "2", matches = "^(item|title-id)$" } ]
subject  = "call"
match    = [ { property = "P9002", param = "1", parser = "external-id" } ]
label    = "{disp}"

[mapping.fsc-callnumber]
template = "FSC"
when     = [ { param = "2", absent = true } ]
subject  = "call"
match    = [ { property = "P9003", param = "1", parser = "external-id" } ]
label    = "{disp}"
```

A `RecordSearch` call in a resource line is the line's resource; [0073](0073-lines-links-and-url-patterns.md) §5 adds the line's context to it.

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
