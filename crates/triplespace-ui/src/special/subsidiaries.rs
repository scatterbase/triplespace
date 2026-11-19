//! `Special:Account`'s Subsidiaries section (0024 §9; 19 §2.4), without JavaScript.
//!
//! The list of one's subsidiaries is public, as the operator relation is; each one's keys
//! are private: label, grants, expiry, last used, and Revoke. **Issue key** shows the
//! secret once, on the page that answers the form, with a warning that it will not be
//! shown again; the page is not a redirect, so the secret is in no URL and no history
//! entry. **Rename** and **Retire** (or **Reactivate**) sit with each subsidiary, and
//! **Create subsidiary** suggests `{Name}Bot`. Every form posts back to the page with
//! `subsidiary_action`, and the API is `/account/subsidiaries` (18 §3.2).

use std::collections::BTreeMap;

use serde_json::Value;
use triplespace_client::{Subsidiaries, SubsidiaryListing};

use crate::codex::{Action, Button, ButtonModel, Field, FieldModel, Weight};
use crate::frame::title_url;
use crate::i18n::Messages;
use crate::special::external::iso_date;

/// The section as data: `special/account.html` draws it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SubsidiariesSection {
    /// The heading.
    pub heading: String,
    /// That the list is public.
    pub public: String,
    /// That the keys are private.
    pub private: String,
    /// What a subsidiary is for.
    pub intro: String,
    /// Where the forms post.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// A key just issued, shown once.
    pub issued: Option<IssuedKey>,
    /// The subsidiaries.
    pub items: Vec<SubsidiaryItem>,
    /// What the list says when there are none.
    pub empty: Option<String>,
    /// The form that creates one, where the reader may.
    pub create: Option<CreateForm>,
    /// Why there is no create form: the limit, or no `createaccount`.
    pub create_note: Option<String>,
}

/// A key just issued.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IssuedKey {
    /// The heading.
    pub heading: String,
    /// That the secret will not be shown again.
    pub warning: String,
    /// Label and value pairs: the bot-password login name, the password, the bearer key.
    pub rows: Vec<(String, String)>,
}

/// The create form.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CreateForm {
    /// The name field, holding the suggestion.
    pub field: FieldModel,
    /// **Create subsidiary**.
    pub button: ButtonModel,
}

/// One subsidiary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SubsidiaryItem {
    /// Its name.
    pub name: String,
    /// Its user page.
    pub href: String,
    /// The element ID's suffix.
    pub slug: String,
    /// Its state: bot flag, awaiting the flag, pending or retired.
    pub state: String,
    /// Whether it is retired.
    pub retired: bool,
    /// The keys' heading.
    pub keys_heading: String,
    /// Its keys, unrevoked first.
    pub keys: Vec<KeyItem>,
    /// What the key list says when there are none.
    pub no_keys: Option<String>,
    /// The issue form, for an unretired subsidiary.
    pub issue: Option<IssueForm>,
    /// The rename form, for an unretired subsidiary.
    pub rename: Option<RenameForm>,
    /// **Retire**, or **Reactivate** for a retired one.
    pub status_button: ButtonModel,
    /// What the status button does.
    pub status_action: String,
    /// What retiring does, or what reactivating needs.
    pub status_note: String,
}

/// One key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct KeyItem {
    /// Its ID, which the revoke form names.
    pub key_id: String,
    /// The label, and the bot-password login name it gives.
    pub label: String,
    /// `{name}@{label}`.
    pub login: String,
    /// The grants, comma-separated.
    pub grants: String,
    /// Created, expires and last used.
    pub dates: String,
    /// When it was revoked; then there is no Revoke.
    pub revoked: Option<String>,
    /// **Revoke**.
    pub revoke: Option<ButtonModel>,
}

/// The issue form.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IssueForm {
    /// Its heading.
    pub heading: String,
    /// The label field.
    pub label: FieldModel,
    /// The grants' legend.
    pub grants_legend: String,
    /// The grants' note: `basic` is always included.
    pub grants_note: String,
    /// One checkbox per grant.
    pub grants: Vec<GrantBox>,
    /// The expiry field.
    pub expires: FieldModel,
    /// **Issue key**.
    pub button: ButtonModel,
}

/// One grant's checkbox.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GrantBox {
    /// The input's ID.
    pub id: String,
    /// The input's name, `grant_{name}`: each its own field, since the form's fields are
    /// read by name.
    pub name: String,
    /// The grant's name.
    pub label: String,
    /// What it is for.
    pub description: Option<String>,
}

/// The rename form.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RenameForm {
    /// The new name's field.
    pub field: FieldModel,
    /// **Rename**.
    pub button: ButtonModel,
}

/// An element ID's suffix for a name: letters, digits and hyphens.
fn slug(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn button(m: &Messages, key: &str, action: Action, weight: Weight) -> ButtonModel {
    Button {
        label: &m.get(key),
        kind: "submit",
        action,
        weight,
    }
    .model()
}

fn state(m: &Messages, s: &SubsidiaryListing) -> String {
    m.get(match s.status.as_str() {
        "retired" => "ts-account-subsidiary-retired",
        "pending" => "ts-account-subsidiary-pending",
        _ if s.groups.iter().any(|g| g == "bot") => "ts-account-subsidiary-flagged",
        _ => "ts-account-subsidiary-unflagged",
    })
}

/// One subsidiary's part of the section.
#[allow(clippy::too_many_lines)]
fn item(m: &Messages, all: &Subsidiaries, s: &SubsidiaryListing) -> SubsidiaryItem {
    let retired = s.status == "retired";
    let id = slug(&s.name);
    let mut keys: Vec<KeyItem> = s
        .keys
        .iter()
        .map(|k| {
            let mut dates = vec![m.with("ts-account-added", &[&iso_date(k.created)])];
            if let Some(e) = k.expires {
                dates.push(m.with("ts-account-key-expires", &[&iso_date(e)]));
            }
            dates.push(k.last_used.map_or_else(
                || m.get("ts-account-never-used"),
                |t| m.with("ts-account-last-used", &[&iso_date(t)]),
            ));
            KeyItem {
                key_id: k.key_id.clone(),
                label: k.label.clone(),
                login: format!("{}@{}", s.name, k.label),
                grants: k.grants.join(", "),
                dates: dates.join(" · "),
                revoked: k
                    .revoked
                    .map(|t| m.with("ts-account-key-revoked", &[&iso_date(t)])),
                revoke: k.revoked.is_none().then(|| {
                    button(
                        m,
                        "ts-account-key-revoke",
                        Action::Destructive,
                        Weight::Normal,
                    )
                }),
            }
        })
        .collect();
    keys.sort_by_key(|k| k.revoked.is_some());
    let field = |id: &str, name: &str, kind: &str, label: &str, help: Option<&str>, required| {
        Field {
            id,
            name,
            kind,
            value: "",
            label,
            description: None,
            help,
            required,
            autocomplete: Some("off"),
        }
        .model()
    };
    let issue = (!retired).then(|| IssueForm {
        heading: m.get("ts-account-key-issue-heading"),
        label: field(
            &format!("ts-key-label-{id}"),
            "label",
            "text",
            &m.get("ts-account-key-label"),
            Some(&m.get("ts-account-key-label-help")),
            true,
        ),
        grants_legend: m.get("ts-account-key-grants"),
        grants_note: m.get("ts-account-key-grants-note"),
        grants: all
            .grants
            .iter()
            .map(|g| GrantBox {
                id: format!("ts-key-grant-{id}-{}", slug(&g.name)),
                name: format!("grant_{}", g.name),
                label: g.name.clone(),
                description: g.description.clone(),
            })
            .collect(),
        expires: field(
            &format!("ts-key-expires-{id}"),
            "expires_days",
            "number",
            &m.get("ts-account-key-expires-label"),
            Some(&m.get("ts-account-key-expires-help")),
            false,
        ),
        button: button(
            m,
            "ts-account-key-issue",
            Action::Progressive,
            Weight::Primary,
        ),
    });
    let rename = (!retired).then(|| RenameForm {
        field: field(
            &format!("ts-subsidiary-rename-{id}"),
            "to",
            "text",
            &m.get("ts-account-subsidiary-rename-label"),
            None,
            true,
        ),
        button: button(
            m,
            "ts-account-subsidiary-rename",
            Action::Default,
            Weight::Normal,
        ),
    });
    SubsidiaryItem {
        name: s.name.clone(),
        href: format!("/wiki/{}", title_url(&format!("User:{}", s.name))),
        slug: id,
        state: state(m, s),
        retired,
        keys_heading: m.get("ts-account-keys-heading"),
        no_keys: keys.is_empty().then(|| m.get("ts-account-keys-none")),
        keys,
        issue,
        rename,
        status_button: if retired {
            button(
                m,
                "ts-account-subsidiary-reactivate",
                Action::Default,
                Weight::Normal,
            )
        } else {
            button(
                m,
                "ts-account-subsidiary-retire",
                Action::Destructive,
                Weight::Normal,
            )
        },
        status_action: if retired { "reactivate" } else { "retire" }.to_string(),
        status_note: m.get(if retired {
            "ts-account-subsidiary-reactivate-note"
        } else {
            "ts-account-subsidiary-retire-note"
        }),
    }
}

/// The section's model. `issued` is the API's answer to an issue, shown once.
#[must_use]
pub fn model(
    m: &Messages,
    s: &Subsidiaries,
    action: &str,
    token: &str,
    issued: Option<&Value>,
) -> SubsidiariesSection {
    let at_limit = s.held >= s.max;
    let create = (s.can_create && !at_limit).then(|| CreateForm {
        field: Field {
            id: "ts-subsidiary-name",
            name: "name",
            kind: "text",
            value: &s.suggested,
            label: &m.get("ts-account-subsidiary-name"),
            description: None,
            help: Some(&m.get("ts-account-subsidiary-name-help")),
            required: true,
            autocomplete: Some("off"),
        }
        .model(),
        button: button(
            m,
            "ts-account-subsidiary-create",
            Action::Progressive,
            Weight::Normal,
        ),
    });
    let create_note = if !s.can_create {
        Some(m.get("ts-account-subsidiary-cannot-create"))
    } else if at_limit {
        Some(m.with("ts-account-subsidiary-limit", &[&s.max.to_string()]))
    } else {
        None
    };
    SubsidiariesSection {
        heading: m.get("ts-account-subsidiaries-heading"),
        public: m.get("ts-account-public"),
        private: m.get("ts-account-private"),
        intro: m.get("ts-account-subsidiaries-intro"),
        action: action.to_string(),
        token: token.to_string(),
        issued: issued.map(|i| {
            let text = |k: &str| i[k].as_str().unwrap_or_default().to_string();
            IssuedKey {
                heading: m.with("ts-account-key-issued", &[&text("label")]),
                warning: m.get("ts-account-key-issued-warning"),
                rows: vec![
                    (m.get("ts-account-key-issued-login"), text("login")),
                    (m.get("ts-account-key-issued-password"), text("secret")),
                    (m.get("ts-account-key-issued-bearer"), text("bearer")),
                ],
            }
        }),
        items: s.subsidiaries.iter().map(|x| item(m, s, x)).collect(),
        empty: s
            .subsidiaries
            .is_empty()
            .then(|| m.get("ts-account-subsidiaries-none")),
        create,
        create_note,
    }
}

/// The message for a refusal the API answered a subsidiary write with.
#[must_use]
pub fn refusal(m: &Messages, code: &str, body: &Value) -> String {
    match code {
        "ts-subsidiary-limit" => m.get("ts-account-subsidiary-at-limit"),
        "ts-bad-key" => m.with(
            "ts-account-key-refused",
            &[body["message"].as_str().unwrap_or_default()],
        ),
        "ts-subsidiary-retired" => m.get("ts-account-subsidiary-is-retired"),
        "invaliduser" => m.get("ts-account-subsidiary-invalid"),
        "userexists" => m.get("ts-account-subsidiary-taken"),
        "ts-name-reserved" => m.get("ts-account-subsidiary-reserved"),
        "badtoken" => m.get("ts-login-expired"),
        "permissiondenied" => m.get("ts-account-subsidiary-cannot-create"),
        _ => m.get("ts-account-subsidiary-failed"),
    }
}

/// What a posted form asks: the action, and the API call's arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Create one named so.
    Create(String),
    /// Rename `name` to `to`.
    Rename {
        /// The subsidiary.
        name: String,
        /// Its new name.
        to: String,
    },
    /// Retire or reactivate `name`.
    Status {
        /// The subsidiary.
        name: String,
        /// `retire` or `reactivate`.
        action: String,
    },
    /// Issue a key.
    Issue {
        /// The subsidiary.
        name: String,
        /// The key's label.
        label: String,
        /// The grants ticked.
        grants: Vec<String>,
        /// Days until it expires.
        expires_days: Option<u64>,
    },
    /// Revoke a key.
    Revoke {
        /// The subsidiary.
        name: String,
        /// The key.
        key_id: String,
    },
}

/// Reads a posted form; `None` when it asks nothing this section knows.
#[must_use]
pub fn ask(form: &BTreeMap<String, String>) -> Option<Ask> {
    let field = |k: &str| {
        form.get(k)
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    };
    let name = field("subsidiary");
    Some(match form.get("subsidiary_action")?.as_str() {
        "create" => Ask::Create(field("name")),
        "rename" => Ask::Rename {
            name,
            to: field("to"),
        },
        a @ ("retire" | "reactivate") => Ask::Status {
            name,
            action: a.to_string(),
        },
        "issue" => Ask::Issue {
            name,
            label: field("label"),
            grants: form
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .filter_map(|(k, _)| k.strip_prefix("grant_").map(str::to_string))
                .collect(),
            expires_days: field("expires_days").parse().ok(),
        },
        "revoke" => Ask::Revoke {
            name,
            key_id: field("key_id"),
        },
        _ => return None,
    })
}

/// The notice a successful write returns to the page with.
#[must_use]
pub fn notice(a: &Ask) -> &'static str {
    match a {
        Ask::Create(_) => "ts-subsidiary-created",
        Ask::Rename { .. } => "ts-subsidiary-renamed",
        Ask::Status { action, .. } if action == "retire" => "ts-subsidiary-retired",
        Ask::Status { .. } => "ts-subsidiary-reactivated",
        Ask::Issue { .. } => "ts-key-issued",
        Ask::Revoke { .. } => "ts-key-revoked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::{GrantListing, KeyListing};

    fn listing() -> Subsidiaries {
        Subsidiaries {
            subsidiaries: vec![
                SubsidiaryListing {
                    name: "Alice Bot".into(),
                    actor: "lb:8".into(),
                    status: "active".into(),
                    groups: vec!["bot".into()],
                    keys: vec![
                        KeyListing {
                            key_id: "old".into(),
                            label: "laptop".into(),
                            grants: vec!["basic".into()],
                            created: 1_792_281_600,
                            expires: None,
                            last_used: None,
                            revoked: Some(1_792_281_600),
                        },
                        KeyListing {
                            key_id: "k1".into(),
                            label: "toolforge".into(),
                            grants: vec!["basic".into(), "editentity".into()],
                            created: 1_792_281_600,
                            expires: Some(1_792_281_600),
                            last_used: Some(1_792_281_600),
                            revoked: None,
                        },
                    ],
                },
                SubsidiaryListing {
                    name: "OldBot".into(),
                    actor: "lb:9".into(),
                    status: "retired".into(),
                    groups: vec![],
                    keys: vec![],
                },
            ],
            can_create: true,
            max: 10,
            held: 1,
            suggested: "AliceBot".into(),
            grants: vec![GrantListing {
                name: "editentity".into(),
                description: Some("Edit entities".into()),
            }],
        }
    }

    fn drawn(m: &Messages, section: SubsidiariesSection) -> String {
        let skins = crate::skin::Skins::load(None);
        let plain = skins.get("plain").expect("plain");
        let page = crate::special::external::AccountPage {
            notice: None,
            login_needed: None,
            login: None,
            methods: None,
            email: None,
            subsidiaries: Some(section),
        };
        plain.render("special/account.html", m, page).unwrap()
    }

    /// 0024 §9 (19 §2.4): the list is public and the keys private; an unrevoked key has
    /// Revoke and a revoked one says when; the issue form offers each grant as its own
    /// field; a retired subsidiary has Reactivate and no issue or rename form; Create
    /// suggests `{Name}Bot`.
    #[test]
    fn the_subsidiaries_section() {
        let m = Messages::for_language("en");
        let html = drawn(
            &m,
            model(
                &m,
                &listing(),
                "/w/index.php?title=Special:Account",
                "t&k",
                None,
            ),
        );
        assert!(html.contains("id=\"subsidiaries\""), "{html}");
        assert!(html.contains(">Public</span>"), "{html}");
        assert!(html.contains(">Private</span>"), "{html}");
        assert!(html.contains("href=\"/wiki/User:Alice_Bot\""), "{html}");
        assert!(html.contains("Alice Bot@toolforge"), "{html}");
        assert!(html.contains("revoked 2026-10-18"), "{html}");
        assert_eq!(
            html.matches("name=\"subsidiary_action\" value=\"revoke\"")
                .count(),
            1
        );
        assert!(html.contains("name=\"grant_editentity\""), "{html}");
        assert_eq!(html.matches("value=\"issue\"").count(), 1, "{html}");
        assert_eq!(html.matches("value=\"rename\"").count(), 1, "{html}");
        assert!(html.contains("value=\"reactivate\""), "{html}");
        assert!(html.contains("value=\"AliceBot\""), "{html}");
        assert!(html.contains("value=\"t&amp;k\""), "{html}");
        assert!(
            !html.contains("cdx-message--warning"),
            "no key shown: {html}"
        );

        // At the limit, the form gives way to the note; a key just issued is shown once.
        let mut full = listing();
        full.held = 10;
        let issued = serde_json::json!({
            "label": "toolforge", "login": "Alice Bot@toolforge",
            "secret": "s3cr3t<", "bearer": "k1.s3cr3t<"
        });
        let html = drawn(&m, model(&m, &full, "/x", "t", Some(&issued)));
        assert!(!html.contains("value=\"create\""), "{html}");
        assert!(
            html.contains("the most subsidiaries this wiki allows, 10"),
            "{html}"
        );
        assert!(html.contains("<code>s3cr3t&lt;</code>"), "{html}");
        assert!(html.contains("shown only this once"), "{html}");
    }

    #[test]
    fn forms_are_read_by_name() {
        let f = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(
            ask(&f(&[
                ("subsidiary_action", "issue"),
                ("subsidiary", "Alice Bot"),
                ("label", " laptop "),
                ("grant_editentity", "1"),
                ("grant_patrol", "1"),
                ("expires_days", "30"),
            ])),
            Some(Ask::Issue {
                name: "Alice Bot".into(),
                label: "laptop".into(),
                grants: vec!["editentity".into(), "patrol".into()],
                expires_days: Some(30),
            })
        );
        assert_eq!(ask(&f(&[("subsidiary_action", "drop")])), None);
        assert_eq!(
            notice(&ask(&f(&[("subsidiary_action", "retire"), ("subsidiary", "B")])).unwrap()),
            "ts-subsidiary-retired"
        );
    }
}
