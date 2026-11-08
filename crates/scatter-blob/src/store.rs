//! The store itself (0039 §3): put, get, ranged and streamed reads, staged writes that hash
//! as they go, copy-if-absent, deletion by key and by prefix, listing, and the full check.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use bytes::Bytes;
use futures::stream::{BoxStream, Stream, StreamExt, TryStreamExt};
use object_store::local::LocalFileSystem;
use object_store::{
    GetOptions, GetRange, ObjectStore, ObjectStoreExt as _, PutMode, PutOptions, WriteMultipart,
};

use crate::error::BlobError;
use crate::hash::{Digest, Hasher, Sha256};
use crate::key::{Key, Kind, Prefix, Scope};

/// The blob store: one backend, every scope (0039 §3).
///
/// The backend is deployment configuration. [`BlobStore::filesystem`] is the `fs` backend,
/// a directory; [`BlobStore::new`] takes any `object_store` backend, which is where `s3` will
/// plug in.
#[derive(Clone)]
pub struct BlobStore {
    inner: Arc<dyn ObjectStore>,
    temp: Arc<AtomicU64>,
}

impl std::fmt::Debug for BlobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BlobStore({})", self.inner)
    }
}

/// What a put returns: the bytes' digest and whether they were already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored {
    /// Both hashes and the size.
    pub digest: Digest,
    /// `true` when an object with these bytes was present and nothing was written: the
    /// deduplication the layout gives for free (0039 §3).
    pub existed: bool,
}

/// A staged write under `stash/` (0039 §3, §5): the bytes are stored and hashed, and not yet
/// at their content address. [`BlobStore::commit`] moves them there; [`BlobStore::delete`]
/// of `key` discards them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    /// The stash key the bytes are at.
    pub key: Key,
    /// Both hashes and the size, computed as the bytes streamed in.
    pub digest: Digest,
}

/// An object's metadata, from a `head` or a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// The size in bytes.
    pub size: u64,
    /// When the object was last written.
    pub last_modified: SystemTime,
}

/// One object a listing yields (0039 §14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The key, or `None` for a path under the prefix that the layout does not describe: a
    /// stray, which the orphan sweep and `verify` report.
    pub key: Option<Key>,
    /// The backend path, for a stray and for reporting.
    pub location: String,
    /// Size and age; the orphan sweep's grace period reads the age.
    pub meta: Meta,
}

/// An open read: the object's size, the range this read covers, and its bytes as a stream.
pub struct Reader {
    /// The whole object's size in bytes.
    pub size: u64,
    /// The byte range this read returns.
    pub range: Range<u64>,
    /// The bytes, in order.
    pub stream: BoxStream<'static, Result<Bytes, BlobError>>,
}

impl std::fmt::Debug for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reader")
            .field("size", &self.size)
            .field("range", &self.range)
            .finish_non_exhaustive()
    }
}

/// The outcome of the `full` check of one object (0039 §14): its bytes hash to its key, or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verified {
    /// The bytes hash to the key.
    Intact(Digest),
    /// The bytes do not: `actual` is what they hash to.
    Corrupt {
        /// The key's hash.
        expected: Sha256,
        /// What the bytes hash to.
        actual: Digest,
    },
    /// No object at the key.
    Missing,
}

impl BlobStore {
    /// The `fs` backend (0039 §3): a directory, created if it does not exist. Writes go to a
    /// temporary name and are renamed into place, so the directory's filesystem must make
    /// that rename atomic (local volumes, NFS and SMB do; a FUSE mount of an object store
    /// generally does not). Deleting the last object in a shard directory removes the
    /// directory.
    pub fn filesystem(root: impl AsRef<std::path::Path>) -> Result<Self, BlobError> {
        let root = root.as_ref();
        std::fs::create_dir_all(root).map_err(|source| {
            BlobError::Store(object_store::Error::Generic {
                store: "fs",
                source: Box::new(source),
            })
        })?;
        let fs = LocalFileSystem::new_with_prefix(root)?.with_automatic_cleanup(true);
        Ok(Self::new(Arc::new(fs)))
    }

    /// The store over any `object_store` backend.
    #[must_use]
    pub fn new(inner: Arc<dyn ObjectStore>) -> Self {
        Self {
            inner,
            temp: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The backend.
    #[must_use]
    pub fn backend(&self) -> &Arc<dyn ObjectStore> {
        &self.inner
    }

    /// Stores bytes already in memory at their content address, unless an object is already
    /// there (0039 §3: identical bytes have one key; a second write is harmless and skipped).
    pub async fn put(&self, scope: &Scope, bytes: Bytes) -> Result<Stored, BlobError> {
        let mut h = Hasher::new();
        h.update(&bytes);
        let digest = h.finish();
        let key = Key::sha256(scope.clone(), digest.sha256);
        let opts = PutOptions {
            mode: PutMode::Create,
            ..PutOptions::default()
        };
        let existed = match self.inner.put_opts(&key.path(), bytes.into(), opts).await {
            Ok(_) => false,
            Err(object_store::Error::AlreadyExists { .. }) => true,
            Err(e) => return Err(e.into()),
        };
        Ok(Stored { digest, existed })
    }

    /// Stores a stream of bytes at their content address: staged under `stash/` while both
    /// hashes are computed, then committed (0039 §3: bytes first, then the record). The stash
    /// key is store-generated; a crash leaves at worst that stash object for the orphan sweep.
    /// `max_size` is the size limit to enforce, if any.
    pub async fn put_stream<S, E>(
        &self,
        scope: &Scope,
        stream: S,
        max_size: Option<u64>,
    ) -> Result<Stored, BlobError>
    where
        S: Stream<Item = Result<Bytes, E>> + Send,
        E: std::error::Error + Send + Sync + 'static,
    {
        let n = self.temp.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let key = format!("tmp-{}-{nanos}-{n}", std::process::id());
        let staged = self.stash(scope, &key, stream, max_size).await?;
        self.commit(&staged).await
    }

    /// Streams bytes into `{scope}/stash/{key}`, computing SHA-256 and SHA-1 and enforcing
    /// `max_size` as they arrive (0039 §3, §5). Over the limit, or on a failed source, the
    /// write is aborted and nothing is left at the key. The bytes are written to a temporary
    /// name and renamed into place when the stream ends.
    pub async fn stash<S, E>(
        &self,
        scope: &Scope,
        key: &str,
        stream: S,
        max_size: Option<u64>,
    ) -> Result<Staged, BlobError>
    where
        S: Stream<Item = Result<Bytes, E>> + Send,
        E: std::error::Error + Send + Sync + 'static,
    {
        let key = Key::stash(scope.clone(), key)?;
        let path = key.path();
        let upload = self.inner.put_multipart(&path).await?;
        let mut writer = WriteMultipart::new(upload);
        let mut hasher = Hasher::new();
        let mut stream = std::pin::pin!(stream);

        let outcome: Result<(), BlobError> = async {
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| BlobError::Source(Box::new(e)))?;
                if let Some(limit) = max_size
                    && hasher.size() + chunk.len() as u64 > limit
                {
                    return Err(BlobError::TooLarge { limit });
                }
                hasher.update(&chunk);
                writer.wait_for_capacity(8).await?;
                writer.put(chunk);
            }
            Ok(())
        }
        .await;

        if let Err(e) = outcome {
            // Best effort: the abort may itself fail, and the original error is the one to
            // report; whatever an abort leaves, the orphan sweep removes.
            let _ = writer.abort().await;
            return Err(e);
        }
        writer.finish().await?;
        Ok(Staged {
            key,
            digest: hasher.finish(),
        })
    }

    /// Moves a staged write to its content address: copy-if-absent from the stash key to
    /// `sha256/…`, then the stash object is deleted (0039 §3). Idempotent: if the address is
    /// already taken, by this upload's twin or by an earlier one, nothing is copied and
    /// `existed` is `true`; a commit retried after the stash object is gone succeeds the same
    /// way as long as the address holds an object, and is [`BlobError::NotFound`] otherwise.
    pub async fn commit(&self, staged: &Staged) -> Result<Stored, BlobError> {
        let Key::Stash { scope, .. } = &staged.key else {
            return Err(BlobError::Key(crate::key::KeyError::Layout(
                staged.key.to_string(),
            )));
        };
        let from = staged.key.path();
        let address = Key::sha256(scope.clone(), staged.digest.sha256);
        let existed = match self.inner.copy_if_not_exists(&from, &address.path()).await {
            Ok(()) => false,
            Err(object_store::Error::AlreadyExists { .. }) => true,
            Err(object_store::Error::NotFound { .. }) if self.exists(&address).await? => true,
            Err(e) => return Err(e.into()),
        };
        self.delete(&staged.key).await?;
        Ok(Stored {
            digest: staged.digest,
            existed,
        })
    }

    /// Writes a derived or cached object at its key, replacing what is there (0039 §6, §11).
    /// Original bytes go through [`BlobStore::put`] or a staged write, never here: a key
    /// under `sha256/` is refused, since those objects are immutable.
    pub async fn put_at(&self, key: &Key, bytes: Bytes) -> Result<(), BlobError> {
        if key.kind() == Kind::Sha256 {
            return Err(BlobError::Key(crate::key::KeyError::Layout(
                key.to_string(),
            )));
        }
        self.inner.put(&key.path(), bytes.into()).await?;
        Ok(())
    }

    /// Reads a whole object into memory.
    pub async fn get(&self, key: &Key) -> Result<Bytes, BlobError> {
        Ok(self.inner.get(&key.path()).await?.bytes().await?)
    }

    /// Reads one byte range into memory. A range that starts past the end is an error; one
    /// that ends past it is clipped.
    pub async fn get_range(&self, key: &Key, range: Range<u64>) -> Result<Bytes, BlobError> {
        Ok(self.inner.get_range(&key.path(), range).await?)
    }

    /// Opens an object, or a byte range of it, as a stream (0039 §3: streamed reads).
    pub async fn read(&self, key: &Key, range: Option<Range<u64>>) -> Result<Reader, BlobError> {
        let opts = GetOptions {
            range: range.map(GetRange::Bounded),
            ..GetOptions::default()
        };
        let got = self.inner.get_opts(&key.path(), opts).await?;
        Ok(Reader {
            size: got.meta.size,
            range: got.range.clone(),
            stream: got.into_stream().map_err(BlobError::from).boxed(),
        })
    }

    /// An object's metadata, or `None` if there is no object at the key.
    pub async fn head(&self, key: &Key) -> Result<Option<Meta>, BlobError> {
        match self.inner.head(&key.path()).await {
            Ok(m) => Ok(Some(Meta {
                size: m.size,
                last_modified: m.last_modified.into(),
            })),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Whether an object is at the key: the `presence` check of one hash (0039 §14).
    pub async fn exists(&self, key: &Key) -> Result<bool, BlobError> {
        Ok(self.head(key).await?.is_some())
    }

    /// Deletes one object. Deleting what is not there is not an error, so a retried
    /// `ops.blob_delete` job (03 §4.9) converges.
    pub async fn delete(&self, key: &Key) -> Result<(), BlobError> {
        match self.inner.delete(&key.path()).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Deletes every object under a prefix and returns how many it deleted: a tenant's
    /// scope when it leaves (0039 §4, §14), a repository's cache, one hash's thumbnails.
    pub async fn delete_prefix(&self, prefix: &Prefix) -> Result<u64, BlobError> {
        let locations = self
            .inner
            .list(Some(prefix.path()))
            .map_ok(|m| m.location)
            .boxed();
        let mut deleted = 0;
        let mut results = self.inner.delete_stream(locations);
        while let Some(r) = results.next().await {
            match r {
                Ok(_) | Err(object_store::Error::NotFound { .. }) => deleted += 1,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(deleted)
    }

    /// Lists every object under a prefix: what the orphan sweep and `verify` walk (0039 §3,
    /// §14). Each entry carries its parsed key, or `None` for a stray the layout does not
    /// describe, with its size and age. Order is the backend's.
    pub fn list(&self, prefix: &Prefix) -> BoxStream<'static, Result<Entry, BlobError>> {
        self.inner
            .list(Some(prefix.path()))
            .map(|r| {
                let m = r?;
                Ok(Entry {
                    key: Key::parse(&m.location).ok(),
                    location: m.location.to_string(),
                    meta: Meta {
                        size: m.size,
                        last_modified: m.last_modified.into(),
                    },
                })
            })
            .boxed()
    }

    /// The `full` check of one object (0039 §14): reads every byte and compares the hash to
    /// the key.
    pub async fn verify(&self, scope: &Scope, hash: Sha256) -> Result<Verified, BlobError> {
        let key = Key::sha256(scope.clone(), hash);
        let reader = match self.read(&key, None).await {
            Ok(r) => r,
            Err(e) if e.is_not_found() => return Ok(Verified::Missing),
            Err(e) => return Err(e),
        };
        let mut hasher = Hasher::new();
        let mut stream = reader.stream;
        while let Some(chunk) = stream.next().await {
            hasher.update(&chunk?);
        }
        let actual = hasher.finish();
        Ok(if actual.sha256 == hash {
            Verified::Intact(actual)
        } else {
            Verified::Corrupt {
                expected: hash,
                actual,
            }
        })
    }
}
