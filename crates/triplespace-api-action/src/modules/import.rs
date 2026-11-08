//! `action=import` (18 §2.3; 10 §6; 21 §4.3; 0008 §9, A48): MediaWiki's module, running
//! the general import job on the request path, since no job runner exists yet (0047 §8.3).
//!
//! **Sources.** `xml`, a file in a `multipart/form-data` body (`importupload`); or
//! `interwikisource`, a site alias of `sites.toml` (0026 §2), with `interwikipage` the
//! title, fetched as that wiki's `Special:Export` writes it, with `fullhistory` and
//! `templates` passed on (`import`). `interwikiprefix` names the prefix an uploaded
//! file's authors carry (`imported` by default); for a fetch it is the alias.
//!
//! **What happens.** The template census runs first (0042 §5): with `census` the
//! response carries it and nothing is written, which is how `Special:Import` shows it
//! before confirmation. Otherwise the job imports the file's document pages under the
//! tenant's own IDs, merging onto titles that exist by timestamp (0008 A48), refusing
//! entity pages, then converts its talk pages to threads (0090 §10). `summary` is the
//! job's reason. `namespace` and `rootpage` are accepted and warned about: the file's
//! titles are kept. A file of up to `import.sync_pages` pages is imported on the request
//! path; a larger one is queued for the runner (0104 §4), and the response names the job
//! it will run as, `queued`, whose page is `Special:Jobs/{id}`.
//!
//! **The response** is MediaWiki's: `import`, one entry per page with its namespace,
//! title and the revisions imported; beside it Triplespace's `job` (the ID, its status,
//! the counts, the talk conversion, what was left out and why, and its page) and, when
//! asked for, `census`.

use scatter_mediawiki_xml::{DumpPage, XmlDump};
use scatter_wikibase_model::sites::SiteRegistry;
use serde_json::{Value, json};
use triplespace_api_ingest::import::{ImportRequest, census, issuer_for, run_request};
use triplespace_api_ingest::jobs;

use crate::api::Ctx;
use crate::edit::now_micros;
use crate::pages::Site;
use crate::response::{ApiError, ApiResponse};

/// The registry default of `import.max_upload`, 100 MB (0047 A36): the most an upload
/// or a fetched export may be, in bytes, where the tenant's setting is unreadable.
pub const DEFAULT_MAX_UPLOAD: u64 = 100 * 1024 * 1024;

/// The registry default of `import.sync_pages` (0104 §4): an import of up to this many
/// pages runs on the request path; a larger one is queued for the runner.
pub const DEFAULT_SYNC_PAGES: u64 = 100;

/// The URL of a wiki's `Special:Export` of a title, through its site alias: the alias's
/// article path with `Special:Export/{title}`, and MediaWiki's `history` and `templates`
/// parameters.
#[must_use]
pub fn export_url(
    site: &scatter_wikibase_model::sites::Site,
    title: &str,
    history: bool,
    templates: bool,
) -> String {
    let mut url = site.url_for_title(&format!("Special:Export/{title}"));
    let mut query = Vec::new();
    if history {
        query.push("history=1");
    }
    if templates {
        query.push("templates=1");
    }
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    url
}

/// Fetches another wiki's export of a title.
async fn fetch(ctx: &Ctx, url: &str, max_bytes: usize) -> Result<Vec<u8>, ApiError> {
    let client = reqwest::Client::builder()
        .user_agent(&ctx.app.config().generator)
        .timeout(std::time::Duration::from_mins(2))
        .build()
        .map_err(ApiError::internal)?;
    let response = client.get(url).send().await.map_err(|e| {
        ApiError::new(
            "import-fetch-failed",
            format!("The export could not be fetched from {url}: {e}"),
        )
    })?;
    if !response.status().is_success() {
        return Err(ApiError::new(
            "import-fetch-failed",
            format!(
                "The export could not be fetched from {url}: HTTP {}",
                response.status()
            ),
        ));
    }
    let bytes = response.bytes().await.map_err(|e| {
        ApiError::new(
            "import-fetch-failed",
            format!("The export could not be read from {url}: {e}"),
        )
    })?;
    if bytes.len() > max_bytes {
        return Err(ApiError::new(
            "import-too-large",
            format!("The export is larger than {max_bytes} bytes (import.max_upload)."),
        ));
    }
    Ok(bytes.to_vec())
}

/// The file's pages.
fn read(bytes: &[u8]) -> Result<Vec<DumpPage>, ApiError> {
    let mut pages = Vec::new();
    for page in XmlDump::new(bytes) {
        pages.push(page.map_err(|e| {
            ApiError::new(
                "import-unreadable",
                format!("The file is not a MediaWiki XML export: {e}"),
            )
        })?);
    }
    Ok(pages)
}

/// `action=import`.
#[allow(clippy::too_many_lines)]
pub async fn run(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require("import")?;
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("import"))?;
    let census_only = ctx.params.flag("census");
    let history = ctx.params.flag("fullhistory");
    let templates = ctx.params.flag("templates");
    let summary = ctx.params.get("summary").map(str::to_string);
    let mut response = ApiResponse::ok(json!({}));
    for p in ["namespace", "rootpage"] {
        if ctx.params.get(p).is_some_and(|v| !v.is_empty()) {
            response.warn(
                "import",
                &format!("The {p} parameter is not supported; the file's titles are kept."),
            );
        }
    }

    // The wiki's upload limit (0047 A36).
    let (settings, _) =
        triplespace_accounts::settings::load(ctx.db(), Some(&ctx.tenant.slug)).await?;
    let max_bytes = usize::try_from(
        settings
            .value("import.max_upload")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_MAX_UPLOAD),
    )
    .unwrap_or(usize::MAX);

    // The source, and the registered issuer it is, if any (0008 A50): a fetched page's
    // by its alias and host; an upload's only where `interwikiprefix` names an issuer's
    // code outright, since the file itself is not to be trusted with real accounts.
    let (bytes, source, prefix, issuer) = if let Some(xml) = ctx.params.file("xml") {
        ctx.caller.require("importupload")?;
        if xml.len() > max_bytes {
            return Err(ApiError::new(
                "import-too-large",
                format!("The file is larger than {max_bytes} bytes (import.max_upload)."),
            ));
        }
        let prefix = ctx
            .params
            .get("interwikiprefix")
            .filter(|p| !p.trim().is_empty())
            .unwrap_or("imported")
            .trim()
            .to_string();
        let issuer = scatter_actors::issuer::IssuerRegistry::default_registry()
            .by_code(&prefix)
            .filter(|i| {
                !i.template
                    && !i.builtin
                    && i.actor_model == scatter_actors::issuer::ActorModel::Numeric
            })
            .map(|i| i.code.clone());
        (xml.to_vec(), "upload".to_string(), prefix, issuer)
    } else if let Some(alias) = ctx.params.get("interwikisource") {
        let alias = alias.trim().to_string();
        let title = ctx.params.required("interwikipage")?.trim().to_string();
        if title.is_empty() {
            return Err(ApiError::missing_param("interwikipage"));
        }
        let Some(site) = SiteRegistry::default_registry().by_id(&alias) else {
            return Err(ApiError::new(
                "import-unknownsource",
                format!("`{alias}` is not a site alias this wiki knows."),
            ));
        };
        let url = export_url(site, &title, history, templates);
        let issuer = issuer_for(&alias, Some(&site.host));
        (
            fetch(ctx, &url, max_bytes).await?,
            alias.clone(),
            alias,
            issuer,
        )
    } else {
        return Err(ApiError::new(
            "import-nosource",
            "Pass an `xml` file or `interwikisource` with `interwikipage`.",
        ));
    };
    let pages = read(&bytes)?;

    // The census (10 §6).
    let site = Site::load(ctx).await?;
    let c = census(ctx.db(), &ctx.tenant.slug, pages.iter())
        .await
        .map_err(ApiError::internal)?;
    ctx.cache_class = crate::http_cache::Class::Private;
    if census_only {
        response.body.insert("import".into(), json!([]));
        response.body.insert(
            "census".into(),
            serde_json::to_value(&c).map_err(ApiError::internal)?,
        );
        return Ok(response);
    }
    if c.imported == 0 && c.talk == 0 {
        return Err(ApiError::new(
            "import-nothing",
            "The file holds no page this wiki can import.",
        ));
    }

    // The job (10 §6 steps 1–3): on the request path up to `import.sync_pages` pages,
    // else queued for the runner, which answers with the job's ID (0104 §4).
    let request = ImportRequest {
        source: source.clone(),
        version: None,
        prefix,
        issuer: issuer.clone(),
        summary,
        tags: ctx.params.list("tags"),
        skip_talk: false,
    };
    let sync_pages = settings
        .value("import.sync_pages")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_SYNC_PAGES);
    if c.imported + c.talk > sync_pages {
        let id = jobs::submit(
            ctx.app.store(),
            &jobs::Submission {
                tenant: ctx.tenant.slug.clone(),
                kind: jobs::Kind::Import,
                params: serde_json::to_value(&request).map_err(ApiError::internal)?,
                payload: Some(bytes),
                actor: actor.clone(),
            },
        )
        .await
        .map_err(ApiError::internal)?;
        response.body.insert("import".into(), json!([]));
        response.body.insert(
            "job".into(),
            json!({
                "id": id,
                "status": "queued",
                "issuer": issuer,
                "pages": c.imported + c.talk,
                "page": format!("Special:Jobs/{id}"),
            }),
        );
        return Ok(response);
    }
    let store = ctx.app.store();
    let pipeline = ctx.app.pipeline();
    let report = run_request(
        store,
        pipeline,
        ctx.db(),
        &ctx.tenant.slug,
        &actor,
        &request,
        None,
        pages,
        now_micros(),
        &mut |_| {},
    )
    .await
    .map_err(ApiError::internal)?;
    let (outcome, read, talk) = (report.outcome, report.read, report.talk);

    let imported: Vec<Value> = outcome
        .pages
        .iter()
        .map(|p| {
            json!({
                "ns": p.ns,
                "title": site.prefixed(p.ns, &p.title),
                "revisions": p.revisions,
                "present": p.present,
                "created": p.created,
                "restored": p.restored,
                "pageid": p.page_id,
            })
        })
        .collect();
    response
        .body
        .insert("import".into(), Value::Array(imported));
    response.body.insert(
        "job".into(),
        json!({
            "id": outcome.job_id,
            "status": "finished",
            "issuer": issuer,
            "counts": outcome.counts,
            "talk": talk,
            "entities": read.entities,
            "left_out": read.left_out,
            "rejects": outcome.rejects.iter().map(|r| json!({"line": r.line, "reason": r.reason})).collect::<Vec<_>>(),
            "page": format!("Special:Jobs/{}", outcome.job_id),
        }),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_export_url_follows_the_alias() {
        let site = SiteRegistry::default_registry()
            .by_id("enwiki")
            .expect("enwiki is in sites.toml");
        assert_eq!(
            export_url(site, "Douglas Adams", true, true),
            "https://en.wikipedia.org/wiki/Special:Export/Douglas_Adams?history=1&templates=1"
        );
        assert_eq!(
            export_url(site, "Talk:X", false, false),
            "https://en.wikipedia.org/wiki/Special:Export/Talk:X"
        );
    }
}
