# 0039. Files, blob storage and foreign file repositories

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-01 (A7)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0006](0006-log-integrity-and-erasure.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0028](0028-tenancy-policy.md), [0030](0030-edit-filters.md), [0033](0033-backend-stack.md), [0035](0035-adopting-a-wikibase.md), [0038](0038-page-metadata-and-categories.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0007](0007-actor-identity.md), [0019](0019-discussions.md), [0040](0040-instance-prerogatives.md), [0041](0041-content-models.md)

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

Three reserved numbers are implemented, as [0008](0008-namespaces-and-document-pages.md) §2 rule 1 allows:

| ID | Canonical name | Kind | Notes |
|---|---|---|---|
| −2 | Media | Virtual | `Media:X` resolves to the current version of the file `File:X`, local or foreign (§11), and links to its bytes. It never names a page |
| 6 | File | `pages`, with `uploads = true` | A `pages` namespace ([0041](0041-content-models.md) §4) whose pages may also carry uploads (§2). Content model `wikitext` only, as MediaWiki's description pages are, and a second slot, `mediainfo` (model `wikibase-mediainfo`), which presents the page's statements as the MediaInfo entity `M{page ID}` over the Wikibase Action API ([0041](0041-content-models.md) §6–7) |
| 7 | File talk | Composite | Enabled with its subject, as [0008](0008-namespaces-and-document-pages.md) §2 rule 2 requires |

**An `uploads` namespace is a `pages` namespace plus uploads.** Everything 0008 says of document pages holds: page IDs, full-text revisions, moves, the title index, statements ([0038](0038-page-metadata-and-categories.md) §1). A file page may exist with no upload, as in MediaWiki ("No file by this name exists"); it may also exist with no upload because it describes a **foreign** file of the same name (§11), in which case its text and statements are local annotations on someone else's file.

**A File page may be a redirect** ([0051](0051-page-redirects.md) §4): the file lookup below (§11) follows a local redirect page one hop before trying repositories, as MediaWiki's `RepoGroup::findFile` does, so `[[File:Old name.jpg]]` embeds the renamed file, and `movefile` leaves one as any move does.

**The `file-name` normalizer** is `first-letter` with MediaWiki's file-name rules: the title must end in an extension the type registry allows (§5), may not contain `/`, `\`, `:` (after the prefix) or control characters, and is at most 240 bytes in UTF-8. Extensions are matched without regard to case and kept as written, so `File:Example.JPG` and `File:Example.jpg` are different titles, as in MediaWiki.

### 2. Upload records (extends 0015 §1 and 0038 §1)

A file version is a record of a new payload type, **`scatter:v0/upload`**, appended to the tenant's `pages` partition and keyed by the file page's ID. Like every record in `pages` it takes a revision ID ([0013](0013-postgres-storage.md) §6) and needs a base offset ([0006](0006-log-integrity-and-erasure.md) §8). So **a file page has one history** in which text revisions, statement change sets and uploads interleave in the order they were made, which is the rule [0038](0038-page-metadata-and-categories.md) §1 set for pages.

| Part | Holds |
|---|---|
| Content | `action` (`upload`, `overwrite`, `revert`, `import`); `sha256` (32 bytes), the address of the bytes (§3); `sha1` (20 bytes), for MediaWiki compatibility only; `size`; `mime`; `media_type` (MediaWiki's `BITMAP`, `DRAWING`, `AUDIO`, `VIDEO`, `MULTIMEDIA`, `OFFICE`, `TEXT`, `ARCHIVE`, `3D`); `width`, `height`, `pages` and `duration` where the type has them; for `revert`, the coordinates of the version reverted to; for `import`, the upstream timestamp and uploader ([0007](0007-actor-identity.md) §5) |
| Comment | The upload summary |
| Attestation | Who uploaded, as for every record |

**The log commits to the bytes without holding them.** The content part carries the SHA-256 of the bytes, and the content part is committed to by the header ([0015](0015-record-format-and-partition-registry.md) §1). A verifier holding the record and the blob can check one against the other (§14); a verifier without the blob can still check the log. The content part is salted like every part, so the hash of an erased version cannot be read from its header.

**First upload.** Uploading to a title with no page appends a `page` `create` record carrying the description text (MediaWiki's "initial page text") and the `upload` record, in one transaction. A later upload appends only the `upload` record: it is the revision, and no null text revision is written, which is what MediaWiki writes to make an upload show in page history.

**The current version** is the newest `upload` record of the page. `revert` re-points the page at an earlier version's bytes by writing a new record with that version's hash; since bytes are content-addressed, a revert stores nothing.

**Extended metadata is not in the record.** EXIF, XMP and IPTC fields, colour profiles and embedded text are derived from the bytes (§6). The record holds only what describes the version on its own: what `prop=imageinfo` returns by default, and what an export without blobs still needs.

### 3. The blob store (uses 0033 §1)

**Bytes live in a blob store, addressed by SHA-256.** A new substrate crate, `scatter-blob`, defines the store over the [`object_store`](https://crates.io/crates/object_store) crate, which gives one interface to a local directory, S3 and S3-compatible services (MinIO, Ceph RGW, Garage, Cloudflare R2), Google Cloud Storage and Azure. Two backends are supported and tested:

| Backend | What it is | When |
|---|---|---|
| `fs` *(default)* | A directory: local disk, a volume mounted into the container, or a mounted network filesystem | Every small instance. It keeps [0033](0033-backend-stack.md) §1's deployment: the binary and Postgres, with no new service |
| `s3` | Any S3-compatible API, with endpoint, region, bucket, prefix and credentials | Larger instances, and any instance whose storage is object storage already |

The backend is **deployment configuration**, read from the server's configuration file or environment, not a `config` record ([0015](0015-record-format-and-partition-registry.md) §3), because it holds credentials and can differ between a primary and its replicas. Everything else about files is in `config`.

**Layout.** Within a **scope** (§4), objects are keyed:

| Key | Holds |
|---|---|
| `{scope}/sha256/{h[0..2]}/{h[2..4]}/{h}` | Original bytes, `{h}` the lowercase hex SHA-256 |
| `{scope}/thumb/{h}/{transform}` | Thumbnails and other derived renderings (§6) |
| `{scope}/stash/{key}` | Uploads in progress and stashed uploads (§5) |
| `{scope}/cache/{repo}/{sha1}/…` | Bytes fetched from a proxied foreign repository (§11) |

Objects under `sha256/` are **immutable**: a key's bytes never change, and identical bytes have one key. So the store needs no locks, a move or rename touches no object, deduplication is free, and two servers writing the same file at once write the same bytes to the same key, which is harmless.

**Bytes first, then the record.** An upload streams into `stash/` while the server computes SHA-256 and SHA-1 and enforces the size limit, is validated (§5), and is copied to its `sha256/` key unless an object is already there. Only then is the `upload` record appended. A crash leaves at worst an unreferenced object, which the **orphan sweep** deletes after a grace period (`files.orphan_grace`, default 24 hours); it never leaves a record pointing at bytes that were never stored.

**What a network filesystem must provide.** The `fs` backend writes to a temporary name and renames it into place. A mounted filesystem must make that rename atomic within a directory, which NFS, SMB and local volumes do. A FUSE mount of an object store (s3fs, goofys, rclone) generally does not, and an instance on object storage should use the `s3` backend directly.

**The same store serves Scatterbase**, whose blobs are already content-addressed by SHA-256 ([0006](0006-log-integrity-and-erasure.md) §2). `scatter-blob` is a substrate crate for that reason (§22).

### 4. Storage scopes: separate per tenant by default (extends 0018 §2)

A **scope** is the part of the store a reference counts against. The instance `site` setting **`files.separation`** decides what a scope is:

| Value | Scope | Effect |
|---|---|---|
| `tenant` *(default)* | One per tenant: its slug | Each tenant's bytes are stored apart. Identical bytes uploaded to two tenants are stored twice. A tenant's files leave with it as a set of objects under one prefix (§14) |
| `instance` | One for the instance: `_instance` | Identical bytes are stored once for every tenant. A reference from any tenant keeps them |

**Why tenant by default.** Shared storage is observable: if bytes another tenant holds are not stored again, an upload can learn, from its timing or a warning, that some tenant has a file, and a private tenant's file is then not private. It also makes a tenant's files depend on other tenants' erasures (§9) and complicates moving a tenant away ([0018](0018-tenants.md) §10). The saving is small in practice, because the files many tenants share are usually a shared repository's, and §11 serves those once without copying them.

**Under `instance`,** every warning and listing stays per tenant: `duplicate` warnings and `prop=duplicatefiles` name only the uploader's own tenant's files, and an upload of bytes already stored still runs every check and takes as long as reading them. What the setting cannot hide is that the store skips a write, and an operator who chooses `instance` accepts that.

**Changing the setting** is a job (`triplespace-cli files rescope`) that copies objects into the new scopes, re-derives `view.blob` (§20), and switches reads when it finishes. It needs `owner`, as the identity switches of [0028](0028-tenancy-policy.md) §2 do.

### 5. Uploading: stash, validation and warnings (extends 0030 §2)

**The stash** holds uploads that have no record yet: a chunked upload being assembled, or a file stashed with warnings for the uploader to confirm. MediaWiki's `filekey`, `stash` and chunked `offset`/`filesize` parameters work against it. A stash entry belongs to one actor, is not readable by anyone else, and expires (`files.stash_ttl`, default 48 hours); its row is in `ops.upload_stash` (§20).

**The type registry**, `docs/registry/file-types.toml`, lists each permitted type: extensions, MIME type, MediaWiki media type, the magic-byte signature used to detect it, whether it may be served `inline` or only as an `attachment` (§7), and its thumbnailer (§6). The instance `site` setting `files.types` lists the types the instance allows, by default the registry's `default = true` entries (raster images, SVG, PDF, common audio and video); a tenant `site` setting may narrow that list but not widen it. Types MediaWiki prohibits outright (executables, HTML, JavaScript, `.php` and the rest of `$wgProhibitedFileExtensions`) are never permitted, whatever configuration says.

**Checks**, in order, with MediaWiki's error codes where it has them:

1. The title's extension is permitted (`filetype-banned`), and the size is within `files.max_size` (`file-too-large`).
2. The type detected from the bytes matches the extension (`filetype-mime-mismatch`).
3. The bytes contain nothing a browser would run: no HTML or script markers in the first kilobytes of any type (`verification-error` with `uploadscripted`), and for SVG an XML parse that refuses scripts, event-handler attributes, `javascript:` and external references, `<foreignObject>` and entity declarations (`uploaded-hostile-svg`, `uploaded-script-svg`).
4. The hash is not blocked by a takedown or expunge (§10): `ts-file-blocked`, which says only that the file cannot be uploaded.
5. Edit filters (below).

**Warnings**, which stop the upload in the stash until the uploader confirms with `ignorewarnings`: `exists` (the title has a file), `exists-normalized`, `duplicate` (the same bytes are already a current version in this tenant), `duplicate-archive` (they were a deleted version), `was-deleted` (the title was deleted), `nochange` (an overwrite with identical bytes), `badfilename`. Uploading a local file over a foreign file's name (§11) is refused with `fileexists-shared-forbidden` without `reupload-shared`.

**Edit filters run on uploads.** [0030](0030-edit-filters.md) §2's contexts gain a third, **`upload`**, with AbuseFilter's file variables (`file_sha1`, `file_size`, `file_mime`, `file_mediatype`, `file_width`, `file_height`, `file_bits_per_channel`), `file_sha256`, the page variables of the text context for the title, and `action` set to `upload` or `stashupload`. Global filters ([0030](0030-edit-filters.md) §8) run on every tenant's uploads, as on its edits.

**Upload by URL** (`upload_by_url`, `action=upload&url=`) is off by default and held by no group. Fetching a URL on a user's behalf is a server-side request forgery risk; an instance that enables it gets a fetcher that refuses private and link-local addresses, follows no redirect to them, and goes through the upstream client's rate limits ([0012](0012-api-requirements.md) §6).

### 6. Derived data: metadata and thumbnails

**Metadata is a projection.** The file projection reads each new version's bytes once and writes its extracted metadata (EXIF, XMP, IPTC, page count, duration, bit depth, colour profile) to `view.file_version.metadata` (§20). Like every `view` table it is rebuilt from the log, which here means from the log and the blob store. A version whose content part is erased has no bytes and no metadata.

**Thumbnails are cache, not data.** A thumbnail is rendered on first request, stored under `thumb/{h}/{transform}` in the same scope as its original, and served from there afterwards. It is never logged and never exported, and it can be deleted at any time and rendered again. Its key is the original's hash and a normalized **transform**: width, page or time offset, and output format. Requested widths are rounded up to the next entry of `files.thumb_widths`, so that a client cannot fill the store with every width from 1 to 10,000; MediaWiki's `renderfile-nonstandard` limit (§15) covers what rounding does not. Rendering runs on a bounded pool (`files.thumb_concurrency`) with single flight per key ([0014](0014-caches-and-search.md) §2).

**Thumbnailers** are registered per type. This ADR ships two: raster images through the `image` crate, and SVG rasterized through `resvg`. Both are pure Rust under licences [0033](0033-backend-stack.md) §1 accepts. Video posters and transcodes, PDF pages, audio waveforms and 3D previews are left open (Q4); until a thumbnailer exists for a type, its files show an icon.

### 7. Serving: every byte behind an access check (extends 0014 §6)

**Access to every Triplespace resource goes through an access check.** This ADR states the rule because files are where it is most tempting to break it: a public bucket behind a CDN serves bytes cheaply and never asks who is reading. Triplespace does not do that. The binary serves every original and every thumbnail, local or proxied, after evaluating the viewer's `read` on the version under [0016](0016-permissions-and-access-control.md) §4 and [0023](0023-moderation.md) §1, including the instance ACLs of §10. A version that fails the check is answered with 404, never 403, so the answer does not confirm that the bytes exist.

**The media base.** Files are served from the tenant `site` setting `media.base_url`, by default `{tenant base}/media`. A **separate origin** is strongly recommended (`https://upload.example.org/{slug}` for a farm), because a file served from the wiki's own origin runs with the wiki's cookies if a check in §5 ever misses something. `meta=siteinfo&siprop=triplespace` reports whether the media base is same-origin, and the site UI warns administrators while it is.

| Route under the media base | Serves |
|---|---|
| `/file/{h}/{name}` | Original bytes. `{name}` is the title without its prefix, used for `Content-Disposition` |
| `/thumb/{h}/{transform}-{name}` | A thumbnail, in MediaWiki's `320px-Example.jpg` shape |
| `/repo/{repo}/file/{sha1}/{name}`, `/repo/{repo}/thumb/{sha1}/{transform}-{name}` | A proxied foreign file (§11) |

`Special:FilePath/{name}` and `Special:Redirect/file/{name}` on the wiki redirect to the current version's URL, and `[[Media:…]]` links point at them, so a stable link never names a hash.

**The check on a cookieless origin.** A request for a version that `universe` may read needs no identity: the check is on the version's state (deleted, hidden, erased, taken down) and the tenant's `read` ACLs, and the answer is the same for everyone. A version that only some viewers may read (a deleted version shown to the deletion group, a file on a private tenant) is served only from a **signed URL**: the wiki origin performs the check and issues `?exp={time}&sig={mac}`, an HMAC over the scope, hash, transform, viewer's actor key and expiry under the instance's media key, valid for minutes. The media origin verifies the signature and evaluates the check again for that actor, so a deletion between issue and fetch still holds. The media key is deployment configuration, rotated with two keys accepted during rotation.

**Headers.** Every response carries `X-Content-Type-Options: nosniff`, `Cross-Origin-Resource-Policy: cross-origin` (other sites may embed public files, as Commons allows; an instance may set `same-site`), and `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; img-src data:; sandbox`. A type the registry marks `inline` (raster images, audio, video, PDF) is served inline; every other type with `Content-Disposition: attachment`. SVG originals are served under the sandboxing CSP, and on a same-origin media base always as attachments; SVG thumbnails are rasterized and always inline. `Range` requests are honoured, for seeking in audio and video. `ETag` is the hash plus transform.

**Caching** follows [0014](0014-caches-and-search.md) §6. A public version is served with `Cache-Control: public, max-age=86400, s-maxage=2592000` and `Cache-Tag` values `blob:{scope}:{h}` and `file:{tenant}:{page id}`; any change that makes it unreadable (§8) purges both tags at L2, so the long `s-maxage` costs nothing in correctness. A signed URL is served `Cache-Control: private, no-store`. Bytes go from the store to the client as a stream; the binary never holds a whole file in memory.

### 8. Removing files: the vocabulary (extends 0023 §1)

Removal has two independent axes. **Who acts:** a tenant, through its own moderation, or the instance operator, whose action covers every tenant and which no tenant can undo. **How final it is:** a tombstone, which keeps the bytes and can be reversed, or destruction, which cannot. Hiding one part of a version is the familiar third, finer tool. The words:

| | **Reversible** (tombstone; bytes kept) | **Permanent** (bytes destroyed) |
|---|---|---|
| **Tenant** (editorial; one page or version) | **Delete**, and **undelete** ([0023](0023-moderation.md) §4) | **Erase** ([0006](0006-log-integrity-and-erasure.md) §7), and **reclaim**, its automatic form after a retention period (§9) |
| **Instance operator** (prerogative; by content hash, across every tenant) | **Take down**, and **reinstate** (§10) | **Expunge** (§10) |

And, unchanged from [0023](0023-moderation.md) §5, **hide** (and **suppress**) one part of one version: its bytes and metadata (`content`), its summary or its uploader.

- **Delete** is what MediaWiki means: the page, or one old version, is visible only to the deletion group, and an administrator can undelete it. It is a `read` ACL, nothing more.
- **Erase** is what MediaWiki lacks: the version's content part is erased, its bytes are destroyed once nothing else references them (§9), and a verifiable gap remains in the history. Nobody can undo it. It is a tenant action, for a tenant's own legal or privacy obligation.
- **Reclaim** is erase done by the instance on a schedule, to versions a tenant deleted long ago. It is how deletion turns into purging "after enough attrition" without anyone having to ask.
- **Take down** is the operator's tombstone: the bytes become unreadable everywhere, by everyone, and cannot be uploaded again, until the operator **reinstates** them. It answers notices whose remedy is withdrawal of access with room for appeal, such as a DMCA notice.
- **Expunge** is the operator's destruction: every tenant's references to the bytes are erased and the bytes are destroyed at once, and the hash stays blocked. It answers obligations whose remedy is that the material no longer exist here: a GDPR erasure request, or illegal material.

**Two steps, or one.** Each permanent action may follow its reversible partner as a separate, later decision: delete, then erase from the deleted view; take down, then expunge from the takedown. Each may also be requested directly, and then the reversible record and the permanent one are written in one request, so that there is no moment when the material is destroyed but its tombstone is missing. Which is right depends on why the request was made, which is why both exist.

**Why not "purge".** MediaWiki's `action=purge` refreshes a page's cache; giving the word a destructive meaning would make the most dangerous action in the system sound like the most harmless one. "Expunge" is the legal word for destroying a record. "Erase" keeps the meaning [0006](0006-log-integrity-and-erasure.md) gave it, and files only extend what it reaches.

### 9. Tenant removal: delete, hide, erase and reclaim

*Changed by A2.*

**Deleting a file page** is [0023](0023-moderation.md) §4 unchanged: a `read` ACL on the page, which covers its text, statements and every upload. Its versions stop being served (§7), leave search and file usage, and their L2 tags are purged.

**Deleting one old version** (MediaWiki's `action=delete&oldimage=`) is a `record` ACL on that `upload` record with no `parts`, so the whole version is hidden from everyone outside the deletion group. **Hiding** a version's bytes (MediaWiki's revision deletion of type `oldimage`) is a `record` ACL with `parts = [content]`; its summary or uploader likewise. The **current version** cannot be deleted or hidden on its own, as in MediaWiki: an administrator reverts to an earlier version first, which the UI offers as one action, or deletes the page.

**Erasing a version** is an `erase` record ([0006](0006-log-integrity-and-erasure.md) §7) naming the `upload` record and, at least, its `content` part. Erasing the content part takes the version's hash out of the log, so the version no longer references its bytes. Erasure needs `ts-erase`, held by `suppress` ([0016](0016-permissions-and-access-control.md) §2), and its reason class is `legal` or `privacy` ([0016](0016-permissions-and-access-control.md) §6). The UI offers it on a deleted version, and directly as "Delete permanently" with the delete ACL in the same request (§8).

**Bytes go when the last reference goes.** `view.blob` (§20) counts, per scope and hash, the upload records whose content part is not erased, mirrored repository records (§11) and stash entries. When an erasure brings the count to zero, the projection queues the object and its thumbnails for **deletion at once**, not at the next sweep, and purges the hash's L2 tags. Deleted and hidden versions still count: their bytes must survive to be undeleted. Under `tenant` separation (§4), erasing a tenant's last reference therefore destroys the bytes. Under `instance` separation, another tenant's reference keeps them, and a request that the bytes leave the instance entirely is the operator's expunge (§10). The erasure form says which applies.

**Reclamation** is erasure on a schedule. A version that has been unreadable to `universe` (its page or record deleted, or its content part hidden or suppressed) continuously for **`files.reclaim_after`** (instance `site` setting, default 365 days; `never` disables it) is erased by the instance: an `erase` of its content part with reason class `operational`, in a nightly instance job. Reclamation is an instance prerogative ([0040](0040-instance-prerogatives.md) §6): the erasures carry the instance attestation, with the job record in the instance `log` as authority. A tenant may set a shorter period in its own `site` configuration, never a longer one. After reclamation an undeleted page keeps its text, statements and history; the reclaimed versions show as erased. Reclamation does not apply to a version that is unreadable only because of a takedown (§10), since a takedown may be reversed on appeal and the operator, not the schedule, decides when it becomes final.

### 10. Operator removal: take down, reinstate, expunge (extends 0023 §2; extends 0028 §8)

*Changed by A2.*

**Where operator records live.** The instance gains its own `log` partition, beside its `config` ([0018](0018-tenants.md) §2): `logged`, `full`, **internal**, attested by the primary tenant's actors as every instance-level record is ([0018](0018-tenants.md) §4). It is the instance counterpart of a tenant's `log`, and like it is never in a public dump, because the notices behind a takedown can themselves be sensitive.

**A takedown is an ACL on a new target kind**, `blob`, at instance scope, so that it reuses everything [0023](0023-moderation.md) §1 built and is undone the same way:

| Target kind | Key | Restricts |
|---|---|---|
| `blob` | `acl:blob:{takedown id}` | `read` and `upload` of every file version, local or foreign, in every tenant and scope, whose `sha256` or `sha1` equals the hash in the record's content |

- **The key is an ID, not the hash.** The takedown ID is a random 64-bit number shown in decimal. The hash (algorithm and value) is in the content part, so no header carries the hash of something taken down ([0006](0006-log-integrity-and-erasure.md) §3).
- **Content is an enclosure axis.** A `blob` target encloses every version with that hash, as a property encloses every snak that uses it ([0023](0023-moderation.md) §2). Evaluation stays conjunctive, so a tenant's own ACLs cannot satisfy or override it.
- **The restriction names no group.** Nobody reads a taken-down version through any tenant, including the tenant's administrators and its suppressors. Operators review taken-down bytes through the instance's takedown pages (§18), which need `ts-viewtakedown`.
- **SHA-1 is accepted as a target** because a foreign repository in `link` mode (§11) reports only SHA-1, and an operator must be able to stop the instance showing a foreign file. Every local version records both hashes (§2), so a SHA-1 takedown covers local copies too.
- **Content of the record:** the hash; the reason class (`legal` or `privacy`, [0016](0016-permissions-and-access-control.md) §6); a reference to the notice (a Lumen URL, a case or ticket number); and `confidential`, a flag. **Comment:** the public reason.

**What a takedown does.** In one request, synchronously ([0023](0023-moderation.md) §10): every matching version becomes unreadable; their thumbnails are deleted (they are regenerated on reinstatement); their L2 tags, `blob:` and `file:`, are purged in every scope; they leave search, file usage and `prop=duplicatefiles`; and the hash is **blocked**, so an upload of the same bytes to any tenant is refused (§5). The bytes themselves stay in the store, and every tenant's records are untouched. The description pages stay: a takedown removes bytes, not a tenant's words. Each affected page shows, in place of the file, that it was removed by the instance operator, with the public reason unless the takedown is `confidential`.

**Reinstating** retires the ACL: a record with the same key and null content. Thumbnails are rendered again on demand, and the hash is unblocked.

**Expunge** is a record of a new payload type, `scatter:v0/expunge`, in the instance `log`, keyed `expunge:{takedown id}` and carrying the same hash, reason class and authority. If no takedown exists for the hash, one is written in the same request, so that the material is unreadable before anything else happens. Then, in order:

1. **The bytes are destroyed at once,** in every scope that holds them: originals, thumbnails, stash entries and proxy caches. Their L2 tags are purged. This happens inside the request, so that an operator acting on illegal material knows the bytes are gone when the request returns.
2. **Every reference is erased.** A job appends, in each affected tenant's `pages` partition and in any `files/{repo}` mirror partition, an `erase` record naming the content parts of every `upload` record with that hash. Its reason class is the expunge's, and its authority reference is the expunge record's coordinates. The job is idempotent and resumes from the expunge record if interrupted.
3. **The hash stays blocked** while the expunge record exists, after the takedown it implies.

**The erasures are an instance prerogative** ([0040](0040-instance-prerogatives.md) §6). Each carries the instance attestation of [0040](0040-instance-prerogatives.md) §3: attributed to the instance operator, signed by the instance key, citing the `expunge` record as its authority. The operator who expunged is recorded only on the `expunge` record. A tenant can see that the instance erased its records, when, and the public reason; it cannot see the hash, and the reason class and authority reference follow [0016](0016-permissions-and-access-control.md) §6.

**An operator prerogative under every preset (extends 0028 §8).** Takedowns and expunges apply to every tenant whatever the tenancy policy, as the instance sitelink deny list and IP blocks already do ([0028](0028-tenancy-policy.md) §4, §8). An operator who hosts unrelated customers receives the notices, and has to be able to act on them without the customer's cooperation. `ts-takedown`, `ts-expunge` and `ts-viewtakedown` are evaluated on the primary tenant (§21).

**Visibility.** Takedown events are public in each affected tenant's log (§16), as Wikimedia publishes the DMCA notices it acts on; a `confidential` takedown appears as an event with no reason or notice. Expunges appear to tenants as their `erase/erase` events; the expunge record and its hash are visible only to `ts-viewtakedown`. Nothing about either reaches a public dump, because the instance `log` is internal and erased parts are erased.

**What expunge cannot reach.** Copies outside the instance (backups, mirrors made by others, a CDN that ignored a purge) are reached only as [0006](0006-log-integrity-and-erasure.md) §7 describes for erasure in general: the erase records travel on the feed, and backups are a runbook matter. One runbook item is specific to object storage: a bucket with **versioning** keeps a deleted object as a noncurrent version, so a versioned bucket must expire noncurrent versions within the backup window, or expunged bytes survive in it. The same applies to filesystem snapshots.

### 11. Foreign file repositories (uses 0002 §2, §5, 0012 §6 and 0028 §5)

*Changed by A6.*

A **file repository** is a source of files a tenant can use by name without uploading them. It is configured as a `config` record of kind `file-repo`, keyed `file-repo:{name}`, either in a tenant's `config` or in the instance `config`, where a tenant refers to it by name; it can be supplied by a tenancy template ([0028](0028-tenancy-policy.md) §8). The tenant `site` setting `files.repos` lists the repositories a tenant uses, in lookup order.

**Lookup follows MediaWiki.** A file title is looked up locally first, then in each repository in order, and the first match wins. A local file **shadows** a foreign one of the same name, and creating one needs `reupload-shared` (§5). `Special:ListDuplicatedFiles` and the file page say when a local file shadows a foreign one.

**Two kinds of repository:**

| Kind | What it is | MediaWiki counterpart |
|---|---|---|
| `tenant` | Another tenant on this instance | A shared database and directory (`ForeignDBRepo`), as a farm's commons wiki |
| `mediawiki` | Any MediaWiki Action API endpoint, Wikimedia Commons above all. A Triplespace tenant on another instance is one too, since it serves the Action API | `ForeignAPIRepo` (InstantCommons) |

**A `tenant` repository** is read directly: the source tenant's `view.file` and `view.file_version` rows, its bytes from its own scope, its ACLs evaluated as the source tenant's, so a file deleted there is missing here. The source tenant must set `files.share = on` in its `site` configuration; under `providers.between_tenants = operator` only the operator can set it, and a provider's reader list applies ([0028](0028-tenancy-policy.md) §5). A private tenant cannot share files, as it cannot be a provider ([0018](0018-tenants.md) §5).

**A `mediawiki` repository** has a `mode`:

| Mode | Bytes | Metadata | Reader privacy |
|---|---|---|---|
| `proxy` *(default)* | Fetched by the server from the repository and served from the media base (§7), cached under `cache/{repo}/` with a TTL (`repo.cache_ttl`, default 7 days) and a size cap with least-recently-used eviction | `prop=imageinfo` and the rendered description from `action=parse`, through the live upstream client: cached, rate-limited, User-Agent and `maxlag` set, never logged ([0012](0012-api-requirements.md) §6) | Readers' addresses never reach the repository |
| `link` | Not touched. Pages carry the repository's own URLs | As `proxy` | The repository sees every reader, as InstantCommons hotlinking does |
| `mirror` | Copied into the blob store and kept | Written as `upload` records with action `import` into an instance partition **`files/{repo}`** | As `proxy` |

**`link` is the one place bytes do not pass through the binary.** They are not the instance's bytes, and the access check still decides whether the instance shows them at all: a taken-down hash (§10) is never linked, and a file the repository deleted is missing. An operator who wants every byte checked uses `proxy` or `mirror`.

**A mirror** follows [0002](0002-source-graphs-and-mass-ingest.md): `files/{repo}` is an instance mirror partition (`hashed`, history `latest` by default, export `public` without bytes), synced by an instance job. It mirrors the files that are **used**: referenced by a page of any tenant using the repository, or by a `commonsMedia` value in such a tenant's resolved view (§12). A file unused for `repo.mirror_grace` (default 30 days) is tombstoned. Upstream deletions, read from the repository's deletion log and event stream as [0011](0011-logs.md) §4 reads Wikidata's, are tombstoned too. Compaction erases tombstoned records, and their bytes go when unreferenced (§9). Mirroring keeps a repository's files available when the repository is down and makes them verifiable, at the cost of storage; most instances will proxy.

**What a foreign file looks like.** `File:Example.jpg` with no local page shows the repository's description, sanitized ([0019](0019-discussions.md) §5's sanitizer) and labelled with its source, as MediaWiki shows "This file is from Wikimedia Commons". A **local page** for a foreign file holds local text and statements ([0038](0038-page-metadata-and-categories.md) §1) shown beside the foreign description: local annotations on someone else's file, which is the foreign-entity pattern of [0002](0002-source-graphs-and-mass-ingest.md) applied to files. Its talk page is local.

**Images in mirrored pages follow the same policy.** An `<img>` in the HTML of a page served by a page repository ([0053](0053-mirrored-pages.md) §2) is served by the first file repository in `files.repos` that holds the file, in that repository's mode, and its `File:` link goes to the local file title; an image that is no repository's file is fetched through the page repository's `media_hosts` allow-list in `proxy` mode and dropped otherwise. The attribution line a page repository's page carries ([0053](0053-mirrored-pages.md) §9) is this section's attribution rule applied to whole pages.

**API.** `meta=filerepoinfo` lists the local repository and every configured one with MediaWiki's fields (`name`, `displayname`, `rootUrl`, `local`, `url`, `thumbUrl`, `initialCapital`, `scriptDirUrl`, `canUpload`, `fetchDescription`, `descBaseUrl`); `prop=imageinfo` reports `imagerepository` as `local` or the repository's name, and URLs per the mode.

### 12. Media data types (uses 0003 §9 and 0038 §1)

**`commonsMedia`** values are file names on Wikimedia Commons. The tenant `site` setting `files.commons_repo` names the repository that resolves them, by default the one named `commons` if it is configured. Values then render as thumbnails in the statement UI ([0003](0003-statement-ui.md) §9) through that repository's mode, and a local write is validated against it, as Wikibase validates against Commons; if the repository cannot be reached the write is refused with `upstream-unavailable` ([0012](0012-api-requirements.md) §6). Mirrored values are never validated: they are upstream's. With no repository configured, values render as links to Commons, as they do now.

**`localMedia`** is a new data type, with the ID and value shape of the WikibaseLocalMedia extension so that adopted Wikibases ([0035](0035-adopting-a-wikibase.md)) that used it keep their values: a string naming a file in the tenant's own File namespace, validated at write, rendered as a thumbnail, and emitted in RDF, as the extension does, as an IRI to the file under `{base}/wiki/Special:FilePath/`. It is the data type for pointing an item at a file the tenant holds.

**Usage.** A file's usage (§13) includes the entities whose statements name it, listed apart from the pages that embed it.

`geo-shape` and `tabular-data` name pages in Commons' Data namespace, not files, and are left open.

### 13. Wikitext and markdown (amends 0008 §8)

The wikitext subset of [0008](0008-namespaces-and-document-pages.md) §8 gains:

| Syntax | Meaning |
|---|---|
| `[[File:Name]]`, `[[File:Name\|options\|caption]]` | Embeds the file. Options: `thumb` (`thumbnail`), `frame`, `frameless`, `border`; `{w}px`, `x{h}px`, `{w}x{h}px`, `upright`, `upright={factor}`; `left`, `right`, `center`, `none`; `baseline`, `middle`, `top`, `bottom` and the other vertical alignments; `alt=`, `link=`, `page=`, `class=`. The last unnamed option is the caption |
| `[[:File:Name]]` | A link to the description page, not an embed |
| `[[Media:Name]]`, `[[Media:Name\|text]]` | A link to the bytes (§7) |
| `<gallery>` | One file per line with an optional caption; `mode=traditional\|packed\|nolines`, `widths=`, `heights=`, `perrow=`, `caption=` |

A missing file renders as a red link to `Special:Upload?wpDestFile={name}`. External image URLs are never embedded, as MediaWiki's `$wgAllowExternalImages` defaults to false: the instance does not make readers' browsers fetch from third parties.

**Markdown** ([0019](0019-discussions.md) §5) gains CommonMark image syntax whose target is a file title (`File:Example.jpg` as the link destination of `![alt text]`), which embeds a thumbnail at the default width; any other target renders as a link, never an image.

**File usage is a projection**, `view.file_link`, MediaWiki's `imagelinks`: rows of (page, file name) from the latest revision of every page, keyed by name rather than page ID because the target may be foreign or missing. It serves `prop=images`, `list=imageusage`, `Special:WhatLinksHere` and the file page's usage section, and it is what `mirror` mode (§11) counts as use.

### 14. Import, adoption, export and verification (extends 0008 §9, 0035 §2 and 0006 §9)

*Changed by A7.*

**Import** ([0008](0008-namespaces-and-document-pages.md) §9) accepts MediaWiki XML exports with `<upload>` elements, from embedded contents or from the source wiki's URLs, and a directory import, `triplespace-cli files import`, which does what MediaWiki's `importImages.php` does: one upload per file, with a summary and description text from options or a sidecar file. Imported versions are `upload` records with action `import`, the upstream timestamp and uploader, projected as `import/upload` ([0011](0011-logs.md) §6.1).

**A fork's files** are copied on request, never by default: the file job of [0054](0054-forking-a-mirrored-page.md) §7 imports each file a forked page's render uses, the latest version or every version, as `import` uploads attributed to their upstream uploaders, with the description page forked, skipping files already local or held by a `mirror` repository; it needs `upload` and `reupload-shared`.

**Adoption** ([0035](0035-adopting-a-wikibase.md) §2) of a wiki with files reads its file tables (`image`, `oldimage`, or `list=allimages` and `prop=imageinfo&iilimit=max` from its API) and its upload directory (`images/` with `archive/` for old versions), and writes each version as an `import` upload on the file page, which keeps its source page ID ([0035](0035-adopting-a-wikibase.md) §4). Deleted files in the source's `filearchive` are imported as deleted versions, so that the adopted wiki's administrators keep what they could restore before.

**Export bundles** ([0006](0006-log-integrity-and-erasure.md) §9) gain `--blobs include|list|omit`: the bytes of every referenced hash under `blobs/sha256/…` in the bundle, a list of hashes only, or nothing. Moving a tenant ([0018](0018-tenants.md) §10) uses `include`. Public dumps omit bytes; a dump of a repository's files is a separate artifact, if an instance wants one.

**Verification** gains a blob check, at two depths: `presence`, that every unerased `upload` record's hash has an object in its scope; and `full`, that each object's bytes hash to its key. An object with no reference is reported as an orphan. A missing object with no `erase` accounting for it is a failure, as a missing body is ([0006](0006-log-integrity-and-erasure.md) §9). `full` reads every byte, so it is the check an operator schedules, not one `verify` runs by default.

### 15. Rate limits (extends 0024 §5)

The rate-limit classes of [0024](0024-subsidiary-accounts.md) §5 gain MediaWiki's three for files: **`upload`**, per actor and per IP; **`renderfile`**, thumbnails rendered on a miss, per IP; and **`renderfile-nonstandard`**, renders whose transform is not in `files.thumb_widths` (a `page=` or time offset). Serving bytes already stored is not rate-limited.

### 16. Log events (amends 0011 §6.1)

| Log type/action | Comes from | Visible to |
|---|---|---|
| `upload/upload`, `upload/overwrite`, `upload/revert` | An `upload` record with that action | Everyone |
| `import/upload` | An `upload` record with action `import` (§14) | Everyone |
| `delete/delete`, `delete/revision` with `type=oldimage` | A `read` ACL on a file page, or a `record` ACL on an `upload` record (§9) | Everyone, as [0023](0023-moderation.md) §3 |
| `takedown/takedown`, `takedown/reinstate` *(new)* | The `blob` ACL in the instance `log` and its retirement (§10), projected into the log of each tenant with an affected version, on the affected file pages | Everyone; without reason or notice when `confidential` |
| `takedown/expunge` *(new)* | An `expunge` record (§10), in the instance's own log only | `ts-viewtakedown` |
| `erase/erase` | The erasures of §9 and §10, as for any `erase` | As [0016](0016-permissions-and-access-control.md) §6 |

### 17. API (extends 0012 §4 and §5)

**Action API**, in MediaWiki's shape because Pywikibot, the Commons upload tools and the mobile apps speak it:

| Module | Behaviour |
|---|---|
| `action=upload` | `filename`, `file`, `text`, `comment`, `ignorewarnings`, `stash`, `filekey`, chunked `offset`/`filesize`/`chunk`, `checkstatus`; `url` only where enabled (§5). Warnings and errors as §5 |
| `action=filerevert` | `filename`, `archivename`, `comment`: writes a `revert` upload (§2). Needs `reupload` |
| `action=delete` | Gains `oldimage` for one old version (§9) |
| `action=undelete` | Gains `fileids` |
| `action=revisiondelete` | Gains `type=oldimage` and `type=filearchive` (§9) |
| `action=move` | File pages move with `movefile`; the bytes do not move, since nothing is keyed by title |
| `prop=imageinfo` | `iiprop=timestamp\|user\|userid\|comment\|parsedcomment\|canonicaltitle\|url\|size\|dimensions\|sha1\|mime\|mediatype\|metadata\|commonmetadata\|extmetadata\|archivename\|bitdepth\|badfile`, `iiurlwidth`/`iiurlheight` for `thumburl`; `iiprop=sha256` is added. Hidden and erased fields are flagged as [0023](0023-moderation.md) §5 flags them |
| `prop=stashimageinfo`, `list=mystashedfiles` | The viewer's own stash |
| `list=allimages`, `prop=images`, `list=imageusage`, `prop=fileusage`, `prop=duplicatefiles` | Over `view.file`, `view.file_version` and `view.file_link`, per tenant |
| `list=filearchive` | Deleted versions, for the deletion group |
| `meta=filerepoinfo` | §11 |
| `meta=siteinfo` | `general.maxuploadsize`, `fileextensions`, `siprop=triplespace` with the media base, separation and same-origin flag (§4, §7) |

`action=imagerotate` is not offered; rotation is a new upload.

**REST**, under `rest.php/triplespace/v0`: `GET /file/{title}` (current version, MediaWiki REST's `v1/file` shape), `GET /file/{title}/versions`, `POST /file/{title}/versions/{revid}/erase`, `GET /file/{title}/usage` (pages and entities, §12), `GET /blob/{sha256}/references` (for the deletion group, within the tenant). At the farm base, for the operator rights of §21: `GET`, `POST /takedowns`, `GET /takedowns/{id}`, `POST /takedowns/{id}/reinstate`, `POST /takedowns/{id}/expunge`, and `POST /expunge`, which takes down and expunges in one request.

**Every response is redacted for its viewer** ([0012](0012-api-requirements.md) §8), and the privacy test gains cases for files: no URL, thumbnail URL, hash or metadata of a version the viewer may not read is returned by any route, and no media route serves its bytes.

### 18. Site UI (extends 0010 §2 and §5)

- **The file page** shows, in the frame of [0010](0010-site-ui.md) §2: the file (thumbnail, or player for audio and video), its dimensions and size, the description text, a **File history** table of versions with thumbnail, date, uploader, dimensions, size and summary, with revert and delete actions for those with the rights; **File usage**, pages and entities; **Metadata**; the page-data tab of [0038](0038-page-metadata-and-categories.md) §7; and, for a foreign file, its source. A taken-down or expunged version shows the operator's notice in place of the file.
- **History** ([0010](0010-site-ui.md) §5) interleaves uploads with text and statement revisions, each row labelled by kind.
- **Special pages**, where MediaWiki users expect them: `Special:Upload` (chunked and resumable through the stash), `Special:ListFiles`, `Special:NewFiles`, `Special:MIMESearch`, `Special:MediaStatistics`, `Special:ListDuplicatedFiles`, `Special:FileDuplicateSearch`, `Special:UnusedFiles`, `Special:UncategorizedFiles`, `Special:FilePath`, and in `Special:Undelete` an **Erase permanently** action for `ts-erase`.
- **Operator pages** at the farm base: `Special:Takedowns` (list, file, reinstate, expunge), `Special:Takedown/{id}` (the notice, every affected tenant and page, the bytes behind a click-to-reveal for `ts-viewtakedown`), and `Special:Log/takedown`.

### 19. Search (extends 0014 §7)

File pages are documents in the `pages` index like other pages, with the current readable version's fields: `file_media_type`, `file_mime`, `file_size`, `file_width`, `file_height`, `file_resolution`, `file_bits` and `file_text` (text the metadata projection extracts from PDFs and SVGs), supporting CirrusSearch's `filetype:`, `filemime:`, `filesize:`, `fileres:`, `filew:` and `fileh:` keywords. A version that becomes unreadable (§8) is removed from the document, as hidden text is. Foreign files are not indexed; a local page annotating a foreign file is (§11), with the foreign file's fields.

### 20. Storage (extends 0013 §5.6 and §7)

| Schema | Table | Holds |
|---|---|---|
| `view` | `file` | `(tenant, page_id, current_partition, current_offset, sha256, mime, media_type, size, width, height)`: the current version of each file page |
| `view` | `file_version` | `(tenant, page_id, partition, offset, revid, action, sha256, sha1, size, mime, media_type, width, height, pages, duration, metadata jsonb, visibility)`, indexed by `sha256` and by `sha1` |
| `view` | `blob` | `(scope, sha256, size, refs, readable_refs, unreadable_since, state)`: the reference count of §9, `unreadable_since` for reclamation, `state` `present` or `gone` |
| `view` | `file_link` | `(tenant, from_page_id, file_name)`: file usage (§13) |
| `view` | `file_block` | `(alg, hash, takedown_id, expunged boolean)`: from `blob` ACLs and `expunge` records (§10), read on every upload and every serve |
| `view` | `file-repo` in `registry` | Repository configuration (§11) |
| `ops` | `upload_stash` | `(key, tenant, actor_key, sha256, sha1, size, mime, warnings, created, expires)` (§5) |
| `ops` | `blob_delete` | Objects queued for deletion by erasure, expunge and the orphan sweep, with attempts, so that a store outage delays deletion and never forgets it |

**Projection order** ([0013](0013-postgres-storage.md) §7): the file projection runs in step 2 with pages, after the ACL projection, since `visibility` and `readable_refs` read ACLs; it is synchronous for interactive writes, so an upload, deletion or takedown is effective when it commits. Metadata extraction (§6) runs behind, as the search indexer does, and a version is served before its metadata is ready.

**Caches** ([0014](0014-caches-and-search.md) §10): L1 keys `fi:{tenant}:{page id}:{gen}` for a file's imageinfo, and `rf:{repo}:{name}` for a foreign file's, with the repository cache TTL; L0 holds `file_block` in full, since it is small and read on every request; Cache-Tags `blob:{scope}:{h}` and `file:{tenant}:{page id}` at L2. Deletion, hiding, takedown and erasure take the purge path of [0014](0014-caches-and-search.md) §5 and also purge these tags.

### 21. Permissions (extends 0016 §2)

| Permission | Governs | Default groups |
|---|---|---|
| `upload` | Uploading a new file; stashing | `autoconfirmed` |
| `reupload` | Overwriting an existing file; `filerevert` | `autoconfirmed` |
| `reupload-own` | Overwriting a file one uploaded oneself | `autoconfirmed` |
| `reupload-shared` | Shadowing a foreign repository's file with a local one (§11) | `sysop` |
| `upload_by_url` | Uploading from a URL, where enabled (§5) | none |
| `movefile` | Moving a file page | `autoconfirmed` |
| `delete`, `undelete`, `deleterevision`, `suppressrevision`, `ts-erase` | As [0016](0016-permissions-and-access-control.md) §2 and [0023](0023-moderation.md) §11, applied to file pages and versions (§9) | unchanged |
| `ts-takedown` *(new)* | Taking down and reinstating (§10). Evaluated on the primary tenant | `owner` |
| `ts-expunge` *(new)* | Expunging (§10). Evaluated on the primary tenant | `owner` |
| `ts-viewtakedown` *(new)* | Seeing taken-down bytes, confidential notices and `takedown/expunge` events | `owner` |

The three operator rights are held only through the primary tenant ([0018](0018-tenants.md) §1) or through a global group ([0028](0028-tenancy-policy.md) §3); held on any other tenant they grant nothing. `tenancy.toml` adds them to `steward` and `platform-admin`.

### 22. Scatterbase

Scatterbase gets `scatter-blob` whole: its blobs and Triplespace's files are the same objects under the same layout, so a Scatterbase instance can use a directory or a bucket exactly as Triplespace does. The removal vocabulary of §8 is available to it; whether a signed claim's blob may be expunged is its policy, as [0006](0006-log-integrity-and-erasure.md) §7 left erasure of claims. File pages, thumbnails and repositories are Triplespace's.

### 23. Crates (amends 0005 §2)

*Changed by A1.*

*Superseded by [0005](0005-crate-organization.md) §2 (A1).*

[0005](0005-crate-organization.md) §2 keeps the crate table that CI checks, with `scatter-blob`, `scatter-files`, `triplespace-files` and every change this section listed. The table this section first gave is in A1.

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
- **Q6. Mirroring at Wikidata scale.** A full Wikidata mirror references millions of Commons files through P18 and similar properties; whether `used` should count mirrored statements by default, or only local use.
- **Q7. MediaInfo.** If an instance mirrors Commons' MediaInfo entities (`WDM`, [0000](0000-init.md)), whether a foreign Commons file's page should show them as its page data.
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
