//! The table's text (0106 §1, §2; 21 §6.6): the canonical form of what
//! `Special:CreateManyEntities` imports. This module reads the text and writes it back, so that
//! the preview can fill in what it found (a matched row's `id`, a proposed data type) and
//! anything that edits a table, the grid included, passes through the same form.
//!
//! - The table: tab- or comma-separated, the first row naming the columns ([`Column`]).
//! - A cell of several values: `|`-separated, `\|` for a literal bar and `\\` for a
//!   literal backslash ([`split_values`]).
//! - The `id` cell ([`IdCell`]): an item ID to add to, empty to let the preview decide,
//!   `new` to create whatever matches, `skip` to leave the row out.
//! - The statements on every item, and the reference on every statement, one
//!   `P31:Q5` per line; a reference line's value may be a column's header in braces
//!   ([`Line`]).

use serde::{Deserialize, Serialize};

use crate::i18n;
use crate::special::values::implied_type;

/// What a column holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Column {
    /// The existing item the row adds to, or how the row is to be treated ([`IdCell`]).
    Id,
    /// A label in a language.
    Label(String),
    /// A description in a language.
    Description(String),
    /// Aliases in a language, `|`-separated.
    Aliases(String),
    /// A property's statements.
    Property(String),
    /// The statements of a property that does not exist yet (0106 §4): its label in the
    /// wiki's language and, once chosen, its data type.
    New {
        /// The label.
        label: String,
        /// The data type, where the header gives one.
        datatype: Option<String>,
    },
    /// A value of the reference of the row's statements of one property (0106 §8).
    Reference {
        /// The property whose statements carry the reference.
        of: String,
        /// The reference snak's property.
        property: String,
    },
    /// Any other header: ignored unless the reference on every statement reads it.
    Plain(String),
}

/// Whether `id` is a property ID.
fn is_property(id: &str) -> bool {
    implied_type(id).as_deref() == Some("property")
}

/// Reads a header cell.
#[must_use]
pub fn column(header: &str, default_lang: &str) -> Column {
    let h = header.trim();
    let lower = h.to_ascii_lowercase();
    if lower == "id" {
        return Column::Id;
    }
    if let Some(rest) = lower.strip_prefix("new:") {
        // The label keeps its case; only the prefix is case-blind.
        let rest = &h[h.len() - rest.len()..];
        let (label, datatype) = match rest.rsplit_once('@') {
            Some((l, d)) if !d.trim().is_empty() && !d.contains(char::is_whitespace) => {
                (l.trim(), Some(d.trim().to_string()))
            }
            _ => (rest.trim(), None),
        };
        if !label.is_empty() {
            return Column::New {
                label: label.to_string(),
                datatype,
            };
        }
        return Column::Plain(h.to_string());
    }
    if let Some((of, property)) = h.split_once("/ref:") {
        let (of, property) = (of.trim(), property.trim());
        if is_property(of) && is_property(property) {
            return Column::Reference {
                of: of.to_ascii_uppercase(),
                property: property.to_ascii_uppercase(),
            };
        }
        return Column::Plain(h.to_string());
    }
    let (name, lang) = match lower.split_once(':') {
        Some((n, l)) => (n.trim().to_string(), l.trim().to_string()),
        None => (lower.clone(), String::new()),
    };
    let lang = if lang.is_empty() {
        Some(default_lang.to_string())
    } else {
        i18n::valid_code(&lang)
    };
    match (name.as_str(), lang) {
        ("label" | "labels", Some(l)) => Column::Label(l),
        ("description" | "descriptions", Some(l)) => Column::Description(l),
        ("alias" | "aliases", Some(l)) => Column::Aliases(l),
        _ if is_property(h) => Column::Property(h.to_ascii_uppercase()),
        _ => Column::Plain(h.to_string()),
    }
}

/// Writes a column as its header: what [`column()`] reads back as the same column.
#[must_use]
pub fn header(column: &Column, default_lang: &str) -> String {
    let term = |name: &str, lang: &str| {
        if lang == default_lang {
            name.to_string()
        } else {
            format!("{name}:{lang}")
        }
    };
    match column {
        Column::Id => "id".into(),
        Column::Label(l) => term("label", l),
        Column::Description(l) => term("description", l),
        Column::Aliases(l) => term("aliases", l),
        Column::Property(p) => p.clone(),
        Column::New {
            label,
            datatype: Some(d),
        } => format!("new:{label}@{d}"),
        Column::New {
            label,
            datatype: None,
        } => format!("new:{label}"),
        Column::Reference { of, property } => format!("{of}/ref:{property}"),
        Column::Plain(h) => h.clone(),
    }
}

/// Splits a table: tab-separated when the first line holds a tab, else comma-separated
/// with double-quoted cells as a spreadsheet writes them. Empty lines are skipped.
#[must_use]
pub fn rows_of(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if first.contains('\t') {
        return text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split('\t').map(|c| c.trim().to_string()).collect())
            .collect();
    }
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) => {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            }
            ('"', false) if cell.is_empty() => quoted = true,
            (',', false) => row.push(std::mem::take(&mut cell)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut cell));
                if row.iter().any(|c| !c.trim().is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            _ => cell.push(c),
        }
    }
    row.push(cell);
    if row.iter().any(|c| !c.trim().is_empty()) {
        rows.push(row);
    }
    for r in &mut rows {
        for c in r.iter_mut() {
            *c = c.trim().to_string();
        }
    }
    rows
}

/// Whether `text` is read as tab-separated ([`rows_of`]).
#[must_use]
pub fn is_tabbed(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.lines()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|l| l.contains('\t'))
}

/// Writes a table as text that [`rows_of`] reads back as the same rows: tab-separated
/// when `tabbed` and the cells allow it (more than one column, and no tab, line break or
/// empty row), else comma-separated with the cells that need it quoted.
#[must_use]
pub fn text_of(rows: &[Vec<String>], tabbed: bool) -> String {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let plain = |c: &String| !c.contains(['\t', '\n', '\r']);
    let tabs = tabbed && width > 1 && rows.iter().all(|r| r.len() > 1 && r.iter().all(plain));
    let mut out = String::new();
    for r in rows {
        let cells: Vec<String> = r
            .iter()
            .map(|c| {
                if tabs {
                    c.clone()
                } else if c.contains([',', '"', '\n', '\r']) || c.starts_with(' ') {
                    format!("\"{}\"", c.replace('"', "\"\""))
                } else {
                    c.clone()
                }
            })
            .collect();
        out.push_str(&cells.join(if tabs { "\t" } else { "," }));
        out.push('\n');
    }
    out
}

/// The values of a cell that may hold several: `|`-separated, `\|` a literal bar and
/// `\\` a literal backslash. Empty values are dropped.
#[must_use]
pub fn split_values(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = cell.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // An escaped bar or backslash is itself; any other backslash is as written.
            '\\' if matches!(chars.peek(), Some('|' | '\\')) => {
                cur.extend(chars.next());
            }
            '|' => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

/// Writes several values as one cell, which [`split_values`] reads back.
#[must_use]
pub fn join_values(values: &[String]) -> String {
    values
        .iter()
        .map(|v| v.replace('\\', "\\\\").replace('|', "\\|"))
        .collect::<Vec<_>>()
        .join("|")
}

/// What a row's `id` cell says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdCell {
    /// Empty: the preview looks for an existing item the row matches.
    Auto,
    /// The existing item the row adds to.
    Item(String),
    /// `new`: create an item whatever the row matches.
    New,
    /// `skip`: leave the row out.
    Skip,
    /// Anything else, which is an error.
    Bad(String),
}

/// Reads an `id` cell.
#[must_use]
pub fn id_cell(cell: &str) -> IdCell {
    let c = cell.trim();
    match c.to_ascii_lowercase().as_str() {
        "" => IdCell::Auto,
        "new" => IdCell::New,
        "skip" => IdCell::Skip,
        _ if implied_type(c).as_deref() == Some("item") => IdCell::Item(c.to_ascii_uppercase()),
        _ => IdCell::Bad(c.to_string()),
    }
}

/// The row a cell value names with `row:{n}`, counting from 1 below the header.
#[must_use]
pub fn row_ref(value: &str) -> Option<usize> {
    let v = value.trim();
    let n = v
        .strip_prefix("row:")
        .or_else(|| v.strip_prefix("ROW:"))
        .or_else(|| v.strip_prefix("Row:"))?;
    n.trim().parse().ok().filter(|n| *n > 0)
}

/// One `P31:Q5` line of the statements on every item or of the reference on every
/// statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// As written.
    pub raw: String,
    /// The property.
    pub property: String,
    /// The value as written; for a reference line that reads a column, see
    /// [`Line::column`].
    pub value: String,
}

impl Line {
    /// The header of the column this line's value is read from, when it is written in
    /// braces (`P854:{source url}`).
    #[must_use]
    pub fn column(&self) -> Option<&str> {
        self.value
            .strip_prefix('{')
            .and_then(|v| v.strip_suffix('}'))
            .map(str::trim)
            .filter(|h| !h.is_empty())
    }
}

/// Reads the lines, one statement or snak each; a line that is not one is returned as
/// written.
#[must_use]
pub fn lines_of(text: &str) -> Vec<Result<Line, String>> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|raw| {
            raw.split_once(':')
                .map(|(p, v)| (p.trim().to_ascii_uppercase(), v.trim().to_string()))
                .filter(|(p, v)| is_property(p) && !v.is_empty())
                .map(|(property, value)| Line {
                    raw: raw.to_string(),
                    property,
                    value,
                })
                .ok_or_else(|| raw.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_split_as_tabs_or_commas() {
        let tsv = "label\tP31\nSix\tQ5\n\nSeven\tQ6\n";
        assert_eq!(
            rows_of(tsv),
            vec![vec!["label", "P31"], vec!["Six", "Q5"], vec!["Seven", "Q6"]]
        );
        let csv = "\u{feff}label,description:de\r\n\"Six, the number\",\"eine \"\"Zahl\"\"\"\r\nSeven,\r\n";
        assert_eq!(
            rows_of(csv),
            vec![
                vec!["label", "description:de"],
                vec!["Six, the number", "eine \"Zahl\""],
                vec!["Seven", ""]
            ]
        );
    }

    // 0106 §1: anything the table holds is written as text that reads back the same.
    #[test]
    fn the_text_round_trips() {
        let rows: Vec<Vec<String>> = [
            vec![
                "id",
                "label",
                "new:Publisher@wikibase-item",
                "P577/ref:P854",
            ],
            vec![
                "",
                "Six, the \"number\"",
                "row:2",
                "https://example.org/a|b",
            ],
            vec!["Q5", "!Seven", "", ""],
            vec!["skip", "two\nlines", "a\\|b|c", ""],
        ]
        .iter()
        .map(|r| r.iter().map(ToString::to_string).collect())
        .collect();
        // A line break in a cell forces commas even when tabs were asked for.
        assert_eq!(rows_of(&text_of(&rows, false)), rows);
        assert_eq!(rows_of(&text_of(&rows, true)), rows);
        let simple = &rows[..3];
        let tabbed = text_of(simple, true);
        assert!(tabbed.contains('\t'), "{tabbed}");
        assert_eq!(rows_of(&tabbed), simple);
        // One column cannot be told from commas by a tab, so it is written with commas.
        let one = vec![vec!["label".to_string()], vec!["a, b".to_string()]];
        assert_eq!(rows_of(&text_of(&one, true)), one);
    }

    // 0106 §2: the headers.
    #[test]
    fn headers_name_terms_properties_and_the_rest() {
        assert_eq!(column("label", "en"), Column::Label("en".into()));
        assert_eq!(
            column("Description:de", "en"),
            Column::Description("de".into())
        );
        assert_eq!(column("aliases:fr", "en"), Column::Aliases("fr".into()));
        assert_eq!(column("p31", "en"), Column::Property("P31".into()));
        assert_eq!(column("ID", "en"), Column::Id);
        assert_eq!(
            column("new:Publisher", "en"),
            Column::New {
                label: "Publisher".into(),
                datatype: None
            }
        );
        assert_eq!(
            column("New:Date of first issue@time", "en"),
            Column::New {
                label: "Date of first issue".into(),
                datatype: Some("time".into())
            }
        );
        assert_eq!(
            column("p577/ref:p854", "en"),
            Column::Reference {
                of: "P577".into(),
                property: "P854".into()
            }
        );
        assert_eq!(
            column("label:not a language", "en"),
            Column::Plain("label:not a language".into())
        );
        assert_eq!(
            column("source url", "en"),
            Column::Plain("source url".into())
        );
        for h in [
            "id",
            "label",
            "description:de",
            "aliases:fr",
            "P31",
            "new:Publisher",
            "new:Date of first issue@time",
            "P577/ref:P854",
            "source url",
        ] {
            assert_eq!(header(&column(h, "en"), "en"), h);
        }
    }

    #[test]
    fn cells_hold_several_values() {
        assert_eq!(split_values("a| b |"), vec!["a", "b"]);
        assert_eq!(split_values("a\\|b|c"), vec!["a|b", "c"]);
        assert_eq!(split_values(""), Vec::<String>::new());
        let values = vec!["a|b".to_string(), "c".to_string()];
        assert_eq!(split_values(&join_values(&values)), values);
        // 0106 A8: a backslash is escaped too, so one before a bar is not read as its escape.
        let values = vec![
            "ends\\".to_string(),
            "C:\\dir".to_string(),
            "x\\|y".to_string(),
        ];
        assert_eq!(join_values(&values), "ends\\\\|C:\\\\dir|x\\\\\\|y");
        assert_eq!(split_values(&join_values(&values)), values);
        // Written by hand, a backslash before anything else stands for itself.
        assert_eq!(split_values("C:\\dir|\\alpha"), vec!["C:\\dir", "\\alpha"]);
    }

    #[test]
    fn id_cells_and_row_references() {
        assert_eq!(id_cell(""), IdCell::Auto);
        assert_eq!(id_cell("q12"), IdCell::Item("Q12".into()));
        assert_eq!(id_cell("New"), IdCell::New);
        assert_eq!(id_cell("skip"), IdCell::Skip);
        assert_eq!(id_cell("P12"), IdCell::Bad("P12".into()));
        assert_eq!(row_ref("row:14"), Some(14));
        assert_eq!(row_ref("row:0"), None);
        assert_eq!(row_ref("Q5"), None);
    }

    #[test]
    fn lines_name_a_property_and_a_value_or_a_column() {
        let lines = lines_of("p248:Q1234\n\nP854:{ source url }\nnonsense\n");
        assert_eq!(lines.len(), 3);
        let first = lines[0].as_ref().unwrap();
        assert_eq!(
            (first.property.as_str(), first.value.as_str()),
            ("P248", "Q1234")
        );
        assert_eq!(first.column(), None);
        assert_eq!(lines[1].as_ref().unwrap().column(), Some("source url"));
        assert_eq!(lines[2], Err("nonsense".to_string()));
    }
}
