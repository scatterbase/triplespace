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

use crate::Site;
use crate::codex::{
    Action, Button, ButtonModel, FieldModel, Message, MessageKind, MessageModel, Weight,
};
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::moderation::GateModel;
use crate::pages::{self, Context, Params, Peer};

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
async fn page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    back: Returned,
) -> Response {
    let heading = cx.m.get("ts-tenants-title");
    if !cx.site.at_farm {
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
    let m = &cx.m;
    let rows = tenancy
        .tenants
        .iter()
        .map(|t| TenantRowModel {
            name: t.name.clone(),
            slug: t.slug.clone(),
            host: t.host.clone().unwrap_or_default(),
            base: t.base.clone(),
            issuer: t.issuer.clone(),
            primary: tenancy.primary.as_deref() == Some(t.slug.as_str()),
        })
        .collect();
    let mut model = TenantsPage {
        notices: back
            .notices
            .iter()
            .map(|(k, t)| Message::block(*k, t).model())
            .collect(),
        intro: m.get("ts-tenants-intro"),
        columns: [
            "ts-tenants-col-name",
            "ts-tenants-col-slug",
            "ts-tenants-col-host",
            "ts-tenants-col-issuer",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        rows,
        primary_label: m.get("ts-tenants-primary"),
        form: None,
        requests: None,
    };
    let mut cookies = Vec::new();
    if cx.user.is_some() && cx.holds(RIGHT) {
        let token = match site.client().token(incoming, "csrf").await {
            Ok(t) => t,
            Err(e) => return pages::api_failed(cx, headers, &e),
        };
        cx.inputs.push(token.value.cache.clone());
        cookies = token.set_cookies.clone();
        let token = token.value.value;
        let requests = match site.client().tenant_requests(incoming).await {
            Ok(r) => r,
            Err(e) => return pages::api_failed(cx, headers, &e),
        };
        model.form = Some(create_form(&cx.m, &token, &back));
        model.requests = Some(requests_model(&cx.m, &requests, &token));
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
