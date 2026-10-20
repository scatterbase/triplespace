//! Lowering: parse-wiki-text-2's nodes to the render tree (11 §1.2).
//!
//! The parser's output is half a tree: lists, headings, tables and preformatted blocks
//! nest, but bold and italic are toggles, HTML tags are flat starts and ends, and
//! paragraphs are breaks. This pass supplies the rest, following MediaWiki's documented
//! behaviour:
//!
//! - **Lines.** The source is read line by line. A line holds inline nodes, is blank, is
//!   a block (heading, list, table, preformatted block, redirect), or held only comments
//!   and category links and is dropped, as MediaWiki drops it.
//! - **Paragraphs**, by MediaWiki's block-level rules: a text line opens a paragraph; a
//!   blank line after one makes the next text line a new paragraph, and a second blank
//!   line adds a `<br>`; a line holding a block tag (`<div>`, `<table>`, `<hr>`, `<pre>`,
//!   an end `</p>` and the rest) closes the paragraph and is not wrapped, and one that
//!   opens a block without closing it leaves the lines after it unwrapped until one does.
//! - **Bold and italic**, by MediaWiki's apostrophe rules, per line: runs of two, three
//!   and five apostrophes, four as one literal and three, more than five as literals and
//!   five; when both counts are odd one bold run becomes a literal and italic, chosen by
//!   the letters before it; five apostrophes open in the order the next run implies; and
//!   everything still open closes at the end of the line.
//! - **Tags** are sanitized and handed to the [`Builder`], which balances them.

use std::collections::HashMap;

use parse_wiki_text_2::{
    DefinitionListItem, DefinitionListItemType, ListItem, Node as P, Positioned as _, TableCaption,
    TableCellType, TableRow,
};

use crate::config::{CATEGORY, Config, FILE};
use crate::html::{
    self, decode_entities, escape_id, legacy_id, parse_attributes, sanitize_attributes,
};
use crate::title::{Resolver, Title, parse_target};
use crate::tree::{Builder, Element, Node, Span, text_content};
use crate::{Category, ExternalLink, Extracted, InternalLink, Section, TemplateCall};

/// A piece of a line: a node, or part of a text node cut at newlines.
#[derive(Clone, Copy)]
enum Item<'a, 'n> {
    Node(&'n P<'a>),
    Text(&'a str, usize),
}

impl Item<'_, '_> {
    fn start(&self) -> usize {
        match self {
            Item::Node(n) => n.start(),
            Item::Text(_, s) => *s,
        }
    }
    fn end(&self) -> usize {
        match self {
            Item::Node(n) => n.end(),
            Item::Text(t, s) => s + t.len(),
        }
    }
}

enum Line<'a, 'n> {
    Inline(Vec<Item<'a, 'n>>),
    /// A line that held only comments and category links: it renders nothing and is not
    /// a blank line, but its categories still count.
    Removed(Vec<Item<'a, 'n>>),
    Blank,
    Block(&'n P<'a>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    None,
    P,
}

#[derive(Clone, Copy)]
enum Pending {
    Open,
    CloseOpen,
}

struct Para {
    last: Last,
    pending: Option<Pending>,
    in_block: bool,
}

/// One apostrophe event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Q {
    OpenB,
    OpenI,
    CloseB,
    CloseI,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Run {
    I,
    B,
    Bi,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QState {
    None,
    B,
    I,
    /// `<b><i>`: bold opened first.
    Bi,
    /// `<i><b>`: italic opened first.
    Ib,
}

/// What each apostrophe node of a line does.
#[derive(Default)]
struct QuotePlan {
    /// By node start: literal apostrophes written first, then the events.
    at: HashMap<usize, (usize, Vec<Q>)>,
    end: Vec<Q>,
}

pub(crate) struct Lowerer<'s, 'c> {
    src: &'s str,
    cfg: &'c Config,
    res: &'c dyn Resolver,
    page: &'c Title,
    pub(crate) b: Builder,
    pub(crate) out: Extracted,
    plan: QuotePlan,
    skip_apostrophes: usize,
    in_link: bool,
    autonumber: usize,
    anchors: HashMap<String, usize>,
    pub(crate) toc_here: bool,
}

/// Elements a [`crate::render`] tree uses to mark where `__TOC__` was.
pub(crate) const TOC_MARKER: &str = "ts-toc-marker";

impl<'s, 'c> Lowerer<'s, 'c> {
    pub(crate) fn new(
        src: &'s str,
        cfg: &'c Config,
        res: &'c dyn Resolver,
        page: &'c Title,
    ) -> Self {
        Self {
            src,
            cfg,
            res,
            page,
            b: Builder::new(),
            out: Extracted::default(),
            plan: QuotePlan::default(),
            skip_apostrophes: 0,
            in_link: false,
            autonumber: 0,
            anchors: HashMap::new(),
            toc_here: false,
        }
    }

    // ---- lines and paragraphs ------------------------------------------------------

    /// Lowers a sequence of block-level nodes. With `first_raw`, the first line is not
    /// wrapped in a paragraph: it shares its source line with a table cell's marker.
    pub(crate) fn blocks(&mut self, nodes: &[P<'s>], first_raw: bool) {
        let mut flat = Vec::new();
        flatten(nodes, &mut flat);
        let lines = self.lines(&flat);
        let mut st = Para {
            last: Last::None,
            pending: None,
            in_block: false,
        };
        for (i, line) in lines.iter().enumerate() {
            let raw = first_raw && i == 0;
            match line {
                Line::Removed(items) => {
                    for item in items {
                        if let Item::Node(n) = item {
                            self.inline(n);
                        }
                    }
                }
                Line::Block(node) => {
                    st.pending = None;
                    if let (P::Preformatted { nodes, .. }, true) = (node, st.in_block) {
                        // Inside an open block a leading space is only a space.
                        self.preformatted_as_text(nodes);
                        continue;
                    }
                    self.close_para(&mut st);
                    self.block(node);
                    if matches!(
                        node,
                        P::Table { .. }
                            | P::Heading { .. }
                            | P::Redirect { .. }
                            | P::HorizontalDivider { .. }
                    ) {
                        st.in_block = false;
                    }
                }
                Line::Blank => {
                    if raw || st.in_block {
                        continue;
                    }
                    if let Some(p) = st.pending.take() {
                        self.emit_pending(p);
                        self.b.void("br", Vec::new(), None);
                        st.last = Last::P;
                    } else if st.last == Last::P {
                        st.pending = Some(Pending::CloseOpen);
                    } else {
                        self.close_para(&mut st);
                        st.pending = Some(Pending::Open);
                    }
                }
                Line::Inline(items) => {
                    let (open, close) = Self::classify(items);
                    if raw {
                        st.in_block = false;
                    } else if open || close {
                        st.pending = None;
                        self.close_para(&mut st);
                        st.in_block = !close;
                    } else if !st.in_block {
                        if let Some(p) = st.pending.take() {
                            self.emit_pending(p);
                            st.last = Last::P;
                        } else if st.last != Last::P {
                            self.close_para(&mut st);
                            self.b.open_generated("p", Vec::new(), None);
                            st.last = Last::P;
                        }
                    }
                    self.line(items);
                    self.b.text("\n", None);
                }
            }
        }
        self.close_para(&mut st);
    }

    fn emit_pending(&mut self, p: Pending) {
        if let Pending::CloseOpen = p {
            self.b.close("p");
        }
        self.b.open_generated("p", Vec::new(), None);
    }

    fn close_para(&mut self, st: &mut Para) {
        if st.last == Last::P {
            self.b.close("p");
        }
        st.last = Last::None;
    }

    /// Whether a line opens a block MediaWiki's paragraph rules respect, and whether it
    /// closes one (MediaWiki's `openMatch` and `closeMatch`).
    fn classify(items: &[Item<'s, '_>]) -> (bool, bool) {
        let (mut open, mut close) = (false, false);
        for item in items {
            let Item::Node(n) = item else { continue };
            match n {
                P::StartTag { name, .. } => match name.as_ref() {
                    "table" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "pre" | "tr" | "p"
                    | "ul" | "ol" | "dl" | "li" => open = true,
                    "td" | "th" | "blockquote" | "div" | "hr" | "center" => close = true,
                    _ => {}
                },
                P::EndTag { name, .. } => match name.as_ref() {
                    "tr" | "td" | "th" => open = true,
                    "table" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "pre" | "p" | "li"
                    | "ul" | "ol" | "dl" | "blockquote" | "div" | "center" => close = true,
                    _ => {}
                },
                P::HorizontalDivider { .. } => close = true,
                P::Tag { name, .. }
                    if matches!(name.as_ref(), "pre" | "syntaxhighlight" | "source") =>
                {
                    close = true;
                }
                _ => {}
            }
        }
        (open, close)
    }

    /// Groups items into lines by their source line.
    fn lines<'n>(&self, items: &[Item<'s, 'n>]) -> Vec<Line<'s, 'n>> {
        let starts: Vec<usize> = std::iter::once(0)
            .chain(self.src.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let line_of = |pos: usize| starts.partition_point(|&s| s <= pos).saturating_sub(1);
        let mut lines = Vec::new();
        let mut cur: Vec<Item<'s, 'n>> = Vec::new();
        let mut cur_last = 0usize;
        let mut next_line: Option<usize> = None;
        let flush = |cur: &mut Vec<Item<'s, 'n>>, lines: &mut Vec<Line<'s, 'n>>| {
            if cur.is_empty() {
                return;
            }
            let items = std::mem::take(cur);
            let removed = items.iter().all(|i| {
                matches!(i, Item::Node(P::Comment { .. } | P::Category { .. }))
                    || matches!(i, Item::Text(t, _) if t.trim().is_empty())
            }) && items.iter().any(|i| matches!(i, Item::Node(_)));
            if removed {
                lines.push(Line::Removed(items));
                return;
            }
            if items
                .iter()
                .all(|i| matches!(i, Item::Text(t, _) if t.trim().is_empty()))
            {
                lines.push(Line::Blank);
            } else {
                lines.push(Line::Inline(items));
            }
        };
        for &item in items {
            if let Item::Text("\n", _) = item {
                continue;
            }
            let start = item.start();
            let end = item.end().max(start + 1);
            let ls = line_of(start);
            let le = line_of(end - 1);
            let block = matches!(
                item,
                Item::Node(
                    P::Heading { .. }
                        | P::UnorderedList { .. }
                        | P::OrderedList { .. }
                        | P::DefinitionList { .. }
                        | P::Preformatted { .. }
                        | P::Table { .. }
                        | P::Redirect { .. }
                        | P::HorizontalDivider { .. }
                        | P::ParagraphBreak { .. }
                )
            );
            if !cur.is_empty() && ls <= cur_last && !block {
                cur.push(item);
                cur_last = cur_last.max(le);
                continue;
            }
            if !cur.is_empty() {
                flush(&mut cur, &mut lines);
                next_line = Some(cur_last + 1);
            }
            if let Some(n) = next_line {
                for _ in n..ls {
                    lines.push(Line::Blank);
                }
            }
            if let Item::Node(P::ParagraphBreak { .. }) = item {
                // Its blank lines are counted from the positions around it.
                next_line.get_or_insert(ls);
                continue;
            }
            if block {
                let Item::Node(n) = item else { unreachable!() };
                lines.push(Line::Block(n));
                next_line = Some(le + 1);
            } else {
                cur.push(item);
                cur_last = le;
                next_line = None;
            }
        }
        flush(&mut cur, &mut lines);
        lines
    }

    // ---- blocks --------------------------------------------------------------------

    fn block(&mut self, node: &P<'s>) {
        match node {
            P::Heading {
                level,
                nodes,
                start,
                end,
            } => self.heading(*level, nodes, *start..*end),
            P::UnorderedList { items, .. } => self.list("ul", items),
            P::OrderedList { items, .. } => self.list("ol", items),
            P::DefinitionList { items, .. } => self.definition_list(items),
            P::Preformatted { nodes, start, end } => {
                self.b.open("pre", Vec::new(), Some(*start..*end));
                self.preformatted_lines(nodes);
                self.b.close("pre");
            }
            P::Table {
                attributes,
                captions,
                rows,
                start,
                end,
            } => {
                self.table(attributes, captions, rows, *start..*end);
            }
            P::Redirect { target, start, end } => self.redirect(target, *start..*end),
            // `----` is a line of its own: text after it on the same line starts a new
            // paragraph, as MediaWiki 1.39 renders it.
            P::HorizontalDivider { start, end } => {
                self.b.void("hr", Vec::new(), Some(*start..*end));
            }
            _ => {}
        }
    }

    fn preformatted_lines(&mut self, nodes: &[P<'s>]) {
        let mut flat = Vec::new();
        flatten(nodes, &mut flat);
        let mut line: Vec<Item<'s, '_>> = Vec::new();
        for item in flat {
            if let Item::Text("\n", _) = item {
                self.line(&line);
                self.b.text("\n", None);
                line.clear();
            } else {
                line.push(item);
            }
        }
        self.line(&line);
    }

    fn preformatted_as_text(&mut self, nodes: &[P<'s>]) {
        self.b.text(" ", None);
        let mut flat = Vec::new();
        flatten(nodes, &mut flat);
        let mut line: Vec<Item<'s, '_>> = Vec::new();
        for item in flat {
            if let Item::Text("\n", _) = item {
                self.line(&line);
                self.b.text("\n ", None);
                line.clear();
            } else {
                line.push(item);
            }
        }
        self.line(&line);
        self.b.text("\n", None);
    }

    fn heading(&mut self, level: u8, nodes: &[P<'s>], span: Span) {
        let inner = self.sub(|l| {
            let mut flat = Vec::new();
            flatten(nodes, &mut flat);
            l.line(&flat);
        });
        let mut text = String::new();
        text_content(&inner, &mut text);
        let base = escape_id(&text);
        let key = base.to_lowercase();
        let n = self.anchors.entry(key).or_insert(0);
        *n += 1;
        let anchor = if *n == 1 {
            base.clone()
        } else {
            format!("{base}_{n}")
        };
        let legacy = legacy_id(&anchor);
        let name = format!("h{}", level.clamp(1, 6));
        let mut h = Element::new(&name, Vec::new(), Some(span.clone()));
        if legacy != anchor {
            h.push(Element::new("span", vec![("id".into(), legacy)], None));
        }
        let mut headline = Element::new(
            "span",
            vec![
                ("class".into(), "mw-headline".into()),
                ("id".into(), anchor.clone()),
            ],
            None,
        );
        headline.children.clone_from(&inner);
        h.push(headline);
        self.out.sections.push(Section {
            level,
            anchor,
            line: inner,
            number: String::new(),
            index: self.out.sections.len() + 1,
            span,
        });
        self.b.block(h);
    }

    fn list(&mut self, tag: &str, items: &[ListItem<'s>]) {
        self.b.open(tag, Vec::new(), None);
        for item in items {
            self.b
                .open_generated("li", Vec::new(), Some(item.start..item.end));
            self.item_content(&item.nodes);
            self.b.close("li");
        }
        self.b.close(tag);
    }

    /// A list item: its inline text is one line; nested lists follow it.
    fn item_content(&mut self, nodes: &[P<'s>]) {
        let split = nodes
            .iter()
            .position(|n| {
                matches!(
                    n,
                    P::UnorderedList { .. } | P::OrderedList { .. } | P::DefinitionList { .. }
                )
            })
            .unwrap_or(nodes.len());
        let mut flat = Vec::new();
        flatten(&nodes[..split], &mut flat);
        self.line(&flat);
        for n in &nodes[split..] {
            self.block(n);
        }
    }

    fn definition_list(&mut self, items: &[DefinitionListItem<'s>]) {
        self.b.open("dl", Vec::new(), None);
        for item in items {
            match item.type_ {
                DefinitionListItemType::Details => {
                    self.b
                        .open_generated("dd", Vec::new(), Some(item.start..item.end));
                    self.item_content(&item.nodes);
                    self.b.close("dd");
                }
                DefinitionListItemType::Term => {
                    // `;term:details` on one line: the first colon outside links and tags.
                    let colon = item.nodes.iter().enumerate().find_map(|(i, n)| match n {
                        P::Text { value, .. } => value.find(':').map(|p| (i, p)),
                        _ => None,
                    });
                    self.b
                        .open_generated("dt", Vec::new(), Some(item.start..item.end));
                    match colon {
                        None => {
                            self.item_content(&item.nodes);
                            self.b.close("dt");
                        }
                        Some((i, p)) => {
                            let P::Text { value, start, .. } = &item.nodes[i] else {
                                unreachable!()
                            };
                            let mut flat = Vec::new();
                            flatten(&item.nodes[..i], &mut flat);
                            let before = value[..p].trim_end();
                            if !before.is_empty() {
                                flat.push(Item::Text(before, *start));
                            }
                            self.line(&flat);
                            self.b.close("dt");
                            self.b.open_generated("dd", Vec::new(), None);
                            let mut flat = Vec::new();
                            let after = value[p + 1..].trim_start();
                            if !after.is_empty() {
                                flat.push(Item::Text(after, start + value.len() - after.len()));
                            }
                            flatten(&item.nodes[i + 1..], &mut flat);
                            self.line(&flat);
                            self.b.close("dd");
                        }
                    }
                }
            }
        }
        self.b.close("dl");
    }

    fn attrs_of(&self, tag: &str, nodes: &[P<'s>]) -> Vec<(String, String)> {
        match (nodes.first(), nodes.last()) {
            (Some(a), Some(z)) => {
                sanitize_attributes(tag, parse_attributes(&self.src[a.start()..z.end()]))
            }
            _ => Vec::new(),
        }
    }

    fn table(
        &mut self,
        attributes: &[P<'s>],
        captions: &[TableCaption<'s>],
        rows: &[TableRow<'s>],
        span: Span,
    ) {
        let attrs = self.attrs_of("table", attributes);
        self.b.open("table", attrs, Some(span));
        for cap in captions {
            let attrs = cap
                .attributes
                .as_deref()
                .map(|a| self.attrs_of("caption", a))
                .unwrap_or_default();
            self.b.open("caption", attrs, None);
            self.blocks(&cap.content, true);
            self.b.close("caption");
        }
        if !rows.is_empty() {
            self.b.open("tbody", Vec::new(), None);
        }
        for row in rows {
            let attrs = self.attrs_of("tr", &row.attributes);
            self.b.open("tr", attrs, Some(row.start..row.end));
            for cell in &row.cells {
                let tag = if cell.type_ == TableCellType::Heading {
                    "th"
                } else {
                    "td"
                };
                let attrs = cell
                    .attributes
                    .as_deref()
                    .map(|a| self.attrs_of(tag, a))
                    .unwrap_or_default();
                self.b.open(tag, attrs, None);
                self.blocks(&cell.content, true);
                self.b.close(tag);
            }
            self.b.close("tr");
        }
        if !rows.is_empty() {
            self.b.close("tbody");
        }
        self.b.close("table");
    }

    fn redirect(&mut self, target: &str, span: Span) {
        let Some(t) = parse_target(target, self.cfg, self.page) else {
            return;
        };
        self.out.redirect = Some(t.title.clone());
        let link = self.sub(|l| {
            l.anchor(&t.title, span.clone(), |l| {
                l.b.text(&t.title.prefixed(l.cfg), None);
            });
        });
        let mut div = Element::new(
            "div",
            vec![("class".into(), "redirectMsg".into())],
            Some(span),
        );
        div.push(Element::new("p", Vec::new(), None))
            .push_text(&self.cfg.messages().redirect_to);
        let ul = div.push(Element::new(
            "ul",
            vec![("class".into(), "redirectText".into())],
            None,
        ));
        ul.push(Element::new("li", Vec::new(), None)).children = link;
        self.b.block(div);
    }

    /// Runs `f` against a fresh builder and returns what it built.
    fn sub(&mut self, f: impl FnOnce(&mut Self)) -> Vec<Node> {
        let saved = std::mem::take(&mut self.b);
        let saved_plan = std::mem::take(&mut self.plan);
        f(self);
        self.plan = saved_plan;
        std::mem::replace(&mut self.b, saved).finish()
    }

    // ---- inline --------------------------------------------------------------------

    /// One line of inline items, with its own apostrophe plan.
    fn line(&mut self, items: &[Item<'s, '_>]) {
        let plan = self.plan_quotes(items);
        let saved = std::mem::replace(&mut self.plan, plan);
        self.skip_apostrophes = 0;
        self.items(items);
        let end = std::mem::take(&mut self.plan.end);
        self.quote_events(&end);
        self.plan = saved;
    }

    fn items(&mut self, items: &[Item<'s, '_>]) {
        let mut run = String::new();
        let mut run_span: Option<Span> = None;
        for item in items {
            match item {
                Item::Text(t, s) => {
                    let mut t: &str = t;
                    let mut s = *s;
                    while self.skip_apostrophes > 0 && t.starts_with('\'') {
                        t = &t[1..];
                        s += 1;
                        self.skip_apostrophes -= 1;
                    }
                    self.skip_apostrophes = 0;
                    run.push_str(t);
                    run_span = Some(join(run_span, s..s + t.len()));
                }
                Item::Node(P::CharacterEntity {
                    character,
                    start,
                    end,
                }) => {
                    run.push(*character);
                    run_span = Some(join(run_span, *start..*end));
                }
                Item::Node(n) => {
                    self.flush_run(&mut run, &mut run_span);
                    self.inline(n);
                }
            }
        }
        self.flush_run(&mut run, &mut run_span);
    }

    fn flush_run(&mut self, run: &mut String, span: &mut Option<Span>) {
        if run.is_empty() {
            return;
        }
        let text = std::mem::take(run);
        let span = span.take();
        if self.in_link {
            self.b.text(&text, span);
        } else {
            self.free_links(&text, span);
        }
    }

    // One arm per parser node, which is clearer whole than split.
    #[allow(clippy::too_many_lines)]
    fn inline(&mut self, node: &P<'s>) {
        match node {
            P::Text { value, start, .. } => {
                let mut run = (*value).to_string();
                let mut span = Some(*start..*start + value.len());
                self.flush_run(&mut run, &mut span);
            }
            P::CharacterEntity {
                character,
                start,
                end,
            } => {
                self.b.text(&character.to_string(), Some(*start..*end));
            }
            P::Bold { start, .. } | P::Italic { start, .. } | P::BoldItalic { start, .. } => {
                let (literal, events) = self.plan.at.get(start).cloned().unwrap_or_default();
                if literal > 0 {
                    self.b.text(&"'".repeat(literal), None);
                }
                let consumed = match node {
                    P::Italic { .. } => 2,
                    P::Bold { .. } => 3,
                    _ => 5,
                };
                self.skip_apostrophes = self.run_length(*start).saturating_sub(consumed);
                self.quote_events(&events);
            }
            P::Link {
                target,
                text,
                start,
                end,
            } => self.link(target, text, *start..*end),
            P::Category {
                target,
                ordinal,
                start,
                end,
            } => {
                let sort_key = (!ordinal.is_empty()).then(|| {
                    let (a, z) = (ordinal[0].start(), ordinal[ordinal.len() - 1].end());
                    decode_entities(&self.src[a..z])
                });
                let name = parse_target(target, self.cfg, self.page)
                    .map_or_else(|| (*target).to_string(), |t| t.title.text);
                self.out.categories.push(Category {
                    name,
                    sort_key,
                    span: *start..*end,
                });
            }
            P::Image { start, end, .. } => self.chip(*start..*end, false),
            P::ExternalLink { nodes, start, end } => self.external_link(nodes, *start..*end),
            P::Template {
                name, start, end, ..
            } => {
                let name = match (name.first(), name.last()) {
                    (Some(a), Some(z)) => self.src[a.start()..z.end()].trim().to_string(),
                    _ => String::new(),
                };
                self.out.templates.push(TemplateCall {
                    name,
                    span: *start..*end,
                });
                self.chip(*start..*end, true);
            }
            P::Parameter { start, end, .. } => {
                self.b.text(&self.src[*start..*end], Some(*start..*end));
            }
            P::MagicWord { start, end } => {
                let word = self.src[*start..*end].trim_matches('_').to_uppercase();
                if word == "TOC" && !self.toc_here {
                    self.toc_here = true;
                    self.b.block(Element::new(TOC_MARKER, Vec::new(), None));
                }
                self.out.switches.push(word);
            }
            P::StartTag { name, start, end } => self.start_tag(name, *start..*end),
            P::EndTag { name, .. } => {
                if crate::tree::is_void(name) {
                    self.b.void(name, Vec::new(), None);
                } else if html::tag_allowed(name) {
                    self.b.close(name);
                }
            }
            P::Tag {
                name,
                nodes,
                start,
                end,
            } => self.extension_tag(name, nodes, *start..*end),
            P::HorizontalDivider { start, end } => {
                self.b.void("hr", Vec::new(), Some(*start..*end));
            }
            // Blocks where only inline content was expected.
            P::Heading { .. }
            | P::UnorderedList { .. }
            | P::OrderedList { .. }
            | P::DefinitionList { .. }
            | P::Preformatted { .. }
            | P::Table { .. }
            | P::Redirect { .. } => self.block(node),
            P::Comment { .. } | P::ParagraphBreak { .. } => {}
        }
    }

    fn start_tag(&mut self, name: &str, span: Span) {
        if !html::tag_allowed(name) {
            self.b.text(&self.src[span.clone()], Some(span));
            return;
        }
        let raw = &self.src[span.clone()];
        let self_closing = raw.ends_with("/>");
        let inner = raw
            .get(1 + name.len()..raw.len() - if self_closing { 2 } else { 1 })
            .unwrap_or("");
        let attrs = sanitize_attributes(name, parse_attributes(inner));
        if crate::tree::is_void(name) {
            self.b.void(name, attrs, Some(span));
        } else {
            self.b.open(name, attrs, Some(span));
            if self_closing {
                self.b.close(name);
            }
        }
    }

    /// The source of an extension tag: its attributes and its raw content.
    fn tag_parts(&self, span: &Span) -> (&'s str, &'s str) {
        let raw = &self.src[span.clone()];
        let open_end = raw.find('>').map_or(raw.len(), |i| i + 1);
        let attrs = raw[..open_end].trim_start_matches('<');
        let attrs = attrs.trim_end_matches('>').trim_end_matches('/');
        let attrs = attrs
            .split_once(|c: char| c.is_whitespace())
            .map_or("", |(_, a)| a);
        if raw[..open_end].ends_with("/>") {
            return (attrs, "");
        }
        let close_start = raw
            .rfind("</")
            .filter(|&c| c >= open_end)
            .unwrap_or(raw.len());
        (attrs, &raw[open_end..close_start])
    }

    fn extension_tag(&mut self, name: &str, nodes: &[P<'s>], span: Span) {
        match name {
            "nowiki" => {
                let (_, content) = self.tag_parts(&span);
                self.b.text(&decode_entities(content), Some(span));
            }
            "pre" | "syntaxhighlight" | "source" => {
                let (attrs, content) = self.tag_parts(&span);
                let attrs = if name == "pre" {
                    sanitize_attributes("pre", parse_attributes(attrs))
                } else {
                    Vec::new()
                };
                let content = content.strip_prefix('\n').unwrap_or(content);
                self.b.open("pre", attrs, Some(span.clone()));
                self.b.text(&decode_entities(content), Some(span));
                self.b.close("pre");
            }
            "noinclude" | "onlyinclude" => {
                let mut flat = Vec::new();
                flatten(nodes, &mut flat);
                self.items(&flat);
            }
            "includeonly" | "section" => {}
            _ => self.chip(span, false),
        }
    }

    /// A visible chip showing the source of something the subset does not render
    /// (0008 §8; 0010 §4).
    fn chip(&mut self, span: Span, template: bool) {
        let m = self.cfg.messages();
        let title = if template {
            &m.chip_template
        } else {
            &m.chip_unsupported
        };
        let kind = if template {
            "ts-chip--template"
        } else {
            "ts-chip--unsupported"
        };
        let mut chip = Element::new(
            "span",
            vec![
                ("class".into(), format!("ts-chip ts-chip--wikitext {kind}")),
                ("title".into(), title.clone()),
            ],
            Some(span.clone()),
        );
        chip.push(Element::new("code", Vec::new(), None))
            .push_text(&self.src[span]);
        self.b.inline(chip);
    }

    // ---- links ---------------------------------------------------------------------

    fn link(&mut self, target: &'s str, text: &[P<'s>], span: Span) {
        let target_start = target.as_ptr() as usize - self.src.as_ptr() as usize;
        let target_end = target_start + target.len();
        let piped = self.src.as_bytes().get(target_end) == Some(&b'|');
        let Some(t) = parse_target(target, self.cfg, self.page) else {
            self.b.text(&self.src[span.clone()], Some(span));
            return;
        };
        if !t.leading_colon && t.title.interwiki.is_none() {
            if t.title.namespace == CATEGORY {
                self.out.categories.push(Category {
                    name: t.title.text,
                    sort_key: None,
                    span,
                });
                return;
            }
            if t.title.namespace == FILE {
                self.chip(span, false);
                return;
            }
        }
        self.out.links.push(InternalLink {
            target: t.title.clone(),
            span: span.clone(),
        });
        let label = if piped {
            None
        } else {
            self.res.info(&t.title).label
        };
        let trail_start = self.src[target_end..span.end]
            .find("]]")
            .map_or(span.end, |i| target_end + i + 2);
        self.anchor(&t.title, span, |l| {
            let was = std::mem::replace(&mut l.in_link, true);
            if piped {
                let mut flat = Vec::new();
                flatten(text, &mut flat);
                l.items(&flat);
            } else {
                let shown = if let Some(label) = &label {
                    label.clone()
                } else {
                    let written = &l.src[target_start..target_end];
                    written
                        .strip_prefix(':')
                        .unwrap_or(written)
                        .trim()
                        .to_string()
                };
                l.b.text(&shown, None);
                let mut flat = Vec::new();
                for n in text.iter().filter(|n| n.start() >= trail_start) {
                    flatten(std::slice::from_ref(n), &mut flat);
                }
                l.items(&flat);
            }
            l.in_link = was;
        });
    }

    /// An `<a>` to a title, with `content` inside.
    fn anchor(&mut self, title: &Title, span: Span, content: impl FnOnce(&mut Self)) {
        let mut attrs: Vec<(String, String)> = Vec::new();
        let prefixed = title.prefixed(self.cfg);
        if let Some(prefix) = &title.interwiki {
            let url = self
                .cfg
                .interwiki(prefix)
                .map_or(String::new(), |iw| iw.url.clone());
            let mut encoded = html::url_encode_title(&title.text);
            if let Some(f) = &title.fragment {
                encoded.push('#');
                encoded.push_str(&escape_id(f));
            }
            attrs.push(("href".into(), url.replace("$1", &encoded)));
            attrs.push(("class".into(), "extiw".into()));
            attrs.push(("title".into(), prefixed));
        } else if title.without_fragment() == self.page.without_fragment() {
            match &title.fragment {
                Some(f) => attrs.push(("href".into(), format!("#{}", escape_id(f)))),
                None => attrs.push(("class".into(), "mw-selflink selflink".into())),
            }
        } else {
            let info = self.res.info(&title.without_fragment());
            if info.exists {
                let mut href = self.res.url(&title.without_fragment());
                if let Some(f) = &title.fragment {
                    href.push('#');
                    href.push_str(&escape_id(f));
                }
                attrs.push(("href".into(), href));
                if info.redirect {
                    attrs.push(("class".into(), "mw-redirect".into()));
                }
                attrs.push(("title".into(), prefixed));
            } else {
                attrs.push((
                    "href".into(),
                    self.res.missing_url(&title.without_fragment()),
                ));
                attrs.push(("class".into(), "new".into()));
                let tip = self
                    .cfg
                    .messages()
                    .missing_page
                    .replace("$1", &title.without_fragment().prefixed(self.cfg));
                attrs.push(("title".into(), tip));
            }
        }
        self.b.open("a", attrs, Some(span));
        content(self);
        self.b.close("a");
    }

    fn external_link(&mut self, nodes: &[P<'s>], span: Span) {
        // The URL runs to the first whitespace; the rest is the text.
        let mut url = String::new();
        let mut rest: Vec<Item<'s, '_>> = Vec::new();
        let mut in_url = true;
        let mut flat = Vec::new();
        flatten(nodes, &mut flat);
        for item in flat {
            if !in_url {
                rest.push(item);
                continue;
            }
            match item {
                Item::Text(t, s) => match t.find(char::is_whitespace) {
                    Some(i) => {
                        url.push_str(&t[..i]);
                        in_url = false;
                        let after = t[i..].trim_start();
                        if !after.is_empty() {
                            rest.push(Item::Text(after, s + t.len() - after.len()));
                        }
                    }
                    None => url.push_str(t),
                },
                Item::Node(P::CharacterEntity { character, .. }) => url.push(*character),
                other @ Item::Node(_) => {
                    in_url = false;
                    rest.push(other);
                }
            }
        }
        self.out.external_links.push(ExternalLink {
            url: url.clone(),
            span: span.clone(),
        });
        let has_text = rest
            .iter()
            .any(|i| !matches!(i, Item::Text(t, _) if t.trim().is_empty()));
        let class = if has_text {
            "external text"
        } else {
            "external autonumber"
        };
        self.b.open(
            "a",
            vec![
                ("rel".into(), "nofollow".into()),
                ("class".into(), class.into()),
                ("href".into(), url),
            ],
            Some(span),
        );
        let was = std::mem::replace(&mut self.in_link, true);
        if has_text {
            self.items(&rest);
        } else {
            self.autonumber += 1;
            self.b.text(&format!("[{}]", self.autonumber), None);
        }
        self.in_link = was;
        self.b.close("a");
    }

    /// Text, with bare URLs made links (`class="external free"`).
    fn free_links(&mut self, text: &str, span: Option<Span>) {
        let lower = text.to_lowercase();
        let mut pos = 0;
        let protocols: Vec<&str> = self
            .cfg
            .source
            .protocols
            .iter()
            .map(String::as_str)
            .filter(|p| *p != "//")
            .collect();
        let mut i = 0;
        while i < text.len() {
            if !text.is_char_boundary(i) {
                i += 1;
                continue;
            }
            let boundary = text[..i]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_');
            let found = boundary
                .then(|| {
                    protocols
                        .iter()
                        .find(|p| lower[i..].starts_with(&p.to_lowercase()))
                })
                .flatten();
            let Some(proto) = found else {
                i += text[i..].chars().next().map_or(1, char::len_utf8);
                continue;
            };
            let after = i + proto.len();
            let mut end = after;
            for (o, c) in text[after..].char_indices() {
                if c.is_whitespace()
                    || c.is_control()
                    || matches!(c, '[' | ']' | '<' | '>' | '"' | '\u{FFFD}')
                {
                    break;
                }
                end = after + o + c.len_utf8();
            }
            let mut url = &text[i..end];
            loop {
                let trimmed = url.trim_end_matches([',', ';', '.', ':', '!', '?', '\'']);
                let trimmed = if trimmed.ends_with(')') && !trimmed.contains('(') {
                    &trimmed[..trimmed.len() - 1]
                } else {
                    trimmed
                };
                if trimmed.len() == url.len() {
                    break;
                }
                url = trimmed;
            }
            if url.len() <= proto.len() {
                i = after;
                continue;
            }
            let start_abs = span.as_ref().map(|s| s.start + i);
            self.b.text(&text[pos..i], None);
            let url_span = start_abs.map(|s| s..s + url.len());
            self.out.external_links.push(ExternalLink {
                url: url.to_string(),
                span: url_span.clone().unwrap_or(0..0),
            });
            self.b.open(
                "a",
                vec![
                    ("rel".into(), "nofollow".into()),
                    ("class".into(), "external free".into()),
                    ("href".into(), url.to_string()),
                ],
                url_span,
            );
            self.b.text(url, None);
            self.b.close("a");
            i += url.len();
            pos = i;
        }
        if pos == 0 {
            self.b.text(text, span);
        } else {
            self.b.text(&text[pos..], None);
        }
    }

    // ---- apostrophes ---------------------------------------------------------------

    fn run_length(&self, start: usize) -> usize {
        self.src[start..]
            .bytes()
            .take_while(|&b| b == b'\'')
            .count()
    }

    fn quote_events(&mut self, events: &[Q]) {
        for e in events {
            match e {
                Q::OpenB => self.b.open("b", Vec::new(), None),
                Q::OpenI => self.b.open("i", Vec::new(), None),
                Q::CloseB => self.b.close("b"),
                Q::CloseI => self.b.close("i"),
            }
        }
    }

    fn plan_quotes(&self, items: &[Item<'s, '_>]) -> QuotePlan {
        let mut tokens: Vec<(usize, Run, usize)> = Vec::new();
        for item in items {
            if let Item::Node(n) = item {
                collect_quotes(n, &mut tokens, self);
            }
        }
        tokens.sort_by_key(|t| t.0);
        let italics = tokens
            .iter()
            .filter(|t| matches!(t.1, Run::I | Run::Bi))
            .count();
        let bolds = tokens
            .iter()
            .filter(|t| matches!(t.1, Run::B | Run::Bi))
            .count();
        if italics % 2 == 1 && bolds % 2 == 1 {
            let mut single = None;
            let mut multi = None;
            let mut space = None;
            for (k, &(start, run, literal)) in tokens.iter().enumerate() {
                if run != Run::B {
                    continue;
                }
                let at = start + literal;
                let mut before = self.src[..at].chars().rev();
                let x1 = before.next();
                let x2 = before.next();
                if x1 == Some(' ') {
                    space.get_or_insert(k);
                } else if x2 == Some(' ') {
                    single = Some(k);
                    break;
                } else {
                    multi.get_or_insert(k);
                }
            }
            if let Some(k) = single.or(multi).or(space) {
                tokens[k].1 = Run::I;
                tokens[k].2 += 1;
            }
        }
        let mut plan = QuotePlan::default();
        let mut st = QState::None;
        for k in 0..tokens.len() {
            let (start, run, literal) = tokens[k];
            let next = tokens.get(k + 1).map(|t| t.1);
            let (events, next_st) = transition(st, run, next);
            st = next_st;
            plan.at.insert(start, (literal, events));
        }
        plan.end = match st {
            QState::None => vec![],
            QState::B => vec![Q::CloseB],
            QState::I => vec![Q::CloseI],
            QState::Bi => vec![Q::CloseI, Q::CloseB],
            QState::Ib => vec![Q::CloseB, Q::CloseI],
        };
        plan
    }
}

fn transition(st: QState, run: Run, next: Option<Run>) -> (Vec<Q>, QState) {
    use Q::{CloseB, CloseI, OpenB, OpenI};
    match (run, st) {
        (Run::I, QState::I) => (vec![CloseI], QState::None),
        (Run::I, QState::Bi) => (vec![CloseI], QState::B),
        (Run::I, QState::Ib) => (vec![CloseB, CloseI, OpenB], QState::B),
        (Run::I, QState::None) => (vec![OpenI], QState::I),
        (Run::I, QState::B) => (vec![OpenI], QState::Bi),
        (Run::B, QState::B) => (vec![CloseB], QState::None),
        (Run::B, QState::Bi) => (vec![CloseI, CloseB, OpenI], QState::I),
        (Run::B, QState::Ib) => (vec![CloseB], QState::I),
        (Run::B, QState::None) => (vec![OpenB], QState::B),
        (Run::B, QState::I) => (vec![OpenB], QState::Ib),
        (Run::Bi, QState::B) => (vec![CloseB, OpenI], QState::I),
        (Run::Bi, QState::I) => (vec![CloseI, OpenB], QState::B),
        (Run::Bi, QState::Bi) => (vec![CloseI, CloseB], QState::None),
        (Run::Bi, QState::Ib) => (vec![CloseB, CloseI], QState::None),
        (Run::Bi, QState::None) => match next {
            Some(Run::B | Run::Bi) => (vec![OpenI, OpenB], QState::Ib),
            Some(Run::I) | None => (vec![OpenB, OpenI], QState::Bi),
        },
    }
}

fn collect_quotes(n: &P<'_>, out: &mut Vec<(usize, Run, usize)>, l: &Lowerer<'_, '_>) {
    let run = |start: usize, consumed: usize| l.run_length(start).saturating_sub(consumed);
    match n {
        P::Italic { start, .. } => out.push((*start, Run::I, run(*start, 2))),
        P::Bold { start, .. } => out.push((*start, Run::B, run(*start, 3))),
        P::BoldItalic { start, .. } => out.push((*start, Run::Bi, run(*start, 5))),
        P::Link { text: children, .. }
        | P::ExternalLink {
            nodes: children, ..
        } => {
            for c in children {
                collect_quotes(c, out, l);
            }
        }
        P::Tag { name, nodes, .. } if matches!(name.as_ref(), "noinclude" | "onlyinclude") => {
            for c in nodes {
                collect_quotes(c, out, l);
            }
        }
        _ => {}
    }
}

/// Text nodes split at newlines; `noinclude` and `onlyinclude` replaced by their content
/// and `includeonly` dropped, as a page view does.
fn flatten<'a, 'n>(nodes: &'n [P<'a>], out: &mut Vec<Item<'a, 'n>>) {
    for n in nodes {
        match n {
            P::Text { value, start, .. } => {
                let mut offset = 0;
                for (k, piece) in value.split('\n').enumerate() {
                    if k > 0 {
                        out.push(Item::Text("\n", start + offset - 1));
                    }
                    if !piece.is_empty() {
                        out.push(Item::Text(piece, start + offset));
                    }
                    offset += piece.len() + 1;
                }
            }
            P::Tag { name, nodes, .. } if matches!(name.as_ref(), "noinclude" | "onlyinclude") => {
                flatten(nodes, out);
            }
            P::Tag { name, .. } if name.as_ref() == "includeonly" => {}
            other => out.push(Item::Node(other)),
        }
    }
}

fn join(a: Option<Span>, b: Span) -> Span {
    match a {
        Some(a) => a.start.min(b.start)..a.end.max(b.end),
        None => b,
    }
}
