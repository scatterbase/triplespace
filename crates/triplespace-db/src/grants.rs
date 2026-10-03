//! Grants that migrations cannot express: a log partition's child table is created when
//! the partition is, so its grant is given then. The accounts partition is owned by the
//! accounts role and hidden from the public role (0013 §4).

use tokio_postgres::GenericClient;

use scatter_log_postgres::ids::record_table;

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
