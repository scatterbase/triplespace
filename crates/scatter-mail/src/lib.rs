//! Outgoing email (0100 §5; 16 §5.7): the message type, the [`MailTransport`] trait with
//! its three answers, and the transports a deployment chooses between with
//! `mail.transport`:
//!
//! - [`log::LogTransport`], the development sink: each message written as an `.eml` file
//!   to a directory, from which tests read verification codes.
//! - [`smtp::SmtpTransport`], the baseline: submission to a relay through `lettre` over
//!   rustls, `smtps://` for implicit TLS or `smtp://` with STARTTLS required, plaintext only
//!   to a loopback host.
//! - [`postmark::PostmarkTransport`], the first HTTP mail API: one JSON request per
//!   message, with the message streams that keep codes and security notices apart from
//!   notification digests.
//!
//! A message is plain text, UTF-8, with the RFC 5322 headers a transport needs and
//! `Auto-Submitted: auto-generated` (RFC 3834), so that autoresponders stay quiet. The one
//! canonical encoding of a message is [`Message::rfc5322`] (22 §1.2 rule 4): the `log` and
//! `smtp` transports send exactly those bytes, and `postmark` sends the same fields as JSON.
//!
//! Asynchronous, and shared with Scatterbase, which knows nothing of Triplespace's queue:
//! who stages, retries and expires messages is the caller's business (`ops.delivery`).

#![forbid(unsafe_code)]

pub mod log;
pub mod postmark;
pub mod smtp;

use std::future::Future;
use std::pin::Pin;
use std::time::SystemTime;

/// A boxed future, as the trait's method returns.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Which of a provider's streams a message belongs on (0100 §5): the transactional
/// stream for codes and security notices, a stream of its own for notifications and
/// digests. SMTP relays do not distinguish them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stream {
    /// A verification code, a security notice, or any other mail about the login.
    Transactional,
    /// A notification or a digest.
    Notification,
}

impl Stream {
    /// The word a queue stores.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transactional => "transactional",
            Self::Notification => "notification",
        }
    }

    /// From the word.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "transactional" => Some(Self::Transactional),
            "notification" => Some(Self::Notification),
            _ => None,
        }
    }
}

/// A mailbox: an address with an optional display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mailbox {
    /// The display name, shown before the address.
    pub name: Option<String>,
    /// The address, an RFC 5322 `addr-spec`.
    pub address: String,
}

impl Mailbox {
    /// An address with no display name.
    #[must_use]
    pub fn new(address: &str) -> Self {
        Self {
            name: None,
            address: address.to_string(),
        }
    }

    /// An address with a display name.
    #[must_use]
    pub fn named(name: &str, address: &str) -> Self {
        Self {
            name: Some(name.to_string()),
            address: address.to_string(),
        }
    }

    fn to_lettre(&self) -> Result<lettre::message::Mailbox, MailError> {
        let address: lettre::Address = self
            .address
            .parse()
            .map_err(|e| MailError::Address(format!("{}: {e}", self.address)))?;
        Ok(lettre::message::Mailbox::new(self.name.clone(), address))
    }

    /// `Name <address>`, or the address alone, as Postmark's `From` takes it.
    #[must_use]
    pub fn display(&self) -> String {
        match &self.name {
            Some(n) => format!("{} <{}>", quote_name(n), self.address),
            None => self.address.clone(),
        }
    }
}

/// A display name quoted where RFC 5322 needs it.
fn quote_name(name: &str) -> String {
    let plain = name
        .chars()
        .all(|c| c.is_alphanumeric() || " !#$%&'*+-/=?^_`{|}~".contains(c));
    if plain {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// One message to send: plain text, one recipient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The sender.
    pub from: Mailbox,
    /// The recipient's address.
    pub to: String,
    /// The subject.
    pub subject: String,
    /// The plain-text body.
    pub body: String,
    /// The `Message-ID`, without angle brackets: `{token}@{domain}`.
    pub message_id: String,
    /// The `Date`.
    pub date: SystemTime,
    /// Which stream the message belongs on.
    pub stream: Stream,
}

/// Why a message could not be built or a transport could not be made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MailError {
    /// An address that does not parse.
    #[error("not an address: {0}")]
    Address(String),
    /// A header or the message as a whole could not be built.
    #[error("message: {0}")]
    Message(String),
    /// A transport's configuration is wrong.
    #[error("{0}")]
    Config(String),
}

impl Message {
    /// The message as `lettre` builds it: the headers, then the body as UTF-8 text.
    fn to_lettre(&self) -> Result<lettre::Message, MailError> {
        use lettre::message::header::{ContentType, HeaderName, HeaderValue};
        let message_id = format!("<{}>", self.message_id);
        let auto = HeaderValue::dangerous_new_pre_encoded(
            HeaderName::new_from_ascii_str("Auto-Submitted"),
            "auto-generated".to_string(),
            "auto-generated".to_string(),
        );
        lettre::Message::builder()
            .from(self.from.to_lettre()?)
            .to(Mailbox::new(&self.to).to_lettre()?)
            .subject(self.subject.clone())
            .message_id(Some(message_id))
            .date(self.date)
            .raw_header(auto)
            .header(ContentType::TEXT_PLAIN)
            .body(self.body.clone())
            .map_err(|e| MailError::Message(e.to_string()))
    }

    /// The one canonical encoding of the message (22 §1.2 rule 4): RFC 5322 headers
    /// (`From`, `To`, `Subject`, `Message-ID`, `Date`, `Auto-Submitted`, `Content-Type`)
    /// and the body, as the `log` transport writes it and the `smtp` transport sends it.
    ///
    /// # Errors
    ///
    /// An address that does not parse, or a subject that cannot be encoded.
    pub fn rfc5322(&self) -> Result<Vec<u8>, MailError> {
        Ok(self.to_lettre()?.formatted())
    }
}

/// What a transport answers (0100 §5; 16 §5.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The transport took the message; the queue deletes its row.
    Accepted,
    /// Not now: a connection failure, a `4xx` SMTP reply, a provider's `429` or `5xx`.
    /// The queue retries with backoff until the row expires.
    Temporary(String),
    /// Never: a `5xx` SMTP reply, a provider's refusal of the recipient or the message.
    /// The queue keeps the error until the row expires and does not retry.
    Permanent(String),
}

/// A way of sending mail: one asynchronous method that takes a message and answers
/// accepted, temporary failure or permanent failure. With the three transports of this
/// crate and the queue of `triplespace_notify::mail`, this implements 0100 §5 (A2); the
/// crate map edge is 0100 §11 (A2).
pub trait MailTransport: Send + Sync {
    /// The transport's name, as `mail.transport` names it.
    fn name(&self) -> &'static str;

    /// Sends one message.
    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Outcome>;
}

/// A transport that sends nothing: `mail.transport = none`. The queue is never handed a
/// message when the instance has no transport, so this exists for completeness and tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoTransport;

impl MailTransport for NoTransport {
    fn name(&self) -> &'static str {
        "none"
    }

    fn send<'a>(&'a self, _message: &'a Message) -> BoxFuture<'a, Outcome> {
        Box::pin(async { Outcome::Permanent("the instance sends no mail".into()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> Message {
        Message {
            from: Mailbox::named("Library Base", "noreply@librarybase.org"),
            to: "alice@example.org".into(),
            subject: "Your verification code".into(),
            body: "Your code is 123 456.\n".into(),
            message_id: "abc123@librarybase.org".into(),
            date: SystemTime::UNIX_EPOCH + std::time::Duration::from_hours(497_856),
            stream: Stream::Transactional,
        }
    }

    /// The canonical encoding carries every header 0100 §5 and 0100 §11 name and the body
    /// as UTF-8 text.
    #[test]
    fn rfc5322_has_the_headers() {
        let bytes = message().rfc5322().unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("From: \"Library Base\" <noreply@librarybase.org>\r\n"));
        assert!(text.contains("To: alice@example.org\r\n"));
        assert!(text.contains("Subject: Your verification code\r\n"));
        assert!(text.contains("Message-ID: <abc123@librarybase.org>\r\n"));
        assert!(text.contains("Date: Sun, 18 Oct 2026 "));
        assert!(text.contains("Auto-Submitted: auto-generated\r\n"));
        assert!(text.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert!(text.ends_with("\r\n\r\nYour code is 123 456.\r\n"));
    }

    /// A subject outside ASCII is encoded, and the body stays UTF-8.
    #[test]
    fn non_ascii_subject_is_encoded() {
        let mut m = message();
        m.subject = "Código de verificación".into();
        m.body = "Tu código es 123 456.\n".into();
        let text = String::from_utf8(m.rfc5322().unwrap()).unwrap();
        assert!(text.contains("Subject: =?utf-8?"));
        assert!(text.contains("Tu c"));
    }

    #[test]
    fn display_names_are_quoted_where_needed() {
        assert_eq!(
            Mailbox::named("Library Base", "a@b.org").display(),
            "Library Base <a@b.org>"
        );
        assert_eq!(
            Mailbox::named("Scatter, Inc.", "a@b.org").display(),
            "\"Scatter, Inc.\" <a@b.org>"
        );
        assert_eq!(Mailbox::new("a@b.org").display(), "a@b.org");
    }

    #[test]
    fn bad_addresses_are_refused() {
        let mut m = message();
        m.to = "not an address".into();
        assert!(matches!(m.rfc5322(), Err(MailError::Address(_))));
    }

    #[test]
    fn streams_round_trip() {
        for s in [Stream::Transactional, Stream::Notification] {
            assert_eq!(Stream::parse(s.as_str()), Some(s));
        }
        assert_eq!(Stream::parse("bulk"), None);
    }
}
