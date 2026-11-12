//! Scratch databases for tests: a test that makes one registers it here, and it is dropped
//! when the test's thread ends, whether the test passed or panicked. The test harness runs
//! each test on a thread of its own, so the thread's end is the test's.
//!
//! The databases are on the server `TRIPLESPACE_TEST_DATABASE_URL` names, which is also the
//! connection the drop uses.

use std::cell::RefCell;

/// The databases this thread made, dropped with it.
#[derive(Default)]
struct Made(Vec<String>);

impl Drop for Made {
    fn drop(&mut self) {
        let names = std::mem::take(&mut self.0);
        if names.is_empty() {
            return;
        }
        let Ok(url) = std::env::var("TRIPLESPACE_TEST_DATABASE_URL") else {
            return;
        };
        // The test's runtime is gone by now: drop them on a thread with one of its own.
        let _ = std::thread::spawn(move || drop_now(&url, &names)).join();
    }
}

thread_local! {
    static MADE: RefCell<Made> = RefCell::new(Made::default());
}

/// Drops `names` on the server at `url`, ignoring failures: a database already gone is
/// what was wanted.
fn drop_now(url: &str, names: &[String]) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    runtime.block_on(async {
        let Ok((client, connection)) = tokio_postgres::connect(url, tokio_postgres::NoTls).await
        else {
            return;
        };
        tokio::spawn(connection);
        for name in names {
            let _ = client
                .batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
                .await;
        }
    });
}

/// Drops the database `name` when this thread ends: call it right after creating one.
pub fn drop_at_thread_end(name: &str) {
    MADE.with_borrow_mut(|m| m.0.push(name.to_string()));
}

/// `url` connecting as `role`: the role is set when each connection opens (`-c role=…`), as
/// a login granted it would be. The test user needs `SET` on the role
/// (`docs/testing/gotchas.md`).
#[must_use]
pub fn as_role(url: &str, role: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}options=-c%20role%3D{role}")
}

/// The server's pools for a test against `url`, each part connecting as its own role
/// (0013 §4, A99), so that a test fails where a part reaches a table its role may not.
///
/// # Panics
///
/// If a pool cannot be built from the URL.
#[must_use]
pub fn pools(url: &str, size: usize) -> crate::pool::Pools {
    use crate::grants::role;
    crate::pool::Pools::new(
        &as_role(url, role::SERVER),
        Some(&as_role(url, role::ACCOUNTS)),
        Some(&as_role(url, role::NOTIFY)),
        size,
    )
    .expect("pools")
}
