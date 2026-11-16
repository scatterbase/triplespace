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
use crate::http_cache::{self, Class};
use crate::modules;
use crate::params::Params;
use crate::response::{ApiError, ApiResponse};
use crate::tenant::{self, Tenant};

/// Everything a module needs for one request.
pub struct Ctx {
    /// The state.
    pub app: App,
    /// A pooled connection; writes open their own unit of work through the store. The
    /// public API's pool, or the accounts service's for an action of
    /// [`ACCOUNTS_ACTIONS`] (0013 §4, A99).
    pub client: Object,
    /// A connection of the accounts service's pool, opened when a public action first
    /// needs one ([`Ctx::accounts`]).
    pub accounts: tokio::sync::OnceCell<Object>,
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
    /// The cache class a module asks for; the request may only lower it
    /// ([`Class::permitted`]).
    pub cache_class: Class,
    /// The `Cache-Tag`s of the entities the response drew on (0014 §5).
    pub cache_tags: Vec<String>,
    /// The request's `Accept-Language`, for the preferences negotiated from it (0085 §2.2).
    pub accept_language: Option<String>,
    /// The batch the write joined (0104 §1), which the response reports; 0 for none.
    pub batch: std::sync::atomic::AtomicU64,
    /// Why the action wrote nothing, where it would normally have written and what it
    /// asked for was already so (18 §1.5): the response warns with it under the
    /// action's name ([`Ctx::unchanged`]).
    pub unchanged: std::sync::Mutex<Option<String>>,
}

impl Ctx {
    /// The connection as a plain client.
    #[must_use]
    pub fn db(&self) -> &tokio_postgres::Client {
        &self.client
    }

    /// A connection of the accounts service (0013 §4, A99), for what reads or writes
    /// `private` on an account's behalf: a session, a token, the watch set, preferences.
    /// Opened on first use and kept for the request.
    ///
    /// # Errors
    ///
    /// An internal error when the pool cannot give a connection.
    pub async fn accounts(&self) -> Result<&tokio_postgres::Client, ApiError> {
        let object = self
            .accounts
            .get_or_try_init(|| async {
                self.app
                    .accounts_pool()
                    .get()
                    .await
                    .map_err(ApiError::internal)
            })
            .await?;
        Ok(object)
    }

    /// The tenant's table of names (0078 §3): the registry with its entity sources.
    ///
    /// # Errors
    ///
    /// An internal error when the store fails.
    pub async fn registry(&self) -> Result<&'static scatter_providers::Registry, ApiError> {
        self.app
            .registry_for(&self.client, &self.tenant.slug)
            .await
            .map_err(ApiError::internal)
    }

    /// Adds a warning under a module name.
    pub fn warn(&mut self, module: &str, text: impl Into<String>) {
        self.warnings.push((module.to_string(), text.into()));
    }

    /// Notes that the action changed nothing and wrote nothing, and why: what it asked
    /// for was already so. The response still succeeds, as an idempotent request should,
    /// and carries the reason as a warning under the action's name, so that a client
    /// that expected a new revision or log entry can tell there is none. A later, more
    /// exact reason replaces an earlier one.
    pub fn unchanged(&self, why: impl Into<String>) {
        *self
            .unchanged
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(why.into());
    }
}

/// Adds the warning of an action that wrote nothing ([`Ctx::unchanged`]) to its answer,
/// under the action's name. An answer that is an error says why itself.
fn with_unchanged(ctx: &Ctx, response: &mut ApiResponse) {
    let why = ctx
        .unchanged
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let (Some(why), None) = (why, &response.error) {
        let action = ctx.params.get("action").unwrap_or("help").to_string();
        response.warn(&action, &format!("{why} Nothing was written."));
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

/// Reads a `multipart/form-data` body into the parameters.
async fn multipart_into(
    params: &mut Params,
    content_type: &str,
    body: Bytes,
) -> Result<(), ApiError> {
    use axum::extract::{FromRequest as _, Multipart};
    let request = axum::http::Request::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(body))
        .map_err(|e| ApiError::new("badupload", e.to_string()))?;
    let mut multipart = Multipart::from_request(request, &())
        .await
        .map_err(|e| ApiError::new("badupload", e.body_text()))?;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::new("badupload", e.body_text()))?
    {
        let Some(name) = field.name().map(str::to_string) else {
            continue;
        };
        if field.file_name().is_some() {
            let bytes = field
                .bytes()
                .await
                .map_err(|e| ApiError::new("badupload", e.body_text()))?;
            params.add_part(&name, None, Some(bytes.to_vec()));
        } else {
            let text = field
                .text()
                .await
                .map_err(|e| ApiError::new("badupload", e.body_text()))?;
            params.add_part(&name, Some(text), None);
        }
    }
    Ok(())
}

/// The batch a write joined (0104 §6), reported beside the module's own answer.
fn with_batch(ctx: &Ctx, response: &mut ApiResponse) {
    let batch = ctx.batch.load(std::sync::atomic::Ordering::Relaxed);
    if batch != 0 && response.error.is_none() {
        response
            .body
            .insert("batch".into(), serde_json::json!(batch));
    }
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
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let form = posted && content_type.starts_with("application/x-www-form-urlencoded");
    let body_text = form.then(|| String::from_utf8_lossy(&body).into_owned());
    let mut params = Params::parse(query.as_deref(), body_text.as_deref(), posted);
    if posted && content_type.starts_with("multipart/form-data") {
        // A multipart body (`action=import`'s `xml` upload): its text fields are body
        // parameters, its files kept by field name.
        if let Err(e) = multipart_into(&mut params, &content_type, body).await {
            return ApiResponse::err(e).into_response();
        }
    }
    let caller = match Caller::resolve(&**client, &tenant.slug, &headers).await {
        Ok(c) => c,
        Err(e) => return ApiResponse::err(e).into_response(),
    };
    let formatversion = match params.formatversion() {
        Ok(v) => v,
        Err(e) => return ApiResponse::err(e).into_response(),
    };
    let client = match connection_for(&app, &params, client).await {
        Ok(c) => c,
        Err(e) => return ApiResponse::err(e).into_response(),
    };
    let mut ctx = Ctx {
        accounts: tokio::sync::OnceCell::new(),
        app,
        client,
        tenant,
        origin,
        params,
        caller,
        formatversion,
        set_cookie: None,
        warnings: Vec::new(),
        cache_class: Class::Public,
        cache_tags: Vec::new(),
        accept_language: accept_language(&headers),
        batch: std::sync::atomic::AtomicU64::new(0),
        unchanged: std::sync::Mutex::new(None),
    };
    let mut response = match dispatch(&mut ctx).await {
        Ok(r) => r,
        Err(e) => ApiResponse::err(e),
    };
    response.warnings.append(&mut ctx.warnings);
    with_unchanged(&ctx, &mut response);
    with_batch(&ctx, &mut response);
    if response.set_cookie.is_none() {
        response.set_cookie = ctx.set_cookie.take();
    }
    let credentialed = ctx.caller.has_session() || crate::auth::bearer(&headers).is_some();
    let class = ctx.cache_class.permitted(
        &method,
        credentialed,
        response.set_cookie.is_some(),
        response.error.is_some(),
    );
    let (content_type, body) = match response.raw.take() {
        Some((ct, body)) if response.error.is_none() => (ct, body),
        _ => (
            "application/json; charset=utf-8",
            response.to_value().to_string(),
        ),
    };
    let mut r = http_cache::respond(
        &headers,
        StatusCode::OK,
        content_type,
        body,
        class,
        &ctx.cache_tags,
    );
    if let Some(e) = &response.error
        && let Ok(v) = header::HeaderValue::from_str(&e.code)
    {
        r.headers_mut().insert("MediaWiki-API-Error", v);
    }
    if let Some(c) = &response.set_cookie
        && let Ok(v) = header::HeaderValue::from_str(c)
    {
        r.headers_mut().insert(header::SET_COOKIE, v);
    }
    r
}

/// The request's `Accept-Language`, for the preferences negotiated from it (0085 §2.2).
fn accept_language(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// The connection an action runs on: the accounts service's pool for an action of
/// [`ACCOUNTS_ACTIONS`], which reads and writes `private`, and the accounts partition, in the
/// same transaction as its records (0013 §4, A99); the public API's `client` otherwise.
async fn connection_for(app: &App, params: &Params, client: Object) -> Result<Object, ApiError> {
    if ACCOUNTS_ACTIONS.contains(&params.get("action").unwrap_or_default()) {
        drop(client);
        app.accounts_pool().get().await.map_err(ApiError::internal)
    } else {
        Ok(client)
    }
}

/// The actions of the accounts service (0013 §4, A99), which run on its pool: sign-in and
/// sign-out, accounts and their links, preferences, renames (name tombstones), the watch
/// set and the inbox.
pub const ACCOUNTS_ACTIONS: &[&str] = &[
    "login",
    "clientlogin",
    "createaccount",
    "linkaccount",
    "unlinkaccount",
    "logout",
    "options",
    "renameuser",
    "watch",
    "setnotificationtimestamp",
    "echomarkread",
    "echomarkseen",
];

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
    modules::query::report_action(&action);
    run_action(ctx, &action).await
}

/// Declares every `action=` module once: its name, whether it must be POSTed, and what runs
/// it. [`ACTION_MODULES`], which `action=paraminfo` and the help text read, and the dispatch
/// in [`run_action`] are both generated from the one table, so a module cannot be dispatched
/// without being advertised, or advertised without being dispatched.
macro_rules! action_modules {
    ($ctx:ident, $action:ident; $( $($name:literal)|+ , $post:literal => $run:expr ;)+) => {
        /// The `action=` modules, as `action=paraminfo` lists them: `(name, must be POSTed)`.
        pub(crate) const ACTION_MODULES: &[(&str, bool)] = &[$($(($name, $post),)+)+];

        async fn run_action($ctx: &mut Ctx, $action: &str) -> Result<ApiResponse, ApiError> {
            match $action {
                $($($name)|+ => $run,)+
                other => Err(ApiError::new(
                    "badvalue",
                    format!("Unrecognized value for parameter \"action\": {other}."),
                )),
            }
        }
    };
}

action_modules! { ctx, action;
    "query", false => modules::query::run(ctx).await;
    "paraminfo", false => modules::query::paraminfo(ctx);
    "help", false => Ok(ApiResponse::ok(serde_json::json!({"help": {
        "mime": "text/plain",
        "help": modules::query::help_text(),
    }})));
    "login", true => modules::login::login(ctx).await;
    "clientlogin", true => modules::login::clientlogin(ctx).await;
    "createaccount", true => modules::account::createaccount(ctx).await;
    "linkaccount", true => modules::issuers::linkaccount(ctx).await;
    "unlinkaccount", true => modules::issuers::unlinkaccount(ctx).await;
    "logout", true => modules::login::logout(ctx).await;
    "options", true => modules::options::run(ctx).await;
    "parse", false => modules::parse::run(ctx).await;
    "compare", false => modules::compare::run(ctx).await;
    "edit", true => modules::editpage::run(ctx).await;
    "move", true => modules::movepage::run(ctx).await;
    "import", true => modules::import::run(ctx).await;
    "delete", true => modules::moderation::delete(ctx).await;
    "undelete", true => modules::moderation::undelete(ctx).await;
    "protect", true => modules::moderation::protect(ctx).await;
    "revisiondelete", true => modules::moderation::revisiondelete(ctx).await;
    "userrights", true => modules::userrights::run(ctx).await;
    "renameuser", true => modules::renameuser::run(ctx).await;
    "block", true => modules::block::block(ctx).await;
    "unblock", true => modules::block::unblock(ctx).await;
    "echomarkread", true => modules::notifications::markread(ctx).await;
    "echomarkseen", true => modules::notifications::markseen(ctx).await;
    "watch", true => modules::watch::run(ctx).await;
    "setnotificationtimestamp", true => modules::watch::setnotificationtimestamp(ctx).await;
    "wbgetentities", false => modules::entities::wbgetentities(ctx).await;
    "wbsearchentities", false => modules::search::wbsearchentities(ctx).await;
    "wbeditentity" | "wbcreateclaim" | "wbsetclaim" | "wbremoveclaims" | "wbsetlabel"
        | "wbsetdescription" | "wbsetaliases" | "wbsetclaimvalue" | "wbsetqualifier" | "wbremovequalifiers"
        | "wbsetreference" | "wbremovereferences", true => modules::write::run(ctx, action).await;
}
