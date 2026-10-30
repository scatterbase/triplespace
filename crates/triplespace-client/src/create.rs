//! Creating an entity (`Special:NewItem`, `Special:NewProperty`; 0047 §5): one
//! `wbeditentity` with `new=`, posted with the viewer's cookie and CSRF token, through the
//! same handler as every other client's (0034 §1.5).

use serde_json::Value;

use crate::api::check;
use crate::transport::ClientError;
use crate::{Client, Incoming};

/// The entity a `wbeditentity` with `new=` created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    /// Its ID.
    pub id: String,
    /// Its entity type (`item`, `property`).
    pub entity_type: String,
}

impl Client {
    /// `action=wbeditentity&new={kind}` with `data` (Wikibase JSON: `labels`,
    /// `descriptions`, `aliases`, `claims`, `datatype`) and the CSRF token. A refusal is
    /// [`ClientError::Api`] with the API's code (`badtoken`, `permissiondenied`,
    /// `modification-failed`, …).
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn create_entity(
        &self,
        incoming: &Incoming,
        kind: &str,
        data: &Value,
        token: &str,
    ) -> Result<Created, ClientError> {
        let data = data.to_string();
        let r = self
            .post_form(
                incoming,
                "/w/api.php",
                &[
                    ("action", "wbeditentity"),
                    ("format", "json"),
                    ("formatversion", "2"),
                    ("new", kind),
                    ("data", &data),
                    ("token", token),
                ],
            )
            .await?;
        created(&check(&r)?)
    }

    /// `meta=userinfo&uiprop=rights`: the viewer's rights, for a page that needs one the
    /// frame does not ask about (`property-create`).
    ///
    /// # Errors
    ///
    /// The API away or unreadable.
    pub async fn rights(&self, incoming: &Incoming) -> Result<Vec<String>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&meta=userinfo&uiprop=rights&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        Ok(v["query"]["userinfo"]["rights"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// The created entity, from `wbeditentity`'s answer.
fn created(v: &Value) -> Result<Created, ClientError> {
    let e = &v["entity"];
    let id = e["id"]
        .as_str()
        .ok_or_else(|| ClientError::Unexpected("wbeditentity named no entity".into()))?;
    Ok(Created {
        id: id.to_string(),
        entity_type: e["type"].as_str().unwrap_or("item").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_created_entity() {
        let v = serde_json::json!({"entity": {"id": "P13", "type": "property"}, "success": 1});
        assert_eq!(
            created(&v).unwrap(),
            Created {
                id: "P13".into(),
                entity_type: "property".into()
            }
        );
        assert!(created(&serde_json::json!({"success": 1})).is_err());
    }
}

/// What a term form sets on an entity in one language (`Special:SetLabel` and its
/// siblings; 0047 §5). An empty value removes the term; an empty list of aliases removes
/// the language's aliases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEdit {
    /// `wbsetlabel`.
    Label(String),
    /// `wbsetdescription`.
    Description(String),
    /// `wbsetaliases` with `set`: the language's aliases become these.
    Aliases(Vec<String>),
    /// One `wbeditentity`, so one revision, as Wikibase's `SetLabelDescriptionAliases`
    /// makes: the label (`None` for a keyed entity, whose label is its key), the
    /// description and the aliases.
    All {
        /// The label.
        label: Option<String>,
        /// The description.
        description: String,
        /// The aliases.
        aliases: Vec<String>,
    },
}

impl Client {
    /// Sets an entity's terms in one language, with the viewer's cookie and CSRF token,
    /// through the same modules as every other client. A refusal is [`ClientError::Api`]
    /// with the API's code (`badtoken`, `permissiondenied`, `no-such-entity`,
    /// `derived-label`, …).
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn set_terms(
        &self,
        incoming: &Incoming,
        id: &str,
        language: &str,
        edit: &TermEdit,
        token: &str,
    ) -> Result<(), ClientError> {
        let mut fields: Vec<(&str, String)> = vec![
            ("format", "json".into()),
            ("formatversion", "2".into()),
            ("id", id.to_string()),
        ];
        match edit {
            TermEdit::Label(v) => {
                fields.push(("action", "wbsetlabel".into()));
                fields.push(("language", language.to_string()));
                fields.push(("value", v.clone()));
            }
            TermEdit::Description(v) => {
                fields.push(("action", "wbsetdescription".into()));
                fields.push(("language", language.to_string()));
                fields.push(("value", v.clone()));
            }
            TermEdit::Aliases(list) => {
                fields.push(("action", "wbsetaliases".into()));
                fields.push(("language", language.to_string()));
                fields.push(("set", list.join("|")));
            }
            TermEdit::All {
                label,
                description,
                aliases,
            } => {
                let term = |v: &str| serde_json::json!({"language": language, "value": v});
                let mut data = serde_json::json!({
                    "descriptions": {language: term(description)},
                    // A list without add or remove flags replaces the language's
                    // aliases; one empty value clears them.
                    "aliases": if aliases.is_empty() {
                        serde_json::json!([term("")])
                    } else {
                        Value::Array(aliases.iter().map(|a| term(a)).collect())
                    },
                });
                if let Some(l) = label {
                    data["labels"] = serde_json::json!({language: term(l)});
                }
                fields.push(("action", "wbeditentity".into()));
                fields.push(("data", data.to_string()));
            }
        }
        fields.push(("token", token.to_string()));
        let pairs: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let r = self.post_form(incoming, "/w/api.php", &pairs).await?;
        check(&r).map(|_| ())
    }
}
