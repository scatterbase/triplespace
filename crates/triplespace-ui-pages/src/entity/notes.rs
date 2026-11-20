//! A tab's endnotes (0003 §5; 19 §4.5): every distinct reference its groups cite,
//! numbered once in reading order, listed at the foot of the tab. A group's rows carry
//! the numbers as markers that link to them; `shapes` draws both.

use std::cell::RefCell;

use scatter_wikibase_model::statement::Reference;
use scatter_wikibase_shape::same_reference;

/// One endnote: the reference, and the properties whose groups cite it, in the order
/// they first do.
#[derive(Debug, Clone)]
pub struct Note {
    /// The reference as its first citation has it.
    pub reference: Reference,
    /// The properties citing it.
    pub cited_by: Vec<String>,
}

/// The endnotes of the tab being drawn. Numbering is by first citation, so a tab is
/// drawn, or primed, group by group in its order before any group's numbers are read.
#[derive(Debug, Default)]
pub struct Endnotes(RefCell<Vec<Note>>);

impl Endnotes {
    /// Forgets every note, for a new tab.
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    /// The number of `r` (from 1), giving it the next one if the tab has not cited it
    /// yet, and noting that `property` cites it.
    pub fn number(&self, r: &Reference, property: &str) -> usize {
        let mut notes = self.0.borrow_mut();
        let i = if let Some(i) = notes.iter().position(|n| same_reference(&n.reference, r)) {
            i
        } else {
            notes.push(Note {
                reference: r.clone(),
                cited_by: Vec::new(),
            });
            notes.len() - 1
        };
        if !notes[i].cited_by.iter().any(|p| p == property) {
            notes[i].cited_by.push(property.to_string());
        }
        i + 1
    }

    /// The notes so far, in number order.
    #[must_use]
    pub fn all(&self) -> Vec<Note> {
        self.0.borrow().clone()
    }
}
