//! Shape detection (0003 §3), rank folding (§4), shared qualifiers and references (§5).
//!
//! [`classify`] takes one statement group (every statement for one property on one
//! entity) and says how to draw it: the shape, the order of its values, which values are
//! the best ones and which fold away, the table's columns, the qualifiers stated once for
//! all, and the footnotes. It decides nothing about markup.
//!
//! Where 0003 leaves a case open, the choice made here:
//!
//! - **Hoisted keys and the Table rule.** A qualifier key with the same value on every
//!   statement is hoisted (§5), so it cannot make a Table on its own: rule 4 counts only
//!   keys that stay in the rows. Otherwise a group whose only shared qualifier is
//!   identical everywhere would become a table with no columns.
//! - **Time keys are not hoisted from a Timeline or Series,** whose axis they are.
//! - **"Cited by all"** needs at least two values; a single value's references are
//!   simply its footnotes.
//! - **References are equal when their snaks are,** which is what equal hashes mean
//!   (0003 §5): the model drops matching hashes at ingest (0006 §2).

use std::cmp::Ordering;

use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::{Rank, Reference, Snak, Statement};
use scatter_wikibase_model::value::DataType;

use crate::order::{cmp_cells, cmp_snaks, snak_time};
use crate::roles::{Role, Roles};

/// The thresholds of 0003 §3. The defaults are the ADR's starting guesses (0003 Q1); the
/// audit over a Wikidata sample is what settles them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// Rule 2: the least `time-start` coverage for a Timeline.
    pub timeline_coverage: f64,
    /// Rule 2: the fewest values for a Timeline.
    pub timeline_min: usize,
    /// Rule 3: the least `time-point` coverage for a Series.
    pub series_coverage: f64,
    /// Rule 3: the fewest values for a Series.
    pub series_min: usize,
    /// Rule 4: the least coverage of some qualifier key for a Table.
    pub table_coverage: f64,
    /// Rule 4: the fewest values for a Table.
    pub table_min: usize,
    /// The least coverage for a qualifier key to be a column.
    pub column_coverage: f64,
    /// The most distinct values a column may have for the Matrix view.
    pub matrix_max_distinct: usize,
    /// Above this many values, a group shows only its first few inline.
    pub large: usize,
    /// How many values a large group shows inline.
    pub large_inline: usize,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            timeline_coverage: 0.8,
            timeline_min: 2,
            series_coverage: 0.8,
            series_min: 4,
            table_coverage: 0.8,
            table_min: 3,
            column_coverage: 0.5,
            matrix_max_distinct: 12,
            large: 25,
            large_inline: 10,
        }
    }
}

/// A group's shape (0003 §3, §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shape {
    /// No statements at all.
    Empty,
    /// Only deprecated statements: "No current value", with them beneath (§4).
    NoCurrentValue,
    /// One value, as a headline.
    Single,
    /// Rows on a shared time axis, newest first.
    Timeline,
    /// Quantities over time: a headline, a line chart and a table.
    Series,
    /// One column per common qualifier key.
    Table,
    /// Values without qualifiers, as chips.
    Chips,
    /// One row per value, qualifiers beneath.
    List,
}

impl Shape {
    /// Every shape.
    pub const ALL: [Shape; 8] = [
        Shape::Empty,
        Shape::NoCurrentValue,
        Shape::Single,
        Shape::Timeline,
        Shape::Series,
        Shape::Table,
        Shape::Chips,
        Shape::List,
    ];

    /// The shape's name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Shape::Empty => "empty",
            Shape::NoCurrentValue => "no-current-value",
            Shape::Single => "single",
            Shape::Timeline => "timeline",
            Shape::Series => "series",
            Shape::Table => "table",
            Shape::Chips => "chips",
            Shape::List => "list",
        }
    }
}

/// The footnotes of a group (0003 §5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Footnotes {
    /// The distinct references, in footnote order, each by the first statement and the
    /// position in its references where it appears.
    pub notes: Vec<(usize, usize)>,
    /// Whether the first footnote is cited by every current value ("Cited by all N
    /// values"); its number is then not repeated on the rows.
    pub shared: bool,
    /// For each statement in the group, the footnotes its row shows (indices into
    /// `notes`); empty for deprecated statements, which show their own references.
    pub rows: Vec<Vec<usize>>,
    /// How many current values cite nothing ("2 values have no source").
    pub unsourced: usize,
}

/// How to draw one statement group.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The shape.
    pub shape: Shape,
    /// The current values (normal or preferred rank), as indices into the group, in the
    /// order they are drawn.
    pub order: Vec<usize>,
    /// The best values (preferred rank), which carry the badge.
    pub best: Vec<usize>,
    /// The values folded under "N other values" (Single, Chips and List with a best
    /// value); they are in `order` too, after the best ones.
    pub folded: Vec<usize>,
    /// The deprecated values, folded under "N deprecated", in group order.
    pub deprecated: Vec<usize>,
    /// The Table's columns: qualifier keys, in the order they first appear.
    pub columns: Vec<EntityId>,
    /// The Table's other qualifier keys, listed under their rows.
    pub extra: Vec<EntityId>,
    /// The qualifier keys stated once, under "Same for all N".
    pub hoisted: Vec<EntityId>,
    /// Whether the Table offers a Matrix view.
    pub matrix: bool,
    /// The footnotes.
    pub footnotes: Footnotes,
    /// For a large group, how many values show inline before "Open full view".
    pub inline: Option<usize>,
}

impl Group {
    /// The number of current values (n in 0003 §3).
    #[must_use]
    pub fn n(&self) -> usize {
        self.order.len()
    }

    /// The first value of qualifier `key` on statement `i`.
    #[must_use]
    pub fn cell<'s>(statements: &'s [Statement], i: usize, key: &EntityId) -> Option<&'s Snak> {
        statements[i].qualifiers.get(key).and_then(|v| v.first())
    }
}

fn same_snak(a: &Snak, b: &Snak) -> bool {
    a.property == b.property && a.kind == b.kind
}

fn same_snaks(a: &[Snak], b: &[Snak]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_snak(x, y))
}

fn same_reference(a: &Reference, b: &Reference) -> bool {
    a.snaks.len() == b.snaks.len()
        && a.snaks
            .iter()
            .zip(&b.snaks)
            .all(|((p, x), (q, y))| p == q && same_snaks(x, y))
}

/// The fraction of `current` with a qualifier for which `has` holds.
fn coverage(statements: &[Statement], current: &[usize], has: impl Fn(&EntityId) -> bool) -> f64 {
    if current.is_empty() {
        return 0.0;
    }
    let hits = current
        .iter()
        .filter(|&&i| statements[i].qualifiers.keys().any(&has))
        .count();
    #[allow(clippy::cast_precision_loss)]
    let r = hits as f64 / current.len() as f64;
    r
}

/// The qualifier keys of `current`, in the order they first appear.
fn keys(statements: &[Statement], current: &[usize]) -> Vec<EntityId> {
    let mut out: Vec<EntityId> = Vec::new();
    for &i in current {
        for k in statements[i].qualifiers.keys() {
            if !out.contains(k) {
                out.push(k.clone());
            }
        }
    }
    out
}

/// Keys with the same value list on every one of at least two statements.
fn hoistable(statements: &[Statement], current: &[usize], keys: &[EntityId]) -> Vec<EntityId> {
    if current.len() < 2 {
        return Vec::new();
    }
    keys.iter()
        .filter(|k| {
            let first = statements[current[0]].qualifiers.get(*k);
            first.is_some()
                && current.iter().all(|&i| {
                    statements[i]
                        .qualifiers
                        .get(*k)
                        .zip(first)
                        .is_some_and(|(v, f)| same_snaks(v, f))
                })
        })
        .cloned()
        .collect()
}

/// The time a statement sits at on an axis: its first qualifier in `role`.
fn role_time(
    statements: &[Statement],
    i: usize,
    roles: &Roles,
    role: Role,
) -> Option<(i64, u32, u32)> {
    statements[i]
        .qualifiers
        .iter()
        .filter(|(k, _)| roles.is(k.as_str(), role))
        .flat_map(|(_, v)| v.iter())
        .find_map(snak_time)
}

/// Newest first; values without a time last, in group order.
fn newest_first(statements: &[Statement], idx: &mut [usize], roles: &Roles, role: Role) {
    idx.sort_by(|&a, &b| {
        match (
            role_time(statements, a, roles, role),
            role_time(statements, b, roles, role),
        ) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then(a.cmp(&b))
    });
}

fn footnotes(statements: &[Statement], order: &[usize]) -> Footnotes {
    let mut f = Footnotes {
        rows: vec![Vec::new(); statements.len()],
        ..Footnotes::default()
    };
    // A reference on every current value, when there are at least two.
    if order.len() >= 2 {
        let first = &statements[order[0]];
        if let Some(pos) = first.references.iter().position(|r| {
            order[1..].iter().all(|&i| {
                statements[i]
                    .references
                    .iter()
                    .any(|s| same_reference(s, r))
            })
        }) {
            f.notes.push((order[0], pos));
            f.shared = true;
        }
    }
    for &i in order {
        let refs = &statements[i].references;
        if refs.is_empty() {
            f.unsourced += 1;
        }
        for (pos, r) in refs.iter().enumerate() {
            let found = f
                .notes
                .iter()
                .position(|&(s, p)| same_reference(&statements[s].references[p], r));
            let note = if let Some(n) = found {
                n
            } else {
                f.notes.push((i, pos));
                f.notes.len() - 1
            };
            if (!f.shared || note != 0) && !f.rows[i].contains(&note) {
                f.rows[i].push(note);
            }
        }
    }
    f
}

/// The main values' data type: the given one, else the first main snak's.
fn datatype<'a>(statements: &'a [Statement], given: Option<&'a str>) -> Option<&'a str> {
    given.or_else(|| {
        statements
            .iter()
            .find_map(|s| s.mainsnak.datatype.as_ref().map(DataType::id))
    })
}

/// The order the current values are drawn in, and those folded under "N other values".
fn display_order(
    statements: &[Statement],
    current: &[usize],
    best: &[usize],
    shape: Shape,
    columns: &[EntityId],
    roles: &Roles,
) -> (Vec<usize>, Vec<usize>) {
    let mut order = current.to_vec();
    let mut folded = Vec::new();
    match shape {
        Shape::Timeline => {
            newest_first(statements, &mut order, roles, Role::TimeStart);
            // The best value leads.
            order.sort_by_key(|i| !best.contains(i));
        }
        Shape::Series => newest_first(statements, &mut order, roles, Role::TimePoint),
        Shape::Table => order.sort_by(|&a, &b| {
            columns
                .iter()
                .map(|c| cmp_cells(Group::cell(statements, a, c), Group::cell(statements, b, c)))
                .find(|o| *o != Ordering::Equal)
                .unwrap_or(Ordering::Equal)
                .then_with(|| cmp_snaks(&statements[a].mainsnak, &statements[b].mainsnak))
                .then(a.cmp(&b))
        }),
        Shape::Single | Shape::Chips | Shape::List => {
            order.sort_by_key(|i| !best.contains(i));
            if !best.is_empty() {
                folded = order
                    .iter()
                    .copied()
                    .filter(|i| !best.contains(i))
                    .collect();
            }
        }
        Shape::Empty | Shape::NoCurrentValue => {}
    }
    (order, folded)
}

/// Whether a Table offers the Matrix view (0003 §3): exactly two columns, each with at
/// most `matrix_max_distinct` values, and no two statements with the same pair.
fn matrix_allowed(
    statements: &[Statement],
    order: &[usize],
    columns: &[EntityId],
    t: &Thresholds,
) -> bool {
    if columns.len() != 2 {
        return false;
    }
    let distinct = |c: &EntityId| {
        let mut vals: Vec<&Snak> = Vec::new();
        for &i in order {
            if let Some(s) = Group::cell(statements, i, c)
                && !vals.iter().any(|v| same_snak(v, s))
            {
                vals.push(s);
            }
        }
        vals.len()
    };
    let pairs_unique = order.iter().enumerate().all(|(x, &i)| {
        order[x + 1..].iter().all(|&j| {
            columns.iter().any(|c| {
                match (Group::cell(statements, i, c), Group::cell(statements, j, c)) {
                    (Some(a), Some(b)) => !same_snak(a, b),
                    (None, None) => false,
                    _ => true,
                }
            })
        })
    });
    distinct(&columns[0]) <= t.matrix_max_distinct
        && distinct(&columns[1]) <= t.matrix_max_distinct
        && pairs_unique
}

/// Classifies one statement group. `datatype` is the property's data type where known;
/// otherwise the main snaks' is used.
#[must_use]
pub fn classify(
    statements: &[Statement],
    datatype_hint: Option<&str>,
    roles: &Roles,
    t: &Thresholds,
) -> Group {
    let current: Vec<usize> = (0..statements.len())
        .filter(|&i| statements[i].rank != Rank::Deprecated)
        .collect();
    let deprecated: Vec<usize> = (0..statements.len())
        .filter(|&i| statements[i].rank == Rank::Deprecated)
        .collect();
    let best: Vec<usize> = current
        .iter()
        .copied()
        .filter(|&i| statements[i].rank == Rank::Preferred)
        .collect();
    let n = current.len();
    let dt = datatype(statements, datatype_hint);
    let all_keys = keys(statements, &current);
    let hoist_candidates = hoistable(statements, &current, &all_keys);
    let in_rows = |k: &EntityId| !hoist_candidates.contains(k);

    let shape = if statements.is_empty() {
        Shape::Empty
    } else if n == 0 {
        Shape::NoCurrentValue
    } else if n == 1 {
        Shape::Single
    } else if n >= t.timeline_min
        && coverage(statements, &current, |k| {
            roles.is(k.as_str(), Role::TimeStart)
        }) >= t.timeline_coverage
    {
        Shape::Timeline
    } else if n >= t.series_min
        && dt == Some("quantity")
        && coverage(statements, &current, |k| {
            roles.is(k.as_str(), Role::TimePoint)
        }) >= t.series_coverage
    {
        Shape::Series
    } else if n >= t.table_min
        && all_keys
            .iter()
            .any(|k| in_rows(k) && coverage(statements, &current, |q| q == k) >= t.table_coverage)
    {
        Shape::Table
    } else if current.iter().all(|&i| statements[i].qualifiers.is_empty())
        && matches!(dt, Some("wikibase-item" | "string" | "external-id"))
    {
        Shape::Chips
    } else {
        Shape::List
    };

    // Hoisting (§5), but never the axis of a Timeline or Series.
    let hoisted: Vec<EntityId> = hoist_candidates
        .iter()
        .filter(|k| match shape {
            Shape::Timeline => {
                !roles.is(k.as_str(), Role::TimeStart) && !roles.is(k.as_str(), Role::TimeEnd)
            }
            Shape::Series => !roles.is(k.as_str(), Role::TimePoint),
            _ => true,
        })
        .cloned()
        .collect();

    let (columns, extra) = if shape == Shape::Table {
        all_keys
            .iter()
            .filter(|k| !hoisted.contains(k))
            .cloned()
            .partition(|k| coverage(statements, &current, |q| q == k) >= t.column_coverage)
    } else {
        (Vec::new(), Vec::new())
    };

    let (order, folded) = display_order(statements, &current, &best, shape, &columns, roles);
    let matrix = shape == Shape::Table && matrix_allowed(statements, &order, &columns, t);
    let footnotes = footnotes(statements, &order);
    let inline = (n > t.large).then_some(t.large_inline);
    Group {
        shape,
        order,
        best,
        folded,
        deprecated,
        columns,
        extra,
        hoisted,
        matrix,
        footnotes,
        inline,
    }
}
