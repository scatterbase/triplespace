//! Staging and sending mail (0100 §4–§5; 16 §5.7; 03 §4.16): every outgoing message is
//! written complete to `ops.delivery` before anything tries to send it, and a worker
//! hands due rows to the transport.
//!
//! - [`stage`] writes one row: recipient, sender, subject, a plain-text body in the
//!   recipient's language, a `Message-ID`, the stream, and an expiry; it raises the
//!   `ts_delivery` notification so a listening worker wakes at once.
//! - [`deliver_once`] takes due rows with `FOR UPDATE SKIP LOCKED` and sends each. A
//!   row is deleted as soon as the transport accepts its message. A temporary failure is
//!   retried after 1, 2, 4 … minutes, at most an hour apart, until the row expires; a
//!   permanent failure is kept with its error until it expires and never retried. Expired
//!   rows are deleted first, so a code is never delivered after it has stopped working.
//! - [`texts`] renders the messages 07 §2.9 names: the verification code, the notice that
//!   an address already belongs to a login, and the security notices, each from the
//!   interface messages in the language of the request that caused it.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use scatter_mail::{MailTransport, Mailbox, Message, Outcome, Stream};
use tokio_postgres::Client;

/// The Postgres notification channel a staged row raises.
pub const CHANNEL: &str = "ts_delivery";
/// How long a security notice stays in the queue: three days.
pub const NOTICE_LIFETIME: Duration = Duration::from_hours(72);
/// The longest wait between two attempts at one message: an hour.
const MAX_BACKOFF_SECS: u64 = 3600;
/// Rows one pass takes.
const BATCH: i64 = 20;

/// A message to stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// The recipient's address.
    pub to: String,
    /// Which stream it belongs on.
    pub stream: Stream,
    /// The subject.
    pub subject: String,
    /// The plain-text body.
    pub body: String,
    /// The sender's display name: the instance's, or the tenant's for its
    /// notifications (16 §5.7).
    pub sender_name: Option<String>,
    /// When the row is dropped unsent.
    pub expires: SystemTime,
}

/// Why staging failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum QueueError {
    /// The store failed.
    #[error(transparent)]
    Sql(#[from] tokio_postgres::Error),
    /// The random source failed.
    #[error("random: {0}")]
    Random(String),
}

/// A `Message-ID`: 16 random bytes in hex at the sender's domain.
fn message_id(from: &str) -> Result<String, QueueError> {
    use std::fmt::Write as _;
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| QueueError::Random(e.to_string()))?;
    let hex = bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    });
    let domain = from.rsplit_once('@').map_or("localhost", |(_, d)| d);
    Ok(format!("{hex}@{domain}"))
}

/// Stages one message (16 §5.7): the row's ID.
///
/// # Errors
///
/// If the store fails.
pub async fn stage<C: tokio_postgres::GenericClient>(
    client: &C,
    from: &str,
    m: &Outgoing,
) -> Result<i64, QueueError> {
    let id = message_id(from)?;
    let row = client
        .query_one(
            "INSERT INTO ops.delivery (kind, stream, recipient, sender, sender_name, subject, body, message_id, expires)
             VALUES ('email', $1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
            &[
                &m.stream.as_str(),
                &m.to,
                &from,
                &m.sender_name,
                &m.subject,
                &m.body,
                &id,
                &m.expires,
            ],
        )
        .await?;
    client.batch_execute(&format!("NOTIFY {CHANNEL}")).await?;
    Ok(row.get(0))
}

/// What one pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pass {
    /// Messages the transport accepted.
    pub accepted: usize,
    /// Temporary failures, left for another attempt.
    pub retried: usize,
    /// Permanent failures, kept with their error.
    pub failed: usize,
    /// Rows dropped unsent because they expired.
    pub expired: u64,
}

impl Pass {
    /// Whether the pass found nothing to do.
    #[must_use]
    pub fn idle(&self) -> bool {
        self.accepted + self.retried + self.failed == 0
    }
}

/// The wait after the `n`th failed attempt: 1, 2, 4 … minutes, at most an hour.
#[must_use]
pub fn backoff(attempts: i32) -> Duration {
    let n = u32::try_from(attempts.max(1) - 1).unwrap_or(0).min(16);
    Duration::from_secs((60u64 << n).min(MAX_BACKOFF_SECS))
}

/// One pass of the worker (16 §5.7): drops expired rows, takes up to a batch of due rows
/// with `FOR UPDATE SKIP LOCKED`, and sends each through the transport. With [`stage`] and
/// the server's listener this is the staging half of what implements 0100 §5 (A2).
///
/// # Errors
///
/// If the store fails.
pub async fn deliver_once(
    client: &mut Client,
    transport: &dyn MailTransport,
) -> Result<Pass, tokio_postgres::Error> {
    let mut pass = Pass {
        expired: client
            .execute("DELETE FROM ops.delivery WHERE expires <= now()", &[])
            .await?,
        ..Pass::default()
    };
    let tx = client.transaction().await?;
    let rows = tx
        .query(
            "SELECT id, stream, recipient, sender, sender_name, subject, body, message_id, attempts
             FROM ops.delivery WHERE NOT failed AND next_attempt <= now()
             ORDER BY next_attempt LIMIT $1 FOR UPDATE SKIP LOCKED",
            &[&BATCH],
        )
        .await?;
    for r in &rows {
        let id: i64 = r.get(0);
        let stream: String = r.get(1);
        let sender: String = r.get(3);
        let sender_name: Option<String> = r.get(4);
        let attempts: i32 = r.get(8);
        let message = Message {
            from: Mailbox {
                name: sender_name,
                address: sender,
            },
            to: r.get(2),
            subject: r.get(5),
            body: r.get(6),
            message_id: r.get(7),
            date: SystemTime::now(),
            stream: Stream::parse(&stream).unwrap_or(Stream::Transactional),
        };
        match transport.send(&message).await {
            Outcome::Accepted => {
                tx.execute("DELETE FROM ops.delivery WHERE id = $1", &[&id])
                    .await?;
                pass.accepted += 1;
            }
            Outcome::Temporary(error) => {
                let attempts = attempts + 1;
                let next = SystemTime::now() + backoff(attempts);
                tx.execute(
                    "UPDATE ops.delivery SET attempts = $2, next_attempt = $3, last_error = $4 WHERE id = $1",
                    &[&id, &attempts, &next, &error],
                )
                .await?;
                eprintln!(
                    "mail: to {} \"{}\" attempt {attempts} failed, retrying: {error}",
                    message.to, message.subject
                );
                pass.retried += 1;
            }
            Outcome::Permanent(error) => {
                tx.execute(
                    "UPDATE ops.delivery SET attempts = attempts + 1, failed = true, last_error = $2 WHERE id = $1",
                    &[&id, &error],
                )
                .await?;
                eprintln!(
                    "mail: to {} \"{}\" failed for good: {error}",
                    message.to, message.subject
                );
                pass.failed += 1;
            }
        }
    }
    tx.commit().await?;
    Ok(pass)
}

/// When the next due row is, if any: how long a worker may sleep.
///
/// # Errors
///
/// If the store fails.
pub async fn next_due<C: tokio_postgres::GenericClient>(
    client: &C,
) -> Result<Option<SystemTime>, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT min(next_attempt) FROM ops.delivery WHERE NOT failed",
            &[],
        )
        .await?;
    Ok(row.and_then(|r| r.get::<_, Option<SystemTime>>(0)))
}

/// A queued row, as the operator or a test sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    /// The row.
    pub id: i64,
    /// The recipient.
    pub recipient: String,
    /// The subject.
    pub subject: String,
    /// Attempts so far.
    pub attempts: i32,
    /// Whether it failed for good.
    pub failed: bool,
    /// The last error.
    pub last_error: Option<String>,
}

/// Every row in the queue, oldest first.
///
/// # Errors
///
/// If the store fails.
pub async fn queued<C: tokio_postgres::GenericClient>(
    client: &C,
) -> Result<Vec<Queued>, tokio_postgres::Error> {
    let rows = client
        .query(
            "SELECT id, recipient, subject, attempts, failed, last_error FROM ops.delivery ORDER BY id",
            &[],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| Queued {
            id: r.get(0),
            recipient: r.get(1),
            subject: r.get(2),
            attempts: r.get(3),
            failed: r.get(4),
            last_error: r.get(5),
        })
        .collect())
}

/// The interface messages a message is written from: `get` a key, or `with` its
/// arguments substituted for `$1`, `$2`, …, in the language of the request.
pub trait Texts {
    /// A message by key.
    fn get(&self, key: &str) -> String;
    /// A message with its arguments.
    fn with(&self, key: &str, args: &[&str]) -> String;
}

/// `YYYY-MM-DD HH:MM UTC` of a time.
#[must_use]
pub fn utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    let rem = secs % 86_400;
    format!(
        "{y:04}-{mo:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// What a security notice is about (07 §2.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// An address was verified.
    Verified,
    /// An address was made the primary.
    MadePrimary,
    /// An address was removed.
    Removed,
}

impl Notice {
    fn key(self) -> &'static str {
        match self {
            Self::Verified => "ts-mail-notice-verified",
            Self::MadePrimary => "ts-mail-notice-primary",
            Self::Removed => "ts-mail-notice-removed",
        }
    }
}

/// The messages 07 §2.9 names, rendered: each names the instance and never the login
/// name, and the subject carries no code.
pub mod texts {
    use super::{Notice, Outgoing, Texts, utc};
    use scatter_mail::Stream;
    use std::time::SystemTime;

    /// The verification message: the address, the code as `123 456`, when it expires, and
    /// that the code is useless without signing in.
    #[must_use]
    pub fn verification(
        t: &dyn Texts,
        instance: &str,
        account_url: &str,
        to: &str,
        code: &str,
        expires: SystemTime,
    ) -> Outgoing {
        Outgoing {
            to: to.to_string(),
            stream: Stream::Transactional,
            subject: t.with("ts-mail-code-subject", &[instance]),
            body: t.with(
                "ts-mail-code-body",
                &[instance, to, code, &utc(expires), account_url],
            ),
            sender_name: Some(instance.to_string()),
            expires,
        }
    }

    /// The message sent in place of a code when the address already belongs to a login.
    #[must_use]
    pub fn held_elsewhere(t: &dyn Texts, instance: &str, account_url: &str, to: &str) -> Outgoing {
        Outgoing {
            to: to.to_string(),
            stream: Stream::Transactional,
            subject: t.with("ts-mail-held-subject", &[instance]),
            body: t.with("ts-mail-held-body", &[instance, to, account_url]),
            sender_name: Some(instance.to_string()),
            expires: SystemTime::now() + super::NOTICE_LIFETIME,
        }
    }

    /// A security notice: what changed, the address concerned, the time, and where to
    /// review the login.
    #[must_use]
    pub fn notice(
        t: &dyn Texts,
        instance: &str,
        account_url: &str,
        to: &str,
        what: Notice,
        address: &str,
        at: SystemTime,
    ) -> Outgoing {
        let change = t.with(what.key(), &[instance, address, &utc(at)]);
        Outgoing {
            to: to.to_string(),
            stream: Stream::Transactional,
            subject: t.with("ts-mail-notice-subject", &[instance]),
            body: t.with("ts-mail-notice-body", &[&change, account_url]),
            sender_name: Some(instance.to_string()),
            expires: SystemTime::now() + super::NOTICE_LIFETIME,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_an_hour() {
        assert_eq!(backoff(1), Duration::from_mins(1));
        assert_eq!(backoff(2), Duration::from_mins(2));
        assert_eq!(backoff(3), Duration::from_mins(4));
        assert_eq!(backoff(7), Duration::from_hours(1));
        assert_eq!(backoff(40), Duration::from_hours(1));
        assert_eq!(backoff(0), Duration::from_mins(1));
    }

    #[test]
    fn times_print_in_utc() {
        assert_eq!(utc(UNIX_EPOCH), "1970-01-01 00:00 UTC");
        assert_eq!(
            utc(UNIX_EPOCH + Duration::from_secs(1_792_281_600 + 3661)),
            "2026-10-18 01:01 UTC"
        );
    }

    struct Plain;
    impl Texts for Plain {
        fn get(&self, key: &str) -> String {
            key.to_string()
        }
        fn with(&self, key: &str, args: &[&str]) -> String {
            format!("{key}({})", args.join("|"))
        }
    }

    /// The verification message carries the address, the code and the expiry and expires
    /// with the code; a notice names the change.
    #[test]
    fn messages_carry_their_arguments() {
        let expires = UNIX_EPOCH + Duration::from_hours(1);
        let m = texts::verification(
            &Plain,
            "Library Base",
            "https://lb/wiki/Special:Account",
            "a@b.org",
            "123 456",
            expires,
        );
        assert_eq!(m.subject, "ts-mail-code-subject(Library Base)");
        assert_eq!(
            m.body,
            "ts-mail-code-body(Library Base|a@b.org|123 456|1970-01-01 01:00 UTC|https://lb/wiki/Special:Account)"
        );
        assert_eq!(m.expires, expires);
        assert_eq!(m.stream, Stream::Transactional);
        let n = texts::notice(
            &Plain,
            "Library Base",
            "u",
            "a@b.org",
            Notice::Removed,
            "c@d.org",
            UNIX_EPOCH,
        );
        assert_eq!(
            n.body,
            "ts-mail-notice-body(ts-mail-notice-removed(Library Base|c@d.org|1970-01-01 00:00 UTC)|u)"
        );
    }
}
