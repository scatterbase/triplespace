//! Which headings start threads, which become heading posts, and which are dropped (0090 §3).

use std::ops::Range;

use crate::split::heading;

/// What a heading does in the conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// It starts a thread: level two, a level one with content directly under it, or a deeper
    /// heading before any thread.
    Thread,
    /// It becomes a heading post of this level.
    Sub(u8),
    /// A level-one heading with nothing directly under it: not structure.
    Dropped,
}

/// A heading line.
#[derive(Debug, Clone)]
pub(crate) struct HeadingInfo {
    pub line: usize,
    pub text: String,
    pub role: Role,
}

/// The headings of a page and their roles.
pub(crate) fn plan(lines: &[&str]) -> Vec<HeadingInfo> {
    let found: Vec<(usize, u8, String)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| heading(l).map(|(lv, t)| (i, lv, t)))
        .collect();
    let mut out = Vec::with_capacity(found.len());
    let mut in_thread = false;
    for (k, (line, level, text)) in found.iter().enumerate() {
        let next = found.get(k + 1).map_or(lines.len(), |n| n.0);
        let role = match level {
            1 if lines[line + 1..next].iter().all(|l| l.trim().is_empty()) => Role::Dropped,
            1 | 2 => Role::Thread,
            _ if in_thread => Role::Sub(*level),
            _ => Role::Thread,
        };
        match role {
            Role::Thread => in_thread = true,
            Role::Dropped => in_thread = false,
            Role::Sub(_) => {}
        }
        out.push(HeadingInfo {
            line: *line,
            text: text.clone(),
            role,
        });
    }
    out
}

/// A thread's span: its heading (an index into the plan), its body lines, and the dropped
/// level-one heading above it, if any.
#[derive(Debug, Clone)]
pub(crate) struct Span {
    pub heading: Option<usize>,
    pub body: Range<usize>,
    pub dropped_parent: Option<usize>,
}

/// One span per thread heading. A dropped heading's line belongs to no thread.
pub(crate) fn thread_spans(plan: &[HeadingInfo], n_lines: usize) -> Vec<Span> {
    let mut out = Vec::new();
    let mut dropped = None;
    for (k, h) in plan.iter().enumerate() {
        match h.role {
            Role::Dropped => dropped = Some(k),
            Role::Sub(_) => {}
            Role::Thread => {
                let end = plan[k + 1..]
                    .iter()
                    .find(|n| !matches!(n.role, Role::Sub(_)))
                    .map_or(n_lines, |n| n.line);
                out.push(Span {
                    heading: Some(k),
                    body: h.line + 1..end,
                    dropped_parent: dropped,
                });
            }
        }
    }
    out
}
