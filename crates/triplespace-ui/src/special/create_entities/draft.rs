//! A draft (0106 §9; 15 §2.7): the table being prepared, kept as a Table page whose
//! definition holds the rows not yet written. This module turns the table's text into
//! that definition and back, so that `Special:CreateEntities/{table title}` edits the
//! same thing the page stores, and records an import in it: each written row leaves
//! `staged` for `rows.ids`, and a refused row stays with its reason.
//!
//! The two forms name the same things differently. The text's `id` column is each staged
//! row's `id`, `skip` or `create`; a plain column the reference on every statement reads
//! is a `reference` column with no `of`, read by a snak's `"column"`; a `row:n` counts
//! positions in the text and names a row's `n` in the definition.

use std::collections::BTreeMap;

use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_shape::table::{
    Column as DefColumn, Definition, Field, NewProperty, Property, Staged, StagedRow,
};
use serde_json::{Map, Value, json};

use super::plan::Read;
use super::table::{
    Column, IdCell, Line, header, id_cell, join_values, lines_of, row_ref, split_values,
};
use crate::entity::Lookup;
use crate::i18n::Messages;
use crate::special::values::{datavalue, plain};

/// The prefix of a draft that only stages edits (0106 A1), before the creator's name.
pub const DRAFTS: &str = "Table:Drafts/";

/// The form's fields, as a draft fills them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Text {
    /// The table: the header row, then one row per staged row.
    pub rows: Vec<Vec<String>>,
    /// The statements on every item, one per line.
    pub shared: String,
    /// The reference on every statement, one snak per line.
    pub reference: String,
}

/// The title a new draft gets: `Table:Drafts/{user}/{name}`.
#[must_use]
pub fn title_for(user: &str, name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| if "#<>[]|{}".contains(c) { '-' } else { c })
        .collect();
    format!("{DRAFTS}{user}/{}", name.trim().trim_matches('/'))
}

/// The table's text as a definition. `base` is the draft as it is stored, whose listed
/// rows, description and order are kept.
///
/// # Errors
///
/// What the definition cannot hold, each a message: a reference value that is not one of
/// its property's type, an `id` cell that is not understood.
#[allow(clippy::too_many_lines)]
pub fn definition(
    m: &Messages,
    read: &Read,
    lookup: &Lookup,
    base: Option<&Definition>,
) -> Result<Definition, Vec<String>> {
    let mut errors = Vec::new();
    let id = |p: &str| EntityId::parse(p).ok();
    // The columns, and where each of the text's columns went.
    let mut columns: Vec<DefColumn> = Vec::new();
    let mut place: Vec<Option<usize>> = Vec::new();
    let reads = |h: &str| {
        read.reference
            .iter()
            .find(|l| l.column().is_some_and(|c| c.eq_ignore_ascii_case(h.trim())))
    };
    let statements_of = |columns: &[DefColumn], p: &str| {
        columns
            .iter()
            .position(|c| matches!(&c.field, Field::Statements(Property::Id(q)) if q.as_str() == p))
    };
    for c in &read.columns {
        let field = match c {
            Column::Id => None,
            Column::Label(l) => Some(Field::Label(l.clone())),
            Column::Description(l) => Some(Field::Description(l.clone())),
            Column::Aliases(l) => Some(Field::Aliases(l.clone())),
            Column::Property(p) => id(p).map(|p| Field::Statements(Property::Id(p))),
            Column::New { label, .. } => read
                .declared(label)
                .map(|i| Field::Statements(Property::New(i))),
            Column::Reference { of, property } => {
                // The statements it belongs to need a column to point at; where the
                // property is only among the statements on every item, an empty one.
                let of_at = statements_of(&columns, of).or_else(|| {
                    let p = id(of)?;
                    columns.push(DefColumn {
                        field: Field::Statements(Property::Id(p)),
                        title: None,
                    });
                    Some(columns.len() - 1)
                });
                id(property).map(|property| Field::Reference {
                    property,
                    of: of_at,
                })
            }
            Column::Plain(h) => reads(h).and_then(|l| {
                id(&l.property).map(|property| Field::Reference { property, of: None })
            }),
        };
        let title = match c {
            Column::Plain(h) => Some(h.trim().to_string()),
            _ => None,
        };
        place.push(field.map(|field| {
            columns.push(DefColumn { field, title });
            columns.len() - 1
        }));
    }

    // The reference on every statement.
    let mut groups: Map<String, Value> = Map::new();
    let mut order: Vec<Value> = Vec::new();
    for l in &read.reference {
        let mut snaks = Vec::new();
        if let Some(name) = l.column() {
            let at = read
                .headers
                .iter()
                .position(|h| h.trim().eq_ignore_ascii_case(name))
                .and_then(|at| place.get(at).copied().flatten());
            match at {
                Some(at) => {
                    snaks.push(json!({"snaktype": "value", "property": l.property, "column": at}));
                }
                None => errors.push(m.with("ts-createentities-no-column", &[name, &l.raw])),
            }
        } else {
            for v in split_values(&l.value) {
                let raw = format!("{}:{v}", l.property);
                match lookup
                    .datatype(&l.property)
                    .ok_or_else(|| m.with("ts-new-statement-no-property", &[&l.property, &raw]))
                    .and_then(|dt| datavalue(m, dt, &v, &raw))
                {
                    Ok(dv) => snaks.push(
                        json!({"snaktype": "value", "property": l.property, "datavalue": dv}),
                    ),
                    Err(e) => errors.push(e),
                }
            }
        }
        if snaks.is_empty() {
            continue;
        }
        if !groups.contains_key(&l.property) {
            order.push(json!(l.property));
        }
        if let Some(a) = groups
            .entry(l.property.clone())
            .or_insert_with(|| json!([]))
            .as_array_mut()
        {
            a.extend(snaks);
        }
    }

    // The rows.
    let id_at = read.columns.iter().position(|c| *c == Column::Id);
    let mut rows = Vec::new();
    for (i, cells) in read.rows.iter().enumerate() {
        let mut out = vec![String::new(); columns.len()];
        for (at, to) in place.iter().enumerate() {
            if let (Some(to), Some(text)) = (to, cells.get(at)) {
                out[*to].clone_from(text);
            }
        }
        let mut row = StagedRow {
            n: (i + 1) as u64,
            cells: out,
            id: None,
            skip: false,
            create: false,
            refused: None,
        };
        match id_at.map_or(IdCell::Auto, |at| {
            id_cell(cells.get(at).map_or("", String::as_str))
        }) {
            IdCell::Auto => {}
            IdCell::Item(q) => row.id = id(&q),
            IdCell::New => row.create = true,
            IdCell::Skip => row.skip = true,
            IdCell::Bad(c) => errors.push(m.with("ts-createentities-bad-id", &[&c])),
        }
        rows.push(row);
    }
    if columns.is_empty() {
        errors.push(m.get("ts-createentities-draft-no-columns"));
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Definition {
        description: base.and_then(|b| b.description.clone()),
        rows: base.map(|b| b.rows.clone()).unwrap_or_default(),
        columns,
        default_reference: (!groups.is_empty())
            .then(|| json!({"snaks": groups, "snaks-order": order})),
        staged: Some(Staged {
            shared: read
                .shared
                .iter()
                .map(|l| format!("{}:{}", l.property, l.value))
                .collect(),
            properties: read
                .new
                .iter()
                .map(|d| NewProperty {
                    label: d.label.clone(),
                    datatype: d.datatype.clone(),
                })
                .collect(),
            rows,
        }),
        sort: base.map(|b| b.sort.clone()).unwrap_or_default(),
    })
}

/// A definition as the table's text.
///
/// # Errors
///
/// The columns the text has no form for (a sitelink, a thumbnail), named.
#[allow(clippy::too_many_lines)]
pub fn text(def: &Definition, lang: &str) -> Result<Text, Vec<String>> {
    let empty = Staged::default();
    let staged = def.staged.as_ref().unwrap_or(&empty);
    let mut unsupported = Vec::new();
    let property_header = |at: usize| match def.columns.get(at).map(|c| &c.field) {
        Some(Field::Statements(Property::Id(p))) => Some(p.as_str().to_string()),
        _ => None,
    };
    let mut headers = vec!["id".to_string()];
    for (at, c) in def.columns.iter().enumerate() {
        let column = match &c.field {
            Field::Label(l) => Column::Label(l.clone()),
            Field::Description(l) => Column::Description(l.clone()),
            Field::Aliases(l) => Column::Aliases(l.clone()),
            Field::Statements(Property::Id(p)) => Column::Property(p.as_str().to_string()),
            Field::Statements(Property::New(i)) => match staged.properties.get(*i) {
                Some(p) => Column::New {
                    label: p.label.clone(),
                    datatype: p.datatype.clone(),
                },
                None => Column::Plain(format!("column {}", at + 1)),
            },
            Field::Reference {
                property,
                of: Some(of),
            } => match property_header(*of) {
                Some(of) => Column::Reference {
                    of,
                    property: property.as_str().to_string(),
                },
                None => Column::Plain(format!("column {}", at + 1)),
            },
            Field::Reference { property, of: None } => Column::Plain(
                c.title
                    .clone()
                    .unwrap_or_else(|| format!("{} source", property.as_str())),
            ),
            Field::Sitelink(site) => {
                unsupported.push(format!("sitelink ({site})"));
                continue;
            }
            Field::Thumbnail(_) => {
                unsupported.push("thumbnail".to_string());
                continue;
            }
        };
        headers.push(header(&column, lang));
    }
    if !unsupported.is_empty() {
        return Err(unsupported);
    }
    // `row:n` names a row's number; the text counts positions.
    let position: BTreeMap<u64, usize> = staged
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.n, i + 1))
        .collect();
    let renumber = |cell: &str| -> String {
        if !cell.to_ascii_lowercase().contains("row:") {
            return cell.to_string();
        }
        let values: Vec<String> = split_values(cell)
            .into_iter()
            .map(
                |v| match row_ref(&v).and_then(|n| position.get(&(n as u64))) {
                    Some(p) => format!("row:{p}"),
                    None => v,
                },
            )
            .collect();
        join_values(&values)
    };
    let mut rows = vec![headers];
    for r in &staged.rows {
        let id = match (&r.id, r.skip, r.create) {
            (_, true, _) => "skip".to_string(),
            (Some(id), _, _) => id.as_str().to_string(),
            (None, _, true) => "new".to_string(),
            _ => String::new(),
        };
        let mut row = vec![id];
        row.extend(r.cells.iter().map(|c| renumber(c)));
        rows.push(row);
    }
    let mut reference = Vec::new();
    if let Some(snaks) = def
        .default_reference
        .as_ref()
        .and_then(|r| r["snaks"].as_object())
    {
        let order: Vec<String> = def
            .default_reference
            .as_ref()
            .and_then(|r| r["snaks-order"].as_array())
            .map_or_else(
                || snaks.keys().cloned().collect(),
                |a| {
                    a.iter()
                        .filter_map(|p| p.as_str().map(str::to_string))
                        .collect()
                },
            );
        for p in order {
            for s in snaks
                .get(&p)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(at) = s["column"].as_u64().and_then(|c| usize::try_from(c).ok()) {
                    // The `id` column comes first in the text.
                    if let Some(h) = rows[0].get(at + 1) {
                        reference.push(format!("{p}:{{{h}}}"));
                    }
                } else if let Some(v) = plain(&s["datavalue"]) {
                    reference.push(format!("{p}:{}", join_values(&[v])));
                }
            }
        }
    }
    Ok(Text {
        rows,
        shared: staged.shared.join("\n"),
        reference: reference.join("\n"),
    })
}

/// The statements on every item, as the lines the text reads.
#[must_use]
pub fn shared_lines(def: &Definition) -> Vec<Line> {
    def.staged
        .as_ref()
        .map(|s| {
            lines_of(&s.shared.join("\n"))
                .into_iter()
                .filter_map(Result::ok)
                .collect()
        })
        .unwrap_or_default()
}

/// Records a run in the draft (15 §2.6, §2.7): each written row leaves `staged`, its item
/// joins `rows.ids`, and what still names it with `row:n` names the item instead; a
/// refused row stays, with its reason. Rows are named by their position in the text the
/// run read, which is their position among the staged rows.
pub fn record(def: &mut Definition, written: &[(usize, String)], refused: &[(usize, String)]) {
    let Some(staged) = def.staged.as_mut() else {
        return;
    };
    let number: Vec<u64> = staged.rows.iter().map(|r| r.n).collect();
    let n_of = |position: usize| number.get(position.wrapping_sub(1)).copied();
    let mut done: BTreeMap<u64, String> = BTreeMap::new();
    for (position, id) in written {
        if let Some(n) = n_of(*position) {
            done.insert(n, id.clone());
        }
    }
    for (position, why) in refused {
        if let Some(n) = n_of(*position)
            && let Some(r) = staged.rows.iter_mut().find(|r| r.n == n)
        {
            r.refused = Some(why.clone());
        }
    }
    staged.rows.retain(|r| !done.contains_key(&r.n));
    for r in &mut staged.rows {
        for cell in &mut r.cells {
            if !cell.to_ascii_lowercase().contains("row:") {
                continue;
            }
            let values: Vec<String> = split_values(cell)
                .into_iter()
                .map(|v| match row_ref(&v).and_then(|n| done.get(&(n as u64))) {
                    Some(id) => id.clone(),
                    None => v,
                })
                .collect();
            *cell = join_values(&values);
        }
    }
    for id in done.values() {
        if let Ok(id) = EntityId::parse(id)
            && !def.rows.contains(&id)
        {
            def.rows.push(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::special::create_entities::table::rows_of;
    use scatter_wikibase_shape::table::Limits;

    fn lookup() -> Lookup {
        let mut found = BTreeMap::new();
        for (id, dt) in [
            ("P31", "wikibase-item"),
            ("P577", "time"),
            ("P854", "url"),
            ("P248", "wikibase-item"),
        ] {
            found.insert(
                id.to_string(),
                json!({"id": id, "datatype": dt, "labels": {}}),
            );
        }
        Lookup::from_entities(&found, &["en".to_string()])
    }

    fn read(table: &str, shared: &str, reference: &str) -> Read {
        let rows = rows_of(table);
        let ok = |t: &str| lines_of(t).into_iter().map(Result::unwrap).collect();
        Read::of(&rows[0], &rows[1..], ok(shared), ok(reference), "en")
    }

    const TABLE: &str = "id,label,description:de,P577,P577/ref:P854,new:Publisher@wikibase-item,source url,notes\n\
        ,Paper A,ein Artikel,1969,https://example.org/own,row:3,https://example.org/a,x\n\
        Q5,Paper B,,1970,,,https://example.org/b,\n\
        new,Acme,,,,,,\n\
        skip,Paper C,,,,row:1|row:3,,\n";

    // 0106 §9: the text is the definition and the definition the text.
    #[test]
    fn a_draft_holds_the_text_and_gives_it_back() {
        let m = Messages::for_language("en");
        let r = read(TABLE, "P31:Q13442814", "P248:Q1234\nP854:{source url}");
        let def = definition(&m, &r, &lookup(), None).expect("a definition");
        // The definition is one the content model accepts, as it is written.
        let stored = Definition::parse(&def.canonical(), Limits::default()).expect("valid");
        assert_eq!(stored, def);
        assert_eq!(
            def.columns.len(),
            6,
            "id and the unread column are not columns"
        );
        assert_eq!(
            def.columns[5].field,
            Field::Reference {
                property: EntityId::parse("P854").unwrap(),
                of: None
            }
        );
        let staged = def.staged.as_ref().unwrap();
        assert_eq!(staged.rows.len(), 4);
        assert_eq!(staged.rows[1].id.as_ref().map(EntityId::as_str), Some("Q5"));
        assert!(staged.rows[2].create && staged.rows[3].skip);
        assert_eq!(
            def.default_reference.as_ref().unwrap()["snaks"]["P854"][0]["column"],
            5
        );

        // Back as text, without the column nothing read; and the same definition again.
        let t = text(&def, "en").unwrap();
        assert_eq!(
            t.rows[0],
            [
                "id",
                "label",
                "description:de",
                "P577",
                "P577/ref:P854",
                "new:Publisher@wikibase-item",
                "source url"
            ]
        );
        assert_eq!(t.rows[4][0], "skip");
        assert_eq!(t.rows[4][5], "row:1|row:3");
        assert_eq!(t.shared, "P31:Q13442814");
        assert_eq!(t.reference, "P248:Q1234\nP854:{source url}");
        let ok = |s: &str| lines_of(s).into_iter().map(Result::unwrap).collect();
        let again = Read::of(
            &t.rows[0],
            &t.rows[1..],
            ok(&t.shared),
            ok(&t.reference),
            "en",
        );
        assert_eq!(definition(&m, &again, &lookup(), None).unwrap(), def);

        // A reference column whose property has no column of its own gets an empty one.
        let r = read(
            "label,P577/ref:P854\nA,https://example.org/a\n",
            "P577:1969",
            "",
        );
        let def = definition(&m, &r, &lookup(), None).unwrap();
        assert_eq!(def.columns.len(), 3);
        assert_eq!(
            def.columns[2].field,
            Field::Reference {
                property: EntityId::parse("P854").unwrap(),
                of: Some(1)
            }
        );
        // What cannot be held is said.
        let r = read("label\nA\n", "", "P854:{missing}");
        assert_eq!(definition(&m, &r, &lookup(), None).unwrap_err().len(), 1);
    }

    // 0106 §9; 15 §2.7: importing empties the draft row by row.
    #[test]
    fn an_import_moves_written_rows_to_the_listed_rows() {
        let m = Messages::for_language("en");
        let r = read(TABLE, "P31:Q13442814", "");
        let mut def = definition(&m, &r, &lookup(), None).unwrap();
        record(
            &mut def,
            &[(3, "Q77".to_string()), (2, "Q5".to_string())],
            &[(1, "refused by a filter".to_string())],
        );
        let ids: Vec<&str> = def.rows.iter().map(EntityId::as_str).collect();
        assert_eq!(ids, ["Q5", "Q77"]);
        let staged = def.staged.as_ref().unwrap();
        assert_eq!(staged.rows.len(), 2);
        assert_eq!(
            staged.rows[0].refused.as_deref(),
            Some("refused by a filter")
        );
        // What named the written row names its item; what names a staged row still does,
        // by its new position in the text.
        assert_eq!(staged.rows[0].cells[4], "Q77");
        assert_eq!(staged.rows[1].cells[4], "row:1|Q77");
        let t = text(&def, "en").unwrap();
        assert_eq!(t.rows[2][5], "row:1|Q77");
        // The statements on every item stay, for rows added later (0106 A3).
        assert_eq!(shared_lines(&def).len(), 1);
        assert!(Definition::parse(&def.canonical(), Limits::default()).is_ok());
    }
}
