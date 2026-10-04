//! Which tenant a request is for (0056 §10 #5; 0018 §3): the one whose registered base
//! has the request's `Host`. The farm base serves the primary tenant. An unregistered
//! host is 421, unless development mode names a fallback.

use axum::http::HeaderMap;
use tokio_postgres::GenericClient;

use crate::app::{App, Mode};

/// A tenant as the request sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tenant {
    /// The slug.
    pub slug: String,
    /// The registered base, `https://librarybase.org`.
    pub base: String,
    /// The base the request should see itself under: the registered base, or in
    /// development mode the scheme and host the request came on.
    pub public_base: String,
    /// The host the request came on.
    pub host: String,
}

/// The host of a request: `X-Forwarded-Host` from a trusted proxy, else `Host`.
#[must_use]
pub fn request_host(app: &App, headers: &HeaderMap) -> Option<String> {
    let trusted = !app.trusted_proxies().is_empty();
    if trusted
        && let Some(h) = headers
            .get("x-forwarded-host")
            .and_then(|v| v.to_str().ok())
    {
        return Some(h.split(',').next().unwrap_or(h).trim().to_string());
    }
    headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn request_scheme(app: &App, headers: &HeaderMap) -> &'static str {
    if !app.trusted_proxies().is_empty()
        && headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|p| p.eq_ignore_ascii_case("https"))
    {
        return "https";
    }
    if app.config().mode == Mode::Development {
        "http"
    } else {
        "https"
    }
}

/// The host part of a base URL.
fn host_of(base: &str) -> Option<String> {
    url::Url::parse(base).ok().and_then(|u| {
        u.host_str().map(|h| match u.port() {
            Some(p) => format!("{h}:{p}"),
            None => h.to_string(),
        })
    })
}

/// Every registered tenant: slug and base.
pub async fn registered<C: GenericClient>(
    client: &C,
) -> Result<Vec<(String, String)>, tokio_postgres::Error> {
    let rows = client
        .query(
            "SELECT code, config->>'base' FROM view.registry WHERE tenant = '' AND kind = 'tenant' ORDER BY code",
            &[],
        )
        .await?;
    Ok(rows
        .iter()
        .filter_map(|r| Some((r.get::<_, String>(0), r.get::<_, Option<String>>(1)?)))
        .collect())
}

/// Resolves the request's tenant, or `None` for an unregistered host.
pub async fn resolve<C: GenericClient>(
    app: &App,
    client: &C,
    headers: &HeaderMap,
) -> Result<Option<Tenant>, tokio_postgres::Error> {
    let Some(host) = request_host(app, headers) else {
        return Ok(None);
    };
    let tenants = registered(client).await?;
    let host_lc = host.to_ascii_lowercase();
    for (slug, base) in &tenants {
        if host_of(base).is_some_and(|h| h.eq_ignore_ascii_case(&host_lc)) {
            return Ok(Some(Tenant {
                slug: slug.clone(),
                public_base: base.clone(),
                base: base.clone(),
                host,
            }));
        }
    }
    // The farm base serves the primary tenant (0046): the first registered one until a
    // `primary` entry says otherwise.
    if host_of(&app.config().farm.base).is_some_and(|h| h.eq_ignore_ascii_case(&host_lc))
        && let Some((slug, base)) = tenants.first()
    {
        return Ok(Some(Tenant {
            slug: slug.clone(),
            public_base: base.clone(),
            base: base.clone(),
            host,
        }));
    }
    if app.config().mode == Mode::Development
        && let Some(dev) = &app.config().dev_tenant
        && let Some((slug, base)) = tenants.iter().find(|(s, _)| s == dev)
    {
        let scheme = request_scheme(app, headers);
        return Ok(Some(Tenant {
            slug: slug.clone(),
            base: base.clone(),
            public_base: format!("{scheme}://{host}"),
            host,
        }));
    }
    Ok(None)
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
