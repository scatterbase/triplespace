//! What each row of the table will write (0106 §2, §4, §5, §8; 21 §6.6; 05 §3.5): its terms
//! and statements with their references, the item it creates or adds to, the rows it names
//! with `row:n`, and why it cannot be written. Planning is pure, so the page and the tests
//! share it; the page supplies what the API knows (the properties' data types, the items
//! the rows match) and writes the plans in the order [`order`] gives.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::table::{Column, IdCell, Line, id_cell, row_ref, split_values};
use crate::entity::Lookup;
use crate::i18n::Messages;
use crate::special::values::{datavalue, implied_type};

/// A property a statement names: one that exists, or one the table declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Prop {
    /// An existing property's ID.
    Id(String),
    /// The table's declared property at this position ([`Read::new`]).
    New(usize),
}

/// A statement's value: a data value, or the item another row creates or matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Val {
    /// A `datavalue`.
    Data(Value),
    /// Row `n`'s item, counting from 1 below the header.
    Row(usize),
}

/// One snak of a reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snak {
    /// The property.
    pub property: String,
    /// The `datavalue`.
    pub datavalue: Value,
}

/// A statement a row writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stmt {
    /// Its property.
    pub property: Prop,
    /// Its value.
    pub value: Val,
    /// Its one reference; empty for none.
    pub reference: Vec<Snak>,
}

/// A label or description a row writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Term {
    /// The language.
    pub language: String,
    /// The text.
    pub value: String,
    /// Whether it replaces the matched item's term (`!` before the cell's value).
    pub replace: bool,
}

/// The item a row writes to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    /// A new item.
    New,
    /// An existing item, which the row adds to.
    Existing(String),
    /// Several existing items match; one is to be picked, or the row told to create.
    Ambiguous(Vec<String>),
    /// The row is left out.
    Skip,
}

/// How a row stands before import (0106 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// It creates an item.
    New,
    /// It adds to this item.
    Matches(String),
    /// It matches this many items.
    Ambiguous(usize),
    /// This many of its terms differ from the matched item's, with no intent given.
    TermConflict(usize),
    /// It has this many problems.
    Invalid(usize),
    /// It is left out as asked.
    Skipped,
}

/// One row as it will be written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// The row's number: 1 for the first row below the header, as `row:n` names it.
    pub n: usize,
    /// The label shown for it: the first label column's, else the first cell.
    pub shown: String,
    /// What it writes to.
    pub target: Target,
    /// Whether the `id` cell left the target to the preview.
    pub auto: bool,
    /// Labels.
    pub labels: Vec<Term>,
    /// Descriptions.
    pub descriptions: Vec<Term>,
    /// Aliases, by language.
    pub aliases: Vec<(String, Vec<String>)>,
    /// Statements.
    pub statements: Vec<Stmt>,
    /// Why the row cannot be written, each a message.
    pub errors: Vec<String>,
    /// How many of the errors are term conflicts.
    pub conflicts: usize,
}

impl Plan {
    /// Whether the row is written on import.
    #[must_use]
    pub fn writable(&self) -> bool {
        self.errors.is_empty() && matches!(self.target, Target::New | Target::Existing(_))
    }

    /// The row's status.
    #[must_use]
    pub fn status(&self) -> Status {
        match &self.target {
            Target::Skip => Status::Skipped,
            Target::Ambiguous(ids) => Status::Ambiguous(ids.len()),
            _ if self.conflicts > 0 && self.conflicts == self.errors.len() => {
                Status::TermConflict(self.conflicts)
            }
            _ if !self.errors.is_empty() => Status::Invalid(self.errors.len()),
            Target::Existing(id) => Status::Matches(id.clone()),
            Target::New => Status::New,
        }
    }

    /// The rows this one names with `row:n`.
    #[must_use]
    pub fn needs(&self) -> BTreeSet<usize> {
        self.statements
            .iter()
            .filter_map(|s| match s.value {
                Val::Row(n) => Some(n),
                Val::Data(_) => None,
            })
            .collect()
    }
}

/// A property the table declares (0106 §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declared {
    /// Its label in the wiki's language.
    pub label: String,
    /// Its data type, once the header gives one.
    pub datatype: Option<String>,
}

/// The table as read: what [`plan`] plans from.
#[derive(Debug, Clone, Default)]
pub struct Read {
    /// The headers as written.
    pub headers: Vec<String>,
    /// What each column holds.
    pub columns: Vec<Column>,
    /// The rows below the header.
    pub rows: Vec<Vec<String>>,
    /// The statements on every item.
    pub shared: Vec<Line>,
    /// The reference on every statement.
    pub reference: Vec<Line>,
    /// The properties the table declares, one per label, in column order.
    pub new: Vec<Declared>,
}

impl Read {
    /// Reads a table's columns and collects the properties it declares.
    #[must_use]
    pub fn of(
        headers: &[String],
        rows: &[Vec<String>],
        shared: Vec<Line>,
        reference: Vec<Line>,
        lang: &str,
    ) -> Self {
        let columns: Vec<Column> = headers
            .iter()
            .map(|h| super::table::column(h, lang))
            .collect();
        let mut new: Vec<Declared> = Vec::new();
        for c in &columns {
            if let Column::New { label, datatype } = c {
                match new.iter_mut().find(|d| d.label == *label) {
                    Some(d) => d.datatype = d.datatype.take().or_else(|| datatype.clone()),
                    None => new.push(Declared {
                        label: label.clone(),
                        datatype: datatype.clone(),
                    }),
                }
            }
        }
        Self {
            headers: headers.to_vec(),
            columns,
            rows: rows.to_vec(),
            shared,
            reference,
            new,
        }
    }

    /// The position of a declared property.
    #[must_use]
    pub fn declared(&self, label: &str) -> Option<usize> {
        self.new.iter().position(|d| d.label == label)
    }

    /// The non-empty values of a declared property's columns.
    #[must_use]
    pub fn values_of(&self, label: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (i, c) in self.columns.iter().enumerate() {
            if matches!(c, Column::New { label: l, .. } if l == label) {
                for r in &self.rows {
                    out.extend(split_values(r.get(i).map_or("", String::as_str)));
                }
            }
        }
        out
    }

    /// Every existing property the table names, sorted.
    #[must_use]
    pub fn properties(&self) -> Vec<String> {
        let mut ids: BTreeSet<String> = BTreeSet::new();
        for c in &self.columns {
            match c {
                Column::Property(p) => {
                    ids.insert(p.clone());
                }
                Column::Reference { of, property } => {
                    ids.insert(of.clone());
                    ids.insert(property.clone());
                }
                _ => {}
            }
        }
        for l in self.shared.iter().chain(&self.reference) {
            ids.insert(l.property.clone());
        }
        ids.into_iter().collect()
    }
}

/// The data type a column of values suggests for a new property (0106 A1): `time` when
/// every value is a time, `wikibase-item` when every value is an item ID or another row.
/// The third case, an identifier property's pattern, needs the properties' patterns and
/// is proposed by the grid.
#[must_use]
pub fn propose(m: &Messages, values: &[String]) -> Option<&'static str> {
    if values.is_empty() {
        return None;
    }
    if values
        .iter()
        .all(|v| row_ref(v).is_some() || implied_type(v).as_deref() == Some("item"))
    {
        return Some("wikibase-item");
    }
    if values.iter().all(|v| datavalue(m, "time", v, v).is_ok()) {
        return Some("time");
    }
    None
}

/// Plans the rows: each cell parsed as its column takes it, the statements on every item
/// added where the row's own column does not replace them, every statement given the
/// row's own reference for its property or else the reference on every statement.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn plan(m: &Messages, read: &Read, lookup: &Lookup) -> Vec<Plan> {
    let datatype = |p: &Prop, raw: &str| -> Result<String, String> {
        match p {
            Prop::Id(id) => lookup
                .datatype(id)
                .map(str::to_string)
                .ok_or_else(|| m.with("ts-new-statement-no-property", &[id, raw])),
            Prop::New(i) => read
                .new
                .get(*i)
                .and_then(|d| d.datatype.clone())
                .ok_or_else(|| {
                    m.with(
                        "ts-createentities-new-untyped",
                        &[read.new.get(*i).map_or("", |d| d.label.as_str())],
                    )
                }),
        }
    };
    let value_of = |p: &Prop, value: &str, raw: &str| -> Result<Val, String> {
        let dt = datatype(p, raw)?;
        if dt == "wikibase-item"
            && let Some(n) = row_ref(value)
        {
            return Ok(Val::Row(n));
        }
        datavalue(m, &dt, value, raw).map(Val::Data)
    };
    let snaks_of = |property: &str, cell: &str, errors: &mut Vec<String>| -> Vec<Snak> {
        let mut out = Vec::new();
        for v in split_values(cell) {
            let raw = format!("{property}:{v}");
            match datatype(&Prop::Id(property.to_string()), &raw)
                .and_then(|dt| datavalue(m, &dt, &v, &raw))
            {
                Ok(dv) => out.push(Snak {
                    property: property.to_string(),
                    datavalue: dv,
                }),
                Err(e) => errors.push(e),
            }
        }
        out
    };
    let column_at = |name: &str| {
        read.headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
    };
    read.rows
        .iter()
        .enumerate()
        .map(|(i, cells)| {
            let cell = |at: usize| cells.get(at).map_or("", String::as_str);
            let mut errors = Vec::new();
            let mut target = Target::New;
            let mut auto = true;
            for (at, c) in read.columns.iter().enumerate() {
                if *c == Column::Id {
                    match id_cell(cell(at)) {
                        IdCell::Auto => {}
                        IdCell::Item(id) => {
                            target = Target::Existing(id);
                            auto = false;
                        }
                        IdCell::New => auto = false,
                        IdCell::Skip => {
                            target = Target::Skip;
                            auto = false;
                        }
                        IdCell::Bad(c) => errors.push(m.with("ts-createentities-bad-id", &[&c])),
                    }
                }
            }
            let adds = matches!(target, Target::Existing(_));
            let mut labels = Vec::new();
            let mut descriptions = Vec::new();
            let mut aliases: Vec<(String, Vec<String>)> = Vec::new();
            // Each statement with its property as the table names it, and whether the
            // table's own column gave it.
            let mut statements: Vec<Stmt> = Vec::new();
            let mut own: BTreeSet<String> = BTreeSet::new();
            let term = |language: &str, text: &str| {
                // `!` marks a replacement only where there is an item to replace on.
                let (value, replace) = match text.strip_prefix('!') {
                    Some(rest) if adds => (rest.trim(), true),
                    _ => (text, false),
                };
                Term {
                    language: language.to_string(),
                    value: value.to_string(),
                    replace,
                }
            };
            for (at, c) in read.columns.iter().enumerate() {
                let text = cell(at);
                if text.is_empty() {
                    continue;
                }
                match c {
                    Column::Label(l) => labels.push(term(l, text)),
                    Column::Description(l) => descriptions.push(term(l, text)),
                    Column::Aliases(l) => {
                        let list = split_values(text);
                        if !list.is_empty() {
                            aliases.push((l.clone(), list));
                        }
                    }
                    Column::Property(_) | Column::New { .. } => {
                        let (property, name) = match c {
                            Column::Property(p) => (Prop::Id(p.clone()), p.clone()),
                            Column::New { label, .. } => (
                                Prop::New(read.declared(label).unwrap_or(usize::MAX)),
                                format!("new:{label}"),
                            ),
                            _ => continue,
                        };
                        if let Prop::Id(p) = &property {
                            own.insert(p.clone());
                        }
                        for v in split_values(text) {
                            match value_of(&property, &v, &format!("{name}:{v}")) {
                                Ok(value) => statements.push(Stmt {
                                    property: property.clone(),
                                    value,
                                    reference: Vec::new(),
                                }),
                                Err(e) => errors.push(e),
                            }
                        }
                    }
                    Column::Id | Column::Reference { .. } | Column::Plain(_) => {}
                }
            }
            if target != Target::Skip {
                for s in &read.shared {
                    if own.contains(&s.property) {
                        continue;
                    }
                    let property = Prop::Id(s.property.clone());
                    match value_of(&property, &s.value, &s.raw) {
                        Ok(value) => statements.push(Stmt {
                            property,
                            value,
                            reference: Vec::new(),
                        }),
                        Err(e) => errors.push(e),
                    }
                }
            }
            // The reference on every statement, read once for the row.
            let mut default = Vec::new();
            if !statements.is_empty() {
                for l in &read.reference {
                    let text = match l.column() {
                        Some(name) => column_at(name).map_or_else(
                            || {
                                errors.push(m.with("ts-createentities-no-column", &[name, &l.raw]));
                                ""
                            },
                            cell,
                        ),
                        None => l.value.as_str(),
                    };
                    default.extend(snaks_of(&l.property, text, &mut errors));
                }
            }
            // A statement's own reference replaces it (0106 §8).
            let mut per: BTreeMap<String, Vec<Snak>> = BTreeMap::new();
            for (at, c) in read.columns.iter().enumerate() {
                if let Column::Reference { of, property } = c {
                    let text = cell(at);
                    if text.is_empty() {
                        continue;
                    }
                    if !statements
                        .iter()
                        .any(|s| s.property == Prop::Id(of.clone()))
                    {
                        errors.push(m.with(
                            "ts-createentities-ref-no-statement",
                            &[read.headers.get(at).map_or("", String::as_str), of],
                        ));
                        continue;
                    }
                    per.entry(of.clone()).or_default().extend(snaks_of(
                        property,
                        text,
                        &mut errors,
                    ));
                }
            }
            for s in &mut statements {
                s.reference = match &s.property {
                    Prop::Id(p) if per.contains_key(p) => per[p].clone(),
                    _ => default.clone(),
                };
            }
            let shown = read
                .columns
                .iter()
                .enumerate()
                .find(|(at, c)| matches!(c, Column::Label(_)) && !cell(*at).is_empty())
                .map(|(at, _)| cell(at).trim_start_matches('!').trim().to_string())
                .or_else(|| cells.iter().find(|c| !c.is_empty()).cloned())
                .unwrap_or_default();
            match target {
                Target::Skip => errors.clear(),
                Target::Existing(_) => {
                    if labels.is_empty()
                        && descriptions.is_empty()
                        && aliases.is_empty()
                        && statements.is_empty()
                        && errors.is_empty()
                    {
                        errors.push(m.get("ts-createentities-nothing-to-add"));
                    }
                }
                _ => {
                    if labels.is_empty() && descriptions.is_empty() && aliases.is_empty() {
                        errors.push(m.get("ts-new-insufficient"));
                    }
                }
            }
            Plan {
                n: i + 1,
                shown,
                target,
                auto,
                labels,
                descriptions,
                aliases,
                statements,
                errors,
                conflicts: 0,
            }
        })
        .collect()
}

/// Checks a row that adds to `entity` against it (0106 §5, A1): a term the item already
/// has as written is dropped; one that differs, with no `!`, is a conflict the row waits
/// on; aliases the item has are dropped. `entity` is `None` when there is no such item.
pub fn against(m: &Messages, plan: &mut Plan, entity: Option<&Value>) {
    let Target::Existing(id) = &plan.target else {
        return;
    };
    let Some(entity) = entity else {
        plan.errors.push(m.with("ts-createentities-no-item", &[id]));
        return;
    };
    let mut conflicts = Vec::new();
    for (key, terms, msg) in [
        (
            "labels",
            &mut plan.labels,
            "ts-createentities-conflict-label",
        ),
        (
            "descriptions",
            &mut plan.descriptions,
            "ts-createentities-conflict-description",
        ),
    ] {
        terms.retain(|t| match entity[key][&t.language]["value"].as_str() {
            Some(have) if have == t.value => false,
            Some(have) if !t.replace => {
                conflicts.push(m.with(msg, &[&t.language, have, &t.value, id]));
                true
            }
            _ => true,
        });
    }
    for (lang, list) in &mut plan.aliases {
        let have: Vec<&str> = entity["aliases"][lang.as_str()]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x["value"].as_str()).collect())
            .unwrap_or_default();
        list.retain(|a| !have.contains(&a.as_str()));
    }
    plan.aliases.retain(|(_, list)| !list.is_empty());
    plan.conflicts += conflicts.len();
    plan.errors.extend(conflicts);
}

/// The order in which the writable rows are written, as positions in `plans`: every row
/// after the rows it names with `row:n` (0106 §2). A row naming a row that is not there or
/// is not written, itself, or a row that names it back, is marked and left out.
pub fn order(m: &Messages, plans: &mut [Plan]) -> Vec<usize> {
    let at: BTreeMap<usize, usize> = plans.iter().enumerate().map(|(i, p)| (p.n, i)).collect();
    // Rows naming what will not be written: repeated, since each row marked may be named.
    loop {
        let mut marked = Vec::new();
        for (i, p) in plans.iter().enumerate() {
            if !p.writable() {
                continue;
            }
            for n in p.needs() {
                let ok = n != p.n && at.get(&n).is_some_and(|j| plans[*j].writable());
                if !ok {
                    marked.push((i, n));
                }
            }
        }
        if marked.is_empty() {
            break;
        }
        for (i, n) in marked {
            plans[i]
                .errors
                .push(m.with("ts-createentities-row-unresolved", &[&n.to_string()]));
        }
    }
    let mut done: BTreeSet<usize> = BTreeSet::new();
    let mut out = Vec::new();
    loop {
        let before = out.len();
        for (i, p) in plans.iter().enumerate() {
            if p.writable() && !done.contains(&p.n) && p.needs().iter().all(|n| done.contains(n)) {
                done.insert(p.n);
                out.push(i);
            }
        }
        if out.len() == before {
            break;
        }
    }
    for p in plans.iter_mut() {
        if p.writable() && !done.contains(&p.n) {
            p.errors.push(m.get("ts-createentities-row-cycle"));
        }
    }
    out
}

/// What a write needs that only the run knows: the IDs of the properties it created and
/// of the items its rows became.
#[derive(Debug, Clone, Default)]
pub struct Known {
    /// The declared properties' IDs, by position.
    pub properties: Vec<Option<String>>,
    /// Each written row's item.
    pub rows: BTreeMap<usize, String>,
}

/// A statement as Wikibase JSON, or the name of what it waits on.
fn claim(s: &Stmt, known: &Known) -> Result<(String, Value), String> {
    let property = match &s.property {
        Prop::Id(p) => p.clone(),
        Prop::New(i) => known
            .properties
            .get(*i)
            .cloned()
            .flatten()
            .ok_or_else(|| format!("new:{i}"))?,
    };
    let datavalue = match &s.value {
        Val::Data(v) => v.clone(),
        Val::Row(n) => {
            let id = known.rows.get(n).ok_or_else(|| format!("row:{n}"))?;
            json!({"type": "wikibase-entityid", "value": {"entity-type": "item", "id": id}})
        }
    };
    let mut c = json!({
        "mainsnak": {"snaktype": "value", "property": property, "datavalue": datavalue},
        "type": "statement",
        "rank": "normal",
    });
    if !s.reference.is_empty() {
        c["references"] = json!([reference(&s.reference)]);
    }
    Ok((property, c))
}

/// A reference as Wikibase JSON.
fn reference(snaks: &[Snak]) -> Value {
    let mut map: Map<String, Value> = Map::new();
    let mut order = Vec::new();
    for s in snaks {
        if !map.contains_key(&s.property) {
            order.push(json!(s.property));
        }
        if let Some(a) = map
            .entry(s.property.clone())
            .or_insert_with(|| json!([]))
            .as_array_mut()
        {
            a.push(json!({"snaktype": "value", "property": s.property, "datavalue": s.datavalue}));
        }
    }
    json!({"snaks": map, "snaks-order": order})
}

/// Whether a data value the API served is the one planned: every key the plan gives
/// is the same, the API adding its own beside them (`numeric-id`, a quantity's bounds).
fn same(planned: &Value, served: &Value) -> bool {
    match (planned, served) {
        (Value::Object(a), Value::Object(b)) => {
            a.iter().all(|(k, v)| b.get(k).is_some_and(|w| same(v, w)))
        }
        _ => planned == served,
    }
}

/// Whether a served reference holds exactly the planned snaks.
fn same_reference(planned: &Value, served: &Value) -> bool {
    let (Some(a), Some(b)) = (planned["snaks"].as_object(), served["snaks"].as_object()) else {
        return false;
    };
    a.len() == b.len()
        && a.iter().all(|(p, list)| {
            let (Some(x), Some(y)) = (list.as_array(), b.get(p).and_then(Value::as_array)) else {
                return false;
            };
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|(s, t)| same(&s["datavalue"], &t["datavalue"]))
        })
}

/// `wbeditentity`'s `data` for a row that creates an item.
///
/// # Errors
///
/// The name of the row or property the row waits on, when it was not written.
pub fn create_data(plan: &Plan, known: &Known) -> Result<Value, String> {
    let mut data = Map::new();
    let terms = |list: &[Term]| -> Value {
        Value::Object(
            list.iter()
                .map(|t| {
                    (
                        t.language.clone(),
                        json!({"language": t.language, "value": t.value}),
                    )
                })
                .collect(),
        )
    };
    if !plan.labels.is_empty() {
        data.insert("labels".into(), terms(&plan.labels));
    }
    if !plan.descriptions.is_empty() {
        data.insert("descriptions".into(), terms(&plan.descriptions));
    }
    if !plan.aliases.is_empty() {
        let mut map = Map::new();
        for (lang, list) in &plan.aliases {
            map.insert(
                lang.clone(),
                list.iter()
                    .map(|a| json!({"language": lang, "value": a}))
                    .collect(),
            );
        }
        data.insert("aliases".into(), Value::Object(map));
    }
    let mut claims: Map<String, Value> = Map::new();
    for s in &plan.statements {
        let (property, c) = claim(s, known)?;
        let group = claims.entry(property).or_insert_with(|| json!([]));
        // The same value twice in a row is one statement.
        if let Some(a) = group.as_array_mut()
            && !a.iter().any(|x| x["mainsnak"] == c["mainsnak"])
        {
            a.push(c);
        }
    }
    if !claims.is_empty() {
        data.insert("claims".into(), Value::Object(claims));
    }
    Ok(Value::Object(data))
}

/// `wbeditentity`'s `data` for a row that adds to `entity` (0106 §5; 05 §3.5), or `None`
/// when the item already holds everything the row gives. Terms the item lacks are set,
/// and those marked to replace; aliases are appended; a statement the item already has,
/// with no qualifiers, takes the row's reference where it lacks it, and is otherwise left
/// alone; any other statement is added.
///
/// # Errors
///
/// The name of the row or property the row waits on, when it was not written; or the
/// term that differs from the item's, when the item changed since the preview.
pub fn add_data(plan: &Plan, entity: &Value, known: &Known) -> Result<Option<Value>, String> {
    let mut data = Map::new();
    for (key, terms) in [
        ("labels", &plan.labels),
        ("descriptions", &plan.descriptions),
    ] {
        let mut map = Map::new();
        for t in terms {
            match entity[key][&t.language]["value"].as_str() {
                Some(have) if have == t.value => {}
                Some(have) if !t.replace => return Err(format!("{}: {have}", t.language)),
                _ => {
                    map.insert(
                        t.language.clone(),
                        json!({"language": t.language, "value": t.value}),
                    );
                }
            }
        }
        if !map.is_empty() {
            data.insert(key.into(), Value::Object(map));
        }
    }
    let mut added = Vec::new();
    for (lang, list) in &plan.aliases {
        let have: Vec<&str> = entity["aliases"][lang.as_str()]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x["value"].as_str()).collect())
            .unwrap_or_default();
        for a in list {
            if !have.contains(&a.as_str()) {
                added.push(json!({"language": lang, "value": a, "add": ""}));
            }
        }
    }
    if !added.is_empty() {
        data.insert("aliases".into(), Value::Array(added));
    }
    let mut claims: Vec<Value> = Vec::new();
    for s in &plan.statements {
        let (property, c) = claim(s, known)?;
        if claims
            .iter()
            .any(|x| x.get("id").is_none() && x["mainsnak"] == c["mainsnak"])
        {
            continue;
        }
        let existing = entity["claims"][&property].as_array().and_then(|a| {
            a.iter().find(|e| {
                e["mainsnak"]["snaktype"] == "value"
                    && same(&c["mainsnak"]["datavalue"], &e["mainsnak"]["datavalue"])
                    && e["qualifiers"].as_object().is_none_or(Map::is_empty)
            })
        });
        match existing {
            None => claims.push(c),
            Some(e) => {
                let Some(new_ref) = c["references"].get(0) else {
                    continue;
                };
                let has = e["references"]
                    .as_array()
                    .is_some_and(|r| r.iter().any(|x| same_reference(new_ref, x)));
                if has {
                    continue;
                }
                // The statement as it stands, by its ID, with the reference beside its own.
                let mut e = e.clone();
                let mut refs = e["references"].as_array().cloned().unwrap_or_default();
                refs.push(new_ref.clone());
                e["references"] = Value::Array(refs);
                claims.push(e);
            }
        }
    }
    if !claims.is_empty() {
        data.insert("claims".into(), Value::Array(claims));
    }
    Ok((!data.is_empty()).then_some(Value::Object(data)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::special::create_entities::table::lines_of;

    fn lookup() -> Lookup {
        let mut found = BTreeMap::new();
        for (id, dt) in [
            ("P31", "wikibase-item"),
            ("P12", "quantity"),
            ("P577", "time"),
            ("P854", "url"),
            ("P248", "wikibase-item"),
            ("P123", "wikibase-item"),
        ] {
            found.insert(
                id.to_string(),
                json!({"id": id, "datatype": dt, "labels": {}}),
            );
        }
        Lookup::from_entities(&found, &["en".to_string()])
    }

    fn read(table: &[&[&str]], shared: &str, reference: &str) -> Read {
        let rows: Vec<Vec<String>> = table
            .iter()
            .map(|r| r.iter().map(ToString::to_string).collect())
            .collect();
        let ok = |t: &str| lines_of(t).into_iter().map(Result::unwrap).collect();
        Read::of(&rows[0], &rows[1..], ok(shared), ok(reference), "en")
    }

    // 0104 §11: cells parsed against their properties, each row's problems listed.
    #[test]
    fn rows_are_planned_with_their_errors() {
        let m = Messages::for_language("en");
        let r = read(
            &[
                &["label", "P31", "P12", "notes"],
                &["Six", "Q5", "6", "x"],
                &["", "Q5", "", ""],
                &["Eight", "nonsense", "eight", ""],
            ],
            "P31:Q1",
            "",
        );
        let plans = plan(&m, &r, &lookup());
        assert_eq!(plans.len(), 3);
        assert!(plans[0].errors.is_empty(), "{:?}", plans[0].errors);
        assert_eq!((plans[0].n, plans[0].shown.as_str()), (1, "Six"));
        // 0106 §2: the row's own P31 replaces the one every row shares.
        assert_eq!(plans[0].statements.len(), 2);
        assert_eq!(plans[1].errors.len(), 1, "no terms: {:?}", plans[1].errors);
        assert_eq!(plans[2].errors.len(), 2, "{:?}", plans[2].errors);
        assert_eq!(plans[2].status(), Status::Invalid(2));
        let data = create_data(&plans[0], &Known::default()).unwrap();
        assert_eq!(data["labels"]["en"]["value"], "Six");
        assert_eq!(
            data["claims"]["P12"][0]["mainsnak"]["datavalue"]["value"]["amount"],
            "+6"
        );
    }

    // 0106 §8: the reference on every statement, read from a column; a statement's own
    // reference replaces it.
    #[test]
    #[allow(clippy::many_single_char_names, clippy::too_many_lines)]
    fn a_statements_own_reference_replaces_the_default() {
        let m = Messages::for_language("en");
        let r = read(
            &[
                &["label", "P577", "P577/ref:P854", "source url"],
                &[
                    "A",
                    "1969",
                    "https://example.org/own",
                    "https://example.org/a",
                ],
                &["B", "1970", "", "https://example.org/b"],
                &["C", "", "https://example.org/stray", ""],
            ],
            "P31:Q5",
            "P248:Q1234\nP854:{source url}",
        );
        let plans = plan(&m, &r, &lookup());
        assert!(plans[0].errors.is_empty(), "{:?}", plans[0].errors);
        let a = create_data(&plans[0], &Known::default()).unwrap();
        let own = &a["claims"]["P577"][0]["references"];
        assert_eq!(own.as_array().unwrap().len(), 1);
        assert!(
            own[0]["snaks"].get("P248").is_none(),
            "replaced, not added: {own}"
        );
        assert_eq!(
            own[0]["snaks"]["P854"][0]["datavalue"]["value"],
            "https://example.org/own"
        );
        let shared = &a["claims"]["P31"][0]["references"][0]["snaks"];
        assert_eq!(shared["P248"][0]["datavalue"]["value"]["id"], "Q1234");
        assert_eq!(
            shared["P854"][0]["datavalue"]["value"],
            "https://example.org/a"
        );
        let b = create_data(&plans[1], &Known::default()).unwrap();
        assert_eq!(
            b["claims"]["P577"][0]["references"][0]["snaks"]["P854"][0]["datavalue"]["value"],
            "https://example.org/b"
        );
        // A reference with no statement to carry it is a problem; an empty column cell
        // leaves that snak out.
        assert_eq!(plans[2].errors.len(), 1, "{:?}", plans[2].errors);
        let c = &plans[2].statements[0].reference;
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].property, "P248");
    }

    // 0106 §2: `row:n` orders the writes; a cycle and what names it are left out.
    #[test]
    fn rows_are_written_after_the_rows_they_name() {
        let m = Messages::for_language("en");
        let r = read(
            &[
                &["label", "P123"],
                &["Paper", "row:2"],
                &["Publisher", ""],
                &["Loop a", "row:4"],
                &["Loop b", "row:3"],
                &["Names a loop", "row:3"],
                &["Names nothing", "row:9"],
                &["Names itself", "row:7"],
            ],
            "",
            "",
        );
        let mut plans = plan(&m, &r, &lookup());
        let order = order(&m, &mut plans);
        assert_eq!(order, vec![1, 0], "the publisher first");
        for p in &plans[2..7] {
            assert_eq!(p.errors.len(), 1, "row {}: {:?}", p.n, p.errors);
        }
        assert!(create_data(&plans[0], &Known::default()).is_err());
        let mut known = Known::default();
        known.rows.insert(2, "Q77".into());
        let data = create_data(&plans[0], &known).unwrap();
        assert_eq!(
            data["claims"]["P123"][0]["mainsnak"]["datavalue"]["value"]["id"],
            "Q77"
        );
    }

    // 0106 §4: a declared property's data type, proposed or given, and its statements.
    #[test]
    fn new_properties_are_declared_and_typed() {
        let m = Messages::for_language("en");
        let r = read(
            &[
                &[
                    "label",
                    "new:Founded",
                    "new:Publisher@wikibase-item",
                    "new:Motto",
                ],
                &["A", "1969", "row:2", "x"],
                &["B", "1970-05", "", "1969"],
            ],
            "",
            "",
        );
        assert_eq!(r.new.len(), 3);
        assert_eq!(propose(&m, &r.values_of("Founded")), Some("time"));
        assert_eq!(
            propose(&m, &r.values_of("Publisher")),
            Some("wikibase-item")
        );
        assert_eq!(propose(&m, &r.values_of("Motto")), None);
        let plans = plan(&m, &r, &lookup());
        // Untyped columns hold the rows back until a data type is confirmed.
        assert_eq!(plans[0].errors.len(), 2, "{:?}", plans[0].errors);
        let mut typed = r.clone();
        typed.new[0].datatype = Some("time".into());
        typed.new[2].datatype = Some("string".into());
        let plans = plan(&m, &typed, &lookup());
        assert!(plans[0].errors.is_empty(), "{:?}", plans[0].errors);
        let known = Known {
            properties: vec![Some("P7".into()), Some("P8".into()), Some("P9".into())],
            rows: [(2, "Q2".to_string())].into(),
        };
        let data = create_data(&plans[0], &known).unwrap();
        assert_eq!(
            data["claims"]["P7"][0]["mainsnak"]["datavalue"]["value"]["precision"],
            9
        );
        assert_eq!(
            data["claims"]["P8"][0]["mainsnak"]["datavalue"]["value"]["id"],
            "Q2"
        );
        assert_eq!(
            data["claims"]["P9"][0]["mainsnak"]["datavalue"]["value"],
            "x"
        );
    }

    // 0106 §5, A1: a row with an `id` adds to that item.
    #[test]
    #[allow(clippy::many_single_char_names, clippy::too_many_lines)]
    fn a_matched_row_adds_to_its_item() {
        let m = Messages::for_language("en");
        let entity = json!({
            "id": "Q5",
            "labels": {"en": {"language": "en", "value": "Six"}},
            "descriptions": {"en": {"language": "en", "value": "a number"}},
            "aliases": {"en": [{"language": "en", "value": "half dozen"}]},
            "claims": {"P31": [
                {"id": "Q5$a", "type": "statement", "rank": "normal",
                 "mainsnak": {"snaktype": "value", "property": "P31", "datavalue":
                    {"type": "wikibase-entityid", "value": {"entity-type": "item", "id": "Q1", "numeric-id": 1}}}},
                {"id": "Q5$b", "type": "statement", "rank": "normal",
                 "mainsnak": {"snaktype": "value", "property": "P31", "datavalue":
                    {"type": "wikibase-entityid", "value": {"entity-type": "item", "id": "Q2"}}},
                 "qualifiers": {"P12": [{}]}}
            ]},
        });
        let r = read(
            &[
                &[
                    "id",
                    "label",
                    "description",
                    "label:de",
                    "aliases",
                    "P31",
                    "P12",
                ],
                &[
                    "Q5",
                    "Six",
                    "the number 6",
                    "Sechs",
                    "half dozen|6",
                    "Q1|Q2",
                    "6",
                ],
                &["Q5", "!VI", "!the number 6", "", "", "", ""],
                &["skip", "x", "", "", "", "bad", ""],
                &["q9", "", "", "", "", "", ""],
            ],
            "",
            "P248:Q1234",
        );
        let mut plans = plan(&m, &r, &lookup());
        assert!(!plans[0].auto);
        against(&m, &mut plans[0], Some(&entity));
        // The same label is dropped; the differing description waits on its intent.
        assert_eq!(
            plans[0].status(),
            Status::TermConflict(1),
            "{:?}",
            plans[0].errors
        );
        assert_eq!(plans[0].labels.len(), 1, "only the German label is left");
        assert_eq!(
            plans[0].aliases,
            vec![("en".to_string(), vec!["6".to_string()])]
        );
        // With the description cleared, the row adds what the item lacks.
        plans[0].errors.clear();
        plans[0].conflicts = 0;
        plans[0].descriptions.clear();
        assert_eq!(plans[0].status(), Status::Matches("Q5".into()));
        let data = add_data(&plans[0], &entity, &Known::default())
            .unwrap()
            .unwrap();
        assert_eq!(data["labels"]["de"]["value"], "Sechs");
        assert!(data.get("descriptions").is_none());
        assert_eq!(
            data["aliases"],
            json!([{"language": "en", "value": "6", "add": ""}])
        );
        let claims = data["claims"].as_array().unwrap();
        // Q1 is there: it takes the reference under its own ID. Q2 is there only with a
        // qualifier, so the row's is another statement. The quantity is new.
        assert_eq!(claims.len(), 3, "{claims:?}");
        assert_eq!(claims[0]["id"], "Q5$a");
        assert_eq!(claims[0]["references"].as_array().unwrap().len(), 1);
        assert!(claims[1].get("id").is_none());
        assert_eq!(claims[2]["mainsnak"]["property"], "P12");
        // Run again over the result, nothing is left to add.
        let mut after = entity.clone();
        after["claims"]["P31"][0]["references"] = claims[0]["references"].clone();
        after["claims"]["P31"]
            .as_array_mut()
            .unwrap()
            .push(claims[1].clone());
        after["claims"]["P12"] = json!([claims[2]]);
        after["labels"]["de"] = json!({"language": "de", "value": "Sechs"});
        after["aliases"]["en"]
            .as_array_mut()
            .unwrap()
            .push(json!({"language": "en", "value": "6"}));
        assert_eq!(
            add_data(&plans[0], &after, &Known::default()).unwrap(),
            None
        );

        // `!` replaces the item's term.
        against(&m, &mut plans[1], Some(&entity));
        assert_eq!(plans[1].status(), Status::Matches("Q5".into()));
        let data = add_data(&plans[1], &entity, &Known::default())
            .unwrap()
            .unwrap();
        assert_eq!(data["labels"]["en"]["value"], "VI");
        assert_eq!(data["descriptions"]["en"]["value"], "the number 6");
        // A skipped row has no problems; a row with only an `id` has nothing to add; an
        // item that is not there is said.
        assert_eq!(plans[2].status(), Status::Skipped);
        assert_eq!(plans[3].status(), Status::Invalid(1));
        let mut gone = plans[1].clone();
        against(&m, &mut gone, None);
        assert_eq!(gone.status(), Status::Invalid(1));
    }
}
