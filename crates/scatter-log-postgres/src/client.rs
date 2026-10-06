//! What the store's SQL runs on: a connection that prepares each statement once.
//!
//! Every statement here is a point lookup or a single-row write, and the write path
//! sends tens of them per record (0013 §7). Sent as text, each costs a parse and a plan
//! on the server and a second round trip from the client. [`PgClient`] runs them through
//! a prepared statement instead: on a pooled connection ([`deadpool_postgres`]) the
//! statement is prepared once per connection and cached; on a plain
//! [`tokio_postgres::Client`] or [`tokio_postgres::Transaction`], which the tests and
//! one-off commands use, it is prepared per call, which is what the driver did anyway.
//!
//! The method names shadow the driver's, so a call site reads the same whichever client
//! it holds; only the statement argument is always `&str`.

use std::future::Future;

use tokio_postgres::types::ToSql;
use tokio_postgres::{Error, Row, Statement};

/// A connection the store's functions run on.
pub trait PgClient: Sync {
    /// The connection itself.
    fn raw(&self) -> &tokio_postgres::Client;

    /// The prepared statement for `sql`: cached per connection where the connection is
    /// pooled, prepared afresh otherwise.
    fn statement(&self, sql: &str) -> impl Future<Output = Result<Statement, Error>> + Send;

    /// Like `Client::query`, through the prepared statement.
    fn query(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl Future<Output = Result<Vec<Row>, Error>> + Send {
        async move {
            let s = self.statement(sql).await?;
            self.raw().query(&s, params).await
        }
    }

    /// Like `Client::query_opt`.
    fn query_opt(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl Future<Output = Result<Option<Row>, Error>> + Send {
        async move {
            let s = self.statement(sql).await?;
            self.raw().query_opt(&s, params).await
        }
    }

    /// Like `Client::query_one`.
    fn query_one(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl Future<Output = Result<Row, Error>> + Send {
        async move {
            let s = self.statement(sql).await?;
            self.raw().query_one(&s, params).await
        }
    }

    /// Like `Client::execute`.
    fn execute(
        &self,
        sql: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> impl Future<Output = Result<u64, Error>> + Send {
        async move {
            let s = self.statement(sql).await?;
            self.raw().execute(&s, params).await
        }
    }

    /// Like `Client::batch_execute`: several statements as text, nothing prepared.
    fn batch_execute(&self, sql: &str) -> impl Future<Output = Result<(), Error>> + Send {
        self.raw().batch_execute(sql)
    }
}

impl PgClient for tokio_postgres::Client {
    fn raw(&self) -> &tokio_postgres::Client {
        self
    }

    fn statement(&self, sql: &str) -> impl Future<Output = Result<Statement, Error>> + Send {
        self.prepare(sql)
    }
}

impl PgClient for tokio_postgres::Transaction<'_> {
    fn raw(&self) -> &tokio_postgres::Client {
        self.client()
    }

    fn statement(&self, sql: &str) -> impl Future<Output = Result<Statement, Error>> + Send {
        self.prepare(sql)
    }
}

impl PgClient for deadpool_postgres::ClientWrapper {
    fn raw(&self) -> &tokio_postgres::Client {
        self
    }

    fn statement(&self, sql: &str) -> impl Future<Output = Result<Statement, Error>> + Send {
        self.prepare_cached(sql)
    }
}

impl PgClient for deadpool_postgres::Object {
    fn raw(&self) -> &tokio_postgres::Client {
        self
    }

    fn statement(&self, sql: &str) -> impl Future<Output = Result<Statement, Error>> + Send {
        deadpool_postgres::ClientWrapper::prepare_cached(self, sql)
    }
}
