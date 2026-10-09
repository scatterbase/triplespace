# 12. Files and media

This chapter covers files: the File and Media namespaces, the upload record, storage scopes, uploading and its checks, derived metadata and thumbnails, serving behind an access check, the removal vocabulary and both removal paths, foreign file repositories, the media data types, files in wikitext and markdown, and import, adoption, export and verification. It then covers MediaInfo: the `mediainfo` slot, a File page's statements as `M{page ID}` over the Wikibase Action API, captions, and Commons as a source. It assumes the log and erasure from [01](01-log-and-records.md), the blob store, the file tables and the cache layers from [03](03-storage-caches-and-search.md), the derived `M` ID from [04](04-entities-and-identifiers.md), source graphs from [05](05-providers-and-ingest.md), instance partitions and prerogatives from [08](08-tenants-and-instances.md), ACLs and the upload rights from [09](09-security-and-moderation.md), and document pages from [10](10-pages-and-content-models.md). Mirrored pages are in [13](13-mirrored-pages.md); the file API, log events, special pages and site UI are in [18](18-api.md), [16](16-logs-feeds-and-notifications.md), [21](21-special-pages.md) and [19](19-site-ui.md).

## 1. The File and Media namespaces, and upload records

*Sources: [0039](../decisions/0039-files-and-media.md) §1, §2.*

### 1.1 Three namespaces

*Sources: [0039](../decisions/0039-files-and-media.md) §1.*

Three reserved numbers are implemented, as [0008](../decisions/0008-namespaces-and-document-pages.md) §2 rule 1 allows:

| ID | Canonical name | Kind | Notes |
|---|---|---|---|
| −2 | Media | Virtual | `Media:X` resolves to the current version of the file `File:X`, local or foreign (§6), and links to its bytes. It never names a page |
| 6 | File | `pages`, with `uploads = true` | A `pages` namespace ([0041](../decisions/0041-content-models.md) §4) whose pages may also carry uploads (§1.3). Content model `wikitext` only, as MediaWiki's description pages are, and a second slot, `mediainfo` (model `wikibase-mediainfo`), which presents the page's statements as the MediaInfo entity `M{page ID}` over the Wikibase Action API (§8) |
| 7 | File talk | Composite | Enabled with its subject, as [0008](../decisions/0008-namespaces-and-document-pages.md) §2 rule 2 requires |

**An `uploads` namespace is a `pages` namespace plus uploads.** Everything 0008 says of document pages holds: page IDs, full-text revisions, moves, the title index, statements ([0038](../decisions/0038-page-metadata-and-categories.md) §1). A file page may exist with no upload, as in MediaWiki ("No file by this name exists"); it may also exist with no upload because it describes a **foreign** file of the same name (§6), in which case its text and statements are local annotations on someone else's file.

### 1.2 File names and redirects

*Sources: [0039](../decisions/0039-files-and-media.md) §1.*

**The `file-name` normalizer** is `first-letter` with MediaWiki's file-name rules: the title must end in an extension the type registry allows (§3.1), may not contain `/`, `\`, `:` (after the prefix) or control characters, and is at most 240 bytes in UTF-8. Extensions are matched without regard to case and kept as written, so `File:Example.JPG` and `File:Example.jpg` are different titles, as in MediaWiki.

**A File page may be a redirect** ([0051](../decisions/0051-page-redirects.md) §4): the file lookup of §6.1 follows a local redirect page one hop before trying repositories, as MediaWiki's `RepoGroup::findFile` does, so `[[File:Old name.jpg]]` embeds the renamed file, and `movefile` leaves one as any move does.

### 1.3 Upload records

*Sources: [0039](../decisions/0039-files-and-media.md) §2.*

A file version is a record of the payload type **`scatter:v0/upload`**, appended to the tenant's `pages` partition and keyed by the file page's ID. Like every record in `pages` it takes a revision ID ([0013](../decisions/0013-postgres-storage.md) §6) and needs a base offset ([0006](../decisions/0006-log-integrity-and-erasure.md) §8). So **a file page has one history** in which text revisions, statement change sets and uploads interleave in the order they were made ([0038](../decisions/0038-page-metadata-and-categories.md) §1).

| Part | Holds |
|---|---|
| Content | `action` (`upload`, `overwrite`, `revert`, `import`); `sha256` (32 bytes), the address of the bytes ([03](03-storage-caches-and-search.md) §8); `sha1` (20 bytes), for MediaWiki compatibility only; `size`; `mime`; `media_type` (MediaWiki's `BITMAP`, `DRAWING`, `AUDIO`, `VIDEO`, `MULTIMEDIA`, `OFFICE`, `TEXT`, `ARCHIVE`, `3D`); `width`, `height`, `pages` and `duration` where the type has them; for `revert`, the coordinates of the version reverted to; for `import`, the upstream timestamp and uploader ([0007](../decisions/0007-actor-identity.md) §5) |
| Comment | The upload summary |
| Attestation | Who uploaded, as for every record |

**The log commits to the bytes without holding them.** The content part carries the SHA-256 of the bytes, and the content part is committed to by the header ([0015](../decisions/0015-record-format-and-partition-registry.md) §1). A verifier holding the record and the blob can check one against the other (§9); a verifier without the blob can still check the log. The content part is salted like every part, so the hash of an erased version cannot be read from its header.

**First upload.** Uploading to a title with no page appends a `page` `create` record carrying the description text (MediaWiki's "initial page text") and the `upload` record, in one transaction. A later upload appends only the `upload` record: it is the revision, and no null text revision is written as MediaWiki writes one.

**The current version** is the newest `upload` record of the page. `revert` re-points the page at an earlier version's bytes by writing a new record with that version's hash; since bytes are content-addressed, a revert stores nothing.

**Extended metadata is not in the record.** EXIF, XMP and IPTC fields, colour profiles and embedded text are derived from the bytes (§3.4). The record holds only what describes the version on its own: what `prop=imageinfo` returns by default, and what an export without blobs still needs.

## 2. Storage scopes

*Sources: [0039](../decisions/0039-files-and-media.md) §4.*

Bytes live in the blob store of [03](03-storage-caches-and-search.md) §8, addressed by SHA-256 and keyed within a scope. A **scope** is the part of the store a reference counts against. The instance `site` setting **`files.separation`** decides what a scope is:

| Value | Scope | Effect |
|---|---|---|
| `tenant` *(default)* | One per tenant: its slug | Each tenant's bytes are stored apart. Identical bytes uploaded to two tenants are stored twice. A tenant's files leave with it as a set of objects under one prefix (§9) |
| `instance` | One for the instance: `_instance` | Identical bytes are stored once for every tenant. A reference from any tenant keeps them |

**Why tenant by default.** Shared storage is observable: an upload can learn, from its timing or a warning, that some tenant has a file, and a private tenant's file is then not private. It also makes a tenant's files depend on other tenants' erasures (§5.3) and complicates moving a tenant away ([0018](../decisions/0018-tenants.md) §10); the files many tenants share are usually a shared repository's, which §6 serves once without copying.

**Under `instance`,** every warning and listing stays per tenant: `duplicate` warnings and `prop=duplicatefiles` name only the uploader's own tenant's files, and an upload of bytes already stored still runs every check and takes as long as reading them. What the setting cannot hide is that the store skips a write.

**Changing the setting** is a job (`triplespace-cli files rescope`) that copies objects into the new scopes, re-derives `view.blob` ([03](03-storage-caches-and-search.md) §4.9), and switches reads when it finishes. It needs `owner`, as the identity switches of [0028](../decisions/0028-tenancy-policy.md) §2 do.

## 3. Uploading

*Sources: [0039](../decisions/0039-files-and-media.md) §5, §6.*

### 3.1 The stash and the type registry

*Sources: [0039](../decisions/0039-files-and-media.md) §5.*

**The stash** holds uploads that have no record yet: a chunked upload being assembled, or a file stashed with warnings for the uploader to confirm. MediaWiki's `filekey`, `stash` and chunked `offset`/`filesize` parameters work against it. A stash entry belongs to one actor, is not readable by anyone else, and expires (`files.stash_ttl`, default 48 hours); its row is in `ops.upload_stash` ([03](03-storage-caches-and-search.md) §4.9). The write path from stash to `sha256/` key to record is in [03](03-storage-caches-and-search.md) §8.

**The type registry**, `docs/registry/file-types.toml`, lists each permitted type: extensions, MIME type, MediaWiki media type, the magic-byte signature used to detect it, whether it may be served `inline` or only as an `attachment` (§4.4), and its thumbnailer (§3.4). The instance `site` setting `files.types` lists the types the instance allows, by default the registry's `default = true` entries (raster images, SVG, PDF, common audio and video); a tenant `site` setting may narrow that list but not widen it. Types MediaWiki prohibits outright (executables, HTML, JavaScript, `.php` and the rest of `$wgProhibitedFileExtensions`) are never permitted, whatever configuration says.

`files.stash_ttl`, `files.max_size`, `files.thumb_widths` and `files.thumb_concurrency` are tenant `site` settings ([23](23-configuration-and-registry.md) §3).

### 3.2 Checks and warnings

*Sources: [0039](../decisions/0039-files-and-media.md) §5.*

**Checks**, in order, with MediaWiki's error codes where it has them:

1. The title's extension is permitted (`filetype-banned`), and the size is within `files.max_size` (`file-too-large`).
2. The type detected from the bytes matches the extension (`filetype-mime-mismatch`).
3. The bytes contain nothing a browser would run: no HTML or script markers in the first kilobytes of any type (`verification-error` with `uploadscripted`), and for SVG an XML parse that refuses scripts, event-handler attributes, `javascript:` and external references, `<foreignObject>` and entity declarations (`uploaded-hostile-svg`, `uploaded-script-svg`).
4. The hash is not blocked by a takedown or expunge (§5.4, §5.5): `ts-file-blocked`, which says only that the file cannot be uploaded.
5. Edit filters (§3.3).

**Warnings**, which stop the upload in the stash until the uploader confirms with `ignorewarnings`: `exists` (the title has a file), `exists-normalized`, `duplicate` (the same bytes are already a current version in this tenant), `duplicate-archive` (they were a deleted version), `was-deleted` (the title was deleted), `nochange` (an overwrite with identical bytes), `badfilename`. Uploading a local file over a foreign file's name (§6.1) is refused with `fileexists-shared-forbidden` without `reupload-shared`.

The upload rights and their defaults are in [09](09-security-and-moderation.md) §2.2.

### 3.3 Edit filters and upload by URL

*Sources: [0039](../decisions/0039-files-and-media.md) §5.*

**Edit filters run on uploads.** [0030](../decisions/0030-edit-filters.md) §2's contexts gain a third, **`upload`**, with AbuseFilter's file variables (`file_sha1`, `file_size`, `file_mime`, `file_mediatype`, `file_width`, `file_height`, `file_bits_per_channel`), `file_sha256`, the page variables of the text context for the title, and `action` set to `upload` or `stashupload`. Global filters ([0030](../decisions/0030-edit-filters.md) §8) run on every tenant's uploads, as on its edits.

**Upload by URL** (`upload_by_url`, `action=upload&url=`) is off by default and held by no group. An instance that enables it gets a fetcher that refuses private and link-local addresses, follows no redirect to them, and goes through the upstream client's rate limits ([0012](../decisions/0012-api-requirements.md) §6).

### 3.4 Derived data: metadata and thumbnails

*Sources: [0039](../decisions/0039-files-and-media.md) §6.*

**Metadata is a projection.** The file projection reads each new version's bytes once and writes its extracted metadata (EXIF, XMP, IPTC, page count, duration, bit depth, colour profile) to `view.file_version.metadata` ([03](03-storage-caches-and-search.md) §4.9). Like every `view` table it is rebuilt from the log, which here means from the log and the blob store. A version whose content part is erased has no bytes and no metadata.

**Thumbnails are cache, not data.** A thumbnail is rendered on first request, stored under `thumb/{h}/{transform}` in the same scope as its original, and served from there afterwards. It is never logged and never exported, and it can be deleted at any time and rendered again. Its key is the original's hash and a normalized **transform**: width, page or time offset, and output format. Requested widths are rounded up to the next entry of `files.thumb_widths`, so that a client cannot fill the store with every width; MediaWiki's `renderfile-nonstandard` limit ([07](07-actors-and-accounts.md) §6.2) covers what rounding does not. Rendering runs on a bounded pool (`files.thumb_concurrency`) with single flight per key ([0014](../decisions/0014-caches-and-search.md) §2).

**Thumbnailers** are registered per type. Two ship: raster images through the `image` crate, and SVG rasterized through `resvg`. Both are pure Rust under licences [0033](../decisions/0033-backend-stack.md) §1 accepts.

**Not yet.** Video posters and transcodes, PDF pages, audio waveforms and 3D previews are left open; until a thumbnailer exists for a type, its files show an icon.

## 4. Serving: every byte behind an access check

*Sources: [0039](../decisions/0039-files-and-media.md) §7.*

### 4.1 The access check and the media base

*Sources: [0039](../decisions/0039-files-and-media.md) §7.*

**Access to every Triplespace resource goes through an access check**; a public bucket behind a CDN that never asks who is reading is not used. The binary serves every original and every thumbnail, local or proxied, after evaluating the viewer's `read` on the version under [0016](../decisions/0016-permissions-and-access-control.md) §4 and [0023](../decisions/0023-moderation.md) §1, including the instance ACLs of §5.4. A version that fails the check is answered with 404, never 403, so the answer does not confirm that the bytes exist.

**The media base.** Files are served from the tenant `site` setting `media.base_url`, by default `{tenant base}/media`. A **separate origin** is strongly recommended (`https://upload.example.org/{slug}` for a farm), since a file served from the wiki's own origin runs with the wiki's cookies if a check in §3.2 misses something. `meta=siteinfo&siprop=triplespace` reports whether the media base is same-origin, and the site UI warns administrators while it is.

### 4.2 Routes

*Sources: [0039](../decisions/0039-files-and-media.md) §7.*

| Route under the media base | Serves |
|---|---|
| `/file/{h}/{name}` | Original bytes. `{name}` is the title without its prefix, used for `Content-Disposition` |
| `/thumb/{h}/{transform}-{name}` | A thumbnail, in MediaWiki's `320px-Example.jpg` shape |
| `/repo/{repo}/file/{sha1}/{name}`, `/repo/{repo}/thumb/{sha1}/{transform}-{name}` | A proxied foreign file (§6.3) |

`Special:FilePath/{name}` and `Special:Redirect/file/{name}` on the wiki redirect to the current version's URL, and `[[Media:…]]` links point at them, so a stable link never names a hash.

### 4.3 The check on a cookieless origin: signed URLs

*Sources: [0039](../decisions/0039-files-and-media.md) §7.*

A request for a version that `universe` may read needs no identity: the check is on the version's state (deleted, hidden, erased, taken down) and the tenant's `read` ACLs, and the answer is the same for everyone. A version that only some viewers may read (a deleted version shown to the deletion group, a file on a private tenant) is served only from a **signed URL**: the wiki origin performs the check and issues `?exp={time}&sig={mac}`, an HMAC over the scope, hash, transform, viewer's actor key and expiry under the instance's media key, valid for minutes. The media origin verifies the signature and evaluates the check again for that actor, so a deletion between issue and fetch still holds. The media key is deployment configuration, rotated with two keys accepted during rotation.

### 4.4 Headers and caching

*Sources: [0039](../decisions/0039-files-and-media.md) §7.*

**Headers.** Every response carries `X-Content-Type-Options: nosniff`, `Cross-Origin-Resource-Policy: cross-origin` (other sites may embed public files, as Commons allows; an instance may set `same-site`), and `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; img-src data:; sandbox`. A type the registry marks `inline` (raster images, audio, video, PDF) is served inline; every other type with `Content-Disposition: attachment`. SVG originals are served under the sandboxing CSP, and on a same-origin media base always as attachments; SVG thumbnails are rasterized and always inline. `Range` requests are honoured, for seeking in audio and video. `ETag` is the hash plus transform.

**Caching** follows [0014](../decisions/0014-caches-and-search.md) §6 ([03](03-storage-caches-and-search.md) §10). A public version is served with `Cache-Control: public, max-age=86400, s-maxage=2592000` and `Cache-Tag` values `blob:{scope}:{h}` and `file:{tenant}:{page id}`; any change that makes it unreadable (§5) purges both tags at L2, so the long `s-maxage` costs nothing in correctness. A signed URL is served `Cache-Control: private, no-store`. Bytes go from the store to the client as a stream; the binary never holds a whole file in memory.

## 5. Removing files

*Sources: [0039](../decisions/0039-files-and-media.md) §8, §9, §10.*

### 5.1 The vocabulary

*Sources: [0039](../decisions/0039-files-and-media.md) §8.*

Removal has two independent axes. **Who acts:** a tenant, through its own moderation, or the instance operator, whose action covers every tenant and which no tenant can undo. **How final it is:** a tombstone, which keeps the bytes and can be reversed, or destruction, which cannot. Hiding one part of a version is the third, finer tool. The words:

| | **Reversible** (tombstone; bytes kept) | **Permanent** (bytes destroyed) |
|---|---|---|
| **Tenant** (editorial; one page or version) | **Delete**, and **undelete** ([0023](../decisions/0023-moderation.md) §4) | **Erase** ([0006](../decisions/0006-log-integrity-and-erasure.md) §7), and **reclaim**, its automatic form after a retention period (§5.3) |
| **Instance operator** (prerogative; by content hash, across every tenant) | **Take down**, and **reinstate** (§5.4) | **Expunge** (§5.5) |

And, unchanged from [0023](../decisions/0023-moderation.md) §5, **hide** (and **suppress**) one part of one version: its bytes and metadata (`content`), its summary or its uploader.

- **Delete** is what MediaWiki means: the page, or one old version, is visible only to the deletion group, and an administrator can undelete it. It is a `read` ACL, nothing more.
- **Erase**: the version's content part is erased, its bytes are destroyed once nothing else references them (§5.3), and a verifiable gap remains in the history. Nobody can undo it. It is a tenant action, for a tenant's own legal or privacy obligation.
- **Reclaim** is erase done by the instance on a schedule, to versions a tenant deleted long ago.
- **Take down** is the operator's tombstone: the bytes become unreadable everywhere, by everyone, and cannot be uploaded again, until the operator **reinstates** them. It answers notices whose remedy is withdrawal of access with room for appeal, such as a DMCA notice.
- **Expunge** is the operator's destruction: every tenant's references to the bytes are erased and the bytes are destroyed at once, and the hash stays blocked. It answers obligations whose remedy is that the material no longer exist here: a GDPR erasure request, or illegal material.

**Two steps, or one.** Each permanent action may follow its reversible partner as a separate, later decision: delete, then erase from the deleted view; take down, then expunge from the takedown. Each may also be requested directly, and then the reversible record and the permanent one are written in one request, so that there is no moment when the material is destroyed but its tombstone is missing.

The word "purge" is not used for any of these: MediaWiki's `action=purge` refreshes a page's cache. "Erase" keeps the meaning [0006](../decisions/0006-log-integrity-and-erasure.md) gave it, and files only extend what it reaches.

### 5.2 Tenant removal: delete, hide and erase

*Sources: [0039](../decisions/0039-files-and-media.md) §9.*

**Deleting a file page** is [0023](../decisions/0023-moderation.md) §4 unchanged: a `read` ACL on the page, which covers its text, statements and every upload. Its versions stop being served (§4), leave search and file usage, and their L2 tags are purged.

**Deleting one old version** (MediaWiki's `action=delete&oldimage=`) is a `record` ACL on that `upload` record with no `parts`, so the whole version is hidden from everyone outside the deletion group. **Hiding** a version's bytes (MediaWiki's revision deletion of type `oldimage`) is a `record` ACL with `parts = [content]`; its summary or uploader likewise. The **current version** cannot be deleted or hidden on its own, as in MediaWiki: an administrator reverts to an earlier version first, which the UI offers as one action, or deletes the page.

**Erasing a version** is an `erase` record ([0006](../decisions/0006-log-integrity-and-erasure.md) §7; [01](01-log-and-records.md) §5) naming the `upload` record and, at least, its `content` part. Erasing the content part takes the version's hash out of the log, so the version no longer references its bytes. Erasure needs `ts-erase`, held by `suppress` ([0016](../decisions/0016-permissions-and-access-control.md) §2), and its reason class is `legal` or `privacy` ([0016](../decisions/0016-permissions-and-access-control.md) §6). The UI offers it on a deleted version, and directly as "Delete permanently" with the delete ACL in the same request (§5.1).

### 5.3 References, destruction and reclamation

*Sources: [0039](../decisions/0039-files-and-media.md) §9.*

**Bytes go when the last reference goes.** `view.blob` ([03](03-storage-caches-and-search.md) §4.9) counts, per scope and hash, the upload records whose content part is not erased, mirrored repository records (§6.4) and stash entries. When an erasure brings the count to zero, the projection queues the object and its thumbnails for **deletion at once**, not at the next sweep, and purges the hash's L2 tags. Deleted and hidden versions still count: their bytes must survive to be undeleted. Under `tenant` separation (§2), erasing a tenant's last reference therefore destroys the bytes. Under `instance` separation, another tenant's reference keeps them, and a request that the bytes leave the instance entirely is the operator's expunge (§5.5). The erasure form says which applies.

**Reclamation** is erasure on a schedule. A version that has been unreadable to `universe` (its page or record deleted, or its content part hidden or suppressed) continuously for **`files.reclaim_after`** (instance `site` setting, default 365 days; `never` disables it) is erased by the instance: an `erase` of its content part with reason class `operational`, in a nightly instance job. Reclamation is an instance prerogative ([0040](../decisions/0040-instance-prerogatives.md) §6; [08](08-tenants-and-instances.md) §8): the erasures carry the instance attestation, with the job record in the instance `log` as authority. A tenant may set a shorter period in its own `site` configuration, never a longer one. After reclamation an undeleted page keeps its text, statements and history; the reclaimed versions show as erased. Reclamation does not apply to a version that is unreadable only because of a takedown (§5.4), since a takedown may be reversed on appeal and the operator, not the schedule, decides when it becomes final.

### 5.4 Operator removal: the instance `log`, take down and reinstate

*Sources: [0039](../decisions/0039-files-and-media.md) §10.*

**Where operator records live.** The instance has its own `log` partition, beside its `config` ([0018](../decisions/0018-tenants.md) §2): `logged`, `full`, **internal**, attested by the primary tenant's actors as every instance-level record is ([0018](../decisions/0018-tenants.md) §4). It is the instance counterpart of a tenant's `log`, and like it is never in a public dump.

**A takedown is an ACL on the target kind `blob`**, at instance scope, so that it reuses everything [0023](../decisions/0023-moderation.md) §1 built ([09](09-security-and-moderation.md) §4) and is undone the same way:

| Target kind | Key | Restricts |
|---|---|---|
| `blob` | `acl:blob:{takedown id}` | `read` and `upload` of every file version, local or foreign, in every tenant and scope, whose `sha256` or `sha1` equals the hash in the record's content |

- **The key is an ID, not the hash.** The takedown ID is a random 64-bit number shown in decimal. The hash (algorithm and value) is in the content part, so no header carries the hash of something taken down ([0006](../decisions/0006-log-integrity-and-erasure.md) §3).
- **Content is an enclosure axis.** A `blob` target encloses every version with that hash, as a property encloses every snak that uses it ([0023](../decisions/0023-moderation.md) §2). Evaluation stays conjunctive, so a tenant's own ACLs cannot satisfy or override it.
- **The restriction names no group.** Nobody reads a taken-down version through any tenant, including the tenant's administrators and its suppressors. Operators review taken-down bytes through the instance's takedown pages ([19](19-site-ui.md)), which need `ts-viewtakedown`.
- **SHA-1 is accepted as a target** because a foreign repository in `link` mode (§6.3) reports only SHA-1, and an operator must be able to stop the instance showing a foreign file. Every local version records both hashes (§1.3), so a SHA-1 takedown covers local copies too.
- **Content of the record:** the hash; the reason class (`legal` or `privacy`, [0016](../decisions/0016-permissions-and-access-control.md) §6); a reference to the notice (a Lumen URL, a case or ticket number); and `confidential`, a flag. **Comment:** the public reason.

**What a takedown does.** In one request, synchronously ([0023](../decisions/0023-moderation.md) §10): every matching version becomes unreadable; their thumbnails are deleted (they are regenerated on reinstatement); their L2 tags, `blob:` and `file:`, are purged in every scope; they leave search, file usage and `prop=duplicatefiles`; and the hash is **blocked**, so an upload of the same bytes to any tenant is refused (§3.2). The bytes themselves stay in the store, and every tenant's records are untouched. The description pages stay: a takedown removes bytes, not a tenant's words. Each affected page shows, in place of the file, that it was removed by the instance operator, with the public reason unless the takedown is `confidential`.

**Reinstating** retires the ACL: a record with the same key and null content. Thumbnails are rendered again on demand, and the hash is unblocked.

**An operator prerogative under every preset.** Takedowns and expunges apply to every tenant whatever the tenancy policy, as the instance sitelink deny list and IP blocks do ([0028](../decisions/0028-tenancy-policy.md) §4, §8; [08](08-tenants-and-instances.md) §5.4). `ts-takedown`, `ts-expunge` and `ts-viewtakedown` are evaluated on the primary tenant ([09](09-security-and-moderation.md) §2.3).

### 5.5 Expunge

*Sources: [0039](../decisions/0039-files-and-media.md) §10.*

**Expunge** is a record of the payload type `scatter:v0/expunge`, in the instance `log`, keyed `expunge:{takedown id}` and carrying the same hash, reason class and authority. If no takedown exists for the hash, one is written in the same request, so that the material is unreadable before anything else happens. Then, in order:

1. **The bytes are destroyed at once,** in every scope that holds them: originals, thumbnails, stash entries and proxy caches. Their L2 tags are purged. This happens inside the request, so that the bytes are gone when the request returns.
2. **Every reference is erased.** A job appends, in each affected tenant's `pages` partition and in any `files/{repo}` mirror partition, an `erase` record naming the content parts of every `upload` record with that hash. Its reason class is the expunge's, and its authority reference is the expunge record's coordinates. The job is idempotent and resumes from the expunge record if interrupted.
3. **The hash stays blocked** while the expunge record exists, after the takedown it implies.

**The erasures are an instance prerogative** ([0040](../decisions/0040-instance-prerogatives.md) §6). Each carries the instance attestation of [0040](../decisions/0040-instance-prerogatives.md) §3 ([08](08-tenants-and-instances.md) §8.3): attributed to the instance operator, signed by the instance key, citing the `expunge` record as its authority. The operator who expunged is recorded only on the `expunge` record. A tenant can see that the instance erased its records, when, and the public reason; it cannot see the hash, and the reason class and authority reference follow [0016](../decisions/0016-permissions-and-access-control.md) §6.

### 5.6 Visibility, and what expunge cannot reach

*Sources: [0039](../decisions/0039-files-and-media.md) §10.*

**Visibility.** Takedown events are public in each affected tenant's log ([16](16-logs-feeds-and-notifications.md)), as Wikimedia publishes the DMCA notices it acts on; a `confidential` takedown appears as an event with no reason or notice. Expunges appear to tenants as their `erase/erase` events; the expunge record and its hash are visible only to `ts-viewtakedown`. Nothing about either reaches a public dump, because the instance `log` is internal and erased parts are erased. `takedown/expunge` and `reclaim/reclaim` are rows of the log-event table of [0011](../decisions/0011-logs.md) §6.1 ([16](16-logs-feeds-and-notifications.md)); `reclaim/reclaim` projects from the reclaiming binding record in the tenant `log` and is visible to everyone.

**What expunge cannot reach.** Copies outside the instance (backups, mirrors made by others, a CDN that ignored a purge) are reached only as [0006](../decisions/0006-log-integrity-and-erasure.md) §7 describes for erasure in general ([01](01-log-and-records.md) §5.5): the erase records travel on the feed, and backups are a runbook matter. One runbook item is specific to object storage: a bucket with **versioning** keeps a deleted object as a noncurrent version, so a versioned bucket must expire noncurrent versions within the backup window, or expunged bytes survive in it. The same applies to filesystem snapshots.

## 6. Foreign file repositories

*Sources: [0039](../decisions/0039-files-and-media.md) §11; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §2.*

### 6.1 Repositories and lookup

*Sources: [0039](../decisions/0039-files-and-media.md) §11.*

A **file repository** is a source of files a tenant can use by name without uploading them. It is configured as a `config` record of kind `file-repo`, keyed `file-repo:{name}`, either in a tenant's `config` or in the instance `config`, where a tenant refers to it by name; it can be supplied by a tenancy template ([0028](../decisions/0028-tenancy-policy.md) §8). The tenant `site` setting `files.repos` lists the repositories a tenant uses, in lookup order.

**Lookup follows MediaWiki.** A file title is looked up locally first, then in each repository in order, and the first match wins. A local file **shadows** a foreign one of the same name, and creating one needs `reupload-shared` (§3.2). `Special:ListDuplicatedFiles` and the file page say when a local file shadows a foreign one.

**Two kinds of repository:**

| Kind | What it is | MediaWiki counterpart |
|---|---|---|
| `tenant` | Another tenant on this instance | A shared database and directory (`ForeignDBRepo`), as a farm's commons wiki |
| `mediawiki` | Any MediaWiki Action API endpoint, Wikimedia Commons above all. A Triplespace tenant on another instance is one too, since it serves the Action API | `ForeignAPIRepo` (InstantCommons) |

### 6.2 `tenant` repositories

*Sources: [0039](../decisions/0039-files-and-media.md) §11.*

**A `tenant` repository** is read directly: the source tenant's `view.file` and `view.file_version` rows, its bytes from its own scope, its ACLs evaluated as the source tenant's, so a file deleted there is missing here. The source tenant must set `files.share = on` in its `site` configuration; under `providers.between_tenants = operator` only the operator can set it, and a provider's reader list applies ([0028](../decisions/0028-tenancy-policy.md) §5; [08](08-tenants-and-instances.md) §4). A private tenant cannot share files, as it cannot be a provider ([0018](../decisions/0018-tenants.md) §5).

### 6.3 `mediawiki` repositories and their modes

*Sources: [0039](../decisions/0039-files-and-media.md) §11.*

**A `mediawiki` repository** has a `mode`:

| Mode | Bytes | Metadata | Reader privacy |
|---|---|---|---|
| `proxy` *(default)* | Fetched by the server from the repository and served from the media base (§4), cached under `cache/{repo}/` with a TTL (`repo.cache_ttl`, default 7 days) and a size cap with least-recently-used eviction | `prop=imageinfo` and the rendered description from `action=parse`, through the live upstream client: cached, rate-limited, User-Agent and `maxlag` set, never logged ([0012](../decisions/0012-api-requirements.md) §6) | Readers' addresses never reach the repository |
| `link` | Not touched. Pages carry the repository's own URLs | As `proxy` | The repository sees every reader, as InstantCommons hotlinking does |
| `mirror` | Copied into the blob store and kept | Written as `upload` records with action `import` into an instance partition **`files/{repo}`** | As `proxy` |

`repo.cache_ttl` is a per-repository field of the `file-repo` record, with a default of 7 days for files.

**`link` is the one place bytes do not pass through the binary.** They are not the instance's bytes, and the access check still decides whether the instance shows them at all: a taken-down hash (§5.4) is never linked, and a file the repository deleted is missing. An operator who wants every byte checked uses `proxy` or `mirror`.

### 6.4 Mirrors

*Sources: [0039](../decisions/0039-files-and-media.md) §11.*

**A mirror** follows [0002](../decisions/0002-source-graphs-and-mass-ingest.md) ([05](05-providers-and-ingest.md)): `files/{repo}` is an instance mirror partition (`hashed`, history `latest` by default, export `public` without bytes), synced by an instance job. It mirrors the files that are **used**: referenced by a page of any tenant using the repository, or by a `commonsMedia` value in such a tenant's resolved view (§7.1). A file unused for `repo.mirror_grace` (default 30 days) is tombstoned. Upstream deletions, read from the repository's deletion log and event stream as [0011](../decisions/0011-logs.md) §4 reads Wikidata's, are tombstoned too. Compaction erases tombstoned records, and their bytes go when unreferenced (§5.3).

### 6.5 What a foreign file looks like

*Sources: [0039](../decisions/0039-files-and-media.md) §11.*

`File:Example.jpg` with no local page shows the repository's description, sanitized ([0019](../decisions/0019-discussions.md) §5's sanitizer) and labelled with its source, as MediaWiki shows "This file is from Wikimedia Commons". A **local page** for a foreign file holds local text and statements ([0038](../decisions/0038-page-metadata-and-categories.md) §1) shown beside the foreign description: local annotations on someone else's file, which is the foreign-entity pattern of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) applied to files. Its talk page is local.

**Images in mirrored pages follow the same policy.** An `<img>` in the HTML of a page served by a page repository ([0053](../decisions/0053-mirrored-pages.md) §2; [13](13-mirrored-pages.md)) is served by the first file repository in `files.repos` that holds the file, in that repository's mode, and its `File:` link goes to the local file title; an image that is no repository's file is fetched through the page repository's `media_hosts` allow-list in `proxy` mode and dropped otherwise. The attribution line a page repository's page carries ([0053](../decisions/0053-mirrored-pages.md) §9) is this section's attribution rule applied to whole pages.

**API.** `meta=filerepoinfo` lists the local repository and every configured one with MediaWiki's fields (`name`, `displayname`, `rootUrl`, `local`, `url`, `thumbUrl`, `initialCapital`, `scriptDirUrl`, `canUpload`, `fetchDescription`, `descBaseUrl`); `prop=imageinfo` reports `imagerepository` as `local` or the repository's name, and URLs per the mode.

### 6.6 Commons' MediaInfo on foreign files

*Sources: [0065](../decisions/0065-mediainfo-captions-and-commons.md) §2; [0039](../decisions/0039-files-and-media.md) §11.*

**A file served from a Commons file repository has its MediaInfo mirrored as the foreign entity `WDM{Commons page ID}`**, in the Wikidata provider's mirror graph as any `WD` entity ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2). The Wikidata provider's type `M` has its own **source** fields in `providers.toml` — `api`, `entity_data` and `dumps` pointing at Commons — so that the adapter fetches `M` entities from `commons.wikimedia.org` while attributing them to the same provider; the IRI stays `https://commons.wikimedia.org/entity/M…` as the registry already has it.

**Mirroring set**: `mediainfo.mirror` (site, default **`on-demand`**): a Commons file's MediaInfo is fetched through `Special:EntityData` ([0012](../decisions/0012-api-requirements.md) §6's upstream fetch) the first time the file is used by a local page or shown, written as a `put`, and kept current by Commons' EventStreams `mediainfo` changes as [0053](../decisions/0053-mirrored-pages.md) §6 keeps mirrored pages current; **`linked`** mirrors every file any local page or statement references; **`all`** loads Commons' MediaInfo JSON dump through the adapter, which is a Wikidata-scale job and off by default. `off` disables it.

**The local page for a foreign file** (§6.5) shows the mirrored statements and captions **as its page data**: the page's resolved view is the mirror's `WDM` state with the local graph's `add`/`override`/`remove` overlays ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7) applied, exactly as a foreign item's page shows Wikidata's statements with local corrections. The local page's own `M{local page ID}` names the local overlay; `wbgetentities&ids=M{local}` returns the resolved view and says `foreign: "WDM123"` in `triplespace`, and `wbgetentities&ids=WDM123` returns the mirror alone. **Captions on a foreign file** are overlaid the same way: a local caption `override`s the Commons one for that language on this tenant. Nothing is written to Commons; proposing a caption or statement back is the proposals ADR's job ([14](14-discussions.md)).

**Clusters**: a local `M` entity and the `WDM` it mirrors are **one subject by construction**, not a cluster: the local page *is* the foreign file's page here, with the ranged page ID of [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 ([04](04-entities-and-identifiers.md) §5.3) where the file is only inherited. `same-as` between `M` IDs is refused, as §8.2's "derived, not minted" implies.

## 7. Media data types, and files in wikitext and markdown

*Sources: [0039](../decisions/0039-files-and-media.md) §12, §13.*

### 7.1 `commonsMedia` and `localMedia`

*Sources: [0039](../decisions/0039-files-and-media.md) §12.*

**`commonsMedia`** values are file names on Wikimedia Commons. The tenant `site` setting `files.commons_repo` names the repository that resolves them, by default the one named `commons` if it is configured. Values then render as thumbnails in the statement UI ([0003](../decisions/0003-statement-ui.md) §9) through that repository's mode, and a local write is validated against it, as Wikibase validates against Commons; if the repository cannot be reached the write is refused with `upstream-unavailable` ([0012](../decisions/0012-api-requirements.md) §6). Mirrored values are never validated: they are upstream's. With no repository configured, values render as links to Commons.

**`localMedia`** is a data type with the ID and value shape of the WikibaseLocalMedia extension so that adopted Wikibases ([0035](../decisions/0035-adopting-a-wikibase.md)) that used it keep their values: a string naming a file in the tenant's own File namespace, validated at write, rendered as a thumbnail, and emitted in RDF, as the extension does, as an IRI to the file under `{base}/wiki/Special:FilePath/`. It is the data type for pointing an item at a file the tenant holds.

**Not yet.** `geo-shape` and `tabular-data` name pages in Commons' Data namespace, not files, and are left open.

### 7.2 Wikitext and markdown

*Sources: [0039](../decisions/0039-files-and-media.md) §13.*

The wikitext subset of [0008](../decisions/0008-namespaces-and-document-pages.md) §8 ([10](10-pages-and-content-models.md)) gains:

| Syntax | Meaning |
|---|---|
| `[[File:Name]]`, `[[File:Name\|options\|caption]]` | Embeds the file. Options: `thumb` (`thumbnail`), `frame`, `frameless`, `border`; `{w}px`, `x{h}px`, `{w}x{h}px`, `upright`, `upright={factor}`; `left`, `right`, `center`, `none`; `baseline`, `middle`, `top`, `bottom` and the other vertical alignments; `alt=`, `link=`, `page=`, `class=`. The last unnamed option is the caption |
| `[[:File:Name]]` | A link to the description page, not an embed |
| `[[Media:Name]]`, `[[Media:Name\|text]]` | A link to the bytes (§4) |
| `<gallery>` | One file per line with an optional caption; `mode=traditional\|packed\|nolines`, `widths=`, `heights=`, `perrow=`, `caption=` |

A missing file renders as a red link to `Special:Upload?wpDestFile={name}`. External image URLs are never embedded, as MediaWiki's `$wgAllowExternalImages` defaults to false: the instance does not make readers' browsers fetch from third parties.

**Markdown** ([0019](../decisions/0019-discussions.md) §5; [14](14-discussions.md)) gains CommonMark image syntax whose target is a file title (`File:Example.jpg` as the link destination of `![alt text]`), which embeds a thumbnail at the default width; any other target renders as a link, never an image.

### 7.3 File usage

*Sources: [0039](../decisions/0039-files-and-media.md) §13, §12.*

**File usage is a projection**, `view.file_link` ([03](03-storage-caches-and-search.md) §4.9), MediaWiki's `imagelinks`: rows of (page, file name) from the latest revision of every page, keyed by name rather than page ID because the target may be foreign or missing. It serves `prop=images`, `list=imageusage`, `Special:WhatLinksHere` and the file page's usage section, and it is what `mirror` mode (§6.4) counts as use. A file's usage also includes the entities whose statements name it, listed apart from the pages that embed it.

## 8. MediaInfo

*Sources: [0041](../decisions/0041-content-models.md) §6, §7; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §1.*

### 8.1 Slots: `main` everywhere, `mediainfo` on File pages

*Sources: [0041](../decisions/0041-content-models.md) §6.*

**Every page has one slot, `main`, holding its model.** That is all MediaWiki clients see of most pages, as on a MediaWiki without multi-content revisions.

**File pages have a second slot, `mediainfo`,** with the model `wikibase-mediainfo`. It holds the page's statements ([0038](../decisions/0038-page-metadata-and-categories.md) §1) presented as a MediaInfo entity (§8.2). This is WikibaseMediaInfo's arrangement on Commons: the entity namespace for `mediainfo` is the File namespace, slot `mediainfo`.

**Nothing in storage changes.** A slot is how the Action API presents a page's records. The statement change sets stay in the page's one history (§1.3); a revision that changed only statements appears as MediaWiki shows a revision that changed only a secondary slot (0038 §1), and on a File page that slot has a name.

**A slot is added only to adopt an existing MediaWiki contract.** Statements on other pages (document pages, threads) get no slot, because no MediaWiki or Wikibase client expects statements there. They stay on the REST routes of [0038](../decisions/0038-page-metadata-and-categories.md) §13.

### 8.2 The `M` ID

*Sources: [0041](../decisions/0041-content-models.md) §7.*

On File pages, 0038 §1's "no entity ID" is replaced by WikibaseMediaInfo's contract, which tools written for Structured Data on Commons expect.

**The ID is `M` followed by the page ID**: the File page with page ID 1234 has the MediaInfo ID `M1234`, as on Commons. It has the local form of [0017](../decisions/0017-entity-id-grammar.md) §1 ([04](04-entities-and-identifiers.md) §5.2), but it is **derived, not minted**: it is a name for one File page's statements. It is not a row in `view.entity`, never joins an identity cluster, cannot be the value of an entity data type, and goes when the page goes. `M` followed by the ID of a page that is not a File page, or does not exist, is a missing entity. Because adoption keeps source page IDs (§9), a file's `M` ID survives adoption from a wiki that numbered it.

### 8.3 Reading and writing over the Wikibase API

*Sources: [0041](../decisions/0041-content-models.md) §7.*

**Reading:**

- `wbgetentities&ids=M1234` returns a `mediainfo` entity in WikibaseMediaInfo's serialization: `type`, `id`, `title` (the File title), `ns` 6, `pageid`, `lastrevid`, `modified`, `labels`, `descriptions`, and the page's resolved statements under `statements`. Projected statements ([0038](../decisions/0038-page-metadata-and-categories.md) §2) are included, since they are part of what the page asserts.
- A File page with no statements is what WikibaseMediaInfo calls a *virtual* entity, and is returned as Commons returns one; the exact shape is pinned by a contract test against Commons.
- `wbgetentities` by `sites` and `titles` with a File title on the tenant's own site returns the MediaInfo entity, as it does on Commons. For other titles it finds the paired item, as 0038 §13 says.
- `prop=revisions&rvslots=mediainfo` (or `*`) returns the same JSON as of each revision, with `contentmodel` `wikibase-mediainfo` and `contentformat` `application/json`.
- `Special:EntityPage/M1234` and `Special:EntityData/M1234` resolve to the File page and its MediaInfo JSON.

**Writing.** The statement modules accept an `M` ID: `wbeditentity` (statements only), `wbcreateclaim`, `wbsetclaim`, `wbremoveclaims`, `wbsetclaimvalue`, `wbsetqualifier`, `wbremovequalifiers`, `wbsetreference` and `wbremovereferences`. Each writes a page change set exactly as the REST routes do (0038 §1): the same records, permissions, protection, filters and constraints. `baserevid` is the page's latest revision ID of any kind (text, statements or upload), which gives the base offset. A write to a projected statement is refused with `ts-derived-statement` (0038 §13).

**Unchanged:** REST stays `/page/{pageid}/statements` for every page, File pages included; the Wikibase REST API has no MediaInfo routes to follow. RDF stays as [0038](../decisions/0038-page-metadata-and-categories.md) §11 has it, with the page node as subject ([02](02-graphs-rdf-and-query.md) §5.4, §5.6). Statements on pages other than File pages stay out of the Action API.

### 8.4 Captions are the File page's terms

*Sources: [0065](../decisions/0065-mediainfo-captions-and-commons.md) §1; [0041](../decisions/0041-content-models.md) §7.*

**A File page's `mediainfo` slot carries labels and descriptions**, Commons' captions, in every language, as Wikibase terms. They are written by a **`terms`** operation on the page's change set in `pages` — the change-set payload already carries page statements ([0038](../decisions/0038-page-metadata-and-categories.md) §1), and gains terms for File pages only — and projected into `view.term` with `entity_id = 'M{page ID}'` ([03](03-storage-caches-and-search.md) §4.3), so every reader of terms (search, the label in the viewer's language, `wbgetentities`, the dump) sees them with no new table.

**Labels and descriptions are captions; aliases and sitelinks are not supported.** `wbsetlabel`, `wbsetdescription` and a `wbeditentity` carrying `labels` or `descriptions` on an `M` ID are accepted and append the `terms` operation. `wbsetaliases`, `wbsetsitelink` and a `wbeditentity` carrying aliases or sitelinks are refused with Wikibase's `not-supported` ("The requested feature is not supported by the given entity"), as Commons refuses them. 0038 §1's "terms stay out of page change sets" is narrowed to pages other than File pages, where the short description remains a page property ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6; [10](10-pages-and-content-models.md)).

**Where captions show**: the File page's header, under the title, in the viewer's language with the fallback chain; the caption editor is the term editor of the entity page ([0003](../decisions/0003-statement-ui.md) §8; [19](19-site-ui.md)). **Search** indexes captions as the File page's terms ([0014](../decisions/0014-caches-and-search.md) §7; [03](03-storage-caches-and-search.md) §11), so a search for "sunset over Lisbon" finds the file by caption. **RDF**: the page node `{base}/page/{page ID}` ([0038](../decisions/0038-page-metadata-and-categories.md) §11; [0041](../decisions/0041-content-models.md) §7) gains `rdfs:label` and `schema:description` per language, as Commons' SDC export has them; there is no separate `M` node. **Diffs and history**: a caption change is a page revision whose diff shows the terms, as an item's does.

## 9. Import, adoption, export and verification

*Sources: [0039](../decisions/0039-files-and-media.md) §14.*

**Import** ([0008](../decisions/0008-namespaces-and-document-pages.md) §9; [10](10-pages-and-content-models.md)) accepts MediaWiki XML exports with `<upload>` elements, from embedded contents or from the source wiki's URLs, and a directory import, `triplespace-cli files import`, which does what MediaWiki's `importImages.php` does: one upload per file, with a summary and description text from options or a sidecar file. Imported versions are `upload` records with action `import`, the upstream timestamp and uploader, projected as `import/upload` ([0011](../decisions/0011-logs.md) §6.1; [16](16-logs-feeds-and-notifications.md)).

**A fork's files** are copied on request, never by default: the file job of [0054](../decisions/0054-forking-a-mirrored-page.md) §7 ([13](13-mirrored-pages.md)) imports each file a forked page's render uses, the latest version or every version, as `import` uploads attributed to their upstream uploaders, with the description page forked, skipping files already local or held by a `mirror` repository; it needs `upload` and `reupload-shared`.

**Adoption** ([0035](../decisions/0035-adopting-a-wikibase.md) §2; [05](05-providers-and-ingest.md)) of a wiki with files reads its file tables (`image`, `oldimage`, or `list=allimages` and `prop=imageinfo&iilimit=max` from its API) and its upload directory (`images/` with `archive/` for old versions), and writes each version as an `import` upload on the file page, which keeps its source page ID ([0035](../decisions/0035-adopting-a-wikibase.md) §4). Deleted files in the source's `filearchive` are imported as deleted versions, so that the adopted wiki's administrators keep what they could restore before.

**Export bundles** ([0006](../decisions/0006-log-integrity-and-erasure.md) §9; [01](01-log-and-records.md) §8) gain `--blobs include|list|omit`: the bytes of every referenced hash under `blobs/sha256/…` in the bundle, a list of hashes only, or nothing. Moving a tenant ([0018](../decisions/0018-tenants.md) §10; [08](08-tenants-and-instances.md) §6) uses `include`. Public dumps omit bytes; a dump of a repository's files is a separate artifact, if an instance wants one.

**Verification** gains a blob check, at two depths: `presence`, that every unerased `upload` record's hash has an object in its scope; and `full`, that each object's bytes hash to its key. An object with no reference is reported as an orphan. A missing object with no `erase` accounting for it is a failure, as a missing body is ([0006](../decisions/0006-log-integrity-and-erasure.md) §9). `full` reads every byte, so it is the check an operator schedules, not one `verify` runs by default.
