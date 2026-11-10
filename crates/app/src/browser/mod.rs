//! The browser window: the native frame (tab strip, toolbar, address bar) plus one webview per tab.

mod api;
mod site_info;
mod events;
mod devtools;
mod extensions;
mod groups;
mod ipc;
mod menus;
mod omnibox;
mod page_forms;
mod page_host;
mod passwords;
mod tab_actions;
mod tab_search;
mod updater;
mod lifecycle;
mod media;
mod memory_diagnostics;
mod tab_preview;

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
/// The password manager's page side.
const PASSWORDS_JS: &str = include_str!("../../../../assets/passwords.js");
const PREVIEW_READY_JS: &str = r#"(() => {
  if (window.top !== window || !window.ipc) return;
  const ready = () => window.ipc.postMessage('kk:preview-ready');
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', ready, { once: true });
  else ready();
})();"#;

/// Protocol of the one built-in page a webview loads (the self-test's, with its `/api`). wry
/// serves it by answering every request to `https://<protocol>.*` itself, so the name must not
/// begin a real site's host: as "kakangkung" it swallowed kakangkung.rnevio.com, the browser's
/// own website. Native pages (`ui::INTERNAL`) never reach a webview.
// ponytail: a host starting with "kakangkung-internal." would still be swallowed; an own
// WebResourceRequested handler for one exact host if that ever matters.
const PROTOCOL: &str = "kakangkung-internal";

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
    pub(crate) compacted: bool,
    pub(crate) media_frames: media::MediaFrames,
    pub(crate) revision: u64,
    pub(crate) document_ready: bool,
    pub(crate) web_app: Option<crate::webapps::WebApp>,
    pub(crate) connection: site_info::Connection,
    /// Addresses left by switching between a native page and a web page: Back returns to them
    /// once the webview has no history of its own.
    pub(crate) before: Vec<String>,
}

impl Tab {
    /// A tab without a webview yet: activating it creates one (restored and background tabs).
    pub(crate) fn unloaded(view: TabView) -> Self {
        Self { view, webview: None, hooks: platform::Hooks::default(), state: Lifecycle::Discarded, hidden_since: None, compacted: false, media_frames: media::MediaFrames::default(), revision: 0, document_ready: false, web_app: None, connection: Default::default(), before: vec![] }
    }
}

pub(crate) struct Browser {
    pub(crate) window: Window,
    pub(crate) installed_app: Option<crate::webapps::WebApp>,
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
    /// A menu's submenu, opened beside it.
    pub(crate) subpopup: platform::popup::Popup,
    /// Which submenu of the open menu is shown beside it.
    pub(crate) menu_sub: Option<usize>,
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
    pub(crate) previews: tab_preview::Previews,
    pub(crate) compact_idle: bool,
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
    pub(crate) account_picture: String,
    pub(crate) devtools: Option<platform::DockedDevTools>,
    pub(crate) devtools_tab: u64,
    pub(crate) devtools_ready: bool,
    /// Where DevTools is docked ("bottom", "right", "left"), the share of the page area it takes,
    /// and its button rail and splitter (made the first time it opens).
    pub(crate) devtools_side: &'static str,
    pub(crate) devtools_share: f32,
    pub(crate) devtools_dock: Option<platform::DockControls>,
    /// Latest address-bar request: local rows (once known) and the engine's suggestions.
    pub(crate) suggest_state: (u64, Option<Json>, Vec<String>),
    pub(crate) jobs: mpsc::Sender<Job>,
    /// A second storage thread (its own database connection) for work that takes seconds:
    /// reading another browser's data, a sync round. What pages ask for never waits behind it.
    pub(crate) background: mpsc::Sender<Job>,
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
    /// The open menu: its main rows, its submenus (title, rows) and where it is (x, y, right-aligned).
    pub(crate) menu_main: Vec<serde_json::Value>,
    pub(crate) menu_subs: Vec<(String, Vec<serde_json::Value>)>,
    pub(crate) menu_place: (f64, f64, bool),
    /// Self-test only: windows that would open, as (address, Incognito), instead of starting them.
    pub(crate) window_probe: Option<Vec<(String, bool)>>,
    /// Next id for a pop-up tab (handed out inside the new-window handler).
    pub(crate) next_popup: std::rc::Rc<std::cell::Cell<u64>>,
    /// An extension's toolbar pop-up, open under the extensions button.
    pub(crate) ext_bubble: Option<WebView>,
    /// Where the extensions button is (DIPs: right edge, bottom), the pop-up hangs from it.
    pub(crate) bubble_at: (f64, f64),
    /// The pop-up has DevTools open ("Inspect popup"): losing focus does not close it.
    pub(crate) bubble_inspect: bool,
    /// Which switchable page context-menu items show: (reading mode, translate, Incognito);
    /// shared with every tab's menu, kept current by `apply_settings`.
    pub(crate) page_menu_on: std::rc::Rc<std::cell::Cell<(bool, bool, bool)>>,
    /// New Tab tile hosts whose icon was looked for (once per run).
    pub(crate) site_icons_asked: std::collections::HashSet<String>,
    /// A store extension waiting for the install dialog's answer.
    pub(crate) pending_install: Option<extensions::PendingInstall>,
    pub(crate) downloads_visible: bool,
    /// Loaded toolbar icon of each pinned extension ("" = none, the puzzle glyph shows).
    pub(crate) pinned_icons: std::cell::RefCell<std::collections::HashMap<String, String>>,
    /// The password manager's field focus and the login waiting to be saved.
    pub(crate) passwords: passwords::PasswordState,
    /// The tab whose page is in full screen (a video), and whether that made the window full
    /// screen (false: it already was, by F11, and stays so afterwards).
    pub(crate) page_fullscreen: Option<(u64, bool)>,
    /// The extension page last opened as the New Tab page (its address is not shown, like Chrome).
    pub(crate) new_tab_page: Option<String>,
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
    // Full screen (F11, or a page's video) shows the page alone, like Chrome.
    if window.is_decorated() || window.fullscreen().is_some() { 0 } else { (ui::CHROME_HEIGHT_DIP * window.scale_factor()).round() as u32 }
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
        devtools::quiet_devtools_welcome(&data_dir);
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
        let subpopup = platform::popup::Popup::create(window.hwnd(), move |cmd| {
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
            installed_app: None,
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
            subpopup,
            menu_sub: None,
            page,
            native: page_host::NativeState::default(),
            menu_at: (0.0, 0.0),
            overlay_kind: String::new(),
            overlay_open: false,
            bookmarked: false,
            search: ui::search_template(""),
            engine_setting: String::new(),
            memory_mode: memory::Mode::Balanced,
            previews: Default::default(),
            compact_idle: std::env::var_os("KAKANGKUNG_MEMORY_COMPACT").is_some(),
            page_bg,
            settings: Settings::read(|_| None),
            shown: false,
            downloads_folder: None,
            clear_at_start: false,
            opened: Instant::now(),
            update: crate::update::Status::Idle,
            update_dir: PathBuf::new(),
            account_name: String::new(),
            account_picture: String::new(),
            devtools: None,
            devtools_tab: 0,
            devtools_ready: false,
            devtools_side: "bottom",
            devtools_share: 0.4,
            devtools_dock: None,
            suggest_state: (0, None, vec![]),
            background: jobs.clone(),
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
            menu_main: vec![],
            menu_subs: vec![],
            menu_place: (0.0, 0.0, false),
            window_probe: None,
            next_popup: std::rc::Rc::new(std::cell::Cell::new(FIRST_POPUP_ID)),
            ext_bubble: None,
            bubble_at: (0.0, 0.0),
            bubble_inspect: false,
            page_menu_on: std::rc::Rc::new(std::cell::Cell::new((true, true, true))),
            site_icons_asked: Default::default(),
            pending_install: None,
            downloads_visible: false,
            pinned_icons: Default::default(),
            passwords: Default::default(),
            page_fullscreen: None,
            new_tab_page: None,
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
            app: self.installed_app.clone(),
            parent: self.window.hwnd(),
            bounds: page_bounds(&self.window),
            next_popup: self.next_popup.clone(),
        }
    }

    pub(crate) fn build_tab_webview(&mut self, id: u64, url: &str, visible: bool, private: bool) -> wry::Result<WebView> {
        let kit = self.tab_kit();
        // WebView2 started early while the address bar was typed in (see `prewarm_webview2`).
        if self.env.is_none() {
            self.env = platform::take_prewarmed_environment();
        }
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
        if let Err(error) = platform::prepare_adblock_script(&wv, Some(url), self.settings.protection.adblock, &self.settings.adblock_allowlist) {
            eprintln!("tab {id}: ad blocker: {error}");
        }
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
    app: Option<crate::webapps::WebApp>,
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
        let app = self.app.clone(); let navigation = self.proxy.clone();
        builder.with_navigation_handler(move |url| {
            // A web page linked to a native page: nothing to load, the tab shows it natively.
            if crate::native::page_of(&url).is_some() {
                let _ = navigation.send_event(Msg::Load { id, url, done: true });
                return false;
            }
            if let Some(app) = &app {
                if url != "about:blank" && !app.contains(&url) {
                    if url.starts_with("https://") || url.starts_with("http://") { let _ = navigation.send_event(Msg::OpenExternal(url)); }
                    return false;
                }
            }
            true
        })
            .with_initialization_script(MEDIA_WATCH_JS)
            .with_initialization_script(focus_script(self.focus_highlight))
            .with_initialization_script(STORE_JS)
            .with_initialization_script(PASSWORDS_JS)
            .with_initialization_script(PREVIEW_READY_JS)
            .with_initialization_script(include_str!("../../../../assets/webapps.js"))
            // Pages can only send these messages. A page claiming "live" merely keeps itself
            // from being frozen, which it could also do by playing sound; store messages count
            // only from the Chrome Web Store, and installing still asks the person first.
            .with_ipc_handler(move |req| {
                let body = req.body().as_str();
                if body == "kk:preview-ready" {
                    let _ = p4.send_event(Msg::PreviewReady { id, url: req.uri().to_string() });
                    return;
                }
                if let Some(data) = body.strip_prefix("kk:webapp:") {
                    if data.len() <= 32_768 && !private {
                        let _ = p4.send_event(Msg::WebAppFound { id, url: req.uri().to_string(), data: data.into() });
                    }
                    return;
                }
                // Password manager: the page's own origin comes from the browser (the message's
                // source), never from what the page says.
                if let Some(what) = body.strip_prefix("kk:pw:") {
                    let _ = p4.send_event(Msg::Password { tab: id, url: req.uri().to_string(), what: what.to_string() });
                    return;
                }
                if let Some(cmd) = body.strip_prefix("kk:store:") {
                    if ui::host(&req.uri().to_string()) == crate::webstore::HOST {
                        let _ = p4.send_event(Msg::Store { tab: id, cmd: cmd.to_string() });
                    }
                    return;
                }
                if let Some((frame, live)) = media::media_message(body) {
                    let _ = p4.send_event(Msg::Media { id, frame, live });
                }
            })
            .with_background_color(self.page_bg)
            .with_incognito(private)
            .with_devtools(true)
            .with_theme(self.theme)
            .with_bounds(self.bounds)
            .with_https_scheme(true)
            .with_hotkeys_zoom(true)
            .with_asynchronous_custom_protocol(PROTOCOL.into(), {
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
                if platform::adblock_blocks_popup(&features.opener.webview, &url) { return NewWindowResponse::Deny; }
                if kit.app.is_some() {
                    if url.starts_with("https://") || url.starts_with("http://") { let _ = p3.send_event(Msg::OpenExternal(url)); }
                    return NewWindowResponse::Deny;
                }
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
        let (p1, p2, p3, p4) = (self.proxy.clone(), self.proxy.clone(), self.proxy.clone(), self.proxy.clone());
        let mut hooks = platform::hook(
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
            self.page_menu(id, p4),
        );
        let p = self.proxy.clone();
        platform::hook_security(wv, &mut hooks, move |url, data| { let _ = p.send_event(Msg::Security { id, url, data }); });
        let p = self.proxy.clone();
        platform::hook_fullscreen(wv, &mut hooks, move |on| { let _ = p.send_event(Msg::PageFullscreen { id, on }); });
        hooks
    }

    /// The browser's items in tab `id`'s page context menu (like Chrome's), and where a pick goes.
    /// ponytail: labels are fixed when the tab is made; a language change shows on new tabs.
    fn page_menu(&self, id: u64, proxy: tao::event_loop::EventLoopProxy<Msg>) -> platform::PageMenu {
        use platform::{MenuPlace::*, PageMenuItem};
        let item = |cmd, label: &str, place| PageMenuItem { cmd, label: crate::i18n::tr(label).into_owned(), place };
        let items = vec![
            item("linktab", "Open link in new tab", Link),
            item("linkincognito", "Open link in Incognito window", Link),
            item("reader", "Open in reading mode", Page),
            item("translate", "Translate page", Page),
            item("viewsource", "View page source", Source),
        ];
        let on = self.page_menu_on.clone();
        let visible = move |cmd: &str| {
            let (reader, translate, incognito) = on.get();
            match cmd {
                "reader" => reader,
                "translate" => translate,
                "linkincognito" => incognito,
                _ => true,
            }
        };
        let pick = move |cmd, link| {
            let _ = proxy.send_event(Msg::PageMenu { id, cmd, link });
        };
        (items, Box::new(visible), Box::new(pick))
    }

    /// An item of tab `id`'s page context menu was picked; `link` is the link under the pointer.
    pub(crate) fn on_page_menu(&mut self, id: u64, cmd: &str, link: String) {
        let page = self.index(id).map(|i| self.tabs[i].view.url.clone()).unwrap_or_default();
        match cmd {
            "inspect" if id == self.active && self.devtools.is_none() => self.toggle_devtools(),
            "linktab" if !link.is_empty() => {
                let at = self.index(id).map_or(self.tabs.len(), |i| i + 1);
                self.open_tab_background(&link, at);
            }
            "linkincognito" if !link.is_empty() => self.open_window(&link, true),
            "reader" | "translate" if id == self.active => {
                self.on_menu(cmd);
            }
            "viewsource" if !page.is_empty() && !page.starts_with("view-source:") => self.open_tab(&format!("view-source:{page}"), None),
            _ => {}
        }
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
        self.page_menu_on.set((settings.reader, settings.translate, settings.incognito || self.incognito));
        platform::configure_adblock_sites(&settings.adblock_allowlist);
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
        for i in 0..self.tabs.len() {
            self.apply(i, tabs::Event::ProtectionChanged);
        }
        self.draw_native();
        self.render();
    }

    /// The New Tab page: the one chosen in Settings, else an extension's (a New Tab extension such
    /// as a start page from the Chrome Web Store), else Kakangkung's own.
    pub(crate) fn new_tab_address(&mut self) -> String {
        if self.settings.new_tab_url != ui::NEWTAB_URL || self.incognito {
            return self.settings.new_tab_url.clone();
        }
        let prefs = self.ctx.data_directory().map(|d| d.join("EBWebView").join("Default").join("Secure Preferences"));
        self.new_tab_page = prefs.and_then(|p| crate::webstore::new_tab_override(&self.paths.extensions, &p));
        self.new_tab_page.clone().unwrap_or_else(|| ui::NEWTAB_URL.into())
    }

    /// A new tab as the settings ask: which page, and where in the strip.
    pub(crate) fn open_new_tab(&mut self) {
        let url = if self.incognito { format!("{}incognito", ui::INTERNAL) } else { self.new_tab_address() };
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
            // The password manager's key: logins saved for this site.
            if let Some(origin) = storage::passwords::origin_of(&url) {
                let n = s.and_then(|s| s.logins_for(&origin).ok()).map_or(0, |l| l.len());
                let _ = p.send_event(Msg::SavedLogins { origin, n });
            }
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
        // Numeric addresses (routers, devices on the network) are left alone: they rarely speak
        // HTTPS and there is no falling back.
        let numeric = ui::numeric_host(ui::host(&url).split(':').next().unwrap_or(""));
        if self.settings.https_first && !numeric && ui::Security::of(&url) == ui::Security::Insecure {
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
        if self.installed_app.is_some() {
            match a {
                Action::FocusOmnibox => { let url = self.tabs.iter().find(|t| t.view.id == self.active).map(|t|t.view.url.clone()).unwrap_or_default(); self.open_window(&url, false); return true; }
                Action::CloseTab => return false,
                Action::NewTab | Action::NewWindow => { self.open_window(ui::NEWTAB_URL, false); return true; }
                Action::TabSearch | Action::NextTab | Action::PrevTab | Action::SelectTab(_) | Action::ReopenClosed => return true,
                _ => {}
            }
        }
        let pos = self.index(self.active).unwrap_or(0);
        let n = self.tabs.len();
        match a {
            Action::MenuCommand(command @ ("zoomin" | "zoomout" | "zoomreset")) => {
                if let Some(wv) = self.active_webview() {
                    let factor = match command { "zoomin" => Some(1.1), "zoomout" => Some(1.0 / 1.1), _ => None };
                    let _ = platform::zoom(wv, factor);
                }
            }
            Action::MenuCommand(command) => return self.on_menu(command),
            Action::ReloadBypass => {
                if let Some(wv) = self.active_webview() { let _ = platform::devtools_call(wv, "Page.reload", r#"{"ignoreCache":true}"#); }
                else { return self.act(Action::Reload); }
            }
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
            Action::OpenFile => {
                let picked = platform::pick_file(self.window.hwnd(), &crate::i18n::tr("Open file"), &crate::i18n::tr("All files"), "*.*");
                if let Some(url) = picked.and_then(|p| ui::file_url(&p.to_string_lossy())) {
                    self.open_after_active(&url);
                }
            }
            Action::SavePage => {
                if let Some(wv) = self.active_webview() {
                    if let Err(e) = platform::save_page_as(wv) {
                        eprintln!("save as: {e}");
                    }
                }
            }
            Action::DevTools => {
                self.toggle_devtools();
            }
            Action::TaskManager => self.open_after_active(&format!("{}taskmanager", ui::INTERNAL)),
            Action::Fullscreen => {
                // F11 during a page's full screen leaves it, like Chrome.
                let page = self.page_fullscreen.is_some();
                self.end_page_fullscreen();
                let full = self.window.fullscreen().is_some();
                if full || !page {
                    self.window.set_fullscreen((!full).then_some(tao::window::Fullscreen::Borderless(None)));
                    self.relayout();
                }
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
            let app_origin = self.installed_app.as_ref().map(|app| format!(" - {}", ui::host(&app.start_url))).unwrap_or_default();
            self.window.set_title(&format!("{title}{app_origin} - {APP}{mode}"));
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
        // An extension's New Tab page shows an empty address bar, like the built-in one.
        let url = if self.new_tab_page.as_deref() == Some(url) { ui::NEWTAB_URL } else { url };
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
            chip: match self.tabs.iter().find(|t| t.view.id == self.active).map(|t| t.connection.security(url)).unwrap_or_else(|| ui::Security::of(url)) {
                ui::Security::Secure => Chip::Secure,
                ui::Security::Insecure => Chip::Insecure,
                ui::Security::Local => Chip::Local,
                ui::Security::Internal => Chip::Internal,
            },
            bookmarked: self.bookmarked,
            installable: self.tabs.iter().any(|t| t.view.id == self.active && t.web_app.is_some() && !t.view.private),
            show_star: set.bookmark_star && storage::storable(url),
            key: self.key_shown(),
            show_home: set.home_button,
            show_extensions: set.extensions_button && !self.incognito,
            show_tab_search: set.tab_search_button,
            pinned: self.pinned_buttons(),
            avatar: self.account_name.trim().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default(),
            avatar_image: if self.incognito { String::new() } else { self.account_picture.clone() },
            synced: self.synced,
            maximized: self.window.is_maximized(),
            tab_search_open: self.overlay_open && self.overlay_kind == "tabsearch",
        }
    }

    pub(crate) fn relayout(&mut self) {
        self.hide_overlay();
        let size = self.window.inner_size();
        self.frame.set_bounds(0, 0, size.width as i32, chrome_height(&self.window) as i32);
        self.place_devtools();
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

    /// Answer an API request with slow work (seconds: another browser's data, a sync round),
    /// done on the background storage thread so pages and the address bar are not held up.
    pub(crate) fn db_reply_slow(&self, respond: Reply, f: impl FnOnce(&Store) -> Json + Send + 'static) {
        let job: Job = Box::new(move |s| {
            respond.send(match s {
                Some(s) => f(s),
                None => json!({ "error": "the history database could not be opened" }),
            })
        });
        if let Err(mpsc::SendError(job)) = self.background.send(job) {
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
