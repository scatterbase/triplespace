//! The roles and grants that enforce `private`'s privacy (0013 §4), applied so that they
//! converge: [`ensure`] creates any role that is missing and gives every grant the
//! migrations give, then each partition's, and the migration runner calls it on every run.
//! A database restored from `pg_dump` onto another server, which has none of the roles,
//! or restored with `--no-privileges`, is made whole by `triplespace migrate` (08 §6.7).
//!
//! A log partition's child table is created when the partition is, so its grant is given
//! then, by [`grant_partition`], and again by every [`ensure`]. The accounts partition is
//! hidden from every role but the accounts role: its child table by its grant, and its rows
//! in the parent `log.record` by row-level security ([`POLICIES`]), since Postgres checks
//! privileges only on the table a query names.

use tokio_postgres::GenericClient;

use scatter_log_postgres::ids::{partition_from_db, record_table};

/// The roles the migrations create.
pub mod role {
    /// The server and the ingester.
    pub const SERVER: &str = "ts_server";
    /// `triplespace-accounts`.
    pub const ACCOUNTS: &str = "ts_accounts";
    /// `triplespace-notify`.
    pub const NOTIFY: &str = "ts_notify";
    /// `triplespace-federation`.
    pub const FEDERATION: &str = "ts_federation";
    /// `verify`, read-only on the log.
    pub const VERIFY: &str = "ts_verify";

    /// Every role, in the order `0004_roles` creates them.
    pub const ALL: [&str; 5] = [SERVER, ACCOUNTS, NOTIFY, FEDERATION, VERIFY];
}

/// The name of the partition hidden from every role but [`role::ACCOUNTS`].
pub const ACCOUNTS_PARTITION: &str = "accounts";

/// Every grant the migrations give, in their order: `0004_roles`'s after its role
/// creation, then each later migration's. Every statement is idempotent. A migration that
/// adds a grant adds it here too: `every_migration_grant_is_ensured` fails until it does,
/// and the database test `ensure_restores_what_migrations_granted` compares the result.
pub const GRANTS: &str = "
GRANT USAGE ON SCHEMA log, view, ops TO ts_server, ts_accounts, ts_notify, ts_federation;
GRANT USAGE ON SCHEMA log TO ts_verify;
GRANT USAGE ON SCHEMA private TO ts_accounts, ts_notify, ts_federation;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA log, view, ops TO ts_server, ts_accounts, ts_notify, ts_federation;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA log, view, ops TO ts_server, ts_accounts, ts_notify, ts_federation;
GRANT SELECT ON ALL TABLES IN SCHEMA log TO ts_verify;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA private TO ts_accounts;
ALTER DEFAULT PRIVILEGES IN SCHEMA log, view, ops GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO ts_server, ts_accounts, ts_notify, ts_federation;
ALTER DEFAULT PRIVILEGES IN SCHEMA log, view, ops GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO ts_server, ts_accounts, ts_notify, ts_federation;
ALTER DEFAULT PRIVILEGES IN SCHEMA log GRANT SELECT ON TABLES TO ts_verify;
ALTER DEFAULT PRIVILEGES IN SCHEMA private GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO ts_accounts;
GRANT SELECT, INSERT, UPDATE, DELETE ON private.inbox, private.inbox_state TO ts_notify, ts_accounts;
GRANT SELECT ON private.watch TO ts_notify;
GRANT SELECT ON private.email TO ts_notify;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA private TO ts_accounts;
ALTER DEFAULT PRIVILEGES IN SCHEMA private GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO ts_accounts;
GRANT SELECT ON private.preference TO ts_notify;
";

/// The row-level security of `log.record` that `0034_record_policies` gives, each policy
/// created only where it is missing: a restore onto a server without the roles a policy
/// names leaves it out, and the table, its security enabled, then shows the public roles
/// nothing. Creating a policy locks `log.record`, so one that exists is left alone. A
/// migration that adds or changes a policy adds it here too:
/// `every_migration_grant_is_ensured` fails until it does.
pub const POLICIES: &str = "
ALTER TABLE log.record ENABLE ROW LEVEL SECURITY;
DO $$ BEGIN
IF NOT EXISTS (SELECT 1 FROM pg_policy WHERE polrelid = 'log.record'::regclass AND polname = 'ts_accounts_reads_all') THEN
CREATE POLICY ts_accounts_reads_all ON log.record TO ts_accounts USING (true) WITH CHECK (true);
END IF;
IF NOT EXISTS (SELECT 1 FROM pg_policy WHERE polrelid = 'log.record'::regclass AND polname = 'ts_public_skips_accounts') THEN
CREATE POLICY ts_public_skips_accounts ON log.record TO ts_server, ts_notify, ts_federation, ts_verify USING (partition NOT IN (SELECT p.partition FROM log.partition p WHERE p.name = 'accounts')) WITH CHECK (partition NOT IN (SELECT p.partition FROM log.partition p WHERE p.name = 'accounts'));
END IF;
END $$;
";

/// What [`ensure`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ensured {
    /// The roles it created, which the server did not have.
    pub created_roles: Vec<String>,
    /// The partitions whose record tables it granted.
    pub partitions: usize,
}

/// Creates every role in [`role::ALL`] the server lacks, returning those it created.
/// Needs `CREATEROLE`. The migration runner calls it before any migration, so that
/// `0004_roles` finds them made: two sessions creating one role at once fail with
/// `unique_violation`, which `0004_roles` does not catch, and this does.
pub async fn ensure_roles<C: GenericClient>(
    client: &C,
) -> Result<Vec<String>, tokio_postgres::Error> {
    let mut created = Vec::new();
    let existing: Vec<String> = client
        .query(
            "SELECT rolname::text FROM pg_roles WHERE rolname = ANY($1)",
            &[&role::ALL.as_slice()],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    for r in role::ALL {
        if existing.iter().any(|e| e == r) {
            continue;
        }
        // Another session may create it between the check and here, as two databases on
        // one server migrating at once would: that is what was wanted. A concurrent
        // creation is reported as `unique_violation`, a prior one as `duplicate_object`.
        client
            .batch_execute(&format!(
                "DO $$ BEGIN CREATE ROLE {r} NOLOGIN; \
                 EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL; END $$;"
            ))
            .await?;
        created.push(r.to_string());
    }
    Ok(created)
}

/// Creates every role the server lacks ([`ensure_roles`]), then gives [`GRANTS`], any
/// missing policy of [`POLICIES`], and each partition's grant. Idempotent; run by the migration runner after every run, so that a
/// database restored without its roles or its grants regains them (0013 §4; 08 §6.7).
///
/// Granting needs to own the objects, as the role that ran the migrations, or restored the
/// database with `--no-owner`, does. Default privileges are set for the role that runs
/// it, which is the one that will create later tables and partitions.
pub async fn ensure<C: GenericClient>(client: &C) -> Result<Ensured, tokio_postgres::Error> {
    let mut out = Ensured {
        created_roles: ensure_roles(client).await?,
        partitions: 0,
    };
    client.batch_execute(GRANTS).await?;
    client.batch_execute(POLICIES).await?;
    let rows = client
        .query(
            "SELECT partition, name FROM log.partition ORDER BY partition",
            &[],
        )
        .await?;
    for row in &rows {
        let partition = partition_from_db(row.get::<_, i64>(0));
        let name: String = row.get(1);
        grant_partition(client, partition, name == ACCOUNTS_PARTITION).await?;
        out.partitions += 1;
    }
    Ok(out)
}

/// Grants a partition's record table to the roles that may see it. An `accounts`
/// partition is granted to the accounts role alone and revoked from the others.
pub async fn grant_partition<C: GenericClient>(
    client: &C,
    partition: u64,
    is_accounts: bool,
) -> Result<(), tokio_postgres::Error> {
    let table = record_table(partition);
    let sql = if is_accounts {
        format!(
            "REVOKE ALL ON {table} FROM {}, {}, {}, {};
             GRANT SELECT, INSERT, UPDATE, DELETE ON {table} TO {};",
            role::SERVER,
            role::NOTIFY,
            role::FEDERATION,
            role::VERIFY,
            role::ACCOUNTS
        )
    } else {
        format!(
            "GRANT SELECT, INSERT, UPDATE, DELETE ON {table} TO {}, {}, {}, {};
             GRANT SELECT ON {table} TO {};",
            role::SERVER,
            role::ACCOUNTS,
            role::NOTIFY,
            role::FEDERATION,
            role::VERIFY
        )
    };
    client.batch_execute(&sql).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalized(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Every `GRANT`, `ALTER DEFAULT PRIVILEGES`, `CREATE POLICY` and row-level security
    /// statement in a migration is in [`GRANTS`] or [`POLICIES`], so that a restored
    /// database regains it (0013 §4).
    #[test]
    fn every_migration_grant_is_ensured() {
        let ensured = normalized(&format!("{GRANTS}{POLICIES}"));
        let mut seen = 0;
        for (crate_name, m) in crate::migrator().migrations() {
            let sql = m
                .sql
                .lines()
                .filter(|l| !l.trim_start().starts_with("--"))
                .collect::<Vec<_>>()
                .join("\n");
            for stmt in sql.split(';') {
                let stmt = normalized(stmt);
                let upper = stmt.to_ascii_uppercase();
                if upper.starts_with("GRANT ")
                    || upper.starts_with("REVOKE ")
                    || upper.starts_with("ALTER DEFAULT PRIVILEGES")
                    || upper.starts_with("CREATE POLICY")
                    || upper.starts_with("ALTER POLICY")
                    || upper.starts_with("DROP POLICY")
                    || upper.contains("ROW LEVEL SECURITY")
                {
                    seen += 1;
                    assert!(
                        ensured.contains(&format!("{stmt};")),
                        "{crate_name}/{}: `{stmt}` is not in grants::GRANTS",
                        m.name
                    );
                }
            }
        }
        assert!(seen >= 20, "found only {seen} grant statements");
    }

    /// The policies hide the partition [`ensure`] and [`grant_partition`]'s callers call
    /// the accounts partition.
    #[test]
    fn the_policies_name_the_accounts_partition() {
        assert!(POLICIES.contains(&format!("p.name = '{ACCOUNTS_PARTITION}'")));
    }
}
