//! The Codex builder (0034 §1.4): Codex components as their CSS-only markup, so that a page
//! reads without JavaScript and in either direction, styled by `codex.style.css` and
//! themed by the token values of [`crate::theme`].
//!
//! Each component is an askama template that escapes what it is given; its rendering is
//! therefore safe to place in another template unescaped. Phase 0 has the three the frame
//! uses; Phase 1 adds fields, table, card, tabs, chip and progress.

use askama::Template;

/// What a button does (Codex `action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Action {
    /// A neutral action.
    #[default]
    Default,
    /// The action that moves the reader forward.
    Progressive,
    /// An action that removes or ends something.
    Destructive,
}

/// How loud a button is (Codex `weight`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Weight {
    /// Bordered.
    #[default]
    Normal,
    /// Filled.
    Primary,
    /// Borderless.
    Quiet,
}

/// A Codex button.
#[derive(Debug, Clone, Template)]
#[template(path = "codex/button.html")]
pub struct Button<'a> {
    /// The label.
    pub label: &'a str,
    /// `submit` or `button`.
    pub kind: &'a str,
    /// What it does.
    pub action: Action,
    /// How loud it is.
    pub weight: Weight,
}

impl<'a> Button<'a> {
    /// A neutral submit button.
    #[must_use]
    pub fn submit(label: &'a str) -> Self {
        Self {
            label,
            kind: "submit",
            action: Action::Default,
            weight: Weight::Normal,
        }
    }

    /// The `class` attribute.
    #[must_use]
    pub fn classes(&self) -> String {
        let mut c = String::from("cdx-button");
        match self.action {
            Action::Default => {}
            Action::Progressive => c.push_str(" cdx-button--action-progressive"),
            Action::Destructive => c.push_str(" cdx-button--action-destructive"),
        }
        match self.weight {
            Weight::Normal => {}
            Weight::Primary => c.push_str(" cdx-button--weight-primary"),
            Weight::Quiet => c.push_str(" cdx-button--weight-quiet"),
        }
        c
    }
}

/// A Codex search input with its button, for inside a `GET` form.
#[derive(Debug, Clone, Template)]
#[template(path = "codex/search.html")]
pub struct SearchInput<'a> {
    /// The field's name.
    pub name: &'a str,
    /// What is already in it.
    pub value: &'a str,
    /// The placeholder.
    pub placeholder: &'a str,
    /// The field's accessible name.
    pub label: &'a str,
    /// The button's label.
    pub button: &'a str,
}

/// The kind of a message (Codex `type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageKind {
    /// Information.
    #[default]
    Notice,
    /// Something to be careful of.
    Warning,
    /// Something failed.
    Error,
    /// Something succeeded.
    Success,
}

/// A Codex message, of plain text.
#[derive(Debug, Clone, Template)]
#[template(path = "codex/message.html")]
pub struct Message<'a> {
    /// The kind.
    pub kind: MessageKind,
    /// Inline rather than block.
    pub inline: bool,
    /// The text.
    pub text: &'a str,
}

impl<'a> Message<'a> {
    /// A block message.
    #[must_use]
    pub fn block(kind: MessageKind, text: &'a str) -> Self {
        Self {
            kind,
            inline: false,
            text,
        }
    }

    /// The `class` attribute.
    #[must_use]
    pub fn classes(&self) -> String {
        let mut c = String::from("cdx-message");
        if self.inline {
            c.push_str(" cdx-message--inline");
        }
        c.push_str(match self.kind {
            MessageKind::Notice => "",
            MessageKind::Warning => " cdx-message--warning",
            MessageKind::Error => " cdx-message--error",
            MessageKind::Success => " cdx-message--success",
        });
        c
    }

    /// How a screen reader announces it, as Codex's component does: an error at once, the
    /// rest politely.
    #[must_use]
    pub fn live(&self) -> &'static str {
        match self.kind {
            MessageKind::Error => "role=\"alert\"",
            _ => "aria-live=\"polite\"",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons() {
        let b = Button {
            weight: Weight::Primary,
            action: Action::Progressive,
            ..Button::submit("Save <now>")
        };
        assert_eq!(
            b.render().unwrap(),
            "<button type=\"submit\" class=\"cdx-button cdx-button--action-progressive \
             cdx-button--weight-primary\">Save &#60;now&#62;</button>"
        );
        assert_eq!(Button::submit("Go").classes(), "cdx-button");
    }

    #[test]
    fn messages_escape_and_announce() {
        let m = Message::block(MessageKind::Error, "a & b")
            .render()
            .unwrap();
        assert!(m.contains("class=\"cdx-message cdx-message--error\" role=\"alert\""));
        assert!(m.contains("a &#38; b"));
        let n = Message::block(MessageKind::Notice, "x").render().unwrap();
        assert!(n.contains("class=\"cdx-message\" aria-live=\"polite\""));
    }

    #[test]
    fn search_input_escapes_its_value() {
        let s = SearchInput {
            name: "search",
            value: "\"><script>",
            placeholder: "Search",
            label: "Search",
            button: "Search",
        }
        .render()
        .unwrap();
        assert!(s.contains("value=\"&#34;&#62;&#60;script&#62;\""));
        assert!(s.contains("cdx-search-input__end-button"));
    }
}
