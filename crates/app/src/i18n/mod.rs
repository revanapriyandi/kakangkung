//! Interface languages. Text is written in English in the code; [`tr`] looks it up in the
//! table of the chosen language at the point it is shown (built-in pages, menus). To add a
//! language, copy `id.rs`, translate the right-hand side and list it in [`TABLES`] and in the
//! `languages.ui` setting (`prefs.rs`).
//!
//! An entry may hold one `{}`: it then matches any text with that start and end, and the
//! middle is kept. Text with ` · ` between parts is looked up part by part; dates have their
//! day and month names translated.

use std::borrow::Cow;
use std::sync::atomic::{AtomicUsize, Ordering};

use platform::page::{Control, PageDoc};

mod id;

type Table = &'static [(&'static str, &'static str)];

/// Language code and its table (English needs none).
const TABLES: &[(&str, Table)] = &[("id", id::TEXT)];

/// Index into [`TABLES`] + 1; 0 = English.
static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// Use language `code` ("auto": the Windows display language) from now on.
pub(crate) fn set_language(code: &str) {
    let code = if code == "auto" { platform::ui_language() } else { code.to_string() };
    CURRENT.store(TABLES.iter().position(|(c, _)| *c == code).map_or(0, |i| i + 1), Ordering::Relaxed);
}

fn table() -> Option<Table> {
    CURRENT.load(Ordering::Relaxed).checked_sub(1).map(|i| TABLES[i].1)
}

/// `text` in the current language, or as it is when there is no translation.
pub(crate) fn tr(text: &str) -> Cow<'_, str> {
    match table() {
        Some(t) if !text.is_empty() => lookup(t, text),
        _ => Cow::Borrowed(text),
    }
}

fn lookup<'a>(t: Table, text: &'a str) -> Cow<'a, str> {
    // ponytail: linear scans over a few hundred entries, a map if tables grow past thousands.
    if let Some((_, to)) = t.iter().find(|(from, _)| *from == text) {
        return Cow::Borrowed(to);
    }
    for (from, to) in t.iter().filter(|(from, _)| from.contains("{}")) {
        let (head, tail) = from.split_once("{}").unwrap_or_default();
        if text.len() > head.len() + tail.len() && text.starts_with(head) && text.ends_with(tail) {
            return Cow::Owned(to.replacen("{}", &text[head.len()..text.len() - tail.len()], 1));
        }
    }
    if text.contains(" · ") {
        return Cow::Owned(text.split(" · ").map(|part| lookup(t, part)).collect::<Vec<_>>().join(" · "));
    }
    if text.bytes().any(|b| b.is_ascii_digit()) {
        let words: Vec<Cow<str>> = text.split(' ').map(|w| date_word(t, w)).collect();
        if words.iter().any(|w| matches!(w, Cow::Owned(_))) {
            return Cow::Owned(words.join(" "));
        }
    }
    Cow::Borrowed(text)
}

/// A day or month name in a date ("Saturday," "November"), translated.
fn date_word<'a>(t: Table, word: &'a str) -> Cow<'a, str> {
    let bare = word.trim_end_matches(',');
    match t.iter().find(|(from, _)| *from == bare && DATE_WORDS.contains(from)) {
        Some((_, to)) => Cow::Owned(format!("{to}{}", &word[bare.len()..])),
        None => Cow::Borrowed(word),
    }
}

const DATE_WORDS: &[&str] = &[
    "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday", "January", "February", "March",
    "April", "May", "June", "July", "August", "September", "October", "November", "December",
];

/// Translate every piece of text of a built-in page.
pub(crate) fn translate_doc(doc: &mut PageDoc) {
    if table().is_none() {
        return;
    }
    let t = |s: &mut String| *s = tr(s).into_owned();
    for s in [&mut doc.title, &mut doc.search, &mut doc.toast, &mut doc.hero_text, &mut doc.fakebox] {
        t(s);
    }
    for n in &mut doc.nav {
        t(&mut n.label);
    }
    for section in &mut doc.sections {
        t(&mut section.title);
        for row in section.cards.iter_mut().flatten() {
            t(&mut row.title);
            t(&mut row.sub);
            match &mut row.control {
                Control::Select { options, .. } => options.iter_mut().for_each(|(_, label)| t(label)),
                Control::Buttons(buttons) => buttons.iter_mut().for_each(|b| t(&mut b.label)),
                Control::Text { placeholder, button, .. } => {
                    t(placeholder);
                    t(button);
                }
                Control::Value(v) => t(v),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Table = &[("History", "Riwayat"), ("Today", "Hari ini"), ("Saturday", "Sabtu"), ("November", "November"), ("Updated {}", "Diperbarui {}")];

    #[test]
    fn exact_template_parts_and_dates() {
        assert_eq!(lookup(T, "History"), "Riwayat");
        assert_eq!(lookup(T, "Updated 5 min ago"), "Diperbarui 5 min ago");
        assert_eq!(lookup(T, "Today · Saturday, 8 November 2026"), "Hari ini · Sabtu, 8 November 2026");
        assert!(matches!(lookup(T, "github.com"), Cow::Borrowed("github.com")));
    }

    #[test]
    fn indonesian_table_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for (from, _) in id::TEXT {
            assert!(seen.insert(from), "{from} is listed twice");
        }
    }
}
