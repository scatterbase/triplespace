//! The backend a blob store runs over (03 §8; 0039 A17): the handful of object operations
//! [`crate::BlobStore`] needs, as a trait, and the `fs` backend that implements it over a
//! directory.
//!
//! The trait is small on purpose: put (replacing, or only if absent), a streamed upload that
//! takes its key only when it completes, copy-if-absent, whole, ranged and streamed reads,
//! metadata, delete, and a listing under a prefix. The `s3` backend implements the same
//! operations when it is built.
//!
//! [`Filesystem`] keeps the on-disk form the store had over `object_store`'s
//! `LocalFileSystem`, so a directory written by either is read by the other: each key
//! segment is percent-encoded with the same character set (and `.` and `..` encoded whole),
//! a write goes to `{path}#{n}` and is renamed or hard-linked into place, a temporary
//! `#{digits}` name is never listed, and deleting an object removes the directories it
//! leaves empty.

use std::fmt;
use std::io::{ErrorKind, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bytes::Bytes;
use futures::future::BoxFuture;
use futures::stream::{self, BoxStream, StreamExt};
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, percent_encode};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

/// The characters a key segment is percent-encoded for: `/`, which separates segments, and
/// what S3 and Google Cloud Storage advise against in object names. The same set as
/// `object_store`'s, so the paths on disk are unchanged.
const ENCODED: &AsciiSet = &CONTROLS
    .add(b'/')
    .add(b'\\')
    .add(b'{')
    .add(b'^')
    .add(b'}')
    .add(b'%')
    .add(b'`')
    .add(b']')
    .add(b'"')
    .add(b'>')
    .add(b'[')
    .add(b'~')
    .add(b'<')
    .add(b'#')
    .add(b'|')
    .add(b'\r')
    .add(b'\n')
    .add(b'*')
    .add(b'?');

/// How many bytes a streamed read returns at a time.
const CHUNK: usize = 64 * 1024;

/// How many directory entries a listing reads per blocking step.
const LIST_BATCH: usize = 1024;

/// Where an object is in a backend: its segments, each percent-encoded, joined with `/`.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location(String);

impl Location {
    /// The location of these segments, each encoded. Empty segments are skipped.
    pub fn from_segments<I, S>(segments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut raw = String::new();
        for s in segments {
            let s = s.as_ref();
            if s.is_empty() {
                continue;
            }
            if !raw.is_empty() {
                raw.push('/');
            }
            match s {
                "." => raw.push_str("%2E"),
                ".." => raw.push_str("%2E%2E"),
                s => raw.extend(percent_encode(s.as_bytes(), ENCODED)),
            }
        }
        Self(raw)
    }

    /// A location written as `a/b/c`, each segment encoded as [`Location::from_segments`]
    /// encodes it.
    #[must_use]
    pub fn parse(path: &str) -> Self {
        Self::from_segments(path.split('/'))
    }

    /// The segments, decoded: what [`Location::from_segments`] was given.
    pub fn segments(&self) -> impl Iterator<Item = String> + '_ {
        self.raw_segments()
            .map(|p| percent_decode_str(p).decode_utf8_lossy().into_owned())
    }

    /// The segments as they are stored, still encoded.
    fn raw_segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|p| !p.is_empty())
    }

    /// The encoded form, `/`-joined.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Location {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// What a backend operation can fail with.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    /// No object at the location.
    #[error("no object at {location}")]
    NotFound {
        /// The location.
        location: String,
    },
    /// A write that may not replace found an object already there.
    #[error("an object is already at {location}")]
    AlreadyExists {
        /// The location.
        location: String,
    },
    /// A read's range is empty or starts at or past the end of the object.
    #[error("the range {start}..{end} is not within the {size} bytes at {location}")]
    Range {
        /// The location.
        location: String,
        /// The range's start.
        start: u64,
        /// The range's end.
        end: u64,
        /// The object's size.
        size: u64,
    },
    /// A location the backend cannot hold an object at: empty, or a temporary name.
    #[error("not an object location: {0:?}")]
    Location(String),
    /// The filesystem failed.
    #[error("{op} {}: {source}", path.display())]
    Io {
        /// What was being done.
        op: &'static str,
        /// The file or directory.
        path: PathBuf,
        /// The error.
        #[source]
        source: std::io::Error,
    },
    /// A blocking task the operation ran on did not finish.
    #[error("the blocking task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

/// Whether a put may replace an object already at its location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutMode {
    /// Replace what is there.
    Overwrite,
    /// Write only if nothing is there; otherwise [`BackendError::AlreadyExists`].
    Create,
}

/// An object's metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMeta {
    /// Where it is.
    pub location: Location,
    /// Its size in bytes.
    pub size: u64,
    /// When it was last written.
    pub last_modified: SystemTime,
}

/// An open read.
pub struct GetResult {
    /// The whole object's metadata.
    pub meta: ObjectMeta,
    /// The byte range the stream covers.
    pub range: Range<u64>,
    /// The bytes of that range, in order.
    pub stream: BoxStream<'static, Result<Bytes, BackendError>>,
}

impl fmt::Debug for GetResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GetResult")
            .field("meta", &self.meta)
            .field("range", &self.range)
            .finish_non_exhaustive()
    }
}

/// A streamed write in progress. Nothing is at its location until [`Upload::finish`]; then
/// the bytes replace whatever was. [`Upload::abort`] discards them.
pub trait Upload: Send {
    /// Appends bytes.
    fn write(&mut self, chunk: Bytes) -> BoxFuture<'_, Result<(), BackendError>>;
    /// Puts the bytes written at the location.
    fn finish(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>>;
    /// Discards the bytes written.
    fn abort(self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>>;
}

/// The operations a blob store needs from where its objects live (03 §8).
pub trait Backend: fmt::Debug + Send + Sync + 'static {
    /// Writes bytes at a location, replacing what is there or only if nothing is.
    fn put<'a>(
        &'a self,
        location: &'a Location,
        bytes: Bytes,
        mode: PutMode,
    ) -> BoxFuture<'a, Result<(), BackendError>>;

    /// Opens a streamed write to a location.
    fn upload<'a>(
        &'a self,
        location: &'a Location,
    ) -> BoxFuture<'a, Result<Box<dyn Upload>, BackendError>>;

    /// Copies an object unless one is already at the destination
    /// ([`BackendError::AlreadyExists`]). A missing source is [`BackendError::NotFound`].
    fn copy_if_absent<'a>(
        &'a self,
        from: &'a Location,
        to: &'a Location,
    ) -> BoxFuture<'a, Result<(), BackendError>>;

    /// Opens an object, or a range of it, as a stream. A range that ends past the object
    /// is clipped; one that is empty or starts at or past its end is an error.
    fn get<'a>(
        &'a self,
        location: &'a Location,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, Result<GetResult, BackendError>>;

    /// An object's metadata.
    fn head<'a>(
        &'a self,
        location: &'a Location,
    ) -> BoxFuture<'a, Result<ObjectMeta, BackendError>>;

    /// Deletes an object. A missing one is [`BackendError::NotFound`].
    fn delete<'a>(&'a self, location: &'a Location) -> BoxFuture<'a, Result<(), BackendError>>;

    /// Every object under a prefix, matched by whole segments, in the backend's order. A
    /// prefix with nothing under it lists nothing.
    fn list(&self, prefix: &Location) -> BoxStream<'static, Result<ObjectMeta, BackendError>>;
}

/// The `fs` backend (03 §8): a directory. Writes go to a temporary name and are renamed or
/// hard-linked into place, so the directory's filesystem must make both atomic (local
/// volumes, NFS and SMB do; a FUSE mount of an object store generally does not). Nothing is
/// fsynced, as `object_store`'s backend did not by default.
#[derive(Debug, Clone)]
pub struct Filesystem {
    root: PathBuf,
}

fn io<'a>(op: &'static str, path: &'a Path) -> impl FnOnce(std::io::Error) -> BackendError + 'a {
    move |source| BackendError::Io {
        op,
        path: path.to_owned(),
        source,
    }
}

/// Whether a file name is one of the temporary names writes use, `{name}#{digits}`.
fn is_temporary(name: &str) -> bool {
    match name.rsplit_once('#') {
        Some((_, suffix)) => !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// The temporary name `{dest}#{n}`.
fn temporary(dest: &Path, n: u64) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(format!("#{n}"));
    s.into()
}

fn create_parents(path: &Path) -> Result<(), BackendError> {
    match path.parent() {
        Some(parent) => std::fs::create_dir_all(parent).map_err(io("create directory", parent)),
        None => Ok(()),
    }
}

/// Opens a new temporary file beside `dest`, creating its directories if needed.
fn new_temporary(dest: &Path) -> Result<(std::fs::File, PathBuf), BackendError> {
    let mut n = 1;
    loop {
        let path = temporary(dest, n);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(f) => return Ok((f, path)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => n += 1,
            Err(e) if e.kind() == ErrorKind::NotFound => create_parents(&path)?,
            Err(e) => return Err(io("create", &path)(e)),
        }
    }
}

fn not_found(location: &Location) -> BackendError {
    BackendError::NotFound {
        location: location.to_string(),
    }
}

fn meta(location: Location, m: &std::fs::Metadata) -> ObjectMeta {
    ObjectMeta {
        location,
        size: m.len(),
        last_modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    }
}

/// A file's metadata; a missing file, or a directory, is not an object.
fn file_meta(path: &Path, location: &Location) -> Result<std::fs::Metadata, BackendError> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => Err(not_found(location)),
        Ok(m) => Ok(m),
        Err(e) if e.kind() == ErrorKind::NotFound => Err(not_found(location)),
        Err(e) => Err(io("read metadata of", path)(e)),
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, BackendError> + Send + 'static,
) -> Result<T, BackendError> {
    tokio::task::spawn_blocking(f).await?
}

impl Filesystem {
    /// The backend over a directory, which is created if it does not exist.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, BackendError> {
        let root = root.as_ref();
        std::fs::create_dir_all(root).map_err(io("create directory", root))?;
        let root = std::fs::canonicalize(root).map_err(io("resolve", root))?;
        Ok(Self { root })
    }

    /// The directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file an object at `location` is.
    fn path(&self, location: &Location) -> Result<PathBuf, BackendError> {
        let refuse = || BackendError::Location(location.to_string());
        let last = location.raw_segments().last().ok_or_else(refuse)?;
        if is_temporary(last) {
            return Err(refuse());
        }
        let mut path = self.root.clone();
        path.extend(location.raw_segments());
        Ok(path)
    }

    /// The directory a prefix names.
    fn dir(&self, prefix: &Location) -> PathBuf {
        let mut path = self.root.clone();
        path.extend(prefix.raw_segments());
        path
    }
}

impl Backend for Filesystem {
    fn put<'a>(
        &'a self,
        location: &'a Location,
        bytes: Bytes,
        mode: PutMode,
    ) -> BoxFuture<'a, Result<(), BackendError>> {
        Box::pin(async move {
            let dest = self.path(location)?;
            let location = location.clone();
            blocking(move || {
                let (mut file, temp) = new_temporary(&dest)?;
                let written =
                    std::io::Write::write_all(&mut file, &bytes).map_err(io("write", &temp));
                drop(file);
                let placed = written.and_then(|()| match mode {
                    PutMode::Overwrite => {
                        std::fs::rename(&temp, &dest).map_err(io("rename", &temp))
                    }
                    PutMode::Create => match std::fs::hard_link(&temp, &dest) {
                        Ok(()) => Ok(()),
                        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                            Err(BackendError::AlreadyExists {
                                location: location.to_string(),
                            })
                        }
                        Err(e) => Err(io("link", &dest)(e)),
                    },
                });
                // After a rename the temporary name is gone; otherwise it is removed here.
                if mode == PutMode::Create || placed.is_err() {
                    let _ = std::fs::remove_file(&temp);
                }
                placed
            })
            .await
        })
    }

    fn upload<'a>(
        &'a self,
        location: &'a Location,
    ) -> BoxFuture<'a, Result<Box<dyn Upload>, BackendError>> {
        Box::pin(async move {
            let dest = self.path(location)?;
            let (file, temp, dest) = blocking(move || {
                let (file, temp) = new_temporary(&dest)?;
                Ok((file, temp, dest))
            })
            .await?;
            Ok(Box::new(FsUpload {
                file: Some(tokio::fs::File::from_std(file)),
                temp: Some(temp),
                dest,
            }) as Box<dyn Upload>)
        })
    }

    fn copy_if_absent<'a>(
        &'a self,
        from: &'a Location,
        to: &'a Location,
    ) -> BoxFuture<'a, Result<(), BackendError>> {
        Box::pin(async move {
            let src = self.path(from)?;
            let dest = self.path(to)?;
            let (from, to) = (from.clone(), to.clone());
            blocking(move || {
                loop {
                    match std::fs::hard_link(&src, &dest) {
                        Ok(()) => return Ok(()),
                        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                            return Err(BackendError::AlreadyExists {
                                location: to.to_string(),
                            });
                        }
                        Err(e) if e.kind() == ErrorKind::NotFound => {
                            if src.is_file() {
                                create_parents(&dest)?;
                            } else {
                                return Err(not_found(&from));
                            }
                        }
                        Err(e) => return Err(io("link", &dest)(e)),
                    }
                }
            })
            .await
        })
    }

    fn get<'a>(
        &'a self,
        location: &'a Location,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, Result<GetResult, BackendError>> {
        Box::pin(async move {
            let path = self.path(location)?;
            let loc = location.clone();
            let (file, m) = blocking(move || {
                let file = match std::fs::File::open(&path) {
                    Ok(f) => f,
                    Err(e) if e.kind() == ErrorKind::NotFound => return Err(not_found(&loc)),
                    Err(e) => return Err(io("open", &path)(e)),
                };
                let m = file.metadata().map_err(io("read metadata of", &path))?;
                if m.is_dir() {
                    return Err(not_found(&loc));
                }
                Ok((file, meta(loc, &m)))
            })
            .await?;
            let range = match range {
                None => 0..m.size,
                Some(r) if r.start < r.end && r.start < m.size => r.start..r.end.min(m.size),
                Some(r) => {
                    return Err(BackendError::Range {
                        location: location.to_string(),
                        start: r.start,
                        end: r.end,
                        size: m.size,
                    });
                }
            };
            let path = self.path(location)?;
            let mut file = tokio::fs::File::from_std(file);
            if range.start > 0 {
                file.seek(SeekFrom::Start(range.start))
                    .await
                    .map_err(io("seek", &path))?;
            }
            let remaining = range.end - range.start;
            let stream = stream::try_unfold(
                (file, remaining, path),
                |(mut file, remaining, path)| async move {
                    if remaining == 0 {
                        return Ok(None);
                    }
                    let want = usize::try_from(remaining).map_or(CHUNK, |r| r.min(CHUNK));
                    let mut buf = vec![0; want];
                    let n = file.read(&mut buf).await.map_err(io("read", &path))?;
                    if n == 0 {
                        return Err(io("read", &path)(ErrorKind::UnexpectedEof.into()));
                    }
                    buf.truncate(n);
                    Ok(Some((Bytes::from(buf), (file, remaining - n as u64, path))))
                },
            )
            .boxed();
            Ok(GetResult {
                meta: m,
                range,
                stream,
            })
        })
    }

    fn head<'a>(
        &'a self,
        location: &'a Location,
    ) -> BoxFuture<'a, Result<ObjectMeta, BackendError>> {
        Box::pin(async move {
            let path = self.path(location)?;
            let location = location.clone();
            blocking(move || {
                let m = file_meta(&path, &location)?;
                Ok(meta(location, &m))
            })
            .await
        })
    }

    fn delete<'a>(&'a self, location: &'a Location) -> BoxFuture<'a, Result<(), BackendError>> {
        Box::pin(async move {
            let path = self.path(location)?;
            let root = self.root.clone();
            let location = location.clone();
            blocking(move || {
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == ErrorKind::NotFound => return Err(not_found(&location)),
                    Err(e) => return Err(io("delete", &path)(e)),
                }
                // Remove the directories the deletion left empty, up to the root.
                let mut parent = path.parent();
                while let Some(dir) = parent {
                    if dir == root || std::fs::remove_dir(dir).is_err() {
                        break;
                    }
                    parent = dir.parent();
                }
                Ok(())
            })
            .await
        })
    }

    fn list(&self, prefix: &Location) -> BoxStream<'static, Result<ObjectMeta, BackendError>> {
        let walk = Walk {
            root: self.root.clone(),
            pending: vec![self.dir(prefix)],
            open: None,
        };
        stream::try_unfold(Some(walk), |walk| async move {
            let Some(mut walk) = walk else {
                return Ok(None);
            };
            let (walk, batch) = blocking(move || {
                let batch = walk.next_batch()?;
                Ok((walk, batch))
            })
            .await?;
            // An empty batch means the walk is over.
            Ok(if batch.is_empty() {
                None
            } else {
                Some((batch, Some(walk)))
            })
        })
        .map(|batch| match batch {
            Ok(batch) => stream::iter(batch.into_iter().map(Ok)).left_stream(),
            Err(e) => stream::once(async { Err(e) }).right_stream(),
        })
        .flatten()
        .boxed()
    }
}

/// A depth-first walk of a directory tree, read in batches on a blocking thread.
struct Walk {
    root: PathBuf,
    pending: Vec<PathBuf>,
    open: Option<(PathBuf, std::fs::ReadDir)>,
}

impl Walk {
    /// Up to [`LIST_BATCH`] objects; none when the walk is over.
    fn next_batch(&mut self) -> Result<Vec<ObjectMeta>, BackendError> {
        let mut batch = Vec::new();
        while batch.len() < LIST_BATCH {
            let Some((dir, entries)) = &mut self.open else {
                let Some(dir) = self.pending.pop() else {
                    break;
                };
                match std::fs::read_dir(&dir) {
                    Ok(entries) => self.open = Some((dir, entries)),
                    // A prefix with nothing under it, or a directory deleted mid-walk.
                    Err(e) if e.kind() == ErrorKind::NotFound => {}
                    Err(e) if e.kind() == ErrorKind::NotADirectory => {}
                    Err(e) => return Err(io("list", &dir)(e)),
                }
                continue;
            };
            let Some(entry) = entries.next() else {
                self.open = None;
                continue;
            };
            let entry = entry.map_err(io("list", dir))?;
            let path = entry.path();
            // Follows symbolic links, as a read does.
            let m = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => return Err(io("read metadata of", &path)(e)),
            };
            if m.is_dir() {
                self.pending.push(path);
                continue;
            }
            let Some(location) = self.location(&path) else {
                continue;
            };
            batch.push(meta(location, &m));
        }
        Ok(batch)
    }

    /// The location of a file under the root, or `None` for a temporary name or a name
    /// that is not UTF-8, neither of which any write of the store's made.
    fn location(&self, path: &Path) -> Option<Location> {
        let rel = path.strip_prefix(&self.root).ok()?;
        let mut raw = String::new();
        for part in rel.components() {
            let part = part.as_os_str().to_str()?;
            if !raw.is_empty() {
                raw.push('/');
            }
            raw.push_str(part);
        }
        let name = raw.rsplit('/').next()?;
        (!is_temporary(name)).then_some(Location(raw))
    }
}

/// A streamed write to a temporary file, renamed into place when it finishes. Dropped
/// unfinished, as when the task writing it is cancelled, it removes the temporary file.
struct FsUpload {
    /// `None` once finished or aborted.
    file: Option<tokio::fs::File>,
    /// `None` once the temporary file is renamed into place or removed.
    temp: Option<PathBuf>,
    dest: PathBuf,
}

impl FsUpload {
    fn parts(&mut self) -> (tokio::fs::File, PathBuf) {
        let file = self
            .file
            .take()
            .expect("an upload is finished or aborted once");
        let temp = self
            .temp
            .take()
            .expect("an upload is finished or aborted once");
        (file, temp)
    }
}

impl Upload for FsUpload {
    fn write(&mut self, chunk: Bytes) -> BoxFuture<'_, Result<(), BackendError>> {
        Box::pin(async move {
            let (Some(file), Some(temp)) = (&mut self.file, &self.temp) else {
                unreachable!("an upload is not written after it is finished or aborted")
            };
            file.write_all(&chunk).await.map_err(io("write", temp))
        })
    }

    fn finish(mut self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async move {
            let (mut file, temp) = self.parts();
            let dest = std::mem::take(&mut self.dest);
            // tokio writes in the background: the last write's error surfaces here.
            if let Err(e) = file.flush().await {
                drop(file);
                let _ = tokio::fs::remove_file(&temp).await;
                return Err(io("write", &temp)(e));
            }
            drop(file);
            blocking(move || {
                let done = std::fs::rename(&temp, &dest).map_err(io("rename", &temp));
                if done.is_err() {
                    let _ = std::fs::remove_file(&temp);
                }
                done
            })
            .await
        })
    }

    fn abort(mut self: Box<Self>) -> BoxFuture<'static, Result<(), BackendError>> {
        Box::pin(async move {
            let (file, temp) = self.parts();
            drop(file);
            match tokio::fs::remove_file(&temp).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(io("delete", &temp)(e)),
            }
        })
    }
}

impl Drop for FsUpload {
    fn drop(&mut self) {
        drop(self.file.take());
        if let Some(temp) = self.temp.take() {
            // Best effort, as `object_store`'s upload did: what is left, the orphan sweep
            // does not see (a temporary name is never listed), so remove it now.
            match tokio::runtime::Handle::try_current() {
                Ok(rt) => drop(rt.spawn_blocking(move || std::fs::remove_file(temp))),
                Err(_) => drop(std::fs::remove_file(temp)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_encoded_as_object_store_encoded_them() {
        // 03 §8: the paths on disk do not change with the backend's implementation.
        let l = Location::from_segments(["a b", "x#1", "50%", ".", "..", "", "ok"]);
        assert_eq!(l.as_str(), "a b/x%231/50%25/%2E/%2E%2E/ok");
        let back: Vec<String> = l.segments().collect();
        assert_eq!(back, ["a b", "x#1", "50%", ".", "..", "ok"]);
        assert_eq!(Location::parse("a//b/").as_str(), "a/b");
    }

    #[test]
    fn temporary_names_are_recognised() {
        assert!(is_temporary("abc#1"));
        assert!(is_temporary("abc#123"));
        assert!(!is_temporary("abc#"));
        assert!(!is_temporary("abc#x1"));
        assert!(!is_temporary("abc"));
    }
}
