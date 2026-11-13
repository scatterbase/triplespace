//! Storage scopes and the content-addressed key layout (0039 §3, §4).
//!
//! Within a scope, objects are keyed (03 §8):
//!
//! | Key | Holds |
//! |---|---|
//! | `{scope}/sha256/{h[0..2]}/{h[2..4]}/{h}` | Original bytes, `{h}` the lowercase hex SHA-256 |
//! | `{scope}/thumb/{h}/{transform}` | Thumbnails and other derived renderings (0039 §6) |
//! | `{scope}/stash/{key}` | Uploads in progress and stashed uploads (0039 §5) |
//! | `{scope}/cache/{repo}/{sha1}/…` | Bytes fetched from a proxied foreign repository (0039 §11) |
//! | `{scope}/job/{job_id}/{name}` | A job's output, `{job_id}` the job's ID in decimal (0104 A9) |
//!
//! A [`Scope`] is the part of the store a reference counts against (0039 §4): a tenant's
//! slug under `files.separation = tenant`, or `_instance` for the whole instance. Which one
//! applies is the caller's setting; the store only keeps them apart.
//!
//! [`Key`] builds and parses those paths; [`Prefix`] names the sets of objects a listing
//! walks or a deletion clears: a scope (what leaves with a tenant, 0039 §14), one kind within
//! it, one hash's thumbnails, one repository's cache, or one job's output.

use std::fmt;

use object_store::path::Path;

use crate::hash::{HashParseError, Sha1, Sha256};

/// A storage scope (0039 §4): a tenant's slug, or [`Scope::INSTANCE`].
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Scope(String);

/// A scope, key, path or prefix that does not follow the layout.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// A scope name that cannot be a path segment, or that takes a reserved name.
    #[error("not a storage scope: {0:?}")]
    Scope(String),
    /// A segment (a stash key, a transform, a repository name, a cache path) that is empty
    /// or contains `/`.
    #[error("not a key segment: {0:?}")]
    Segment(String),
    /// A hash that is not the canonical lowercase hex.
    #[error(transparent)]
    Hash(#[from] HashParseError),
    /// A path under the store that the layout does not describe.
    #[error("not a key of the layout: {0:?}")]
    Layout(String),
}

impl Scope {
    /// The one scope under `files.separation = instance` (0039 §4).
    pub const INSTANCE: &'static str = "_instance";

    /// The instance scope.
    #[must_use]
    pub fn instance() -> Self {
        Self(Self::INSTANCE.to_owned())
    }

    /// A tenant's scope, named by its slug. A slug is never empty (08 §1.2) and here cannot
    /// contain `/`, be `.` or `..`, or begin with `_`, which is reserved for the instance's
    /// own scopes.
    pub fn tenant(slug: &str) -> Result<Self, KeyError> {
        if slug.is_empty()
            || slug == "."
            || slug == ".."
            || slug.starts_with('_')
            || slug.contains('/')
            || slug.contains('\\')
            || slug.chars().any(char::is_control)
        {
            return Err(KeyError::Scope(slug.to_owned()));
        }
        Ok(Self(slug.to_owned()))
    }

    /// Either scope from its name, as a key's first segment carries it.
    pub fn parse(name: &str) -> Result<Self, KeyError> {
        if name == Self::INSTANCE {
            Ok(Self::instance())
        } else {
            Self::tenant(name)
        }
    }

    /// The name: the slug, or `_instance`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is the instance scope.
    #[must_use]
    pub fn is_instance(&self) -> bool {
        self.0 == Self::INSTANCE
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Scope({:?})", self.0)
    }
}

/// The five kinds of object a scope holds (0039 §3; 0104 A9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// Original bytes under `sha256/`: immutable, one key per distinct byte sequence.
    Sha256,
    /// Derived renderings under `thumb/` (0039 §6).
    Thumb,
    /// Uploads in progress and stashed uploads under `stash/` (0039 §5).
    Stash,
    /// Bytes fetched from a proxied foreign repository under `cache/` (0039 §11).
    Cache,
    /// A job's output under `job/` (0104 A9): written once by the job, removed by its
    /// retention.
    Job,
}

impl Kind {
    /// The segment that names the kind.
    #[must_use]
    pub fn segment(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Thumb => "thumb",
            Self::Stash => "stash",
            Self::Cache => "cache",
            Self::Job => "job",
        }
    }

    fn parse(segment: &str) -> Option<Self> {
        match segment {
            "sha256" => Some(Self::Sha256),
            "thumb" => Some(Self::Thumb),
            "stash" => Some(Self::Stash),
            "cache" => Some(Self::Cache),
            "job" => Some(Self::Job),
            _ => None,
        }
    }
}

/// One object's key (0039 §3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// `{scope}/sha256/{h[0..2]}/{h[2..4]}/{h}`: original bytes.
    Sha256 {
        /// The scope.
        scope: Scope,
        /// The SHA-256 of the bytes.
        hash: Sha256,
    },
    /// `{scope}/thumb/{h}/{transform}`: a derived rendering of the object at `h`.
    Thumb {
        /// The scope.
        scope: Scope,
        /// The original's SHA-256.
        hash: Sha256,
        /// The transform, as 0039 §6 names it.
        transform: String,
    },
    /// `{scope}/stash/{key}`: an upload in progress or stashed.
    Stash {
        /// The scope.
        scope: Scope,
        /// The stash key.
        key: String,
    },
    /// `{scope}/cache/{repo}/{sha1}/…`: bytes fetched from a proxied foreign repository.
    Cache {
        /// The scope.
        scope: Scope,
        /// The repository's name.
        repo: String,
        /// The foreign file's SHA-1, which is what a `mediawiki` repository reports.
        sha1: Sha1,
        /// The remainder of the key, one or more segments joined by `/`.
        rest: String,
    },
    /// `{scope}/job/{job_id}/{name}`: a file a job wrote as its output.
    Job {
        /// The scope.
        scope: Scope,
        /// The job's ID.
        job: u64,
        /// The file's name within the job.
        name: String,
    },
}

fn segment(s: &str) -> Result<&str, KeyError> {
    if s.is_empty() || s.contains('/') {
        Err(KeyError::Segment(s.to_owned()))
    } else {
        Ok(s)
    }
}

impl Key {
    /// The key of original bytes.
    #[must_use]
    pub fn sha256(scope: Scope, hash: Sha256) -> Self {
        Self::Sha256 { scope, hash }
    }

    /// The key of a derived rendering.
    pub fn thumb(scope: Scope, hash: Sha256, transform: &str) -> Result<Self, KeyError> {
        Ok(Self::Thumb {
            scope,
            hash,
            transform: segment(transform)?.to_owned(),
        })
    }

    /// The key of a stashed upload.
    pub fn stash(scope: Scope, key: &str) -> Result<Self, KeyError> {
        Ok(Self::Stash {
            scope,
            key: segment(key)?.to_owned(),
        })
    }

    /// The key of cached foreign bytes; `rest` is one or more segments joined by `/`.
    pub fn cache(scope: Scope, repo: &str, sha1: Sha1, rest: &str) -> Result<Self, KeyError> {
        let repo = segment(repo)?.to_owned();
        if rest.is_empty() || rest.split('/').any(str::is_empty) {
            return Err(KeyError::Segment(rest.to_owned()));
        }
        Ok(Self::Cache {
            scope,
            repo,
            sha1,
            rest: rest.to_owned(),
        })
    }

    /// The key of a job's output file.
    pub fn job(scope: Scope, job: u64, name: &str) -> Result<Self, KeyError> {
        Ok(Self::Job {
            scope,
            job,
            name: segment(name)?.to_owned(),
        })
    }

    /// The key's scope.
    #[must_use]
    pub fn scope(&self) -> &Scope {
        match self {
            Self::Sha256 { scope, .. }
            | Self::Thumb { scope, .. }
            | Self::Stash { scope, .. }
            | Self::Cache { scope, .. }
            | Self::Job { scope, .. } => scope,
        }
    }

    /// The key's kind.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::Sha256 { .. } => Kind::Sha256,
            Self::Thumb { .. } => Kind::Thumb,
            Self::Stash { .. } => Kind::Stash,
            Self::Cache { .. } => Kind::Cache,
            Self::Job { .. } => Kind::Job,
        }
    }

    /// The segments of the key, in order.
    fn segments(&self) -> Vec<String> {
        let mut v = vec![
            self.scope().as_str().to_owned(),
            self.kind().segment().to_owned(),
        ];
        match self {
            Self::Sha256 { hash, .. } => {
                let h = hash.hex();
                v.push(h[0..2].to_owned());
                v.push(h[2..4].to_owned());
                v.push(h);
            }
            Self::Thumb {
                hash, transform, ..
            } => {
                v.push(hash.hex());
                v.push(transform.clone());
            }
            Self::Stash { key, .. } => v.push(key.clone()),
            Self::Cache {
                repo, sha1, rest, ..
            } => {
                v.push(repo.clone());
                v.push(sha1.hex());
                v.extend(rest.split('/').map(str::to_owned));
            }
            Self::Job { job, name, .. } => {
                v.push(job.to_string());
                v.push(name.clone());
            }
        }
        v
    }

    /// The key as the backend's path.
    #[must_use]
    pub fn path(&self) -> Path {
        Path::from_iter(self.segments())
    }

    /// Reads a key back from a backend path, as a listing returns it. A path the layout
    /// does not describe is a [`KeyError::Layout`]: the orphan sweep and `verify` (0039 §14)
    /// report such an object as a stray rather than skipping it.
    pub fn parse(path: &Path) -> Result<Self, KeyError> {
        let layout = || KeyError::Layout(path.to_string());
        let parts: Vec<String> = path.parts().map(|p| p.as_ref().to_owned()).collect();
        let [scope, kind, rest @ ..] = parts.as_slice() else {
            return Err(layout());
        };
        let scope = Scope::parse(scope)?;
        let kind = Kind::parse(kind).ok_or_else(layout)?;
        match (kind, rest) {
            (Kind::Sha256, [a, b, h]) => {
                let hash = Sha256::parse_hex(h)?;
                if hash.hex() != *h || h[0..2] != *a || h[2..4] != *b {
                    return Err(layout());
                }
                Ok(Self::Sha256 { scope, hash })
            }
            (Kind::Thumb, [h, transform]) => {
                let hash = Sha256::parse_hex(h)?;
                if hash.hex() != *h {
                    return Err(layout());
                }
                Ok(Self::Thumb {
                    scope,
                    hash,
                    transform: transform.clone(),
                })
            }
            (Kind::Stash, [key]) => Ok(Self::Stash {
                scope,
                key: key.clone(),
            }),
            (Kind::Cache, [repo, h, rest @ ..]) if !rest.is_empty() => {
                let sha1 = Sha1::parse_hex(h)?;
                if sha1.hex() != *h {
                    return Err(layout());
                }
                Ok(Self::Cache {
                    scope,
                    repo: repo.clone(),
                    sha1,
                    rest: rest.join("/"),
                })
            }
            (Kind::Job, [id, name]) => {
                // The ID as the layout writes it, and no other spelling of the number.
                let job: u64 = id.parse().map_err(|_| layout())?;
                if job.to_string() != *id {
                    return Err(layout());
                }
                Ok(Self::Job {
                    scope,
                    job,
                    name: name.clone(),
                })
            }
            _ => Err(layout()),
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.path().as_ref())
    }
}

/// A set of objects named by a key prefix: what a listing walks and a prefix deletion
/// clears (0039 §3, §14).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Prefix(Path);

impl Prefix {
    /// Everything in a scope: a tenant's files as one set of objects (0039 §4, §14).
    #[must_use]
    pub fn scope(scope: &Scope) -> Self {
        Self(Path::from_iter([scope.as_str()]))
    }

    /// One kind of object within a scope.
    #[must_use]
    pub fn kind(scope: &Scope, kind: Kind) -> Self {
        Self(Path::from_iter([scope.as_str(), kind.segment()]))
    }

    /// Every derived rendering of one object (0039 §6).
    #[must_use]
    pub fn thumbs(scope: &Scope, hash: Sha256) -> Self {
        Self(Path::from_iter([
            scope.as_str(),
            Kind::Thumb.segment(),
            &hash.hex(),
        ]))
    }

    /// One foreign repository's cache (0039 §11).
    pub fn cache(scope: &Scope, repo: &str) -> Result<Self, KeyError> {
        Ok(Self(Path::from_iter([
            scope.as_str(),
            Kind::Cache.segment(),
            segment(repo)?,
        ])))
    }

    /// Every job's output in a scope (0104 A9): what a retention sweep walks.
    #[must_use]
    pub fn jobs(scope: &Scope) -> Self {
        Self::kind(scope, Kind::Job)
    }

    /// One job's output.
    #[must_use]
    pub fn job(scope: &Scope, job: u64) -> Self {
        Self(Path::from_iter([
            scope.as_str(),
            Kind::Job.segment(),
            &job.to_string(),
        ]))
    }

    /// The prefix as the backend's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl fmt::Display for Prefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const S: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

    fn lb() -> Scope {
        Scope::tenant("librarybase").unwrap()
    }

    #[test]
    fn scopes_are_slugs_or_the_instance() {
        // 0039 §4: one scope per tenant, its slug; `_instance` for the instance.
        assert_eq!(lb().as_str(), "librarybase");
        assert!(Scope::instance().is_instance());
        assert_eq!(Scope::parse("_instance").unwrap(), Scope::instance());
        assert_eq!(Scope::parse("librarybase").unwrap(), lb());
        for bad in ["", ".", "..", "a/b", "_instance", "_other", "a\nb"] {
            assert!(Scope::tenant(bad).is_err(), "{bad:?} is not a tenant scope");
        }
    }

    #[test]
    fn keys_follow_the_layout_and_parse_back() {
        // 0039 §3: the four kinds of key; 0104 A9: a job's output.
        let h: Sha256 = H.parse().unwrap();
        let s: Sha1 = S.parse().unwrap();
        let cases = [
            (
                Key::sha256(lb(), h),
                format!("librarybase/sha256/ba/78/{H}"),
            ),
            (
                Key::thumb(lb(), h, "320px").unwrap(),
                format!("librarybase/thumb/{H}/320px"),
            ),
            (
                Key::stash(Scope::instance(), "k1").unwrap(),
                "_instance/stash/k1".to_owned(),
            ),
            (
                Key::cache(lb(), "commons", s, "Example.jpg").unwrap(),
                format!("librarybase/cache/commons/{S}/Example.jpg"),
            ),
            (
                Key::cache(lb(), "commons", s, "thumb/120px-Example.jpg").unwrap(),
                format!("librarybase/cache/commons/{S}/thumb/120px-Example.jpg"),
            ),
            (
                Key::job(lb(), 42, "export.xml").unwrap(),
                "librarybase/job/42/export.xml".to_owned(),
            ),
        ];
        for (key, want) in cases {
            assert_eq!(key.to_string(), want);
            assert_eq!(Key::parse(&key.path()).unwrap(), key, "{want} parses back");
        }
    }

    #[test]
    fn segments_cannot_be_empty_or_nested() {
        let h: Sha256 = H.parse().unwrap();
        assert!(Key::thumb(lb(), h, "").is_err());
        assert!(Key::thumb(lb(), h, "a/b").is_err());
        assert!(Key::stash(lb(), "a/b").is_err());
        assert!(Key::cache(lb(), "r", S.parse().unwrap(), "a//b").is_err());
        assert!(Prefix::cache(&lb(), "a/b").is_err());
        assert!(Key::job(lb(), 1, "").is_err());
        assert!(Key::job(lb(), 1, "a/b").is_err());
    }

    #[test]
    fn paths_outside_the_layout_are_strays() {
        // 0039 §14: an orphan sweep must see what it cannot name.
        let bad = [
            "librarybase".to_owned(),
            "librarybase/other/x".to_owned(),
            format!("librarybase/sha256/{H}"),
            format!("librarybase/sha256/ba/79/{H}"),
            format!("librarybase/sha256/ba/78/{}", H.to_uppercase()),
            format!("librarybase/thumb/{H}"),
            "librarybase/stash/a/b".to_owned(),
            format!("librarybase/cache/commons/{S}"),
            "_x/stash/k".to_owned(),
            "librarybase/job/42".to_owned(),
            "librarybase/job/042/export.xml".to_owned(),
            "librarybase/job/x/export.xml".to_owned(),
            "librarybase/job/42/a/b".to_owned(),
        ];
        for p in bad {
            assert!(
                Key::parse(&Path::from(p.as_str())).is_err(),
                "{p} is a stray"
            );
        }
    }

    #[test]
    fn prefixes_name_the_sets_a_sweep_walks() {
        let h: Sha256 = H.parse().unwrap();
        assert_eq!(Prefix::scope(&lb()).to_string(), "librarybase");
        assert_eq!(
            Prefix::kind(&lb(), Kind::Stash).to_string(),
            "librarybase/stash"
        );
        assert_eq!(
            Prefix::thumbs(&lb(), h).to_string(),
            format!("librarybase/thumb/{H}")
        );
        assert_eq!(
            Prefix::cache(&lb(), "commons").unwrap().to_string(),
            "librarybase/cache/commons"
        );
        assert_eq!(Prefix::jobs(&lb()).to_string(), "librarybase/job");
        assert_eq!(Prefix::job(&lb(), 42).to_string(), "librarybase/job/42");
        assert_eq!(Key::job(lb(), 42, "f").unwrap().kind(), Kind::Job);
    }
}
