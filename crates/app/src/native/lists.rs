//! History and Bookmarks: lists of pages, described from the `history` and `bookmarks` API
//! answers. History is grouped by day, newest first, and searched by the browser; Bookmarks
//! are searched in the page.

use platform::page::{Control, PageDoc, Row, Section};
use serde_json::Value as Json;

use super::build::{empty, nav};
use super::when::{clock, day_title, local, short_date};

const HISTORY: &str = "\u{E81C}";
const STAR: &str = "\u{E735}";

fn str_of<'a>(v: &'a Json, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

/// The History page for the `history` answer, found with `query`.
pub(crate) fn history_doc(items: &Json, query: &str) -> PageDoc {
    let now = storage::now_ms();
    let (today, yesterday) = (local(now), local(now - 86_400_000));
    let mut sections: Vec<Section> = vec![];
    for it in items.as_array().into_iter().flatten() {
        let url = str_of(it, "url");
        let t = local(it["last"].as_i64().unwrap_or(0));
        let title = day_title(t, today, yesterday);
        if sections.last().is_none_or(|s| s.title != title) {
            sections.push(Section { id: format!("day-{}-{}-{}", t.year, t.month, t.day), title, keywords: String::new(), cards: vec![vec![]] });
        }
        let visits = it["count"].as_i64().unwrap_or(1);
        let mut sub = format!("{} · {}", clock(t), ui::host(url));
        if visits > 1 {
            sub.push_str(&format!(" · {visits} visits"));
        }
        let name = Some(str_of(it, "title")).filter(|t| !t.is_empty()).unwrap_or(url);
        let entry = Control::Entry { id: url.into(), icon: HISTORY.into(), remove: format!("remove:{url}") };
        if let Some(card) = sections.last_mut().and_then(|s| s.cards.last_mut()) {
            card.push(Row::new(name, &sub, entry));
        }
    }
    if sections.is_empty() {
        sections.push(if query.is_empty() {
            empty("none", "", HISTORY, "No history yet", "Pages you visit show up here, grouped by day.")
        } else {
            empty("none", "", "\u{E721}", &format!("No history matches “{query}”"), "Try other words, or part of the address.")
        });
    }
    PageDoc {
        title: "History".into(),
        search: "Search history".into(),
        nav: nav(&[
            ("open:history", "History", HISTORY),
            ("open:sync", "Tabs from other devices", "\u{E737}"),
            ("open:welcome#chrome", "Import from a browser", "\u{E8B5}"),
            ("clear", "Clear browsing data", "\u{E74D}"),
        ]),
        sections,
        nav_on: "open:history".into(),
        remote_search: true,
        ..Default::default()
    }
}

/// The Bookmarks page for the `bookmarks` answer.
pub(crate) fn bookmarks_doc(items: &Json) -> PageDoc {
    let rows: Vec<Row> = items
        .as_array()
        .into_iter()
        .flatten()
        .map(|b| {
            let url = str_of(b, "url");
            let name = Some(str_of(b, "title")).filter(|t| !t.is_empty()).unwrap_or(url);
            let sub = format!("{} · added {}", ui::host(url), short_date(local(b["added"].as_i64().unwrap_or(0))));
            Row::new(name, &sub, Control::Entry { id: url.into(), icon: STAR.into(), remove: format!("unbookmark:{url}") })
        })
        .collect();
    let tip = Row::new("Bookmark the pages you come back to", "Press Ctrl+D or the star in the address bar. Bookmarks show first on the new tab page and in address-bar suggestions.", Control::None);
    let mut sections = vec![Section { id: "tip".into(), cards: vec![vec![tip]], ..Default::default() }];
    sections.push(if rows.is_empty() {
        empty("none", "", STAR, "No bookmarks yet", "Pages you bookmark show up here.")
    } else {
        Section { id: "all".into(), title: format!("All bookmarks ({})", rows.len()), keywords: String::new(), cards: vec![rows] }
    });
    PageDoc {
        title: "Bookmarks".into(),
        search: "Search bookmarks".into(),
        nav: nav(&[("open:bookmarks", "All bookmarks", STAR), ("open:history", "History", HISTORY)]),
        sections,
        nav_on: "open:bookmarks".into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn history_groups_visits_by_day() {
        let now = storage::now_ms();
        let items = json!([
            { "url": "https://a.example/x", "title": "A", "count": 3, "last": now },
            { "url": "https://b.example/", "title": "", "count": 1, "last": now - 1000 },
            { "url": "https://c.example/", "title": "C", "count": 1, "last": now - 3 * 86_400_000 },
        ]);
        let doc = history_doc(&items, "");
        assert_eq!(doc.sections.len(), 2);
        assert!(doc.sections[0].title.starts_with("Today"));
        let first = &doc.sections[0].cards[0];
        assert_eq!(first.len(), 2);
        assert!(first[0].sub.ends_with("a.example · 3 visits"));
        assert_eq!(first[1].title, "https://b.example/");
        assert!(matches!(&first[0].control, Control::Entry { id, remove, .. } if id == "https://a.example/x" && remove == "remove:https://a.example/x"));
        assert!(history_doc(&json!([]), "rust").sections[0].cards[0][0].title.contains("rust"));
    }

    #[test]
    fn bookmarks_list_with_remove() {
        let doc = bookmarks_doc(&json!([{ "url": "https://a.example/", "title": "A", "added": 0 }]));
        let all = doc.sections.iter().find(|s| s.id == "all").expect("list");
        assert!(matches!(&all.cards[0][0].control, Control::Entry { remove, .. } if remove == "unbookmark:https://a.example/"));
        assert!(!doc.paged && !doc.remote_search);
    }
}
