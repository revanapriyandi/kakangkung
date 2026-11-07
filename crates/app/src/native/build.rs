//! Small builders the page descriptions share.

use platform::page::{Button, ButtonStyle, Control, NavItem, Row, Section};

/// Navigation entries: (id, label, icon glyph).
pub(super) fn nav(items: &[(&str, &str, &str)]) -> Vec<NavItem> {
    items.iter().map(|(id, label, icon)| NavItem { id: (*id).into(), label: (*label).into(), icon: (*icon).into() }).collect()
}

/// A section of one card.
pub(super) fn section(id: &str, title: &str, rows: Vec<Row>) -> Section {
    Section { id: id.into(), title: title.into(), keywords: String::new(), cards: vec![rows] }
}

/// Buttons: (id, label, style).
pub(super) fn buttons<S: AsRef<str>>(list: &[(S, &str, ButtonStyle)]) -> Control {
    Control::Buttons(list.iter().map(|(id, label, style)| Button::new(id.as_ref(), label, *style)).collect())
}

/// "A" for "alice": the letter of an avatar.
pub(super) fn initial(name: &str) -> String {
    name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()
}

/// An empty state: an illustration around `glyph`, a title and a line of text, in a section of
/// its own.
pub(super) fn empty(id: &str, title: &str, glyph: &str, heading: &str, text: &str) -> Section {
    section(id, title, vec![Row::new(heading, text, Control::Illustration { glyph: glyph.into() })])
}
