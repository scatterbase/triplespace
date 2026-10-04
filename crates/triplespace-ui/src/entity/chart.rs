//! Server-drawn SVG for the Series chart and the Timeline axis (0003 §3).
//!
//! Both are pictures of data that the same group also gives as text: the Series in its
//! table view, the Timeline in its rows. The chart has an accessible name that says what
//! it shows; the axis is decoration (`aria-hidden`). Colours come from the stylesheet
//! (`ts-chart__…` classes), not from attributes, so a theme reaches them.

use std::fmt::Write as _;

use crate::html::esc;

/// A time as a fractional year, for placing it on an axis.
#[must_use]
pub fn year_of((year, month, day): (i64, u32, u32)) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let y = year as f64;
    y + f64::from(month.max(1) - 1) / 12.0 + f64::from(day.max(1) - 1) / 365.0
}

const W: f64 = 600.0;
const H: f64 = 180.0;
const LEFT: f64 = 78.0;
const RIGHT: f64 = 14.0;
const TOP: f64 = 12.0;
const BOTTOM: f64 = 26.0;

fn fmt_year(x: f64) -> String {
    format!("{:.0}", x.floor())
}

/// A line chart of `(year, value)` points, in time order, with the first and last years
/// on the time axis and the least and greatest values on the value axis, as `label`
/// formats them.
#[must_use]
pub fn series(points: &[(f64, f64)], name: &str, value_label: impl Fn(f64) -> String) -> String {
    if points.len() < 2 {
        return String::new();
    }
    let (x0, x1) = (points[0].0, points[points.len() - 1].0);
    let (mut y0, mut y1) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.1), hi.max(p.1))
        });
    if (y1 - y0).abs() < f64::EPSILON {
        y0 -= 1.0;
        y1 += 1.0;
    }
    let xs = |x: f64| LEFT + (x - x0) / (x1 - x0).max(f64::EPSILON) * (W - LEFT - RIGHT);
    let ys = |y: f64| TOP + (1.0 - (y - y0) / (y1 - y0)) * (H - TOP - BOTTOM);
    let mut s = format!(
        "<svg class=\"ts-chart\" viewBox=\"0 0 {W} {H}\" role=\"img\" aria-label=\"{}\">",
        esc(name)
    );
    let _ = write!(
        s,
        "<line class=\"ts-chart__axis\" x1=\"{LEFT}\" y1=\"{b}\" x2=\"{r}\" y2=\"{b}\"/>\
         <line class=\"ts-chart__axis\" x1=\"{LEFT}\" y1=\"{TOP}\" x2=\"{LEFT}\" y2=\"{b}\"/>",
        b = H - BOTTOM,
        r = W - RIGHT
    );
    let pts: Vec<String> = points
        .iter()
        .map(|&(x, y)| format!("{:.1},{:.1}", xs(x), ys(y)))
        .collect();
    let _ = write!(
        s,
        "<polyline class=\"ts-chart__line\" points=\"{}\"/>",
        pts.join(" ")
    );
    for &(x, y) in points {
        let _ = write!(
            s,
            "<circle class=\"ts-chart__point\" cx=\"{:.1}\" cy=\"{:.1}\" r=\"2.5\"/>",
            xs(x),
            ys(y)
        );
    }
    let _ = write!(
        s,
        "<text class=\"ts-chart__label\" x=\"{LEFT}\" y=\"{}\" text-anchor=\"start\">{}</text>\
         <text class=\"ts-chart__label\" x=\"{}\" y=\"{}\" text-anchor=\"end\">{}</text>\
         <text class=\"ts-chart__label\" x=\"{}\" y=\"{}\" text-anchor=\"end\">{}</text>\
         <text class=\"ts-chart__label\" x=\"{}\" y=\"{}\" text-anchor=\"end\">{}</text>",
        H - 8.0,
        fmt_year(x0),
        W - RIGHT,
        H - 8.0,
        fmt_year(x1),
        LEFT - 6.0,
        TOP + 10.0,
        esc(&value_label(y1)),
        LEFT - 6.0,
        H - BOTTOM,
        esc(&value_label(y0)),
    );
    s.push_str("</svg>");
    s
}

/// Height of one row's lane on the Timeline axis, in pixels, and of its bar.
const LANE: f64 = 6.0;
const BAR: f64 = 4.0;

/// The Timeline's shared axis: a lane per row, in the rows' order, with a bar from the
/// row's start to its end, the best values marked. A row with no end runs off the end of
/// the axis, which reaches a little past the latest time in the group.
#[must_use]
pub fn timeline(rows: &[(Option<f64>, Option<f64>, bool)]) -> String {
    let times: Vec<f64> = rows.iter().flat_map(|r| [r.0, r.1]).flatten().collect();
    let (Some(lo), Some(hi)) = (
        times.iter().copied().reduce(f64::min),
        times.iter().copied().reduce(f64::max),
    ) else {
        return String::new();
    };
    let open = rows.iter().any(|r| r.0.is_some() && r.1.is_none());
    let pad = if open {
        ((hi - lo) * 0.08).max(1.0)
    } else {
        0.0
    };
    let span = (hi + pad - lo).max(1.0);
    let at = |t: f64| (t - lo) / span * 1000.0;
    #[allow(clippy::cast_precision_loss)]
    let height = LANE * rows.len() as f64;
    let mut s = format!(
        "<svg class=\"ts-axis\" viewBox=\"0 0 1000 {height}\" height=\"{height}\" preserveAspectRatio=\"none\" aria-hidden=\"true\" focusable=\"false\">\
         <rect class=\"ts-axis__track\" x=\"0\" y=\"0\" width=\"1000\" height=\"{height}\"/>",
    );
    for (lane, &(start, end, best)) in rows.iter().enumerate() {
        let (from, to, class) = match (start, end) {
            (Some(from), Some(to)) => (at(from), at(to), "ts-axis__bar"),
            (Some(from), None) => (at(from), 1000.0, "ts-axis__bar ts-axis__bar--open"),
            (None, Some(to)) => (0.0, at(to), "ts-axis__bar ts-axis__bar--open"),
            (None, None) => continue,
        };
        let best = if best { " ts-axis__bar--best" } else { "" };
        #[allow(clippy::cast_precision_loss)]
        let top = LANE * lane as f64 + (LANE - BAR) / 2.0;
        let _ = write!(
            s,
            "<rect class=\"{class}{best}\" x=\"{:.1}\" y=\"{top}\" width=\"{:.1}\" height=\"{BAR}\"/>",
            from.min(to),
            (to - from).abs().max(4.0)
        );
    }
    let _ = write!(
        s,
        "</svg><p class=\"ts-axis__span\" aria-hidden=\"true\"><span>{}</span><span>{}</span></p>",
        fmt_year(lo),
        fmt_year(hi)
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn years_and_charts() {
        assert!((year_of((2020, 7, 1)) - 2020.5).abs() < 0.01);
        assert!((year_of((2020, 0, 0)) - 2020.0).abs() < f64::EPSILON);
        let svg = series(
            &[(1900.0, 102.0), (2020.0, 3898.0)],
            "population from 1900 to 2020",
            |v| format!("{v}"),
        );
        assert!(svg.starts_with("<svg class=\"ts-chart\""));
        assert!(svg.contains("aria-label=\"population from 1900 to 2020\""));
        assert!(svg.contains(">1900</text>") && svg.contains(">2020</text>"));
        assert!(series(&[(1.0, 1.0)], "x", |v| format!("{v}")).is_empty());
        let axis = timeline(&[
            (Some(1990.0), Some(2000.0), false),
            (Some(2000.0), None, true),
        ]);
        assert!(axis.contains("aria-hidden=\"true\""));
        assert!(axis.contains("ts-axis__bar--best"));
        assert!(timeline(&[(None, None, false)]).is_empty());
    }
}
