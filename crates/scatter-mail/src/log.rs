//! The `log` transport (0100 §5; 16 §5.7): the development sink. Each message is written
//! as an `.eml` file to a directory, named by the time it was written and its
//! `Message-ID`, and its recipient and subject are logged. Tests and the e2e suite read
//! codes from the files. A server refuses it under `server.mode = production`; that is
//! the caller's check, not this crate's.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{BoxFuture, MailError, MailTransport, Message, Outcome};

/// Writes each message as an `.eml` file.
#[derive(Debug)]
pub struct LogTransport {
    dir: PathBuf,
    sequence: AtomicU64,
}

impl LogTransport {
    /// A sink over a directory, created if it does not exist.
    ///
    /// # Errors
    ///
    /// If the directory cannot be created.
    pub fn new(dir: &Path) -> Result<Self, MailError> {
        std::fs::create_dir_all(dir)
            .map_err(|e| MailError::Config(format!("mail.log_dir {}: {e}", dir.display())))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            sequence: AtomicU64::new(0),
        })
    }

    /// The directory the files go in.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file a message is written to: `{seconds}-{n}-{message-id local part}.eml`,
    /// which sorts by time.
    fn path_for(&self, message: &Message) -> PathBuf {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let n = self.sequence.fetch_add(1, Ordering::Relaxed);
        let local = message
            .message_id
            .split('@')
            .next()
            .unwrap_or("message")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(40)
            .collect::<String>();
        self.dir.join(format!("{secs:012}-{n:04}-{local}.eml"))
    }
}

impl MailTransport for LogTransport {
    fn name(&self) -> &'static str {
        "log"
    }

    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Outcome> {
        Box::pin(async move {
            let bytes = match message.rfc5322() {
                Ok(b) => b,
                Err(e) => return Outcome::Permanent(e.to_string()),
            };
            let path = self.path_for(message);
            // A small synchronous write; no runtime is assumed here.
            match std::fs::write(&path, bytes) {
                Ok(()) => {
                    eprintln!(
                        "mail: to {} \"{}\" written to {}",
                        message.to,
                        message.subject,
                        path.display()
                    );
                    Outcome::Accepted
                }
                Err(e) => Outcome::Temporary(format!("write {}: {e}", path.display())),
            }
        })
    }
}

/// A quoted-printable body decoded (RFC 2045 §6.7): soft line breaks removed and `=XX`
/// read as the byte, so that a test reads the text as the recipient does.
#[must_use]
pub fn decode_quoted_printable(body: &str) -> String {
    let joined = body.replace("=\r\n", "").replace("=\n", "");
    let mut out = Vec::with_capacity(joined.len());
    let bytes = joined.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'='
            && let (Some(h), Some(l)) = (
                bytes.get(i + 1).and_then(|b| (*b as char).to_digit(16)),
                bytes.get(i + 2).and_then(|b| (*b as char).to_digit(16)),
            )
        {
            out.push(u8::try_from(h * 16 + l).unwrap_or(b'?'));
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The text of a message as its recipient reads it: the headers as written, and the body
/// decoded where it was quoted-printable.
#[must_use]
pub fn readable(text: &str) -> String {
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return text.to_string();
    };
    let qp = head
        .lines()
        .any(|l| l.eq_ignore_ascii_case("content-transfer-encoding: quoted-printable"));
    if qp {
        format!("{head}\r\n\r\n{}", decode_quoted_printable(body))
    } else {
        text.to_string()
    }
}

/// Reads every `.eml` file in a directory, oldest first, as `(recipient, text)`, the body
/// decoded ([`readable`]): what a test does to find the code the instance sent.
///
/// # Errors
///
/// If the directory cannot be read.
pub fn read_all(dir: &Path) -> std::io::Result<Vec<(String, String)>> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "eml"))
        .collect();
    names.sort();
    let mut out = Vec::new();
    for p in names {
        let text = std::fs::read_to_string(&p)?;
        let to = text
            .lines()
            .find_map(|l| l.strip_prefix("To: "))
            .unwrap_or_default()
            .trim()
            .to_string();
        out.push((to, readable(&text)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Mailbox, Stream};

    /// The sink writes one file per message, readable back with its recipient.
    #[test]
    fn writes_eml_files() {
        let dir = std::env::temp_dir().join(format!("scatter-mail-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let t = LogTransport::new(&dir).unwrap();
        let m = Message {
            from: Mailbox::new("noreply@example.org"),
            to: "bob@example.org".into(),
            subject: "Hello".into(),
            body: "A body.\n".into(),
            message_id: "id-1@example.org".into(),
            date: SystemTime::UNIX_EPOCH,
            stream: Stream::Transactional,
        };
        let outcome = pollster::block_on(t.send(&m));
        assert_eq!(outcome, Outcome::Accepted);
        let all = read_all(&dir).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, "bob@example.org");
        assert!(all[0].1.contains("A body."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A long line is quoted-printable on the wire and whole again when read back.
    #[test]
    fn quoted_printable_is_decoded() {
        assert_eq!(
            decode_quoted_printable("Library B=\r\nase at 04:59 =C3=A9=\r\n."),
            "Library Base at 04:59 é."
        );
        let wire = "Subject: x\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nab=\r\ncd";
        assert_eq!(
            readable(wire),
            "Subject: x\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nabcd"
        );
        assert_eq!(
            readable("Subject: x\r\n\r\nab=\r\ncd"),
            "Subject: x\r\n\r\nab=\r\ncd"
        );
    }
}
