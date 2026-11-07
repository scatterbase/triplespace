//! The `postmark` transport (0100 §5, A2; 16 §5.7): one JSON `POST /email` per message
//! with the server token, sending through the egress path of 0012 §6. What an HTTP API adds
//! over SMTP is structured results and bounce and complaint webhooks; the webhook's payloads
//! are read by [`Event`], and what a bounce does to an address is the caller's business.
//!
//! **Results.** `200` is accepted. `422` with an API error code is a permanent failure
//! (`300` send validation, `406` inactive recipient, `1235` no such stream, …), as are
//! `401` (a wrong token), `413` and `415`: nothing about retrying would change them. `429`,
//! `500`, `503` and a connection failure are temporary.
//!
//! **Streams.** A transactional message goes on `mail.postmark.stream`, `outbound` by
//! default, Postmark's default transactional stream; a notification or digest on
//! `mail.postmark.notification_stream`, a broadcast stream the operator creates.

use serde::Deserialize;
use serde_json::json;

use crate::{BoxFuture, MailError, MailTransport, Message, Outcome, Stream};

/// Postmark's API.
pub const ENDPOINT: &str = "https://api.postmarkapp.com";

/// Sends through Postmark's email API.
#[derive(Debug)]
pub struct PostmarkTransport {
    client: reqwest::Client,
    endpoint: String,
    token: String,
    transactional_stream: String,
    notification_stream: String,
}

/// What Postmark answers a send with.
#[derive(Debug, Deserialize)]
struct Reply {
    #[serde(rename = "ErrorCode", default)]
    error_code: i64,
    #[serde(rename = "Message", default)]
    message: String,
}

impl PostmarkTransport {
    /// A transport with the server token the caller read from `mail.postmark.token_file`,
    /// and the two stream IDs.
    ///
    /// # Errors
    ///
    /// If the HTTP client cannot be built.
    pub fn new(
        token: &str,
        transactional_stream: &str,
        notification_stream: &str,
    ) -> Result<Self, MailError> {
        Self::at(ENDPOINT, token, transactional_stream, notification_stream)
    }

    /// The same, against another endpoint: a test's fake.
    ///
    /// # Errors
    ///
    /// If the HTTP client cannot be built.
    pub fn at(
        endpoint: &str,
        token: &str,
        transactional_stream: &str,
        notification_stream: &str,
    ) -> Result<Self, MailError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| MailError::Config(format!("postmark client: {e}")))?;
        Ok(Self {
            client,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            token: token.trim().to_string(),
            transactional_stream: transactional_stream.to_string(),
            notification_stream: notification_stream.to_string(),
        })
    }

    /// The stream a message goes on.
    fn stream_of(&self, m: &Message) -> &str {
        match m.stream {
            Stream::Transactional => &self.transactional_stream,
            Stream::Notification => &self.notification_stream,
        }
    }

    /// The request body: the same fields as the RFC 5322 encoding, as JSON.
    #[must_use]
    pub fn body_of(&self, m: &Message) -> serde_json::Value {
        json!({
            "From": m.from.display(),
            "To": m.to,
            "Subject": m.subject,
            "TextBody": m.body,
            "MessageStream": self.stream_of(m),
            "Headers": [
                {"Name": "Message-ID", "Value": format!("<{}>", m.message_id)},
                {"Name": "Auto-Submitted", "Value": "auto-generated"},
            ],
        })
    }
}

/// How a send's HTTP status and API error code are read (the module documentation).
#[must_use]
pub fn classify(status: u16, error_code: i64, message: &str) -> Outcome {
    let describe = || format!("postmark {status} (error {error_code}): {message}");
    match status {
        200..=299 if error_code == 0 => Outcome::Accepted,
        429 | 500 | 502 | 503 | 504 => Outcome::Temporary(describe()),
        _ => Outcome::Permanent(describe()),
    }
}

impl MailTransport for PostmarkTransport {
    fn name(&self) -> &'static str {
        "postmark"
    }

    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Outcome> {
        Box::pin(async move {
            let r = self
                .client
                .post(format!("{}/email", self.endpoint))
                .header("Accept", "application/json")
                .header("X-Postmark-Server-Token", &self.token)
                .json(&self.body_of(message))
                .send()
                .await;
            let r = match r {
                Ok(r) => r,
                Err(e) => return Outcome::Temporary(format!("postmark: {e}")),
            };
            let status = r.status().as_u16();
            let reply: Reply = r.json().await.unwrap_or(Reply {
                error_code: 0,
                message: String::new(),
            });
            classify(status, reply.error_code, &reply.message)
        })
    }
}

/// A bounce or spam-complaint webhook event (0100 A2): the fields of Postmark's `Bounce`
/// and `SpamComplaint` payloads that decide what happens to the address.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Event {
    /// `Bounce` or `SpamComplaint`.
    #[serde(rename = "RecordType")]
    pub record_type: String,
    /// `HardBounce`, `Transient`, `SoftBounce`, `SpamComplaint`, …
    #[serde(rename = "Type", default)]
    pub kind: String,
    /// The type's code: 1 for a hard bounce, 4096 for a soft one, 100001 for a complaint.
    #[serde(rename = "TypeCode", default)]
    pub type_code: i64,
    /// The address concerned.
    #[serde(rename = "Email")]
    pub email: String,
    /// Whether Postmark has deactivated the address.
    #[serde(rename = "Inactive", default)]
    pub inactive: bool,
    /// The message's `MessageID` at Postmark.
    #[serde(rename = "MessageID", default)]
    pub message_id: String,
    /// The stream it was sent on.
    #[serde(rename = "MessageStream", default)]
    pub message_stream: String,
    /// Postmark's description of the event.
    #[serde(rename = "Description", default)]
    pub description: String,
}

/// What an event means for the address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The address took a hard bounce: it does not exist or refuses mail for good.
    Bounced,
    /// The recipient marked the message as spam.
    Complained,
    /// A transient or soft bounce, an autoresponder, or anything else: nothing to record.
    Nothing,
}

impl Event {
    /// Reads the event: a complaint by record type or type code; a hard bounce by type
    /// (`HardBounce`, `BadEmailAddress`, or any bounce Postmark deactivated the address
    /// for); everything else nothing.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        if self.record_type == "SpamComplaint" || self.kind == "SpamComplaint" {
            return Verdict::Complained;
        }
        if self.record_type != "Bounce" {
            return Verdict::Nothing;
        }
        match self.kind.as_str() {
            "HardBounce" | "BadEmailAddress" => Verdict::Bounced,
            "Transient"
            | "SoftBounce"
            | "DnsError"
            | "AutoResponder"
            | "Unsubscribe"
            | "Subscribe"
            | "AddressChange"
            | "OpenRelayTest"
            | "ChallengeVerification" => Verdict::Nothing,
            _ if self.inactive => Verdict::Bounced,
            _ => Verdict::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mailbox;

    fn message(stream: Stream) -> Message {
        Message {
            from: Mailbox::named("Library Base", "noreply@librarybase.org"),
            to: "alice@example.org".into(),
            subject: "Hi".into(),
            body: "Body\n".into(),
            message_id: "x1@librarybase.org".into(),
            date: std::time::SystemTime::UNIX_EPOCH,
            stream,
        }
    }

    /// The request carries the message's fields and the stream its kind chooses.
    #[test]
    fn body_has_the_fields_and_stream() {
        let t = PostmarkTransport::new("tok", "outbound", "digests").unwrap();
        let b = t.body_of(&message(Stream::Transactional));
        assert_eq!(b["From"], "Library Base <noreply@librarybase.org>");
        assert_eq!(b["To"], "alice@example.org");
        assert_eq!(b["TextBody"], "Body\n");
        assert_eq!(b["MessageStream"], "outbound");
        assert_eq!(b["Headers"][0]["Value"], "<x1@librarybase.org>");
        assert_eq!(b["Headers"][1]["Name"], "Auto-Submitted");
        let b = t.body_of(&message(Stream::Notification));
        assert_eq!(b["MessageStream"], "digests");
    }

    /// Accepted on 200; temporary on 429 and 5xx; permanent on 422's error codes and on
    /// a wrong token.
    #[test]
    fn results_are_classified() {
        assert_eq!(classify(200, 0, "OK"), Outcome::Accepted);
        assert!(matches!(classify(429, 0, ""), Outcome::Temporary(_)));
        assert!(matches!(
            classify(503, 100, "offline"),
            Outcome::Temporary(_)
        ));
        assert!(matches!(classify(500, 101, ""), Outcome::Temporary(_)));
        assert!(matches!(
            classify(422, 406, "Inactive recipient"),
            Outcome::Permanent(_)
        ));
        assert!(matches!(
            classify(422, 300, "Invalid 'To'"),
            Outcome::Permanent(_)
        ));
        assert!(matches!(
            classify(401, 10, "bad token"),
            Outcome::Permanent(_)
        ));
        assert!(matches!(classify(200, 300, "odd"), Outcome::Permanent(_)));
    }

    /// Hard bounces and complaints are recorded; transient and soft bounces are not.
    #[test]
    fn events_have_verdicts() {
        let e = |record: &str, kind: &str, inactive: bool| {
            serde_json::from_value::<Event>(json!({
                "RecordType": record, "Type": kind, "TypeCode": 1,
                "Email": "alice@example.org", "Inactive": inactive,
                "MessageID": "m", "MessageStream": "outbound", "Description": "d"
            }))
            .unwrap()
            .verdict()
        };
        assert_eq!(e("Bounce", "HardBounce", true), Verdict::Bounced);
        assert_eq!(e("Bounce", "BadEmailAddress", false), Verdict::Bounced);
        assert_eq!(e("Bounce", "Transient", false), Verdict::Nothing);
        assert_eq!(e("Bounce", "SoftBounce", false), Verdict::Nothing);
        assert_eq!(e("Bounce", "Blocked", true), Verdict::Bounced);
        assert_eq!(e("Bounce", "Blocked", false), Verdict::Nothing);
        assert_eq!(
            e("SpamComplaint", "SpamComplaint", true),
            Verdict::Complained
        );
        assert_eq!(e("Delivery", "", false), Verdict::Nothing);
    }
}
