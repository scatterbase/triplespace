//! Statement groups in their shapes (0003 §3–5, §9): the group card around the shape
//! `scatter-wikibase-shape` picks, its folds, and its footer.
//!
//! Everything here works without script (0057 §1):
//!
//! - Folds ("5 other values", "1 deprecated", "Show 18 more values") are `<details>`.
//! - The view switch (Series: chart or table; Table: table or Matrix) is a pair of radio
//!   buttons in a `<fieldset>`; the stylesheet hides the panel not chosen with `:has()`.
//!   A browser without `:has()` shows both panels, one under the other.
//! - Footnote markers are links to the footnotes at the foot of the group.
//!
//! Where 0003 leaves a case open, the choice made here:
//!
//! - **Large groups** show their first values and fold the rest under "Show N more
//!   values", until the full view of §3 exists.
//! - **"N values have no source"** is shown where some values have a source and some do
//!   not. Where none has one, the missing footnotes already say so, and the Identifiers
//!   tab, whose values seldom cite anything, stays quiet.
//! - **Repeated cells** in a Table are hidden from the left: a column's value is hidden
//!   only where it and every column before it repeat the row above, so that the hidden
//!   cells read as a grouping. They stay in the markup as visually hidden text.
//! - **The Matrix** draws the statements that have values in both columns; the table
//!   view beside it has every statement.

use std::cmp::Ordering;
use std::fmt::Write as _;

use askama::Template as _;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::{Snak, SnakKind, Statement};
use scatter_wikibase_model::value::DataValue;
use scatter_wikibase_shape::order::{amount, cmp_cells, cmp_snaks, snak_time};
use scatter_wikibase_shape::{Group, Role, Shape, Thresholds, classify};

use super::{Render, chart, values};
use crate::codex::{Align, Chip, ChipKind};
use crate::html::esc;

/// One group being drawn.
struct Card<'s> {
    property: &'s str,
    st: &'s [Statement],
    g: Group,
}

impl Card<'_> {
    fn best(&self, i: usize) -> bool {
        self.g.best.contains(&i)
    }

    fn hoisted(&self, k: &EntityId) -> bool {
        self.g.hoisted.contains(k)
    }
}

/// A row of a group's table.
struct Row {
    id: String,
    best: bool,
    cells: Vec<(bool, Align, String)>,
}

/// The `id` attribute of a statement's element: its statement ID.
fn id_attr(st: &Statement) -> String {
    st.id
        .as_ref()
        .map(|i| format!(" id=\"{}\"", esc(i.as_str())))
        .unwrap_or_default()
}

/// A table in Codex's markup. The caption is for assistive technology: the group's
/// property cell already names it.
fn grid(caption: &str, head: &[(String, Align)], rows: &[Row]) -> String {
    let mut s = format!(
        "<div class=\"cdx-table ts-grid\"><div class=\"cdx-table__table-wrapper\"><table class=\"cdx-table__table\"><caption class=\"ts-visually-hidden\">{}</caption><thead><tr>",
        esc(caption)
    );
    for (h, a) in head {
        let class = a
            .class()
            .map(|c| format!(" class=\"{c}\""))
            .unwrap_or_default();
        let _ = write!(s, "<th scope=\"col\"{class}>{h}</th>");
    }
    s.push_str("</tr></thead><tbody>");
    for r in rows {
        let class = if r.best {
            " class=\"ts-grid__row--best\""
        } else {
            ""
        };
        let _ = write!(s, "<tr{}{class}>", r.id);
        for (header, a, html) in &r.cells {
            let tag = if *header { "th" } else { "td" };
            let scope = if *header { " scope=\"row\"" } else { "" };
            let class = a
                .class()
                .map(|c| format!(" class=\"{c}\""))
                .unwrap_or_default();
            let _ = write!(s, "<{tag}{scope}{class}>{html}</{tag}>");
        }
        s.push_str("</tr>");
    }
    s.push_str("</tbody></table></div></div>");
    s
}

fn is_quantity(snak: Option<&Snak>) -> bool {
    matches!(
        snak.map(|s| &s.kind),
        Some(SnakKind::Value(DataValue::Quantity(_)))
    )
}

fn align_of(snak: Option<&Snak>) -> Align {
    if is_quantity(snak) {
        Align::Number
    } else {
        Align::Start
    }
}

fn same(a: Option<&Snak>, b: Option<&Snak>) -> bool {
    a.is_some() && b.is_some() && cmp_cells(a, b) == Ordering::Equal
}

impl Render<'_> {
    /// The `statements/{P}` region: one statement group in its shape, or the empty group
    /// for a property the entity does not use.
    #[must_use]
    pub fn group(&self, property: &str) -> String {
        let st = self
            .entity
            .statements
            .iter()
            .find(|(p, _)| p.as_str() == property)
            .map_or(&[][..], |(_, v)| v.as_slice());
        let dt = self.datatype_of(property, st);
        let c = Card {
            property,
            st,
            g: classify(st, dt.as_deref(), self.roles, &Thresholds::default()),
        };
        let shape = c.g.shape.name();
        let pid = esc(property);
        let mut s = format!(
            "<section class=\"ts-group ts-group--{shape}\" id=\"{pid}\" data-region=\"statements/{pid}\" data-shape=\"{shape}\"><div class=\"ts-group__property\">{}",
            self.property_link(property)
        );
        if st.len() > 1 {
            let n = st.len().to_string();
            let _ = write!(
                s,
                "<span class=\"ts-group__count\">{}</span>",
                esc(&self.m.with("ts-values-count", &[&n]))
            );
        }
        if self.editable {
            let label = self.property_label(property);
            let _ = write!(
                s,
                "<button type=\"button\" class=\"cdx-button cdx-button--action-progressive cdx-button--weight-quiet ts-edit\" data-ts-edit=\"group\" data-property=\"{pid}\" aria-label=\"{}\" hidden>{}</button>",
                esc(&self.m.with("ts-edit-group-label", &[&label])),
                esc(&self.m.get("ts-edit-group"))
            );
        }
        s.push_str("</div><div class=\"ts-group__body\">");
        match c.g.shape {
            Shape::Empty => {
                let _ = write!(
                    s,
                    "<p class=\"ts-empty\">{}</p>",
                    esc(&self.m.get("ts-group-empty"))
                );
            }
            Shape::NoCurrentValue => {
                let _ = write!(
                    s,
                    "<p class=\"ts-empty\">{}</p>",
                    esc(&self.m.get("ts-no-current-value"))
                );
            }
            Shape::Single => {
                let i = c.g.order[0];
                let skip = |k: &EntityId| c.hoisted(k);
                let _ = write!(
                    s,
                    "<div class=\"ts-statement ts-statement--headline\"{}>{}</div>",
                    id_attr(&c.st[i]),
                    self.body(&c, i, &skip, "")
                );
            }
            Shape::List => s.push_str(&self.list(&c, false)),
            Shape::Chips => s.push_str(&self.list(&c, true)),
            Shape::Timeline => s.push_str(&self.timeline(&c)),
            Shape::Series => s.push_str(&self.series(&c)),
            Shape::Table if c.g.numbered => s.push_str(&self.numbered(&c)),
            Shape::Table => s.push_str(&self.table(&c)),
        }
        s.push_str(&self.deprecated(&c));
        s.push_str(&self.footer(&c));
        s.push_str("</div></section>");
        s
    }

    /// A message whose parameters are HTML: the message is escaped, the parameters not,
    /// and each is isolated (`<bdi>`), so that a date in a fallback language keeps its
    /// order in a right-to-left page.
    fn msg_html(&self, key: &str, params: &[&str]) -> String {
        let marks: Vec<String> = (0..params.len())
            .map(|i| format!("\u{E000}{i}\u{E001}"))
            .collect();
        let refs: Vec<&str> = marks.iter().map(String::as_str).collect();
        let mut out = esc(&self.m.with(key, &refs));
        for (m, p) in marks.iter().zip(params) {
            out = out.replace(m.as_str(), &format!("<bdi>{p}</bdi>"));
        }
        out
    }

    fn property_label(&self, property: &str) -> String {
        self.lookup
            .label(property)
            .map_or_else(|| property.to_string(), |(t, _)| t.to_string())
    }

    /// The "Best value" badge, on a best value.
    fn badge(&self, c: &Card, i: usize) -> String {
        if !c.best(i) {
            return String::new();
        }
        let (text, tip) = (self.m.get("ts-best-value"), self.m.get("ts-best-value-tip"));
        let chip = Chip {
            kind: ChipKind::Best,
            text: &text,
            title: Some(&tip),
        };
        format!(" {}", chip.render().unwrap_or_default())
    }

    /// The footnote markers of row `i`.
    fn marks(&self, c: &Card, i: usize) -> String {
        let rows = &c.g.footnotes.rows[i];
        if rows.is_empty() {
            return String::new();
        }
        let mut s = String::from(" <span class=\"ts-fns\">");
        for k in rows {
            let n = (k + 1).to_string();
            let _ = write!(
                s,
                "<a class=\"ts-fn\" href=\"#{}-fn-{n}\" aria-label=\"{}\">{n}</a>",
                esc(c.property),
                esc(&self.m.with("ts-footnote-label", &[&n]))
            );
        }
        s.push_str("</span>");
        s
    }

    /// A statement's qualifiers as "key: value" rows, but for the keys `skip` takes out.
    fn qualifiers(&self, st: &Statement, skip: &dyn Fn(&EntityId) -> bool) -> String {
        let mut s = String::new();
        for (p, snaks) in st.qualifiers.iter().filter(|(k, _)| !skip(k)) {
            let values = snaks
                .iter()
                .map(|v| self.snak(v))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = write!(
                s,
                "<div class=\"ts-qualifiers__row\"><dt>{}</dt><dd>{values}</dd></div>",
                self.property_link(p.as_str())
            );
        }
        if s.is_empty() {
            s
        } else {
            format!("<dl class=\"ts-qualifiers\">{s}</dl>")
        }
    }

    /// The value line of statement `i`, then `extra`, then its qualifiers.
    fn body(&self, c: &Card, i: usize, skip: &dyn Fn(&EntityId) -> bool, extra: &str) -> String {
        let st = &c.st[i];
        format!(
            "<div class=\"ts-statement__value\"><span class=\"ts-statement__main\">{}</span>{}{}{}</div>{extra}{}",
            self.snak(&st.mainsnak),
            self.badge(c, i),
            self.statement_chips(st),
            self.marks(c, i),
            self.qualifiers(st, skip)
        )
    }

    /// The first value of a qualifier playing `role` (0003 §7).
    fn role_snak<'s>(&self, st: &'s Statement, role: Role) -> Option<&'s Snak> {
        st.qualifiers
            .iter()
            .find(|(k, _)| self.roles.is(k.as_str(), role))
            .and_then(|(_, v)| v.first())
    }

    /// Values `idx`, drawn by `render`; past the large-group limit, the rest under
    /// "Show N more values".
    fn split(&self, c: &Card, idx: &[usize], render: &dyn Fn(&[usize]) -> String) -> String {
        match c.g.inline {
            Some(k) if idx.len() > k => {
                let n = (idx.len() - k).to_string();
                format!(
                    "{}<details class=\"ts-fold ts-fold--more\"><summary>{}</summary>{}</details>",
                    render(&idx[..k]),
                    esc(&self.m.with("ts-show-more", &[&n])),
                    render(&idx[k..])
                )
            }
            _ => render(idx),
        }
    }

    /// The List and Chips shapes: best values first, the others under "N other values".
    fn list(&self, c: &Card, chips: bool) -> String {
        let skip = |k: &EntityId| c.hoisted(k);
        let render = |idx: &[usize]| {
            let items: String = idx
                .iter()
                .map(|&i| {
                    if chips {
                        let st = &c.st[i];
                        format!(
                            "<li class=\"ts-chips__item\"{}><span class=\"ts-statement__main\">{}</span>{}{}{}</li>",
                            id_attr(st),
                            self.snak(&st.mainsnak),
                            self.badge(c, i),
                            self.statement_chips(st),
                            self.marks(c, i)
                        )
                    } else {
                        format!(
                            "<li class=\"ts-statement\"{}>{}</li>",
                            id_attr(&c.st[i]),
                            self.body(c, i, &skip, "")
                        )
                    }
                })
                .collect();
            if chips {
                format!("<ul class=\"ts-chips\">{items}</ul>")
            } else {
                format!("<ol class=\"ts-group__values\">{items}</ol>")
            }
        };
        let visible: Vec<usize> =
            c.g.order
                .iter()
                .copied()
                .filter(|i| !c.g.folded.contains(i))
                .collect();
        let mut s = self.split(c, &visible, &render);
        if !c.g.folded.is_empty() {
            let n = c.g.folded.len().to_string();
            let _ = write!(
                s,
                "<details class=\"ts-fold\"><summary>{}</summary>{}</details>",
                esc(&self.m.with("ts-fold-others", &[&n])),
                render(&c.g.folded)
            );
        }
        s
    }

    /// When a Timeline row held: "since 2022", "until 1999", "1993 – 2001".
    fn period(&self, st: &Statement) -> String {
        let (a, b) = (
            self.role_snak(st, Role::TimeStart),
            self.role_snak(st, Role::TimeEnd),
        );
        let text = match (a, b) {
            (Some(a), Some(b)) => self.msg_html("ts-period-range", &[&self.snak(a), &self.snak(b)]),
            (Some(a), None) => self.msg_html("ts-period-since", &[&self.snak(a)]),
            (None, Some(b)) => self.msg_html("ts-period-until", &[&self.snak(b)]),
            (None, None) => return String::new(),
        };
        format!("<p class=\"ts-period\">{text}</p>")
    }

    /// The Timeline: a shared axis, then the rows, newest first, the best value leading.
    fn timeline(&self, c: &Card) -> String {
        let year = |s: Option<&Snak>| s.and_then(snak_time).map(chart::year_of);
        let rows: Vec<_> =
            c.g.order
                .iter()
                .map(|&i| {
                    (
                        year(self.role_snak(&c.st[i], Role::TimeStart)),
                        year(self.role_snak(&c.st[i], Role::TimeEnd)),
                        c.best(i),
                    )
                })
                .collect();
        let skip = |k: &EntityId| {
            c.hoisted(k)
                || self.roles.is(k.as_str(), Role::TimeStart)
                || self.roles.is(k.as_str(), Role::TimeEnd)
        };
        let render = |idx: &[usize]| {
            let mut items = String::new();
            for &i in idx {
                let _ = write!(
                    items,
                    "<li class=\"ts-statement\"{}>{}</li>",
                    id_attr(&c.st[i]),
                    self.body(c, i, &skip, &self.period(&c.st[i]))
                );
            }
            format!("<ol class=\"ts-group__values ts-timeline\">{items}</ol>")
        };
        let mut s = chart::timeline(&rows);
        s.push_str(&self.split(c, &c.g.order, &render));
        s
    }

    /// A view switch between two panels (0003 §9), the first chosen.
    fn views(&self, c: &Card, a: (&str, &str), b: (&str, &str)) -> String {
        let p = esc(c.property);
        let option = |x: &str, label: &str, checked: &str| {
            format!(
                "<span class=\"ts-views__option\"><input type=\"radio\" class=\"ts-views__input ts-views__input--{x}\" name=\"ts-view-{p}\" id=\"{p}-view-{x}\"{checked}><label for=\"{p}-view-{x}\">{}</label></span>",
                esc(label)
            )
        };
        format!(
            "<div class=\"ts-views\"><fieldset class=\"ts-views__switch\"><legend class=\"ts-views__legend\">{}</legend>{}{}</fieldset><div class=\"ts-views__panel ts-views__panel--a\">{}</div><div class=\"ts-views__panel ts-views__panel--b\">{}</div></div>",
            esc(&self.m.get("ts-views-label")),
            option("a", &self.m.get(a.0), " checked"),
            option("b", &self.m.get(b.0), ""),
            a.1,
            b.1
        )
    }

    /// The Series: the best (else the newest) value as a headline, then a chart with a
    /// switch to a table.
    fn series(&self, c: &Card) -> String {
        let point = |i: usize| self.role_snak(&c.st[i], Role::TimePoint);
        let head = c.g.best.first().copied().unwrap_or(c.g.order[0]);
        let when = point(head)
            .map(|t| {
                format!(
                    " <span class=\"ts-subtle\">{}</span>",
                    self.msg_html("ts-series-in", &[&self.snak(t)])
                )
            })
            .unwrap_or_default();
        let mut s = format!(
            "<div class=\"ts-statement ts-statement--headline\"><div class=\"ts-statement__value\"><span class=\"ts-statement__main\">{}</span>{when}{}</div></div>",
            self.snak(&c.st[head].mainsnak),
            self.badge(c, head)
        );

        let label = self.property_label(c.property);
        let chart = self.series_chart(c, &label);
        let time_key =
            c.g.order
                .iter()
                .find_map(|&i| point(i))
                .map(|t| t.property.clone());
        let skip = |k: &EntityId| c.hoisted(k) || self.roles.is(k.as_str(), Role::TimePoint);
        let marked = c.g.footnotes.rows.iter().any(|r| !r.is_empty());
        let mut head_row = vec![
            (
                time_key
                    .as_ref()
                    .map_or_else(String::new, |k| self.property_link(k.as_str())),
                Align::Start,
            ),
            (esc(&label), Align::Number),
        ];
        if marked {
            head_row.push((esc(&self.m.get("ts-column-sources")), Align::Start));
        }
        let rows = |idx: &[usize]| -> Vec<Row> {
            idx.iter()
                .map(|&i| {
                    let st = &c.st[i];
                    let mut row = Row {
                        id: id_attr(st),
                        best: c.best(i),
                        cells: vec![
                            (
                                true,
                                Align::Start,
                                point(i).map(|t| self.snak(t)).unwrap_or_default(),
                            ),
                            (
                                false,
                                Align::Number,
                                format!(
                                    "{}{}{}{}",
                                    self.snak(&st.mainsnak),
                                    self.badge(c, i),
                                    self.statement_chips(st),
                                    self.qualifiers(st, &skip)
                                ),
                            ),
                        ],
                    };
                    if marked {
                        row.cells.push((false, Align::Start, self.marks(c, i)));
                    }
                    row
                })
                .collect()
        };
        let table = self.split(c, &c.g.order, &|idx| grid(&label, &head_row, &rows(idx)));
        if chart.is_empty() {
            s.push_str(&table);
        } else {
            s.push_str(&self.views(c, ("ts-view-chart", &chart), ("ts-view-table", &table)));
        }
        s
    }

    /// The Series chart: the values with a point in time, oldest first; empty where
    /// fewer than two have one.
    fn series_chart(&self, c: &Card, label: &str) -> String {
        let mut points: Vec<(f64, f64)> =
            c.g.order
                .iter()
                .filter_map(|&i| {
                    let t = self
                        .role_snak(&c.st[i], Role::TimePoint)
                        .and_then(snak_time)?;
                    let SnakKind::Value(DataValue::Quantity(q)) = &c.st[i].mainsnak.kind else {
                        return None;
                    };
                    Some((chart::year_of(t), amount(&q.amount)?))
                })
                .collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (Some(first), Some(last)) = (points.first(), points.last()) else {
            return String::new();
        };
        let name = self.m.with(
            "ts-chart-label",
            &[
                label,
                &format!("{:.0}", first.0.floor()),
                &format!("{:.0}", last.0.floor()),
            ],
        );
        chart::series(&points, &name, |v| values::group(&format!("{v}")))
    }

    /// The Table: a column per qualifier key, the extra keys under each value, and a
    /// Matrix view where 0003 §3 allows one.
    fn table(&self, c: &Card) -> String {
        let label = self.property_label(c.property);
        let skip = |k: &EntityId| c.hoisted(k) || c.g.columns.contains(k);
        let value_align = align_of(c.g.order.first().map(|&i| &c.st[i].mainsnak));
        let mut head = vec![(esc(&label), value_align)];
        for k in &c.g.columns {
            let first = c.g.order.iter().find_map(|&i| Group::cell(c.st, i, k));
            head.push((self.property_link(k.as_str()), align_of(first)));
        }
        let marked = c.g.footnotes.rows.iter().any(|r| !r.is_empty());
        if marked {
            head.push((esc(&self.m.get("ts-column-sources")), Align::Start));
        }
        let rows = |idx: &[usize]| -> Vec<Row> {
            let mut prev: Option<usize> = None;
            idx.iter()
                .map(|&i| {
                    let st = &c.st[i];
                    let mut cells = vec![(
                        false,
                        value_align,
                        format!(
                            "<span class=\"ts-statement__main\">{}</span>{}{}{}",
                            self.snak(&st.mainsnak),
                            self.badge(c, i),
                            self.statement_chips(st),
                            self.qualifiers(st, &skip)
                        ),
                    )];
                    let mut run = prev.is_some();
                    for k in &c.g.columns {
                        let cell = Group::cell(c.st, i, k);
                        run = run && same(cell, prev.and_then(|j| Group::cell(c.st, j, k)));
                        let html = cell.map(|s| self.snak(s)).unwrap_or_default();
                        let html = if run {
                            format!("<span class=\"ts-visually-hidden\">{html}</span>")
                        } else {
                            html
                        };
                        cells.push((false, align_of(cell), html));
                    }
                    if marked {
                        cells.push((false, Align::Start, self.marks(c, i)));
                    }
                    prev = Some(i);
                    Row {
                        id: id_attr(st),
                        best: c.best(i),
                        cells,
                    }
                })
                .collect()
        };
        let table = self.split(c, &c.g.order, &|idx| grid(&label, &head, &rows(idx)));
        if c.g.matrix && c.g.columns.len() == 2 {
            self.views(
                c,
                ("ts-view-table", &table),
                ("ts-view-matrix", &self.matrix(c, &label)),
            )
        } else {
            table
        }
    }

    /// The Matrix: the first column's values down, the second's across.
    fn matrix(&self, c: &Card, label: &str) -> String {
        let (ka, kb) = (&c.g.columns[0], &c.g.columns[1]);
        let distinct = |k: &EntityId| {
            let mut v: Vec<&Snak> =
                c.g.order
                    .iter()
                    .filter_map(|&i| Group::cell(c.st, i, k))
                    .collect();
            v.sort_by(|a, b| cmp_snaks(a, b));
            v.dedup_by(|a, b| cmp_snaks(a, b) == Ordering::Equal);
            v
        };
        let (down, across) = (distinct(ka), distinct(kb));
        let value_align = align_of(c.g.order.first().map(|&i| &c.st[i].mainsnak));
        let corner = self.msg_html(
            "ts-matrix-corner",
            &[
                &self.property_link(ka.as_str()),
                &self.property_link(kb.as_str()),
            ],
        );
        let mut head = vec![(corner, Align::Start)];
        head.extend(across.iter().map(|b| (self.snak(b), align_of(Some(b)))));
        let none = format!(
            "<span class=\"ts-special-value\">{}</span>",
            esc(&self.m.get("ts-value-novalue"))
        );
        let rows: Vec<Row> = down
            .iter()
            .map(|a| {
                let mut cells = vec![(true, align_of(Some(a)), self.snak(a))];
                for b in &across {
                    let hit = c.g.order.iter().copied().find(|&i| {
                        same(Group::cell(c.st, i, ka), Some(a))
                            && same(Group::cell(c.st, i, kb), Some(b))
                    });
                    let html = hit.map_or_else(
                        || none.clone(),
                        |i| {
                            format!(
                                "{}{}{}",
                                self.snak(&c.st[i].mainsnak),
                                self.badge(c, i),
                                self.marks(c, i)
                            )
                        },
                    );
                    cells.push((false, value_align, html));
                }
                Row {
                    id: String::new(),
                    best: false,
                    cells,
                }
            })
            .collect();
        grid(label, &head, &rows)
    }

    /// A Table whose only column is the series ordinal: a list numbered by it.
    fn numbered(&self, c: &Card) -> String {
        let key = &c.g.columns[0];
        let skip = |k: &EntityId| c.hoisted(k) || k == key;
        let render = |idx: &[usize]| {
            let mut items = String::new();
            for &i in idx {
                let ordinal = Group::cell(c.st, i, key).and_then(|s| match &s.kind {
                    SnakKind::Value(DataValue::String(t)) => t.trim().parse::<u64>().ok(),
                    _ => None,
                });
                let value = ordinal
                    .map(|n| format!(" value=\"{n}\""))
                    .unwrap_or_default();
                let _ = write!(
                    items,
                    "<li class=\"ts-statement\"{value}{}>{}</li>",
                    id_attr(&c.st[i]),
                    self.body(c, i, &skip, "")
                );
            }
            format!("<ol class=\"ts-numbered\">{items}</ol>")
        };
        self.split(c, &c.g.order, &render)
    }

    /// The deprecated values, struck through with their reasons, under "N deprecated";
    /// open where there is no current value (0003 §4).
    fn deprecated(&self, c: &Card) -> String {
        if c.g.deprecated.is_empty() {
            return String::new();
        }
        let skip = |k: &EntityId| self.roles.is(k.as_str(), Role::DeprecationReason);
        let mut items = String::new();
        for &i in &c.g.deprecated {
            let st = &c.st[i];
            let reason = self
                .role_snak(st, Role::DeprecationReason)
                .map(|r| {
                    format!(
                        " <span class=\"ts-deprecated__reason\">{}</span>",
                        self.snak(r)
                    )
                })
                .unwrap_or_default();
            let _ = write!(
                items,
                "<li class=\"ts-statement ts-statement--deprecated\"{}><div class=\"ts-statement__value\"><span class=\"ts-statement__main\">{}</span>{reason}{}</div>{}{}</li>",
                id_attr(st),
                self.snak(&st.mainsnak),
                self.statement_chips(st),
                self.qualifiers(st, &skip),
                self.references(&st.references)
            );
        }
        let open = if c.g.shape == Shape::NoCurrentValue {
            " open"
        } else {
            ""
        };
        let n = c.g.deprecated.len().to_string();
        format!(
            "<details class=\"ts-fold ts-fold--deprecated\"{open}><summary>{}</summary><ol class=\"ts-group__values\">{items}</ol></details>",
            esc(&self.m.with("ts-fold-deprecated", &[&n]))
        )
    }

    /// The group's footer: "Same for all N", the footnotes, and the count of values
    /// without a source.
    fn footer(&self, c: &Card) -> String {
        let mut s = String::new();
        let count = c.g.n().to_string();
        if let (false, Some(&first)) = (c.g.hoisted.is_empty(), c.g.order.first()) {
            let keep = |k: &EntityId| !c.hoisted(k);
            let _ = write!(
                s,
                "<div class=\"ts-hoisted\"><p class=\"ts-hoisted__title\">{}</p>{}</div>",
                esc(&self.m.with("ts-same-for-all", &[&count])),
                self.qualifiers(&c.st[first], &keep)
            );
        }
        let notes = &c.g.footnotes;
        if !notes.notes.is_empty() {
            let _ = write!(
                s,
                "<ol class=\"ts-footnotes\" aria-label=\"{}\">",
                esc(&self.m.get("ts-column-sources"))
            );
            for (k, &(si, ri)) in notes.notes.iter().enumerate() {
                let shared = if k == 0 && notes.shared {
                    format!(
                        "<span class=\"ts-footnotes__shared\">{}</span>",
                        esc(&self.m.with("ts-cited-by-all", &[&count]))
                    )
                } else {
                    String::new()
                };
                let _ = write!(
                    s,
                    "<li id=\"{}-fn-{}\">{shared}{}</li>",
                    esc(c.property),
                    k + 1,
                    self.snak_groups(&c.st[si].references[ri].snaks, "ts-snaks")
                );
            }
            s.push_str("</ol>");
        }
        if notes.unsourced > 0 && notes.unsourced < c.g.n() {
            let u = notes.unsourced.to_string();
            let _ = write!(
                s,
                "<p class=\"ts-subtle ts-unsourced\">{}</p>",
                esc(&self.m.with("ts-unsourced", &[&u]))
            );
        }
        if s.is_empty() {
            s
        } else {
            format!("<div class=\"ts-group__footer\">{s}</div>")
        }
    }
}
