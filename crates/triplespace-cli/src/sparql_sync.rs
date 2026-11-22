//! `triplespace sparql-sync`: the reference consumer of the SPARQL Update stream
//! (02 §6.4; 0032 §6). Run by whoever runs the triplestore, it polls an instance's
//! `GET /updates` from the cursor in its state file, POSTs each document to the store's
//! update endpoint as `application/sparql-update` with the store's own credential, and
//! commits the new cursor to the state file once the store has acknowledged. The instance
//! never holds the store's credential, and nothing here is specific to one store.
//!
//! With each batch it writes the marker triple
//! `<{source}/.well-known/query> scatter:cursor "{epoch}:{seq}"` in the store's default
//! graph, in the same request, so that the store itself says how far it has been brought
//! (0032 A7). On `410 Gone` it stops and names where the dumps are: the store is reloaded
//! from the newest, and the state file set to that dump's cursor.
//!
//! **Not yet:** loading the dump on the first run; following `GET /updates/stream`
//! rather than polling.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use clap::Args;

/// The predicate of the marker triple.
pub const CURSOR: &str = "https://scatter.red/terms/v0/cursor";

/// `sparql-sync`.
#[derive(Debug, Clone, Args)]
pub struct SparqlSync {
    /// The instance's base, `https://librarybase.org`.
    #[arg(long)]
    pub source: String,
    /// The graphs to follow: `resolved`, `local`, or both separated by a comma.
    #[arg(long, default_value = "resolved")]
    pub graphs: String,
    /// The store's SPARQL 1.1 Update endpoint.
    #[arg(long)]
    pub endpoint: String,
    /// A file holding the store's access token, sent as a bearer token.
    #[arg(long)]
    pub token_file: Option<PathBuf>,
    /// Where the cursor is kept between runs.
    #[arg(long)]
    pub state: PathBuf,
    /// The cursor to start from when the state file does not exist: that of the dump the
    /// store was loaded from, `{epoch}:{seq}`.
    #[arg(long)]
    pub from: Option<String>,
    /// The most deltas to ask for at a time.
    #[arg(long, default_value_t = 1000)]
    pub limit: u32,
    /// Seconds to wait before asking again once caught up.
    #[arg(long, default_value_t = 5)]
    pub interval: u64,
    /// Stop once caught up, rather than following.
    #[arg(long)]
    pub once: bool,
}

/// The marker triple's update: the old cursor out, the new one in.
#[must_use]
pub fn marker(source: &str, cursor: &str) -> String {
    let node = format!("{}/.well-known/query", source.trim_end_matches('/'));
    format!(
        "DELETE WHERE {{ <{node}> <{CURSOR}> ?cursor }} ;\n\
         INSERT DATA {{ <{node}> <{CURSOR}> \"{cursor}\" }}"
    )
}

/// Runs `triplespace sparql-sync`. Returns the cursor it stopped at.
///
/// # Errors
///
/// No cursor to start from, a refused cursor (`410`), or a failure of the instance, the
/// store or the state file; the state file then holds the last cursor the store
/// acknowledged.
pub async fn run(args: SparqlSync) -> Result<String> {
    let mut cursor = match std::fs::read_to_string(&args.state) {
        Ok(s) => s.trim().to_string(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => args.from.clone().context(
            "no state file yet: pass --from with the cursor of the dump the store was loaded from",
        )?,
        Err(e) => return Err(e).with_context(|| args.state.display().to_string()),
    };
    let token = match &args.token_file {
        Some(f) => Some(
            std::fs::read_to_string(f)
                .with_context(|| f.display().to_string())?
                .trim()
                .to_string(),
        ),
        None => None,
    };
    let http = reqwest::Client::builder()
        .timeout(Duration::from_mins(5))
        .user_agent(concat!(
            "triplespace-sparql-sync/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()?;
    let source = args.source.trim_end_matches('/');
    let updates = format!("{source}/rest.php/triplespace/v0/updates");
    loop {
        let response = http
            .get(&updates)
            .query(&[
                ("graphs", args.graphs.as_str()),
                ("from", cursor.as_str()),
                ("limit", &args.limit.to_string()),
            ])
            .send()
            .await
            .with_context(|| updates.clone())?;
        let status = response.status();
        if status == reqwest::StatusCode::GONE {
            let body: serde_json::Value = response.json().await.unwrap_or_default();
            bail!(
                "the stream cannot be resumed from {cursor}: reload the store from the newest dump at {} and start again from that dump's cursor",
                body["dumps"].as_str().unwrap_or("the instance's /dumps/")
            );
        }
        if !status.is_success() {
            bail!(
                "{updates}: {status}: {}",
                response.text().await.unwrap_or_default()
            );
        }
        let next = response
            .headers()
            .get("updates-cursor")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .context("the instance sent no Updates-Cursor header")?;
        let document = response.text().await?;
        if document.trim().is_empty() {
            if args.once {
                return Ok(cursor);
            }
            tokio::time::sleep(Duration::from_secs(args.interval)).await;
            continue;
        }
        // The document ends each request with `;`, so the marker's follows as one more.
        let body = format!("{document}{}\n", marker(source, &next));
        let mut post = http
            .post(&args.endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/sparql-update")
            .body(body);
        if let Some(t) = &token {
            post = post.bearer_auth(t);
        }
        let answer = post.send().await.with_context(|| args.endpoint.clone())?;
        if !answer.status().is_success() {
            bail!(
                "{}: {}: {}",
                args.endpoint,
                answer.status(),
                answer.text().await.unwrap_or_default()
            );
        }
        // Acknowledged: the cursor is committed, whole or not at all.
        let tmp = args.state.with_extension("tmp");
        std::fs::write(&tmp, format!("{next}\n")).with_context(|| tmp.display().to_string())?;
        std::fs::rename(&tmp, &args.state).with_context(|| args.state.display().to_string())?;
        println!("applied through {next}");
        cursor = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0032 A7: the marker replaces the store's cursor in the default graph.
    #[test]
    fn the_marker_replaces_the_cursor() {
        assert_eq!(
            marker("https://librarybase.org/", "3:41"),
            "DELETE WHERE { <https://librarybase.org/.well-known/query> <https://scatter.red/terms/v0/cursor> ?cursor } ;\n\
             INSERT DATA { <https://librarybase.org/.well-known/query> <https://scatter.red/terms/v0/cursor> \"3:41\" }"
        );
    }
}
