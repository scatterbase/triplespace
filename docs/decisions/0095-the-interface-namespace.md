# 0095. The Interface namespace: messages and names

- **Status:** Proposed
- **Date:** 2026-10-28
- **Updated:** 2026-10-29 (A2)
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

**Namespace 8 holds pages, shown as Interface.** It keeps MediaWiki's number and canonical name, `MediaWiki`, which `meta=siteinfo` reports as `canonical` and every client reads; its local name on every wiki is **`Interface`**, which the registry gives (`name` in `namespaces.toml`) and a tenant's `namespace` record may change, as for any namespace. A title is read with either name, so `MediaWiki:X` reaches `Interface:X`, as James asked. Its talk namespace, 9, is `Interface talk` (canonical `MediaWiki talk`), of composite talk pages like every other ([0019](0019-discussions.md) §2).

**Its pages are plain text**, of the model `text` alone, with subpages and first-letter titles. A page in it is either a message's override (§2) or a name (§3). Site styles stay one designated Project page ([0055](0055-templatestyles-templatedata-and-page-properties.md) §4): no `Interface:Common.css` or script page is admitted, since the model is `text` only. An adopted wiki's `MediaWiki:` pages come with it as Interface pages (§7).

### 2. Interface pages override messages (amends 0042 §5; amends 0043 §5; amends 0055 §4)

*Changed by A1.*

**`Interface:{Key}` overrides the message `{key}`** for the wiki, as MediaWiki's namespace does: the title is the key with its first letter folded to upper case, so `Interface:Ts-new` overrides `ts-new` and `Interface:Mainpage` overrides MediaWiki core's `mainpage`. **`Interface:{Key}/{lang}`** overrides it in one language; the base page is the wiki's own language. For a reader's language, each language along its fallback chain is tried in turn, the wiki's page for that language (or the base page, where it is the wiki's language) before the shipped message, so a German override applies to a reader of German and never shows English to them.

**Which messages:** the site's interface messages (`i18n/`), and MediaWiki core's messages as `{{int:}}` and Scribunto's `mw.message` read them, which 0042 §5 and 0043 §5 left unoverridable. A page for a key no message has is unused by the site, but `{{int:}}` and `meta=allmessages` (§8) read it, so a wiki may write messages of its own. **`{{int:key|…}}` reads the wiki's language**, its base page or its page for that language, not the reader's, since a page renders once for every reader; its `$1`… are the call's arguments. The build ships none of MediaWiki core's messages, so `{{int:}}` of a key the wiki does not override stays a chip.

**An override is plain message text**, with `$1`, `{{PLURAL:}}` and the rest as a shipped message has them. The site shows it as it shows any message, escaped, and does not take an override holding `<`, `>` or `"`, so that no page's text can become markup wherever a message reaches the page.

**The instance's pages apply to every wiki**: the primary tenant's Interface pages ([0046](0046-primary-tenant.md)) are the instance's overrides, and a wiki's own page for the same key and language replaces the instance's, local pages taking precedence.

### 3. Names for IDs, and the Server Operator (extends 0010 §7)

**`Interface:Name/{id}` names an ID**: an actor key (`{issuer}:{subject}`) or any other identifier the site shows, such as `Interface:Name/instance:gpuldqlgzv33g67ibpcbqfuygk`. Its text is the name. Unlike a message's, the base page names the ID in every language; `Interface:Name/{id}/{lang}` names it in one. A name is what the site shows; the ID stays what every record, API answer and link carries, and is the name's tooltip.

**Where the site shows an account** in a list of changes, as a row's target or its actor:

- **The instance's own actor key**, `instance:{farm code}`, is a **Server Operator** badge (the message `ts-server-operator`, which an Interface page may itself override), with the key as its tooltip.
- **An account as a target is marked as an account**, not its user page: a person icon, with *Account* as its accessible text, before the name, which links to the account's contributions.
- **The name** is the account's username where the row gives one, else the wiki's name for its key, with the key as its tooltip, else the key.
- **The key is not shown for an account of this wiki** (its tenant's issuer code, or `local`): the username is enough. An account of another wiki that the wiki has not named shows its key beside its name.

**Entities keep their IDs** beside their labels, as James asked, so `The Supreme Court from Taft to Warren. Q100001` reads as an entity and not a page of the main namespace.

### 4. Who edits them (extends 0016 §2)

**MediaWiki's `editinterface` right** is needed to create, edit or move a page into or out of the Interface namespace, besides the rights any edit or move takes. `sysop` holds it, as in MediaWiki, and `owner`, which includes every group, through it.

### 5. Serving (extends 0012 §4)

**`siprop=triplespace` reports `interface`**: `{messages: {key: {language: text}}}`, the base page under `""`, the instance's merged under the wiki's. It is part of `meta=siteinfo`, which the site reads for every page, so a change to an Interface page reaches the next page, and the response's validator moves with it.

**Only pages anyone may read are overrides**: a page under a read restriction, a deleted page or a redirect is left out, since what an override says is shown to every reader. A private wiki's outsiders, whom `meta=siteinfo` gives only its name, logo and theme, are given none. **Bounds**: at most 1,000 Interface pages a wiki, each at most 2,000 characters; a page beyond either is not an override. They are sensible defaults against surprises.

### 6. Crates (extends 0005 §2)

*Changed by A1.*

| Crate | Change |
|---|---|
| `scatter-titles` | The registry's `name` for a namespace; `INTERFACE` |
| `triplespace-projections` | The registry's local names in the wikitext configuration; a wiki's Interface pages anyone may read |
| `triplespace-api-action` | `editinterface` on edits and moves; `siprop=triplespace` `interface` |
| `triplespace-client` | The tenant's issuer code, the farm code and the overrides from `meta=siteinfo` |
| `triplespace-ui` | Overrides along the language chain; names; the account mark and the Server Operator badge in lists of changes |

`scatter-wikitext-expand` expands `{{int:}}` through its host, which `triplespace-api-action` answers from the wiki's Interface pages; `triplespace-cli` adopts the MediaWiki namespace (§7). `mw.message` reads the overrides once Lua is built (0043).

### 7. Adopting a wiki brings its MediaWiki namespace (settles Q1; extends 0008 §9)

*Changed by A1.*

**An adopted wiki's MediaWiki-namespace pages come with it**, with their history, as Interface pages, as James decided on 2026-10-28. A message's text is its source whether the source wiki stored it as `wikitext` or `text`, so each revision comes as `text`, the namespace's model. **A page the instance cannot use is ignored**: one of any other model, such as `MediaWiki:Common.css`, `MediaWiki:Common.js` or a JSON message, is not brought, and the adoption reports how many it ignored, apart from the pages it defers. A page that comes is an override like any other: it changes the site where its key is one of the site's messages, and serves `{{int:}}` and `meta=allmessages` otherwise. Its talk page (namespace 9) is converted to threads as every talk page is ([0090](0090-converting-talk-pages-comment-by-comment.md) §10).

### 8. `meta=allmessages` (settles Q2; amends 0042 §5, §8.1, §14)

*Changed by A1, A2.*

**`meta=allmessages`** answers as MediaWiki's does, with `ammessages` (keys, or `*`), `amlang`, `amcustomised` (`all`, `modified`, `unmodified`), `amprefix`, `amfilter`, `amfrom`, `amto`, `amargs`, `amnocontent` and `amincludelocal`. Each message is the wiki's override in the language asked (the wiki's own by default) where there is one, marked `customised`, else the message the build ships, else `missing`. The messages the build ships are the site's own, in English; with `*` and `amincludelocal`, keys the wiki alone has are listed too, its names of IDs among them. **`amprop=default`** adds each message's shipped text as `default`, or `defaultmissing` where the build ships none, as MediaWiki gives it, so that `Special:AllMessages` ([0047](0047-special-pages.md) §9) shows a customised message's default beside the wiki's text (A2).

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

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §6 | extends | 0005 A108 |
| [0008](0008-namespaces-and-document-pages.md) §2 | §1 | amends | 0008 A44 |
| [0008](0008-namespaces-and-document-pages.md) §9 | §7 (A1) | extends | 0008 A45 |
| [0010](0010-site-ui.md) §7 | §3 | extends | 0010 A46 |
| [0012](0012-api-requirements.md) §4 | §5 | extends | 0012 A81 |
| [0016](0016-permissions-and-access-control.md) §2 | §4 | extends | 0016 A39 |
| [0042](0042-template-expansion-and-parsoid.md) §5 | §2 | amends | 0042 A20 |
| [0042](0042-template-expansion-and-parsoid.md) §5 | §8 (A1) | amends | 0042 A21 |
| [0042](0042-template-expansion-and-parsoid.md) §8.1 | §8 (A1) | amends | 0042 A21 |
| [0042](0042-template-expansion-and-parsoid.md) §14 | §8 (A1) | amends | 0042 A21 |
| [0043](0043-lua-modules.md) §5 | §2 | amends | 0043 A9 |
| [0055](0055-templatestyles-templatedata-and-page-properties.md) §4 | §2 | amends | 0055 A6 |

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
