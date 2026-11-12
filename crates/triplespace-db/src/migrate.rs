//! The migration runner (0005 rule 9; 0033 §4): each crate embeds its SQL files, this
//! applies them in build order and records each in `ops.migration` with a checksum. A
//! migration is applied once; a recorded migration whose SQL has changed is an error,
//! since a change to a schema is a new migration, never an edit.
//!
//! A migrator made `with_grants` then runs [`crate::grants::ensure`] on every run, applied
//! or not: roles belong to the server, not the database, so a database restored onto
//! another server has its `ops.migration` rows but not the roles `0004_roles` made, and
//! one restored with `--no-privileges` has neither its grants (0013 §4; 08 §6.7).

use sha2::{Digest as _, Sha256};
use tokio_postgres::GenericClient;

use scatter_log_postgres::Migration;

/// Why migrating stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MigrateError {
    /// The database refused.
    #[error("{crate_name}/{name}: {source}")]
    Sql {
        /// The crate.
        crate_name: String,
        /// The migration.
        name: String,
        /// What Postgres said.
        #[source]
        source: tokio_postgres::Error,
    },
    /// Creating the roles or giving the grants failed (0013 §4).
    #[error("roles and grants: {source}")]
    Grants {
        /// What Postgres said.
        #[source]
        source: tokio_postgres::Error,
    },
    /// The bookkeeping table could not be reached.
    #[error("ops.migration: {0}")]
    Bookkeeping(#[from] tokio_postgres::Error),
    /// A recorded migration's SQL has changed since it was applied.
    #[error(
        "{crate_name}/{name} was applied with checksum {recorded}, but its SQL now hashes to {current}"
    )]
    Changed {
        /// The crate.
        crate_name: String,
        /// The migration.
        name: String,
        /// The checksum in `ops.migration`.
        recorded: String,
        /// The checksum of the embedded SQL.
        current: String,
    },
    /// A backup names a migration this binary does not have: a newer binary wrote it.
    #[error(
        "the backup was written under {crate_name}/{name}, a migration this binary does not \
         have: restore it with the version that wrote it, or a newer one"
    )]
    Unknown {
        /// The crate.
        crate_name: String,
        /// The migration.
        name: String,
    },
}

/// What a run did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// `(crate, name)` of each migration applied this run.
    pub applied: Vec<(String, String)>,
    /// `(crate, name)` of each already recorded.
    pub skipped: Vec<(String, String)>,
    /// What [`crate::grants::ensure`] did, for a migrator made `with_grants`.
    pub grants: Option<crate::grants::Ensured>,
}

/// The status of one migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The crate.
    pub crate_name: String,
    /// The migration.
    pub name: String,
    /// The checksum of the embedded SQL.
    pub checksum: String,
    /// Whether and when it was applied.
    pub applied_at: Option<std::time::SystemTime>,
}

/// Migrations from several crates, in order.
#[derive(Debug, Clone, Default)]
pub struct Migrator {
    sets: Vec<(&'static str, &'static [Migration])>,
    grants: bool,
}

/// SHA-256 of the SQL, lower-case hex.
#[must_use]
pub fn checksum(sql: &str) -> String {
    use std::fmt::Write as _;
    Sha256::digest(sql.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Applies one migration in a transaction of its own, with its bookkeeping row.
async fn apply_one(
    client: &mut tokio_postgres::Client,
    crate_name: &str,
    m: &Migration,
    checksum: &str,
) -> Result<(), MigrateError> {
    let tx = client.transaction().await?;
    tx.batch_execute(m.sql)
        .await
        .map_err(|source| MigrateError::Sql {
            crate_name: crate_name.into(),
            name: m.name.into(),
            source,
        })?;
    tx.execute(
        "INSERT INTO ops.migration (crate_name, name, checksum) VALUES ($1, $2, $3)",
        &[&crate_name, &m.name, &checksum],
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

const BOOKKEEPING: &str = "
CREATE SCHEMA IF NOT EXISTS ops;
CREATE TABLE IF NOT EXISTS ops.migration (
  crate_name text NOT NULL,
  name       text NOT NULL,
  checksum   text NOT NULL,
  applied_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (crate_name, name)
);";

impl Migrator {
    /// No migrations yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a crate's migrations after the ones so far.
    #[must_use]
    pub fn with(mut self, crate_name: &'static str, migrations: &'static [Migration]) -> Self {
        self.sets.push((crate_name, migrations));
        self
    }

    /// Runs [`crate::grants::ensure`] after every [`Migrator::apply`]: for a set that
    /// includes `triplespace-db`'s `0004_roles` and the migrations its grants name.
    #[must_use]
    pub fn with_grants(mut self) -> Self {
        self.grants = true;
        self
    }

    /// Every migration in order.
    pub fn migrations(&self) -> impl Iterator<Item = (&'static str, &'static Migration)> + '_ {
        self.sets
            .iter()
            .flat_map(|(c, ms)| ms.iter().map(move |m| (*c, m)))
    }

    /// Creates the bookkeeping table if needed.
    pub async fn prepare<C: GenericClient>(&self, client: &C) -> Result<(), MigrateError> {
        client.batch_execute(BOOKKEEPING).await?;
        Ok(())
    }

    /// Applies every migration not yet recorded, in order, each in its own transaction
    /// with its bookkeeping row; stops at the first failure. Then, for a migrator made
    /// `with_grants`, creates any missing role and gives every grant, in one transaction.
    pub async fn apply(
        &self,
        client: &mut tokio_postgres::Client,
    ) -> Result<Applied, MigrateError> {
        self.prepare(client).await?;
        // Roles first, outside the migrations' transactions, so that `0004_roles` finds
        // them made rather than racing another database's run to make them.
        let created_roles = if self.grants {
            crate::grants::ensure_roles(&*client)
                .await
                .map_err(|source| MigrateError::Grants { source })?
        } else {
            Vec::new()
        };
        let mut report = Applied::default();
        for (crate_name, m) in self.migrations() {
            let current = checksum(m.sql);
            let recorded = client
                .query_opt(
                    "SELECT checksum FROM ops.migration WHERE crate_name = $1 AND name = $2",
                    &[&crate_name, &m.name],
                )
                .await?;
            if let Some(row) = recorded {
                let recorded: String = row.get(0);
                if recorded != current {
                    return Err(MigrateError::Changed {
                        crate_name: crate_name.into(),
                        name: m.name.into(),
                        recorded,
                        current,
                    });
                }
                report.skipped.push((crate_name.into(), m.name.into()));
                continue;
            }
            apply_one(client, crate_name, m, &current).await?;
            report.applied.push((crate_name.into(), m.name.into()));
        }
        if self.grants {
            let tx = client.transaction().await?;
            let mut ensured = crate::grants::ensure(&tx)
                .await
                .map_err(|source| MigrateError::Grants { source })?;
            tx.commit().await?;
            ensured.created_roles = created_roles;
            report.grants = Some(ensured);
        }
        Ok(report)
    }

    /// Brings an empty database to the schema a backup was written under (0103 §4, A11):
    /// applies, in this binary's order, exactly the migrations `recorded` names, each
    /// `(crate, name, checksum)`, and none of the later ones, which [`Migrator::apply`]
    /// then runs over the backup's rows as it would have over the live database's. Makes
    /// the roles, for a migrator made `with_grants`, but gives no grants: they name tables
    /// later migrations make. Returns whether this binary has migrations the backup lacks.
    ///
    /// # Errors
    ///
    /// [`MigrateError::Unknown`] for a migration this binary lacks, and
    /// [`MigrateError::Changed`] for one whose SQL differs from what the backup recorded.
    pub async fn apply_recorded(
        &self,
        client: &mut tokio_postgres::Client,
        recorded: &[(String, String, String)],
    ) -> Result<bool, MigrateError> {
        self.prepare(client).await?;
        if self.grants {
            crate::grants::ensure_roles(&*client)
                .await
                .map_err(|source| MigrateError::Grants { source })?;
        }
        for (crate_name, name, _) in recorded {
            if !self
                .migrations()
                .any(|(c, m)| c == crate_name && m.name == name)
            {
                return Err(MigrateError::Unknown {
                    crate_name: crate_name.clone(),
                    name: name.clone(),
                });
            }
        }
        let mut older = false;
        for (crate_name, m) in self.migrations() {
            let Some((_, _, sum)) = recorded
                .iter()
                .find(|(c, n, _)| c == crate_name && n == m.name)
            else {
                older = true;
                continue;
            };
            let current = checksum(m.sql);
            if *sum != current {
                return Err(MigrateError::Changed {
                    crate_name: crate_name.into(),
                    name: m.name.into(),
                    recorded: sum.clone(),
                    current,
                });
            }
            let applied = client
                .query_opt(
                    "SELECT 1 FROM ops.migration WHERE crate_name = $1 AND name = $2",
                    &[&crate_name, &m.name],
                )
                .await?;
            if applied.is_none() {
                apply_one(client, crate_name, m, &current).await?;
            }
        }
        Ok(older)
    }

    /// The `(crate, name)` of every migration the database has not applied, in order:
    /// what `triplespace migrate` would apply, and what a server refuses to start over.
    pub async fn pending<C: GenericClient>(
        &self,
        client: &C,
    ) -> Result<Vec<(String, String)>, MigrateError> {
        Ok(self
            .status(client)
            .await?
            .into_iter()
            .filter(|s| s.applied_at.is_none())
            .map(|s| (s.crate_name, s.name))
            .collect())
    }

    /// The status of every migration against the database. Reads only: a database without
    /// `ops.migration` has applied nothing, and the server, which asks before it starts,
    /// connects as a login that may not create the table (0013 §4, A99).
    pub async fn status<C: GenericClient>(&self, client: &C) -> Result<Vec<Status>, MigrateError> {
        let recorded: bool = client
            .query_one("SELECT to_regclass('ops.migration') IS NOT NULL", &[])
            .await?
            .get(0);
        let rows = if recorded {
            client
                .query(
                    "SELECT crate_name, name, applied_at FROM ops.migration",
                    &[],
                )
                .await?
        } else {
            Vec::new()
        };
        let mut out = Vec::new();
        for (crate_name, m) in self.migrations() {
            let applied_at = rows
                .iter()
                .find(|r| r.get::<_, String>(0) == crate_name && r.get::<_, String>(1) == m.name)
                .map(|r| r.get::<_, std::time::SystemTime>(2));
            out.push(Status {
                crate_name: crate_name.into(),
                name: m.name.into(),
                checksum: checksum(m.sql),
                applied_at,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_and_checksums() {
        let m = crate::migrator();
        let names: Vec<_> = m
            .migrations()
            .map(|(c, m)| format!("{c}/{}", m.name))
            .collect();
        assert_eq!(names[0], "scatter-log-postgres/0001_log");
        assert_eq!(names[1], "scatter-log-postgres/0002_partition_origin");
        assert_eq!(names[2], "triplespace-db/0001_ops");
        assert!(names.last().unwrap().ends_with("0038_ops_backup_classes"));
        assert_eq!(checksum("").len(), 64);
        assert_ne!(checksum("a"), checksum("b"));
    }
}
