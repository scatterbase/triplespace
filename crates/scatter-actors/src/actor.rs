//! Actor records (0007 §4–6; 0024 §1; 0025 §3): the `scatter:v0/actor` payload.
//!
//! Change sets and revisions carry actor keys, never names. Each actor's name, kind and
//! status live in actor records, keyed by the actor key, so that every name and every
//! raw value a surrogate stands for is in one place per actor and can be erased in one
//! place. A rename or a status change appends a new record; the latest per key is current.

use serde::{Deserialize, Serialize};

use crate::key::ActorKey;

/// What kind of actor this is (0007 §4–5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorKind {
    /// A registered account of its issuer, a person's.
    Registered,
    /// A temporary account (0007 §3, §5).
    Temporary,
    /// A subsidiary account, always with an operator (0024 §1).
    Bot,
    /// A surrogate for an upstream IP edit; the record holds the IP.
    Anonymous,
    /// A surrogate for an imported edit; the record holds the prefix and name.
    Imported,
    /// A provider as a whole, or the instance operator (0007 §6; 0040 §2).
    Provider,
    /// A surrogate for a fediverse actor; the record holds the remote IRI (0022 §8).
    Federated,
}

impl ActorKind {
    /// Whether the actor is a surrogate minted by the instance (0007 §5), whose record
    /// carries the raw value it stands for.
    #[must_use]
    pub const fn is_surrogate(self) -> bool {
        matches!(self, Self::Anonymous | Self::Imported | Self::Federated)
    }

    /// The `kind` as the `view.actor` column spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Temporary => "temporary",
            Self::Bot => "bot",
            Self::Anonymous => "anonymous",
            Self::Imported => "imported",
            Self::Provider => "provider",
            Self::Federated => "federated",
        }
    }
}

/// An actor's status (0007 §4; 0024 §2; 0025 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorStatus {
    /// In use.
    #[default]
    Active,
    /// Renamed; this record carries the new name.
    Renamed,
    /// Vanished: the name is a placeholder and the history was erased.
    Vanished,
    /// The name is hidden by an `actor` ACL restricted to the suppression group (0023 §5).
    /// Set by the projection from the ACL, not written by hand.
    Hidden,
    /// A subsidiary retired by its operator or a bureaucrat (0024 §2).
    Retired,
    /// A subsidiary created by an OAuth authorization and not yet approved (0025 §3).
    Pending,
}

impl ActorStatus {
    /// The `status` as the `view.actor` column spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Renamed => "renamed",
            Self::Vanished => "vanished",
            Self::Hidden => "hidden",
            Self::Retired => "retired",
            Self::Pending => "pending",
        }
    }
}

/// The content part of a `scatter:v0/actor` record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorRecord {
    /// The kind.
    pub kind: ActorKind,
    /// The current name, if the actor has one. A surrogate has none; an `imported`
    /// surrogate's prefix and name are in `raw`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The status.
    #[serde(default)]
    pub status: ActorStatus,
    /// For a surrogate only: the raw value it stands for, an IP address, an
    /// `imported>Name` string, or a remote actor IRI. Erasable with the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    /// For a `bot`: the actor key of the primary account that owns it (0024 §1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<ActorKey>,
}

/// Why an actor record is not well formed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ActorRecordError {
    /// A `bot` has no operator, or something other than a `bot` has one.
    #[error("a {0} actor {1} an operator")]
    Operator(&'static str, &'static str),
    /// The operator is the subsidiary itself.
    #[error("a subsidiary cannot be its own operator")]
    SelfOperated,
    /// The operator is not an account of the same tenant (0024 §1).
    #[error("the operator `{0}` is not a local account")]
    ForeignOperator(ActorKey),
    /// A surrogate has a name, or a non-surrogate has a raw value.
    #[error("a {0} actor {1} a raw value")]
    Raw(&'static str, &'static str),
    /// `retired` or `pending` on something other than a subsidiary.
    #[error("only a subsidiary can be {0}")]
    SubsidiaryStatus(&'static str),
    /// The name is not a valid account name.
    #[error("`{0}` is not a valid account name: {1}")]
    Name(String, &'static str),
}

impl ActorRecord {
    /// A registered account with a name.
    #[must_use]
    pub fn registered(name: &str) -> Self {
        Self {
            kind: ActorKind::Registered,
            name: Some(name.to_string()),
            status: ActorStatus::Active,
            raw: None,
            operator: None,
        }
    }

    /// A subsidiary of `operator` (0024 §1).
    #[must_use]
    pub fn subsidiary(name: &str, operator: ActorKey) -> Self {
        Self {
            kind: ActorKind::Bot,
            name: Some(name.to_string()),
            status: ActorStatus::Active,
            raw: None,
            operator: Some(operator),
        }
    }

    /// A surrogate of the given kind standing for `raw` (0007 §5).
    #[must_use]
    pub fn surrogate(kind: ActorKind, raw: &str) -> Self {
        Self {
            kind,
            name: None,
            status: ActorStatus::Active,
            raw: Some(raw.to_string()),
            operator: None,
        }
    }

    /// Checks the invariants of 0007 §4–5 and 0024 §1, for a record that will be keyed by
    /// `key`.
    pub fn validate(&self, key: &ActorKey) -> Result<(), ActorRecordError> {
        let is_bot = self.kind == ActorKind::Bot;
        match (&self.operator, is_bot) {
            (None, true) => return Err(ActorRecordError::Operator("bot", "needs")),
            (Some(_), false) => {
                return Err(ActorRecordError::Operator(self.kind.name(), "cannot have"));
            }
            (Some(op), true) => {
                if op == key {
                    return Err(ActorRecordError::SelfOperated);
                }
                if op.issuer() != key.issuer() {
                    return Err(ActorRecordError::ForeignOperator(op.clone()));
                }
            }
            (None, false) => {}
        }
        match (self.kind.is_surrogate(), &self.raw) {
            (true, None) => return Err(ActorRecordError::Raw(self.kind.name(), "needs")),
            (false, Some(_)) => {
                return Err(ActorRecordError::Raw(self.kind.name(), "cannot have"));
            }
            _ => {}
        }
        if self.kind.is_surrogate() && self.name.is_some() {
            return Err(ActorRecordError::Name(
                self.name.clone().unwrap_or_default(),
                "a surrogate has no name; its raw value is in `raw`",
            ));
        }
        if matches!(self.status, ActorStatus::Retired | ActorStatus::Pending) && !is_bot {
            return Err(ActorRecordError::SubsidiaryStatus(self.status.name()));
        }
        if let Some(name) = &self.name
            && let Err(reason) = check_name(name)
        {
            return Err(ActorRecordError::Name(name.clone(), reason));
        }
        Ok(())
    }

    /// Whether the actor is a subsidiary (0024 §1).
    #[must_use]
    pub fn is_subsidiary(&self) -> bool {
        self.kind == ActorKind::Bot
    }

    /// Whether the implicit `user` membership is withheld (0025 §3): a pending subsidiary
    /// acts as `universe` until approved.
    #[must_use]
    pub fn withholds_user(&self) -> bool {
        self.status == ActorStatus::Pending
    }
}

/// MediaWiki's account-name rules, as far as they are the same on every wiki: not empty,
/// at most 255 bytes, no control characters, no leading, trailing or doubled spaces, and
/// none of the characters MediaWiki forbids in titles or usernames (`@` is the bot-password
/// separator of 0024 §4, `:` the actor-key separator, `/` the subpage separator).
pub fn check_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("empty");
    }
    if name.len() > 255 {
        return Err("longer than 255 bytes");
    }
    if name.chars().any(char::is_control) {
        return Err("contains a control character");
    }
    if name.starts_with(' ') || name.ends_with(' ') || name.contains("  ") {
        return Err("leading, trailing or doubled spaces");
    }
    if name.contains(['@', ':', '/', '#', '<', '>', '[', ']', '|', '{', '}', '_']) {
        return Err("contains a character MediaWiki forbids in a username");
    }
    if name
        .chars()
        .next()
        .is_some_and(|c| c.is_lowercase() && c.to_uppercase().ne(std::iter::once(c)))
    {
        return Err("the first letter is not capitalized");
    }
    Ok(())
}

/// Normalizes a name as MediaWiki does before checking it: underscores to spaces, runs of
/// spaces collapsed, trimmed, first letter capitalized.
#[must_use]
pub fn normalize_name(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_space = true;
    for c in input.chars() {
        let c = if c == '_' { ' ' } else { c };
        if c == ' ' {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    let trimmed = out.trim_end();
    let mut chars = trimmed.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_and_validate() {
        let r = ActorRecord::registered("Example");
        assert!(r.validate(&ActorKey::local(42)).is_ok());
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"kind":"registered","name":"Example","status":"active"}"#
        );
        let bot = ActorRecord::subsidiary("Example-bot", ActorKey::local(42));
        assert!(bot.validate(&ActorKey::local(43)).is_ok());
        assert!(bot.is_subsidiary());
        assert_eq!(
            serde_json::from_str::<ActorRecord>(
                r#"{"kind":"bot","name":"Example-bot","operator":"local:42"}"#
            )
            .unwrap(),
            bot
        );
        let s = ActorRecord::surrogate(ActorKind::Anonymous, "192.0.2.1");
        assert!(s.validate(&ActorKey::local(7)).is_ok());
        let mut pending = bot.clone();
        pending.status = ActorStatus::Pending;
        assert!(pending.withholds_user());
    }

    #[test]
    fn invariants() {
        let key = ActorKey::local(43);
        let mut bot_without_operator = ActorRecord::subsidiary("B", ActorKey::local(42));
        bot_without_operator.operator = None;
        assert!(matches!(
            bot_without_operator.validate(&key),
            Err(ActorRecordError::Operator("bot", "needs"))
        ));
        assert!(matches!(
            ActorRecord::subsidiary("B", key.clone()).validate(&key),
            Err(ActorRecordError::SelfOperated)
        ));
        assert!(matches!(
            ActorRecord::subsidiary("B", ActorKey::parse("wikidatawiki:1").unwrap()).validate(&key),
            Err(ActorRecordError::ForeignOperator(_))
        ));
        let mut person_with_operator = ActorRecord::registered("P");
        person_with_operator.operator = Some(ActorKey::local(1));
        assert!(matches!(
            person_with_operator.validate(&key),
            Err(ActorRecordError::Operator("registered", "cannot have"))
        ));
        let mut named_surrogate =
            ActorRecord::surrogate(ActorKind::Federated, "https://m.example/u/x");
        named_surrogate.name = Some("X".into());
        assert!(matches!(
            named_surrogate.validate(&key),
            Err(ActorRecordError::Name(..))
        ));
        let mut raw_person = ActorRecord::registered("P");
        raw_person.raw = Some("x".into());
        assert!(matches!(
            raw_person.validate(&key),
            Err(ActorRecordError::Raw(..))
        ));
        let mut pending_person = ActorRecord::registered("P");
        pending_person.status = ActorStatus::Pending;
        assert!(matches!(
            pending_person.validate(&key),
            Err(ActorRecordError::SubsidiaryStatus("pending"))
        ));
        assert!(matches!(
            ActorRecord::registered("bad@name").validate(&key),
            Err(ActorRecordError::Name(..))
        ));
    }

    #[test]
    fn names() {
        assert_eq!(normalize_name("  example_user  name "), "Example user name");
        assert_eq!(normalize_name("élan"), "Élan");
        assert!(check_name("Example").is_ok());
        assert!(check_name("Example-bot").is_ok());
        assert!(check_name("Ünïcode Name").is_ok());
        assert!(check_name("中文").is_ok(), "no case: fine");
        for bad in [
            "", "example", " X", "X ", "A  B", "A@B", "A:B", "A/B", "A_B", "A\u{1}B", "A[B",
        ] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
        assert!(check_name(&"x".repeat(256)).is_err());
    }
}
