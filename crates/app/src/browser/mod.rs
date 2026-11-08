//! The browser window: the native frame (tab strip, toolbar, address bar) plus one webview per tab.

mod api;
mod events;
mod extensions;
mod groups;
mod ipc;
mod menus;
mod omnibox;
mod page_forms;
mod page_host;
mod tab_actions;
mod tab_search;
mod updater;
mod lifecycle;

use std::path::PathBuf;
use std::sync::{mpsc, Mutex};
use std::time::Instant;

use serde_json::{json, Value as Json};
use storage::Store;
use tabs::Lifecycle;
use tao::event_loop::EventLoopProxy;
use tao::platform::windows::WindowExtWindows;
use tao::window::Window;
use platform::frame::{Chip, Frame, FrameGroup, FrameState, FrameTab};
use ui::{Action, TabView};
use wry::{NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows};
use wry::dpi::{PhysicalPosition, PhysicalSize};

use crate::{timing, APP};
use crate::jobs::Job;
use crate::msg::Msg;
use crate::pages::{builtin_page, json_reply, random_hex, Reply};
use crate::prefs::Settings;

/// wry's default is `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`.
/// We keep the first two (Edge-only mini menus) but leave SmartScreen protection ON.
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI";

/// Runs in every tab page: reports whether it holds a live camera, microphone or screen-capture
/// track, so Memory Saver never freezes a call. `track.stop()` fires no event, hence the check
/// every few seconds, which only runs once the page has captured something.
pub(crate) const MEDIA_WATCH_JS: &str = include_str!("../../../../assets/media-watch.js");

/// Runs in every tab page, acts only on the Chrome Web Store: the "Add to Kakangkung" button.
const STORE_JS: &str = include_str!("../../../../assets/webstore.js");

/// Run-wide locations.
pub(crate) struct Paths {
    /// Kakangkung's copy of imported extensions.
    pub(crate) extensions: PathBuf,
}

pub(crate) struct Tab {
    pub(crate) view: TabView,
    pub(crate) webview: Option<WebView>,
    pub(crate) hooks: platform::Hooks,
    pub(crate) state: Lifecycle,
    /// When the tab stopped being visible (memory saver clock).
    pub(crate) hidden_since: Option<Instant>,
    /// Addresses left by switching between a native page and a web page: Back returns to them
    /// once the webview has no history of its own.
    pub(crate) before: Vec<String>,
}

impl Tab {
    /// A tab without a webview yet: activating it creates one (restored and background tabs).
    pub(crate) fn unloaded(view: TabView) -> Self {
        Self { view, webview: None, hooks: platform::Hooks::default(), state: Lifecycle::Discarded, hidden_since: None, before: vec![] }
    }
}

pub(crate) struct Browser {
    pub(crate) window: Window,
    /// Tab strip, toolbar and address bar (native, Direct2D).
    pub(crate) frame: Frame,
    /// Profile folder of the tabs; the first tab creates the shared WebView2 environment from it.
    pub(crate) ctx: WebContext,
    pub(crate) env: Option<platform::Environment>,
    /// Browser-process switches, applied when the environment is created.
    pub(crate) browser_args: String,
    /// Finished downloads already seen ("Show downloads when they're done" opens the page for new ones).
    pub(crate) done_downloads: Vec<u64>,
    pub(crate) omnibox: omnibox::Omnibox,
    pub(crate) proxy: EventLoopProxy<Msg>,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: u64,
    pub(crate) next_id: u64,
    pub(crate) closed: Vec<tab_search::ClosedTab>,
    pub(crate) ui_ready: bool,
    /// Native list pop-up above the page: address-bar suggestions and tab search.
    pub(crate) popup: platform::popup::Popup,
    /// Native built-in pages (Settings, ...), shown in place of a webview.
    pub(crate) page: platform::page::Page,
    pub(crate) native: page_host::NativeState,
    /// Where the ⋮ menu was opened (to redraw it in place after zooming).
    pub(crate) menu_at: (f64, f64),
    /// Which pop-up is showing: "suggest", "menu" or "siteinfo".
    pub(crate) overlay_kind: String,
    pub(crate) overlay_open: bool,
    pub(crate) bookmarked: bool,
    /// Search URL template and engine name (Google unless changed in settings).
    pub(crate) search: (String, String),
    /// The saved setting behind `search` (engine id, or a custom template).
    pub(crate) engine_setting: String,
    pub(crate) memory_mode: memory::Mode,
    /// Tab background while a page loads, matching the Windows theme (no white flash).
    pub(crate) page_bg: (u8, u8, u8, u8),
    pub(crate) settings: Settings,
    /// The window is shown once the toolbar has painted (no blank flash at start).
    pub(crate) shown: bool,
    /// Where downloads go, applied to the tabs' profile when its first webview exists.
    pub(crate) downloads_folder: Option<PathBuf>,
    /// Clear browsing data once the first webview exists ("Clear … when you exit").
    pub(crate) clear_at_start: bool,
    /// When the window opened (the New Tab logo bounces in during the first moments).
    pub(crate) opened: Instant,
    /// Automatic updates: where the updater is, and where it keeps downloaded installers.
    pub(crate) update: crate::update::Status,
    pub(crate) update_dir: PathBuf,
    /// Signed-in account name ("" when signed out), shown in the avatar.
    pub(crate) account_name: String,
    /// Latest address-bar request: local rows (once known) and the engine's suggestions.
    pub(crate) suggest_state: (u64, Option<Json>, Vec<String>),
    pub(crate) jobs: mpsc::Sender<Job>,
    pub(crate) token: String,
    pub(crate) paths: Paths,
    pub(crate) synced: bool,
    pub(crate) groups: Vec<ui::Group>,
    pub(crate) next_group: u32,
    /// Split view: (left, right) tab ids shown side by side.
    pub(crate) split: Option<(u64, u64)>,
    /// Tab whose right-click menu is open.
    pub(crate) menu_tab: u64,
    /// An Incognito window: every tab is InPrivate and nothing is recorded.
    pub(crate) incognito: bool,
    /// Which pop-up closed last, and when (see `toggles_closed`).
    pub(crate) overlay_closed: Option<(String, Instant)>,
    /// Self-test only: native menus record their labels here instead of opening.
    pub(crate) menu_probe: Option<Vec<String>>,
    /// Self-test only: windows that would open, as (address, Incognito), instead of starting them.
    pub(crate) window_probe: Option<Vec<(String, bool)>>,
    /// Next id for a pop-up tab (handed out inside the new-window handler).
    pub(crate) next_popup: std::rc::Rc<std::cell::Cell<u64>>,
    /// An extension's toolbar pop-up, open under the extensions button.
    pub(crate) ext_bubble: Option<WebView>,
    /// Where the extensions button is (DIPs: right edge, bottom), the pop-up hangs from it.
    pub(crate) bubble_at: (f64, f64),
}

/// Accessibility focus outline, as a script that switches it on or off in a page.
const FOCUS_JS: &str = include_str!("../../../../assets/focus-highlight.js");

pub(crate) fn focus_script(on: bool) -> String {
    format!("({})({on});", FOCUS_JS.trim().trim_end_matches(';'))
}

/// Page zoom of a tab set to the default zoom from Settings (1.0 = 100 %).
pub(crate) fn set_default_zoom(wv: &WebView, zoom: f64) {
    let _ = platform::zoom(wv, None).and_then(|_| platform::zoom(wv, Some(zoom)));
}

pub(crate) fn chrome_height(window: &Window) -> u32 {
    (ui::CHROME_HEIGHT_DIP * window.scale_factor()).round() as u32
}

pub(crate) fn page_bounds(window: &Window) -> Rect {
    let size = window.inner_size();
    let top = chrome_height(window).min(size.height);
    Rect {
        position: PhysicalPosition::new(0, top as i32).into(),
        size: PhysicalSize::new(size.width, size.height - top).into(),
    }
}

/// Shortcut handler shared by every webview: matches Chrome's keys, forwards the action.
pub(crate) fn key_handler(proxy: &EventLoopProxy<Msg>) -> impl FnMut(platform::Key) -> bool + 'static {
    let p = proxy.clone();
    move |k| match ui::shortcut(k.vk, k.ctrl, k.shift, k.alt) {
        Some(a) => {
            let _ = p.send_event(Msg::Do(a));
            true
        }
        None => false,
    }
}

impl Browser {
    pub(crate) fn new(
        window: Window,
        data_dir: PathBuf,
        proxy: EventLoopProxy<Msg>,
        jobs: mpsc::Sender<Job>,
        paths: Paths,
        extra_args: &str,
    ) -> wry::Result<Self> {
        timing("window created");
        let ctx = WebContext::new(Some(data_dir));
        let dark = platform::dark_mode();
        let page_bg = if dark { (0x1c, 0x1a, 0x21, 255) } else { (255, 255, 255, 255) };
        let commands = proxy.clone();
        let frame = Frame::create(window.hwnd(), move |cmd| {
            let _ = commands.send_event(Msg::Ipc(cmd));
        })
        .map_err(|e| wry::Error::Io(std::io::Error::other(e.to_string())))?;
        let size = window.inner_size();
        frame.set_bounds(0, 0, size.width as i32, chrome_height(&window) as i32);
        let commands = proxy.clone();
        let popup = platform::popup::Popup::create(window.hwnd(), move |cmd| {
            let _ = commands.send_event(Msg::Ipc(cmd));
        })
        .map_err(|e| wry::Error::Io(std::io::Error::other(e.to_string())))?;
        let commands = proxy.clone();
        let page = platform::page::Page::create(window.hwnd(), move |cmd| {
            let _ = commands.send_event(Msg::Ipc(cmd));
        })
        .map_err(|e| wry::Error::Io(std::io::Error::other(e.to_string())))?;
        timing("native frame created");
        let browser_args = format!("{BROWSER_ARGS}{extra_args}");
        Ok(Self {
            window,
            frame,
            ctx,
            env: None,
            browser_args,
            done_downloads: vec![],
            omnibox: omnibox::Omnibox::default(),
            proxy,
            tabs: vec![],
            active: 0,
            next_id: 1,
            closed: vec![],
            ui_ready: false,
            popup,
            page,
            native: page_host::NativeState::default(),
            menu_at: (0.0, 0.0),
            overlay_kind: String::new(),
            overlay_open: false,
            bookmarked: false,
            search: ui::search_template(""),
            engine_setting: String::new(),
            memory_mode: memory::Mode::Balanced,
            page_bg,
            settings: Settings::read(|_| None),
            shown: false,
            downloads_folder: None,
            clear_at_start: false,
            opened: Instant::now(),
            update: crate::update::Status::Idle,
            update_dir: PathBuf::new(),
            account_name: String::new(),
            suggest_state: (0, None, vec![]),
            jobs,
            token: random_hex(),
            paths,
            synced: false,
            groups: vec![],
            next_group: 1,
            split: None,
            menu_tab: 0,
            incognito: false,
            overlay_closed: None,
            menu_probe: None,
            window_probe: None,
            next_popup: std::rc::Rc::new(std::cell::Cell::new(FIRST_POPUP_ID)),
            ext_bubble: None,
            bubble_at: (0.0, 0.0),
        })
    }

    /// What every tab webview is built with (also handed to the pop-up handler).
    fn tab_kit(&self) -> TabKit {
        TabKit {
            proxy: self.proxy.clone(),
            token: self.token.clone(),
            focus_highlight: self.settings.focus_highlight,
            page_bg: self.page_bg,
            theme: self.settings.wry_theme(),
            parent: self.window.hwnd() as isize,
            bounds: page_bounds(&self.window),
            next_popup: self.next_popup.clone(),
        }
    }

    pub(crate) fn build_tab_webview(&mut self, id: u64, url: &str, visible: bool, private: bool) -> wry::Result<WebView> {
        let kit = self.tab_kit();
        let builder = match &self.env {
            Some(env) => WebViewBuilder::new().with_environment(env.clone()),
            None => WebViewBuilder::new_with_web_context(&mut self.ctx)
                .with_additional_browser_args(&self.browser_args)
                .with_browser_extensions_enabled(true),
        };
        let wv = kit.configure(builder, id, private).with_visible(visible).build_as_child(&self.window)?;
        if self.env.is_none() {
            self.env = Some(wv.environment());
        }
        // The identity applies from the first request, so the page loads after it.
        if let Err(e) = platform::present_as_edge(&wv) {
            eprintln!("tab {id}: browser identity: {e}");
        }
        wv.load_url(url)?;
        Ok(wv)
    }
}

thread_local! {
    /// Pop-up webviews built inside the new-window handler, waiting for the UI to adopt them
    /// as tabs (`Msg::Popup`). A webview is not `Send`, so it cannot travel in the message.
    static POPUPS: std::cell::RefCell<Vec<(u64, WebView)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The pop-up webview created for tab `id`, if it is still waiting.
pub(crate) fn take_popup(id: u64) -> Option<WebView> {
    POPUPS.with(|p| {
        let mut p = p.borrow_mut();
        let i = p.iter().position(|(pid, _)| *pid == id)?;
        Some(p.remove(i).1)
    })
}

/// Ids of pop-up tabs start here, apart from the ids the strip hands out.
pub(crate) const FIRST_POPUP_ID: u64 = 1 << 40;

/// Everything a tab webview is configured with; cloned into the new-window handler so a pop-up
/// is built exactly like a tab.
#[derive(Clone)]
pub(crate) struct TabKit {
    proxy: EventLoopProxy<Msg>,
    token: String,
    focus_highlight: bool,
    page_bg: (u8, u8, u8, u8),
    theme: wry::Theme,
    parent: isize,
    bounds: Rect,
    next_popup: std::rc::Rc<std::cell::Cell<u64>>,
}

impl TabKit {
    /// A sized `window.open` (sign-in, payment): our own webview instead of WebView2's bare
    /// window, so it keeps `window.opener`, gets the tab handlers and the tab's browser identity
    /// (Google refuses sign-in pop-ups that announce WebView2), and shows as a tab next to its
    /// opener that closes again when the page calls `window.close()`.
    fn popup(&self, opener: u64, private: bool, url: String, features: wry::NewWindowFeatures) -> NewWindowResponse {
        let id = self.next_popup.get();
        self.next_popup.set(id + 1);
        let builder = WebViewBuilder::new().with_environment(features.opener.environment);
        let built = self.configure(builder, id, private).with_visible(false).build_as_child(&platform::ParentWindow(self.parent));
        match built {
            Ok(wv) => {
                if let Err(e) = platform::present_as_edge(&wv) {
                    eprintln!("pop-up: browser identity: {e}");
                }
                let core = wv.webview();
                POPUPS.with(|p| p.borrow_mut().push((id, wv)));
                let _ = self.proxy.send_event(Msg::Popup { id, opener, url });
                NewWindowResponse::Create { webview: core }
            }
            Err(e) => {
                eprintln!("pop-up: {e}");
                NewWindowResponse::Allow
            }
        }
    }

    fn configure<'a>(&self, builder: WebViewBuilder<'a>, id: u64, private: bool) -> WebViewBuilder<'a> {
        let p = self.proxy.clone();
        let (p1, p2, p3, p4) = (p.clone(), p.clone(), p.clone(), p.clone());
        let kit = self.clone();
        builder
            .with_initialization_script(MEDIA_WATCH_JS)
            .with_initialization_script(focus_script(self.focus_highlight))
            .with_initialization_script(STORE_JS)
            // Pages can only send these messages. A page claiming "live" merely keeps itself
            // from being frozen, which it could also do by playing sound; store messages count
            // only from the Chrome Web Store, and installing still asks the person first.
            .with_ipc_handler(move |req| {
                let body = req.body().as_str();
                if let Some(cmd) = body.strip_prefix("kk:store:") {
                    if ui::host(&req.uri().to_string()) == crate::webstore::HOST {
                        let _ = p4.send_event(Msg::Store { tab: id, cmd: cmd.to_string() });
                    }
                    return;
                }
                let live = match body {
                    "kk:media:1" => true,
                    "kk:media:0" => false,
                    _ => return,
                };
                let _ = p4.send_event(Msg::Media { id, live });
            })
            .with_background_color(self.page_bg)
            .with_incognito(private)
            .with_devtools(true)
            .with_theme(self.theme)
            .with_bounds(self.bounds)
            .with_https_scheme(true)
            .with_hotkeys_zoom(true)
            .with_asynchronous_custom_protocol("kakangkung".into(), {
                let token = self.token.clone();
                let proxy = Mutex::new(self.proxy.clone());
                move |_, req, respond| {
                    let path = req.uri().path().to_string();
                    let Some(api) = path.strip_prefix("/api/") else {
                        return respond.respond(builtin_page(&path, &token));
                    };
                    // Only Kakangkung's own pages know the token (CSRF guard for the actions below).
                    let sent = req.headers().get("X-Kakangkung-Token").and_then(|v| v.to_str().ok());
                    if sent != Some(token.as_str()) {
                        return respond.respond(json_reply(json!({ "error": "forbidden" })));
                    }
                    let msg = Msg::Api {
                        path: api.to_string(),
                        query: req.uri().query().unwrap_or("").to_string(),
                        body: String::from_utf8_lossy(req.body()).into_owned(),
                        respond: Reply::Http(respond),
                    };
                    if let Ok(p) = proxy.lock() {
                        let _ = p.send_event(msg);
                    }
                }
            })
            .with_on_page_load_handler(move |ev, url| {
                let done = matches!(ev, PageLoadEvent::Finished);
                let _ = p1.send_event(Msg::Load { id, url, done });
            })
            .with_document_title_changed_handler(move |title| {
                let _ = p2.send_event(Msg::Title { id, title });
            })
            // `window.open` with a size is a pop-up that keeps `window.opener` (sign-in and
            // payment pop-ups); everything else opens as a new tab.
            .with_new_window_req_handler(move |url, features| {
                if features.size.is_some() {
                    return kit.popup(id, private, url, features);
                }
                let _ = p3.send_event(Msg::OpenTab(url));
                NewWindowResponse::Deny
            })
    }
}

impl Browser {

    pub(crate) fn hook_tab(&self, id: u64, wv: &WebView) -> platform::Hooks {
        let (p1, p2, p3) = (self.proxy.clone(), self.proxy.clone(), self.proxy.clone());
        platform::hook(
            wv,
            key_handler(&self.proxy),
            move |uri| {
                let _ = p1.send_event(Msg::Favicon { id, uri });
            },
            move |playing| {
                let _ = p2.send_event(Msg::Audio { id, playing });
            },
            move || {
                let _ = p3.send_event(Msg::DownloadsChanged);
            },
        )
    }

    pub(crate) fn index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.view.id == id)
    }

    pub(crate) fn active_webview(&self) -> Option<&WebView> {
        self.index(self.active).and_then(|i| self.tabs[i].webview.as_ref())
    }

    pub(crate) fn show_window(&mut self) {
        if !self.shown {
            self.shown = true;
            self.window.set_visible(true);
            self.window.set_focus();
            timing("window shown");
        }
    }

    /// Apply changed appearance/privacy settings to every open webview.
    pub(crate) fn apply_settings(&mut self, settings: Settings) {
        let theme = settings.wry_theme();
        platform::motion::set_motion(settings.animations.then_some(settings.animation_speed), settings.smooth_scrolling);
        crate::i18n::set_language(&settings.ui_language);
        platform::menu::set_menu_theme(match settings.theme.as_str() {
            "light" => Some(false),
            "dark" => Some(true),
            _ => None,
        });
        for t in &self.tabs {
            if let Some(wv) = &t.webview {
                let _ = wv.set_theme(theme);
                if settings.protection != self.settings.protection {
                    for e in platform::apply_protection(wv, settings.protection) {
                        eprintln!("settings: {e}");
                    }
                }
            }
        }
        let old = std::mem::replace(&mut self.settings, settings);
        let new = &self.settings;
        for wv in self.tabs.iter().filter_map(|t| t.webview.as_ref()) {
            if new.zoom != old.zoom {
                set_default_zoom(wv, new.zoom);
            }
            if new.focus_highlight != old.focus_highlight {
                let _ = wv.evaluate_script(&focus_script(new.focus_highlight));
            }
        }
        // Features switched off in Settings leave nothing behind.
        if !new.groups {
            self.tabs.iter_mut().for_each(|t| t.view.group = 0);
            self.groups.clear();
        }
        if !self.settings.split_view && self.split.is_some() {
            let a = self.active;
            self.tab_action(&format!("unsplit:{a}"));
        }
        self.draw_native();
        self.render();
    }

    /// A new tab as the settings ask: which page, and where in the strip.
    pub(crate) fn open_new_tab(&mut self) {
        let url = if self.incognito { format!("{}incognito", ui::INTERNAL) } else { self.settings.new_tab_url.clone() };
        let at = if self.settings.new_tab_next { self.index(self.active).map(|i| i + 1) } else { None };
        self.open_tab(&url, at);
        self.focus_omnibox();
    }

    /// "Ask before closing a window with several tabs". True when the window may close.
    pub(crate) fn confirm_quit(&self) -> bool {
        let n = self.tabs.len();
        !self.settings.confirm_close || n < 2 || platform::confirm(self.window.hwnd(), APP, &format!("Close {n} tabs?"))
    }

    /// Re-read whether the active page is bookmarked (for the star).
    pub(crate) fn refresh_bookmark(&self) {
        let Some(url) = self.tabs.iter().find(|t| t.view.id == self.active).map(|t| t.view.url.clone()) else { return };
        let p = self.proxy.clone();
        self.job(Box::new(move |s| {
            let on = s.is_some_and(|s| s.is_bookmarked(&url));
            let _ = p.send_event(Msg::Bookmarked { url, on });
        }));
    }

    pub(crate) fn focus_omnibox(&self) {
        self.frame.focus_omnibox();
    }

    pub(crate) fn navigate(&mut self, text: &str) {
        self.hide_overlay();
        let Some(mut url) = ui::resolve_input(text, &self.search.0) else { return };
        // The Incognito page belongs to an Incognito window: asked for elsewhere, it opens one.
        if crate::native::page_of(&url) == Some("incognito") && !self.incognito {
            if self.settings.incognito {
                self.open_window(ui::NEWTAB_URL, true);
            }
            return;
        }
        if self.settings.https_first && ui::Security::of(&url) == ui::Security::Insecure {
            url = url.replacen("http://", "https://", 1);
        }
        if let Some(i) = self.index(self.active) {
            let (was, now) = (crate::native::page_of(&self.tabs[i].view.url), crate::native::page_of(&url));
            if was.is_some() || now.is_some() {
                let old = std::mem::replace(&mut self.tabs[i].view.url, url.clone());
                if was != now {
                    self.tabs[i].before.push(old);
                }
                self.reload_kind(i);
                self.render();
                return;
            }
            self.tabs[i].view.url = url.clone();
            if let Some(wv) = &self.tabs[i].webview {
                if let Err(e) = wv.load_url(&url) {
                    eprintln!("load_url({url}) failed: {e}");
                }
                let _ = wv.focus();
            }
        }
        self.render();
    }

    pub(crate) fn act(&mut self, a: Action) -> bool {
        let pos = self.index(self.active).unwrap_or(0);
        let n = self.tabs.len();
        match a {
            Action::NewTab => self.open_new_tab(),
            // Incognito opens its own window (like Chrome); inside one it is just a new tab.
            // Incognito always opens its own window, also from an Incognito window (like Chrome).
            Action::NewPrivateTab if self.incognito || self.settings.incognito => self.open_window(ui::NEWTAB_URL, true),
            Action::NewWindow if self.settings.new_window => self.open_window(&self.settings.new_tab_url.clone(), false),
            Action::TabSearch => self.show_tab_search(),
            Action::CloseTab => self.close_tab(self.active),
            Action::ReopenClosed => {
                if let Some(c) = self.closed.pop() {
                    self.open_tab(&c.url, None);
                }
            }
            Action::NextTab if n > 0 => self.activate(self.tabs[(pos + 1) % n].view.id),
            Action::PrevTab if n > 0 => self.activate(self.tabs[(pos + n - 1) % n].view.id),
            Action::SelectTab(k) if n > 0 => self.activate(self.tabs[k.min(n - 1)].view.id),
            Action::FocusOmnibox => self.focus_omnibox(),
            Action::History => self.open_after_active(&format!("{}history", ui::INTERNAL)),
            Action::Reload if self.native.tab == self.active => {
                if let Some(page) = self.index(self.active).and_then(|i| crate::native::page_of(&self.tabs[i].view.url)) {
                    self.refresh_native(page);
                }
            }
            Action::Reload => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.reload();
                }
            }
            Action::Back => {
                let own = self.active_webview().is_some_and(|wv| wv.can_go_back().unwrap_or(false));
                match (own, self.index(self.active)) {
                    (true, _) => {
                        if let Some(wv) = self.active_webview() {
                            let _ = wv.go_back();
                        }
                    }
                    (false, Some(i)) => {
                        if let Some(url) = self.tabs[i].before.pop() {
                            self.tabs[i].view.url = url;
                            self.reload_kind(i);
                            self.render();
                        }
                    }
                    _ => {}
                }
            }
            Action::Forward => {
                if let Some(wv) = self.active_webview() {
                    let _ = wv.go_forward();
                }
            }
            Action::Home => self.navigate(&self.settings.home_url.clone()),
            Action::Bookmarks => self.open_after_active(&format!("{}bookmarks", ui::INTERNAL)),
            Action::Downloads => self.open_after_active(&format!("{}downloads", ui::INTERNAL)),
            Action::SavePage => {
                if let Some(wv) = self.active_webview() {
                    if let Err(e) = platform::save_page_as(wv) {
                        eprintln!("save as: {e}");
                    }
                }
            }
            Action::DevTools => {
                if let Some(wv) = self.active_webview() {
                    wv.open_devtools();
                }
            }
            Action::TaskManager => self.open_after_active(&format!("{}taskmanager", ui::INTERNAL)),
            Action::Fullscreen => {
                let full = self.window.fullscreen().is_some();
                self.window.set_fullscreen((!full).then_some(tao::window::Fullscreen::Borderless(None)));
            }
            Action::Bookmark => {
                if let Some(t) = self.tabs.iter().find(|t| t.view.id == self.active) {
                    let (url, title, on) = (t.view.url.clone(), t.view.title.clone(), !self.bookmarked);
                    let p = self.proxy.clone();
                    self.job(Box::new(move |s| {
                        if let Some(s) = s {
                            match s.set_bookmark(&url, &title, on) {
                                Ok(()) => {
                                    let _ = p.send_event(Msg::Bookmarked { url, on });
                                }
                                Err(e) => eprintln!("bookmark: {e}"),
                            }
                        }
                    }));
                }
            }
            _ => {}
        }
        !self.tabs.is_empty()
    }

    pub(crate) fn render(&self) {
        let (can_back, can_forward) = self
            .active_webview()
            .map(|wv| (wv.can_go_back().unwrap_or(false), wv.can_go_forward().unwrap_or(false)))
            .unwrap_or((false, false));
        let can_back = can_back || self.index(self.active).is_some_and(|i| !self.tabs[i].before.is_empty());
        if let Some(t) = self.tabs.iter().find(|t| t.view.id == self.active) {
            let title = if t.view.title.is_empty() { "New Tab" } else { &t.view.title };
            let mode = if self.incognito { " (Incognito)" } else { "" };
            self.window.set_title(&format!("{title} - {APP}{mode}"));
        }
        if !self.ui_ready {
            return;
        }
        self.frame.set_state(self.frame_state(can_back, can_forward));
    }

    /// Light or dark for the native UI (frame, pop-up), from Settings > Appearance.
    pub(crate) fn dark_ui(&self) -> bool {
        match self.settings.theme.as_str() {
            "dark" => true,
            "light" => false,
            _ => platform::dark_mode(),
        }
    }

    /// What the native frame shows.
    pub(crate) fn frame_state(&self, can_back: bool, can_forward: bool) -> FrameState {
        let set = &self.settings;
        let active = self.tabs.iter().find(|t| t.view.id == self.active).map(|t| &t.view);
        let url = active.map_or("", |v| v.url.as_str());
        let internal = url.starts_with(ui::INTERNAL);
        let group = |id: u32| self.groups.iter().find(|g| g.id == id);
        let hex = |c: &str| u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0);
        let mut groups = vec![];
        let mut last = 0;
        let tabs = self
            .tabs
            .iter()
            .filter(|t| t.view.id == self.active || !group(t.view.group).is_some_and(|g| g.collapsed))
            .map(|t| {
                let v = &t.view;
                if let Some(g) = group(v.group).filter(|g| g.id != last) {
                    groups.push((v.id, FrameGroup { id: g.id, name: g.name.clone(), color: hex(g.color), collapsed: g.collapsed }));
                }
                last = v.group;
                FrameTab {
                    id: v.id,
                    title: v.title.clone(),
                    loading: v.loading,
                    audio: v.audio,
                    muted: v.muted,
                    pinned: v.pinned,
                    private: v.private,
                    discarded: v.discarded,
                    group_color: group(v.group).map_or(0, |g| hex(g.color)),
                    in_split: self.split.is_some_and(|(l, r)| v.id == l || v.id == r),
                    favicon: v.favicon.clone(),
                }
            })
            .collect();
        let dark = self.dark_ui();
        FrameState {
            dark,
            incognito: self.incognito,
            tabs,
            groups,
            active: self.active,
            can_back,
            can_forward,
            loading: active.is_some_and(|v| v.loading),
            display: if set.full_urls && !internal { url.to_string() } else { ui::display_url(url) },
            url: if internal { ui::display_url(url) } else { url.to_string() },
            chip: match ui::Security::of(url) {
                ui::Security::Secure => Chip::Secure,
                ui::Security::Insecure => Chip::Insecure,
                ui::Security::Local => Chip::Local,
                ui::Security::Internal => Chip::Internal,
            },
            bookmarked: self.bookmarked,
            show_star: set.bookmark_star && storage::storable(url),
            show_home: set.home_button,
            show_extensions: set.extensions_button && !self.incognito,
            show_tab_search: set.tab_search_button,
            avatar: self.account_name.trim().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(),
            synced: self.synced,
            maximized: self.window.is_maximized(),
            tab_search_open: self.overlay_open && self.overlay_kind == "tabsearch",
        }
    }

    pub(crate) fn relayout(&mut self) {
        self.hide_overlay();
        let size = self.window.inner_size();
        self.frame.set_bounds(0, 0, size.width as i32, chrome_height(&self.window) as i32);
        for id in self.visible_with(self.active) {
            if let Some(wv) = self.index(id).and_then(|i| self.tabs[i].webview.as_ref()) {
                let _ = wv.set_bounds(self.tab_bounds(id));
            }
            if id == self.native.tab {
                self.place_native(id);
            }
        }
        self.render();
    }

    /// Live COM event registrations (for the leak check in the self-test).
    pub(crate) fn hook_count(&self) -> usize {
        self.tabs.iter().map(|t| t.hooks.count()).sum::<usize>()
    }

    /// Fire-and-forget database write on the storage thread.
    pub(crate) fn db(&self, f: impl FnOnce(&Store) -> Result<(), String> + Send + 'static) {
        self.job(Box::new(move |s| {
            if let Some(s) = s {
                if let Err(e) = f(s) {
                    eprintln!("history database: {e}");
                }
            }
        }));
    }

    pub(crate) fn job(&self, job: Job) {
        if let Err(mpsc::SendError(job)) = self.jobs.send(job) {
            eprintln!("storage thread is gone");
            job(None);
        }
    }

    /// Answer an API request with work done on the storage thread.
    pub(crate) fn db_reply(&self, respond: Reply, f: impl FnOnce(&Store) -> Json + Send + 'static) {
        self.job(Box::new(move |s| {
            respond.send(match s {
                Some(s) => f(s),
                None => json!({ "error": "the history database could not be opened" }),
            })
        }));
    }

    /// Any live tab webview: extensions live in the tabs' profile, not the UI's.
    pub(crate) fn extension_host(&self) -> Option<&WebView> {
        self.active_webview().or_else(|| self.tabs.iter().find_map(|t| t.webview.as_ref()))
    }

    /// Open tabs as stored for "continue where you left off": (URLs as JSON, active index).
    /// Incognito tabs are left out, and the active index counts only the stored tabs.
    pub(crate) fn session(&self) -> (String, String) {
        let kept: Vec<&Tab> = self.tabs.iter().filter(|t| !t.view.private).collect();
        let urls = serde_json::to_string(&kept.iter().map(|t| &t.view.url).collect::<Vec<_>>()).unwrap_or_default();
        let active = kept.iter().position(|t| t.view.id == self.active).unwrap_or(0);
        (urls, active.to_string())
    }

    pub(crate) fn save_session(&self, session: (String, String)) {
        self.job(Box::new(move |s| {
            if let Some(s) = s {
                let _ = s.set_setting("session.tabs", &session.0).and_then(|_| s.set_setting("session.active", &session.1));
            }
        }));
    }

    pub(crate) fn open_tabs(&self) -> Vec<storage::sync::SyncTab> {
        self.tabs
            .iter()
            .filter(|t| !t.view.private)
            .map(|t| storage::sync::SyncTab { url: t.view.url.clone(), title: t.view.title.clone() })
            .collect()
    }
}
