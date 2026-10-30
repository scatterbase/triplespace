//! `list=querypage` (0047 §11; compat §3.3): a report's rows in MediaWiki's shape.
//! `qppage` takes the report's MediaWiki name (`Wantedpages`) or its registry name
//! (`WantedPages`, `ListProperties`); `qpoffset` continues; `qplimit` is 10 by default,
//! at most 500. Besides MediaWiki's `name` and `results`, the answer carries the report's
//! state as `ts-state` (backing, mode, when built, work waiting), and each row Triplespace's
//! extra fields (`ts-target`, `ts-final`, `ts-label`; [`crate::reports::row_json`]).

use serde_json::{Map, Value, json};

use crate::api::Ctx;
use crate::pages::Site;
use crate::response::ApiError;

const DEFAULT_LIMIT: i64 = 10;

/// `list=querypage`.
pub async fn querypage(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let name = ctx.params.required("qppage")?;
    let report = crate::reports::named(name)?;
    let offset = match ctx.params.get("qpoffset") {
        None => 0,
        Some(v) => v
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .ok_or_else(|| ApiError::bad_value("qpoffset", v))?,
    };
    let limit = match ctx.params.get("qplimit") {
        None => DEFAULT_LIMIT,
        Some("max") => crate::reports::MAX_LIMIT,
        Some(v) => v
            .parse::<i64>()
            .map_err(|_| ApiError::bad_value("qplimit", v))?
            .clamp(1, crate::reports::MAX_LIMIT),
    };
    let site = Site::load(ctx).await?;
    let lang = ctx
        .params
        .get("uselang")
        .map_or_else(|| site.lang.clone(), str::to_string);
    let page = crate::reports::page(
        &ctx.app,
        ctx.db(),
        &ctx.tenant.slug,
        &ctx.caller.effective,
        report,
        &crate::reports::Window {
            offset,
            limit,
            lang,
        },
    )
    .await?;
    let results: Vec<Value> = page
        .rows
        .iter()
        .map(|r| crate::reports::row_json(&site, r))
        .collect();
    query.insert(
        "querypage".into(),
        json!({
            "name": report.mediawiki_name.unwrap_or(report.name),
            "results": results,
            "ts-state": crate::reports::state_json(report, &page.state),
        }),
    );
    if page.more {
        body.insert(
            "continue".into(),
            json!({"qpoffset": offset + limit, "continue": "-||"}),
        );
    }
    Ok(())
}
