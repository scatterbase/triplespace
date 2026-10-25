//! Threads and talk pages (0019 §9; 18 §3.2): a talk page's listing, a thread made ready
//! for display (`GET /thread/{id}?html=1`), and the writes a reader makes from the site,
//! starting a thread and replying to one.

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use crate::Client;
use crate::api::{CacheInfo, Fetched};
use crate::forward::Incoming;
use crate::transport::{ApiResponse, ClientError};

const REST: &str = "/w/rest.php/triplespace/v0";

/// One thread as a talk page's listing names it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listed {
    /// Its page ID.
    pub id: u64,
    /// Its title, without the `Thread:` prefix.
    pub title: String,
    /// Its subject.
    pub subject: String,
    /// Its status.
    pub status: String,
    /// Whether that status is open or closed.
    pub closed: bool,
    /// Whether the listing page is its home.
    pub home: bool,
    /// When it was pinned on the listing page, ISO 8601.
    pub pinned: Option<String>,
    /// Its latest post's time, ISO 8601.
    pub last_post: Option<String>,
    /// How many posts it has.
    pub posts: u64,
}

/// A talk page's or board's threads (`GET /page/{id}/threads`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// The talk page's ID.
    pub talk_page_id: u64,
    /// The threads, pinned first.
    pub threads: Vec<Listed>,
}

/// A page a thread is attached to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attached {
    /// The talk page's ID.
    pub talk_page_id: Option<u64>,
    /// Its title, prefixed.
    pub title: Option<String>,
    /// When the thread was attached there, ISO 8601.
    pub attached: Option<String>,
    /// When it was pinned there, ISO 8601.
    pub pinned: Option<String>,
}

/// One post, ready to show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PostView {
    /// Its revision ID.
    pub id: u64,
    /// The post it answers.
    pub in_reply_to: Option<u64>,
    /// The name its author shows; `None` where hidden.
    pub author: Option<String>,
    /// When it was posted, ISO 8601.
    pub published: Option<String>,
    /// When it was last edited, ISO 8601.
    pub updated: Option<String>,
    /// Its markup's media type.
    pub media_type: String,
    /// Its text, rendered and sanitized; `None` where hidden or erased.
    pub html: Option<String>,
    /// Its depth in the tree.
    pub depth: usize,
    /// Whether a standing collapse hides it.
    pub collapsed: bool,
    /// The range it collapses beneath itself, `(first, last)`.
    pub collapse: Option<(u64, u64)>,
    /// The status it set.
    pub status: Option<String>,
    /// Whether its text is hidden or erased.
    pub texthidden: bool,
    /// Whether an import wrote it.
    pub imported: bool,
}

/// A thread, ready to show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadView {
    /// Its page ID.
    pub id: u64,
    /// Its title, without the `Thread:` prefix.
    pub title: String,
    /// Its subject.
    pub subject: String,
    /// Its status.
    pub status: String,
    /// Whether that status is closed.
    pub closed: bool,
    /// Its home.
    pub home: Attached,
    /// Its listings.
    pub listings: Vec<Attached>,
    /// Its posts, in tree order.
    pub posts: Vec<PostView>,
    /// The offset a later write names.
    pub base: u64,
}

impl ThreadView {
    /// When the thread was attached to a talk page, ISO 8601: its home's or a listing's.
    #[must_use]
    pub fn attached_to(&self, talk_page_id: u64) -> Option<&Attached> {
        std::iter::once(&self.home)
            .chain(&self.listings)
            .find(|a| a.talk_page_id == Some(talk_page_id))
    }
}

/// A reply or a new thread, as the site's form posts it.
#[derive(Debug, Clone, Default)]
pub struct PostForm<'a> {
    /// The text.
    pub text: &'a str,
    /// The post it answers; `None` for a top-level post.
    pub in_reply_to: Option<u64>,
    /// A status the post sets.
    pub status: Option<&'a str>,
    /// The viewer's CSRF token.
    pub token: &'a str,
}

/// What a write answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Written {
    /// Written: the thread and the new post's revision ID.
    Done {
        /// The thread's page ID.
        thread: u64,
        /// The new record's revision ID.
        revid: u64,
        /// The thread's title, without the `Thread:` prefix.
        title: Option<String>,
    },
    /// Refused, with the API's code and message.
    Refused {
        /// The code (`ts-empty-post`, `badtoken`, …).
        code: String,
        /// The message.
        message: String,
    },
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

fn attached(v: &Value) -> Attached {
    Attached {
        talk_page_id: v["talk_page_id"].as_u64(),
        title: s(&v["talk_page"]["prefixed"]),
        attached: s(&v["attached"]),
        pinned: s(&v["pinned"]),
    }
}

fn post(v: &Value) -> PostView {
    PostView {
        id: v["id"].as_u64().unwrap_or(0),
        in_reply_to: v["inReplyTo"].as_u64(),
        author: s(&v["author"]),
        published: s(&v["published"]),
        updated: s(&v["updated"]),
        media_type: s(&v["mediaType"]).unwrap_or_else(|| "text/markdown".into()),
        html: s(&v["html"]),
        depth: v["depth"]
            .as_u64()
            .and_then(|d| usize::try_from(d).ok())
            .unwrap_or(0),
        collapsed: v["collapsed"].as_bool().unwrap_or(false),
        collapse: v["collapse"]["first"]
            .as_u64()
            .zip(v["collapse"]["last"].as_u64()),
        status: s(&v["status"]),
        texthidden: v["texthidden"].as_bool().unwrap_or(false),
        imported: v["imported"].as_bool().unwrap_or(false),
    }
}

/// A thread body as JSON.
#[must_use]
pub fn thread_of(v: &Value) -> ThreadView {
    ThreadView {
        id: v["id"].as_u64().unwrap_or(0),
        title: s(&v["title"]).unwrap_or_default(),
        subject: s(&v["subject"]).unwrap_or_default(),
        status: s(&v["status"]).unwrap_or_else(|| "open".into()),
        closed: v["category"] == "closed",
        home: attached(&v["home"]),
        listings: v["listings"]
            .as_array()
            .map(|a| a.iter().map(attached).collect())
            .unwrap_or_default(),
        posts: v["posts"]
            .as_array()
            .map(|a| a.iter().map(post).collect())
            .unwrap_or_default(),
        base: v["base"].as_u64().unwrap_or(0),
    }
}

fn listing_of(v: &Value) -> Listing {
    Listing {
        talk_page_id: v["talk_page_id"].as_u64().unwrap_or(0),
        threads: v["threads"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|t| t["deleted"] != Value::Bool(true))
                    .map(|t| Listed {
                        id: t["id"].as_u64().unwrap_or(0),
                        title: s(&t["title"]).unwrap_or_default(),
                        subject: s(&t["subject"]).unwrap_or_default(),
                        status: s(&t["status"]).unwrap_or_else(|| "open".into()),
                        closed: t["category"] == "closed",
                        home: t["home"].as_bool().unwrap_or(false),
                        pinned: s(&t["pinned"]),
                        last_post: s(&t["last_post"]),
                        posts: t["posts"].as_u64().unwrap_or(0),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn rest_value(r: &ApiResponse) -> Result<Value, ClientError> {
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    if r.status == StatusCode::OK {
        return Ok(v);
    }
    Err(ClientError::Api {
        code: s(&v["code"]).unwrap_or_else(|| format!("http-{}", r.status.as_u16())),
        info: s(&v["message"]).unwrap_or_default(),
    })
}

fn written(r: &ApiResponse) -> Result<Written, ClientError> {
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    match r.status {
        StatusCode::OK | StatusCode::CREATED => Ok(Written::Done {
            thread: v["thread"].as_u64().unwrap_or(0),
            revid: v["revid"].as_u64().unwrap_or(0),
            title: s(&v["title"]),
        }),
        StatusCode::BAD_REQUEST
        | StatusCode::FORBIDDEN
        | StatusCode::CONFLICT
        | StatusCode::GONE
        | StatusCode::NOT_FOUND => Ok(Written::Refused {
            code: s(&v["code"]).unwrap_or_else(|| format!("http-{}", r.status.as_u16())),
            message: s(&v["message"]).unwrap_or_default(),
        }),
        other => Err(ClientError::Unexpected(format!(
            "a thread write answered HTTP {other}"
        ))),
    }
}

impl Client {
    /// `GET /page/{id}/threads`: a talk page's or board's threads, pinned first. Threads
    /// marked deleted, which only the deletion group sees, are left out.
    ///
    /// # Errors
    ///
    /// The API's refusal (`ts-not-a-talk-page`, `ts-page-deleted`), or the API away.
    pub async fn talk_threads(
        &self,
        incoming: &Incoming,
        talk_page_id: u64,
    ) -> Result<Fetched<Listing>, ClientError> {
        let r = self
            .get(incoming, &format!("{REST}/page/{talk_page_id}/threads"))
            .await?;
        let v = rest_value(&r)?;
        Ok(Fetched {
            value: listing_of(&v),
            cache: CacheInfo::of(&r),
        })
    }

    /// `GET /thread/{id}?html=1`: a thread with its posts rendered and named.
    ///
    /// # Errors
    ///
    /// The API's refusal (`ts-thread-not-found`, `ts-thread-deleted`), or the API away.
    pub async fn thread(
        &self,
        incoming: &Incoming,
        id: u64,
    ) -> Result<Fetched<ThreadView>, ClientError> {
        let r = self
            .get(incoming, &format!("{REST}/thread/{id}?html=1"))
            .await?;
        let v = rest_value(&r)?;
        Ok(Fetched {
            value: thread_of(&v),
            cache: CacheInfo::of(&r),
        })
    }

    /// `POST /thread`: a new thread on a page's talk page.
    ///
    /// # Errors
    ///
    /// The API away or unreadable; a refusal is [`Written::Refused`].
    pub async fn start_thread(
        &self,
        incoming: &Incoming,
        subject_page: u64,
        subject: &str,
        form: &PostForm<'_>,
    ) -> Result<Written, ClientError> {
        let body = json!({
            "subject": subject,
            "target": {"kind": "page", "id": subject_page},
            "text": form.text,
            "token": form.token,
        });
        let r = self
            .send_json(incoming, Method::POST, &format!("{REST}/thread"), &body)
            .await?;
        written(&r)
    }

    /// `POST /thread/{id}/post`: a reply, or a top-level post, perhaps setting a status.
    ///
    /// # Errors
    ///
    /// The API away or unreadable; a refusal is [`Written::Refused`].
    pub async fn reply(
        &self,
        incoming: &Incoming,
        thread: u64,
        form: &PostForm<'_>,
    ) -> Result<Written, ClientError> {
        let mut body = json!({"text": form.text, "token": form.token});
        if let Some(p) = form.in_reply_to {
            body["inReplyTo"] = json!(p);
        }
        if let Some(st) = form.status.filter(|s| !s.is_empty()) {
            body["status"] = json!(st);
        }
        let r = self
            .send_json(
                incoming,
                Method::POST,
                &format!("{REST}/thread/{thread}/post"),
                &body,
            )
            .await?;
        written(&r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_body_reads_into_its_view() {
        let v = json!({
            "id": 12, "title": "2026-10-25/Scope", "subject": "Scope", "status": "resolved",
            "category": "closed", "base": 40,
            "home": {"talk_page_id": 9, "talk_page": {"ns": 5, "title": "Scope", "prefixed": "Librarybase talk:Scope"},
                     "attached": "2026-10-25T10:00:00Z", "pinned": null},
            "listings": [{"talk_page_id": 30, "talk_page": {"prefixed": "Board:Ontology"}, "attached": "2026-10-26T10:00:00Z"}],
            "posts": [
                {"id": 12, "inReplyTo": null, "author": "Alice", "mediaType": "text/markdown",
                 "html": "<p>Hi</p>", "depth": 0, "collapsed": false},
                {"id": 13, "inReplyTo": 12, "author": null, "depth": 1, "texthidden": true,
                 "status": "resolved", "collapse": {"first": 12, "last": 12}}
            ]
        });
        let t = thread_of(&v);
        assert!(t.closed);
        assert_eq!(t.home.title.as_deref(), Some("Librarybase talk:Scope"));
        assert_eq!(
            t.attached_to(30).and_then(|a| a.attached.as_deref()),
            Some("2026-10-26T10:00:00Z")
        );
        assert_eq!(t.posts[0].author.as_deref(), Some("Alice"));
        assert_eq!(t.posts[1].in_reply_to, Some(12));
        assert!(t.posts[1].texthidden && t.posts[1].html.is_none());
        assert_eq!(t.posts[1].collapse, Some((12, 12)));
        assert_eq!(t.posts[1].status.as_deref(), Some("resolved"));
    }
}
