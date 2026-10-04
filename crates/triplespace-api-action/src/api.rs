//! `api.php`: one handler, the request context every module works in, and the dispatch
//! on `action`.

use axum::body::Bytes;
use axum::extract::{Extension, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use deadpool_postgres::Object;

use crate::app::App;
use crate::auth::Caller;
use crate::forwarded::Origin;
use crate::modules;
use crate::params::Params;
use crate::response::{ApiError, ApiResponse};
use crate::tenant::{self, Tenant};

/// Everything a module needs for one request.
pub struct Ctx {
    /// The state.
    pub app: App,
    /// A pooled connection; writes open their own unit of work through the store.
    pub client: Object,
    /// The tenant the host named.
    pub tenant: Tenant,
    /// Where the request came from: the client address behind trusted proxies.
    pub origin: Origin,
    /// The parameters.
    pub params: Params,
    /// The caller.
    pub caller: Caller,
    /// `formatversion`.
    pub formatversion: u8,
    /// A cookie to set on the way out.
    pub set_cookie: Option<String>,
    /// Warnings gathered along the way.
    pub warnings: Vec<(String, String)>,
}

impl Ctx {
    /// The connection as a plain client.
    #[must_use]
    pub fn db(&self) -> &tokio_postgres::Client {
        &self.client
    }

    /// Adds a warning under a module name.
    pub fn warn(&mut self, module: &str, text: impl Into<String>) {
        self.warnings.push((module.to_string(), text.into()));
    }
}

/// 421 for a host that is no tenant (0056 §10 #5).
fn misdirected(host: Option<&str>) -> Response {
    (
        StatusCode::MISDIRECTED_REQUEST,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        format!(
            "{} is not a tenant of this instance\n",
            host.unwrap_or("(no host)")
        ),
    )
        .into_response()
}

/// The `api.php` handler.
pub async fn handle(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return ApiResponse::err(ApiError::internal(e)).into_response(),
    };
    let tenant = match tenant::resolve(&app, &**client, &origin, &headers).await {
        Ok(Some(t)) => t,
        Ok(None) => return misdirected(tenant::request_host(&origin, &headers).as_deref()),
        Err(e) => return ApiResponse::err(ApiError::internal(e)).into_response(),
    };
    let posted = method == Method::POST;
    let form = posted
        && headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.starts_with("application/x-www-form-urlencoded"));
    let body_text = if form {
        Some(String::from_utf8_lossy(&body).into_owned())
    } else {
        None
    };
    let params = Params::parse(query.as_deref(), body_text.as_deref(), posted);
    let caller = match Caller::resolve(&**client, &tenant.slug, &headers).await {
        Ok(c) => c,
        Err(e) => return ApiResponse::err(e).into_response(),
    };
    let formatversion = match params.formatversion() {
        Ok(v) => v,
        Err(e) => return ApiResponse::err(e).into_response(),
    };
    let mut ctx = Ctx {
        app,
        client,
        tenant,
        origin,
        params,
        caller,
        formatversion,
        set_cookie: None,
        warnings: Vec::new(),
    };
    let mut response = match dispatch(&mut ctx).await {
        Ok(r) => r,
        Err(e) => ApiResponse::err(e),
    };
    response.warnings.append(&mut ctx.warnings);
    if response.set_cookie.is_none() {
        response.set_cookie = ctx.set_cookie.take();
    }
    response.into_response()
}

async fn dispatch(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    match ctx.params.get("format").unwrap_or("jsonfm") {
        "json" | "jsonfm" => {}
        other => {
            return Err(ApiError::new(
                "unknown_format",
                format!("Unrecognized format: {other}. Only json is served."),
            ));
        }
    }
    let action = ctx.params.get("action").unwrap_or("help").to_string();
    match action.as_str() {
        "query" => modules::query::run(ctx).await,
        "login" => modules::login::login(ctx).await,
        "clientlogin" => modules::login::clientlogin(ctx).await,
        "logout" => modules::login::logout(ctx).await,
        "wbgetentities" => modules::entities::wbgetentities(ctx).await,
        "wbsearchentities" => modules::search::wbsearchentities(ctx).await,
        "wbeditentity" | "wbcreateclaim" | "wbsetclaim" | "wbremoveclaims" | "wbsetlabel"
        | "wbsetdescription" | "wbsetaliases" | "wbsetqualifier" | "wbremovequalifiers"
        | "wbsetreference" | "wbremovereferences" => modules::write::run(ctx, &action).await,
        "paraminfo" => modules::query::paraminfo(ctx),
        "help" => Ok(ApiResponse::ok(serde_json::json!({"help": {
            "mime": "text/plain",
            "help": "Triplespace Action API. Modules: query (meta=siteinfo|tokens|userinfo), login, clientlogin, logout, wbgetentities, wbsearchentities, wbeditentity, wbcreateclaim, wbsetclaim, wbremoveclaims, wbsetlabel, wbsetdescription, wbsetaliases, wbsetqualifier, wbremovequalifiers, wbsetreference, wbremovereferences."
        }}))),
        other => Err(ApiError::new(
            "badvalue",
            format!("Unrecognized value for parameter \"action\": {other}."),
        )),
    }
}
