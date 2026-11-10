//! The `triplespace-table` content model (0045 §3, §4; 0106 §9; 15 §2.2, §2.3, §2.7): a
//! table's definition, which names the entities that are its rows and the fields that are
//! its columns, and stores no values except a draft's staged rows.
//!
//! - [`Definition::parse`] validates a definition against the schema: every key known,
//!   every ID well formed, no row twice, the limits kept. It does not check that an entity
//!   or property exists (15 §2.3).
//! - [`Definition::canonical`] is the pre-save transform and the serialization: keys in
//!   schema order, two-space indentation, one row ID per line and one staged row per
//!   line, so that a table diffs and merges row by row.
//!
//! Of the `rows` kinds, only `ids` is read here; the scope kinds (0060 §6) join with
//! `scatter-scope`.

use std::collections::BTreeSet;
use std::fmt;
use std::fmt::Write as _;

use scatter_wikibase_model::id::EntityId;
use serde_json::{Map, Value};

/// The model's ID.
pub const MODEL: &str = "triplespace-table";

/// The limits a definition is held to: settings, passed in (`tables.max_rows`,
/// `tables.max_columns`; 23).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most rows, staged rows included.
    pub max_rows: usize,
    /// The most columns.
    pub max_columns: usize,
}

impl Default for Limits {
    /// The settings' defaults.
    fn default() -> Self {
        Self {
            max_rows: 5000,
            max_columns: 50,
        }
    }
}

/// What is wrong with a definition, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Where, as a path into the definition (`columns[2].property`); empty for the whole.
    pub path: String,
    /// What, in a sentence.
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

/// The property of a `statements` column: one that exists, or one a draft declares
/// (0106 §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Property {
    /// A property's ID.
    Id(EntityId),
    /// The position of a property in `staged.properties`.
    New(usize),
}

/// What a column shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    /// The label in a language.
    Label(String),
    /// The description in a language.
    Description(String),
    /// The aliases in a language.
    Aliases(String),
    /// A property's best values.
    Statements(Property),
    /// The page linked on a site.
    Sitelink(String),
    /// The thumbnail of a file row, at a width in pixels.
    Thumbnail(u32),
    /// A value of a reference (0106 §9, A5): of the statements of one `statements` column,
    /// in place of the default reference, or, with no column named, a value the default
    /// reference reads through a snak's `"column"`.
    Reference {
        /// The reference snak's property.
        property: EntityId,
        /// The position of the `statements` column whose statements carry it; `None`
        /// for a value the default reference reads.
        of: Option<usize>,
    },
}

/// One column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// What it shows.
    pub field: Field,
    /// A header that overrides the generated one.
    pub title: Option<String>,
}

/// A direction of the default order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Smallest first.
    Ascending,
    /// Largest first.
    Descending,
}

/// One key of the default order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    /// The column's position, from zero.
    pub column: usize,
    /// The direction.
    pub order: Order,
}

/// A property a draft declares and has not created yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProperty {
    /// Its label in the wiki's language.
    pub label: String,
    /// Its data type, once confirmed.
    pub datatype: Option<String>,
}

/// A row not yet written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedRow {
    /// Its number in the table, which `row:n` names.
    pub n: u64,
    /// One string per column, in the text form of 21 §6.6.
    pub cells: Vec<String>,
    /// The existing item the row is matched to.
    pub id: Option<EntityId>,
    /// Whether the row is left out.
    pub skip: bool,
    /// Whether the row creates an item whatever it matches (0106 A4, A5).
    pub create: bool,
    /// Why an import refused it.
    pub refused: Option<String>,
}

/// A draft's rows and what they share (15 §2.7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Staged {
    /// The statements on every item, one `P:value` each.
    pub shared: Vec<String>,
    /// The properties the table declares.
    pub properties: Vec<NewProperty>,
    /// The rows.
    pub rows: Vec<StagedRow>,
}

/// A table's definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    /// Plain text shown under the title.
    pub description: Option<String>,
    /// The rows: entity IDs as written.
    pub rows: Vec<EntityId>,
    /// The columns, in order.
    pub columns: Vec<Column>,
    /// One reference in Wikibase's JSON, without its hash; a snak may name a column in
    /// place of a value.
    pub default_reference: Option<Value>,
    /// Rows not yet written.
    pub staged: Option<Staged>,
    /// The default order.
    pub sort: Vec<Sort>,
}

/// Checks an ID against what only the caller knows, the provider and keyed-type
/// registries, and gives its stored form; `None` refuses it. [`Definition::parse`] checks
/// the shape alone.
pub type IdCheck<'a> = &'a dyn Fn(&EntityId) -> Option<EntityId>;

/// Collects problems while reading.
struct Reader<'a> {
    problems: Vec<Problem>,
    check: IdCheck<'a>,
}

impl Reader<'_> {
    fn bad(&mut self, path: &str, message: impl Into<String>) {
        self.problems.push(Problem {
            path: path.to_string(),
            message: message.into(),
        });
    }

    /// The object at `path`, with every key outside `known` reported.
    fn object<'a>(
        &mut self,
        v: &'a Value,
        path: &str,
        known: &[&str],
    ) -> Option<&'a Map<String, Value>> {
        let Some(o) = v.as_object() else {
            self.bad(path, "must be an object");
            return None;
        };
        for k in o.keys() {
            if !known.contains(&k.as_str()) {
                self.bad(path, format!("unknown key \"{k}\""));
            }
        }
        Some(o)
    }

    fn string(&mut self, o: &Map<String, Value>, path: &str, key: &str) -> Option<String> {
        match o.get(key) {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Some(_) => {
                self.bad(
                    &format!("{path}.{key}"),
                    "must be a string that is not empty",
                );
                None
            }
            None => {
                self.bad(path, format!("needs \"{key}\""));
                None
            }
        }
    }

    fn id(&mut self, text: &str, path: &str) -> Option<EntityId> {
        let id = EntityId::parse(text.trim())
            .ok()
            .and_then(|id| (self.check)(&id));
        if id.is_none() {
            self.bad(path, format!("\"{text}\" is not an entity ID"));
        }
        id
    }

    fn property(&mut self, text: &str, path: &str) -> Option<EntityId> {
        let id = self.id(text, path)?;
        if id.implied_entity_type() == Some("property") {
            Some(id)
        } else {
            self.bad(path, format!("\"{text}\" is not a property ID"));
            None
        }
    }

    fn index(&mut self, v: Option<&Value>, path: &str) -> Option<usize> {
        let n = v
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok());
        if n.is_none() {
            self.bad(path, "must be a position, counted from zero");
        }
        n
    }
}

/// A language code's shape. Whether the wiki knows the language is not checked here.
fn is_language(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 42
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Definition {
    /// Reads and validates a definition (15 §2.3, §2.7), each ID checked by its shape.
    ///
    /// # Errors
    ///
    /// Every problem found, each with its place.
    pub fn parse(text: &str, limits: Limits) -> Result<Self, Vec<Problem>> {
        Self::parse_with(text, limits, &|id| Some(id.clone()))
    }

    /// [`Self::parse`], with each ID also checked, and given its stored form, by `check`.
    ///
    /// # Errors
    ///
    /// Every problem found, each with its place.
    #[allow(clippy::too_many_lines)]
    pub fn parse_with(text: &str, limits: Limits, check: IdCheck) -> Result<Self, Vec<Problem>> {
        let mut r = Reader {
            problems: Vec::new(),
            check,
        };
        let v: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                r.bad("", format!("not JSON: {e}"));
                return Err(r.problems);
            }
        };
        let Some(top) = r.object(
            &v,
            "",
            &[
                "version",
                "description",
                "rows",
                "columns",
                "default_reference",
                "staged",
                "sort",
            ],
        ) else {
            return Err(r.problems);
        };
        if top.get("version").and_then(Value::as_u64) != Some(1) {
            r.bad("version", "must be 1");
        }
        let description = match top.get("description") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()).filter(|s| !s.trim().is_empty()),
            Some(_) => {
                r.bad("description", "must be a string");
                None
            }
        };

        // Rows.
        let mut rows = Vec::new();
        let mut is_ids = false;
        match top.get("rows") {
            None => r.bad("", "needs \"rows\""),
            Some(v) => {
                if let Some(o) = v.as_object() {
                    if o.len() != 1 {
                        r.bad("rows", "must hold exactly one scope kind");
                    }
                    match o.get("ids") {
                        Some(Value::Array(ids)) => {
                            is_ids = true;
                            let mut seen = BTreeSet::new();
                            for (i, id) in ids.iter().enumerate() {
                                let path = format!("rows.ids[{i}]");
                                let Some(text) = id.as_str() else {
                                    r.bad(&path, "must be an entity ID");
                                    continue;
                                };
                                if let Some(id) = r.id(text, &path) {
                                    if seen.insert(id.as_str().to_string()) {
                                        rows.push(id);
                                    } else {
                                        r.bad(&path, format!("{} is a row already", id.as_str()));
                                    }
                                }
                            }
                        }
                        Some(_) => r.bad("rows.ids", "must be a list of entity IDs"),
                        None => {
                            for k in o.keys() {
                                r.bad(
                                    "rows",
                                    format!(
                                        "the scope kind \"{k}\" is not supported yet; use \"ids\""
                                    ),
                                );
                            }
                        }
                    }
                } else {
                    r.bad("rows", "must be an object");
                }
            }
        }

        // Columns.
        let mut columns = Vec::new();
        match top.get("columns") {
            Some(Value::Array(list)) if !list.is_empty() => {
                if list.len() > limits.max_columns {
                    r.bad(
                        "columns",
                        format!(
                            "{} columns; a table has at most {}",
                            list.len(),
                            limits.max_columns
                        ),
                    );
                }
                for (i, c) in list.iter().enumerate() {
                    let path = format!("columns[{i}]");
                    let Some(field) = c.get("field").and_then(Value::as_str) else {
                        r.bad(&path, "needs \"field\"");
                        continue;
                    };
                    let keys: &[&str] = match field {
                        "label" | "description" | "aliases" => &["field", "language", "title"],
                        "statements" => &["field", "property", "new", "title"],
                        "sitelink" => &["field", "site", "title"],
                        "thumbnail" => &["field", "width", "title"],
                        "reference" => &["field", "property", "of", "title"],
                        other => {
                            r.bad(&path, format!("unknown field \"{other}\""));
                            continue;
                        }
                    };
                    let Some(o) = r.object(c, &path, keys) else {
                        continue;
                    };
                    let title = match o.get("title") {
                        None | Some(Value::Null) => None,
                        Some(Value::String(s)) => Some(s.clone()).filter(|s| !s.trim().is_empty()),
                        Some(_) => {
                            r.bad(&format!("{path}.title"), "must be a string");
                            None
                        }
                    };
                    let field = match field {
                        "label" | "description" | "aliases" => {
                            let Some(lang) = r.string(o, &path, "language") else {
                                continue;
                            };
                            if !is_language(&lang) {
                                r.bad(
                                    &format!("{path}.language"),
                                    format!("\"{lang}\" is not a language code"),
                                );
                                continue;
                            }
                            match field {
                                "label" => Field::Label(lang),
                                "description" => Field::Description(lang),
                                _ => Field::Aliases(lang),
                            }
                        }
                        "statements" => match (o.get("property"), o.get("new")) {
                            (Some(_), Some(_)) => {
                                r.bad(&path, "takes \"property\" or \"new\", not both");
                                continue;
                            }
                            (None, Some(n)) => {
                                let Some(n) = r.index(Some(n), &format!("{path}.new")) else {
                                    continue;
                                };
                                Field::Statements(Property::New(n))
                            }
                            _ => {
                                let Some(p) = r.string(o, &path, "property") else {
                                    continue;
                                };
                                let Some(p) = r.property(&p, &format!("{path}.property")) else {
                                    continue;
                                };
                                Field::Statements(Property::Id(p))
                            }
                        },
                        "sitelink" => {
                            let Some(site) = r.string(o, &path, "site") else {
                                continue;
                            };
                            Field::Sitelink(site)
                        }
                        "thumbnail" => {
                            let width = o
                                .get("width")
                                .and_then(Value::as_u64)
                                .and_then(|w| u32::try_from(w).ok())
                                .filter(|w| *w > 0);
                            let Some(width) = width else {
                                r.bad(&format!("{path}.width"), "must be a width in pixels");
                                continue;
                            };
                            Field::Thumbnail(width)
                        }
                        _ => {
                            let Some(p) = r.string(o, &path, "property") else {
                                continue;
                            };
                            let Some(property) = r.property(&p, &format!("{path}.property")) else {
                                continue;
                            };
                            let of = match o.get("of") {
                                None | Some(Value::Null) => None,
                                Some(v) => {
                                    let Some(of) = r.index(Some(v), &format!("{path}.of")) else {
                                        continue;
                                    };
                                    Some(of)
                                }
                            };
                            Field::Reference { property, of }
                        }
                    };
                    columns.push(Column { field, title });
                }
                // Positions are checked once every column is read; a column that failed
                // above has already been reported, so the positions may be off by then.
                if columns.len() == list.len() {
                    for (i, c) in columns.iter().enumerate() {
                        if let Field::Reference { of: Some(of), .. } = &c.field
                            && !matches!(
                                columns.get(*of).map(|c| &c.field),
                                Some(Field::Statements(_))
                            )
                        {
                            r.bad(
                                &format!("columns[{i}].of"),
                                "must be the position of a statements column",
                            );
                        }
                    }
                }
            }
            Some(_) => r.bad("columns", "must be a list of one or more columns"),
            None => r.bad("", "needs \"columns\""),
        }
        let width = top
            .get("columns")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);

        // The default reference.
        let default_reference = match top.get("default_reference") {
            None | Some(Value::Null) => None,
            Some(v) => reference(&mut r, v, width),
        };

        // Staged rows.
        let staged = match top.get("staged") {
            None | Some(Value::Null) => None,
            Some(v) => {
                if !is_ids {
                    r.bad("staged", "is allowed only when \"rows\" is \"ids\"");
                }
                staged(&mut r, v, width)
            }
        };
        let declared = staged.as_ref().map_or(0, |s| s.properties.len());
        for (i, c) in columns.iter().enumerate() {
            if let Field::Statements(Property::New(n)) = &c.field
                && *n >= declared
            {
                r.bad(
                    &format!("columns[{i}].new"),
                    "must be the position of a property in staged.properties",
                );
            }
        }
        let total = rows.len() + staged.as_ref().map_or(0, |s| s.rows.len());
        if total > limits.max_rows {
            r.bad(
                "rows",
                format!("{total} rows; a table has at most {}", limits.max_rows),
            );
        }

        // Sort.
        let mut sort = Vec::new();
        match top.get("sort") {
            None | Some(Value::Null) => {}
            Some(Value::Array(list)) => {
                for (i, s) in list.iter().enumerate() {
                    let path = format!("sort[{i}]");
                    let Some(o) = r.object(s, &path, &["column", "order"]) else {
                        continue;
                    };
                    let Some(column) = r.index(o.get("column"), &format!("{path}.column")) else {
                        continue;
                    };
                    if column >= width {
                        r.bad(&format!("{path}.column"), "is not a column of the table");
                        continue;
                    }
                    let order = match o.get("order").and_then(Value::as_str) {
                        Some("ascending") => Order::Ascending,
                        Some("descending") => Order::Descending,
                        _ => {
                            r.bad(
                                &format!("{path}.order"),
                                "must be \"ascending\" or \"descending\"",
                            );
                            continue;
                        }
                    };
                    sort.push(Sort { column, order });
                }
            }
            Some(_) => r.bad("sort", "must be a list"),
        }

        if r.problems.is_empty() {
            Ok(Self {
                description,
                rows,
                columns,
                default_reference,
                staged,
                sort,
            })
        } else {
            Err(r.problems)
        }
    }

    /// The canonical text (15 §2.2): what is stored, served and diffed.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn canonical(&self) -> String {
        let q = |s: &str| Value::String(s.to_string()).to_string();
        let mut out = String::from("{\n  \"version\": 1");
        if let Some(d) = &self.description {
            let _ = write!(out, ",\n  \"description\": {}", q(d));
        }
        out.push_str(",\n  \"rows\": {\n    \"ids\": [");
        for (i, id) in self.rows.iter().enumerate() {
            let _ = write!(
                out,
                "{}\n      {}",
                if i == 0 { "" } else { "," },
                q(id.as_str())
            );
        }
        out.push_str(if self.rows.is_empty() {
            "]\n  }"
        } else {
            "\n    ]\n  }"
        });
        out.push_str(",\n  \"columns\": [");
        for (i, c) in self.columns.iter().enumerate() {
            let mut parts = match &c.field {
                Field::Label(l) => vec![("field", q("label")), ("language", q(l))],
                Field::Description(l) => vec![("field", q("description")), ("language", q(l))],
                Field::Aliases(l) => vec![("field", q("aliases")), ("language", q(l))],
                Field::Statements(Property::Id(p)) => {
                    vec![("field", q("statements")), ("property", q(p.as_str()))]
                }
                Field::Statements(Property::New(n)) => {
                    vec![("field", q("statements")), ("new", n.to_string())]
                }
                Field::Sitelink(s) => vec![("field", q("sitelink")), ("site", q(s))],
                Field::Thumbnail(w) => vec![("field", q("thumbnail")), ("width", w.to_string())],
                Field::Reference { property, of } => {
                    let mut parts = vec![
                        ("field", q("reference")),
                        ("property", q(property.as_str())),
                    ];
                    if let Some(of) = of {
                        parts.push(("of", of.to_string()));
                    }
                    parts
                }
            };
            if let Some(t) = &c.title {
                parts.push(("title", q(t)));
            }
            let _ = write!(
                out,
                "{}\n    {}",
                if i == 0 { "" } else { "," },
                line(&parts)
            );
        }
        out.push_str("\n  ]");
        if let Some(reference) = &self.default_reference {
            out.push_str(",\n  \"default_reference\": ");
            pretty(reference, 1, &mut out);
        }
        if let Some(s) = &self.staged {
            out.push_str(",\n  \"staged\": {\n    \"shared\": [");
            for (i, l) in s.shared.iter().enumerate() {
                let _ = write!(out, "{}\n      {}", if i == 0 { "" } else { "," }, q(l));
            }
            out.push_str(if s.shared.is_empty() { "]" } else { "\n    ]" });
            out.push_str(",\n    \"properties\": [");
            for (i, p) in s.properties.iter().enumerate() {
                let mut parts = vec![("label", q(&p.label))];
                if let Some(d) = &p.datatype {
                    parts.push(("datatype", q(d)));
                }
                let _ = write!(
                    out,
                    "{}\n      {}",
                    if i == 0 { "" } else { "," },
                    line(&parts)
                );
            }
            out.push_str(if s.properties.is_empty() {
                "]"
            } else {
                "\n    ]"
            });
            out.push_str(",\n    \"rows\": [");
            for (i, row) in s.rows.iter().enumerate() {
                let cells: Vec<String> = row.cells.iter().map(|c| q(c)).collect();
                let mut parts = vec![
                    ("n", row.n.to_string()),
                    ("cells", format!("[{}]", cells.join(", "))),
                ];
                if let Some(id) = &row.id {
                    parts.push(("id", q(id.as_str())));
                }
                if row.skip {
                    parts.push(("skip", "true".into()));
                }
                if row.create {
                    parts.push(("create", "true".into()));
                }
                if let Some(why) = &row.refused {
                    parts.push(("refused", q(why)));
                }
                let _ = write!(
                    out,
                    "{}\n      {}",
                    if i == 0 { "" } else { "," },
                    line(&parts)
                );
            }
            out.push_str(if s.rows.is_empty() { "]" } else { "\n    ]" });
            out.push_str("\n  }");
        }
        if !self.sort.is_empty() {
            out.push_str(",\n  \"sort\": [");
            for (i, s) in self.sort.iter().enumerate() {
                let order = match s.order {
                    Order::Ascending => "ascending",
                    Order::Descending => "descending",
                };
                let parts = [("column", s.column.to_string()), ("order", q(order))];
                let _ = write!(
                    out,
                    "{}\n    {}",
                    if i == 0 { "" } else { "," },
                    line(&parts)
                );
            }
            out.push_str("\n  ]");
        }
        out.push_str("\n}\n");
        out
    }

    /// Whether the table holds rows not yet written: a draft (15 §2.7).
    #[must_use]
    pub fn is_draft(&self) -> bool {
        self.staged.as_ref().is_some_and(|s| !s.rows.is_empty())
    }
}

/// One object on one line, its keys in the order given.
fn line(parts: &[(&str, String)]) -> String {
    let body: Vec<String> = parts.iter().map(|(k, v)| format!("\"{k}\": {v}")).collect();
    format!("{{ {} }}", body.join(", "))
}

/// A JSON value over several lines, two spaces a level.
fn pretty(v: &Value, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth + 1);
    let end = "  ".repeat(depth);
    match v {
        Value::Object(o) if !o.is_empty() => {
            out.push('{');
            for (i, (k, v)) in o.iter().enumerate() {
                let _ = write!(
                    out,
                    "{}\n{pad}{}: ",
                    if i == 0 { "" } else { "," },
                    Value::String(k.clone())
                );
                pretty(v, depth + 1, out);
            }
            let _ = write!(out, "\n{end}}}");
        }
        Value::Array(a) if !a.is_empty() => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                let _ = write!(out, "{}\n{pad}", if i == 0 { "" } else { "," });
                pretty(v, depth + 1, out);
            }
            let _ = write!(out, "\n{end}]");
        }
        _ => out.push_str(&v.to_string()),
    }
}

/// The default reference: Wikibase's JSON without its hash, each snak of its property's
/// group, a `value` snak holding a `datavalue` or the position of a column (0106 §9).
#[allow(clippy::many_single_char_names)]
fn reference(r: &mut Reader, v: &Value, width: usize) -> Option<Value> {
    let path = "default_reference";
    let o = r.object(v, path, &["snaks", "snaks-order", "hash"])?;
    let Some(snaks) = o.get("snaks").and_then(Value::as_object) else {
        r.bad(path, "needs \"snaks\", an object of lists by property");
        return None;
    };
    let mut out = Map::new();
    let mut groups = Map::new();
    for (p, list) in snaks {
        let at = format!("{path}.snaks.{p}");
        r.property(p, &at)?;
        let Some(list) = list.as_array() else {
            r.bad(&at, "must be a list of snaks");
            continue;
        };
        for (i, s) in list.iter().enumerate() {
            let at = format!("{at}[{i}]");
            let Some(s) = r.object(
                s,
                &at,
                &[
                    "snaktype",
                    "property",
                    "datavalue",
                    "datatype",
                    "column",
                    "hash",
                ],
            ) else {
                continue;
            };
            if s.get("property").and_then(Value::as_str) != Some(p.as_str()) {
                r.bad(&at, format!("\"property\" must be {p}"));
            }
            match s.get("snaktype").and_then(Value::as_str) {
                Some("value") => match (s.get("datavalue"), s.get("column")) {
                    (Some(_), None) => {}
                    (None, Some(c)) => {
                        if let Some(c) = r.index(Some(c), &format!("{at}.column"))
                            && c >= width
                        {
                            r.bad(&format!("{at}.column"), "is not a column of the table");
                        }
                    }
                    _ => r.bad(&at, "needs \"datavalue\" or \"column\", and not both"),
                },
                Some("somevalue" | "novalue") => {}
                _ => r.bad(
                    &format!("{at}.snaktype"),
                    "must be \"value\", \"somevalue\" or \"novalue\"",
                ),
            }
        }
        let cleaned: Vec<Value> = list
            .iter()
            .map(|s| {
                let mut s = s.clone();
                if let Some(o) = s.as_object_mut() {
                    o.remove("hash");
                }
                s
            })
            .collect();
        groups.insert(p.clone(), Value::Array(cleaned));
    }
    if groups.is_empty() {
        r.bad(path, "holds no snaks");
    }
    out.insert("snaks".into(), Value::Object(groups));
    if let Some(order) = o.get("snaks-order") {
        let ok = order.as_array().is_some_and(|a| {
            a.iter()
                .all(|p| p.as_str().is_some_and(|p| snaks.contains_key(p)))
        });
        if ok {
            out.insert("snaks-order".into(), order.clone());
        } else {
            r.bad(
                &format!("{path}.snaks-order"),
                "must list the properties of \"snaks\"",
            );
        }
    }
    Some(Value::Object(out))
}

/// The staged rows and what they share.
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
fn staged(r: &mut Reader, v: &Value, width: usize) -> Option<Staged> {
    let o = r.object(v, "staged", &["shared", "properties", "rows"])?;
    let mut out = Staged::default();
    match o.get("shared") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            for (i, l) in list.iter().enumerate() {
                match l.as_str().map(str::trim) {
                    Some(l) if l.contains(':') => out.shared.push(l.to_string()),
                    _ => r.bad(
                        &format!("staged.shared[{i}]"),
                        "must be a property, a colon and a value",
                    ),
                }
            }
        }
        Some(_) => r.bad("staged.shared", "must be a list"),
    }
    match o.get("properties") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            for (i, p) in list.iter().enumerate() {
                let path = format!("staged.properties[{i}]");
                let Some(p) = r.object(p, &path, &["label", "datatype"]) else {
                    continue;
                };
                let Some(label) = r.string(p, &path, "label") else {
                    continue;
                };
                let datatype = match p.get("datatype") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(d)) if !d.trim().is_empty() => Some(d.trim().to_string()),
                    Some(_) => {
                        r.bad(&format!("{path}.datatype"), "must be a data type's ID");
                        None
                    }
                };
                out.properties.push(NewProperty { label, datatype });
            }
        }
        Some(_) => r.bad("staged.properties", "must be a list"),
    }
    let mut numbers = BTreeSet::new();
    match o.get("rows") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            for (i, row) in list.iter().enumerate() {
                let path = format!("staged.rows[{i}]");
                let Some(row) = r.object(
                    row,
                    &path,
                    &["n", "cells", "id", "skip", "create", "refused"],
                ) else {
                    continue;
                };
                let n = row.get("n").and_then(Value::as_u64).filter(|n| *n > 0);
                let Some(n) = n else {
                    r.bad(&format!("{path}.n"), "must be the row's number, from 1");
                    continue;
                };
                if !numbers.insert(n) {
                    r.bad(&format!("{path}.n"), format!("row {n} is staged already"));
                    continue;
                }
                let cells: Option<Vec<String>> = row
                    .get("cells")
                    .and_then(Value::as_array)
                    .and_then(|a| a.iter().map(|c| c.as_str().map(str::to_string)).collect());
                let Some(cells) = cells.filter(|c| c.len() == width) else {
                    r.bad(
                        &format!("{path}.cells"),
                        "must be one string for each column",
                    );
                    continue;
                };
                let id = match row.get("id") {
                    None | Some(Value::Null) => None,
                    // A value that is not a string is no ID either.
                    Some(v) => r.id(v.as_str().unwrap_or_default(), &format!("{path}.id")),
                };
                let mut flag = |key: &str| match row.get(key) {
                    None | Some(Value::Null) => false,
                    Some(Value::Bool(b)) => *b,
                    Some(_) => {
                        r.bad(&format!("{path}.{key}"), "must be true or false");
                        false
                    }
                };
                let skip = flag("skip");
                let create = flag("create");
                let refused = match row.get("refused") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => Some(s.clone()),
                    Some(_) => {
                        r.bad(&format!("{path}.refused"), "must be a string");
                        None
                    }
                };
                out.rows.push(StagedRow {
                    n,
                    cells,
                    id,
                    skip,
                    create,
                    refused,
                });
            }
        }
        Some(_) => r.bad("staged.rows", "must be a list"),
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example of 15 §2.3.
    const EXAMPLE: &str = r#"{
  "version": 1,
  "description": "Journals cited in the 2026 citation sample",
  "rows": { "ids": ["Q5", "WDQ42", "LBQ6"] },
  "columns": [
    { "field": "label", "language": "en" },
    { "field": "statements", "property": "P50" },
    { "field": "statements", "property": "WDP577", "title": "Published" },
    { "field": "sitelink", "site": "en.wikipedia.org" }
  ],
  "default_reference": {
    "snaks": {
      "P248": [ { "snaktype": "value", "property": "P248",
                  "datavalue": { "type": "wikibase-entityid", "value": { "id": "Q1234" } } } ]
    }
  },
  "sort": [ { "column": 2, "order": "descending" } ]
}"#;

    // 0045 §4: the definition is read, and written with one row ID per line.
    #[test]
    fn the_example_is_read_and_written_canonically() {
        let d = Definition::parse(EXAMPLE, Limits::default()).expect("valid");
        assert_eq!(d.rows.len(), 3);
        assert_eq!(d.columns.len(), 4);
        assert_eq!(d.columns[2].title.as_deref(), Some("Published"));
        assert_eq!(
            d.sort,
            vec![Sort {
                column: 2,
                order: Order::Descending
            }]
        );
        assert!(!d.is_draft());
        let text = d.canonical();
        assert!(
            text.contains("    \"ids\": [\n      \"Q5\",\n      \"WDQ42\",\n      \"LBQ6\"\n    ]"),
            "{text}"
        );
        assert!(
            text.contains(
                "    { \"field\": \"statements\", \"property\": \"WDP577\", \"title\": \"Published\" },\n"
            ),
            "{text}"
        );
        // 0045 §3: the canonical text is a fixed point of the pre-save transform.
        let again = Definition::parse(&text, Limits::default()).expect("canonical is valid");
        assert_eq!(again, d);
        assert_eq!(again.canonical(), text);
    }

    // 0045 §4: what validation refuses.
    #[test]
    fn problems_are_named_with_their_place() {
        let problems = |text: &str| -> Vec<String> {
            Definition::parse(
                text,
                Limits {
                    max_rows: 2,
                    max_columns: 2,
                },
            )
            .err()
            .unwrap_or_default()
            .iter()
            .map(ToString::to_string)
            .collect()
        };
        let has = |list: &[String], part: &str| list.iter().any(|p| p.contains(part));
        let p = problems("not json");
        assert!(has(&p, "not JSON"), "{p:?}");
        let p = problems(
            r#"{"version": 2, "rows": {"ids": ["Q1", "5", "qqq1", "Q2", "Q3"]},
                "columns": [{"field": "label"}, {"field": "statements", "property": "Q5"},
                            {"field": "colour"}, {"field": "sitelink", "site": "x", "extra": 1}],
                "sort": [{"column": 9, "order": "descending"}], "surprise": true}"#,
        );
        for part in [
            "version: must be 1",
            "unknown key \"surprise\"",
            "rows.ids[1]: \"5\" is not an entity ID",
            "rows.ids[2]: Q1 is a row already",
            "columns: 4 columns; a table has at most 2",
            "columns[0]: needs \"language\"",
            "columns[1].property: \"Q5\" is not a property ID",
            "columns[2]: unknown field \"colour\"",
            "columns[3]: unknown key \"extra\"",
            "rows: 3 rows; a table has at most 2",
            "sort[0].column: is not a column of the table",
        ] {
            assert!(has(&p, part), "{part} not in {p:#?}");
        }
        let p = problems(r#"{"version": 1, "rows": {"scope": "Scope:X"}, "columns": []}"#);
        assert!(has(&p, "not supported yet"), "{p:?}");
        assert!(has(&p, "one or more columns"), "{p:?}");
        // What only the registries know is the caller's to check.
        let known = |id: &EntityId| {
            (id.form() != scatter_wikibase_model::IdForm::Foreign).then(|| id.clone())
        };
        let checked = Definition::parse_with(
            r#"{"version": 1, "rows": {"ids": ["XYQ1"]}, "columns": [{"field": "label", "language": "en"}]}"#,
            Limits::default(),
            &known,
        );
        assert_eq!(checked.unwrap_err()[0].path, "rows.ids[0]");
    }

    // 0106 §9: a draft's staged rows, declared properties, reference columns and a default
    // reference that reads a column; one staged row per line.
    #[test]
    fn a_draft_holds_staged_rows() {
        let text = r#"{
  "version": 1,
  "rows": { "ids": ["Q123"] },
  "columns": [
    { "field": "label", "language": "en" },
    { "field": "description", "language": "en" },
    { "field": "statements", "new": 0 },
    { "field": "statements", "property": "P577" },
    { "field": "reference", "property": "P854", "of": 3 },
    { "field": "reference", "property": "P854", "title": "source url" }
  ],
  "default_reference": { "snaks": { "P854": [ { "snaktype": "value", "property": "P854", "column": 5, "hash": "x" } ] } },
  "staged": {
    "shared": [ "P31:Q13442814" ],
    "properties": [ { "label": "Publisher", "datatype": "wikibase-item" } ],
    "rows": [
      { "n": 1, "cells": [ "Smith family papers", "", "row:2", "", "", "" ], "create": true },
      { "n": 2, "cells": [ "Smith & Co.", "publisher", "", "", "", "" ], "id": "Q123" },
      { "n": 3, "cells": [ "", "", "", "", "", "" ], "skip": true, "refused": "no" }
    ]
  }
}"#;
        let d = Definition::parse(text, Limits::default()).expect("valid");
        assert!(d.is_draft());
        let staged = d.staged.as_ref().unwrap();
        assert_eq!(staged.rows.len(), 3);
        assert_eq!(
            staged.rows[1].id.as_ref().map(EntityId::as_str),
            Some("Q123")
        );
        assert!(staged.rows[2].skip);
        assert!(staged.rows[0].create);
        assert_eq!(
            d.columns[5].field,
            Field::Reference {
                property: EntityId::parse("P854").unwrap(),
                of: None
            }
        );
        assert_eq!(d.columns[2].field, Field::Statements(Property::New(0)));
        let canonical = d.canonical();
        assert!(
            canonical.contains(
                "      { \"n\": 2, \"cells\": [\"Smith & Co.\", \"publisher\", \"\", \"\", \"\", \"\"], \"id\": \"Q123\" },\n"
            ),
            "{canonical}"
        );
        assert!(
            !canonical.contains("hash"),
            "the hash is dropped: {canonical}"
        );
        assert_eq!(Definition::parse(&canonical, Limits::default()).unwrap(), d);

        let bad = text
            .replace("\"new\": 0", "\"new\": 3")
            .replace("\"of\": 3", "\"of\": 0")
            .replace("\"column\": 5", "\"column\": 9")
            .replace(
                "{ \"n\": 2, \"cells\": [ \"Smith & Co.\"",
                "{ \"n\": 1, \"cells\": [ \"Smith & Co.\"",
            )
            .replace("[ \"\", \"\", \"\", \"\", \"\", \"\" ]", "[ \"\" ]");
        let problems: Vec<String> = Definition::parse(&bad, Limits::default())
            .unwrap_err()
            .iter()
            .map(ToString::to_string)
            .collect();
        for part in [
            "columns[2].new: must be the position of a property in staged.properties",
            "columns[4].of: must be the position of a statements column",
            "column: is not a column of the table",
            "staged.rows[1].n: row 1 is staged already",
            "staged.rows[2].cells: must be one string for each column",
        ] {
            assert!(
                problems.iter().any(|p| p.contains(part)),
                "{part} not in {problems:#?}"
            );
        }
        // Staged rows need listed rows to join.
        let scoped = text.replace("{ \"ids\": [\"Q123\"] }", "{ \"scope\": \"Scope:X\" }");
        assert!(
            Definition::parse(&scoped, Limits::default())
                .unwrap_err()
                .iter()
                .any(|p| p.path == "staged")
        );
    }
}
