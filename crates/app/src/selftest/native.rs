//! Self-test of the native built-in pages: Settings opens without a webview, shows every
//! section, its search box filters, and a switch on it saves the setting.

use std::path::PathBuf;

use tao::platform::windows::WindowExtWindows;

use crate::browser::Browser;
use crate::native;

/// First look, once the page's data had time to arrive. Flips "Tab groups" through the page's
/// own command; returns (looks right, detail, the setting before the flip).
pub(super) fn settings_shown(b: &mut Browser) -> (bool, String, bool) {
    if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
        let _ = platform::capture_window_png(b.window.hwnd(), &PathBuf::from(dir).join("settings.png"));
    }
    let doc = b.page.doc();
    let native_tab = b.index(b.active).is_some_and(|i| b.tabs[i].webview.is_none() && native::page_of(&b.tabs[i].view.url) == Some("settings"));
    let visible = b.page.is_visible();
    let rows = doc.sections.iter().flat_map(|s| s.cards.iter().flatten()).count();
    let first = b.page.section();
    let found = b.page.search("smartscreen");
    let cleared = b.page.search("");
    // The left navigation opens a section as its own page; the address follows.
    b.page.show_section("privacy");
    b.on_page_command("nav", "privacy", "");
    let url = b.index(b.active).map(|i| b.tabs[i].view.url.clone()).unwrap_or_default();
    let paged = b.page.section() == "privacy" && url.ends_with("/settings/privacy");
    let before = b.settings.groups;
    b.on_page_command("toggle", "tabs.groups", if before { "0" } else { "1" });
    let ok = native_tab && visible && doc.sections.len() == 15 && doc.nav.len() == 16 && rows > 60 && first == "you" && found == ["privacy"] && cleared == ["you"] && paged;
    let detail = format!(
        "Settings opened without a webview={native_tab}, page window shown={visible}: {} sections, {} navigation entries, {rows} rows; one section per page (first '{first}'); search 'smartscreen' found {found:?} across pages, cleared shows {cleared:?}; Privacy in the navigation -> its own page at {url} ({paged})",
        doc.sections.len(),
        doc.nav.len()
    );
    (ok, detail, before)
}

/// After the switch's API call: the browser applied the new value. Puts it back.
pub(super) fn settings_saved(b: &mut Browser, before: bool) -> (bool, String) {
    if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
        let _ = platform::capture_window_png(b.window.hwnd(), &PathBuf::from(dir).join("settings-privacy.png"));
    }
    let now = b.settings.groups;
    let shown = b
        .page
        .doc()
        .sections
        .iter()
        .flat_map(|s| s.cards.iter().flatten())
        .find_map(|r| match &r.control {
            platform::page::Control::Toggle { id, on } if id == "tabs.groups" => Some(*on),
            _ => None,
        });
    b.on_page_command("toggle", "tabs.groups", if before { "1" } else { "0" });
    (now != before && shown == Some(now), format!("the 'Tab groups' switch saved {before} -> {now}, the redrawn page shows {shown:?}; switched back"))
}

/// The page window shows `page` for the active tab, which has no webview.
fn showing(b: &Browser, page: &str) -> bool {
    b.index(b.active).is_some_and(|i| b.tabs[i].webview.is_none() && native::page_of(&b.tabs[i].view.url) == Some(page)) && b.page.is_visible()
}

/// Pages visited today and yesterday and two bookmarks (reserved example domains), so the
/// lists have something to show. Queued on the storage thread before the pages ask.
pub(super) fn seed_lists(b: &Browser) {
    b.db(|s| {
        let now = storage::now_ms();
        s.add_visit("https://example.com/", "Example Domain", now - 60_000)?;
        s.add_visit("https://www.example.org/docs/start", "Getting started · Example docs", now - 2 * 3_600_000)?;
        s.add_visit("https://example.net/news", "Example News", now - 26 * 3_600_000)?;
        s.set_bookmark("https://example.com/", "Example Domain", true)?;
        s.set_bookmark("https://www.example.org/docs/start", "Getting started · Example docs", true)
    });
}

/// History opened (Ctrl+H): listed by day, then searched by the browser for something absent.
pub(super) fn history_shown(b: &mut Browser) -> (bool, String) {
    if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
        let _ = platform::capture_window_png(b.window.hwnd(), &PathBuf::from(dir).join("history.png"));
    }
    let doc = b.page.doc();
    let shown = showing(b, "history");
    let entries = doc.sections.iter().flat_map(|s| s.cards.iter().flatten()).filter(|r| matches!(r.control, platform::page::Control::Entry { .. })).count();
    let days: Vec<String> = doc.sections.iter().map(|s| s.title.clone()).collect();
    b.on_page_command("search", "", "zz-no-such-page");
    (shown && doc.title == "History" && doc.remote_search && entries >= 3 && doc.sections.len() >= 2, format!("History native={shown}: {entries} pages under {days:?}"))
}

/// The browser's answer to the search replaced the list.
pub(super) fn history_searched(b: &mut Browser) -> (bool, String) {
    let doc = b.page.doc();
    let none = doc.sections.first().is_some_and(|s| s.id == "none" && s.cards[0][0].title.contains("zz-no-such-page"));
    b.on_page_command("search", "", "");
    (none, format!("searching 'zz-no-such-page' answered with no match={none}"))
}

/// Bookmarks opened (Ctrl+Shift+O).
pub(super) fn bookmarks_shown(b: &mut Browser) -> (bool, String) {
    if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
        let _ = platform::capture_window_png(b.window.hwnd(), &PathBuf::from(dir).join("bookmarks.png"));
    }
    let doc = b.page.doc();
    let shown = showing(b, "bookmarks");
    let tabs = b.tabs.len();
    (shown && doc.title == "Bookmarks" && doc.sections.iter().any(|s| s.id == "all" && s.cards[0].len() >= 2), format!("Bookmarks native={shown} ({} sections); {tabs} tabs open", doc.sections.len()))
}

fn shot(b: &Browser, name: &str) {
    if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
        let _ = platform::capture_window_png(b.window.hwnd(), &PathBuf::from(dir).join(name));
    }
}

/// Task manager (Shift+Esc): processes listed, refreshed on its own.
pub(super) fn taskmanager_shown(b: &mut Browser) -> (bool, String) {
    shot(b, "taskmanager.png");
    let doc = b.page.doc();
    let shown = showing(b, "taskmanager");
    let procs = doc.sections.iter().find(|s| s.id == "processes").map_or(0, |s| s.cards[0].len().saturating_sub(1));
    let ticking = b.native.refresh_at.is_some();
    (shown && procs >= 3 && ticking, format!("Task manager native={shown}: {procs} processes, refreshes every 2 s={ticking}"))
}

/// Downloads (Ctrl+J): this run's list and the kept downloads.
pub(super) fn downloads_shown(b: &mut Browser) -> (bool, String) {
    let doc = b.page.doc();
    let shown = showing(b, "downloads");
    (shown && doc.title == "Downloads" && !doc.sections.is_empty(), format!("Downloads (Ctrl+J) native={shown}, {} downloads this run", platform::downloads().len()))
}

/// Extensions: the installed list (or why it is unavailable) and the ways to add one.
pub(super) fn extensions_shown(b: &mut Browser) -> (bool, String) {
    shot(b, "extensions.png");
    let doc = b.page.doc();
    let shown = showing(b, "extensions");
    let ok = shown && doc.sections.iter().any(|s| s.id == "installed") && doc.sections.iter().any(|s| s.id == "add");
    (ok, format!("Extensions native={shown}: {:?}", doc.sections.iter().map(|s| s.title.clone()).collect::<Vec<_>>()))
}

/// The rest of the native pages: (address under `ui::INTERNAL`, page, title it shows).
pub(super) const TOUR: [(&str, &str, &str); 5] =
    [("", "newtab", ""), ("signin", "signin", "Account"), ("sync", "sync", "Sync"), ("welcome", "welcome", "Welcome"), ("incognito", "incognito", "You've gone Incognito")];

pub(super) fn tour_open(b: &mut Browser, k: usize) {
    // The Incognito page only shows in an Incognito window (elsewhere it opens one); the test
    // looks at it as that window would, and never starts a window.
    b.window_probe = Some(vec![]);
    b.incognito = TOUR[k].1 == "incognito";
    b.navigate(&format!("{}{}", ui::INTERNAL, TOUR[k].0));
}

/// Page `k` of the tour is on show, drawn from its data.
pub(super) fn tour_check(b: &mut Browser, k: usize) -> (bool, String) {
    let (_, page, title) = TOUR[k];
    shot(b, &format!("{page}.png"));
    let doc = b.page.doc();
    let shown = showing(b, page);
    b.incognito = false;
    b.window_probe = None;
    (shown && (title.is_empty() || doc.title == title) && (!doc.sections.is_empty() || !doc.fakebox.is_empty()), format!("{page} native={shown} '{}' ({} sections)", doc.title, doc.sections.len()))
}

/// Select the last tab, then click 2 DIPs above the first tab (above its body, below the
/// window's resize edge): that must pick the first tab, like Chrome.
pub(super) fn press_above_first_tab(b: &mut Browser) {
    if let Some(last) = b.tabs.last().map(|t| t.view.id) {
        b.activate(last);
    }
    b.frame.press(60.0, 5.0);
}

pub(super) fn top_click_selected(b: &mut Browser) -> (bool, String) {
    let first = b.tabs.first().map(|t| t.view.id).unwrap_or(0);
    let ok = b.tabs.len() > 1 && b.active == first;
    (ok, format!("a click above the first tab's body selected it={ok} ({} tabs)", b.tabs.len()))
}

/// The ⌄ button at the start of the tab strip opens tab search, laid out like Chrome's.
pub(super) fn press_tab_search(b: &mut Browser) {
    b.frame.press(22.0, 24.0);
}

pub(super) fn tab_search_shown(b: &mut Browser) -> (bool, String) {
    if let Some(dir) = std::env::var_os("KAKANGKUNG_SHOT_DIR") {
        let _ = platform::capture_window_png(b.popup.hwnd(), &PathBuf::from(dir).join("tabsearch.png"));
    }
    let open = b.overlay_open && b.overlay_kind == "tabsearch" && b.popup.is_visible();
    let rows = b.popup.shown_rows();
    let ok = open && rows.first().map(String::as_str) == Some("Open Tabs") && rows.iter().any(|r| r == "Recently Closed") && rows.len() >= b.tabs.len() + 2;
    b.hide_overlay();
    (ok, format!("the ⌄ button opened tab search={open}: {} rows, sections {:?}", rows.len(), rows.iter().filter(|r| *r == "Open Tabs" || *r == "Recently Closed").collect::<Vec<_>>()))
}
