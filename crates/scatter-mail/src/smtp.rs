//! The `smtp` transport (0100 §5; 16 §5.7): submission to a relay through `lettre` over
//! rustls. `mail.smtp.url` names the relay: `smtps://user@host:port` for implicit TLS
//! (RFC 8314), `smtp://user@host:port` for STARTTLS, which is required unless the host is
//! a loopback address, where plaintext is allowed for a local relay. The password comes
//! from `mail.smtp.password_file`, read by the caller.
//!
//! A `4xx` reply, a connection failure or a timeout is a temporary failure; a `5xx` reply
//! is permanent.

use lettre::AsyncTransport as _;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, Tokio1Executor};

use crate::{BoxFuture, MailError, MailTransport, Message, Outcome};

/// What `mail.smtp.url` says: the host, port, user name and which TLS mode to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpConfig {
    /// The relay's host.
    pub host: String,
    /// Its port: 465 for `smtps://`, 587 for `smtp://`, unless the URL says.
    pub port: u16,
    /// The user name from the URL, if any.
    pub user: Option<String>,
    /// How the connection is secured.
    pub security: Security,
}

/// How the connection to the relay is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// `smtps://`: TLS from the first byte.
    Implicit,
    /// `smtp://`: STARTTLS, required.
    StartTls,
    /// `smtp://` to a loopback host: plaintext.
    Plaintext,
}

impl SmtpConfig {
    /// Parses `mail.smtp.url`.
    ///
    /// # Errors
    ///
    /// A URL that is not `smtp://` or `smtps://`, or has no host.
    pub fn parse(url: &str) -> Result<Self, MailError> {
        let u =
            url::Url::parse(url).map_err(|e| MailError::Config(format!("mail.smtp.url: {e}")))?;
        let host = u
            .host_str()
            .ok_or_else(|| MailError::Config("mail.smtp.url: no host".into()))?
            .to_string();
        let loopback = host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        let (security, default_port) = match u.scheme() {
            "smtps" => (Security::Implicit, 465),
            "smtp" if loopback => (Security::Plaintext, 25),
            "smtp" => (Security::StartTls, 587),
            other => {
                return Err(MailError::Config(format!(
                    "mail.smtp.url: `{other}://` is not smtp:// or smtps://"
                )));
            }
        };
        let user = (!u.username().is_empty()).then(|| percent_decode(u.username()));
        Ok(Self {
            host,
            port: u.port().unwrap_or(default_port),
            user,
            security,
        })
    }
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// Submits to an SMTP relay.
pub struct SmtpTransport {
    inner: AsyncSmtpTransport<Tokio1Executor>,
}

impl std::fmt::Debug for SmtpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SmtpTransport")
    }
}

impl SmtpTransport {
    /// A transport over a relay, with the password the caller read from
    /// `mail.smtp.password_file`.
    ///
    /// # Errors
    ///
    /// A URL that does not parse, or TLS that cannot be set up.
    pub fn new(url: &str, password: Option<&str>) -> Result<Self, MailError> {
        let config = SmtpConfig::parse(url)?;
        let tls = |host: &str| {
            TlsParameters::builder(host.to_string())
                .build_rustls()
                .map_err(|e| MailError::Config(format!("mail.smtp.url TLS: {e}")))
        };
        let mut builder = match config.security {
            Security::Implicit => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
                    .tls(Tls::Wrapper(tls(&config.host)?))
            }
            Security::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host)
                    .tls(Tls::Required(tls(&config.host)?))
            }
            Security::Plaintext => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.host).tls(Tls::None)
            }
        }
        .port(config.port)
        .timeout(Some(std::time::Duration::from_secs(30)));
        if let Some(user) = &config.user {
            builder = builder.credentials(Credentials::new(
                user.clone(),
                password.unwrap_or_default().to_string(),
            ));
        }
        Ok(Self {
            inner: builder.build(),
        })
    }
}

impl MailTransport for SmtpTransport {
    fn name(&self) -> &'static str {
        "smtp"
    }

    fn send<'a>(&'a self, message: &'a Message) -> BoxFuture<'a, Outcome> {
        Box::pin(async move {
            let m = match message.to_lettre() {
                Ok(m) => m,
                Err(e) => return Outcome::Permanent(e.to_string()),
            };
            match self.inner.send(m).await {
                Ok(_) => Outcome::Accepted,
                Err(e) if e.is_permanent() => Outcome::Permanent(e.to_string()),
                Err(e) => Outcome::Temporary(e.to_string()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `smtps://` is implicit TLS on 465, `smtp://` STARTTLS on 587, and `smtp://` to a
    /// loopback host plaintext on 25, unless the URL names a port.
    #[test]
    fn urls_choose_the_security() {
        let c = SmtpConfig::parse("smtps://postmaster%40librarybase.org@smtp.example.com").unwrap();
        assert_eq!(c.security, Security::Implicit);
        assert_eq!(c.port, 465);
        assert_eq!(c.user.as_deref(), Some("postmaster@librarybase.org"));
        let c = SmtpConfig::parse("smtp://relay.example.com:2525").unwrap();
        assert_eq!(c.security, Security::StartTls);
        assert_eq!(c.port, 2525);
        assert_eq!(c.user, None);
        let c = SmtpConfig::parse("smtp://127.0.0.1").unwrap();
        assert_eq!(c.security, Security::Plaintext);
        assert_eq!(c.port, 25);
        let c = SmtpConfig::parse("smtp://localhost:1025").unwrap();
        assert_eq!(c.security, Security::Plaintext);
        assert!(SmtpConfig::parse("http://relay").is_err());
        assert!(SmtpConfig::parse("smtp:///").is_err());
    }

    #[test]
    fn transports_build() {
        assert!(SmtpTransport::new("smtps://u@relay.example.com", Some("pw")).is_ok());
        assert!(SmtpTransport::new("smtp://localhost:1025", None).is_ok());
    }
}
