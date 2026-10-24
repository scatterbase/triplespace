//! `Special:WikiSettings`, `Special:InstanceSettings` and `Special:Tenancy` (0085 §4;
//! 19 §2.8): a tenant's settings at its base, the instance's at the farm base, built from
//! `GET /settings/schema` and `GET /settings` with no code per setting.
//!
//! The page is one section per registry group, reached by the links at its top; each
//! section is a form of its own and saves with one `PATCH /settings`, so a section's
//! anchor (`#tenancy`) is a place on the page with or without JavaScript. Each row shows
//! the label and help, the value with a control drawn from its type, an origin chip and
//! **Reset**. A viewer who may not change a setting sees its value without a control.
//!
//! `Special:WikiSettings` adds a read-only **Tenancy** section: the instance's policy as
//! it applies to the wiki. `Special:InstanceSettings`, served only at the farm base, adds
//! the **Tenancy** switches and, unless `config.template` is `none`, **Defaults for
//! wikis**: every tenant setting with its registry default, the instance's default, its
//! lock under `config.template = locks`, and how many wikis inherit the default.
//!
//! A save that works answers `303` to the section with `?saved=`; one the API refuses
//! shows the page again with the reader's input and every reason beside its row.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use askama::Template as _;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::{Map, Value, json};
use triplespace_client::settings::{Refusal, Resolved, Saved, Schema, SettingDef, Settings};
use triplespace_client::{CacheInfo, ClientError, Incoming};

use crate::Site;
use crate::codex::{Action, Button, MessageKind, Weight};
use crate::features::Feature;
use crate::frame::{Page, Tab, title_url};
use crate::html::esc;
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{self, Control, Kind};

/// Which page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `Special:WikiSettings`.
    Wiki,
    /// `Special:InstanceSettings`.
    Instance,
}

impl Scope {
    fn title(self) -> &'static str {
        match self {
            Self::Wiki => "Special:WikiSettings",
            Self::Instance => "Special:InstanceSettings",
        }
    }

    fn instance(self) -> bool {
        self == Self::Instance
    }
}

/// The section of the tenancy policy.
pub const TENANCY: &str = "tenancy";
/// The Visibility section's anchor (19 §6.3).
pub const VISIBILITY: &str = "visibility";
/// The section of the instance's defaults for wikis.
pub const DEFAULTS: &str = "defaults";

/// The text of a registry entry's Markdown description, for the help shown while its
/// message is missing: links reduced to their text, code marks dropped.
fn plain(markdown: &str) -> String {
    let mut out = String::new();
    let mut rest = markdown;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| {
            after[close + 2..]
                .find(')')
                .map(|end| (close, close + 2 + end))
        });
        if let Some((close, end)) = link {
            out.push_str(&after[..close]);
            rest = &after[end + 1..];
        } else {
            out.push('[');
            rest = after;
        }
    }
    out.push_str(rest);
    out.replace('`', "")
}

/// A setting's label: its message, or its key where the message is missing (a setting an
/// instance registered).
fn label(m: &Messages, def: &SettingDef) -> String {
    if m.has(&def.label) {
        m.get(&def.label)
    } else {
        def.key.clone()
    }
}

/// A setting's help: its message, or the registry's description.
fn help(m: &Messages, def: &SettingDef) -> Option<String> {
    if m.has(&def.help) {
        Some(m.get(&def.help))
    } else {
        def.meaning.as_deref().map(plain)
    }
}

/// A group's label.
fn group_label(m: &Messages, id: &str, key: &str) -> String {
    if m.has(key) {
        m.get(key)
    } else {
        id.to_string()
    }
}

/// The origin chip (0085 §4).
fn origin_chip(m: &Messages, scope: Scope, r: &Resolved) -> String {
    let (class, text) = match (r.origin.as_str(), scope) {
        ("locked", _) => ("locked", m.get("ts-settings-origin-locked")),
        ("tenant", _) => ("own", m.get("ts-settings-origin-tenant")),
        ("instance", Scope::Wiki) => ("inherited", m.get("ts-settings-origin-instance")),
        ("instance", Scope::Instance) => ("own", m.get("ts-settings-origin-instance-own")),
        _ => ("default", m.get("ts-settings-origin-registry")),
    };
    let mut s = format!(
        "<span class=\"ts-chip ts-setting__origin ts-setting__origin--{class}\">{}</span>",
        esc(&text)
    );
    if let Some(c) = &r.capped {
        let _ = write!(
            s,
            " <span class=\"ts-chip ts-setting__origin ts-setting__origin--capped\">{}</span>",
            esc(&m.with(
                "ts-settings-origin-capped",
                &[&controls::as_text(Kind::Line, c)]
            ))
        );
    }
    s
}

/// What the page shows in place of an absent value: the registry's description of what
/// applies, or `none`.
fn absent(m: &Messages, def: &SettingDef) -> String {
    def.default_text
        .as_deref()
        .map_or_else(|| m.get("ts-settings-none"), plain)
}

/// What came back with a form: the reader's input and the API's reasons.
#[derive(Debug, Default)]
struct Returned<'a> {
    /// The section the form was.
    section: String,
    /// The form.
    form: Option<&'a Params>,
    /// The reasons, by key; the empty key for the section as a whole.
    errors: BTreeMap<String, Vec<String>>,
}

impl Returned<'_> {
    /// The form the reader sent, if it was this section's.
    fn sent(&self, section: &str) -> Option<&BTreeMap<String, String>> {
        if self.section == section {
            self.form.map(|f| &f.map)
        } else {
            None
        }
    }

    /// A text field of this section's form, if it was sent.
    fn text(&self, section: &str, field: &str) -> Option<&str> {
        self.sent(section)?.get(field).map(String::as_str)
    }
}

/// Everything a page draws from the API.
struct Loaded {
    schema: Schema,
    settings: Settings,
    token: Option<String>,
    cookies: Vec<HeaderValue>,
    /// The wiki's visibility and the groups it may be restricted to, for the Visibility
    /// section (09 §4.7; 19 §6.3); `None` at the instance's pages or where the API does
    /// not serve `/acl`.
    visibility: Option<(triplespace_client::TenantVisibility, Vec<String>)>,
}

/// Whether the viewer may write settings at all: they may change one that needs only
/// `ts-config`.
fn may_configure(schema: &Schema, settings: &Settings) -> bool {
    schema
        .settings
        .iter()
        .filter(|d| d.right.is_none())
        .any(|d| settings.settings.get(&d.key).is_some_and(|r| r.can_change))
}

/// The settings that name a palette or type set (0086 §4) offered as a select of the
/// wiki's palettes and type sets.
fn offer_themes(defs: &mut [SettingDef], theme: &crate::theme::Theme) {
    for d in defs {
        let sheet = match d.ty.as_str() {
            "palette" => crate::theme::Sheet::Palette,
            "typeset" => crate::theme::Sheet::Typeset,
            _ => continue,
        };
        d.ty = "choice".into();
        d.choices = theme.offered(sheet);
    }
}

async fn load(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    scope: Scope,
) -> Result<Loaded, ClientError> {
    let client = site.client();
    let (schema, settings) = tokio::join!(
        client.settings_schema(incoming, scope.instance()),
        client.settings(incoming, scope.instance())
    );
    let (schema, settings) = (schema?, settings?);
    cx.inputs.push(schema.cache);
    cx.inputs.push(settings.cache);
    let (mut token, mut cookies) = (None, Vec::new());
    if may_configure(&schema.value, &settings.value) {
        let t = client.token(incoming, "csrf").await?;
        // A page holding a token is the viewer's alone.
        cx.inputs.push(CacheInfo::default());
        token = Some(t.value.value);
        cookies = t.set_cookies;
    }
    let mut schema = schema.value;
    offer_themes(&mut schema.settings, &cx.theme);
    let visibility = if scope == Scope::Wiki && cx.features.has(Feature::Protect) {
        let (v, g) = tokio::join!(
            client.tenant_visibility(incoming, &cx.site.tenant),
            client.groups(incoming)
        );
        match (v, g) {
            (Ok(v), Ok(g)) => {
                cx.inputs.push(v.cache);
                cx.inputs.push(g.cache);
                Some((
                    v.value,
                    g.value
                        .into_iter()
                        .filter(|g| !g.implicit && g.name != "*")
                        .map(|g| g.name)
                        .collect(),
                ))
            }
            // A reader of a private wiki, or an API without the route: no section.
            _ => None,
        }
    } else {
        None
    };
    Ok(Loaded {
        schema,
        settings: settings.value,
        token,
        cookies,
        visibility,
    })
}

fn hidden(name: &str, value: &str) -> String {
    format!(
        "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
        esc(name),
        esc(value)
    )
}

fn submit(label: &str) -> String {
    Button {
        label,
        kind: "submit",
        action: Action::Progressive,
        weight: Weight::Primary,
    }
    .render()
    .unwrap_or_default()
}

/// A **Reset** button: submits the section's form asking for one key only.
fn reset_button(m: &Messages, value: &str, label: &str) -> String {
    format!(
        "<button class=\"cdx-button cdx-button--weight-quiet ts-setting__reset\" type=\"submit\" name=\"reset\" value=\"{}\" aria-label=\"{}\">{}</button>",
        esc(value),
        esc(&m.with("ts-settings-reset-label", &[label])),
        esc(&m.get("ts-settings-reset"))
    )
}

fn errors_html(errors: Option<&Vec<String>>) -> String {
    errors.map_or_else(String::new, |e| {
        e.iter()
            .map(|t| pages::message(MessageKind::Error, t))
            .collect()
    })
}

/// The form's opening: where it posts, which section it is, the base and the token.
fn form_open(scope: Scope, section: &str, base: u64, token: Option<&str>, keys: &[&str]) -> String {
    format!(
        "<form class=\"ts-settings__form\" method=\"post\" action=\"/w/index.php?title={}\">{}{}{}{}",
        title_url(scope.title()),
        hidden("section", section),
        hidden("base", &base.to_string()),
        token.map(|t| hidden("token", t)).unwrap_or_default(),
        hidden("keys", &keys.join(" "))
    )
}

/// The summary field and **Save**.
fn form_close(m: &Messages, section: &str, summary: &str) -> String {
    format!(
        "<div class=\"ts-settings__save\"><div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"ts-summary-{s}\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\"><div class=\"cdx-text-input\"><input class=\"cdx-text-input__input\" type=\"text\" id=\"ts-summary-{s}\" name=\"summary\" value=\"{}\"></div></div></div>{}</div></form>",
        esc(&m.get("ts-settings-summary")),
        esc(summary),
        submit(&m.get("ts-settings-save")),
        s = esc(section)
    )
}

/// One setting's row.
fn row(
    m: &Messages,
    scope: Scope,
    def: &SettingDef,
    r: &Resolved,
    editable: bool,
    back: &Returned<'_>,
) -> String {
    let id = format!("ts-s-{}", def.key);
    let help_id = format!("{id}-help");
    let kind = Kind::of(&def.ty);
    let name = format!("v.{}", def.key);
    let label_text = label(m, def);
    let mut s = format!(
        "<div class=\"ts-setting\" id=\"setting-{}\"><div class=\"ts-setting__head\">",
        esc(&def.key)
    );
    if editable && kind != Kind::ReadOnly {
        let _ = write!(
            s,
            "<label class=\"ts-setting__label\" for=\"{}\">{}</label>",
            esc(&id),
            esc(&label_text)
        );
    } else {
        let _ = write!(
            s,
            "<span class=\"ts-setting__label\">{}</span>",
            esc(&label_text)
        );
    }
    let _ = write!(
        s,
        " {} <code class=\"ts-setting__key\">{}</code></div>",
        origin_chip(m, scope, r),
        esc(&def.key)
    );
    let help_text = help(m, def);
    if let Some(h) = &help_text {
        let _ = write!(
            s,
            "<p class=\"ts-setting__help\" id=\"{}\">{}</p>",
            esc(&help_id),
            esc(h)
        );
    }
    s.push_str(&errors_html(back.errors.get(&def.key)));
    s.push_str("<div class=\"ts-setting__value\">");
    if editable && kind != Kind::ReadOnly {
        // Input the reader sent back stands in for the value.
        let typed;
        let value = match back.sent(&def.group) {
            Some(f) if kind == Kind::Boolean => {
                typed = Value::Bool(f.contains_key(&name));
                Some(&typed)
            }
            Some(f) if f.contains_key(&name) => {
                typed = Value::String(f[&name].clone());
                Some(&typed)
            }
            _ => r.value.as_ref(),
        };
        let choice_label = |c: &str| {
            let key = format!("{}-{}", def.label, c);
            if m.has(&key) {
                m.get(&key)
            } else {
                c.to_string()
            }
        };
        s.push_str(&controls::render(&Control {
            id: &id,
            name: &name,
            kind,
            choices: &def.choices,
            value,
            empty_choice: (r.value.is_none()).then_some("—"),
            described_by: help_text.as_ref().map(|_| help_id.as_str()),
            choice_label: Some(&choice_label),
        }));
        let own = match scope {
            Scope::Wiki => r.origin == "tenant",
            Scope::Instance => r.origin == "instance",
        };
        if own {
            s.push_str(&reset_button(m, &def.key, &label_text));
        }
    } else {
        let text = match &r.value {
            None => absent(m, def),
            Some(v) => controls::shown(m, kind, Some(v)),
        };
        let _ = write!(s, "<span class=\"ts-setting__shown\">{}</span>", esc(&text));
    }
    if r.unresolved_default {
        let _ = write!(
            s,
            "<p class=\"ts-setting__note\">{}</p>",
            esc(&m.get("ts-settings-unresolved-default"))
        );
    }
    s.push_str("</div></div>");
    s
}

/// The `?saved=` notice of a section.
fn saved_notice(m: &Messages, saved: Option<&String>, section: &str) -> String {
    if saved.is_some_and(|s| s == section) {
        pages::message(MessageKind::Success, &m.get("ts-settings-saved"))
    } else {
        String::new()
    }
}

/// One registry group's section.
fn group_section(
    m: &Messages,
    scope: Scope,
    loaded: &Loaded,
    group: &str,
    group_title: &str,
    back: &Returned<'_>,
    saved: Option<&String>,
) -> String {
    let defs: Vec<&SettingDef> = loaded
        .schema
        .settings
        .iter()
        .filter(|d| d.group == group)
        .collect();
    let editable: BTreeSet<&str> = defs
        .iter()
        .filter(|d| {
            loaded.token.is_some()
                && loaded
                    .settings
                    .settings
                    .get(&d.key)
                    .is_some_and(|r| r.can_change)
        })
        .map(|d| d.key.as_str())
        .collect();
    let mut s = format!(
        "<section class=\"ts-settings__section\" id=\"{}\" aria-labelledby=\"{}-title\"><h2 id=\"{}-title\">{}</h2>",
        esc(group),
        esc(group),
        esc(group),
        esc(group_title)
    );
    s.push_str(&saved_notice(m, saved, group));
    if back.section == group {
        s.push_str(&errors_html(back.errors.get("")));
    }
    let keys: Vec<&str> = editable.iter().copied().collect();
    if !keys.is_empty() {
        s.push_str(&form_open(
            scope,
            group,
            loaded.settings.base,
            loaded.token.as_deref(),
            &keys,
        ));
    }
    for def in &defs {
        let Some(r) = loaded.settings.settings.get(&def.key) else {
            continue;
        };
        s.push_str(&row(
            m,
            scope,
            def,
            r,
            editable.contains(def.key.as_str()),
            back,
        ));
    }
    if keys.is_empty() {
        s.push_str("</section>");
    } else {
        let summary = back.text(group, "summary").unwrap_or_default();
        s.push_str(&form_close(m, group, summary));
        s.push_str("</section>");
    }
    s
}

/// The Visibility section (09 §4.7; 0056 §3; 19 §6.3): who may read the wiki at all.
/// *Anyone*; *Accounts on this wiki* (`user`); *A group*. The tenant ACL behind it needs
/// `ts-config`, which the page's token stands for; without it the section only reports.
fn visibility_section(
    m: &Messages,
    cx: &Context,
    loaded: &Loaded,
    back: &Returned<'_>,
    saved: Option<&String>,
) -> String {
    let Some((vis, groups)) = &loaded.visibility else {
        return String::new();
    };
    let mut s = format!(
        "<section class=\"ts-settings__section\" id=\"{VISIBILITY}\" aria-labelledby=\"{VISIBILITY}-title\"><h2 id=\"{VISIBILITY}-title\">{}</h2>",
        esc(&m.get("ts-settings-group-visibility")),
    );
    s.push_str(&saved_notice(m, saved, VISIBILITY));
    if back.section == VISIBILITY {
        s.push_str(&errors_html(back.errors.get("")));
    }
    let current = match vis.read_group.as_deref() {
        None => "anyone".to_string(),
        Some("user") => "user".to_string(),
        Some(_) => "group".to_string(),
    };
    let chosen = back
        .text(VISIBILITY, "visibility")
        .unwrap_or(&current)
        .to_string();
    let group_now = back
        .text(VISIBILITY, "group")
        .map(str::to_string)
        .or_else(|| vis.read_group.clone().filter(|g| g != "user"))
        .unwrap_or_default();
    let _ = write!(
        s,
        "<p>{}</p>",
        esc(&m.with(
            "ts-settings-visibility-now",
            &[&match vis.read_group.as_deref() {
                None => m.get("ts-settings-visibility-anyone"),
                Some("user") => m.get("ts-settings-visibility-user"),
                Some(g) => m.with("ts-settings-visibility-a-group", &[g]),
            }]
        ))
    );
    let Some(token) = loaded.token.as_deref() else {
        s.push_str("</section>");
        return s;
    };
    let _ = write!(
        s,
        "<form method=\"post\" action=\"/wiki/{}\">{}{}<fieldset class=\"ts-settings__visibility\"><legend>{}</legend>",
        esc(&title_url(Scope::Wiki.title())),
        hidden("token", token),
        hidden("section", VISIBILITY),
        esc(&m.get("ts-settings-visibility-who")),
    );
    for (value, key) in [
        ("anyone", "ts-settings-visibility-anyone"),
        ("user", "ts-settings-visibility-user"),
        ("group", "ts-settings-visibility-group"),
    ] {
        let _ = write!(
            s,
            "<span class=\"cdx-radio\"><input class=\"cdx-radio__input\" type=\"radio\" id=\"ts-vis-{value}\" name=\"visibility\" value=\"{value}\"{}><span class=\"cdx-radio__icon\"></span><label class=\"cdx-radio__label\" for=\"ts-vis-{value}\">{}</label></span>",
            if chosen == value { " checked" } else { "" },
            esc(&m.get(key)),
        );
    }
    let group_value = Value::String(group_now);
    let picker = controls::render(&Control {
        id: "ts-vis-group-name",
        name: "group",
        kind: Kind::Choice,
        choices: groups,
        value: Some(&group_value),
        empty_choice: Some(""),
        described_by: Some("ts-vis-group-help"),
        choice_label: None,
    });
    let _ = write!(
        s,
        "<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"ts-vis-group-name\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\">{picker}</div><div class=\"cdx-field__help-text\" id=\"ts-vis-group-help\">{}</div></div></fieldset>{}<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"ts-vis-reason\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\"><div class=\"cdx-text-input\"><input class=\"cdx-text-input__input\" type=\"text\" id=\"ts-vis-reason\" name=\"reason\" value=\"{}\"></div></div></div>{}</form></section>",
        esc(&m.get("ts-settings-visibility-which-group")),
        esc(&m.with(
            "ts-settings-visibility-group-help",
            &[&cx.groups.join(", ")]
        )),
        pages::message(
            MessageKind::Warning,
            &m.get("ts-settings-visibility-warning")
        ),
        esc(&m.get("ts-settings-visibility-reason")),
        esc(back.text(VISIBILITY, "reason").unwrap_or_default()),
        submit(&m.get("ts-settings-save")),
    );
    s
}

fn value_or_unset(m: &Messages, v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => m.get("ts-settings-unset"),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

/// The tenancy policy: read-only on a wiki, the switches' form at the farm base.
#[allow(clippy::too_many_lines)] // one section of markup, read top to bottom
fn tenancy_section(
    m: &Messages,
    scope: Scope,
    loaded: &Loaded,
    back: &Returned<'_>,
    saved: Option<&String>,
) -> String {
    let schema = &loaded.schema;
    let editable = scope == Scope::Instance
        && loaded.token.is_some()
        && may_configure(schema, &loaded.settings);
    let mut s = format!(
        "<section class=\"ts-settings__section\" id=\"{TENANCY}\" aria-labelledby=\"{TENANCY}-title\"><h2 id=\"{TENANCY}-title\">{}</h2><p>{}</p>",
        esc(&m.get("ts-settings-group-tenancy")),
        esc(&m.get(match scope {
            Scope::Wiki => "ts-settings-tenancy-wiki",
            Scope::Instance => "ts-settings-tenancy-instance",
        }))
    );
    s.push_str(&saved_notice(m, saved, TENANCY));
    if back.section == TENANCY {
        s.push_str(&errors_html(back.errors.get("")));
    }
    let switches: Vec<&str> = schema.tenancy.keys().map(String::as_str).collect();
    if editable {
        s.push_str(&form_open(
            scope,
            TENANCY,
            loaded.settings.base,
            loaded.token.as_deref(),
            &switches,
        ));
    }
    s.push_str("<dl class=\"ts-settings__switches\">");
    for (name, sw) in &schema.tenancy {
        let id = format!("ts-t-{name}");
        let field = format!("t.{name}");
        let label_key = format!("ts-tenancy-{}", name.replace('.', "-"));
        let label_text = if m.has(&label_key) {
            m.get(&label_key)
        } else {
            name.clone()
        };
        let _ = write!(s, "<div class=\"ts-setting\" id=\"switch-{}\">", esc(name));
        if editable {
            let _ = write!(
                s,
                "<dt><label class=\"ts-setting__label\" for=\"{}\">{}</label> <code class=\"ts-setting__key\">{}</code></dt>",
                esc(&id),
                esc(&label_text),
                esc(name)
            );
        } else {
            let _ = write!(
                s,
                "<dt><span class=\"ts-setting__label\">{}</span> <code class=\"ts-setting__key\">{}</code></dt>",
                esc(&label_text),
                esc(name)
            );
        }
        s.push_str(&errors_html(back.errors.get(name)));
        s.push_str("<dd>");
        if editable {
            let typed;
            let value = match back.text(TENANCY, &field) {
                Some(raw) => {
                    typed = Value::String(raw.to_string());
                    Some(&typed)
                }
                _ => sw.value.as_ref(),
            };
            let kind = if sw.allowed.choices.is_empty() {
                Kind::Line
            } else {
                Kind::Choice
            };
            s.push_str(&controls::render(&Control {
                id: &id,
                name: &field,
                kind,
                choices: &sw.allowed.choices,
                value,
                empty_choice: sw.value.is_none().then_some("—"),
                described_by: None,
                choice_label: None,
            }));
            if let Some(t) = &sw.allowed.text {
                let _ = write!(s, " <span class=\"ts-setting__help\">{}</span>", esc(t));
            }
        } else {
            let _ = write!(
                s,
                "<span class=\"ts-setting__shown\">{}</span>",
                esc(&value_or_unset(m, sw.value.as_ref()))
            );
        }
        s.push_str("</dd></div>");
    }
    s.push_str("</dl>");
    if scope == Scope::Wiki {
        let _ = write!(
            s,
            "<h3>{}</h3><p>{}</p>",
            esc(&m.get("ts-settings-tenancy-locked")),
            esc(&if schema.locked.is_empty() || schema.template != "locks" {
                m.get("ts-settings-tenancy-none-locked")
            } else {
                schema
                    .locked
                    .iter()
                    .map(|k| k.strip_prefix("site:").unwrap_or(k).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
        );
        let _ = write!(
            s,
            "<h3>{}</h3><p>{}</p>",
            esc(&m.get("ts-settings-tenancy-ceilings")),
            esc(&if schema.ceilings.is_empty() {
                m.get("ts-settings-tenancy-no-ceilings")
            } else {
                schema
                    .ceilings
                    .iter()
                    .map(|(k, v)| format!("{k} ≤ {}", value_or_unset(m, Some(v))))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
        );
    }
    if editable {
        let summary = back.text(TENANCY, "summary").unwrap_or_default();
        s.push_str(&form_close(m, TENANCY, summary));
    }
    s.push_str("</section>");
    s
}

/// A default's control: a boolean is a select of on, off and no default, since a
/// checkbox cannot say "none"; other types as their own control.
fn default_control(
    m: &Messages,
    def: &SettingDef,
    value: Option<&Value>,
    id: &str,
    name: &str,
) -> String {
    let kind = Kind::of(&def.ty);
    if kind == Kind::Boolean {
        let choices = vec!["true".to_string(), "false".to_string()];
        let as_choice = value
            .and_then(Value::as_bool)
            .map(|b| Value::String(b.to_string()));
        let label = |c: &str| {
            if c == "true" {
                m.get("ts-settings-on")
            } else {
                m.get("ts-settings-off")
            }
        };
        return controls::render(&Control {
            id,
            name,
            kind: Kind::Choice,
            choices: &choices,
            value: as_choice.as_ref().or(value.filter(|v| v.is_string())),
            empty_choice: Some(&m.get("ts-settings-no-default")),
            described_by: None,
            choice_label: Some(&label),
        });
    }
    if kind == Kind::Choice {
        return controls::render(&Control {
            id,
            name,
            kind,
            choices: &def.choices,
            value,
            empty_choice: Some(&m.get("ts-settings-no-default")),
            described_by: None,
            choice_label: None,
        });
    }
    controls::render(&Control {
        id,
        name,
        kind,
        choices: &def.choices,
        value,
        empty_choice: None,
        described_by: None,
        choice_label: None,
    })
}

/// The instance's defaults for wikis, with their locks (0085 §4).
#[allow(clippy::too_many_lines)] // one section of markup, read top to bottom
fn defaults_section(
    m: &Messages,
    loaded: &Loaded,
    tenant_defs: &[SettingDef],
    back: &Returned<'_>,
    saved: Option<&String>,
) -> String {
    let schema = &loaded.schema;
    let locks = schema.template == "locks";
    let editable = loaded.token.is_some() && may_configure(schema, &loaded.settings);
    let mut s = format!(
        "<section class=\"ts-settings__section\" id=\"{DEFAULTS}\" aria-labelledby=\"{DEFAULTS}-title\"><h2 id=\"{DEFAULTS}-title\">{}</h2><p>{}</p>",
        esc(&m.get("ts-settings-group-defaults")),
        esc(&m.get(if locks {
            "ts-settings-defaults-intro-locks"
        } else {
            "ts-settings-defaults-intro"
        }))
    );
    s.push_str(&saved_notice(m, saved, DEFAULTS));
    if back.section == DEFAULTS {
        s.push_str(&errors_html(back.errors.get("")));
    }
    let keys: Vec<&str> = tenant_defs
        .iter()
        .filter(|d| Kind::of(&d.ty) != Kind::ReadOnly)
        .map(|d| d.key.as_str())
        .collect();
    if editable {
        s.push_str(&form_open(
            Scope::Instance,
            DEFAULTS,
            loaded.settings.base,
            loaded.token.as_deref(),
            &keys,
        ));
    }
    for def in tenant_defs {
        let Some(ts) = schema.tenant_settings.get(&def.key) else {
            continue;
        };
        let kind = Kind::of(&def.ty);
        let id = format!("ts-d-{}", def.key);
        let name = format!("d.{}", def.key);
        let label_text = label(m, def);
        let _ = write!(
            s,
            "<div class=\"ts-setting\" id=\"default-{}\"><div class=\"ts-setting__head\">",
            esc(&def.key)
        );
        if editable && kind != Kind::ReadOnly {
            let _ = write!(
                s,
                "<label class=\"ts-setting__label\" for=\"{}\">{}</label>",
                esc(&id),
                esc(&label_text)
            );
        } else {
            let _ = write!(
                s,
                "<span class=\"ts-setting__label\">{}</span>",
                esc(&label_text)
            );
        }
        let _ = write!(
            s,
            " <code class=\"ts-setting__key\">{}</code></div>",
            esc(&def.key)
        );
        s.push_str(&errors_html(back.errors.get(&def.key)));
        let _ = write!(
            s,
            "<p class=\"ts-setting__help\">{}</p>",
            esc(&m.with(
                "ts-settings-registry-default",
                &[&ts
                    .registry_default
                    .as_ref()
                    .map_or_else(|| absent(m, def), |v| controls::shown(m, kind, Some(v)))]
            ))
        );
        s.push_str("<div class=\"ts-setting__value\">");
        if editable && kind != Kind::ReadOnly {
            let typed;
            let value = match back.text(DEFAULTS, &name) {
                Some(raw) => {
                    typed = Value::String(raw.to_string());
                    Some(&typed)
                }
                _ => ts.instance_default.as_ref(),
            };
            s.push_str(&default_control(m, def, value, &id, &name));
            if ts.instance_default.is_some() {
                s.push_str(&reset_button(m, &format!("d.{}", def.key), &label_text));
            }
            if locks {
                let lock_id = format!("ts-l-{}", def.key);
                let checked = match back.sent(DEFAULTS) {
                    Some(f) => f.contains_key(&format!("l.{}", def.key)),
                    None => ts.locked,
                };
                let _ = write!(
                    s,
                    " <span class=\"cdx-checkbox cdx-checkbox--inline\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" id=\"{lid}\" name=\"l.{key}\" value=\"1\"{}><span class=\"cdx-checkbox__icon\"></span><label class=\"cdx-checkbox__label\" for=\"{lid}\">{}</label></span>",
                    if checked { " checked" } else { "" },
                    esc(&m.get("ts-settings-lock")),
                    lid = esc(&lock_id),
                    key = esc(&def.key)
                );
            }
        } else {
            let _ = write!(
                s,
                "<span class=\"ts-setting__shown\">{}</span>",
                esc(&ts.instance_default.as_ref().map_or_else(
                    || m.get("ts-settings-no-default"),
                    |v| controls::shown(m, kind, Some(v))
                ))
            );
            if ts.locked {
                let _ = write!(
                    s,
                    " <span class=\"ts-chip ts-setting__origin ts-setting__origin--locked\">{}</span>",
                    esc(&m.get("ts-settings-origin-locked"))
                );
            }
        }
        if let Some(n) = ts.inheriting {
            let _ = write!(
                s,
                "<p class=\"ts-setting__note\">{}</p>",
                esc(&m.with("ts-settings-inheriting", &[&n.to_string()]))
            );
        }
        s.push_str("</div></div>");
    }
    if editable {
        let summary = back.text(DEFAULTS, "summary").unwrap_or_default();
        s.push_str(&form_close(m, DEFAULTS, summary));
    }
    s.push_str("</section>");
    s
}

/// The page's body and its section links.
fn body(
    cx: &Context,
    scope: Scope,
    loaded: &Loaded,
    tenant_defs: &[SettingDef],
    back: &Returned<'_>,
    saved: Option<&String>,
) -> (String, Vec<Tab>) {
    let m = &cx.m;
    let mut tabs = Vec::new();
    let mut sections = String::new();
    for g in &loaded.schema.groups {
        let title = group_label(m, &g.id, &g.label);
        tabs.push(Tab {
            label: title.clone(),
            href: format!("#{}", g.id),
            current: false,
        });
        sections.push_str(&group_section(m, scope, loaded, &g.id, &title, back, saved));
    }
    if loaded.visibility.is_some() {
        tabs.push(Tab {
            label: m.get("ts-settings-group-visibility"),
            href: format!("#{VISIBILITY}"),
            current: false,
        });
        sections.push_str(&visibility_section(m, cx, loaded, back, saved));
    }
    tabs.push(Tab {
        label: m.get("ts-settings-group-tenancy"),
        href: format!("#{TENANCY}"),
        current: false,
    });
    sections.push_str(&tenancy_section(m, scope, loaded, back, saved));
    if scope == Scope::Instance
        && loaded.schema.template != "none"
        && !loaded.schema.template.is_empty()
    {
        tabs.push(Tab {
            label: m.get("ts-settings-group-defaults"),
            href: format!("#{DEFAULTS}"),
            current: false,
        });
        sections.push_str(&defaults_section(m, loaded, tenant_defs, back, saved));
    }
    let mut out = String::new();
    if loaded.token.is_none() {
        out.push_str(&pages::message(
            MessageKind::Notice,
            &m.get("ts-settings-read-only"),
        ));
    }
    out.push_str(&sections);
    (format!("<div class=\"ts-settings\">{out}</div>"), tabs)
}

/// The tenant settings' definitions, for **Defaults for wikis**: the tenant-scope schema,
/// read at the farm base, where the farm base is also the primary tenant's base.
async fn tenant_defs(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    scope: Scope,
    loaded: &Loaded,
) -> Result<Vec<SettingDef>, ClientError> {
    if scope != Scope::Instance || loaded.schema.template == "none" {
        return Ok(Vec::new());
    }
    let s = site.client().settings_schema(incoming, false).await?;
    cx.inputs.push(s.cache);
    let mut defs = s.value.settings;
    offer_themes(&mut defs, &cx.theme);
    Ok(defs)
}

/// The context, or the response that ends the request.
async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
    scope: Scope,
) -> Result<Context, Response> {
    let cx = pages::context(site, incoming, query).await?;
    let served = cx.features.has(Feature::Settings) && (scope == Scope::Wiki || cx.site.at_farm);
    if !served {
        return Err(pages::no_page(&cx, headers, scope.title().to_string()));
    }
    Ok(cx)
}

#[allow(clippy::too_many_arguments)]
async fn render_page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    scope: Scope,
    back: &Returned<'_>,
    saved: Option<&String>,
    status: StatusCode,
) -> Response {
    let loaded = match load(site, cx, incoming, scope).await {
        Ok(l) => l,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let defs = match tenant_defs(site, cx, incoming, scope, &loaded).await {
        Ok(d) => d,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let (html, tabs) = body(cx, scope, &loaded, &defs, back, saved);
    let page = pages::respond(cx, headers, status, |m| Page {
        title: m.get(match scope {
            Scope::Wiki => "ts-wikisettings-title",
            Scope::Instance => "ts-instancesettings-title",
        }),
        returnto: Some(scope.title().to_string()),
        tabs,
        body: html,
        ..Page::default()
    });
    pages::with_cookies(page, &loaded.cookies)
}

/// `GET Special:WikiSettings` or `Special:InstanceSettings`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    scope: Scope,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map, scope).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    render_page(
        site,
        &mut cx,
        headers,
        &incoming,
        scope,
        &Returned::default(),
        params.map.get("saved"),
        StatusCode::OK,
    )
    .await
}

/// `GET Special:Tenancy`: to the Tenancy section of the page this base serves (0085 §4).
pub async fn tenancy(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Settings) {
        return pages::no_page(&cx, headers, "Special:Tenancy".into());
    }
    let target = if cx.site.at_farm {
        Scope::Instance
    } else {
        Scope::Wiki
    };
    let mut r = pages::redirect_to(&cx, target.title());
    if let Some(loc) = r
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        && let Ok(v) = HeaderValue::from_str(&format!("{loc}#{TENANCY}"))
    {
        r.headers_mut().insert(axum::http::header::LOCATION, v);
    }
    r
}

type Errors = BTreeMap<String, Vec<String>>;

/// The tenancy switches the reader changed.
fn tenancy_patch(
    f: &BTreeMap<String, String>,
    keys: &[&str],
    loaded: &Loaded,
) -> Map<String, Value> {
    let mut tenancy = Map::new();
    for name in keys {
        let Some(sw) = loaded.schema.tenancy.get(*name) else {
            continue;
        };
        let raw = f
            .get(&format!("t.{name}"))
            .map(|s| s.trim())
            .unwrap_or_default();
        if !raw.is_empty() && sw.value.as_ref().and_then(Value::as_str) != Some(raw) {
            tenancy.insert((*name).to_string(), json!(raw));
        }
    }
    tenancy
}

/// A default as posted: a boolean comes as `true`, `false` or nothing.
fn default_value(kind: Kind, raw: Option<&str>) -> Result<Option<Value>, String> {
    match (kind, raw.map(str::trim)) {
        (_, None | Some("")) => Ok(None),
        (Kind::Boolean, Some("true")) => Ok(Some(Value::Bool(true))),
        (Kind::Boolean, Some("false")) => Ok(Some(Value::Bool(false))),
        _ => controls::parse(kind, raw),
    }
}

/// The defaults and locks the reader changed.
fn defaults_patch(
    f: &BTreeMap<String, String>,
    keys: &[&str],
    loaded: &Loaded,
    tenant_defs: &[SettingDef],
    errors: &mut Errors,
) -> (Map<String, Value>, Map<String, Value>) {
    let (mut defaults, mut locks) = (Map::new(), Map::new());
    if let Some(key) = f.get("reset").and_then(|r| r.strip_prefix("d.")) {
        defaults.insert(key.to_string(), Value::Null);
        return (defaults, locks);
    }
    for key in keys {
        let Some(def) = tenant_defs.iter().find(|d| d.key == *key) else {
            continue;
        };
        let ts = loaded
            .schema
            .tenant_settings
            .get(*key)
            .cloned()
            .unwrap_or_default();
        let kind = Kind::of(&def.ty);
        match default_value(kind, f.get(&format!("d.{key}")).map(String::as_str)) {
            Err(e) => errors.entry((*key).to_string()).or_default().push(e),
            Ok(None) => {
                if ts.instance_default.is_some() && kind != Kind::Lines {
                    defaults.insert((*key).to_string(), Value::Null);
                }
            }
            Ok(Some(v)) => {
                if ts.instance_default.as_ref() != Some(&v) {
                    defaults.insert((*key).to_string(), v);
                }
            }
        }
        if loaded.schema.template == "locks" {
            let lock = f.contains_key(&format!("l.{key}"));
            if lock != ts.locked {
                locks.insert((*key).to_string(), Value::Bool(lock));
            }
        }
    }
    (defaults, locks)
}

/// The settings of a group section the reader changed.
fn group_patch(
    f: &BTreeMap<String, String>,
    keys: &[&str],
    loaded: &Loaded,
    errors: &mut Errors,
) -> Map<String, Value> {
    let mut set = Map::new();
    for key in keys {
        let Some(def) = loaded.schema.settings.iter().find(|d| d.key == *key) else {
            continue;
        };
        let kind = Kind::of(&def.ty);
        let current = loaded
            .settings
            .settings
            .get(*key)
            .and_then(|r| r.value.clone());
        match controls::parse(kind, f.get(&format!("v.{key}")).map(String::as_str)) {
            Err(e) => errors.entry((*key).to_string()).or_default().push(e),
            Ok(None) => {}
            Ok(Some(v)) => {
                // An absent value compares equal to the empty list, or the unticked box,
                // that stands for it on the page.
                let unchanged = current.as_ref() == Some(&v)
                    || (current.is_none() && (v == json!([]) || v == json!(false)));
                if !unchanged {
                    set.insert((*key).to_string(), v);
                }
            }
        }
    }
    set
}

/// The body of a section's save, and the reasons the form itself is not a save.
fn patch_of(
    form: &Params,
    loaded: &Loaded,
    tenant_defs: &[SettingDef],
) -> (Map<String, Value>, Errors) {
    let f = &form.map;
    let section = f.get("section").map_or("", String::as_str);
    let keys: Vec<&str> = f
        .get("keys")
        .map_or_else(Vec::new, |k| k.split_whitespace().collect());
    let mut errors = Errors::new();
    let mut body = Map::new();
    if let Some(t) = f.get("token") {
        body.insert("token".into(), json!(t));
    }
    if let Some(b) = f.get("base").and_then(|b| b.parse::<u64>().ok()) {
        body.insert("base".into(), json!(b));
    }
    if let Some(s) = f.get("summary").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        body.insert("summary".into(), json!(s));
    }
    let mut put = |name: &str, m: Map<String, Value>| {
        if !m.is_empty() {
            body.insert(name.into(), Value::Object(m));
        }
    };
    match section {
        TENANCY => put("tenancy", tenancy_patch(f, &keys, loaded)),
        DEFAULTS => {
            let (defaults, locks) = defaults_patch(f, &keys, loaded, tenant_defs, &mut errors);
            put("defaults", defaults);
            put("locks", locks);
        }
        _ => {
            if let Some(key) = f.get("reset") {
                body.insert("unset".into(), json!([key]));
            } else {
                put("set", group_patch(f, &keys, loaded, &mut errors));
            }
        }
    }
    (body, errors)
}

/// `POST Special:WikiSettings` or `Special:InstanceSettings`: saves one section.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    scope: Scope,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, scope).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let section = form.map.get("section").cloned().unwrap_or_default();
    let loaded = match load(site, &mut cx, &incoming, scope).await {
        Ok(l) => l,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    if section == VISIBILITY && scope == Scope::Wiki {
        return post_visibility(site, &mut cx, headers, &incoming, loaded, form).await;
    }
    let defs = match tenant_defs(site, &mut cx, &incoming, scope, &loaded).await {
        Ok(d) => d,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let (body, errors) = patch_of(form, &loaded, &defs);
    let back_to = |saved: bool| {
        format!(
            "/wiki/{}{}#{}",
            title_url(scope.title()),
            if saved {
                format!("?saved={section}")
            } else {
                String::new()
            },
            section
        )
    };
    if !errors.is_empty() {
        let back = Returned {
            section: section.clone(),
            form: Some(form),
            errors,
        };
        return render_page(
            site,
            &mut cx,
            headers,
            &incoming,
            scope,
            &back,
            None,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    let changes = ["set", "unset", "defaults", "locks", "tenancy"];
    if !changes.iter().any(|k| body.contains_key(*k)) {
        return pages::see_other(&back_to(false), &loaded.cookies);
    }
    match site
        .client()
        .save_settings(&incoming, scope.instance(), &Value::Object(body))
        .await
    {
        Ok(Saved::Ok(_)) => pages::see_other(&back_to(true), &loaded.cookies),
        Ok(Saved::Refused(refusals)) => {
            let back = Returned {
                section: section.clone(),
                form: Some(form),
                errors: refusal_errors(&cx.m, &refusals),
            };
            render_page(
                site,
                &mut cx,
                headers,
                &incoming,
                scope,
                &back,
                None,
                StatusCode::BAD_REQUEST,
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// The Visibility form: `PUT` or `DELETE /acl/tenant/{slug}` (09 §4.7).
async fn post_visibility(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    loaded: Loaded,
    form: &Params,
) -> Response {
    let field = |k: &str| form.map.get(k).map_or("", String::as_str);
    let group = match field("visibility") {
        "user" => Some("user"),
        "group" => Some(field("group").trim()).filter(|g| !g.is_empty()),
        _ => None,
    };
    if field("visibility") == "group" && group.is_none() {
        let back = Returned {
            section: VISIBILITY.into(),
            form: Some(form),
            errors: BTreeMap::from([(
                String::new(),
                vec![cx.m.get("ts-settings-visibility-no-group")],
            )]),
        };
        return render_page(
            site,
            cx,
            headers,
            incoming,
            Scope::Wiki,
            &back,
            None,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    let back_to = format!(
        "/wiki/{}?saved={VISIBILITY}#{VISIBILITY}",
        title_url(Scope::Wiki.title())
    );
    match site
        .client()
        .set_tenant_visibility(
            incoming,
            &cx.site.tenant,
            group,
            field("reason").trim(),
            field("token"),
        )
        .await
    {
        Ok(Saved::Ok(_)) => pages::see_other(&back_to, &loaded.cookies),
        Ok(Saved::Refused(refusals)) => {
            let errors = refusals
                .iter()
                .map(|r| match r.code.as_str() {
                    "ts-locked-out" => cx.m.get("ts-protect-locked-out"),
                    "ts-policy" => cx.m.get("ts-protect-policy"),
                    "ts-unknown-group" => cx.m.get("ts-protect-unknown-group"),
                    "badtoken" => cx.m.get("ts-login-expired"),
                    "permissiondenied" => cx.m.get("ts-settings-visibility-denied"),
                    _ => r.message.clone(),
                })
                .collect();
            let back = Returned {
                section: VISIBILITY.into(),
                form: Some(form),
                errors: BTreeMap::from([(String::new(), errors)]),
            };
            render_page(
                site,
                cx,
                headers,
                incoming,
                Scope::Wiki,
                &back,
                None,
                StatusCode::BAD_REQUEST,
            )
            .await
        }
        Err(e) => pages::api_failed(cx, headers, &e),
    }
}

/// The API's reasons by key, with the ones the reader can act on in their own words.
fn refusal_errors(m: &Messages, refusals: &[Refusal]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in refusals {
        let text = match r.code.as_str() {
            "editconflict" => m.get("ts-settings-conflict"),
            "badtoken" => m.get("ts-login-expired"),
            "permissiondenied" => m.get("ts-settings-denied"),
            "ts-locked" => m.get("ts-settings-locked"),
            _ => r.message.clone(),
        };
        out.entry(r.key.clone().unwrap_or_default())
            .or_default()
            .push(text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_setting_and_preference_has_its_messages() {
        let m = Messages::for_language("en");
        let settings: toml::Table =
            toml::from_str(include_str!("../../../../docs/registry/settings.toml")).unwrap();
        for s in settings["setting"].as_array().unwrap() {
            let key = s["key"].as_str().unwrap();
            let label = format!("ts-setting-{}", key.replace('.', "-"));
            assert!(m.has(&label), "{label}");
            assert!(m.has(&format!("{label}-help")), "{label}-help");
        }
        for g in settings["group"].as_array().unwrap() {
            let key = format!("ts-settings-group-{}", g["id"].as_str().unwrap());
            assert!(m.has(&key), "{key}");
        }
        let prefs: toml::Table =
            toml::from_str(include_str!("../../../../docs/registry/preferences.toml")).unwrap();
        for p in prefs["preference"].as_array().unwrap() {
            if p.get("pattern").and_then(toml::Value::as_bool) == Some(true) {
                continue;
            }
            let key = format!("ts-pref-{}", p["key"].as_str().unwrap().replace('.', "-"));
            assert!(m.has(&key), "{key}");
        }
        for g in prefs["group"].as_array().unwrap() {
            let key = format!("ts-prefs-group-{}", g["id"].as_str().unwrap());
            assert!(m.has(&key), "{key}");
        }
    }

    #[test]
    fn markdown_to_plain_text() {
        assert_eq!(
            plain("The logo in [07](07-actors.md) §2 and `general.logo`"),
            "The logo in 07 §2 and general.logo"
        );
        assert_eq!(plain("a [b] c"), "a [b] c");
    }

    #[test]
    fn a_tenancy_save_sends_only_changed_switches() {
        let loaded = Loaded {
            schema: Schema {
                tenancy: BTreeMap::from([
                    (
                        "config.template".to_string(),
                        triplespace_client::settings::Switch {
                            allowed: triplespace_client::settings::Allowed {
                                choices: vec!["none".into(), "defaults".into(), "locks".into()],
                                text: None,
                            },
                            value: Some(json!("none")),
                        },
                    ),
                    (
                        "wikitext.ceiling".to_string(),
                        triplespace_client::settings::Switch {
                            allowed: triplespace_client::settings::Allowed::default(),
                            value: Some(json!("lua")),
                        },
                    ),
                ]),
                ..Schema::default()
            },
            settings: Settings::default(),
            token: Some("t".into()),
            cookies: Vec::new(),
            visibility: None,
        };
        let form = Params::new(vec![
            ("section".into(), TENANCY.into()),
            ("keys".into(), "config.template wikitext.ceiling".into()),
            ("t.config.template".into(), "defaults".into()),
            ("t.wikitext.ceiling".into(), "lua".into()),
            ("token".into(), "t".into()),
        ]);
        let (body, errors) = patch_of(&form, &loaded, &[]);
        assert!(errors.is_empty());
        assert_eq!(body["tenancy"], json!({"config.template": "defaults"}));
        assert_eq!(body["token"], "t");
    }
}
