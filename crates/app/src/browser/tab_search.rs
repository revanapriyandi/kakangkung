//! Chrome's tab search (the ⌄ button, Ctrl+Shift+A): open tabs, most recently used first, and
//! recently closed tabs, each with its site and how long ago; type to filter, pick to switch,
//! × to close, a closed one to reopen it.

use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

use crate::browser::Browser;

/// Recently closed tabs listed.
const RECENTLY_CLOSED: usize = 10;

/// A closed tab, for Ctrl+Shift+T and tab search.
#[derive(Debug, Clone)]
pub(crate) struct ClosedTab {
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) at: Instant,
}

/// "3 secs ago", "1 min ago", "2 hours ago".
pub(crate) fn ago(d: Duration) -> String {
    let s = d.as_secs();
    let (n, unit) = match s {
        0..=59 => (s, "sec"),
        60..=3599 => (s / 60, "min"),
        3600..=86_399 => (s / 3600, "hour"),
        _ => (s / 86_400, "day"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// A tab's name: its title, else its address (kakangkung://… for built-in pages).
fn name(title: &str, url: &str) -> String {
    if !title.is_empty() {
        title.to_string()
    } else if url.starts_with(ui::INTERNAL) {
        ui::display_url(url)
    } else {
        url.to_string()
    }
}

/// "github.com • 3 mins ago" (internal pages show as kakangkung://…).
fn site(url: &str, since: Option<Duration>) -> String {
    let host = if url.starts_with(ui::INTERNAL) { ui::display_url(url) } else { ui::host(url).to_string() };
    match since {
        Some(d) => format!("{host} • {}", ago(d)),
        None => host,
    }
}

impl Browser {
    /// Open tabs (most recently used first) then recently closed ones, as pop-up rows.
    pub(crate) fn tab_search_rows(&self) -> Vec<Json> {
        let mut open: Vec<_> = self.tabs.iter().collect();
        // The tab on show first, then the others by how long they have been hidden.
        open.sort_by_key(|t| if t.view.id == self.active { Duration::ZERO } else { t.hidden_since.map_or(Duration::MAX, |h| h.elapsed()) });
        let mut rows = vec![json!({ "header": true, "id": "", "label": "Open Tabs" })];
        for t in open {
            let title = name(&t.view.title, &t.view.url);
            let since = if t.view.id == self.active { Some(Duration::ZERO) } else { t.hidden_since.map(|h| h.elapsed()) };
            let id = t.view.id;
            rows.push(json!({ "id": format!("tab:goto:{id}"), "label": title, "sub": site(&t.view.url, since), "close": format!("tab:close:{id}") }));
        }
        if !self.closed.is_empty() {
            rows.push(json!({ "header": true, "id": "fold:closed", "label": "Recently Closed" }));
            for c in self.closed.iter().rev().take(RECENTLY_CLOSED) {
                let title = name(&c.title, &c.url);
                rows.push(json!({ "id": format!("closed:{}", c.url), "label": title, "sub": site(&c.url, Some(c.at.elapsed())) }));
            }
        }
        rows
    }

    /// Show tab search under its button at the start of the tab strip.
    pub(crate) fn show_tab_search(&mut self) {
        let rows = self.tab_search_rows();
        self.show_menu_rows("tabsearch", 8.0, platform::frame::STRIP_H as f64 - 2.0, 400.0, rows, true);
        self.render();
    }

    /// Reopen the most recent closed tab showing `url` (from tab search).
    pub(crate) fn reopen_closed(&mut self, url: &str) {
        if let Some(i) = self.closed.iter().rposition(|c| c.url == url) {
            let c = self.closed.remove(i);
            self.open_tab(&c.url, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_read_like_chrome() {
        assert_eq!(ago(Duration::from_secs(3)), "3 secs ago");
        assert_eq!(ago(Duration::from_secs(60)), "1 min ago");
        assert_eq!(ago(Duration::from_secs(39 * 60)), "39 mins ago");
        assert_eq!(ago(Duration::from_secs(2 * 3600 + 5)), "2 hours ago");
        assert_eq!(site("https://www.youtube.com/watch", Some(Duration::from_secs(60))), "www.youtube.com • 1 min ago");
    }
}
