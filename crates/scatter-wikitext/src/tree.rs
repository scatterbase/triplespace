//! The render tree: elements and text with the source spans they came from, and the
//! builder that balances the tags lowering emits.
//!
//! Lowering (the `lower` module) emits a stream of opens, closes and text, as MediaWiki's
//! parser emits a string. The builder applies the HTML tree-construction rules that
//! matter for that stream, as the HTML5 parser MediaWiki tidies its output with would:
//! a block start closes an open paragraph, an end tag with no open element is dropped, a
//! formatting element closed implicitly is reopened before the next inline content, a
//! list item or cell closes the previous one, and table rows get a `tbody`. The tree is
//! well formed by construction, so extraction and the HTML both read a tree.

use crate::html::{escape_attr, escape_text};

/// A byte range in the text that was parsed.
pub type Span = std::ops::Range<usize>;

/// A node of the render tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    /// An element.
    Element(Element),
    /// Text.
    Text(Text),
}

/// An element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    /// The tag name, lowercase.
    pub name: String,
    /// Attributes, in the order written.
    pub attrs: Vec<(String, String)>,
    /// Children.
    pub children: Vec<Node>,
    /// The source the element came from, where it came from one place.
    pub span: Option<Span>,
}

/// Text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    /// The characters, unescaped.
    pub value: String,
    /// The source they came from.
    pub span: Option<Span>,
}

impl Element {
    /// An element with no children.
    #[must_use]
    pub fn new(name: &str, attrs: Vec<(String, String)>, span: Option<Span>) -> Self {
        Self {
            name: name.to_string(),
            attrs,
            children: Vec::new(),
            span,
        }
    }

    /// The value of an attribute.
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Adds a class.
    pub fn add_class(&mut self, class: &str) {
        if let Some(slot) = self.attrs.iter_mut().find(|(n, _)| n == "class") {
            if !slot.1.split_whitespace().any(|c| c == class) {
                if !slot.1.is_empty() {
                    slot.1.push(' ');
                }
                slot.1.push_str(class);
            }
        } else {
            self.attrs.push(("class".into(), class.into()));
        }
    }

    /// Appends a child element and returns it.
    pub fn push(&mut self, e: Element) -> &mut Element {
        self.children.push(Node::Element(e));
        match self.children.last_mut() {
            Some(Node::Element(e)) => e,
            _ => unreachable!("just pushed an element"),
        }
    }

    /// Appends text.
    pub fn push_text(&mut self, s: &str) {
        push_text(&mut self.children, s, None);
    }
}

/// Appends text, joining it to a preceding text node.
fn push_text(children: &mut Vec<Node>, s: &str, span: Option<Span>) {
    if s.is_empty() {
        return;
    }
    if let Some(Node::Text(t)) = children.last_mut() {
        t.value.push_str(s);
        t.span = match (t.span.take(), span) {
            (Some(a), Some(b)) => Some(a.start.min(b.start)..a.end.max(b.end)),
            (a, b) => a.or(b),
        };
        return;
    }
    children.push(Node::Text(Text {
        value: s.to_string(),
        span,
    }));
}

/// Elements HTML writes without an end tag.
pub fn is_void(name: &str) -> bool {
    matches!(
        name,
        "br" | "hr" | "wbr" | "input" | "img" | "col" | "meta" | "link"
    )
}

/// Serializes nodes as HTML.
pub fn to_html(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(t) => escape_text(&t.value, out),
            Node::Element(e) => {
                out.push('<');
                out.push_str(&e.name);
                for (k, v) in &e.attrs {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    escape_attr(v, out);
                    out.push('"');
                }
                if is_void(&e.name) {
                    out.push_str(" />");
                    continue;
                }
                out.push('>');
                to_html(&e.children, out);
                out.push_str("</");
                out.push_str(&e.name);
                out.push('>');
            }
        }
    }
}

/// The text of nodes, without markup.
pub fn text_content(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push_str(&t.value),
            Node::Element(e) => text_content(&e.children, out),
        }
    }
}

/// Elements whose start closes an open paragraph.
fn closes_p(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "center"
            | "details"
            | "dialog"
            | "dir"
            | "div"
            | "dl"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "header"
            | "hgroup"
            | "main"
            | "menu"
            | "nav"
            | "ol"
            | "p"
            | "section"
            | "summary"
            | "table"
            | "ul"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "pre"
            | "hr"
            | "li"
            | "dd"
            | "dt"
    )
}

/// Formatting elements: closed implicitly, they are reopened before the next content.
fn is_formatting(name: &str) -> bool {
    matches!(
        name,
        "a" | "b"
            | "big"
            | "code"
            | "em"
            | "font"
            | "i"
            | "nobr"
            | "s"
            | "small"
            | "strike"
            | "strong"
            | "tt"
            | "u"
    )
}

/// Elements that bound the search for an open element to close.
fn is_scope_boundary(name: &str) -> bool {
    matches!(name, "table" | "td" | "th" | "caption" | "html")
}

fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

/// Elements an `li` search stops at, besides `address`, `div` and `p`.
fn is_special(name: &str) -> bool {
    closes_p(name) && !matches!(name, "address" | "div" | "p")
        || matches!(name, "td" | "th" | "tr" | "table" | "caption" | "tbody")
}

struct Open {
    element: Element,
    /// Opened by the parser rather than written as a tag: an empty one is marked.
    generated: bool,
}

/// Builds a tree from opens, closes and text.
pub struct Builder {
    stack: Vec<Open>,
    /// Formatting elements closed implicitly, to reopen before the next content; `None`
    /// is a marker (a cell or caption boundary).
    active: Vec<Option<Element>>,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    /// A builder whose root is a `div`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stack: vec![Open {
                element: Element::new("div", Vec::new(), None),
                generated: false,
            }],
            active: Vec::new(),
        }
    }

    /// The name of the innermost open element.
    #[must_use]
    pub fn current(&self) -> &str {
        self.stack.last().map_or("", |o| o.element.name.as_str())
    }

    /// Whether an element is open, within the current table or cell.
    #[must_use]
    pub fn in_scope(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    fn find(&self, name: &str) -> Option<usize> {
        for (i, o) in self.stack.iter().enumerate().skip(1).rev() {
            if o.element.name == name {
                return Some(i);
            }
            if is_scope_boundary(&o.element.name) {
                return None;
            }
        }
        None
    }

    /// Opens an element.
    pub fn open(&mut self, name: &str, attrs: Vec<(String, String)>, span: Option<Span>) {
        self.open_inner(name, attrs, span, false);
    }

    /// Opens an element the parser generates (a paragraph, a list item).
    pub fn open_generated(&mut self, name: &str, attrs: Vec<(String, String)>, span: Option<Span>) {
        self.open_inner(name, attrs, span, true);
    }

    fn open_inner(
        &mut self,
        name: &str,
        attrs: Vec<(String, String)>,
        span: Option<Span>,
        generated: bool,
    ) {
        if closes_p(name)
            && let Some(i) = self.find("p")
        {
            self.pop_to(i);
        }
        match name {
            "li" => self.close_item(&["li"]),
            "dd" | "dt" => self.close_item(&["dd", "dt"]),
            n if is_heading(n) && is_heading(self.current()) => self.pop_one(),
            "td" | "th" => {
                if let Some(i) = self.find("td").or_else(|| self.find("th")) {
                    self.pop_to(i);
                    self.clear_to_marker();
                }
                if !self.in_scope("tr") {
                    self.open_inner("tr", Vec::new(), None, true);
                }
            }
            "tr" => {
                if let Some(i) = self.find("tr") {
                    self.pop_to(i);
                }
                if matches!(self.current(), "table") {
                    self.open_inner("tbody", Vec::new(), None, true);
                }
            }
            _ => {}
        }
        if !closes_p(name) && !matches!(name, "td" | "th" | "tr" | "tbody" | "table" | "caption") {
            self.reconstruct();
        }
        self.stack.push(Open {
            element: Element::new(name, attrs, span),
            generated,
        });
        if matches!(name, "td" | "th" | "caption") {
            self.active.push(None);
        }
    }

    /// Closes the open list item an `li` (or `dd`/`dt`) start implies the end of.
    fn close_item(&mut self, names: &[&str]) {
        for i in (1..self.stack.len()).rev() {
            let n = self.stack[i].element.name.as_str();
            if names.contains(&n) {
                self.pop_to(i);
                return;
            }
            if is_special(n) {
                return;
            }
        }
    }

    /// Closes an element, as an end tag would.
    pub fn close(&mut self, name: &str) {
        if name == "p" && self.find("p").is_none() {
            // `</p>` with no open paragraph is an empty one.
            self.open("p", Vec::new(), None);
        }
        if name == "br" {
            self.void("br", Vec::new(), None);
            return;
        }
        let target = if is_heading(name) {
            (1..self.stack.len())
                .rev()
                .take_while(|&i| !is_scope_boundary(&self.stack[i].element.name))
                .find(|&i| is_heading(&self.stack[i].element.name))
        } else {
            self.find(name)
        };
        match target {
            Some(i) => {
                self.pop_to(i);
                // The element itself was closed explicitly: it is not reopened.
                self.remove_active(name);
                if matches!(name, "td" | "th" | "caption") {
                    self.clear_to_marker();
                }
            }
            None => self.remove_active(name),
        }
    }

    /// Inserts a void element.
    pub fn void(&mut self, name: &str, attrs: Vec<(String, String)>, span: Option<Span>) {
        if closes_p(name) {
            if let Some(i) = self.find("p") {
                self.pop_to(i);
            }
        } else {
            self.reconstruct();
        }
        self.append_node(Node::Element(Element::new(name, attrs, span)));
    }

    /// Inserts text.
    pub fn text(&mut self, s: &str, span: Option<Span>) {
        if s.is_empty() {
            return;
        }
        self.reconstruct();
        let top = &mut self.stack.last_mut().expect("root").element;
        push_text(&mut top.children, s, span);
    }

    /// Inserts a finished inline subtree (a chip).
    pub fn inline(&mut self, e: Element) {
        self.reconstruct();
        self.append_node(Node::Element(e));
    }

    /// Inserts a finished block subtree (the TOC, a redirect box).
    pub fn block(&mut self, e: Element) {
        if let Some(i) = self.find("p") {
            self.pop_to(i);
        }
        self.append_node(Node::Element(e));
    }

    fn append_node(&mut self, n: Node) {
        self.stack
            .last_mut()
            .expect("root")
            .element
            .children
            .push(n);
    }

    /// Pops elements down to and including index `i`, remembering implicitly closed
    /// formatting elements above it.
    fn pop_to(&mut self, i: usize) {
        while self.stack.len() > i {
            let top = self.stack.len() - 1;
            if top > i {
                let name = self.stack[top].element.name.clone();
                if is_formatting(&name) {
                    let mut copy = self.stack[top].element.clone();
                    copy.children.clear();
                    copy.span = None;
                    self.active.push(Some(copy));
                }
            }
            self.pop_one();
        }
    }

    fn pop_one(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let Open {
            mut element,
            generated,
        } = self.stack.pop().expect("non-empty");
        if generated
            && element.children.is_empty()
            && matches!(element.name.as_str(), "p" | "li" | "dt" | "dd")
        {
            element.add_class("mw-empty-elt");
        }
        self.append_node(Node::Element(element));
    }

    fn remove_active(&mut self, name: &str) {
        if let Some(pos) = self
            .active
            .iter()
            .rposition(|e| e.as_ref().is_some_and(|e| e.name == name))
        {
            self.active.remove(pos);
        }
    }

    fn clear_to_marker(&mut self) {
        while let Some(e) = self.active.pop() {
            if e.is_none() {
                break;
            }
        }
    }

    /// Reopens implicitly closed formatting elements, outermost first.
    fn reconstruct(&mut self) {
        let start = self
            .active
            .iter()
            .rposition(Option::is_none)
            .map_or(0, |m| m + 1);
        if start >= self.active.len() {
            return;
        }
        let pending: Vec<Element> = self.active.drain(start..).flatten().collect();
        for e in pending {
            self.stack.push(Open {
                element: e,
                generated: false,
            });
        }
    }

    /// Closes everything and returns the root's children.
    #[must_use]
    pub fn finish(mut self) -> Vec<Node> {
        while self.stack.len() > 1 {
            self.pop_one();
        }
        self.stack
            .pop()
            .map(|o| o.element.children)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(b: Builder) -> String {
        let mut s = String::new();
        to_html(&b.finish(), &mut s);
        s
    }

    #[test]
    fn paragraphs_close_before_blocks() {
        let mut b = Builder::new();
        b.open_generated("p", vec![], None);
        b.text("a", None);
        b.open("div", vec![], None);
        b.text("b", None);
        b.close("div");
        b.close("p");
        assert_eq!(html(b), "<p>a</p><div>b</div><p></p>");
    }

    #[test]
    fn formatting_is_reopened() {
        let mut b = Builder::new();
        b.open_generated("p", vec![], None);
        b.open("b", vec![], None);
        b.text("x", None);
        b.close("p");
        b.open_generated("p", vec![], None);
        b.text("y", None);
        b.close("b");
        b.text("z", None);
        b.close("p");
        assert_eq!(html(b), "<p><b>x</b></p><p><b>y</b>z</p>");
    }

    #[test]
    fn stray_end_tags_are_dropped() {
        let mut b = Builder::new();
        b.text("a", None);
        b.close("span");
        b.close("div");
        assert_eq!(html(b), "a");
    }

    #[test]
    fn empty_generated_items_are_marked() {
        let mut b = Builder::new();
        b.open("ul", vec![], None);
        b.open_generated("li", vec![], None);
        b.close("li");
        b.close("ul");
        assert_eq!(html(b), "<ul><li class=\"mw-empty-elt\"></li></ul>");
    }

    #[test]
    fn rows_and_cells() {
        let mut b = Builder::new();
        b.open("table", vec![], None);
        b.open("td", vec![], None);
        b.text("a", None);
        b.open("td", vec![], None);
        b.text("b", None);
        b.close("table");
        assert_eq!(
            html(b),
            "<table><tbody><tr><td>a</td><td>b</td></tr></tbody></table>"
        );
    }
}
