//! `GET /activity` (0012 §5; 0020 §4–5; 16 §4): a feed as the site shows it, in the
//! row shape of 16 §3.1.
//!
//! Parameters, each optional:
//!
//! - `target={kind}:{id}` — the one-target set: `entity:Q6`, `page:12`, `actor:local:7`,
//!   `job:8`. Without it, the everything set.
//! - `actor={key or name}` — rows by one actor: contributions.
//! - `kind=edit|sync|log|job|erased` — row kinds, several with `|`. The default is every
//!   kind but `sync` for the everything set (16 §4.2), every kind for a target.
//! - `source=local|mirror/wikidata|…` — graphs, several with `|`: a history's source
//!   switch (19 §3.5).
//! - `syncs=on|off` — the sync switch of 16 §4.2, a shorthand on `kind`.
//! - `namespace={n}` — entity namespaces, several with `|`.
//! - `tag`, `job`, `exclude_actor`, `log_type`, `log_action`.
//! - `show=bot|!bot|minor|!minor|new|!new|patrolled|!patrolled`.
//! - `since`, `until` — ISO 8601 or MediaWiki timestamps.
//! - `dir=older|newer` — `older` (newest first) by default.
//! - `limit` — rows per page, 50 by default, 500 at most.
//! - `continue` — the `next` token of the previous page.
//! - `lang` — the language of the targets' labels, with `mul` and `en` as fallbacks.
//!
//! The response is `{"rows": [...], "next": token?}`. The *related* and *watched* sets
//! and the stream wait for their projections (16 §4.2–4.4).

use std::collections::BTreeMap;

use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use triplespace_api_action::App;
use triplespace_api_action::activity::{actor_key_of, ts_row, viewer};
use triplespace_api_action::auth::{Caller, bearer};
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::tenant::{self, Tenant};
use triplespace_projections::feed::{
    self, Cursor, Direction, Feed, Flag, TargetSet, parse_timestamp, target_kind,
};

/// A REST error: `{"code": …, "message": …}` with a status.
fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    let body = json!({"code": code, "message": message.into()}).to_string();
    (
        status,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "private, no-cache"),
        ],
        body,
    )
        .into_response()
}

/// The default and the most rows per page.
const DEFAULT_LIMIT: usize = 50;

/// A target parameter: `{kind}:{id}`.
fn target_of(s: &str) -> Option<TargetSet> {
    let (kind, id) = s.split_once(':')?;
    let kind = match kind {
        "entity" => target_kind::ENTITY,
        "page" => target_kind::PAGE,
        "actor" => target_kind::ACTOR,
        "job" => target_kind::JOB,
        "record" => target_kind::RECORD,
        _ => return None,
    };
    (!id.is_empty()).then(|| TargetSet::Target {
        kind,
        id: id.to_string(),
    })
}

/// The entity types a namespace list selects.
fn entity_types_of(namespaces: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for ns in namespaces.split('|').filter(|s| !s.is_empty()) {
        match ns {
            "*" => return Ok(Vec::new()),
            "120" => out.push("item".into()),
            "122" => out.push("property".into()),
            "146" => out.push("lexeme".into()),
            "210" => out.push("domain".into()),
            "212" => out.push("keyword".into()),
            "216" => out.push("notation".into()),
            other if other.parse::<i64>().is_ok() => {}
            other => return Err(format!("\"{other}\" is not a namespace")),
        }
    }
    Ok(out)
}

/// The feed a query asks for.
#[allow(clippy::too_many_lines)]
pub(crate) async fn feed_of(
    ctx: &triplespace_api_action::api::Ctx,
    q: &BTreeMap<String, String>,
    preset: Option<TargetSet>,
) -> Result<Feed, Response> {
    let bad = |name: &str, v: &str| {
        error(
            StatusCode::BAD_REQUEST,
            "bad-parameter",
            format!("\"{v}\" is not a value for {name}"),
        )
    };
    let internal = |e: &dyn std::fmt::Display| {
        error(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string())
    };
    let mut feed = Feed::everything(DEFAULT_LIMIT);
    let mut targeted = false;
    if let Some(t) = preset {
        feed.set = t;
        targeted = true;
    } else if let Some(t) = q.get("target") {
        feed.set = target_of(t).ok_or_else(|| bad("target", t))?;
        targeted = true;
    }
    if let Some(sources) = q.get("source") {
        feed.filters.sources = sources
            .split('|')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
    }
    if let Some(a) = q.get("actor") {
        let key = actor_key_of(ctx, a)
            .await
            .map_err(|e| internal(&e))?
            .unwrap_or_else(|| a.clone());
        match &feed.set {
            TargetSet::Everything => feed.set = TargetSet::Actor(key),
            _ => feed.filters.actor = Some(key),
        }
        targeted = true;
    }
    if let Some(k) = q.get("kind") {
        let kinds: Vec<String> = k
            .split('|')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if let Some(unknown) = kinds.iter().find(|k| !feed::KINDS.contains(&k.as_str())) {
            return Err(bad("kind", unknown));
        }
        feed.filters.kinds = kinds;
    } else {
        feed.filters.kinds = feed::KINDS.iter().map(|s| (*s).to_string()).collect();
        let syncs = match q.get("syncs").map(String::as_str) {
            Some("on") => true,
            Some("off") => false,
            None => targeted,
            Some(other) => return Err(bad("syncs", other)),
        };
        if !syncs {
            feed.filters.kinds.retain(|k| k != "sync");
        }
    }
    if let Some(ns) = q.get("namespace") {
        feed.filters.entity_types = entity_types_of(ns).map_err(|e| bad("namespace", &e))?;
    }
    if let Some(t) = q.get("tag") {
        feed.filters.tag = Some(t.clone());
    }
    if let Some(j) = q.get("job") {
        feed.filters.job = Some(j.parse().map_err(|_| bad("job", j))?);
    }
    if let Some(a) = q.get("exclude_actor") {
        feed.filters.exclude_actor = Some(
            actor_key_of(ctx, a)
                .await
                .map_err(|e| internal(&e))?
                .unwrap_or_else(|| a.clone()),
        );
    }
    if let Some(t) = q.get("log_type") {
        feed.filters.log_type = Some(t.clone());
    }
    if let Some(a) = q.get("log_action") {
        feed.filters.log_action = Some(a.clone());
    }
    if let Some(show) = q.get("show") {
        let show: Vec<String> = show.split('|').map(str::to_string).collect();
        feed.filters.bot = Flag::from_show(&show, "bot");
        feed.filters.minor = Flag::from_show(&show, "minor");
        feed.filters.new = Flag::from_show(&show, "new");
        feed.filters.patrolled = Flag::from_show(&show, "patrolled");
    }
    if let Some(s) = q.get("since") {
        feed.filters.since = Some(parse_timestamp(s).ok_or_else(|| bad("since", s))?);
    }
    if let Some(u) = q.get("until") {
        feed.filters.until = Some(parse_timestamp(u).ok_or_else(|| bad("until", u))?);
    }
    feed.direction = match q.get("dir").map(String::as_str) {
        None | Some("older") => Direction::Older,
        Some("newer") => Direction::Newer,
        Some(other) => return Err(bad("dir", other)),
    };
    if let Some(l) = q.get("limit") {
        let n: usize = l.parse().map_err(|_| bad("limit", l))?;
        feed.limit = n.clamp(1, feed::MAX_LIMIT);
    }
    if let Some(c) = q.get("continue") {
        feed.cursor = Some(Cursor::parse(c).ok_or_else(|| bad("continue", c))?);
    }
    let lang = q.get("lang").map_or("en", String::as_str).to_string();
    feed.languages = if lang == "en" {
        vec!["en".into(), "mul".into()]
    } else {
        vec![lang, "mul".into(), "en".into()]
    };
    Ok(feed)
}

/// The handler.
pub async fn handle(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    serve(app, origin, &method, &headers, &q, None).await
}

/// Serves a feed: the one the query names, or the `preset` target set, which
/// `GET /entity/{id}/history` passes ([`crate::history`]).
pub(crate) async fn serve(
    app: App,
    origin: Origin,
    method: &Method,
    headers: &HeaderMap,
    q: &BTreeMap<String, String>,
    preset: Option<TargetSet>,
) -> Response {
    let internal = |e: &dyn std::fmt::Display| {
        error(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string())
    };
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let tenant = match tenant::resolve(&app, &**client, &origin, headers).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return error(
                StatusCode::MISDIRECTED_REQUEST,
                "misdirected",
                "not a tenant of this instance",
            );
        }
        Err(e) => return internal(&e),
    };
    let caller = match Caller::resolve(&**client, &tenant.slug, headers).await {
        Ok(c) => c,
        Err(e) => return internal(&e.info),
    };
    if caller.require("read").is_err() {
        return error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "reading needs the \"read\" right",
        );
    }
    let credentialed = caller.has_session() || bearer(headers).is_some();
    let ctx = triplespace_api_action::api::Ctx {
        app: app.clone(),
        client,
        tenant,
        origin,
        params: triplespace_api_action::params::Params::default(),
        caller,
        formatversion: 2,
        set_cookie: None,
        warnings: Vec::new(),
        cache_class: Class::Public,
        cache_tags: Vec::new(),
        accept_language: None,
    };
    let feed = match feed_of(&ctx, q, preset).await {
        Ok(f) => f,
        Err(r) => return r,
    };
    let page = match feed::query(ctx.db(), &ctx.tenant.slug, &feed, &viewer(&ctx.caller)).await {
        Ok(p) => p,
        Err(e) => return internal(&e),
    };
    let body = body(&ctx.tenant, &feed, &page).to_string();
    // A feed changes with every write, so the public form caches only briefly at L2
    // (18 §3.1); an actor's or target's feed tags that target.
    let class = Class::Public.permitted(method, credentialed, false, false);
    let tags: Vec<String> = match &feed.set {
        TargetSet::Target { id, kind } if *kind == target_kind::ENTITY => {
            vec![format!("entity:{id}")]
        }
        _ => vec!["activity".into()],
    };
    http_cache::respond(
        headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body,
        class,
        &tags,
    )
}

/// The response body.
fn body(tenant: &Tenant, feed: &Feed, page: &feed::Page) -> Value {
    let rows: Vec<Value> = page.rows.iter().map(|r| ts_row(tenant, r)).collect();
    let mut v = json!({
        "rows": rows,
        "set": match &feed.set {
            TargetSet::Everything => json!("everything"),
            TargetSet::Target { kind, id } => json!({"kind": match *kind {
                target_kind::ENTITY => "entity",
                target_kind::PAGE => "page",
                target_kind::ACTOR => "actor",
                target_kind::JOB => "job",
                _ => "record",
            }, "id": id}),
            TargetSet::Actor(key) => json!({"actor": key}),
        },
        "kinds": feed.filters.kinds,
        "sources": feed.filters.sources,
    });
    if let Some(n) = page.next {
        v["next"] = json!(n.token());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENAPI: &str = include_str!("../../../docs/api/triplespace-v0.openapi.json");

    #[test]
    fn targets_parse() {
        assert_eq!(
            target_of("entity:Q6"),
            Some(TargetSet::Target {
                kind: target_kind::ENTITY,
                id: "Q6".into()
            })
        );
        assert_eq!(
            target_of("actor:local:7"),
            Some(TargetSet::Target {
                kind: target_kind::ACTOR,
                id: "local:7".into()
            })
        );
        assert_eq!(target_of("thing:1"), None);
        assert_eq!(target_of("entity:"), None);
    }

    #[test]
    fn the_contract_names_the_route_and_the_body_meets_it() {
        let doc: Value = serde_json::from_str(OPENAPI).unwrap();
        let get = &doc["paths"]["/activity"]["get"];
        assert_eq!(get["operationId"], "getActivity");
        let names: Vec<&str> = get["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["name"].as_str())
            .collect();
        for p in [
            "target",
            "actor",
            "kind",
            "syncs",
            "namespace",
            "tag",
            "job",
            "exclude_actor",
            "log_type",
            "log_action",
            "show",
            "since",
            "until",
            "dir",
            "limit",
            "continue",
            "lang",
        ] {
            assert!(names.contains(&p), "{p} is not in the contract");
        }
        let schema = &doc["components"]["schemas"]["ActivityPage"];
        let props = schema["properties"].as_object().unwrap();
        let tenant = Tenant {
            slug: "t".into(),
            issuer: "t".into(),
            base: "https://t.example".into(),
            public_base: "https://t.example".into(),
            host: "t.example".into(),
        };
        let b = body(&tenant, &Feed::everything(5), &feed::Page::default());
        for key in b.as_object().unwrap().keys() {
            assert!(props.contains_key(key), "{key} is not in ActivityPage");
        }
        let row = &doc["components"]["schemas"]["ActivityRow"]["properties"];
        for key in [
            "kind",
            "time",
            "source",
            "record",
            "revid",
            "logid",
            "target",
            "actor",
            "operator",
            "requested_by",
            "job",
            "summary",
            "comment",
            "tags",
            "flags",
            "patrolled",
            "size",
            "delta",
            "changes",
            "log",
            "visibility",
            "links",
        ] {
            assert!(row.get(key).is_some(), "{key} is not in ActivityRow");
        }
    }
}
