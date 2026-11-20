//! `Special:QuickStatements` (0106 A9; 21 §6.9): QuickStatements V1 commands read as the
//! table of `Special:CreateManyEntities`, which checks and writes them. The page is a
//! reader in front of that one and writes nothing itself: `CREATE` starts a row that
//! creates an item, a command on an item's ID is a row that adds to it, and the commands
//! on `LAST` or on the same item fill the row's terms, statements and references. What
//! the table cannot hold is refused, line by line, before anything is opened: a removal,
//! a sitelink, a merge, a target that is not an item. Qualifiers are left out, and the
//! page says how many.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use super::table::text_of;
use super::{HandoffPage, Typed, error, handoff, open, viewer};
use triplespace_ui_kit::Site;
use triplespace_ui_kit::codex::{Action, Button, Message, MessageKind, Weight};
use triplespace_ui_kit::controls::{ControlModel, FieldRow};
use triplespace_ui_kit::pages::{self, Params, Peer};
use triplespace_ui_kit::values::join_values;

/// The page's title.
pub const TITLE: &str = "Special:QuickStatements";

/// Why a line cannot be read into the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// `LAST` before any `CREATE`.
    NoLast,
    /// A removal (`-Q1|P2|…`).
    Removal,
    /// A sitelink (`Senwiki`).
    Sitelink,
    /// `MERGE`.
    Merge,
    /// A target that is neither `LAST` nor an item's ID.
    Target(String),
    /// Too few fields, or a field that is not one of the command's.
    Fields,
}

/// What the commands read as.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Read {
    /// The table, its header row first; empty when no command was read.
    pub rows: Vec<Vec<String>>,
    /// The lines that cannot be read, by number from 1, each with the line as written.
    pub problems: Vec<(usize, String, Why)>,
    /// How many qualifiers were left out.
    pub qualifiers: usize,
}

/// The fields of a line: tab-separated when it holds a tab, else `|`-separated, where a
/// bar inside double quotes is not a separator.
fn fields(line: &str) -> Vec<String> {
    let mut out: Vec<String> = if line.contains('\t') {
        line.split('\t').map(|f| f.trim().to_string()).collect()
    } else {
        let mut out = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        for c in line.chars() {
            if c == '|' && !quoted {
                out.push(std::mem::take(&mut field));
                continue;
            }
            if c == '"' {
                quoted = !quoted;
            }
            field.push(c);
        }
        out.push(field);
        out.iter().map(|f| f.trim().to_string()).collect()
    };
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

/// A line without its `/* … */` comments.
fn uncommented(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        match rest[at..].find("*/") {
            Some(end) => rest = &rest[at + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `P12`, in either case.
fn property(s: &str) -> Option<String> {
    let rest = s.strip_prefix(['P', 'p'])?;
    digits(rest).then(|| format!("P{rest}"))
}

/// An item's ID, with a source's prefix where it has one (`Q5`, `WDQ5`).
fn item(s: &str) -> Option<String> {
    let at = s.find(['Q', 'q'])?;
    let (prefix, rest) = (&s[..at], &s[at + 1..]);
    (prefix.len() <= 3 && prefix.bytes().all(|b| b.is_ascii_alphabetic()) && digits(rest))
        .then(|| s.to_ascii_uppercase())
}

/// A term command's kind and language: `Len`, `Dde`, `Apt-br`.
fn term(s: &str) -> Option<(char, &str)> {
    let kind = s.chars().next().filter(|c| "LDA".contains(*c))?;
    let lang = &s[1..];
    let mut parts = lang.split('-');
    let first = parts.next()?;
    let ok = first.len() >= 2
        && first.bytes().all(|b| b.is_ascii_lowercase())
        && parts.all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    ok.then_some((kind, lang))
}

/// A string without its double quotes.
fn unquote(raw: &str) -> &str {
    raw.strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(raw)
        .trim()
}

/// A time, `+1967-01-17T00:00:00Z/11`, to the day, month or year it is precise to.
fn time(raw: &str) -> Option<String> {
    let (stamp, precision) = raw.split_once('/')?;
    let sign = stamp.chars().next().filter(|c| "+-".contains(*c))?;
    let (date, clock) = stamp[1..].split_once('T')?;
    if !clock.ends_with('Z') {
        return None;
    }
    let mut parts = date.splitn(3, '-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if !(digits(y) && m.len() == 2 && digits(m) && d.len() == 2 && digits(d)) {
        return None;
    }
    let year = format!("{}{y}", if sign == '-' { "-" } else { "" });
    match precision {
        "11" => Some(format!("{year}-{m}-{d}")),
        "10" => Some(format!("{year}-{m}")),
        "9" => Some(year),
        _ => None,
    }
}

fn number(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (whole, part) = s.split_once('.').unwrap_or((s, "0"));
    digits(whole) && digits(part)
}

/// A value as the table writes it (21 §6.6): IDs as they are, strings without quotes,
/// times to the day, month or year, quantities without a leading `+` and with their unit
/// as an item ID, monolingual texts as `text@lang`, coordinates as `lat, lon`. Anything
/// else is kept as written, for the check to refuse.
fn value(raw: &str) -> String {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        return unquote(raw).to_string();
    }
    if let Some(t) = time(raw) {
        return t;
    }
    if let Some((lang, text)) = raw.split_once(':')
        && text.len() >= 2
        && text.starts_with('"')
        && text.ends_with('"')
        && lang.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return format!("{}@{lang}", unquote(text));
    }
    if let Some((lat, lon)) = raw.strip_prefix('@').and_then(|r| r.split_once('/'))
        && number(lat.trim())
        && number(lon.trim())
    {
        return format!("{}, {}", lat.trim(), lon.trim());
    }
    let plain = raw
        .strip_prefix('+')
        .filter(|r| r.chars().next().is_some_and(|c| c.is_ascii_digit()));
    let amount = plain.unwrap_or(raw);
    if let Some((n, unit)) = amount.split_once('U')
        && number(n)
        && digits(unit)
    {
        return format!("{n} Q{unit}");
    }
    amount.to_string()
}

/// A row being read: the item it adds to, and its cells by column.
#[derive(Default)]
struct Row {
    id: Option<String>,
    cells: BTreeMap<usize, Vec<String>>,
}

/// Reads the commands as a table in the wiki's language `lang`, whose term columns are
/// written without a suffix.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn read(text: &str, lang: &str) -> Read {
    let mut out = Read::default();
    let mut headers: Vec<String> = Vec::new();
    let mut rows: Vec<Row> = Vec::new();
    // The row `LAST` names.
    let mut last: Option<usize> = None;
    let mut at = |header: String| {
        headers
            .iter()
            .position(|h| *h == header)
            .unwrap_or_else(|| {
                headers.push(header);
                headers.len() - 1
            })
    };
    for (n, full) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = uncommented(full);
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut refuse = |why: Why| out.problems.push((n + 1, full.trim().to_string(), why));
        let f = fields(line);
        let Some(first) = f.first() else { continue };
        if f.len() == 1 && first == "CREATE" {
            rows.push(Row::default());
            last = Some(rows.len() - 1);
            continue;
        }
        if first == "MERGE" {
            refuse(Why::Merge);
            continue;
        }
        if first.starts_with('-') {
            refuse(Why::Removal);
            continue;
        }
        let row = if first == "LAST" {
            let Some(r) = last else {
                refuse(Why::NoLast);
                continue;
            };
            r
        } else if let Some(id) = item(first) {
            rows.iter()
                .position(|r| r.id.as_deref() == Some(&id))
                .unwrap_or_else(|| {
                    rows.push(Row {
                        id: Some(id),
                        ..Row::default()
                    });
                    rows.len() - 1
                })
        } else {
            refuse(Why::Target(first.clone()));
            continue;
        };
        if f.len() < 3 {
            refuse(Why::Fields);
            continue;
        }
        if let Some((kind, language)) = term(&f[1]) {
            if f.len() != 3 {
                refuse(Why::Fields);
                continue;
            }
            let name = match kind {
                'L' => "label",
                'D' => "description",
                _ => "aliases",
            };
            let header = if language == lang {
                name.to_string()
            } else {
                format!("{name}:{language}")
            };
            let text = unquote(&f[2]).to_string();
            // On an item that exists, the command sets the term whatever it was.
            let mark = if rows[row].id.is_some() { "!" } else { "" };
            let cell = rows[row].cells.entry(at(header)).or_default();
            if kind == 'A' {
                cell.push(text);
            } else {
                *cell = vec![format!("{mark}{text}")];
            }
            continue;
        }
        let Some(p) = property(&f[1]) else {
            let sitelink = f[1].starts_with('S') && !digits(&f[1][1..]);
            refuse(if sitelink { Why::Sitelink } else { Why::Fields });
            continue;
        };
        if f.len().is_multiple_of(2) {
            refuse(Why::Fields);
            continue;
        }
        // The pairs after the value: a source's snak, or a qualifier, which is left out.
        let mut sources: Vec<(String, String)> = Vec::new();
        let mut qualifiers = 0;
        let mut ok = true;
        for pair in f[3..].chunks(2) {
            let source = pair[0]
                .strip_prefix(['S', 's'])
                .filter(|d| digits(d))
                .map(|d| format!("{p}/ref:P{d}"));
            match source {
                Some(header) => sources.push((header, value(&pair[1]))),
                None if property(&pair[0]).is_some() => qualifiers += 1,
                None => ok = false,
            }
        }
        if !ok {
            refuse(Why::Fields);
            continue;
        }
        out.qualifiers += qualifiers;
        rows[row].cells.entry(at(p)).or_default().push(value(&f[2]));
        for (header, v) in sources {
            rows[row].cells.entry(at(header)).or_default().push(v);
        }
    }
    let rows: Vec<Row> = rows.into_iter().filter(|r| !r.cells.is_empty()).collect();
    if rows.is_empty() {
        return out;
    }
    let ids = rows.iter().any(|r| r.id.is_some());
    let mut head: Vec<String> = Vec::new();
    if ids {
        head.push("id".into());
    }
    head.extend(headers.iter().cloned());
    out.rows.push(head);
    for r in rows {
        let mut cells: Vec<String> = Vec::new();
        if ids {
            cells.push(r.id.clone().unwrap_or_default());
        }
        for i in 0..headers.len() {
            cells.push(r.cells.get(&i).map(|v| join_values(v)).unwrap_or_default());
        }
        out.rows.push(cells);
    }
    out
}

/// Whether a file's text is QuickStatements commands rather than a table: its first
/// line that says anything is `CREATE`, or a command on `LAST` or on an item.
#[must_use]
pub fn looks_like(text: &str) -> bool {
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(|l| uncommented(l).trim().to_string())
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .is_some_and(|l| {
            let f = fields(&l);
            match f.first().map(String::as_str) {
                Some("CREATE") => f.len() == 1,
                Some("LAST") => f.len() >= 3,
                Some(first) => {
                    f.len() >= 3
                        && item(first.trim_start_matches('-')).is_some()
                        && (term(&f[1]).is_some() || property(&f[1]).is_some())
                }
                None => false,
            }
        })
}

/// The page: the commands in a box.
fn model(cx: &pages::Context, commands: &str) -> HandoffPage {
    let m = &cx.m;
    HandoffPage {
        gate: None,
        class: "ts-quickstatements".into(),
        intro: m.get("ts-quickstatements-intro"),
        notices: Vec::new(),
        action: format!("/wiki/{TITLE}"),
        multipart: false,
        hidden: Vec::new(),
        fields: vec![FieldRow {
            id: "ts-quickstatements-commands".into(),
            label: m.get("ts-quickstatements-commands"),
            class: None,
            control: ControlModel::TextArea {
                id: Some("ts-quickstatements-commands".into()),
                name: "commands".into(),
                value: commands.into(),
                rows: 12,
                no_spellcheck: true,
                described_by: Some("ts-quickstatements-commands-help".into()),
            },
            help: Some(m.get("ts-quickstatements-commands-help")),
            help_id: Some("ts-quickstatements-commands-help".into()),
        }],
        data_html: String::new(),
        submit: Button {
            label: &m.get("ts-quickstatements-read"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .model(),
        links: vec![
            (
                format!("/wiki/{}", super::TITLE),
                m.get("ts-createentities-title"),
            ),
            (
                format!("/wiki/{}", super::from_file::TITLE),
                m.get("ts-createfromfile-title"),
            ),
        ],
    }
}

/// `GET Special:QuickStatements`: the box.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let (cx, gate) = match viewer(site, headers, &incoming, &params.map, TITLE).await {
        Ok(found) => found,
        Err(r) => return r,
    };
    let mut page = model(&cx, "");
    page.gate = gate;
    handoff(
        &cx,
        headers,
        StatusCode::OK,
        TITLE,
        "ts-quickstatements-title",
        &page,
    )
}

/// The commands read, and what comes of them: the form of `Special:CreateManyEntities`
/// holding their table, or this page saying which lines cannot be read.
pub(crate) async fn answer(
    site: &Site,
    mut cx: pages::Context,
    headers: &HeaderMap,
    incoming: &triplespace_client::Incoming,
    commands: &str,
) -> Response {
    let found = read(commands, &cx.site.lang);
    let m = &cx.m;
    let mut notices = Vec::new();
    for (n, line, why) in &found.problems {
        let n = n.to_string();
        let why = match why {
            Why::NoLast => m.get("ts-quickstatements-no-last"),
            Why::Removal => m.get("ts-quickstatements-removal"),
            Why::Sitelink => m.get("ts-quickstatements-sitelink"),
            Why::Merge => m.get("ts-quickstatements-merge"),
            Why::Target(t) => m.with("ts-quickstatements-target", &[t]),
            Why::Fields => m.get("ts-quickstatements-fields"),
        };
        notices.push(error(&m.with("ts-quickstatements-line", &[&n, line, &why])));
    }
    if notices.is_empty() && found.rows.is_empty() {
        notices.push(error(&m.get("ts-quickstatements-empty")));
    }
    if !notices.is_empty() {
        let mut page = model(&cx, commands);
        page.notices = notices;
        return handoff(
            &cx,
            headers,
            StatusCode::OK,
            TITLE,
            "ts-quickstatements-title",
            &page,
        );
    }
    if found.qualifiers > 0 {
        notices.push(
            Message::block(
                MessageKind::Warning,
                &m.with(
                    "ts-quickstatements-qualifiers",
                    &[&found.qualifiers.to_string()],
                ),
            )
            .model(),
        );
    }
    let typed = Typed {
        rows: text_of(&found.rows, false),
        ..Typed::default()
    };
    open(site, &mut cx, headers, incoming, typed, notices).await
}

/// `POST Special:QuickStatements`: the commands read into the table.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let (cx, gate) = match viewer(site, headers, &incoming, query, TITLE).await {
        Ok(found) => found,
        Err(r) => return r,
    };
    let commands = params.map.get("commands").cloned().unwrap_or_default();
    if gate.is_some() {
        let mut page = model(&cx, &commands);
        page.gate = gate;
        return handoff(
            &cx,
            headers,
            StatusCode::OK,
            TITLE,
            "ts-quickstatements-title",
            &page,
        );
    }
    answer(site, cx, headers, &incoming, &commands).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> Vec<Vec<String>> {
        let found = read(text, "en");
        assert_eq!(found.problems, Vec::new());
        found.rows
    }

    // 0106 A9: CREATE and LAST lines are rows that create items.
    #[test]
    fn create_and_last_lines_become_rows() {
        let rows = table(
            "CREATE\nLAST|Len|\"Six\"\nLAST|Dde|\"eine Zahl\"\nLAST|Aen|\"6\"\nLAST|Aen|\"VI\"\n\
             LAST|P31|Q5|S854|\"https://example.org/a|b\"\nLAST|P31|Q6\n\
             CREATE\nLAST\tLen\t\"Seven\"\nLAST\tP577\t+1967-01-17T00:00:00Z/11\n",
        );
        assert_eq!(
            rows,
            vec![
                vec![
                    "label",
                    "description:de",
                    "aliases",
                    "P31",
                    "P31/ref:P854",
                    "P577"
                ],
                vec![
                    "Six",
                    "eine Zahl",
                    "6|VI",
                    "Q5|Q6",
                    "https://example.org/a\\|b",
                    ""
                ],
                vec!["Seven", "", "", "", "", "1967-01-17"],
            ]
        );
    }

    // 0106 A9: a command on an item is a row that adds to it, its terms set outright.
    #[test]
    fn commands_on_an_item_add_to_it() {
        let rows = table("Q42|P31|Q5\nq42|Len|\"Douglas\"\nQ7|P2|\"x\" /* why */\n# note\n");
        assert_eq!(
            rows,
            vec![
                vec!["id", "P31", "label", "P2"],
                vec!["Q42", "Q5", "!Douglas", ""],
                vec!["Q7", "", "", "x"],
            ]
        );
    }

    #[test]
    fn values_are_written_in_the_tables_forms() {
        assert_eq!(value("+1967-01-00T00:00:00Z/10"), "1967-01");
        assert_eq!(value("-0500-00-00T00:00:00Z/9"), "-0500");
        assert_eq!(value("en:\"some text\""), "some text@en");
        assert_eq!(value("@43.26193/10.92708"), "43.26193, 10.92708");
        assert_eq!(value("+12"), "12");
        assert_eq!(value("12U11573"), "12 Q11573");
        assert_eq!(value("Q5"), "Q5");
        assert_eq!(value("somevalue"), "somevalue");
    }

    // 0106 A9: what the table cannot hold is refused by line; qualifiers are counted.
    #[test]
    fn what_the_table_cannot_hold_is_refused() {
        let found = read(
            "LAST|P31|Q5\nCREATE\n-Q1|P31|Q5\nQ1|Senwiki|\"Six\"\nMERGE|Q1|Q2\nP5|Len|\"x\"\n\
             LAST|P31\nLAST|P31|Q5|P580|+2000-00-00T00:00:00Z/9\n",
            "en",
        );
        let whys: Vec<(usize, Why)> = found
            .problems
            .iter()
            .map(|(n, _, w)| (*n, w.clone()))
            .collect();
        assert_eq!(
            whys,
            vec![
                (1, Why::NoLast),
                (3, Why::Removal),
                (4, Why::Sitelink),
                (5, Why::Merge),
                (6, Why::Target("P5".into())),
                (7, Why::Fields),
            ]
        );
        assert_eq!(found.qualifiers, 1);
        assert_eq!(found.rows, vec![vec!["P31"], vec!["Q5"]]);
    }

    #[test]
    fn commands_are_told_from_a_table() {
        assert!(looks_like("# batch\nCREATE\nLAST|Len|\"x\"\n"));
        assert!(looks_like("Q1\tP31\tQ5\n"));
        assert!(looks_like("-Q1|P31|Q5\n"));
        assert!(!looks_like("label,P31\nSix,Q5\n"));
        assert!(!looks_like("id\tlabel\nQ1\tSix\n"));
        assert!(!looks_like(""));
    }
}
