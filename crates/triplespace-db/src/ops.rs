//! What a backup does with every `ops` table (0103 A2, A4). The classification is data: a
//! table that is not listed here, or whose comment in the database disagrees, fails
//! [`check`], so an `ops` table cannot exist without saying whether a backup carries it.
//! The `private` tables' portability classes ([`crate::private`]) are the same idea.

use tokio_postgres::GenericClient;

/// What a backup does with an `ops` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backup {
    /// Carried in the bundle as it is: state no rebuild recovers.
    Carried,
    /// Carried, but encrypted with `private` (0103 A3): operational state that holds
    /// personal data.
    Sealed,
    /// Left out: a restore's migrations or rebuild fill it, or a restored instance starts
    /// it empty.
    Left,
}

impl Backup {
    /// The word in the table comment, `backup: {word}`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Carried => "carried",
            Self::Sealed => "sealed",
            Self::Left => "left",
        }
    }

    /// From the word.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "carried" => Some(Self::Carried),
            "sealed" => Some(Self::Sealed),
            "left" => Some(Self::Left),
            _ => None,
        }
    }
}

/// Every `ops` table and what a backup does with it.
pub const OPS_TABLES: &[(&str, Backup)] = &[
    // Outgoing messages, staged complete with their addresses and bodies (0100): carried,
    // so that a restore sends what was waiting, and sealed, since they are personal data.
    ("delivery", Backup::Sealed),
    // Queued jobs, their uploads with them; a job running at the backup's instant has a
    // stale claim after a restore and runs again, as after a runner's crash (0104 §4).
    ("job_queue", Backup::Carried),
    // Hashes of the keys proxies present (0057 §10): no account's.
    ("forwarder_key", Backup::Carried),
    // Followers' positions (0092 §3): a hint the version cursor could rebuild, carried
    // because it is small and saves polling from the start.
    ("repo_cursor", Backup::Carried),
    // Part of every cache key (03 §9.6): starting again from 0 could meet entries a CDN
    // still holds from before.
    ("visibility_epoch", Backup::Carried),
    // The restore's own migrations write it.
    ("migration", Backup::Left),
    // The rebuild writes positions and has no queued fan-out left over.
    ("projection_state", Backup::Left),
    ("projection_work", Backup::Left),
    // Purges for caches a restored instance does not have.
    ("purge", Backup::Left),
];

/// Why the live schema disagrees with [`OPS_TABLES`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum OpsError {
    /// A table in the database that this crate does not classify.
    #[error("ops.{0} has no backup class")]
    Unclassified(String),
    /// A table this crate lists that the database lacks.
    #[error("ops.{0} is classified but does not exist")]
    Missing(String),
    /// The comment says something else.
    #[error("ops.{table} is `{listed}` here but `{comment:?}` in the database")]
    Disagrees {
        /// The table.
        table: String,
        /// This crate's class.
        listed: &'static str,
        /// The database's comment.
        comment: Option<String>,
    },
    /// The query failed.
    #[error("ops: {0}")]
    Sql(String),
}

/// Checks that every table in `ops` is classified, exists, and carries its class as its
/// comment.
pub async fn check<C: GenericClient>(client: &C) -> Result<(), OpsError> {
    let rows = client
        .query(
            "SELECT c.relname, obj_description(c.oid, 'pg_class')
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = 'ops' AND c.relkind IN ('r', 'p')",
            &[],
        )
        .await
        .map_err(|e| OpsError::Sql(e.to_string()))?;
    let live: Vec<(String, Option<String>)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
    for (table, comment) in &live {
        let Some((_, class)) = OPS_TABLES.iter().find(|(t, _)| t == table) else {
            return Err(OpsError::Unclassified(table.clone()));
        };
        let want = format!("backup: {}", class.as_str());
        if comment.as_deref() != Some(want.as_str()) {
            return Err(OpsError::Disagrees {
                table: table.clone(),
                listed: class.as_str(),
                comment: comment.clone(),
            });
        }
    }
    for (table, _) in OPS_TABLES {
        if !live.iter().any(|(t, _)| t == table) {
            return Err(OpsError::Missing((*table).to_string()));
        }
    }
    Ok(())
}

/// The tables a backup carries in the clear, in [`OPS_TABLES`]' order.
#[must_use]
pub fn carried_tables() -> Vec<&'static str> {
    of(Backup::Carried)
}

/// The tables a backup carries encrypted with `private`.
#[must_use]
pub fn sealed_tables() -> Vec<&'static str> {
    of(Backup::Sealed)
}

fn of(class: Backup) -> Vec<&'static str> {
    OPS_TABLES
        .iter()
        .filter(|(_, c)| *c == class)
        .map(|(t, _)| *t)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_round_trip() {
        for c in [Backup::Carried, Backup::Sealed, Backup::Left] {
            assert_eq!(Backup::parse(c.as_str()), Some(c));
        }
        assert_eq!(Backup::parse("kept"), None);
    }

    #[test]
    fn every_table_once() {
        let mut names: Vec<_> = OPS_TABLES.iter().map(|(t, _)| *t).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), OPS_TABLES.len());
        assert!(carried_tables().contains(&"job_queue"));
        assert_eq!(sealed_tables(), vec!["delivery"]);
    }
}
