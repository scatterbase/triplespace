# 0039. Files, blob storage and foreign file repositories

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-09 (A12)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0028](0028-tenancy-policy.md), [0030](0030-edit-filters.md), [0033](0033-backend-stack.md), [0035](0035-adopting-a-wikibase.md), [0038](0038-page-metadata-and-categories.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0007](0007-actor-identity.md), [0019](0019-discussions.md), [0040](0040-instance-prerogatives.md), [0041](0041-content-models.md)
- **Chapters:** [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [09](../architecture/09-security-and-moderation.md), [12](../architecture/12-files-and-media.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md)

## Context

MediaWiki splits a file into two things. The **description page** is an ordinary wikitext page in the File namespace (6), with a revision history like any page. The **file** is a sequence of versions held in the `image`, `oldimage` and `filearchive` tables, whose bytes live in a file repository behind a `FileBackend`: a directory by default, OpenStack Swift on Wikimedia, S3 through an extension. The virtual Media namespace (−2) links straight to the bytes. Uploading creates the page if it does not exist and a log entry; uploading again adds a version. Thumbnails are generated on demand and cached on disk. A wiki can also draw files from **foreign repositories**: InstantCommons, which reads Wikimedia Commons over its API, or a shared database and directory on a farm, so that `[[File:Example.jpg]]` falls through to Commons when no local file has that name.

Triplespace already has most of this. [0008](0008-namespaces-and-document-pages.md) took Scatterbase's distinction between a **blob**, a sequence of bytes, and a **view**, which decides what the bytes mean. A description page is a blob of text in a document namespace. A file is a blob of binary. Pages are keyed by page ID, so a title is an attribute and a move touches nothing else ([0008](0008-namespaces-and-document-pages.md) §4). Pages carry statements ([0038](0038-page-metadata-and-categories.md) §1), which is Structured Data on Commons without MediaInfo entities. Deletion and hiding are `read` ACLs ([0023](0023-moderation.md)), and erasure destroys a record's parts with a verifiable gap ([0006](0006-log-integrity-and-erasure.md) §7, [0015](0015-record-format-and-partition-registry.md) §1). Mirror partitions, tombstones and compaction ([0002](0002-source-graphs-and-mass-ingest.md) §2, §5) and live upstream fetches ([0012](0012-api-requirements.md) §6) are the machinery for data that belongs to someone else. What is missing is a place for the bytes, and the rules that follow from keeping bytes outside the log.

MediaWiki's file storage has two habits worth not copying. Paths are derived from the **title** (`/a/ab/Example.jpg`), so a move renames objects, and the deleted zone renames them again by SHA-1. And MediaWiki has **no permanent deletion** short of maintenance scripts run behind the scenes: a deleted file sits in the deleted zone forever, restorable by any administrator, and removing it for good is an exceptional act outside the wiki.

James's direction, from the design discussion of 2026-09-30:

- A basic setup stores files in a **directory**, which can be a mounted volume in a container or a mounted remote filesystem, with the option of an **S3-compatible** store.
- **Storage is separated per tenant by default**, and that is a configured value, not a fixed rule.
- **Files always go through the binary's access check.** As a general principle, access to every Triplespace resource is through an access check.
- A tenant deleting a file and an operator taking a file down are different acts. Deletion by a tenant leads to the bytes being purged eventually, after enough attrition. A takedown is an **operator prerogative** that tenants cannot override, as global edit filters are.
- **Deletion is tombstoning** unless permanent deletion is requested in a later step, or by a command that does both at once. They are requested in different circumstances. Editorial deletion and a DMCA takedown only need a tombstone: copyright law concerns making material available, and an appeal may succeed. A GDPR erasure request or the removal of illegal material needs permanent deletion at once. This extends MediaWiki's deletion concepts with a permanent delete that MediaWiki does not have, and needs a vocabulary.
- **Foreign file repositories are in scope.**

## Decision

### 1. The File and Media namespaces (amends 0008 §2)

*Changed by A3, A5.*

*Current text: [12](../architecture/12-files-and-media.md) §1.1, §1.2.*

### 2. Upload records (extends 0015 §1 and 0038 §1)

*Current text: [12](../architecture/12-files-and-media.md) §1.3.*

### 3. The blob store (uses 0033 §1)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §8.*

### 4. Storage scopes: separate per tenant by default (extends 0018 §2)

*Current text: [12](../architecture/12-files-and-media.md) §2.*

### 5. Uploading: stash, validation and warnings (extends 0030 §2)

*Current text: [12](../architecture/12-files-and-media.md) §3.1, §3.2, §3.3.*

### 6. Derived data: metadata and thumbnails

*Current text: [12](../architecture/12-files-and-media.md) §3.4.*

### 7. Serving: every byte behind an access check (extends 0014 §6)

*Current text: [12](../architecture/12-files-and-media.md) §4.1, §4.2, §4.3, §4.4.*

### 8. Removing files: the vocabulary (extends 0023 §1)

*Current text: [12](../architecture/12-files-and-media.md) §5.1.*

### 9. Tenant removal: delete, hide, erase and reclaim

*Changed by A2, A11.*

*Current text: [12](../architecture/12-files-and-media.md) §5.2, §5.3.*

### 10. Operator removal: take down, reinstate, expunge (extends 0023 §2; extends 0028 §8)

*Changed by A2.*

*Current text: [12](../architecture/12-files-and-media.md) §5.4, §5.5, §5.6.*

### 11. Foreign file repositories (uses 0002 §2, §5, 0012 §6 and 0028 §5)

*Changed by A6, A8, A9, A12.*

*Current text: [12](../architecture/12-files-and-media.md) §6.1, §6.2, §6.3, §6.4, §6.5, §6.6.*

### 12. Media data types (uses 0003 §9 and 0038 §1)

*Changed by A12.*

*Current text: [12](../architecture/12-files-and-media.md) §7.1, §7.3.*

### 13. Wikitext and markdown (amends 0008 §8)

*Changed by A12.*

*Current text: [12](../architecture/12-files-and-media.md) §7.2, §7.3.*

### 14. Import, adoption, export and verification (extends 0008 §9, 0035 §2 and 0006 §9)

*Changed by A7.*

*Current text: [12](../architecture/12-files-and-media.md) §9.*

### 15. Rate limits (extends 0024 §5)

*Current text: [07](../architecture/07-actors-and-accounts.md) §6.2.*

### 16. Log events (amends 0011 §6.1)

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.3.*

### 17. API (extends 0012 §4 and §5)

*Current text: [18](../architecture/18-api.md) §2.2, §2.3, §2.4, §3.1, §3.2.*

### 18. Site UI (extends 0010 §2 and §5)

*Current text: [19](../architecture/19-site-ui.md) §3.3, §6.6.*

### 19. Search (extends 0014 §7)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §11.1.*

### 20. Storage (extends 0013 §5.6 and §7)

*Changed by A11.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.9, §5, §6.1, §9.6, §12.1, §12.2, §12.4.*

### 21. Permissions (extends 0016 §2)

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §2.3, §8.5.*

### 22. Scatterbase

*Current text: [22](../architecture/22-crates-and-stack.md) §1.3.*

### 23. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §2.2.*

## Consequences

- **A file is a page with versions, and nothing new is needed to have one.** Description pages, statements, history, moves, deletion and hiding are the machinery that already exists; uploads add one payload type and a place for bytes.
- **The log stays small and verifiable.** It holds hashes, not bytes, and still commits to every version's exact contents, so `verify` can check the store against the log and the log without the store.
- **A small instance stays one binary and Postgres.** A directory is the default store, and it can be a volume or a network mount; S3 is a backend, not a dependency.
- **Moves and duplicates cost nothing.** Content addressing means a rename touches no object, a revert stores nothing and identical bytes are stored once per scope.
- **Every byte is checked**, including thumbnails and proxied foreign files; only `link`-mode repositories are outside the rule, by explicit choice. The cost is that the binary carries file traffic. A CDN in front of the media base keeps public files cheap, and purges keep it correct.
- **Deletion has a full vocabulary.** Tenants delete, hide and erase; the instance reclaims what has stayed deleted long enough; operators take down, reinstate and expunge across every tenant. Permanent removal exists, is always a separate and named act, and leaves a verifiable trace. MediaWiki's only equivalent is a maintenance script.
- **Tenant isolation is the default and can be traded away.** Separate scopes cost duplicate storage across tenants and buy privacy and clean moves; an instance that prefers deduplication can choose it and is told what it gives up.
- **Foreign files are first-class.** Commons, a farm's shared tenant and any MediaWiki work as InstantCommons does, with reader privacy by default, a mirror for those who want resilience, and local annotations on foreign files.
- **Expunge and reclamation are instance prerogatives** ([0040](0040-instance-prerogatives.md) §6): written into tenants' partitions on the instance's authority, attributed to the instance operator. **One earlier rule gains an exception,** stated where it is made: `link` mode serves no bytes through the binary (§11).

## Open questions

- **Q1. Delivery by presigned redirect.** For very large instances on S3, whether public versions may be served by a short-lived presigned redirect after the check, so that bytes skip the binary. The check would still run; a redirect stays valid for its lifetime after a takedown.
- **Q2. Location metadata.** Whether uploads should have GPS EXIF stripped, or the uploader warned, by default. Stripping changes the bytes, and therefore the hash, of what was uploaded.
- **Q3. Perceptual hashing and hash lists.** A takedown blocks exact bytes only. Matching re-encoded copies (PDQ, PhotoDNA) and importing industry hash lists for illegal material are operator tools this ADR does not specify.
- **Q4. More thumbnailers.** PDF and DjVu pages, video posters and transcoding (MediaWiki's TimedMediaHandler), audio waveforms, 3D models. Each needs a renderer that fits the licence and sandboxing rules.
- **Q5. `files.reclaim_after`.** Whether 365 days is the right default, and whether an instance should be able to set reclamation per namespace or per reason.
- **Q6.** ~~**Mirroring at Wikidata scale.** A full Wikidata mirror references millions of Commons files through P18 and similar properties; whether `used` should count mirrored statements by default, or only local use.~~ *Settled by A12: page usage plus the local graph's statements by default; a repository opts in to resolved-view usage with `mirror_usage = resolved`, and `view.value_key` carries media-type rows only when a repository is configured.*
- **Q7.** ~~**MediaInfo.** If an instance mirrors Commons' MediaInfo entities (`WDM`, [0000](0000-init.md)), whether a foreign Commons file's page should show them as its page data.~~ *Settled by [0065](0065-mediainfo-captions-and-commons.md) §2: it does, overlaid with local corrections.*
- **Q8. `geo-shape` and `tabular-data`**, which point at Commons' Data namespace rather than at files.
- **Q9. Two-person expunge.** Whether `ts-expunge` should need a second operator's confirmation, given that it cannot be undone.

## Changes to other ADRs

| Target | By | Change | Target's log |
|---|---|---|---|
| [0005](0005-crate-organization.md) §2 | §23 | extends | 0005 A39 |
| [0006](0006-log-integrity-and-erasure.md) §7, §9 | §8–10, §14 | extends | 0006 A10 |
| [0008](0008-namespaces-and-document-pages.md) §1, §2, §3, §8, §9 | §1, §13, §14 | extends | 0008 A10 |
| [0010](0010-site-ui.md) §2, §5.1 | §18 | extends | 0010 A23 |
| [0011](0011-logs.md) §6.1 | §2, §10, §14 | extends | 0011 A13 |
| [0012](0012-api-requirements.md) §4, §5, §8 | §17 | extends | 0012 A24 |
| [0013](0013-postgres-storage.md) §5.6, §7 | §20 | extends | 0013 A14 |
| [0014](0014-caches-and-search.md) §6, §7, §10 | §7, §19–20 | extends | 0014 A5 |
| [0015](0015-record-format-and-partition-registry.md) §1, §5 | §2, §5, §10–11 | extends | 0015 A17 |
| [0016](0016-permissions-and-access-control.md) §2, §6 | §10, §21 | extends | 0016 A13 |
| [0018](0018-tenants.md) §2 | §4, §10 | extends | 0018 A6 |
| [0023](0023-moderation.md) §1, §2 | §8–10 | extends | 0023 A5 |
| [0024](0024-subsidiary-accounts.md) §5 | §15 | extends | 0024 A4 |
| [0028](0028-tenancy-policy.md) §8 | §10 | extends | 0028 A4 |
| [0030](0030-edit-filters.md) §2 | §5 | extends | 0030 A4 |
| [0033](0033-backend-stack.md) §5 | §3, §6 | extends | 0033 A2 |
| [0035](0035-adopting-a-wikibase.md) §2 | §14 | extends | 0035 A2 |
| [0038](0038-page-metadata-and-categories.md) §1 | §2 | extends | 0038 A2 |

## References

- [Manual:Image administration](https://www.mediawiki.org/wiki/Manual:Image_administration), [Manual:Image table](https://www.mediawiki.org/wiki/Manual:Image_table), [Manual:Oldimage table](https://www.mediawiki.org/wiki/Manual:Oldimage_table), [Manual:Filearchive table](https://www.mediawiki.org/wiki/Manual:Filearchive_table)
- [Manual:FileBackend](https://www.mediawiki.org/wiki/Manual:FileBackend), [Manual:$wgForeignFileRepos](https://www.mediawiki.org/wiki/Manual:$wgForeignFileRepos), [InstantCommons](https://www.mediawiki.org/wiki/InstantCommons)
- [Manual:Configuring file uploads](https://www.mediawiki.org/wiki/Manual:Configuring_file_uploads), [Manual:$wgProhibitedFileExtensions](https://www.mediawiki.org/wiki/Manual:$wgProhibitedFileExtensions), [Manual:importImages.php](https://www.mediawiki.org/wiki/Manual:ImportImages.php), [Manual:deleteArchivedFiles.php](https://www.mediawiki.org/wiki/Manual:DeleteArchivedFiles.php)
- [API:Upload](https://www.mediawiki.org/wiki/API:Upload), [API:Imageinfo](https://www.mediawiki.org/wiki/API:Imageinfo), [API:Filerepoinfo](https://www.mediawiki.org/wiki/API:Filerepoinfo), [API:Filerevert](https://www.mediawiki.org/wiki/API:Filerevert)
- [Help:Images](https://www.mediawiki.org/wiki/Help:Images) (embedding syntax), [Help:Images#Gallery syntax](https://www.mediawiki.org/wiki/Help:Images#Gallery_syntax)
- [Extension:AbuseFilter/Rules format](https://www.mediawiki.org/wiki/Extension:AbuseFilter/Rules_format) (file variables), [Manual:$wgRateLimits](https://www.mediawiki.org/wiki/Manual:$wgRateLimits)
- [Extension:WikibaseLocalMedia](https://www.mediawiki.org/wiki/Extension:WikibaseLocalMedia)
- [Help:CirrusSearch](https://www.mediawiki.org/wiki/Help:CirrusSearch) (file keywords)
- [`object_store`](https://docs.rs/object_store) (Apache Arrow)
- [Lumen](https://lumendatabase.org/), the public database of takedown notices

## Amendment log

### A1. Crate table

- **Date:** 2026-09-30
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §23
- **Summary:** 0005 §2 is the one crate table CI checks, and carries the three new crates and every change this section listed (0005 A39).

Replaced text (§23):

> | Layer | Crate | Change |
> |---|---|---|
> | Substrate | `scatter-blob` *(new)* | The blob store over `object_store`: the `fs` and `s3` backends, scopes and the key layout, streaming SHA-256 and SHA-1, copy-if-absent, ranged and streamed reads, prefix deletion, and the listing the orphan sweep and `verify` walk (§3–4). Async, like `scatter-log-postgres` |
> | | `scatter-files` *(new)* | The `scatter:v0/upload` and `scatter:v0/expunge` payload types (§2, §10); the `file-name` normalizer; the type registry, embedding `docs/registry/file-types.toml`; type detection, the script and SVG checks (§5); metadata extraction; the transform grammar and the raster and SVG thumbnailers (§6). Pure |
> | | `scatter-log` | The instance `log` partition and `files/{repo}` in the graph registry (§10–11) |
> | | `scatter-actors` | The `blob` target kind at instance scope, the content axis of enclosure, restriction to no group, and the file and operator rights (§10, §21) |
> | | `scatter-integrity` | Blob verification at `presence` and `full`, and bundles with `--blobs` (§14) |
> | Wikibase | `scatter-wikibase-model`, `scatter-wikibase-rdf` | The `localMedia` data type and its RDF (§12) |
> | | `scatter-wikitext`, `scatter-pages` | File embeds, gallery and `Media:` links; markdown image syntax with file targets (§13) |
> | | `scatter-filter` | The `upload` context (§5) |
> | | `scatter-mwlog` | `upload/*`, `takedown/*` and `import/upload` for files (§16) |
> | Triplespace | `triplespace-files` *(new)* | The upload pipeline and stash; the media routes, signed URLs and headers (§7); thumbnail rendering and caching; reclamation, the orphan sweep and `ops.blob_delete`; takedown and expunge execution; the `tenant` and `mediawiki` repositories in their three modes, and the mirror job (§11); file import and the adoption step (§14) |
> | | `triplespace-projections` | `view.file`, `file_version`, `blob`, `file_link`, `file_block` and the metadata projection (§20) |
> | | `triplespace-titles` | The `file` kind, `Media:` resolution and repository fall-through (§1, §11) |
> | | `triplespace-upstream` | `prop=imageinfo` and rendered descriptions from `mediawiki` repositories (§11) |
> | | `triplespace-cache`, `triplespace-search` | The tags and keys of §20; file fields in `pages` (§19) |
> | | `triplespace-api-action`, `triplespace-api-rest` | §17 |
> | | `triplespace-server`, `triplespace-cli` | The media routes; `files import`, `files rescope`, `files verify`, `takedown`, `reinstate` and `expunge` |
>
> The workspace goes from forty-six crates to forty-nine.

### A2. Reclamation and expunge are instance prerogatives

- **Date:** 2026-09-30
- **Source:** [0040](0040-instance-prerogatives.md) §6
- **Change:** amends §9, §10
- **Summary:** The erasures that reclamation and expunge write into a tenant's `pages` carry the instance attestation of 0040 §3, with the reclamation job record or the `expunge` record in the instance `log` as authority. This replaced the exception to 0018 §4 this ADR first made for files alone; §9, §10 and the consequence were written to it in place.

Replaced text: not recorded; 0040's change was written into §9, §10 and the Consequences before this ADR was first committed, together with 0040 (commit `644c2b6`, 2026-09-30).

### A3. File is a `pages` namespace with uploads and a `mediainfo` slot

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §4, §6–7
- **Change:** amends §1
- **Summary:** The `file` kind is gone: File (6) is a `pages` namespace with the `wikitext` model and `uploads = true`. A File page also has a second slot, `mediainfo` (model `wikibase-mediainfo`), which presents its statements as the MediaInfo entity `M{page ID}` over the Wikibase Action API.

Replaced text (§1):

> | 6 | File | **File** *(new kind)* | A document namespace whose pages may also carry uploads (§2). Content models: `wikitext` only, as MediaWiki's description pages are |
>
> **The `file` kind is `document` plus uploads.** Everything 0008 says of document pages holds:

### A4. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–23
- **Summary:** A1–A3 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A3 was a blockquote, A2 was written in place with no note, and A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A5. File redirects

- **Date:** 2026-10-01
- **Source:** [0051](0051-page-redirects.md) §4
- **Change:** extends §1
- **Summary:** A File page may be a redirect; the file lookup follows it one hop before trying repositories, and `movefile` leaves one.

### A6. Images in mirrored pages

- **Date:** 2026-10-01
- **Source:** [0053](0053-mirrored-pages.md) §2, §9
- **Change:** extends §11
- **Summary:** Images in a page repository's HTML are served by the tenant's file repositories in their modes, with an allow-list for images that are no repository's file; the attribution rule applies to whole mirrored pages.

### A7. Copying a fork's files

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §7
- **Change:** extends §14
- **Summary:** A separate job copies the files a forked page uses as `import` uploads, on request, under `upload` and `reupload-shared`.

### A8. Commons' MediaInfo on foreign files

- **Date:** 2026-10-05
- **Source:** [0065](0065-mediainfo-captions-and-commons.md) §2
- **Change:** extends §11
- **Summary:** `WDM` mirrored on demand and shown as the foreign file's page data; Q7 settled.

### A9. `repo.cache_ttl` is a per-repository field

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §11
- **Summary:** `repo.cache_ttl` is a per-repository field of the `page-repo`/`file-repo` record, not a setting beside it, with a per-kind default: 7 days for files, one hour for template source and page bundles ([0042](0042-template-expansion-and-parsoid.md) §11; [0052](0052-page-repositories-and-title-inheritance.md) §1). §11's `proxy` row names the field as a field of the `file-repo` record with the 7-day default. (PENDING C20)

Replaced text (§11):

> | `proxy` *(default)* | Fetched by the server from the repository and served from the media base (§7), cached under `cache/{repo}/` with a TTL (`repo.cache_ttl`, default 7 days) and a size cap with least-recently-used eviction | `prop=imageinfo` and the rendered description from `action=parse`, through the live upstream client: cached, rate-limited, User-Agent and `maxlag` set, never logged ([0012](0012-api-requirements.md) §6) | Readers' addresses never reach the repository |

### A10. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§23
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [09](../architecture/09-security-and-moderation.md), [12](../architecture/12-files-and-media.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A11. Byte destruction is an `ops` job, never a projection step

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §6
- **Change:** amends §9; extends §20
- **Summary:** No projection has side effects during replay. `view.blob` is a pure count: when an erasure brings a count to zero, the projection enqueues an `ops` job for the scope and hash, keyed by them so that a repeat is a no-op, and the job takes a lock on the hash, re-checks the count at the live head, and only then deletes the object and its thumbnails and purges the hash's L2 tags. The job runs at once, so the bytes of an erased version are gone within its latency; a projection rebuild re-derives `view.blob` and re-runs no job, since the `ops` queue is not a projection target, and a count a rebuild finds at zero for an object still present is reported by `verify` as an orphan, not deleted. 0083's table names §20 (storage), whose chapter row in [03](../architecture/03-storage-caches-and-search.md) §4.9 is extended; the sentence contradicted is §9's, in [12](../architecture/12-files-and-media.md) §5.3, so a row for §9 is added. (REVIEW G13)

Replaced text ([12](../architecture/12-files-and-media.md) §5.3, as it stood):

> When an erasure brings the count to zero, the projection queues the object and its thumbnails for **deletion at once**, not at the next sweep, and purges the hash's L2 tags.

### A12. Foreign files: a local page to annotate, an ordinary cluster, cached existence and bounded usage

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §11, §12, §13
- **Summary:** Annotating a foreign file locally requires a **local File page**, a page `create` with the description text and no upload, which takes a page ID from the tenant's sequence and whose `M{page ID}` keys every local statement and caption on the file; the overlay keyed by a ranged page ID is dropped, and a foreign file no local page describes has no local statements (§11). The local `M` entity and the `WDM` it mirrors are an **ordinary identity cluster with a fixed canonical**, formed by the file lookup when a local File page is created for a title a Commons repository serves, the canonical always the `WDM` member, never re-pointed by `same-as` or `different-from` and dissolved only when the page is deleted or the file stops being the repository's; a hand-written `same-as` on an `M` ID is still refused (§11). `mediainfo.mirror = on-demand` enqueues a fetch through `ops.entity_fetch` and the page or statement renders without the statements until the fetch lands; nothing waits on Commons inside a read or a write (§11). `commonsMedia` existence is cached per (repository, name) in L1 with the repository's `cache_ttl`, the uncached names of a change set are validated in one batched request, and a job may declare `validate_media = defer`, its failures tracked as constraint-style rows beside the statement; an interactive write whose batch cannot reach the repository is refused with `upstream-unavailable`, a deferred job's is recorded (§12). Usage by statement is served from `view.value_key` for the media data types, written only when a repository is configured, so a tenant that reads Wikidata and names no Commons repository carries no rows for the millions of `P18` values in its view (§12, §13). `mirror` mode's "used" is page usage plus the local graph's statements by default; a repository opts in to resolved-view usage with `mirror_usage = resolved` (§11). This settles Q6. The ledger's rows name §10 and §14, whose chapter text did not change; the folds are in [12](../architecture/12-files-and-media.md) §6.4, §6.5, §6.6, §7.1 and §7.3, which hold §11, §12 and §13. (REVIEW G44, G45)

Replaced text ([12](../architecture/12-files-and-media.md) §6.4, as it stood):

> It mirrors the files that are **used**: referenced by a page of any tenant using the repository, or by a `commonsMedia` value in such a tenant's resolved view (§7.1).

Replaced text ([12](../architecture/12-files-and-media.md) §6.5, as it stood):

> A **local page** for a foreign file holds local text and statements ([0038](../decisions/0038-page-metadata-and-categories.md) §1) shown beside the foreign description: local annotations on someone else's file, which is the foreign-entity pattern of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) applied to files. Its talk page is local.

Replaced text ([12](../architecture/12-files-and-media.md) §6.6, as it stood):

> **Mirroring set**: `mediainfo.mirror` (site, default **`on-demand`**): a Commons file's MediaInfo is fetched through `Special:EntityData` ([0012](../decisions/0012-api-requirements.md) §6's upstream fetch) the first time the file is used by a local page or shown, written as a `put`, and kept current by Commons' EventStreams `mediainfo` changes as [0053](../decisions/0053-mirrored-pages.md) §6 keeps mirrored pages current;

> **Clusters**: a local `M` entity and the `WDM` it mirrors are **one subject by construction**, not a cluster: the local page *is* the foreign file's page here, with the ranged page ID of [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 ([04](../architecture/04-entities-and-identifiers.md) §5.3) where the file is only inherited. `same-as` between `M` IDs is refused, as §8.2's "derived, not minted" implies.

Replaced text ([12](../architecture/12-files-and-media.md) §7.1, as it stood):

> a local write is validated against it, as Wikibase validates against Commons; if the repository cannot be reached the write is refused with `upstream-unavailable` ([0012](../decisions/0012-api-requirements.md) §6). Mirrored values are never validated: they are upstream's. With no repository configured, values render as links to Commons.

Replaced text ([12](../architecture/12-files-and-media.md) §7.3, as it stood):

> It serves `prop=images`, `list=imageusage`, `Special:WhatLinksHere` and the file page's usage section, and it is what `mirror` mode (§6.4) counts as use. A file's usage also includes the entities whose statements name it, listed apart from the pages that embed it.
