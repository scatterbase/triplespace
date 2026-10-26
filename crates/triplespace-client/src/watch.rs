//! The viewer's watch set (0020 §3, §5; 16 §4.3; 19 §6.2): what the star and its
//! **Watch for…** menu read and write, and what `Special:EditWatchlist` lists. Writes are
//! the Action API's `action=watch` and `action=setnotificationtimestamp`, with the viewer's
//! own token; the list is `GET /watchlist`.

use serde::Deserialize;
use serde_json::Value;

use crate::api::{CacheInfo, Fetched, check};
use crate::settings::Saved;
use crate::{Client, ClientError, Incoming};

const REST: &str = "/w/rest.php/triplespace/v0";

/// The viewer's watch of one title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchState {
    /// Whether the viewer watches it.
    pub watched: bool,
    /// When the watch lapses, ISO 8601; `None` for a permanent one.
    pub expires: Option<String>,
    /// Whether its changes are notifications too.
    pub notify: bool,
    /// When the first change the viewer has not seen happened, ISO 8601.
    pub unseen: Option<String>,
}

/// One watch, as `GET /watchlist` lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct WatchEntry {
    /// `entity`, `page`, `actor` or `title`.
    pub kind: String,
    /// The target.
    pub id: String,
    /// The namespace, where the title is known.
    #[serde(default)]
    pub ns: Option<i32>,
    /// The title, prefixed; `None` when the viewer may not see one now.
    #[serde(default)]
    pub title: Option<String>,
    /// When it lapses.
    #[serde(default)]
    pub expires: Option<String>,
    /// Notify me.
    #[serde(default)]
    pub notify: bool,
    /// The first change not seen.
    #[serde(default)]
    pub unseen: Option<String>,
}

/// What `action=watch` is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchForm<'a> {
    /// The titles.
    pub titles: &'a [String],
    /// Stop watching instead.
    pub unwatch: bool,
    /// `infinite`, or a span such as `1 week`.
    pub expiry: Option<&'a str>,
    /// Notify me, when given.
    pub notify: Option<bool>,
    /// A `watch` token.
    pub token: &'a str,
}

fn q(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

impl Client {
    /// `prop=info&inprop=watched|notificationtimestamp` of one title, as the viewer watches it.
    ///
    /// # Errors
    ///
    /// The API away or unreadable.
    pub async fn watch_state(
        &self,
        incoming: &Incoming,
        title: &str,
    ) -> Result<Fetched<WatchState>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&prop=info&inprop=watched%7Cnotificationtimestamp&titles={}&format=json&formatversion=2",
                    q(title)
                ),
            )
            .await?;
        let v = check(&r)?;
        let p = &v["query"]["pages"][0];
        let value = WatchState {
            watched: p["watched"].as_bool().unwrap_or(false),
            expires: p["watchlistexpiry"].as_str().map(str::to_string),
            notify: p["ts-notify"].as_bool().unwrap_or(false),
            unseen: p["notificationtimestamp"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        };
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `action=watch`.
    ///
    /// # Errors
    ///
    /// The API away or unreadable; a refusal is [`Saved::Refused`].
    pub async fn watch(
        &self,
        incoming: &Incoming,
        form: &WatchForm<'_>,
    ) -> Result<Saved<Value>, ClientError> {
        let titles = form.titles.join("|");
        let mut fields: Vec<(&str, &str)> = vec![
            ("action", "watch"),
            ("titles", &titles),
            ("token", form.token),
        ];
        if form.unwatch {
            fields.push(("unwatch", "1"));
        }
        if let Some(e) = form.expiry {
            fields.push(("expiry", e));
        }
        match form.notify {
            Some(true) => fields.push(("tsnotify", "1")),
            Some(false) => fields.push(("tsnotify", "0")),
            None => {}
        }
        self.action(incoming, &fields).await
    }

    /// `action=setnotificationtimestamp`: the viewer has seen every change to a title, or
    /// to every watched title with `None`, up to now.
    ///
    /// # Errors
    ///
    /// The API away or unreadable; a refusal is [`Saved::Refused`].
    pub async fn mark_watch_seen(
        &self,
        incoming: &Incoming,
        title: Option<&str>,
        token: &str,
    ) -> Result<Saved<Value>, ClientError> {
        let mut fields: Vec<(&str, &str)> =
            vec![("action", "setnotificationtimestamp"), ("token", token)];
        match title {
            Some(t) => fields.push(("titles", t)),
            None => fields.push(("entirewatchlist", "1")),
        }
        self.action(incoming, &fields).await
    }

    /// `GET /watchlist`: the viewer's watches.
    ///
    /// # Errors
    ///
    /// The API away or unreadable, or a refusal.
    pub async fn watches(
        &self,
        incoming: &Incoming,
    ) -> Result<Fetched<Vec<WatchEntry>>, ClientError> {
        let r = self.get(incoming, &format!("{REST}/watchlist")).await?;
        if !r.status.is_success() {
            let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
            return Err(ClientError::Api {
                code: v["code"].as_str().unwrap_or("unknown").to_string(),
                info: v["message"].as_str().unwrap_or_default().to_string(),
            });
        }
        let v: Value = serde_json::from_slice(&r.body)
            .map_err(|e| ClientError::Unexpected(format!("not JSON: {e}")))?;
        let value = serde_json::from_value(v["watches"].clone())
            .map_err(|e| ClientError::Unexpected(e.to_string()))?;
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }
}
