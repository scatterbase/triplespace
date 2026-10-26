//! The viewer's notifications (16 §5; 19 §6.2): what the bell, the orange bar and
//! `Special:Notifications` read, through Echo's modules (`meta=notifications`,
//! `action=echomarkread`, `action=echomarkseen`).

use serde_json::Value;

use crate::api::{CacheInfo, Fetched, check};
use crate::settings::Saved;
use crate::{Client, ClientError, Incoming};

/// The unread counts and times the frame draws the bell and the orange bar from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    /// Unread alerts the viewer may see, up to 100.
    pub alerts: u64,
    /// Unread messages, which are `talk` notifications: the orange bar's.
    pub messages: u64,
    /// When the newest unread item happened, ISO 8601.
    pub newest_unread: Option<String>,
    /// When the viewer last opened the inbox, ISO 8601.
    pub seen: Option<String>,
}

impl Counts {
    /// Every unread item.
    #[must_use]
    pub fn unread(&self) -> u64 {
        self.alerts + self.messages
    }

    /// Whether something unread is newer than the viewer's last look.
    #[must_use]
    pub fn unseen(&self) -> bool {
        match (&self.newest_unread, &self.seen) {
            (Some(n), Some(s)) => n > s,
            (Some(_), None) => true,
            _ => false,
        }
    }
}

/// One notification, as the page lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    /// The inbox row's ID.
    pub id: i64,
    /// Why it was delivered (16 §5.2): `talk`, `mention`, `reply`, `thread-status`,
    /// `rights`, `group-join`, …
    pub reason: String,
    /// When, ISO 8601.
    pub timestamp: String,
    /// Who did it; `None` when hidden, or for an instance act.
    pub agent: Option<String>,
    /// The page it is about, prefixed: a thread, or a user page.
    pub title: Option<String>,
    /// For a thread: its subject.
    pub subject: Option<String>,
    /// For a thread: the talk page it is on, prefixed.
    pub talk: Option<String>,
    /// For a thread: the record's revision ID, which is the post's.
    pub revid: Option<u64>,
    /// Whether it has been read.
    pub read: bool,
    /// The activity's parameters, for the reasons that carry them (`rights`).
    pub params: Value,
}

impl Item {
    fn of(v: &Value) -> Self {
        Self {
            id: v["id"].as_i64().unwrap_or(0),
            reason: v["ts-reason"].as_str().unwrap_or_default().to_string(),
            timestamp: v["timestamp"]["utciso8601"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            agent: v["agent"]["name"].as_str().map(str::to_string),
            title: v["title"]["full"].as_str().map(str::to_string),
            subject: v["ts-thread"]["subject"].as_str().map(str::to_string),
            talk: v["ts-thread"]["talk"]["full"].as_str().map(str::to_string),
            revid: v["revid"].as_u64(),
            read: v.get("read").is_some_and(|r| !r.is_null()),
            params: v["ts-params"].clone(),
        }
    }
}

/// A page of notifications.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// Newest first.
    pub items: Vec<Item>,
    /// `notcontinue` for the next page.
    pub next: Option<String>,
}

fn q(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

impl Client {
    /// The bell's and the orange bar's counts: `meta=notifications` with `notprop=count|
    /// seenTime|list`, grouped by section, the newest unread item of each.
    ///
    /// # Errors
    ///
    /// The API away or unreadable, or the viewer not logged in.
    pub async fn notification_counts(
        &self,
        incoming: &Incoming,
    ) -> Result<Fetched<Counts>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&meta=notifications&notprop=count%7CseenTime%7Clist&notfilter=!read&notlimit=1&notgroupbysection=1&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        let n = &v["query"]["notifications"];
        let newest = ["alert", "message"]
            .iter()
            .filter_map(|s| n[*s]["list"][0]["timestamp"]["utciso8601"].as_str())
            .max()
            .map(str::to_string);
        let value = Counts {
            alerts: n["alert"]["rawcount"].as_u64().unwrap_or(0),
            messages: n["message"]["rawcount"].as_u64().unwrap_or(0),
            newest_unread: newest,
            seen: n["alert"]["seenTime"].as_str().map(str::to_string),
        };
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// One page of the inbox, newest first: everything, or the unread only.
    ///
    /// # Errors
    ///
    /// The API away or unreadable, or the viewer not logged in.
    pub async fn notifications(
        &self,
        incoming: &Incoming,
        unread_only: bool,
        limit: u32,
        from: Option<&str>,
    ) -> Result<Fetched<Listing>, ClientError> {
        let mut path = format!(
            "/w/api.php?action=query&meta=notifications&notprop=list&notlimit={limit}&format=json&formatversion=2"
        );
        if unread_only {
            path.push_str("&notfilter=!read");
        }
        if let Some(c) = from {
            path.push_str("&notcontinue=");
            path.push_str(&q(c));
        }
        let r = self.get(incoming, &path).await?;
        let v = check(&r)?;
        let n = &v["query"]["notifications"];
        let value = Listing {
            items: n["list"]
                .as_array()
                .map(|a| a.iter().map(Item::of).collect())
                .unwrap_or_default(),
            next: n["continue"].as_str().map(str::to_string),
        };
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `action=echomarkread`: these IDs read, or with `None` every one of a section
    /// (`alert` or `message`), or every one at all.
    ///
    /// # Errors
    ///
    /// The API away or unreadable; a refusal is [`Saved::Refused`].
    pub async fn mark_read(
        &self,
        incoming: &Incoming,
        ids: Option<&[i64]>,
        section: Option<&str>,
        token: &str,
    ) -> Result<Saved<Value>, ClientError> {
        let list = ids
            .unwrap_or_default()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("|");
        let mut fields: Vec<(&str, &str)> = vec![("action", "echomarkread"), ("token", token)];
        if ids.is_some() {
            fields.push(("list", &list));
        } else {
            fields.push(("all", "1"));
            if let Some(s) = section {
                fields.push(("sections", s));
            }
        }
        self.action(incoming, &fields).await
    }

    /// `action=echomarkseen&type=all`: the viewer has looked at the inbox now.
    ///
    /// # Errors
    ///
    /// The API away or unreadable; a refusal is [`Saved::Refused`].
    pub async fn mark_seen(
        &self,
        incoming: &Incoming,
        token: &str,
    ) -> Result<Saved<Value>, ClientError> {
        self.action(
            incoming,
            &[
                ("action", "echomarkseen"),
                ("type", "all"),
                ("token", token),
            ],
        )
        .await
    }
}
