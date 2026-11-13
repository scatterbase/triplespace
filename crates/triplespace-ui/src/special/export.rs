//! `Special:Export` (0047 §8.1; 21 §4.2): MediaWiki's form, with its parameters, and the
//! `records` format of 21 §4.4 as the second format the page offers. The form lists
//! titles, adds a category's pages (`addcat`) and, where the wiki allows it
//! (`export.from_namespaces`), a namespace's pages (`addns`); exports the current
//! revision only (`curonly`, on by default; off only where `export.history` allows) or
//! full history; accepts `templates`, which adds nothing until the transclusion index
//! exists and says so (0047 A33); and with `wpDownload` sends the file as an attachment.
//! The file comes from `POST /export` through the client, as everything the site reads
//! does (22 §1.2 rule 6); its refusal is shown over the form with what was typed.
//! `Special:Export/{title}` and `?pages=` prefill the titles, as MediaWiki's page does.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use triplespace_client::pages::PageListQuery;
use triplespace_client::{ClientError, ExportAsk, Incoming};

use crate::Site;
use crate::codex::{Action, Button, ButtonModel, Message, MessageKind, MessageModel, Weight};
use crate::features::Feature;
use crate::frame::Page;
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{CheckRow, ControlModel, FieldRow};

const TITLE: &str = "Special:Export";

/// `Special:Export` as Plain's `special/export.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportPage {
    /// A refusal or a notice, then what the page is for.
    pub notices: Vec<MessageModel>,
    /// What the page does.
    pub intro: String,
    /// Where the form posts.
    pub action: String,
    /// The category to add pages from (`catname`), and **Add**.
    pub category: AddRow,
    /// The namespace to add pages from (`nsindex`), and **Add**, where the wiki allows.
    pub namespace: Option<AddRow>,
    /// The titles, one per line (`pages`).
    pub pages: FieldRow,
    /// `curonly`, `templates`, `wpDownload`.
    pub options: Vec<CheckRow>,
    /// The formats' legend.
    pub formats_legend: String,
    /// `format`: `xml` or `records`.
    pub formats: Vec<ControlModel>,
    /// **Export**.
    pub submit: ButtonModel,
}

/// A field with a button that adds its pages to the list.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AddRow {
    /// The field.
    pub field: FieldRow,
    /// The button's name (`addcat`, `addns`).
    pub name: String,
    /// The button.
    pub button: ButtonModel,
}

/// What the form holds, typed or defaulted.
#[derive(Debug, Clone, Default)]
pub struct Typed {
    /// `catname`.
    pub category: String,
    /// `nsindex`.
    pub namespace: String,
    /// `pages`.
    pub pages: String,
    /// `curonly`.
    pub current_only: bool,
    /// `templates`.
    pub templates: bool,
    /// `wpDownload`.
    pub download: bool,
    /// `format`.
    pub format: String,
}

impl Typed {
    /// The form as it comes up: current revision only, as a download, as XML.
    fn fresh(pages: String) -> Self {
        Self {
            pages,
            current_only: true,
            download: true,
            format: "xml".into(),
            ..Self::default()
        }
    }

    /// From a posted form.
    fn posted(params: &Params) -> Self {
        let field = |k: &str| params.map.get(k).map_or("", String::as_str);
        Self {
            category: field("catname").trim().to_string(),
            namespace: field("nsindex").trim().to_string(),
            pages: field("pages").to_string(),
            current_only: params.map.contains_key("curonly"),
            templates: params.map.contains_key("templates"),
            download: params.map.contains_key("wpDownload"),
            format: match field("format") {
                "records" => "records".into(),
                _ => "xml".into(),
            },
        }
    }

    /// The titles listed, trimmed, without blank lines.
    fn titles(&self) -> Vec<String> {
        self.pages
            .lines()
            .map(|l| l.trim().replace('_', " "))
            .filter(|l| !l.is_empty())
            .collect()
    }

    /// Adds titles to the list, each once.
    fn add(&mut self, titles: &[String]) {
        let mut have = self.titles();
        for t in titles {
            if !have.contains(t) {
                have.push(t.clone());
            }
        }
        self.pages = have.join("\n");
        if !self.pages.is_empty() {
            self.pages.push('\n');
        }
    }
}

/// The title a request names: `Special:Export/{title}`, or `?pages=`.
fn prefill(title: &str, params: &Params) -> String {
    let from_title = title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest.replace('_', " ").trim().to_string())
        .filter(|s| !s.is_empty());
    match from_title {
        Some(t) => format!("{t}\n"),
        None => params
            .map
            .get("pages")
            .map(|p| p.replace('_', " "))
            .unwrap_or_default(),
    }
}

/// The namespaces `addns` offers: those holding pages, with the site's names.
async fn namespace_options(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    chosen: &str,
) -> Vec<(String, String, bool)> {
    let Ok(rows) = site.client().namespaces(incoming).await else {
        return Vec::new();
    };
    cx.inputs.push(rows.cache);
    rows.value
        .iter()
        .filter(|n| {
            n.id >= 0
                && n.model
                    .as_deref()
                    .is_none_or(|m| !m.starts_with("wikibase-"))
        })
        .map(|n| {
            let label = if n.name.is_empty() {
                cx.m.get("ts-export-main-namespace")
            } else {
                n.name.clone()
            };
            (n.id.to_string(), label, n.id.to_string() == chosen)
        })
        .collect()
}

/// The form.
#[allow(clippy::too_many_lines)]
async fn form(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    typed: &Typed,
    notice: Option<(MessageKind, String)>,
) -> Response {
    if !cx.features.has(Feature::Export) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let namespace = if cx.site.export.from_namespaces {
        let options = namespace_options(site, &mut cx, incoming, &typed.namespace).await;
        Some(options)
    } else {
        None
    };
    let m = &cx.m;
    let mut notices: Vec<MessageModel> = notice
        .iter()
        .map(|(k, t)| Message::block(*k, t).model())
        .collect();
    if !cx.site.export.history {
        notices.push(Message::block(MessageKind::Notice, &m.get("ts-export-no-history")).model());
    }
    let add = |label: &str| {
        Button {
            label,
            kind: "submit",
            action: Action::Default,
            weight: Weight::Normal,
        }
        .model()
    };
    let option = |id: &str, name: &str, on: bool| CheckRow {
        class: "ts-export__option".into(),
        control: ControlModel::check(id, name, on, m.get(id)),
        help: m.get(&format!("{id}-help")),
    };
    let mut options = Vec::new();
    if cx.site.export.history {
        options.push(option("ts-export-curonly", "curonly", typed.current_only));
    }
    options.push(option("ts-export-templates", "templates", typed.templates));
    options.push(option("ts-export-download", "wpDownload", typed.download));
    let format = |value: &str, key: &str| ControlModel::Radio {
        id: Some(format!("ts-export-format-{value}")),
        name: "format".into(),
        value: value.into(),
        checked: typed.format == value,
        label: m.get(key),
        described_by: None,
    };
    let model = ExportPage {
        notices,
        intro: m.get("ts-export-intro"),
        action: format!("/wiki/{TITLE}"),
        category: AddRow {
            field: FieldRow::text(
                "ts-export-catname",
                "catname",
                m.get("ts-export-catname"),
                &typed.category,
                false,
            ),
            name: "addcat".into(),
            button: add(&m.get("ts-export-add")),
        },
        namespace: namespace.map(|options| AddRow {
            field: FieldRow {
                id: "ts-export-nsindex".into(),
                label: m.get("ts-export-nsindex"),
                class: None,
                control: ControlModel::Select {
                    id: Some("ts-export-nsindex".into()),
                    name: "nsindex".into(),
                    options: options
                        .into_iter()
                        .map(
                            |(value, label, selected)| crate::special::controls::OptionModel {
                                value,
                                label,
                                selected,
                            },
                        )
                        .collect(),
                    described_by: None,
                },
                help: None,
                help_id: None,
            },
            name: "addns".into(),
            button: add(&m.get("ts-export-add")),
        }),
        pages: FieldRow {
            id: "ts-export-pages".into(),
            label: m.get("ts-export-pages"),
            class: None,
            control: ControlModel::TextArea {
                id: Some("ts-export-pages".into()),
                name: "pages".into(),
                rows: typed.pages.lines().count().clamp(6, 24),
                value: typed.pages.clone(),
                no_spellcheck: true,
                described_by: Some("ts-export-pages-help".into()),
            },
            help: Some(m.get("ts-export-pages-help")),
            help_id: Some("ts-export-pages-help".into()),
        },
        options,
        formats_legend: m.get("ts-export-format"),
        formats: vec![
            format("xml", "ts-export-format-xml"),
            format("records", "ts-export-format-records"),
        ],
        submit: Button {
            label: &m.get("ts-export-submit"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .model(),
    };
    let body = cx.draw("special/export.html", &model);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-export-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

/// `GET Special:Export[/{title}]`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let typed = Typed::fresh(prefill(title, params));
    form(site, cx, headers, &incoming, &typed, None).await
}

fn refusal(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "no-pages" => m.get("ts-export-none"),
        "export-too-large" => m.get("ts-export-too-large"),
        "export-namespaces-off" => m.get("ts-export-namespaces-off"),
        "tenant-not-readable" | "permissiondenied" | "readapidenied" => {
            m.with("ts-moderation-denied", &["read"])
        }
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

/// `POST Special:Export`: **Add** a category's or a namespace's pages to the list, or
/// export the list.
#[allow(clippy::too_many_lines)]
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &std::collections::BTreeMap<String, String>,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Export) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let mut typed = Typed::posted(params);
    if params.map.contains_key("addcat") {
        let notice = if typed.category.is_empty() {
            None
        } else {
            match site
                .client()
                .category_members(&incoming, &typed.category)
                .await
            {
                Ok(found) => {
                    cx.inputs.push(found.cache);
                    let n = found.value.len();
                    typed.add(&found.value);
                    Some((
                        MessageKind::Notice,
                        cx.m.with("ts-export-added", &[&n.to_string()]),
                    ))
                }
                Err(e) => Some((MessageKind::Error, client_refusal(&cx.m, &e))),
            }
        };
        return form(site, cx, headers, &incoming, &typed, notice).await;
    }
    if params.map.contains_key("addns") {
        let notice = match typed.namespace.parse::<i64>() {
            Err(_) => None,
            Ok(ns) => {
                // Up to MediaWiki's page-list limit, 5,000, in the API's pages of 500.
                let mut titles = Vec::new();
                let mut from = String::new();
                let mut failed = None;
                for _ in 0..10 {
                    let ask = PageListQuery {
                        namespace: ns,
                        from: from.clone(),
                        limit: 500,
                        ..PageListQuery::default()
                    };
                    match site.client().all_pages(&incoming, &ask).await {
                        Ok(page) => {
                            cx.inputs.push(page.cache);
                            titles.extend(page.value.titles);
                            match page.value.next {
                                Some(next) => from = next,
                                None => break,
                            }
                        }
                        Err(e) => {
                            failed = Some(e);
                            break;
                        }
                    }
                }
                if let Some(e) = failed {
                    Some((MessageKind::Error, client_refusal(&cx.m, &e)))
                } else {
                    let n = titles.len();
                    typed.add(&titles);
                    Some((
                        MessageKind::Notice,
                        cx.m.with("ts-export-added", &[&n.to_string()]),
                    ))
                }
            }
        };
        return form(site, cx, headers, &incoming, &typed, notice).await;
    }
    let titles = typed.titles();
    if titles.is_empty() {
        let text = cx.m.get("ts-export-none");
        return form(
            site,
            cx,
            headers,
            &incoming,
            &typed,
            Some((MessageKind::Warning, text)),
        )
        .await;
    }
    let ask = ExportAsk {
        format: typed.format.clone(),
        pages: titles,
        categories: Vec::new(),
        namespaces: Vec::new(),
        history: cx.site.export.history && !typed.current_only,
        templates: typed.templates,
    };
    match site.client().export(&incoming, &ask).await {
        Ok(file) => {
            let mut r = Response::new(file.body);
            *r.status_mut() = StatusCode::OK;
            let h = r.headers_mut();
            if let Ok(v) = HeaderValue::from_str(&file.content_type) {
                h.insert(header::CONTENT_TYPE, v);
            }
            h.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            );
            if typed.download
                && let Some(name) = &file.filename
                && let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
            {
                h.insert(header::CONTENT_DISPOSITION, v);
            }
            h.insert("x-export-missing", HeaderValue::from(file.missing));
            if let Some(n) = &file.notice
                && let Ok(v) = HeaderValue::from_str(n)
            {
                h.insert("x-export-notice", v);
            }
            r
        }
        Err(e) => {
            let text = client_refusal(&cx.m, &e);
            form(
                site,
                cx,
                headers,
                &incoming,
                &typed,
                Some((MessageKind::Error, text)),
            )
            .await
        }
    }
}

/// What the page says of a failed call.
fn client_refusal(m: &Messages, e: &ClientError) -> String {
    match e {
        ClientError::Api { code, info } => refusal(m, code, info),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_grows_without_repeats() {
        let mut t = Typed::fresh("Project:Home\n".into());
        t.add(&["Project:Scope".into(), "Project:Home".into()]);
        assert_eq!(t.titles(), ["Project:Home", "Project:Scope"]);
        assert!(t.pages.ends_with('\n'));
        let p = Params::new(vec![
            ("pages".into(), " A_b \n\n c ".into()),
            ("curonly".into(), "1".into()),
            ("format".into(), "records".into()),
        ]);
        let t = Typed::posted(&p);
        assert_eq!(t.titles(), ["A b", "c"]);
        assert!(t.current_only && !t.download && t.format == "records");
    }

    #[test]
    fn the_title_prefills() {
        assert_eq!(
            prefill("Special:Export/Project:Home", &Params::default()),
            "Project:Home\n"
        );
        let p = Params::new(vec![("pages".into(), "A_b\nC".into())]);
        assert_eq!(prefill("Special:Export", &p), "A b\nC");
        assert_eq!(prefill("Special:Export", &Params::default()), "");
    }
}
