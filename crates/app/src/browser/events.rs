//! Messages arriving on the UI thread (page loads, titles, settings changes, ...).


use serde_json::json;
use tabs::Lifecycle;

use crate::browser::Browser;
use crate::msg::Msg;
use crate::pages::{api_json, ext_json};

/// A page's title for its tab: a file shown on its own (video, audio, PDF) has none, or only
/// its address: its file name then, like Chrome.
fn tab_title(url: &str, title: String) -> String {
    if title.trim().is_empty() || title == url || title == ui::host(url) {
        ui::page_name(url)
    } else {
        title
    }
}

impl Browser {
    /// Returns false when the window should close.
    pub(crate) fn on_msg(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Ipc(m) => return self.on_ipc(&m),
            Msg::PageReply { path, done, json } => self.on_page_reply(&path, &done, json),
            Msg::DownloadsChanged => self.downloads_changed(),
            Msg::WebAppFound { id, url, data } => self.web_app_found(id, &url, &data),
            Msg::Security { id, url, data } => self.security_changed(id, &url, &data),
            Msg::PreviewReady { id, url } => self.preview_document_ready(id, &url),
            Msg::TabThumbnail { id, revision, result } => self.thumbnail_ready(id,revision,result),
            Msg::TabMemory { id, revision, result } => self.preview_memory_ready(id,revision,result),
            Msg::PageMenu { id, cmd, link } => self.on_page_menu(id, cmd, link),
            Msg::Update(status) => self.on_update(status),
            Msg::Api { path, query, body, respond } => self.api(&path, &query, body, respond),
            Msg::ExtAdd { folder, respond } => match self.extension_host() {
                Some(wv) => platform::extension_add(wv, &folder, move |r| respond.send(api_json(r.map(ext_json)))),
                None => respond.send(json!({ "error": "open a tab first" })),
            },
            Msg::Popup { id, opener, url } => self.adopt_popup(id, opener, url),
            Msg::CloseTab(id) => {
                self.close_tab(id);
                return !self.tabs.is_empty();
            }
            Msg::Store { tab, cmd } => self.store_message(tab, &cmd),
            Msg::StoreUnpacked { tab, ext, result } => self.store_unpacked(tab, ext, result),
            Msg::StorePrepared { tab, ext, result } => self.load_prepared_extension(tab, ext, result),
            Msg::StoreInstalled { tab, ext, result } => self.store_installed(tab, ext, result),
            Msg::SiteIcon { host, path } => self.on_site_icon(&host, path),
            Msg::Password { tab, url, what } => self.on_password_message(tab, &url, &what),
            Msg::Logins { tab, origin, logins } => self.offer_logins(tab, &origin, logins),
            Msg::PasswordAsk { tab, cred, update, sequence } => self.accept_password_offer(tab, cred, update, sequence),
            Msg::SavedLogins { origin, n } => self.on_saved_logins(origin, n),
            Msg::PasswordFill { tab, origin, user, pass } => self.fill_login(tab, &origin, &user, &pass),
            Msg::StoreButton { tab, ext, state, text } => self.store_button(tab, &ext, state, &text),
            Msg::ExtMenu { list, x, y } => self.show_extensions_menu(list, x, y),
            Msg::Bubble { close, size } => self.on_bubble(close, size),
            Msg::SyncSoon => return self.on_menu("syncnow"),
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
                self.account_picture.clear();
                self.render();
            }
            Msg::AccountProfile(name, path) => {
                self.account_name = name;
                self.account_picture = platform::load_avatar_file(std::path::Path::new(&path)).unwrap_or_default();
                self.render();
                if self.native.tab != 0 {
                    let page = self.native.page;
                    self.refresh_native(page);
                }
            }
            Msg::BookmarksChanged => self.refresh_bookmark(),
            Msg::MemoryMode(m) => {
                self.memory_mode = m;
                // Leaving a mode that freezes tabs: wake them so nothing stays frozen by surprise.
                if m.default_thresholds().suspend_after.is_none() {
                    for i in 0..self.tabs.len() {
                        if self.tabs[i].state == Lifecycle::Suspended {
                            if let Some(wv) = &self.tabs[i].webview {
                                if let Err(e) = platform::resume(wv) {
                                    eprintln!("tab {}: resume failed: {e}", self.tabs[i].view.id);
                                    continue;
                                }
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
            Msg::OpenExternal(url) => self.open_window(&url, false),
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
                    if !done {
                        self.tabs[i].document_ready = false;
                        self.tabs[i].web_app = None;
                        self.tabs[i].connection = Default::default();
                        self.tabs[i].media_frames.clear();
                        self.tabs[i].revision = self.tabs[i].revision.wrapping_add(1);
                        self.previews.forget(id);
                    }
                    if done && self.visible_with(self.active).contains(&id) {
                        self.previews.capture_due = Some((id, std::time::Instant::now() + std::time::Duration::from_millis(500)));
                    }
                    if done { self.tabs[i].document_ready = true; }
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
                        let t = self.tabs[i].webview.as_ref().and_then(platform::document_title).unwrap_or_default();
                        self.tabs[i].view.title = tab_title(&self.tabs[i].view.url, t);
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
                        let t = self.tabs[i].webview.as_ref().and_then(platform::document_title).filter(|t| !t.is_empty()).unwrap_or(title);
                        tab_title(&self.tabs[i].view.url, t)
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
                    self.apply(i, tabs::Event::ProtectionChanged);
                }
                self.render();
            }
            Msg::Media { id, frame, live } => {
                if let Some(i) = self.index(id) {
                    self.tabs[i].view.media = self.tabs[i].media_frames.set(frame, live);
                    self.apply(i, tabs::Event::ProtectionChanged);
                }
            }
            // TrySuspend can refuse (a page still loading, for one). The tab is not frozen then:
            // back to Idle, tried again one full wait later instead of counted as frozen forever.
            Msg::Suspended { id, ok: false } => {
                if let Some(i) = self.index(id).filter(|&i| self.tabs[i].state == Lifecycle::Suspended) {
                    eprintln!("tab {id}: TrySuspend refused, trying again later");
                    self.tabs[i].state = Lifecycle::Idle;
                    self.tabs[i].hidden_since = Some(std::time::Instant::now());
                }
            }
            Msg::Suspended { .. } | Msg::Script(_) => {}
        }
        true
    }
}
