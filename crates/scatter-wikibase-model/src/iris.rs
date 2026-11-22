//! The IRIs inside values (0004 A20): the `unit` of a quantity, the `globe` of a coordinate
//! and the `calendarmodel` of a time, which Wikibase writes as concept IRIs.
//!
//! A store may keep them in another form, as Triplespace keeps the entity ID an IRI names.
//! [`ValueIriMap`] is how such a store gives the model the IRIs back, so that hashes are
//! taken, and JSON served, over the IRIs Wikibase would write; the map itself is the
//! store's (`scatter-wikibase-ids`' `ValueIris` for Triplespace's).

use std::borrow::Cow;

use crate::value::DataValue;

/// Gives back the IRI a stored value IRI stands for.
pub trait ValueIriMap: std::fmt::Debug + Send + Sync {
    /// The IRI Wikibase would write for `stored`, or `stored` itself where it already is
    /// one, is `"1"`, or names nothing the map knows.
    fn expand<'a>(&self, stored: &'a str) -> Cow<'a, str>;
}

/// Rewrites the value IRIs of `value` with `f`: its unit, globe or calendar model.
pub fn map_value_iris(value: &mut DataValue, f: impl Fn(&str) -> Cow<'_, str>) {
    let field = match value {
        DataValue::Quantity(q) => &mut q.unit,
        DataValue::GlobeCoordinate(g) => &mut g.globe,
        DataValue::Time(t) => &mut t.calendarmodel,
        _ => return,
    };
    if let Cow::Owned(new) = f(field) {
        *field = new;
    }
}
