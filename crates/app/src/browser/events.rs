//! Messages arriving on the UI thread (page loads, titles, settings changes, ...).


use serde_json::json;
use tabs::Lifecycle;

use crate::browser::Browser;
use crate::msg::Msg;
use crate::pages::{api_json, ext_json};

impl Browser {
    /// Returns false when the window should close.
    pub(crate) fn on_msg(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Ipc(m) => return self.on_ipc(&m),
            Msg::PageReply { path, done, json } => self.on_page_reply(&path, &done, json),
            Msg::DownloadsChanged => self.downloads_changed(),
            Msg::Update(status) => self.on_update(status),
            Msg::Api { path, query, body, respond } => self.api(&path, &query, body, respond),
            Msg::ExtAdd { folder, respond } => match self.extension_host() {
                Some(wv) => platform::extension_add(wv, &folder, move |r| respond.send(api_json(r.map(ext_json)))),
                None => respond.send(json!({ "error": "open a tab first" })),
            },
            Msg::Suggest { seq, json } => {
                if seq == self.suggest_state.0 {
                    self.suggest_state.1 = serde_json::from_str(&json).ok();
                    self.send_suggestions();
                }
            }
            Msg::RemoteSuggest { seq, items } => {
                if seq == self.suggest_state.0 && !items.is_empty() {
                    self.suggest_state.2 = items;
                    self.send_suggestions();
                }
            }
            Msg::Bookmarked { url, on } => {
                if self.tabs.iter().any(|t| t.view.id == self.active && t.view.url == url) {
                    self.bookmarked = on;
                    self.render();
                }
            }
            Msg::SettingsChanged(settings) => self.apply_settings(settings),
            Msg::Account(name) => {
                self.account_name = name;
                self.render();
            }
            Msg::BookmarksChanged => self.refresh_bookmark(),
            Msg::MemoryMode(m) => {
                self.memory_mode = m;
                // Leaving a mode that freezes tabs: wake them so nothing stays frozen by surprise.
                if m.default_thresholds().suspend_after.is_none() {
                    for i in 0..self.tabs.len() {
                        if self.tabs[i].state == Lifecycle::Suspended {
                            if let Some(wv) = &self.tabs[i].webview {
                                let _ = platform::resume(wv);
                            }
                            self.tabs[i].state = Lifecycle::Idle;
                        }
                    }
                }
            }
            Msg::Engine(setting) => {
                self.search = ui::search_template(&setting);
                self.engine_setting = setting;
                self.render();
            }
            Msg::Synced(ok) => {
                self.synced = ok;
                self.render();
            }
            Msg::Do(a) => return self.act(a),
            // A link opened in a new tab: selected at once, or left in the background (Settings).
            Msg::OpenTab(url) => {
                let at = self.index(self.active).map_or(self.tabs.len(), |i| i + 1);
                if self.settings.link_focus {
                    self.open_tab(&url, Some(at));
                } else {
                    self.open_tab_background(&url, at);
                }
            }
            Msg::Load { id, url, done } => {
                // A web page linked to a native page: show it natively.
                let native = crate::native::page_of(&url).is_some();
                if let Some(i) = self.index(id).filter(|&i| native && self.tabs[i].webview.is_some()) {
                    let old = std::mem::replace(&mut self.tabs[i].view.url, url);
                    self.tabs[i].before.push(old);
                    if id == self.active {
                        self.reload_kind(i);
                    } else if let Some(wv) = self.tabs[i].webview.take() {
                        platform::unhook(&wv, &mut self.tabs[i].hooks);
                        self.tabs[i].state = Lifecycle::Discarded;
                    }
                    self.render();
                    return !self.tabs.is_empty();
                }
                if let Some(i) = self.index(id) {
                    let v = &mut self.tabs[i].view;
                    v.loading = !done;
                    v.url = url;
                    if !done {
                        v.favicon.clear();
                        // A new document starts without the old page's camera or microphone.
                        v.media = false;
                    }
                    let is_ntp = v.url == ui::NEWTAB_URL;
                    if is_ntp {
                        v.title = "New Tab".into();
                    }
                    if done && !is_ntp {
                        if let Some(t) = self.tabs[i].webview.as_ref().and_then(platform::document_title).filter(|t| !t.is_empty()) {
                            self.tabs[i].view.title = t;
                        }
                    }
                    if done && id == self.active {
                        self.refresh_bookmark();
                    }
                    let v = &mut self.tabs[i].view;
                    if done && !v.private && storage::storable(&v.url) {
                        let (url, title) = (v.url.clone(), v.title.clone());
                        self.db(move |s| s.add_visit(&url, &title, storage::now_ms()));
                    }
                }
                self.render();
            }
            Msg::Title { id, title } => {
                if let Some(i) = self.index(id) {
                    // Use the current document's title: the event may belong to the page just left.
                    let title = if self.tabs[i].view.url == ui::NEWTAB_URL {
                        "New Tab".to_string()
                    } else {
                        self.tabs[i].webview.as_ref().and_then(platform::document_title).filter(|t| !t.is_empty()).unwrap_or(title)
                    };
                    let url = self.tabs[i].view.url.clone();
                    let t = title.clone();
                    self.db(move |s| s.set_title(&url, &t));
                    self.tabs[i].view.title = title;
                }
                self.render();
            }
            Msg::Favicon { id, uri } => {
                if let Some(i) = self.index(id) {
                    self.tabs[i].view.favicon = uri;
                }
                self.render();
            }
            Msg::Audio { id, playing } => {
                if let Some(i) = self.index(id) {
                    self.tabs[i].view.audio = playing;
                }
                self.render();
            }
            Msg::Media { id, live } => {
                if let Some(i) = self.index(id) {
                    self.tabs[i].view.media = live;
                }
            }
            Msg::Suspended { .. } | Msg::Script(_) => {}
        }
        true
    }
}
