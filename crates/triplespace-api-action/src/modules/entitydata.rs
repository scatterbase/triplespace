//! `Special:EntityData/{id}.json` (0001 §1; 0009 §6; special-pages.toml) and the concept
//! URI `/entity/{id}` (0002 §4), which answers 303 to the data document, as Wikibase does.

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::api::Ctx;
use crate::app::App;
use crate::auth::Caller;
use crate::forwarded::Origin;
use crate::http_cache::{self, Class};
use crate::modules::entities::{entity_json, parse_id};
use crate::params::Params;
use crate::response::ApiError;
use crate::tenant;

fn plain(status: StatusCode, text: String) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        text,
    )
        .into_response()
}

async fn context(app: App, origin: Origin, headers: &HeaderMap) -> Result<Ctx, Response> {
    let client = app
        .pool()
        .get()
        .await
        .map_err(|e| plain(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let tenant = match tenant::resolve(&app, &**client, &origin, headers).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err(plain(
                StatusCode::MISDIRECTED_REQUEST,
                "not a tenant of this instance\n".into(),
            ));
        }
        Err(e) => return Err(plain(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    };
    let caller = Caller::resolve(&**client, &tenant.slug, headers)
        .await
        .map_err(|e| plain(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Ctx {
        app,
        client,
        tenant,
        origin,
        params: Params::default(),
        caller,
        formatversion: 2,
        set_cookie: None,
        warnings: Vec::new(),
        cache_class: crate::http_cache::Class::Public,
        cache_tags: Vec::new(),
    })
}

/// `GET /wiki/Special:EntityData/{id}.json`: the entity document, or 404.
pub async fn special_entity_data(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(file): Path<String>,
) -> Response {
    let Some(id_text) = file.strip_suffix(".json") else {
        return plain(
            StatusCode::NOT_ACCEPTABLE,
            "only the .json form is served\n".into(),
        );
    };
    let ctx = match context(app, origin, &headers).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let id = match parse_id(id_text) {
        Ok(id) => id,
        Err(e) => return plain(StatusCode::BAD_REQUEST, format!("{}\n", e.info)),
    };
    match entity_json(&ctx, &id, id_text).await {
        Ok(v) if v.get("missing").is_some() => plain(
            StatusCode::NOT_FOUND,
            format!("no entity {}\n", id.as_str()),
        ),
        Ok(v) => {
            let body = json!({"entities": {id.as_str(): v}}).to_string();
            let credentialed = ctx.caller.has_session() || crate::auth::bearer(&headers).is_some();
            let class = Class::Public.permitted(&Method::GET, credentialed, false, false);
            http_cache::respond(
                &headers,
                StatusCode::OK,
                "application/json; charset=utf-8",
                body,
                class,
                &[format!("entity:{}", id.as_str())],
            )
        }
        Err(ApiError { code, info, .. }) => plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{code}: {info}\n"),
        ),
    }
}

/// `GET /entity/{id}`: 303 to the data document.
pub async fn concept_uri(Path(id): Path<String>) -> Response {
    match parse_id(&id) {
        Ok(parsed) => (
            StatusCode::SEE_OTHER,
            [(
                header::LOCATION,
                format!("/wiki/Special:EntityData/{}.json", parsed.as_str()),
            )],
        )
            .into_response(),
        Err(e) => plain(StatusCode::BAD_REQUEST, format!("{}\n", e.info)),
    }
}
