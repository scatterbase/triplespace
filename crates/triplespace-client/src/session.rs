//! The viewer's session facts in one call (0012 A70; 19 §5.1): who they are, their
//! unread notifications and, on a page that shows the watch star, the star's state and
//! the tokens its form posts with. A page reaches its first byte within four API calls,
//! and these facts are one of them, where they were up to four.
//!
//! It is one `action=query` with `meta=userinfo|notifications`, joined on a page with a
//! star by `meta=tokens` and `prop=info` for the page's title. A part the API does not
//! serve, or refuses, is absent from the answer and the page draws without it.

use serde_json::Value;

use crate::api::{CacheInfo, Fetched, UserInfo, check, user_info_of};
use crate::notifications::{COUNT_PARAMS, Counts, counts_of};
use crate::transport::ClientError;
use crate::watch::{WatchState, q, watch_state_of};
use crate::{Client, Incoming};

/// The star's facts for the title the session was asked with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watching {
    /// The title asked about, as it was asked.
    pub title: String,
    /// How the viewer watches it.
    pub state: WatchState,
    /// A `watch` token, for the star's form.
    pub token: String,
    /// A `csrf` token, for marking the page's changes seen.
    pub csrf: Option<String>,
}

/// The viewer's session facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    /// Who the viewer is.
    pub user: UserInfo,
    /// Their unread notifications; `None` for an anonymous viewer, or where the API
    /// serves none.
    pub counts: Option<Counts>,
    /// The star's facts, where a title was asked about and the API answered for it.
    pub watching: Option<Watching>,
}

/// The path and query of the call.
fn path(watch: Option<&str>) -> String {
    let mut p = String::from("/w/api.php?action=query&meta=userinfo|notifications");
    if watch.is_some() {
        p.push_str("|tokens&type=watch|csrf");
    }
    p.push_str("&uiprop=options|accountrequests|rights|groups&");
    p.push_str(COUNT_PARAMS);
    if let Some(title) = watch {
        p.push_str("&prop=info&inprop=watched%7Cnotificationtimestamp&titles=");
        p.push_str(&q(title));
    }
    p.push_str("&format=json&formatversion=2");
    p
}

/// The answer read as the session.
fn session_of(v: &Value, watch: Option<&str>) -> Session {
    let query = &v["query"];
    let user = user_info_of(&query["userinfo"]);
    let counts = (!user.anon && query["notifications"].is_object())
        .then(|| counts_of(&query["notifications"]));
    let watching = watch.and_then(|title| {
        let page = query["pages"].get(0)?;
        let token = query["tokens"]["watchtoken"].as_str()?;
        Some(Watching {
            title: title.to_string(),
            state: watch_state_of(page),
            token: token.to_string(),
            csrf: query["tokens"]["csrftoken"].as_str().map(str::to_string),
        })
    });
    Session {
        user,
        counts,
        watching,
    }
}

impl Client {
    /// The viewer's session facts, in one call; with `watch`, a title, also the watch
    /// star's facts for it.
    ///
    /// The API refuses the whole query when one part of it is not the viewer's to ask,
    /// as `meta=notifications` is not an anonymous viewer's: the call is then made again
    /// for who the viewer is alone, so that a stale cookie costs a second call and not
    /// the page.
    ///
    /// # Errors
    ///
    /// The API away or unreadable.
    pub async fn session(
        &self,
        incoming: &Incoming,
        watch: Option<&str>,
    ) -> Result<Fetched<Session>, ClientError> {
        let r = self.get(incoming, &path(watch)).await?;
        match check(&r) {
            Ok(v) => Ok(Fetched {
                value: session_of(&v, watch),
                cache: CacheInfo::of(&r),
            }),
            Err(ClientError::Api { .. }) => {
                let user = self.userinfo(incoming).await?;
                Ok(Fetched {
                    value: Session {
                        user: user.value,
                        counts: None,
                        watching: None,
                    },
                    cache: user.cache,
                })
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 0012 §5 (A70); 0034 §1 rule 5: the session facts are one call.
    #[test]
    fn the_call_asks_for_everything_at_once() {
        let plain = path(None);
        assert!(
            plain.contains("meta=userinfo|notifications&uiprop="),
            "{plain}"
        );
        assert!(
            !plain.contains("tokens") && !plain.contains("titles="),
            "{plain}"
        );
        let starred = path(Some("Item:Q6 & more"));
        assert!(
            starred.contains("meta=userinfo|notifications|tokens&type=watch|csrf&"),
            "{starred}"
        );
        assert!(
            starred.contains(
                "&prop=info&inprop=watched%7Cnotificationtimestamp&titles=Item%3AQ6+%26+more&"
            ),
            "{starred}"
        );
    }

    #[test]
    fn each_part_is_read_and_a_missing_part_is_absent() {
        let v = json!({"query": {
            "userinfo": {"id": 7, "name": "Alice", "rights": ["read"], "groups": ["user"]},
            "notifications": {"alert": {"rawcount": 2, "seenTime": "2026-11-01T00:00:00Z"},
                              "message": {"rawcount": 1}},
            "tokens": {"watchtoken": "w+\\", "csrftoken": "c+\\"},
            "pages": [{"title": "Item:Q6", "watched": true, "watchlistexpiry": "infinity",
                       "notificationtimestamp": "2026-11-02T00:00:00Z"}]
        }});
        let s = session_of(&v, Some("Item:Q6"));
        assert_eq!(s.user.name, "Alice");
        assert_eq!(s.counts.as_ref().map(Counts::unread), Some(3));
        let w = s.watching.expect("the star's facts");
        assert_eq!((w.title.as_str(), w.token.as_str()), ("Item:Q6", "w+\\"));
        assert_eq!(w.csrf.as_deref(), Some("c+\\"));
        assert!(w.state.watched);
        assert_eq!(w.state.unseen.as_deref(), Some("2026-11-02T00:00:00Z"));
        // Asked without a title, the star's facts are not read.
        assert!(session_of(&v, None).watching.is_none());
        // An API that serves no notifications, or no watching, leaves them out.
        let bare = json!({"query": {"userinfo": {"id": 7, "name": "Alice"}}});
        let s = session_of(&bare, Some("Item:Q6"));
        assert!(s.counts.is_none() && s.watching.is_none());
        // An anonymous viewer has no inbox, whatever the answer holds.
        let anon = json!({"query": {"userinfo": {"id": 0, "name": "127.0.0.1", "anon": true},
                                    "notifications": {"alert": {"rawcount": 9}}}});
        assert!(session_of(&anon, None).counts.is_none());
    }
}
