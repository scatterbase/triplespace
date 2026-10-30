//! Wikibase's term forms (0047 §5; 21 §3.1): `Special:SetLabel`, `Special:SetDescription`,
//! `Special:SetAliases` and `Special:SetLabelDescriptionAliases`, server-rendered and
//! usable without JavaScript, at Wikibase's addresses: `/{id}/{language}` or `id=` and
//! `language=`, with `value=` (or `label=`, `description=` and `aliases=`, separated by
//! `|`) to prefill.
//!
//! A `GET` with an entity shows its current terms in the language, filled in; the form
//! posts back here, and the page writes through the same modules as every other client
//! (`wbsetlabel`, `wbsetdescription`, `wbsetaliases`; one `wbeditentity` for all three, so
//! one revision), then goes on to the entity's page. The API's refusal is shown above the
//! form, with what was typed kept. A keyed entity's label is its key (0009 §5), so its
//! label is not offered.

use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::Value;
use triplespace_client::{CacheInfo, ClientError, Incoming, TermEdit};

use crate::Site;
use crate::codex::{Action, Button, MessageKind, Weight};
use crate::entity;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::html::{esc, link};
use crate::i18n::{self, Messages};
use crate::pages::{self, Context, Params, Peer};

/// The four forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// `Special:SetLabel`.
    Label,
    /// `Special:SetDescription`.
    Description,
    /// `Special:SetAliases`.
    Aliases,
    /// `Special:SetLabelDescriptionAliases`.
    All,
}

impl Which {
    /// The form a special page's name names.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        Some(match name {
            "SetLabel" => Self::Label,
            "SetDescription" => Self::Description,
            "SetAliases" => Self::Aliases,
            "SetLabelDescriptionAliases" => Self::All,
            _ => return None,
        })
    }

    fn title(self) -> &'static str {
        match self {
            Self::Label => "Special:SetLabel",
            Self::Description => "Special:SetDescription",
            Self::Aliases => "Special:SetAliases",
            Self::All => "Special:SetLabelDescriptionAliases",
        }
    }

    fn key(self, suffix: &str) -> String {
        let name = match self {
            Self::Label => "setlabel",
            Self::Description => "setdescription",
            Self::Aliases => "setaliases",
            Self::All => "setlabeldescriptionaliases",
        };
        format!("ts-{name}-{suffix}")
    }
}

/// What the form holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Input {
    id: String,
    language: String,
    label: String,
    description: String,
    /// `|`-separated, as typed.
    aliases: String,
}

impl Input {
    /// From the subpage (`{id}/{language}`) and the parameters; `value=` fills the one term
    /// a single-term form sets.
    fn of(which: Which, title: Option<&str>, params: &Params, default_lang: &str) -> Self {
        let mut parts = title
            .and_then(|t| t.split_once('/'))
            .map(|(_, rest)| {
                rest.splitn(2, '/')
                    .map(|p| p.trim().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
            .into_iter();
        let (sub_id, sub_lang) = (parts.next(), parts.next());
        let get = |k: &str| params.map.get(k).map(|v| v.trim().to_string());
        let value = get("value");
        let mut input = Self {
            id: get("id")
                .filter(|v| !v.is_empty())
                .or(sub_id)
                .unwrap_or_default(),
            language: get("language")
                .filter(|v| !v.is_empty())
                .or(sub_lang)
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| default_lang.to_string()),
            label: get("label").unwrap_or_default(),
            description: get("description").unwrap_or_default(),
            aliases: get("aliases").unwrap_or_default(),
        };
        if let Some(v) = value {
            match which {
                Which::Label => input.label = v,
                Which::Description => input.description = v,
                Which::Aliases => input.aliases = v,
                Which::All => {}
            }
        }
        input
    }

    fn alias_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for a in self.aliases.split('|').map(str::trim) {
            if !a.is_empty() && !out.iter().any(|x| x == a) {
                out.push(a.to_string());
            }
        }
        out
    }
}

/// An entity's terms in one language, as `wbgetentities` gives them.
fn terms_of(e: &Value, lang: &str) -> (String, String, String) {
    let text = |k: &str| e[k][lang]["value"].as_str().unwrap_or_default().to_string();
    let aliases = e["aliases"][lang]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x["value"].as_str())
                .collect::<Vec<_>>()
                .join("|")
        })
        .unwrap_or_default();
    (text("labels"), text("descriptions"), aliases)
}

fn is_keyed(id: &str) -> bool {
    id.contains(':')
}

fn field(m: &Messages, name: &str, value: &str, label_key: &str, help: Option<&str>) -> String {
    let help = help
        .map(|k| {
            format!(
                "<div class=\"cdx-field__help-text\">{}</div>",
                esc(&m.get(k))
            )
        })
        .unwrap_or_default();
    format!(
        "<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"ts-setterms-{name}\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\"><div class=\"cdx-text-input\"><input class=\"cdx-text-input__input\" type=\"text\" id=\"ts-setterms-{name}\" name=\"{name}\" value=\"{}\"></div></div>{help}</div>",
        esc(&m.get(label_key)),
        esc(value)
    )
}

/// The form; `token` is `None` before an entity is chosen, when it only asks which.
fn form(m: &Messages, which: Which, input: &Input, token: Option<&str>, keyed: bool) -> String {
    let mut s = format!(
        "<p>{}</p><form class=\"ts-form\" method=\"{}\" action=\"/w/index.php?title={}\">",
        esc(&m.get(&which.key("intro"))),
        if token.is_some() { "post" } else { "get" },
        title_url(which.title())
    );
    if let Some(t) = token {
        let _ = write!(
            s,
            "<input type=\"hidden\" name=\"token\" value=\"{}\">",
            esc(t)
        );
    } else {
        let _ = write!(
            s,
            "<input type=\"hidden\" name=\"title\" value=\"{}\">",
            which.title()
        );
    }
    s.push_str(&field(m, "id", &input.id, "ts-setterms-id", None));
    s.push_str(&field(
        m,
        "language",
        &input.language,
        "ts-setterms-language",
        None,
    ));
    if token.is_some() {
        if matches!(which, Which::Label | Which::All) && !keyed {
            s.push_str(&field(m, "label", &input.label, "ts-setterms-label", None));
        }
        if matches!(which, Which::Description | Which::All) {
            s.push_str(&field(
                m,
                "description",
                &input.description,
                "ts-setterms-description",
                None,
            ));
        }
        if matches!(which, Which::Aliases | Which::All) {
            s.push_str(&field(
                m,
                "aliases",
                &input.aliases,
                "ts-setterms-aliases",
                Some("ts-setterms-aliases-help"),
            ));
        }
    }
    let label = if token.is_some() {
        m.get("ts-setterms-save")
    } else {
        m.get("ts-setterms-choose")
    };
    let _ = write!(
        s,
        "<div class=\"ts-form__actions\">{}</div></form>",
        Button {
            label: &label,
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .render()
        .unwrap_or_default()
    );
    s
}

fn page(
    cx: &Context,
    headers: &HeaderMap,
    which: Which,
    status: StatusCode,
    body: &str,
) -> Response {
    pages::respond(cx, headers, status, |m| Page {
        title: m.get(&which.key("title")),
        returnto: Some(which.title().to_string()),
        body: format!("<div class=\"ts-set-terms\">{body}</div>"),
        ..Page::default()
    })
}

/// The context, or the response that ends the request: editing not offered (`404`), or a
/// viewer not signed in (asked to, as the New item form asks).
async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    which: Which,
    query: &std::collections::BTreeMap<String, String>,
) -> Result<Context, Response> {
    let cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Edit) {
        return Err(pages::no_page(&cx, headers, which.title().to_string()));
    }
    if cx.user.is_none() {
        let mut body = pages::message(MessageKind::Notice, &cx.m.get("ts-setterms-login"));
        if cx.features.has(Feature::Login) {
            let _ = write!(
                body,
                "<p>{}</p>",
                link(
                    &format!(
                        "/w/index.php?title=Special:UserLogin&returnto={}",
                        title_url(which.title())
                    ),
                    &esc(&cx.m.get("ts-login-title")),
                    ""
                )
            );
        }
        return Err(page(&cx, headers, which, StatusCode::OK, &body));
    }
    Ok(cx)
}

/// The entity and its type, or the page saying there is none.
async fn entity_of(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    which: Which,
    input: &Input,
) -> Result<(Value, String), Response> {
    let found = match site
        .client()
        .entities(
            incoming,
            std::slice::from_ref(&input.id),
            "info|labels|descriptions|aliases",
            &input.language,
        )
        .await
    {
        Ok(f) => f,
        Err(e) => return Err(pages::api_failed(cx, headers, &e)),
    };
    cx.inputs.push(found.cache);
    if let Some(e) = found.value.found.into_values().next() {
        let entity_type = e["type"].as_str().unwrap_or("item").to_string();
        Ok((e, entity_type))
    } else {
        let mut body = pages::message(
            MessageKind::Error,
            &cx.m.with("ts-setterms-no-entity", &[&input.id]),
        );
        let mut asked = input.clone();
        asked.id.clear();
        body.push_str(&form(&cx.m, which, &asked, None, false));
        Err(page(cx, headers, which, StatusCode::NOT_FOUND, &body))
    }
}

/// The form for an entity, with a fresh token and any notice above it.
async fn edit_page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    which: Which,
    input: &Input,
    notice: Option<(String, StatusCode)>,
) -> Response {
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    // A page holding the viewer's token is theirs alone.
    cx.inputs.push(token.value.cache);
    cx.inputs.push(CacheInfo::default());
    let mut body = String::new();
    let mut status = StatusCode::OK;
    if let Some((n, s)) = notice {
        body.push_str(&pages::message(MessageKind::Error, &n));
        status = s;
    }
    body.push_str(&form(
        &cx.m,
        which,
        input,
        Some(&token.value.value),
        is_keyed(&input.id),
    ));
    pages::with_cookies(page(cx, headers, which, status, &body), &token.set_cookies)
}

/// `GET Special:SetLabel` and its siblings.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, which, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let mut input = Input::of(which, Some(title), params, cx.m.lang());
    if input.id.is_empty() {
        let body = form(&cx.m, which, &input, None, false);
        return page(&cx, headers, which, StatusCode::OK, &body);
    }
    if i18n::valid_code(&input.language).is_none() {
        let e = cx.m.with("ts-new-bad-lang", &[&input.language]);
        let body = format!(
            "{}{}",
            pages::message(MessageKind::Error, &e),
            form(&cx.m, which, &input, None, false)
        );
        return page(&cx, headers, which, StatusCode::BAD_REQUEST, &body);
    }
    let (e, _) = match entity_of(site, &mut cx, headers, &incoming, which, &input).await {
        Ok(e) => e,
        Err(r) => return r,
    };
    // The current terms, unless the address prefilled them.
    let (label, description, aliases) = terms_of(&e, &input.language);
    let given = |field: &str, its: Which| {
        params.map.contains_key(field) || (which == its && params.map.contains_key("value"))
    };
    if !given("label", Which::Label) {
        input.label = label;
    }
    if !given("description", Which::Description) {
        input.description = description;
    }
    if !given("aliases", Which::Aliases) {
        input.aliases = aliases;
    }
    if let Some(id) = e["id"].as_str() {
        input.id = id.to_string();
    }
    edit_page(site, &mut cx, headers, &incoming, which, &input, None).await
}

/// `POST Special:SetLabel` and its siblings: sets the terms, then goes to the entity.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    query: &std::collections::BTreeMap<String, String>,
    form_params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, which, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let input = Input::of(which, None, form_params, cx.m.lang());
    if input.id.is_empty() || i18n::valid_code(&input.language).is_none() {
        let e = if input.id.is_empty() {
            cx.m.get("ts-setterms-no-id")
        } else {
            cx.m.with("ts-new-bad-lang", &[&input.language])
        };
        return edit_page(
            site,
            &mut cx,
            headers,
            &incoming,
            which,
            &input,
            Some((e, StatusCode::BAD_REQUEST)),
        )
        .await;
    }
    let (_, entity_type) = match entity_of(site, &mut cx, headers, &incoming, which, &input).await {
        Ok(e) => e,
        Err(r) => return r,
    };
    let edit = match which {
        Which::Label => TermEdit::Label(input.label.clone()),
        Which::Description => TermEdit::Description(input.description.clone()),
        Which::Aliases => TermEdit::Aliases(input.alias_list()),
        Which::All => TermEdit::All {
            label: (!is_keyed(&input.id)).then(|| input.label.clone()),
            description: input.description.clone(),
            aliases: input.alias_list(),
        },
    };
    let token = form_params.map.get("token").map_or("", String::as_str);
    match site
        .client()
        .set_terms(&incoming, &input.id, &input.language, &edit, token)
        .await
    {
        Ok(()) => pages::see_other(&entity::entity_href(&input.id, &entity_type), &[]),
        Err(ClientError::Api { code, info }) => {
            let text = if code == "badtoken" {
                cx.m.get("ts-setterms-badtoken")
            } else {
                cx.m.with("ts-setterms-refused", &[&info])
            };
            edit_page(
                site,
                &mut cx,
                headers,
                &incoming,
                which,
                &input,
                Some((text, StatusCode::OK)),
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn params(pairs: &[(&str, &str)]) -> Params {
        Params::new(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    #[test]
    fn wikibase_addresses() {
        let i = Input::of(
            Which::Label,
            Some("Special:SetLabel/Q42/de"),
            &params(&[]),
            "en",
        );
        assert_eq!((i.id.as_str(), i.language.as_str()), ("Q42", "de"));
        let i = Input::of(
            Which::Description,
            Some("Special:SetDescription"),
            &params(&[("id", "P7"), ("value", " a thing ")]),
            "fr",
        );
        assert_eq!(
            (i.id.as_str(), i.language.as_str(), i.description.as_str()),
            ("P7", "fr", "a thing")
        );
        let i = Input::of(
            Which::Aliases,
            None,
            &params(&[("id", "Q1"), ("aliases", "a| b ||a")]),
            "en",
        );
        assert_eq!(i.alias_list(), ["a", "b"]);
    }

    #[test]
    fn current_terms_in_a_language() {
        let e = json!({"labels": {"de": {"language": "de", "value": "Sechs"}},
            "aliases": {"de": [{"language": "de", "value": "6"}, {"language": "de", "value": "VI"}]}});
        assert_eq!(
            terms_of(&e, "de"),
            ("Sechs".into(), String::new(), "6|VI".into())
        );
        assert_eq!(
            terms_of(&e, "en"),
            (String::new(), String::new(), String::new())
        );
    }
}
