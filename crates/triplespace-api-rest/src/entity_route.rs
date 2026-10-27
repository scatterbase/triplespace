//! What every `/entity/{id}/…` route does first: the error shape, the tenant, the
//! caller and the `read` right, the ID in any accepted form, and whether the entity is
//! there for this viewer — a missing entity and a confidential one answer alike
//! (0056 §5), as `/provenance` answers.

use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use scatter_wikibase_model::id::{EntityId, IdForm};
use serde_json::json;
use triplespace_api_action::App;
use triplespace_api_action::auth::{Caller, bearer};
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::modules::entities::parse_id;
use triplespace_api_action::tenant::{self, Tenant};
use triplespace_projections::read;

/// A REST error: `{"code": …, "message": …}` with a status.
pub(crate) fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    let body = json!({"code": code, "message": message.into()}).to_string();
    let mut r = (
        status,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "private, no-cache"),
        ],
        body,
    )
        .into_response();
    if status == StatusCode::NOT_FOUND {
        r.headers_mut().insert(
            header::VARY,
            header::HeaderValue::from_static("Accept, Accept-Language"),
        );
    }
    r
}

/// An internal error.
pub(crate) fn internal(e: &dyn std::fmt::Display) -> Response {
    error(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string())
}

/// The request, resolved.
pub(crate) struct Resolved {
    /// The tenant.
    pub tenant: Tenant,
    /// The entity.
    pub id: EntityId,
    /// Whether the request carried a credential, for the cache class.
    pub credentialed: bool,
}

/// Resolves an entity route on a connection, or answers for it.
pub(crate) async fn resolve(
    app: &App,
    client: &tokio_postgres::Client,
    origin: &Origin,
    headers: &HeaderMap,
    id_text: &str,
) -> Result<Resolved, Response> {
    let tenant = match tenant::resolve(app, client, origin, headers).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err(error(
                StatusCode::MISDIRECTED_REQUEST,
                "misdirected",
                "not a tenant of this instance",
            ));
        }
        Err(e) => return Err(internal(&e)),
    };
    let caller = Caller::resolve(client, &tenant.slug, headers)
        .await
        .map_err(|e| internal(&e.info))?;
    if caller.require("read").is_err() {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "reading needs the \"read\" right",
        ));
    }
    let id = parse_id(id_text)
        .map_err(|e| error(StatusCode::BAD_REQUEST, "invalid-entity-id", e.info))?;
    let registry = app
        .registry_for(client, &tenant.slug)
        .await
        .map_err(|e| internal(&e))?;
    let id = triplespace_api_action::entity_json::canonical_in(id, registry);
    if id.form() == IdForm::Foreign && registry.parse_foreign_id(id.as_str()).is_err() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid-entity-id",
            format!("\"{}\" names no registered provider", id.as_str()),
        ));
    }
    // A key-mapped provider's own item ID resolves to the keyed entity (05 §4.1).
    let id = match read::mapped_alias(client, registry, &tenant.slug, &id)
        .await
        .map_err(|e| internal(&e))?
    {
        Some(target) => target,
        None => id,
    };
    let current = read::current(client, app.order(), &tenant.slug, &id)
        .await
        .map_err(|e| internal(&e))?;
    let held = current.is_some();
    // An entity under a restriction the caller does not satisfy is absent, and a deleted
    // one is to those outside its group (09 §5.3, §6.2); a private tenant's are absent for
    // everyone outside it (09 §4.7).
    let graph = current
        .as_ref()
        .and_then(|c| c.contributions.first().map(|g| g.graph.clone()));
    let chain = triplespace_projections::acl_read::entity_chain(
        client,
        &tenant.slug,
        id.as_str(),
        graph.as_deref(),
    )
    .await
    .map_err(|e| internal(&e))?;
    let readable = scatter_actors::read_decision(
        &caller.effective,
        &chain,
        scatter_actors::Timestamp(triplespace_api_action::edit::now_micros()),
    ) == scatter_actors::ReadDecision::Readable;
    // A keyed ID no graph holds is still an entity (0009 §4); a source's is not.
    let keyed = id.form() == IdForm::Keyed
        && !triplespace_api_action::entity_json::is_source_id(&id, registry);
    if !readable || (!held && !keyed) {
        return Err(error(
            StatusCode::NOT_FOUND,
            "entity-not-found",
            format!("no entity {}", id.as_str()),
        ));
    }
    let credentialed = caller.has_session() || bearer(headers).is_some();
    Ok(Resolved {
        tenant,
        id,
        credentialed,
    })
}
