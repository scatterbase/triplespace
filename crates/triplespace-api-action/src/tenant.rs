//! Which tenant a request is for (0056 §10 #5; 0018 §3): the one whose registered base
//! has the request's `Host`, or the `X-Forwarded-Host` of a trusted proxy (0057 §10).
//! The farm base serves the primary tenant, the one the `primary` record names (0046 §2).
//! The auth base (0089 §1) is a registered host but no tenant's. Any other host is 421,
//! unless development mode names a fallback.

use axum::http::HeaderMap;
use tokio_postgres::GenericClient;
use triplespace_projections::tenants::TenantEntry;

use crate::app::{App, Mode};
use crate::forwarded::Origin;

/// A tenant as the request sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tenant {
    /// The slug, which keys the tenant's rows on this instance.
    pub slug: String,
    /// The issuer code its actor keys carry, derived from its founding record (0079 §1).
    pub issuer: String,
    /// The registered base, `https://librarybase.org`.
    pub base: String,
    /// The base the request should see itself under: the registered base, or in
    /// development mode the scheme and host the request came on.
    pub public_base: String,
    /// The host the request came on.
    pub host: String,
}

/// The host of a request: `X-Forwarded-Host` from a trusted proxy (0057 §10), else `Host`.
#[must_use]
pub fn request_host(origin: &Origin, headers: &HeaderMap) -> Option<String> {
    if let Some(h) = &origin.forwarded_host {
        return Some(h.clone());
    }
    headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn request_scheme(app: &App, origin: &Origin) -> &'static str {
    match origin.forwarded_proto.as_deref() {
        Some("https") => "https",
        _ if app.config().mode == Mode::Development => "http",
        _ => "https",
    }
}

/// The host part of a base URL.
pub fn host_of(base: &str) -> Option<String> {
    url::Url::parse(base).ok().and_then(|u| {
        u.host_str().map(|h| match u.port() {
            Some(p) => format!("{h}:{p}"),
            None => h.to_string(),
        })
    })
}

/// Whether a request came to the farm base: the farm's registered base is the tenant's,
/// or the request's host is the farm base's (0046; 0085 §3).
#[must_use]
pub fn at_farm(app: &App, tenant: &Tenant) -> bool {
    let farm = &app.config().farm.base;
    farm.trim_end_matches('/') == tenant.base.trim_end_matches('/')
        || host_of(farm).is_some_and(|h| h.eq_ignore_ascii_case(&tenant.host))
}

/// Whether a request came to the auth base (0089 §1), which is no tenant's.
#[must_use]
pub fn at_auth(app: &App, origin: &Origin, headers: &HeaderMap) -> bool {
    let Some(auth) = &app.config().auth_base else {
        return false;
    };
    let Some(host) = request_host(origin, headers) else {
        return false;
    };
    host_of(auth).is_some_and(|h| h.eq_ignore_ascii_case(&host))
}

/// Every registered tenant.
pub async fn registered<C: GenericClient>(
    client: &C,
) -> Result<Vec<TenantEntry>, tokio_postgres::Error> {
    triplespace_projections::tenants::tenants(client).await
}

fn served(t: &TenantEntry, public_base: String, host: String) -> Tenant {
    Tenant {
        slug: t.slug.clone(),
        issuer: t.issuer.clone(),
        base: t.base.clone(),
        public_base,
        host,
    }
}

/// Resolves the request's tenant, or `None` for an unregistered host.
pub async fn resolve<C: GenericClient>(
    app: &App,
    client: &C,
    origin: &Origin,
    headers: &HeaderMap,
) -> Result<Option<Tenant>, tokio_postgres::Error> {
    let Some(host) = request_host(origin, headers) else {
        return Ok(None);
    };
    let tenants = registered(client).await?;
    let host_lc = host.to_ascii_lowercase();
    if let Some(t) = tenants
        .iter()
        .find(|t| host_of(&t.base).is_some_and(|h| h.eq_ignore_ascii_case(&host_lc)))
    {
        return Ok(Some(served(t, t.base.clone(), host)));
    }
    // The farm base serves the primary tenant (0046 §2).
    if host_of(&app.config().farm.base).is_some_and(|h| h.eq_ignore_ascii_case(&host_lc)) {
        let primary = triplespace_projections::tenants::primary(client).await?;
        if let Some(t) = tenants.iter().find(|t| Some(&t.slug) == primary.as_ref()) {
            return Ok(Some(served(t, t.base.clone(), host)));
        }
    }
    if app.config().mode == Mode::Development
        && let Some(dev) = &app.config().dev_tenant
        && let Some(t) = tenants.iter().find(|t| &t.slug == dev)
    {
        let scheme = request_scheme(app, origin);
        return Ok(Some(served(t, format!("{scheme}://{host}"), host)));
    }
    Ok(None)
}

/// The tenancy policy as `siprop=triplespace` and `GET /tenancy` report it (0028 §1,
/// §11): the switches, and the preset whose values they all are, if any.
pub async fn tenancy_report<C: GenericClient>(
    client: &C,
) -> Result<serde_json::Value, tokio_postgres::Error> {
    let switches = triplespace_projections::tenants::tenancy(client).await?;
    let registry = scatter_actors::tenancy::TenancyRegistry::default_registry();
    let preset = registry
        .presets()
        .iter()
        .find(|p| {
            p.switches
                .iter()
                .filter(|(_, v)| v.as_str() != "set-at-creation")
                .all(|(k, v)| switches.get(k) == Some(v))
        })
        .map(|p| p.name.clone());
    Ok(serde_json::json!({"preset": preset, "switches": switches}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(
            host_of("https://librarybase.org").as_deref(),
            Some("librarybase.org")
        );
        assert_eq!(
            host_of("http://localhost:8080/").as_deref(),
            Some("localhost:8080")
        );
        assert_eq!(host_of("nonsense"), None);
    }
}
