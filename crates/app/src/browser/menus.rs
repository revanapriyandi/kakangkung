//! Pop-ups above the page: ⋮ menu, site info, profile, tab menu, tab search, suggestions.

use std::time::Instant;

use platform::menu::MenuItem;
use platform::popup::PopupRow;
use serde_json::{json, Value as Json};
use storage::Store;
use ui::Action;
use tao::platform::windows::WindowExtWindows;

use crate::browser::Browser;
use crate::msg::Msg;
use crate::profile::{run_sync, sign_out};

/// Rows in the address-bar dropdown.
const MAX_SUGGESTION_ROWS: usize = 8;
/// Row height of the tab-search pop-up in DIPs (platform::popup).
const MENU_ROW: f64 = 32.0;
/// Zoom step of the menu's − / + buttons (Chrome steps 90/100/110 %…; estimated as ×1.1).
const ZOOM_STEP: f64 = 1.1;
/// A press within this time of a pop-up closing itself is the press that closed it.
const POPUP_REOPEN_GUARD: std::time::Duration = std::time::Duration::from_millis(300);
/// Heights of the pop-up list: separator, zoom row, and the list's top + bottom padding.
const SEP_HEIGHT: f64 = 11.0;
const ZOOM_ROW: f64 = 40.0;
const MENU_PADDING: f64 = 12.0;


impl Browser {
    pub(crate) fn hide_overlay(&mut self) {
        if self.overlay_open {
            self.overlay_open = false;
            self.overlay_closed = Some((self.overlay_kind.clone(), Instant::now()));
            self.popup.hide();
            if self.overlay_kind == "tabsearch" {
                self.render();
            }
        }
    }

    /// The button of pop-up `kind` was pressed: true when that should close it. A pop-up that
    /// lost focus because of this very press closed a moment ago; the press must not reopen it.
    pub(crate) fn toggles_closed(&mut self, kind: &str) -> bool {
        if self.overlay_open && self.overlay_kind == kind {
            self.hide_overlay();
            return true;
        }
        matches!(&self.overlay_closed, Some((k, at)) if k == kind && at.elapsed() < POPUP_REOPEN_GUARD)
    }

    /// Show the pop-up list at `x,y,w` (DIPs of the window's client area). `data` holds the rows:
    /// suggestions `{kind,title,display,url}`, or menu rows `{id,label,key}` with `filter`.
    pub(crate) fn show_overlay(&mut self, kind: &str, rect: [f64; 4], data: &str) {
        let data: Json = serde_json::from_str(data).unwrap_or(Json::Null);
        let rows: Vec<PopupRow> = data["rows"]
            .as_array()
            .map(|rows| rows.iter().filter(|r| r.get("sep").is_none()).map(|r| popup_row(kind, r)).collect())
            .unwrap_or_default();
        let selected = data["selected"].as_i64().filter(|s| *s >= 0).map(|s| s as usize);
        let filter = data["filter"].as_bool().unwrap_or(false);
        self.overlay_open = true;
        self.overlay_kind = kind.to_string();
        let dark = self.dark_ui();
        self.popup.show(kind, rect[0] as f32, rect[1] as f32, rect[2] as f32, rows, selected, filter, dark);
    }

    /// What the lock / tune chip says when clicked (like Chrome's page info bubble).
    pub(crate) fn show_site_info(&mut self, left: f64, top: f64) -> Option<String> {
        let t = self.tabs.iter().find(|t| t.view.id == self.active)?;
        let host = ui::host(&t.view.url);
        let (head, detail) = match ui::Security::of(&t.view.url) {
            ui::Security::Secure => ("Connection is secure", "Information you send to this site (passwords, card numbers) is encrypted."),
            ui::Security::Insecure => ("Connection is not secure", "Don't enter passwords or payment details: others on the network could read them."),
            ui::Security::Local => ("Local page", "This page is on your computer or local network."),
            ui::Security::Internal => ("Kakangkung page", "A built-in page of the browser."),
        };
        let mut rows = vec![json!({ "id": "", "label": if host.is_empty() { head.to_string() } else { format!("{host}: {head}") }, "key": "" })];
        rows.push(json!({ "id": "", "label": detail, "key": "" }));
        if t.view.private {
            rows.push(json!({ "id": "", "label": "Incognito: nothing from this tab is saved", "key": "" }));
        }
        let level = ["off", "basic", "balanced", "strict"][self.settings.protection.tracking.min(3) as usize];
        rows.push(json!({ "id": "", "label": format!("Tracking prevention: {level} · Safe Browsing: {}", if self.settings.protection.safe_browsing { "on" } else { "off" }), "key": "" }));
        rows.push(json!({ "sep": true }));
        rows.push(json!({ "id": "privacy", "label": "Privacy and security settings", "key": "" }));
        self.native_popup(rows, left, top, false)
    }

    /// Chrome's profile bubble: who is signed in, sync status, and the account actions.
    pub(crate) fn show_profile(&mut self, right: f64, top: f64) -> Option<String> {
        let mut rows = vec![];
        if self.account_name.is_empty() {
            rows.push(json!({ "id": "", "label": "Not signed in", "key": "" }));
            rows.push(json!({ "id": "", "label": "Sign in to sync your bookmarks, history, open tabs and settings across your PCs.", "key": "" }));
            rows.push(json!({ "sep": true }));
            rows.push(json!({ "id": "signin", "label": "Sign in to Kakangkung…", "key": "", "icon": "sync" }));
        } else {
            rows.push(json!({ "id": "", "label": self.account_name.clone(), "key": "" }));
            rows.push(json!({ "id": "", "label": if self.synced { "Sync is on" } else { "Sync is paused (check the sync folder)" }, "key": "" }));
            rows.push(json!({ "sep": true }));
            rows.push(json!({ "id": "syncsettings", "label": "Manage what you sync", "key": "", "icon": "settings" }));
            rows.push(json!({ "id": "devices", "label": "Tabs from other devices", "key": "", "icon": "newtab" }));
            rows.push(json!({ "id": "syncnow", "label": "Sync now", "key": "", "icon": "sync" }));
            rows.push(json!({ "id": "signout", "label": "Sign out", "key": "", "icon": "exit" }));
        }
        rows.push(json!({ "sep": true }));
        rows.push(json!({ "id": "incognito", "label": "Open Incognito window", "key": "Ctrl+Shift+N", "icon": "incognito" }));
        self.native_popup(rows, right, top, true)
    }

    /// The ⋮ menu: the everyday commands; page tools under More tools (like Chrome). Returns the
    /// chosen command.
    pub(crate) fn show_menu(&mut self, right: f64, top: f64) -> Option<String> {
        self.menu_at = (right, top);
        let zoom = self.active_webview().and_then(|wv| platform::zoom(wv, Some(1.0)).ok()).unwrap_or(1.0);
        let split = if self.split.is_some_and(|(l, r)| self.active == l || self.active == r) {
            json!({ "id": "unsplit", "label": "Close split view", "key": "" })
        } else {
            json!({ "id": format!("tab:splitnew:{}", self.active), "label": "New split view", "key": "" })
        };
        let more_tools = json!([
            { "id": "tabsearch", "label": "Search tabs", "key": "Ctrl+Shift+A" },
            split,
            { "id": "pip", "label": "Picture in picture", "key": "" },
            { "id": "reader", "label": "Reading mode", "key": "" },
            { "id": "translate", "label": "Translate page", "key": "" },
            { "sep": true },
            { "id": "save", "label": "Save page as…", "key": "Ctrl+S" },
            { "id": "import", "label": "Import bookmarks and settings…", "key": "" },
            { "id": "sync", "label": "Sync and devices", "key": "" },
            { "sep": true },
            { "id": "taskmanager", "label": "Task manager", "key": "Shift+Esc" },
            { "id": "devtools", "label": "Developer tools", "key": "Ctrl+Shift+I" },
        ]);
        let rows = json!([
            { "id": "newtab", "label": "New tab", "key": "Ctrl+T" },
            { "id": "newwindow", "label": "New window", "key": "Ctrl+N" },
            { "id": "incognito", "label": "New Incognito window", "key": "Ctrl+Shift+N" },
            { "sep": true },
            { "id": "passwords", "label": "Passwords and autofill", "key": "" },
            { "id": "history", "label": "History", "key": "Ctrl+H" },
            { "id": "downloads", "label": "Downloads", "key": "Ctrl+J" },
            { "id": "bookmarks", "label": "Bookmarks", "key": "Ctrl+Shift+O" },
            { "id": "extensions", "label": "Extensions", "key": "" },
            { "id": "clear", "label": "Delete browsing data…", "key": "" },
            { "sep": true },
            { "zoom": true, "value": format!("{:.0}%", zoom * 100.0) },
            { "sep": true },
            { "id": "print", "label": "Print…", "key": "Ctrl+P" },
            { "id": "find", "label": "Find…", "key": "Ctrl+F" },
            { "label": "More tools", "sub": more_tools },
            { "sep": true },
            { "id": "settings", "label": "Settings", "key": "" },
            { "id": "about", "label": "Help and about", "key": "" },
            { "id": "exit", "label": "Exit", "key": "" },
        ]);
        let mut rows = rows.as_array().cloned().unwrap_or_default();
        // A downloaded update: offered first, like Chrome's "Update" item.
        if let crate::update::Status::Ready { version, .. } = &self.update {
            rows.splice(0..0, [json!({ "id": "update-restart", "label": format!("Restart to update Kakangkung ({version})"), "key": "" }), json!({ "sep": true })]);
        }
        self.native_popup(rows, right, top, true)
    }

    /// Show menu `rows` as a native Windows menu at the UI point `x, y` (logical px of the tab
    /// strip / toolbar, which sits at the window's origin) and return the chosen command id.
    /// Rows: `{id,label,key}` (no id = information line), `{sep:true}`, `{zoom:true,value}`,
    /// `{label,sub:[rows]}` for a submenu. Features switched off in Settings are left out.
    pub(crate) fn native_popup(&mut self, rows: Vec<Json>, x: f64, y: f64, align_right: bool) -> Option<String> {
        let rows = self.enabled_rows(rows);
        let mut ids = vec![];
        let items = menu_items(&rows, &mut ids);
        if let Some(probe) = &mut self.menu_probe {
            // Self-test: record what would be shown instead of waiting in a modal menu.
            *probe = menu_labels(&items);
            return None;
        }
        let scale = self.window.scale_factor();
        let origin = self.window.inner_position().unwrap_or_default();
        let (sx, sy) = (origin.x + (x * scale).round() as i32, origin.y + (y * scale).round() as i32);
        let chosen = platform::menu::popup(self.window.hwnd(), sx, sy, align_right, &items)?;
        ids.get(chosen as usize - 1).cloned()
    }

    /// Returns false when the window should close.
    pub(crate) fn on_menu(&mut self, id: &str) -> bool {
        let internal = |page: &str| format!("{}{page}", ui::INTERNAL);
        let wv_zoom = |b: &Self, f: Option<f64>| {
            if let Some(wv) = b.active_webview() {
                let _ = platform::zoom(wv, f);
            }
        };
        if let Some(url) = id.strip_prefix("closed:") {
            self.reopen_closed(url);
            return !self.tabs.is_empty();
        }
        if id == "update-restart" {
            return !self.restart_to_update();
        }
        if let Some(what) = id.strip_prefix("tab:") {
            self.tab_action(what);
            return !self.tabs.is_empty();
        }
        match id {
            "newtab" => return self.act(Action::NewTab),
            "newwindow" => return self.act(Action::NewWindow),
            "incognito" => return self.act(Action::NewPrivateTab),
            "tabsearch" => return self.act(Action::TabSearch),
            "pip" => self.picture_in_picture(),
            "translate" => self.translate(),
            "opensettings" => self.open_after_active(&format!("{}settings", ui::INTERNAL)),
            "reader" => self.reading_mode(),
            "unsplit" => {
                let a = self.active;
                self.tab_action(&format!("unsplit:{a}"));
            }
            "signin" => self.open_after_active(&format!("{}signin", ui::INTERNAL)),
            "syncsettings" => self.open_after_active(&format!("{}settings#you", ui::INTERNAL)),
            "devices" => self.open_after_active(&format!("{}sync", ui::INTERNAL)),
            "syncnow" => {
                let (p, tabs) = (self.proxy.clone(), self.open_tabs());
                self.job(Box::new(move |s| {
                    if let Some(s) = s {
                        let _ = p.send_event(Msg::Synced(run_sync(s, tabs, &p).is_ok()));
                    }
                }));
            }
            "signout" => {
                let p = self.proxy.clone();
                self.job(Box::new(move |s| {
                    if let Some(s) = s {
                        if sign_out(s).is_ok() {
                            let _ = p.send_event(Msg::Account(String::new()));
                            let _ = p.send_event(Msg::Synced(false));
                        }
                    }
                }));
            }
            "privacy" => self.open_after_active(&format!("{}settings#privacy", ui::INTERNAL)),
            "history" => return self.act(Action::History),
            "downloads" => return self.act(Action::Downloads),
            "bookmarks" => return self.act(Action::Bookmarks),
            "extensions" | "sync" | "settings" => self.open_after_active(&internal(id)),
            "about" => self.open_after_active(&internal("settings/about")),
            "import" => self.open_after_active(&format!("{}#chrome", internal("welcome"))),
            // Zoom reopens the menu with the new percentage, so several steps are quick (like Chrome).
            "zoomin" | "zoomout" | "zoomreset" => {
                wv_zoom(self, match id { "zoomin" => Some(ZOOM_STEP), "zoomout" => Some(1.0 / ZOOM_STEP), _ => None });
                let (r, t) = self.menu_at;
                if let Some(next) = self.show_menu(r, t) {
                    return self.on_menu(&next);
                }
            }
            "passwords" | "clear" => self.open_after_active(&format!("{}settings#privacy", ui::INTERNAL)),
            "find" => {
                if let Some(wv) = self.active_webview() {
                    if let Err(e) = platform::open_find(wv) {
                        eprintln!("find: {e}");
                    }
                }
            }
            "save" => return self.act(Action::SavePage),
            "taskmanager" => return self.act(Action::TaskManager),
            "devtools" => return self.act(Action::DevTools),
            "print" => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.print();
                }
            }
            "fullscreen" => return self.act(Action::Fullscreen),
            "exit" => return !self.confirm_quit(),
            _ => {}
        }
        true
    }

    /// Local rows (typed text, history, bookmarks) plus the engine's query suggestions, Chrome order:
    /// what you typed, then suggestions, capped at MAX_SUGGESTION_ROWS.
    pub(crate) fn send_suggestions(&mut self) {
        let (seq, Some(local), remote) = &self.suggest_state.clone() else { return };
        let mut v = local.clone();
        if let Some(rows) = v["rows"].as_array_mut() {
            let typed = rows.first().and_then(|r| r["display"].as_str()).unwrap_or("").to_lowercase();
            let searches: Vec<Json> = remote
                .iter()
                .filter(|q| q.to_lowercase() != typed)
                .filter_map(|q| {
                    let url = ui::resolve_input(q, &self.search.0)?;
                    Some(json!({ "kind": "search", "title": q, "display": q, "url": url }))
                })
                .collect();
            let at = rows.len().min(1);
            for (i, r) in searches.into_iter().enumerate() {
                rows.insert(at + i, r);
            }
            rows.truncate(MAX_SUGGESTION_ROWS);
        }
        let seq = *seq;
        self.show_suggestions(seq, v);
    }

    /// Menu rows without the features switched off in Settings.
    fn enabled_rows(&self, rows: Vec<Json>) -> Vec<Json> {
        let s = &self.settings;
        let on = |id: &str| match id {
            "newwindow" => s.new_window,
            "incognito" => s.incognito || self.incognito,
            "pip" => s.pip,
            "reader" => s.reader,
            "translate" => s.translate,
            "unsplit" => s.split_view,
            _ if id.starts_with("tab:window:") => s.new_window,
            _ if id.starts_with("tab:split") || id.starts_with("tab:unsplit") => s.split_view,
            _ if id.starts_with("tab:group") || id.starts_with("tab:ungroup") => s.groups,
            _ => true,
        };
        rows.into_iter()
            .filter(|r| on(r["id"].as_str().unwrap_or("")))
            .map(|mut r| {
                if let Some(sub) = r.get("sub").and_then(Json::as_array).cloned() {
                    r["sub"] = Json::Array(self.enabled_rows(sub));
                }
                r
            })
            .collect()
    }

    /// Show a menu-style pop-up and give it the keyboard (`kind` = what `overlay_kind` reports).
    /// Rows are `{id,label,key,icon}`, `{sep:true}` or the zoom row `{zoom:true,value}`.
    pub(crate) fn show_menu_rows(&mut self, kind: &str, left: f64, top: f64, width: f64, rows: Vec<Json>, filter: bool) {
        let count = |key: &str| rows.iter().filter(|r| r.get(key).is_some()).count() as f64;
        let (seps, zooms) = (count("sep"), count("zoom"));
        let items = rows.len() as f64 - seps - zooms;
        let height = items * MENU_ROW + seps * SEP_HEIGHT + zooms * ZOOM_ROW + MENU_PADDING + if filter { MENU_ROW } else { 0.0 };
        let room = self.window.inner_size().to_logical::<f64>(self.window.scale_factor()).height - top - 8.0;
        let height = height.min(room.max(MENU_ROW * 3.0));
        let data = json!({ "rows": rows, "selected": -1, "filter": filter }).to_string();
        self.show_overlay("menu", [left, top, width, height], &data);
        self.overlay_kind = kind.into();
    }

    /// Right-click menu of one tab.
    pub(crate) fn show_tab_menu(&mut self, id: u64, left: f64, top: f64) -> Option<String> {
        let t = self.index(id).map(|i| self.tabs[i].view.clone())?;
        self.menu_tab = id;
        let row = |action: &str, label: String, key: &str| json!({ "id": format!("tab:{action}:{id}"), "label": label, "key": key });
        let mut rows = vec![
            row("newright", "New tab to the right".into(), ""),
            json!({ "sep": true }),
            row("reload", "Reload".into(), "Ctrl+R"),
            row("dup", "Duplicate".into(), ""),
            row("pin", if t.pinned { "Unpin" } else { "Pin" }.into(), ""),
            row("mute", if t.muted { "Unmute site" } else { "Mute site" }.into(), ""),
            json!({ "sep": true }),
            row("group0", "Add tab to new group".into(), ""),
        ];
        for g in self.groups.iter().filter(|g| g.id != t.group) {
            rows.push(json!({ "id": format!("tab:group{}:{id}", g.id), "label": format!("Add tab to group \u{201c}{}\u{201d}", g.name), "key": "" }));
        }
        if t.group != 0 {
            rows.push(row("ungroup", "Remove from group".into(), ""));
        }
        match self.split {
            Some((l, r)) if id == l || id == r => rows.push(row("unsplit", "Close split view".into(), "")),
            _ if id != self.active => rows.push(row("split", "Show side by side with current tab".into(), "")),
            _ => rows.push(row("splitnew", "New split view".into(), "")),
        }
        if !t.private {
            rows.push(row("window", "Move tab to new window".into(), ""));
        }
        rows.push(json!({ "sep": true }));
        rows.push(row("close", "Close".into(), "Ctrl+W"));
        rows.push(row("others", "Close other tabs".into(), ""));
        rows.push(row("right", "Close tabs to the right".into(), ""));
        self.native_popup(rows, left, top, false)
    }
}

/// Rows for the address-bar pop-up: what Enter would do with the typed text, then history and
/// bookmarks. Runs on the storage thread.
pub(crate) fn suggest_rows(s: Option<&Store>, text: &str, engine: &(String, String)) -> Json {
    let Some(go) = ui::resolve_input(text, &engine.0) else { return json!({ "rows": [], "inline": "" }) };
    let searching = go.starts_with(engine.0.split("%s").next().unwrap_or(""));
    let mut rows = vec![if searching {
        json!({ "kind": "search", "title": format!("{} – Search with {}", text.trim(), engine.1), "display": text.trim(), "url": go })
    } else {
        json!({ "kind": "url", "title": "", "display": ui::display_url(&go), "url": go })
    }];
    let found = s.and_then(|s| s.suggest(text, 6).ok()).unwrap_or_default();
    for it in found.items.iter().filter(|it| it.url != go).take(5) {
        rows.push(json!({
            "kind": if it.bookmarked { "bookmark" } else { "history" },
            "title": it.title,
            "display": ui::display_url(&it.url),
            "url": it.url,
        }));
    }
    json!({ "rows": rows, "inline": found.inline })
}

/// Menu rows (see `Browser::native_popup`) as native menu items. `ids[n - 1]` is the command of
/// item id `n`. Separators never lead, trail or repeat.
fn menu_items(rows: &[Json], ids: &mut Vec<String>) -> Vec<MenuItem> {
    let mut out: Vec<MenuItem> = vec![];
    let item = |ids: &mut Vec<String>, id: &str, label: &str, key: &str| {
        ids.push(id.to_string());
        MenuItem::new(ids.len() as u32, label, key)
    };
    for r in rows {
        let label = r["label"].as_str().unwrap_or("");
        let next = if r.get("sep").is_some() {
            if matches!(out.last(), None | Some(MenuItem::Separator)) {
                continue;
            }
            MenuItem::Separator
        } else if r.get("zoom").is_some() {
            let items = vec![
                item(ids, "zoomin", "Zoom in", "Ctrl+Plus"),
                item(ids, "zoomout", "Zoom out", "Ctrl+Minus"),
                item(ids, "zoomreset", "Reset to 100%", "Ctrl+0"),
                MenuItem::Separator,
                item(ids, "fullscreen", "Full screen", "F11"),
            ];
            MenuItem::Submenu { label: format!("Zoom  {}", r["value"].as_str().unwrap_or("")), items }
        } else if let Some(sub) = r.get("sub").and_then(Json::as_array) {
            MenuItem::Submenu { label: label.to_string(), items: menu_items(sub, ids) }
        } else {
            match r["id"].as_str().unwrap_or("") {
                // Information line (site info, profile): shown, not clickable.
                "" => MenuItem::Item { id: u32::MAX, label: label.to_string(), shortcut: String::new(), enabled: false, checked: false },
                id => item(ids, id, label, r["key"].as_str().unwrap_or("")),
            }
        };
        out.push(next);
    }
    while matches!(out.last(), Some(MenuItem::Separator)) {
        out.pop();
    }
    out
}

/// Every label of a menu, submenus as "Parent > Child" (self-test).
fn menu_labels(items: &[MenuItem]) -> Vec<String> {
    let mut out = vec![];
    for i in items {
        match i {
            MenuItem::Item { label, .. } => out.push(label.clone()),
            MenuItem::Submenu { label, items } => out.extend(menu_labels(items).into_iter().map(|c| format!("{label} > {c}"))),
            MenuItem::Separator => {}
        }
    }
    out
}

/// One pop-up row from the menu/suggestion JSON the browser builds.
fn popup_row(kind: &str, r: &Json) -> PopupRow {
    let text = |k: &str| r[k].as_str().unwrap_or("").to_string();
    if kind == "suggest" {
        let (title, display, row_kind) = (text("title"), text("display"), text("kind"));
        let detail = if !title.is_empty() && row_kind != "search" { display.clone() } else { String::new() };
        PopupRow { id: text("url"), kind: row_kind, title: if title.is_empty() { display } else { title }, detail, ..Default::default() }
    } else {
        let kind = if r["header"].as_bool() == Some(true) { "header" } else { "tab" };
        PopupRow { id: text("id"), kind: kind.into(), title: text("label"), detail: text("key"), sub: text("sub"), close: text("close") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_become_native_items_with_ids() {
        let rows = json!([
            { "sep": true },
            { "id": "", "label": "Connection is secure" },
            { "id": "newtab", "label": "New tab", "key": "Ctrl+T" },
            { "sep": true }, { "sep": true },
            { "label": "More tools", "sub": [{ "id": "devtools", "label": "Developer tools", "key": "Ctrl+Shift+I" }] },
            { "zoom": true, "value": "110%" },
            { "sep": true },
        ]);
        let mut ids = vec![];
        let items = menu_items(rows.as_array().unwrap(), &mut ids);
        assert_eq!(ids, ["newtab", "devtools", "zoomin", "zoomout", "zoomreset", "fullscreen"]);
        assert!(matches!(&items[0], MenuItem::Item { enabled: false, .. }), "information line first, no leading separator");
        assert_eq!(items.iter().filter(|i| **i == MenuItem::Separator).count(), 1, "no doubled or trailing separators");
        let labels = menu_labels(&items);
        assert!(labels.contains(&"More tools > Developer tools".to_string()));
        assert!(labels.contains(&"Zoom  110% > Zoom in".to_string()));
    }
}
