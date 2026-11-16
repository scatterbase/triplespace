//! Thread statuses: the `thread-status` registry (14 §2.1).

use serde::Deserialize;

/// The default statuses, `docs/registry/thread-statuses.toml`.
pub const THREAD_STATUSES_TOML: &str = include_str!("../../../docs/registry/thread-statuses.toml");

/// The status of a thread no post has set one on.
pub const OPEN: &str = "open";

/// Whether a status leaves a thread open or closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Still under discussion.
    Open,
    /// Over: collapsed by default (14 §2.2), and notified on (0021 §2).
    Closed,
}

/// One status.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    /// Its name, as posts carry it.
    pub name: String,
    /// Its default English label.
    pub label: String,
    /// Open or closed.
    pub category: Category,
    /// Its place in the status menu.
    pub order: i64,
    /// `open` is required and cannot be retired.
    #[serde(default)]
    pub required: bool,
    /// A retired status is no longer set, but threads that carry it keep it.
    #[serde(default)]
    pub retired: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u32,
    #[serde(default)]
    status: Vec<Status>,
}

/// A malformed status registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusesError(pub String);

impl std::fmt::Display for StatusesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StatusesError {}

/// The statuses a tenant may set: the registry's defaults, and any a tenant adds as `config`
/// records of kind `thread-status`.
#[derive(Debug, Clone)]
pub struct Statuses {
    statuses: Vec<Status>,
}

impl Statuses {
    /// Reads a registry file.
    pub fn from_toml(text: &str) -> Result<Self, StatusesError> {
        let file: File = toml::from_str(text).map_err(|e| StatusesError(e.to_string()))?;
        if file.version != 1 {
            return Err(StatusesError(format!("unknown version {}", file.version)));
        }
        Statuses::new(file.status)
    }

    /// Builds the set from a list, as a tenant's config records give it.
    pub fn new(mut statuses: Vec<Status>) -> Result<Self, StatusesError> {
        statuses.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
        for (k, s) in statuses.iter().enumerate() {
            if statuses[..k].iter().any(|t| t.name == s.name) {
                return Err(StatusesError(format!("`{}` is listed twice", s.name)));
            }
        }
        match statuses.iter().find(|s| s.name == OPEN) {
            Some(s) if s.category == Category::Open && !s.retired => Ok(Statuses { statuses }),
            _ => Err(StatusesError(
                "`open` must exist, in the open category, and not be retired".into(),
            )),
        }
    }

    /// The defaults, embedded at build time.
    ///
    /// # Panics
    ///
    /// Never: the embedded file is checked by this crate's tests.
    #[must_use]
    pub fn embedded() -> &'static Statuses {
        static CELL: std::sync::OnceLock<Statuses> = std::sync::OnceLock::new();
        CELL.get_or_init(|| {
            Statuses::from_toml(THREAD_STATUSES_TOML).expect("thread-statuses.toml is valid")
        })
    }

    /// A status by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Status> {
        self.statuses.iter().find(|s| s.name == name)
    }

    /// Every status, in display order.
    pub fn iter(&self) -> impl Iterator<Item = &Status> {
        self.statuses.iter()
    }
}
