# 0085. Settings and preferences: registries, resolution and pages

- **Status:** Proposed
- **Date:** 2026-10-17
- **Updated:** 2026-10-30 (A1)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0021](0021-notifications.md), [0027](0027-preferences-and-portability.md), [0028](0028-tenancy-policy.md), [0034](0034-frontend-stack.md), [0039](0039-files-and-media.md), [0040](0040-instance-prerogatives.md), [0046](0046-primary-tenant.md), [0047](0047-special-pages.md), [0056](0056-security-model.md), [0057](0057-web-tier.md), [0067](0067-proposals.md)
- **Uses:** [0014](0014-caches-and-search.md)

## Context

An instance and each of its tenants are configured by `site` records in their `config` partitions ([0015](0015-record-format-and-partition-registry.md) §3), and an account by preferences in `private.preference` ([0027](0027-preferences-and-portability.md) §1). Both had grown without an interface.

- **The settings catalogue was prose.** Chapter [23](../architecture/23-configuration-and-registry.md) §3 listed about 130 tenant `site` settings and a dozen instance ones in hand-written tables, with a Bound column since the review of 2026-10-09 ([0015](0015-record-format-and-partition-registry.md) A57). Value types were descriptions ("a collation", "schedule"), and nothing machine-readable said what a setting accepts. Preference keys, by contrast, had a registry file.
- **Configuration was written only by the CLI.** `ts-config` governed every `site` record, and `triplespace-cli` was the only writer; no API route and no page wrote one. Chapter [09](../architecture/09-security-and-moderation.md) §4.7 already referred to "the settings page", which nothing defined.
- **Instance defaults stopped at creation.** Under `config.template = defaults` the instance's `template:site:{key}` records were copied into a tenant when it was created ([0028](0028-tenancy-policy.md) §8). Changing an instance default afterwards reached no existing tenant unless the key was locked.
- **The wiki had no name of its own.** No setting held a site name, a content language or a logo; the Action API reported the tenant slug as `general.sitename`.
- **Preferences were a section.** `Special:Preferences` redirected to a Preferences section of `Special:Account` ([0010](0010-site-ui.md) §11; [0027](0027-preferences-and-portability.md) §7), and the tenancy policy and a tenant's visibility had a page of their own, `Special:Tenancy` ([0028](0028-tenancy-policy.md) §11; [0056](0056-security-model.md) §13).
- **Readers who are not logged in had no preferences** ([0027](0027-preferences-and-portability.md) Q4).

The prompt was theming. A palette, a type set, a colour mode and text size are choices an instance, a tenant and a reader each make, and a reader who is not logged in needs them as much as anyone. That needs an interface for preferences, one for settings, and a rule for how the three levels combine, which theming then uses rather than defines.

James compared the problem with ManageWiki and CreateWiki on Miraheze, whose pain is that they cross operational boundaries: each wiki is a separate SQL database with its own connection, and MediaWiki's settings live in a PHP file executed as code. Here creating a tenant is the insertion of records, and a setting is a validated record in a log, so a settings page is one more writer of records that already exist.

### Direction

James's direction, from the design discussion of 2026-10-17:

- **"I feel like there is broadly a general need for the ability of instance administrators and individual tenants to apply settings relevant to their domain, and appearance settings is just one of those reasons."**
- **"My idea is that appearance settings is from an easy-to-access button on any page, and if you are logged in, the settings persist to your account, and if you are logged out, it's stored in the user's browser."** The edge cases proposed with it (what happens at login and logout, without JavaScript, and on a farm): **"Your ideas for edge cases sound reasonable."**
- **"I think we avoid many of the main downsides because we treat tenant instantiation as data insertion rather than a change outside of the normal operational boundary."**
- **"I am guessing we have Special:Preferences for individual preferences, a separate thing for appearance settings, and then something like Special:WikiSettings or Special:InstanceSettings for tenant and instance config respectively."**
- **"I would do the settings/preferences ADR first and then theming."**
- On the logo of a private wiki: **"This is actually a pretty common problem on Miraheze. Miraheze allows wikis that are not open to the public, and therefore their files have special auth requirements, but then their logo is invisible to the public unless they upload it to a public wiki like Miraheze Commons. The system should probably have an affordance for a class of files that are necessarily public because they implicate basic site function, like the site logo or font files."** And on the shape proposed for it: **"It should be reference-driven as originally proposed. SVG should be allowed for logos since there is an expectation that users with ts-config are trusted. However, before Triplespace is used for untrusted tenants, SVG sanitation needs to be added. Write the general mechanism into 0085."**
- On the languages a reader is served in: **"Interface language and label languages I feel should be served depending on the user, either through content negotiation or some kind of setting. Otherwise, there is a farm default and a tenant default."**
- On renaming a wiki: **"Whether an old project name is retained as a redirect for the Project namespace depends on whether the project name was used as the name for the Project namespace to begin with. If the namespace was always called Project and nothing else, there is no reason to make that namespace redirect."**
- Confirmed: a site asset is pinned to a version, and a new upload changes the logo only once that version is pinned; the Defaults tab is absent under `config.template = none`; `Special:Preferences` is usable only with `editmyoptions`; the resolver and validator live in `triplespace-accounts`; the instance config kind `setting` is new.
- On whether a reader's saved choices follow their farm account: **"Preferences follow throughout the farm account."** Then, for the preferences that depend on a wiki's catalogue: **"Palette and type set are per-wiki unless the user specifies they want it to follow across all wikis."**
- On the nine choices put to him (one ADR; `settings.toml` as the catalogue of record with the chapter tables generated; instance defaults read live; `Special:Preferences` as its own page; `Special:Tenancy` merged into the settings pages; settings readable by everyone and written with `ts-config`; one change set per save with each value's origin shown; site identity settings; the API): **"I agree with your choices. You may proceed."**

## Decision

### 1. Two registries of record (amends 0015 §3 and §5; amends 0027 §1)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.4; [07](../architecture/07-actors-and-accounts.md) §8.1; [23](../architecture/23-configuration-and-registry.md) §1.3, §2.2, §3.1, §3.2, §3.3, §3.7, §4.1, §6.*

### 2. Resolving a value (amends 0028 §8, 0027 §1, 0040 §6, 0012 §4 and 0034 §9; extends 0015 §3 and 0057 §6)

*Current text: [23](../architecture/23-configuration-and-registry.md) §3.1.*

#### 2.1 Settings

*Current text: [08](../architecture/08-tenants-and-instances.md) §5.3, §8.6; [18](../architecture/18-api.md) §2.2; [23](../architecture/23-configuration-and-registry.md) §1.4.*

#### 2.2 Preferences

*Current text: [03](../architecture/03-storage-caches-and-search.md) §10.3; [07](../architecture/07-actors-and-accounts.md) §8.1; [19](../architecture/19-site-ui.md) §5.6; [23](../architecture/23-configuration-and-registry.md) §3.7.*

#### 2.3 What a tenant carries (extends 0018 §10)

*Current text: [08](../architecture/08-tenants-and-instances.md) §6.2.*

### 3. Writing settings (extends 0012 §5; extends 0016 §2; extends 0011 §6.1)

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3; [18](../architecture/18-api.md) §3.2; [23](../architecture/23-configuration-and-registry.md) §5.2.*

### 4. The settings pages (amends 0028 §11; amends 0046 §9; amends 0056 §3, §4 and §13; extends 0047 §1; amends 0047 §3)

*Current text: [09](../architecture/09-security-and-moderation.md) §4.5, §4.7; [18](../architecture/18-api.md) §1.2; [19](../architecture/19-site-ui.md) §2.8, §6.3, §6.11; [21](../architecture/21-special-pages.md) §1.3, §8.*

### 5. Preferences and the account page (amends 0010 §11; amends 0027 §7; amends 0021 §6; amends 0047 §2; amends 0067 §6)

*Current text: [17](../architecture/17-federation-and-publication.md) §3.2; [19](../architecture/19-site-ui.md) §2.4, §2.6; [21](../architecture/21-special-pages.md) §1.2, §8.*

### 6. The Appearance menu (extends 0010 §2; settles 0027 Q4)

*Current text: [19](../architecture/19-site-ui.md) §1.2, §2.7.*

### 7. The wiki's name, language, logo and icon (extends 0015 §3; amends 0012 §4; extends 0012 §5; amends 0008 §7)

*Current text: [10](../architecture/10-pages-and-content-models.md) §3.5; [18](../architecture/18-api.md) §2.2, §3.2; [23](../architecture/23-configuration-and-registry.md) §2.2, §3.2, §6.*

### 8. Site assets (amends 0039 §7)

*Current text: [12](../architecture/12-files-and-media.md) §4.1, §4.2, §4.5.*

### 9. Crates (extends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **Keeping instance defaults as provisions.** A copy written at creation never reaches the tenant again, so an operator who changes a default changes it only for wikis created afterwards, and "the tenant never chose this" and "the tenant chose this value" become the same record. Rejected for evaluation, which [08](../architecture/08-tenants-and-instances.md) §8.1 already prefers wherever it can do the job. Locks stay written, because a prerogative has to be in the guest's log to bind it.
- **A per-file public flag, or asset bytes kept in configuration.** A flag outlives the reason for it, and widens access, which [0056](0056-security-model.md)'s rules otherwise never do; bytes in configuration, public by nature, lose the file page, its history and its licence record, which fonts need.
- **One settings page whose scope follows the base.** On a single-tenant instance the farm base is the tenant's base, so one name would have two meanings on one host.
- **Settings as code**, as `LocalSettings.php` and ManageWiki have them. Rejected, as James put it, because it crosses the operational boundary that records keep.
- **A cookie for anonymous appearance.** It would make every anonymous page vary by cookie and split the public cache by every combination of choices.
- **Hand-written chapter tables beside a registry.** Two copies drift; one generated from the other cannot.

## Consequences

- **Every `site` setting has an interface** without code of its own: a setting an ADR adds is a registry entry, and the pages, the API, the CLI and the chapter table follow from it.
- **An operator's change to a default reaches every tenant that has not set its own**, at once. The Defaults tab says how many tenants that is before it saves.
- **A tenant's inherited values do not travel with it** (§2.3).
- **`siteinfo` depends on the instance's configuration as well as the tenant's**, so an instance setting change invalidates every tenant's cached `siteinfo`.
- **Editing the chapter 23 settings tables by hand fails CI**; the registry is edited instead.
- **A reader's appearance choices never reach the server** unless they log in, and cost one cached script request.
- **A private wiki's logo, icon and typefaces are public**, as site assets: exactly the versions a trusted setter named, for as long as a setting names them. Miraheze's workaround of uploading a private wiki's logo to a public wiki is unnecessary.
- **SVG assets rest on trusting `ts-config`** until a sanitizer exists (Q5).

## Open questions

- **Q1. A view of overrides across tenants**: which tenants set their own value for a setting the instance defaults, as a report at the farm base.
- **Q2. Changing `site.language` on a wiki with content**: whether anything that depends on it, such as category collation or the Project namespace's localized name, should be refreshed by a job.
- **Q3. MediaWiki's preference anchors** (`#mw-prefsection-rendering` and the like): whether `Special:Preferences` should honour them as aliases of its tabs.
- **Q4. Ceilings without a switch.** Ten ceiling-bound settings rely on a tenancy switch of their own name that `tenancy.toml` does not register yet, and the order of `wikitext.expansion` and `wikitext.lua` against `wikitext.ceiling` is implicit. Both predate this ADR and need registering before the validator can check them.
- **Q5. SVG sanitization.** Site assets accept SVG from `ts-config` holders, who are trusted (§8). Before Triplespace hosts tenants whose `ts-config` holders the operator does not vet, SVG assets need a sanitizer, an allow-list of elements and attributes as `scatter-css` is for CSS, or the instance must refuse them; which, and whether a tenancy switch enforces it, is open.

## References

- MediaWiki: `Special:Preferences`, `$wgDefaultUserOptions`, `$wgSitename`, `$wgLanguageCode`, `$wgLogos`, `$wgNamespaceAliases`.
- Miraheze's ManageWiki and CreateWiki extensions, the precedent for per-wiki settings on a farm.
- Vector 2022's client preferences and Appearance menu, the precedent for appearance choices kept in the browser for readers who are not logged in.

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§9
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [10](../architecture/10-pages-and-content-models.md), [12](../architecture/12-files-and-media.md), [16](../architecture/16-logs-feeds-and-notifications.md), [17](../architecture/17-federation-and-publication.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
