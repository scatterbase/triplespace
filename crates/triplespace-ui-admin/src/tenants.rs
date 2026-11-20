//! `Special:Tenants` (0046 §9; 0111 §7; 19 §6.11): the instance's wikis, at the farm base,
//! from `GET /tenancy`: each one's name, slug, host and issuer code, the primary tenant
//! marked.
//!
//! For a holder of `ts-createtenant`, below the list:
//!
//! - **Create a wiki…**, a form of a tenant request's fields: the slug, the base, the wiki's
//!   name, the owner's account name, and **The owner signs in with my login**, which names
//!   the viewer's own account as the owner login source (0111 §5). It posts
//!   `POST /tenancy/requests`; the provisioner carries the request out. The page does not
//!   check that the base's DNS and proxy are in place: the operator sets them up first.
//! - **Requests**, newest first, from `GET /tenancy/requests`: each with its state,
//!   `pending`, `created`, or `refused` with the reason; a created wiki whose request named
//!   no login offers **Invite the owner**, which asks for the owner's invitation and shows
//!   its address once, on the page that answers the post (0111 §5, A2).

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use std::collections::BTreeMap;
use triplespace_client::settings::Saved;
use triplespace_client::{Incoming, Invitation, TenantAsk, TenantRequest};

use triplespace_ui_kit::Site;
use triplespace_ui_kit::codex::{
    Action, Button, ButtonModel, FieldModel, Message, MessageKind, MessageModel, Weight,
};
use triplespace_ui_kit::frame::{Page, title_url};
use triplespace_ui_kit::i18n::Messages;
use triplespace_ui_kit::pages::{self, Context, Params, Peer};
use triplespace_ui_pages::moderation::GateModel;

const TITLE: &str = "Special:Tenants";

/// The right that files a request and reads the requests (0111 §8).
pub const RIGHT: &str = "ts-createtenant";

/// `Special:Tenants` as Plain's `special/tenants.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TenantsPage {
    /// Notices: a filed request, a refusal, an invitation's address.
    pub notices: Vec<MessageModel>,
    /// What the page lists.
    pub intro: String,
    /// The column headings.
    pub columns: Vec<String>,
    /// One row per wiki.
    pub rows: Vec<TenantRowModel>,
    /// The primary tenant's mark.
    pub primary_label: String,
    /// The form, for a holder of the right.
    pub form: Option<CreateForm>,
    /// The requests, for a holder of the right.
    pub requests: Option<RequestsModel>,
    /// Where the row buttons post.
    pub action: String,
    /// The `csrf` token the row buttons carry, for a holder of a right they need.
    pub token: Option<String>,
    /// **Offer the primary role**, for a holder of `ts-primary`.
    pub offer: Option<OfferModel>,
}

/// A button on a wiki's row that files a request for an act (0111 §10).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RowAction {
    /// The act.
    pub act: String,
    /// The button's label.
    pub label: String,
}

/// **Offer the primary role** (0046 §5; 08 §7.5).
#[derive(Debug, Clone, serde::Serialize)]
pub struct OfferModel {
    /// The heading.
    pub heading: String,
    /// What it does.
    pub intro: String,
    /// The wikis it may be offered to: slug and name.
    pub choices: Vec<(String, String)>,
    /// The button.
    pub submit: ButtonModel,
    /// The open offers.
    pub open: Vec<OpenOffer>,
}

/// An open offer, with **Withdraw**.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OpenOffer {
    /// Its ID.
    pub id: u64,
    /// What it says: to whom, until when.
    pub text: String,
    /// **Withdraw**.
    pub withdraw: String,
}

/// On the offered wiki, for a member of its `owner` group: **Accept the primary role**.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AcceptPage {
    /// What accepting means.
    pub intro: String,
    /// Where it posts.
    pub action: String,
    /// The `csrf` token.
    pub token: String,
    /// The offers naming this wiki.
    pub offers: Vec<OpenOffer>,
    /// **Accept the primary role**.
    pub submit: ButtonModel,
}

/// One wiki.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TenantRowModel {
    /// Its name.
    pub name: String,
    /// Its slug.
    pub slug: String,
    /// Its host.
    pub host: String,
    /// Its base, for the link.
    pub base: String,
    /// Its issuer code.
    pub issuer: String,
    /// Whether it is the primary tenant.
    pub primary: bool,
    /// Its status as the viewer reads it.
    pub status: String,
    /// The acts a holder of the right may ask for on it.
    pub actions: Vec<RowAction>,
}

/// A date, `YYYY-MM-DD`, from microseconds since the epoch.
fn date(micros: u64) -> String {
    // Days to a civil date (Howard Hinnant's algorithm).
    let days = i64::try_from(micros / 86_400_000_000).unwrap_or(0) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn status_label(m: &Messages, status: &str, erase_after: Option<u64>) -> String {
    match (status, erase_after) {
        ("deleted", Some(e)) => m.with("ts-tenants-status-deleted", &[&date(e)]),
        ("frozen" | "deleted", _) => m.get(&format!("ts-tenants-status-{status}")),
        _ => m.get("ts-tenants-status-active"),
    }
}

fn row_actions(m: &Messages, status: &str, primary: bool) -> Vec<RowAction> {
    if primary {
        return Vec::new();
    }
    let acts: &[&str] = match status {
        "active" => &["freeze", "delete"],
        "frozen" => &["unfreeze", "delete"],
        "deleted" => &["undelete"],
        _ => &[],
    };
    acts.iter()
        .map(|a| RowAction {
            act: (*a).to_string(),
            label: m.get(&format!("ts-tenants-act-{a}")),
        })
        .collect()
}

/// **Create a wiki…**.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CreateForm {
    /// The heading.
    pub heading: String,
    /// What it does, and what it does not check.
    pub intro: MessageModel,
    /// Where it posts.
    pub action: String,
    /// The `csrf` token.
    pub token: String,
    /// The slug.
    pub slug: FieldModel,
    /// The base.
    pub base: FieldModel,
    /// The wiki's name.
    pub name: FieldModel,
    /// The owner's account name.
    pub owner_name: FieldModel,
    /// **The owner signs in with my login**.
    pub own_login: String,
    /// What it means.
    pub own_login_help: String,
    /// Whether it is ticked.
    pub own_login_checked: bool,
    /// The reason.
    pub reason: FieldModel,
    /// **Request the wiki**.
    pub submit: ButtonModel,
}

/// **Requests**.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RequestsModel {
    /// The heading.
    pub heading: String,
    /// Said where there are none.
    pub nothing: Option<String>,
    /// The column headings.
    pub columns: Vec<String>,
    /// One row per request, newest first.
    pub rows: Vec<RequestRow>,
}

/// One request.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RequestRow {
    /// Its slug.
    pub slug: String,
    /// Its base.
    pub base: String,
    /// `pending`, `created` or `refused`, for the class.
    pub state: String,
    /// The state as the viewer reads it.
    pub state_label: String,
    /// The refusal's reason, or who filed it and why.
    pub detail: String,
    /// **Invite the owner**, where it applies.
    pub invite: Option<InviteForm>,
}

/// **Invite the owner**.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InviteForm {
    /// Where it posts.
    pub action: String,
    /// The `csrf` token.
    pub token: String,
    /// The request's ID.
    pub id: u64,
    /// The button.
    pub submit: ButtonModel,
}

/// What the form shows when it comes back.
#[derive(Debug, Default)]
struct Returned {
    notices: Vec<(MessageKind, String)>,
    slug: String,
    base: String,
    name: String,
    owner_name: String,
    own_login: bool,
    reason: String,
}

fn field(m: &Messages, name: &str, value: &str, required: bool) -> FieldModel {
    FieldModel {
        id: format!("ts-tenants-{name}"),
        name: name.into(),
        kind: "text".into(),
        value: value.into(),
        label: m.get(&format!("ts-tenants-{name}")),
        description: None,
        help: Some(m.get(&format!("ts-tenants-{name}-help"))),
        required,
        autocomplete: Some("off".into()),
    }
}

fn button(label: &str, weight: Weight) -> ButtonModel {
    Button {
        label,
        kind: "submit",
        action: Action::Progressive,
        weight,
    }
    .model()
}

fn action() -> String {
    format!("/w/index.php?title={}", title_url(TITLE))
}

fn request_row(m: &Messages, r: &TenantRequest, token: &str) -> RequestRow {
    let state_label = m.get(&format!("ts-tenants-state-{}", r.state));
    let detail = match (&r.state[..], &r.refusal) {
        ("refused", Some(refusal)) => {
            let why = refusal.why.clone().unwrap_or_default();
            if why.is_empty() {
                refusal.code.clone()
            } else {
                format!("{}: {why}", refusal.code)
            }
        }
        _ => m.with(
            "ts-tenants-requested-by",
            &[
                r.actor.as_deref().unwrap_or("?"),
                r.reason.as_deref().unwrap_or(""),
            ],
        ),
    };
    let invite = (r.state == "created" && r.owner_login_source.is_none()).then(|| InviteForm {
        action: action(),
        token: token.to_string(),
        id: r.id,
        submit: button(&m.get("ts-tenants-invite"), Weight::Normal),
    });
    RequestRow {
        slug: r.slug.clone(),
        base: r.base.clone(),
        state: r.state.clone(),
        state_label,
        detail,
        invite,
    }
}

fn create_form(m: &Messages, token: &str, back: &Returned) -> CreateForm {
    CreateForm {
        heading: m.get("ts-tenants-create-heading"),
        intro: Message::block(MessageKind::Notice, &m.get("ts-tenants-create-intro")).model(),
        action: action(),
        token: token.to_string(),
        slug: field(m, "slug", &back.slug, true),
        base: field(m, "base", &back.base, true),
        name: field(m, "name", &back.name, false),
        owner_name: field(m, "owner_name", &back.owner_name, true),
        own_login: m.get("ts-tenants-own-login"),
        own_login_help: m.get("ts-tenants-own-login-help"),
        own_login_checked: back.own_login,
        reason: field(m, "reason", &back.reason, false),
        submit: button(&m.get("ts-tenants-submit"), Weight::Primary),
    }
}

fn requests_model(m: &Messages, requests: &[TenantRequest], token: &str) -> RequestsModel {
    RequestsModel {
        heading: m.get("ts-tenants-requests-heading"),
        nothing: requests
            .is_empty()
            .then(|| m.get("ts-tenants-requests-none")),
        columns: [
            "ts-tenants-requests-col-wiki",
            "ts-tenants-requests-col-state",
            "ts-tenants-requests-col-detail",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        rows: requests.iter().map(|r| request_row(m, r, token)).collect(),
    }
}

/// The page: the list for everyone, the form and the requests for a holder of the right.
/// The list's columns, as message keys.
const COLUMNS: [&str; 5] = [
    "ts-tenants-col-name",
    "ts-tenants-col-slug",
    "ts-tenants-col-host",
    "ts-tenants-col-issuer",
    "ts-tenants-col-status",
];

async fn page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    back: Returned,
) -> Response {
    let heading = cx.m.get("ts-tenants-title");
    if !cx.site.at_farm {
        if let Some(r) = accept_page(site, cx, headers, incoming, &back).await {
            return r;
        }
        let text = cx.m.get("ts-tenants-farm-only");
        let body = cx.draw(
            "moderation/gate.html",
            GateModel::notice(MessageKind::Notice, &text),
        );
        return pages::respond(cx, headers, StatusCode::OK, |_| Page {
            title: heading.clone(),
            returnto: Some(TITLE.to_string()),
            body,
            ..Page::default()
        });
    }
    let tenancy = match site.client().tenancy(incoming).await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    cx.inputs.push(tenancy.cache.clone());
    let tenancy = tenancy.value;
    let lifecycle = cx.user.is_some() && cx.holds(RIGHT);
    let offers_role = cx.user.is_some() && cx.holds("ts-primary");
    let m = &cx.m;
    let rows = tenancy
        .tenants
        .iter()
        .map(|t| {
            let primary = tenancy.primary.as_deref() == Some(t.slug.as_str());
            TenantRowModel {
                name: t.name.clone(),
                slug: t.slug.clone(),
                host: t.host.clone().unwrap_or_default(),
                base: t.base.clone(),
                issuer: t.issuer.clone(),
                primary,
                status: status_label(m, &t.status, t.erase_after),
                actions: if lifecycle {
                    row_actions(m, &t.status, primary)
                } else {
                    Vec::new()
                },
            }
        })
        .collect();
    let mut model = TenantsPage {
        notices: back
            .notices
            .iter()
            .map(|(k, t)| Message::block(*k, t).model())
            .collect(),
        intro: m.get("ts-tenants-intro"),
        columns: COLUMNS.iter().map(|k| m.get(k)).collect(),
        rows,
        primary_label: m.get("ts-tenants-primary"),
        form: None,
        requests: None,
        action: action(),
        token: None,
        offer: None,
    };
    let mut cookies = Vec::new();
    if lifecycle || offers_role {
        let token = match site.client().token(incoming, "csrf").await {
            Ok(t) => t,
            Err(e) => return pages::api_failed(cx, headers, &e),
        };
        cx.inputs.push(token.value.cache.clone());
        model.token = Some(token.value.value);
        cookies = token.set_cookies;
    }
    let token = model.token.clone().unwrap_or_default();
    if lifecycle {
        let requests = match site.client().tenant_requests(incoming).await {
            Ok(r) => r,
            Err(e) => return pages::api_failed(cx, headers, &e),
        };
        model.form = Some(create_form(&cx.m, &token, &back));
        model.requests = Some(requests_model(&cx.m, &requests, &token));
    }
    if offers_role {
        model.offer = Some(offer_model(&cx.m, &tenancy));
    }
    let body = cx.draw("special/tenants.html", &model);
    let r = pages::respond(cx, headers, StatusCode::OK, |_| Page {
        title: heading.clone(),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    });
    pages::with_cookies(r, &cookies)
}

fn offer_model(m: &Messages, tenancy: &triplespace_client::Tenancy) -> OfferModel {
    OfferModel {
        heading: m.get("ts-tenants-offer-heading"),
        intro: m.get("ts-tenants-offer-intro"),
        choices: tenancy
            .tenants
            .iter()
            .filter(|t| t.status == "active" && tenancy.primary.as_deref() != Some(t.slug.as_str()))
            .map(|t| (t.slug.clone(), t.name.clone()))
            .collect(),
        submit: button(&m.get("ts-tenants-offer-submit"), Weight::Normal),
        open: tenancy
            .offers
            .iter()
            .map(|o| OpenOffer {
                id: o.id,
                text: m.with("ts-tenants-offer-open", &[&o.tenant, &date(o.expires)]),
                withdraw: m.get("ts-tenants-offer-withdraw"),
            })
            .collect(),
    }
}

/// On the offered wiki, for a member of its `owner` group with an offer open: the page that
/// accepts the primary role. `None` where there is nothing to accept.
async fn accept_page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    back: &Returned,
) -> Option<Response> {
    if cx.user.is_none() || !cx.groups.iter().any(|g| g == "owner") {
        return None;
    }
    let offers = site.client().offers_here(incoming).await.ok()?;
    if offers.is_empty() && back.notices.is_empty() {
        return None;
    }
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return Some(pages::api_failed(cx, headers, &e)),
    };
    let m = &cx.m;
    let model = AcceptPage {
        intro: m.get("ts-tenants-accept-intro"),
        action: action(),
        token: token.value.value.clone(),
        offers: offers
            .iter()
            .map(|o| OpenOffer {
                id: o.id,
                text: m.with(
                    "ts-tenants-accept-offer",
                    &[o.actor.as_deref().unwrap_or("?"), &date(o.expires)],
                ),
                withdraw: String::new(),
            })
            .collect(),
        submit: button(&m.get("ts-tenants-accept-submit"), Weight::Primary),
    };
    let notices: Vec<MessageModel> = back
        .notices
        .iter()
        .map(|(k, t)| Message::block(*k, t).model())
        .collect();
    let body = cx.draw(
        "special/tenants-accept.html",
        serde_json::json!({"accept": model, "notices": notices}),
    );
    let heading = cx.m.get("ts-tenants-accept-title");
    let r = pages::respond(cx, headers, StatusCode::OK, |_| Page {
        title: heading.clone(),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    });
    Some(pages::with_cookies(r, &token.set_cookies))
}

/// `GET Special:Tenants`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let mut back = Returned::default();
    if let Some(slug) = params.map.get("filed") {
        back.notices
            .push((MessageKind::Success, cx.m.with("ts-tenants-filed", &[slug])));
    }
    page(site, &mut cx, headers, &incoming, back).await
}

fn refusal_text(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "permissiondenied" => m.get("ts-tenants-denied"),
        "badtoken" => m.get("ts-login-expired"),
        _ if info.is_empty() => code.to_string(),
        _ => info.to_string(),
    }
}

/// `POST Special:Tenants`: files a request, or asks for an owner's invitation.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let field = |k: &str| form.get(k).map_or("", String::as_str).trim().to_string();
    let token = field("token");
    if let Ok(id) = field("invite").parse::<u64>() {
        let notice = match site
            .client()
            .invite_tenant_owner(&incoming, id, &token)
            .await
        {
            Ok(Saved::Ok(Invitation {
                url,
                tenant,
                valid_days,
                ..
            })) => (
                MessageKind::Success,
                cx.m.with(
                    "ts-tenants-invited",
                    &[&tenant, &valid_days.to_string(), &url],
                ),
            ),
            Ok(Saved::Refused(r)) => {
                let first = r.first();
                (
                    MessageKind::Error,
                    refusal_text(
                        &cx.m,
                        first.map_or("", |x| x.code.as_str()),
                        first.map_or("", |x| x.message.as_str()),
                    ),
                )
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        };
        let back = Returned {
            notices: vec![notice],
            ..Returned::default()
        };
        return page(site, &mut cx, headers, &incoming, back).await;
    }
    if let Some(r) = post_act(site, &mut cx, headers, &incoming, form).await {
        return r;
    }
    let own_login = form.contains_key("own_login");
    let owner_login_source = if own_login {
        match site.client().userinfo(&incoming).await {
            Ok(u) if !u.value.anon => format!("{}:{}", cx.site.issuer, u.value.id),
            Ok(_) => String::new(),
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    } else {
        String::new()
    };
    let ask = TenantAsk {
        slug: field("slug"),
        base: field("base"),
        name: field("name"),
        owner_name: field("owner_name"),
        owner_login_source,
        reason: field("reason"),
        token,
        act: String::new(),
    };
    match site.client().file_tenant_request(&incoming, &ask).await {
        Ok(Saved::Ok(r)) => {
            pages::see_other(&format!("/wiki/{TITLE}?filed={}", title_url(&r.slug)), &[])
        }
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let back = Returned {
                notices: vec![(
                    MessageKind::Error,
                    refusal_text(
                        &cx.m,
                        first.map_or("", |x| x.code.as_str()),
                        first.map_or("", |x| x.message.as_str()),
                    ),
                )],
                slug: ask.slug,
                base: ask.base,
                name: ask.name,
                owner_name: ask.owner_name,
                own_login,
                reason: ask.reason,
            };
            page(site, &mut cx, headers, &incoming, back).await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

fn saved_notice(
    m: &Messages,
    saved: Saved<serde_json::Value>,
    done: String,
) -> (MessageKind, String) {
    match saved {
        Saved::Ok(_) => (MessageKind::Success, done),
        Saved::Refused(r) => {
            let first = r.first();
            (
                MessageKind::Error,
                refusal_text(
                    m,
                    first.map_or("", |x| x.code.as_str()),
                    first.map_or("", |x| x.message.as_str()),
                ),
            )
        }
    }
}

/// The posts of the row buttons, the offer and the acceptance (0111 §10; 0046 §5); `None`
/// for the creation form's.
async fn post_act(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    form: &BTreeMap<String, String>,
) -> Option<Response> {
    let field = |k: &str| form.get(k).map_or("", String::as_str).trim().to_string();
    let token = field("token");
    let client = site.client();
    let result = if let Some(act) = form.get("act").filter(|a| !a.is_empty()) {
        let slug = field("slug");
        let ask = TenantAsk {
            slug: slug.clone(),
            token,
            act: act.clone(),
            ..TenantAsk::default()
        };
        client
            .file_tenant_request(incoming, &ask)
            .await
            .map(|s| match s {
                Saved::Ok(_) => Saved::Ok(serde_json::Value::Null),
                Saved::Refused(r) => Saved::Refused(r),
            })
            .map(|s| saved_notice(&cx.m, s, cx.m.with("ts-tenants-act-filed", &[act, &slug])))
    } else if let Some(to) = form.get("offer_to").filter(|s| !s.is_empty()) {
        client
            .offer_primary(incoming, to, &token)
            .await
            .map(|s| saved_notice(&cx.m, s, cx.m.with("ts-tenants-offer-made", &[to])))
    } else if let Ok(id) = field("withdraw").parse::<u64>() {
        client
            .withdraw_primary_offer(incoming, id, &token)
            .await
            .map(|s| saved_notice(&cx.m, s, cx.m.get("ts-tenants-offer-withdrawn")))
    } else if let Ok(id) = field("accept").parse::<u64>() {
        client
            .accept_primary(incoming, id, &token)
            .await
            .map(|s| saved_notice(&cx.m, s, cx.m.get("ts-tenants-accept-filed")))
    } else {
        return None;
    };
    let notice = match result {
        Ok(n) => n,
        Err(e) => return Some(pages::api_failed(cx, headers, &e)),
    };
    let back = Returned {
        notices: vec![notice],
        ..Returned::default()
    };
    Some(page(site, cx, headers, incoming, back).await)
}
