//! A Table page drawn as its grid (0045 §5; 0106 §9; 15 §2.4, §2.7): a row for each
//! entity the definition lists, read from the entity as the API serves it, a cell for
//! each column, and below them a draft's staged rows as they were entered, marked staged,
//! with the way back to `Special:CreateManyEntities`.
//!
//! A statement cell shows the property's best values, the preferred ones if any and else
//! the normal ones: the first, and how many more. Deprecated values are never shown, only
//! counted where nothing else is there. A term cell shows the term in the column's
//! language, or the reader's fallback with its language. A row whose entity is missing
//! is its ID and that state.
//!
//! A viewer who may edit also gets the data the grid's editor starts from (0045 §6, §7;
//! `ui/src/grid`): each row's cells in the form they are edited in, and which of them
//! the grid edits.
//!
//! Not drawn yet: the statement group in a popover, constraint markers and correction
//! chips, the duplicate mark of a cluster member, thumbnails and the default order.

use std::collections::{BTreeMap, BTreeSet};

use scatter_wikibase_shape::table::{Definition, Field, Limits, MODEL, Property};
use serde::Serialize;
use serde_json::Value;
use triplespace_client::{Incoming, Lookup as Found};

use crate::Site;
use crate::entity::{Lookup, entity_href};
use crate::frame::title_url;
use crate::i18n::Messages;
use crate::pages::Context;
use crate::special::values::{implied_type, plain, unit_id};

/// The grid.
#[derive(Debug, Clone, Serialize)]
pub struct GridModel {
    /// The table's description.
    pub description: Option<String>,
    /// The heading of the ID column.
    pub id_heading: String,
    /// The column headings.
    pub headings: Vec<Heading>,
    /// The rows.
    pub rows: Vec<RowModel>,
    /// What is said where the table lists no rows.
    pub empty: Option<String>,
    /// The staged rows.
    pub staged: Option<StagedModel>,
}

/// A column's heading.
#[derive(Debug, Clone, Serialize)]
pub struct Heading {
    /// The heading.
    pub label: String,
    /// What it is, beneath: a property's ID, a language.
    pub sub: Option<String>,
}

/// One entity's row.
#[derive(Debug, Clone, Serialize)]
pub struct RowModel {
    /// The entity's ID.
    pub id: String,
    /// Its page.
    pub href: String,
    /// Its state, where it has no cells: missing.
    pub state: Option<String>,
    /// The cells.
    pub cells: Vec<CellModel>,
}

/// One cell.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CellModel {
    /// What it shows.
    pub text: String,
    /// Where it links.
    pub href: Option<String>,
    /// The language of a fallback term.
    pub fallback: Option<String>,
    /// The fold: "+2", or "2 deprecated".
    pub more: Option<String>,
}

/// The staged rows of a draft.
#[derive(Debug, Clone, Serialize)]
pub struct StagedModel {
    /// The heading.
    pub heading: String,
    /// What staged rows are.
    pub note: String,
    /// `Special:CreateManyEntities/{title}`, and its label.
    pub resume: (String, String),
    /// The heading of the row-number column.
    pub n_heading: String,
    /// The rows.
    pub rows: Vec<StagedRowModel>,
}

/// One staged row.
#[derive(Debug, Clone, Serialize)]
pub struct StagedRowModel {
    /// Its number.
    pub n: String,
    /// How it stands: staged, matched to an item, skipped, refused.
    pub state: String,
    /// The cells as entered.
    pub cells: Vec<String>,
}

/// The best statements of a property: the preferred ones if any, else the normal ones;
/// and how many deprecated ones there are.
fn best(entity: &Value, property: &str) -> (Vec<Value>, usize) {
    let all: Vec<&Value> = entity["claims"][property]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let rank = |r: &str| -> Vec<Value> {
        all.iter()
            .filter(|s| s["rank"].as_str().unwrap_or("normal") == r)
            .map(|s| (*s).clone())
            .collect()
    };
    let preferred = rank("preferred");
    let deprecated = rank("deprecated").len();
    if preferred.is_empty() {
        (rank("normal"), deprecated)
    } else {
        (preferred, deprecated)
    }
}

/// The entity a main snak names, where it names one.
fn named(snak: &Value) -> Option<&str> {
    (snak["datavalue"]["type"] == "wikibase-entityid")
        .then(|| snak["datavalue"]["value"]["id"].as_str())
        .flatten()
}

/// A main snak as a cell shows it.
fn value_cell(m: &Messages, snak: &Value, labels: &Lookup) -> CellModel {
    match snak["snaktype"].as_str() {
        Some("somevalue") => CellModel {
            text: m.get("ts-value-somevalue"),
            ..CellModel::default()
        },
        Some("novalue") => CellModel {
            text: m.get("ts-value-novalue"),
            ..CellModel::default()
        },
        _ => match named(snak) {
            Some(id) => CellModel {
                text: labels
                    .label(id)
                    .map_or_else(|| id.to_string(), |(l, _)| l.to_string()),
                href: Some(entity_href(
                    id,
                    implied_type(id).as_deref().unwrap_or("item"),
                )),
                ..CellModel::default()
            },
            None => CellModel {
                // A quantity is shown with its unit's label, where the unit is an item.
                text: unit_id(&snak["datavalue"])
                    .and_then(|unit| {
                        let amount = snak["datavalue"]["value"]["amount"].as_str()?;
                        let name = labels.label(&unit).map_or(unit.as_str(), |(l, _)| l);
                        Some(format!("{} {name}", amount.trim_start_matches('+')))
                    })
                    .or_else(|| plain(&snak["datavalue"]))
                    .unwrap_or_else(|| {
                        // A value with no plain form: its text where it has one.
                        let v = &snak["datavalue"]["value"];
                        v["text"]
                            .as_str()
                            .or_else(|| v["amount"].as_str())
                            .or_else(|| v["time"].as_str())
                            .unwrap_or_default()
                            .to_string()
                    }),
                ..CellModel::default()
            },
        },
    }
}

/// A term cell: the term in the column's language, or the first along the reader's chain.
fn term_cell(entity: &Value, key: &str, lang: &str, chain: &[String]) -> CellModel {
    if let Some(text) = entity[key][lang]["value"].as_str() {
        return CellModel {
            text: text.to_string(),
            ..CellModel::default()
        };
    }
    chain
        .iter()
        .find_map(|l| {
            entity[key][l.as_str()]["value"]
                .as_str()
                .map(|text| CellModel {
                    text: text.to_string(),
                    fallback: Some(l.clone()),
                    ..CellModel::default()
                })
        })
        .unwrap_or_default()
}

/// The grid's model, from the definition and what the API served.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn model(
    m: &Messages,
    title: &str,
    def: &Definition,
    entities: &BTreeMap<String, Value>,
    labels: &Lookup,
) -> GridModel {
    let chain = m.term_chain();
    let empty = scatter_wikibase_shape::table::Staged::default();
    let staged = def.staged.as_ref().unwrap_or(&empty);
    let property = |p: &str| Heading {
        label: labels
            .label(p)
            .map_or_else(|| p.to_string(), |(l, _)| l.to_string()),
        sub: Some(p.to_string()),
    };
    let headings = def
        .columns
        .iter()
        .map(|c| {
            let mut h = match &c.field {
                Field::Label(l) => Heading {
                    label: m.get("ts-table-col-label"),
                    sub: Some(l.clone()),
                },
                Field::Description(l) => Heading {
                    label: m.get("ts-table-col-description"),
                    sub: Some(l.clone()),
                },
                Field::Aliases(l) => Heading {
                    label: m.get("ts-table-col-aliases"),
                    sub: Some(l.clone()),
                },
                Field::Statements(Property::Id(p)) => property(p.as_str()),
                Field::Statements(Property::New(i)) => Heading {
                    label: staged
                        .properties
                        .get(*i)
                        .map(|p| p.label.clone())
                        .unwrap_or_default(),
                    sub: Some(m.get("ts-table-col-new")),
                },
                Field::Sitelink(site) => Heading {
                    label: site.clone(),
                    sub: None,
                },
                Field::Thumbnail(_) => Heading {
                    label: m.get("ts-table-col-thumbnail"),
                    sub: None,
                },
                Field::Reference { property: p, .. } => Heading {
                    label: m.with("ts-table-col-reference", &[&property(p.as_str()).label]),
                    sub: Some(p.as_str().to_string()),
                },
            };
            if let Some(t) = &c.title {
                h.label.clone_from(t);
            }
            h
        })
        .collect();
    let rows = def
        .rows
        .iter()
        .map(|id| {
            let id = id.as_str();
            let href = entity_href(id, implied_type(id).as_deref().unwrap_or("item"));
            let Some(entity) = entities.get(id) else {
                return RowModel {
                    id: id.to_string(),
                    href,
                    state: Some(m.get("ts-table-row-missing")),
                    cells: Vec::new(),
                };
            };
            let cells = def
                .columns
                .iter()
                .map(|c| match &c.field {
                    Field::Label(l) => term_cell(entity, "labels", l, &chain),
                    Field::Description(l) => term_cell(entity, "descriptions", l, &chain),
                    Field::Aliases(l) => CellModel {
                        text: entity["aliases"][l.as_str()]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x["value"].as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .unwrap_or_default(),
                        ..CellModel::default()
                    },
                    Field::Statements(Property::Id(p)) => {
                        let (values, deprecated) = best(entity, p.as_str());
                        match values.first() {
                            Some(first) => {
                                let mut cell = value_cell(m, &first["mainsnak"], labels);
                                if values.len() > 1 {
                                    cell.more = Some(format!("+{}", values.len() - 1));
                                }
                                cell
                            }
                            None if deprecated > 0 => CellModel {
                                more: Some(
                                    m.with("ts-table-cell-deprecated", &[&deprecated.to_string()]),
                                ),
                                ..CellModel::default()
                            },
                            None => CellModel::default(),
                        }
                    }
                    Field::Sitelink(site) => CellModel {
                        text: entity["sitelinks"][site.as_str()]["title"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        ..CellModel::default()
                    },
                    // A draft's own columns hold nothing of an entity that is written.
                    Field::Statements(Property::New(_))
                    | Field::Thumbnail(_)
                    | Field::Reference { .. } => CellModel::default(),
                })
                .collect();
            RowModel {
                id: id.to_string(),
                href,
                state: None,
                cells,
            }
        })
        .collect();
    let staged_model = (!staged.rows.is_empty()).then(|| StagedModel {
        heading: m.with("ts-table-staged-heading", &[&staged.rows.len().to_string()]),
        note: m.get("ts-table-staged-note"),
        resume: (
            format!("/wiki/Special:CreateManyEntities/{}", title_url(title)),
            m.get("ts-table-staged-resume"),
        ),
        n_heading: m.get("ts-createentities-col-line"),
        rows: staged
            .rows
            .iter()
            .map(|r| StagedRowModel {
                n: r.n.to_string(),
                state: match (&r.refused, r.skip, &r.id) {
                    (Some(why), _, _) => m.with("ts-table-staged-refused", &[why]),
                    (_, true, _) => m.get("ts-createentities-status-skipped"),
                    (_, _, Some(id)) => m.with("ts-createentities-status-matches", &[id.as_str()]),
                    _ => m.get("ts-table-staged-state"),
                },
                cells: r.cells.clone(),
            })
            .collect(),
    });
    GridModel {
        description: def.description.clone(),
        id_heading: m.get("ts-table-col-id"),
        headings,
        empty: (def.rows.is_empty() && staged_model.is_none()).then(|| m.get("ts-table-empty")),
        rows,
        staged: staged_model,
    }
}

/// The grid of the Table page at `title`, drawn; `None` where the page is not a table
/// that validates, which is then shown as its text.
pub async fn grid(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    title: &str,
) -> Option<String> {
    let found = site.client().page_source(incoming, title).await.ok()?;
    cx.inputs.push(found.cache);
    let Found::Found(source) = found.value else {
        return None;
    };
    if source.model != MODEL {
        return None;
    }
    // The page was held to the limits when it was saved.
    let limits = Limits {
        max_rows: usize::MAX,
        max_columns: usize::MAX,
    };
    let def = Definition::parse(&source.text, limits).ok()?;
    let chain = cx.m.term_chain();
    let mut languages: BTreeSet<String> = chain.iter().cloned().collect();
    let mut properties: BTreeSet<String> = BTreeSet::new();
    for c in &def.columns {
        match &c.field {
            Field::Label(l) | Field::Description(l) | Field::Aliases(l) => {
                languages.insert(l.clone());
            }
            Field::Statements(Property::Id(p)) | Field::Reference { property: p, .. } => {
                properties.insert(p.as_str().to_string());
            }
            _ => {}
        }
    }
    let languages = languages.into_iter().collect::<Vec<_>>().join("|");
    let ids: Vec<String> = def.rows.iter().map(|id| id.as_str().to_string()).collect();
    let entities = if ids.is_empty() {
        BTreeMap::new()
    } else {
        let f = site
            .client()
            .entities(
                incoming,
                &ids,
                "info|labels|descriptions|aliases|claims|sitelinks",
                &languages,
            )
            .await
            .ok()?;
        cx.inputs.push(f.cache);
        f.value.found
    };
    // The labels of the properties and of the entities the first values name.
    let mut wanted = properties.clone();
    for e in entities.values() {
        for p in &properties {
            if let Some(first) = best(e, p).0.first() {
                if let Some(id) = named(&first["mainsnak"]) {
                    wanted.insert(id.to_string());
                }
                if let Some(unit) = unit_id(&first["mainsnak"]["datavalue"]) {
                    wanted.insert(unit);
                }
            }
        }
    }
    let labels = if wanted.is_empty() {
        Lookup::default()
    } else {
        let wanted: Vec<String> = wanted.into_iter().collect();
        let f = site
            .client()
            .entities(incoming, &wanted, "labels|datatype", &chain.join("|"))
            .await
            .ok()?;
        cx.inputs.push(f.cache);
        Lookup::from_entities(&f.value.found, &chain)
    };
    let mut html = cx.draw(
        "document/table.html",
        model(&cx.m, &source.title, &def, &entities, &labels),
    );
    // A viewer who may edit gets what the grid's editor starts from (0045 §6, §7).
    if cx.user.is_some() && cx.features.has(crate::features::Feature::Edit) {
        html.push_str(&sheet_data(cx, &source, &def, &entities, &labels));
    }
    Some(html)
}

/// Whether a cell of a data type can be edited as plain text (19 §3.1).
fn plainly_edited(datatype: &str) -> bool {
    use scatter_wikibase_model::value::{DataType, ValueType};
    let dt = DataType::parse(datatype);
    match dt.value_type() {
        Some(
            ValueType::String
            | ValueType::Quantity
            | ValueType::Time
            | ValueType::MonolingualText
            | ValueType::GlobeCoordinate,
        ) => true,
        Some(ValueType::EntityId) => matches!(dt.entity_type(), Some("item" | "property")),
        _ => false,
    }
}

/// The data the grid's editor starts from (0034 §4; 0045 §6, §7): the definition, and each
/// row's cells in the form they are edited in, with the one best statement's ID where a
/// cell shows one. A cell is edited in the grid when its row is a local entity and it
/// holds a term, or at most one best value of a type with a plain form; a cell of several
/// best values is edited on the entity's page (15 §2.5).
#[allow(clippy::too_many_lines)]
fn sheet_data(
    cx: &Context,
    source: &triplespace_client::Source,
    def: &Definition,
    entities: &BTreeMap<String, Value>,
    labels: &Lookup,
) -> String {
    use scatter_wikibase_model::IdForm;
    let m = &cx.m;
    let drawn = model(m, &source.title, def, entities, labels);
    let columns: Vec<Value> = def
        .columns
        .iter()
        .zip(&drawn.headings)
        .map(|(c, h)| {
            let mut col = serde_json::json!({
                "kind": "other",
                "label": h.label,
                "sub": h.sub.clone().unwrap_or_default(),
            });
            match &c.field {
                Field::Label(l) | Field::Description(l) | Field::Aliases(l) => {
                    col["kind"] = match &c.field {
                        Field::Label(_) => "label",
                        Field::Description(_) => "description",
                        _ => "aliases",
                    }
                    .into();
                    col["language"] = l.as_str().into();
                }
                Field::Statements(Property::Id(p)) => {
                    col["kind"] = "statements".into();
                    col["property"] = p.as_str().into();
                    col["datatype"] = labels.datatype(p.as_str()).unwrap_or_default().into();
                }
                _ => {}
            }
            col
        })
        .collect();
    let rows: Vec<Value> = def
        .rows
        .iter()
        .zip(&drawn.rows)
        .map(|(id, shown)| {
            let entity = entities.get(id.as_str());
            let local = id.form() == IdForm::Local;
            let cells: Vec<Value> = def
                .columns
                .iter()
                .enumerate()
                .map(|(at, c)| {
                    let seen = shown.cells.get(at).cloned().unwrap_or_default();
                    let fixed = |text: String| {
                        serde_json::json!({"text": text, "note": seen.more.clone().unwrap_or_default(), "editable": false})
                    };
                    let Some(entity) = entity else {
                        return fixed(String::new());
                    };
                    match &c.field {
                        Field::Label(l) | Field::Description(l) => {
                            let key = if matches!(c.field, Field::Label(_)) {
                                "labels"
                            } else {
                                "descriptions"
                            };
                            let own = entity[key][l.as_str()]["value"].as_str();
                            serde_json::json!({
                                "text": own.unwrap_or_default(),
                                // A fallback is shown beside the empty cell, and editing
                                // writes the column's language (15 §2.4).
                                "note": if own.is_none() && !seen.text.is_empty() {
                                    format!("{} ({})", seen.text, seen.fallback.clone().unwrap_or_default())
                                } else {
                                    String::new()
                                },
                                "editable": local,
                            })
                        }
                        Field::Aliases(l) => {
                            let list: Vec<String> = entity["aliases"][l.as_str()]
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|x| x["value"].as_str())
                                        .map(str::to_string)
                                        .collect()
                                })
                                .unwrap_or_default();
                            let text = crate::special::create_entities::table::join_values(&list);
                            serde_json::json!({"text": text, "note": "", "editable": local})
                        }
                        Field::Statements(Property::Id(p)) => {
                            let (values, _) = best(entity, p.as_str());
                            let typed = labels.datatype(p.as_str()).is_some_and(plainly_edited);
                            match values.as_slice() {
                                [] => serde_json::json!({
                                    "text": "",
                                    "note": seen.more.clone().unwrap_or_default(),
                                    "editable": local && typed,
                                }),
                                [one] => {
                                    let text = (one["mainsnak"]["snaktype"] == "value")
                                        .then(|| plain(&one["mainsnak"]["datavalue"]))
                                        .flatten();
                                    match (text, one["id"].as_str()) {
                                        (Some(text), Some(guid)) if local && typed => serde_json::json!({
                                            // An item is edited by its ID and shown with its label.
                                            // An item, or a quantity with a unit, is shown as the page shows it.
                                            "note": if seen.href.is_some() || unit_id(&one["mainsnak"]["datavalue"]).is_some() { seen.text.clone() } else { String::new() },
                                            "text": text,
                                            "editable": true,
                                            "guid": guid,
                                        }),
                                        _ => fixed(seen.text.clone()),
                                    }
                                }
                                _ => fixed(seen.text.clone()),
                            }
                        }
                        _ => fixed(seen.text.clone()),
                    }
                })
                .collect();
            serde_json::json!({
                "id": id.as_str(),
                "href": shown.href,
                "lastrevid": entity.and_then(|e| e["lastrevid"].as_u64()).unwrap_or(0),
                "missing": entity.is_none(),
                "cells": cells,
            })
        })
        .collect();
    let data = serde_json::json!({
        "page": "table",
        "conceptBase": cx.site.concept_base,
        "lang": cx.site.lang,
        "dir": m.dir(),
        "messages": m.with_prefix("ts-sheet-"),
        "title": source.title,
        "pageId": source.page_id,
        "revid": source.revid,
        "definition": source.text,
        "hasReference": def.default_reference.is_some(),
        "columns": columns,
        "rows": rows,
    });
    format!(
        "<script type=\"application/json\" id=\"ts-sheet-data\">{}</script>",
        data.to_string().replace("</", "<\\/")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 0045 §5: best values, the fold, deprecated values counted, terms with their
    // fallback, a missing entity; 0106 §9: staged rows as entered.
    #[test]
    fn cells_show_best_values_and_terms() {
        let m = Messages::for_language("en");
        let def = Definition::parse(
            r#"{"version": 1, "rows": {"ids": ["Q1", "Q2"]},
                "columns": [{"field": "label", "language": "de"},
                            {"field": "statements", "property": "P31"},
                            {"field": "statements", "property": "P577", "title": "Published"}],
                "staged": {"rows": [{"n": 1, "cells": ["Neu", "Q5", "1970"]},
                                    {"n": 2, "cells": ["", "", ""], "refused": "no"}]}}"#,
            Limits::default(),
        )
        .unwrap();
        let snak =
            |p: &str, dv: Value| json!({"snaktype": "value", "property": p, "datavalue": dv});
        let item = |id: &str| json!({"type": "wikibase-entityid", "value": {"id": id}});
        let year =
            json!({"type": "time", "value": {"time": "+1969-00-00T00:00:00Z", "precision": 9}});
        let mut entities = BTreeMap::new();
        entities.insert(
            "Q1".to_string(),
            json!({"labels": {"en": {"value": "One"}},
                   "claims": {
                     "P31": [{"rank": "normal", "mainsnak": snak("P31", item("Q5"))},
                             {"rank": "preferred", "mainsnak": snak("P31", item("Q6"))},
                             {"rank": "preferred", "mainsnak": snak("P31", item("Q7"))}],
                     "P577": [{"rank": "deprecated", "mainsnak": snak("P577", year.clone())}]}}),
        );
        let mut found = BTreeMap::new();
        found.insert(
            "Q6".to_string(),
            json!({"labels": {"en": {"language": "en", "value": "six"}}}),
        );
        found.insert("P31".to_string(), json!({"labels": {"en": {"language": "en", "value": "instance of"}}, "datatype": "wikibase-item"}));
        let labels = Lookup::from_entities(&found, &["en".to_string()]);
        let g = model(&m, "Table:Drafts/A/x", &def, &entities, &labels);
        assert_eq!(g.headings[1].label, "instance of");
        assert_eq!(g.headings[2].label, "Published");
        let one = &g.rows[0];
        assert_eq!(one.cells[0].text, "One");
        assert_eq!(one.cells[0].fallback.as_deref(), Some("en"));
        assert_eq!(
            one.cells[1].text, "six",
            "the first preferred value, by its label"
        );
        assert_eq!(one.cells[1].more.as_deref(), Some("+1"));
        assert_eq!(one.cells[2].text, "");
        assert_eq!(one.cells[2].more.as_deref(), Some("1 deprecated"));
        assert!(g.rows[1].state.is_some() && g.rows[1].cells.is_empty());
        let staged = g.staged.expect("staged rows");
        assert_eq!(staged.rows[0].cells, ["Neu", "Q5", "1970"]);
        assert!(staged.rows[1].state.contains("no"));
        assert_eq!(
            staged.resume.0,
            "/wiki/Special:CreateManyEntities/Table:Drafts/A/x"
        );
    }
}
