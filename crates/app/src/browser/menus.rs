//! Pop-ups above the page: ⋮ menu, site info, profile, tab menu, tab search, suggestions.

use std::time::Instant;

use platform::menu::MenuItem;

use crate::i18n::tr;
use platform::popup::PopupRow;
use serde_json::{json, Value as Json};
use storage::Store;
use ui::Action;

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
    pub(crate) fn show_adblock(&mut self, right: f64, top: f64) {
        let url = self.tabs.iter().find(|t| t.view.id == self.active).map(|t| t.view.url.as_str()).unwrap_or("");
        let web = storage::storable(url);
        let allowed = platform::adblock_site_allowed(url);
        let on = self.settings.protection.adblock && !allowed;
        let mut rows = vec![
            json!({ "header": true, "label": "Ad blocker" }),
            json!({ "note": true, "label": if on { "On for this site" } else if allowed { "Paused for this site" } else { "Off" } }),
            json!({ "note": true, "label": format!("{} recorded blocks for this site; {} across all sites this session", platform::blocked_for_site(url), platform::ads_blocked()) }),
        ];
        if web {
            rows.push(json!({ "id": "adblock-site", "label": if allowed { "Resume on this site" } else { "Pause on this site" }, "checked": !allowed }));
        }
        rows.push(json!({ "id": "adblock-global", "label": "Block ads on all sites", "checked": self.settings.protection.adblock }));
        rows.push(json!({ "id": "privacy", "label": "Privacy settings" }));
        rows.push(json!({ "note": true, "label": "EasyList + EasyPrivacy and YouTube player filtering. Some ads and sponsored content may remain." }));
        self.menu_at = (right, top);
        let _ = self.native_popup(rows, right, top, true);
    }

    pub(crate) fn hide_overlay(&mut self) {
        self.previews.hide();
        if self.overlay_open {
            self.overlay_open = false;
            self.overlay_closed = Some((self.overlay_kind.clone(), Instant::now()));
            self.popup.hide();
            self.hide_submenu();
            if self.overlay_kind == "tabsearch" {
                self.render();
            }
            // The install dialog closed without an answer: the extension is not added.
            if self.overlay_kind == "extinstall" && self.pending_install.is_some() {
                self.finish_install(false);
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
            rows.push(json!({ "id": "signin", "label": "Sign in with Google…", "key": "", "icon": "account" }));
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
            { "id": "openfile", "label": "Open file…", "key": "Ctrl+O" },
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
        // Kakangkung's own pop-up, not a Windows menu: the choice comes back later as a pick of
        // the "menu" pop-up (ipc "pick"), which runs `on_menu`. Submenus open in place.
        let (main, subs) = flatten_menu(rows);
        self.menu_main = main.clone();
        self.menu_subs = subs;
        self.menu_place = (x, y, align_right);
        self.show_open_menu(&main);
        None
    }

    /// Show `rows` as the open menu (the main list or a submenu) at the menu's place.
    fn show_open_menu(&mut self, rows: &[Json]) {
        let (x, y, align_right) = self.menu_place;
        let w = MENU_W;
        let items: Vec<PopupRow> = rows.iter().flat_map(menu_popup_rows).collect();
        self.overlay_open = true;
        self.overlay_kind = format!("menu@{x},{y}");
        let dark = self.dark_ui();
        let left = if align_right { (x - w).max(4.0) } else { x };
        self.popup.show("menu", left as f32, y as f32, w as f32, items, None, false, dark);
    }

    /// Open submenu `n` of the open menu beside its row (the menu stays as it is, so where
    /// you are is never lost). It takes the keyboard: ↑ ↓ Enter, ← or Escape back to the menu.
    pub(crate) fn open_submenu(&mut self, n: usize) {
        let Some((_, rows)) = self.menu_subs.get(n).cloned() else { return };
        let Some((left, right, top)) = self.popup.row_box(&format!("submenu:{n}")) else { return };
        let items: Vec<PopupRow> = self.enabled_rows(rows).iter().flat_map(menu_popup_rows).collect();
        let width = self.window.inner_size().width as f64 / self.window.scale_factor();
        let w = MENU_W - 60.0;
        // Beside the menu on the right, or on its left when the window ends there.
        let x = if f64::from(right) - 4.0 + w <= width { f64::from(right) - 4.0 } else { (f64::from(left) - w + 4.0).max(0.0) };
        self.menu_sub = Some(n);
        let dark = self.dark_ui();
        self.subpopup.show("submenu", x as f32, top - 6.0, w as f32, items, None, false, dark);
    }

    pub(crate) fn hide_submenu(&mut self) {
        if self.menu_sub.take().is_some() {
            self.subpopup.hide();
        }
    }

    /// The pointer is on row `id` of the open menu: its submenu opens, any other closes.
    pub(crate) fn menu_hover(&mut self, id: &str) {
        match id.strip_prefix("submenu:").and_then(|n| n.parse::<usize>().ok()) {
            Some(n) if self.menu_sub != Some(n) => self.open_submenu(n),
            Some(_) => {}
            None if !id.is_empty() => {
                self.hide_submenu();
                self.popup.focus();
            }
            None => {}
        }
    }

    /// ← or Escape in the submenu: back to the menu.
    pub(crate) fn menu_back(&mut self) {
        self.hide_submenu();
        self.popup.focus();
    }

    /// Returns false when the window should close.
    pub(crate) fn on_menu(&mut self, id: &str) -> bool {
        let internal = |page: &str| format!("{}{page}", ui::INTERNAL);
        let wv_zoom = |b: &Self, f: Option<f64>| {
            if let Some(wv) = b.active_webview() {
                let _ = platform::zoom(wv, f);
            }
        };
        if let Some(n) = id.strip_prefix("submenu:") {
            match n.parse::<usize>() {
                Ok(n) => self.open_submenu(n),
                Err(_) if self.menu_sub.is_some() => self.menu_back(),
                Err(_) => {
                    let main = self.menu_main.clone();
                    self.show_open_menu(&main);
                }
            }
            return true;
        }
        if let Some(ext) = id.strip_prefix("extbubble:") {
            let (ext, (right, top)) = (ext.to_string(), self.menu_at);
            self.open_bubble(&ext, right, top);
            return true;
        }
        if let Some(ext) = id.strip_prefix("extpin:") {
            self.toggle_pin(ext);
            return true;
        }
        if id == "extstore" {
            self.open_tab(&format!("https://{}/category/extensions", crate::webstore::HOST), None);
            return true;
        }
        if let Some(pin) = id.strip_prefix("unpin:") {
            self.unpin(pin);
            return true;
        }
        if let Some(url) = id.strip_prefix("closed:") {
            self.reopen_closed(url);
            return !self.tabs.is_empty();
        }
        if id == "update-restart" {
            return !self.restart_to_update();
        }
        if let Some(what) = id.strip_prefix("group:") {
            self.group_action(what);
            return !self.tabs.is_empty();
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
            "adblock-site" | "adblock-global" => {
                let global = id == "adblock-global";
                let url = self.tabs.iter().find(|t| t.view.id == self.active).map(|t| t.view.url.clone()).unwrap_or_default();
                let Some(_) = storage::passwords::origin_of(&url).filter(|_| !global) else {
                    if global {
                        let next = !self.settings.protection.adblock;
                        let mut settings = self.settings.clone();
                        settings.protection.adblock = next;
                        self.apply_settings(settings);
                        self.db(move |s| s.set_setting("privacy.adblock", if next { "1" } else { "0" }));
                    }
                    return true;
                };
                let host = platform::adblock_site_key(&url);
                let mut sites: Vec<String> = self.settings.adblock_allowlist.lines().map(str::to_string).collect();
                if platform::adblock_site_allowed(&url) { sites.retain(|s| s != &host); }
                else if sites.len() < 100 { sites.push(host); }
                let value = sites.join("\n");
                self.settings.adblock_allowlist = value.clone();
                platform::configure_adblock_sites(&value);
                self.db(move |s| s.set_setting("privacy.adblock_allowlist", &value));
                if let Some(wv) = self.active_webview() { let _ = wv.reload(); }
            }
            "history" => return self.act(Action::History),
            "downloads" => return self.act(Action::Downloads),
            "bookmarks" => return self.act(Action::Bookmarks),
            "extensions" | "sync" | "settings" => self.open_after_active(&internal(id)),
            "about" => self.open_after_active(&internal("settings/about")),
            "import" => self.open_after_active(&format!("{}#chrome", internal("welcome"))),
            // Zoom reopens its submenu with the new percentage, so several steps are quick (like Chrome).
            "zoomin" | "zoomout" | "zoomreset" => {
                wv_zoom(self, match id { "zoomin" => Some(ZOOM_STEP), "zoomout" => Some(1.0 / ZOOM_STEP), _ => None });
                let (r, t) = self.menu_at;
                let _ = self.show_menu(r, t);
            }
            "passwords" => self.open_after_active(&format!("{}passwords", ui::INTERNAL)),
            "clear" => self.open_after_active(&format!("{}settings#privacy", ui::INTERNAL)),
            "find" => {
                if let Some(wv) = self.active_webview() {
                    if let Err(e) = platform::open_find(wv) {
                        eprintln!("find: {e}");
                    }
                }
            }
            "save" => return self.act(Action::SavePage),
            "openfile" => return self.act(Action::OpenFile),
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
        MenuItem::new(ids.len() as u32, &tr(label), key)
    };
    for r in rows {
        let label = &*tr(r["label"].as_str().unwrap_or(""));
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
            MenuItem::Submenu { label: tr(&format!("Zoom  {}", r["value"].as_str().unwrap_or(""))).into_owned(), items }
        } else if let Some(sub) = r.get("sub").and_then(Json::as_array) {
            MenuItem::Submenu { label: label.to_string(), items: menu_items(sub, ids) }
        } else {
            match r["id"].as_str().unwrap_or("") {
                // Information line (site info, profile): shown, not clickable.
                "" => MenuItem::Item { id: u32::MAX, label: label.to_string(), shortcut: String::new(), enabled: false, checked: false },
                id => match item(ids, id, label, r["key"].as_str().unwrap_or("")) {
                    MenuItem::Item { id, label, shortcut, enabled, .. } if r["checked"] == true => MenuItem::Item { id, label, shortcut, enabled, checked: true },
                    other => other,
                },
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
        let flag = |k: &str| r[k].as_bool() == Some(true);
        let kind = if flag("header") { "header" } else if flag("note") { "note" } else if flag("buttons") { "buttons" } else { "tab" };
        // Headers are ours to translate; tab rows hold page titles.
        let title = if kind == "header" { tr(&text("label")).into_owned() } else { text("label") };
        PopupRow { id: text("id"), kind: kind.into(), title, detail: text("key"), sub: text("sub"), close: text("close") }
    }
}

/// Width of the menus (DIPs).
const MENU_W: f64 = 340.0;
/// Information lines of a menu wrap at about this many characters.
const NOTE_CHARS: usize = 44;

/// A menu's rows for the pop-up: each submenu, and the zoom row, becomes a row that opens it in
/// place (`submenu:<n>`); the submenus come back as (title, rows).
fn flatten_menu(rows: Vec<Json>) -> (Vec<Json>, Vec<(String, Vec<Json>)>) {
    let (mut main, mut subs) = (vec![], vec![]);
    for r in rows {
        let (label, items, icon) = if r.get("zoom").is_some() {
            // Zoom stays in the menu as one row: − 100% + and full screen (Chrome's).
            main.push(json!({ "id": "zoomreset", "zoomrow": true, "label": "Zoom", "key": r["value"] }));
            continue;
        } else if let Some(items) = r.get("sub").and_then(Json::as_array) {
            let label = r["label"].as_str().unwrap_or("").to_string();
            let icon = match label.as_str() {
                "More tools" => "tools",
                "Colour" => "color",
                "Pin to toolbar" => "pin",
                _ => "",
            };
            (label, items.clone(), icon)
        } else {
            main.push(r);
            continue;
        };
        main.push(json!({ "id": format!("submenu:{}", subs.len()), "label": label, "key": "\u{203A}", "icon": icon }));
        subs.push((label, items));
    }
    (main, subs)
}

/// The icon of a menu command (by its id), for rows that name none.
fn menu_icon(id: &str) -> &'static str {
    let action = id.split(':').take(2).collect::<Vec<_>>();
    match action.as_slice() {
        ["newtab"] | ["tab", "newright"] => "newtab",
        ["newwindow"] | ["tab", "window"] | ["fullscreen"] => "window",
        ["incognito"] => "incognito",
        ["passwords"] => "passwords",
        ["history"] => "history",
        ["downloads"] | ["save"] => "download",
        ["openfile"] => "url",
        ["bookmarks"] | ["reader"] => "bookmarks",
        ["extensions"] | ["extbubble", _] => "extension",
        ["clear"] => "delete",
        ["print"] => "print",
        ["find"] | ["tabsearch"] => "find",
        ["settings"] | ["syncsettings"] => "settings",
        ["about"] => "help",
        ["exit"] | ["signout"] => "exit",
        ["update-restart"] => "update",
        ["unsplit"] | ["splitnew"] => "split",
        ["tab", a] if a.starts_with("split") || *a == "unsplit" => "split",
        ["sync"] | ["syncnow"] => "sync",
        ["taskmanager"] | ["devtools"] => "tools",
        ["zoomin"] | ["zoomout"] | ["zoomreset"] => "zoom",
        ["devices"] => "devices",
        ["signin"] => "account",
        ["privacy"] => "lock",
        ["extstore"] => "store",
        ["import"] => "bookmarks",
        ["translate"] | ["pip"] => "window",
        ["tab", "reload"] => "reload",
        ["tab", "dup"] => "copy",
        ["tab", "pin"] => "pin",
        ["tab", "mute"] => "mute",
        ["tab", a] if a.starts_with("group") || *a == "ungroup" => "group",
        ["tab", "close" | "others" | "right"] | ["group", "close"] => "close",
        ["group", "rename"] => "rename",
        ["group", "collapse" | "ungroup"] => "group",
        ["unpin", _] => "unpin",
        _ => "",
    }
}

/// Pop-up rows of one menu row: a divider, a line of text (wrapped), or a command with its
/// icon, shortcut (or the › of a submenu) and a check when it is on.
fn menu_popup_rows(r: &Json) -> Vec<PopupRow> {
    if r.get("sep").is_some() {
        return vec![PopupRow { kind: "sep".into(), ..Default::default() }];
    }
    let id = r["id"].as_str().unwrap_or("");
    let label = tr(r["label"].as_str().unwrap_or("")).into_owned();
    if r["zoomrow"] == true {
        return vec![PopupRow { id: id.into(), kind: "zoom".into(), title: label, detail: r["key"].as_str().unwrap_or("").into(), ..Default::default() }];
    }
    if id.is_empty() {
        return wrap_words(&label, NOTE_CHARS).into_iter().map(|line| PopupRow { kind: "note".into(), title: line, ..Default::default() }).collect();
    }
    let kind = if r["checked"] == true {
        "check".to_string()
    } else {
        r["icon"].as_str().filter(|i| !i.is_empty()).map(str::to_string).unwrap_or_else(|| menu_icon(id).to_string())
    };
    // A two-line row (the profile card): title, then the line under it.
    let sub = r["sub"].as_str().unwrap_or("").to_string();
    vec![PopupRow { id: id.into(), kind, title: label, detail: r["key"].as_str().unwrap_or("").into(), sub, close: String::new() }]
}

/// `text` in lines of at most about `max` characters, broken between words.
fn wrap_words(text: &str, max: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= max => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_flatten_into_pop_up_rows() {
        let rows = vec![
            json!({ "id": "newtab", "label": "New tab", "key": "Ctrl+T" }),
            json!({ "sep": true }),
            json!({ "zoom": true, "value": "110%" }),
            json!({ "label": "More tools", "sub": [{ "id": "devtools", "label": "Developer tools", "key": "Ctrl+Shift+I" }] }),
            json!({ "id": "", "label": "Information you send to this site (passwords, card numbers) is encrypted." }),
        ];
        let (main, subs) = flatten_menu(rows);
        assert_eq!(subs.len(), 1);
        assert_eq!(main[3]["id"], "submenu:0");
        assert_eq!(subs[0].1[0]["id"], "devtools");
        let popup: Vec<PopupRow> = main.iter().flat_map(menu_popup_rows).collect();
        assert_eq!(popup[0].kind, "newtab");
        assert_eq!(popup[1].kind, "sep");
        assert_eq!((popup[2].kind.as_str(), popup[2].detail.as_str()), ("zoom", "110%"), "zoom is a row of its own");
        assert_eq!(popup[3].detail, "\u{203A}");
        assert!(popup.iter().filter(|r| r.kind == "note").count() >= 2, "a long line wraps");
        assert!(popup.iter().filter(|r| r.kind == "note").all(|r| r.title.chars().count() <= NOTE_CHARS));
        assert_eq!(menu_icon("tab:dup:4"), "copy");
        assert_eq!(menu_icon("group:close:2"), "close");
        assert_eq!(menu_icon("tab:group3:9"), "group");
    }

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
