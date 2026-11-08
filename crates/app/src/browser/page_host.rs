//! Tabs showing a native built-in page (`crate::native`) instead of a webview: one page window
//! per browser window, placed where the tab's page goes, fed from the same `/api` handlers the
//! HTML pages use.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use tao::platform::windows::WindowExtWindows;
use wry::Rect;

use crate::browser::Browser;
use crate::native::{self, Step};
use crate::pages::{percent_encode, Reply};
use crate::APP;

/// What the native page on show needs besides the page window itself.
#[derive(Debug, Default)]
pub(crate) struct NativeState {
    /// Tab whose page is on show (0 = none), and which page it is.
    pub(crate) tab: u64,
    pub(crate) page: &'static str,
    /// Words searched on a page the browser searches (History).
    pub(crate) query: String,
    /// Latest answer describing the page.
    pub(crate) data: Json,
    /// Site permissions, once asked for on the Settings page.
    pub(crate) permissions: Option<Vec<Json>>,
    /// Message for the next drawing (after an action).
    pub(crate) toast: String,
    /// Section to scroll to once the page is drawn.
    pub(crate) anchor: Option<String>,
    /// When to ask for the page's data again (Task manager every 2 s, Downloads as they move).
    pub(crate) refresh_at: Option<Instant>,
    /// What was typed or switched in a form not sent yet (Account page), by control id.
    pub(crate) inputs: HashMap<String, String>,
}

/// Task manager's refresh period.
const TASKMANAGER_EVERY: Duration = Duration::from_secs(2);
/// The New Tab page drops its logo in when shown this soon after the window opened.
const LOGO_BOUNCE_WINDOW: Duration = Duration::from_secs(3);
/// Downloads redraw at most this often while bytes arrive.
const DOWNLOADS_EVERY: Duration = Duration::from_millis(250);

/// This run's downloads of Incognito windows (`private`) or the others, as the Downloads page
/// reads them.
fn live_downloads(private: bool) -> Vec<Json> {
    platform::downloads()
        .into_iter()
        .filter(|d| d.private == private)
        .map(|d| json!({ "id": d.id.to_string(), "url": d.url, "path": d.path, "received": d.received, "total": d.total, "state": format!("{:?}", d.state), "started": d.started }))
        .collect()
}

impl Browser {
    pub(crate) fn native_page(&self) -> Option<&'static str> {
        self.index(self.native.tab).and_then(|i| native::page_of(&self.tabs[i].view.url))
    }

    pub(crate) fn reply_to(&self, path: &str, done: &str) -> Reply {
        Reply::Native { proxy: self.proxy.clone(), path: path.into(), done: done.into() }
    }

    /// Put the page window where tab `id` is drawn.
    pub(crate) fn place_native(&self, id: u64) {
        let Rect { position, size } = self.tab_bounds(id);
        let scale = self.window.scale_factor();
        let (p, s) = (position.to_physical::<i32>(scale), size.to_physical::<i32>(scale));
        self.page.set_bounds(p.x, p.y, s.width, s.height);
    }

    /// While the first web page gets ready (WebView2 takes most of a second to start), the page
    /// area shows the logo hopping; the tab's webview covers it when it is shown.
    pub(crate) fn show_launch(&mut self) {
        let Rect { position, size } = crate::browser::page_bounds(&self.window);
        let scale = self.window.scale_factor();
        let (p, s) = (position.to_physical::<i32>(scale), size.to_physical::<i32>(scale));
        self.page.set_bounds(p.x, p.y, s.width, s.height);
        let dark = self.dark_ui() || self.incognito;
        self.page.set_doc(native::launch_doc(), dark);
        self.page.set_visible(true);
    }

    /// Tab `id` (a native page) comes on screen.
    pub(crate) fn show_native(&mut self, id: u64) {
        let Some(url) = self.index(id).map(|i| self.tabs[i].view.url.clone()) else { return };
        let Some(page) = native::page_of(&url) else { return };
        if self.native.tab != id || self.native.page != page {
            // Another page: nothing of the previous one carries over.
            self.native.permissions = None;
            self.native.query.clear();
            self.native.inputs.clear();
            self.native.data = Json::Null;
            self.native.page = page;
        }
        self.native.tab = id;
        self.native.anchor = native::anchor(&url).map(String::from);
        self.place_native(id);
        self.page.set_visible(true);
        self.refresh_native(page);
    }

    pub(crate) fn hide_native(&mut self, id: u64) {
        if self.native.tab == id {
            self.page.set_visible(false);
            self.native.tab = 0;
        }
    }

    /// Ask for the page's data again; it is redrawn when the answer arrives.
    pub(crate) fn refresh_native(&mut self, page: &str) {
        self.native.refresh_at = None;
        // Incognito windows list only their own downloads, which are never kept on disk.
        let path = native::data_path(page).filter(|_| !(page == "downloads" && self.incognito));
        let Some(path) = path else {
            self.native.data = if page == "downloads" { json!([]) } else { Json::Null };
            self.draw_native();
            return;
        };
        let reply = self.reply_to(path, "");
        let query = match page {
            "history" => format!("q={}&limit=300", percent_encode(&self.native.query)),
            _ => String::new(),
        };
        self.api(path, &query, String::new(), reply);
    }

    /// Draw the page on show from the data at hand (also after a theme change).
    pub(crate) fn draw_native(&mut self) {
        let Some(page) = self.native_page() else { return };
        if self.native.data.is_null() && native::data_path(page).is_some() {
            return;
        }
        let data = &self.native.data;
        let mut doc = match page {
            "newtab" => native::newtab_doc(&self.search.1),
            "incognito" => native::incognito_doc(),
            "signin" => native::signin_doc(data, &self.native.inputs),
            "sync" => native::sync_doc(data),
            "welcome" => native::welcome_doc(data),
            "settings" => native::settings_doc(&self.native.data, self.native.permissions.as_deref()),
            "history" => native::history_doc(&self.native.data, &self.native.query),
            "bookmarks" => native::bookmarks_doc(&self.native.data),
            "downloads" => native::downloads_doc(&self.downloads_list()),
            "extensions" => native::extensions_doc(&self.native.data),
            "taskmanager" => native::taskmanager_doc(&self.native.data),
            _ => return,
        };
        // Opening the browser on the New Tab page: the logo drops in, bouncing.
        if page == "newtab" && self.opened.elapsed() < LOGO_BOUNCE_WINDOW {
            doc.logo_bounce = platform::page::Bounce::Once;
        }
        doc.toast = std::mem::take(&mut self.native.toast);
        // Incognito pages are always dark, like Chrome's.
        let dark = self.dark_ui() || self.incognito || page == "incognito";
        self.page.set_doc(doc, dark);
        if let Some(a) = self.native.anchor.take() {
            self.page.show_section(&a);
        }
    }

    /// `page\t<kind>\t<id>\t<value>` from the page window.
    pub(crate) fn on_page_command(&mut self, kind: &str, id: &str, value: &str) {
        let Some(page) = self.native_page() else { return };
        match native::step(page, kind, id, value) {
            Some(Step::Call { path, body, done, confirm }) => {
                if confirm.is_some_and(|q| !platform::confirm(self.window.hwnd(), APP, q)) {
                    return;
                }
                let reply = self.reply_to(&path, &done);
                self.api(&path, "", body, reply);
            }
            Some(Step::Load(path)) => {
                let reply = self.reply_to(path, "");
                self.api(path, "", String::new(), reply);
            }
            Some(Step::Open(target)) => self.navigate(&format!("{}{target}", ui::INTERNAL)),
            Some(Step::Visit(url)) => self.navigate(&url),
            Some(Step::Search(words)) => {
                self.native.query = words;
                self.refresh_native(page);
            }
            Some(Step::Download { id, what }) => {
                self.download_step(id, &what);
                self.refresh_native(page);
            }
            Some(Step::Section(section)) => {
                if let Some(i) = self.index(self.native.tab) {
                    self.tabs[i].view.url = format!("{}{page}/{section}", ui::INTERNAL);
                    self.render();
                }
            }
            Some(Step::FocusOmnibox) => self.focus_omnibox(),
            Some(form @ (Step::Input { .. } | Step::SyncType { .. } | Step::SignIn | Step::Keep { .. })) => self.form_step(form),
            None => eprintln!("page: unknown control {kind} {id}"),
        }
    }

    /// An API answer for the native page.
    pub(crate) fn on_page_reply(&mut self, path: &str, done: &str, json: Json) {
        let Some(page) = self.native_page() else { return };
        if Some(path) == native::data_path(page) {
            self.native.data = json;
            self.draw_native();
            if page == "taskmanager" {
                self.native.refresh_at = Some(Instant::now() + TASKMANAGER_EVERY);
            }
            return;
        }
        let error = json["error"].as_str().map(String::from);
        if path == "permissions" {
            match error {
                Some(e) => self.native.toast = e,
                None => self.native.permissions = Some(json.as_array().cloned().unwrap_or_default()),
            }
            self.draw_native();
            return;
        }
        if error.is_none() && path.starts_with("account/") {
            // The form went through: what was typed is the account now.
            self.native.inputs.clear();
        }
        self.native.toast = match error {
            Some(e) => e,
            None if json["cancelled"].as_bool() == Some(true) => String::new(),
            None => json["summary"].as_str().unwrap_or(done).to_string(),
        };
        if path == "permissions/reset" && self.native.permissions.is_some() {
            let reply = self.reply_to("permissions", "");
            self.api("permissions", "", String::new(), reply);
        }
        self.refresh_native(page);
    }

    /// Downloads of this run, then the kept ones, newest first.
    fn downloads_list(&self) -> Json {
        let mut list = live_downloads(self.incognito);
        list.extend(self.native.data.as_array().into_iter().flatten().cloned());
        list.sort_by_key(|d| std::cmp::Reverse(d["started"].as_i64().unwrap_or(0)));
        Json::Array(list)
    }

    /// Take kept downloads off the list on the storage thread: one (`Some(id)`) or all.
    fn forget_kept(&self, id: Option<i64>) {
        self.job(Box::new(move |s| {
            if let Some(Err(e)) = s.map(|s| s.forget_downloads(id)) {
                eprintln!("downloads: {e}");
            }
        }));
    }

    fn download_step(&mut self, id: String, what: &str) {
        use platform::{download_action, DownloadAction, DownloadState};
        let path = self.downloads_list().as_array().into_iter().flatten().find(|d| d["id"] == id.as_str()).and_then(|d| d["path"].as_str()).unwrap_or_default().to_string();
        let kept = id.strip_prefix('s').and_then(|n| n.parse::<i64>().ok());
        let live = id.parse::<u64>().unwrap_or(0);
        let r = match what {
            "pause" => download_action(live, DownloadAction::Pause).map_err(|e| e.message()),
            "resume" => download_action(live, DownloadAction::Resume).map_err(|e| e.message()),
            "cancel" => download_action(live, DownloadAction::Cancel).map_err(|e| e.message()),
            "forget" if kept.is_some() => {
                self.forget_kept(kept);
                Ok(())
            }
            "forget" => download_action(live, DownloadAction::Forget).map_err(|e| e.message()),
            "clear" => {
                let ended = platform::downloads().into_iter().filter(|d| d.private == self.incognito && matches!(d.state, DownloadState::Done | DownloadState::Stopped));
                for d in ended {
                    let _ = download_action(d.id, DownloadAction::Forget);
                }
                if !self.incognito {
                    self.forget_kept(None);
                }
                Ok(())
            }
            // The person asked to open the file they downloaded; Windows opens it with its app.
            "open" if !path.is_empty() => std::process::Command::new("explorer.exe").arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string()),
            "show" if !path.is_empty() => std::process::Command::new("explorer.exe").arg(format!("/select,{path}")).spawn().map(|_| ()).map_err(|e| e.to_string()),
            _ => Ok(()),
        };
        if let Err(e) = r {
            self.native.toast = e;
        }
    }

    /// A download moved on: redraw the Downloads page soon (not for every chunk of bytes).
    pub(crate) fn downloads_changed(&mut self) {
        // Finished ones move to disk so the list survives a restart (Incognito ones stay here).
        let finished: Vec<storage::PastDownload> = platform::take_finished()
            .into_iter()
            .map(|d| storage::PastDownload { id: 0, url: d.url, path: d.path, size: d.received, done: d.state == platform::DownloadState::Done, started: d.started })
            .collect();
        if !finished.is_empty() {
            self.job(Box::new(move |s| {
                if let Some(Err(e)) = s.map(|s| s.add_downloads(&finished)) {
                    eprintln!("downloads: {e}");
                }
            }));
        }
        if self.native.page == "downloads" && self.native.tab != 0 && self.native.refresh_at.is_none() {
            self.native.refresh_at = Some(Instant::now() + DOWNLOADS_EVERY);
        }
    }

    /// Refresh the page on show when its time has come; returns when to look again.
    pub(crate) fn native_tick(&mut self) -> Option<Instant> {
        let at = self.native.refresh_at?;
        if self.native.tab == 0 {
            self.native.refresh_at = None;
            return None;
        }
        if Instant::now() < at {
            return Some(at);
        }
        let page = self.native.page;
        self.refresh_native(page);
        self.native.refresh_at
    }
}
