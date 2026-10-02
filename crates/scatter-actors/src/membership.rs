//! Memberships and blocks (0016 §3; 0024 §3; 0028 §3–4; 0040 §5): the
//! `scatter:v0/membership` and `scatter:v0/block` payloads, keyed by the actor, and
//! their state as of a time.
//!
//! Both are actor records with full history, so "the groups an actor was in when a record
//! was appended" is a fold over the records up to that moment ([`memberships_as_of`]),
//! which is what the autopatrol rule needs (0023 §6) and what a rebuild reproduces.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

/// A membership change: the content part of a `scatter:v0/membership` record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Membership {
    /// The group.
    pub group: String,
    /// Added or removed.
    pub action: MembershipAction,
    /// For an addition, when it lapses; absent means until removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<Timestamp>,
}

/// Add or remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MembershipAction {
    /// The actor joins the group.
    Add,
    /// The actor leaves the group.
    Remove,
}

impl Membership {
    /// An addition.
    #[must_use]
    pub fn add(group: &str, expires: Option<Timestamp>) -> Self {
        Self {
            group: group.to_string(),
            action: MembershipAction::Add,
            expires,
        }
    }

    /// A removal.
    #[must_use]
    pub fn remove(group: &str) -> Self {
        Self {
            group: group.to_string(),
            action: MembershipAction::Remove,
            expires: None,
        }
    }
}

/// A record with the time it was appended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appended<T> {
    /// `appended_at` from the record header.
    pub at: Timestamp,
    /// The content.
    pub content: T,
}

/// The explicit groups an actor is in at `at`, from its membership records in log order:
/// an addition holds until removed or until its expiry; a later record for the same
/// group replaces an earlier one. Records appended after `at` are ignored.
#[must_use]
pub fn memberships_as_of(records: &[Appended<Membership>], at: Timestamp) -> BTreeSet<String> {
    let mut groups: BTreeSet<String> = BTreeSet::new();
    for r in records.iter().filter(|r| r.at <= at) {
        match r.content.action {
            MembershipAction::Add => {
                if r.content.expires.is_none_or(|e| at < e) {
                    groups.insert(r.content.group.clone());
                } else {
                    groups.remove(&r.content.group);
                }
            }
            MembershipAction::Remove => {
                groups.remove(&r.content.group);
            }
        }
    }
    groups
}

/// The content part of a `scatter:v0/block` record (0016 §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
pub enum Block {
    /// A block: the permissions removed, until an expiry.
    Block {
        /// What is removed.
        removes: BlockScope,
        /// When the block lapses; absent means indefinite.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires: Option<Timestamp>,
    },
    /// An unblock: the actor's block in this layer is lifted.
    Unblock,
}

/// What a block removes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BlockScope {
    /// Everything but `read`: a full block.
    #[serde(with = "all_but_read")]
    AllButRead,
    /// The named permissions; by default `edit` and `createpage`.
    Permissions(BTreeSet<String>),
}

mod all_but_read {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    const MARKER: &str = "all-but-read";

    pub fn serialize<S: Serializer>(s: S) -> Result<S::Ok, S::Error> {
        MARKER.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<(), D::Error> {
        let s = String::deserialize(d)?;
        if s == MARKER {
            Ok(())
        } else {
            Err(serde::de::Error::custom(format!(
                "expected \"{MARKER}\", got \"{s}\""
            )))
        }
    }
}

impl Block {
    /// MediaWiki's default block: `edit` and `createpage`.
    #[must_use]
    pub fn default_block(expires: Option<Timestamp>) -> Self {
        Self::Block {
            removes: BlockScope::Permissions(["edit".to_string(), "createpage".to_string()].into()),
            expires,
        }
    }

    /// A block of everything but `read`.
    #[must_use]
    pub fn full(expires: Option<Timestamp>) -> Self {
        Self::Block {
            removes: BlockScope::AllButRead,
            expires,
        }
    }
}

/// What an actor's blocks remove at `at`, in one layer: the latest block record wins,
/// an unblock lifts it, and an expired block removes nothing. `all` is the set of every
/// permission, for a full block.
#[must_use]
pub fn blocked_as_of(
    records: &[Appended<Block>],
    at: Timestamp,
    all: &BTreeSet<String>,
) -> BTreeSet<String> {
    let current = records.iter().rfind(|r| r.at <= at);
    match current.map(|r| &r.content) {
        Some(Block::Block { removes, expires }) if expires.is_none_or(|e| at < e) => {
            match removes {
                BlockScope::AllButRead => all.iter().filter(|p| *p != "read").cloned().collect(),
                BlockScope::Permissions(ps) => ps.clone(),
            }
        }
        _ => BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at<T>(t: u64, content: T) -> Appended<T> {
        Appended {
            at: Timestamp(t),
            content,
        }
    }

    #[test]
    fn memberships_fold_in_time() {
        let records = vec![
            at(10, Membership::add("sysop", None)),
            at(20, Membership::add("bot", Some(Timestamp(100)))),
            at(30, Membership::remove("sysop")),
            at(40, Membership::add("sysop", None)),
        ];
        let g = |t| memberships_as_of(&records, Timestamp(t));
        assert!(g(5).is_empty());
        assert_eq!(g(15), ["sysop".to_string()].into());
        assert_eq!(g(25), ["bot".to_string(), "sysop".to_string()].into());
        assert_eq!(g(35), ["bot".to_string()].into());
        assert_eq!(g(45), ["bot".to_string(), "sysop".to_string()].into());
        assert_eq!(g(100), ["sysop".to_string()].into(), "bot expired");
        assert_eq!(
            serde_json::to_string(&records[1].content).unwrap(),
            r#"{"group":"bot","action":"add","expires":100}"#
        );
    }

    #[test]
    fn blocks_fold_in_time() {
        let all: BTreeSet<String> = ["read", "edit", "createpage", "delete"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let records = vec![
            at(10, Block::default_block(Some(Timestamp(50)))),
            at(60, Block::full(None)),
            at(70, Block::Unblock),
        ];
        let b = |t| blocked_as_of(&records, Timestamp(t), &all);
        assert!(b(5).is_empty());
        assert_eq!(b(20), ["edit".to_string(), "createpage".to_string()].into());
        assert!(b(50).is_empty(), "expired");
        assert_eq!(
            b(65),
            [
                "edit".to_string(),
                "createpage".to_string(),
                "delete".to_string()
            ]
            .into()
        );
        assert!(b(75).is_empty(), "unblocked");
        assert_eq!(
            serde_json::to_string(&records[1].content).unwrap(),
            r#"{"action":"block","removes":"all-but-read"}"#
        );
        assert_eq!(
            serde_json::to_string(&records[0].content).unwrap(),
            r#"{"action":"block","removes":["createpage","edit"],"expires":50}"#
        );
        let back: Block =
            serde_json::from_str(r#"{"action":"block","removes":"all-but-read"}"#).unwrap();
        assert_eq!(back, Block::full(None));
        assert_eq!(
            serde_json::from_str::<Block>(r#"{"action":"unblock"}"#).unwrap(),
            Block::Unblock
        );
        assert!(
            serde_json::from_str::<Block>(r#"{"action":"block","removes":"everything"}"#).is_err()
        );
    }
}
