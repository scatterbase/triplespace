# 0095. The Interface namespace: messages and names

- **Status:** Proposed
- **Date:** 2026-10-28
- **Updated:** 2026-10-30 (A4)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0016](0016-permissions-and-access-control.md), [0042](0042-template-expansion-and-parsoid.md), [0043](0043-lua-modules.md), [0055](0055-templatestyles-templatedata-and-page-properties.md)
- **Uses:** [0007](0007-actor-identity.md), [0019](0019-discussions.md), [0046](0046-primary-tenant.md), [0079](0079-derived-issuer-codes.md)

## Context

[0008](0008-namespaces-and-document-pages.md) §2 reserved MediaWiki's namespace 8, so that "no wiki can override a message" ([11](../architecture/11-rendering-templates-and-modules.md) §2.3): `{{int:}}`, Scribunto's `mw.message` and the site's own interface read only the messages the build ships. A wiki could not change a word of its interface, and could not give a name to anything the site shows by its identifier.

The site shows identifiers in many places. Recent changes, read on Librarybase on 2026-10-28, showed a subsidiary account created by Harej as `Harej bot 2 wwquhzyl62htysnyzyxc5h2teu:25`, linked to its user page, so that the account read as a page title with a code beside it; and the wiki's creation as performed by `instance:gpuldqlgzv33g67ibpcbqfuygk`, the instance's own actor key ([0007](0007-actor-identity.md) §1; [0079](0079-derived-issuer-codes.md) §4), a string no reader can make sense of and a word, "instance", most readers do not know.

### Direction

James's direction, from the design discussion of 2026-10-28, on that list:

- **"When the target is a user/actor rather than a particular page, something should clarify that it's that user as opposed to something like their user page. You probably don't need to spell out their user ID if it's a user on the same wiki and their username is sufficient. Where something is attributed to the instance, if the ID is the same as the current instance, there's instead a badge that says "Server Operator". (The end-user might not be familiar with the jargon of "instance".) In general it would be nice to be able to define human-readable names for these ID strings. This could be something we use the MediaWiki namespace for, though instead of MediaWiki it should be called Interface (with "MediaWiki" as a redirect)."**
- **"Please continue to show entity IDs like "The Supreme Court from Taft to Warren. Q100001" to clarify that it's an entity and not a main namespace page."**

Asked what the namespace holds and how an account is marked, James chose (2026-10-28): one page per ID, `Interface:Name/{id}`, with language subpages, edited with `editinterface`, a wiki's page winning over the instance's; that Interface pages also override interface messages, as MediaWiki's do, a name being one family of messages; and a person icon before an account's name, which links to its contributions, with no key for an account of the same wiki.

## Decision

### 1. The Interface namespace (amends 0008 §2)

*Changed by A1.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §1.4.*

### 2. Interface pages override messages (amends 0042 §5; amends 0043 §5; amends 0055 §4)

*Changed by A1, A3.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §2.3, §6.4, §7.5.*

### 3. Names for IDs, and the Server Operator (extends 0010 §7)

*Current text: [19](../architecture/19-site-ui.md) §3.9.*

### 4. Who edits them (extends 0016 §2)

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2.*

### 5. Serving (extends 0012 §4)

*Current text: [18](../architecture/18-api.md) §2.2.*

### 6. Crates (extends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 7. Adopting a wiki brings its MediaWiki namespace (settles Q1; extends 0008 §9)

*Changed by A1.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §6.*

### 8. `meta=allmessages` (settles Q2; amends 0042 §5, §8.1, §14)

*Changed by A1, A2.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §2.3, §3.1; [18](../architecture/18-api.md) §2.3.*

## Alternatives considered

- **One JSON page of names**, `Interface:Names.json`. One page to watch, but one history for every name, and a name could not be protected or translated on its own.
- **Names only**, leaving messages unoverridable. James chose MediaWiki's whole meaning for the namespace, of which a name is one family.
- **Rename the canonical name to Interface.** Clients that read `canonical` (Pywikibot, WikibaseIntegrator) and every link written `MediaWiki:` expect MediaWiki's; a local name changes what readers see and nothing they send.
- **An "Account" chip** before an account's name, like the Log and Job chips. A chip in the target column reads as a kind of row; the icon marks the name itself.

## Consequences

- **A wiki can change any word of its interface**, and name any identifier, without a release; an instance can do both for every wiki at once.
- **What every reader sees can be changed by holders of `editinterface`**, a right MediaWiki administrators already hold; its history is a page history and its protection a page's.
- **`meta=siteinfo` grows with a wiki's overrides**, up to the bounds of §5, and is read for every page.
- **An account no longer reads as a page**, the instance's acts read as the Server Operator's, and keys appear only where a name would be ambiguous.

## Open questions

- **Q1.** ~~**Adopting a wiki's `MediaWiki:` pages**, which are `wikitext`: as overrides, which would also change messages the site uses, or deferred, as now.~~ *Settled by §7: James decided on 2026-10-28 that adoption brings them, and ignores those the instance cannot use.*
- **Q2.** ~~**`meta=allmessages`**, MediaWiki's module for reading messages and overrides (`amcustomised`), which Parsoid and tools call.~~ *Settled by §8: served, over the site's shipped messages.*

## References

- MediaWiki: the MediaWiki namespace, `$wgUseDatabaseMessages`, and the `editinterface` right.

## Amendment log

### A1. Adoption brings the MediaWiki namespace; `{{int:}}` and `meta=allmessages`

- **Date:** 2026-10-28
- **Source:** Direct: James, design discussion of 2026-10-28
- **Change:** amends §1, §2, §6; extends §7, §8
- **Summary:** James asked for the two parts left unbuilt, `{{int:}}` reading overrides and `meta=allmessages`, and decided: "When adopting a wiki, it should bring in the MediaWiki-namespace pages. Those that can't be used by the Triplespace instance are just ignored." An adopted wiki's MediaWiki pages of model `wikitext` or `text` come as Interface pages, each revision as `text`; pages of any other model are ignored and counted (§7). `{{int:key|…}}` expands the wiki's override in the wiki's language, with its arguments, and stays a chip where there is none, the build shipping no MediaWiki core messages (§2). `meta=allmessages` answers from the overrides over the site's shipped English messages (§8). `mw.message` waits on Lua (0043).

Replaced text (§1):

> An adopted wiki's `MediaWiki:` pages are `wikitext`, so adoption still defers them (Q1).

Replaced text (§2):

> **Which messages:** the site's interface messages (`i18n/`), and MediaWiki core's messages as `{{int:}}` and Scribunto's `mw.message` read them, which 0042 §5 and 0043 §5 left unoverridable. A page for a key no message has is unused, except a name (§3).

Replaced text (§6):

> `{{int:}}` and `mw.message` still read the shipped messages alone; their overrides wait on the renderer reading the same report.

### A2. `amprop=default`

- **Date:** 2026-10-29
- **Source:** Direct: James, design discussion of 2026-10-29
- **Change:** extends §8
- **Summary:** `meta=allmessages` takes `amprop=default`, which `action=paraminfo` already listed: each message carries its shipped text as `default`, or `defaultmissing` (`true`, or `""` in format version 1) where the build ships none, whether or not the wiki overrides it, as MediaWiki's module gives it. `Special:AllMessages` reads it to show a customised message's default beside the wiki's text. James asked for §8 to name the parameter once it was built.

### A3. Messages come in bundles

- **Date:** 2026-10-29
- **Source:** [0097](0097-versioning-and-skins.md) §13
- **Change:** extends §2
- **Summary:** The site's messages are the binary's bundle and each skin's, a skin's keys carrying its name as a prefix, so an override names one bundle's key; switching skins leaves the overrides of the old skin's keys in place and inert.

### A4. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [09](../architecture/09-security-and-moderation.md), [10](../architecture/10-pages-and-content-models.md), [11](../architecture/11-rendering-templates-and-modules.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
