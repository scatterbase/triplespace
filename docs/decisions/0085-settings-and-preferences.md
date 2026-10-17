# 0085. Settings and preferences: registries, resolution and pages

- **Status:** Proposed
- **Date:** 2026-10-17
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

**`docs/registry/settings.toml` registers every `site` setting**, a tenant's and the instance's, as `preferences.toml` registers every preference key. Each entry holds:

| Field | Holds |
|---|---|
| `key` | The setting name, never reused |
| `scope` | `tenant` or `instance` |
| `bound` | `tenant`, `ceiling` or `instance`, the Bound of [23](../architecture/23-configuration-and-registry.md) §3.1, with `ceiling` naming the capping switch where it is not of the setting's own name, and `tenant_may_narrow` on an instance setting a tenant may narrow |
| `group` | The tab of the settings page (§4) it is shown on |
| `type`, `choices` | A closed vocabulary of value types: `boolean`, `integer`, `duration` (ISO 8601), `size`, `text`, `string`, `url`, `language`, `language-list`, `choice` with its `choices`, `string-list`, `integer-list`, `map`, `json`, and reference kinds that name something of the tenant's (`page`, `page-list`, `file`, `group`, `actor`, `repository`, `repository-list`, `property-list`, `namespace-list`, `site`, `licence`, `collation`, `pattern`, `schedule`, and [0086](0086-theming.md)'s `palette` and `typeset`), and `site-asset`, a pinned version of a local file the setting uses for a site function (§8) |
| `default` | The registry default as a value of the type; an entry without one is marked `no_default`, and its `default_text` says what applies (a computed value, or none) |
| `media` | For a `site-asset`: the media types it accepts |
| `right` | The permission that writes it, where it is stronger than `ts-config`, or `owner` where only members of `owner` may |
| `reported` | Whether `meta=siteinfo` reports it |
| `values`, `default_text`, `meaning`, `source` | The catalogue's text |

Labels and help are interface messages, `ts-setting-{key}` and `ts-setting-{key}-help`, never registry fields, so they are translated with the rest of the interface.

**The chapter's tables are generated from the file.** `docs/architecture/settings.py` renders [23](../architecture/23-configuration-and-registry.md) §3.2 and §3.3 from `settings.toml`, `--write` rewrites them, and CI fails when they are stale, as it does for `INDEX.md`. A setting a later ADR adds is a registry entry; the table follows.

**`settings.toml` is copied on create and synced like every registry file** ([23](../architecture/23-configuration-and-registry.md) §4.1; [0027](0027-preferences-and-portability.md) A7): `instance create` appends a config record of kind `setting`, keyed `setting:{key}`, for every entry, `registry sync` appends entries newer than the instance's, and an instance may register settings of its own for features it adds. The validator reads `view.registry`, so a setting registered at runtime is validated, resolved and shown like a shipped one.

**`preferences.toml` gains the same shape.** Each key gains a `group` (the tab of `Special:Preferences`, §5), `choice` joins its types with `choices`, and two fields are new:

- **`follows`**: `account`, the default; `farm`; or `choice`. Under farm identity ([0028](0028-tenancy-policy.md) §2) a `farm` key belongs to the farm account, so every linked tenant account reads the same value. A `choice` key belongs to the tenant account unless the reader marks it **Use on every wiki**, which keeps it on the farm account instead; a tenant account's own value is read before the farm account's. Without farm identity every key is the account's. A reader's own choice of interface language, label languages, time zone and date format follows the farm account; everything that depends on a tenant's own catalogue, such as the palette and type set of [0086](0086-theming.md), is `choice`.
- **`browser`**: whether a reader who is not logged in may hold the key in the browser's storage (§6). The appearance keys of [0086](0086-theming.md) §4 are; no key shipped before them is.
- **`negotiated`**: `accept-language` on the interface language and the label languages, whose value, where the reader has not chosen one, comes from the request (§2.2).

Preference labels are the messages `ts-pref-{key}` and `ts-pref-{key}-help`.

### 2. Resolving a value (amends 0028 §8, 0027 §1, 0040 §6, 0012 §4 and 0034 §9; extends 0015 §3 and 0057 §6)

#### 2.1 Settings

**A tenant setting resolves in three steps, first match wins:** the tenant's own `site` record; the instance's default, the `template:site:{key}` record, read when the value is needed; the registry default. A lock and a ceiling then apply:

- **Locked** (`tenancy:locked` names `site:{key}` under `config.template = locks`): the value is the instance's, and a tenant record is refused with `ts-locked`, as [0028](0028-tenancy-policy.md) §8 has it. Locked templates are still written into every tenant as prerogatives ([0040](0040-instance-prerogatives.md) §6); only defaults change.
- **Capped** (`bound = ceiling`): a value above the ceiling is refused when written, the instance's default included, and a value that a lowered ceiling has left above it resolves to the ceiling; for a list, capping is the intersection with the ceiling.

**Instance defaults are evaluated, not provisioned.** `tenant create` no longer writes `site` templates into the new tenant. A tenant without a record of its own reads the instance's default whenever the default changes, which is what not having chosen means; a tenant that has set a value keeps it. Templates of every other kind (namespaces, groups, `sitelink-policy`, `thread-status`, `view-pin`) are still written at creation as provisions. Under `config.template = none` there are no templates, and a tenant setting resolves to the tenant's record or the registry default.

**Only the tenant's own records are the tenant's.** A `site` record in a tenant's `config` that carries an instance attestation ([08](../architecture/08-tenants-and-instances.md) §8.3), which is a locked template's copy or a provision written before this ADR, is not the tenant's value: resolution passes over it. Unlocking a key therefore returns every tenant that never set it to the instance's default, and an old provision no longer masks later defaults.

**A reference-typed default** (a page, a file, a group, a repository) is resolved in each tenant when it is read. Where it names nothing the tenant has, the setting resolves to the registry default, and the settings page marks the instance's default as unresolved there.

**Changing an instance default does for every inheriting tenant what changing the tenant's own record does**: the render-epoch bumps and refresh queues of the `wikitext.*` settings, the jobs a setting such as `entities.mirror` or `query.enabled` starts, and the refresh of `siteinfo`. An instance job (`ts-runjob` at the farm base) fans the change out tenant by tenant. It writes nothing into the tenants' logs, since nothing in them changed.

**An instance setting resolves to the instance's record, then the registry default.** Where `tenant_may_narrow` is set, a tenant record narrows it for that tenant.

**Every resolved value carries its origin**: `registry`, `instance`, `tenant` or `locked`, and `capped` with the ceiling when one applied. The settings pages and `GET /settings` show it (§3, §4).

**`siteinfo` is cached by the instance's registry offset as well as the tenant's**, since a tenant's settings now depend on instance records ([0014](0014-caches-and-search.md) §6; [18](../architecture/18-api.md) §2.2). A change to an instance default therefore refreshes every tenant's `siteinfo`.

#### 2.2 Preferences

**A preference resolves, first match wins:** the account's own value (for a `choice` key, the tenant account's, then the farm account's where the reader chose **Use on every wiki**); for a key marked `negotiated`, the value the request asks for; the tenant's `preferences.defaults`; the instance's default for `preferences.defaults`; the registry default. **The interface language and the label languages are served by the reader**, through their own choice or, without one, through content negotiation: `uselang` overrides everything for one request, and otherwise the languages `Accept-Language` lists, in order of preference, are matched against the languages the interface has messages for (for `language`) or taken as the label languages' order (for `label-languages`). A reader who is not logged in is served this way too. Only when the request names nothing usable do the tenant's and then the farm's defaults apply, and the tenant's default for `language` is `site.language` unless `preferences.defaults` names another. The web tier keys its public cache by the negotiated language, not the raw header, so negotiation does not split the cache by every browser's `Accept-Language`. `preferences.defaults` is a map, and its instance and tenant values merge key by key, the tenant's entries over the instance's. A key with `follows = farm` reads the farm account's value under farm identity (§1).

#### 2.3 What a tenant carries (extends 0018 §10)

A tenant's bundle carries the records it wrote, and so no longer carries values it inherited from its instance. A tenant that moves keeps every value it set and takes the new instance's defaults for the rest. A tenant that wants a value to survive a move sets it.

### 3. Writing settings (extends 0012 §5; extends 0016 §2; extends 0011 §6.1)

**Three REST routes under `triplespace/v0`.** At a tenant base they serve that tenant. With `scope=instance` they serve the instance's settings, and only at the farm base, so that on a single-tenant instance, whose farm base is its tenant's base, the two never share a meaning:

| Route | Purpose |
|---|---|
| `GET /settings/schema` | Readable by anyone who may read the tenant. The registered settings, from `view.registry`: §1's fields, the groups, and the message keys. With `scope=instance`, the instance settings, the tenancy switches of `tenancy.toml`, and every tenant setting with its instance default and lock |
| `GET /settings` | Readable by anyone who may read the tenant. Each setting's resolved value with its origin (§2.1), the record that set it as `{partition, offset}`, whether the viewer may change it, and the `config` offset read, the base of a later write. `keys=` and `group=` narrow it |
| `PATCH /settings` | `{set: {key: value}, unset: [key], summary, base}`; with `scope=instance` also `defaults: {key: value or null}`, `locks: {key: true or false}` and `tenancy: {switch: value}`. All or nothing |

**A save is one change set.** Every record it writes at its base is appended in one transaction, each carrying the summary in its comment part, and projects as one `settings/change` log event naming the keys with their old and new values ([16](../architecture/16-logs-feeds-and-notifications.md) §2.3). `unset` writes the tombstone of the tenant's record, so the setting inherits again. A lock's copies into the tenants follow as an instance job, as [08](../architecture/08-tenants-and-instances.md) §5.3 has it. A save is refused whole when any part fails:

- `ts-unknown-setting`: the key is not registered;
- `ts-invalid-setting`: the value does not fit the type or choices, or a reference names nothing the tenant has;
- `ts-above-ceiling`: the value exceeds the instance's ceiling;
- `ts-locked`: the key is locked;
- `ts-instance-policy`: an instance-bound key at a tenant's scope, except a narrowing of a setting marked `tenant_may_narrow`;
- `permissiondenied`: the actor lacks `ts-config`, or the entry's `right`;
- `editconflict`: a key in the request has a record after `base`. Records for other keys are no conflict.

**One validator serves every writer.** The routes, the settings pages and `triplespace-cli settings get|set|unset`, with `--instance`, `--default` and `--lock` at the farm base, call the same function in `triplespace-accounts`, so the CLI can no longer write a record the API would refuse.

**Rights.** Writing needs `ts-config` at the base, an instance right at the farm base ([0016](0016-permissions-and-access-control.md) §2), or the entry's `right` where it names a stronger one; the identity switches keep `ts-keys`, and `files.separation` keeps `owner`. Reading needs only what reading the tenant needs, since the `config` partition is public and exported ([23](../architecture/23-configuration-and-registry.md) §1.1). A secret is never a `site` setting; secrets are deployment configuration ([23](../architecture/23-configuration-and-registry.md) §3.6).

### 4. The settings pages (amends 0028 §11; amends 0046 §9; amends 0056 §3, §4 and §13; extends 0047 §1; amends 0047 §3)

**`Special:WikiSettings`** (scope `tenant`) shows a tenant's settings, and **`Special:InstanceSettings`** (scope `farm`) the instance's. Both are built from the registry and `GET /settings`, with no code per setting:

- **Tabs** are the registry's groups. Each row shows the label and help, the value with a control drawn from the type, an origin chip (*Default*, *Instance default*, *Set on this wiki*, *Locked by the instance*, *Capped at N*), **Reset** to inherit again, and a link to the setting's history in the log.
- **Anyone who can read the tenant may read them.** A viewer without the right sees the values without controls.
- **Saving** sends one `PATCH /settings` per tab with a summary field. It works without JavaScript as a form posted to the web tier, which answers 303 or re-renders the form with the API's errors ([20](../architecture/20-web-tier.md) §3.1).

`Special:WikiSettings` also has:

- **Visibility**: the tenant's `read` ACL, *Anyone*, *Accounts on this wiki* or *A group…*, with the account-creation switch beside it and the warning [0056](0056-security-model.md) §13 requires before a public tenant goes private. It writes the `tenant` ACL through the ACL routes, with `ts-config`, as before ([09](../architecture/09-security-and-moderation.md) §4.7), not through `PATCH /settings`; under `config.template = locks` the instance may lock it.
- **Tenancy**: read-only, the instance's policy as it applies to this wiki: the preset, the switches, the locked keys and the ceilings; and, for an `owner` of a tenant offered the primary role, **Accept** ([0046](0046-primary-tenant.md) §9).

`Special:InstanceSettings` also has:

- **Tenancy**: the tenancy switches, from `tenancy.toml`.
- **Defaults for wikis**: every tenant setting with its registry default, the instance's default and, under `config.template = locks`, a lock. Changing a default says how many tenants inherit it and so will change. The tab is absent under `config.template = none`.

**`Special:Tenancy` redirects**, to `Special:WikiSettings#tenancy` at a tenant base and to `Special:InstanceSettings#tenancy` at the farm base; its name stays in the special-page registry, unlisted ([0047](0047-special-pages.md) §1). On a single-tenant instance, where the farm base is the primary tenant's base ([08](../architecture/08-tenants-and-instances.md) §1.1), both pages are served on the same host.

### 5. Preferences and the account page (amends 0010 §11; amends 0027 §7; amends 0021 §6; amends 0047 §2; amends 0067 §6)

**`Special:Preferences` is a page of its own** (scope `tenant`), no longer a section alias of `Special:Account`:

- **Tabs** are the groups of `preferences.toml`: Interface, Appearance, History, Watching, Notifications, Discussion and Statements.
- **Each key** is drawn from its type with its label, help and **Reset**; **Restore all defaults** resets every key, as `action=options&reset=1` does. A key that follows the farm account says it applies on every wiki of the farm.
- **The Notifications tab** is the reason-by-channel matrix and the digest, which leave the account page.
- **Saving** is `PATCH /account/preferences`, or a form posted through the web tier without JavaScript. The page needs `editmyoptions`; a reader who is not logged in is offered **Log in** and pointed to the Appearance menu (§6).

**`Special:Account` keeps the account itself**: Name, Sign-in methods, Linked accounts, **Contacts** (the email address and fediverse handle with their verification state, from the old Notifications section), Watchlist token, Subsidiaries, Connected applications (which also lists the upstream grants the instance holds for the account at destination wikis, each with **Revoke**, [0067](0067-proposals.md) §6), Fediverse, Your data and Leave, with a link to `Special:Preferences`.

**Section aliases follow** ([0047](0047-special-pages.md) §2): `DisplayNotificationsConfiguration` goes to `Preferences#notifications`; `ChangeEmail`, `Confirmemail` and `Invalidateemail` to `Account#contacts`; `ResetTokens` to `Account#watchlist-token`.

### 6. The Appearance menu (extends 0010 §2; settles 0027 Q4)

**Every page's global header has an Appearance button**, beside the account menu, for every viewer. It opens a panel of the preference keys of the `appearance` group, as a disclosure that opens without JavaScript.

- **Signed in**, the values are the account's preferences and a change writes them through `action=options`. With JavaScript the change applies at once; without it the panel is a form posted to the web tier, which writes the preference and redirects back ([20](../architecture/20-web-tier.md) §3.1).
- **Not logged in**, the panel offers the keys with `browser = true`, and a change is kept in the browser's `localStorage` under one entry for the tenant's host. A small same-origin script, loaded blocking in `<head>` with an immutable hashed name, applies the stored values before the first paint, which the content security policy permits as `script-src 'self'` ([0034](0034-frontend-stack.md) §11). **No cookie is set**, so an anonymous page stays `public` and its cache key does not vary ([0057](0057-web-tier.md) §6). Storage that is absent or refused leaves the defaults.
- **Without JavaScript and not logged in**, nothing can be kept, and the panel says that the page is following the device's settings.
- **At login**, when the account holds none of the keys the browser holds, the browser's values are written to the account, so a choice made before signing up survives it; otherwise the account's values win and overwrite the browser's copy. **At logout** the browser keeps the last values, so the page does not change under the reader.
- **While signed in**, the page mirrors the account's values into the browser's storage, and the head script applies stored values only where the server rendered none, so the account's values win while signed in and remain after logout.
- **On a farm**, browser storage is per host, so a reader's choices on one tenant do not reach another; a signed-in reader's keys that follow the farm account do. Of the keys theming defines ([0086](0086-theming.md) §4), colour mode, contrast and text size follow the farm account; palette and type set, which depend on what each tenant offers, are per wiki unless the reader chooses **Use on every wiki**.

The keys themselves, and what they change, are [0086](0086-theming.md) §4's. This settles [0027](0027-preferences-and-portability.md) Q4: anonymous preferences are held in the browser, not in a cookie, and only for keys marked `browser`.

### 7. The wiki's name, language, logo and icon (extends 0015 §3; amends 0012 §4; extends 0012 §5; amends 0008 §7)

Four tenant settings in the `site` group:

- **`site.name`**: the wiki's name. `general.sitename` in `meta=siteinfo`, the global header, the name of the Project namespace, the `name` of the tenant list in `GET /tenancy`, and the operator's display name when the tenant is primary ([08](../architecture/08-tenants-and-instances.md) §8.2). `tenant create --name` writes it; the default is the slug. It is validated as a namespace name is, and refused (`ts-invalid-setting`) where it clashes with another namespace's name or alias or with an interwiki prefix. **Where the Project namespace is named after the site, a changed name keeps the old one as an alias of the namespace**, written in the same change set, so links to `Old name:Page` still resolve, as `$wgNamespaceAliases` would ([0008](0008-namespaces-and-document-pages.md) §7). A tenant whose `namespace` record gives namespace 4 a fixed local name, `Project` or any other, keeps that name when the site is renamed, and no alias is written, since nothing ever linked through the site's name.
- **`site.language`**: the content language, `$wgLanguageCode`: `general.lang`, the default interface language, and the fallback for labels and reports. Default `en`. Changing it rewrites nothing; it changes what is reported and what falls back to it.
- **`site.logo`**: a site asset (§8) accepting PNG, JPEG, GIF, WebP and SVG, shown in the global header and on a private tenant's landing page, and reported as `general.logo`. Without one the site name is shown as text.
- **`site.icon`**: a site asset accepting PNG, ICO and SVG, served as the favicon and the touch icon, and reported as `general.favicon`, as MediaWiki reports `$wgFavicon`.

### 8. Site assets (amends 0039 §7)

**A site asset is a file version a setting uses for a basic site function**: the logo, the favicon, a typeface. It is public because, and only while, configuration names it: a setting of type `site-asset`, or a type set's font file ([0086](0086-theming.md) §3). The exposure follows the reference, never a flag on the file, so it has a reason, an author and an end.

- **Asset settings.** A setting of type `site-asset` names a local `File:` and pins one of its versions: its value is the title and the version's SHA-1, and its registry entry lists the media types it accepts in `media` (ICO joins the permitted file types for `site.icon`, [0039](0039-files-and-media.md) §5). Writing it needs the setting's right, `ts-config` at least, and the setter must be able to read the version. A later upload to the file changes nothing until someone with that right names the new version; the settings page offers **Use the newest version** when there is one. An instance default that names a file resolves in each tenant (§2.1), so an asset is always one of the tenant's own files.
- **Served to anyone.** `{media base}/asset/{h}/{name}` serves a version that the tenant's configuration names, a resolved asset setting or a font file of a type set it offers, to anyone, a private tenant's outsiders included, with every media route's headers ([12](../architecture/12-files-and-media.md) §4.4). It is the one route that skips [0039](0039-files-and-media.md) §7's access check. The URL names the content hash, so it caches as immutable and a new version is a new URL. A version no setting names is answered as the ordinary media routes would answer it.
- **Moderation wins.** A version that is hidden, suppressed or taken down is never served as an asset; naming one is refused, and a setting whose version is hidden later resolves as unset, so the logo falls back to the site name.
- **Exposure is visible.** An asset's file page says so ("Public as this wiki's logo"); `Special:WikiSettings` lists every file the wiki exposes this way under **Public files**; and every change to what is exposed is a `settings/change` event.
- **SVG.** `site.logo` and `site.icon` accept SVG, because a holder of `ts-config` is trusted with the wiki's appearance. An SVG asset is drawn with `<img>`, where its scripts never run, and served with `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'`, so they do not run when the file is opened directly either. That trust does not extend to tenants whose `ts-config` holders the operator does not vet: **SVG sanitization (Q5) must exist before an instance hosts untrusted tenants.**

The first asset settings are `site.logo` and `site.icon` (§7); type sets name font files the same way ([0086](0086-theming.md) §3).

### 9. Crates (extends 0005 §2)

| Crate | Change |
|---|---|
| `triplespace-accounts` | Embeds `docs/registry/settings.toml`; the `setting` config kind; the setting resolver (tenant, instance default, registry; locks, ceilings, origins) and the one validator every writer calls; preference resolution through `preferences.defaults` and the farm account for `follows = farm` |
| `triplespace-api-rest` | `GET /settings/schema`, `GET /settings`, `PATCH /settings` |
| `triplespace-files` | The site-asset route `{media base}/asset/{h}/{name}`, and the checks on naming a version (§8) |
| `triplespace-api-action` | `general.sitename`, `general.lang` and `general.logo` from the `site.*` settings; the `siteinfo` cache key with the instance's registry offset |
| `triplespace-projections` | The `settings/change` log event |
| `triplespace-ui` | `Special:Preferences`, `Special:WikiSettings` with its Public files list, `Special:InstanceSettings`, the `Special:Account` sections, the Appearance menu and its head script, and the site-asset notice on a file page |
| `triplespace-cli` | `settings get`, `set` and `unset`, with `--instance`, `--default` and `--lock` |

`docs/architecture/settings.py` generates [23](../architecture/23-configuration-and-registry.md) §3.2 and §3.3 and runs in CI.

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

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §9 | extends | 0005 A95 |
| [0008](0008-namespaces-and-document-pages.md) §7 | §7 | amends | 0008 A37 |
| [0010](0010-site-ui.md) §2 | §6 | extends | 0010 A41 |
| [0010](0010-site-ui.md) §11 | §5 | amends | 0010 A41 |
| [0011](0011-logs.md) §6.1 | §3 | extends | 0011 A31 |
| [0012](0012-api-requirements.md) §4 | §2.1, §7 | amends | 0012 A75 |
| [0012](0012-api-requirements.md) §5 | §3, §7 | extends | 0012 A75 |
| [0015](0015-record-format-and-partition-registry.md) §3 | §1, §2.1, §7 | amends | 0015 A59 |
| [0015](0015-record-format-and-partition-registry.md) §5 | §1 | amends | 0015 A59 |
| [0016](0016-permissions-and-access-control.md) §2 | §3 | extends | 0016 A33 |
| [0021](0021-notifications.md) §6 | §5 | amends | 0021 A16 |
| [0027](0027-preferences-and-portability.md) §1 | §1, §2.2 | amends | 0027 A8 |
| [0027](0027-preferences-and-portability.md) §7 | §5 | amends | 0027 A8 |
| [0027](0027-preferences-and-portability.md) Q4 | §6 | settles | 0027 Q4 |
| [0028](0028-tenancy-policy.md) §8 | §2.1 | amends | 0028 A21 |
| [0028](0028-tenancy-policy.md) §11 | §4 | amends | 0028 A21 |
| [0034](0034-frontend-stack.md) §9 | §2.2 | amends | 0034 A11 |
| [0039](0039-files-and-media.md) §7 | §8 | amends | 0039 A13 |
| [0040](0040-instance-prerogatives.md) §6 | §2.1 | amends | 0040 A7 |
| [0046](0046-primary-tenant.md) §9 | §4 | amends | 0046 A8 |
| [0047](0047-special-pages.md) §1 | §4, §5 | extends | 0047 A22 |
| [0047](0047-special-pages.md) §2 | §5 | amends | 0047 A22 |
| [0047](0047-special-pages.md) §3 | §4 | amends | 0047 A22 |
| [0018](0018-tenants.md) §10 | §2.3 | extends | 0018 A29 |
| [0056](0056-security-model.md) §3 | §4 | amends | 0056 A16 |
| [0056](0056-security-model.md) §4 | §4 | amends | 0056 A16 |
| [0056](0056-security-model.md) §13 | §4 | amends | 0056 A16 |
| [0057](0057-web-tier.md) §6 | §2.2 | extends | 0057 A5 |
| [0067](0067-proposals.md) §6 | §5 | amends | 0067 A9 |

## References

- MediaWiki: `Special:Preferences`, `$wgDefaultUserOptions`, `$wgSitename`, `$wgLanguageCode`, `$wgLogos`, `$wgNamespaceAliases`.
- Miraheze's ManageWiki and CreateWiki extensions, the precedent for per-wiki settings on a farm.
- Vector 2022's client preferences and Appearance menu, the precedent for appearance choices kept in the browser for readers who are not logged in.
