//! The portability class of every `private` table (0027 §2). The classification is
//! data: a table that is not listed here, or whose comment in the database disagrees,
//! fails [`check`], so a private table cannot exist without saying what it is.

use tokio_postgres::GenericClient;

/// Whether state may leave the instance with its account (0027 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Portability {
    /// Chosen by the person, about the person: in the user data bundle.
    Portable,
    /// Secrets and proofs an instance mints for itself: reissued elsewhere.
    ReEstablished,
    /// Not the person's, or bound to this instance.
    NeverLeaves,
}

impl Portability {
    /// The word in the table comment, `portability: {word}`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portable => "portable",
            Self::ReEstablished => "re-established",
            Self::NeverLeaves => "never-leaves",
        }
    }

    /// From the word.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "portable" => Some(Self::Portable),
            "re-established" => Some(Self::ReEstablished),
            "never-leaves" => Some(Self::NeverLeaves),
            _ => None,
        }
    }
}

/// Every `private` table and its class.
pub const PRIVATE_TABLES: &[(&str, Portability)] = &[
    ("password", Portability::ReEstablished),
    ("api_key", Portability::ReEstablished),
    ("session", Portability::NeverLeaves),
];

/// Why the live schema disagrees with [`PRIVATE_TABLES`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PrivateError {
    /// A table in the database that this crate does not classify.
    #[error("private.{0} has no portability class")]
    Unclassified(String),
    /// A table this crate lists that the database lacks.
    #[error("private.{0} is classified but does not exist")]
    Missing(String),
    /// The comment says something else.
    #[error("private.{table} is `{listed}` here but `{comment:?}` in the database")]
    Disagrees {
        /// The table.
        table: String,
        /// This crate's class.
        listed: &'static str,
        /// The database's comment.
        comment: Option<String>,
    },
    /// The query failed.
    #[error("private: {0}")]
    Sql(String),
}

/// Checks that every table in `private` is classified, exists, and carries its class as
/// its comment.
pub async fn check<C: GenericClient>(client: &C) -> Result<(), PrivateError> {
    let rows = client
        .query(
            "SELECT c.relname, obj_description(c.oid, 'pg_class')
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = 'private' AND c.relkind IN ('r', 'p')",
            &[],
        )
        .await
        .map_err(|e| PrivateError::Sql(e.to_string()))?;
    let live: Vec<(String, Option<String>)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
    for (table, comment) in &live {
        let Some((_, class)) = PRIVATE_TABLES.iter().find(|(t, _)| t == table) else {
            return Err(PrivateError::Unclassified(table.clone()));
        };
        let want = format!("portability: {}", class.as_str());
        if comment.as_deref() != Some(want.as_str()) {
            return Err(PrivateError::Disagrees {
                table: table.clone(),
                listed: class.as_str(),
                comment: comment.clone(),
            });
        }
    }
    for (table, _) in PRIVATE_TABLES {
        if !live.iter().any(|(t, _)| t == table) {
            return Err(PrivateError::Missing((*table).to_string()));
        }
    }
    Ok(())
}

/// The portable tables: what a user data bundle may read (0027 §3).
#[must_use]
pub fn portable_tables() -> Vec<&'static str> {
    PRIVATE_TABLES
        .iter()
        .filter(|(_, c)| *c == Portability::Portable)
        .map(|(t, _)| *t)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_round_trip() {
        for c in [
            Portability::Portable,
            Portability::ReEstablished,
            Portability::NeverLeaves,
        ] {
            assert_eq!(Portability::parse(c.as_str()), Some(c));
        }
        assert_eq!(Portability::parse("secret"), None);
        assert!(
            portable_tables().is_empty(),
            "nothing portable is in the schema yet"
        );
    }
}
